// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Key binding files in Konsole's `.keytab` format. A file only has to name the keys it
//! changes: all others keep their built-in sequences.
//!
//! Each rule is one line: `key`, a key name, conditions, a colon, and what the key does.
//!
//! ```text
//! keyboard "Custom keys"
//! key Up+Shift-AppCuKeys : "\E[1;2A"
//! key PgUp+Shift : scrollPageUp
//! ```
//!
//! A condition is `+` or `-` and a modifier key (`Shift`, `Ctrl`, `Alt`, `Meta`, `KeyPad`,
//! `AnyMod` for any of the first four) or a terminal mode (`AppCuKeys`, `AppScreen`,
//! `NewLine`, `AppKeyPad`, `Ansi`): `+` requires it, `-` forbids it, and what a rule leaves
//! out does not matter. The first rule that fits wins. A text may use `\E` for escape, `\t`,
//! `\r`, `\n`, `\b`, `\xHH` and quoted backslashes and quotes; in rules with `+AnyMod`, a `*`
//! becomes xterm's modifier number.

use std::fs;
use std::path::PathBuf;

use crate::input::{Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Erase,
    PageUp,
    PageDown,
    LineUp,
    LineDown,
    Top,
    Bottom,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Text(Vec<u8>),
    Command(Command),
}

/// The terminal modes a rule can ask for.
#[derive(Clone, Copy, Debug, Default)]
pub struct Modes {
    pub app_cursor: bool,
    pub app_screen: bool,
    pub new_line: bool,
    pub app_keypad: bool,
}

/// Something a rule requires or forbids.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Condition {
    Modifier(Modifiers),
    AnyModifier,
    AppCursor,
    AppScreen,
    NewLine,
    AppKeypad,
    /// Always true: Kudari has no VT52 mode.
    Ansi,
}

