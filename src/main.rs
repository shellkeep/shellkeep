// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! shellkeep — SSH terminal manager.
//!
//! Persistent sessions that survive everything.
//! Open source. Cross-platform. Zero server setup.

mod app;
mod cli;
mod instance;
mod theme;

use app::session::{SshSubscriptionData, establish_ssh_session, ssh_channel_stream};
use app::tab::{
    ChannelHolder, ConnParams, Tab, SPINNER_FRAMES,
    make_backend_settings, make_font_settings, make_theme_settings,
};
use app::Message;

use std::sync::Arc;
use iced::{Point, Size, Subscription, Task, Theme, keyboard, window};
use iced_term::settings::{BackendSettings, FontSettings, Settings};
use iced_term::{AlacrittyColumn, AlacrittyLine, AlacrittyPoint, RegexSearch, SearchMatch};
use shellkeep::config::Config;
use shellkeep::ssh::manager::{ConnKey, ConnectionManager};
use shellkeep::state::history;
use shellkeep::state::recent::{RecentConnection, RecentConnections};
use shellkeep::state::state_file::{StateFile, TabState, WindowState};
use shellkeep::tray::{Tray, TrayAction};
use shellkeep::{i18n, ssh};
use tokio::sync::Mutex;

const RENAME_INPUT_ID: &str = "rename-tab-input";

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

    // NFR-OBS-04: rotate log if it exceeds 10 MB
    shellkeep::crash::rotate_logs(&log_path);
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
    let initial_ssh_args = cli::parse_cli_ssh_args(&args[1..]);

    tracing::info!("shellkeep v{} starting", env!("CARGO_PKG_VERSION"));

    // NFR-I18N-07: detect and initialize locale
    let locale = i18n::detect_locale();
    i18n::init(&locale);
    tracing::info!("locale: {locale}");

    // NFR-SEC-10: disable core dumps
    shellkeep::crash::disable_core_dumps();

    // NFR-OBS-09: install crash handler
    shellkeep::crash::install_panic_hook();

    // NFR-SEC-03: verify and fix file permissions on startup
    shellkeep::state::permissions::verify_and_fix();

    // FR-CLI-04: single instance detection
    let _pid_guard = match instance::check_single_instance() {
        Some(guard) => guard,
        None => {
            eprintln!("shellkeep is already running (another instance detected)");
            std::process::exit(0);
        }
    };

    // FR-STATE-14: load saved window geometry for startup
    let saved_window = {
        let tmp_client_id =
            shellkeep::state::client_id::resolve(Config::load().general.client_id.as_deref());
        StateFile::load_local(&StateFile::local_cache_path(&tmp_client_id)).and_then(|s| s.window)
    };

    let mut app_builder = iced::application(
        move || ShellKeep::new(initial_ssh_args.clone()),
        ShellKeep::update,
        ShellKeep::view,
    )
    .title(ShellKeep::title)
    .subscription(ShellKeep::subscription)
    .theme(ShellKeep::theme)
    .antialiasing(true)
    // FR-TABS-17: intercept window close to show confirmation dialog
    .exit_on_close_request(false);

    if let Some(ref geo) = saved_window {
        app_builder = app_builder.window_size(Size::new(geo.width as f32, geo.height as f32));
        if let (Some(x), Some(y)) = (geo.x, geo.y) {
            app_builder =
                app_builder.position(window::Position::Specific(Point::new(x as f32, y as f32)));
        }
    } else {
        app_builder = app_builder.window_size((900.0, 600.0));
    }

    app_builder.run()
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
    /// FR-RECONNECT-02: spinner animation frame index
    spinner_frame: usize,
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
    /// Whether we've already listed existing sessions after first connect
    sessions_listed: bool,
    /// Debounce: time of last state flush
    last_state_save: Option<std::time::Instant>,
    /// Debounce: state has unsaved changes
    state_dirty: bool,

    // Welcome screen state
    /// FR-UI-03: first-use client-id name input
    client_id_input: String,
    /// FR-UI-01: toggle for advanced connection options (port, user, identity)
    show_advanced: bool,
    host_input: String,
    port_input: String,
    user_input: String,
    identity_input: String,

    config: Config,
    recent: RecentConnections,
    title_text: String,
    error: Option<String>,

    /// System tray icon (FR-TRAY-01)
    tray: Option<Tray>,

    // Scrollback search state (FR-TABS-09, FR-TERMINAL-07)
    search_active: bool,
    search_input: String,
    search_regex: Option<RegexSearch>,
    search_last_match: Option<SearchMatch>,

    /// FR-CONFIG-04: config hot reload receiver
    config_reload_rx: Option<std::sync::mpsc::Receiver<()>>,

    /// FR-TABS-17: close confirmation dialog visible
    show_close_dialog: bool,
    /// FR-TABS-17: window ID to close after dialog
    close_window_id: Option<window::Id>,
    /// FR-STATE-14: current window geometry for persistence
    window_width: u32,
    window_height: u32,
    window_x: Option<i32>,
    window_y: Option<i32>,
    /// FR-STATE-14: debounce timer for geometry saves
    last_geometry_save: Option<std::time::Instant>,
    /// FR-CONN-20: remote state syncer (SFTP or shell fallback)
    state_syncer: Option<Arc<ssh::sftp::StateSyncer>>,

    /// FR-ENV-06: one environment active per instance
    current_environment: String,

    // FR-ENV-03: environment selection dialog state
    show_env_dialog: bool,
    env_list: Vec<String>,
    env_filter: String,
    selected_env: Option<String>,
    // FR-ENV-07..09: environment management modals
    show_new_env_dialog: bool,
    new_env_input: String,
    show_rename_env_dialog: bool,
    rename_env_input: String,
    rename_env_target: Option<String>,
    show_delete_env_dialog: bool,
    delete_env_target: Option<String>,

    // FR-CONN-03: host key TOFU dialog
    #[allow(dead_code)]
    pending_host_key_prompt: Option<ssh::connection::HostKeyPrompt>,
    // FR-CONN-09: password prompt dialog
    #[allow(dead_code)]
    show_password_dialog: bool,
    #[allow(dead_code)]
    password_input: String,
    #[allow(dead_code)]
    password_target_tab: Option<u64>,
    #[allow(dead_code)]
    password_conn_params: Option<ConnParams>,
    // FR-LOCK-05: lock conflict dialog
    #[allow(dead_code)]
    show_lock_dialog: bool,
    #[allow(dead_code)]
    lock_info_text: String,
    #[allow(dead_code)]
    lock_target_tab: Option<u64>,

    /// FR-SESSION-10a: close-tab confirmation dialog
    pending_close_tabs: Option<Vec<usize>>,

    /// FR-RECONNECT-08: last known default gateway (Linux network monitoring)
    #[cfg(target_os = "linux")]
    last_gateway: Option<String>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

