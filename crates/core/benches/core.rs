// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! A benchmark of the parts of Kudari that need no window, to see a change in milliseconds
//! without `KWin`, Qt or a graphics card:
//!
//!     cargo bench -p kudari-core --bench core [-- [throughput] [frame] [cases] [pty] [--runs N] [--csv]]
//!
//! `throughput` passes the loads of `tools/bench.sh` through the scanner of the marks and
//! through the parser of the emulator, each on its own, 64 KiB at a time as the PTY reads.
//! `frame` builds the paints of a frame with `render::frame` for screens of 315×93 cells:
//! text, cells of colors of their own, coloured bars, a selection over the screen and many
//! matches of a search. `cases` times smaller parts one by one: the words for the numbers of
//! links, a link and a color under the mouse, the pieces of box drawing, keys, a search
//! through a long history, the copy as HTML and the wrapping on a resize. Without a name,
//! these run. `pty`, only when named and only on Unix, runs a real session with its PTY: a
//! program in it writes each load and asks for the place of the cursor, and the time to the
//! answer counts, as in `tools/bench.sh`; a thread holds the lock of the terminal 0, 1, 5 or
//! 10 ms of every 16.7 ms meanwhile, as drawing does. Each figure is the median of the runs (5 by default); the data comes from a fixed
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
            "throughput" | "frame" | "cases" | "pty" => chosen.push(argument),
            other => {
                eprintln!("core: no part {other}; there are throughput, frame, cases and pty, and --runs N and --csv");
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
    let parts: [(&str, Part); 4] = [
        ("throughput", throughput),
        ("frame", frame),
        ("cases", cases),
        ("pty", pty::run),
    ];
    let mut first = true;
    for (part, run) in parts {
        // A session with its PTY takes a minute or more: only when named.
        let named = chosen.iter().any(|chosen| chosen == part);
        if named || (chosen.is_empty() && part != "pty") {
            if !first && !csv {
                println!();
            }
            first = false;
            run(&settings);
        }
    }
}

/// A real session with its PTY, as Kudari runs it, with a front end of its own.
#[cfg(unix)]
mod pty {
    use std::os::unix::fs::DirBuilderExt as _;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{Sender, channel};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use kudari_core::options::Options;
    use kudari_core::services::{Clipboard, Selection};
    use kudari_core::terminal::{self, Listener, Report};

    use super::{Settings, common, median};

    /// The milliseconds of each frame that a thread holds the lock of the terminal, as
    /// drawing it does.
    const HELD: [u64; 4] = [0, 1, 5, 10];
    /// A frame at 60 Hz.
    const FRAME: Duration = Duration::from_micros(16_667);

    /// Writes the load, asks for the place of the cursor and notes the seconds from the first
    /// byte to the answer and to the last byte, as the driver of `tools/bench.sh` does.
    const PROGRAM: &str = r#"
import os, re, select, sys, termios, time
load, result = sys.argv[1], sys.argv[2]
mode = termios.tcgetattr(0)
mode[3] &= ~(termios.ICANON | termios.ECHO)
termios.tcsetattr(0, termios.TCSANOW, mode)
time.sleep(0.3)
start = time.monotonic()
with open(load, "rb") as source:
    while block := source.read(131072):
        view = memoryview(block)
        while view:
            view = view[os.write(1, view):]
written = time.monotonic()
termios.tcflush(0, termios.TCIFLUSH)
os.write(1, b"\x1b[6n")
got, deadline = b"", time.monotonic() + 60
while not re.search(rb"\x1b\[\d+;\d+R", got) and select.select([0], [], [], max(0, deadline - time.monotonic()))[0]:
    got += os.read(0, 64)
end = time.monotonic()
with open(result + ".part", "w") as file:
    file.write(f"{end - start:.6f} {written - start:.6f}\n")
os.rename(result + ".part", result)
time.sleep(0.5)
"#;

    /// Work for the thread of the front end, as the thread of Qt does it.
    enum Work {
        Run(Box<dyn FnOnce() + Send>),
        /// A report of output, which waits for the next frame as in Kudari.
        Hold(terminal::Pending),
    }

    struct Front(Mutex<Sender<Work>>);

    impl Listener for Front {
        fn run(&self, work: Box<dyn FnOnce() + Send>) {
            let _ = self.0.lock().map(|sender| sender.send(Work::Run(work)));
        }

        fn report(&self, _terminal: i32, report: Report) {
            if let Report::Output(pending) = report {
                let _ = self.0.lock().map(|sender| sender.send(Work::Hold(pending)));
            }
        }

        fn clipboard(&self) -> &dyn Clipboard {
            self
        }
    }

    impl Clipboard for Front {
        fn copy(&self, _text: &str, _target: Selection) {}

        fn copy_rich(&self, _text: &str, _html: &str) {}

        fn paste(&self, _source: Selection) -> String {
            String::new()
        }
    }

