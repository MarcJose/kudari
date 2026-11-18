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
//! `KUDARI_RANDOM_TRACE=1` writes the steps of a case to stderr, a line each, which
//! `KUDARI_RANDOM_REPLAY=file` runs again, also with lines left out:
//!
//!     KUDARI_RANDOM_CASES=1000 KUDARI_RANDOM_SEED=7 cargo test -p kudari-core --lib random -- --ignored

use std::fmt::Write as _;
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

/// A step of a case. Its line, from [`Step::line`], reads back with [`Step::parse`], so that a
/// case written with `KUDARI_RANDOM_TRACE=1` can run again with `KUDARI_RANDOM_REPLAY`, with
/// steps left out to find the few that fail.
/// Where a step scrolls the view to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Top,
    Bottom,
    By(i32),
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// Output, in pieces of this many bytes.
    Output(usize, Vec<u8>),
    Resize(usize, usize),
    Scroll(Place),
    SelectionMode,
    SelectAll(bool),
    History(usize),
    Clear(usize),
    Find(bool, String),
}

impl Step {
    fn random(random: &mut Random) -> Self {
        match random.below(20) {
            0..=10 => {
                let bytes = output(random);
                Self::Output(1 + random.below(16), bytes)
            }
            11 | 12 => Self::Resize(MIN_COLUMNS + random.below(120), 1 + random.below(40)),
            13 | 14 => Self::Scroll(match random.below(4) {
                0 => Place::Top,
                1 => Place::Bottom,
                _ => Place::By(random.below(80) as i32 - 40),
            }),
            15 => Self::SelectionMode,
            16 => Self::SelectAll(random.below(2) == 0),
            17 => Self::History(random.below(300)),
            18 => Self::Clear(random.below(4)),
            _ => {
                let patterns = ["o", "[a-z]+", "日", "x{2,}", "(", ""];
                let pattern = patterns[random.below(patterns.len())];
                Self::Find(random.below(2) == 0, pattern.to_string())
            }
        }
    }

    fn line(&self) -> String {
        let hex = |bytes: &[u8]| {
            bytes.iter().fold(String::new(), |mut text, byte| {
                let _ = write!(text, "{byte:02x}");
                text
            })
        };
        match self {
            Self::Output(piece, bytes) => format!("output {piece} {}", hex(bytes)),
            Self::Resize(columns, lines) => format!("resize {columns} {lines}"),
            Self::Scroll(Place::Top) => "scroll top".to_string(),
            Self::Scroll(Place::Bottom) => "scroll bottom".to_string(),
            Self::Scroll(Place::By(lines)) => format!("scroll {lines}"),
            Self::SelectionMode => "selection-mode".to_string(),
            Self::SelectAll(all) => format!("select {}", if *all { "all" } else { "none" }),
            Self::History(lines) => format!("history {lines}"),
            Self::Clear(mode) => format!("clear {mode}"),
            Self::Find(backwards, pattern) => format!("find {} {}", u8::from(*backwards), hex(pattern.as_bytes())),
        }
    }

    fn parse(line: &str) -> Option<Self> {
        let unhex = |text: &str| -> Option<Vec<u8>> {
            (0..text.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
                .collect()
        };
        let mut words = line.split_whitespace();
        let number = |word: Option<&str>| word.and_then(|word| word.parse::<usize>().ok());
        Some(match words.next()? {
            "output" => Self::Output(number(words.next())?, unhex(words.next().unwrap_or(""))?),
            "resize" => Self::Resize(number(words.next())?, number(words.next())?),
            "scroll" => Self::Scroll(match words.next()? {
                "top" => Place::Top,
                "bottom" => Place::Bottom,
                lines => Place::By(lines.parse().ok()?),
            }),
            "selection-mode" => Self::SelectionMode,
            "select" => Self::SelectAll(words.next()? == "all"),
            "history" => Self::History(number(words.next())?),
            "clear" => Self::Clear(number(words.next())?),
            "find" => Self::Find(
                words.next()? == "1",
                String::from_utf8(unhex(words.next().unwrap_or(""))?).ok()?,
            ),
            _ => return None,
        })
    }
}

/// A terminal and what goes with it in Kudari, which the steps act on.
struct State {
    term: Term<VoidListener>,
    processor: Processor<StdSyncHandler>,
    scanner: Scanner,
    marks: Mutex<Marks>,
    view: View,
}

impl State {
    fn new() -> Self {
        Self {
            term: Term::new(Config::default(), &TermSize::new(40, 10), VoidListener),
            processor: Processor::new(),
            scanner: Scanner::default(),
            marks: Mutex::new(Marks::default()),
            view: View::default(),
        }
    }

