---
description: What the level meters measure (tracks, groups, returns, the main output), their ballistics, the clip indicator, where the window shows them and the commands that read and reset them
read_when: level meter, meters, peak, RMS, clip indicator, clipping, peak hold, mixer.levels, mixer.resetClip, MeterMonitor, track header meter, mixer strip meter, dBFS
type: spec
status: current
last_verified: 2026-10-15
covers:
  - crates/engine/src/meters.rs
  - crates/engine/src/meters_tests.rs
  - crates/session/src/meters.rs
  - crates/session/tests/meters.rs
  - crates/ui/src/kit/meter.rs
  - crates/ui/tests/meters.rs
---

# Level meters

Every track, group and return track, and the main output, has a level meter: how loud it is
now, per side, and whether it went over full scale. The engine measures; the window, the CLI and
MCP read the same numbers.

## What is measured

- **A track** is measured at its own level: after its own devices, volume and pan, before the
  groups and the main channel it plays through. Turning the main volume or a group's volume down
  does not move a track's meter; turning the track itself down does.
- **A group** shows the sum of what plays into it, after the group's own volume (and after its
  devices, when it has them). A group's pan does not apply, as when it is mixed.
- **A return track** is measured like a track, from what plays on it.
- **A track that is not heard** (turned off, or not soloed while others are) reads silence.
- **The main output** is what the engine hands the audio device: every track through the main
  channel, the metronome and the browser's preview.

Levels are linear, 1.0 being full scale (0 dBFS). A centred track at unity volume playing a
full-scale audio clip reads 0 dB; the built-in sine voice of a note at full velocity reads
−12 dB.

## Ballistics

The engine measures each audio block (at most 1024 frames):

- **Peak:** the highest sample of the block, per side. Once the sound is quieter the shown peak
  falls at 24 dB a second (`PEAK_FALL_DB_PER_SECOND`), so a short hit stays visible.
- **RMS:** the mean square of the block, averaged with a time constant of 0.3 s
  (`RMS_SECONDS`), shown as its square root and never above the peak. A steady sine reads its
  peak less 3 dB.
- **Silence:** below −120 dB a level reads exactly zero, so a falling meter ends.
- **Clip:** a sample above full scale lights the meter's clip indicator, which stays lit until it
  is reset (a click on the meter, or `mixer.resetClip`). A track is checked at its own level, so
  a track can clip while the main output does not, and the other way round.

The window keeps the highest peak of the last 1.5 s as a line (peak hold); that is drawing only,
the engine does not hold peaks.

## In the window

Sizes and tokens are in [components](../design/components.md) › Meter.

- **Session view:** each mixer strip (tracks, groups, returns) shows its meter right of the pan
  knob, standing up; the main strip under the scene column shows the main output's.
- **Arrangement view:** each track header shows its meter standing at its right edge, as tall as
  the rows the header shows; the return and main rows along the bottom show theirs lying down,
  left of the volume.
- The scale is the volume control's: −70 dB at the bottom (or left) to +6 dB at the top, in dB.
  The peak is drawn dimmer, the RMS level over it in the full colour; above −3 dB (the top 12 %)
  the bar turns `meter.hot`. The clip indicator is a light at the top (or right end) in
  `recording` while lit; a click anywhere on the meter turns it off. Each meter is named for
  screen readers, "Drums level: −12.0 dB" or "Main level: silent, clipped".
- The window draws again whenever the levels change, and looks for new levels every 50 ms for
  0.3 s after they last changed while a meter shows something, so falling meters reach the
  bottom; an engine that stopped rendering leaves the window at rest.

## Commands

Both need an engine playing the project (`suhina-cli play --launcher`, the MCP server with
`--audio`, or the desktop app); without one they return an error. Neither is an undo step.

- `mixer.levels` returns `main` and `tracks` (in the project's order, with `track` and
  `track_name`), each with `peak_db` and `rms_db` as `[left, right]` in dB to a tenth (`null`
  for silence) and `clipped`.
- `mixer.resetClip` turns off clip indicators: `track` (id or name) for one track, `main: true`
  for the main output, both for both, neither for every one.

## Limits

- The first 512 tracks of a project have meters (`MAX_METERED_TRACKS`); tracks after them
  have none.
- An edit that moves tracks to other places in the project's order restarts the meters that
  moved from silence, with their clip indicators off; an edit that keeps the order (a volume,
  a clip) keeps them.
- A track whose path to the output has a volume of zero after its own (a group at −inf) reads
  silence, as nothing of it is heard.
- Meters of the cue output and of track inputs come with them (ROADMAP).

Sample instruments additionally publish up to 32 independent voice positions in fixed
atomic slots per metered track. These are read heads, separate from audio levels;
inactive voices are absent and do not leave a stale waveform line.

Sampler voice positions are published through the assigned chain-to-meter slots;
the render callback does not search all meter rows for each chain.
