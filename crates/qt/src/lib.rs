// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The Qt parts that every Qt front end of Kudari shares: the terminal item that paints and
//! takes input, the terminal backend, the controller for QML and the keys of Qt. A front end
//! installs itself with [`frontend::install`]; see `docs/architecture.md`.

pub mod controller;
pub mod frontend;
pub mod qt;
pub mod qt_input;
#[cfg_attr(
    not(any(test, feature = "test-hooks")),
    allow(dead_code, reason = "only the GUI test guards against calls back")
)]
mod reentry;
pub mod terminal;
pub mod terminal_item;

// The modules of the core, under the names these modules have always used for them.
use kudari_core::{app, colors, input, keytab, process, render, services, sessions, settings, stats, theme, view};
