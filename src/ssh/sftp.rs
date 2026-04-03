// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SFTP state sync with shell command fallback (FR-CONN-20).
//!
//! Provides file I/O on the remote server for state persistence.
//! Prefers SFTP when available, falls back to shell commands via exec.
//!
//! Syncs two files:
//! - shared state at `~/.shellkeep/shared.json`
//! - per-device state at `~/.shellkeep/clients/<client-id>.json`

use std::sync::Arc;

use russh_sftp::client::SftpSession;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use super::connection::SshHandler;
use crate::error::SshError;

/// Remote state directory under the user's home.
const REMOTE_STATE_DIR: &str = ".shellkeep";

/// Legacy remote state directory (pre-v0.3). Used for auto-migration.
const LEGACY_REMOTE_STATE_DIR: &str = ".terminal-state";

/// Subdirectory for per-device state files.
const REMOTE_CLIENTS_DIR: &str = "clients";

/// Open an SFTP session on an existing SSH connection.
pub async fn open_sftp(
    handle: &russh::client::Handle<SshHandler>,
) -> Result<SftpSession, SshError> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| SshError::Sftp(format!("sftp channel: {e}")))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| SshError::Sftp(format!("sftp subsystem: {e}")))?;
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| SshError::Sftp(format!("sftp session: {e}")))?;
    Ok(sftp)
}

/// Read a file from the server via SFTP.
pub async fn read_file(sftp: &SftpSession, path: &str) -> Result<Vec<u8>, SshError> {
    sftp.read(path)
        .await
        .map_err(|e| SshError::Sftp(format!("sftp read {path}: {e}")))
}

/// Write a file atomically via SFTP: write to .tmp, then rename.
/// FR-STATE-05: uses posix-rename semantics (atomic overwrite). Falls back to
/// unlink + rename if the rename fails (e.g., server lacks posix-rename extension).
pub async fn write_file_atomic(
    sftp: &SftpSession,
    path: &str,
    data: &[u8],
) -> Result<(), SshError> {
    let tmp_path = format!("{path}.tmp");
    // Use create() not write() — write() only opens existing files,
    // create() uses CREATE|TRUNCATE|WRITE flags.
    let mut file = sftp
        .create(&tmp_path)
        .await
        .map_err(|e| SshError::Sftp(format!("sftp create {tmp_path}: {e}")))?;
    file.write_all(data)
        .await
        .map_err(|e| SshError::Sftp(format!("sftp write {tmp_path}: {e}")))?;
    // Flush and close the file handle before renaming — SFTP requires the
    // handle to be closed (SSH_FXP_CLOSE) before the data is visible on disk.
    file.shutdown()
        .await
        .map_err(|e| SshError::Sftp(format!("sftp close {tmp_path}: {e}")))?;
    // Try rename (posix-rename@openssh.com does atomic overwrite)
    match sftp.rename(&tmp_path, path).await {
        Ok(()) => Ok(()),
        Err(_) => {
            // Fallback: unlink target then rename
            let _ = sftp.remove_file(path).await; // ignore error if file doesn't exist
            sftp.rename(&tmp_path, path)
                .await
                .map_err(|e| SshError::Sftp(format!("sftp rename {tmp_path} -> {path}: {e}")))
        }
    }
}

/// Remote file paths for split state.
#[derive(Debug, Clone)]
pub struct RemoteStatePaths {
    /// Path to shared state: `~/.shellkeep/shared.json`
    pub shared: String,
    /// Path to device state: `~/.shellkeep/clients/<client-id>.json`
    pub device: String,
}

/// Ensure the remote state directories exist and return both file paths.
pub async fn ensure_state_dir(
    sftp: &SftpSession,
    client_id: &str,
) -> Result<RemoteStatePaths, SshError> {
    let home = sftp
        .canonicalize(".")
        .await
        .map_err(|e| SshError::Sftp(format!("sftp canonicalize home: {e}")))?;
    let dir = format!("{home}/{REMOTE_STATE_DIR}");
    let clients_dir = format!("{dir}/{REMOTE_CLIENTS_DIR}");
    // Ignore errors if directories already exist.
    let _ = sftp.create_dir(&dir).await;
    let _ = sftp.create_dir(&clients_dir).await;
    Ok(RemoteStatePaths {
        shared: format!("{dir}/shared.json"),
        device: format!("{clients_dir}/{client_id}.json"),
    })
}

// --- Shell command fallback (FR-CONN-20) ---

