// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! shellkeep — SSH terminal manager.
//!
//! Persistent sessions that survive everything.
//! Open source. Cross-platform. Zero server setup.

mod config;
mod state;

use config::Config;
use iced::keyboard;
use iced::widget::{Space, button, center, column, container, row, scrollable, text, text_input};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use iced_term::ColorPalette;
use iced_term::settings::{BackendSettings, FontSettings, Settings, ThemeSettings};
use state::recent::{RecentConnection, RecentConnections};

fn main() -> iced::Result {
    let args: Vec<String> = std::env::args().collect();

    // Handle --version and --help before initializing anything
    for arg in &args[1..] {
        match arg.as_str() {
            "--version" | "-V" => {
                println!("shellkeep {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "shellkeep {} — SSH sessions that survive everything\n\n\
                     Usage: shellkeep [user@]host [-p port] [-i identity] [-l user]\n\
                     \n\
                     Options:\n  \
                       -p PORT       SSH port (default: 22)\n  \
                       -i FILE       Identity file (private key)\n  \
                       -l USER       Login user name\n  \
                       --debug       Enable debug logging\n  \
                       --version     Show version\n  \
                       --help        Show this help\n\
                     \n\
                     Without arguments, opens the welcome screen.\n\
                     https://github.com/shellkeep/shellkeep",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }

    let log_level = if args.iter().any(|a| a == "--debug") {
        "debug"
    } else {
        "info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();

    // Parse SSH args (skip --debug which is shellkeep-specific)
    let ssh_relevant: Vec<String> = args[1..]
        .iter()
        .filter(|a| *a != "--debug")
        .cloned()
        .collect();

    let initial_ssh_args: Option<Vec<String>> =
        if ssh_relevant.is_empty() || ssh_relevant.iter().all(|a| a.starts_with('-')) {
            None
        } else {
            Some(ssh_relevant)
        };

    tracing::info!("shellkeep v{} starting", env!("CARGO_PKG_VERSION"));

    iced::application(
        move || ShellKeep::new(initial_ssh_args.clone()),
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

// ---------------------------------------------------------------------------
// Tab
// ---------------------------------------------------------------------------

struct Tab {
    id: u64,
    label: String,
    terminal: Option<iced_term::Terminal>,
    ssh_args: Vec<String>,
    dead: bool,
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct ShellKeep {
    tabs: Vec<Tab>,
    active_tab: usize,
    next_id: u64,
    show_welcome: bool,
    renaming_tab: Option<usize>,
    rename_input: String,

    // Welcome screen state
    host_input: String,
    port_input: String,
    user_input: String,
    identity_input: String,

    config: Config,
    recent: RecentConnections,
    title_text: String,
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    TerminalEvent(iced_term::Event),
    SelectTab(usize),
    CloseTab(usize),
    NewTab,
    ReconnectTab(usize),
    ConnectRecent(usize),
    StartRenameTab(usize),
    RenameInputChanged(String),
    FinishRename,
    CancelRename,
    HostInputChanged(String),
    PortInputChanged(String),
    UserInputChanged(String),
    IdentityInputChanged(String),
    Connect,
    KeyEvent(keyboard::Event),
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

impl ShellKeep {
    fn new(initial_ssh_args: Option<Vec<String>>) -> (Self, Task<Message>) {
        let username = whoami::username();
        let config = Config::load();
        let recent = RecentConnections::load();
        let default_port = config.ssh.default_port.to_string();
        let mut app = ShellKeep {
            tabs: Vec::new(),
            active_tab: 0,
            next_id: 0,
            show_welcome: false,
            renaming_tab: None,
            rename_input: String::new(),
            host_input: String::new(),
            port_input: default_port,
            user_input: username,
            identity_input: String::new(),
            config,
            recent,
            title_text: "shellkeep".to_string(),
            error: None,
        };

        if let Some(ssh_args) = initial_ssh_args {
            let label = ssh_args.join(" ");
            app.open_tab(&ssh_args, &label);
        }

        (app, Task::none())
    }

    fn open_tab(&mut self, ssh_args: &[String], label: &str) {
        let id = self.next_id;
        self.next_id += 1;

        let settings = Settings {
            font: FontSettings {
                size: self.config.terminal.font_size,
                ..FontSettings::default()
            },
            theme: ThemeSettings {
                color_pallete: Box::new(catppuccin_mocha()),
                ..Default::default()
            },
            backend: BackendSettings {
                program: "ssh".to_string(),
                args: ssh_args.to_vec(),
                ..Default::default()
            },
        };

        match iced_term::Terminal::new(id, settings) {
            Ok(terminal) => {
                self.tabs.push(Tab {
                    id,
                    label: label.to_string(),
                    terminal: Some(terminal),
                    ssh_args: ssh_args.to_vec(),
                    dead: false,
                });
                self.active_tab = self.tabs.len() - 1;
                self.error = None;
                self.update_title();
                tracing::info!("opened tab {id}: {label}");
            }
            Err(e) => {
                tracing::error!("failed to create terminal: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            let tab = self.tabs.remove(index);
            tracing::info!("closed tab {}: {}", tab.id, tab.label);
            if self.active_tab >= self.tabs.len() && self.active_tab > 0 {
                self.active_tab -= 1;
            }
            self.update_title();
        }
    }

    fn reconnect_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }

        let ssh_args = self.tabs[index].ssh_args.clone();
        let label = self.tabs[index].label.clone();

        // Remove old dead tab and open fresh one
        self.tabs.remove(index);

        let id = self.next_id;
        self.next_id += 1;

        let settings = Settings {
            font: FontSettings {
                size: self.config.terminal.font_size,
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

        match iced_term::Terminal::new(id, settings) {
            Ok(terminal) => {
                self.tabs.insert(
                    index,
                    Tab {
                        id,
                        label,
                        terminal: Some(terminal),
                        ssh_args,
                        dead: false,
                    },
                );
                self.active_tab = index;
                self.update_title();
                tracing::info!("reconnected tab {id}");
            }
            Err(e) => {
                tracing::error!("reconnect failed: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    fn update_title(&mut self) {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            let status = if tab.dead { " (disconnected)" } else { "" };
            self.title_text = format!("shellkeep — {}{}", tab.label, status);
        } else {
            self.title_text = "shellkeep".to_string();
        }
    }

    fn build_ssh_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        let host = self.host_input.trim();

        // Parse user@host:port from host field
        let (parsed_user, parsed_host, parsed_port) = parse_host_input(host);

        let user = if !self.user_input.is_empty() {
            self.user_input.clone()
        } else {
            parsed_user.unwrap_or_default()
        };

        let host = parsed_host;
        let port = parsed_port.unwrap_or_else(|| self.port_input.trim().to_string());

        if !user.is_empty() {
            args.push(format!("{user}@{host}"));
        } else {
            args.push(host);
        }

        if !port.is_empty() && port != "22" {
            args.push("-p".to_string());
            args.push(port);
        }

        if !self.identity_input.is_empty() {
            args.push("-i".to_string());
            args.push(self.identity_input.clone());
        }

        args
    }

    fn title(&self) -> String {
        self.title_text.clone()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TerminalEvent(iced_term::Event::BackendCall(id, cmd)) => {
                let mut needs_title_update = false;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                    if let Some(ref mut terminal) = tab.terminal {
                        let action = terminal.handle(iced_term::Command::ProxyToBackend(cmd));
                        match action {
                            iced_term::actions::Action::ChangeTitle(new_title) => {
                                tab.label = new_title;
                                needs_title_update = true;
                            }
                            iced_term::actions::Action::Shutdown => {
                                tab.dead = true;
                                tab.terminal = None;
                                needs_title_update = true;
                                tracing::info!("session ended for tab {id}");
                            }
                            _ => {}
                        }
                    }
                }
                if needs_title_update {
                    self.update_title();
                }
            }

            Message::SelectTab(index) => {
                if index < self.tabs.len() {
                    self.active_tab = index;
                    self.show_welcome = false;
                    self.update_title();
                }
            }

            Message::CloseTab(index) => {
                self.close_tab(index);
            }

            Message::NewTab => {
                self.show_welcome = true;
            }

            Message::ReconnectTab(index) => {
                self.reconnect_tab(index);
            }

            Message::StartRenameTab(index) => {
                if index < self.tabs.len() {
                    self.rename_input = self.tabs[index].label.clone();
                    self.renaming_tab = Some(index);
                }
            }

            Message::RenameInputChanged(v) => {
                self.rename_input = v;
            }

            Message::FinishRename => {
                if let Some(index) = self.renaming_tab {
                    if index < self.tabs.len() && !self.rename_input.trim().is_empty() {
                        self.tabs[index].label = self.rename_input.trim().to_string();
                        self.update_title();
                    }
                }
                self.renaming_tab = None;
            }

            Message::CancelRename => {
                self.renaming_tab = None;
            }

            Message::HostInputChanged(v) => {
                self.host_input = v;
            }
            Message::PortInputChanged(v) => self.port_input = v,
            Message::UserInputChanged(v) => self.user_input = v,
            Message::IdentityInputChanged(v) => self.identity_input = v,

            Message::Connect => {
                if self.host_input.trim().is_empty() {
                    return Task::none();
                }
                let ssh_args = self.build_ssh_args();
                let label = ssh_args.join(" ");
                self.recent.push(RecentConnection {
                    label: label.clone(),
                    ssh_args: ssh_args.clone(),
                    host: self.host_input.clone(),
                    user: self.user_input.clone(),
                    port: self.port_input.clone(),
                });
                self.recent.save();
                self.open_tab(&ssh_args, &label);
                self.show_welcome = false;
            }

            Message::ConnectRecent(index) => {
                if let Some(conn) = self.recent.connections.get(index).cloned() {
                    self.host_input = conn.host;
                    self.user_input = conn.user;
                    self.port_input = conn.port;
                    self.open_tab(&conn.ssh_args, &conn.label);
                    self.show_welcome = false;
                }
            }

            Message::KeyEvent(event) => {
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = event {
                    // Ctrl+Shift+T — new tab
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("t".into())
                    {
                        self.show_welcome = true;
                    }
                    // Ctrl+Shift+W — close current tab
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("w".into())
                        && !self.tabs.is_empty()
                    {
                        self.close_tab(self.active_tab);
                    }
                    // Ctrl+Tab — next tab
                    if modifiers.control()
                        && !modifiers.shift()
                        && key == keyboard::Key::Named(keyboard::key::Named::Tab)
                        && !self.tabs.is_empty()
                    {
                        self.active_tab = (self.active_tab + 1) % self.tabs.len();
                        self.show_welcome = false;
                        self.update_title();
                    }
                    // Ctrl+Shift+Tab — previous tab
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Named(keyboard::key::Named::Tab)
                        && !self.tabs.is_empty()
                    {
                        if self.active_tab == 0 {
                            self.active_tab = self.tabs.len() - 1;
                        } else {
                            self.active_tab -= 1;
                        }
                        self.show_welcome = false;
                        self.update_title();
                    }
                    // F2 — rename current tab
                    if key == keyboard::Key::Named(keyboard::key::Named::F2)
                        && !self.tabs.is_empty()
                        && self.renaming_tab.is_none()
                    {
                        self.rename_input = self.tabs[self.active_tab].label.clone();
                        self.renaming_tab = Some(self.active_tab);
                    }
                    // Escape — cancel rename or welcome
                    if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                        if self.renaming_tab.is_some() {
                            self.renaming_tab = None;
                        } else if self.show_welcome && !self.tabs.is_empty() {
                            self.show_welcome = false;
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.tabs.is_empty() {
            return self.view_welcome();
        }

        if self.show_welcome {
            let tab_bar = self.view_tab_bar();
            return column![tab_bar, self.view_welcome()].into();
        }

        let tab_bar = self.view_tab_bar();
        let content: Element<'_, Message> = if let Some(tab) = self.tabs.get(self.active_tab) {
            if tab.dead {
                self.view_dead_tab(tab)
            } else if let Some(ref terminal) = tab.terminal {
                container(iced_term::TerminalView::show(terminal).map(Message::TerminalEvent))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            } else {
                center(text("Terminal not available")).into()
            }
        } else {
            center(text("No active tab")).into()
        };

        let status_bar = self.view_status_bar();

        column![tab_bar, content, status_bar].into()
    }

    fn view_dead_tab<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        let index = self.tabs.iter().position(|t| t.id == tab.id).unwrap_or(0);

        center(
            column![
                text("⚠").size(48),
                text("Session disconnected")
                    .size(20)
                    .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                text(&tab.label)
                    .size(14)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                Space::new().height(16),
                button(
                    text("Reconnect")
                        .size(14)
                        .color(Color::from_rgb8(0x1e, 0x1e, 0x2e))
                )
                .on_press(Message::ReconnectTab(index))
                .padding([10, 24])
                .style(|_theme, _status| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(0xa6, 0xe3, 0xa1,))),
                    text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
                button(text("Close tab").size(12))
                    .on_press(Message::CloseTab(index))
                    .padding([6, 16])
                    .style(|_theme: &Theme, _status| button::Style {
                        background: None,
                        text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                        ..Default::default()
                    }),
            ]
            .spacing(12)
            .align_x(iced::Alignment::Center),
        )
        .into()
    }

    fn view_tab_bar(&self) -> Element<'_, Message> {
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
                // Show inline text input for renaming
                container(
                    text_input("tab name", &self.rename_input)
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

                let label_color = if tab.dead {
                    Color::from_rgb8(0xf3, 0x8b, 0xa8)
                } else {
                    Color::from_rgb8(0xcd, 0xd6, 0xf4)
                };

                let close_btn = button(text("×").size(12))
                    .on_press(Message::CloseTab(i))
                    .padding([0, 4])
                    .style(|_theme: &Theme, _status| button::Style {
                        background: None,
                        text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                        ..Default::default()
                    });

                let tab_content = row![text(label_text).size(12).color(label_color), close_btn]
                    .spacing(6)
                    .align_y(iced::Alignment::Center);

                button(tab_content)
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
                    })
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

    fn view_status_bar(&self) -> Element<'_, Message> {
        let active_count = self.tabs.iter().filter(|t| !t.dead).count();
        let dead_count = self.tabs.iter().filter(|t| t.dead).count();
        let total = self.tabs.len();

        let status_text = if dead_count > 0 {
            format!("{total} tabs ({active_count} active, {dead_count} disconnected)")
        } else {
            format!("{total} tab{}", if total == 1 { "" } else { "s" })
        };

        let active_label = if let Some(tab) = self.tabs.get(self.active_tab) {
            tab.label.clone()
        } else {
            String::new()
        };

        container(
            row![
                text(active_label)
                    .size(11)
                    .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                Space::new().width(Length::Fill),
                text(status_text)
                    .size(11)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86)),
            ]
            .padding([2, 8])
            .width(Length::Fill),
        )
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x18, 0x18, 0x25))),
            ..Default::default()
        })
        .into()
    }

    fn view_welcome(&self) -> Element<'_, Message> {
        let logo = text("🐚").size(64);
        let title = text("shellkeep")
            .size(28)
            .color(Color::from_rgb8(0x89, 0xb4, 0xfa));

        let subtitle = text("SSH sessions that survive everything")
            .size(14)
            .color(Color::from_rgb8(0xa6, 0xad, 0xc8));

        let host_field = text_input("user@host or just hostname", &self.host_input)
            .on_input(Message::HostInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let user_field = text_input("username", &self.user_input)
            .on_input(Message::UserInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let port_field = text_input("22", &self.port_input)
            .on_input(Message::PortInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10)
            .width(80);

        let identity_field = text_input("~/.ssh/id_ed25519 (optional)", &self.identity_input)
            .on_input(Message::IdentityInputChanged)
            .on_submit(Message::Connect)
            .size(14)
            .padding(10);

        let connect_btn = button(
            text("Connect")
                .size(14)
                .color(Color::from_rgb8(0x1e, 0x1e, 0x2e)),
        )
        .on_press(Message::Connect)
        .padding([10, 24])
        .style(|_theme, _status| button::Style {
            background: Some(iced::Background::Color(Color::from_rgb8(0x89, 0xb4, 0xfa))),
            text_color: Color::from_rgb8(0x1e, 0x1e, 0x2e),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

        let host_row = row![
            column![text("Host").size(12), host_field]
                .spacing(4)
                .width(Length::Fill),
            column![text("Port").size(12), port_field].spacing(4),
        ]
        .spacing(8);

        let user_row = column![text("Username").size(12), user_field].spacing(4);
        let identity_row = column![text("Identity file").size(12), identity_field].spacing(4);

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
                text("Recent connections")
                    .size(12)
                    .color(Color::from_rgb8(0x6c, 0x70, 0x86))
                    .into(),
            );
            for (i, conn) in self.recent.connections.iter().enumerate() {
                let item: Element<'_, Message> = button(
                    text(&conn.label)
                        .size(13)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                )
                .on_press(Message::ConnectRecent(i))
                .padding([6, 12])
                .width(Length::Fill)
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
                    text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                })
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
            user_row,
            identity_row,
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

    fn subscription(&self) -> Subscription<Message> {
        let mut subs: Vec<Subscription<Message>> = Vec::new();

        for tab in &self.tabs {
            if let Some(ref terminal) = tab.terminal {
                subs.push(terminal.subscription().map(Message::TerminalEvent));
            }
        }

        subs.push(keyboard::listen().map(Message::KeyEvent));

        Subscription::batch(subs)
    }

    fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }
}

// ---------------------------------------------------------------------------
// Host input parsing: supports user@host:port, user@host, host:port, host
// ---------------------------------------------------------------------------

fn parse_host_input(input: &str) -> (Option<String>, String, Option<String>) {
    let mut user = None;
    let mut remaining = input.to_string();

    // Extract user@
    if let Some(at_pos) = remaining.find('@') {
        user = Some(remaining[..at_pos].to_string());
        remaining = remaining[at_pos + 1..].to_string();
    }

    // Extract :port (but not IPv6 brackets)
    let port = if remaining.starts_with('[') {
        // IPv6: [::1]:port
        if let Some(bracket_end) = remaining.find(']') {
            let host = remaining[1..bracket_end].to_string();
            let port = if remaining.len() > bracket_end + 2
                && remaining.as_bytes()[bracket_end + 1] == b':'
            {
                Some(remaining[bracket_end + 2..].to_string())
            } else {
                None
            };
            remaining = host;
            port
        } else {
            None
        }
    } else if let Some(colon_pos) = remaining.rfind(':') {
        let maybe_port = &remaining[colon_pos + 1..];
        if maybe_port.parse::<u16>().is_ok() {
            let port = Some(maybe_port.to_string());
            remaining = remaining[..colon_pos].to_string();
            port
        } else {
            None
        }
    } else {
        None
    };

    (user, remaining, port)
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_simple() {
        let (user, host, port) = parse_host_input("example.com");
        assert_eq!(user, None);
        assert_eq!(host, "example.com");
        assert_eq!(port, None);
    }

    #[test]
    fn parse_host_with_user() {
        let (user, host, port) = parse_host_input("alice@example.com");
        assert_eq!(user, Some("alice".into()));
        assert_eq!(host, "example.com");
        assert_eq!(port, None);
    }

    #[test]
    fn parse_host_with_port() {
        let (user, host, port) = parse_host_input("example.com:2222");
        assert_eq!(user, None);
        assert_eq!(host, "example.com");
        assert_eq!(port, Some("2222".into()));
    }

    #[test]
    fn parse_host_full() {
        let (user, host, port) = parse_host_input("alice@example.com:2222");
        assert_eq!(user, Some("alice".into()));
        assert_eq!(host, "example.com");
        assert_eq!(port, Some("2222".into()));
    }

    #[test]
    fn parse_host_ipv6() {
        let (user, host, port) = parse_host_input("[::1]:2222");
        assert_eq!(user, None);
        assert_eq!(host, "::1");
        assert_eq!(port, Some("2222".into()));
    }
}
