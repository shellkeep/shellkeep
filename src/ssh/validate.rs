// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Input validation for session and environment names.
//!
//! NFR-SEC-05: Reject tmux-incompatible chars and path traversal.
//! NFR-SEC-06: UUID validation for file paths.

/// Validate a tmux session/environment name.
/// Rejects: `:`, `.`, `/`, `\`, `..`, null bytes, control chars.
pub fn is_valid_session_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 128 {
        return false;
    }
    if name.contains(':') || name.contains('/') || name.contains('\\') || name.contains('\0') {
        return false;
    }
    if name.contains("..") {
        return false;
    }
    if name.chars().any(|c| c.is_control()) {
        return false;
    }
    true
}

/// Validate a UUID format: [a-f0-9-] only.
pub fn is_valid_uuid(uuid: &str) -> bool {
    !uuid.is_empty() && uuid.len() <= 128 && uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_session_names() {
        assert!(is_valid_session_name("shellkeep-0"));
        assert!(is_valid_session_name("my-session_123"));
        assert!(is_valid_session_name("production"));
    }

    #[test]
    fn invalid_session_names() {
        assert!(!is_valid_session_name(""));
        assert!(!is_valid_session_name("has:colon"));
        assert!(!is_valid_session_name("has/slash"));
        assert!(!is_valid_session_name("has\\backslash"));
        assert!(!is_valid_session_name("has..dots"));
        assert!(!is_valid_session_name("has\0null"));
        assert!(!is_valid_session_name("has\ttab"));
    }

    #[test]
    fn valid_uuids() {
        assert!(is_valid_uuid("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_uuid("abc-0-def"));
        assert!(is_valid_uuid("abc123"));
    }

    #[test]
    fn invalid_uuids() {
        assert!(!is_valid_uuid(""));
        assert!(!is_valid_uuid("has/slash"));
        assert!(!is_valid_uuid("has space"));
    }
}
