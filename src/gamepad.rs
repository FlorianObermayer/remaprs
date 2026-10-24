//! Controller state abstraction shared by the real `XInput` backend and the tests.

use anyhow::Result;

use crate::keys::Button;

/// Raw `XINPUT_GAMEPAD.wButtons` bits, kept identical to the Win32 values so no
/// conversion is needed when reading a real pad.
pub mod bits {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LEFT_THUMB: u16 = 0x0040;
    pub const RIGHT_THUMB: u16 = 0x0080;
    pub const LEFT_SHOULDER: u16 = 0x0100;
    pub const RIGHT_SHOULDER: u16 = 0x0200;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
}

/// The `XInput` bit belonging to `button`, if it has one.
///
/// Analog triggers have no bit; they are read from [`PadState`]'s trigger bytes.
#[must_use]
pub const fn button_bit(button: Button) -> Option<u16> {
    let bit = match button {
        Button::A => bits::A,
        Button::B => bits::B,
        Button::X => bits::X,
        Button::Y => bits::Y,
        Button::Lb => bits::LEFT_SHOULDER,
        Button::Rb => bits::RIGHT_SHOULDER,
        Button::Ls => bits::LEFT_THUMB,
        Button::Rs => bits::RIGHT_THUMB,
        Button::Back => bits::BACK,
        Button::Start => bits::START,
        Button::Up => bits::DPAD_UP,
        Button::Down => bits::DPAD_DOWN,
        Button::Left => bits::DPAD_LEFT,
        Button::Right => bits::DPAD_RIGHT,
        Button::Lt | Button::Rt => return None,
    };
    Some(bit)
}

/// One frame of controller state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadState {
    /// Digital button bits.
    pub buttons: u16,
    /// Left trigger, 0-255.
    pub left_trigger: u8,
    /// Right trigger, 0-255.
    pub right_trigger: u8,
}

impl PadState {
    /// Builds a state from raw button bits, for tests and tooling.
    #[must_use]
    pub const fn button(bits: u16) -> Self {
        Self {
            buttons: bits,
            left_trigger: 0,
            right_trigger: 0,
        }
    }

    /// Whether `button` currently counts as pressed.
    #[must_use]
    pub fn is_pressed(&self, button: Button, trigger_threshold: u8) -> bool {
        match button {
            Button::Lt => self.left_trigger >= trigger_threshold,
            Button::Rt => self.right_trigger >= trigger_threshold,
            digital => button_bit(digital).is_some_and(|bit| self.buttons & bit != 0),
        }
    }

    /// Human-readable summary used by `--list-buttons`.
    #[must_use]
    pub fn describe(&self, trigger_threshold: u8) -> String {
        let pressed: Vec<&str> = Button::ALL
            .into_iter()
            .filter(|button| !button.is_analog() && self.is_pressed(*button, trigger_threshold))
            .map(Button::name)
            .collect();

        let analog = format!("LT={:<3} RT={:<3}", self.left_trigger, self.right_trigger);

        if pressed.is_empty() {
            analog
        } else {
            format!("{analog} | {}", pressed.join(" "))
        }
    }
}

/// Where controller frames come from.
pub trait GamepadSource {
    /// Returns `Ok(None)` while the pad is unplugged.
    ///
    /// # Errors
    ///
    /// Fails on unexpected backend errors.
    fn poll(&mut self) -> Result<Option<PadState>>;
}

/// Replays a fixed list of frames, repeating the last one forever.
///
/// Used by tests and by `--dry-run` demos.
#[derive(Debug, Default)]
pub struct ScriptedSource {
    frames: std::collections::VecDeque<Option<PadState>>,
    last: Option<PadState>,
}

impl ScriptedSource {
    #[must_use]
    pub fn new(frames: impl IntoIterator<Item = Option<PadState>>) -> Self {
        Self {
            frames: frames.into_iter().collect(),
            last: None,
        }
    }
}

impl GamepadSource for ScriptedSource {
    fn poll(&mut self) -> Result<Option<PadState>> {
        if let Some(frame) = self.frames.pop_front() {
            self.last = frame;
        }
        Ok(self.last)
    }
}

#[cfg(test)]
mod tests {
    use super::{PadState, bits};

    use crate::keys::Button;

    #[test]
    fn digital_buttons_read_the_bitmask() {
        let state = PadState::button(bits::A | bits::X);
        assert!(state.is_pressed(Button::A, 30));
        assert!(state.is_pressed(Button::X, 30));
        assert!(!state.is_pressed(Button::B, 30));
    }

    #[test]
    fn triggers_use_the_threshold() {
        let state = PadState {
            buttons: 0,
            left_trigger: 29,
            right_trigger: 30,
        };
        assert!(!state.is_pressed(Button::Lt, 30));
        assert!(state.is_pressed(Button::Rt, 30));
        assert!(state.is_pressed(Button::Lt, 0));
    }

    #[test]
    fn describe_mentions_pressed_buttons() {
        let state = PadState::button(bits::A | bits::X);
        let text = state.describe(30);
        assert!(text.contains('A'), "{text}");
        assert!(text.contains('X'), "{text}");
    }
}
