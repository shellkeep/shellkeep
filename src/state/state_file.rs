// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! State file: persisted window/tab layout.
//!
//! Stored on server at `~/.terminal-state/<client-id>.json` (primary)
//! and locally at `$XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/<client-id>.json`.

use serde::{Deserialize, Serialize};
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

    /// Load state from a local file.
    pub fn load_local(path: &std::path::Path) -> Option<Self> {
        let data = fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
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
}
