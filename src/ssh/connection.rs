// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SSH connection management using russh.

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

use russh::client::KeyboardInteractiveAuthResponse;
#[cfg(unix)]
use russh::keys::agent::client::AgentClient;
use russh::keys::{PrivateKeyWithHashAlg, ssh_key};
use ssh_key::{Algorithm, HashAlg};

use super::known_hosts::HostKeyStatus;
use super::{proxy, ssh_config};

pub use crate::error::SshError;

/// Result of host key verification during connection.
/// Stored in shared state so the UI can show a dialog after connection.
#[derive(Debug, Clone)]
pub struct HostKeyPrompt {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub status: HostKeyStatus,
    /// Old fingerprint (only set for Changed status)
    pub old_fingerprint: Option<String>,
}

/// SSH connection handler for russh client events.
pub struct SshHandler {
    pub auto_accept_hosts: bool,
    pub host: String,
    pub port: u16,
    /// FR-CONN-05: StrictHostKeyChecking mode from ssh_config
    pub strict_host_key_checking: String,
    /// Shared slot for deferred host key prompt (phase-2 UI dialog)
    pub pending_host_key: Arc<std::sync::Mutex<Option<HostKeyPrompt>>>,
}

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

        use super::known_hosts;

        let mode = self.strict_host_key_checking.as_str();

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
                    "HOST KEY CHANGED — possible MITM attack"
                );

                // FR-CONN-02: changed keys always rejected, but store prompt for UI
                if let Ok(mut slot) = self.pending_host_key.lock() {
                    // Try to find old fingerprint from known_hosts
                    let old_fp = known_hosts::get_stored_fingerprint(&self.host, self.port);
                    *slot = Some(HostKeyPrompt {
                        host: self.host.clone(),
                        port: self.port,
                        fingerprint: fingerprint.to_string(),
                        status: HostKeyStatus::Changed,
                        old_fingerprint: old_fp,
                    });
                }
                Ok(false)
            }
            HostKeyStatus::Unknown => {
                match mode {
                    // FR-CONN-05: yes = reject unknown hosts
                    "yes" => {
                        tracing::warn!("StrictHostKeyChecking=yes: rejecting unknown host");
                        Ok(false)
                    }
                    // FR-CONN-05: no = auto-accept everything
                    "no" => {
                        tracing::info!("StrictHostKeyChecking=no: auto-accepting unknown host");
                        if let Err(e) =
                            known_hosts::add_host_key(&self.host, self.port, server_public_key)
                        {
                            tracing::warn!("failed to save host key: {e}");
                        }
                        Ok(true)
                    }
                    // FR-CONN-05: accept-new = auto-accept unknown, reject changed
                    "accept-new" => {
                        tracing::info!("StrictHostKeyChecking=accept-new: accepting new host");
                        if let Err(e) =
                            known_hosts::add_host_key(&self.host, self.port, server_public_key)
                        {
                            tracing::warn!("failed to save host key: {e}");
                        }
                        Ok(true)
                    }
                    // FR-CONN-05: ask (default) = accept temporarily, store prompt for UI
                    _ => {
                        tracing::info!(
                            host = %self.host,
                            fingerprint = %fingerprint,
                            "unknown host, deferring to UI (TOFU)"
                        );
                        // Accept temporarily — UI will show dialog after connection
                        if let Ok(mut slot) = self.pending_host_key.lock() {
                            *slot = Some(HostKeyPrompt {
                                host: self.host.clone(),
                                port: self.port,
                                fingerprint: fingerprint.to_string(),
                                status: HostKeyStatus::Unknown,
                                old_fingerprint: None,
                            });
                        }
                        // Save to known_hosts provisionally (UI can undo on reject)
                        if let Err(e) =
                            known_hosts::add_host_key(&self.host, self.port, server_public_key)
                        {
                            tracing::warn!("failed to save host key: {e}");
                        }
                        Ok(true)
                    }
                }
            }
        }
    }
}

