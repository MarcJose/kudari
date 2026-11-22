// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! `kudari mcp` on KDE Plasma: the server of the core, which reaches the running Kudari
//! over its `io.github.marcjose.kudari.Agent` D-Bus interface (see `agent.rs`).

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;
use zbus::block_on;

use kudari_core::mcp::{Instance, serve};

/// How long `open_session` waits for the shell of the new terminal.
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a tool waits for the user to allow the agent. Many clients give up on a tool
/// after a minute.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(50);
const ANSWER_POLL: Duration = Duration::from_millis(250);

pub fn run() -> i32 {
    match Dbus::connect() {
        Ok(kudari) => serve(&kudari, std::io::stdin().lock(), std::io::stdout().lock()),
        Err(error) => {
            eprintln!("kudari mcp: D-Bus is not available: {error}");
            1
        }
    }
}

/// Every call carries `no_autostart`: a call to a name nobody owns would else start Kudari
/// from its service file, which is not for an agent to decide, also when the user quits
/// Kudari while a call of the agent waits for an answer and tries again.
#[zbus::proxy(
    interface = "io.github.marcjose.kudari.Agent",
    default_service = "io.github.marcjose.kudari",
    default_path = "/kudari/agent"
)]
trait Agent {
    #[zbus(no_autostart)]
    fn terminals(&self) -> zbus::Result<String>;
    #[zbus(name = "readTerminal", no_autostart)]
    fn read_terminal(&self, terminal: i32, lines: u32) -> zbus::Result<String>;
    #[zbus(name = "sendText", no_autostart)]
    fn send_text(&self, terminal: i32, text: &str, execute: bool) -> zbus::Result<String>;
    #[zbus(name = "sendKey", no_autostart)]
    fn send_key(&self, terminal: i32, key: &str) -> zbus::Result<()>;
    #[zbus(name = "openSession", no_autostart)]
    fn open_session(&self, title: &str) -> zbus::Result<String>;
    #[zbus(name = "closeSession", no_autostart)]
    fn close_session(&self, session: i32) -> zbus::Result<()>;
}

/// How long one call waits for its answer: Kudari answers within a second, also when its
/// window is busy, so only a Kudari that hangs takes longer.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// What a tool says when Kudari does not run.
const NOT_RUNNING: &str =
    "Kudari is not running. The user can start it from the application menu or with the command kudari.";

/// The running Kudari, reached over the session bus.
struct Dbus {
    agent: AgentProxy<'static>,
}

impl Dbus {
    fn connect() -> zbus::Result<Self> {
        let connection = block_on(
            zbus::connection::Builder::session()
                .method_timeout(CALL_TIMEOUT)
                .build(),
        )?;
        let agent = block_on(
            AgentProxy::builder(&connection)
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build(),
        )?;
        Ok(Self { agent })
    }
}

/// Whether Kudari is still asking the user about this agent.
fn waiting(result: &zbus::Result<impl Sized>) -> bool {
    matches!(result, Err(zbus::Error::FDO(error)) if matches!(**error, zbus::fdo::Error::InteractiveAuthorizationRequired(_)))
}

/// Reads a new terminal until its shell runs, which Kudari tells with a plain failure, or
/// until [`START_TIMEOUT`] passed. Another answer, as when reading is switched off, says
/// nothing about the start, and waiting for it would only cost time.
fn wait_for_start(read: impl Fn() -> zbus::Result<String>) {
    let start = Instant::now();
    while matches!(read(), Err(zbus::Error::FDO(ref error)) if matches!(**error, zbus::fdo::Error::Failed(_)))
        && start.elapsed() < START_TIMEOUT
    {
        sleep(Duration::from_millis(100));
    }
}

/// Calls the interface, and while Kudari asks the user about the agent, calls again until
/// the user answered or [`ANSWER_TIMEOUT`] passed.
fn patiently<T>(call: impl Fn() -> zbus::Result<T>) -> Result<T, String> {
    let start = Instant::now();
    loop {
        let result = call();
        if !waiting(&result) || start.elapsed() >= ANSWER_TIMEOUT {
            return result.map_err(reason);
        }
        sleep(ANSWER_POLL);
    }
}

