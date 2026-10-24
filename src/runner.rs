//! The single-threaded control loop: poll, decide, emit, sleep.
//!
//! Running everything on one thread avoids holding a lock around the engine and
//! makes the ordering between "read the pad" and "fire the scheduled events"
//! deterministic. Latency is bounded by the poll interval plus one wake-up.
//!
//! Backend failures are *survivable*: a pad read that errors is treated as "no
//! pad this frame", and input that cannot be delivered is queued and retried.
//! `SendInput` refuses to inject while the desktop is not the active input
//! desktop (lock screen, UAC prompt, Ctrl+Alt+Del) or when another process
//! blocks input, so treating those as fatal used to end the run silently.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use tracing::{debug, error, info, warn};

use crate::clock::Clock;
use crate::engine::{Action, Engine};
use crate::gamepad::{GamepadSource, PadState};
use crate::input::{InputFrame, KeyboardSource};
use crate::output::KeyEmitter;
use crate::safety::FocusGuard;

/// How often the foreground window is re-checked while emitting.
///
/// Re-checking on every poll would call `OpenProcess` hundreds of times per
/// second. A few dozen milliseconds is short enough that a focus change cannot
/// leak more than a couple of synthetic events.
const FOCUS_CHECK_INTERVAL_MS: u64 = 25;

/// How many actions may queue up while emission keeps failing.
///
/// The queue exists so a release is never dropped; the cap keeps a long outage
/// from growing without bound.
const RETRY_LIMIT: usize = 64;

/// Wiring for one run of the program.
pub struct Runner {
    /// The output state machine.
    pub engine: Engine,
    /// Controller frames.
    pub source: Box<dyn GamepadSource>,
    /// Keyboard state.
    pub keyboard: Box<dyn KeyboardSource>,
    /// Synthetic input sink.
    pub emitter: Box<dyn KeyEmitter>,
    /// Focus gate.
    pub guard: Box<dyn FocusGuard>,
    /// How often the pad is polled.
    pub poll_interval_ms: u64,
    /// Whether to release everything on the way out.
    pub release_on_exit: bool,
}

impl Runner {
    /// Runs until `shutdown` is set.
    ///
    /// `toggle` is consumed: every set to `true` flips between normal operation
    /// and suspended.
    ///
    /// # Errors
    ///
    /// Propagates backend failures from the source and the emitter.
    pub fn run(
        &mut self,
        clock: &dyn Clock,
        shutdown: &AtomicBool,
        toggle: &AtomicBool,
    ) -> Result<()> {
        let mut suspended = false;
        let mut connected: Option<bool> = None;
        let mut guard_allows = true;
        let mut guard_checked_at: Option<u64> = None;
        let mut retry: Vec<Action> = Vec::new();
        let mut emit_failing = false;
        let mut poll_failing = false;

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            // Clear the backlog before adding to it, so a transient outage
            // cannot reorder a release behind newer presses.
            if !self.flush_retry(&mut retry, &mut emit_failing) {
                clock.sleep(Duration::from_millis(self.poll_interval_ms.max(1)));
                continue;
            }

            if toggle.swap(false, Ordering::Relaxed) {
                suspended = !suspended;
                if suspended {
                    info!("suspended: all keys released");
                } else {
                    info!("resumed");
                }
            }

            let now = clock.now_ms();

            let due_for_check =
                guard_checked_at.is_none_or(|at| now.saturating_sub(at) >= FOCUS_CHECK_INTERVAL_MS);
            if due_for_check {
                guard_checked_at = Some(now);
                let allows = self.guard.allows();
                if allows != guard_allows {
                    if allows {
                        debug!("foreground target matched, emitting enabled");
                    } else {
                        debug!("foreground target not matched, emitting paused");
                    }
                }
                guard_allows = allows;
            }

            let active = !suspended && guard_allows;

            let pad = self.poll_pad(&mut poll_failing);
            self.announce_connection(pad.is_some(), &mut connected);

            let frame = InputFrame {
                pad,
                keyboard: self.keyboard.poll(),
            };

            // Keyboard bindings keep working without a pad, so a missing
            // controller is no longer a reason to release everything: the frame
            // simply reports no controller button as pressed.
            let actions = if active {
                self.engine.on_input(now, &frame)
            } else {
                self.engine.release_all()
            };
            self.deliver(&actions, &mut retry, &mut emit_failing);

            if active {
                let since_poll = clock.now_ms();
                let actions = self.engine.tick(since_poll);
                self.deliver(&actions, &mut retry, &mut emit_failing);
            }

            let mut sleep_ms = self.poll_interval_ms;
            if active && let Some(deadline) = self.engine.next_deadline() {
                sleep_ms = sleep_ms.min(deadline.saturating_sub(clock.now_ms()));
            }
            // Never spin: even an overdue deadline gets a 1 ms breather.
            clock.sleep(Duration::from_millis(sleep_ms.max(1)));
        }

