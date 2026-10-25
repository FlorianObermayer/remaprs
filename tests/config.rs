//! Configuration parsing, defaults and validation.

use remaprs::config::{
    BindingConfig, Config, JitterRange, KeyMode, OutputMode, OutputSpec, StickAxisSpec,
    StickTargetSpec,
};
use remaprs::keys::{Button, Key, KeyboardKey, Stick, StickDirection, Trigger};

/// The `(key, hold, pause, initial_delay)` of a one-step sequence, which is how
/// a plain spam is written.
fn step_parts(output: &OutputSpec) -> (Key, JitterRange, JitterRange, JitterRange) {
    match output {
        OutputSpec::Sequence {
            steps,
            initial_delay_ms,
            ..
        } if steps.len() == 1 => (
            steps[0].key,
            steps[0].hold_ms,
            steps[0].pause_ms,
            *initial_delay_ms,
        ),
        other => panic!("expected a one-step sequence, got {other:?}"),
    }
}

/// The only key an output emits, for the single-key cases.
fn output_key(output: &OutputSpec) -> Key {
    match output {
        OutputSpec::Hold { key, .. } => *key,
        OutputSpec::Sequence { steps, .. } => steps[0].key,
        other @ OutputSpec::Stick { .. } => panic!("a stick output emits no key, got {other:?}"),
    }
}

const SAMPLE: &str = r"
version: 1

device:
  index: 1
  poll_interval_ms: 8
  trigger_threshold: 40

output:
  key_mode: virtual_key

hotkey:
  toggle: F11

safety:
  target_process: Game.exe
  require_focused: true
  release_on_exit: true

bindings:
  - from: A
    key: SPACE
    pause_ms: [40, 90]
    hold_ms: 20
  - from: LB
    key: MOUSE_LEFT
";

#[test]
fn parses_a_full_file() {
    let config = Config::from_yaml(SAMPLE).expect("sample must parse");

    assert_eq!(config.version, 1);
    assert_eq!(config.device.index, 1);
    assert_eq!(config.device.poll_interval_ms, 8);
    assert_eq!(config.device.trigger_threshold, 40);
    assert_eq!(config.output.key_mode, KeyMode::VirtualKey);
    assert_eq!(config.safety.target_process.as_deref(), Some("Game.exe"));
    assert_eq!(config.bindings.len(), 2);
    assert_eq!(config.bindings[0].outputs.len(), 1);
    let output = &config.bindings[0].outputs[0];
    assert_eq!(output.mode(), OutputMode::Sequence);
    let (key, hold, pause, delay) = step_parts(output);
    assert_eq!(key, Key::parse("SPACE").expect("valid key"));
    assert_eq!(hold, JitterRange::fixed(20));
    assert_eq!(pause, JitterRange::new(40, 90));
    assert_eq!(
        delay,
        JitterRange::fixed(0),
        "the first press is immediate unless configured otherwise"
    );
    assert_eq!(
        config.bindings[0].from,
        Trigger::Button(remaprs::keys::Button::A)
    );
    assert_eq!(
        output_key(&config.bindings[1].outputs[0]),
        Key::parse("MOUSE_LEFT").expect("valid key")
    );
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn applies_defaults() {
    let config = Config::from_yaml("bindings:\n  - from: Y\n    key: F1\n")
        .expect("minimal config must parse");

    assert_eq!(config.version, 1);
    assert_eq!(config.device.index, 0);
    assert_eq!(config.device.poll_interval_ms, 4);
    assert_eq!(config.device.trigger_threshold, 30);
    assert_eq!(config.output.key_mode, KeyMode::Scancode);
    assert_eq!(
        config.hotkey.toggle,
        Some(Key::parse("F12").expect("valid key"))
    );
    assert_eq!(config.bindings[0].outputs.len(), 1);
    let (_, hold, pause, delay) = step_parts(&config.bindings[0].outputs[0]);
    assert_eq!(hold, JitterRange::new(15, 35));
    assert_eq!(pause, JitterRange::new(40, 90));
    assert_eq!(delay, JitterRange::fixed(0));
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn rejects_unknown_fields_so_typos_are_visible() {
    assert!(Config::from_yaml("bindingz: []").is_err());
    assert!(
        Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n    intervall_ms: 5\n").is_err()
    );
}

#[test]
fn rejects_an_unknown_output_mode() {
    let error = Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n    outputs:\n      - key: E\n        mode: combo\n")
        .expect_err("`combo` is no longer a mode");

    assert!(error.to_string().contains("combo"), "{error}");
}

#[test]
fn a_binding_needs_at_least_one_key() {
    let error = Config::from_yaml("bindings:\n  - from: A\n")
        .expect_err("a binding without keys is useless");

    assert!(error.to_string().contains("outputs"), "{error}");
}

#[test]
fn parses_a_multi_output_binding() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    outputs:
      - key: Y
        pause_ms: 50
        hold_ms: 15
      - key: Z
        mode: hold
",
    )
    .expect("valid config");

    let outputs = &config.bindings[0].outputs;
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].mode(), OutputMode::Sequence);
    assert_eq!(output_key(&outputs[0]), Key::parse("Y").expect("valid key"));
    assert_eq!(outputs[1].mode(), OutputMode::Hold);
    assert_eq!(output_key(&outputs[1]), Key::parse("Z").expect("valid key"));
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn outputs_inherit_the_binding_level_defaults() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    mode: hold
    pause_ms: 70
    hold_ms: 25
    outputs:
      - key: Y
      - key: Z
