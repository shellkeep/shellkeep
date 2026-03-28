// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Catppuccin Mocha color palette for the terminal and UI.

#![allow(dead_code)]

use iced::Color;
use iced_term::ColorPalette;

// Catppuccin Mocha UI colors
pub const BASE: Color = Color::from_rgb(
    0x1e as f32 / 255.0,
    0x1e as f32 / 255.0,
    0x2e as f32 / 255.0,
);
pub const MANTLE: Color = Color::from_rgb(
    0x18 as f32 / 255.0,
    0x18 as f32 / 255.0,
    0x25 as f32 / 255.0,
);
pub const SURFACE0: Color = Color::from_rgb(
    0x31 as f32 / 255.0,
    0x32 as f32 / 255.0,
    0x44 as f32 / 255.0,
);
pub const TEXT: Color = Color::from_rgb(
    0xcd as f32 / 255.0,
    0xd6 as f32 / 255.0,
    0xf4 as f32 / 255.0,
);
pub const SUBTEXT0: Color = Color::from_rgb(
    0xa6 as f32 / 255.0,
    0xad as f32 / 255.0,
    0xc8 as f32 / 255.0,
);
pub const OVERLAY0: Color = Color::from_rgb(
    0x6c as f32 / 255.0,
    0x70 as f32 / 255.0,
    0x86 as f32 / 255.0,
);
pub const BLUE: Color = Color::from_rgb(
    0x89 as f32 / 255.0,
    0xb4 as f32 / 255.0,
    0xfa as f32 / 255.0,
);
pub const GREEN: Color = Color::from_rgb(
    0xa6 as f32 / 255.0,
    0xe3 as f32 / 255.0,
    0xa1 as f32 / 255.0,
);
pub const RED: Color = Color::from_rgb(
    0xf3 as f32 / 255.0,
    0x8b as f32 / 255.0,
    0xa8 as f32 / 255.0,
);
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
