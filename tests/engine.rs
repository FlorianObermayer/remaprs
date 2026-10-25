//! Behaviour of the output state machine, driven by a simulated millisecond
//! clock.
//!
//! The clock advances one millisecond at a time, so every assertion about a
//! timestamp is exact rather than approximate.

use remaprs::config::JitterRange;
use remaprs::engine::{Action, Binding, Engine, Output, Step, StickAxes, StickOutput, StickTarget};
use remaprs::gamepad::{PadState, StickState, bits};
use remaprs::input::{InputFrame, InputThresholds, KeyboardState};
use remaprs::jitter::SeededJitter;
use remaprs::keys::{Button, Key, KeyboardKey, Stick, StickDirection, Trigger};

const THRESHOLD: u8 = 30;
/// Trigger threshold plus the default stick deadzone, which no test here uses.
const THRESHOLDS: InputThresholds = InputThresholds::with_default_deadzone(THRESHOLD);
/// Randomized hold range used by the jittered engine.
const HOLD: JitterRange = JitterRange::new(15, 35);
/// Randomized pause range used by the jittered engine.
const PAUSE: JitterRange = JitterRange::new(5, 75);

/// Virtual-key code of `Q`; any letter works, this one is used by the tests.
const KEY_Q: u16 = 0x51;

fn key(name: &str) -> Key {
    match Key::parse(name) {
        Ok(key) => key,
        Err(error) => panic!("test key `{name}` is invalid: {error}"),
    }
}

fn spacebar() -> Key {
    Key::Keyboard(KeyboardKey::new(0x20, false))
}

fn q() -> KeyboardKey {
    KeyboardKey::new(KEY_Q, false)
}

/// A one-step repeating sequence: `period` ms press-to-press, of which the key
/// is held for `hold` ms. This is the concise way to write a plain spam.
fn spam(key: Key, period: u64, hold: u64) -> Output {
    assert!(hold <= period, "the hold must fit inside the period");
    Output::Sequence {
        steps: vec![Step {
            key,
            hold: JitterRange::fixed(hold),
            pause: JitterRange::fixed(period - hold),
        }],
        repeat: true,
        initial_delay: JitterRange::fixed(0),
    }
}

/// A `hold` output that stays down until the trigger is released.
fn hold(key: Key) -> Output {
    Output::Hold {
        key,
        duration: None,
        initial_delay: JitterRange::fixed(0),
    }
}

/// A `hold` output with a fixed duration.
fn hold_for(key: Key, duration: u64) -> Output {
    Output::Hold {
        key,
        duration: Some(JitterRange::fixed(duration)),
        initial_delay: JitterRange::fixed(0),
    }
}

/// A sequence of `(key, hold, pause)` steps.
fn sequence(steps: &[(&str, u64, u64)], repeat: bool) -> Output {
    Output::Sequence {
        steps: steps
            .iter()
            .map(|(name, hold, pause)| Step {
                key: key(name),
                hold: JitterRange::fixed(*hold),
                pause: JitterRange::fixed(*pause),
            })
            .collect(),
        repeat,
        initial_delay: JitterRange::fixed(0),
    }
}

fn binding(from: Trigger, key: Key, period: u64, hold: u64) -> Binding {
    Binding {
        from,
        outputs: vec![spam(key, period, hold)],
    }
}

