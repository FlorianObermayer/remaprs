//! The release tooling: the changelog template and the version bump script.
//!
//! These run the real scripts against a throwaway git repository rather than
//! asserting on their source text, because the failure that matters is a
//! template that renders the wrong compare link or a bump that writes the wrong
//! version. `git-cliff` is required; `scripts/check.ps1` installs it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// A scratch repository that removes itself, so the tests need no dev-dependency.
struct ScratchRepo {
    path: PathBuf,
}

impl ScratchRepo {
    #[expect(
        clippy::expect_used,
        reason = "a scratch repository that cannot be created makes the test meaningless"
    )]
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "remaprs-tooling-{}-{label}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create scratch repository");
        let repo = Self { path };
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "user.name", "remaprs tests"]);
        // Signing would fail on a machine without a configured key.
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.write(
            "Cargo.toml",
            "[package]\nname = \"remaprs\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        // `cargo update` refuses a manifest without a target, and the bump
        // script runs it to keep Cargo.lock in step with the version.
        std::fs::create_dir_all(repo.path().join("src")).expect("create src directory");
        repo.write("src/lib.rs", "// fixture\n");
        repo
    }

    fn path(&self) -> &Path {
        &self.path
    }

    #[expect(
        clippy::expect_used,
        reason = "a fixture that cannot be written makes the test meaningless"
    )]
    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.path.join(name), contents).expect("write fixture file");
    }

    #[expect(
        clippy::expect_used,
        reason = "an unreadable fixture means the test produced nothing to assert on"
    )]
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path.join(name)).expect("read fixture file")
    }

    #[expect(
        clippy::expect_used,
        reason = "git is a hard requirement of these tests; a missing executable is not a fixture error"
    )]
    fn git(&self, args: &[&str]) -> Output {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    /// Commits `file` with `message`, so the changelog has something to group.
    fn commit(&self, file: &str, message: &str) {
        self.write(file, message);
        self.git(&["add", file]);
        self.git(&["commit", "--quiet", "--message", message]);
    }

    fn tag(&self, name: &str) {
        self.git(&["tag", name]);
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The repository the scripts live in, so they are exercised as committed.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[expect(
    clippy::expect_used,
    reason = "a script that will not run makes the test meaningless"
)]
fn copy_into(repo: &ScratchRepo, relative: &str) {
    let source = workspace_root().join(relative);
    let destination = repo.path().join(relative);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).expect("create destination directory");
    }
    std::fs::copy(&source, &destination)
        .unwrap_or_else(|error| panic!("copy {} into the scratch repo: {error}", source.display()));
}

