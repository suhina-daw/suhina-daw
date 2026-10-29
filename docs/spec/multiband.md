---
description: The built-in three-band Multiband dynamics - crossovers, level followers, above and below stages, Amount and Time, how the observed multiband dynamics device imports into it, and how close it comes to the measured renders
read_when: multiband, multiband dynamics, multiband compressor, upward compression, downward expansion, above threshold, below threshold, level follower, crossover, importing the multiband dynamics device, MultibandDynamics elements
type: spec
status: current
last_verified: 2026-10-28
covers:
  - devices/multiband/**
  - scripts/multiband_panel.py
  - crates/als/src/devices.rs
---

# Multiband

Suhina's `multiband` (`devices/multiband`): three bands split by Linkwitz-Riley crossovers, each
with its own two dynamics stages: an **above** stage that acts on the band's level over a
threshold (downward compression, or upward expansion) and a **below** stage that acts under a
second threshold (upward compression, or downward expansion). One instance can therefore compress
loud parts and lift quiet ones in every band at once. It is designed so that the observed
multiband dynamics device imports into it ([`device-import.md`](device-import.md)).

- **Observed with:** the counterpart at 12.4.6 and 12.4.7. Two sets of renders: the measurement
  runs of the Suhina device notes (`live-12/audio-effects/29.26-multiband-dynamics.md` and
  `measurements/29.26-multiband-dynamics/` in the device-specs repository: crossovers, static
  curves, attack, release, Time, Amount, knee, Peak and RMS, side-chain, at 44.1 kHz on synthetic
  probes), and our own renders of 78 settings (level steps down to −90 dB, bursts, sweeps, noise,
  58 music clips and a full song; one export per setting, every other setting changed one at a
  time). Our renders were exported at 48 kHz from an engine running at 44.1 kHz, which renders the
  counterpart with its 44.1 kHz coefficients: every frequency in them reads 1.088 times too high and
  every time 0.919 times too short. The numbers below are corrected to the engine's own rate;
  renders at matching rates confirmed the shift (crossover 120.2 Hz at 44.1 kHz, 130.9 Hz at 48).
- **Sources:** the manual of the observed device, section 29.26 (what each control is for); the
  measurements above (how it sounds); Giannoulis, Massberg and Reiss, *Digital Dynamic Range
  Compressor Design* (JAES 2012) for the quadratic knee and the branching level follower; the
  Linkwitz-Riley crossover. Everything else is this device's own design and is marked as a
  decision below. No preset, sample or text of the counterpart is part of Suhina.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable. The 9 band parameters repeat
for `low`, `mid` and `high` (shown for `low`).

| # | Id | Name | Range | Default | Unit, scale |
|---|---|---|---|---|---|
| 0 | `lowCrossover` | Low Crossover | 30 Hz – 3 kHz | 120 Hz | hertz, logarithmic |
| 1 | `highCrossover` | High Crossover | 300 Hz – 15 kHz | 2.5 kHz | hertz, logarithmic |
| 2 | `peak` | Peak | off (RMS) / on | off | toggle |
| 3 | `softKnee` | Soft Knee | off / on | on | toggle |
| 4 | `amount` | Amount | 0 – 100 % | 100 % | percent, linear |
| 5 | `time` | Time | 10 – 1000 % | 100 % | factor 0.1 – 10, logarithmic |
| 6 | `output` | Output | −24 – +24 dB | 0 dB | decibels, linear |
| 7 + 9b | `lowOn` | Low On | off / on | on | toggle |
| 8 + 9b | `lowInput` | Low Input | −24 – +24 dB | 0 dB | decibels, linear |
| 9 + 9b | `lowAboveThreshold` | Low Above Threshold | −80 – 0 dB | −20 dB | decibels, linear |
| 10 + 9b | `lowAboveSlope` | Low Above Slope | 0 – 2 | 1 | none, linear |
| 11 + 9b | `lowBelowThreshold` | Low Below Threshold | −80 – 0 dB | −60 dB | decibels, linear |
| 12 + 9b | `lowBelowSlope` | Low Below Slope | 0 – 4 | 1 | none, linear |
| 13 + 9b | `lowAttack` | Low Attack | 0.1 ms – 5 s | 50 ms (mid 10, high 5) | seconds, logarithmic |
| 14 + 9b | `lowRelease` | Low Release | 0.1 ms – 5 s | 300 ms (mid 200, high 100) | seconds, logarithmic |
| 15 + 9b | `lowGain` | Low Gain | −24 – +24 dB | 0 dB | decibels, linear |

| 34 | `band` | Band | Low, Mid, High | Mid | choice |
| 35 + b | `lowSolo` | Low Solo | off, on | off | toggle |
| 38 | `meterRange` | Meter Range | 12 – 96 dB | 60 dB | decibels, linear |

`b` is the band: 0 low, 1 mid, 2 high (39 parameters). `band` only chooses which band's controls
the panel shows and `meterRange` how many decibels its output meter spans; neither changes the
sound and the import leaves both at their defaults. While any band is
soloed only the soloed bands are heard; a band that is not heard still follows its level, so it
comes back without a jump.

- **Slopes** are the stage's output slope: decibels out per decibel in, beyond its threshold.
  Above: 0 holds the level at the threshold (a limiter, 1 : ∞), 0.25 is 4 : 1 compression, 1
  does nothing, 2 doubles every decibel over the threshold. Below: 0 holds quiet parts at the
  threshold (upward compression to the end), 0.5 halves how far they sit under it, 1 does
  nothing, 4 pushes them four times as far down (expansion). Decisions 1.

## Behaviour

Per frame, with the left and right sides through the same filters:

1. **Crossovers.** Each side splits at the low crossover into low and the rest, the rest at the
   high crossover into mid and high (24 dB/octave Linkwitz-Riley, −6 dB at each crossover), and
   the low band goes through the high crossover's all-pass, so the three bands add up to a flat
   magnitude (an all-pass; measured flat to 0.011 dB at three crossover pairs, ours to 0.01 dB).
   The phase is that of the two crossovers: the sum's group delay is about 4 ms at 120 Hz with the
   default crossovers, as measured. Nothing is added for lookahead, so the device has no latency
   of its own.
2. **Band input.** A band that is off passes unchanged: no input gain, no dynamics, no gain.
   Otherwise its input gain applies, and the band's detector sees the band after it (measured:
   Input +12 dB moves both thresholds 12 dB lower).
3. **Level.** One follower per band, shared by both sides, follows the mean of the two sides:
   `|x|` with Peak on, `x²` with Peak off (RMS). It is a branching one-pole: it moves toward a
   larger value with the attack coefficient and toward a smaller one with the release
   coefficient, each with the time constant `setting / 8 × Time`. The follower acts within each
   cycle of a low tone at short times, as measured (the gain modulation that gives 10 – 35 % THD
   at the 0.1 ms minimum). The level in decibels is `20 log10` of the Peak follower and `10 log10`
   of the RMS one, so a sine reads the same peak level in both modes (measured: no offset between
   the modes), while the RMS mode releases half as fast in decibels. Levels under −100 dB count
   as −100 dB.
4. **Gain.** Above: `(slope − 1) × knee(level − above threshold)`. Below:
   `(1 − slope) × knee(below threshold − level)`. `knee(u)` is 0 under `−w/2`, `u` over `w/2` and
   `(u + w/2)² / 2w` between (a quadratic knee centred on the threshold), with `w` 23.6 dB for
   the above stage with Soft Knee on and 10 dB with it off, and 16.3 dB for the below stage.
   Each stage's gain is limited: `g / (1 + |g / 36|^7.04)^(1/7.04)`, close to `g` up to about 25 dB
   and never past 36 dB either way. The two stages add in decibels (measured: they do not affect
   each other).
5. **Amount** scales every gain of the device in decibels: both stages, the band input and band
   gains and the output gain (measured: 0 % is a plain crossover, 25 % gives 0.25 – 0.27 of the
   change, 50 % 0.51 – 0.53, and with Amount 48 % an output gain of 12.6 dB gave 6.0 dB; the input
   gain scales with it as well).
6. **Band gain and output.** Each band gets `(stages + gain) × Amount` decibels, the bands are
   added, and the output gain (× Amount) applies last.

- **Time** scales both times of every band (measured: 316 % is 3.1 times slower, 10 % finishes a
  100 ms attack in about 2.6 ms); it does not move the static curve.
- **Changes while playing.** Gains and thresholds glide over 20 ms, crossovers over 50 ms as
  log2 hertz with the filters retuned every 16 frames; slopes, times, knee, Peak and Amount take
  effect at once.

## Panel

`devices/multiband/panel.json`, written by `scripts/multiband_panel.py` (run it after changing the
script; the file is its output), lays out the design boards D (the device row) and E (the expanded
view). The canvases draw from the parameters and from the device's live values
([ADR 0032](../decisions/0032-device-readouts.md)), so they follow every knob and the sound. Every
colour follows the theme: text, panel, control, border and accent are the theme's, and the display,
the three band colours and the gain line are theme tokens (`--device-display`,
`--device-series-1` … `3`, `--device-highlight`; [SDK UI](sdk-custom-ui.md)).

- **Band view** (both canvases): the dark display with each band tinted between the crossovers;
  the input spectrum filled in the accent, the output's (input plus the band's gain change) dotted
  over it; a 0 dB line near the top with the gain line under it, falling by each band's cut with
  the area above it shaded; each band's above threshold dashed with a ring in the middle of its
  band and its below threshold fainter; the crossovers with a grip near the bottom and their
  frequency; each band's name with its gain change, the chosen band boxed; the frequency axis; and
  a grey veil over a band that is off or not heard because another is soloed. The crossovers are
  dragged sideways and the thresholds up and down within their band; a double click resets one.
- **Compact** (820 × 168, the device row's height): the band view at the left; over the right part
  a row of Low / Mid / High (the `band` parameter), RMS / Peak, Soft knee, and the Default and OTT
  quick presets; the chosen band's column (its name, transfer curve with the signal's place as a
  dot, power button and Solo); its knobs Above, Ratio, Below, Upward, Attack, Release, Input, Gain
  (Ratio and Upward are the slopes shown as `1/s : 1`); after a divider Amount (large, in the
  accent), Time and Output; and the output's two meters, dragged up or down to change their range
  (`meterRange`), with their scale. While the expanded strip is open the band view gives way to the
  three bands' curves side by side, each with its power button and Solo, and the rest moves left
  into the chosen band's column.
- **Expanded** (1200 × 160, `strip`: a strip over the device row scaled to its width, its header
  with the name and Close): the band view large with its handles and a dB scale.
- **Quick preset Default**: every sound parameter at its default, in one undo step.
- **Quick preset OTT**: heavy upward and downward compression on all three bands, in one undo step;
  Suhina's own rounded values (crossovers 90 Hz and 2.5 kHz, input +5 dB, above thresholds
  -34/-30/-35 dB with slopes 0.015/0.015/0, below -41 dB with slope 0.24, attacks 50/22/13 ms,
  releases 280/280/130 ms, gains +10/+6/+10 dB, no solo). The name is a style name
  ([ADR 0014](../decisions/0014-stock-presets.md) › Amendment: established style names).

### Readouts

What `Device::readouts` holds, from the block's last frame:

| Index | Value |
|---|---|
| 0, 2, 4 | the low, mid and high band's level as its follower reads it, smoothed over 80 ms for the display, dB (-100 when off) |
| 1, 3, 5 | that band's gain change from its stages, scaled by Amount, smoothed the same way, dB (0 when off) |
| 6, 7 | the output's peak, left and right, dBFS, falling with a 0.3 s time constant |
| 8 – 31 | the input spectrum: 24 bands spaced evenly in log frequency from 20 Hz to 20 kHz, each a band-pass with an RMS follower that rises in 10 ms and falls in 300 ms (`suhina_dsp::Analyser`), dB |

## Import

The observed device is stored as `MultibandDynamics`; every value is a `Manual` value in a
physical unit (Hz, dB, ms, factors), the ratios as the stored values `v` of their own scale.

| Stored | Ours | Conversion |
|---|---|---|
| `SplitLowMid`, `SplitMidHigh` | `lowCrossover`, `highCrossover` | Hz as they are |
| `EnvelopeIsPeak` | `peak` | true = on |
| `SoftKnee` | `softKnee` | as it is |
| `GlobalAmount` | `amount` | fraction as it is |
| `GlobalTime` | `time` | factor as it is |
| `OutputGain` | `output` | dB as they are |
| `ActiveLow` … | `lowOn` … | as it is |
| `InputGainLow` … | `lowInput` … | dB |
| `AboveThresholdLow` …, `BelowThresholdLow` … | `lowAboveThreshold` …, `lowBelowThreshold` … | dB |
| `AboveRatioLow` … (−1 … 1, displayed `1 : 1/(1+v)`) | `lowAboveSlope` … | `1 + v` |
| `BelowRatioLow` … (−3 … 1, displayed `1 : 1/(1−v)`) | `lowBelowSlope` … | `1 − v` |
| `AttackLow` …, `ReleaseLow` … | `lowAttack` …, `lowRelease` … | ms / 1000 |
| `GainLow` … | `lowGain` … | dB |

- The stored `SideChain` block (its own `OnOff`, `DryWet` = Mix and `Volume` = gain) is not
  imported yet; a side-chain that is on is noted in the import's warnings.
- `SplitLowMidOn` and `SplitMidHighOn` (the buttons that merge a band into its neighbour) are plain
  values, not parameters; a merged band is noted and played as its own band. The plain `Solo`
  values (`SoloLow`, `SoloMid`, `SoloHigh`) become the band's Solo.
- A value missing or not a finite number keeps our default; ratios outside their range are held
  at its ends.

## Decisions

1. **Slopes instead of ratios.** The parameters are the output slopes beyond each threshold, not
   `1 : x` ratios: one linear scale per stage that runs through 1 (nothing) to both sides, so
   compression and expansion are one knob and the import is `1 ± v`. A panel can show the slope as
   a ratio.
2. **The level follower and its time constant.** `setting / 8` with no further smoothing of the
   gain reproduces the measured attack and release curves (0.13 dB mean over nine attack, release
   and Time cases) and the music renders; a follower on the level followed by a gain smoother fits
   the music only and misses the minimum-time measurements.
3. **The mean of the two sides.** The measurements with the same signal on both sides cannot
   tell the link apart; on stereo music the mean fits (0.10 dB on a Peak-mode setting) where
   following the louder side does not (1.01 dB).
4. **One limit for both stages.** The rational limit at 36 dB fits the above stage's saturation at
   extreme settings and the below stage's upward gain, which stops at +36 dB. The measured
   downward limit of the below stage stops harder, at about −32 dB (Known differences).
5. **Knee widths.** 23.6 dB (soft), 10 dB (off) and 16.3 dB (below) are fitted values: with the
   follower acting within each cycle, the corner a sine shows is wider than the gain computer's
   own.
6. **No side-chain yet.** The measured side-chain (key split into the bands, Gain moving the level
   one for one, Mix as a sum of the two waveforms at 0.707 each at 50 %) is for a later change.

## Known differences

- **Level.** Against the renders of 27 settings over 30 s of music and 30 s of a song (one export
  each, settings across ratios, thresholds, times, Amount, Time, knee, Peak, band gains, band
  switches and two heavy multi-stage settings): 0.01 – 0.34 dB RMS of the 10 ms levels, with the
  heaviest settings 0.22 – 0.34 dB, and a waveform residual of −21 to −42 dB. A blind A/B of eight
  of them (four settings on music and the song) heard no difference.
- **Below stage, deep cuts.** At `1 : 0.25` the measured cut stops at −32 dB; ours reaches −28 dB
  there (2.5 dB RMS on that static curve, 0.5 – 0.6 dB at `1 : 0.5` and `1 : ∞`).
- **Side-chain and band merge buttons** are not built (Decisions 6, Import).

## Cost

Per frame and side, five biquad pairs (two crossovers and the all-pass); per band one follower,
two logarithms and one exponential; for the panel's spectrum 24 band-pass biquads on the mono
sum and 24 logarithms per block. Not measured against the 1 % budget yet.

## Tests

`devices/multiband/src/tests.rs`: the device contract; the three bands sum flat at 1 (all-pass);
a band that is off passes untouched; the above stage's static gain at slopes 0.25, 0 and 2 and
the below stage's at 0.5 and 4 against the formulas; the knee; Amount 0 is a plain crossover and
50 % halves the gain change; Time scales the attack; Peak and RMS read a sine alike; the limit
holds at 36 dB; the follower's attack reaches 63 % in about `attack / 8`; a soloed band plays
alone and soloing all three changes nothing; the readouts show the compressing band's level and
cut, the output meter and the tone's spectrum band, and rest after a reset. `crates/als/src/devices_tests.rs`:
the stored elements import with the conversions above. `crates/device-registry/src/lib_tests.rs`: the panel loads, fits the device row, reaches every parameter, switches bands by its tabs and has an expanded canvas; every layout loads (`layout_errors` is empty).
