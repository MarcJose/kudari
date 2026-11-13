// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The application without its window: sessions and their terminals, the monitors, the bell,
//! the window state and the questions to the user. A front end calls [`App`] for what the
//! user and the terminals do, and carries out the [`Effect`]s it returns, in their order.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent::{self, Access};
use crate::i18n::{tr, tr_arg};
use crate::sessions::{
    Direction, Layout, Orientation, Rect, Removal, SessionId, SessionStack, SessionType, TerminalId,
};
use crate::settings::{Settings, Store};

/// Bell notifications are at most this frequent, so that a burst of bells is one notification.
const BELL_INTERVAL: Duration = Duration::from_millis(500);

/// How often the front end calls [`App::poll`] per second, for the auto save interval.
pub const POLLS_PER_SECOND: u32 = 2;

/// What the app needs to ask the front end about.
pub trait Host {
    /// The working directory of the program in a terminal.
    fn working_directory(&self, terminal: TerminalId) -> Option<PathBuf>;
    /// Whether a window of the application, such as a menu or dialog, has the focus.
    fn has_focus(&self) -> bool;
    /// A terminal whose program runs, for AI agents.
    fn terminal(&self, terminal: TerminalId) -> Option<Arc<dyn agent::Terminal>>;
}

/// What the front end does with the program of a terminal, for an action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalCommand {
    /// Asks for the number of lines of scrollback.
    AdjustHistory,
    /// Lets the program see the mouse, or not.
    MouseTracking(bool),
    /// Opens the working directory in the file manager.
    OpenFolder,
    /// Searches the web for the selected text.
    WebSearch,
    /// Clears the scrollback and resets the terminal.
    Reset,
    /// Asks for a file and saves the output there.
    SaveOutput,
    Print,
    /// Asks for a file that the output goes to from now on, or stops writing it.
    AutoSave(bool),
    /// Pastes text the user confirmed, prepared for the mode the terminal is in by then.
    Paste(String),
}

/// The button that confirms a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    Quit,
    Close,
    Continue,
}

/// A question to the user. The front end answers it with [`App::answer`] and its `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub id: u32,
    pub title: String,
    pub text: String,
    pub confirm: Confirm,
}

/// What a question was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    CloseSession(SessionId),
    CloseTerminal(SessionId),
    Quit,
    Paste { terminal: TerminalId, text: String },
    Import,
}

/// Something the front end does, in the order of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// The sessions, tabs or layout changed: start new terminals and show the model.
    Publish,
    /// The settings changed: show them.
    PublishSettings,
    /// The state of the session actions may have changed.
    UpdateActions,
    /// Ends the program of a terminal that is gone.
    Stop(TerminalId),
    Repaint(TerminalId),
    Focus(TerminalId),
    Highlight {
        terminal: TerminalId,
        /// Stays until [`Effect::RemoveHighlight`], instead of fading.
        persistent: bool,
    },
    RemoveHighlight,
    Notify {
        event: &'static str,
        text: String,
    },
    Ask(Question),
    Quit,
    /// Measures the screen the window opens on.
    UpdateWorkArea,
    WindowVisible(bool),
    /// Hides the window and shows it right away, which gives it the keyboard where asking for
    /// the focus would be ignored.
    Refocus,
    Fullscreen(bool),
    /// The time of the last output starts again, for the silence monitor.
    RestartSilence(TerminalId),
    /// Writes the output of every terminal that saves it automatically.
    AutoSave,
    RenameTab(SessionId),
    FirstRun,
    /// Opens the settings, at a page or at the first one.
    OpenSettings(Option<&'static str>),
    /// An action of the view of a terminal, such as copying or searching.
    TerminalAction {
        terminal: TerminalId,
        action: String,
    },
    /// Something only the program of a terminal can do.
    Terminal {
        terminal: TerminalId,
        command: TerminalCommand,
    },
    /// Takes over the settings of Yakuake and Konsole.
    Import,
    /// Asks the user whether an AI agent may use Kudari, with the text of
    /// [`App::agent_question`], and passes the answer to [`App::agent_answered`].
    AskAgent(String),
    /// Withdraws the question about an agent.
    CloseAgentQuestion,
    /// Shows whether any AI agent may use Kudari now.
    AgentAccess(bool),
}

/// A monitor of a terminal, or its keyboard lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Monitor {
    /// Locks the keyboard input; checking it means the terminal ignores keys.
    Keyboard,
    Activity,
    Silence,
    /// The program in the foreground ended.
    Process,
    /// The monitors switch themselves off after their first alert.
    Once,
}

impl Monitor {
    /// The monitor of a name the menus and the D-Bus interface use.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "keyboard" => Self::Keyboard,
            "activity" => Self::Activity,
            "silence" => Self::Silence,
            "process" => Self::Process,
            "once" => Self::Once,
            _ => return None,
        })
    }
}

/// A tab of the tab bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub id: SessionId,
    pub title: String,
    pub selected: bool,
    pub closable: bool,
    pub color: String,
    /// A terminal of this background tab had output.
    pub activity: bool,
}

/// What the front end shows of the sessions.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub tabs: Vec<Tab>,
    /// The terminals and splitters of the active session, with its active terminal.
    pub layout: Option<(Layout, TerminalId)>,
    pub title: String,
    pub terminals: Vec<TerminalId>,
    pub keyboard_disabled: Vec<TerminalId>,
    /// Terminals with a font size of their own, and its difference.
    pub font_zoom: Vec<(TerminalId, i32)>,
}

/// A terminal to start, in the area of its pane.
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub terminal: TerminalId,
    pub rect: Rect,
    pub directory: Option<PathBuf>,
    /// The shell chosen for the terminal, by the id of [`crate::shells::Shell`].
    pub shell: Option<String>,
}

/// What a poll learned about a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub terminal: TerminalId,
    pub title: String,
    /// A program other than the shell runs in the foreground.
    pub busy: bool,
    /// Time since the last output.
    pub quiet_for: Duration,
}

pub struct App {
    stack: SessionStack,
    settings: Settings,
    area: (i32, i32),
    cell: (f64, f64),
    /// The tab a context menu was opened on; its actions apply to that session.
    menu_session: Option<SessionId>,
    /// Where new terminals start.
    pending_directories: Vec<(TerminalId, Option<PathBuf>)>,
    /// The shells chosen for terminals; a split starts the shell of the terminal it splits.
    shells: BTreeMap<TerminalId, String>,
    running: BTreeSet<TerminalId>,
    activity_reported: BTreeSet<TerminalId>,
    /// Background tabs with output since they were last shown.
    tab_activity: BTreeSet<SessionId>,
    silence_reported: BTreeSet<TerminalId>,
    busy: BTreeSet<TerminalId>,
    last_bell: Option<Instant>,
    /// A dialog just closed, which moves the focus away: that must not retract the window.
    focus_toggle_lock: bool,
    /// The window is only being shown again to get the focus, without animation.
    refocusing: bool,
    polls: u32,
    /// Sessions an AI agent opened, the only ones it may close.
    /// The sessions agents opened, with the name of the agent that opened each.
    agent_sessions: BTreeMap<SessionId, String>,
    /// Which AI agents the user allowed, denied or is being asked about.
    agents: Access,
    window_visible: bool,
    fullscreen: bool,
    questions: BTreeMap<u32, Pending>,
    next_question: u32,
}

impl App {
    /// The app with these settings. Tabs get their default titles in the language of the
    /// translator, so it has to be installed first.
    #[must_use]
    pub fn new(settings: Settings) -> Self {
        let mut stack = SessionStack::default();
        stack.set_tab_title_names(tr("@title:tab", "Shell"), tr_arg("@title:tab", "Shell %1", "%1"));
        Self {
            stack,
            settings,
            area: (0, 0),
            cell: (1.0, 1.0),
            menu_session: None,
            pending_directories: Vec::new(),
            shells: BTreeMap::new(),
            running: BTreeSet::new(),
            activity_reported: BTreeSet::new(),
            tab_activity: BTreeSet::new(),
            silence_reported: BTreeSet::new(),
            busy: BTreeSet::new(),
            last_bell: None,
            focus_toggle_lock: false,
            refocusing: false,
            polls: 0,
            agent_sessions: BTreeMap::new(),
            agents: Access::default(),
            window_visible: false,
            fullscreen: false,
            questions: BTreeMap::new(),
            next_question: 0,
        }
    }

    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Takes new settings. Returns the old ones, to compare.
    pub fn replace_settings(&mut self, settings: Settings) -> Settings {
        std::mem::replace(&mut self.settings, settings)
    }

    #[must_use]
    pub fn stack(&self) -> &SessionStack {
        &self.stack
    }

    pub fn stack_mut(&mut self) -> &mut SessionStack {
        &mut self.stack
    }

    /// The area of the terminals in pixels.
    #[must_use]
    pub fn area(&self) -> (i32, i32) {
        self.area
    }

    #[must_use]
    pub fn window_visible(&self) -> bool {
        self.window_visible
    }

