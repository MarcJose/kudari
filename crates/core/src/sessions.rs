// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Sessions, their tabs and how their terminals share the space, without any UI.
//!
//! A session is a tab. Its area is a tree of panes: a pane is either a terminal or a row or
//! column of panes, each with its share of the space. Session and terminal ids count up
//! from 0 and are never reused, so that scripts can rely on them.

use std::collections::{BTreeMap, BTreeSet};

pub type SessionId = i32;
pub type TerminalId = i32;

/// Width of the line between two terminals, as Breeze draws a splitter.
pub const HANDLE_WIDTH: i32 = 1;
/// Smallest extent a terminal can be resized to.
pub const MIN_EXTENT: i32 = 40;
/// Limits of the font zoom in points.
const MIN_FONT_ZOOM: i32 = -8;
const MAX_FONT_ZOOM: i32 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    /// Side by side.
    Horizontal,
    /// One above the other.
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionType {
    Single,
    TwoHorizontal,
    TwoVertical,
    Quad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Right,
    Down,
    Left,
}

impl Direction {
    fn orientation(self) -> Orientation {
        match self {
            Self::Left | Self::Right => Orientation::Horizontal,
            Self::Up | Self::Down => Orientation::Vertical,
        }
    }

    fn forward(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    fn extent(self, orientation: Orientation) -> i32 {
        match orientation {
            Orientation::Horizontal => self.width,
            Orientation::Vertical => self.height,
        }
    }
}

/// A terminal, or a row or column of panes with their shares of the space.
#[derive(Clone, Debug, PartialEq)]
enum Pane {
    Terminal(TerminalId),
    Split {
        orientation: Orientation,
        parts: Vec<(Self, f64)>,
    },
}

impl Pane {
    fn split(orientation: Orientation, panes: Vec<Self>) -> Self {
        Self::Split {
            orientation,
            parts: panes.into_iter().map(|pane| (pane, 1.0)).collect(),
        }
    }

    /// The child indices that lead from this pane to the terminal.
    fn path_to(&self, terminal: TerminalId) -> Option<Vec<usize>> {
        match self {
            Self::Terminal(id) => (*id == terminal).then(Vec::new),
            Self::Split { parts, .. } => parts.iter().enumerate().find_map(|(index, (pane, _))| {
                let mut path = pane.path_to(terminal)?;
                path.insert(0, index);
                Some(path)
            }),
        }
    }

    fn at_mut(&mut self, path: &[usize]) -> Option<&mut Self> {
        match path.split_first() {
            None => Some(self),
            Some((first, rest)) => match self {
                Self::Split { parts, .. } => parts.get_mut(*first)?.0.at_mut(rest),
                Self::Terminal(_) => None,
            },
        }
    }

    /// Takes the terminal out and drops the rows and columns that became empty.
    fn remove(&mut self, terminal: TerminalId) {
        if let Self::Split { parts, .. } = self {
            parts.retain_mut(|(pane, _)| {
                pane.remove(terminal);
                match pane {
                    Self::Terminal(id) => *id != terminal,
                    Self::Split { parts, .. } => !parts.is_empty(),
                }
            });
        }
    }

    /// Places the terminals and the handles between panes into `rect`.
    fn place(&self, rect: Rect, path: &mut Vec<usize>, layout: &mut Layout) {
        let Self::Split { orientation, parts } = self else {
            if let Self::Terminal(id) = self {
                layout.terminals.push((*id, rect));
            }
            return;
        };
        let rects = divide(rect, *orientation, parts);
        for (index, ((pane, _), part)) in parts.iter().zip(&rects).enumerate() {
            path.push(index);
            pane.place(*part, path, layout);
            path.pop();
            if index + 1 < rects.len() {
                let handle = match orientation {
                    Orientation::Horizontal => Rect {
                        x: part.x + part.width,
                        width: HANDLE_WIDTH,
                        ..*part
                    },
                    Orientation::Vertical => Rect {
                        y: part.y + part.height,
                        height: HANDLE_WIDTH,
                        ..*part
                    },
                };
                layout.handles.push(Handle {
                    path: path.clone(),
                    index,
                    orientation: *orientation,
                    rect: handle,
                });
            }
        }
    }

