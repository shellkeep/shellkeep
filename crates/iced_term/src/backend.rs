// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: MIT

use crate::actions::Action;
use crate::settings::BackendSettings;
use alacritty_terminal::event::{
    Event, EventListener, Notify, OnResize, WindowSize,
};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::{
    self, cell::Cell, test::TermSize, viewport_to_point, Term, TermMode,
};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};
use alacritty_terminal::{tty, Grid};
use iced::keyboard::Modifiers;
use iced_core::Size;
use std::borrow::Cow;
use std::cmp::min;
use std::io::Result;
use std::ops::{Index, RangeInclusive};
use std::sync::Arc;
use tokio::sync::mpsc;

const URL_REGEX: &str = r#"(ipfs:|ipns:|magnet:|mailto:|gemini://|gopher://|https://|http://|news:|file://|git://|ssh:|ftp://)[^\u{0000}-\u{001F}\u{007F}-\u{009F}<>"\s{-}\^⟨⟩`]+"#;

#[derive(Debug, Clone)]
pub enum Command {
    Write(Vec<u8>),
    Scroll(i32),
    Resize(Option<Size<f32>>, Option<Size<f32>>),
    SelectStart(SelectionType, (f32, f32)),
    SelectUpdate((f32, f32)),
    ProcessLink(LinkAction, Point),
    MouseReport(MouseButton, Modifiers, Point, bool),
    ProcessAlacrittyEvent(Event),
}

#[derive(Debug, Clone)]
pub enum MouseMode {
    Sgr,
    Normal(bool),
}

impl From<TermMode> for MouseMode {
    fn from(term_mode: TermMode) -> Self {
        if term_mode.contains(TermMode::SGR_MOUSE) {
            MouseMode::Sgr
        } else if term_mode.contains(TermMode::UTF8_MOUSE) {
            MouseMode::Normal(true)
        } else {
            MouseMode::Normal(false)
        }
    }
}

#[derive(Debug, Clone)]
pub enum MouseButton {
    LeftButton = 0,
    MiddleButton = 1,
    RightButton = 2,
    LeftMove = 32,
    MiddleMove = 33,
    RightMove = 34,
    NoneMove = 35,
    ScrollUp = 64,
    ScrollDown = 65,
    Other = 99,
}

#[derive(Debug, Clone)]
pub enum LinkAction {
    Clear,
    Hover,
    Open,
}

#[derive(Clone, Copy, Debug)]
pub struct TerminalSize {
    pub cell_width: u16,
    pub cell_height: u16,
    num_cols: u16,
    num_lines: u16,
    layout_width: f32,
    layout_height: f32,
}

impl Default for TerminalSize {
    fn default() -> Self {
        // Use reasonable default cell dimensions to avoid tiny initial terminal.
        // These will be corrected once the widget measures its actual font.
        Self {
            cell_width: 9,
            cell_height: 18,
            num_cols: 100,
            num_lines: 30,
            layout_width: 900.0,
            layout_height: 540.0,
        }
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn columns(&self) -> usize {
        self.num_cols as usize
    }

    fn last_column(&self) -> Column {
        Column(self.num_cols as usize - 1)
    }

    fn bottommost_line(&self) -> Line {
        Line(self.num_lines as i32 - 1)
    }

    fn screen_lines(&self) -> usize {
        self.num_lines as usize
    }
}

impl From<TerminalSize> for WindowSize {
    fn from(size: TerminalSize) -> Self {
        Self {
            num_lines: size.num_lines,
            num_cols: size.num_cols,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
        }
    }
}

pub struct Backend {
    term: Arc<FairMutex<Term<EventProxy>>>,
    size: TerminalSize,
    notifier: Option<Notifier>,
    ssh_writer: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    last_content: RenderableContent,
    pub(crate) url_regex: RegexSearch,
}

/// Parse a cursor shape string into alacritty's CursorShape.
fn parse_cursor_shape(shape: &str) -> CursorShape {
    match shape.to_lowercase().as_str() {
        "ibeam" | "bar" | "beam" => CursorShape::Beam,
        "underline" => CursorShape::Underline,
        _ => CursorShape::Block,
    }
}

impl Backend {
    pub fn new(
        id: u64,
        pty_event_proxy_sender: mpsc::Sender<Event>,
        settings: BackendSettings,
    ) -> Result<Self> {
        let pty_config = tty::Options {
            shell: Some(tty::Shell::new(settings.program, settings.args)),
            working_directory: settings.working_directory,
            env: settings.env,
            ..tty::Options::default()
        };

        let mut config = term::Config::default();
        config.default_cursor_style = CursorStyle {
            shape: parse_cursor_shape(&settings.cursor_shape),
            blinking: false,
        };
        let terminal_size = TerminalSize::default();
        let pty = tty::new(&pty_config, terminal_size.into(), id)?;

        let event_proxy = EventProxy(pty_event_proxy_sender);

        let mut term = Term::new(config, &terminal_size, event_proxy.clone());

        let cursor = term.grid_mut().cursor_cell().clone();

        let cursor_shape = term.cursor_style().shape;
        let initial_content = RenderableContent {
            grid: term.grid().clone(),
            selectable_range: None,
            terminal_mode: *term.mode(),
            terminal_size,
            cursor: cursor.clone(),
            cursor_shape,
            hovered_hyperlink: None,
        };

        let term = Arc::new(FairMutex::new(term));

        let pty_event_loop =
            EventLoop::new(term.clone(), event_proxy, pty, false, false)?;

        let notifier = Notifier(pty_event_loop.channel());

        let _ = pty_event_loop.spawn();

        Ok(Self {
            term: term.clone(),
            size: terminal_size,
            notifier: Some(notifier),
            ssh_writer: None,
            last_content: initial_content,
            url_regex: RegexSearch::new(URL_REGEX).expect("invalid url regexp"),
        })
    }

