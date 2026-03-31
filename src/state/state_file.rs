// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! State file: persisted window/tab layout, split into shared and per-device.
//!
//! Shared state (environments, tabs) is stored on server at
//! `~/.terminal-state/shared.json` and locally at
//! `$XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/shared.json`.
//!
//! Per-device state (geometry, hidden sessions) is stored on server at
//! `~/.terminal-state/clients/<client-id>.json` and locally at
//! `$XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/<client-id>.json`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 3;

/// Cached regex for validating tmux session names.
// SAFETY: this regex pattern is a compile-time constant and is known to be valid
#[allow(clippy::unwrap_used)]
static TMUX_NAME_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^[a-zA-Z0-9_][a-zA-Z0-9_.:\-]*$").unwrap());

// ---------------------------------------------------------------------------
// Shared state — same for all devices connecting to a server
// ---------------------------------------------------------------------------

/// Shared state: environments with windows/tabs, last active environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedState {
    pub schema_version: u32,
    pub last_modified: String,
    /// FR-ENV-01: named environment groupings
    #[serde(default)]
    pub environments: HashMap<String, Environment>,
    /// FR-ENV-04: last active environment
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_environment: Option<String>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_modified: chrono_now(),
            environments: HashMap::new(),
            last_environment: None,
        }
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

    /// Load shared state from a local file.
    /// Validates, deduplicates tabs, and handles corrupt files.
    pub fn load_local(path: &std::path::Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<SharedState>(&content) {
            Ok(mut state) => {
                if state.schema_version > SCHEMA_VERSION {
                    tracing::error!(
                        "shared state from newer version (v{}), cannot load (current: v{})",
                        state.schema_version,
                        SCHEMA_VERSION
                    );
                    return None;
                }

                // Validate and deduplicate tabs in each environment
                validate_environments(&mut state.environments);
                state.last_modified = chrono_now();

                Some(state)
            }
            Err(e) => {
                tracing::warn!("corrupt shared state file {}: {e}", path.display());
                rename_corrupt(path);
                None
            }
        }
    }

    /// Get the local cache path for shared state.
    pub fn local_cache_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellkeep")
            .join("state")
            .join("shared.json")
    }
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Per-device state — unique per client-id
// ---------------------------------------------------------------------------

/// Per-device window geometry.
/// FR-STATE-14: persisted window position and size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
}

/// Per-device state: window geometry, hidden sessions, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub schema_version: u32,
    pub last_modified: String,
    pub client_id: String,
    /// FR-STATE-14: window geometry per window-id (single window for now)
    #[serde(default)]
    pub window_geometry: HashMap<String, WindowGeometry>,
    /// Hidden session UUIDs (Phase 3 will populate this)
    #[serde(default)]
    pub hidden_sessions: Vec<String>,
    /// Last active window ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_window: Option<String>,
}

impl DeviceState {
    pub fn new(client_id: &str) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_modified: chrono_now(),
            client_id: client_id.to_string(),
            window_geometry: HashMap::new(),
            hidden_sessions: Vec::new(),
            last_active_window: None,
        }
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

    /// Load device state from a local file.
    pub fn load_local(path: &std::path::Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<DeviceState>(&content) {
            Ok(mut state) => {
                if state.schema_version > SCHEMA_VERSION {
                    tracing::error!(
                        "device state from newer version (v{}), cannot load (current: v{})",
                        state.schema_version,
                        SCHEMA_VERSION
                    );
                    return None;
                }
                state.last_modified = chrono_now();
                Some(state)
            }
            Err(e) => {
                tracing::warn!("corrupt device state file {}: {e}", path.display());
                rename_corrupt(path);
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

// ---------------------------------------------------------------------------
// Legacy StateFile — kept for migration from v1/v2 single-file format
// ---------------------------------------------------------------------------

/// Legacy top-level state file (v1/v2 format). Used only for migration.
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
    /// FR-STATE-14: persisted window geometry (v2 format)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
}

/// FR-ENV-01: a named grouping of sessions within a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub tabs: Vec<TabState>,
}

/// Legacy FR-STATE-14: window position and size (v2 format).
/// Kept for backward compatibility during migration.
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
            schema_version: 2,
            last_modified: chrono_now(),
            client_id: client_id.to_string(),
            environments: HashMap::new(),
            last_environment: None,
            tabs: Vec::new(),
            window: None,
        }
    }

    /// Migrate v1 state (flat tabs) to v2 (environments).
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
        self.schema_version = 2;
    }

    /// Convert legacy StateFile into split SharedState + DeviceState.
    pub fn into_split(self) -> (SharedState, DeviceState) {
        let mut shared = SharedState {
            schema_version: SCHEMA_VERSION,
            last_modified: chrono_now(),
            environments: self.environments,
            last_environment: self.last_environment,
        };
        validate_environments(&mut shared.environments);

        let mut device = DeviceState::new(&self.client_id);
        if let Some(w) = self.window {
            device.window_geometry.insert(
                "main".to_string(),
                WindowGeometry {
                    x: w.x,
                    y: w.y,
                    width: w.width,
                    height: w.height,
                },
            );
        }

        (shared, device)
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

    /// Load state from a local file. Handles v1/v2 legacy format and migrates
    /// to v3 (split shared/device). Returns None if the file does not exist
    /// or is fatally corrupt.
    ///
    /// If new v3 shared state already exists, this returns None to avoid
    /// overwriting it with stale data.
    pub fn load_local(path: &std::path::Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<StateFile>(&content) {
            Ok(mut state) => {
                if state.schema_version == 0 {
                    tracing::warn!("state file has schema_version 0, expected > 0");
                }

                // FR-STATE-08: schema version migration
                if state.schema_version > 2 {
                    // v3+ uses the new split format, not this legacy loader
                    tracing::debug!("state file is v3+, skipping legacy load");
                    return None;
                }
                if state.schema_version < 2 {
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
                validate_environments(&mut state.environments);

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
                rename_corrupt(path);
                None
            }
        }
    }

    /// Get the local cache path for a given client_id (legacy format).
    pub fn local_cache_path(client_id: &str) -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellkeep")
            .join("state")
            .join(format!("{client_id}.json"))
    }
}

