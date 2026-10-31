---
name: suhina-ui
description: How Suhina's UI code (`crates/ui`, egui) is written and tested - where each view and menu lives, labels as the test API, theme tokens, menus with text fields, what has been seen of egui and kittest, the window test fixtures, and the checklist for adding a setting. Use when changing a view, a menu, a dialog, the Settings window or the theme, or writing or fixing a UI test.
---

Nothing below `crates/ui` knows egui, and the UI never changes a project itself: it pushes an
`Action` for a command (the [`suhina-commands`](../suhina-commands/SKILL.md) skill). Before
finishing, the checks of [`working-on-suhina`](../working-on-suhina/SKILL.md) apply.

## Where things are

- *What is on screen, in code*, at the top of `crates/ui/src/lib.rs`: the function that draws
  each part of the window and the one that handles its result.
- The views live in `crates/ui/src/<view>/` (`app/`, `arrangement/`, `browser/`, `detail/`,
  `session/`, `settings/`; `interaction/` has menus, drops and the keymap; `kit/` the theme,
  widgets and motion), with a module file beside each folder (`app.rs`, `browser.rs`, ...).

## The house style

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
- `get_by_label_contains` matches a part of a label, so a new item whose label contains an old
  one's ("Duplicate" and "Duplicate ×4") breaks the tests that find the old one: prefer
  `get_by_label` with the whole label.
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

## A setting

The rules are [`docs/spec/ui-settings.md`](../../../docs/spec/ui-settings.md) › Adding a setting
(page by task, at most seven in a group, one sentence of description, one control per kind of
choice). The places to touch:

1. **The value:** a field of `Settings` in `crates/ui/src/settings.rs`, with its default, saved
   in `settings.json` under its key (a key never changes once released).
2. **The row:** one entry in `ROWS` in `crates/ui/src/settings/settings_rows.rs`, under its page
   and section, with its name, description, keys and control (`cx.on_off(r, |s| &mut s.field)` or
   `cx.choose(...)`). The tests in `settings_rows_tests.rs` check the machine-checkable rules.
3. **What reads it:** the view or the app, from `Settings`.
4. **The spec and the website help:** the table in `docs/spec/ui-settings.md`, then
   `docs/guide-data/settings-help.json`, `python scripts/settings_guide.py`, and its `--check`
   (AGENTS.md › Settings include their website help).
