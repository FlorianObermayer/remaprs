# Copilot instructions

## Never commit

Do not run `git commit`, `git commit --amend`, `git rebase`, or any command that
creates a commit. Stage changes and *propose* a message instead.

## Disclosure

This project discloses AI assistance openly, and every commit you propose must
therefore end with a trailer naming the tool:

```text
Assisted-by: GitHub Copilot (VS Code agent)
```

Keep it in the trailer block, after a blank line. Pair it with the GitHub
co-author trailer so the contributor graph shows the assistance too:

```text
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
Assisted-by: GitHub Copilot (VS Code agent)
```

Never claim work was unassisted, and never remove an existing `Assisted-by` line.
See `CONTRIBUTING.md` and the README section "AI assistance" for the reasoning.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/):

```text
<type>[optional scope]: <description>
```

* Lowercase imperative description, at most 72 characters.
* Types: `feat`, `fix`, `refactor`, `perf`, `docs`, `style`, `test`, `chore`.
* Never `chore(release)`.

Creating branches, pushing, and opening pull requests is allowed.

## Style

* Rust 2024 edition, `rustfmt` defaults, maximum line length 100.
* Every public item has a doc comment; explain *why*, not *what*.
* `unsafe` blocks carry a `// SAFETY:` comment stating the invariant that makes
  them sound.
* Prefer returning errors to panicking. `unwrap` and `expect` are lint errors in
  non-test code (`unwrap_used` / `expect_used`): return a `Result` and add
  context at the call site instead. Inside tests both are fine.

## Architecture

Keep platform code out of the domain logic:

* `src/engine.rs`, `src/config.rs`, `src/keys.rs` must stay free of Win32 calls
  so they remain testable without a controller.
* Win32 belongs in `src/xinput.rs`, `src/sendinput.rs`, `src/winfocus.rs`,
  `src/hotkey.rs`.

## Testing

Every feature or bug fix needs a test. Prefer deterministic tests: feed the
engine a fixed clock value and `SeededJitter` rather than sleeping.

## Verification before handing work back

```powershell
./scripts/check.ps1 -Fast
```

That runs the same four gates individually:

```powershell
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
```

Clippy runs with `all`, `pedantic` and `cargo` plus restriction lints; both
`Cargo.toml` and `clippy.toml` are part of the policy. Do not silence a lint with
`#[allow(...)]` unless the reason is written next to it.

## Domain caveats to keep in mind

* `SendInput` is foreground-only and subject to UIPI; do not add features that
  pretend otherwise.
* Never leave a synthetic key held: every exit path must release what it pressed.
* The physical button cannot be suppressed without a kernel driver, and this
  project deliberately avoids drivers.
