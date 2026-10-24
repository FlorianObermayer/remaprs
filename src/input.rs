//! One frame of every input device, shared by the controller and keyboard
//! sources.
//!
//! Keeping the frame free of platform code lets the engine decide what to emit
//! without knowing whether a trigger came from `XInput` or `GetAsyncKeyState`.

use crate::gamepad::PadState;
use crate::keys::{KeyboardKey, Trigger};

/// Virtual-key codes are 8-bit, so a fixed table holds every key.
const VIRTUAL_KEYS: usize = 256;

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
    /// working whether or not a controller is plugged in.
    #[must_use]
    pub fn is_pressed(&self, trigger: Trigger, threshold: u8) -> bool {
        match trigger {
            Trigger::Button(button) => self
                .pad
                .is_some_and(|pad| pad.is_pressed(button, threshold)),
            Trigger::Keyboard(key) => self.keyboard.is_down(key),
        }
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
    use super::{InputFrame, KeyboardState};
    use crate::gamepad::{PadState, bits};
    use crate::keys::{Button, KeyboardKey, Trigger};

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

        assert!(frame.is_pressed(Trigger::Button(Button::A), 30));
        assert!(!frame.is_pressed(Trigger::Button(Button::B), 30));
        assert!(frame.is_pressed(Trigger::Keyboard(q), 30));
        assert!(!frame.is_pressed(Trigger::Keyboard(key(0x52)), 30));
    }

    #[test]
    fn a_missing_pad_presses_no_buttons() {
        let frame = InputFrame::default();
        assert!(!frame.is_pressed(Trigger::Button(Button::A), 30));
    }
}
