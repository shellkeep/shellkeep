// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! shellkeep — SSH terminal manager.
//!
//! Persistent sessions that survive everything.
//! Open source. Cross-platform. Zero server setup.

mod theme;

use iced::futures::stream::BoxStream;
use iced::futures::{SinkExt, StreamExt};
use iced::keyboard;
use iced::widget::{
    Space, button, center, column, container, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use iced_term::ColorPalette;
use iced_term::settings::{BackendSettings, FontSettings, Settings, ThemeSettings};
use shellkeep::config::Config;
use shellkeep::ssh;
use shellkeep::ssh::manager::{ConnKey, ConnectionManager};
use shellkeep::state::recent::{RecentConnection, RecentConnections};
use shellkeep::state::state_file::{StateFile, TabState};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::sync::Mutex;

const RENAME_INPUT_ID: &str = "rename-tab-input";

/// Shared holder for a value that is take()n by the SSH subscription on first run.
type Holder<T> = Arc<Mutex<Option<T>>>;
type ChannelHolder = Holder<russh::Channel<russh::client::Msg>>;
type WriterRxHolder = Holder<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>;
type ResizeRxHolder = Holder<tokio::sync::mpsc::UnboundedReceiver<(u32, u32)>>;

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();

    // Handle --version and --help before initializing anything
    for arg in &args[1..] {
        match arg.as_str() {
            "--crash-report" => {
                let dir = shellkeep::crash::crash_dir();
                if dir.exists() {
                    match std::fs::read_dir(&dir) {
                        Ok(entries) => {
                            let mut files: Vec<_> = entries
                                .filter_map(|e| e.ok())
                                .filter(|e| e.path().extension().is_some_and(|ext| ext == "txt"))
                                .collect();
                            files.sort_by_key(|e| e.path());
                            if files.is_empty() {
                                println!("No crash dumps found.");
                            } else {
                                println!("Crash dumps in {}:", dir.display());
                                for f in &files {
                                    println!("  {}", f.path().display());
                                }
                                // Show the latest one
                                if let Some(latest) = files.last() {
                                    println!(
                                        "\nLatest:\n{}",
                                        std::fs::read_to_string(latest.path()).unwrap_or_default()
                                    );
                                }
                            }
                        }
                        Err(_) => println!("No crash dumps found."),
                    }
                } else {
                    println!("No crash dumps found.");
                }
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("shellkeep {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "shellkeep {} — SSH sessions that survive everything\n\n\
                     Usage: shellkeep [user@]host [-p port] [-i identity] [-l user]\n\
                     \n\
                     Options:\n  \
                       -p PORT          SSH port (default: 22)\n  \
                       -i FILE          Identity file (private key)\n  \
                       -l USER          Login user name\n  \
                       --debug          Enable debug logging\n  \
                       --crash-report   Show crash dumps from previous runs\n  \
                       --version        Show version\n  \
                       --help           Show this help\n\
                     \n\
                     Without arguments, opens the welcome screen.\n\
                     https://github.com/shellkeep/shellkeep",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }

    let log_level = if args.iter().any(|a| a == "--trace") {
        "trace"
    } else if args.iter().any(|a| a == "--debug") {
        "debug"
    } else {
        "info"
    };

    // Set up logging — stderr + optional file
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    // Try to also log to file
    let log_dir = dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("shellkeep")
        .join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("shellkeep.log");

    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false);
        let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .with(stderr_layer)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    // Parse SSH args (skip --debug which is shellkeep-specific)
    let ssh_relevant: Vec<String> = args[1..]
        .iter()
        .filter(|a| *a != "--debug" && *a != "--trace")
        .cloned()
        .collect();

    let initial_ssh_args: Option<Vec<String>> =
        if ssh_relevant.is_empty() || ssh_relevant.iter().all(|a| a.starts_with('-')) {
            None
        } else {
            Some(ssh_relevant)
        };

    tracing::info!("shellkeep v{} starting", env!("CARGO_PKG_VERSION"));

    // NFR-SEC-10: disable core dumps
    shellkeep::crash::disable_core_dumps();

    // NFR-OBS-09: install crash handler
    shellkeep::crash::install_panic_hook();

    // NFR-SEC-03: verify and fix file permissions on startup
    shellkeep::state::permissions::verify_and_fix();

    iced::application(
        move || ShellKeep::new(initial_ssh_args.clone()),
        ShellKeep::update,
        ShellKeep::view,
    )
    .title(ShellKeep::title)
    .subscription(ShellKeep::subscription)
    .theme(ShellKeep::theme)
    .window_size((900.0, 600.0))
    .antialiasing(true)
    .run()
}

// ---------------------------------------------------------------------------
// Tab
// ---------------------------------------------------------------------------

/// Connection parameters parsed from user input.
#[derive(Clone, Debug)]
struct ConnParams {
    host: String,
    port: u16,
    username: String,
    identity_file: Option<String>,
}

struct Tab {
    id: u64,
    label: String,
    terminal: Option<iced_term::Terminal>,
    /// Legacy: system ssh args (kept for compatibility during transition)
    ssh_args: Vec<String>,
    conn_params: Option<ConnParams>,
    tmux_session: String,
    dead: bool,
    reconnect_attempts: u32,
    auto_reconnect: bool,
    /// Whether this tab uses russh (true) or system ssh (false)
    uses_russh: bool,
    // russh channel holder — taken by the subscription on first run
    ssh_channel_holder: Option<ChannelHolder>,
    // Writer rx holder — keyboard input receiver, taken by subscription
    ssh_writer_rx_holder: Option<WriterRxHolder>,
    // Resize command sender
    ssh_resize_tx: Option<tokio::sync::mpsc::UnboundedSender<(u32, u32)>>,
    // Resize rx holder — taken by subscription
    ssh_resize_rx_holder: Option<ResizeRxHolder>,
    #[allow(dead_code)]
    conn_key: Option<ConnKey>,
    /// Holder for a channel being established by the async task.
    /// Moved to ssh_channel_holder when SshConnected(Ok) arrives.
    pending_channel: Option<ChannelHolder>,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct ShellKeep {
    tabs: Vec<Tab>,
    active_tab: usize,
    next_id: u64,
    show_welcome: bool,
    renaming_tab: Option<usize>,
    rename_input: String,
    current_font_size: f32,
    context_menu: Option<(f32, f32)>,
    tab_context_menu: Option<(usize, f32, f32)>,
    /// Toast message (auto-dismisses)
    toast: Option<(String, std::time::Instant)>,
    /// Current connection params (for russh control connection)
    current_conn: Option<ConnParams>,
    /// Client identifier for state persistence
    client_id: String,
    /// Shared SSH connection manager
    conn_manager: Arc<Mutex<ConnectionManager>>,

    // Welcome screen state
    host_input: String,
    port_input: String,
    user_input: String,
    identity_input: String,

    config: Config,
    recent: RecentConnections,
    title_text: String,
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

// (no large bundle structs — channel is passed via Arc<Mutex<Option<>>> holders)

#[derive(Debug, Clone)]
enum Message {
    TerminalEvent(iced_term::Event),
    SshData(u64, Vec<u8>),
    SshDisconnected(u64, String),
    SshConnected(u64, Result<(), String>),
    #[allow(dead_code)]
    SshSessionsListed(Result<(), String>),
    SelectTab(usize),
    CloseTab(usize),
    NewTab,
    ReconnectTab(usize),
    AutoReconnectTick,
    ContextMenuCopy,
    ContextMenuPaste,
    ContextMenuDismiss,
    TabContextMenu(usize, f32, f32),
    TabMoveLeft(usize),
    TabMoveRight(usize),
    StartRename(usize),
    ConnectRecent(usize),
    RenameInputChanged(String),
    FinishRename,
    ToastDismiss,
    HostInputChanged(String),
    PortInputChanged(String),
    UserInputChanged(String),
    IdentityInputChanged(String),
    Connect,
    KeyEvent(keyboard::Event),
}

// ---------------------------------------------------------------------------
// SSH subscription
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct SshSubscriptionData {
    tab_id: u64,
    channel: ChannelHolder,
    writer_rx: WriterRxHolder,
    resize_rx: ResizeRxHolder,
}

impl Hash for SshSubscriptionData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tab_id.hash(state);
    }
}

fn ssh_channel_stream(data: &SshSubscriptionData) -> BoxStream<'static, Message> {
    let tab_id = data.tab_id;
    let channel_holder = data.channel.clone();
    let writer_rx_holder = data.writer_rx.clone();
    let resize_rx_holder = data.resize_rx.clone();

    iced::stream::channel(1000, async move |mut output| {
        // Take ownership of channel, writer_rx, resize_rx from holders.
        // These are only taken once — subsequent subscription recreations see None
        // but iced keeps the existing stream running (matched by hash).
        let mut channel = match channel_holder.lock().await.take() {
            Some(ch) => ch,
            None => {
                // Stream already running or channel gone — keep alive silently
                iced::futures::future::pending::<()>().await;
                return;
            }
        };
        let mut writer_rx = match writer_rx_holder.lock().await.take() {
            Some(rx) => rx,
            None => {
                iced::futures::future::pending::<()>().await;
                return;
            }
        };
        let mut resize_rx = match resize_rx_holder.lock().await.take() {
            Some(rx) => rx,
            None => {
                iced::futures::future::pending::<()>().await;
                return;
            }
        };

        tracing::info!("ssh stream {tab_id}: started");

        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(russh::ChannelMsg::Data { data }) => {
                            let _ = output.send(Message::SshData(tab_id, data.to_vec())).await;
                        }
                        Some(russh::ChannelMsg::Eof) | None => {
                            tracing::info!("ssh stream {tab_id}: channel closed");
                            let _ = output.send(
                                Message::SshDisconnected(tab_id, "channel closed".into())
                            ).await;
                            break;
                        }
                        _ => {}
                    }
                }
                Some(input) = writer_rx.recv() => {
                    if let Err(e) = channel.data(&input[..]).await {
                        tracing::warn!("ssh stream {tab_id}: write error: {e}");
                        let _ = output.send(
                            Message::SshDisconnected(tab_id, format!("write error: {e}"))
                        ).await;
                        break;
                    }
                }
                Some((cols, rows)) = resize_rx.recv() => {
                    if let Err(e) = channel.window_change(cols, rows, 0, 0).await {
                        tracing::warn!("ssh stream {tab_id}: resize error: {e}");
                    }
                }
            }
        }

        // Keep the future alive so iced doesn't restart the stream
        iced::futures::future::pending::<()>().await;
    })
    .boxed()
}