    /// The area of the pane at `path`, if this pane fills `rect`.
    fn area_of(&self, rect: Rect, path: &[usize]) -> Option<Rect> {
        let Some((first, rest)) = path.split_first() else {
            return Some(rect);
        };
        let Self::Split { orientation, parts } = self else {
            return None;
        };
        let part = *divide(rect, *orientation, parts).get(*first)?;
        parts.get(*first)?.0.area_of(part, rest)
    }
}

/// Splits `rect` along `orientation` by the shares of `parts`, with handles in between. The
/// last part takes what rounding leaves over.
fn divide(rect: Rect, orientation: Orientation, parts: &[(Pane, f64)]) -> Vec<Rect> {
    let count = parts.len() as i32;
    let space = (rect.extent(orientation) - HANDLE_WIDTH * (count - 1)).max(0);
    let total: f64 = parts.iter().map(|(_, share)| share).sum();
    let mut offset = 0;
    let mut rects = Vec::with_capacity(parts.len());
    for (index, (_, share)) in parts.iter().enumerate() {
        let size = if index + 1 == parts.len() {
            (space - offset).max(0)
        } else if total > 0.0 {
            (f64::from(space) * share / total).floor() as i32
        } else {
            space / count
        };
        let start = offset + HANDLE_WIDTH * index as i32;
        rects.push(match orientation {
            Orientation::Horizontal => Rect {
                x: rect.x + start,
                width: size,
                ..rect
            },
            Orientation::Vertical => Rect {
                y: rect.y + start,
                height: size,
                ..rect
            },
        });
        offset += size;
    }
    rects
}

#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    /// Child indices leading from the session's pane to the row or column it belongs to.
    pub path: Vec<usize>,
    /// The handle separates part `index` and part `index + 1`.
    pub index: usize,
    pub orientation: Orientation,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub terminals: Vec<(TerminalId, Rect)>,
    pub handles: Vec<Handle>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalState {
    pub title: String,
    pub keyboard_input_enabled: bool,
    pub monitor_activity: bool,
    pub monitor_silence: bool,
    pub monitor_process: bool,
    /// A monitor switches itself off after its first alert.
    pub monitor_once: bool,
    /// Points added to the configured font size of this terminal.
    pub font_zoom: i32,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            title: String::new(),
            keyboard_input_enabled: true,
            monitor_activity: false,
            monitor_silence: false,
            monitor_process: false,
            monitor_once: false,
            font_zoom: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub id: SessionId,
    /// Always a row or column, even with one terminal, so that a split can turn it.
    pane: Pane,
    pub active_terminal: TerminalId,
    pub terminals: BTreeMap<TerminalId, TerminalState>,
    pub closable: bool,
    /// The title of the active terminal.
    pub title: String,
    /// Color of the tab, overriding the configured one.
    pub color: Option<String>,
}

impl Session {
    fn new(id: SessionId, kind: SessionType, next_terminal: &mut TerminalId) -> Self {
        let count = match kind {
            SessionType::Single => 1,
            SessionType::TwoHorizontal | SessionType::TwoVertical => 2,
            SessionType::Quad => 4,
        };
        let ids: Vec<TerminalId> = (0..count).map(|offset| *next_terminal + offset).collect();
        *next_terminal += count;
        let leaves = |range: std::ops::Range<usize>| -> Vec<Pane> {
            ids.get(range)
                .unwrap_or_default()
                .iter()
                .map(|id| Pane::Terminal(*id))
                .collect()
        };
        let pane = match kind {
            SessionType::Single | SessionType::TwoHorizontal => {
                Pane::split(Orientation::Horizontal, leaves(0..count as usize))
            }
            SessionType::TwoVertical => Pane::split(Orientation::Vertical, leaves(0..2)),
            SessionType::Quad => Pane::split(
                Orientation::Vertical,
                vec![
                    Pane::split(Orientation::Horizontal, leaves(0..2)),
                    Pane::split(Orientation::Horizontal, leaves(2..4)),
                ],
            ),
        };
        Self {
            id,
            pane,
            active_terminal: ids.first().copied().unwrap_or_default(),
            terminals: ids.iter().map(|id| (*id, TerminalState::default())).collect(),
            closable: true,
            title: String::new(),
            color: None,
        }
    }

    #[must_use]
    pub fn layout(&self, width: i32, height: i32) -> Layout {
        let mut layout = Layout::default();
        let area = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        self.pane.place(area, &mut Vec::new(), &mut layout);
        layout
    }

    #[must_use]
    pub fn terminal_ids(&self) -> Vec<TerminalId> {
        self.terminals.keys().copied().collect()
    }

    fn activate(&mut self, terminal: TerminalId) -> bool {
        let Some(state) = self.terminals.get(&terminal) else {
            return false;
        };
        self.title = state.title.clone();
        self.active_terminal = terminal;
        true
    }

