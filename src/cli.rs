//! Command-line surface and the rule that picks a configuration file.
//!
//! This module is deliberately free of platform code, so the flags, the
//! `config` subcommands and the lookup order can be tested on any host without
//! a controller attached. `main` stays the wiring: it opens log sinks, starts
//! Win32 backends and maps the results to an exit code.
//!
//! The lookup order is explicit flag, then `remaprs.yaml` next to the running
//! executable, then the built-in defaults. Lookup never consults the working
//! directory: a shortcut or a task runner sets it to something unrelated, and a
//! config that changes meaning depending on where it was started is a trap.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::config::Config;
use crate::keys::Key;

/// Name looked up in the executable's directory when no `--config` is given.
pub const DEFAULT_CONFIG_FILE: &str = "remaprs.yaml";

/// The commented example, embedded so `config init` cannot drift from it.
pub const STARTER_CONFIG: &str = include_str!("../examples/remaprs.yaml");

const AFTER_HELP: &str = "\
Configuration:
  `bindings` is the only required key; every other section has a default.
  Without --config, `remaprs.yaml` next to the executable is loaded when it
  exists. Pass --no-config to start from the built-in defaults instead.

  remaprs config init [FILE]   write a commented starter remaprs.yaml
  remaprs config check         validate the configuration this run would use

The annotated reference is examples/remaprs.yaml; README.md documents every key.";

/// Parsed command line.
// A clap parser is a flat bag of independent switches; folding them into enums
// would change the command line rather than the code.
#[allow(
    clippy::struct_excessive_bools,
    reason = "these are independent command-line switches, not a state machine"
)]
#[derive(Debug, Parser)]
#[command(
    name = "remaprs",
    // Two version flags on purpose: `-V` is clap's convention (`--help`'s own
    // synopsis advertises it), and `-v` is what people reach for first. Both
    // print the same line; neither is an alias of `--verbose`, which is why
    // verbosity is spelled `--log-level`.
    // `version` is the bare string, not "remaprs {version}": clap prepends the
    // command name itself, and passing both printed "remaprs remaprs 0.1.0".
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true,
    about = "Turns a held controller button or keyboard key into randomized keyboard, mouse or sequenced input",
    // Printed above the description so the version is the first thing `-h`
    // shows; `--version` alone is easy to forget when filing a bug report.
    before_help = concat!("remaprs ", env!("CARGO_PKG_VERSION")),
    long_about = None,
    after_help = AFTER_HELP,
)]
pub struct Cli {
    /// Print the version and exit.
    ///
    /// `-v` is accepted alongside clap's conventional `-V`; both print the same
    /// line, and neither is a `--verbose` switch (verbosity is `--log-level`).
    #[arg(
        short = 'V',
        long = "version",
        visible_short_alias = 'v',
        action = clap::ArgAction::Version
    )]
    version: Option<bool>,

    /// Maintenance commands that need no controller.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// YAML file describing the button mappings.
    ///
    /// Defaults to `remaprs.yaml` next to the executable, when that file exists.
    #[arg(short, long, value_name = "FILE", global = true)]
    pub config: Option<PathBuf>,

    /// Ignore any configuration file and use only the built-in defaults.
    #[arg(long, global = true, conflicts_with = "config")]
    pub no_config: bool,

    /// Xbox controller slot to read (0-3).
    #[arg(short, long, value_name = "INDEX", global = true)]
    pub device: Option<u32>,

    /// Add or replace a controller mapping, e.g. `A=SPACE:40-90:15-35:0`.
    ///
    /// The key is held for HOLD and released for PAUSE; both are optional and
    /// accept `40` or `40-90`.
    #[arg(long, value_name = "BUTTON=KEY[:PAUSE[:HOLD[:DELAY]]]", global = true)]
    pub set: Vec<String>,

    /// Add or replace a keyboard mapping, e.g. `Q=E:40-90:15-35:0`.
    #[arg(long, value_name = "KEY=KEY[:PAUSE[:HOLD[:DELAY]]]", global = true)]
    pub set_key: Vec<String>,

    /// Log the events that would be emitted, without sending any input.
    #[arg(long)]
    pub dry_run: bool,

    /// Print live controller input and exit on Ctrl+C.
    #[arg(long)]
    pub list_buttons: bool,

    /// Suspend/resume hotkey; `none` disables it.
    #[arg(long, value_name = "KEY", global = true)]
    pub toggle_hotkey: Option<String>,

    /// Ignore `safety.target_process` and emit regardless of what is focused.
    #[arg(long, global = true)]
    pub no_focus_guard: bool,

    /// Log verbosity: error, warn, info, debug or trace.
    #[arg(long, value_name = "LEVEL")]
    pub log_level: Option<String>,

    /// Append every log line to this file.
    ///
    /// Use it when the console closes together with the program, so the reason
    /// for an exit is still readable afterwards.
    #[arg(long, value_name = "FILE")]
    pub log_file: Option<PathBuf>,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create or validate a configuration file.
    Config {
        /// What to do with the configuration.
        #[command(subcommand)]
        action: ConfigCommand,
    },
}