",
    )
    .expect("valid config");

    let outputs = &config.bindings[0].outputs;
    assert_eq!(outputs.len(), 2);
    for output in outputs {
        match output {
            OutputSpec::Hold {
                duration_ms,
                initial_delay_ms,
                ..
            } => {
                assert_eq!(
                    *duration_ms,
                    Some(JitterRange::fixed(25)),
                    "binding default"
                );
                assert_eq!(*initial_delay_ms, JitterRange::fixed(0));
            }
            other @ (OutputSpec::Sequence { .. } | OutputSpec::Stick { .. }) => {
                panic!("expected a hold output, got {other:?}")
            }
        }
    }
}

#[test]
fn outputs_can_override_the_binding_level_defaults() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    key: Y
    pause_ms: 70
    outputs:
      - key: Z
        mode: hold
      - key: W
        pause_ms: 20
",
    )
    .expect("valid config");

    let outputs = &config.bindings[0].outputs;
    assert_eq!(outputs.len(), 3, "`key` and `outputs` combine");
    assert_eq!(output_key(&outputs[0]), Key::parse("Y").expect("valid key"));
    assert_eq!(outputs[1].mode(), OutputMode::Hold);
    assert_eq!(
        step_parts(&outputs[2]).2,
        JitterRange::fixed(20),
        "the output overrode the binding-level pause"
    );
}

#[test]
fn rejects_the_same_key_twice_in_one_binding() {
    let config =
        Config::from_yaml("bindings:\n  - from: X\n    outputs:\n      - key: Y\n      - key: Y\n")
            .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("more than one output")),
        "{issues:?}"
    );
}

#[test]
fn a_hold_output_needs_only_a_key() {
    let config = Config::from_yaml("bindings:\n  - from: X\n    key: Y\n    mode: hold\n")
        .expect("valid config");

    assert!(
        config.validate().is_empty(),
        "a hold has no cadence to validate: {:?}",
        config.validate()
    );
}

#[test]
fn a_one_step_sequence_needs_a_valid_hold() {
    let config = Config::from_yaml(
        "bindings:\n  - from: X\n    outputs:\n      - key: Y\n      - key: Z\n        hold_ms: 0\n",
    )
    .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues.iter().any(|issue| issue.contains("at least 1")),
        "{issues:?}"
    );
}

#[test]
fn parses_a_sequence_output() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    outputs:
      - mode: sequence
        initial_delay_ms: [0, 50]
        steps:
          - key: A
            hold_ms: [30, 80]
            pause_ms: [100, 250]
          - key: B
            hold_ms: 40
            pause_ms: 150
