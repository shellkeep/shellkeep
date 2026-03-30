// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed error hierarchy for shellkeep.
//!
//! These types are defined now but callers will be migrated in subsequent tasks
//! (R-22 through R-24).

/// SSH-related errors.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("connection failed: {0}")]
    Connect(#[source] russh::Error),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("channel error: {0}")]
    Channel(#[source] russh::Error),
    #[error("{0}")]
    Proxy(#[from] crate::ssh::proxy::ProxyError),
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
