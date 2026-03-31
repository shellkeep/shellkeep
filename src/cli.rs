// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! CLI argument parsing and host input parsing.

// ---------------------------------------------------------------------------
// Host input parsing: supports user@host:port, user@host, host:port, host
// ---------------------------------------------------------------------------

pub(crate) fn parse_host_input(input: &str) -> (Option<String>, String, Option<String>) {
    let mut user = None;
    let mut remaining = input.to_string();

    // Extract user@
    if let Some(at_pos) = remaining.find('@') {
        user = Some(remaining[..at_pos].to_string());
        remaining = remaining[at_pos + 1..].to_string();
    }

    // Extract :port (but not IPv6 brackets)
    let port = if remaining.starts_with('[') {
        // IPv6: [::1]:port
        if let Some(bracket_end) = remaining.find(']') {
            let host = remaining[1..bracket_end].to_string();
            let port = if remaining.len() > bracket_end + 2
                && remaining.as_bytes()[bracket_end + 1] == b':'
            {
                Some(remaining[bracket_end + 2..].to_string())
            } else {
                None
            };
            remaining = host;
            port
        } else {
            None
        }
    } else if let Some(colon_pos) = remaining.rfind(':') {
        let maybe_port = &remaining[colon_pos + 1..];
        if maybe_port.parse::<u16>().is_ok() {
            let port = Some(maybe_port.to_string());
            remaining = remaining[..colon_pos].to_string();
            port
        } else {
            None
        }
    } else {
        None
    };

    (user, remaining, port)
}

/// Default SSH username — the current OS user.
pub(crate) fn default_ssh_username() -> String {
    whoami::username()
}

/// Parsed SSH connection arguments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedSshArgs {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) username: Option<String>,
    pub(crate) identity_file: Option<String>,
    /// The raw host argument as given (before parse_host_input splitting).
    pub(crate) label: String,
}