",
    )
    .expect("valid config");

    match &config.bindings[0].outputs[0] {
        OutputSpec::Sequence {
            steps,
            repeat,
            initial_delay_ms,
        } => {
            assert_eq!(steps.len(), 2);
            assert_eq!(steps[0].key, Key::parse("A").expect("valid key"));
            assert_eq!(steps[0].hold_ms, JitterRange::new(30, 80));
            assert_eq!(steps[0].pause_ms, JitterRange::new(100, 250));
            assert_eq!(steps[1].hold_ms, JitterRange::fixed(40));
            assert_eq!(steps[1].pause_ms, JitterRange::fixed(150));
            assert!(*repeat, "sequences loop by default");
            assert_eq!(*initial_delay_ms, JitterRange::new(0, 50));
        }
        other @ (OutputSpec::Hold { .. } | OutputSpec::Stick { .. }) => {
            panic!("expected a sequence, got {other:?}")
        }
    }
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn a_sequence_can_run_once() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    outputs:
      - mode: sequence
        repeat: false
        steps:
          - key: A
",
    )
    .expect("valid config");

    match &config.bindings[0].outputs[0] {
        OutputSpec::Sequence { repeat, .. } => assert!(!*repeat),
        other @ (OutputSpec::Hold { .. } | OutputSpec::Stick { .. }) => {
            panic!("expected a sequence, got {other:?}")
        }
    }
}

#[test]
fn sequence_steps_inherit_the_binding_defaults() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    hold_ms: 25
    pause_ms: 80
    outputs:
      - mode: sequence
        steps:
          - key: A
          - key: B
            hold_ms: 10
",
    )
    .expect("valid config");

    match &config.bindings[0].outputs[0] {
        OutputSpec::Sequence { steps, .. } => {
            assert_eq!(steps[0].hold_ms, JitterRange::fixed(25), "binding hold_ms");
            assert_eq!(
                steps[0].pause_ms,
                JitterRange::fixed(80),
                "binding pause_ms"
            );
            assert_eq!(steps[1].hold_ms, JitterRange::fixed(10), "step override");
        }
        other @ (OutputSpec::Hold { .. } | OutputSpec::Stick { .. }) => {
            panic!("expected a sequence, got {other:?}")
        }
    }
}

#[test]
fn a_sequence_needs_at_least_one_step() {
    let error = Config::from_yaml(
        "bindings:\n  - from: X\n    outputs:\n      - mode: sequence\n        steps: []\n",
    )
    .expect_err("an empty sequence is useless");

    assert!(error.to_string().contains("steps"), "{error}");
}

#[test]
fn a_sequence_may_repeat_a_key() {
    // Holding X -> A, B, A is a legitimate combo; only the same key in two
    // different outputs would collide.
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    outputs:
      - mode: sequence
        steps:
          - key: A
          - key: B
          - key: A
",
    )
    .expect("valid config");

    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn a_sequence_step_needs_a_valid_hold() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    outputs:
      - mode: sequence
        steps:
          - key: A
            hold_ms: 0
",
    )
    .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues.iter().any(|issue| issue.contains("sequence step 1")),
        "{issues:?}"
    );
}

#[test]
fn a_sequence_pause_may_be_zero() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: X
    outputs:
      - mode: sequence
        steps:
          - key: A
            pause_ms: 0
",
    )
    .expect("valid config");

    assert!(
        config.validate().is_empty(),
        "a zero pause just means the next step follows at once: {:?}",
        config.validate()
    );
}

#[test]
fn sequence_steps_count_as_emitted_keys() {
    // A sequence that presses a keyboard trigger would feed itself.
    let config = Config::from_yaml(
        "\
bindings:
  - source: keyboard
    from: Q
    key: E
  - from: X
    outputs:
      - mode: sequence
        steps:
          - key: Q
",
    )
    .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues.iter().any(|issue| issue.contains("re-trigger")),
        "{issues:?}"
    );
}

#[test]
fn rejects_duplicate_buttons_and_keys() {
    let config = Config::from_yaml(
        "bindings:\n  - from: A\n    key: SPACE\n  - from: A\n    key: E\n  - from: B\n    key: SPACE\n",
    )
    .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues.iter().any(|issue| issue.contains("already mapped")),
        "{issues:?}"
    );
    assert!(
        issues.iter().any(|issue| issue.contains("already emitted")),
        "{issues:?}"
    );
}

