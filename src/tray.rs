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

// ---------------------------------------------------------------------------
// Real implementation (feature = "tray")
// ---------------------------------------------------------------------------
#[cfg(feature = "tray")]
mod real_tray {
    use super::TrayAction;
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    /// FR-TRAY-01: system tray handle backed by `tray-icon` + `muda`.
    pub struct Tray {
        _icon: TrayIcon,
        show_id: muda::MenuId,
        hide_id: muda::MenuId,
        quit_id: muda::MenuId,
    }

    impl Tray {
        /// Create and show the system tray icon.
        ///
        /// Returns `None` if tray creation fails (e.g. missing system deps).
        pub fn new(enabled: bool) -> Option<Self> {
            if !enabled {
                tracing::debug!("system tray disabled in config");
                return None;
            }

            let icon = match create_icon() {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("failed to create tray icon: {e}");
                    return None;
                }
            };

            // FR-TRAY-02: build the menu
            let show_item = MenuItem::new("Show window", true, None);
            let hide_item = MenuItem::new("Hide window", true, None);
            let quit_item = MenuItem::new("Quit", true, None);

            let show_id = show_item.id().clone();
            let hide_id = hide_item.id().clone();
            let quit_id = quit_item.id().clone();

            let menu = Menu::new();
            let _ = menu.append(&show_item);
            let _ = menu.append(&hide_item);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&quit_item);

            match TrayIconBuilder::new()
                .with_tooltip("shellkeep")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .build()
            {
                Ok(tray_icon) => {
                    tracing::info!("system tray icon shown");
                    Some(Tray {
                        _icon: tray_icon,
                        show_id,
                        hide_id,
                        quit_id,
                    })
                }
                Err(e) => {
                    tracing::warn!("failed to build tray icon: {e}");
                    None
                }
            }
        }

        /// FR-TRAY-02: update the tooltip to reflect the number of active sessions.
        pub fn set_session_count(&self, count: usize) {
            use crate::i18n;
            let tooltip = if count > 0 {
                // NFR-I18N-03: use ngettext-style plural
                let sessions = i18n::tn(
                    i18n::N_ACTIVE_SESSIONS_1,
                    i18n::N_ACTIVE_SESSIONS_N,
                    count,
                );
                format!("shellkeep — {sessions}")
            } else {
                "shellkeep".to_string()
            };
            let _ = self._icon.set_tooltip(Some(&tooltip));
        }

        /// FR-TRAY-04: change icon appearance when sessions active but windows hidden.
        /// Orange icon = active sessions with hidden windows; blue = normal.
        pub fn set_hidden_active(&self, hidden_active: bool) {
            let icon = if hidden_active {
                create_icon_color(0xfa, 0xb3, 0x87) // catppuccin peach/orange
            } else {
                create_icon_color(0x89, 0xb4, 0xfa) // catppuccin blue
            };
            if let Ok(icon) = icon {
                let _ = self._icon.set_icon(Some(icon));
            }
        }

        /// Poll for tray menu events. Returns `None` when no event is pending.
        pub fn poll_event(&self) -> Option<TrayAction> {
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == self.show_id {
                    return Some(TrayAction::ShowWindow);
                }
                if event.id == self.hide_id {
                    return Some(TrayAction::HideWindow);
                }
                if event.id == self.quit_id {
                    return Some(TrayAction::Quit);
                }
            }
            None
        }
    }

    /// Generate a simple 32x32 RGBA icon — filled circle with given color.
    fn create_icon_color(r: u8, g: u8, b: u8) -> Result<Icon, tray_icon::BadIcon> {
        let size = 32u32;
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        let center = 15.5_f32;
        let radius_sq = 144.0_f32; // radius 12

        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                if dx * dx + dy * dy < radius_sq {
                    let i = ((y * size + x) * 4) as usize;
                    rgba[i] = r;
                    rgba[i + 1] = g;
                    rgba[i + 2] = b;
                    rgba[i + 3] = 0xff;
                }
            }
        }
        Icon::from_rgba(rgba, size, size)
    }

    /// Generate default icon (catppuccin blue #89b4fa).
    fn create_icon() -> Result<Icon, tray_icon::BadIcon> {
        create_icon_color(0x89, 0xb4, 0xfa)
    }
}

// ---------------------------------------------------------------------------
// Stub implementation (no tray feature)
// ---------------------------------------------------------------------------
#[cfg(not(feature = "tray"))]
mod stub_tray {
    use super::TrayAction;

    /// Stub system tray — always returns `None` from `new()`.
    pub struct Tray {
        _active: bool,
    }

    impl Tray {
        pub fn new(enabled: bool) -> Option<Self> {
            if !enabled {
                tracing::debug!("system tray disabled in config");
                return None;
            }
            tracing::info!("system tray not available (compile with --features tray)");
            None
        }

        pub fn set_session_count(&self, _count: usize) {}

        pub fn set_hidden_active(&self, _hidden_active: bool) {}

        pub fn poll_event(&self) -> Option<TrayAction> {
            None
        }
    }
}

#[cfg(feature = "tray")]
pub use real_tray::Tray;
#[cfg(not(feature = "tray"))]
pub use stub_tray::Tray;
