// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! State file: persisted window/tab layout, split into shared and per-device.
//!
//! Shared state (workspaces, tabs) is stored on server at
//! `~/.shellkeep/shared.json` and locally at
//! `$XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/shared.json`.
//!
//! Per-device state (geometry, hidden sessions) is stored on server at
//! `~/.shellkeep/clients/<client-id>.json` and locally at
//! `$XDG_DATA_HOME/shellkeep/cache/servers/<fingerprint>/<client-id>.json`.

use std::collections::{HashMap, HashSet};
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

/// Shared state: workspaces with windows/tabs, last active workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedState {
    pub schema_version: u32,
    pub last_modified: String,
    /// FR-ENV-01: named workspace groupings
    #[serde(default, alias = "environments")]
    pub workspaces: HashMap<String, Workspace>,
    /// FR-ENV-04: last active workspace
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "last_environment"
    )]
    pub last_workspace: Option<String>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_modified: chrono_now(),
            workspaces: HashMap::new(),
            last_workspace: None,
        }
    }

    /// Get tabs for a workspace.
    pub fn workspace_tabs(&self, workspace_name: &str) -> Vec<TabState> {
        self.workspaces
            .get(workspace_name)
            .map(|e| e.tabs.clone())
            .unwrap_or_default()
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
    /// FR-ENV-01: named workspace groupings
    #[serde(default, alias = "environments")]
    pub workspaces: HashMap<String, Workspace>,
    /// FR-ENV-04: last active workspace
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "last_environment"
    )]
    pub last_workspace: Option<String>,
    /// Legacy v1 field — migrated to "Default" workspace on load
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tabs: Vec<TabState>,
    /// FR-STATE-14: persisted window geometry (v2 format)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowState>,
}

/// FR-ENV-01: a named grouping of sessions within a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
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
    /// FR-SESSION-10: which window this tab belongs to (for multi-window restore)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_window_id: Option<String>,
}

impl StateFile {
    pub fn new(client_id: &str) -> Self {
        Self {
            schema_version: 2,
            last_modified: chrono_now(),
            client_id: client_id.to_string(),
            workspaces: HashMap::new(),
            last_workspace: None,
            tabs: Vec::new(),
            window: None,
        }
    }

    /// Convert legacy StateFile into split SharedState + DeviceState.
    pub fn into_split(self) -> (SharedState, DeviceState) {
        let mut shared = SharedState {
            schema_version: SCHEMA_VERSION,
            last_modified: chrono_now(),
            workspaces: self.workspaces,
            last_workspace: self.last_workspace,
        };
        validate_workspaces(&mut shared.workspaces);

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

    /// Get tabs for a workspace.
    pub fn workspace_tabs(&self, workspace_name: &str) -> Vec<TabState> {
        self.workspaces
            .get(workspace_name)
            .map(|e| e.tabs.clone())
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn chrono_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Validate and deduplicate tabs in all workspaces.
fn validate_workspaces(workspaces: &mut HashMap<String, Workspace>) {
    for ws in workspaces.values_mut() {
        for tab in &ws.tabs {
            if !TMUX_NAME_RE.is_match(&tab.tmux_session_name) {
                tracing::warn!(
                    "tab {} has invalid tmux_session_name: {:?}",
                    tab.session_uuid,
                    tab.tmux_session_name
                );
            }
        }
        let orig_len = ws.tabs.len();
        let mut seen_uuids = HashSet::new();
        ws.tabs
            .retain(|t| seen_uuids.insert(t.session_uuid.clone()));
        if ws.tabs.len() < orig_len {
            tracing::warn!(
                "removed {} duplicate session UUID(s) from workspace '{}'",
                orig_len - ws.tabs.len(),
                ws.name
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_state_roundtrip() {
        let mut state = SharedState::new();
        state.workspaces.insert(
            "Default".to_string(),
            Workspace {
                name: "Default".to_string(),
                tabs: vec![TabState {
                    session_uuid: "uuid-1".into(),
                    tmux_session_name: "shellkeep-0".into(),
                    title: "Session 1".into(),
                    position: 0,
                    server_window_id: None,
                }],
            },
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: SharedState = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.schema_version, 3);
        assert_eq!(loaded.workspaces.len(), 1);
        let ws = loaded.workspaces.get("Default").unwrap();
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.tabs[0].tmux_session_name, "shellkeep-0");
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
        assert!(state.workspaces.is_empty());
        assert_eq!(state.schema_version, 3);
    }

    #[test]
    fn legacy_state_roundtrip() {
        let mut state = StateFile::new("test-client");
        state.workspaces.insert(
            "Default".to_string(),
            Workspace {
                name: "Default".to_string(),
                tabs: vec![TabState {
                    session_uuid: "uuid-1".into(),
                    tmux_session_name: "shellkeep-0".into(),
                    title: "Session 1".into(),
                    position: 0,
                    server_window_id: None,
                }],
            },
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: StateFile = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.schema_version, 2);
        assert_eq!(loaded.client_id, "test-client");
        assert_eq!(loaded.workspaces.len(), 1);
        let ws = loaded.workspaces.get("Default").unwrap();
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.tabs[0].tmux_session_name, "shellkeep-0");
    }

    #[test]
    fn legacy_to_split_migration() {
        let mut legacy = StateFile::new("migrate-test");
        legacy.workspaces.insert(
            "Default".to_string(),
            Workspace {
                name: "Default".to_string(),
                tabs: vec![TabState {
                    session_uuid: "uuid-1".into(),
                    tmux_session_name: "shellkeep-0".into(),
                    title: "Tab 1".into(),
                    position: 0,
                    server_window_id: None,
                }],
            },
        );
        legacy.last_workspace = Some("Default".to_string());
        legacy.window = Some(WindowState {
            x: Some(50),
            y: Some(100),
            width: 800,
            height: 600,
        });

        let (shared, device) = legacy.into_split();

        assert_eq!(shared.schema_version, 3);
        assert_eq!(shared.workspaces.len(), 1);
        assert_eq!(shared.last_workspace, Some("Default".to_string()));
        let ws = shared.workspaces.get("Default").unwrap();
        assert_eq!(ws.tabs.len(), 1);

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
        assert!(loaded.workspaces.is_empty());
    }

    #[test]
    fn device_state_geometry_absent() {
        let json = r#"{"schema_version":3,"last_modified":"0Z","client_id":"test"}"#;
        let loaded: DeviceState = serde_json::from_str(json).unwrap();
        assert!(loaded.window_geometry.is_empty());
        assert!(loaded.hidden_sessions.is_empty());
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
        legacy.workspaces.insert(
            "Default".to_string(),
            Workspace {
                name: "Default".to_string(),
                tabs: vec![TabState {
                    session_uuid: "u1".into(),
                    tmux_session_name: "sk-0".into(),
                    title: "T1".into(),
                    position: 0,
                    server_window_id: None,
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
        assert_eq!(shared.workspaces.len(), 1);
        assert_eq!(device.client_id, "split-test");
        let geo = device.window_geometry.get("main").unwrap();
        assert_eq!(geo.width, 1280);
        assert_eq!(geo.height, 720);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
