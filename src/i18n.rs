// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Internationalization support.
//!
//! NFR-I18N-01 through NFR-I18N-10.
//! v1 languages: English (source) and Brazilian Portuguese (pt_BR).
//!
//! All user-visible strings go through this module for future localization.

/// Get a translated string. Currently returns the English original.
/// This function will be replaced with proper gettext/fluent lookup
/// once the translation infrastructure is set up.
pub fn t(key: &str) -> &str {
    key
}

// User-visible strings
pub const WELCOME_TITLE: &str = "shellkeep";
pub const WELCOME_SUBTITLE: &str = "SSH sessions that survive everything";
pub const CONNECT: &str = "Connect";
pub const HOST_PLACEHOLDER: &str = "user@host or just hostname";
pub const PORT_LABEL: &str = "Port";
pub const USERNAME_LABEL: &str = "Username";
pub const IDENTITY_LABEL: &str = "Identity file";
pub const IDENTITY_PLACEHOLDER: &str = "~/.ssh/id_ed25519 (optional)";
pub const RECENT_CONNECTIONS: &str = "Recent connections";
pub const SESSION_DISCONNECTED: &str = "Session disconnected";
pub const RECONNECT: &str = "Reconnect";
pub const CLOSE_TAB: &str = "Close tab";
pub const RECONNECTING: &str = "Reconnecting...";
pub const CONNECTION_LOST: &str = "Connection lost";
pub const COPY: &str = "Copy";
pub const PASTE: &str = "Paste";
pub const MOVE_LEFT: &str = "Move left";
pub const MOVE_RIGHT: &str = "Move right";
pub const RENAME: &str = "Rename";
pub const SESSION_KEPT: &str = "Session kept on server — you can restore it later";
pub const NO_CRASH_DUMPS: &str = "No crash dumps found.";
pub const TAB_NAME_PLACEHOLDER: &str = "tab name";

// pt_BR translations (for future use)
#[cfg(test)]
mod pt_br {
    pub const WELCOME_SUBTITLE: &str = "Sessões SSH que sobrevivem a tudo";
    pub const CONNECT: &str = "Conectar";
    pub const HOST_PLACEHOLDER: &str = "usuario@host ou apenas hostname";
    pub const PORT_LABEL: &str = "Porta";
    pub const USERNAME_LABEL: &str = "Usuário";
    pub const IDENTITY_LABEL: &str = "Arquivo de identidade";
    pub const RECENT_CONNECTIONS: &str = "Conexões recentes";
    pub const SESSION_DISCONNECTED: &str = "Sessão desconectada";
    pub const RECONNECT: &str = "Reconectar";
    pub const CLOSE_TAB: &str = "Fechar aba";
    pub const RECONNECTING: &str = "Reconectando...";
    pub const CONNECTION_LOST: &str = "Conexão perdida";
    pub const COPY: &str = "Copiar";
    pub const PASTE: &str = "Colar";
    pub const SESSION_KEPT: &str = "Sessão mantida no servidor — você pode restaurá-la depois";
}
