// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Detect existing shellkeep tmux sessions on a remote server.

use std::process::Command;

const SESSION_PREFIX: &str = "shellkeep-";

/// List existing shellkeep tmux sessions on a remote server.
///
/// Runs `ssh <args> "tmux list-sessions -F '#{session_name}'"` and
/// filters for sessions starting with "shellkeep-".
///
/// Returns session names sorted, or empty vec if tmux is not available.
pub fn list_remote_sessions(ssh_args: &[String]) -> Vec<String> {
    let mut cmd = Command::new("ssh");
    for arg in ssh_args {
        cmd.arg(arg);
    }
    cmd.arg("-o").arg("BatchMode=yes");
    cmd.arg("-o").arg("ConnectTimeout=5");
    cmd.arg("tmux list-sessions -F '#{session_name}' 2>/dev/null");

    match cmd.output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut sessions: Vec<String> = stdout
                .lines()
                .map(|l| l.trim().trim_matches('\'').to_string())
                .filter(|s| s.starts_with(SESSION_PREFIX))
                .collect();
            sessions.sort();
            sessions
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_prefix_filter() {
        let lines = "shellkeep-0\nshellkeep-1\nother-session\nshellkeep-5\n";
        let sessions: Vec<String> = lines
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|s| s.starts_with(SESSION_PREFIX))
            .collect();
        assert_eq!(sessions, vec!["shellkeep-0", "shellkeep-1", "shellkeep-5"]);
    }
}
