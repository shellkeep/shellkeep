// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! View layer — builds the iced widget tree from application state.
//!
//! The view is composed of stacked layers (bottom to top):
//! 1. Tab bar + terminal content + status bar (the main layout)
//! 2. Close-tab confirmation dialog overlay
//! 3. Environment selection/management dialog overlay
//! 4. Host key verification dialog overlay
//! 5. Password prompt dialog overlay
//! 6. Lock conflict dialog overlay
//!
//! Sub-modules handle specific view concerns:
//! - `welcome` — first-use / connection screen
//! - `tab_bar` — tab strip with context menus
//! - `dead_tab` — disconnected session view with reconnect options
//! - `status_bar` — bottom bar with connection info and latency
//! - `dialogs` — environment CRUD dialogs
//! - `styles` — iced style functions (buttons, containers, scrim)

mod dead_tab;
mod dialogs;
mod status_bar;
pub(crate) mod styles;
mod tab_bar;
mod welcome;

use crate::ShellKeep;
use crate::app::Message;
use crate::app::tab::SPINNER_FRAMES;

use crate::app::WindowKind;

use iced::widget::{
    Space, button, center, column, container, mouse_area, row, stack, text, text_input,
};
use iced::{Color, Element, Length, Theme, window};
use shellkeep::i18n;

impl ShellKeep {
    pub(crate) fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        let win = match self.windows.get(&window_id) {
            Some(w) => w,
            None => return text("Unknown window").into(),
        };

        // Phase 5: control window renders the welcome/server view
        if win.kind == WindowKind::Control {
            return self.view_control_window();
        }

        if win.tabs.is_empty() {
            // Session window with no tabs — show a placeholder
            return center(
                column![
                    text("No sessions in this window")
                        .size(16)
                        .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    text("Connect from the control window or press Ctrl+Shift+T")
                        .size(12)
                        .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                ]
                .spacing(8)
                .align_x(iced::Alignment::Center),
            )
            .into();
        }

        if win.show_welcome {
            let tab_bar = self.view_tab_bar(win);
            return column![tab_bar, self.view_welcome()].into();
        }

