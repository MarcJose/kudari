// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

mod agent;
mod config;
mod dbus;
// The test functions of the bridge are only called with the `test-hooks` feature.
#[cfg_attr(not(feature = "test-hooks"), allow(dead_code))]
mod kde;
mod mcp;
mod migrate;
mod plasma;

// The modules of the core, under the names the front end has always used for them.
#[cfg(test)]
use kudari_core::fuzz;
use kudari_core::{app, ini, process, services, sessions, settings, theme};

// The shared Qt parts, under the names this front end has always used for them.
use kudari_qt::{controller, terminal};

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};
use cxx_qt_lib_extras::QApplication;

fn main() {
    // An AI agent's client starts `kudari mcp`, which talks to the running instance and
    // needs no window of its own.
    if std::env::args_os().nth(1).is_some_and(|argument| argument == "mcp") {
        std::process::exit(mcp::run());
    }
    let mut app = QApplication::new();
    kudari_core::i18n::set_translator(kde::tr_arg);
    kudari_qt::frontend::install(&plasma::Plasma);
    // Handles --help and --version, which must work next to a running instance.
    kde::rk_command_line(&kde::qs(env!("CARGO_PKG_VERSION")));

    // A second start toggles the running instance.
    let connection = match dbus::serve() {
        Ok(Some(connection)) => Some(connection),
        Ok(None) => {
            if let Err(error) = dbus::toggle_running_instance() {
                eprintln!("kudari: could not reach the running instance: {error}");
                std::process::exit(1);
            }
            return;
        }
        Err(error) => {
            eprintln!("kudari: D-Bus is not available: {error}");
            None
        }
    };

    kde::rk_init();

    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        kde::rk_setup_qml_i18n(engine.as_mut());
        engine.load(&QUrl::from("qrc:/qt/qml/io/github/marcjose/kudari/qml/Main.qml"));
    }
    let show_tray = settings::Settings::load(&config::KConfig).show_systray_icon;
    kde::rk_finish_startup(show_tray);

    let code = app.as_mut().map_or(1, |app| app.exec());
    drop(connection);
    std::process::exit(code);
}
