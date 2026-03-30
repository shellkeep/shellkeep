// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tab-related types for the shellkeep application.

use std::sync::Arc;

use shellkeep::ssh::manager::ConnKey;
use shellkeep::state::history;
use tokio::sync::Mutex;

/// Shared holder for a value that is take()n by the SSH subscription on first run.
pub(crate) type Holder<T> = Arc<Mutex<Option<T>>>;
pub(crate) type ChannelHolder = Holder<russh::Channel<russh::client::Msg>>;
pub(crate) type WriterRxHolder = Holder<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>;
pub(crate) type ResizeRxHolder = Holder<tokio::sync::mpsc::UnboundedReceiver<(u32, u32)>>;

/// Connection parameters parsed from user input.
#[derive(Clone, Debug)]
pub(crate) struct ConnParams {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: String,
    pub(crate) identity_file: Option<String>,
}

pub(crate) struct Tab {
    pub(crate) id: u64,
    pub(crate) label: String,
    /// FR-SESSION-07: stable UUID for state persistence
    pub(crate) session_uuid: String,
    pub(crate) terminal: Option<iced_term::Terminal>,
    /// Legacy: system ssh args (kept for compatibility during transition)
    pub(crate) ssh_args: Vec<String>,
    pub(crate) conn_params: Option<ConnParams>,
    pub(crate) tmux_session: String,
    pub(crate) dead: bool,
    pub(crate) reconnect_attempts: u32,
    pub(crate) auto_reconnect: bool,
    /// FR-RECONNECT-06: current reconnect delay in milliseconds (0 = use base)
    pub(crate) reconnect_delay_ms: u64,
    /// FR-UI-08: last error reason for display in dead tab
    pub(crate) last_error: Option<String>,
    /// FR-UI-04..05: last measured latency in milliseconds
    pub(crate) last_latency_ms: Option<u32>,
    /// FR-RECONNECT-02: timestamp when reconnection started (for countdown display)
    pub(crate) reconnect_started: Option<std::time::Instant>,
    /// Whether this tab uses russh (true) or system ssh (false)
    pub(crate) uses_russh: bool,
    // russh channel holder — taken by the subscription on first run
    pub(crate) ssh_channel_holder: Option<ChannelHolder>,
    // Writer rx holder — keyboard input receiver, taken by subscription
    pub(crate) ssh_writer_rx_holder: Option<WriterRxHolder>,
    // Resize command sender
    pub(crate) ssh_resize_tx: Option<tokio::sync::mpsc::UnboundedSender<(u32, u32)>>,
    // Resize rx holder — taken by subscription
    pub(crate) ssh_resize_rx_holder: Option<ResizeRxHolder>,
    #[allow(dead_code)]
    pub(crate) conn_key: Option<ConnKey>,
    /// Holder for a channel being established by the async task.
    /// Moved to ssh_channel_holder when SshConnected(Ok) arrives.
    pub(crate) pending_channel: Option<ChannelHolder>,
    /// FR-CONN-16: connection phase text, shared with async task
    pub(crate) connection_phase: Option<Arc<std::sync::Mutex<String>>>,
    /// FR-HISTORY-02: client-side JSONL history writer
    pub(crate) history_writer: Option<history::HistoryWriter>,
    /// FR-TERMINAL-16: true until first resize is sent to SSH channel after connect
    pub(crate) needs_initial_resize: bool,
}

/// FR-RECONNECT-02: braille spinner frames for reconnection animation
pub(crate) const SPINNER_FRAMES: &[char] = &[
    '\u{280B}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283C}', '\u{2834}', '\u{2826}',
    '\u{2827}', '\u{2807}', '\u{280F}',
];
