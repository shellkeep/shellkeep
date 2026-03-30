// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Error classification for SSH connection failures.
//! FR-RECONNECT-07: distinguish transient vs permanent errors.

/// Transient errors that should trigger auto-retry.
pub fn is_transient(error: &str) -> bool {
    let transient_patterns = [
        "timeout",
        "timed out",
        "connection reset",
        "connection refused",
        "network unreachable",
        "no route to host",
        "broken pipe",
        "connection closed",
        "channel closed",
        "write error",
    ];
    let lower = error.to_lowercase();
    transient_patterns.iter().any(|p| lower.contains(p))
}

/// Permanent errors that should NOT auto-retry.
pub fn is_permanent(error: &str) -> bool {
    let permanent_patterns = [
        "auth",
        "authentication",
        "permission denied",
        "host key",
        "protocol error",
        "key exchange",
        "no matching",
        "session exited",
    ];
    let lower = error.to_lowercase();
    permanent_patterns.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_errors() {
        assert!(is_transient("connection reset by peer"));
        assert!(is_transient("Connection timed out"));
        assert!(is_transient("write error: broken pipe"));
        assert!(is_transient("channel closed"));
        assert!(!is_transient("auth failed: invalid key"));
    }

    #[test]
    fn permanent_errors() {
        assert!(is_permanent("auth failed: invalid key"));
        assert!(is_permanent("Permission denied (publickey)"));
        assert!(is_permanent("host key verification failed"));
        assert!(is_permanent("session exited"));
        assert!(!is_permanent("connection reset"));
        assert!(!is_permanent("channel closed"));
    }
}
