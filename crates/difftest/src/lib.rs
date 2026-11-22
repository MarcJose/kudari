// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Inputs for the tests that compare the copies of `vte` and `alacritty_terminal` with the
//! originals; see `tests/`.

/// A small generator of numbers that is the same on every computer, so a failing input comes
/// again with its seed.
pub struct Numbers(u64);

impl Numbers {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    /// The next number, by `xorshift`.
    pub fn number(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `bound`.
    pub fn below(&mut self, bound: usize) -> usize {
        (self.number() % bound.max(1) as u64) as usize
    }

    /// One of `choices`.
    pub fn pick<'a>(&mut self, choices: &[&'a str]) -> &'a str {
        choices.get(self.below(choices.len())).copied().unwrap_or_default()
    }
}

/// Where to cut `length` bytes into pieces for a parser that is fed in parts, as a PTY reads.
#[must_use]
pub fn cuts(numbers: &mut Numbers, length: usize) -> Vec<usize> {
    let mut cuts = Vec::new();
    let mut at = 0;
    while at < length {
        at += 1 + numbers.below(64);
        cuts.push(at.min(length));
    }
    cuts
}
