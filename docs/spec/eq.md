---
description: The built-in eight-band parametric EQ - its parameters, the band types and their responses, the 48 dB cuts' Q, Adaptive Q, Scale, the Stereo, Left/Right and Mid/Side curves, how Live's EQ Eight imports into it, and how close it comes to Live's renders
read_when: eq, equaliser, equalizer, parametric eq, eight-band eq, bell, shelf, notch, low cut, high cut, adaptive q, mid/side eq, state-variable filter, SVF, importing Live's EQ Eight, Eq8 elements
type: spec
status: current
last_verified: 2026-10-28
covers:
  - devices/eq/**
  - crates/dsp/src/svf.rs
---

# EQ

Suhina's `eq` (`devices/eq`): an eight-band parametric equaliser. Each band is a low cut or high
cut (12 or 48 dB/octave), a low or high shelf, a bell or a notch, with its own frequency, gain
and Q, and can be switched off. Every band exists twice, in curve A and curve B, for the Stereo,
Left/Right and Mid/Side modes. It is designed so that Live's EQ Eight imports into it parameter
for parameter ([`device-import.md`](device-import.md)).

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

Values are plain values in their unit (ADR 0004). Ids are stable. Every band has five parameters,
for curve A (`a1…`–`a8…`, group "Curve A") and curve B (`b1…`–`b8…`, group "Curve B"), indices
`(curve × 8 + band) × 5 + field`:

| Field | Id (band 1, curve A) | Name | Range | Unit, scale |
|---|---|---|---|---|
| 0 | `a1On` | Band 1 On | off/on | toggle |
| 1 | `a1Type` | Band 1 Type | Low Cut 48, Low Cut 12, Low Shelf, Bell, Notch, High Shelf, High Cut 12, High Cut 48 | choice |
| 2 | `a1Frequency` | Band 1 Frequency | 10 Hz – 22 kHz | hertz, logarithmic |
| 3 | `a1Gain` | Band 1 Gain | −15 – +15 dB | decibels, linear |
| 4 | `a1Q` | Band 1 Q | 0.1 – 18, default 0.71 (1/√2) | none, logarithmic |

Curve B's names end in " B" ("Band 1 On B"). Then:

| # | Id | Name | Range | Default | Unit, scale |
|---|---|---|---|---|---|
| 80 | `output` | Output | −12 – +12 dB | 0 dB | decibels, linear |
| 81 | `scale` | Scale | −200 – +200 % | 100 % | percent, linear |
| 82 | `adaptiveQ` | Adaptive Q | off/on | on | toggle |
| 83 | `mode` | Mode | Stereo, Left/Right, Mid/Side | Stereo | choice |

Ranges and step lists are Live's, and the defaults are what Live's EQ Eight holds when it is added
(its factory preset): gain 0 dB and Q 0.71 everywhere, Adaptive Q on, and these bands:

| Band | Curve A: on, type, frequency | Curve B: on, type, frequency |
|---|---|---|
| 1 | on, Low Shelf, 30 Hz | off, Low Cut 12, 40 Hz |
| 2 | on, Bell, 200 Hz | off, Low Shelf, 200 Hz |
| 3 | on, Bell, 1 kHz | on, Bell, 100 Hz |
| 4 | on, High Shelf, 5 kHz | on, Bell, 500 Hz |
| 5 | off, Bell, 100 Hz | on, Bell, 2 kHz |
| 6 | off, Bell, 10 kHz | on, Bell, 10 kHz |
| 7 | off, Bell, 5 kHz | off, High Shelf, 5 kHz |
| 8 | off, High Cut 12, 18 kHz | off, High Cut 12, 18 kHz |

## Behaviour

Per frame: in Mid/Side the input is encoded, mid `(L + R) / 2` and side `(L − R) / 2`; each side
of the signal runs its curve's bands in series, band 1 first; Mid/Side decodes again
(`L = M + S`, `R = M − S`); Output is a gain at the end. Stereo runs curve A on both sides,
Left/Right curve A on the left and B on the right, Mid/Side curve A on the mid and B on the side.
Bands that are off cost nothing; a shelf or bell with no gain is skipped too.

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
- **Scale** multiplies the gain of shelves and bells (0 % flat, negative inverts); cuts and
  notches have no gain.
- **Adaptive Q** multiplies a bell's Q by `1.12^(|gain| − 6)` (gain after Scale): narrower above
  6 dB of boost or cut, wider below, the same for a cut as for a boost. Shelves are not touched.
- **Changes while playing.** Frequency (in octaves), gain (in dB), Q (as its logarithm), Scale
  and Output glide over 20 ms, and a band's filters are retuned every 16 frames while they move.
  Switching a band on or off, or changing its type, fades the band in or out over 10 ms (a type
  change fades out, swaps, fades in). A change of mode dips the output for 10 ms, swaps and comes
  back. The filters' state carries through every retune, so a sweep does not click.
- **Latency:** none.

## Import

Live's EQ Eight is the element `Eq8` ([`als-format.md`](als-format.md) › Devices).
`crates/als/src/devices.rs` maps it:

| Element | Stored | Suhina |
|---|---|---|
| `On` | true/false | the device's on/off |
| `GlobalGain/Manual` | dB | `output` |
| `Scale/Manual` | a fraction, 1 = 100 % | `scale` |
| `AdaptiveQ/Manual` | true/false | `adaptiveQ` |
| `Mode@Value` (plain, on the device) | 0 Stereo, 1 L/R, 2 M/S | `mode` |
| `Bands.n/ParameterA/…`, `Bands.n/ParameterB/…` (n = 0 … 7) | | band n + 1 of curve A, curve B |
| `…/IsOn/Manual` | true/false | `…On` |
| `…/Mode/Manual` | 0 … 7, in the order of the type list | `…Type` |
| `…/Freq/Manual`, `…/Gain/Manual`, `…/Q/Manual` | Hz, dB, Q | `…Frequency`, `…Gain`, `…Q` |

A type or mode outside its list is left at the default with a warning. `Precision` other than 0
(Live's oversampling option) is noted with a warning and not used (Decisions 3). The Edit
switch, Audition, the analyser's settings and `SelectedBand` are about the panel and are not
read; `AdaptiveQFactor` (1.12) and `AdaptiveQAffectsShelves` (false) are the constants above.

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
6. **Type changes, on/off and mode changes fade; frequency, gain and Q glide over 20 ms.** Not
   measured (they need automation during a render). A bell whose Q falls fast swells a little
   while its resonance empties (half again the level for 20 ms from Q 6 to 0.3), with no step.
7. **Left for later:** the analyser, Audition, the curve display with its draggable dots and the
   Edit switch are panel parts (the device-specs AGENTS › Scope); the panel is the generated
   parameter list until a layout gives it a curve.

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

One instance at its defaults (four bands on, none with gain): 0.36 % of one core at 48 kHz (60 s
rendered in 0.22 s, release build, Apple M-series), inside the 1 % budget. Sixteen 48 dB cuts in
Left/Right (64 sections): 3.6 %. Run `cargo test --release -p suhina-device-eq cost -- --ignored
--nocapture` to measure it.

## Tests

`devices/eq/src/tests.rs`: the device contract (`device_api::conformance`); every type against the
measured response table; the cut filters' Q against the measured peaks; Adaptive Q against the
measured half-gain points, shelves untouched; Scale; the defaults and every band off passing the
sound; Left/Right and Mid/Side; ten changes while playing without a click; the same bell at four
sample rates; every band at its extremes stable, silence in, silence out.
`devices/eq/src/golden_tests.rs`: golden renders of six scenarios, compared within 1e-6 with the
files in `devices/eq/tests/golden`, whose README lists them. `crates/dsp/src/svf.rs` tests the
filter against the cookbook biquads and under fast modulation; `crates/als/src/devices_tests.rs`
tests the import.
