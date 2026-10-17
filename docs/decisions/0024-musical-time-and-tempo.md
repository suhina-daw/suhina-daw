---
description: Musical time (beats) is the model's only time base; tempo and time signature over the song are a start value plus lists of changes edited in place (tempo points with steps and ramps, signature changes on bar lines), and two maps merge three-way by beat and by bar
read_when: tempo map, tempo change, tempo ramp, time signature change, musical time, beats, tick, frames_per_beat, transport, scene tempo, warp markers, tempo point, merge, issue 402
type: decision
status: current
last_verified: 2026-10-15
covers:
  - crates/model/src/lib.rs
  - crates/model/src/tempo.rs
  - crates/project-file/src/lib.rs
  - crates/engine/src/lib.rs
  - crates/engine/src/tempo.rs
  - crates/session/src/commands.rs
  - crates/session/src/tempo.rs
---

# 0024: Musical time and tempo over the song

- **Status:** accepted
- **Date:** 2026-10-15; amended and accepted 2026-10-17 to describe the tempo map that landed in
  [#474](https://github.com/suhina-daw/suhina-daw/pull/474) (see *Amendment*)
- **Task:** tempo and time-signature changes over the song
  ([#402](https://github.com/suhina-daw/suhina-daw/issues/402)). The sibling decisions are
  [0023](0023-collaboration-and-project-versions.md) (the shape of sharing and versions),
  [0025](0025-object-identity.md) (object identity), [0026](0026-project-folder-and-package.md)
  (the project folder and the format version) and [0027](0027-merge-and-versions.md) (merge).

## Context

A song changes tempo and metre, and every position in the model (clip starts, lengths and offsets,
loop ranges, fades, note times, warp markers, the playhead) is a position in time.
[ADR 0001](0001-daw-neutral-model.md) fixed the unit as `f64` beats and left a revisit open: "if
rounding errors appear against tick-based formats (FL uses PPQ ticks), we revisit this with a
rational or tick-based time type". The launcher already carried a per-scene tempo override, so
the model already admitted that the tempo is not one number.

Being wrong about the time base is a model-wide rewrite, and the shape of the tempo map decides
whether two versions of a project can be merged ([0023](0023-collaboration-and-project-versions.md),
[0027](0027-merge-and-versions.md)). Both are therefore decided here, while no `.suhina` file exists
outside the repository's tests.

The behaviour (what a ramp does, how bars are counted, the commands, what follows the map) is
specified in [`docs/spec/tempo-map.md`](../spec/tempo-map.md). This ADR records the representation
and why.

## Decision

1. **Every position in the model is musical time, in beats. Never wall-clock time, never seconds,
   never audio samples.** This is an invariant of `suhina-model`, not a convention: the model has no
   sample rate and no frame, sample or second field, so a position cannot be stored in another unit
   without adding one. A tempo change does not move a note: a note at beat 12 stays at beat 12, and
   only *when* beat 12 is heard changes. A position in beats also means the same thing on two
   machines whose audio clocks differ, which a position in seconds does not.

2. **The tempo is a start tempo plus a list of tempo points.** `Transport::tempo_bpm` is the tempo
   from beat 0, and `Transport::tempo_changes: Vec<TempoPoint>` holds the changes after it
   (`crates/model/src/lib.rs`, `crates/model/src/tempo.rs`). A point is `{ beat, bpm, ramp }`:
   without `ramp` the tempo steps to `bpm` at `beat`; with it, the tempo glides linearly in beats
   from the tempo before to `bpm`, which it reaches at `beat`. A ramp belongs to the point it arrives
   at, so the start tempo needs no flag and deleting a point never leaves a ramp pointing nowhere.

3. **The time signature is a second, separate list, keyed by bar.** `Transport::time_signature` is
   the signature from bar 0, and `time_signature_changes: Vec<TimeSignatureChange>` holds
   `{ bar, time_signature }` changes. A change is on a bar line by construction, so a bar is never
   cut. Tempo and metre are separate because they change independently and are keyed in different
   units: a tempo change is at a beat, a metre change at a bar.

4. **The lists are edited in place, by commands.** `tempo.addPoint`, `tempo.movePoint`,
   `tempo.deletePoint`, `timeSignature.add`, `timeSignature.move` and `timeSignature.delete` are
   edits with one undo step each ([ADR 0002](0002-commands.md)), and the lists are kept in beat (or
   bar) order. Points are addressed by their number in that order, as `tempo.points` lists them.
   **A beat holds at most a ramp and a step**, the ramp first: the tempo glides to the ramp's tempo
   and jumps there to the step's, a vertical line at the end of a slope. Tempo envelopes in other
   formats draw the same thing as two breakpoints at one time with different values (DAWproject
   allows any number of points at one time; a Live Set's glide that ends in a jump reads this way),
   and a format with a type per tempo event has the same two kinds, a jump and a ramp. A command
   replaces a point of its own kind on its beat and leaves the other kind. Any other pair on one beat
   says nothing more (two steps: the first lasts no time; a step then a ramp: the ramp glides over no
   beats), so the conversion keeps the later of such a pair, and only an imported map or a
   hand-written file can make one. A bar holds one time-signature change, and a change on a bar
   that has one replaces it.

5. **Merging is three-way, keyed by beat and by bar.** The tempo map of two versions is merged
   against their common ancestor ([0027](0027-merge-and-versions.md)) point by point, with the beat
   as the key (the bar for signature changes): a point added, moved or deleted on one side only is
   taken; the same change on both sides is taken once; different changes to the point on the same
   beat, or a change on one side and a deletion on the other, are a conflict reported to the user.
   No clock and no append-only history are needed, because the ancestor says what each side changed.
   The key is the beat with the ramp flag, the same key the commands replace by (decision 4), so
   the glide and the jump on one beat are two keys and each merges on its own.

6. **A scene's tempo is an override and never edits the map.** Launching a scene with a tempo or
   a time signature takes the launcher clock off the map; `transport.followTempoMap`, starting the
   transport again, or editing the map puts it back. A transport command never changes the project
   and records no undo step (ADR 0002), so a performance action is never an edit.

7. **Beats turn into time through one conversion, built off the audio thread.**
   `suhina_model::TempoMap` converts beats to time and back in any unit (seconds for the UI, frames
   for the engine), with the exact closed form for a ramp so a long ramp does not drift. It is built
   when a graph is built (it allocates) and only read on the audio thread (it does not).
   `frames_per_beat` is derived from it and is never stored in the model, the project file or a
   command's parameters. At a constant tempo the arithmetic is the one the engine always used, so a
   project without changes renders bit for bit as before.

8. **Malformed changes are bounded and never crash.** A tempo is 1 to 999 bpm, a point is after
   beat 0 and at most beat 120 000, and a project keeps at most 10 000 tempo points and 10 000
   signature changes (`MAX_TEMPO_CHANGES`, `MAX_TIME_SIGNATURE_CHANGES`). The project file refuses
   a file over these by name; the conversion ignores what an importer or a hand-edited model gets
   wrong, so a bad point never stops a project from playing (AGENTS.md rule 2).

9. **No format version bump.** `tempo_changes` and `time_signature_changes` are optional fields of
   `project.transport`, written only when they are not empty. A `version: 1` file without them is a
   project at one tempo, so files written before the map open unchanged
   ([ADR 0010](0010-project-files.md) decision 3).

10. **Equal beats are compared exactly.** Two points are on the same beat when their `f64` beats are
    equal: a beat comes from a command's parameter or from a file, not from accumulated arithmetic,
    so an exact key is what keeps the list free of near-duplicates and makes decision 5's key
    well-defined. Positions computed through the conversion are compared with a tolerance, as the
    model already does for fades.

## Consequences

- `model` holds the map and the conversion; `engine`, the launcher, the metronome and the stretch
  worker all read the map ([`tempo-map.md`](../spec/tempo-map.md) › *What follows the map*).
- `project-file` writes and reads both lists field by field; the round-trip test covers them.
- A tempo change while playing keeps the playhead on its beat; the new map decides when that beat is
  ([ADR 0005](0005-edits-while-playing.md)).
- Warped audio follows the map in pieces of constant speed cut at every change, and in pieces of at
  most a sixteenth note during a ramp ([ADR 0003](0003-time-stretching.md)).
- Editing the map in the window comes later; the commands edit it now (MCP and `suhina-cli`).
- A merge of two versions needs a common ancestor to merge the map (decision 5). Two folders with no
  common ancestor have no basis for a three-way merge of the map either; what then happens is
  [0027](0027-merge-and-versions.md)'s to decide.
- ADR 0001's revisit to a tick or rational time base stays open in principle and is closed in
  practice: changing the time base would touch every position, reader, writer, command and golden
  render. The concrete revisit condition is in *Alternatives considered* (c).

## Amendment (2026-10-17)

The first version of this ADR (2026-10-15, proposed) decided an **append-only list of segments**, each
a state at a beat carrying both tempo and time signature, with changes appended rather than edited
and two maps merged by set union. The tempo map that landed for #402 in #474 took a different shape,
and this ADR now records that one. The reasons for the change:

- **Append-only was bought for a merge without an ancestor, and Suhina's merge has one.**
  [0027](0027-merge-and-versions.md) merges three-way against a common ancestor, which is enough to
  merge an edited list by key (decision 5). A union of appended segments needs no ancestor but grows
  without bound and makes "delete this change" an append that supersedes, which no user means.
- **Ramps.** A tempo that glides is common music practice and is what a Live Set's tempo envelope
  holds. The first version rejected a curve as unmergeable (alternative d); a ramp that belongs to the
  point it arrives at is still keyed by one beat and merges like a step.
- **Tempo and metre are keyed differently.** A segment holding both had to repeat the signature at
  every tempo change and put a signature change at a beat that might be inside a bar. Two lists, one
  by beat and one by bar, have neither problem.
- **No version bump was needed.** The first version expected a format change; optional fields kept
  `version: 1` files readable unchanged (decision 9).

Decisions 1 (beats only), 6 (a scene's tempo is an override) and 7 (`frames_per_beat` derived, never
stored) are unchanged from the first version.

## Alternatives considered

**(a) Keep a single tempo.** No model change and nothing to merge. Rejected: a song with a
ritardando, a half-time section or a metre change could not be written, and every feature that needs
a tempo at a position (a grid, a metronome, warped audio, an imported tempo envelope) would carry its
own notion of a tempo change.

**(b) Changes at absolute seconds or host time.** Rejected: the map would be machine-specific, and
with the map in seconds and the notes in beats a tempo change would move notes, the failure decision
1 exists to prevent.

**(c) A rational or PPQ-tick time base instead of `f64` beats.** Rejected on measurement. `f64` gives
a resolution near 2 × 10⁻¹³ beats at beat 1 200 (ten minutes at 120 bpm), while a 960 PPQ tick is
about 1.04 × 10⁻³ beats, nine orders of magnitude coarser. The risk is accumulation and comparison,
not resolution, and decisions 7 and 10 handle it. **Revisit condition:** a tick-based format's
round-trip test (the `.als` and later `.flp` corpora) shows a position that cannot be represented
exactly in `f64` beats within the tolerance; then a superseding ADR reopens this.

**(d) An append-only list of segments merged by union.** The first version of this ADR; see
*Amendment*.

**(e) A free tempo curve** (an envelope with curved segments, or automation interpolated by a
function). Rejected for the song's tempo: a curve's shape is not keyed by one beat, so two edits to it
cannot be merged point by point, and the closed-form conversion of decision 7 would need numeric
integration. A linear ramp covers a glide; a curved one is many short ramps.

**(f) Derive the tempo from the audio instead of storing it.** Rejected: the tempo map is a decision
about the song, and a decision that is recomputed changes when the detector improves. Detection is
welcome as a command that proposes a map for the user to accept.

## Not decided yet

- **Editing the map in the window**: how the Arrangement's ruler and the control bar's tempo field
  add, move and delete points. Behaviour for the spec first.
- **A time-signature change in a Live Set**: where a set stores it and what happens to one inside a
  bar ([`tempo-map.md`](../spec/tempo-map.md) › *To observe*).

## Sources

- [`docs/spec/tempo-map.md`](../spec/tempo-map.md): the behaviour, the conversion and the commands.
- [ADR 0001: A DAW-neutral project model](0001-daw-neutral-model.md): positions are `f64` beats, and
  the revisit this ADR answers.
- [ADR 0002: Every action is a command](0002-commands.md): edits have one undo step; transport
  commands record none.
- [ADR 0003: Time-stretching and stretch modes](0003-time-stretching.md) and
  [ADR 0005: Edits while playing](0005-edits-while-playing.md).
- [ADR 0010: Suhina's own project files](0010-project-files.md): the versioned JSON and the rule that
  a later version keeps reading older ones.
- Issue [#402](https://github.com/suhina-daw/suhina-daw/issues/402) and pull request
  [#474](https://github.com/suhina-daw/suhina-daw/pull/474): the tempo map as implemented.
