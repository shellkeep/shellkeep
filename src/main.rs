// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! shellkeep — SSH terminal manager.
//!
//! Persistent sessions that survive everything.
//! Open source. Cross-platform. Zero server setup.

mod theme;

use iced::keyboard;
use iced::widget::{
    Space, button, center, column, container, mouse_area, row, scrollable, stack, text, text_input,
};
use iced::{Color, Element, Length, Subscription, Task, Theme};
use iced_term::ColorPalette;
use iced_term::settings::{BackendSettings, FontSettings, Settings, ThemeSettings};
use shellkeep::config::Config;
use shellkeep::ssh;
use shellkeep::state::recent::{RecentConnection, RecentConnections};

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
    tmux_session: String,
    dead: bool,
    reconnect_attempts: u32,
    auto_reconnect: bool,
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
    current_font_size: f32,
    context_menu: Option<(f32, f32)>, // (x, y) position of context menu

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
    AutoReconnectTick,
    ContextMenuCopy,
    ContextMenuPaste,
    ContextMenuDismiss,
    ConnectRecent(usize),
    RenameInputChanged(String),
    FinishRename,
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
            current_font_size: config.terminal.font_size,
            context_menu: None,
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
            // Use just the host part as label (first non-flag arg)
            let label = ssh_args
                .iter()
                .find(|a| !a.starts_with('-'))
                .cloned()
                .unwrap_or_else(|| ssh_args.join(" "));
            app.open_tab(&ssh_args, &label);
        }

        (app, Task::none())
    }

    fn open_tab(&mut self, ssh_args: &[String], label: &str) {
        let tmux_session = format!("shellkeep-{}", self.next_id);
        self.open_tab_with_tmux(ssh_args, label, &tmux_session);
    }

    fn open_tab_with_tmux(&mut self, ssh_args: &[String], label: &str, tmux_session: &str) {
        let id = self.next_id;
        self.next_id += 1;

        let tmux_cmd = format!(
            "TERM=xterm-256color tmux new-session -A -s {tmux_session} \\; set status off || exec $SHELL"
        );

        let mut full_args = Vec::new();
        full_args.extend_from_slice(ssh_args);
        full_args.push("-t".to_string());
        full_args.push(tmux_cmd);

        let settings = Settings {
            font: FontSettings {
                size: self.config.terminal.font_size,
                ..FontSettings::default()
            },
            theme: ThemeSettings {
                color_pallete: Box::new(catppuccin_mocha()),
            },
            backend: BackendSettings {
                program: "ssh".to_string(),
                args: full_args,
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
                    tmux_session: tmux_session.to_string(),
                    dead: false,
                    reconnect_attempts: 0,
                    auto_reconnect: true,
                });
                self.active_tab = self.tabs.len() - 1;
                self.error = None;
                self.update_title();
                tracing::info!("opened tab {id}: {label} (tmux: {tmux_session})");
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
        let tmux_session = self.tabs[index].tmux_session.clone();

        // Remove old dead tab, open fresh one at same position
        self.tabs.remove(index);
        self.open_tab_with_tmux(&ssh_args, &label, &tmux_session);

        // Move the newly added tab (at end) to the original position
        if self.tabs.len() > 1 && index < self.tabs.len() - 1 {
            let tab = self.tabs.pop().unwrap();
            self.tabs.insert(index, tab);
            self.active_tab = index;
            self.update_title();
        }
    }

    fn apply_font_to_all_tabs(&mut self) {
        let font_settings = FontSettings {
            size: self.current_font_size,
            ..FontSettings::default()
        };
        for tab in &mut self.tabs {
            if let Some(ref mut terminal) = tab.terminal {
                terminal.handle(iced_term::Command::ChangeFont(font_settings.clone()));
            }
        }
        tracing::debug!("font size: {}", self.current_font_size);
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
            Message::TerminalEvent(iced_term::Event::ContextMenu(_id, x, y)) => {
                self.context_menu = Some((x, y));
            }

            Message::ContextMenuCopy => {
                self.context_menu = None;
                // Copy is handled by iced_term's Ctrl+Shift+C binding
                // We trigger it via the backend
                if let Some(tab) = self.tabs.get_mut(self.active_tab)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    terminal.handle(iced_term::Command::ProxyToBackend(
                        iced_term::BackendCommand::ProcessAlacrittyEvent(
                            iced_term::AlacrittyEvent::PtyWrite(String::new()),
                        ),
                    ));
                }
            }

            Message::ContextMenuPaste => {
                self.context_menu = None;
                // Paste is handled at the OS level — we just dismiss the menu
            }

            Message::ContextMenuDismiss => {
                self.context_menu = None;
            }

            Message::TerminalEvent(iced_term::Event::BackendCall(id, cmd)) => {
                let mut needs_title_update = false;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
                    && let Some(ref mut terminal) = tab.terminal
                {
                    let action = terminal.handle(iced_term::Command::ProxyToBackend(cmd));
                    match action {
                        iced_term::actions::Action::ChangeTitle(new_title) => {
                            tab.label = new_title;
                            needs_title_update = true;
                        }
                        iced_term::actions::Action::Shutdown => {
                            tab.terminal = None;
                            if tab.auto_reconnect
                                && tab.reconnect_attempts < self.config.ssh.reconnect_max_attempts
                            {
                                tab.reconnect_attempts += 1;
                                tracing::info!(
                                    "tab {id} disconnected, will auto-reconnect (attempt {})",
                                    tab.reconnect_attempts
                                );
                                // Don't mark as dead yet — auto-reconnect timer will handle it
                            } else {
                                tab.dead = true;
                                tab.auto_reconnect = false;
                                tracing::info!("tab {id} session ended (no more retries)");
                            }
                            needs_title_update = true;
                        }
                        _ => {}
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
                // Open new session to the same server as the current/last tab
                if let Some(tab) = self.tabs.last() {
                    let ssh_args = tab.ssh_args.clone();
                    let n = self.tabs.len() + 1;
                    let label = format!("Session {n}");
                    self.open_tab(&ssh_args, &label);
                } else {
                    self.show_welcome = true;
                }
            }

            Message::ReconnectTab(index) => {
                if index < self.tabs.len() {
                    self.tabs[index].auto_reconnect = false; // manual reconnect resets state
                    self.tabs[index].reconnect_attempts = 0;
                }
                self.reconnect_tab(index);
            }

            Message::AutoReconnectTick => {
                // Find tabs that need auto-reconnection
                let reconnect_indices: Vec<usize> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.terminal.is_none() && t.auto_reconnect && !t.dead)
                    .map(|(i, _)| i)
                    .collect();

                if let Some(&index) = reconnect_indices.first() {
                    tracing::info!(
                        "auto-reconnecting tab {} (attempt {})",
                        self.tabs[index].id,
                        self.tabs[index].reconnect_attempts,
                    );
                    self.reconnect_tab(index);
                }
            }

            Message::RenameInputChanged(v) => {
                self.rename_input = v;
            }

            Message::FinishRename => {
                if let Some(index) = self.renaming_tab
                    && index < self.tabs.len()
                    && !self.rename_input.trim().is_empty()
                {
                    self.tabs[index].label = self.rename_input.trim().to_string();
                    self.update_title();
                }
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
                let label = ssh_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ssh_args.join(" "));
                self.recent.push(RecentConnection {
                    label: label.clone(),
                    ssh_args: ssh_args.clone(),
                    host: self.host_input.clone(),
                    user: self.user_input.clone(),
                    port: self.port_input.clone(),
                });
                self.recent.save();

                // Check for existing shellkeep tmux sessions on the server
                let existing = ssh::tmux::list_remote_sessions(&ssh_args);
                if existing.is_empty() {
                    // No existing sessions — open one new tab
                    self.open_tab(&ssh_args, &label);
                } else {
                    // Restore all existing sessions as tabs
                    tracing::info!(
                        "found {} existing tmux session(s): {:?}",
                        existing.len(),
                        existing
                    );
                    for (i, session_name) in existing.iter().enumerate() {
                        let tab_label = if i == 0 {
                            label.clone()
                        } else {
                            format!("Session {}", i + 1)
                        };
                        self.open_tab_with_tmux(&ssh_args, &tab_label, session_name);
                    }
                }
                self.show_welcome = false;
            }

            Message::ConnectRecent(index) => {
                if let Some(conn) = self.recent.connections.get(index).cloned() {
                    self.host_input = conn.host;
                    self.user_input = conn.user;
                    self.port_input = conn.port;

                    // Check for existing sessions
                    let existing = ssh::tmux::list_remote_sessions(&conn.ssh_args);
                    if existing.is_empty() {
                        self.open_tab(&conn.ssh_args, &conn.label);
                    } else {
                        for (i, session_name) in existing.iter().enumerate() {
                            let tab_label = if i == 0 {
                                conn.label.clone()
                            } else {
                                format!("Session {}", i + 1)
                            };
                            self.open_tab_with_tmux(&conn.ssh_args, &tab_label, session_name);
                        }
                    }
                    self.show_welcome = false;
                }
            }

            Message::KeyEvent(event) => {
                if let keyboard::Event::KeyPressed { key, modifiers, .. } = event {
                    // Ctrl+Shift+T — new tab (same server)
                    if modifiers.control()
                        && modifiers.shift()
                        && key == keyboard::Key::Character("t".into())
                    {
                        if let Some(tab) = self.tabs.last() {
                            let ssh_args = tab.ssh_args.clone();
                            let n = self.tabs.len() + 1;
                            let label = format!("Session {n}");
                            self.open_tab(&ssh_args, &label);
                        } else {
                            self.show_welcome = true;
                        }
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
                    // Ctrl+Shift+= or Ctrl+= — zoom in
                    if modifiers.control()
                        && (key == keyboard::Key::Character("=".into())
                            || key == keyboard::Key::Character("+".into()))
                    {
                        self.current_font_size = (self.current_font_size + 1.0).min(36.0);
                        self.apply_font_to_all_tabs();
                    }
                    // Ctrl+- — zoom out
                    if modifiers.control() && key == keyboard::Key::Character("-".into()) {
                        self.current_font_size = (self.current_font_size - 1.0).max(8.0);
                        self.apply_font_to_all_tabs();
                    }
                    // Ctrl+0 — zoom reset
                    if modifiers.control() && key == keyboard::Key::Character("0".into()) {
                        self.current_font_size = self.config.terminal.font_size;
                        self.apply_font_to_all_tabs();
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
            } else if tab.auto_reconnect {
                // Reconnecting state
                let attempt_text = format!(
                    "Reconnecting... (attempt {}/{})",
                    tab.reconnect_attempts, self.config.ssh.reconnect_max_attempts
                );
                center(
                    column![
                        text("🔄").size(48),
                        text("Connection lost")
                            .size(20)
                            .color(Color::from_rgb8(0xf9, 0xe2, 0xaf)),
                        text(attempt_text)
                            .size(14)
                            .color(Color::from_rgb8(0xa6, 0xad, 0xc8)),
                    ]
                    .spacing(12)
                    .align_x(iced::Alignment::Center),
                )
                .into()
            } else {
                center(text("Terminal not available")).into()
            }
        } else {
            center(text("No active tab")).into()
        };

        let status_bar = self.view_status_bar();

        // Wrap with context menu overlay if active
        let main_view: Element<'_, Message> = if let Some((x, y)) = self.context_menu {
            let menu = container(
                column![
                    button(text("Copy").size(13))
                        .on_press(Message::ContextMenuCopy)
                        .padding([6, 16])
                        .width(Length::Fill)
                        .style(|_theme: &Theme, _status| button::Style {
                            background: None,
                            text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                            ..Default::default()
                        }),
                    button(text("Paste").size(13))
                        .on_press(Message::ContextMenuPaste)
                        .padding([6, 16])
                        .width(Length::Fill)
                        .style(|_theme: &Theme, _status| button::Style {
                            background: None,
                            text_color: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                            ..Default::default()
                        }),
                ]
                .spacing(2),
            )
            .padding(4)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Background::Color(Color::from_rgb8(0x31, 0x32, 0x44))),
                border: iced::Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: Color::from_rgb8(0x45, 0x47, 0x5a),
                },
                ..Default::default()
            });

            // Use a stack to overlay the menu at the right position
            let dismiss_area = mouse_area(
                container(Space::new().width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .on_press(Message::ContextMenuDismiss);

            stack![
                column![tab_bar, content, status_bar],
                dismiss_area,
                container(menu)
                    .padding(iced::Padding {
                        top: y,
                        right: 0.0,
                        bottom: 0.0,
                        left: x,
                    })
                    .width(Length::Shrink)
                    .height(Length::Shrink),
            ]
            .into()
        } else {
            column![tab_bar, content, status_bar].into()
        };

        main_view
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

                // Status indicator: green=connected, yellow=reconnecting, red=dead
                let (indicator, label_color) = if tab.dead {
                    ("●", Color::from_rgb8(0xf3, 0x8b, 0xa8)) // red
                } else if tab.terminal.is_none() && tab.auto_reconnect {
                    ("●", Color::from_rgb8(0xf9, 0xe2, 0xaf)) // yellow
                } else {
                    ("●", Color::from_rgb8(0xa6, 0xe3, 0xa1)) // green
                };

                let close_btn = button(text("×").size(12))
                    .on_press(Message::CloseTab(i))
                    .padding([0, 4])
                    .style(|_theme: &Theme, _status| button::Style {
                        background: None,
                        text_color: Color::from_rgb8(0x6c, 0x70, 0x86),
                        ..Default::default()
                    });

                let tab_content = row![
                    text(indicator).size(8).color(label_color),
                    text(label_text)
                        .size(12)
                        .color(Color::from_rgb8(0xcd, 0xd6, 0xf4)),
                    close_btn
                ]
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

        let zoom_info = if (self.current_font_size - self.config.terminal.font_size).abs() > 0.1 {
            format!("  {}pt", self.current_font_size)
        } else {
            String::new()
        };

        let status_text = if dead_count > 0 {
            format!("{total} tabs ({active_count} active, {dead_count} disconnected){zoom_info}")
        } else {
            format!(
                "{total} tab{}{zoom_info}",
                if total == 1 { "" } else { "s" }
            )
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

        let version = format!(
            "v{} — SSH sessions that survive everything",
            env!("CARGO_PKG_VERSION")
        );
        let subtitle = text(version)
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

        let shortcuts_hint =
            text("Ctrl+Shift+T new tab  |  Ctrl+Shift+W close  |  F2 rename  |  Ctrl+=/- zoom")
                .size(10)
                .color(Color::from_rgb8(0x58, 0x5b, 0x70));

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
            Space::new().height(20),
            shortcuts_hint,
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

        // Auto-reconnect timer — check every 3 seconds for tabs needing reconnection
        let has_reconnectable = self
            .tabs
            .iter()
            .any(|t| t.terminal.is_none() && t.auto_reconnect && !t.dead);
        if has_reconnectable {
            subs.push(
                iced::time::every(std::time::Duration::from_secs(3))
                    .map(|_| Message::AutoReconnectTick),
            );
        }

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
