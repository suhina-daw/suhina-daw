---
description: How the cue output works: a second output pair on the same audio device for headphones, where browser previews and tracks whose cue (the headphones button) is on are heard apart from the main mix; the cue volume, Same as main, offline renders of the cue, and the commands
read_when: cue, cue output, headphones, pre-listen, solo in place, preview output, cue volume, cue pair, outputs 3/4, track.setCue, output.setCuePair, output.setCueVolume, headphones button, cue button, render --cue, multi-channel device
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/engine/src/cue.rs
  - crates/engine/src/cue_tests.rs
  - crates/engine/src/buses.rs
  - crates/output/src/lib.rs
  - crates/output/src/exclusive.rs
  - crates/session/src/audio.rs
  - crates/session/tests/cue.rs
  - crates/ui/src/kit/widgets.rs
  - crates/automation/src/render.rs
---

# Cue output

A performer listens on headphones to what the audience does not hear yet: a sample from the
browser before dropping it, or a track before bringing it into the mix. The **cue** is that
second listening path. It plays on a second pair of outputs of the same audio device as the main
mix, for example outputs 3 and 4 of an interface with the headphones on them, while the main
outputs (1 and 2, the speakers) go on unchanged.

## What is heard on the cue

- **Browser previews**: samples previewed from the browser and the sample map, and MIDI files
  previewed through the fallback instrument (ADR 0011).
- **Tracks whose cue is on** (the headphones button on a track header, `track.setCue`): the track as
  the mix has it after its own devices, volume and pan, before any group and the main channel.
  Turning the main volume down does not change it. A muted track, or one a solo leaves out, is
  silent on the cue too: the cue listens to what the track plays, it does not override the mix.
- A MIDI file previewed through a track's instrument plays where that track plays: in the main
  mix, and on the cue when the track's cue is on.

The whole cue is scaled by the **cue volume** (0 dB by default, from silence to +12 dB).

Several tracks can be cued at once; their sound adds up on the cue. The cue is project state:
it is saved with the project (a channel's `cue`, written only when on) and undone like any edit.

## Where the cue plays

Settings › Audio › Cue chooses the **cue output**: **Same as main (1/2)**, or a pair of the
chosen device's outputs from 3/4 on (`output.setCuePair`, pair 1 is outputs 3/4, pair 2 is 5/6,
up to pair 31, outputs 63/64). The choice belongs to Suhina, not to a project, and is kept with
the other audio settings.

- **A pair of its own:** the main outputs carry the project only; previews are no longer heard
  there. Cued tracks are heard on both: in the mix, as always, and on the cue pair.
- **Same as main:** the cue has no outputs of its own. Previews play on the main outputs at the
  cue volume, as they did before the cue existed; a track's headphones button changes nothing audible
  (the track is in the main mix anyway). Settings says so for a device with only two outputs.
- **A pair the device does not have** (another device was chosen, or one with fewer outputs
  plays in its place): the cue plays as with Same as main, and a notice says the device has no
  such outputs. The choice is kept for when the device is back.

The output opens with enough channels for the pair: the device's default channel count when
that is enough, else the fewest channels the device offers at the sample rate that hold it. Every
other output channel gets silence.

Choosing a pair opens the output again with the transport where it was, as choosing a device
does. Changing the cue volume does not reopen anything: the engine takes the new level from its
next block.

## Commands

| Command | What it does |
|---|---|
| `track.setCue {track, cue}` | Turns a track's cue on or off. An edit: undone with `edit.undo` |
| `output.setCuePair {pair}` | The pair the cue plays on, 0 for Same as main. Refuses a pair the chosen device lacks, naming the pairs it has; reopens the playing output. Without an audio system it only records the choice |
| `output.setCueVolume {db}` | The cue volume, `-inf` (or below -150) for silence, at most +12 |

`audio.info` and the results of both output commands report `channels` (opened) and
`cue_pair` (the pair the cue plays on now, `null` with the main outputs), and the choice's
`cue_pair` and `cue_db` under `chosen`.

## Offline

Offline renders run the same engine (AGENTS.md rule 4). A render makes the main mix as before,
and the cue only when asked: `suhina-cli render … -o mix.wav --cue cue.wav` writes the cue to a
second WAV file of the same length, at unity cue volume, with cue outputs of their own. The main
file is the same with or without `--cue`, and the same whether tracks are cued or not.

## How it is built

- **Engine** (`crates/engine/src/cue.rs`): the renderer keeps a cue bus beside the main output,
  two buffers of one chunk each made when it is built, so nothing is allocated on the audio
  thread. A cued track always has a device chain (a passthrough one when it has no devices,
  as a track into a group bus does), so its sound is there on its own; the chain adds it after
  its devices to the cue bus, at its own channel's volume and pan. A passthrough chain adds no
  release tail, so a cued track does not lengthen a render. The preview voice renders into a
  buffer of its own, which goes to the cue bus, and to the main output too while the cue plays
  with it. `Renderer::process_with_cue` gives both outputs; `Renderer::process` the main one.
  `LaunchCommand::SetCue` sets the volume and whether the cue has outputs of its own.
- **Output** (`crates/output`): the ring from the render thread to the device carries four
  samples a frame (main left and right, cue left and right). The device callback writes the main
  pair to outputs 1 and 2 and the cue to its pair, on cpal's hosts and in WASAPI exclusive mode
  alike. The output tells the engine whether the cue has a pair of its own when it starts.

## Tests

- Engine (`cue_tests.rs`): a cued track is on the cue and the main mix does not change; after
  its devices; the cue volume and the track's own volume apply, the main volume does not; a
  muted track is silent on the cue; the preview goes to the cue, and to the main output only
  while the cue plays with it; `process_with_cue` matches `process`; the volume is kept in range.
- Output: a fake eight-channel device gets the main pair on 1/2 and the cue on 5/6, silence on
  the rest; a pair the device lacks plays with the main output; channel counts are chosen to fit;
  WASAPI exclusive encoding puts the cue on its pair.
- Session (`tests/cue.rs`, with a stand-in eight-channel device): `track.setCue` undoes and
  redoes; `output.setCuePair` reopens on the pair and refuses pair 4; `output.setCueVolume`
  changes without reopening; nonsense is refused.
- Project file: the cue is written only when on and reads back.
- Render: `--cue` writes the cue beside an unchanged main mix.

## Not yet

- A cue on a second, different device (a separate headphone dongle): two devices mean two
  clocks to keep in step.
- A crossfader between cue and main, or splitting cue and main into the two ears of one pair.
- Level meters for the cue (they come with the mixer's meters).
