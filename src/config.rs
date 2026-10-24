//! YAML configuration plus the command-line overrides that sit on top of it.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::{self, Deserializer};

use crate::keys::{Button, Key, KeyboardKey, Trigger};

/// Schema version understood by this build.
pub const CURRENT_VERSION: u32 = 1;
/// Default gap between one synthetic press being released and the next starting.
pub const DEFAULT_PAUSE: JitterRange = JitterRange::new(40, 90);
/// Default time a synthetic key stays down.
pub const DEFAULT_HOLD: JitterRange = JitterRange::new(15, 35);
/// Default delay between the trigger going down and the first press.
pub const DEFAULT_INITIAL_DELAY: JitterRange = JitterRange::fixed(0);
/// Default controller poll cadence.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 4;
/// Default analog trigger threshold; a trigger counts as pressed at or above it.
pub const DEFAULT_TRIGGER_THRESHOLD: u8 = 30;
/// Default suspend/resume hotkey (`F12`).
pub const DEFAULT_TOGGLE_KEY: Key = Key::Keyboard(KeyboardKey::new(0x7B, false));

/// A delay range. A bare number means "always this value".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitterRange {
    /// Smallest delay in milliseconds.
    pub min: u64,
    /// Largest delay in milliseconds.
    pub max: u64,
}

impl JitterRange {
    #[must_use]
    pub const fn new(min: u64, max: u64) -> Self {
        Self { min, max }
    }

    #[must_use]
    pub const fn fixed(value: u64) -> Self {
        Self {
            min: value,
            max: value,
        }
    }
}

impl fmt::Display for JitterRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.min == self.max {
            write!(formatter, "{}", self.min)
        } else {
            write!(formatter, "{}-{}", self.min, self.max)
        }
    }
}

impl<'de> Deserialize<'de> for JitterRange {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Single(u64),
            Pair([u64; 2]),
        }

        match Repr::deserialize(deserializer)? {
            Repr::Single(value) => Ok(Self::fixed(value)),
            Repr::Pair([min, max]) => Ok(Self::new(min, max)),
        }
    }
}

/// Which Win32 input message to synthesise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyMode {
    /// Send scan codes. Required by games that read `DirectInput` or raw input.
    #[default]
    Scancode,
    /// Send virtual-key codes. Useful when scan-code translation misbehaves.
    VirtualKey,
}

/// What an output does while its trigger is held.
///
/// There is deliberately no separate `turbo` mode: a one-step sequence that
/// repeats *is* a spam. Keeping a single code path means every output shares
/// the same timing rules and the same stall recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    /// Press a key, release it, pause, and repeat: either forever or through a
    /// fixed list of steps.
    #[default]
    Sequence,
    /// Press the key once and keep it down until the trigger is released, or
    /// for as long as `hold_ms` says.
    Hold,
}

impl OutputMode {
    /// Lower-case name used in diagnostics and in the YAML file.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sequence => "sequence",
            Self::Hold => "hold",
        }
    }
}

/// Where a binding's trigger comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputSource {
    /// A button on the `XInput` pad.
    #[default]
    Controller,
    /// A key on the physical keyboard.
    Keyboard,
}

/// One press in a [`OutputSpec::Sequence`].
#[derive(Debug, Clone, Copy)]
pub struct StepSpec {
    /// Key or mouse button to emit for this step.
    pub key: Key,
    /// How long the key stays down.
    pub hold_ms: JitterRange,
    /// How long the key stays up before the next step starts.
    pub pause_ms: JitterRange,
}

/// One key or sequence emitted while a trigger is held.
#[derive(Debug, Clone)]
pub enum OutputSpec {
    /// Press one key and keep it down.
    Hold {
        /// Key or mouse button to emit.
        key: Key,
        /// How long the key stays down.
        ///
        /// `None` keeps it down until the trigger is released.
        duration_ms: Option<JitterRange>,
        /// Time from the trigger going down to the press.
        initial_delay_ms: JitterRange,
    },
    /// Play a list of press/hold/pause steps.
    Sequence {
        /// Steps, in order. Must not be empty.
        steps: Vec<StepSpec>,
        /// Restart from the first step after the last one finishes.
        repeat: bool,
        /// Time from the trigger going down to the first press.
        initial_delay_ms: JitterRange,
    },
}

