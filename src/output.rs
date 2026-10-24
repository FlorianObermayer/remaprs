//! Where synthetic input goes.

use anyhow::Result;

use crate::engine::Action;
use crate::keys::Key;

/// Destinations for synthetic input.
pub trait KeyEmitter {
    /// Holds `key` down.
    ///
    /// # Errors
    ///
    /// Fails when the platform refuses the input.
    fn press(&mut self, key: Key) -> Result<()>;

    /// Lets `key` back up.
    ///
    /// # Errors
    ///
    /// Fails when the platform refuses the input.
    fn release(&mut self, key: Key) -> Result<()>;

    /// Applies a batch of actions in order.
    ///
    /// # Errors
    ///
    /// Fails on the first rejected action.
    fn emit(&mut self, actions: &[Action]) -> Result<()> {
        for action in actions {
            match *action {
                Action::Press(key) => self.press(key)?,
                Action::Release(key) => self.release(key)?,
            }
        }
        Ok(())
    }
}

/// Emitter that records instead of sending, used by `--dry-run` and tests.
#[derive(Debug, Default)]
pub struct RecordingEmitter {
    events: Vec<Action>,
}

impl RecordingEmitter {
    /// Everything recorded so far, in order.
    #[must_use]
    pub fn events(&self) -> &[Action] {
        &self.events
    }

    /// Removes and returns the recorded events.
    pub fn take(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.events)
    }
}

impl KeyEmitter for RecordingEmitter {
    fn press(&mut self, key: Key) -> Result<()> {
        self.events.push(Action::Press(key));
        Ok(())
    }

    fn release(&mut self, key: Key) -> Result<()> {
        self.events.push(Action::Release(key));
        Ok(())
    }
}
