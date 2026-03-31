// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Message dispatch and state update logic.
//!
//! This is the core of the iced application: every user action, SSH event,
//! and timer tick arrives as a [`Message`] and is routed by [`ShellKeep::update`]
//! to one of seven handler methods:
//!
//! - `handle_ssh_message` — SSH data, connect/disconnect, session discovery
//! - `handle_tab_message` — tab open/close/move/rename, recent connections
//! - `handle_input_message` — welcome screen form, keyboard shortcuts
//! - `handle_dialog_message` — close, env, host-key, password, lock dialogs
//! - `handle_timer_message` — reconnect backoff, spinner, heartbeat, latency
//! - `handle_terminal_message` — terminal I/O, context menu, window geometry
//! - `handle_search_message` — scrollback search, export, clipboard
//!
//! To add a new message: add the variant to [`Message`], add it to the
//! appropriate arm in `update()`, then implement the handler.

use super::ShellKeep;
use super::message::Message;
use super::tab::{ChannelHolder, SPINNER_FRAMES};

use iced::{Task, keyboard, window};
use iced_term::settings::FontSettings;
use iced_term::{AlacrittyColumn, AlacrittyLine, AlacrittyPoint, RegexSearch};
use shellkeep::config::Config;
use shellkeep::ssh;
use shellkeep::ssh::manager::ConnKey;
use shellkeep::state::recent::RecentConnection;
use shellkeep::state::state_file::{self, SharedState};
use shellkeep::tray::{Tray, TrayAction};
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) const RENAME_INPUT_ID: &str = "rename-tab-input";

impl ShellKeep {
    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // --- SSH messages ---
            Message::SshData(..)
            | Message::PasteToTerminal(..)
            | Message::SshDisconnected(..)
            | Message::SshConnected(..)
            | Message::ExistingSessionsFound(..) => self.handle_ssh_message(message),

            // --- Tab messages ---
            Message::SelectTab(..)
            | Message::CloseTab(..)
            | Message::NewTab
            | Message::ReconnectTab(..)
            | Message::HideTab(..)
            | Message::CloseOtherTabs(..)
            | Message::CloseTabsToRight(..)
            | Message::ConfirmCloseTabs
            | Message::CancelCloseTabs
            | Message::StartRename(..)
            | Message::RenameInputChanged(..)
            | Message::FinishRename
            | Message::CreateNewSession(..)
            | Message::TabMoveLeft(..)
            | Message::TabMoveRight(..)
            | Message::TabContextMenu(..)
            | Message::ConnectRecent(..)
            | Message::ShowRestoreDropdown
            | Message::DismissRestoreDropdown
            | Message::RestoreHiddenSession(..) => self.handle_tab_message(message),

            // --- Input / welcome screen messages ---
            Message::KeyEvent(..)
            | Message::HostInputChanged(..)
            | Message::PortInputChanged(..)
            | Message::UserInputChanged(..)
            | Message::IdentityInputChanged(..)
            | Message::Connect
            | Message::ToggleAdvanced
            | Message::ClientIdInputChanged(..) => self.handle_input_message(message),

            // --- Dialog messages ---
            Message::WindowCloseRequested(..)
            | Message::CloseDialogClose
            | Message::CloseDialogCancel
            | Message::ShowEnvDialog
            | Message::EnvFilterChanged(..)
            | Message::SelectEnv(..)
            | Message::ConfirmEnv
            | Message::NewEnvFromDialog
            | Message::CancelEnvDialog
            | Message::ShowNewEnvDialog
            | Message::NewEnvInputChanged(..)
            | Message::ConfirmNewEnv
            | Message::CancelNewEnv
            | Message::ShowRenameEnvDialog(..)
            | Message::RenameEnvInputChanged(..)
            | Message::ConfirmRenameEnv
            | Message::CancelRenameEnv
            | Message::ShowDeleteEnvDialog(..)
            | Message::ConfirmDeleteEnv
            | Message::CancelDeleteEnv
            | Message::SwitchEnvironment(..)
            | Message::HostKeyAcceptSave
            | Message::HostKeyConnectOnce
            | Message::HostKeyReject
            | Message::HostKeyChangedDismiss
            | Message::PasswordInputChanged(..)
            | Message::PasswordSubmit
            | Message::PasswordCancel
            | Message::LockTakeOver
            | Message::LockCancel => self.handle_dialog_message(message),

            // --- Timer / periodic messages ---
            Message::AutoReconnectTick
            | Message::SpinnerTick
            | Message::FlushState
            | Message::ConnectionPhaseTick
            | Message::LockHeartbeatTick
            | Message::LockHeartbeatDone(..)
            | Message::LatencyTick
            | Message::LatencyMeasured(..)
            | Message::TrayPoll
            | Message::NetworkChanged
            | Message::ConfigReloaded => self.handle_timer_message(message),

            // --- Terminal / context menu / misc ---
            Message::TerminalEvent(..)
            | Message::ContextMenuCopy
            | Message::ContextMenuPaste
            | Message::ContextMenuDismiss
            | Message::ToastDismiss
            | Message::WindowMoved(..)
            | Message::WindowResized(..) => self.handle_terminal_message(message),

            // --- Search messages ---
            Message::SearchToggle
            | Message::SearchInputChanged(..)
            | Message::SearchNext
            | Message::SearchPrev
            | Message::SearchClose
            | Message::ExportScrollback
            | Message::CopyScrollback => self.handle_search_message(message),

            // --- State sync messages ---
            Message::StateSyncerReady(..) | Message::ServerStateLoaded(..) => {
                self.handle_state_sync_message(message)
            }

