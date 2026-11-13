// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Keys and the mouse in Kudari's own terms, and what the program in a terminal receives
//! for them. A front end translates the events of its toolkit into these types, once.

use std::ops::BitOr;

use alacritty_terminal::term::TermMode;

/// The modifier keys held, and whether the key is on the numeric keypad.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1);
    pub const CONTROL: Self = Self(2);
    pub const ALT: Self = Self(4);
    pub const META: Self = Self(8);
    /// Not a modifier key: the key pressed is on the numeric keypad.
    pub const KEYPAD: Self = Self(16);

    /// Whether all of `other` is held.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any of `other` is held.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The modifier keys alone, without the keypad.
    #[must_use]
    pub const fn held(self) -> Self {
        Self(self.0 & !Self::KEYPAD.0)
    }

    /// Names joined by `+`, such as `ctrl+shift` in the `UrlHintsModifier` setting. Unknown
    /// names, and `none`, add nothing.
    #[must_use]
    pub fn from_names(text: &str) -> Self {
        text.split('+').fold(Self::NONE, |held, name| {
            held | match name.trim().to_ascii_lowercase().as_str() {
                "shift" => Self::SHIFT,
                "ctrl" | "control" => Self::CONTROL,
                "alt" => Self::ALT,
                "meta" => Self::META,
                _ => Self::NONE,
            }
        })
    }

    /// xterm's modifier parameter for cursor and function keys: 1, plus 1 for Shift, 2 for
    /// Alt and 4 for Ctrl.
    #[must_use]
    pub fn parameter(self) -> i32 {
        1 + i32::from(self.contains(Self::SHIFT))
            + 2 * i32::from(self.contains(Self::ALT))
            + 4 * i32::from(self.contains(Self::CONTROL))
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// A key, as a keyboard names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A key with a character on it: letters as capitals, as keyboards label them, digits
    /// and signs as they are.
    Character(char),
    Escape,
    Tab,
    /// Tab with Shift, which some keyboards report as a key of its own.
    Backtab,
    Backspace,
    Return,
    /// Enter on the keypad.
    Enter,
    Insert,
    Delete,
    Pause,
    Print,
    SysReq,
    Clear,
    Home,
    End,
    Left,
    Up,
    Right,
    Down,
    PageUp,
    PageDown,
    Menu,
    /// F1 to F35.
    Function(u8),
    /// Any other key, such as a modifier or media key: only its text counts.
    Other,
}

/// What the program receives for a key without a text of its own, in the legacy encoding
/// of xterm. `None` for keys whose text is all they send.
#[must_use]
pub fn key_sequence(key: Key, modifiers: Modifiers, app_cursor: bool) -> Option<String> {
    let parameter = modifiers.parameter();
    let cursor = |letter: char| {
        Some(if parameter > 1 {
            format!("\x1b[1;{parameter}{letter}")
        } else if app_cursor {
            format!("\x1bO{letter}")
        } else {
            format!("\x1b[{letter}")
        })
    };
    let tilde = |code: u8| {
        Some(if parameter > 1 {
            format!("\x1b[{code};{parameter}~")
        } else {
            format!("\x1b[{code}~")
        })
    };
    let function = |letter: char| {
        Some(if parameter > 1 {
            format!("\x1b[1;{parameter}{letter}")
        } else {
            format!("\x1bO{letter}")
        })
    };
    match key {
        Key::Escape => Some("\x1b".to_string()),
        Key::Tab => Some("\t".to_string()),
        Key::Backtab => Some("\x1b[Z".to_string()),
        Key::Backspace => Some(
            if modifiers.contains(Modifiers::CONTROL) {
                "\x08"
            } else {
                "\x7f"
            }
            .to_string(),
        ),
        Key::Return | Key::Enter => Some("\r".to_string()),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Left => cursor('D'),
        Key::Up => cursor('A'),
        Key::Right => cursor('C'),
        Key::Down => cursor('B'),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::Function(1) => function('P'),
        Key::Function(2) => function('Q'),
        Key::Function(3) => function('R'),
        Key::Function(4) => function('S'),
        Key::Function(5) => tilde(15),
        Key::Function(6) => tilde(17),
        Key::Function(7) => tilde(18),
        Key::Function(8) => tilde(19),
        Key::Function(9) => tilde(20),
        Key::Function(10) => tilde(21),
        Key::Function(11) => tilde(23),
        Key::Function(12) => tilde(24),
        _ => None,
    }
}

