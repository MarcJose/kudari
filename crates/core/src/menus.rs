// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The menus of Kudari as data, for a front end without menus of its own to show them in
//! its own controls; the KDE front end builds the same with `KActionCollection`.
//!
//! A menu is a JSON list of entries:
//! - `{"type": "action", "name", "text", "checkable", "checked", "enabled"}` runs an action
//!   of [`crate::app::App::trigger`],
//! - `{"type": "value", "kind", "value", "text", "checked"}` chooses a value, as of the
//!   width of the window or a signal for the program,
//! - `{"type": "link", "link", "copy", "text"}` opens or copies a link,
//! - `{"type": "shell", "shell", "text"}` opens a session with a shell,
//! - `{"type": "menu", "title", "entries"}` is a submenu, and `{"type": "separator"}`.

use serde_json::{Value, json};

use crate::i18n::{tr, tr_arg};
use crate::process::Signal;
use crate::shells::{Kind, Shell};

/// The text of an action in its menus, translated.
#[must_use]
pub fn action_text(name: &str) -> Option<String> {
    if let Some(number) = name.strip_prefix("switch-to-session-") {
        return Some(tr_arg("@action", "Switch to Session %1", number));
    }
    let text = match name {
        "toggle-window-state" => tr("@action", "Show/Hide Kudari"),
        "keep-open" => tr("@action", "Keep Open When Focus Moves Elsewhere"),
        "increase-window-width" => tr("@action", "Widen Window"),
        "decrease-window-width" => tr("@action", "Narrow Window"),
        "increase-window-height" => tr("@action", "Make Window Taller"),
        "decrease-window-height" => tr("@action", "Make Window Shorter"),
        "new-session" => tr("@action", "New Session"),
        "new-session-two-horizontal" => tr("@action", "New Session, Side by Side"),
        "new-session-two-vertical" => tr("@action", "New Session, Stacked"),
        "new-session-quad" => tr("@action", "New Session, Four Terminals"),
        "close-session" => tr("@action", "Close Session"),
        "previous-session" => tr("@action", "Previous Session"),
        "next-session" => tr("@action", "Next Session"),
        "move-session-left" => tr("@action", "Move Session Left"),
        "move-session-right" => tr("@action", "Move Session Right"),
        "rename-session" => tr("@action", "Rename Session..."),
        "close-active-terminal" => tr("@action", "Close Terminal"),
        "split-left-right" => tr("@action", "Split into Columns"),
        "split-top-bottom" => tr("@action", "Split into Rows"),
        "split-auto" => tr("@action", "Split Automatically"),
        "toggle-session-prevent-closing" => tr("@action", "Protect from Closing"),
        "toggle-session-keyboard-input" => tr("@action", "Disable Keyboard Input"),
        "toggle-session-monitor-activity" => tr("@action", "Watch for Activity"),
        "toggle-session-monitor-silence" => tr("@action", "Watch for Silence"),
        "toggle-session-monitor-process" => tr("@action", "Monitor for Process Finishing"),
        "toggle-session-monitor-once" => tr("@action", "One-Shot Monitors"),
        "clear-scrollback" => tr("@action", "Clear Scrollback"),
        "clear-history-and-reset" => tr("@action", "Clear Scrollback and Reset"),
        "select-line" => tr("@action", "Select Line"),
        "select-mode" => tr("@action", "Select Mode"),
        "paste-selection" => tr("@action", "Paste Selection"),
        "open-browser" => tr("@action", "Open File Manager"),
        "web-search" => tr("@action", "Search the Web"),
        "auto-save" => tr("@action", "Auto Save Output"),
        "adjust-history" => tr("@action", "Adjust Scrollback..."),
        "allow-mouse-tracking" => tr("@action", "Allow Mouse Tracking"),
        "enlarge-font" => tr("@action", "Enlarge Font"),
        "shrink-font" => tr("@action", "Shrink Font"),
        "reset-font-size" => tr("@action", "Reset Font Size"),
        "view-full-screen" => tr("@action:inmenu", "Full Screen Mode"),
        "file_quit" => tr("@action:inmenu", "Quit"),
        "options_configure" => tr("@title:menu", "Settings"),
        "options_configure_keybinding" => tr("@title Preferences page name", "Shortcuts"),
        "help_about_app" => tr("@action:inmenu", "About Kudari"),
        "edit_copy" => tr("@action:inmenu", "Copy"),
        "edit_paste" => tr("@action:inmenu", "Paste"),
        "edit_select_all" => tr("@action:inmenu", "Select All"),
        "edit_find" => tr("@action:inmenu", "Find…"),
        "file_save_as" => tr("@action:inmenu", "Save Output As…"),
        "file_print" => tr("@action:inmenu", "Print…"),
        "edit_find_next" => tr("@info:tooltip", "Find Next"),
        "edit_find_prev" => tr("@info:tooltip", "Find Previous"),
        "grow-terminal-right" => tr("@action", "Expand Terminal Right"),
        "grow-terminal-left" => tr("@action", "Expand Terminal Left"),
        "grow-terminal-top" => tr("@action", "Expand Terminal Up"),
        "grow-terminal-bottom" => tr("@action", "Expand Terminal Down"),
        "previous-terminal" => tr("@action", "Previous Terminal"),
        "next-terminal" => tr("@action", "Next Terminal"),
        _ => return None,
    };
    Some(text)
}

