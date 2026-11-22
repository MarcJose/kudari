// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The `io.github.marcjose.kudari.Agent` D-Bus interface, which `kudari mcp` offers AI
//! agents over the Model Context Protocol. What they may do is decided in
//! [`kudari_core::agent`]; this passes their requests on.
//!
//! Any program of the user can call the interface, so the settings are not enough: Kudari
//! also asks the user before each new agent gets access. An agent is one D-Bus connection,
//! named by its unique name. On Wayland, other programs cannot click the answer for the user.

// `#[interface]` needs methods with `self`, but all state lives on the Qt thread.
#![allow(clippy::unused_self)]

use std::pin::Pin;

use zbus::fdo::{Error, Result};
use zbus::interface;
use zbus::message::Header;

use crate::controller::{qobject::Controller, queue};
use crate::dbus::ask;
use crate::kde::{self, qs, tr};
use kudari_core::agent::{self, Request};

/// Asks the user whether `agent` may use Kudari, with the name of its program.
pub fn ask_user(agent: String) {
    // Finding the program asks the bus, which must not happen on the Qt thread. Without a
    // thread, as when the user has as many processes as allowed, the question comes without
    // the name of the program.
    let unnamed = agent.clone();
    let asked = std::thread::Builder::new().name("agent".to_string()).spawn(move || {
        let program = crate::dbus::process_id(&agent)
            .and_then(|pid| crate::process::program(pid, std::env::current_exe().ok().as_deref()));
        queue(move |c| show_question(c, &agent, program.as_deref()));
    });
    if asked.is_err() {
        queue(move |c| show_question(c, &unnamed, None));
    }
}

fn show_question(c: Pin<&mut Controller>, agent: &str, program: Option<&str>) {
    let Some(text) = c.app().agent_question(
        agent,
        agent::Asker {
            name: program,
            ..agent::Asker::default()
        },
    ) else {
        return;
    };
    kde::rk_ask_agent(
        &qs(agent),
        &qs(&tr("@title:window", "AI Agent")),
        &qs(&text),
        &qs(&tr("@action:button", "Allow")),
        &qs(&tr("@action:button", "Deny")),
    );
}

/// The error of D-Bus that `kudari mcp` understands for an error of the core.
fn to_dbus(error: agent::Error) -> Error {
    let message = error.message().to_string();
    match error {
        agent::Error::Denied(_) => Error::AccessDenied(message),
        agent::Error::Waiting => Error::InteractiveAuthorizationRequired(message),
        agent::Error::Invalid(_) => Error::InvalidArgs(message),
        agent::Error::Failed(_) => Error::Failed(message),
    }
}

/// Passes a request of the caller on to the core, on the Qt thread.
async fn request(header: &Header<'_>, request: Request) -> Result<String> {
    let caller = header.sender().map(ToString::to_string);
    ask(
        move |c| c.agent_request(caller.as_deref(), request).map_err(to_dbus),
        Err(Error::Failed("Kudari did not answer in time.".to_string())),
    )
    .await
}

pub struct Agent;

/// The methods of [`Request`].
#[interface(name = "io.github.marcjose.kudari.Agent")]
impl Agent {
    async fn terminals(&self, #[zbus(header)] header: Header<'_>) -> Result<String> {
        request(&header, Request::Terminals).await
    }

    #[zbus(name = "readTerminal")]
    async fn read_terminal(&self, #[zbus(header)] header: Header<'_>, terminal: i32, lines: u32) -> Result<String> {
        request(&header, Request::Read { terminal, lines }).await
    }

    /// Returns what happened, in words for the agent.
    #[zbus(name = "sendText")]
    async fn send_text(
        &self,
        #[zbus(header)] header: Header<'_>,
        terminal: i32,
        text: String,
        execute: bool,
    ) -> Result<String> {
        request(
            &header,
            Request::Type {
                terminal,
                text,
                execute,
            },
        )
        .await
    }

    #[zbus(name = "sendKey")]
    async fn send_key(&self, #[zbus(header)] header: Header<'_>, terminal: i32, key: String) -> Result<()> {
        request(&header, Request::Key { terminal, key }).await.map(drop)
    }

    #[zbus(name = "openSession")]
    async fn open_session(&self, #[zbus(header)] header: Header<'_>, title: String) -> Result<String> {
        request(&header, Request::Open { title }).await
    }

    #[zbus(name = "closeSession")]
    async fn close_session(&self, #[zbus(header)] header: Header<'_>, session: i32) -> Result<()> {
        request(&header, Request::Close { session }).await.map(drop)
    }
}
