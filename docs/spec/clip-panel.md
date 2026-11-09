---
description: Functional spec of the audio clip's panel in the detail area (the clip column): its groups (Clip, Audio, Gain, Pitch), each control, its range and its command; typing numbers; Set to the playhead; the clip's tempo with its source, halve and double; the gain fader, the pitch knob and cents; the gain envelope on the clip's waveform; and the per-clip audio state they edit
read_when: clip panel, clip column, audio clip properties, gain fader, pitch knob, cents, clip tempo, find tempo, tempo source, Set to playhead, audio clip scale, lists under a field, gain envelope, clip.setEnvelope, clip.setTempo, per-clip gain
type: spec
status: draft
last_verified: 2026-11-07
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

## Layout

**Title row** (26 points): the clip's **colour** (a swatch; a click opens the palette, as a track's
colour does; `clip.setColor`), its **name** (a double-click renames it in a text field, Enter keeps
it, Escape leaves it; `clip.rename`) and its length in beats at the right. A line under it.

Under it, in a row (the detail area under the views), three parts side by side, 8 points in from the
panel's edges and 12 points apart; with the detail area stacked at the side, one under another, the
Gain and Pitch part centred. The Gain and Pitch part is at most 150 points tall, as tall as the
other parts, so its fader does not stretch when the detail area is made taller. Every control has its name over it (small, at its left) rather than
beside it, and controls are 6 points apart, so no two touch.

