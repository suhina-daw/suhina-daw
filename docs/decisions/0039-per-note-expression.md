---
description: Runtime note identity, allocation-free expression events, MPE routing and note-relative curves shared by native devices and plug-in adapters
read_when: MPE, note identity, per-note expression, note curves, MIDI 2.0, expressive instruments
type: decision
status: proposed
built: partly
last_verified: 2026-11-20
covers:
  - crates/device-api/src/lib.rs
  - crates/device-api/src/note_expression.rs
  - crates/device-api/src/note_expression/
  - crates/engine/src/live.rs
  - crates/midi/src/lib.rs
---

# 0039: Per-note expression

- **Date:** 2026-11-19
- **Issues:** [#924](https://github.com/suhina-daw/suhina-daw/issues/924),
  [#921](https://github.com/suhina-daw/suhina-daw/issues/921),
  [#1579](https://github.com/suhina-daw/suhina-daw/issues/1579)

## Context

A key number does not identify a sounding note: two sources can play the same key, a loop can
retrigger it before its tail ends, and an MPE member channel can be reused after key release.
The current `NoteEvent` and `LiveEvent` carry channel-wide performance controls but cannot
address these independent lifetimes. Instruments, MIDI effects and plug-in adapters need one
contract, including the new instrument's stock and exported forms.

[ADR 0025](0025-object-identity.md) gives a note an editing identity scoped to its content and
keeps it out of the saved file. A sounding occurrence is a different identity: one edited note
can produce many occurrences through loops, overlapping clips or a MIDI effect.

What is built: [MPE specification](../spec/mpe.md#status).

## Decision

1. **A sounding occurrence has a runtime identity.** `device-api::NoteInstanceId` is an opaque
   pair of nonzero `u64` origin and serial numbers. Graph preparation assigns distinct origins
   to event producers; each producer advances its own serial with checked arithmetic. No
   allocation, shared atomic, key hash or wall clock is involved. Exhaustion rejects new notes
   until a newly prepared origin is installed; reset never rewinds a live allocator. Origins
   are unique across simultaneously connected graphs, including retiring graphs. The pair is
   never truncated to a plug-in's smaller ID. A note retains its identity until every voice it
   produced has finished, even after its note-off. Voice stealing sends a targeted choke and
   retires that occurrence. A new occurrence always gets a new identity.

2. **The existing event stream carries identified events.** Add
   `NoteEvent::Instance { id, event: NoteInstanceEvent }`, with `On`, `Off`, `Expression` and
   `Choke` payloads. On carries key, velocity and a complete initial `NoteExpressionState`;
   off carries key and release velocity, choke carries key. Keys remain metadata for legacy
   adaptation, never an identity lookup when an ID is present. Expression has a kind and
   `f64` value: pitch offset in semitones, pressure and timbre in 0..=1. Neutral values are
   0, 0 and 0.5. Nonfinite values are rejected; pitch is bounded to -256..=256 semitones.
   Velocity and key validation remain mandatory. These are musical values, not MIDI bytes
   or a physical model's private controls. Pan, gain and arbitrary modulation are separate
   future extensions, not aliases for these dimensions. All payloads are fixed-size `Copy`
   data. The initial state is atomic with note-on so queue exhaustion cannot partly initialize
   a new voice.

   Native `On` with velocity zero remains a new occurrence with its initial state, as in
   CLAP. Finite velocities are bounded to 0..=1 without changing the event kind. MIDI 1.0's
   zero-velocity note-on encoding is converted to `Off` only by the MIDI protocol adapter,
   before entering this common contract; the existing MIDI byte parser already does this.

   The same payload first enters opted-in devices through `Device::note_instances`, a
   `NoteInstanceDevice` capability with event input and bounded finished-note output. This
   capability also supports exported cores before the engine queue migrates. The eventual
   `NoteEvent::Instance` wrapper dispatches to that endpoint; it does not add another payload
   definition or another renderer. An absent capability is an explicit unsupported result.
   The legacy queue continues unchanged until its effects and consumers are migrated together.
   Event admission returns a result: capacity failure starts no occurrence and owes no
   retirement notification. Adapters commit a newly reserved ID mapping only after admission;
   a duplicate-ID rejection never removes the already sounding occurrence's mapping.

3. **Frame timing remains the host's responsibility.** `Device::event` still acts at the
   start of the next processing span. Hosts split spans at event and automation offsets,
   refresh context before frame-zero events, and preserve arrival order at equal offsets.
   They never quantize expression to the block boundary or add a second timestamp inside
   the device event. A zero-length span consumes ordered events without rendering samples.
   Live input queues retain source and input timestamp beside the event. Playback evaluates
   prepared curves through this same event/render path.

4. **Compatibility is explicit at the dispatch boundary.** Instruments and MIDI effects
   advertise support for identified notes. Until a whole chain supports the contract, the
   host selects the existing ordinary-note path and reports that per-note expression is
   unavailable for that chain; it must not silently send an unhandled enum variant. Ordinary
   channel events remain available. A declared legacy adapter may use the carried key for
   on/off and ignore per-note expression, but cannot claim MPE support or independently choke
   overlapping same-key notes. Tests cover each dispatch path. MIDI effects that only
   transpose or delay retain identity and transform the carried key consistently; filtering
   removes the complete lifetime. One-to-many effects allocate fresh child identities in
   their assigned origin, retain a bounded parent-to-children table and fan out expression,
   releases and chokes. Capacity failure drops a complete new lifetime, never its eventual
   release. An effect never manufactures an ID from pitch or list position.

5. **MIDI channel lifetimes end before voice lifetimes.** A bounded router associates input
   port/channel/key occurrences with runtime IDs, tracks MPE zones and controls, and emits
   already resolved expression. Member expression changes only active keys; physical note-off
   detaches the occurrence from member controls even when sustain holds it. Its captured
   member values continue through sustain and release. Manager pitch remains applicable to
   sounding tails. Release, pedal release, final voice retirement and immediate choke are
   distinct operations. A native device reports finished occurrence IDs through a bounded
   caller-owned output; adapters use explicit host note-end feedback where available and
   bounded retirement/stealing where it is absent. In neither case may a stale table entry
   acquire another note's identity. The detailed routing policies are in
   [the input specification](../spec/mpe.md#input-and-zones).

6. **Expression belongs to a saved note, not to its runtime ID.** The model adds optional
   pitch, pressure and timbre curves with finite beat offsets relative to note start and
   finite values in the runtime units. A missing curve means its neutral default. Curves
   use piecewise-linear interpolation and hold endpoints. Each curve has at most 4,096
   points; all curves in one clip content have at most 262,144 points. A file's lists are
   counted before their points are stored, and a command is checked before it changes a note;
   an edit never leaves a curve or a content past a limit. The prepared renderer owns
   immutable bounded curve data; an audio callback does not clone or free it. Editing identities retain ADR 0025's
   rules. [Curve operations](../spec/mpe.md#note-curves-and-editing) define move, resize,
   stretch, split, join, quantize and transpose, including boundary interpolation and undo.
   Adding owned curves means auditing existing `Note: Copy` assumptions rather than hiding
   mutable expression in an untracked global side table.

7. **Expression and notation each take their own file version.** Expression is version 17:
   a note's optional `expression`, missing fields read as defaults, with old-file tests.
   Notation takes the next version when it lands; neither holds a partially valid writer on
   an obsolete version. Runtime IDs are never serialized. Source MIDI channels may remain as
   import/export hints, not a key for stored expression.
   *Amended 2026-11-27, see Amendment: expression versioned alone.*

8. **Host protocols map at their boundaries.** CLAP note tuples and VST3 note IDs map through
   preallocated tables to the same `NoteInstanceId`. CLAP wildcard addressing is expanded to
   matching occurrences before entering the core. VST3 expression normalization uses the
   plug-in's declared type information; normalized tuning is not assumed to be semitones.
   Native expression, channel-MPE compatibility and ordinary MIDI are explicit per-plug-in
   modes. MPE output owns a bounded member-channel allocator and a documented overflow
   policy, with setup controls before note-on and releases before channel reassignment.
   Hosting remains out of process; IPC validates counts, values and IDs before use. Exported
   built-ins use the same event contract and rendering core, not a second note engine.

9. **The core is neutral between MIDI 1.0 and MIDI 2.0.** Neither zones nor 7/14-bit values
   are part of runtime identity or expression. An eventual MIDI 2.0 adapter can preserve the
   precision of supported 32-bit controller dimensions in `f64`, but supporting three
   dimensions is not a claim to preserve every MIDI 2.0 registered or assignable controller.
   Unknown protocol data must not be silently advertised as lossless. Native per-note MIDI
   2.0 messages and the MIDI-CI MPE Profile are distinct adapters; the latter still uses
   channel groups and has different configuration rules from MPE 1.1.

10. **Capacity and lifecycle failures are deterministic.** Tables, event queues and output
    lists are allocated at preparation and have declared caps. Full capacity rejects a new
    note or steals a declared oldest eligible occurrence with an explicit choke; it never
    drops an already accepted note's termination. A producer queue reserves recovery state
    for overflow so the consumer can release/choke the affected source even if no more events
    fit. Unknown IDs and malformed expression are ignored without disturbing other voices.
    Port loss, graph replacement, transport stop, sustain and panic each have tests. The
    [acceptance matrix](../spec/mpe.md#acceptance-and-implementation-order) includes exact
    split-block equality, allocation checks and deterministic offline audio comparisons.

## Amendment: expression versioned alone (2026-11-27)

Decision 7 asked expression and notation to share one version bump. When #928 changed the
writer, no notation work had landed (no model field, no file field, no decision), and the
project file rule allows no field without a rise: an older Suhina skips a key it does not know
and drops it on its next save ([ADR 0010](0010-project-files.md) › Amendment: format versions).
So expression takes version 17 on its own: a note's optional `expression` with `pitch`,
`pressure` and `timbre` lists of `[offset, value]` points, missing in earlier files
([project file](../spec/suhina-format.md#versions)). Notation takes the next version when it lands.

## Consequences

The common core can play repeated pitches and recycled channels without moving a release tail
with a new note. It does not need to know whether an event came from a clip, MIDI hardware or
a plug-in host. Explicit compatibility keeps old devices playable during migration.

The cost is bounded lifetime bookkeeping in the engine, MIDI effects and host bridges. A wider
runtime ID costs more queue memory than a channel number; this avoids collision-prone arithmetic
and permits independently prepared producers. Optional curves also require a deliberate audit
of note copying and edit commands. These are implementation work, not solved by adding an enum.

## Alternatives

- Use MIDI channel or key as identity: conflates recycled channels, repeated pitches and tails.
- Reuse `model::NoteId`: fails when one edited note has concurrent sounding occurrences and
  would confuse editing persistence with playback state.
- Persist runtime IDs: adds no useful saved reference and contradicts ADR 0025.
- Add a second timestamped instrument-only stream: splits native, exported and engine behavior.
- Store raw MPE bytes as expression: couples editing to channel allocation and loses native
  host precision and non-MIDI input semantics.
- Silently translate every identified event to ordinary notes: appears to work while losing
  independent release and expression, so compatibility must be visible and tested.

## Sources

- MMA/AMEI, **M1-100-UM MIDI Polyphonic Expression v1.1**, 2022-04-14, sections 1.5 and 2,
  appendices B-D. Public specification; reference copy supplied in the research library.
  [MIDI Association MPE resource](https://midi.org/tag/midi-polyphonic-expression).
- [CLAP event definitions](https://github.com/free-audio/clap/blob/main/include/clap/events.h):
  note address tuples, valid zero-velocity note-on, expression units and note-end notification.
- [MMA/AMEI UMP and MIDI 2.0 Protocol v1.0](https://amei.or.jp/midistandardcommittee/MIDI2.0/MIDI2.0-DOCS/M2-104-UM_v1-0_UMP_and_MIDI_2-0_Protocol_Specification.pdf),
  appendix D.3.1: MIDI 1.0 zero-velocity note-on is converted to note-off at the protocol boundary.
- [VST3 note expression](https://steinbergmedia.github.io/vst3_doc/vstinterfaces/structSteinberg_1_1Vst_1_1NoteExpressionValueEvent.html):
  note IDs, ordering and normalized expression values.
- [MMA/AMEI MPE Profile v2.0.3](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-120-UM_v2-0-3_MIDI_Polyphonic_Expression_Profile.pdf):
  the distinction between the channel-based profile and native MIDI 2.0 per-note controllers.
