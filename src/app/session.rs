// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SSH session establishment and channel streaming.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use iced::futures::stream::BoxStream;
use iced::futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;

use crate::app::Message;
use crate::app::tab::{ChannelHolder, ConnParams, ResizeRxHolder, TabId, WriterRxHolder};
use shellkeep::error::SshError;
use shellkeep::ssh::manager::ConnectionManager;
use shellkeep::state::history;
use shellkeep::{i18n, ssh};

#[derive(Clone)]
pub(crate) struct SshSubscriptionData {
    pub(crate) tab_id: TabId,
    pub(crate) channel: ChannelHolder,
    pub(crate) writer_rx: WriterRxHolder,
    pub(crate) resize_rx: ResizeRxHolder,
}

impl Hash for SshSubscriptionData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tab_id.hash(state);
    }
}

pub(crate) fn ssh_channel_stream(data: &SshSubscriptionData) -> BoxStream<'static, Message> {
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
                        Some(russh::ChannelMsg::Eof) => {
                            tracing::info!("ssh stream {tab_id}: session exited");
                            let _ = output.send(
                                Message::SshDisconnected(tab_id, "session exited".into())
                            ).await;
                            break;
                        }
                        None => {
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

/// Parameters for establishing an SSH session (backward-compat wrapper).
pub(crate) struct EstablishParams {
    pub conn_manager: Arc<Mutex<ConnectionManager>>,
    pub conn: ConnParams,
    pub tmux_session: String,
    pub cols: u32,
    pub rows: u32,
    pub keepalive_secs: u32,
    pub client_id: String,
    pub session_uuid: String,
    pub phase: Arc<std::sync::Mutex<String>>,
    /// If Some, pass this password to `get_or_connect` (keyboard-interactive auth).
    pub password: Option<String>,
    /// If true, release the lock before acquiring it (lock takeover).
    pub force_lock: bool,
    /// Workspace (environment) name for per-workspace lock.
    pub workspace: String,
}

/// Parameters for connecting to a server (control-plane).
pub(crate) struct ConnectServerParams {
    pub conn_manager: Arc<Mutex<ConnectionManager>>,
    pub conn: ConnParams,
    pub keepalive_secs: u32,
    pub client_id: String,
    pub workspace: String,
    pub force_lock: bool,
    pub phase: Arc<std::sync::Mutex<String>>,
    pub password: Option<String>,
}

/// Parameters for opening a tab channel (data-plane).
pub(crate) struct OpenTabParams {
    pub conn_manager: Arc<Mutex<ConnectionManager>>,
    pub conn: ConnParams,
    pub tmux_session: String,
    pub cols: u32,
    pub rows: u32,
    pub session_uuid: String,
    pub phase: Arc<std::sync::Mutex<String>>,
}

/// Result returned by the control-plane server connection task.
pub(crate) struct ServerConnectResult {
    pub sessions: Vec<String>,
    pub syncer: Arc<ssh::sftp::StateSyncer>,
    pub shared_state: Option<String>,
    pub device_state: Option<String>,
}

impl Clone for ServerConnectResult {
    fn clone(&self) -> Self {
        Self {
            sessions: self.sessions.clone(),
            syncer: self.syncer.clone(),
            shared_state: self.shared_state.clone(),
            device_state: self.device_state.clone(),
        }
    }
}

impl std::fmt::Debug for ServerConnectResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConnectResult")
            .field("sessions", &self.sessions)
            .field("shared_state", &self.shared_state.is_some())
            .field("device_state", &self.device_state.is_some())
            .finish()
    }
}

