// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What Kudari asks of Windows. Qt answers most of it; the global shortcut goes through
//! `RegisterHotKey`, and notifications through the tray icon. The menus are those of
//! `kudari_core::menus`, which the window shows in its own controls. The backdrop of
//! Windows 11 follows.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, PoisonError};

use kudari_core::app::Question;
use kudari_core::ini::IniFile;
use kudari_core::services::{Clipboard, Desktop, GlobalShortcut, Notifications, Screens, Selection, Themes, Window};
use kudari_core::settings::{SYSTEM_SKIN, Store};
use kudari_core::theme::Palette;
use kudari_qt::frontend::{Feature, Frontend, Skin};
use kudari_qt::qt::{self, qs};

use crate::win;

/// The global shortcut without a setting. F12, as on KDE, belongs to the debugger on Windows.
pub const DEFAULT_TOGGLE: &str = "Ctrl+F12";

/// Windows, or another system the program was built for to try it out.
pub struct Windows;

/// `%APPDATA%\Kudari\kudari.ini`, elsewhere `kudari-windows.ini` in the configuration.
fn settings_path() -> PathBuf {
    if let Some(data) = std::env::var_os("APPDATA") {
        return PathBuf::from(data).join("Kudari").join("kudari.ini");
    }
    let configuration = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| kudari_core::process::home().map(|home| home.join(".config")))
        .unwrap_or_default();
    configuration.join("kudari-windows.ini")
}

static SETTINGS: LazyLock<IniFile> = LazyLock::new(|| IniFile::open(&settings_path()));

/// What the app last said about its actions, checked and enabled, for the menus.
static ACTIONS: LazyLock<Mutex<HashMap<String, (bool, bool)>>> = LazyLock::new(Mutex::default);

/// The state of the main menu: the width and height of the window.
static MENU: LazyLock<Mutex<serde_json::Value>> = LazyLock::new(Mutex::default);

struct Recorded;

impl kudari_core::menus::States for Recorded {
    /// Actions the app never mentioned are enabled and not checked.
    fn state(&self, name: &str) -> (bool, bool) {
        let actions = ACTIONS.lock().unwrap_or_else(PoisonError::into_inner);
        actions.get(name).copied().unwrap_or((false, true))
    }
}

/// The signals the menu offers: Windows has no others.
const SIGNALS: &[kudari_core::process::Signal] = if cfg!(windows) {
    &[
        kudari_core::process::Signal::Interrupt,
        kudari_core::process::Signal::Terminate,
        kudari_core::process::Signal::Kill,
    ]
} else {
    &kudari_core::menus::SIGNALS
};

impl Clipboard for Windows {
    fn copy(&self, text: &str, target: Selection) {
        qt::rk_clipboard_set(&qs(text), target == Selection::Primary);
    }

    fn copy_rich(&self, text: &str, html: &str) {
        qt::rk_clipboard_set_rich(&qs(text), &qs(html));
    }

    fn paste(&self, source: Selection) -> String {
        qt::rk_clipboard_get(source == Selection::Primary).to_string()
    }
}

impl Notifications for Windows {
    fn notify(&self, _event: &str, text: &str) {
        win::rk_win_notify(&qs("Kudari"), &qs(text));
    }
}

impl Desktop for Windows {
    /// Windows opens files with the program it has for them, which starts programs too, so
    /// whatever comes here passes the check of the core once more.
    fn open_url(&self, url: &str) {
        let schemes = kudari_core::terminal::options().unwrap_or_default().link_schemes;
        if let Some(target) = kudari_core::links::openable(url, &schemes) {
            qt::rk_open_url_with_qt(&qs(&target));
        }
    }

    /// With the search engine of the `WebSearchUrl` setting, through the same check as links.
    fn web_search(&self, text: &str) {
        let template = SETTINGS.read("Terminal", "WebSearchUrl", kudari_core::settings::DEFAULT_WEB_SEARCH);
        if let Some(url) = kudari_core::links::web_search(&template, text) {
            self.open_url(&url);
        }
    }
}

impl Themes for Windows {
    /// Windows has one theme, whose colors Qt knows.
    fn palette(&self, theme: Option<&str>) -> Option<Palette> {
        theme
            .is_none()
            .then(|| kudari_core::theme::parse_palette(&qt::rk_palette_colors().to_string()))
            .flatten()
    }

