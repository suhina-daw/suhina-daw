---
description: Proposed way for the community to build devices as Pure Data patches (plugdata, libpd) with access to commands, and how imported Max devices are kept
read_when: Pure Data, Pd, plugdata, libpd, patch, patch device, community devices, Max for Live, Max device, amxd, scripting devices, collect and save
type: decision
status: proposed
built: no
last_verified: 2026-11-02
---

# 0008: Patch devices

- **Date:** 2026-10-11
- **Task:** project item "Patch devices: Pure Data patches with access to commands"

## Context

Some DAWs let users build their own devices in a visual patching language that also reaches
into the DAW itself: a patch can read the set, launch clips and change parameters, not only
process audio and MIDI. That is what made such devices a community: people share instruments,
effects, sequencers and controller tools built without a compiler.

Suhina should have the same, with open tools. Candidates:

- **Pure Data** (Pd): an open visual patching language (BSD-3-Clause). **libpd** embeds it as a
  C library.
- **plugdata**: a modern editor and plug-in built on Pd (GPL-3.0). It already runs as a CLAP,
  VST3 or LV2 plug-in, so patches can be used in any host that loads plug-ins.

Suhina already has what such devices need from the DAW side: every action is a command
(ADR 0002) and the MCP server shows that commands work as an object model for an outside
program. What is missing is a way for a patch to send them.

Imported sets can also contain the other DAW's patch devices (`.amxd` files). They need a
proprietary runtime and cannot run in Suhina.

## Decision (proposed)

1. **plugdata works as a plug-in, with no extra work.** Plug-in hosting has landed (ADR 0006,
   `crates/plugin-host`), so plugdata is expected to load like any CLAP or VST3 plug-in, out of
   process, and run Pd patches with audio, MIDI and parameters; it has not been tried yet. This
   is the first step and needs no Pd-specific code.
2. **Patch devices are a device kind of their own.** A patch device is a Pd patch (and its
   abstractions) that Suhina loads directly, without a plug-in in between. libpd runs it in
   the plug-in host process (ADR 0006), never in the engine process: a patch is user code and
   can hang or crash like a plug-in (AGENTS.md rule 5). It uses the same shared-memory block
   exchange and deadline as plug-ins, so no new `unsafe` crate is needed.
3. **A patch can run commands.** Beside audio and MIDI, a patch device gets a message channel:
   it sends `{id, params}` commands (the same registry the UI, CLI and MCP use) and receives
   results and engine state (transport, beat, playing clips). Edits are undoable like any
   other. What a device may do is declared in the device (read only, transport, edit) and
   shown to the user when it is added.
4. **Patches are part of the project.** A project refers to a patch device's files relative to
   the project folder; Collect and Save copies them in, so a project with community devices
   opens on another machine.
5. **Imported Max devices are kept, not run.** They stay `DeviceKind::Foreign` placeholders
   with their file reference and stored parameter values, once the format is known from our
   own sets (corpus series 8). Collect and Save copies the `.amxd` file into the project
   folder, so nothing is lost and a later export writes the device back. The audio passes
   through the placeholder unchanged.

## Not decided yet

- **The editor.** plugdata's editor is the obvious candidate (open the patch in plugdata
  standalone and reload on save), or a Suhina patch view later.
- **The command channel's shape:** Pd messages to command ids and JSON parameters, rate limits,
  and how a device's permissions are checked.
- **Which Pd objects are allowed:** vanilla Pd only at first; externals bring their own licences
  and native code.
- **Converting Max patches to Pd.** Both languages share their origin and many basic objects,
  so a converter could translate simple patches. Many devices use objects with no Pd
  counterpart (`gen~`, `js`, the DAW-specific objects, video), so it would only ever cover some.
  Its own decision, after series 8, and only from the `.amxd` format as observed in our own files.

## Consequences

- No Pd code was written before plug-in hosting existed; step 1 comes with it, once plugdata is
  tried as a plug-in.
- Licences fit: libpd is BSD-3-Clause and plugdata GPL-3.0, both compatible with
  GPL-3.0-or-later (AGENTS.md rule 10).
- The command registry becomes an interface for device authors as well as agents, so command
  ids and parameters must stay stable once published.

## Sources

- Pure Data: https://puredata.info
- libpd: https://github.com/libpd/libpd
- plugdata: https://plugdata.org, https://github.com/plugdata-team/plugdata
