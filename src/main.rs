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
    ChannelHolder, ConnParams, Tab,
    make_backend_settings, make_font_settings, make_theme_settings,
};
use app::Message;
pub(crate) use app::ShellKeep;

use std::sync::Arc;
use iced::{Point, Size, Subscription, Task, Theme, keyboard, window};
use iced_term::settings::{BackendSettings, Settings};
use shellkeep::config::Config;
use shellkeep::ssh::manager::ConnKey;
use shellkeep::state::history;
use shellkeep::state::state_file::{StateFile, TabState, WindowState};
use shellkeep::{i18n, ssh};
use tokio::sync::Mutex;

// Re-export for view layer
pub(crate) use app::update::RENAME_INPUT_ID;

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
// App
// ---------------------------------------------------------------------------

impl ShellKeep {
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



