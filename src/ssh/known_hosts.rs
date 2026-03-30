// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! OpenSSH known_hosts file parsing and host key verification.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use russh::keys::ssh_key;
use ssh_key::PublicKey;

/// Result of checking a server's host key against known_hosts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// Key matches the stored entry.
    Known,
    /// Host is in known_hosts but with a different key.
    Changed,
    /// Host is not in known_hosts.
    Unknown,
}

/// Format a hostname for known_hosts lookup.
/// Non-standard ports use OpenSSH's `[host]:port` format.
fn format_host(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Get the path to ~/.ssh/known_hosts.
fn known_hosts_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

/// Check a server's public key against ~/.ssh/known_hosts.
pub fn check_host_key(host: &str, port: u16, server_key: &PublicKey) -> HostKeyStatus {
    let Some(path) = known_hosts_path() else {
        return HostKeyStatus::Unknown;
    };

    let contents = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return HostKeyStatus::Unknown,
    };

    check_host_key_in(host, port, server_key, &contents)
}

/// Check a server key against known_hosts content (testable without filesystem).
fn check_host_key_in(
    host: &str,
    port: u16,
    server_key: &PublicKey,
    contents: &str,
) -> HostKeyStatus {
    let lookup = format_host(host, port);

    // Encode the server key to openssh format for comparison
    let server_encoded = match server_key.to_openssh() {
        Ok(s) => s,
        Err(_) => return HostKeyStatus::Unknown,
    };
    // openssh format: "key-type base64-data [comment]"
    let server_parts: Vec<&str> = server_encoded.split_whitespace().collect();
    let (server_type, server_b64) = if server_parts.len() >= 2 {
        (server_parts[0], server_parts[1])
    } else {
        return HostKeyStatus::Unknown;
    };

    let mut host_found = false;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Skip hashed hostnames (start with |1|)
        if line.starts_with("|1|") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let hostnames = parts[0];
        let key_type = parts[1];
        let key_data = parts[2];

        // Check if this line matches our host
        let matches_host = hostnames.split(',').any(|h| h == lookup);

        if !matches_host {
            continue;
        }

        host_found = true;

        // Compare key type and data
        if key_type == server_type && key_data == server_b64 {
            return HostKeyStatus::Known;
        }
    }

    if host_found {
        HostKeyStatus::Changed
    } else {
        HostKeyStatus::Unknown
    }
}

/// Append a host key to ~/.ssh/known_hosts.
pub fn add_host_key(host: &str, port: u16, server_key: &PublicKey) -> Result<(), std::io::Error> {
    let path = known_hosts_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory"))?;

    // Ensure ~/.ssh directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }

    let hostname = format_host(host, port);
    let openssh_str = server_key
        .to_openssh()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    // openssh format: "key-type base64-data [comment]"
    // We want: "hostname key-type base64-data\n"
    let parts: Vec<&str> = openssh_str.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "failed to encode public key",
        ));
    }
    let line = format!("{hostname} {} {}\n", parts[0], parts[1]);

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }

    file.write_all(line.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real ed25519 public key in openssh format for testing
    const TEST_KEY_OPENSSH: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";
    const TEST_KEY_B64: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";

    // A different ed25519 key
    const OTHER_KEY_OPENSSH: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBl2FpcCxfVJxLNq+KPVxMkHqmWS17v9PEMPnJHBCuEN";

    fn parse_test_key(openssh: &str) -> PublicKey {
        PublicKey::from_openssh(openssh).expect("failed to parse test key")
    }

    #[test]
    fn known_host_matches() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        let contents = format!("example.com ssh-ed25519 {TEST_KEY_B64}\n");
        assert_eq!(
            check_host_key_in("example.com", 22, &key, &contents),
            HostKeyStatus::Known,
        );
    }

    #[test]
    fn unknown_host() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        let contents = format!("other.com ssh-ed25519 {TEST_KEY_B64}\n");
        assert_eq!(
            check_host_key_in("example.com", 22, &key, &contents),
            HostKeyStatus::Unknown,
        );
    }

    #[test]
    fn changed_key_detected() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        let other_b64 = OTHER_KEY_OPENSSH.split_whitespace().nth(1).unwrap();
        let contents = format!("example.com ssh-ed25519 {other_b64}\n");
        assert_eq!(
            check_host_key_in("example.com", 22, &key, &contents),
            HostKeyStatus::Changed,
        );
    }

    #[test]
    fn non_standard_port() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        let contents = format!("[example.com]:2222 ssh-ed25519 {TEST_KEY_B64}\n");
        assert_eq!(
            check_host_key_in("example.com", 2222, &key, &contents),
            HostKeyStatus::Known,
        );
    }

    #[test]
    fn skips_hashed_hostnames() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        let contents = format!("|1|abc123|def456 ssh-ed25519 {TEST_KEY_B64}\n");
        assert_eq!(
            check_host_key_in("example.com", 22, &key, &contents),
            HostKeyStatus::Unknown,
        );
    }

    #[test]
    fn skips_comments_and_empty_lines() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        let contents = format!("# this is a comment\n\nexample.com ssh-ed25519 {TEST_KEY_B64}\n");
        assert_eq!(
            check_host_key_in("example.com", 22, &key, &contents),
            HostKeyStatus::Known,
        );
    }

    #[test]
    fn multi_hostname_entry() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        let contents = format!("example.com,192.168.1.1 ssh-ed25519 {TEST_KEY_B64}\n");
        assert_eq!(
            check_host_key_in("192.168.1.1", 22, &key, &contents),
            HostKeyStatus::Known,
        );
    }

    #[test]
    fn empty_known_hosts() {
        let key = parse_test_key(TEST_KEY_OPENSSH);
        assert_eq!(
            check_host_key_in("example.com", 22, &key, ""),
            HostKeyStatus::Unknown,
        );
    }
}
