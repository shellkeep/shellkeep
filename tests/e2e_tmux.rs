// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! E2E tests for tmux session management over SSH.
//!
//! Requires SSH access to the test server (209.38.150.61).
//! Run with: cargo test --test e2e_tmux -- --ignored

use std::process::Command;

const SSH_KEY: &str = concat!(env!("HOME"), "/.ssh/id_shellkeep");
const SSH_HOST: &str = "root@209.38.150.61";

fn ssh_args() -> Vec<String> {
    vec![
        "-i".to_string(),
        SSH_KEY.to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
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

fn cleanup_sessions() {
    // Kill all shellkeep-test-* sessions
    let _ = ssh_run("tmux kill-server 2>/dev/null; true");
    std::thread::sleep(std::time::Duration::from_millis(500));
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
    cleanup_sessions();

    // Create a session
    assert!(ssh_run_ok("tmux new-session -d -s shellkeep-test-0"));

    // Verify it exists
    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    assert!(
        sessions.contains("shellkeep-test-0"),
        "session not found: {sessions}"
    );

    // Clean up
    let _ = ssh_run("tmux kill-session -t shellkeep-test-0");
}

#[test]
#[ignore]
fn test_create_multiple_sessions() {
    cleanup_sessions();

    // Create 3 sessions
    for i in 0..3 {
        assert!(ssh_run_ok(&format!(
            "tmux new-session -d -s shellkeep-test-{i}"
        )));
    }

    // List and filter
    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    let shellkeep_sessions: Vec<&str> = sessions
        .lines()
        .filter(|l| l.starts_with("shellkeep-test-"))
        .collect();

    assert_eq!(shellkeep_sessions.len(), 3);
    assert!(shellkeep_sessions.contains(&"shellkeep-test-0"));
    assert!(shellkeep_sessions.contains(&"shellkeep-test-1"));
    assert!(shellkeep_sessions.contains(&"shellkeep-test-2"));

    cleanup_sessions();
}

#[test]
#[ignore]
fn test_session_survives_disconnect() {
    cleanup_sessions();

    // Create a session and run a command in it
    assert!(ssh_run_ok("tmux new-session -d -s shellkeep-test-persist"));
    assert!(ssh_run_ok(
        "tmux send-keys -t shellkeep-test-persist 'echo PERSIST_MARKER' Enter"
    ));

    // Wait for command to execute
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Verify session still exists (simulating disconnect + reconnect)
    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    assert!(
        sessions.contains("shellkeep-test-persist"),
        "session lost after simulated disconnect"
    );

    // Verify the command output is in the session
    let capture = ssh_run("tmux capture-pane -t shellkeep-test-persist -p 2>/dev/null");
    assert!(
        capture.contains("PERSIST_MARKER"),
        "command output lost: {capture}"
    );

    cleanup_sessions();
}

#[test]
#[ignore]
fn test_reattach_to_session() {
    cleanup_sessions();

    // Create a session
    assert!(ssh_run_ok("tmux new-session -d -s shellkeep-test-reattach"));

    // Verify the session exists (simulates what shellkeep checks before reattach)
    assert!(ssh_run_ok("tmux has-session -t shellkeep-test-reattach"));

    // Session should be exactly one with that name
    let sessions = ssh_run("tmux list-sessions -F '#{session_name}' 2>/dev/null");
    let count = sessions
        .lines()
        .filter(|l| *l == "shellkeep-test-reattach")
        .count();
    assert_eq!(count, 1, "expected 1 session, got {count}");

    cleanup_sessions();
}

#[test]
#[ignore]
fn test_list_remote_sessions_function() {
    cleanup_sessions();

    // Create some sessions
    for i in 0..3 {
        assert!(ssh_run_ok(&format!("tmux new-session -d -s shellkeep-{i}")));
    }
    // Also create a non-shellkeep session
    assert!(ssh_run_ok("tmux new-session -d -s other-session"));

    // Test our detection function
    let sessions = shellkeep::ssh::tmux::list_remote_sessions(&ssh_args());
    assert_eq!(
        sessions.len(),
        3,
        "expected 3 shellkeep sessions, got {:?}",
        sessions
    );
    assert!(sessions.contains(&"shellkeep-0".to_string()));
    assert!(sessions.contains(&"shellkeep-1".to_string()));
    assert!(sessions.contains(&"shellkeep-2".to_string()));
    // Should NOT include 'other-session'
    assert!(!sessions.iter().any(|s| s == "other-session"));

    cleanup_sessions();
}
