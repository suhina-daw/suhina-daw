---
description: Instrument preset version 2 preserves the accepted modal model and one verified referenced or collected source
read_when: Instrument save preset, collected preset, preset v2, source hash, modal profile state
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/device-registry/
  - crates/session/src/devices.rs
  - crates/session/src/files.rs
  - crates/session/src/relink.rs
  - crates/model/src/lib.rs
  - crates/ui/src/interaction/menus.rs
---

# Instrument presets

## Document and limits

Instrument writes the existing `suhina-preset` JSON format, version 2, with its stable
`instrument` device id, role, name and parameter values. A required `instrument` object holds
an optional `profile` array of the canonical modal codec's bytes and an optional `sample`:
`scope` (`library` or `preset`), `path`, `sha256` (64 lowercase hexadecimal characters), and
`bytes` (1 through 128 MiB). Profile bytes must decode through the one shared modal-profile
codec. Profile presence and `model_revision` agree; only the retained default renderer and the
implemented profile renderer are accepted. A missing required field, unknown version, malformed
payload or unsupported revision is an error, never a default-model substitution.

The entire document is at most 64 KiB and carries at most one source. Paths use `/`, are at
most 1024 bytes, and contain only normal portable components: no empty, dot, parent, absolute,
drive, UNC, device, control-character or backslash form. Collected filenames retain the owner's
name. Existing version-1 preset and NAM file-reference semantics are unchanged.

## Save and collect

`device.savePreset {track, device, name, collect?}` saves into the existing library preset
folder. `collect` defaults to false. A referenced Instrument preset requires the actually
loaded source to be inside the library; an outside source must be explicitly collected.
Collection stores the source below `<preset-name>.assets/<sha256>/<original-name>` beside the
preset and writes a `preset`-relative reference. It copies the encoded source, not decoded PCM.

Hash the bounded source and verify its current identity. Publish each complete asset before
the preset, then publish the preset through the shared atomic writer. An existing matching
asset is reused; a conflicting asset is not overwritten. A failed save leaves an older preset
whole. Unreferenced completed assets after an interrupted save are harmless and are not removed
by deleting unrelated files. Saving does not alter the loaded instrument or its undo history.

## Load, missing files and explicit replacement

`device.add {preset}` and `track.add {preset}` use the same prepared Instrument state and the
normal project edit/undo path, including grouped insertion. Validate the entire preset first.
Resolve inside the specified root without following a link outside it, check encoded length
and digest before decoding, and prepare source audio before project mutation. Corruption,
unsupported revisions or invalid paths change neither the target nor its history.

A genuinely missing source remains recorded, is reported in the command result and appears in
the ordinary missing-files workflow. It does not silently select another source. The accepted
modal profile is independent of that reference. A located replacement for a hashed missing
source must match its identity. An intentional new source uses `device.loadSample`, which
clears the old source identity in the same undoable edit.

Project save/reopen, collection and path rebasing preserve both the profile and source identity.
Audio preparation, file reads, hashing and copying never happen on the audio callback.
Existing caller file-access policies and the agent-origin restriction on `device.savePreset`
remain in effect; recording a digest does not authorize a file read.

## Verification

CI covers codec round trips and malformed fields; unknown model versions; v1 and NAM behavior;
source hash mismatch, traversal and oversize refusal; save failures; missing-source and matching
relink; one-step add undo/redo; and matching renders after relocating a collected preset folder
into a clean project. Verification never re-runs fitting.

## Not yet

This slice does not implement archive transport, multi-sample maps, future family schemas,
expression-profile packaging, automatic provenance acquisition or plug-in resource state.
Those remain in #1620, #1623 and #931. The canonical model payload comes from the coordinated
modal-profile implementation, not a private preset-only model.