// ---------------------------------------------------------------------------
// Migration: load v3 split state, falling back to legacy single-file
// ---------------------------------------------------------------------------

/// Load shared + device state from local cache, migrating from legacy format if needed.
///
/// Tries v3 split files first. If shared.json does not exist, looks for a legacy
/// `<client-id>.json` and migrates it into split format.
pub fn load_split_state(client_id: &str) -> (Option<SharedState>, Option<DeviceState>) {
    let shared_path = SharedState::local_cache_path();
    let device_path = DeviceState::local_cache_path(client_id);

    let shared = SharedState::load_local(&shared_path);
    let device = DeviceState::load_local(&device_path);

    if shared.is_some() {
        return (shared, device);
    }

    // No v3 shared state — try legacy migration
    let legacy_path = StateFile::local_cache_path(client_id);
    if let Some(legacy) = StateFile::load_local(&legacy_path) {
        tracing::info!("migrating legacy state to v3 split format");
        let (new_shared, new_device) = legacy.into_split();

        // Save migrated files
        if let Err(e) = new_shared.save_local(&shared_path) {
            tracing::warn!("failed to save migrated shared state: {e}");
        }
        if let Err(e) = new_device.save_local(&device_path) {
            tracing::warn!("failed to save migrated device state: {e}");
        }

        // Back up legacy file
        let bak_path = legacy_path.with_extension("v2.bak");
        if let Err(e) = fs::rename(&legacy_path, &bak_path) {
            tracing::warn!("failed to rename legacy state file: {e}");
        } else {
            tracing::info!("backed up legacy state to {}", bak_path.display());
        }

        return (Some(new_shared), Some(new_device));
    }

    (None, None)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn chrono_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Validate and deduplicate tabs in all environments.
fn validate_environments(environments: &mut HashMap<String, Environment>) {
    for env in environments.values_mut() {
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
}

/// Rename a corrupt file to `.corrupt.<timestamp>` for diagnosis.
fn rename_corrupt(path: &std::path::Path) {
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
}

/// FR-STATE-07: remove orphaned .tmp files from state directory.
pub fn cleanup_tmp_files(client_id: &str) {
    // Clean up tmp files in both the legacy path directory and new paths
    let paths = [
        StateFile::local_cache_path(client_id),
        SharedState::local_cache_path(),
        DeviceState::local_cache_path(client_id),
    ];
    let mut cleaned_dirs = HashSet::new();
    for state_path in &paths {
        if let Some(dir) = state_path.parent()
            && cleaned_dirs.insert(dir.to_path_buf())
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_state_roundtrip() {
        let mut state = SharedState::new();
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
        let loaded: SharedState = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.environments.len(), 1);
        let env = loaded.environments.get("Default").unwrap();
        assert_eq!(env.tabs.len(), 1);
        assert_eq!(env.tabs[0].tmux_session_name, "shellkeep-0");
    }

    #[test]
    fn device_state_roundtrip() {
        let mut state = DeviceState::new("test-device");
        state.window_geometry.insert(
            "main".to_string(),
            WindowGeometry {
                x: Some(100),
                y: Some(200),
                width: 1024,
                height: 768,
            },
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: DeviceState = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.client_id, "test-device");
        let geo = loaded.window_geometry.get("main").unwrap();
        assert_eq!(geo.x, Some(100));
        assert_eq!(geo.y, Some(200));
        assert_eq!(geo.width, 1024);
        assert_eq!(geo.height, 768);
    }

    #[test]
    fn device_state_hidden_sessions_default_empty() {
        let state = DeviceState::new("test");
        assert!(state.hidden_sessions.is_empty());
    }

    #[test]
    fn shared_state_empty() {
        let state = SharedState::new();
        assert!(state.environments.is_empty());
        assert_eq!(state.schema_version, 3);
    }

    #[test]
    fn legacy_state_roundtrip() {
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
    fn legacy_to_split_migration() {
        let mut legacy = StateFile::new("migrate-test");
        legacy.environments.insert(
            "Default".to_string(),
            Environment {
                name: "Default".to_string(),
                tabs: vec![TabState {
                    session_uuid: "uuid-1".into(),
                    tmux_session_name: "shellkeep-0".into(),
                    title: "Tab 1".into(),
                    position: 0,
                }],
            },
        );
        legacy.last_environment = Some("Default".to_string());
        legacy.window = Some(WindowState {
            x: Some(50),
            y: Some(100),
            width: 800,
            height: 600,
        });

        let (shared, device) = legacy.into_split();

        assert_eq!(shared.schema_version, 3);
        assert_eq!(shared.environments.len(), 1);
        assert_eq!(shared.last_environment, Some("Default".to_string()));
        let env = shared.environments.get("Default").unwrap();
        assert_eq!(env.tabs.len(), 1);

        assert_eq!(device.schema_version, 3);
        assert_eq!(device.client_id, "migrate-test");
        let geo = device.window_geometry.get("main").unwrap();
        assert_eq!(geo.x, Some(50));
        assert_eq!(geo.y, Some(100));
        assert_eq!(geo.width, 800);
        assert_eq!(geo.height, 600);
        assert!(device.hidden_sessions.is_empty());
    }

    #[test]
    fn shared_state_window_geometry_absent() {
        let json = r#"{"schema_version":3,"last_modified":"0Z"}"#;
        let loaded: SharedState = serde_json::from_str(json).unwrap();
        assert!(loaded.environments.is_empty());
    }

    #[test]
    fn device_state_geometry_absent() {
        let json = r#"{"schema_version":3,"last_modified":"0Z","client_id":"test"}"#;
        let loaded: DeviceState = serde_json::from_str(json).unwrap();
        assert!(loaded.window_geometry.is_empty());
        assert!(loaded.hidden_sessions.is_empty());
    }

    #[test]
    fn shared_state_dedup_uuids_in_environment() {
        let dir = std::env::temp_dir().join("sk-dedup-env-test-v3");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("dedup.json");

        let mut state = SharedState::new();
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

        let loaded = SharedState::load_local(&path).unwrap();
        let env = loaded.environments.get("Default").unwrap();
        assert_eq!(env.tabs.len(), 2);
        assert_eq!(env.tabs[0].session_uuid, "uuid-A");
        assert_eq!(env.tabs[0].title, "First");
        assert_eq!(env.tabs[1].session_uuid, "uuid-B");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_split_state_from_legacy() {
        let dir = std::env::temp_dir().join("sk-split-load-test");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);

        // Create a legacy state file where load_split_state will find it.
        // We need to use a unique client_id and set up the directory structure.
        // Since local_cache_path uses dirs::data_dir(), we test into_split directly.
        let mut legacy = StateFile::new("split-test");
        legacy.environments.insert(
            "Default".to_string(),
            Environment {
                name: "Default".to_string(),
                tabs: vec![TabState {
                    session_uuid: "u1".into(),
                    tmux_session_name: "sk-0".into(),
                    title: "T1".into(),
                    position: 0,
                }],
            },
        );
        legacy.window = Some(WindowState {
            x: None,
            y: None,
            width: 1280,
            height: 720,
        });

        let (shared, device) = legacy.into_split();
        assert_eq!(shared.environments.len(), 1);
        assert_eq!(device.client_id, "split-test");
        let geo = device.window_geometry.get("main").unwrap();
        assert_eq!(geo.width, 1280);
        assert_eq!(geo.height, 720);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
