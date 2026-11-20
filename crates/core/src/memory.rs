// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Gives the memory that terminals freed back to the system, as after closing one or clearing
//! its history: the allocator of glibc keeps what was freed for the process otherwise, so that
//! a terminal with a long history left its size behind after it was gone.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long after the last request to wait: the thread of a terminal frees it a moment after
/// it was told to end, and closing several tabs makes one trim.
const AFTER: Duration = Duration::from_millis(1500);

/// When memory was last freed, while a thread waits to give it back.
static REQUESTED: Mutex<Option<Instant>> = Mutex::new(None);

#[cfg(test)]
static TRIMS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Gives back what the allocator keeps of freed memory, on a thread of its own a moment after
/// the last call: the trim holds the locks of the allocator, which the threads that draw and
/// that read the terminals need. Nothing where the allocator is not that of glibc.
pub fn release_later() {
    let mut requested = REQUESTED.lock().unwrap_or_else(PoisonError::into_inner);
    let waiting = requested.is_some();
    *requested = Some(Instant::now());
    drop(requested);
    if waiting {
        return;
    }
    let started = std::thread::Builder::new().name("kudari-trim".to_string()).spawn(|| {
        loop {
            let wait = {
                let mut requested = REQUESTED.lock().unwrap_or_else(PoisonError::into_inner);
                let wait = requested.map_or(Duration::ZERO, |at| AFTER.saturating_sub(at.elapsed()));
                if wait.is_zero() {
                    *requested = None;
                }
                wait
            };
            if wait.is_zero() {
                trim();
                return;
            }
            std::thread::sleep(wait);
        }
    });
    if started.is_err() {
        *REQUESTED.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

fn trim() {
    #[cfg(test)]
    TRIMS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: `malloc_trim` takes no pointer; it only hands free pages of the arenas back.
    unsafe {
        libc::malloc_trim(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn requests_close_together_make_one_trim_after_the_last() {
        let before = TRIMS.load(Ordering::Relaxed);
        release_later();
        std::thread::sleep(Duration::from_millis(1000));
        // Still within the time after the first: this one moves the trim on.
        release_later();
        std::thread::sleep(Duration::from_millis(1000));
        assert_eq!(TRIMS.load(Ordering::Relaxed), before, "not yet, 1.5 s after the last");
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(TRIMS.load(Ordering::Relaxed), before + 1);
        // A request after that makes a trim of its own.
        release_later();
        std::thread::sleep(Duration::from_millis(2000));
        assert_eq!(TRIMS.load(Ordering::Relaxed), before + 2);
    }
}
