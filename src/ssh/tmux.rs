// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Detect existing shellkeep tmux sessions on a remote server.

use super::connection;

/// Check if a session name belongs to shellkeep (but NOT the lock session).
/// Matches old format ("shellkeep-N"), v1 format ("<client-id>--shellkeep-YYYYMMDD-HHMMSS"),
/// and v2 environment format ("<client-id>--<env>--<session-name>").
/// FR-LOCK-11: lock sessions ("shellkeep-lock-*") are never treated as terminal sessions.
fn is_shellkeep_session(name: &str) -> bool {
    if name.starts_with("shellkeep-lock-") || name.contains("--shellkeep-lock-") {
        return false;
    }
    name.starts_with("shellkeep-") || name.contains("--shellkeep-")
}

/// FR-ENV-02: filter sessions by client-id AND environment name.
/// Returns sessions matching `<client-id>--<env-name>--` prefix.
pub fn filter_sessions_by_env(sessions: &[String], client_id: &str, env_name: &str) -> Vec<String> {
    let prefix = format!("{client_id}--{env_name}--");
    sessions
        .iter()
        .filter(|s| s.starts_with(&prefix))
        .cloned()
        .collect()
}

/// FR-ENV-02: generate a tmux session name scoped to client + environment.
/// Format: `<client-id>--<env-name>--shellkeep-YYYYMMDD-HHMMSS`
pub fn env_tmux_session_name(client_id: &str, env_name: &str) -> String {
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    format!("{client_id}--{env_name}--shellkeep-{timestamp}")
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
        let lines = "shellkeep-0\nshellkeep-1\nother-session\nshellkeep-5\nmylaptop--shellkeep-20260329-120000\nmylaptop--Default--shellkeep-20260329-130000\n";
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
                "mylaptop--shellkeep-20260329-120000",
                "mylaptop--Default--shellkeep-20260329-130000",
            ]
        );
    }

    #[test]
    fn filter_by_env() {
        let sessions = vec![
            "laptop--Default--shellkeep-20260329-120000".to_string(),
            "laptop--Default--shellkeep-20260329-120100".to_string(),
            "laptop--ProjectA--shellkeep-20260329-130000".to_string(),
            "other--Default--shellkeep-20260329-140000".to_string(),
            "shellkeep-0".to_string(),
        ];
        let filtered = filter_sessions_by_env(&sessions, "laptop", "Default");
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|s| s.starts_with("laptop--Default--")));

        let proj_a = filter_sessions_by_env(&sessions, "laptop", "ProjectA");
        assert_eq!(proj_a.len(), 1);

        let empty = filter_sessions_by_env(&sessions, "laptop", "Nonexistent");
        assert!(empty.is_empty());
    }

    #[test]
    fn env_session_name_format() {
        let name = env_tmux_session_name("my-client", "Default");
        assert!(name.starts_with("my-client--Default--shellkeep-"));
        // Should contain a timestamp-like pattern
        assert!(name.len() > "my-client--Default--shellkeep-".len());
    }

    #[test]
    fn lock_sessions_excluded() {
        // FR-LOCK-11: lock sessions must never appear as regular sessions
        assert!(!is_shellkeep_session("shellkeep-lock-my-client"));
        assert!(!is_shellkeep_session("my-client--shellkeep-lock-my-client"));
        // Regular sessions still match
        assert!(is_shellkeep_session("shellkeep-0"));
        assert!(is_shellkeep_session(
            "my-client--Default--shellkeep-20260330-120000"
        ));
    }
}
