//! The input-to-output state machine.
//!
//! One [`Engine`] owns the phase of every output. The caller supplies the
//! current time on each call and forwards the returned [`Action`]s to an
//! emitter, which keeps this module free of I/O and wall-clock access. A
//! binding's trigger may be a controller button or a keyboard key; both arrive
//! in the same [`InputFrame`].
//!
//! Every output of a binding starts when the trigger goes down and runs in
//! parallel with its siblings, so one trigger can spam a key, hold another and
//! play a sequence at the same time.
//!
//! There is one repeating mechanism, not two: an output either holds a key for
//! its duration, or walks a list of steps. A step holds its key for `hold` and
//! then waits `pause` before the next press, so a one-step sequence that
//! repeats is exactly a spam. Both therefore share the same timing rules and
//! the same recovery from a late wake-up.
//!
//! ```text
//! Idle --trigger--> Delayed(initial_delay)
//!                        |
//!                        v
//!                    Down(key, release_at)
//!                        |
//!                        v
//!                 Waiting(next press)        Finished
//! ```
//!
//! ```text
//! X        ──┬─────────────────────────────────────────
//! spam      ─┼┐  ┌──┐  ┌──┐  ┌──┐      repeat: true, one step
//!            │└──┘  └──┘  └──┘  └──┘
//! Z         ─┼────────────────────────────────┘      hold
//! A,B seq   ─┼┐ ┌──┐   ┌┐ ┌──┐   ┌┐ ┌──┐            repeat: true, two steps
//!            │└─┘  └───┘└─┘  └───┘└─┘  └───┘
//! ```

use crate::config::JitterRange;
use crate::input::InputFrame;
use crate::jitter::Jitter;
use crate::keys::{Key, Trigger};

/// A single synthetic input change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Hold the key down.
    Press(Key),
    /// Let the key back up.
    Release(Key),
}

/// One press in an [`Output::Sequence`].
#[derive(Debug, Clone, Copy)]
pub struct Step {
    /// Key emitted for this step.
    pub key: Key,
    /// How long the key stays down.
    pub hold: JitterRange,
    /// How long the key stays up before the next step starts.
    pub pause: JitterRange,
}

/// One output of a binding.
#[derive(Debug, Clone)]
pub enum Output {
    /// Press one key and keep it down.
    Hold {
        /// Key emitted by this output.
        key: Key,
        /// How long the key stays down; `None` waits for the trigger release.
        duration: Option<JitterRange>,
        /// Time between the trigger going down and the press.
        initial_delay: JitterRange,
    },
    /// Play a list of press/hold/pause steps.
    Sequence {
        /// Steps, in order.
        steps: Vec<Step>,
        /// Restart from the first step after the last one finishes.
        repeat: bool,
        /// Time between the trigger going down and the first press.
        initial_delay: JitterRange,
    },
}

impl Output {
    /// Time between the trigger going down and this output's first action.
    const fn initial_delay(&self) -> JitterRange {
        match self {
            Self::Hold { initial_delay, .. } | Self::Sequence { initial_delay, .. } => {
                *initial_delay
            }
        }
    }
}

/// A compiled trigger-to-outputs mapping.
#[derive(Debug, Clone)]
pub struct Binding {
    /// Controller button or keyboard key that drives the mapping.
    pub from: Trigger,
    /// Outputs emitted while the trigger is held. They run in parallel.
    pub outputs: Vec<Output>,
}

/// What to do when a down key comes up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// Press sequence step `index` at `press_at`.
    Step { press_at: u64, index: usize },
    /// Stay idle until the trigger is released and pressed again.
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Nothing held, nothing scheduled.
    Idle,
    /// The trigger is down; the first press is due at `press_at`.
    Delayed { press_at: u64 },
    /// `key` is down and comes up at `release_at` (`None` = on trigger release).
    Down {
        key: Key,
        release_at: Option<u64>,
        next: Next,
    },
    /// Nothing is down; the press of step `step` is due at `press_at`.
    Waiting { press_at: u64, step: usize },
    /// This output is done until the trigger is released.
    Finished,
}

