// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Detect existing shellkeep tmux sessions on a remote server.

use std::process::Command;

use super::connection;

/// Check if a session name belongs to shellkeep.
/// Matches both old format ("shellkeep-N") and new format ("<client-id>--shellkeep-YYYYMMDD-HHMMSS").
fn is_shellkeep_session(name: &str) -> bool {
    name.starts_with("shellkeep-") || name.contains("--shellkeep-")
}

/// List existing shellkeep tmux sessions on a remote server.
///
/// Runs `ssh <args> "tmux list-sessions -F '#{session_name}'"` and
/// filters for shellkeep sessions (old and new naming formats).
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
                .filter(|s| is_shellkeep_session(s))
                .collect();
            sessions.sort();
            sessions
        }
        _ => Vec::new(),
    }
}

/// List existing shellkeep tmux sessions using a russh connection.
/// This is the async version — preferred over the blocking `list_remote_sessions`.
pub async fn list_sessions_russh(
    handle: &russh::client::Handle<connection::SshHandler>,
) -> Vec<String> {
    match connection::exec_command(
        handle,
        "tmux list-sessions -F '#{session_name}' 2>/dev/null",
    )
    .await
    {
        Ok(stdout) => {
            let mut sessions: Vec<String> = stdout
                .lines()
                .map(|l| l.trim().trim_matches('\'').to_string())
                .filter(|s| is_shellkeep_session(s))
                .collect();
            sessions.sort();
            sessions
        }
        Err(_) => Vec::new(),
    }
}

/// Create a tmux session via russh exec.
pub async fn create_session_russh(
    handle: &russh::client::Handle<connection::SshHandler>,
    session_name: &str,
) -> Result<(), connection::SshError> {
    let cmd = format!(
        "TERM=xterm-256color tmux new-session -d -s {session_name} \\; set status off 2>/dev/null || true"
    );
    connection::exec_command(handle, &cmd).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_prefix_filter() {
        let lines = "shellkeep-0\nshellkeep-1\nother-session\nshellkeep-5\nmylaptop--shellkeep-20260329-120000\n";
        let sessions: Vec<String> = lines
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|s| is_shellkeep_session(s))
            .collect();
        assert_eq!(
            sessions,
            vec![
                "shellkeep-0",
                "shellkeep-1",
                "shellkeep-5",
                "mylaptop--shellkeep-20260329-120000"
            ]
        );
    }
}