    /// Takes `step`, repairs the view as each lock of a terminal in Kudari does, and reads
    /// everything, with `reading` choosing where.
    fn take(&mut self, step: &Step, reading: u64) {
        let term = &mut self.term;
        match step {
            Step::Output(piece, bytes) => {
                let mut bytes = bytes.clone();
                // In pieces, as the PTY reads them, so that sequences are cut.
                for part in bytes.chunks_mut((*piece).max(1)) {
                    self.scanner.scan(part, &self.marks, &|| true);
                    self.processor.advance(term, part);
                }
            }
            Step::Resize(columns, lines) => term.resize(TermSize::new(*columns, *lines)),
            Step::Scroll(place) => term.scroll_display(match place {
                Place::Top => Scroll::Top,
                Place::Bottom => Scroll::Bottom,
                Place::By(lines) => Scroll::Delta(*lines),
            }),
            Step::SelectionMode => {
                std::hint::black_box(view::toggle_selection_mode(term));
            }
            Step::SelectAll(true) => view::select_all(term),
            Step::SelectAll(false) => term.selection = None,
            Step::History(lines) => term.set_options(Config {
                scrolling_history: *lines,
                ..Config::default()
            }),
            Step::Clear(mode) => term.clear_screen(match mode {
                0 => ClearMode::All,
                1 => ClearMode::Saved,
                2 => ClearMode::Above,
                _ => ClearMode::Below,
            }),
            Step::Find(backwards, pattern) => {
                std::hint::black_box(self.view.find(term, pattern, *backwards, false, true, true));
            }
        }
        view::repair_view(term);
        read_all(term, &mut self.view, &mut Random::new(reading));
    }
}

/// One case: `steps` random steps from `seed`. With `KUDARI_RANDOM_TRACE=1`, each step goes
/// to stderr as a line that `KUDARI_RANDOM_REPLAY` reads.
fn case(seed: u64, steps: usize) {
    let trace = std::env::var_os("KUDARI_RANDOM_TRACE").is_some_and(|value| value == "1");
    let mut random = Random::new(seed);
    let mut state = State::new();
    for number in 0..steps {
        let step = Step::random(&mut random);
        if trace {
            eprintln!("{}", step.line());
        }
        state.take(&step, number as u64);
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
#[ignore = "the steps of KUDARI_RANDOM_REPLAY, one a line, as KUDARI_RANDOM_TRACE writes them"]
fn a_case_runs_again_from_its_steps() {
    let Some(path) = std::env::var_os("KUDARI_RANDOM_REPLAY") else {
        return;
    };
    let steps: Vec<Step> = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(Step::parse)
        .collect();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut state = State::new();
        for (number, step) in steps.iter().enumerate() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| state.take(step, number as u64)));
            if result.is_err() {
                let _ = sender.send(format!("step {number} panicked: {}", step.line()));
                return;
            }
        }
        let _ = sender.send(String::new());
    });
    let failed = receiver
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| "it took longer than 10 s".to_string());
    assert!(failed.is_empty(), "{failed}");
}

#[test]
fn a_step_reads_back_from_its_line() {
    let mut random = Random::new(3);
    for _ in 0..500 {
        let step = Step::random(&mut random);
        assert_eq!(Step::parse(&step.line()), Some(step));
    }
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