fn engine_with(outputs: Vec<Output>) -> Engine {
    Engine::new(
        vec![Binding {
            from: Trigger::Button(Button::A),
            outputs,
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    )
}

/// Engine with a perfectly regular cadence, so timestamps are predictable.
fn fixed_engine(period: u64, hold: u64) -> Engine {
    Engine::new(
        vec![binding(
            Trigger::Button(Button::A),
            spacebar(),
            period,
            hold,
        )],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    )
}

/// Like [`fixed_engine`], but the first press waits `delay` ms.
fn delayed_engine(period: u64, hold: u64, delay: u64) -> Engine {
    let mut binding = binding(Trigger::Button(Button::A), spacebar(), period, hold);
    binding.outputs[0] = Output::Sequence {
        steps: vec![Step {
            key: spacebar(),
            hold: JitterRange::fixed(hold),
            pause: JitterRange::fixed(period - hold),
        }],
        repeat: true,
        initial_delay: JitterRange::fixed(delay),
    };
    Engine::new(vec![binding], THRESHOLDS, Box::new(SeededJitter::new(1)))
}

/// Engine with randomized ranges.
fn jittered_engine(seed: u64) -> Engine {
    Engine::new(
        vec![Binding {
            from: Trigger::Button(Button::A),
            outputs: vec![Output::Sequence {
                steps: vec![Step {
                    key: spacebar(),
                    hold: HOLD,
                    pause: PAUSE,
                }],
                repeat: true,
                initial_delay: JitterRange::fixed(0),
            }],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(seed)),
    )
}

fn held() -> InputFrame {
    InputFrame::pad(PadState::button(bits::A))
}

fn held_x() -> InputFrame {
    InputFrame::pad(PadState::button(bits::X))
}

fn released() -> InputFrame {
    InputFrame::pad(PadState::default())
}

fn record(log: &mut Vec<(u64, Action)>, at: u64, actions: Vec<Action>) {
    log.extend(actions.into_iter().map(|action| (at, action)));
}

/// Ticks the engine one millisecond at a time and returns the final clock value.
fn run(engine: &mut Engine, from: u64, millis: u64, log: &mut Vec<(u64, Action)>) -> u64 {
    let mut now = from;
    for _ in 0..millis {
        now += 1;
        record(log, now, engine.tick(now));
    }
    now
}

fn presses(log: &[(u64, Action)]) -> Vec<u64> {
    log.iter()
        .filter_map(|(at, action)| matches!(action, Action::Press(_)).then_some(*at))
        .collect()
}

fn releases(log: &[(u64, Action)]) -> Vec<u64> {
    log.iter()
        .filter_map(|(at, action)| matches!(action, Action::Release(_)).then_some(*at))
        .collect()
}

#[test]
fn presses_immediately_when_the_trigger_goes_down() {
    let mut engine = fixed_engine(50, 15);

    assert_eq!(
        engine.on_input(0, &held()),
        vec![Action::Press(spacebar())],
        "the trigger edge must fire at once, not one interval later"
    );
    // The key comes up after `hold`, so the next deadline is the release.
    assert_eq!(engine.next_deadline(), Some(15));
}

#[test]
fn holds_the_first_press_back_by_the_initial_delay() {
    let mut engine = delayed_engine(50, 15, 30);

    assert!(
        engine.on_input(0, &held()).is_empty(),
        "an explicit delay must not fire immediately"
    );
    assert_eq!(engine.next_deadline(), Some(30));

    let mut log = Vec::new();
    run(&mut engine, 0, 29, &mut log);
    assert!(log.is_empty(), "fired too early: {log:?}");

    record(&mut log, 30, engine.tick(30));
    assert_eq!(log, vec![(30, Action::Press(spacebar()))]);
}

#[test]
fn keeps_a_regular_cadence() {
    let mut engine = fixed_engine(50, 15);
    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 400, &mut log);

    // The trigger fires at 0 ms, then every 50 ms after that, *not* every 50 + 15.
    assert_eq!(
        presses(&log),
        vec![0, 50, 100, 150, 200, 250, 300, 350, 400]
    );
    // Each press is released 15 ms later, inside its own period.
    assert_eq!(releases(&log), vec![15, 65, 115, 165, 215, 265, 315, 365]);
}

#[test]
fn the_next_press_is_scheduled_from_the_previous_press() {
    let mut engine = fixed_engine(50, 15);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 1_000, &mut log);

    let presses = presses(&log);
    for pair in presses.windows(2) {
        assert_eq!(pair[1] - pair[0], 50, "cadence drifted");
    }

    let releases = releases(&log);
    assert_eq!(presses.len(), releases.len() + 1, "ends while still held");
    for (press, release) in presses.iter().zip(releases.iter()) {
        assert_eq!(release - press, 15, "hold length changed");
    }
}

#[test]
fn releasing_the_button_stops_the_sequence() {
    let mut engine = fixed_engine(50, 15);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    assert_eq!(presses(&log), vec![0]);

    run(&mut engine, 0, 10, &mut log);

    // Let go while the key is down: the release must be immediate.
    record(&mut log, 11, engine.on_input(11, &released()));
    assert_eq!(log.last(), Some(&(11, Action::Release(spacebar()))));
    assert!(engine.next_deadline().is_none());

    let before_idle = log.len();
    run(&mut engine, 11, 500, &mut log);
    assert_eq!(log.len(), before_idle, "kept emitting after release");
}

#[test]
fn a_short_tap_still_presses_once() {
    let mut engine = fixed_engine(50, 15);

    assert_eq!(engine.on_input(0, &held()), vec![Action::Press(spacebar())]);
    assert_eq!(
        engine.on_input(5, &released()),
        vec![Action::Release(spacebar())],
        "letting go emits the key-up for the press that already happened"
    );
    assert!(engine.next_deadline().is_none());

    let mut log = Vec::new();
    run(&mut engine, 5, 200, &mut log);
    assert!(log.is_empty(), "a tap must not start a sequence: {log:?}");
}

#[test]
fn releasing_between_presses_emits_only_the_release() {
    let mut engine = fixed_engine(50, 15);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 20, &mut log);
    assert_eq!(presses(&log), vec![0]);
    assert_eq!(releases(&log), vec![15]);

    // The key is already up, so letting go produces no extra action.
    assert_eq!(engine.on_input(21, &released()), [] as [Action; 0]);
}

#[test]
fn randomized_cadence_stays_within_the_ranges() {
    let mut engine = jittered_engine(0xC0FF_EE01);
    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 5_000, &mut log);

    let presses = presses(&log);
    let releases = releases(&log);
    assert!(presses.len() > 20, "only {} presses in 5 s", presses.len());

    // The first press lands on the trigger edge.
    assert_eq!(presses[0], 0, "the first press must be immediate");

    // One cycle is hold + pause, because the pause starts at the release.
    for pair in presses.windows(2) {
        let gap = pair[1] - pair[0];
        let low = HOLD.min + PAUSE.min;
        let high = HOLD.max + PAUSE.max;
        assert!(
            (low..=high).contains(&gap),
            "press gap {gap} ms is outside {low}-{high} ms"
        );
    }

    for (press, release) in presses.iter().zip(releases.iter()) {
        let hold = release - press;
        assert!(
            (HOLD.min..=HOLD.max + 1).contains(&hold),
            "hold {hold} ms is outside {HOLD}"
        );
    }
}

