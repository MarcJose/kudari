// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Faults of `alacritty_terminal` that output or a step of the user can reach, each as the
//! smallest case that shows it. The emulator is a dependency: a fault there ends or freezes all
//! terminals of Kudari, since a release aborts on a panic. Faults that Kudari works around have
//! their tests where it does so, as in `view`.
//!
//! The faults that are still open in the version Kudari builds with are ignored here, with the
//! fault in the reason, and expect what a correct emulator does: `cargo test -- --ignored`
//! shows which a new version or a fix of Kudari mends. Each case runs on a thread of its own
//! with a limit of time, so that a hang or a panic fails with its reason rather than stopping
//! the tests.

use std::sync::mpsc;
use std::time::Duration;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermDamage, TermMode};
use alacritty_terminal::vi_mode::ViMotion;
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

use crate::colors::ColorScheme;
use crate::options::Options;
use crate::render::{self, Geometry, Scene};

/// A terminal of `columns` by `lines` with `config` that has taken in `output`.
fn term(columns: usize, lines: usize, config: Config, output: &[u8]) -> Term<VoidListener> {
    let mut term = Term::new(config, &TermSize::new(columns, lines), VoidListener);
    Processor::<StdSyncHandler>::new().advance(&mut term, output);
    term
}

/// The text of the line `line` of the screen, without the spaces at its end.
fn line_text(term: &Term<VoidListener>, line: i32) -> String {
    let row = &term.grid()[Line(line)];
    let text: String = (0..term.columns()).map(|column| row[Column(column)].c).collect();
    text.trim_end().to_string()
}

/// Builds a frame, as each paint does.
fn draw(term: &Term<VoidListener>) {
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
        focused: true,
        cursor_hidden: false,
        cursor_fade: 1.0,
        cursor_apart: false,
        hints: &[],
        matches: &[],
    };
    std::hint::black_box(render::frame(term, &Options::default(), &scene));
}

/// Runs `case` on a thread of its own and gives what it gives, or why not: a panic, or no end
/// within `limit`.
fn within<R: Send + 'static>(limit: Duration, case: impl FnOnce() -> R + Send + 'static) -> Result<R, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(case));
        let _ = sender.send(result.map_err(|_| "it panicked".to_string()));
    });
    receiver
        .recv_timeout(limit)
        .unwrap_or_else(|_| Err(format!("it took longer than {limit:?}")))
}

const SECONDS: Duration = Duration::from_secs(5);

/// Held by the tests that run a real PTY, one at a time: one measures the time of the thread of
/// the PTY of its own, which those of others would add to.
#[cfg(unix)]
static REAL_PTY: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn many_kitty_keyboard_modes_do_not_end_the_terminal() {
    // `alacritty_terminal` 0.26 took the oldest from the stack of titles instead of the keyboard
    // modes once that was full, and panicked when it was empty; fixed in `third_party`.
    let result = within(SECONDS, || {
        let config = Config {
            kitty_keyboard: true,
            ..Config::default()
        };
        let mut bytes = "\x1b[>u".repeat(4096);
        // The newest goes on top of a full stack and is the one in force.
        bytes.push_str("\x1b[>5u");
        let term = term(80, 24, config, bytes.as_bytes());
        (
            term.columns(),
            term.mode()
                .contains(TermMode::DISAMBIGUATE_ESC_CODES | TermMode::REPORT_ALTERNATE_KEYS),
        )
    });
    assert_eq!(result, Ok((80, true)));
}

#[test]
fn repeating_a_character_fills_no_more_than_the_screen() {
    // `alacritty_terminal` 0.26 repeated a character as often as REP asked, which took minutes;
    // fixed in `third_party`.
    let result = within(SECONDS, || {
        term(80, 24, Config::default(), "a\x1b[65535b".repeat(2000).as_bytes()).columns()
    });
    assert_eq!(result, Ok(80));
}

/// What the screen shows, with the cursor, and how many lines the history holds.
fn shown(term: &Term<VoidListener>) -> (Vec<Vec<(char, Flags)>>, Point, usize) {
    let lines = (0..term.screen_lines() as i32)
        .map(|line| {
            let row = &term.grid()[Line(line)];
            (0..term.columns())
                .map(|column| (row[Column(column)].c, row[Column(column)].flags))
                .collect()
        })
        .collect();
    (lines, term.grid().cursor.point, term.grid().history_size())
}

