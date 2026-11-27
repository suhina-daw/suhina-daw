---
description: The built-in parametric EQ of up to 24 bands - its parameters, the band slots, the band types and their responses (tilts included), the 48 dB cuts' Q, Adaptive Q, Scale, each band's stereo placement, projects saved with two curves, the panel's band column, display, band list and expanded strip, the output spectrum's analyser, how Live's EQ Eight imports into it, and how close it comes to Live's renders
read_when: eq, equaliser, equalizer, parametric eq, 24 bands, band slots, placement, mid/side, tilt, bell, shelf, notch, cut, adaptive q, svf, EQ Eight import, Eq8, eq panel, band column, band list, expanded strip, analyser
type: spec
status: current
last_verified: 2026-11-03
covers:
  - devices/eq/**
  - crates/dsp/src/svf.rs
  - crates/ui/src/detail/eq_curve.rs
  - crates/model/src/eq_slots.rs
  - crates/dsp/src/band_dynamics.rs
  - crates/engine/src/spectrum.rs
---

# EQ

Suhina's `eq` (`devices/eq`): a parametric equaliser of up to 24 bands. The device has 24 band
slots; a slot in use is a band: a low cut or high cut (12 or 48 dB/octave), a low or high shelf, a
bell, a notch, a tilt shelf or a flat tilt, with its own frequency, gain and Q and its own stereo
placement (Stereo, Left, Right, Mid or Side), and it can be switched off without leaving its slot.
It is designed so that Live's EQ Eight imports into it parameter for parameter
([`device-import.md`](device-import.md)): its curve A lands on bands 1–8, its curve B on bands
9–16, and its mode on those bands' placements.

- **Observed with:** Live 12.4.6, 44.1 kHz. The parameters (ranges, defaults, steps, stored
  elements) come from the parameter dump and a saved set; the sound from four behaviour runs,
  `eq-eight-types`, `eq-eight-bell-q`, `eq-eight-cut-q` and `eq-eight-adaptive-q` (38 cases),
  rendered in Live and analysed by the measurement harness of the Suhina device notes
  (`live-12/audio-effects/29.15-eq-eight.md` in the device-specs repository, which lists the
  runs not made yet in `measurements/29.15-eq-eight/runs.py`).
- **Sources:** Live 12 Reference Manual, 29.15 EQ Eight (what each control is for); the
  measurements above (how it sounds); the Audio EQ Cookbook (R. Bristow-Johnson) for the
  responses and A. Simper's technical papers on the linear trapezoidal state-variable filter
  (Cytomic, 2013) for the structure. Everything else is this device's own design and is marked
  as a decision below.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable. Every band slot has fourteen
parameters, ids `a<n><Field>` for band n = 1 … 24 (group "Band n"), indices `(n − 1) × 14 + field`.
Bands 1–8 keep the ids the EQ always had, so a project, its automation and an import address them
unchanged.

| Field | Id (band 1) | Name | Range | Unit, scale |
|---|---|---|---|---|
| 0 | `a1On` | Band 1 On | off/on | toggle |
| 1 | `a1Type` | Band 1 Type | Low Cut 48, Low Cut 12, Low Shelf, Bell, Notch, High Shelf, High Cut 12, High Cut 48, Tilt Shelf, Flat Tilt | choice |
| 2 | `a1Frequency` | Band 1 Frequency | 10 Hz – 22 kHz | hertz, logarithmic |
| 3 | `a1Gain` | Band 1 Gain | −30 – +30 dB | decibels, linear |
| 4 | `a1Q` | Band 1 Q | 0.1 – 18, default 0.71 (1/√2) | none, logarithmic |
| 5 | `a1InUse` | Band 1 In Use | off/on | toggle |
| 6 | `a1Place` | Band 1 Placement | Stereo, Left, Right, Mid, Side | choice |
| 7 | `a1Mode` | Band 1 Mode | Static, Dynamic, Spectral | choice |
| 8 | `a1Range` | Band 1 Range | −30 – +30 dB, default −6 | decibels, linear |
| 9 | `a1Threshold` | Band 1 Threshold | −60 – 0 dB, default −24 | decibels, linear |
| 10 | `a1Attack` | Band 1 Attack | 0.5 ms – 200 ms, default 10 ms | seconds, logarithmic |
| 11 | `a1Release` | Band 1 Release | 10 ms – 2 s, default 120 ms | seconds, logarithmic |
| 12 | `a1Detail` | Band 1 Detail | 0 – 100 %, default 50 % | percent, linear |
| 13 | `a1Trigger` | Band 1 Trigger | Band, Key | choice |

The first eight types keep their indices, so a saved or imported type is the same type. Then:

| # | Id | Name | Range | Default | Unit, scale |
|---|---|---|---|---|---|
| 336 | `output` | Output | −12 – +12 dB | 0 dB | decibels, linear |
| 337 | `scale` | Scale | −200 – +200 % | 100 % | percent, linear |
| 338 | `adaptiveQ` | Adaptive Q | off/on | on | toggle |
| 339 | `audition` | Audition | 0 – 24 | 0 | whole number |

`audition` is never stored: `device.setParam` refuses it, a project and an import never hold it,
and only `device.audition` sets it, through the engine (*Audition* below).

The gain's range is twice EQ Eight's ±15 dB, so a band can boost or cut as far as the panel's
widest range shows; everything EQ Eight stores is inside it. Gain 0 dB, Q 0.71 and Stereo
everywhere, Adaptive Q on. Bands 1–8 are in use in a new EQ, with EQ Eight's defaults (its factory
preset, its curve A); bands 9–16 are not in use and hold EQ Eight's curve B defaults, so a project
saved with two curves renders the same once curve B lands there; bands 17–24 are not in use, on,
Bell, 1 kHz:

| Band | On, type, frequency | Band | On, type, frequency |
|---|---|---|---|
| 1 | on, Low Shelf, 30 Hz | 9 | off, Low Cut 12, 40 Hz |
| 2 | on, Bell, 200 Hz | 10 | off, Low Shelf, 200 Hz |
| 3 | on, Bell, 1 kHz | 11 | on, Bell, 100 Hz |
| 4 | on, High Shelf, 5 kHz | 12 | on, Bell, 500 Hz |
| 5 | off, Bell, 100 Hz | 13 | on, Bell, 2 kHz |
| 6 | off, Bell, 10 kHz | 14 | on, Bell, 10 kHz |
| 7 | off, Bell, 5 kHz | 15 | off, High Shelf, 5 kHz |
| 8 | off, High Cut 12, 18 kHz | 16 | off, High Cut 12, 18 kHz |

### Projects saved with two curves

Before the slots the EQ had eight bands in two curves, A (`a1…`–`a8…`) and B (`b1…`–`b8…`), and a
`mode` (Stereo, Left/Right, Mid/Side). Such a device's values move onto the slots when the project
is read (`suhina_model::eq_device_to_slots`, called by `crates/project-file`), and an EQ Eight
import goes the same way (`eq_two_curves_to_slots`):

- curve A stays bands 1–8; `b<n><Field>` becomes `a<n+8><Field>`, its automation lanes with it;
- Left/Right places bands 1–8 Left and 9–16 Right, Mid/Side places them Mid and Side, and in
  both bands 9–16 are in use; in Stereo bands 9–16 keep curve B's values but stay unused;
- `mode` is removed, and an automation lane of it is dropped (the mode was a plain value in EQ
  Eight, so an import never has one).

The legacy ids tell such a device apart, so no format version marks it; an EQ saved with slots
has none of them and is left as it is. The golden renders `left-right` and `mid-side` are the
renders of the two-curve EQ, unchanged.

## Behaviour

Per frame: the bands placed Stereo, Left or Right run first, in band order, on the left and right
(a Stereo band on both, a Left or Right band on its side); then, only while a Mid or Side band is
heard, the signal is encoded, mid `(L + R) / 2` and side `(L − R) / 2`, the Mid and Side bands run
in band order on their part, and it is decoded again (`L = M + S`, `R = M − S`); Output is a gain
at the end. Running the left/right bands before the mid/side ones is this device's decision: it
keeps a project saved in Stereo, Left/Right or Mid/Side sounding exactly as before, and skipping
the encoding while no mid or side band runs keeps the left/right path free of its rounding. A slot
not in use and a band that is off cost nothing; a Static band of a type with gain and no gain is
skipped too (a Dynamic or Spectral band runs, since its gain can move).

- **Responses.** Every band is a state-variable filter integrated with the trapezoidal rule
  (`dsp::Svf`), mixed to give exactly the cookbook response of its type at its frequency, gain
  and Q: the 12 dB cuts are the cookbook low-pass and high-pass (Q at the corner, 1/√2 a
  Butterworth), the shelves are half way in decibels at the frequency (their Q is mapped before it
  reaches the cookbook formula, see *Shelf Q* below), the bell's Q sets the
  width between its half-gain points and its centre is the set gain at every Q, the notch is
  `(p² + 1) / (p² + p/Q + 1)`. The bilinear transform bends the responses near Nyquist as the
  cookbook ones bend; a frequency above 0.49 of the sample rate is held there.
- **Shelf Q.** A shelf is the cookbook shelf, but the Q control does not enter it directly: the
  measured shelves (12 dB at 1 kHz, Q 0.1 to 18, and the same at 100 Hz, 10 kHz, ±12 dB, +3 dB and
  +15 dB) match a cookbook shelf whose Q is `0.1687 + 0.4281 · ln(1 + Q / 0.281)`, the same for
  low and high shelves and at every gain and frequency tried (1/√2 maps to 1/√2, Q 2 to 1.06,
  Q 6 to 1.50, Q 18 to 1.96). The cookbook shelf with the Q itself overshoots by up to 11 dB at
  Q 6. The constants are a fit to the 14 measured Qs (largest error 0.004 in Q); between them it
  is an interpolation.
- **48 dB cuts.** Four sections at the frequency with the Qs of an 8th-order Butterworth filter
  (0.510, 0.601, 0.900, 2.563), each multiplied by `1 + 0.263 · ln(Q / 0.7071)` (at least 0.1):
  a Butterworth at the default Q, and a resonant corner as Q rises.
- **Tilts** (a decision of this device's own, built from the same shelves). A **Tilt Shelf** is a
  low shelf of −gain/2 and a high shelf of +gain/2 at the band's frequency, with the shelves' Q
  mapping: the spectrum tips around the frequency by the whole gain. A **Flat Tilt** is two low
  shelves of −gain/4 at 3.75 and 1.25 octaves below the frequency and two high shelves of +gain/4
  at 1.25 and 3.75 octaves above, Q 0.4: −gain/2 far below, +gain/2 far above, and an even slope of
  about the same dB per octave over the ten octaves between. Its Q does nothing.
- **Scale** multiplies the gain of the types with gain (0 % flat, negative inverts); cuts and
  notches have no gain.
- **Adaptive Q** multiplies a bell's Q by `1.12^(|gain| − 6)` (gain after Scale): narrower above
  6 dB of boost or cut, wider below, the same for a cut as for a boost. Shelves are not touched.
- **Changes while playing.** Frequency (in octaves), gain (in dB), Q (as its logarithm), Scale
  and Output glide over 20 ms, and a band's filters are retuned every 16 frames while they move.
  Switching a band on or off, taking its slot into use or out of it, or changing its type or
  placement fades the band in or out over 10 ms (a type or placement change fades out, swaps,
  fades in). The filters' state carries through every retune, so a sweep does not click.
- **Latency:** none, Spectral bands included (*Dynamic and Spectral bands*).

### Dynamic and Spectral bands

A band's Mode is Static (the EQ as it always was), Dynamic or Spectral. Dynamic applies to the
types with gain (the shelves, the bell and the two tilts), Spectral to the bell and the shelves; a
band of another type runs Static whatever its Mode says. A project without these parameters loads
them at their defaults, Static, so it sounds as it did; an import sets none. The dynamics are one
block of `crates/dsp` (`band_dynamics::BandDynamics`, AGENTS.md rule 8):

- **The detector** hears the sound coming into the band, or with Trigger on Key the device's key
  (its sidechain, `docs/spec/sidechain.md`; the EQ is a keyed built-in, `KEYED_BUILTINS`), on the
  band's placement (both sides of a Stereo band, linked: the louder decides; a Mid or Side band
  hears the key's mid or side). A keyed band of an EQ with no key connected hears its own sound;
  a frame the key does not cover is silence, as the compressor reads it. Through its filter: a band pass at a bell's frequency and Q, a low pass under a
  low shelf's frequency, a high pass over a high shelf's, a broad band pass (Q 0.3) for a tilt.
- **The follower** follows its level in dB with the band's Attack and Release (the compressor's
  `AttackRelease`).
- **The gain change** is none below the threshold's knee, the whole Range 12 dB over the
  threshold, and a 6 dB soft knee around it: `Range · clamp(over / 12, 0, 1)`, `over` softened in
  the knee. Negative Range turns the band down as it gets louder, positive turns it up.
- **Dynamic:** the band's gain is its set gain plus the gain change, and its filter is retuned
  every 16 frames while the change moves by more than 0.01 dB. A Dynamic band with no gain of its
  own still runs, since its gain can move.
- **Spectral** (a decision of this device's own: minimum phase, no look-ahead, no latency): the
  band's own filter runs as it is, and up to eleven narrow parts spread over its range — a bell's
  width between its half-gain points, or the three octaves a shelf lifts, clamped to 10 Hz and
  0.45 of the rate — as many as fit 0.12 octave apart, each with its own detector
  (a band pass at the part, Q from Detail: `2 + 14 · Detail`) and their own gain change, which
  tunes a bell of that Q at the part. So only the parts whose level passes the threshold move: a
  resonance is cut and the rest of the band is left alone. Detail sets how narrow and selective
  the parts are.
- **Automation.** An EQ's lanes count toward the chain's 256 device lanes (`crates/engine`); a
  two-curve project's mode lane becomes 24 of them.
- **The live gain change** of every band (a Dynamic band's, or a Spectral band's deepest part)
  goes to the panel as the device's readouts (ADR 0032), one per band in band order.
- **Not yet:** an automatic threshold, a free trigger with its own range, the
  detector's tilt, a relative (soft) threshold for Spectral, and a linear-phase or look-ahead
  Spectral (issue #1169 keeps them).

### Audition

While a band is held to listen (the panel's listen button, or its dot while the Audition switch
is on), the EQ plays only the range that band works on, without its boost or cut: a band pass at
a bell's or notch's frequency and Q, at a shelf's or tilt's frequency with Q 0.71; for a low cut
what it removes (a low pass at its frequency), for a high cut a high pass. It runs on the band's
placement (a Left band's range on the left only, a Mid band's on the mid, and so on), fades in and
out over 10 ms, and follows the band while it is dragged. `device.audition` (track, device, band
1–24 or 0) sends the band to the playing engine as the device's `audition` parameter without
touching the project: nothing is saved or undone, a new graph plays the whole EQ again, and an EQ
inside a device group cannot be auditioned yet (the command says so). Without a player nothing is
heard.

## Import

Live's EQ Eight is the element `Eq8` ([`als-format.md`](als-format.md) › Devices).
`crates/als/src/devices.rs` maps it:

| Element | Stored | Suhina |
|---|---|---|
| `On` | true/false | the device's on/off |
| `GlobalGain/Manual` | dB | `output` |
| `Scale/Manual` | a fraction, 1 = 100 % | `scale` |
| `AdaptiveQ/Manual` | true/false | `adaptiveQ` |
| `Mode@Value` (plain, on the device) | 0 Stereo, 1 L/R, 2 M/S | the placements of bands 1–16 (*Projects saved with two curves*) |
| `Bands.n/ParameterA/…`, `Bands.n/ParameterB/…` (n = 0 … 7) | | band n + 1, band n + 9 |
| `…/IsOn/Manual` | true/false | `…On` |
| `…/Mode/Manual` | 0 … 7, in the order of the type list | `…Type` |
| `…/Freq/Manual`, `…/Gain/Manual`, `…/Q/Manual` | Hz, dB, Q | `…Frequency`, `…Gain`, `…Q` |

A type or mode outside its list is left at the default with a warning. `Precision` other than 0
(Live's oversampling option) is noted with a warning and not used (Decisions 3). The Edit
switch, Audition, the analyser's settings and `SelectedBand` are about the panel and are not
read; `AdaptiveQFactor` (1.12) and `AdaptiveQAffectsShelves` (false) are the constants above.

## The panel

The EQ's panel (ADR 0021 › Amendment: device panels for built-in devices) is a canvas 560 × 168 px
(`devices/eq/src/panel.rs`) and an expanded strip. Everything that follows the bands is one
host-drawn widget (`Widget::EqCurve`, `crates/ui/src/detail/eq_curve.rs`) that lays itself out
from the rectangle it is given; the globals are the SDK's own controls. The parameters stay the
device's: every edit is one `device.setParam` command, the same one a generated list writes, and
the edits of one gesture (a drag, a new band) are one undo step. The command's `device` is the
device's full address — the index on the track, or its path through device groups ("2/1/0") — so
an EQ inside a group is edited where it is; the panel's widget ids and its state are keyed to that
path too, so two devices never share either.

Measurements in canvas px, `[x, y, w, h]`:

- **The band column** `[2, 2, 92, 164]`, in rows (positions relative to it): the selected band's
  number in its colour `[2, 2, 22, 18]` (a click switches the band on or off — the number is its
  power button —, a right click lists the bands in use to pick another), previous and next
  `[50, 2, 19, 18]`, `[71, 2, 19, 18]`; the shape menu `[2, 22, 88, 18]` (the type's glyph and
  name); the placement menu `[2, 42, 44, 18]` (its name in its colour), listen `[48, 42, 20, 18]`
  (the listen mode's switch) and delete `[70, 42, 20, 18]`; the mode's menu `[2, 62, 64, 16]`
  (the mode written out, Static, Dynamic or Spectral, in the band's colour; the menu lists the
  three, a mode the type cannot take dimmed, then "Detector hears the sidechain"; picking Dynamic
  or Spectral shows the dynamics' dials) and the dials' switch `[68, 62, 22, 16]` (between the main
  dials and the dynamics' dials, for a Dynamic or Spectral band); then three dials, cells
  `[2, 80 + 28·i, 88, 27]`, each a dial at the left with its caption and value beside it (9.5 pt):
  Frequency, Gain and Q, or with the dynamics shown Range, Threshold and Attack (Detail for a
  Spectral band). Every dial drags like every value in a panel (Shift for precision, double-click
  resets); a click makes it the active value (#707's `value_keys`): the arrows step it, and a
  typed value is set with Enter — a frequency also as a note ("A4", "D#5 +13", "Bb3") or in "k"
  notation ("6k3" is 6.3 kHz), attack and release in ms, Detail in percent — and Escape or a click
  elsewhere lets it go. A dial whose band is off, the Gain of a type without gain, or a dynamics
  dial of a Static band is dimmed. With no band selected the column stays and reads "No band
  selected", so the display keeps its width.
- **The display** `[98, 2, 384, 164]`: frequency 10 Hz – 22 kHz logarithmically, gain ± the
  display's range (6, 12, 15 or 30 dB; 15 at first), a dB line at the range's steps, the decades
  and the lines between them, labels at 50 … 10k. The range button on its top right steps through
  the ranges, and a gain dragged or set past the range widens it. The curves:
  - the stereo bands' summed response in the accent colour;
  - for each placement with a band that is heard, the curve that side hears (the stereo bands and
    its own) in the placement's colour: Left blue, Right pink, Mid yellow, Side violet; the
    selected band's placement's curve is drawn stronger;
  - the selected band's own response, filled faintly from 0 dB.

  Every curve is the device's own filter sections (`band_sections`, `spectral_parts`) evaluated
  at 48 kHz: the drawing is the device's maths, never a second copy of it. Each heard band's
  response is computed once a frame and the curves are its sums, so more curves cost little. The
  curves are drawn only inside the curve's area: a curve past the display's range leaves it
  rather than lying along its edge.
- **A Dynamic or Spectral band's reach.** Whether or not sound plays, a faint curve in orange
  (violet for Spectral) shows the band's side with the whole range applied — a Dynamic band at its
  gain plus its range, a Spectral band with every part moved by its range — and the area between
  it and the curve is filled.
- **Dots.** One numbered dot per band in use, in the band's own colour (hues spread around the wheel
  slot by slot, `band_color`), at its frequency and gain
  (after Scale) — for the cuts and the notch on the 0 dB line; a band that is off has a dashed
  ring. Dragging moves the frequency, and the gain where the type has one (Shift for precision);
  dragging a band that is off turns it on. A click or a drag start selects the band; a
  double-click switches it on or off; the wheel over it sets its Q (12 % a step, 3 % with Shift);
  a right click opens its shape menu; Alt and the wheel set its dynamic range (0.5 dB a step),
  making a Static band Dynamic. Hovering or dragging a dot shows its values beside it: frequency,
  the nearest note, gain and Q (in the strip also the cents and the placement). A Dynamic or
  Spectral band's reach, live curve and handle are drawn in the band's colour.
- **Several bands.** A click on a dot selects that band alone; Ctrl (Cmd) or Shift with the click
  adds it to the selection or takes it out. A drag on the background draws a box, and the bands
  whose dots it holds become the selection (with Ctrl or Shift they join it). Dragging a selected
  dot moves every selected band by the same octaves and decibels (one undo step), and Delete
  removes them together. The column, the floating panel and the list follow the band clicked
  last.
- **Draw mode.** The pencil switch on the display makes a drag on its background draw a curve
  instead of a box. The curve is kept in 128 columns across the display, each holding the gain
  the stroke last passed through it, so moving back within a stroke redraws what it crosses. On
  release each stretch of the curve more than 1 dB from 0 dB (one sign) becomes a band in a free
  slot, finished where the curve comes back to 0 dB: in the middle a bell at the stretch's peak,
  its Q from the width between its half-gain points; from the curve's start near the left edge (the
  first 15 %) a low shelf at its half-gain point, or a 48 dB low cut once it falls past −18 dB; the
  same at the right edge a high shelf or cut. The whole stroke is one undo step, and the bands it
  made are selected. It is meant for a fast rough start, tuned afterwards.
- **The live curve.** While sound plays through a Dynamic or Spectral band, a curve in the band's
  colour draws its side with the bands as they are at that moment, from the device's readouts —
  drawn all the while, so it never blinks in and out: a Dynamic band at its gain plus its change;
  the first Spectral band that runs with every part's bell at its change (the readouts after the
  bands' carry its parts); another Spectral band with a bell at its deepest part (its readout
  carries that part's index in hundreds, `spectral_readout`).
- **The range handle.** A small arrow where a Dynamic or Spectral band's range ends (no line to
  it) is a handle: dragged up or down it sets the range (at the display's dB per point when the drag
  began, divided by Scale), one undo step.
- **Making and deleting bands.** A double-click on the display's background makes a band there,
  in the first slot not in use: at the far left (the first 6 %) a Low Cut 48, at the far right a
  High Cut 48, elsewhere a bell at the pointer's gain, on, Q 0.71, Stereo; the new band is
  selected. With all 24 slots in use nothing is made. The pointer over the background shows a
  ring where the band would go. The column's delete button, or Delete while the pointer is over
  the panel, frees the selected band's slot (its In Use goes off; the other bands keep their
  numbers, so automation stays with its band), and the selection moves to the next band. A click
  on the background clears the selection.
- **Switches on the display.** At its bottom left (in the strip at its top left), every 20 px: the
  piano keys (the frequency axis drawn as a keyboard along the display's bottom), the analyser (on
  at first; a right click opens its settings), draw mode, listen (while it is on, grabbing a band —
  its dot or one of its dials — plays that band alone until it is let go; the column's and the
  floating panel's listen buttons switch the same mode), then the settings (a gear). While a band is heard alone, the range it works on is shaded in its
  placement's colour.
- **The settings page.** The gear covers the display with a page of settings, panel state like the
  switches: the strip's floating panel Centered (the default) or Follows band, and the analyser's
  range, fall, tilt and freeze (the same as its right-click menu). Done goes back.
- **The placement menu** lists Stereo, Left, Right, Mid and Side in their colours, then Split into
  L/R and Split into M/S: the band takes Left (or Mid) and a copy of it in the first free slot
  takes Right (or Side), one undo step. With every slot in use the splits are dimmed.
- **The globals' column** at x 486, 60 wide: Adapt. Q `[486, 4, 60, 18]`, Scale's caption
  `[486, 28, 60, 14]` over its value `[486, 42, 60, 20]`, Output's `[486, 66, 60, 14]` over
  `[486, 80, 60, 20]`.
- **The output meter** `[550, 2, 8, 164]`: the peak of the device's output over the newest tenth
  of the analyser's ring, from −48 dB to 0 dB, green, yellow above −9.6 dB, red above −1.9 dB.
- **The expanded strip.** The header's expand button opens the expanded panel, which for the EQ is
  a strip over the device row (`strip: true`, canvas 1200 × 160), as Multiband's: it opens as tall
  as its canvas scaled to its width, its header drags its height up to the whole main area, and a
  double-click on the header fits it again. The widget lays itself out from the strip's own
  rectangle: the display over the whole strip (4 px in, with the output meter at its right edge),
  the frequency labels from 20 Hz to 20k, the dB labels at the right, and a readout of the
  frequency, note and gain under the pointer. From 340 pt tall the strip keeps a 100 px lane under
  the curve for the selected band's **floating panel** (428 × 84, in the middle of the display; it
  follows its band only when the settings say so): the number (on/off) and listen over the shape
  and placement menus `[8, 8 … 77]`, the Frequency, Gain and Q dials `[108, 176, 250]` (dial on
  top, caption and value under it), the mode's menu `[318, 8, 74, 21]` and the dynamics' switch
  ("Dynamics", "Dynamics · SC" while the detector hears the sidechain) `[318, 32, 74, 21]`, then
  delete, previous and next. With the dynamics shown it is 214 px wider: Range, Threshold, Attack
  and Release (Range, Threshold, Detail and Attack for a Spectral band), 50 px apart. The widget tells the strip from the compact panel by the layer it is
  drawn in (the strip's is the foreground).
- **The band list.** While the strip is open, the compact panel shows a list of the bands in use
  in place of the column and the display, `[2, 2, 480, 164]`, with a header row and rows of 20
  (seven visible; the wheel scrolls). Columns, `[x, w]`: # `[0, 32]`, Shape `[34, 132]`, Freq
  `[166, 84]`, Gain `[250, 72]`, Q `[322, 54]`, Stereo `[376, 52]`, Mode `[428, 52]`. A click on a
  row selects its band; its number switches it on or off; Shape and Stereo open their menus;
  Mode steps to the next mode the type takes; Freq, Gain and Q drag vertically and reset with a
  double-click.
- **Panel state.** The selected bands, the display's range, the switches, the analyser's settings,
  whether the dynamics are open and the list's scroll are panel state, kept per device while the app runs and shared by the compact
  panel and the strip: they are not parameters, they are not saved in a project, and undo does not
  take them back.
- **Not yet** (#1168, #1169): typed values for Scale and Output and in the band list, the
  analyser's input (pre) spectrum and an external one, stock presets (they need #1140's loader),
  and the Dynamic and Spectral bands' remaining settings listed above.

### The analyser

While sound plays through the device, the display also draws the spectrum of what the device's
**output** is at that moment — the sound as the EQ shaped it — as a filled grey area behind the
response curve, on the same frequency axis and across the display's height, so the area and the
curve tell one story: where the curve pulls the sound down, the area follows. With nothing
playing it holds the last spectrum; a bypassed EQ runs in no chain and draws no area.

- **The tap.** The chain pass on the audio thread mixes a watched device's output block to mono
  (left and right halved) and appends it to a ring of 8192 samples in the meter monitor
  (`crates/engine/src/spectrum.rs`), after the device has run and before the next one — on every
  path that runs the chain's devices, a bus's or a return's included: their chains never run the
  stages plug-ins share, and their watched devices' output is captured all the same. The rings
  are fixed tables of atomics, like the meters': the audio thread only stores, never allocates,
  locks or waits, and a device whose row nobody watches costs nothing. The watch rows are keyed
  by the device's index in the chain; a panel claims row `device index mod 16` while it is
  drawn, and reads the ring only while the watch still names its device (a claim outlives its
  panel until the next graph swap, so at most that row's mixdown goes on). When a row changes
  owner, it starts from nothing: the new owner's ring holds no sample and no count of the
  previous owner's, whose panel-claims and in-flight blocks cannot publish under the new
  owner's name (each block publishes under the watch generation its snapshot took, and a
  generation that is no longer current resets the count). When two chains of the same track
  play at once (clips in more than one column), whichever wrote last shows. A device inside a
  device group's branches is not tapped yet, and its panel claims no watch — its branch-local
  index would name a top-level device's row — so it draws only the response curve (issue #1050
  keys watches by the device's path, which lifts this).
- **The bands.** The UI reads the ring, applies a Hann window and an 8192-point FFT (rustfft,
  already in the tree), and folds the spectrum into 256 bands spaced logarithmically over
  10 Hz – 22 kHz, taking each band's loudest bin; a band narrower than a bin (the low end) takes
  the level between the two bins around its center, so the low end glides instead of repeating one
  bin as steps. The level is the sine's amplitude against full scale in dB. The
  line is drawn on the display's own vertical scale — full scale at the top edge, −60 dB at
  the bottom — not the gain grid: normal material sits far below a ±15 dB grid, so on the grid
  the line falls below the display and never shows (live monitoring at a normal level drew no
  line at all before the scale had its own range). The spectrum is drawn as a filled grey area,
  the theme's grey at less than half strength, from its boundary down to the display's bottom,
  behind the curve and the grid's labels. Each frame's fresh reading is the last 8192 samples
  (about 170 ms), drawn as a smooth curve through the bands' centers (Catmull-Rom); each band's shown level rises to it over about 20 ms and falls after it over
  about 1.2 s — the DSP crate's attack/release follower, the compressor's own block, driven
  once a frame — so the area flows with the music instead of flickering window to window. No
  peak hold. While the engine appends to the ring the panel asks for the next frame (the
  meters' repaints alone would hold steady with a steady level and freeze the area); when the
  engine stops, the area holds the last shown levels. The monitor a panel reads is its own
  session's: the frame carries the active session's meter monitor, and none while that session
  has no player, so a panel never reads the previous session's engine as its own.
- **Settings.** A right click on the analyser switch opens its settings, panel state like the
  rest: the range at the display's bottom (60, 90 or 120 dB below full scale; 60 at first), the
  fall (Fast 0.3 s, Medium 1.2 s, Slow 3 s), a tilt of 0, 3 or 4.5 dB per octave around 1 kHz (0 at
  first; a mix's falling top then reads flatter), and Freeze, which holds each band's highest
  reading until it is switched off. The input (pre) spectrum and another device's spectrum need a
  second tap in the engine and are not there yet.

## Decisions

Each entry: what was chosen, what it costs, why.

1. **A state-variable filter with cookbook mixes, not direct-form biquads.** The measured
   responses are the cookbook ones to 0.003 dB, and the SVF gives exactly those (the tests compare
   them to 1e-6 dB) while keeping a meaningful state when its coefficients change, so automation of
   frequency, gain or Q does not bump. Cost: about the same arithmetic as a biquad.
2. **The 48 dB cuts' Q multiplies all four Butterworth section Qs by `1 + 0.263 · ln(Q/0.7071)`.**
   Fitted from two measured Qs (2 and 6, both to 0.02 dB in the note); the factor at Q 18 and
   below 0.71 is an extrapolation. Candidate run: the 48 dB cuts at Q 0.1, 0.4, 12 and 18.
3. **No oversampling option.** Live's (off by default) lessens the bending near Nyquist; it was
   not measured and adds latency. An imported set that had it on keeps the default response with
   a warning. Candidate run: Measurement plan 7 of the note.
4. **Adaptive Q is `1.12^(|gain| − 6)` on bells only, from the gain after Scale.** Fitted to three
   measured gains (3, 6 and 12 dB, cuts as boosts); the shelves were measured untouched. Whether
   Scale acts before Adaptive Q, and the notch, were not measured.
5. **Mid/Side is `(L ± R) / 2` and `M ± S`.** A flat curve passes the sound unchanged; the
   original's normalisation was not measured (candidate run: Measurement plan 5).
6. **Type, placement and on/off changes fade; frequency, gain and Q glide over 20 ms.** Not
   measured (they need automation during a render). A bell whose Q falls fast swells a little
   while its resonance empties (half again the level for 20 ms from Q 6 to 0.3), with no step.
7. **The curve display is the device's own maths, drawn at 48 kHz; the analyser reads the
   device's output.** The display sums the same filter
   sections the device runs (`band_sections`), so what the panel shows is what the device
   renders and there is no second response implementation to drift from. Cost: about 6 000
   magnitude evaluations per curve per frame, on the UI thread, at the display's 256 points.
   The drawn bend near Nyquist is 48 kHz's wherever the project runs; only a corner above
   20 kHz could differ at another rate. The analyser taps the device's output, not its input:
   the line then shows what the EQ made of the sound, the same story the curve tells, and a
   boost or cut shows on the line the moment the dot moves. The other candidate — the input,
   what is being equalised — shows the problem rather than the effect. The tap is the meters'
   pattern: the mono mixdown and the ring stores are the audio thread's whole cost, only while
   a panel watches, and the FFT runs on the UI thread. Audition is its own section above: it
   works on an EQ on a track's own chain with a player, not yet inside a device group.

## Known differences

`compare.py` (the harness's comparison, default tolerances 0.5 dB, 10 %, 0.5 ms) on the 32 cases
of the four runs, rendered through the device at 44.1 kHz from the probe signal with each case's
stored values: **30 of 32** within tolerance. The two others are the notches at 1 kHz (Q 0.71 and
6): the steady 1 kHz tone's AM and FM depth, read from what is left of a tone the notch removes
(about −70 dB in both renders); not audible, left as is.

Two further runs measure the shelves' Q (Q 0.1 to 18; gains +3, +12, +15 and −12 dB; 100 Hz, 1 kHz
and 10 kHz; low and high). With the shelf Q mapping above the 37 cases agree with the
measured responses to within 0.25 dB (the cookbook Q itself was up to 11 dB off). The mapping is
a fit, so a Q between the measured ones is interpolated.

## Cost

Measured on the two-curve EQ, before the band slots (a baseline, not measured again yet): one
instance at its defaults (four bands on, none with gain): 0.36 % of one core at 48 kHz (60 s
rendered in 0.22 s, release build, Apple M-series), inside the 1 % budget. Sixteen 48 dB cuts in
the two-curve EQ's Left/Right (64 sections): 3.6 %; the `cost` test now runs 24 bands of 48 dB
cuts, half of them on the mid and side (96 sections), not measured again yet. Run `cargo test --release -p suhina-device-eq cost -- --ignored
--nocapture` to measure it.

## Tests

`devices/eq/src/tests.rs`: the device contract (`device_api::conformance`); every type against the
measured response table; the cut filters' Q against the measured peaks; Adaptive Q against the
measured half-gain points, shelves untouched; Scale; the defaults and every band off passing the
sound; each placement where it says; a slot not in use silent, and a new EQ using the first eight;
the tilts' shapes; thirteen changes while playing without a click; the same bell at four
sample rates; every band at its extremes stable, silence in, silence out.
`devices/eq/src/golden_tests.rs`: golden renders of seven scenarios, compared within 1e-6 with the
files in `devices/eq/tests/golden`, whose README lists them. `crates/dsp/src/svf.rs` tests the
filter against the cookbook biquads and under fast modulation; `crates/als/src/devices_tests.rs`
tests the import, and `crates/model/src/eq_slots.rs` the move of a two-curve EQ onto the slots.
`devices/eq/src/panel.rs` tests the layout and the strip; `crates/ui/tests/detail_eq_panel.rs` the
panel itself: the display, the dots and their drags, the column's switch, the type and placement
menus, a band made by a double-click and a band deleted, each through the commands it writes.
`crates/engine/src/spectrum.rs` tests the watch and the ring round trip; the panel's band
folding in `crates/ui/src/detail/eq_curve.rs` tests a tone against its band.
