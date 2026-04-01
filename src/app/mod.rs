// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Main application state, tab management, and iced integration.

pub(crate) mod message;
pub(crate) mod session;
pub(crate) mod tab;
pub(crate) mod update;
pub(crate) mod view;

pub(crate) use message::Message;

use session::{EstablishParams, SshSubscriptionData, establish_ssh_session, ssh_channel_stream};
use tab::{
    ChannelHolder, ConnParams, TabId, make_backend_settings, make_font_settings,
    make_theme_settings,
};

use iced::{Subscription, Task, Theme, keyboard, window};
use iced_term::settings::{BackendSettings, Settings};
use iced_term::{RegexSearch, SearchMatch};
use shellkeep::config::Config;
use shellkeep::ssh::manager::{ConnKey, ConnectionManager};
use shellkeep::state::history;
use shellkeep::state::recent::RecentConnections;
use shellkeep::state::state_file::{
    self, DeviceState, Environment, SharedState, TabState, WindowGeometry,
};
use shellkeep::tray::Tray;
use shellkeep::{i18n, ssh};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// App state sub-structs (defined for future migration; see R-30)
// ---------------------------------------------------------------------------

/// Welcome screen and connection input state.
pub(crate) struct WelcomeState {
    pub(crate) client_id_input: String,
    pub(crate) show_advanced: bool,
    pub(crate) host_input: String,
    pub(crate) port_input: String,
    pub(crate) user_input: String,
    pub(crate) identity_input: String,
}

/// Scrollback search state.
pub(crate) struct SearchState {
    pub(crate) active: bool,
    pub(crate) input: String,
    pub(crate) regex: Option<RegexSearch>,
    pub(crate) last_match: Option<SearchMatch>,
}

/// All dialog-related state (env, host key, password, lock, close).
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
// Per-window state — Phase 4: server > window > tab hierarchy
// ---------------------------------------------------------------------------

/// Phase 5: distinguish control windows from session windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowKind {
    /// The control/manager window: shows welcome screen, server list.
    Control,
    /// A terminal session window: shows tabs with terminal content.
    Session,
}

/// Per-window state. Each window contains tabs for a single server connection.
#[allow(dead_code)]
pub(crate) struct AppWindow {
    pub(crate) id: window::Id,
    /// Phase 5: whether this is a control or session window
    pub(crate) kind: WindowKind,
    /// Stable UUID for state persistence across restarts
    pub(crate) server_window_id: String,
    pub(crate) tabs: Vec<tab::Tab>,
    pub(crate) active_tab: usize,
    pub(crate) title: String,
    /// User-visible window name (e.g. "user@host - Window 1")
    pub(crate) name: String,
    /// Whether the welcome screen is shown in this window
    pub(crate) show_welcome: bool,
    pub(crate) renaming_tab: Option<usize>,
    pub(crate) context_menu: Option<(f32, f32)>,
    pub(crate) tab_context_menu: Option<(usize, f32, f32)>,
    /// Whether the restore-hidden-sessions dropdown is visible
    pub(crate) show_restore_dropdown: bool,
    /// FR-STATE-14: current window geometry for persistence
    pub(crate) window_width: u32,
    pub(crate) window_height: u32,
    pub(crate) window_x: Option<i32>,
    pub(crate) window_y: Option<i32>,
    /// FR-STATE-14: debounce timer for geometry saves
    pub(crate) last_geometry_save: Option<std::time::Instant>,
}

impl AppWindow {
    pub(crate) fn new(id: window::Id) -> Self {
        Self {
            id,
            kind: WindowKind::Session,
            server_window_id: uuid::Uuid::new_v4().to_string(),
            tabs: Vec::new(),
            active_tab: 0,
            title: "shellkeep".to_string(),
            name: String::new(),
            show_welcome: false,
            renaming_tab: None,
            context_menu: None,
            tab_context_menu: None,
            show_restore_dropdown: false,
            window_width: 900,
            window_height: 600,
            window_x: None,
            window_y: None,
            last_geometry_save: None,
        }
    }

    /// Create a new control window.
    pub(crate) fn new_control(id: window::Id) -> Self {
        Self {
            id,
            kind: WindowKind::Control,
            server_window_id: "control".to_string(),
            tabs: Vec::new(),
            active_tab: 0,
            title: "shellkeep".to_string(),
            name: "shellkeep".to_string(),
            show_welcome: true,
            renaming_tab: None,
            context_menu: None,
            tab_context_menu: None,
            show_restore_dropdown: false,
            window_width: 500,
            window_height: 700,
            window_x: None,
            window_y: None,
            last_geometry_save: None,
        }
    }

