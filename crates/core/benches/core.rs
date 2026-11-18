// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! A benchmark of the parts of Kudari that need no window, to see a change in milliseconds
//! without `KWin`, Qt or a graphics card:
//!
//!     cargo bench -p kudari-core --bench core [-- [throughput] [frame] [cases] [--runs N] [--csv]]
//!
//! `throughput` passes the loads of `tools/bench.sh` through the scanner of the marks and
//! through the parser of the emulator, each on its own, 64 KiB at a time as the PTY reads.
//! `frame` builds the paints of a frame with `render::frame` for screens of 315×93 cells:
//! text, cells of colors of their own, coloured bars, a selection over the screen and many
//! matches of a search. `cases` times smaller parts one by one: the words for the numbers of
//! links, a link and a color under the mouse, the pieces of box drawing, keys, a search
//! through a long history, the copy as HTML and the wrapping on a resize. Without a name,
//! all run. Each figure is the median of the runs (5 by default); the data comes from a fixed
//! seed, so every run gets the same. On Linux, each figure comes with the instructions it
//! took as well, which vary far less than times and show a change of a few percent; `--csv`
//! writes the figures for `tools/bench-compare.py`. The benchmark `allocations` counts the
//! allocations of the same frames and cases.

mod common;

use std::hint::black_box;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use common::{CHUNK, COLUMNS, LINES};
use kudari_core::colors;
use kudari_core::marks::{Marks, Scanner};
use kudari_core::options::Options;
use kudari_core::render::{self, Paint, Scene};

/// Frames to build for each figure of a scene.
const FRAMES: usize = 100;

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

/// How the benchmark runs: its runs, whether it writes CSV for a script rather than tables
/// for people, and the counter of instructions, if this system has one.
struct Settings {
    runs: usize,
    csv: bool,
    instructions: Option<common::Instructions>,
}

impl Settings {
    /// The instructions of `work`, run once more after the times, so that counting adds
    /// nothing to them; 0 without a counter.
    fn instructions(&self, work: &mut dyn FnMut()) -> u64 {
        self.instructions.as_ref().map_or(0, |counter| counter.count(work))
    }

    /// One figure as a line of CSV: the part, the name, what was measured and its value.
    fn record(&self, part: &str, name: &str, metric: &str, value: f64) {
        if self.csv {
            println!("{part},{name},{metric},{value}");
        }
    }

    fn heading(&self, text: &str) {
        if !self.csv {
            println!("{text}");
        }
    }
}

/// Millions of instructions, or a dash without a counter.
fn millions(instructions: u64) -> String {
    if instructions == 0 {
        "-".to_string()
    } else {
        format!("{:.1}", instructions as f64 / 1e6)
    }
}

fn throughput(settings: &Settings) {
    settings.heading(&format!(
        "throughput: median of {} runs, {} KiB at a time as the PTY reads",
        settings.runs,
        CHUNK >> 10
    ));
    settings.heading(&format!(
        "{:<8} {:>6}  {:>10} {:>8} {:>10}  {:>10} {:>8} {:>10}",
        "load", "MB", "scanner ms", "MB/s", "M instr", "parser ms", "MB/s", "M instr"
    ));
    let marks = Mutex::new(Marks::default());
    for (name, data) in common::loads() {
        let mut scanner_times = Vec::new();
        let mut parser_times = Vec::new();
        let scan = |bytes: &mut Vec<u8>| {
            let mut scanner = Scanner::default();
            for chunk in bytes.chunks_mut(CHUNK) {
                scanner.scan(chunk, &marks, &|| true);
            }
        };
        for _ in 0..settings.runs {
            // The scanner changes what it passes on, so each run gets the data afresh.
            let mut bytes = data.clone();
            let started = Instant::now();
            scan(&mut bytes);
            scanner_times.push(started.elapsed());
            black_box(&bytes);

            let mut term = common::terminal();
            let started = Instant::now();
            common::feed(&mut term, &data);
            parser_times.push(started.elapsed());
            black_box(term.grid().cursor.point);
        }
        let mut bytes = data.clone();
        let scanner_instructions = settings.instructions(&mut || scan(&mut bytes));
        let mut term = common::terminal();
        let parser_instructions = settings.instructions(&mut || common::feed(&mut term, &data));
        let (scanner, parser) = (median(scanner_times), median(parser_times));
        settings.record("throughput", name, "scanner_ms", milliseconds(scanner));
        settings.record("throughput", name, "scanner_instructions", scanner_instructions as f64);
        settings.record("throughput", name, "parser_ms", milliseconds(parser));
        settings.record("throughput", name, "parser_instructions", parser_instructions as f64);
        if !settings.csv {
            println!(
                "{name:<8} {:>6.1}  {:>10.2} {:>8.0} {:>10}  {:>10.2} {:>8.0} {:>10}",
                data.len() as f64 / 1e6,
                milliseconds(scanner),
                megabytes_per_second(data.len(), scanner),
                millions(scanner_instructions),
                milliseconds(parser),
                megabytes_per_second(data.len(), parser),
                millions(parser_instructions),
            );
        }
    }
}

