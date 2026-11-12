// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! How the user works with a terminal beyond typing: the selection and the selection mode,
//! search, links and their numbers, scrolling, and the mouse. Positions are cells; the front
//! end turns pixels into cells and carries out the [`Effect`]s.

use std::path::{Path, PathBuf};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vi_mode::ViMotion;
use alacritty_terminal::vte::ansi::Rgb;

use crate::colors::{self, resolve};
use crate::input::{self, Key, Modifiers, MouseButton, MouseEvent};
use crate::keytab;
use crate::options::Options;

/// Something the front end does after the view handled an input, in this order.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Bytes the user typed. They return the view to the bottom of the scrollback first.
    Type(Vec<u8>),
    /// Bytes for the program that are no typing, such as mouse reports.
    Write(Vec<u8>),
    OpenLink(String),
    /// Text for the clipboard.
    Copy(String),
    /// Text for the primary selection.
    CopyPrimary(String),
    /// Pastes the primary selection, or the clipboard.
    Paste {
        selection: bool,
    },
    /// Scrolls by lines, up for positive ones, and marks the lines that came in.
    Scroll(i32),
    /// The selection mode began or ended.
    SelectionMode(bool),
    /// The numbers of the links appear or go away.
    Hints(bool),
    /// The cursor, the matches or the scroll position may have changed.
    Refresh,
    /// Only what is painted changed, such as the selection.
    Repaint,
}

/// What the view needs to know about a terminal besides its grid.
pub struct Context<'a> {
    pub options: &'a Options,
    /// Whether the program in the terminal may see the mouse.
    pub tracks_mouse: bool,
    /// The working directory of the program, for links to files. It is only asked when a
    /// link is looked up, since finding it out takes system calls.
    pub directory: &'a dyn Fn() -> Option<PathBuf>,
}

/// A link under the pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub target: String,
    /// The first cell of the link.
    pub start: Point,
    /// How many cells it covers.
    pub cells: usize,
}

/// The state of one terminal's view.
#[derive(Default)]
pub struct View {
    search: Option<RegexSearch>,
    visible_matches: Vec<Match>,
    /// The left button went down on the text, and moving the mouse selects.
    selecting: bool,
    /// The button the program learned about, while it is held.
    pressed_button: Option<MouseButton>,
    /// The cell of the last motion report, so a program learns about each cell once.
    last_motion: Option<Point>,
}

/// Whether the selection mode is on: it uses alacritty's vi mode.
pub fn selection_mode<T>(term: &Term<T>) -> bool {
    term.mode().contains(TermMode::VI)
}

/// Starts or ends the selection mode. Ending it drops the selection.
pub fn toggle_selection_mode<T: EventListener>(term: &mut Term<T>) -> Vec<Effect> {
    term.toggle_vi_mode();
    let active = selection_mode(term);
    if !active {
        term.selection = None;
    }
    vec![Effect::SelectionMode(active), Effect::Repaint]
}

/// True when the program in the terminal wants the mouse. Shift keeps it for selecting.
fn reports_mouse<T>(term: &Term<T>, modifiers: Modifiers, context: &Context) -> bool {
    !modifiers.contains(Modifiers::SHIFT) && context.tracks_mouse && term.mode().intersects(TermMode::MOUSE_MODE)
}

/// The report of a mouse event at a cell of the screen as it is shown.
fn mouse_report<T>(term: &Term<T>, event: MouseEvent, modifiers: Modifiers, point: Point) -> Option<Effect> {
    let line = point.line.0 + term.grid().display_offset() as i32;
    input::mouse_report(*term.mode(), event, modifiers, point.column.0, line.max(0) as usize).map(Effect::Write)
}

/// The link at a cell: a hyperlink the program set, a web address, or with `files` the path
/// of an existing file. Only safe schemes count, and files that would start a program do not.
pub fn link_at<T>(term: &Term<T>, point: Point, files: bool, context: &Context) -> Option<Link> {
    let grid = term.grid();
    let columns = term.columns();
    let cell = &grid[point.line][point.column];
    let hyperlink = cell.hyperlink().filter(|_| context.options.escaped_links);
    if let Some(link) = hyperlink {
        let same = |column: usize| grid[point.line][Column(column)].hyperlink().as_ref() == Some(&link);
        let start = (0..point.column.0)
            .rev()
            .take_while(|column| same(*column))
            .last()
            .unwrap_or(point.column.0);
        let end = (point.column.0..columns)
            .take_while(|column| same(*column))
            .last()
            .unwrap_or(point.column.0);
        return is_safe_link(link.uri(), &context.options.link_schemes).then(|| Link {
            target: link.uri().to_string(),
            start: Point::new(point.line, Column(start)),
            cells: end - start + 1,
        });
    }
    let line: String = (0..columns).map(|column| grid[point.line][Column(column)].c).collect();
    let (start, end, text) = word_span(&line, point.column.0)?;
    let (target, column, cells) = url_in(&text)
        .map(|url| {
            let offset = text.find(url.as_str()).unwrap_or(0);
            let cells = url.chars().count();
            (url, start + offset, cells)
        })
        .or_else(|| {
            let directory = (context.directory)().filter(|_| files)?;
            let path = existing_file(&text, &directory)?;
            Some((path, start, end - start))
        })?;
    Some(Link {
        target,
        start: Point::new(point.line, Column(column)),
        cells,
    })
}

/// Whether the selection covers a cell.
pub fn selection_contains<T>(term: &Term<T>, point: Point) -> bool {
    term.selection
        .as_ref()
        .and_then(|selection| selection.to_range(term))
        .is_some_and(|range| range.contains(point))
}

/// Selects the line of the cursor.
pub fn select_line<T>(term: &mut Term<T>) {
    let point = term.grid().cursor.point;
    term.selection = Some(Selection::new(SelectionType::Lines, point, Side::Left));
}

/// Selects the scrollback and the screen.
pub fn select_all<T>(term: &mut Term<T>) {
    let top = term.grid().topmost_line();
    let bottom = term.grid().bottommost_line();
    let last = term.grid().last_column();
    let mut selection = Selection::new(SelectionType::Simple, Point::new(top, Column(0)), Side::Left);
    selection.update(Point::new(bottom, last), Side::Right);
    term.selection = Some(selection);
}

/// The color named by the word at a cell, such as `#3daee9` or `tomato`.
pub fn color_word_at<T>(term: &Term<T>, point: Point) -> Option<String> {
    let line = &term.grid()[point.line];
    let text: String = (0..term.columns()).map(|column| line[Column(column)].c).collect();
    color_word(&text, point.column.0).map(str::to_string)
}

/// Scrolls by lines, up for positive ones. Returns how far the view moved.
pub fn scroll<T: EventListener>(term: &mut Term<T>, lines: i32) -> i32 {
    let before = term.grid().display_offset() as i32;
    term.scroll_display(Scroll::Delta(lines));
    term.grid().display_offset() as i32 - before
}

impl View {
    /// The matches of the search on the screen, for "Highlight All Matches".
    #[must_use]
    pub fn matches(&self) -> &[Match] {
        &self.visible_matches
    }

    /// Whether moving the mouse selects text now.
    #[must_use]
    pub fn selecting(&self) -> bool {
        self.selecting
    }