/// Subcommands of `remaprs config`.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Write a commented starter configuration file.
    Init {
        /// Destination file. Defaults to `remaprs.yaml` next to the executable,
        /// which is where it is loaded from automatically.
        #[arg(value_name = "FILE")]
        path: Option<PathBuf>,

        /// Overwrite the file if it already exists.
        #[arg(long)]
        force: bool,

        /// Print the starter file to stdout instead of writing it.
        #[arg(long)]
        stdout: bool,
    },

    /// Parse and validate the configuration this run would use, then exit.
    ///
    /// Honours `--config`, the `remaprs.yaml` next to the executable and the
    /// `--set`, `--set-key`, `--device`, `--toggle-hotkey` and
    /// `--no-focus-guard` overrides. Nothing is sent to the system.
    Check,
}

/// Where the effective configuration came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// Named by `--config`.
    Explicit(PathBuf),
    /// Found next to the executable because no `--config` was given.
    AutoLoaded(PathBuf),
    /// Neither: the built-in defaults apply.
    None,
}

impl ConfigSource {
    /// The file backing this configuration, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Explicit(path) | Self::AutoLoaded(path) => Some(path),
            Self::None => None,
        }
    }

    /// A short label for reports, naming the defaults when no file is used.
    #[must_use]
    pub fn label(&self) -> String {
        self.path().map_or_else(
            || "<built-in defaults>".to_owned(),
            |path| path.display().to_string(),
        )
    }
}

/// The path auto-loading looks for, given an executable directory.
#[must_use]
pub fn candidate_path(exe_dir: &Path) -> PathBuf {
    exe_dir.join(DEFAULT_CONFIG_FILE)
}

/// Decides which configuration applies.
///
/// `is_file` is injected so the precedence rules are testable without touching
/// the filesystem. `no_config` wins over everything: it is the documented way to
/// reproduce a defaults-only run, and clap rejects combining it with `--config`.
#[must_use]
pub fn resolve_config_source(
    explicit: Option<&Path>,
    no_config: bool,
    exe_dir: &Path,
    is_file: impl Fn(&Path) -> bool,
) -> ConfigSource {
    if no_config {
        return ConfigSource::None;
    }
    if let Some(path) = explicit {
        return ConfigSource::Explicit(path.to_path_buf());
    }
    let candidate = candidate_path(exe_dir);
    if is_file(&candidate) {
        ConfigSource::AutoLoaded(candidate)
    } else {
        ConfigSource::None
    }
}

/// [`resolve_config_source`] with the real executable directory and filesystem.
///
/// # Errors
///
/// Fails only when the running executable has no readable location, which would
/// also make `--config` unusable.
pub fn resolve_config(explicit: Option<&Path>, no_config: bool) -> Result<ConfigSource> {
    let exe_dir = exe_dir()?;
    Ok(resolve_config_source(
        explicit,
        no_config,
        &exe_dir,
        Path::is_file,
    ))
}

