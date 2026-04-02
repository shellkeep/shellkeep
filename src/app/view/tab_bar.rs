// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::AppWindow;
use crate::app::Message;
use crate::app::view::styles;
use crate::{RENAME_INPUT_ID, ShellKeep};

use iced::widget::{Space, button, container, mouse_area, row, scrollable, text, text_input};
use iced::{Color, Element, Length, Theme};

impl ShellKeep {
    pub(crate) fn view_tab_bar<'a>(&'a self, win: &'a AppWindow) -> Element<'a, Message> {
        let mut tabs_row: Vec<Element<'_, Message>> = Vec::new();
        // P7: track accumulated x position for context menu positioning
        let mut tab_x_offset: f32 = 0.0;

        for (i, tab) in win.tabs.iter().enumerate() {
            let is_active = i == win.active_tab && !win.show_welcome;
            let is_renaming = win.renaming_tab == Some(i);

            let bg = if is_active {
                Color::from_rgb8(0x31, 0x32, 0x44)
            } else {
                Color::from_rgb8(0x1e, 0x1e, 0x2e)
            };

            let tab_btn: Element<'_, Message> = if is_renaming {
                container(
                    text_input("tab name", &self.rename_input)
                        .id(RENAME_INPUT_ID)
                        .on_input(Message::RenameInputChanged)
                        .on_submit(Message::FinishRename)
                        .size(12)
                        .padding([4, 8])
                        .width(150),
                )
                .padding([2, 4])
                .style(move |_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(bg)),
                    ..Default::default()
                })
                .into()
            } else {
                let label_text: String = if tab.label.len() > 25 {
                    format!("{}...", &tab.label[..22])
                } else {
                    tab.label.clone()
                };

                // FR-UI-04: connection status indicator
                // red = dead/disconnected, blue = reconnecting, orange = high latency (>300ms),
                // green = connected and healthy
                let (indicator, label_color) = if tab.is_dead() {
                    ("●", Color::from_rgb8(0xf3, 0x8b, 0xa8))
                } else if tab.is_auto_reconnect() || (tab.is_russh() && !tab.has_channel()) {
                    ("●", Color::from_rgb8(0x89, 0xb4, 0xfa))
                } else if tab.last_latency_ms.is_some_and(|ms| ms > 300) {
                    // FR-UI-04: orange for high latency (>300ms)
                    ("●", Color::from_rgb8(0xfa, 0xb3, 0x87))
                } else {
                    ("●", Color::from_rgb8(0xa6, 0xe3, 0xa1))
                };

                let close_btn = button(text("×").size(12))
                    .on_press(Message::CloseTab(i))
                    .padding([0, 4])
                    .style(styles::ghost_button_style);

                // FR-UI-05: build tab content with optional latency display
                let mut tab_items: Vec<Element<'_, Message>> = vec![
                    text(indicator).size(8).color(label_color).into(),
                    text(label_text)
                        .size(12)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4))
                        .into(),
                ];
                // Show latency value when > 300ms
                if let Some(ms) = tab.last_latency_ms
                    && ms > 300
                {
                    tab_items.push(
                        text(format!("{ms}ms"))
                            .size(9)
                            .color(Color::from_rgb8(0xfa, 0xb3, 0x87))
                            .into(),
                    );
                }
                tab_items.push(close_btn.into());
                let tab_content = row(tab_items).spacing(6).align_y(iced::Alignment::Center);

                let tab_button = button(tab_content)
                    .on_press(Message::SelectTab(i))
                    .padding([6, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(iced::Background::Color(bg)),
                        text_color: label_color,
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    });

                // P7: estimate tab width from label length
                // indicator(~14px) + label(~7px/char) + latency?(~30px) + close(~26px) + padding(24px) + spacing(~24px)
                let display_len = tab.label.len().min(25);
                let latency_extra: f32 = if tab.last_latency_ms.is_some_and(|ms| ms > 300) {
                    36.0
                } else {
                    0.0
                };
                let est_width =
                    14.0 + (display_len as f32 * 7.0) + latency_extra + 26.0 + 24.0 + 18.0;
                let ctx_x = tab_x_offset;
                tab_x_offset += est_width + 1.0; // +1 for row spacing

                mouse_area(tab_button)
                    .on_right_press(Message::TabContextMenu(i, ctx_x, 30.0))
                    .into()
            };

            tabs_row.push(tab_btn);
        }

        let new_tab_btn = button(text("+").size(14))
            .on_press(Message::NewTab)
            .padding([6, 10])
            .style(styles::ghost_button_style);

        // P8: always show restore button, with badge count when hidden sessions exist
        let hidden_count = self.hidden_sessions.len();
        let restore_label = if hidden_count > 0 {
            format!("\u{25BC} {hidden_count}")
        } else {
            "\u{25BC}".to_string()
        };
        let restore_color = if hidden_count > 0 {
            Color::from_rgb8(0xcd, 0xd6, 0xf4)
        } else {
            Color::from_rgb8(0x6c, 0x70, 0x86)
        };
        let restore_btn: Element<'_, Message> =
            button(text(restore_label).size(11).color(restore_color))
                .on_press(Message::ShowRestoreDropdown)
                .padding([6, 8])
                .style(styles::ghost_button_style)
                .into();

        // P10: wrap tabs in horizontal scrollable for overflow handling
        let scrollable_tabs = scrollable(row(tabs_row).spacing(1))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(0).scroller_width(0),
            ))
            .width(Length::Fill);

        let bar = row![scrollable_tabs, new_tab_btn, restore_btn]
            .width(Length::Fill)
            .align_y(iced::Alignment::Center);

        container(bar)
            .width(Length::Fill)
            .style(styles::bar_background_style)
            .into()
    }

    /// Build the list of hidden session menu items from saved state.
    pub(crate) fn build_hidden_session_items(&self) -> Vec<Element<'_, Message>> {
        let saved_state = self.cached_shared_state.as_ref().cloned();
        let saved_env_tabs = saved_state
            .as_ref()
            .map(|s| s.env_tabs(&self.current_environment))
            .unwrap_or_default();

        let mut items: Vec<Element<'_, Message>> = Vec::new();

        // Item 5: rename window option at the top of the dropdown
        items.push(
            button(text("Rename window...").size(13))
                .on_press(Message::RenameWindow)
                .padding([8, 16])
                .width(220)
                .style(styles::context_menu_style)
                .into(),
        );
        // Separator
        items.push(
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(styles::separator_style)
                .into(),
        );

        for uuid in &self.hidden_sessions {
            let title = saved_env_tabs
                .iter()
                .find(|t| &t.session_uuid == uuid)
                .map(|t| t.title.clone())
                .unwrap_or_else(|| format!("Session {}", &uuid[..8.min(uuid.len())]));

            let uuid_owned = uuid.clone();
            items.push(
                button(text(title).size(13))
                    .on_press(Message::RestoreHiddenSession(uuid_owned))
                    .padding([8, 16])
                    .width(220)
                    .style(styles::context_menu_style)
                    .into(),
            );
        }
        items
    }
}