/// Application keypad mode, used by some full-screen programs.
fn keypad_sequence(mode: TermMode, key: Key, modifiers: Modifiers) -> Option<&'static str> {
    if !modifiers.contains(Modifiers::KEYPAD) || !mode.contains(TermMode::APP_KEYPAD) {
        return None;
    }
    Some(match key {
        Key::Character('0') => "\x1bOp",
        Key::Character('1') => "\x1bOq",
        Key::Character('2') => "\x1bOr",
        Key::Character('3') => "\x1bOs",
        Key::Character('4') => "\x1bOt",
        Key::Character('5') => "\x1bOu",
        Key::Character('6') => "\x1bOv",
        Key::Character('7') => "\x1bOw",
        Key::Character('8') => "\x1bOx",
        Key::Character('9') => "\x1bOy",
        Key::Character('*') => "\x1bOj",
        Key::Character('+') => "\x1bOk",
        Key::Character('-') => "\x1bOm",
        Key::Character('.' | ',') => "\x1bOn",
        Key::Character('/') => "\x1bOo",
        Key::Character('=') => "\x1bOX",
        Key::Enter => "\x1bOM",
        _ => return None,
    })
}

/// Kitty keyboard protocol for key presses. Programs that enabled it can tell keys apart
/// that legacy encodings mix up, such as Ctrl+I and Tab.
fn kitty_sequence(mode: TermMode, key: Key, modifiers: Modifiers) -> Option<String> {
    if !mode.intersects(TermMode::DISAMBIGUATE_ESC_CODES | TermMode::REPORT_ALL_KEYS_AS_ESC) {
        return None;
    }
    let report_all = mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC);
    let parameter = modifiers.parameter();
    let code = match key {
        Key::Escape => 27,
        Key::Return | Key::Enter if report_all || parameter > 1 => 13,
        Key::Tab | Key::Backtab if report_all || parameter > 1 => 9,
        Key::Backspace if report_all || parameter > 1 => 127,
        // A text key: with Ctrl or Alt, or always when all keys are reported.
        Key::Character(character) if character == ' ' || character.is_ascii_graphic() => {
            if !report_all && !modifiers.intersects(Modifiers::CONTROL | Modifiers::ALT) {
                return None;
            }
            u32::from(character.to_ascii_lowercase())
        }
        _ => return None,
    };
    Some(if parameter > 1 {
        format!("\x1b[{code};{parameter}u")
    } else {
        format!("\x1b[{code}u")
    })
}

/// What the program receives for a key that no key binding file took: the kitty protocol
/// if the program asked for it, the application keypad, the sequence of a special key, or
/// the text of the key. Alt sends Escape first, as in xterm's default.
#[must_use]
pub fn key_bytes(mode: TermMode, key: Key, modifiers: Modifiers, text: &str) -> Option<Vec<u8>> {
    let sequence = kitty_sequence(mode, key, modifiers)
        .or_else(|| keypad_sequence(mode, key, modifiers).map(str::to_string))
        .or_else(|| key_sequence(key, modifiers, mode.contains(TermMode::APP_CURSOR)))
        .or_else(|| (!text.is_empty()).then(|| text.to_string()))?;
    let mut bytes = sequence.into_bytes();
    if modifiers.contains(Modifiers::ALT) && bytes.first() != Some(&0x1b) {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

/// The mouse buttons a terminal program can learn about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    fn code(self) -> i32 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
        }
    }
}

/// What the mouse did, for a program that tracks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEvent {
    Press(MouseButton),
    Release(MouseButton),
    /// The pointer moved to another cell, with a button held or without.
    Motion(Option<MouseButton>),
    WheelUp,
    WheelDown,
}

