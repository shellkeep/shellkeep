// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ShellKeep;
use crate::app::Message;
use crate::app::view::styles;

use iced::widget::{Space, button, center, column, container, row, scrollable, text, text_input};
use iced::{Color, Element, Length};
use shellkeep::i18n;

impl ShellKeep {
    pub(crate) fn view_welcome(&self) -> Element<'_, Message> {
        let logo = text("🐚").size(64);
        let title = text("shellkeep")
            .size(28)
            .color(Color::from_rgb8(0x89, 0xb4, 0xfa));

        // FR-UI-03: first-use experience — show extended welcome on first run
        let is_first_use =
            self.recent.connections.is_empty() && !shellkeep::config::config_file_exists();

        let subtitle: Element<'_, Message> = if is_first_use {
            // P3: simplified first-use — device name auto-generated from hostname
            column![
                text(i18n::t(i18n::WELCOME_TEXT))
                    .size(16)
                    .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                text(i18n::t(i18n::WELCOME_DESCRIPTION))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text(i18n::t(i18n::WELCOME_PROMPT))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
            ]
            .spacing(6)
            .align_x(iced::Alignment::Center)
            .into()
        } else {
            let version = format!(
                "v{} — SSH sessions that survive everything",
                env!("CARGO_PKG_VERSION")
            );
            text(version)
                .size(14)
                .color(Color::from_rgb8(0xa6, 0xad, 0xc8))
                .into()
        };

        let host_field = text_input(i18n::t(i18n::HOST_PLACEHOLDER), &self.welcome.host_input)
            .on_input(Message::HostInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let user_field = text_input("username", &self.welcome.user_input)
            .on_input(Message::UserInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let port_field = text_input("22", &self.welcome.port_input)
            .on_input(Message::PortInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10)
            .width(80);

        let identity_field = text_input(
            i18n::t(i18n::IDENTITY_PLACEHOLDER),
            &self.welcome.identity_input,
        )
        .on_input(Message::IdentityInputChanged)
        .on_submit(Message::Connect)
        .size(14)
        .padding(10);

        // P4: disable connect button while a connection is in progress
        let is_connecting = self
            .all_tabs()
            .any(|t| t.is_russh() && !t.has_channel() && !t.is_dead());
        let connect_btn = if is_connecting {
            button(
                text("Connecting...")
                    .size(14)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
            )
            .padding([10, 24])
            .style(styles::secondary_button_style)
        } else {
            button(
                text(i18n::t(i18n::CONNECT))
                    .size(14)
                    .color(Color::from_rgb8(0x1e, 0x1e, 0x2e)),
            )
            .on_press(Message::Connect)
            .padding([10, 24])
            .style(styles::primary_button_style)
        };

        // FR-UI-01: simple host input is always visible
        let host_row = column![text(i18n::t(i18n::HOST_LABEL)).size(12), host_field].spacing(4);

        // FR-UI-01: advanced toggle button
        let advanced_label = if self.welcome.show_advanced {
            "Hide advanced options"
        } else {
            "Advanced options (port, user, key)"
        };
        let advanced_toggle = button(
            text(advanced_label)
                .size(11)
                .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
        )
        .on_press(Message::ToggleAdvanced)
        .padding([4, 8])
        .style(styles::ghost_button_style);

        // FR-UI-01: advanced fields, hidden by default
        let advanced_section: Element<'_, Message> = if self.welcome.show_advanced {
            // Compact layout: username + port on one row, identity on second
            let user_port_row = row![
                column![text(i18n::t(i18n::USERNAME_LABEL)).size(12), user_field]
                    .spacing(4)
                    .width(Length::Fill),
                column![text(i18n::t(i18n::PORT_LABEL)).size(12), port_field]
                    .spacing(4)
                    .width(80),
            ]
            .spacing(8);
            let identity_row = column![
                text(format!("{} (optional)", i18n::t(i18n::IDENTITY_LABEL)))
                    .size(12)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                identity_field
            ]
            .spacing(4);
            column![user_port_row, identity_row].spacing(8).into()
        } else {
            Space::new().height(0).into()
        };

        let error_text: Element<'_, Message> = if let Some(ref err) = self.error {
            text(err)
                .size(12)
                .color(Color::from_rgb8(0xf3, 0x8b, 0xa8))
                .into()
        } else {
            Space::new().height(0).into()
        };

        // Recent connections list
        let recent_section: Element<'_, Message> = if self.recent.connections.is_empty() {
            Space::new().height(0).into()
        } else {
            let mut recent_items: Vec<Element<'_, Message>> = Vec::new();
            recent_items.push(
                text(i18n::t(i18n::RECENT_CONNECTIONS))
                    .size(12)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86))
                    .into(),
            );
            for (i, conn) in self.recent.connections.iter().enumerate() {
                let display_label = if let Some(ts) = conn.last_connected {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let ago = now.saturating_sub(ts);
                    let time_str = i18n::format_relative_time(ago);
                    format!("{}  ({})", conn.label, time_str)
                } else {
                    conn.label.clone()
                };

                let item: Element<'_, Message> = button(
                    text(display_label)
                        .size(13)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                )
                .on_press(Message::ConnectRecent(i))
                .padding([6, 12])
                .width(Length::Fill)
                .style(styles::recent_item_style)
                .into();
                recent_items.push(item);
            }
            scrollable(column(recent_items).spacing(4))
                .height(Length::Shrink)
                .into()
        };

        let form = column![
            logo,
            title,
            subtitle,
            Space::new().height(20),
            host_row,
            advanced_toggle,
            advanced_section,
            Space::new().height(8),
            connect_btn,
            error_text,
            Space::new().height(12),
            recent_section,
        ]
        .spacing(12)
        .align_x(iced::Alignment::Center)
        .max_width(420);

        center(form).into()
    }

    /// Phase 5: render the control window — welcome/connect form + connected servers.
    pub(crate) fn view_control_window(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = Vec::new();

        // Item 7: small logo and slogan at the top
        let logo_row = row![
            text("\u{1F41A}").size(24),
            column![
                text("shellkeep")
                    .size(16)
                    .color(Color::from_rgb8(0x89, 0xb4, 0xfa)),
                text("SSH sessions that survive everything")
                    .size(10)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
            ]
            .spacing(2),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        items.push(logo_row.into());
        items.push(Space::new().height(8).into());

        // Show connected servers section if we have an active connection
        if self.current_conn.is_some() {
            let label_color = Color::from_rgb8(0xa6, 0xad, 0xc8);
            let text_color = Color::from_rgb8(0xcd, 0xd6, 0xf4);

            items.push(text("Connected servers").size(16).color(text_color).into());

            // Gather info about the current connection
            if let Some(ref conn) = self.current_conn {
                let server_label =
                    format!("{}@{}:{}", conn.key.username, conn.key.host, conn.key.port);
                let session_count = self
                    .windows
                    .values()
                    .filter(|w| w.kind == crate::app::WindowKind::Session)
                    .flat_map(|w| w.tabs.iter())
                    .filter(|t| !t.is_dead() && t.terminal.is_some())
                    .count();
                let total_count = self
                    .windows
                    .values()
                    .filter(|w| w.kind == crate::app::WindowKind::Session)
                    .flat_map(|w| w.tabs.iter())
                    .count();
                let status_text = if session_count > 0 {
                    format!(
                        "{session_count} active session{}",
                        if session_count == 1 { "" } else { "s" }
                    )
                } else if total_count > 0 {
                    "disconnected".to_string()
                } else {
                    "connected, no sessions".to_string()
                };

                // Item 6: state sync status
                let sync_status = if self.state_syncer.is_some() {
                    "State synced \u{2713}"
                } else if self.current_conn.is_some() {
                    "Local only \u{26A0}"
                } else {
                    ""
                };

                let server_card = container(
                    column![
                        row![
                            text("\u{25CF}").size(10).color(if session_count > 0 {
                                Color::from_rgb8(0xa6, 0xe3, 0xa1)
                            } else {
                                Color::from_rgb8(0xf9, 0xe2, 0xaf)
                            }),
                            column![
                                text(server_label).size(14).color(text_color),
                                text(status_text).size(11).color(label_color),
                                text(sync_status)
                                    .size(10)
                                    .color(if self.state_syncer.is_some() {
                                        Color::from_rgb8(0xa6, 0xe3, 0xa1)
                                    } else {
                                        Color::from_rgb8(0xf9, 0xe2, 0xaf)
                                    }),
                            ]
                            .spacing(2),
                        ]
                        .spacing(8)
                        .align_y(iced::Alignment::Center),
                        Space::new().height(8),
                        // Item 2: disconnect and close buttons
                        row![
                            button(
                                text("Disconnect")
                                    .size(12)
                                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4))
                            )
                            .on_press(Message::DisconnectServer)
                            .padding([6, 12])
                            .style(styles::secondary_button_style),
                            button(
                                text("Close all")
                                    .size(12)
                                    .color(Color::from_rgb8(0xf3, 0x8b, 0xa8))
                            )
                            .on_press(Message::CloseServer)
                            .padding([6, 12])
                            .style(styles::danger_button_style),
                        ]
                        .spacing(8),
                    ]
                    .spacing(4)
                    .padding(12),
                )
                .width(Length::Fill)
                .style(styles::server_card_style);

                items.push(server_card.into());
            }

            items.push(Space::new().height(16).into());

            // Item 7: collapsible connect form when already connected
            let toggle_label = if self.show_connect_form {
                "Hide connect form"
            } else {
                "Connect to another server..."
            };
            items.push(
                button(
                    text(toggle_label)
                        .size(12)
                        .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                )
                .on_press(Message::ToggleConnectForm)
                .padding([4, 8])
                .style(styles::ghost_button_style)
                .into(),
            );

            if self.show_connect_form {
                items.push(self.view_welcome());
            }
        } else {
            // No connection — show full welcome form
            items.push(self.view_welcome());
        }

        // Item 2: close-server confirmation overlay
        let control_content: Element<'_, Message> = scrollable(
            container(column(items).spacing(8).padding(16).max_width(420)).width(Length::Fill),
        )
        .into();

        if self.confirm_close_server {
            let dialog = container(
                column![
                    text("Close all sessions?")
                        .size(18)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    text("This will terminate ALL tmux sessions on the server.\nThis cannot be undone.")
                        .size(13)
                        .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    Space::new().height(12),
                    row![
                        button(text("Cancel").size(14))
                            .on_press(Message::CancelCloseServer)
                            .padding([10, 24])
                            .style(styles::secondary_button_style),
                        Space::new().width(Length::Fill),
                        button(
                            text("Terminate all")
                                .size(14)
                                .color(Color::from_rgb8(0x1e, 0x1e, 0x2e))
                        )
                        .on_press(Message::ConfirmCloseServer)
                        .padding([10, 24])
                        .style(styles::danger_button_style),
                    ]
                    .width(Length::Fill),
                ]
                .spacing(8)
                .padding(24)
                .width(380),
            )
            .style(styles::dialog_container_style);
            use iced::widget::{center, stack};
            stack![control_content, center(dialog)].into()
        } else {
            control_content
        }
    }
}
