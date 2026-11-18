// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Random output mixed with random steps of the user, and after each step everything that the
//! user interface reads from the terminal. Output goes in pieces through the scanner of the
//! marks and the parser, as from the PTY; the steps resize the grid, scroll, enter and leave
//! the selection mode, select all, change the history and clear the screen. Each case runs on
//! a thread of its own with a limit of time, so that a hang or a panic fails with its seed.
//!
//! The steps leave out moves of the cursor of the selection mode: with wide characters they
//! reach two faults of the emulator that are still open, which `upstream_tests` keeps.
//!
//! `cargo test` runs a few cases with fixed seeds. More run with the following, and
//! `KUDARI_RANDOM_TRACE=1` shows the steps of a case that fails:
//!
//!     KUDARI_RANDOM_CASES=1000 KUDARI_RANDOM_SEED=7 cargo test -p kudari-core --lib random -- --ignored

use std::sync::{Mutex, mpsc};
use std::time::Duration;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, MIN_COLUMNS, Term};
use alacritty_terminal::vte::ansi::{ClearMode, Handler, Processor, StdSyncHandler};

use crate::colors::ColorScheme;
use crate::fuzz::Random;
use crate::marks::{Marks, Scanner};
use crate::options::Options;
use crate::render::{self, Geometry, Scene};
use crate::view::{self, View};

/// The final bytes of control sequences that move, erase, insert, scroll and set modes.
const FINALS: &[u8] = b"ABCDEFGHJKLMPSTXZ@`abdefghlmnrsu";
/// Text of a few kinds: plain, wide, with combining marks, emoji, box drawing, and tabs.
const TEXTS: &[&str] = &[
    "hello world ",
    "日本語のテキスト",
    "e\u{301}a\u{308}o\u{302}",
    "🙂👍🏽",
    "─│┼╭╮╯╰",
    "\t\tx\t",
    "https://example.org/a?b=c ",
    "/usr/lib/file.rs:12:3 ",
    "#ff8800 rgb(1,2,3) ",
    "\r\n",
    "\r",
    "\x08",
];

/// A piece of random output: text, a control sequence, a mode, a string sequence or a reset.
fn output(random: &mut Random) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..=random.below(12) {
        match random.below(10) {
            0..=3 => out.extend_from_slice(TEXTS[random.below(TEXTS.len())].as_bytes()),
            4 | 5 => {
                // Parameters as programs send them, and some far too large.
                let parameter = |random: &mut Random| match random.below(4) {
                    0 => String::new(),
                    1 => random.below(5).to_string(),
                    2 => random.below(300).to_string(),
                    _ => (random.below(70_000)).to_string(),
                };
                let first = parameter(random);
                let second = parameter(random);
                let mut last = FINALS[random.below(FINALS.len())];
                // A large repeat freezes the emulator for minutes; see `upstream_tests`.
                if last == b'b' {
                    last = b'@';
                }
                out.extend_from_slice(format!("\x1b[{first};{second}").as_bytes());
                out.push(last);
            }
            6 => {
                let modes = [
                    "?7", "?6", "?25", "?47", "?1047", "?1049", "?1000", "?1006", "?2004", "4", "20",
                ];
                let mode = modes[random.below(modes.len())];
                let set = if random.below(2) == 0 { 'h' } else { 'l' };
                out.extend_from_slice(format!("\x1b[{mode}{set}").as_bytes());
            }
            7 => {
                let codes = [
                    "0;title",
                    "2;",
                    "7;file:///tmp",
                    "8;;https://example.org",
                    "8;;",
                    "52;c;aGk=",
                    "133;A",
                    "133;D;0",
                    "4;1;?",
                ];
                let code = codes[random.below(codes.len())];
                let end = if random.below(2) == 0 { "\x07" } else { "\x1b\\" };
                out.extend_from_slice(format!("\x1b]{code}{end}").as_bytes());
            }
            8 => {
                let others: [&[u8]; 8] = [
                    b"\x1b7",
                    b"\x1b8",
                    b"\x1b#8",
                    b"\x1bD",
                    b"\x1bM",
                    b"\x1bE",
                    b"\x1bP+q544e\x1b\\",
                    b"\x1b(0lqk\x1b(B",
                ];
                out.extend_from_slice(others[random.below(others.len())]);
            }
            _ => {
                // A scrolling region, or a reset of everything.
                if random.below(8) == 0 {
                    out.extend_from_slice(b"\x1bc");
                } else {
                    out.extend_from_slice(format!("\x1b[{};{}r", 1 + random.below(10), random.below(40)).as_bytes());
                }
            }
        }
    }
    out
}