fn modifier_code(modifiers: Modifiers) -> i32 {
    4 * i32::from(modifiers.contains(Modifiers::SHIFT))
        + 8 * i32::from(modifiers.contains(Modifiers::ALT))
        + 16 * i32::from(modifiers.contains(Modifiers::CONTROL))
}

/// The report of a mouse event at a zero-based cell, in the encoding the program asked
/// for: SGR, UTF-8 or the classic one. `None` when the program does not want the event, or
/// when the cell lies beyond what the encoding can name.
#[must_use]
pub fn mouse_report(
    mode: TermMode,
    event: MouseEvent,
    modifiers: Modifiers,
    column: usize,
    line: usize,
) -> Option<Vec<u8>> {
    let (code, pressed) = match event {
        MouseEvent::Press(button) => (button.code(), true),
        MouseEvent::Release(button) => (button.code(), false),
        MouseEvent::Motion(Some(button)) if mode.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG) => {
            (32 + button.code(), true)
        }
        MouseEvent::Motion(None) if mode.contains(TermMode::MOUSE_MOTION) => (35, true),
        MouseEvent::Motion(_) => return None,
        MouseEvent::WheelUp => (64, true),
        MouseEvent::WheelDown => (65, true),
    };
    encode_mouse(mode, code + modifier_code(modifiers), column, line, pressed)
}

fn encode_mouse(mode: TermMode, code: i32, column: usize, line: usize, pressed: bool) -> Option<Vec<u8>> {
    if mode.contains(TermMode::SGR_MOUSE) {
        let suffix = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{};{}{suffix}", column + 1, line + 1).into_bytes());
    }
    // Without SGR, a release cannot name its button.
    let code = if pressed { code } else { (code & !3) | 3 };
    let mut report = b"\x1b[M".to_vec();
    report.push(u8::try_from(32 + code).ok()?);
    for position in [column, line] {
        if mode.contains(TermMode::UTF8_MOUSE) {
            let character = char::from_u32(u32::try_from(33 + position).ok()?)?;
            report.extend(character.encode_utf8(&mut [0; 4]).as_bytes());
        } else {
            report.push(u8::try_from(33 + position).ok()?);
        }
    }
    Some(report)
}

/// True if pasting `text` into a plain shell would run a command without confirmation.
#[must_use]
pub fn would_execute(text: &str) -> bool {
    text.contains(['\r', '\n'])
}

