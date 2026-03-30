// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Main application state, tab management, and iced integration.

pub(crate) mod message;
pub(crate) mod session;
pub(crate) mod tab;
pub(crate) mod update;
pub(crate) mod view;

pub(crate) use message::Message;

use tab::{ChannelHolder, ConnParams, TabId, make_backend_settings, make_font_settings, make_theme_settings};
use session::{EstablishParams, SshSubscriptionData, establish_ssh_session, ssh_channel_stream};

use std::sync::Arc;
use iced::{Subscription, Task, Theme, keyboard, window};
use iced_term::settings::{BackendSettings, Settings};
use iced_term::{RegexSearch, SearchMatch};
use shellkeep::config::Config;
use shellkeep::ssh::manager::{ConnKey, ConnectionManager};
use shellkeep::state::history;
use shellkeep::state::recent::RecentConnections;
use shellkeep::state::state_file::{StateFile, TabState, WindowState};
use shellkeep::tray::Tray;
use shellkeep::{i18n, ssh};
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// App state sub-structs (defined for future migration; see R-30)
// ---------------------------------------------------------------------------

/// Welcome screen and connection input state.
#[allow(dead_code)] // Staged for future migration (R-30)
pub(crate) struct WelcomeState {
    pub(crate) client_id_input: String,
    pub(crate) show_advanced: bool,
    pub(crate) host_input: String,
    pub(crate) port_input: String,
    pub(crate) user_input: String,
    pub(crate) identity_input: String,
}

/// Scrollback search state.
#[allow(dead_code)] // Staged for future migration (R-30)
pub(crate) struct SearchState {
    pub(crate) active: bool,
    pub(crate) input: String,
    pub(crate) regex: Option<RegexSearch>,
    pub(crate) last_match: Option<SearchMatch>,
}