/// Whether an action switches something on and off.
fn is_checkable(name: &str) -> bool {
    matches!(
        name,
        "keep-open"
            | "view-full-screen"
            | "toggle-session-prevent-closing"
            | "toggle-session-keyboard-input"
            | "toggle-session-monitor-activity"
            | "toggle-session-monitor-silence"
            | "toggle-session-monitor-process"
            | "toggle-session-monitor-once"
            | "auto-save"
            | "allow-mouse-tracking"
    )
}

/// What the app last said about an action: whether it is checked, and whether it is enabled.
pub trait States {
    fn state(&self, name: &str) -> (bool, bool);
}

fn action(name: &str, states: &dyn States) -> Value {
    let (checked, enabled) = states.state(name);
    json!({
        "type": "action",
        "name": name,
        "text": action_text(name).unwrap_or_else(|| name.to_string()),
        "checkable": is_checkable(name),
        "checked": checked,
        "enabled": enabled,
    })
}

fn actions(names: &[&str], states: &dyn States) -> Vec<Value> {
    names
        .iter()
        .map(|name| {
            if name.is_empty() {
                separator()
            } else {
                action(name, states)
            }
        })
        .collect()
}

/// An entry that is enabled only if `condition` holds as well.
fn enabled(mut entry: Value, condition: bool) -> Value {
    if let Some(entry) = entry.as_object_mut() {
        let before = entry.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        entry.insert("enabled".to_string(), Value::from(before && condition));
    }
    entry
}

fn separator() -> Value {
    json!({ "type": "separator" })
}

fn submenu(title: String, entries: Vec<Value>) -> Value {
    json!({ "type": "menu", "title": title, "entries": entries })
}

/// The window size in percent, as the menu offers it, from 10 to 100.
fn percents(kind: &str, current: i64) -> Vec<Value> {
    (1..=10)
        .map(|step| {
            let value = step * 10;
            json!({
                "type": "value",
                "kind": kind,
                "value": value,
                "text": tr_arg("", "%1%", &value.to_string()),
                "checked": value == current,
            })
        })
        .collect()
}

/// The choice of the screen: the one under the pointer, or one of `count`.
fn screens(current: i64, count: i64) -> Vec<Value> {
    let mut entries = vec![json!({
        "type": "value", "kind": "screen", "value": 0, "checked": current == 0,
        "text": tr("@item:inmenu", "Where the mouse pointer is"),
    })];
    entries.extend((1..=count).map(|screen| {
        json!({
            "type": "value", "kind": "screen", "value": screen, "checked": current == screen,
            "text": tr_arg("@item:inmenu", "Screen %1", &screen.to_string()),
        })
    }));
    entries
}

