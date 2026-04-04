// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Message dispatch and state update logic.
//!
// TODO: migrate from RecentConnections to SavedServers, then remove this allow
#![allow(deprecated)]
//!
//! This is the core of the iced application: every user action, SSH event,
//! and timer tick arrives as a [`Message`] and is routed by [`ShellKeep::update`]
//! to one of seven handler methods:
//!
//! - `handle_ssh_message` — SSH data, connect/disconnect, session discovery
//! - `handle_tab_message` — tab open/close/move/rename, recent connections
//! - `handle_input_message` — welcome screen form, keyboard shortcuts
//! - `handle_dialog_message` — close, workspace, host-key, password, lock dialogs
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
use shellkeep::i18n;
use shellkeep::ssh;
use shellkeep::ssh::manager::ConnKey;
#[allow(deprecated)] // legacy type kept for migration
use shellkeep::state::recent::RecentConnection;
use shellkeep::state::state_file::{DeviceState, SharedState};
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
            | Message::RestoreHiddenSession(..)
            | Message::DisconnectServer
            | Message::CloseServer
            | Message::ConfirmCloseServer
            | Message::CancelCloseServer
            | Message::RenameWindow
            | Message::WindowRenameInputChanged(..)
            | Message::FinishWindowRename
            | Message::CancelWindowRename
            | Message::CancelConnect(..) => self.handle_tab_message(message),

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
            | Message::ShowWorkspaceDialog
            | Message::WorkspaceFilterChanged(..)
            | Message::SelectWorkspace(..)
            | Message::ConfirmWorkspaceSelection
            | Message::NewWorkspaceFromDialog
            | Message::CancelWorkspaceDialog
            | Message::ShowNewWorkspaceDialog
            | Message::NewWorkspaceDialogInput(..)
            | Message::ConfirmNewWorkspaceDialog
            | Message::CancelNewWorkspaceDialog
            | Message::ShowRenameWorkspaceDialog(..)
            | Message::RenameWorkspaceDialogInput(..)
            | Message::ConfirmRenameWorkspaceDialog
            | Message::CancelRenameWorkspaceDialog
            | Message::ShowDeleteWorkspaceDialog(..)
            | Message::ConfirmDeleteWorkspaceDialog
            | Message::CancelDeleteWorkspaceDialog
            | Message::SwitchWorkspace(..)
            | Message::HostKeyAcceptSave
            | Message::HostKeyConnectOnce
            | Message::HostKeyReject
            | Message::HostKeyChangedDismiss
            | Message::PasswordInputChanged(..)
            | Message::PasswordSubmit
            | Message::PasswordCancel
            | Message::LockTakeOver
            | Message::LockCancel
            | Message::ShowShortcutsDialog
            | Message::DismissShortcutsDialog => self.handle_dialog_message(message),

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
            | Message::WindowResized(..)
            | Message::WindowFocused(..)
            | Message::NewWindow
            | Message::NewWindowForWorkspace(..)
            | Message::FocusWorkspaceWindows(..)
            | Message::WindowOpened(..)
            | Message::ShowControlWindow
            | Message::CopyToClipboard(..) => self.handle_terminal_message(message),

            // --- Search messages ---
            Message::SearchToggle
            | Message::SearchInputChanged(..)
            | Message::SearchNext
            | Message::SearchPrev
            | Message::SearchClose
            | Message::ExportScrollback
            | Message::CopyScrollback => self.handle_search_message(message),

            // --- State sync messages ---
            Message::StateSyncerReady(..)
            | Message::ServerStateLoaded(..)
            | Message::ServerConnected(..) => self.handle_state_sync_message(message),

            // --- Server / workspace management messages ---
            Message::ConnectServer(..)
            | Message::DisconnectAllWorkspaces(..)
            | Message::EditServer(..)
            | Message::ForgetServer(..)
            | Message::ConfirmForgetServer
            | Message::CancelForgetServer
            | Message::ShowServerForm(..)
            | Message::BackToServerList
            | Message::SaveServer
            | Message::SaveAndConnectServer
            | Message::ServerFormNameChanged(..)
            | Message::ServerFormHostChanged(..)
            | Message::ServerFormPortChanged(..)
            | Message::ServerFormUserChanged(..)
            | Message::ServerFormIdentityChanged(..)
            | Message::ConnectWorkspace(..)
            | Message::DisconnectWorkspace(..)
            | Message::OpenWorkspace(..)
            | Message::ShowNewWorkspace(..)
            | Message::NewWorkspaceInputChanged(..)
            | Message::ConfirmNewWorkspace
            | Message::CancelNewWorkspace
            | Message::ShowRenameWorkspace(..)
            | Message::RenameWorkspaceInputChanged(..)
            | Message::ConfirmRenameWorkspace
            | Message::CancelRenameWorkspace
            | Message::ShowDeleteWorkspace(..)
            | Message::ConfirmDeleteWorkspace
            | Message::CancelDeleteWorkspace
            | Message::WorkspaceSessionsFound(..)
            | Message::RestoreWorkspaceHiddenWindows(..) => self.handle_workspace_message(message),

            Message::Noop => Task::none(),
        }
    }

    // -----------------------------------------------------------------------
    // SSH messages
    // -----------------------------------------------------------------------

    fn handle_ssh_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SshData(tab_id, data) => {
                if let Some(tab) = self.find_tab_mut(tab_id) {
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
                // Clear connecting state on disconnect
                self.connecting_server = None;
                let max_attempts = self.config.ssh.reconnect_max_attempts;
                if let Some(tab) = self.find_tab_mut(tab_id) {
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
                        if was_reconnectable && attempt < max_attempts {
                            let new_attempt = attempt + 1;
                            // P12: preserve terminal widget during reconnect so it shows behind overlay
                            let phase = Arc::new(std::sync::Mutex::new(String::new()));
                            tab.mark_reconnecting(new_attempt, 0, phase, None);
                            tracing::info!("SSH tab {tab_id} disconnected: {reason}, will retry");
                        } else {
                            tab.terminal = None;
                            tab.mark_disconnected(Some(reason.clone()));
                            tracing::info!("SSH tab {tab_id} disconnected: {reason}");
                        }
                    }
                }
                self.update_title();
                Task::none()
            }

            Message::SshConnected(tab_id, result) => self.handle_ssh_connected(tab_id, result),

            Message::ExistingSessionsFound(result) => self.handle_existing_sessions(result),

            Message::PasteToTerminal(tab_id, data) => {
                if let Some(tab) = self.find_tab_mut(tab_id)
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
                // Clear connecting state on successful SSH connection
                self.connecting_server = None;
                // The async task wrote the channel into pending_channel.
                // Move it to Connected state so the subscription picks it up.
                if let Some(tab) = self.find_tab_mut(tab_id)
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

                Task::none()
            }
            Err(e) => {
                tracing::error!("SSH tab {tab_id}: connection failed: {e}");
                if let Some(tab) = self.find_tab_mut(tab_id) {
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
                    if let Some(tab) = self.find_tab_mut(tab_id) {
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
                    if let Some(tab) = self.find_tab_mut(tab_id) {
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
            // If server state hasn't been loaded yet, stash the session list
            // and wait — reconciliation runs when ServerStateLoaded arrives.
            // This covers both cases: syncer not ready yet, or syncer ready but
            // state read still in flight.
            if !self.server_state_loaded {
                tracing::info!(
                    "server sessions found but state not loaded yet, deferring reconciliation ({} sessions)",
                    server_sessions.len()
                );
                self.pending_server_sessions = Some(server_sessions);
                return Task::none();
            }
            return self.reconcile_sessions(server_sessions);
        }
        Task::none()
    }

    fn reconcile_sessions(&mut self, server_sessions: Vec<String>) -> Task<Message> {
        {
            let shared_opt = self.cached_shared_state.clone();
            let device_opt = self.cached_device_state.clone();
            let saved_state = shared_opt;

            // FR-STATE-14: restore window geometry from device state per window
            if let Some(ref device) = device_opt {
                for win in self.windows.values_mut() {
                    if win.kind == super::WindowKind::Control {
                        continue;
                    }
                    let geo = device
                        .window_geometry
                        .get(&win.server_window_id)
                        .or_else(|| {
                            // Fallback for old state: try last_active, "main", or any
                            device
                                .last_active_window
                                .as_ref()
                                .and_then(|k| device.window_geometry.get(k))
                                .or_else(|| device.window_geometry.get("main"))
                                .or_else(|| device.window_geometry.values().next())
                        });
                    if let Some(geo) = geo {
                        win.window_x = geo.x;
                        win.window_y = geo.y;
                        win.window_width = geo.width;
                        win.window_height = geo.height;
                        tracing::info!(
                            "restored window geometry for {}: {}x{} at ({:?},{:?})",
                            win.server_window_id,
                            geo.width,
                            geo.height,
                            geo.x,
                            geo.y
                        );
                    }
                }
            }

            // Load hidden sessions from device state
            if let Some(ref device) = device_opt {
                self.hidden_sessions = device.hidden_sessions.clone();
            }

            // FR-ENV-05: restore last workspace from saved state
            if let Some(ref saved) = saved_state
                && let Some(ref workspace_name) = saved.last_workspace
            {
                self.current_workspace = workspace_name.clone();
            }

            // FR-ENV-04: populate workspace_list from saved state
            if let Some(ref saved) = saved_state {
                self.dialogs.workspace_list = saved.workspaces.keys().cloned().collect();
                self.dialogs.workspace_list.sort();
            }

            // FR-SESSION-08: reconcile by UUID — match saved tabs to server sessions
            let saved_ws_tabs = saved_state
                .as_ref()
                .map(|s| s.workspace_tabs(&self.current_workspace))
                .unwrap_or_default();

            // Prune hidden_sessions: remove UUIDs that no longer exist in server state
            if !saved_ws_tabs.is_empty() {
                let known_uuids: Vec<&str> = saved_ws_tabs
                    .iter()
                    .map(|t| t.session_uuid.as_str())
                    .collect();
                let before = self.hidden_sessions.len();
                self.hidden_sessions
                    .retain(|u| known_uuids.contains(&u.as_str()));
                let pruned = before - self.hidden_sessions.len();
                if pruned > 0 {
                    tracing::info!("pruned {pruned} stale hidden session UUIDs");
                }
            }
            if !saved_ws_tabs.is_empty() {
                for tab in self.all_tabs_mut() {
                    // Find saved tab entry by UUID
                    if let Some(saved_tab) = saved_ws_tabs
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
                self.all_tabs().map(|t| t.tmux_session.clone()).collect();

            let mut restorable: Vec<(&str, &str, Option<&str>)> = Vec::new();
            let mut stale: Vec<String> = Vec::new();

            for session in &server_sessions {
                if existing_tmux.contains(session) {
                    continue; // Already open in a tab
                }
                if let Some(saved) = saved_ws_tabs
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
                    restorable.push((
                        saved.title.as_str(),
                        session.as_str(),
                        saved.server_window_id.as_deref(),
                    ));
                } else {
                    // Check if this is a shellkeep session in OUR workspace
                    // (UUID format or legacy format) but NOT in saved state —
                    // orphan from a failed kill.
                    let ws_uuid = self.current_workspace_uuid();
                    let uuid_prefix = format!("shellkeep--{ws_uuid}--");
                    let legacy_prefix = format!("{}--shellkeep-", self.current_workspace);
                    if session.starts_with(&uuid_prefix) || session.starts_with(&legacy_prefix) {
                        stale.push(session.clone());
                    }
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

                // Remove the auto-created initial tab whose tmux session is not in
                // saved state — it was a placeholder that is now redundant.
                let saved_tmux_names: Vec<&str> = saved_ws_tabs
                    .iter()
                    .map(|t| t.tmux_session_name.as_str())
                    .collect();
                let mut placeholder_tmux: Vec<String> = Vec::new();
                for win in self.windows.values_mut() {
                    let mut i = 0;
                    while i < win.tabs.len() {
                        if !saved_tmux_names.contains(&win.tabs[i].tmux_session.as_str()) {
                            // This tab's tmux session is not in saved state — it was
                            // auto-created as a placeholder and is now redundant.
                            let removed = win.tabs.remove(i);
                            placeholder_tmux.push(removed.tmux_session.clone());
                            tracing::info!(
                                "removed placeholder tab {} ({})",
                                removed.id,
                                removed.tmux_session
                            );
                            if win.active_tab >= win.tabs.len() && win.active_tab > 0 {
                                win.active_tab -= 1;
                            }
                        } else {
                            i += 1;
                        }
                    }
                }

                // Kill tmux sessions of removed placeholder tabs (they may or may not
                // have been created on the server by the time reconciliation runs).
                if !placeholder_tmux.is_empty() {
                    let mgr = self.conn_manager.clone();
                    if let Some(ref conn) = self.current_conn {
                        let conn_key = conn.key.clone();
                        tasks.push(Task::perform(
                            async move {
                                let mgr_guard = mgr.lock().await;
                                if let Some(handle_arc) = mgr_guard.get_cached(&conn_key) {
                                    let handle = handle_arc.lock().await;
                                    for name in &placeholder_tmux {
                                        let cmd =
                                            format!("tmux kill-session -t {name} 2>/dev/null");
                                        let _ = ssh::connection::exec_command(&handle, &cmd).await;
                                        tracing::info!("killed placeholder tmux session: {name}");
                                    }
                                }
                            },
                            |_| Message::Noop,
                        ));
                    }
                }

                for (label, session_name, swid) in &restorable {
                    // Route tab to the window matching its server_window_id
                    let target = swid.and_then(|swid| {
                        self.windows
                            .iter()
                            .find(|(_, w)| w.server_window_id == swid)
                            .map(|(id, _)| *id)
                    });
                    tasks.push(self.open_tab_russh_in_window(label, session_name, target));
                }
            }

            if !tasks.is_empty() {
                return Task::batch(tasks);
            }
        }
        Task::none()
    }

    /// Try to run reconciliation if both server sessions and server state are available.
    fn try_reconcile_pending(&mut self) -> Task<Message> {
        if let Some(sessions) = self.pending_server_sessions.take() {
            tracing::info!(
                "running deferred reconciliation with {} server sessions",
                sessions.len()
            );
            return self.reconcile_sessions(sessions);
        }
        Task::none()
    }

    // -----------------------------------------------------------------------
    // Tab messages
    // -----------------------------------------------------------------------

    fn handle_tab_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SelectTab(index) => {
                tracing::debug!("select tab: {index}");
                if let Some(win) = self.active_window_mut()
                    && index < win.tabs.len()
                {
                    win.active_tab = index;
                    win.show_welcome = false;
                    win.renaming_tab = None;
                    win.tab_context_menu = None;
                    win.update_title();
                }
                Task::none()
            }

            // FR-SESSION-10a: close tab with confirmation for active sessions
            Message::CloseTab(index) => {
                tracing::info!("close tab requested: index {index}");
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = None;
                }
                let needs_confirm = self
                    .active_window()
                    .and_then(|w| w.tabs.get(index))
                    .is_some_and(|tab| !tab.is_dead() && tab.terminal.is_some());
                if needs_confirm {
                    // Active session — ask confirmation
                    self.dialogs.pending_close_tabs = Some(vec![index]);
                    return Task::none();
                }
                // Dead/disconnected — close immediately
                self.close_tab(index)
            }

            Message::ConfirmCloseTabs => {
                tracing::info!("confirm close tabs");
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
                tracing::debug!("cancel close tabs");
                self.dialogs.pending_close_tabs = None;
                Task::none()
            }

            Message::NewTab => {
                tracing::info!("new tab requested");
                // Phase 5: if active window is control, find or create a session window
                let is_control = self
                    .active_window()
                    .is_some_and(|w| w.kind == super::WindowKind::Control);
                if is_control {
                    // Find first session window and focus it
                    if let Some((&id, _)) = self
                        .windows
                        .iter()
                        .find(|(_, w)| w.kind == super::WindowKind::Session)
                    {
                        self.focused_window = Some(id);
                    } else if self.current_conn.is_some() {
                        // No session window exists — open one
                        return self.update(Message::NewWindow);
                    } else {
                        return Task::none();
                    }
                }

                let tab_count = self.active_window().map(|w| w.tabs.len()).unwrap_or(0);
                if self.current_conn.is_some() {
                    let n = tab_count + 1;
                    let label = format!("Session {n}");
                    let tmux_session = self.next_tmux_session();
                    return self.open_tab_russh(&label, &tmux_session);
                } else {
                    let ssh_args = self
                        .active_window()
                        .and_then(|w| w.tabs.last())
                        .map(|t| t.ssh_args().to_vec());
                    if let Some(args) = ssh_args {
                        let n = tab_count + 1;
                        let label = format!("Session {n}");
                        self.open_tab_with_tmux(&args, &label);
                    } else if let Some(win) = self.active_window_mut() {
                        win.show_welcome = true;
                    }
                }
                Task::none()
            }

            Message::ReconnectTab(index) => {
                tracing::info!("reconnect tab: index {index}");
                // Manual reconnect: reset state before calling reconnect_tab
                // which will set up Connecting state
                if let Some(win) = self.active_window_mut()
                    && index < win.tabs.len()
                {
                    win.tabs[index].mark_disconnected(None);
                }
                self.reconnect_tab(index)
            }

            // FR-UI-07: create a fresh session replacing a dead tab
            Message::CreateNewSession(index) => {
                tracing::info!("create new session for tab {index}");
                let can_create = self.active_window().is_some_and(|w| index < w.tabs.len())
                    && self.current_conn.is_some();
                if can_create {
                    let label = self
                        .active_window()
                        .and_then(|w| w.tabs.get(index))
                        .map(|t| t.label.clone())
                        .unwrap_or_default();
                    let tmux_session = self.next_tmux_session();
                    // Remove the dead tab
                    if let Some(win) = self.active_window_mut() {
                        win.tabs.remove(index);
                        if win.active_tab >= win.tabs.len() && win.active_tab > 0 {
                            win.active_tab -= 1;
                        }
                    }
                    // Open fresh tab
                    let task = self.open_tab_russh(&label, &tmux_session);
                    // Move the new tab to the original position
                    if let Some(win) = self.active_window_mut()
                        && win.tabs.len() > 1
                        && index < win.tabs.len()
                    {
                        // SAFETY: len() > 1 guarantees pop() returns Some
                        #[allow(clippy::unwrap_used)]
                        let new_tab = win.tabs.pop().unwrap();
                        win.tabs.insert(index, new_tab);
                        win.active_tab = index;
                        win.update_title();
                    }
                    return task;
                }
                Task::none()
            }

            Message::HideTab(index) => {
                tracing::info!("hide tab: index {index}");
                self.hide_tab(index);
                // P9: dismiss close dialog if it was open
                self.dialogs.pending_close_tabs = None;
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = None;
                }
                Task::none()
            }

            // P11: cancel an in-progress SSH connection
            Message::CancelConnect(tab_id) => {
                if let Some(tab) = self.find_tab_mut(tab_id) {
                    tab.terminal = None;
                    tab.mark_disconnected(Some("Connection cancelled".to_string()));
                }
                Task::none()
            }

            Message::CloseOtherTabs(keep_index) => {
                tracing::info!("close other tabs, keeping index {keep_index}");
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = None;
                }
                let (to_close, has_active) = {
                    let win = match self.active_window() {
                        Some(w) => w,
                        None => return Task::none(),
                    };
                    let keep_id = win.tabs.get(keep_index).map(|t| t.id);
                    let to_close: Vec<usize> = (0..win.tabs.len())
                        .filter(|&i| win.tabs.get(i).map(|t| t.id) != keep_id)
                        .collect();
                    let has_active = to_close.iter().any(|&i| {
                        win.tabs
                            .get(i)
                            .is_some_and(|t| !t.is_dead() && t.terminal.is_some())
                    });
                    (to_close, has_active)
                };
                if has_active {
                    self.dialogs.pending_close_tabs = Some(to_close);
                } else {
                    let mut tasks = Vec::new();
                    for idx in to_close.into_iter().rev() {
                        tasks.push(self.close_tab(idx));
                    }
                    if let Some(win) = self.active_window_mut() {
                        win.active_tab = 0;
                    }
                    return Task::batch(tasks);
                }
                Task::none()
            }

            Message::CloseTabsToRight(index) => {
                tracing::info!("close tabs to right of index {index}");
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = None;
                }
                let (to_close, has_active) = {
                    let win = match self.active_window() {
                        Some(w) => w,
                        None => return Task::none(),
                    };
                    let to_close: Vec<usize> = (index + 1..win.tabs.len()).collect();
                    let has_active = to_close.iter().any(|&i| {
                        win.tabs
                            .get(i)
                            .is_some_and(|t| !t.is_dead() && t.terminal.is_some())
                    });
                    (to_close, has_active)
                };
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
                tracing::info!("start rename tab {index}");
                let label = self
                    .active_window()
                    .and_then(|w| w.tabs.get(index))
                    .map(|t| t.label.clone());
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = None;
                    if index < win.tabs.len() {
                        win.active_tab = index;
                        win.renaming_tab = Some(index);
                    }
                }
                if let Some(label) = label {
                    self.rename_input = label;
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
                tracing::info!("rename tab to: {}", self.rename_input.trim());
                let rename_task = Task::none();
                let renaming_tab = self.active_window().and_then(|w| w.renaming_tab);
                if let Some(index) = renaming_tab {
                    let valid = self.active_window().is_some_and(|w| index < w.tabs.len())
                        && !self.rename_input.trim().is_empty();
                    if valid {
                        let new_label = self.rename_input.trim().to_string();
                        if let Some(win) = self.active_window_mut() {
                            win.tabs[index].label = new_label;
                            win.update_title();
                        }
                        self.save_state();
                        // FR-SESSION-06: tab rename only updates the label.
                        // Tmux session name is UUID-based and does not change.
                    }
                }
                if let Some(win) = self.active_window_mut() {
                    win.renaming_tab = None;
                }
                rename_task
            }

            Message::TabMoveLeft(index) => {
                tracing::debug!("move tab left: {index}");
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = None;
                    if index > 0 && index < win.tabs.len() {
                        win.tabs.swap(index, index - 1);
                        if win.active_tab == index {
                            win.active_tab -= 1;
                        } else if win.active_tab == index - 1 {
                            win.active_tab += 1;
                        }
                    }
                }
                Task::none()
            }

            Message::TabMoveRight(index) => {
                tracing::debug!("move tab right: {index}");
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = None;
                    if index + 1 < win.tabs.len() {
                        win.tabs.swap(index, index + 1);
                        if win.active_tab == index {
                            win.active_tab += 1;
                        } else if win.active_tab == index + 1 {
                            win.active_tab -= 1;
                        }
                    }
                }
                Task::none()
            }

            Message::TabContextMenu(index, x, y) => {
                if let Some(win) = self.active_window_mut() {
                    win.tab_context_menu = Some((index, x, y));
                    win.context_menu = None;
                    win.show_restore_dropdown = false;
                }
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
                tracing::debug!("show restore dropdown");
                if let Some(win) = self.active_window_mut() {
                    win.show_restore_dropdown = !win.show_restore_dropdown;
                }
                Task::none()
            }

            Message::DismissRestoreDropdown => {
                tracing::debug!("dismiss restore dropdown");
                if let Some(win) = self.active_window_mut() {
                    win.show_restore_dropdown = false;
                }
                Task::none()
            }

            Message::RestoreHiddenSession(session_uuid) => {
                if let Some(win) = self.active_window_mut() {
                    win.show_restore_dropdown = false;
                }

                // Find the hidden session in saved state to get its tmux name and title
                let saved_state = self.cached_shared_state.clone();
                let saved_ws_tabs = saved_state
                    .as_ref()
                    .map(|s| s.workspace_tabs(&self.current_workspace))
                    .unwrap_or_default();

                if let Some(saved_tab) = saved_ws_tabs
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

            // Item 2: disconnect server (keep tmux sessions alive)
            Message::DisconnectServer => {
                tracing::info!("disconnect server: hiding all sessions, closing windows");
                // Hide all tabs and close all session windows
                let mut session_win_ids: Vec<window::Id> = Vec::new();
                for (&win_id, win) in &self.windows {
                    if win.kind == super::WindowKind::Session {
                        // Move all session UUIDs to hidden
                        for tab in &win.tabs {
                            if !tab.session_uuid.is_empty()
                                && !self.hidden_sessions.contains(&tab.session_uuid)
                            {
                                self.hidden_sessions.push(tab.session_uuid.clone());
                            }
                        }
                        session_win_ids.push(win_id);
                    }
                }
                // Remove and close all session windows
                let mut close_tasks: Vec<Task<Message>> = Vec::new();
                for win_id in &session_win_ids {
                    self.windows.remove(win_id);
                    self.window_order.retain(|id| id != win_id);
                    close_tasks.push(window::close(*win_id));
                }
                if self
                    .focused_window
                    .is_some_and(|id| session_win_ids.contains(&id))
                {
                    self.focused_window = self.window_order.first().copied();
                }
                self.current_conn = None;
                self.sessions_listed = false;
                self.server_state_loaded = false;
                self.state_syncer = None;
                self.connecting_server = None;
                self.cached_shared_state = None;
                self.cached_device_state = None;
                self.save_state();
                self.toast = Some((
                    "Disconnected. Sessions kept on server.".into(),
                    std::time::Instant::now(),
                ));
                if close_tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(close_tasks)
                }
            }

            // Item 2: close all sessions (destructive) — show confirmation
            Message::CloseServer => {
                tracing::info!("close server: terminate all sessions");
                self.confirm_close_server = true;
                Task::none()
            }

            Message::ConfirmCloseServer => {
                tracing::info!("confirm close server: terminating all tmux sessions");
                self.confirm_close_server = false;
                // Close all tabs in all session windows (kills tmux sessions)
                let mut tasks = Vec::new();
                let session_win_ids: Vec<window::Id> = self
                    .windows
                    .iter()
                    .filter(|(_, w)| w.kind == super::WindowKind::Session)
                    .map(|(&id, _)| id)
                    .collect();
                for win_id in session_win_ids {
                    if let Some(win) = self.windows.get(&win_id) {
                        let tab_count = win.tabs.len();
                        for i in (0..tab_count).rev() {
                            tasks.push(self.close_tab_in_window(win_id, i));
                        }
                    }
                }
                self.current_conn = None;
                self.sessions_listed = false;
                self.server_state_loaded = false;
                self.state_syncer = None;
                self.connecting_server = None;
                self.cached_shared_state = None;
                self.cached_device_state = None;
                self.save_state();
                Task::batch(tasks)
            }

            Message::CancelCloseServer => {
                tracing::debug!("cancel close server");
                self.confirm_close_server = false;
                Task::none()
            }

            // Item 5: rename window
            Message::RenameWindow => {
                tracing::info!("rename window started");
                // Dismiss the dropdown first
                if let Some(win) = self.active_window_mut() {
                    win.show_restore_dropdown = false;
                }
                if let Some(win) = self.active_window() {
                    let current_name = win.name.clone();
                    let win_id = win.id;
                    self.renaming_window = Some(win_id);
                    self.window_rename_input = current_name;
                }
                Task::none()
            }

            Message::WindowRenameInputChanged(v) => {
                self.window_rename_input = v;
                Task::none()
            }

            Message::FinishWindowRename => {
                tracing::info!("window renamed to: {}", self.window_rename_input);
                if let Some(win_id) = self.renaming_window.take() {
                    let new_name = self.window_rename_input.trim().to_string();
                    if let Some(win) = self.windows.get_mut(&win_id) {
                        if !new_name.is_empty() {
                            win.name = new_name;
                        }
                        win.update_title();
                    }
                    self.window_rename_input.clear();
                    self.save_state();
                }
                Task::none()
            }

            Message::CancelWindowRename => {
                tracing::debug!("window rename cancelled");
                self.renaming_window = None;
                self.window_rename_input.clear();
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
                tracing::info!("connect: initiating SSH to {}", self.welcome.host_input);
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
                let _label = ssh_args
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
                    label: _label.clone(),
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

                self.launch_server_connect(false, None)
            }

            _ => Task::none(),
        }
    }

    fn handle_key_event(&mut self, event: keyboard::Event) -> Task<Message> {
        if let keyboard::Event::KeyPressed { key, modifiers, .. } = event {
            // Item 1: determine if the focused window is a session window
            let focused_is_session = self
                .active_window()
                .is_some_and(|w| w.kind == super::WindowKind::Session);
            let win_tab_count = self.active_window().map(|w| w.tabs.len()).unwrap_or(0);

            // Ctrl+Shift+T — new tab (same server) — session windows only
            if focused_is_session
                && modifiers.control()
                && modifiers.shift()
                && key == keyboard::Key::Character("t".into())
            {
                return self.update(Message::NewTab);
            }
            // Ctrl+Shift+N — new window for the current server — session windows only
            if focused_is_session
                && modifiers.control()
                && modifiers.shift()
                && key == keyboard::Key::Character("n".into())
            {
                return self.update(Message::NewWindow);
            }
            // Ctrl+Shift+W — close current tab — session windows only
            if focused_is_session
                && modifiers.control()
                && modifiers.shift()
                && key == keyboard::Key::Character("w".into())
                && win_tab_count > 0
            {
                let active = self.active_window().map(|w| w.active_tab).unwrap_or(0);
                return self.update(Message::CloseTab(active));
            }
            // Ctrl+Tab — next tab — session windows only
            if focused_is_session
                && modifiers.control()
                && !modifiers.shift()
                && key == keyboard::Key::Named(keyboard::key::Named::Tab)
                && win_tab_count > 0
                && let Some(win) = self.active_window_mut()
            {
                win.active_tab = (win.active_tab + 1) % win.tabs.len();
                win.show_welcome = false;
                win.update_title();
            }
            // Ctrl+Shift+Tab — previous tab — session windows only
            if focused_is_session
                && modifiers.control()
                && modifiers.shift()
                && key == keyboard::Key::Named(keyboard::key::Named::Tab)
                && win_tab_count > 0
                && let Some(win) = self.active_window_mut()
            {
                if win.active_tab == 0 {
                    win.active_tab = win.tabs.len() - 1;
                } else {
                    win.active_tab -= 1;
                }
                win.show_welcome = false;
                win.update_title();
            }
            // F2 — rename current tab — session windows only
            if focused_is_session
                && key == keyboard::Key::Named(keyboard::key::Named::F2)
                && win_tab_count > 0
            {
                let renaming = self.active_window().and_then(|w| w.renaming_tab);
                if renaming.is_none() {
                    let label = self
                        .active_window()
                        .and_then(|w| w.tabs.get(w.active_tab))
                        .map(|t| t.label.clone());
                    if let Some(win) = self.active_window_mut() {
                        win.renaming_tab = Some(win.active_tab);
                    }
                    if let Some(label) = label {
                        self.rename_input = label;
                    }
                    return iced_runtime::widget::operation::focus(RENAME_INPUT_ID);
                }
            }
            // Ctrl+Shift+= or Ctrl+= — zoom in — session windows only
            if focused_is_session
                && modifiers.control()
                && (key == keyboard::Key::Character("=".into())
                    || key == keyboard::Key::Character("+".into()))
            {
                self.current_font_size = (self.current_font_size + 1.0).min(36.0);
                self.apply_font_to_all_tabs();
            }
            // Ctrl+- — zoom out — session windows only
            if focused_is_session
                && modifiers.control()
                && key == keyboard::Key::Character("-".into())
            {
                self.current_font_size = (self.current_font_size - 1.0).max(8.0);
                self.apply_font_to_all_tabs();
            }
            // Ctrl+0 — zoom reset — session windows only
            if focused_is_session
                && modifiers.control()
                && key == keyboard::Key::Character("0".into())
            {
                self.current_font_size = self.config.terminal.font_size;
                self.apply_font_to_all_tabs();
            }
            // Ctrl+Shift+F — toggle scrollback search — session windows only
            if focused_is_session
                && modifiers.control()
                && modifiers.shift()
                && key == keyboard::Key::Character("f".into())
            {
                return self.update(Message::SearchToggle);
            }
            // Ctrl+Shift+S — export scrollback — session windows only
            if focused_is_session
                && modifiers.control()
                && modifiers.shift()
                && key == keyboard::Key::Character("s".into())
                && win_tab_count > 0
            {
                return self.update(Message::ExportScrollback);
            }
            // Ctrl+Shift+A — copy scrollback — session windows only
            if focused_is_session
                && modifiers.control()
                && modifiers.shift()
                && key == keyboard::Key::Character("a".into())
                && win_tab_count > 0
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
            // Escape — dismiss search, context menu, cancel rename, shortcuts dialog, or cancel welcome
            if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                if self.dialogs.show_shortcuts_dialog {
                    return self.update(Message::DismissShortcutsDialog);
                } else if self.search.active {
                    return self.update(Message::SearchClose);
                } else if let Some(win) = self.active_window_mut() {
                    if win.context_menu.is_some() {
                        win.context_menu = None;
                    } else if win.renaming_tab.is_some() {
                        win.renaming_tab = None;
                    } else if win.show_welcome && !win.tabs.is_empty() {
                        win.show_welcome = false;
                    }
                }
            }
            // Tab / Shift+Tab — cycle focus between form inputs on dialogs/welcome
            let show_welcome = self.active_window().is_some_and(|w| w.show_welcome);
            let is_renaming = self
                .active_window()
                .is_some_and(|w| w.renaming_tab.is_some());
            if key == keyboard::Key::Named(keyboard::key::Named::Tab)
                && (show_welcome
                    || self.dialogs.show_workspace_dialog
                    || self.dialogs.show_new_workspace_dialog
                    || self.dialogs.show_rename_workspace_dialog
                    || self.search.active
                    || is_renaming)
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
                tracing::info!("window close requested: {win_id:?}");
                // Phase 5: closing the control window just hides it (stays in tray)
                let is_control = self
                    .windows
                    .get(&win_id)
                    .is_some_and(|w| w.kind == super::WindowKind::Control);
                if is_control {
                    // Control window closed — remove from tracking
                    self.windows.remove(&win_id);
                    self.window_order.retain(|&id| id != win_id);
                    // If no session windows remain, exit the app
                    let has_session_windows = self
                        .windows
                        .values()
                        .any(|w| w.kind == super::WindowKind::Session);
                    if !has_session_windows && self.hidden_windows.is_empty() {
                        return iced::exit();
                    }
                    return Task::none();
                }

                // Session window: snapshot as hidden window, then close
                if let Some(win) = self.windows.remove(&win_id) {
                    let hidden_tabs: Vec<_> = win
                        .tabs
                        .iter()
                        .filter(|t| !t.is_dead() && !t.session_uuid.is_empty())
                        .map(|t| super::HiddenTab {
                            session_uuid: t.session_uuid.clone(),
                            tmux_session_name: t.tmux_session.clone(),
                            label: t.label.clone(),
                        })
                        .collect();
                    for ht in &hidden_tabs {
                        if !self.hidden_sessions.contains(&ht.session_uuid) {
                            self.hidden_sessions.push(ht.session_uuid.clone());
                        }
                    }
                    if !hidden_tabs.is_empty() {
                        self.hidden_windows.push(super::HiddenWindow {
                            server_window_id: win.server_window_id,
                            name: win.name,
                            server_uuid: win.server_uuid,
                            workspace_env: win.workspace_env,
                            tabs: hidden_tabs,
                        });
                    }
                }
                self.window_order.retain(|&id| id != win_id);
                if self.focused_window == Some(win_id) {
                    self.focused_window = self.window_order.first().copied();
                }
                self.state_dirty = true;
                self.flush_state();
                // Show control window so user sees updated visible/hidden counts
                // (window is already closed by the OS at this point via Event::Closed)
                if self.windows.contains_key(&self.control_window_id) {
                    window::gain_focus(self.control_window_id)
                } else {
                    Task::none()
                }
            }

            Message::CloseDialogClose => {
                self.dialogs.show_close_dialog = false;
                self.flush_state();
                if let Some(id) = self.dialogs.close_window_id.take() {
                    self.windows.remove(&id);
                    self.window_order.retain(|&wid| wid != id);
                    if self.focused_window == Some(id) {
                        self.focused_window = self.window_order.first().copied();
                    }
                    // Phase 5: check if only the control window remains
                    let has_session_windows = self
                        .windows
                        .values()
                        .any(|w| w.kind == super::WindowKind::Session);
                    if !has_session_windows {
                        return Task::batch([window::close(id), iced::exit()]);
                    }
                    return window::close(id);
                }
                iced::exit()
            }

            Message::CloseDialogCancel => {
                self.dialogs.show_close_dialog = false;
                self.dialogs.close_window_id = None;
                Task::none()
            }

            // FR-ENV-03: workspace selection dialog
            Message::ShowWorkspaceDialog => {
                tracing::debug!("show workspace dialog");
                // FR-ENV-04: if only one workspace, select it directly
                if self.dialogs.workspace_list.len() == 1 {
                    let workspace_name = self.dialogs.workspace_list[0].clone();
                    if workspace_name != self.current_workspace {
                        return self.update(Message::SwitchWorkspace(workspace_name));
                    }
                    return Task::none();
                }
                self.dialogs.show_workspace_dialog = true;
                self.dialogs.workspace_filter.clear();
                // Pre-select current workspace
                self.dialogs.selected_workspace = Some(self.current_workspace.clone());
                Task::none()
            }

            Message::WorkspaceFilterChanged(filter) => {
                self.dialogs.workspace_filter = filter;

                Task::none()
            }

            Message::SelectWorkspace(name) => {
                self.dialogs.selected_workspace = Some(name);
                Task::none()
            }

            Message::ConfirmWorkspaceSelection => {
                tracing::debug!("confirm workspace selection");
                if let Some(ref workspace_name) = self.dialogs.selected_workspace {
                    let workspace_name = workspace_name.clone();
                    self.dialogs.show_workspace_dialog = false;
                    if workspace_name != self.current_workspace {
                        return self.update(Message::SwitchWorkspace(workspace_name));
                    }
                }
                Task::none()
            }

            Message::NewWorkspaceFromDialog => {
                // Close workspace selection, open new workspace creation
                self.dialogs.show_workspace_dialog = false;
                self.dialogs.new_workspace_dialog_input.clear();
                self.dialogs.show_new_workspace_dialog = true;
                Task::none()
            }

            Message::CancelWorkspaceDialog => {
                tracing::debug!("cancel workspace dialog");
                self.dialogs.show_workspace_dialog = false;
                Task::none()
            }

            // FR-ENV-07: create new workspace
            Message::ShowNewWorkspaceDialog => {
                self.dialogs.new_workspace_dialog_input.clear();
                self.dialogs.show_new_workspace_dialog = true;
                Task::none()
            }

            Message::NewWorkspaceDialogInput(input) => {
                self.dialogs.new_workspace_dialog_input = input;
                Task::none()
            }

            Message::ConfirmNewWorkspaceDialog => {
                let name = self.dialogs.new_workspace_dialog_input.trim().to_string();
                if !name.is_empty() && !self.dialogs.workspace_list.contains(&name) {
                    self.dialogs.workspace_list.push(name.clone());
                    self.dialogs.workspace_list.sort();
                    self.current_workspace = name;
                    self.toast = Some((
                        format!("Workspace \"{}\" created", self.current_workspace),
                        std::time::Instant::now(),
                    ));
                    self.state_dirty = true;
                    self.flush_state();
                }
                self.dialogs.show_new_workspace_dialog = false;
                self.dialogs.new_workspace_dialog_input.clear();
                Task::none()
            }

            Message::CancelNewWorkspaceDialog => {
                self.dialogs.show_new_workspace_dialog = false;
                self.dialogs.new_workspace_dialog_input.clear();
                Task::none()
            }

            // FR-ENV-08: rename workspace
            Message::ShowRenameWorkspaceDialog(name) => {
                self.dialogs.rename_workspace_target = Some(name.clone());
                self.dialogs.rename_workspace_dialog_input = name;
                self.dialogs.show_rename_workspace_dialog = true;
                Task::none()
            }

            Message::RenameWorkspaceDialogInput(input) => {
                self.dialogs.rename_workspace_dialog_input = input;
                Task::none()
            }

            Message::ConfirmRenameWorkspaceDialog => {
                let new_name = self
                    .dialogs
                    .rename_workspace_dialog_input
                    .trim()
                    .to_string();
                if let Some(ref old_name) = self.dialogs.rename_workspace_target
                    && !new_name.is_empty()
                    && new_name != *old_name
                {
                    let old_name = old_name.clone();
                    // Rename in the display list
                    if let Some(entry) = self
                        .dialogs
                        .workspace_list
                        .iter_mut()
                        .find(|e| **e == old_name)
                    {
                        *entry = new_name.clone();
                    }
                    self.dialogs.workspace_list.sort();
                    // Rename in cached shared state (the authoritative source)
                    if let Some(ref mut state) = self.cached_shared_state
                        && let Some(mut ws) = state.workspaces.remove(&old_name)
                    {
                        ws.name = new_name.clone();
                        state.workspaces.insert(new_name.clone(), ws);
                    }
                    if self.current_workspace == old_name {
                        self.current_workspace = new_name.clone();
                    }
                    self.toast = Some((
                        format!("Workspace renamed to \"{new_name}\""),
                        std::time::Instant::now(),
                    ));
                    self.state_dirty = true;
                    self.flush_state();
                }
                self.dialogs.show_rename_workspace_dialog = false;
                self.dialogs.rename_workspace_dialog_input.clear();
                self.dialogs.rename_workspace_target = None;
                Task::none()
            }

            Message::CancelRenameWorkspaceDialog => {
                self.dialogs.show_rename_workspace_dialog = false;
                self.dialogs.rename_workspace_dialog_input.clear();
                self.dialogs.rename_workspace_target = None;
                Task::none()
            }

            // FR-ENV-09: delete workspace
            Message::ShowDeleteWorkspaceDialog(name) => {
                self.dialogs.delete_workspace_target = Some(name);
                self.dialogs.show_delete_workspace_dialog = true;
                Task::none()
            }

            Message::ConfirmDeleteWorkspaceDialog => {
                if let Some(ref name) = self.dialogs.delete_workspace_target {
                    let name = name.clone();
                    self.dialogs.workspace_list.retain(|e| *e != name);
                    if self.current_workspace == name {
                        self.current_workspace = self
                            .dialogs
                            .workspace_list
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "default".to_string());
                    }
                    self.toast = Some((
                        format!("Workspace \"{name}\" deleted"),
                        std::time::Instant::now(),
                    ));
                    self.state_dirty = true;
                    self.flush_state();
                }
                self.dialogs.show_delete_workspace_dialog = false;
                self.dialogs.delete_workspace_target = None;
                Task::none()
            }

            Message::CancelDeleteWorkspaceDialog => {
                self.dialogs.show_delete_workspace_dialog = false;
                self.dialogs.delete_workspace_target = None;
                Task::none()
            }

            // FR-ENV-10: switch active workspace
            Message::SwitchWorkspace(name) => {
                if name != self.current_workspace {
                    tracing::info!(
                        "switching workspace: {} -> {}",
                        self.current_workspace,
                        name
                    );
                    // Save current tabs for the current workspace
                    self.flush_state();
                    // Switch to the new workspace
                    self.current_workspace = name;
                    // TODO: load tabs for the new workspace from state
                    self.state_dirty = true;
                    self.update_title();
                    self.toast = Some((
                        format!("Switched to \"{}\" workspace", self.current_workspace),
                        std::time::Instant::now(),
                    ));
                }
                Task::none()
            }

            // FR-CONN-03: host key TOFU — accept and save to known_hosts
            Message::HostKeyAcceptSave => {
                tracing::info!("host key accepted and saved");
                self.dialogs.pending_host_key_prompt = None;
                Task::none()
            }
            Message::HostKeyConnectOnce => {
                tracing::info!("host key accepted for this session only");
                if let Some(ref prompt) = self.dialogs.pending_host_key_prompt {
                    let _ = ssh::known_hosts::remove_host_key(&prompt.host, prompt.port);
                }
                self.dialogs.pending_host_key_prompt = None;
                Task::none()
            }
            Message::HostKeyReject => {
                tracing::info!("host key rejected");
                if let Some(ref prompt) = self.dialogs.pending_host_key_prompt {
                    let _ = ssh::known_hosts::remove_host_key(&prompt.host, prompt.port);
                }
                self.dialogs.pending_host_key_prompt = None;
                for tab in self.all_tabs_mut() {
                    tab.mark_disconnected(Some("Host key rejected by user".to_string()));
                }
                self.error = Some("Connection cancelled: host key rejected.".to_string());
                Task::none()
            }
            Message::HostKeyChangedDismiss => {
                tracing::info!("host key changed warning dismissed");
                self.dialogs.pending_host_key_prompt = None;
                Task::none()
            }

            // FR-CONN-09: password auth dialog
            Message::PasswordInputChanged(val) => {
                self.dialogs.password_input = val;
                Task::none()
            }
            Message::PasswordSubmit => {
                tracing::info!("password submitted");
                self.handle_password_submit()
            }
            Message::PasswordCancel => {
                tracing::info!("password prompt cancelled");
                self.dialogs.show_password_dialog = false;
                self.dialogs.password_input.clear();
                if let Some(tab_id) = self.dialogs.password_target_tab.take()
                    && let Some(tab) = self.find_tab_mut(tab_id)
                {
                    tab.mark_disconnected(Some("Authentication cancelled".to_string()));
                }
                self.error = Some("Authentication cancelled.".to_string());
                Task::none()
            }

            // FR-LOCK-05: lock conflict — take over
            Message::LockTakeOver => {
                tracing::info!("lock takeover requested");
                self.handle_lock_takeover()
            }
            Message::LockCancel => {
                tracing::info!("lock takeover cancelled");
                self.dialogs.show_lock_dialog = false;
                if let Some(tab_id) = self.dialogs.lock_target_tab.take()
                    && let Some(tab) = self.find_tab_mut(tab_id)
                {
                    tab.mark_disconnected(Some("Lock takeover cancelled".to_string()));
                }
                Task::none()
            }

            // P18-20: keyboard shortcuts dialog
            Message::ShowShortcutsDialog => {
                tracing::debug!("show shortcuts dialog");
                self.dialogs.show_shortcuts_dialog = true;
                Task::none()
            }
            Message::DismissShortcutsDialog => {
                tracing::debug!("dismiss shortcuts dialog");
                self.dialogs.show_shortcuts_dialog = false;
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

            if let Some(tab) = self.find_tab_mut(tab_id) {
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
        tracing::info!("handle_lock_takeover: force acquiring lock");
        self.dialogs.show_lock_dialog = false;
        self.sessions_listed = false;

        // If the lock conflict came from the control-plane flow (no target tab),
        // re-trigger the full server connect with force_lock.
        if self.dialogs.lock_target_tab.is_none() {
            self.connecting_server = self.current_conn.as_ref().and_then(|c| {
                self.saved_servers
                    .servers
                    .iter()
                    .find(|s| s.host == c.key.host)
                    .map(|s| s.uuid.clone())
            });
            return self.launch_server_connect(true, None);
        }

        // Legacy path: lock conflict from a specific tab (reconnection)
        if let Some(tab_id) = self.dialogs.lock_target_tab.take()
            && let Some(conn) = self.current_conn.clone()
            && let Some(tab) = self.find_tab_mut(tab_id)
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

    /// Launch the control-plane server connection task.
    /// Connects SSH, acquires lock, lists sessions, loads state.
    /// Result arrives as `Message::ServerConnected`.
    fn launch_server_connect(&self, force_lock: bool, password: Option<String>) -> Task<Message> {
        let Some(conn) = self.current_conn.clone() else {
            return Task::none();
        };
        let mgr = self.conn_manager.clone();
        let conn_key = conn.key.clone();
        let client_id = self.client_id.clone();
        let workspace = self.current_workspace.clone();
        let keepalive = self.config.ssh.keepalive_interval;
        let phase = Arc::new(std::sync::Mutex::new(i18n::t(i18n::CONNECTING).to_string()));

        Task::perform(
            async move {
                // 1. Connect to server (SSH + tmux check + lock)
                super::session::connect_server(super::session::ConnectServerParams {
                    conn_manager: mgr.clone(),
                    conn: conn.clone(),
                    keepalive_secs: keepalive,
                    client_id: client_id.clone(),
                    workspace,
                    force_lock,
                    phase,
                    password,
                })
                .await
                .map_err(|e| e.to_string())?;

                // 2. List existing tmux sessions
                let sessions = {
                    let m = mgr.lock().await;
                    if let Some(handle_arc) = m.get_cached(&conn_key) {
                        let handle = handle_arc.lock().await;
                        ssh::tmux::list_sessions_russh(&handle).await
                    } else {
                        Vec::new()
                    }
                };

                // 3. Create StateSyncer + load state
                let conn_result = {
                    let mut m = mgr.lock().await;
                    m.get_or_connect(&conn_key, conn.identity_file.as_deref(), None, 15)
                        .await
                        .map_err(|e| e.to_string())?
                };
                let syncer = ssh::sftp::StateSyncer::new(conn_result.handle, &client_id)
                    .await
                    .map_err(|e| e.to_string())?;
                let syncer = Arc::new(syncer);

                // 4. Read server state
                let shared_state = syncer
                    .read_shared_state()
                    .await
                    .map_err(|e| e.to_string())?;
                let device_state = syncer
                    .read_device_state()
                    .await
                    .map_err(|e| e.to_string())?;

                Ok(Box::new(super::session::ServerConnectResult {
                    sessions,
                    syncer,
                    shared_state,
                    device_state,
                }))
            },
            Message::ServerConnected,
        )
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
                        for tab in self.all_tabs_mut() {
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
                tracing::debug!("flush state triggered");
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
                let workspace = self.current_workspace.clone();
                Task::perform(
                    async move {
                        let mgr = mgr.lock().await;
                        if let Some(handle_arc) = mgr.get_cached(&conn_key) {
                            let handle = handle_arc.lock().await;
                            ssh::lock::heartbeat(&handle, &workspace)
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
                    .all_tabs()
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
                    for tab in self.all_tabs_mut() {
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
                            tracing::debug!("tray: show control window requested");
                            return self.update(Message::ShowControlWindow);
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
            .all_tabs()
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

        // Find the first window and tab index that needs reconnection
        let reconnect_target: Option<(window::Id, usize)> = self
            .windows
            .iter()
            .flat_map(|(win_id, win)| {
                win.tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.is_auto_reconnect())
                    .map(move |(i, _)| (*win_id, i))
            })
            .next();

        // Legacy compatibility: also build flat indices for the active window
        let win_id = match self.active_window_id() {
            Some(id) => id,
            None => return Task::none(),
        };
        let reconnect_indices: Vec<usize> = self
            .windows
            .get(&win_id)
            .map(|w| {
                w.tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.is_auto_reconnect())
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        // If we found a target in any window, use that
        if let Some((target_win_id, target_idx)) = reconnect_target
            && (!reconnect_indices.contains(&target_idx) || target_win_id != win_id)
        {
            // Target is in a different window — reconnect there
            let win = match self.windows.get_mut(&target_win_id) {
                Some(w) => w,
                None => return Task::none(),
            };
            let tab = &mut win.tabs[target_idx];
            let attempt = tab.reconnect_attempts();
            let next_delay =
                reconnect_backoff_delay(self.config.ssh.reconnect_backoff_base, attempt);
            tab.set_reconnect_delay_ms(next_delay);
            tracing::info!(
                "auto-reconnecting tab {} (attempt {}, next delay {}ms)",
                tab.id,
                tab.reconnect_attempts(),
                next_delay,
            );
            return self.reconnect_tab_in_window(target_win_id, target_idx);
        }

        // For simplicity, reconnect within the active window using flat indices
        let reconnect_indices: Vec<usize> = self
            .active_window()
            .map(|w| {
                w.tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.is_auto_reconnect())
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();

        if let Some(&index) = reconnect_indices.first() {
            let backoff_base = self.config.ssh.reconnect_backoff_base;
            if let Some(win) = self.active_window_mut()
                && index < win.tabs.len()
            {
                let attempt = win.tabs[index].reconnect_attempts();
                let next_delay = reconnect_backoff_delay(backoff_base, attempt);
                win.tabs[index].set_reconnect_delay_ms(next_delay);
                tracing::info!(
                    "auto-reconnecting tab {} (attempt {}, next delay {}ms)",
                    win.tabs[index].id,
                    win.tabs[index].reconnect_attempts(),
                    next_delay,
                );
            }
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
            for tab in self.all_tabs_mut() {
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
                if let Some(win) = self.active_window_mut() {
                    win.context_menu = Some((x, y));
                    win.renaming_tab = None;
                    win.tab_context_menu = None;
                }
                Task::none()
            }

            // FR-TABS-11: context menu copy — copy selected text to clipboard
            Message::ContextMenuCopy => {
                if let Some(win) = self.active_window_mut() {
                    win.context_menu = None;
                }
                if let Some(win) = self.active_window()
                    && let Some(tab) = win.tabs.get(win.active_tab)
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
                if let Some(win) = self.active_window_mut() {
                    win.context_menu = None;
                }
                let tab_id = self
                    .active_window()
                    .and_then(|w| w.tabs.get(w.active_tab))
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
                if let Some(win) = self.active_window_mut() {
                    win.context_menu = None;
                    win.tab_context_menu = None;
                    win.renaming_tab = None;
                    win.show_restore_dropdown = false;
                }
                Task::none()
            }

            Message::ToastDismiss => {
                self.toast = None;
                Task::none()
            }

            Message::TerminalEvent(iced_term::Event::BackendCall(id, cmd)) => {
                self.handle_terminal_backend_call(super::tab::TabId(id), cmd)
            }

            // FR-STATE-14: track window geometry changes (per-window)
            Message::WindowMoved(win_id, pos) => {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    tracing::info!(
                        "window {win_id:?} moved to ({}, {})",
                        pos.x as i32,
                        pos.y as i32
                    );
                    win.window_x = Some(pos.x as i32);
                    win.window_y = Some(pos.y as i32);
                }
                self.focused_window = Some(win_id);
                self.save_geometry(win_id);
                Task::none()
            }

            Message::WindowResized(win_id, size) => {
                if let Some(win) = self.windows.get_mut(&win_id) {
                    tracing::info!(
                        "window {win_id:?} resized to {}x{}",
                        size.width as u32,
                        size.height as u32
                    );
                    win.window_width = size.width as u32;
                    win.window_height = size.height as u32;
                }
                self.save_geometry(win_id);
                Task::none()
            }

            // Bug 7 fix: track OS-level window focus changes so that
            // NewTab always targets the window the user is interacting with.
            Message::WindowFocused(win_id) => {
                tracing::debug!("window focused: {win_id:?}");
                if self.windows.contains_key(&win_id) {
                    self.focused_window = Some(win_id);
                }
                Task::none()
            }

            // Phase 4: open a new window
            // Bug 7 fix: if connected to a server, auto-create a session tab
            // in the new window so it's immediately useful.
            Message::NewWindow => {
                tracing::info!("new window requested");
                let (new_id, open_task) = window::open(window::Settings {
                    size: iced::Size::new(900.0, 600.0),
                    ..window::Settings::default()
                });
                let mut new_win = super::AppWindow::new(new_id);
                new_win.server_window_id = uuid::Uuid::new_v4().to_string();
                new_win.server_uuid = self.current_conn.as_ref().and_then(|c| {
                    self.saved_servers
                        .servers
                        .iter()
                        .find(|s| s.host == c.key.host)
                        .map(|s| s.uuid.clone())
                });
                // FR-TABS-07: new window belongs to the focused window's workspace
                let ws = self
                    .active_window()
                    .and_then(|w| w.workspace_env.clone())
                    .unwrap_or_else(|| self.current_workspace.clone());
                new_win.workspace_env = Some(ws);
                // Default window name
                self.window_counter += 1;
                if let Some(ref conn) = self.current_conn {
                    new_win.name = format!(
                        "{}@{} - Window {}",
                        conn.key.username, conn.key.host, self.window_counter
                    );
                } else {
                    new_win.name = format!("shellkeep - Window {}", self.window_counter);
                }
                self.windows.insert(new_id, new_win);
                self.window_order.push(new_id);
                self.focused_window = Some(new_id);

                if self.current_conn.is_some() {
                    // Auto-create a session tab in the new window
                    let tmux_session = self.next_tmux_session();
                    let label = format!("Session {}", self.all_tabs().count() + 1);
                    let tab_task = self.open_tab_russh(&label, &tmux_session);
                    return Task::batch([open_task.map(Message::WindowOpened), tab_task]);
                }

                // No active connection — show welcome screen
                if let Some(win) = self.windows.get_mut(&new_id) {
                    win.show_welcome = true;
                }
                open_task.map(|_| Message::Noop)
            }

            // Open a new window for a specific workspace
            Message::NewWindowForWorkspace(server_uuid, env) => {
                tracing::info!("new window for workspace: {server_uuid} / {env}");
                let prev_env = self.current_workspace.clone();
                self.current_workspace = env.clone();

                let (new_id, open_task) = window::open(window::Settings {
                    size: iced::Size::new(900.0, 600.0),
                    ..window::Settings::default()
                });
                let mut new_win = super::AppWindow::new(new_id);
                new_win.server_window_id = uuid::Uuid::new_v4().to_string();
                new_win.server_uuid = Some(server_uuid);
                new_win.workspace_env = Some(env);
                self.window_counter += 1;
                if let Some(ref conn) = self.current_conn {
                    new_win.name = format!(
                        "{}@{} - Window {}",
                        conn.key.username, conn.key.host, self.window_counter
                    );
                } else {
                    new_win.name = format!("shellkeep - Window {}", self.window_counter);
                }
                self.windows.insert(new_id, new_win);
                self.window_order.push(new_id);
                self.focused_window = Some(new_id);

                let result = if self.current_conn.is_some() {
                    let tmux_session = self.next_tmux_session();
                    let label = format!("Session {}", self.all_tabs().count() + 1);
                    let tab_task = self.open_tab_russh(&label, &tmux_session);
                    Task::batch([open_task.map(Message::WindowOpened), tab_task])
                } else {
                    if let Some(win) = self.windows.get_mut(&new_id) {
                        win.show_welcome = true;
                    }
                    open_task.map(|_| Message::Noop)
                };

                self.current_workspace = prev_env;
                result
            }

            // Focus all windows belonging to a workspace
            Message::FocusWorkspaceWindows(server_uuid, env) => {
                let win_ids: Vec<_> = self
                    .windows
                    .iter()
                    .filter(|(_, w)| {
                        w.kind == super::WindowKind::Session
                            && w.server_uuid.as_deref() == Some(server_uuid.as_str())
                            && w.workspace_env.as_deref() == Some(env.as_str())
                    })
                    .map(|(id, _)| *id)
                    .collect();
                if win_ids.is_empty() {
                    return Task::none();
                }
                let tasks: Vec<_> = win_ids.iter().map(|id| window::gain_focus(*id)).collect();
                Task::batch(tasks)
            }

            // Phase 4: window opened callback
            Message::WindowOpened(win_id) => {
                tracing::debug!("window opened: {win_id:?}");
                self.focused_window = Some(win_id);
                // Bug 3 fix: focus the terminal widget in the new window so
                // keyboard input goes to it without requiring a mouse click.
                let terminal_id = self
                    .windows
                    .get(&win_id)
                    .and_then(|w| w.tabs.get(w.active_tab))
                    .and_then(|t| t.terminal.as_ref())
                    .map(|t| t.widget_id().clone());
                if let Some(id) = terminal_id {
                    return Task::batch([
                        window::gain_focus(win_id),
                        iced_term::TerminalView::focus(id),
                    ]);
                }
                window::gain_focus(win_id)
            }

            // Phase 5: show (focus) the control window
            Message::ShowControlWindow => {
                tracing::info!("show control window");
                let control_id = self.control_window_id;
                if self.windows.contains_key(&control_id) {
                    // Control window still open, just focus it
                    self.focused_window = Some(control_id);
                    window::gain_focus(control_id)
                } else {
                    // Control window was closed, re-open it
                    // P1: compact control window size
                    let (new_id, open_task) = window::open(window::Settings {
                        size: iced::Size::new(360.0, 420.0),
                        ..window::Settings::default()
                    });
                    let control_win = super::AppWindow::new_control(new_id);
                    self.windows.insert(new_id, control_win);
                    self.window_order.push(new_id);
                    self.control_window_id = new_id;
                    self.focused_window = Some(new_id);
                    open_task.map(|_| Message::Noop)
                }
            }

            // P23: copy arbitrary string to clipboard
            Message::CopyToClipboard(s) => {
                self.toast = Some(("Copied to clipboard".to_string(), std::time::Instant::now()));
                iced::clipboard::write(s)
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

        if let Some(tab) = self.find_tab_mut(id) {
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
        let max_attempts = self.config.ssh.reconnect_max_attempts;
        if shutdown && let Some(tab) = self.find_tab_mut(id) {
            tab.terminal = None;
            let attempt = tab.reconnect_attempts();
            let was_auto = tab.is_auto_reconnect()
                || matches!(
                    tab.conn_state,
                    super::tab::ConnectionState::Connected { .. }
                        | super::tab::ConnectionState::Connecting { .. }
                );
            if was_auto && attempt < max_attempts {
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
            && let Some(tab) = self.find_tab_mut(id)
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
                let active_tab = self.active_window().map(|w| w.active_tab).unwrap_or(0);
                let win_id = self.active_window_id();
                if let (Some(regex), Some(wid)) = (&mut self.search.regex, win_id)
                    && let Some(win) = self.windows.get_mut(&wid)
                    && let Some(tab) = win.tabs.get_mut(active_tab)
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
                let active_tab = self.active_window().map(|w| w.active_tab).unwrap_or(0);
                let win_id = self.active_window_id();
                if let (Some(regex), Some(wid)) = (&mut self.search.regex, win_id)
                    && let Some(win) = self.windows.get_mut(&wid)
                    && let Some(tab) = win.tabs.get_mut(active_tab)
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
                tracing::info!("export scrollback");
                if let Some(win) = self.active_window()
                    && let Some(tab) = win.tabs.get(win.active_tab)
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
                tracing::info!("copy scrollback to clipboard");
                if let Some(win) = self.active_window()
                    && let Some(tab) = win.tabs.get(win.active_tab)
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
                        // FR-STATE-02: read server state (shared + device)
                        Task::perform(
                            async move {
                                let shared = syncer_clone
                                    .read_shared_state()
                                    .await
                                    .map_err(|e| e.to_string())?;
                                let device = syncer_clone
                                    .read_device_state()
                                    .await
                                    .map_err(|e| e.to_string())?;
                                Ok((shared, device))
                            },
                            |result: Result<(Option<String>, Option<String>), String>| {
                                Message::ServerStateLoaded(result)
                            },
                        )
                    }
                    Err(e) => {
                        tracing::warn!("state syncer init failed: {e}");
                        Task::none()
                    }
                }
            }

            // FR-STATE-02: server state loaded (shared + device)
            Message::ServerStateLoaded(result) => {
                match result {
                    Ok((shared_json, device_json)) => {
                        if let Some(json) = shared_json {
                            match serde_json::from_str::<SharedState>(&json) {
                                Ok(state) => {
                                    tracing::info!(
                                        "loaded server shared state: {} workspaces",
                                        state.workspaces.len()
                                    );
                                    self.restore_hidden_windows_from_shared(&state);
                                    self.cached_shared_state = Some(state);
                                }
                                Err(e) => {
                                    tracing::warn!("corrupt server shared state: {e}");
                                    self.cached_shared_state = Some(SharedState::new());
                                }
                            }
                        } else {
                            tracing::info!("no server shared state found (first connection)");
                            let mut initial = SharedState::new();
                            shellkeep::state::environment::create_workspace(
                                &mut initial,
                                &self.current_workspace,
                            )
                            .ok();
                            self.cached_shared_state = Some(initial);
                        }
                        if let Some(json) = device_json {
                            match serde_json::from_str::<DeviceState>(&json) {
                                Ok(state) => {
                                    tracing::info!(
                                        "loaded server device state for {}",
                                        state.client_id
                                    );
                                    self.cached_device_state = Some(state);
                                }
                                Err(e) => tracing::warn!("corrupt server device state: {e}"),
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("failed to read server state: {e}");
                        // Set empty state so reconciliation can still proceed
                        self.cached_shared_state =
                            Some(self.cached_shared_state.take().unwrap_or_default());
                    }
                }
                self.server_state_loaded = true;
                // Clear connecting state — we're fully connected now
                self.connecting_server = None;
                // Run deferred reconciliation FIRST (before flush_state, which
                // would overwrite cached_shared_state with current tabs).
                let reconcile_task = self.try_reconcile_pending();
                // Now flush — after reconciliation, the tab list reflects the
                // restored sessions and gets written to server correctly.
                self.state_dirty = true;
                self.flush_state();
                reconcile_task
            }

            // Control-plane connection complete: SSH + tmux check + lock + state loaded
            Message::ServerConnected(result) => match result {
                Err(e) => {
                    tracing::error!("server connection failed: {e}");
                    self.connecting_server = None;
                    let el = e.to_lowercase();
                    if el.contains("session locked by") || el.contains("lock held by") {
                        // FR-LOCK-05: show lock conflict dialog
                        tracing::info!("server locked, showing conflict dialog");
                        self.dialogs.show_lock_dialog = true;
                        self.dialogs.lock_info_text = e;
                        self.dialogs.lock_target_tab = None;
                    } else if (el.contains("auth failed")
                        || el.contains("no authentication method succeeded"))
                        && !self.dialogs.show_password_dialog
                    {
                        // FR-CONN-09: show password prompt on auth failure
                        tracing::info!("auth failed, prompting for password");
                        self.dialogs.show_password_dialog = true;
                        self.dialogs.password_input.clear();
                        self.dialogs.password_target_tab = None;
                        self.dialogs.password_conn_params = self.current_conn.clone();
                    } else if el.contains("already connected to this server") {
                        self.error = Some(format!(
                            "Duplicate connection — {e}\nUse the existing entry or disconnect it first."
                        ));
                    } else {
                        self.error = Some(format!("Connection failed: {e}"));
                    }
                    Task::none()
                }
                Ok(result) => {
                    // Store the syncer
                    self.state_syncer = Some(result.syncer);

                    // Parse shared state
                    if let Some(json) = result.shared_state {
                        match serde_json::from_str::<SharedState>(&json) {
                            Ok(state) => {
                                tracing::info!(
                                    "loaded server shared state: {} workspaces",
                                    state.workspaces.len()
                                );
                                self.restore_hidden_windows_from_shared(&state);
                                self.cached_shared_state = Some(state);
                            }
                            Err(e) => {
                                tracing::warn!("corrupt server shared state: {e}");
                                self.cached_shared_state = Some(SharedState::new());
                            }
                        }
                    } else {
                        tracing::info!("no server shared state found (first connection)");
                        let mut initial = SharedState::new();
                        shellkeep::state::environment::create_workspace(
                            &mut initial,
                            &self.current_workspace,
                        )
                        .ok();
                        self.cached_shared_state = Some(initial);
                    }

                    // Parse device state
                    if let Some(json) = result.device_state {
                        match serde_json::from_str::<DeviceState>(&json) {
                            Ok(state) => {
                                tracing::info!(
                                    "loaded server device state for {}",
                                    state.client_id
                                );
                                self.cached_device_state = Some(state);
                            }
                            Err(e) => tracing::warn!("corrupt server device state: {e}"),
                        }
                    }

                    self.server_state_loaded = true;
                    self.sessions_listed = true;
                    self.connecting_server = None;

                    let label = self
                        .current_conn
                        .as_ref()
                        .map(|c| format!("{}@{}", c.key.username, c.key.host))
                        .unwrap_or_else(|| "shellkeep".to_string());

                    // Open session window(s) BEFORE reconciliation so tabs land in them.
                    // Multi-window restore: create one window per distinct server_window_id
                    // from saved state, or one default window if no saved state.
                    let has_session_windows = self
                        .windows
                        .values()
                        .any(|w| w.kind == super::WindowKind::Session);

                    let mut window_open_tasks: Vec<Task<Message>> = Vec::new();
                    if !has_session_windows {
                        let saved_ws_tabs = self
                            .cached_shared_state
                            .as_ref()
                            .map(|s| s.workspace_tabs(&self.current_workspace))
                            .unwrap_or_default();

                        // Collect distinct server_window_ids (preserving order)
                        let mut saved_window_ids: Vec<String> = Vec::new();
                        for tab in &saved_ws_tabs {
                            if let Some(ref swid) = tab.server_window_id
                                && !saved_window_ids.contains(swid)
                            {
                                saved_window_ids.push(swid.clone());
                            }
                        }
                        // Old state or no tabs: create one default window
                        if saved_window_ids.is_empty() {
                            saved_window_ids.push(uuid::Uuid::new_v4().to_string());
                        }

                        let device_state = self.cached_device_state.clone();
                        let last_active = device_state
                            .as_ref()
                            .and_then(|d| d.last_active_window.clone());
                        let server_uuid = self.current_conn.as_ref().and_then(|c| {
                            self.saved_servers
                                .servers
                                .iter()
                                .find(|s| s.host == c.key.host)
                                .map(|s| s.uuid.clone())
                        });

                        let mut first_session_win = true;
                        for swid in &saved_window_ids {
                            let geo = device_state
                                .as_ref()
                                .and_then(|d| d.window_geometry.get(swid));
                            let size = geo
                                .map(|g| iced::Size::new(g.width as f32, g.height as f32))
                                .unwrap_or(iced::Size::new(900.0, 600.0));
                            let position = geo
                                .and_then(|g| match (g.x, g.y) {
                                    (Some(x), Some(y)) => Some(window::Position::Specific(
                                        iced::Point::new(x as f32, y as f32),
                                    )),
                                    _ => None,
                                })
                                .unwrap_or(window::Position::Default);

                            let (win_id, open_task) = window::open(window::Settings {
                                size,
                                position,
                                ..window::Settings::default()
                            });

                            let mut session_win = super::AppWindow::new(win_id);
                            session_win.server_window_id = swid.clone();
                            session_win.server_uuid = server_uuid.clone();
                            session_win.workspace_env = Some(self.current_workspace.clone());
                            self.window_counter += 1;
                            session_win.name =
                                format!("{} - Window {}", label, self.window_counter);

                            if let Some(geo) = geo {
                                session_win.window_x = geo.x;
                                session_win.window_y = geo.y;
                                session_win.window_width = geo.width;
                                session_win.window_height = geo.height;
                            }

                            self.windows.insert(win_id, session_win);
                            self.window_order.push(win_id);

                            // Always focus the first session window; last_active overrides
                            if first_session_win || last_active.as_deref() == Some(swid.as_str()) {
                                self.focused_window = Some(win_id);
                            }
                            first_session_win = false;

                            window_open_tasks.push(open_task.map(|_| Message::Noop));
                        }
                    }

                    // Run reconciliation — tabs will be routed to their original windows
                    let reconcile_task = self.reconcile_sessions(result.sessions);

                    // If reconciliation didn't restore any tabs, create a fresh one
                    let fresh_tab_task = if self.all_tabs().next().is_none() {
                        let tmux_session = self.next_tmux_session();
                        Some(self.open_tab_russh(&label, &tmux_session))
                    } else {
                        None
                    };

                    self.state_dirty = true;
                    self.flush_state();

                    let mut tasks = vec![reconcile_task];
                    tasks.extend(window_open_tasks);
                    if let Some(t) = fresh_tab_task {
                        tasks.push(t);
                    }
                    Task::batch(tasks)
                }
            },

            _ => Task::none(),
        }
    }

    // -----------------------------------------------------------------------
    // Server / workspace management messages (Phase 6)
    // -----------------------------------------------------------------------

    fn handle_workspace_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ConnectServer(uuid) => {
                tracing::info!("connect server: {uuid}");
                if let Some(server) = self.saved_servers.find_by_uuid(&uuid).cloned() {
                    // Populate welcome fields from saved server
                    self.welcome.host_input = server.host.clone();
                    self.welcome.port_input = server.port.clone();
                    self.welcome.user_input = server.user.clone();
                    self.welcome.identity_input = server.identity_file.clone().unwrap_or_default();
                    // Update last_connected timestamp
                    self.saved_servers.push(server);
                    self.saved_servers.save();
                    // Mark this server as connecting for UI feedback
                    self.connecting_server = Some(uuid);
                    // Trigger the existing Connect flow
                    return self.update(Message::Connect);
                }
                Task::none()
            }

            Message::DisconnectAllWorkspaces(_uuid) => {
                tracing::info!("disconnect all workspaces");
                // Delegate to existing DisconnectServer logic
                self.update(Message::DisconnectServer)
            }

            Message::EditServer(uuid) => {
                tracing::info!("edit server: {uuid}");
                self.update(Message::ShowServerForm(Some(uuid)))
            }

            Message::ForgetServer(uuid) => {
                tracing::info!("forget server requested: {uuid}");
                self.dialogs.show_forget_server = Some(uuid);
                Task::none()
            }

            Message::ConfirmForgetServer => {
                tracing::info!("forget server confirmed");
                if let Some(uuid) = self.dialogs.show_forget_server.take() {
                    self.saved_servers.remove_by_uuid(&uuid);
                    self.saved_servers.save();
                }
                Task::none()
            }

            Message::CancelForgetServer => {
                tracing::debug!("forget server cancelled");
                self.dialogs.show_forget_server = None;
                Task::none()
            }

            Message::ShowServerForm(opt_uuid) => {
                tracing::info!("show server form, edit={}", opt_uuid.is_some());
                if let Some(ref uuid) = opt_uuid {
                    // Editing: populate form from saved server
                    if let Some(server) = self.saved_servers.find_by_uuid(uuid) {
                        self.dialogs.server_form_name = server.name.clone().unwrap_or_default();
                        self.dialogs.server_form_host = server.host.clone();
                        self.dialogs.server_form_port = server.port.clone();
                        self.dialogs.server_form_user = server.user.clone();
                        self.dialogs.server_form_identity =
                            server.identity_file.clone().unwrap_or_default();
                    }
                } else {
                    // Adding: clear form
                    self.dialogs.server_form_name.clear();
                    self.dialogs.server_form_host.clear();
                    self.dialogs.server_form_port = "22".to_string();
                    self.dialogs.server_form_user = crate::cli::default_ssh_username();
                    self.dialogs.server_form_identity.clear();
                }
                self.dialogs.show_server_form = Some(opt_uuid);
                Task::none()
            }

            Message::BackToServerList => {
                tracing::debug!("back to server list");
                self.dialogs.show_server_form = None;
                Task::none()
            }

            Message::SaveServer => {
                tracing::info!("save server");
                let server = self.build_server_from_form();
                self.saved_servers.push(server);
                self.saved_servers.save();
                self.dialogs.show_server_form = None;
                Task::none()
            }

            Message::SaveAndConnectServer => {
                tracing::info!("save and connect server");
                let server = self.build_server_from_form();
                let uuid = server.uuid.clone();
                self.saved_servers.push(server);
                self.saved_servers.save();
                self.dialogs.show_server_form = None;
                self.update(Message::ConnectServer(uuid))
            }

            Message::ServerFormNameChanged(v) => {
                self.dialogs.server_form_name = v;
                Task::none()
            }
            Message::ServerFormHostChanged(v) => {
                self.dialogs.server_form_host = v;
                Task::none()
            }
            Message::ServerFormPortChanged(v) => {
                self.dialogs.server_form_port = v;
                Task::none()
            }
            Message::ServerFormUserChanged(v) => {
                self.dialogs.server_form_user = v;
                Task::none()
            }
            Message::ServerFormIdentityChanged(v) => {
                self.dialogs.server_form_identity = v;
                Task::none()
            }

            Message::ConnectWorkspace(_server_uuid, env) => {
                tracing::info!("connect workspace: {_server_uuid}/{env}");
                // Delegate to existing SwitchWorkspace logic
                self.update(Message::SwitchWorkspace(env))
            }

            Message::DisconnectWorkspace(_server_uuid, _env) => {
                tracing::info!("disconnect workspace: {_server_uuid}/{_env}");
                self.update(Message::DisconnectServer)
            }

            Message::OpenWorkspace(_server_uuid, _env) => {
                tracing::info!("open workspace: {_server_uuid}/{_env}");
                // Focus first session window
                if let Some((&id, _)) = self
                    .windows
                    .iter()
                    .find(|(_, w)| w.kind == super::WindowKind::Session)
                {
                    self.focused_window = Some(id);
                }
                Task::none()
            }

            Message::ShowNewWorkspace(server_uuid) => {
                tracing::info!("show new workspace dialog for server {server_uuid}");
                self.dialogs.show_new_workspace = Some(server_uuid);
                self.dialogs.new_workspace_input.clear();
                Task::none()
            }

            Message::NewWorkspaceInputChanged(v) => {
                self.dialogs.new_workspace_input = v;
                Task::none()
            }

            Message::ConfirmNewWorkspace => {
                tracing::info!(
                    "confirm new workspace: {}",
                    self.dialogs.new_workspace_input
                );
                self.dialogs.show_new_workspace = None;
                let name = std::mem::take(&mut self.dialogs.new_workspace_input);
                if !name.trim().is_empty() {
                    self.dialogs.new_workspace_dialog_input = name;
                    return self.update(Message::ConfirmNewWorkspaceDialog);
                }
                Task::none()
            }

            Message::CancelNewWorkspace => {
                tracing::debug!("cancel new workspace");
                self.dialogs.show_new_workspace = None;
                Task::none()
            }

            Message::ShowRenameWorkspace(_server_uuid, env) => {
                tracing::info!("show rename workspace: {env}");
                self.dialogs.show_workspace_rename = Some((_server_uuid, env.clone()));
                self.dialogs.workspace_rename_input = env;
                Task::none()
            }

            Message::RenameWorkspaceInputChanged(v) => {
                self.dialogs.workspace_rename_input = v;
                Task::none()
            }

            Message::ConfirmRenameWorkspace => {
                tracing::info!("confirm rename workspace");
                if let Some((_server_uuid, old_name)) = self.dialogs.show_workspace_rename.take() {
                    let new_name = std::mem::take(&mut self.dialogs.workspace_rename_input);
                    if !new_name.trim().is_empty() {
                        self.dialogs.rename_workspace_target = Some(old_name);
                        self.dialogs.rename_workspace_dialog_input = new_name;
                        return self.update(Message::ConfirmRenameWorkspaceDialog);
                    }
                }
                Task::none()
            }

            Message::CancelRenameWorkspace => {
                tracing::debug!("cancel rename workspace");
                self.dialogs.show_workspace_rename = None;
                Task::none()
            }

            Message::ShowDeleteWorkspace(_server_uuid, env) => {
                tracing::info!("show delete workspace: {env}");
                self.dialogs.show_workspace_delete = Some((_server_uuid, env.clone()));
                self.dialogs.delete_workspace_target = Some(env);
                Task::none()
            }

            Message::ConfirmDeleteWorkspace => {
                let (server_uuid, workspace_name) = match self.dialogs.show_workspace_delete.take()
                {
                    Some(pair) => pair,
                    None => return Task::none(),
                };
                tracing::info!(
                    "confirm delete workspace: {workspace_name} on server {server_uuid}"
                );

                // 1. Find tmux sessions belonging to this workspace
                let prefix = format!("{workspace_name}--shellkeep-");
                let sessions_to_kill: Vec<String> = self
                    .cached_shared_state
                    .as_ref()
                    .and_then(|s| s.workspaces.get(&workspace_name))
                    .map(|ws| {
                        ws.tabs
                            .iter()
                            .map(|t| t.tmux_session_name.clone())
                            .collect()
                    })
                    .unwrap_or_default();

                // 2. Close all session windows and remove hidden sessions for this workspace
                let mut close_tasks: Vec<Task<Message>> = Vec::new();
                let mut win_ids_to_remove: Vec<window::Id> = Vec::new();
                for (&win_id, win) in &self.windows {
                    if win.kind == super::WindowKind::Session {
                        // Remove tabs that belong to this workspace
                        let has_workspace_tabs =
                            win.tabs.iter().any(|t| t.tmux_session.starts_with(&prefix));
                        if has_workspace_tabs {
                            win_ids_to_remove.push(win_id);
                            close_tasks.push(window::close(win_id));
                        }
                    }
                }
                for win_id in &win_ids_to_remove {
                    self.windows.remove(win_id);
                    self.window_order.retain(|id| id != win_id);
                }
                if self
                    .focused_window
                    .is_some_and(|id| win_ids_to_remove.contains(&id))
                {
                    self.focused_window = self.window_order.first().copied();
                }

                // Remove hidden sessions for this workspace
                self.hidden_sessions.retain(|uuid| {
                    if let Some(ref state) = self.cached_shared_state
                        && let Some(ws) = state.workspaces.get(&workspace_name)
                    {
                        return !ws.tabs.iter().any(|t| t.session_uuid == *uuid);
                    }
                    true
                });

                // 3. Remove workspace from cached shared state
                if let Some(ref mut state) = self.cached_shared_state {
                    state.workspaces.remove(&workspace_name);
                }
                self.dialogs.workspace_list.retain(|e| *e != workspace_name);
                if self.current_workspace == workspace_name {
                    self.current_workspace = self
                        .dialogs
                        .workspace_list
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "Default".to_string());
                }

                // 4. Kill tmux sessions on server
                if !sessions_to_kill.is_empty() {
                    let mgr = self.conn_manager.clone();
                    if let Some(ref conn) = self.current_conn {
                        let conn_key = conn.key.clone();
                        let sessions = sessions_to_kill.clone();
                        close_tasks.push(Task::perform(
                            async move {
                                let mgr_guard = mgr.lock().await;
                                if let Some(handle_arc) = mgr_guard.get_cached(&conn_key) {
                                    let handle = handle_arc.lock().await;
                                    for name in &sessions {
                                        let cmd =
                                            format!("tmux kill-session -t {name} 2>/dev/null");
                                        let _ = ssh::connection::exec_command(&handle, &cmd).await;
                                        tracing::info!("killed tmux session: {name}");
                                    }
                                }
                            },
                            |_| Message::Noop,
                        ));
                    }
                }

                self.state_dirty = true;
                self.flush_state();
                self.toast = Some((
                    format!("Workspace \"{workspace_name}\" removed"),
                    std::time::Instant::now(),
                ));

                if close_tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(close_tasks)
                }
            }

            Message::CancelDeleteWorkspace => {
                tracing::debug!("cancel delete workspace");
                self.dialogs.show_workspace_delete = None;
                self.dialogs.delete_workspace_target = None;
                Task::none()
            }

            Message::WorkspaceSessionsFound(_uuid, _env, result) => {
                // Delegate to existing ExistingSessionsFound
                self.update(Message::ExistingSessionsFound(result))
            }

            Message::RestoreWorkspaceHiddenWindows(env) => {
                tracing::info!("restore hidden windows for workspace: {env}");
                let mut to_restore = Vec::new();
                let mut kept = Vec::new();
                for hw in self.hidden_windows.drain(..) {
                    if hw.workspace_env.as_deref() == Some(env.as_str()) {
                        to_restore.push(hw);
                    } else {
                        kept.push(hw);
                    }
                }
                self.hidden_windows = kept;

                let mut tasks: Vec<Task<Message>> = Vec::new();
                for hw in to_restore {
                    // Remove session UUIDs from hidden_sessions
                    for ht in &hw.tabs {
                        self.hidden_sessions.retain(|u| u != &ht.session_uuid);
                    }

                    // FR-SESSION-13a: geometry comes from per-device window_geometry
                    let geo = self
                        .cached_device_state
                        .as_ref()
                        .and_then(|d| d.window_geometry.get(&hw.server_window_id));
                    let size = geo
                        .map(|g| iced::Size::new(g.width as f32, g.height as f32))
                        .unwrap_or(iced::Size::new(900.0, 600.0));
                    let position = geo
                        .and_then(|g| match (g.x, g.y) {
                            (Some(x), Some(y)) => Some(window::Position::Specific(
                                iced::Point::new(x as f32, y as f32),
                            )),
                            _ => None,
                        })
                        .unwrap_or(window::Position::Default);

                    let (new_id, open_task) = window::open(window::Settings {
                        size,
                        position,
                        ..window::Settings::default()
                    });

                    // Create AppWindow with restored state
                    let mut new_win = super::AppWindow::new(new_id);
                    new_win.server_window_id = hw.server_window_id;
                    new_win.name = hw.name;
                    new_win.server_uuid = hw.server_uuid;
                    new_win.workspace_env = hw.workspace_env;
                    if let Some(geo) = geo {
                        new_win.window_width = geo.width;
                        new_win.window_height = geo.height;
                        new_win.window_x = geo.x;
                        new_win.window_y = geo.y;
                    }

                    self.windows.insert(new_id, new_win);
                    self.window_order.push(new_id);
                    self.focused_window = Some(new_id);

                    tasks.push(open_task.map(Message::WindowOpened));

                    // Restore each tab by reattaching to existing tmux session
                    let prev_env = self.current_workspace.clone();
                    self.current_workspace = env.clone();
                    for ht in hw.tabs {
                        tasks.push(self.open_tab_russh(&ht.label, &ht.tmux_session_name));
                    }
                    self.current_workspace = prev_env;
                }

                self.save_state();
                if tasks.is_empty() {
                    Task::none()
                } else {
                    Task::batch(tasks)
                }
            }

            _ => Task::none(),
        }
    }

    /// Build a `SavedServer` from the current form fields.
    fn build_server_from_form(&self) -> shellkeep::state::server::SavedServer {
        use shellkeep::state::server::SavedServer;
        // If editing an existing server, reuse its UUID
        let uuid = self
            .dialogs
            .show_server_form
            .as_ref()
            .and_then(|opt| opt.as_ref())
            .and_then(|u| self.saved_servers.find_by_uuid(u))
            .map(|s| s.uuid.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        // Normalize host: if user typed "host:port", split them
        let raw_host = self.dialogs.server_form_host.trim();
        let (parsed_user, parsed_host, parsed_port) = crate::cli::parse_host_input(raw_host);
        let effective_host = parsed_host;
        let effective_user = if !self.dialogs.server_form_user.trim().is_empty() {
            self.dialogs.server_form_user.clone()
        } else if let Some(u) = parsed_user {
            u
        } else {
            crate::cli::default_ssh_username()
        };
        let effective_port = if !self.dialogs.server_form_port.trim().is_empty() {
            self.dialogs.server_form_port.clone()
        } else if let Some(p) = parsed_port {
            p
        } else {
            "22".to_string()
        };

        SavedServer {
            uuid,
            name: if self.dialogs.server_form_name.trim().is_empty() {
                None
            } else {
                Some(self.dialogs.server_form_name.clone())
            },
            host: effective_host,
            user: effective_user,
            port: effective_port,
            identity_file: if self.dialogs.server_form_identity.trim().is_empty() {
                None
            } else {
                Some(self.dialogs.server_form_identity.clone())
            },
            last_connected: None,
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