#[test]
fn a_repeat_ends_as_writing_the_character_that_often() {
    let result = within(SECONDS, || {
        let mut differ = Vec::new();
        // Where the repeat starts, how it writes, and the character.
        let starts = ["", "x\r\ny", "\x1b[3;5H", "\x1b[4h", "\x1b[?7l", "\x1b[2;4r\x1b[3;1H"];
        for start in starts {
            for c in ['a', '界', '\u{301}'] {
                for count in [1, 7, 10, 79, 81, 2000, 2001, 2017, 5000, 65535] {
                    let repeated = format!("{start}{c}\x1b[{count}b");
                    let written = format!("{start}{}", c.to_string().repeat(count + 1));
                    let (a, b) = (
                        term(10, 4, Config::default(), repeated.as_bytes()),
                        term(10, 4, Config::default(), written.as_bytes()),
                    );
                    let ((screen, cursor, _), (expected, expected_cursor, _)) = (shown(&a), shown(&b));
                    if screen != expected || cursor != expected_cursor {
                        differ.push(format!("{start:?} {c:?} {count}"));
                    }
                }
            }
        }
        differ
    });
    // The same screen and cursor; only fewer lines of the same go into the history.
    assert_eq!(result, Ok(Vec::<String>::new()));
}

#[cfg(unix)]
#[test]
fn costly_output_lets_others_have_the_terminal() {
    use std::sync::Arc;
    use std::time::Instant;

    use alacritty_terminal::event::WindowSize;
    use alacritty_terminal::event_loop::{EventLoop, Msg};
    use alacritty_terminal::sync::FairMutex;
    use alacritty_terminal::tty::{self, Options as Pty, Shell};

    // A screen filled with E (DECALN) for every four bytes: in `alacritty_terminal` 0.26 the
    // thread of the PTY parsed up to 1 MiB of it while it held the terminal, for seconds, and
    // nothing else could draw it or take input meanwhile; fixed in `third_party`.
    let _alone = REAL_PTY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let term = Arc::new(FairMutex::new(Term::new(
        Config::default(),
        &TermSize::new(80, 24),
        VoidListener,
    )));
    let options = Pty {
        shell: Some(Shell::new(
            "sh".to_string(),
            vec![
                "-c".to_string(),
                "yes \"$(printf '\\033#8')\" | head -c 4000000; sleep 30".to_string(),
            ],
        )),
        ..Pty::default()
    };
    let size = WindowSize {
        num_lines: 24,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };
    let pty = tty::new(&options, size, 0).expect("a PTY");
    let event_loop = EventLoop::new(Arc::clone(&term), VoidListener, pty, false, false).expect("an event loop");
    let channel = event_loop.channel();
    let thread = event_loop.spawn();
    // As the user interface does: take the terminal for each frame, for two seconds.
    let mut longest = Duration::ZERO;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(2) {
        let asked = Instant::now();
        let locked = term.lock();
        longest = longest.max(asked.elapsed());
        drop(locked);
        std::thread::sleep(Duration::from_millis(16));
    }
    let _ = channel.send(Msg::Shutdown);
    let _ = thread.join();
    assert!(
        longest < Duration::from_millis(200),
        "waited {longest:?} for the terminal"
    );
}

#[test]
fn deleting_half_of_a_wide_character_leaves_a_cursor_that_moves() {
    // `alacritty_terminal` 0.26 left the second half of a wide character in column 0 after
    // DCH, where the cursor of the selection mode panicked; fixed in `third_party`.
    let result = within(SECONDS, || {
        let mut term = term(10, 3, Config::default(), "界\r\x1b[P".as_bytes());
        term.toggle_vi_mode();
        term.vi_motion(ViMotion::Left);
        draw(&term);
        term.vi_mode_cursor.point.column.0
    });
    assert_eq!(result, Ok(0));
}