    #[must_use]
    pub fn fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// The session the actions of a menu apply to: the one of the tab menu, or the active one.
    #[must_use]
    pub fn context_session(&self) -> Option<SessionId> {
        self.menu_session.or(self.stack.active_id())
    }

    #[must_use]
    pub fn active_terminal(&self) -> Option<TerminalId> {
        self.stack.active().map(|session| session.active_terminal)
    }

    /// Remembers that `agent` opened `session`.
    pub fn add_agent_session(&mut self, session: SessionId, agent: &str) {
        self.agent_sessions.insert(session, agent.to_string());
    }

    /// Whether an agent opened `session`.
    #[must_use]
    pub fn is_agent_session(&self, session: SessionId) -> bool {
        self.agent_sessions.contains_key(&session)
    }

    /// The agent that opened `session`.
    #[must_use]
    pub fn agent_session_owner(&self, session: SessionId) -> Option<&str> {
        self.agent_sessions.get(&session).map(String::as_str)
    }

    #[must_use]
    pub fn agents(&self) -> &Access {
        &self.agents
    }

    pub(crate) fn agents_mut(&mut self) -> &mut Access {
        &mut self.agents
    }

    /// Opens the first session, and the window if this is the first start or the settings
    /// ask for it.
    pub fn start(&mut self) -> Vec<Effect> {
        self.stack.add_session(SessionType::Single);
        let mut effects = vec![Effect::Publish];
        if self.settings.first_run {
            effects.extend(self.toggle_window(true));
            effects.push(Effect::FirstRun);
        } else if self.settings.open_after_start {
            effects.extend(self.toggle_window(true));
        }
        effects
    }

    /// The area for terminals and the size of their cells changed.
    pub fn set_geometry(&mut self, width: i32, height: i32, cell_width: f64, cell_height: f64) -> Vec<Effect> {
        self.area = (width, height);
        self.cell = (cell_width.max(1.0), cell_height.max(1.0));
        vec![Effect::Publish]
    }

    /// The size of a cell, in pixels.
    #[must_use]
    pub fn cell(&self) -> (f64, f64) {
        self.cell
    }

    /// The terminals that have no program yet, in the area of their panes. Empty while the
    /// window has no size, since the size of a terminal decides how its program starts.
    #[must_use]
    pub fn pending_starts(&self) -> Vec<Start> {
        let (width, height) = self.area;
        if width <= 0 || height <= 0 {
            return Vec::new();
        }
        self.stack
            .sessions()
            .flat_map(|session| session.layout(width, height).terminals)
            .filter(|(terminal, _)| !self.running.contains(terminal))
            .map(|(terminal, rect)| Start {
                terminal,
                rect,
                directory: self
                    .pending_directories
                    .iter()
                    .find(|(pending, _)| *pending == terminal)
                    .and_then(|(_, directory)| directory.clone())
                    .or_else(crate::process::home),
                shell: self.shells.get(&terminal).cloned(),
            })
            .collect()
    }

    /// The front end started the programs of these terminals; the others failed and are
    /// tried again with the next [`Effect::Publish`].
    pub fn started(&mut self, terminals: &[TerminalId]) {
        self.running.extend(terminals);
        self.pending_directories.clear();
        let stack = &self.stack;
        self.shells
            .retain(|terminal, _| stack.session_for_terminal(*terminal).is_some());
    }

    /// What the front end shows. Showing the active tab clears its activity.
    pub fn model(&mut self) -> Model {
        let active = self.stack.active_id();
        if let Some(active) = active {
            self.tab_activity.remove(&active);
        }
        let tabs = self
            .stack
            .tabs()
            .iter()
            .filter_map(|id| {
                let session = self.stack.session(*id)?;
                Some(Tab {
                    id: *id,
                    title: self.stack.tab_title(*id).unwrap_or_default().to_string(),
                    selected: Some(*id) == active,
                    closable: session.closable,
                    color: session.color.clone().unwrap_or_else(|| self.settings.tab_color.clone()),
                    activity: self.tab_activity.contains(id),
                })
            })
            .collect();
        let (width, height) = self.area;
        let terminal_states = || self.stack.sessions().flat_map(|session| session.terminals.iter());
        Model {
            tabs,
            layout: self
                .stack
                .active()
                .map(|session| (session.layout(width, height), session.active_terminal)),
            title: self
                .stack
                .active()
                .map(|session| session.title.clone())
                .unwrap_or_default(),
            terminals: self.stack.all_terminals(),
            keyboard_disabled: terminal_states()
                .filter(|(_, state)| !state.keyboard_input_enabled)
                .map(|(id, _)| *id)
                .collect(),
            font_zoom: terminal_states()
                .filter(|(_, state)| state.font_zoom != 0)
                .map(|(id, state)| (*id, state.font_zoom))
                .collect(),
        }
    }

    /// The state of the actions for the context session, as name, checked and enabled.
    #[must_use]
    pub fn session_actions(&self) -> Vec<(&'static str, bool, bool)> {
        let session_id = self.context_session();
        let tabs = self.stack.tabs();
        let index = tabs.iter().position(|tab| Some(*tab) == session_id);
        let mut actions = vec![
            ("move-session-left", false, index.is_some_and(|index| index > 0)),
            (
                "move-session-right",
                false,
                index.is_some_and(|index| index + 1 < tabs.len()),
            ),
        ];
        if let Some(session) = session_id.and_then(|id| self.stack.session(id)) {
            actions.extend([
                ("toggle-session-prevent-closing", !session.closable, true),
                (
                    "toggle-session-keyboard-input",
                    !session.has_keyboard_input_enabled(),
                    true,
                ),
                (
                    "toggle-session-monitor-activity",
                    !session.has_monitor_activity_disabled(),
                    true,
                ),
                (
                    "toggle-session-monitor-silence",
                    !session.has_monitor_silence_disabled(),
                    true,
                ),
                (
                    "toggle-session-monitor-process",
                    session.terminals.values().all(|state| state.monitor_process),
                    true,
                ),
                (
                    "toggle-session-monitor-once",
                    session.terminals.values().all(|state| state.monitor_once),
                    true,
                ),
            ]);
        }
        actions
    }

    fn focus_active(&self) -> Vec<Effect> {
        self.active_terminal().map(Effect::Focus).into_iter().collect()
    }

    /// Publishes the model and gives the active terminal the keyboard.
    #[must_use]
    pub fn refresh(&self) -> Vec<Effect> {
        let mut effects = vec![Effect::Publish];
        effects.extend(self.focus_active());
        effects
    }

    /// Where a terminal opened from `terminal` starts: in its directory, if the settings say
    /// so and it is known, else in the home directory. Not in the directory of Kudari, which
    /// is `System32` when Windows starts it.
    fn start_directory(&self, terminal: TerminalId, host: &dyn Host) -> Option<PathBuf> {
        self.settings
            .inherit_directory
            .then(|| host.working_directory(terminal))
            .flatten()
            .or_else(crate::process::home)
    }

    /// Where a terminal opened from the active one starts, the first one in the home directory.
    #[must_use]
    pub fn active_directory(&self, host: &dyn Host) -> Option<PathBuf> {
        match self.active_terminal() {
            Some(terminal) => self.start_directory(terminal, host),
            None => crate::process::home(),
        }
    }

    /// Opens a session whose terminals start in the directory of the active terminal.
    pub fn add_session(&mut self, kind: SessionType, host: &dyn Host) -> (SessionId, Vec<Effect>) {
        self.add_session_with_shell(kind, None, host)
    }

    /// Opens a session whose terminals start `shell`, an id of [`crate::shells::Shell`], or
    /// the shell of the settings.
    pub fn add_session_with_shell(
        &mut self,
        kind: SessionType,
        shell: Option<&str>,
        host: &dyn Host,
    ) -> (SessionId, Vec<Effect>) {
        let directory = self.active_directory(host);
        let id = self.stack.add_session(kind);
        let terminals = self
            .stack
            .session(id)
            .map(crate::sessions::Session::terminal_ids)
            .unwrap_or_default();
        if let Some(shell) = shell {
            self.shells
                .extend(terminals.iter().map(|terminal| (*terminal, shell.to_string())));
        }
        self.pending_directories
            .extend(terminals.into_iter().map(|terminal| (terminal, directory.clone())));
        (id, self.refresh())
    }

    /// Splits the active terminal of a session, in `orientation` or the longer side.
    pub fn split(
        &mut self,
        session: SessionId,
        orientation: Option<Orientation>,
        host: &dyn Host,
    ) -> (Option<TerminalId>, Vec<Effect>) {
        let Some(terminal) = self.stack.session(session).map(|found| found.active_terminal) else {
            return (None, Vec::new());
        };
        let directory = self.start_directory(terminal, host);
        let (width, height) = self.area;
        let new = match orientation {
            Some(orientation) => self.stack.split(terminal, orientation),
            None => self.stack.split_auto(terminal, width, height),
        };
        let Some(new) = new else {
            return (None, Vec::new());
        };
        self.pending_directories.push((new, directory));
        if let Some(shell) = self.shells.get(&terminal).cloned() {
            self.shells.insert(new, shell);
        }
        (Some(new), self.refresh())
    }