impl Condition {
    fn named(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "shift" => Self::Modifier(Modifiers::SHIFT),
            "ctrl" | "control" => Self::Modifier(Modifiers::CONTROL),
            "alt" => Self::Modifier(Modifiers::ALT),
            "meta" => Self::Modifier(Modifiers::META),
            "keypad" => Self::Modifier(Modifiers::KEYPAD),
            "anymod" | "anymodifier" => Self::AnyModifier,
            "appcukeys" | "appcursorkeys" => Self::AppCursor,
            "appscreen" => Self::AppScreen,
            "newline" => Self::NewLine,
            "appkeypad" => Self::AppKeypad,
            "ansi" => Self::Ansi,
            _ => return None,
        })
    }

    fn holds(self, modifiers: Modifiers, modes: Modes) -> bool {
        match self {
            Self::Modifier(modifier) => modifiers.contains(modifier),
            Self::AnyModifier => !modifiers.held().is_empty(),
            Self::AppCursor => modes.app_cursor,
            Self::AppScreen => modes.app_screen,
            Self::NewLine => modes.new_line,
            Self::AppKeypad => modes.app_keypad,
            Self::Ansi => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    key: Key,
    /// Each condition with whether the rule requires (`true`) or forbids it.
    conditions: Vec<(Condition, bool)>,
    action: Action,
}

/// Konsole's names for keys without a character of their own.
const NAMED_KEYS: [(&str, Key); 33] = [
    ("escape", Key::Escape),
    ("esc", Key::Escape),
    ("tab", Key::Tab),
    ("backtab", Key::Backtab),
    ("backspace", Key::Backspace),
    ("return", Key::Return),
    ("enter", Key::Enter),
    ("insert", Key::Insert),
    ("ins", Key::Insert),
    ("delete", Key::Delete),
    ("del", Key::Delete),
    ("pause", Key::Pause),
    ("print", Key::Print),
    ("sysreq", Key::SysReq),
    ("clear", Key::Clear),
    ("home", Key::Home),
    ("end", Key::End),
    ("left", Key::Left),
    ("up", Key::Up),
    ("right", Key::Right),
    ("down", Key::Down),
    ("pageup", Key::PageUp),
    ("pgup", Key::PageUp),
    ("prior", Key::PageUp),
    ("pagedown", Key::PageDown),
    ("pgdown", Key::PageDown),
    ("next", Key::PageDown),
    ("menu", Key::Menu),
    ("space", Key::Character(' ')),
    ("plus", Key::Character('+')),
    ("minus", Key::Character('-')),
    ("asterisk", Key::Character('*')),
    ("slash", Key::Character('/')),
];

/// The key of a key name: a name of the list before, `F1` to `F35`, or a single character.
fn key_code(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    if let Some((_, key)) = NAMED_KEYS.iter().find(|(known, _)| *known == lower) {
        return Some(*key);
    }
    if let Some(number) = lower.strip_prefix('f').and_then(|digits| digits.parse::<u8>().ok())
        && (1..=35).contains(&number)
    {
        return Some(Key::Function(number));
    }
    let mut characters = name.chars();
    match (characters.next(), characters.next()) {
        // Keys name letters by their capital.
        (Some(character), None) if character.is_ascii_graphic() => Some(Key::Character(character.to_ascii_uppercase())),
        _ => None,
    }
}

fn command(name: &str) -> Option<Command> {
    Some(match name.to_ascii_lowercase().as_str() {
        "erase" => Command::Erase,
        "scrollpageup" => Command::PageUp,
        "scrollpagedown" => Command::PageDown,
        "scrolllineup" => Command::LineUp,
        "scrolllinedown" => Command::LineDown,
        "scrolluptotop" => Command::Top,
        "scrolldowntobottom" => Command::Bottom,
        _ => return None,
    })
}

/// The bytes of a quoted text, from just after its opening quote. `None` if it never ends.
fn text_bytes(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '"' => return Some(bytes),
            '\\' => match characters.next()? {
                'E' | 'e' => bytes.push(0x1b),
                't' => bytes.push(b'\t'),
                'r' => bytes.push(b'\r'),
                'n' => bytes.push(b'\n'),
                'b' => bytes.push(0x08),
                'x' => {
                    let mut value = 0u8;
                    for _ in 0..2 {
                        let Some(digit) = characters.peek().and_then(|digit| digit.to_digit(16)) else {
                            break;
                        };
                        value = value.wrapping_mul(16).wrapping_add(digit as u8);
                        characters.next();
                    }
                    bytes.push(value);
                }
                other => {
                    let mut buffer = [0; 4];
                    bytes.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
                }
            },
            other => {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    None
}

/// Splits `Up+Shift-AppCuKeys` into the key and its conditions. A key name is a word, or a
/// single character such as `+`, which lets `+` and `-` be keys themselves.
fn key_and_conditions(spec: &str) -> Option<(Key, Vec<(Condition, bool)>)> {
    let first = spec.chars().next()?;
    let name_length = if first.is_ascii_alphanumeric() {
        spec.find(|character: char| !character.is_ascii_alphanumeric())
            .unwrap_or(spec.len())
    } else {
        first.len_utf8()
    };
    let (name, mut rest) = spec.split_at(name_length);
    let key = key_code(name)?;
    let mut conditions = Vec::new();
    while let Some(sign) = rest.chars().next() {
        let wanted = match sign {
            '+' => true,
            '-' => false,
            _ => return None,
        };
        let word = &rest[1..];
        let end = word.find(['+', '-']).unwrap_or(word.len());
        conditions.push((Condition::named(&word[..end])?, wanted));
        rest = &word[end..];
    }
    Some((key, conditions))
}

fn parse_line(line: &str) -> Option<Rule> {
    let rule = line.trim().strip_prefix("key")?;
    if !rule.starts_with(char::is_whitespace) {
        return None;
    }
    let (spec, action) = rule.split_once(':')?;
    let (key, conditions) = key_and_conditions(&spec.split_whitespace().collect::<String>())?;
    let action = action.trim();
    let action = match action.strip_prefix('"') {
        Some(text) => Action::Text(text_bytes(text)?),
        None => Action::Command(command(action)?),
    };
    Some(Rule {
        key,
        conditions,
        action,
    })
}

/// Reads the rules of a file; lines it does not understand are left out.
pub fn parse(text: &str) -> Vec<Rule> {
    text.lines().filter_map(parse_line).collect()
}

/// xterm's number for the modifiers held: 1 plus 1 for Shift, 2 for Alt, 4 for Ctrl, 8 for Meta.
fn modifier_number(modifiers: Modifiers) -> u8 {
    [
        (Modifiers::SHIFT, 1),
        (Modifiers::ALT, 2),
        (Modifiers::CONTROL, 4),
        (Modifiers::META, 8),
    ]
    .iter()
    .filter(|(modifier, _)| modifiers.contains(*modifier))
    .map(|(_, value)| value)
    .sum::<u8>()
        + 1
}

/// What the first fitting rule makes of a key press.
#[must_use]
pub fn find(rules: &[Rule], key: Key, modifiers: Modifiers, modes: Modes) -> Option<Action> {
    let rule = rules.iter().find(|rule| {
        rule.key == key
            && rule
                .conditions
                .iter()
                .all(|(condition, wanted)| condition.holds(modifiers, modes) == *wanted)
    })?;
    let any_modifier = rule.conditions.contains(&(Condition::AnyModifier, true));
    Some(match &rule.action {
        Action::Text(bytes) if any_modifier => {
            let number = modifier_number(modifiers).to_string();
            let mut expanded = Vec::with_capacity(bytes.len());
            for byte in bytes {
                if *byte == b'*' {
                    expanded.extend_from_slice(number.as_bytes());
                } else {
                    expanded.push(*byte);
                }
            }
            Action::Text(expanded)
        }
        action => action.clone(),
    })
}

fn directory() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::process::home().map(|home| home.join(".local/share")))?;
    Some(data.join("kudari/keytabs"))
}