/// Connect to a server: SSH connect, health check, tmux version check, lock acquire.
/// Does NOT create tmux sessions or open PTY channels (control-plane only).
pub(crate) async fn connect_server(params: ConnectServerParams) -> Result<(), SshError> {
    tracing::info!(
        "connect_server: {}:{} as {}",
        params.conn.key.host,
        params.conn.key.port,
        params.conn.key.username
    );
    let conn_key = params.conn.key.clone();

    // SAFETY: mutex is never held across a panic path
    #[allow(clippy::unwrap_used)]
    {
        *params.phase.lock().unwrap() = i18n::t(i18n::AUTHENTICATING).to_string();
    }

    let mut handle_arc = {
        let mut mgr = params.conn_manager.lock().await;
        mgr.get_or_connect(
            &conn_key,
            params.conn.identity_file.as_deref(),
            params.password.as_deref(),
            params.keepalive_secs,
        )
        .await?
        .handle
    };

    // IMPORTANT: Don't hold handle_arc.lock() across the entire function.
    // Multiple tabs share the same Handle via ConnectionManager. Each operation
    // locks briefly, then releases, allowing other tabs to interleave.

    // Health check: verify the cached handle is alive. If stale, evict and reconnect.
    {
        let h = handle_arc.lock().await;
        let alive = ssh::connection::exec_command(&h, "true").await.is_ok();
        drop(h);
        if !alive {
            tracing::info!("cached SSH handle is stale, reconnecting");
            let mut mgr = params.conn_manager.lock().await;
            mgr.remove(&conn_key);
            handle_arc = mgr
                .get_or_connect(
                    &conn_key,
                    params.conn.identity_file.as_deref(),
                    params.password.as_deref(),
                    params.keepalive_secs,
                )
                .await?
                .handle;
        }
    }

    // FR-CONN-13..15: check tmux availability and version
    // SAFETY: mutex is never held across a panic path
    #[allow(clippy::unwrap_used)]
    {
        *params.phase.lock().unwrap() = i18n::t(i18n::CHECKING_TMUX).to_string();
    }

    let tmux_version_output = {
        let h = handle_arc.lock().await;
        ssh::connection::exec_command(&h, "tmux -V 2>/dev/null || echo 'NOT_FOUND'")
            .await
            .unwrap_or_else(|_| "NOT_FOUND".to_string())
    };

    if tmux_version_output.contains("NOT_FOUND") || tmux_version_output.trim().is_empty() {
        return Err(SshError::Channel(
            "tmux not found on server — install tmux >= 3.0 to use shellkeep".to_string(),
        ));
    }

    if let Some(ver_str) = tmux_version_output.trim().strip_prefix("tmux ")
        && let Ok(major) = ver_str.split('.').next().unwrap_or("0").parse::<u32>()
        && major < 3
    {
        tracing::warn!("tmux version {ver_str} < 3.0 — some features may not work");
    }

    // FR-LOCK-01: acquire client-ID lock before reading state or creating sessions
    // SAFETY: mutex is never held across a panic path
    #[allow(clippy::unwrap_used)]
    {
        *params.phase.lock().unwrap() = if params.force_lock {
            "Taking over lock...".to_string()
        } else {
            i18n::t("Acquiring lock...").to_string()
        };
    }

    let keepalive_timeout = if params.keepalive_secs > 0 {
        Some(params.keepalive_secs as u64)
    } else {
        None
    };

    // If force_lock, release the existing lock first (lock takeover)
    if params.force_lock {
        let h = handle_arc.lock().await;
        ssh::lock::release_lock(&h, &params.workspace).await?;
    }

    {
        let h = handle_arc.lock().await;
        ssh::lock::acquire_lock(&h, &params.client_id, keepalive_timeout, &params.workspace)
            .await?;
    }

    Ok(())
}

