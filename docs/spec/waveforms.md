---
description: How waveforms are computed and drawn in clips, the browser's preview and the sample map's tooltips: peak levels with RMS, lanes per channel, the shape, samples close up, the dB scale, the frame budget
read_when: waveform, peaks, RMS, waveform drawing, zoom, samples, stereo lanes, dB scale, clip waveform, preview waveform, sample map tooltip, frame budget
type: spec
status: current
last_verified: 2026-10-13
covers:
  - crates/ui/src/kit/waveform.rs
  - crates/ui/src/kit/waveform/tests.rs
  - crates/ui/tests/waveforms.rs
---

# Waveforms

One renderer draws every waveform: audio clips in the Arrangement view, the browser's preview
strip and the tooltip of a point on the sample map. It is in `crates/ui/src/kit/waveform.rs`.

## What other editors do (public documentation)

- Audacity draws the peak envelope (lowest and highest sample per pixel column) with a lighter
  RMS band inside it, and the samples themselves, joined by a line with a dot on each, once
  zoomed in far enough. Its vertical scale is linear, linear-dB or log-dB.
- Ardour reads precomputed peak files rather than the audio, because reading the audio of a long
  file for every redraw is slow.

No existing crate under a GPL-compatible licence does both the summary and the drawing:
`waveform` (waveform-rs, MIT/Apache-2.0, last released 2017) bins min/max at several resolutions
but renders images and has no RMS; `audio-visualizer` plots for debugging. Computing peaks is a
few dozen lines; drawing them is specific to egui. Both are written here.

## Peaks

A file is summarised once, when it is decoded (the project's audio, a dropped file, the
browser's selected or hovered file), off the drawing thread:

- **Levels.** Buckets of 256, 2048 and 16384 frames (`LEVELS`), each eight of the one before.
  Every bucket holds, per lane, the lowest sample, the highest sample and the RMS level. A
  10-minute stereo file at 44.1 kHz needs about 2.9 MB.
- **Lanes.** A mono file has one; a file with two or more channels has two, left and right.
  Channels after the second are not drawn. A mixed reading combines both lanes.
- **Samples.** The decoded samples stay reachable from the peaks (shared with the audio pool,
  not copied), for drawing closer than the finest level. The browser's cache of files keeps the
  peaks without the samples, except for the selected file.
- Samples that are not numbers count as silence; an empty or channel-less file gives no columns.

## Reading a column

Each pixel column shows a stretch of the file (from a clip's mapping of beats to the file,
warps and loops included, or a steady zoom). The column reads:

- the **samples** themselves when the stretch is shorter than the finest level (256 frames);
- otherwise the **coarsest level** with at least one bucket in the stretch, so a column reads at
  most eight buckets per lane at any zoom.

The RMS of several buckets is the root of their mean square, weighted by frames.

## Drawing

- **Lanes.** A stereo file in a box at least 40 points tall (`STEREO_MIN_HEIGHT`) is two lanes,
  left above right, two points apart; shorter, one mixed lane.
- **Envelope.** Per column, from the highest to the lowest sample, at least a point tall so
  silence shows as a line. It is a filled strip between the columns' tops and bottoms (a
  triangle mesh) with its top and bottom edges drawn as smoothed lines, in the colour at 55 %.
  The RMS band, from −RMS to +RMS and never outside the envelope, is drawn over it in the colour
  itself. Columns outside the file break the strip.
- **Samples.** When a column covers less than one frame, the samples are joined by a line;
  samples six points apart or more get a dot each.
- **Gain.** A clip's gain and fades scale each column, as the engine plays them.
- **Scale.** Linear (full scale at the edges of a lane), or decibels (Settings › Appearance ›
  Waveforms › Waveform scale; `waveform_scale = linear|db` in `settings.txt`): full scale at
  the edges, 48 dB below it (`DB_RANGE`) and quieter in the middle, so quiet parts show. The
  setting applies to every waveform.
- **Colours.** A clip uses its track's colour (paler while muted or disabled); the browser's
  preview uses `text.secondary` for the part played and `text.hint` for the rest; the map's
  tooltip `text.secondary`.

## Where

- **Clips** (Arrangement view): inside the clip under its name, in view only.
- **Browser preview strip**: the whole file across the strip; a click previews from there
  ([browser spec](browser.md)).
- **Sample map**: the tooltip of a point the pointer rests on for 0.15 s shows the file's
  waveform, 220 × 36 points, once the browser's reader has decoded it
  ([sample map spec](sample-map.md)).

## Budget

Drawing a 10-minute stereo file 2000 points wide, at every zoom from the whole file to single
samples, in one or two lanes, in decibels, painted and tessellated, stays within a frame at 60
frames a second (16.7 ms); about 6 ms at worst in the dev profile on the machine it was measured
on, at 255 frames a column, where columns read the samples. Summarising the file takes about
0.3 s, once, off the drawing thread.

## Tests

- `src/kit/waveform/tests.rs`: levels and reading per lane, odd files, the dB scale, geometry at
  several zooms against text snapshots in `crates/ui/tests/golden/waveform-*.txt`
  (`SUHINA_BLESS=1` rewrites them), and the frame budget.
- `tests/waveforms.rs` (ignored, needs a GPU): renders the waveform at several zooms to
  `target/ui-snapshots/waveforms.png` for looking at.
