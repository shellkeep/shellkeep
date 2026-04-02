// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Client-ID lock mechanism using tmux sessions.
//! FR-LOCK-01..11: prevents multiple clients from managing the same sessions.

use super::connection::{self, SshError, SshHandler};

/// Information about who holds a lock.
#[derive(Debug, Clone)]
#[must_use]
pub struct LockInfo {
    pub client_id: String,
    pub hostname: String,
    pub connected_at: String, // ISO 8601
    pub pid: u32,
    pub version: String,
}

/// Default orphan timeout multiplier: lock is orphaned if
/// connected_at + (2 * keepalive_timeout) < now.
const LOCK_ORPHAN_MULTIPLIER: u64 = 2;

/// Default keepalive timeout in seconds.
const DEFAULT_KEEPALIVE_TIMEOUT: u64 = 30;

/// Build the per-workspace lock session name. /* FR-LOCK-02 */
/// Sanitizes the workspace name for use as a tmux session name.
fn lock_session_name(workspace: &str) -> String {
    let sanitized: String = workspace
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("shellkeep-lock-{sanitized}")
}

/// Legacy lock session name (pre-v0.3, single lock per server).
const LEGACY_LOCK_NAME: &str = "shellkeep-lock";

/// Check if a lock exists on this server for the given workspace. /* FR-LOCK-02 */
/// Returns Some(LockInfo) if locked, None if not.
pub async fn check_lock(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
    workspace: &str,
) -> Result<Option<LockInfo>, SshError> {
    let lock_name = lock_session_name(workspace);

    // tmux has-session exits 0 if session exists, 1 if not.
    // We wrap it in a shell check to get the exit status.
    let check_cmd = format!(
        "tmux has-session -t {lock_name} 2>/dev/null && echo LOCK_EXISTS || echo LOCK_NONE"
    );
    let result = connection::exec_command(handle, &check_cmd).await?;

    if !result.trim().contains("LOCK_EXISTS") {
        return Ok(None);
    }

    // Lock exists — read environment variables
    let env_cmd = format!("tmux show-environment -t {lock_name} 2>/dev/null");
    let env_output = connection::exec_command(handle, &env_cmd).await?;

    Ok(Some(parse_lock_env(&env_output, client_id)))
}

/// Parse tmux environment variables into LockInfo.
fn parse_lock_env(env_output: &str, fallback_client_id: &str) -> LockInfo {
    let mut info = LockInfo {
        client_id: fallback_client_id.to_string(),
        hostname: String::new(),
        connected_at: String::new(),
        pid: 0,
        version: String::new(),
    };

    for line in env_output.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_CLIENT_ID=") {
            info.client_id = val.to_string();
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_HOSTNAME=") {
            info.hostname = val.to_string();
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_CONNECTED_AT=") {
            info.connected_at = val.to_string();
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_PID=") {
            info.pid = val.parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_VERSION=") {
            info.version = val.to_string();
        }
    }

    info
}

