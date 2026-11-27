---
description: Planned per-note expression behavior for live controllers, native instruments, plug-ins and editable note curves
read_when: MPE, per-note expression, MIDI pressure, note curves, member channel, manager channel
type: spec
status: planned
last_verified: 2026-11-27
covers:
  - crates/device-api/src/lib.rs
  - crates/device-api/src/note_expression.rs
  - crates/device-api/src/note_expression/
  - crates/engine/src/live.rs
  - crates/engine/src/chain/mpe.rs
  - crates/midi/src/lib.rs
  - crates/midi/src/mpe.rs
  - crates/midi/src/mpe/
  - crates/session/src/mpe_config.rs
  - crates/session/src/midi.rs
  - crates/engine/src/live_mpe_tests.rs
---

# Per-note expression and MPE

## Status

This is the implementation contract for [#921](https://github.com/suhina-daw/suhina-daw/issues/921)
and the shared instrument in [#1579](https://github.com/suhina-daw/suhina-daw/issues/1579).
[ADR 0039](../decisions/0039-per-note-expression.md) records the decisions.
The shared API provides runtime identity allocation, validated musical expression values,
atomic initial note state, explicit event-admission results, and an opt-in instrument capability
with bounded retirement feedback. The live implementation carries identified notes through the
engine to FM Synth and Sampler; a direct SDK voice-group instrument also exposes per-voice
pitch (semitones), pressure and timbre signals. The native MIDI settings select off, automatic
or manual expression mode per input. A note holds its curves in the model and the project file
saves them (version 17); the piano roll edits them, and playback sends them to a plug-in
instrument as channel MPE ([Channel-MPE output](#channel-mpe-output)). Built-in instruments do
not play saved curves yet, recording does not capture expression, and native plug-in adapters
(CLAP note expressions, VST3 note expression) are not yet built.

`Device::note_instances` returns `None` for an ordinary device. The validated
`dispatch_note_instance` helper returns `Unsupported` instead of dropping a note. Accepted
identified notes use `NoteInstanceDevice::event`; its result distinguishes capacity rejection,
duplicate IDs and unknown occurrences. Hosts commit new mappings only after successful
admission and drain final IDs into caller-owned storage. This endpoint will receive the same
payload from the `NoteEvent::Instance` queue wrapper; no
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
Off preserves the existing ordinary channel/key path, including mono, legato and retrigger
allocation. Automatic and Manual enable identified occurrences and poly key pressure on
channels outside an MPE zone (and on its manager). Off does not consume poly key pressure
or activate a zone implicitly; automatic mode consumes valid RPN 6
configuration. Manual mode shows both zone sizes and bend ranges and ignores automatic zone
reconfiguration until the user selects automatic mode. Settings and changes are commands.
Protocol channel numbers in code are 0..15; the UI displays 1..16.

MPE 1.1 defines a lower zone managed by channel 1 and an upper zone managed by channel 16.
RPN 6 chooses member counts; the latest configuration takes precedence over overlapping
membership. Its defaults are manager bend ±2 and member bend ±48 semitones: each RPN 6 restores
them for the zone it addresses and retunes that zone's held notes, and Automatic mode starts from
them whatever ranges are saved for Manual mode. RPN 0 on one
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
  Releasing a key detaches its occurrence but does not erase the channel latch. Fresh neutral
  messages before channel reuse initialize the new note neutrally without changing the old
  tail. With no fresh controls, MPE 1.1 requires the last received values to initialize it.
- More than one active key on a member channel is supported: member expression affects each
  active occurrence there. Repeated note-ons for the same port/channel/key are paired with
  note-offs oldest-first. A bounded per-source occurrence table, not a boolean key array,
  preserves the other occurrences and their tails.
- Polyphonic key pressure on ordinary channels in Automatic/Manual mode affects active
  occurrences of that channel/key.
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

The router retains source provenance and timestamp for recording. Translation itself emits
resolved musical events without allocating. The existing opt-in MIDI trace runs only on the
driver thread, outside the audio render callback. The router rejects an entire new note when there is no
bounded lifetime slot. Its release is still owed: until every refused press on that channel/key
has been released, later presses there are refused too, so releases keep pairing oldest first and
a refused press's release never ends a newer note. CC120, CC123, a zone change and port panic clear
what is owed. Overflow recovery must still terminate already accepted notes.

The input worker conservatively retains up to 256 protocol occurrences per physical port;
it has no per-track completion feedback. At capacity it chokes the oldest released occurrence
before accepting a new one, never a held or pedal-held occurrence. Thus very long release
tails can be cut when the protocol budget fills. Runtime IDs never wrap or get reused.
The engine also bounds input occurrence and per-chain recipient tables; released entries
are reclaimed with an explicit choke, and all-held exhaustion rejects a new occurrence.
Device admission reserves final completion storage; the engine drains device completions at
render-span boundaries. Those completions free device capacity, not the input worker's
conservative protocol history. A live queue overflow sets an out-of-band atomic panic flag, one per queue, so a replaced queue
does not inherit it: the renderer discards the incomplete queue batch and terminates its existing
notes, closing in a running take the ones whose release was not recorded.
Per-chain recipient entries record delivery intent, not a promise of device admission. A
device-capacity rejection starts no voice and owes no completion; the bounded recipient
entry remains until the matching release/reclamation. On an ordinary-compatibility chain only
an occurrence's first termination becomes a key-wide off: reclaiming an already released entry
sends nothing there, so it cannot end a newer note on the same key. Recording likewise writes
one release per occurrence. There is no reverse admission channel
to the physical input in this slice. Timestamped expression floods coalesce only the same
ID/dimension; a new expression with no reserved control slot, or with no place left on the note timeline, is
dropped rather than being played before its note-on. Exhausting the note timeline with a note
lifecycle event invokes the same panic recovery.

FM Synth retains its existing 64-frame internal control quantum and fixed-point numerical
behavior. Event dispatch is sample-offset accurate, but audible FM changes take effect at
the next internal quantum (at most 63 frames later). Sampler applies pitch at the exact next
processing sample. Both keep the same result when the host changes its block partition.
Neutral pressure preserves the old patch level; per-note pressure adds up to 6 dB and also
feeds the FM pressure routing. Timbre shifts each enabled voice filter by up to two octaves
in either direction; with the patch filter off that mapping has no effect.

Pitch preserves runtime identity and the transposed key through release/choke, including
equal-pitch occurrences and later parameter changes. An edit that changes the MIDI effects in
front of the instrument ends each held identified note at the instrument, by its id, since its
later off would pass effects that never saw its on; an edit that keeps that route keeps the note
playing. Unmigrated MIDI effects, hosted plug-ins,
layered SDK graphs and instruments without the capability use ordinary-note compatibility
for the entire chain. Direct SDK voice-group graphs expose `VoiceInput.pitch` in semitones
and `VoiceInput.timbre` in 0..1 without changing the legacy normalized `bend` output. Graphs
with additional native consumers or transformed event routing do not advertise independent
expression until their admission and final retirement can be aggregated safely.

`midi.setMpe` takes a port name, `mode` (`off`, `automatic`, `manual`) and optional lower/upper
member counts (`lowerMembers`, `upperMembers`) and bend ranges (`lowerMemberBend`,
`lowerManagerBend`, `upperMemberBend`, `upperManagerBend`). It validates the whole configuration before
closing/reopening only that port, so old occurrences are choked first. Unchanged values do
not interrupt notes. `midi.inputs` reports saved configuration; automatic runtime zone changes
follow the controller and are not persisted as manual choices. At most 256 saved inputs are
retained. The MIDI settings' manual zone control shortens the other zone when required to
make room; command clients must supply a nonoverlapping configuration explicitly.

## Note curves and editing

Each saved note may own three optional curves. A point may carry a curvature, -1 to 1, for the
segment from it to the next point: 0 is straight; otherwise the segment's progress `t` (0 to 1
along its time) becomes `t^e` with `e = 2^(-3 · curvature · sign(rise))`, so a positive curvature
bulges toward higher values and a negative one toward lower values whichever way the segment goes.
A flat segment and the last point ignore it. Split, crop and stretch keep each point's curvature
(a cut segment keeps its shape's curvature, an approximation of the exact part); join keeps both
sides'. Points have finite beat offsets in
`0..=note.duration` and finite values in the runtime ranges. Offsets are strictly increasing;
editing replaces an existing equal-time point, while malformed files with duplicates are
rejected. Interpolation is linear; before the first point and after the last, hold its value.
An absent curve means the neutral value. Constant curves may have one point. Limits are 4,096
points per curve and 262,144 expression points per clip content. The file reader checks a list's
length in the parsed document before it stores the curve's points; a command checks before it
changes the note, and an edit that would pass either limit (a join, a duplicate) is refused whole.
An edit that reshapes a curve never adds points to it beyond the limit.
Curve values and note data participate in the same command validation and undo operation.
`note.setExpression {clip, note, kind, points}` replaces one curve of one note, one undo step:
`kind` is `pitch`, `pressure` or `timbre`, `points` is `[[offset, value], ...]` given strictly
increasing and within the note and the range (nothing is sorted or clamped), and `[]` removes the
curve; the result is `{note, kind, points}`, the last the number of points.

| Operation | Curve behavior |
|---|---|
| Move or start quantize | Keep relative offsets and values; the complete gesture moves with the note. |
| Transpose | Change base key; keep pitch offsets, pressure and timbre unchanged. |
| Resize right edge | Crop beyond the new duration, inserting the evaluated boundary value when needed; extending holds the endpoint. |
| Resize left edge | Crop/rebase offsets by the start change and insert the boundary value when a point was cropped; extending holds the old initial value. |
| Stretch, including duration quantize | Scale offsets by new duration / old duration; reject zero or nonfinite divisors. |
| Split at an interior time | Evaluate each curve at the split; both parts get that boundary value, as a point where the part has points on the other side of the cut (else its held endpoint already is that value), the right offsets are rebased. The first note keeps its editing ID, the second gets a new one. |
| Join adjacent notes | Require matching key/channel, touching endpoints, and equal evaluated boundary values for every curve; retain the first editing ID and concatenate rebased points. Reject a discontinuous or nonadjacent join rather than inserting a ramp into the earlier note, and a join whose curve would pass 4,096 points. |
| Duplicate / copy content | Copy values with the normal editing identity rules; playback allocates independent runtime occurrences. |
| Delete / mute | Delete curves with their note, or suppress the whole muted occurrence. |

Playback prepares curves off the audio thread and evaluates against beat time in the common
renderer. Release holds the note's terminal value unless an explicitly addressed live event
changes it. Tempo changes use the same beat-to-frame map as notes. Loops, seeks and transport
stops terminate old occurrences and start fresh ones; they never reuse a runtime ID. Curves
are stored inside the note, without runtime IDs: project file version 17 writes a note's
`expression` as `[offset, value]` lists per curve and refuses a file that breaks the rules above
([project file](suhina-format.md#versions)). Notation was not ready, so expression took its own
version (ADR 0039 › Amendment: expression versioned alone).

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

### Channel-MPE output

This is what is built of the modes above: channel-MPE compatibility, chosen automatically
(`crates/engine/src/chain/mpe.rs`).

- **When.** A track's chain plays as MPE when its first device is a plug-in instrument and a
  note it plays, in an arrangement clip or a launch clip, has a curve. There is no setting or
  command yet; the per-plug-in mode above (native, channel-MPE, ordinary MIDI) will be a
  command when native expression exists to choose from. A chain that plays as MPE sends every
  note on a member channel, a note without curves with neutral controls. Every other chain
  plays as before: every note on the first channel, no extra events. A MIDI effect before the
  instrument, an instrument inside a device group and built-in instruments get no MPE.
- **Setup.** Before the chain's first note, and again after a stop: the MPE Configuration
  Message on channel 1 (RPN 6: CC101 0, CC100 6, CC6 15, then the null RPN CC101/CC100 127),
  a Lower Zone of 15 member channels; then on each member channel 2 to 16 RPN 0 at 48
  semitones (CC101 0, CC100 0, CC6 48, CC38 0, null RPN) and the neutral controls: pitch bend
  8192, channel pressure 0, CC74 64. Loops and seeks do not send it again.
- **Initial controls, then note-on.** A note takes a member channel; the channel gets the
  curves' values where the note starts (where a seek or a start lands inside it, for a note
  started part-way), each only when it differs from what the channel last had, then the
  note-on on that channel. Its note-off goes on the same channel.
- **Values.** Pitch is 14-bit bend at ±48 semitones: 8192 + semitones / 48 × 8191, rounded
  and clamped to 0..16383 (a curve past ±48 stays at the end). Pressure is `round(p × 127)`.
  Timbre is CC74 with 0.5 at 64, 0 and 1 at 0 and 127, linear on either side, the mapping MPE
  input uses. The plug-in host turns the event values back into exactly these numbers.
- **Rate.** The graph prepares each curve off the audio thread: its value at the note's start,
  at every point, and every 1/256 beat between points, kept only where the MIDI value changes;
  nothing after the last point, as the curve holds. The changes are kept in beats from the
  note's start and turned into frames through the tempo map (an arrangement note, or a launch
  clip on the map) or the launcher's tempo, so a value at a point lands on the point's frame.
  A graph holds at most 1,048,576 changes; a note past that plays without its curves. The
  audio thread only reads the prepared lists; a graph or plan frees them off the audio thread.
- **Channels and stealing.** A note takes a free member channel, the one released longest ago
  first, which gives a release tail the most time. With all 15 sounding, the oldest note is
  released on its channel and the new note takes it (its controls reset to the new note's
  first), so a stolen note's tail is cut on plug-ins that keep one voice state per channel.
- **Edits, loops, seeks, stops.** A loop wrap, a seek and a launch release the sounding notes
  and start fresh ones on free channels. A stop releases them, sends CC123 on each member
  channel and sets the zone up again before the next note. After an edit while playing, an
  arrangement note that still sounds follows its curve in the new graph from where it is; a
  launched note holds its values until it ends.
- **Formats.** A CLAP and a VST2 plug-in get every member channel's notes, bend, pressure and
  CC74 as MIDI on that channel. A VST3 plug-in gets the notes on their channels, and each
  member channel's bend, pressure and CC74 as parameter changes through that channel's own
  `IMidiMapping` assignment, as for the first channel: a plug-in that plays MPE maps every
  channel's controllers to parameters of its own. The member channels are asked only of a
  plug-in that maps pitch bend on the first ([plug-in hosting](plugin-hosting.md)); a channel it
  maps nothing for moves nothing, never the first channel's parameter, through which one note's
  bend would bend every note.
- **Follow-ups.** Built-in instruments (FM Synth, Sampler) play the curves once they take
  identified notes (#1598); recording curves from an MPE controller; the per-plug-in mode
  command; native CLAP and VST3 note expression.

The stock instrument, VST3 export and CLAP export consume the same `device-api` event contract.
Imported plug-ins remain sandboxed. Both IPC directions bound event counts and validate values.

## MIDI 2.0

Runtime identity, curves and musical units do not depend on MIDI 1.0 channel allocation.
Native per-note controllers can map through a future UMP adapter without reducing supported
32-bit dimensions to seven bits. This is not an implementation of every UMP message or a
lossless archival representation of arbitrary controllers. The MIDI-CI MPE Profile is a
separate configuration protocol; it is not a synonym for native per-note MIDI 2.0 messages.

## Acceptance and implementation order

### Live implementation boundary (#926)

The MIDI input worker owns protocol state for each physical port: selected RPN/NRPN,
lower/upper zones, controller latches and physical key/pedal state. It resolves these into
the shared identified events before the engine's live queue. The queue retains the original
source channel and timestamp. The engine carries `NoteEvent::Instance` through its existing
sample-accurate split spans; it does not quantize expression to an audio block or reinterpret
native zero-velocity note-ons as MIDI bytes.

The first live implementation supports independent pitch, pressure and timbre in the FM
instrument and the shared sample runtime. Each owns bounded occurrence admission and final
retirement, including stolen voices and layered tails. A note-off never frees an identity
while it can still sound. Ordinary MIDI and unsupported-chain compatibility remain playable;
the MIDI settings describe which instruments support independent expression. An unsupported
device does not advertise the opt-in capability. MIDI effects must preserve identity for
one-to-one transformations; an effect without that contract selects the documented ordinary
compatibility path for the whole chain.

Input configuration is a session command and a persistent setting per port. Changing mode
or manual zones terminates the affected input's old occurrences before accepting new notes.
Automatic mode follows valid RPN 6 messages; manual mode retains the user's zone sizes and
bend ranges. No input configuration, expression state or runtime note identity is written
into saved project notes by this work; recording and curve editing remain #928 and #930.

Implementation is checked in this order: bounded protocol fixtures, identified FM/sample
voice fixtures, live routing/golden renders, then command/settings round trips and UI flows.
The draft remains incomplete until all #926 rows below pass, including recycled-channel
tails, per-zone sustain and queue/capacity recovery.

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