/// All dialog-related state (env, host key, password, lock, close).
#[allow(dead_code)] // Staged for future migration (R-30)
pub(crate) struct DialogState {
    pub(crate) show_close_dialog: bool,
    pub(crate) close_window_id: Option<window::Id>,
    pub(crate) show_env_dialog: bool,
    pub(crate) env_list: Vec<String>,
    pub(crate) env_filter: String,
    pub(crate) selected_env: Option<String>,
    pub(crate) show_new_env_dialog: bool,
    pub(crate) new_env_input: String,
    pub(crate) show_rename_env_dialog: bool,
    pub(crate) rename_env_input: String,
    pub(crate) rename_env_target: Option<String>,
    pub(crate) show_delete_env_dialog: bool,
    pub(crate) delete_env_target: Option<String>,
    pub(crate) pending_host_key_prompt: Option<ssh::connection::HostKeyPrompt>,
    pub(crate) show_password_dialog: bool,
    pub(crate) password_input: String,
    pub(crate) password_target_tab: Option<TabId>,
    pub(crate) password_conn_params: Option<ConnParams>,
    pub(crate) show_lock_dialog: bool,
    pub(crate) lock_info_text: String,
    pub(crate) lock_target_tab: Option<TabId>,
    pub(crate) pending_close_tabs: Option<Vec<usize>>,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub(crate) struct ShellKeep {
    pub(crate) tabs: Vec<tab::Tab>,
    pub(crate) active_tab: usize,
    pub(crate) next_id: u64,
    pub(crate) show_welcome: bool,
    pub(crate) renaming_tab: Option<usize>,
    /// FR-RECONNECT-02: spinner animation frame index
    pub(crate) spinner_frame: usize,
    pub(crate) rename_input: String,
    pub(crate) current_font_size: f32,
    pub(crate) context_menu: Option<(f32, f32)>,
    pub(crate) tab_context_menu: Option<(usize, f32, f32)>,
    /// Toast message (auto-dismisses)
    pub(crate) toast: Option<(String, std::time::Instant)>,
    /// Current connection params (for russh control connection)
    pub(crate) current_conn: Option<ConnParams>,
    /// Client identifier for state persistence
    pub(crate) client_id: String,
    /// Shared SSH connection manager
    pub(crate) conn_manager: Arc<Mutex<ConnectionManager>>,
    /// Whether we've already listed existing sessions after first connect
    pub(crate) sessions_listed: bool,
    /// Debounce: time of last state flush
    pub(crate) last_state_save: Option<std::time::Instant>,
    /// Debounce: state has unsaved changes
    pub(crate) state_dirty: bool,

    // Welcome screen state
    /// FR-UI-03: first-use client-id name input
    pub(crate) client_id_input: String,
    /// FR-UI-01: toggle for advanced connection options (port, user, identity)
    pub(crate) show_advanced: bool,
    pub(crate) host_input: String,
    pub(crate) port_input: String,
    pub(crate) user_input: String,
    pub(crate) identity_input: String,

    pub(crate) config: Config,
    pub(crate) recent: RecentConnections,
    pub(crate) title_text: String,
    pub(crate) error: Option<String>,

    /// System tray icon (FR-TRAY-01)
    pub(crate) tray: Option<Tray>,

    // Scrollback search state (FR-TABS-09, FR-TERMINAL-07)
    pub(crate) search_active: bool,
    pub(crate) search_input: String,
    pub(crate) search_regex: Option<RegexSearch>,
    pub(crate) search_last_match: Option<SearchMatch>,

    /// FR-CONFIG-04: config hot reload receiver
    pub(crate) config_reload_rx: Option<std::sync::mpsc::Receiver<()>>,

    /// FR-TABS-17: close confirmation dialog visible
    pub(crate) show_close_dialog: bool,
    /// FR-TABS-17: window ID to close after dialog
    pub(crate) close_window_id: Option<window::Id>,
    /// FR-STATE-14: current window geometry for persistence
    pub(crate) window_width: u32,
    pub(crate) window_height: u32,
    pub(crate) window_x: Option<i32>,
    pub(crate) window_y: Option<i32>,
    /// FR-STATE-14: debounce timer for geometry saves
    pub(crate) last_geometry_save: Option<std::time::Instant>,
    /// FR-CONN-20: remote state syncer (SFTP or shell fallback)
    pub(crate) state_syncer: Option<Arc<ssh::sftp::StateSyncer>>,

    /// FR-ENV-06: one environment active per instance
    pub(crate) current_environment: String,

    // FR-ENV-03: environment selection dialog state
    pub(crate) show_env_dialog: bool,
    pub(crate) env_list: Vec<String>,
    pub(crate) env_filter: String,
    pub(crate) selected_env: Option<String>,
    // FR-ENV-07..09: environment management modals
    pub(crate) show_new_env_dialog: bool,
    pub(crate) new_env_input: String,
    pub(crate) show_rename_env_dialog: bool,
    pub(crate) rename_env_input: String,
    pub(crate) rename_env_target: Option<String>,
    pub(crate) show_delete_env_dialog: bool,
    pub(crate) delete_env_target: Option<String>,

    // FR-CONN-03: host key TOFU dialog
    pub(crate) pending_host_key_prompt: Option<ssh::connection::HostKeyPrompt>,
    // FR-CONN-09: password prompt dialog
    pub(crate) show_password_dialog: bool,
    pub(crate) password_input: String,
    pub(crate) password_target_tab: Option<TabId>,
    pub(crate) password_conn_params: Option<ConnParams>,
    // FR-LOCK-05: lock conflict dialog
    pub(crate) show_lock_dialog: bool,
    pub(crate) lock_info_text: String,
    pub(crate) lock_target_tab: Option<TabId>,

    /// FR-SESSION-10a: close-tab confirmation dialog
    pub(crate) pending_close_tabs: Option<Vec<usize>>,

    /// FR-RECONNECT-08: last known default gateway (Linux network monitoring)
    #[cfg(target_os = "linux")]
    pub(crate) last_gateway: Option<String>,
}

// ---------------------------------------------------------------------------
// Constructor
// ---------------------------------------------------------------------------

impl ShellKeep {
    pub(crate) fn new(initial_ssh_args: Option<Vec<String>>) -> (Self, Task<Message>) {
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
            last_gateway: crate::read_default_gateway(),
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
            let (parsed_user, parsed_host, parsed_port) =
                crate::cli::parse_host_input(&host_arg);
            app.current_conn = Some(ConnParams {
                key: ConnKey {
                    host: parsed_host,
                    port: parsed_port
                        .and_then(|p| p.parse().ok())
                        .unwrap_or(cli_port.parse().unwrap_or(22)),
                    username: cli_user_flag
                        .or(parsed_user)
                        .unwrap_or_else(whoami::username),
                },
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
}

/// Escape special regex characters for literal matching in terminal search.
pub(crate) fn escape_regex(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len() * 2);
    for c in input.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

// ---------------------------------------------------------------------------
// ShellKeep methods
// ---------------------------------------------------------------------------

impl ShellKeep {
    /// Build ssh args from ConnParams (for system ssh fallback).
    pub(crate) fn build_ssh_args_from_conn(&self, conn: &ConnParams) -> Vec<String> {
        let mut args = Vec::new();
        if conn.key.username.is_empty() {
            args.push(conn.key.host.clone());
        } else {
            args.push(format!("{}@{}", conn.key.username, conn.key.host));
        }
        if conn.key.port != 22 {
            args.push("-p".to_string());
            args.push(conn.key.port.to_string());
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
    pub(crate) fn next_tmux_session(&self) -> String {
        shellkeep::ssh::tmux::env_tmux_session_name(&self.client_id, &self.current_environment)
    }

    /// Open a tab using russh SSH. Returns a Task that establishes the connection.
    pub(crate) fn open_tab_russh(&mut self, label: &str, tmux_session: &str) -> Task<Message> {
        let conn = match &self.current_conn {
            Some(c) => c.clone(),
            None => {
                self.error = Some("No connection parameters available".into());
                return Task::none();
            }
        };

        let id = TabId(self.next_id);
        self.next_id += 1;

        // Create channels for SSH I/O
        let (ssh_writer_tx, ssh_writer_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (resize_tx, resize_rx) = tokio::sync::mpsc::unbounded_channel::<(u32, u32)>();

        let settings = Settings {
            font: make_font_settings(&self.config, self.config.terminal.font_size),
            theme: make_theme_settings(&self.config),
            backend: make_backend_settings(&self.config),
        };

        let terminal = match iced_term::Terminal::new_ssh(id.0, settings, ssh_writer_tx) {
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
        self.tabs.push(tab::Tab {
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
            ssh_writer_rx_holder: Some(writer_rx_holder.clone()),
            ssh_resize_tx: Some(resize_tx),
            ssh_resize_rx_holder: Some(resize_rx_holder.clone()),
            pending_channel: Some(channel_holder.clone()),
            connection_phase: Some(phase.clone()),
            conn_state: tab::ConnectionState::Connecting {
                phase: phase.clone(),
                pending_channel: channel_holder.clone(),
            },
            backend: tab::TabBackend::Russh {
                conn_params: self.current_conn.clone().unwrap_or_else(|| ConnParams {
                    key: ConnKey { host: String::new(), port: 22, username: String::new() },
                    identity_file: None,
                }),
                writer_rx: Some(writer_rx_holder),
                resize_rx: Some(resize_rx_holder),
            },
            history_writer,
            needs_initial_resize: true,
        });
        self.active_tab = self.tabs.len() - 1;
        self.error = None;
        self.update_title();
        self.save_state();
        tracing::info!("opened SSH tab {id}: {label} (tmux: {tmux_session}) via russh");

        // Launch async connection — writes channel into the pre-allocated holder
        let holder = channel_holder;
        let params = EstablishParams {
            conn_manager: self.conn_manager.clone(),
            conn,
            tmux_session: tmux_session.to_string(),
            cols: 80,
            rows: 24,
            keepalive_secs: self.config.ssh.keepalive_interval,
            client_id: self.client_id.clone(),
            session_uuid: suuid,
            phase,
            password: None,
            force_lock: false,
        };
        Task::perform(
            async move {
                match establish_ssh_session(params).await {
                    Ok(channel) => {
                        *holder.lock().await = Some(channel);
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                }
            },
            move |result: Result<(), String>| Message::SshConnected(id, result),
        )
    }

    /// Open a tab using system ssh + PTY (legacy path, used for CLI launch).
    pub(crate) fn open_tab_with_tmux(&mut self, ssh_args: &[String], label: &str) {
        let tmux_session = self.next_tmux_session();
        self.open_tab_with_tmux_session(ssh_args, label, &tmux_session);
    }

    pub(crate) fn open_tab_with_tmux_session(
        &mut self,
        ssh_args: &[String],
        label: &str,
        tmux_session: &str,
    ) {
        let id = TabId(self.next_id);
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

        match iced_term::Terminal::new(id.0, settings) {
            Ok(terminal) => {
                self.tabs.push(tab::Tab {
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
                    pending_channel: None,
                    connection_phase: None,
                    conn_state: tab::ConnectionState::Disconnected {
                        error: None,
                        can_reconnect: false,
                    },
                    backend: tab::TabBackend::SystemSsh {
                        ssh_args: ssh_args.to_vec(),
                    },
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
    pub(crate) fn close_tab(&mut self, index: usize) -> Task<Message> {
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
                let conn_key = conn.key.clone();
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
    pub(crate) fn hide_tab(&mut self, index: usize) {
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

    pub(crate) fn reconnect_tab(&mut self, index: usize) -> Task<Message> {
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

            match iced_term::Terminal::new_ssh(tab.id.0, settings, ssh_writer_tx) {
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
                    let tab_id = tab.id;
                    let holder = channel_holder;
                    let params = EstablishParams {
                        conn_manager: self.conn_manager.clone(),
                        conn,
                        tmux_session: tab.tmux_session.clone(),
                        cols: 80,
                        rows: 24,
                        keepalive_secs: self.config.ssh.keepalive_interval,
                        client_id: self.client_id.clone(),
                        session_uuid: tab.session_uuid.clone(),
                        phase,
                        password: None,
                        force_lock: false,
                    };
                    self.update_title();

                    Task::perform(
                        async move {
                            match establish_ssh_session(params).await {
                                Ok(channel) => {
                                    *holder.lock().await = Some(channel);
                                    Ok(())
                                }
                                Err(e) => Err(e.to_string()),
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

    pub(crate) fn apply_font_to_all_tabs(&mut self) {
        let font_settings = make_font_settings(&self.config, self.current_font_size);
        for tab in &mut self.tabs {
            if let Some(ref mut terminal) = tab.terminal {
                terminal.handle(iced_term::Command::ChangeFont(font_settings.clone()));
            }
        }
        tracing::debug!("font size: {}", self.current_font_size);
    }

    pub(crate) fn save_state(&mut self) {
        self.state_dirty = true;
        if let Some(last) = self.last_state_save
            && last.elapsed() < std::time::Duration::from_secs(2)
        {
            return; // debounced — will be saved by FlushState timer
        }
        self.flush_state();
    }

    pub(crate) fn flush_state(&mut self) {
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

    pub(crate) fn update_title(&mut self) {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            let status = if tab.dead { " (disconnected)" } else { "" };
            self.title_text = format!("shellkeep — {}{}", tab.label, status);
        } else {
            self.title_text = "shellkeep".to_string();
        }
    }

    pub(crate) fn build_ssh_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let host = self.host_input.trim();

        // Parse user@host:port from host field
        let (parsed_user, parsed_host, parsed_port) = crate::cli::parse_host_input(host);

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

    pub(crate) fn title(&self) -> String {
        self.title_text.clone()
    }

    /// FR-STATE-14: save window geometry (debounced)
    pub(crate) fn save_geometry(&mut self) {
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

    pub(crate) fn subscription(&self) -> Subscription<Message> {
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

    pub(crate) fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }
}