    /// A key press that reaches the terminal: link numbers, scrolling, the selection mode,
    /// key binding files, and else what the program receives for the key.
    pub fn key<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        key: Key,
        modifiers: Modifiers,
        text: &str,
        context: &Context,
    ) -> Vec<Effect> {
        let options = context.options;
        let hints_held = !options.url_hints_modifiers.is_empty() && modifiers.held() == options.url_hints_modifiers;
        let mut effects = vec![Effect::Hints(hints_held)];
        if let (true, Key::Character(digit @ '1'..='9')) = (hints_held, key) {
            let links = links_on_screen(term, (context.directory)().as_deref());
            let index = digit as usize - '1' as usize;
            if let Some(link) = pick_hint(&links, index, options.reverse_url_hints) {
                effects.push(Effect::OpenLink(link.0.clone()));
                effects.push(Effect::Hints(false));
                return effects;
            }
        }

        if modifiers.contains(Modifiers::SHIFT) && matches!(key, Key::PageUp | Key::PageDown) {
            let page = scroll_page(term.screen_lines(), options);
            effects.push(Effect::Scroll(if key == Key::PageUp { page } else { -page }));
            return effects;
        }

        if selection_mode(term) {
            match selection_step(term, key, modifiers, text, options) {
                SelectionStep::Moved => effects.push(Effect::Refresh),
                SelectionStep::Leave(copied) => {
                    effects.extend(copied.map(Effect::Copy));
                    effects.extend(toggle_selection_mode(term));
                }
            }
            return effects;
        }

        let mode = *term.mode();
        let modes = keytab::Modes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            app_screen: mode.contains(TermMode::ALT_SCREEN),
            new_line: mode.contains(TermMode::LINE_FEED_NEW_LINE),
            app_keypad: mode.contains(TermMode::APP_KEYPAD),
        };
        let half = term.screen_lines() as i32 / 2;
        match keytab::find(&options.key_rules, key, modifiers, modes) {
            Some(keytab::Action::Text(bytes)) => effects.push(Effect::Type(bytes)),
            Some(keytab::Action::Command(command)) => effects.push(match command {
                keytab::Command::Erase => Effect::Type(vec![0x7f]),
                keytab::Command::PageUp => Effect::Scroll(half),
                keytab::Command::PageDown => Effect::Scroll(-half),
                keytab::Command::LineUp => Effect::Scroll(1),
                keytab::Command::LineDown => Effect::Scroll(-1),
                keytab::Command::Top => Effect::Scroll(i32::MAX / 2),
                keytab::Command::Bottom => Effect::Scroll(i32::MIN / 2),
            }),
            None => {
                if let Some(bytes) = input::key_bytes(mode, key, modifiers, text) {
                    term.selection = None;
                    effects.push(Effect::Type(bytes));
                    effects.push(Effect::Refresh);
                }
            }
        }
        effects
    }

    /// A mouse button went down on a cell. `middle_paste` says what the middle button
    /// pastes: the primary selection, the clipboard, or nothing.
    #[allow(clippy::too_many_arguments, reason = "the parts of one mouse event")]
    pub fn press<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        (point, side): (Point, Side),
        button: Option<MouseButton>,
        clicks: i32,
        modifiers: Modifiers,
        middle_paste: Option<bool>,
        context: &Context,
    ) -> Vec<Effect> {
        let block = modifiers.contains(Modifiers::CONTROL | Modifiers::ALT);
        let opens_link = button == Some(MouseButton::Left) && modifiers.contains(Modifiers::CONTROL) && !block;
        if let Some(pressed) = button.filter(|_| reports_mouse(term, modifiers, context) && !opens_link) {
            self.pressed_button = Some(pressed);
            return mouse_report(term, MouseEvent::Press(pressed), modifiers, point)
                .into_iter()
                .collect();
        }
        match button {
            Some(MouseButton::Left) if opens_link => link_at(term, point, true, context)
                .map(|link| Effect::OpenLink(link.target))
                .into_iter()
                .collect(),
            Some(MouseButton::Left) => {
                let kind = match clicks {
                    2 => SelectionType::Semantic,
                    3 => SelectionType::Lines,
                    _ if block => SelectionType::Block,
                    _ => SelectionType::Simple,
                };
                let extend = modifiers.contains(Modifiers::SHIFT) && term.selection.is_some();
                if extend {
                    if let Some(selection) = term.selection.as_mut() {
                        selection.update(point, side);
                    }
                } else {
                    term.selection = Some(Selection::new(kind, point, side));
                }
                if clicks == 3 && !extend && context.options.triple_click_forward {
                    // From the click to the end of the line instead of the whole line.
                    let end = Point::new(point.line, Column(term.columns() - 1));
                    let mut selection = Selection::new(SelectionType::Simple, point, Side::Left);
                    selection.update(end, Side::Right);
                    term.selection = Some(selection);
                }
                self.selecting = true;
                vec![Effect::Repaint]
            }
            Some(MouseButton::Middle) => middle_paste
                .map(|selection| Effect::Paste { selection })
                .into_iter()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The mouse moved over a cell: it selects, or the program learns about it.
    pub fn motion<T>(
        &mut self,
        term: &mut Term<T>,
        (point, side): (Point, Side),
        modifiers: Modifiers,
        context: &Context,
    ) -> Vec<Effect> {
        if self.selecting {
            if let Some(selection) = term.selection.as_mut() {
                selection.update(point, side);
            }
            return vec![Effect::Repaint];
        }
        if !reports_mouse(term, modifiers, context) || self.last_motion == Some(point) {
            return Vec::new();
        }
        let report = mouse_report(term, MouseEvent::Motion(self.pressed_button), modifiers, point);
        if report.is_some() {
            self.last_motion = Some(point);
        }
        report.into_iter().collect()
    }

    /// A mouse button went up, over `point` if that is on the text. `clicked` says whether
    /// the mouse stayed where the button went down.
    #[allow(clippy::too_many_arguments, reason = "the parts of one mouse event")]
    pub fn release<T>(
        &mut self,
        term: &mut Term<T>,
        point: Option<Point>,
        button: Option<MouseButton>,
        modifiers: Modifiers,
        clicked: bool,
        auto_copy: bool,
        context: &Context,
    ) -> Vec<Effect> {
        if reports_mouse(term, modifiers, context) {
            let report = point
                .zip(button)
                .and_then(|(point, button)| mouse_report(term, MouseEvent::Release(button), modifiers, point));
            self.pressed_button = None;
            return report.into_iter().collect();
        }
        if !self.selecting {
            return Vec::new();
        }
        self.selecting = false;
        let opens =
            clicked && button == Some(MouseButton::Left) && modifiers.is_empty() && context.options.open_links_by_click;
        if let Some(link) = point
            .filter(|_| opens)
            .and_then(|point| link_at(term, point, true, context))
        {
            return vec![Effect::OpenLink(link.target)];
        }
        // A finished selection goes into the primary selection, as in other terminals.
        selected_text(term, context.options)
            .filter(|_| auto_copy)
            .map(Effect::CopyPrimary)
            .into_iter()
            .collect()
    }

    /// The wheel turned over `point`: the program learns about it, full-screen programs get
    /// arrow keys, or the view scrolls.
    pub fn wheel<T>(
        &self,
        term: &Term<T>,
        point: Option<Point>,
        up: bool,
        alternate_scrolling: bool,
        context: &Context,
    ) -> Vec<Effect> {
        let mode = *term.mode();
        if reports_mouse(term, Modifiers::NONE, context) {
            let event = if up { MouseEvent::WheelUp } else { MouseEvent::WheelDown };
            let mut effects: Vec<Effect> = point
                .and_then(|point| mouse_report(term, event, Modifiers::NONE, point))
                .into_iter()
                .collect();
            effects.push(Effect::Refresh);
            effects
        } else if mode.contains(TermMode::ALT_SCREEN)
            && mode.contains(TermMode::ALTERNATE_SCROLL)
            && alternate_scrolling
        {
            let arrow = if up { "\x1bOA" } else { "\x1bOB" };
            vec![Effect::Write(arrow.repeat(3).into_bytes()), Effect::Refresh]
        } else {
            vec![Effect::Scroll(if up { 3 } else { -3 })]
        }
    }

    /// Searches from the selection, or from the cursor, and selects what it finds. With
    /// `highlight_all`, the search stays for [`Self::matches`] and [`Self::search_lines`].
    /// Returns whether something was found.
    #[allow(clippy::fn_params_excessive_bools, reason = "the options of the search bar")]
    pub fn find<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        pattern: &str,
        backwards: bool,
        case_sensitive: bool,
        regular_expression: bool,
        highlight_all: bool,
    ) -> bool {
        let pattern = search_pattern(pattern, case_sensitive, regular_expression);
        let Ok(mut regex) = RegexSearch::new(&pattern) else {
            return false;
        };
        let range = term.selection.as_ref().and_then(|selection| selection.to_range(term));
        let origin = match (range, backwards) {
            (Some(range), false) => range.end.add(term, Boundary::Grid, 1),
            (Some(range), true) => range.start.sub(term, Boundary::Grid, 1),
            (None, _) => term.grid().cursor.point,
        };
        let (direction, side) = if backwards {
            (Direction::Left, Side::Right)
        } else {
            (Direction::Right, Side::Left)
        };
        let found = term.search_next(&mut regex, origin, direction, side, None);
        if let Some(found) = &found {
            let mut selection = Selection::new(SelectionType::Simple, *found.start(), Side::Left);
            selection.update(*found.end(), Side::Right);
            term.selection = Some(selection);
            term.scroll_to_point(*found.start());
        }
        self.search = highlight_all.then_some(regex);
        found.is_some()
    }

    /// Finds the matches on the screen again, after it changed.
    pub fn update_matches<T>(&mut self, term: &Term<T>) {
        self.visible_matches = match self.search.as_mut() {
            Some(regex) => {
                let top = -(term.grid().display_offset() as i32);
                let start = Point::new(Line(top), Column(0));
                let end = Point::new(Line(top + term.screen_lines() as i32 - 1), term.last_column());
                RegexIter::new(start, end, Direction::Right, term, regex).collect()
            }
            None => Vec::new(),
        };
    }

    /// Where the matches lie in the scrollback, as fractions from its top, for the
    /// scrollbar. `None` without a search to highlight.
    pub fn search_lines<T>(&mut self, term: &Term<T>) -> Option<Vec<f64>> {
        const MOST_MATCHES: usize = 5000;
        let regex = self.search.as_mut()?;
        let top = -(term.grid().history_size() as i32);
        let total = f64::from(term.screen_lines() as i32 - top);
        let start = Point::new(Line(top), Column(0));
        let end = Point::new(Line(term.screen_lines() as i32 - 1), term.last_column());
        let mut lines: Vec<i32> = RegexIter::new(start, end, Direction::Right, term, regex)
            .take(MOST_MATCHES)
            .map(|found| found.start().line.0)
            .collect();
        lines.dedup();
        Some(lines.iter().map(|line| f64::from(line - top) / total).collect())
    }

    /// Closes the search and drops what it selected.
    pub fn end_search<T>(&mut self, term: &mut Term<T>) {
        self.search = None;
        self.visible_matches.clear();
        term.selection = None;
    }
}

