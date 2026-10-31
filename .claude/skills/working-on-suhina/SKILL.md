---
name: working-on-suhina
description: How to work in the Suhina codebase - choosing between a review, new work and continuing an open pull request, where code goes, which area skill has the checklist for the change, and the checks to run and report before finishing. Use when about to write or review code in this repository, when a build or test misbehaves, or when asked how to do something in Suhina.
---

[`AGENTS.md`](../../../AGENTS.md) holds the rules and decides designs; read it first. This skill
does not repeat it. It adds what the rules mean while working and what this codebase's code and
tests look like. Find the documents for a task with `python scripts/docs.py find WORDS` or `covers PATH`
([`docs/INDEX.md`](../../../docs/INDEX.md) lists them all).

## First: which task is this?

- **A review, or a question about the code:** read the sources and report findings. Write
  nothing to GitHub (no `/claim`, no branch, no issue) unless the user asks for it.
- **New work:** check the issue's linked pull requests and the open ones that touch the same
  paths, then claim and work as in [`docs/workflow.md`](../../../docs/workflow.md) › Tasks.
- **Continuing an open pull request:** work on its branch. Do not claim again or open a second
  pull request. A pull request from a fork can only be pushed to by its author (or a maintainer
  when the author allows edits); otherwise report what to change.

## Where code goes

The layers and the crate table are in [`docs/architecture.md`](../../../docs/architecture.md). The
usual decisions:

- Something a user does to a project: a command in `crates/session/src/commands/<namespace>.rs` (id, params, policy,
  tests in `crates/session/tests/`). The view turns the gesture into
  `Action::new("command.id", json!({...}))` and never changes the project itself.
- A selection, a hover, a draft in a field: view state in `crates/ui`, not a command.
- Sound: `crates/engine` (no file I/O); a building block for effects: `crates/dsp`; a built-in
  effect or instrument: `devices/<name>` ([ADR 0004](../../../docs/decisions/0004-built-in-devices.md)).
- Nothing below `crates/ui` knows egui.

## The checklist for the change

Each area skill names the files, functions, test helpers and documents for its common changes:

| Change | Skill |
|---|---|
| A command, a parameter on one, the menu item or key that runs it | [`suhina-commands`](../suhina-commands/SKILL.md) |
| A view, a menu, the Settings window, a setting, the theme, a UI test | [`suhina-ui`](../suhina-ui/SKILL.md) |
| A parameter on a built-in device, a new built-in device | [`suhina-devices`](../suhina-devices/SKILL.md) |
| Playback, the launcher, an engine test, golden renders | [`suhina-engine`](../suhina-engine/SKILL.md) |
| *How to check it* steps | [`how-to-check`](../how-to-check/SKILL.md) |
| A bug seen in the running app (lag, a stall, something late) | [`trace-debugging`](../trace-debugging/SKILL.md) |

`python scripts/docs.py check` checks that every path and `suhina_*::` symbol a skill names in
backticks still exists, so a checklist that goes stale fails CI.

## Tests that do not depend on timing

CI runners are slow and uneven, so a test that depends on timing fails there now and then
(AGENTS.md rule 7). Do not write one; do this instead:

- **Drive the engine by hand.** Call `renderer.process(&mut block)` a fixed number of times; never
  run it against the clock or an audio device. For a plug-in in a host process, call
  `renderer.set_offline(true)`: it waits for every answer, where real time drops a block that
  misses a deadline of under a millisecond (`stopping_a_transport_sensitive_instrument_fades_its_waveform`).
- **Wait on a signal, not on time.** To read what a background thread made of what you gave it,
  synchronize with it: `Session::sync_recording(finish)` makes the recording thread empty its
  rings and report, and `finish` settles ended takes at once (`crates/session/tests/recording.rs`).
  Never `sleep` and hope. A new background thread gets such a barrier if its results are tested.
- **Fake the clock.** Code with a deadline or settling time takes the time as an argument or
  through an injectable clock, and the test moves it by a fixed step. The engine's plug-in
  deadlines read `crate::clock::now()`; its own tests start `clock::Manual` and stand-in plug-ins
  move it as they "wait" (`crates/engine/src/chain_wait_tests.rs`). It exists only under
  `cfg(test)`, so nothing in a shipped build can set it. Do not assert an elapsed
  time, a latency bound or "within N ms".
- **No retries, wider tolerances or `#[ignore]`** to hide a race: find what the test waits on.
- Check a threaded test by running it many times with the machine busy
  (`yes > /dev/null &` per core, then a loop of `cargo test -p <crate> <name>`).

## Before finishing

The checks are in [AGENTS.md › Before you finish a task](../../../AGENTS.md#4-before-you-finish-a-task).
Besides `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace`, CI also runs:

```sh
python scripts/docs.py check
python scripts/rename_suhina.py --check
python scripts/license_header.py --check     # after adding a source file
python scripts/parity/update_report.py --check-inputs
cargo deny check                             # when Cargo.lock changed
```

Report each check as **passed**, **failed** (with the output) or **not run** (and why). A check
that could not run in the environment is never reported as passed.

The pull request follows the repository's template. *How to check it* follows
[`docs/how-to-check.md`](../../../docs/how-to-check.md); the parity line and the documents to
update are in [`docs/workflow.md`](../../../docs/workflow.md) › What a pull request carries, and `python scripts/docs.py impact` lists the documents that cover the
code you changed.
