// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The INI dialect of `KConfig` files, enough to read what Yakuake and Konsole left behind,
//! and [`IniFile`], which keeps the settings of front ends without `KConfig` in such a file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::settings::Store;

pub type Group = HashMap<String, String>;

/// Parses the INI dialect used by `KConfig`.
#[must_use]
pub fn parse_ini(text: &str) -> HashMap<String, Group> {
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut current = String::new();
    // Editors on Windows put a byte order mark in front, which trim keeps.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Only the white space of ASCII goes, as in KConfig: a value can start or end with a
    // space that does not break, such as U+00A0 in the separators of words.
    for line in text.lines() {
        let line = line.trim_ascii();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.to_string();
        } else if let Some((key, value)) = line.split_once('=') {
            groups
                .entry(current.clone())
                .or_default()
                .insert(key.trim_ascii().to_string(), value.trim_ascii().to_string());
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
    /// False when the file exists but could not be read: writing it would replace the
    /// settings in it with the few changed since, so changes stay in memory.
    writable: bool,
    /// The file was not UTF-8; the original goes to a backup before the first save.
    back_up: AtomicBool,
}

impl IniFile {
    /// The settings in `path`; none if it does not exist yet. A file in the ANSI code page of
    /// Windows (Windows-1252), as older editors and PowerShell 5.1 save it, is read as such.
    #[must_use]
    pub fn open(path: &Path) -> Self {
        let (text, writable, back_up) = match read_text(path) {
            Ok((text, utf8)) => (text, true, !utf8),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), true, false),
            Err(error) => {
                eprintln!(
                    "kudari: could not read {}: {error}; changes to the settings are not saved",
                    path.display()
                );
                (String::new(), false, false)
            }
        };
        let groups = groups_of(&text);
        Self {
            path: path.to_path_buf(),
            groups: Mutex::new(groups),
            writable,
            back_up: AtomicBool::new(back_up),
        }
    }

    fn save(&self, groups: &BTreeMap<String, BTreeMap<String, String>>) -> std::io::Result<()> {
        use std::fmt::Write;

        if !self.writable {
            return Ok(());
        }
        if self.back_up.load(Ordering::Relaxed) {
            std::fs::copy(&self.path, self.path.with_extension("ini.bak"))?;
            self.back_up.store(false, Ordering::Relaxed);
        }

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
        let mut file = std::fs::File::create(&partial)?;
        std::io::Write::write_all(&mut file, text.as_bytes())?;
        // On the disk before it takes the place of the old one, or a crash just after can
        // leave an empty file there.
        file.sync_all()?;
        drop(file);
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
        // The user can have changed the file in an editor while Kudari ran: what is in it
        // stays, and only this key changes. The settings of Kudari are in it already, as each
        // is saved when it changes; a file that cannot be read leaves the ones in memory.
        if self.writable
            && let Ok((text, _)) = read_text(&self.path)
        {
            *groups = groups_of(&text);
        }
        // The same value again, as a slider sends it, needs no write.
        if groups.get(group).and_then(|found| found.get(key)).map(String::as_str) == Some(value) {
            return;
        }
        groups
            .entry(group.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        if let Err(error) = self.save(&groups) {
            eprintln!("kudari: could not save {}: {error}", self.path.display());
        }
    }
}

/// The text of an INI file, and whether it was UTF-8; else it is read as Windows-1252.
fn read_text(path: &Path) -> std::io::Result<(String, bool)> {
    let bytes = std::fs::read(path)?;
    Ok(match String::from_utf8(bytes) {
        Ok(text) => (text, true),
        Err(error) => (windows_1252(error.as_bytes()), false),
    })
}

/// The groups of an INI file with their values unescaped.
fn groups_of(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    parse_ini(text)
        .into_iter()
        .map(|(name, group)| {
            let group = group.into_iter().map(|(key, value)| (key, unescape(&value))).collect();
            (name, group)
        })
        .collect()
}

/// The characters of Windows-1252 for the bytes 0x80 to 0x9F; the rest is Latin-1. Bytes
/// without a character there keep their code point, as browsers decode them.
const WINDOWS_1252: [char; 32] = [
    '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{2c6}', '\u{2030}',
    '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}', '\u{8f}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}',
    '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}',
    '\u{17e}', '\u{178}',
];

fn windows_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| match byte {
            0x80..=0x9f => WINDOWS_1252
                .get(usize::from(byte - 0x80))
                .copied()
                .unwrap_or('\u{fffd}'),
            _ => char::from(byte),
        })
        .collect()
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

