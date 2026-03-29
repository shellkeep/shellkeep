// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SSH connection management using russh.

use std::sync::Arc;

use russh::keys::PrivateKeyWithHashAlg;
use russh::keys::ssh_key;

/// SSH connection handler for russh client events.
pub struct SshHandler {
    pub auto_accept_hosts: bool,
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
        if self.auto_accept_hosts {
            return Ok(true);
        }

        // Check against ~/.ssh/known_hosts
        let known_hosts_path = dirs::home_dir()
            .map(|h| h.join(".ssh").join("known_hosts"))
            .unwrap_or_default();

        if known_hosts_path.exists() {
            // For now, accept if known_hosts exists (system ssh already verified)
            // TODO: parse known_hosts and match against server_public_key
            tracing::debug!(
                "host key fingerprint: {}",
                server_public_key.fingerprint(ssh_key::HashAlg::Sha256)
            );
            return Ok(true);
        }

        // No known_hosts — accept (TOFU behavior)
        tracing::warn!("no known_hosts file, accepting host key (TOFU)");
        Ok(true)
    }
}

/// Connect to an SSH server and authenticate.
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    identity_file: Option<&str>,
) -> Result<russh::client::Handle<SshHandler>, SshError> {
    let config = russh::client::Config::default();

    let handler = SshHandler {
        auto_accept_hosts: true,
    };

    let addr = format!("{host}:{port}");
    tracing::info!("russh: connecting to {addr}");

    let mut handle = russh::client::connect(Arc::new(config), &addr, handler)
        .await
        .map_err(|e| SshError::Connect(e.to_string()))?;

    // Try authentication methods
    authenticate(&mut handle, username, identity_file).await?;

    tracing::info!("russh: authenticated as {username}");
    Ok(handle)
}

async fn authenticate(
    handle: &mut russh::client::Handle<SshHandler>,
    username: &str,
    identity_file: Option<&str>,
) -> Result<(), SshError> {
    // Try explicit identity file first
    if let Some(key_path) = identity_file
        && try_key_auth(handle, username, key_path).await?
    {
        return Ok(());
    }

    // Try default key paths
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