#[test]
fn every_press_is_released_before_the_next_starts() {
    let mut engine = jittered_engine(7);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 3_000, &mut log);

    let mut down = false;
    for (at, action) in &log {
        match action {
            Action::Press(_) => {
                assert!(!down, "pressed twice in a row at {at} ms");
                down = true;
            }
            Action::Release(_) => {
                assert!(down, "released without a press at {at} ms");
                down = false;
            }
            Action::MouseMove { .. } | Action::Scroll { .. } => {
                panic!("a key sequence must not emit motion: {action:?} at {at} ms")
            }
        }
    }
}

#[test]
fn release_all_clears_pending_state() {
    let mut engine = fixed_engine(50, 15);
    engine.on_input(0, &held());

    assert_eq!(engine.release_all(), vec![Action::Release(spacebar())]);
    assert!(
        engine.release_all().is_empty(),
        "release_all must be idempotent"
    );
    assert!(engine.next_deadline().is_none());
}

#[test]
fn release_all_before_the_first_press_is_a_no_op() {
    let mut engine = delayed_engine(50, 15, 30);
    engine.on_input(0, &held());

    // The key was never pressed, so there is nothing to release.
    assert_eq!(engine.release_all(), [] as [Action; 0]);
    assert!(engine.next_deadline().is_none());
}