/// Build restricted algorithm preferences. /* NFR-SEC-12..16 */
fn restricted_preferred() -> russh::Preferred {
    russh::Preferred {
        // NFR-SEC-12: Allowed ciphers — AEAD and CTR modes only, no CBC
        cipher: Cow::Borrowed(&[
            russh::cipher::CHACHA20_POLY1305,
            russh::cipher::AES_256_GCM,
            russh::cipher::AES_128_GCM,
            russh::cipher::AES_256_CTR,
            russh::cipher::AES_192_CTR,
            russh::cipher::AES_128_CTR,
        ]),
        // NFR-SEC-13: Allowed MACs — SHA-2 ETM preferred, no plain SHA-1
        mac: Cow::Borrowed(&[
            russh::mac::HMAC_SHA512_ETM,
            russh::mac::HMAC_SHA256_ETM,
            russh::mac::HMAC_SHA512,
            russh::mac::HMAC_SHA256,
        ]),
        // NFR-SEC-14: Allowed KEX — curve25519 and DH group14+ only
        kex: Cow::Borrowed(&[
            russh::kex::CURVE25519,
            russh::kex::CURVE25519_PRE_RFC_8731,
            russh::kex::DH_GEX_SHA256,
            russh::kex::DH_G16_SHA512,
            russh::kex::DH_G14_SHA256,
            russh::kex::EXTENSION_SUPPORT_AS_CLIENT,
            russh::kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT,
        ]),
        // NFR-SEC-15: Weak algorithms (3des-cbc, hmac-sha1, dh-group1, DSA)
        // are implicitly rejected by not appearing in the above lists
        ..russh::Preferred::DEFAULT
    }
}

/// Result of a successful SSH connection, including deferred host key prompt.
#[must_use]
pub struct ConnectResult {
    pub handle: russh::client::Handle<SshHandler>,
    /// If set, the UI should show a host key dialog before using this connection.
    pub host_key_prompt: Option<HostKeyPrompt>,
}

/// Connect to an SSH server and authenticate.
/// FR-RECONNECT-01, FR-CONFIG-06: keepalive_interval_secs configures SSH keepalive.
/// FR-COMPAT-01, FR-CONFIG-07: applies ~/.ssh/config overrides.
pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    identity_file: Option<&str>,
    password: Option<&str>,
    keepalive_interval_secs: u32,
) -> Result<ConnectResult, SshError> {
    // FR-COMPAT-01: load ~/.ssh/config overrides
    let host_cfg = ssh_config::load_host_config(host);

    let effective_host = host_cfg.hostname.as_deref().unwrap_or(host);
    let effective_port = host_cfg.port.unwrap_or(port);
    let effective_user = host_cfg.user.as_deref().unwrap_or(username);
    let effective_identity = identity_file
        .map(ssh_config::expand_tilde)
        .or(host_cfg.identity_file);

    // Log effective connection parameters (helpful for troubleshooting ssh_config issues)
    if effective_host != host || effective_port != port || effective_user != username {
        tracing::info!(
            "ssh_config overrides: host={effective_host} port={effective_port} user={effective_user}"
        );
    }
    if let Some(ref id) = effective_identity {
        tracing::info!("using identity file: {id}");
    }
    let effective_keepalive = if keepalive_interval_secs > 0 {
        keepalive_interval_secs
    } else {
        host_cfg.server_alive_interval.unwrap_or(0)
    };

    // FR-CONN-05: StrictHostKeyChecking from ssh_config
    let strict_mode = host_cfg
        .strict_host_key_checking
        .as_deref()
        .unwrap_or("ask")
        .to_string();

    let mut config = russh::client::Config {
        preferred: restricted_preferred(), /* NFR-SEC-12..16 */
        ..Default::default()
    };

    if effective_keepalive > 0 {
        config.keepalive_interval =
            Some(std::time::Duration::from_secs(effective_keepalive as u64));
        if let Some(max) = host_cfg.server_alive_count_max {
            config.keepalive_max = max as usize;
        }
    }

    let pending_host_key = Arc::new(std::sync::Mutex::new(None));
    let handler = SshHandler {
        auto_accept_hosts: false,
        host: effective_host.to_string(),
        port: effective_port,
        strict_host_key_checking: strict_mode,
        pending_host_key: pending_host_key.clone(),
    };

    let addr = format!("{effective_host}:{effective_port}");

    // FR-PROXY-01, FR-PROXY-02: check for ProxyJump/ProxyCommand
    let proxy_cmd = proxy::resolve_proxy_command(
        host_cfg.proxy_jump.as_deref(),
        host_cfg.proxy_command.as_deref(),
    );

    let mut handle = if let Some(ref cmd) = proxy_cmd {
        tracing::info!("russh: connecting to {addr} via proxy");
        let stream =
            proxy::ProxyStream::spawn(cmd, effective_host, effective_port, Some(effective_user))
                .await
                .map_err(|e| SshError::connect(e.to_string()))?;
        russh::client::connect_stream(Arc::new(config), stream, handler)
            .await
            .map_err(|e| SshError::connect_with(format!("proxy connection to {addr} failed"), e))?
    } else {
        tracing::info!("russh: connecting to {addr}");
        russh::client::connect(Arc::new(config), &addr, handler)
            .await
            .map_err(|e| SshError::connect_with(addr.to_string(), e))?
    };

    // Try authentication methods /* FR-CONN-07 */
    authenticate(
        &mut handle,
        effective_user,
        effective_identity.as_deref(),
        password,
    )
    .await?;

    tracing::info!("russh: authenticated as {effective_user}");

    // Check if there's a deferred host key prompt
    let prompt = pending_host_key
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());

    Ok(ConnectResult {
        handle,
        host_key_prompt: prompt,
    })
}

