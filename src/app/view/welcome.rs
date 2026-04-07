// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// TODO: migrate from RecentConnections to SavedServers, then remove this allow
#![allow(deprecated)]

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
            text("SSH sessions that survive everything")
                .size(14)
                .color(Color::from_rgb8(0xa6, 0xad, 0xc8))
                .into()
        };

        let version_text: Element<'_, Message> = text(format!("v{}", env!("CARGO_PKG_VERSION")))
            .size(11)
            .color(Color::from_rgb8(0x6c, 0x70, 0x86))
            .into();

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
            Space::new().height(8),
            version_text,
        ]
        .spacing(12)
        .align_x(iced::Alignment::Center)
        .max_width(420);

        center(form).into()
    }

    /// Phase 6: render the control window — server cards with workspace sub-cards.
    pub(crate) fn view_control_window(&self) -> Element<'_, Message> {
        let text_color = Color::from_rgb8(0xcd, 0xd6, 0xf4);
        let label_color = Color::from_rgb8(0xa6, 0xad, 0xc8);

        // Phase 6: if the server form is open, show it full-screen
        if let Some(ref opt_uuid) = self.dialogs.show_server_form {
            return self.view_server_form(opt_uuid.as_deref());
        }

        // If no saved servers AND no active connection, show the original welcome form
        let has_servers = !self.saved_servers.servers.is_empty();
        let has_connection = self.current_conn.is_some();

        if !has_servers && !has_connection {
            let items: Vec<Element<'_, Message>> = vec![
                self.view_welcome(),
                Space::new().height(8).into(),
                button(
                    text("+ Add server")
                        .size(13)
                        .color(Color::from_rgb8(0x89, 0xb4, 0xfa)),
                )
                .on_press(Message::ShowServerForm(None))
                .padding([8, 16])
                .style(styles::ghost_button_style)
                .into(),
            ];
            let content: Element<'_, Message> = scrollable(
                container(column(items).spacing(8).padding(16).max_width(420)).width(Length::Fill),
            )
            .into();
            return content;
        }

        // --- Server card list ---
        let mut items: Vec<Element<'_, Message>> = Vec::new();

        // Logo header
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

        // Render each saved server as a card
        for server in &self.saved_servers.servers {
            let is_connected = self.is_server_connected(&server.uuid)
                || self.current_conn.as_ref().is_some_and(|c| {
                    // Normalize: parse server.host in case it contains user@ or :port
                    let (_, parsed_host, _) = crate::cli::parse_host_input(&server.host);
                    c.key.host == parsed_host
                        && (c.key.username == server.user
                            || server.user.is_empty()
                            || c.key.username == crate::cli::default_ssh_username())
                });
            let is_connecting = self.connecting_server.as_deref() == Some(server.uuid.as_str());

            let status_icon = if is_connecting && !is_connected {
                text("\u{25CF}")
                    .size(10)
                    .color(Color::from_rgb8(0xf9, 0xe2, 0xaf))
            } else if is_connected {
                text("\u{25CF}")
                    .size(10)
                    .color(Color::from_rgb8(0xa6, 0xe3, 0xa1))
            } else {
                text("\u{25CB}")
                    .size(10)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86))
            };

            let label = server.display_label();
            let uuid = server.uuid.clone();

            let mut card_items: Vec<Element<'_, Message>> = Vec::new();

            // Header row: status icon + label
            card_items.push(
                row![status_icon, text(label).size(14).color(text_color),]
                    .spacing(8)
                    .align_y(iced::Alignment::Center)
                    .into(),
            );

            if is_connecting && !is_connected {
                // Show connecting state — no buttons
                card_items.push(
                    text("Connecting...")
                        .size(11)
                        .color(Color::from_rgb8(0xf9, 0xe2, 0xaf))
                        .into(),
                );
            } else if is_connected {
                // Show workspace sub-cards for each workspace
                let envs = self.server_workspaces(&uuid);
                for env in &envs {
                    let visible_count = self
                        .windows
                        .values()
                        .filter(|w| {
                            w.kind == crate::app::WindowKind::Session
                                && w.workspace_env.as_deref() == Some(env.as_str())
                        })
                        .count();
                    let hidden_win_count = self
                        .hidden_windows
                        .iter()
                        .filter(|hw| hw.workspace_env.as_deref() == Some(env.as_str()))
                        .count();

                    let visible_text = format!(
                        "{visible_count} visible window{}",
                        if visible_count == 1 { "" } else { "s" }
                    );

                    let uuid_focus = uuid.clone();
                    let env_focus = env.clone();
                    let uuid_new_win = uuid.clone();
                    let env_new_win = env.clone();
                    let uuid_rename = uuid.clone();
                    let env_rename = env.clone();
                    let uuid_delete = uuid.clone();
                    let env_delete = env.clone();

                    let mut info_col = column![
                        text(env.clone()).size(12).color(text_color),
                        button(text(visible_text).size(10).color(label_color))
                            .on_press(Message::FocusWorkspaceWindows(uuid_focus, env_focus,))
                            .padding(0)
                            .style(styles::ghost_button_style),
                    ]
                    .spacing(2)
                    .width(Length::Fill);

                    if hidden_win_count > 0 {
                        let hidden_text = format!(
                            "{hidden_win_count} hidden window{}",
                            if hidden_win_count == 1 { "" } else { "s" }
                        );
                        let env_restore = env.clone();
                        info_col = info_col.push(
                            button(
                                text(hidden_text)
                                    .size(10)
                                    .color(Color::from_rgb8(0x89, 0xb4, 0xfa)),
                            )
                            .on_press(Message::RestoreWorkspaceHiddenWindows(env_restore))
                            .padding(0)
                            .style(styles::ghost_button_style),
                        );
                    }

                    let workspace_card = container(
                        row![
                            info_col,
                            button(
                                text("+ New window")
                                    .size(10)
                                    .color(Color::from_rgb8(0x89, 0xb4, 0xfa)),
                            )
                            .on_press(Message::NewWindowForWorkspace(uuid_new_win, env_new_win,))
                            .padding([3, 6])
                            .style(styles::ghost_button_style),
                            button(text("Rename").size(10).color(label_color))
                                .on_press(Message::ShowRenameWorkspace(uuid_rename, env_rename,))
                                .padding([3, 6])
                                .style(styles::ghost_button_style),
                            button(
                                text("Remove")
                                    .size(10)
                                    .color(Color::from_rgb8(0xf3, 0x8b, 0xa8)),
                            )
                            .on_press(Message::ShowDeleteWorkspace(uuid_delete, env_delete,))
                            .padding([3, 6])
                            .style(styles::ghost_button_style),
                        ]
                        .spacing(4)
                        .align_y(iced::Alignment::Center)
                        .padding([6, 8]),
                    )
                    .style(|_: &iced::Theme| container::Style {
                        background: Some(iced::Background::Color(Color::from_rgb8(
                            0x24, 0x24, 0x36,
                        ))),
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    });
                    card_items.push(workspace_card.into());
                }

                // If no workspaces listed yet, show status
                if envs.is_empty() {
                    let active_sessions = self
                        .windows
                        .values()
                        .filter(|w| w.kind == crate::app::WindowKind::Session)
                        .flat_map(|w| w.tabs.iter())
                        .filter(|t| !t.is_dead())
                        .count();
                    let status = format!(
                        "{active_sessions} active session{}",
                        if active_sessions == 1 { "" } else { "s" }
                    );
                    card_items.push(text(status).size(11).color(label_color).into());
                }

                // Server-level buttons
                let uuid_new = uuid.clone();
                let uuid_disc = uuid.clone();
                let visible_windows = self
                    .windows
                    .values()
                    .filter(|w| w.kind == crate::app::WindowKind::Session)
                    .count();
                card_items.push(Space::new().height(4).into());
                let mut btn_row_items: Vec<Element<'_, Message>> = vec![
                    button(
                        text("+ New workspace")
                            .size(11)
                            .color(Color::from_rgb8(0x89, 0xb4, 0xfa)),
                    )
                    .on_press(Message::ShowNewWorkspace(uuid_new))
                    .padding([4, 8])
                    .style(styles::ghost_button_style)
                    .into(),
                    Space::new().width(Length::Fill).into(),
                ];
                if visible_windows > 0 {
                    btn_row_items.push(
                        button(text("Terminate all sessions").size(11).color(label_color))
                            .on_press(Message::CloseServer)
                            .padding([4, 8])
                            .style(styles::ghost_button_style)
                            .into(),
                    );
                }
                btn_row_items.push(
                    button(
                        text("Disconnect")
                            .size(11)
                            .color(Color::from_rgb8(0xf3, 0x8b, 0xa8)),
                    )
                    .on_press(Message::DisconnectAllWorkspaces(uuid_disc))
                    .padding([4, 8])
                    .style(styles::ghost_button_style)
                    .into(),
                );
                card_items.push(row(btn_row_items).spacing(4).into());
            } else {
                // Disconnected server: show last connected + action buttons
                if let Some(ts) = server.last_connected {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let ago = now.saturating_sub(ts);
                    let time_str = i18n::format_relative_time(ago);
                    card_items.push(
                        text(format!("Last connected: {time_str}"))
                            .size(11)
                            .color(label_color)
                            .into(),
                    );
                }

                let uuid_conn = uuid.clone();
                let uuid_edit = uuid.clone();
                let uuid_forget = uuid.clone();
                card_items.push(Space::new().height(4).into());
                card_items.push(
                    row![
                        button(
                            text("Connect")
                                .size(12)
                                .color(Color::from_rgb8(0x1e, 0x1e, 0x2e))
                        )
                        .on_press(Message::ConnectServer(uuid_conn))
                        .padding([6, 12])
                        .style(styles::primary_button_style),
                        button(text("Edit").size(12).color(text_color))
                            .on_press(Message::EditServer(uuid_edit))
                            .padding([6, 12])
                            .style(styles::secondary_button_style),
                        button(
                            text("Forget")
                                .size(12)
                                .color(Color::from_rgb8(0xf3, 0x8b, 0xa8))
                        )
                        .on_press(Message::ForgetServer(uuid_forget))
                        .padding([6, 12])
                        .style(styles::ghost_button_style),
                    ]
                    .spacing(8)
                    .into(),
                );
            }

            let server_card = container(column(card_items).spacing(4).padding(12))
                .width(Length::Fill)
                .style(styles::server_card_style);
            items.push(server_card.into());
            items.push(Space::new().height(4).into());
        }

        // + Add server button
        items.push(Space::new().height(8).into());
        items.push(
            button(
                text("+ Add server")
                    .size(13)
                    .color(Color::from_rgb8(0x89, 0xb4, 0xfa)),
            )
            .on_press(Message::ShowServerForm(None))
            .padding([8, 16])
            .style(styles::ghost_button_style)
            .into(),
        );

        let control_content: Element<'_, Message> = scrollable(
            container(column(items).spacing(8).padding(16).max_width(420)).width(Length::Fill),
        )
        .into();

        // Overlay dialogs
        if self.confirm_close_server {
            let dialog = container(
                column![
                    text("Close all sessions?")
                        .size(18)
                        .color(text_color),
                    text("This will terminate ALL tmux sessions on the server.\nThis cannot be undone.")
                        .size(13)
                        .color(label_color),
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
        } else if self.dialogs.show_forget_server.is_some() {
            let label = self
                .dialogs
                .show_forget_server
                .as_ref()
                .and_then(|u| self.saved_servers.find_by_uuid(u))
                .map(|s| s.display_label())
                .unwrap_or_else(|| "this server".to_string());
            let dialog = self.view_forget_server_dialog(&label);
            use iced::widget::{center, stack};
            stack![control_content, center(dialog)].into()
        } else if self.dialogs.show_new_workspace.is_some() {
            let dialog = self.view_new_workspace_dialog(&self.dialogs.new_workspace_input.clone());
            use iced::widget::{center, stack};
            stack![control_content, center(dialog)].into()
        } else if self.dialogs.show_workspace_rename.is_some() {
            let env = self
                .dialogs
                .show_workspace_rename
                .as_ref()
                .map(|(_, e)| e.clone())
                .unwrap_or_default();
            let dialog = self
                .view_workspace_rename_dialog(&env, &self.dialogs.workspace_rename_input.clone());
            use iced::widget::{center, stack};
            stack![control_content, center(dialog)].into()
        } else if self.dialogs.show_workspace_delete.is_some() {
            let env = self
                .dialogs
                .show_workspace_delete
                .as_ref()
                .map(|(_, e)| e.clone())
                .unwrap_or_default();
            let dialog = self.view_workspace_delete_dialog(&env);
            use iced::widget::{center, stack};
            stack![control_content, center(dialog)].into()
        } else {
            control_content
        }
    }

    /// Phase 6: server add/edit form.
    fn view_server_form(&self, editing_uuid: Option<&str>) -> Element<'_, Message> {
        let text_color = Color::from_rgb8(0xcd, 0xd6, 0xf4);
        let label_color = Color::from_rgb8(0xa6, 0xad, 0xc8);

        let title = if editing_uuid.is_some() {
            "Edit server"
        } else {
            "Add server"
        };

        let name_field = text_input("Display name (optional)", &self.dialogs.server_form_name)
            .on_input(Message::ServerFormNameChanged)
            .size(14)
            .padding(10);

        let host_field = text_input("hostname or IP", &self.dialogs.server_form_host)
            .id(crate::SERVER_FORM_HOST_ID)
            .on_input(Message::ServerFormHostChanged)
            .on_submit(Message::SaveAndConnectServer)
            .size(14)
            .padding(10);

        let user_field = text_input("username", &self.dialogs.server_form_user)
            .on_input(Message::ServerFormUserChanged)
            .size(14)
            .padding(10);

        let port_field = text_input("22", &self.dialogs.server_form_port)
            .on_input(Message::ServerFormPortChanged)
            .size(14)
            .padding(10)
            .width(80);

        let identity_field = text_input(
            "path to private key (optional)",
            &self.dialogs.server_form_identity,
        )
        .on_input(Message::ServerFormIdentityChanged)
        .size(14)
        .padding(10);

        let can_save = !self.dialogs.server_form_host.trim().is_empty();

        let save_btn = if can_save {
            button(text("Save").size(14).color(text_color))
                .on_press(Message::SaveServer)
                .padding([10, 24])
                .style(styles::secondary_button_style)
        } else {
            button(
                text("Save")
                    .size(14)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
            )
            .padding([10, 24])
            .style(styles::secondary_button_style)
        };

        let connect_btn = if can_save {
            button(
                text("Save & Connect")
                    .size(14)
                    .color(Color::from_rgb8(0x1e, 0x1e, 0x2e)),
            )
            .on_press(Message::SaveAndConnectServer)
            .padding([10, 24])
            .style(styles::primary_button_style)
        } else {
            button(
                text("Save & Connect")
                    .size(14)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
            )
            .padding([10, 24])
            .style(styles::secondary_button_style)
        };

        let form = column![
            // Back button
            button(
                text("\u{2190} Back")
                    .size(13)
                    .color(Color::from_rgb8(0x89, 0xb4, 0xfa))
            )
            .on_press(Message::BackToServerList)
            .padding([4, 8])
            .style(styles::ghost_button_style),
            Space::new().height(8),
            text(title).size(20).color(text_color),
            Space::new().height(12),
            column![text("Name").size(12).color(label_color), name_field].spacing(4),
            column![text("Host").size(12).color(label_color), host_field].spacing(4),
            row![
                column![text("Username").size(12).color(label_color), user_field]
                    .spacing(4)
                    .width(Length::Fill),
                column![text("Port").size(12).color(label_color), port_field]
                    .spacing(4)
                    .width(80),
            ]
            .spacing(8),
            column![
                text("Identity file").size(12).color(label_color),
                identity_field
            ]
            .spacing(4),
            Space::new().height(16),
            row![save_btn, connect_btn].spacing(8),
        ]
        .spacing(8)
        .padding(24)
        .max_width(420);

        scrollable(center(form).width(Length::Fill)).into()
    }

    /// Phase 6: forget-server confirmation dialog.
    fn view_forget_server_dialog(&self, label: &str) -> Element<'_, Message> {
        use iced::widget::container;
        container(
            column![
                text("Forget server?")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text(format!("Remove \"{label}\" from saved servers?"))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text("This does not affect sessions on the server.")
                    .size(12)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                Space::new().height(12),
                row![
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelForgetServer)
                        .padding([8, 16])
                        .style(styles::secondary_button_style),
                    Space::new().width(Length::Fill),
                    button(
                        text("Forget")
                            .size(13)
                            .color(Color::from_rgb8(0x1e, 0x1e, 0x2e))
                    )
                    .on_press(Message::ConfirmForgetServer)
                    .padding([8, 16])
                    .style(styles::danger_button_style),
                ]
                .width(Length::Fill),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(styles::dialog_container_style)
        .into()
    }

    /// Phase 6: new workspace dialog.
    fn view_new_workspace_dialog(&self, input: &str) -> Element<'_, Message> {
        use iced::widget::container;
        container(
            column![
                text("New workspace")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text("Create a new workspace on this server.")
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text_input("Workspace name", input)
                    .id(crate::NEW_WORKSPACE_INPUT_ID)
                    .on_input(Message::NewWorkspaceInputChanged)
                    .on_submit(Message::ConfirmNewWorkspace)
                    .size(13)
                    .padding(8),
                Space::new().height(8),
                row![
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelNewWorkspace)
                        .padding([8, 16])
                        .style(styles::secondary_button_style),
                    Space::new().width(Length::Fill),
                    button(text("Create").size(13))
                        .on_press(Message::ConfirmNewWorkspace)
                        .padding([8, 16])
                        .style(styles::primary_button_style),
                ]
                .width(Length::Fill),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(styles::dialog_container_style)
        .into()
    }

    /// Phase 6: rename workspace dialog.
    fn view_workspace_rename_dialog(&self, env: &str, input: &str) -> Element<'_, Message> {
        use iced::widget::container;
        container(
            column![
                text("Rename workspace")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text(format!("Renaming \"{env}\""))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text_input("New name", input)
                    .id(crate::RENAME_WORKSPACE_INPUT_ID)
                    .on_input(Message::RenameWorkspaceInputChanged)
                    .on_submit(Message::ConfirmRenameWorkspace)
                    .size(13)
                    .padding(8),
                Space::new().height(8),
                row![
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelRenameWorkspace)
                        .padding([8, 16])
                        .style(styles::secondary_button_style),
                    Space::new().width(Length::Fill),
                    button(text("Rename").size(13))
                        .on_press(Message::ConfirmRenameWorkspace)
                        .padding([8, 16])
                        .style(styles::primary_button_style),
                ]
                .width(Length::Fill),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(styles::dialog_container_style)
        .into()
    }

    /// Phase 6: remove workspace confirmation dialog.
    fn view_workspace_delete_dialog(&self, env: &str) -> Element<'_, Message> {
        use iced::widget::container;
        let is_last = self.dialogs.workspace_list.len() <= 1;

        if is_last {
            // Last workspace: offer to clear remote state and disconnect
            container(
                column![
                    text("Remove last workspace?")
                        .size(18)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    text(format!(
                        "\"{env}\" is the only workspace on this server."
                    ))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    text(
                        "You can remove it and start fresh, or clear all shellkeep data from the server and disconnect."
                    )
                    .size(12)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    Space::new().height(12),
                    row![
                        button(text("Cancel").size(13))
                            .on_press(Message::CancelDeleteWorkspace)
                            .padding([8, 16])
                            .style(styles::secondary_button_style),
                        Space::new().width(Length::Fill),
                        button(text("Remove").size(13))
                            .on_press(Message::ConfirmDeleteWorkspace)
                            .padding([8, 16])
                            .style(styles::secondary_button_style),
                        Space::new().width(8),
                        button(text("Clear & disconnect").size(13))
                            .on_press(Message::ConfirmDeleteLastWorkspaceAndClear)
                            .padding([8, 16])
                            .style(styles::danger_button_style),
                    ]
                    .width(Length::Fill),
                ]
                .spacing(8)
                .padding(24)
                .width(440),
            )
            .style(styles::dialog_container_style)
            .into()
        } else {
            container(
                column![
                    text("Remove workspace?")
                        .size(18)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    text(format!("Remove workspace \"{env}\"?"))
                        .size(13)
                        .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    text("All sessions will be terminated and windows closed.")
                        .size(12)
                        .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                    Space::new().height(8),
                    row![
                        button(text("Cancel").size(13))
                            .on_press(Message::CancelDeleteWorkspace)
                            .padding([8, 16])
                            .style(styles::secondary_button_style),
                        Space::new().width(Length::Fill),
                        button(text("Remove").size(13))
                            .on_press(Message::ConfirmDeleteWorkspace)
                            .padding([8, 16])
                            .style(styles::danger_button_style),
                    ]
                    .width(Length::Fill),
                ]
                .spacing(8)
                .padding(24)
                .width(360),
            )
            .style(styles::dialog_container_style)
            .into()
        }
    }
}
