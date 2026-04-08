// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! FR-STATE-20: merge-on-flush — resolve conflicts between local and remote SharedState.
//!
//! When two devices write to `shared.json` concurrently, the flush task detects a
//! version mismatch and merges per-entry using `updated_at` timestamps.  Tab existence
//! is determined by the live tmux session list (if the tmux session is alive, the tab
//! survives; if dead, it is removed regardless of what either state says).

use std::collections::{HashMap, HashSet};

use super::state_file::{SharedState, TabState, Workspace};

/// Merge local and remote shared state, filtering by live tmux sessions.
///
/// - Tabs whose tmux session is alive are preserved.
/// - For tabs present in both states, the one with a newer `updated_at` wins.
/// - Tabs only in local or only in remote are kept (additions).
/// - Orphaned tmux sessions (alive but in neither state) produce new tabs.
/// - Local tab ordering is preserved; remote-only tabs are appended.
pub fn merge_shared_states(
    local: &SharedState,
    remote: &SharedState,
    live_tmux_sessions: &[String],
    client_id: &str,
) -> SharedState {
    let live_set: HashSet<&str> = live_tmux_sessions.iter().map(|s| s.as_str()).collect();

    let all_ws_names: HashSet<&str> = local
        .workspaces
        .keys()
        .chain(remote.workspaces.keys())
        .map(|s| s.as_str())
        .collect();

    let mut merged_workspaces = HashMap::new();

    for ws_name in all_ws_names {
        let local_ws = local.workspaces.get(ws_name);
        let remote_ws = remote.workspaces.get(ws_name);

        let merged_ws = match (local_ws, remote_ws) {
            (Some(l), Some(r)) => merge_workspace(l, r, &live_set),
            (Some(l), None) => filter_workspace(l, &live_set),
            (None, Some(r)) => filter_workspace(r, &live_set),
            (None, None) => continue,
        };

        merged_workspaces.insert(ws_name.to_string(), merged_ws);
    }

    // Union hidden windows by server_window_id
    let mut seen_hw: HashSet<String> = HashSet::new();
    let mut hidden_windows = Vec::new();
    for hw in local
        .hidden_windows
        .iter()
        .chain(remote.hidden_windows.iter())
    {
        if seen_hw.insert(hw.server_window_id.clone()) {
            hidden_windows.push(hw.clone());
        }
    }

    SharedState {
        schema_version: local.schema_version,
        version_uuid: uuid::Uuid::new_v4().to_string(),
        last_modified_by: client_id.to_string(),
        last_modified: chrono::Utc::now().to_rfc3339(),
        workspaces: merged_workspaces,
        last_workspace: None, // deprecated in v4
        hidden_windows,
    }
}

/// Merge two versions of the same workspace.
fn merge_workspace(local: &Workspace, remote: &Workspace, live_set: &HashSet<&str>) -> Workspace {
    // Index remote tabs by session_uuid for fast lookup
    let remote_by_uuid: HashMap<&str, &TabState> = remote
        .tabs
        .iter()
        .map(|t| (t.session_uuid.as_str(), t))
        .collect();

    let mut merged_tabs: Vec<TabState> = Vec::new();
    let mut seen_uuids: HashSet<String> = HashSet::new();

    // 1. Walk local tabs in order — preserve local ordering
    for local_tab in &local.tabs {
        if !live_set.contains(local_tab.tmux_session_name.as_str()) {
            continue; // tmux session dead — drop
        }

        seen_uuids.insert(local_tab.session_uuid.clone());

        if let Some(remote_tab) = remote_by_uuid.get(local_tab.session_uuid.as_str()) {
            // Tab in both — merge by updated_at (newer wins)
            merged_tabs.push(merge_tab(local_tab, remote_tab));
        } else {
            // Local-only tab — keep
            merged_tabs.push(local_tab.clone());
        }
    }

    // 2. Append remote-only tabs (not in local, tmux alive)
    for remote_tab in &remote.tabs {
        if seen_uuids.contains(&remote_tab.session_uuid) {
            continue; // Already handled above
        }
        if !live_set.contains(remote_tab.tmux_session_name.as_str()) {
            continue; // tmux dead
        }
        seen_uuids.insert(remote_tab.session_uuid.clone());
        merged_tabs.push(remote_tab.clone());
    }

    // Reindex positions
    for (i, tab) in merged_tabs.iter_mut().enumerate() {
        tab.position = i;
    }

    // Workspace metadata: use newer updated_at
    let name = if remote.updated_at > local.updated_at && !remote.updated_at.is_empty() {
        remote.name.clone()
    } else {
        local.name.clone()
    };

    let updated_at = if remote.updated_at > local.updated_at {
        remote.updated_at.clone()
    } else {
        local.updated_at.clone()
    };

    Workspace {
        name,
        uuid: local.uuid.clone(), // stable, never changes
        tabs: merged_tabs,
        updated_at,
    }
}

/// Filter a workspace's tabs: keep only those with live tmux sessions.
fn filter_workspace(ws: &Workspace, live_set: &HashSet<&str>) -> Workspace {
    let mut tabs: Vec<TabState> = ws
        .tabs
        .iter()
        .filter(|t| live_set.contains(t.tmux_session_name.as_str()))
        .cloned()
        .collect();
    for (i, tab) in tabs.iter_mut().enumerate() {
        tab.position = i;
    }
    Workspace {
        name: ws.name.clone(),
        uuid: ws.uuid.clone(),
        tabs,
        updated_at: ws.updated_at.clone(),
    }
}