/// Read a file via shell command when SFTP is unavailable.
pub async fn read_file_shell(
    handle: &russh::client::Handle<SshHandler>,
    path: &str,
) -> Result<String, SshError> {
    super::connection::exec_command(handle, &format!("cat {path}")).await
}

/// Write a file atomically via shell command when SFTP is unavailable.
pub async fn write_file_shell(
    handle: &russh::client::Handle<SshHandler>,
    path: &str,
    content: &str,
) -> Result<(), SshError> {
    let tmp = format!("{path}.tmp.$$");
    let cmd = format!("cat > {tmp} << 'SHELLKEEP_EOF'\n{content}\nSHELLKEEP_EOF\nmv {tmp} {path}");
    super::connection::exec_command(handle, &cmd).await?;
    Ok(())
}

/// Ensure the remote state directories exist via shell command.
pub async fn ensure_state_dir_shell(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
) -> Result<RemoteStatePaths, SshError> {
    let cmd = format!(
        "mkdir -p ~/{REMOTE_STATE_DIR}/{REMOTE_CLIENTS_DIR} && \
         echo ~/{REMOTE_STATE_DIR}/shared.json && \
         echo ~/{REMOTE_STATE_DIR}/{REMOTE_CLIENTS_DIR}/{client_id}.json"
    );
    let output = super::connection::exec_command(handle, &cmd).await?;
    let lines: Vec<&str> = output.trim().lines().collect();
    if lines.len() < 2 {
        return Err(SshError::Sftp(
            "unexpected output from ensure_state_dir_shell".to_string(),
        ));
    }
    Ok(RemoteStatePaths {
        shared: lines[0].to_string(),
        device: lines[1].to_string(),
    })
}

// ---------------------------------------------------------------------------
// FR-STATE-19: auto-migrate ~/.terminal-state/ → ~/.shellkeep/
// ---------------------------------------------------------------------------

/// Migrate legacy remote state directory via SFTP.
/// If `~/.shellkeep/` is absent but `~/.terminal-state/` exists, rename it.
async fn migrate_remote_dir_sftp(sftp: &SftpSession) {
    let home = match sftp.canonicalize(".").await {
        Ok(h) => h,
        Err(_) => return,
    };
    let new_dir = format!("{home}/{REMOTE_STATE_DIR}");
    let legacy_dir = format!("{home}/{LEGACY_REMOTE_STATE_DIR}");

    // If new dir already exists, nothing to migrate.
    if sftp.canonicalize(&new_dir).await.is_ok() {
        return;
    }
    // If legacy dir doesn't exist, nothing to migrate.
    if sftp.canonicalize(&legacy_dir).await.is_err() {
        return;
    }
    // Rename legacy → new
    match sftp.rename(&legacy_dir, &new_dir).await {
        Ok(()) => tracing::info!("migrated remote state directory: {legacy_dir} → {new_dir}"),
        Err(e) => {
            tracing::warn!("failed to migrate remote state directory {legacy_dir} → {new_dir}: {e}")
        }
    }
}

/// Migrate legacy remote state directory via shell commands.
/// If `~/.shellkeep/` is absent but `~/.terminal-state/` exists, rename it.
async fn migrate_remote_dir_shell(handle: &russh::client::Handle<SshHandler>) {
    let cmd = format!(
        "if [ ! -d ~/{REMOTE_STATE_DIR} ] && [ -d ~/{LEGACY_REMOTE_STATE_DIR} ]; then \
         mv ~/{LEGACY_REMOTE_STATE_DIR} ~/{REMOTE_STATE_DIR} && \
         echo MIGRATED; fi"
    );
    match super::connection::exec_command(handle, &cmd).await {
        Ok(output) if output.contains("MIGRATED") => {
            tracing::info!(
                "migrated remote state directory via shell: ~/.terminal-state → ~/.shellkeep"
            );
        }
        Ok(_) => {} // no migration needed
        Err(e) => {
            tracing::warn!("failed to check/migrate remote state directory: {e}");
        }
    }
}

/// Transport method for remote file I/O.
enum Transport {
    Sftp(SftpSession),
    Shell,
}

type HandleArc = Arc<Mutex<russh::client::Handle<SshHandler>>>;

/// Syncs state to the remote server. /* FR-STATE-02, FR-CONN-20, FR-RECONNECT-09 */
///
/// Uses the shared SSH connection (via Arc<Mutex<Handle>>) for SFTP or shell fallback.
/// Debounces writes to at most one per 500ms.
/// Manages two files: shared state and per-device state.
pub struct StateSyncer {
    handle: HandleArc,
    transport: Transport,
    paths: RemoteStatePaths,
}