    /// Create a backend connected to an SSH channel instead of a local PTY.
    /// The `ssh_writer` sends keyboard input to the SSH channel.
    /// The `ssh_reader` feeds SSH output into the terminal — call `feed_ssh_data()`.
    pub fn new_ssh(
        _id: u64,
        pty_event_proxy_sender: mpsc::Sender<Event>,
        ssh_writer: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    ) -> Result<Self> {
        let config = term::Config::default();
        let terminal_size = TerminalSize::default();
        let event_proxy = EventProxy(pty_event_proxy_sender);

        let mut term = Term::new(config, &terminal_size, event_proxy);
        let cursor = term.grid_mut().cursor_cell().clone();
        let cursor_shape = term.cursor_style().shape;

        let initial_content = RenderableContent {
            grid: term.grid().clone(),
            selectable_range: None,
            terminal_mode: *term.mode(),
            terminal_size,
            cursor,
            cursor_shape,
            hovered_hyperlink: None,
        };

        let term = Arc::new(FairMutex::new(term));

        Ok(Self {
            term,
            size: terminal_size,
            notifier: None,
            ssh_writer: Some(ssh_writer),
            last_content: initial_content,
            url_regex: RegexSearch::new(URL_REGEX).expect("invalid url regexp"),
        })
    }

    /// Feed raw bytes from SSH channel into the terminal parser.
    pub fn feed_ssh_data(&self, data: &[u8]) {
        let mut term = self.term.lock();
        let mut parser: alacritty_terminal::vte::ansi::Processor = Default::default();
        parser.advance(&mut *term, data);
    }

    /// Get the terminal Arc for external access.
    #[allow(dead_code)]
    pub fn term_arc(&self) -> Arc<FairMutex<Term<EventProxy>>> {
        self.term.clone()
    }

    /// Returns the current terminal grid size as (cols, rows).
    pub fn terminal_size(&self) -> (u16, u16) {
        (self.size.num_cols, self.size.num_lines)
    }

    pub fn handle(&mut self, cmd: Command) -> Action {
        let mut action = Action::default();
        let term = self.term.clone();
        let mut term = term.lock();
        match cmd {
            Command::ProcessAlacrittyEvent(event) => {
                match event {
                    Event::Exit => {
                        action = Action::Shutdown;
                    },
                    Event::Title(title) => {
                        action = Action::ChangeTitle(title);
                    },
                    Event::PtyWrite(pty) => {
                        self.write(pty.into_bytes());
                    },
                    _ => {},
                };
            },
            Command::Write(input) => {
                self.write(input);
                term.scroll_display(Scroll::Bottom);
            },
            Command::Scroll(delta) => {
                self.scroll(&mut term, delta);
            },
            Command::Resize(layout_size, font_measure) => {
                self.resize(&mut term, layout_size, font_measure);
            },
            Command::SelectStart(selection_type, (x, y)) => {
                self.start_selection(&mut term, selection_type, x, y);
            },
            Command::SelectUpdate((x, y)) => {
                self.update_selection(&mut term, x, y);
            },
            Command::ProcessLink(link_action, point) => {
                self.process_link_action(&term, link_action, point);
            },
            Command::MouseReport(button, modifiers, point, pressed) => {
                self.process_mouse_report(button, modifiers, point, pressed);
            },
        };

        action
    }

