// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! What the benchmarks `core` and `allocations` measure: the loads of `tools/bench.sh`, the
//! screens of the frames and the cases, from a fixed seed, so that both measure the same.
//! Each uses only part of it.
#![allow(dead_code, reason = "each benchmark uses a part of it")]

use std::fmt::Write as _;
use std::hint::black_box;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use kudari_core::input::{self, Key, Modifiers};
use kudari_core::linedraw;
use kudari_core::options::Options;
use kudari_core::render::Geometry;
use kudari_core::view::{self, Context, View};

pub const WORDS: [&str; 12] = [
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
];
pub const CELLS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789#@%&";
/// The screen of the measurements, as a full screen of 2560×1440 with the font of Kudari.
pub const COLUMNS: usize = 315;
pub const LINES: usize = 93;
/// What the PTY reads at a time.
pub const CHUNK: usize = 64 << 10;
/// A small generator with a fixed seed: the same data in every run, with no crate for it.
pub struct Random(u64);

impl Random {
    pub fn next(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % below as u64) as usize
    }

    pub fn word(&mut self) -> &'static str {
        WORDS.get(self.next(WORDS.len())).copied().unwrap_or_default()
    }

    pub fn cell(&mut self) -> char {
        CELLS.get(self.next(CELLS.len())).map_or(' ', |&byte| char::from(byte))
    }
}

/// The loads of `tools/bench.sh`, in its order: about 24 MB of plain text, 28 MB of text in 256
/// colors, 300 full screens with colors of their own in every cell, and the numbers up to
/// 2,000,000.
pub fn loads() -> Vec<(&'static str, Vec<u8>)> {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let mut plain = String::new();
    for number in 0..400_000 {
        let _ = write!(plain, "{number:7}");
        for _ in 0..8 {
            plain.push(' ');
            plain.push_str(random.word());
        }
        plain.push('\n');
    }
    let mut color = String::new();
    for _ in 0..200_000 {
        let words: Vec<String> = (0..8)
            .map(|_| format!("\x1b[38;5;{}m{}", random.next(256), random.word()))
            .collect();
        color.push_str(&words.join(" "));
        color.push_str("\x1b[0m\n");
    }
    let mut frames = String::new();
    for _ in 0..300 {
        frames.push_str("\x1b[H");
        for row in 0..LINES {
            for _ in 0..COLUMNS {
                let (foreground, background) = (random.next(256), random.next(256));
                let _ = write!(frames, "\x1b[38;5;{foreground};48;5;{background}m{}", random.cell());
            }
            if row + 1 < LINES {
                frames.push_str("\r\n");
            }
        }
        frames.push_str("\x1b[0m");
    }
    let mut seq = String::new();
    for number in 1..=2_000_000 {
        let _ = writeln!(seq, "{number}");
    }
    vec![
        ("plain", plain.into_bytes()),
        ("color", color.into_bytes()),
        ("frames", frames.into_bytes()),
        ("seq", seq.into_bytes()),
    ]
}

/// A terminal of the size of the measurements, with the default history of Kudari.
pub fn terminal() -> Term<VoidListener> {
    let config = Config {
        scrolling_history: 1000,
        ..Config::default()
    };
    Term::new(config, &TermSize::new(COLUMNS, LINES), VoidListener)
}

pub fn feed(term: &mut Term<VoidListener>, bytes: &[u8]) {
    let mut parser: Processor<StdSyncHandler> = Processor::new();
    for chunk in bytes.chunks(CHUNK) {
        parser.advance(term, chunk);
    }
}

/// Counts the instructions that this thread runs in user space, which vary far less from run
/// to run than times do: through `perf_event_open`, on Linux, if the kernel lets this process
/// count its own (`perf_event_paranoid` up to 2). Elsewhere, or when it may not, nothing.
pub struct Instructions {
    #[cfg(target_os = "linux")]
    file: std::fs::File,
}

#[cfg(target_os = "linux")]
/// The attributes of the kernel's first version, which every later one accepts.
#[repr(C)]
#[derive(Default)]
struct Attributes {
    kind: u32,
    size: u32,
    config: u64,
    sample_period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup_events: u32,
    breakpoint_type: u32,
    config1: u64,
}

