// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! A benchmark of the parts of Kudari that need no window, to see a change in milliseconds
//! without `KWin`, Qt or a graphics card:
//!
//!     cargo bench -p kudari-core --bench core [-- [throughput] [frame] [--runs N]]
//!
//! `throughput` passes the loads of `tools/bench.sh` through the scanner of the marks and
//! through the parser of the emulator, each on its own, 64 KiB at a time as the PTY reads.
//! `frame` builds the paints of a frame with `render::frame` for screens of 315×93 cells:
//! text, cells of colors of their own, coloured bars, a selection over the screen and many
//! matches of a search. Without a name, both run. Each figure is the median of the runs
//! (5 by default); the data comes from a fixed seed, so every run gets the same.

use std::fmt::Write as _;
use std::hint::black_box;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use kudari_core::colors;
use kudari_core::marks::{Marks, Scanner};
use kudari_core::options::Options;
use kudari_core::render::{self, Geometry, Paint, Scene};

const WORDS: [&str; 12] = [
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
const CELLS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789#@%&";
/// The screen of the measurements, as a full screen of 2560×1440 with the font of Kudari.
const COLUMNS: usize = 315;
const LINES: usize = 93;
/// What the PTY reads at a time.
const CHUNK: usize = 64 << 10;
/// Frames to build for each figure of a scene.
const FRAMES: usize = 100;

/// A small generator with a fixed seed: the same data in every run, with no crate for it.
struct Random(u64);

impl Random {
    fn next(&mut self, below: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % below as u64) as usize
    }

    fn word(&mut self) -> &'static str {
        WORDS.get(self.next(WORDS.len())).copied().unwrap_or_default()
    }

    fn cell(&mut self) -> char {
        CELLS.get(self.next(CELLS.len())).map_or(' ', |&byte| char::from(byte))
    }
}

/// The loads of `tools/bench.sh`, in its order: about 24 MB of plain text, 28 MB of text in 256
/// colors, 300 full screens with colors of their own in every cell, and the numbers up to
/// 2,000,000.
fn loads() -> Vec<(&'static str, Vec<u8>)> {
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
fn terminal() -> Term<VoidListener> {
    let config = Config {
        scrolling_history: 1000,
        ..Config::default()
    };
    Term::new(config, &TermSize::new(COLUMNS, LINES), VoidListener)
}

fn feed(term: &mut Term<VoidListener>, bytes: &[u8]) {
    let mut parser: Processor<StdSyncHandler> = Processor::new();
    for chunk in bytes.chunks(CHUNK) {
        parser.advance(term, chunk);
    }
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times.get(times.len() / 2).copied().unwrap_or_default()
}

fn milliseconds(time: Duration) -> f64 {
    time.as_secs_f64() * 1000.0
}

fn megabytes_per_second(bytes: usize, time: Duration) -> f64 {
    bytes as f64 / 1e6 / time.as_secs_f64().max(f64::MIN_POSITIVE)
}

fn throughput(runs: usize) {
    println!(
        "throughput: median of {runs} runs, {} KiB at a time as the PTY reads",
        CHUNK >> 10
    );
    println!(
        "{:<8} {:>6}  {:>10} {:>8}  {:>10} {:>8}",
        "load", "MB", "scanner ms", "MB/s", "parser ms", "MB/s"
    );
    let marks = Mutex::new(Marks::default());
    for (name, data) in loads() {
        let mut scanner_times = Vec::new();
        let mut parser_times = Vec::new();
        for _ in 0..runs {
            // The scanner changes what it passes on, so each run gets the data afresh.
            let mut bytes = data.clone();
            let mut scanner = Scanner::default();
            let started = Instant::now();
            for chunk in bytes.chunks_mut(CHUNK) {
                scanner.scan(chunk, &marks, &|| true);
            }
            scanner_times.push(started.elapsed());
            black_box(&bytes);

            let mut term = terminal();
            let started = Instant::now();
            feed(&mut term, &data);
            parser_times.push(started.elapsed());
            black_box(term.grid().cursor.point);
        }
        let (scanner, parser) = (median(scanner_times), median(parser_times));
        println!(
            "{name:<8} {:>6.1}  {:>10.2} {:>8.0}  {:>10.2} {:>8.0}",
            data.len() as f64 / 1e6,
            milliseconds(scanner),
            megabytes_per_second(data.len(), scanner),
            milliseconds(parser),
            megabytes_per_second(data.len(), parser),
        );
    }
}

/// The screens of the frames: what they show, and the matches of a search on them.
fn scenes() -> Vec<(&'static str, Term<VoidListener>, Vec<Match>)> {
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

fn frame(runs: usize) {
    println!("frame: median of {runs} runs of {FRAMES} frames each, {COLUMNS}×{LINES} cells");
    println!("{:<10} {:>10} {:>8} {:>8}", "screen", "µs/frame", "paints", "matches");
    let scheme = colors::active();
    let options = Options::default();
    let mut paints: Vec<Paint> = Vec::new();
    for (name, term, matches) in scenes() {
        let scene = Scene {
            geometry: Geometry {
                origin_x: 0.0,
                origin_y: 0.0,
                cell_width: 8.0,
                cell_height: 15.0,
                ascent: 12.0,
                pixel_ratio: 1.0,
            },
            scheme: &scheme,
            bold_intense: true,
            focused: true,
            cursor_hidden: false,
            cursor_fade: 1.0,
            hints: &[],
            matches: &matches,
        };
        let mut times = Vec::new();
        for _ in 0..runs {
            let started = Instant::now();
            for _ in 0..FRAMES {
                render::frame_into(&term, &options, &scene, &mut paints);
                black_box(&paints);
            }
            times.push(started.elapsed() / FRAMES as u32);
        }
        println!(
            "{name:<10} {:>10.1} {:>8} {:>8}",
            median(times).as_secs_f64() * 1e6,
            paints.len(),
            matches.len()
        );
    }
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mut runs = 5;
    let mut chosen = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--runs" => {
                runs = arguments
                    .next()
                    .and_then(|runs| runs.parse().ok())
                    .unwrap_or(runs)
                    .max(1);
            }
            // Cargo passes this to every benchmark.
            "--bench" => {}
            "throughput" | "frame" => chosen.push(argument),
            other => {
                eprintln!("core: no part {other}; there are throughput and frame, and --runs N");
                std::process::exit(2);
            }
        }
    }
    let all = chosen.is_empty();
    if all || chosen.iter().any(|part| part == "throughput") {
        throughput(runs);
    }
    if all || chosen.iter().any(|part| part == "frame") {
        if all {
            println!();
        }
        frame(runs);
    }
}