    fn process_link_action(
        &mut self,
        terminal: &Term<EventProxy>,
        link_action: LinkAction,
        point: Point,
    ) {
        match link_action {
            LinkAction::Hover => {
                self.last_content.hovered_hyperlink = self.regex_match_at(
                    terminal,
                    point,
                    &mut self.url_regex.clone(),
                );
            },
            LinkAction::Clear => {
                self.last_content.hovered_hyperlink = None;
            },
            LinkAction::Open => {
                self.open_link();
            },
        };
    }

    fn open_link(&self) {
        if let Some(range) = &self.last_content.hovered_hyperlink {
            let start = range.start();
            let end = range.end();

            let mut url = String::from(self.last_content.grid.index(*start).c);
            for indexed in self.last_content.grid.iter_from(*start) {
                url.push(indexed.c);
                if indexed.point == *end {
                    break;
                }
            }

            if let Err(e) = open::that(&url) {
                eprintln!("iced_term: failed to open link: {e}");
            }
        }
    }

    fn process_mouse_report(
        &self,
        button: MouseButton,
        modifiers: Modifiers,
        point: Point,
        pressed: bool,
    ) {
        let mut mods = 0;
        if modifiers.contains(Modifiers::SHIFT) {
            mods += 4;
        }
        if modifiers.contains(Modifiers::ALT) {
            mods += 8;
        }
        if modifiers.contains(Modifiers::COMMAND) {
            mods += 16;
        }

        match MouseMode::from(self.last_content.terminal_mode) {
            MouseMode::Sgr => {
                self.sgr_mouse_report(point, button as u8 + mods, pressed)
            },
            MouseMode::Normal(is_utf8) => {
                if pressed {
                    self.normal_mouse_report(
                        point,
                        button as u8 + mods,
                        is_utf8,
                    )
                } else {
                    self.normal_mouse_report(point, 3 + mods, is_utf8)
                }
            },
        }
    }

    fn sgr_mouse_report(&self, point: Point, button: u8, pressed: bool) {
        let c = if pressed { 'M' } else { 'm' };

        let msg = format!(
            "\x1b[<{};{};{}{}",
            button,
            point.column + 1,
            point.line + 1,
            c
        );

        self.write(msg.as_bytes().to_vec());
    }

    fn normal_mouse_report(&self, point: Point, button: u8, is_utf8: bool) {
        let Point { line, column } = point;
        let max_point = if is_utf8 { 2015 } else { 223 };

        if line >= max_point || column >= max_point {
            return;
        }

        let mut msg = vec![b'\x1b', b'[', b'M', 32 + button];

        let mouse_pos_encode = |pos: usize| -> Vec<u8> {
            let pos = 32 + 1 + pos;
            let first = 0xC0 + pos / 64;
            let second = 0x80 + (pos & 63);
            vec![first as u8, second as u8]
        };

        if is_utf8 && column >= Column(95) {
            msg.append(&mut mouse_pos_encode(column.0));
        } else {
            msg.push(32 + 1 + column.0 as u8);
        }

        if is_utf8 && line >= 95 {
            msg.append(&mut mouse_pos_encode(line.0 as usize));
        } else {
            msg.push(32 + 1 + line.0 as u8);
        }

        self.write(msg);
    }

    fn start_selection(
        &mut self,
        terminal: &mut Term<EventProxy>,
        selection_type: SelectionType,
        x: f32,
        y: f32,
    ) {
        let location = Self::selection_point(
            x,
            y,
            &self.size,
            terminal.grid().display_offset(),
        );
        terminal.selection = Some(Selection::new(
            selection_type,
            location,
            self.selection_side(x),
        ));
    }

    fn update_selection(
        &mut self,
        terminal: &mut Term<EventProxy>,
        x: f32,
        y: f32,
    ) {
        let display_offset = terminal.grid().display_offset();
        if let Some(ref mut selection) = terminal.selection {
            let location =
                Self::selection_point(x, y, &self.size, display_offset);
            selection.update(location, self.selection_side(x));
        }
    }

