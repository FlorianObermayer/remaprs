//! One frame of every input device, shared by the controller and keyboard
//! sources.
//!
//! Keeping the frame free of platform code lets the engine decide what to emit
//! without knowing whether a trigger came from `XInput` or `GetAsyncKeyState`.

use crate::gamepad::PadState;
use crate::keys::{KeyboardKey, Stick, Trigger};

/// Virtual-key codes are 8-bit, so a fixed table holds every key.
const VIRTUAL_KEYS: usize = 256;

/// How a frame decides whether a trigger counts as pressed.
///
/// The two values are kept together because every caller that reads a frame
/// needs both: the trigger threshold for `LT`/`RT`, and the deadzone for the
/// sticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputThresholds {
    /// An analog trigger counts as pressed at or above this value.
    pub trigger: u8,
    /// A stick push shorter than this (in `XInput` units) is centred.
    pub stick_deadzone: u16,
}

impl InputThresholds {
    /// Thresholds for `trigger` and `stick_deadzone`.
    #[must_use]
    pub const fn new(trigger: u8, stick_deadzone: u16) -> Self {
        Self {
            trigger,
            stick_deadzone,
        }
    }

    /// The most common pairing, used by tests and scripted sources.
    #[must_use]
    pub const fn with_default_deadzone(trigger: u8) -> Self {
        Self {
            trigger,
            stick_deadzone: DEFAULT_STICK_DEADZONE,
        }
    }
}

/// `XInput`'s own default deadzone for the left stick.
pub const DEFAULT_STICK_DEADZONE: u16 = 7_849;

/// The keyboard keys that are down at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardState {
    down: [bool; VIRTUAL_KEYS],
}

impl Default for KeyboardState {
    fn default() -> Self {
        // `[bool; 256]` has no derived `Default` because the standard library
        // only covers arrays up to 32 elements.
        Self {
            down: [false; VIRTUAL_KEYS],
        }
    }
}

impl KeyboardState {
    /// Records `key` as down.
    ///
    /// Values outside the virtual-key range are ignored rather than panicking,
    /// because [`KeyboardKey::new`] accepts any `u16`.
    pub fn set(&mut self, key: KeyboardKey) {
        if let Some(slot) = self.down.get_mut(usize::from(key.vk)) {
            *slot = true;
        }
    }

    /// Whether `key` is down.
    #[must_use]
    pub fn is_down(&self, key: KeyboardKey) -> bool {
        self.down.get(usize::from(key.vk)).copied().unwrap_or(false)
    }

    /// Builds a state with `keys` down, for scripted sources and tests.
    #[must_use]
    pub fn from_keys(keys: impl IntoIterator<Item = KeyboardKey>) -> Self {
        let mut state = Self::default();
        for key in keys {
            state.set(key);
        }
        state
    }
}

/// One sample of every input device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputFrame {
    /// Controller state, or `None` while no pad is connected.
    pub pad: Option<PadState>,
    /// Keyboard keys that are down.
    pub keyboard: KeyboardState,
}

impl InputFrame {
    /// A frame with only the controller connected.
    #[must_use]
    pub const fn pad(pad: PadState) -> Self {
        Self {
            pad: Some(pad),
            keyboard: KeyboardState {
                down: [false; VIRTUAL_KEYS],
            },
        }
    }

    /// Attaches a keyboard state.
    #[must_use]
    pub fn with_keyboard(mut self, keyboard: KeyboardState) -> Self {
        self.keyboard = keyboard;
        self
    }

    /// Whether `trigger` currently counts as pressed.
    ///
    /// A missing pad presses no controller buttons; keyboard triggers keep
    /// working whether or not a controller is plugged in. A stick direction
    /// counts as pressed only when the push clears the deadzone and falls in the
    /// matching octant.
    #[must_use]
    pub fn is_pressed(&self, trigger: Trigger, thresholds: InputThresholds) -> bool {
        match trigger {
            Trigger::Button(button) => self
                .pad
                .is_some_and(|pad| pad.is_pressed(button, thresholds.trigger)),
            Trigger::Keyboard(key) => self.keyboard.is_down(key),
            Trigger::StickDirection { stick, direction } => self.pad.is_some_and(|pad| {
                pad.stick(stick).direction(thresholds.stick_deadzone) == Some(direction)
            }),
            // An axis produces continuous motion, never a press; the stick is
            // an edge only in the sense that it drives `Engine`'s rate phase.
            Trigger::StickAxis(stick) => self
                .pad
                .is_some_and(|pad| Self::deflection(pad, stick, thresholds).is_some()),
        }
    }

    /// The stick's deadzone-rescaled deflection, or `None` while it is centred.
    ///
    /// `None` covers both "no pad" and "inside the deadzone", because both mean
    /// the same thing to a continuous output: do not move.
    #[must_use]
    pub fn stick_deflection(
        &self,
        stick: Stick,
        thresholds: InputThresholds,
    ) -> Option<(i32, i32)> {
        self.pad
            .and_then(|pad| Self::deflection(pad, stick, thresholds))
    }

