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

    /// Moves the pointer by a relative amount, in pixels.
    ///
    /// # Errors
    ///
    /// Fails when the platform refuses the input.
    fn move_mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
        let _ = (dx, dy);
        Ok(())
    }

    /// Turns the wheel by a number of notches.
    ///
    /// # Errors
    ///
    /// Fails when the platform refuses the input.
    fn scroll(&mut self, horizontal: i32, vertical: i32) -> Result<()> {
        let _ = (horizontal, vertical);
        Ok(())
    }

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
                Action::MouseMove { dx, dy } => self.move_mouse(dx, dy)?,
                Action::Scroll {
                    horizontal,
                    vertical,
                } => self.scroll(horizontal, vertical)?,
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

    fn move_mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.events.push(Action::MouseMove { dx, dy });
        Ok(())
    }

    fn scroll(&mut self, horizontal: i32, vertical: i32) -> Result<()> {
        self.events.push(Action::Scroll {
            horizontal,
            vertical,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyEmitter, RecordingEmitter};
    use crate::engine::Action;
    use crate::keys::Key;

    #[test]
    fn records_every_action_kind_in_order() {
        let mut emitter = RecordingEmitter::default();
        let space = Key::parse("SPACE").expect("valid key");
        let actions = [
            Action::Press(space),
            Action::MouseMove { dx: 4, dy: -2 },
            Action::Scroll {
                horizontal: 0,
                vertical: 1,
            },
            Action::Release(space),
        ];
        emitter.emit(&actions).expect("recording cannot fail");
        assert_eq!(emitter.events(), actions);
    }
}
