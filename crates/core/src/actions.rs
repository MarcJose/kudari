// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The default shortcuts of the actions that [`crate::app::App::trigger`] runs, in the
//! portable text of Qt key sequences. They are those of Yakuake and Konsole; the KDE front
//! end sets the same up in `cpp/kdebridge.cpp`, where the user can change them. The global
//! shortcut that shows and hides the window is not among them.

use crate::settings::Store;

/// Action name and its default key sequences.
pub const DEFAULT_SHORTCUTS: &[(&str, &[&str])] = &[
    ("view-full-screen", &["Ctrl+Shift+F11"]),
    ("file_quit", &["Ctrl+Shift+Q"]),
    // As in Windows Terminal; KDE's Ctrl+Shift+, does not work where Shift+, is no comma.
    ("options_configure", &["Ctrl+,"]),
    ("edit_copy", &["Ctrl+Shift+C"]),
    ("edit_paste", &["Ctrl+Shift+V", "Shift+Ins"]),
    ("edit_find", &["Ctrl+Shift+F"]),
    ("edit_find_next", &["F3"]),
    ("edit_find_prev", &["Shift+F3"]),
    ("file_print", &["Ctrl+Shift+P"]),
    ("increase-window-width", &["Alt+Shift+Right"]),
    ("decrease-window-width", &["Alt+Shift+Left"]),
    ("increase-window-height", &["Alt+Shift+Down"]),
    ("decrease-window-height", &["Alt+Shift+Up"]),
    ("new-session", &["Ctrl+Shift+T"]),
    ("close-session", &["Ctrl+Shift+W"]),
    ("previous-session", &["Shift+Left"]),
    ("next-session", &["Shift+Right"]),
    ("move-session-left", &["Ctrl+Shift+Left"]),
    ("move-session-right", &["Ctrl+Shift+Right"]),
    ("grow-terminal-right", &["Ctrl+Alt+Right"]),
    ("grow-terminal-left", &["Ctrl+Alt+Left"]),
    ("grow-terminal-top", &["Ctrl+Alt+Up"]),
    ("grow-terminal-bottom", &["Ctrl+Alt+Down"]),
    ("rename-session", &["Ctrl+Alt+S"]),
    ("previous-terminal", &["Ctrl+Shift+Tab"]),
    ("next-terminal", &["Ctrl+Tab"]),
    ("close-active-terminal", &["Ctrl+Shift+R"]),
    ("split-left-right", &["Ctrl+("]),
    ("split-top-bottom", &["Ctrl+)"]),
    ("split-auto", &["Ctrl+*"]),
    ("toggle-session-monitor-activity", &["Ctrl+Shift+A"]),
    ("toggle-session-monitor-silence", &["Ctrl+Shift+I"]),
    ("switch-to-session-1", &["Alt+1"]),
    ("switch-to-session-2", &["Alt+2"]),
    ("switch-to-session-3", &["Alt+3"]),
    ("switch-to-session-4", &["Alt+4"]),
    ("switch-to-session-5", &["Alt+5"]),
    ("switch-to-session-6", &["Alt+6"]),
    ("switch-to-session-7", &["Alt+7"]),
    ("switch-to-session-8", &["Alt+8"]),
    ("switch-to-session-9", &["Alt+9"]),
    ("switch-to-session-10", &["Alt+0"]),
    ("clear-history-and-reset", &["Ctrl+Shift+K"]),
    ("select-mode", &["Ctrl+Shift+D"]),
    ("paste-selection", &["Ctrl+Shift+Ins"]),
    ("enlarge-font", &["Ctrl++", "Ctrl+="]),
    ("shrink-font", &["Ctrl+-"]),
    ("reset-font-size", &["Ctrl+Alt+0"]),
];

/// What a key of `[Shortcuts]` reads as while it is not set.
const UNSET: &str = "\u{0}";