/// Merge two versions of the same tab — newer `updated_at` wins.
fn merge_tab(local: &TabState, remote: &TabState) -> TabState {
    if !remote.updated_at.is_empty() && remote.updated_at > local.updated_at {
        // Remote is newer — use its metadata
        TabState {
            session_uuid: local.session_uuid.clone(),
            tmux_session_name: local.tmux_session_name.clone(), // tmux name is canonical
            title: remote.title.clone(),
            position: local.position, // will be reindexed
            server_window_id: remote.server_window_id.clone(),
            updated_at: remote.updated_at.clone(),
        }
    } else {
        // Local is newer (or equal/empty) — local wins
        local.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tab(uuid: &str, tmux: &str, title: &str, updated_at: &str) -> TabState {
        TabState {
            session_uuid: uuid.to_string(),
            tmux_session_name: tmux.to_string(),
            title: title.to_string(),
            position: 0,
            server_window_id: None,
            updated_at: updated_at.to_string(),
        }
    }

    fn make_workspace(name: &str, tabs: Vec<TabState>) -> Workspace {
        Workspace {
            name: name.to_string(),
            uuid: "ws-uuid".to_string(),
            tabs,
            updated_at: String::new(),
        }
    }

    fn make_state(workspaces: Vec<(&str, Workspace)>) -> SharedState {
        let mut state = SharedState::new();
        for (name, ws) in workspaces {
            state.workspaces.insert(name.to_string(), ws);
        }
        state
    }

    #[test]
    fn merge_union_tabs() {
        let local = make_state(vec![(
            "Default",
            make_workspace("Default", vec![make_tab("a", "sk-a", "Tab A", "")]),
        )]);
        let remote = make_state(vec![(
            "Default",
            make_workspace("Default", vec![make_tab("b", "sk-b", "Tab B", "")]),
        )]);
        let live = vec!["sk-a".to_string(), "sk-b".to_string()];

        let merged = merge_shared_states(&local, &remote, &live, "test");

        let ws = merged.workspaces.get("Default").unwrap();
        assert_eq!(ws.tabs.len(), 2);
        assert_eq!(ws.tabs[0].session_uuid, "a"); // local first
        assert_eq!(ws.tabs[1].session_uuid, "b"); // remote appended
    }

    #[test]
    fn merge_dead_tabs_removed() {
        let local = make_state(vec![(
            "Default",
            make_workspace(
                "Default",
                vec![
                    make_tab("a", "sk-a", "Tab A", ""),
                    make_tab("b", "sk-b", "Tab B", ""),
                ],
            ),
        )]);
        let remote = make_state(vec![(
            "Default",
            make_workspace("Default", vec![make_tab("a", "sk-a", "Tab A", "")]),
        )]);
        // sk-b is dead (not in live list)
        let live = vec!["sk-a".to_string()];

        let merged = merge_shared_states(&local, &remote, &live, "test");

        let ws = merged.workspaces.get("Default").unwrap();
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.tabs[0].session_uuid, "a");
    }

    #[test]
    fn merge_newer_title_wins() {
        let local = make_state(vec![(
            "Default",
            make_workspace(
                "Default",
                vec![make_tab("a", "sk-a", "Old Title", "2026-01-01T00:00:00Z")],
            ),
        )]);
        let remote = make_state(vec![(
            "Default",
            make_workspace(
                "Default",
                vec![make_tab("a", "sk-a", "New Title", "2026-01-02T00:00:00Z")],
            ),
        )]);
        let live = vec!["sk-a".to_string()];

        let merged = merge_shared_states(&local, &remote, &live, "test");

        let ws = merged.workspaces.get("Default").unwrap();
        assert_eq!(ws.tabs[0].title, "New Title");
        assert_eq!(ws.tabs[0].updated_at, "2026-01-02T00:00:00Z");
    }

    #[test]
    fn merge_local_wins_on_equal_timestamp() {
        let local = make_state(vec![(
            "Default",
            make_workspace(
                "Default",
                vec![make_tab("a", "sk-a", "Local Title", "2026-01-01T00:00:00Z")],
            ),
        )]);
        let remote = make_state(vec![(
            "Default",
            make_workspace(
                "Default",
                vec![make_tab(
                    "a",
                    "sk-a",
                    "Remote Title",
                    "2026-01-01T00:00:00Z",
                )],
            ),
        )]);
        let live = vec!["sk-a".to_string()];

        let merged = merge_shared_states(&local, &remote, &live, "test");

        let ws = merged.workspaces.get("Default").unwrap();
        assert_eq!(ws.tabs[0].title, "Local Title"); // local wins on tie
    }

    #[test]
    fn merge_preserves_version_uuid() {
        let local = make_state(vec![]);
        let remote = make_state(vec![]);
        let live: Vec<String> = vec![];

        let merged = merge_shared_states(&local, &remote, &live, "test");

        // version_uuid should be a fresh UUID
        assert!(!merged.version_uuid.is_empty());
        assert_ne!(merged.version_uuid, local.version_uuid);
        assert_ne!(merged.version_uuid, remote.version_uuid);
    }
}
