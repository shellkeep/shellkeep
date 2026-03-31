// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Color palettes for the terminal and UI.
//!
//! Built-in themes (catppuccin-mocha) plus FR-TERMINAL-11 JSON theme loading
//! from `~/.config/shellkeep/themes/<name>.json`.

use std::path::PathBuf;

use iced::Color;
use iced_term::ColorPalette;
use serde::Deserialize;

// Catppuccin Mocha UI color palette — use these instead of inline hex in view code.
// Not all are referenced yet; retained as the canonical palette for consistency.
#[allow(dead_code)]
pub const BASE: Color = Color::from_rgb(
    0x1e as f32 / 255.0,
    0x1e as f32 / 255.0,
    0x2e as f32 / 255.0,
);
#[allow(dead_code)]
pub const MANTLE: Color = Color::from_rgb(
    0x18 as f32 / 255.0,
    0x18 as f32 / 255.0,
    0x25 as f32 / 255.0,
);
#[allow(dead_code)]
pub const SURFACE0: Color = Color::from_rgb(
    0x31 as f32 / 255.0,
    0x32 as f32 / 255.0,
    0x44 as f32 / 255.0,
);
#[allow(dead_code)]
pub const TEXT: Color = Color::from_rgb(
    0xcd as f32 / 255.0,
    0xd6 as f32 / 255.0,
    0xf4 as f32 / 255.0,
);
#[allow(dead_code)]
pub const SUBTEXT0: Color = Color::from_rgb(
    0xa6 as f32 / 255.0,
    0xad as f32 / 255.0,
    0xc8 as f32 / 255.0,
);
#[allow(dead_code)]
pub const OVERLAY0: Color = Color::from_rgb(
    0x6c as f32 / 255.0,
    0x70 as f32 / 255.0,
    0x86 as f32 / 255.0,
);
#[allow(dead_code)]
pub const BLUE: Color = Color::from_rgb(
    0x89 as f32 / 255.0,
    0xb4 as f32 / 255.0,
    0xfa as f32 / 255.0,
);
#[allow(dead_code)]
pub const GREEN: Color = Color::from_rgb(
    0xa6 as f32 / 255.0,
    0xe3 as f32 / 255.0,
    0xa1 as f32 / 255.0,
);
#[allow(dead_code)]
pub const RED: Color = Color::from_rgb(
    0xf3 as f32 / 255.0,
    0x8b as f32 / 255.0,
    0xa8 as f32 / 255.0,
);
#[allow(dead_code)]
pub const YELLOW: Color = Color::from_rgb(
    0xf9 as f32 / 255.0,
    0xe2 as f32 / 255.0,
    0xaf as f32 / 255.0,
);

pub fn catppuccin_mocha() -> ColorPalette {
    ColorPalette {
        foreground: "#cdd6f4".into(),
        background: "#1e1e2e".into(),
        black: "#45475a".into(),
        red: "#f38ba8".into(),
        green: "#a6e3a1".into(),
        yellow: "#f9e2af".into(),
        blue: "#89b4fa".into(),
        magenta: "#f5c2e7".into(),
        cyan: "#94e2d5".into(),
        white: "#bac2de".into(),
        bright_black: "#585b70".into(),
        bright_red: "#f38ba8".into(),
        bright_green: "#a6e3a1".into(),
        bright_yellow: "#f9e2af".into(),
        bright_blue: "#89b4fa".into(),
        bright_magenta: "#f5c2e7".into(),
        bright_cyan: "#94e2d5".into(),
        bright_white: "#a6adc8".into(),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// FR-TERMINAL-11: JSON theme loading
// ---------------------------------------------------------------------------

/// JSON representation of a terminal color theme.
#[derive(Debug, Clone, Deserialize)]
struct JsonTheme {
    foreground: String,
    background: String,
    black: String,
    red: String,
    green: String,
    yellow: String,
    blue: String,
    magenta: String,
    cyan: String,
    white: String,
    #[serde(default)]
    bright_black: Option<String>,
    #[serde(default)]
    bright_red: Option<String>,
    #[serde(default)]
    bright_green: Option<String>,
    #[serde(default)]
    bright_yellow: Option<String>,
    #[serde(default)]
    bright_blue: Option<String>,
    #[serde(default)]
    bright_magenta: Option<String>,
    #[serde(default)]
    bright_cyan: Option<String>,
    #[serde(default)]
    bright_white: Option<String>,
}

fn themes_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("shellkeep")
        .join("themes")
}

/// FR-TERMINAL-11: Load a theme from `~/.config/shellkeep/themes/<name>.json`.
/// Returns `None` if the file doesn't exist or fails to parse.
pub fn load_theme(name: &str) -> Option<ColorPalette> {
    let path = themes_dir().join(format!("{name}.json"));
    let data = std::fs::read_to_string(&path).ok()?;
    let jt: JsonTheme = serde_json::from_str(&data).ok()?;
    Some(ColorPalette {
        foreground: jt.foreground.clone(),
        background: jt.background.clone(),
        black: jt.black.clone(),
        red: jt.red.clone(),
        green: jt.green.clone(),
        yellow: jt.yellow.clone(),
        blue: jt.blue.clone(),
        magenta: jt.magenta.clone(),
        cyan: jt.cyan.clone(),
        white: jt.white.clone(),
        bright_black: jt.bright_black.unwrap_or_else(|| jt.black.clone()),
        bright_red: jt.bright_red.unwrap_or_else(|| jt.red.clone()),
        bright_green: jt.bright_green.unwrap_or_else(|| jt.green.clone()),
        bright_yellow: jt.bright_yellow.unwrap_or_else(|| jt.yellow.clone()),
        bright_blue: jt.bright_blue.unwrap_or_else(|| jt.blue.clone()),
        bright_magenta: jt.bright_magenta.unwrap_or_else(|| jt.magenta.clone()),
        bright_cyan: jt.bright_cyan.unwrap_or_else(|| jt.cyan.clone()),
        bright_white: jt.bright_white.unwrap_or_else(|| jt.white.clone()),
        ..Default::default()
    })
}

/// Resolve a theme by name: try user themes dir first, then fall back to built-in.
pub fn resolve_theme(name: &str) -> ColorPalette {
    if let Some(palette) = load_theme(name) {
        tracing::info!("loaded custom theme '{name}' from themes directory");
        return palette;
    }
    match name {
        "catppuccin-mocha" | "dark" => catppuccin_mocha(),
        _ => {
            tracing::warn!("unknown theme '{name}', falling back to catppuccin-mocha");
            catppuccin_mocha()
        }
    }
}