// ---------------------------------------------------------------------------
// Async SSH operations
// ---------------------------------------------------------------------------

/// Establish an SSH session: connect, create tmux session, open PTY channel.
/// Returns the raw russh Channel on success.
async fn establish_ssh_session(
    conn_manager: Arc<Mutex<ConnectionManager>>,
    conn: ConnParams,
    tmux_session: String,
    cols: u32,
    rows: u32,
) -> Result<russh::Channel<russh::client::Msg>, String> {
    let conn_key = ConnKey {
        host: conn.host.clone(),
        port: conn.port,
        username: conn.username.clone(),
    };

    let handle_arc = {
        let mut mgr = conn_manager.lock().await;
        mgr.get_or_connect(&conn_key, conn.identity_file.as_deref())
            .await
            .map_err(|e| e.to_string())?
    };

    let handle = handle_arc.lock().await;

    // Create tmux session (idempotent — no error if already exists)
    ssh::tmux::create_session_russh(&handle, &tmux_session)
        .await
        .map_err(|e| e.to_string())?;

    // Open PTY channel and attach to tmux session
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("channel open: {e}"))?;

    channel
        .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
        .await
        .map_err(|e| format!("pty: {e}"))?;

    let tmux_cmd = format!(
        "TERM=xterm-256color tmux new-session -A -s {tmux_session} \\; set status off || exec $SHELL"
    );
    channel
        .exec(true, tmux_cmd)
        .await
        .map_err(|e| format!("exec: {e}"))?;

    Ok(channel)
}

// (connect_and_list_sessions removed — CLI launch uses system ssh for now)

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

impl ShellKeep {
    fn new(initial_ssh_args: Option<Vec<String>>) -> (Self, Task<Message>) {
        let username = whoami::username();
        let config = Config::load();
        let recent = RecentConnections::load();
        let default_port = config.ssh.default_port.to_string();
        let mut app = ShellKeep {
            tabs: Vec::new(),
            active_tab: 0,
            next_id: 0,
            show_welcome: false,
            renaming_tab: None,
            rename_input: String::new(),
            current_font_size: config.terminal.font_size,
            context_menu: None,
            tab_context_menu: None,
            toast: None,
            current_conn: None,
            client_id: shellkeep::state::client_id::resolve(config.general.client_id.as_deref()),
            conn_manager: Arc::new(Mutex::new(ConnectionManager::new())),
            host_input: String::new(),
            port_input: default_port,
            user_input: username,
            identity_input: String::new(),
            config,
            recent,
            title_text: "shellkeep".to_string(),
            error: None,
        };

        if let Some(ssh_args) = initial_ssh_args {
            // Parse connection params from CLI args
            let host_arg = ssh_args
                .iter()
                .find(|a| !a.starts_with('-'))
                .cloned()
                .unwrap_or_default();
            let label = host_arg.clone();
            let (parsed_user, parsed_host, parsed_port) = parse_host_input(&host_arg);
            let mut cli_port = "22".to_string();
            let mut cli_identity = None;
            let mut i = 0;
            while i < ssh_args.len() {
                match ssh_args[i].as_str() {
                    "-p" if i + 1 < ssh_args.len() => {
                        cli_port = ssh_args[i + 1].clone();
                        i += 1;
                    }
                    "-i" if i + 1 < ssh_args.len() => {
                        cli_identity = Some(ssh_args[i + 1].clone());
                        i += 1;
                    }
                    _ => {}
                }
                i += 1;
            }
            app.current_conn = Some(ConnParams {
                host: parsed_host,
                port: parsed_port
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(cli_port.parse().unwrap_or(22)),
                username: parsed_user.unwrap_or_else(whoami::username),
                identity_file: cli_identity,
            });

            // CLI launch: use system ssh for immediate feedback
            // (russh is used for interactive Connect button flow)
            let ssh_args_vec = app.build_ssh_args_from_conn(app.current_conn.as_ref().unwrap());
            let existing = ssh::tmux::list_remote_sessions(&ssh_args_vec);
            let saved_state = StateFile::load_local(&StateFile::local_cache_path(&app.client_id));

            if existing.is_empty() {
                app.open_tab_with_tmux(&ssh_args_vec, &label);
            } else {
                tracing::info!(
                    "found {} existing tmux session(s): {:?}",
                    existing.len(),
                    existing
                );
                for (i, session_name) in existing.iter().enumerate() {
                    let tab_label = saved_state
                        .as_ref()
                        .and_then(|s| {
                            s.tabs
                                .iter()
                                .find(|t| t.tmux_session_name == *session_name)
                                .map(|t| t.title.clone())
                        })
                        .unwrap_or_else(|| {
                            if i == 0 {
                                label.clone()
                            } else {
                                format!("Session {}", i + 1)
                            }
                        });
                    app.open_tab_with_tmux_session(&ssh_args_vec, &tab_label, session_name);
                }
            }
        } else {
            app.show_welcome = true;
        }

        (app, Task::none())
    }

