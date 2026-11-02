---
description: How recording works: MIDI and audio takes into a clip slot or the arrangement, the count-in, where a played note or a recorded sound is placed (lined up with what was heard), the files audio takes are written to, and what happens when a disk fills or a device is lost
read_when: recording, record, take, record button, clip.record, transport.record, count-in, transport.setCountIn, record.state, MIDI recording, audio recording, Recorded folder, latency compensation of a take, overdub, loopback
type: spec
status: current
last_verified: 2026-11-02
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
clip slot of the Launcher view or in the Arrangement. Each take is one edit: one undo removes it.

This spec is written from how recording is commonly understood and from Suhina's own engine; it
names no other program's behaviour.

## What is recorded

- **Armed tracks only.** Arming (`track.setArm`) is what says a track listens to its inputs (with
  Settings › MIDI › Auto-arm on, selecting a track arms it, but not while a take records, so a
  take is never cut off by a click elsewhere, #834 — a take asked to end counts until it is a
  clip, so a slot's take records on to its launch boundary); a take is made on every armed
  track that heard something it can keep, and every armed MIDI track makes one, an empty clip
  when it heard no notes:
  - a track that plays notes records the notes of the MIDI inputs it hears
    (`track.setMidiInput`), as a MIDI clip;
  - a track with an audio input (`track.setAudioInput`) records the input's channel or pair,
    before the track's devices and mixer, as an audio clip. Monitoring (In, Auto, Off) changes
    only what is heard while recording, not what is recorded. An audio track armed in the
    window while it hears no input gets the device's first input (channels 1/2, or 1 on a
    one-channel device), so Record has something to record; recording an armed track that
    hears none is refused with the track's name.
- **Notes as they were heard.** A note starts when its key went down and ends when the sound
  was let go: at the key's release, or, while the sustain pedal is down, when the pedal comes
  up. A note still held when the take ends is cut at the take's end. Notes are kept with their
  velocity; MIDI channel and pedal are not stored as such.
