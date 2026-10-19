---
description: How recording works: MIDI and audio takes into a clip slot or the arrangement, the count-in, where a played note or a recorded sound is placed (lined up with what was heard), the files audio takes are written to, and what happens when a disk fills or a device is lost
read_when: recording, record, take, record button, clip.record, transport.record, count-in, transport.setCountIn, record.state, MIDI recording, audio recording, Recorded folder, latency compensation of a take, overdub, loopback
type: spec
status: current
last_verified: 2026-10-16
covers:
  - crates/engine/src/record.rs
  - crates/engine/src/record_tests.rs
  - crates/engine/src/count_in.rs
  - crates/session/src/recording.rs
  - crates/session/src/record_writer.rs
  - crates/session/tests/recording.rs
  - crates/ui/src/session/record_controls.rs
  - crates/ui/tests/recording.rs
---

# Recording

Suhina records what armed tracks play ([ADR 0017](../decisions/0017-inputs-and-latency.md)):
the notes a MIDI controller plays on a track that plays notes, and the sound of the audio input
on a track that has one. A recording makes one clip per armed track, a **take**, either in a
clip slot of the Session view or in the Arrangement. Each take is one edit: one undo removes it.

This spec is written from how recording is commonly understood and from Suhina's own engine; it
names no other program's behaviour.

## What is recorded

- **Armed tracks only.** Arming (`track.setArm`) is what says a track listens to its inputs; a
  take is made on every armed track that heard something it can keep:
  - a track that plays notes records the notes of the MIDI inputs it hears
    (`track.setMidiInput`), as a MIDI clip;
  - a track with an audio input (`track.setAudioInput`) records the input's channel or pair,
    before the track's devices and mixer, as an audio clip. Monitoring (In, Auto, Off) changes
    only what is heard while recording, not what is recorded.
- **Notes as they were heard.** A note starts when its key went down and ends when the sound
  was let go: at the key's release, or, while the sustain pedal is down, when the pedal comes
  up. A note still held when the take ends is cut at the take's end. Notes are kept with their
  velocity; MIDI channel and pedal are not stored as such.
- **A MIDI take with no notes still makes its clip** (an empty clip of the take's length), so
  what was recorded is visible; an audio take always has sound, even if silent.

## Where it goes

Sampler's waveform Record/Stop controls use `device.recordSample`. The track must
be armed with an audio input selected and the project saved. This records directly
into the instrument source through the same recording writer, without making a
clip. Start and stop use the current arrangement beat rather than launch
quantization. Loading the finished take is one undo step; undo keeps the WAV file.

### Into a clip slot

`clip.record` on an **empty slot** of an armed track:

1. Recording starts at the next launch boundary (the launch quantisation, as a launch would). If
   the transport is stopped it starts, after the count-in when one is set, and the take starts
   where playback starts.
2. Launching the same slot again (`clip.launch`, a click on its record button), or
   `transport.record` with `on: false`, ends the take at the next launch boundary. The clip is
   made once the boundary has passed, as long as the beats recorded, looped, and it plays from
   the boundary on: what was just played repeats. Notes played between the press and the
   boundary are still part of the take. Suhina looks for takes that are done every frame in the
   window, and around every command over MCP, so the clip appears then; it plays as if it had
   started on the boundary.
3. Stopping the transport ends the take where the playhead stopped; the clip is made and does
   not play.

One take per track at a time: a second `clip.record` on the same track is refused while the
first runs. A slot that fills while recording (another edit) ends nothing: the take is put in
the slot it started in only while it is still empty, else in the first empty slot below it,
else in a new scene.

### Into the arrangement

`transport.record` with `on: true` records every armed track into the arrangement:

1. With the transport stopped, it starts after the count-in, and the take starts at the play
   start marker. With the transport running, the take starts at the playhead.
2. `transport.record` with `on: false` or `transport.stop` ends it at the playhead.
3. Each take becomes a clip on its track from the beat recording started to the beat it ended,
   replacing what it covers, like a clip pasted there. With the loop on, the notes of every pass
   are merged into the one clip (when the playhead ended before where it started, the clip
   reaches the last note, at least a bar); an audio take keeps everything from where it started,
   one pass after another, as one long clip.

While recording into the arrangement, launching a slot plays it as usual; it is the slot record
button that records into slots.

## The count-in

`transport.setCountIn` sets 0 (none, the default), 1, 2 or 4 bars. It applies when recording
starts the transport, and to nothing else. During the count-in the transport has not started:
the playhead stays at the play start marker, the metronome clicks the count-in's beats (higher
on the first beat of each bar) whether or not the metronome is on, at the metronome's level,
and `record.state` reports the beats left. When it ends, the transport starts on that frame.
`transport.stop` during the count-in cancels it and ends the takes with nothing in them; the
play start marker stays where it was. The count-in's bars are bars of the time signature at the
play start marker, at the tempo playing.

