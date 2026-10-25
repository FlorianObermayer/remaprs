//! Controller state abstraction shared by the real `XInput` backend and the tests.

use anyhow::Result;

use crate::keys::{Button, Stick, StickDirection};

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

/// The `XInput` bits belonging to `button`, if it has any.
///
/// Digital buttons map to one bit, the derived d-pad diagonals to the two
/// cardinal bits they span, and analog triggers to none at all; the triggers are
/// read from [`PadState`]'s trigger bytes instead.
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
        Button::UpLeft => bits::DPAD_UP | bits::DPAD_LEFT,
        Button::UpRight => bits::DPAD_UP | bits::DPAD_RIGHT,
        Button::DownLeft => bits::DPAD_DOWN | bits::DPAD_LEFT,
        Button::DownRight => bits::DPAD_DOWN | bits::DPAD_RIGHT,
        Button::Lt | Button::Rt => return None,
    };
    Some(bit)
}

/// Full-scale deflection, which is what `XInput` reports at the stick's edge.
pub const STICK_MAX: i16 = 32_767;

/// `tan(22.5°)` scaled by 10 000.
///
/// A component is "pure" when the other one is smaller than this fraction of it;
/// otherwise the push is diagonal. The constant keeps the classification integer
/// and therefore exact, which is what makes the octant tests deterministic.
const TAN_22_5_PER_10K: i32 = 4_142;

/// Scale used to compare the two components of a deflection.
const OCTANT_SCALE: i32 = 10_000;

/// One stick's deflection.
///
/// The axes keep `XInput`'s orientation: positive `y` is up, and `x` grows to the
/// right, so `-32768..=32767` on both axes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StickState {
    /// Horizontal deflection.
    pub x: i16,
    /// Vertical deflection, positive being up.
    pub y: i16,
}

impl StickState {
    /// A stick pushed to `(x, y)`, for tests and tooling.
    #[must_use]
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }

    /// Distance from the centre, in `0..=32767`.
    ///
    /// Clamped so that the corners, where `XInput` can report values beyond the
    /// circle, still behave like the edge instead of wrapping.
    #[must_use]
    pub fn magnitude(self) -> u16 {
        u16::try_from(self.raw_magnitude())
            .map_or(STICK_MAX as u16, |value| value.min(STICK_MAX as u16))
    }

    /// Unclamped distance from the centre.
    ///
    /// A corner reaches about 46 341, because `XInput` reports the square rather
    /// than the circle. The exact value is what lets [`Self::effective`] map a
    /// deflection back onto the unit circle, so a corner is not faster than the
    /// rim and a diagonal is not slower than a cardinal.
    fn raw_magnitude(self) -> i64 {
        // `i64` because the corner `(-32768, -32768)` squares to more than
        // `i32::MAX`; `-32768` is a value `XInput` genuinely reports.
        let x = i64::from(self.x);
        let y = i64::from(self.y);
        (x * x + y * y).isqrt()
    }

    /// The direction this stick is pushed in, or `None` inside the deadzone.
    ///
    /// The deadzone is radial, so it agrees exactly with [`Self::effective`]:
    /// anything that counts as a direction also produces motion. Testing the
    /// axes separately would leave a notch at 45°, where a push clears the
    /// deadzone diagonally while neither axis alone does.
    #[must_use]
    pub fn direction(self, deadzone: u16) -> Option<StickDirection> {
        if self.raw_magnitude() < i64::from(deadzone.max(1)) {
            return None;
        }
        Some(self.octant())
    }

    /// The octant of this deflection, ignoring the deadzone.
    ///
    /// A zero component is treated as "no push on that axis", so a deflection of
    /// `(100, 0)` is [`StickDirection::Right`] rather than a division by zero.
    fn octant(self) -> StickDirection {
        let x = i32::from(self.x);
        let y = i32::from(self.y);
        // `abs` is only reachable for `i32::MIN`, which an `i16` cannot be.
        let (ax, ay) = (x.abs(), y.abs());
        let horizontal = x.signum();
        let vertical = y.signum();

        // A component is pure when the other one is below `tan(22.5°)` of it.
        // Comparing cross-multiplied keeps this integer and therefore exact.
        if ay * OCTANT_SCALE < ax * TAN_22_5_PER_10K {
            return if horizontal < 0 {
                StickDirection::Left
            } else {
                StickDirection::Right
            };
        }
        if ax * OCTANT_SCALE < ay * TAN_22_5_PER_10K {
            return if vertical < 0 {
                StickDirection::Down
            } else {
                StickDirection::Up
            };
        }
        match (horizontal < 0, vertical < 0) {
            (true, true) => StickDirection::DownLeft,
            (true, false) => StickDirection::UpLeft,
            (false, true) => StickDirection::DownRight,
            (false, false) => StickDirection::UpRight,
        }
    }

    /// Deflection rescaled past the deadzone, in `-32767..=32767` per axis.
    ///
    /// The deadzone is radial, and the result keeps the push *direction* while
    /// its length depends only on how far the stick is pushed. Both halves
    /// matter for a stick that is being rotated:
    ///
    /// * Testing the axes separately would open a notch at 45°, where a push
    ///   clears the deadzone diagonally but neither axis alone does, so a slow
    ///   sweep would stutter as it crossed each diagonal.
    /// * Rescaling each axis on its own would make a diagonal push travel at
    ///   `1/√2` of a cardinal one, so a sweep at constant deflection would speed
    ///   up and slow down four times per turn.
    ///
    /// Scaling the whole vector instead keeps the speed constant for a constant
    /// deflection, whatever the angle, and clamps a square-gate corner to the rim
    /// rather than letting it outrun a round-gate stick.
    #[must_use]
    pub fn effective(self, deadzone: u16) -> (i32, i32) {
        let deadzone = i64::from(deadzone).min(i64::from(STICK_MAX) - 1);
        let extent = i64::from(STICK_MAX) - deadzone;

        let raw = self.raw_magnitude();
        if raw <= deadzone {
            return (0, 0);
        }

        // How much of the post-deadzone travel is used, expressed as a length on
        // the unit circle; a corner that reaches past the rim is capped there.
        let travel = raw - deadzone;
        let length = (travel * i64::from(STICK_MAX) / extent).min(i64::from(STICK_MAX));

        let scale = |component: i16| {
            let component = i64::from(component);
            let scaled = component * length / raw;
            i32::try_from(scaled).unwrap_or_default()
        };

        (scale(self.x), scale(self.y))
    }
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
    /// Left stick deflection.
    pub left_stick: StickState,
    /// Right stick deflection.
    pub right_stick: StickState,
}

