// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! A job for another thread that the asking thread may stop waiting for. Either the job never
//! runs, so that the asker can say that nothing happened, or it runs and the asker waits for
//! its result. A request of an agent that reports a failure must not type its text later all
//! the same, or a second attempt would type it twice.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

const WAITING: u8 = 0;
const RUNNING: u8 = 1;
const GIVEN_UP: u8 = 2;

/// The side that asks and waits.
#[derive(Debug)]
pub struct Handoff {
    state: Arc<AtomicU8>,
}

/// The side that does the job, on the other thread.
#[derive(Clone, Debug)]
pub struct Claim {
    state: Arc<AtomicU8>,
}

/// A new pair for one job.
#[must_use]
pub fn handoff() -> (Handoff, Claim) {
    let state = Arc::new(AtomicU8::new(WAITING));
    (
        Handoff {
            state: Arc::clone(&state),
        },
        Claim { state },
    )
}

impl Claim {
    /// Whether the job may run: false once the asker gave up.
    #[must_use]
    pub fn start(&self) -> bool {
        self.state
            .compare_exchange(WAITING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

impl Handoff {
    /// The result of the job, or `None` if it did not start within `timeout`; it then never
    /// runs. A job that started in time is waited for to its end.
    #[must_use]
    pub fn wait<T>(self, receiver: &Receiver<T>, timeout: Duration) -> Option<T> {
        match receiver.recv_timeout(timeout) {
            Ok(result) => Some(result),
            Err(RecvTimeoutError::Timeout) => {
                let gave_up = self
                    .state
                    .compare_exchange(WAITING, GIVEN_UP, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok();
                if gave_up { None } else { receiver.recv().ok() }
            }
            Err(RecvTimeoutError::Disconnected) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn a_job_in_time_gives_its_result() {
        let (handoff, claim) = handoff();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            if claim.start() {
                let _ = sender.send(7);
            }
        });
        assert_eq!(handoff.wait(&receiver, Duration::from_secs(5)), Some(7));
    }

    #[test]
    fn a_job_that_starts_too_late_never_runs() {
        let (handoff, claim) = handoff();
        let (sender, receiver) = mpsc::channel::<i32>();
        let ran = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        assert_eq!(handoff.wait(&receiver, Duration::from_millis(50)), None);
        assert!(started.elapsed() < Duration::from_secs(2));
        // The other thread comes to it only now, as a busy Qt thread would.
        let flag = Arc::clone(&ran);
        std::thread::spawn(move || {
            if claim.start() {
                flag.store(true, Ordering::SeqCst);
                let _ = sender.send(1);
            }
        })
        .join()
        .unwrap();
        assert!(!ran.load(Ordering::SeqCst), "the job ran after the asker had given up");
    }

    #[test]
    fn a_job_that_runs_at_the_deadline_is_waited_for() {
        let (handoff, claim) = handoff();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            assert!(claim.start());
            // Started in time, done after the deadline.
            std::thread::sleep(Duration::from_millis(200));
            let _ = sender.send("typed");
        });
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(handoff.wait(&receiver, Duration::from_millis(50)), Some("typed"));
    }
}
