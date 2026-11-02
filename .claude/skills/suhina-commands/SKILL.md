---
name: suhina-commands
description: Checklist for adding or changing a Suhina command (the session's `area.verb` commands that the UI, CLI and MCP all run) - where its spec and code go, its policy, its parameters, the menu item or key that runs it, its tests and the documents to touch. Use when adding a command or a parameter to one, wiring a menu item or shortcut to a command, or asked which "duplicate" (or other similarly named action) is which.
---

Every change a user makes to a project is a command (AGENTS.md rule 6). The registry is
`suhina_session::commands::commands`; how it fits together is in
[`docs/architecture.md`](../../../docs/architecture.md). Before finishing, the checks of
[`working-on-suhina`](../working-on-suhina/SKILL.md) apply.

## A new command, or a parameter on one

1. **Find its namespace module**: `crates/session/src/commands/<namespace>.rs` holds the
   `COMMANDS` of `<namespace>.*` and the functions that run them (`clip.rs` holds `clip.*`). A new
   namespace gets a module there, a `mod` line and an entry in `NAMESPACES` in
   `crates/session/src/commands.rs`. Some namespaces sit beside the code they drive instead:
   `library.*` in `crates/session/src/library.rs`, `plugin.*` in `crates/session/src/plugins.rs`,
   `audio.*` and `output.*` in `audio.rs`, `midi.*` in `midi.rs`, `soundmap.*` in `soundmap.rs`,
   and part of `project.*` in `projects.rs` and `backup.rs`. `NAMESPACES` lists them all.
2. **Add a `CommandSpec`** to that `COMMANDS`: `id` in `area.verb` form (stable: agents call it),
   a one-line `label`, `params` built with `param`/`optional` and a `ParamType` (shared ones such
   as `CLIPS` and `MARKER` are constants beside them), `kind`, `run`, and `policy`:
   - `Policy::DEFAULT`: heard, so the playing graph is rebuilt after it;
   - `Policy::UNHEARD`: changes only what is shown (a name, a colour, a marker);
   - `Policy::CRITICAL`: a backup is written just before it;
   - `agents: Agents::Denied(..)` or `Agents::NotWhenAttached(Agents::SETTINGS)` for what an agent
     may not run (ADR 0028).
   A parameter that names a file is `ParamType::Path`, so MCP confines it to the project's roots.
   Its type confines a file or folder the command reads. One it writes (`PathKind::WriteFile`,
   `PathKind::WriteFolder`) needs the command's own case in `confine` in
   `crates/automation/src/confine.rs`, with a test in `confine_tests.rs`. Without it, the path
   sweep in `crates/automation/src/server.rs` fails. `docs/mcp.md` describes each such rule.
3. **Read the parameters** with `Args` (`crates/session/src/commands/args.rs`): `f64`, `index`,
   `bool`, `flag`, `text` and their `opt_` forms, `has` and `value`. Every command reads through
   it, so a rule (a finite number, a trimmed text, `null` as left out) and its error are written
   once; add a reader there rather than a helper beside the command. What a command does after
   it has run, for the open projects (share a clipboard, record a recent item, a save's steps), is
   its `Policy`'s `after`, not a check of its id.
4. **Return** the ids of what it made or changed as JSON (`{ "clips": [...] }`): the UI selects
   from it and agents chain on it.
5. **Tests** in `crates/session/tests/<area>.rs` with `support::run(&mut session, "id", json!(..))`
   (`crates/session/tests/support.rs`): the change, undo and redo, and every malformed parameter
   (missing, wrong type, out of range, an id that does not exist). A new test file gets a
   `mod` line in `crates/session/tests/main.rs`.
6. **Documents:** the spec of the behaviour in `docs/spec/`; `docs/mcp.md` only if agents use it
   in a new way. `docs/commands.md` is generated on `main`, so leave it alone.

## The menu item or key that runs it

The view never changes the project: it pushes an `Action` and the app runs it.

- **A menu item:** push `Action::new("area.verb", json!({..}))` in the menu's function in
  `crates/ui/src/interaction/menus.rs` (`clip_menu`, `slot_menu`, `track_menu`, `device_menu` ...).
  The table *What is on screen, in code* at the top of `crates/ui/src/lib.rs` says which function
  draws each menu.
- **What happens with the result** (select the copies, open the new clip): `App::run` in
  `crates/ui/src/app.rs`, matched on `action.id`.
- **A key:** a key action is not a command. Add it to `WINDOW_ACTIONS` (id, label, group; the
  array's length is in its type) and its default keys to `DEFAULTS` in
  `crates/ui/src/settings/keymap.rs`. `App::do_action` in `crates/ui/src/app/actions.rs` turns
  it into commands, per view and focus.
- **Test** in `crates/ui/tests/window/<part>.rs` from `demo_app()`: open the menu with
  `click_secondary()`, click the item by its label (`get_by_label`), and check the project.

## Which "duplicate" is which

- `clip.duplicate`: a command; a copy of each arrangement clip right after it.
- `range.duplicate`: a command; the time range and everything in it.
- `track.duplicate`, `scene.duplicate`, `device.duplicateMany`: commands on those objects.
- `selection.duplicate`: a key action (Ctrl+D), not a command. `App::do_action` turns it into one
  of the commands above from the view and its focus: the piano roll's notes, a range on an
  automation lane, the selected devices, or clips.

Copy, cut and paste work the same way: `selection.*` key actions resolved in `App::do_action`.