    pub(crate) fn update_title(&mut self) {
        if !self.name.is_empty() {
            if let Some(tab) = self.tabs.get(self.active_tab) {
                let status = if tab.is_dead() { " (disconnected)" } else { "" };
                self.title = format!("{} — {}{}", self.name, tab.label, status);
            } else {
                self.title = self.name.clone();
            }
        } else if let Some(tab) = self.tabs.get(self.active_tab) {
            let status = if tab.is_dead() { " (disconnected)" } else { "" };
            self.title = format!("shellkeep — {}{}", tab.label, status);
        } else {
            self.title = "shellkeep".to_string();
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub(crate) struct ShellKeep {
    /// Multi-window state: each window has its own tabs
    pub(crate) windows: HashMap<window::Id, AppWindow>,
    /// Ordered list of window IDs for consistent iteration
    pub(crate) window_order: Vec<window::Id>,
    /// Phase 5: the control window ID (always exists, may be hidden)
    pub(crate) control_window_id: window::Id,
    /// The currently focused window
    pub(crate) focused_window: Option<window::Id>,
    pub(crate) next_id: u64,
    /// FR-RECONNECT-02: spinner animation frame index
    pub(crate) spinner_frame: usize,
    pub(crate) rename_input: String,
    pub(crate) current_font_size: f32,
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
    pub(crate) welcome: WelcomeState,

    pub(crate) config: Config,
    pub(crate) recent: RecentConnections,
    pub(crate) error: Option<String>,

    /// System tray icon (FR-TRAY-01)
    pub(crate) tray: Option<Tray>,

    // Scrollback search state (FR-TABS-09, FR-TERMINAL-07)
    pub(crate) search: SearchState,

    /// FR-CONFIG-04: config hot reload receiver
    pub(crate) config_reload_rx: Option<std::sync::mpsc::Receiver<()>>,

    // Dialog state (close, env, host key, password, lock)
    pub(crate) dialogs: DialogState,
    /// FR-CONN-20: remote state syncer (SFTP or shell fallback)
    pub(crate) state_syncer: Option<Arc<ssh::sftp::StateSyncer>>,

    /// FR-ENV-06: one environment active per instance
    pub(crate) current_environment: String,

    /// Hidden session UUIDs (per-device, not restored as tabs on connect)
    pub(crate) hidden_sessions: Vec<String>,

    /// Whether the connect form is shown in the control window
    pub(crate) show_connect_form: bool,

    /// Whether we're currently confirming a destructive CloseServer action
    pub(crate) confirm_close_server: bool,

    /// Window rename state: which window is being renamed
    pub(crate) renaming_window: Option<window::Id>,

    /// Window rename input value
    pub(crate) window_rename_input: String,

    /// Auto-incrementing counter for default window names per server
    pub(crate) window_counter: u32,

    /// Snapshot of saved state taken before the first tab is opened, so that
    /// handle_existing_sessions can see sessions that save_state() overwrote.
    pub(crate) pre_connect_state: Option<(Option<SharedState>, Option<DeviceState>)>,

    /// FR-RECONNECT-08: last known default gateway (Linux network monitoring)
    #[cfg(target_os = "linux")]
    pub(crate) last_gateway: Option<String>,
}

// ---------------------------------------------------------------------------
// Constructor
// ---------------------------------------------------------------------------

impl ShellKeep {
    pub(crate) fn new(
        initial_ssh_args: Option<Vec<String>>,
        initial_window_settings: window::Settings,
    ) -> (Self, Task<Message>) {
        let username = crate::cli::default_ssh_username();
        let config = Config::load();
        let recent = RecentConnections::load();
        let default_port = config.ssh.default_port.to_string();

        // Phase 5: open the control window on startup
        let control_settings = window::Settings {
            size: iced::Size::new(500.0, 700.0),
            min_size: Some(iced::Size::new(400.0, 300.0)),
            ..initial_window_settings.clone()
        };
        let (first_window_id, open_task) = window::open(control_settings);
        let control_window = AppWindow::new_control(first_window_id);

        let mut windows = HashMap::new();
        windows.insert(first_window_id, control_window);

        let mut app = ShellKeep {
            windows,
            window_order: vec![first_window_id],
            control_window_id: first_window_id,
            focused_window: Some(first_window_id),
            next_id: 0,
            spinner_frame: 0,
            rename_input: String::new(),
            current_font_size: config.terminal.font_size,
            toast: None,
            current_conn: None,
            client_id: shellkeep::state::client_id::resolve(config.general.client_id.as_deref()),
            current_environment: "Default".to_string(),
            hidden_sessions: Vec::new(),
            show_connect_form: false,
            confirm_close_server: false,
            renaming_window: None,
            window_rename_input: String::new(),
            window_counter: 0,
            conn_manager: Arc::new(Mutex::new(ConnectionManager::new())),
            sessions_listed: false,
            last_state_save: None,
            state_dirty: false,
            welcome: WelcomeState {
                client_id_input: String::new(),
                show_advanced: false,
                host_input: String::new(),
                port_input: default_port,
                user_input: username,
                identity_input: String::new(),
            },
            config,
            recent,
            error: None,
            tray: None,
            search: SearchState {
                active: false,
                input: String::new(),
                regex: None,
                last_match: None,
            },
            config_reload_rx: None,
            dialogs: DialogState {
                show_close_dialog: false,
                close_window_id: None,
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
            },
            state_syncer: None,
            pre_connect_state: None,
            #[cfg(target_os = "linux")]
            last_gateway: shellkeep::network::read_default_gateway(),
        };

        // FR-CONFIG-04: start watching config file for hot reload
        app.config_reload_rx = Some(shellkeep::config::watch_config(Config::file_path()));

        // FR-TRAY-01: initialize system tray icon
        app.tray = Tray::new(app.config.tray.enabled);

        // FR-STATE-07: clean up orphaned .tmp files from interrupted saves
        shellkeep::state::state_file::cleanup_tmp_files(&app.client_id);

        // FR-HISTORY-11: clean up old history files on startup
        history::cleanup_old_history(app.config.state.history_max_days);

        // The open_task just signals when the window is ready; map it to a Noop
        let open_task = open_task.map(|_| Message::Noop);

        if let Some(ssh_args) = initial_ssh_args {
            // Parse connection params from CLI args
            let arg_refs: Vec<&str> = ssh_args.iter().map(|s| s.as_str()).collect();
            let parsed = crate::cli::parse_ssh_args(&arg_refs);
            let label = parsed.label.clone();
            app.current_conn = Some(ConnParams {
                key: ConnKey {
                    host: parsed.host,
                    port: parsed.port,
                    username: parsed
                        .username
                        .unwrap_or_else(crate::cli::default_ssh_username),
                },
                identity_file: parsed.identity_file,
            });

            // Phase 5: open a session window for the CLI connection
            let (session_win_id, session_open_task) = window::open(initial_window_settings);
            let mut session_win = AppWindow::new(session_win_id);
            session_win.server_window_id = "main".to_string();
            // Item 8: default window name for CLI-launched windows
            app.window_counter += 1;
            session_win.name = format!("{} - Window {}", label, app.window_counter);
            app.windows.insert(session_win_id, session_win);
            app.window_order.push(session_win_id);
            app.focused_window = Some(session_win_id);

            // FR-CONN-21: CLI launch via russh (async, non-blocking)
            // Opens one tab immediately; existing sessions discovered after connect.
            // Snapshot saved state BEFORE open_tab_russh, which calls save_state()
            // and overwrites the file — reconciliation needs the pre-overwrite state.
            app.pre_connect_state = Some(state_file::load_split_state(&app.client_id));
            let tmux_session = app.next_tmux_session();
            let tab_task = app.open_tab_russh(&label, &tmux_session);
            return (
                app,
                Task::batch([
                    open_task,
                    session_open_task.map(|_| Message::Noop),
                    tab_task,
                ]),
            );
        }
        // Control window always shows welcome screen (already set in new_control)

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

        (app, open_task)
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
    // -----------------------------------------------------------------------
    // Multi-window helpers
    // -----------------------------------------------------------------------

    /// Get the focused window ID, falling back to the first window.
    pub(crate) fn active_window_id(&self) -> Option<window::Id> {
        self.focused_window
            .or_else(|| self.window_order.first().copied())
    }

    /// Get an immutable reference to the active (focused) window.
    pub(crate) fn active_window(&self) -> Option<&AppWindow> {
        self.active_window_id().and_then(|id| self.windows.get(&id))
    }

    /// Get a mutable reference to the active (focused) window.
    pub(crate) fn active_window_mut(&mut self) -> Option<&mut AppWindow> {
        let id = self.active_window_id();
        id.and_then(move |id| self.windows.get_mut(&id))
    }

    /// Get all tabs across all windows (for subscriptions, etc.).
    pub(crate) fn all_tabs(&self) -> impl Iterator<Item = &tab::Tab> {
        self.windows.values().flat_map(|w| w.tabs.iter())
    }

    /// Get all tabs mutably across all windows.
    pub(crate) fn all_tabs_mut(&mut self) -> impl Iterator<Item = &mut tab::Tab> {
        self.windows.values_mut().flat_map(|w| w.tabs.iter_mut())
    }

    /// Find which window contains a tab by TabId. Returns (window_id, tab_index).
    #[allow(dead_code)]
    pub(crate) fn find_tab_window(&self, tab_id: TabId) -> Option<(window::Id, usize)> {
        for (win_id, win) in &self.windows {
            if let Some(idx) = win.tabs.iter().position(|t| t.id == tab_id) {
                return Some((*win_id, idx));
            }
        }
        None
    }

    /// Find a tab by TabId (mutable) across all windows.
    pub(crate) fn find_tab_mut(&mut self, tab_id: TabId) -> Option<&mut tab::Tab> {
        for win in self.windows.values_mut() {
            if let Some(tab) = win.tabs.iter_mut().find(|t| t.id == tab_id) {
                return Some(tab);
            }
        }
        None
    }

    /// Find a tab by TabId (immutable) across all windows.
    #[allow(dead_code)]
    pub(crate) fn find_tab(&self, tab_id: TabId) -> Option<&tab::Tab> {
        for win in self.windows.values() {
            if let Some(tab) = win.tabs.iter().find(|t| t.id == tab_id) {
                return Some(tab);
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Tab / session management
    // -----------------------------------------------------------------------

    /// Open a new tab, assigning it the next unused tmux session name.
    /// FR-SESSION-04, FR-SESSION-05, FR-ENV-02: generate tmux session name with
    /// environment and timestamp.
    pub(crate) fn next_tmux_session(&self) -> String {
        shellkeep::ssh::tmux::env_tmux_session_name(&self.current_environment)
    }

    /// Open a tab using russh SSH. Returns a Task that establishes the connection.
    /// The tab is added to the currently focused window.
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

        // FR-HISTORY-02: create history writer (None if disabled via config)
        let session_uuid = uuid::Uuid::new_v4().to_string();
        let history_writer =
            history::HistoryWriter::new(&session_uuid, self.config.state.history_max_size_mb);
        let suuid = session_uuid.clone();

        let new_tab = tab::Tab {
            id,
            label: label.to_string(),
            session_uuid,
            terminal: Some(terminal),
            tmux_session: tmux_session.to_string(),
            last_error: None,
            last_latency_ms: None,
            conn_state: tab::ConnectionState::Connecting {
                phase: phase.clone(),
                pending_channel: channel_holder.clone(),
            },
            backend: tab::TabBackend::Russh {
                conn_params: self.current_conn.clone().unwrap_or_else(|| ConnParams {
                    key: ConnKey {
                        host: String::new(),
                        port: 22,
                        username: String::new(),
                    },
                    identity_file: None,
                }),
                writer_rx: Some(writer_rx_holder),
                resize_rx: Some(resize_rx_holder),
                resize_tx: Some(resize_tx),
            },
            history_writer,
            needs_initial_resize: true,
        };

        // Add tab to the active window
        if let Some(win) = self.active_window_mut() {
            win.tabs.push(new_tab);
            win.active_tab = win.tabs.len() - 1;
            win.update_title();
        }
        self.error = None;
        self.save_state();
        tracing::info!("opened SSH tab {id}: {label} (tmux: {tmux_session}) via russh");

        // Launch async connection
        self.start_ssh_connection(
            id,
            &conn,
            tmux_session,
            &suuid,
            phase,
            channel_holder,
            None,
            false,
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
                cursor_shape: self.config.terminal.cursor_shape.to_string(),
                ..Default::default()
            },
        };

        match iced_term::Terminal::new(id.0, settings) {
            Ok(terminal) => {
                let new_tab = tab::Tab {
                    id,
                    label: label.to_string(),
                    session_uuid: uuid::Uuid::new_v4().to_string(),
                    terminal: Some(terminal),
                    tmux_session: tmux_session.to_string(),
                    last_error: None,
                    last_latency_ms: None,
                    // System SSH tabs use PTY directly, so they start "connected"
                    // in the sense that no SSH handshake is tracked here.
                    conn_state: tab::ConnectionState::Connected {
                        channel: Arc::new(Mutex::new(None)),
                    },
                    backend: tab::TabBackend::SystemSsh {
                        ssh_args: ssh_args.to_vec(),
                    },
                    history_writer: None,
                    needs_initial_resize: true,
                };
                if let Some(win) = self.active_window_mut() {
                    win.tabs.push(new_tab);
                    win.active_tab = win.tabs.len() - 1;
                    win.update_title();
                }
                self.error = None;
                self.save_state();
                tracing::info!("opened tab {id}: {label} (tmux: {tmux_session})");
            }
            Err(e) => {
                tracing::error!("failed to create terminal: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    /// Close a tab by index in the active window and kill the tmux session on the server.
    pub(crate) fn close_tab(&mut self, index: usize) -> Task<Message> {
        let win_id = match self.active_window_id() {
            Some(id) => id,
            None => return Task::none(),
        };
        self.close_tab_in_window(win_id, index)
    }

    /// Close a tab by index in a specific window and kill the tmux session.
    pub(crate) fn close_tab_in_window(
        &mut self,
        win_id: window::Id,
        index: usize,
    ) -> Task<Message> {
        let tab = {
            let win = match self.windows.get_mut(&win_id) {
                Some(w) => w,
                None => return Task::none(),
            };
            if index >= win.tabs.len() {
                return Task::none();
            }
            let count_before = win.tabs.len();
            let tab = win.tabs.remove(index);
            win.active_tab = update::active_tab_after_removal(win.active_tab, count_before, index);
            win.update_title();
            tab
        };
        tracing::info!(
            "closed tab {}: {} (killing tmux session)",
            tab.id,
            tab.label
        );
        // Force immediate flush (bypass debounce) so the closed tab is removed
        // from saved state before the app can exit or reconnect.
        self.state_dirty = true;
        self.flush_state();

        self.toast = Some((
            "Session closed and terminated on server.".into(),
            std::time::Instant::now(),
        ));

        // Kill the tmux session on the server.
        if tab.is_russh() {
            let tmux_session = tab.tmux_session.clone();
            let mgr = self.conn_manager.clone();
            if let Some(ref conn) = self.current_conn {
                let conn_key = conn.key.clone();
                let identity = conn.identity_file.clone();
                let keepalive = self.config.ssh.keepalive_interval;
                return Task::perform(
                    async move {
                        let mut mgr_guard = mgr.lock().await;
                        let handle_arc = if let Some(h) = mgr_guard.get_cached(&conn_key) {
                            h
                        } else {
                            match mgr_guard
                                .get_or_connect(&conn_key, identity.as_deref(), None, keepalive)
                                .await
                            {
                                Ok(r) => r.handle,
                                Err(e) => {
                                    tracing::warn!(
                                        "cannot kill tmux session {tmux_session}: no connection ({e})"
                                    );
                                    return;
                                }
                            }
                        };
                        drop(mgr_guard);
                        let handle = handle_arc.lock().await;
                        let cmd = format!("tmux kill-session -t {tmux_session} 2>/dev/null");
                        if let Err(e) = ssh::connection::exec_command(&handle, &cmd).await {
                            tracing::warn!("failed to kill tmux session {tmux_session}: {e}");
                        } else {
                            tracing::info!("killed tmux session: {tmux_session}");
                        }
                    },
                    |_| Message::Noop,
                );
            }
        }
        Task::none()
    }

    /// Hide a tab — disconnect SSH but keep the tmux session alive on the server.
    /// Adds the session UUID to hidden_sessions so it won't be auto-restored.
    pub(crate) fn hide_tab(&mut self, index: usize) {
        let win_id = match self.active_window_id() {
            Some(id) => id,
            None => return,
        };
        let tab = {
            let win = match self.windows.get_mut(&win_id) {
                Some(w) => w,
                None => return,
            };
            if index >= win.tabs.len() {
                return;
            }
            let count_before = win.tabs.len();
            let tab = win.tabs.remove(index);
            win.active_tab = update::active_tab_after_removal(win.active_tab, count_before, index);
            win.update_title();
            tab
        };
        // Track the hidden session UUID so it's not restored on reconnect
        if !self.hidden_sessions.contains(&tab.session_uuid) {
            self.hidden_sessions.push(tab.session_uuid.clone());
        }
        tracing::info!("hid tab {}: {} (session kept on server)", tab.id, tab.label);
        self.save_state();
        self.toast = Some((
            i18n::t(i18n::SESSION_KEPT).into(),
            std::time::Instant::now(),
        ));
    }

    pub(crate) fn reconnect_tab(&mut self, index: usize) -> Task<Message> {
        let win_id = match self.active_window_id() {
            Some(id) => id,
            None => return Task::none(),
        };
        self.reconnect_tab_in_window(win_id, index)
    }

    pub(crate) fn reconnect_tab_in_window(
        &mut self,
        win_id: window::Id,
        index: usize,
    ) -> Task<Message> {
        let win = match self.windows.get_mut(&win_id) {
            Some(w) => w,
            None => return Task::none(),
        };
        if index >= win.tabs.len() {
            return Task::none();
        }

        let tab = &mut win.tabs[index];

        if tab.is_russh() {
            // Russh reconnection: clear old state, create new terminal, launch connection
            tab.clear_resize_tx();

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
                    tab.set_writer_rx(Arc::new(Mutex::new(Some(ssh_writer_rx))));
                    tab.set_resize_tx(resize_tx);
                    tab.set_resize_rx(Arc::new(Mutex::new(Some(resize_rx))));
                    let phase = Arc::new(std::sync::Mutex::new(
                        i18n::t(i18n::RECONNECTING).to_string(),
                    ));
                    tab.mark_connecting(phase.clone(), channel_holder.clone());

                    let conn = match tab.conn_params() {
                        Some(c) => c.clone(),
                        None => return Task::none(),
                    };
                    let tab_id = tab.id;
                    let tmux = tab.tmux_session.clone();
                    let suuid = tab.session_uuid.clone();
                    win.update_title();

                    self.start_ssh_connection(
                        tab_id,
                        &conn,
                        &tmux,
                        &suuid,
                        phase,
                        channel_holder,
                        None,
                        false,
                    )
                }
                Err(e) => {
                    tracing::error!("failed to create terminal for reconnect: {e}");
                    Task::none()
                }
            }
        } else {
            // System ssh reconnection (legacy)
            let ssh_args = tab.ssh_args().to_vec();
            let label = tab.label.clone();
            let tmux_session = tab.tmux_session.clone();

            win.tabs.remove(index);
            // Drop mutable borrow before calling method that needs &mut self
            self.open_tab_with_tmux_session(&ssh_args, &label, &tmux_session);

            if let Some(win) = self.windows.get_mut(&win_id)
                && win.tabs.len() > 1
                && index < win.tabs.len() - 1
            {
                // SAFETY: len() > 1 guarantees pop() returns Some
                #[allow(clippy::unwrap_used)]
                let tab = win.tabs.pop().unwrap();
                win.tabs.insert(index, tab);
                win.active_tab = index;
                win.update_title();
            }
            Task::none()
        }
    }

    /// Launch an async SSH connection for an existing tab.
    ///
    /// Shared logic for open_tab_russh, reconnect_tab, password retry, and lock takeover.
    /// The tab must already exist and be in a state ready for connecting (Connecting or similar).
    /// The `channel_holder` must already be set on the tab via `mark_connecting`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_ssh_connection(
        &self,
        tab_id: tab::TabId,
        conn: &ConnParams,
        tmux_session: &str,
        session_uuid: &str,
        phase: Arc<std::sync::Mutex<String>>,
        channel_holder: ChannelHolder,
        password: Option<String>,
        force_lock: bool,
    ) -> Task<Message> {
        let params = EstablishParams {
            conn_manager: self.conn_manager.clone(),
            conn: conn.clone(),
            tmux_session: tmux_session.to_string(),
            cols: 80,
            rows: 24,
            keepalive_secs: self.config.ssh.keepalive_interval,
            client_id: self.client_id.clone(),
            session_uuid: session_uuid.to_string(),
            phase,
            password,
            force_lock,
        };
        Task::perform(
            async move {
                match establish_ssh_session(params).await {
                    Ok(channel) => {
                        *channel_holder.lock().await = Some(channel);
                        Ok(())
                    }
                    Err(e) => Err(e.to_string()),
                }
            },
            move |result: Result<(), String>| Message::SshConnected(tab_id, result),
        )
    }

    pub(crate) fn apply_font_to_all_tabs(&mut self) {
        let font_settings = make_font_settings(&self.config, self.current_font_size);
        for tab in self.all_tabs_mut() {
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

        // Build shared state (environments, tabs from all windows)
        let mut shared = SharedState::new();
        let mut pos = 0usize;
        let mut env_tabs: Vec<TabState> = Vec::new();
        for win_id in &self.window_order {
            if let Some(win) = self.windows.get(win_id) {
                for tab in &win.tabs {
                    env_tabs.push(TabState {
                        session_uuid: tab.session_uuid.clone(),
                        tmux_session_name: tab.tmux_session.clone(),
                        title: tab.label.clone(),
                        position: pos,
                    });
                    pos += 1;
                }
            }
        }
        shared.environments.insert(
            self.current_environment.clone(),
            Environment {
                name: self.current_environment.clone(),
                tabs: env_tabs,
            },
        );
        shared.last_environment = Some(self.current_environment.clone());
        // Preserve other environments from the previously loaded shared state
        if let Some(prev) = SharedState::load_local(&SharedState::local_cache_path()) {
            for (name, env) in &prev.environments {
                if name != &self.current_environment {
                    shared.environments.insert(name.clone(), env.clone());
                }
            }
        }

        // Build device state (geometry per window, hidden sessions)
        let mut device = DeviceState::new(&self.client_id);
        for (win_id, win) in &self.windows {
            device.window_geometry.insert(
                win.server_window_id.clone(),
                WindowGeometry {
                    x: win.window_x,
                    y: win.window_y,
                    width: win.window_width,
                    height: win.window_height,
                },
            );
            if Some(*win_id) == self.focused_window {
                device.last_active_window = Some(win.server_window_id.clone());
            }
        }
        device.hidden_sessions = self.hidden_sessions.clone();

        let shared_path = SharedState::local_cache_path();
        let device_path = DeviceState::local_cache_path(&self.client_id);

        // FR-TRAY-02: update tray tooltip with session count
        let any_show_welcome = self.windows.values().any(|w| w.show_welcome);
        if let Some(ref tray) = self.tray {
            let active_count = self.all_tabs().filter(|t| !t.is_dead()).count();
            tray.set_session_count(active_count);
            // FR-TRAY-04: change icon when active sessions exist but window may be hidden
            tray.set_hidden_active(active_count > 0 && !any_show_welcome);
        }

        // Write state to local disk synchronously so it survives process kill.
        // Server sync remains async (non-critical for local restore).
        let shared_json = match serde_json::to_string_pretty(&shared) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("failed to serialize shared state: {e}");
                return;
            }
        };
        let device_json = match serde_json::to_string_pretty(&device) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!("failed to serialize device state: {e}");
                return;
            }
        };

        // FR-CONN-20: sync both files to server if syncer is available
        if let Some(ref syncer) = self.state_syncer {
            let syncer = syncer.clone();
            let shared_remote = shared_json.clone();
            let device_remote = device_json.clone();
            tokio::task::spawn(async move {
                if let Err(e) = syncer.write_shared_state(&shared_remote).await {
                    tracing::warn!("server shared state sync failed: {e}");
                }
                if let Err(e) = syncer.write_device_state(&device_remote).await {
                    tracing::warn!("server device state sync failed: {e}");
                }
            });
        }

        // Write both files locally — synchronous to survive SIGTERM/killall.
        // This blocks briefly (~1ms for JSON write) but ensures durability.
        write_state_file(&shared_path, &shared_json, "shared");
        write_state_file(&device_path, &device_json, "device");
    }

    pub(crate) fn update_title(&mut self) {
        if let Some(win) = self.active_window_mut() {
            win.update_title();
        }
    }

    pub(crate) fn build_ssh_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let host = self.welcome.host_input.trim();

        // Parse user@host:port from host field
        let (parsed_user, parsed_host, parsed_port) = crate::cli::parse_host_input(host);

        let user = if !self.welcome.user_input.is_empty() {
            self.welcome.user_input.clone()
        } else {
            parsed_user.unwrap_or_default()
        };

        let host = parsed_host;
        let port = parsed_port.unwrap_or_else(|| self.welcome.port_input.trim().to_string());

        if !user.is_empty() {
            args.push(format!("{user}@{host}"));
        } else {
            args.push(host);
        }

        if !port.is_empty() && port != "22" {
            args.push("-p".to_string());
            args.push(port);
        }

        if !self.welcome.identity_input.is_empty() {
            args.push("-i".to_string());
            args.push(self.welcome.identity_input.clone());
        }

        args
    }

    pub(crate) fn title(&self, window_id: window::Id) -> String {
        self.windows
            .get(&window_id)
            .map(|w| {
                if w.kind == WindowKind::Control {
                    "shellkeep".to_string()
                } else {
                    w.title.clone()
                }
            })
            .unwrap_or_else(|| "shellkeep".to_string())
    }

    /// FR-STATE-14: save window geometry (debounced, per-window)
    pub(crate) fn save_geometry(&mut self, window_id: window::Id) {
        let debounced = if let Some(win) = self.windows.get(&window_id) {
            win.last_geometry_save
                .is_some_and(|last| last.elapsed() < std::time::Duration::from_millis(500))
        } else {
            false
        };
        if debounced {
            self.state_dirty = true;
            return;
        }
        if let Some(win) = self.windows.get_mut(&window_id) {
            win.last_geometry_save = Some(std::time::Instant::now());
        }
        self.state_dirty = true;
        self.flush_state();
    }

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let mut subs: Vec<Subscription<Message>> = Vec::new();

        for tab in self.all_tabs() {
            if let Some(ref terminal) = tab.terminal {
                subs.push(terminal.subscription().map(Message::TerminalEvent));
            }

            // SSH channel I/O subscription for russh tabs with a connected channel
            if tab.is_russh()
                && let (Some(channel_holder), Some(writer_rx_holder), Some(resize_rx_holder)) = (
                    tab.channel_holder(),
                    tab.writer_rx_holder(),
                    tab.resize_rx_holder(),
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
        if self.all_tabs().any(|t| t.connection_phase_text().is_some()) {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(200))
                    .map(|_| Message::ConnectionPhaseTick),
            );
        }

        subs.push(keyboard::listen().map(Message::KeyEvent));

        // FR-RECONNECT-02: spinner animation subscription (100ms tick)
        let any_reconnecting = self.all_tabs().any(|t| t.is_auto_reconnect());
        if any_reconnecting {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(100))
                    .map(|_| Message::SpinnerTick),
            );
        }