/// What a key did in the selection mode.
#[derive(Debug, PartialEq)]
enum SelectionStep {
    /// The cursor, the selection or the view changed.
    Moved,
    /// The mode ends, after copying the selected text if there is some.
    Leave(Option<String>),
}

/// Keys of the selection mode: vi motions move the cursor, `v` and `V` select, `y` or Return
/// copy and Escape leaves.
fn selection_step<T: EventListener>(
    term: &mut Term<T>,
    key: Key,
    modifiers: Modifiers,
    text: &str,
    options: &Options,
) -> SelectionStep {
    let motion = match (key, text) {
        (Key::Left, _) | (_, "h") => Some(ViMotion::Left),
        (Key::Right, _) | (_, "l") => Some(ViMotion::Right),
        (Key::Up, _) | (_, "k") => Some(ViMotion::Up),
        (Key::Down, _) | (_, "j") => Some(ViMotion::Down),
        (Key::Home, _) | (_, "0") => Some(ViMotion::First),
        (Key::End, _) | (_, "$") => Some(ViMotion::Last),
        (_, "w") => Some(ViMotion::WordRight),
        (_, "b") => Some(ViMotion::WordLeft),
        (_, "H") => Some(ViMotion::High),
        (_, "M") => Some(ViMotion::Middle),
        (_, "L") => Some(ViMotion::Low),
        _ => None,
    };
    if let Some(motion) = motion {
        term.vi_motion(motion);
        let point = term.vi_mode_cursor.point;
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, Side::Right);
        }
    } else if matches!(key, Key::PageUp | Key::PageDown) {
        let page = scroll_page(term.screen_lines(), options);
        term.scroll_display(Scroll::Delta(if key == Key::PageUp { page } else { -page }));
    } else if text == "v" {
        let point = term.vi_mode_cursor.point;
        let kind = if modifiers.contains(Modifiers::CONTROL) {
            SelectionType::Block
        } else {
            SelectionType::Simple
        };
        term.selection = match term.selection {
            Some(_) => None,
            None => Some(Selection::new(kind, point, Side::Left)),
        };
    } else if text == "V" {
        let point = term.vi_mode_cursor.point;
        term.selection = Some(Selection::new(SelectionType::Lines, point, Side::Left));
    } else if text == "y" || matches!(key, Key::Return | Key::Enter) {
        return SelectionStep::Leave(selected_text(term, options));
    } else if key == Key::Escape {
        return SelectionStep::Leave(None);
    }
    SelectionStep::Moved
}

/// Only these schemes open from the terminal. Text printed by a program must not be
/// able to start arbitrary handlers such as custom URL schemes or local files.
#[must_use]
pub fn is_safe_link(uri: &str, schemes: &[String]) -> bool {
    schemes.iter().any(|scheme| uri.starts_with(scheme.as_str()))
}

