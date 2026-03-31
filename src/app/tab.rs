// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tab-related types for the shellkeep application.

use std::fmt;
use std::sync::Arc;

use shellkeep::ssh::manager::ConnKey;
use shellkeep::state::history;
use tokio::sync::Mutex;

/// Shared holder for a value that is take()n by the SSH subscription on first run.
pub(crate) type Holder<T> = Arc<Mutex<Option<T>>>;
pub(crate) type ChannelHolder = Holder<russh::Channel<russh::client::Msg>>;
pub(crate) type WriterRxHolder = Holder<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>;
pub(crate) type ResizeRxHolder = Holder<tokio::sync::mpsc::UnboundedReceiver<(u32, u32)>>;

/// Strongly-typed tab identifier.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub(crate) struct TabId(pub(crate) u64);

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Connection parameters parsed from user input.
#[derive(Clone, Debug)]
pub(crate) struct ConnParams {
    pub(crate) key: ConnKey,
    pub(crate) identity_file: Option<String>,
}

/// Connection lifecycle state for a tab.
///
/// This enum is the single source of truth for where a tab stands in its
/// connection lifecycle.  All code should match on `tab.conn_state` (or use
/// the helper methods on `Tab`) instead of checking scattered booleans.
pub(crate) enum ConnectionState {
    /// Initial connection in progress (tab just opened).
    Connecting {
        phase: Arc<std::sync::Mutex<String>>,
        pending_channel: ChannelHolder,
    },
    /// Fully connected with an active SSH channel.
    Connected { channel: ChannelHolder },
    /// Connection lost; automatic reconnection in progress.
    Reconnecting {
        attempt: u32,
        delay_ms: u64,
        started: std::time::Instant,
        phase: Arc<std::sync::Mutex<String>>,
        pending_channel: Option<ChannelHolder>,
    },
    /// Disconnected (terminal dead). User can manually reconnect via the UI.
    /// Error details are stored in `Tab::last_error`.
    Disconnected,
}

/// Backend type for a tab — either system ssh (spawned process) or russh (async library).
///
/// This enum is the single source of truth for the backend type and its
/// associated resources.
pub(crate) enum TabBackend {
    SystemSsh {
        ssh_args: Vec<String>,
    },
    Russh {
        conn_params: ConnParams,
        writer_rx: Option<WriterRxHolder>,
        resize_rx: Option<ResizeRxHolder>,
        resize_tx: Option<tokio::sync::mpsc::UnboundedSender<(u32, u32)>>,
    },
}

pub(crate) struct Tab {
    pub(crate) id: TabId,
    pub(crate) label: String,
    /// FR-SESSION-07: stable UUID for state persistence
    pub(crate) session_uuid: String,
    pub(crate) terminal: Option<iced_term::Terminal>,
    pub(crate) tmux_session: String,
    /// FR-UI-08: last error reason for display in dead tab
    pub(crate) last_error: Option<String>,
    /// FR-UI-04..05: last measured latency in milliseconds
    pub(crate) last_latency_ms: Option<u32>,
    /// Connection lifecycle state — single source of truth.
    pub(crate) conn_state: ConnectionState,
    /// Backend type — single source of truth.
    pub(crate) backend: TabBackend,
    /// FR-HISTORY-02: client-side JSONL history writer
    pub(crate) history_writer: Option<history::HistoryWriter>,
    /// FR-TERMINAL-16: true until first resize is sent to SSH channel after connect
    pub(crate) needs_initial_resize: bool,
}

// ---------------------------------------------------------------------------
// Tab helper methods — read from conn_state / backend instead of booleans
// ---------------------------------------------------------------------------

impl Tab {
    /// True if the tab is in the Disconnected state (session dead).
    pub(crate) fn is_dead(&self) -> bool {
        matches!(self.conn_state, ConnectionState::Disconnected)
    }

    /// True if auto-reconnect is enabled (Reconnecting state).
    pub(crate) fn is_auto_reconnect(&self) -> bool {
        matches!(self.conn_state, ConnectionState::Reconnecting { .. })
    }

    /// True if the tab backend is russh.
    pub(crate) fn is_russh(&self) -> bool {
        matches!(self.backend, TabBackend::Russh { .. })
    }

