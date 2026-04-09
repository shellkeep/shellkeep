// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-device workspace tracker using tmux sessions.
//! FR-LOCK-01..11: tracks connected devices per workspace, allows simultaneous connections.
//!
//! The tmux session `shellkeep-lock-{workspace}` stores a JSON array of connected devices
//! in the `SHELLKEEP_LOCK_DEVICES` environment variable. Multiple devices can connect to
//! the same workspace simultaneously. State consistency is handled by merge-on-flush
//! (FR-STATE-20) and the state watcher (FR-STATE-21).

use super::connection::{self, SshError, SshHandler};

/// Information about a connected device.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectedDevice {
    pub client_id: String,
    pub hostname: String,
    pub connected_at: String, // ISO 8601
    pub pid: u32,
    pub version: String,
}

/// Orphan timeout multiplier: device is orphaned if
/// connected_at + (3 * keepalive_timeout) < now.
/// Set to 3 (not 2) to tolerate one missed heartbeat without pruning.
const LOCK_ORPHAN_MULTIPLIER: u64 = 3;

/// Default keepalive timeout in seconds.
const DEFAULT_KEEPALIVE_TIMEOUT: u64 = 30;

/// Build the per-workspace lock session name. /* FR-LOCK-02 */
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

/// Join a workspace: add this device to the connected devices list.
/// Returns the list of OTHER connected devices (for toast notification).
/// Creates the tmux lock session if it doesn't exist.
pub async fn join_workspace(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
    keepalive_timeout: Option<u64>,
    workspace: &str,
) -> Result<Vec<ConnectedDevice>, SshError> {
    let lock_name = lock_session_name(workspace);
    let hostname = whoami::fallible::hostname().unwrap_or_else(|_| "unknown".to_string());
    let pid = std::process::id();
    let version = env!("CARGO_PKG_VERSION");
    let now = chrono::Utc::now().to_rfc3339();
    let timeout = keepalive_timeout.unwrap_or(DEFAULT_KEEPALIVE_TIMEOUT);

    // Ensure lock session exists
    let check_cmd =
        format!("tmux has-session -t {lock_name} 2>/dev/null && echo EXISTS || echo NONE");
    let result = connection::exec_command(handle, &check_cmd).await?;
    if result.trim().contains("NONE") {
        let create_cmd = format!("tmux new-session -d -s {lock_name} 2>&1 && echo OK || echo FAIL");
        let create_result = connection::exec_command(handle, &create_cmd).await?;
        if !create_result.contains("OK") {
            tracing::warn!("failed to create lock session: {create_result}");
        }
    }

    // Read-modify-write device list. Note: not atomic — if two devices join
    // simultaneously, one entry may be lost. The heartbeat self-heals this by
    // re-adding the device if its entry is missing (see heartbeat() below).
    let mut devices = read_devices(handle, &lock_name).await;

    // Prune orphaned entries
    devices.retain(|d| !is_device_orphaned(d, timeout));

    // Remove stale entry for our client_id (crash recovery)
    devices.retain(|d| d.client_id != client_id);

    // Collect other devices for toast
    let others: Vec<ConnectedDevice> = devices.clone();

    if !others.is_empty() {
        tracing::info!(
            "workspace {workspace}: {} other device(s) connected: {:?}",
            others.len(),
            others.iter().map(|d| &d.client_id).collect::<Vec<_>>()
        );
    }

    // Add ourselves
    devices.push(ConnectedDevice {
        client_id: client_id.to_string(),
        hostname,
        connected_at: now,
        pid,
        version: version.to_string(),
    });

    // Write back
    write_devices(handle, &lock_name, &devices).await?;

    Ok(others)
}

/// Leave a workspace: remove this device from the connected devices list.
/// Destroys the tmux lock session if no devices remain.
pub async fn leave_workspace(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
    workspace: &str,
) -> Result<(), SshError> {
    let lock_name = lock_session_name(workspace);

    let mut devices = read_devices(handle, &lock_name).await;
    devices.retain(|d| d.client_id != client_id);

    if devices.is_empty() {
        // Last device — kill the session
        let cmd = format!("tmux kill-session -t {lock_name} 2>/dev/null || true");
        connection::exec_command(handle, &cmd).await?;
        tracing::info!("workspace {workspace}: last device left, lock session destroyed");
    } else {
        write_devices(handle, &lock_name, &devices).await?;
        tracing::info!(
            "workspace {workspace}: left ({} device(s) remaining)",
            devices.len()
        );
    }

    Ok(())
}

