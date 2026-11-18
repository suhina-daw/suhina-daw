---
description: Functional spec of the audio editor in the detail area: what a double-clicked audio clip shows (its whole file across the content's beats under a bar, beat and sixteenth ruler, the played part lit, the loop's edges), the waveform colour, zoom and scroll, and editing warp markers (add, drag, remove, reset warp)
read_when: audio editor, audio clip editor, waveform view, big waveform, detail area waveform, double-click audio clip, warp markers, add warp marker, move warp marker, reset warp, align snares, audio ruler, audio grid, waveform colour
type: spec
status: draft
last_verified: 2026-11-16
covers:
  - crates/ui/src/detail/audio_editor.rs
  - crates/ui/src/app/detail_area.rs
  - crates/session/src/clip_audio.rs
  - crates/session/src/commands/clip.rs
  - crates/ui/src/detail/detail_view/clip_panel.rs
  - crates/engine/src/ahead.rs
---

# UI: audio editor

Functional spec of the view the detail area shows for a selected audio clip, in the place where a
MIDI clip shows its piano roll. Suhina's own design, written in our own words under the clean-room
rules (`CONTRIBUTING.md` › Clean-room).

**What is built.** The audio shown large under a ruler, its warp markers added, moved and removed
on it, and a dialog that resets the warp. Playing from a click on the ruler is **Later**.

## Where it opens

- A double-click on an audio clip (arrangement or launcher slot) opens the detail area on it, as
  it does a MIDI clip's piano roll ([clip editor spec](ui-clip-editor.md) › Where it opens). A
  click only selects, so a hidden editor stays hidden.
- The clip's column ([clip panel spec](clip-panel.md)) is at its left, the editor fills the rest.
- The **Clip / Devices** switch, the stacked devices and *Devices beside piano roll* act as they
  do for a MIDI clip: the editor is the clip's side of the switch.
- An audio clip on a track whose plug-in edits clips (ARA) keeps that plug-in's editor in this
  place ([ARA spec](ara-hosting.md)).
- The area is resized with its splitter like any clip editor, and pulled to the top with Escape
  twice. Its least height is a ruler and about 90 points of waveform.

## What it shows

The horizontal axis is the clip's **content beats**: where the warp markers are, so a marker sits
in the same place whichever clip plays the file. The file is drawn once, from its first sample to
its last, at the zoom below. Each pixel column reads the stretch of the file the warp markers put
there, as [waveforms](waveforms.md) draws any waveform (peaks with RMS, two lanes for a stereo file
when tall enough, samples close up, the dB scale setting).

### The ruler

A strip along the top (28 points) that counts where you are in the audio, so a sound can be put
on a beat:

- **Bars** are numbered from 1 (content beat 0 is the start of bar 1), every few bars when they
  would crowd; **beats** are numbered `bar.beat` once beats are 32 points apart, and
  **sixteenths** `bar.beat.sixteenth` once they are close enough to read.
- **The grid** has lines at the finest of a 32nd, 16th, 8th and quarter note, then one bar, two,
  four and so on, whose lines are at least 10 points apart: it gets finer as the view zooms in.
  A bar's line is the strongest, a beat's weaker, the rest faint; they run down the waveform as
  well as along the ruler, under the waveform.
- **The grid's value** is the editor's own, as the piano roll's is its own: Adaptive (the lines
  follow the zoom) until another is picked, in the clip panel's Warp part (› Grid list, Snap) or
  with the piano roll's keys while the editor has the keyboard (it, or the clip's panel, was
  pressed last): **Ctrl+1** finer, **Ctrl+2** coarser, **Ctrl+3** triplets, **Ctrl+4** snapping on
  or off. The status bar says the grid picked. A fixed value too fine for the zoom is drawn and
  snapped to coarser, so lines are never closer than 10 points. With snapping off (or Alt held)
  marks go where the pointer is. The Arrangement's grid is left alone.
- **Under the pointer**, a thin line down the waveform and, on the ruler, where it is as
  `bar.beat.sixteenth` snapped to the grid, so the beat under a snare can be read off.

The lines, the numbers and the strip take the theme's `grid-line`, `label` and `detail-fill`.

### The rest