    /// Get the connection phase text (for Connecting or Reconnecting states).
    pub(crate) fn connection_phase_text(&self) -> Option<String> {
        match &self.conn_state {
            ConnectionState::Connecting { phase, .. }
            | ConnectionState::Reconnecting { phase, .. } => phase.lock().ok().map(|g| g.clone()),
            _ => None,
        }
    }

    /// Get reconnect attempt count.
    pub(crate) fn reconnect_attempts(&self) -> u32 {
        match &self.conn_state {
            ConnectionState::Reconnecting { attempt, .. } => *attempt,
            ConnectionState::Disconnected => 0,
            _ => 0,
        }
    }

    /// Get reconnect delay in milliseconds.
    pub(crate) fn reconnect_delay_ms(&self) -> u64 {
        match &self.conn_state {
            ConnectionState::Reconnecting { delay_ms, .. } => *delay_ms,
            _ => 0,
        }
    }

    /// Get reconnect started instant.
    pub(crate) fn reconnect_started(&self) -> Option<std::time::Instant> {
        match &self.conn_state {
            ConnectionState::Reconnecting { started, .. } => Some(*started),
            _ => None,
        }
    }

    /// True if the tab has a connected SSH channel (russh).
    pub(crate) fn has_channel(&self) -> bool {
        matches!(self.conn_state, ConnectionState::Connected { .. })
    }

    /// Get the SSH channel holder (Connected state only).
    pub(crate) fn channel_holder(&self) -> Option<&ChannelHolder> {
        match &self.conn_state {
            ConnectionState::Connected { channel } => Some(channel),
            _ => None,
        }
    }

    /// Get the pending channel holder (Connecting or Reconnecting states).
    pub(crate) fn pending_channel(&self) -> Option<&ChannelHolder> {
        match &self.conn_state {
            ConnectionState::Connecting {
                pending_channel, ..
            } => Some(pending_channel),
            ConnectionState::Reconnecting {
                pending_channel, ..
            } => pending_channel.as_ref(),
            _ => None,
        }
    }

    /// Take the pending channel out of Connecting state.
    /// Returns None if not in Connecting state or already taken.
    pub(crate) fn take_pending_channel(&mut self) -> Option<ChannelHolder> {
        match &mut self.conn_state {
            ConnectionState::Connecting {
                pending_channel, ..
            } => {
                // Clone it out (Arc is cheap to clone)
                Some(pending_channel.clone())
            }
            ConnectionState::Reconnecting {
                pending_channel, ..
            } => pending_channel.take(),
            _ => None,
        }
    }

    /// Get the writer_rx holder from the backend (Russh only).
    pub(crate) fn writer_rx_holder(&self) -> Option<&WriterRxHolder> {
        match &self.backend {
            TabBackend::Russh { writer_rx, .. } => writer_rx.as_ref(),
            _ => None,
        }
    }

    /// Get the resize_rx holder from the backend (Russh only).
    pub(crate) fn resize_rx_holder(&self) -> Option<&ResizeRxHolder> {
        match &self.backend {
            TabBackend::Russh { resize_rx, .. } => resize_rx.as_ref(),
            _ => None,
        }
    }

    /// Get the resize_tx sender from the backend (Russh only).
    pub(crate) fn resize_tx(&self) -> Option<&tokio::sync::mpsc::UnboundedSender<(u32, u32)>> {
        match &self.backend {
            TabBackend::Russh { resize_tx, .. } => resize_tx.as_ref(),
            _ => None,
        }
    }

    /// Get ssh_args (SystemSsh only).
    pub(crate) fn ssh_args(&self) -> &[String] {
        match &self.backend {
            TabBackend::SystemSsh { ssh_args } => ssh_args,
            _ => &[],
        }
    }

    /// Get conn_params (Russh only).
    pub(crate) fn conn_params(&self) -> Option<&ConnParams> {
        match &self.backend {
            TabBackend::Russh { conn_params, .. } => Some(conn_params),
            _ => None,
        }
    }

    /// Transition to Connected state from Connecting/Reconnecting.
    /// Moves the pending channel to the Connected channel.
    pub(crate) fn mark_connected(&mut self, channel: ChannelHolder) {
        self.conn_state = ConnectionState::Connected { channel };
    }