/// The sequences of an action: those the user chose in `[Shortcuts]` of `store`, separated
/// by "; " and empty for none, or else the defaults.
#[must_use]
pub fn sequences(store: &dyn Store, name: &str) -> Vec<String> {
    let value = store.read("Shortcuts", name, UNSET);
    if value == UNSET {
        return DEFAULT_SHORTCUTS
            .iter()
            .find(|(action, _)| *action == name)
            .map(|(_, keys)| keys.iter().map(ToString::to_string).collect())
            .unwrap_or_default();
    }
    value
        .split(';')
        .map(str::trim)
        .filter(|sequence| !sequence.is_empty())
        .map(str::to_string)
        .collect()
}

/// Gives an action other sequences; none takes its shortcut away.
pub fn set_sequences(store: &dyn Store, name: &str, sequences: &[String]) {
    store.write("Shortcuts", name, &sequences.join("; "));
}

/// The shortcuts as the QML of a window binds them and the settings show them: a list of
/// `name`, `sequences` and the `defaults`.
#[must_use]
pub fn shortcuts_json(store: &dyn Store) -> String {
    let list: Vec<serde_json::Value> = DEFAULT_SHORTCUTS
        .iter()
        .map(|(name, defaults)| {
            serde_json::json!({ "name": name, "sequences": sequences(store, name), "defaults": defaults })
        })
        .collect();
    serde_json::Value::from(list).to_string()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};

    use super::*;

    #[derive(Default)]
    struct Table(RefCell<HashMap<String, String>>);

    impl Store for Table {
        fn read(&self, _: &str, key: &str, default: &str) -> String {
            self.0.borrow().get(key).cloned().unwrap_or_else(|| default.to_string())
        }

        fn write(&self, _: &str, key: &str, value: &str) {
            self.0.borrow_mut().insert(key.to_string(), value.to_string());
        }
    }

    #[test]
    fn every_action_and_key_is_named_once() {
        let names: HashSet<_> = DEFAULT_SHORTCUTS.iter().map(|(name, _)| name).collect();
        assert_eq!(names.len(), DEFAULT_SHORTCUTS.len());
        let keys: Vec<_> = DEFAULT_SHORTCUTS.iter().flat_map(|(_, keys)| keys.iter()).collect();
        let unique: HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len(), "a key for two actions");
        assert!(DEFAULT_SHORTCUTS.iter().all(|(_, keys)| !keys.is_empty()));
        for (name, _) in DEFAULT_SHORTCUTS {
            assert!(
                crate::menus::action_text(name).is_some(),
                "{name} has no text for the settings"
            );
        }
    }

    #[test]
    fn the_list_is_json_for_qml() {
        let list: serde_json::Value = serde_json::from_str(&shortcuts_json(&Table::default())).unwrap();
        assert_eq!(list[0]["name"], "view-full-screen");
        assert_eq!(list[4]["sequences"][1], "Shift+Ins");
        assert_eq!(list[4]["defaults"], list[4]["sequences"]);
    }

    #[test]
    fn chosen_shortcuts_replace_the_defaults() {
        let table = Table::default();
        assert_eq!(sequences(&table, "new-session"), ["Ctrl+Shift+T"]);
        set_sequences(&table, "new-session", &["Ctrl+T".into(), "Alt+N".into()]);
        assert_eq!(sequences(&table, "new-session"), ["Ctrl+T", "Alt+N"]);
        set_sequences(&table, "new-session", &[]);
        assert!(sequences(&table, "new-session").is_empty(), "none at all");
        let list: serde_json::Value = serde_json::from_str(&shortcuts_json(&table)).unwrap();
        let entry = list
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == "new-session")
            .unwrap();
        assert_eq!(entry["sequences"], serde_json::json!([]));
        assert_eq!(entry["defaults"], serde_json::json!(["Ctrl+Shift+T"]));
        assert!(sequences(&table, "no-such-action").is_empty());
    }
}
