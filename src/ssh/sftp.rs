// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SFTP state sync with shell command fallback (FR-CONN-20).
//!
//! Provides file I/O on the remote server for state persistence.
//! Prefers SFTP when available, falls back to shell commands via exec.

use std::sync::Arc;
use std::time::{Duration, Instant};

use russh_sftp::client::SftpSession;
use tokio::sync::Mutex;

use super::connection::SshHandler;
use crate::error::SshError;

/// Remote state directory under the user's home.
const REMOTE_STATE_DIR: &str = ".terminal-state";

/// Minimum interval between server state writes (debounce). /* FR-STATE-06 */
const SYNC_DEBOUNCE: Duration = Duration::from_millis(500);

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
    sftp.write(&tmp_path, data)
        .await
        .map_err(|e| SshError::Sftp(format!("sftp write {tmp_path}: {e}")))?;
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

/// Ensure the remote state directory exists and return the state file path.
pub async fn ensure_state_dir(sftp: &SftpSession, client_id: &str) -> Result<String, SshError> {
    let home = sftp
        .canonicalize(".")
        .await
        .map_err(|e| SshError::Sftp(format!("sftp canonicalize home: {e}")))?;
    let dir = format!("{home}/{REMOTE_STATE_DIR}");
    // Ignore error if directory already exists.
    let _ = sftp.create_dir(&dir).await;
    Ok(format!("{dir}/{client_id}.json"))
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

/// Ensure the remote state directory exists via shell command.
pub async fn ensure_state_dir_shell(
    handle: &russh::client::Handle<SshHandler>,
    client_id: &str,
) -> Result<String, SshError> {
    let cmd =
        format!("mkdir -p ~/{REMOTE_STATE_DIR} && echo ~/{REMOTE_STATE_DIR}/{client_id}.json");
    let output = super::connection::exec_command(handle, &cmd).await?;
    Ok(output.trim().to_string())
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
pub struct StateSyncer {
    handle: HandleArc,
    transport: Transport,
    state_path: String,
    last_write: Mutex<Option<Instant>>,
}

// Manual Debug because SftpSession doesn't impl Debug.
impl std::fmt::Debug for StateSyncer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateSyncer")
            .field("state_path", &self.state_path)
            .field("is_sftp", &self.is_sftp())
            .finish()
    }
}

impl StateSyncer {
    /// Create a new StateSyncer. Tries SFTP first, falls back to shell.
    pub async fn new(handle: HandleArc, client_id: &str) -> Result<Self, SshError> {
        let guard = handle.lock().await;
        // Try SFTP first
        match open_sftp(&guard).await {
            Ok(sftp) => {
                let state_path = ensure_state_dir(&sftp, client_id).await?;
                drop(guard);
                tracing::info!("sftp: state sync via SFTP at {state_path}");
                Ok(Self {
                    handle,
                    transport: Transport::Sftp(sftp),
                    state_path,
                    last_write: Mutex::new(None),
                })
            }
            Err(e) => {
                // E-CONN-8: SFTP unavailable, use shell fallback
                tracing::warn!("sftp unavailable ({e}), using shell fallback");
                let state_path = ensure_state_dir_shell(&guard, client_id).await?;
                drop(guard);
                tracing::info!("sftp: state sync via shell at {state_path}");
                Ok(Self {
                    handle,
                    transport: Transport::Shell,
                    state_path,
                    last_write: Mutex::new(None),
                })
            }
        }
    }

    /// Read state from the server. Server state takes precedence (FR-STATE-02).
    pub async fn read_state(&self) -> Result<Option<String>, SshError> {
        match &self.transport {
            Transport::Sftp(sftp) => match read_file(sftp, &self.state_path).await {
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
                match read_file_shell(&guard, &self.state_path).await {
                    Ok(s) if s.is_empty() => Ok(None),
                    Ok(s) => Ok(Some(s)),
                    Err(e) if e.to_string().contains("No such file") => Ok(None),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Write state to the server, debounced to at most 1 write per 500ms.
    pub async fn write_state(&self, json: &str) -> Result<(), SshError> {
        {
            let mut last = self.last_write.lock().await;
            if let Some(t) = *last
                && t.elapsed() < SYNC_DEBOUNCE
            {
                return Ok(()); // debounced
            }
            *last = Some(Instant::now());
        }

        match &self.transport {
            Transport::Sftp(sftp) => {
                write_file_atomic(sftp, &self.state_path, json.as_bytes()).await
            }
            Transport::Shell => {
                let guard = self.handle.lock().await;
                write_file_shell(&guard, &self.state_path, json).await
            }
        }
    }

    /// Returns whether this syncer is using SFTP (true) or shell fallback (false).
    pub fn is_sftp(&self) -> bool {
        matches!(self.transport, Transport::Sftp(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_state_dir_constant() {
        assert_eq!(REMOTE_STATE_DIR, ".terminal-state");
    }

    #[test]
    fn debounce_duration() {
        assert_eq!(SYNC_DEBOUNCE, Duration::from_millis(500));
    }
}