/// A value as `KConfig` reads it from its file: `\s`, `\t`, `\n`, `\r`, `\\` and `\xHH`, which
/// can also be one byte of a character in UTF-8. Other escapes keep their backslash, for
/// lists to split.
#[must_use]
pub fn kconfig_unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut text = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        index += 1;
        if byte != b'\\' {
            text.push(byte);
            continue;
        }
        let Some(&next) = bytes.get(index) else {
            text.push(b'\\');
            break;
        };
        index += 1;
        match next {
            b's' => text.push(b' '),
            b't' => text.push(b'\t'),
            b'n' => text.push(b'\n'),
            b'r' => text.push(b'\r'),
            b'\\' => text.push(b'\\'),
            b'x' => {
                let hex = bytes
                    .get(index..index + 2)
                    .and_then(|hex| std::str::from_utf8(hex).ok());
                if let Some(value) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    text.push(value);
                    index += 2;
                } else {
                    text.extend_from_slice(b"\\x");
                }
            }
            other => text.extend_from_slice(&[b'\\', other]),
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// The entries of a list that `KConfig` wrote, separated by commas, in which `\,` is a comma
/// and `\\` a backslash; an empty value is an empty list.
#[must_use]
pub fn kconfig_list(value: &str) -> Vec<String> {
    let mut list = Vec::new();
    if value.is_empty() {
        return list;
    }
    let mut entry = String::new();
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => entry.push(characters.next().unwrap_or('\\')),
            ',' => list.push(std::mem::take(&mut entry)),
            other => entry.push(other),
        }
    }
    list.push(entry);
    list
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
    fn values_of_kconfig_are_unescaped_as_kconfig_does() {
        assert_eq!(kconfig_unescape("%d : %n\\s"), "%d : %n ");
        assert_eq!(kconfig_unescape("a\\\\b\\tc\\nd\\re"), "a\\b\tc\nd\re");
        // Bytes of UTF-8 one by one.
        assert_eq!(kconfig_unescape("\\xc3\\xa4 \\x41"), "ä A");
        assert_eq!(kconfig_unescape("\\xzz\\,end\\"), "\\xzz\\,end\\");
        assert_eq!(kconfig_unescape("plain ä"), "plain ä");
        assert_eq!(kconfig_list("A=1,B=x\\,y,C=\\\\"), ["A=1", "B=x,y", "C=\\"]);
        assert!(kconfig_list("").is_empty());
        assert_eq!(kconfig_list("one"), ["one"]);
    }

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
        settings.write("Terminal", "WordSeparators", "\u{a0}x\u{3000}");
        let again = IniFile::open(&path);
        assert_eq!(again.read("Window", "Width", "90"), "80");
        assert_eq!(again.read("Terminal", "Environment", ""), "A=1\nB=C:\\x\tz");
        assert_eq!(again.read("Terminal", "BadgeText", ""), " padded ");
        assert_eq!(again.read("Terminal", "WordSeparators", ""), "\u{a0}x\u{3000}");
        assert_eq!(again.read("Terminal", "Missing", "fallback"), "fallback");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[Window]\nWidth=80\n"), "{text}");
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn changes_made_by_hand_while_kudari_runs_stay() {
        let path = file("by-hand");
        let settings = IniFile::open(&path);
        settings.write("Window", "Width", "80");
        // The user adds a key that the dialog has no field for, in an editor.
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{text}[Appearance]\nTerminalHighlightOpacity=0.8\n")).unwrap();
        // Then Kudari saves a setting of its own.
        settings.write("Window", "KeepOpen", "true");
        let again = IniFile::open(&path);
        assert_eq!(again.read("Appearance", "TerminalHighlightOpacity", ""), "0.8");
        assert_eq!(again.read("Window", "KeepOpen", ""), "true");
        assert_eq!(again.read("Window", "Width", ""), "80");
        assert_eq!(settings.read("Appearance", "TerminalHighlightOpacity", ""), "0.8");
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn the_same_value_again_writes_nothing() {
        let path = file("same");
        let store = IniFile::open(&path);
        store.write("Window", "Height", "50");
        let written = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // A slider that sends the value it has: the file stays as it was.
        store.write("Window", "Height", "50");
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), written);
        store.write("Window", "Height", "60");
        assert_ne!(std::fs::metadata(&path).unwrap().modified().unwrap(), written);
        assert_eq!(IniFile::open(&path).read("Window", "Height", ""), "60");
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
    fn a_byte_order_mark_does_not_hide_the_first_group() {
        let groups = parse_ini("\u{feff}[Window]\nWidth=70\n");
        assert_eq!(groups["Window"]["Width"], "70");
        assert!(!groups.contains_key(""), "{groups:?}");
    }

    #[test]
    fn an_ansi_file_keeps_its_settings_and_a_backup() {
        // Saved by PowerShell 5.1 or an editor in Windows-1252: ü is 0xFC, € is 0x80.
        let path = file("ansi");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"[Terminal]\nDirectory=C:\\\\Users\\\\J\xfcrgen\nBadgeText=\x80\n[Window]\nWidth=70\n".to_vec();
        std::fs::write(&path, &original).unwrap();
        let settings = IniFile::open(&path);
        assert_eq!(settings.read("Terminal", "Directory", ""), "C:\\Users\\Jürgen");
        assert_eq!(settings.read("Terminal", "BadgeText", ""), "€");
        settings.write("Window", "KeepOpen", "true");
        let again = IniFile::open(&path);
        assert_eq!(again.read("Window", "Width", "90"), "70");
        assert_eq!(again.read("Window", "KeepOpen", "false"), "true");
        assert_eq!(again.read("Terminal", "Directory", ""), "C:\\Users\\Jürgen");
        assert_eq!(std::fs::read(path.with_extension("ini.bak")).unwrap(), original);
        // Now UTF-8, so a second change does not overwrite the backup with it.
        again.write("Window", "Width", "60");
        assert_eq!(std::fs::read(path.with_extension("ini.bak")).unwrap(), original);
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_missing_file_gets_no_backup() {
        let path = file("fresh");
        IniFile::open(&path).write("Window", "Width", "80");
        assert!(path.exists());
        assert!(!path.with_extension("ini.bak").exists());
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;

        let path = file("locked");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = "[Window]\nWidth=70\n[Shortcuts]\nnew-tab=Ctrl+Shift+T\n";
        std::fs::write(&path, original).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root reads it anyway, so there is no failure to test.
        if std::fs::read(&path).is_err() {
            let settings = IniFile::open(&path);
            assert_eq!(settings.read("Window", "Width", "90"), "90");
            settings.write("Window", "KeepOpen", "true");
            assert_eq!(settings.read("Window", "KeepOpen", "false"), "true");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            assert!(!path.with_extension("ini.partial").exists());
        }
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
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
