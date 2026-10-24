//! Decides whether synthetic input is allowed right now.

/// Gate evaluated before every press.
pub trait FocusGuard: Send {
    /// Returns `true` when input may be emitted.
    fn allows(&mut self) -> bool;
}

/// Guard used when no target process is configured.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAll;

impl FocusGuard for AllowAll {
    fn allows(&mut self) -> bool {
        true
    }
}
