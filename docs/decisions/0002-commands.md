---
description: Why every action is a command with an id, typed params and snapshot undo, shared by CLI, MCP and UI
read_when: adding a command, undo, redo, command registry, params, CommandSpec
type: decision
status: accepted
built: yes
last_verified: 2026-11-02
covers:
  - crates/session/src/commands.rs
  - crates/session/src/commands
  - crates/session/src/lib.rs
  - crates/session/src/projects.rs
  - crates/session/src/plugins.rs
  - crates/session/src/library.rs
  - crates/session/src/audio.rs
  - crates/session/src/devices.rs
---

# 0002: Every action is a command

- **Date:** 2026-10-11; amended 2026-10-11 to 2026-10-24 (transport, projects, plug-ins, the
  browser, audio, lists, devices, drags, policies), and written as one decision 2026-11-02 (#759)

## Context

Suhina is driven from several places: the CLI, an MCP server for agents, the window and MIDI
controllers. If each of them changed the project in its own way, they would drift apart, undo
would only work in some of them, and an agent could not do everything a person can.

[PhotoCraft](https://github.com/storytold/photocraft) (MIT/Apache-2.0) solves the same problem with a command registry: every menu item,
tool and dialog is a command with an id, a parameter description and a `run` function, and the
UI, CLI, control channel and MCP server all dispatch commands by id. We take the idea, not code.

## Decision

1. **A session and its commands.** The crate `suhina-session` holds a **session**: the open
   project, where it came from, and its undo history. Every change to a session goes through a
   **command**. Layers: `model` (L0) ← `als`, `engine` (L1) ← `session` (L2) ← CLI, MCP, UI.
2. **A command has a stable id** in `area.verb` form (`track.setPan`, `edit.undo`), a short label,
   a parameter description, a kind (below) and a `run` function. Each namespace's commands are in
   a module of their own, `crates/session/src/commands/<namespace>.rs`, with what runs them.
3. **Parameters and results are JSON**, which is what MCP and the CLI speak, and which keeps the
   registry independent of any one front end. Every command's parameters pass one check before it
   runs: no unknown key, every required one there, each of its declared type, with one form of
   error (`clip.move: start must be a number, not "x"`); commands read them through `Args`.
   Besides numbers, texts and booleans there are typed lists (`ParamType::Texts`, `Integers`,
   `Numbers`, `Notes`, `Clips`), and a parameter that names a place on disk has the type `Path`,
   which says what the command does there and by which the MCP server confines it.
4. **One edit, one undo step.** A command that changes the project records exactly one undo step.
   If it fails, or leaves a project that does not validate, the session is restored to how it was
   and the command returns an error; it never panics, whatever the parameters.
5. **Undo is a snapshot of the project** before the change. Projects are small (no audio data), so
   this is simple and always correct. If memory becomes a problem we switch to diffs without
   changing the command interface.
6. **Reading is a command too** (`project.info`, `project.tracks`), so agents discover the project
   the way they change it. `command.list` returns every command with its parameters; front ends
   build their help and the MCP tool list from it.
7. **Nine kinds of command.** `CommandSpec` is generic over what a command runs on, so every kind
   shares one shape, one parameter schema and one `command.list`:

   | Kind | Runs on | Changes the project | Examples |
   |---|---|---|---|
   | **read** | a session | no | `project.info`, `track.devices`, `device.list` |
   | **edit** | a session | yes, one undo step | `track.setVolume`, `clip.move`, `device.add`, `device.setParam`, `device.setAutomation` |
   | **history** | a session | moves through the undo history | `edit.undo`, `edit.redo` |
   | **transport** | the session's attached engine (a `Player`) | no; without an engine, an error that says how to start one | `transport.play`, `transport.setPosition`, `clip.launch`, `launcher.state` |
   | **projects** | `Projects` ([ADR 0007](0007-several-open-projects.md)) | no | `project.list`, `project.switch`, `project.close` |
   | **plugins** | `Projects` ([ADR 0006](0006-plug-in-hosting.md)) | no | `plugin.scan`, the scanned folders, `plugin.setHosting` |
   | **plugin** | one of the session's running plug-ins | no | `device.openEditor`, `device.restart`, `device.pluginParams` |
   | **library** | `Projects` ([ADR 0012](0012-file-library.md), [ADR 0013](0013-sample-map-analysis.md)) | no | `library.*`, `soundmap.*` |
   | **audio** | `Projects`, through the `AudioSystem` and `MidiSystem` traits the front end hands it | no | `audio.devices`, `audio.setOutput`, `midi.setInput` ([ADR 0017](0017-inputs-and-latency.md)) |

   Two transport commands work without an engine, because the session keeps their value and
   sends it to each new player: `launcher.setQuantization`, which is also the Arrangement view's
   grid ([arrangement editing](../spec/arrangement-editing.md) › The grid), and `track.setArm`.
   Adding a plug-in (`device.addPlugin`) is an edit like `device.add`. A successful edit that
   adds something from the browser (`clip.addFiles`, `track.add`, `device.add`,
   `device.addPlugin`) is recorded among the library's recent items by `Projects`, so every front
   end keeps them.
8. **A selection is one command.** A command that acts on mixer strips or arrangement clips takes
   a list: `tracks` (ids or names, as `track.group` takes them) or `clips` (each a track and an
   index as `project.clips` lists them, or a clip id, [ADR 0025](0025-object-identity.md)), at
   least one. The `set` commands take the value each item gets, the `adjust` commands how far each
   moves, so a selection keeps what is between its members ([arrangement
   editing](../spec/arrangement-editing.md) › Selecting clips and Volume, pan, mute and solo).
   A device is addressed by its track and its index in the chain (`device`, from 0, as
   `track.devices` lists them), a parameter by its stable id ([ADR 0004](0004-built-in-devices.md))
   or a plug-in's parameter id.
9. **One drag, one undo step.** A value dragged in the window is heard while it moves, so the
   window runs the command on every frame of the drag
   ([#313](https://github.com/suhina-daw/suhina-daw/issues/313)). `track.setVolume`,
   `track.setPan` and `transport.setTempo` take `continue`: with `true`, the change joins the undo
   step the previous call of the same command on the same target made, as long as nothing else
   happened in between; any other edit, a call without it (even one that changes nothing), undo
   and redo end that step. The window sends the first frame without `continue` and the rest, and
   the release, with it; a double-click reset is a step of its own. `device.setParam` does the
   same with `continuous: true`, a final call without it ending the gesture; every effective
   update marks the project changed, after a save mid-gesture too. A drag can send graphs faster
   than the engine takes them ([ADR 0005](0005-edits-while-playing.md)), so a continued call whose
   graph does not fit is not an error: the project keeps the value, and
   `Session::catch_up_player`, which the window calls every frame, sends it again until the engine
   has it.
10. **A command's rules are on its spec.** What follows from a command beside its work is in its
    `CommandSpec`'s `policy` ([#764](https://github.com/suhina-daw/suhina-daw/issues/764) › 1),
    not in lists of ids in other crates: whether the player hears an edit (`heard`), whether a
    backup comes first (`critical`), and whether an agent may run it (`agents`; for example,
    agents may scan and list plug-ins but not add or remove the folders a scan runs code from).

## Consequences

- New behaviour starts as a command with tests, then gets a UI. The UI never edits the model
  directly.
- Command ids are part of the public interface for agents; renaming one is a breaking change.
- The registry is the only list of commands: [`docs/commands.md`](../commands.md) is generated
  from it (`suhina-cli commands --markdown`) on `main` after every merge.
- Suhina does not write `.als` (there is no writer). A session opened from a Live Set is saved
  as a Suhina project (`.suhina`) with `project.save` and a path ([ADR 0010](0010-project-files.md)).
