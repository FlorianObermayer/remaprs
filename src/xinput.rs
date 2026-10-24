//! `XInput` polling backend (Windows only).

use anyhow::{Result, anyhow};
use windows::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};

use crate::gamepad::{GamepadSource, PadState};

/// `ERROR_DEVICE_NOT_CONNECTED`, the error `XInputGetState` reports for an
/// empty slot. It is an ordinary Win32 error code, not an `HRESULT`.
const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;

/// Reads one `XInput` slot.
#[derive(Debug, Clone, Copy)]
pub struct XInputSource {
    slot: u32,
}

impl XInputSource {
    /// Wraps `slot`, which must be 0-3.
    #[must_use]
    pub const fn new(slot: u32) -> Self {
        Self { slot }
    }
}

impl GamepadSource for XInputSource {
    fn poll(&mut self) -> Result<Option<PadState>> {
        let mut state = XINPUT_STATE::default();

        // SAFETY: `state` is a valid, properly aligned XINPUT_STATE that lives
        // for the duration of the call, and `slot` was validated against the
        // documented 0-3 range by `Config::validate`.
        let result = unsafe { XInputGetState(self.slot, &raw mut state) };

        match result {
            0 => Ok(Some(PadState {
                buttons: state.Gamepad.wButtons.0,
                left_trigger: state.Gamepad.bLeftTrigger,
                right_trigger: state.Gamepad.bRightTrigger,
            })),
            ERROR_DEVICE_NOT_CONNECTED => Ok(None),
            code => Err(anyhow!("XInputGetState failed with error {code}")),
        }
    }
}