    pub fn selection_point(
        x: f32,
        y: f32,
        terminal_size: &TerminalSize,
        display_offset: usize,
    ) -> Point {
        let col = (x as usize) / (terminal_size.cell_width as usize);
        let col = min(Column(col), Column(terminal_size.num_cols as usize - 1));

        let line = (y as usize) / (terminal_size.cell_height as usize);
        let line = min(line, terminal_size.num_lines as usize - 1);

        viewport_to_point(display_offset, Point::new(line, col))
    }

    fn selection_side(&self, x: f32) -> Side {
        let cell_x = x as usize % self.size.cell_width as usize;
        let half_cell_width = (self.size.cell_width as f32 / 2.0) as usize;

        if cell_x > half_cell_width {
            Side::Right
        } else {
            Side::Left
        }
    }

    fn resize(
        &mut self,
        terminal: &mut Term<EventProxy>,
        layout_size: Option<Size<f32>>,
        font_measure: Option<Size<f32>>,
    ) {
        if let Some(size) = layout_size {
            self.size.layout_height = size.height;
            self.size.layout_width = size.width;
        };

        if let Some(size) = font_measure {
            self.size.cell_height = size.height as u16;
            self.size.cell_width = size.width as u16;
        }

        let lines = (self.size.layout_height / self.size.cell_height as f32)
            .floor() as u16;
        let cols = (self.size.layout_width / self.size.cell_width as f32)
            .floor() as u16;
        if lines > 0 && cols > 0 {
            self.size.num_lines = lines;
            self.size.num_cols = cols;
            if let Some(ref mut notifier) = self.notifier {
                notifier.on_resize(self.size.into());
            }
            // For SSH, resize is handled externally via channel.window_change()
            terminal.resize(TermSize::new(
                self.size.num_cols as usize,
                self.size.num_lines as usize,
            ));
        }
    }

    fn write<I: Into<Cow<'static, [u8]>>>(&self, input: I) {
        let data = input.into();
        if let Some(ref ssh_writer) = self.ssh_writer {
            let _ = ssh_writer.send(data.to_vec());
        } else if let Some(ref notifier) = self.notifier {
            notifier.notify(data);
        }
    }

    fn scroll(&mut self, terminal: &mut Term<EventProxy>, delta_value: i32) {
        if delta_value != 0 {
            let scroll = Scroll::Delta(delta_value);
            if terminal
                .mode()
                .contains(TermMode::ALTERNATE_SCROLL | TermMode::ALT_SCREEN)
            {
                let line_cmd = if delta_value > 0 { b'A' } else { b'B' };
                let mut content = vec![];

                for _ in 0..delta_value.abs() {
                    content.push(0x1b);
                    content.push(b'O');
                    content.push(line_cmd);
                }

                self.write(content);
            } else {
                terminal.grid_mut().scroll_display(scroll);
            }
        }
    }

