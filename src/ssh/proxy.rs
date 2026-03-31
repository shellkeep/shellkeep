// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! ProxyCommand and ProxyJump support (FR-PROXY-01, FR-PROXY-02, FR-PROXY-03).
//!
//! ProxyCommand spawns a local process and uses its stdin/stdout as the SSH
//! transport. ProxyJump is syntactic sugar: `ProxyJump bastion` becomes
//! `ProxyCommand ssh -W %h:%p bastion`.

use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};

use tokio::io::{self, AsyncRead, AsyncWrite, ReadBuf};
use tokio::process::{Child, Command};

/// A bidirectional stream backed by a child process's stdin/stdout.
///
/// Reads come from the child's stdout, writes go to the child's stdin.
/// When dropped, the child process is killed.
pub struct ProxyStream {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
}

impl ProxyStream {
    /// Spawn a proxy command and return the bidirectional stream.
    ///
    /// The command string is passed to `sh -c` after substituting:
    /// - `%h` → target hostname
    /// - `%p` → target port
    /// - `%r` → target username (if provided)
    /// - `%%` → literal `%`
    pub async fn spawn(
        proxy_cmd: &str,
        host: &str,
        port: u16,
        username: Option<&str>,
    ) -> Result<Self, ProxyError> {
        let cmd = substitute_tokens(proxy_cmd, host, port, username);
        tracing::info!("proxy: spawning command: {cmd}");

        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ProxyError::Spawn(cmd.clone(), e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProxyError::Spawn(cmd.clone(), "failed to capture stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProxyError::Spawn(cmd.clone(), "failed to capture stdout".into()))?;

        Ok(ProxyStream {
            child,
            stdin,
            stdout,
        })
    }
}

impl AsyncRead for ProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdout).poll_read(cx, buf)
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stdin).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stdin).poll_shutdown(cx)
    }
}

impl Drop for ProxyStream {
    fn drop(&mut self) {
        // Best-effort kill — don't block on the result
        let _ = self.child.start_kill();
    }
}

/// Convert a ProxyJump host specification to a ProxyCommand string.
///
/// `ProxyJump bastion` → `ssh -W %h:%p bastion`
/// `ProxyJump user@bastion:2222` → `ssh -W %h:%p -p 2222 user@bastion`
pub fn proxy_jump_to_command(jump_spec: &str) -> String {
    // Parse optional user@ prefix and :port suffix
    let (userhost, port) = if let Some(colon_pos) = jump_spec.rfind(':') {
        let port_str = &jump_spec[colon_pos + 1..];
        if port_str.parse::<u16>().is_ok() {
            (&jump_spec[..colon_pos], Some(port_str))
        } else {
            (jump_spec, None)
        }
    } else {
        (jump_spec, None)
    };

    let mut cmd = format!("ssh -W %h:%p {userhost}");
    if let Some(p) = port {
        cmd = format!("ssh -W %h:%p -p {p} {userhost}");
    }
    cmd
}

/// Resolve proxy configuration into a ProxyCommand string, if any.
///
/// ProxyJump takes precedence over ProxyCommand (matching OpenSSH behavior).
/// Returns `None` if neither is configured.
pub fn resolve_proxy_command(
    proxy_jump: Option<&str>,
    proxy_command: Option<&str>,
) -> Option<String> {
    // "none" disables proxy (OpenSSH convention)
    if let Some(pj) = proxy_jump {
        if pj.eq_ignore_ascii_case("none") {
            return None;
        }
        return Some(proxy_jump_to_command(pj));
    }
    if let Some(pc) = proxy_command {
        if pc.eq_ignore_ascii_case("none") {
            return None;
        }
        return Some(pc.to_string());
    }
    None
}

/// Substitute SSH config tokens in a proxy command string.
fn substitute_tokens(cmd: &str, host: &str, port: u16, username: Option<&str>) -> String {
    let port_str = port.to_string();
    let user = username.unwrap_or("");
    let mut result = String::with_capacity(cmd.len());
    let mut chars = cmd.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some(&'h') => {
                    result.push_str(host);
                    chars.next();
                }
                Some(&'p') => {
                    result.push_str(&port_str);
                    chars.next();
                }
                Some(&'r') => {
                    result.push_str(user);
                    chars.next();
                }
                Some(&'%') => {
                    result.push('%');
                    chars.next();
                }
                _ => result.push('%'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Errors specific to proxy connections. /* FR-PROXY-03 */
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    /// Failed to spawn the proxy command process.
    #[error("proxy command failed to start: {1} (command: {0})")]
    Spawn(String, String),
    /// Proxy command exited before connection could be established.
    #[error("proxy connection failed: could not reach {0}")]
    ProxyFailed(String),
    /// Proxy connected but the target host was unreachable.
    #[error("proxy connected to {0}, but target {1} is unreachable")]
    TargetUnreachable(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_tokens() {
        assert_eq!(
            substitute_tokens("ssh -W %h:%p bastion", "target.com", 22, None),
            "ssh -W target.com:22 bastion"
        );
        assert_eq!(
            substitute_tokens("ssh -W %h:%p -l %r bastion", "host", 2222, Some("admin")),
            "ssh -W host:2222 -l admin bastion"
        );
        assert_eq!(
            substitute_tokens("echo %%h is %h", "foo", 22, None),
            "echo %h is foo"
        );
    }

    #[test]
    fn test_proxy_jump_to_command_simple() {
        assert_eq!(proxy_jump_to_command("bastion"), "ssh -W %h:%p bastion");
    }

    #[test]
    fn test_proxy_jump_to_command_with_user() {
        assert_eq!(
            proxy_jump_to_command("admin@bastion"),
            "ssh -W %h:%p admin@bastion"
        );
    }

    #[test]
    fn test_proxy_jump_to_command_with_port() {
        assert_eq!(
            proxy_jump_to_command("bastion:2222"),
            "ssh -W %h:%p -p 2222 bastion"
        );
    }

    #[test]
    fn test_proxy_jump_to_command_with_user_and_port() {
        assert_eq!(
            proxy_jump_to_command("admin@bastion:2222"),
            "ssh -W %h:%p -p 2222 admin@bastion"
        );
    }

    #[test]
    fn test_resolve_proxy_command_jump_takes_precedence() {
        let result = resolve_proxy_command(Some("bastion"), Some("nc %h %p"));
        assert_eq!(result, Some("ssh -W %h:%p bastion".to_string()));
    }

    #[test]
    fn test_resolve_proxy_command_none_disables() {
        assert_eq!(resolve_proxy_command(Some("none"), None), None);
        assert_eq!(resolve_proxy_command(None, Some("none")), None);
        assert_eq!(resolve_proxy_command(Some("NONE"), None), None);
    }

    #[test]
    fn test_resolve_proxy_command_returns_none_when_empty() {
        assert_eq!(resolve_proxy_command(None, None), None);
    }

    #[test]
    fn test_resolve_proxy_command_uses_proxy_command() {
        let result = resolve_proxy_command(None, Some("nc %h %p"));
        assert_eq!(result, Some("nc %h %p".to_string()));
    }
}
