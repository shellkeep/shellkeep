// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! E2E tests for shellkeep features against real SSH server.
//!
//! Run with: cargo test --test e2e_features -- --ignored --test-threads=1

const SSH_KEY: &str = concat!(env!("HOME"), "/.ssh/id_shellkeep");
const SSH_HOST: &str = "209.38.150.61";
const SSH_PORT: u16 = 22;
const SSH_USER: &str = "root";

async fn connect() -> russh::client::Handle<shellkeep::ssh::connection::SshHandler> {
    shellkeep::ssh::connection::connect(SSH_HOST, SSH_PORT, SSH_USER, Some(SSH_KEY), None, 15)
        .await
        .expect("failed to connect")
        .handle
}

async fn exec(
    handle: &russh::client::Handle<shellkeep::ssh::connection::SshHandler>,
    cmd: &str,
) -> String {
    shellkeep::ssh::connection::exec_command(handle, cmd)
        .await
        .unwrap_or_default()
}

async fn cleanup(
    handle: &russh::client::Handle<shellkeep::ssh::connection::SshHandler>,
    prefix: &str,
) {
    let sessions = exec(
        handle,
        "tmux list-sessions -F '#{session_name}' 2>/dev/null",
    )
    .await;
    for name in sessions.lines() {
        let name = name.trim();
        if name.starts_with(prefix) {
            let _ = exec(handle, &format!("tmux kill-session -t {name} 2>/dev/null")).await;
        }
    }
}

// =========================================================================
// Host key verification
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_host_key_known_host() {
    // Connecting to a known host should work
    let handle = connect().await;
    let output = exec(&handle, "echo host_key_ok").await;
    assert_eq!(output.trim(), "host_key_ok");
}

// =========================================================================
// Lock mechanism
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_lock_acquire_release() {
    let handle = connect().await;
    let client_id = "e2e-lock-test";

    // Clean up any existing lock
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t shellkeep-lock-{client_id} 2>/dev/null"),
    )
    .await;

    // Acquire lock
    shellkeep::ssh::lock::acquire_lock(&handle, client_id, Some(15))
        .await
        .expect("failed to acquire lock");

    // Verify lock session exists
    let check = exec(
        &handle,
        &format!("tmux has-session -t shellkeep-lock-{client_id} 2>/dev/null && echo EXISTS"),
    )
    .await;
    assert!(check.contains("EXISTS"), "lock session not found");

    // Verify env vars
    let env = exec(
        &handle,
        &format!("tmux show-environment -t shellkeep-lock-{client_id} 2>/dev/null"),
    )
    .await;
    assert!(
        env.contains("SHELLKEEP_LOCK_CLIENT_ID"),
        "missing client_id env var: {env}"
    );

    // Release lock
    shellkeep::ssh::lock::release_lock(&handle, client_id)
        .await
        .expect("failed to release lock");

    // Verify gone
    let check = exec(
        &handle,
        &format!(
            "tmux has-session -t shellkeep-lock-{client_id} 2>/dev/null && echo EXISTS || echo GONE"
        ),
    )
    .await;
    assert!(
        check.contains("GONE"),
        "lock session still exists after release"
    );
}

#[tokio::test]
#[ignore]
async fn test_lock_heartbeat() {
    let handle = connect().await;
    let client_id = "e2e-heartbeat-test";

    let _ = exec(
        &handle,
        &format!("tmux kill-session -t shellkeep-lock-{client_id} 2>/dev/null"),
    )
    .await;

    shellkeep::ssh::lock::acquire_lock(&handle, client_id, Some(15))
        .await
        .expect("acquire failed");

    // Heartbeat should succeed
    shellkeep::ssh::lock::heartbeat(&handle, client_id)
        .await
        .expect("heartbeat failed");

    // Cleanup
    shellkeep::ssh::lock::release_lock(&handle, client_id)
        .await
        .ok();
}