#[test]
fn analog_triggers_respect_the_threshold() {
    let mut engine = Engine::new(
        vec![binding(Trigger::Button(Button::Rt), spacebar(), 50, 15)],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    let just_below = InputFrame::pad(PadState {
        buttons: 0,
        left_trigger: 0,
        right_trigger: THRESHOLD - 1,
        ..PadState::default()
    });
    assert_eq!(engine.on_input(0, &just_below), [] as [Action; 0]);
    assert!(engine.next_deadline().is_none());

    let at_threshold = InputFrame::pad(PadState {
        buttons: 0,
        left_trigger: 0,
        right_trigger: THRESHOLD,
        ..PadState::default()
    });
    assert_eq!(
        engine.on_input(1, &at_threshold),
        vec![Action::Press(spacebar())]
    );
    assert_eq!(engine.next_deadline(), Some(16), "released after 15 ms");
}

#[test]
fn a_keyboard_trigger_fires_without_a_controller() {
    let mut engine = Engine::new(
        vec![binding(Trigger::Keyboard(q()), spacebar(), 50, 15)],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    let q_down = InputFrame::default().with_keyboard(KeyboardState::from_keys([q()]));
    assert_eq!(
        engine.on_input(0, &q_down),
        vec![Action::Press(spacebar())],
        "a keyboard binding must not need a pad"
    );

    let released = InputFrame::default().with_keyboard(KeyboardState::default());
    assert_eq!(
        engine.on_input(5, &released),
        vec![Action::Release(spacebar())]
    );
}

#[test]
fn bindings_are_independent() {
    let mut engine = Engine::new(
        vec![
            binding(Trigger::Button(Button::A), spacebar(), 50, 5),
            binding(
                Trigger::Button(Button::B),
                Key::parse("E").expect("E is a valid key"),
                20,
                5,
            ),
        ],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    let both = InputFrame::pad(PadState::button(bits::A | bits::B));
    assert_eq!(
        engine.on_input(0, &both),
        vec![
            Action::Press(spacebar()),
            Action::Press(Key::parse("E").expect("valid"))
        ],
        "both bindings fire on the same poll"
    );

    assert_eq!(
        engine.tick(5),
        vec![
            Action::Release(spacebar()),
            Action::Release(Key::parse("E").expect("valid"))
        ]
    );

    // Let go of A before its next press was due; B must be unaffected.
    let only_b = InputFrame::pad(PadState::button(bits::B));
    assert!(
        engine.on_input(6, &only_b).is_empty(),
        "A's key is already up, so nothing is released"
    );
    assert_eq!(engine.next_deadline(), Some(20), "B's cadence continues");

    assert_eq!(
        engine.tick(20),
        vec![Action::Press(Key::parse("E").expect("valid"))]
    );
    assert_eq!(
        engine.next_deadline(),
        Some(25),
        "B holds E for 5 ms from the press"
    );
}

#[test]
fn a_late_wake_up_does_not_replay_missed_presses() {
    // A delay keeps the first press pending, so the stall happens mid-wait.
    let mut engine = delayed_engine(50, 15, 50);
    engine.on_input(0, &held());

    // The loop was blocked for five seconds.
    let actions = engine.tick(5_000);

    assert_eq!(
        actions,
        vec![Action::Press(spacebar()), Action::Release(spacebar())],
        "a stall must produce one cycle, not a burst"
    );
    // The cadence restarts from the recovery point, not from the past.
    assert_eq!(engine.next_deadline(), Some(5_050));
}

#[test]
fn a_late_wake_up_does_not_replay_a_multi_step_sequence() {
    let mut engine = engine_with(vec![sequence(&[("A", 20, 30), ("B", 20, 30)], true)]);

    record(&mut Vec::new(), 0, engine.on_input(0, &held()));

    // The loop was blocked for five seconds, long after both steps were due.
    let actions = engine.tick(5_000);

    assert_eq!(
        actions,
        vec![
            Action::Release(key("A")),
            Action::Press(key("B")),
            Action::Release(key("B")),
        ],
        "one catch-up cycle, not a replay of the whole sequence"
    );
    // One 50 ms cycle after the recovery point, not an instant second burst.
    assert_eq!(engine.next_deadline(), Some(5_050));
}

#[test]
fn runs_are_reproducible_for_a_given_seed() {
    let simulate = |seed: u64| {
        let mut engine = jittered_engine(seed);
        let mut log = Vec::new();
        record(&mut log, 0, engine.on_input(0, &held()));
        run(&mut engine, 0, 600, &mut log);
        log
    };

    assert_eq!(simulate(1234), simulate(1234));
    assert_ne!(simulate(1234), simulate(4321));
}

#[test]
fn one_trigger_can_spam_one_key_while_holding_another() {
    let mut engine = Engine::new(
        vec![Binding {
            from: Trigger::Button(Button::X),
            outputs: vec![
                spam(Key::parse("Y").expect("valid key"), 50, 15),
                hold(Key::parse("Z").expect("valid key")),
            ],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    let x_down = InputFrame::pad(PadState::button(bits::X));
    assert_eq!(
        engine.on_input(0, &x_down),
        vec![
            Action::Press(Key::parse("Y").expect("valid")),
            Action::Press(Key::parse("Z").expect("valid")),
        ],
        "both outputs fire on the trigger edge"
    );

    // Only the spamming output cycles; the held key has no deadline.
    let mut log = Vec::new();
    run(&mut engine, 0, 160, &mut log);
    assert_eq!(presses(&log), vec![50, 100, 150], "only Y repeats: {log:?}");
    assert_eq!(
        releases(&log),
        vec![15, 65, 115],
        "Z is never released while X is held"
    );

    // Letting go releases everything that is down.
    let x_up = InputFrame::pad(PadState::default());
    assert_eq!(
        engine.on_input(160, &x_up),
        vec![
            Action::Release(Key::parse("Y").expect("valid")),
            Action::Release(Key::parse("Z").expect("valid")),
        ],
        "the held key must come up with the trigger"
    );
    assert!(engine.next_deadline().is_none());
}

#[test]
fn a_hold_output_does_not_fire_the_trigger_again_while_held() {
    let mut engine = Engine::new(
        vec![Binding {
            from: Trigger::Button(Button::X),
            outputs: vec![hold(Key::parse("Z").expect("valid key"))],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    assert_eq!(
        engine.on_input(0, &held_x()),
        vec![Action::Press(Key::parse("Z").expect("valid"))]
    );

    // Repeated frames while the trigger stays down must not press again.
    let mut log = Vec::new();
    for at in 1..=100 {
        record(&mut log, at, engine.on_input(at, &held_x()));
    }
    assert!(log.is_empty(), "the hold output pressed twice: {log:?}");
    assert!(
        engine.next_deadline().is_none(),
        "a held key has no deadline"
    );
}

#[test]
fn a_hold_output_is_released_by_release_all() {
    let mut engine = Engine::new(
        vec![Binding {
            from: Trigger::Button(Button::X),
            outputs: vec![hold(Key::parse("Z").expect("valid key"))],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    engine.on_input(0, &held_x());

    assert_eq!(
        engine.release_all(),
        vec![Action::Release(Key::parse("Z").expect("valid"))],
        "so a suspend or exit cannot leave the key stuck"
    );
    assert_eq!(engine.release_all(), [] as [Action; 0]);
}

#[test]
fn a_hold_with_a_duration_releases_itself() {
    let mut engine = engine_with(vec![hold_for(key("Z"), 200)]);

    assert_eq!(engine.on_input(0, &held()), vec![Action::Press(key("Z"))]);
    assert_eq!(engine.next_deadline(), Some(200));

    let mut log = Vec::new();
    run(&mut engine, 0, 190, &mut log);
    assert!(log.is_empty(), "released too early: {log:?}");

    record(&mut log, 200, engine.tick(200));
    assert_eq!(log, vec![(200, Action::Release(key("Z")))]);

    // The trigger is still down, but the hold is a one-shot per press.
    assert!(engine.next_deadline().is_none());
    let quiet = engine.tick(500);
    assert_eq!(quiet, [] as [Action; 0]);
}

#[test]
fn a_sequence_plays_every_step_then_finishes() {
    // One pass: A held 30 ms, pause 70 ms, B held 50 ms, pause 100 ms.
    let mut engine = engine_with(vec![sequence(&[("A", 30, 70), ("B", 50, 100)], false)]);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 400, &mut log);

    assert_eq!(
        log,
        vec![
            (0, Action::Press(key("A"))),
            (30, Action::Release(key("A"))),
            (100, Action::Press(key("B"))),
            (150, Action::Release(key("B"))),
        ],
        "the sequence must stop after the last step"
    );
    assert!(engine.next_deadline().is_none());
}

#[test]
fn a_looping_sequence_restarts_after_the_last_pause() {
    // A: hold 30, pause 20; B: hold 40, pause 60. One cycle is 150 ms.
    let mut engine = engine_with(vec![sequence(&[("A", 30, 20), ("B", 40, 60)], true)]);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 310, &mut log);

    assert_eq!(
        presses(&log),
        vec![0, 50, 150, 200, 300],
        "the sequence restarts from step one"
    );
    assert_eq!(
        log.iter()
            .filter_map(|(at, action)| matches!(action, Action::Release(_)).then_some(*at))
            .collect::<Vec<_>>(),
        vec![30, 90, 180, 240]
    );
    assert!(
        matches!(
            log.iter().find(|(at, _)| *at == 150),
            Some((_, Action::Press(_)))
        ),
        "{log:?}"
    );
}

#[test]
fn a_sequence_can_reuse_a_key_across_steps() {
    let mut engine = engine_with(vec![sequence(
        &[("A", 20, 10), ("B", 20, 10), ("A", 20, 10)],
        false,
    )]);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 200, &mut log);

    assert_eq!(presses(&log), vec![0, 30, 60], "{log:?}");
    assert_eq!(
        log.iter()
            .filter_map(|(_, action)| match action {
                Action::Press(key) => Some(("press", *key)),
                Action::Release(_) | Action::MouseMove { .. } | Action::Scroll { .. } => None,
            })
            .count(),
        3
    );
}

#[test]
fn a_sequence_runs_in_parallel_with_a_hold() {
    // X plays a two-step sequence while keeping Z down for 500 ms.
    let mut engine = engine_with(vec![
        sequence(&[("A", 20, 10), ("B", 20, 10)], false),
        hold_for(key("Z"), 500),
    ]);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 600, &mut log);

    let events: Vec<Action> = log.iter().map(|(_, action)| *action).collect();
    assert_eq!(
        events[0],
        Action::Press(key("A")),
        "sequence and hold start together"
    );
    assert_eq!(
        log.iter()
            .filter(|(_, action)| *action == Action::Press(key("Z")))
            .count(),
        1
    );
    assert_eq!(
        log.iter()
            .filter(|(_, action)| *action == Action::Release(key("Z")))
            .count(),
        1
    );
    assert_eq!(
        log.last(),
        Some(&(500, Action::Release(key("Z")))),
        "the hold outlives the shorter sequence"
    );
}

#[test]
fn releasing_the_trigger_aborts_a_sequence_mid_step() {
    let mut engine = engine_with(vec![sequence(&[("A", 100, 50), ("B", 100, 50)], true)]);

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held()));
    run(&mut engine, 0, 40, &mut log);
    assert_eq!(presses(&log), vec![0]);

    // Let go while step A is still held: it must come up at once.
    record(&mut log, 41, engine.on_input(41, &released()));
    assert_eq!(log.last(), Some(&(41, Action::Release(key("A")))));
    assert!(engine.next_deadline().is_none());

    let before = log.len();
    run(&mut engine, 41, 500, &mut log);
    assert_eq!(log.len(), before, "the sequence kept running: {log:?}");
}

#[test]
fn outputs_of_a_binding_can_use_different_modes_and_timings() {
    let mut engine = Engine::new(
        vec![Binding {
            from: Trigger::Button(Button::X),
            outputs: vec![
                spam(Key::parse("Y").expect("valid key"), 40, 10),
                spam(Key::parse("B").expect("valid key"), 80, 20),
            ],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &held_x()));
    run(&mut engine, 0, 80, &mut log);

    // Y runs at half the period of B, so it has twice as many presses.
    let y_presses = log
        .iter()
        .filter(|(_, action)| *action == Action::Press(Key::parse("Y").expect("valid")))
        .count();
    let b_presses = log
        .iter()
        .filter(|(_, action)| *action == Action::Press(Key::parse("B").expect("valid")))
        .count();
    assert_eq!((y_presses, b_presses), (3, 2), "{log:?}");
}

#[test]
fn the_initial_delay_is_sampled_per_trigger_press() {
    let mut engine = Engine::new(
        vec![Binding {
            from: Trigger::Button(Button::A),
            outputs: vec![Output::Sequence {
                steps: vec![Step {
                    key: spacebar(),
                    hold: JitterRange::fixed(10),
                    pause: JitterRange::fixed(40),
                }],
                repeat: true,
                initial_delay: JitterRange::new(0, 40),
            }],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(99)),
    );

    // With a range starting at zero, some trigger edges must fire at once and
    // some must wait; the point is that neither is hard-coded.
    let mut immediate = 0;
    let mut delayed = 0;
    for step in 0..40 {
        let at = step * 200;
        if engine.on_input(at, &held()).is_empty() {
            delayed += 1;
        } else {
            immediate += 1;
        }
        engine.on_input(at + 100, &released());
        engine.release_all();
    }

    assert!(immediate > 0, "no trigger edge fired immediately");
    assert!(delayed > 0, "no trigger edge used the delay");
}

/// An engine with one `stick` output on the right stick.
fn stick_engine(output: StickOutput, delay: u64) -> Engine {
    Engine::new(
        vec![Binding {
            from: Trigger::StickAxis(Stick::Right),
            outputs: vec![Output::Stick(StickOutput {
                initial_delay: JitterRange::fixed(delay),
                ..output
            })],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    )
}

/// A mouse output at full scale on one axis.
fn mouse_stick(sensitivity: u32, axes: StickAxes) -> StickOutput {
    StickOutput {
        target: StickTarget::Mouse,
        axes,
        sensitivity,
        invert_x: false,
        invert_y: false,
        initial_delay: JitterRange::fixed(0),
    }
}

/// A frame with the right stick pushed fully right.
fn right_stick_right() -> InputFrame {
    InputFrame::pad(PadState {
        right_stick: StickState::new(i16::MAX, 0),
        ..PadState::default()
    })
}

/// A frame with the right stick centred.
fn right_stick_centred() -> InputFrame {
    InputFrame::pad(PadState::default())
}

#[test]
fn a_stick_moves_the_pointer_in_proportion_to_elapsed_time() {
    let mut engine = stick_engine(mouse_stick(1_000, StickAxes::Both), 0);
    engine.on_input(0, &right_stick_right());

    // 1000 px/s for 30 ms is 30 px, independent of how often we tick.
    assert_eq!(engine.tick(10), vec![Action::MouseMove { dx: 10, dy: 0 }]);
    assert_eq!(engine.tick(20), vec![Action::MouseMove { dx: 10, dy: 0 }]);
    assert_eq!(engine.tick(30), vec![Action::MouseMove { dx: 10, dy: 0 }]);
}

#[test]
fn the_pointer_distance_does_not_depend_on_the_tick_cadence() {
    // The same 100 ms of travel, sampled at two different rates.
    let mut fast = stick_engine(mouse_stick(500, StickAxes::Both), 0);
    fast.on_input(0, &right_stick_right());
    let mut fast_total = 0;
    for step in 1..=100 {
        for action in fast.tick(step) {
            match action {
                Action::MouseMove { dx, .. } => fast_total += dx,
                other => panic!("expected motion, got {other:?}"),
            }
        }
    }

    let mut slow = stick_engine(mouse_stick(500, StickAxes::Both), 0);
    slow.on_input(0, &right_stick_right());
    let mut slow_total = 0;
    for step in 1..=10 {
        for action in slow.tick(step * 10) {
            match action {
                Action::MouseMove { dx, .. } => slow_total += dx,
                other => panic!("expected motion, got {other:?}"),
            }
        }
    }

    assert_eq!(fast_total, slow_total);
    assert_eq!(fast_total, 50, "500 px/s over 100 ms");
}

#[test]
fn a_slow_rate_still_moves_because_remainders_are_carried() {
    // One pixel per second: a 4 ms tick is 0.004 px, which must not round away.
    let mut engine = stick_engine(mouse_stick(1, StickAxes::Both), 0);
    engine.on_input(0, &right_stick_right());

    let mut total = 0;
    for step in 1..=1_000 {
        for action in engine.tick(step * 4) {
            if let Action::MouseMove { dx, .. } = action {
                total += dx;
            }
        }
    }
    // ~4 s of travel at 1 px/s, so a handful of pixels rather than zero.
    assert!((3..=6).contains(&total), "travelled {total} px");
}

#[test]
fn a_centred_stick_emits_no_motion() {
    let mut engine = stick_engine(mouse_stick(1_000, StickAxes::Both), 0);
    engine.on_input(0, &right_stick_centred());

    assert_eq!(engine.tick(10), [] as [Action; 0]);
    assert_eq!(engine.tick(1_000), [] as [Action; 0]);
}

#[test]
fn releasing_the_stick_stops_the_motion_without_releasing_a_key() {
    let mut engine = stick_engine(mouse_stick(1_000, StickAxes::Both), 0);
    engine.on_input(0, &right_stick_right());
    assert_eq!(engine.tick(10), vec![Action::MouseMove { dx: 10, dy: 0 }]);

    // The trigger is a stick, not a button: a release must not invent a key-up.
    assert_eq!(
        engine.on_input(11, &right_stick_centred()),
        [] as [Action; 0]
    );
    assert_eq!(engine.tick(20), [] as [Action; 0]);
    assert!(engine.next_deadline().is_none());
}

#[test]
fn axes_and_inversions_are_applied() {
    let inverted_x = StickOutput {
        invert_x: true,
        ..mouse_stick(1_000, StickAxes::Both)
    };
    let mut engine = stick_engine(inverted_x, 0);
    engine.on_input(0, &right_stick_right());
    assert_eq!(engine.tick(10), vec![Action::MouseMove { dx: -10, dy: 0 }]);

    // A horizontal-only output ignores the vertical push entirely.
    let mut horizontal = stick_engine(mouse_stick(1_000, StickAxes::Horizontal), 0);
    horizontal.on_input(
        0,
        &InputFrame::pad(PadState {
            right_stick: StickState::new(0, i16::MAX),
            ..PadState::default()
        }),
    );
    assert_eq!(horizontal.tick(10), [] as [Action; 0]);
}

#[test]
fn a_scroll_output_turns_notches_instead_of_pixels() {
    let mut engine = stick_engine(
        StickOutput {
            target: StickTarget::Scroll,
            ..mouse_stick(1_000, StickAxes::Both)
        },
        0,
    );
    engine.on_input(0, &right_stick_right());

    assert_eq!(
        engine.tick(10),
        vec![Action::Scroll {
            horizontal: 10,
            vertical: 0
        }]
    );
}

#[test]
fn the_initial_delay_delays_the_first_motion() {
    let mut engine = stick_engine(mouse_stick(1_000, StickAxes::Both), 30);
    assert_eq!(engine.on_input(0, &right_stick_right()), [] as [Action; 0]);
    assert_eq!(engine.next_deadline(), Some(30));

    assert_eq!(engine.tick(29), [] as [Action; 0]);
    // The clock starts when the delay expires, so the full 30 ms is still due.
    assert_eq!(engine.tick(30), [] as [Action; 0]);
    assert_eq!(engine.tick(40), vec![Action::MouseMove { dx: 10, dy: 0 }]);
}

#[test]
fn a_stalled_loop_cannot_fling_the_pointer() {
    let mut engine = stick_engine(mouse_stick(20_000, StickAxes::Both), 0);
    engine.on_input(0, &right_stick_right());

    // Ten seconds at 20 000 px/s would be 200 000 px without the clamp.
    match engine.tick(10_000).as_slice() {
        [Action::MouseMove { dx, dy }] => {
            assert_eq!(*dx, 4_096, "clamped to the per-tick maximum");
            assert_eq!(*dy, 0);
        }
        other => panic!("expected one clamped move, got {other:?}"),
    }
}

#[test]
fn stick_directions_drive_the_key_outputs() {
    let mut engine = Engine::new(
        vec![Binding {
            from: Trigger::StickDirection {
                stick: Stick::Left,
                direction: StickDirection::Up,
            },
            outputs: vec![spam(spacebar(), 50, 15)],
        }],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    let pushed = InputFrame::pad(PadState {
        left_stick: StickState::new(0, i16::MAX),
        ..PadState::default()
    });
    let mut log = Vec::new();
    record(&mut log, 0, engine.on_input(0, &pushed));
    run(&mut engine, 0, 100, &mut log);
    assert_eq!(presses(&log), vec![0, 50, 100], "{log:?}");

    // Letting go releases the held key at once, like any other trigger.
    let centred = InputFrame::pad(PadState::default());
    record(&mut log, 101, engine.on_input(101, &centred));
    assert_eq!(log.last(), Some(&(101, Action::Release(spacebar()))));
}

#[test]
fn a_dpad_diagonal_drives_a_binding() {
    let mut engine = Engine::new(
        vec![binding(Trigger::Button(Button::UpLeft), spacebar(), 50, 10)],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );

    let up_left = InputFrame::pad(PadState::button(bits::DPAD_UP | bits::DPAD_LEFT));
    assert_eq!(
        engine.on_input(0, &up_left),
        vec![Action::Press(spacebar())]
    );

    let up_only = InputFrame::pad(PadState::button(bits::DPAD_UP));
    assert_eq!(
        engine.on_input(5, &up_only),
        vec![Action::Release(spacebar())],
        "losing a direction ends the diagonal"
    );
}

#[test]
fn uses_motion_reports_a_stick_output() {
    let engine = stick_engine(mouse_stick(800, StickAxes::Both), 0);
    assert!(engine.uses_motion());

    let keyboard_only = Engine::new(
        vec![binding(Trigger::Keyboard(q()), spacebar(), 50, 10)],
        THRESHOLDS,
        Box::new(SeededJitter::new(1)),
    );
    assert!(!keyboard_only.uses_motion());
}

/// A stick position on the unit circle at `degrees` (0 = right, counter-clockwise
/// positive), pushed to `deflection` of full scale.
fn at_angle(degrees: f64, deflection: f64) -> StickState {
    let radians = degrees.to_radians();
    let scale = deflection * f64::from(i16::MAX);
    #[allow(clippy::cast_possible_truncation, reason = "the test values fit i16")]
    StickState::new(
        (radians.cos() * scale) as i16,
        (radians.sin() * scale) as i16,
    )
}

/// Sweeps the right stick counter-clockwise from `from` to `to` degrees over
/// `duration` ms, at `deflection` of full scale, ticking every millisecond.
///
/// Wide tick used by [`sweep`], in milliseconds.
///
/// The two axes round independently, so a 1 ms tick turns a steady diagonal
/// push into a staircase whose per-step length overstates the real speed. A
/// wider tick keeps the rounding error small enough to compare speeds.
const SWEEP_TICK_MS: u64 = 10;

/// Sweeps the right stick counter-clockwise from `from` to `to` degrees over
/// `duration` ms, at `deflection` of full scale.
///
/// Returns the net pointer displacement and the actions emitted. Net
/// displacement is used rather than the sum of step lengths because the axes
/// quantise independently: adding up steps counts the zigzag, not the motion.
fn sweep(
    from: f64,
    to: f64,
    duration: u64,
    deflection: f64,
    sensitivity: u32,
) -> ((i64, i64), Vec<Action>) {
    let mut engine = stick_engine(mouse_stick(sensitivity, StickAxes::Both), 0);
    let mut actions = Vec::new();
    let mut net = (0_i64, 0_i64);

    let mut step = 0_u64;
    loop {
        let progress = to_f64(step) / to_f64(duration);
        let angle = to.mul_add(progress, from * (1.0 - progress));
        let frame = InputFrame::pad(PadState {
            right_stick: at_angle(angle, deflection),
            ..PadState::default()
        });
        actions.extend(engine.on_input(step, &frame));

        if step == duration {
            break;
        }
        step = (step + SWEEP_TICK_MS).min(duration);
        actions.extend(engine.tick(step));
    }

    for action in &actions {
        if let Action::MouseMove { dx, dy } = action {
            net.0 += i64::from(*dx);
            net.1 += i64::from(*dy);
        }
    }
    (net, actions)
}

/// Converts a small value to `f64` for the sweep math.
///
/// Every value here is a tick count or a pixel delta, far below `2^53`, so the
/// conversion is exact. Taking the string round-trip keeps an explicit cast out
/// of the test without an `as` clippy would flag.
fn to_f64(value: impl std::fmt::Display) -> f64 {
    value.to_string().parse().unwrap_or_default()
}

/// Euclidean length of a displacement.
fn length(displacement: (i64, i64)) -> f64 {
    to_f64(displacement.0).hypot(to_f64(displacement.1))
}

#[test]
fn a_ninety_degree_counter_clockwise_sweep_in_half_a_second_traces_an_arc() {
    // From pointing right to pointing up, a quarter turn over 500 ms at
    // 1000 px/s. The velocity turns, so the net displacement is the integral of
    // a rotating vector: speed/omega * sqrt(2) = 500/pi * sqrt(2) * 2.
    let (net, actions) = sweep(0.0, 90.0, 500, 1.0, 1_000);

    let moves: Vec<(i32, i32)> = actions
        .iter()
        .filter_map(|action| match action {
            Action::MouseMove { dx, dy } => Some((*dx, *dy)),
            _ => None,
        })
        .collect();
    assert!(!moves.is_empty(), "the sweep produced no motion");

    // Counter-clockwise on the stick is up on screen, so the pointer goes right
    // and up (negative `dy`, which is the SendInput screen convention).
    let first = moves.first().copied().expect("motion");
    let last = moves.last().copied().expect("motion");
    assert!(
        first.0 > 0 && first.1 == 0,
        "started facing right: {first:?}"
    );
    // At exactly 90 degrees the push is straight up, so the horizontal step
    // collapses to rounding noise; what matters is that the motion is upward.
    assert!(
        last.1 < 0 && last.1.unsigned_abs() > last.0.unsigned_abs(),
        "ended facing up: {last:?}"
    );

    // Every step moves right and up, never down again: the sweep is monotonic.
    for (dx, dy) in &moves {
        assert!(
            *dx >= 0 && *dy <= 0,
            "the pointer did not sweep monotonically: ({dx}, {dy})"
        );
    }

    // Net displacement of a 90 degree arc at constant speed is
    // `2 * speed / omega * sin(45°)` = `speed * T * 2 * sqrt(2) / pi` = 450 px,
    // noticeably shorter than the 500 px straight-line path a non-rotating
    // output would produce over the same time.
    let expected = 1_000.0 * 0.5 * 2.0 * std::f64::consts::SQRT_2 / std::f64::consts::PI;
    let actual = length(net);
    assert!(
        (expected * 0.95..=expected * 1.05).contains(&actual),
        "expected a net displacement of about {expected:.0} px, got {actual:.0} px"
    );
    assert!(
        actual < 500.0,
        "a rotating output must not travel like a fixed one: {actual:.0} px"
    );
}

#[test]
fn a_constant_deflection_keeps_a_constant_speed_through_the_whole_turn() {
    // The defect this guards: rescaling each axis on its own made a diagonal push
    // travel at 1/sqrt(2) of a cardinal one, so a sweep pulsed four times per
    // turn. Net displacement over a fixed direction isolates that.
    let mut speeds = Vec::new();
    for angle in [0.0, 22.5, 45.0, 67.5, 90.0, 135.0, 180.0, 270.0] {
        let (net, _) = sweep(angle, angle, 200, 0.9, 1_000);
        speeds.push((angle, length(net)));
    }

    let reference = speeds[0].1;
    for (angle, travelled) in &speeds {
        let ratio = travelled / reference;
        assert!(
            (0.97..=1.03).contains(&ratio),
            "speed at {angle} degrees is {ratio:.3} of the cardinal speed"
        );
    }
}

#[test]
fn no_direction_fires_without_also_producing_motion() {
    // The defect this guards: at 45 degrees a push could clear the radial
    // deadzone (so the direction trigger fired) while both axes stayed inside a
    // per-axis deadzone (so `effective` returned nothing).
    let deadzone = 7_849;
    let mut checked = 0;
    for angle in 0..360 {
        for deflection in [0.25, 0.5, 0.75, 1.0] {
            let state = at_angle(f64::from(angle), deflection);
            let direction = state.direction(deadzone);
            let (x, y) = state.effective(deadzone);
            let moving = x != 0 || y != 0;

            assert_eq!(
                direction.is_some(),
                moving,
                "{angle} degrees at {deflection} of full scale: direction={direction:?} motion=({x}, {y})"
            );
            checked += 1;
        }
    }
    assert!(checked > 1_000);
}

#[test]
fn the_direction_crossings_of_a_quarter_turn_are_sectoral() {
    // A 90 degree sweep crosses two octant boundaries, at 22.5 and 67.5.
    let mut seen = Vec::new();
    for step in 0..=500 {
        let angle = f64::from(step) * 90.0 / 500.0;
        let state = at_angle(angle, 1.0);
        let direction = state.direction(THRESHOLDS.stick_deadzone);
        if seen.last() != Some(&direction) {
            seen.push(direction);
        }
    }

    assert_eq!(
        seen,
        vec![
            Some(StickDirection::Right),
            Some(StickDirection::UpRight),
            Some(StickDirection::Up),
        ],
        "a quarter turn spans three sectors, not a smooth rotation"
    );
}
