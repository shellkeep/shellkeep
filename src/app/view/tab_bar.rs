// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::Message;
use crate::{RENAME_INPUT_ID, ShellKeep};

use iced::widget::{button, container, mouse_area, row, text, text_input};
use iced::{Color, Element, Length, Theme};

impl ShellKeep {
    pub(crate) fn view_tab_bar(&self) -> Element<'_, Message> {
        let mut tabs_row: Vec<Element<'_, Message>> = Vec::new();

        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab && !self.show_welcome;
            let is_renaming = self.renaming_tab == Some(i);

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
                // red = dead/disconnected, yellow = reconnecting or high latency (>300ms),
                // green = connected and healthy
                let (indicator, label_color) = if tab.dead {
                    ("●", Color::from_rgb8(0xf3, 0x8b, 0xa8))
                } else if (tab.terminal.is_none() && tab.auto_reconnect)
                    || (tab.uses_russh && tab.ssh_channel_holder.is_none())
                {
                    ("●", Color::from_rgb8(0xf9, 0xe2, 0xaf))
                } else if tab.last_latency_ms.is_some_and(|ms| ms > 300) {
                    // FR-UI-04: yellow for high latency (>300ms)
                    ("●", Color::from_rgb8(0xf9, 0xe2, 0xaf))
                } else {
                    ("●", Color::from_rgb8(0xa6, 0xe3, 0xa1))
                };

                let close_btn = button(text("×").size(12))
                    .on_press(Message::CloseTab(i))
                    .padding([0, 4])
                    .style(|_theme: &Theme, _status| button::Style {
                        background: None,
                        text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                        ..Default::default()
                    });

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
                            .color(Color::from_rgb8(0xf9, 0xe2, 0xaf))
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

                mouse_area(tab_button)
                    .on_right_press(Message::TabContextMenu(i, 0.0, 30.0))
                    .into()
            };

            tabs_row.push(tab_btn);
        }

        let new_tab_btn = button(text("+").size(14))
            .on_press(Message::NewTab)
            .padding([6, 10])
            .style(|_theme: &Theme, _status| button::Style {
                background: None,
                text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                ..Default::default()
            });

        let bar = row![row(tabs_row).spacing(1), new_tab_btn]
            .width(Length::Fill)
            .align_y(iced::Alignment::Center);

        container(bar)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgb8(0x18, 0x18, 0x25))),
                ..Default::default()
            })
            .into()
    }
}
