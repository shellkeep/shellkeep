// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! shellkeep — SSH terminal manager.
//!
//! Persistent sessions that survive everything.
//! Open source. Cross-platform. Zero server setup.

use iced::widget::{center, column, container, text};
use iced::{Element, Length, Subscription, Task, Theme};
use iced_term::ColorPalette;
use iced_term::settings::{BackendSettings, FontSettings, Settings, ThemeSettings};

const TERM_ID: u64 = 0;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: shellkeep [user@]host [-p port] [-i identity_file]");
        std::process::exit(1);
    }

    let ssh_args: Vec<String> = args[1..].to_vec();

    tracing::info!(
        "shellkeep v{} — ssh {}",
        env!("CARGO_PKG_VERSION"),
        ssh_args.join(" "),
    );

    iced::application(
        move || ShellKeep::new(ssh_args.clone()),
        ShellKeep::update,
        ShellKeep::view,
    )
    .title(ShellKeep::title)
    .subscription(ShellKeep::subscription)
    .theme(ShellKeep::theme)
    .window_size((900.0, 600.0))
    .antialiasing(true)
    .run()
}

struct ShellKeep {
    terminal: Option<iced_term::Terminal>,
    ssh_args: Vec<String>,
    title_text: String,
    error: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    TerminalEvent(iced_term::Event),
}

impl ShellKeep {
    fn new(ssh_args: Vec<String>) -> (Self, Task<Message>) {
        let settings = Settings {
            font: FontSettings {
                size: 14.0,
                ..FontSettings::default()
            },
            theme: ThemeSettings {
                color_pallete: Box::new(catppuccin_mocha()),
                ..Default::default()
            },
            backend: BackendSettings {
                program: "ssh".to_string(),
                args: ssh_args.clone(),
                ..Default::default()
            },
        };

        let title_text = format!("shellkeep — ssh {}", ssh_args.join(" "));

        match iced_term::Terminal::new(TERM_ID, settings) {
            Ok(terminal) => (
                ShellKeep {
                    terminal: Some(terminal),
                    ssh_args,
                    title_text,
                    error: None,
                },
                Task::none(),
            ),
            Err(e) => {
                tracing::error!("failed to create terminal: {e}");
                (
                    ShellKeep {
                        terminal: None,
                        ssh_args,
                        title_text,
                        error: Some(e.to_string()),
                    },
                    Task::none(),
                )
            }
        }
    }

    fn title(&self) -> String {
        self.title_text.clone()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TerminalEvent(iced_term::Event::BackendCall(id, cmd)) => {
                if let Some(ref mut terminal) = self.terminal {
                    if id == terminal.id {
                        let action = terminal.handle(iced_term::Command::ProxyToBackend(cmd));
                        match action {
                            iced_term::actions::Action::ChangeTitle(new_title) => {
                                self.title_text = format!("shellkeep — {new_title}");
                            }
                            iced_term::actions::Action::Shutdown => {
                                self.terminal = None;
                                self.error = Some("Session ended".to_string());
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if let Some(ref terminal) = self.terminal {
            container(iced_term::TerminalView::show(terminal).map(Message::TerminalEvent))
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            let msg = self.error.as_deref().unwrap_or("Terminal closed");
            center(
                column![text("shellkeep").size(24), text(msg).size(14),]
                    .spacing(10)
                    .align_x(iced::Alignment::Center),
            )
            .into()
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        if let Some(ref terminal) = self.terminal {
            terminal.subscription().map(Message::TerminalEvent)
        } else {
            Subscription::none()
        }
    }

    fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }
}

fn catppuccin_mocha() -> ColorPalette {
    ColorPalette {
        foreground: "#cdd6f4".into(),
        background: "#1e1e2e".into(),
        black: "#45475a".into(),
        red: "#f38ba8".into(),
        green: "#a6e3a1".into(),
        yellow: "#f9e2af".into(),
        blue: "#89b4fa".into(),
        magenta: "#f5c2e7".into(),
        cyan: "#94e2d5".into(),
        white: "#bac2de".into(),
        bright_black: "#585b70".into(),
        bright_red: "#f38ba8".into(),
        bright_green: "#a6e3a1".into(),
        bright_yellow: "#f9e2af".into(),
        bright_blue: "#89b4fa".into(),
        bright_magenta: "#f5c2e7".into(),
        bright_cyan: "#94e2d5".into(),
        bright_white: "#a6adc8".into(),
        ..Default::default()
    }
}
