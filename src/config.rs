// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Configuration file support.
//!
//! Loads settings from `$XDG_CONFIG_HOME/shellkeep/config.toml`.
//! All values have sensible defaults — the file is optional.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub terminal: TerminalConfig,
    pub ssh: SshConfig,
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
    /// Cursor shape: "block", "ibeam", "underline".
    pub cursor_shape: String,
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

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            terminal: TerminalConfig::default(),
            ssh: SshConfig::default(),
        }
    }
}

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
            cursor_shape: "block".to_string(),
            hyperlinks: true,
        }
    }
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            default_port: 22,
            connect_timeout: 10,
            keepalive_interval: 30,
            reconnect_max_attempts: 10,
            reconnect_backoff_base: 2.0,
        }
    }
}

impl Config {
    /// Load config from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        let path = Self::file_path();
        match fs::read_to_string(&path) {
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
        }
    }

    fn file_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellkeep")
            .join("config.toml")
    }
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
}
