// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Counters of what Kudari does, for the GUI test and the bench: how often the PTY thread
//! wakes the user interface, how often a terminal refreshes and paints, how long the lock of a
//! terminal is waited for and held, how long work waits in the queue of the Qt thread, and
//! what the parts of a frame take.
//!
//! Only a build with the feature `stats` counts; `test-hooks` turns it on as well. Without it,
//! every function here is empty and every type has no content, so the program does not change.
//! With it, the counters are atomics, and the times go into histograms of powers of two in
//! microseconds, which need no lock and no allocation.

use std::time::Duration;

/// What is counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    /// Output woke the PTY thread's listener.
    Wakeups,
    /// The listener told the user interface of output, once until it took it.
    Reports,
    /// A terminal refreshed what it shows.
    Refreshes,
}

/// What is timed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Time {
    /// From asking for the lock of a terminal until having it.
    LockWait,
    /// From having the lock of a terminal until giving it back.
    LockHold,
    /// From queueing work for the Qt thread until it runs.
    Queue,
    /// Building the paints of a frame.
    FrameBuild,
    /// Drawing the paints of a frame into its image.
    FrameDraw,
    /// Copying the image of a frame into the item.
    FrameCopy,
}

#[cfg(feature = "stats")]
mod counting {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

    use super::{Count, Time};

    /// Buckets of powers of two in microseconds: under 1, 1 to 2, 2 to 4, and so on; the last
    /// takes everything from about 34 minutes on.
    pub const BUCKETS: usize = 32;

    pub struct Histogram {
        buckets: [AtomicU64; BUCKETS],
        total: AtomicU64,
        most: AtomicU64,
    }

    impl Histogram {
        pub const fn new() -> Self {
            Self {
                buckets: [const { AtomicU64::new(0) }; BUCKETS],
                total: AtomicU64::new(0),
                most: AtomicU64::new(0),
            }
        }

        pub fn add(&self, time: Duration) {
            let micros = u64::try_from(time.as_micros()).unwrap_or(u64::MAX);
            let bucket = micros.bit_width() as usize;
            if let Some(bucket) = self.buckets.get(bucket.min(BUCKETS - 1)) {
                bucket.fetch_add(1, Ordering::Relaxed);
            }
            self.total.fetch_add(micros, Ordering::Relaxed);
            self.most.fetch_max(micros, Ordering::Relaxed);
        }

        pub fn reset(&self) {
            for bucket in &self.buckets {
                bucket.store(0, Ordering::Relaxed);
            }
            self.total.store(0, Ordering::Relaxed);
            self.most.store(0, Ordering::Relaxed);
        }

        pub fn buckets(&self) -> Vec<u64> {
            self.buckets
                .iter()
                .map(|bucket| bucket.load(Ordering::Relaxed))
                .collect()
        }

        pub fn summary(&self) -> super::Summary {
            let buckets: Vec<u64> = self
                .buckets
                .iter()
                .map(|bucket| bucket.load(Ordering::Relaxed))
                .collect();
            let count = buckets.iter().sum();
            // The upper end of the bucket that holds the given share of all times.
            let quantile = |share: f64| {
                let needed = (count as f64 * share).ceil() as u64;
                let mut seen = 0;
                for (index, found) in buckets.iter().enumerate() {
                    seen += found;
                    if seen >= needed.max(1) {
                        return 1u64 << index;
                    }
                }
                0
            };
            super::Summary {
                count,
                total_micros: self.total.load(Ordering::Relaxed),
                most_micros: self.most.load(Ordering::Relaxed),
                p50_micros: if count == 0 { 0 } else { quantile(0.5) },
                p95_micros: if count == 0 { 0 } else { quantile(0.95) },
            }
        }
    }