fn frame(settings: &Settings) {
    settings.heading(&format!(
        "frame: median of {} runs of {FRAMES} frames each, {COLUMNS}×{LINES} cells",
        settings.runs
    ));
    settings.heading(&format!(
        "{:<10} {:>10} {:>12} {:>8} {:>8}",
        "screen", "µs/frame", "instr/frame", "paints", "matches"
    ));
    let scheme = colors::active();
    let options = Options::default();
    let mut paints: Vec<Paint> = Vec::new();
    for (name, term, matches) in common::scenes() {
        let scene = Scene {
            geometry: common::geometry(),
            scheme: &scheme,
            bold_intense: true,
            focused: true,
            cursor_hidden: false,
            cursor_fade: 1.0,
            cursor_apart: false,
            hints: &[],
            matches: &matches,
        };
        let mut build = || {
            for _ in 0..FRAMES {
                render::frame_into(&term, &options, &scene, &mut paints);
                black_box(&paints);
            }
        };
        let mut times = Vec::new();
        for _ in 0..settings.runs {
            let started = Instant::now();
            build();
            times.push(started.elapsed() / FRAMES as u32);
        }
        let instructions = settings.instructions(&mut build) / FRAMES as u64;
        let microseconds = median(times).as_secs_f64() * 1e6;
        settings.record("frame", name, "us", microseconds);
        settings.record("frame", name, "instructions", instructions as f64);
        if !settings.csv {
            println!(
                "{name:<10} {microseconds:>10.1} {:>12} {:>8} {:>8}",
                if instructions == 0 {
                    "-".to_string()
                } else {
                    instructions.to_string()
                },
                paints.len(),
                matches.len()
            );
        }
    }
}

/// Times `work`, called `calls` times in each run: the median time of one call, and the
/// instructions of one call.
fn time(settings: &Settings, name: &str, calls: usize, work: &mut dyn FnMut()) {
    let mut times = Vec::new();
    let mut run = || {
        for _ in 0..calls {
            work();
        }
    };
    for _ in 0..settings.runs {
        let started = Instant::now();
        run();
        times.push(started.elapsed() / calls as u32);
    }
    let instructions = settings.instructions(&mut run) / calls as u64;
    let microseconds = median(times).as_secs_f64() * 1e6;
    settings.record("cases", name, "us", microseconds);
    settings.record("cases", name, "instructions", instructions as f64);
    if !settings.csv {
        println!(
            "{name:<22} {microseconds:>12.2} {:>14}",
            if instructions == 0 {
                "-".to_string()
            } else {
                instructions.to_string()
            }
        );
    }
}

fn cases(settings: &Settings) {
    settings.heading(&format!(
        "cases: median of {} runs, {COLUMNS}×{LINES} cells",
        settings.runs
    ));
    settings.heading(&format!("{:<22} {:>12} {:>14}", "case", "µs/call", "instr/call"));
    common::cases(&mut |name, calls, work| time(settings, name, calls, work));
}

/// One part of the benchmark.
type Part = fn(&Settings);

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mut runs = 5;
    let mut csv = false;
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
            "--csv" => csv = true,
            // Cargo passes this to every benchmark.
            "--bench" => {}
            "throughput" | "frame" | "cases" => chosen.push(argument),
            other => {
                eprintln!("core: no part {other}; there are throughput, frame and cases, and --runs N and --csv");
                std::process::exit(2);
            }
        }
    }
    let settings = Settings {
        runs,
        csv,
        instructions: common::Instructions::new(),
    };
    if settings.instructions.is_none() && !csv {
        println!("(no count of instructions: perf_event_open is not there or not allowed)\n");
    }
    let parts: [(&str, Part); 3] = [("throughput", throughput), ("frame", frame), ("cases", cases)];
    let mut first = true;
    for (part, run) in parts {
        if chosen.is_empty() || chosen.iter().any(|chosen| chosen == part) {
            if !first && !csv {
                println!();
            }
            first = false;
            run(&settings);
        }
    }
}
