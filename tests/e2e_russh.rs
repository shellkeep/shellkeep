// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! E2E tests for russh SSH connection.
//!
//! Run with: cargo test --test e2e_russh -- --ignored

const SSH_KEY: &str = concat!(env!("HOME"), "/.ssh/id_shellkeep");
const SSH_HOST: &str = "209.38.150.61";
const SSH_PORT: u16 = 22;
const SSH_USER: &str = "root";

#[tokio::test]
#[ignore]
async fn test_russh_connect() {
    let handle =
        shellkeep::ssh::connection::connect(SSH_HOST, SSH_PORT, SSH_USER, Some(SSH_KEY), 15)
            .await
            .expect("failed to connect");

    // Verify we can run a command
    let output = shellkeep::ssh::connection::exec_command(&handle, "echo hello_russh")
        .await
        .expect("exec failed");

    assert!(
        output.trim() == "hello_russh",
        "unexpected output: {output}"
    );
}

#[tokio::test]
#[ignore]
async fn test_russh_list_tmux_sessions() {
    let handle =
        shellkeep::ssh::connection::connect(SSH_HOST, SSH_PORT, SSH_USER, Some(SSH_KEY), 15)
            .await
            .expect("failed to connect");

    // Clean up any existing sessions
    let _ =
        shellkeep::ssh::connection::exec_command(&handle, "tmux kill-server 2>/dev/null || true")
            .await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Create some test sessions
    for i in 0..3 {
        shellkeep::ssh::tmux::create_session_russh(&handle, &format!("shellkeep-test-{i}"))
            .await
            .expect("failed to create session");
    }

    // List sessions
    let sessions = shellkeep::ssh::tmux::list_sessions_russh(&handle).await;
    assert_eq!(sessions.len(), 3, "expected 3 sessions, got {:?}", sessions);

    // Clean up
    let _ =
        shellkeep::ssh::connection::exec_command(&handle, "tmux kill-server 2>/dev/null || true")
            .await;
}

#[tokio::test]
#[ignore]
async fn test_russh_open_shell() {
    let handle =
        shellkeep::ssh::connection::connect(SSH_HOST, SSH_PORT, SSH_USER, Some(SSH_KEY), 15)
            .await
            .expect("failed to connect");

    let mut channel = shellkeep::ssh::connection::open_shell(&handle, 80, 24)
        .await
        .expect("failed to open shell");

    // Send a command
    channel
        .data("echo russh_shell_test\n".as_bytes())
        .await
        .expect("write failed");

    // Read output
    let mut output = Vec::new();
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        output.extend_from_slice(&data);
                        let s = String::from_utf8_lossy(&output);
                        if s.contains("russh_shell_test") {
                            break;
                        }
                    }
                    None => break,
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
        output_str.contains("russh_shell_test"),
        "shell output missing test string: {output_str}"
    );
}