/// `git-cliff` prints its configuration notice on stderr, which is dropped.
#[expect(
    clippy::expect_used,
    reason = "a script that will not run makes the test meaningless"
)]
fn run_release_notes(repo: &ScratchRepo, args: &[&str]) -> String {
    let output = Command::new("pwsh")
        .arg("-NoProfile")
        .arg("-File")
        .arg(repo.path().join("scripts/release-notes.ps1"))
        .args(args)
        .current_dir(repo.path())
        // An empty token makes the script pass --offline, so a test never talks
        // to the GitHub API and never depends on CI having a token.
        .env("GITHUB_TOKEN", "")
        .env("RELEASE_VERSION", "")
        .output()
        .expect("run release-notes.ps1");
    assert!(
        output.status.success(),
        "release-notes.ps1 {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    repo.read("body.md")
}

fn prepare_notes_repo(label: &str) -> ScratchRepo {
    let repo = ScratchRepo::new(label);
    copy_into(&repo, "cliff.toml");
    copy_into(&repo, "scripts/release-notes.ps1");
    repo.commit("Cargo.toml", "feat(engine): initial remapper");
    repo.tag("v0.1.0");
    repo.commit("src.txt", "fix: release the key on exit");
    repo
}

#[test]
fn stable_notes_name_the_version_and_its_changes() {
    let repo = prepare_notes_repo("stable");
    let body = run_release_notes(
        &repo,
        &[
            "-Version",
            "0.2.0",
            "-Build",
            "0.2.0-7",
            "-Commit",
            "0123456789abcdef",
            "-Kind",
            "stable",
            "-OutputPath",
            "body.md",
        ],
    );

    assert!(body.contains("remaprs-0.2.0-x86_64-pc-windows-gnu.zip"));
    assert!(body.contains("Build: `0.2.0-7`"));
    assert!(body.contains("Commit: [`0123456`]"));
    assert!(body.contains("## What's Changed"));
    assert!(body.contains("### Bug Fixes"));
    assert!(body.contains("Release the key on exit"));
    // The commit that produced v0.1.0 is already released, so it must not be
    // repeated in the next version's notes.
    assert!(!body.contains("Initial remapper"));
}

#[test]
fn stable_notes_link_the_previous_release() {
    let repo = prepare_notes_repo("compare");
    let body = run_release_notes(
        &repo,
        &[
            "-Version",
            "0.2.0",
            "-Build",
            "0.2.0-7",
            "-Kind",
            "stable",
            "-OutputPath",
            "body.md",
        ],
    );

    assert!(
        body.contains(
            "**Full Changelog**: [v0.1.0...v0.2.0](\
             https://github.com/FlorianObermayer/remaprs/compare/v0.1.0...v0.2.0)"
        ),
        "missing compare link in:\n{body}"
    );
}

#[test]
fn nightly_notes_say_they_are_not_a_release() {
    let repo = prepare_notes_repo("nightly");
    let body = run_release_notes(
        &repo,
        &[
            "-Version",
            "0.1.0",
            "-Build",
            "0.1.0-7",
            "-Kind",
            "nightly",
            "-OutputPath",
            "body.md",
        ],
    );

    assert!(body.contains("not a stable release"));
    assert!(body.contains("## Changes since v0.1.0"));
    assert!(
        body.contains("**Full Changelog**: [v0.1.0...nightly]"),
        "missing nightly compare link in:\n{body}"
    );
}

#[test]
fn notes_never_stack_two_version_headings() {
    let repo = prepare_notes_repo("heading");
    let body = run_release_notes(
        &repo,
        &[
            "-Version",
            "0.2.0",
            "-Build",
            "0.2.0-7",
            "-Kind",
            "stable",
            "-OutputPath",
            "body.md",
        ],
    );

    // git-cliff emits its own "## [0.2.0]" heading; the body already has
    // "## What's Changed" above the changelog.
    assert!(
        !body.contains("## [0.2.0]"),
        "duplicate heading in:\n{body}"
    );
    assert_eq!(body.matches("## What's Changed").count(), 1);
}

#[test]
fn notes_are_rewritten_not_appended() {
    let repo = prepare_notes_repo("idempotent");
    let args = [
        "-Version",
        "0.2.0",
        "-Build",
        "0.2.0-7",
        "-Kind",
        "stable",
        "-OutputPath",
        "body.md",
    ];

    let first = run_release_notes(&repo, &args);
    let second = run_release_notes(&repo, &args);

    assert_eq!(first, second, "re-running must not append a second copy");
}

#[test]
fn changelog_groups_commit_types_into_sections() {
    let repo = ScratchRepo::new("groups");
    copy_into(&repo, "cliff.toml");
    copy_into(&repo, "scripts/release-notes.ps1");
    repo.commit("a.txt", "feat: add a binding");
    repo.commit("b.txt", "fix: keep the key held");
    repo.commit("c.txt", "docs: explain the limits");
    repo.commit("d.txt", "test: cover the engine");
    repo.commit("e.txt", "chore: tidy up");

    let body = run_release_notes(
        &repo,
        &[
            "-Version",
            "0.1.0",
            "-Build",
            "0.1.0-1",
            "-Kind",
            "stable",
            "-OutputPath",
            "body.md",
        ],
    );

    for section in [
        "### Features",
        "### Bug Fixes",
        "### Documentation",
        "### Testing",
        "### Miscellaneous",
    ] {
        assert!(body.contains(section), "missing {section} in:\n{body}");
    }
}

#[test]
fn changelog_skips_the_release_commit_itself() {
    let repo = ScratchRepo::new("skip-release");
    copy_into(&repo, "cliff.toml");
    copy_into(&repo, "scripts/release-notes.ps1");
    repo.commit("a.txt", "feat: add a binding");
    repo.commit("b.txt", "chore(release): v0.2.0");

    let body = run_release_notes(
        &repo,
        &[
            "-Version",
            "0.2.0",
            "-Build",
            "0.2.0-1",
            "-Kind",
            "stable",
            "-OutputPath",
            "body.md",
        ],
    );

    // The version bump is noise in its own release notes.
    assert!(
        !body.contains("chore(release)"),
        "release commit leaked into:\n{body}"
    );
    assert!(body.contains("Add a binding"));
}

#[test]
fn bump_script_bumps_patch_by_default() {
    let repo = ScratchRepo::new("bump-patch");
    copy_into(&repo, "scripts/bump-version.ps1");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "--quiet", "--message", "chore: fixture"]);

    let output = run_bump(&repo, &[]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("0.1.0 -> 0.1.1"));
    assert!(repo.read("Cargo.toml").contains("version = \"0.1.1\""));
}

