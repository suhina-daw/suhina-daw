---
description: The plug-in host protocol - the control messages, the shared-memory regions and where they live, reconfiguring an instance, and the version history of the protocol, the region, the ARA source and the plug-in catalog
read_when: plug-in protocol, protocol version, hello, load, unload, get_params, set_param, param_changed, open_editor, place_editor, latency_changed, ara_update, reconfigure, shared memory, region version, memfd, OpenFileMappingW, sweep, catalog format
type: spec
status: current
last_verified: 2026-11-02
covers:
  - crates/plugins/src/protocol.rs
  - crates/ipc/**
---

# The plug-in protocol

What the engine and `suhina-plugin-host` say to each other ([ADR 0006](../decisions/0006-plug-in-hosting.md) › 3).
How hosting behaves is in [plug-in hosting](plugin-hosting.md).

## Versions

Both sides refuse a different version: the engine compares the host's `hello` answer, and
`Region::open` refuses a region made by another version. Each change to a message or a layout
raises its version and adds a row here.

**Control protocol** (`suhina_plugins::protocol::VERSION`):

| Version | What it added |
|---|---|
| 2 | `set_state` carries a VST3 controller's state with the component's |
| 3 | parameters: `get_params`, `set_param` |
| 4 | `editor_closed`, unasked |
| 5 | instance numbers: `load` names the instance, and every message about one carries it; `unload` answered `unloaded` |
| 6 | `open_editor` names the window that owns the editor (`owner`) |
| 7 | embedded editors: `open_editor`'s `embedded`, `place_editor` (not answered), `editor_resized` unasked |
| 8 | `latency_changed`, unasked |
| 9 | ARA: `load` may carry a document, `ara_update`, `ara_select`, and `fit_editor` ([ADR 0033](../decisions/0033-ara-hosting.md)) |
| 10 | `memory`, unasked once a second, and `host_memory` |
| 11 | `transport_key`, unasked when Space is pressed in a plug-in's window |
| 12 | ARA: `ara_playback` and `ara_notes`, unasked |
| 13 | ARA: `ara_export` and `ara_import`, a clip's edits carried to another track |
| 14 | `param_changed`, unasked (#828); `ara_playback`'s `cycle` and `cycle_on`; `reconfigure` and `reconfigured` |
| 15 | `open_editor` may carry the `title` of the editor's own window; `editor_title` changes it later (not answered) |

**Block region** (`ipc::region`): 2, events as fixed 16-byte records with their frame; 3, the
block's followers ([plug-in hosting](plugin-hosting.md) › One round trip). **ARA audio source**
(`ipc::source`): 1. **Plug-in catalog** (`plugin-catalog.json`): 3 keeps when each file was last
read; a catalog in format 2 is read as it is.

## Messages

Messages go over a local socket or pipe, one JSON object per message with an `op`.

- **The process:** `hello` (the version), `ping`, `quit`.
- **One instance:** `load` names the instance, a number the engine chooses, and every message
  about one instance and its reply carry it: `unload` (answered `unloaded`), state save and load
  (`set_state` with a VST3 controller's state beside the component's), parameters, editors, and
  `reconfigure` (below). An `error` carries the instance of the request it answers.
- **Editors:** `open_editor` names the window that owns the editor (`owner`: a Win32 `HWND` or an
  X11 window id), whether it is `embedded` in it, and the `title` of an editor in a window of its
  own (without one, the plug-in's name, or a CLAP plug-in's id); `editor_title` changes that title and is not answered;
  `place_editor` puts an embedded editor in the window's pixels and is not answered; `fit_editor`
  asks an embedded editor to take the room it has.
- **ARA** ([ADR 0033](../decisions/0033-ara-hosting.md)): `load` may carry a document,
  `ara_update` changes it, `ara_select` tells its editor the selected clip, and `ara_export` and
  `ara_import` carry a clip's edits to another track.
- **Unasked, from the host:** `editor_closed`; `editor_resized` when a plug-in gives its editor a
  new size; `latency_changed` when a running plug-in reports another latency; `memory` once a
  second, the host process's resident memory, which `host_memory` reads for the control bar's
  total; `transport_key` when Space is pressed in a plug-in's window, so the engine starts or
  stops playback; `ara_playback` when an ARA plug-in asks the transport to play, stop, move or
  loop (`cycle`, `cycle_on`); `ara_notes` when the notes it found in a region change; and
  `param_changed` (below).

**Parameters.** `get_params` lists each with its id, name, group, range, default,
current value, the plug-in's own text for it, steps and flags; `set_param` sets one in its
own range (a VST3 plug-in's normalized, a CLAP plug-in's plain), on a VST3 controller at
once and on the processor (VST3 parameter changes, CLAP parameter events) at the start of
the next block. The engine side keeps values set since the state was last read and sets
them again after a restart. `param_changed` (unasked, #828) tells the
session what the user moved in the plug-in's editor: the value the plug-in runs with now,
which the session keeps the project's values in step with — no undo step of its own, and
a change since the last save. A VST3 plug-in reports it through the host's component
handler (`performEdit`); a CLAP one comes out with the blocks as output parameter events,
which the host reads after each block (the only way CLAP has: its host params extension
has no call of the kind, and the host does not give that extension today, so its
`request_flush` stays unanswered and a change made while no block runs waits for the
next one).

## Shared memory

The engine's side makes each region
(a link's, or an ARA audio source's, ADR 0033) and the host opens it by an address; the
operating system frees it when both have let go of it, however they end, so a crash or a
kill leaves nothing (`crates/ipc/src/shm.rs`, #805):
- *Windows:* a mapping backed by the page file, named `Local\suhina-ipc-<pid>-<n>-<nanos>`
 (`suhina-ara-…` for a source) in the session's namespace, like the doorbells. The address
 is the name; the host opens it with `OpenFileMappingW`. Nothing is written to `%TEMP%`.
- *Linux:* a `memfd` (`memfd_create`, closed on exec). The address is
 `/proc/<engine pid>/fd/<n>#<token>`, which the host opens as a path while the engine holds
 it; the token is part of the memfd's name, and the host checks that the path is exactly
 the memfd `/memfd:suhina-<kind>-<token>`, so a descriptor number the engine closed and
 reused for another region never opens as the old one. Nothing is written to `/dev/shm`.
- *macOS (and other Unix systems):* a file in the temporary folder,
 `suhina-ipc-<pid>@<start>-<n>`, where `<start>` is the process's start time. A link's file
 loses its name as soon as the host has answered the load that named it (the host never
 opens a link twice); a source's keeps it until the engine drops the source, because a
 restarted host opens the same sources again. The file of a process that crashed before
 then is removed by the first region the next Suhina process makes (`crates/ipc/src/sweep.rs`):
 a pid and a start time name one process for good, so a file goes when no process has that
 pid with that start time, with no lock files, and a live process's files are never taken
 for a gone one's.

The same sweep removes files that earlier versions left on every system
(`suhina-ipc-<pid>-<n>-<nanos>` in `/dev/shm` or the temporary folder, and the
`suhina-ipc-<pid>.lock` files of a first fix's test builds) once no process has their pid. It never errors and never runs on the audio thread; a file it cannot remove is
left for the next start. *Limits:* on Windows a region counts against the commit limit (RAM
and page file) while it lives, as a source of several hours does; on macOS an ARA source's file
stays in the temporary folder for as long as the engine holds the source, by design, so a
restarted host can open it again, and a crash between making a link and the host's answer,
or while a source lives, leaves a file until the next Suhina process starts; an earlier version's file whose pid another process has taken stays
until that process ends.

## Reconfiguring a retained instance

Protocol 14 adds `reconfigure {instance, sample_rate}` and
`reconfigured {instance, latency}`. The engine must quiesce the instance's audio
link before requesting a change. Its shared-memory maximum block capacity is
unchanged; engine block-size changes within that capacity require no new link.
Rates must be finite, positive and at most 768000 Hz.