    /// FR-TERMINAL-18 / FR-TABS-12: Extract all scrollback + visible text.
    pub fn scrollback_text(&self) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let total = grid.total_lines();
        let cols = grid.columns();
        let mut result = String::new();
        for line_idx in 0..total {
            // Grid lines: topmost scrollback is -(total-screen_lines), bottommost visible is screen_lines-1.
            let line = Line(line_idx as i32 - (total as i32 - grid.screen_lines() as i32));
            let mut row_text = String::new();
            for col in 0..cols {
                let cell = &grid[line][Column(col)];
                row_text.push(cell.c);
            }
            // Trim trailing spaces per line
            let trimmed = row_text.trim_end();
            result.push_str(trimmed);
            result.push('\n');
        }
        result
    }

    pub fn selectable_content(&self) -> String {
        let content = self.renderable_content();
        let mut result = String::new();
        if let Some(range) = content.selectable_range {
            for indexed in content.grid.display_iter() {
                if range.contains(indexed.point) {
                    result.push(indexed.c);
                }
            }
        }
        result
    }

    pub fn sync(&mut self) {
        let term = self.term.clone();
        let mut term = term.lock();
        self.internal_sync(&mut term);
    }

    fn internal_sync(&mut self, terminal: &mut Term<EventProxy>) {
        let selectable_range = match &terminal.selection {
            Some(s) => s.to_range(terminal),
            None => None,
        };

        let cursor = terminal.grid_mut().cursor_cell().clone();
        self.last_content.grid = terminal.grid().clone();
        self.last_content.selectable_range = selectable_range;
        self.last_content.cursor = cursor.clone();
        self.last_content.cursor_shape = terminal.cursor_style().shape;
        self.last_content.terminal_mode = *terminal.mode();
        self.last_content.terminal_size = self.size;
    }

    pub fn renderable_content(&self) -> &RenderableContent {
        &self.last_content
    }

    /// Search forward (right/down) from the given origin for the pattern.
    /// Returns the match range if found, and scrolls the viewport to show it.
    pub fn search_next(&mut self, regex: &mut RegexSearch, origin: Point) -> Option<Match> {
        let term = self.term.clone();
        let mut term = term.lock();
        let result = term.search_next(regex, origin, Direction::Right, Side::Left, None);
        if let Some(ref m) = result {
            self.scroll_to_match(&mut term, m);
        }
        result
    }

    /// Search backward (left/up) from the given origin for the pattern.
    /// Returns the match range if found, and scrolls the viewport to show it.
    pub fn search_prev(&mut self, regex: &mut RegexSearch, origin: Point) -> Option<Match> {
        let term = self.term.clone();
        let mut term = term.lock();
        let result = term.search_next(regex, origin, Direction::Left, Side::Right, None);
        if let Some(ref m) = result {
            self.scroll_to_match(&mut term, m);
        }
        result
    }

    /// Scroll the viewport so that the match start is visible.
    fn scroll_to_match(&mut self, term: &mut Term<EventProxy>, m: &Match) {
        let match_line = m.start().line;
        let display_offset = term.grid().display_offset() as i32;
        let screen_lines = self.size.num_lines as i32;
        // match_line is in grid coordinates (negative = scrollback).
        // Visible lines range from -display_offset to -display_offset + screen_lines - 1.
        let viewport_top = -(display_offset as i32);
        let viewport_bottom = viewport_top + screen_lines - 1;
        if match_line.0 < viewport_top || match_line.0 > viewport_bottom {
            // Scroll so match is near the top of viewport
            let new_offset = -match_line.0;
            let delta = new_offset - display_offset;
            if delta != 0 {
                term.grid_mut().scroll_display(Scroll::Delta(delta));
            }
        }
    }

    /// Based on alacritty/src/display/hint.rs > regex_match_at
    /// Retrieve the match, if the specified point is inside the content matching the regex.
    fn regex_match_at(
        &self,
        terminal: &Term<EventProxy>,
        point: Point,
        regex: &mut RegexSearch,
    ) -> Option<Match> {
        let x = visible_regex_match_iter(terminal, regex)
            .find(|rm| rm.contains(&point));
        x
    }
}

/// Copied from alacritty/src/display/hint.rs:
/// Iterate over all visible regex matches.
fn visible_regex_match_iter<'a>(
    term: &'a Term<EventProxy>,
    regex: &'a mut RegexSearch,
) -> impl Iterator<Item = Match> + 'a {
    let viewport_start = Line(-(term.grid().display_offset() as i32));
    let viewport_end = viewport_start + term.bottommost_line();
    let mut start =
        term.line_search_left(Point::new(viewport_start, Column(0)));
    let mut end = term.line_search_right(Point::new(viewport_end, Column(0)));
    start.line = start.line.max(viewport_start - 100);
    end.line = end.line.min(viewport_end + 100);

    RegexIter::new(start, end, Direction::Right, term, regex)
        .skip_while(move |rm| rm.end().line < viewport_start)
        .take_while(move |rm| rm.start().line <= viewport_end)
}

pub struct RenderableContent {
    pub grid: Grid<Cell>,
    pub hovered_hyperlink: Option<RangeInclusive<Point>>,
    pub selectable_range: Option<SelectionRange>,
    pub cursor: Cell,
    pub cursor_shape: CursorShape,
    pub terminal_mode: TermMode,
    pub terminal_size: TerminalSize,
}

impl Default for RenderableContent {
    fn default() -> Self {
        Self {
            grid: Grid::new(0, 0, 0),
            hovered_hyperlink: None,
            selectable_range: None,
            cursor: Cell::default(),
            cursor_shape: CursorShape::Block,
            terminal_mode: TermMode::empty(),
            terminal_size: TerminalSize::default(),
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(ref notifier) = self.notifier {
            let _ = notifier.0.send(Msg::Shutdown);
        }
    }
}

#[derive(Clone)]
pub struct EventProxy(mpsc::Sender<Event>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.0.blocking_send(event);
    }
}
