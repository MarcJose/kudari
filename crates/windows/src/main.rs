// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Kudari for Windows: the shared Qt parts with pages in the `FluentWinUI3` style, settings
//! in an INI file and the translations of `po/` built in. It builds on Linux as well, where
//! the Win32 parts are missing, so that it can be tried out there.
//!
//! On Windows it is a program without a console, so that starting it opens no console
//! window; the commands of the command line write to the console they came from.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod agents;
mod frontend;
mod win;

use cxx_qt_lib::{QQmlApplicationEngine, QUrl};
use cxx_qt_lib_extras::QApplication;

include!(concat!(env!("OUT_DIR"), "/catalogues.rs"));

const HELP: &str = "Usage: kudari [OPTION]
       kudari mcp

A drop-down terminal. A second start shows or hides the window of the first one.

  mcp          serve the Model Context Protocol on standard input and output, through
               which AI agents use the running Kudari
  --help       show this help
  --version    show the version";

fn main() {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("mcp") => {
            win::rk_win_attach_console();
            std::process::exit(agents::run_mcp());
        }
        Some("--version" | "-v") => {
            win::rk_win_attach_console();
            println!("kudari {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--help" | "-h") => {
            win::rk_win_attach_console();
            println!("{HELP}");
            return;
        }
        _ => {}
    }
    win::rk_win_prepare_translucency();
    let mut app = QApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_application_version(&cxx_qt_lib::QString::from(env!("CARGO_PKG_VERSION")));
    }
    win::rk_win_use_style();
    // As with gettext, LANGUAGE replaces the languages of the system; "en" means none.
    let languages: Vec<String> = match std::env::var("LANGUAGE") {
        Ok(chosen) if !chosen.is_empty() => chosen.split(':').map(str::to_string).collect(),
        _ => kudari_qt::qt::rk_ui_languages()
            .to_string()
            .split(',')
            .map(str::to_string)
            .collect(),
    };
    if let Some(catalogue) = kudari_core::gettext::choose(&languages, CATALOGUES) {
        kudari_core::gettext::install(catalogue);
    }
    kudari_qt::frontend::install(&frontend::Windows);
    if !win::rk_win_claim_instance(&cxx_qt_lib::QString::from(&instance_name())) {
        return;
    }
    start_services();
    agents::listen();

    // For checks: `--screenshot=FILE` saves the window as an image and quits.
    if let Some(path) = std::env::args().find_map(|argument| argument.strip_prefix("--screenshot=").map(str::to_string))
    {
        win::rk_win_screenshot_later(&cxx_qt_lib::QString::from(&path), 3000);
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from(
            "qrc:/qt/qml/io/github/marcjose/kudari/windows/qml/Main.qml",
        ));
    }
    let code = app.as_mut().map_or(1, |app| app.exec());
    std::process::exit(code);
}

/// What names this instance for another start: on Windows the name of a mutex in the login
/// session and of a hidden window, elsewhere a socket in the private runtime directory.
fn instance_name() -> String {
    let user: String = kudari_core::process::user()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    if cfg!(windows) {
        return format!("KudariInstance-{user}");
    }
    let directory = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| kudari_core::process::home().map(|home| home.join(".cache")))
        .unwrap_or_default();
    directory.join("kudari-windows").to_string_lossy().into_owned()
}

/// The tray icon and the global shortcut, as the settings have them.
fn start_services() {
    use kudari_core::i18n::tr;
    use kudari_core::services::GlobalShortcut;

    let qs = kudari_qt::qt::qs;
    // The texts of the menus of KDE, which po/ translates already.
    win::rk_win_setup_tray(
        &qs(&tr("@action", "Show/Hide Kudari")),
        &qs(&tr("@info:tooltip Quits the application", "Quit")),
    );
    let settings = kudari_core::settings::Settings::load(kudari_qt::frontend::frontend().store());
    win::rk_win_set_tray_visible(settings.show_systray_icon);
    win::rk_win_watch_theme();
    let toggle = frontend::Windows.toggle();
    if !win::rk_win_register_hotkey(&qs(&toggle)) {
        eprintln!("kudari: Windows refused the shortcut {toggle}");
    }
}