#[test]
fn a_wide_character_below_the_scrolling_region_keeps_the_cursor_in_the_grid() {
    // `alacritty_terminal` 0.26 marked the last line of the screen below the scrolling region as
    // going on in the next and left a spacer there, though a line feed does not move on, and a
    // move right in the selection mode went below the screen; fixed in `third_party`.
    let result = within(SECONDS, || {
        let mut term = term(10, 5, Config::default(), "\x1b[1;3r\x1b[5Habcdefghi界".as_bytes());
        term.toggle_vi_mode();
        term.vi_mode_cursor.point = Point::new(Line(4), Column(9));
        term.vi_motion(ViMotion::Right);
        draw(&term);
        term.vi_mode_cursor.point.column.0 < term.columns()
    });
    assert_eq!(result, Ok(true));
}

#[test]
fn deleting_past_the_end_of_the_line_keeps_what_is_before_the_cursor() {
    // `alacritty_terminal` 0.26 counted DCH past the end of the line from the end, and deleted
    // cells before the cursor; fixed in `third_party`.
    let term = term(10, 3, Config::default(), b"abcdefghij\r\x1b[5C\x1b[6P");
    assert_eq!(line_text(&term, 0), "abcde");
}

#[test]
fn erasing_to_the_cursor_erases_the_lines_above() {
    // `alacritty_terminal` 0.26 left the first line as it was with ED 1 from the second; fixed
    // in `third_party`.
    let term = term(10, 3, Config::default(), b"erste\r\nzweite\x1b[2;3H\x1b[1J");
    assert_eq!(
        (line_text(&term, 0), line_text(&term, 1)),
        (String::new(), "   ite".to_string())
    );
}

#[test]
fn copying_to_the_end_of_a_line_takes_the_wide_character_that_wrapped() {
    // `alacritty_terminal` 0.26 took the first cell of the line before for the wide character
    // that wrapped; fixed in `third_party`.
    let mut term = term(10, 4, Config::default(), "x\r\naaaaaaaaa界".as_bytes());
    let mut selection = Selection::new(SelectionType::Simple, Point::new(Line(1), Column(0)), Side::Left);
    selection.update(Point::new(Line(1), Column(9)), Side::Right);
    term.selection = Some(selection);
    assert_eq!(term.selection_to_string().as_deref(), Some("aaaaaaaaa界"));
}

#[test]
fn a_narrower_and_wider_grid_keeps_a_full_history() {
    // `alacritty_terminal` 0.26 dropped the oldest lines of a full history when a narrower
    // grid wrapped them, and did not get them back; fixed in `third_party`.
    let config = Config {
        scrolling_history: 100,
        ..Config::default()
    };
    let lines = "x".repeat(9) + "\r\n";
    let mut term = term(10, 5, config, lines.repeat(200).as_bytes());
    let before = term.grid().history_size();
    term.resize(TermSize::new(8, 5));
    term.resize(TermSize::new(10, 5));
    assert_eq!(term.grid().history_size(), before);
}

#[test]
fn the_last_line_below_the_scrolling_region_goes_on_in_no_other() {
    let term = term(10, 5, Config::default(), "\x1b[1;3r\x1b[5Habcdefghi界".as_bytes());
    let last = &term.grid()[Line(4)];
    // The wide character starts the line again; the last cell is an ordinary blank one.
    assert!(last[Column(0)].flags.contains(Flags::WIDE_CHAR));
    assert!(
        !last[Column(9)]
            .flags
            .intersects(Flags::WRAPLINE | Flags::LEADING_WIDE_CHAR_SPACER)
    );
}

#[cfg(windows)]
#[test]
fn a_pseudo_console_that_fails_is_an_error_not_a_panic() {
    use alacritty_terminal::event::{OnResize, WindowSize};
    use alacritty_terminal::tty::{self, Options as Pty};

    // `alacritty_terminal` 0.26 asserted that CreatePseudoConsole and ResizePseudoConsole work,
    // and a release of Kudari ended with every terminal where they did not; fixed in
    // `third_party`. A size of nothing is one that the console takes for neither.
    let nothing = WindowSize {
        num_lines: 0,
        num_cols: 0,
        cell_width: 8,
        cell_height: 16,
    };
    let created = std::panic::catch_unwind(|| tty::new(&Pty::default(), nothing, 0).is_err());
    assert_eq!(created.ok(), Some(true));
    let size = WindowSize {
        num_lines: 24,
        num_cols: 80,
        ..nothing
    };
    let mut pty = tty::new(&Pty::default(), size, 0).expect("a pseudo console");
    let resized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pty.on_resize(nothing)));
    assert!(resized.is_ok());
}