/// Update heartbeat timestamp for this device. /* FR-LOCK-09 */
pub async fn heartbeat(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
    workspace: &str,
) -> Result<(), SshError> {
    let lock_name = lock_session_name(workspace);
    let now = chrono::Utc::now().to_rfc3339();

    let mut devices = read_devices(handle, &lock_name).await;
    if let Some(entry) = devices.iter_mut().find(|d| d.client_id == client_id) {
        entry.connected_at = now;
        write_devices(handle, &lock_name, &devices).await?;
    } else {
        tracing::warn!("heartbeat: our client_id not found in device list, re-joining");
        // Re-add ourselves (might have been pruned as orphan)
        let hostname = whoami::fallible::hostname().unwrap_or_else(|_| "unknown".to_string());
        devices.push(ConnectedDevice {
            client_id: client_id.to_string(),
            hostname,
            connected_at: now,
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        });
        write_devices(handle, &lock_name, &devices).await?;
    }

    Ok(())
}

/// List connected devices for a workspace. Prunes orphaned entries.
pub async fn list_devices(
    handle: &russh::client::Handle<SshHandler>,
    keepalive_timeout: Option<u64>,
    workspace: &str,
) -> Vec<ConnectedDevice> {
    let lock_name = lock_session_name(workspace);
    let timeout = keepalive_timeout.unwrap_or(DEFAULT_KEEPALIVE_TIMEOUT);
    let mut devices = read_devices(handle, &lock_name).await;
    devices.retain(|d| !is_device_orphaned(d, timeout));
    devices
}

