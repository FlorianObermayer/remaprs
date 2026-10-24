//! The control loop's reaction to backend failures.
//!
//! `SendInput` refuses to inject while the secure desktop is up, and `XInput`
//! returns transient codes while a pad is re-enumerated. Neither may end the
//! run: the process used to exit silently on both.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow};
use remaprs::clock::Clock;
use remaprs::config::JitterRange;
use remaprs::engine::{Action, Binding, Engine, Output, Step};
use remaprs::gamepad::{GamepadSource, PadState, ScriptedSource, bits};
use remaprs::input::NoKeyboard;
use remaprs::keys::{Key, KeyboardKey, Trigger};
use remaprs::output::KeyEmitter;
use remaprs::runner::Runner;
use remaprs::safety::AllowAll;

const THRESHOLD: u8 = 30;

fn spacebar() -> Key {
    Key::Keyboard(KeyboardKey::new(0x20, false))
}

/// A clock that advances by the requested sleep and stops the run at `stop_at`.
struct TestClock {
    now: Cell<u64>,
    shutdown: Arc<AtomicBool>,
    stop_at: u64,
}

impl TestClock {
    fn new(shutdown: Arc<AtomicBool>, stop_at: u64) -> Self {
        Self {
            now: Cell::new(0),
            shutdown,
            stop_at,
        }
    }
}

impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.now.get()
    }

    fn sleep(&self, duration: Duration) {
        let step = u64::try_from(duration.as_millis()).unwrap_or(1).max(1);
        self.now.set(self.now.get() + step);
        if self.now.get() >= self.stop_at {
            self.shutdown.store(true, Ordering::Relaxed);
        }
    }
}

/// An emitter that refuses the first `failures` events, like a blocked desktop.
struct FlakyEmitter {
    failures_left: Rc<Cell<usize>>,
    events: Rc<RefCell<Vec<Action>>>,
}

impl FlakyEmitter {
    fn new(failures: usize) -> Self {
        Self {
            failures_left: Rc::new(Cell::new(failures)),
            events: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn record(&self, action: Action) -> Result<()> {
        let left = self.failures_left.get();
        if left > 0 {
            self.failures_left.set(left - 1);
            // Windows reports error 0 here because SendInput often does not set
            // the last error when the desktop blocks it.
            return Err(anyhow!("SendInput blocked (simulated)"));
        }
        self.events.borrow_mut().push(action);
        Ok(())
    }
}

impl KeyEmitter for FlakyEmitter {
    fn press(&mut self, key: Key) -> Result<()> {
        self.record(Action::Press(key))
    }

    fn release(&mut self, key: Key) -> Result<()> {
        self.record(Action::Release(key))
    }
}

/// A pad source whose first read fails, then reports `frames`.
struct FlakySource {
    reads: usize,
    inner: ScriptedSource,
}

impl GamepadSource for FlakySource {
    fn poll(&mut self) -> Result<Option<PadState>> {
        self.reads += 1;
        if self.reads == 1 {
            return Err(anyhow!("XInputGetState failed with error 1167 (simulated)"));
        }
        self.inner.poll()
    }
}

fn engine() -> Engine {
    Engine::new(
        vec![Binding {
            from: Trigger::Button(remaprs::keys::Button::A),
            outputs: vec![Output::Sequence {
                steps: vec![Step {
                    key: spacebar(),
                    hold: JitterRange::fixed(10),
                    pause: JitterRange::fixed(10),
                }],
                repeat: true,
                initial_delay: JitterRange::fixed(0),
            }],
        }],
        THRESHOLD,
        Box::new(remaprs::jitter::SeededJitter::new(1)),
    )
}

fn held() -> PadState {
    PadState::button(bits::A)
}

#[test]
fn a_blocked_emit_keeps_the_run_alive() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let clock = TestClock::new(Arc::clone(&shutdown), 500);
    let emitter = FlakyEmitter::new(usize::MAX);
    let events = Rc::clone(&emitter.events);

    let mut runner = Runner {
        engine: engine(),
        source: Box::new(ScriptedSource::new([Some(held())])),
        keyboard: Box::new(NoKeyboard),
        emitter: Box::new(emitter),
        guard: Box::new(AllowAll),
        poll_interval_ms: 4,
        // The final release is checked separately; this test is about the loop.
        release_on_exit: false,
    };
    let toggle = AtomicBool::new(false);

    let result = runner.run(&clock, &shutdown, &toggle);

    // The important part: a blocked desktop is not fatal any more.
    assert!(
        result.is_ok(),
        "a blocked emit must not end the run: {result:?}"
    );
    assert!(
        events.borrow().is_empty(),
        "nothing was delivered, so nothing may be recorded"
    );
}

#[test]
fn a_blocked_release_on_exit_is_reported() {
    // If the release cannot be delivered, the exit status must say so rather
    // than pretending the keys came up.
    let shutdown = Arc::new(AtomicBool::new(false));
    let clock = TestClock::new(Arc::clone(&shutdown), 60);

    let mut runner = Runner {
        engine: engine(),
        source: Box::new(ScriptedSource::new([Some(held())])),
        keyboard: Box::new(NoKeyboard),
        emitter: Box::new(FlakyEmitter::new(usize::MAX)),
        guard: Box::new(AllowAll),
        poll_interval_ms: 4,
        release_on_exit: true,
    };
    let toggle = AtomicBool::new(false);

    let result = runner.run(&clock, &shutdown, &toggle);

    assert!(
        result.is_err(),
        "a failed release must not be reported as a clean exit"
    );
}

#[test]
fn a_transient_emit_failure_is_retried_until_it_succeeds() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let clock = TestClock::new(Arc::clone(&shutdown), 500);
    let emitter = FlakyEmitter::new(2);
    let events = Rc::clone(&emitter.events);

