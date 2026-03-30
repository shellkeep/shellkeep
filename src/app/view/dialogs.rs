// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::app::Message;
use crate::ShellKeep;

use iced::widget::{
    Space, button, center, column, container, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Color, Element, Length, Theme};

impl ShellKeep {
    /// FR-ENV-03: environment selection dialog overlay
    pub(crate) fn view_env_dialog(&self) -> Element<'_, Message> {
        let dialog_style = |_theme: &Theme| container::Style {
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
        };
        let btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
            text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let primary_btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x89, 0xb4, 0xfa))),
            text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };

        let filter = self.env_filter.to_lowercase();
        let filtered: Vec<&String> = self
            .env_list
            .iter()
            .filter(|e| filter.is_empty() || e.to_lowercase().contains(&filter))
            .collect();

        let mut env_buttons: Vec<Element<'_, Message>> = Vec::new();
        for env in &filtered {
            let is_selected = self.selected_env.as_ref() == Some(env);
            let is_current = **env == self.current_environment;
            let label = if is_current {
                format!("{} (current)", env)
            } else {
                (*env).clone()
            };
            let item_style = move |_theme: &Theme, _status: button::Status| {
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
            env_buttons.push(
                button(text(label).size(13))
                    .on_press(Message::SelectEnv((*env).clone()))
                    .padding([8, 12])
                    .width(Length::Fill)
                    .style(item_style)
                    .into(),
            );
        }

        let env_list = scrollable(column(env_buttons).spacing(2)).height(200);

        let dialog = container(
            column![
                text("Select environment")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text_input("Filter environments...", &self.env_filter)
                    .on_input(Message::EnvFilterChanged)
                    .size(13)
                    .padding(8),
                env_list,
                Space::new().height(8),
                row![
                    button(text("New environment").size(13))
                        .on_press(Message::NewEnvFromDialog)
                        .padding([8, 16])
                        .style(btn_style),
                    Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelEnvDialog)
                        .padding([8, 16])
                        .style(btn_style),
                    button(text("Connect").size(13))
                        .on_press(Message::ConfirmEnv)
                        .padding([8, 16])
                        .style(primary_btn_style),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .padding(24)
            .width(400),
        )
        .style(dialog_style);

        let scrim = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba8(0, 0, 0, 0.5))),
                    ..Default::default()
                }),
        )
        .on_press(Message::CancelEnvDialog);

        stack![scrim, center(dialog)].into()
    }

    /// FR-ENV-07: new environment creation dialog
    pub(crate) fn view_new_env_dialog(&self) -> Element<'_, Message> {
        let dialog_style = |_theme: &Theme| container::Style {
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
        };
        let btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
            text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let primary_btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x89, 0xb4, 0xfa))),
            text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };

        let dialog = container(
            column![
                text("New environment")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text("Enter a name for the new environment.")
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text_input("Environment name", &self.new_env_input)
                    .on_input(Message::NewEnvInputChanged)
                    .on_submit(Message::ConfirmNewEnv)
                    .size(13)
                    .padding(8),
                Space::new().height(8),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelNewEnv)
                        .padding([8, 16])
                        .style(btn_style),
                    button(text("Create").size(13))
                        .on_press(Message::ConfirmNewEnv)
                        .padding([8, 16])
                        .style(primary_btn_style),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(dialog_style);

        let scrim = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba8(0, 0, 0, 0.5))),
                    ..Default::default()
                }),
        )
        .on_press(Message::CancelNewEnv);

        stack![scrim, center(dialog)].into()
    }

    /// FR-ENV-08: rename environment dialog
    pub(crate) fn view_rename_env_dialog(&self) -> Element<'_, Message> {
        let dialog_style = |_theme: &Theme| container::Style {
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
        };
        let btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
            text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let primary_btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x89, 0xb4, 0xfa))),
            text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };

        let target_name = self.rename_env_target.as_deref().unwrap_or("unknown");

        let dialog = container(
            column![
                text("Rename environment")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text(format!("Renaming \"{}\"", target_name))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text_input("New name", &self.rename_env_input)
                    .on_input(Message::RenameEnvInputChanged)
                    .on_submit(Message::ConfirmRenameEnv)
                    .size(13)
                    .padding(8),
                Space::new().height(8),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelRenameEnv)
                        .padding([8, 16])
                        .style(btn_style),
                    button(text("Rename").size(13))
                        .on_press(Message::ConfirmRenameEnv)
                        .padding([8, 16])
                        .style(primary_btn_style),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(dialog_style);

        let scrim = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba8(0, 0, 0, 0.5))),
                    ..Default::default()
                }),
        )
        .on_press(Message::CancelRenameEnv);

        stack![scrim, center(dialog)].into()
    }

    /// FR-ENV-09: delete environment confirmation dialog
    pub(crate) fn view_delete_env_dialog(&self) -> Element<'_, Message> {
        let dialog_style = |_theme: &Theme| container::Style {
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
        };
        let btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
            text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let danger_btn_style = |_theme: &Theme, _status: button::Status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0xf3, 0x8b, 0xa8))),
            text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };

        let target_name = self.delete_env_target.as_deref().unwrap_or("unknown");
        // Count sessions in the target environment (stub: 0 for now)
        let session_count = 0_usize;
        let warning = if session_count > 0 {
            format!(
                "This will remove {session_count} session{} from this environment.",
                if session_count == 1 { "" } else { "s" }
            )
        } else {
            "This environment has no active sessions.".to_string()
        };

        let dialog = container(
            column![
                text("Delete environment?")
                    .size(18)
                    .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                text(format!("Environment: \"{}\"", target_name))
                    .size(13)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                text(warning)
                    .size(13)
                    .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                Space::new().height(8),
                row![
                    Space::new().width(Length::Fill),
                    button(text("Cancel").size(13))
                        .on_press(Message::CancelDeleteEnv)
                        .padding([8, 16])
                        .style(btn_style),
                    button(text("Delete").size(13))
                        .on_press(Message::ConfirmDeleteEnv)
                        .padding([8, 16])
                        .style(danger_btn_style),
                ]
                .spacing(8),
            ]
            .spacing(8)
            .padding(24)
            .width(360),
        )
        .style(dialog_style);

        let scrim = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(iced::Background::Color(Color::from_rgba8(0, 0, 0, 0.5))),
                    ..Default::default()
                }),
        )
        .on_press(Message::CancelDeleteEnv);

        stack![scrim, center(dialog)].into()
    }
}
