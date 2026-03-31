// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed error hierarchy for shellkeep.
//!
//! SshError is used throughout src/ssh/ (connection, sftp, lock, tmux, manager).
//! StateError is used throughout src/state/ (client_id, state_file, environment).

/// SSH-related errors with structured context.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("connection failed: {context}")]
    Connect {
        context: String,
        #[source]
        source: Option<russh::Error>,
    },
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("channel error: {0}")]
    Channel(String),
    #[error("SFTP error: {0}")]
    Sftp(String),
    #[error("{0}")]
    Proxy(#[from] crate::ssh::proxy::ProxyError),
}

impl SshError {
    /// Create a Connect error with a context string and no source error.
    pub fn connect(context: impl Into<String>) -> Self {
        SshError::Connect {
            context: context.into(),
            source: None,
        }
    }

    /// Create a Connect error wrapping a russh::Error.
    pub fn connect_with(context: impl Into<String>, source: russh::Error) -> Self {
        SshError::Connect {
            context: context.into(),
            source: Some(source),
        }
    }
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        SshError::Connect {
            context: e.to_string(),
            source: Some(e),
        }
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
