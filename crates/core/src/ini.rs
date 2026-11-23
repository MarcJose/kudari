// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The INI dialect of `KConfig` files, enough to read what Yakuake and Konsole left behind,
//! and [`IniFile`], which keeps the settings of front ends without `KConfig` in such a file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

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

/// Settings in an INI file, written back on every change, or with [`IniFile::with_delay`]
/// a moment after the last of changes close together. Values keep line breaks and
/// backslashes through the escapes of `KConfig`.
#[derive(Debug)]
pub struct IniFile {
    shared: Arc<Shared>,
}

type Groups = BTreeMap<String, BTreeMap<String, String>>;

#[derive(Debug)]
struct Shared {
    path: PathBuf,
    groups: Mutex<Groups>,
    /// False when the file exists but could not be read: writing it would replace the
    /// settings in it with the few changed since, so changes stay in memory.
    writable: bool,
    /// The file was not UTF-8; the original goes to a backup before the first save.
    back_up: AtomicBool,
    /// Changes that wait for the file. Taken after `groups`, where both are.
    waiting: Mutex<Waiting>,
    /// Wakes the thread that writes them.
    wake: Condvar,
    /// How often the file was written.
    saves: AtomicUsize,
    /// How long changes wait to go to the file together; nothing writes each at once.
    delay: Duration,
}

#[derive(Debug, Default)]
struct Waiting {
    /// Group, key and value, in the order they came.
    changes: Vec<(String, String, String)>,
    /// When they go to the file.
    due: Option<Instant>,
    /// The writes that failed in a row.
    failures: u32,
    /// The file is closed: the thread ends.
    closed: bool,
}

/// How often a write that failed is tried again by itself. After that the changes wait for
/// the next one, or for the end, and stay in memory meanwhile.
const RETRIES: u32 = 5;

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
            shared: Arc::new(Shared {
                path: path.to_path_buf(),
                groups: Mutex::new(groups),
                writable,
                back_up: AtomicBool::new(back_up),
                waiting: Mutex::new(Waiting::default()),
                wake: Condvar::new(),
                saves: AtomicUsize::new(0),
                delay: Duration::ZERO,
            }),
        }
    }

    /// Writes changes `delay` after the last of them, all together, as a slider or a grip
    /// that is dragged sends many. What guards the user, and that the first start is done,
    /// still go at once; [`IniFile::flush`] and dropping the file write what waits.
    #[must_use]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        let Some(shared) = Arc::get_mut(&mut self.shared) else {
            return self;
        };
        shared.delay = delay;
        if !delay.is_zero() {
            let shared = Arc::clone(&self.shared);
            let started = std::thread::Builder::new()
                .name("kudari-settings".to_string())
                .spawn(move || shared.write_when_due());
            // Without the thread, each change goes at once.
            if started.is_err()
                && let Some(shared) = Arc::get_mut(&mut self.shared)
            {
                shared.delay = Duration::ZERO;
            }
        }
        self
    }

    /// Writes the changes that wait, at once.
    pub fn flush(&self) {
        self.shared.flush();
    }

    #[cfg(test)]
    fn saves(&self) -> usize {
        self.shared.saves.load(Ordering::Relaxed)
    }
}

impl Drop for IniFile {
    fn drop(&mut self) {
        self.shared.flush();
        self.shared.waiting().closed = true;
        self.shared.wake.notify_all();
    }
}

impl Shared {
    fn groups(&self) -> MutexGuard<'_, Groups> {
        self.groups.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn waiting(&self) -> MutexGuard<'_, Waiting> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Writes `changes` and those that wait into the file as it is now.
    fn write_now(&self, groups: &mut Groups, changes: Vec<(String, String, String)>) {
        let mut waiting = self.waiting();
        // These waited only where they changed what was in memory, which the file can lack.
        let waited = std::mem::take(&mut waiting.changes);
        waiting.due = None;
        drop(waiting);
        // The user can have changed the file in an editor while Kudari ran: what is in it
        // stays, and only these keys change. The settings of Kudari are in it already, as
        // each is saved; a file that cannot be read leaves the ones in memory.
        if self.writable
            && let Ok((text, _)) = read_text(&self.path)
        {
            *groups = groups_of(&text);
        }
        let mut written = Vec::new();
        for (group, key, value) in waited {
            groups
                .entry(group.clone())
                .or_default()
                .insert(key.clone(), value.clone());
            written.push((group, key, value));
        }
        for (group, key, value) in changes {
            let entry = groups.entry(group.clone()).or_default();
            // The same value again, as a slider sends it, needs no write.
            if entry.get(&key) != Some(&value) {
                entry.insert(key.clone(), value.clone());
                written.push((group, key, value));
            }
        }
        if written.is_empty() {
            return;
        }
        let saved = self.save(groups);
        let mut waiting = self.waiting();
        let Err(error) = saved else {
            waiting.failures = 0;
            return;
        };
        eprintln!("kudari: could not save {}: {error}", self.path.display());
        if self.delay.is_zero() {
            return;
        }
        // Another program can hold the file for a moment, as an editor or a virus scanner
        // does on Windows: the changes wait again, before those that came meanwhile.
        written.append(&mut waiting.changes);
        waiting.changes = written;
        waiting.failures += 1;
        if waiting.failures <= RETRIES {
            waiting.due = Some(Instant::now() + self.delay);
            self.wake.notify_all();
        }
    }

