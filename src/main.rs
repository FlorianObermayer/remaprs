//! Turns a held controller button or keyboard key into randomized keyboard,
//! mouse or sequenced input.
//!
//! The binary is Windows-only: it reads the pad through `XInput` and synthesizes
//! input with `SendInput`.
//!
//! `remaprs config init` and `remaprs config check` are handled before any log
//! sink, controller or timer exists, so inspecting a configuration never depends
//! on the machine having a pad plugged in.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Parser;
use remaprs::cli::{
    Cli, Command, ConfigCommand, ConfigSource, STARTER_CONFIG, init_target, is_next_to_executable,
    resolve_config, write_starter,
};
use remaprs::clock::SystemClock;
use remaprs::config::{Config, OutputSpec, SafetyConfig};
use remaprs::engine::{Binding, Engine, Output, Step};
use remaprs::gamepad::GamepadSource;
use remaprs::hotkey;
use remaprs::input::{KeyboardSource, NoKeyboard};
use remaprs::jitter::RandomJitter;
use remaprs::keys::{Key, KeyboardKey, Trigger};
use remaprs::logging::{self, LogSink};
use remaprs::output::{KeyEmitter, RecordingEmitter};
use remaprs::runner::Runner;
use remaprs::safety::{AllowAll, FocusGuard};
use remaprs::sendinput::SendInputEmitter;
use remaprs::winfocus::ProcessFocusGuard;
use remaprs::wininput::AsyncKeyboardSource;
use remaprs::xinput::XInputSource;
use tracing::{error, info, warn};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};

/// How often `--list-buttons` samples the pad.
const LIST_POLL_INTERVAL: Duration = Duration::from_millis(8);

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Handled before the log sink exists: `config check` must print its report
    // to the console even when `--log-file` points somewhere unwritable, and
    // neither command needs a log file to be opened for it.
    if let Some(Command::Config { action }) = &cli.command {
        return match config_command(&cli, action) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::FAILURE
            }
        };
    }

    let sink = match &cli.log_file {
        Some(path) => match LogSink::to_file(path) {
            Ok(sink) => sink,
            Err(error) => {
                eprintln!(
                    "error: cannot open `{}` for logging: {error}",
                    path.display()
                );
                return ExitCode::FAILURE;
            }
        },
        None => LogSink::stderr(),
    };

    // Installed before anything else can fail so a panic is always recorded.
    logging::install_panic_hook(&sink);

    match run(&cli, &sink) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let line = format!("error: {error:#}");
            eprintln!("{line}");
            sink.append_line(&line);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli, sink: &LogSink) -> Result<()> {
    let level = cli.log_level.clone().unwrap_or_else(|| {
        if cli.dry_run {
            "debug".to_owned()
        } else {
            "info".to_owned()
        }
    });
    logging::init(&level, sink);

    info!("remaprs {} starting", env!("CARGO_PKG_VERSION"));
    match sink.path() {
        Some(path) => info!("logging to `{}`", path.display()),
        None => debug_log_file_hint(),
    }

    let source = resolve_config(cli.config.as_deref(), cli.no_config)?;
    let mut config = match &source {
        ConfigSource::Explicit(path) => {
            info!("using config `{}`", path.display());
            Config::load(path)?
        }
        // Named separately from the explicit case so a leftover file cannot be
        // mistaken for one the user asked for.
        ConfigSource::AutoLoaded(path) => {
            info!("using config next to the executable `{}`", path.display());
            Config::load(path)?
        }
        ConfigSource::None => {
            info!("no configuration file; using built-in defaults");
            Config::default()
        }
    };
    cli.apply_overrides(&mut config)?;

    // `--list-buttons` never emits anything, so it does not need bindings.
    validate_or_bail(&config, cli.list_buttons)?;

    if cli.list_buttons {
        return list_buttons(&config);
    }

    start(cli.dry_run, &config)
}

/// Rejects a configuration with any problem, listing all of them at once.
fn validate_or_bail(config: &Config, device_only: bool) -> Result<()> {
    let issues = if device_only {
        config.validate_device()
    } else {
        config.validate()
    };
    if issues.is_empty() {
        return Ok(());
    }
    bail!("invalid configuration:\n  - {}", issues.join("\n  - "))
}

/// Runs a `remaprs config` subcommand.
///
/// `Ok(false)` means the command did its job and found the configuration
/// invalid: the report has already been printed, so only the exit code is left.
fn config_command(cli: &Cli, action: &ConfigCommand) -> Result<bool> {
    match action {
        ConfigCommand::Init {
            path,
            force,
            stdout,
        } => {
            init_config(path.as_deref(), *force, *stdout)?;
            Ok(true)
        }
        ConfigCommand::Check => check_config(cli),
    }
}

