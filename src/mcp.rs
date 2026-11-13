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

#[zbus::proxy(
    interface = "io.github.marcjose.kudari.Agent",
    default_service = "io.github.marcjose.kudari",
    default_path = "/kudari/agent"
)]
trait Agent {
    fn terminals(&self) -> zbus::Result<String>;
    #[zbus(name = "readTerminal")]
    fn read_terminal(&self, terminal: i32, lines: u32) -> zbus::Result<String>;
    #[zbus(name = "sendText")]
    fn send_text(&self, terminal: i32, text: &str, execute: bool) -> zbus::Result<String>;
    #[zbus(name = "sendKey")]
    fn send_key(&self, terminal: i32, key: &str) -> zbus::Result<()>;
    #[zbus(name = "openSession")]
    fn open_session(&self, title: &str) -> zbus::Result<String>;
    #[zbus(name = "closeSession")]
    fn close_session(&self, session: i32) -> zbus::Result<()>;
}

/// The running Kudari, reached over the session bus.
struct Dbus {
    connection: zbus::Connection,
}

impl Dbus {
    fn connect() -> zbus::Result<Self> {
        Ok(Self {
            connection: block_on(zbus::Connection::session())?,
        })
    }

    /// The interface of the running instance. A call to a name nobody owns would start
    /// Kudari from its service file, which is not for an agent to decide.
    fn agent(&self) -> Result<AgentProxy<'_>, String> {
        let text = |error: zbus::Error| error.to_string();
        let bus = block_on(zbus::fdo::DBusProxy::new(&self.connection)).map_err(text)?;
        let service = zbus::names::BusName::try_from(crate::dbus::SERVICE).map_err(|error| error.to_string())?;
        if !block_on(bus.name_has_owner(service)).map_err(|error| error.to_string())? {
            return Err(
                "Kudari is not running. The user can start it from the application menu or with the command kudari."
                    .to_string(),
            );
        }
        block_on(
            AgentProxy::builder(&self.connection)
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build(),
        )
        .map_err(text)
    }
}

/// Whether Kudari is still asking the user about this agent.
fn waiting(result: &zbus::Result<impl Sized>) -> bool {
    matches!(result, Err(zbus::Error::FDO(error)) if matches!(**error, zbus::fdo::Error::InteractiveAuthorizationRequired(_)))
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
    match error {
        zbus::Error::FDO(error) => error.description().map_or_else(|| error.to_string(), str::to_string),
        zbus::Error::MethodError(_, Some(detail), _) => detail,
        other => other.to_string(),
    }
}

impl Instance for Dbus {
    fn terminals(&self) -> Result<String, String> {
        let agent = self.agent()?;
        patiently(|| block_on(agent.terminals()))
    }

    fn read_terminal(&self, terminal: i32, lines: u32) -> Result<String, String> {
        let agent = self.agent()?;
        patiently(|| block_on(agent.read_terminal(terminal, lines)))
    }

    fn send_text(&self, terminal: i32, text: &str, execute: bool) -> Result<String, String> {
        let agent = self.agent()?;
        patiently(|| block_on(agent.send_text(terminal, text, execute)))
    }

    fn send_key(&self, terminal: i32, key: &str) -> Result<(), String> {
        let agent = self.agent()?;
        patiently(|| block_on(agent.send_key(terminal, key)))
    }

    /// Returns once the shell of the new terminal runs, so the next call can use it.
    fn open_session(&self, title: &str) -> Result<String, String> {
        let agent = self.agent()?;
        let opened = patiently(|| block_on(agent.open_session(title)))?;
        let terminal = serde_json::from_str::<Value>(&opened)
            .ok()
            .and_then(|value| value.get("terminal").and_then(Value::as_i64))
            .and_then(|id| i32::try_from(id).ok());
        if let Some(terminal) = terminal {
            let start = Instant::now();
            while block_on(agent.read_terminal(terminal, 1)).is_err() && start.elapsed() < START_TIMEOUT {
                sleep(Duration::from_millis(100));
            }
        }
        Ok(opened)
    }

    fn close_session(&self, session: i32) -> Result<(), String> {
        let agent = self.agent()?;
        patiently(|| block_on(agent.close_session(session)))
    }
}