impl OutputSpec {
    /// A single repeating step, which is what the command line produces.
    ///
    /// This is the concise spelling of a plain spam: press at t0, release after
    /// `hold_ms`, press again after `pause_ms`, and so on.
    #[must_use]
    pub fn one_step(
        key: Key,
        hold_ms: JitterRange,
        pause_ms: JitterRange,
        initial_delay_ms: JitterRange,
    ) -> Self {
        Self::Sequence {
            steps: vec![StepSpec {
                key,
                hold_ms,
                pause_ms,
            }],
            repeat: true,
            initial_delay_ms,
        }
    }

    /// The mode this output runs in.
    #[must_use]
    pub const fn mode(&self) -> OutputMode {
        match self {
            Self::Hold { .. } => OutputMode::Hold,
            Self::Sequence { .. } => OutputMode::Sequence,
        }
    }

    /// Time from the trigger going down to this output's first action.
    #[must_use]
    pub const fn initial_delay_ms(&self) -> JitterRange {
        match self {
            Self::Hold {
                initial_delay_ms, ..
            }
            | Self::Sequence {
                initial_delay_ms, ..
            } => *initial_delay_ms,
        }
    }

    /// Every key this output can emit, in the order it first appears.
    ///
    /// A sequence may repeat a key on purpose, so duplicates inside one output
    /// are collapsed rather than reported.
    #[must_use]
    pub fn keys(&self) -> Vec<Key> {
        let mut keys = Vec::new();
        match self {
            Self::Hold { key, .. } => keys.push(*key),
            Self::Sequence { steps, .. } => {
                for step in steps {
                    if !keys.contains(&step.key) {
                        keys.push(step.key);
                    }
                }
            }
        }
        keys
    }
}

/// One trigger-to-keys mapping, from the pad or the keyboard.
#[derive(Debug, Clone)]
pub struct BindingConfig {
    /// Button or key that drives the mapping.
    pub from: Trigger,
    /// Outputs emitted while the trigger is held. They run in parallel.
    pub outputs: Vec<OutputSpec>,
}

impl BindingConfig {
    /// Parses a controller `--set` value such as `A=SPACE:40-90:15-35`.
    ///
    /// The key is pressed for `hold` and then released for `pause`, so one full
    /// cycle is `hold + pause` long. Both are optional and accept either a
    /// single value or a `min-max` pair.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message describing the malformed part.
    pub fn parse_spec(spec: &str) -> Result<Self, String> {
        Self::parse(spec, InputSource::Controller)
    }

    /// Parses a keyboard `--set-key` value such as `Q=E:40-90:15-35`.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message describing the malformed part.
    pub fn parse_key_spec(spec: &str) -> Result<Self, String> {
        Self::parse(spec, InputSource::Keyboard)
    }

    fn parse(spec: &str, source: InputSource) -> Result<Self, String> {
        let (from_text, rest) = spec
            .split_once('=')
            .ok_or_else(|| format!("expected FROM=KEY, got `{spec}`"))?;

        let from = parse_trigger(source, from_text.trim())
            .map_err(|error| format!("in `{spec}`: {error}"))?;

        let mut parts = rest.split(':');
        let key_text = parts.next().unwrap_or_default();
        let pause_text = parts.next();
        let hold_text = parts.next();
        let delay_text = parts.next();
        if parts.next().is_some() {
            return Err(format!("too many `:` separators in `{spec}`"));
        }

        let key = Key::parse(key_text).map_err(|error| format!("in `{spec}`: {error}"))?;

        let pause_ms = match pause_text {
            Some(text) => {
                parse_range(text, "pause").map_err(|error| format!("in `{spec}`: {error}"))?
            }
            None => DEFAULT_PAUSE,
        };
        let hold_ms = match hold_text {
            Some(text) => {
                parse_range(text, "hold").map_err(|error| format!("in `{spec}`: {error}"))?
            }
            None => DEFAULT_HOLD,
        };
        let initial_delay_ms = match delay_text {
            Some(text) => parse_range(text, "initial delay")
                .map_err(|error| format!("in `{spec}`: {error}"))?,
            None => DEFAULT_INITIAL_DELAY,
        };

        Ok(Self {
            from,
            outputs: vec![OutputSpec::one_step(
                key,
                hold_ms,
                pause_ms,
                initial_delay_ms,
            )],
        })
    }
}

/// One entry of `bindings[].outputs[]`. Every field is optional so a binding
/// can supply the defaults.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputRepr {
    key: Option<Key>,
    mode: Option<OutputMode>,
    pause_ms: Option<JitterRange>,
    hold_ms: Option<JitterRange>,
    initial_delay_ms: Option<JitterRange>,
    repeat: Option<bool>,
    #[serde(default)]
    steps: Vec<StepRepr>,
}

