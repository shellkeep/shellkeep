// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::ShellKeep;
use crate::app::Message;
use crate::app::view::styles;

use iced::widget::{
    Space, button, center, column, container, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Color, Element, Length};

impl ShellKeep {
    /// P21: unified workspace management dialog
    /// Combines select, new, rename, and delete into a single dialog.
    pub(crate) fn view_workspace_dialog(&self) -> Element<'_, Message> {
        let text_color = Color::from_rgb8(0xcd, 0xd6, 0xf4);
        let label_color = Color::from_rgb8(0xa6, 0xad, 0xc8);

        let filter = self.dialogs.workspace_filter.to_lowercase();
        let filtered: Vec<&String> = self
            .dialogs
            .workspace_list
            .iter()
            .filter(|e| filter.is_empty() || e.to_lowercase().contains(&filter))
            .collect();

        let mut env_rows: Vec<Element<'_, Message>> = Vec::new();
        for env in &filtered {
            let is_selected = self.dialogs.selected_workspace.as_ref() == Some(env);
            let is_current = **env == self.current_workspace;
            let label = if is_current {
                format!("{env} (current)")
            } else {
                (*env).clone()
            };
            let item_style = move |_theme: &iced::Theme, _status: button::Status| {
                let bg = if is_selected {
                    Color::from_rgb8(0x45, 0x47, 0x5a)
                } else {
                    Color::from_rgb8(0x31, 0x32, 0x44)
                };
                button::Style {
                    background: Some(iced::Background::Color(bg)),
                    text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            };

            let env_name = (*env).clone();
            let env_name2 = (*env).clone();

            // Row: [Select button (fill)] [Rename icon] [Delete icon]
            let select_btn = button(text(label).size(13))
                .on_press(Message::SelectWorkspace((*env).clone()))
                .padding([8, 12])
                .width(Length::Fill)
                .style(item_style);

            let rename_btn = button(text("\u{270E}").size(12))
                .on_press(Message::ShowRenameWorkspaceDialog(env_name))
                .padding([6, 8])
                .style(styles::ghost_button_style);

            let delete_btn = button(
                text("\u{1F5D1}")
                    .size(12)
                    .color(Color::from_rgb8(0xf3, 0x8b, 0xa8)),
            )
            .on_press(Message::ShowDeleteWorkspaceDialog(env_name2))
            .padding([6, 8])
            .style(styles::ghost_button_style);

            env_rows.push(
                row![select_btn, rename_btn, delete_btn]
                    .spacing(4)
                    .align_y(iced::Alignment::Center)
                    .into(),
            );
        }

        let env_list = scrollable(column(env_rows).spacing(2)).height(200);

        // Inline "add new" section
        let new_section: Element<'_, Message> = if self.dialogs.show_new_workspace_dialog {
            row![
                text_input(
                    "New workspace name",
                    &self.dialogs.new_workspace_dialog_input
                )
                .id(crate::NEW_WORKSPACE_INPUT_ID)
                .on_input(Message::NewWorkspaceDialogInput)
                .on_submit(Message::ConfirmNewWorkspaceDialog)
                .size(13)
                .padding(8)
                .width(Length::Fill),
                button(text("Add").size(13))
                    .on_press(Message::ConfirmNewWorkspaceDialog)
                    .padding([8, 12])
                    .style(styles::primary_button_style),
                button(text("\u{00D7}").size(14))
                    .on_press(Message::CancelNewWorkspaceDialog)
                    .padding([6, 8])
                    .style(styles::ghost_button_style),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            button(text("+ New workspace").size(13).color(label_color))
                .on_press(Message::NewWorkspaceFromDialog)
                .padding([8, 12])
                .style(styles::ghost_button_style)
                .into()
        };

        let dialog = container(
            column![
                text("Workspaces").size(18).color(text_color),
                text_input("Filter...", &self.dialogs.workspace_filter)
                    .on_input(Message::WorkspaceFilterChanged)
                    .size(13)
                    .padding(8),
                env_list,
                new_section,
                Space::new().height(4),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelWorkspaceDialog)
                        .padding([8, 16])
                        .style(styles::secondary_button_style),
                    button(text("Connect").size(13))
                        .on_press(Message::ConfirmWorkspaceSelection)
                        .padding([8, 16])
                        .style(styles::primary_button_style),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .padding(24)
            .width(420),
        )
        .style(styles::dialog_container_style);

        let scrim = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(styles::scrim_style),
        )
        .on_press(Message::CancelWorkspaceDialog)
        .interaction(iced::mouse::Interaction::Idle);

        stack![scrim, center(dialog)].into()
    }

    /// FR-ENV-08: rename workspace dialog (kept as separate overlay for inline rename)
    pub(crate) fn view_rename_workspace_dialog(&self) -> Element<'_, Message> {
        let target_name = self
            .dialogs
            .rename_workspace_target
            .as_deref()
            .unwrap_or("unknown");

        let dialog = container(
            column![
                text("Rename workspace")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text(format!("Renaming \"{target_name}\""))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text_input("New name", &self.dialogs.rename_workspace_dialog_input)
                    .id(crate::RENAME_WORKSPACE_INPUT_ID)
                    .on_input(Message::RenameWorkspaceDialogInput)
                    .on_submit(Message::ConfirmRenameWorkspaceDialog)
                    .size(13)
                    .padding(8),
                Space::new().height(8),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelRenameWorkspaceDialog)
                        .padding([8, 16])
                        .style(styles::secondary_button_style),
                    button(text("Rename").size(13))
                        .on_press(Message::ConfirmRenameWorkspaceDialog)
                        .padding([8, 16])
                        .style(styles::primary_button_style),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(styles::dialog_container_style);

        let scrim = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(styles::scrim_style),
        )
        .on_press(Message::CancelRenameWorkspaceDialog)
        .interaction(iced::mouse::Interaction::Idle);

        stack![scrim, center(dialog)].into()
    }

    /// FR-ENV-09: delete workspace confirmation dialog
    pub(crate) fn view_delete_workspace_dialog(&self) -> Element<'_, Message> {
        let target_name = self
            .dialogs
            .delete_workspace_target
            .as_deref()
            .unwrap_or("unknown");
        let session_count = 0_usize;
        let warning = if session_count > 0 {
            format!(
                "This will remove {session_count} session{} from this workspace.",
                if session_count == 1 { "" } else { "s" }
            )
        } else {
            "This workspace has no active sessions.".to_string()
        };

        let dialog = container(
            column![
                text("Delete workspace?")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text(format!("Workspace: \"{target_name}\""))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text(warning)
                    .size(13)
                    .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                Space::new().height(8),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelDeleteWorkspaceDialog)
                        .padding([8, 16])
                        .style(styles::secondary_button_style),
                    button(text("Delete").size(13))
                        .on_press(Message::ConfirmDeleteWorkspaceDialog)
                        .padding([8, 16])
                        .style(styles::danger_button_style),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(styles::dialog_container_style);

        let scrim = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(styles::scrim_style),
        )
        .on_press(Message::CancelDeleteWorkspaceDialog)
        .interaction(iced::mouse::Interaction::Idle);

        stack![scrim, center(dialog)].into()
    }
}
