// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Looks at the file system for the links in a terminal, on a thread of its own and within a
//! time. A look at a slow or hanging drive, a stopped FUSE service or a share whose server is
//! gone, can take seconds or never end; the window, the drawing and the program in the
//! terminal must not wait for it. There is then no file link until the drive answers again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

/// How long the user interface waits for a look: far longer than one at a local disk takes,
/// short enough that a hanging drive goes unnoticed under the pointer.
pub const TIME: Duration = Duration::from_millis(50);

type Job = Box<dyn FnOnce() + Send>;

/// The thread that looks, started with the first look.
static WORKER: Mutex<Option<mpsc::Sender<Job>>> = Mutex::new(None);

/// Whether a look is still running. One that hangs keeps it set: later looks then give
/// nothing at once instead of waiting, or piling up behind it, until it ends.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Sets [`BUSY`] back when a look ends, also when it panics.
struct Done;

impl Drop for Done {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

/// The result of `look`, or nothing when it takes longer than `time` or a look before it
/// still runs.
pub fn within<R: Send + 'static>(time: Duration, look: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    if BUSY.swap(true, Ordering::AcqRel) {
        return None;
    }
    let (answer, answered) = mpsc::sync_channel(1);
    let job: Job = Box::new(move || {
        let done = Done;
        #[cfg(feature = "test-hooks")]
        std::thread::sleep(Duration::from_millis(SLOWER.load(Ordering::Relaxed)));
        let found = look();
        drop(done);
        let _ = answer.send(found);
    });
    if let Err(job) = send(job) {
        drop(job);
        BUSY.store(false, Ordering::Release);
        return None;
    }
    answered.recv_timeout(time).ok()
}

/// Hands `job` to the thread that looks, which starts if it is not there or ended.
fn send(job: Job) -> Result<(), Job> {
    let mut worker = WORKER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let job = match worker.as_ref() {
        Some(sender) => match sender.send(job) {
            Ok(()) => return Ok(()),
            Err(mpsc::SendError(job)) => job,
        },
        None => job,
    };
    let (sender, jobs) = mpsc::channel::<Job>();
    let started = std::thread::Builder::new()
        .name("kudari-file-looks".to_string())
        .spawn(move || {
            for job in jobs {
                // A look that panics ends only itself; `Done` has set `BUSY` back.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            }
        });
    if started.is_err() {
        return Err(job);
    }
    let sent = sender.send(job).map_err(|mpsc::SendError(job)| job);
    *worker = Some(sender);
    sent
}

/// For the GUI test: each look waits this many milliseconds first, as at a slow drive.
#[cfg(feature = "test-hooks")]
static SLOWER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Lets each look wait `milliseconds` first; 0 ends that.
#[cfg(feature = "test-hooks")]
pub fn slow_down(milliseconds: u64) {
    SLOWER.store(milliseconds, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// One test at a time: the thread that looks is one for the whole process.
    static ALONE: Mutex<()> = Mutex::new(());

    fn alone() -> std::sync::MutexGuard<'static, ()> {
        ALONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn a_quick_look_gives_its_result() {
        let _alone = alone();
        assert_eq!(within(Duration::from_secs(5), || 42), Some(42));
        assert_eq!(within(Duration::from_secs(5), || "again"), Some("again"));
    }

    /// A look that hangs costs the caller its time only once: the next look gives nothing at
    /// once, and looks work again when it ends.
    #[test]
    fn a_hanging_look_lets_the_caller_go_on() {
        let _alone = alone();
        let (release, released) = mpsc::channel::<()>();
        let started = Instant::now();
        let hanging = within(Duration::from_millis(50), move || released.recv().is_ok());
        assert_eq!(hanging, None);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "waited {:?}",
            started.elapsed()
        );
        let started = Instant::now();
        assert_eq!(within(Duration::from_secs(5), || 1), None);
        assert!(
            started.elapsed() < Duration::from_millis(20),
            "waited {:?}",
            started.elapsed()
        );
        release.send(()).ok();
        let again = (0..200).find_map(|_| {
            std::thread::sleep(Duration::from_millis(5));
            within(Duration::from_secs(5), || 2)
        });
        assert_eq!(again, Some(2));
    }

    #[test]
    fn a_look_that_panics_does_not_stop_the_next() {
        let _alone = alone();
        #[allow(clippy::panic, reason = "the test needs a look that panics")]
        let broken = within(Duration::from_secs(5), || -> i32 { panic!("a look that breaks") });
        assert_eq!(broken, None);
        let again = (0..200).find_map(|_| {
            std::thread::sleep(Duration::from_millis(5));
            within(Duration::from_secs(5), || 3)
        });
        assert_eq!(again, Some(3));
    }
}
