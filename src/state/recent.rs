// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recent connections persistence.
//!
//! Stores the last 20 SSH connections in a JSON file at
//! `$XDG_DATA_HOME/shellkeep/recent.json`.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MAX_RECENT: usize = 50;

#[deprecated(since = "0.3.0", note = "Use state::server::SavedServers instead")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentConnection {
    pub label: String,
    #[serde(default)]
    pub ssh_args: Vec<String>,
    pub host: String,
    pub user: String,
    pub port: String,
    #[serde(default)]
    pub identity_file: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub last_connected: Option<u64>,
    #[serde(default)]
    pub host_key_fingerprint: Option<String>,
}

#[deprecated(since = "0.3.0", note = "Use state::server::SavedServers instead")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecentConnections {
    #[allow(deprecated)]
    pub connections: Vec<RecentConnection>,
}

#[allow(deprecated)] // impl on deprecated type, needed for migration
impl RecentConnections {
    /// Load recent connections from disk.
    pub fn load() -> Self {
        let path = Self::file_path();
        match fs::read_to_string(&path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save recent connections to disk.
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

    /// Add a connection to the front of the list, deduplicating by label.
    /// Sets last_connected timestamp automatically.
    pub fn push(&mut self, mut conn: RecentConnection) {
        conn.last_connected = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        // Remove existing entry with same label
        self.connections.retain(|c| c.label != conn.label);
        // Add to front
        self.connections.insert(0, conn);
        // Trim to max
        self.connections.truncate(MAX_RECENT);
    }

    fn file_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("shellkeep")
            .join("recent.json")
    }
}

#[cfg(test)]
#[allow(deprecated)] // tests exercise deprecated types kept for migration
mod tests {
    use super::*;

    #[test]
    fn push_deduplicates() {
        let mut recent = RecentConnections::default();
        recent.push(RecentConnection {
            label: "alice@example.com".into(),
            ssh_args: vec!["alice@example.com".into()],
            host: "example.com".into(),
            user: "alice".into(),
            port: "22".into(),
            identity_file: None,
            alias: None,
            last_connected: None,
            host_key_fingerprint: None,
        });
        recent.push(RecentConnection {
            label: "bob@other.com".into(),
            ssh_args: vec!["bob@other.com".into()],
            host: "other.com".into(),
            user: "bob".into(),
            port: "22".into(),
            identity_file: None,
            alias: None,
            last_connected: None,
            host_key_fingerprint: None,
        });
        // Push duplicate — should move to front
        recent.push(RecentConnection {
            label: "alice@example.com".into(),
            ssh_args: vec!["alice@example.com".into()],
            host: "example.com".into(),
            user: "alice".into(),
            port: "22".into(),
            identity_file: None,
            alias: None,
            last_connected: None,
            host_key_fingerprint: None,
        });
        assert_eq!(recent.connections.len(), 2);
        assert_eq!(recent.connections[0].label, "alice@example.com");
    }

    #[test]
    fn truncates_at_max() {
        let mut recent = RecentConnections::default();
        for i in 0..55 {
            recent.push(RecentConnection {
                label: format!("host-{i}"),
                ssh_args: vec![format!("host-{i}")],
                host: format!("host-{i}"),
                user: "user".into(),
                port: "22".into(),
                identity_file: None,
                alias: None,
                last_connected: None,
                host_key_fingerprint: None,
            });
        }
        assert_eq!(recent.connections.len(), MAX_RECENT);
        assert_eq!(recent.connections[0].label, "host-54"); // most recent
    }
}