    /// Build ssh args from ConnParams (for system ssh fallback).
    fn build_ssh_args_from_conn(&self, conn: &ConnParams) -> Vec<String> {
        let mut args = Vec::new();
        if conn.username.is_empty() {
            args.push(conn.host.clone());
        } else {
            args.push(format!("{}@{}", conn.username, conn.host));
        }
        if conn.port != 22 {
            args.push("-p".to_string());
            args.push(conn.port.to_string());
        }
        if let Some(ref id_file) = conn.identity_file {
            args.push("-i".to_string());
            args.push(id_file.clone());
        }
        args
    }

    /// Open a new tab, assigning it the next unused tmux session name.
    fn next_tmux_session(&self) -> String {
        let max_existing = self
            .tabs
            .iter()
            .filter_map(|t| {
                t.tmux_session
                    .strip_prefix("shellkeep-")
                    .and_then(|n| n.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0);
        let session_num = max_existing.max(self.next_id);
        format!("shellkeep-{session_num}")
    }

    /// Open a tab using russh SSH. Returns a Task that establishes the connection.
    fn open_tab_russh(&mut self, label: &str, tmux_session: &str) -> Task<Message> {
        let conn = match &self.current_conn {
            Some(c) => c.clone(),
            None => {
                self.error = Some("No connection parameters available".into());
                return Task::none();
            }
        };

        let id = self.next_id;
        self.next_id += 1;

        // Create channels for SSH I/O
        let (ssh_writer_tx, ssh_writer_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, resize_rx) = tokio::sync::mpsc::unbounded_channel::<(u32, u32)>();

        let settings = Settings {
            font: FontSettings {
                size: self.config.terminal.font_size,
                ..FontSettings::default()
            },
            theme: ThemeSettings {
                color_pallete: Box::new(catppuccin_mocha()),
            },
            backend: BackendSettings::default(),
        };

        let terminal = match iced_term::Terminal::new_ssh(id, settings, ssh_writer_tx) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("failed to create SSH terminal: {e}");
                self.error = Some(format!("Terminal creation failed: {e}"));
                return Task::none();
            }
        };

        // Pre-allocate holders for the subscription to take() on first run.
        // The async task will write the channel into channel_holder.
        let writer_rx_holder = Arc::new(Mutex::new(Some(ssh_writer_rx)));
        let resize_rx_holder = Arc::new(Mutex::new(Some(resize_rx)));
        let channel_holder: ChannelHolder = Arc::new(Mutex::new(None));

        let ssh_args = self
            .current_conn
            .as_ref()
            .map(|c| self.build_ssh_args_from_conn(c))
            .unwrap_or_default();

        self.tabs.push(Tab {
            id,
            label: label.to_string(),
            terminal: Some(terminal),
            ssh_args,
            conn_params: self.current_conn.clone(),
            tmux_session: tmux_session.to_string(),
            dead: false,
            reconnect_attempts: 0,
            auto_reconnect: true,
            uses_russh: true,
            ssh_channel_holder: None, // set when SshConnected(Ok) arrives
            ssh_writer_rx_holder: Some(writer_rx_holder),
            ssh_resize_tx: Some(resize_tx),
            ssh_resize_rx_holder: Some(resize_rx_holder),
            conn_key: None,
            pending_channel: Some(channel_holder.clone()),
        });
        self.active_tab = self.tabs.len() - 1;
        self.error = None;
        self.update_title();
        self.save_state();
        tracing::info!("opened SSH tab {id}: {label} (tmux: {tmux_session}) via russh");

        // Launch async connection — writes channel into the pre-allocated holder
        let mgr = self.conn_manager.clone();
        let tmux = tmux_session.to_string();
        let holder = channel_holder;
        Task::perform(
            async move {
                match establish_ssh_session(mgr, conn, tmux, 80, 24).await {
                    Ok(channel) => {
                        *holder.lock().await = Some(channel);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            },
            move |result: Result<(), String>| Message::SshConnected(id, result),
        )
    }

    /// Open a tab using system ssh + PTY (legacy path, used for CLI launch).
    fn open_tab_with_tmux(&mut self, ssh_args: &[String], label: &str) {
        let tmux_session = self.next_tmux_session();
        self.open_tab_with_tmux_session(ssh_args, label, &tmux_session);
    }

    fn open_tab_with_tmux_session(&mut self, ssh_args: &[String], label: &str, tmux_session: &str) {
        let id = self.next_id;
        self.next_id += 1;

        let tmux_cmd = format!(
            "TERM=xterm-256color tmux new-session -A -s {tmux_session} \\; set status off || exec $SHELL"
        );

        let mut full_args = Vec::new();
        full_args.extend_from_slice(ssh_args);
        full_args.push("-t".to_string());
        full_args.push(tmux_cmd);

        let settings = Settings {
            font: FontSettings {
                size: self.config.terminal.font_size,
                ..FontSettings::default()
            },
            theme: ThemeSettings {
                color_pallete: Box::new(catppuccin_mocha()),
            },
            backend: BackendSettings {
                program: "ssh".to_string(),
                args: full_args,
                ..Default::default()
            },
        };

        match iced_term::Terminal::new(id, settings) {
            Ok(terminal) => {
                self.tabs.push(Tab {
                    id,
                    label: label.to_string(),
                    terminal: Some(terminal),
                    ssh_args: ssh_args.to_vec(),
                    conn_params: self.current_conn.clone(),
                    tmux_session: tmux_session.to_string(),
                    dead: false,
                    reconnect_attempts: 0,
                    auto_reconnect: true,
                    uses_russh: false,
                    ssh_channel_holder: None,
                    ssh_writer_rx_holder: None,
                    ssh_resize_tx: None,
                    ssh_resize_rx_holder: None,
                    conn_key: None,
                    pending_channel: None,
                });
                self.active_tab = self.tabs.len() - 1;
                self.error = None;
                self.update_title();
                self.save_state();
                tracing::info!("opened tab {id}: {label} (tmux: {tmux_session})");
            }
            Err(e) => {
                tracing::error!("failed to create terminal: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            let tab = self.tabs.remove(index);
            tracing::info!("closed tab {}: {}", tab.id, tab.label);
            if self.active_tab >= self.tabs.len() && self.active_tab > 0 {
                self.active_tab -= 1;
            }
            self.update_title();
            self.save_state();
            // Toast notification
            if !tab.dead {
                self.toast = Some((
                    "Session kept on server — you can restore it later".into(),
                    std::time::Instant::now(),
                ));
            }
        }
    }

    fn reconnect_tab(&mut self, index: usize) -> Task<Message> {
        if index >= self.tabs.len() {
            return Task::none();
        }

        let tab = &mut self.tabs[index];

        if tab.uses_russh {
            // Russh reconnection: clear old state, create new terminal, launch connection
            tab.ssh_channel_holder = None;
            tab.ssh_resize_tx = None;
            tab.pending_channel = None;

            let (ssh_writer_tx, ssh_writer_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
            let (resize_tx, resize_rx) = tokio::sync::mpsc::unbounded_channel::<(u32, u32)>();
            let channel_holder: ChannelHolder = Arc::new(Mutex::new(None));

            let settings = Settings {
                font: FontSettings {
                    size: self.current_font_size,
                    ..FontSettings::default()
                },
                theme: ThemeSettings {
                    color_pallete: Box::new(catppuccin_mocha()),
                },
                backend: BackendSettings::default(),
            };

            match iced_term::Terminal::new_ssh(tab.id, settings, ssh_writer_tx) {
                Ok(terminal) => {
                    tab.terminal = Some(terminal);
                    tab.ssh_writer_rx_holder = Some(Arc::new(Mutex::new(Some(ssh_writer_rx))));
                    tab.ssh_resize_tx = Some(resize_tx);
                    tab.ssh_resize_rx_holder = Some(Arc::new(Mutex::new(Some(resize_rx))));
                    tab.pending_channel = Some(channel_holder.clone());
                    tab.dead = false;

                    let conn = match &tab.conn_params {
                        Some(c) => c.clone(),
                        None => return Task::none(),
                    };
                    let tmux = tab.tmux_session.clone();
                    let mgr = self.conn_manager.clone();
                    let tab_id = tab.id;
                    let holder = channel_holder;
                    self.update_title();

                    Task::perform(
                        async move {
                            match establish_ssh_session(mgr, conn, tmux, 80, 24).await {
                                Ok(channel) => {
                                    *holder.lock().await = Some(channel);
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            }
                        },
                        move |result: Result<(), String>| Message::SshConnected(tab_id, result),
                    )
                }
                Err(e) => {
                    tracing::error!("failed to create terminal for reconnect: {e}");
                    Task::none()
                }
            }
        } else {
            // System ssh reconnection (legacy)
            let ssh_args = tab.ssh_args.clone();
            let label = tab.label.clone();
            let tmux_session = tab.tmux_session.clone();

            self.tabs.remove(index);
            self.open_tab_with_tmux_session(&ssh_args, &label, &tmux_session);

            if self.tabs.len() > 1 && index < self.tabs.len() - 1 {
                let tab = self.tabs.pop().unwrap();
                self.tabs.insert(index, tab);
                self.active_tab = index;
                self.update_title();
            }
            Task::none()
        }
    }

    fn apply_font_to_all_tabs(&mut self) {
        let font_settings = FontSettings {
            size: self.current_font_size,
            ..FontSettings::default()
        };
        for tab in &mut self.tabs {
            if let Some(ref mut terminal) = tab.terminal {
                terminal.handle(iced_term::Command::ChangeFont(font_settings.clone()));
            }
        }
        tracing::debug!("font size: {}", self.current_font_size);
    }

    fn save_state(&self) {
        let mut state = StateFile::new(&self.client_id);
        for (i, tab) in self.tabs.iter().enumerate() {
            state.tabs.push(TabState {
                session_uuid: format!("tab-{}", tab.id),
                tmux_session_name: tab.tmux_session.clone(),
                title: tab.label.clone(),
                position: i,
            });
        }
        let path = StateFile::local_cache_path(&self.client_id);
        if let Err(e) = state.save_local(&path) {
            tracing::warn!("failed to save state: {e}");
        } else {
            tracing::debug!("state saved to {}", path.display());
        }
    }

    fn update_title(&mut self) {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            let status = if tab.dead { " (disconnected)" } else { "" };
            self.title_text = format!("shellkeep — {}{}", tab.label, status);
        } else {
            self.title_text = "shellkeep".to_string();
        }
    }

    fn build_ssh_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let host = self.host_input.trim();

        // Parse user@host:port from host field
        let (parsed_user, parsed_host, parsed_port) = parse_host_input(host);

        let user = if !self.user_input.is_empty() {
            self.user_input.clone()
        } else {
            parsed_user.unwrap_or_default()
        };

        let host = parsed_host;
        let port = parsed_port.unwrap_or_else(|| self.port_input.trim().to_string());

        if !user.is_empty() {
            args.push(format!("{user}@{host}"));
        } else {
            args.push(host);
        }

        if !port.is_empty() && port != "22" {
            args.push("-p".to_string());
            args.push(port);
        }

        if !self.identity_input.is_empty() {
            args.push("-i".to_string());
            args.push(self.identity_input.clone());
        }

        args
    }

    fn title(&self) -> String {
        self.title_text.clone()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SshData(tab_id, data) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    terminal.process_ssh_data(&data);
                }
            }

            Message::SshDisconnected(tab_id, reason) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    // Clear channel state so subscription stops
                    tab.ssh_channel_holder = None;
                    tab.ssh_resize_tx = None;
                    if tab.auto_reconnect
                        && tab.reconnect_attempts < self.config.ssh.reconnect_max_attempts
                    {
                        tab.reconnect_attempts += 1;
                        tab.terminal = None;
                        tracing::info!("SSH tab {tab_id} disconnected: {reason}, will retry");
                    } else {
                        tab.terminal = None;
                        tab.dead = true;
                        tab.auto_reconnect = false;
                        tracing::info!("SSH tab {tab_id} disconnected: {reason}");
                    }
                    self.update_title();
                }
            }

            Message::SshConnected(tab_id, result) => {
                match result {
                    Ok(()) => {
                        // The async task wrote the channel into pending_channel.
                        // Move it to ssh_channel_holder so the subscription picks it up.
                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                            && let Some(holder) = tab.pending_channel.take()
                        {
                            tab.ssh_channel_holder = Some(holder);
                            tracing::info!("SSH tab {tab_id}: connected, channel ready");
                        }
                    }
                    Err(e) => {
                        tracing::error!("SSH tab {tab_id}: connection failed: {e}");
                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                            tab.pending_channel = None;
                            tab.terminal = None;
                            tab.dead = true;
                            tab.auto_reconnect = true;
                            tab.reconnect_attempts += 1;
                        }
                        self.error = Some(format!("Connection failed: {e}"));
                        self.update_title();
                    }
                }
            }

