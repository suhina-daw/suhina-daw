---
description: The built-in three-band DJ EQ - its bands, gains, kills, crossovers and slopes, how the observed three-band EQ with kills imports into it, and how close it comes to the measured renders
read_when: dj eq, three-band eq, band split, kill, crossover, linkwitz-riley, importing the three-band EQ, FilterEQ3 elements
type: spec
status: current
last_verified: 2026-10-28
covers:
  - devices/dj-eq/**
  - crates/als/src/devices.rs
---

# DJ EQ

Suhina's `dj-eq` (`devices/dj-eq`): three bands split by Linkwitz-Riley crossovers, each with a
gain and a kill switch. Two crossover frequencies divide a signal into low, mid and high; each
band's gain runs from silence up to +6 dB, and its on/off button silences the band. A switch
chooses 24 or 48 dB per octave for both crossovers. It is designed so that the observed three-band
EQ with kills imports into it ([`device-import.md`](device-import.md)).

- **Observed with:** the counterpart at 12.4.6, renders at 44.1 kHz. The parameters (ranges,
  defaults, tapers, the stored elements) come from the parameter dump and a saved set; the sound
  from the measured runs `eq-three-bands` and `eq-three-gain` (band shapes at crossovers 500 Hz
  and 2 kHz in both slope settings, gains from +6 dB down to silence, the 0 dB sums), analysed by
  the measurement harness of the Suhina device notes (`live-12/audio-effects/29.16-eq-three.md` in
  the device-specs repository, which lists the runs not made yet in `measurements/29.16-eq-three/runs.py`).
- **Sources:** the manual of the observed three-band EQ, section 29.16 (what each control is for);
  the measurements above (how it sounds); the Linkwitz-Riley crossover (Siegfried Linkwitz's active
  crossover papers) for the structure. Everything else is this device's own design and is marked as
  a decision below.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable.

| # | Id | Name | Range | Default | Unit, scale |
|---|---|---|---|---|---|
| 0 | `low` | Low | silence … +6 dB | 0 dB | decibels, linear |
| 1 | `mid` | Mid | silence … +6 dB | 0 dB | decibels, linear |
| 2 | `high` | High | silence … +6 dB | 0 dB | decibels, linear |
| 3 | `lowKill` | Low Kill | off/on | off | toggle |
| 4 | `midKill` | Mid Kill | off/on | off | toggle |
| 5 | `highKill` | High Kill | off/on | off | toggle |
| 6 | `lowFrequency` | Low Frequency | 50 Hz – 5 kHz | 250 Hz | hertz, logarithmic |
| 7 | `highFrequency` | High Frequency | 200 Hz – 18 kHz | 2.5 kHz | hertz, logarithmic |
| 8 | `slope` | Slope | 24 dB/oct, 48 dB/oct | 24 dB/oct | choice |

"Silence" is the gain's bottom of the range (−70 dB): the band is removed, other frequencies stay.

## Behaviour

Per frame: each side is split at the low crossover into low and the rest, and the rest at the high
crossover into mid and high; each band is multiplied by its smoothed gain (a kill drives it to 0)
and the three bands are added. The left and right sides run the same filters separately. All three
bands of both sides are always built; a silenced or killed band is multiplied, not skipped.

- **Band shapes.** The low band is the low crossover's low-pass alone. The mid band is a high-pass
  at the low crossover followed by a low-pass at the high one (a cascade, not the remainder); the
  high band is a high-pass at the low crossover followed by a high-pass at the high one. Every
  crossover point is −6 dB at the set frequency in both slope settings: 24 dB is a 4th-order
  Linkwitz-Riley split (two 2nd-order Butterworth sections per band), 48 dB an 8th-order one (two
  4th-order sections). The bands match those responses to 0.01 dB in the measurements, and the
  same responses come out of this device (the tests compare them at 48 kHz within 0.2 dB).
- **The sum at 0 dB.** No all-pass is added anywhere, so the three bands do not add back to a flat
  magnitude: with crossovers at 500 Hz and 2 kHz the sum dips −0.75 dB at 653 Hz in the 24 dB
  setting and −2.14 dB at 528 Hz in the 48 dB one, both around the low crossover; the dip at the
  high crossover is smaller (−0.07 dB at 2 kHz in the 24 dB setting, none in the 48 dB one). This
  is the colour the counterpart's manual describes as an analogue filter cascade; it is part of
  the device (Decisions 2).
- **Gain and kill.** A band's gain is a plain multiplier of its own response: nothing else moves.
  A kill drives the band's gain to 0, the same as the gain's bottom of the range. In the
  measurements a killed band and a band at silence rendered identically.
- **Changes while playing.** Gains and kills glide over 20 ms; crossover frequencies glide over
  50 ms, smoothed as log2 hertz so a sweep sounds even, and the filters are retuned every 16
  frames while they move. The filters' state carries through a retune, so a sweep does not click.
- **Ordering.** The crossovers are independent: with the low frequency above the high one the
  mid band (a high-pass at the low frequency, then a low-pass at the high one) is almost empty,
  about −63 dB, and the sum with every gain at 0 dB stays flat to 0.3 dB (Decisions 4).
- **Latency:** none beyond the filters' own group delay (the largest tap of the sum's impulse is
  the first sample, as in the measurements).

## Import

The observed three-band EQ is the element `FilterEQ3` ([`als-format.md`](als-format.md) ›
Devices). `crates/als/src/devices.rs` maps it:

| Element | Stored | Suhina |
|---|---|---|
| `On` | true/false | the device's on/off |
| `GainLo/Manual`, `GainMid/Manual`, `GainHi/Manual` | a linear gain, 1 = 0 dB, 0 = silence | `low`, `mid`, `high` in dB |
| `LowOn/Manual`, `MidOn/Manual`, `HighOn/Manual` | true/false, true = the band is on | `lowKill`, `midKill`, `highKill`, inverted |
| `FreqLo/Manual`, `FreqHi/Manual` | hertz | `lowFrequency`, `highFrequency` |
| `Slope/Manual` | 0 = 24 dB/oct, 1 = 48 dB/oct | `slope`, the same order as ours |

A gain above the +6 dB top of the range clamps to it (a stored 2.0 is about +6 dB). A slope
outside its two settings is left at the default with a warning.

## Decisions

Each entry: what was chosen, what it costs, why.

1. **Own name and panel.** The device is Suhina's DJ EQ with the generated parameter list; the
   three band LEDs and the knob tapers (the non-linear dB display curves) are panel parts and are
   left for later (the device-specs AGENTS › Scope). The stored values are plain decibels, so the
   import is exact without them.
2. **The measured band split, not a textbook flat crossover.** No all-pass on the low band: the
   three bands at 0 dB keep the measured dip at the low crossover instead of summing flat. Cost:
   the flat setting is coloured by up to −0.75 dB (24 dB setting) or −2.14 dB (48 dB setting) at
   the low crossover, and about 1 % less CPU than an all-pass split would be. Why: the
   measurements reproduce the band shapes and the 0 dB sums this way to 0.01 dB, and the earlier
   guess of an all-pass was wrong.
3. **Gains run from silence to +6 dB.** The dump's range; the earlier +12 dB top was ours alone.
   Cost: none; a set that asked for more is clamped (import does the same).
4. **The crossovers are not tied to each other.** The counterpart does not clamp FreqHi up to
   FreqLo: with FreqLo above FreqHi its mid band is closed (about −63 dB) and the sum is flat
   to 0.3 dB, and with FreqHi below FreqLo the high band carries both high-passes. The same
   structure reproduces these runs to 0.4 dB without a clamp, so there is none (the note's
   Measurement plan 8).
5. **Changes ramp.** Gains and kills glide over 20 ms, frequencies over 50 ms. Whether the
   counterpart ramps its kills or switches them was not measured (a [measure] gap); ramping keeps
   a kill click-free while playing. Candidate run: the note's Measurement plan 5 (kill clicks).
6. **Both sides run the same filters.** The counterpart's bands are the same in left and right and
   do not depend on level; there is no mid/side or per-channel curve.

## Known differences

The three band LEDs are not built: they are panel parts (a band level detector at −24 dB on top of
the bands), left for later with the layout that would draw them. Whether the counterpart's kills
ramp or switch, and the LEDs' level measure and hold, were not measured; ours ramp, so a kill is
click-free. The measured runs were made at 44.1 kHz with crossovers at 500 Hz and 2 kHz; other
crossover settings follow the same structure and were not separately measured (candidate run: the
note's `eq-three-freq`).

## Cost

One instance at its defaults: 0.10 % of one core at 48 kHz (60 s rendered in 0.059 s, release
build, Apple M-series), inside the 1 % budget. Run `cargo test --release -p suhina-device-dj-eq
cost -- --ignored --nocapture` to measure it.

## Tests

`devices/dj-eq/src/tests.rs`: the device contract (`device_api::conformance`); each band alone
against the measured magnitudes in both slope settings (loose where the measurement's own render
floor limited it); the measured 0 dB sums and flatness far from the crossovers; gains boosting,
cutting and silencing, the +6 dB top; kills removing their band and leaving the others; crossover
moves while playing; crossed crossovers against the measured responses; a frequency sweep staying clean;
the cost benchmark. `crates/als/src/devices_tests.rs` tests the import: every stored value in our
units, a gain of 0 becoming silence, an odd slope noted and left out.
`crates/dsp/src/crossover.rs` tests the Linkwitz-Riley block itself.
