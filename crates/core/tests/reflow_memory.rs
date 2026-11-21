// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The memory of the history when the width of a terminal changes. A test binary of its own,
//! so that its allocator counts only what this test holds.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

/// The bytes allocated and not freed.
static HELD: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// SAFETY: every call goes to the system allocator with the same arguments; only the bytes it
// hands out and takes back are counted.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        HELD.fetch_add(isize::try_from(layout.size()).unwrap_or(isize::MAX), Ordering::Relaxed);
        // SAFETY: as for `System`, with the caller's layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        HELD.fetch_sub(isize::try_from(layout.size()).unwrap_or(isize::MAX), Ordering::Relaxed);
        // SAFETY: as for `System`: `ptr` came from it with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        HELD.fetch_add(
            isize::try_from(size).unwrap_or(isize::MAX) - isize::try_from(layout.size()).unwrap_or(isize::MAX),
            Ordering::Relaxed,
        );
        // SAFETY: as for `System`: `ptr` came from it with this layout.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn held() -> isize {
    HELD.load(Ordering::Relaxed)
}

/// A terminal with a history of 20,000 full lines of 100 columns.
fn full_history() -> Term<VoidListener> {
    let config = Config {
        scrolling_history: 20_000,
        ..Config::default()
    };
    let mut term = Term::new(config, &TermSize::new(100, 24), VoidListener);
    let line = "x".repeat(99) + "\r\n";
    Processor::<StdSyncHandler>::new().advance(&mut term, line.repeat(20_100).as_bytes());
    term
}

/// A history grows by about the cells of the new columns when the terminal is three columns
/// wider, not by twice its size. `alacritty_terminal` 0.26 doubled the room of every line it
/// widened and kept it; fixed in `third_party`.
#[test]
fn three_columns_more_take_little_more_memory() {
    let mut term = full_history();
    let before = held();
    term.resize(TermSize::new(103, 24));
    let grown = (held() - before) * 100 / before;
    assert!(grown < 10, "three columns more took {grown} % more memory");
}

/// A history gives the memory of a much wider terminal back when it is as narrow as before,
/// as after zooming out and in again.
#[test]
fn a_terminal_as_narrow_as_before_gives_the_memory_back() {
    let mut term = full_history();
    let before = held();
    term.resize(TermSize::new(140, 24));
    term.resize(TermSize::new(100, 24));
    let kept = (held() - before) * 100 / before;
    assert!(kept < 5, "as narrow as before, the history kept {kept} % more memory");
}