            Message::SshSessionsListed(_result) => {
                // Handled by the connect flow — sessions are opened in the handler
            }

            Message::TerminalEvent(iced_term::Event::ContextMenu(_id, x, y)) => {
                self.context_menu = Some((x, y));
                self.renaming_tab = None;
                self.tab_context_menu = None;
            }

            Message::ContextMenuCopy => {
                self.context_menu = None;
                if let Some(tab) = self.tabs.get_mut(self.active_tab)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    terminal.handle(iced_term::Command::ProxyToBackend(
                        iced_term::BackendCommand::ProcessAlacrittyEvent(
                            iced_term::AlacrittyEvent::PtyWrite(String::new()),
                        ),
                    ));
                }
            }

            Message::ContextMenuPaste => {
                self.context_menu = None;
            }

            Message::ToastDismiss => {
                self.toast = None;
            }

            Message::ContextMenuDismiss => {
                self.context_menu = None;
                self.tab_context_menu = None;
                self.renaming_tab = None;
            }

            Message::TabContextMenu(index, x, y) => {
                self.tab_context_menu = Some((index, x, y));
                self.context_menu = None;
            }

            Message::StartRename(index) => {
                self.tab_context_menu = None;
                if index < self.tabs.len() {
                    self.active_tab = index;
                    self.rename_input = self.tabs[index].label.clone();
                    self.renaming_tab = Some(index);
                    return Task::batch([
                        iced_runtime::widget::operation::focus(RENAME_INPUT_ID),
                        iced_runtime::widget::operation::select_all(RENAME_INPUT_ID),
                    ]);
                }
            }

            Message::TabMoveLeft(index) => {
                self.tab_context_menu = None;
                if index > 0 && index < self.tabs.len() {
                    self.tabs.swap(index, index - 1);
                    if self.active_tab == index {
                        self.active_tab -= 1;
                    } else if self.active_tab == index - 1 {
                        self.active_tab += 1;
                    }
                }
            }

            Message::TabMoveRight(index) => {
                self.tab_context_menu = None;
                if index + 1 < self.tabs.len() {
                    self.tabs.swap(index, index + 1);
                    if self.active_tab == index {
                        self.active_tab += 1;
                    } else if self.active_tab == index + 1 {
                        self.active_tab -= 1;
                    }
                }
            }

            Message::TerminalEvent(iced_term::Event::BackendCall(id, cmd)) => {
                let is_resize = matches!(&cmd, iced_term::BackendCommand::Resize(..));
                let mut needs_title_update = false;
                let mut shutdown = false;
                let mut resize_info: Option<(u32, u32)> = None;

                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    let action = terminal.handle(iced_term::Command::ProxyToBackend(cmd));
                    match action {
                        iced_term::actions::Action::ChangeTitle(new_title) => {
                            tab.label = new_title;
                            needs_title_update = true;
                        }
                        iced_term::actions::Action::Shutdown => {
                            shutdown = true;
                            needs_title_update = true;
                        }
                        _ => {}
                    }

                    // Collect resize info before dropping the terminal borrow
                    if is_resize && tab.uses_russh && !shutdown {
                        let (cols, rows) = terminal.terminal_size();
                        if cols > 0 && rows > 0 {
                            resize_info = Some((cols as u32, rows as u32));
                        }
                    }
                }

                // Handle shutdown after terminal borrow is released
                if shutdown && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                    tab.terminal = None;
                    if tab.auto_reconnect
                        && tab.reconnect_attempts < self.config.ssh.reconnect_max_attempts
                    {
                        tab.reconnect_attempts += 1;
                        tracing::info!(
                            "tab {id} disconnected, will auto-reconnect (attempt {})",
                            tab.reconnect_attempts
                        );
                    } else {
                        tab.dead = true;
                        tab.auto_reconnect = false;
                        tracing::info!("tab {id} session ended (no more retries)");
                    }
                }

                // Propagate resize to SSH channel
                if let Some((cols, rows)) = resize_info
                    && let Some(tab) = self.tabs.iter().find(|t| t.id == id)
                    && let Some(ref resize_tx) = tab.ssh_resize_tx
                {
                    let _ = resize_tx.send((cols, rows));
                }

                if needs_title_update {
                    self.update_title();
                }
            }

            Message::SelectTab(index) => {
                if index < self.tabs.len() {
                    self.active_tab = index;
                    self.show_welcome = false;
                    self.renaming_tab = None;
                    self.tab_context_menu = None;
                    self.update_title();
                }
            }

            Message::CloseTab(index) => {
                self.close_tab(index);
            }

            Message::NewTab => {
                if self.current_conn.is_some() {
                    let n = self.tabs.len() + 1;
                    let label = format!("Session {n}");
                    let tmux_session = self.next_tmux_session();
                    return self.open_tab_russh(&label, &tmux_session);
                } else if let Some(tab) = self.tabs.last() {
                    // Fallback: use system ssh args from existing tab
                    let ssh_args = tab.ssh_args.clone();
                    let n = self.tabs.len() + 1;
                    let label = format!("Session {n}");
                    self.open_tab_with_tmux(&ssh_args, &label);
                } else {
                    self.show_welcome = true;
                }
            }

            Message::ReconnectTab(index) => {
                if index < self.tabs.len() {
                    self.tabs[index].auto_reconnect = false;
                    self.tabs[index].reconnect_attempts = 0;
                }
                return self.reconnect_tab(index);
            }

            Message::AutoReconnectTick => {
                let reconnect_indices: Vec<usize> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.terminal.is_none() && t.auto_reconnect && !t.dead)
                    .map(|(i, _)| i)
                    .collect();

                if let Some(&index) = reconnect_indices.first() {
                    tracing::info!(
                        "auto-reconnecting tab {} (attempt {})",
                        self.tabs[index].id,
                        self.tabs[index].reconnect_attempts,
                    );
                    return self.reconnect_tab(index);
                }
            }

            Message::RenameInputChanged(v) => {
                self.rename_input = v;
            }

            Message::FinishRename => {
                if let Some(index) = self.renaming_tab
                    && index < self.tabs.len()
                    && !self.rename_input.trim().is_empty()
                {
                    self.tabs[index].label = self.rename_input.trim().to_string();
                    self.update_title();
                    self.save_state();
                }
                self.renaming_tab = None;
            }

            Message::HostInputChanged(v) => {
                self.host_input = v;
            }
            Message::PortInputChanged(v) => self.port_input = v,
            Message::UserInputChanged(v) => self.user_input = v,
            Message::IdentityInputChanged(v) => self.identity_input = v,

            Message::Connect => {
                if self.host_input.trim().is_empty() {
                    return Task::none();
                }
                let ssh_args = self.build_ssh_args();
                let label = ssh_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ssh_args.join(" "));

                // Store connection params
                let (parsed_user, parsed_host, parsed_port) =
                    parse_host_input(self.host_input.trim());
                let conn = ConnParams {
                    host: parsed_host,
                    port: parsed_port
                        .and_then(|p| p.parse().ok())
                        .unwrap_or(self.port_input.trim().parse().unwrap_or(22)),
                    username: if !self.user_input.is_empty() {
                        self.user_input.clone()
                    } else {
                        parsed_user.unwrap_or_else(whoami::username)
                    },
                    identity_file: if self.identity_input.is_empty() {
                        None
                    } else {
                        Some(self.identity_input.clone())
                    },
                };
                self.current_conn = Some(conn);

                self.recent.push(RecentConnection {
                    label: label.clone(),
                    ssh_args: ssh_args.clone(),
                    host: self.host_input.clone(),
                    user: self.user_input.clone(),
                    port: self.port_input.clone(),
                    alias: None,
                    last_connected: None,
                    host_key_fingerprint: None,
                });
                self.recent.save();

                // Use russh for new connections: open tab immediately, connect async
                let tmux_session = self.next_tmux_session();
                self.show_welcome = false;
                return self.open_tab_russh(&label, &tmux_session);
            }

            Message::ConnectRecent(index) => {
                if let Some(conn) = self.recent.connections.get(index).cloned() {
                    self.host_input = conn.host.clone();
                    self.user_input = conn.user.clone();
                    self.port_input = conn.port.clone();
                    self.current_conn = Some(ConnParams {
                        host: conn.host,
                        port: conn.port.parse().unwrap_or(22),
                        username: conn.user,
                        identity_file: None,
                    });

                    // Use russh: open tab, connect async
                    let tmux_session = self.next_tmux_session();
                    self.show_welcome = false;
                    return self.open_tab_russh(&conn.label, &tmux_session);
                }
            }

            Message::KeyEvent(event) => {
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = event {
                    // Ctrl+Shift+T — new tab (same server)
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("t".into())
                    {
                        if self.current_conn.is_some() {
                            let n = self.tabs.len() + 1;
                            let label = format!("Session {n}");
                            let tmux_session = self.next_tmux_session();
                            return self.open_tab_russh(&label, &tmux_session);
                        } else if let Some(tab) = self.tabs.last() {
                            let ssh_args = tab.ssh_args.clone();
                            let n = self.tabs.len() + 1;
                            let label = format!("Session {n}");
                            self.open_tab_with_tmux(&ssh_args, &label);
                        } else {
                            self.show_welcome = true;
                        }
                    }
                    // Ctrl+Shift+N — new window
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("n".into())
                        && let Ok(exe) = std::env::current_exe()
                    {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    // Ctrl+Shift+W — close current tab
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("w".into())
                        && !self.tabs.is_empty()
                    {
                        self.close_tab(self.active_tab);
                    }
                    // Ctrl+Tab — next tab
                    if modifiers.control()
                        && !modifiers.shift()
                        && key == keyboard::Key::Named(keyboard::key::Named::Tab)
                        && !self.tabs.is_empty()
                    {
                        self.active_tab = (self.active_tab + 1) % self.tabs.len();
                        self.show_welcome = false;
                        self.update_title();
                    }
                    // Ctrl+Shift+Tab — previous tab
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Named(keyboard::key::Named::Tab)
                        && !self.tabs.is_empty()
                    {
                        if self.active_tab == 0 {
                            self.active_tab = self.tabs.len() - 1;
                        } else {
                            self.active_tab -= 1;
                        }
                        self.show_welcome = false;
                        self.update_title();
                    }
                    // F2 — rename current tab
                    if key == keyboard::Key::Named(keyboard::key::Named::F2)
                        && !self.tabs.is_empty()
                        && self.renaming_tab.is_none()
                    {
                        self.rename_input = self.tabs[self.active_tab].label.clone();
                        self.renaming_tab = Some(self.active_tab);
                        return iced_runtime::widget::operation::focus(RENAME_INPUT_ID);
                    }
                    // Ctrl+Shift+= or Ctrl+= — zoom in
                    if modifiers.control()
                        && (key == keyboard::Key::Character("=".into())
                            || key == keyboard::Key::Character("+".into()))
                    {
                        self.current_font_size = (self.current_font_size + 1.0).min(36.0);
                        self.apply_font_to_all_tabs();
                    }
                    // Ctrl+- — zoom out
                    if modifiers.control() && key == keyboard::Key::Character("-".into()) {
                        self.current_font_size = (self.current_font_size - 1.0).max(8.0);
                        self.apply_font_to_all_tabs();
                    }
                    // Ctrl+0 — zoom reset
                    if modifiers.control() && key == keyboard::Key::Character("0".into()) {
                        self.current_font_size = self.config.terminal.font_size;
                        self.apply_font_to_all_tabs();
                    }
                    // Escape — dismiss context menu, cancel rename, or cancel welcome
                    if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                        if self.context_menu.is_some() {
                            self.context_menu = None;
                        } else if self.renaming_tab.is_some() {
                            self.renaming_tab = None;
                        } else if self.show_welcome && !self.tabs.is_empty() {
                            self.show_welcome = false;
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.tabs.is_empty() {
            return self.view_welcome();
        }

        if self.show_welcome {
            let tab_bar = self.view_tab_bar();
            return column![tab_bar, self.view_welcome()].into();
        }

        let tab_bar = self.view_tab_bar();
        let content: Element<'_, Message> = if let Some(tab) = self.tabs.get(self.active_tab) {
            if tab.dead {
                self.view_dead_tab(tab)
            } else if let Some(ref terminal) = tab.terminal {
                // Show "Connecting..." overlay if russh tab without channel yet
                if tab.uses_russh && tab.ssh_channel_holder.is_none() {
                    stack![
                        container(
                            iced_term::TerminalView::show(terminal).map(Message::TerminalEvent)
                        )
                        .width(Length::Fill)
                        .height(Length::Fill),
                        center(
                            text("Connecting...")
                                .size(16)
                                .color(Color::from_rgb8(0xf9, 0xe2, 0xaf))
                        ),
                    ]
                    .into()
                } else {
                    container(iced_term::TerminalView::show(terminal).map(Message::TerminalEvent))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
            } else if tab.auto_reconnect {
                let attempt_text = format!(
                    "Reconnecting... (attempt {}/{})",
                    tab.reconnect_attempts, self.config.ssh.reconnect_max_attempts
                );
                center(
                    column![
                        text("🔄").size(48),
                        text("Connection lost")
                            .size(20)
                            .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                        text(attempt_text)
                            .size(14)
                            .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    ]
                    .spacing(12)
                    .align_x(iced::Alignment::Center),
                )
                .into()
            } else {
                center(text("Terminal not available")).into()
            }
        } else {
            center(text("No active tab")).into()
        };

        let status_bar = self.view_status_bar();

        // Wrap with tab context menu if active
        let main_view: Element<'_, Message> = if let Some((tab_idx, _x, _y)) = self.tab_context_menu
        {
            let ctx_style = |_theme: &Theme, status: button::Status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Color::from_rgb8(0x45, 0x47, 0x5a)
                    }
                    _ => Color::from_rgb8(0x24, 0x24, 0x36),
                };
                button::Style {
                    background: Some(iced::Background::Color(bg)),
                    text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                    ..Default::default()
                }
            };

            let mut menu_items: Vec<Element<'_, Message>> = Vec::new();

            if tab_idx > 0 {
                menu_items.push(
                    button(text("Move left").size(13))
                        .on_press(Message::TabMoveLeft(tab_idx))
                        .padding([8, 16])
                        .width(180)
                        .style(ctx_style)
                        .into(),
                );
            }
            if tab_idx + 1 < self.tabs.len() {
                menu_items.push(
                    button(text("Move right").size(13))
                        .on_press(Message::TabMoveRight(tab_idx))
                        .padding([8, 16])
                        .width(180)
                        .style(ctx_style)
                        .into(),
                );
            }
            menu_items.push(
                button(text("Rename         F2").size(13))
                    .on_press(Message::StartRename(tab_idx))
                    .padding([8, 16])
                    .width(180)
                    .style(ctx_style)
                    .into(),
            );
            menu_items.push(
                button(text("Close tab").size(13))
                    .on_press(Message::CloseTab(tab_idx))
                    .padding([8, 16])
                    .width(180)
                    .style(ctx_style)
                    .into(),
            );

            let tab_menu =
                container(column(menu_items).spacing(1))
                    .padding(4)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgb8(
                            0x24, 0x24, 0x36,
                        ))),
                        border: iced::Border {
                            radius: 8.0.into(),
                            width: 1.0,
                            color: Color::from_rgb8(0x45, 0x47, 0x5a),
                        },
                        shadow: iced::Shadow {
                            color: Color::from_rgba8(0, 0, 0, 0.5),
                            offset: iced::Vector::new(2.0, 2.0),
                            blur_radius: 8.0,
                        },
                        ..Default::default()
                    });

            let dismiss = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::ContextMenuDismiss);

            stack![
                column![tab_bar, content, status_bar],
                dismiss,
                container(tab_menu).padding(iced::Padding {
                    top: 28.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: (tab_idx as f32) * 120.0,
                }),
            ]
            .into()
        } else if let Some((x, y)) = self.context_menu {
            let ctx_style = |_theme: &Theme, status: button::Status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Color::from_rgb8(0x45, 0x47, 0x5a)
                    }
                    _ => Color::from_rgb8(0x24, 0x24, 0x36),
                };
                button::Style {
                    background: Some(iced::Background::Color(bg)),
                    text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                    ..Default::default()
                }
            };

            let menu = container(
                column![
                    button(text("Copy        Ctrl+Shift+C").size(13))
                        .on_press(Message::ContextMenuCopy)
                        .padding([8, 16])
                        .width(250)
                        .style(ctx_style),
                    button(text("Paste       Ctrl+Shift+V").size(13))
                        .on_press(Message::ContextMenuPaste)
                        .padding([8, 16])
                        .width(250)
                        .style(ctx_style),
                ]
                .spacing(1),
            )
            .padding(4)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgb8(0x24, 0x24, 0x36))),
                border: iced::Border {
                    radius: 8.0.into(),
                    width: 1.0,
                    color: Color::from_rgb8(0x45, 0x47, 0x5a),
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.5),
                    offset: iced::Vector::new(2.0, 2.0),
                    blur_radius: 8.0,
                },
                ..Default::default()
            });

            let dismiss_area = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::ContextMenuDismiss);

            stack![
                column![tab_bar, content, status_bar],
                dismiss_area,
                container(menu).padding(iced::Padding {
                    top: y,
                    right: 0.0,
                    bottom: 0.0,
                    left: x,
                }),
            ]
            .into()
        } else {
            column![tab_bar, content, status_bar].into()
        };

        // Toast overlay
        let main_view: Element<'_, Message> = if let Some((ref msg, _)) = self.toast {
            let toast_widget =
                container(text(msg).size(13).color(Color::from_rgb8(0xcd, 0xd6, 0xf4)))
                    .padding([8, 16])
                    .style(|_theme: &Theme| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgb8(
                            0x31, 0x32, 0x44,
                        ))),
                        border: iced::Border {
                            radius: 8.0.into(),
                            width: 1.0,
                            color: Color::from_rgb8(0x45, 0x47, 0x5a),
                        },
                        ..Default::default()
                    });

            stack![
                main_view,
                column![
                    Space::new().height(Length::Fill),
                    container(row![Space::new().width(Length::Fill), toast_widget,])
                        .padding(16)
                        .width(Length::Fill)
                        .align_bottom(Length::Fill),
                ],
            ]
            .into()
        } else {
            main_view
        };

        main_view
    }

    fn view_dead_tab<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        let index = self.tabs.iter().position(|t| t.id == tab.id).unwrap_or(0);

        center(
            column![
                text("⚠").size(48),
                text("Session disconnected")
                    .size(20)
                    .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                text(&tab.label)
                    .size(14)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                Space::new().height(16),
                button(
                    text("Reconnect")
                        .size(14)
                        .color(Color::from_rgb8(0x1e, 0x1e, 0x2e))
                )
                .on_press(Message::ReconnectTab(index))
                .padding([10, 24])
                .style(|_theme, _status| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(0xa6, 0xe3, 0xa1,))),
                    text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                button(text("Close tab").size(12))
                    .on_press(Message::CloseTab(index))
                    .padding([6, 16])
                    .style(|_theme: &Theme, _status| button::Style {
                        background: None,
                        text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                        ..Default::default()
                    }),
            ]
            .spacing(12)
            .align_x(iced::Alignment::Center),
        )
        .into()
    }

    fn view_tab_bar(&self) -> Element<'_, Message> {
        let mut tabs_row: Vec<Element<'_, Message>> = Vec::new();

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab && !self.show_welcome;
            let is_renaming = self.renaming_tab == Some(i);

            let bg = if is_active {
                Color::from_rgb8(0x31, 0x32, 0x44)
            } else {
                Color::from_rgb8(0x1e, 0x1e, 0x2e)
            };

            let tab_btn: Element<'_, Message> = if is_renaming {
                container(
                    text_input("tab name", &self.rename_input)
                        .id(RENAME_INPUT_ID)
                        .on_input(Message::RenameInputChanged)
                        .on_submit(Message::FinishRename)
                        .size(12)
                        .padding([4, 8])
                        .width(150),
                )
                .padding([2, 4])
                .style(move |_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(bg)),
                    ..Default::default()
                })
                .into()
            } else {
                let label_text: String = if tab.label.len() > 25 {
                    format!("{}...", &tab.label[..22])
                } else {
                    tab.label.clone()
                };

                let (indicator, label_color) = if tab.dead {
                    ("●", Color::from_rgb8(0xf3, 0x8b, 0xa8))
                } else if tab.terminal.is_none() && tab.auto_reconnect {
                    ("●", Color::from_rgb8(0xf9, 0xe2, 0xaf))
                } else if tab.uses_russh && tab.ssh_channel_holder.is_none() {
                    // Connecting state for russh tabs
                    ("●", Color::from_rgb8(0xf9, 0xe2, 0xaf))
                } else {
                    ("●", Color::from_rgb8(0xa6, 0xe3, 0xa1))
                };

                let close_btn = button(text("×").size(12))
                    .on_press(Message::CloseTab(i))
                    .padding([0, 4])
                    .style(|_theme: &Theme, _status| button::Style {
                        background: None,
                        text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                        ..Default::default()
                    });

                let tab_content = row![
                    text(indicator).size(8).color(label_color),
                    text(label_text)
                        .size(12)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    close_btn
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center);

                let tab_button = button(tab_content)
                    .on_press(Message::SelectTab(i))
                    .padding([6, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(iced::Background::Color(bg)),
                        text_color: label_color,
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    });

                mouse_area(tab_button)
                    .on_right_press(Message::TabContextMenu(i, 0.0, 30.0))
                    .into()
            };

            tabs_row.push(tab_btn);
        }

        let new_tab_btn = button(text("+").size(14))
            .on_press(Message::NewTab)
            .padding([6, 10])
            .style(|_theme: &Theme, _status| button::Style {
                background: None,
                text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                ..Default::default()
            });

        let bar = row![row(tabs_row).spacing(1), new_tab_btn]
            .width(Length::Fill)
            .align_y(iced::Alignment::Center);

        container(bar)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgb8(0x18, 0x18, 0x25))),
                ..Default::default()
            })
            .into()
    }

    fn view_status_bar(&self) -> Element<'_, Message> {
        let active_count = self.tabs.iter().filter(|t| !t.dead).count();
        let dead_count = self.tabs.iter().filter(|t| t.dead).count();
        let total = self.tabs.len();

        let zoom_info = if (self.current_font_size - self.config.terminal.font_size).abs() > 0.1 {
            format!("  {}pt", self.current_font_size)
        } else {
            String::new()
        };

        let status_text = if dead_count > 0 {
            format!("{total} tabs ({active_count} active, {dead_count} disconnected){zoom_info}")
        } else {
            format!(
                "{total} tab{}{zoom_info}",
                if total == 1 { "" } else { "s" }
            )
        };

        let active_label = if let Some(tab) = self.tabs.get(self.active_tab) {
            tab.label.clone()
        } else {
            String::new()
        };

        container(
            row![
                text(active_label)
                    .size(11)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                Space::new().width(Length::Fill),
                text(status_text)
                    .size(11)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
            ]
            .padding([2, 8])
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x18, 0x18, 0x25))),
            ..Default::default()
        })
        .into()
    }

    fn view_welcome(&self) -> Element<'_, Message> {
        let logo = text("🐚").size(64);
        let title = text("shellkeep")
            .size(28)
            .color(Color::from_rgb8(0x89, 0xb4, 0xfa));

        let version = format!(
            "v{} — SSH sessions that survive everything",
            env!("CARGO_PKG_VERSION")
        );
        let subtitle = text(version)
            .size(14)
            .color(Color::from_rgb8(0xa6, 0xad, 0xc8));

        let host_field = text_input("user@host or just hostname", &self.host_input)
            .on_input(Message::HostInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let user_field = text_input("username", &self.user_input)
            .on_input(Message::UserInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let port_field = text_input("22", &self.port_input)
            .on_input(Message::PortInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10)
            .width(80);

        let identity_field = text_input("~/.ssh/id_ed25519 (optional)", &self.identity_input)
            .on_input(Message::IdentityInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let connect_btn = button(
            text("Connect")
                .size(14)
                .color(Color::from_rgb8(0x1e, 0x1e, 0x2e)),
        )
        .on_press(Message::Connect)
        .padding([10, 24])
        .style(|_theme, _status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x89, 0xb4, 0xfa))),
            text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

        let host_row = row![
            column![text("Host").size(12), host_field]
                .spacing(4)
                .width(Length::Fill),
            column![text("Port").size(12), port_field].spacing(4),
        ]
        .spacing(8);

        let user_row = column![text("Username").size(12), user_field].spacing(4);
        let identity_row = column![text("Identity file").size(12), identity_field].spacing(4);

        let error_text: Element<'_, Message> = if let Some(ref err) = self.error {
            text(err)
                .size(12)
                .color(Color::from_rgb8(0xf3, 0x8b, 0xa8))
                .into()
        } else {
            Space::new().height(0).into()
        };

        // Recent connections list
        let recent_section: Element<'_, Message> = if self.recent.connections.is_empty() {
            Space::new().height(0).into()
        } else {
            let mut recent_items: Vec<Element<'_, Message>> = Vec::new();
            recent_items.push(
                text("Recent connections")
                    .size(12)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86))
                    .into(),
            );
            for (i, conn) in self.recent.connections.iter().enumerate() {
                let display_label = if let Some(ts) = conn.last_connected {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let ago = now.saturating_sub(ts);
                    let time_str = if ago < 60 {
                        "just now".to_string()
                    } else if ago < 3600 {
                        format!("{}m ago", ago / 60)
                    } else if ago < 86400 {
                        format!("{}h ago", ago / 3600)
                    } else {
                        format!("{}d ago", ago / 86400)
                    };
                    format!("{}  ({})", conn.label, time_str)
                } else {
                    conn.label.clone()
                };

                let item: Element<'_, Message> = button(
                    text(display_label)
                        .size(13)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                )
                .on_press(Message::ConnectRecent(i))
                .padding([6, 12])
                .width(Length::Fill)
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
                    text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                .into();
                recent_items.push(item);
            }
            scrollable(column(recent_items).spacing(4))
                .height(Length::Shrink)
                .into()
        };

        let shortcuts_hint = text(
            "Ctrl+Shift+T new tab  |  Ctrl+Shift+N new window  |  Ctrl+Shift+W close  |  F2 rename",
        )
        .size(10)
        .color(Color::from_rgb8(0x58, 0x5b, 0x70));

        let form = column![
            logo,
            title,
            subtitle,
            Space::new().height(20),
            host_row,
            user_row,
            identity_row,
            Space::new().height(8),
            connect_btn,
            error_text,
            Space::new().height(12),
            recent_section,
            Space::new().height(20),
            shortcuts_hint,
        ]
        .spacing(12)
        .align_x(iced::Alignment::Center)
        .max_width(420);

        center(form).into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs: Vec<Subscription<Message>> = Vec::new();

        for tab in &self.tabs {
            if let Some(ref terminal) = tab.terminal {
                subs.push(terminal.subscription().map(Message::TerminalEvent));
            }

            // SSH channel I/O subscription for russh tabs with a connected channel
            if tab.uses_russh
                && let (Some(channel_holder), Some(writer_rx_holder), Some(resize_rx_holder)) = (
                    &tab.ssh_channel_holder,
                    &tab.ssh_writer_rx_holder,
                    &tab.ssh_resize_rx_holder,
                )
            {
                let data = SshSubscriptionData {
                    tab_id: tab.id,
                    channel: channel_holder.clone(),
                    writer_rx: writer_rx_holder.clone(),
                    resize_rx: resize_rx_holder.clone(),
                };
                subs.push(Subscription::run_with(data, ssh_channel_stream));
            }
        }

        subs.push(keyboard::listen().map(Message::KeyEvent));

        // Auto-reconnect timer — check every 3 seconds for tabs needing reconnection
        let has_reconnectable = self
            .tabs
            .iter()
            .any(|t| t.terminal.is_none() && t.auto_reconnect && !t.dead);
        if has_reconnectable {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(3))
                    .map(|_| Message::AutoReconnectTick),
            );
        }

        // Toast auto-dismiss after 3 seconds
        if let Some((_, created)) = &self.toast {
            if created.elapsed() > std::time::Duration::from_secs(3) {
                // Can't mutate self in subscription, use a timer instead
            }
            subs.push(
                iced::time::every(std::time::Duration::from_secs(3)).map(|_| Message::ToastDismiss),
            );
        }

        Subscription::batch(subs)
    }

    fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }
}

