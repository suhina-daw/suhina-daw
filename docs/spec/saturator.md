---
description: The built-in saturator - its parameters, its eight curves as formulas, the Color filters and their inverse, the post clip, the low-latency oversampling, how Live's Saturator imports into it, and how close it comes to Live's renders
read_when: saturator, saturation, waveshaper, distortion curves, analog clip, soft sine, bass shaper, sinoid fold, digital clip, post clip, color filters, drive, importing Live's Saturator, Saturator elements, IIR oversampler, half-band
type: spec
status: planned
last_verified: 2026-11-01
covers:
  - devices/saturator/**
  - crates/dsp/src/shape.rs
  - crates/dsp/src/iir_oversample.rs
  - crates/dsp/src/emphasis.rs
---

# Saturator

Suhina's `saturator` (`devices/saturator`): a waveshaper with eight curves, a drive before the
curve, Color filters that choose which band is driven hardest, a post clip, an output level and
a dry/wet mix. It is designed so that Live's Saturator imports into it parameter for parameter
([`device-import.md`](device-import.md)).

- **Observed with:** Live 12.4.6 and 12.4.7, 44.1 kHz, and again at 48 kHz for the fold-back lines (below). The parameters (ranges, defaults, steps,
  stored elements) come from the parameter dump and a saved set; the sound from 13 behaviour
  runs (87 cases, among them the curve ramps, Color, the post clip and the Waveshaper), rendered
  in Live and analysed by the measurement harness of the Suhina device notes
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
input ─► DC filter (when on) ─► ┌ at four times the sample rate ────────────────────────────────┐ ─► post clip ─► Output ─► Dry/Wet ─► out
                                │ Color filters ─► Drive ─► curve ─► inverse Color filters      │
                                └───────────────────────────────────────────────────────────────┘
input (delayed by the latency) ─────────────────────────────────────────────────────────────────────────────────────────► dry ─┘
```

- **Curves.** `u` is the signal after Drive. Every curve but the Waveshaper is odd
  (`f(−u) = −f(u)`), so it adds odd harmonics only. With `k(u, t)`, a *knee clip*: `u` up to `t`, then
  `1 − (1 − t)·e^(−(|u| − t)/(1 − t))` with the sign of `u` (slope 1 at the knee, so no corner,
  and approaching ±1):

  | Curve | Formula | Slope at 0 |
  |---|---|---|
  | Analog Clip | `k(u, 0.73)`: straight to −2.7 dB, 0.90 at `u` = 1 | 1 |
  | Soft Sine | `sin(π/2 · u)` up to ±1, then ±1 | π/2 (+3.9 dB) |
  | Bass Shaper | `k(u, t)` with `t` the Bass Threshold as a gain: −50 dB bends from the start (`1 − e^−u`), 0 dB is a hard clip | 1 |
  | Medium Curve | `q(2u)`, a quadratic clip: `w` up to ⅔, then `w − (w − ⅔)² / (4/3)` up to ±1 at `w` = 4/3, then ±1 | 2 (+6 dB) |
  | Hard Curve | `atan(10u) / atan(10)`: 1 at `u` = 1, still rising slowly beyond | 6.8 (+16.6 dB) |
  | Sinoid Fold | `sin(π/2 · u)` with `u` held to ±9: it folds back (0 at `u` = 2, −1 at 3) up to its fifth peak and stays there | π/2 |
  | Digital Clip | `u` held to ±1 | 1 |
  | Waveshaper | see below | depends |

- **Waveshaper.** Six amounts, each 0 – 100 %, fitted to the measured gain and distortion of
  the original's staircase (`saturator-waveshaper` and `-more` in the device specs). With `x`
  the driven signal held to ±10: Damp first takes the slope at the origin down to `f` of its
  size and gives it back with level, `x · (f + (1 − f) · x⁴ / (x⁴ + w⁴))` with
  `f = 1 / (1 + 64.2 · damp)` and `w = 0.156 + 0.216 · damp − 0.096 · damp²` (half way back at
  0.204, 0.240 and 0.264 for 25, 50 and 75 %). Then
  `linear · x + sin(2π · curve · max(x², 0.0049 · |x|)) + depth² / 2 · sin((π + 94.1 · period) · x)`:
  Linear is the amplitude gain of the straight part (0 silent, 50 % −6 dB, 100 % unity); Curve is
  an even term, so it adds even harmonics (the second of a small tone is `2π · curve · A` of the
  fundamental) and an offset; it folds back once it reaches 1, and below −46 dBFS it turns into
  `|x|`, so the second harmonic stays at −31.7 dB (Curve 100 %) however quiet the input; Depth is
  the ripple's height (its square, up to 0.5) and Period its frequency, from one period over ±1
  to thirty. Shaper Drive blends the straight line `x` (0 %) with this curve (100 %). The offset
  is taken out after the curve by a one-pole high-pass at 5 Hz (at four times the sample rate), as
  the original's output has none; it runs only while the Waveshaper plays.
- **Color.** Two peaking filters (the cookbook bell) run before Drive. Color Low: ±100 % =
  ±36 dB at 41.4 Hz with Q 0.103, so wide that above 100 Hz it falls about 6 dB per octave like
  a shelf, is gone by 5 kHz, and falls again below its centre. Color High: ±100 % = ±24 dB at
  Color Frequency, with Q `2.04 − 1.74 · width` (2.04 at 0 %, 1.52 at 30 %, 0.30 at 100 %).
  After the curve their exact inverses run (`dsp::Emphasis`: each filter and its inverse in
  direct form I, sharing one set of coefficients, the inverse solving the filter's equation for
  its input), so while the curve stays straight the device is flat whatever the Color settings,
  sample for sample even while they glide; only how hard each band is driven changes. Color off
  sets both amounts to 0; back at 0 the filters run on until what they held has died away, and
  only then are they skipped (Decisions 10).
- **Post clip.** At the base rate, after the downsampler, so it also holds the downsampler's
  overshoot (the original's hard clip peaks at exactly 1). Soft: straight to 0.6, then
  `0.6 + 0.4 · tanh((|y| − 0.6) / 0.4)` towards ±1 (slope 1 at the knee); Hard: held to ±1.
  Either way the ceiling is the Output level, since Output follows.
- **Output and Dry/Wet.** Output is a gain after the curve. Dry/Wet is linear,
  `dry × (1 − mix) + wet × mix`; the dry signal is the input, untouched by Drive and Output.
- **DC filter.** Two identical one-pole high-passes at 27 Hz in series (magnitude
  `(f / 27)² / (1 + (f / 27)²)`, the original's step response and sweep) before everything.
- **Oversampling and latency.** The curve and the Color filters (not the post clip) run at four
  times the sample rate, as the original's Hi-Quality does: two stages of polyphase IIR half-band
  filters (`dsp::IirOversampler4x`, two `dsp::IirOversampler2x` in series, each four allpasses
  of an elliptic design). The round trip delays the audio band by 3.29 frames and up to 7.5
  frames more near 20 kHz (at 44.1 kHz); three samples of delay at the high rate bring it to
  4.04. The device reports 4 frames of latency and delays the dry signal by the same, so a
  dry/wet blend lines up.
- **Changes while playing.** Drive, Output, Dry/Wet, the Bass Threshold, the Waveshaper amounts
  and the DC filter switch ramp over 20 ms; the Color amounts, frequency and width glide over
  20 ms with the filters retuned every 16 frames; a change of Type crossfades from the old curve
  to the new one over 20 ms, and so does a change of Post Clip. A change that arrives while a
  crossfade still runs waits for it, so clicking quickly through the curve list morphs in turn
  instead of jumping (Decisions 8). Whichever of the two changes, a new crossfade starts from the
  sound that is playing — a post clip change after a curve change has finished does not replay the
  curve that was faded from.

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
(the plain value `Oversampling`) is not read: Suhina always oversamples four times, as the
original does with it on, its default (Decisions 3).

## Decisions

Each entry: what was chosen, what it costs, why.

1. **Closed-form curves fitted to the measured transfer curves.** Analog Clip and Bass Shaper
   share the knee clip; the knee at −2.7 dB matches the original's ramp (`saturator-curves-ramp`)
   to 0.01 dB, and at a threshold of −50 dB the same formula is the measured `1 − e^−u` of the Bass
   Shaper. Medium Curve's quadratic clip and Hard Curve's arctangent match their ramps at Drive 0
   and +18 dB to 0.04 dB and 0.4 points of THD; the knee clip with twice the gain (the earlier
   Medium Curve) and a power sigmoid (the earlier Hard Curve) were 1.1 and 1.7 points of THD off
   on the staircase. Cost: one `exp` per sample above the knee, one `atan` for Hard Curve.
2. **The Bass Shaper's threshold is the knee of the same curve.** Only −50 dB was measured; the
   manual's "soft clip at −50 dB, hard clip at 0 dB" is what the knee clip does at its two ends.
   Candidate run: Measurement plan 2 of the note.
3. **Always oversampled four times, by two IIR half-bands of four allpasses each.** The
   original's Hi-Quality, on when the device is added, is 4x: the lines a 10 and a 15 kHz sine
   fold back to through Digital Clip at Drive +24 dB (`saturator-hiquality-types`) match an
   ideal 4x model to 0.1 dB, and Suhina's to 0.4 dB (Analog Clip, Digital Clip; the strongest
   lines of the Waveshaper and Sinoid Fold too). Recorded again at 48 kHz (`saturator-hiquality-types-48k`),
   the strongest lines still match to 0.5 dB (10 kHz: 22, 2 and 18 kHz at −33, −37 and −42 dB; 15 kHz:
   3 kHz at −27 dB). The weaker ones (10 kHz: 6 and 14 kHz near −50 dB; 15 kHz: 9 and 21 kHz near
   −49 and −53 dB) differ by 2 to 7 dB, and are not a fault of the filters: at exactly 10 or
   15 kHz the clipped sine repeats on the oversampled grid, so many harmonics fold onto one bin
   and their sum depends on where the sine sits against the grid, to a fraction of a sample.
   An ideal 4x model moves these lines by up to 8 dB over a 0.05-frame shift, and averaged over
   that shift Suhina's lines are within 0.3 dB of the ideal model's. They are therefore not in
   the test. At 2x, the earlier design, the 10 kHz sine
   had lines 15 dB louder (18.2 kHz at −18 dB, 1.8 kHz at −21 dB), heard as chirps falling
   against the top of a sweep. The phase matches the measured one: 0.14, 0.65, 2.0 and 7.5
   frames more delay at 5, 10, 15 and 20 kHz against the measured 0.1, 0.6, 2.0 and 7.9 (2x:
   0.13, 0.6, 1.8, 7.2). A linear-phase FIR (`dsp::Oversampler2x`) was turned down: 31 frames of
   latency the original does not have, a pre-ring the latency check catches on a full-band path,
   and a dry/wet blend 2 dB off at 16 kHz. Steeper elliptic designs (10 allpasses, 110 dB) were
   turned down too: more dispersion than measured. The wet path is delayed three samples at the
   high rate to a whole number of frames; without it the 0.29 frames left over against the dry
   signal put Dry/Wet 50 % 1.3 dB under the original at 16 kHz. Cost: about twice the CPU of
   2x (Cost) and 4 frames of latency instead of 2. There is no Hi-Quality switch: its off state
   (no oversampling, the 10 kHz sine's strongest line at −9.7 dB) is measured but not offered.
4. **Color Low is a very wide bell, not a shelf.** The measured boost of +100 % (36 dB stored)
   fits neither a first-order nor a second-order shelf to within the tolerance (first-order
   shelves of 36 to 38.4 dB and second-order ones of Q 0.33 to 0.45 were 1.2 to 3.1 dB off); a
   cookbook peaking filter of 36 dB at 41.4 Hz with Q 0.103 follows the sweep through the clip to
   0.4 dB and the noise response to 0.1 dB. Below about 40 Hz it falls again, where a shelf would
   not; the sweep there agrees with it.
5. **Color Width maps to Q by `2.04 − 1.74 · width`.** Fitted to the three measured widths (0 %,
   30 %, 100 %) with the cookbook peaking filter; within 0.2 dB of the sweep in every Color High
   case. The earlier law in octaves (`0.71 + 2.89 · width²`) left Color High +100 % 1.9 to 7.5
   points of THD off at 1 kHz.
6. **The Waveshaper is fitted, not derived.** The formula above reproduces the measured gain of
   the 21 staircase cases within 0.1 dB (Damp included), the
   second harmonic of Curve to 2 % up to −12 dBFS and its floor below −46 dBFS (the original's
   second harmonic stops falling with level there, and its fourth sits 14 dB under it, as a
   rectified sine's does, so the floor is `|x|`; a smooth blend of `x²` and `|x|` misses the
   flat floor by up to 1.4 points). The shape of the ripple and the dead zone between the
   measured points is a guess: Depth and Period were measured at two Period values (0 and 50 %),
   so Period's law is a straight line through those; Damp's width follows three settings.
   Whether the original removes the offset in the Waveshaper or elsewhere was not read; the note
   finds none at the output.
7. **Post clip before Output, at the base rate, with a measured soft curve.** The manual says
   the output never exceeds the Output level; the order was not measured (candidate run:
   Measurement plan 6). At twice the rate, with the Analog Clip curve for Soft, Hard was 1.6 dB
   off in response and 2 points in THD and Soft 1.0 dB and 1 point: the downsampler's overshoot
   passed the hard clip, which the original's never does, and the original's soft clip starts at
   0.6. At the base rate the clip aliases a little more; the original's renders show the same.
8. **A change of Type crossfades over 20 ms.** Switching a curve abruptly clicks; the original's
   behaviour was not measured (it needs automation during a render). A change that arrives while
   a crossfade runs waits for it: letting it restart the fade jumped between the two curves (a
   pop on every quick click through the list, heard in testing). The post clip's switch fades
   the same way instead of switching at the next block. Every new crossfade starts from the
   sound that is playing, whichever of Type or Post Clip changed: a post clip change after a
   curve change has finished would otherwise replay the curve that was faded from.
9. **The driven signal is held to ±10 for the Waveshaper and ±9 for Sinoid Fold.** At Drive
   +24 dB the original's Waveshaper falls from +18 dB of gain to +15.6 dB between −5 and 0 dBFS
   and its Sinoid Fold stops folding (`saturator-hiquality-types`); a hard limit at those values
   matches gain to 0.1 and 0.3 dB and THD to 2 and 0.4 points at every level, a `tanh` limit
   misses by 1 to 2 dB. The other curves are flat that far out (Hard Curve rises 0.02 dB more
   without the limit), so they are left alone. It also keeps the fold and the ripple bounded
   however hard they are driven.
10. **The Color filters and their inverses share their history (direct form I).** Two biquads of
   opposite gain cancel only while their coefficients hold still: each retune of a glide left a
   little of the filter's state uncancelled, which grew to −31 dB under a 100 Hz tone through
   Color Low's 20 ms glide to +100 % and then vanished within a frame when the glide ended, a
   click with the curve straight. Solved for its input, the inverse is exact at every sample.
   Switching the filters off the moment Color Low reached 0 clicked too: the wide 41 Hz bell
   still rang, so what reached the curve jumped (1.4 times the steady step through Digital
   Clip); they now run until both filters have settled to within −140 dB. Cost: direct form I
   takes a few more operations than the transposed form (2.7 % against 2.3 % of a core with
   both amounts on).

## Known differences

Every case is rendered through the device at 44.1 kHz from the probe signal with the values the
original's runs stored (Drive and Output sit on a 1/64 grid of their range, so "+12 dB" is
12.375 dB; rendering the nominal values instead leaves nine cases 0.37 dB off). The counts
below were taken with 2x oversampling; since the change to 4x only the folded-back lines
(Decisions 3) and Dry/Wet 50 % have been measured again.

`compare.py` (the harness's comparison, default tolerances 0.5 dB, 10 %, 0.5 ms) on the 22 cases
of the first three runs: **1 of 22** within tolerance. What keeps the others out:

- **Impulse readings, every case:** RT60 around 2 ms against 2 ms (more than 10 % apart) and the
  level of the first impulse taps 0.5 to 10 dB off. A full-band impulse lands between samples
  differently through the two oversamplers, and through Color High the later taps are small
  differences of large ones; both ring for about 2 ms above 20 kHz. Not audible; left as is.
- **FM depth** in Color High +100 % at Width 0 and the linear check: the reading of a sweep
  through a clip, hundreds of cents in both renders.
- **Steady-state readings:** below.

Every recorded case of the device specs' 13 runs on the main probe (49), judged by static gain
(0.5 dB), THD (1 point) and response (0.5 dB) alone: **45 of 49** within tolerance (26 before
the curves, Color, post clip and Waveshaper were refitted). Outside it:

- **Color Low −100 %** (two cases): the sweep matches to 0.03 dB at every band, but the noise
  response is up to 1.2 dB off below 130 Hz, with the sign alternating from band to band. The
  original's own readings jump by 6 to 8 dB between neighbouring bands there: the bands hold
  the clip's intermodulation of the noise, which no two implementations share. Left as is.
- **Linear check** (Analog Clip, Drive −23.6 dB, both Color amounts +100 %): within 0.03 dB and
  0.1 points except at the −9 dBFS step, where the curve starts to bend: 0.26 dB and 9.8 points
  of THD (250 against 240 %). The THD is magnified by the −24 dB inverse bell at 1 kHz, which
  cuts the fundamental and not its harmonics; each filter on its own is within tolerance.
- **Waveshaper Depth 50 % with Period 50 %:** gain within 0.06 dB at every step, THD 59 points
  off at −18 dBFS (841 against 782 %) and 23 at −21, where the fundamental all but cancels
  (THD over 1000 %); the noise response is 2.2 dB off at 28 Hz for the same reason. Depth 25 and
  75 % at Period 50 % behave the same on the extended probe (21 points at 361 %, gain within
  0.1 dB).
- **Dry/Wet 50 %** is within it: 0.32 dB under the original at 16 kHz and 0.26 dB at 18 kHz, the
  oversamplers' phase (Decisions 3; 0.71 dB at 2x).

The extended probe's readings (the staircase at 100 Hz, the ramp up and down, single tones, the
DC step) of the 38 cases of `saturator-curves-ramp`, `-hiquality`, `-hiquality-types`,
`-predc-step` and `-waveshaper-more`, which `compare.py` does not compare, judged the same way:
**36 of 38**. Outside it: Depth 75 % at Period 50 % (above), and Sinoid Fold with Hi-Quality off,
0.78 dB at the top of the ramp, where the original's fold aliases without oversampling and
Suhina's is always oversampled. The DC filter's step starts at 0.364 against 0.378 and decays
with 2.11 ms against 1.95 ms.

## Cost

One instance at its defaults: 1.2 % of one core at 48 kHz (60 s rendered in 0.70 s, release
build, Apple M-series), just over the 1 % budget, for the original's 4x oversampling
(Decisions 3; 0.5 % at 2x). With Hard Curve, both Color amounts and the soft post clip: 2.7 %. Run `cargo test --release -p suhina-device-saturator cost -- --ignored
--nocapture` to measure it.

## Tests

`devices/saturator/src/tests.rs`: the device contract (`device_api::conformance`, latency
included); the dry path delayed exactly; each curve's small-signal gain against the measured one;
Drive as a gain before the curve; the lines a hard clip folds back to, against the original's
with Hi-Quality; the Waveshaper's Linear as the straight path's gain and its
Curve as even harmonics with no offset; Analog Clip's knee and ceiling; the fold and the soft sine; the
Bass Threshold moving the knee; Color flat while the curve is straight and changing only the
saturation; the bell at its frequency; the post clip holding the Output level; Output and the
equal-gain mix; the DC filter; eleven changes while playing without a click (on a 97 Hz tone, so
that a clip's edges do not land on the same fraction of a frame every cycle); the Color glides
leaving a straight curve's sound untouched, sample for sample; two quick curve
changes morphing in turn instead of jumping; the post clip's switch fading; mixed `process` call
sizes leaving the sound bit-identical; the same sound at 44.1 and 96 kHz; silence in, silence out.

`devices/saturator/src/golden_tests.rs`: golden renders of thirteen scenarios (the defaults,
every curve, the waveshaper, Color, both post clips, changes while playing), compared within
1e-6 with the files in `devices/saturator/tests/golden`; its README lists them and how to make
them again when a change to the sound is intended.

`crates/dsp/src/shape.rs`,
`crates/dsp/src/iir_oversample.rs` and `crates/dsp/src/emphasis.rs` test the curves, the
oversamplers (2x and 4x: flat to 0.02 and 0.04 dB, their latency, images removed, 4x folding back
far less of a hard clip than 2x, broken input) and the Color filters' pairs (the inverse exact
while the filter is retuned, where two biquads of opposite gain are not; transparent only once
settled). `crates/als/src/devices_tests.rs` tests the import.
