// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SSH connection manager — shares one russh Handle per (host, port, username).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::connection::{self, SshError, SshHandler};

/// Key for deduplicating SSH connections.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct ConnKey {
    pub host: String,
    pub port: u16,
    pub username: String,
}

/// Manages shared russh connection handles.
///
/// Multiple tabs to the same server share one TCP connection (russh multiplexes
/// channels over a single connection). Each tab opens its own channel.
pub struct ConnectionManager {
    handles: HashMap<ConnKey, Arc<Mutex<russh::client::Handle<SshHandler>>>>,
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
        keepalive_interval_secs: u32,
    ) -> Result<Arc<Mutex<russh::client::Handle<SshHandler>>>, SshError> {
        if let Some(handle) = self.handles.get(key) {
            return Ok(handle.clone());
        }

        let handle = connection::connect(
            &key.host,
            key.port,
            &key.username,
            identity_file,
            keepalive_interval_secs,
        )
        .await?;
        let arc = Arc::new(Mutex::new(handle));
        self.handles.insert(key.clone(), arc.clone());
        Ok(arc)
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
    }
}