/// The menu of the button in the bar, as KDE's: `menu` is the state of [`crate::app`]'s
/// menu, with the `width`, `height` and `screen` of the window and the number of `screens`.
#[must_use]
pub fn main_menu(menu: &Value, states: &dyn States) -> Value {
    let number = |key: &str| menu.get(key).and_then(Value::as_i64).unwrap_or_default();
    let mut entries = vec![action("help_about_app", states), separator()];
    entries.extend(actions(&["view-full-screen", "keep-open"], states));
    if number("screens") > 1 {
        entries.push(submenu(
            tr("@title:menu", "Screen"),
            screens(number("screen"), number("screens")),
        ));
    }
    entries.push(submenu(tr("@title:menu", "Width"), percents("width", number("width"))));
    entries.push(submenu(
        tr("@title:menu", "Height"),
        percents("height", number("height")),
    ));
    entries.push(separator());
    entries.extend(actions(&["options_configure_keybinding", "options_configure"], states));
    entries.push(separator());
    entries.push(action("file_quit", states));
    Value::from(entries)
}

/// The menu of the button for new sessions, with the shells to start them with.
#[must_use]
pub fn session_menu(shells: &[Shell], states: &dyn States) -> Value {
    let mut entries = actions(
        &[
            "new-session",
            "",
            "new-session-two-horizontal",
            "new-session-two-vertical",
            "new-session-quad",
        ],
        states,
    );
    if !shells.is_empty() {
        let choices = shells
            .iter()
            .map(|shell| json!({ "type": "shell", "shell": shell.id, "text": shell_name(shell) }))
            .collect();
        entries.push(separator());
        entries.push(submenu(tr("@title:menu", "New Session With"), choices));
    }
    Value::from(entries)
}

/// The name of a shell as menus and settings show it.
#[must_use]
pub fn shell_name(shell: &Shell) -> String {
    match shell.kind {
        Kind::CommandPrompt => tr("@item:inmenu", "Command Prompt"),
        _ => shell.name.clone(),
    }
}

/// The shells as a choice of the settings: `text` and the `value` of the `Shell` setting.
#[must_use]
pub fn shell_choices(shells: &[Shell]) -> Value {
    shells
        .iter()
        .map(|shell| json!({ "text": shell_name(shell), "value": shell.id }))
        .collect()
}

/// The menu of a tab, for the session the app made the one of the menu.
#[must_use]
pub fn tab_menu(states: &dyn States) -> Value {
    Value::from(actions(
        &[
            "split-left-right",
            "split-top-bottom",
            "",
            "rename-session",
            "",
            "toggle-session-prevent-closing",
            "toggle-session-keyboard-input",
            "toggle-session-monitor-activity",
            "toggle-session-monitor-silence",
            "toggle-session-monitor-process",
            "toggle-session-monitor-once",
            "",
            "move-session-left",
            "move-session-right",
            "",
            "close-active-terminal",
            "close-session",
        ],
        states,
    ))
}

/// The menu of a tab with the entries for its color, which the front end asks for itself:
/// `session` and its `color`, empty for none; the entry that takes it away has `reset`.
#[must_use]
pub fn tab_menu_with_color(states: &dyn States, session: i64, color: &str) -> Value {
    let mut entries = match tab_menu(states) {
        Value::Array(entries) => entries,
        _ => Vec::new(),
    };
    let place = entries
        .iter()
        .position(|entry| entry["name"] == "rename-session")
        .map_or(entries.len(), |index| index + 1);
    let mut colors = vec![json!({
        "type": "tab-color", "session": session, "color": color, "text": tr("@action", "Tab Color..."),
    })];
    if !color.is_empty() {
        colors.push(json!({
            "type": "tab-color", "session": session, "color": "", "reset": true,
            "text": tr("@action", "Reset Tab Color"),
        }));
    }
    entries.splice(place..place, colors);
    Value::from(entries)
}

/// The signals as the menu names them, in the order of Konsole.
fn signal_text(signal: Signal) -> String {
    match signal {
        Signal::Stop => tr("@item:inmenu", "Suspend Task (STOP)"),
        Signal::Continue => tr("@item:inmenu", "Continue Task (CONT)"),
        Signal::Hangup => tr("@item:inmenu", "Hangup (HUP)"),
        Signal::Interrupt => tr("@item:inmenu", "Interrupt Task (INT)"),
        Signal::Terminate => tr("@item:inmenu", "Terminate Task (TERM)"),
        Signal::Kill => tr("@item:inmenu", "Kill Task (KILL)"),
        Signal::User1 => tr("@item:inmenu", "User Signal 1 (USR1)"),
        Signal::User2 => tr("@item:inmenu", "User Signal 2 (USR2)"),
    }
}

