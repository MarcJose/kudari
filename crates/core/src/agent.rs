// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What AI agents may do in Kudari, whatever the way they reach it.
//!
//! Three settings under Settings → Security decide: reading the terminals, controlling them,
//! and whether the user confirms every command. While commands need confirmation, an agent
//! types only at the shell prompt and never presses Return, so nothing runs that the user
//! has not seen. On top of the settings, the user allows each new agent: an agent is one
//! connection, normally one `kudari mcp`, and the answer holds until it disconnects.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::app::{App, Effect, Host};
use crate::i18n::{tr, tr_arg};
use crate::input::{self, Key, Modifiers};
use crate::sessions::{SessionId, SessionType, TerminalId};

/// The longest text an agent can type at once, in characters.
pub const MAX_TEXT: usize = 4096;
/// The most lines an agent can read at once.
pub const MAX_LINES: u32 = 10_000;
/// The longest tab title an agent can set.
pub const MAX_TITLE: usize = 200;

/// The keys an agent can press. Return is not among them: it runs a command, which only
/// `sendText` with `execute` does, and only when the user does not confirm commands.
pub const KEYS: [&str; 16] = [
    "ctrl+c",
    "ctrl+d",
    "ctrl+l",
    "ctrl+z",
    "escape",
    "tab",
    "backspace",
    "delete",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "page_up",
    "page_down",
];

/// What the terminal receives for a key of [`KEYS`].
#[must_use]
pub fn key_bytes(name: &str, app_cursor: bool) -> Option<Vec<u8>> {
    let control = |byte: u8| Some(vec![byte]);
    let key = match name {
        "ctrl+c" => return control(0x03),
        "ctrl+d" => return control(0x04),
        "ctrl+l" => return control(0x0c),
        "ctrl+z" => return control(0x1a),
        "escape" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "home" => Key::Home,
        "end" => Key::End,
        "left" => Key::Left,
        "up" => Key::Up,
        "right" => Key::Right,
        "down" => Key::Down,
        "page_up" => Key::PageUp,
        "page_down" => Key::PageDown,
        _ => return None,
    };
    input::key_sequence(key, Modifiers::NONE, app_cursor).map(String::into_bytes)
}

/// Why a text cannot be typed as it is.
#[must_use]
pub fn text_refusal(text: &str) -> Option<String> {
    if text.is_empty() {
        Some("The text is empty.".to_string())
    } else if text.chars().count() > MAX_TEXT {
        Some(format!("The text is longer than {MAX_TEXT} characters."))
    } else if text.chars().any(char::is_control) {
        Some(
            "The text contains control characters, such as line breaks or tabs. Type one line \
             at a time, and press keys with send_key."
                .to_string(),
        )
    } else {
        None
    }
}

const LOCKED: &str = "The user has locked keyboard input in this terminal.";
const BUSY: &str = "A program is running in this terminal. While Kudari asks before running the \
                    commands of agents, they can type only at the shell prompt; ctrl+c stops the program.";
const NOT_EDITING: &str = "The shell is not editing a command line now, for example while a script asks a \
                           question. While Kudari asks before running the commands of agents, they type \
                           only into the line editor of the shell, which bash, zsh and fish show with \
                           bracketed paste and PowerShell with the marks of the shell integration of \
                           Kudari, outside the vi mode of PSReadLine; shells without either, such as \
                           dash or the Command Prompt, need the user to switch off the confirmation of \
                           commands.";
const ESCAPE: &str = "escape could form a key binding of the shell together with what follows, such as \
                      Alt+A, which runs the line in zsh, so it needs the user to switch off the \
                      confirmation of commands.";

/// What an agent needs to know about a terminal before it types.
#[derive(Clone, Copy)]
pub struct Prompt {
    /// The user confirms the commands of agents.
    pub confirm: bool,
    /// A program other than the shell runs in the foreground.
    pub busy: bool,
    /// The line editor of the shell is active: the terminal is in bracketed paste mode, or
    /// the marks of the shell say that the user types a command.
    pub editing: bool,
    /// The program asked for bracketed paste, so text goes in as a paste.
    pub bracketed: bool,
    pub keyboard_input: bool,
}

/// Why an agent may not type into a terminal now. While commands need confirmation, it
/// types only into the line editor of the shell: the shell is in the foreground, and it
/// edits a command line rather than running a script that reads a key.
#[must_use]
pub fn typing_refusal(prompt: Prompt) -> Option<&'static str> {
    if !prompt.keyboard_input {
        Some(LOCKED)
    } else if prompt.confirm && prompt.busy {
        Some(BUSY)
    } else if prompt.confirm && !prompt.editing {
        Some(NOT_EDITING)
    } else {
        None
    }
}