#[test]
fn bump_script_honours_major_and_minor() {
    for (part, expected) in [("minor", "0.2.0"), ("major", "1.0.0")] {
        let repo = ScratchRepo::new(&format!("bump-{part}"));
        copy_into(&repo, "scripts/bump-version.ps1");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "--quiet", "--message", "chore: fixture"]);

        run_bump(&repo, &[part]);
        assert!(
            repo.read("Cargo.toml")
                .contains(&format!("version = \"{expected}\"")),
            "{part} bump did not produce {expected}: {}",
            repo.read("Cargo.toml")
        );
    }
}

#[test]
fn bump_script_dry_run_writes_nothing() {
    let repo = ScratchRepo::new("bump-dry");
    copy_into(&repo, "scripts/bump-version.ps1");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "--quiet", "--message", "chore: fixture"]);
    let before = repo.read("Cargo.toml");

    let output = run_bump(&repo, &["-DryRun"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("[dry-run]"));

    assert_eq!(repo.read("Cargo.toml"), before);
    let status = repo.git(&["status", "--porcelain"]);
    assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "");
}

#[test]
fn bump_script_prints_commands_that_paste_verbatim() {
    let repo = ScratchRepo::new("bump-paste");
    copy_into(&repo, "scripts/bump-version.ps1");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "--quiet", "--message", "chore: fixture"]);

    let output = run_bump(&repo, &["minor"]);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    // The release steps are meant to be selected and pasted into a shell, so
    // every command line must start at column 0 with no list numbering. An
    // indented "1. git ..." block is not runnable, which is what this guards.
    let commands: Vec<&str> = stdout
        .lines()
        .filter(|line| line.starts_with("git ") || line.starts_with("gh "))
        .collect();

    assert_eq!(
        commands,
        [
            "git switch -c release/v0.2.0",
            "git commit -am 'chore: bump version to 0.2.0'",
            "git push -u origin release/v0.2.0",
            "gh pr create --fill --base main",
        ],
        "release commands must be pasteable as printed:\n{stdout}"
    );

    // Merging is out of scope for a script that only bumps the version: it is a
    // separate decision, and `--admin` would skip the checks if it were pasted
    // along with the commands above.
    assert!(
        !stdout.contains("gh pr merge"),
        "the bump script must not merge the pull request:\n{stdout}"
    );

    // No leftovers of an indented or numbered variant.
    assert!(
        !stdout
            .lines()
            .any(|line| line.starts_with(' ') && (line.contains("git ") || line.contains("gh "))),
        "an indented command would paste with leading whitespace:\n{stdout}"
    );
}

#[test]
fn bump_script_refuses_to_bump_a_dirty_tree() {
    let repo = ScratchRepo::new("bump-dirty");
    copy_into(&repo, "scripts/bump-version.ps1");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "--quiet", "--message", "chore: fixture"]);
    repo.write("unrelated.txt", "staged work");
    repo.git(&["add", "unrelated.txt"]);

    let output = run_bump_raw(&repo, &[]);
    assert!(
        !output.status.success(),
        "a bump must not mix with staged work"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("uncommitted changes"),
        "unexpected error: {stderr}"
    );
}

#[expect(
    clippy::expect_used,
    reason = "a script that will not run makes the test meaningless"
)]
fn run_bump_raw(repo: &ScratchRepo, extra: &[&str]) -> Output {
    Command::new("pwsh")
        .arg("-NoProfile")
        .arg("-File")
        .arg(repo.path().join("scripts/bump-version.ps1"))
        .args(extra)
        .current_dir(repo.path())
        .output()
        .expect("run bump-version.ps1")
}

fn run_bump(repo: &ScratchRepo, extra: &[&str]) -> Output {
    let output = run_bump_raw(repo, extra);
    assert!(
        output.status.success(),
        "bump-version.ps1 {} failed: {}",
        extra.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
