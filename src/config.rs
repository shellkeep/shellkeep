// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuration file support.
//!
//! Loads settings from `$XDG_CONFIG_HOME/shellkeep/config.toml`.
//! All values have sensible defaults — the file is optional.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

/// Terminal cursor shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorShape {
    #[default]
    Block,
    Ibeam,
    Underline,
}

impl std::fmt::Display for CursorShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Block => write!(f, "block"),
            Self::Ibeam => write!(f, "ibeam"),
            Self::Underline => write!(f, "underline"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub terminal: TerminalConfig,
    pub ssh: SshConfig,
    pub keybindings: KeybindingsConfig,
    pub state: StateConfig,
    pub tray: TrayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Friendly client identifier (e.g. "work-laptop").
    pub client_id: Option<String>,
    /// Theme name: "dark" (default), "light", or a custom theme file.
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Font size in points.
    pub font_size: f32,
    /// Font family (monospace).
    pub font_family: Option<String>,
    /// Scrollback buffer size in lines.
    pub scrollback_lines: u32,
    /// Cursor shape.
    pub cursor_shape: CursorShape,
    /// Enable hyperlink detection.
    pub hyperlinks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SshConfig {
    /// Default SSH port.
    pub default_port: u16,
    /// Connection timeout in seconds.
    pub connect_timeout: u32,
    /// Keepalive interval in seconds (0 = disabled).
    pub keepalive_interval: u32,
    /// Max reconnection attempts (0 = infinite).
    pub reconnect_max_attempts: u32,
    /// Reconnection backoff base delay in seconds.
    pub reconnect_backoff_base: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    pub new_tab: String,
    pub close_tab: String,
    pub next_tab: String,
    pub prev_tab: String,
    pub rename_tab: String,
    pub zoom_in: String,
    pub zoom_out: String,
    pub zoom_reset: String,
    pub copy: String,
    pub paste: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StateConfig {
    /// Max history file size in MB.
    pub history_max_size_mb: u32,
    /// History retention in days.
    pub history_max_days: u32,
    /// Auto-save interval in seconds.
    pub auto_save_interval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TrayConfig {
    /// Show system tray icon.
    pub enabled: bool,
    /// Close to tray instead of exit.
    pub close_to_tray: bool,
    /// Start minimized to tray.
    pub start_minimized: bool,
}

// Default implementations

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            client_id: None,
            theme: "dark".to_string(),
        }
    }
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            font_family: None,
            scrollback_lines: 10_000,
            cursor_shape: CursorShape::Block,
            hyperlinks: true,
        }
    }
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            default_port: 22,
            connect_timeout: 10,
            keepalive_interval: 15,
            reconnect_max_attempts: 10,
            reconnect_backoff_base: 2.0,
        }
    }
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            new_tab: "Ctrl+Shift+T".into(),
            close_tab: "Ctrl+Shift+W".into(),
            next_tab: "Ctrl+Tab".into(),
            prev_tab: "Ctrl+Shift+Tab".into(),
            rename_tab: "F2".into(),
            zoom_in: "Ctrl+=".into(),
            zoom_out: "Ctrl+-".into(),
            zoom_reset: "Ctrl+0".into(),
            copy: "Ctrl+Shift+C".into(),
            paste: "Ctrl+Shift+V".into(),
        }
    }
}

impl Default for StateConfig {
    fn default() -> Self {
        Self {
            history_max_size_mb: 50,
            history_max_days: 90,
            auto_save_interval: 30,
        }
    }
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            close_to_tray: false,
            start_minimized: false,
        }
    }
}

impl Config {
    /// Load config from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        let path = Self::file_path();
        let mut config = match fs::read_to_string(&path) {
            Ok(data) => match toml::from_str(&data) {
                Ok(config) => {
                    tracing::info!("loaded config from {}", path.display());
                    config
                }
                Err(e) => {
                    tracing::warn!("config parse error (using defaults): {e}");
                    Self::default()
                }
            },
            Err(_) => {
                tracing::debug!("no config file found, using defaults");
                Self::default()
            }
        };
        config.validate();
        config
    }

    /// FR-CONFIG-03: clamp configuration values to safe ranges.
    fn validate(&mut self) {
        if self.ssh.keepalive_interval < 5 {
            tracing::warn!(
                "keepalive_interval {} too low, clamping to 5",
                self.ssh.keepalive_interval
            );
            self.ssh.keepalive_interval = 5;
        }
        if self.ssh.keepalive_interval > 300 {
            tracing::warn!(
                "keepalive_interval {} too high, clamping to 300",
                self.ssh.keepalive_interval
            );
            self.ssh.keepalive_interval = 300;
        }
        if self.ssh.reconnect_max_attempts > 100 {
            self.ssh.reconnect_max_attempts = 100;
        }
        self.terminal.font_size = self.terminal.font_size.clamp(6.0, 72.0);
        if self.terminal.scrollback_lines > 1_000_000 {
            self.terminal.scrollback_lines = 1_000_000;
        }
    }

    pub fn file_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellkeep")
            .join("config.toml")
    }
}

/// FR-UI-03: check if the config file exists (first-use detection).
pub fn config_file_exists() -> bool {
    Config::file_path().exists()
}

/// FR-CONFIG-04: start watching the config file for changes, returning a receiver
/// that gets notified when the file is modified.
pub fn watch_config(path: PathBuf) -> mpsc::Receiver<()> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (notify_tx, notify_rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res
                && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
            {
                let _ = notify_tx.send(());
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("failed to create config watcher: {e}");
                return;
            }
        };
        // Watch parent directory — some editors do atomic save (write tmp + rename)
        let watch_path = path.parent().unwrap_or(&path);
        if let Err(e) = watcher.watch(watch_path, RecursiveMode::NonRecursive) {
            tracing::warn!("failed to watch config directory: {e}");
            return;
        }
        tracing::info!("config watcher started for {}", path.display());
        for () in notify_rx {
            let _ = tx.send(());
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let config = Config::default();
        assert_eq!(config.terminal.font_size, 14.0);
        assert_eq!(config.ssh.default_port, 22);
        assert_eq!(config.ssh.connect_timeout, 10);
        assert_eq!(config.terminal.scrollback_lines, 10_000);
        assert_eq!(config.keybindings.new_tab, "Ctrl+Shift+T");
        assert!(config.tray.enabled);
        assert_eq!(config.state.history_max_days, 90);
    }

    #[test]
    fn parse_partial_toml() {
        let toml_str = r#"
            [terminal]
            font_size = 16.0

            [ssh]
            default_port = 2222
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.terminal.font_size, 16.0);
        assert_eq!(config.ssh.default_port, 2222);
        // Other values should be defaults
        assert_eq!(config.terminal.scrollback_lines, 10_000);
        assert_eq!(config.ssh.connect_timeout, 10);
    }

    #[test]
    fn parse_empty_toml() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.terminal.font_size, 14.0);
    }

    #[test]
    fn validate_clamps_values() {
        let mut config = Config::default();
        config.ssh.keepalive_interval = 1;
        config.ssh.reconnect_max_attempts = 999;
        config.terminal.font_size = 2.0;
        config.terminal.scrollback_lines = 5_000_000;
        config.validate();
        assert_eq!(config.ssh.keepalive_interval, 5);
        assert_eq!(config.ssh.reconnect_max_attempts, 100);
        assert_eq!(config.terminal.font_size, 6.0);
        assert_eq!(config.terminal.scrollback_lines, 1_000_000);
    }
}