/// Why an agent may not press `key` now. Ctrl+C only stops what runs, so it always works;
/// Ctrl+D would end the shell, and Escape starts the Alt bindings of line editors, so both
/// need commands without confirmation.
#[must_use]
pub fn key_refusal(key: &str, prompt: Prompt) -> Option<String> {
    if !KEYS.contains(&key) {
        return Some(format!("Unknown key {key:?}; the keys are {}.", KEYS.join(", ")));
    }
    if !prompt.keyboard_input {
        return Some(LOCKED.to_string());
    }
    if key == "ctrl+c" {
        return None;
    }
    if prompt.confirm && key == "ctrl+d" {
        return Some(
            "ctrl+d could end the shell, so it needs the user to switch off the confirmation of \
             commands."
                .to_string(),
        );
    }
    if prompt.confirm && key == "escape" {
        return Some(ESCAPE.to_string());
    }
    typing_refusal(prompt).map(str::to_string)
}

/// What to write for a text, and what to tell the agent about it. A line editor that asked
/// for bracketed paste gets the text as a paste, so it inserts it literally instead of
/// treating characters as key bindings, as the command mode of vi would. Without it, the text
/// goes in as keys only where the signed marks of PowerShell say that `PSReadLine` edits the
/// line, which marks no command line in its vi mode, and inserts printable keys elsewhere.
#[must_use]
pub fn typed(text: &str, execute: bool, confirm: bool, bracketed: bool) -> (Vec<u8>, &'static str) {
    let mut bytes = Vec::with_capacity(text.len() + 13);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    bytes.extend_from_slice(text.as_bytes());
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    let outcome = match (execute, confirm) {
        (false, _) => "Typed the text.",
        (true, true) => {
            "Typed the command. Kudari asks before running the commands of agents, so it runs \
             when the user presses Return in the terminal; read the terminal to see when it did."
        }
        (true, false) => {
            bytes.push(b'\r');
            "Typed and ran the command."
        }
    };
    (bytes, outcome)
}

/// What the user decided about an agent.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Denied,
    /// The user is being asked, about this agent or another one.
    Waiting,
    /// Nobody is being asked yet, so the user should be asked about this agent now.
    Ask,
}

/// The agents the user allowed or denied, by the name of their connection, such as the
/// unique name on D-Bus. Names are never reused, so a program that connects later has to
/// ask for itself.
#[derive(Default)]
pub struct Access {
    allowed: BTreeSet<String>,
    denied: BTreeSet<String>,
    asking: Option<String>,
}

impl Access {
    /// What applies to `agent`. Only one agent is asked about at a time; the others wait.
    pub fn check(&mut self, agent: &str) -> Decision {
        if self.allowed.contains(agent) {
            Decision::Allowed
        } else if self.denied.contains(agent) {
            Decision::Denied
        } else if self.asking.is_some() {
            Decision::Waiting
        } else {
            self.asking = Some(agent.to_string());
            Decision::Ask
        }
    }

    /// The agent the user is being asked about.
    #[must_use]
    pub fn asking(&self) -> Option<&str> {
        self.asking.as_deref()
    }

    /// Records the answer about `agent`, unless the question was withdrawn meanwhile.
    pub fn answer(&mut self, agent: &str, allowed: bool) -> bool {
        if self.asking.as_deref() != Some(agent) {
            return false;
        }
        self.asking = None;
        let decided = agent.to_string();
        if allowed {
            self.allowed.insert(decided);
        } else {
            self.denied.insert(decided);
        }
        true
    }

    /// Forgets an agent that disconnected. Returns whether the user was being asked about it.
    pub fn gone(&mut self, agent: &str) -> bool {
        self.allowed.remove(agent);
        self.denied.remove(agent);
        if self.asking.as_deref() == Some(agent) {
            self.asking = None;
            true
        } else {
            false
        }
    }

    /// Ends the access of the allowed agents. They are not asked about again.
    pub fn revoke(&mut self) {
        let allowed = std::mem::take(&mut self.allowed);
        self.denied.extend(allowed);
    }

    /// Forgets every answer. Returns whether the user was being asked.
    pub fn clear(&mut self) -> bool {
        self.allowed.clear();
        self.denied.clear();
        self.asking.take().is_some()
    }

    /// Whether any agent may use Kudari.
    #[must_use]
    pub fn active(&self) -> bool {
        !self.allowed.is_empty()
    }
}