    pub static COUNTS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];
    pub static TIMES: [Histogram; 6] = [const { Histogram::new() }; 6];
    /// Paints for each terminal, by its id.
    pub static PAINTS: Mutex<BTreeMap<i32, u64>> = Mutex::new(BTreeMap::new());
    /// Work queued for the Qt thread that has not run yet.
    pub static QUEUED: AtomicU64 = AtomicU64::new(0);
    pub static QUEUED_MOST: AtomicU64 = AtomicU64::new(0);
    /// When anything was painted last, in microseconds from the first use of these counters.
    pub static LAST_PAINT: AtomicU64 = AtomicU64::new(0);
    pub static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

    pub fn index(count: Count) -> usize {
        match count {
            Count::Wakeups => 0,
            Count::Reports => 1,
            Count::Refreshes => 2,
        }
    }

    pub fn time_index(time: Time) -> usize {
        match time {
            Time::LockWait => 0,
            Time::LockHold => 1,
            Time::Queue => 2,
            Time::FrameBuild => 3,
            Time::FrameDraw => 4,
            Time::FrameCopy => 5,
        }
    }

    pub fn since_start() -> u64 {
        let start = START.get_or_init(Instant::now);
        u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    pub fn paints() -> std::sync::MutexGuard<'static, BTreeMap<i32, u64>> {
        PAINTS.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether this build counts.
#[must_use]
pub const fn enabled() -> bool {
    cfg!(feature = "stats")
}

/// Counts one of `what`.
#[inline]
pub fn count(what: Count) {
    #[cfg(feature = "stats")]
    if let Some(counter) = counting::COUNTS.get(counting::index(what)) {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(not(feature = "stats"))]
    let _ = what;
}

/// Counts a paint of the terminal `terminal`.
#[inline]
pub fn paint(terminal: i32) {
    #[cfg(feature = "stats")]
    {
        *counting::paints().entry(terminal).or_default() += 1;
        counting::LAST_PAINT.store(counting::since_start(), std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(not(feature = "stats"))]
    let _ = terminal;
}

/// Adds a time of `what` that was measured elsewhere.
#[inline]
pub fn record(what: Time, time: Duration) {
    #[cfg(feature = "stats")]
    if let Some(histogram) = counting::TIMES.get(counting::time_index(what)) {
        histogram.add(time);
    }
    #[cfg(not(feature = "stats"))]
    let _ = (what, time);
}

/// A clock that runs from its start until [`Stopwatch::stop`]; without the feature, it is
/// nothing and reads no clock.
#[must_use]
pub struct Stopwatch {
    #[cfg(feature = "stats")]
    started: std::time::Instant,
}

impl Stopwatch {
    #[inline]
    pub fn start() -> Self {
        Self {
            #[cfg(feature = "stats")]
            started: std::time::Instant::now(),
        }
    }

    /// Adds the time since the start to `what`, and starts again.
    #[inline]
    pub fn lap(&mut self, what: Time) {
        #[cfg(feature = "stats")]
        {
            let now = std::time::Instant::now();
            record(what, now - self.started);
            self.started = now;
        }
        #[cfg(not(feature = "stats"))]
        let _ = what;
    }

    /// Adds the time since the start to `what`.
    #[inline]
    pub fn stop(mut self, what: Time) {
        self.lap(what);
    }
}

/// Work queued for the Qt thread: counted until it runs, then timed.
#[must_use]
pub struct Queued {
    #[cfg(feature = "stats")]
    since: std::time::Instant,
}

impl Queued {
    #[inline]
    pub fn new() -> Self {
        #[cfg(feature = "stats")]
        {
            use std::sync::atomic::Ordering;
            let waiting = counting::QUEUED.fetch_add(1, Ordering::Relaxed) + 1;
            counting::QUEUED_MOST.fetch_max(waiting, Ordering::Relaxed);
        }
        Self {
            #[cfg(feature = "stats")]
            since: std::time::Instant::now(),
        }
    }

    /// The work runs now.
    #[inline]
    pub fn runs(self) {
        #[cfg(feature = "stats")]
        {
            counting::QUEUED.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            record(Time::Queue, self.since.elapsed());
        }
    }
}

impl Default for Queued {
    fn default() -> Self {
        Self::new()
    }
}

/// A histogram in short: how many times, their sum and the longest, and the upper ends of the
/// buckets that hold the half and the 95 percent of them, in microseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub count: u64,
    pub total_micros: u64,
    pub most_micros: u64,
    pub p50_micros: u64,
    pub p95_micros: u64,
}

/// How long no terminal was painted, or none while work waits in the queue of the Qt thread:
/// then the user interface has not settled, however long ago the last paint was. Without
/// the feature, the longest time, as there is nothing to wait for.
#[must_use]
pub fn quiet() -> Option<Duration> {
    #[cfg(feature = "stats")]
    {
        use std::sync::atomic::Ordering;
        if counting::QUEUED.load(Ordering::Relaxed) > 0 {
            return None;
        }
        let idle = counting::since_start().saturating_sub(counting::LAST_PAINT.load(Ordering::Relaxed));
        Some(Duration::from_micros(idle))
    }
    #[cfg(not(feature = "stats"))]
    Some(Duration::MAX)
}

/// All counters as JSON, with the paints of each terminal; `{}` without the feature.
#[must_use]
pub fn json() -> String {
    #[cfg(feature = "stats")]
    {
        use std::sync::atomic::Ordering;
        let count = |what| {
            counting::COUNTS
                .get(counting::index(what))
                .map_or(0, |counter| counter.load(Ordering::Relaxed))
        };
        let time = |what| {
            let histogram = counting::TIMES.get(counting::time_index(what));
            let summary = histogram.map_or_else(Summary::default, counting::Histogram::summary);
            serde_json::json!({
                "buckets": histogram.map_or_else(Vec::new, counting::Histogram::buckets),
                "count": summary.count,
                "total_us": summary.total_micros,
                "max_us": summary.most_micros,
                "p50_us": summary.p50_micros,
                "p95_us": summary.p95_micros,
            })
        };
        let paints: serde_json::Map<String, serde_json::Value> = counting::paints()
            .iter()
            .map(|(terminal, paints)| (terminal.to_string(), (*paints).into()))
            .collect();
        serde_json::json!({
            "wakeups": count(Count::Wakeups),
            "reports": count(Count::Reports),
            "refreshes": count(Count::Refreshes),
            "paints": paints,
            "queued": counting::QUEUED.load(Ordering::Relaxed),
            "queued_max": counting::QUEUED_MOST.load(Ordering::Relaxed),
            "lock_wait": time(Time::LockWait),
            "lock_hold": time(Time::LockHold),
            "queue": time(Time::Queue),
            "frame_build": time(Time::FrameBuild),
            "frame_draw": time(Time::FrameDraw),
            "frame_copy": time(Time::FrameCopy),
        })
        .to_string()
    }
    #[cfg(not(feature = "stats"))]
    String::from("{}")
}

/// With `KUDARI_STATS=1` in a build that counts, writes all counters to stderr on a thread of
/// its own, once a second or every `KUDARI_STATS_MS` milliseconds (10 at least): a line of
/// `kudari-stats`, the seconds since 1970 and [`json`]. The counters keep counting; a reader,
/// such as `tools/bench.py`, takes the differences between two lines. Without the feature or
/// the variable, it does nothing.
pub fn report_from_environment() {
    #[cfg(feature = "stats")]
    {
        if std::env::var_os("KUDARI_STATS").is_none_or(|value| value != "1") {
            return;
        }
        let every = std::env::var("KUDARI_STATS_MS")
            .ok()
            .and_then(|milliseconds| milliseconds.parse::<u64>().ok())
            .map_or(Duration::from_secs(1), |milliseconds| {
                Duration::from_millis(milliseconds.max(10))
            });
        let _ = std::thread::Builder::new()
            .name("kudari-stats".to_string())
            .spawn(move || {
                loop {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64();
                    eprintln!("kudari-stats {now:.3} {}", json());
                    std::thread::sleep(every);
                }
            });
    }
}

/// Sets every counter back to nothing, but not what waits in the queue, which is still there.
pub fn reset() {
    #[cfg(feature = "stats")]
    {
        use std::sync::atomic::Ordering;
        for counter in &counting::COUNTS {
            counter.store(0, Ordering::Relaxed);
        }
        for histogram in &counting::TIMES {
            histogram.reset();
        }
        counting::paints().clear();
        counting::QUEUED_MOST.store(counting::QUEUED.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

#[cfg(all(test, feature = "stats"))]
mod tests {
    use super::*;

    /// All of the global counters in one test, as tests run side by side.
    #[test]
    fn the_counters_count_know_the_quiet_and_go_back_to_nothing() {
        reset();
        count(Count::Wakeups);
        count(Count::Wakeups);
        count(Count::Refreshes);
        paint(7);
        paint(7);
        paint(9);
        record(Time::LockWait, Duration::from_micros(3));
        let queued = Queued::new();
        // Work waits in the queue: nothing has settled, whatever the time since a paint.
        assert_eq!(quiet(), None);
        queued.runs();
        assert!(
            quiet().is_some_and(|quiet| quiet < Duration::from_secs(60)),
            "a paint just now"
        );
        let stats: serde_json::Value = serde_json::from_str(&json()).unwrap_or_default();
        assert_eq!(stats["wakeups"], 2);
        assert_eq!(stats["refreshes"], 1);
        assert_eq!(stats["paints"]["7"], 2);
        assert_eq!(stats["paints"]["9"], 1);
        assert_eq!(stats["lock_wait"]["count"], 1);
        // 3 µs lie in the bucket of 2 to 4, the third.
        assert_eq!(stats["lock_wait"]["buckets"][2], 1);
        assert_eq!(stats["queue"]["count"], 1);
        assert_eq!(stats["queued"], 0);
        assert_eq!(stats["queued_max"], 1);
        reset();
        let stats: serde_json::Value = serde_json::from_str(&json()).unwrap_or_default();
        assert_eq!(stats["wakeups"], 0);
        assert_eq!(stats["paints"], serde_json::json!({}));
        assert_eq!(stats["lock_wait"]["count"], 0);
    }

    #[test]
    fn times_go_into_buckets_of_powers_of_two() {
        let histogram = counting::Histogram::new();
        for micros in [0, 1, 3, 3, 700, 5000] {
            histogram.add(Duration::from_micros(micros));
        }
        let summary = histogram.summary();
        assert_eq!(summary.count, 6);
        assert_eq!(summary.total_micros, 5707);
        assert_eq!(summary.most_micros, 5000);
        // Half of six are three: 0, 1 and the first time of 3, which lies in 2 to 4.
        assert_eq!(summary.p50_micros, 4);
        // Ninety-five percent need all six; 5000 lies in 4096 to 8192.
        assert_eq!(summary.p95_micros, 8192);
    }
}