// ---------------------------------------------------------------------------
// Host input parsing: supports user@host:port, user@host, host:port, host
// ---------------------------------------------------------------------------

fn parse_host_input(input: &str) -> (Option<String>, String, Option<String>) {
    let mut user = None;
    let mut remaining = input.to_string();

    // Extract user@
    if let Some(at_pos) = remaining.find('@') {
        user = Some(remaining[..at_pos].to_string());
        remaining = remaining[at_pos + 1..].to_string();
    }

    // Extract :port (but not IPv6 brackets)
    let port = if remaining.starts_with('[') {
        // IPv6: [::1]:port
        if let Some(bracket_end) = remaining.find(']') {
            let host = remaining[1..bracket_end].to_string();
            let port = if remaining.len() > bracket_end + 2
                && remaining.as_bytes()[bracket_end + 1] == b':'
            {
                Some(remaining[bracket_end + 2..].to_string())
            } else {
                None
            };
            remaining = host;
            port
        } else {
            None
        }
    } else if let Some(colon_pos) = remaining.rfind(':') {
        let maybe_port = &remaining[colon_pos + 1..];
        if maybe_port.parse::<u16>().is_ok() {
            let port = Some(maybe_port.to_string());
            remaining = remaining[..colon_pos].to_string();
            port
        } else {
            None
        }
    } else {
        None
    };

    (user, remaining, port)
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

fn catppuccin_mocha() -> ColorPalette {
    ColorPalette {
        foreground: "#cdd6f4".into(),
        background: "#1e1e2e".into(),
        black: "#45475a".into(),
        red: "#f38ba8".into(),
        green: "#a6e3a1".into(),
        yellow: "#f9e2af".into(),
        blue: "#89b4fa".into(),
        magenta: "#f5c2e7".into(),
        cyan: "#94e2d5".into(),
        white: "#bac2de".into(),
        bright_black: "#585b70".into(),
        bright_red: "#f38ba8".into(),
        bright_green: "#a6e3a1".into(),
        bright_yellow: "#f9e2af".into(),
        bright_blue: "#89b4fa".into(),
        bright_magenta: "#f5c2e7".into(),
        bright_cyan: "#94e2d5".into(),
        bright_white: "#a6adc8".into(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_simple() {
        let (user, host, port) = parse_host_input("example.com");
        assert_eq!(user, None);
        assert_eq!(host, "example.com");
        assert_eq!(port, None);
    }

    #[test]
    fn parse_host_with_user() {
        let (user, host, port) = parse_host_input("alice@example.com");
        assert_eq!(user, Some("alice".into()));
        assert_eq!(host, "example.com");
        assert_eq!(port, None);
    }

    #[test]
    fn parse_host_with_port() {
        let (user, host, port) = parse_host_input("example.com:2222");
        assert_eq!(user, None);
        assert_eq!(host, "example.com");
        assert_eq!(port, Some("2222".into()));
    }

    #[test]
    fn parse_host_full() {
        let (user, host, port) = parse_host_input("alice@example.com:2222");
        assert_eq!(user, Some("alice".into()));
        assert_eq!(host, "example.com");
        assert_eq!(port, Some("2222".into()));
    }

    #[test]
    fn parse_host_ipv6() {
        let (user, host, port) = parse_host_input("[::1]:2222");
        assert_eq!(user, None);
        assert_eq!(host, "::1");
        assert_eq!(port, Some("2222".into()));
    }
}