/// The text of the question about an agent.
#[must_use]
pub fn question_text(reading: bool, control: bool, program: Option<&str>) -> String {
    let program = program.map_or_else(|| tr("@info", "an unknown program"), str::to_string);
    let wants = match (reading, control) {
        (true, true) => tr_arg(
            "@info",
            "An AI agent wants to read and control your terminals. The request comes from %1.",
            &program,
        ),
        (false, true) => tr_arg(
            "@info",
            "An AI agent wants to control your terminals. The request comes from %1.",
            &program,
        ),
        _ => tr_arg(
            "@info",
            "An AI agent wants to read your terminals. The request comes from %1.",
            &program,
        ),
    };
    let advice = tr(
        "@info",
        "Allow this only if you started the agent yourself. Its access lasts until the agent ends, or until you end it with the eye button in the bar.",
    );
    // KDE gives translations with arguments as rich text, with the argument escaped.
    let paragraph = |text: &str| {
        let inner = text
            .strip_prefix("<html>")
            .and_then(|rest| rest.strip_suffix("</html>"));
        format!("<p>{}</p>", inner.unwrap_or(text))
    };
    format!("<html>{}{}</html>", paragraph(&wants), paragraph(&advice))
}

/// A running terminal, as agents use it.
pub trait Terminal {
    /// Columns and lines.
    fn size(&self) -> (usize, usize);
    /// The title in the `TitleFormat` setting.
    fn title(&self, format: &str) -> String;
    fn working_directory(&self) -> Option<PathBuf>;
    /// The program in the foreground.
    fn process_name(&self) -> Option<String>;
    /// A program other than the shell runs in the foreground.
    fn is_busy(&self) -> bool;
    /// The line editor of the shell is active: the terminal is in bracketed paste mode.
    fn bracketed_paste(&self) -> bool;
    /// The marks of the shell say that the user types a command; see
    /// [`crate::marks::Marks::at_command_line`].
    fn at_command_line(&self) -> bool;
    /// The program asked for the cursor keys of applications.
    fn app_cursor(&self) -> bool;
    /// The last `lines` lines, or the screen for zero.
    fn recent_text(&self, lines: usize) -> String;
    /// Writes as if the user typed, which scrolls to the bottom.
    fn write_user_input(&self, bytes: Vec<u8>);
}

/// What an agent asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// The sessions and their terminals, as JSON.
    Terminals,
    /// The last lines of a terminal; `-1` is the active terminal, as in the other interfaces.
    Read { terminal: i32, lines: u32 },
    /// Types one line, and with `execute` also Return, unless the user confirms commands.
    Type { terminal: i32, text: String, execute: bool },
    /// Presses one of [`KEYS`].
    Key { terminal: i32, key: String },
    /// Opens a session for the agent, which it may close again. Answers with the ids of
    /// the session and its terminal, as JSON.
    Open { title: String },
    /// Closes a session the agent opened.
    Close { session: SessionId },
}

/// Why a request failed, with a sentence for the agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The settings or the user do not allow it.
    Denied(String),
    /// The user is being asked about the agent, which should try again in a moment.
    Waiting,
    /// The request is wrong.
    Invalid(String),
    /// Kudari cannot do it now.
    Failed(String),
}

const WAITING: &str = "Kudari is asking the user whether this agent may use it. Ask the user to answer \
                       the question of Kudari.";
const DENIED: &str = "The user did not allow this agent to use Kudari, or ended its access. Kudari asks \
                      again when the agent connects anew.";

impl Error {
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Denied(text) | Self::Invalid(text) | Self::Failed(text) => text,
            Self::Waiting => WAITING,
        }
    }
}

fn switched_off(what: &str) -> Error {
    Error::Denied(format!(
        "{what} is switched off in Kudari. The user can switch it on under Settings → Security → AI Agents."
    ))
}

impl App {
    /// Answers a request of `agent`, the name of its connection, or of a caller without a
    /// name, which is refused. The first request of an agent asks the user.
    pub fn agent_request(
        &mut self,
        agent: Option<&str>,
        request: Request,
        host: &dyn Host,
    ) -> (Result<String, Error>, Vec<Effect>) {
        let mut effects = Vec::new();
        let reading = matches!(request, Request::Terminals | Request::Read { .. });
        let result = self
            .admit(agent, reading, &mut effects)
            .and_then(|()| self.serve_agent(request, host, &mut effects));
        (result, effects)
    }

    fn admit(&mut self, agent: Option<&str>, reading: bool, effects: &mut Vec<Effect>) -> Result<(), Error> {
        if reading && !self.settings().allow_agent_reading {
            return Err(switched_off("Reading terminals"));
        }
        if !reading && !self.settings().allow_agent_control {
            return Err(switched_off("Controlling terminals"));
        }
        let agent = agent.ok_or_else(|| Error::Denied(DENIED.to_string()))?;
        match self.agents_mut().check(agent) {
            Decision::Allowed => Ok(()),
            Decision::Denied => Err(Error::Denied(DENIED.to_string())),
            Decision::Waiting => Err(Error::Waiting),
            Decision::Ask => {
                effects.push(Effect::AskAgent(agent.to_string()));
                Err(Error::Waiting)
            }
        }
    }

