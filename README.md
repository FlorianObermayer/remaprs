# remaprs

![AI-assisted](https://img.shields.io/badge/AI--assisted-GitHub%20Copilot-6e40c9)

Rebinds a held Xbox controller button or keyboard key to randomized keyboard or
mouse input.

A trigger drives one or more outputs in parallel. An output either **holds** a
key down, or plays a **sequence** of press/hold/pause steps. A sequence with a
single step that repeats is a plain spam: while the trigger is held, the program
presses the key, releases it after `hold_ms`, waits `pause_ms`, and repeats.
Releasing the trigger releases the key immediately.

```text
hold A  ──┬─────────────────────────────────────────────
          │
SPACE   ──┼┐  ┌──┐  ┌──┐  ┌──┐  ┌──┐
          │└──┘  └──┘  └──┘  └──┘  └──┘
          t0  t+65   t+130   t+195       hold 15 ms, pause 50 ms
          └hold┘└─pause─┘
```

The rhythm is even: with `hold_ms: 15` and `pause_ms: 50`, presses start at t0,
t+65, t+130, … A range such as `[40, 90]` draws a fresh pause for each cycle, so
the spacing varies but never accumulates drift. Set `initial_delay_ms` when you
do want a gap before the first press.

## Requirements

* Windows 10 or 11, x64. No driver installation and no administrator rights.
* An Xbox-compatible controller (XInput) for controller bindings; keyboard
  bindings work without one. PlayStation pads need a wrapper such as DS4Windows.
* To build: Rust 1.88+ with the GNU toolchain and MinGW-w64 binutils.

## Install

Download `remaprs-<version>-x86_64-pc-windows-gnu.zip` from the
[releases page](https://github.com/FlorianObermayer/remaprs/releases), unzip it
anywhere, and run `remaprs.exe`. Nothing else is needed: the executable has no
runtime dependencies beyond what Windows 10 and 11 already ship.

To build it yourself instead:

```powershell
git clone https://github.com/FlorianObermayer/remaprs "$env:USERPROFILE\git\remaprs"
cd "$env:USERPROFILE\git\remaprs"
cargo build --release
```

The binary lands in `target\release\remaprs.exe`.

Every release zip is published with a SHA256 next to it. To check a download:

```powershell
(Get-FileHash .\remaprs-0.1.0-x86_64-pc-windows-gnu.zip -Algorithm SHA256).Hash
```

### Toolchain notes

The project builds with the `x86_64-pc-windows-gnu` toolchain, which avoids a
multi-gigabyte Visual Studio Build Tools install:

```powershell
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-gnu
winget install BrechtSanders.WinLibs.POSIX.UCRT
```

`dlltool` from MinGW-w64 must be on `PATH`. The `windows-*` crates invoke it at
build time to create import libraries; the copy bundled inside `rustup`'s
`rust-mingw` component fails with `dlltool.exe: CreateProcess` on some installs,
which is why the WinLibs package is listed above.

## Quick start

```powershell
# Write a commented config next to the executable; it is then loaded automatically.
.\target\release\remaprs.exe config init

# Check it without touching the controller or sending anything.
.\target\release\remaprs.exe config check

# See what the pad is reporting, without emitting anything.
.\target\release\remaprs.exe --list-buttons

# Log the synthetic events without sending them.
.\target\release\remaprs.exe --dry-run

# Run it.
.\target\release\remaprs.exe

# Run it and keep a log file, so an exit is still explainable afterwards.
.\target\release\remaprs.exe --log-file remaprs.log
```

Edit the generated `remaprs.yaml` to add bindings; `examples/remaprs.yaml` is a
richer reference with sequences and parallel outputs, loadable with
`--config examples\remaprs.yaml`.

Press the toggle hotkey (default `F12`) to suspend or resume without quitting.
`Ctrl+C` exits and releases every synthetic key. If the program ever seems to
disappear, see [Troubleshooting](#troubleshooting).

## Running from VS Code

The repository ships run configurations, so no terminal typing is needed.

**F5** (or *Run and Debug*) offers four configurations:

| Configuration | What it does |
| --- | --- |
| `Run (example config)` | Release build with `examples/remaprs.yaml` |
| `Run (dry run)` | Release build, logs events instead of sending them |
| `Run (list buttons)` | Live controller readout |
| `Debug (breakpoints, example config)` | Debug build with symbols, dry run |

`Debug (breakpoints, …)` is the only one that can hit breakpoints: the release
profile sets `strip = true`, so release binaries carry no debug information.

*Terminal → Run Task* has the same starts plus `start (logged)` (which writes
`remaprs.log`), `config check (example)`, `build`, `build (release)`, `test`,
and `check (fmt, clippy, test, deny, release)`.

Both need the recommended extensions (**Rust Analyzer** and **CodeLLDB**); VS
Code offers them when the folder is opened. Editor diagnostics run the same
`cargo clippy --all-targets -- -D warnings` as the check script, so a clean
editor means a passing build.

## Configuration

`remaprs config init` writes a commented `remaprs.yaml` next to the executable;
[`examples/remaprs.yaml`](examples/remaprs.yaml) is the same file, with more
examples.

### Which file is loaded

1. The file named by `--config FILE`, exactly as given; a missing file is an
   error rather than a fallback.
2. Otherwise `remaprs.yaml` next to `remaprs.exe`, if it exists.
3. Otherwise the built-in defaults. Those contain no bindings, so the tool
   refuses to start until `--set` supplies one.

The working directory is never consulted, so a shortcut, a task runner and an
interactive shell all load the same file. `--no-config` skips step 2.

Whichever file is used is logged at startup and validated before anything runs.
`remaprs config check` performs exactly that step and prints every problem at
once; it needs no controller and never sends input.

### Root keys

| Key | Meaning | Default |
| --- | --- | --- |
| `version` | Schema version; must be `1` | `1` |
| `device` | Which pad to read and how often | see [`device`](#device) |
| `output` | How synthetic presses are encoded | `scancode` |
| `hotkey` | Suspend/resume key | `F12` |
| `safety` | Focus guard and release-on-exit | see [`safety`](#safety) |
| `bindings` | Button and key mappings | must not be empty |

Unknown keys are rejected rather than ignored, so a typo is visible instead of
silently taking a default.

### `bindings`

Each binding maps one controller button or keyboard key to one or more outputs.
All outputs of a binding start together and run in parallel.

| Field              | Meaning                                                                  | Default    |
| ------------------ | ------------------------------------------------------------------------ | ---------- |
| `source`           | `controller` or `keyboard`; tells the two `from` namespaces apart          | `controller` |
| `from`             | Controller button or, with `source: keyboard`, a keyboard key             | required   |
| `key`              | Shorthand for a one-step repeating sequence                              | —          |
| `outputs`          | List of outputs, each with its own `mode` (see below)                     | —          |
| `mode`             | `sequence` or `hold`; applies to `key` and is the default for `outputs`   | `sequence` |
| `pause_ms`         | How long the key stays up between presses, counted from the release. The cycle is `hold_ms + pause_ms`. `40`, or `[40, 90]` | `[40, 90]` |
| `hold_ms`          | How long each press stays down. `hold`: omit to keep the key down until the trigger is released | `[15, 35]` |
| `initial_delay_ms` | Delay between the trigger going down and the first press. `0` fires at once; same syntax | `0`        |
| `repeat`           | Restart after the last step                                             | `true`     |

A single value means "always this exact delay", which makes a mapping perfectly
regular: `hold_ms: 15` with `pause_ms: 50` presses at t0, t+65, t+130.
Ranges are sampled uniformly, once per cycle.

Because the pause is measured from the release, it is independent of the hold:
a long hold with a short pause, or a zero pause that re-presses the instant the
key comes up, are both valid.

**Buttons:** `A` `B` `X` `Y` `LB` `RB` `LS` `RS` `BACK` `START`
`UP` `DOWN` `LEFT` `RIGHT` `LT` `RT`.

**Keys** (`key`, and `from` when `source: keyboard`): any letter, digit,
`F1`-`F24`, plus `SPACE` `ENTER` `TAB` `ESCAPE` `BACKSPACE` `DELETE` `INSERT`
`HOME` `END` `PAGEUP` `PAGEDOWN` `UP` `DOWN` `LEFT` `RIGHT` `SHIFT` `LSHIFT`
`RSHIFT` `CONTROL` `LCONTROL` `RCONTROL` `ALT` `LALT` `RALT` `LWIN` `RWIN`
`CAPSLOCK` `NUMLOCK` `PRINTSCREEN` `PAUSE` `MINUS` `EQUALS` `COMMA` `PERIOD`
`SLASH` `BACKSLASH` `SEMICOLON` `APOSTROPHE` `GRAVE` `LBRACKET` `RBRACKET`
`NUMPAD0`-`NUMPAD9`.

**Mouse:** `MOUSE_LEFT` `MOUSE_RIGHT` `MOUSE_MIDDLE` `MOUSE_WHEEL_UP`
`MOUSE_WHEEL_DOWN`. Wheel directions emit a single scroll per press cycle.

Names are case-insensitive.

#### Keyboard to keyboard

A binding whose `source` is `keyboard` is driven by a physical key instead of a
controller button:

```yaml
bindings:
  - source: keyboard
    from: Q
    key: E
    pause_ms: [40, 90]
    hold_ms: [15, 35]
```

`source` is required for keyboard bindings because the two namespaces overlap:
`A` is both a pad button and a keyboard key. `Q` is unambiguous, and would be
rejected as a controller button, but `source` is still read literally.

Note that `GetAsyncKeyState` also reports synthetic presses, so the output key of
a keyboard binding cannot also be a keyboard trigger; the configuration is
rejected instead of letting the binding feed itself.

The keyboard is polled only for the keys that are actually configured as
triggers, and keyboard bindings work with no controller plugged in.

#### Parallel outputs

`outputs` lets one trigger drive several keys at once, each with its own mode:

```yaml
bindings:
  - from: X
    outputs:
      - key: Y            # spams Y while X is held
        pause_ms: [40, 90]
        hold_ms: [15, 35]
      - key: Z            # keeps Z down until X is released
        mode: hold
```

The modes are:

* `sequence` (default) — press the key, release it after `hold_ms`, wait
  `pause_ms`, repeat. With one step this is a plain spam; with several it walks
  the list. A single `key` with no `steps` is a one-step repeating sequence.
* `hold` — press the key once and keep it down. Without `hold_ms` it stays down
  until the trigger is released; with `hold_ms` it comes up on its own, which
  lets a hold outlast or undercut a sequence running beside it.

#### Sequences

```yaml
bindings:
  - from: Y
    outputs:
      - mode: sequence
        initial_delay_ms: [0, 50]   # random start
        repeat: true                # loop until Y is released
        steps:
          - key: G
            hold_ms: [30, 80]
            pause_ms: [100, 250]
          - key: H
            hold_ms: [40, 60]
            pause_ms: [150, 300]
```

Every `hold_ms` and `pause_ms` is drawn fresh for each step, so a long-pressed
button produces a varied but reproducible-looking rhythm. `initial_delay_ms`
delays the whole sequence, and each output has its own, so parallel outputs can
be staggered. A `hold` output with a `hold_ms` range is the way to run a key for
"roughly N ms" alongside a sequence that loops.

Releasing the trigger cancels the sequence mid-step and releases whatever key is
down. Steps may repeat a key (`G`, `H`, `G`); only two different outputs of the
same binding may not emit the same key.

To trigger *one* key from the sequence *and* keep another key held for the whole
duration, list both as outputs:

```yaml
bindings:
  - from: X
    outputs:
      - mode: sequence
        repeat: true
        steps:
          - key: A
            hold_ms: 40
            pause_ms: 200
          - key: B
            hold_ms: 40
            pause_ms: 200
      - key: Z
        mode: hold
        hold_ms: [2000, 4000]
```

### `device`

| Field               | Meaning                                | Default |
| ------------------- | -------------------------------------- | ------- |
| `index`             | XInput slot, 0-3                       | `0`     |
| `poll_interval_ms`  | Controller read cadence                | `4`     |
| `trigger_threshold` | Trigger value (0-255) that counts as pressed | `30` |

### `output`

`key_mode` selects the Win32 encoding:

* `scancode` (default) — best compatibility with DirectInput and raw-input games.
* `virtual_key` — fallback when a game ignores scan codes.

### `hotkey`

`toggle` is the global suspend key; `none` disables it. Polling is used rather
than `RegisterHotKey`, because a registered hotkey is consumed system-wide and
the game would stop seeing that key.

### `safety`

`target_process` (for example `Game.exe`) limits emission to the time that
executable owns the foreground window. Any failure to identify the window counts
as "not focused", so a transient error sends nothing rather than sending input to
the wrong place. `require_focused` is the master switch.

`release_on_exit` releases every synthetic key on the way out. Turning it off is
only useful for debugging.

## Command line

```text
-c, --config <FILE>              YAML file describing the button mappings
    --no-config                  Ignore any config file; use the built-in defaults
-d, --device <INDEX>             Xbox controller slot to read (0-3)
    --set <BUTTON=KEY[:PAUSE[:HOLD[:DELAY]]]>
                                 Add or replace a controller mapping
    --set-key <KEY=KEY[:PAUSE[:HOLD[:DELAY]]]>
                                 Add or replace a keyboard mapping
    --dry-run                    Log events instead of sending them
    --list-buttons               Print live controller input
    --toggle-hotkey <KEY>        Override the suspend key; `none` disables it
    --no-focus-guard             Ignore safety.target_process
    --log-level <LEVEL>          error | warn | info | debug | trace
    --log-file <FILE>            Append every log line to this file

config init [FILE] [--force] [--stdout]
                                 Write a commented starter remaprs.yaml
config check                     Validate the configuration this run would use
```

`config init` writes next to the executable unless `FILE` is given, because that
is where auto-loading looks; it refuses to overwrite an existing file without
`--force`, and `--stdout` prints instead of writing.

`config check` prints `<path>: ok`, or the path plus every problem it found, and
exits non-zero when there is one. It accepts `--config`, `--no-config`,
`--device`, `--set`, `--set-key`, `--toggle-hotkey` and `--no-focus-guard`, so
it checks the same effective configuration the run would use. `--dry-run` and
`--list-buttons` are deliberately not accepted: they belong to a run.

`--set` and `--set-key` may be repeated, e.g. `--set A=SPACE:40-90:15-35` and
`--set-key Q=E:50:20`. They replace an existing binding for the same trigger, so
the YAML file stays the single source of truth for everything else. Each produces
a one-step repeating sequence; `PAUSE` and `HOLD` accept a value or a range, and
`DELAY` is the optional `initial_delay_ms`; omit it to fire immediately.

## Troubleshooting

### The program exited and the window is gone

Its console belongs to the parent task or shortcut, so anything written to
stderr disappears with the process. Pass `--log-file` to keep a record:

```powershell
.\remaprs.exe --log-file remaprs.log
```

The file is appended to, not overwritten, so the history of several runs stays
intact. It receives every log line plus the final `error:` line and any panic,
which is enough to tell the cases apart:

| Last line in the log | What happened |
| --- | --- |
| `stopped: shutdown requested` | Clean exit: Ctrl+C, the console was closed, or the task was stopped |
| `input emission failed (...)` | `SendInput` was refused; the run continues and retries |
| `controller read failed (...)` | An XInput error; the pad is treated as disconnected and polling continues |
| `error: ...` | A startup failure, usually a bad configuration or an unreadable path |
| `panic: at ...` | A bug; please report that line |

Add `--log-level debug` for one line per synthetic event, which shows exactly
what was emitted and when.

`SendInput` is refused while the desktop is not the active input desktop — lock
screen, UAC prompt, Ctrl+Alt+Del, a disconnected RDP session — and when another
process calls `BlockInput`. Those are temporary, so a refused delivery is queued
and retried instead of ending the run. Windows often reports error 0 for it,
because `SendInput` does not always set the last error when it is blocked.

The only failure that still exits is an undeliverable *release* on the way out:
the process leaves with a non-zero status rather than pretending the keys came
up.

### It runs but nothing is typed

Check, in order:

* `--list-buttons` — if the pad is dead there, the game will not see it either.
* The focus guard: `safety.target_process` must match the foreground window, or
  run with `--no-focus-guard`.
* Elevation: a normal process cannot inject into an elevated game (UIPI).
* A `debug` log shows whether events were produced at all, which separates "the
  configuration never fired" from "the game ignored the input".

## How it works

* One thread polls XInput and the configured keyboard trigger keys, runs the
  state machine, and calls `SendInput`. Keeping it single-threaded makes the
  ordering between "read the inputs" and "fire what is due" deterministic and
  removes any locking.
* Each output of a binding owns one phase, and the phases all advance on the
  same poll, so parallel outputs stay in lockstep without threads or timers:
  `Idle -> Delayed(initial_delay) -> Down(release_at) -> Waiting(next press)`.
  `hold` presses once and stops; `sequence` walks its steps, each a `Down`
  followed by the step's pause, and loops while `repeat` is on.
* The trigger edge presses at once unless `initial_delay_ms` says otherwise, so
  a tap always produces one press.
* Each press is scheduled from the moment the previous one was due, not from
  "now", so the rhythm stays even instead of drifting a little later with each
  cycle. If the loop is starved for longer than a whole cycle, it resynchronises
  from the recovery point instead of replaying the missed presses in a burst —
  for multi-step sequences too.
* A physical release forces an immediate key-up regardless of phase, cancels a
  pending sequence step, and leaves nothing held.
* The loop sleeps until the earlier of the next poll and the next scheduled
  event, so the configured delays are honoured to about a millisecond.
* `timeBeginPeriod(1)` is raised for the lifetime of the process. Without it
  `sleep` rounds to ~15.6 ms and the jitter becomes meaningless.
* The hotkey watcher and the `Ctrl+C` handler only set atomic flags, so they can
  never race with the emission loop.
* Backend failures are survivable: a pad read that errors is treated as "no pad
  this frame", and refused input is queued and retried, so a lock screen or a
  UAC prompt pauses emission instead of ending the run.

## Limitations

These are inherent to a driver-free approach and are worth reading before filing
a bug.

* **This is not a cheat for competitive play.** remaprs is a local input remapper
  with no game integration, no memory access and no anti-cheat bypass. Using it
  may still violate the terms of service of a given game; that is your call and
  your risk. It is not affiliated with, or endorsed by, any game or hardware
  vendor.
* **The physical button still reaches the game.** Nothing hides it. Map triggers
  or buttons the game does not use, otherwise the game reacts twice. The same
  applies to keyboard triggers: the physical key still arrives.
* **`SendInput` only reaches the foreground window.** Run the tool while the game
  is focused; the `target_process` guard exists to stop it typing into other apps.
* **Elevation must match.** A non-elevated process cannot inject input into an
  elevated one (UIPI). If the game runs as administrator, run this as
  administrator too.
* **Anti-cheat may block synthetic input.** Several competitive titles drop
  `SendInput` events outright. There is no supported workaround here.
* **XInput limits:** four pads, analog triggers, no Guide button.
* **Config changes need a restart.** The file is read once at startup.
* Some games only read DirectInput or raw input; try `key_mode: virtual_key` if
  presses are ignored.

## Development

```powershell
./scripts/check.ps1          # format, lint, test, dependency policy, release build
./scripts/check.ps1 -Fast    # same, without the release build
./scripts/release.ps1        # build the release zip into dist\
./scripts/bump-version.ps1   # bump the crate version (patch/minor/major)
./scripts/release-notes.ps1  # render release notes (needs git-cliff)
```

`tests/tooling.rs` runs the release scripts against a throwaway git repository,
so a broken changelog template or version bump fails `cargo test` rather than a
release. It needs `git-cliff` on `PATH` (`cargo install --locked git-cliff`); CI
installs it, and `scripts/check.ps1` warns when it is missing.

Or run the individual steps:

```powershell
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check
```

CI runs the same four gates on every push and pull request, plus the release zip
build. See [`.github/workflows/ci-cd.yml`](.github/workflows/ci-cd.yml).

Contributions: [CONTRIBUTING.md](CONTRIBUTING.md) covers the ground rules and how
AI assistance is disclosed.

### Releasing

`scripts/release.ps1` stages `remaprs.exe`, the README, the license and the
example config into `dist\remaprs-<version>-x86_64-pc-windows-gnu.zip` and prints
its SHA256. The same layout is what the workflow attaches to a GitHub release, so
a local zip can be uploaded by hand if CI is unavailable.

The version comes from `Cargo.toml`; no script edits it. **Merging a version
bump into `main` is the release trigger**: the workflow asks whether a release
for `v<version>` exists yet, and a version that has never been released gets a
stable release. `main` is protected, so the bump goes through a pull request
rather than a direct push:

```powershell
./scripts/bump-version.ps1 minor   # or patch / major, or -DryRun to preview
git switch -c release/v0.2.0
git commit -am "chore: bump version to 0.2.0"
git push -u origin release/v0.2.0
gh pr create --fill --base main
```

Merging that PR is the release. `main` only accepts **rebase merges** (merge
commits and squash are disabled, and history must be linear), and the
`fmt, clippy, test, deny` and `release archive` checks must pass first. No admin
bypass is configured, so this applies to the maintainer too.

`bump-version.ps1` rewrites the `Cargo.toml` version line and runs
`cargo update --workspace` so `Cargo.lock` matches; a lock file left behind by a
manual edit makes `--locked` builds fail. It refuses to run on a working tree
with uncommitted changes, and it stops at the four commands above: the commit,
the PR and the tag are separate, deliberate steps. Pushing a tag by hand does
nothing on its own — the release job creates it.

Every later merge into `main` under an already-released version instead refreshes
a rolling `nightly` pre-release, which is deleted when the next version ships.
Neither path publishes a zip without the checks having passed on that commit.

Release notes are generated from the commit history by
[git-cliff](https://github.com/orhun/git-cliff) using [`cliff.toml`](cliff.toml),
which is why commit types are not cosmetic: they choose the section a change
lands in. [`scripts/release-notes.ps1`](scripts/release-notes.ps1) renders the
body and is what the workflow calls, so the notes can be previewed locally:

```powershell
cargo install --locked git-cliff
./scripts/release-notes.ps1 -Version 0.1.0 -Build 0.1.0-42 -Kind stable
```

Pass `-Kind nightly` to preview the nightly body. Without `GITHUB_TOKEN` the
script passes `--offline` to git-cliff, so it works on a private checkout but
omits the `by @user` attribution that the API would add.

The workflow runs on `windows-latest` with the same MinGW toolchain the checks use,
so a release build reproduces the documented layout and toolchain rather than a
different configuration.

### Linting

[Clippy](https://doc.rust-lang.org/clippy/) is the Rust linter, and the project
runs it with more than the defaults:

* `clippy::all`, `clippy::pedantic` and `clippy::cargo` as warnings, elevated to
errors by `-D warnings`.
* Restriction lints: `unwrap_used`, `expect_used`, `todo`, `unimplemented`,
`dbg_macro`, and `undocumented_unsafe_blocks` (every `unsafe` block needs a
`// SAFETY:` note). Tests are exempt from the two panic lints via
`allow-expect-in-tests` and `allow-unwrap-in-tests` in `clippy.toml`.
* `clippy.toml` adds project rules a generic linter cannot know, such as banning
`std::process::exit` and `std::process::abort` because they skip the key-release
guard, and `std::sync::Mutex` because the runner is single-threaded.

[cargo-deny](https://embarkstudios.github.io/cargo-deny/) covers what Clippy does
not: security advisories, yanked crates, dependency licenses, duplicate versions
and untrusted sources. Its policy lives in `deny.toml`.

Install the extra tool once:

```powershell
cargo install --locked cargo-deny
```

Everything that decides *what* to emit lives in `src/engine.rs`, `src/config.rs`,
`src/input.rs` and `src/keys.rs` and is free of Win32 calls, so those tests run
without a controller attached. The Win32 code is confined to `src/xinput.rs`,
`src/wininput.rs`, `src/sendinput.rs`, `src/winfocus.rs` and `src/hotkey.rs`.

## AI assistance

This project was written with the help of an AI coding assistant (GitHub Copilot
in VS Code). The disclosure exists so nobody has to guess; it does not mean the
shipped program contains or calls a model. `remaprs.exe` is ordinary Rust that
reads controller state and calls `SendInput`: no inference, no network access, no
training data at runtime.

What the disclosure commits the author to:

* A human reviewed every change, owns the design, and is accountable for the
  behaviour described here, including the [Limitations](#limitations).
* Commits name the tool in an `Assisted-by:` trailer, the convention used by the
  [Linux kernel](https://docs.kernel.org/process/coding-assistants.html) and
  [LLVM](https://llvm.org/docs/AIToolPolicy.html):

  ```text
  Assisted-by: GitHub Copilot (VS Code agent)
  ```

  The initial commit predates this section, so the statement above stands in for
  its trailer.
* Correctness rests on the test suite, not on who typed the code:
  `./scripts/check.ps1` runs 93 tests plus clippy, `cargo deny` and a release
  build, and CI repeats them on every push.
* The same statement is published in machine-readable form under
  `[package.metadata.ai]` in `Cargo.toml`, so tooling does not have to parse this
  section.
* Contributions written with AI tools follow the same rules; see
  [CONTRIBUTING.md](CONTRIBUTING.md).

On the legal side: this repository is not an "AI system" in the sense of
[EU AI Act](https://eur-lex.europa.eu/eli/reg/2024/1689/oj) Article 3(1), and the
Act's transparency duty for generated content (Article 50) addresses synthetic
audio, image, video and published text rather than source code. Free and
open-source releases are carved out by Article 2(12) unless they are high-risk.
The disclosure above therefore follows open-source convention, not a statutory
obligation, and is not legal advice.

## License

MIT. See [LICENSE](LICENSE).

remaprs is an independent project and is not affiliated with Microsoft, Sony or
any game publisher. Xbox is a trademark of Microsoft Corporation.

