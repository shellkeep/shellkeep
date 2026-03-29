// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SSH connection management using russh.

use std::sync::Arc;

use russh::keys::PrivateKeyWithHashAlg;
use russh::keys::agent::client::AgentClient;
use russh::keys::ssh_key;
use ssh_key::Algorithm;

/// SSH connection handler for russh client events.
pub struct SshHandler {
    pub auto_accept_hosts: bool,
    pub host: String,
    pub port: u16,
}

#[derive(Debug)]
pub enum SshError {
    Connect(String),
    Auth(String),
    Channel(String),
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshError::Connect(s) => write!(f, "connection failed: {s}"),
            SshError::Auth(s) => write!(f, "auth failed: {s}"),
            SshError::Channel(s) => write!(f, "channel error: {s}"),
        }
    }
}

impl std::error::Error for SshError {}

impl russh::client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key.fingerprint(ssh_key::HashAlg::Sha256);
        tracing::info!("host key fingerprint: {fingerprint}");

        if self.auto_accept_hosts {
            return Ok(true);
        }

        use super::known_hosts::{self, HostKeyStatus};

        match known_hosts::check_host_key(&self.host, self.port, server_public_key) {
            HostKeyStatus::Known => {
                tracing::debug!("host key matches known_hosts");
                Ok(true)
            }
            HostKeyStatus::Changed => {
                tracing::error!(
                    host = %self.host,
                    port = self.port,
                    fingerprint = %fingerprint,
                    "HOST KEY CHANGED — possible MITM attack, rejecting connection"
                );
                Ok(false)
            }
            HostKeyStatus::Unknown => {
                tracing::info!(
                    host = %self.host,
                    port = self.port,
                    fingerprint = %fingerprint,
                    "unknown host, accepting key (TOFU)"
                );
                if let Err(e) = known_hosts::add_host_key(&self.host, self.port, server_public_key)
                {
                    tracing::warn!("failed to save host key to known_hosts: {e}");
                }
                Ok(true)
            }
        }
    }
}

/// Connect to an SSH server and authenticate.
/// FR-RECONNECT-01, FR-CONFIG-06: keepalive_interval_secs configures SSH keepalive.
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    identity_file: Option<&str>,
    keepalive_interval_secs: u32,
) -> Result<russh::client::Handle<SshHandler>, SshError> {
    let mut config = russh::client::Config::default();
    if keepalive_interval_secs > 0 {
        config.keepalive_interval = Some(std::time::Duration::from_secs(
            keepalive_interval_secs as u64,
        ));
    }

    let handler = SshHandler {
        auto_accept_hosts: false,
        host: host.to_string(),
        port,
    };

    let addr = format!("{host}:{port}");
    tracing::info!("russh: connecting to {addr}");

    let mut handle = russh::client::connect(Arc::new(config), &addr, handler)
        .await
        .map_err(|e| SshError::Connect(e.to_string()))?;

    // Try authentication methods /* FR-CONN-07 */
    authenticate(&mut handle, username, identity_file).await?;

    tracing::info!("russh: authenticated as {username}");
    Ok(handle)
}

async fn authenticate(
    handle: &mut russh::client::Handle<SshHandler>,
    username: &str,
    identity_file: Option<&str>,
) -> Result<(), SshError> {
    // 1. Try ssh-agent first /* FR-CONN-07 */
    if std::env::var("SSH_AUTH_SOCK").is_ok() && try_agent_auth(handle, username).await? {
        return Ok(());
    }

    // 2. Try explicit identity file
    if let Some(key_path) = identity_file
        && try_key_auth(handle, username, key_path).await?
    {
        return Ok(());
    }

    // 3. Try default key paths (Ed25519, RSA, ECDSA)
    if let Some(home) = dirs::home_dir() {
        let ssh_dir = home.join(".ssh");
        for key_name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
            let key_path = ssh_dir.join(key_name);
            if key_path.exists()
                && try_key_auth(handle, username, key_path.to_str().unwrap_or("")).await?
            {
                return Ok(());
            }
        }
    }

    Err(SshError::Auth("no authentication method succeeded".into()))
}