    fn serve_agent(&mut self, request: Request, host: &dyn Host, effects: &mut Vec<Effect>) -> Result<String, Error> {
        match request {
            Request::Terminals => Ok(self.terminals_json(host).to_string()),
            Request::Read { terminal, lines } => {
                let (_, running) = self.running(terminal, host)?;
                Ok(running.recent_text(lines.min(MAX_LINES) as usize))
            }
            Request::Type {
                terminal,
                text,
                execute,
            } => {
                if let Some(refusal) = text_refusal(&text) {
                    return Err(Error::Invalid(refusal));
                }
                let (id, running) = self.running(terminal, host)?;
                let state = self.prompt(id, running.as_ref());
                if let Some(refusal) = typing_refusal(state) {
                    return Err(Error::Denied(refusal.to_string()));
                }
                let (bytes, outcome) = typed(&text, execute, state.confirm, state.bracketed);
                running.write_user_input(bytes);
                Ok(outcome.to_string())
            }
            Request::Key { terminal, key } => {
                let (id, running) = self.running(terminal, host)?;
                if let Some(refusal) = key_refusal(&key, self.prompt(id, running.as_ref())) {
                    return Err(Error::Denied(refusal));
                }
                if let Some(bytes) = key_bytes(&key, running.app_cursor()) {
                    running.write_user_input(bytes);
                }
                Ok(String::new())
            }
            Request::Open { title } => {
                if title.chars().count() > MAX_TITLE || title.chars().any(char::is_control) {
                    return Err(Error::Invalid(format!(
                        "A title has at most {MAX_TITLE} characters and no control characters."
                    )));
                }
                let (session, added) = self.add_session(SessionType::Single, host);
                effects.extend(added);
                self.add_agent_session(session);
                if !title.is_empty() {
                    self.stack_mut().set_tab_title(session, &title, true);
                    effects.extend(self.refresh());
                }
                let terminal = self.stack().session(session).map_or(-1, |found| found.active_terminal);
                Ok(json!({ "session": session, "terminal": terminal }).to_string())
            }
            Request::Close { session } => {
                if !self.is_agent_session(session) {
                    return Err(Error::Denied(
                        "Agents may close only the sessions they opened themselves.".to_string(),
                    ));
                }
                effects.extend(self.remove_session(session));
                Ok(String::new())
            }
        }
    }

    /// The terminal an agent means, with its program.
    fn running(&self, terminal: i32, host: &dyn Host) -> Result<(TerminalId, std::sync::Arc<dyn Terminal>), Error> {
        let found = if terminal == -1 {
            self.stack().active().map(|session| session.active_terminal)
        } else {
            Some(terminal).filter(|id| self.stack().session_for_terminal(*id).is_some())
        };
        let id = found
            .ok_or_else(|| Error::Invalid(format!("There is no terminal {terminal}; list_terminals names them.")))?;
        let running = host
            .terminal(id)
            .ok_or_else(|| Error::Failed("The terminal is still starting; try again in a moment.".to_string()))?;
        Ok((id, running))
    }

    fn keyboard_input(&self, terminal: TerminalId) -> bool {
        self.stack()
            .terminal_state(terminal)
            .is_none_or(|state| state.keyboard_input_enabled)
    }

    fn prompt(&self, terminal: TerminalId, running: &dyn Terminal) -> Prompt {
        Prompt {
            confirm: self.settings().confirm_agent_commands,
            busy: running.is_busy(),
            editing: running.bracketed_paste() || running.at_command_line(),
            bracketed: running.bracketed_paste(),
            keyboard_input: self.keyboard_input(terminal),
        }
    }

    /// The sessions in the order of their tabs, with their terminals.
    fn terminals_json(&self, host: &dyn Host) -> Value {
        let active = self.stack().active_id();
        let format = &self.settings().title_format;
        let sessions: Vec<Value> = self
            .stack()
            .sessions()
            .map(|session| {
                let terminals: Vec<Value> = session
                    .terminal_ids()
                    .into_iter()
                    .map(|id| {
                        let active = id == session.active_terminal;
                        let keyboard = self.keyboard_input(id);
                        let Some(running) = host.terminal(id) else {
                            return json!({ "id": id, "active": active, "keyboard_input": keyboard, "starting": true });
                        };
                        let (columns, lines) = running.size();
                        json!({
                            "id": id,
                            "active": active,
                            "keyboard_input": keyboard,
                            "title": running.title(format),
                            "directory": running.working_directory(),
                            "process": running.process_name(),
                            "busy": running.is_busy(),
                            "columns": columns,
                            "lines": lines,
                        })
                    })
                    .collect();
                json!({
                    "id": session.id,
                    "title": self.stack().tab_title(session.id).unwrap_or_default(),
                    "active": Some(session.id) == active,
                    "closable": session.closable,
                    "opened_by_agent": self.is_agent_session(session.id),
                    "terminals": terminals,
                })
            })
            .collect();
        json!({ "sessions": sessions })
    }

