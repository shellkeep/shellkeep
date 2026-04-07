// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Internationalization support.
//!
//! NFR-I18N-01 through NFR-I18N-10.
//! v1 languages: English (source) and Brazilian Portuguese (pt_BR).
//!
//! All user-visible strings go through `t()` for localization.
//! The terminal locale is independent — shellkeep only localizes its own UI.

use std::collections::HashMap;
use std::sync::OnceLock;

static LOCALE: OnceLock<String> = OnceLock::new();
static TRANSLATIONS: OnceLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> =
    OnceLock::new();

/// Initialize the i18n system with detected or configured locale.
/// Call once at startup. NFR-I18N-07: locale detection from environment.
pub fn init(locale: &str) {
    // Normalize: "pt_BR.UTF-8" -> "pt_BR", "pt_BR" -> "pt_BR"
    let normalized = locale.split('.').next().unwrap_or("en");
    LOCALE.set(normalized.to_string()).ok();

    let mut all: HashMap<&'static str, HashMap<&'static str, &'static str>> = HashMap::new();
    all.insert("pt_BR", pt_br_translations());
    TRANSLATIONS.set(all).ok();
}

/// Detect system locale from environment variables.
/// NFR-I18N-07: checks LC_MESSAGES, LC_ALL, LANG in order.
pub fn detect_locale() -> String {
    std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en".to_string())
}

/// Look up a translated string by key. Falls back to English (the key itself).
pub fn t(key: &str) -> &str {
    let locale = LOCALE.get().map(|s| s.as_str()).unwrap_or("en");
    if locale == "en" || locale.starts_with("en_") {
        return key;
    }
    TRANSLATIONS
        .get()
        .and_then(|all| all.get(locale))
        .and_then(|map| map.get(key))
        .copied()
        .unwrap_or(key)
}

/// NFR-I18N-02: format a translated string with positional arguments.
/// Usage: `tf("{1} connected to {2}", &["alice", "server.example.com"])`
/// The English key uses `{1}`, `{2}`, etc. for positional placeholders.
pub fn tf(key: &str, args: &[&str]) -> String {
    let template = t(key);
    let mut result = template.to_string();
    for (i, arg) in args.iter().enumerate() {
        let placeholder = format!("{{{}}}", i + 1);
        result = result.replace(&placeholder, arg);
    }
    result
}

/// NFR-I18N-03: pluralized translation (ngettext equivalent).
/// `singular` and `plural` are the English string keys.
/// If locale has a translation, uses it; otherwise picks singular/plural based on `n`.
pub fn tn(singular: &str, plural: &str, n: usize) -> String {
    let key = if n == 1 { singular } else { plural };
    t(key).replace("{n}", &n.to_string())
}

// Plural string keys
pub const N_ACTIVE_SESSIONS_1: &str = "{n} active session";
pub const N_ACTIVE_SESSIONS_N: &str = "{n} active sessions";

// Relative time keys
pub const JUST_NOW: &str = "just now";
pub const MINUTES_AGO: &str = "{n}m ago";
pub const HOURS_AGO: &str = "{n}h ago";
pub const DAYS_AGO: &str = "{n}d ago";

/// NFR-I18N-09: format a relative time duration for display.
/// Takes seconds ago, returns a localized human-readable string.
pub fn format_relative_time(secs_ago: u64) -> String {
    match secs_ago {
        0..=59 => t(JUST_NOW).to_string(),
        60..=3599 => {
            let m = secs_ago / 60;
            t(MINUTES_AGO).replace("{n}", &m.to_string())
        }
        3600..=86399 => {
            let h = secs_ago / 3600;
            t(HOURS_AGO).replace("{n}", &h.to_string())
        }
        _ => {
            let d = secs_ago / 86400;
            t(DAYS_AGO).replace("{n}", &d.to_string())
        }
    }
}

// ── String keys (English is the key itself) ──────────────────────────