/// Acquire the lock for a workspace. Returns Err if lock already held by another client. /* FR-LOCK-01 */
pub async fn acquire_lock(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
    keepalive_timeout: Option<u64>,
    workspace: &str,
) -> Result<(), SshError> {
    let lock_name = lock_session_name(workspace);
    let hostname = whoami::fallible::hostname().unwrap_or_else(|_| "unknown".to_string());
    let pid = std::process::id();
    let version = env!("CARGO_PKG_VERSION");
    let now = chrono::Utc::now().to_rfc3339();

    // Check if lock already exists
    if let Some(existing) = check_lock(handle, client_id, workspace).await? {
        // FR-LOCK-06: same host + PID → renew silently
        if existing.hostname == hostname && existing.pid == pid {
            return set_lock_env(handle, &lock_name, client_id, &hostname, pid, version, &now)
                .await;
        }

        // FR-LOCK-07: orphan detection
        let timeout = keepalive_timeout.unwrap_or(DEFAULT_KEEPALIVE_TIMEOUT);
        if is_orphaned(&existing, timeout) {
            tracing::info!(
                "lock held by {}@{} is orphaned (last heartbeat: {}), taking over",
                existing.client_id,
                existing.hostname,
                existing.connected_at
            );
            release_lock(handle, workspace).await?;
            // Fall through to create new lock
        } else {
            return Err(SshError::Channel(format!(
                "session locked by {} (connected from {}, pid {}, since {})",
                existing.client_id, existing.hostname, existing.pid, existing.connected_at
            )));
        }
    }

    // Create lock session atomically — tmux new-session fails if it already exists
    let create_cmd =
        format!("tmux new-session -d -s {lock_name} 2>&1 && echo LOCK_OK || echo LOCK_FAIL");
    let result = connection::exec_command(handle, &create_cmd).await?;

    if !result.contains("LOCK_OK") {
        return Err(SshError::Channel(format!(
            "failed to create lock session: {result}"
        )));
    }

    set_lock_env(handle, &lock_name, client_id, &hostname, pid, version, &now).await
}

/// Set lock environment variables on the tmux session.
async fn set_lock_env(
    handle: &russh::client::Handle<SshHandler>,
    lock_name: &str,
    client_id: &str,
    hostname: &str,
    pid: u32,
    version: &str,
    connected_at: &str,
) -> Result<(), SshError> {
    let cmd = format!(
        "tmux set-environment -t {lock_name} SHELLKEEP_LOCK_CLIENT_ID '{client_id}' && \
         tmux set-environment -t {lock_name} SHELLKEEP_LOCK_HOSTNAME '{hostname}' && \
         tmux set-environment -t {lock_name} SHELLKEEP_LOCK_CONNECTED_AT '{connected_at}' && \
         tmux set-environment -t {lock_name} SHELLKEEP_LOCK_PID '{pid}' && \
         tmux set-environment -t {lock_name} SHELLKEEP_LOCK_VERSION '{version}'"
    );
    connection::exec_command(handle, &cmd).await?;
    Ok(())
}

/// Update heartbeat timestamp for a workspace lock. /* FR-LOCK-04 */
pub async fn heartbeat(
    handle: &russh::client::Handle<SshHandler>,
    workspace: &str,
) -> Result<(), SshError> {
    let lock_name = lock_session_name(workspace);
    let now = chrono::Utc::now().to_rfc3339();
    let cmd = format!("tmux set-environment -t {lock_name} SHELLKEEP_LOCK_CONNECTED_AT '{now}'");
    connection::exec_command(handle, &cmd).await?;
    Ok(())
}

/// Release the lock for a workspace. /* FR-LOCK-05 */
pub async fn release_lock(
    handle: &russh::client::Handle<SshHandler>,
    workspace: &str,
) -> Result<(), SshError> {
    let lock_name = lock_session_name(workspace);
    let cmd = format!("tmux kill-session -t {lock_name} 2>/dev/null || true");
    connection::exec_command(handle, &cmd).await?;
    Ok(())
}

/// Clean up the legacy lock session (pre-v0.3 "shellkeep-lock") if it exists
/// and belongs to this client.
pub async fn cleanup_legacy_lock(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
) -> Result<(), SshError> {
    let check_cmd = format!(
        "tmux has-session -t {LEGACY_LOCK_NAME} 2>/dev/null && echo LOCK_EXISTS || echo LOCK_NONE"
    );
    let result = connection::exec_command(handle, &check_cmd).await?;
    if !result.trim().contains("LOCK_EXISTS") {
        return Ok(());
    }
    // Read the lock owner
    let env_cmd = format!("tmux show-environment -t {LEGACY_LOCK_NAME} 2>/dev/null");
    let env_output = connection::exec_command(handle, &env_cmd).await?;
    let info = parse_lock_env(&env_output, "");
    // Only kill if it belongs to this client (or is orphaned)
    if info.client_id == client_id || is_orphaned(&info, DEFAULT_KEEPALIVE_TIMEOUT) {
        let kill_cmd = format!("tmux kill-session -t {LEGACY_LOCK_NAME} 2>/dev/null || true");
        connection::exec_command(handle, &kill_cmd).await?;
        tracing::info!("cleaned up legacy lock session '{LEGACY_LOCK_NAME}'");
    }
    Ok(())
}

