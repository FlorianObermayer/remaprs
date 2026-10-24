//! Command-line parsing, config-file lookup and starter-file writing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::Parser;
use clap::error::ErrorKind;
use remaprs::cli::{
    Cli, Command, ConfigCommand, ConfigSource, DEFAULT_CONFIG_FILE, STARTER_CONFIG, candidate_path,
    init_target, resolve_config_source, write_starter,
};
use remaprs::config::Config;
use remaprs::keys::Key;

/// A scratch directory that removes itself, so the tests need no dev-dependency.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    #[expect(
        clippy::unwrap_used,
        reason = "a scratch directory that cannot be created makes the test meaningless"
    )]
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "remaprs-test-{}-{label}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper; a rejected fixture means the test is wrong"
)]
fn parse(parts: &[&str]) -> Cli {
    let mut argv = vec!["remaprs"];
    argv.extend_from_slice(parts);
    Cli::try_parse_from(argv).unwrap()
}

#[expect(
    clippy::unwrap_used,
    reason = "test helper; an accepted fixture means the test is wrong"
)]
fn error_kind(parts: &[&str]) -> ErrorKind {
    let mut argv = vec!["remaprs"];
    argv.extend_from_slice(parts);
    Cli::try_parse_from(argv).unwrap_err().kind()
}

#[test]
fn the_existing_run_flags_still_parse() {
    let cli = parse(&[
        "-c",
        "custom.yaml",
        "-d",
        "2",
        "--set",
        "A=SPACE:40-90:15-35:0",
        "--set-key",
        "Q=E:50:20",
        "--dry-run",
        "--log-level",
        "debug",
    ]);

    assert!(cli.command.is_none());
    assert_eq!(cli.config, Some(PathBuf::from("custom.yaml")));
    assert_eq!(cli.device, Some(2));
    assert_eq!(cli.set, vec!["A=SPACE:40-90:15-35:0".to_owned()]);
    assert_eq!(cli.set_key, vec!["Q=E:50:20".to_owned()]);
    assert!(cli.dry_run);
    assert!(!cli.no_config);
    assert_eq!(cli.log_level.as_deref(), Some("debug"));
}

#[test]
fn no_arguments_still_parse_into_a_run() {
    // Bare `remaprs` must keep its old behavior: no subcommand, defaults, and
    // the "no bindings configured" error coming from validation.
    let cli = parse(&[]);

    assert!(cli.command.is_none());
    assert!(cli.config.is_none());
    assert!(!cli.no_config);
}

#[test]
fn config_init_has_no_default_path_of_its_own() {
    // The destination depends on where the executable lives, so it is resolved
    // later rather than by clap.
    let cli = parse(&["config", "init"]);

    match cli.command {
        Some(Command::Config {
            action:
                ConfigCommand::Init {
                    path,
                    force,
                    stdout,
                },
        }) => {
            assert!(path.is_none());
            assert_eq!(DEFAULT_CONFIG_FILE, "remaprs.yaml");
            assert!(!force);
            assert!(!stdout);
        }
        other => panic!("expected `config init`, got {other:?}"),
    }
}

#[test]
fn the_init_target_defaults_next_to_the_executable() {
    let explicit = init_target(Some(Path::new("my-config.yaml"))).unwrap();
    assert_eq!(explicit, PathBuf::from("my-config.yaml"));

    // A bare `config init` must write the file auto-loading would pick.
    let default = init_target(None).unwrap();
    assert_eq!(
        default.file_name().unwrap().to_str(),
        Some(DEFAULT_CONFIG_FILE)
    );
    assert_eq!(default, candidate_path(default.parent().unwrap()));
}

#[test]
fn config_init_accepts_a_path_with_force() {
    let cli = parse(&["config", "init", "my-config.yaml", "--force"]);

    match cli.command {
        Some(Command::Config {
            action: ConfigCommand::Init { path, force, .. },
        }) => {
            assert_eq!(path, Some(PathBuf::from("my-config.yaml")));
            assert!(force);
        }
        other => panic!("expected `config init`, got {other:?}"),
    }
}

#[test]
fn config_init_can_write_to_stdout() {
    let cli = parse(&["config", "init", "--stdout"]);

    match cli.command {
        Some(Command::Config {
            action: ConfigCommand::Init { stdout, .. },
        }) => assert!(stdout),
        other => panic!("expected `config init`, got {other:?}"),
    }
}

