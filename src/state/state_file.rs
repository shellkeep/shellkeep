// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! State file: persisted window/tab layout.
//!
//! Stored on server at `~/.terminal-state/<client-id>.json` (primary)
//! and locally at `$XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/<client-id>.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const SCHEMA_VERSION: u32 = 1;

/// Top-level state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub schema_version: u32,
    pub last_modified: String,
    pub client_id: String,
    pub tabs: Vec<TabState>,
    /// FR-STATE-14: persisted window geometry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
}

/// FR-STATE-14: window position and size for geometry persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
}

/// Per-tab state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabState {
    pub session_uuid: String,
    pub tmux_session_name: String,
    pub title: String,
    pub position: usize,
}

impl StateFile {
    pub fn new(client_id: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_modified: chrono_now(),
            client_id: client_id.to_string(),
            tabs: Vec::new(),
            window: None,
        }
    }

    /// Save state to a local file atomically (tmp + rename).
    pub fn save_local(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Load state from a local file. Renames corrupt files instead of silently ignoring them.
    /// FR-TABS-02: deduplicates tabs by session_uuid, keeping only the first occurrence.
    pub fn load_local(path: &std::path::Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<StateFile>(&content) {
            Ok(mut state) => {
                let orig_len = state.tabs.len();
                let mut seen_uuids = HashSet::new();
                state
                    .tabs
                    .retain(|t| seen_uuids.insert(t.session_uuid.clone()));
                if state.tabs.len() < orig_len {
                    tracing::warn!(
                        "removed {} duplicate session UUID(s) from state",
                        orig_len - state.tabs.len()
                    );
                }
                Some(state)
            }
            Err(e) => {
                tracing::warn!("corrupt state file {}: {e}", path.display());
                // FR-CONN-19: rename to .corrupt.<timestamp> for diagnosis
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let corrupt_path = path.with_extension(format!("corrupt.{timestamp}"));
                if let Err(rename_err) = fs::rename(path, &corrupt_path) {
                    tracing::error!("failed to rename corrupt file: {rename_err}");
                } else {
                    tracing::info!("renamed corrupt state to {}", corrupt_path.display());
                }
                None
            }
        }
    }

    /// Get the local cache path for a given client_id.
    pub fn local_cache_path(client_id: &str) -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellkeep")
            .join("state")
            .join(format!("{client_id}.json"))
    }
}

fn chrono_now() -> String {
    // Simple ISO 8601 UTC timestamp without chrono dependency
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}Z", now.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        let mut state = StateFile::new("test-client");
        state.tabs.push(TabState {
            session_uuid: "uuid-1".into(),
            tmux_session_name: "shellkeep-0".into(),
            title: "Session 1".into(),
            position: 0,
        });

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: StateFile = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.client_id, "test-client");
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.tabs[0].tmux_session_name, "shellkeep-0");
    }

    #[test]
    fn state_empty() {
        let state = StateFile::new("empty");
        assert_eq!(state.tabs.len(), 0);
        assert_eq!(state.schema_version, 1);
    }

    #[test]
    fn state_window_geometry_roundtrip() {
        use super::WindowState;
        let mut state = StateFile::new("geo-test");
        state.window = Some(WindowState {
            x: Some(100),
            y: Some(200),
            width: 1024,
            height: 768,
        });
        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: StateFile = serde_json::from_str(&json).unwrap();
        let w = loaded.window.unwrap();
        assert_eq!(w.x, Some(100));
        assert_eq!(w.y, Some(200));
        assert_eq!(w.width, 1024);
        assert_eq!(w.height, 768);
    }

    #[test]
    fn state_window_geometry_absent() {
        // Old state files without window field should load fine
        let json = r#"{"schema_version":1,"last_modified":"0Z","client_id":"test","tabs":[]}"#;
        let loaded: StateFile = serde_json::from_str(json).unwrap();
        assert!(loaded.window.is_none());
    }

    #[test]
    fn state_dedup_uuids() {
        let dir = std::env::temp_dir().join("sk-dedup-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("dedup.json");

        let mut state = StateFile::new("dedup-client");
        state.tabs.push(TabState {
            session_uuid: "uuid-A".into(),
            tmux_session_name: "s1".into(),
            title: "First".into(),
            position: 0,
        });
        state.tabs.push(TabState {
            session_uuid: "uuid-A".into(), // duplicate
            tmux_session_name: "s2".into(),
            title: "Duplicate".into(),
            position: 1,
        });
        state.tabs.push(TabState {
            session_uuid: "uuid-B".into(),
            tmux_session_name: "s3".into(),
            title: "Third".into(),
            position: 2,
        });
        state.save_local(&path).unwrap();

        let loaded = StateFile::load_local(&path).unwrap();
        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.tabs[0].session_uuid, "uuid-A");
        assert_eq!(loaded.tabs[0].title, "First"); // first occurrence kept
        assert_eq!(loaded.tabs[1].session_uuid, "uuid-B");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