    /// Transition to Disconnected state.
    pub(crate) fn mark_disconnected(&mut self, error: Option<String>) {
        self.last_error = error;
        self.conn_state = ConnectionState::Disconnected;
    }

    /// Transition to Reconnecting state.
    pub(crate) fn mark_reconnecting(
        &mut self,
        attempt: u32,
        delay_ms: u64,
        phase: Arc<std::sync::Mutex<String>>,
        pending_channel: Option<ChannelHolder>,
    ) {
        self.conn_state = ConnectionState::Reconnecting {
            attempt,
            delay_ms,
            started: std::time::Instant::now(),
            phase,
            pending_channel,
        };
    }

    /// Transition to Connecting state.
    pub(crate) fn mark_connecting(
        &mut self,
        phase: Arc<std::sync::Mutex<String>>,
        pending_channel: ChannelHolder,
    ) {
        self.conn_state = ConnectionState::Connecting {
            phase,
            pending_channel,
        };
    }

    /// Clear the resize_tx sender (on disconnect).
    pub(crate) fn clear_resize_tx(&mut self) {
        if let TabBackend::Russh {
            ref mut resize_tx, ..
        } = self.backend
        {
            *resize_tx = None;
        }
    }

    /// Set writer_rx holder on backend.
    pub(crate) fn set_writer_rx(&mut self, holder: WriterRxHolder) {
        if let TabBackend::Russh {
            ref mut writer_rx, ..
        } = self.backend
        {
            *writer_rx = Some(holder);
        }
    }

    /// Set resize_rx holder on backend.
    pub(crate) fn set_resize_rx(&mut self, holder: ResizeRxHolder) {
        if let TabBackend::Russh {
            ref mut resize_rx, ..
        } = self.backend
        {
            *resize_rx = Some(holder);
        }
    }

    /// Set resize_tx on backend.
    pub(crate) fn set_resize_tx(&mut self, tx: tokio::sync::mpsc::UnboundedSender<(u32, u32)>) {
        if let TabBackend::Russh {
            ref mut resize_tx, ..
        } = self.backend
        {
            *resize_tx = Some(tx);
        }
    }

    /// Set reconnect delay_ms in Reconnecting state.
    pub(crate) fn set_reconnect_delay_ms(&mut self, delay: u64) {
        if let ConnectionState::Reconnecting {
            ref mut delay_ms, ..
        } = self.conn_state
        {
            *delay_ms = delay;
        }
    }

    /// Reset reconnect attempts to 0 and delay to 0 in Reconnecting state.
    #[cfg(target_os = "linux")]
    pub(crate) fn reset_reconnect(&mut self) {
        if let ConnectionState::Reconnecting {
            ref mut attempt,
            ref mut delay_ms,
            ..
        } = self.conn_state
        {
            *attempt = 0;
            *delay_ms = 0;
        }
    }
}

/// Build terminal font settings from app config and current font size.
pub(crate) fn make_font_settings(
    config: &shellkeep::config::Config,
    font_size: f32,
) -> iced_term::settings::FontSettings {
    iced_term::settings::FontSettings {
        size: font_size,
        font_family: config.terminal.font_family.clone(),
        ..iced_term::settings::FontSettings::default()
    }
}

/// Build terminal theme settings from app config.
pub(crate) fn make_theme_settings(
    config: &shellkeep::config::Config,
) -> iced_term::settings::ThemeSettings {
    iced_term::settings::ThemeSettings {
        color_palette: Box::new(crate::theme::resolve_theme(&config.general.theme)),
    }
}

/// Build backend settings with cursor shape from config.
pub(crate) fn make_backend_settings(
    config: &shellkeep::config::Config,
) -> iced_term::settings::BackendSettings {
    iced_term::settings::BackendSettings {
        cursor_shape: config.terminal.cursor_shape.to_string(),
        ..iced_term::settings::BackendSettings::default()
    }
}

/// FR-RECONNECT-02: braille spinner frames for reconnection animation
pub(crate) const SPINNER_FRAMES: &[char] = &[
    '\u{280B}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283C}', '\u{2834}', '\u{2826}', '\u{2827}',
    '\u{2807}', '\u{280F}',
];
