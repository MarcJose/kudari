// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The INI dialect of `KConfig` files, enough to read what Yakuake and Konsole left behind,
//! and [`IniFile`], which keeps the settings of front ends without `KConfig` in such a file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::settings::Store;

pub type Group = HashMap<String, String>;

/// Parses the INI dialect used by `KConfig`.
#[must_use]
pub fn parse_ini(text: &str) -> HashMap<String, Group> {
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.to_string();
        } else if let Some((key, value)) = line.split_once('=') {
            groups
                .entry(current.clone())
                .or_default()
                .insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    groups
}

/// Settings in an INI file, written back on every change. Values keep line breaks and
/// backslashes through the escapes of `KConfig`.
#[derive(Debug)]
pub struct IniFile {
    path: PathBuf,
    groups: Mutex<BTreeMap<String, BTreeMap<String, String>>>,
}

impl IniFile {
    /// The settings in `path`; none if it does not exist yet.
    #[must_use]
    pub fn open(path: &Path) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let groups = parse_ini(&text)
            .into_iter()
            .map(|(name, group)| {
                let group = group.into_iter().map(|(key, value)| (key, unescape(&value))).collect();
                (name, group)
            })
            .collect();
        Self {
            path: path.to_path_buf(),
            groups: Mutex::new(groups),
        }
    }

    fn save(&self, groups: &BTreeMap<String, BTreeMap<String, String>>) -> std::io::Result<()> {
        use std::fmt::Write;

        let mut text = String::new();
        for (name, group) in groups {
            let _ = writeln!(text, "[{name}]");
            for (key, value) in group {
                let _ = writeln!(text, "{key}={}", escape(value));
            }
            text.push('\n');
        }
        if let Some(directory) = self.path.parent() {
            std::fs::create_dir_all(directory)?;
        }
        // Written beside it and renamed, so that a crash leaves the old file whole.
        let partial = self.path.with_extension("ini.partial");
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &self.path)
    }
}

impl Store for IniFile {
    fn read(&self, group: &str, key: &str, default: &str) -> String {
        let groups = self.groups.lock().unwrap_or_else(PoisonError::into_inner);
        groups
            .get(group)
            .and_then(|found| found.get(key))
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    fn write(&self, group: &str, key: &str, value: &str) {
        let mut groups = self.groups.lock().unwrap_or_else(PoisonError::into_inner);
        groups
            .entry(group.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        if let Err(error) = self.save(&groups) {
            eprintln!("kudari: could not save {}: {error}", self.path.display());
        }
    }
}

fn escape(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => text.push_str("\\\\"),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            _ => text.push(character),
        }
    }
    // Leading and trailing spaces would be trimmed when read.
    if text.starts_with(' ') {
        text.replace_range(..1, "\\s");
    }
    if text.ends_with(' ') && text.len() > 1 {
        text.truncate(text.len() - 1);
        text.push_str("\\s");
    }
    text
}

fn unescape(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            text.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => text.push('\n'),
            Some('r') => text.push('\r'),
            Some('t') => text.push('\t'),
            Some('s') => text.push(' '),
            Some(other) => text.push(other),
            None => text.push('\\'),
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_and_comments_are_understood() {
        let groups = parse_ini("# note\n[One]\nkey=value\n; other note\n[Two]\nother = 1\n");
        assert_eq!(groups["One"]["key"], "value");
        assert_eq!(groups["Two"]["other"], "1");
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn random_files_do_no_harm() {
        for input in crate::fuzz::inputs(&["[Group]\nkey=value\n"]) {
            let _ = parse_ini(&input);
        }
    }

    fn file(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("kudari-ini-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory.join("Kudari").join("kudari.ini")
    }

    #[test]
    fn settings_survive_a_restart() {
        let path = file("restart");
        let settings = IniFile::open(&path);
        assert_eq!(settings.read("Window", "Width", "90"), "90");
        settings.write("Window", "Width", "80");
        settings.write("Terminal", "Environment", "A=1\nB=C:\\x\tz");
        settings.write("Terminal", "BadgeText", " padded ");
        let again = IniFile::open(&path);
        assert_eq!(again.read("Window", "Width", "90"), "80");
        assert_eq!(again.read("Terminal", "Environment", ""), "A=1\nB=C:\\x\tz");
        assert_eq!(again.read("Terminal", "BadgeText", ""), " padded ");
        assert_eq!(again.read("Terminal", "Missing", "fallback"), "fallback");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[Window]\nWidth=80\n"), "{text}");
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn escapes_go_both_ways() {
        for value in ["", " ", "a\\b", "line\nline\r\n", "\\n", "trailing\\"] {
            assert_eq!(unescape(&escape(value)), value, "{value:?}");
        }
        assert_eq!(unescape("\\"), "\\");
        assert_eq!(unescape("\\q"), "q");
    }

    #[test]
    fn an_unwritable_place_only_warns() {
        // A file where the directory would have to be.
        let blocked = file("blocked");
        let directory = blocked.parent().unwrap();
        std::fs::create_dir_all(directory.parent().unwrap()).unwrap();
        std::fs::write(directory, "").unwrap();
        let settings = IniFile::open(&blocked);
        settings.write("Window", "Width", "80");
        assert_eq!(settings.read("Window", "Width", "90"), "80");
        std::fs::remove_dir_all(directory.parent().unwrap()).unwrap();
    }
}