#[cfg(target_os = "linux")]
impl Instructions {
    const HARDWARE: u32 = 0;
    const INSTRUCTIONS: u64 = 1;
    const DISABLED: u64 = 1;
    const EXCLUDE_KERNEL: u64 = 1 << 5;
    const EXCLUDE_HYPERVISOR: u64 = 1 << 6;
    const ENABLE: libc::c_ulong = 0x2400;
    const DISABLE: libc::c_ulong = 0x2401;
    const RESET: libc::c_ulong = 0x2403;

    pub fn new() -> Option<Self> {
        use std::os::fd::FromRawFd;
        let attributes = Attributes {
            kind: Self::HARDWARE,
            size: size_of::<Attributes>() as u32,
            config: Self::INSTRUCTIONS,
            flags: Self::DISABLED | Self::EXCLUDE_KERNEL | Self::EXCLUDE_HYPERVISOR,
            ..Attributes::default()
        };
        // SAFETY: the attributes live through the call and say their own size; the thread is
        // this one (0) on any processor (-1), with no group and no flags.
        let descriptor = unsafe { libc::syscall(libc::SYS_perf_event_open, &raw const attributes, 0, -1, -1, 0) };
        let descriptor = i32::try_from(descriptor).ok().filter(|&descriptor| descriptor >= 0)?;
        // SAFETY: the kernel just opened the descriptor, and nothing else owns it.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        Some(Self { file })
    }

    fn control(&self, request: libc::c_ulong) {
        use std::os::fd::AsRawFd;
        // SAFETY: a request without an argument, on the descriptor of this counter.
        unsafe { libc::ioctl(self.file.as_raw_fd(), request, 0) };
    }

    /// The instructions that `work` ran.
    pub fn count(&self, work: &mut dyn FnMut()) -> u64 {
        use std::io::Read;
        self.control(Self::RESET);
        self.control(Self::ENABLE);
        work();
        self.control(Self::DISABLE);
        let mut bytes = [0; 8];
        (&self.file)
            .read_exact(&mut bytes)
            .map_or(0, |()| u64::from_ne_bytes(bytes))
    }
}

#[cfg(not(target_os = "linux"))]
impl Instructions {
    pub fn new() -> Option<Self> {
        None
    }

    #[allow(clippy::unused_self, reason = "the same as on Linux, where it counts")]
    pub fn count(&self, work: &mut dyn FnMut()) -> u64 {
        work();
        0
    }
}

/// The cells of the frames: 8 by 15 pixels, at a scale of 1.
pub fn geometry() -> Geometry {
    Geometry {
        origin_x: 0.0,
        origin_y: 0.0,
        cell_width: 8.0,
        cell_height: 15.0,
        ascent: 12.0,
        pixel_ratio: 1.0,
    }
}

/// The screens of the frames: what they show, and the matches of a search on them.
pub fn scenes() -> Vec<(&'static str, Term<VoidListener>, Vec<Match>)> {
    let data = loads();
    let load = |name: &str| {
        data.iter()
            .find(|(load, _)| *load == name)
            .map(|(_, bytes)| bytes.as_slice())
    };
    let tail = |bytes: &[u8]| {
        bytes
            .get(bytes.len().saturating_sub(64 << 10)..)
            .unwrap_or_default()
            .to_vec()
    };

    let mut text = terminal();
    feed(&mut text, &tail(load("plain").unwrap_or_default()));

    let mut colors = terminal();
    let frames = load("frames").unwrap_or_default();
    // The last of the full screens.
    let screen = frames.len() / 300;
    feed(
        &mut colors,
        frames.get(frames.len().saturating_sub(screen)..).unwrap_or_default(),
    );

    // Coloured bars, as a file manager draws them: a blue background with a status line.
    let mut bars = terminal();
    let mut screen = String::from("\x1b[44m\x1b[2J\x1b[H");
    for row in 0..LINES - 1 {
        let _ = write!(screen, "\x1b[44m  {row:>4}  some name of a file\x1b[K\r\n");
    }
    screen.push_str("\x1b[46m\x1b[30m 1Help  2Menu  3View  4Edit  5Copy  6Move\x1b[K\x1b[0m");
    feed(&mut bars, screen.as_bytes());

    let mut selected = terminal();
    feed(&mut selected, &tail(load("plain").unwrap_or_default()));
    let last = Point::new(Line(LINES as i32 - 1), Column(COLUMNS - 1));
    let mut selection = Selection::new(SelectionType::Simple, Point::new(Line(0), Column(0)), Side::Left);
    selection.update(last, Side::Right);
    selected.selection = Some(selection);

    let mut searched = terminal();
    feed(&mut searched, &tail(load("plain").unwrap_or_default()));
    let matches = RegexSearch::new("e").map_or_else(
        |_| Vec::new(),
        |mut regex| {
            RegexIter::new(
                Point::new(Line(0), Column(0)),
                Point::new(Line(searched.screen_lines() as i32 - 1), searched.last_column()),
                Direction::Right,
                &searched,
                &mut regex,
            )
            .collect()
        },
    );

    vec![
        ("text", text, Vec::new()),
        ("colors", colors, Vec::new()),
        ("bars", bars, Vec::new()),
        ("selection", selected, Vec::new()),
        ("matches", searched, matches),
    ]
}

