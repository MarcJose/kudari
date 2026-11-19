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
use alacritty_terminal::term::{Config, Term, TermMode};
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
#[ignore = "alacritty_terminal 0.26 leaves the spacer of a wide character in column 0 after DCH, where the cursor of the selection mode panics"]
fn deleting_half_of_a_wide_character_leaves_a_cursor_that_moves() {
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
#[ignore = "alacritty_terminal 0.26 wraps a wide character in the last line below the scrolling region into a cell outside the grid"]
fn a_wide_character_below_the_scrolling_region_keeps_the_cursor_in_the_grid() {
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
#[ignore = "alacritty_terminal 0.26 counts DCH past the end of the line from the end, and deletes cells before the cursor"]
fn deleting_past_the_end_of_the_line_keeps_what_is_before_the_cursor() {
    let term = term(10, 3, Config::default(), b"abcdefghij\r\x1b[5C\x1b[6P");
    assert_eq!(line_text(&term, 0), "abcde");
}

#[test]
#[ignore = "alacritty_terminal 0.26 leaves the lines above the line of the cursor as they are with ED 1"]
fn erasing_to_the_cursor_erases_the_lines_above() {
    let term = term(10, 3, Config::default(), b"erste\r\nzweite\x1b[2;3H\x1b[1J");
    assert_eq!(
        (line_text(&term, 0), line_text(&term, 1)),
        (String::new(), "   ite".to_string())
    );
}

#[test]
#[ignore = "alacritty_terminal 0.26 takes the first cell of the line before for the wide character that wrapped"]
fn copying_to_the_end_of_a_line_takes_the_wide_character_that_wrapped() {
    let mut term = term(10, 4, Config::default(), "x\r\naaaaaaaaa界".as_bytes());
    let mut selection = Selection::new(SelectionType::Simple, Point::new(Line(1), Column(0)), Side::Left);
    selection.update(Point::new(Line(1), Column(9)), Side::Right);
    term.selection = Some(selection);
    assert_eq!(term.selection_to_string().as_deref(), Some("aaaaaaaaa界"));
}

#[test]
#[ignore = "alacritty_terminal 0.26 drops the oldest lines of a full history when a narrower grid wraps them, and does not get them back"]
fn a_narrower_and_wider_grid_keeps_a_full_history() {
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