    fn flush(&self) {
        let mut groups = self.groups();
        if !self.waiting().changes.is_empty() {
            self.write_now(&mut groups, Vec::new());
        }
    }

    /// The thread that writes the changes once they are due, until the file is closed.
    fn write_when_due(&self) {
        let mut waiting = self.waiting();
        while !waiting.closed {
            match waiting.due {
                None => waiting = self.wake.wait(waiting).unwrap_or_else(PoisonError::into_inner),
                Some(due) if Instant::now() < due => {
                    let left = due.saturating_duration_since(Instant::now());
                    waiting = self
                        .wake
                        .wait_timeout(waiting, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                Some(_) => {
                    drop(waiting);
                    self.flush();
                    waiting = self.waiting();
                }
            }
        }
    }

    fn save(&self, groups: &Groups) -> std::io::Result<()> {
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
        std::fs::rename(&partial, &self.path)?;
        self.saves.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// What guards the user, and that the first start is done, which go to the file at once.
fn at_once(group: &str, key: &str) -> bool {
    group == "Security" || (group == "Dialogs" && key == "FirstRun")
}

impl Store for IniFile {
    fn read(&self, group: &str, key: &str, default: &str) -> String {
        let groups = self.shared.groups();
        groups
            .get(group)
            .and_then(|found| found.get(key))
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    fn write(&self, group: &str, key: &str, value: &str) {
        let mut groups = self.shared.groups();
        let change = (group.to_string(), key.to_string(), value.to_string());
        if self.shared.delay.is_zero() || at_once(group, key) {
            self.shared.write_now(&mut groups, vec![change]);
            return;
        }
        if groups.get(group).and_then(|found| found.get(key)).map(String::as_str) == Some(value) {
            return;
        }
        groups
            .entry(group.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        let mut waiting = self.shared.waiting();
        waiting.changes.push(change);
        waiting.due = Some(Instant::now() + self.shared.delay);
        self.shared.wake.notify_all();
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

    /// Waits up to a second for `done`.
    fn soon(done: impl Fn() -> bool) -> bool {
        let until = Instant::now() + Duration::from_secs(1);
        while !done() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    #[test]
    fn changes_close_together_go_to_the_file_once_a_moment_after_the_last() {
        let path = file("delayed");
        let store = IniFile::open(&path).with_delay(Duration::from_millis(150));
        // A grip that is dragged: each step is read back at once, and none is in the file yet.
        for height in 10..=50 {
            store.write("Window", "Height", &height.to_string());
        }
        assert_eq!(store.read("Window", "Height", ""), "50");
        assert!(!path.exists());
        // The thread counts a save once the file is in place: the count, not the file, says
        // that it is done.
        assert!(soon(|| store.saves() == 1));
        assert_eq!(IniFile::open(&path).read("Window", "Height", ""), "50");
        // A change by hand meanwhile stays beside the next one of Kudari.
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("[Window]", "[Window]\nKeepOpen=true")).unwrap();
        store.write("Window", "Width", "80");
        assert!(soon(|| store.saves() == 2));
        assert_eq!(IniFile::open(&path).read("Window", "Width", ""), "80");
        assert_eq!(IniFile::open(&path).read("Window", "KeepOpen", ""), "true");
        // And no more: the change waited for its moment, not for each step.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(store.saves(), 2);
        drop(store);
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn what_guards_the_user_goes_to_the_file_at_once_and_the_rest_with_it() {
        let path = file("guards");
        let store = IniFile::open(&path).with_delay(Duration::from_secs(60));
        store.write("Window", "Width", "80");
        store.write("Security", "AllowAgentControl", "false");
        let again = IniFile::open(&path);
        assert_eq!(again.read("Security", "AllowAgentControl", ""), "false");
        assert_eq!(again.read("Window", "Width", ""), "80");
        store.write("Dialogs", "FirstRun", "false");
        assert_eq!(IniFile::open(&path).read("Dialogs", "FirstRun", ""), "false");
        drop(store);
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn flushing_and_closing_write_what_waits() {
        let path = file("flushed");
        let store = IniFile::open(&path).with_delay(Duration::from_secs(60));
        store.write("Window", "Width", "80");
        store.flush();
        assert_eq!(IniFile::open(&path).read("Window", "Width", ""), "80");
        // Nothing waits: flushing again writes nothing.
        store.flush();
        assert_eq!(store.saves(), 1);
        store.write("Window", "Height", "40");
        drop(store);
        assert_eq!(IniFile::open(&path).read("Window", "Height", ""), "40");
        std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_write_that_fails_is_tried_again_a_moment_later() {
        let path = file("retried");
        let store = IniFile::open(&path).with_delay(Duration::from_millis(50));
        // A folder where the file goes takes no file, as a file that another program holds
        // on Windows takes none for a moment.
        std::fs::create_dir_all(&path).unwrap();
        store.write("Window", "Width", "80");
        assert!(soon(|| store.shared.waiting().failures > 0));
        assert_eq!(store.read("Window", "Width", ""), "80");
        std::fs::remove_dir(&path).unwrap();
        assert!(soon(|| IniFile::open(&path).read("Window", "Width", "") == "80"));
        drop(store);
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
