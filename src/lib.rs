//! Library half of `remaprs`.
//!
//! The crate is split so that everything that decides *what* to emit is free of
//! platform code and can therefore be tested without a controller attached:
//!
//! * [`cli`] parses the command line and decides which config file applies.
//! * [`config`] turns YAML and command-line text into validated mappings.
//! * [`engine`] is the state machine that turns a held button or key into a
//!   stream of randomized press/release pairs.
//! * [`input`] merges controller and keyboard state into one frame.
//! * [`logging`] sends log lines to stderr and, optionally, a file.
//! * [`runner`] drives polling, emission and scheduling in a single thread.
//!
//! Only [`xinput`], [`wininput`], [`sendinput`], [`winfocus`] and [`hotkey`]
//! talk to Win32, and they are compiled only on Windows.

pub mod cli;
pub mod clock;
pub mod config;
pub mod engine;
pub mod gamepad;
pub mod input;
pub mod jitter;
pub mod keys;
pub mod logging;
pub mod output;
pub mod runner;
pub mod safety;

#[cfg(windows)]
pub mod hotkey;
#[cfg(windows)]
pub mod sendinput;
#[cfg(windows)]
pub mod winfocus;
#[cfg(windows)]
pub mod wininput;
#[cfg(windows)]
pub mod xinput;
