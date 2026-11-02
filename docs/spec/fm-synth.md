---
description: The built-in FM Synth - operators, waveforms and drawn harmonics, DX7 and time envelopes, filter and shaper, LFOs, the modulation matrix, stereo and spread, algorithms, the panel, what a theme may change, and the randomizer
read_when: FM synth, fm-synth, operators, algorithms, FM waveforms, harmonics, time envelope, envelope loop, FM filter, spread, modulation matrix, FM panel, randomize, device.randomize, instrument randomizer
type: spec
status: current
last_verified: 2026-11-02
covers:
  - devices/fm-synth/**
---

# FM Synth

Suhina's `fm-synth` (`devices/fm-synth`). Why it grew in place:
[ADR 0034](../decisions/0034-fm-synth-grows-in-place.md). The DX7 file format:
[dx7-voice-format.md](dx7-voice-format.md).

Status: this document describes the target of [#1139](https://github.com/suhina-daw/suhina-daw/issues/1139);
the pull request that closes it builds all of it.

## Signal path

Each note runs its operators through the algorithm, then its own filter and shaper, then its pan.
Notes sum to a stereo pair, followed by tone and volume. The engine works in blocks of 64 frames on
a grid from the last reset, so the output does not depend on how the host divides a stream.

## Operators

Six operators, each with level, coarse, fine, detune, ratio or fixed frequency, on or off, keyboard
scaling and velocity sensitivity, as before, and new:

- **Wave:** sine (the default), triangle, saw, square, two noises, 4-bit and 8-bit sines, digital saw
  and square, or drawn. Band-limited tables are chosen by pitch.
- **Harmonics:** 64 amplitudes for the drawn wave, edited in 16, 32 or 64 partials. Repeat continues
  the drawn partials upward as they fade; Normalize keeps the level steady.
- **Feedback** per operator, for an operator nothing else modulates; the voice's own feedback stays.
- **Phase** at the start of a note, **Retrigger**, and **frequency by velocity** with an optional
  quantize to whole ratios.

## Envelopes

Each operator selects **Rates and levels** (the DX7 model and the default) or **Time**: initial,
attack, peak, decay, sustain and release, in seconds and decibels, time by velocity and by key, and a
loop mode: off, loop, beat, sync or trigger. The filter, pitch and LFO envelopes use the same model
and add slopes and an end level. **Time** on the device scales every time envelope.

## Filter and shaper

One per note, off by default: low-pass, high-pass, band-pass, notch and morph at 12 or 24 dB/octave,
in a clean circuit or one of four driven circuits of Suhina's own; frequency, resonance, drive,
envelope amount, frequency by velocity and by key; then a shaper with its own drive and dry/wet.

## LFOs

The DX7 LFO stays. A second LFO reaches audio rate, with classic waves, sample and hold and noise, in
low, high or sync ranges, rate by key and amount by velocity, and goes to chosen operators and the
filter. Its default amount is zero.

## Modulation matrix

Nine sources (velocity, key, pressure, pitch bend, mod wheel, foot, breath, LFO, random), each with
two destinations and amounts from -100 to +100 %. Destinations: operator levels, feedback, fixed
frequency, FM amount, filter, LFO, pitch envelope amount, volume, pan, tone and time.

## Stereo and voices

Pan, pan by key and random pan per note. **Spread** runs two detuned notes panned apart; it is read
when a note starts. **Voices** is 1 to 32, 16 by default; at 1 notes play legato. Glide uses the
existing portamento on every voice.

## Algorithms

The 32 of the DX7 format, then 11 for four operators. A voice file reaches only the first 32.

## Panel

Layout from `scripts/fm_synth_panel.py`, written to `devices/fm-synth/panel.json`.

- **Device row (168 px high):** six operator rows (number, envelope picture, ratio or Hz, coarse, fine,
  level, on), the algorithm with its picture and feedback, a display with the pages Env, Wave, Filter,
  LFO, Pitch and Mod with eight dials that follow the page, then Time, Tone, Spread and Volume, Random,
  `%` and meters. The Mod page shows routings; editing them needs the expanded panel.
- **Expanded panel (1180 by 640):** operators on the left, the page in the middle, sections on the
  right. Pages: Operator, Filter, LFO, Pitch, Matrix, Algorithm and Random.
- It opens on Env with operator 1 selected. Selected operator and page are hidden parameters.
- Algorithm pictures are computed from the engine's table. An operator heard directly is filled, one
  that modulates is outlined, and the selected one takes the accent.

**A theme changes styles only:** palette, `--device-display`, `--device-series-*`,
`--device-highlight`, `--device-spectrum` and fonts. Layout is fixed.

## Randomizer

`device.randomize` takes the device, a seed, an amount and per-section percentages and locks, and
sets the new values as one undo step. Sections: envelopes, levels, waves, filter, LFO, pitch envelope,
modulation, stereo, ratios and algorithm; the last two are off by default, since they change a sound
most. A value moves from where it is, by at most its section's percentage of its range. The device's
rules keep a result audible: at least one carrier keeps its level, ratios stay whole unless asked, the
filter does not close entirely. Random on the panel runs at once; `%` opens its settings.
