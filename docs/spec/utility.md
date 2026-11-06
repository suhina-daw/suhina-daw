---
description: The built-in utility effect - its parameters, its signal path, the Gain knob's law (even in dB over its top three quarters, then down to silence), how its gain glides along the knob when it jumps, and how Live's Utility compares
read_when: utility, gain, gain knob, gain taper, balance, width, mid/side, mono, bass mono, phase invert, channel select, DC filter, mute, Scale::Gain, gain smoothing, Glide, importing Live's Utility, StereoGain
type: spec
status: current
last_verified: 2026-10-24
covers:
  - devices/utility/**
  - crates/device-api/src/lib.rs
  - crates/dsp/src/smooth.rs
---

# Utility

Suhina's `utility` (`devices/utility`): gain, balance, stereo width or mid/side, mono and bass
mono, phase inversion, channel selection, a DC filter and mute, in one effect. Live's Utility
(`StereoGain`) imports into it parameter for parameter ([`als-format.md`](als-format.md#utility-stereogain)).

- **Observed with:** Live 12.4.6: the parameter dump, saved sets, renders of the probe signal and
  renders of automated Gain (`live-12/audio-effects/29.40-utility.md` and
  `live-12/automation.md`, with `measurements/29.40-utility/` and `measurements/automation/`, in
  the device-specs repository).
- **Sources:** Live 12 Reference Manual, 29.40 Utility (what each control is for); the
  measurements above (what it does). Everything else is this device's own design.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable.

| # | Id | Name | Range | Default | Unit, kind |
|---|---|---|---|---|---|
| 0 | `gain` | Gain | -70 (silence) – +35 | 0 | dB, the gain law below |
| 1 | `balance` | Balance | -1 – 1 | 0 | balance |
| 2 | `width` | Width | 0 – 4 | 1 | percent (a fraction) |
| 3 | `midSide` | Mid/Side | -1 – 1 | 0 | -1 only the mid, 1 only the side |
| 4 | `mono` | Mono | off/on | off | toggle |
| 5 | `bassMono` | Bass Mono | off/on | off | toggle |
| 6 | `bassMonoFrequency` | Bass Mono Frequency | 50 – 500 | 120 | Hz, logarithmic |
| 7 | `invertLeft` | Invert Left | off/on | off | toggle |
| 8 | `invertRight` | Invert Right | off/on | off | toggle |
| 9 | `channel` | Channel | Stereo, Left, Right, Swap | Stereo | choice |
| 10 | `dcFilter` | DC Filter | off/on | off | toggle |
| 11 | `mute` | Mute | off/on | off | toggle |
| 12 | `bassMonoListen` | Bass Mono Listen | off/on | off | toggle |

## Signal path

DC filter, then one 2×2 matrix that does channel selection, phase inversion, width, mid/side and
mono at once, then bass mono (Bass Mono Listen plays only the band below the frequency, as bass
mono leaves it), then the gain, then balance and mute. With every parameter at its default the
output equals the input exactly.

## The Gain knob

Gain's knob (`Scale::Gain` in `suhina-device-api`) has Live's law, measured from automation: with
the knob's travel p from 0 to 1,

- **from a quarter of the travel to the top**, Gain = 70 p - 35 dB: even in decibels, 0 dB in the
  middle, +35 dB at the top, -17.5 dB at a quarter;
- **below a quarter**, the gain is a power of the travel, g = g(¼) · (4p)^n with
  n = 70 · ¼ · ln 10 / 20 ≈ 2.015, the power at which the slope in decibels does not break
  (-29.6 dB at an eighth);
- **at the bottom**, silence, which the parameter holds as its minimum, -70 dB. Anything at or
  below -70 dB is silence (`suhina_dsp::gain::db_to_gain_or_silence`).

An automation lane of Gain moves evenly along this knob, as every lane moves along its control
([`automation.md`](automation.md#lanes-and-points)): half-way from -60 dB to 0 dB is -16.7 dB,
not -30 dB.

## Smoothing

Every control except the gain ramps in a straight line over 20 ms (`RAMP_SECONDS`), toggles too,
so automating any of them does not click.

The gain **glides along its knob**: the knob's position goes through two one-pole low-passes in
a row with a 0.66 ms time constant (`GAIN_GLIDE_SECONDS`, `suhina_dsp::Glide`), and the gain is
the law above at the glided position. A jump becomes an S-shaped move in knob position whose
middle 80 % takes 2.2 ms, the same as Live's ramp, whatever the size of the jump; a lane's steady
motion is followed 1.3 ms behind. While the knob rests, the gain is worked out once.

## Decisions

- **The glide follows the jump; Live's ramp is centred on it.** Live's Gain ramp is a fixed
  4.4 ms S that starts half its length before the point (it reads the automation ahead). A
  device only sees a change when it arrives, so Suhina's glide starts at the jump and is half-way
  1.1 ms after it. Chosen over reading the automation ahead in the host for every device: one
  shared block, no lookahead, the same sound for a knob turned by hand. The cost: an automated
  jump lands about 1.1 ms later than Live's.
- **A critically damped glide over a restarted S-ramp.** A fixed-length S restarted at every new
  target (automation sends one every 64 frames) would stall at each restart and zipper; two
  one-poles in a row never restart and have no corner at a jump. They approach the target
  without a fixed end (99.9 % after 6.1 ms) and land on it once within a millionth.
- **The other controls keep their 20 ms straight ramp.** Their smoothing in Live is not measured.

## Known differences

- **The panel's Gain slider moves evenly in dB.** The generated parameter list draws every
  continuous parameter as a slider over its range (linear or logarithmic); it has no control
  with the gain law yet, so the slider's travel is not the knob's. The value, the lane and the
  sound follow the law.
- **The jump lands later**, as above.
- **Mute and balance** ramp over 20 ms; Live's times are not measured.
