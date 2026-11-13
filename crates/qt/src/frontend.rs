// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What the shared Qt parts ask of the front end they run in: the services of the platform,
//! the storage of the settings, and what only the shell of a front end has, such as its
//! menus, its actions with their shortcuts, its tray icon and its questions.
//!
//! The controller is a `QObject` of cxx-qt and cannot be generic, so the front end installs
//! itself with [`install`] before it loads the QML.

use std::sync::OnceLock;

use crate::app::Question;
use crate::services::{
    Clipboard, Desktop, GlobalShortcut, Notifications, Screens, Selection, Services, Themes, Window,
};
use crate::settings::Store;
use crate::theme::Palette;

/// A choice of the `Skin` setting, as the settings page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skin {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub trait Frontend: Services + Send + Sync {
    /// Where the settings live.
    fn store(&self) -> &dyn Store;

    /// Checks or enables an action of the menus and shortcuts, without triggering it.
    fn set_action_state(&self, name: &str, checked: bool, enabled: bool);
    /// The text of an action, without accelerator marks.
    fn action_text(&self, name: &str) -> String;
    /// The shortcuts of the actions, as JSON for the settings.
    fn shortcuts_json(&self) -> String;
    /// Reads the shortcuts again after they were written to the settings.
    fn reload_shortcuts(&self);
    /// The window size and screen as the menu shows them, as JSON.
    fn set_menu_state(&self, json: &str);
    fn set_tray_visible(&self, visible: bool);
    /// The color for highlighting a terminal, as `#rrggbb`.
    fn highlight_color(&self) -> String;
    /// The choices of the `Skin` setting, the theme of the system first.
    fn skins(&self) -> Vec<Skin>;

    // The menus show at `x`, `y` in the window. A front end without menus of its own returns
    // the menu as the data of `kudari_core::menus` instead, which the controller hands to
    // its QML with `menuRequested`.
    fn popup_main_menu(&self, x: i32, y: i32) -> Option<String>;
    fn popup_session_menu(&self, x: i32, y: i32) -> Option<String>;
    /// `state` describes the session and its terminals, as JSON.
    fn popup_tab_menu(&self, x: i32, y: i32, state: &str) -> Option<String>;
    fn popup_terminal_menu(&self, x: i32, y: i32, can_copy: bool, link: &str, auto_saving: bool) -> Option<String>;
    /// Offers what to do with dropped files; `paths` has one file per line.
    fn popup_drop_menu(&self, x: i32, y: i32, terminal: i32, paths: &str, directory: &str);

    /// Asks without blocking; the answer goes to `Controller::answer`. Returns false for a
    /// front end without dialogs of its own, which then asks in its QML.
    fn ask(&self, question: &Question) -> bool;
    /// Asks without blocking whether an AI agent may use Kudari.
    fn ask_agent(&self, agent: String);
    /// Withdraws the question about an agent.
    fn close_agent_question(&self);

    /// Whether settings of another terminal can be taken over.
    fn can_import(&self) -> bool;
    /// Takes them over. Returns false on failure.
    fn import(&self) -> bool;

    /// Gives an action other sequences, for a front end without a shortcut dialog of its own.
    fn set_shortcut(&self, name: &str, sequences: &[String]);

    /// Whether the front end has `feature`, so that the settings pages show what belongs to it.
    fn supports(&self, feature: Feature) -> bool;
}

/// What only some front ends have, by the names the settings pages ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// Themes of the desktop, with the border and background of the window.
    Skins,
    /// Programs that type into terminals over D-Bus.
    RemoteCommands,
    /// Konsole's `.keytab` files.
    KeyTables,
    /// A dialog of its own for the shortcuts.
    ShortcutDialog,
    /// A choice of shells, with the shell integration of Kudari.
    Shells,
    /// Light or dark by a setting of Kudari, where there are no themes.
    ColorScheme,
}

impl Feature {
    /// The feature of a name from QML.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "skins" => Self::Skins,
            "remote-commands" => Self::RemoteCommands,
            "key-tables" => Self::KeyTables,
            "shortcut-dialog" => Self::ShortcutDialog,
            "shells" => Self::Shells,
            "color-scheme" => Self::ColorScheme,
            _ => return None,
        })
    }
}

static FRONTEND: OnceLock<&'static dyn Frontend> = OnceLock::new();

/// Installs the front end. Only the first call counts.
pub fn install(frontend: &'static dyn Frontend) {
    let _ = FRONTEND.set(frontend);
}

/// The installed front end, or one that does nothing.
pub fn frontend() -> &'static dyn Frontend {
    *FRONTEND.get_or_init(|| &Headless)
}

/// Settings that are never stored.
struct Defaults;

impl Store for Defaults {
    fn read(&self, _group: &str, _key: &str, default: &str) -> String {
        default.to_string()
    }

    fn write(&self, _group: &str, _key: &str, _value: &str) {}
}

/// A front end without a platform, before one is installed.
struct Headless;

impl Clipboard for Headless {
    fn copy(&self, _text: &str, _target: Selection) {}

    fn copy_rich(&self, _text: &str, _html: &str) {}

    fn paste(&self, _source: Selection) -> String {
        String::new()
    }
}

impl Notifications for Headless {
    fn notify(&self, _event: &str, _text: &str) {}
}

impl Desktop for Headless {
    fn open_url(&self, _url: &str) {}

    fn web_search(&self, _text: &str) {}
}

impl Themes for Headless {
    fn palette(&self, _theme: Option<&str>) -> Option<Palette> {
        None
    }

