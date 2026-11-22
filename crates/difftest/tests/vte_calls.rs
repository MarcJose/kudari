// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The copy of `vte` makes the same calls of `Perform` as the original, for any bytes: its
//! changes are for speed alone. The copy gets them in pieces, as a PTY reads, the original
//! all at once.

use kudari_difftest::{Numbers, cuts};

/// A `Perform` of the given `vte` that writes down each call, with what it got.
macro_rules! recorder {
    () => {
        #[derive(Default)]
        pub struct Calls(pub Vec<String>);

        impl Calls {
            fn params(params: &Params) -> Vec<Vec<u16>> {
                params.iter().map(<[u16]>::to_vec).collect()
            }
        }

        impl Perform for Calls {
            fn print(&mut self, character: char) {
                self.0.push(format!("print {character:?}"));
            }
            fn execute(&mut self, byte: u8) {
                self.0.push(format!("execute {byte}"));
            }
            fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
                self.0.push(format!(
                    "hook {:?} {intermediates:?} {ignore} {action:?}",
                    Self::params(params)
                ));
            }
            fn put(&mut self, byte: u8) {
                self.0.push(format!("put {byte}"));
            }
            fn unhook(&mut self) {
                self.0.push("unhook".into());
            }
            fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
                self.0.push(format!("osc {params:?} {bell_terminated}"));
            }
            fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
                self.0.push(format!(
                    "csi {:?} {intermediates:?} {ignore} {action:?}",
                    Self::params(params)
                ));
            }
            fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
                self.0.push(format!("esc {intermediates:?} {ignore} {byte}"));
            }
        }
    };
}

mod copy {
    pub use alacritty_terminal::vte::{Params, Parser, Perform};
    recorder!();
}

mod original {
    pub use original_vte::{Params, Parser, Perform};
    recorder!();
}

/// Bytes as a program writes them: text, wide and combining characters, controls, sequences
/// of every kind with parameters, and now and then any byte, broken UTF-8 among them.
fn output(numbers: &mut Numbers) -> Vec<u8> {
    let parts = [
        "plain text ",
        "0123456789",
        "ä",
        "日本",
        "e\u{301}",
        "🙂",
        "\r\n",
        "\t",
        "\x08",
        "\x07",
        "\x1b[31m",
        "\x1b[1;38;2;10;20;30m",
        "\x1b[38:5:200m",
        "\x1b[0m",
        "\x1b[12;40H",
        "\x1b[K",
        "\x1b[2J",
        "\x1b[?1049h",
        "\x1b[?2026h",
        "\x1b[?25l",
        "\x1b[5S",
        "\x1b[3@",
        "\x1b[>c",
        "\x1b]0;title\x07",
        "\x1b]8;;https://x.example\x1b\\",
        "\x1b]52;c;aGk=\x07",
        "\x1bP1$r\x1b\\",
        "\x1b7",
        "\x1b8",
        "\x1bc",
        "\x1b(B",
        "\x1b[1;2;3;4;5;6;7;8;9;10;11;12;13;14;15;16;17;18m",
        "\x1b[",
        "\x1b]",
        ";",
        ":",
        "\u{9b}2J",
    ];
    let mut bytes = Vec::new();
    for _ in 0..numbers.below(400) {
        if numbers.below(10) == 0 {
            bytes.push(numbers.number() as u8);
        } else {
            bytes.extend_from_slice(numbers.pick(&parts).as_bytes());
        }
    }
    bytes
}

#[test]
fn the_copy_of_vte_performs_what_the_original_does() {
    for seed in 1..=3000 {
        let mut numbers = Numbers::new(seed);
        let bytes = output(&mut numbers);
        let pieces = cuts(&mut numbers, bytes.len());
        let mut expected = original::Calls::default();
        let mut reference = original::Parser::new();
        let mut got = copy::Calls::default();
        let mut parser = copy::Parser::new();
        let mut from = 0;
        for &to in &pieces {
            reference.advance(&mut expected, &bytes[from..to]);
            parser.advance(&mut got, &bytes[from..to]);
            from = to;
        }
        if got.0 != expected.0 {
            let at = got
                .0
                .iter()
                .zip(&expected.0)
                .position(|(a, b)| a != b)
                .unwrap_or(got.0.len().min(expected.0.len()));
            panic!(
                "seed {seed}: call {at} differs: copy {:?}, original {:?}\ninput {:?}",
                got.0.get(at),
                expected.0.get(at),
                String::from_utf8_lossy(&bytes)
            );
        }
    }
}
