// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parser for ~/.ssh/config (FR-COMPAT-01, FR-CONFIG-07).
//!
//! Reads Host blocks, applies pattern matching (exact + wildcard `*`),
//! and merges directives in file order (first match wins per directive).

use std::fs;
use std::path::{Path, PathBuf};

/// Resolved configuration for a single host.
#[derive(Debug, Default, Clone)]
pub struct HostConfig {
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
    pub identities_only: Option<bool>,
    pub server_alive_interval: Option<u32>,
    pub server_alive_count_max: Option<u32>,
    pub connect_timeout: Option<u32>,
    pub strict_host_key_checking: Option<String>,
    pub proxy_jump: Option<String>,
    pub proxy_command: Option<String>,
}

impl HostConfig {
    /// Merge another config into self. First-match-wins: only fills in None fields.
    fn merge(&mut self, other: &HostConfig) {
        macro_rules! fill {
            ($field:ident) => {
                if self.$field.is_none() {
                    self.$field = other.$field.clone();
                }
            };
        }
        fill!(hostname);
        fill!(user);
        fill!(port);
        fill!(identity_file);
        fill!(identities_only);
        fill!(server_alive_interval);
        fill!(server_alive_count_max);
        fill!(connect_timeout);
        fill!(strict_host_key_checking);
        fill!(proxy_jump);
        fill!(proxy_command);
    }
}

/// A parsed Host block: patterns + directives.
struct HostBlock {
    patterns: Vec<String>,
    config: HostConfig,
}

/// Load and resolve SSH config for the given host alias.
pub fn load_host_config(host: &str) -> HostConfig {
    let path = match dirs::home_dir() {
        Some(home) => home.join(".ssh").join("config"),
        None => return HostConfig::default(),
    };
    load_host_config_from(&path, host)
}

/// Load from a specific config file path (useful for testing).
pub fn load_host_config_from(path: &Path, host: &str) -> HostConfig {
    let blocks = match parse_config_file(path, 0) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("ssh config: failed to read {}: {e}", path.display());
            return HostConfig::default();
        }
    };

    let mut result = HostConfig::default();
    for block in &blocks {
        if block.patterns.iter().any(|p| pattern_matches(p, host)) {
            result.merge(&block.config);
        }
    }
    result
}

/// Parse a config file, handling Include directives.
/// `depth` guards against infinite Include recursion.
fn parse_config_file(path: &Path, depth: u32) -> Result<Vec<HostBlock>, std::io::Error> {
    if depth > 8 {
        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(path)?;
    let mut blocks: Vec<HostBlock> = Vec::new();
    // Directives before any Host block apply to all hosts (implicit "Host *")
    let mut current = HostBlock {
        patterns: vec!["*".to_string()],
        config: HostConfig::default(),
    };

    let base_dir = path.parent().unwrap_or(Path::new("."));

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (key, value) = match split_directive(line) {
            Some(kv) => kv,
            None => continue,
        };

        match key.to_ascii_lowercase().as_str() {
            "host" => {
                blocks.push(current);
                current = HostBlock {
                    patterns: value.split_whitespace().map(|s| s.to_string()).collect(),
                    config: HostConfig::default(),
                };
            }
            "include" => {
                let include_path = resolve_include_path(value, base_dir);
                // Glob expansion for Include
                if let Ok(entries) = glob_paths(&include_path) {
                    for entry in entries {
                        if let Ok(mut included) = parse_config_file(&entry, depth + 1) {
                            // Insert included blocks at current position
                            // They need to be checked in order with the rest
                            blocks.append(&mut included);
                        }
                    }
                }
            }
            _ => apply_directive(&mut current.config, &key, value),
        }
    }
    blocks.push(current);

    Ok(blocks)
}

/// Split a directive line into (key, value).
fn split_directive(line: &str) -> Option<(String, &str)> {
    // SSH config supports both "Key Value" and "Key=Value"
    let (key, rest) = if let Some(eq_pos) = line.find('=') {
        let k = line[..eq_pos].trim();
        let v = line[eq_pos + 1..].trim();
        (k, v)
    } else {
        let mut parts = line.splitn(2, char::is_whitespace);
        let k = parts.next()?;
        let v = parts.next().map(|s| s.trim()).unwrap_or("");
        (k, v)
    };
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), rest))
}