/// Try authentication via ssh-agent. /* FR-CONN-07 */
async fn try_agent_auth(
    handle: &mut russh::client::Handle<SshHandler>,
    username: &str,
) -> Result<bool, SshError> {
    let mut agent = match AgentClient::connect_env().await {
        Ok(a) => {
            tracing::debug!("ssh-agent: connected");
            a
        }
        Err(e) => {
            tracing::debug!("ssh-agent: connect failed: {e}");
            return Ok(false);
        }
    };

    let identities = match agent.request_identities().await {
        Ok(ids) => ids,
        Err(e) => {
            tracing::debug!("ssh-agent: failed to list identities: {e}");
            return Ok(false);
        }
    };

    tracing::debug!("ssh-agent: {} identities available", identities.len());

    for identity in &identities {
        let pubkey = identity.public_key();

        // Skip DSA keys from agent too /* FR-CONN-08 */
        if pubkey.algorithm() == Algorithm::Dsa {
            tracing::warn!("skipping DSA key from agent: DSA is deprecated and insecure");
            continue;
        }

        let comment = identity.comment();
        tracing::debug!("ssh-agent: trying key {comment}");

        match handle
            .authenticate_publickey_with(username, pubkey.into_owned(), None, &mut agent)
            .await
        {
            Ok(result) if result.success() => {
                tracing::info!("ssh-agent: authenticated with key {comment}");
                return Ok(true);
            }
            Ok(_) => {
                tracing::debug!("ssh-agent: key {comment} rejected");
            }
            Err(e) => {
                tracing::debug!("ssh-agent: auth error with key {comment}: {e}");
            }
        }
    }

    Ok(false)
}

async fn try_key_auth(
    handle: &mut russh::client::Handle<SshHandler>,
    username: &str,
    key_path: &str,
) -> Result<bool, SshError> {
    let path = std::path::Path::new(key_path);
    if !path.exists() {
        return Ok(false);
    }

    tracing::debug!("russh: trying key {key_path}");
    let key = match russh::keys::load_secret_key(path, None) {
        Ok(k) => k,
        Err(e) => {
            tracing::debug!("russh: key load failed: {e}");
            return Ok(false);
        }
    };

    // Reject DSA keys — deprecated and insecure /* FR-CONN-08 */
    if key.algorithm() == Algorithm::Dsa {
        tracing::warn!("skipping DSA key {key_path}: DSA is deprecated and insecure");
        return Ok(false);
    }

    let key_with_alg = PrivateKeyWithHashAlg::new(Arc::new(key), None);
    match handle.authenticate_publickey(username, key_with_alg).await {
        Ok(result) => Ok(result.success()),
        Err(_) => Ok(false),
    }
}

/// Open a PTY channel with shell.
pub async fn open_shell(
    handle: &russh::client::Handle<SshHandler>,
    cols: u32,
    rows: u32,
) -> Result<russh::Channel<russh::client::Msg>, SshError> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| SshError::Channel(e.to_string()))?;

    channel
        .request_pty(false, "xterm-256color", cols, rows, 0, 0, &[])
        .await
        .map_err(|e| SshError::Channel(format!("PTY request failed: {e}")))?;

    channel
        .request_shell(false)
        .await
        .map_err(|e| SshError::Channel(format!("shell request failed: {e}")))?;

    Ok(channel)
}

/// Open an exec channel for running a single command.
pub async fn exec_command(
    handle: &russh::client::Handle<SshHandler>,
    command: &str,
) -> Result<String, SshError> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| SshError::Channel(e.to_string()))?;

    channel
        .exec(true, command)
        .await
        .map_err(|e| SshError::Channel(format!("exec failed: {e}")))?;

    let mut output = Vec::new();
    let mut ch = channel;
    loop {
        match ch.wait().await {
            Some(russh::ChannelMsg::Data { data }) => {
                output.extend_from_slice(&data);
            }
            Some(russh::ChannelMsg::Eof) | None => break,
            _ => {}
        }
    }

    Ok(String::from_utf8_lossy(&output).to_string())
}
