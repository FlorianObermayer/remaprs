//! Randomness for spacing out the synthetic presses.
//!
//! A randomized cadence needs spread, not cryptographic quality, so a 64-bit
//! mixer seeded from the high-resolution clock is enough. Keeping it in-crate
//! avoids pulling in a random-number dependency for two calls to `next()`, and
//! lets tests swap in a fully deterministic source.

/// Supplies the randomized delays used by the engine.
pub trait Jitter: Send {
    /// Returns a value in `min..=max`, or `min` when the range is empty.
    fn range_inclusive(&mut self, min: u64, max: u64) -> u64;
}

/// Entropy-backed jitter used by the binary.
#[derive(Debug, Clone, Copy)]
pub struct RandomJitter {
    state: u64,
}

impl RandomJitter {
    #[must_use]
    pub fn new() -> Self {
        Self { state: seed() }
    }
}

impl Default for RandomJitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Jitter for RandomJitter {
    fn range_inclusive(&mut self, min: u64, max: u64) -> u64 {
        if max <= min {
            return min;
        }
        let span = max - min + 1;
        min + (splitmix64(&mut self.state) % span)
    }
}

/// Deterministic jitter for tests and reproducible dry runs.
#[derive(Debug, Clone, Copy)]
pub struct SeededJitter {
    state: u64,
}

impl SeededJitter {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl Jitter for SeededJitter {
    fn range_inclusive(&mut self, min: u64, max: u64) -> u64 {
        if max <= min {
            return min;
        }
        let span = max - min + 1;
        min + (splitmix64(&mut self.state) % span)
    }
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn seed() -> u64 {
    // Nanoseconds since the epoch do not fit in a u64 after the year 2554; the
    // saturation below is unreachable in practice and only satisfies the cast.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
        });
    let stack_marker = 0_u8;
    let address = std::ptr::from_ref(&stack_marker).addr() as u64;
    let mut state = nanos ^ address.rotate_left(17) ^ u64::from(std::process::id());
    splitmix64(&mut state);
    state
}

#[cfg(test)]
mod tests {
    use super::{Jitter, SeededJitter};

    #[test]
    fn stays_within_bounds() {
        let mut jitter = SeededJitter::new(0x5EED);
        for _ in 0..10_000 {
            let value = jitter.range_inclusive(40, 90);
            assert!((40..=90).contains(&value), "got {value}");
        }
    }

    #[test]
    fn fixed_range_returns_the_only_value() {
        let mut jitter = SeededJitter::new(1);
        for _ in 0..10 {
            assert_eq!(jitter.range_inclusive(17, 17), 17);
        }
    }

    #[test]
    fn inverted_range_degrades_to_min() {
        let mut jitter = SeededJitter::new(1);
        assert_eq!(jitter.range_inclusive(90, 40), 90);
    }

    #[test]
    fn same_seed_gives_same_sequence() {
        let mut first = SeededJitter::new(42);
        let mut second = SeededJitter::new(42);
        for _ in 0..100 {
            assert_eq!(
                first.range_inclusive(1, 1000),
                second.range_inclusive(1, 1000)
            );
        }
    }

    #[test]
    fn produces_more_than_one_value() {
        let mut jitter = SeededJitter::new(7);
        let values: std::collections::BTreeSet<u64> =
            (0..200).map(|_| jitter.range_inclusive(1, 50)).collect();
        assert!(values.len() > 10, "suspiciously few distinct values");
    }
}
