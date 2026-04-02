// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ShellKeep;
use crate::app::Message;
use crate::app::view::styles;

use iced::widget::{Space, button, container, row, text};
use iced::{Color, Element, Length};

use crate::app::AppWindow;

impl ShellKeep {
    pub(crate) fn view_status_bar<'a>(&'a self, win: &'a AppWindow) -> Element<'a, Message> {
        let active_count = win.tabs.iter().filter(|t| !t.is_dead()).count();
        let dead_count = win.tabs.iter().filter(|t| t.is_dead()).count();
        let total = win.tabs.len();

        let zoom_info = if (self.current_font_size - self.config.terminal.font_size).abs() > 0.1 {
            format!("  {}pt", self.current_font_size)
        } else {
            String::new()
        };

        let hidden_count = self.hidden_sessions.len();
        let hidden_suffix = if hidden_count > 0 {
            format!(" + {hidden_count} hidden")
        } else {
            String::new()
        };
        let status_text = if dead_count > 0 {
            format!(
                "{total} tabs ({active_count} active, {dead_count} disconnected){hidden_suffix}{zoom_info}"
            )
        } else {
            format!(
                "{total} tab{}{hidden_suffix}{zoom_info}",
                if total == 1 { "" } else { "s" }
            )
        };

        let active_label = if let Some(tab) = win.tabs.get(win.active_tab) {
            tab.label.clone()
        } else {
            String::new()
        };

        // FR-ENV-01: environment indicator (hidden when only "default" env)
        // Phase 6: prefer workspace_env from window, fall back to current_environment
        let env_label = win
            .workspace_env
            .as_deref()
            .unwrap_or(&self.current_environment);
        let show_env = !env_label.eq_ignore_ascii_case("default");

        let shortcuts_btn = button(text("\u{2328}").size(12))
            .on_press(Message::ShowShortcutsDialog)
            .padding([0, 6])
            .style(styles::ghost_button_style);

        let mut bar_items: Vec<Element<'_, Message>> = vec![
            text(active_label)
                .size(11)
                .color(Color::from_rgb8(0xa6, 0xad, 0xc8))
                .into(),
        ];

        if show_env {
            bar_items.push(Space::new().width(16).into());
            bar_items.push(
                text(format!("workspace: {env_label}"))
                    .size(11)
                    .color(Color::from_rgb8(0x89, 0xb4, 0xfa))
                    .into(),
            );
        }

        bar_items.push(Space::new().width(Length::Fill).into());
        bar_items.push(
            text(status_text)
                .size(11)
                .color(Color::from_rgb8(0x6c, 0x70, 0x86))
                .into(),
        );
        bar_items.push(Space::new().width(8).into());
        bar_items.push(shortcuts_btn.into());

        container(
            row(bar_items)
                .padding([2, 8])
                .width(Length::Fill)
                .align_y(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .style(styles::bar_background_style)
        .into()
    }
}