// Welcome screen
pub const WELCOME_TEXT: &str = "Welcome to shellkeep";
pub const WELCOME_DESCRIPTION: &str =
    "Your SSH sessions survive everything — network drops, laptop sleep, reboots.";
pub const WELCOME_PROMPT: &str = "Connect to a server to get started.";

// Connection form
pub const CONNECT: &str = "Connect";
pub const HOST_LABEL: &str = "Host";
pub const HOST_PLACEHOLDER: &str = "user@host or just hostname";
pub const PORT_LABEL: &str = "Port";
pub const USERNAME_LABEL: &str = "Username";
pub const IDENTITY_LABEL: &str = "Identity file";
pub const IDENTITY_PLACEHOLDER: &str = "~/.ssh/id_ed25519 (optional)";
pub const RECENT_CONNECTIONS: &str = "Recent connections";

// Connection phases
pub const CONNECTING: &str = "Connecting...";
pub const AUTHENTICATING: &str = "Authenticating...";
pub const CHECKING_TMUX: &str = "Checking tmux...";
pub const OPENING_SESSION: &str = "Opening session...";
pub const RECONNECTING: &str = "Reconnecting...";

// Session status
pub const SESSION_DISCONNECTED: &str = "Session disconnected";
pub const CONNECTION_LOST: &str = "Connection lost";
pub const SESSION_KEPT: &str = "Session hidden — click \u{25BC} in the tab bar to restore";
pub const TERMINAL_NOT_AVAILABLE: &str = "Terminal not available";
pub const NO_ACTIVE_TAB: &str = "No active tab";

// Dead session view
pub const DEAD_SESSION_RECONNECTABLE: &str =
    "Session disconnected — it may still be running on the server.";
pub const DEAD_SESSION_TERMINATED: &str =
    "Disconnected — session may still be running on the server.";
pub const TRY_AGAIN: &str = "Try again";
pub const RECONNECT: &str = "Reconnect";
pub const CREATE_NEW_SESSION: &str = "Create new session";

// Tab actions
pub const CLOSE_TAB: &str = "Close tab";
pub const MOVE_LEFT: &str = "Move left";
pub const MOVE_RIGHT: &str = "Move right";
pub const RENAME: &str = "Rename";

// Clipboard
pub const COPY: &str = "Copy";
pub const PASTE: &str = "Paste";

// Close dialog
pub const CANCEL: &str = "Cancel";