/// The number of a signal on Linux, which `Signal::from_number` reads back.
fn signal_number(signal: Signal) -> i32 {
    match signal {
        Signal::Stop => 19,
        Signal::Continue => 18,
        Signal::Hangup => 1,
        Signal::Interrupt => 2,
        Signal::Terminate => 15,
        Signal::Kill => 9,
        Signal::User1 => 10,
        Signal::User2 => 12,
    }
}

/// What the terminal menu shows: a link under the pointer, whether text is selected, and the
/// signals the platform can send.
pub struct Terminal<'a> {
    pub link: &'a str,
    pub can_copy: bool,
    pub signals: &'a [Signal],
}

/// The menu of a terminal.
#[must_use]
pub fn terminal_menu(terminal: &Terminal, states: &dyn States) -> Value {
    let mut entries = Vec::new();
    if !terminal.link.is_empty() {
        entries.push(
            json!({ "type": "link", "link": terminal.link, "copy": false, "text": tr("@action:inmenu", "Open Link") }),
        );
        entries.push(json!({ "type": "link", "link": terminal.link, "copy": true, "text": tr("@action:inmenu", "Copy Link Address") }));
        entries.push(separator());
    }
    entries.extend(actions(&["split-left-right", "split-top-bottom", ""], states));
    entries.push(enabled(action("edit_copy", states), terminal.can_copy));
    entries.extend(actions(
        &[
            "edit_paste",
            "paste-selection",
            "edit_select_all",
            "select-line",
            "select-mode",
            "",
            "edit_find",
        ],
        states,
    ));
    entries.push(enabled(action("web-search", states), terminal.can_copy));
    entries.extend(actions(
        &[
            "open-browser",
            "",
            "adjust-history",
            "clear-scrollback",
            "clear-history-and-reset",
            "file_save_as",
            "file_print",
            "auto-save",
            "allow-mouse-tracking",
        ],
        states,
    ));
    if !terminal.signals.is_empty() {
        let signals = terminal
            .signals
            .iter()
            .map(|signal| {
                json!({ "type": "value", "kind": "signal", "value": signal_number(*signal), "text": signal_text(*signal), "checked": false })
            })
            .collect();
        entries.push(separator());
        entries.push(submenu(tr("@title:menu", "Send Signal"), signals));
    }
    Value::from(entries)
}

