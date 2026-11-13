// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! Random input for parsers. Text from files, programs and the network reaches them, and
//! with `panic = "abort"` a single panic would end all terminals.

/// Characters that mean something to the parsers, so that random input gets past the
/// first checks instead of being rejected right away.
const ALPHABET: &[char] = &[
    '[', ']', '=', ':', ';', ',', '.', '/', '\\', '"', '\'', '%', '#', '@', '+', '-', '*', '~', '_', ' ', '\n', '\t',
    '0', '1', '9', 'a', 'f', 'x', 'E', 'e', 'k', 'y', 'ä', '€', '😀', '\u{0}', '\u{1b}', '\u{7f}', '\u{2500}',
];

/// A small generator: the runs have to be repeatable, not unpredictable.
pub struct Random(u64);

impl Random {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(2) + 1)
    }

    pub fn below(&mut self, limit: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % limit.max(1) as u64) as usize
    }

    /// A character of [`ALPHABET`].
    fn character(&mut self) -> char {
        ALPHABET.get(self.below(ALPHABET.len())).copied().unwrap_or('a')
    }

    fn text(&mut self, length: usize) -> String {
        (0..length).map(|_| self.character()).collect()
    }

    /// `sample` with characters replaced, removed or inserted.
    fn mutated(&mut self, sample: &str) -> String {
        let mut characters: Vec<char> = sample.chars().collect();
        for _ in 0..=self.below(8) {
            if characters.is_empty() {
                break;
            }
            let index = self.below(characters.len());
            let replacement = self.character();
            match self.below(3) {
                0 => {
                    if let Some(character) = characters.get_mut(index) {
                        *character = replacement;
                    }
                }
                1 => drop(characters.remove(index)),
                _ => characters.insert(index, replacement),
            }
        }
        characters.into_iter().collect()
    }
}

/// Random strings of up to 80 characters, followed by damaged copies of `samples`.
#[must_use]
pub fn inputs(samples: &[&str]) -> Vec<String> {
    let mut random = Random::new(0x5eed);
    let mut inputs: Vec<String> = (0..2000)
        .map(|_| {
            let length = random.below(80);
            random.text(length)
        })
        .collect();
    for sample in samples {
        inputs.extend((0..1000).map(|_| random.mutated(sample)));
    }
    inputs
}