/// Release the lock for a workspace (backward compat wrapper). /* FR-LOCK-10 */
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
    // Read the lock owner — try new format first, fall back to legacy
    let env_cmd = format!("tmux show-environment -t {LEGACY_LOCK_NAME} 2>/dev/null");
    let env_output = connection::exec_command(handle, &env_cmd).await?;
    let devices = parse_legacy_lock_env(&env_output);
    let is_ours = devices.iter().any(|d| d.client_id == client_id);
    let all_orphaned = devices
        .iter()
        .all(|d| is_device_orphaned(d, DEFAULT_KEEPALIVE_TIMEOUT));
    if is_ours || all_orphaned || devices.is_empty() {
        let kill_cmd = format!("tmux kill-session -t {LEGACY_LOCK_NAME} 2>/dev/null || true");
        connection::exec_command(handle, &kill_cmd).await?;
        tracing::info!("cleaned up legacy lock session '{LEGACY_LOCK_NAME}'");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Read the connected devices list from tmux env var.
async fn read_devices(
    handle: &russh::client::Handle<SshHandler>,
    lock_name: &str,
) -> Vec<ConnectedDevice> {
    let cmd = format!("tmux show-environment -t {lock_name} SHELLKEEP_LOCK_DEVICES 2>/dev/null");
    let output = match connection::exec_command(handle, &cmd).await {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    // Parse SHELLKEEP_LOCK_DEVICES=<json>
    let json_str = output
        .trim()
        .strip_prefix("SHELLKEEP_LOCK_DEVICES=")
        .unwrap_or("");

    if json_str.is_empty() {
        // Try legacy single-device format
        let env_cmd = format!("tmux show-environment -t {lock_name} 2>/dev/null");
        if let Ok(env_output) = connection::exec_command(handle, &env_cmd).await {
            let legacy = parse_legacy_lock_env(&env_output);
            if !legacy.is_empty() {
                return legacy;
            }
        }
        return Vec::new();
    }

    serde_json::from_str(json_str).unwrap_or_default()
}

/// Write the connected devices list to tmux env var.
async fn write_devices(
    handle: &russh::client::Handle<SshHandler>,
    lock_name: &str,
    devices: &[ConnectedDevice],
) -> Result<(), SshError> {
    let json = serde_json::to_string(devices).unwrap_or_else(|_| "[]".to_string());
    // Escape single quotes for safe shell embedding: ' → '\''
    let escaped = json.replace('\'', "'\\''");
    let cmd = format!("tmux set-environment -t {lock_name} SHELLKEEP_LOCK_DEVICES '{escaped}'");
    connection::exec_command(handle, &cmd).await?;
    Ok(())
}

/// Parse legacy single-device lock env vars into a device list.
fn parse_legacy_lock_env(env_output: &str) -> Vec<ConnectedDevice> {
    let mut device = ConnectedDevice {
        client_id: String::new(),
        hostname: String::new(),
        connected_at: String::new(),
        pid: 0,
        version: String::new(),
    };

    for line in env_output.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_CLIENT_ID=") {
            device.client_id = val.to_string();
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_HOSTNAME=") {
            device.hostname = val.to_string();
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_CONNECTED_AT=") {
            device.connected_at = val.to_string();
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_PID=") {
            device.pid = val.parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("SHELLKEEP_LOCK_VERSION=") {
            device.version = val.to_string();
        }
    }

    if device.client_id.is_empty() {
        Vec::new()
    } else {
        vec![device]
    }
}

/// Check if a device entry is orphaned. /* FR-LOCK-07 */
fn is_device_orphaned(device: &ConnectedDevice, keepalive_timeout: u64) -> bool {
    if device.connected_at.is_empty() {
        return true;
    }

    let connected_at = match chrono::DateTime::parse_from_rfc3339(&device.connected_at) {
        Ok(dt) => dt,
        Err(_) => return true,
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
    fn parse_legacy_env_complete() {
        let env = "\
SHELLKEEP_LOCK_CLIENT_ID=my-laptop
SHELLKEEP_LOCK_HOSTNAME=workstation.local
SHELLKEEP_LOCK_CONNECTED_AT=2026-03-29T10:00:00+00:00
SHELLKEEP_LOCK_PID=12345
SHELLKEEP_LOCK_VERSION=0.3.0
";
        let devices = parse_legacy_lock_env(env);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].client_id, "my-laptop");
        assert_eq!(devices[0].hostname, "workstation.local");
        assert_eq!(devices[0].connected_at, "2026-03-29T10:00:00+00:00");
        assert_eq!(devices[0].pid, 12345);
        assert_eq!(devices[0].version, "0.3.0");
    }

    #[test]
    fn parse_legacy_env_partial() {
        let env = "SHELLKEEP_LOCK_HOSTNAME=server1\nSOME_OTHER_VAR=foo\n";
        let devices = parse_legacy_lock_env(env);
        assert!(devices.is_empty()); // no client_id → empty
    }

    #[test]
    fn parse_legacy_env_empty() {
        let devices = parse_legacy_lock_env("");
        assert!(devices.is_empty());
    }

    #[test]
    fn orphan_detection_expired() {
        let device = ConnectedDevice {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: "2020-01-01T00:00:00+00:00".to_string(),
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(is_device_orphaned(&device, 30));
    }

    #[test]
    fn orphan_detection_fresh() {
        let now = chrono::Utc::now().to_rfc3339();
        let device = ConnectedDevice {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: now,
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(!is_device_orphaned(&device, 30));
    }

    #[test]
    fn orphan_detection_empty_timestamp() {
        let device = ConnectedDevice {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: String::new(),
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(is_device_orphaned(&device, 30));
    }

    #[test]
    fn orphan_detection_invalid_timestamp() {
        let device = ConnectedDevice {
            client_id: "test".to_string(),
            hostname: "host".to_string(),
            connected_at: "not-a-date".to_string(),
            pid: 1,
            version: "0.1.0".to_string(),
        };
        assert!(is_device_orphaned(&device, 30));
    }

    #[test]
    fn json_device_roundtrip() {
        let devices = vec![
            ConnectedDevice {
                client_id: "laptop".to_string(),
                hostname: "macbook".to_string(),
                connected_at: "2026-04-08T10:00:00Z".to_string(),
                pid: 1234,
                version: "0.3.0".to_string(),
            },
            ConnectedDevice {
                client_id: "desktop".to_string(),
                hostname: "pc".to_string(),
                connected_at: "2026-04-08T10:05:00Z".to_string(),
                pid: 5678,
                version: "0.3.0".to_string(),
            },
        ];

        let json = serde_json::to_string(&devices).unwrap();
        let parsed: Vec<ConnectedDevice> = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].client_id, "laptop");
        assert_eq!(parsed[1].client_id, "desktop");
    }
}