/// The sentence the interface answered with, or what went wrong on the way.
fn reason(error: zbus::Error) -> String {
    use zbus::DBusError;
    if not_running(&error) {
        return NOT_RUNNING.to_string();
    }
    match error {
        zbus::Error::FDO(error) => error.description().map_or_else(|| error.to_string(), str::to_string),
        zbus::Error::MethodError(_, Some(detail), _) => detail,
        other => other.to_string(),
    }
}

/// Whether the bus answered that nobody has the name of Kudari, which a call without autostart
/// gets when Kudari does not run.
fn not_running(error: &zbus::Error) -> bool {
    use zbus::DBusError;
    let name = match error {
        zbus::Error::FDO(error) => error.name().to_string(),
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        _ => return false,
    };
    matches!(
        name.as_str(),
        "org.freedesktop.DBus.Error.ServiceUnknown" | "org.freedesktop.DBus.Error.NameHasNoOwner"
    )
}

impl Instance for Dbus {
    fn terminals(&self) -> Result<String, String> {
        let agent = &self.agent;
        patiently(|| block_on(agent.terminals()))
    }

    fn read_terminal(&self, terminal: i32, lines: u32) -> Result<String, String> {
        let agent = &self.agent;
        patiently(|| block_on(agent.read_terminal(terminal, lines)))
    }

    fn send_text(&self, terminal: i32, text: &str, execute: bool) -> Result<String, String> {
        let agent = &self.agent;
        patiently(|| block_on(agent.send_text(terminal, text, execute)))
    }

    fn send_key(&self, terminal: i32, key: &str) -> Result<(), String> {
        let agent = &self.agent;
        patiently(|| block_on(agent.send_key(terminal, key)))
    }

    /// Returns once the shell of the new terminal runs, so the next call can use it.
    fn open_session(&self, title: &str) -> Result<String, String> {
        let agent = &self.agent;
        let opened = patiently(|| block_on(agent.open_session(title)))?;
        let terminal = serde_json::from_str::<Value>(&opened)
            .ok()
            .and_then(|value| value.get("terminal").and_then(Value::as_i64))
            .and_then(|id| i32::try_from(id).ok());
        if let Some(terminal) = terminal {
            wait_for_start(|| block_on(agent.read_terminal(terminal, 1)));
        }
        Ok(opened)
    }

    fn close_session(&self, session: i32) -> Result<(), String> {
        let agent = &self.agent;
        patiently(|| block_on(agent.close_session(session)))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn failing(error: zbus::fdo::Error) -> zbus::Result<String> {
        Err(zbus::Error::FDO(Box::new(error)))
    }

    #[test]
    fn nobody_with_the_name_means_kudari_does_not_run() {
        let unknown = zbus::Error::FDO(Box::new(zbus::fdo::Error::ServiceUnknown(
            "The name is not activatable".into(),
        )));
        assert_eq!(reason(unknown), NOT_RUNNING);
        let no_owner = zbus::Error::FDO(Box::new(zbus::fdo::Error::NameHasNoOwner("gone".into())));
        assert_eq!(reason(no_owner), NOT_RUNNING);
        // What Kudari answers stays as it said it.
        let denied = zbus::Error::FDO(Box::new(zbus::fdo::Error::AccessDenied(
            "Reading terminals is switched off".into(),
        )));
        assert_eq!(reason(denied), "Reading terminals is switched off");
    }

    #[test]
    fn only_a_starting_terminal_is_waited_for() {
        // Reading switched off: no waiting at all.
        let start = Instant::now();
        let calls = Cell::new(0);
        wait_for_start(|| {
            calls.set(calls.get() + 1);
            failing(zbus::fdo::Error::AccessDenied(
                "Reading terminals is switched off".into(),
            ))
        });
        assert_eq!(calls.get(), 1);
        assert!(start.elapsed() < Duration::from_secs(1));
        // A terminal that is still starting, then runs.
        let calls = Cell::new(0);
        wait_for_start(|| {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                failing(zbus::fdo::Error::Failed("The terminal is still starting.".into()))
            } else {
                Ok(String::new())
            }
        });
        assert_eq!(calls.get(), 3);
    }
}
