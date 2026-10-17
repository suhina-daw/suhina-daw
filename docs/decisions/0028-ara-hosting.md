---
description: How ARA plug-ins (audio editors such as pitch editors) are hosted out of process, how Suhina's clips map onto an ARA document, how audio reaches the plug-in, and the first version's overlap policy, which never loses edits
read_when: ARA, ARA2, Melodyne, pitch editor, audio random access, playback region, audio source, audio modification, region sequence, document controller, overlapping audio clips, audio overlap, arrangement.audioOverlaps, stacked clips and ARA
type: decision
status: draft
last_verified: 2026-10-17
covers:
  - crates/model/src/overlap.rs
  - crates/session/src/commands.rs
---

# 0028: ARA hosting

- **Status:** proposed
- **Date:** 2026-10-17
- **Epic:** [#587](https://github.com/suhina-daw/suhina-daw/issues/587), under plug-in hosting
  ([#30](https://github.com/suhina-daw/suhina-daw/issues/30))

## Context

ARA (Audio Random Access) is an extension of plug-in formats that lets a plug-in read a clip's
whole audio, not only the block that plays now, analyse it and edit it in place: pitch and timing
editors are the best-known use. The host describes its arrangement to the plug-in as a *document*
(audio sources, versions of them with edits, the regions that play them, their order on tracks,
the tempo) and the plug-in renders those regions as a track's effect.

ROADMAP puts ARA after VST3 hosting works in the sandbox. It does
([ADR 0006](0006-plug-in-hosting.md)): VST3 plug-ins load, play, save their state and restart in
`suhina-plugin-host`, and the engine reaches them only through `crates/ipc` and the control
protocol (`crates/plugins/src/protocol.rs`).

**What was checked (2026-10-17):**

- **Licence.** The ARA SDK ([`Celemony/ARA_SDK`](https://github.com/Celemony/ARA_SDK), with
  [`ARA_API`](https://github.com/Celemony/ARA_API) and `ARA_Library`) is Apache-2.0 since
  2.0 draft 14 (2021-02, ChangeLog); `ARA_API`'s README says implementing the API in a host is
  normal use, not a derived work. Apache-2.0 is compatible with GPL-3.0-or-later (AGENTS.md rule 9).
  Version 2.3 (2025-11) supports VST3, CLAP, Audio Units and AAX as companion APIs. The AAX SDK
  is separate, needs registration with its vendor, and is not used.
- **Clean room.** The API is public, documented C headers (`ARAInterface.h`, `ARAVST3.h`,
  `ARACLAP.h`) and a manual; nothing is disassembled. What commercial ARA plug-ins do is learnt by
  running them and is written down only as observed behaviour (AGENTS.md rule 1).
- **The VST3 side** (`ARAVST3.h`): a module that supports ARA has a class in the category
  `ARA Main Factory Class` (`ARA::IMainFactory`), and its audio processor answers
  `ARA::IPlugInEntryPoint` and, for ARA 2, `IPlugInEntryPoint2::bindToDocumentControllerWithRoles`,
  which binds that instance to a document as playback renderer, editor renderer, editor view or
  any of them.
- **Threads** (`ARAInterface.h`): the document controller is always called from one thread, usually
  the host's main thread. `readAudioSamples`, by which the plug-in reads a source's audio, is
  called from the plug-in's own non-real-time threads, may block (*"The host may decide to let
  the thread sleep until the requested data is available"*), and must fill out-of-range samples
  with silence. Playback-region rendering runs in the plug-in's ordinary `process` call.
- **Overlaps.** ARA itself allows regions to overlap: *"Regions in a sequence can overlap, and such
  overlapping regions will sound concurrently."* The limit below is Suhina's own policy for its
  first version, not a limit of the API.
- **Persistence.** The edits live in the document controller, not in a VST3 component's state:
  the host stores them with `storeObjectsToArchive` (optionally for a part of the document) and
  restores them with `restoreObjectsFromArchive`, matched by the *persistent ids* the host gave
  each audio source and audio modification. A component state alone is therefore not enough.
  `cloneAudioModification` makes an independent copy of a version's edits ("make unique").
  ARA 2.3 adds `notifyDocumentDataChanged`, so a host learns when the archive would change.
- **Stacking is ordinary in Suhina.** Several clips may overlap on a track and all play
  ([#63](https://github.com/suhina-daw/suhina-daw/issues/63)); the arrangement draws them in
  sub-rows (`crates/ui/src/arrangement/timeline.rs`, `sub_rows`, which treats clips that end
  within 1e-9 beats of the next start as touching).
- **Identity.** Clips are still addressed by their position on a lane;
  [ADR 0025](0025-object-identity.md) and [#568](https://github.com/suhina-daw/suhina-daw/issues/568)
  give clips, lanes and scenes ids. ARA's persistent ids must not be positions.

## Decision

1. **The document controller lives in the plug-in host process,** on its main thread, which is
   ARA's model thread. Nothing ARA runs in the Suhina process (AGENTS.md rule 5). The host
   implements ARA's host-side interfaces there (audio access, archiving, content access, model
   updates, playback control) and forwards what the engine must know.

2. **One document per ARA plug-in product per project, in one process.** Every instance bound to a
   document (one per track the plug-in sits on) must live in the process that holds its document
   controller. ADR 0006's default, one process per plug-in file, already does this; the
   *own process per instance* choice is refused for an ARA plug-in, with the reason.

3. **The engine drives the document with edit batches.** Each command that changes what the
   document describes (adding, moving, trimming, splitting, deleting a clip; a tempo change; undo
   and redo of any of these) becomes one message with the changes between `beginEditing` and
   `endEditing`. A batch the host refuses leaves the document as it was, and the session rebuilds
   the document from the project when it cannot be sure (after a host restart). The batches are
   small JSON messages on the existing control protocol: names, ids, positions, never audio.

4. **Audio reaches the plug-in through shared memory, read only.** For each audio source the
   session writes the decoded samples the engine plays (the audio pool's, ADR 0011) into a region
   of its own, once and off the audio thread, and the host maps it read only.
   `readAudioSamples` copies from it with no round trip and no file I/O; reads past either end are
   silence. A source whose audio changes (relinked, re-recorded) gets a new region, and the host
   disables sample access on the old one before it goes, as ARA prescribes. Nothing new reaches the
   audio callback.

5. **The model maps onto the document like this:**

   | ARA object | Suhina | Persistent id |
   |---|---|---|
   | Audio source | an audio content's file (`AudioRegion`, by its pool key) | from the content id |
   | Audio modification | the edits of one content; clips that share the content share them | from the content id and a counter |
   | Playback region | an arrangement clip: start, length and offset as samples through the tempo map | from the clip id ([#568](https://github.com/suhina-daw/suhina-daw/issues/568)) |
   | Region sequence | a track's arrangement lane | from the track id |
   | Musical context | the tempo and time-signature changes ([ADR 0024](0024-musical-time-and-tempo.md)) | one per project |

   A duplicate shares its content and so its edits, as in the model (ADR 0001); *make unique*
   clones the modification. Ids are never reused (ADR 0025 decision 2), so an archive restored
   later never attaches edits to the wrong clip.

6. **Overlaps, first version: refused for ARA clips, per clip, never by losing edits.**
   - Two audio clips overlap when they sound on the same track and their arrangement spans share
     time: by their actual start and end, never by how many clips a track has. Clips that only
     touch (one ends where the next starts, within 1e-9 beats, as the stacking layout counts it)
     do not overlap. `Project::audio_overlaps` finds them and the read command
     `arrangement.audioOverlaps` reports them (built with this ADR).
   - The restriction is as narrow as the guarantee allows: it applies to a pair of clips only when
     at least one of them is bound to an ARA plug-in. Other clips on the same track, other tracks,
     stacked audio clips that no ARA plug-in edits, and MIDI stacking are never limited.
   - A command that would make such an overlap is refused before it changes anything, with an
     error naming both clips and the ways out: move the clip, put it on another track, shorten one
     of them, or render the ARA result to audio. Nothing is refused for clips that no ARA plug-in
     edits, so ordinary stacking keeps working as now.
   - A project that already holds such an overlap (imported, or saved by a build without the guard)
     opens with the affected clips' playback regions **suspended**: the region is not in the
     document, but its audio modification is, so the plug-in keeps the edits and stores them in
     every archive. The clip plays its plain audio, marked as suspended with the reason, until the
     overlap is removed; then the region is restored and plays its edits again.
   - Render to audio is always an explicit, undoable command that adds a new clip and keeps the
     original clip and its edits. Nothing is baked, reset or replaced on its own.

7. **The project keeps the archive.** Per ARA plug-in device the project holds the document
   archive as an opaque blob with the plug-in's `documentArchiveID`, and per clip the modification
   it plays. Save reads the archive the way it reads plug-in states (ADR 0006 decision 5: each one
   asked, at most a second, the last archive read standing in), and so do autosave and the backups
   ([ADR 0010](0010-project-files.md)). A host that crashes is restarted with the last archive, so
   at most the edits since the last read are lost, as with a plug-in's own state today.
   - **Missing plug-in:** the archive and the bindings stay in the project untouched and are saved
     back as they were; the clips play their plain audio and the device says the plug-in is
     missing.
   - **Missing audio file:** the source is left out of the document until the file is relinked;
     its modification stays in the archive. Relinking an ARA-bound source to a file of another
     length asks first, as the edits are tied to sample positions.
   - **Older projects** have no archive and read as before.

8. **Undo.** Suhina's undo covers what the host changed. Edits made in the plug-in's editor are
   the plug-in's; they make the project unsaved without an undo step, as a plug-in editor's
   changes do today (ADR 0010 decision 7). ARA has postponed host integration of the plug-in's
   own undo history (ChangeLog, *Proposed features*), so nothing more is possible yet.

9. **First version scope:** arrangement audio clips, VST3 plug-ins. Deferred, each to be added
   when integration tests show it works: looped clips, warped (time-stretched) clips, overlapping
   ARA regions, launcher clips (whose looping, quantised playback does not fit ARA's linear
   timeline), complicated shared-source layouts, CLAP's ARA companion API.

10. **Tests come first, and a test plug-in makes them possible.** `test:ara`, an ARA plug-in built
    into `suhina-plugin-host` as `test:vst3` is, records what it is told and reads audio, so CI
    runs the whole path without a commercial plug-in. The SDK's example plug-ins come next, then
    real plug-ins, by hand, per system.

## Alternatives considered

- **Audio through the control protocol** (the host asks, the engine answers with samples): every
  read would cost a round trip and move audio through JSON, which the protocol was never meant to
  carry, and a plug-in analysing a long file reads all of it. Rejected.
- **The host decodes the files itself:** no shared memory, but a second decoder in another
  process, which may decode differently (resampling, stems, a later decoder version) and so edit
  samples that differ from the ones the engine plays. It also gives the host process file access
  the rest of the boundary does not need. Rejected; kept in mind for files far larger than memory.
- **A dedicated socket for audio requests:** bounded and simple, but a round trip per read for
  data that does not change once decoded. Shared memory gives the same bound with none.
- **Disable ARA for the whole track while any overlap exists:** simpler to state, but one stacked
  clip anywhere on a vocal track would switch off the edits of every clip on it. Rejected for the
  per-clip rule, which the suspension (a region left out, its modification kept) makes safe.
- **Silently move or trim the clip that makes the overlap:** changes what the user did without
  asking. Rejected.
- **Render the ARA result on conflict:** turns a reversible state into audio without being asked.
  Offered only as an explicit choice.
- **In-process ARA:** ARA plug-ins expect the host to call them directly, and in-process would be
  simpler. AGENTS.md rule 5 rules it out, and decisions 1 to 4 show it is not needed: the
  API's own threading rules allow a host process to serve every call.

## Consequences

- New protocol messages and a new region kind in `crates/ipc`; Rust bindings to the ARA C API live
  in `crates/plugin-host` only, under ADR 0006 decision 6's `unsafe` rules.
- Persistence needs stable ids: the archive and bindings stage is blocked by
  [#568](https://github.com/suhina-daw/suhina-daw/issues/568). The ADR, the overlap detection,
  scanning, the host-side document and the audio regions are not.
- A track whose ARA clips overlap other audio clips behaves differently from one without ARA: the
  error and the suspension say why, and stacking stays the ordinary behaviour elsewhere.
- Shared memory per source costs the decoded audio's size once more while an ARA plug-in uses it.
- Platforms: the host runs on Windows, macOS and Linux, and ARA plug-ins exist for Windows and
  macOS; on Linux only test and example plug-ins are expected. Each system is marked supported
  only after the SDK's examples and a real plug-in pass there.

## Stages

The sub-issues of [#587](https://github.com/suhina-daw/suhina-daw/issues/587), in order:

1. This ADR, overlap detection in the model and `arrangement.audioOverlaps`.
2. Scanning marks the VST3 plug-ins that support ARA; *own process per instance* is refused for them.
3. The document controller in the host, the protocol's edit batches, `test:ara`.
4. Audio through shared memory.
5. The project keeps the archive: save, reopen, crashes, missing plug-in or file (blocked by #568).
6. Clip edits reach the plug-in; the overlap guard and suspension.
7. The UI: open a clip in its ARA editor, show conflicts and their fixes, render to audio.
8. The SDK's example plug-ins and real plug-ins on each system; this ADR records the results.

## Sources

- ARA SDK and API, licence, ChangeLog: https://github.com/Celemony/ARA_SDK, https://github.com/Celemony/ARA_API
  (`ARAInterface.h`: audio access controller, document controller, archiving, region sequences;
  `ARAVST3.h`: `IMainFactory`, `IPlugInEntryPoint`, `IPlugInEntryPoint2`)
- This repository: [ADR 0006](0006-plug-in-hosting.md), [ADR 0010](0010-project-files.md),
  [ADR 0011](0011-preview-and-added-audio.md), [ADR 0024](0024-musical-time-and-tempo.md),
  [ADR 0025](0025-object-identity.md), `crates/ui/src/arrangement/timeline.rs` (`sub_rows`).