/// A terminal with `history` lines of history, all of them full of text.
pub fn with_history(history: usize) -> Term<VoidListener> {
    let config = Config {
        scrolling_history: history,
        ..Config::default()
    };
    let mut term = Term::new(config, &TermSize::new(COLUMNS, LINES), VoidListener);
    let mut random = Random(7);
    let mut text = String::new();
    for number in 0..history + LINES {
        let _ = write!(text, "{number:7}");
        for _ in 0..12 {
            text.push(' ');
            text.push_str(random.word());
        }
        text.push_str("\r\n");
    }
    feed(&mut term, text.as_bytes());
    term
}

/// What gets each case: its name, the calls that make a figure, and the work of one call.
pub type Each<'a> = dyn FnMut(&str, usize, &mut dyn FnMut()) + 'a;

/// The cases, one after the other.
pub fn cases(each: &mut Each<'_>) {
    let data = loads();
    let plain: &[u8] = data
        .iter()
        .find(|(load, _)| *load == "plain")
        .map_or(&[], |(_, bytes)| bytes.as_slice());
    let mut text = terminal();
    // A screen of text with addresses and paths in it, as the numbers of links look for.
    let mut screen = String::new();
    for row in 0..LINES {
        let _ = write!(
            screen,
            "{row:4} see https://example.org/page/{row} and ./src/file{row}.rs:12 or #a0b0c0 {}\r\n",
            String::from_utf8_lossy(plain.get(row * 80..row * 80 + 80).unwrap_or_default())
        );
    }
    feed(&mut text, screen.as_bytes());
    let options = Options::default();
    let nowhere = || None;
    let context = Context {
        options: &options,
        tracks_mouse: false,
        directory: &nowhere,
    };
    let middle = Point::new(Line(LINES as i32 / 2), Column(12));

    each("hint words", 100, &mut || {
        black_box(view::hint_words(&text));
    });
    each("link at", 10_000, &mut || {
        black_box(view::link_at(&text, middle, false, &context));
    });
    each("color word at", 10_000, &mut || {
        black_box(view::color_word_at(&text, Point::new(Line(10), Column(60))));
    });
    each("line pieces", 10_000, &mut || {
        for character in ['─', '│', '┼', '╬', '╭', '░', '▒', '⠿'] {
            black_box(linedraw::pieces(character, 8.0, 15.0, true, true));
        }
    });
    each("keys", 100_000, &mut || {
        black_box(input::key_bytes(
            TermMode::empty(),
            Key::Character('a'),
            Modifiers::NONE,
            "a",
        ));
        black_box(input::key_bytes(TermMode::APP_CURSOR, Key::Up, Modifiers::CONTROL, ""));
    });
    for history in [10_000, 100_000] {
        let mut term = with_history(history);
        let mut search = View::default();
        // A pattern that is nowhere, so that the search goes through all of the history.
        each(&format!("search {}k lines", history / 1000), 3, &mut || {
            term.selection = None;
            black_box(search.find(&mut term, "nowhere", true, false, false, false));
        });
        // A key typed into the search: only the screen and 1000 lines above it.
        each(
            &format!("search while typing {}k lines", history / 1000),
            3,
            &mut || {
                term.selection = None;
                black_box(search.find_near(&mut term, "nowhere", false, false, false, LINES + 1000));
            },
        );
    }
    let mut selected = terminal();
    feed(&mut selected, screen.as_bytes());
    view::select_all(&mut selected);
    each("copy as HTML", 20, &mut || {
        black_box(view::selection_html(&selected, true));
    });
    let mut resized = with_history(10_000);
    each("resize 10k lines", 2, &mut || {
        resized.resize(TermSize::new(COLUMNS / 2, LINES));
        resized.resize(TermSize::new(COLUMNS, LINES));
    });
}