    /// The terminal `step` places away from the active one in the order of ids, wrapping around.
    fn neighbour(&self, step: isize) -> Option<TerminalId> {
        let ids = self.terminal_ids();
        let position = ids.iter().position(|id| *id == self.active_terminal)? as isize;
        let count = ids.len() as isize;
        ids.get((position + step).rem_euclid(count) as usize).copied()
    }

    #[must_use]
    pub fn has_keyboard_input_disabled(&self) -> bool {
        self.terminals.values().any(|state| !state.keyboard_input_enabled)
    }

    #[must_use]
    pub fn has_keyboard_input_enabled(&self) -> bool {
        self.terminals.values().any(|state| state.keyboard_input_enabled)
    }

    #[must_use]
    pub fn has_monitor_activity_disabled(&self) -> bool {
        self.terminals.values().any(|state| !state.monitor_activity)
    }

    #[must_use]
    pub fn has_monitor_silence_disabled(&self) -> bool {
        self.terminals.values().any(|state| !state.monitor_silence)
    }

    /// Moves the handle after part `index` of the row or column at `path` by up to `delta`
    /// pixels, leaving both sides at least `MIN_EXTENT`. Returns the pixels it moved.
    fn shift_handle(&mut self, path: &[usize], index: usize, delta: i32, width: i32, height: i32) -> i32 {
        let area = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let Some(rect) = self.pane.area_of(area, path) else {
            return 0;
        };
        let Some(Pane::Split { orientation, parts }) = self.pane.at_mut(path) else {
            return 0;
        };
        let mut extents: Vec<i32> = divide(rect, *orientation, parts)
            .iter()
            .map(|part| part.extent(*orientation))
            .collect();
        let (Some(before), Some(after)) = (extents.get(index).copied(), extents.get(index + 1).copied()) else {
            return 0;
        };
        let lowest = (MIN_EXTENT - before).min(0);
        let highest = (after - MIN_EXTENT).max(0);
        let moved = delta.clamp(lowest, highest);
        if let Some(extent) = extents.get_mut(index) {
            *extent += moved;
        }
        if let Some(extent) = extents.get_mut(index + 1) {
            *extent -= moved;
        }
        for ((_, share), extent) in parts.iter_mut().zip(extents) {
            *share = f64::from(extent.max(0));
        }
        moved
    }
}

/// What the window does after a session was removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    /// The session does not exist.
    Nothing,
    /// This tab is active now.
    Selected(SessionId),
    /// The last tab closed; this new session replaces it.
    LastTabClosed(SessionId),
}

#[derive(Clone, Debug, Default)]
pub struct SessionStack {
    sessions: BTreeMap<SessionId, Session>,
    /// Session ids in the order of their tabs.
    tabs: Vec<SessionId>,
    tab_titles: BTreeMap<SessionId, String>,
    /// Tabs the user named; their titles do not follow the terminal.
    named_by_user: BTreeSet<SessionId>,
    active: Option<SessionId>,
    next_session: SessionId,
    next_terminal: TerminalId,
    /// Translated titles of the first tab and of further ones; the latter contains `%1`.
    tab_title_names: Option<(String, String)>,
}

impl SessionStack {
    pub fn set_tab_title_names(&mut self, first: String, numbered: String) {
        self.tab_title_names = Some((first, numbered));
    }

    /// The default title with the lowest number that no tab uses.
    fn free_title(&self) -> String {
        let (first, numbered) = self
            .tab_title_names
            .clone()
            .unwrap_or_else(|| ("Shell".to_string(), "Shell %1".to_string()));
        // One more number than there are tabs is always free.
        (1..=self.tab_titles.len() as u32 + 1)
            .map(|number: u32| {
                if number == 1 {
                    first.clone()
                } else {
                    numbered.replace("%1", &number.to_string())
                }
            })
            .find(|title| !self.tab_titles.values().any(|used| used == title))
            .unwrap_or(first)
    }

    #[must_use]
    pub fn active_id(&self) -> Option<SessionId> {
        self.active
    }

    #[must_use]
    pub fn active(&self) -> Option<&Session> {
        self.sessions.get(&self.active?)
    }