async fn authenticate(
    handle: &mut russh::client::Handle<SshHandler>,
    username: &str,
    identity_file: Option<&str>,
    password: Option<&str>,
) -> Result<(), SshError> {
    // 1. Try ssh-agent first /* FR-CONN-07 */
    #[cfg(unix)]
    if std::env::var("SSH_AUTH_SOCK").is_ok() && try_agent_auth(handle, username).await? {
        return Ok(());
    }

    // 2. Try explicit identity file (with certificate if present) /* FR-CONN-11 */
    if let Some(key_path) = identity_file
        && try_key_auth(handle, username, key_path).await?
    {
        return Ok(());
    }

    // 3. Try keys from ~/.ssh/ — default names first, then scan for others.
    // This matches ssh behavior: try well-known names, then discover any other
    // private keys the user may have (e.g. id_rsa_2023, work_key, etc.)
    if let Some(home) = dirs::home_dir() {
        let ssh_dir = home.join(".ssh");

        // 3a. Try well-known default key names first (same order as OpenSSH)
        for key_name in &["id_ed25519", "id_ecdsa", "id_rsa"] {
            let key_path = ssh_dir.join(key_name);
            if key_path.exists()
                && try_key_auth(handle, username, key_path.to_str().unwrap_or("")).await?
            {
                return Ok(());
            }
        }

        // 3b. Scan ~/.ssh/ for any other private key files
        if let Ok(entries) = std::fs::read_dir(&ssh_dir) {
            let skip = [
                "known_hosts",
                "known_hosts.old",
                "config",
                "authorized_keys",
                "environment",
                "rc",
            ];
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                // Skip: public keys, known files, directories, hidden files, already-tried defaults
                if name_str.ends_with(".pub")
                    || name_str.starts_with('.')
                    || skip.contains(&name_str.as_ref())
                    || name_str == "id_ed25519"
                    || name_str == "id_ecdsa"
                    || name_str == "id_rsa"
                {
                    continue;
                }
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                // Quick check: private keys start with "-----BEGIN" or "openssh-key-v1"
                if let Ok(header) = std::fs::read(&path) {
                    let dominated =
                        header.starts_with(b"-----BEGIN") || header.starts_with(b"openssh-key-v1");
                    if !dominated {
                        continue;
                    }
                }
                if try_key_auth(handle, username, path.to_str().unwrap_or("")).await? {
                    return Ok(());
                }
            }
        }
    }

    // 4. Try password authentication /* FR-CONN-09 */
    // Password is never stored to disk (NFR-SEC-08), only accepted as a parameter
    if let Some(pw) = password
        && try_password_auth(handle, username, pw).await?
    {
        return Ok(());
    }

    // 5. Try keyboard-interactive /* FR-CONN-10 */
    if try_keyboard_interactive(handle, username).await? {
        return Ok(());
    }

    Err(SshError::Auth("no authentication method succeeded".into()))
}

