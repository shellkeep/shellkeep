// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! FR-ENV-01: workspace management — CRUD operations for named groups of windows/sessions per server.

use super::state_file::{Environment, SharedState};
use crate::error::StateError;

/// FR-ENV-01: create a new empty environment.
pub fn create_environment(state: &mut SharedState, name: &str) -> Result<(), StateError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StateError::Validation(
            "environment name cannot be empty".to_string(),
        ));
    }
    if state.environments.contains_key(name) {
        return Err(StateError::Validation(format!(
            "environment '{name}' already exists"
        )));
    }
    state.environments.insert(
        name.to_string(),
        Environment {
            name: name.to_string(),
            tabs: Vec::new(),
        },
    );
    Ok(())
}

/// FR-ENV-01: delete an environment. Cannot delete the last remaining environment.
pub fn delete_environment(state: &mut SharedState, name: &str) -> Result<(), StateError> {
    if !state.environments.contains_key(name) {
        return Err(StateError::Validation(format!(
            "environment '{name}' does not exist"
        )));
    }
    if state.environments.len() <= 1 {
        return Err(StateError::Validation(
            "cannot delete the last environment".to_string(),
        ));
    }
    state.environments.remove(name);
    // If the deleted environment was the last active, switch to another
    if state.last_environment.as_deref() == Some(name) {
        state.last_environment = state.environments.keys().next().cloned();
    }
    Ok(())
}

/// FR-ENV-01: rename an environment.
pub fn rename_environment(state: &mut SharedState, old: &str, new: &str) -> Result<(), StateError> {
    let new = new.trim();
    if new.is_empty() {
        return Err(StateError::Validation(
            "environment name cannot be empty".to_string(),
        ));
    }
    if !state.environments.contains_key(old) {
        return Err(StateError::Validation(format!(
            "environment '{old}' does not exist"
        )));
    }
    if old != new && state.environments.contains_key(new) {
        return Err(StateError::Validation(format!(
            "environment '{new}' already exists"
        )));
    }
    if let Some(mut env) = state.environments.remove(old) {
        env.name = new.to_string();
        state.environments.insert(new.to_string(), env);
        if state.last_environment.as_deref() == Some(old) {
            state.last_environment = Some(new.to_string());
        }
    }
    Ok(())
}

/// FR-ENV-01: list all environment names, sorted alphabetically.
pub fn list_environments(state: &SharedState) -> Vec<String> {
    let mut names: Vec<String> = state.environments.keys().cloned().collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> SharedState {
        let mut state = SharedState::new();
        create_environment(&mut state, "Default").unwrap();
        state
    }

    #[test]
    fn create_and_list() {
        let mut state = make_state();
        create_environment(&mut state, "Project A").unwrap();
        create_environment(&mut state, "Project B").unwrap();
        assert_eq!(
            list_environments(&state),
            vec!["Default", "Project A", "Project B"]
        );
    }

    #[test]
    fn create_duplicate_fails() {
        let mut state = make_state();
        let err = create_environment(&mut state, "Default").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn create_empty_name_fails() {
        let mut state = make_state();
        let err = create_environment(&mut state, "  ").unwrap_err();
        assert!(err.to_string().contains("cannot be empty"));
    }

    #[test]
    fn delete_environment_works() {
        let mut state = make_state();
        create_environment(&mut state, "Temp").unwrap();
        delete_environment(&mut state, "Temp").unwrap();
        assert_eq!(list_environments(&state), vec!["Default"]);
    }

    #[test]
    fn delete_last_environment_fails() {
        let mut state = make_state();
        let err = delete_environment(&mut state, "Default").unwrap_err();
        assert!(err.to_string().contains("cannot delete the last"));
    }

    #[test]
    fn delete_nonexistent_fails() {
        let mut state = make_state();
        let err = delete_environment(&mut state, "nope").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn delete_active_switches() {
        let mut state = make_state();
        state.last_environment = Some("Default".to_string());
        create_environment(&mut state, "Other").unwrap();
        delete_environment(&mut state, "Default").unwrap();
        assert!(state.last_environment.is_some());
        assert_ne!(state.last_environment.as_deref(), Some("Default"));
    }

    #[test]
    fn rename_environment_works() {
        let mut state = make_state();
        state.last_environment = Some("Default".to_string());
        rename_environment(&mut state, "Default", "Main").unwrap();
        assert_eq!(list_environments(&state), vec!["Main"]);
        assert_eq!(state.last_environment, Some("Main".to_string()));
    }

    #[test]
    fn rename_to_existing_fails() {
        let mut state = make_state();
        create_environment(&mut state, "Other").unwrap();
        let err = rename_environment(&mut state, "Default", "Other").unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn rename_nonexistent_fails() {
        let mut state = make_state();
        let err = rename_environment(&mut state, "nope", "new").unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn rename_same_name_is_noop() {
        let mut state = make_state();
        rename_environment(&mut state, "Default", "Default").unwrap();
        assert_eq!(list_environments(&state), vec!["Default"]);
    }
}
