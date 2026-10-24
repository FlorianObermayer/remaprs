# Contributing

Thanks for looking. This is a small Windows-only tool, so the bar is mostly
"does it keep the invariant that no synthetic key is left held".

## Setup

```powershell
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-gnu
winget install BrechtSanders.WinLibs.POSIX.UCRT
cargo install --locked cargo-deny
cargo install --locked git-cliff
```

Then `./scripts/check.ps1` must pass. CI runs the same gates on
`windows-latest`; see [`.github/workflows/ci-cd.yml`](../.github/workflows/ci-cd.yml).

`main` is protected: changes arrive through a pull request, and only rebase
merges are allowed. Rebase your branch rather than adding a merge commit, or the
merge will be rejected for breaking linear history.

## Ground rules

* Every feature or bug fix needs a test. Deterministic tests are preferred: feed
  the engine a fixed clock value and `SeededJitter` instead of sleeping.
* Keep platform code out of the domain logic. `src/engine.rs`, `src/config.rs`
  and `src/keys.rs` must stay free of Win32 so they remain testable without a
  controller.
* Never leave a synthetic key held. Every exit path releases what it pressed.
* `SendInput` is foreground-only and subject to UIPI. Do not add features that
  pretend otherwise, and do not add a driver.
* Do not silence a lint with `#[allow(...)]` unless the reason is written next to
  it.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/):
`<type>[optional scope]: <description>`, lowercase, imperative, 72 characters at
most.

The type is load-bearing: release notes are grouped by it (see `cliff.toml`), so
a `fix:` that lands under "Miscellaneous" misleads whoever reads the release.

## AI-assisted contributions

Tools are welcome; accountability stays with the human. If a meaningful part of
your change was written by a coding assistant, add a trailer to each affected
commit:

```text
Assisted-by: <tool and client>
```

This follows the [Linux kernel](https://docs.kernel.org/process/coding-assistants.html)
and [LLVM](https://llvm.org/docs/AIToolPolicy.html) conventions: it exists to help
reviewers, not to track authors. You are still the author of the patch and should
be able to explain every line of it in review.

Trivial tool use needs no trailer: formatting, renaming, completion of
boilerplate, and spell-checking are out of scope.

Never let a tool decide about safety behaviour on its own; see the ground rules
above.