/// Try authentication via ssh-agent. /* FR-CONN-07 */
#[cfg(unix)]
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
    let path = Path::new(key_path);
    if !path.exists() {
        return Ok(false);
    }

    // NFR-SEC-09: Private key memory protection
    // In Rust, explicit mlock() and explicit_bzero() are not available for safe code.
    // Private keys are managed by russh-keys via Arc<PrivateKey> and are dropped when
    // the last reference goes out of scope. The Rust ownership model ensures keys are
    // not accidentally copied. However, key material may be swapped to disk by the OS.
    // Core dumps are disabled (NFR-SEC-10) to mitigate this risk.
    // For enhanced security, users should use ssh-agent (FR-CONN-07) which handles
    // key material in a separate process with its own memory protections.
    tracing::info!("russh: trying key {key_path}");
    let key = match russh::keys::load_secret_key(path, None) {
        Ok(k) => k,
        Err(e) => {
            tracing::info!("russh: key {key_path} load failed: {e}");
            return Ok(false);
        }
    };

    // Reject DSA keys — deprecated and insecure /* FR-CONN-08 */
    if key.algorithm() == Algorithm::Dsa {
        tracing::warn!("skipping DSA key {key_path}: DSA is deprecated and insecure");
        return Ok(false);
    }

    // FR-CONN-11: try certificate auth if <key_path>-cert.pub exists
    let cert_path = format!("{key_path}-cert.pub");
    if Path::new(&cert_path).exists() {
        tracing::debug!("russh: found SSH certificate at {cert_path}");
        match russh::keys::load_openssh_certificate(&cert_path) {
            Ok(cert) => {
                match handle
                    .authenticate_openssh_cert(username, Arc::new(key.clone()), cert)
                    .await
                {
                    Ok(result) if result.success() => {
                        tracing::info!("russh: authenticated with certificate {cert_path}");
                        return Ok(true);
                    }
                    Ok(_) => {
                        tracing::debug!(
                            "russh: certificate {cert_path} rejected, falling back to key"
                        );
                    }
                    Err(e) => {
                        tracing::debug!("russh: certificate auth error: {e}, falling back to key");
                    }
                }
            }
            Err(e) => {
                tracing::debug!("russh: failed to load certificate {cert_path}: {e}");
            }
        }
    }

    let key_arc = Arc::new(key);

    // For RSA keys, modern servers require rsa-sha2-256 or rsa-sha2-512 (not legacy ssh-rsa/SHA-1).
    // Try SHA-512 first (strongest), then SHA-256, then legacy SHA-1 as last resort.
    let hash_algs: Vec<Option<HashAlg>> = if key_arc.algorithm().is_rsa() {
        vec![
            Some(HashAlg::Sha512),
            Some(HashAlg::Sha256),
            None, // legacy ssh-rsa (SHA-1) as fallback for old servers
        ]
    } else {
        vec![None] // non-RSA keys don't use hash_alg
    };

    for hash_alg in hash_algs {
        let alg_name: &str = match hash_alg {
            Some(HashAlg::Sha512) => "rsa-sha2-512",
            Some(HashAlg::Sha256) => "rsa-sha2-256",
            _ => "default",
        };
        let key_with_alg = PrivateKeyWithHashAlg::new(key_arc.clone(), hash_alg);
        match handle.authenticate_publickey(username, key_with_alg).await {
            Ok(result) if result.success() => {
                tracing::info!("russh: authenticated with key {key_path} ({alg_name})");
                return Ok(true);
            }
            Ok(_) => {
                tracing::debug!("russh: key {key_path} ({alg_name}) rejected by server");
            }
            Err(e) => {
                tracing::debug!("russh: key {key_path} ({alg_name}) auth error: {e}");
            }
        }
    }
    tracing::info!("russh: key {key_path} rejected by server (all algorithms tried)");
    Ok(false)
}