/// Apply a known directive to a HostConfig.
fn apply_directive(config: &mut HostConfig, key: &str, value: &str) {
    match key.to_ascii_lowercase().as_str() {
        "hostname" => config.hostname = Some(value.to_string()),
        "user" => config.user = Some(value.to_string()),
        "port" => {
            if let Ok(p) = value.parse::<u16>() {
                config.port = Some(p);
            }
        }
        "identityfile" => {
            let expanded = expand_tilde(value);
            config.identity_file = Some(expanded);
        }
        "identitiesonly" => {
            config.identities_only = Some(value.eq_ignore_ascii_case("yes"));
        }
        "serveraliveinterval" => {
            if let Ok(v) = value.parse::<u32>() {
                config.server_alive_interval = Some(v);
            }
        }
        "serveralivecountmax" => {
            if let Ok(v) = value.parse::<u32>() {
                config.server_alive_count_max = Some(v);
            }
        }
        "connecttimeout" => {
            if let Ok(v) = value.parse::<u32>() {
                config.connect_timeout = Some(v);
            }
        }
        "stricthostkeychecking" => {
            config.strict_host_key_checking = Some(value.to_string());
        }
        "proxyjump" => config.proxy_jump = Some(value.to_string()),
        "proxycommand" => config.proxy_command = Some(value.to_string()),
        // Unknown directives are silently ignored per OpenSSH behavior
        _ => {}
    }
}

/// Match a Host pattern against a hostname.
/// Supports `*` as a glob wildcard and `?` as single-char wildcard.
fn pattern_matches(pattern: &str, host: &str) -> bool {
    // Negated patterns (e.g., "!*.example.com") — not matched here,
    // would need a broader context to handle properly. Skip negation prefix.
    if pattern.starts_with('!') {
        return false;
    }
    glob_match(pattern, host)
}

/// Simple glob matching: `*` matches any sequence, `?` matches one char.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_inner(&p, &t)
}

fn glob_match_inner(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(&'*'), _) => {
            // Try matching * with zero chars, or consume one text char
            glob_match_inner(&pattern[1..], text)
                || (!text.is_empty() && glob_match_inner(pattern, &text[1..]))
        }
        (Some(&'?'), Some(_)) => glob_match_inner(&pattern[1..], &text[1..]),
        (Some(&pc), Some(&tc)) => {
            pc.eq_ignore_ascii_case(&tc) && glob_match_inner(&pattern[1..], &text[1..])
        }
        _ => false,
    }
}

/// Expand ~ to home directory in paths.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().to_string();
    }
    path.to_string()
}

/// Resolve an Include path relative to the SSH config directory.
fn resolve_include_path(value: &str, base_dir: &Path) -> PathBuf {
    let expanded = expand_tilde(value);
    let path = Path::new(&expanded);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

/// Simple glob expansion for Include directives.
fn glob_paths(pattern: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let pattern_str = pattern.to_string_lossy();
    if !pattern_str.contains('*') && !pattern_str.contains('?') {
        // No glob chars — just check if file exists
        if pattern.exists() {
            return Ok(vec![pattern.to_path_buf()]);
        }
        return Ok(Vec::new());
    }

    // For glob patterns, expand in the parent directory
    let parent = pattern.parent().unwrap_or(Path::new("."));
    let file_pattern = match pattern.file_name() {
        Some(f) => f.to_string_lossy().to_string(),
        None => return Ok(Vec::new()),
    };

    let mut results = Vec::new();
    if let Ok(entries) = fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if glob_match(&file_pattern, &name) {
                results.push(entry.path());
            }
        }
    }
    results.sort();
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_config(dir: &Path, filename: &str, content: &str) -> PathBuf {
        let path = dir.join(filename);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_basic_host_match() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(
            dir.path(),
            "config",
            "\
Host myserver
    HostName 10.0.0.1
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_myserver

Host other
    HostName 10.0.0.2
    User root
",
        );

        let cfg = load_host_config_from(&config, "myserver");
        assert_eq!(cfg.hostname.as_deref(), Some("10.0.0.1"));
        assert_eq!(cfg.user.as_deref(), Some("admin"));
        assert_eq!(cfg.port, Some(2222));
        assert!(cfg.identity_file.is_some());

        let cfg2 = load_host_config_from(&config, "other");
        assert_eq!(cfg2.hostname.as_deref(), Some("10.0.0.2"));
        assert_eq!(cfg2.user.as_deref(), Some("root"));
        assert_eq!(cfg2.port, None);
    }

    #[test]
    fn test_wildcard_match() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(
            dir.path(),
            "config",
            "\
Host prod-*
    User deploy
    Port 22

Host prod-db
    HostName db.internal
    Port 5432

Host *
    ServerAliveInterval 60
    ServerAliveCountMax 3
",
        );

        // prod-db matches both "prod-*" and "prod-db" — first match wins per field
        let cfg = load_host_config_from(&config, "prod-db");
        assert_eq!(cfg.user.as_deref(), Some("deploy")); // from prod-*
        assert_eq!(cfg.hostname.as_deref(), Some("db.internal")); // from prod-db
        assert_eq!(cfg.port, Some(22)); // from prod-* (first match wins)
        assert_eq!(cfg.server_alive_interval, Some(60)); // from *

        // prod-web matches "prod-*" and "*"
        let cfg2 = load_host_config_from(&config, "prod-web");
        assert_eq!(cfg2.user.as_deref(), Some("deploy"));
        assert_eq!(cfg2.hostname, None);
        assert_eq!(cfg2.server_alive_interval, Some(60));
    }

    #[test]
    fn test_include_directive() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            "extra.conf",
            "\
Host included-host
    HostName 192.168.1.1
    User included-user
