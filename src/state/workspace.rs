// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! FR-ENV-01: workspace management — CRUD operations for named groups of windows/sessions per server.

use super::state_file::{SharedState, Workspace};
use crate::error::StateError;

/// FR-ENV-01: create a new empty workspace.
pub fn create_workspace(state: &mut SharedState, name: &str) -> Result<(), StateError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StateError::Validation(
            "workspace name cannot be empty".to_string(),
        ));
    }
    if state.workspaces.contains_key(name) {
        return Err(StateError::Validation(format!(
            "workspace '{name}' already exists"
        )));
    }
    state.workspaces.insert(
        name.to_string(),
        Workspace {
            name: name.to_string(),
            uuid: uuid::Uuid::new_v4().to_string(),
            tabs: Vec::new(),
        },
    );
    Ok(())
}

/// FR-ENV-01: delete a workspace. Cannot delete the last remaining workspace.
pub fn delete_workspace(state: &mut SharedState, name: &str) -> Result<(), StateError> {
    if !state.workspaces.contains_key(name) {
        return Err(StateError::Validation(format!(
            "workspace '{name}' does not exist"
        )));
    }
    if state.workspaces.len() <= 1 {
        return Err(StateError::Validation(
            "cannot delete the last workspace".to_string(),
        ));
    }
    state.workspaces.remove(name);
    // If the deleted workspace was the last active, switch to another
    if state.last_workspace.as_deref() == Some(name) {
        state.last_workspace = state.workspaces.keys().next().cloned();
    }
    Ok(())
}

/// FR-ENV-01: rename a workspace.
pub fn rename_workspace(state: &mut SharedState, old: &str, new: &str) -> Result<(), StateError> {
    let new = new.trim();
    if new.is_empty() {
        return Err(StateError::Validation(
            "workspace name cannot be empty".to_string(),
        ));
    }
    if !state.workspaces.contains_key(old) {
        return Err(StateError::Validation(format!(
            "workspace '{old}' does not exist"
        )));
    }
    if old != new && state.workspaces.contains_key(new) {
        return Err(StateError::Validation(format!(
            "workspace '{new}' already exists"
        )));
    }
    if let Some(mut ws) = state.workspaces.remove(old) {
        ws.name = new.to_string();
        state.workspaces.insert(new.to_string(), ws);
        if state.last_workspace.as_deref() == Some(old) {
            state.last_workspace = Some(new.to_string());
        }
    }
    Ok(())
}

/// FR-ENV-01: list all workspace names, sorted alphabetically.
pub fn list_workspaces(state: &SharedState) -> Vec<String> {
    let mut names: Vec<String> = state.workspaces.keys().cloned().collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> SharedState {
        let mut state = SharedState::new();
        create_workspace(&mut state, "Default").unwrap();
        state
    }

    #[test]
    fn create_and_list() {
        let mut state = make_state();
        create_workspace(&mut state, "Project A").unwrap();
        create_workspace(&mut state, "Project B").unwrap();
        assert_eq!(
            list_workspaces(&state),
            vec!["Default", "Project A", "Project B"]
        );
    }

    #[test]
    fn create_duplicate_fails() {
        let mut state = make_state();
        let err = create_workspace(&mut state, "Default").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn create_empty_name_fails() {
        let mut state = make_state();
        let err = create_workspace(&mut state, "  ").unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[test]
    fn delete_workspace_works() {
        let mut state = make_state();
        create_workspace(&mut state, "Temp").unwrap();
        delete_workspace(&mut state, "Temp").unwrap();
        assert_eq!(list_workspaces(&state), vec!["Default"]);
    }

    #[test]
    fn delete_last_workspace_fails() {
        let mut state = make_state();
        let err = delete_workspace(&mut state, "Default").unwrap_err();
        assert!(err.to_string().contains("cannot delete the last"));
    }

    #[test]
    fn delete_nonexistent_fails() {
        let mut state = make_state();
        let err = delete_workspace(&mut state, "nope").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn delete_active_switches() {
        let mut state = make_state();
        state.last_workspace = Some("Default".to_string());
        create_workspace(&mut state, "Other").unwrap();
        delete_workspace(&mut state, "Default").unwrap();
        assert!(state.last_workspace.is_some());
        assert_ne!(state.last_workspace.as_deref(), Some("Default"));
    }

    #[test]
    fn rename_workspace_works() {
        let mut state = make_state();
        state.last_workspace = Some("Default".to_string());
        rename_workspace(&mut state, "Default", "Main").unwrap();
        assert_eq!(list_workspaces(&state), vec!["Main"]);
        assert_eq!(state.last_workspace, Some("Main".to_string()));
    }

    #[test]
    fn rename_to_existing_fails() {
        let mut state = make_state();
        create_workspace(&mut state, "Other").unwrap();
        let err = rename_workspace(&mut state, "Default", "Other").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn rename_nonexistent_fails() {
        let mut state = make_state();
        let err = rename_workspace(&mut state, "nope", "new").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn rename_same_name_is_noop() {
        let mut state = make_state();
        rename_workspace(&mut state, "Default", "Default").unwrap();
        assert_eq!(list_workspaces(&state), vec!["Default"]);
    }
}