/// Try password-based authentication. /* FR-CONN-09 */
/// The password is never stored to disk (NFR-SEC-08).
async fn try_password_auth(
    handle: &mut russh::client::Handle<SshHandler>,
    username: &str,
    password: &str,
) -> Result<bool, SshError> {
    tracing::debug!("russh: trying password auth");
    match handle.authenticate_password(username, password).await {
        Ok(result) if result.success() => {
            tracing::info!("russh: authenticated with password");
            Ok(true)
        }
        Ok(_) => {
            tracing::debug!("russh: password auth rejected");
            Ok(false)
        }
        Err(e) => {
            tracing::debug!("russh: password auth error: {e}");
            Ok(false)
        }
    }
}

/// Try keyboard-interactive authentication (MFA). /* FR-CONN-10 */
/// Currently initiates the flow and responds with empty strings.
/// UI prompt integration will come later.
async fn try_keyboard_interactive(
    handle: &mut russh::client::Handle<SshHandler>,
    username: &str,
) -> Result<bool, SshError> {
    tracing::debug!("russh: trying keyboard-interactive auth");
    let response = match handle
        .authenticate_keyboard_interactive_start(username, None::<String>)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("russh: keyboard-interactive start error: {e}");
            return Ok(false);
        }
    };

    match response {
        KeyboardInteractiveAuthResponse::Success => {
            tracing::info!("russh: authenticated via keyboard-interactive (no prompts)");
            return Ok(true);
        }
        KeyboardInteractiveAuthResponse::InfoRequest {
            name,
            instructions,
            prompts,
        } => {
            tracing::info!(
                name = %name,
                instructions = %instructions,
                num_prompts = prompts.len(),
                "keyboard-interactive: server sent prompts (UI integration pending)"
            );
            for prompt in &prompts {
                tracing::debug!(
                    prompt = %prompt.prompt,
                    echo = prompt.echo,
                    "keyboard-interactive prompt"
                );
            }
            // Respond with empty strings for now — UI will provide real responses later
            let responses = vec![String::new(); prompts.len()];
            match handle
                .authenticate_keyboard_interactive_respond(responses)
                .await
            {
                Ok(KeyboardInteractiveAuthResponse::Success) => {
                    tracing::info!("russh: authenticated via keyboard-interactive");
                    return Ok(true);
                }
                Ok(_) => {
                    tracing::debug!("russh: keyboard-interactive auth failed after response");
                }
                Err(e) => {
                    tracing::debug!("russh: keyboard-interactive respond error: {e}");
                }
            }
        }
        KeyboardInteractiveAuthResponse::Failure { .. } => {
            tracing::debug!("russh: keyboard-interactive not supported by server");
        }
    }

    Ok(false)
}

/// Open a PTY channel with shell.
pub async fn open_shell(
    handle: &russh::client::Handle<SshHandler>,
    cols: u32,
    rows: u32,
) -> Result<russh::Channel<russh::client::Msg>, SshError> {
    tracing::debug!("opening shell channel ({cols}x{rows})");
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

    tracing::debug!("shell channel opened ({cols}x{rows})");
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