        let tab_bar = self.view_tab_bar(win);
        let content: Element<'_, Message> = if let Some(tab) = win.tabs.get(win.active_tab) {
            if tab.is_dead() {
                // INV-DEAD-1: dead session never accepts input — the TerminalView
                // widget is not rendered, so keyboard events cannot reach it.
                self.view_dead_tab(tab)
            } else if let Some(ref terminal) = tab.terminal {
                // INV-CONN-2: before auth completes, the SSH I/O subscription only starts
                // when ssh_channel_holder is set (after SshConnected). Input is buffered
                // in ssh_writer_tx but not sent until the channel is ready.
                // Show "Connecting..." overlay if russh tab without channel yet
                if tab.is_russh() && !tab.has_channel() {
                    let phase_text = tab
                        .connection_phase_text()
                        .unwrap_or_else(|| i18n::t(i18n::CONNECTING).to_string());
                    stack![
                        container(
                            iced_term::TerminalView::show(terminal).map(Message::TerminalEvent)
                        )
                        .width(Length::Fill)
                        .height(Length::Fill),
                        center(
                            text(phase_text)
                                .size(16)
                                .color(Color::from_rgb8(0xf9, 0xe2, 0xaf))
                        ),
                    ]
                    .into()
                } else {
                    container(iced_term::TerminalView::show(terminal).map(Message::TerminalEvent))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                }
            } else if tab.is_auto_reconnect() {
                // FR-RECONNECT-02: spinner overlay with attempt count and countdown
                let spinner = SPINNER_FRAMES[self.spinner_frame % SPINNER_FRAMES.len()];
                let attempt_text = format!(
                    "{} {}/{}",
                    i18n::t(i18n::RECONNECTING),
                    tab.reconnect_attempts(),
                    self.config.ssh.reconnect_max_attempts
                );
                let delay = tab.reconnect_delay_ms();
                let countdown_text = if delay > 0 {
                    let elapsed = tab
                        .reconnect_started()
                        .map(|t| t.elapsed().as_millis() as u64)
                        .unwrap_or(0);
                    let remaining_ms = delay.saturating_sub(elapsed);
                    let remaining_secs = remaining_ms.div_ceil(1000);
                    if remaining_secs > 0 {
                        format!("Next retry in {}s", remaining_secs)
                    } else {
                        "Retrying now...".to_string()
                    }
                } else {
                    i18n::t(i18n::CONNECTING).to_string()
                };
                center(
                    column![
                        text(format!("{spinner}")).size(48),
                        text(i18n::t(i18n::CONNECTION_LOST))
                            .size(20)
                            .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                        text(attempt_text)
                            .size(14)
                            .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                        text(countdown_text)
                            .size(12)
                            .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                    ]
                    .spacing(12)
                    .align_x(iced::Alignment::Center),
                )
                .into()
            } else {
                center(text(i18n::t(i18n::TERMINAL_NOT_AVAILABLE))).into()
            }
        } else {
            center(text(i18n::t(i18n::NO_ACTIVE_TAB))).into()
        };

        // FR-TABS-09: search bar overlay
        let content: Element<'_, Message> = if self.search.active {
            let match_info: Element<'_, Message> = if self.search.last_match.is_some() {
                text(i18n::t(i18n::MATCH_FOUND))
                    .size(11)
                    .color(Color::from_rgb8(0xa6, 0xe3, 0xa1))
                    .into()
            } else if !self.search.input.is_empty() {
                text(i18n::t(i18n::NO_MATCHES))
                    .size(11)
                    .color(Color::from_rgb8(0xf3, 0x8b, 0xa8))
                    .into()
            } else {
                Space::new().width(0).into()
            };
            let search_bar = container(
                row![
                    text_input("Search...", &self.search.input)
                        .id("search-input")
                        .on_input(Message::SearchInputChanged)
                        .on_submit(Message::SearchNext)
                        .size(13)
                        .padding(6)
                        .width(280),
                    button(text(i18n::t(i18n::PREVIOUS)).size(11))
                        .on_press(Message::SearchPrev)
                        .padding([4, 8])
                        .style(styles::search_button_style),
                    button(text(i18n::t(i18n::NEXT)).size(11))
                        .on_press(Message::SearchNext)
                        .padding([4, 8])
                        .style(styles::search_button_style),
                    match_info,
                    Space::new().width(Length::Fill),
                    button(text(i18n::t(i18n::CLOSE)).size(11))
                        .on_press(Message::SearchClose)
                        .padding([4, 8])
                        .style(styles::search_button_style),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .padding([4, 8]),
            )
            .width(Length::Fill)
            .style(styles::search_bar_style);
            column![search_bar, content].into()
        } else {
            content
        };

        let status_bar = self.view_status_bar(win);

        // Wrap with tab context menu if active
        let main_view: Element<'_, Message> = if let Some((tab_idx, _x, _y)) = win.tab_context_menu
        {
            let mut menu_items: Vec<Element<'_, Message>> = Vec::new();

            if tab_idx > 0 {
                menu_items.push(
                    button(text(i18n::t(i18n::MOVE_LEFT)).size(13))
                        .on_press(Message::TabMoveLeft(tab_idx))
                        .padding([8, 16])
                        .width(200)
                        .style(styles::context_menu_style)
                        .into(),
                );
            }
            if tab_idx + 1 < win.tabs.len() {
                menu_items.push(
                    button(text(i18n::t(i18n::MOVE_RIGHT)).size(13))
                        .on_press(Message::TabMoveRight(tab_idx))
                        .padding([8, 16])
                        .width(200)
                        .style(styles::context_menu_style)
                        .into(),
                );
            }
            menu_items.push(
                button(text(format!("{}         F2", i18n::t(i18n::RENAME))).size(13))
                    .on_press(Message::StartRename(tab_idx))
                    .padding([8, 16])
                    .width(200)
                    .style(styles::context_menu_style)
                    .into(),
            );
            menu_items.push(
                button(text("Hide (keep on server)").size(13))
                    .on_press(Message::HideTab(tab_idx))
                    .padding([8, 16])
                    .width(200)
                    .style(styles::context_menu_style)
                    .into(),
            );
            // Separator
            menu_items.push(
                container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(styles::separator_style)
                    .into(),
            );
            if win.tabs.len() > 1 {
                menu_items.push(
                    button(text("Close other tabs").size(13))
                        .on_press(Message::CloseOtherTabs(tab_idx))
                        .padding([8, 16])
                        .width(200)
                        .style(styles::context_menu_style)
                        .into(),
                );
            }
            if tab_idx + 1 < win.tabs.len() {
                menu_items.push(
                    button(text("Close tabs to the right").size(13))
                        .on_press(Message::CloseTabsToRight(tab_idx))
                        .padding([8, 16])
                        .width(200)
                        .style(styles::context_menu_style)
                        .into(),
                );
            }
            menu_items.push(
                button(
                    text(i18n::t(i18n::CLOSE_TAB))
                        .size(13)
                        .color(Color::from_rgb8(0xf3, 0x8b, 0xa8)),
                )
                .on_press(Message::CloseTab(tab_idx))
                .padding([8, 16])
                .width(200)
                .style(styles::context_menu_style)
                .into(),
            );

            let tab_menu = container(column(menu_items).spacing(1))
                .padding(4)
                .style(styles::context_menu_container_style);

            // Bug 4 fix: explicitly make the dismiss overlay fully transparent
            // so it doesn't render a visible background over the terminal.
            let dismiss = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(|_: &Theme| container::Style {
                        background: None,
                        ..Default::default()
                    }),
            )
            .on_press(Message::ContextMenuDismiss);

            stack![
                column![tab_bar, content, status_bar],
                dismiss,
                container(tab_menu).padding(iced::Padding {
                    top: 28.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: (tab_idx as f32) * 120.0,
                }),
            ]
            .into()
        } else if let Some((x, y)) = win.context_menu {
            let menu = container(
                column![
                    button(text(format!("{}        Ctrl+Shift+C", i18n::t(i18n::COPY))).size(13))
                        .on_press(Message::ContextMenuCopy)
                        .padding([8, 16])
                        .width(250)
                        .style(styles::context_menu_style),
                    button(text(format!("{}       Ctrl+Shift+V", i18n::t(i18n::PASTE))).size(13))
                        .on_press(Message::ContextMenuPaste)
                        .padding([8, 16])
                        .width(250)
                        .style(styles::context_menu_style),
                ]
                .spacing(1),
            )
            .padding(4)
            .style(styles::context_menu_container_style);

            let dismiss_area = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::ContextMenuDismiss);

            stack![
                column![tab_bar, content, status_bar],
                dismiss_area,
                container(menu).padding(iced::Padding {
                    top: y,
                    right: 0.0,
                    bottom: 0.0,
                    left: x,
                }),
            ]
            .into()
        } else if win.show_restore_dropdown {
            let hidden_items = self.build_hidden_session_items();
            let dropdown_content: Element<'_, Message> = if hidden_items.is_empty() {
                container(
                    text("No hidden sessions")
                        .size(13)
                        .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                )
                .padding([8, 16])
                .into()
            } else {
                column(hidden_items).spacing(1).into()
            };

            let dropdown = container(dropdown_content)
                .padding(4)
                .style(styles::context_menu_container_style);

            // Bug 5 fix: position the dropdown near the restore button
            // rather than at the far right edge of the window. Estimate
            // position from tab count (each tab ~120px) + buttons (~60px).
            let dropdown_left = (win.tabs.len() as f32) * 120.0 + 40.0;
            stack![
                column![tab_bar, content, status_bar],
                mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(|_: &Theme| container::Style {
                            background: None,
                            ..Default::default()
                        }),
                )
                .on_press(Message::DismissRestoreDropdown),
                // Position dropdown below tab bar, near the restore button
                container(dropdown).padding(iced::Padding {
                    top: 28.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: dropdown_left,
                }),
            ]
            .into()
        } else {
            column![tab_bar, content, status_bar].into()
        };

        // Item 5: window rename overlay
        let main_view: Element<'_, Message> = if self.renaming_window.is_some() {
            let rename_dialog = container(
                column![
                    text("Rename window")
                        .size(16)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    text_input("Window name", &self.window_rename_input)
                        .on_input(Message::WindowRenameInputChanged)
                        .on_submit(Message::FinishWindowRename)
                        .size(14)
                        .padding(8)
                        .width(300),
                    row![
                        button(text("Rename").size(13))
                            .on_press(Message::FinishWindowRename)
                            .padding([8, 16])
                            .style(styles::primary_button_style),
                        button(text("Cancel").size(13))
                            .on_press(Message::CancelWindowRename)
                            .padding([8, 16])
                            .style(styles::secondary_button_style),
                    ]
                    .spacing(8),
                ]
                .spacing(8)
                .padding(24),
            )
            .style(styles::dialog_container_style);

            let scrim = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(styles::scrim_style),
            )
            .on_press(Message::CancelWindowRename);

            stack![main_view, scrim, center(rename_dialog)].into()
        } else {
            main_view
        };

        // FR-TABS-17: close confirmation dialog overlay
        // FR-SESSION-10a: close-tab confirmation dialog
        let main_view: Element<'_, Message> = if self.dialogs.pending_close_tabs.is_some() {
            let count = self
                .dialogs
                .pending_close_tabs
                .as_ref()
                .map(|v| v.len())
                .unwrap_or(0);
            let msg = if count == 1 {
                "This will terminate the session on the server.\nThis cannot be undone.".to_string()
            } else {
                format!(
                    "This will terminate {count} sessions on the server.\nThis cannot be undone."
                )
            };
            let dialog = container(
                column![
                    text("Terminate session?")
                        .size(18)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    text(msg).size(13).color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    Space::new().height(12),
                    row![
                        button(text("Cancel").size(14))
                            .on_press(Message::CancelCloseTabs)
                            .padding([10, 24])
                            .style(styles::secondary_button_style),
                        Space::new().width(Length::Fill),
                        button(
                            text("Terminate")
                                .size(14)
                                .color(Color::from_rgb8(0x1e, 0x1e, 0x2e))
                        )
                        .on_press(Message::ConfirmCloseTabs)
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
            center(dialog).into()
        } else if self.dialogs.show_close_dialog {
            let active_count = self
                .all_tabs()
                .filter(|t| !t.is_dead() && t.terminal.is_some())
                .count();
            let session_word = if active_count == 1 {
                "session"
            } else {
                "sessions"
            };
            let dialog = container(
                column![
                    text("Close shellkeep?")
                        .size(18)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    text(format!(
                        "Your {active_count} active {session_word} will keep running on the server."
                    ))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    text("To terminate sessions, close the tabs before quitting.")
                        .size(12)
                        .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
                    Space::new().height(12),
                    row![
                        button(text(i18n::t(i18n::CANCEL)).size(13))
                            .on_press(Message::CloseDialogCancel)
                            .padding([8, 16])
                            .style(styles::secondary_button_style),
                        Space::new().width(Length::Fill),
                        button(
                            text("Close")
                                .size(13)
                                .color(Color::from_rgb8(0x1e, 0x1e, 0x2e))
                        )
                        .on_press(Message::CloseDialogClose)
                        .padding([8, 16])
                        .style(styles::primary_button_style),
                    ]
                    .width(Length::Fill),
                ]
                .spacing(8)
                .padding(24),
            )
            .style(styles::dialog_container_style);

            let scrim = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(styles::scrim_style),
            )
            .on_press(Message::CloseDialogCancel);

            stack![main_view, scrim, center(dialog),].into()
        } else {
            main_view
        };

        // FR-ENV-03: environment selection dialog overlay
        let main_view: Element<'_, Message> = if self.dialogs.show_env_dialog {
            stack![main_view, self.view_env_dialog()].into()
        } else if self.dialogs.show_new_env_dialog {
            stack![main_view, self.view_new_env_dialog()].into()
        } else if self.dialogs.show_rename_env_dialog {
            stack![main_view, self.view_rename_env_dialog()].into()
        } else if self.dialogs.show_delete_env_dialog {
            stack![main_view, self.view_delete_env_dialog()].into()
        } else {
            main_view
        };

        // FR-CONN-03, FR-CONN-02: host key verification dialog overlay
        let main_view: Element<'_, Message> =
            if let Some(ref prompt) = self.dialogs.pending_host_key_prompt {
                use shellkeep::ssh::known_hosts::HostKeyStatus;
                let label_color = Color::from_rgb8(0xa6, 0xad, 0xc8);
                let text_color = Color::from_rgb8(0xcd, 0xd6, 0xf4);

                let dialog = match prompt.status {
                    HostKeyStatus::Unknown => {
                        let host_label = format!("Host: {}:{}", prompt.host, prompt.port);
                        let fp_label = format!("Fingerprint: {}", prompt.fingerprint);
                        container(
                            column![
                                text("Unknown Host Key").size(18).color(text_color),
                                Space::new().height(8),
                                text(host_label.clone()).size(13).color(label_color),
                                text(fp_label.clone()).size(13).color(label_color),
                                Space::new().height(8),
                                text("This host is not in your known_hosts file.")
                                    .size(13)
                                    .color(label_color),
                                Space::new().height(12),
                                row![
                                    button(text("Accept and save").size(13))
                                        .on_press(Message::HostKeyAcceptSave)
                                        .padding([8, 16])
                                        .style(styles::primary_button_style),
                                    button(text("Connect once").size(13))
                                        .on_press(Message::HostKeyConnectOnce)
                                        .padding([8, 16])
                                        .style(styles::secondary_button_style),
                                    button(text("Cancel").size(13))
                                        .on_press(Message::HostKeyReject)
                                        .padding([8, 16])
                                        .style(styles::danger_button_style),
                                ]
                                .spacing(8),
                            ]
                            .spacing(4)
                            .padding(24),
                        )
                        .style(styles::dialog_container_style)
                    }
                    HostKeyStatus::Changed => {
                        let host_label = format!("Host: {}:{}", prompt.host, prompt.port);
                        let new_fp = format!("New: {}", prompt.fingerprint);
                        let old_fp = prompt
                            .old_fingerprint
                            .as_deref()
                            .map(|fp| format!("Old: {fp}"))
                            .unwrap_or_default();
                        container(
                            column![
                                text("WARNING: HOST KEY HAS CHANGED")
                                    .size(18)
                                    .color(Color::from_rgb8(0xf3, 0x8b, 0xa8)),
                                Space::new().height(8),
                                text(host_label.clone()).size(13).color(label_color),
                                text(old_fp.clone()).size(13).color(label_color),
                                text(new_fp.clone()).size(13).color(label_color),
                                Space::new().height(8),
                                text("This may indicate a man-in-the-middle attack.")
                                    .size(13)
                                    .color(Color::from_rgb8(0xf3, 0x8b, 0xa8)),
                                text("Update your known_hosts file manually if this is expected.")
                                    .size(13)
                                    .color(label_color),
                                Space::new().height(12),
                                button(text("Disconnect").size(13))
                                    .on_press(Message::HostKeyChangedDismiss)
                                    .padding([8, 16])
                                    .style(styles::danger_button_style),
                            ]
                            .spacing(4)
                            .padding(24),
                        )
                        .style(styles::dialog_container_style)
                    }
                    HostKeyStatus::Known => {
                        // Should not happen, but dismiss gracefully
                        container(text(""))
                    }
                };

                let scrim = mouse_area(
                    container(Space::new().width(Length::Fill).height(Length::Fill))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(styles::scrim_style),
                )
                .on_press(Message::HostKeyReject);

                stack![main_view, scrim, center(dialog)].into()
            } else {
                main_view
            };

        // FR-CONN-09: password prompt dialog overlay
        let main_view: Element<'_, Message> = if self.dialogs.show_password_dialog {
            let label_color = Color::from_rgb8(0xa6, 0xad, 0xc8);
            let text_color = Color::from_rgb8(0xcd, 0xd6, 0xf4);

            let title = if let Some(ref conn) = self.current_conn {
                format!("Password for {}@{}", conn.key.username, conn.key.host)
            } else {
                "Password required".to_string()
            };

            let dialog = container(
                column![
                    text(title.clone()).size(18).color(text_color),
                    Space::new().height(8),
                    text("Key-based authentication failed. Enter password:")
                        .size(13)
                        .color(label_color),
                    Space::new().height(8),
                    text_input("Password", &self.dialogs.password_input)
                        .on_input(Message::PasswordInputChanged)
                        .on_submit(Message::PasswordSubmit)
                        .secure(true)
                        .padding(8)
                        .width(300),
                    Space::new().height(12),
                    row![
                        button(text("Connect").size(13))
                            .on_press(Message::PasswordSubmit)
                            .padding([8, 16])
                            .style(styles::primary_button_style),
                        button(text("Cancel").size(13))
                            .on_press(Message::PasswordCancel)
                            .padding([8, 16])
                            .style(styles::secondary_button_style),
                    ]
                    .spacing(8),
                ]
                .spacing(4)
                .padding(24),
            )
            .style(styles::dialog_container_style);

            let scrim = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(styles::scrim_style),
            )
            .on_press(Message::PasswordCancel);

            stack![main_view, scrim, center(dialog)].into()
        } else {
            main_view
        };

        // FR-LOCK-05: lock conflict dialog overlay
        let main_view: Element<'_, Message> = if self.dialogs.show_lock_dialog {
            let text_color = Color::from_rgb8(0xcd, 0xd6, 0xf4);
            let label_color = Color::from_rgb8(0xa6, 0xad, 0xc8);

            let dialog = container(
                column![
                    text("Another shellkeep instance connected")
                        .size(18)
                        .color(text_color),
                    Space::new().height(8),
                    text(&self.dialogs.lock_info_text)
                        .size(13)
                        .color(label_color),
                    Space::new().height(8),
                    text("Taking over will disconnect the other instance.")
                        .size(13)
                        .color(label_color),
                    Space::new().height(12),
                    row![
                        button(text("Take over").size(13))
                            .on_press(Message::LockTakeOver)
                            .padding([8, 16])
                            .style(styles::warn_button_style),
                        button(text("Cancel").size(13))
                            .on_press(Message::LockCancel)
                            .padding([8, 16])
                            .style(styles::secondary_button_style),
                    ]
                    .spacing(8),
                ]
                .spacing(4)
                .padding(24),
            )
            .style(styles::dialog_container_style);

            let scrim = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(styles::scrim_style),
            )
            .on_press(Message::LockCancel);

            stack![main_view, scrim, center(dialog)].into()
        } else {
            main_view
        };

        // Toast overlay
        let main_view: Element<'_, Message> = if let Some((ref msg, _)) = self.toast {
            let toast_widget =
                container(text(msg).size(13).color(Color::from_rgb8(0xcd, 0xd6, 0xf4)))
                    .padding([8, 16])
                    .style(styles::toast_style);

            stack![
                main_view,
                column![
                    Space::new().height(Length::Fill),
                    container(row![Space::new().width(Length::Fill), toast_widget,])
                        .padding(16)
                        .width(Length::Fill)
                        .align_bottom(Length::Fill),
                ],
            ]
            .into()
        } else {
            main_view
        };

        main_view
    }
}
