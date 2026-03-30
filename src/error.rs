// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed error hierarchy for shellkeep.
//!
//! SshError is used throughout src/ssh/ (connection, sftp, lock, tmux, manager).
//! StateError will be adopted by src/state/ in a subsequent task.

/// SSH-related errors.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("channel error: {0}")]
    Channel(String),
    #[error("SFTP error: {0}")]
    Sftp(String),
    #[error("{0}")]
    Proxy(#[from] crate::ssh::proxy::ProxyError),
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        SshError::Connect(e.to_string())
    }
}

/// State persistence errors.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{0}")]
    Validation(String),
}
