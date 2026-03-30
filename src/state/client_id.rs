// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Client ID resolution and persistence.
//!
//! FR-CONFIG-08: Priority order:
//! 1. Config file ([general] client_id)
//! 2. Persisted file ($XDG_CONFIG_HOME/shellkeep/client-id)
//! 3. Auto-generate from username + hostname

use std::fs;
use std::path::PathBuf;

use crate::error::StateError;

/// Resolve the client ID.
pub fn resolve(config_client_id: Option<&str>) -> String {
    // 1. Config file
    if let Some(id) = config_client_id
        && is_valid(id)
    {
        return id.to_string();
    }

    // 2. Persisted file
    let path = file_path();
    if let Ok(id) = fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if is_valid(&id) {
            return id;
        }
    }

    // 3. Auto-generate
    let hostname = whoami::fallible::hostname().unwrap_or_else(|_| "unknown".into());
    let id = format!("{}-{}", whoami::username(), hostname);

    // Persist for next time
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, &id);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    id
}

/// Validate client ID format: [a-zA-Z0-9_-], max 64 chars.
pub fn is_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Save a client ID to the persistent file.
/// FR-UI-03: allows user to set a friendly client name on first use.
pub fn save_client_id(id: &str) -> Result<(), StateError> {
    if !is_valid(id) {
        return Err(StateError::Validation(format!("invalid client-id: {id}")));
    }
    let path = file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, id)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn file_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shellkeep")
        .join("client-id")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ids() {
        assert!(is_valid("work-laptop"));
        assert!(is_valid("user_host123"));
        assert!(is_valid("a"));
    }

    #[test]
    fn invalid_ids() {
        assert!(!is_valid(""));
        assert!(!is_valid("has space"));
        assert!(!is_valid("has/slash"));
        assert!(!is_valid("has.dot"));
        assert!(!is_valid(&"x".repeat(65)));
    }
}