/// One entry of `outputs[].steps[]`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StepRepr {
    key: Key,
    hold_ms: Option<JitterRange>,
    pause_ms: Option<JitterRange>,
}

/// Binding-level values inherited by every output and step.
#[derive(Debug, Default)]
struct Defaults {
    mode: Option<OutputMode>,
    pause_ms: Option<JitterRange>,
    hold_ms: Option<JitterRange>,
    initial_delay_ms: Option<JitterRange>,
    repeat: Option<bool>,
}

impl<'de> Deserialize<'de> for BindingConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            #[serde(default)]
            source: InputSource,
            from: String,
            /// Shorthand for a single output.
            key: Option<Key>,
            /// Every output of the binding, on top of `key` when both are given.
            #[serde(default)]
            outputs: Vec<OutputRepr>,
            // Binding-level defaults for the outputs below.
            mode: Option<OutputMode>,
            pause_ms: Option<JitterRange>,
            hold_ms: Option<JitterRange>,
            initial_delay_ms: Option<JitterRange>,
            repeat: Option<bool>,
        }

        let repr = Repr::deserialize(deserializer)?;
        let from = parse_trigger(repr.source, &repr.from).map_err(de::Error::custom)?;

        let defaults = Defaults {
            mode: repr.mode,
            pause_ms: repr.pause_ms,
            hold_ms: repr.hold_ms,
            initial_delay_ms: repr.initial_delay_ms,
            repeat: repr.repeat,
        };

        let mut outputs = Vec::with_capacity(repr.outputs.len() + usize::from(repr.key.is_some()));

        if let Some(key) = repr.key {
            let shorthand = OutputRepr {
                key: Some(key),
                mode: repr.mode,
                pause_ms: None,
                hold_ms: None,
                initial_delay_ms: None,
                repeat: None,
                steps: Vec::new(),
            };
            outputs.push(build_output(&shorthand, &defaults).map_err(de::Error::custom)?);
        }
        for output in &repr.outputs {
            outputs.push(build_output(output, &defaults).map_err(de::Error::custom)?);
        }

        if outputs.is_empty() {
            return Err(de::Error::custom(
                "a binding needs `key` or at least one entry in `outputs`",
            ));
        }

        Ok(Self { from, outputs })
    }
}

/// Turns one deserialised output entry into a concrete [`OutputSpec`].
fn build_output(repr: &OutputRepr, defaults: &Defaults) -> Result<OutputSpec, String> {
    let mode = repr.mode.or(defaults.mode).unwrap_or_default();
    let initial_delay_ms = repr
        .initial_delay_ms
        .or(defaults.initial_delay_ms)
        .unwrap_or(DEFAULT_INITIAL_DELAY);
    let pause_ms = repr.pause_ms.or(defaults.pause_ms);
    let hold_ms = repr.hold_ms.or(defaults.hold_ms);

    match mode {
        OutputMode::Hold => Ok(OutputSpec::Hold {
            key: repr
                .key
                .ok_or_else(|| "a `hold` output needs a `key`".to_owned())?,
            // No default: an omitted duration means "until the trigger is
            // released", not "15-35 ms".
            duration_ms: hold_ms,
            initial_delay_ms,
        }),
        OutputMode::Sequence => {
            let mut steps: Vec<StepSpec> = repr
                .steps
                .iter()
                .map(|step| StepSpec {
                    key: step.key,
                    hold_ms: step.hold_ms.or(hold_ms).unwrap_or(DEFAULT_HOLD),
                    pause_ms: step.pause_ms.or(pause_ms).unwrap_or(DEFAULT_PAUSE),
                })
                .collect();

            // `key: X` with no `steps` is a single repeating step, which is the
            // concise way to write a plain spam.
            if steps.is_empty() {
                let key = repr.key.ok_or_else(|| {
                    "a `sequence` output needs `key` or at least one entry in `steps`".to_owned()
                })?;
                steps.push(StepSpec {
                    key,
                    hold_ms: hold_ms.unwrap_or(DEFAULT_HOLD),
                    pause_ms: pause_ms.unwrap_or(DEFAULT_PAUSE),
                });
            }

            Ok(OutputSpec::Sequence {
                steps,
                repeat: repr.repeat.or(defaults.repeat).unwrap_or(true),
                initial_delay_ms,
            })
        }
    }
}

