//! Monotonic time and sleeping, behind a trait so the loop stays testable.

use std::time::{Duration, Instant};

/// Time source used by the [`crate::runner::Runner`].
pub trait Clock {
    /// Milliseconds elapsed since this clock was created.
    fn now_ms(&self) -> u64;

    /// Blocks for at least `duration`.
    fn sleep(&self, duration: Duration);
}

/// The real clock: `Instant` plus `thread::sleep`.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
