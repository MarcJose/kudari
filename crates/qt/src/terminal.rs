// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The terminals of the core, heard on the thread of Qt.

pub use kudari_core::terminal::*;

use crate::frontend::frontend;
use crate::services::Clipboard;
use crate::sessions::TerminalId;

/// Hands what the terminals report to the controller, on the thread of Qt.
struct Qt;

impl Listener for Qt {
    fn run(&self, work: Box<dyn FnOnce() + Send>) {
        crate::controller::queue(move |_| work());
    }

    fn report(&self, terminal: TerminalId, report: Report) {
        crate::controller::queue(move |controller| match report {
            Report::Output(pending) => {
                drop(pending);
                controller.terminal_output(terminal);
            }
            Report::Bell => controller.terminal_bell(terminal),
            Report::Exited => controller.terminal_exited(terminal),
        });
    }

    fn clipboard(&self) -> &dyn Clipboard {
        frontend()
    }
}

/// Lets the terminals report to the controller.
pub fn listen() {
    set_listener(&Qt);
}

/// Opens a link, with `:line:column` after a file going to the configured editor.
pub fn open_link(link: &str) {
    kudari_core::terminal::open_link(link, frontend());
}
