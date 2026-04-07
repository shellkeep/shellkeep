// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: MIT

//! Terminal emulator widget for iced, backed by alacritty_terminal.

pub mod actions;
pub mod bindings;
pub mod settings;

mod backend;
mod font;
mod terminal;
mod theme;
mod view;

pub use alacritty_terminal::event::Event as AlacrittyEvent;
pub use alacritty_terminal::selection::SelectionType;
pub use alacritty_terminal::term::TermMode;
pub use alacritty_terminal::vte::ansi::CursorShape;
pub use backend::Command as BackendCommand;
pub use backend::{LinkAction, MouseButton};
pub use terminal::{Command, Event, Terminal};
pub use theme::{ColorPalette, Theme};
pub use view::TerminalView;