| Part | Controls | Command |
|---|---|---|
| **Clip** (156 points) | **Start** and **End** side by side, each with **Set** at the right of its name | `clip.setStart`, `clip.setLength` |
| | **Loop**: a switch the width of the part, lit while on | `clip.setLoop` |
| | **Position** and **Length** of the loop side by side, each with **Set**; faint while the loop is off, a change turns it on | `clip.setLoop` (`start`, `end`) |
| | **Scale**: the root (C, C#/Db … B) and the mode (None, then Major, Minor … Blues), each a list | `clip.setScale` |
| **Audio** (112 points) | **Warp**: a switch the width of the part | `clip.setAudio` (`warp`) |
| | **Mode**: a list of Re-pitch, Rhythmic, Tonal, Texture, Smooth, High quality | `clip.setAudio` (`mode`) |
| | **Tempo** with **Find** at the right of its name; under the field **÷2** and **×2**, and what the tempo came from | `clip.setTempo`, `clip.findTempo` |
| | **Edit gain envelope**: a switch that opens the gain envelope on the waveform for editing (every audio clip's, or the clip's own, by Settings › *Envelope editing*), as Alt+E and the button by the clip's name do | `clip.setEnvelope` |
| **Gain and Pitch** (128 points) | **Gain**: a vertical fader, its scale at its left (24, 0 half-way, −inf at the bottom), the dB readout under it | `clip.setGain` |
| | **Pitch**: a knob in semitones; under it the semitones and the cents side by side, `st` and `ct` under them | `clip.setAudio` (`transpose`) |

**Lists** (Mode, the scale's root and mode) open right under their field, as wide as it (the scale's
mode as wide as both scale fields), the value they hold lit; a click on an entry sets it and closes
the list, a click elsewhere closes it.

**Scale.** An audio clip's scale says the key of its audio, as a MIDI clip's says the key its notes
are written in; it changes nothing that plays. It is the content's, as a MIDI clip's is, and a content
other clips play is copied for this clip first.

## Numbers

Every number in the column is a field that:

- is dragged up or down, and the clip follows while it moves (a few times a second), the whole
  drag one undo step (`continue` on the command), as the project's tempo field does. The tempo's
  drag is also drawn on every frame in between: the Arrangement shows the clip stretched to the
  tempo dragged so far, as it shows an edge stretch, until the project has it. So is the pitch
  knob's on an unwarped clip, whose length the transpose changes (an octave up halves it);
- a tempo, marker or loop field is dragged by the place under the pointer where it was pressed:
  the tempo's integer part steps 1 BPM and each decimal a tenth of the one before (#829); a
  marker's bar, beat or sixteenth steps a bar, a beat or a quarter beat. The place is underlined;
- opens a text field on a **click** (a drag moves the value instead): the number is selected in
  it, Enter sets it, Escape or a click elsewhere leaves it as it was, and the arrow keys step it
  while it is open, as they step every value control
  ([#707](https://github.com/suhina-daw/suhina-daw/issues/707)). A text that is not a number is
  ignored. A double-click was needed before, and a click that only took the keys showed nothing.

The fader and the knob follow a drag as the fields do, and take keys the same way: clicked, a
number typed and Enter set them and the arrows step them (the fader 1 dB, Shift 0.1; the knob a
semitone). A double-click on them puts them back to 0 (0 dB, 0 semitones). A marker typed as
`3.2` is bar 3, beat 2; `3` alone is bar 3's start.

| Value | Range | Steps (drag, arrows) | Shown |
|---|---|---|---|
| Gain | −70 dB to +24 dB (below −70 is silence, shown −inf); on the fader 0 dB is half-way, 0 … +24 dB the upper half, −24 … 0 dB the next three tenths, −70 … −24 dB the lowest fifth | 0.1 dB | `-6.0 dB` |
| Semitones | −48 to +48 | 1 semitone | `+7` |
| Cents | −50 to +50 | 1 cent | `+12` |
| Tempo | 20 to 999 BPM | the place grabbed: 1, 0.1, 0.01 or 0.001 BPM | `96.000` |
| Markers, loop position | 0 to 120 000 content beats, as `bar.beat.sixteenth` from 1 in the project's time signature | the place grabbed: a bar, a beat or a quarter beat | `3.2.1` |
| Loop length | a quarter beat to 120 000 beats, as `bars.beats.sixteenths` from 0 | as the markers | `1.0.2` |

The transpose is semitones and cents together (`clip.setAudio` takes semitones with a fraction, a
cent a hundredth). The knob sets the whole semitones and keeps the cents; the Cents field keeps the
semitones. A transpose with cents past ±50 (from a file or an agent) is shown as the nearest
semitone and the rest.

## Set to the playhead

**Set** beside a marker or loop field puts it where the playhead is in the clip: the playhead's song
beat, as a beat of the clip's content (the clip's start marker plus the beats since the clip's
start, times its repitch rate when it is not warped). Set on Loop length makes the loop end there.
A value outside what the control takes is kept inside it. A slot's clip has no place on the
arrangement's timeline, so its column has no Set buttons. While the playhead is outside an
arrangement clip its Set buttons stay, greyed, so they do not come and go as a loop passes the
clip's end.

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
  the clip gets longer; its start stays. Warp turns on. An arrangement clip made longer stops at the next clip on its lane, so a
  tempo dragged while it plays never removes another clip.
- **÷2** and **×2** halve and double it, for the common half- or double-time miss.
- **Find** (the small button after the source) lists what each source says, `Metadata 96`,
  `File name 96`, `Detected 95.9`, a source that finds nothing greyed; picking one sets the tempo
  with that source (`clip.findTempo`, then `clip.setTempo`).

## Gain envelope

A clip's gain envelope shapes its level over time: points of a beat of the clip's content and an
offset in dB from the clip's gain, −70 dB to +24 dB ([ADR 0035](../decisions/0035-clip-gain-envelope.md)).
Between points it is straight in dB, or bent along the first point's curve as an automation
segment bends ([automation](automation.md) › Lanes and points); before the first point and after the last it holds their
values; a clip with no points plays at its gain. The fader moves the whole envelope with it. Two
points on one beat make a step. The fades multiply on top.

Because its points are on the content's beats, as the warp markers are, a looping clip plays the
same stretch of envelope on every pass, a clip whose start marker moves plays the envelope from
there, and a clip that is split gives each part the part of the envelope it plays. Stretching a
clip stretches its envelope with its audio.

**The scale.** On a clip in the Arrangement the envelope is drawn over the waveform's area (under
the name bar): 0 dB, the sample as it is, across its middle; the ceiling (Settings › Arrangement ›
Gain envelope ceiling: +3, +6, +12 or +24 dB, +6 by default) at its top; −70 dB at its bottom.
The upper half is even in dB. The lower half goes by the gain a level plays at, as the waveform is
drawn: the first dB of a cut, where most cuts are, take most of it (−1 dB about 11 % of the way
down, −3 dB about 30 %, −6 dB half, −20 dB 90 %), so a cut of a few dB is a drag of many points,
not of a few, and the line's height reads like the waveform's. A point set
above the ceiling before it was lowered keeps its level, and the scale's top reaches it, so the line
is always drawn where it is heard.

**The waveform follows it.** The clip's waveform is drawn through its envelope, as through its gain
and fades: while a point is dragged the waveform grows and shrinks with it, before the button is let
go.

**Seeing it.** With **Show gain envelope** on (Settings › Arrangement, on by default), every audio
clip with points shows its line, thin, while it is not being edited.

**Editing it.** **Edit gain envelope** opens envelopes for editing: Alt+E (`view.gainEnvelope`),
the small envelope button left of a clip's name, or the clip panel's switch; the same again closes
them. Settings › Arrangement › Gain envelope › *Envelope editing* (`gain_envelope_editing = all |
one`) says which open:

- **All audio clips** (the default): it is a mode. Every audio clip's envelope opens and closes
  together, from any clip's button or the panel, and Alt+E turns it on or off with or without a
  clip selected, so the envelopes of several clips (doubled vocal takes) are set side by side;
- **One clip at a time**: only that clip's envelope opens (Alt+E with the clip selected), and
  opening another clip's closes it.

While an envelope is open:

- only the line and the points take the pointer: a click or a drag anywhere else on the clip
  selects and moves it as ever. The pointer within 5 points of the line is a pointing hand; over a
  point, an open hand, and a closed one while it is dragged;
- faint lines show the dB grid (Settings › Gain envelope grid: 0.5, 1, 2, 3 or 6 dB, 1 dB by
  default; lines closer than 5 points are left out), the 0 dB line stronger;
- the points are round dots, as an automation lane's are; two never sit on each other: a click on
  the line within a dot's width (8 points) of a point adds none, and a point let go within a dot's
  width of another's dot, on any pass of a loop, is one with it;
- a **click** on the line adds a point there, on the envelope grid, at the level the line has
  there, so the line keeps its shape;
- a **drag** from the line adds a point and moves it; a point is **dragged** to move it: sideways in
  time on the envelope grid, up and down in level on the dB grid and below the ceiling;
  dragged over another point it passes it, as on an automation lane, and on another point's beat it
  stays on the side it came from. Within 3 points of the middle it goes to 0 dB itself, the sample's
  own level. Alt holds it off both grids and the middle while it is dragged, as Free move does (and
  as Alt frees a drag on the piano roll's grid); Shift moves it a tenth as much, off the grids too. Its value shows beside the pointer
  while the button is down ("−3 dB");
- the **ring** half-way along a sloping segment (hollow, as a device's curve handle is, so it is not
  taken for a point) (one at least 28 points wide) bends it when
  dragged up or down, as an automation lane's does, and a double-click on it straightens it; Alt
  and a drag from the line bend the segment under the pointer the same way. A point added on a bent
  segment cuts its curve in two, so the line keeps its shape (`Curve::split`);
- **Free move**, a button under the name and an item of the clip's menu, lets points move freely,
  in time and to a tenth of a dB, instead of on the grids; Settings › Envelope points move says
  which it starts as;
- the **envelope grid** is the envelope's own, not the Arrangement's: 1/16 until another is picked
  under **Envelope grid** in any audio clip's menu (8 Bars to 1/32, as the piano roll's fixed grid,
  and 1/64). The pick holds for every clip's envelope until the app quits. Finer than 1/64 is Free
  move: 1/128 is about two points apart at a usual zoom;
- while a clip's edge is dragged, its envelope is drawn with its waveform: a looping clip dragged
  longer shows the envelope on every new pass at once;
- **Shift** and a drag of the line moves the whole line up or down by the same dB, and
  **Ctrl+Shift** and a drag moves only the segment under the pointer (its one or two points), on
  the dB grid unless Free move is on or Alt held; the change shows beside the pointer ("−2 dB"). A
  line without points gets one where the drag started;
- **Alt+Shift** and a click on the line takes away the segment's points;
- a **double-click** on a point puts it back at 0 dB where it is; **Alt** and a click on it, or
  Delete with the pointer over it, removes it;
- **Clear gain envelope** in the clip's menu takes all its points away, one undo step, so the clip
  plays at its own level again; it is off while the clip has none.

These follow a spectral editor's clip gain, where a click on the line adds a point, a double-click
puts one back at 0 dB, Alt and a click removes one and Shift or Ctrl+Shift and a drag of a segment
moves the line or the segment up and down; Ctrl and a click anywhere, which adds a point there, is
left out, as Ctrl and a click selects clips in the Arrangement.

Each change is one undo step. A drag sets the envelope as it moves, at most every 30 ms, with a `gesture`; an arrangement clip's envelope reaches the playing engine as a message, without a new graph ([ADR 0005](../decisions/0005-edits-while-playing.md), amendment: edits in the next block, 5),
so the clip is heard as the line is drawn while the button is down (each new level reached by a
30 ms glide, not a step, which crackled), and the drag's frames are one undo step; a click is one `clip.setEnvelope`. The clip itself is moved by its name bar while
its envelope is edited. A slot's clip has no waveform there; its envelope is set by command.

The points stay on the sound they were put on. Their beats are the content's, so whatever moves
the audio against the beats moves them with it: a clip's tempo or a stretch scales them with the
warp markers, and Warp turned on or off, or the project's tempo under unwarped audio, moves each
to the beat its second of the file is at now, as the clip's offset, length and loop move.

## The grey box

Before #1190 every editable value of the column (Loop to Transpose) was filled with the control
background over the whole row's height, so the rows touched and read as one grey box over the
column (#1190). The panel now keeps its controls 6 points apart, with their names over them.

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
- `clip.setEnvelope {…, points, curves?, gesture?}`: the envelope as pairs, `[beat, dB, beat, dB, …]`; `[]` clears it. `curves` gives each point its curve to the next, in the order of `points`: a bend from -1 to 1 or `[x1, y1, x2, y2]`, `null` for straight. Two points in one place (the same beat and dB), such as one dragged onto another, are one point; a beat holds at most two, a step, and a third there replaces the second. Calls with the same open `gesture` (a drag) are one undo step.

Every one changes only the clip it names and is one undo step.

## Project file

A clip's audio state is the clip's `audio` object (`gain`, `gain_envelope`, `transpose`, `warped`,
`stretch`, `warp_markers`, `tempo_source`), written only when it is not the starting one; the
region keeps the file. Version 10 ([`.suhina` format](suhina-format.md) › Versions).
