---
description: How Suhina uses an optional component it does not ship — the manifest and the licence it must carry, adding and listing one, what the user sees, and what happens when an extension is missing, crashes or hangs
read_when: extension, add-on, optional component, extension manifest, extension.json, model weights, licence shown to the user, extensions.add, extensions.list, missing extension, inference runtime, surface script, Lua runner
type: spec
status: planned
last_verified: 2026-11-05
---

# Extensions

Suhina can use an optional component it does not ship and cannot vouch for: a program that runs a
model, a native inference runtime, a user's own tool. Such a component is an **extension**. The
decision behind it is [ADR 0015](../decisions/0015-extensions.md); this document describes how one
behaves.

- **Sources:** Suhina's own design, from the constraints in [ADR 0015](../decisions/0015-extensions.md).
  The licensing line follows the FSF's GPL FAQ (plug-ins and combined programs, mere aggregation) and
  Blender's add-on licensing guidelines; the process boundary is Suhina's own plug-in host.
- **Builds on:** the out-of-process host and its protocol ([ADR 0006](../decisions/0006-plug-in-hosting.md)),
  the command registry ([ADR 0002](../decisions/0002-commands.md)) and the permission model of patch
  devices ([ADR 0008](../decisions/0008-patch-devices.md) §3).
- **Status:** behaviour spec only; nothing is implemented. First user: stem separation.

## Terms

- **Extension:** a folder Suhina has been told about, holding a manifest and the program or programs
  the manifest names.
- **Provides:** what an extension offers Suhina, by name and version ("separate a clip into stems").
  A feature command names the extension that serves it.
- **State:** what Suhina knows about it now: `available`, `running`, `missing`, `refused`, `crashed`.
- **Job:** one run of what an extension provides (one separation of one clip), with progress and a
  cancel.

## The manifest

`extension.json` at the top level of the extension's folder.

| Field | Meaning |
|---|---|
| `id` | machine name: `[a-z0-9]` then `[a-z0-9._-]`, at most 64 characters; also the folder's name under `<settings>/extensions/` |
| `name` | display name, at most 128 characters |
| `version` | the extension's own version |
| `provides` | what it offers: for each, an id, a label, and the command namespace it serves (`separation`); a `surface` entry carries no namespace, and marks a surface program ([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md)) |
| `license` | an SPDX id, or `{ "name": …, "text_file": … }` with the rights holder's exact string and a file in the folder holding it |
| `license_source` | the URL the licence was read from |
| `license_checked` | the date it was checked, `YYYY-MM-DD` |
| `source` | where the component comes from: a project URL, or `user-provided` |
| `protocol` | the host protocol version it speaks |
| `programs` | each program: a relative path inside the folder, and its SHA-256 (with `runner` set, the script file; see *Running one*) |
| `ports` | on a surface program: the port name patterns it claims, matched as a controller profile's are (`*` wildcards, case ignored) ([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md)) |
| `runner` | when set (today `"lua"`): the extension is a surface program's script; the `programs` entry is the script file, never started as a process, and Suhina starts the shipped runner of that name with it (see *Running one*) |
| `permissions` | what it may do: `project.read`, `project.write`, `files.write:<folder>`, `audio.read` |
| `credit` | the attribution the rights holder requires, at most 512 characters |

Checked when the manifest is read, with everything capped:

- A missing manifest, a manifest without `license`, or a licence whose text cannot be read as UTF-8
  text → the extension is `refused` and the message names the reason. **No licence stated is a
  blocker, not a blank.**
- A `programs` entry whose file is gone, or whose SHA-256 does not match → `refused`, naming the file
  and whether it is missing or changed.
- `protocol` newer than Suhina speaks → `refused`; older → it runs when the protocol says the older
  version stays compatible.
- A `runner` value Suhina does not ship → `refused`, naming the value and the runners Suhina ships
  (`lua` today).
- Caps, because a manifest is input like any other: at most 32 programs, at most 16 `provides`
  entries, at most 64 `ports` patterns, an id at most 64 characters, a name at most 128,
  `credit` at most 512, and no path that leaves the extension's folder. A manifest that breaks
  a cap is refused with the reason.

## Adding, listing, removing

Commands, the same from the UI, the CLI and MCP ([ADR 0002](../decisions/0002-commands.md)):

| Command | What it does |
|---|---|
| `extensions.list` | every known extension: id, name, version, what it provides, state, licence name, source, and the reason when it is not usable |
| `extensions.add` | takes a folder, checks its manifest, copies it into the settings folder, and returns the licence and the permissions; not a project edit, so it is not an undo step |
| `extensions.remove` | forgets an extension; the user's own files stay where they are |
| `extensions.info` | one extension in full: the manifest, the licence text, the permissions, the state and the last error |
| `extensions.restart` | stops its program and starts it again (a crashed or wedged one) |

- Adding never copies a program into a project and never writes outside
  `<settings>/extensions/<id>/`.
- `extensions.remove` refuses while a job of that extension runs, and names the job.
- Extensions are kept in `<settings>/extensions/<id>/`, in the settings folder the library's index
  and the sample map's cache also live under; `SUHINA_EXTENSIONS` names another folder instead, for
  development.

## What the user sees

- **Settings › Extensions** lists them with their state, the licence's name and where it came from.
  An extension that is not usable is shown with its reason, not hidden.
- The licence text and the permissions are shown **before** the first time it runs, and stay reachable
  from the same list afterwards.
- A feature that needs one says so by name: "`separation` needs the Stem Split extension (separates a
  clip into four stems)". The message says what it provides and where the user can get it, when the
  manifest names a source.

## Running one

- Suhina starts the program as a child process and speaks the host protocol to it; the extension is
  never loaded into Suhina's process ([ADR 0006](../decisions/0006-plug-in-hosting.md)).
- A manifest with `runner` set is a **surface program's script**
  ([ADR 0019](../decisions/0019-midi-controllers-and-mapping.md)). The `programs` entry is the
  script's file, and it is never started as a process: the child process is the runner the name
  `runner` gives, the one Suhina ships, built from this tree and carrying no manifest of its own.
  The runner is a program Suhina ships, not an extension and not a surface program; the script's
  folder is both. The extension's state, its health and its restart
  are the runner process's, and the script's ports fall back to their profiles while that process
  is down.
- Suhina does the parts it owns: decoding the audio, the resampling and the DSP chain around the
  model, placing the result in the project, and the undo step. The extension does the part Suhina may
  not ship.
- Progress is the extension's, passed on as it arrives; cancelling stops the job and drops whatever it
  produced — a half-written result is never placed, and no partial file is left behind.
- An extension that exits, hangs past the deadline, or answers with something malformed fails the job
  with a message naming the extension and the reason. The project is untouched, the transport keeps
  playing, and `extensions.restart` tries it again.
- An extension's job runs off the audio thread and never on it. A job that runs while the project
  plays changes nothing about the mix until its result is placed, as one undo step.

## Not yet

- Downloading an extension or a model, and a registry or store to find one in.
- Trust beyond the SHA-256 in the manifest and the user's explicit add: no signature, no sandbox
  beyond the process boundary and the declared permissions.
- Extensions on the audio thread (real-time effects), and extensions that edit the project directly
  rather than through commands.
- Commands of an extension's own outside the namespace its manifest declares.
- Extensions that Suhina ships itself: what ships stays built from this tree and GPL-compatible.
  No extension is shipped yet: the runner a surface script names is a shipped program, not an
  extension (see *Running one*).