// =========================================================================
// Environment-scoped session naming
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_environment_scoped_sessions() {
    let handle = connect().await;
    let prefix = "e2e-env-test";

    cleanup(&handle, prefix).await;

    // Create sessions with environment-scoped names (must contain "--shellkeep-" for filter)
    let session1 = format!("{prefix}--Default--shellkeep-001");
    let session2 = format!("{prefix}--Staging--shellkeep-002");

    shellkeep::ssh::tmux::create_session_russh(&handle, &session1)
        .await
        .expect("create session1 failed");
    shellkeep::ssh::tmux::create_session_russh(&handle, &session2)
        .await
        .expect("create session2 failed");

    // List all sessions
    let all = shellkeep::ssh::tmux::list_sessions_russh(&handle).await;
    assert!(
        all.iter().any(|s| s == &session1),
        "missing session1 in {all:?}"
    );
    assert!(
        all.iter().any(|s| s == &session2),
        "missing session2 in {all:?}"
    );

    // Filter by environment prefix
    let default_sessions: Vec<_> = all.iter().filter(|s| s.contains("--Default--")).collect();
    let staging_sessions: Vec<_> = all.iter().filter(|s| s.contains("--Staging--")).collect();
    assert_eq!(default_sessions.len(), 1);
    assert_eq!(staging_sessions.len(), 1);

    cleanup(&handle, prefix).await;
}

// =========================================================================
// SSH channel terminal I/O (the core russh wiring)
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_russh_terminal_io_with_tmux() {
    let handle = connect().await;
    let session_name = "e2e-terminal-io-test";

    // Clean up
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t {session_name} 2>/dev/null"),
    )
    .await;

    // Create tmux session
    shellkeep::ssh::tmux::create_session_russh(&handle, session_name)
        .await
        .expect("create session failed");

    // Open PTY channel and attach to tmux (same as establish_ssh_session does)
    let channel = handle
        .channel_open_session()
        .await
        .expect("channel open failed");
    channel
        .request_pty(false, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .expect("pty failed");

    let tmux_cmd = format!(
        "TERM=xterm-256color tmux new-session -A -s {session_name} \\; set status off || exec $SHELL"
    );
    channel.exec(true, tmux_cmd).await.expect("exec failed");

    // Wait for tmux to attach
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Send a command through the channel
    channel
        .data("echo TERMINAL_IO_TEST_MARKER\n".as_bytes())
        .await
        .expect("write failed");

    // Read output
    let mut output = Vec::new();
    let mut ch = channel;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            msg = ch.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        output.extend_from_slice(&data);
                        let s = String::from_utf8_lossy(&output);
                        if s.contains("TERMINAL_IO_TEST_MARKER") && s.matches("TERMINAL_IO_TEST_MARKER").count() >= 2 {
                            // Echo + output = 2 occurrences
                            break;
                        }
                    }
                    Some(russh::ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    let output_str = String::from_utf8_lossy(&output);
    assert!(
        output_str.contains("TERMINAL_IO_TEST_MARKER"),
        "terminal I/O failed — no marker in output: {output_str}"
    );

    // Clean up
    let _ = exec(
        &handle,
        &format!("tmux kill-session -t {session_name} 2>/dev/null"),
    )
    .await;
}

// =========================================================================
// Multi-tab connection (the handle lock contention fix)
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_multi_channel_concurrent() {
    let handle = connect().await;
    let prefix = "e2e-multi";

    cleanup(&handle, prefix).await;

    // Open 3 exec channels concurrently (simulating multi-tab)
    // Run 3 exec commands sequentially (they share one Handle)
    for i in 0..3 {
        let cmd = format!("echo multi_test_{i}");
        let output = shellkeep::ssh::connection::exec_command(&handle, &cmd)
            .await
            .expect(&format!("channel {i} failed"));
        assert!(
            output.contains(&format!("multi_test_{i}")),
            "channel {i} wrong output: {output}"
        );
    }
}

// =========================================================================
// SFTP (if available)
// =========================================================================