- **The part the clip plays** is lit: from its start (the offset) to its loop's end, or to where
  its length reaches. What it does not play is dimmed.
- **Loop**: a line in the clip's colour at each edge of the loop.
- **Warp markers**: a line from the ruler down the waveform in the clip's colour with a flag on
  the ruler, for each marker of a warped clip. A clip that is not warped has none, and says *Warp
  is off*: turn Warp on in the clip panel.
- The waveform is at unity gain; the clip's gain, fades and envelope are not applied, so quiet
  audio is seen as recorded.
- A file whose waveform is not there yet says *The audio is not loaded*; one with no length says
  so too.

### Waveform colour

The waveform follows **Settings › Waveform colour** (the setting the Arrangement's waveforms
follow):

| Setting | The editor's waveform |
|---|---|
| Theme (the default) | The theme's `clip-waveform` `color` where a theme gives one; else the clip's own colour (its track's) |
| Auto | The clip's own colour |
| Light, Dark, Custom | That ink |

So a theme that finds the clip's colour clashes sets `clip-waveform { color: … }` once, for the
Arrangement's clips and for this view; with no rule the colour is the clip's.

## Warp markers

Markers can be edited while the clip is warped (Warp on, two markers or more). Each edit is one
command and one undo step, and the clip is heard with it at once.

| Gesture | Effect | Command |
|---|---|---|
| Double-click on the waveform | Pins a marker at that beat (on the grid; Alt: free), where the sound is: nothing moves yet | `clip.addWarpMarker` |
| Drag a marker | Moves the marker, on the grid (Alt: free), and the sound pinned to it with it, so what plays between it and its neighbours stretches or squeezes. The waveform is drawn moving while it is dragged and the ruler says the beat it is at | `clip.moveWarpMarker` |
| Double-click a marker | Takes it away; the audio between its neighbours then runs evenly. A warped clip keeps two | `clip.removeWarpMarker` |

A marker never passes its neighbours (it stops a 64th of a beat short of them).

**The end of the audio.** Moving a marker moves where the audio ends in beats (dragging the last
one later stretches the whole file longer). The clip keeps its start and length, and the end its
right edge can be drawn out to in the Arrangement (`clip.trim`) follows the audio's end: a clip
whose audio was stretched past it is drawn out over the rest with a drag, without setting its length
first. A reset follows it the same way. The audio of a clip that other clips play too is copied for
it first, so theirs keeps its end.

To put a snare
on a beat: zoom in until the grid has lines near it, double-click on the snare's start to pin it,
and drag the marker to the beat.

### Reset warp

The **Reset…** button in the clip panel's Warp part (*Reset warp…* to a screen reader) opens a dialog that says how many markers the clip has and
asks:

- **Keep warp markers**: closes it and changes nothing (so do Escape and a click outside it);
- **Reset to clip tempo**: replaces the markers with two, one at each end of the audio, at the
  tempo the clip has (the project's for a clip that was not warped), so the audio runs evenly again
  (`clip.resetWarp`);
- **Find tempo again**: the same, at the tempo found in the file's metadata, then its name, then
  its sound (`clip.resetWarp` with `detect`); with none found it changes nothing and says so.

Either reset turns warp on, keeps the clip's start, length and loop where they are, and is one undo
step.

## Zoom and scroll

| Gesture | Effect |
|---|---|
| Opening a clip | The file is fitted to the area, with 8 points left at each end |
| Ctrl and the wheel (or a pinch) | Zoom, the beat under the pointer staying under it |
| The wheel | Scroll along the file |
| A drag on the waveform (not on a marker) | Pan |
| **Fit** in the clip panel's Warp part, or a double-click on the ruler | Fit the file again |
| A drag on the ruler | Up zooms in and down zooms out, the beat where the drag began staying put; sideways scrolls. The pointer is a magnifier over the ruler and hidden while the button is held, as on the piano roll's ruler |

Zoom stays between 0.05 and 20 000 points per beat, scroll between the file's two ends. Another
clip starts fitted. The zoom and scroll are the view's and are not saved with the project.

## Later

- A playhead, and a click on the ruler to move it.
- Warp markers snapped to the file's transients, and Alt-drag to move a marker's sound without
  its neighbours stretching.
- Vertical zoom of the waveform.