    /// The text of the question about `agent`, which comes from `program`, or `None` if the
    /// user is no longer to be asked about it.
    #[must_use]
    pub fn agent_question(&self, agent: &str, program: Option<&str>) -> Option<String> {
        let settings = self.settings();
        (self.agents().asking() == Some(agent))
            .then(|| question_text(settings.allow_agent_reading, settings.allow_agent_control, program))
    }

    /// The user answered the question about `agent`.
    pub fn agent_answered(&mut self, agent: &str, allowed: bool) -> Vec<Effect> {
        if self.agents_mut().answer(agent, allowed) {
            vec![Effect::AgentAccess(self.agents().active())]
        } else {
            Vec::new()
        }
    }

    /// A connection ended; if it was an agent, its access ends with it.
    pub fn agent_gone(&mut self, agent: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.agents_mut().gone(agent) {
            effects.push(Effect::CloseAgentQuestion);
        }
        effects.push(Effect::AgentAccess(self.agents().active()));
        effects
    }

    /// The user ended the access of the agents.
    pub fn revoke_agents(&mut self) -> Vec<Effect> {
        self.agents_mut().revoke();
        vec![Effect::AgentAccess(false)]
    }

    /// The settings for agents changed, so every agent has to ask again.
    pub fn forget_agents(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.agents_mut().clear() {
            effects.push(Effect::CloseAgentQuestion);
        }
        effects.push(Effect::AgentAccess(false));
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_bytes() {
        for key in KEYS {
            assert!(key_bytes(key, false).is_some_and(|bytes| !bytes.is_empty()), "{key}");
        }
        assert_eq!(key_bytes("up", false).unwrap(), b"\x1b[A");
        assert_eq!(key_bytes("up", true).unwrap(), b"\x1bOA");
        assert_eq!(key_bytes("ctrl+c", true).unwrap(), b"\x03");
        assert_eq!(key_bytes("backspace", false).unwrap(), b"\x7f");
        assert_eq!(key_bytes("page_down", false).unwrap(), b"\x1b[6~");
        assert_eq!(key_bytes("enter", false), None);
        assert_eq!(key_bytes("return", false), None);
    }

    #[test]
    fn only_plain_lines_are_typed() {
        assert_eq!(text_refusal("ls -l"), None);
        assert_eq!(text_refusal("echo grüße ✓"), None);
        assert!(text_refusal("").is_some());
        for text in ["ls\n", "ls\rrm", "a\tb", "\x1b[201~", "\u{9b}2J", "x\x7f"] {
            assert!(text_refusal(text).is_some(), "{text:?}");
        }
        assert_eq!(text_refusal(&"a".repeat(MAX_TEXT)), None);
        assert!(text_refusal(&"a".repeat(MAX_TEXT + 1)).is_some());
    }

    /// A shell at its prompt, with confirmation on.
    const AT_PROMPT: Prompt = Prompt {
        confirm: true,
        busy: false,
        editing: true,
        bracketed: true,
        keyboard_input: true,
    };

    #[test]
    fn confirmation_keeps_agents_at_the_prompt() {
        assert_eq!(typing_refusal(AT_PROMPT), None);
        assert_eq!(
            typing_refusal(Prompt {
                busy: true,
                ..AT_PROMPT
            }),
            Some(BUSY)
        );
        // A script of the shell reading a key: the shell runs, but edits no command line.
        assert_eq!(
            typing_refusal(Prompt {
                editing: false,
                bracketed: false,
                ..AT_PROMPT
            }),
            Some(NOT_EDITING)
        );
        let unconfirmed = Prompt {
            confirm: false,
            busy: true,
            editing: false,
            bracketed: false,
            ..AT_PROMPT
        };
        assert_eq!(typing_refusal(unconfirmed), None);
        assert_eq!(
            typing_refusal(Prompt {
                keyboard_input: false,
                ..unconfirmed
            }),
            Some(LOCKED)
        );
    }

    #[test]
    fn keys_follow_the_same_rules() {
        let unconfirmed = Prompt {
            confirm: false,
            ..AT_PROMPT
        };
        let running = Prompt {
            busy: true,
            ..AT_PROMPT
        };
        assert!(key_refusal("enter", unconfirmed).unwrap().contains("ctrl+c"));
        // Stopping a program always works, unless the user locked the terminal.
        assert_eq!(key_refusal("ctrl+c", running), None);
        let locked = Prompt {
            keyboard_input: false,
            ..unconfirmed
        };
        assert_eq!(key_refusal("ctrl+c", locked).as_deref(), Some(LOCKED));
        assert!(key_refusal("ctrl+d", AT_PROMPT).is_some());
        assert_eq!(
            key_refusal(
                "ctrl+d",
                Prompt {
                    busy: true,
                    ..unconfirmed
                }
            ),
            None
        );
        // Escape and a letter would make Alt bindings, such as Alt+A, which runs the line in zsh.
        assert_eq!(key_refusal("escape", AT_PROMPT).as_deref(), Some(ESCAPE));
        assert_eq!(key_refusal("escape", unconfirmed), None);
        assert_eq!(key_refusal("up", AT_PROMPT), None);
        assert_eq!(key_refusal("up", running).as_deref(), Some(BUSY));
        assert_eq!(
            key_refusal(
                "tab",
                Prompt {
                    editing: false,
                    bracketed: false,
                    ..AT_PROMPT
                }
            )
            .as_deref(),
            Some(NOT_EDITING)
        );
    }

    #[test]
    fn the_user_decides_about_each_agent() {
        let mut access = Access::default();
        assert_eq!(access.check(":1.5"), Decision::Ask);
        // One question at a time: the first agent and the others wait for the answer.
        assert_eq!(access.check(":1.5"), Decision::Waiting);
        assert_eq!(access.check(":1.6"), Decision::Waiting);
        assert!(!access.answer(":1.6", true));
        assert!(access.answer(":1.5", true));
        assert!(access.active());
        assert_eq!(access.check(":1.5"), Decision::Allowed);
        // An allowed agent does not let in a program that connects later.
        assert_eq!(access.check(":1.6"), Decision::Ask);
        assert!(access.answer(":1.6", false));
        assert_eq!(access.check(":1.6"), Decision::Denied);
        assert!(!access.answer(":1.6", true));
        assert_eq!(access.check(":1.6"), Decision::Denied);
    }

    #[test]
    fn access_ends_with_the_agent_or_the_user() {
        let mut access = Access::default();
        access.check(":1.5");
        access.answer(":1.5", true);
        access.revoke();
        assert!(!access.active());
        assert_eq!(access.check(":1.5"), Decision::Denied);
        assert!(!access.gone(":1.5"));
        assert_eq!(access.check(":1.7"), Decision::Ask);
        // A withdrawn question takes no late answer.
        assert!(access.gone(":1.7"));
        assert!(!access.answer(":1.7", true));
        assert_eq!(access.check(":1.8"), Decision::Ask);
        assert!(access.clear());
        assert!(!access.answer(":1.8", true));
        assert_eq!(access.check(":1.8"), Decision::Ask);
        access.answer(":1.8", true);
        assert!(!access.clear());
        assert_eq!(access.check(":1.8"), Decision::Ask);
    }

    #[test]
    fn return_is_left_to_the_user_while_commands_need_confirmation() {
        assert_eq!(typed("ls", false, false, false).0, b"ls");
        assert_eq!(typed("ls", true, true, false).0, b"ls");
        assert!(typed("ls", true, true, true).1.contains("presses Return"));
        assert_eq!(
            typed("ls", true, false, false),
            (b"ls\r".to_vec(), "Typed and ran the command.")
        );
    }

    #[test]
    fn line_editors_get_the_text_as_a_paste() {
        assert_eq!(typed("ls -l", true, true, true).0, b"\x1b[200~ls -l\x1b[201~");
        assert_eq!(typed("ls", true, false, true).0, b"\x1b[200~ls\x1b[201~\r");
        // The text cannot end the paste early: control characters never get this far.
        assert!(text_refusal("\x1b[201~a").is_some());
    }

    mod requests {
        use std::cell::RefCell;
        use std::collections::HashMap;
        use std::sync::Arc;

        use super::super::*;
        use crate::settings::{Settings, Store};

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

        #[derive(Default)]
        struct Shell {
            busy: bool,
            editing: bool,
            /// Marks of shell integration say the user types a command.
            marked: bool,
            written: std::sync::Mutex<Vec<u8>>,
        }

        impl Terminal for Shell {
            fn size(&self) -> (usize, usize) {
                (80, 24)
            }

            fn title(&self, format: &str) -> String {
                format!("title {format}")
            }

            fn working_directory(&self) -> Option<PathBuf> {
                Some(PathBuf::from("/work"))
            }

            fn process_name(&self) -> Option<String> {
                Some("zsh".into())
            }

            fn is_busy(&self) -> bool {
                self.busy
            }

            fn bracketed_paste(&self) -> bool {
                self.editing
            }

            fn at_command_line(&self) -> bool {
                self.marked
            }

            fn app_cursor(&self) -> bool {
                false
            }

            fn recent_text(&self, lines: usize) -> String {
                format!("{lines} lines")
            }

            fn write_user_input(&self, bytes: Vec<u8>) {
                self.written.lock().unwrap().extend(bytes);
            }
        }

        /// Terminal 0 runs `shell`; the others are still starting.
        struct Running(Arc<Shell>);

        impl Host for Running {
            fn working_directory(&self, _terminal: TerminalId) -> Option<PathBuf> {
                None
            }

            fn has_focus(&self) -> bool {
                true
            }

            fn terminal(&self, terminal: TerminalId) -> Option<std::sync::Arc<dyn Terminal>> {
                (terminal == 0).then(|| self.0.clone() as Arc<dyn Terminal>)
            }
        }

        fn running(busy: bool, editing: bool) -> Running {
            Running(Arc::new(Shell {
                busy,
                editing,
                ..Shell::default()
            }))
        }

        fn app_with(values: &[(&str, &str)]) -> App {
            let table = Table::default();
            table.write("", "FirstRun", "false");
            for (key, value) in values {
                table.write("", key, value);
            }
            let mut app = App::new(Settings::load(&table));
            let _ = app.start();
            app
        }

        /// Reading and control allowed, commands confirmed, and agent `a` let in.
        fn allowed() -> App {
            let mut app = app_with(&[("AllowAgentReading", "true"), ("AllowAgentControl", "true")]);
            let _ = app.agent_request(Some("a"), Request::Terminals, &running(false, true));
            let _ = app.agent_answered("a", true);
            app
        }

        fn ask(app: &mut App, request: Request, host: &Running) -> Result<String, Error> {
            app.agent_request(Some("a"), request, host).0
        }

        #[test]
        fn the_settings_come_first() {
            let mut app = app_with(&[]);
            let (result, effects) = app.agent_request(Some("a"), Request::Terminals, &running(false, true));
            assert!(
                matches!(result, Err(Error::Denied(text)) if text.starts_with("Reading terminals is switched off"))
            );
            assert!(effects.is_empty());
            let (result, _) =
                app.agent_request(Some("a"), Request::Open { title: String::new() }, &running(false, true));
            assert!(matches!(result, Err(Error::Denied(text)) if text.starts_with("Controlling terminals")));
        }

        #[test]
        fn the_first_request_asks_the_user_and_others_wait() {
            let mut app = app_with(&[("AllowAgentReading", "true")]);
            let host = running(false, true);
            let (result, effects) = app.agent_request(Some("a"), Request::Terminals, &host);
            assert_eq!(
                (result, effects),
                (Err(Error::Waiting), vec![Effect::AskAgent("a".into())])
            );
            assert_eq!(
                app.agent_request(Some("b"), Request::Terminals, &host),
                (Err(Error::Waiting), vec![])
            );
            assert!(app.agent_question("a", Some("claude")).unwrap().contains("claude"));
            assert_eq!(app.agent_question("b", None), None);
            assert_eq!(app.agent_answered("a", true), [Effect::AgentAccess(true)]);
            assert!(ask(&mut app, Request::Terminals, &host).is_ok());
            assert_eq!(app.agent_answered("a", false), []);
            let (result, _) = app.agent_request(None, Request::Terminals, &host);
            assert_eq!(result, Err(Error::Denied(DENIED.into())));
            assert_eq!(Error::Waiting.message(), WAITING);
        }

        #[test]
        fn the_list_shows_sessions_and_terminals() {
            let mut app = allowed();
            let list: Value =
                serde_json::from_str(&ask(&mut app, Request::Terminals, &running(true, false)).unwrap()).unwrap();
            let terminal = &list["sessions"][0]["terminals"][0];
            assert_eq!(list["sessions"][0]["active"], true);
            assert_eq!(list["sessions"][0]["opened_by_agent"], false);
            assert_eq!(terminal["process"], "zsh");
            assert_eq!(terminal["busy"], true);
            assert_eq!(terminal["columns"], 80);
            assert_eq!(terminal["keyboard_input"], true);
            assert!(terminal["title"].as_str().unwrap().starts_with("title"));
        }

        #[test]
        fn reading_names_a_terminal_that_runs() {
            let mut app = allowed();
            let host = running(false, true);
            assert_eq!(
                ask(&mut app, Request::Read { terminal: -1, lines: 3 }, &host),
                Ok("3 lines".into())
            );
            let many = ask(
                &mut app,
                Request::Read {
                    terminal: 0,
                    lines: u32::MAX,
                },
                &host,
            );
            assert_eq!(many, Ok(format!("{MAX_LINES} lines")));
            assert!(matches!(
                ask(&mut app, Request::Read { terminal: 9, lines: 0 }, &host),
                Err(Error::Invalid(_))
            ));
            let (second, _) = app.split(0, Some(crate::sessions::Orientation::Horizontal), &host);
            let second = second.unwrap();
            assert!(matches!(
                ask(
                    &mut app,
                    Request::Read {
                        terminal: second,
                        lines: 0
                    },
                    &host
                ),
                Err(Error::Failed(_))
            ));
        }

        #[test]
        fn typing_follows_the_rules_of_the_prompt() {
            let mut app = allowed();
            let host = running(false, true);
            let typed = ask(
                &mut app,
                Request::Type {
                    terminal: 0,
                    text: "ls".into(),
                    execute: true,
                },
                &host,
            );
            assert!(typed.unwrap().starts_with("Typed the command."));
            assert_eq!(*host.0.written.lock().unwrap(), b"\x1b[200~ls\x1b[201~");
            let empty = ask(
                &mut app,
                Request::Type {
                    terminal: 0,
                    text: String::new(),
                    execute: false,
                },
                &host,
            );
            assert!(matches!(empty, Err(Error::Invalid(_))));
            let busy = running(true, true);
            let refused = ask(
                &mut app,
                Request::Type {
                    terminal: 0,
                    text: "ls".into(),
                    execute: false,
                },
                &busy,
            );
            assert_eq!(refused, Err(Error::Denied(BUSY.into())));
            assert!(busy.0.written.lock().unwrap().is_empty());
        }

        #[test]
        fn marks_of_the_prompt_let_agents_type_without_a_paste() {
            let mut app = allowed();
            let host = Running(Arc::new(Shell {
                marked: true,
                ..Shell::default()
            }));
            let typed = ask(
                &mut app,
                Request::Type {
                    terminal: 0,
                    text: "dir".into(),
                    execute: true,
                },
                &host,
            );
            assert!(typed.unwrap().starts_with("Typed the command."));
            assert_eq!(*host.0.written.lock().unwrap(), b"dir");
            let unmarked = running(false, false);
            let refused = ask(
                &mut app,
                Request::Type {
                    terminal: 0,
                    text: "dir".into(),
                    execute: false,
                },
                &unmarked,
            );
            assert_eq!(refused, Err(Error::Denied(NOT_EDITING.into())));
        }

        #[test]
        fn keys_follow_the_same_rules() {
            let mut app = allowed();
            let host = running(true, false);
            assert_eq!(
                ask(
                    &mut app,
                    Request::Key {
                        terminal: 0,
                        key: "ctrl+c".into()
                    },
                    &host
                ),
                Ok(String::new())
            );
            assert_eq!(*host.0.written.lock().unwrap(), [0x03]);
            let refused = ask(
                &mut app,
                Request::Key {
                    terminal: 0,
                    key: "up".into(),
                },
                &host,
            );
            assert_eq!(refused, Err(Error::Denied(BUSY.into())));
        }

        #[test]
        fn agents_close_only_their_own_sessions() {
            let mut app = allowed();
            let host = running(false, true);
            let opened = ask(&mut app, Request::Open { title: "build".into() }, &host).unwrap();
            let opened: Value = serde_json::from_str(&opened).unwrap();
            let session = i32::try_from(opened["session"].as_i64().unwrap()).unwrap();
            assert!(app.is_agent_session(session));
            assert_eq!(app.stack().tab_title(session), Some("build"));
            assert_eq!(
                opened["terminal"],
                app.stack().session(session).unwrap().active_terminal
            );
            let long = ask(
                &mut app,
                Request::Open {
                    title: "x".repeat(MAX_TITLE + 1),
                },
                &host,
            );
            assert!(matches!(long, Err(Error::Invalid(_))));
            assert!(matches!(
                ask(&mut app, Request::Close { session: 0 }, &host),
                Err(Error::Denied(_))
            ));
            assert_eq!(ask(&mut app, Request::Close { session }, &host), Ok(String::new()));
            assert!(app.stack().session(session).is_none());
        }

        #[test]
        fn access_ends_with_the_agent_or_the_user() {
            let mut app = allowed();
            let host = running(false, true);
            assert_eq!(app.revoke_agents(), [Effect::AgentAccess(false)]);
            assert_eq!(
                ask(&mut app, Request::Terminals, &host),
                Err(Error::Denied(DENIED.into()))
            );
            assert_eq!(app.agent_gone("a"), [Effect::AgentAccess(false)]);
            let (_, effects) = app.agent_request(Some("a"), Request::Terminals, &host);
            assert_eq!(effects, [Effect::AskAgent("a".into())]);
            assert_eq!(
                app.agent_gone("a"),
                [Effect::CloseAgentQuestion, Effect::AgentAccess(false)]
            );
            let _ = app.agent_request(Some("b"), Request::Terminals, &host);
            assert_eq!(
                app.forget_agents(),
                [Effect::CloseAgentQuestion, Effect::AgentAccess(false)]
            );
            assert_eq!(app.forget_agents(), [Effect::AgentAccess(false)]);
        }
    }
}
