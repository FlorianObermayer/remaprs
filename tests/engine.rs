//! Behaviour of the output state machine, driven by a simulated millisecond
//! clock.
//!
//! The clock advances one millisecond at a time, so every assertion about a
//! timestamp is exact rather than approximate.

use remaprs::config::JitterRange;
use remaprs::engine::{Action, Binding, Engine, Output, Step};
use remaprs::gamepad::{PadState, bits};
use remaprs::input::{InputFrame, KeyboardState};
use remaprs::jitter::SeededJitter;
use remaprs::keys::{Button, Key, KeyboardKey, Trigger};

const THRESHOLD: u8 = 30;
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
        THRESHOLD,
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
        THRESHOLD,
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
    Engine::new(vec![binding], THRESHOLD, Box::new(SeededJitter::new(1)))
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
        THRESHOLD,
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
        THRESHOLD,
        Box::new(SeededJitter::new(1)),
    );

    let just_below = InputFrame::pad(PadState {
        buttons: 0,
        left_trigger: 0,
        right_trigger: THRESHOLD - 1,
    });
    assert_eq!(engine.on_input(0, &just_below), [] as [Action; 0]);
    assert!(engine.next_deadline().is_none());

    let at_threshold = InputFrame::pad(PadState {
        buttons: 0,
        left_trigger: 0,
        right_trigger: THRESHOLD,
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
        THRESHOLD,
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
        THRESHOLD,
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
        THRESHOLD,
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
        THRESHOLD,
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
        THRESHOLD,
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
                Action::Release(_) => None,
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
        THRESHOLD,
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
        THRESHOLD,
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