/// Check if a lock is orphaned. /* FR-LOCK-07 */
fn is_orphaned(info: &LockInfo, keepalive_timeout: u64) -> bool {
    if info.connected_at.is_empty() {
        return true;
    }

    let connected_at = match chrono::DateTime::parse_from_rfc3339(&info.connected_at) {
        Ok(dt) => dt,
        Err(_) => return true, // Can't parse timestamp — treat as orphaned
    };

    let threshold = chrono::Duration::seconds((LOCK_ORPHAN_MULTIPLIER * keepalive_timeout) as i64);
    let now = chrono::Utc::now();
    now.signed_duration_since(connected_at) > threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_session_name_default() {
        assert_eq!(lock_session_name("Default"), "shellkeep-lock-Default");
    }

    #[test]
    fn lock_session_name_sanitizes_spaces() {
        assert_eq!(lock_session_name("My Project"), "shellkeep-lock-My-Project");
    }

    #[test]
    fn lock_session_name_sanitizes_special_chars() {
        assert_eq!(
            lock_session_name("test/foo@bar"),
            "shellkeep-lock-test-foo-bar"
        );
    }

    #[test]
    fn lock_session_name_preserves_valid_chars() {
        assert_eq!(
            lock_session_name("ESSR-2024_prod"),
            "shellkeep-lock-ESSR-2024_prod"
        );
    }

    #[test]
    fn legacy_lock_name_constant() {
        assert_eq!(LEGACY_LOCK_NAME, "shellkeep-lock");
    }

    #[test]
    fn parse_env_complete() {
        let env = "\
SHELLKEEP_LOCK_CLIENT_ID=my-laptop
SHELLKEEP_LOCK_HOSTNAME=workstation.local
SHELLKEEP_LOCK_CONNECTED_AT=2026-03-29T10:00:00+00:00
SHELLKEEP_LOCK_PID=12345
SHELLKEEP_LOCK_VERSION=0.3.0
";
        let info = parse_lock_env(env, "fallback");
        assert_eq!(info.client_id, "my-laptop");
        assert_eq!(info.hostname, "workstation.local");
        assert_eq!(info.connected_at, "2026-03-29T10:00:00+00:00");
        assert_eq!(info.pid, 12345);
        assert_eq!(info.version, "0.3.0");
    }

    #[test]
    fn parse_env_partial() {
        let env = "SHELLKEEP_LOCK_HOSTNAME=server1\nSOME_OTHER_VAR=foo\n";
        let info = parse_lock_env(env, "default-id");
        assert_eq!(info.client_id, "default-id");
        assert_eq!(info.hostname, "server1");
        assert_eq!(info.pid, 0);
    }

    #[test]
    fn parse_env_empty() {
        let info = parse_lock_env("", "fallback");
        assert_eq!(info.client_id, "fallback");
        assert_eq!(info.hostname, "");
        assert_eq!(info.pid, 0);
    }

    #[test]
    fn orphan_detection_expired() {
        let info = LockInfo {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: "2020-01-01T00:00:00+00:00".to_string(),
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(is_orphaned(&info, 30));
    }

    #[test]
    fn orphan_detection_fresh() {
        let now = chrono::Utc::now().to_rfc3339();
        let info = LockInfo {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: now,
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(!is_orphaned(&info, 30));
    }

    #[test]
    fn orphan_detection_empty_timestamp() {
        let info = LockInfo {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: String::new(),
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(is_orphaned(&info, 30));
    }

    #[test]
    fn orphan_detection_invalid_timestamp() {
        let info = LockInfo {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: "not-a-date".to_string(),
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(is_orphaned(&info, 30));
    }
}