    fn installed(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

impl Screens for Headless {
    fn outputs(&self) -> Vec<String> {
        Vec::new()
    }

    fn pointer_output(&self) -> Option<String> {
        None
    }

    fn active_output(&self) -> String {
        String::new()
    }

    fn work_area(&self, _output: &str) -> Option<(i32, i32)> {
        None
    }
}

impl Window for Headless {
    fn slide(&self) {}

    fn set_blur(&self, _enabled: bool) {}

    fn set_color_scheme(&self, _scheme: &str) {}
}

impl GlobalShortcut for Headless {
    fn toggle(&self) -> String {
        String::new()
    }

    fn set_toggle(&self, _sequence: &str) -> bool {
        false
    }

    fn foreign_toggle(&self) -> String {
        String::new()
    }
}

impl Frontend for Headless {
    fn store(&self) -> &dyn Store {
        &Defaults
    }

    fn set_action_state(&self, _name: &str, _checked: bool, _enabled: bool) {}

    fn action_text(&self, _name: &str) -> String {
        String::new()
    }

    fn shortcuts_json(&self) -> String {
        "[]".to_string()
    }

    fn reload_shortcuts(&self) {}

    fn set_menu_state(&self, _json: &str) {}

    fn set_tray_visible(&self, _visible: bool) {}

    fn highlight_color(&self) -> String {
        String::new()
    }

    fn skins(&self) -> Vec<Skin> {
        Vec::new()
    }

    fn popup_main_menu(&self, _x: i32, _y: i32) -> Option<String> {
        None
    }

    fn popup_session_menu(&self, _x: i32, _y: i32) -> Option<String> {
        None
    }

    fn popup_tab_menu(&self, _x: i32, _y: i32, _state: &str) -> Option<String> {
        None
    }

    fn popup_terminal_menu(
        &self,
        _x: i32,
        _y: i32,
        _can_copy: bool,
        _link: &str,
        _auto_saving: bool,
    ) -> Option<String> {
        None
    }

    fn popup_drop_menu(&self, _x: i32, _y: i32, _terminal: i32, _paths: &str, _directory: &str) {}

    fn ask(&self, _question: &Question) -> bool {
        false
    }

    fn ask_agent(&self, _agent: String) {}

    fn close_agent_question(&self) {}

    fn can_import(&self) -> bool {
        false
    }

    fn import(&self) -> bool {
        false
    }

    fn set_shortcut(&self, _name: &str, _sequences: &[String]) {}

    fn supports(&self, _feature: Feature) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_go_by_their_names() {
        for feature in [
            Feature::Skins,
            Feature::RemoteCommands,
            Feature::KeyTables,
            Feature::ShortcutDialog,
            Feature::Shells,
            Feature::ColorScheme,
        ] {
            let name = format!("{feature:?}");
            let kebab: String = name
                .chars()
                .enumerate()
                .flat_map(|(index, letter)| {
                    let dash = (index > 0 && letter.is_uppercase()).then_some('-');
                    dash.into_iter().chain(letter.to_lowercase())
                })
                .collect();
            assert_eq!(Feature::from_name(&kebab), Some(feature));
            assert!(!Headless.supports(feature));
        }
        assert_eq!(Feature::from_name("skin"), None);
    }

    #[test]
    fn without_a_front_end_nothing_happens() {
        let frontend = frontend();
        assert_eq!(frontend.store().read("Window", "Width", "90"), "90");
        frontend.store().write("Window", "Width", "50");
        assert_eq!(frontend.store().read("Window", "Width", "90"), "90");
        assert!(!frontend.can_import() && !frontend.import());
        assert!(frontend.skins().is_empty());
        assert_eq!(frontend.paste(Selection::Clipboard), "");
        assert_eq!(frontend.work_area("DP-1"), None);
        assert!(frontend.outputs().is_empty() && frontend.pointer_output().is_none());
        assert_eq!(frontend.active_output(), "");
        assert!(frontend.palette(None).is_none() && frontend.installed().is_empty());
        assert_eq!(
            (frontend.toggle(), frontend.foreign_toggle()),
            (String::new(), String::new())
        );
        assert_eq!(frontend.shortcuts_json(), "[]");
        assert_eq!(
            (frontend.action_text("file_quit"), frontend.highlight_color()),
            (String::new(), String::new())
        );
        let question = Question {
            id: 0,
            title: String::new(),
            text: String::new(),
            confirm: crate::app::Confirm::Quit,
        };
        // None of these may fail without a platform.
        frontend.copy("a", Selection::Primary);
        frontend.copy_rich("a", "<b>a</b>");
        frontend.notify("bell", "a");
        frontend.open_url("https://kde.org");
        frontend.web_search("a");
        frontend.slide();
        frontend.set_blur(true);
        frontend.set_color_scheme("dark");
        frontend.set_toggle("F12");
        frontend.set_action_state("keep-open", true, true);
        frontend.reload_shortcuts();
        frontend.set_menu_state("{}");
        frontend.set_tray_visible(true);
        assert_eq!(frontend.popup_main_menu(0, 0), None);
        assert_eq!(frontend.popup_session_menu(0, 0), None);
        assert_eq!(frontend.popup_tab_menu(0, 0, "{}"), None);
        assert_eq!(frontend.popup_terminal_menu(0, 0, true, "", false), None);
        frontend.popup_drop_menu(0, 0, 0, "/tmp", "/");
        assert!(!frontend.ask(&question));
        frontend.ask_agent("a".into());
        frontend.close_agent_question();
    }
}