impl PadState {
    /// Builds a state from raw button bits, for tests and tooling.
    #[must_use]
    pub const fn button(bits: u16) -> Self {
        Self {
            buttons: bits,
            left_trigger: 0,
            right_trigger: 0,
            left_stick: StickState::new(0, 0),
            right_stick: StickState::new(0, 0),
        }
    }

    /// The deflection of `stick`.
    #[must_use]
    pub const fn stick(&self, stick: Stick) -> StickState {
        match stick {
            Stick::Left => self.left_stick,
            Stick::Right => self.right_stick,
        }
    }

    /// Whether `button` currently counts as pressed.
    ///
    /// A d-pad diagonal needs every bit of its mask, so it fires only while both
    /// cardinals are held.
    #[must_use]
    pub fn is_pressed(&self, button: Button, trigger_threshold: u8) -> bool {
        match button {
            Button::Lt => self.left_trigger >= trigger_threshold,
            Button::Rt => self.right_trigger >= trigger_threshold,
            digital => button_bit(digital).is_some_and(|mask| self.buttons & mask == mask),
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

        let analog = format!(
            "LS=({:>6},{:>6}) RS=({:>6},{:>6}) LT={:<3} RT={:<3}",
            self.left_stick.x,
            self.left_stick.y,
            self.right_stick.x,
            self.right_stick.y,
            self.left_trigger,
            self.right_trigger
        );

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
    use super::{PadState, StickState, bits};

    use crate::keys::{Button, Stick, StickDirection};

    #[test]
    fn digital_buttons_read_the_bitmask() {
        let state = PadState::button(bits::A | bits::X);
        assert!(state.is_pressed(Button::A, 30));
        assert!(state.is_pressed(Button::X, 30));
        assert!(!state.is_pressed(Button::B, 30));
    }

    #[test]
    fn dpad_diagonals_need_both_cardinals() {
        let up_left = PadState::button(bits::DPAD_UP | bits::DPAD_LEFT);
        assert!(up_left.is_pressed(Button::UpLeft, 30));
        assert!(
            up_left.is_pressed(Button::Up, 30),
            "the cardinals stay usable"
        );
        assert!(
            !up_left.is_pressed(Button::UpRight, 30),
            "the opposite diagonal must not fire"
        );

        let up_only = PadState::button(bits::DPAD_UP);
        assert!(
            !up_only.is_pressed(Button::UpLeft, 30),
            "a single cardinal is not a diagonal"
        );
    }

    #[test]
    fn triggers_use_the_threshold() {
        let state = PadState {
            buttons: 0,
            left_trigger: 29,
            right_trigger: 30,
            ..PadState::default()
        };
        assert!(!state.is_pressed(Button::Lt, 30));
        assert!(state.is_pressed(Button::Rt, 30));
        assert!(state.is_pressed(Button::Lt, 0));
    }

    #[test]
    fn describe_mentions_pressed_buttons_and_sticks() {
        let state = PadState {
            left_stick: StickState::new(-16_000, 8_000),
            ..PadState::button(bits::A | bits::X)
        };
        let text = state.describe(30);
        assert!(text.contains('A'), "{text}");
        assert!(text.contains('X'), "{text}");
        assert!(
            text.contains("-16000"),
            "the stick position is shown: {text}"
        );
    }

    #[test]
    fn octants_follow_the_eight_directions() {
        let edge = super::STICK_MAX;
        let near_edge = edge - 1;
        for (x, y, expected) in [
            (0, edge, StickDirection::Up),
            (0, -edge, StickDirection::Down),
            (-edge, 0, StickDirection::Left),
            (near_edge, 0, StickDirection::Right),
            (-edge, edge, StickDirection::UpLeft),
            (edge, edge, StickDirection::UpRight),
            (-edge, -edge, StickDirection::DownLeft),
            (edge, -edge, StickDirection::DownRight),
        ] {
            assert_eq!(
                StickState::new(x, y).direction(1),
                Some(expected),
                "({x}, {y})"
            );
        }
        assert_eq!(
            StickState::new(0, 0).direction(1),
            None,
            "a centred stick has no direction"
        );
    }

    #[test]
    fn the_diagonal_boundary_is_22_5_degrees() {
        // Just inside the diagonal sector, and just outside it.
        let pure = StickState::new(10_000, 4_100);
        let diagonal = StickState::new(10_000, 4_200);
        assert_eq!(pure.direction(1), Some(StickDirection::Right));
        assert_eq!(diagonal.direction(1), Some(StickDirection::UpRight));
    }

    #[test]
    fn the_deadzone_suppresses_small_deflections() {
        let small = StickState::new(3_000, 2_000);
        assert!(small.magnitude() < 7_849);
        assert_eq!(small.direction(7_849), None);
        assert_eq!(small.direction(1), Some(StickDirection::UpRight));

        let far = StickState::new(20_000, 0);
        assert_eq!(far.direction(7_849), Some(StickDirection::Right));
    }

    #[test]
    fn magnitude_is_clamped_at_full_scale() {
        // A diagonal at the rim reports more than 32767 on the raw vector.
        assert_eq!(StickState::new(32_767, 32_767).magnitude(), 32_767);
        // The corner `XInput` really can report, whose square overflows `i32`.
        assert_eq!(StickState::new(-32_768, -32_768).magnitude(), 32_767);
        assert_eq!(StickState::new(0, 0).magnitude(), 0);
        assert_eq!(StickState::new(3, 4).magnitude(), 5, "3-4-5 is exact");
    }

    #[test]
    fn effective_rescales_past_the_deadzone() {
        let deadzone = 8_000;
        assert_eq!(
            StickState::new(4_000, -4_000).effective(deadzone),
            (0, 0),
            "inside the deadzone nothing moves"
        );
        assert_eq!(
            StickState::new(8_000, 0).effective(deadzone),
            (0, 0),
            "the deadzone edge is exactly zero"
        );
        assert_eq!(
            StickState::new(32_767, 0).effective(deadzone),
            (32_767, 0),
            "a cardinal push at the rim reaches full speed"
        );
        // A corner is a square gate, so it reaches past the circle; it is
        // clamped to it rather than being allowed to outrun a round gate.
        let (x, y) = StickState::new(32_767, -32_767).effective(deadzone);
        assert_eq!((x.signum(), y.signum()), (1, -1), "the sign is preserved");
        let length = f64::from(x).hypot(f64::from(y));
        assert!(
            (32_000.0..33_000.0).contains(&length),
            "a corner stays on the rim, not beyond it: {length}"
        );

        let (x, y) = StickState::new(20_400, 0).effective(deadzone);
        assert!(
            (16_000..17_000).contains(&x),
            "halfway is about half speed: {x}"
        );
        assert_eq!(y, 0);
    }

    #[test]
    fn each_stick_is_read_separately() {
        let state = PadState {
            left_stick: StickState::new(1_000, 2_000),
            right_stick: StickState::new(-3_000, -4_000),
            ..PadState::default()
        };
        assert_eq!(state.stick(Stick::Left), StickState::new(1_000, 2_000));
        assert_eq!(state.stick(Stick::Right), StickState::new(-3_000, -4_000));
    }
}
