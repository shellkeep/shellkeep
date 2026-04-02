// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saved servers persistence.
//!
//! Stores locally-known SSH servers in a JSON file at
//! `$XDG_DATA_HOME/shellkeep/servers.json`.
//!
//! Each server has a stable UUID for local identification.
//! Replaces the flat `RecentConnection` list with a structured model
//! that separates local credentials from server-side workspace state.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[allow(deprecated)] // Migration code needs access to legacy types
use super::recent::RecentConnections;

const MAX_SERVERS: usize = 50;

/// A locally-saved SSH server. /* FR-UI-02 */
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedServer {
    /// Stable local identifier.
    pub uuid: String,
    /// Optional display name (e.g., "My VPS").
    #[serde(default)]
    pub name: Option<String>,
    /// SSH host.
    pub host: String,
    /// SSH username.
    pub user: String,
    /// SSH port (as string for compatibility).
    pub port: String,
    /// Path to identity file (private key).
    #[serde(default)]
    pub identity_file: Option<String>,
    /// Unix epoch of last successful connection.
    #[serde(default)]
    pub last_connected: Option<u64>,
}

impl SavedServer {
    /// Display label: `name` if set, otherwise `user@host` (port omitted if 22).
    pub fn display_label(&self) -> String {
        if let Some(name) = &self.name {
            if !name.is_empty() {
                return name.clone();
            }
        }
        if self.port == "22" {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

/// Collection of saved servers. /* FR-UI-02 */
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SavedServers {
    pub servers: Vec<SavedServer>,
}

impl SavedServers {
    /// Load saved servers from disk.
    /// If `servers.json` doesn't exist but `recent.json` does, migrates automatically.
    #[allow(deprecated)] // accesses legacy RecentConnections for migration
    pub fn load() -> Self {
        let path = Self::file_path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(servers) = serde_json::from_str(&data) {
                return servers;
            }
        }
        // Try migration from recent.json
        let recent = RecentConnections::load();
        if recent.connections.is_empty() {
            return Self::default();
        }
        let migrated = Self::migrate_from_recent(&recent);
        migrated.save();
        migrated
    }

    /// Save servers to disk.
    /// NFR-SEC-11: sets 0600 permissions on the file (Unix only).
    pub fn save(&self) {
        let path = Self::file_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, &data);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
        }
    }

    /// Add or update a server. Deduplicates by UUID.
    /// Sets `last_connected` timestamp automatically.
    pub fn push(&mut self, mut server: SavedServer) {
        server.last_connected = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        self.servers.retain(|s| s.uuid != server.uuid);
        self.servers.insert(0, server);
        self.servers.truncate(MAX_SERVERS);
    }

    /// Find a server by UUID.
    pub fn find_by_uuid(&self, uuid: &str) -> Option<&SavedServer> {
        self.servers.iter().find(|s| s.uuid == uuid)
    }

    /// Find a mutable server by UUID.
    pub fn find_by_uuid_mut(&mut self, uuid: &str) -> Option<&mut SavedServer> {
        self.servers.iter_mut().find(|s| s.uuid == uuid)
    }

    /// Remove a server by UUID. Returns true if found and removed.
    pub fn remove_by_uuid(&mut self, uuid: &str) -> bool {
        let before = self.servers.len();
        self.servers.retain(|s| s.uuid != uuid);
        self.servers.len() < before
    }

    /// Migrate from the legacy `RecentConnections` format.
    #[allow(deprecated)] // accesses legacy RecentConnections for migration
    fn migrate_from_recent(recent: &RecentConnections) -> Self {
        let servers: Vec<SavedServer> = recent
            .connections
            .iter()
            .map(|rc| SavedServer {
                uuid: uuid::Uuid::new_v4().to_string(),
                name: None,
                host: rc.host.clone(),
                user: rc.user.clone(),
                port: rc.port.clone(),
                identity_file: rc.identity_file.clone(),
                last_connected: rc.last_connected,
            })
            .collect();
        tracing::info!(
            "migrated {} recent connections to saved servers",
            servers.len()
        );
        Self { servers }
    }

    fn file_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellkeep")
            .join("servers.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_server(uuid: &str, host: &str) -> SavedServer {
        SavedServer {
            uuid: uuid.to_string(),
            name: None,
            host: host.to_string(),
            user: "user".to_string(),
            port: "22".to_string(),
            identity_file: None,
            last_connected: None,
        }
    }

