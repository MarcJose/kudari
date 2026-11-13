// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Qt's key codes, modifiers and mouse buttons in the terms of the core. The only place
//! that knows their numbers.

use crate::input::{Key, Modifiers, MouseButton};

const SHIFT: i32 = 0x0200_0000;
const CONTROL: i32 = 0x0400_0000;
const ALT: i32 = 0x0800_0000;
const META: i32 = 0x1000_0000;
const KEYPAD: i32 = 0x2000_0000;

/// `Qt::KeyboardModifiers`.
#[must_use]
pub fn modifiers(bits: i32) -> Modifiers {
    [
        (SHIFT, Modifiers::SHIFT),
        (CONTROL, Modifiers::CONTROL),
        (ALT, Modifiers::ALT),
        (META, Modifiers::META),
        (KEYPAD, Modifiers::KEYPAD),
    ]
    .into_iter()
    .filter(|(bit, _)| bits & bit != 0)
    .fold(Modifiers::NONE, |held, (_, modifier)| held | modifier)
}

/// `Qt::Key`: special keys from `0x0100_0000` on, below that the character of the key,
/// letters as capitals.
pub fn key(code: i32) -> Key {
    match code {
        0x0100_0000 => Key::Escape,
        0x0100_0001 => Key::Tab,
        0x0100_0002 => Key::Backtab,
        0x0100_0003 => Key::Backspace,
        0x0100_0004 => Key::Return,
        0x0100_0005 => Key::Enter,
        0x0100_0006 => Key::Insert,
        0x0100_0007 => Key::Delete,
        0x0100_0008 => Key::Pause,
        0x0100_0009 => Key::Print,
        0x0100_000a => Key::SysReq,
        0x0100_000b => Key::Clear,
        0x0100_0010 => Key::Home,
        0x0100_0011 => Key::End,
        0x0100_0012 => Key::Left,
        0x0100_0013 => Key::Up,
        0x0100_0014 => Key::Right,
        0x0100_0015 => Key::Down,
        0x0100_0016 => Key::PageUp,
        0x0100_0017 => Key::PageDown,
        0x0100_0055 => Key::Menu,
        0x0100_0030..=0x0100_0052 => Key::Function((code - 0x0100_002f) as u8),
        0x20..0x0100_0000 => u32::try_from(code)
            .ok()
            .and_then(char::from_u32)
            .filter(|character| !character.is_control())
            .map_or(Key::Other, Key::Character),
        _ => Key::Other,
    }
}

/// `Qt::MouseButton`, for the buttons a terminal program can learn about.
#[must_use]
pub fn button(code: i32) -> Option<MouseButton> {
    match code {
        1 => Some(MouseButton::Left),
        2 => Some(MouseButton::Right),
        4 => Some(MouseButton::Middle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_keep_their_meaning() {
        assert_eq!(modifiers(0), Modifiers::NONE);
        assert_eq!(modifiers(CONTROL | SHIFT), Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(
            modifiers(ALT | META | KEYPAD),
            Modifiers::ALT | Modifiers::META | Modifiers::KEYPAD
        );
    }

    #[test]
    fn keys_keep_their_meaning() {
        let special = [
            (0x0100_0000, Key::Escape),
            (0x0100_0001, Key::Tab),
            (0x0100_0002, Key::Backtab),
            (0x0100_0003, Key::Backspace),
            (0x0100_0004, Key::Return),
            (0x0100_0005, Key::Enter),
            (0x0100_0006, Key::Insert),
            (0x0100_0007, Key::Delete),
            (0x0100_0008, Key::Pause),
            (0x0100_0009, Key::Print),
            (0x0100_000a, Key::SysReq),
            (0x0100_000b, Key::Clear),
            (0x0100_0010, Key::Home),
            (0x0100_0011, Key::End),
            (0x0100_0012, Key::Left),
            (0x0100_0013, Key::Up),
            (0x0100_0014, Key::Right),
            (0x0100_0015, Key::Down),
            (0x0100_0016, Key::PageUp),
            (0x0100_0017, Key::PageDown),
            (0x0100_0055, Key::Menu),
            (0x0100_0030, Key::Function(1)),
            (0x0100_0052, Key::Function(35)),
        ];
        for (code, expected) in special {
            assert_eq!(key(code), expected, "{code:#x}");
        }
        assert_eq!(key(0x41), Key::Character('A'));
        assert_eq!(key(0x20), Key::Character(' '));
        assert_eq!(key(0xc4), Key::Character('Ä'));
        // Modifier keys alone and unknown keys have no meaning of their own.
        assert_eq!(key(0x0100_0020), Key::Other);
        assert_eq!(key(0x01ff_ffff), Key::Other);
        assert_eq!(key(0x1b), Key::Other);
    }

    #[test]
    fn buttons_keep_their_meaning() {
        assert_eq!(
            [1, 2, 4, 8].map(button),
            [
                Some(MouseButton::Left),
                Some(MouseButton::Right),
                Some(MouseButton::Middle),
                None
            ]
        );
    }
}
