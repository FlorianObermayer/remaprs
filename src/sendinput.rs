//! Synthetic keyboard and mouse input via `SendInput` (Windows only).

use std::io;

use anyhow::Result;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MapVirtualKeyW,
    SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::WHEEL_DELTA;

use crate::config::KeyMode;
use crate::keys::{Key, KeyboardKey, MouseButton};
use crate::output::KeyEmitter;

/// Sends input through the `SendInput` API.
///
/// `SendInput` reaches the foreground window only, and is subject to UIPI: a
/// non-elevated process cannot inject input into an elevated one.
#[derive(Debug, Clone, Copy)]
pub struct SendInputEmitter {
    mode: KeyMode,
}

impl SendInputEmitter {
    #[must_use]
    pub const fn new(mode: KeyMode) -> Self {
        Self { mode }
    }

    /// Posts one input event.
    fn send(input: INPUT) -> Result<()> {
        let size = i32::try_from(size_of::<INPUT>()).unwrap_or(i32::MAX);
        // SAFETY: `input` is fully initialised and `size` is its exact size.
        let sent = unsafe { SendInput(&[input], size) };
        if sent == 1 {
            Ok(())
        } else {
            Err(io::Error::last_os_error().into())
        }
    }

    fn keyboard(self, key: KeyboardKey, key_up: bool) -> Result<()> {
        Self::send(keyboard_event(key, self.mode, key_up))
    }
}

impl KeyEmitter for SendInputEmitter {
    fn press(&mut self, key: Key) -> Result<()> {
        match key {
            Key::Keyboard(keyboard) => self.keyboard(keyboard, false),
            Key::Mouse(button) => match mouse_event(button, false) {
                Some(input) => Self::send(input),
                // Wheel directions are a single event, not a held button.
                None => Ok(()),
            },
        }
    }

    fn release(&mut self, key: Key) -> Result<()> {
        match key {
            Key::Keyboard(keyboard) => self.keyboard(keyboard, true),
            Key::Mouse(button) => match mouse_event(button, true) {
                Some(input) => Self::send(input),
                None => Ok(()),
            },
        }
    }
}

fn keyboard_event(key: KeyboardKey, mode: KeyMode, key_up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }

    let mut virtual_key = VIRTUAL_KEY(0);
    let mut scan_code = 0_u16;

    match mode {
        KeyMode::Scancode => {
            // SAFETY: MapVirtualKeyW is a pure lookup with no preconditions.
            let mapped = unsafe { MapVirtualKeyW(u32::from(key.vk), MAPVK_VK_TO_VSC) };
            if mapped == 0 {
                // Some keys have no scan code; fall back so the press is not lost.
                virtual_key = VIRTUAL_KEY(key.vk);
            } else {
                flags |= KEYEVENTF_SCANCODE;
                scan_code = u16::try_from(mapped).unwrap_or_default();
            }
        }
        KeyMode::VirtualKey => virtual_key = VIRTUAL_KEY(key.vk),
    }

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: virtual_key,
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_event(button: MouseButton, key_up: bool) -> Option<INPUT> {
    let (flags, data) = match (button, key_up) {
        (MouseButton::Left, false) => (MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButton::Left, true) => (MOUSEEVENTF_LEFTUP, 0),
        (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTUP, 0),
        (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButton::WheelUp, false) => (MOUSEEVENTF_WHEEL, WHEEL_DELTA),
        // A negative scroll amount is expressed as an unsigned DWORD.
        (MouseButton::WheelDown, false) => (MOUSEEVENTF_WHEEL, 0_u32.wrapping_sub(WHEEL_DELTA)),
        (MouseButton::WheelUp | MouseButton::WheelDown, true) => return None,
    };

    Some(INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    })
}