#[test]
fn config_check_takes_the_global_config_before_or_after() {
    let before = parse(&["-c", "x.yaml", "config", "check"]);
    let after = parse(&["config", "check", "-c", "x.yaml"]);

    assert!(matches!(
        before.command,
        Some(Command::Config {
            action: ConfigCommand::Check
        })
    ));
    assert_eq!(before.config, Some(PathBuf::from("x.yaml")));
    assert_eq!(after.config, Some(PathBuf::from("x.yaml")));
}

#[test]
fn config_check_accepts_the_overrides_that_change_the_result() {
    let cli = parse(&[
        "config",
        "check",
        "--set",
        "A=SPACE",
        "--device",
        "1",
        "--no-focus-guard",
    ]);

    assert_eq!(cli.set, vec!["A=SPACE".to_owned()]);
    assert_eq!(cli.device, Some(1));
    assert!(cli.no_focus_guard);
}

#[test]
fn no_config_is_accepted_alone_and_with_check() {
    assert!(parse(&["--no-config"]).no_config);
    assert!(parse(&["config", "check", "--no-config"]).no_config);
}

#[test]
fn no_config_conflicts_with_config() {
    assert_eq!(
        error_kind(&["--no-config", "-c", "x.yaml"]),
        ErrorKind::ArgumentConflict
    );
    assert_eq!(
        error_kind(&["config", "check", "--no-config", "-c", "x.yaml"]),
        ErrorKind::ArgumentConflict
    );
}

#[test]
fn bare_config_requires_a_subcommand() {
    // Silently starting a run would be the worst possible reading of
    // `remaprs config`; clap prints the subcommand help and exits non-zero.
    assert_eq!(
        error_kind(&["config"]),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
}

#[test]
fn config_rejects_unknown_subcommands_and_flags() {
    assert_eq!(
        error_kind(&["config", "explain"]),
        ErrorKind::InvalidSubcommand
    );
    assert_eq!(
        error_kind(&["--definitely-not-a-flag"]),
        ErrorKind::UnknownArgument
    );
}

#[test]
fn run_only_flags_are_not_available_to_config_check() {
    // `--dry-run` belongs to the runner; `config check` never emits anything, so
    // accepting it would suggest otherwise.
    assert_eq!(
        error_kind(&["config", "check", "--dry-run"]),
        ErrorKind::UnknownArgument
    );
    assert_eq!(
        error_kind(&["config", "check", "--list-buttons"]),
        ErrorKind::UnknownArgument
    );
}

#[test]
fn every_version_flag_prints_the_crate_version() {
    // `-V` is clap's convention and `-v` is what people try first; both must
    // print the version rather than fall through to the run path.
    for flag in ["-V", "-v", "--version"] {
        let error = Cli::try_parse_from(["remaprs", flag]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::DisplayVersion, "flag {flag}");
        assert_eq!(
            error.to_string().trim(),
            concat!("remaprs ", env!("CARGO_PKG_VERSION")),
            "flag {flag} printed an unexpected version line"
        );
    }
}

#[test]
fn help_states_the_version() {
    // The version belongs in the help header: a bug report that pastes `-h`
    // should say which build produced it.
    let error = Cli::try_parse_from(["remaprs", "--help"]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let help = error.to_string();
    assert!(
        help.starts_with(concat!("remaprs ", env!("CARGO_PKG_VERSION"))),
        "help does not open with the version:\n{help}"
    );
}

#[test]
fn an_explicit_config_wins_over_the_auto_loaded_file() {
    let exe_dir = PathBuf::from("apps").join("remaprs");
    let auto = candidate_path(&exe_dir);

    let source = resolve_config_source(Some(Path::new("chosen.yaml")), false, &exe_dir, |path| {
        path == auto.as_path()
    });

    assert_eq!(source, ConfigSource::Explicit(PathBuf::from("chosen.yaml")));
}

#[test]
fn a_file_next_to_the_executable_is_auto_loaded() {
    let exe_dir = PathBuf::from("apps").join("remaprs");
    let auto = candidate_path(&exe_dir);

    let source = resolve_config_source(None, false, &exe_dir, |path| path == auto.as_path());

    assert_eq!(source, ConfigSource::AutoLoaded(auto.clone()));
    assert_eq!(auto.file_name().unwrap().to_str(), Some("remaprs.yaml"));
}

#[test]
fn a_missing_file_next_to_the_executable_means_defaults() {
    let exe_dir = PathBuf::from("apps").join("remaprs");

    let source = resolve_config_source(None, false, &exe_dir, |_| false);

    assert_eq!(source, ConfigSource::None);
    assert_eq!(source.label(), "<built-in defaults>");
    assert!(source.path().is_none());
}

#[test]
fn no_config_overrides_both_the_file_and_the_flag() {
    // clap rejects the combination, but the resolver must still be unambiguous
    // about which one wins if it is ever called directly.
    let exe_dir = PathBuf::from("apps").join("remaprs");
    let auto = candidate_path(&exe_dir);

    assert_eq!(
        resolve_config_source(None, true, &exe_dir, |path| path == auto.as_path()),
        ConfigSource::None
    );
    assert_eq!(
        resolve_config_source(Some(Path::new("chosen.yaml")), true, &exe_dir, |path| path
            == auto.as_path()),
        ConfigSource::None
    );
}

#[test]
fn the_starter_file_is_valid_and_writes_verbatim() {
    let dir = TempDir::new("starter");
    let path = dir.join("remaprs.yaml");

    write_starter(&path, false).unwrap();

    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, STARTER_CONFIG);
}

#[test]
fn the_starter_file_parses_and_validates_cleanly() {
    // The embedded example is what `config init` hands out, so a broken example
    // would ship a broken generator.
    let config = Config::from_yaml(STARTER_CONFIG).unwrap();

    assert_eq!(config.validate(), Vec::<String>::new());
}

#[test]
fn an_existing_starter_file_is_not_overwritten_without_force() {
    let dir = TempDir::new("exists");
    let path = dir.join("remaprs.yaml");
    std::fs::write(&path, "version: 1\nbindings: []\n").unwrap();

    let error = write_starter(&path, false).unwrap_err();

    assert!(error.to_string().contains("already exists"));
    assert!(error.to_string().contains("--force"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "version: 1\nbindings: []\n"
    );
}

#[test]
fn force_replaces_an_existing_starter_file() {
    let dir = TempDir::new("force");
    let path = dir.join("remaprs.yaml");
    std::fs::write(&path, "version: 1\nbindings: []\n").unwrap();

    write_starter(&path, true).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), STARTER_CONFIG);
}

