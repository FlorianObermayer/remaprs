//! Keyboard recording via `GetAsyncKeyState` (Windows only).

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use crate::input::{KeyboardSource, KeyboardState};
use crate::keys::KeyboardKey;

/// Reads the configured trigger keys with `GetAsyncKeyState`.
///
/// Only the configured keys are polled: a frame runs every few milliseconds, so
/// querying all 256 virtual keys would waste time for no benefit.
///
/// `GetAsyncKeyState` is used instead of `RegisterHotKey` because a registered
/// hotkey is consumed system-wide, which would stop the game from ever seeing
/// that key.
#[derive(Debug, Clone, Default)]
pub struct AsyncKeyboardSource {
    keys: Vec<KeyboardKey>,
}

impl AsyncKeyboardSource {
    /// Polls exactly `keys`, with duplicates removed.
    #[must_use]
    pub fn new(keys: impl IntoIterator<Item = KeyboardKey>) -> Self {
        let mut keys: Vec<KeyboardKey> = keys.into_iter().collect();
        keys.sort_unstable();
        keys.dedup();
        Self { keys }
    }
}

impl KeyboardSource for AsyncKeyboardSource {
    fn poll(&mut self) -> KeyboardState {
        let mut state = KeyboardState::default();

        for key in &self.keys {
            // SAFETY: GetAsyncKeyState has no preconditions and accepts any
            // i32. The high bit is set while the key is down, so the i16 is
            // negative; zero means "not down" and is a normal result.
            if unsafe { GetAsyncKeyState(i32::from(key.vk)) } < 0 {
                state.set(*key);
            }
        }

        state
    }
}