/// Everything the user interface reads, as it reads it: a frame, the selection as text and as
/// HTML, the links on the screen, and a link and a color under a point.
fn read_all(term: &Term<VoidListener>, view: &mut View, random: &mut Random) {
    let scheme = ColorScheme::default();
    let scene = Scene {
        geometry: Geometry {
            origin_x: 0.0,
            origin_y: 0.0,
            cell_width: 8.0,
            cell_height: 16.0,
            ascent: 12.0,
            pixel_ratio: 1.0,
        },
        scheme: &scheme,
        bold_intense: true,
        focused: random.below(2) == 0,
        cursor_hidden: false,
        cursor_fade: 1.0,
        cursor_apart: random.below(2) == 0,
        hints: view.hints(),
        matches: view.matches(),
    };
    let options = Options::default();
    std::hint::black_box(render::frame(term, &options, &scene));
    let mut paints = Vec::new();
    std::hint::black_box(render::cursor_into(term, &options, &scene, &mut paints));
    std::hint::black_box(view::selected_text(term, &options));
    std::hint::black_box(view::selection_html(term, true));
    std::hint::black_box(view::links_on_screen(term, None));
    let point = Point::new(
        Line(random.below(term.screen_lines() + 2) as i32 - 1),
        Column(random.below(term.columns() + 2)),
    );
    let context = view::Context {
        options: &options,
        tracks_mouse: false,
        directory: &|| None,
    };
    std::hint::black_box(view::link_at(term, point, false, &context));
    std::hint::black_box(view::color_word_at(term, point));
    view.update_matches(term);
}

/// One case: `steps` random steps from `seed`. With `KUDARI_RANDOM_TRACE=1`, each step goes
/// to stderr, to find the one that fails.
fn case(seed: u64, steps: usize) {
    let trace = std::env::var_os("KUDARI_RANDOM_TRACE").is_some_and(|value| value == "1");
    let mut random = Random::new(seed);
    let mut term = Term::new(Config::default(), &TermSize::new(40, 10), VoidListener);
    let mut processor = Processor::<StdSyncHandler>::new();
    let mut scanner = Scanner::default();
    let marks = Mutex::new(Marks::default());
    let mut view = View::default();
    for step in 0..steps {
        match random.below(20) {
            0..=10 => {
                let mut bytes = output(&mut random);
                if trace {
                    eprintln!("{seed} {step}: output {:?}", String::from_utf8_lossy(&bytes));
                }
                // In pieces, as the PTY reads them, so that sequences are cut.
                for piece in bytes.chunks_mut(1 + random.below(16)) {
                    scanner.scan(piece, &marks, &|| true);
                    processor.advance(&mut term, piece);
                }
            }
            11 | 12 => {
                let columns = MIN_COLUMNS + random.below(120);
                let lines = 1 + random.below(40);
                if trace {
                    eprintln!("{seed} {step}: resize to {columns}x{lines}");
                }
                term.resize(TermSize::new(columns, lines));
            }
            13 | 14 => {
                let scroll = match random.below(4) {
                    0 => Scroll::Top,
                    1 => Scroll::Bottom,
                    _ => Scroll::Delta(random.below(80) as i32 - 40),
                };
                if trace {
                    eprintln!("{seed} {step}: scroll {scroll:?}");
                }
                term.scroll_display(scroll);
            }
            15 => {
                if trace {
                    eprintln!("{seed} {step}: selection mode");
                }
                std::hint::black_box(view::toggle_selection_mode(&mut term));
            }
            16 => {
                if trace {
                    eprintln!("{seed} {step}: select all or nothing");
                }
                if random.below(2) == 0 {
                    view::select_all(&mut term);
                } else {
                    term.selection = None;
                }
            }
            17 => {
                let config = Config {
                    scrolling_history: random.below(300),
                    ..Config::default()
                };
                if trace {
                    eprintln!("{seed} {step}: history {}", config.scrolling_history);
                }
                term.set_options(config);
            }
            18 => {
                if trace {
                    eprintln!("{seed} {step}: clear the screen");
                }
                term.clear_screen(match random.below(4) {
                    0 => ClearMode::All,
                    1 => ClearMode::Saved,
                    2 => ClearMode::Above,
                    _ => ClearMode::Below,
                });
            }
            _ => {
                let patterns = ["o", "[a-z]+", "日", "x{2,}", "(", ""];
                let pattern = patterns[random.below(patterns.len())];
                if trace {
                    eprintln!("{seed} {step}: find {pattern:?}");
                }
                std::hint::black_box(view.find(&mut term, pattern, random.below(2) == 0, false, true, true));
            }
        }
        // As each lock of a terminal in Kudari does.
        view::repair_view(&mut term);
        read_all(&term, &mut view, &mut random);
    }
}

/// Runs `cases` cases from `first`, each on a thread of its own, and gives the seeds that
/// failed, with why.
fn run(first: u64, cases: u64, steps: usize, limit: Duration) -> Vec<String> {
    let mut failed = Vec::new();
    for seed in first..first + cases {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| case(seed, steps));
            let _ = sender.send(result.is_ok());
        });
        match receiver.recv_timeout(limit) {
            Ok(true) => {}
            Ok(false) => failed.push(format!("seed {seed}: it panicked")),
            Err(_) => failed.push(format!("seed {seed}: it took longer than {limit:?}")),
        }
    }
    failed
}

#[test]
fn random_output_and_steps_of_the_user_do_no_harm() {
    let failed = run(1, 24, 120, Duration::from_secs(10));
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
#[ignore = "many cases; KUDARI_RANDOM_CASES and KUDARI_RANDOM_SEED choose them"]
fn many_random_cases_do_no_harm() {
    let number = |name: &str, fallback: u64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(fallback)
    };
    let failed = run(
        number("KUDARI_RANDOM_SEED", 1000),
        number("KUDARI_RANDOM_CASES", 500),
        300,
        Duration::from_secs(20),
    );
    assert!(failed.is_empty(), "{failed:#?}");
}