impl Phase {
    /// The key this phase is holding down, if any.
    const fn down_key(self) -> Option<Key> {
        match self {
            Self::Down { key, .. } => Some(key),
            _ => None,
        }
    }

    /// The earliest moment this phase needs attention, if any.
    const fn deadline(self) -> Option<u64> {
        match self {
            Self::Delayed { press_at } | Self::Waiting { press_at, .. } => Some(press_at),
            Self::Down {
                release_at: Some(at),
                ..
            } => Some(at),
            Self::Idle | Self::Down { .. } | Self::Finished => None,
        }
    }
}

/// One output paired with the trigger that drives it and where it lives.
///
/// Flattening the bindings this way keeps every phase in one array, so the
/// engine never has to allocate while running.
#[derive(Debug, Clone, Copy)]
struct Slot {
    from: Trigger,
    binding: usize,
    output: usize,
}

/// Turns held triggers into synthetic press/release cycles.
pub struct Engine {
    bindings: Vec<Binding>,
    slots: Vec<Slot>,
    phases: Vec<Phase>,
    trigger_threshold: u8,
    jitter: Box<dyn Jitter>,
}

impl Engine {
    /// Builds an engine for `bindings`.
    ///
    /// `trigger_threshold` decides when an analog trigger counts as pressed.
    #[must_use]
    pub fn new(bindings: Vec<Binding>, trigger_threshold: u8, jitter: Box<dyn Jitter>) -> Self {
        let mut slots = Vec::new();
        for (binding_index, binding) in bindings.iter().enumerate() {
            for output_index in 0..binding.outputs.len() {
                slots.push(Slot {
                    from: binding.from,
                    binding: binding_index,
                    output: output_index,
                });
            }
        }
        let phases = vec![Phase::Idle; slots.len()];
        Self {
            bindings,
            slots,
            phases,
            trigger_threshold,
            jitter,
        }
    }

    /// The compiled bindings, in configuration order.
    #[must_use]
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Whether any binding needs a controller to be connected.
    ///
    /// Used to keep the "controller not connected" warning out of pure
    /// keyboard setups.
    #[must_use]
    pub fn uses_controller(&self) -> bool {
        self.bindings
            .iter()
            .any(|binding| !binding.from.is_keyboard())
    }

    /// Feeds one polled frame of controller and keyboard state.
    ///
    /// A rising trigger edge starts every output of the binding at once unless
    /// the output asks for an `initial_delay`, so a short tap still produces
    /// one press. A falling edge releases everything that output is holding,
    /// which is what keeps a game from seeing a stuck key.
    pub fn on_input(&mut self, now: u64, frame: &InputFrame) -> Vec<Action> {
        let mut actions = Vec::new();

        for index in 0..self.slots.len() {
            let slot = self.slots[index];
            let phase = self.phases[index];
            let pressed = frame.is_pressed(slot.from, self.trigger_threshold);

            if pressed {
                if phase != Phase::Idle {
                    continue;
                }
                let delay = self.sample(self.output(slot).initial_delay());
                if delay == 0 {
                    if let Some(action) = self.press(index, now, now, 0) {
                        actions.push(action);
                    }
                } else {
                    self.phases[index] = Phase::Delayed {
                        press_at: now.saturating_add(delay),
                    };
                }
            } else {
                if let Some(key) = phase.down_key() {
                    actions.push(Action::Release(key));
                }
                self.phases[index] = Phase::Idle;
            }
        }

        actions
    }

    /// Emits everything that has become due. Safe to call at any frequency.
    pub fn tick(&mut self, now: u64) -> Vec<Action> {
        let mut actions = Vec::new();

        for index in 0..self.slots.len() {
            // A single tick can cover a whole sequence step, so several passes
            // are needed. The bound keeps a late wake-up from bursting.
            for _ in 0..8 {
                match self.phases[index] {
                    // Nothing scheduled: `Finished` waits for the trigger
                    // release, and so does a hold without a duration.
                    Phase::Idle
                    | Phase::Finished
                    | Phase::Down {
                        release_at: None, ..
                    } => break,
                    Phase::Delayed { press_at } => {
                        if now < press_at {
                            break;
                        }
                        if let Some(action) = self.press(index, press_at, now, 0) {
                            actions.push(action);
                        }
                    }
                    Phase::Waiting { press_at, step } => {
                        if now < press_at {
                            break;
                        }
                        if let Some(action) = self.press(index, press_at, now, step) {
                            actions.push(action);
                        }
                    }
                    Phase::Down {
                        key,
                        release_at: Some(release_at),
                        next,
                    } => {
                        if now < release_at {
                            break;
                        }
                        actions.push(Action::Release(key));
                        self.phases[index] = match next {
                            Next::Step {
                                press_at,
                                index: step,
                            } => Phase::Waiting { press_at, step },
                            Next::Finish => Phase::Finished,
                        };
                    }
                }
            }
        }

        actions
    }