    #[test]
    fn serde_roundtrip() {
        let server = SavedServer {
            uuid: "abc-123".to_string(),
            name: Some("My VPS".to_string()),
            host: "example.com".to_string(),
            user: "alice".to_string(),
            port: "2222".to_string(),
            identity_file: Some("/home/alice/.ssh/id_ed25519".to_string()),
            last_connected: Some(1711900000),
        };
        let json = serde_json::to_string(&server).unwrap();
        let deser: SavedServer = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.uuid, "abc-123");
        assert_eq!(deser.name.as_deref(), Some("My VPS"));
        assert_eq!(deser.host, "example.com");
        assert_eq!(deser.port, "2222");
        assert_eq!(
            deser.identity_file.as_deref(),
            Some("/home/alice/.ssh/id_ed25519")
        );
        assert_eq!(deser.last_connected, Some(1711900000));
    }

    #[test]
    fn push_deduplicates_by_uuid() {
        let mut servers = SavedServers::default();
        servers.push(make_server("id-1", "host-a"));
        servers.push(make_server("id-2", "host-b"));
        servers.push(make_server("id-1", "host-a-updated"));
        assert_eq!(servers.servers.len(), 2);
        assert_eq!(servers.servers[0].uuid, "id-1");
        assert_eq!(servers.servers[0].host, "host-a-updated");
    }

    #[test]
    fn truncates_at_max() {
        let mut servers = SavedServers::default();
        for i in 0..55 {
            servers.push(make_server(&format!("id-{i}"), &format!("host-{i}")));
        }
        assert_eq!(servers.servers.len(), MAX_SERVERS);
        assert_eq!(servers.servers[0].uuid, "id-54");
    }

    #[test]
    fn find_by_uuid_returns_correct_entry() {
        let mut servers = SavedServers::default();
        servers.push(make_server("id-1", "host-a"));
        servers.push(make_server("id-2", "host-b"));
        assert_eq!(servers.find_by_uuid("id-2").unwrap().host, "host-b");
        assert!(servers.find_by_uuid("nonexistent").is_none());
    }

    #[test]
    fn remove_by_uuid_works() {
        let mut servers = SavedServers::default();
        servers.push(make_server("id-1", "host-a"));
        servers.push(make_server("id-2", "host-b"));
        assert!(servers.remove_by_uuid("id-1"));
        assert_eq!(servers.servers.len(), 1);
        assert_eq!(servers.servers[0].uuid, "id-2");
        assert!(!servers.remove_by_uuid("nonexistent"));
    }

    #[test]
    fn display_label_with_name() {
        let server = SavedServer {
            uuid: "id".to_string(),
            name: Some("My VPS".to_string()),
            host: "example.com".to_string(),
            user: "alice".to_string(),
            port: "22".to_string(),
            identity_file: None,
            last_connected: None,
        };
        assert_eq!(server.display_label(), "My VPS");
    }

    #[test]
    fn display_label_without_name_port_22() {
        let server = make_server("id", "example.com");
        assert_eq!(server.display_label(), "user@example.com");
    }

    #[test]
    fn display_label_without_name_custom_port() {
        let mut server = make_server("id", "example.com");
        server.port = "2222".to_string();
        assert_eq!(server.display_label(), "user@example.com:2222");
    }

    #[test]
    fn display_label_empty_name_falls_back() {
        let mut server = make_server("id", "example.com");
        server.name = Some(String::new());
        assert_eq!(server.display_label(), "user@example.com");
    }

    #[test]
    #[allow(deprecated)] // tests legacy migration path
    fn migrate_from_recent() {
        use crate::state::recent::{RecentConnection, RecentConnections};
        let recent = RecentConnections {
            connections: vec![
                RecentConnection {
                    label: "alice@server1".to_string(),
                    ssh_args: vec![],
                    host: "server1.com".to_string(),
                    user: "alice".to_string(),
                    port: "22".to_string(),
                    identity_file: Some("/path/to/key".to_string()),
                    alias: Some("s1".to_string()),
                    last_connected: Some(1711800000),
                    host_key_fingerprint: Some("SHA256:abc".to_string()),
                },
                RecentConnection {
                    label: "bob@server2:2222".to_string(),
                    ssh_args: vec![],
                    host: "server2.com".to_string(),
                    user: "bob".to_string(),
                    port: "2222".to_string(),
                    identity_file: None,
                    alias: None,
                    last_connected: None,
                    host_key_fingerprint: None,
                },
            ],
        };
        let migrated = SavedServers::migrate_from_recent(&recent);
        assert_eq!(migrated.servers.len(), 2);
        // UUIDs should be unique
        assert_ne!(migrated.servers[0].uuid, migrated.servers[1].uuid);
        // Fields mapped correctly
        assert_eq!(migrated.servers[0].host, "server1.com");
        assert_eq!(migrated.servers[0].user, "alice");
        assert_eq!(
            migrated.servers[0].identity_file.as_deref(),
            Some("/path/to/key")
        );
        assert_eq!(migrated.servers[0].last_connected, Some(1711800000));
        // name should be None (not migrated from label)
        assert!(migrated.servers[0].name.is_none());
        // Second entry
        assert_eq!(migrated.servers[1].host, "server2.com");
        assert_eq!(migrated.servers[1].port, "2222");
    }
}