#[test]
fn a_missing_directory_is_reported_as_a_write_error() {
    let dir = TempDir::new("missing");
    let path = dir.join("no-such-directory").join("remaprs.yaml");

    let error = write_starter(&path, false).unwrap_err();

    assert!(error.to_string().contains("cannot write"));
}

#[test]
fn overrides_apply_on_top_of_the_loaded_configuration() {
    let mut config = Config::from_yaml(
        r"
bindings:
  - from: A
    key: SPACE
",
    )
    .unwrap();

    let cli = parse(&[
        "--device",
        "3",
        "--no-focus-guard",
        "--toggle-hotkey",
        "F9",
        "--set",
        "A=ENTER",
        "--set-key",
        "Q=E",
    ]);
    cli.apply_overrides(&mut config).unwrap();

    assert_eq!(config.device.index, 3);
    assert!(!config.safety.require_focused);
    assert_eq!(config.hotkey.toggle, Some(Key::parse("F9").unwrap()));
    assert_eq!(config.bindings.len(), 2, "A is replaced, Q is added");
    assert_eq!(config.validate(), Vec::<String>::new());
}

#[test]
fn the_hotkey_override_accepts_none_and_rejects_the_mouse() {
    let mut config = Config::default();

    parse(&["--toggle-hotkey", "none"])
        .apply_overrides(&mut config)
        .unwrap();
    assert_eq!(config.hotkey.toggle, None);

    let error = parse(&["--toggle-hotkey", "MOUSE_LEFT"])
        .apply_overrides(&mut config)
        .unwrap_err();
    assert!(error.to_string().contains("keyboard key"));
}

#[test]
fn a_malformed_set_spec_is_an_error() {
    let mut config = Config::default();

    let error = parse(&["--set", "A-SPACE"])
        .apply_overrides(&mut config)
        .unwrap_err();

    assert!(error.to_string().contains("expected"));
}