/// Parse SSH-style arguments (-p PORT, -i FILE, -l USER, [user@]host[:port]).
///
/// This is the single source of truth for parsing SSH CLI arguments, used by:
/// - CLI launch (`ShellKeep::new`)
/// - SSH command paste detection (`handle_input_message`)
/// - Test helpers
pub(crate) fn parse_ssh_args(args: &[&str]) -> ParsedSshArgs {
    let mut cli_port: Option<u16> = None;
    let mut cli_identity = None;
    let mut cli_user_flag = None;
    let mut flag_value_indices = std::collections::HashSet::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "-p" if i + 1 < args.len() => {
                cli_port = args[i + 1].parse().ok();
                flag_value_indices.insert(i);
                flag_value_indices.insert(i + 1);
                i += 1;
            }
            "-i" if i + 1 < args.len() => {
                cli_identity = Some(args[i + 1].to_string());
                flag_value_indices.insert(i);
                flag_value_indices.insert(i + 1);
                i += 1;
            }
            "-l" if i + 1 < args.len() => {
                cli_user_flag = Some(args[i + 1].to_string());
                flag_value_indices.insert(i);
                flag_value_indices.insert(i + 1);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let host_arg = args
        .iter()
        .enumerate()
        .find(|(idx, a)| !a.starts_with('-') && !flag_value_indices.contains(idx))
        .map(|(_, a)| (*a).to_string())
        .unwrap_or_default();
    let label = host_arg.clone();
    let (parsed_user, parsed_host, parsed_port) = parse_host_input(&host_arg);
    let port = parsed_port
        .and_then(|p| p.parse().ok())
        .or(cli_port)
        .unwrap_or(22);
    let username = cli_user_flag.or(parsed_user);
    ParsedSshArgs {
        host: parsed_host,
        port,
        username,
        identity_file: cli_identity,
        label,
    }
}

// ---------------------------------------------------------------------------
// CLI SSH arg filtering
// ---------------------------------------------------------------------------

/// Filter CLI args to extract SSH-relevant arguments, stripping shellkeep-specific
/// flags like --debug and --trace. Returns None if no host argument is present.
pub(crate) fn parse_cli_ssh_args(args: &[String]) -> Option<Vec<String>> {
    let ssh_relevant: Vec<String> = args
        .iter()
        .filter(|a| *a != "--debug" && *a != "--trace")
        .cloned()
        .collect();

    if ssh_relevant.is_empty() || ssh_relevant.iter().all(|a| a.starts_with('-')) {
        None
    } else {
        Some(ssh_relevant)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_simple() {
        let (user, host, port) = parse_host_input("example.com");
        assert_eq!(user, None);
        assert_eq!(host, "example.com");
        assert_eq!(port, None);
    }

    #[test]
    fn parse_host_with_user() {
        let (user, host, port) = parse_host_input("alice@example.com");
        assert_eq!(user, Some("alice".into()));
        assert_eq!(host, "example.com");
        assert_eq!(port, None);
    }

    #[test]
    fn parse_host_with_port() {
        let (user, host, port) = parse_host_input("example.com:2222");
        assert_eq!(user, None);
        assert_eq!(host, "example.com");
        assert_eq!(port, Some("2222".into()));
    }

    #[test]
    fn parse_host_full() {
        let (user, host, port) = parse_host_input("alice@example.com:2222");
        assert_eq!(user, Some("alice".into()));
        assert_eq!(host, "example.com");
        assert_eq!(port, Some("2222".into()));
    }

    #[test]
    fn parse_host_ipv6() {
        let (user, host, port) = parse_host_input("[::1]:2222");
        assert_eq!(user, None);
        assert_eq!(host, "::1");
        assert_eq!(port, Some("2222".into()));
    }

    /// Helper wrapping parse_ssh_args for test assertions.
    fn parse_cli_args(args: &[&str]) -> (String, u16, String, Option<String>) {
        let parsed = parse_ssh_args(args);
        let username = parsed
            .username
            .unwrap_or_else(|| "default_user".to_string());
        (parsed.host, parsed.port, username, parsed.identity_file)
    }

    #[test]
    fn cli_port_before_host() {
        // shellkeep -p 2247 tiago@example.com
        let (host, port, user, _) = parse_cli_args(&["-p", "2247", "tiago@example.com"]);
        assert_eq!(host, "example.com");
        assert_eq!(port, 2247);
        assert_eq!(user, "tiago");
    }

    #[test]
    fn cli_host_before_port() {
        // shellkeep tiago@example.com -p 2247
        let (host, port, user, _) = parse_cli_args(&["tiago@example.com", "-p", "2247"]);
        assert_eq!(host, "example.com");
        assert_eq!(port, 2247);
        assert_eq!(user, "tiago");
    }

    #[test]
    fn cli_identity_and_port() {
        // shellkeep -i /path/key -p 2222 user@host
        let (host, port, user, identity) =
            parse_cli_args(&["-i", "/path/key", "-p", "2222", "user@host"]);
        assert_eq!(host, "host");
        assert_eq!(port, 2222);
        assert_eq!(user, "user");
        assert_eq!(identity, Some("/path/key".to_string()));
    }

    #[test]
    fn cli_user_flag() {
        // shellkeep -l alice example.com
        let (host, port, user, _) = parse_cli_args(&["-l", "alice", "example.com"]);
        assert_eq!(host, "example.com");
        assert_eq!(port, 22);
        assert_eq!(user, "alice");
    }

    #[test]
    fn cli_host_with_colon_port() {
        // shellkeep user@example.com:3333
        let (host, port, user, _) = parse_cli_args(&["user@example.com:3333"]);
        assert_eq!(host, "example.com");
        assert_eq!(port, 3333);
        assert_eq!(user, "user");
    }

    #[test]
    fn cli_just_host() {
        // shellkeep example.com
        let (host, port, user, _) = parse_cli_args(&["example.com"]);
        assert_eq!(host, "example.com");
        assert_eq!(port, 22);
        assert_eq!(user, "default_user");
    }

    #[test]
    fn parse_cli_ssh_args_filters_debug() {
        let args: Vec<String> = vec!["--debug", "user@host"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = super::parse_cli_ssh_args(&args);
        assert_eq!(result, Some(vec!["user@host".to_string()]));
    }

    #[test]
    fn parse_cli_ssh_args_no_host() {
        let args: Vec<String> = vec!["--debug"].into_iter().map(String::from).collect();
        let result = super::parse_cli_ssh_args(&args);
        assert_eq!(result, None);
    }

    #[test]
    fn parse_cli_ssh_args_only_flags() {
        let args: Vec<String> = vec!["-v"].into_iter().map(String::from).collect();
        let result = super::parse_cli_ssh_args(&args);
        assert_eq!(result, None);
    }

    #[test]
    fn parse_cli_ssh_args_empty() {
        let args: Vec<String> = vec![];
        let result = super::parse_cli_ssh_args(&args);
        assert_eq!(result, None);
    }
}