    fn installed(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

impl Screens for Windows {
    fn outputs(&self) -> Vec<String> {
        let names = qt::rk_screen_names().to_string();
        names
            .split(',')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    fn pointer_output(&self) -> Option<String> {
        Some(qt::rk_pointer_screen().to_string()).filter(|name| !name.is_empty())
    }

    fn active_output(&self) -> String {
        qt::rk_primary_screen().to_string()
    }

    fn work_area(&self, output: &str) -> Option<(i32, i32)> {
        let size = qt::rk_screen_available_size(&qs(output)).to_string();
        let (width, height) = size.split_once(',')?;
        Some((width.parse().ok()?, height.parse().ok()?))
    }
}

impl Window for Windows {
    /// The window slides in through its QML.
    fn slide(&self) {}

    fn set_blur(&self, enabled: bool) {
        win::rk_win_set_backdrop(enabled);
    }

    fn set_color_scheme(&self, scheme: &str) {
        win::rk_win_set_color_scheme(&qs(scheme));
    }
}

impl GlobalShortcut for Windows {
    fn toggle(&self) -> String {
        SETTINGS.read("Shortcuts", "toggle-window-state", DEFAULT_TOGGLE)
    }

    /// Keeps the shortcut before if Windows refuses the new one, which another program holds.
    fn set_toggle(&self, sequence: &str) -> bool {
        if win::rk_win_register_hotkey(&qs(sequence)) {
            SETTINGS.write("Shortcuts", "toggle-window-state", sequence);
            true
        } else {
            eprintln!("kudari: Windows refused the shortcut {sequence}");
            win::rk_win_register_hotkey(&qs(&self.toggle()));
            false
        }
    }

    fn foreign_toggle(&self) -> String {
        String::new()
    }
}

impl Frontend for Windows {
    fn store(&self) -> &dyn Store {
        &*SETTINGS
    }

    fn set_action_state(&self, name: &str, checked: bool, enabled: bool) {
        let mut actions = ACTIONS.lock().unwrap_or_else(PoisonError::into_inner);
        actions.insert(name.to_string(), (checked, enabled));
    }

    fn action_text(&self, name: &str) -> String {
        kudari_core::menus::action_text(name).unwrap_or_default()
    }

    fn shortcuts_json(&self) -> String {
        kudari_core::actions::shortcuts_json(&*SETTINGS)
    }

    fn reload_shortcuts(&self) {}

    fn set_shortcut(&self, name: &str, sequences: &[String]) {
        kudari_core::actions::set_sequences(&*SETTINGS, name, sequences);
    }

    fn set_menu_state(&self, json: &str) {
        *MENU.lock().unwrap_or_else(PoisonError::into_inner) = serde_json::from_str(json).unwrap_or_default();
    }

    fn set_tray_visible(&self, visible: bool) {
        win::rk_win_set_tray_visible(visible);
    }

    fn highlight_color(&self) -> String {
        self.palette(None).map(|palette| palette.highlight).unwrap_or_default()
    }

    fn skins(&self) -> Vec<Skin> {
        vec![Skin {
            id: SYSTEM_SKIN.to_string(),
            name: kudari_core::i18n::tr("@item:inlistbox", "System theme"),
            description: String::new(),
        }]
    }

    fn popup_main_menu(&self, _x: i32, _y: i32) -> Option<String> {
        let menu = MENU.lock().unwrap_or_else(PoisonError::into_inner).clone();
        Some(kudari_core::menus::main_menu(&menu, &Recorded).to_string())
    }

    fn popup_session_menu(&self, _x: i32, _y: i32) -> Option<String> {
        Some(kudari_core::menus::session_menu(kudari_core::shells::installed(), &Recorded).to_string())
    }

    /// The app made the session of the tab the one its actions apply to.
    fn popup_tab_menu(&self, _x: i32, _y: i32, state: &str) -> Option<String> {
        let state: serde_json::Value = serde_json::from_str(state).unwrap_or_default();
        let session = state.get("session").and_then(serde_json::Value::as_i64).unwrap_or(-1);
        let color = state
            .get("color")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Some(kudari_core::menus::tab_menu_with_color(&Recorded, session, color).to_string())
    }

    fn popup_terminal_menu(&self, _x: i32, _y: i32, can_copy: bool, link: &str, auto_saving: bool) -> Option<String> {
        self.set_action_state("auto-save", auto_saving, true);
        let terminal = kudari_core::menus::Terminal {
            link,
            can_copy,
            signals: SIGNALS,
        };
        Some(kudari_core::menus::terminal_menu(&terminal, &Recorded).to_string())
    }

    fn popup_drop_menu(&self, _x: i32, _y: i32, _terminal: i32, _paths: &str, _directory: &str) {}

    /// The window asks in its QML.
    fn ask(&self, _question: &Question) -> bool {
        false
    }

    fn ask_agent(&self, agent: String) {
        crate::agents::ask_user(agent);
    }

    fn close_agent_question(&self) {
        crate::agents::close_question();
    }

    fn supports(&self, feature: Feature) -> bool {
        matches!(feature, Feature::Shells | Feature::ColorScheme)
    }

    fn can_import(&self) -> bool {
        false
    }

    fn import(&self) -> bool {
        false
    }
}