    /// Presses the key of slot `index`, logging it at `at`.
    ///
    /// `now` is the current time, used only to resynchronise a sequence whose
    /// next press fell behind by a whole cycle; the schedule itself runs from
    /// `at`, the moment the press belongs to, so a late wake-up does not make
    /// the cadence drift.
    ///
    /// `step` selects the sequence step to play. Returns `None` for an output
    /// that has nothing to press, which only happens for an empty sequence
    /// built outside the configuration layer.
    fn press(&mut self, index: usize, at: u64, now: u64, step: usize) -> Option<Action> {
        /// The schedule of one press, gathered before sampling so that the
        /// immutable borrow of the bindings ends here.
        enum Plan {
            Hold {
                key: Key,
                duration: Option<JitterRange>,
            },
            Step {
                key: Key,
                hold: JitterRange,
                pause: JitterRange,
                following: Option<usize>,
            },
        }

        let slot = self.slots[index];
        let plan = match self.output(slot) {
            Output::Hold { key, duration, .. } => Plan::Hold {
                key: *key,
                duration: *duration,
            },
            Output::Sequence { steps, repeat, .. } => {
                let current = steps.get(step).or_else(|| steps.first())?;

                let following = if step + 1 < steps.len() {
                    Some(step + 1)
                } else if *repeat {
                    Some(0)
                } else {
                    None
                };

                Plan::Step {
                    key: current.key,
                    hold: current.hold,
                    pause: current.pause,
                    following,
                }
            }
        };

        let (key, release_at, next) = match plan {
            Plan::Hold { key, duration } => (
                key,
                duration.map(|duration| at.saturating_add(self.sample(duration))),
                Next::Finish,
            ),
            Plan::Step {
                key,
                hold,
                pause,
                following,
            } => {
                let hold = self.sample(hold);
                let pause = self.sample(pause);
                let release_at = at.saturating_add(hold);

                let next = following.map_or(Next::Finish, |index| {
                    let period = hold.saturating_add(pause);
                    let mut press_at = release_at.saturating_add(pause);
                    if press_at <= now {
                        // We fell behind by a whole cycle; resynchronise from
                        // the recovery point rather than replaying the missed
                        // presses in a burst.
                        press_at = now.saturating_add(period);
                    }
                    Next::Step { press_at, index }
                });

                (key, Some(release_at), next)
            }
        };

        self.phases[index] = Phase::Down {
            key,
            release_at,
            next,
        };

        Some(Action::Press(key))
    }

    /// Earliest pending event, or `None` when nothing is scheduled.
    ///
    /// A `hold` output without a duration has no deadline: it waits for the
    /// trigger release.
    #[must_use]
    pub fn next_deadline(&self) -> Option<u64> {
        self.phases
            .iter()
            .filter_map(|phase| phase.deadline())
            .min()
    }

    /// Releases everything and forgets all state.
    ///
    /// Idempotent, so callers can invoke it on every iteration while emission is
    /// disabled.
    pub fn release_all(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();

        for index in 0..self.slots.len() {
            if let Some(key) = self.phases[index].down_key() {
                actions.push(Action::Release(key));
            }
            self.phases[index] = Phase::Idle;
        }

        actions
    }

    fn output(&self, slot: Slot) -> &Output {
        &self.bindings[slot.binding].outputs[slot.output]
    }

    fn sample(&mut self, range: JitterRange) -> u64 {
        self.jitter.range_inclusive(range.min, range.max)
    }
}
