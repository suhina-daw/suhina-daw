---
description: Planned per-note expression behavior for live controllers, native instruments, plug-ins and editable note curves
read_when: MPE, per-note expression, MIDI pressure, note curves, member channel, manager channel
type: spec
status: planned
last_verified: 2026-11-19
covers:
  - crates/device-api/src/lib.rs
  - crates/device-api/src/note_expression.rs
  - crates/device-api/src/note_expression/
  - crates/engine/src/live.rs
  - crates/midi/src/lib.rs
---

# Per-note expression and MPE

## Status

This is the implementation contract for [#921](https://github.com/suhina-daw/suhina-daw/issues/921)
and the shared instrument in [#1579](https://github.com/suhina-daw/suhina-daw/issues/1579).
[ADR 0039](../decisions/0039-per-note-expression.md) records the decisions.
The shared API provides runtime identity allocation, validated musical expression values,
atomic initial note state, explicit event-admission results, and an opt-in instrument capability
with bounded retirement feedback. These are contract primitives; no built-in opts in on this
branch. The current live path has ordinary notes, channel bend, pressure and controllers.
End-to-end identified events, live MPE, recording, editable curves and plug-in adapters are not yet built.
The rules below are planned behavior, not a claim that a controller already exercises them.

`Device::note_instances` returns `None` for an ordinary device. The validated
`dispatch_note_instance` helper returns `Unsupported` instead of dropping a note. Accepted
identified notes use `NoteInstanceDevice::event`; its result distinguishes capacity rejection,
duplicate IDs and unknown occurrences. Hosts commit new mappings only after successful
admission and drain final IDs into caller-owned storage. This endpoint will receive the same
payload from the planned `NoteEvent::Instance` queue wrapper after the chain migrates; no
second identity or expression representation is introduced for exported instruments.

## Runtime notes

Each sounding occurrence has an opaque `NoteInstanceId`, distinct from a note's editing ID.
An identified note-on carries its complete initial pitch/pressure/timbre state. Expression
targets the occurrence, not every voice at its key. Note-off starts release (or waits for
sustain); choke ends it immediately. The final voice ending retires the occurrence. A note
can own several layered voices; ending one layer does not retire the other layers' ID.

Native identified `On` remains note-on at velocity zero and retains its initial expression
state. Validation bounds finite attack and release velocities to 0..=1 without changing the
event kind; invalid keys and nonfinite velocities are rejected. MIDI 1.0 zero-velocity
note-on bytes encode note-off and are normalized by the MIDI protocol adapter before entering
the common event stream. The existing MIDI parser and legacy ordinary-note path retain that
behavior. Native CLAP note-on must not inherit that MIDI 1.0 encoding rule.

Expression uses semitones for pitch, 0..=1 for pressure/timbre, and `f64` values throughout
the control path. Defaults are pitch 0, pressure 0, timbre 0.5. Nonfinite input is ignored;
finite expression is bounded before it reaches DSP. A patch decides how pressure and timbre
affect its synthesis; an absent controller must not accidentally mute the default patch.

The host dispatches events between render spans at their exact frames. Same-frame arrival
order is preserved. Note-on state is applied before its first sample. A release followed by
a new note-on at the same frame creates distinct lifetimes, even for the same pitch. Unsupported
chains use a visible ordinary-note compatibility path; they do not claim independent expression.

## Input and zones

Input mode is per physical port: ordinary MIDI, automatic MPE, or manual MPE configuration.
Ordinary mode does not activate a zone implicitly; automatic mode consumes valid RPN 6
configuration. Manual mode shows both zone sizes and bend ranges and ignores automatic zone
reconfiguration until the user selects automatic mode. Settings and changes are commands.
Protocol channel numbers in code are 0..15; the UI displays 1..16.

MPE 1.1 defines a lower zone managed by channel 1 and an upper zone managed by channel 16.
RPN 6 chooses member counts; the latest configuration takes precedence over overlapping
membership. Its defaults are manager bend ±2 and member bend ±48 semitones. RPN 0 on one
member changes the whole zone's member sensitivity. Suhina accepts whole-semitone ranges
0..96. RPN selection is tracked per port/channel, either selector order is accepted, null
selection ends it, and an NRPN selection prevents a stale RPN from consuming data entry.
Malformed status/data bytes, incomplete messages and out-of-range zone counts are ignored.

Suhina's routing policies are:

- Changing zone membership chokes occurrences on affected channels and resets their controller
  state. No note from another port or unaffected zone is changed. New configuration and its
  reset are processed in one ordered operation. Configuring all 15 members disables the
  opposite zone, including its manager; configuring two zones leaves both managers outside
  their member sets. Tests cover both orders of conflicting full-zone configuration.
- Bend is member bend plus manager bend in semitones. Pressure is the greater of member and
  manager pressure. Timbre is `clamp(member + manager - 0.5, 0, 1)`. MIDI CC74 maps its center
  value 64 to 0.5, endpoints 0 and 127 to 0 and 1, linearly on either side. These combinations
  are Suhina's choice, not a claim that the protocol mandates one synthesis mapping.
- Controls received before note-on are latched for the next note. At physical key release,
  member bend, pressure and timbre are captured; subsequent member changes cannot move the
  sustained/releasing old note. Manager bend still changes sounding tails. Manager pressure
  and timbre are captured for released notes; active keys continue following them.
- More than one active key on a member channel is supported: member expression affects each
  active occurrence there. Repeated note-ons for the same port/channel/key are paired with
  note-offs oldest-first. A bounded per-source occurrence table, not a boolean key array,
  preserves the other occurrences and their tails.
- Polyphonic key pressure in ordinary mode affects active occurrences of that channel/key.
  In MPE mode it is supported on the manager channel for that manager's own keys, and ignored
  on member channels as required by MPE 1.1. It never becomes channel pressure for other keys.
- Manager sustain applies to its zone; member sustain is ignored in MPE mode. Physical key-up
  detaches member expression immediately, even while pedal hold delays the audible release.
  Lifting the pedal releases only held occurrences in that zone. Ordinary sustain remains
  source/channel scoped.
- CC123 releases keys and respects pedal hold. CC120 immediately chokes the addressed channel
  or zone, including tails. Port disconnection and user panic clear its held/sustained state
  and choke its notes. These operations are not collapsed into one all-notes-off event.
- Reset-all-controllers resets controls and lifts sustain for the scope without fabricating
  new key presses. The MIDI mode controls are distinct from ordinary controller reset.

The router retains source provenance and timestamp for recording. It emits resolved musical
events without allocating in the callback. It rejects an entire new note when there is no
bounded lifetime slot. Overflow recovery must still terminate already accepted notes.

## Note curves and editing

Each saved note may own three optional curves. Points have finite beat offsets in
`0..=note.duration` and finite values in the runtime ranges. Offsets are strictly increasing;
editing replaces an existing equal-time point, while malformed files with duplicates are
rejected. Interpolation is linear; before the first point and after the last, hold its value.
An absent curve means the neutral value. Constant curves may have one point. Limits are 4,096
points per curve and 262,144 expression points per clip content, enforced before allocation.
Curve values and note data participate in the same command validation and undo operation.

| Operation | Curve behavior |
|---|---|
| Move or start quantize | Keep relative offsets and values; the complete gesture moves with the note. |
| Transpose | Change base key; keep pitch offsets, pressure and timbre unchanged. |
| Resize right edge | Crop beyond the new duration, inserting the evaluated boundary value when needed; extending holds the endpoint. |
| Resize left edge | Crop/rebase offsets by the start change and insert the boundary value; extending holds the old initial value. |
| Stretch, including duration quantize | Scale offsets by new duration / old duration; reject zero or nonfinite divisors. |
| Split at an interior time | Evaluate each curve at the split; both parts get that boundary value, the right offsets are rebased. The first note keeps its editing ID, the second gets a new one. |
| Join adjacent notes | Require matching key/channel, touching endpoints, and equal evaluated boundary values for every curve; retain the first editing ID and concatenate rebased points. Reject a discontinuous or nonadjacent join rather than inserting a ramp into the earlier note. |
| Duplicate / copy content | Copy values with the normal editing identity rules; playback allocates independent runtime occurrences. |
| Delete / mute | Delete curves with their note, or suppress the whole muted occurrence. |

Playback prepares curves off the audio thread and evaluates against beat time in the common
renderer. Release holds the note's terminal value unless an explicitly addressed live event
changes it. Tempo changes use the same beat-to-frame map as notes. Loops, seeks and transport
stops terminate old occurrences and start fresh ones; they never reuse a runtime ID. Curves
are stored inside the note's representation, without runtime IDs. #928 coordinates the file
migration with notation as specified by ADR 0039; this document alone changes no file version.

## Plug-in boundaries

CLAP native note events retain the full address tuple in an adapter table. Wildcards resolve
against that table, not against only key or only note ID. Expression values become the common
units. Note-end feedback retires the mapping only after the instrument is finished.

VST3 note-expression support is queried per event bus/channel. The adapter maps the instrument's
declared types and normalization, rejects unsupported expressions visibly, and uses a bounded
host-ID table. It does not cast the runtime ID to 32 bits or assume every plug-in uses the same
tuning range. A plug-in without lifetime feedback retains bounded mappings until termination,
explicit choke or deterministic voice-budget reclamation; reclamation never retargets a stale ID.

Per-plug-in expression mode selects native expression, channel-MPE compatibility or ordinary
MIDI. Channel-MPE output emits configuration, initial controls and note-on in that order. It
reserves member channels for active keys; channel reuse cannot alter a captured old tail in a
native core. If a legacy recipient cannot guarantee tail isolation, the compatibility mode
states that limitation. Capacity exhaustion has an explicit voice/channel stealing policy.

The stock instrument, VST3 export and CLAP export consume the same `device-api` event contract.
Imported plug-ins remain sandboxed. Both IPC directions bound event counts and validate values.

## MIDI 2.0

Runtime identity, curves and musical units do not depend on MIDI 1.0 channel allocation.
Native per-note controllers can map through a future UMP adapter without reducing supported
32-bit dimensions to seven bits. This is not an implementation of every UMP message or a
lossless archival representation of arbitrary controllers. The MIDI-CI MPE Profile is a
separate configuration protocol; it is not a synonym for native per-note MIDI 2.0 messages.

## Acceptance and implementation order

| Work | Required deterministic evidence |
|---|---|
| #924 shared contract | Identity and retirement rules; curve operations; protocol sources; agreement on this ADR/spec. |
| #926 live built-ins | Two held notes with only one bent; initial controls before note-on; repeated keys; reused channels with old tails; both zones and ports; RPN/NRPN reset; member and manager range changes; sustain; poly key pressure; CC120 versus CC123; malformed input; fixed-capacity exhaustion. |
| #927 plug-in hosting | CLAP tuple/wildcard and note-end tests; VST3 normalization and ID mapping; channel-MPE allocation; malformed IPC; a crashing plug-in does not affect the engine. |
| #928 recording/model/playback | Record and reopen the gesture; old format defaults and malformed limits; every curve edit and undo; looping, seeking and tempo changes; bit-identical repeated offline renders. |
| #930 piano roll | Edit each dimension and undo it; keyboard/accessibility paths; boundaries and selected-note identity survive edits. |
| #929 MIDI files | MPE zone setup and expression import/export; explicit quantization and channel-budget limits; ordinary MIDI compatibility. |
| #1579 and #931 integration | The same identified event fixture renders identically in native and exported cores at 1, 17, 64 and 512 frame blocks; no audio callback allocations; tested voice stealing and release tails. |

Acceptance requires audible instrument tests as well as event assertions: a no-op expression
handler cannot pass by preserving IDs alone. Listening sessions compare prepared identical
performances; fixed-frame render tests guard pitch, envelopes, finite output and regressions.
Performance measurements are reported separately from deterministic tests, never used as a
wall-clock test assertion. Subsequent features remain open until their own end-to-end evidence
exists; the contract does not close the parent epic.

## Sources

- MMA/AMEI **M1-100-UM MIDI Polyphonic Expression v1.1**, 2022-04-14, sections 2.2.1,
  2.2.5-2.2.8, 2.3, appendices C-D; supplied reference copy. Publication information:
  [MIDI Association](https://midi.org/tag/midi-polyphonic-expression).
- [CLAP events](https://github.com/free-audio/clap/blob/main/include/clap/events.h).
- [MMA/AMEI UMP and MIDI 2.0 Protocol v1.0](https://amei.or.jp/midistandardcommittee/MIDI2.0/MIDI2.0-DOCS/M2-104-UM_v1-0_UMP_and_MIDI_2-0_Protocol_Specification.pdf),
  appendix D.3.1: MIDI 1.0 zero-velocity note-on conversion.
- [VST3 expression events](https://steinbergmedia.github.io/vst3_doc/vstinterfaces/structSteinberg_1_1Vst_1_1NoteExpressionValueEvent.html).
- [MIDI-CI MPE Profile v2.0.3](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-120-UM_v2-0-3_MIDI_Polyphonic_Expression_Profile.pdf).