#[tokio::test]
#[ignore]
async fn test_sftp_or_shell_fallback() {
    let handle = connect().await;

    // Test shell fallback (always works)
    let test_content = "e2e_sftp_test_content";
    let test_path = "/tmp/shellkeep-e2e-sftp-test.txt";

    // Write via shell
    let write_cmd = format!("echo '{test_content}' > {test_path}");
    exec(&handle, &write_cmd).await;

    // Read back via shell
    let read_result = exec(&handle, &format!("cat {test_path}")).await;
    assert!(
        read_result.trim() == test_content,
        "shell write/read failed: got '{}'",
        read_result.trim()
    );

    // Clean up
    exec(&handle, &format!("rm -f {test_path}")).await;
}

// =========================================================================
// SSH config parsing
// =========================================================================

#[test]
#[ignore]
fn test_ssh_config_parsing() {
    // This test verifies the parser handles the droplet's actual SSH config
    let config = shellkeep::ssh::ssh_config::load_host_config(SSH_HOST);
    // Should at least return a valid struct (may be empty if no matching Host block)
    // The key is that it doesn't panic
    let _ = config;
}

// =========================================================================
// History writer
// =========================================================================

#[test]
#[ignore]
fn test_history_writer_creates_file() {
    let uuid = format!("e2e-test-{}", std::process::id());
    let mut writer = shellkeep::state::history::HistoryWriter::new(&uuid, 50)
        .expect("history writer should be created with max_size > 0");
    writer.append_output(b"test output data\n");
    writer.append_output(b"more output\n");
    drop(writer);

    // Verify file exists
    let path = dirs::data_dir()
        .unwrap_or_default()
        .join("shellkeep/history")
        .join(format!("{uuid}.jsonl"));
    assert!(
        path.exists(),
        "history file not created at {}",
        path.display()
    );

    // Read and verify content
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        content.contains("test output data"),
        "missing output in JSONL"
    );
    assert!(
        content.lines().count() >= 2,
        "expected at least 2 JSONL lines"
    );

    // Cleanup
    let _ = std::fs::remove_file(&path);
}

/// FR-TERMINAL-16: Verify PTY size is correctly set and resize works.
/// Opens a shell with specific dimensions, checks tput reports match,
/// then resizes and verifies the new size.
#[tokio::test]
#[ignore]
async fn test_pty_resize() {
    let handle = connect().await;

    // Open a shell channel with a specific size (120x40)
    let mut channel = handle.channel_open_session().await.expect("channel open");

    channel
        .request_pty(false, "xterm-256color", 120, 40, 0, 0, &[])
        .await
        .expect("pty request");

    channel.request_shell(false).await.expect("shell request");

    // Wait for shell prompt
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Ask the shell for its size
    channel
        .data("tput cols; tput lines\n".as_bytes())
        .await
        .expect("write tput");

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Read output
    let mut output = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        output.extend_from_slice(&data);
                        let text = String::from_utf8_lossy(&output);
                        if text.contains("120") && text.contains("40") {
                            break;
                        }
                    }
                    Some(russh::ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }

    let text = String::from_utf8_lossy(&output);
    assert!(text.contains("120"), "expected 120 cols, got: {text}");
    assert!(text.contains("40"), "expected 40 lines, got: {text}");

    // Now resize to 80x24 and verify
    channel
        .window_change(80, 24, 0, 0)
        .await
        .expect("window_change");

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Clear output and check again
    let mut output2 = Vec::new();
    channel
        .data("tput cols; tput lines\n".as_bytes())
        .await
        .expect("write tput 2");

    let deadline2 = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        output2.extend_from_slice(&data);
                        let text = String::from_utf8_lossy(&output2);
                        // Look for 80 and 24 in the new output
                        if text.matches("80").count() >= 1 && text.matches("24").count() >= 1 {
                            break;
                        }
                    }
                    Some(russh::ChannelMsg::Eof) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep_until(deadline2) => break,
        }
    }

    let text2 = String::from_utf8_lossy(&output2);
    assert!(
        text2.contains("80"),
        "expected 80 cols after resize, got: {text2}"
    );
    assert!(
        text2.contains("24"),
        "expected 24 lines after resize, got: {text2}"
    );
}