    fn active_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(&self.active?)
    }

    #[must_use]
    pub fn session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.get(&id)
    }

    pub fn session_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.get_mut(&id)
    }

    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values()
    }

    #[must_use]
    pub fn tabs(&self) -> &[SessionId] {
        &self.tabs
    }

    pub fn set_session_color(&mut self, session: SessionId, color: Option<String>) {
        if let Some(session) = self.sessions.get_mut(&session) {
            session.color = color;
        }
    }

    pub fn tab_title(&self, id: SessionId) -> Option<&str> {
        self.tab_titles.get(&id).map(String::as_str)
    }

    /// The session of the tab at `index`, or `-1`.
    #[must_use]
    pub fn session_at_tab(&self, index: i32) -> SessionId {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.tabs.get(index))
            .copied()
            .unwrap_or(-1)
    }

    #[must_use]
    pub fn session_for_terminal(&self, terminal: TerminalId) -> Option<SessionId> {
        self.sessions
            .values()
            .find(|session| session.terminals.contains_key(&terminal))
            .map(|session| session.id)
    }

    fn session_of_mut(&mut self, terminal: TerminalId) -> Option<&mut Session> {
        self.sessions
            .values_mut()
            .find(|session| session.terminals.contains_key(&terminal))
    }

    #[must_use]
    pub fn terminal_state(&self, terminal: TerminalId) -> Option<&TerminalState> {
        self.sessions
            .values()
            .find_map(|session| session.terminals.get(&terminal))
    }

    pub fn terminal_state_mut(&mut self, terminal: TerminalId) -> Option<&mut TerminalState> {
        self.sessions
            .values_mut()
            .find_map(|session| session.terminals.get_mut(&terminal))
    }

    /// All terminals of all sessions, by id.
    pub fn all_terminals(&self) -> Vec<TerminalId> {
        let mut ids: Vec<TerminalId> = self.sessions.values().flat_map(Session::terminal_ids).collect();
        ids.sort_unstable();
        ids
    }

    /// Opens a session in a new tab at the end and selects it.
    pub fn add_session(&mut self, kind: SessionType) -> SessionId {
        let id = self.next_session;
        self.next_session += 1;
        let title = self.free_title();
        let session = Session::new(id, kind, &mut self.next_terminal);
        self.sessions.insert(id, session);
        self.tabs.push(id);
        self.tab_titles.insert(id, title);
        self.active = Some(id);
        id
    }

    pub fn raise_session(&mut self, id: SessionId) -> bool {
        let exists = self.sessions.contains_key(&id);
        if exists {
            self.active = Some(id);
        }
        exists
    }

    /// Closes a session. If it was selected, the last tab is selected; if it was the only one,
    /// a new session takes its place.
    pub fn remove_session(&mut self, id: SessionId) -> Removal {
        if self.sessions.remove(&id).is_none() {
            return Removal::Nothing;
        }
        self.tabs.retain(|tab| *tab != id);
        self.tab_titles.remove(&id);
        self.named_by_user.remove(&id);
        if self.tabs.is_empty() {
            return Removal::LastTabClosed(self.add_session(SessionType::Single));
        }
        if self.active == Some(id) {
            self.active = self.tabs.last().copied();
        }
        self.active.map_or(Removal::Nothing, Removal::Selected)
    }

    /// Closes a terminal and selects the one with the next lower id. Returns the session if the
    /// terminal was its last one, which leaves closing the whole session to the caller.
    pub fn close_terminal(&mut self, terminal: TerminalId) -> Option<SessionId> {
        let session = self.session_of_mut(terminal)?;
        if session.terminals.len() == 1 {
            return Some(session.id);
        }
        if session.active_terminal == terminal
            && let Some(previous) = session.neighbour(-1)
        {
            session.activate(previous);
        }
        session.pane.remove(terminal);
        session.terminals.remove(&terminal);
        None
    }

    pub fn focus_previous_terminal(&mut self) -> Option<TerminalId> {
        let session = self.active_mut()?;
        let previous = session.neighbour(-1)?;
        session.activate(previous);
        Some(previous)
    }

    pub fn focus_next_terminal(&mut self) -> Option<TerminalId> {
        let session = self.active_mut()?;
        let next = session.neighbour(1)?;
        session.activate(next);
        Some(next)
    }

    pub fn set_active_terminal(&mut self, terminal: TerminalId) -> bool {
        self.session_of_mut(terminal)
            .is_some_and(|session| session.activate(terminal))
    }

    /// Opens a terminal next to `terminal`, which gives up half of its space, and selects it.
    pub fn split(&mut self, terminal: TerminalId, orientation: Orientation) -> Option<TerminalId> {
        let new = self.next_terminal;
        let session = self.session_of_mut(terminal)?;
        let path = session.pane.path_to(terminal)?;
        let (index, parent_path) = path.split_last()?;
        let Some(Pane::Split {
            orientation: parent_orientation,
            parts,
        }) = session.pane.at_mut(parent_path)
        else {
            return None;
        };
        if parts.len() == 1 {
            *parent_orientation = orientation;
        }
        if *parent_orientation == orientation {
            let share = parts.get(*index)?.1 / 2.0;
            if let Some(part) = parts.get_mut(*index) {
                part.1 = share;
            }
            parts.insert(index + 1, (Pane::Terminal(new), share));
        } else {
            let part = parts.get_mut(*index)?;
            part.0 = Pane::split(orientation, vec![Pane::Terminal(terminal), Pane::Terminal(new)]);
        }
        session.terminals.insert(new, TerminalState::default());
        session.activate(new);
        self.next_terminal += 1;
        Some(new)
    }

    /// Splits along the longer side of the terminal.
    pub fn split_auto(&mut self, terminal: TerminalId, width: i32, height: i32) -> Option<TerminalId> {
        let session = self.session(self.session_for_terminal(terminal)?)?;
        let (_, rect) = session
            .layout(width, height)
            .terminals
            .into_iter()
            .find(|(id, _)| *id == terminal)?;
        let orientation = if rect.width > rect.height {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        };
        self.split(terminal, orientation)
    }

    /// Makes a terminal larger towards `direction` by moving the nearest handle on that side.
    /// Returns the pixels it grew, or -1 if it could not grow.
    pub fn try_grow(
        &mut self,
        terminal: TerminalId,
        direction: Direction,
        pixels: i32,
        width: i32,
        height: i32,
    ) -> i32 {
        let Some(session) = self.session_of_mut(terminal) else {
            return -1;
        };
        let Some(path) = session.pane.path_to(terminal) else {
            return -1;
        };
        let pixels = pixels.max(0);
        // From the terminal's own row or column outwards.
        for depth in (0..path.len()).rev() {
            let (parent_path, rest) = path.split_at(depth);
            let Some(index) = rest.first().copied() else {
                continue;
            };
            let Some(Pane::Split { orientation, parts }) = session.pane.at_mut(parent_path) else {
                continue;
            };
            if *orientation != direction.orientation() {
                continue;
            }
            let (handle, delta) = if direction.forward() {
                if index + 1 >= parts.len() {
                    continue;
                }
                (index, pixels)
            } else {
                let Some(before) = index.checked_sub(1) else {
                    continue;
                };
                (before, -pixels)
            };
            let moved = session.shift_handle(parent_path, handle, delta, width, height).abs();
            return if moved > 0 { moved } else { -1 };
        }
        -1
    }

    pub fn drag_handle(&mut self, handle: &Handle, delta: i32, width: i32, height: i32) {
        if let Some(session) = self.active_mut() {
            session.shift_handle(&handle.path, handle.index, delta, width, height);
        }
    }

    pub fn zoom_terminal(&mut self, terminal: TerminalId, delta: i32) {
        if let Some(state) = self.terminal_state_mut(terminal) {
            state.font_zoom = state
                .font_zoom
                .saturating_add(delta)
                .clamp(MIN_FONT_ZOOM, MAX_FONT_ZOOM);
        }
    }

    pub fn reset_zoom(&mut self, terminal: TerminalId) {
        if let Some(state) = self.terminal_state_mut(terminal) {
            state.font_zoom = 0;
        }
    }

    /// Stores what the program in a terminal calls itself. Returns the terminal's session.
    pub fn set_terminal_title(&mut self, terminal: TerminalId, title: &str) -> Option<SessionId> {
        let session = self.session_of_mut(terminal)?;
        if let Some(state) = session.terminals.get_mut(&terminal) {
            state.title = title.to_string();
        }
        if session.active_terminal == terminal {
            session.title = title.to_string();
        }
        Some(session.id)
    }

    /// Names a tab. A name the user gives sticks until the user clears it, which brings back a
    /// default name; until then, titles from the terminal do not replace it.
    pub fn set_tab_title(&mut self, id: SessionId, title: &str, interactive: bool) {
        if !self.sessions.contains_key(&id) {
            return;
        }
        if interactive {
            if title.is_empty() {
                self.named_by_user.remove(&id);
                self.tab_titles.remove(&id);
                let title = self.free_title();
                self.tab_titles.insert(id, title);
            } else {
                self.named_by_user.insert(id);
                self.tab_titles.insert(id, title.to_string());
            }
        } else if !self.named_by_user.contains(&id) && !title.is_empty() {
            self.tab_titles.insert(id, title.to_string());
        }
    }

    /// Selects the tab `step` places away from the selected one, wrapping around.
    fn select_relative(&mut self, step: isize) -> Option<SessionId> {
        let position = self.tabs.iter().position(|tab| Some(*tab) == self.active)? as isize;
        let count = self.tabs.len() as isize;
        let id = *self.tabs.get((position + step).rem_euclid(count) as usize)?;
        self.active = Some(id);
        Some(id)
    }

    pub fn select_next_tab(&mut self) -> Option<SessionId> {
        self.select_relative(1)
    }

    pub fn select_previous_tab(&mut self) -> Option<SessionId> {
        self.select_relative(-1)
    }

    pub fn move_tab_left(&mut self, id: SessionId) {
        if let Some(position) = self.tabs.iter().position(|tab| *tab == id)
            && position > 0
        {
            self.tabs.swap(position, position - 1);
        }
    }

    pub fn move_tab_right(&mut self, id: SessionId) {
        if let Some(position) = self.tabs.iter().position(|tab| *tab == id)
            && position + 1 < self.tabs.len()
        {
            self.tabs.swap(position, position + 1);
        }
    }

    /// Moves a tab to where it was dropped: before the tab at `target`, as counted before the
    /// move, or to the end.
    pub fn drop_tab(&mut self, id: SessionId, target: Option<usize>) {
        let Some(from) = self.tabs.iter().position(|tab| *tab == id) else {
            return;
        };
        self.tabs.remove(from);
        let to = match target {
            Some(target) if target > from => target - 1,
            Some(target) => target,
            None => self.tabs.len(),
        };
        self.tabs.insert(to.min(self.tabs.len()), id);
    }

    #[must_use]
    pub fn has_unclosable_sessions(&self) -> bool {
        self.sessions.values().any(|session| !session.closable)
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.sessions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack() -> SessionStack {
        let mut stack = SessionStack::default();
        stack.add_session(SessionType::Single);
        stack
    }

    fn rect_of(stack: &SessionStack, session: SessionId, terminal: TerminalId, width: i32, height: i32) -> Rect {
        let layout = stack.session(session).unwrap().layout(width, height);
        layout.terminals.into_iter().find(|(id, _)| *id == terminal).unwrap().1
    }

    #[test]
    fn ids_and_titles_count_up() {
        let mut stack = stack();
        stack.add_session(SessionType::Quad);
        let third = stack.add_session(SessionType::Single);
        assert_eq!(stack.tab_title(0), Some("Shell"));
        assert_eq!(stack.tab_title(1), Some("Shell 2"));
        assert_eq!(stack.tab_title(2), Some("Shell 3"));
        assert_eq!(stack.session(1).unwrap().terminal_ids(), vec![1, 2, 3, 4]);
        assert_eq!(stack.session(2).unwrap().terminal_ids(), vec![5]);
        assert_eq!(stack.active_id(), Some(third));
    }

    #[test]
    fn tab_titles_can_be_translated() {
        let mut stack = SessionStack::default();
        stack.set_tab_title_names("Konsole".to_string(), "Konsole Nr. %1".to_string());
        stack.add_session(SessionType::Single);
        let second = stack.add_session(SessionType::Single);
        assert_eq!(stack.tab_title(0), Some("Konsole"));
        assert_eq!(stack.tab_title(second), Some("Konsole Nr. 2"));
    }

    #[test]
    fn free_titles_get_reused() {
        let mut stack = stack();
        let second = stack.add_session(SessionType::Single);
        stack.add_session(SessionType::Single);
        stack.remove_session(second);
        let fourth = stack.add_session(SessionType::Single);
        assert_eq!(stack.tab_title(fourth), Some("Shell 2"));
    }

    #[test]
    fn removing_the_active_tab_selects_the_last_one() {
        let mut stack = stack();
        stack.add_session(SessionType::Single);
        stack.add_session(SessionType::Single);
        stack.raise_session(1);
        assert_eq!(stack.remove_session(1), Removal::Selected(2));
    }

    #[test]
    fn removing_another_tab_keeps_the_selection() {
        let mut stack = stack();
        stack.add_session(SessionType::Single);
        stack.add_session(SessionType::Single);
        stack.raise_session(0);
        assert_eq!(stack.remove_session(1), Removal::Selected(0));
        assert_eq!(stack.remove_session(7), Removal::Nothing);
    }

    #[test]
    fn closing_the_last_tab_creates_a_new_session() {
        let mut stack = stack();
        assert_eq!(stack.remove_session(0), Removal::LastTabClosed(1));
        assert_eq!(stack.tab_title(1), Some("Shell"));
    }

    #[test]
    fn quad_layout_divides_the_area_with_one_pixel_lines() {
        let mut stack = stack();
        let id = stack.add_session(SessionType::Quad);
        let layout = stack.session(id).unwrap().layout(1149, 345);
        assert_eq!(layout.terminals.len(), 4);
        assert_eq!(layout.handles.len(), 3);
        let (first, second) = (layout.terminals[0].1, layout.terminals[1].1);
        assert_eq!(first.x + first.width + HANDLE_WIDTH, second.x);
        assert_eq!(second.x + second.width, 1149);
        let lower = layout.terminals[2].1;
        assert_eq!(first.height + HANDLE_WIDTH + lower.height, 345);
    }

    #[test]
    fn splitting_a_single_terminal_follows_the_orientation() {
        let mut stack = stack();
        let new = stack.split(0, Orientation::Vertical).unwrap();
        assert_eq!(stack.session(0).unwrap().active_terminal, new);
        let (top, bottom) = (rect_of(&stack, 0, 0, 800, 401), rect_of(&stack, 0, new, 800, 401));
        assert_eq!((top.width, bottom.width), (800, 800));
        assert_eq!(top.y + top.height + HANDLE_WIDTH, bottom.y);
    }

    #[test]
    fn a_second_split_divides_only_the_terminal_split() {
        let mut stack = stack();
        let right = stack.split(0, Orientation::Horizontal).unwrap();
        let below = stack.split(0, Orientation::Vertical).unwrap();
        let (size, half) = (rect_of(&stack, 0, right, 1001, 401), rect_of(&stack, 0, 0, 1001, 401));
        assert_eq!(size.height, 401, "the right terminal keeps the full height");
        assert_eq!(half.width, 500);
        assert_eq!(rect_of(&stack, 0, below, 1001, 401).x, 0);
    }

    #[test]
    fn splitting_in_the_same_direction_halves_the_terminal() {
        let mut stack = stack();
        let second = stack.split(0, Orientation::Horizontal).unwrap();
        let third = stack.split(second, Orientation::Horizontal).unwrap();
        assert_eq!(rect_of(&stack, 0, 0, 1002, 100).width, 500);
        assert_eq!(rect_of(&stack, 0, second, 1002, 100).width, 250);
        assert_eq!(rect_of(&stack, 0, third, 1002, 100).width, 250);
    }

    #[test]
    fn closing_terminals_gives_back_the_space_and_activates_the_previous_id() {
        let mut stack = stack();
        stack.split(0, Orientation::Horizontal);
        let third = stack.split(1, Orientation::Vertical).unwrap();
        assert_eq!(stack.close_terminal(third), None);
        assert_eq!(stack.session(0).unwrap().active_terminal, 1);
        assert_eq!(stack.close_terminal(1), None);
        assert_eq!(stack.session(0).unwrap().active_terminal, 0);
        assert_eq!(
            rect_of(&stack, 0, 0, 600, 300),
            Rect {
                x: 0,
                y: 0,
                width: 600,
                height: 300
            }
        );
        assert_eq!(stack.close_terminal(0), Some(0));
    }

    #[test]
    fn focus_cycles_through_terminals_by_id() {
        let mut stack = stack();
        let id = stack.add_session(SessionType::Quad);
        assert_eq!(stack.session(id).unwrap().active_terminal, 1);
        assert_eq!(stack.focus_previous_terminal(), Some(4));
        assert_eq!(stack.focus_next_terminal(), Some(1));
        assert_eq!(stack.focus_next_terminal(), Some(2));
    }

    #[test]
    fn grow_moves_the_shared_handle_and_reports_the_change() {
        let mut stack = stack();
        stack.split(0, Orientation::Horizontal);
        let before = rect_of(&stack, 0, 0, 1001, 400).width;
        assert_eq!(stack.try_grow(0, Direction::Right, 10, 1001, 400), 10);
        assert_eq!(rect_of(&stack, 0, 0, 1001, 400).width, before + 10);
        assert_eq!(stack.try_grow(1, Direction::Left, 4, 1001, 400), 4);
        assert_eq!(rect_of(&stack, 0, 0, 1001, 400).width, before + 6);
    }

    #[test]
    fn grow_without_a_neighbour_fails() {
        let mut stack = stack();
        stack.split(0, Orientation::Horizontal);
        assert_eq!(stack.try_grow(0, Direction::Left, 10, 1001, 400), -1);
        assert_eq!(stack.try_grow(0, Direction::Up, 10, 1001, 400), -1);
    }

    #[test]
    fn grow_walks_up_to_a_matching_splitter() {
        let mut stack = stack();
        let id = stack.add_session(SessionType::Quad);
        // Terminal 1 sits in the upper row; "down" needs the column that holds both rows.
        let before = rect_of(&stack, id, 1, 1000, 401).height;
        assert_eq!(stack.try_grow(1, Direction::Down, 10, 1000, 401), 10);
        assert_eq!(rect_of(&stack, id, 1, 1000, 401).height, before + 10);
    }

    #[test]
    fn zoom_only_changes_one_terminal() {
        let mut stack = stack();
        let second = stack.split(0, Orientation::Horizontal).unwrap();
        stack.zoom_terminal(second, 2);
        assert_eq!(stack.terminal_state(second).unwrap().font_zoom, 2);
        assert_eq!(stack.terminal_state(0).unwrap().font_zoom, 0);
        stack.reset_zoom(second);
        assert_eq!(stack.terminal_state(second).unwrap().font_zoom, 0);
    }

    #[test]
    fn zoom_stays_within_its_limits() {
        let mut stack = stack();
        stack.zoom_terminal(0, 1000);
        assert_eq!(stack.terminal_state(0).unwrap().font_zoom, MAX_FONT_ZOOM);
        stack.zoom_terminal(0, -1000);
        assert_eq!(stack.terminal_state(0).unwrap().font_zoom, MIN_FONT_ZOOM);
    }

    #[test]
    fn tab_navigation_wraps_around() {
        let mut stack = stack();
        stack.add_session(SessionType::Single);
        assert_eq!(stack.select_next_tab(), Some(0));
        assert_eq!(stack.select_previous_tab(), Some(1));
    }

    #[test]
    fn automated_titles_do_not_replace_interactive_ones() {
        let mut stack = stack();
        stack.set_tab_title(0, "mine", true);
        stack.set_tab_title(0, "auto", false);
        assert_eq!(stack.tab_title(0), Some("mine"));
        stack.set_tab_title(0, "", true);
        assert_eq!(stack.tab_title(0), Some("Shell"));
        stack.set_tab_title(0, "auto", false);
        assert_eq!(stack.tab_title(0), Some("auto"));
    }

    #[test]
    fn dropping_a_tab_follows_the_index_correction() {
        let mut stack = stack();
        stack.add_session(SessionType::Single);
        stack.add_session(SessionType::Single);
        stack.drop_tab(0, None);
        assert_eq!(stack.tabs(), &[1, 2, 0]);
        stack.drop_tab(0, Some(1));
        assert_eq!(stack.tabs(), &[1, 0, 2]);
        stack.drop_tab(2, Some(0));
        assert_eq!(stack.tabs(), &[2, 1, 0]);
    }

    #[test]
    fn tabs_move_by_one_and_stop_at_the_ends() {
        let mut stack = stack();
        stack.add_session(SessionType::Single);
        stack.move_tab_left(0);
        assert_eq!(stack.tabs(), &[0, 1]);
        stack.move_tab_right(0);
        assert_eq!(stack.tabs(), &[1, 0]);
        assert_eq!(stack.session_at_tab(1), 0);
        assert_eq!(stack.session_at_tab(5), -1);
    }

    #[test]
    fn dragging_a_handle_respects_the_minimum_size() {
        let mut stack = stack();
        stack.split(0, Orientation::Horizontal);
        let handle = stack.session(0).unwrap().layout(1001, 400).handles[0].clone();
        stack.drag_handle(&handle, -5000, 1001, 400);
        assert_eq!(rect_of(&stack, 0, 0, 1001, 400).width, MIN_EXTENT);
    }

    #[test]
    fn automatic_splits_follow_the_longer_side() {
        let mut stack = stack();
        let wide = stack.split_auto(0, 600, 300).unwrap();
        assert_eq!(rect_of(&stack, 0, wide, 600, 300).height, 300);
        let tall = stack.split_auto(wide, 600, 300).unwrap();
        assert!(rect_of(&stack, 0, tall, 600, 300).height < 300);
    }

    #[test]
    fn terminal_titles_reach_the_session_when_active() {
        let mut stack = stack();
        let second = stack.split(0, Orientation::Horizontal).unwrap();
        assert_eq!(stack.set_terminal_title(0, "vim"), Some(0));
        assert_eq!(stack.session(0).unwrap().title, "");
        stack.set_terminal_title(second, "top");
        assert_eq!(stack.session(0).unwrap().title, "top");
        assert!(stack.set_active_terminal(0));
        assert_eq!(stack.session(0).unwrap().title, "vim");
    }
}
