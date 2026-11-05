---
description: Functional spec of the audio clip's panel in the detail area (the clip column): its groups (Clip, Audio, Gain, Pitch), each control, its range and its command; typing numbers; Set to the playhead; the clip's tempo with its source, halve and double; the gain fader, the pitch knob and cents; the gain envelope on the clip's waveform; and the per-clip audio state they edit
read_when: clip panel, clip column, audio clip properties, gain fader, pitch knob, cents, clip tempo, halve or double tempo, find tempo, tempo source, Set to playhead, type a number, gain envelope, clip.setEnvelope, clip.setTempo, clip.findTempo, per-clip gain
type: spec
status: draft
last_verified: 2026-11-05
covers:
  - crates/ui/src/detail/detail_view/clip_properties.rs
  - crates/ui/src/detail/detail_view/clip_panel.rs
  - crates/ui/src/detail/detail_view/model.rs
  - crates/ui/src/arrangement/envelope.rs
  - crates/session/src/clip_audio.rs
  - crates/model/src/clip_audio.rs
---

# Clip panel for audio clips

The detail area shows a selected clip's properties in a column beside its editor or devices
(the clip column, [clip editor spec](ui-clip-editor.md) › Clip column). For an audio clip the column
is laid out in groups, each control the kind that fits its value
([#1190](https://github.com/suhina-daw/suhina-daw/issues/1190)). Everything it edits belongs to that
clip alone: two clips that play the same file have their own gain, envelope, pitch and warp
([#742](https://github.com/suhina-daw/suhina-daw/issues/742),
[ADR 0001](../decisions/0001-daw-neutral-model.md) › Amendment: per-clip audio). A MIDI clip's
column is as the clip editor spec describes it.

## Groups

From the top, under the title row (the clip's name, and the Clip / Devices switch where there is
one). Each group has a small heading. Rows that do not fit under the column's bottom are left out
whole, as before.

| Group | Controls | Command |
|---|---|---|
| **Clip** | **Colour** (a swatch; a click opens the palette, as a track's colour does) | `clip.setColor` |
| | **Track**, **Start** (or **Scene**), **Length**: read only | |
| | **Start marker** with **Set** | `clip.setStart` |
| | **End marker** with **Set**, when the clip does not loop | `clip.setLength` |
| | **Loop** switch | `clip.setLoop` |
| | **Loop position** and **Loop length**, each with **Set**, when it loops | `clip.setLoop` (`start`, `end`) |
| | **Fade in**, **Fade out**: read only | |
| **Audio** | **Warp** switch | `clip.setAudio` (`warp`) |
| | **Mode**: a drop-down of Re-pitch, Rhythmic, Tonal, Texture, Smooth, High quality | `clip.setAudio` (`mode`) |
| | **Tempo** in BPM, with **÷2** and **×2**, and what it came from | `clip.setTempo` |
| **Gain** | A vertical **fader** with a dB readout under it | `clip.setGain` |
| | **Envelope**: a switch that shows the clip's gain envelope on its waveform to edit | `clip.setEnvelope` |
| **Pitch** | A **knob** in semitones with its readout, and a **Cents** field | `clip.setAudio` (`transpose`) |

In a row (the detail area under the views) the panel has three parts side by side, each 184 points
wide: the Clip group; the Audio group; the Gain and Pitch groups beside each other, the fader as tall
as the room under them allows. With the detail area stacked at the side, the groups follow one under
another.

The title's name is renamed by a double-click on it (a text field; Enter keeps it, Escape leaves
it), through `clip.rename`.

## Numbers

Every number in the column is a field that:

- is dragged up or down (sideways for a marker or loop field, as before) and makes one undo step
  when the drag ends;
- steps with the arrow keys after a click, and takes a number typed after a click with Enter, as
  every value control does ([#707](https://github.com/suhina-daw/suhina-daw/issues/707));
- opens a text field on a **double-click**: the number is selected in it, Enter sets it, Escape or a
  click elsewhere leaves it as it was. A text that is not a number is ignored.

The fader and the knob are not fields: a double-click on them puts them back to 0 (0 dB, 0
semitones). Their readouts are fields, so the gain and the pitch can be typed.

| Value | Range | Steps (drag, arrows) | Shown |
|---|---|---|---|
| Gain | −70 dB to +24 dB (below −70 is silence, shown −inf); on the fader −24 … +24 dB take its upper three quarters, 0 dB at 62.5 % | 0.1 dB | `-6.0 dB` |
| Semitones | −48 to +48 | 1 semitone | `+7 st` |
| Cents | −50 to +50 | 1 cent | `+12 ct` |
| Tempo | 20 to 999 BPM | 0.1 BPM | `96.00 BPM` |
| Markers, loop | 0 to 120 000 content beats; the loop a quarter beat long at least | a quarter beat | `4.25 beats` |

The transpose is semitones and cents together (`clip.setAudio` takes semitones with a fraction, a
cent a hundredth). The knob sets the whole semitones and keeps the cents; the Cents field keeps the
semitones. A transpose with cents past ±50 (from a file or an agent) is shown as the nearest
semitone and the rest.

## Set to the playhead

**Set** beside a marker or loop field puts it where the playhead is in the clip: the playhead's song
beat, as a beat of the clip's content (the clip's start marker plus the beats since the clip's
start, times its repitch rate when it is not warped). Set on Loop length makes the loop end there.
A value outside what the control takes is kept inside it. A slot's clip has no place on the
arrangement's timeline, so its column has no Set buttons.

## Tempo

An audio clip's tempo is the tempo its audio is warped from: the beats between its first and last
warp marker over the seconds of audio between them. A clip that is not warped shows `—` and *not
warped*; a tempo typed into it warps it.

Under the field, what the tempo came from: *from metadata*, *from the file name*, *detected*, *set
by hand*, or nothing for a clip whose tempo was never known (it was not warped when dropped). A
dropped file takes its tempo from its metadata, then its name, then detection, and the clip
remembers which (`docs/spec/browser.md` › Warping).

- **Typing a tempo** (or dragging it) sets it by hand: the audio then fills the beats it holds at
  that tempo. A higher tempo gives the audio more beats, so it plays slower against the song and
  the clip gets longer; its start stays. Warp turns on. An arrangement clip made longer replaces
  what it now covers, as a stretched clip does.
- **÷2** and **×2** halve and double it, for the common half- or double-time miss.
- **Find** (the small button after the source) lists what each source says, `Metadata 96`,
  `File name 96`, `Detected 95.9`, a source that finds nothing greyed; picking one sets the tempo
  with that source (`clip.findTempo`, then `clip.setTempo`).

## Gain envelope

A clip's gain envelope shapes its level over time: points of a beat of the clip's content and an
offset in dB from the clip's gain, −70 dB to +24 dB ([ADR 0035](../decisions/0035-clip-gain-envelope.md)).
Between points it is straight in dB; before the first point and after the last it holds their
values; a clip with no points plays at its gain. The fader moves the whole envelope with it. Two
points on one beat make a step. The fades multiply on top.

Because its points are on the content's beats, as the warp markers are, a looping clip plays the
same stretch of envelope on every pass, a clip whose start marker moves plays the envelope from
there, and a clip that is split gives each part the part of the envelope it plays. Stretching a
clip stretches its envelope with its audio.

**Editing it:** the Gain group's **Envelope** switch shows the envelope of the selected arrangement
clip as a line over its waveform in the Arrangement, the clip's points as small squares. While it
is shown:

- a **double-click** on the clip adds a point there (its beat and the level under the pointer);
- a point is **dragged** to move it: sideways in time, up and down in level, kept between its
  neighbours; Shift moves it by a tenth as much;
- a **double-click** on a point, or Delete with the pointer over it, removes it.

Each of these is one `clip.setEnvelope`, one undo step. The line is drawn on a scale from −70 dB at
the clip's bottom to +24 dB at its top, 0 dB marked. A slot's clip has no waveform there; its
envelope is set by command.

## The grey box

Before #1190 every editable value of the column (Loop to Transpose) was filled with the control
background over the whole row's height, so the rows touched and read as one grey box over the
column (#1190). Fields now keep a gap of 2 points above and below inside their row, so two fields one under
another are 4 points apart, and the switch, drop-down and Set controls are drawn as their own
controls.

## Commands

- `clip.setGain {clip | track + index | track + scene, db}`: the clip's gain.
- `clip.setAudio {…, warp?, mode?, transpose?}`: warp, warp mode, transpose (semitones, cents as
  hundredths). Warp turned on for audio whose file has a tempo sets it to that tempo, unless more
  than two markers were placed; otherwise two markers at its start and end at the tempo where it
  plays.
- `clip.setTempo {…, bpm, source?}`: the tempo, 20 to 999, and its source (`metadata`, `name`,
  `detected`, `manual`; manual by default).
- `clip.findTempo {…}`: `{metadata, name, detected, found, bpm, source}`, nulls where a source
  finds nothing. Changes nothing.
- `clip.setEnvelope {…, points}`: the envelope as pairs, `[beat, dB, beat, dB, …]`; `[]` clears it.

Every one changes only the clip it names and is one undo step.

## Project file

A clip's audio state is the clip's `audio` object (`gain`, `gain_envelope`, `transpose`, `warped`,
`stretch`, `warp_markers`, `tempo_source`), written only when it is not the starting one; the
region keeps the file. Version 8 ([`.suhina` format](suhina-format.md) › Versions).