            Message::Noop => Task::none(),
        }
    }

    // -----------------------------------------------------------------------
    // SSH messages
    // -----------------------------------------------------------------------

    fn handle_ssh_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SshData(tab_id, data) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    if let Some(ref mut terminal) = tab.terminal {
                        terminal.process_ssh_data(&data);
                        // FR-HISTORY-02: write to local JSONL history
                        if let Some(ref mut writer) = tab.history_writer {
                            writer.append_output(&data);
                        }
                    }
                    // FR-TERMINAL-16: deferred initial resize — by the time data arrives,
                    // the terminal widget has definitely rendered and knows its real size
                    if tab.needs_initial_resize {
                        if let Some(ref terminal) = tab.terminal {
                            let (cols, rows) = terminal.terminal_size();
                            if cols > 0
                                && rows > 0
                                && let Some(resize_tx) = tab.resize_tx()
                            {
                                let _ = resize_tx.send((cols as u32, rows as u32));
                                tracing::info!(
                                    "tab {tab_id}: deferred initial resize {cols}x{rows}"
                                );
                            }
                        }
                        tab.needs_initial_resize = false;
                    }
                }
                Task::none()
            }

            Message::SshDisconnected(tab_id, reason) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    // Clear channel state so subscription stops
                    tab.clear_resize_tx();
                    // FR-UI-08: store last error for dead tab display
                    tab.last_error = Some(reason.clone());

                    // FR-RECONNECT-07: classify error
                    if ssh::errors::is_permanent(&reason) {
                        tab.terminal = None;
                        tab.mark_disconnected(Some(reason.clone()));
                        tracing::error!("permanent error for tab {tab_id}: {reason}");
                    } else {
                        // Check if we were reconnecting and can retry
                        let attempt = tab.reconnect_attempts();
                        let was_reconnectable = tab.is_auto_reconnect()
                            || matches!(
                                tab.conn_state,
                                super::tab::ConnectionState::Connected { .. }
                                    | super::tab::ConnectionState::Connecting { .. }
                            );
                        if was_reconnectable && attempt < self.config.ssh.reconnect_max_attempts {
                            let new_attempt = attempt + 1;
                            tab.terminal = None;
                            let phase = Arc::new(std::sync::Mutex::new(String::new()));
                            tab.mark_reconnecting(new_attempt, 0, phase, None);
                            tracing::info!("SSH tab {tab_id} disconnected: {reason}, will retry");
                        } else {
                            tab.terminal = None;
                            tab.mark_disconnected(Some(reason.clone()));
                            tracing::info!("SSH tab {tab_id} disconnected: {reason}");
                        }
                    }
                    self.update_title();
                }
                Task::none()
            }

            Message::SshConnected(tab_id, result) => self.handle_ssh_connected(tab_id, result),

            Message::ExistingSessionsFound(result) => self.handle_existing_sessions(result),

            Message::PasteToTerminal(tab_id, data) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    terminal.handle(iced_term::Command::ProxyToBackend(
                        iced_term::BackendCommand::Write(data),
                    ));
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }

    fn handle_ssh_connected(
        &mut self,
        tab_id: super::tab::TabId,
        result: Result<(), String>,
    ) -> Task<Message> {
        match result {
            Ok(()) => {
                // The async task wrote the channel into pending_channel.
                // Move it to Connected state so the subscription picks it up.
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                    && let Some(holder) = tab.take_pending_channel()
                {
                    tab.mark_connected(holder);
                    tracing::info!("SSH tab {tab_id}: connected, channel ready");

                    // FR-TERMINAL-16: send immediate resize to match actual
                    // terminal widget size (PTY was opened with default 80x24)
                    let size = tab.terminal.as_ref().map(|t| t.terminal_size());
                    if let Some((cols, rows)) = size {
                        tracing::info!("tab {tab_id}: terminal widget size {cols}x{rows}");
                        if cols > 0 && rows > 0 {
                            if let Some(resize_tx) = tab.resize_tx() {
                                let _ = resize_tx.send((cols as u32, rows as u32));
                            }
                            tab.needs_initial_resize = false;
                            tracing::info!("tab {tab_id}: sent initial resize {cols}x{rows}");
                        }
                    } else {
                        tracing::info!("tab {tab_id}: no terminal widget yet, resize deferred");
                    }
                }

                // After first successful connect, list existing tmux sessions
                if !self.sessions_listed && self.current_conn.is_some() {
                    self.sessions_listed = true;
                    let mgr = self.conn_manager.clone();
                    // SAFETY: is_some() checked on the line above
                    #[allow(clippy::unwrap_used)]
                    let conn = self.current_conn.clone().unwrap();
                    let conn_key = conn.key.clone();
                    // FR-CONN-20: open a separate connection for SFTP state sync
                    let mgr2 = self.conn_manager.clone();
                    // SAFETY: is_some() checked above
                    #[allow(clippy::unwrap_used)]
                    let conn2 = self.current_conn.clone().unwrap();
                    let conn_key2 = conn2.key.clone();
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
                                let syncer =
                                    ssh::sftp::StateSyncer::new(conn_result.handle, &client_id)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                Ok(Arc::new(syncer))
                            },
                            |result: Result<Arc<ssh::sftp::StateSyncer>, String>| {
                                Message::StateSyncerReady(result)
                            },
                        ),
                    ]);
                }
                Task::none()
            }
            Err(e) => {
                tracing::error!("SSH tab {tab_id}: connection failed: {e}");
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.terminal = None;
                    tab.last_error = Some(e.clone());
                    // FR-RECONNECT-07: classify error
                    if ssh::errors::is_permanent(&e) {
                        tab.mark_disconnected(Some(e.clone()));
                    } else {
                        let attempt = tab.reconnect_attempts() + 1;
                        let phase = Arc::new(std::sync::Mutex::new(String::new()));
                        tab.mark_reconnecting(attempt, 0, phase, None);
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
                    && !self.dialogs.show_password_dialog
                {
                    // FR-CONN-09: show password prompt dialog on auth failure
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        // Waiting for user input — not dead, not auto-reconnecting
                        tab.mark_disconnected(None);
                    }
                    tracing::info!("tab {tab_id}: auth failed, prompting for password");
                    self.dialogs.show_password_dialog = true;
                    self.dialogs.password_input.clear();
                    self.dialogs.password_target_tab = Some(tab_id);
                    self.dialogs.password_conn_params = self.current_conn.clone();
                } else if el.contains("session locked by") || el.contains("lock held by") {
                    // FR-LOCK-05: show lock conflict dialog
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.mark_disconnected(None);
                    }
                    tracing::info!("tab {tab_id}: session locked, showing conflict dialog");
                    self.dialogs.show_lock_dialog = true;
                    self.dialogs.lock_info_text = e.clone();
                    self.dialogs.lock_target_tab = Some(tab_id);
                } else if el.contains("auth failed") || el.contains("authentication failed") {
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
                Task::none()
            }
        }
    }

    fn handle_existing_sessions(&mut self, result: Result<Vec<String>, String>) -> Task<Message> {
        if let Err(ref e) = result {
            tracing::warn!("failed to list existing sessions: {e}");
        }
        if let Ok(server_sessions) = result {
            // Load split state (shared + device), migrating from legacy if needed
            let (shared_opt, device_opt) = state_file::load_split_state(&self.client_id);
            let saved_state = shared_opt;

            // FR-STATE-14: restore window geometry from device state
            if let Some(ref device) = device_opt
                && let Some(geo) = device.window_geometry.get("main")
            {
                self.window_x = geo.x;
                self.window_y = geo.y;
                self.window_width = geo.width;
                self.window_height = geo.height;
            }

            // Load hidden sessions from device state
            if let Some(ref device) = device_opt {
                self.hidden_sessions = device.hidden_sessions.clone();
            }

            // FR-ENV-05: restore last environment from saved state
            if let Some(ref saved) = saved_state
                && let Some(ref env_name) = saved.last_environment
            {
                self.current_environment = env_name.clone();
            }

            // FR-ENV-04: populate env_list from saved state
            if let Some(ref saved) = saved_state {
                self.dialogs.env_list = saved.environments.keys().cloned().collect();
                self.dialogs.env_list.sort();
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
                        } else if !tab.is_dead()
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
                            tab.mark_disconnected(None);
                        }
                    }
                }
            }

            // FR-SESSION-12: find orphaned sessions (on server but not in any tab).
            // Only restore sessions that exist in saved state — sessions the user
            // explicitly closed (close_tab kills tmux) should not be reopened.
            let existing_tmux: Vec<String> =
                self.tabs.iter().map(|t| t.tmux_session.clone()).collect();

            let mut restorable: Vec<(&str, &str)> = Vec::new();
            let mut stale: Vec<String> = Vec::new();

            for session in &server_sessions {
                if existing_tmux.contains(session) {
                    continue; // Already open in a tab
                }
                if let Some(saved) = saved_env_tabs
                    .iter()
                    .find(|t| t.tmux_session_name == *session)
                {
                    // Skip sessions that are in this device's hidden list
                    if self.hidden_sessions.contains(&saved.session_uuid) {
                        tracing::debug!(
                            "skipping hidden session {} ({})",
                            saved.session_uuid,
                            session
                        );
                        continue;
                    }
                    // In saved state — restore it
                    restorable.push((saved.title.as_str(), session.as_str()));
                } else if session.contains("--shellkeep-") {
                    // Shellkeep session NOT in saved state — orphan from a failed kill.
                    // Only clean up sessions with our naming convention to avoid
                    // touching user's manually-created tmux sessions.
                    stale.push(session.clone());
                }
            }

            // Build a task list: restore saved sessions + clean up stale orphans
            let mut tasks = Vec::new();

            // Clean up stale orphans
            if !stale.is_empty() {
                tracing::info!(
                    "cleaning up {} stale tmux session(s): {:?}",
                    stale.len(),
                    stale
                );
                let mgr = self.conn_manager.clone();
                if let Some(ref conn) = self.current_conn {
                    let conn_key = conn.key.clone();
                    tasks.push(Task::perform(
                        async move {
                            let mgr_guard = mgr.lock().await;
                            if let Some(handle_arc) = mgr_guard.get_cached(&conn_key) {
                                let handle = handle_arc.lock().await;
                                for name in &stale {
                                    let cmd = format!("tmux kill-session -t {name} 2>/dev/null");
                                    let _ = ssh::connection::exec_command(&handle, &cmd).await;
                                    tracing::info!("killed stale tmux session: {name}");
                                }
                            }
                        },
                        |_| Message::Noop,
                    ));
                }
            }

            if !restorable.is_empty() {
                tracing::info!(
                    "restoring {} saved session(s): {:?}",
                    restorable.len(),
                    restorable
                );
                for (label, session_name) in &restorable {
                    tasks.push(self.open_tab_russh(label, session_name));
                }
            }

            if !tasks.is_empty() {
                return Task::batch(tasks);
            }
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // Tab messages
    // -----------------------------------------------------------------------

    fn handle_tab_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectTab(index) => {
                if index < self.tabs.len() {
                    self.active_tab = index;
                    self.show_welcome = false;
                    self.renaming_tab = None;
                    self.tab_context_menu = None;
                    self.update_title();
                }
                Task::none()
            }

            // FR-SESSION-10a: close tab with confirmation for active sessions
            Message::CloseTab(index) => {
                self.tab_context_menu = None;
                if let Some(tab) = self.tabs.get(index)
                    && !tab.is_dead()
                    && tab.terminal.is_some()
                {
                    // Active session — ask confirmation
                    self.dialogs.pending_close_tabs = Some(vec![index]);
                    return Task::none();
                }
                // Dead/disconnected — close immediately
                self.close_tab(index)
            }

            Message::ConfirmCloseTabs => {
                if let Some(indices) = self.dialogs.pending_close_tabs.take() {
                    let mut tasks = Vec::new();
                    // Close from end to avoid index shifting
                    for idx in indices.into_iter().rev() {
                        tasks.push(self.close_tab(idx));
                    }
                    return Task::batch(tasks);
                }
                Task::none()
            }

            Message::CancelCloseTabs => {
                self.dialogs.pending_close_tabs = None;
                Task::none()
            }

            Message::NewTab => {
                if self.current_conn.is_some() {
                    let n = self.tabs.len() + 1;
                    let label = format!("Session {n}");
                    let tmux_session = self.next_tmux_session();
                    return self.open_tab_russh(&label, &tmux_session);
                } else if let Some(tab) = self.tabs.last() {
                    // Fallback: use system ssh args from existing tab
                    let ssh_args = tab.ssh_args().to_vec();
                    let n = self.tabs.len() + 1;
                    let label = format!("Session {n}");
                    self.open_tab_with_tmux(&ssh_args, &label);
                } else {
                    self.show_welcome = true;
                }
                Task::none()
            }

            Message::ReconnectTab(index) => {
                // Manual reconnect: reset state before calling reconnect_tab
                // which will set up Connecting state
                if index < self.tabs.len() {
                    self.tabs[index].mark_disconnected(None);
                }
                self.reconnect_tab(index)
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
                Task::none()
            }

            Message::HideTab(index) => {
                self.hide_tab(index);
                self.tab_context_menu = None;
                Task::none()
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
                        .is_some_and(|t| !t.is_dead() && t.terminal.is_some())
                });
                if has_active {
                    self.dialogs.pending_close_tabs = Some(to_close);
                } else {
                    let mut tasks = Vec::new();
                    for idx in to_close.into_iter().rev() {
                        tasks.push(self.close_tab(idx));
                    }
                    self.active_tab = 0;
                    return Task::batch(tasks);
                }
                Task::none()
            }

            Message::CloseTabsToRight(index) => {
                self.tab_context_menu = None;
                let to_close: Vec<usize> = (index + 1..self.tabs.len()).collect();
                let has_active = to_close.iter().any(|&i| {
                    self.tabs
                        .get(i)
                        .is_some_and(|t| !t.is_dead() && t.terminal.is_some())
                });
                if has_active {
                    self.dialogs.pending_close_tabs = Some(to_close);
                } else {
                    let mut tasks = Vec::new();
                    for idx in to_close.into_iter().rev() {
                        tasks.push(self.close_tab(idx));
                    }
                    return Task::batch(tasks);
                }
                Task::none()
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
                Task::none()
            }

            Message::RenameInputChanged(v) => {
                self.rename_input = v;
                Task::none()
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
                    if self.tabs[index].is_russh() && self.tabs[index].conn_params().is_some() {
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
                        let new_tmux =
                            format!("{}--shellkeep-{}", self.current_environment, sanitized);
                        self.tabs[index].tmux_session = new_tmux.clone();
                        let mgr = self.conn_manager.clone();
                        // SAFETY: conn_params().is_some() checked in the enclosing if-let
                        #[allow(clippy::unwrap_used)]
                        let conn = self.tabs[index].conn_params().cloned().unwrap();
                        rename_task = Task::perform(
                            async move {
                                let conn_key = conn.key.clone();
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
                            |_| Message::Noop,
                        );
                    }
                }
                self.renaming_tab = None;
                rename_task
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
                Task::none()
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
                Task::none()
            }

            Message::TabContextMenu(index, x, y) => {
                self.tab_context_menu = Some((index, x, y));
                self.context_menu = None;
                self.show_restore_dropdown = false;
                Task::none()
            }

            // FR-UI-01: clicking a recent connection fills the form
            Message::ConnectRecent(index) => {
                if let Some(conn) = self.recent.connections.get(index).cloned() {
                    self.welcome.host_input = conn.host;
                    self.welcome.user_input = conn.user;
                    self.welcome.port_input = conn.port;
                    self.welcome.identity_input = conn.identity_file.unwrap_or_default();
                    // Show advanced if non-default port or identity is set
                    if self.welcome.port_input != "22" || !self.welcome.identity_input.is_empty() {
                        self.welcome.show_advanced = true;
                    }
                }
                Task::none()
            }

            Message::ShowRestoreDropdown => {
                self.show_restore_dropdown = !self.show_restore_dropdown;
                Task::none()
            }

            Message::DismissRestoreDropdown => {
                self.show_restore_dropdown = false;
                Task::none()
            }

            Message::RestoreHiddenSession(session_uuid) => {
                self.show_restore_dropdown = false;

                // Find the hidden session in saved state to get its tmux name and title
                let shared_path = SharedState::local_cache_path();
                let saved_state = SharedState::load_local(&shared_path);
                let saved_env_tabs = saved_state
                    .as_ref()
                    .map(|s| s.env_tabs(&self.current_environment))
                    .unwrap_or_default();

                if let Some(saved_tab) = saved_env_tabs
                    .iter()
                    .find(|t| t.session_uuid == session_uuid)
                {
                    // Remove from hidden list
                    self.hidden_sessions.retain(|u| u != &session_uuid);

                    // Open the tab, reattaching to the existing tmux session
                    let label = saved_tab.title.clone();
                    let tmux = saved_tab.tmux_session_name.clone();
                    tracing::info!(
                        "restoring hidden session {} ({}) as tab",
                        session_uuid,
                        tmux
                    );
                    self.save_state();
                    return self.open_tab_russh(&label, &tmux);
                } else {
                    tracing::warn!(
                        "hidden session {} not found in saved state, removing from list",
                        session_uuid
                    );
                    self.hidden_sessions.retain(|u| u != &session_uuid);
                    self.save_state();
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }

    // -----------------------------------------------------------------------
    // Input / welcome screen messages
    // -----------------------------------------------------------------------

    fn handle_input_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::KeyEvent(event) => self.handle_key_event(event),

            // FR-UI-01: toggle advanced connection options
            Message::ToggleAdvanced => {
                self.welcome.show_advanced = !self.welcome.show_advanced;
                Task::none()
            }

            // FR-UI-03: client-id naming
            Message::ClientIdInputChanged(v) => {
                // Validate: only [a-zA-Z0-9_-], max 64 chars
                let filtered: String = v
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .take(64)
                    .collect();
                self.welcome.client_id_input = filtered;
                Task::none()
            }

            Message::HostInputChanged(v) => {
                // Detect pasted SSH commands: "ssh -p 2247 user@host" or "ssh user@host -i key"
                let trimmed = v.trim();
                if trimmed.starts_with("ssh ") {
                    let parts: Vec<&str> = trimmed.split_whitespace().skip(1).collect();
                    let parsed = crate::cli::parse_ssh_args(&parts);
                    self.welcome.host_input = parsed.host;
                    if parsed.port != 22 {
                        self.welcome.port_input = parsed.port.to_string();
                    }
                    if let Some(user) = parsed.username {
                        self.welcome.user_input = user;
                    }
                    if let Some(identity) = parsed.identity_file {
                        self.welcome.identity_input = identity;
                    }
                    // Auto-show advanced panel if non-default values were parsed
                    if self.welcome.port_input != "22"
                        || !self.welcome.user_input.is_empty()
                        || !self.welcome.identity_input.is_empty()
                    {
                        self.welcome.show_advanced = true;
                    }
                } else {
                    self.welcome.host_input = v;
                }
                Task::none()
            }

            Message::PortInputChanged(v) => {
                self.welcome.port_input = v;
                Task::none()
            }

            Message::UserInputChanged(v) => {
                self.welcome.user_input = v;
                Task::none()
            }

            Message::IdentityInputChanged(v) => {
                self.welcome.identity_input = v;
                Task::none()
            }

            Message::Connect => {
                if self.welcome.host_input.trim().is_empty() {
                    return Task::none();
                }
                // FR-UI-03: if user provided a client-id name on first use, save it
                if !self.welcome.client_id_input.is_empty()
                    && self.welcome.client_id_input != self.client_id
                {
                    self.client_id = self.welcome.client_id_input.clone();
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
                    crate::cli::parse_host_input(self.welcome.host_input.trim());
                let conn = super::tab::ConnParams {
                    key: ConnKey {
                        host: parsed_host,
                        port: parsed_port
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(self.welcome.port_input.trim().parse().unwrap_or(22)),
                        username: if !self.welcome.user_input.is_empty() {
                            self.welcome.user_input.clone()
                        } else {
                            parsed_user.unwrap_or_else(crate::cli::default_ssh_username)
                        },
                    },
                    identity_file: if self.welcome.identity_input.is_empty() {
                        None
                    } else {
                        Some(self.welcome.identity_input.clone())
                    },
                };
                self.current_conn = Some(conn);

                self.recent.push(RecentConnection {
                    label: label.clone(),
                    ssh_args: ssh_args.clone(),
                    host: self.welcome.host_input.clone(),
                    user: self.welcome.user_input.clone(),
                    port: self.welcome.port_input.clone(),
                    identity_file: if self.welcome.identity_input.is_empty() {
                        None
                    } else {
                        Some(self.welcome.identity_input.clone())
                    },
                    alias: None,
                    last_connected: None,
                    host_key_fingerprint: None,
                });
                self.recent.save();

                // Use russh for new connections: open tab immediately, connect async
                let tmux_session = self.next_tmux_session();
                self.show_welcome = false;
                self.open_tab_russh(&label, &tmux_session)
            }

            _ => Task::none(),
        }
    }

    fn handle_key_event(&mut self, event: keyboard::Event) -> Task<Message> {
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
                    let ssh_args = tab.ssh_args().to_vec();
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
            if self.dialogs.pending_close_tabs.is_some() {
                if key == keyboard::Key::Named(keyboard::key::Named::Enter) {
                    return self.update(Message::ConfirmCloseTabs);
                }
                if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                    return self.update(Message::CancelCloseTabs);
                }
            }
            // Escape — dismiss search, context menu, cancel rename, or cancel welcome
            if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                if self.search.active {
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
                    || self.dialogs.show_env_dialog
                    || self.dialogs.show_new_env_dialog
                    || self.dialogs.show_rename_env_dialog
                    || self.search.active
                    || self.renaming_tab.is_some())
            {
                return if modifiers.shift() {
                    iced_runtime::widget::operation::focus_previous()
                } else {
                    iced_runtime::widget::operation::focus_next()
                };
            }
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // Dialog messages
    // -----------------------------------------------------------------------

    fn handle_dialog_message(&mut self, message: Message) -> Task<Message> {
        match message {
            // FR-TABS-17: window close requested by window manager
            Message::WindowCloseRequested(win_id) => {
                let active_count = self
                    .tabs
                    .iter()
                    .filter(|t| !t.is_dead() && t.terminal.is_some())
                    .count();
                if active_count == 0 {
                    // FR-TABS-18: no active sessions, close immediately
                    self.flush_state();
                    return window::close(win_id);
                }
                // Show confirmation dialog, remember which window to close
                self.dialogs.close_window_id = Some(win_id);
                self.dialogs.show_close_dialog = true;
                Task::none()
            }

            Message::CloseDialogClose => {
                self.dialogs.show_close_dialog = false;
                self.flush_state();
                // FR-LOCK-10: lock is released via orphan detection (2x keepalive timeout)
                // when the SSH connection drops on process exit.
                if let Some(id) = self.dialogs.close_window_id.take() {
                    return window::close(id);
                }
                std::process::exit(0);
            }

            Message::CloseDialogCancel => {
                self.dialogs.show_close_dialog = false;
                self.dialogs.close_window_id = None;
                Task::none()
            }

            // FR-ENV-03: environment selection dialog
            Message::ShowEnvDialog => {
                // FR-ENV-04: if only one environment, select it directly
                if self.dialogs.env_list.len() == 1 {
                    let env_name = self.dialogs.env_list[0].clone();
                    if env_name != self.current_environment {
                        return self.update(Message::SwitchEnvironment(env_name));
                    }
                    return Task::none();
                }
                self.dialogs.show_env_dialog = true;
                self.dialogs.env_filter.clear();
                // Pre-select current environment
                self.dialogs.selected_env = Some(self.current_environment.clone());
                Task::none()
            }

            Message::EnvFilterChanged(filter) => {
                self.dialogs.env_filter = filter;
                Task::none()
            }

            Message::SelectEnv(name) => {
                self.dialogs.selected_env = Some(name);
                Task::none()
            }

            Message::ConfirmEnv => {
                if let Some(ref env_name) = self.dialogs.selected_env {
                    let env_name = env_name.clone();
                    self.dialogs.show_env_dialog = false;
                    if env_name != self.current_environment {
                        return self.update(Message::SwitchEnvironment(env_name));
                    }
                }
                Task::none()
            }

            Message::NewEnvFromDialog => {
                // Close env selection, open new-env creation
                self.dialogs.show_env_dialog = false;
                self.dialogs.new_env_input.clear();
                self.dialogs.show_new_env_dialog = true;
                Task::none()
            }

            Message::CancelEnvDialog => {
                self.dialogs.show_env_dialog = false;
                Task::none()
            }

            // FR-ENV-07: create new environment
            Message::ShowNewEnvDialog => {
                self.dialogs.new_env_input.clear();
                self.dialogs.show_new_env_dialog = true;
                Task::none()
            }

            Message::NewEnvInputChanged(input) => {
                self.dialogs.new_env_input = input;
                Task::none()
            }

            Message::ConfirmNewEnv => {
                let name = self.dialogs.new_env_input.trim().to_string();
                if !name.is_empty() && !self.dialogs.env_list.contains(&name) {
                    self.dialogs.env_list.push(name.clone());
                    self.dialogs.env_list.sort();
                    self.current_environment = name;
                    self.toast = Some((
                        format!("Environment \"{}\" created", self.current_environment),
                        std::time::Instant::now(),
                    ));
                    self.state_dirty = true;
                    self.flush_state();
                }
                self.dialogs.show_new_env_dialog = false;
                self.dialogs.new_env_input.clear();
                Task::none()
            }

            Message::CancelNewEnv => {
                self.dialogs.show_new_env_dialog = false;
                self.dialogs.new_env_input.clear();
                Task::none()
            }

            // FR-ENV-08: rename environment
            Message::ShowRenameEnvDialog(name) => {
                self.dialogs.rename_env_target = Some(name.clone());
                self.dialogs.rename_env_input = name;
                self.dialogs.show_rename_env_dialog = true;
                Task::none()
            }

            Message::RenameEnvInputChanged(input) => {
                self.dialogs.rename_env_input = input;
                Task::none()
            }

            Message::ConfirmRenameEnv => {
                let new_name = self.dialogs.rename_env_input.trim().to_string();
                if let Some(ref old_name) = self.dialogs.rename_env_target
                    && !new_name.is_empty()
                    && new_name != *old_name
                {
                    if let Some(entry) = self.dialogs.env_list.iter_mut().find(|e| *e == old_name) {
                        *entry = new_name.clone();
                    }
                    self.dialogs.env_list.sort();
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
                self.dialogs.show_rename_env_dialog = false;
                self.dialogs.rename_env_input.clear();
                self.dialogs.rename_env_target = None;
                Task::none()
            }

            Message::CancelRenameEnv => {
                self.dialogs.show_rename_env_dialog = false;
                self.dialogs.rename_env_input.clear();
                self.dialogs.rename_env_target = None;
                Task::none()
            }

            // FR-ENV-09: delete environment
            Message::ShowDeleteEnvDialog(name) => {
                self.dialogs.delete_env_target = Some(name);
                self.dialogs.show_delete_env_dialog = true;
                Task::none()
            }

            Message::ConfirmDeleteEnv => {
                if let Some(ref name) = self.dialogs.delete_env_target {
                    let name = name.clone();
                    self.dialogs.env_list.retain(|e| *e != name);
                    if self.current_environment == name {
                        self.current_environment = self
                            .dialogs
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
                self.dialogs.show_delete_env_dialog = false;
                self.dialogs.delete_env_target = None;
                Task::none()
            }

            Message::CancelDeleteEnv => {
                self.dialogs.show_delete_env_dialog = false;
                self.dialogs.delete_env_target = None;
                Task::none()
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
                Task::none()
            }

            // FR-CONN-03: host key TOFU — accept and save to known_hosts
            Message::HostKeyAcceptSave => {
                self.dialogs.pending_host_key_prompt = None;
                Task::none()
            }
            Message::HostKeyConnectOnce => {
                if let Some(ref prompt) = self.dialogs.pending_host_key_prompt {
                    let _ = ssh::known_hosts::remove_host_key(&prompt.host, prompt.port);
                }
                self.dialogs.pending_host_key_prompt = None;
                Task::none()
            }
            Message::HostKeyReject => {
                if let Some(ref prompt) = self.dialogs.pending_host_key_prompt {
                    let _ = ssh::known_hosts::remove_host_key(&prompt.host, prompt.port);
                }
                self.dialogs.pending_host_key_prompt = None;
                for tab in &mut self.tabs {
                    tab.mark_disconnected(Some("Host key rejected by user".to_string()));
                }
                self.error = Some("Connection cancelled: host key rejected.".to_string());
                Task::none()
            }
            Message::HostKeyChangedDismiss => {
                self.dialogs.pending_host_key_prompt = None;
                Task::none()
            }

            // FR-CONN-09: password auth dialog
            Message::PasswordInputChanged(val) => {
                self.dialogs.password_input = val;
                Task::none()
            }
            Message::PasswordSubmit => self.handle_password_submit(),
            Message::PasswordCancel => {
                self.dialogs.show_password_dialog = false;
                self.dialogs.password_input.clear();
                if let Some(tab_id) = self.dialogs.password_target_tab.take()
                    && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.mark_disconnected(Some("Authentication cancelled".to_string()));
                }
                self.error = Some("Authentication cancelled.".to_string());
                Task::none()
            }

            // FR-LOCK-05: lock conflict — take over
            Message::LockTakeOver => self.handle_lock_takeover(),
            Message::LockCancel => {
                self.dialogs.show_lock_dialog = false;
                if let Some(tab_id) = self.dialogs.lock_target_tab.take()
                    && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
                {
                    tab.mark_disconnected(Some("Lock takeover cancelled".to_string()));
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }

    fn handle_password_submit(&mut self) -> Task<Message> {
        self.dialogs.show_password_dialog = false;
        let password = self.dialogs.password_input.clone();
        self.dialogs.password_input.clear();

        if let Some(tab_id) = self.dialogs.password_target_tab.take()
            && let Some(conn) = self
                .dialogs
                .password_conn_params
                .take()
                .or(self.current_conn.clone())
        {
            let mgr = self.conn_manager.clone();
            let conn_key = conn.key.clone();

            if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                let phase = Arc::new(std::sync::Mutex::new(String::new()));
                tab.last_error = None;

                let channel_holder: ChannelHolder = Arc::new(Mutex::new(None));
                tab.mark_connecting(phase.clone(), channel_holder.clone());
                let tmux = tab.tmux_session.clone();
                let suuid = tab.session_uuid.clone();

                // Remove cached connection before retrying with password
                let mgr2 = mgr.clone();
                let conn_key2 = conn_key.clone();
                let connect_task = self.start_ssh_connection(
                    tab_id,
                    &conn,
                    &tmux,
                    &suuid,
                    phase,
                    channel_holder,
                    Some(password),
                    false,
                );
                return Task::perform(
                    async move {
                        let mut m = mgr2.lock().await;
                        m.remove(&conn_key2);
                    },
                    |_| Message::Noop,
                )
                .chain(connect_task);
            }
        }
        Task::none()
    }

    fn handle_lock_takeover(&mut self) -> Task<Message> {
        self.dialogs.show_lock_dialog = false;
        if let Some(tab_id) = self.dialogs.lock_target_tab.take()
            && let Some(conn) = self.current_conn.clone()
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
        {
            let phase = Arc::new(std::sync::Mutex::new(String::new()));
            tab.last_error = None;

            let channel_holder: ChannelHolder = Arc::new(Mutex::new(None));
            tab.mark_connecting(phase.clone(), channel_holder.clone());
            let tmux = tab.tmux_session.clone();
            let suuid = tab.session_uuid.clone();

            return self.start_ssh_connection(
                tab_id,
                &conn,
                &tmux,
                &suuid,
                phase,
                channel_holder,
                None,
                true,
            );
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // Timer / periodic messages
    // -----------------------------------------------------------------------

    fn handle_timer_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AutoReconnectTick => self.handle_auto_reconnect(),

            // FR-RECONNECT-08: network change detected — force immediate reconnect
            Message::NetworkChanged => {
                #[cfg(target_os = "linux")]
                {
                    let current = shellkeep::network::read_default_gateway();
                    if current != self.last_gateway {
                        tracing::info!(
                            "network change detected (gateway {:?} -> {:?}), triggering immediate reconnect",
                            self.last_gateway,
                            current
                        );
                        self.last_gateway = current;
                        for tab in &mut self.tabs {
                            if tab.is_auto_reconnect() {
                                tab.reset_reconnect();
                            }
                        }
                    }
                }
                Task::none()
            }

            Message::SpinnerTick => {
                // FR-RECONNECT-02: advance spinner frame
                self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
                Task::none()
            }

            Message::FlushState => {
                self.flush_state();
                Task::none()
            }

            Message::ConnectionPhaseTick => {
                // Just triggers a redraw to update connection phase text
                Task::none()
            }

            // FR-LOCK-04: periodic heartbeat to keep the lock alive
            Message::LockHeartbeatTick => {
                let mgr = self.conn_manager.clone();
                let conn = match &self.current_conn {
                    Some(c) => c.clone(),
                    None => return Task::none(),
                };
                let conn_key = conn.key.clone();
                Task::perform(
                    async move {
                        let mgr = mgr.lock().await;
                        if let Some(handle_arc) = mgr.get_cached(&conn_key) {
                            let handle = handle_arc.lock().await;
                            ssh::lock::heartbeat(&handle)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            Ok(()) // No connection, skip heartbeat
                        }
                    },
                    Message::LockHeartbeatDone,
                )
            }

            Message::LockHeartbeatDone(result) => {
                if let Err(e) = result {
                    tracing::warn!("lock heartbeat failed: {e}");
                }
                Task::none()
            }

            // FR-UI-04/05: latency measurement
            Message::LatencyTick => {
                let mgr = self.conn_manager.clone();
                let conn = match &self.current_conn {
                    Some(c) => c.clone(),
                    None => return Task::none(),
                };
                let conn_key = conn.key.clone();
                // Collect tab IDs that are connected via russh
                let tab_ids: Vec<super::tab::TabId> = self
                    .tabs
                    .iter()
                    .filter(|t| t.is_russh() && !t.is_dead() && t.terminal.is_some())
                    .map(|t| t.id)
                    .collect();
                if tab_ids.is_empty() {
                    return Task::none();
                }
                Task::perform(
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
                    move |(ids, latency): (Vec<super::tab::TabId>, Option<u32>)| {
                        // Send measurement for the first tab; the update handler
                        // applies the same latency to all tabs on this connection.
                        if let Some(&first) = ids.first() {
                            Message::LatencyMeasured(first, latency)
                        } else {
                            Message::LatencyMeasured(super::tab::TabId(0), None)
                        }
                    },
                )
            }

            Message::LatencyMeasured(_, latency) => {
                // All tabs on the same connection share the same latency
                if self.current_conn.is_some() {
                    for tab in &mut self.tabs {
                        if tab.is_russh() && !tab.is_dead() && tab.terminal.is_some() {
                            tab.last_latency_ms = latency;
                        }
                    }
                }
                Task::none()
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
                Task::none()
            }

            // FR-CONFIG-04: config file changed, reload hot-reloadable settings
            Message::ConfigReloaded => self.handle_config_reload(),

            _ => Task::none(),
        }
    }

    fn handle_auto_reconnect(&mut self) -> Task<Message> {
        // FR-RECONNECT-05: limit concurrent reconnections to 5
        let reconnecting_count = self
            .tabs
            .iter()
            .filter(|t| {
                t.is_russh()
                    && t.terminal.is_some()
                    && !t.has_channel()
                    && t.pending_channel().is_some()
            })
            .count();

        if reconnecting_count >= 5 {
            tracing::debug!("skipping auto-reconnect: {reconnecting_count} already in progress");
            return Task::none();
        }

        let reconnect_indices: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_auto_reconnect())
            .map(|(i, _)| i)
            .collect();

        if let Some(&index) = reconnect_indices.first() {
            let attempt = self.tabs[index].reconnect_attempts();
            let next_delay =
                reconnect_backoff_delay(self.config.ssh.reconnect_backoff_base, attempt);
            self.tabs[index].set_reconnect_delay_ms(next_delay);
            tracing::info!(
                "auto-reconnecting tab {} (attempt {}, next delay {}ms)",
                self.tabs[index].id,
                self.tabs[index].reconnect_attempts(),
                next_delay,
            );
            return self.reconnect_tab(index);
        }
        Task::none()
    }

    fn handle_config_reload(&mut self) -> Task<Message> {
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
        Task::none()
    }

    // -----------------------------------------------------------------------
    // Terminal / context menu messages
    // -----------------------------------------------------------------------

    fn handle_terminal_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TerminalEvent(iced_term::Event::ContextMenu(_id, x, y)) => {
                self.context_menu = Some((x, y));
                self.renaming_tab = None;
                self.tab_context_menu = None;
                Task::none()
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
                Task::none()
            }

            // FR-TABS-11: context menu paste — read clipboard and send to terminal
            Message::ContextMenuPaste => {
                self.context_menu = None;
                let tab_id = self
                    .tabs
                    .get(self.active_tab)
                    .map(|t| t.id)
                    .unwrap_or(super::tab::TabId(0));
                iced::clipboard::read().map(move |text| {
                    if let Some(text) = text {
                        Message::PasteToTerminal(tab_id, text.into_bytes())
                    } else {
                        Message::Noop
                    }
                })
            }

            Message::ContextMenuDismiss => {
                self.context_menu = None;
                self.tab_context_menu = None;
                self.renaming_tab = None;
                self.show_restore_dropdown = false;
                Task::none()
            }

            Message::ToastDismiss => {
                self.toast = None;
                Task::none()
            }

            Message::TerminalEvent(iced_term::Event::BackendCall(id, cmd)) => {
                self.handle_terminal_backend_call(super::tab::TabId(id), cmd)
            }

            // FR-STATE-14: track window geometry changes
            Message::WindowMoved(pos) => {
                self.window_x = Some(pos.x as i32);
                self.window_y = Some(pos.y as i32);
                self.save_geometry();
                Task::none()
            }

            Message::WindowResized(size) => {
                self.window_width = size.width as u32;
                self.window_height = size.height as u32;
                self.save_geometry();
                Task::none()
            }

            _ => Task::none(),
        }
    }

    fn handle_terminal_backend_call(
        &mut self,
        id: super::tab::TabId,
        cmd: iced_term::BackendCommand,
    ) -> Task<Message> {
        let is_resize = matches!(&cmd, iced_term::BackendCommand::Resize(..));
        let mut needs_title_update = false;
        let mut shutdown = false;
        let mut resize_info: Option<(u32, u32)> = None;

        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            let is_russh = tab.is_russh();
            if let Some(ref mut terminal) = tab.terminal {
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
                if is_resize && is_russh && !shutdown {
                    let (cols, rows) = terminal.terminal_size();
                    if cols > 0 && rows > 0 {
                        resize_info = Some((cols as u32, rows as u32));
                    }
                }
            }
        }

        // Handle shutdown after terminal borrow is released
        if shutdown && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.terminal = None;
            let attempt = tab.reconnect_attempts();
            let was_auto = tab.is_auto_reconnect()
                || matches!(
                    tab.conn_state,
                    super::tab::ConnectionState::Connected { .. }
                        | super::tab::ConnectionState::Connecting { .. }
                );
            if was_auto && attempt < self.config.ssh.reconnect_max_attempts {
                let new_attempt = attempt + 1;
                let phase = Arc::new(std::sync::Mutex::new(String::new()));
                tab.mark_reconnecting(new_attempt, 0, phase, None);
                tracing::info!(
                    "tab {id} disconnected, will auto-reconnect (attempt {})",
                    new_attempt
                );
            } else {
                tab.mark_disconnected(None);
                tracing::info!("tab {id} session ended (no more retries)");
            }
        }

        // Propagate resize to SSH channel
        if let Some((cols, rows)) = resize_info
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
        {
            if tab.needs_initial_resize {
                tracing::info!("tab {id}: initial terminal size {cols}x{rows}, sending to SSH");
                tab.needs_initial_resize = false;
            }
            if let Some(resize_tx) = tab.resize_tx() {
                let _ = resize_tx.send((cols, rows));
            }
        }

        if needs_title_update {
            self.update_title();
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // Search messages
    // -----------------------------------------------------------------------

    fn handle_search_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SearchToggle => {
                self.search.active = !self.search.active;
                if !self.search.active {
                    self.search.input.clear();
                    self.search.regex = None;
                    self.search.last_match = None;
                    Task::none()
                } else {
                    iced_runtime::widget::operation::focus("search-input")
                }
            }

            Message::SearchInputChanged(v) => {
                self.search.input = v;
                if self.search.input.is_empty() {
                    self.search.regex = None;
                    self.search.last_match = None;
                    Task::none()
                } else {
                    let escaped = super::escape_regex(&self.search.input);
                    self.search.regex = RegexSearch::new(&escaped).ok();
                    if self.search.regex.is_some() {
                        self.update(Message::SearchNext)
                    } else {
                        Task::none()
                    }
                }
            }

            Message::SearchNext => {
                if let Some(ref mut regex) = self.search.regex
                    && let Some(tab) = self.tabs.get_mut(self.active_tab)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    let origin = self
                        .search
                        .last_match
                        .as_ref()
                        .map(|m| {
                            let mut p = *m.end();
                            p.column.0 += 1;
                            p
                        })
                        .unwrap_or(AlacrittyPoint::new(AlacrittyLine(0), AlacrittyColumn(0)));
                    self.search.last_match = terminal.search_next(regex, origin);
                }
                Task::none()
            }

            Message::SearchPrev => {
                if let Some(ref mut regex) = self.search.regex
                    && let Some(tab) = self.tabs.get_mut(self.active_tab)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    let origin = self
                        .search
                        .last_match
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
                    self.search.last_match = terminal.search_prev(regex, origin);
                }
                Task::none()
            }

            Message::SearchClose => {
                self.search.active = false;
                self.search.input.clear();
                self.search.regex = None;
                self.search.last_match = None;
                Task::none()
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
                Task::none()
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
                Task::none()
            }

            _ => Task::none(),
        }
    }

    // -----------------------------------------------------------------------
    // State sync messages
    // -----------------------------------------------------------------------

    fn handle_state_sync_message(&mut self, message: Message) -> Task<Message> {
        match message {
            // FR-CONN-20: state syncer initialized
            Message::StateSyncerReady(result) => {
                match result {
                    Ok(syncer) => {
                        let transport = if syncer.is_sftp() { "SFTP" } else { "shell" };
                        tracing::info!("state syncer ready (transport: {transport})");
                        let syncer_clone = syncer.clone();
                        self.state_syncer = Some(syncer);
                        // FR-STATE-02: read server state (takes precedence over local)
                        Task::perform(
                            async move { syncer_clone.read_state().await.map_err(|e| e.to_string()) },
                            Message::ServerStateLoaded,
                        )
                    }
                    Err(e) => {
                        tracing::warn!("state syncer init failed: {e}");
                        Task::none()
                    }
                }
            }

            // FR-STATE-02: server state loaded (shared state from server)
            Message::ServerStateLoaded(result) => {
                match result {
                    Ok(Some(json)) => {
                        match serde_json::from_str::<SharedState>(&json) {
                            Ok(server_state) => {
                                tracing::info!(
                                    "loaded server shared state: {} environments",
                                    server_state.environments.len()
                                );
                                // Server state takes precedence — update local cache
                                let path = SharedState::local_cache_path();
                                if let Err(e) = server_state.save_local(&path) {
                                    tracing::warn!("failed to cache server shared state: {e}");
                                }
                            }
                            Err(e) => {
                                tracing::warn!("corrupt server shared state: {e}");
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::debug!("no server shared state found, using local");
                    }
                    Err(e) => {
                        tracing::warn!("failed to read server shared state: {e}");
                    }
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }
}

/// FR-RECONNECT-06: calculate exponential backoff delay with jitter.
///
/// Pure function extracted from `handle_auto_reconnect` for testability.
/// Returns the delay in milliseconds for the given attempt number.
///
/// Formula: base_ms * 2^(attempt-1), capped at 60s, with +/-25% jitter,
/// floored at base_ms.
pub(crate) fn reconnect_backoff_delay(backoff_base_secs: f64, attempt: u32) -> u64 {
    let base_ms = (backoff_base_secs * 1000.0) as u64;
    let exp_delay = base_ms.saturating_mul(
        1u64.checked_shl(attempt.saturating_sub(1))
            .unwrap_or(u64::MAX),
    );
    let capped = exp_delay.min(60_000);
    use rand::Rng;
    let jitter_range = capped / 4;
    let jitter = if jitter_range > 0 {
        rand::rng().random_range(0..jitter_range * 2) as i64 - jitter_range as i64
    } else {
        0
    };
    (capped as i64 + jitter).max(base_ms as i64) as u64
}

/// Compute the new active_tab index after removing a tab at `removed_index`.
///
/// Pure function extracted from `close_tab`/`hide_tab` for testability.
pub(crate) fn active_tab_after_removal(
    active_tab: usize,
    tab_count_before: usize,
    removed_index: usize,
) -> usize {
    debug_assert!(removed_index < tab_count_before);
    let new_len = tab_count_before - 1;
    if new_len == 0 {
        return 0;
    }
    if active_tab >= new_len && active_tab > 0 {
        active_tab - 1
    } else {
        active_tab
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // Exponential backoff
    // -------------------------------------------------------------------

    #[test]
    fn backoff_attempt_1_is_base() {
        // Attempt 1: base * 2^0 = base (with jitter)
        let base_secs = 2.0;
        let base_ms = 2000u64;
        for _ in 0..50 {
            let delay = reconnect_backoff_delay(base_secs, 1);
            // Jitter is +/-25%, so range is [base*0.75, base*1.25]
            assert!(
                delay >= base_ms * 3 / 4 && delay <= base_ms * 5 / 4,
                "attempt 1 delay {delay} out of range [{}, {}]",
                base_ms * 3 / 4,
                base_ms * 5 / 4,
            );
        }
    }

    #[test]
    fn backoff_doubles_each_attempt() {
        // Without jitter noise, verify the central tendency doubles.
        // Run many samples and check the average is close to expected.
        let base_secs = 1.0;
        for attempt in 1..=5 {
            let expected_center = 1000u64 * (1u64 << (attempt - 1));
            let expected_center = expected_center.min(60_000);
            let sum: u64 = (0..200)
                .map(|_| reconnect_backoff_delay(base_secs, attempt))
                .sum();
            let avg = sum / 200;
            let tolerance = expected_center / 3; // generous tolerance for randomness
            assert!(
                avg.abs_diff(expected_center) < tolerance,
                "attempt {attempt}: avg {avg} too far from expected {expected_center}",
            );
        }
    }

    #[test]
    fn backoff_capped_at_60s() {
        // Very high attempt should not exceed 60s + jitter
        let base_secs = 2.0;
        for _ in 0..50 {
            let delay = reconnect_backoff_delay(base_secs, 100);
            // Cap is 60_000, jitter +25% max = 75_000, but floor is base_ms
            assert!(delay <= 75_000, "delay {delay} exceeds cap+jitter");
            assert!(delay >= 2000, "delay {delay} below base_ms floor");
        }
    }

    #[test]
    fn backoff_attempt_0_uses_base() {
        // Attempt 0 (edge case): 2^(0-1) wraps to 2^(u32::MAX) = overflow -> u64::MAX
        // saturating_mul with base gives u64::MAX, capped at 60s
        let base_secs = 2.0;
        for _ in 0..20 {
            let delay = reconnect_backoff_delay(base_secs, 0);
            // Should be capped at 60s with jitter
            assert!(delay >= 2000, "delay {delay} below base_ms floor");
            assert!(delay <= 75_000, "delay {delay} exceeds cap+jitter");
        }
    }

    #[test]
    fn backoff_never_below_base() {
        // Even with max negative jitter, should never go below base_ms
        let base_secs = 3.0;
        for attempt in 1..=10 {
            for _ in 0..50 {
                let delay = reconnect_backoff_delay(base_secs, attempt);
                assert!(
                    delay >= 3000,
                    "delay {delay} below base 3000ms at attempt {attempt}"
                );
            }
        }
    }

    // -------------------------------------------------------------------
    // Tab index adjustment after removal
    // -------------------------------------------------------------------

    #[test]
    fn active_tab_stays_when_removing_after() {
        // 5 tabs, active=1, remove tab 3 -> active stays 1
        assert_eq!(active_tab_after_removal(1, 5, 3), 1);
    }

    #[test]
    fn active_tab_decrements_when_at_end() {
        // 3 tabs, active=2, remove tab 2 -> active=1
        assert_eq!(active_tab_after_removal(2, 3, 2), 1);
    }

    #[test]
    fn active_tab_stays_when_removing_before() {
        // 4 tabs, active=2, remove tab 0 -> active stays 2
        // (close_tab/hide_tab don't shift active when removing before,
        // they only clamp if active >= new_len)
        assert_eq!(active_tab_after_removal(2, 4, 0), 2);
    }

    #[test]
    fn active_tab_clamps_to_last() {
        // 2 tabs, active=1, remove tab 0 -> new_len=1, active=1 >= 1 -> active=0
        assert_eq!(active_tab_after_removal(1, 2, 0), 0);
    }

    #[test]
    fn active_tab_zero_when_single_tab_removed() {
        // 1 tab, active=0, remove tab 0 -> returns 0
        assert_eq!(active_tab_after_removal(0, 1, 0), 0);
    }

    #[test]
    fn active_tab_stays_zero() {
        // 3 tabs, active=0, remove tab 2 -> active stays 0
        assert_eq!(active_tab_after_removal(0, 3, 2), 0);
    }

    // -------------------------------------------------------------------
    // SSH command paste detection (logic from HostInputChanged)
    // -------------------------------------------------------------------

    /// Simulates the SSH paste detection logic from handle_input_message.
    fn simulate_ssh_paste(input: &str) -> super::super::WelcomeState {
        let mut state = super::super::WelcomeState {
            client_id_input: String::new(),
            show_advanced: false,
            host_input: String::new(),
            port_input: "22".to_string(),
            user_input: String::new(),
            identity_input: String::new(),
        };

        let trimmed = input.trim();
        if trimmed.starts_with("ssh ") {
            let parts: Vec<&str> = trimmed.split_whitespace().skip(1).collect();
            let parsed = crate::cli::parse_ssh_args(&parts);
            state.host_input = parsed.host;
            if parsed.port != 22 {
                state.port_input = parsed.port.to_string();
            }
            if let Some(user) = parsed.username {
                state.user_input = user;
            }
            if let Some(identity) = parsed.identity_file {
                state.identity_input = identity;
            }
            if state.port_input != "22"
                || !state.user_input.is_empty()
                || !state.identity_input.is_empty()
            {
                state.show_advanced = true;
            }
        } else {
            state.host_input = input.to_string();
        }

        state
    }

    #[test]
    fn paste_ssh_command_basic() {
        let state = simulate_ssh_paste("ssh user@example.com");
        assert_eq!(state.host_input, "example.com");
        assert_eq!(state.user_input, "user");
        assert_eq!(state.port_input, "22");
        assert!(state.show_advanced); // user is non-empty
    }

    #[test]
    fn paste_ssh_command_with_port() {
        let state = simulate_ssh_paste("ssh -p 2222 root@myserver.io");
        assert_eq!(state.host_input, "myserver.io");
        assert_eq!(state.user_input, "root");
        assert_eq!(state.port_input, "2222");
        assert!(state.show_advanced);
    }

    #[test]
    fn paste_ssh_command_with_identity() {
        let state = simulate_ssh_paste("ssh -i /home/me/.ssh/key user@host");
        assert_eq!(state.host_input, "host");
        assert_eq!(state.user_input, "user");
        assert_eq!(state.identity_input, "/home/me/.ssh/key");
        assert!(state.show_advanced);
    }

    #[test]
    fn paste_ssh_command_port_after_host() {
        let state = simulate_ssh_paste("ssh alice@server.com -p 3333");
        assert_eq!(state.host_input, "server.com");
        assert_eq!(state.user_input, "alice");
        assert_eq!(state.port_input, "3333");
        assert!(state.show_advanced);
    }

    #[test]
    fn paste_ssh_command_host_only() {
        let state = simulate_ssh_paste("ssh example.com");
        assert_eq!(state.host_input, "example.com");
        assert_eq!(state.user_input, "");
        assert_eq!(state.port_input, "22");
        assert!(!state.show_advanced); // nothing non-default
    }

    #[test]
    fn paste_normal_hostname() {
        let state = simulate_ssh_paste("example.com");
        assert_eq!(state.host_input, "example.com");
        assert_eq!(state.user_input, "");
        assert!(!state.show_advanced);
    }

    #[test]
    fn paste_ssh_with_leading_whitespace() {
        let state = simulate_ssh_paste("  ssh -p 22 root@box  ");
        assert_eq!(state.host_input, "box");
        assert_eq!(state.user_input, "root");
        assert!(state.show_advanced);
    }

    // -------------------------------------------------------------------
    // Client ID input validation
    // -------------------------------------------------------------------

    fn filter_client_id(input: &str) -> String {
        input
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(64)
            .collect()
    }

    #[test]
    fn client_id_strips_special_chars() {
        assert_eq!(filter_client_id("my laptop!@#$"), "mylaptop");
    }

    #[test]
    fn client_id_allows_dashes_underscores() {
        assert_eq!(filter_client_id("work-laptop_01"), "work-laptop_01");
    }

    #[test]
    fn client_id_truncates_at_64() {
        let long = "a".repeat(100);
        assert_eq!(filter_client_id(&long).len(), 64);
    }

    #[test]
    fn client_id_empty_input() {
        assert_eq!(filter_client_id(""), "");
    }

    // -------------------------------------------------------------------
    // escape_regex
    // -------------------------------------------------------------------

    #[test]
    fn escape_regex_special_chars() {
        assert_eq!(
            super::super::escape_regex("hello.world*"),
            "hello\\.world\\*"
        );
    }

    #[test]
    fn escape_regex_no_specials() {
        assert_eq!(super::super::escape_regex("foobar"), "foobar");
    }

    #[test]
    fn escape_regex_all_specials() {
        let input = r"\\.+*?()|[]{}^$";
        let escaped = super::super::escape_regex(input);
        // Every char should be preceded by backslash
        for c in input.chars() {
            let expected = format!("\\{c}");
            assert!(escaped.contains(&expected), "missing escape for '{c}'");
        }
    }
}
