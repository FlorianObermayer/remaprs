//! Foreground-process guard (Windows only).

use std::fmt;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows::core::PWSTR;

use crate::safety::FocusGuard;

/// Longest path [`QueryFullProcessImageNameW`] will return without truncation
/// on Windows 10 and later.
const MAX_PATH_WIDE: usize = 1024;

/// Emits only while the foreground window belongs to `expected`.
///
/// `expected` is a file name such as `Game.exe`, matched case-insensitively
/// against the full image path's final component. Any failure to identify the
/// window counts as "not focused", so a transient error fails safe: no input is
/// sent rather than input going to the wrong window.
pub struct ProcessFocusGuard {
    expected: String,
}

impl ProcessFocusGuard {
    #[must_use]
    pub fn new(expected: impl Into<String>) -> Self {
        Self {
            expected: expected.into(),
        }
    }
}

impl FocusGuard for ProcessFocusGuard {
    fn allows(&mut self) -> bool {
        foreground_process_name().is_some_and(|name| name.eq_ignore_ascii_case(&self.expected))
    }
}

impl fmt::Debug for ProcessFocusGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessFocusGuard")
            .field("expected", &self.expected)
            .finish()
    }
}

fn foreground_process_name() -> Option<String> {
    // SAFETY: every handle is closed before returning, the buffer is sized for
    // the documented maximum, and `length` is only read after a successful call.
    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }

        let mut process_id = 0_u32;
        GetWindowThreadProcessId(window, Some(&raw mut process_id));
        if process_id == 0 {
            return None;
        }

        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;

        let mut buffer = [0_u16; MAX_PATH_WIDE];
        let mut length = u32::try_from(buffer.len()).ok()?;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;

        let path = String::from_utf16_lossy(buffer.get(..usize::try_from(length).ok()?)?);
        path.rsplit('\\').next().map(str::to_owned)
    }
}
