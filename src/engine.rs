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
//! A `stick` output is the exception: it holds nothing down, and instead turns
//! the current deflection into a pointer or wheel delta on every tick, scaled by
//! the time that actually elapsed.
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
use crate::gamepad::STICK_MAX;
use crate::input::{InputFrame, InputThresholds};
use crate::jitter::Jitter;
use crate::keys::{Key, Trigger};

/// A single synthetic input change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Hold the key down.
    Press(Key),
    /// Let the key back up.
    Release(Key),
    /// Move the pointer by a relative amount, in pixels.
    ///
    /// Relative rather than absolute, so the game's own mouse input and the
    /// synthetic motion add up instead of fighting over the same position.
    MouseMove {
        /// Horizontal pixels, positive to the right.
        dx: i32,
        /// Vertical pixels, positive downwards (the `SendInput` convention).
        dy: i32,
    },
    /// Turn the wheel by a number of notches.
    Scroll {
        /// Horizontal notches, positive to the right.
        horizontal: i32,
        /// Vertical notches, positive upwards.
        vertical: i32,
    },
}

impl Action {
    /// Whether this action is a relative motion rather than an edge.
    ///
    /// Motion is dropped rather than queued when emission fails: replaying a
    /// stale delta would jerk the pointer by the distance it was meant to travel
    /// while the desktop was unavailable.
    #[must_use]
    pub const fn is_motion(self) -> bool {
        matches!(self, Self::MouseMove { .. } | Self::Scroll { .. })
    }
}

/// How a stick output is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickTarget {
    /// Move the pointer.
    Mouse,
    /// Turn the wheel.
    Scroll,
}

/// Which axes of the stick drive the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickAxes {
    /// Horizontal and vertical.
    Both,
    /// Horizontal only.
    Horizontal,
    /// Vertical only.
    Vertical,
}

/// A continuous output driven by a stick.
///
/// The stick itself is not stored here: it comes from the binding's trigger,
/// which is where the configuration declares it. Keeping one source of truth
/// means a `stick` output can never read a different stick than its own binding.
#[derive(Debug, Clone, Copy)]
pub struct StickOutput {
    /// What the deflection is applied to.
    pub target: StickTarget,
    /// Which axes are read.
    pub axes: StickAxes,
    /// Full-scale rate: pixels per second, or notches per second.
    pub sensitivity: u32,
    /// Flip the horizontal direction.
    pub invert_x: bool,
    /// Flip the vertical direction.
    pub invert_y: bool,
    /// Time between the stick leaving the deadzone and the first motion.
    pub initial_delay: JitterRange,
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
    /// Turn stick deflection into pointer motion or wheel scrolling.
    Stick(StickOutput),
}