#[test]
fn rejects_a_hotkey_that_collides_with_output() {
    let config =
        Config::from_yaml("hotkey:\n  toggle: SPACE\nbindings:\n  - from: A\n    key: SPACE\n")
            .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues.iter().any(|issue| issue.contains("collides")),
        "{issues:?}"
    );
}

#[test]
fn hotkey_can_be_disabled() {
    let config =
        Config::from_yaml("hotkey:\n  toggle: none\nbindings:\n  - from: A\n    key: SPACE\n")
            .expect("valid");

    assert!(config.hotkey.toggle.is_none());
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn rejects_inverted_and_zero_ranges() {
    let inverted =
        Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n    pause_ms: [90, 40]\n")
            .expect("parses, then fails validation");
    assert!(
        inverted
            .validate()
            .iter()
            .any(|issue| issue.contains("smaller than its minimum")),
        "{:?}",
        inverted.validate()
    );

    let zero = Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n    hold_ms: [0, 5]\n")
        .expect("parses, then fails validation");
    assert!(
        zero.validate()
            .iter()
            .any(|issue| issue.contains("at least 1")),
        "{:?}",
        zero.validate()
    );
}

#[test]
fn rejects_a_device_slot_above_three() {
    let config = Config::from_yaml("device:\n  index: 4\nbindings:\n  - from: A\n    key: SPACE\n")
        .expect("parses, then fails validation");

    assert!(
        config
            .validate()
            .iter()
            .any(|issue| issue.contains("out of range")),
        "{:?}",
        config.validate()
    );
}

#[test]
fn rejects_an_unsupported_version() {
    // A file left over from a future or older build must fail loudly instead of
    // quietly running with defaults it does not understand.
    let config = Config::from_yaml("version: 2\nbindings:\n  - from: A\n    key: SPACE\n")
        .expect("parses, then fails validation");

    assert!(
        config
            .validate()
            .iter()
            .any(|issue| issue.contains("version 2 is not supported")),
        "{:?}",
        config.validate()
    );
}

#[test]
fn rejects_a_mouse_toggle_hotkey() {
    let config = Config::from_yaml(
        "hotkey:\n  toggle: MOUSE_LEFT\nbindings:\n  - from: A\n    key: SPACE\n",
    )
    .expect("parses, then fails validation");

    assert!(
        config
            .validate()
            .iter()
            .any(|issue| issue.contains("keyboard key")),
        "{:?}",
        config.validate()
    );
}

#[test]
fn parses_set_specs() {
    let binding = BindingConfig::parse_spec("A=SPACE:40-90:15-35").expect("valid spec");
    assert_eq!(binding.outputs.len(), 1);
    let (key, hold, pause, _) = step_parts(&binding.outputs[0]);
    assert_eq!(key, Key::parse("SPACE").expect("valid key"));
    assert_eq!(hold, JitterRange::new(15, 35));
    assert_eq!(pause, JitterRange::new(40, 90));

    let defaults = BindingConfig::parse_spec("lb=mouse_left").expect("valid spec");
    let (_, hold, pause, _) = step_parts(&defaults.outputs[0]);
    assert_eq!(hold, JitterRange::new(15, 35));
    assert_eq!(pause, JitterRange::new(40, 90));

    let fixed = BindingConfig::parse_spec("RT=E:50").expect("valid spec");
    let (_, hold, pause, _) = step_parts(&fixed.outputs[0]);
    assert_eq!(hold, JitterRange::new(15, 35));
    assert_eq!(pause, JitterRange::fixed(50));

    assert!(BindingConfig::parse_spec("A").is_err(), "missing key");
    assert!(
        BindingConfig::parse_spec("A=SPACE:1-2:3:4:5").is_err(),
        "too many separators"
    );
    assert!(BindingConfig::parse_spec("A=NOPE").is_err(), "unknown key");
    assert!(
        BindingConfig::parse_spec("NOPE=SPACE").is_err(),
        "unknown button"
    );
    assert!(
        BindingConfig::parse_spec("A=SPACE:fast").is_err(),
        "non-numeric range"
    );
}

#[test]
fn parses_the_initial_delay_from_a_set_spec() {
    let binding = BindingConfig::parse_spec("A=SPACE:40:20:0").expect("valid spec");
    let (_, hold, pause, delay) = step_parts(&binding.outputs[0]);
    assert_eq!(hold, JitterRange::fixed(20));
    assert_eq!(pause, JitterRange::fixed(40));
    assert_eq!(delay, JitterRange::fixed(0));

    let ranged = BindingConfig::parse_spec("A=SPACE:40:20:5-60").expect("valid spec");
    assert_eq!(step_parts(&ranged.outputs[0]).3, JitterRange::new(5, 60));

    // Without the fourth field the first press is immediate.
    let default = BindingConfig::parse_spec("A=SPACE:40:20").expect("valid spec");
    assert_eq!(step_parts(&default.outputs[0]).3, JitterRange::fixed(0));
}

#[test]
fn parses_keyboard_bindings_from_yaml() {
    let config = Config::from_yaml("bindings:\n  - source: keyboard\n    from: Q\n    key: E\n")
        .expect("valid config");

    assert_eq!(
        config.bindings[0].from,
        Trigger::Keyboard(KeyboardKey::new(0x51, false))
    );
    assert!(config.uses_keyboard());
    assert_eq!(
        config.keyboard_triggers(),
        vec![KeyboardKey::new(0x51, false)]
    );
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn parses_keyboard_bindings_from_a_key_spec() {
    let binding = BindingConfig::parse_key_spec("Q=E:40:20").expect("valid spec");
    assert_eq!(
        binding.from,
        Trigger::Keyboard(KeyboardKey::new(0x51, false))
    );
    assert_eq!(
        output_key(&binding.outputs[0]),
        Key::parse("E").expect("valid key")
    );

    // The controller and keyboard namespaces stay separate, so `MOUSE_LEFT` is
    // never a keyboard trigger.
    assert!(BindingConfig::parse_key_spec("MOUSE_LEFT=E").is_err());
}

#[test]
fn rejects_an_emitted_key_that_is_also_a_keyboard_trigger() {
    // `GetAsyncKeyState` sees our own `SendInput` press, so this would loop.
    let config = Config::from_yaml(
        "bindings:\n  - source: keyboard\n    from: Q\n    key: E\n  - from: A\n    key: Q\n",
    )
    .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues.iter().any(|issue| issue.contains("re-trigger")),
        "{issues:?}"
    );
}

#[test]
fn rejects_a_hotkey_that_is_also_a_keyboard_trigger() {
    let config = Config::from_yaml(
        "hotkey:\n  toggle: Q\nbindings:\n  - source: keyboard\n    from: Q\n    key: E\n",
    )
    .expect("parses, then fails validation");

    let issues = config.validate();
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("would toggle instead of firing")),
        "{issues:?}"
    );
}

