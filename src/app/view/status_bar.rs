// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ShellKeep;
use crate::app::Message;
use crate::app::view::styles;

use iced::widget::{Space, container, row, text};
use iced::{Color, Element, Length};

impl ShellKeep {
    pub(crate) fn view_status_bar(&self) -> Element<'_, Message> {
        let active_count = self.tabs.iter().filter(|t| !t.is_dead()).count();
        let dead_count = self.tabs.iter().filter(|t| t.is_dead()).count();
        let total = self.tabs.len();

        let zoom_info = if (self.current_font_size - self.config.terminal.font_size).abs() > 0.1 {
            format!("  {}pt", self.current_font_size)
        } else {
            String::new()
        };

        let status_text = if dead_count > 0 {
            format!("{total} tabs ({active_count} active, {dead_count} disconnected){zoom_info}")
        } else {
            format!(
                "{total} tab{}{zoom_info}",
                if total == 1 { "" } else { "s" }
            )
        };

        let active_label = if let Some(tab) = self.tabs.get(self.active_tab) {
            tab.label.clone()
        } else {
            String::new()
        };

        // FR-ENV-01: environment indicator
        let env_indicator = text(format!("env: {}", self.current_environment))
            .size(11)
            .color(Color::from_rgb8(0x89, 0xb4, 0xfa));

        container(
            row![
                text(active_label)
                    .size(11)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                Space::new().width(16),
                env_indicator,
                Space::new().width(Length::Fill),
                text(status_text)
                    .size(11)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
            ]
            .padding([2, 8])
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(styles::bar_background_style)
        .into()
    }
}