/// The names of the installed files.
#[must_use]
pub fn available() -> Vec<String> {
    let mut names: Vec<String> = directory()
        .and_then(|directory| fs::read_dir(directory).ok())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some(entry.file_name().to_str()?.strip_suffix(".keytab")?.to_string()))
        .collect();
    names.sort();
    names
}

/// Loads the rules of a file by name; an empty name means the built-in keys.
pub fn load(name: &str) -> Vec<Rule> {
    if name.is_empty() || name.contains('/') {
        return Vec::new();
    }
    directory()
        .and_then(|directory| fs::read_to_string(directory.join(format!("{name}.keytab"))).ok())
        .map_or_else(Vec::new, |text| parse(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: Key = Key::Up;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const CONTROL: Modifiers = Modifiers::CONTROL;
    const KEYPAD: Modifiers = Modifiers::KEYPAD;
    const NONE: Modifiers = Modifiers::NONE;

    #[test]
    fn rules_match_by_key_modifiers_and_modes() {
        let rules = parse(
            "# comment\nkeyboard \"Test\"\nkey Up+Shift : scrollLineUp\nkey Up-Shift+AppCuKeys : \"\\EOA\"\nkey Up-Shift-AppCuKeys : \"\\E[A\"\nkey Q+Ctrl : \"\\x11x\"\n",
        );
        assert_eq!(rules.len(), 4);
        let app = Modes {
            app_cursor: true,
            ..Modes::default()
        };
        assert_eq!(
            find(&rules, UP, SHIFT, Modes::default()),
            Some(Action::Command(Command::LineUp))
        );
        assert_eq!(find(&rules, UP, NONE, app), Some(Action::Text(b"\x1bOA".to_vec())));
        assert_eq!(
            find(&rules, UP, NONE, Modes::default()),
            Some(Action::Text(b"\x1b[A".to_vec()))
        );
        assert_eq!(
            find(&rules, Key::Character('Q'), CONTROL, Modes::default()),
            Some(Action::Text(vec![0x11, b'x']))
        );
        assert_eq!(find(&rules, Key::Character('Q'), NONE, Modes::default()), None);
    }

    #[test]
    fn the_modifier_wildcard_is_expanded() {
        let rules = parse("key F5+AnyMod : \"\\E[15;*~\"\n");
        let f5 = Key::Function(5);
        assert_eq!(
            find(&rules, f5, CONTROL | SHIFT, Modes::default()),
            Some(Action::Text(b"\x1b[15;6~".to_vec()))
        );
        assert_eq!(find(&rules, f5, NONE, Modes::default()), None);
    }

    #[test]
    fn random_files_do_no_harm() {
        let sample = "keyboard \"x\"\nkey Up+Shift-AppCuKeys : \"\\E[1;*A\"\nkey F5+AnyMod : scrollPageUp\n";
        for input in crate::fuzz::inputs(&[sample]) {
            let rules = parse(&input);
            let _ = find(&rules, UP, SHIFT, Modes::default());
        }
    }

    #[test]
    fn unknown_lines_are_skipped() {
        assert!(parse("key Nonsense : \"x\"\nkey Up : unknownCommand\nkey Up+Bogus : \"x\"\ngarbage\n").is_empty());
    }

    #[test]
    fn plus_and_minus_can_be_keys() {
        let rules = parse("key +-Shift : \"p\"\n");
        assert_eq!(
            find(&rules, Key::Character('+'), NONE, Modes::default()),
            Some(Action::Text(b"p".to_vec()))
        );
    }

    #[test]
    fn named_and_function_keys_are_known() {
        assert_eq!(key_code("PgUp"), Some(Key::PageUp));
        assert_eq!(key_code("Escape"), Some(Key::Escape));
        assert_eq!(key_code("F1"), Some(Key::Function(1)));
        assert_eq!(key_code("F35"), Some(Key::Function(35)));
        assert_eq!(key_code("F36"), None);
        assert_eq!(key_code("a"), Some(Key::Character('A')));
    }

    #[test]
    fn texts_understand_their_escapes() {
        assert_eq!(text_bytes(r#"\E[\x7f\t\\\"x""#), Some(b"\x1b[\x7f\t\\\"x".to_vec()));
        assert_eq!(text_bytes("never closed"), None);
    }

    #[test]
    fn ansi_always_holds_and_keypad_is_a_modifier() {
        let rules = parse("key Up+Ansi+KeyPad : \"k\"\n");
        assert_eq!(
            find(&rules, UP, KEYPAD, Modes::default()),
            Some(Action::Text(b"k".to_vec()))
        );
        assert_eq!(find(&rules, UP, NONE, Modes::default()), None);
    }
}