/// Every signal of the menu of Konsole, in its order.
pub const SIGNALS: [Signal; 8] = [
    Signal::Stop,
    Signal::Continue,
    Signal::Hangup,
    Signal::Interrupt,
    Signal::Terminate,
    Signal::Kill,
    Signal::User1,
    Signal::User2,
];

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Everything enabled, and only `keep-open` checked.
    struct Checked;

    impl States for Checked {
        fn state(&self, name: &str) -> (bool, bool) {
            (name == "keep-open", true)
        }
    }

    fn names(menu: &Value) -> Vec<String> {
        menu.as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                entry["name"]
                    .as_str()
                    .or_else(|| entry["title"].as_str())
                    .unwrap_or_else(|| entry["type"].as_str().unwrap())
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn every_action_of_the_menus_has_a_text() {
        for menu in [
            main_menu(&json!({}), &Checked),
            session_menu(&[], &Checked),
            tab_menu(&Checked),
            terminal_menu(
                &Terminal {
                    link: "",
                    can_copy: true,
                    signals: &SIGNALS,
                },
                &Checked,
            ),
        ] {
            for entry in menu.as_array().unwrap() {
                if entry["type"] == "action" {
                    let name = entry["name"].as_str().unwrap();
                    assert_eq!(entry["text"], Value::from(action_text(name).unwrap()), "{name}");
                }
            }
        }
        assert_eq!(action_text("nothing"), None);
    }

    #[test]
    fn the_main_menu_shows_the_size_and_checks() {
        let menu = main_menu(&json!({"width": 90, "height": 50, "screen": 2, "screens": 2}), &Checked);
        assert_eq!(
            names(&menu),
            [
                "help_about_app",
                "separator",
                "view-full-screen",
                "keep-open",
                "Screen",
                "Width",
                "Height",
                "separator",
                "options_configure_keybinding",
                "options_configure",
                "separator",
                "file_quit"
            ]
        );
        assert_eq!(menu[3]["checked"], true);
        assert_eq!(menu[3]["checkable"], true);
        assert_eq!(menu[2]["checked"], false);
        let screens = menu[4]["entries"].as_array().unwrap();
        assert_eq!(screens.len(), 3, "the pointer and two screens");
        assert_eq!(
            (screens[2]["value"].clone(), screens[2]["checked"].clone()),
            (json!(2), json!(true))
        );
        let width = menu[5]["entries"].as_array().unwrap();
        assert_eq!(width.len(), 10);
        assert_eq!(width[8]["checked"], true);
        assert_eq!(width[8]["value"], 90);
        assert_eq!(menu[6]["entries"][4]["checked"], true);
        let single = main_menu(&json!({"screens": 1}), &Checked);
        assert!(
            !names(&single).contains(&"Screen".to_string()),
            "one screen needs no choice"
        );
    }

    #[test]
    fn the_session_menu_offers_the_shells() {
        assert_eq!(names(&session_menu(&[], &Checked)).len(), 5);
        let shells = [
            Shell {
                id: "cmd".into(),
                name: "Command Prompt".into(),
                program: PathBuf::from("cmd.exe"),
                arguments: Vec::new(),
                kind: Kind::CommandPrompt,
            },
            Shell {
                id: "wsl:Debian".into(),
                name: "Debian".into(),
                program: PathBuf::from("wsl.exe"),
                arguments: Vec::new(),
                kind: Kind::Wsl,
            },
        ];
        let menu = session_menu(&shells, &Checked);
        let with = &menu[6]["entries"];
        assert_eq!(with[0]["shell"], "cmd");
        assert_eq!(with[1]["text"], "Debian");
    }

    #[test]
    fn the_terminal_menu_follows_link_selection_and_signals() {
        let plain = terminal_menu(
            &Terminal {
                link: "",
                can_copy: false,
                signals: &[],
            },
            &Checked,
        );
        assert!(!names(&plain).contains(&"Send Signal".to_string()));
        let copy = plain
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == "edit_copy")
            .unwrap();
        assert_eq!(copy["enabled"], false);
        let windows = [Signal::Interrupt, Signal::Terminate, Signal::Kill];
        let linked = terminal_menu(
            &Terminal {
                link: "https://kde.org",
                can_copy: true,
                signals: &windows,
            },
            &Checked,
        );
        assert_eq!(linked[0]["type"], "link");
        assert_eq!(linked[1]["copy"], true);
        let signals = linked.as_array().unwrap().last().unwrap()["entries"]
            .as_array()
            .unwrap()
            .clone();
        let numbers: Vec<i64> = signals.iter().map(|entry| entry["value"].as_i64().unwrap()).collect();
        assert_eq!(numbers, [2, 15, 9]);
        for signal in SIGNALS {
            assert_eq!(Signal::from_number(signal_number(signal)), Some(signal));
        }
    }

    #[test]
    fn the_tab_menu_lists_the_session_actions() {
        let menu = tab_menu(&Checked);
        assert!(names(&menu).contains(&"toggle-session-prevent-closing".to_string()));
        assert_eq!(menu.as_array().unwrap().last().unwrap()["name"], "close-session");
    }

    #[test]
    fn the_tab_menu_offers_a_color_after_renaming() {
        let colors = |menu: &Value| -> Vec<(usize, String)> {
            let entries = menu.as_array().unwrap();
            entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry["type"] == "tab-color")
                .map(|(index, entry)| (index, entry["color"].as_str().unwrap().to_string()))
                .collect()
        };
        let plain = tab_menu_with_color(&Checked, 7, "");
        let rename = names(&plain).iter().position(|name| name == "rename-session").unwrap();
        assert_eq!(colors(&plain), [(rename + 1, String::new())]);
        assert_eq!(plain[rename + 1]["session"], 7);
        let colored = tab_menu_with_color(&Checked, 7, "#ff0000");
        assert_eq!(
            colors(&colored),
            [(rename + 1, "#ff0000".into()), (rename + 2, String::new())]
        );
        assert_eq!(colored[rename + 2]["reset"], true);
        assert!(colored[rename + 1].get("reset").is_none());
    }
}
