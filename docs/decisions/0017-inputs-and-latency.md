---
description: How played input reaches the engine (MIDI controllers first, audio inputs later), what arming a track is, and what a key press costs in latency today and how that is made smaller
read_when: MIDI input, MIDI controller, keyboard, arm, armed track, track.setArm, midi.inputs, live notes, sustain pedal, input latency, monitoring, recording, low latency, midir
type: decision
status: current
last_verified: 2026-10-24
covers:
  - crates/engine/src/record.rs
  - crates/engine/src/live.rs
  - crates/engine/src/live_tests.rs
  - crates/session/src/midi.rs
  - crates/midi/src/lib.rs
  - crates/output/src/lib.rs
  - crates/output/src/input.rs
  - crates/engine/src/input.rs
  - crates/engine/src/input_tests.rs
---

# 0017: Inputs, arming and live latency

- **Status:** accepted (MIDI input; the shallower queue while armed, #265; a MIDI input per track, #278; audio input and monitoring, #267; notes placed by the driver's timestamps, #269; MIDI and audio recording with a count-in, #266 and #268; controllers, pitch bend and aftertouch reaching the instrument, #812)
- **Date:** 2026-10-14

## Context

Until now Suhina had no inputs: it could play a project, but nobody could play an instrument
into it, monitor a guitar through its effects, or record. That is the first tier's third item in
[ADR 0016](0016-scope.md), and the first place the engine answers to a device rather than
to its own clock.

Played input is where latency is felt. A clip launch waits for its quantisation anyway; a key
pressed on a keyboard must sound within a few milliseconds, or the instrument feels late. The
output was built for playback: the render thread runs `Renderer::process` ahead of the device
into a lock-free ring and keeps two blocks queued (`docs/architecture.md` › Threads), so a frame
rendered now is heard about *two blocks plus the device buffer* later. Plug-ins add a round trip
per block, within a deadline ([ADR 0006](0006-plug-in-hosting.md)).

AGENTS.md rule 3 rules out allocating, locking or waiting on the audio thread, rule 6 asks that
arming and choosing inputs be commands, and rule 4 that offline renders keep testing the same
engine code.

## Decision

1. **MIDI first, through `midir`.** A new L3 crate, `crates/midi` (`suhina-midi`), lists the
   system's MIDI input ports and opens them with `midir` (MIT; the ALSA sequencer on Linux, WinMM
   on Windows, CoreMIDI on macOS). It is the only code that opens a MIDI device; the session sees
   it as a `MidiSystem` (`crates/session/src/midi.rs`), as it sees the audio output as an
   `AudioSystem`. It reads note on, note off (a note on of velocity 0 too), the sustain pedal
   (controller 64), all-notes-off (controllers 120 and 123), every other control change, pitch
   bend and channel pressure (aftertouch), on every channel (amendment, #812); program change,
   polyphonic key pressure, system exclusive and other system messages are ignored, as are short
   or malformed messages.
   *Amended 2026-10-23, see Amendment: controllers, pitch bend and aftertouch.*
2. **Live notes reach the engine over their own queue.** The renderer has a live-note queue
   (`rtrb`, 1024 events) beside the controller's command queue (`crates/engine/src/live.rs`).
   Its sending end goes to the MIDI driver's callback thread, which may lock and allocate: it is
   not the audio thread. The session's `LiveInput` holds the sending end of the engine that plays
   now; `Projects::poll_plugins` (every frame in the window, around every command over MCP) hands
   it the queue of a new player (another tab, a new output). The render thread takes the queue's
   events at the start of each block, after the controller's commands, and plays them at the
   block's first frame through `Chain::note_on` and `Chain::release_key`, the way the preview
   voice plays its notes ([ADR 0011](0011-preview-and-added-audio.md)). A controller, pitch bend
   or channel pressure message goes to the same monitored tracks' instruments the same way, through
   `Chain::live_control`, as a `NoteEvent` the device already takes (#812). A track that stops
   hearing a source (disarmed, given another input, or the port closed) has the pitch bend and
   pressure that source moved set back to the centre and to none, so its instrument is not left
   bent. A stamped control waits for its frame as a note does, each value at its own frame so a
   wheel moves smoothly, but controls take at most half of the waiting places: past that, a new
   value takes the place of the one before it that moves the same control, at that one's frame,
   so a stream never leaves a note-off playing ahead of its note-on. Nothing on the render
   thread allocates, locks or frees. The tracks that hear the live notes are the monitored ones
   (decision 3).
   *Amended 2026-10-23, see Amendment: controllers, pitch bend and aftertouch, and Amendment: MIDI
   monitoring.*
3. **Arming is playing state, not an edit.** A track's instrument plays the live notes, whether
   the transport runs or not, as its MIDI monitoring says (decision 8): **Auto**, the default,
   while the track is armed; **In** always, armed or not; **Off** never, though an armed track
   still records. Clip playback is never held back by it. Which tracks are armed is kept by the `Session` beside the
   project, not in it: `track.setArm` is a transport command, is not undone and is not saved
   (a project opens with nothing armed). It reaches the engine as `LaunchCommand::SetArmed`,
   without a new graph, and the renderer keeps the armed tracks (up to 64) across graph swaps; a
   new player is told the armed tracks when it is attached. Arming is exclusive when asked
   (`exclusive: true`, what a plain click in the window does; Ctrl adds a track), like solo.
   Disarming a track lets go of the keys it holds. A track armed while a key is down hears the
   next key, not that one.
   *Auto-arm (#834):* with Settings › MIDI › Auto-arm the selected track on (off by default, a
   setting of the app, not the project), selecting a track (the one the detail area shows, in
   either view; a clip slot, empty or not, selects its track) arms it through `track.setArm`, and disarms the track auto-arm armed before. A
   track armed by hand is never disarmed by it, and selecting one already armed leaves it as it
   is. Only a track that plays notes or audio through a regular channel is armed: a return,
   the main track or a group leaves the arm where it is. An audio track is armed as its arm
   button arms it: one with no audio input gets the device's first input (#682). While a take records, the selection
   does not move the arm (a take asked to end still records until it is a clip: a slot's, to its
   launch boundary); once the take ends, the track selected then is armed. Turning the
   setting off leaves the arm as it is. The window remembers which track it armed, per project.
   *Amended 2026-10-23, see Amendment: MIDI monitoring, and Amendment: Auto-arm.*
4. **Every port is on until turned off.** `midi.inputs` lists the ports, `midi.setInput` turns
   one on or off; the window keeps the ports turned off in its settings. The ports are looked at
   again every two seconds (every thirty when they cannot be listed at all), so a controller
   plugged in is heard without a restart, and one taken out is closed. Closing a port lets go of
   its keys.
5. **What a key press costs**, from the driver's callback to the speaker: the fixed delay of
   decision 10 (before it, up to one block until the render thread started its next one), what
   is queued ahead in the ring, and the device's buffer and its own delay. A player sets the buffer small (64 to 256 frames, with ASIO or
   WASAPI exclusive mode on Windows); the default 1024 frames is too slow to play.
6. **While a track is armed, the output keeps one block queued instead of two** (amendment,
   [#265](https://github.com/suhina-daw/suhina-daw/issues/265)). The render thread reads
   `Renderer::live_armed` (a track is armed or monitors MIDI with In) before each block and aims for one block or one device buffer,
   whichever is more, instead of two blocks or a device buffer and a block. It is automatic, not
   a setting: an armed track is the one sign that someone may be playing, and playback alone
   keeps the safer depth. Arming lets the device drain the queue to the new depth and disarming
   lets the thread fill it again, so neither is heard. The cost is time to render: the thread
   has one device buffer's time to fill the next block instead of two, so a heavy project or a
   slow plug-in shows as underruns (Settings › Audio, `audio.info`), and the answer is a larger
   buffer. At 48 kHz with 128-frame blocks a key now waits at most about 5.3 ms plus the device
   (8 ms before); at 256 frames, 10.7 ms (16 ms).
7. **The latency is reported.** `audio.info` gives `live` (a track is armed) and
   `live_latency_ms`, what a key takes to be heard: the measured engine-to-speaker latency and
   the fixed delay of decision 10 (one block before it). Settings › Audio shows it as *Key to sound*. The engine's own part is tested
   offline: a live note sounds from the first frames of the next block, at any block size; the
   output's queue depth is tested on the real render thread.
8. **Each track chooses what it hears** (amendment,
   [#278](https://github.com/suhina-daw/suhina-daw/issues/278)), so two players, or a keyboard
   and a pad controller, each play their own armed track. A track's `midi_input` is part of the
   project (model, `.suhina`, undone like any edit): a port (every port, the default; one by
   name; or none), a channel (every one, or 1 to 16) and its monitoring (Auto, In or Off, decision
   3). `track.setMidiInput` sets it; the
   track's menu in both views and, at 90 px, a dropdown in the Arrangement header show it. Each
   live note carries its source, a port and a channel: the MIDI layer tags it with a hash of the
   port's name (`port_id`, FNV-1a), which the session computes the same way from the track's
   input, so no table of ports is shared between threads. `SetArmed` carries the track's input
   as a `LiveFilter`; the session sends it again after every edit and undo, so a changed input
   is heard at once. The keys held and the sustain pedal are followed per source (up to 16 at
   once, silent ones reused), so one controller's pedal does not hold another's notes. A track
   that stops hearing a source lets go of that source's keys. A Live Set's track input routing
   is not read yet; imported tracks hear every port.
   *Amended 2026-10-23, see Amendment: MIDI monitoring.*
9. **Audio input** (amendment, [#267](https://github.com/suhina-daw/suhina-daw/issues/267)).
   - *The device.* An input device is chosen beside the output (`audio.setOutput`'s `input`,
     Settings › Audio › Input): off by default, so nothing opens a microphone unasked; the host's
     default input; or one by name. It is opened on the output's host at the output's sample rate
     (the engine takes no resampled input: a device that cannot record at that rate says so in a
     notice, and the output plays on without input), by `crates/output/src/input.rs`, beside the
     output on the audio owner's thread. Its callback converts every channel, up to 64, to `f32`
     and writes whole frames into a lock-free ring; frames that find it full are dropped.
   - *The engine.* `Renderer::set_audio_input` gives the renderer the ring's reading end. At the
     start of each block the render thread takes one block of frames into a scratch buffer made
     up front (`crates/engine/src/input.rs`). The input device runs on its own clock, so when more
     than four blocks wait, the oldest are dropped down to two, and a block that finds too little
     is filled with silence and counted (`input_underruns`).
   - *The track.* A track's `audio_input` is part of the project: no input (the default), one
     channel (heard in the middle, then panned by the track) or a neighbouring pair (left and
     right), counted from 1; and its monitoring: **Auto** (heard while the track is armed, the
     default), **In** (always) or **Off** (never; for recording). `track.setAudioInput` sets
     it; the track's menu and, at 90 px, the Arrangement header show it. Heard input goes the way
     an audio clip's sound goes: into the track's chain, or with the track's gains to the output,
     so its devices, plug-ins, mix, solo and latency compensation apply. Arm appears on tracks
     that play audio too, and the same arm (and Ctrl to add tracks) arms many tracks at once: a
     band on separate tracks, each from its own input, is the acceptance test
     (`input_tests.rs`, thirteen inputs onto tracks 1, 2 and 10 to 20).
   - *Latency.* What a microphone costs to be heard is the input device's buffer, up to a block
     waiting in the ring, and then the same path as a key on a MIDI controller (decision 5); an
     armed track keeps the shallower queue (decision 6). `audio.info` reports the input device,
     its channels and its underruns.

10. **Notes are placed by when they were played** (amendment,
    [#269](https://github.com/suhina-daw/suhina-daw/issues/269)). A note on the first frame of the
    next block jitters by up to a block (1.3 ms at 64 frames, 21 ms at 1024). Instead each note
    carries a time on the engine's live clock (`live_now`, microseconds since the process
    started): the MIDI layer turns the driver's timestamp into it per port (`StampClock`: the
    driver's clock sits where the shortest gap from a stamp to its arrival puts it, since a
    message can arrive late but never early, and that gap may grow slowly so a driver clock that
    runs slow is followed). Before every block the render thread tells the renderer when it
    renders it (`Renderer::set_live_clock`), and a fixed delay: the longest the thread waits
    between blocks (a block, or a device buffer that takes several) and one block more. The
    renderer keeps its frames on a steady cadence of blocks, pulled slowly towards the time it is
    told and started again when the two are further apart than the delay, so the render
    thread's own jitter does not move notes. A note plays at the frame where its time and the
    delay fall: in a later block when that is past this one (up to 256 notes wait; a note never
    overtakes one before it from the same source), at the block's first frame when it has
    passed or the note has no time. It trades a little latency (one block more at the smallest
    buffers) for none of the jitter. Tested with synthetic timestamps against the render clock
    (`live_tests.rs`): every block size, a note anywhere in a block, a jittering render thread.

11. **Recording** (amendment, [#266](https://github.com/suhina-daw/suhina-daw/issues/266) and
    [#268](https://github.com/suhina-daw/suhina-daw/issues/268);
    [`docs/spec/recording.md`](../spec/recording.md) has the behaviour). While recording is on
    (`LaunchCommand::SetRecording`) and the transport runs, the render thread copies what the
    armed tracks play into two rings made with the controller and taken by the session
    (`Controller::take_recorder`): every key down and every sound let go (`RecordedNote`), and
    every armed track's input channel or pair in chunks of 64 frames (`RecordedAudio`), each
    stamped with the beat that was heard when it was played: a note's frame less the
    key-to-sound latency (decision 10's delay and the output's latency), an input frame's less
    the output's latency, the input device's (measured in its callback) and the frames that
    waited in the input ring. The output layer tells the renderer the two latencies before each
    block (`Renderer::set_record_latency`). What finds a ring full is dropped and counted;
    nothing is allocated, locked or written on the render thread. A thread in the session
    (`record_writer.rs`) empties the rings as they fill, writes each audio take to a 32-bit
    float WAV file in `Recorded/` next to the project file, and hands the notes on; the session
    decides what a take keeps (its start and end beats) and makes each take one undo step.
    The count-in is the engine's (`LaunchCommand::CountIn`): it clicks the beats with the
    transport stopped and starts the transport on the frame it ends. A loopback test (the
    output fed back into the input offline, with a known delay) checks that a recorded frame
    lands on the frame it was played at.

## Amendment: MIDI monitoring (2026-10-23)

[#808](https://github.com/suhina-daw/suhina-daw/issues/808),
[#810](https://github.com/suhina-daw/suhina-daw/pull/810). MIDI input has the same Auto / In / Off
choice as audio input, in the track's `midi_input` (decision 8): Auto follows the record arm, In
monitors without it, and Off records an armed track without its live sound. `track.setMidiInput`
sets, saves and undoes it; a file without `midi_input.monitor` reads as Auto. The engine gets one
`SetMidiInput` with the arm and the monitoring apart, keeps bounded, preallocated lists of the
tracks that record and of those that monitor, plays notes only on the second and records only
the first. Turning monitoring off lets go of the voices it held. The shallower queue of decision
6 stays on while a track monitors as well as while one is armed. `SetArmed` keeps Auto for
callers that choose no mode. Decisions 2, 3 and 8 say so.

## Amendment: controllers, pitch bend and aftertouch (2026-10-23)

[#812](https://github.com/suhina-daw/suhina-daw/issues/812),
[#837](https://github.com/suhina-daw/suhina-daw/pull/837). Every control change, pitch bend and
channel pressure is read, not only the sustain pedal and all-notes-off (decision 1), and reaches
the monitored tracks' instruments through `Chain::live_control` (decision 2). A track that stops
hearing a source has the bend and pressure it moved set back; stamped controls wait for their
frame as notes do, but take at most half the waiting places, a newer value replacing the one
before it for the same control.

## Amendment: Auto-arm (2026-10-23)

[#834](https://github.com/suhina-daw/suhina-daw/issues/834),
[#840](https://github.com/suhina-daw/suhina-daw/pull/840). With Settings › MIDI › Auto-arm on, the
selected track is armed through `track.setArm` as well as the tracks armed by hand, which it never
disarms; the track it armed before is disarmed. While a take records the selection does not move
the arm. Decision 3 has the rules.

## Consequences

- A MIDI keyboard plays any instrument, built in or a plug-in, on the tracks that monitor it
  (armed ones with Auto, any with In), with the sustain
  pedal, every other controller, pitch bend and channel pressure (#812); the latency follows the
  buffer size chosen.
- Live notes keep the spacing they were played with, a fixed delay after they were played
  (decision 10), instead of jittering by up to one block. Recording (#266) places them by the
  same times.
- A clip note and a live note on the same key of the same track share that key: the later one
  restarts it, and either's release ends it.
- Arming is lost when the project closes; the MIDI monitoring choice is part of the project and
  is kept.
- The last queued block is the one left between the engine and the device; rendering inside the
  device's callback would remove it, but would put plug-in round trips and graph swaps on the
  driver's thread, so it is not done. Lower latency from here comes from real-time priority
  ([#150](https://github.com/suhina-daw/suhina-daw/issues/150)) and fewer plug-in round trips
  ([#108](https://github.com/suhina-daw/suhina-daw/issues/108)), which make small buffers safe.

## Planned

Overdubbing into a clip that exists, punch in and out, and recording automation follow recording
([`docs/spec/recording.md`](../spec/recording.md) › Not yet). An input device on another host
than the output, or at another sample rate (resampled), is not planned yet.