    /// The front end, with its thread: work at once, reports of output let go at each frame.
    fn front() {
        static FRONT: OnceLock<Front> = OnceLock::new();
        let (sender, receiver) = channel();
        if FRONT.set(Front(Mutex::new(sender))).is_err() {
            return;
        }
        if let Some(front) = FRONT.get() {
            terminal::set_listener(front);
        }
        std::thread::spawn(move || {
            let mut held = Vec::new();
            let mut frame = Instant::now() + FRAME;
            loop {
                match receiver.recv_timeout(frame.saturating_duration_since(Instant::now())) {
                    Ok(Work::Run(work)) => work(),
                    Ok(Work::Hold(pending)) => held.push(pending),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        held.clear();
                        frame = Instant::now() + FRAME;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
    }

    /// One load in a new session, with the lock held `held` ms of every frame: the seconds to
    /// the answer, and the CPU time of this process meanwhile, which holds the PTY thread.
    fn session(id: i32, program: &Path, load: &Path, held: u64) -> Option<(f64, f64)> {
        let result = load.with_extension(format!("{id}.result"));
        let _ = std::fs::remove_file(&result);
        // Each path in quotes of its own, as a temporary directory may hold spaces.
        let quoted = |path: &Path| format!("'{}'", path.display().to_string().replace('\'', r"'\''"));
        terminal::apply_options(Options {
            command: format!("python3 {} {} {}", quoted(program), quoted(load), quoted(&result)),
            shell_integration: false,
            ..Options::default()
        });
        let before = cpu_seconds();
        terminal::start(id, common::COLUMNS as u16, common::LINES as u16, 8, 16, None, None).ok()?;
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let holder = (held > 0).then(|| {
            let done = Arc::clone(&done);
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(FRAME.saturating_sub(Duration::from_millis(held)));
                    if let Some(backend) = terminal::backend(id) {
                        let term = backend.lock();
                        std::thread::sleep(Duration::from_millis(held));
                        drop(term);
                    }
                }
            })
        });
        let deadline = Instant::now() + Duration::from_secs(90);
        while !result.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(holder) = holder {
            let _ = holder.join();
        }
        let cpu = cpu_seconds() - before;
        terminal::stop(id);
        let text = std::fs::read_to_string(&result).ok()?;
        let seconds = text.split_whitespace().next()?.parse().ok()?;
        Some((seconds, cpu))
    }

    /// Writes a file that must not be there yet.
    fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write as _;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(bytes)
    }

    /// The CPU time of this process so far, of all its threads, in seconds.
    fn cpu_seconds() -> f64 {
        // SAFETY: `getrusage` fills the struct it is given, which lives on this stack.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: as above; RUSAGE_SELF needs nothing else.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) };
        let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
        seconds(usage.ru_utime) + seconds(usage.ru_stime)
    }

    pub fn run(settings: &Settings) {
        // A directory of its own, made here and for this user alone: a directory or a link
        // that someone else made under that name beforehand would hand them the program that
        // runs here.
        let started = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory: PathBuf = std::env::temp_dir().join(format!("kudari-pty-{}-{started}", std::process::id()));
        if let Err(error) = std::fs::DirBuilder::new().mode(0o700).create(&directory) {
            eprintln!("pty: no directory {}: {error}", directory.display());
            return;
        }
        let program = directory.join("program.py");
        if write_new(&program, PROGRAM.as_bytes()).is_err() {
            let _ = std::fs::remove_dir_all(&directory);
            return;
        }
        front();
        settings.heading(&format!(
            "pty: median of {} runs of a session with its PTY, to the answer after the last byte",
            settings.runs
        ));
        settings.heading(&format!(
            "{:<8} {:>6}  {:>7} {:>8} {:>8} {:>8}",
            "load", "MB", "held ms", "s", "MB/s", "CPU s"
        ));
        let mut id = 1_000;
        for (name, data) in common::loads() {
            let load = directory.join(format!("{name}.txt"));
            if write_new(&load, &data).is_err() {
                continue;
            }
            for held in HELD {
                let mut times = Vec::new();
                let mut used = Vec::new();
                for _ in 0..settings.runs {
                    id += 1;
                    if let Some((seconds, cpu)) = session(id, &program, &load, held) {
                        times.push(Duration::from_secs_f64(seconds));
                        used.push(Duration::from_secs_f64(cpu));
                    }
                }
                if times.is_empty() {
                    eprintln!("pty: {name} with {held} ms held did not finish; is python3 there?");
                    continue;
                }
                let (time, cpu) = (median(times), median(used));
                settings.record("pty", &format!("{name}-{held}ms"), "seconds", time.as_secs_f64());
                if !settings.csv {
                    println!(
                        "{name:<8} {:>6.1}  {held:>7} {:>8.3} {:>8.0} {:>8.2}",
                        data.len() as f64 / 1e6,
                        time.as_secs_f64(),
                        data.len() as f64 / 1e6 / time.as_secs_f64(),
                        cpu.as_secs_f64(),
                    );
                }
            }
        }
        let _ = std::fs::remove_dir_all(&directory);
    }
}

#[cfg(not(unix))]
mod pty {
    pub fn run(_settings: &super::Settings) {
        println!("pty: only on Unix");
    }
}
