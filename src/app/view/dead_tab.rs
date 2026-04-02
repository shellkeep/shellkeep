// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ShellKeep;
use crate::app::Message;
use crate::app::tab::Tab;
use crate::app::view::styles;

use iced::widget::{Space, button, center, column, container, scrollable, text};
use iced::{Color, Element, Length};
use shellkeep::i18n;
use shellkeep::state::history;

impl ShellKeep {
    pub(crate) fn view_dead_tab<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        let index = self
            .active_window()
            .and_then(|w| w.tabs.iter().position(|t| t.id == tab.id))
            .unwrap_or(0);

        // FR-UI-07..08: enhanced dead session banner
        let banner_text = if tab.reconnect_attempts() > 0 {
            i18n::t(i18n::DEAD_SESSION_RECONNECTABLE)
        } else {
            i18n::t(i18n::DEAD_SESSION_TERMINATED)
        };

        let mut items: Vec<Element<'a, Message>> = vec![
            text("⚠").size(48).into(),
            text(i18n::t(i18n::SESSION_DISCONNECTED))
                .size(20)
                .color(Color::from_rgb8(0xf9, 0xe2, 0xaf))
                .into(),
            text(banner_text)
                .size(13)
                .color(Color::from_rgb8(0xa6, 0xad, 0xc8))
                .into(),
            text(&tab.label)
                .size(14)
                .color(Color::from_rgb8(0xa6, 0xad, 0xc8))
                .into(),
        ];

        // FR-UI-08: show reconnect attempt count and last error
        let attempts = tab.reconnect_attempts();
        if attempts > 0 {
            items.push(
                text(format!(
                    "Connection lost after {} reconnection attempt{}",
                    attempts,
                    if attempts == 1 { "" } else { "s" }
                ))
                .size(12)
                .color(Color::from_rgb8(0xf3, 0x8b, 0xa8))
                .into(),
            );
        }
        if let Some(ref err) = tab.last_error {
            items.push(
                text(format!("Last error: {err}"))
                    .size(11)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86))
                    .into(),
            );
        }

        items.push(Space::new().height(16).into());

        // FR-RECONNECT-04: reconnect button — label varies based on context
        let reconnect_label = if tab.reconnect_attempts() > 0 {
            i18n::t(i18n::TRY_AGAIN)
        } else {
            i18n::t(i18n::RECONNECT)
        };
        items.push(
            button(
                text(reconnect_label)
                    .size(14)
                    .color(Color::from_rgb8(0x1e, 0x1e, 0x2e)),
            )
            .on_press(Message::ReconnectTab(index))
            .padding([10, 24])
            .style(styles::reconnect_button_style)
            .into(),
        );

        // Close tab button
        items.push(
            button(text(i18n::t(i18n::CLOSE_TAB)).size(12))
                .on_press(Message::CloseTab(index))
                .padding([6, 16])
                .style(styles::ghost_button_style)
                .into(),
        );

        // Hide (keep on server) button
        items.push(
            button(text("Hide (keep on server)").size(12))
                .on_press(Message::HideTab(index))
                .padding([6, 16])
                .style(styles::ghost_button_style)
                .into(),
        );

        // FR-UI-09..10: show preserved session history if available
        let history_output = history::reconstruct_output(&tab.session_uuid);
        let banner = column(items).spacing(12).align_x(iced::Alignment::Center);

        match history_output {
            Some(output) if !output.is_empty() => {
                let history_view = container(
                    scrollable(
                        container(
                            text(output)
                                .size(13)
                                .font(iced::Font::MONOSPACE)
                                .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                        )
                        .padding(12),
                    )
                    .height(Length::Fill),
                )
                .style(styles::history_container_style)
                .width(Length::Fill)
                .height(Length::Fill);

                column![
                    container(banner).padding(iced::Padding {
                        top: 24.0,
                        right: 0.0,
                        bottom: 8.0,
                        left: 0.0
                    }),
                    history_view,
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            Some(_) => column![
                container(banner).padding(iced::Padding {
                    top: 24.0,
                    right: 0.0,
                    bottom: 8.0,
                    left: 0.0
                }),
                center(
                    text("History file is empty.")
                        .size(12)
                        .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                ),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            // FR-UI-09: no history file exists
            None => column![
                container(banner).padding(iced::Padding {
                    top: 24.0,
                    right: 0.0,
                    bottom: 8.0,
                    left: 0.0
                }),
                center(
                    text("History unavailable")
                        .size(12)
                        .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                ),
            ]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        }
    }
}