    fn deflection(pad: PadState, stick: Stick, thresholds: InputThresholds) -> Option<(i32, i32)> {
        let state = pad.stick(stick);
        state.direction(thresholds.stick_deadzone)?;
        Some(state.effective(thresholds.stick_deadzone))
    }
}

/// Where keyboard state comes from.
pub trait KeyboardSource {
    /// Returns the keys that are down right now.
    fn poll(&mut self) -> KeyboardState;
}

/// Keyboard source that never reports a key as down.
///
/// Useful as a default for callers that do not read the keyboard.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoKeyboard;

impl KeyboardSource for NoKeyboard {
    fn poll(&mut self) -> KeyboardState {
        KeyboardState::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{InputFrame, InputThresholds, KeyboardState};
    use crate::gamepad::{PadState, StickState, bits};
    use crate::keys::{Button, KeyboardKey, Stick, StickDirection, Trigger};

    fn key(vk: u16) -> KeyboardKey {
        KeyboardKey::new(vk, false)
    }

    #[test]
    fn tracks_keys_independently_of_their_extended_flag() {
        let mut state = KeyboardState::default();
        assert!(!state.is_down(key(0x51)));

        state.set(KeyboardKey::new(0x51, true));
        assert!(
            state.is_down(key(0x51)),
            "state is tracked by virtual-key code"
        );
        assert!(!state.is_down(key(0x52)));
    }

    #[test]
    fn ignores_virtual_keys_outside_the_table() {
        let mut state = KeyboardState::default();
        state.set(KeyboardKey::new(u16::MAX, false));
        assert!(!state.is_down(KeyboardKey::new(u16::MAX, false)));
    }

    #[test]
    fn triggers_read_the_matching_device() {
        let q = key(0x51);
        let frame =
            InputFrame::pad(PadState::button(bits::A)).with_keyboard(KeyboardState::from_keys([q]));
        let thresholds = InputThresholds::with_default_deadzone(30);

        assert!(frame.is_pressed(Trigger::Button(Button::A), thresholds));
        assert!(!frame.is_pressed(Trigger::Button(Button::B), thresholds));
        assert!(frame.is_pressed(Trigger::Keyboard(q), thresholds));
        assert!(!frame.is_pressed(Trigger::Keyboard(key(0x52)), thresholds));
    }

    #[test]
    fn a_missing_pad_presses_no_buttons() {
        let frame = InputFrame::default();
        let thresholds = InputThresholds::with_default_deadzone(30);
        assert!(!frame.is_pressed(Trigger::Button(Button::A), thresholds));
        assert!(!frame.is_pressed(
            Trigger::StickDirection {
                stick: Stick::Left,
                direction: StickDirection::Up,
            },
            thresholds
        ));
        assert_eq!(frame.stick_deflection(Stick::Left, thresholds), None);
    }

    #[test]
    fn stick_directions_respect_the_deadzone() {
        let thresholds = InputThresholds::new(30, 8_000);
        let pushed = |x, y| {
            InputFrame::pad(PadState {
                left_stick: StickState::new(x, y),
                ..PadState::default()
            })
        };

        let up = pushed(0, 20_000);
        assert!(up.is_pressed(
            Trigger::StickDirection {
                stick: Stick::Left,
                direction: StickDirection::Up,
            },
            thresholds
        ));
        assert!(!up.is_pressed(
            Trigger::StickDirection {
                stick: Stick::Left,
                direction: StickDirection::Down,
            },
            thresholds
        ));
        assert!(
            !up.is_pressed(
                Trigger::StickDirection {
                    stick: Stick::Right,
                    direction: StickDirection::Up,
                },
                thresholds
            ),
            "the other stick must not fire"
        );

        let centred = pushed(1_000, 1_000);
        assert!(!centred.is_pressed(
            Trigger::StickDirection {
                stick: Stick::Left,
                direction: StickDirection::UpRight,
            },
            thresholds
        ));
        assert_eq!(centred.stick_deflection(Stick::Left, thresholds), None);
    }

    #[test]
    fn stick_axes_report_a_rescaled_deflection() {
        let thresholds = InputThresholds::new(30, 8_000);
        let frame = InputFrame::pad(PadState {
            right_stick: StickState::new(32_767, 0),
            ..PadState::default()
        });

        let (x, y) = frame
            .stick_deflection(Stick::Right, thresholds)
            .expect("the stick is past the deadzone");
        assert_eq!(x, 32_767);
        assert_eq!(y, 0);
        assert!(frame.is_pressed(Trigger::StickAxis(Stick::Right), thresholds));
        assert_eq!(
            frame.stick_deflection(Stick::Left, thresholds),
            None,
            "the untouched stick is centred"
        );
    }
}