        if self.release_on_exit {
            let actions = self.engine.release_all();
            if let Err(error) = self.emit(&actions) {
                // Report the failure, but the caller still exits; a key that
                // cannot be released is worth a non-zero exit code.
                warn!("releasing keys on exit failed ({error:#})");
                return Err(error);
            }
        }

        Ok(())
    }

    /// Delivers the retry queue. Returns `false` while delivery is blocked.
    ///
    /// Called before anything new is queued, so a release cannot end up behind
    /// presses that arrived during the outage.
    fn flush_retry(&mut self, retry: &mut Vec<Action>, emit_failing: &mut bool) -> bool {
        if retry.is_empty() {
            return true;
        }

        let pending = std::mem::take(retry);
        match self.emit(&pending) {
            Ok(()) => {
                if *emit_failing {
                    info!("input emission recovered");
                    *emit_failing = false;
                }
                true
            }
            Err(error) => {
                if !*emit_failing {
                    error!("input emission failed ({error:#}); retrying");
                    *emit_failing = true;
                }
                queue(retry, pending);
                false
            }
        }
    }

    /// Polls the pad, downgrading a backend error to "not connected".
    ///
    /// `XInput` returns transient codes while a pad is unplugged or
    /// re-enumerated, which must not end the run.
    fn poll_pad(&mut self, failing: &mut bool) -> Option<PadState> {
        match self.source.poll() {
            Ok(pad) => {
                if *failing {
                    info!("controller read recovered");
                    *failing = false;
                }
                pad
            }
            Err(error) => {
                if !*failing {
                    warn!("controller read failed ({error:#}); treating the pad as disconnected");
                    *failing = true;
                }
                None
            }
        }
    }

    /// Logs a connection change, once per transition.
    fn announce_connection(&self, present: bool, connected: &mut Option<bool>) {
        if present {
            if *connected != Some(true) {
                info!("controller connected");
                *connected = Some(true);
            }
        } else if *connected != Some(false) {
            if self.engine.uses_controller() {
                warn!("controller not connected, waiting for it to appear");
            } else {
                debug!("controller not connected; no controller bindings are configured");
            }
            *connected = Some(false);
        }
    }

    /// Emits a batch, holding it in `retry` when delivery fails.
    ///
    /// Emission is retried rather than treated as fatal: `SendInput` fails
    /// while the secure desktop is up, which is a temporary condition.
    fn deliver(&mut self, actions: &[Action], retry: &mut Vec<Action>, emit_failing: &mut bool) {
        if actions.is_empty() {
            return;
        }

        match self.emit(actions) {
            Ok(()) => {
                if *emit_failing {
                    info!("input emission recovered");
                    *emit_failing = false;
                }
            }
            Err(error) => {
                if !*emit_failing {
                    error!("input emission failed ({error:#}); retrying");
                    *emit_failing = true;
                }
                queue(retry, actions.iter().copied());
                // Keep running: the next iteration retries the queue.
            }
        }
    }

    fn emit(&mut self, actions: &[Action]) -> Result<()> {
        if actions.is_empty() {
            return Ok(());
        }
        for action in actions {
            debug!("{action:?}");
        }
        self.emitter.emit(actions)
    }
}

/// Appends `actions` to `queue`, dropping the oldest entries past the cap.
///
/// Oldest-first dropping keeps the newest state: a press followed by its
/// release survives, while stale duplicates from a long outage do not.
fn queue(queue: &mut Vec<Action>, actions: impl IntoIterator<Item = Action>) {
    queue.extend(actions);
    if queue.len() > RETRY_LIMIT {
        let excess = queue.len() - RETRY_LIMIT;
        queue.drain(..excess);
    }
}