        // FR-RECONNECT-06: exponential backoff auto-reconnect timer
        if let Some(delay_ms) = self
            .all_tabs()
            .filter(|t| t.is_auto_reconnect())
            .map(|t| {
                let d = t.reconnect_delay_ms();
                if d == 0 {
                    (self.config.ssh.reconnect_backoff_base * 1000.0) as u64
                } else {
                    d
                }
            })
            .min()
        {
            subs.push(
                iced::time::every(std::time::Duration::from_millis(delay_ms))
                    .map(|_| Message::AutoReconnectTick),
            );
        }

        let has_any_tabs = self.all_tabs().next().is_some();

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
        if self.current_conn.is_some() && has_any_tabs {
            let heartbeat_secs = (self.config.ssh.keepalive_interval as u64) * 2;
            subs.push(
                iced::time::every(std::time::Duration::from_secs(heartbeat_secs))
                    .map(|_| Message::LockHeartbeatTick),
            );
        }

        // FR-UI-04/05: latency measurement timer — every keepalive_interval
        let has_connected_russh = self
            .all_tabs()
            .any(|t| t.is_russh() && !t.is_dead() && t.terminal.is_some());
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
        if self.all_tabs().any(|t| t.is_auto_reconnect()) {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(5))
                    .map(|_| Message::NetworkChanged),
            );
        }

        // FR-TABS-17: intercept window close requests
        subs.push(window::close_requests().map(Message::WindowCloseRequested));

        // FR-STATE-14: track window move/resize for geometry persistence
        // Bug 7 fix: also track Focused events so focused_window is always
        // accurate, ensuring NewTab goes to the correct window.
        subs.push(window::events().map(|(id, event)| match event {
            window::Event::Moved(pos) => Message::WindowMoved(id, pos),
            window::Event::Resized(size) => Message::WindowResized(id, size),
            window::Event::Focused => Message::WindowFocused(id),
            _ => Message::Noop,
        }));

        Subscription::batch(subs)
    }

    pub(crate) fn theme(&self, _window_id: window::Id) -> Theme {
        Theme::CatppuccinMocha
    }
}

/// Write a state file atomically (tmp + rename). Synchronous.
fn write_state_file(path: &std::path::Path, json: &str, label: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, json) {
        tracing::warn!("failed to write {label} state tmp: {e}");
    } else if let Err(e) = std::fs::rename(&tmp, path) {
        tracing::warn!("failed to rename {label} state file: {e}");
    } else {
        tracing::debug!("{label} state saved to {}", path.display());
    }
}