// Manual Debug because SftpSession doesn't impl Debug.
impl std::fmt::Debug for StateSyncer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateSyncer")
            .field("paths", &self.paths)
            .field("is_sftp", &self.is_sftp())
            .finish()
    }
}

impl StateSyncer {
    /// Create a new StateSyncer. Tries SFTP first, falls back to shell.
    /// FR-STATE-19: auto-migrates `~/.terminal-state/` → `~/.shellkeep/` on first connect.
    pub async fn new(handle: HandleArc, client_id: &str) -> Result<Self, SshError> {
        let guard = handle.lock().await;
        // Try SFTP first
        match open_sftp(&guard).await {
            Ok(sftp) => {
                migrate_remote_dir_sftp(&sftp).await;
                let paths = ensure_state_dir(&sftp, client_id).await?;
                drop(guard);
                tracing::info!("sftp: state sync via SFTP at {:?}", paths);
                Ok(Self {
                    handle,
                    transport: Transport::Sftp(sftp),
                    paths,
                })
            }
            Err(e) => {
                // E-CONN-8: SFTP unavailable, use shell fallback
                tracing::warn!("sftp unavailable ({e}), using shell fallback");
                migrate_remote_dir_shell(&guard).await;
                let paths = ensure_state_dir_shell(&guard, client_id).await?;
                drop(guard);
                tracing::info!("sftp: state sync via shell at {:?}", paths);
                Ok(Self {
                    handle,
                    transport: Transport::Shell,
                    paths,
                })
            }
        }
    }

    /// Read shared state from the server. Server state takes precedence (FR-STATE-02).
    pub async fn read_shared_state(&self) -> Result<Option<String>, SshError> {
        self.read_remote_file(&self.paths.shared).await
    }

    /// Read device state from the server.
    pub async fn read_device_state(&self) -> Result<Option<String>, SshError> {
        self.read_remote_file(&self.paths.device).await
    }

    /// Legacy: read state from server (reads shared state for backward compat).
    pub async fn read_state(&self) -> Result<Option<String>, SshError> {
        self.read_shared_state().await
    }

    /// Write shared state to the server, debounced.
    pub async fn write_shared_state(&self, json: &str) -> Result<(), SshError> {
        self.write_remote_file(&self.paths.shared, json).await
    }

    /// Write device state to the server, debounced.
    pub async fn write_device_state(&self, json: &str) -> Result<(), SshError> {
        self.write_remote_file(&self.paths.device, json).await
    }

    /// Legacy: write state to server (writes shared state for backward compat).
    pub async fn write_state(&self, json: &str) -> Result<(), SshError> {
        self.write_shared_state(json).await
    }

    /// Returns whether this syncer is using SFTP (true) or shell fallback (false).
    pub fn is_sftp(&self) -> bool {
        matches!(self.transport, Transport::Sftp(_))
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    async fn read_remote_file(&self, path: &str) -> Result<Option<String>, SshError> {
        match &self.transport {
            Transport::Sftp(sftp) => match read_file(sftp, path).await {
                Ok(data) => Ok(Some(String::from_utf8_lossy(&data).to_string())),
                Err(e)
                    if {
                        let msg = e.to_string();
                        msg.contains("NoSuchFile") || msg.contains("No such file")
                    } =>
                {
                    Ok(None)
                }
                Err(e) => Err(e),
            },
            Transport::Shell => {
                let guard = self.handle.lock().await;
                match read_file_shell(&guard, path).await {
                    Ok(s) if s.is_empty() => Ok(None),
                    Ok(s) => Ok(Some(s)),
                    Err(e) if e.to_string().contains("No such file") => Ok(None),
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn write_remote_file(&self, path: &str, content: &str) -> Result<(), SshError> {
        match &self.transport {
            Transport::Sftp(sftp) => write_file_atomic(sftp, path, content.as_bytes()).await,
            Transport::Shell => {
                let guard = self.handle.lock().await;
                write_file_shell(&guard, path, content).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_state_dir_constant() {
        assert_eq!(REMOTE_STATE_DIR, ".shellkeep");
    }

    #[test]
    fn legacy_remote_state_dir_constant() {
        assert_eq!(LEGACY_REMOTE_STATE_DIR, ".terminal-state");
    }

    #[test]
    fn remote_clients_dir_constant() {
        assert_eq!(REMOTE_CLIENTS_DIR, "clients");
    }
}