    fn ask(&mut self, pending: Pending, title: String, text: String, confirm: Confirm) -> Vec<Effect> {
        let id = self.next_question;
        self.next_question = self.next_question.wrapping_add(1);
        self.questions.insert(id, pending);
        vec![Effect::Ask(Question {
            id,
            title,
            text,
            confirm,
        })]
    }

    fn ask_close(&mut self, session: SessionId, terminal_only: bool) -> Vec<Effect> {
        let intro = tr("@info", "<warning>This session is protected from closing.</warning>");
        let (question, pending) = if terminal_only {
            (
                tr("@info", "Close this terminal anyway?"),
                Pending::CloseTerminal(session),
            )
        } else {
            (
                tr("@info", "Close this session anyway?"),
                Pending::CloseSession(session),
            )
        };
        let title = tr("@title:window", "Close Protected Session");
        self.ask(pending, title, format!("{intro}<br/><br/>{question}"), Confirm::Close)
    }

    /// Closes a session, after asking if it is protected from closing.
    pub fn remove_session(&mut self, session: SessionId) -> Vec<Effect> {
        match self.stack.session(session).map(|found| found.closable) {
            Some(true) => self.finish_session_removal(session),
            Some(false) => self.ask_close(session, false),
            None => Vec::new(),
        }
    }

    /// Closes the active terminal of a session, after asking if the session is protected.
    pub fn close_active_terminal(&mut self, session: SessionId) -> Vec<Effect> {
        match self
            .stack
            .session(session)
            .map(|found| (found.closable, found.active_terminal))
        {
            Some((true, terminal)) => self.close_terminal(terminal),
            Some((false, _)) => self.ask_close(session, true),
            None => Vec::new(),
        }
    }

    fn finish_session_removal(&mut self, session: SessionId) -> Vec<Effect> {
        let terminals = self
            .stack
            .session(session)
            .map(crate::sessions::Session::terminal_ids)
            .unwrap_or_default();
        let mut effects: Vec<Effect> = terminals
            .into_iter()
            .map(|terminal| self.forget_terminal(terminal))
            .collect();
        self.agent_sessions.remove(&session);
        let removal = self.stack.remove_session(session);
        effects.extend(self.refresh());
        if let Removal::LastTabClosed(_) = removal
            && self.window_visible
            && !self.settings.keep_open_after_last_session_closes
        {
            effects.extend(self.toggle_window(true));
        }
        effects
    }

    fn forget_terminal(&mut self, terminal: TerminalId) -> Effect {
        self.running.remove(&terminal);
        self.activity_reported.remove(&terminal);
        self.silence_reported.remove(&terminal);
        self.busy.remove(&terminal);
        Effect::Stop(terminal)
    }

    /// Removes a terminal from its session, closing the session when it was the last one.
    pub fn close_terminal(&mut self, terminal: TerminalId) -> Vec<Effect> {
        let mut effects = vec![self.forget_terminal(terminal)];
        match self.stack.close_terminal(terminal) {
            Some(session) => effects.extend(self.finish_session_removal(session)),
            None => effects.extend(self.refresh()),
        }
        effects
    }

    /// The program of a terminal ended.
    pub fn terminal_exited(&mut self, terminal: TerminalId) -> Vec<Effect> {
        if self.running.contains(&terminal) {
            self.close_terminal(terminal)
        } else {
            Vec::new()
        }
    }

    /// The answer to a question. Questions about sessions that are gone meanwhile do nothing.
    pub fn answer(&mut self, id: u32, accepted: bool) -> Vec<Effect> {
        let Some(pending) = self.questions.remove(&id).filter(|_| accepted) else {
            return Vec::new();
        };
        match pending {
            Pending::CloseSession(session) if self.stack.session(session).is_some() => {
                self.finish_session_removal(session)
            }
            Pending::CloseTerminal(session) => match self.stack.session(session).map(|found| found.active_terminal) {
                Some(terminal) => self.close_terminal(terminal),
                None => Vec::new(),
            },
            Pending::Quit => vec![Effect::Quit],
            // The keyboard may have been locked while the user read the question.
            Pending::Paste { terminal, text } if self.accepts_input(terminal) => vec![Effect::Terminal {
                terminal,
                command: TerminalCommand::Paste(text),
            }],
            Pending::Import => vec![Effect::Import],
            Pending::CloseSession(_) | Pending::Paste { .. } => Vec::new(),
        }
    }

    fn accepts_input(&self, terminal: TerminalId) -> bool {
        self.stack
            .terminal_state(terminal)
            .is_some_and(|state| state.keyboard_input_enabled)
    }

    /// Asks before pasting text whose lines the shell would run one by one as they arrive,
    /// which it does without bracketed paste.
    pub fn ask_paste(&mut self, terminal: TerminalId, text: String) -> Vec<Effect> {
        let question = tr(
            "@info",
            "The text you are pasting contains line breaks. Commands in it will run immediately. Paste it anyway?",
        );
        let title = tr("@title:window", "Paste Multiple Lines?");
        self.ask(Pending::Paste { terminal, text }, title, question, Confirm::Continue)
    }

    /// Asks before the settings of Yakuake and Konsole replace the matching ones.
    pub fn ask_import(&mut self) -> Vec<Effect> {
        let question = tr(
            "@info",
            "This replaces the matching settings with those from Yakuake and Konsole. Continue?",
        );
        let title = tr("@title:window", "Import from Yakuake");
        self.ask(Pending::Import, title, question, Confirm::Continue)
    }

    /// Quits, after asking if several sessions are open or some are protected.
    pub fn quit(&mut self) -> Vec<Effect> {
        let many = self.settings.confirm_quit && self.stack.count() > 1;
        let locked = self.stack.has_unclosable_sessions();
        if !many && !locked {
            return vec![Effect::Quit];
        }
        let warning = match (many, locked) {
            (true, true) => tr(
                "@info",
                "<warning>Several sessions are open, <emphasis>and some of them are protected from closing.</emphasis> Quitting ends all of them.</warning>",
            ),
            (true, false) => tr(
                "@info",
                "<warning>Several sessions are open. Quitting ends all of them.</warning>",
            ),
            _ => tr(
                "@info",
                "<warning>Some sessions are protected from closing. Quitting ends them as well.</warning>",
            ),
        };
        let question = tr("@info", "Quit Kudari?");
        let text = format!("{warning}<br /><br />{question}");
        self.ask(Pending::Quit, tr("@title:window", "Quit"), text, Confirm::Quit)
    }

    fn notify(&self, event: &'static str, text: &dyn Fn(&str) -> String, terminal: TerminalId) -> Effect {
        let title = self
            .stack
            .session_for_terminal(terminal)
            .and_then(|session| self.stack.tab_title(session))
            .unwrap_or_default();
        Effect::Notify {
            event,
            text: text(title),
        }
    }

    /// Switches a monitor off after its alert, if the terminal asks for that.
    fn end_one_shot(&mut self, terminal: TerminalId, monitor: Monitor) -> Vec<Effect> {
        let Some(state) = self
            .stack
            .terminal_state_mut(terminal)
            .filter(|state| state.monitor_once)
        else {
            return Vec::new();
        };
        match monitor {
            Monitor::Activity => state.monitor_activity = false,
            Monitor::Process => state.monitor_process = false,
            _ => state.monitor_silence = false,
        }
        vec![Effect::Publish]
    }

    /// A terminal printed something.
    pub fn terminal_output(&mut self, terminal: TerminalId) -> Vec<Effect> {
        let mut effects = vec![Effect::Repaint(terminal)];
        if !self.settings.tab_activity_color.is_empty()
            && let Some(session) = self.stack.session_for_terminal(terminal)
            && Some(session) != self.stack.active_id()
            && self.tab_activity.insert(session)
        {
            effects.push(Effect::Publish);
        }
        let monitored = self
            .stack
            .terminal_state(terminal)
            .is_some_and(|state| state.monitor_activity);
        if monitored && self.activity_reported.insert(terminal) {
            effects.push(self.notify(
                "activity",
                &|title| tr_arg("@info", "Activity in session \"%1\".", title),
                terminal,
            ));
            effects.extend(self.end_one_shot(terminal, Monitor::Activity));
        }
        self.silence_reported.remove(&terminal);
        effects
    }

    /// A terminal rang its bell. A program that rings in a loop must not flood the
    /// notifications.
    pub fn terminal_bell(&mut self, terminal: TerminalId) -> Vec<Effect> {
        if self.last_bell.is_some_and(|last| last.elapsed() < BELL_INTERVAL) {
            return Vec::new();
        }
        self.last_bell = Some(Instant::now());
        match self.settings.bell_mode.as_str() {
            "none" => return Vec::new(),
            "visual" => {
                return vec![Effect::Highlight {
                    terminal,
                    persistent: false,
                }];
            }
            _ => {}
        }
        let visible = self
            .stack
            .active()
            .is_some_and(|session| session.terminals.contains_key(&terminal));
        let event = if visible { "BellVisible" } else { "BellInvisible" };
        vec![self.notify(
            event,
            &|title| tr_arg("@info", "Bell in session \"%1\".", title),
            terminal,
        )]
    }

