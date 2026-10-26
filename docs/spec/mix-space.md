---
description: The Mix Space device and the analysis behind it: what it draws (stereo position, level and frequency of every track in a 3D box), the overlaps it marks, its parameters, the engine's mix tap and the analysis thread
read_when: mix space, mix-space, 3D mix view, visualizer, spectrum, stereo field, overlap, masking, mix tap, MixSpaceAnalyzer, pre-fader, post-fader, analysis thread
type: spec
status: current
last_verified: 2026-10-26
covers:
  - crates/dsp/src/mix_space.rs
  - crates/engine/src/mix_tap.rs
  - crates/session/src/mix_space.rs
  - crates/ui/src/detail/mix_space_panel.rs
  - devices/mix-space/src/lib.rs
---

# Mix Space

Mix Space is a built-in device that changes nothing in the sound. Its panel shows where every
track that plays through the device's track sits in the mix, each track in its own colour in a
box:

- **Across, left to right:** where the track's sound sits in the stereo field, band by band.
- **Up:** how loud it is, from -72 dBFS at the floor to 0 dBFS at the ceiling. A zone 12 dB
  high above the ceiling shows what goes over it; a line's part over 0 dB is drawn red.
- **Into the depth:** frequency, from 20 Hz at the front to 20 kHz at the back.

By default every track is **blended** into one landscape, like a spectrogram in three
dimensions. Over a grid of the stereo field against the bands, each point is as high as all the
tracks' sound there together and coloured by the tracks that make it, each in proportion to its
share, so where two tracks meet their colours mix. A band spreads across the stereo field as a
soft bell centred on its position, as wide as the band is; quiet ground fades out towards the
floor and the loudest ground burns towards white. With **Draw** set to Lines, each track is one
line through its bands' positions instead, with a fainter line above it for the bands' falling
peaks.

Each track's spectrum is drawn as its envelope at the **Detail** setting, riding over the tips
of its harmonics so an instrument reads as one shape; pointing at a track's name in the legend
shows that track at every band and fades the others back.

The panel opens looking from the side, where the box reads as an ordinary spectrum analyser
with every track in its own colour; turning it shows the stereo field too.

On a track the device shows that track; on a group, the group and every track inside it; on
the main channel, every track. Returns are not inside a group; on the main channel they show as
lines of their own, so the reverb a send adds is seen where it sits.

## Overlaps

Two tracks overlap in a band when both are louder than -60 dBFS there, they sit within 0.3 of
each other in the stereo field (of 2 from left to right), and their levels are within the
**Overlap Range**. A white dot marks the overlap, brighter the closer the levels; hovering it
names the tracks, their levels, the frequency and the side. Where the levels are further apart
than the range, the quieter track is covered there and its line is dashed.

## Parameters

| Parameter | Values | What it does |
|---|---|---|
| Tap | Post-fader, Pre-fader | Post-fader shows the tracks as their meters do, after their devices, volume and pan. Pre-fader shows them after their devices but before their volume and pan, at a centred unity gain; a group shows the sum of its tracks before its own volume. |
| Muted Tracks | Dim, Hide | How a muted track shows. Post-fader a muted track is silent either way. |
| Overlaps | Off, Mark, Only Overlaps | Off by default; the blend already mixes colours where tracks meet. Only Overlaps fades every track so the overlaps stand out. |
| Overlap Range | 1 to 12 dB, 6 | How close two levels must be to overlap. A small range (about 3 dB) suits dense mixes, where every element overlaps somewhere. |
| Release | 0.05 to 2 s, 0.3 | How fast the levels fall. They rise in 10 ms. |
| Draw | Blend, Lines | How the tracks are drawn. |
| Floor | -120 to -12 dB, -72 | The level at the box's floor; quieter sound is not drawn. |
| Detail | 1/3, 1/6, 1/12 octave, Every Band | How finely each track's spectrum is drawn. |
| Tilt | 0 to 6 dB per octave, 0 | Lifts each band by its octaves above 1 kHz and lowers those below, so a mix with a natural falling balance reads level; 3 or 4.5 dB is usual. |
| Response | Fast, Slow | Slow rises and falls over about 3 s, so the view shows the tonal balance of a whole section rather than its moments. |

## The view

Dragging turns the box; Shift-dragging or dragging with the right button moves it; the wheel
zooms; a double-click resets it to Side. **Front** looks at the stereo field and the levels, **Side**
is a spectrum and the default, **Top** maps frequency against the stereo field, **3D** looks
from above at an angle. A view button turns the box there over a few frames.

## The analysis

Each band of a 1/24-octave grid from 20 Hz to 20 kHz (or up to just below half the sample
rate) is read from a Hann-windowed FFT of about 6 Hz per bin, every eighth of the window:

- **Level:** the louder channel's mean square, as dBFS of a sine's peak, so a full-scale sine
  reads 0 dB. Silence reads -120 dB.
- **Position:** the angle of the two channels' amplitudes, the exact inverse of the equal-power
  pan law, so a track panned 30 % right reads 0.3.
- **Correlation:** the channels' cross-spectrum over their amplitudes, from -1 to 1.

A band quieter than -96 dBFS keeps its last position and correlation.

## The mix tap and the thread

The engine's meters already gather each track's block. While the mix tap asks for a point
(`MixTap::set_points`), the audio thread also copies each track's block into one lock-free
queue, tagged with its point and meter slot; a block that does not fit is dropped and counted,
never waited for. Pre-fader blocks of a track without devices are the sound as mixed divided by
its gains, so a side whose gain is zero (fully panned away, the fader at its bottom, muted) is
silent there too.

The `suhina-mix-space` thread empties the queue every 8 ms and publishes all readings about 60
times a second. A view asks for its point every frame it is drawn; half a second after the
last request the thread turns the tap off, so nothing is copied while no view is open.

## Limits

- The levels are smoothed, so a side-chained bass and its kick can look as if they overlap
  when they take turns faster than the release.
- The depth of a sound in the room (reverb, distance) is not shown.
- The main channel's own devices (its limiter) are not shown; the lines are the tracks before
  the main channel.