fn init_config(path: Option<&Path>, force: bool, stdout: bool) -> Result<()> {
    if stdout {
        print!("{STARTER_CONFIG}");
        return Ok(());
    }

    let target = init_target(path)?;
    write_starter(&target, force)?;
    println!("wrote `{}`", target.display());

    if is_next_to_executable(&target) {
        println!("next: remaprs config check");
    } else {
        println!("next: remaprs config check -c {}", target.display());
        println!("hint: name it `remaprs.yaml` next to remaprs.exe to load it without --config");
    }
    Ok(())
}

fn check_config(cli: &Cli) -> Result<bool> {
    let source = resolve_config(cli.config.as_deref(), cli.no_config)?;

    // Validating the defaults would only report the missing bindings, which
    // hides the real problem: there is nothing to check.
    let Some(path) = source.path().map(PathBuf::from) else {
        bail!(
            "no configuration to check; pass --config <FILE> or put `remaprs.yaml` next to \
             remaprs.exe"
        );
    };

    let mut config = Config::load(&path)?;
    cli.apply_overrides(&mut config)?;

    let issues = config.validate();
    if issues.is_empty() {
        println!("{}: ok", source.label());
        return Ok(true);
    }

    eprintln!("{}: {} problem(s)", source.label(), issues.len());
    for issue in &issues {
        eprintln!("  - {issue}");
    }
    Ok(false)
}

/// Tells the user how to keep a record, without cluttering normal output.
///
/// This is the answer to "it exited and I saw nothing": the console of a task
/// runner or a shortcut disappears together with the process.
fn debug_log_file_hint() {
    info!("hint: pass --log-file <FILE> to keep a record of this run");
}

fn list_buttons(config: &Config) -> Result<()> {
    let shutdown = Arc::new(AtomicBool::new(false));
    install_ctrlc(&shutdown)?;

    let mut source = XInputSource::new(config.device.index);
    info!(
        "listening on controller slot {}; press Ctrl+C to stop",
        config.device.index
    );

    let mut connected: Option<bool> = None;
    let mut previous = String::new();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        if let Some(pad) = source.poll()? {
            if connected != Some(true) {
                info!("controller connected");
                connected = Some(true);
            }
            let line = pad.describe(config.device.trigger_threshold);
            if line != previous {
                println!("{line}");
                previous = line;
            }
        } else if connected != Some(false) {
            warn!("controller not connected, waiting for it to appear");
            connected = Some(false);
            previous.clear();
        }
        std::thread::sleep(LIST_POLL_INTERVAL);
    }

    Ok(())
}

fn start(dry_run: bool, config: &Config) -> Result<()> {
    // `SendInput` and the scheduling loop both depend on the 1 ms system timer;
    // without it `sleep` rounds to ~15.6 ms and the jitter becomes meaningless.
    let _timer_resolution = TimerResolution::raise();

    log_plan(dry_run, config);

    let bindings: Vec<Binding> = config
        .bindings
        .iter()
        .map(|binding| Binding {
            from: binding.from,
            outputs: binding.outputs.iter().map(to_output).collect(),
        })
        .collect();

    let engine = Engine::new(
        bindings,
        config.device.trigger_threshold,
        Box::new(RandomJitter::new()),
    );

    let source: Box<dyn GamepadSource> = Box::new(XInputSource::new(config.device.index));
    let keyboard: Box<dyn KeyboardSource> = if config.uses_keyboard() {
        Box::new(AsyncKeyboardSource::new(config.keyboard_triggers()))
    } else {
        Box::new(NoKeyboard)
    };
    let emitter: Box<dyn KeyEmitter> = if dry_run {
        Box::new(RecordingEmitter::default())
    } else {
        Box::new(SendInputEmitter::new(config.output.key_mode))
    };
    let guard = focus_guard(&config.safety);

    let shutdown = Arc::new(AtomicBool::new(false));
    let toggle = Arc::new(AtomicBool::new(false));
    install_ctrlc(&shutdown)?;

    // A remapper without its kill switch is a footgun, so refuse to start rather
    // than run with the suspend hotkey silently missing.
    let _hotkey_thread = match config.hotkey.toggle.and_then(as_keyboard_key) {
        Some(key) => Some(
            hotkey::spawn(key, Arc::clone(&toggle))
                .context("failed to start the suspend-hotkey thread")?,
        ),
        None => None,
    };

    let clock = SystemClock::new();
    let mut runner = Runner {
        engine,
        source,
        keyboard,
        emitter,
        guard,
        poll_interval_ms: config.device.poll_interval_ms,
        release_on_exit: config.safety.release_on_exit,
    };

    let result = runner.run(&clock, &shutdown, &toggle);

    // Always state why the loop ended: after a silent exit this is the single
    // most useful line in the log.
    match &result {
        Ok(()) => info!("stopped: shutdown requested"),
        Err(error) => error!("stopped: {error:#}"),
    }
    result
}

