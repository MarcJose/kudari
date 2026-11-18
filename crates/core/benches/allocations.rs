// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Counts the allocations of the frames and the cases of the benchmark `core`, with an
//! allocator that counts what it is asked for:
//!
//!     cargo bench -p kudari-core --bench allocations
//!
//! A frame or a case that allocates less than before is a gain that the times may not show
//! yet. Counting needs an allocator of its own, which slows down the frames by about two
//! fifths, so the times come from `core` and the counts from here.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{COLUMNS, LINES};
use kudari_core::colors;
use kudari_core::options::Options;
use kudari_core::render::{self, Paint, Scene};

/// The allocations so far, of every thread.
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// The allocator of the system, counting what it is asked for.
struct Counting;

// SAFETY: every call goes on to the allocator of the system unchanged, with its contract;
// this only counts the calls that give memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller keeps the contract of `GlobalAlloc::alloc`, which is the same.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: as for `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `pointer` came from this allocator, which is the system's.
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from this allocator, which is the system's.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The allocations of one call of `work`, on average over `calls`, after one call that may
/// fill buffers kept from one call to the next.
fn count(calls: usize, work: &mut dyn FnMut()) -> usize {
    work();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for _ in 0..calls {
        work();
    }
    (ALLOCATIONS.load(Ordering::Relaxed) - before) / calls.max(1)
}

fn main() {
    println!("allocations of a frame, {COLUMNS}×{LINES} cells");
    println!("{:<22} {:>12}", "screen", "allocations");
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
        let allocated = count(10, &mut || {
            render::frame_into(&term, &options, &scene, &mut paints);
            black_box(&paints);
        });
        println!("{name:<22} {allocated:>12}");
    }
    println!();
    println!("allocations of a case");
    println!("{:<22} {:>12}", "case", "allocations");
    common::cases(&mut |name, calls, work| {
        println!("{name:<22} {:>12}", count(calls.min(100), work));
    });
}