/// Open a PTY channel for a tab: create/attach tmux session, open channel.
/// Assumes SSH connection already exists (uses get_or_connect for reconnection safety).
pub(crate) async fn open_tab_channel(
    params: OpenTabParams,
) -> Result<russh::Channel<russh::client::Msg>, SshError> {
    tracing::info!("open_tab_channel: tmux={}", params.tmux_session);
    let conn_key = params.conn.key.clone();

    // FR-RECONNECT-03: verify tmux session exists before reattaching, create if needed
    // SAFETY: mutex is never held across a panic path
    #[allow(clippy::unwrap_used)]
    {
        *params.phase.lock().unwrap() = i18n::t(i18n::OPENING_SESSION).to_string();
    }

    let handle_arc = {
        let mut mgr = params.conn_manager.lock().await;
        mgr.get_or_connect(&conn_key, params.conn.identity_file.as_deref(), None, 15)
            .await?
            .handle
    };

    let check = {
        let h = handle_arc.lock().await;
        ssh::connection::exec_command(
            &h,
            &format!(
                "tmux has-session -t {} 2>/dev/null && echo EXISTS",
                params.tmux_session
            ),
        )
        .await
        .unwrap_or_default()
    };

    if !check.trim().contains("EXISTS") {
        tracing::info!(
            "tmux session {} not found, creating new one",
            params.tmux_session
        );
        let h = handle_arc.lock().await;
        ssh::tmux::create_session_russh(&h, &params.tmux_session).await?;
    }

    // Open PTY channel and attach to tmux session
    let channel = {
        let h = handle_arc.lock().await;
        let ch = h
            .channel_open_session()
            .await
            .map_err(|e| SshError::Channel(format!("channel open: {e}")))?;

        ch.request_pty(false, "xterm-256color", params.cols, params.rows, 0, 0, &[])
            .await
            .map_err(|e| SshError::Channel(format!("pty: {e}")))?;

        let tmux_cmd = format!(
            "TERM=xterm-256color tmux new-session -A -s {} \\; set status off || exec $SHELL",
            params.tmux_session
        );
        ch.exec(true, tmux_cmd)
            .await
            .map_err(|e| SshError::Channel(format!("exec: {e}")))?;
        ch
    };

    // FR-SESSION-07: set SHELLKEEP_SESSION_UUID env var inside the tmux session
    {
        let h = handle_arc.lock().await;
        let uuid_cmd = format!(
            "tmux set-environment -t {} SHELLKEEP_SESSION_UUID {}",
            params.tmux_session, params.session_uuid
        );
        if let Err(e) = ssh::connection::exec_command(&h, &uuid_cmd).await {
            tracing::warn!("failed to set SHELLKEEP_SESSION_UUID: {e}");
        }
    }

    // FR-HISTORY-01: start server-side capture via tmux pipe-pane
    let pipe_cmd = history::pipe_pane_command(&params.tmux_session, &params.session_uuid);
    {
        let h = handle_arc.lock().await;
        if let Err(e) = ssh::connection::exec_command(&h, &pipe_cmd).await {
            tracing::warn!("failed to setup history pipe-pane: {e}");
        }
    }

    Ok(channel)
}

/// Establish an SSH session: connect, acquire lock, create tmux session, open PTY channel.
/// Returns the raw russh Channel on success.
///
/// This is a backward-compat wrapper that calls `connect_server()` (control-plane)
/// then `open_tab_channel()` (data-plane). Used by CLI launch and reconnection paths.
pub(crate) async fn establish_ssh_session(
    params: EstablishParams,
) -> Result<russh::Channel<russh::client::Msg>, SshError> {
    connect_server(ConnectServerParams {
        conn_manager: params.conn_manager.clone(),
        conn: params.conn.clone(),
        keepalive_secs: params.keepalive_secs,
        client_id: params.client_id.clone(),
        workspace: params.workspace.clone(),
        force_lock: params.force_lock,
        phase: params.phase.clone(),
        password: params.password.clone(),
    })
    .await?;

    open_tab_channel(OpenTabParams {
        conn_manager: params.conn_manager,
        conn: params.conn,
        tmux_session: params.tmux_session,
        cols: params.cols,
        rows: params.rows,
        session_uuid: params.session_uuid,
        phase: params.phase,
    })
    .await
}
