// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared style functions for the view layer, extracted from inline closures.

use iced::widget::{button, container};
use iced::{Color, Theme};

/// Standard dialog container: dark background, rounded corners, shadow.
/// Used by all modal dialogs (env, host-key, password, lock, close-confirm).
pub(crate) fn dialog_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x24, 0x24, 0x36))),
        border: iced::Border {
            radius: 12.0.into(),
            width: 1.0,
            color: Color::from_rgb8(0x45, 0x47, 0x5a),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.6),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..Default::default()
    }
}

/// Primary action button: blue background, dark text.
/// Used for "Connect", "Create", "Rename", "Accept and save", "Close" (quit dialog).
pub(crate) fn primary_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x89, 0xb4, 0xfa))),
        text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Secondary button: subtle dark background, light text.
/// Used for "Cancel", "Connect once", navigation buttons, recent connections.
pub(crate) fn secondary_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
        text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Danger button: red/pink background, dark text.
/// Used for "Terminate", "Delete", "Cancel" (host-key reject), "Disconnect".
pub(crate) fn danger_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0xf3, 0x8b, 0xa8))),
        text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Warning button: orange background, dark text.
/// Used for "Take over" in lock conflict dialog.
pub(crate) fn warn_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0xfa, 0xb3, 0x87))),
        text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Context menu button: hover-aware, used in tab and right-click context menus.
pub(crate) fn context_menu_style(_theme: &Theme, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered | button::Status::Pressed => Color::from_rgb8(0x45, 0x47, 0x5a),
        _ => Color::from_rgb8(0x24, 0x24, 0x36),
    };
    button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
        ..Default::default()
    }
}

/// Semi-transparent dark scrim behind modal dialogs.
pub(crate) fn scrim_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgba8(0, 0, 0, 0.5))),
        ..Default::default()
    }
}

/// Context menu popup container: dark background, rounded corners, shadow.
pub(crate) fn context_menu_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x24, 0x24, 0x36))),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgb8(0x45, 0x47, 0x5a),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.5),
            offset: iced::Vector::new(2.0, 2.0),
            blur_radius: 8.0,
        },
        ..Default::default()
    }
}

/// Ghost button: no background, muted text. Used for close/dismiss/toggle controls.
pub(crate) fn ghost_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
        ..Default::default()
    }
}

/// Dark bar background used for tab bar and status bar.
pub(crate) fn bar_background_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x18, 0x18, 0x25))),
        ..Default::default()
    }
}

/// Search bar background style.
pub(crate) fn search_bar_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x24, 0x24, 0x36))),
        border: iced::Border {
            radius: 0.0.into(),
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        ..Default::default()
    }
}

/// Search bar button style (prev/next/close).
pub(crate) fn search_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
        text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Toast notification container style.
pub(crate) fn toast_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgb8(0x45, 0x47, 0x5a),
        },
        ..Default::default()
    }
}

/// Context menu separator line.
pub(crate) fn separator_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x45, 0x47, 0x5a))),
        ..Default::default()
    }
}

/// Reconnect button: green background, dark text.
pub(crate) fn reconnect_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0xa6, 0xe3, 0xa1))),
        text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
        border: iced::Border {
            radius: 6.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// "Create new session" button on dead tab: secondary with visible border.
pub(crate) fn new_session_button_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
        text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
        border: iced::Border {
            radius: 6.0.into(),
            width: 1.0,
            color: Color::from_rgb8(0x45, 0x47, 0x5a),
        },
        ..Default::default()
    }
}

/// History view container on dead tab.
pub(crate) fn history_container_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x18, 0x18, 0x25))),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgb8(0x31, 0x32, 0x44),
        },
        ..Default::default()
    }
}

/// Recent connection list item button: similar to secondary but with smaller radius.
pub(crate) fn recent_item_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
        text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Server card container in the control window.
pub(crate) fn server_card_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: Color::from_rgb8(0x45, 0x47, 0x59),
        },
        ..Default::default()
    }
}
