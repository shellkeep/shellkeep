// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Detect existing shellkeep tmux sessions on a remote server.

use super::connection;

/// Check if a session name belongs to shellkeep (but NOT the lock session).
/// FR-SESSION-04: current format is `shellkeep--<workspace-uuid>--<session-uuid>`.
/// Also matches legacy formats for backward compatibility.
/// FR-LOCK-11: lock sessions ("shellkeep-lock*") are never treated as terminal sessions.
fn is_shellkeep_session(name: &str) -> bool {
    if name == "shellkeep-lock" || name.starts_with("shellkeep-lock-") {
        return false;
    }
    if name.contains("--shellkeep-lock") {
        return false;
    }
    name.starts_with("shellkeep--")
        || name.starts_with("shellkeep-")
        || name.contains("--shellkeep-")
}

/// FR-ENV-02: filter sessions by workspace UUID.
/// Returns sessions matching `shellkeep--<workspace-uuid>--` prefix,
/// plus legacy sessions matching `<workspace-name>--shellkeep-` prefix.
pub fn filter_sessions_by_workspace(
    sessions: &[String],
    workspace_uuid: &str,
    workspace_name: &str,
) -> Vec<String> {
    let uuid_prefix = format!("shellkeep--{workspace_uuid}--");
    let legacy_prefix = format!("{workspace_name}--shellkeep-");
    sessions
        .iter()
        .filter(|s| s.starts_with(&uuid_prefix) || s.starts_with(&legacy_prefix))
        .cloned()
        .collect()
}

/// FR-SESSION-04: generate a tmux session name.
/// Format: `shellkeep--<workspace-uuid>--<session-uuid>`
pub fn make_tmux_session_name(workspace_uuid: &str, session_uuid: &str) -> String {
    format!("shellkeep--{workspace_uuid}--{session_uuid}")
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
            tracing::debug!("found {} shellkeep tmux sessions", sessions.len());
            sessions
        }
        Err(e) => {
            tracing::warn!("failed to list tmux sessions: {e}");
            Vec::new()
        }
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
    tracing::info!("created tmux session: {session_name}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_prefix_filter() {
        let lines = "shellkeep--abc-123--def-456\nshellkeep-0\nother-session\nDefault--shellkeep-20260329-130000\n";
        let sessions: Vec<String> = lines
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|s| is_shellkeep_session(s))
            .collect();
        assert_eq!(
            sessions,
            vec![
                "shellkeep--abc-123--def-456",
                "shellkeep-0",
                "Default--shellkeep-20260329-130000",
            ]
        );
    }

    #[test]
    fn filter_by_workspace() {
        let ws_uuid = "aaaa-bbbb";
        let sessions = vec![
            format!("shellkeep--{ws_uuid}--sess-1"),
            format!("shellkeep--{ws_uuid}--sess-2"),
            "shellkeep--other-uuid--sess-3".to_string(),
            // Legacy format
            "Default--shellkeep-20260329-120000".to_string(),
            "ProjectA--shellkeep-20260329-130000".to_string(),
        ];
        let filtered = filter_sessions_by_workspace(&sessions, ws_uuid, "Default");
        assert_eq!(filtered.len(), 3); // 2 UUID + 1 legacy
        let proj_a = filter_sessions_by_workspace(&sessions, "no-match", "ProjectA");
        assert_eq!(proj_a.len(), 1);
        let empty = filter_sessions_by_workspace(&sessions, "no-match", "Nonexistent");
        assert!(empty.is_empty());
    }

    #[test]
    fn tmux_session_name_format() {
        let name = make_tmux_session_name("ws-uuid-123", "sess-uuid-456");
        assert_eq!(name, "shellkeep--ws-uuid-123--sess-uuid-456");
    }

    #[test]
    fn lock_sessions_excluded() {
        assert!(!is_shellkeep_session("shellkeep-lock"));
        assert!(!is_shellkeep_session("shellkeep-lock-my-client"));
        assert!(!is_shellkeep_session("my-client--shellkeep-lock-my-client"));
        // New UUID format matches
        assert!(is_shellkeep_session("shellkeep--abc--def"));
        // Legacy formats still match
        assert!(is_shellkeep_session("shellkeep-0"));
        assert!(is_shellkeep_session("Default--shellkeep-20260330-120000"));
    }
}
