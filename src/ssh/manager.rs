// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SSH connection manager — shares one russh Handle per (host, port, username).

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::connection::{self, HostKeyPrompt, SshError, SshHandler};

/// Key for deduplicating SSH connections.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct ConnKey {
    pub host: String,
    pub port: u16,
    pub username: String,
}

/// Result from get_or_connect, including any deferred host key prompt.
#[must_use]
pub struct ManagedConnectResult {
    pub handle: Arc<Mutex<russh::client::Handle<SshHandler>>>,
    /// Set on first connection if the host key was unknown (TOFU).
    pub host_key_prompt: Option<HostKeyPrompt>,
    /// Server host key fingerprint (always set after successful handshake).
    pub fingerprint: Option<String>,
}

/// Manages shared russh connection handles.
///
/// Multiple tabs to the same server share one TCP connection (russh multiplexes
/// channels over a single connection). Each tab opens its own channel.
pub struct ConnectionManager {
    handles: HashMap<ConnKey, Arc<Mutex<russh::client::Handle<SshHandler>>>>,
    /// Maps (username, host key fingerprint) → ConnKey for duplicate server detection.
    fingerprint_to_key: HashMap<(String, String), ConnKey>,
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            handles: HashMap::new(),
            fingerprint_to_key: HashMap::new(),
        }
    }

    /// Get an existing handle or create a new connection.
    ///
    /// If the cached handle is stale (connection dropped), the caller should
    /// call `remove()` and retry.
    pub async fn get_or_connect(
        &mut self,
        key: &ConnKey,
        identity_file: Option<&str>,
        password: Option<&str>,
        keepalive_interval_secs: u32,
    ) -> Result<ManagedConnectResult, SshError> {
        if let Some(handle) = self.handles.get(key) {
            return Ok(ManagedConnectResult {
                handle: handle.clone(),
                host_key_prompt: None,
                fingerprint: None,
            });
        }

        let result = connection::connect(
            &key.host,
            key.port,
            &key.username,
            identity_file,
            password,
            keepalive_interval_secs,
        )
        .await?;

        // Duplicate server detection: same user + same host key = same server
        if let Some(ref fp) = result.fingerprint {
            let identity = (key.username.clone(), fp.clone());
            if let Some(existing) = self.fingerprint_to_key.get(&identity)
                && existing != key
                && self.handles.contains_key(existing)
            {
                return Err(SshError::DuplicateServer {
                    fingerprint: fp.clone(),
                    existing_host: existing.host.clone(),
                    existing_port: existing.port,
                });
            }
            self.fingerprint_to_key.insert(identity, key.clone());
        }

        let arc = Arc::new(Mutex::new(result.handle));
        self.handles.insert(key.clone(), arc.clone());
        Ok(ManagedConnectResult {
            handle: arc,
            host_key_prompt: result.host_key_prompt,
            fingerprint: result.fingerprint,
        })
    }

    /// Get a cached handle without creating a new connection.
    pub fn get_cached(
        &self,
        key: &ConnKey,
    ) -> Option<Arc<Mutex<russh::client::Handle<SshHandler>>>> {
        self.handles.get(key).cloned()
    }

    /// Remove a cached handle (e.g. after connection failure).
    pub fn remove(&mut self, key: &ConnKey) {
        self.handles.remove(key);
        self.fingerprint_to_key.retain(|_, v| v != key);
    }
}