/// The word that can name a color at a column: `#rgb` to `#rrrrggggbbbb`, or letters.
#[must_use]
pub fn color_word(line: &str, column: usize) -> Option<&str> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '#';
    let characters: Vec<(usize, char)> = line.char_indices().collect();
    if !is_word(characters.get(column)?.1) {
        return None;
    }
    let before = characters.get(..column)?;
    let first = before
        .iter()
        .rposition(|(_, c)| !is_word(*c))
        .map_or(0, |index| index + 1);
    let last = characters
        .iter()
        .skip(column)
        .position(|(_, c)| !is_word(*c))
        .map(|index| index + column);
    let start = characters.get(first)?.0;
    let end = last
        .and_then(|last| characters.get(last))
        .map_or(line.len(), |(offset, _)| *offset);
    let word = line.get(start..end)?;
    let valid = match word.strip_prefix('#') {
        Some(digits) => (3..=12).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_hexdigit()),
        None => (3..=20).contains(&word.len()) && word.bytes().all(|byte| byte.is_ascii_alphabetic()),
    };
    valid.then_some(word)
}

/// Links on the screen, top to bottom, as `(target, screen line, column)`.
#[must_use]
pub fn links_on_screen<T>(term: &Term<T>, directory: Option<&Path>) -> Vec<(String, i32, i32)> {
    let offset = term.grid().display_offset() as i32;
    let mut links = Vec::new();
    for screen_line in 0..term.screen_lines() as i32 {
        let row = &term.grid()[Line(screen_line - offset)];
        let text: String = (0..term.columns()).map(|column| row[Column(column)].c).collect();
        let mut column = 0;
        while let Some((start, end, word)) = next_word(&text, column) {
            let target = url_in(&word).or_else(|| directory.and_then(|directory| existing_file(&word, directory)));
            if let Some(target) = target {
                links.push((target, screen_line, start as i32));
            }
            column = end;
        }
    }
    links
}

fn next_word(line: &str, from: usize) -> Option<(usize, usize, String)> {
    let length = line.chars().count();
    (from..length).find_map(|column| word_span(line, column))
}

/// The links that get a number: the first nine, or the last nine when reversed.
#[must_use]
pub fn visible_hints(links: &[(String, i32, i32)], reverse: bool) -> Vec<(usize, &(String, i32, i32))> {
    (0..links.len().min(9))
        .filter_map(|index| pick_hint(links, index, reverse).map(|link| (index, link)))
        .collect()
}

#[must_use]
pub fn pick_hint(links: &[(String, i32, i32)], index: usize, reverse: bool) -> Option<&(String, i32, i32)> {
    if reverse {
        links
            .len()
            .checked_sub(index + 1)
            .and_then(|position| links.get(position))
    } else {
        links.get(index)
    }
}