#[test]
fn accepts_a_delay_of_zero() {
    let config =
        Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n    initial_delay_ms: 0\n")
            .expect("valid config");

    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn rejects_an_inverted_initial_delay() {
    let config = Config::from_yaml(
        "bindings:\n  - from: A\n    key: SPACE\n    initial_delay_ms: [60, 10]\n",
    )
    .expect("parses, then fails validation");

    assert!(
        config
            .validate()
            .iter()
            .any(|issue| issue.contains("smaller than its minimum")),
        "{:?}",
        config.validate()
    );
}

#[test]
fn upsert_replaces_an_existing_binding() {
    let mut config =
        Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n").expect("valid config");

    config.upsert_binding("A=E:10").expect("valid spec");

    assert_eq!(
        config.bindings.len(),
        1,
        "the existing mapping was replaced"
    );
    assert_eq!(
        output_key(&config.bindings[0].outputs[0]),
        Key::parse("E").expect("valid key")
    );
}

#[test]
fn upsert_appends_a_new_binding() {
    let mut config =
        Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n").expect("valid config");

    config.upsert_binding("B=E").expect("valid spec");

    assert_eq!(config.bindings.len(), 2);
}

#[test]
fn upsert_key_binding_keeps_controller_and_keyboard_apart() {
    let mut config =
        Config::from_yaml("bindings:\n  - from: A\n    key: SPACE\n").expect("valid config");

    // `A` is both a controller button and a keyboard key, so the two overrides
    // must not overwrite each other.
    config.upsert_key_binding("A=E").expect("valid spec");

    assert_eq!(config.bindings.len(), 2);
    assert_eq!(
        config.bindings[0].from,
        Trigger::Button(remaprs::keys::Button::A)
    );
    assert_eq!(
        config.bindings[1].from,
        Trigger::Keyboard(KeyboardKey::new(0x41, false))
    );

    config.upsert_key_binding("A=Q").expect("valid spec");
    assert_eq!(
        config.bindings.len(),
        2,
        "the keyboard mapping was replaced"
    );
    assert_eq!(
        output_key(&config.bindings[1].outputs[0]),
        Key::parse("Q").expect("valid key")
    );
}

#[test]
fn a_step_may_hold_longer_than_it_pauses() {
    // The pause starts at the release, so a long hold is not a mistake.
    let config = Config::from_yaml(
        "bindings:\n  - from: A\n    key: SPACE\n    pause_ms: 10\n    hold_ms: 200\n",
    )
    .expect("valid config");

    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn a_zero_pause_is_allowed() {
    // Re-pressing the instant the key comes up is a legitimate machine gun.
    let config = Config::from_yaml(
        "bindings:\n  - from: A\n    key: SPACE\n    pause_ms: 0\n    hold_ms: 20\n",
    )
    .expect("valid config");

    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn an_empty_configuration_is_reported() {
    let issues = Config::default().validate();

    assert!(
        issues.iter().any(|issue| issue.contains("no bindings")),
        "{issues:?}"
    );
}

#[test]
fn list_buttons_only_needs_a_valid_device() {
    // `--list-buttons` emits nothing, so it must work without bindings.
    let config = Config::default();

    assert_eq!(config.validate_device(), [] as [std::string::String; 0]);
    assert_ne!(config.validate(), [] as [std::string::String; 0]);
}

#[test]
fn parses_a_stick_output() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: RS_AXIS
    outputs:
      - mode: stick
        target: scroll
        axis: vertical
        sensitivity: 1200
        invert_y: true
        initial_delay_ms: 20
",
    )
    .expect("valid config");

    match &config.bindings[0].outputs[0] {
        OutputSpec::Stick {
            target,
            axes,
            sensitivity,
            invert_x,
            invert_y,
            initial_delay_ms,
        } => {
            assert_eq!(*target, StickTargetSpec::Scroll);
            assert_eq!(*axes, StickAxisSpec::Vertical);
            assert_eq!(*sensitivity, 1_200);
            assert!(!*invert_x);
            assert!(*invert_y);
            assert_eq!(*initial_delay_ms, JitterRange::fixed(20));
        }
        other => panic!("expected a stick output, got {other:?}"),
    }
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn stick_outputs_have_sensible_defaults() {
    let config =
        Config::from_yaml("bindings:\n  - from: LS_AXIS\n    mode: stick\n").expect("valid config");

    match &config.bindings[0].outputs[0] {
        OutputSpec::Stick {
            target,
            axes,
            sensitivity,
            invert_x,
            invert_y,
            initial_delay_ms,
        } => {
            assert_eq!(*target, StickTargetSpec::Mouse);
            assert_eq!(*axes, StickAxisSpec::Both);
            assert_eq!(*sensitivity, remaprs::config::DEFAULT_STICK_SENSITIVITY);
            assert!(!*invert_x);
            assert!(!*invert_y);
            assert_eq!(*initial_delay_ms, JitterRange::fixed(0));
        }
        other => panic!("expected a stick output, got {other:?}"),
    }
}

#[test]
fn a_stick_output_needs_an_axis_trigger() {
    let error = Config::from_yaml(
        "\
bindings:
  - from: A
    mode: stick
",
    )
    .expect_err("a button cannot drive a stick output");

    assert!(error.to_string().contains("LS_AXIS"), "{error}");
}

#[test]
fn an_axis_trigger_needs_a_stick_output() {
    let error = Config::from_yaml("bindings:\n  - from: LS_AXIS\n    key: W\n")
        .expect_err("an axis cannot press a key");

    assert!(error.to_string().contains("mode: stick"), "{error}");
}

#[test]
fn a_direction_trigger_can_drive_a_key() {
    let config = Config::from_yaml("bindings:\n  - from: LS_UP\n    key: W\n")
        .expect("a direction is a digital trigger");

    assert_eq!(
        config.bindings[0].from,
        Trigger::StickDirection {
            stick: Stick::Left,
            direction: StickDirection::Up,
        }
    );
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn stick_only_fields_are_rejected_on_a_key_output() {
    let error = Config::from_yaml("bindings:\n  - from: A\n    key: W\n    sensitivity: 500\n")
        .expect_err("sensitivity only applies to a stick output");

    assert!(error.to_string().contains("only apply"), "{error}");
}

#[test]
fn key_only_fields_are_rejected_on_a_stick_output() {
    let error = Config::from_yaml("bindings:\n  - from: LS_AXIS\n    mode: stick\n    key: W\n")
        .expect_err("a stick output cannot also press a key");

    assert!(error.to_string().contains("do not apply"), "{error}");
}

#[test]
fn stick_sensitivity_is_bounded() {
    let zero =
        Config::from_yaml("bindings:\n  - from: LS_AXIS\n    mode: stick\n    sensitivity: 0\n")
            .expect("the value parses");
    assert!(
        zero.validate()
            .iter()
            .any(|issue| issue.contains("sensitivity must be at least 1")),
        "{:?}",
        zero.validate()
    );

    let high = Config::from_yaml(
        "bindings:\n  - from: LS_AXIS\n    mode: stick\n    sensitivity: 999999\n",
    )
    .expect("the value parses");
    assert!(
        high.validate()
            .iter()
            .any(|issue| issue.contains("unreasonably high")),
        "{:?}",
        high.validate()
    );
}

#[test]
fn two_stick_outputs_may_not_drive_the_same_axis() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: LS_AXIS
    outputs:
      - mode: stick
      - mode: stick
        axis: horizontal
",
    )
    .expect("the file parses");

    let issues = config.validate();
    assert!(
        issues
            .iter()
            .any(|issue| issue.contains("more than one `stick` output")),
        "{issues:?}"
    );
}

#[test]
fn separate_axes_on_the_same_stick_are_allowed() {
    let config = Config::from_yaml(
        "\
bindings:
  - from: LS_AXIS
    outputs:
      - mode: stick
        axis: horizontal
      - mode: stick
        axis: vertical
        target: scroll
",
    )
    .expect("the file parses");

    assert!(config.validate().is_empty(), "{:?}", config.validate());
}

#[test]
fn the_stick_deadzone_is_validated() {
    let too_high =
        Config::from_yaml("device:\n  stick_deadzone: 65000\nbindings:\n  - from: A\n    key: W\n")
            .expect("the value parses");
    assert!(
        too_high
            .validate_device()
            .iter()
            .any(|issue| issue.contains("stick_deadzone")),
        "{:?}",
        too_high.validate_device()
    );

    let zero =
        Config::from_yaml("device:\n  stick_deadzone: 0\nbindings:\n  - from: A\n    key: W\n")
            .expect("the value parses");
    assert!(
        zero.validate_device()
            .iter()
            .any(|issue| issue.contains("stick_deadzone")),
        "{:?}",
        zero.validate_device()
    );

    let default = Config::from_yaml("bindings:\n  - from: A\n    key: W\n").expect("valid");
    assert_eq!(default.device.stick_deadzone, 7_849);
    assert_eq!(default.validate_device(), [] as [std::string::String; 0]);
}

#[test]
fn dpad_diagonals_parse_as_controller_triggers() {
    let config = Config::from_yaml("bindings:\n  - from: DPAD_UP_LEFT\n    key: Q\n")
        .expect("a d-pad diagonal is a controller button");

    assert_eq!(config.bindings[0].from, Trigger::Button(Button::UpLeft));
    assert!(config.validate().is_empty(), "{:?}", config.validate());
}
