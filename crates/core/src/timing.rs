// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Marks of the phases of the start and of showing the window, for `tools/bench.sh`: with
//! `KUDARI_TIMING=1`, each phase writes `kudari-timing PHASE SECONDS` to stderr, the seconds
//! since 1970 when it was reached. Only the names of the phases and their times go out,
//! nothing of what the terminals show. Without the variable, nothing is written; every build
//! has the marks, as they cost one look at a flag.

use std::sync::OnceLock;

/// The variables of this module, of [`crate::stats`] and of how long a hidden window keeps
/// its graphics, which a shell in Kudari should not inherit: a Kudari started in it would
/// mark, count and keep as well.
pub const VARIABLES: [&str; 4] = [
    "KUDARI_TIMING",
    "KUDARI_STATS",
    "KUDARI_STATS_MS",
    "KUDARI_RELEASE_HIDDEN_MS",
];

fn wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| std::env::var_os("KUDARI_TIMING").is_some_and(|value| value == "1"))
}

/// Marks that `phase` was reached.
pub fn mark(phase: &str) {
    if wanted() {
        eprintln!("kudari-timing {phase} {:.6}", now());
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
