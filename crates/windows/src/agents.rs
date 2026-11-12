// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The agent interface of Kudari for Windows: the named pipe of [`kudari_core::pipe`],
//! whose requests go to the controller on the Qt thread, and `kudari mcp`, which
//! connects to it. What agents may do is decided in [`kudari_core::agent`].
//!
//! Any program of the user can open the pipe, so the settings are not enough: Kudari also
//! asks the user before each new agent gets access, and names the program that connected.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use kudari_core::agent::{Error, Request};
use kudari_core::pipe::{self, Handler};
use kudari_qt::controller::queue;

/// How long a request waits for the Qt thread.
const QT_TIMEOUT: Duration = Duration::from_secs(5);

/// The processes of the agents that are connected, for the question to the user.
static PROCESSES: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

fn processes<T>(action: impl FnOnce(&mut HashMap<String, u32>) -> T) -> T {
    let mut processes = PROCESSES.lock().unwrap_or_else(PoisonError::into_inner);
    action(processes.get_or_insert_with(HashMap::new))
}

struct Agents;

impl Handler for Agents {
    fn connected(&self, agent: &str, process: Option<u32>) {
        if let Some(process) = process {
            processes(|processes| processes.insert(agent.to_string(), process));
        }
    }

    fn request(&self, agent: &str, request: Request) -> Result<String, Error> {
        let (sender, receiver) = mpsc::channel();
        let agent = agent.to_string();
        queue(move |controller| {
            let _ = sender.send(controller.agent_request(Some(&agent), request));
        });
        receiver
            .recv_timeout(QT_TIMEOUT)
            .unwrap_or_else(|_| Err(Error::Failed("Kudari did not answer in time.".to_string())))
    }

    fn gone(&self, agent: &str) {
        processes(|processes| processes.remove(agent));
        let agent = agent.to_string();
        queue(move |controller| controller.agent_gone(&agent));
    }
}

/// Listens for agents. Without the pipe, they cannot reach Kudari, which works on.
pub fn listen() {
    let listening = pipe::address().and_then(|address| pipe::serve(&address, Arc::new(Agents)));
    if let Err(error) = listening {
        eprintln!("kudari: agents cannot connect: {error}");
    }
}

/// Asks the user whether `agent` may use Kudari, with the name of its program.
pub fn ask_user(agent: String) {
    let process = processes(|processes| processes.get(&agent).copied());
    // Finding the program reads the list of processes, which takes a moment.
    std::thread::spawn(move || {
        let own = std::env::current_exe().ok();
        let program = process.and_then(|process| kudari_core::process::program(process, own.as_deref()));
        queue(move |mut controller| {
            // The question is part of the window, which shows for it.
            if controller.as_mut().ask_about_agent(&agent, program.as_deref()) && !*controller.window_visible() {
                controller.toggle_window();
            }
        });
    });
}

/// Withdraws the question about an agent that went.
pub fn close_question() {
    queue(kudari_qt::controller::qobject::Controller::agent_question_closed);
}

/// `kudari mcp`: the server of the Model Context Protocol on standard input and
/// output, for the Kudari of this user.
pub fn run_mcp() -> i32 {
    let address = match pipe::address() {
        Ok(address) => address,
        Err(error) => {
            eprintln!("kudari mcp: {error}");
            return 1;
        }
    };
    // In English, as everything an agent hears.
    let client = pipe::Client::new(
        address,
        "Kudari is not running. The user can start it from the Start menu.",
    );
    kudari_core::mcp::serve(&client, std::io::stdin().lock(), std::io::stdout().lock())
}