",
        );

        let extra_path = dir.path().join("extra.conf");
        let config = write_config(
            dir.path(),
            "config",
            &format!(
                "\
Include {}

Host main-host
    HostName 10.0.0.1
",
                extra_path.display()
            ),
        );

        let cfg = load_host_config_from(&config, "included-host");
        assert_eq!(cfg.hostname.as_deref(), Some("192.168.1.1"));
        assert_eq!(cfg.user.as_deref(), Some("included-user"));

        let cfg2 = load_host_config_from(&config, "main-host");
        assert_eq!(cfg2.hostname.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn test_unknown_directives_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(
            dir.path(),
            "config",
            "\
Host testhost
    HostName test.example.com
    FooBarBaz some_value
    SendEnv LANG LC_*
    ForwardAgent no
    User testuser
",
        );

        let cfg = load_host_config_from(&config, "testhost");
        assert_eq!(cfg.hostname.as_deref(), Some("test.example.com"));
        assert_eq!(cfg.user.as_deref(), Some("testuser"));
    }

    #[test]
    fn test_equals_syntax() {
        let dir = tempfile::tempdir().unwrap();
        let config = write_config(
            dir.path(),
            "config",
            "\
Host eqhost
    HostName=eq.example.com
    Port=8022
    User=equser
",
        );

        let cfg = load_host_config_from(&config, "eqhost");
        assert_eq!(cfg.hostname.as_deref(), Some("eq.example.com"));
        assert_eq!(cfg.port, Some(8022));
        assert_eq!(cfg.user.as_deref(), Some("equser"));
    }

    #[test]
    fn test_global_defaults() {
        let dir = tempfile::tempdir().unwrap();
        // In real SSH config, Host * usually goes at the bottom.
        // Directives before any Host block are treated as implicit "Host *"
        // and first-match-wins applies, so put specific hosts first.
        let config = write_config(
            dir.path(),
            "config",
            "\
Host specific
    HostName specific.example.com
    User specificuser

Host *
    User globaluser
    ServerAliveInterval 30
",
        );

        // specific host: User from Host block (first match), interval from *
        let cfg = load_host_config_from(&config, "specific");
        assert_eq!(cfg.user.as_deref(), Some("specificuser"));
        assert_eq!(cfg.server_alive_interval, Some(30));

        // unknown host: gets * defaults
        let cfg2 = load_host_config_from(&config, "unknown");
        assert_eq!(cfg2.user.as_deref(), Some("globaluser"));
        assert_eq!(cfg2.server_alive_interval, Some(30));
    }

    #[test]
    fn test_glob_match() {
        assert!(super::glob_match("*", "anything"));
        assert!(super::glob_match("prod-*", "prod-db"));
        assert!(super::glob_match("prod-*", "prod-"));
        assert!(!super::glob_match("prod-*", "staging-db"));
        assert!(super::glob_match("*.example.com", "foo.example.com"));
        assert!(super::glob_match("?oo", "foo"));
        assert!(!super::glob_match("?oo", "fooo"));
        assert!(super::glob_match("server", "server"));
        assert!(!super::glob_match("server", "servers"));
    }

    #[test]
    fn test_missing_config_file() {
        let cfg = load_host_config_from(Path::new("/nonexistent/path/config"), "anyhost");
        assert_eq!(cfg.hostname, None);
        assert_eq!(cfg.user, None);
    }
}