fn log_plan(dry_run: bool, config: &Config) {
    if dry_run {
        info!("dry run: no input will be sent");
    }
    info!(
        "controller slot {}, poll every {} ms, trigger threshold {}",
        config.device.index, config.device.poll_interval_ms, config.device.trigger_threshold
    );
    for binding in &config.bindings {
        for output in &binding.outputs {
            describe_output(binding.from, output);
        }
    }

    if let Some(key) = config.hotkey.toggle {
        info!("press {key} to suspend or resume");
    } else {
        info!("suspend hotkey disabled");
    }

    match (&config.safety.target_process, config.safety.require_focused) {
        (Some(name), true) => info!("emitting only while `{name}` is focused"),
        (None, true) => warn!(
            "safety.require_focused is enabled but safety.target_process is empty; \
             the focus guard is inactive"
        ),
        _ => info!("focus guard disabled"),
    }

    if !config.safety.release_on_exit {
        warn!("safety.release_on_exit is disabled; an abrupt exit could leave a key held");
    }
}

fn focus_guard(safety: &SafetyConfig) -> Box<dyn FocusGuard> {
    match (&safety.target_process, safety.require_focused) {
        (Some(name), true) => Box::new(ProcessFocusGuard::new(name.clone())),
        _ => Box::new(AllowAll),
    }
}

/// Compiles one configured output into the engine's representation.
fn to_output(output: &OutputSpec) -> Output {
    match output {
        OutputSpec::Hold {
            key,
            duration_ms,
            initial_delay_ms,
        } => Output::Hold {
            key: *key,
            duration: *duration_ms,
            initial_delay: *initial_delay_ms,
        },
        OutputSpec::Sequence {
            steps,
            repeat,
            initial_delay_ms,
        } => Output::Sequence {
            steps: steps
                .iter()
                .map(|step| Step {
                    key: step.key,
                    hold: step.hold_ms,
                    pause: step.pause_ms,
                })
                .collect(),
            repeat: *repeat,
            initial_delay: *initial_delay_ms,
        },
    }
}

/// Logs one output so `--dry-run` shows the whole plan.
fn describe_output(from: Trigger, output: &OutputSpec) {
    let delay = output.initial_delay_ms();
    match output {
        OutputSpec::Hold {
            key, duration_ms, ..
        } => {
            if let Some(duration) = duration_ms {
                info!("{from} -> {key} held {duration} ms, starting after {delay} ms");
            } else {
                info!("{from} -> {key} held while the trigger is down (after {delay} ms)");
            }
        }
        OutputSpec::Sequence { steps, repeat, .. } => {
            let keys: Vec<String> = steps
                .iter()
                .map(|step| {
                    format!(
                        "{} {}-{}/{}-{} ms",
                        step.key,
                        step.hold_ms.min,
                        step.hold_ms.max,
                        step.pause_ms.min,
                        step.pause_ms.max
                    )
                })
                .collect();
            let looped = if *repeat { ", looping" } else { "" };
            info!(
                "{from} -> sequence [{}] starting after {delay} ms{looped}",
                keys.join(" -> ")
            );
        }
    }
}

fn as_keyboard_key(key: Key) -> Option<KeyboardKey> {
    match key {
        Key::Keyboard(keyboard) => Some(keyboard),
        Key::Mouse(_) => None,
    }
}

fn install_ctrlc(shutdown: &Arc<AtomicBool>) -> Result<()> {
    let flag = Arc::clone(shutdown);
    ctrlc::set_handler(move || {
        flag.store(true, Ordering::Relaxed);
    })
    .context("failed to install the Ctrl+C handler")
}

/// Raises the system timer resolution for the lifetime of the value.
struct TimerResolution;

impl TimerResolution {
    fn raise() -> Self {
        // SAFETY: timeBeginPeriod has no preconditions. The matching
        // timeEndPeriod call runs in `drop`.
        unsafe {
            timeBeginPeriod(1);
        }
        Self
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        // SAFETY: balances the single timeBeginPeriod(1) call above.
        unsafe {
            timeEndPeriod(1);
        }
    }
}