    /// Monitored terminals report activity once per visit: the next report is due after the
    /// user has switched away or retracted the window.
    fn rearm_activity(&mut self) {
        self.activity_reported.clear();
    }

    /// The periodic look at the terminals: titles, finished programs, silence and auto save.
    pub fn poll(&mut self, statuses: &[Status]) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.polls = self.polls.wrapping_add(1);
        if self
            .polls
            .is_multiple_of(self.settings.auto_save_seconds.max(1) as u32 * POLLS_PER_SECOND)
        {
            effects.push(Effect::AutoSave);
        }
        let mut changed = false;
        for status in statuses {
            let id = status.terminal;
            let finished = if status.busy {
                self.busy.insert(id);
                false
            } else {
                self.busy.remove(&id)
            };
            if finished && self.stack.terminal_state(id).is_some_and(|state| state.monitor_process) {
                effects.push(self.notify(
                    "processFinished",
                    &|title| {
                        tr_arg(
                            "@info",
                            "Process finished in monitored terminal in session \"%1\".",
                            title,
                        )
                    },
                    id,
                ));
                effects.extend(self.end_one_shot(id, Monitor::Process));
            }
            let silent = status.quiet_for >= Duration::from_secs(self.settings.silence_seconds as u64);
            if self
                .stack
                .terminal_state(id)
                .is_some_and(|state| state.title != status.title)
            {
                if let Some(session) = self.stack.set_terminal_title(id, &status.title)
                    && self.settings.dynamic_tab_titles
                    && self
                        .stack
                        .session(session)
                        .is_some_and(|found| found.active_terminal == id)
                {
                    self.stack.set_tab_title(session, &status.title, false);
                }
                changed = true;
            }
            let watching = self.stack.terminal_state(id).is_some_and(|state| state.monitor_silence);
            if watching && silent && self.silence_reported.insert(id) {
                effects.push(self.notify(
                    "silence",
                    &|title| tr_arg("@info", "Silence in session \"%1\".", title),
                    id,
                ));
                effects.extend(self.end_one_shot(id, Monitor::Silence));
            }
        }
        if changed {
            effects.push(Effect::Publish);
        }
        effects
    }

    /// Grows the active terminal by a few pixels.
    pub fn grow(&mut self, direction: Direction) -> Vec<Effect> {
        let Some(terminal) = self.active_terminal() else {
            return Vec::new();
        };
        let (width, height) = self.area;
        self.stack.try_grow(terminal, direction, 10, width, height);
        vec![Effect::Publish]
    }

    /// Shows a session.
    pub fn raise(&mut self, session: SessionId) -> Vec<Effect> {
        self.rearm_activity();
        self.stack.raise_session(session);
        self.refresh()
    }

    pub fn select_next_tab(&mut self) -> Vec<Effect> {
        self.rearm_activity();
        self.stack.select_next_tab();
        self.refresh()
    }

    pub fn select_previous_tab(&mut self) -> Vec<Effect> {
        self.rearm_activity();
        self.stack.select_previous_tab();
        self.refresh()
    }

    /// Moves a tab before the tab at `target`, or to the end without one.
    pub fn drop_tab(&mut self, session: SessionId, target: Option<usize>) -> Vec<Effect> {
        self.stack.drop_tab(session, target);
        self.refresh()
    }

    pub fn rename_tab(&mut self, session: SessionId, title: &str) -> Vec<Effect> {
        self.stack.set_tab_title(session, title.trim(), true);
        self.refresh()
    }

    /// Highlights a terminal the user clicked into, if that is enabled and there are several.
    #[must_use]
    pub fn terminal_clicked(&self, terminal: TerminalId) -> Vec<Effect> {
        let several = self.stack.active().is_some_and(|session| session.terminals.len() > 1);
        if self.settings.terminal_highlight_on_manual_activation && several {
            vec![Effect::Highlight {
                terminal,
                persistent: false,
            }]
        } else {
            Vec::new()
        }
    }

    /// Moves the keyboard to the next or previous terminal of the session.
    pub fn focus_neighbor(&mut self, next: bool) -> Vec<Effect> {
        if next {
            self.stack.focus_next_terminal();
        } else {
            self.stack.focus_previous_terminal();
        }
        let mut effects = self.refresh();
        if self.settings.terminal_highlight_on_manual_activation
            && let Some(terminal) = self.active_terminal()
        {
            effects.push(Effect::Highlight {
                terminal,
                persistent: false,
            });
        }
        effects
    }

    /// Changes the font size of one terminal by `delta` points.
    pub fn zoom_terminal(&mut self, terminal: TerminalId, delta: i32) -> Vec<Effect> {
        self.stack.zoom_terminal(terminal, delta);
        vec![Effect::Publish]
    }

    pub fn reset_zoom(&mut self, terminal: TerminalId) -> Vec<Effect> {
        self.stack.reset_zoom(terminal);
        vec![Effect::Publish]
    }

    pub fn activate_terminal(&mut self, terminal: TerminalId) -> Vec<Effect> {
        self.stack.set_active_terminal(terminal);
        vec![Effect::Publish]
    }

    /// Moves a splitter of the active session.
    pub fn drag_handle(&mut self, handle: usize, delta: i32) -> Vec<Effect> {
        let (width, height) = self.area;
        let found = self
            .stack
            .active()
            .and_then(|session| session.layout(width, height).handles.into_iter().nth(handle));
        match found {
            Some(found) => {
                self.stack.drag_handle(&found, delta, width, height);
                vec![Effect::Publish]
            }
            None => Vec::new(),
        }
    }

    pub fn set_tab_color(&mut self, session: SessionId, color: Option<String>) -> Vec<Effect> {
        self.stack.set_session_color(session, color);
        vec![Effect::Publish]
    }

    /// Runs an action of a menu or shortcut. `checked` is the new state of a checkable one.
    pub fn trigger(&mut self, name: &str, checked: bool, store: &dyn Store, host: &dyn Host) -> Vec<Effect> {
        let session = self.context_session();
        let active = self.active_terminal();
        let (width, height) = (self.settings.width, self.settings.height);
        let for_session = |action: &dyn Fn(SessionId) -> Vec<Effect>| session.map(action).unwrap_or_default();
        let terminal = |command: TerminalCommand| {
            active
                .map(|terminal| Effect::Terminal { terminal, command })
                .into_iter()
                .collect()
        };
        match name {
            "toggle-window-state" => self.toggle_window(host.has_focus()),
            "view-full-screen" => self.toggle_fullscreen(),
            "enlarge-font" | "shrink-font" => match active {
                Some(terminal) => self.zoom_terminal(terminal, if name == "enlarge-font" { 1 } else { -1 }),
                None => Vec::new(),
            },
            "reset-font-size" => match active {
                Some(terminal) => self.reset_zoom(terminal),
                None => Vec::new(),
            },
            "file_quit" => self.quit(),
            "options_configure" => vec![Effect::OpenSettings(None)],
            "options_configure_keybinding" => vec![Effect::OpenSettings(Some("shortcuts"))],
            "edit-profile" => vec![Effect::OpenSettings(Some("terminal"))],
            "keep-open" => self.set_keep_open(store, checked),
            "increase-window-width" if width <= 90 => self.set_window_size(store, "width", width + 10),
            "decrease-window-width" if width >= 20 => self.set_window_size(store, "width", width - 10),
            "increase-window-height" if height <= 90 => self.set_window_size(store, "height", height + 10),
            "decrease-window-height" if height >= 20 => self.set_window_size(store, "height", height - 10),
            "new-session" => self.add_session(SessionType::Single, host).1,
            "new-session-two-horizontal" => self.add_session(SessionType::TwoHorizontal, host).1,
            "new-session-two-vertical" => self.add_session(SessionType::TwoVertical, host).1,
            "new-session-quad" => self.add_session(SessionType::Quad, host).1,
            "close-session" => match session {
                Some(session) => self.remove_session(session),
                None => Vec::new(),
            },
            "previous-session" => self.select_previous_tab(),
            "next-session" => self.select_next_tab(),
            "move-session-left" | "move-session-right" => match session {
                Some(session) => {
                    if name.ends_with("left") {
                        self.stack.move_tab_left(session);
                    } else {
                        self.stack.move_tab_right(session);
                    }
                    vec![Effect::Publish]
                }
                None => Vec::new(),
            },
            "grow-terminal-right" => self.grow(Direction::Right),
            "grow-terminal-left" => self.grow(Direction::Left),
            "grow-terminal-top" => self.grow(Direction::Up),
            "grow-terminal-bottom" => self.grow(Direction::Down),
            "rename-session" => for_session(&|session| vec![Effect::RenameTab(session)]),
            "previous-terminal" | "next-terminal" => self.focus_neighbor(name.starts_with("next")),
            "close-active-terminal" => match session {
                Some(session) => self.close_active_terminal(session),
                None => Vec::new(),
            },
            "split-left-right" | "split-top-bottom" | "split-auto" => match session {
                Some(session) => {
                    let orientation = match name {
                        "split-left-right" => Some(Orientation::Horizontal),
                        "split-top-bottom" => Some(Orientation::Vertical),
                        _ => None,
                    };
                    self.split(session, orientation, host).1
                }
                None => Vec::new(),
            },
            "toggle-session-prevent-closing" => match session.and_then(|session| self.stack.session_mut(session)) {
                Some(found) => {
                    found.closable = !checked;
                    vec![Effect::Publish]
                }
                None => Vec::new(),
            },
            "toggle-session-keyboard-input"
            | "toggle-session-monitor-activity"
            | "toggle-session-monitor-silence"
            | "toggle-session-monitor-process"
            | "toggle-session-monitor-once" => {
                let monitor = name.rsplit('-').next().and_then(|last| match last {
                    "input" => Some(Monitor::Keyboard),
                    other => Monitor::named(other),
                });
                match (session, monitor) {
                    (Some(session), Some(monitor)) => self.set_all_monitor(session, monitor, checked),
                    _ => Vec::new(),
                }
            }
            "adjust-history" => terminal(TerminalCommand::AdjustHistory),
            "allow-mouse-tracking" => terminal(TerminalCommand::MouseTracking(checked)),
            "open-browser" => terminal(TerminalCommand::OpenFolder),
            "web-search" => terminal(TerminalCommand::WebSearch),
            "clear-history-and-reset" => terminal(TerminalCommand::Reset),
            "file_save_as" => terminal(TerminalCommand::SaveOutput),
            "file_print" => terminal(TerminalCommand::Print),
            "auto-save" => terminal(TerminalCommand::AutoSave(checked)),
            "edit_copy" | "edit_paste" | "edit_select_all" | "edit_find" | "edit_find_next" | "edit_find_prev"
            | "clear-scrollback" | "paste-selection" | "select-line" | "select-mode" => active
                .map(|terminal| Effect::TerminalAction {
                    terminal,
                    action: name.to_string(),
                })
                .into_iter()
                .collect(),
            other => match other
                .strip_prefix("switch-to-session-")
                .and_then(|index| index.parse::<i32>().ok())
            {
                Some(index) => {
                    let session = self.stack.session_at_tab(index - 1);
                    self.raise(session)
                }
                None => Vec::new(),
            },
        }
    }

    /// Shows or retracts the window. With `has_focus` false, a window that stays open and
    /// toggles to the focus only gets the keyboard.
    pub fn toggle_window(&mut self, has_focus: bool) -> Vec<Effect> {
        let visible = !self.window_visible;
        if !visible && self.settings.keep_open && self.settings.toggle_to_focus && !has_focus {
            // A global shortcut brings no activation token, so asking for focus would be
            // ignored. Only showing the window gives it the keyboard.
            self.refocusing = true;
            return vec![Effect::Refocus];
        }
        let mut effects = Vec::new();
        if visible {
            effects.push(Effect::UpdateWorkArea);
        } else {
            self.rearm_activity();
        }
        if !visible && !self.settings.remember_fullscreen && self.fullscreen {
            self.fullscreen = false;
            effects.push(Effect::Fullscreen(false));
        }
        self.window_visible = visible;
        effects.push(Effect::WindowVisible(visible));
        effects
    }

    pub fn toggle_fullscreen(&mut self) -> Vec<Effect> {
        self.fullscreen = !self.fullscreen;
        vec![Effect::Fullscreen(self.fullscreen)]
    }

    /// The window appeared. Returns whether it slides in, which it does unless it only came
    /// back for the focus.
    pub fn after_show(&mut self) -> (bool, Vec<Effect>) {
        (!std::mem::take(&mut self.refocusing), self.focus_active())
    }

    /// Another window got the focus. Own menus and dialogs, and a dialog that just closed, do
    /// not retract the window.
    pub fn focus_lost(&mut self, has_focus: bool, dialog_open: bool) -> Vec<Effect> {
        if has_focus {
            return Vec::new();
        }
        if std::mem::take(&mut self.focus_toggle_lock) {
            return Vec::new();
        }
        if !self.settings.keep_open && self.window_visible && !dialog_open {
            self.toggle_window(false)
        } else {
            Vec::new()
        }
    }

    /// A dialog of the front end closed.
    pub fn dialog_closed(&mut self) {
        self.focus_toggle_lock = true;
    }

    /// A tab menu opened for a session, whose actions then apply to it.
    pub fn menu_opened(&mut self, session: SessionId) -> Vec<Effect> {
        self.menu_session = Some(session);
        vec![Effect::UpdateActions]
    }

    /// The tab menu closed; its triggered action has run already.
    pub fn menu_closed(&mut self) -> Vec<Effect> {
        self.menu_session = None;
        vec![Effect::UpdateActions]
    }

    pub fn set_keep_open(&mut self, store: &dyn Store, keep_open: bool) -> Vec<Effect> {
        self.settings.set_keep_open(store, keep_open);
        vec![Effect::PublishSettings]
    }

    /// Sets the width, height or screen of the window from a menu.
    pub fn set_window_size(&mut self, store: &dyn Store, kind: &str, value: i32) -> Vec<Effect> {
        match kind {
            "width" => self.settings.set_width(store, value),
            "height" => self.settings.set_height(store, value),
            "screen" => self.settings.set_screen(store, value),
            _ => return Vec::new(),
        }
        vec![Effect::PublishSettings]
    }

    /// Switches a monitor of one terminal.
    pub fn terminal_toggled(&mut self, monitor: Monitor, terminal: TerminalId, checked: bool) -> Vec<Effect> {
        if let Some(state) = self.stack.terminal_state_mut(terminal) {
            match monitor {
                Monitor::Keyboard => state.keyboard_input_enabled = !checked,
                Monitor::Activity => state.monitor_activity = checked,
                Monitor::Silence => state.monitor_silence = checked,
                Monitor::Process | Monitor::Once => return Vec::new(),
            }
        }
        self.activity_reported.remove(&terminal);
        let mut effects = Vec::new();
        if monitor == Monitor::Silence {
            effects.extend(self.restart_silence(terminal));
        }
        effects.push(Effect::Publish);
        effects
    }

    /// The silence period counts from now, and may be reported again.
    fn restart_silence(&mut self, terminal: TerminalId) -> Vec<Effect> {
        self.silence_reported.remove(&terminal);
        vec![Effect::RestartSilence(terminal)]
    }

    /// Switches a monitor of every terminal of a session.
    pub fn set_all_monitor(&mut self, session: SessionId, monitor: Monitor, checked: bool) -> Vec<Effect> {
        if let Some(found) = self.stack.session_mut(session) {
            for state in found.terminals.values_mut() {
                match monitor {
                    Monitor::Keyboard => state.keyboard_input_enabled = !checked,
                    Monitor::Activity => state.monitor_activity = checked,
                    Monitor::Process => state.monitor_process = checked,
                    Monitor::Once => state.monitor_once = checked,
                    Monitor::Silence => state.monitor_silence = checked,
                }
            }
        }
        self.activity_reported.clear();
        let mut effects = Vec::new();
        if monitor == Monitor::Silence {
            let terminals = self
                .stack
                .session(session)
                .map(crate::sessions::Session::terminal_ids)
                .unwrap_or_default();
            for terminal in terminals {
                effects.extend(self.restart_silence(terminal));
            }
        }
        effects.push(Effect::Publish);
        effects
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    #[derive(Default)]
    struct Table(RefCell<HashMap<String, String>>);

    impl Store for Table {
        fn read(&self, _: &str, key: &str, default: &str) -> String {
            self.0.borrow().get(key).cloned().unwrap_or_else(|| default.to_string())
        }

        fn write(&self, _: &str, key: &str, value: &str) {
            self.0.borrow_mut().insert(key.to_string(), value.to_string());
        }
    }

    struct Here;

    impl Host for Here {
        fn working_directory(&self, terminal: TerminalId) -> Option<PathBuf> {
            Some(PathBuf::from(format!("/work/{terminal}")))
        }

        fn has_focus(&self) -> bool {
            true
        }

        fn terminal(&self, _terminal: TerminalId) -> Option<Arc<dyn agent::Terminal>> {
            None
        }
    }

    fn app_with(values: &[(&str, &str)]) -> App {
        let table = Table::default();
        table.write("", "FirstRun", "false");
        for (key, value) in values {
            table.write("", key, value);
        }
        let mut app = App::new(Settings::load(&table));
        app.start();
        app.set_geometry(800, 400, 8.0, 16.0);
        app
    }

    fn fresh() -> App {
        app_with(&[])
    }

    fn status(terminal: TerminalId, title: &str, busy: bool, quiet: u64) -> Status {
        Status {
            terminal,
            title: title.to_string(),
            busy,
            quiet_for: Duration::from_secs(quiet),
        }
    }

    fn notified(effects: &[Effect]) -> Vec<(&'static str, String)> {
        effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::Notify { event, text } => Some((*event, text.clone())),
                _ => None,
            })
            .collect()
    }

    fn question(effects: &[Effect]) -> Option<Question> {
        effects.iter().find_map(|effect| match effect {
            Effect::Ask(question) => Some(question.clone()),
            _ => None,
        })
    }

    #[test]
    fn the_first_start_opens_the_window_and_welcomes() {
        let table = Table::default();
        let mut app = App::new(Settings::load(&table));
        let effects = app.start();
        assert_eq!(effects.first(), Some(&Effect::Publish));
        assert!(effects.contains(&Effect::WindowVisible(true)) && effects.contains(&Effect::FirstRun));
        assert_eq!(app.stack().count(), 1);
        let mut quiet = App::new(Settings::load(&Table::default()));
        quiet.replace_settings({
            let table = Table::default();
            table.write("", "FirstRun", "false");
            table.write("", "OpenAfterStart", "true");
            Settings::load(&table)
        });
        assert!(quiet.start().contains(&Effect::WindowVisible(true)));
    }

    #[test]
    fn terminals_start_once_the_window_has_a_size() {
        let mut app = App::new(Settings::load(&Table::default()));
        app.start();
        assert!(app.pending_starts().is_empty(), "no size yet");
        app.set_geometry(800, 400, 8.0, 16.0);
        let starts = app.pending_starts();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].rect.width, 800);
        app.started(&[starts[0].terminal]);
        assert!(app.pending_starts().is_empty());
        assert_eq!(app.cell(), (8.0, 16.0));
        app.set_geometry(800, 400, 0.0, 0.0);
        assert_eq!(app.cell(), (1.0, 1.0), "cells have a size");
    }

    #[test]
    fn terminals_start_at_home_without_another_directory() {
        struct Nowhere;

        impl Host for Nowhere {
            fn working_directory(&self, _terminal: TerminalId) -> Option<PathBuf> {
                None
            }

            fn has_focus(&self) -> bool {
                true
            }

            fn terminal(&self, _terminal: TerminalId) -> Option<Arc<dyn agent::Terminal>> {
                None
            }
        }

        let home = crate::process::home();
        assert!(home.is_some());
        let mut app = app_with(&[]);
        let starts = app.pending_starts();
        assert_eq!(starts[0].directory, home, "the first terminal");
        app.started(&[starts[0].terminal]);
        app.add_session(SessionType::Single, &Nowhere);
        assert_eq!(
            app.pending_starts()[0].directory,
            home,
            "a terminal whose directory is not known"
        );
    }

    #[test]
    fn new_sessions_start_where_the_active_terminal_is() {
        let mut app = app_with(&[("InheritDirectory", "true")]);
        let first = app.active_terminal().unwrap();
        app.started(&[first]);
        let (session, effects) = app.add_session(SessionType::TwoHorizontal, &Here);
        assert_eq!(app.stack().active_id(), Some(session));
        assert_eq!(effects.first(), Some(&Effect::Publish));
        let starts = app.pending_starts();
        assert_eq!(starts.len(), 2);
        let expected = PathBuf::from(format!("/work/{first}"));
        assert!(starts.iter().all(|start| start.directory.as_ref() == Some(&expected)));
        let (new, _) = app.split(session, None, &Here);
        assert!(new.is_some());
        assert_eq!(app.split(-5, None, &Here), (None, Vec::new()));
        let home = app_with(&[("InheritDirectory", "false")]);
        assert_eq!(home.active_directory(&Here), crate::process::home());
    }

    #[test]
    fn protected_sessions_close_only_after_asking() {
        let mut app = fresh();
        let (second, _) = app.add_session(SessionType::Single, &Here);
        app.stack_mut().session_mut(second).unwrap().closable = false;
        let effects = app.remove_session(second);
        let asked = question(&effects).expect("asks");
        assert_eq!(asked.confirm, Confirm::Close);
        assert!(asked.text.contains("Close this session anyway?"));
        assert!(app.answer(asked.id, false).is_empty());
        assert!(app.stack().session(second).is_some());
        let asked = question(&app.remove_session(second)).unwrap();
        let closed = app.answer(asked.id, true);
        assert!(closed.iter().any(|effect| matches!(effect, Effect::Stop(_))));
        assert!(app.stack().session(second).is_none());
        assert!(app.answer(asked.id, true).is_empty(), "an answer counts once");
        assert!(app.remove_session(second).is_empty());
    }

    #[test]
    fn a_chosen_shell_carries_over_to_splits() {
        let mut app = fresh();
        let shell_of = |app: &App, terminal: TerminalId| {
            app.pending_starts()
                .into_iter()
                .find(|start| start.terminal == terminal)
                .and_then(|start| start.shell)
        };
        let first = app.stack().active().unwrap().active_terminal;
        assert_eq!(shell_of(&app, first), None);
        let (session, _) = app.add_session_with_shell(SessionType::Single, Some("wsl:Debian"), &Here);
        let terminal = app.stack().session(session).unwrap().active_terminal;
        assert_eq!(shell_of(&app, terminal).as_deref(), Some("wsl:Debian"));
        let (split, _) = app.split(session, None, &Here);
        let split = split.unwrap();
        assert_eq!(shell_of(&app, split).as_deref(), Some("wsl:Debian"));
        let (plain, _) = app.add_session(SessionType::Single, &Here);
        let plain = app.stack().session(plain).unwrap().active_terminal;
        assert_eq!(shell_of(&app, plain), None);
        let _ = app.remove_session(session);
        app.started(&[first, plain]);
        assert!(app.shells.is_empty(), "the shells of closed terminals are forgotten");
    }

    #[test]
    fn pasting_lines_that_would_run_waits_for_the_user() {
        let mut app = fresh();
        let terminal = app.stack().active().unwrap().active_terminal;
        let asked = question(&app.ask_paste(terminal, "ls\nrm x\n".into())).unwrap();
        assert_eq!(asked.confirm, Confirm::Continue);
        assert!(asked.text.contains("line breaks"));
        assert_eq!(
            app.answer(asked.id, true),
            [Effect::Terminal {
                terminal,
                command: TerminalCommand::Paste("ls\nrm x\n".into())
            }]
        );
        let declined = question(&app.ask_paste(terminal, "ls\n".into())).unwrap();
        assert!(app.answer(declined.id, false).is_empty());
    }

    #[test]
    fn a_paste_waits_in_vain_for_a_locked_or_closed_terminal() {
        let mut app = fresh();
        let terminal = app.stack().active().unwrap().active_terminal;
        let asked = question(&app.ask_paste(terminal, "ls\n".into())).unwrap();
        let _ = app.terminal_toggled(Monitor::Keyboard, terminal, true);
        assert!(app.answer(asked.id, true).is_empty());
        let asked = question(&app.ask_paste(99, "ls\n".into())).unwrap();
        assert!(app.answer(asked.id, true).is_empty());
    }

    #[test]
    fn importing_waits_for_the_user() {
        let mut app = fresh();
        let asked = question(&app.ask_import()).unwrap();
        assert_eq!(asked.confirm, Confirm::Continue);
        assert!(asked.text.contains("Yakuake"));
        assert_eq!(app.answer(asked.id, true), [Effect::Import]);
        let asked = question(&app.ask_import()).unwrap();
        assert!(app.answer(asked.id, false).is_empty());
    }

    #[test]
    fn a_protected_terminal_closes_after_asking() {
        let mut app = fresh();
        let (session, _) = app.add_session(SessionType::TwoVertical, &Here);
        app.stack_mut().session_mut(session).unwrap().closable = false;
        let asked = question(&app.close_active_terminal(session)).unwrap();
        assert!(asked.text.contains("Close this terminal anyway?"));
        app.answer(asked.id, true);
        assert_eq!(app.stack().session(session).map(|found| found.terminals.len()), Some(1));
        app.stack_mut().session_mut(session).unwrap().closable = true;
        let closed = app.close_active_terminal(session);
        assert!(closed.iter().any(|effect| matches!(effect, Effect::Stop(_))));
        assert!(app.stack().session(session).is_none());
        assert!(app.close_active_terminal(session).is_empty());
    }

    #[test]
    fn the_last_session_closes_the_window_unless_asked_to_stay() {
        let mut app = fresh();
        app.toggle_window(true);
        let session = app.stack().active_id().unwrap();
        let effects = app.remove_session(session);
        assert!(effects.contains(&Effect::WindowVisible(false)));
        assert_eq!(app.stack().count(), 1, "a new session replaces it");
        let mut staying = app_with(&[("KeepOpenAfterLastSessionCloses", "true")]);
        staying.toggle_window(true);
        let session = staying.stack().active_id().unwrap();
        assert!(!staying.remove_session(session).contains(&Effect::WindowVisible(false)));
    }

    #[test]
    fn ended_programs_close_their_terminals() {
        let mut app = fresh();
        let terminal = app.active_terminal().unwrap();
        assert!(app.terminal_exited(terminal).is_empty(), "never started");
        app.started(&[terminal]);
        assert!(app.terminal_exited(terminal).contains(&Effect::Stop(terminal)));
        app.add_agent_session(7, "a");
        assert!(app.is_agent_session(7) && !app.is_agent_session(8));
        assert_eq!(app.agent_session_owner(7), Some("a"));
    }

    #[test]
    fn quitting_asks_when_much_would_end() {
        let mut lonely = fresh();
        assert_eq!(lonely.quit(), [Effect::Quit]);
        let mut busy = app_with(&[("ConfirmQuit", "true")]);
        busy.add_session(SessionType::Single, &Here);
        let asked = question(&busy.quit()).unwrap();
        assert_eq!(asked.confirm, Confirm::Quit);
        assert!(asked.text.contains("Several sessions are open.") && asked.text.ends_with("Quit Kudari?"));
        assert_eq!(busy.answer(asked.id, true), [Effect::Quit]);
        let session = busy.stack().active_id().unwrap();
        busy.stack_mut().session_mut(session).unwrap().closable = false;
        assert!(
            question(&busy.quit())
                .unwrap()
                .text
                .contains("and some of them are protected")
        );
        let mut locked = fresh();
        let session = locked.stack().active_id().unwrap();
        locked.stack_mut().session_mut(session).unwrap().closable = false;
        assert!(
            question(&locked.quit())
                .unwrap()
                .text
                .contains("Quitting ends them as well")
        );
    }

    #[test]
    fn output_in_the_background_marks_its_tab() {
        let mut app = app_with(&[("TabActivityColor", "#ff0000")]);
        let first = app.stack().active_id().unwrap();
        let terminal = app.active_terminal().unwrap();
        app.add_session(SessionType::Single, &Here);
        assert_eq!(
            app.terminal_output(terminal),
            [Effect::Repaint(terminal), Effect::Publish]
        );
        assert_eq!(app.terminal_output(terminal), [Effect::Repaint(terminal)], "once");
        let model = app.model();
        assert!(model.tabs.iter().any(|tab| tab.id == first && tab.activity));
        app.raise(first);
        assert!(app.model().tabs.iter().all(|tab| !tab.activity), "seen");
    }

    #[test]
    fn activity_is_reported_once_per_visit() {
        let mut app = fresh();
        let session = app.stack().active_id().unwrap();
        let terminal = app.active_terminal().unwrap();
        app.set_all_monitor(session, Monitor::Activity, true);
        let first = notified(&app.terminal_output(terminal));
        assert_eq!(first, [("activity", "Activity in session \"Shell\".".to_string())]);
        assert!(notified(&app.terminal_output(terminal)).is_empty());
        app.select_next_tab();
        assert_eq!(notified(&app.terminal_output(terminal)).len(), 1);
        // A one-shot monitor ends with its alert.
        app.set_all_monitor(session, Monitor::Once, true);
        let effects = app.terminal_output(terminal);
        assert_eq!(notified(&effects).len(), 1);
        assert!(!app.stack().terminal_state(terminal).unwrap().monitor_activity);
    }

    #[test]
    fn the_bell_rings_as_configured_but_not_in_a_burst() {
        let mut app = fresh();
        let terminal = app.active_terminal().unwrap();
        let rung = notified(&app.terminal_bell(terminal));
        assert_eq!(rung, [("BellVisible", "Bell in session \"Shell\".".to_string())]);
        assert!(app.terminal_bell(terminal).is_empty(), "too soon");
        let mut visual = app_with(&[("BellMode", "visual")]);
        let terminal = visual.active_terminal().unwrap();
        assert_eq!(
            visual.terminal_bell(terminal),
            [Effect::Highlight {
                terminal,
                persistent: false
            }]
        );
        let mut silent = app_with(&[("BellMode", "none")]);
        let terminal = silent.active_terminal().unwrap();
        assert!(silent.terminal_bell(terminal).is_empty());
        let mut hidden = fresh();
        let terminal = hidden.active_terminal().unwrap();
        hidden.add_session(SessionType::Single, &Here);
        assert_eq!(notified(&hidden.terminal_bell(terminal))[0].0, "BellInvisible");
    }

    #[test]
    fn polls_follow_titles_programs_and_silence() {
        let mut app = app_with(&[("SilenceSeconds", "10"), ("AutoSaveSeconds", "1")]);
        let session = app.stack().active_id().unwrap();
        let terminal = app.active_terminal().unwrap();
        app.set_all_monitor(session, Monitor::Process, true);
        app.set_all_monitor(session, Monitor::Silence, true);
        let renamed = app.poll(&[status(terminal, "vim", true, 0)]);
        assert!(renamed.contains(&Effect::Publish));
        assert_eq!(app.stack().tab_title(session), Some("vim"));
        assert!(
            app.poll(&[status(terminal, "vim", true, 0)])
                .contains(&Effect::AutoSave)
        );
        let ended = notified(&app.poll(&[status(terminal, "vim", false, 11)]));
        assert_eq!(ended.len(), 2, "{ended:?}");
        assert_eq!(ended[0].0, "processFinished");
        assert_eq!(ended[1], ("silence", "Silence in session \"vim\".".to_string()));
        assert!(notified(&app.poll(&[status(terminal, "vim", false, 12)])).is_empty());
        app.terminal_output(terminal);
        assert_eq!(
            notified(&app.poll(&[status(terminal, "vim", false, 12)])).len(),
            1,
            "silent again"
        );
        let mut fixed = app_with(&[("DynamicTabTitles", "false")]);
        let terminal = fixed.active_terminal().unwrap();
        fixed.poll(&[status(terminal, "top", false, 0)]);
        assert_eq!(
            fixed.stack().tab_title(fixed.stack().active_id().unwrap()),
            Some("Shell")
        );
    }

    #[test]
    fn the_window_toggles_and_keeps_the_keyboard() {
        let mut app = fresh();
        assert_eq!(
            app.toggle_window(true),
            [Effect::UpdateWorkArea, Effect::WindowVisible(true)]
        );
        assert!(app.window_visible());
        assert_eq!(app.toggle_fullscreen(), [Effect::Fullscreen(true)]);
        let hidden = app.toggle_window(true);
        assert_eq!(hidden, [Effect::Fullscreen(false), Effect::WindowVisible(false)]);
        let mut staying = app_with(&[("KeepOpen", "true"), ("ToggleToFocus", "true")]);
        staying.toggle_window(true);
        assert_eq!(staying.toggle_window(false), [Effect::Refocus]);
        assert!(staying.window_visible());
        let (slides, focus) = staying.after_show();
        assert!(!slides, "coming back for the focus does not slide");
        assert_eq!(focus.len(), 1);
        assert!(staying.after_show().0);
        let mut remembering = app_with(&[("RememberFullscreen", "true")]);
        remembering.toggle_window(true);
        remembering.toggle_fullscreen();
        assert_eq!(remembering.toggle_window(true), [Effect::WindowVisible(false)]);
        assert!(remembering.fullscreen());
    }

    #[test]
    fn losing_the_focus_retracts_unless_something_keeps_it() {
        let mut app = app_with(&[("KeepOpen", "false")]);
        app.toggle_window(true);
        assert!(app.focus_lost(true, false).is_empty(), "own menu");
        assert!(app.focus_lost(false, true).is_empty(), "own dialog");
        app.dialog_closed();
        assert!(app.focus_lost(false, false).is_empty(), "the dialog just closed");
        assert!(app.focus_lost(false, false).contains(&Effect::WindowVisible(false)));
        let mut pinned = app_with(&[("KeepOpen", "true")]);
        pinned.toggle_window(true);
        assert!(pinned.focus_lost(false, false).is_empty());
    }

    #[test]
    fn tabs_and_terminals_move_around() {
        let mut app = fresh();
        let first = app.stack().active_id().unwrap();
        let (second, _) = app.add_session(SessionType::TwoHorizontal, &Here);
        assert_eq!(app.select_previous_tab().first(), Some(&Effect::Publish));
        assert_eq!(app.stack().active_id(), Some(first));
        app.select_next_tab();
        assert_eq!(app.stack().active_id(), Some(second));
        app.drop_tab(second, Some(0));
        assert_eq!(app.stack().tabs(), [second, first]);
        app.rename_tab(second, "  build  ");
        assert_eq!(app.stack().tab_title(second), Some("build"));
        let before = app.active_terminal().unwrap();
        let moved = app.focus_neighbor(true);
        assert_ne!(app.active_terminal(), Some(before));
        assert!(!moved.iter().any(|effect| matches!(effect, Effect::Highlight { .. })));
        assert_eq!(app.activate_terminal(before), [Effect::Publish]);
        assert_eq!(app.active_terminal(), Some(before));
        assert_eq!(app.grow(Direction::Right), [Effect::Publish]);
        assert_eq!(app.drag_handle(0, 5), [Effect::Publish]);
        assert!(app.drag_handle(9, 5).is_empty());
        app.zoom_terminal(before, 2);
        assert_eq!(app.model().font_zoom, [(before, 2)]);
        app.reset_zoom(before);
        assert!(app.model().font_zoom.is_empty());
        app.set_tab_color(second, Some("#00ff00".into()));
        assert_eq!(app.model().tabs[0].color, "#00ff00");
        let model = app.model();
        assert_eq!(model.terminals.len(), 3);
        assert_eq!(model.layout.map(|(layout, _)| layout.terminals.len()), Some(2));
        assert_eq!(model.title, app.stack().active().unwrap().title);
    }

    #[test]
    fn manual_activation_can_highlight() {
        let mut app = app_with(&[("TerminalHighlightOnManualActivation", "true")]);
        let single = app.active_terminal().unwrap();
        assert!(app.terminal_clicked(single).is_empty(), "nothing to tell apart");
        app.add_session(SessionType::TwoVertical, &Here);
        let terminal = app.active_terminal().unwrap();
        assert_eq!(app.terminal_clicked(terminal).len(), 1);
        assert!(
            app.focus_neighbor(false)
                .iter()
                .any(|effect| matches!(effect, Effect::Highlight { .. }))
        );
    }

    #[test]
    fn monitors_and_locks_switch_per_terminal_and_session() {
        let mut app = fresh();
        let session = app.stack().active_id().unwrap();
        let terminal = app.active_terminal().unwrap();
        assert_eq!(
            app.terminal_toggled(Monitor::Silence, terminal, true),
            [Effect::RestartSilence(terminal), Effect::Publish]
        );
        app.terminal_toggled(Monitor::Keyboard, terminal, true);
        assert_eq!(app.model().keyboard_disabled, [terminal]);
        app.terminal_toggled(Monitor::Activity, terminal, true);
        assert!(app.stack().terminal_state(terminal).unwrap().monitor_activity);
        assert!(app.terminal_toggled(Monitor::Process, terminal, true).is_empty());
        app.set_all_monitor(session, Monitor::Keyboard, false);
        assert!(app.model().keyboard_disabled.is_empty());
        let actions = app.session_actions();
        assert!(actions.contains(&("toggle-session-monitor-silence", true, true)));
        assert!(actions.contains(&("move-session-left", false, false)));
        assert_eq!(Monitor::named("once"), Some(Monitor::Once));
        assert_eq!(Monitor::named("bogus"), None);
    }

    #[test]
    fn menus_choose_the_session_of_their_tab() {
        let mut app = fresh();
        let first = app.stack().active_id().unwrap();
        app.add_session(SessionType::Single, &Here);
        assert_eq!(app.menu_opened(first), [Effect::UpdateActions]);
        assert_eq!(app.context_session(), Some(first));
        assert!(app.session_actions().contains(&("move-session-right", false, true)));
        app.menu_closed();
        assert_ne!(app.context_session(), Some(first));
    }

    #[test]
    fn window_settings_are_stored() {
        let table = Table::default();
        let mut app = fresh();
        assert_eq!(app.set_keep_open(&table, true), [Effect::PublishSettings]);
        assert!(app.settings().keep_open);
        app.set_window_size(&table, "width", 70);
        app.set_window_size(&table, "height", 40);
        app.set_window_size(&table, "screen", 2);
        assert!(app.set_window_size(&table, "depth", 1).is_empty());
        let stored = Settings::load(&table);
        assert_eq!(
            (stored.width, stored.height, stored.screen, stored.keep_open),
            (70, 40, 2, true)
        );
    }

    #[test]
    fn actions_change_sessions_and_the_window() {
        let table = Table::default();
        let mut app = fresh();
        let trigger = |app: &mut App, name: &str, checked: bool| app.trigger(name, checked, &table, &Here);
        assert!(trigger(&mut app, "toggle-window-state", false).contains(&Effect::WindowVisible(true)));
        assert_eq!(trigger(&mut app, "view-full-screen", false), [Effect::Fullscreen(true)]);
        for (name, count) in [
            ("new-session", 2),
            ("new-session-two-horizontal", 3),
            ("new-session-two-vertical", 4),
            ("new-session-quad", 5),
        ] {
            trigger(&mut app, name, false);
            assert_eq!(app.stack().count(), count, "{name}");
        }
        let quad = app.stack().active_id().unwrap();
        assert_eq!(app.stack().session(quad).map(|found| found.terminals.len()), Some(4));
        trigger(&mut app, "previous-session", false);
        assert_ne!(app.stack().active_id(), Some(quad));
        trigger(&mut app, "next-session", false);
        trigger(&mut app, "move-session-left", false);
        assert_eq!(app.stack().tabs().iter().position(|id| *id == quad), Some(3));
        trigger(&mut app, "move-session-right", false);
        assert_eq!(app.stack().tabs().last(), Some(&quad));
        assert_eq!(trigger(&mut app, "rename-session", false), [Effect::RenameTab(quad)]);
        trigger(&mut app, "split-auto", false);
        trigger(&mut app, "split-left-right", false);
        trigger(&mut app, "split-top-bottom", false);
        assert_eq!(app.stack().session(quad).map(|found| found.terminals.len()), Some(7));
        for name in [
            "grow-terminal-right",
            "grow-terminal-left",
            "grow-terminal-top",
            "grow-terminal-bottom",
        ] {
            assert_eq!(trigger(&mut app, name, false), [Effect::Publish], "{name}");
        }
        let terminal = app.active_terminal().unwrap();
        trigger(&mut app, "enlarge-font", false);
        trigger(&mut app, "enlarge-font", false);
        trigger(&mut app, "shrink-font", false);
        assert_eq!(app.model().font_zoom, [(terminal, 1)]);
        trigger(&mut app, "reset-font-size", false);
        assert!(app.model().font_zoom.is_empty());
        trigger(&mut app, "next-terminal", false);
        assert_ne!(app.active_terminal(), Some(terminal));
        trigger(&mut app, "previous-terminal", false);
        assert_eq!(app.active_terminal(), Some(terminal));
        trigger(&mut app, "toggle-session-prevent-closing", true);
        assert!(!app.stack().session(quad).unwrap().closable);
        assert!(question(&trigger(&mut app, "close-active-terminal", false)).is_some());
        assert!(question(&trigger(&mut app, "close-session", false)).is_some());
        trigger(&mut app, "toggle-session-prevent-closing", false);
        trigger(&mut app, "close-active-terminal", false);
        assert_eq!(app.stack().session(quad).map(|found| found.terminals.len()), Some(6));
        trigger(&mut app, "close-session", false);
        assert!(app.stack().session(quad).is_none());
        trigger(&mut app, "switch-to-session-1", false);
        assert_eq!(app.stack().active_id(), app.stack().tabs().first().copied());
        assert!(trigger(&mut app, "switch-to-session-x", false).is_empty());
        assert!(trigger(&mut app, "no-such-action", false).is_empty());
    }

    #[test]
    fn actions_switch_monitors_of_the_session() {
        let table = Table::default();
        let mut app = fresh();
        let terminal = app.active_terminal().unwrap();
        for name in [
            "toggle-session-keyboard-input",
            "toggle-session-monitor-activity",
            "toggle-session-monitor-silence",
            "toggle-session-monitor-process",
            "toggle-session-monitor-once",
        ] {
            app.trigger(name, true, &table, &Here);
        }
        let state = app.stack().terminal_state(terminal).unwrap();
        assert!(!state.keyboard_input_enabled);
        assert!(state.monitor_activity && state.monitor_silence && state.monitor_process && state.monitor_once);
    }

    #[test]
    fn actions_store_the_size_and_reach_the_front_end() {
        let table = Table::default();
        let mut app = app_with(&[("Width", "50"), ("Height", "50")]);
        let trigger = |app: &mut App, name: &str, checked: bool| app.trigger(name, checked, &table, &Here);
        trigger(&mut app, "increase-window-width", false);
        trigger(&mut app, "increase-window-height", false);
        trigger(&mut app, "increase-window-height", false);
        trigger(&mut app, "decrease-window-height", false);
        trigger(&mut app, "decrease-window-width", false);
        trigger(&mut app, "decrease-window-width", false);
        assert_eq!((app.settings().width, app.settings().height), (40, 60));
        trigger(&mut app, "keep-open", false);
        assert!(!app.settings().keep_open);
        assert_eq!(
            trigger(&mut app, "options_configure", false),
            [Effect::OpenSettings(None)]
        );
        assert_eq!(
            trigger(&mut app, "edit-profile", false),
            [Effect::OpenSettings(Some("terminal"))]
        );
        assert_eq!(trigger(&mut app, "file_quit", false), [Effect::Quit]);
        let terminal = app.active_terminal().unwrap();
        assert_eq!(
            trigger(&mut app, "edit_copy", false),
            [Effect::TerminalAction {
                terminal,
                action: "edit_copy".into()
            }]
        );
        let commands = [
            ("adjust-history", TerminalCommand::AdjustHistory),
            ("allow-mouse-tracking", TerminalCommand::MouseTracking(true)),
            ("open-browser", TerminalCommand::OpenFolder),
            ("web-search", TerminalCommand::WebSearch),
            ("clear-history-and-reset", TerminalCommand::Reset),
            ("file_save_as", TerminalCommand::SaveOutput),
            ("file_print", TerminalCommand::Print),
            ("auto-save", TerminalCommand::AutoSave(true)),
        ];
        for (name, command) in commands {
            assert_eq!(
                trigger(&mut app, name, true),
                [Effect::Terminal { terminal, command }],
                "{name}"
            );
        }
        let mut wide = app_with(&[("Width", "100")]);
        assert!(wide.trigger("increase-window-width", false, &table, &Here).is_empty());
    }
}
