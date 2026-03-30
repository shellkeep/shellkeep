// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub(crate) mod message;
pub(crate) mod session;
pub(crate) mod tab;
pub(crate) mod view;

pub(crate) use message::Message;

use tab::ConnParams;

use std::sync::Arc;
use iced::{Task, window};
use iced_term::{RegexSearch, SearchMatch};
use shellkeep::config::Config;
use shellkeep::ssh::manager::ConnectionManager;
use shellkeep::state::history;
use shellkeep::state::recent::RecentConnections;
use shellkeep::tray::Tray;
use shellkeep::ssh;
use tokio::sync::Mutex;

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
    #[allow(dead_code)]
    pub(crate) pending_host_key_prompt: Option<ssh::connection::HostKeyPrompt>,
    // FR-CONN-09: password prompt dialog
    #[allow(dead_code)]
    pub(crate) show_password_dialog: bool,
    #[allow(dead_code)]
    pub(crate) password_input: String,
    #[allow(dead_code)]
    pub(crate) password_target_tab: Option<u64>,
    #[allow(dead_code)]
    pub(crate) password_conn_params: Option<ConnParams>,
    // FR-LOCK-05: lock conflict dialog
    #[allow(dead_code)]
    pub(crate) show_lock_dialog: bool,
    #[allow(dead_code)]
    pub(crate) lock_info_text: String,
    #[allow(dead_code)]
    pub(crate) lock_target_tab: Option<u64>,

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