- **A MIDI take with no notes still makes its clip** (an empty clip of the take's length), so
  what was recorded is visible; an audio take always has sound, even if silent.

## Where it goes

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

While a take is running, Arrangement shows a growing recording-coloured region on its
track, from its start to the current transport beat. This preview does not create project
clips or undo entries. The finished clip replaces it after recording ends; notes and
waveforms are drawn from the finished take. New recorded clips receive the corresponding
MIDI/audio creation colour without recolouring earlier clips.

`transport.record` with `on: true` records every armed track into the arrangement:

1. With the transport stopped, it starts after the count-in, and the take starts at the play
   start marker. With the transport running, the take starts at the playhead.
2. `transport.record` with `on: false` or `transport.stop` ends it at the playhead.
3. Each take becomes a clip on its track from the beat recording started to the beat it ended,
   replacing what it covers, like a clip pasted there, as soon as it has ended: the arm, Play or
   Stop make no difference (#909).
4. **Every pass over the beats is a take of its own.** When the playhead jumps while recording
   (the loop wrapping at its end, or a click on the ruler), the pass so far ends where the
   playhead was and a new one begins where it went. Each pass becomes a clip on the track, the
   later ones over the earlier, so the latest plays; and each is kept on a take lane under the
   track (*Take lanes* below), so nothing played is thrown away. A MIDI pass holds the notes
   played in it (a key held across the jump is cut at the pass's end); the passes of an audio
   take are clips of its one file, each from where its pass began. When the playhead ended before
   where the last pass began, that pass reaches its last note, at least a bar.

While a take records into the arrangement, it is drawn on its track as a clip from where its
pass began to the playhead, edged in the record colour and named **Recording**, with its notes
or, for audio, its waveform drawn as they come in. After a jump it starts again at the new pass.

### Take lanes

A track that recorded into the arrangement has its passes as **take lanes** under it, once the
recording stops: while it runs nothing is added under the track, and the clip being recorded has a
mark in its lower left corner, three stacked lines, as have the parts on the track and the pieces of
the takes. Lanes a track already has stay as they are while it records again. The lanes go oldest
first, shown with the track's name: **Drums Take 1**, **Drums Take 2** and so on (the project keeps
"Take 1", so the names follow a renamed track). A take keeps its number when another is deleted, and
a new pass takes the next number after the highest one the track's takes show, so no two have one
name; the take commands' `take` is that number. Take lanes do not play: their clips are drawn
greyed. They are where the best parts of each pass are picked and put together on the track
(comping):

- The pointer over a take lane is the text cursor, as for selecting. A single click takes nothing;
  a **double-click** on a piece of a take copies the whole piece onto the track over what is there.
  With **Ctrl** held the take lane takes ordinary clicks: a click adds a piece to the pieces picked
  or takes it out (each outlined; **Recolor** colours the one clicked), and a drag comps nothing. A
  click without Ctrl lets go of them all. **Delete** deletes the picked pieces
  (`take.deletePieces {track, take, pieces}`, one undo step per take), and a take left with no piece
  goes; the track's clips stay.
- **Ctrl and a drag** anywhere on a take lane only paints an **area** across the track's take lanes:
  the lanes from the one the drag began on to the one under the pointer, between two beats (on the
  grid; Alt for no grid). Pieces wholly inside it are picked too. **Delete** deletes what is in the
  area (`take.deleteRange {track, takes, start, end}`) and **Ctrl+X** cuts it for pasting
  (`take.cutRange`), one undo step either way: a piece crossing an edge keeps its parts outside, and
  a take left with nothing goes. A click on a take lane, a clip or the time selection lets go of the
  area.
- **Moving a piece:** a piece of a take moves as a clip does (Settings › Arrangement › *Clip drag
  handle*, [arrangement editing](arrangement-editing.md)): a drag from its name bar, or from its lower
  half, moves it along the timeline on the grid (Alt for no grid), with the pieces picked when it is
  one of them, each on its own lane (`take.movePieces {track, takes, pieces, by}`, one undo step). A
  moved piece covers what it lands on in its lane and plays what it played. Parts comped from it
  earlier stay where they are on the track and no longer count as its parts; a part comped after the
  move does.
- **A piece's edges:** a piece's lower right and lower left corners drag its edges (on the grid; Alt
  for no grid; `take.trimPiece {track, take, piece, start?, end?}`). So what an area cut away comes
  back, and more: a take lane is a window on one continuous recording, its passes clips of one file
  at different offsets, so an audio piece goes on up to the end of that file, and takes all starting
  at bar 1 and pulled out fully form a staircase, the earliest the longest. A MIDI pass has a content
  of its own, so a MIDI piece stops where its pass ends. An edge also stops at the take's other pieces.
- **Drag** across a take from the part of a piece that does not move it: the part dragged over (on
  the grid; Alt for no grid) is copied onto the track over what is there, as new clips that start in the take where the part does. Each copy is
  one undo step.
- **Right-click** a take's name or lane: **Recolor lane** gives the whole take a colour of its own
  and its parts on the track with it (`take.setColor`); **Recolor** colours only the piece of the
  take the right-click was over (a split take has several) and its parts (`take.setColor` with
  `piece`); **Delete take** removes the lane, and the track's clips stay.
- **Hide take lanes** / **Show take lanes** in the right-click menu of the track's lane folds
  them away and back; **Delete unused takes** there deletes the take lanes the comp uses nothing
  of (`take.deleteUnused`).
- A take lane's header has buttons, as a track's: **✕** deletes the take, **S** solos it (the
  take is heard in place of the track's clips, which grey; not saved and not an undo step,
  `take.solo`), the colour swatch recolours the lane, and **↑** puts the whole take on the track
  (`take.promote`).
- A take lane is a child row of its track: as tall as a track that has not been resized, and the
  lanes' zoom (Alt and the wheel) takes it along. Dragging the line under one makes that lane alone
  taller or shorter, within a track's limits; a double-click on the line puts it back. The heights
  are the view's, not saved with the project.

The track's lane reads as the comp (#989). A clip on the track that plays a take's content from
the same place is a **part** of that take, found by its content and by where in it each beat lands
(the passes of an audio take are clips of one file at different offsets; each MIDI pass has a
content of its own), so nothing extra is stored and a trimmed part keeps its take:

- Each take lane has a colour of its own, the palette's in turn until recoloured, and a part is
  named after its take ("Drums Take 4") and shows its piece's colour unless it has one of its own; the track's own copy of the last pass is a part of that take too.
- On each take lane the spans that are on the track are drawn lit and the rest dimmed.
- **Alt+Up** / **Alt+Down** swap every selected part for the same span of the take above or below,
  in turn round the takes that have something there (`take.swap`), each one undo step.
- Dragging the edge between two neighbouring parts moves their cut: one gets longer and the other
  shorter, each still playing its own take (`take.moveCut`).
- Splitting a clip on the track (`clip.split`) splits every take lane of the track at the same
  beat, so the takes stay cut where the comp is.
- Where two parts from different takes meet, they crossfade: half of the length on each side of the
  cut, with equal-power fades (the engine works this out from the project when it builds its graph,
  so trims, swaps, splits and undo keep it right; ARA clips are left out). The length is the longer
  of the two parts' fades at the cut, and 10 ms when neither has one. An audio part comped from a take
  gets fades as long as Settings › Arrangement › *Take crossfade* says (10, 20 or 40 ms, or a custom
  length), so its outer edges never click either. The setting is for parts comped from then on.
- A cut with a crossfade shows a cross over the cut, at least wide enough to see, and a handle at its
  top. **Dragging the handle** sideways sets the crossfade's length, up to the shorter part's, as
  both parts' fades at the cut (`take.setCrossfade {clip, length?}`, one undo step); the pointer over
  it says the length in milliseconds. A **double-click** puts it back to the length in Settings.
- **A part takes its takes along.** A clip on the track that is a part of a take takes what all the
  track's takes hold under it to where it lands, cut to its span, in the same undo step: moved
  (`clip.move`, `clip.moveMany`) the pieces leave where they were; copied (a Ctrl drag, Ctrl+C and
  Ctrl+V, Ctrl+D, a duplicated time selection) they stay there as well. What is already at the spot
  is not added twice: a clip of the same content, lined up with the beats the same way, plays the same
  audio at the same place, so nothing extra is stored to recognise it. The rest goes on the same lanes
  when those are free where it lands; if any of them holds something else there, nothing is replaced
  and it gets as many new lanes as it fills. On another track it goes on that track's first lanes when
  those are free, or on new lanes; a lane left with nothing goes. A clip that is no part of a take
  moves alone, and a part pasted from another project does not know its takes. A copied part keeps
  playing its take's content until it is edited (copy on write).
- A take lane's header is drawn in a dark shade of the take's colour with a strip of the colour
  itself; its pieces are that colour, darkened where the comp does not use them.

Take lanes are saved with the project, a take's own colour with them (the project format,
[`suhina-format.md`](suhina-format.md)); deleting the track deletes its takes. A part on the
track is a clip like any other: editing it leaves the take as it was.

While recording into the arrangement, launching a slot plays it as usual; it is the slot record
button that records into slots.

### Into a Sampler

The Record and Stop controls on a Sampler's waveform run `device.recordSample`: the track must be
armed with an audio input chosen, and the project saved. The take goes through the same writer
into the instrument's source, with no clip. It starts and stops at the current arrangement beat,
not on the launch quantisation. Loading the finished take is one undo step, and undo keeps the
WAV file.

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

After a stop, or when recording is turned off while playing, the input goes on being recorded for
that latency, so the last frames played reach the take; before, the take's end was cut by the
latency, and on a loop the later passes found nothing of themselves in the file and were dropped.

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

An offline render or a test cannot let the writer thread run at its own pace and expect the same
result on every machine. `Session::sync_recording` waits until the thread has emptied the rings
and reported what it found, and with `finish` set (no more audio is coming) it ends the takes that
were ended, without the wait the thread gives a device for frames still on their way.

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
| `take.comp` | `track`, `take` (from 1), `start?`, `end?`: copies the take, or its part from `start` to `end` beats, onto the track over what is there; one undo step |
| `take.delete` | `track`, `take`: deletes the take lane; one undo step |
| `take.setColor` | `track`, `take`, `color` (`"#rrggbb"` or null), `piece?` (from 1): the take's colour and its parts', or one piece's and its parts'; one undo step |
| `take.solo` | `track`, `take` (from 1, or 0 for none): the take heard in place of the track's clips; not an undo step |
| `take.promote` | `track`, `take`: every piece of the take put on the track over what is there; one undo step |
| `take.deleteUnused` | `track`: deletes the take lanes the comp uses nothing of; one undo step |
| `take.swap` | `clip`, `direction` (`up` or `down`): the part replaced by the same span of the next take that has something there; one undo step |
| `take.moveCut` | `clip`, `to`: moves the cut between the part and the one right after it; one undo step |
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
- Without a MIDI keyboard, the computer keyboard plays notes (#944): the control bar's **Keys**
  button, or Ctrl+K (`view.computerKeyboard`), turns it on. A W S E D F T G Y H U J K play C to
  the C above (the black keys on the upper row), O L P ; go on up; Z and X move the octave
  (A is C3 at first, C-2 to C8), C and V the velocity in tenths. The notes go through the same
  live input a controller's notes take ([ADR 0017](../decisions/0017-inputs-and-latency.md)),
  so they sound on the armed tracks, monitor and record alike. A held key plays one note; its
  repeats play nothing. While it is on those keys run no shortcut, and a text field with the
  keyboard takes them as typing; turning it off, or a field taking the keyboard, ends the
  notes held.

## Not yet

- Overdubbing into a clip that exists (recording adds notes to a playing clip).
- Punch in and out points.
- Hearing a take lane on its own (audition), and marking on a take which parts are on the track.
- Recording automation (a moved parameter written as a lane).
- Recording a track's output (resampling) rather than its input.
