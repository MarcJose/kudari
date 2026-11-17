// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! A benchmark of the parts of Kudari that need no window, to see a change in milliseconds
//! without `KWin`, Qt or a graphics card:
//!
//!     cargo bench -p kudari-core --bench core [-- [throughput] [frame] [cases] [--runs N]]
//!
//! `throughput` passes the loads of `tools/bench.sh` through the scanner of the marks and
//! through the parser of the emulator, each on its own, 64 KiB at a time as the PTY reads.
//! `frame` builds the paints of a frame with `render::frame` for screens of 315×93 cells:
//! text, cells of colors of their own, coloured bars, a selection over the screen and many
//! matches of a search. `cases` times smaller parts one by one: the words for the numbers of
//! links, a link and a color under the mouse, the pieces of box drawing, keys, a search
//! through a long history, the copy as HTML and the wrapping on a resize. Without a name,
//! all run. Each figure is the median of the runs (5 by default); the data comes from a fixed
//! seed, so every run gets the same. The benchmark `allocations` counts the allocations of the
//! same frames and cases; this one has no allocator of its own, as one slows down the frames
//! by about two fifths and would make its times wrong.

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
    for (name, data) in common::loads() {
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

            let mut term = common::terminal();
            let started = Instant::now();
            common::feed(&mut term, &data);
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

fn frame(runs: usize) {
    println!("frame: median of {runs} runs of {FRAMES} frames each, {COLUMNS}×{LINES} cells");
    println!("{:<10} {:>10} {:>8} {:>8}", "screen", "µs/frame", "paints", "matches");
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

/// Times `work`, called `calls` times in each run: the median time of one call.
fn time(name: &str, runs: usize, calls: usize, work: &mut dyn FnMut()) {
    let mut times = Vec::new();
    for _ in 0..runs {
        let started = Instant::now();
        for _ in 0..calls {
            work();
        }
        times.push(started.elapsed() / calls as u32);
    }
    println!("{name:<22} {:>12.2}", median(times).as_secs_f64() * 1e6);
}

fn cases(runs: usize) {
    println!("cases: median of {runs} runs, {COLUMNS}×{LINES} cells");
    println!("{:<22} {:>12}", "case", "µs/call");
    common::cases(&mut |name, calls, work| time(name, runs, calls, work));
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
            "throughput" | "frame" | "cases" => chosen.push(argument),
            other => {
                eprintln!("core: no part {other}; there are throughput, frame and cases, and --runs N");
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
    if all || chosen.iter().any(|part| part == "cases") {
        if all {
            println!();
        }
        cases(runs);
    }
}
