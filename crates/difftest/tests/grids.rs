// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The copy of `alacritty_terminal` leaves the same screen and history as the original, for
//! the output its changes for speed touch: text of any width, line breaks, colors and moves
//! of the cursor. Its fixes change what other sequences do, as deleting characters, erasing
//! to the cursor or a scrolling region, so those are left out here; `upstream_tests.rs` of
//! the core tests each of them.

use kudari_difftest::{Numbers, cuts};

/// What a terminal shows and keeps, as text: each cell of the history and the screen, and the
/// cursor. The types of the two crates differ, the text of their `Debug` does not.
macro_rules! seen {
    ($alacritty:ident, $bytes:expr, $pieces:expr) => {{
        use $alacritty::event::VoidListener;
        use $alacritty::grid::Dimensions;
        use $alacritty::index::{Column, Line};
        use $alacritty::term::test::TermSize;
        use $alacritty::term::{Config, Term};
        use $alacritty::vte::ansi::{Processor, StdSyncHandler};

        let config = Config {
            scrolling_history: 200,
            ..Config::default()
        };
        let mut term = Term::new(config, &TermSize::new(40, 12), VoidListener);
        let mut parser: Processor<StdSyncHandler> = Processor::new();
        let mut from = 0;
        for &to in $pieces {
            parser.advance(&mut term, &$bytes[from..to]);
            from = to;
        }
        let grid = term.grid();
        let mut text = Vec::new();
        for line in grid.topmost_line().0..=grid.bottommost_line().0 {
            for column in 0..grid.columns() {
                text.push(format!("{line},{column}: {:?}", grid[Line(line)][Column(column)]));
            }
        }
        text.push(format!("cursor {:?}", grid.cursor.point));
        text
    }};
}

/// Output of the kinds that the changes for speed touch.
fn output(numbers: &mut Numbers) -> Vec<u8> {
    let parts = [
        "plain text ",
        "0123456789",
        "ä",
        "日本",
        "e\u{301}",
        "🙂",
        "\r\n",
        "\n",
        "\r",
        "\t",
        "\x08",
        "\x1b[31m",
        "\x1b[1;38;2;10;20;30m",
        "\x1b[48;5;200m",
        "\x1b[4m",
        "\x1b[7m",
        "\x1b[0m",
        "\x1b[5;30H",
        "\x1b[H",
        "\x1b[3C",
        "\x1b[2D",
        "\x1b[K",
        "\x1b[2K",
        "\x1b[J",
        "\x1b[2J",
        "a fairly long line of text that goes past the end of a line of forty columns ",
    ];
    let mut bytes = Vec::new();
    for _ in 0..numbers.below(300) {
        bytes.extend_from_slice(numbers.pick(&parts).as_bytes());
    }
    bytes
}

#[test]
fn the_copy_of_alacritty_terminal_shows_what_the_original_does() {
    for seed in 1..=500 {
        let mut numbers = Numbers::new(seed);
        let bytes = output(&mut numbers);
        let pieces = cuts(&mut numbers, bytes.len());
        let expected = seen!(original_alacritty_terminal, bytes, &pieces);
        let got = seen!(alacritty_terminal, bytes, &pieces);
        if got != expected {
            let at = got.iter().zip(&expected).position(|(a, b)| a != b).unwrap_or(0);
            panic!(
                "seed {seed}: copy {:?}, original {:?}\ninput {:?}",
                got.get(at),
                expected.get(at),
                String::from_utf8_lossy(&bytes)
            );
        }
    }
}
