// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The part of Kudari that depends on no user interface and no platform. The front ends,
//! today the one for KDE Plasma, build on it; see `docs/architecture.md`.

// The settings table in `Settings::to_config_json` exceeds the default macro depth.
#![recursion_limit = "512"]

pub mod actions;
pub mod agent;
pub mod app;
pub mod colors;
pub mod command;
#[cfg(any(test, feature = "fuzz"))]
pub mod fuzz;
pub mod gettext;
pub mod i18n;
pub mod ini;
pub mod input;
pub mod keytab;
pub mod linedraw;
pub mod links;
pub mod marks;
pub mod mcp;
pub mod menus;
pub mod options;
pub mod pipe;
pub mod process;
pub mod remote;
pub mod render;
pub mod services;
pub mod sessions;
pub mod settings;
pub mod shells;
pub mod terminal;
pub mod theme;
pub mod title;
pub mod view;