// (no large bundle structs — channel is passed via Arc<Mutex<Option<>>> holders)


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
            spinner_frame: 0,
            rename_input: String::new(),
            current_font_size: config.terminal.font_size,
            context_menu: None,
            tab_context_menu: None,
            toast: None,
            current_conn: None,
            client_id: shellkeep::state::client_id::resolve(config.general.client_id.as_deref()),
            current_environment: "Default".to_string(),
            conn_manager: Arc::new(Mutex::new(ConnectionManager::new())),
            sessions_listed: false,
            last_state_save: None,
            state_dirty: false,
            client_id_input: String::new(),
            show_advanced: false,
            host_input: String::new(),
            port_input: default_port,
            user_input: username,
            identity_input: String::new(),
            config,
            recent,
            title_text: "shellkeep".to_string(),
            error: None,
            tray: None,
            search_active: false,
            search_input: String::new(),
            search_regex: None,
            search_last_match: None,
            config_reload_rx: None,
            show_close_dialog: false,
            close_window_id: None,
            window_width: 900,
            window_height: 600,
            window_x: None,
            window_y: None,
            last_geometry_save: None,
            state_syncer: None,
            show_env_dialog: false,
            env_list: Vec::new(),
            env_filter: String::new(),
            selected_env: None,
            show_new_env_dialog: false,
            new_env_input: String::new(),
            show_rename_env_dialog: false,
            rename_env_input: String::new(),
            rename_env_target: None,
            show_delete_env_dialog: false,
            delete_env_target: None,
            pending_host_key_prompt: None,
            show_password_dialog: false,
            password_input: String::new(),
            password_target_tab: None,
            password_conn_params: None,
            show_lock_dialog: false,
            lock_info_text: String::new(),
            lock_target_tab: None,
            pending_close_tabs: None,
            #[cfg(target_os = "linux")]
            last_gateway: read_default_gateway(),
        };

        // FR-CONFIG-04: start watching config file for hot reload
        app.config_reload_rx = Some(shellkeep::config::watch_config(Config::file_path()));

        // FR-TRAY-01: initialize system tray icon
        app.tray = Tray::new(app.config.tray.enabled);

        // FR-STATE-07: clean up orphaned .tmp files from interrupted saves
        shellkeep::state::state_file::cleanup_tmp_files(&app.client_id);

        // FR-HISTORY-11: clean up old history files on startup
        history::cleanup_old_history(app.config.state.history_max_days);
        if let Some(ssh_args) = initial_ssh_args {
            // Parse connection params from CLI args.
            // First extract flags with values (-p PORT, -i FILE, -l USER)
            // so we can correctly identify the host argument.
            let mut cli_port = "22".to_string();
            let mut cli_identity = None;
            let mut cli_user_flag = None;
            let mut flag_value_indices = std::collections::HashSet::new();
            let mut i = 0;
            while i < ssh_args.len() {
                match ssh_args[i].as_str() {
                    "-p" if i + 1 < ssh_args.len() => {
                        cli_port = ssh_args[i + 1].clone();
                        flag_value_indices.insert(i);
                        flag_value_indices.insert(i + 1);
                        i += 1;
                    }
                    "-i" if i + 1 < ssh_args.len() => {
                        cli_identity = Some(ssh_args[i + 1].clone());
                        flag_value_indices.insert(i);
                        flag_value_indices.insert(i + 1);
                        i += 1;
                    }
                    "-l" if i + 1 < ssh_args.len() => {
                        cli_user_flag = Some(ssh_args[i + 1].clone());
                        flag_value_indices.insert(i);
                        flag_value_indices.insert(i + 1);
                        i += 1;
                    }
                    _ => {}
                }
                i += 1;
            }
            // The host is the first non-flag argument
            let host_arg = ssh_args
                .iter()
                .enumerate()
                .find(|(idx, a)| !a.starts_with('-') && !flag_value_indices.contains(idx))
                .map(|(_, a)| a.clone())
                .unwrap_or_default();
            let label = host_arg.clone();
            let (parsed_user, parsed_host, parsed_port) = cli::parse_host_input(&host_arg);
            app.current_conn = Some(ConnParams {
                host: parsed_host,
                port: parsed_port
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(cli_port.parse().unwrap_or(22)),
                username: cli_user_flag
                    .or(parsed_user)
                    .unwrap_or_else(whoami::username),
                identity_file: cli_identity,
            });

            // FR-CONN-21: CLI launch via russh (async, non-blocking)
            // Opens one tab immediately; existing sessions discovered after connect
            let tmux_session = app.next_tmux_session();
            let task = app.open_tab_russh(&label, &tmux_session);
            return (app, task);
        } else {
            app.show_welcome = true;
        }

        // NFR-OBS-11: check for previous crash dumps
        let crash_dir = shellkeep::crash::crash_dir();
        if crash_dir.exists()
            && let Ok(entries) = std::fs::read_dir(&crash_dir)
        {
            let count = entries.filter_map(|e| e.ok()).count();
            if count > 0 {
                app.toast = Some((
                    format!(
                        "Previous crash detected ({count} dump(s)). Run shellkeep --crash-report for details."
                    ),
                    std::time::Instant::now(),
                ));
            }
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
    /// FR-SESSION-04, FR-SESSION-05, FR-ENV-02: generate tmux session name with client-id,
    /// environment, and timestamp.
    fn next_tmux_session(&self) -> String {
        shellkeep::ssh::tmux::env_tmux_session_name(&self.client_id, &self.current_environment)
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
            font: make_font_settings(&self.config, self.config.terminal.font_size),
            theme: make_theme_settings(&self.config),
            backend: make_backend_settings(&self.config),
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
        let phase = Arc::new(std::sync::Mutex::new(i18n::t(i18n::CONNECTING).to_string()));

        let ssh_args = self
            .current_conn
            .as_ref()
            .map(|c| self.build_ssh_args_from_conn(c))
            .unwrap_or_default();

        // FR-HISTORY-02: create history writer (None if disabled via config)
        let session_uuid = uuid::Uuid::new_v4().to_string();
        let history_writer =
            history::HistoryWriter::new(&session_uuid, self.config.state.history_max_size_mb);
        let suuid = session_uuid.clone();
        self.tabs.push(Tab {
            id,
            label: label.to_string(),
            session_uuid,
            terminal: Some(terminal),
            ssh_args,
            conn_params: self.current_conn.clone(),
            tmux_session: tmux_session.to_string(),
            dead: false,
            reconnect_attempts: 0,
            auto_reconnect: true,
            uses_russh: true,
            reconnect_delay_ms: 0,
            last_error: None,
            last_latency_ms: None,
            reconnect_started: None,
            ssh_channel_holder: None, // set when SshConnected(Ok) arrives
            ssh_writer_rx_holder: Some(writer_rx_holder),
            ssh_resize_tx: Some(resize_tx),
            ssh_resize_rx_holder: Some(resize_rx_holder),
            conn_key: None,
            pending_channel: Some(channel_holder.clone()),
            connection_phase: Some(phase.clone()),
            history_writer,
            needs_initial_resize: true,
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
        let keepalive = self.config.ssh.keepalive_interval;
        let cid = self.client_id.clone();
        let phase_clone = phase;
        Task::perform(
            async move {
                match establish_ssh_session(
                    mgr,
                    conn,
                    tmux,
                    80,
                    24,
                    keepalive,
                    cid,
                    phase_clone,
                    suuid,
                )
                .await
                {
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
            font: make_font_settings(&self.config, self.config.terminal.font_size),
            theme: make_theme_settings(&self.config),
            backend: BackendSettings {
                program: "ssh".to_string(),
                args: full_args,
                cursor_shape: self.config.terminal.cursor_shape.clone(),
                ..Default::default()
            },
        };

        match iced_term::Terminal::new(id, settings) {
            Ok(terminal) => {
                self.tabs.push(Tab {
                    id,
                    label: label.to_string(),
                    session_uuid: uuid::Uuid::new_v4().to_string(),
                    terminal: Some(terminal),
                    ssh_args: ssh_args.to_vec(),
                    conn_params: self.current_conn.clone(),
                    tmux_session: tmux_session.to_string(),
                    dead: false,
                    reconnect_attempts: 0,
                    auto_reconnect: true,
                    reconnect_delay_ms: 0,
                    last_error: None,
                    last_latency_ms: None,
                    reconnect_started: None,
                    uses_russh: false,
                    ssh_channel_holder: None,
                    ssh_writer_rx_holder: None,
                    ssh_resize_tx: None,
                    ssh_resize_rx_holder: None,
                    conn_key: None,
                    pending_channel: None,
                    connection_phase: None,
                    history_writer: None,
                    needs_initial_resize: true,
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

    /// Close a tab and kill the tmux session on the server.
    fn close_tab(&mut self, index: usize) -> Task<Message> {
        if index >= self.tabs.len() {
            return Task::none();
        }
        let tab = self.tabs.remove(index);
        tracing::info!(
            "closed tab {}: {} (killing tmux session)",
            tab.id,
            tab.label
        );
        if self.active_tab >= self.tabs.len() && self.active_tab > 0 {
            self.active_tab -= 1;
        }
        self.update_title();
        self.save_state();

        self.toast = Some((
            "Session closed and terminated on server.".into(),
            std::time::Instant::now(),
        ));

        // Kill the tmux session on the server
        if !tab.dead && tab.uses_russh {
            let tmux_session = tab.tmux_session.clone();
            let mgr = self.conn_manager.clone();
            if let Some(ref conn) = self.current_conn {
                let conn_key = ConnKey {
                    host: conn.host.clone(),
                    port: conn.port,
                    username: conn.username.clone(),
                };
                return Task::perform(
                    async move {
                        let mgr_guard = mgr.lock().await;
                        if let Some(handle_arc) = mgr_guard.get_cached(&conn_key) {
                            let handle = handle_arc.lock().await;
                            let cmd = format!("tmux kill-session -t {tmux_session} 2>/dev/null");
                            if let Err(e) = ssh::connection::exec_command(&handle, &cmd).await {
                                tracing::warn!("failed to kill tmux session {tmux_session}: {e}");
                            } else {
                                tracing::info!("killed tmux session: {tmux_session}");
                            }
                        }
                    },
                    |_| Message::ContextMenuDismiss, // no-op callback
                );
            }
        }
        Task::none()
    }

    /// Hide a tab — disconnect SSH but keep the tmux session alive on the server.
    fn hide_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let tab = self.tabs.remove(index);
        tracing::info!("hid tab {}: {} (session kept on server)", tab.id, tab.label);
        if self.active_tab >= self.tabs.len() && self.active_tab > 0 {
            self.active_tab -= 1;
        }
        self.update_title();
        self.save_state();
        self.toast = Some((
            i18n::t(i18n::SESSION_KEPT).into(),
            std::time::Instant::now(),
        ));
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
                font: make_font_settings(&self.config, self.current_font_size),
                theme: make_theme_settings(&self.config),
                backend: make_backend_settings(&self.config),
            };

            match iced_term::Terminal::new_ssh(tab.id, settings, ssh_writer_tx) {
                Ok(terminal) => {
                    tab.terminal = Some(terminal);
                    tab.ssh_writer_rx_holder = Some(Arc::new(Mutex::new(Some(ssh_writer_rx))));
                    tab.ssh_resize_tx = Some(resize_tx);
                    tab.ssh_resize_rx_holder = Some(Arc::new(Mutex::new(Some(resize_rx))));
                    let phase = Arc::new(std::sync::Mutex::new(
                        i18n::t(i18n::RECONNECTING).to_string(),
                    ));
                    tab.pending_channel = Some(channel_holder.clone());
                    tab.connection_phase = Some(phase.clone());
                    tab.dead = false;

                    let conn = match &tab.conn_params {
                        Some(c) => c.clone(),
                        None => return Task::none(),
                    };
                    let tmux = tab.tmux_session.clone();
                    let mgr = self.conn_manager.clone();
                    let tab_id = tab.id;
                    let holder = channel_holder;
                    let keepalive = self.config.ssh.keepalive_interval;
                    let cid = self.client_id.clone();
                    let phase_clone = phase;
                    let suuid = tab.session_uuid.clone();
                    self.update_title();

                    Task::perform(
                        async move {
                            match establish_ssh_session(
                                mgr,
                                conn,
                                tmux,
                                80,
                                24,
                                keepalive,
                                cid,
                                phase_clone,
                                suuid,
                            )
                            .await
                            {
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
                // SAFETY: len() > 1 guarantees pop() returns Some
                #[allow(clippy::unwrap_used)]
                let tab = self.tabs.pop().unwrap();
                self.tabs.insert(index, tab);
                self.active_tab = index;
                self.update_title();
            }
            Task::none()
        }
    }

    fn apply_font_to_all_tabs(&mut self) {
        let font_settings = make_font_settings(&self.config, self.current_font_size);
        for tab in &mut self.tabs {
            if let Some(ref mut terminal) = tab.terminal {
                terminal.handle(iced_term::Command::ChangeFont(font_settings.clone()));
            }
        }
        tracing::debug!("font size: {}", self.current_font_size);
    }

    fn save_state(&mut self) {
        self.state_dirty = true;
        if let Some(last) = self.last_state_save
            && last.elapsed() < std::time::Duration::from_secs(2)
        {
            return; // debounced — will be saved by FlushState timer
        }
        self.flush_state();
    }

    fn flush_state(&mut self) {
        if !self.state_dirty {
            return;
        }
        self.state_dirty = false;
        self.last_state_save = Some(std::time::Instant::now());
        let mut state = StateFile::new(&self.client_id);
        // FR-ENV-06: save tabs into the current environment
        let env_tabs: Vec<TabState> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| TabState {
                session_uuid: tab.session_uuid.clone(),
                tmux_session_name: tab.tmux_session.clone(),
                title: tab.label.clone(),
                position: i,
            })
            .collect();
        state.environments.insert(
            self.current_environment.clone(),
            shellkeep::state::state_file::Environment {
                name: self.current_environment.clone(),
                tabs: env_tabs,
            },
        );
        state.last_environment = Some(self.current_environment.clone());
        // Preserve other environments from the previously loaded state
        if let Some(prev) = StateFile::load_local(&StateFile::local_cache_path(&self.client_id)) {
            for (name, env) in &prev.environments {
                if name != &self.current_environment {
                    state.environments.insert(name.clone(), env.clone());
                }
            }
        }
        // FR-STATE-14: persist window geometry
        state.window = Some(WindowState {
            x: self.window_x,
            y: self.window_y,
            width: self.window_width,
            height: self.window_height,
        });
        let path = StateFile::local_cache_path(&self.client_id);

        // FR-TRAY-02: update tray tooltip with session count
        if let Some(ref tray) = self.tray {
            let active_count = self.tabs.iter().filter(|t| !t.dead).count();
            tray.set_session_count(active_count);
            // FR-TRAY-04: change icon when active sessions exist but window may be hidden
            tray.set_hidden_active(active_count > 0 && !self.show_welcome);
        }

        // FR-STATE-06: write state to disk asynchronously to avoid blocking the UI
        match serde_json::to_string_pretty(&state) {
            Ok(state_json) => {
                // FR-CONN-20: also sync to server if syncer is available
                if let Some(ref syncer) = self.state_syncer {
                    let syncer = syncer.clone();
                    let json = state_json.clone();
                    tokio::task::spawn(async move {
                        if let Err(e) = syncer.write_state(&json).await {
                            tracing::warn!("server state sync failed: {e}");
                        }
                    });
                }
                tokio::task::spawn_blocking(move || {
                    let tmp = path.with_extension("tmp");
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::write(&tmp, &state_json) {
                        tracing::warn!("failed to write state tmp: {e}");
                    } else if let Err(e) = std::fs::rename(&tmp, &path) {
                        tracing::warn!("failed to rename state file: {e}");
                    } else {
                        tracing::debug!("state saved to {}", path.display());
                    }
                });
            }
            Err(e) => {
                tracing::warn!("failed to serialize state: {e}");
            }
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
        let (parsed_user, parsed_host, parsed_port) = cli::parse_host_input(host);

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
                    // FR-HISTORY-02: write to local JSONL history
                    if let Some(ref mut writer) = tab.history_writer {
                        writer.append_output(&data);
                    }
                    // FR-TERMINAL-16: deferred initial resize — by the time data arrives,
                    // the terminal widget has definitely rendered and knows its real size
                    if tab.needs_initial_resize {
                        let (cols, rows) = terminal.terminal_size();
                        if cols > 0
                            && rows > 0
                            && let Some(ref resize_tx) = tab.ssh_resize_tx
                        {
                            let _ = resize_tx.send((cols as u32, rows as u32));
                            tracing::info!("tab {tab_id}: deferred initial resize {cols}x{rows}");
                        }
                        tab.needs_initial_resize = false;
                    }
                }
            }

            Message::SshDisconnected(tab_id, reason) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    // Clear channel state so subscription stops
                    tab.ssh_channel_holder = None;
                    tab.ssh_resize_tx = None;
                    tab.connection_phase = None;
                    // FR-UI-08: store last error for dead tab display
                    tab.last_error = Some(reason.clone());

                    // FR-RECONNECT-07: classify error
                    if ssh::errors::is_permanent(&reason) {
                        tab.terminal = None;
                        tab.dead = true;
                        tab.auto_reconnect = false;
                        tracing::error!("permanent error for tab {tab_id}: {reason}");
                    } else if tab.auto_reconnect
                        && tab.reconnect_attempts < self.config.ssh.reconnect_max_attempts
                    {
                        tab.reconnect_attempts += 1;
                        tab.terminal = None;
                        tab.reconnect_started = Some(std::time::Instant::now());
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
                            tab.connection_phase = None;
                            tracing::info!("SSH tab {tab_id}: connected, channel ready");

                            // FR-TERMINAL-16: send immediate resize to match actual
                            // terminal widget size (PTY was opened with default 80x24)
                            if let Some(ref terminal) = tab.terminal {
                                let (cols, rows) = terminal.terminal_size();
                                tracing::info!("tab {tab_id}: terminal widget size {cols}x{rows}");
                                if cols > 0
                                    && rows > 0
                                    && let Some(ref resize_tx) = tab.ssh_resize_tx
                                {
                                    let _ = resize_tx.send((cols as u32, rows as u32));
                                    tab.needs_initial_resize = false;
                                    tracing::info!(
                                        "tab {tab_id}: sent initial resize {cols}x{rows}"
                                    );
                                }
                            } else {
                                tracing::info!(
                                    "tab {tab_id}: no terminal widget yet, resize deferred"
                                );
                            }
                        }

                        // After first successful connect, list existing tmux sessions
                        if !self.sessions_listed && self.current_conn.is_some() {
                            self.sessions_listed = true;
                            let mgr = self.conn_manager.clone();
                            // SAFETY: is_some() checked on the line above
                            #[allow(clippy::unwrap_used)]
                            let conn = self.current_conn.clone().unwrap();
                            let conn_key = ConnKey {
                                host: conn.host.clone(),
                                port: conn.port,
                                username: conn.username.clone(),
                            };
                            // FR-CONN-20: open a separate connection for SFTP state sync
                            let mgr2 = self.conn_manager.clone();
                            // SAFETY: is_some() checked above
                            #[allow(clippy::unwrap_used)]
                            let conn2 = self.current_conn.clone().unwrap();
                            let conn_key2 = ConnKey {
                                host: conn2.host.clone(),
                                port: conn2.port,
                                username: conn2.username.clone(),
                            };
                            let client_id = self.client_id.clone();

                            return Task::batch([
                                Task::perform(
                                    async move {
                                        let conn_result = {
                                            let mut m = mgr.lock().await;
                                            m.get_or_connect(
                                                &conn_key,
                                                conn.identity_file.as_deref(),
                                                None,
                                                15,
                                            )
                                            .await
                                            .map_err(|e| e.to_string())?
                                        };
                                        let handle = conn_result.handle.lock().await;
                                        Ok(ssh::tmux::list_sessions_russh(&handle).await)
                                    },
                                    |result: Result<Vec<String>, String>| {
                                        Message::ExistingSessionsFound(result)
                                    },
                                ),
                                Task::perform(
                                    async move {
                                        let conn_result = {
                                            let mut m = mgr2.lock().await;
                                            m.get_or_connect(
                                                &conn_key2,
                                                conn2.identity_file.as_deref(),
                                                None,
                                                15,
                                            )
                                            .await
                                            .map_err(|e| e.to_string())?
                                        };
                                        let syncer = ssh::sftp::StateSyncer::new(
                                            conn_result.handle,
                                            &client_id,
                                        )
                                        .await?;
                                        Ok(Arc::new(syncer))
                                    },
                                    |result: Result<Arc<ssh::sftp::StateSyncer>, String>| {
                                        Message::StateSyncerReady(result)
                                    },
                                ),
                            ]);
                        }
                    }
                    Err(e) => {
                        tracing::error!("SSH tab {tab_id}: connection failed: {e}");
                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                            tab.pending_channel = None;
                            tab.connection_phase = None;
                            tab.terminal = None;
                            // FR-RECONNECT-07: classify error
                            // FR-UI-08: store last error for dead tab display
                            tab.last_error = Some(e.clone());
                            if ssh::errors::is_permanent(&e) {
                                tab.dead = true;
                                tab.auto_reconnect = false;
                            } else {
                                tab.dead = true;
                                tab.auto_reconnect = true;
                                tab.reconnect_attempts += 1;
                                tab.reconnect_started = Some(std::time::Instant::now());
                            }
                        }
                        // FR-CONN-14: helpful message when tmux is not installed
                        let el = e.to_lowercase();
                        if el.contains("tmux not found") || el.contains("tmux: not found") {
                            self.error = Some(
                                "tmux not found on server. Install it:\n\
                                 \u{2022} Debian/Ubuntu: sudo apt install tmux\n\
                                 \u{2022} Fedora/RHEL: sudo dnf install tmux\n\
                                 \u{2022} Arch: sudo pacman -S tmux\n\
                                 \u{2022} macOS: brew install tmux"
                                    .to_string(),
                            );
                        } else if (el.contains("auth failed")
                            || el.contains("no authentication method succeeded"))
                            && !self.show_password_dialog
                        {
                            // FR-CONN-09: show password prompt dialog on auth failure
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                                tab.dead = false;
                                tab.auto_reconnect = false;
                            }
                            tracing::info!("tab {tab_id}: auth failed, prompting for password");
                            self.show_password_dialog = true;
                            self.password_input.clear();
                            self.password_target_tab = Some(tab_id);
                            self.password_conn_params = self.current_conn.clone();
                        } else if el.contains("session locked by") || el.contains("lock held by") {
                            // FR-LOCK-05: show lock conflict dialog
                            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                                tab.dead = false;
                                tab.auto_reconnect = false;
                            }
                            tracing::info!("tab {tab_id}: session locked, showing conflict dialog");
                            self.show_lock_dialog = true;
                            self.lock_info_text = e.clone();
                            self.lock_target_tab = Some(tab_id);
                        } else if el.contains("auth failed") || el.contains("authentication failed")
                        {
                            // FR-CONN-17: descriptive auth error (password already tried)
                            self.error = Some(format!(
                                "Authentication failed. Check your SSH key or try a different identity file.\n\
                                 Detail: {e}"
                            ));
                        } else if el.contains("connection refused") {
                            self.error = Some(format!(
                                "Connection refused. Check the hostname and port.\nDetail: {e}"
                            ));
                        } else if el.contains("timeout") || el.contains("timed out") {
                            self.error = Some(format!(
                                "Connection timed out. The server may be down or unreachable.\nDetail: {e}"
                            ));
                        } else {
                            self.error = Some(format!("Connection failed: {e}"));
                        }
                        self.update_title();
                    }
                }
            }

            Message::ExistingSessionsFound(result) => {
                if let Err(ref e) = result {
                    tracing::warn!("failed to list existing sessions: {e}");
                }
                if let Ok(server_sessions) = result {
                    let saved_state =
                        StateFile::load_local(&StateFile::local_cache_path(&self.client_id));

                    // FR-ENV-05: restore last environment from saved state
                    if let Some(ref saved) = saved_state
                        && let Some(ref env_name) = saved.last_environment
                    {
                        self.current_environment = env_name.clone();
                    }

                    // FR-ENV-04: populate env_list from saved state
                    if let Some(ref saved) = saved_state {
                        self.env_list = saved.environments.keys().cloned().collect();
                        self.env_list.sort();
                    }

                    // FR-SESSION-08: reconcile by UUID — match saved tabs to server sessions
                    let saved_env_tabs = saved_state
                        .as_ref()
                        .map(|s| s.env_tabs(&self.current_environment))
                        .unwrap_or_default();
                    if !saved_env_tabs.is_empty() {
                        for tab in &mut self.tabs {
                            // Find saved tab entry by UUID
                            if let Some(saved_tab) = saved_env_tabs
                                .iter()
                                .find(|st| st.session_uuid == tab.session_uuid)
                            {
                                // Check if the tmux session still exists on server
                                if server_sessions.contains(&tab.tmux_session) {
                                    // Session exists — check if name was changed externally
                                    if saved_tab.tmux_session_name != tab.tmux_session {
                                        tracing::info!(
                                            "session {} renamed: {} -> {}",
                                            tab.session_uuid,
                                            saved_tab.tmux_session_name,
                                            tab.tmux_session
                                        );
                                    }
                                } else if !tab.dead
                                    && tab.terminal.is_none()
                                    && !server_sessions
                                        .iter()
                                        .any(|s| s == &saved_tab.tmux_session_name)
                                {
                                    // Tmux session is gone — mark tab as dead
                                    tracing::info!(
                                        "session {} gone from server, marking dead",
                                        tab.session_uuid
                                    );
                                    tab.dead = true;
                                    tab.auto_reconnect = false;
                                }
                            }
                        }
                    }

                    // FR-SESSION-12: find orphaned sessions (on server but not in any tab)
                    let existing_tmux: Vec<String> =
                        self.tabs.iter().map(|t| t.tmux_session.clone()).collect();
                    let orphaned: Vec<String> = server_sessions
                        .into_iter()
                        .filter(|s| !existing_tmux.contains(s))
                        .collect();

                    if !orphaned.is_empty() {
                        tracing::info!(
                            "found {} orphaned session(s): {:?}",
                            orphaned.len(),
                            orphaned
                        );
                        let mut tasks = Vec::new();
                        for (i, session_name) in orphaned.iter().enumerate() {
                            // Try to match orphan to saved state by tmux session name
                            let tab_label = saved_env_tabs
                                .iter()
                                .find(|t| t.tmux_session_name == *session_name)
                                .map(|t| t.title.clone())
                                .unwrap_or_else(|| format!("Session {}", i + 2));
                            tasks.push(self.open_tab_russh(&tab_label, session_name));
                        }
                        return Task::batch(tasks);
                    }
                }
            }

            Message::TerminalEvent(iced_term::Event::ContextMenu(_id, x, y)) => {
                self.context_menu = Some((x, y));
                self.renaming_tab = None;
                self.tab_context_menu = None;
            }

            // FR-TABS-11: context menu copy — copy selected text to clipboard
            Message::ContextMenuCopy => {
                self.context_menu = None;
                if let Some(tab) = self.tabs.get(self.active_tab)
                    && let Some(ref terminal) = tab.terminal
                {
                    let selected = terminal.selectable_content();
                    if !selected.is_empty() {
                        return iced::clipboard::write(selected);
                    }
                }
            }

            // FR-TABS-11: context menu paste — read clipboard and send to terminal
            Message::ContextMenuPaste => {
                self.context_menu = None;
                let tab_id = self.tabs.get(self.active_tab).map(|t| t.id).unwrap_or(0);
                return iced::clipboard::read().map(move |text| {
                    if let Some(text) = text {
                        Message::PasteToTerminal(tab_id, text.into_bytes())
                    } else {
                        Message::ToastDismiss // no-op
                    }
                });
            }

            // FR-TABS-11: write clipboard text to terminal (input direction)
            Message::PasteToTerminal(tab_id, data) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    terminal.handle(iced_term::Command::ProxyToBackend(
                        iced_term::BackendCommand::Write(data),
                    ));
                }
            }

            Message::ToastDismiss => {
                self.toast = None;
            }

            Message::FlushState => {
                self.flush_state();
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

            Message::HideTab(index) => {
                self.hide_tab(index);
                self.tab_context_menu = None;
            }

            Message::CloseOtherTabs(keep_index) => {
                self.tab_context_menu = None;
                let keep_id = self.tabs.get(keep_index).map(|t| t.id);
                let to_close: Vec<usize> = (0..self.tabs.len())
                    .filter(|&i| self.tabs.get(i).map(|t| t.id) != keep_id)
                    .collect();
                // FR-SESSION-10a: if any active tabs, ask confirmation
                let has_active = to_close.iter().any(|&i| {
                    self.tabs
                        .get(i)
                        .is_some_and(|t| !t.dead && t.terminal.is_some())
                });
                if has_active {
                    self.pending_close_tabs = Some(to_close);
                } else {
                    let mut tasks = Vec::new();
                    for idx in to_close.into_iter().rev() {
                        tasks.push(self.close_tab(idx));
                    }
                    self.active_tab = 0;
                    return Task::batch(tasks);
                }
            }

            Message::CloseTabsToRight(index) => {
                self.tab_context_menu = None;
                let to_close: Vec<usize> = (index + 1..self.tabs.len()).collect();
                let has_active = to_close.iter().any(|&i| {
                    self.tabs
                        .get(i)
                        .is_some_and(|t| !t.dead && t.terminal.is_some())
                });
                if has_active {
                    self.pending_close_tabs = Some(to_close);
                } else {
                    let mut tasks = Vec::new();
                    for idx in to_close.into_iter().rev() {
                        tasks.push(self.close_tab(idx));
                    }
                    return Task::batch(tasks);
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
                        tab.reconnect_started = Some(std::time::Instant::now());
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
                    && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
                    && let Some(ref resize_tx) = tab.ssh_resize_tx
                {
                    if tab.needs_initial_resize {
                        tracing::info!(
                            "tab {id}: initial terminal size {cols}x{rows}, sending to SSH"
                        );
                        tab.needs_initial_resize = false;
                    }
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

            // FR-SESSION-10a: close tab with confirmation for active sessions
            Message::CloseTab(index) => {
                self.tab_context_menu = None;
                if let Some(tab) = self.tabs.get(index)
                    && !tab.dead
                    && tab.terminal.is_some()
                {
                    // Active session — ask confirmation
                    self.pending_close_tabs = Some(vec![index]);
                    return Task::none();
                }
                // Dead/disconnected — close immediately
                return self.close_tab(index);
            }

            Message::ConfirmCloseTabs => {
                if let Some(indices) = self.pending_close_tabs.take() {
                    let mut tasks = Vec::new();
                    // Close from end to avoid index shifting
                    for idx in indices.into_iter().rev() {
                        tasks.push(self.close_tab(idx));
                    }
                    return Task::batch(tasks);
                }
            }

            Message::CancelCloseTabs => {
                self.pending_close_tabs = None;
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

            // FR-UI-07: create a fresh session replacing a dead tab
            Message::CreateNewSession(index) => {
                if index < self.tabs.len() && self.current_conn.is_some() {
                    let tab = &self.tabs[index];
                    let label = tab.label.clone();
                    // Reuse same tmux session name prefix but generate new UUID
                    let tmux_session = self.next_tmux_session();
                    // Remove the dead tab
                    self.tabs.remove(index);
                    if self.active_tab >= self.tabs.len() && self.active_tab > 0 {
                        self.active_tab -= 1;
                    }
                    // Open fresh tab
                    let task = self.open_tab_russh(&label, &tmux_session);
                    // Move the new tab to the original position
                    if self.tabs.len() > 1 && index < self.tabs.len() {
                        // SAFETY: len() > 1 guarantees pop() returns Some
                        #[allow(clippy::unwrap_used)]
                        let new_tab = self.tabs.pop().unwrap();
                        self.tabs.insert(index, new_tab);
                        self.active_tab = index;
                        self.update_title();
                    }
                    return task;
                }
            }

            Message::AutoReconnectTick => {
                // FR-RECONNECT-05: limit concurrent reconnections to 5
                let reconnecting_count = self
                    .tabs
                    .iter()
                    .filter(|t| {
                        t.uses_russh
                            && t.terminal.is_some()
                            && t.ssh_channel_holder.is_none()
                            && t.pending_channel.is_some()
                    })
                    .count();

                if reconnecting_count >= 5 {
                    tracing::debug!(
                        "skipping auto-reconnect: {reconnecting_count} already in progress"
                    );
                    return Task::none();
                }

                let reconnect_indices: Vec<usize> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.terminal.is_none() && t.auto_reconnect && !t.dead)
                    .map(|(i, _)| i)
                    .collect();

                if let Some(&index) = reconnect_indices.first() {
                    // FR-RECONNECT-06: exponential backoff with jitter
                    let base_ms = (self.config.ssh.reconnect_backoff_base * 1000.0) as u64;
                    let attempt = self.tabs[index].reconnect_attempts;
                    let exp_delay = base_ms.saturating_mul(
                        1u64.checked_shl(attempt.saturating_sub(1))
                            .unwrap_or(u64::MAX),
                    );
                    let capped = exp_delay.min(60_000);
                    use rand::Rng;
                    let jitter_range = capped / 4;
                    let jitter = if jitter_range > 0 {
                        rand::rng().random_range(0..jitter_range * 2) as i64
                            - jitter_range as i64
                    } else {
                        0
                    };
                    let next_delay = (capped as i64 + jitter).max(base_ms as i64) as u64;
                    self.tabs[index].reconnect_delay_ms = next_delay;
                    tracing::info!(
                        "auto-reconnecting tab {} (attempt {}, next delay {}ms)",
                        self.tabs[index].id,
                        self.tabs[index].reconnect_attempts,
                        next_delay,
                    );
                    return self.reconnect_tab(index);
                }
            }

            // FR-RECONNECT-08: network change detected — force immediate reconnect
            Message::NetworkChanged => {
                #[cfg(target_os = "linux")]
                {
                    let current = read_default_gateway();
                    if current != self.last_gateway {
                        tracing::info!(
                            "network change detected (gateway {:?} -> {:?}), triggering immediate reconnect",
                            self.last_gateway,
                            current
                        );
                        self.last_gateway = current;
                        for tab in &mut self.tabs {
                            if tab.terminal.is_none() && tab.auto_reconnect && !tab.dead {
                                tab.reconnect_delay_ms = 0;
                                tab.reconnect_attempts = 0;
                            }
                        }
                    }
                }
            }

            Message::RenameInputChanged(v) => {
                self.rename_input = v;
            }

            Message::FinishRename => {
                let mut rename_task = Task::none();
                if let Some(index) = self.renaming_tab
                    && index < self.tabs.len()
                    && !self.rename_input.trim().is_empty()
                {
                    let new_label = self.rename_input.trim().to_string();
                    let old_tmux = self.tabs[index].tmux_session.clone();
                    self.tabs[index].label = new_label.clone();
                    self.update_title();
                    self.save_state();

                    // FR-SESSION-06: also rename tmux session on the server
                    if self.tabs[index].uses_russh && self.tabs[index].conn_params.is_some() {
                        let sanitized: String = new_label
                            .chars()
                            .map(|c| {
                                if c.is_alphanumeric() || c == '-' || c == '_' {
                                    c
                                } else {
                                    '-'
                                }
                            })
                            .collect();
                        let new_tmux = format!("{}--shellkeep-{}", self.client_id, sanitized);
                        self.tabs[index].tmux_session = new_tmux.clone();
                        let mgr = self.conn_manager.clone();
                        // SAFETY: conn_params.is_some() checked in the enclosing if-let
                        #[allow(clippy::unwrap_used)]
                        let conn = self.tabs[index].conn_params.clone().unwrap();
                        rename_task = Task::perform(
                            async move {
                                let conn_key = ConnKey {
                                    host: conn.host.clone(),
                                    port: conn.port,
                                    username: conn.username.clone(),
                                };
                                let mgr_guard = mgr.lock().await;
                                if let Some(handle_arc) = mgr_guard.get_cached(&conn_key) {
                                    let handle = handle_arc.lock().await;
                                    let cmd = format!(
                                        "tmux rename-session -t {} {} 2>/dev/null || true",
                                        old_tmux, new_tmux
                                    );
                                    let _ = ssh::connection::exec_command(&handle, &cmd).await;
                                }
                            },
                            |_| Message::ContextMenuDismiss,
                        );
                    }
                }
                self.renaming_tab = None;
                return rename_task;
            }

            // FR-UI-01: toggle advanced connection options
            Message::ToggleAdvanced => {
                self.show_advanced = !self.show_advanced;
            }
            // FR-UI-03: client-id naming
            Message::ClientIdInputChanged(v) => {
                // Validate: only [a-zA-Z0-9_-], max 64 chars
                let filtered: String = v
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .take(64)
                    .collect();
                self.client_id_input = filtered;
            }
            Message::HostInputChanged(v) => {
                // Detect pasted SSH commands: "ssh -p 2247 user@host" or "ssh user@host -i key"
                let trimmed = v.trim();
                if trimmed.starts_with("ssh ") {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    let mut host = String::new();
                    let mut port = String::new();
                    let mut user = String::new();
                    let mut identity = String::new();
                    let mut i = 1; // skip "ssh"
                    while i < parts.len() {
                        match parts[i] {
                            "-p" if i + 1 < parts.len() => {
                                port = parts[i + 1].to_string();
                                i += 2;
                            }
                            "-i" if i + 1 < parts.len() => {
                                identity = parts[i + 1].to_string();
                                i += 2;
                            }
                            "-l" if i + 1 < parts.len() => {
                                user = parts[i + 1].to_string();
                                i += 2;
                            }
                            arg if !arg.starts_with('-') => {
                                // user@host or just host
                                if let Some((u, h)) = arg.split_once('@') {
                                    if user.is_empty() {
                                        user = u.to_string();
                                    }
                                    host = h.to_string();
                                } else {
                                    host = arg.to_string();
                                }
                                i += 1;
                            }
                            _ => {
                                i += 1; // skip unknown flags
                            }
                        }
                    }
                    self.host_input = host;
                    if !port.is_empty() {
                        self.port_input = port;
                    }
                    if !user.is_empty() {
                        self.user_input = user;
                    }
                    if !identity.is_empty() {
                        self.identity_input = identity;
                    }
                    // Auto-show advanced panel if non-default values were parsed
                    if self.port_input != "22"
                        || !self.user_input.is_empty()
                        || !self.identity_input.is_empty()
                    {
                        self.show_advanced = true;
                    }
                } else {
                    self.host_input = v;
                }
            }
            Message::PortInputChanged(v) => self.port_input = v,
            Message::UserInputChanged(v) => self.user_input = v,
            Message::IdentityInputChanged(v) => self.identity_input = v,

            Message::Connect => {
                if self.host_input.trim().is_empty() {
                    return Task::none();
                }
                // FR-UI-03: if user provided a client-id name on first use, save it
                if !self.client_id_input.is_empty() && self.client_id_input != self.client_id {
                    self.client_id = self.client_id_input.clone();
                    if let Err(e) = shellkeep::state::client_id::save_client_id(&self.client_id) {
                        tracing::warn!("failed to save client-id: {e}");
                    }
                }
                let ssh_args = self.build_ssh_args();
                let label = ssh_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ssh_args.join(" "));

                // Store connection params
                let (parsed_user, parsed_host, parsed_port) =
                    cli::parse_host_input(self.host_input.trim());
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
                    identity_file: if self.identity_input.is_empty() {
                        None
                    } else {
                        Some(self.identity_input.clone())
                    },
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

            // FR-UI-01: clicking a recent connection fills the form (user reviews, then clicks Connect)
            Message::ConnectRecent(index) => {
                if let Some(conn) = self.recent.connections.get(index).cloned() {
                    self.host_input = conn.host;
                    self.user_input = conn.user;
                    self.port_input = conn.port;
                    self.identity_input = conn.identity_file.unwrap_or_default();
                    // Show advanced if non-default port or identity is set
                    if self.port_input != "22" || !self.identity_input.is_empty() {
                        self.show_advanced = true;
                    }
                }
            }

            Message::ConnectionPhaseTick => {
                // Just triggers a redraw to update connection phase text
            }

            Message::SpinnerTick => {
                // FR-RECONNECT-02: advance spinner frame
                self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
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
                    // Ctrl+Shift+W — close current tab (goes through confirmation)
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("w".into())
                        && !self.tabs.is_empty()
                    {
                        return self.update(Message::CloseTab(self.active_tab));
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
                    // Ctrl+Shift+F — toggle scrollback search (FR-TABS-09)
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("f".into())
                    {
                        return self.update(Message::SearchToggle);
                    }
                    // Ctrl+Shift+S — export scrollback to file (FR-TERMINAL-18)
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("s".into())
                        && !self.tabs.is_empty()
                    {
                        return self.update(Message::ExportScrollback);
                    }
                    // Ctrl+Shift+A — copy entire scrollback to clipboard (FR-TABS-12)
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("a".into())
                        && !self.tabs.is_empty()
                    {
                        return self.update(Message::CopyScrollback);
                    }
                    // Enter/Escape on close-tab confirmation dialog
                    if self.pending_close_tabs.is_some() {
                        if key == keyboard::Key::Named(keyboard::key::Named::Enter) {
                            return self.update(Message::ConfirmCloseTabs);
                        }
                        if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                            return self.update(Message::CancelCloseTabs);
                        }
                    }
                    // Escape — dismiss search, context menu, cancel rename, or cancel welcome
                    if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                        if self.search_active {
                            return self.update(Message::SearchClose);
                        } else if self.context_menu.is_some() {
                            self.context_menu = None;
                        } else if self.renaming_tab.is_some() {
                            self.renaming_tab = None;
                        } else if self.show_welcome && !self.tabs.is_empty() {
                            self.show_welcome = false;
                        }
                    }
                    // Tab / Shift+Tab — cycle focus between form inputs on dialogs/welcome
                    if key == keyboard::Key::Named(keyboard::key::Named::Tab)
                        && (self.show_welcome
                            || self.show_env_dialog
                            || self.show_new_env_dialog
                            || self.show_rename_env_dialog
                            || self.search_active
                            || self.renaming_tab.is_some())
                    {
                        return if modifiers.shift() {
                            iced_runtime::widget::operation::focus_previous()
                        } else {
                            iced_runtime::widget::operation::focus_next()
                        };
                    }
                }
            }

            // FR-TRAY-01: poll tray menu events
            Message::TrayPoll => {
                if let Some(ref tray) = self.tray {
                    match tray.poll_event() {
                        Some(TrayAction::ShowWindow) => {
                            tracing::debug!("tray: show window requested");
                        }
                        Some(TrayAction::HideWindow) => {
                            tracing::debug!("tray: hide window requested");
                        }
                        Some(TrayAction::Quit) => {
                            tracing::info!("tray: quit requested");
                            std::process::exit(0);
                        }
                        None => {}
                    }
                }
            }

            Message::SearchToggle => {
                self.search_active = !self.search_active;
                if !self.search_active {
                    self.search_input.clear();
                    self.search_regex = None;
                    self.search_last_match = None;
                } else {
                    return iced_runtime::widget::operation::focus("search-input");
                }
            }

            Message::SearchInputChanged(v) => {
                self.search_input = v;
                if self.search_input.is_empty() {
                    self.search_regex = None;
                    self.search_last_match = None;
                } else {
                    let escaped = app::escape_regex(&self.search_input);
                    self.search_regex = RegexSearch::new(&escaped).ok();
                    if self.search_regex.is_some() {
                        return self.update(Message::SearchNext);
                    }
                }
            }

            Message::SearchNext => {
                if let Some(ref mut regex) = self.search_regex
                    && let Some(tab) = self.tabs.get_mut(self.active_tab)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    let origin = self
                        .search_last_match
                        .as_ref()
                        .map(|m| {
                            let mut p = *m.end();
                            p.column.0 += 1;
                            p
                        })
                        .unwrap_or(AlacrittyPoint::new(AlacrittyLine(0), AlacrittyColumn(0)));
                    self.search_last_match = terminal.search_next(regex, origin);
                }
            }

            Message::SearchPrev => {
                if let Some(ref mut regex) = self.search_regex
                    && let Some(tab) = self.tabs.get_mut(self.active_tab)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    let origin = self
                        .search_last_match
                        .as_ref()
                        .map(|m| {
                            let mut p = *m.start();
                            if p.column.0 > 0 {
                                p.column.0 -= 1;
                            } else {
                                p.line -= 1i32;
                            }
                            p
                        })
                        .unwrap_or(AlacrittyPoint::new(AlacrittyLine(0), AlacrittyColumn(0)));
                    self.search_last_match = terminal.search_prev(regex, origin);
                }
            }

            Message::SearchClose => {
                self.search_active = false;
                self.search_input.clear();
                self.search_regex = None;
                self.search_last_match = None;
            }

            // FR-TERMINAL-18: export scrollback to text file
            Message::ExportScrollback => {
                if let Some(tab) = self.tabs.get(self.active_tab)
                    && let Some(ref terminal) = tab.terminal
                {
                    let text = terminal.scrollback_text();
                    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
                    let filename = format!("shellkeep-export-{timestamp}.txt");
                    let path = dirs::home_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join(&filename);
                    match std::fs::write(&path, &text) {
                        Ok(()) => {
                            tracing::info!("exported scrollback to {}", path.display());
                            self.toast = Some((
                                format!("Scrollback exported to {}", path.display()),
                                std::time::Instant::now(),
                            ));
                        }
                        Err(e) => {
                            tracing::error!("failed to export scrollback: {e}");
                            self.error = Some(format!("Export failed: {e}"));
                        }
                    }
                }
            }

            // FR-TABS-12: copy entire scrollback to clipboard
            Message::CopyScrollback => {
                if let Some(tab) = self.tabs.get(self.active_tab)
                    && let Some(ref terminal) = tab.terminal
                {
                    let text = terminal.scrollback_text();
                    self.toast = Some((
                        "Scrollback copied to clipboard".to_string(),
                        std::time::Instant::now(),
                    ));
                    return iced::clipboard::write(text);
                }
            }

            // FR-CONFIG-04: config file changed, reload hot-reloadable settings
            Message::ConfigReloaded => {
                // Check if the watcher actually signaled a change
                let changed = self
                    .config_reload_rx
                    .as_ref()
                    .is_some_and(|rx| rx.try_recv().is_ok());
                if !changed {
                    return Task::none();
                }

                let new_config = Config::load();
                tracing::info!("config reloaded from disk");

                // Hot-reload font size/family
                if (new_config.terminal.font_size - self.config.terminal.font_size).abs() > 0.1
                    || new_config.terminal.font_family != self.config.terminal.font_family
                {
                    self.current_font_size = new_config.terminal.font_size;
                    // Apply to all open terminals
                    let new_font = FontSettings {
                        size: new_config.terminal.font_size,
                        font_family: new_config.terminal.font_family.clone(),
                        ..FontSettings::default()
                    };
                    for tab in &mut self.tabs {
                        if let Some(ref mut terminal) = tab.terminal {
                            terminal.handle(iced_term::Command::ChangeFont(new_font.clone()));
                        }
                    }
                    tracing::info!("font updated: size={}", new_config.terminal.font_size);
                }

                // Hot-reload tray settings
                if new_config.tray.enabled != self.config.tray.enabled {
                    if new_config.tray.enabled {
                        self.tray = Tray::new(true);
                    } else {
                        self.tray = None;
                    }
                    tracing::info!("tray enabled={}", new_config.tray.enabled);
                }

                // Note: scrollback_lines is NOT hot-reloadable (requires terminal recreation)
                if new_config.terminal.scrollback_lines != self.config.terminal.scrollback_lines {
                    tracing::info!(
                        "scrollback_lines changed {} -> {} (requires restart to take effect)",
                        self.config.terminal.scrollback_lines,
                        new_config.terminal.scrollback_lines
                    );
                }

                self.config = new_config;
                self.toast = Some(("Configuration reloaded".into(), std::time::Instant::now()));
            }

            // FR-LOCK-04: periodic heartbeat to keep the lock alive
            Message::LockHeartbeatTick => {
                let mgr = self.conn_manager.clone();
                let cid = self.client_id.clone();
                let conn = match &self.current_conn {
                    Some(c) => c.clone(),
                    None => return Task::none(),
                };
                let conn_key = ConnKey {
                    host: conn.host.clone(),
                    port: conn.port,
                    username: conn.username.clone(),
                };
                return Task::perform(
                    async move {
                        let mgr = mgr.lock().await;
                        if let Some(handle_arc) = mgr.get_cached(&conn_key) {
                            let handle = handle_arc.lock().await;
                            ssh::lock::heartbeat(&handle, &cid)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            Ok(()) // No connection, skip heartbeat
                        }
                    },
                    Message::LockHeartbeatDone,
                );
            }

            Message::LockHeartbeatDone(result) => {
                if let Err(e) = result {
                    tracing::warn!("lock heartbeat failed: {e}");
                }
            }

            // FR-UI-04/05: latency measurement
            Message::LatencyTick => {
                let mgr = self.conn_manager.clone();
                let conn = match &self.current_conn {
                    Some(c) => c.clone(),
                    None => return Task::none(),
                };
                let conn_key = ConnKey {
                    host: conn.host.clone(),
                    port: conn.port,
                    username: conn.username.clone(),
                };
                // Collect tab IDs that are connected via russh
                let tab_ids: Vec<u64> = self
                    .tabs
                    .iter()
                    .filter(|t| t.uses_russh && !t.dead && t.terminal.is_some())
                    .map(|t| t.id)
                    .collect();
                if tab_ids.is_empty() {
                    return Task::none();
                }
                return Task::perform(
                    async move {
                        let mgr = mgr.lock().await;
                        let latency = if let Some(handle_arc) = mgr.get_cached(&conn_key) {
                            let handle = handle_arc.lock().await;
                            let start = std::time::Instant::now();
                            match ssh::connection::exec_command(&handle, "true").await {
                                Ok(_) => Some(start.elapsed().as_millis() as u32),
                                Err(_) => None,
                            }
                        } else {
                            None
                        };
                        (tab_ids, latency)
                    },
                    move |(ids, latency): (Vec<u64>, Option<u32>)| {
                        // Send measurement for the first tab; the update handler
                        // applies the same latency to all tabs on this connection.
                        if let Some(&first) = ids.first() {
                            Message::LatencyMeasured(first, latency)
                        } else {
                            Message::LatencyMeasured(0, None)
                        }
                    },
                );
            }

            Message::LatencyMeasured(_, latency) => {
                // All tabs on the same connection share the same latency
                if let Some(conn) = &self.current_conn {
                    let _host = &conn.host;
                    for tab in &mut self.tabs {
                        if tab.uses_russh && !tab.dead && tab.terminal.is_some() {
                            tab.last_latency_ms = latency;
                        }
                    }
                }
            }

            // FR-TABS-17: window close requested by window manager
            Message::WindowCloseRequested(win_id) => {
                let active_count = self
                    .tabs
                    .iter()
                    .filter(|t| !t.dead && t.terminal.is_some())
                    .count();
                if active_count == 0 {
                    // FR-TABS-18: no active sessions, close immediately
                    self.flush_state();
                    return window::close(win_id);
                }
                // Show confirmation dialog, remember which window to close
                self.close_window_id = Some(win_id);
                self.show_close_dialog = true;
            }

            Message::CloseDialogClose => {
                self.show_close_dialog = false;
                self.flush_state();
                // FR-LOCK-10: lock is released via orphan detection (2x keepalive timeout)
                // when the SSH connection drops on process exit.
                if let Some(id) = self.close_window_id.take() {
                    return window::close(id);
                }
                std::process::exit(0);
            }

            Message::CloseDialogCancel => {
                self.show_close_dialog = false;
                self.close_window_id = None;
            }

            // FR-STATE-14: track window geometry changes
            Message::WindowMoved(pos) => {
                self.window_x = Some(pos.x as i32);
                self.window_y = Some(pos.y as i32);
                self.save_geometry();
            }

            Message::WindowResized(size) => {
                self.window_width = size.width as u32;
                self.window_height = size.height as u32;
                self.save_geometry();
            }

            // FR-ENV-03: environment selection dialog
            Message::ShowEnvDialog => {
                // FR-ENV-04: if only one environment, select it directly
                if self.env_list.len() == 1 {
                    let env_name = self.env_list[0].clone();
                    if env_name != self.current_environment {
                        return self.update(Message::SwitchEnvironment(env_name));
                    }
                    return Task::none();
                }
                self.show_env_dialog = true;
                self.env_filter.clear();
                // Pre-select current environment
                self.selected_env = Some(self.current_environment.clone());
            }

            Message::EnvFilterChanged(filter) => {
                self.env_filter = filter;
            }

            Message::SelectEnv(name) => {
                self.selected_env = Some(name);
            }

            Message::ConfirmEnv => {
                if let Some(ref env_name) = self.selected_env {
                    let env_name = env_name.clone();
                    self.show_env_dialog = false;
                    if env_name != self.current_environment {
                        return self.update(Message::SwitchEnvironment(env_name));
                    }
                }
            }

            Message::NewEnvFromDialog => {
                // Close env selection, open new-env creation
                self.show_env_dialog = false;
                self.new_env_input.clear();
                self.show_new_env_dialog = true;
            }

            Message::CancelEnvDialog => {
                self.show_env_dialog = false;
            }

            // FR-ENV-07: create new environment
            Message::ShowNewEnvDialog => {
                self.new_env_input.clear();
                self.show_new_env_dialog = true;
            }

            Message::NewEnvInputChanged(input) => {
                self.new_env_input = input;
            }

            Message::ConfirmNewEnv => {
                let name = self.new_env_input.trim().to_string();
                if !name.is_empty() && !self.env_list.contains(&name) {
                    self.env_list.push(name.clone());
                    self.env_list.sort();
                    self.current_environment = name;
                    self.toast = Some((
                        format!("Environment \"{}\" created", self.current_environment),
                        std::time::Instant::now(),
                    ));
                    self.state_dirty = true;
                    self.flush_state();
                }
                self.show_new_env_dialog = false;
                self.new_env_input.clear();
            }

            Message::CancelNewEnv => {
                self.show_new_env_dialog = false;
                self.new_env_input.clear();
            }

            // FR-ENV-08: rename environment
            Message::ShowRenameEnvDialog(name) => {
                self.rename_env_target = Some(name.clone());
                self.rename_env_input = name;
                self.show_rename_env_dialog = true;
            }

            Message::RenameEnvInputChanged(input) => {
                self.rename_env_input = input;
            }

            Message::ConfirmRenameEnv => {
                let new_name = self.rename_env_input.trim().to_string();
                if let Some(ref old_name) = self.rename_env_target
                    && !new_name.is_empty()
                    && new_name != *old_name
                {
                    if let Some(entry) = self.env_list.iter_mut().find(|e| *e == old_name) {
                        *entry = new_name.clone();
                    }
                    self.env_list.sort();
                    if self.current_environment == *old_name {
                        self.current_environment = new_name.clone();
                    }
                    self.toast = Some((
                        format!("Environment renamed to \"{new_name}\""),
                        std::time::Instant::now(),
                    ));
                    self.state_dirty = true;
                    self.flush_state();
                }
                self.show_rename_env_dialog = false;
                self.rename_env_input.clear();
                self.rename_env_target = None;
            }

            Message::CancelRenameEnv => {
                self.show_rename_env_dialog = false;
                self.rename_env_input.clear();
                self.rename_env_target = None;
            }

            // FR-ENV-09: delete environment
            Message::ShowDeleteEnvDialog(name) => {
                self.delete_env_target = Some(name);
                self.show_delete_env_dialog = true;
            }

            Message::ConfirmDeleteEnv => {
                if let Some(ref name) = self.delete_env_target {
                    let name = name.clone();
                    self.env_list.retain(|e| *e != name);
                    if self.current_environment == name {
                        self.current_environment = self
                            .env_list
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "default".to_string());
                    }
                    self.toast = Some((
                        format!("Environment \"{}\" deleted", name),
                        std::time::Instant::now(),
                    ));
                    self.state_dirty = true;
                    self.flush_state();
                }
                self.show_delete_env_dialog = false;
                self.delete_env_target = None;
            }

            Message::CancelDeleteEnv => {
                self.show_delete_env_dialog = false;
                self.delete_env_target = None;
            }

            // FR-ENV-10: switch active environment
            Message::SwitchEnvironment(name) => {
                if name != self.current_environment {
                    tracing::info!(
                        "switching environment: {} -> {}",
                        self.current_environment,
                        name
                    );
                    // Save current tabs for the current environment
                    self.flush_state();
                    // Switch to the new environment
                    self.current_environment = name;
                    // TODO: load tabs for the new environment from state
                    self.state_dirty = true;
                    self.update_title();
                    self.toast = Some((
                        format!("Switched to \"{}\" environment", self.current_environment),
                        std::time::Instant::now(),
                    ));
                }
            }

            // FR-CONN-20: state syncer initialized
            Message::StateSyncerReady(result) => {
                match result {
                    Ok(syncer) => {
                        let transport = if syncer.is_sftp() { "SFTP" } else { "shell" };
                        tracing::info!("state syncer ready (transport: {transport})");
                        let syncer_clone = syncer.clone();
                        self.state_syncer = Some(syncer);
                        // FR-STATE-02: read server state (takes precedence over local)
                        return Task::perform(
                            async move { syncer_clone.read_state().await },
                            Message::ServerStateLoaded,
                        );
                    }
                    Err(e) => {
                        tracing::warn!("state syncer init failed: {e}");
                    }
                }
            }

            // FR-STATE-02: server state loaded
            Message::ServerStateLoaded(result) => {
                match result {
                    Ok(Some(json)) => {
                        match serde_json::from_str::<StateFile>(&json) {
                            Ok(server_state) => {
                                tracing::info!(
                                    "loaded server state: {} tabs",
                                    server_state.tabs.len()
                                );
                                // Server state takes precedence — update local cache
                                let path = StateFile::local_cache_path(&self.client_id);
                                if let Err(e) = server_state.save_local(&path) {
                                    tracing::warn!("failed to cache server state: {e}");
                                }
                            }
                            Err(e) => {
                                tracing::warn!("corrupt server state: {e}");
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::debug!("no server state found, using local");
                    }
                    Err(e) => {
                        tracing::warn!("failed to read server state: {e}");
                    }
                }
            }

            // FR-CONN-03: host key TOFU — accept and save to known_hosts
            Message::HostKeyAcceptSave => {
                self.pending_host_key_prompt = None;
            }
            Message::HostKeyConnectOnce => {
                if let Some(ref prompt) = self.pending_host_key_prompt {
                    let _ = ssh::known_hosts::remove_host_key(&prompt.host, prompt.port);
                }
                self.pending_host_key_prompt = None;
            }
            Message::HostKeyReject => {
                if let Some(ref prompt) = self.pending_host_key_prompt {
                    let _ = ssh::known_hosts::remove_host_key(&prompt.host, prompt.port);
                }
                self.pending_host_key_prompt = None;
                for tab in &mut self.tabs {
                    tab.dead = true;
                    tab.auto_reconnect = false;
                    tab.last_error = Some("Host key rejected by user".to_string());
                }
                self.error = Some("Connection cancelled: host key rejected.".to_string());
            }
            Message::HostKeyChangedDismiss => {
                self.pending_host_key_prompt = None;
            }

            // FR-CONN-09: password auth dialog
            Message::PasswordInputChanged(val) => {
                self.password_input = val;
            }
            Message::PasswordSubmit => {
                self.show_password_dialog = false;
                let password = self.password_input.clone();
                self.password_input.clear();

                if let Some(tab_id) = self.password_target_tab.take()
                    && let Some(conn) = self
                        .password_conn_params
                        .take()
                        .or(self.current_conn.clone())
                {
                    let mgr = self.conn_manager.clone();
                    let conn_key = ConnKey {
                        host: conn.host.clone(),
                        port: conn.port,
                        username: conn.username.clone(),
                    };
                    {
                        let mut m = mgr.blocking_lock();
                        m.remove(&conn_key);
                    }

                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        let phase = Arc::new(std::sync::Mutex::new(String::new()));
                        tab.connection_phase = Some(phase.clone());
                        tab.dead = false;
                        tab.last_error = None;

                        let tmux_session = tab.tmux_session.clone();
                        let client_id = self.client_id.clone();
                        let session_uuid = tab.session_uuid.clone();
                        let keepalive = self.config.ssh.keepalive_interval;
                        let channel_holder: ChannelHolder = Arc::new(Mutex::new(None));
                        tab.pending_channel = Some(channel_holder.clone());

                        return Task::perform(
                            async move {
                                let conn_result = {
                                    let mut m = mgr.lock().await;
                                    m.get_or_connect(
                                        &conn_key,
                                        conn.identity_file.as_deref(),
                                        Some(&password),
                                        keepalive,
                                    )
                                    .await
                                    .map_err(|e| e.to_string())?
                                };
                                let handle = conn_result.handle.lock().await;

                                // SAFETY: mutex is never held across a panic path
                                #[allow(clippy::unwrap_used)]
                                {
                                    *phase.lock().unwrap() = "Opening session...".to_string();
                                }

                                ssh::lock::acquire_lock(
                                    &handle,
                                    &client_id,
                                    Some(keepalive as u64),
                                )
                                .await
                                .map_err(|e| e.to_string())?;

                                let check = ssh::connection::exec_command(
                                        &handle,
                                        &format!(
                                            "tmux has-session -t {tmux_session} 2>/dev/null && echo EXISTS"
                                        ),
                                    )
                                    .await
                                    .unwrap_or_default();

                                if !check.trim().contains("EXISTS") {
                                    ssh::tmux::create_session_russh(&handle, &tmux_session)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                }

                                let channel = handle
                                    .channel_open_session()
                                    .await
                                    .map_err(|e| format!("channel: {e}"))?;
                                channel
                                    .request_pty(false, "xterm-256color", 80, 24, 0, 0, &[])
                                    .await
                                    .map_err(|e| format!("pty: {e}"))?;
                                let tmux_cmd = format!(
                                    "TERM=xterm-256color tmux new-session -A -s {tmux_session} \\; set status off || exec $SHELL"
                                );
                                channel
                                    .exec(true, tmux_cmd)
                                    .await
                                    .map_err(|e| format!("exec: {e}"))?;

                                let pipe_cmd =
                                    history::pipe_pane_command(&tmux_session, &session_uuid);
                                if let Err(e) =
                                    ssh::connection::exec_command(&handle, &pipe_cmd).await
                                {
                                    tracing::warn!("failed to setup history pipe-pane: {e}");
                                }

                                *channel_holder.lock().await = Some(channel);
                                Ok(())
                            },
                            move |result: Result<(), String>| Message::SshConnected(tab_id, result),
                        );
                    }
                }
            }
            Message::PasswordCancel => {
                self.show_password_dialog = false;
                self.password_input.clear();
                if let Some(tab_id) = self.password_target_tab.take()
                    && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.dead = true;
                    tab.auto_reconnect = false;
                    tab.last_error = Some("Authentication cancelled".to_string());
                }
                self.error = Some("Authentication cancelled.".to_string());
            }

            // FR-LOCK-05: lock conflict — take over
            Message::LockTakeOver => {
                self.show_lock_dialog = false;
                if let Some(tab_id) = self.lock_target_tab.take()
                    && let Some(conn) = self.current_conn.clone()
                {
                    let mgr = self.conn_manager.clone();
                    let conn_key = ConnKey {
                        host: conn.host.clone(),
                        port: conn.port,
                        username: conn.username.clone(),
                    };
                    let client_id = self.client_id.clone();

                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        let phase = Arc::new(std::sync::Mutex::new(String::new()));
                        tab.connection_phase = Some(phase.clone());
                        tab.dead = false;
                        tab.last_error = None;

                        let tmux_session = tab.tmux_session.clone();
                        let session_uuid = tab.session_uuid.clone();
                        let keepalive = self.config.ssh.keepalive_interval;
                        let channel_holder: ChannelHolder = Arc::new(Mutex::new(None));
                        tab.pending_channel = Some(channel_holder.clone());

                        return Task::perform(
                            async move {
                                let conn_result = {
                                    let mut m = mgr.lock().await;
                                    m.get_or_connect(
                                        &conn_key,
                                        conn.identity_file.as_deref(),
                                        None,
                                        keepalive,
                                    )
                                    .await
                                    .map_err(|e| e.to_string())?
                                };
                                let handle = conn_result.handle.lock().await;

                                // SAFETY: mutex is never held across a panic path
                                #[allow(clippy::unwrap_used)]
                                {
                                    *phase.lock().unwrap() = "Taking over lock...".to_string();
                                }
                                ssh::lock::release_lock(&handle, &client_id)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                ssh::lock::acquire_lock(
                                    &handle,
                                    &client_id,
                                    Some(keepalive as u64),
                                )
                                .await
                                .map_err(|e| e.to_string())?;

                                // SAFETY: mutex is never held across a panic path
                                #[allow(clippy::unwrap_used)]
                                {
                                    *phase.lock().unwrap() = "Opening session...".to_string();
                                }

                                let check = ssh::connection::exec_command(
                                        &handle,
                                        &format!(
                                            "tmux has-session -t {tmux_session} 2>/dev/null && echo EXISTS"
                                        ),
                                    )
                                    .await
                                    .unwrap_or_default();

                                if !check.trim().contains("EXISTS") {
                                    ssh::tmux::create_session_russh(&handle, &tmux_session)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                }

                                let channel = handle
                                    .channel_open_session()
                                    .await
                                    .map_err(|e| format!("channel: {e}"))?;
                                channel
                                    .request_pty(false, "xterm-256color", 80, 24, 0, 0, &[])
                                    .await
                                    .map_err(|e| format!("pty: {e}"))?;
                                let tmux_cmd = format!(
                                    "TERM=xterm-256color tmux new-session -A -s {tmux_session} \\; set status off || exec $SHELL"
                                );
                                channel
                                    .exec(true, tmux_cmd)
                                    .await
                                    .map_err(|e| format!("exec: {e}"))?;

                                let pipe_cmd =
                                    history::pipe_pane_command(&tmux_session, &session_uuid);
                                if let Err(e) =
                                    ssh::connection::exec_command(&handle, &pipe_cmd).await
                                {
                                    tracing::warn!("failed to setup history pipe-pane: {e}");
                                }

                                *channel_holder.lock().await = Some(channel);
                                Ok(())
                            },
                            move |result: Result<(), String>| Message::SshConnected(tab_id, result),
                        );
                    }
                }
            }
            Message::LockCancel => {
                self.show_lock_dialog = false;
                if let Some(tab_id) = self.lock_target_tab.take()
                    && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.dead = true;
                    tab.auto_reconnect = false;
                    tab.last_error = Some("Lock takeover cancelled".to_string());
                }
            }
        }
        Task::none()
    }

    /// FR-STATE-14: save window geometry (debounced)
    fn save_geometry(&mut self) {
        if let Some(last) = self.last_geometry_save
            && last.elapsed() < std::time::Duration::from_millis(500)
        {
            self.state_dirty = true;
            return;
        }
        self.last_geometry_save = Some(std::time::Instant::now());
        self.state_dirty = true;
        self.flush_state();
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

        // FR-CONN-16: poll for connection phase updates
        if self.tabs.iter().any(|t| t.connection_phase.is_some()) {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(200))
                    .map(|_| Message::ConnectionPhaseTick),
            );
        }

        subs.push(keyboard::listen().map(Message::KeyEvent));

        // FR-RECONNECT-02: spinner animation subscription (100ms tick)
        let any_reconnecting = self
            .tabs
            .iter()
            .any(|t| t.terminal.is_none() && t.auto_reconnect && !t.dead);
        if any_reconnecting {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(100))
                    .map(|_| Message::SpinnerTick),
            );
        }

        // FR-RECONNECT-06: exponential backoff auto-reconnect timer
        if let Some(delay_ms) = self
            .tabs
            .iter()
            .filter(|t| t.terminal.is_none() && t.auto_reconnect && !t.dead)
            .map(|t| {
                if t.reconnect_delay_ms == 0 {
                    (self.config.ssh.reconnect_backoff_base * 1000.0) as u64
                } else {
                    t.reconnect_delay_ms
                }
            })
            .min()
        {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(delay_ms))
                    .map(|_| Message::AutoReconnectTick),
            );
        }

        // State debounce flush — FR-STATE-03
        if self.state_dirty {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(2)).map(|_| Message::FlushState),
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

        // FR-LOCK-04: heartbeat timer — keepalive_interval * 2
        if self.current_conn.is_some() && !self.tabs.is_empty() {
            let heartbeat_secs = (self.config.ssh.keepalive_interval as u64) * 2;
            subs.push(
                iced::time::every(std::time::Duration::from_secs(heartbeat_secs))
                    .map(|_| Message::LockHeartbeatTick),
            );
        }

        // FR-UI-04/05: latency measurement timer — every keepalive_interval
        let has_connected_russh = self
            .tabs
            .iter()
            .any(|t| t.uses_russh && !t.dead && t.terminal.is_some());
        if has_connected_russh && self.current_conn.is_some() {
            let interval = self.config.ssh.keepalive_interval.max(5) as u64;
            subs.push(
                iced::time::every(std::time::Duration::from_secs(interval))
                    .map(|_| Message::LatencyTick),
            );
        }

        // FR-TRAY-01: poll tray events
        if self.tray.is_some() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(100)).map(|_| Message::TrayPoll),
            );
        }

        // FR-CONFIG-04: poll config file watcher every 500ms
        if self.config_reload_rx.is_some() {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(500))
                    .map(|_| Message::ConfigReloaded),
            );
        }

        // FR-RECONNECT-08: poll network gateway changes (Linux only, every 5s)
        #[cfg(target_os = "linux")]
        if self
            .tabs
            .iter()
            .any(|t| t.terminal.is_none() && t.auto_reconnect && !t.dead)
        {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(5))
                    .map(|_| Message::NetworkChanged),
            );
        }

        // FR-TABS-17: intercept window close requests
        subs.push(window::close_requests().map(Message::WindowCloseRequested));

        // FR-STATE-14: track window move/resize for geometry persistence
        subs.push(window::events().map(|(_id, event)| match event {
            window::Event::Moved(pos) => Message::WindowMoved(pos),
            window::Event::Resized(size) => Message::WindowResized(size),
            _ => Message::FlushState, // ignored events mapped to no-op
        }));

        Subscription::batch(subs)
    }

    fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }
}

// ---------------------------------------------------------------------------
// FR-RECONNECT-08: read default gateway from /proc/net/route (Linux only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn read_default_gateway() -> Option<String> {
    let content = std::fs::read_to_string("/proc/net/route").ok()?;
    // Each line: Iface Destination Gateway Flags RefCnt Use Metric Mask MTU Window IRTT
    // Default route has destination 00000000
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 && fields[1] == "00000000" {
            return Some(fields[2].to_string());
        }
    }
    None
}