impl Output {
    /// Time between the trigger going down and this output's first action.
    const fn initial_delay(&self) -> JitterRange {
        match self {
            Self::Hold { initial_delay, .. }
            | Self::Sequence { initial_delay, .. }
            | Self::Stick(StickOutput { initial_delay, .. }) => *initial_delay,
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
    /// A stick output is active; motion accumulates against `last_at`.
    Axis {
        /// When the previous tick was, so motion follows elapsed time rather
        /// than the poll cadence.
        last_at: u64,
        /// Sub-pixel remainder, kept so a slow rate still moves.
        carry_x: i64,
        /// Sub-pixel remainder of the vertical axis.
        carry_y: i64,
    },
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
    ///
    /// An [`Phase::Axis`] has none: it produces motion on every tick the loop
    /// makes anyway, and the elapsed-time arithmetic means a late tick loses no
    /// distance. Waking up extra often would only add jitter.
    const fn deadline(self) -> Option<u64> {
        match self {
            Self::Delayed { press_at } | Self::Waiting { press_at, .. } => Some(press_at),
            Self::Down {
                release_at: Some(at),
                ..
            } => Some(at),
            Self::Idle | Self::Axis { .. } | Self::Down { .. } | Self::Finished => None,
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

/// Turns held triggers into synthetic press/release cycles and pointer motion.
pub struct Engine {
    bindings: Vec<Binding>,
    slots: Vec<Slot>,
    phases: Vec<Phase>,
    thresholds: InputThresholds,
    /// The most recent frame, kept so [`Self::tick`] can apply motion without
    /// the caller having to repeat it. The runner polls once per iteration and
    /// then ticks, so this is always the current frame.
    frame: InputFrame,
    jitter: Box<dyn Jitter>,
}

impl Engine {
    /// Builds an engine for `bindings`.
    ///
    /// `thresholds` decide when an analog trigger counts as pressed and how far
    /// a stick has to move before it counts as pushed.
    #[must_use]
    pub fn new(
        bindings: Vec<Binding>,
        thresholds: InputThresholds,
        jitter: Box<dyn Jitter>,
    ) -> Self {
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
            thresholds,
            frame: InputFrame::default(),
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

    /// Whether any binding produces continuous motion.
    ///
    /// Motion is emitted from the poll itself, so no key needs to be held; this
    /// only reports the capability, which keeps `--dry-run` honest.
    #[must_use]
    pub fn uses_motion(&self) -> bool {
        self.bindings.iter().any(|binding| {
            binding
                .outputs
                .iter()
                .any(|output| matches!(output, Output::Stick(_)))
        })
    }

    /// Feeds one polled frame of controller and keyboard state.
    ///
    /// A rising trigger edge starts every output of the binding at once unless
    /// the output asks for an `initial_delay`, so a short tap still produces
    /// one press. A falling edge releases everything that output is holding,
    /// which is what keeps a game from seeing a stuck key.
    pub fn on_input(&mut self, now: u64, frame: &InputFrame) -> Vec<Action> {
        let mut actions = Vec::new();
        self.frame = *frame;

        for index in 0..self.slots.len() {
            let slot = self.slots[index];
            let phase = self.phases[index];
            let pressed = frame.is_pressed(slot.from, self.thresholds);

            if pressed {
                if phase != Phase::Idle {
                    continue;
                }
                let delay = self.sample(self.output(slot).initial_delay());
                if delay == 0 {
                    self.edge(index, now, now, &mut actions);
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
    ///
    /// Motion is emitted here rather than in `on_input` so a stick output
    /// advances on every loop iteration against the time that actually elapsed,
    /// which keeps the pointer speed independent of the poll cadence.
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
                    Phase::Axis { .. } => {
                        self.motion(index, now, &mut actions);
                        break;
                    }
                    Phase::Delayed { press_at } => {
                        if now < press_at {
                            break;
                        }
                        // `edge` so a delayed `stick` output starts its clock at
                        // the moment the delay expired, not at the trigger.
                        self.edge(index, press_at, now, &mut actions);
                    }
                    Phase::Waiting { press_at, step } => {
                        if now < press_at {
                            break;
                        }
                        self.press_action(index, press_at, now, step, &mut actions);
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

    /// Starts whatever output `index` owns, at the moment its trigger went down.
    fn edge(&mut self, index: usize, at: u64, now: u64, actions: &mut Vec<Action>) {
        if matches!(self.output(self.slots[index]), Output::Stick(_)) {
            self.phases[index] = Phase::Axis {
                last_at: at,
                carry_x: 0,
                carry_y: 0,
            };
            self.motion(index, now, actions);
            return;
        }
        self.press_action(index, at, now, 0, actions);
    }

    /// Advances the stick output of slot `index` by the time that has passed.
    ///
    /// Rates are applied as elapsed time times rate, so the distance travelled
    /// does not depend on how often the loop happens to wake up. Sub-unit
    /// remainders are carried between ticks, so even a rate of one pixel per
    /// second eventually moves the pointer instead of rounding to nothing.
    fn motion(&mut self, index: usize, now: u64, actions: &mut Vec<Action>) {
        let Phase::Axis {
            last_at,
            carry_x,
            carry_y,
        } = self.phases[index]
        else {
            return;
        };

        let Output::Stick(config) = self.output(self.slots[index]) else {
            return;
        };
        let config = *config;
        let Trigger::StickAxis(stick) = self.slots[index].from else {
            // Only a stick axis can drive a stick output; the configuration
            // rejects anything else.
            return;
        };
        let elapsed = now.saturating_sub(last_at);
        self.phases[index] = Phase::Axis {
            last_at: now,
            carry_x,
            carry_y,
        };

        let Some((x, y)) = self.frame.stick_deflection(stick, self.thresholds) else {
            return;
        };

        let (x, y) = apply_axes(config, x, y);
        let rate = i64::from(config.sensitivity);
        let elapsed = i64::try_from(elapsed).unwrap_or(i64::MAX);

        // `x * rate * elapsed / (STICK_MAX * 1000)`, then clamped: the product
        // of a full-scale axis, a large sensitivity and a long stall would
        // otherwise overflow, and a huge jump is never desirable.
        let step = |value: i64, carry: i64| {
            let scaled = value
                .saturating_mul(rate)
                .saturating_mul(elapsed)
                .saturating_add(carry);
            let divisor = i64::from(STICK_MAX) * 1_000;
            (scaled.div_euclid(divisor), scaled.rem_euclid(divisor))
        };
        let (dx, carry_x) = step(x, carry_x);
        let (dy, carry_y) = step(y, carry_y);
        self.phases[index] = Phase::Axis {
            last_at: now,
            carry_x,
            carry_y,
        };

        let dx = clamp_motion(dx);
        let dy = clamp_motion(dy);
        if dx == 0 && dy == 0 {
            return;
        }

        actions.push(match config.target {
            // The stick's `y` is positive upwards, while a mouse or wheel delta
            // is positive downwards, so pushing the stick up must send a
            // negative `dy`. Without the flip the pointer runs against the stick.
            StickTarget::Mouse => Action::MouseMove { dx, dy: -dy },
            StickTarget::Scroll => Action::Scroll {
                horizontal: dx,
                vertical: dy,
            },
        });
    }

    /// Presses the key of slot `index` and appends the action, if there is one.
    ///
    /// The `Option` from [`Self::press`] is only `None` for an empty sequence
    /// built outside the configuration layer, which must not panic.
    fn press_action(
        &mut self,
        index: usize,
        at: u64,
        now: u64,
        step: usize,
        actions: &mut Vec<Action>,
    ) {
        if let Some(action) = self.press(index, at, now, step) {
            actions.push(action);
        }
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
            // A stick output has no key to press; `edge` and `tick` route it to
            // the axis phase instead of ever reaching here.
            Output::Stick(_) => return None,
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

/// Applies the configured axis filter and inversions to a deflection.
fn apply_axes(config: StickOutput, x: i32, y: i32) -> (i64, i64) {
    let x = match config.axes {
        StickAxes::Both | StickAxes::Horizontal => x,
        StickAxes::Vertical => 0,
    };
    let y = match config.axes {
        StickAxes::Both | StickAxes::Vertical => y,
        StickAxes::Horizontal => 0,
    };
    let x = if config.invert_x { -x } else { x };
    let y = if config.invert_y { -y } else { y };
    (i64::from(x), i64::from(y))
}

/// The largest movement a single tick may produce, in pixels or notches.
///
/// A loop that was stalled (a dragged window, a suspended laptop) must not fling
/// the pointer across the screen when it resumes.
const MAX_MOTION_PER_TICK: i32 = 4_096;

fn clamp_motion(value: i64) -> i32 {
    i32::try_from(value.clamp(
        -i64::from(MAX_MOTION_PER_TICK),
        i64::from(MAX_MOTION_PER_TICK),
    ))
    .unwrap_or_default()
}