/// Removes control characters, so pasted text cannot inject terminal sequences or end
/// a bracketed paste early, and converts line ends to what the Return key sends.
#[must_use]
pub fn prepare_paste(text: &str, bracketed: bool) -> String {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let clean = clean.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        format!("\x1b[200~{clean}\x1b[201~")
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: Modifiers = Modifiers::SHIFT;
    const CONTROL: Modifiers = Modifiers::CONTROL;
    const ALT: Modifiers = Modifiers::ALT;
    const KEYPAD: Modifiers = Modifiers::KEYPAD;
    const NONE: Modifiers = Modifiers::NONE;

    #[test]
    fn modifiers_combine_and_parse() {
        let held = CONTROL | SHIFT;
        assert!(held.contains(CONTROL) && held.intersects(ALT | SHIFT) && !held.contains(ALT));
        assert_eq!((held | KEYPAD).held(), held);
        assert!(NONE.is_empty() && !held.is_empty());
        assert_eq!(Modifiers::from_names("none"), NONE);
        assert_eq!(Modifiers::from_names("ctrl"), CONTROL);
        assert_eq!(Modifiers::from_names("ctrl+shift"), held);
        assert_eq!(Modifiers::from_names("shift+alt"), SHIFT | ALT);
        assert_eq!(Modifiers::from_names("Meta+unknown"), Modifiers::META);
        assert_eq!((SHIFT | ALT | CONTROL | Modifiers::META).parameter(), 8);
    }

    #[test]
    fn arrows_follow_the_cursor_key_mode() {
        assert_eq!(key_sequence(Key::Up, NONE, false).unwrap(), "\x1b[A");
        assert_eq!(key_sequence(Key::Up, NONE, true).unwrap(), "\x1bOA");
    }

    #[test]
    fn modified_keys_carry_the_xterm_parameter() {
        assert_eq!(key_sequence(Key::Right, CONTROL, false).unwrap(), "\x1b[1;5C");
        assert_eq!(key_sequence(Key::Delete, SHIFT, false).unwrap(), "\x1b[3;2~");
        assert_eq!(key_sequence(Key::Function(1), NONE, false).unwrap(), "\x1bOP");
    }

    #[test]
    fn plain_text_keys_have_no_sequence() {
        assert_eq!(key_sequence(Key::Character('A'), NONE, false), None);
        assert_eq!(key_sequence(Key::Function(13), NONE, false), None);
        assert_eq!(key_sequence(Key::Other, NONE, false), None);
    }

    #[test]
    fn every_special_key_has_its_sequence() {
        let expected = [
            (Key::Escape, "\x1b"),
            (Key::Tab, "\t"),
            (Key::Backtab, "\x1b[Z"),
            (Key::Backspace, "\x7f"),
            (Key::Return, "\r"),
            (Key::Enter, "\r"),
            (Key::Insert, "\x1b[2~"),
            (Key::Delete, "\x1b[3~"),
            (Key::Home, "\x1b[H"),
            (Key::End, "\x1b[F"),
            (Key::Left, "\x1b[D"),
            (Key::Down, "\x1b[B"),
            (Key::PageUp, "\x1b[5~"),
            (Key::PageDown, "\x1b[6~"),
            (Key::Function(1), "\x1bOP"),
            (Key::Function(2), "\x1bOQ"),
            (Key::Function(3), "\x1bOR"),
            (Key::Function(4), "\x1bOS"),
            (Key::Function(5), "\x1b[15~"),
            (Key::Function(6), "\x1b[17~"),
            (Key::Function(7), "\x1b[18~"),
            (Key::Function(8), "\x1b[19~"),
            (Key::Function(9), "\x1b[20~"),
            (Key::Function(10), "\x1b[21~"),
            (Key::Function(11), "\x1b[23~"),
            (Key::Function(12), "\x1b[24~"),
        ];
        for (key, sequence) in expected {
            assert_eq!(key_sequence(key, NONE, false).as_deref(), Some(sequence), "{key:?}");
        }
        assert_eq!(key_sequence(Key::Backspace, CONTROL, false).as_deref(), Some("\x08"));
        assert_eq!(
            key_sequence(Key::Function(1), SHIFT, false).as_deref(),
            Some("\x1b[1;2P")
        );
        assert_eq!(
            key_sequence(Key::Function(5), ALT, false).as_deref(),
            Some("\x1b[15;3~")
        );
    }

    #[test]
    fn the_keypad_only_changes_in_application_mode() {
        let five = Key::Character('5');
        assert_eq!(keypad_sequence(TermMode::APP_KEYPAD, five, KEYPAD), Some("\x1bOu"));
        assert_eq!(keypad_sequence(TermMode::empty(), five, KEYPAD), None);
        assert_eq!(keypad_sequence(TermMode::APP_KEYPAD, five, NONE), None);
    }

    #[test]
    fn the_whole_keypad_has_application_sequences() {
        let mode = TermMode::APP_KEYPAD;
        let digits: String = ('0'..='9')
            .map(|digit| {
                keypad_sequence(mode, Key::Character(digit), KEYPAD)
                    .unwrap()
                    .chars()
                    .last()
                    .unwrap()
            })
            .collect();
        assert_eq!(digits, "pqrstuvwxy");
        let signs = [
            ('*', 'j'),
            ('+', 'k'),
            ('-', 'm'),
            ('.', 'n'),
            (',', 'n'),
            ('/', 'o'),
            ('=', 'X'),
        ];
        for (sign, letter) in signs {
            assert_eq!(
                keypad_sequence(mode, Key::Character(sign), KEYPAD),
                Some(format!("\x1bO{letter}").as_str())
            );
        }
        assert_eq!(keypad_sequence(mode, Key::Enter, KEYPAD), Some("\x1bOM"));
        assert_eq!(keypad_sequence(mode, Key::Character('A'), KEYPAD), None);
    }

    #[test]
    fn kitty_keys_need_the_protocol() {
        assert_eq!(kitty_sequence(TermMode::empty(), Key::Escape, NONE), None);
        let mode = TermMode::DISAMBIGUATE_ESC_CODES;
        assert_eq!(kitty_sequence(mode, Key::Escape, NONE).as_deref(), Some("\x1b[27u"));
        assert_eq!(
            kitty_sequence(mode, Key::Character('I'), CONTROL).as_deref(),
            Some("\x1b[105;5u")
        );
        assert_eq!(kitty_sequence(mode, Key::Character('A'), NONE), None);
        assert_eq!(kitty_sequence(mode, Key::Return, NONE), None);
        assert_eq!(kitty_sequence(mode, Key::Return, SHIFT).as_deref(), Some("\x1b[13;2u"));
        assert_eq!(kitty_sequence(mode, Key::Up, CONTROL), None);
    }

    #[test]
    fn kitty_can_report_every_key() {
        let mode = TermMode::REPORT_ALL_KEYS_AS_ESC;
        assert_eq!(
            kitty_sequence(mode, Key::Character('A'), NONE).as_deref(),
            Some("\x1b[97u")
        );
        assert_eq!(kitty_sequence(mode, Key::Return, NONE).as_deref(), Some("\x1b[13u"));
        assert_eq!(kitty_sequence(mode, Key::Tab, NONE).as_deref(), Some("\x1b[9u"));
        assert_eq!(kitty_sequence(mode, Key::Backspace, NONE).as_deref(), Some("\x1b[127u"));
        assert_eq!(
            kitty_sequence(mode, Key::Character(' '), NONE).as_deref(),
            Some("\x1b[32u")
        );
    }

    #[test]
    fn keys_become_bytes_in_order_of_preference() {
        let plain = TermMode::empty();
        assert_eq!(key_bytes(plain, Key::Up, NONE, "").unwrap(), b"\x1b[A");
        assert_eq!(key_bytes(TermMode::APP_CURSOR, Key::Up, NONE, "").unwrap(), b"\x1bOA");
        assert_eq!(key_bytes(plain, Key::Character('A'), SHIFT, "A").unwrap(), b"A");
        assert_eq!(key_bytes(plain, Key::Other, NONE, "ä").unwrap(), "ä".as_bytes());
        assert_eq!(key_bytes(plain, Key::Other, NONE, ""), None);
        // Alt sends Escape first, unless the sequence starts with it already.
        assert_eq!(key_bytes(plain, Key::Character('B'), ALT, "b").unwrap(), b"\x1bb");
        assert_eq!(key_bytes(plain, Key::Up, ALT, "").unwrap(), b"\x1b[1;3A");
        let keypad = key_bytes(TermMode::APP_KEYPAD, Key::Character('5'), KEYPAD, "5");
        assert_eq!(keypad.unwrap(), b"\x1bOu");
        let kitty = key_bytes(TermMode::DISAMBIGUATE_ESC_CODES, Key::Escape, NONE, "\x1b");
        assert_eq!(kitty.unwrap(), b"\x1b[27u");
    }

    #[test]
    fn sgr_reports_name_the_button_on_release() {
        let right = MouseButton::Right;
        let mode = TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_report(mode, MouseEvent::Press(right), NONE, 4, 9).unwrap(),
            b"\x1b[<2;5;10M"
        );
        assert_eq!(
            mouse_report(mode, MouseEvent::Release(right), NONE, 4, 9).unwrap(),
            b"\x1b[<2;5;10m"
        );
    }

    #[test]
    fn classic_reports_use_offset_bytes() {
        let left = MouseButton::Left;
        let mode = TermMode::empty();
        assert_eq!(
            mouse_report(mode, MouseEvent::Press(left), NONE, 4, 9).unwrap(),
            b"\x1b[M\x20\x25\x2a"
        );
        assert_eq!(
            mouse_report(mode, MouseEvent::Release(left), NONE, 4, 9).unwrap(),
            b"\x1b[M\x23\x25\x2a"
        );
    }

    #[test]
    fn classic_reports_stop_at_their_range() {
        let press = MouseEvent::Press(MouseButton::Left);
        assert_eq!(mouse_report(TermMode::empty(), press, NONE, 300, 0), None);
    }

    #[test]
    fn utf8_reports_extend_the_range() {
        let press = MouseEvent::Press(MouseButton::Left);
        let report = mouse_report(TermMode::UTF8_MOUSE, press, NONE, 300, 0).unwrap();
        assert_eq!(
            report,
            [b"\x1b[M\x20".as_slice(), "\u{14d}".as_bytes(), b"\x21"].concat()
        );
    }

    #[test]
    fn buttons_and_modifiers_have_their_codes() {
        let sgr = TermMode::SGR_MOUSE;
        let codes = [MouseButton::Left, MouseButton::Middle, MouseButton::Right]
            .map(|button| mouse_report(sgr, MouseEvent::Press(button), NONE, 0, 0).unwrap());
        assert_eq!(
            codes,
            [
                b"\x1b[<0;1;1M".to_vec(),
                b"\x1b[<1;1;1M".to_vec(),
                b"\x1b[<2;1;1M".to_vec()
            ]
        );
        let held = mouse_report(sgr, MouseEvent::Press(MouseButton::Left), SHIFT | CONTROL, 0, 0);
        assert_eq!(held.unwrap(), b"\x1b[<20;1;1M");
        let alt = mouse_report(sgr, MouseEvent::Press(MouseButton::Left), ALT, 0, 0);
        assert_eq!(alt.unwrap(), b"\x1b[<8;1;1M");
        assert_eq!(
            mouse_report(sgr, MouseEvent::WheelUp, NONE, 0, 0).unwrap(),
            b"\x1b[<64;1;1M"
        );
        assert_eq!(
            mouse_report(sgr, MouseEvent::WheelDown, NONE, 0, 0).unwrap(),
            b"\x1b[<65;1;1M"
        );
    }

    #[test]
    fn motion_is_reported_only_when_asked_for() {
        let drag = MouseEvent::Motion(Some(MouseButton::Left));
        let hover = MouseEvent::Motion(None);
        let sgr = TermMode::SGR_MOUSE;
        assert_eq!(mouse_report(sgr, drag, NONE, 0, 0), None);
        assert_eq!(
            mouse_report(sgr | TermMode::MOUSE_DRAG, drag, NONE, 0, 0).unwrap(),
            b"\x1b[<32;1;1M"
        );
        assert_eq!(mouse_report(sgr | TermMode::MOUSE_DRAG, hover, NONE, 0, 0), None);
        assert_eq!(
            mouse_report(sgr | TermMode::MOUSE_MOTION, hover, NONE, 0, 0).unwrap(),
            b"\x1b[<35;1;1M"
        );
    }

    #[test]
    fn pasted_text_cannot_end_a_bracketed_paste() {
        let paste = prepare_paste("safe\x1b[201~rm -rf x\r", true);
        assert_eq!(paste, "\x1b[200~safe[201~rm -rf x\r\x1b[201~");
    }

    #[test]
    fn paste_keeps_tabs_and_converts_line_ends() {
        assert_eq!(prepare_paste("a\tb\r\nc\nd", false), "a\tb\rc\rd");
    }

    #[test]
    fn paste_removes_other_control_characters() {
        assert_eq!(prepare_paste("a\x07b\x03c\u{9b}d", false), "abcd");
    }

    #[test]
    fn line_breaks_count_as_executing() {
        assert!(would_execute("ls\n"));
        assert!(would_execute("a\nb"));
        assert!(!would_execute("just one line"));
    }
}