The count-in is a setting of the open project, like the launch quantisation: it is not saved in
the project, and it is not undone.

## Lined up with what was heard

A player plays along with what they hear, and what they hear is late: the engine renders ahead
of the speaker. A take is placed where what was played lines up with what was heard:

- **A note** is placed at the beat that was sounding from the speaker when the key went down:
  the frame the engine played it at, less the key-to-sound latency. That latency is the fixed
  delay notes are played after their driver timestamp (ADR 0017, decision 10) and the measured
  output latency from the engine to the speaker. Offline, with no device, both are 0.
- **A recorded sound** is placed at the beat that was sounding when it reached the microphone:
  the frame the engine read it at, less the output latency, the input device's latency and the
  frames that waited in the input ring. A loopback (the output played straight into the input)
  then records the project back exactly on its beats.

Notes before the take's start (a key a little early) are not kept, nor is sound before it.

## Audio files

Audio takes are written to `Recorded/` in the folder of the project file, one WAV file per take,
named after the track and the bar the take starts at (`Bass bar 5.wav`, then `Bass bar 5 2.wav`),
as 32-bit float at the engine's sample rate, mono for a one-channel input and stereo for a pair.
Audio recording therefore needs a saved project: `clip.record` and `transport.record` refuse
with that reason when an armed track would record audio into an unsaved project.

The render thread never writes a file: it copies the input into a lock-free ring made before
recording started, and a writer thread empties the ring into the file as it goes, so a take is
on disk before it ends. The file is finished when the take ends; the clip is made from the
file's frames.

- **A full disk, or a file that cannot be written**, ends that take: what was written so far is
  kept and becomes the clip when the take ends, and the reason is reported (a notice in the
  window's status line, `notices` in `record.state`).
- **A lost device** (the output fails or is opened again) ends every take the same way, keeping
  what was recorded. An input device that goes away while the output plays on records silence.
- **More input than the ring holds** (the writer thread could not keep up for seconds) is
  dropped by the render thread; the take goes on, with silence in its place so later sound stays
  on its beat.

## Commands

| Command | What it does |
|---|---|
| `device.recordSample` | `track`, `device`, `on`: records the armed track audio input into Sampler without a clip; starts at the current beat, or the play start marker while stopped |
| `clip.record` | `track`, `scene`: records into that empty slot of an armed track, from the next launch boundary |
| `transport.record` | `on`: true records every armed track into the arrangement; false ends every take (slot takes at the next launch boundary) |
| `transport.setCountIn` | `bars`: 0, 1, 2 or 4 |
| `record.state` | Whether the arrangement records, the takes running (track, slot or arrangement, audio or MIDI, start and end beat, notes so far, frames written, file), the count-in setting and the beats it has left, notices |

`clip.launch` on a slot that records ends its take; `transport.stop` ends every take. All of
them are transport commands: none is undone itself, and the clip each take makes is one undo
step.

## In the window

- An armed track's empty slots show a round record button at their left edge (**Record on
  Keys (scene 1)** to a screen reader); clicking it runs `clip.record`. A slot recording is
  filled with the record colour, says **Recording** and its button pulses; clicking it again
  (**End the recording on …**) ends the take with `clip.launch`.
- The control bar has a record button right of play and stop (**Record into the
  arrangement**, **Stop recording** while lit), and a **Count-in** menu right of the launch
  quantisation (Off, 1 bar, 2 bars, 4 bars). It has no default shortcut yet.

## Not yet

- Overdubbing into a clip that exists (recording adds notes to a playing clip).
- Punch in and out points, and keeping each loop pass of an audio take as its own take.
- Recording automation (a moved parameter written as a lane).
- Recording a track's output (resampling) rather than its input.