#[test]
fn deleting_a_half_of_a_wide_character_deletes_all_of_it() {
    // From the first half, and from the second: the character goes, the rest moves up.
    let first = term(10, 3, Config::default(), "a界b\r\x1b[C\x1b[P".as_bytes());
    assert_eq!(line_text(&first, 0), "a b");
    let second = term(10, 3, Config::default(), "a界b\r\x1b[2C\x1b[P".as_bytes());
    assert_eq!(line_text(&second, 0), "a b");
    for term in [first, second] {
        let row = &term.grid()[Line(0)];
        assert!((0..10).all(|column| !row[Column(column)].flags.contains(Flags::WIDE_CHAR_SPACER)));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_thread_of_the_pty_waits_for_the_terminal_without_spinning() {
    use std::sync::Arc;
    use std::time::Instant;

    use alacritty_terminal::event::WindowSize;
    use alacritty_terminal::event_loop::{EventLoop, Msg};
    use alacritty_terminal::sync::FairMutex;
    use alacritty_terminal::tty::{self, Options as Pty, Shell};

    /// The CPU time of the thread of this process named "PTY reader" so far.
    fn reader_time() -> Duration {
        let tasks = std::fs::read_dir("/proc/self/task").into_iter().flatten().flatten();
        tasks
            .filter(|task| {
                std::fs::read_to_string(task.path().join("comm")).is_ok_and(|name| name.trim() == "PTY reader")
            })
            .filter_map(|task| std::fs::read_to_string(task.path().join("schedstat")).ok())
            .filter_map(|stat| stat.split_whitespace().next()?.parse().ok())
            .map(Duration::from_nanos)
            .sum()
    }

    // `alacritty_terminal` 0.26 read again at once while the terminal was locked and the PTY
    // had nothing new, and spun on a core for as long as the lock was held; fixed in
    // `third_party`.
    let _alone = REAL_PTY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let term = Arc::new(FairMutex::new(Term::new(
        Config::default(),
        &TermSize::new(80, 24),
        VoidListener,
    )));
    let options = Pty {
        shell: Some(Shell::new(
            "sh".to_string(),
            vec!["-c".to_string(), "while :; do printf x; sleep 0.01; done".to_string()],
        )),
        ..Pty::default()
    };
    let size = WindowSize {
        num_lines: 24,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };
    let pty = tty::new(&options, size, 0).expect("a PTY");
    let event_loop = EventLoop::new(Arc::clone(&term), VoidListener, pty, false, false).expect("an event loop");
    let channel = event_loop.channel();
    let thread = event_loop.spawn();
    std::thread::sleep(Duration::from_millis(300));
    // Holding the terminal for a second, as a long search does, while output comes slowly.
    let locked = term.lock();
    let before = reader_time();
    let started = Instant::now();
    std::thread::sleep(Duration::from_secs(1));
    let spent = reader_time().saturating_sub(before);
    drop(locked);
    let _ = channel.send(Msg::Shutdown);
    let _ = thread.join();
    assert!(
        spent < started.elapsed() / 10,
        "the thread of the PTY took {spent:?} of a second"
    );
}

#[test]
fn a_repeat_after_a_run_repeats_its_last_character() {
    // The copy of vte in `third_party` hands a run of printable ASCII on at once; a repeat
    // (REP) after it repeats the last character of the run, as after one character.
    let term = term(10, 2, Config::default(), b"ab\x1b[3b");
    assert_eq!(line_text(&term, 0), "abbbb");
}

/// Random output of every kind that meets runs of printable ASCII: words and lines longer than
/// the screen, colours and links, the line-drawing charset, insert mode, lines that do not
/// wrap, wide and combining characters, cursor moves, erasing, scroll regions, repeats, and
/// inserted and deleted characters.
fn output_around_runs(seed: &mut u64) -> Vec<u8> {
    let mut next = |bound: u64| {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed % bound
    };
    let pieces: [&[u8]; 26] = [
        b"word ",
        b"a line longer than the screen is wide",
        b"\x1b[31mred\x1b[0m",
        b"\x1b[1;4;44m",
        b"\x1b[0m",
        // With an id of its own: alacritty numbers links without one across all terminals.
        b"\x1b]8;id=a;https://a.org\x1b\\link\x1b]8;;\x1b\\",
        b"\x1b(0lqqk\x1b(B",
        b"\x0eabc\x0f",
        b"\x1b[4h",
        b"\x1b[4l",
        b"\x1b[?7l",
        b"\x1b[?7h",
        "日本".as_bytes(),
        "e\u{301}".as_bytes(),
        "🙂".as_bytes(),
        b"\r",
        b"\n",
        b"\x08",
        b"\t",
        b"\x1b[K",
        b"\x1b[2J",
        b"\x1b[2;4r",
        b"\x1b[r",
        b"\x1b[3b",
        b"\x1b[2P",
        b"\x1b[2@",
    ];
    let mut bytes = Vec::new();
    for _ in 0..next(60) {
        match next(30) {
            found @ 0..=25 => bytes.extend_from_slice(pieces[found as usize]),
            26 => bytes.extend_from_slice(format!("\x1b[{};{}H", 1 + next(6), 1 + next(11)).as_bytes()),
            _ => (0..=next(30)).for_each(|_| bytes.push(b' ' + next(95) as u8)),
        }
    }
    bytes
}

/// Where two terminals differ, from the top of the history, if they do: the size of their
/// history, or the first cell. `Grid` compares only grids that never scrolled.
fn first_difference(one: &Term<VoidListener>, other: &Term<VoidListener>) -> Option<String> {
    let history = one.grid().history_size();
    if history != other.grid().history_size() {
        return Some(format!(
            "histories of {history} and {} lines",
            other.grid().history_size()
        ));
    }
    for line in -(history as i32)..one.screen_lines() as i32 {
        for column in 0..one.columns() {
            let point = Point::new(Line(line), Column(column));
            if one.grid()[point] != other.grid()[point] {
                return Some(format!(
                    "{point:?}: {:?} against {:?}",
                    one.grid()[point],
                    other.grid()[point]
                ));
            }
        }
    }
    None
}

/// The damage of a terminal since the last time, as lines with the columns that changed.
fn damage_of(term: &mut Term<VoidListener>) -> Option<Vec<(usize, usize, usize)>> {
    let found = match term.damage() {
        TermDamage::Full => None,
        TermDamage::Partial(lines) => Some(lines.map(|line| (line.line, line.left, line.right)).collect()),
    };
    term.reset_damage();
    found
}

#[test]
fn printable_runs_leave_the_terminal_as_characters_one_by_one() {
    // `alacritty_terminal` 0.26 took each character of a run on its own; the copy in
    // `third_party` writes a run a line at a time where nothing but the characters changes.
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    for _ in 0..2000 {
        let bytes = output_around_runs(&mut seed);
        let config = || Config {
            scrolling_history: 20,
            ..Config::default()
        };
        let mut runs = Term::new(config(), &TermSize::new(10, 5), VoidListener);
        let mut single = Term::new(config(), &TermSize::new(10, 5), VoidListener);
        single.input_by_character(true);
        let (mut by_runs, mut by_characters) = (Processor::<StdSyncHandler>::new(), Processor::<StdSyncHandler>::new());
        let mut at = 0;
        while at < bytes.len() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let end = (at + 1 + (seed % 40) as usize).min(bytes.len());
            by_runs.advance(&mut runs, &bytes[at..end]);
            by_characters.advance(&mut single, &bytes[at..end]);
            at = end;
            let shown = String::from_utf8_lossy(&bytes[..at]);
            if let Some(difference) = first_difference(&runs, &single) {
                panic!("{difference} after {shown:?}");
            }
            assert_eq!(runs.grid().cursor, single.grid().cursor, "the cursor after {shown:?}");
            assert_eq!(runs.mode(), single.mode(), "the modes after {shown:?}");
            assert_eq!(
                damage_of(&mut runs),
                damage_of(&mut single),
                "the damage after {shown:?}"
            );
        }
    }
}
