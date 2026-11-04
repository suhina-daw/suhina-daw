---
description: How the engine runs devices on tracks - chains, instruments and their notes, MIDI effects and their room for events, group buses, sends into returns, device groups, the main channel's chain, latency across them, and what a graph swap keeps
read_when: device chain, chain, instrument notes, note events, MIDI effect, process_events, group bus, bus, sends into returns, device group in the engine, main channel chain, latency compensation, graph swap keeps devices, chain order, Slot::Group
type: spec
status: current
last_verified: 2026-11-02
covers:
  - crates/engine/src/chain.rs
  - crates/engine/src/buses.rs
  - crates/engine/src/sends.rs
  - crates/engine/src/group.rs
  - crates/device-registry/**
---

# Device chains in the engine

How the engine runs built-in devices, `.sd` devices and plug-ins on tracks. The decisions are
[ADR 0004](../decisions/0004-built-in-devices.md) (8 to 14); this is how they behave in detail.
Own design: nothing here is observed from another program.

## Chains on tracks

`crates/device-registry` makes a device from its `DeviceKind::Builtin` id. When the renderer's
graph is built, a track whose channel holds enabled built-in effects and instruments gets a chain: the
devices made, prepared, given the project's parameter values (`Device::params`, by
parameter id; missing ones at their defaults) and reset. Everything the track plays in a
block (sine voices, audio clips, stretched and launched clips) goes into the chain's buffer
at a centred unity gain instead of the output; the devices run over it in order, and then
the track's pan and volume (and those of the channels it feeds) apply, so the fader is
after the devices. A track whose cue is on also has a chain (a passthrough one without
devices), which adds its output to the cue bus too ([cue spec](cue-output.md)).
A muted track, or one a solo leaves out, keeps its chain at a gain of
zero, so its devices (and plug-ins, which would otherwise be let go of) keep running and
sound again at once. Every device gets the block's own transport (tempo, beat, playing),
also when its chain runs in the stages plug-ins share ([ADR 0006](../decisions/0006-plug-in-hosting.md) › 4). When the
first device is an instrument, the track's notes (arrangement
and launched) go to it as note events at their frames, the chain running in pieces split at
each event, and note-offs follow from each note's end; stopping the transport or a column
releases them. A track without devices is mixed straight into the output exactly as
before. Unknown ids, plug-ins that are not installed and other DAWs' devices pass the sound
through; installed plug-ins run in the same chain ([ADR 0006](../decisions/0006-plug-in-hosting.md)), so
the devices of a chain run one after another over a whole block, each split at the events.
Chains hold at most 1024 frames, so the renderer splits longer blocks. A graph swap (ADR 0005)
hands a chain's running devices to the new graph when the same devices sit on the same
track, so held notes and tails carry on through an edit and the new parameter values
apply; otherwise the new chain starts fresh and the old devices are freed with the old
graph, off the audio thread. A render with chains is 2 s longer, for releases and tails.
The main channel runs a chain of its own too, last, over the mix that reaches the main
output, when it holds devices or its volume, pan or mute follows a lane.

## Instruments and their notes

`DeviceInfo::role` says whether a device is an `Effect` or an `Instrument`; an instrument writes its output over the buffer. `Device::event` hands it a
`NoteEvent` (key on and off with velocity 0..1, a MIDI controller 0..1, pitch bend -1..1,
pressure, all notes off) at the frame where the next `process` starts: the host splits a
block where an event falls, exactly as for automation, so one mechanism times both and the
trait needs no event list. Values are plain and in range from a well-behaved host, but a
device accepts anything without misbehaving. Effects ignore events. Whole-number parameters
(a level from 0 to 99, an algorithm number) get their own kind, `ParamKind::Integer`, so they
never take a fraction. The contract tests add, for instruments, a fixed performance (notes
that start inside blocks, the sustain pedal, bend, the wheel, pressure) in every block size,
`reset` while notes sound, hostile events (keys over 127, values that are not numbers, far
more notes than voices), silence after all notes are released, and every sample rate.

An instrument that works in fixed internal blocks (the FM synth renders 64 frames at a time)
applies an event at its next internal block boundary on a grid counted from `reset`, which
keeps the output independent of the host's block sizes at the cost of up to one internal
block of timing.

## Automation inside a chain

A built-in device's lanes (`Device::automation`, ADR 0001) set its parameters at their points and at
every 64th frame of the arrangement where the value moved, at a block's first frame only when
the transport started or moved; the chain splits the device's block there, with note events,
a parameter change before a note at one frame. So the sound does not depend on the block size,
as ADR 0004 › 5 asks, and the device smooths each step. A track whose mixer has lanes runs through a
chain, one without devices when it has none, whose gains move frame by frame after its devices.
[The automation spec](automation.md) has the details.

## Group buses

When a group track's channel holds devices, the tracks routed into it (at any depth) no longer reach the output on their own:
each gets a chain, one without devices when it has none, whose output goes into the group's
chain buffer at the track's pan and the volumes from its channel up to the group; the
group's devices run over the sum, and the volumes from the group to the master apply after
them (a group's pan does not apply, as before). Chains are ordered so a bus comes after
every chain that feeds it, and a block runs them in that order. Latency compensation follows
every path: a bus's input waits for the slowest chain feeding it, and the output for the
slowest path, so a track with a lookahead device inside a group with another still starts
with the rest (`crates/engine/src/buses.rs`). A device reports its delay with `latency()`
(the overdrive and vinyl distortion, 31 frames; the limiter, its 1.5 ms lookahead), and every
other track is delayed to match ([ADR 0006](../decisions/0006-plug-in-hosting.md) › Consequences). A group without devices stays a gain on the
tracks routed through it, with no buffer of its own. Groups nest (up to 16 levels, ADR
0001): a bus inside a bus runs before it, and a level without devices between two buses is
a gain on the way; offline renders through three nested buses check the sum at every level,
solo and mute through the levels and the latency of every path (`crates/engine/src/buses_tests.rs`).

## Sends into returns

Behaviour as the user sees it: [sends and returns](sends-and-returns.md). Every track with a heard send gets a
chain (one without devices when it has none), and every return fed by a send gets one too,
which is a bus like a group's. After its devices, before its own delay line and gains, a
chain adds its buffer to each return's input at the send's gains: the level alone before
the fader, the level times the channel's volume and equal-power pan after it. The chains are
put in a topological order over both kinds of edge (output into a bus, send into a return),
so one pass per block still runs every chain after everything that plays into it; a send
that would close a loop through outputs and earlier sends is left out when the graph is
built, so the order always exists. Each send has a delay line of its own: a return's input
waits for the slowest chain that feeds it, through its output or a send, and the output for
the slowest path, as for group buses. A graph swap hands a send's delay line over when the chain sends
to the same return with the same delay (`crates/engine/src/sends.rs`).

## Device groups

Behaviour as the user sees it: [device groups](device-groups.md). `Slot::Group` holds its branches, each a
list of slots with a buffer of its own, all made when the graph is built
(`crates/engine/src/group.rs`). Every branch gets a copy of the group's input and runs its
slots one after another over the same span the chain runs, with the same note events, so an
instrument in every branch plays every note at its frame. The branches are summed at their
volume and equal-power pan, centred at unity like a channel's, then crossfaded linearly with
the dry input by `mix` and scaled by `gain`; branch gains, mix and gain move to a new value
over 20 ms. A muted branch, or one left out by a solo in its group, still runs its devices
and adds nothing, as a muted track does (*Chains on tracks*). A group whose branches start with an instrument
counts as the chain's instrument and has no dry signal. A graph swap keeps a group's running
devices when the identity it compares, its branches' device ids in order, is the same, and
takes the new branch gains and parameter values, so an edit to one value never restarts a
sound. A switched-off group is not in the chain, like a switched-off device. Latency is
compensated inside the group as across tracks (*Group buses*): a branch's latency is the sum of its
devices', the group's the largest of its branches' (cut at 2 s), every branch is delayed by
what it lacks of that and the dry signal by all of it, with delay lines made with the graph,
and the group reports its latency to its chain like any device.

## MIDI effects

The first one is [the pitch effect](pitch.md). `DeviceInfo::role` has a third value, `MidiEffect`. The host never asks such a device to process a buffer; it calls
`Device::process_events` with the events of a span (at frames from the span's start) and an
output, and the device writes what it sends on, in frame order. Nothing on the audio thread
allocates, so the room is fixed: a chain holds at most 512 events a block for its MIDI
effects (it runs its devices up to the frame it has reached whenever 256 notes wait), and
each span gets the room left of those, at least as many events as it hands over, twice as
many while the block has space. A device with more to send than fits leaves out note-ons
first: it keeps room for the note-offs and all-notes-off still to come, and a note-on it
leaves out is not counted as sounding, so its note-off goes too. A key played again with no
room to end its note keeps the old note until that key's note-off. So no note a MIDI effect
started is left sounding, however crowded the block; the contract tests run with room for
no more events than the device gets.
The chain splits the span where one of the device's parameters follows a lane, a change
coming before the events at its frame, as for instruments; what one MIDI effect sends is what
the next device gets, so they run in order before the instrument. Notes are paired by key:
a MIDI effect that changes a note-on's key remembers it, so the note-off ends the same key
whatever its settings are by then. A clip's scale reaches the devices as an event,
`NoteEvent::Scale(Option<MusicalScale>)`, which the chain sends before the first note of a
clip whose scale differs from the last one sent; instruments and plug-ins ignore it.
`MusicalScale` is the root and the set of pitch classes, with the steps along it a
scale-aware device needs. Three units come with it: semitones, scale degrees and a MIDI key.
The contract tests add, for MIDI effects, the same events in spans of every size at every
parameter's ends, `reset` while notes are held, hostile events, too little room to write, and
that no key still sounds after every key played is released, after all-notes-off too.
`device.add` places a MIDI effect among the MIDI effects that lead the chain, only on a MIDI
track; an instrument goes right after them (built-in, `.sd` or plug-in). A chain whose MIDI
effects have no instrument after them plays the notes they change as the sine tones a MIDI
track without an instrument plays (an instrument of the engine's own, after the MIDI
effects and before the audio effects), so adding a MIDI effect never silences a track and
never passes its notes by; notes from a MIDI input reach those tones too. MIDI effects run in
the same chain code offline and in real time (golden rule 4): `render_offline`, the player
and the launcher all start notes through it, so the offline renders in
`crates/engine/src/midi_effect_tests.rs` are the oracle for playback as well.

## The main channel's chain

The main channel runs a chain of its own when it holds devices or its volume, pan or mute
follows a lane (`buses::main_runs_chain`). It is built for `TrackId::MAIN`, the id commands, the
engine and the views name the main channel by (it has no track), and comes last among the
graph's chains: what reaches the main output (sources without a chain, and every chain that
plays into the main output after its gains) is its input, its devices run over that mix, and its
volume, mute and lanes apply after them, so a limiter on Main is fed the same level whatever the
main fader says. The main volume then no longer scales each track's gains. Its plug-ins are hosted,
carried over at a swap and taken back like any chain's. Its latency delays the whole output
equally, so it needs no compensation of its own; a node render (Stems) stops below it. The
metronome's click is mixed in before it, as it is into the main output. Without devices or lanes
nothing changes: the main volume scales every source as before.

## Devices added by command

Devices are edited through commands: `device.list`, `track.devices`, `device.add` (an
instrument always first, one per track, and only on a track that plays notes),
`device.remove`, `device.setEnabled` and `device.setParam` (values stored limited to the
parameter's range). `device.add {preset, voice?}` adds a device with a preset's values:
`device-registry::preset` reads DX7 voice data for the FM synth and gives every parameter's
value, which the project stores like any other (stock presets for the other devices:
[ADR 0014](../decisions/0014-stock-presets.md), their loader is not built yet).

The native sampler reuses the SDK's sample runtime and theme-aware UI widgets.
`Device::load_sample` installs immutable decoded audio off the callback; graph
replacement owns its disposal. This is a native instrument, not an `.sd` node.
