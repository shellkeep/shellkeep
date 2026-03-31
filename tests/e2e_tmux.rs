// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! E2E tests for tmux session management over SSH.
//!
//! Requires SSH access to the test server (209.38.150.61).
//! Run with: cargo test --test e2e_tmux -- --ignored --test-threads=1

use std::process::Command;

fn ssh_key_path() -> String {
    dirs::home_dir()
        .map(|h| h.join(".ssh").join("id_shellkeep").display().to_string())
        .unwrap_or_else(|| "/root/.ssh/id_shellkeep".to_string())
}
const SSH_HOST: &str = "root@209.38.150.61";

fn ssh_args() -> Vec<String> {
    vec![
        "-i".to_string(),
        ssh_key_path(),
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        SSH_HOST.to_string(),
    ]
}

fn ssh_run(cmd: &str) -> String {
    let output = Command::new("ssh")
        .args(&ssh_args())
        .arg(cmd)
        .output()
        .expect("failed to run ssh");
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn ssh_run_ok(cmd: &str) -> bool {
    Command::new("ssh")
        .args(&ssh_args())
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a unique prefix for this test run to avoid collisions.
fn test_prefix() -> String {
    format!(
        "sktest-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            % 100000
    )
}

fn cleanup_prefix(prefix: &str) {
    // Kill sessions matching this prefix only
    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    for session in sessions.lines() {
        let name = session.trim();
        if name.starts_with(prefix) {
            let _ = ssh_run(&format!("tmux kill-session -t {name} 2>/dev/null"));
        }
    }
}

#[test]
#[ignore] // requires SSH access
fn test_ssh_connectivity() {
    let result = ssh_run("echo hello");
    assert_eq!(result.trim(), "hello");
}

#[test]
#[ignore]
fn test_tmux_available() {
    let result = ssh_run("tmux -V");
    assert!(result.starts_with("tmux"), "tmux not available: {result}");
}

#[test]
#[ignore]
fn test_create_tmux_session() {
    let prefix = test_prefix();
    let session_name = format!("{prefix}-create");

    assert!(
        ssh_run_ok(&format!("tmux new-session -d -s {session_name}")),
        "failed to create session"
    );

    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    assert!(
        sessions.contains(&session_name),
        "session not found: {sessions}"
    );

    cleanup_prefix(&prefix);
}

#[test]
#[ignore]
fn test_create_multiple_sessions() {
    let prefix = test_prefix();

    for i in 0..3 {
        assert!(ssh_run_ok(&format!(
            "tmux new-session -d -s {prefix}-multi-{i}"
        )));
    }

    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    let matched: Vec<&str> = sessions
        .lines()
        .filter(|l| l.starts_with(&format!("{prefix}-multi-")))
        .collect();

    assert_eq!(matched.len(), 3, "expected 3 sessions, got {:?}", matched);

    cleanup_prefix(&prefix);
}

#[test]
#[ignore]
fn test_session_survives_disconnect() {
    let prefix = test_prefix();
    let session_name = format!("{prefix}-persist");

    assert!(ssh_run_ok(&format!(
        "tmux new-session -d -s {session_name}"
    )));
    assert!(ssh_run_ok(&format!(
        "tmux send-keys -t {session_name} 'echo PERSIST_MARKER' Enter"
    )));

    std::thread::sleep(std::time::Duration::from_millis(500));

    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    assert!(
        sessions.contains(&session_name),
        "session lost after simulated disconnect"
    );

    let capture = ssh_run(&format!(
        "tmux capture-pane -t {session_name} -p 2>/dev/null"
    ));
    assert!(
        capture.contains("PERSIST_MARKER"),
        "command output lost: {capture}"
    );

    cleanup_prefix(&prefix);
}

#[test]
#[ignore]
fn test_reattach_to_session() {
    let prefix = test_prefix();
    let session_name = format!("{prefix}-reattach");

    assert!(ssh_run_ok(&format!(
        "tmux new-session -d -s {session_name}"
    )));
    assert!(ssh_run_ok(&format!("tmux has-session -t {session_name}")));

    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    let count = sessions
        .lines()
        .filter(|l| l.trim() == session_name)
        .count();
    assert_eq!(count, 1, "expected 1 session, got {count}");

    cleanup_prefix(&prefix);
}

// test_list_remote_sessions_function removed — blocking system ssh version
// replaced by async list_sessions_russh tested in e2e_features.rs