/// Parses the `from` half of a binding according to its `source`.
fn parse_trigger(source: InputSource, text: &str) -> Result<Trigger, String> {
    match source {
        InputSource::Controller => Button::parse(text)
            .map(Trigger::Button)
            .map_err(|error| error.to_string()),
        InputSource::Keyboard => match Key::parse(text).map_err(|error| error.to_string())? {
            Key::Keyboard(key) => Ok(Trigger::Keyboard(key)),
            Key::Mouse(_) => Err(format!(
                "`{text}` is a mouse button; a keyboard binding needs a keyboard key"
            )),
        },
    }
}

fn parse_range(text: &str, field: &str) -> Result<JitterRange, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("empty {field}"));
    }
    match text.split_once('-') {
        Some((min_text, max_text)) => {
            let min = parse_millis(min_text, field)?;
            let max = parse_millis(max_text, field)?;
            Ok(JitterRange::new(min, max))
        }
        None => Ok(JitterRange::fixed(parse_millis(text, field)?)),
    }
}

fn parse_millis(text: &str, field: &str) -> Result<u64, String> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| format!("`{}` is not a valid {field} value", text.trim()))
}

/// Controller selection and polling behaviour.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    /// `XInput` slot, 0-3.
    #[serde(default)]
    pub index: u32,
    /// How often the pad is polled, in milliseconds.
    #[serde(default = "default_poll_interval")]
    pub poll_interval_ms: u64,
    /// Analog triggers count as pressed at or above this value.
    #[serde(default = "default_trigger_threshold")]
    pub trigger_threshold: u8,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            index: 0,
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            trigger_threshold: DEFAULT_TRIGGER_THRESHOLD,
        }
    }
}

fn default_poll_interval() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

fn default_trigger_threshold() -> u8 {
    DEFAULT_TRIGGER_THRESHOLD
}

/// How synthetic input is delivered.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    /// Scan codes versus virtual-key codes.
    #[serde(default)]
    pub key_mode: KeyMode,
}

/// Global suspend/resume shortcut.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HotkeyConfig {
    /// Keyboard key that toggles emission. `none` (or `~`) disables the hotkey.
    #[serde(default = "default_toggle", deserialize_with = "deserialize_toggle")]
    pub toggle: Option<Key>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            toggle: default_toggle(),
        }
    }
}

// Serde requires the default function to return the field type, which is
// `Option<Key>`; the wrapper is part of the schema, not redundant.
#[allow(clippy::unnecessary_wraps)]
fn default_toggle() -> Option<Key> {
    Some(DEFAULT_TOGGLE_KEY)
}

/// Accepts a key name, the words `none`/`off`, or YAML null.
fn deserialize_toggle<'de, D>(deserializer: D) -> Result<Option<Key>, D::Error>
where
    D: Deserializer<'de>,
{
    let text = Option::<String>::deserialize(deserializer)?;
    match text {
        None => Ok(None),
        Some(text) => {
            let trimmed = text.trim();
            if trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("off") {
                return Ok(None);
            }
            Key::parse(trimmed)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
    }
}

/// Guard rails around emitting input.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyConfig {
    /// Only emit while this executable owns the foreground window.
    #[serde(default)]
    pub target_process: Option<String>,
    /// Master switch for the focus guard. Has no effect without a target.
    #[serde(default = "default_true")]
    pub require_focused: bool,
    /// Release every synthetic key when the process exits.
    #[serde(default = "default_true")]
    pub release_on_exit: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            target_process: None,
            require_focused: true,
            release_on_exit: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Root of the configuration file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Schema version; must equal [`CURRENT_VERSION`].
    #[serde(default = "default_version")]
    pub version: u32,
    /// Controller selection.
    #[serde(default)]
    pub device: DeviceConfig,
    /// Output encoding.
    #[serde(default)]
    pub output: OutputConfig,
    /// Suspend/resume shortcut.
    #[serde(default)]
    pub hotkey: HotkeyConfig,
    /// Guard rails.
    #[serde(default)]
    pub safety: SafetyConfig,
    /// Button mappings.
    #[serde(default)]
    pub bindings: Vec<BindingConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            device: DeviceConfig::default(),
            output: OutputConfig::default(),
            hotkey: HotkeyConfig::default(),
            safety: SafetyConfig::default(),
            bindings: Vec::new(),
        }
    }
}

fn default_version() -> u32 {
    CURRENT_VERSION
}