    let mut runner = Runner {
        engine: engine(),
        source: Box::new(ScriptedSource::new([Some(held())])),
        keyboard: Box::new(NoKeyboard),
        emitter: Box::new(emitter),
        guard: Box::new(AllowAll),
        poll_interval_ms: 4,
        release_on_exit: true,
    };
    let toggle = AtomicBool::new(false);

    let result = runner.run(&clock, &shutdown, &toggle);
    assert!(result.is_ok(), "{result:?}");

    let events = events.borrow();
    assert!(
        events.contains(&Action::Press(spacebar())),
        "the queued press was never delivered: {events:?}"
    );
}

#[test]
fn a_read_error_is_treated_as_a_disconnected_pad() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let clock = TestClock::new(Arc::clone(&shutdown), 100);
    let emitter = FlakyEmitter::new(0);

    let mut runner = Runner {
        engine: engine(),
        source: Box::new(FlakySource {
            reads: 0,
            inner: ScriptedSource::new([Some(held())]),
        }),
        keyboard: Box::new(NoKeyboard),
        emitter: Box::new(emitter),
        guard: Box::new(AllowAll),
        poll_interval_ms: 4,
        release_on_exit: true,
    };
    let toggle = AtomicBool::new(false);

    let result = runner.run(&clock, &shutdown, &toggle);

    // Previously this error propagated out of `poll()?` and killed the process.
    assert!(
        result.is_ok(),
        "a transient read error must not end the run: {result:?}"
    );
}

#[test]
fn an_unsupported_binding_still_releases_on_exit() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let clock = TestClock::new(Arc::clone(&shutdown), 60);
    let emitter = FlakyEmitter::new(0);
    let events = Rc::clone(&emitter.events);

    let mut runner = Runner {
        engine: engine(),
        source: Box::new(ScriptedSource::new([Some(held())])),
        keyboard: Box::new(NoKeyboard),
        emitter: Box::new(emitter),
        guard: Box::new(AllowAll),
        poll_interval_ms: 4,
        release_on_exit: true,
    };
    let toggle = AtomicBool::new(false);

    runner
        .run(&clock, &shutdown, &toggle)
        .expect("the run ends cleanly");

    let events = events.borrow();
    assert!(
        events.contains(&Action::Release(spacebar())),
        "the exit path must release what it pressed: {events:?}"
    );
}
