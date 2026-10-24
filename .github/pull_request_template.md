<!-- Keep this short. Reviewers read many of these. -->

## What changed

<!-- One paragraph: the problem and the approach. -->

## Why

<!-- The behaviour this fixes or adds, and what it deliberately does not do. -->

## Checks

- [ ] `./scripts/check.ps1` passes
- [ ] New or changed behaviour has a test
- [ ] No synthetic key can be left held on any exit path
- [ ] No Win32 code added outside `src/xinput.rs`, `src/wininput.rs`,
      `src/sendinput.rs`, `src/winfocus.rs`, `src/hotkey.rs`

## AI assistance

- [ ] The commits carry an `Assisted-by:` trailer if a coding assistant wrote a
      meaningful part of this change
- [ ] I can explain every line of this diff and take responsibility for it

See [CONTRIBUTING.md](../CONTRIBUTING.md) and the README section "AI assistance".
