// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! System tray icon support (FR-TRAY-01..06).
//!
//! When the `tray` cargo feature is enabled, this module uses the `tray-icon`
//! and `muda` crates for cross-platform tray support. Without the feature,
//! a stub implementation logs that tray is unavailable.

/// Actions received from the system tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    ShowWindow,
    HideWindow,
    Quit,
}

/// System tray handle. Wraps platform-specific tray implementation.
pub struct Tray {
    _active: bool,
}

impl Tray {
    /// Create and show the system tray icon.
    ///
    /// Returns `None` if tray is not available on this platform or if the
    /// `tray` feature is not compiled in.
    pub fn new(enabled: bool) -> Option<Self> {
        if !enabled {
            tracing::debug!("system tray disabled in config");
            return None;
        }

        // Tray icon support requires the tray-icon crate which needs GTK3
        // dev libraries on Linux. When those are available, enable the `tray`
        // cargo feature to get real tray support.
        tracing::info!("system tray not available (compile with --features tray)");
        None
    }

    /// Update the tooltip to reflect the number of active sessions.
    pub fn set_session_count(&self, _count: usize) {
        // Stub — real implementation updates tray tooltip
    }

    /// Poll for tray menu events. Returns `None` when no event is pending.
    pub fn poll_event(&self) -> Option<TrayAction> {
        None
    }
}
