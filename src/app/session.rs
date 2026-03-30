// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SSH session establishment and channel streaming.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use iced::futures::stream::BoxStream;
use iced::futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;

use crate::app::tab::{ChannelHolder, ConnParams, ResizeRxHolder, TabId, WriterRxHolder};
use crate::app::Message;
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
                                Message::SshDisconnected(tab_id, "connection lost".into())
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

/// Parameters for establishing an SSH session.
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
}

/// Establish an SSH session: connect, acquire lock, create tmux session, open PTY channel.
/// Returns the raw russh Channel on success.
pub(crate) async fn establish_ssh_session(
    params: EstablishParams,
) -> Result<russh::Channel<russh::client::Msg>, SshError> {
    let conn_key = params.conn.key.clone();

    // SAFETY: mutex is never held across a panic path
    #[allow(clippy::unwrap_used)]
    {
        *params.phase.lock().unwrap() = i18n::t(i18n::AUTHENTICATING).to_string();
    }

    let (handle_arc, _host_key_prompt) = {
        let mut mgr = params.conn_manager.lock().await;
        let result = mgr
            .get_or_connect(
                &conn_key,
                params.conn.identity_file.as_deref(),
                params.password.as_deref(),
                params.keepalive_secs,
            )
            .await?;
        (result.handle, result.host_key_prompt)
    };

    // IMPORTANT: Don't hold handle_arc.lock() across the entire function.
    // Multiple tabs share the same Handle via ConnectionManager. Each operation
    // locks briefly, then releases, allowing other tabs to interleave.

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
        return Err(SshError::Channel("tmux not found on server — install tmux >= 3.0 to use shellkeep".to_string()));
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
        ssh::lock::release_lock(&h, &params.client_id).await?;
    }

    {
        let h = handle_arc.lock().await;
        ssh::lock::acquire_lock(&h, &params.client_id, keepalive_timeout)
            .await?;
    }

    // FR-RECONNECT-03: verify tmux session exists before reattaching, create if needed
    // SAFETY: mutex is never held across a panic path
    #[allow(clippy::unwrap_used)]
    {
        *params.phase.lock().unwrap() = i18n::t(i18n::OPENING_SESSION).to_string();
    }

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
        tracing::info!("tmux session {} not found, creating new one", params.tmux_session);
        let h = handle_arc.lock().await;
        ssh::tmux::create_session_russh(&h, &params.tmux_session)
            .await?;
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