/// The word around column `index` as `(start, end, text)`.
fn word_span(line: &str, index: usize) -> Option<(usize, usize, String)> {
    let characters: Vec<char> = line.chars().collect();
    let is_end = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`');
    if is_end(*characters.get(index)?) {
        return None;
    }
    let start = characters
        .get(..index)?
        .iter()
        .rposition(|c| is_end(*c))
        .map_or(0, |i| i + 1);
    let end = characters
        .iter()
        .skip(index)
        .position(|c| is_end(*c))
        .map_or(characters.len(), |i| i + index);
    Some((start, end, characters.get(start..end)?.iter().collect()))
}

/// Finds a web address inside a word, without trailing punctuation.
fn url_in(word: &str) -> Option<String> {
    let begin = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| word.find(scheme))
        .min()?;
    let url = word[begin..].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
    url.split_once("://")
        .is_some_and(|(_, rest)| !rest.is_empty())
        .then(|| url.to_string())
}

/// Finds a web address in `line` that covers column `index`.
#[cfg(test)]
fn url_at(line: &str, index: usize) -> Option<String> {
    url_in(&word_span(line, index)?.2)
}

/// Resolves a word to an existing file. Text printed by a program must not start other
/// programs through a click, so executables and desktop entries do not count.
fn existing_file(word: &str, directory: &Path) -> Option<String> {
    let word = word.trim_end_matches(['.', ',', ';', ')', ']']);
    let name = word.split(':').next().filter(|name| !name.is_empty())?;
    // Even looking at a file on a share would make Windows log in to its server.
    if crate::links::is_remote(name) {
        return None;
    }
    let path = match name.strip_prefix("~/") {
        Some(rest) => crate::process::home()?.join(rest),
        None => directory.join(name),
    };
    let metadata = std::fs::metadata(&path).ok()?;
    let plain = metadata.is_file() && is_inert(&path, &metadata);
    let desktop_entry = path.extension().is_some_and(|extension| extension == "desktop");
    let position = &word[name.len()..];
    let is_position = position
        .split(':')
        .skip(1)
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    let position = if is_position { position } else { "" };
    (plain && !desktop_entry).then(|| format!("{}{position}", path.to_string_lossy()))
}

/// Whether opening a file starts nothing but a viewer or an editor: on Unix, any file that
/// is not executable.
#[cfg(unix)]
#[must_use]
pub fn is_inert(_: &Path, metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 == 0
}

/// Whether opening a file starts nothing but a viewer or an editor. On Windows the
/// extension decides what opens it, and too many run code to list them all, from `.exe` to
/// `.py` once Python is installed, so only types known to be inert count.
#[cfg(not(unix))]
#[must_use]
pub fn is_inert(path: &Path, _: &std::fs::Metadata) -> bool {
    const INERT: [&str; 62] = [
        "txt", "text", "md", "markdown", "rst", "adoc", "log", "csv", "tsv", "json", "jsonc", "toml", "yaml", "yml",
        "ini", "cfg", "conf", "diff", "patch", "tex", "rs", "c", "h", "cc", "cpp", "cxx", "hpp", "hxx", "cs", "go",
        "java", "kt", "swift", "css", "scss", "sql", "xml", "pdf", "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico",
        "tif", "tiff", "mp3", "wav", "flac", "ogg", "mp4", "mkv", "webm", "mov", "docx", "xlsx", "pptx", "odt", "ods",
        "odp", "rtf",
    ];
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| INERT.iter().any(|inert| extension.eq_ignore_ascii_case(inert)))
}

/// Lines moved by Shift+Page Up and Page Down.
#[must_use]
pub fn scroll_page(lines: usize, options: &Options) -> i32 {
    if options.scroll_full_page {
        lines as i32
    } else {
        (lines as i32 / 2).max(1)
    }
}

/// The selection as text, trimmed as configured.
#[must_use]
pub fn selected_text<T>(term: &Term<T>, options: &Options) -> Option<String> {
    let text = term.selection_to_string()?;
    if !options.trim_leading && !options.trim_trailing {
        return Some(text);
    }
    Some(trim_lines(&text, options.trim_leading, options.trim_trailing))
}

fn trim_lines(text: &str, leading: bool, trailing: bool) -> String {
    let trimmed: Vec<&str> = text
        .split('\n')
        .map(|line| {
            let line = if leading { line.trim_start() } else { line };
            if trailing { line.trim_end() } else { line }
        })
        .collect();
    trimmed.join("\n")
}

fn html_escape(character: char, html: &mut String) {
    match character {
        '<' => html.push_str("&lt;"),
        '>' => html.push_str("&gt;"),
        '&' => html.push_str("&amp;"),
        _ => html.push(character),
    }
}

/// The selection with its colors and styles, for pasting into rich text editors.
#[must_use]
pub fn selection_html<T>(term: &Term<T>) -> String {
    let Some(range) = term.selection.as_ref().and_then(|selection| selection.to_range(term)) else {
        return String::new();
    };
    let scheme = colors::active();
    let palette = term.colors();
    let grid = term.grid();
    let columns = term.columns();
    let hex = |color: Rgb| format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b);
    let mut html = format!(
        "<pre style=\"color:{};background-color:{};font-family:monospace\">",
        hex(scheme.foreground),
        hex(scheme.background)
    );
    for line in range.start.line.0..=range.end.line.0 {
        let first = if range.is_block || line == range.start.line.0 {
            range.start.column.0
        } else {
            0
        };
        let last = if range.is_block || line == range.end.line.0 {
            range.end.column.0
        } else {
            columns - 1
        };
        let row = &grid[Line(line)];
        let end = (first..=last)
            .rev()
            .find(|column| row[Column(*column)].c != ' ')
            .map_or(first, |column| column + 1);
        let mut style = String::new();
        for column in first..end.min(columns) {
            let cell = &row[Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let mut wanted = format!("color:{}", hex(resolve(cell.fg, palette, &scheme)));
            let back = resolve(cell.bg, palette, &scheme);
            if back != scheme.background {
                wanted.push_str(";background-color:");
                wanted.push_str(&hex(back));
            }
            if cell.flags.contains(Flags::BOLD) {
                wanted.push_str(";font-weight:bold");
            }
            if cell.flags.contains(Flags::ITALIC) {
                wanted.push_str(";font-style:italic");
            }
            if cell.flags.intersects(Flags::ALL_UNDERLINES) {
                wanted.push_str(";text-decoration:underline");
            }
            if wanted != style {
                if !style.is_empty() {
                    html.push_str("</span>");
                }
                html.push_str("<span style=\"");
                html.push_str(&wanted);
                html.push_str("\">");
                style = wanted;
            }
            html_escape(cell.c, &mut html);
        }
        if !style.is_empty() {
            html.push_str("</span>");
        }
        if line != range.end.line.0 {
            html.push('\n');
        }
    }
    html.truncate(html.trim_end().len());
    html.push_str("</pre>");
    html
}

/// Builds the pattern for the search bar; alacritty's own default is smart case.
#[must_use]
pub fn search_pattern(text: &str, case_sensitive: bool, regular_expression: bool) -> String {
    let body = if regular_expression {
        text.to_string()
    } else {
        text.chars()
            .flat_map(|c| {
                let special = "\\.+*?()|[]{}^$#&-~".contains(c);
                special.then_some('\\').into_iter().chain([c])
            })
            .collect()
    };
    format!("{}{body}", if case_sensitive { "(?-i)" } else { "(?i)" })
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use alacritty_terminal::event::VoidListener;

    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    static OPTIONS: LazyLock<Options> = LazyLock::new(Options::default);

    /// A terminal of 20 columns and 5 lines that has received `output`.
    fn term_with(output: &str) -> Term<VoidListener> {
        term_of(20, output)
    }

    /// A terminal of `columns` columns and 5 lines that has received `output`.
    fn term_of(columns: usize, output: &str) -> Term<VoidListener> {
        use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
        let size = alacritty_terminal::term::test::TermSize::new(columns, 5);
        let mut term = Term::new(
            alacritty_terminal::term::Config::default(),
            &size,
            alacritty_terminal::event::VoidListener,
        );
        Processor::<StdSyncHandler>::new().advance(&mut term, output.as_bytes());
        term
    }

    fn select_row(term: &mut Term<VoidListener>, line: i32) {
        let start = Point::new(Line(line), Column(0));
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(Point::new(Line(line), Column(19)), Side::Right);
        term.selection = Some(selection);
    }

    #[test]
    fn color_words_are_found_around_a_column() {
        let line = "bg #3daee9, name tomato x1";
        assert_eq!(color_word(line, 5), Some("#3daee9"));
        assert_eq!(color_word(line, 18), Some("tomato"));
        assert_eq!(color_word(line, 0), None);
        assert_eq!(color_word(line, 2), None);
        assert_eq!(color_word(line, 25), None);
    }

    #[test]
    fn hints_count_from_either_end() {
        let links: Vec<_> = (0..12).map(|index| (index.to_string(), index, 0)).collect();
        assert_eq!(visible_hints(&links, false).len(), 9);
        assert_eq!(pick_hint(&links, 0, false).unwrap().0, "0");
        assert_eq!(pick_hint(&links, 0, true).unwrap().0, "11");
        assert_eq!(pick_hint(&links[..2], 5, true), None);
    }

    #[test]
    fn copied_lines_can_be_trimmed() {
        assert_eq!(trim_lines("  a  \n b ", true, false), "a  \nb ");
        assert_eq!(trim_lines("  a  \n b ", false, true), "  a\n b");
        assert_eq!(trim_lines("  a  \n\n b ", true, true), "a\n\nb");
    }

    #[test]
    fn search_patterns_escape_plain_text() {
        assert_eq!(search_pattern("a.b", false, false), "(?i)a\\.b");
        assert_eq!(search_pattern("a.b", true, true), "(?-i)a.b");
    }

    #[test]
    fn files_on_a_share_are_no_links() {
        let here = std::env::temp_dir();
        assert_eq!(existing_file("\\\\server\\share\\notes.txt", &here), None);
        assert_eq!(existing_file("//server/share/notes.txt", &here), None);
    }

    #[test]
    fn search_patterns_compile() {
        assert!(RegexSearch::new(&search_pattern("(x) [y]+ {z}", false, false)).is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn plain_files_can_be_opened_but_not_programs() {
        use std::os::unix::fs::PermissionsExt;
        let directory = std::env::temp_dir().join(format!("kudari-links-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("notes.txt"), "x").unwrap();
        std::fs::write(directory.join("run.sh"), "x").unwrap();
        std::fs::set_permissions(directory.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(directory.join("evil.desktop"), "x").unwrap();
        let expected = directory.join("notes.txt").to_string_lossy().into_owned();
        assert_eq!(
            existing_file("notes.txt:12:3", &directory),
            Some(format!("{expected}:12:3"))
        );
        assert_eq!(existing_file("notes.txt:", &directory), Some(expected.clone()));
        assert_eq!(existing_file("notes.txt,", &directory), Some(expected));
        assert_eq!(existing_file("run.sh", &directory), None);
        assert_eq!(existing_file("evil.desktop", &directory), None);
        assert_eq!(existing_file("missing", &directory), None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn urls_are_found_under_the_cursor() {
        let line = "see https://example.org/a?b=1, or (http://x.y).";
        assert_eq!(url_at(line, 10).unwrap(), "https://example.org/a?b=1");
        assert_eq!(url_at(line, 37).unwrap(), "http://x.y");
        assert_eq!(url_at(line, 1), None);
    }

    #[test]
    fn text_without_a_host_is_no_url() {
        assert_eq!(url_at("https:// nothing", 3), None);
    }

    #[test]
    fn only_web_and_mail_links_are_safe() {
        let schemes = &Options::default().link_schemes;
        assert!(is_safe_link("https://kde.org", schemes));
        assert!(is_safe_link("mailto:a@b.c", schemes));
        assert!(!is_safe_link("file:///etc/passwd", schemes));
        assert!(!is_safe_link("javascript:alert(1)", schemes));
        assert!(!is_safe_link("custom-handler://run", schemes));
        assert!(!is_safe_link("https://kde.org", &[]));
    }

    #[test]
    fn the_selection_mode_selects_and_copies() {
        let mut term = term_with("abc def\r\nghi");
        term.toggle_vi_mode();
        for key in ["k", "0", "v", "l", "l"] {
            assert_eq!(
                selection_step(&mut term, Key::Other, NONE, key, &OPTIONS),
                SelectionStep::Moved
            );
        }
        assert_eq!(
            selection_step(&mut term, Key::Other, NONE, "y", &OPTIONS),
            SelectionStep::Leave(Some("abc".into()))
        );
    }

    #[test]
    fn the_selection_mode_knows_lines_words_and_blocks() {
        let mut term = term_with("abc def\r\nghi jkl");
        term.toggle_vi_mode();
        selection_step(&mut term, Key::Other, NONE, "k", &OPTIONS);
        selection_step(&mut term, Key::Other, NONE, "V", &OPTIONS);
        let Some(SelectionStep::Leave(Some(line))) = Some(selection_step(&mut term, Key::Return, NONE, "", &OPTIONS))
        else {
            panic!("Return copies");
        };
        assert!(line.starts_with("abc def"), "{line:?}");

        term.selection = None;
        selection_step(&mut term, Key::Other, NONE, "0", &OPTIONS);
        selection_step(&mut term, Key::Other, Modifiers::CONTROL, "v", &OPTIONS);
        for key in ["w", "j"] {
            selection_step(&mut term, Key::Other, NONE, key, &OPTIONS);
        }
        let Some(SelectionStep::Leave(Some(block))) = Some(selection_step(&mut term, Key::Other, NONE, "y", &OPTIONS))
        else {
            panic!("y copies");
        };
        assert_eq!(block.lines().count(), 2, "{block:?}");
    }

    #[test]
    fn the_selection_mode_moves_and_leaves() {
        let mut term = term_with("abc");
        term.toggle_vi_mode();
        for key in [Key::Left, Key::Right, Key::Up, Key::Down, Key::Home, Key::End] {
            assert_eq!(selection_step(&mut term, key, NONE, "", &OPTIONS), SelectionStep::Moved);
        }
        for text in ["h", "l", "j", "$", "b", "H", "M", "L", "v", "v", "q"] {
            assert_eq!(
                selection_step(&mut term, Key::Other, NONE, text, &OPTIONS),
                SelectionStep::Moved
            );
        }
        assert_eq!(
            selection_step(&mut term, Key::PageUp, NONE, "", &OPTIONS),
            SelectionStep::Moved
        );
        assert_eq!(
            selection_step(&mut term, Key::PageDown, NONE, "", &OPTIONS),
            SelectionStep::Moved
        );
        assert_eq!(
            selection_step(&mut term, Key::Other, NONE, "y", &OPTIONS),
            SelectionStep::Leave(None)
        );
        assert_eq!(
            selection_step(&mut term, Key::Escape, NONE, "", &OPTIONS),
            SelectionStep::Leave(None)
        );
    }

    #[test]
    fn copied_html_keeps_colors_and_styles() {
        let mut term = term_with("\x1b[1;31m<b>\x1b[0m & \x1b[3;4;44mx\x1b[0m");
        assert_eq!(selection_html(&term), "");
        select_row(&mut term, 0);
        let html = selection_html(&term);
        assert!(
            html.starts_with("<pre style=\"color:#") && html.ends_with("</pre>"),
            "{html}"
        );
        assert!(html.contains(";font-weight:bold\">&lt;b&gt;</span>"), "{html}");
        assert!(html.contains("&amp;"), "{html}");
        assert!(html.contains(";background-color:#"), "{html}");
        assert!(
            html.contains(";font-style:italic;text-decoration:underline\">x</span>"),
            "{html}"
        );
    }

    #[test]
    fn copied_html_spans_several_lines() {
        let mut term = term_with("one\r\ntwo");
        let mut selection = Selection::new(SelectionType::Simple, Point::new(Line(0), Column(1)), Side::Left);
        selection.update(Point::new(Line(1), Column(1)), Side::Right);
        term.selection = Some(selection);
        let html = selection_html(&term);
        assert!(html.contains(">ne</span>\n<span"), "{html}");
        assert!(html.contains(">tw</span></pre>"), "{html}");
    }

    #[test]
    fn links_on_the_screen_are_urls_and_files() {
        let directory = std::env::temp_dir().join(format!("kudari-screen-links-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("notes.txt"), "").unwrap();
        let term = term_with("see https://a.org/x\r\nnotes.txt:3 x");
        let links = links_on_screen(&term, Some(&directory));
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(links.len(), 2, "{links:?}");
        assert_eq!(links[0], ("https://a.org/x".to_string(), 0, 4));
        assert!(links[1].0.ends_with("notes.txt:3") && links[1].1 == 1, "{links:?}");
        assert_eq!(links_on_screen(&term, None).len(), 1);
    }

    #[test]
    fn random_text_does_no_harm() {
        let mut random = crate::fuzz::Random::new(7);
        let samples = [
            "see notes.txt:12:3 or https://example.org/a?b=c, #ff8800 tomato",
            "(a|b)+ [x",
        ];
        for input in crate::fuzz::inputs(&samples) {
            let column = random.below(100);
            let _ = word_span(&input, column);
            let _ = color_word(&input, column);
            let _ = url_in(&input);
            let _ = search_pattern(&input, false, false);
            let _ = search_pattern(&input, true, true);
            let _ = trim_lines(&input, true, true);
            let _ = is_safe_link(&input, &OPTIONS.link_schemes);
        }
    }

    fn context<'a>(
        options: &'a Options,
        tracks_mouse: bool,
        directory: &'a dyn Fn() -> Option<PathBuf>,
    ) -> Context<'a> {
        Context {
            options,
            tracks_mouse,
            directory,
        }
    }

    fn nowhere() -> Option<PathBuf> {
        None
    }

    const LEFT: Option<MouseButton> = Some(MouseButton::Left);

    #[test]
    fn keys_type_unless_something_takes_them() {
        let mut term = term_with("abc");
        let mut view = View::default();
        let options = Options::default();
        let context = context(&options, true, &nowhere);
        select_row(&mut term, 0);
        let effects = view.key(&mut term, Key::Character('A'), Modifiers::SHIFT, "A", &context);
        assert_eq!(
            effects,
            [Effect::Hints(false), Effect::Type(b"A".to_vec()), Effect::Refresh]
        );
        assert!(term.selection.is_none(), "typing drops the selection");
        assert_eq!(
            view.key(&mut term, Key::Other, NONE, "", &context),
            [Effect::Hints(false)]
        );
        let page = view.key(&mut term, Key::PageUp, Modifiers::SHIFT, "", &context);
        assert_eq!(page, [Effect::Hints(false), Effect::Scroll(2)]);
        let page = view.key(&mut term, Key::PageDown, Modifiers::SHIFT, "", &context);
        assert_eq!(page, [Effect::Hints(false), Effect::Scroll(-2)]);
    }

    #[test]
    fn key_binding_files_come_first() {
        let mut term = term_with("");
        let mut view = View::default();
        let rules = keytab::parse(
            "key A+Ctrl : \"x\"\nkey B+Ctrl : erase\nkey C+Ctrl : scrollPageUp\nkey D+Ctrl : scrollPageDown\nkey E+Ctrl : scrollLineUp\nkey F+Ctrl : scrollLineDown\nkey G+Ctrl : scrollUpToTop\nkey H+Ctrl : scrollDownToBottom\n",
        );
        let options = Options {
            key_rules: std::sync::Arc::new(rules),
            ..Options::default()
        };
        let context = context(&options, true, &nowhere);
        let mut press = |letter: char| {
            view.key(&mut term, Key::Character(letter), Modifiers::CONTROL, "", &context)
                .pop()
        };
        assert_eq!(press('A'), Some(Effect::Type(b"x".to_vec())));
        assert_eq!(press('B'), Some(Effect::Type(vec![0x7f])));
        assert_eq!(press('C'), Some(Effect::Scroll(2)));
        assert_eq!(press('D'), Some(Effect::Scroll(-2)));
        assert_eq!(press('E'), Some(Effect::Scroll(1)));
        assert_eq!(press('F'), Some(Effect::Scroll(-1)));
        assert_eq!(press('G'), Some(Effect::Scroll(i32::MAX / 2)));
        assert_eq!(press('H'), Some(Effect::Scroll(i32::MIN / 2)));
    }

    #[test]
    fn link_numbers_open_links() {
        let mut term = term_with("a https://a.org b\r\nhttps://b.org");
        let mut view = View::default();
        let options = Options {
            url_hints_modifiers: Modifiers::CONTROL | Modifiers::ALT,
            ..Options::default()
        };
        let context = context(&options, true, &nowhere);
        let held = Modifiers::CONTROL | Modifiers::ALT;
        assert_eq!(
            view.key(&mut term, Key::Other, held, "", &context),
            [Effect::Hints(true)]
        );
        assert_eq!(
            view.key(&mut term, Key::Character('2'), held, "", &context),
            [
                Effect::Hints(true),
                Effect::OpenLink("https://b.org".into()),
                Effect::Hints(false)
            ]
        );
        // A number without a link types as usual.
        let effects = view.key(&mut term, Key::Character('3'), held, "3", &context);
        assert_eq!(effects.first(), Some(&Effect::Hints(true)));
        assert!(!effects.iter().any(|effect| matches!(effect, Effect::OpenLink(_))));
    }

    #[test]
    fn the_selection_mode_takes_the_keys_until_it_ends() {
        let mut term = term_with("abc def");
        let mut view = View::default();
        let options = Options::default();
        let context = context(&options, true, &nowhere);
        assert_eq!(
            toggle_selection_mode(&mut term),
            [Effect::SelectionMode(true), Effect::Repaint]
        );
        assert!(selection_mode(&term));
        view.key(&mut term, Key::Home, NONE, "", &context);
        assert_eq!(
            view.key(&mut term, Key::Other, NONE, "v", &context),
            [Effect::Hints(false), Effect::Refresh]
        );
        view.key(&mut term, Key::Other, NONE, "l", &context);
        assert_eq!(
            view.key(&mut term, Key::Other, NONE, "y", &context),
            [
                Effect::Hints(false),
                Effect::Copy("ab".into()),
                Effect::SelectionMode(false),
                Effect::Repaint
            ]
        );
        assert!(!selection_mode(&term) && term.selection.is_none());
    }

    #[test]
    fn dragging_selects_and_copies_into_the_primary_selection() {
        let mut term = term_with("hello world");
        let mut view = View::default();
        let options = Options::default();
        let context = context(&options, true, &nowhere);
        let start = (Point::new(Line(0), Column(0)), Side::Left);
        let end = (Point::new(Line(0), Column(4)), Side::Right);
        assert_eq!(
            view.press(&mut term, start, LEFT, 1, NONE, None, &context),
            [Effect::Repaint]
        );
        assert!(view.selecting());
        assert_eq!(view.motion(&mut term, end, NONE, &context), [Effect::Repaint]);
        assert!(selection_contains(&term, Point::new(Line(0), Column(2))));
        assert!(!selection_contains(&term, Point::new(Line(0), Column(7))));
        let released = view.release(&mut term, Some(end.0), LEFT, NONE, false, true, &context);
        assert_eq!(released, [Effect::CopyPrimary("hello".into())]);
        assert!(!view.selecting());
        assert!(
            view.release(&mut term, Some(end.0), LEFT, NONE, false, true, &context)
                .is_empty()
        );
        // Shift extends the selection, a double click selects a word, a triple click a line.
        view.press(
            &mut term,
            (Point::new(Line(0), Column(8)), Side::Right),
            LEFT,
            1,
            Modifiers::SHIFT,
            None,
            &context,
        );
        assert_eq!(selected_text(&term, &options).as_deref(), Some("hello wor"));
        view.press(
            &mut term,
            (Point::new(Line(0), Column(8)), Side::Left),
            LEFT,
            2,
            NONE,
            None,
            &context,
        );
        assert_eq!(selected_text(&term, &options).as_deref(), Some("world"));
        view.press(&mut term, start, LEFT, 3, NONE, None, &context);
        assert_eq!(selected_text(&term, &options).as_deref(), Some("hello world\n"));
        let forward = Options {
            triple_click_forward: true,
            ..Options::default()
        };
        let from_click = (Point::new(Line(0), Column(6)), Side::Left);
        view.press(
            &mut term,
            from_click,
            LEFT,
            3,
            NONE,
            None,
            &self::context(&forward, true, &nowhere),
        );
        assert_eq!(selected_text(&term, &options).as_deref(), Some("world"));
        view.press(
            &mut term,
            start,
            LEFT,
            1,
            Modifiers::CONTROL | Modifiers::ALT,
            None,
            &context,
        );
        assert_eq!(
            term.selection.as_ref().map(|selection| selection.ty),
            Some(SelectionType::Block)
        );
    }

    #[test]
    fn clicks_open_links_when_asked_to() {
        let mut term = term_with("go https://kde.org now");
        let mut view = View::default();
        let options = Options::default();
        let context = context(&options, true, &nowhere);
        let on_link = (Point::new(Line(0), Column(5)), Side::Left);
        let opened = vec![Effect::OpenLink("https://kde.org".into())];
        assert_eq!(
            view.press(&mut term, on_link, LEFT, 1, Modifiers::CONTROL, None, &context),
            opened
        );
        assert!(
            view.press(
                &mut term,
                (Point::new(Line(0), Column(0)), Side::Left),
                LEFT,
                1,
                Modifiers::CONTROL,
                None,
                &context
            )
            .is_empty()
        );
        let by_click = Options {
            open_links_by_click: true,
            ..Options::default()
        };
        let click_context = self::context(&by_click, true, &nowhere);
        view.press(&mut term, on_link, LEFT, 1, NONE, None, &click_context);
        assert_eq!(
            view.release(&mut term, Some(on_link.0), LEFT, NONE, true, true, &click_context),
            opened
        );
        view.press(&mut term, on_link, LEFT, 1, NONE, None, &context);
        assert_ne!(
            view.release(&mut term, Some(on_link.0), LEFT, NONE, true, true, &context),
            opened
        );
    }

    #[test]
    fn the_middle_button_pastes() {
        let mut term = term_with("");
        let mut view = View::default();
        let options = Options::default();
        let context = context(&options, true, &nowhere);
        let cell = (Point::new(Line(0), Column(0)), Side::Left);
        let middle = Some(MouseButton::Middle);
        assert_eq!(
            view.press(&mut term, cell, middle, 1, NONE, Some(true), &context),
            [Effect::Paste { selection: true }]
        );
        assert!(view.press(&mut term, cell, middle, 1, NONE, None, &context).is_empty());
        assert!(
            view.press(&mut term, cell, Some(MouseButton::Right), 1, NONE, None, &context)
                .is_empty()
        );
    }

    #[test]
    fn programs_that_track_the_mouse_get_reports() {
        let mut term = term_with("\x1b[?1002h\x1b[?1006h");
        let mut view = View::default();
        let options = Options::default();
        let context = context(&options, true, &nowhere);
        let cell = (Point::new(Line(1), Column(2)), Side::Left);
        let next = (Point::new(Line(1), Column(3)), Side::Left);
        assert_eq!(
            view.press(&mut term, cell, LEFT, 1, NONE, None, &context),
            [Effect::Write(b"\x1b[<0;3;2M".to_vec())]
        );
        assert_eq!(
            view.motion(&mut term, next, NONE, &context),
            [Effect::Write(b"\x1b[<32;4;2M".to_vec())]
        );
        assert!(
            view.motion(&mut term, next, NONE, &context).is_empty(),
            "each cell once"
        );
        assert_eq!(
            view.release(&mut term, Some(next.0), LEFT, NONE, false, true, &context),
            [Effect::Write(b"\x1b[<0;4;2m".to_vec())]
        );
        assert_eq!(
            view.wheel(&term, Some(cell.0), true, true, &context),
            [Effect::Write(b"\x1b[<64;3;2M".to_vec()), Effect::Refresh]
        );
        // Shift keeps the mouse for selecting, and so does a terminal that may not track it.
        assert_eq!(
            view.press(&mut term, cell, LEFT, 1, Modifiers::SHIFT, None, &context),
            [Effect::Repaint]
        );
        view.release(&mut term, Some(cell.0), LEFT, Modifiers::SHIFT, false, false, &context);
        let untracked = self::context(&options, false, &nowhere);
        assert_eq!(
            view.press(&mut term, cell, LEFT, 1, NONE, None, &untracked),
            [Effect::Repaint]
        );
        assert!(
            view.motion(&mut term, next, NONE, &context)
                .iter()
                .all(|effect| *effect == Effect::Repaint)
        );
    }

    #[test]
    fn the_wheel_scrolls_or_sends_arrows() {
        let options = Options::default();
        let context = context(&options, true, &nowhere);
        let view = View::default();
        let term = term_with("");
        assert_eq!(view.wheel(&term, None, true, true, &context), [Effect::Scroll(3)]);
        assert_eq!(view.wheel(&term, None, false, true, &context), [Effect::Scroll(-3)]);
        let full_screen = term_with("\x1b[?1049h\x1b[?1007h");
        assert_eq!(
            view.wheel(&full_screen, None, false, true, &context),
            [Effect::Write(b"\x1bOB\x1bOB\x1bOB".to_vec()), Effect::Refresh]
        );
        assert_eq!(
            view.wheel(&full_screen, None, true, false, &context),
            [Effect::Scroll(3)]
        );
    }

    #[test]
    fn the_search_selects_and_highlights() {
        let mut term = term_with("one two one\r\nthree one");
        let mut view = View::default();
        assert!(view.find(&mut term, "one", false, false, false, true));
        let first = term
            .selection
            .as_ref()
            .and_then(|selection| selection.to_range(&term))
            .unwrap();
        assert!(view.find(&mut term, "one", false, false, false, true));
        let second = term
            .selection
            .as_ref()
            .and_then(|selection| selection.to_range(&term))
            .unwrap();
        assert_ne!(first.start, second.start);
        assert!(view.find(&mut term, "ONE", true, false, false, true));
        assert!(!view.find(&mut term, "ONE", false, true, false, true));
        assert!(
            !view.find(&mut term, "(", false, false, true, true),
            "a broken pattern finds nothing"
        );
        assert!(view.find(&mut term, "t.o", false, false, true, true));
        view.update_matches(&term);
        assert_eq!(view.matches().len(), 1);
        assert_eq!(view.search_lines(&term).map(|lines| lines.len()), Some(1));
        assert!(view.find(&mut term, "one", false, false, false, false));
        view.update_matches(&term);
        assert!(view.matches().is_empty() && view.search_lines(&term).is_none());
        view.end_search(&mut term);
        assert!(term.selection.is_none());
    }

    #[test]
    fn links_are_found_under_the_pointer() {
        let directory = std::env::temp_dir().join(format!("kudari-view-links-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("notes.txt"), "").unwrap();
        let term = term_of(
            40,
            "(https://a.org/x) notes.txt \x1b]8;;https://kde.org\x1b\\KDE\x1b]8;;\x1b\\ \x1b]8;;file:///etc\x1b\\etc\x1b]8;;\x1b\\",
        );
        let options = Options::default();
        let found = || Some(directory.clone());
        let context = context(&options, true, &found);
        let at = |column: usize, files: bool| link_at(&term, Point::new(Line(0), Column(column)), files, &context);
        assert_eq!(
            at(3, true),
            Some(Link {
                target: "https://a.org/x".into(),
                start: Point::new(Line(0), Column(1)),
                cells: 15
            })
        );
        assert!(at(20, true).unwrap().target.ends_with("notes.txt"));
        assert_eq!(at(20, false), None);
        assert_eq!(at(17, true), None);
        let kde = at(29, true).unwrap();
        assert_eq!(
            (kde.target.as_str(), kde.start.column.0, kde.cells),
            ("https://kde.org", 28, 3)
        );
        assert_eq!(at(33, true), None, "unsafe schemes do not count");
        let hidden = Options {
            escaped_links: false,
            ..Options::default()
        };
        let plain = link_at(
            &term,
            Point::new(Line(0), Column(29)),
            true,
            &self::context(&hidden, true, &found),
        );
        assert_eq!(plain, None);
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn lines_and_everything_can_be_selected() {
        let mut term = term_with("first\r\nsecond");
        let options = Options::default();
        select_line(&mut term);
        assert_eq!(selected_text(&term, &options).as_deref(), Some("second\n"));
        select_all(&mut term);
        assert!(selected_text(&term, &options).unwrap().starts_with("first\nsecond"));
        assert_eq!(
            color_word_at(&term_with("x #ff0000"), Point::new(Line(0), Column(4))).as_deref(),
            Some("#ff0000")
        );
        assert_eq!(color_word_at(&term, Point::new(Line(0), Column(9))), None);
    }

    #[test]
    fn scrolling_stops_at_the_ends() {
        let mut term = term_with(&"x\r\n".repeat(20));
        assert_eq!(scroll(&mut term, 3), 3);
        assert_eq!(scroll(&mut term, 1000), 13);
        assert_eq!(scroll(&mut term, -1000), -16);
        assert_eq!(scroll_page(5, &Options::default()), 2);
        let full = Options {
            scroll_full_page: true,
            ..Options::default()
        };
        assert_eq!(scroll_page(5, &full), 5);
    }
}
