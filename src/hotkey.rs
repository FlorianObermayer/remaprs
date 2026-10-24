//! Global suspend/resume hotkey (Windows only).

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use crate::keys::KeyboardKey;

/// Poll cadence for the hotkey. Fast enough to feel instant, slow enough to be
/// invisible in a profiler.
const POLL: Duration = Duration::from_millis(16);

/// Watches `key` and sets `trigger` once per physical press.
///
/// `GetAsyncKeyState` polling is used instead of `RegisterHotKey` because a
/// registered hotkey is consumed system-wide, which would stop the game from
/// ever seeing that key.
///
/// The returned thread runs until the process exits.
///
/// # Errors
///
/// Returns the spawn failure rather than panicking: the caller can decide
/// whether a controller remapper is still useful without its kill switch.
#[must_use = "the returned handle keeps the watcher thread attached to the process"]
pub fn spawn(key: KeyboardKey, trigger: Arc<AtomicBool>) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("remaprs-hotkey".to_owned())
        .spawn(move || watch(key, &trigger))
}

fn watch(key: KeyboardKey, trigger: &AtomicBool) {
    let virtual_key = i32::from(key.vk);
    let mut was_down = false;

    loop {
        // SAFETY: GetAsyncKeyState has no preconditions and accepts any i32.
        // The high bit is set while the key is down, so the i16 is negative.
        let is_down = unsafe { GetAsyncKeyState(virtual_key) } < 0;

        if is_down && !was_down {
            trigger.store(true, Ordering::Relaxed);
        }
        was_down = is_down;

        thread::sleep(POLL);
    }
}
