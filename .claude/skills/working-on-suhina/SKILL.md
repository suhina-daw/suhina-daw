---
name: working-on-suhina
description: How to work in the Suhina codebase - choosing between a review, new work and continuing an open pull request, where code goes, the UI and egui/kittest patterns this codebase uses (labels, theme tokens, menus with fields, modifiers over drops, repaints), the test fixtures, and the checks to run and report before finishing. Use when about to write or review code in this repository, when a build or test misbehaves, when adding a command, device or view, or when asked how to do something in Suhina.
---

[`AGENTS.md`](../../../AGENTS.md) holds the rules and decides designs; read it first. This skill
does not repeat it. It adds what the rules mean while working and what this codebase's code and
tests look like. Find the documents for a task with `python scripts/docs.py find WORDS` or `covers PATH`
([`docs/INDEX.md`](../../../docs/INDEX.md) lists them all).

## First: which task is this?

- **A review, or a question about the code:** read the sources and report findings. Write
  nothing to GitHub (no `/claim`, no branch, no issue) unless the user asks for it.
- **New work:** check the issue's linked pull requests and the open ones that touch the same
  paths, then claim and work as in [AGENTS.md › Tasks](../../../AGENTS.md#3-tasks).
- **Continuing an open pull request:** work on its branch. Do not claim again or open a second
  pull request. A pull request from a fork can only be pushed to by its author (or a maintainer
  when the author allows edits); otherwise report what to change.

## Where code goes

The crate map and layers are in [AGENTS.md › Workspace map](../../../AGENTS.md#2-workspace-map)
and [`docs/architecture.md`](../../../docs/architecture.md). The usual decisions:

- Something a user does to a project: a command in `crates/session/src/commands/<namespace>.rs` (id, params, policy,
  tests in `crates/session/tests/`). The view turns the gesture into
  `Action::new("command.id", json!({...}))` and never changes the project itself.
- A selection, a hover, a draft in a field: view state in `crates/ui`, not a command.
- Sound: `crates/engine` (no file I/O); a building block for effects: `crates/dsp`; a built-in
  effect or instrument: `devices/<name>` ([ADR 0004](../../../docs/decisions/0004-built-in-devices.md)).
- Nothing below `crates/ui` knows egui.

## UI code, the house style

- The views live in `crates/ui/src/<view>/` (`app/`, `arrangement/`, `browser/`, `detail/`,
  `session/`, `settings/`; `interaction/` has menus, drops and the keymap; `kit/` the theme,
  widgets and motion), with a module file beside each folder (`app.rs`, `browser.rs`, ...).
- **Labels are the test and accessibility API.** An interactive thing sets
  `widget_info(|| WidgetInfo::labeled(...))` with a label as a user would say it ("Launch Walk on
  Bass (scene 2)"). Tests find it with `get_by_label`, so keep labels stable.
- **Theme colours are tokens** (`palette!` in `crates/ui/src/kit/theme.rs`), never a raw
  `Color32` for a semantic colour. A new token gets a value in every theme in
  `crates/ui/themes/`; the theme tests check that.
- `motion::mix`/`toward`/`pulse` for hovers and fills, `widgets::button_area` for clickable
  rects, `Status::Hint` for status-bar hints.
- **A menu with a text field** (a rename): egui's context menu closes on a click inside it, which
  closes the menu when the field or its button is clicked. Open it with
  `egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)`,
  keep the draft in egui temp data (`get_temp`/`insert_temp`) and remove it when the menu is not
  open, so a reopen starts from the current name. Example: the clip and slot rename in
  `crates/ui/src/interaction/menus.rs` (`clip_menu`, `slot_menu`, #563).
- **A field the tests must find** gets a `widget_info` label like any other control. #563 also
  gave its menu fields an explicit `.id(...)`, which its draft in temp data is keyed by and
  which it reports the label needed there; most fields in the views have no id and are found
  fine. Add an id when the field needs a stable identity or a test cannot find it, not by
  default.

## egui and kittest: what has been seen (egui 0.36)

Each item names where it was seen. Check it still holds after an egui update and remove it when
it no longer does.

- `clicked()` is true on both clicks of a double-click, and `double_clicked()` on the second.
  By the time the double-click arrives, the first click's action has run. Where both are
  handled, make the single-click action one that the double-click replaces or that is harmless
  to repeat (a selection), not an edit.
- A text field in a context menu takes typed keys in kittest: open the menu with
  `click_secondary()`, click the field, select its text with Ctrl+A
  (`key_press_modifiers(Modifiers::COMMAND, Key::A)`), then `type_text(...)` and
  `key_press(Key::Enter)`. See `a_slots_context_menu_renames_its_clip` and
  `an_empty_rename_field_clears_the_clips_name` (`crates/ui/tests/window/session.rs` and `arrangement.rs`, #563).
- `drop_at` carries no modifiers. To hold one over a drop, send
  `harness.event(egui::Event::ModifiersChanged(mods))` before the drag and
  `ModifiersChanged(Modifiers::NONE)` after it, as `crates/ui/tests/browser.rs` and
  `automation_lanes.rs` do.
- `harness.run()` runs until nothing repaints and fails when something repaints every frame
  (the sample map's layout, a playing meter). Use `run_steps(n)` or `step()` there
  (`browser_preview.rs`).
- A drag: `hover_at`, `step`, `drag_at`, `step`, `hover_at(target)`, `step`, `drop_at(target)`,
  `run`. A double-click needs its two clicks inside egui's double-click window: see
  `double_click_at` in `crates/ui/tests/detail_devices.rs` and `window_with_step(app, 1.0 / 60.0)`
  in `window.rs`.
- Tests that need an audio device are `#[ignore]`d. A software `suhina_engine::Renderer`
  attached with `session.attach_player(Player::new(renderer.controller(), None))` needs no device.

## Test fixtures

- `crates/session/tests` and `crates/ui/tests` are one test binary each, `tests/main.rs`, with
  a module per file (`autotests = false`). A new test file gets a `mod name;` line there; the
  binary's `every_test_file_is_a_module` test fails until it does. Run one file's tests with
  `cargo test -p suhina-ui --test main window::`; the filter is a substring match, so that also
  runs `settings_window::` tests. Add `-- --skip settings_window::` to run `window.rs` alone.
- `crates/ui/tests/window.rs`: the whole window, one module per part of it in `window/`
  (`arrangement`, `session`, `mixer`, `detail`, `settings` ...). `window.rs` holds the shared
  `demo_app()` (the demo project), `window(app)`, `window_with_step(app, step)`.
- `crates/ui/tests/browser.rs`: `pack(&folder)` builds a library (drums, a loop, a MIDI file, a
  project), `wav()`, `midi()`, `row(name)` for a row's label.
- `crates/session/tests/launcher.rs`: commands against an offline renderer. `project()` has clips
  in slots that share contents, a deactivated clip and an empty slot; `run()`, `playing()`,
  `advance(&mut renderer, frames)`.
- Engine tests (`crates/engine/src/launcher.rs`, `chain.rs`): `Renderer::stopped(&project, ...)`,
  `renderer.handle(LaunchCommand::...)`, `renderer.process(&mut block)`; the monitor reads back what
  plays and what is queued.

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
update are in AGENTS.md, and `python scripts/docs.py impact` lists the documents that cover the
code you changed.
