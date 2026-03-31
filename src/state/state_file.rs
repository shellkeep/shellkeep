// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! State file: persisted window/tab layout.
//!
//! Stored on server at `~/.terminal-state/<client-id>.json` (primary)
//! and locally at `$XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/<client-id>.json`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 2;

/// Cached regex for validating tmux session names.
static TMUX_NAME_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[a-zA-Z0-9_][a-zA-Z0-9_.:\-]*$").unwrap());

/// Top-level state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub schema_version: u32,
    pub last_modified: String,
    pub client_id: String,
    /// FR-ENV-01: named environment groupings
    #[serde(default)]
    pub environments: HashMap<String, Environment>,
    /// FR-ENV-04: last active environment
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_environment: Option<String>,
    /// Legacy v1 field — migrated to "Default" environment on load
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<TabState>,
    /// FR-STATE-14: persisted window geometry
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
}

/// FR-ENV-01: a named grouping of sessions within a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub tabs: Vec<TabState>,
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
            environments: HashMap::new(),
            last_environment: None,
            tabs: Vec::new(),
            window: None,
        }
    }

    /// Migrate v1 state (flat tabs) to v2 (environments).
    /// If the state has top-level tabs and no environments, move them into "Default".
    fn migrate_v1_to_v2(&mut self) {
        if !self.tabs.is_empty() && self.environments.is_empty() {
            tracing::info!(
                "migrating v1 state to v2: moving {} tabs to Default environment",
                self.tabs.len()
            );
            let tabs = std::mem::take(&mut self.tabs);
            self.environments.insert(
                "Default".to_string(),
                Environment {
                    name: "Default".to_string(),
                    tabs,
                },
            );
            self.last_environment = Some("Default".to_string());
        }
        self.schema_version = SCHEMA_VERSION;
    }

    /// Get tabs for an environment.
    pub fn env_tabs(&self, env_name: &str) -> Vec<TabState> {
        self.environments
            .get(env_name)
            .map(|e| e.tabs.clone())
            .unwrap_or_default()
    }

    /// Save state to a local file atomically (tmp + rename).
    pub fn save_local(&self, path: &std::path::Path) -> Result<(), crate::error::StateError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Load state from a local file. Renames corrupt files instead of silently ignoring them.
    /// FR-TABS-02: deduplicates tabs by session_uuid, keeping only the first occurrence.
    /// Automatically migrates v1 state to v2.
    pub fn load_local(path: &std::path::Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<StateFile>(&content) {
            Ok(mut state) => {
                // FR-STATE-16: validate state integrity
                if state.schema_version == 0 {
                    tracing::warn!("state file has schema_version 0, expected > 0");
                }

                // FR-STATE-08: schema version migration
                if state.schema_version > SCHEMA_VERSION {
                    tracing::error!(
                        "state file from newer version (v{}), cannot load (current: v{})",
                        state.schema_version,
                        SCHEMA_VERSION
                    );
                    return None;
                }
                if state.schema_version < SCHEMA_VERSION {
                    // Back up old version before migration
                    let bak_path = path.with_extension(format!("v{}.bak", state.schema_version));
                    if let Err(e) = fs::copy(path, &bak_path) {
                        tracing::warn!("failed to create backup before migration: {e}");
                    } else {
                        tracing::info!(
                            "backed up v{} state to {}",
                            state.schema_version,
                            bak_path.display()
                        );
                    }
                    state.migrate_v1_to_v2();
                }

                // Validate and deduplicate tabs in each environment
                for env in state.environments.values_mut() {
                    for tab in &env.tabs {
                        if !TMUX_NAME_RE.is_match(&tab.tmux_session_name) {
                            tracing::warn!(
                                "tab {} has invalid tmux_session_name: {:?}",
                                tab.session_uuid,
                                tab.tmux_session_name
                            );
                        }
                    }
                    let orig_len = env.tabs.len();
                    let mut seen_uuids = HashSet::new();
                    env.tabs
                        .retain(|t| seen_uuids.insert(t.session_uuid.clone()));
                    if env.tabs.len() < orig_len {
                        tracing::warn!(
                            "removed {} duplicate session UUID(s) from environment '{}'",
                            orig_len - env.tabs.len(),
                            env.name
                        );
                    }
                }

                // Also validate/dedup legacy tabs (shouldn't exist post-migration, but safety)
                if !state.tabs.is_empty() {
                    for tab in &state.tabs {
                        if !TMUX_NAME_RE.is_match(&tab.tmux_session_name) {
                            tracing::warn!(
                                "tab {} has invalid tmux_session_name: {:?}",
                                tab.session_uuid,
                                tab.tmux_session_name
                            );
                        }
                    }
                    let orig_len = state.tabs.len();
                    let mut seen_uuids = HashSet::new();
                    state
                        .tabs
                        .retain(|t| seen_uuids.insert(t.session_uuid.clone()));
                    if state.tabs.len() < orig_len {
                        tracing::warn!(
                            "removed {} duplicate session UUID(s) from legacy tabs",
                            orig_len - state.tabs.len()
                        );
                    }
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
    chrono::Utc::now().to_rfc3339()
}

/// FR-STATE-07: remove orphaned .tmp files from state directory.
pub fn cleanup_tmp_files(client_id: &str) {
    let state_path = StateFile::local_cache_path(client_id);
    if let Some(dir) = state_path.parent()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "tmp") {
                tracing::info!("cleaning orphaned tmp file: {}", path.display());
                let _ = fs::remove_file(&path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        let mut state = StateFile::new("test-client");
        state.environments.insert(
            "Default".to_string(),
            Environment {
                name: "Default".to_string(),
                tabs: vec![TabState {
                    session_uuid: "uuid-1".into(),
                    tmux_session_name: "shellkeep-0".into(),
                    title: "Session 1".into(),
                    position: 0,
                }],
            },
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: StateFile = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.schema_version, 2);
        assert_eq!(loaded.client_id, "test-client");
        assert_eq!(loaded.environments.len(), 1);
        let env = loaded.environments.get("Default").unwrap();
        assert_eq!(env.tabs.len(), 1);
        assert_eq!(env.tabs[0].tmux_session_name, "shellkeep-0");
    }

    #[test]
    fn state_empty() {
        let state = StateFile::new("empty");
        assert!(state.environments.is_empty());
        assert_eq!(state.schema_version, 2);
    }

    #[test]
    fn state_v1_migration() {
        // Simulate a v1 state file with flat tabs
        let json = r#"{
            "schema_version": 1,
            "last_modified": "0Z",
            "client_id": "test",
            "tabs": [
                {"session_uuid": "uuid-1", "tmux_session_name": "shellkeep-0", "title": "Tab 1", "position": 0},
                {"session_uuid": "uuid-2", "tmux_session_name": "shellkeep-1", "title": "Tab 2", "position": 1}
            ]
        }"#;

        let dir = std::env::temp_dir().join("sk-v1-mig-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("v1.json");
        std::fs::write(&path, json).unwrap();

        let state = StateFile::load_local(&path).unwrap();
        assert_eq!(state.schema_version, 2);
        assert!(state.tabs.is_empty(), "legacy tabs should be moved");
        assert_eq!(state.environments.len(), 1);
        let env = state.environments.get("Default").unwrap();
        assert_eq!(env.tabs.len(), 2);
        assert_eq!(env.tabs[0].session_uuid, "uuid-1");
        assert_eq!(env.tabs[1].session_uuid, "uuid-2");
        assert_eq!(state.last_environment, Some("Default".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_v1_migration_empty_tabs() {
        let json = r#"{"schema_version":1,"last_modified":"0Z","client_id":"test","tabs":[]}"#;
        let dir = std::env::temp_dir().join("sk-v1-empty-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("v1empty.json");
        std::fs::write(&path, json).unwrap();

        let state = StateFile::load_local(&path).unwrap();
        assert_eq!(state.schema_version, 2);
        assert!(state.environments.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_window_geometry_roundtrip() {
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
        let json = r#"{"schema_version":2,"last_modified":"0Z","client_id":"test"}"#;
        let loaded: StateFile = serde_json::from_str(json).unwrap();
        assert!(loaded.window.is_none());
    }

    #[test]
    fn state_dedup_uuids_in_environment() {
        let dir = std::env::temp_dir().join("sk-dedup-env-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("dedup.json");

        let mut state = StateFile::new("dedup-client");
        state.environments.insert(
            "Default".to_string(),
            Environment {
                name: "Default".to_string(),
                tabs: vec![
                    TabState {
                        session_uuid: "uuid-A".into(),
                        tmux_session_name: "s1".into(),
                        title: "First".into(),
                        position: 0,
                    },
                    TabState {
                        session_uuid: "uuid-A".into(),
                        tmux_session_name: "s2".into(),
                        title: "Duplicate".into(),
                        position: 1,
                    },
                    TabState {
                        session_uuid: "uuid-B".into(),
                        tmux_session_name: "s3".into(),
                        title: "Third".into(),
                        position: 2,
                    },
                ],
            },
        );
        state.save_local(&path).unwrap();

        let loaded = StateFile::load_local(&path).unwrap();
        let env = loaded.environments.get("Default").unwrap();
        assert_eq!(env.tabs.len(), 2);
        assert_eq!(env.tabs[0].session_uuid, "uuid-A");
        assert_eq!(env.tabs[0].title, "First");
        assert_eq!(env.tabs[1].session_uuid, "uuid-B");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