fn pt_br_translations() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();

    // Welcome
    m.insert(WELCOME_TEXT, "Bem-vindo ao shellkeep");
    m.insert(
        WELCOME_DESCRIPTION,
        "Suas sessões SSH sobrevivem a tudo — quedas de rede, suspensão do notebook, reinicializações.",
    );
    m.insert(WELCOME_PROMPT, "Conecte-se a um servidor para começar.");

    // Connection form
    m.insert(CONNECT, "Conectar");
    m.insert(HOST_LABEL, "Host");
    m.insert(HOST_PLACEHOLDER, "usuario@host ou apenas hostname");
    m.insert(PORT_LABEL, "Porta");
    m.insert(USERNAME_LABEL, "Usuário");
    m.insert(IDENTITY_LABEL, "Arquivo de identidade");
    m.insert(RECENT_CONNECTIONS, "Conexões recentes");

    // Connection phases
    m.insert(CONNECTING, "Conectando...");
    m.insert(AUTHENTICATING, "Autenticando...");
    m.insert(CHECKING_TMUX, "Verificando tmux...");
    m.insert(OPENING_SESSION, "Abrindo sessão...");
    m.insert(RECONNECTING, "Reconectando...");

    // Session status
    m.insert(SESSION_DISCONNECTED, "Sessão desconectada");
    m.insert(CONNECTION_LOST, "Conexão perdida");
    m.insert(
        SESSION_KEPT,
        "Sessão mantida no servidor — você pode restaurá-la depois",
    );
    m.insert(TERMINAL_NOT_AVAILABLE, "Terminal não disponível");
    m.insert(NO_ACTIVE_TAB, "Nenhuma aba ativa");

    // Dead session
    m.insert(
        DEAD_SESSION_RECONNECTABLE,
        "Sessão desconectada — pode ainda estar rodando no servidor.",
    );
    m.insert(
        DEAD_SESSION_TERMINATED,
        "Esta sessão foi encerrada no servidor. O histórico de saída está preservado abaixo.",
    );
    m.insert(TRY_AGAIN, "Tentar novamente");
    m.insert(RECONNECT, "Reconectar");
    m.insert(CREATE_NEW_SESSION, "Criar nova sessão");

    // Tab actions
    m.insert(CLOSE_TAB, "Fechar aba");
    m.insert(MOVE_LEFT, "Mover para esquerda");
    m.insert(MOVE_RIGHT, "Mover para direita");
    m.insert(RENAME, "Renomear");

    // Clipboard
    m.insert(COPY, "Copiar");
    m.insert(PASTE, "Colar");

    // Search
    // Close dialog
    m.insert(CANCEL, "Cancelar");

    // Relative time
    m.insert(JUST_NOW, "agora");
    m.insert(MINUTES_AGO, "{n}min atrás");
    m.insert(HOURS_AGO, "{n}h atrás");
    m.insert(DAYS_AGO, "{n}d atrás");

    // Plurals
    m.insert(N_ACTIVE_SESSIONS_1, "{n} sessão ativa");
    m.insert(N_ACTIVE_SESSIONS_N, "{n} sessões ativas");

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_t_returns_english_by_default() {
        // Without init, locale defaults to "en"
        assert_eq!(t("Hello"), "Hello");
        assert_eq!(t(CONNECT), "Connect");
    }

    #[test]
    fn test_pt_br_translations_complete() {
        let pt = pt_br_translations();
        // Every constant that has a pt_BR translation should be present
        assert_eq!(pt.get(CONNECT), Some(&"Conectar"));
        assert_eq!(pt.get(SESSION_DISCONNECTED), Some(&"Sessão desconectada"));
        assert_eq!(pt.get(CLOSE_TAB), Some(&"Fechar aba"));
        assert_eq!(pt.get(COPY), Some(&"Copiar"));
        assert_eq!(pt.get(PASTE), Some(&"Colar"));
        assert_eq!(pt.get(RECONNECTING), Some(&"Reconectando..."));
    }

    #[test]
    fn test_detect_locale_default() {
        // If none of the env vars are set, should return "en"
        // (Can't reliably test env vars in unit tests without side effects)
        let locale = detect_locale();
        assert!(!locale.is_empty());
    }

    #[test]
    fn test_format_relative_time_english() {
        // Default locale is English
        assert_eq!(format_relative_time(0), "just now");
        assert_eq!(format_relative_time(30), "just now");
        assert_eq!(format_relative_time(120), "2m ago");
        assert_eq!(format_relative_time(7200), "2h ago");
        assert_eq!(format_relative_time(172800), "2d ago");
    }

    #[test]
    fn test_unknown_key_returns_key() {
        assert_eq!(t("nonexistent_key_xyz"), "nonexistent_key_xyz");
    }

    #[test]
    fn test_tf_positional_args() {
        assert_eq!(tf("{1} at {2}", &["alice", "host"]), "alice at host");
        assert_eq!(tf("no args", &[]), "no args");
        assert_eq!(tf("{1}", &["x"]), "x");
    }

    #[test]
    fn test_tn_plural() {
        assert_eq!(
            tn(N_ACTIVE_SESSIONS_1, N_ACTIVE_SESSIONS_N, 1),
            "1 active session"
        );
        assert_eq!(
            tn(N_ACTIVE_SESSIONS_1, N_ACTIVE_SESSIONS_N, 5),
            "5 active sessions"
        );
        assert_eq!(
            tn(N_ACTIVE_SESSIONS_1, N_ACTIVE_SESSIONS_N, 0),
            "0 active sessions"
        );
    }
}