/// Directory holding the running executable.
///
/// # Errors
///
/// Fails when the executable path cannot be queried or has no parent.
pub fn exe_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot locate the running executable")?;
    exe.parent()
        .map(Path::to_path_buf)
        .with_context(|| format!("`{}` has no parent directory", exe.display()))
}

/// Whether `path` is exactly the file auto-loading would pick.
///
/// Used to skip a hint that the user has already followed. Any failure counts as
/// "no", because the answer only decides whether to print advice.
#[must_use]
pub fn is_next_to_executable(path: &Path) -> bool {
    let Ok(exe_dir) = exe_dir() else {
        return false;
    };
    let Ok(written) = path.canonicalize() else {
        return false;
    };
    let Ok(expected) = candidate_path(&exe_dir).canonicalize() else {
        return false;
    };
    written == expected
}

/// Destination for `config init`.
///
/// Without an explicit path this is the file auto-loading would find, so a bare
/// `remaprs config init` is enough to make the next run use the written file.
///
/// # Errors
///
/// Fails when the executable location cannot be determined.
pub fn init_target(path: Option<&Path>) -> Result<PathBuf> {
    match path {
        Some(path) => Ok(path.to_path_buf()),
        None => Ok(candidate_path(&exe_dir()?)),
    }
}

/// Problems writing a starter configuration.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// The destination already exists and `--force` was not given.
    #[error("`{path}` already exists; pass --force to overwrite it")]
    Exists {
        /// Destination that was left untouched.
        path: PathBuf,
    },
    /// The file could not be created or written.
    #[error("cannot write `{path}`: {source}")]
    Write {
        /// Destination that failed.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}

/// Writes [`STARTER_CONFIG`] to `path`.
///
/// A new file is created with `create_new`, so an existing file is reported
/// rather than truncated even if it appears between a check and the write.
///
/// # Errors
///
/// Fails when the file exists and `force` is false, or when the write fails.
pub fn write_starter(path: &Path, force: bool) -> Result<(), InitError> {
    let write_error = |source| InitError::Write {
        path: path.to_path_buf(),
        source,
    };

    if force {
        return std::fs::write(path, STARTER_CONFIG).map_err(write_error);
    }

    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file
            .write_all(STARTER_CONFIG.as_bytes())
            .map_err(write_error),
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(InitError::Exists {
                path: path.to_path_buf(),
            })
        }
        Err(source) => Err(write_error(source)),
    }
}

impl Cli {
    /// Applies the command-line overrides on top of a loaded configuration.
    ///
    /// Kept here rather than in `main` so the precedence between a file and the
    /// flags is testable without starting a runner.
    ///
    /// # Errors
    ///
    /// Fails on a malformed `--set`, `--set-key` or `--toggle-hotkey` value.
    pub fn apply_overrides(&self, config: &mut Config) -> Result<()> {
        if let Some(index) = self.device {
            config.device.index = index;
        }
        if self.no_focus_guard {
            config.safety.require_focused = false;
        }
        if let Some(text) = &self.toggle_hotkey {
            config.hotkey.toggle = parse_hotkey(text)?;
        }
        for spec in &self.set {
            config
                .upsert_binding(spec)
                .map_err(|message| anyhow::anyhow!(message))?;
        }
        for spec in &self.set_key {
            config
                .upsert_key_binding(spec)
                .map_err(|message| anyhow::anyhow!(message))?;
        }
        Ok(())
    }
}

fn parse_hotkey(text: &str) -> Result<Option<Key>> {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("off") {
        return Ok(None);
    }

    let key = Key::parse(trimmed)?;
    if matches!(key, Key::Mouse(_)) {
        bail!("the toggle hotkey must be a keyboard key");
    }
    Ok(Some(key))
}