impl Config {
    /// Reads and parses a YAML file.
    ///
    /// # Errors
    ///
    /// Fails when the file cannot be read or is not valid YAML for this schema.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        serde_yaml_ng::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Parses YAML already in memory.
    ///
    /// # Errors
    ///
    /// Fails when the text does not match the schema.
    pub fn from_yaml(text: &str) -> Result<Self, ConfigError> {
        Ok(serde_yaml_ng::from_str(text)?)
    }

    /// Whether any binding is driven by the keyboard.
    #[must_use]
    pub fn uses_keyboard(&self) -> bool {
        self.bindings
            .iter()
            .any(|binding| binding.from.is_keyboard())
    }

    /// Every keyboard key used as a trigger, without duplicates.
    ///
    /// Polling only these keys keeps the input read cheap; a full 256-key sweep
    /// would cost more than the pad read it accompanies.
    #[must_use]
    pub fn keyboard_triggers(&self) -> Vec<KeyboardKey> {
        let mut keys: Vec<KeyboardKey> = self
            .bindings
            .iter()
            .filter_map(|binding| match binding.from {
                Trigger::Keyboard(key) => Some(key),
                Trigger::Button(_) => None,
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// Whether any binding is driven by the controller.
    #[must_use]
    pub fn uses_controller(&self) -> bool {
        self.bindings
            .iter()
            .any(|binding| !binding.from.is_keyboard())
    }

    /// Applies a `--set` override, replacing any existing mapping for the
    /// controller button.
    ///
    /// # Errors
    ///
    /// Fails on a malformed spec.
    pub fn upsert_binding(&mut self, spec: &str) -> Result<(), String> {
        self.upsert(BindingConfig::parse_spec(spec)?);
        Ok(())
    }

    /// Applies a `--set-key` override, replacing any existing mapping for the
    /// keyboard key.
    ///
    /// # Errors
    ///
    /// Fails on a malformed spec.
    pub fn upsert_key_binding(&mut self, spec: &str) -> Result<(), String> {
        self.upsert(BindingConfig::parse_key_spec(spec)?);
        Ok(())
    }

    fn upsert(&mut self, binding: BindingConfig) {
        if let Some(existing) = self
            .bindings
            .iter_mut()
            .find(|candidate| candidate.from == binding.from)
        {
            *existing = binding;
        } else {
            self.bindings.push(binding);
        }
    }

    /// Problems that matter even when no binding will run, such as for
    /// `--list-buttons`.
    #[must_use]
    pub fn validate_device(&self) -> Vec<String> {
        let mut issues = Vec::new();

        if self.version != CURRENT_VERSION {
            issues.push(format!(
                "version {} is not supported; this build understands version {CURRENT_VERSION}",
                self.version
            ));
        }
        if self.device.index > 3 {
            issues.push(format!(
                "device.index {} is out of range; XInput exposes slots 0-3",
                self.device.index
            ));
        }
        if self.device.poll_interval_ms == 0 {
            issues.push("device.poll_interval_ms must be at least 1".to_owned());
        }
        if self.device.poll_interval_ms > 50 {
            issues.push(format!(
                "device.poll_interval_ms {} is too high; use 1-50 so button releases are noticed quickly",
                self.device.poll_interval_ms
            ));
        }
        if self.device.trigger_threshold == 0 {
            issues.push("device.trigger_threshold must be at least 1".to_owned());
        }

        issues
    }

    /// Returns every problem found, so the user can fix them in one pass.
    #[must_use]
    pub fn validate(&self) -> Vec<String> {
        let mut issues = self.validate_device();

        if self.bindings.is_empty() {
            issues.push(
                "no bindings configured; add one to the config file, pass --set BUTTON=KEY or \
                 --set-key KEY=KEY"
                    .to_owned(),
            );
        }

        let mut seen_triggers: HashMap<Trigger, usize> = HashMap::new();
        let mut seen_keys: HashMap<Key, usize> = HashMap::new();
        let mut keyboard_triggers: HashMap<KeyboardKey, usize> = HashMap::new();

        for (offset, binding) in self.bindings.iter().enumerate() {
            let number = offset + 1;

            if binding.outputs.is_empty() {
                issues.push(format!("binding {number}: no keys configured"));
            }

            if let Some(first) = seen_triggers.insert(binding.from, number) {
                issues.push(format!(
                    "binding {number}: {} is already mapped by binding {first}",
                    binding.from
                ));
            }
            if let Trigger::Keyboard(key) = binding.from {
                keyboard_triggers.insert(key, number);
            }

            for output in &binding.outputs {
                for key in output.keys() {
                    if let Some(first) = seen_keys.insert(key, number) {
                        if first == number {
                            issues.push(format!(
                                "binding {number}: key {key} is emitted by more than one output"
                            ));
                        } else {
                            issues.push(format!(
                                "binding {number}: key {key} is already emitted by binding {first}"
                            ));
                        }
                    }
                }
                check_output(&mut issues, number, output);
            }
        }

        // Keyboard triggers are read with `GetAsyncKeyState`, which also reports
        // our own `SendInput` presses. An emitted key that is also a trigger
        // would therefore feed the binding its own output.
        for (offset, binding) in self.bindings.iter().enumerate() {
            let number = offset + 1;
            for output in &binding.outputs {
                for key in output.keys() {
                    if let Key::Keyboard(keyboard) = key
                        && let Some(first) = keyboard_triggers.get(&keyboard)
                    {
                        issues.push(format!(
                            "binding {number}: key {key} is emitted but is also the keyboard \
                             trigger of binding {first}; the synthetic press would re-trigger it"
                        ));
                    }
                }
            }
        }

        if let Some(toggle) = self.hotkey.toggle {
            if matches!(toggle, Key::Mouse(_)) {
                issues.push("hotkey.toggle must be a keyboard key".to_owned());
            }
            if let Some((_, first)) = seen_keys.get_key_value(&toggle) {
                issues.push(format!(
                    "hotkey.toggle {toggle} collides with binding {first}; the hotkey would \
                     intercept its own output"
                ));
            }
            if let Key::Keyboard(key) = toggle
                && let Some(first) = keyboard_triggers.get(&key)
            {
                issues.push(format!(
                    "hotkey.toggle {toggle} is also the keyboard trigger of binding {first}; \
                     pressing it would toggle instead of firing"
                ));
            }
        }

        issues
    }
}

/// Checks the timing of one output.
///
/// A `hold` needs a sensible duration, and a `sequence` a valid hold and pause
/// for every step.
fn check_output(issues: &mut Vec<String>, number: usize, output: &OutputSpec) {
    let label = output.mode().name();
    check_delay(
        issues,
        number,
        &format!("{label} initial_delay_ms"),
        output.initial_delay_ms(),
    );

    match output {
        OutputSpec::Hold {
            key,
            duration_ms: Some(duration),
            ..
        } => check_range(
            issues,
            number,
            &format!("hold hold_ms (key {key})"),
            *duration,
        ),
        OutputSpec::Hold {
            duration_ms: None, ..
        } => {}
        OutputSpec::Sequence { steps, .. } => {
            for (offset, step) in steps.iter().enumerate() {
                let step_number = offset + 1;
                // A zero pause is meaningful: the next step starts at once.
                check_delay(
                    issues,
                    number,
                    &format!("sequence step {step_number} pause_ms (key {})", step.key),
                    step.pause_ms,
                );
                check_range(
                    issues,
                    number,
                    &format!("sequence step {step_number} hold_ms (key {})", step.key),
                    step.hold_ms,
                );
            }
        }
    }
}

fn check_range(issues: &mut Vec<String>, number: usize, field: &str, range: JitterRange) {
    if range.min == 0 {
        issues.push(format!(
            "binding {number}: {field} minimum must be at least 1"
        ));
    }
    check_bounds(issues, number, field, range);
}

/// Like [`check_range`], but zero is a meaningful value: it means "fire at once".
fn check_delay(issues: &mut Vec<String>, number: usize, field: &str, range: JitterRange) {
    check_bounds(issues, number, field, range);
}

fn check_bounds(issues: &mut Vec<String>, number: usize, field: &str, range: JitterRange) {
    if range.max < range.min {
        issues.push(format!(
            "binding {number}: {field} maximum is smaller than its minimum"
        ));
    }
    if range.max > 5000 {
        issues.push(format!(
            "binding {number}: {field} maximum is unreasonably large (limit 5000)"
        ));
    }
}

/// Problems reading or parsing a configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The file could not be read.
    #[error("cannot read `{path}`: {source}")]
    Read {
        /// Path that was attempted.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid YAML for this schema.
    #[error("cannot parse `{path}`: {source}")]
    Parse {
        /// Path that was attempted.
        path: PathBuf,
        /// Underlying YAML failure.
        #[source]
        source: serde_yaml_ng::Error,
    },
    /// The in-memory text is not valid YAML for this schema.
    #[error("cannot parse configuration: {0}")]
    ParseText(#[from] serde_yaml_ng::Error),
}
