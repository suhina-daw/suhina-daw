---
description: The built-in saturator - its parameters, its eight curves as formulas, the Color filters and their inverse, the post clip, the low-latency oversampling, how Live's Saturator imports into it, and how close it comes to Live's renders
read_when: saturator, saturation, waveshaper, distortion curves, analog clip, soft sine, bass shaper, sinoid fold, digital clip, post clip, color filters, drive, importing Live's Saturator, Saturator elements, IIR oversampler, half-band
type: spec
status: current
last_verified: 2026-10-23
covers:
  - devices/saturator/**
  - crates/dsp/src/shape.rs
  - crates/dsp/src/iir_oversample.rs
---

# Saturator

Suhina's `saturator` (`devices/saturator`): a waveshaper with eight curves, a drive before the
curve, Color filters that choose which band is driven hardest, a post clip, an output level and
a dry/wet mix. It is designed so that Live's Saturator imports into it parameter for parameter
([`device-import.md`](device-import.md)).

- **Observed with:** Live 12.4.6, 44.1 kHz. The parameters (ranges, defaults, steps, stored
  elements) come from the parameter dump and a saved set; the sound from three behaviour runs,
  `saturator-curves`, `saturator-drive-output-mix` and `saturator-color` (27 cases), rendered in
  Live and analysed by the measurement harness of the Suhina device notes
  (`live-12/audio-effects/29.34-saturator.md` in the device-specs repository, which lists the
  runs not made yet in `measurements/29.34-saturator/runs.py`).
- **Sources:** Live 12 Reference Manual, 29.34 Saturator (what each control is for); the
  measurements above (how it sounds); the half-band design of Valenzuela and Constantinides
  (1983). Everything else is this device's own design and is marked as a decision below.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable.

| # | Id | Name | Range | Default | Unit, scale |
|---|---|---|---|---|---|
| 0 | `drive` | Drive | −36 – +36 dB | 0 dB | decibels, linear |
| 1 | `type` | Type | Analog Clip, Soft Sine, Bass Shaper, Medium Curve, Hard Curve, Sinoid Fold, Digital Clip, Waveshaper | Analog Clip | choice |
| 2 | `threshold` | Bass Threshold | −50 – 0 dB | −50 dB | decibels, linear |
| 3 | `postClip` | Post Clip | Off, Soft, Hard | Off | choice |
| 4 | `output` | Output | −36 – 0 dB | 0 dB | decibels, linear |
| 5 | `mix` | Dry/Wet | 0 – 100 % | 100 % | percent, linear |
| 6 | `color` | Color | off/on | on | toggle |
| 7 | `colorLow` | Color Low | −100 – +100 % | 0 % | percent, linear |
| 8 | `colorHigh` | Color High | −100 – +100 % | 0 % | percent, linear |
| 9 | `colorFrequency` | Color Frequency | 30 Hz – 18.5 kHz | 1 kHz | hertz, logarithmic |
| 10 | `colorWidth` | Color Width | 0 – 100 % | 30 % | percent, linear |
| 11 | `shaperDrive` | Shaper Drive | 0 – 100 % | 100 % | percent, linear |
| 12 | `shaperCurve` | Shaper Curve | 0 – 100 % | 5 % | percent, linear |
| 13 | `shaperDepth` | Shaper Depth | 0 – 100 % | 0 % | percent, linear |
| 14 | `shaperLinear` | Shaper Linear | 0 – 100 % | 50 % | percent, linear |
| 15 | `shaperDamp` | Shaper Damp | 0 – 100 % | 0 % | percent, linear |
| 16 | `shaperPeriod` | Shaper Period | 0 – 100 % | 0 % | percent, linear |
| 17 | `dcFilter` | DC Filter | off/on | off | toggle |

Ranges, defaults and step lists are Live's; the defaults are what Live's Saturator holds when it
is added (its factory preset: Color on with both amounts at 0, Shaper Curve 5 %). The Color and
Waveshaper parameters are in the groups "Color" and "Waveshaper".

## Behaviour

Per side and frame:

```text
input ─► DC filter (when on) ─► ┌ at twice the sample rate ──────────────────────────────────────┐ ─► Output ─► Dry/Wet ─► out
                                │ Color filters ─► Drive ─► curve ─► inverse Color filters ─► post clip │
                                └────────────────────────────────────────────────────────────────┘
input (delayed by the latency) ──────────────────────────────────────────────────────────────────────────────► dry ─┘
```

- **Curves.** `u` is the signal after Drive. Every curve is odd (`f(−u) = −f(u)`), so it adds
  odd harmonics only. With `k(u, t)`, a *knee clip*: `u` up to `t`, then
  `1 − (1 − t)·e^(−(|u| − t)/(1 − t))` with the sign of `u` (slope 1 at the knee, so no corner,
  and approaching ±1):

  | Curve | Formula | Slope at 0 |
  |---|---|---|
  | Analog Clip | `k(u, 0.708)`: straight to −3 dB, 0.893 at `u` = 1 | 1 |
  | Soft Sine | `sin(π/2 · u)` up to ±1, then ±1 | π/2 (+3.9 dB) |
  | Bass Shaper | `k(u, t)` with `t` the Bass Threshold as a gain: −50 dB bends from the start (`1 − e^−u`), 0 dB is a hard clip | 1 |
  | Medium Curve | `k(2u, 0.708)` | 2 (+6 dB) |
  | Hard Curve | `6.7u / (1 + |6.7u|^1.75)^(1/1.75)` | 6.7 (+16.5 dB) |
  | Sinoid Fold | `sin(π/2 · u)` without a limit: it folds back, 0 at `u` = 2, −1 at 3 | π/2 |
  | Digital Clip | `u` held to ±1 | 1 |
  | Waveshaper | see below | depends |

- **Waveshaper.** Six amounts, each 0 – 100 %. Damp makes a dead zone `|u| ≤ 0.25 · damp`
  (silence for small signals); Linear sets the slope of the straight part, `0.5 + linear`
  (0.5 to 1.5, 1 at 50 %); Curve rounds it, `y − curve · y³ / (3(1 + y²))`, which stays rising for
  every setting and adds mostly the third harmonic; Depth adds a ripple
  `0.25 · depth · sin(π/2 · n · x)`, where Period sets `n` from 1 to 16. Shaper Drive blends the
  straight line `u` (0 %) with this curve (100 %).
- **Color.** A low shelf (Color Low, ±100 % = ±37 dB at the bottom, half way at 400 Hz, first
  order) and a bell (Color High, ±100 % = ±24 dB at Color Frequency; Color Width sets its width
  between its half-gain points, `0.71 + 2.89 · width²` octaves: 0.71 at 0 %, 0.97 at 30 %, 3.6
  at 100 %) run before Drive. After the curve the same filters with the opposite gains run, the
  exact inverse, so while the curve stays straight the device is flat whatever the Color
  settings; only how hard each band is driven changes. Color off sets both amounts to 0.
- **Post clip.** Soft: the Analog Clip curve; Hard: held to ±1. Either way the ceiling is the
  Output level, since Output follows.
- **Output and Dry/Wet.** Output is a gain after the curve. Dry/Wet is linear,
  `dry × (1 − mix) + wet × mix`; the dry signal is the input, untouched by Drive and Output.
- **DC filter.** A first-order high-pass at 5 Hz (`dsp::DcBlocker`) before everything.
- **Oversampling and latency.** The curve and the Color filters run at twice the sample rate
  between two polyphase IIR half-band filters (`dsp::IirOversampler2x`, four allpasses, an
  elliptic design). The round trip delays the audio band by 2.2 frames and up to 7 frames more
  near 20 kHz (at 44.1 kHz). The device reports 2 frames of latency and delays the dry signal by
  the same, so a dry/wet blend lines up.
- **Changes while playing.** Drive, Output, Dry/Wet, the Bass Threshold, the Waveshaper amounts
  and the DC filter switch ramp over 20 ms; the Color amounts, frequency and width glide over
  20 ms with the filters retuned every 16 frames; a change of Type crossfades from the old curve
  to the new one over 20 ms. Post Clip switches at the next block.

## Import

Live's Saturator is the element `Saturator`; every value is `<Element>/Manual@Value`
([`als-format.md`](als-format.md) › Devices). `crates/als/src/devices.rs` maps it:

| Element | Stored | Suhina |
|---|---|---|
| `On` | true/false | the device's on/off |
| `PreDrive` | dB | `drive` |
| `Type` | 0 … 7 | `type` |
| `BassShaperThreshold` | dB | `threshold` |
| `PostClip` | 0 … 2 | `postClip` |
| `PostDrive` | dB | `output` |
| `DryWet` | 0 … 1 | `mix` |
| `ColorOn`, `PreDcFilter` | true/false | `color`, `dcFilter` |
| `BaseDrive` | ±36 dB | `colorLow`, divided by 36 |
| `ColorDepth` | ±24 dB | `colorHigh`, divided by 24 |
| `ColorFrequency` | Hz | `colorFrequency` |
| `ColorWidth` | 0 … 1 | `colorWidth` |
| `WsDrive`, `WsCurve`, `WsDepth`, `WsLin`, `WsDamp`, `WsPeriod` | 0 … 1 | `shaperDrive`, `shaperCurve`, `shaperDepth`, `shaperLinear`, `shaperDamp`, `shaperPeriod` |

A stepped value outside its list is left at the default with a warning. Live's Hi-Quality option
(the plain value `Oversampling`) is not read: Suhina always oversamples (Decisions 3).

## Decisions

Each entry: what was chosen, what it costs, why.

1. **Closed-form curves fitted to the measured transfer curves.** Analog Clip, Bass Shaper and
   Medium Curve share the knee clip; the knee at −3 dB matches the measured Analog Clip to 0.08 dB
   at every measured point, and at a threshold of −50 dB the same formula is the measured
   `1 − e^−u` of the Bass Shaper. Hard Curve's power sigmoid matches the measured points to 0.17 dB
   (a `tanh` misses by 0.8 dB). Cost: one `exp` per sample above the knee, two `powf` for Hard
   Curve.
2. **The Bass Shaper's threshold is the knee of the same curve.** Only −50 dB was measured; the
   manual's "soft clip at −50 dB, hard clip at 0 dB" is what the knee clip does at its two ends.
   Candidate run: Measurement plan 2 of the note.
3. **Always oversampled, by an IIR half-band of four allpasses.** Its phase was chosen to match
   the measured one: 0.13, 0.6, 1.8 and 7.2 frames more delay at 5, 10, 15 and 20 kHz against the
   measured 0.1, 0.6, 2.0 and 7.9. A linear-phase FIR (`dsp::Oversampler2x`) was turned down: 31
   frames of latency the original does not have, a pre-ring the latency check catches on a
   full-band path, and a dry/wet blend 2 dB off at 16 kHz. Steeper elliptic designs (10 allpasses,
   110 dB) were turned down too: more dispersion than measured. Cost: images 38 dB down at 0.28 of
   the high rate and 64 dB from 0.30, so hard curves driven hard alias about as much as the
   original's renders show. There is no Hi-Quality switch; Live's off state was never measured.
4. **Color Low is a first-order shelf of 37 dB, half way at 400 Hz.** The measured boost of
   +100 % (36 dB stored) fits neither a first-order nor a second-order shelf to within the
   tolerance: first-order shelves of 36 to 38.4 dB and second-order ones of Q 0.33 to 0.45 were
   rendered and compared, and this one is the best balance between the sweep through the clip
   (1.8 dB off), the noise response (1.7 dB) and the linear check's staircase (1.1 dB). Candidate
   run: Color Low at ±50 % and at more sweep levels, to separate the filter from the clip.
5. **Color Width maps to octaves by `0.71 + 2.89 · width²`.** Fitted to the three measured widths
   (0 %, 30 %, 100 %); the bell itself is the cookbook peaking filter, whose width is between
   half-gain points, as measured (within 0.1 dB of the sweep in every Color High case).
6. **The Waveshaper is our own curve.** It was not measured; the manual says what each amount is
   for, and the formula above does that, stays rising for every Curve setting and is flat at its
   defaults. Candidate run: Measurement plan 3 of the note.
7. **Post clip before Output, the Analog Clip curve for Soft.** The manual says the output never
   exceeds the Output level; the order was not measured (candidate run: Measurement plan 6).
8. **A change of Type crossfades over 20 ms.** Switching a curve abruptly clicks; the original's
   behaviour was not measured (it needs automation during a render).

## Known differences

`compare.py` (the harness's comparison, default tolerances 0.5 dB, 10 %, 0.5 ms) on the 24 cases
of the three runs, rendered through the device at 44.1 kHz from the probe signal with each case's
stored values: **1 of 24** within tolerance (a bypass). In detail:

- **Impulse readings, every case:** RT60 around 2 ms against 2 ms (more than 10 % apart) and the
  level of the first impulse tap 0.5 to 1.6 dB off. A full-band impulse lands between samples
  differently through the two oversamplers; both ring for about 2 ms above 20 kHz. Not audible;
  left as is.
- **Steady-state readings** (response, static gain, THD): within tolerance in 14 of the 23
  non-bypass cases (every Drive and Output case, Analog Clip, Soft Sine, Bass Shaper, Sinoid Fold,
  Digital Clip, Color High −100 % and the Color default). Outside it: Color Low +100 % (sweep 1.8
  dB, noise 1.7 dB, THD at 1 kHz 4.9 points), Color Low −100 % (noise 1.7 to 3.1 dB below 125 Hz,
  where the note itself finds the original's readings uneven), the linear check (staircase 1.1 dB
  in the steps that clip), Color High +100 % (THD at 1 kHz 1.9 to 7.5 points at several hundred
  percent of THD), Hard Curve and Medium Curve (THD 1.7 and 1.1 points), Dry/Wet 50 % (0.53 dB at
  16 kHz, the oversamplers' phase). Decisions 3 and 4 say why each was left.

## Cost

One instance at its defaults: 0.35 % of one core at 48 kHz (60 s rendered in 0.21 s, release
build, Apple M-series), inside the 1 % budget. With Hard Curve, both Color amounts and the soft
post clip: 1.6 %. Run `cargo test --release -p suhina-device-saturator cost -- --ignored
--nocapture` to measure it.

## Tests

`devices/saturator/src/tests.rs`: the device contract (`device_api::conformance`, latency
included); the dry path delayed exactly; each curve's small-signal gain against the measured one;
Drive as a gain before the curve; Analog Clip's knee and ceiling; the fold and the soft sine; the
Bass Threshold moving the knee; Color flat while the curve is straight and changing only the
saturation; the bell at its frequency; the post clip holding the Output level; Output and the
equal-gain mix; the DC filter; eleven changes while playing without a click; the same sound at
44.1 and 96 kHz; silence in, silence out.

`devices/saturator/src/golden_tests.rs`: golden renders of thirteen scenarios (the defaults,
every curve, the waveshaper, Color, both post clips, changes while playing), compared within
1e-6 with the files in `devices/saturator/tests/golden`; its README lists them and how to make
them again when a change to the sound is intended.

`crates/dsp/src/shape.rs` and
`crates/dsp/src/iir_oversample.rs` test the curves and the oversampler (flat to 0.02 dB, its
latency, images removed, broken input). `crates/als/src/devices_tests.rs` tests the import.
