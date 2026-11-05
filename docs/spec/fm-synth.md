---
description: The built-in FM Synth - operators, waveforms and drawn harmonics, DX7 and time envelopes, filter and shaper, LFOs, the modulation matrix, stereo and spread, algorithms, the panel, what a theme may change, and the randomizer
read_when: FM synth, fm-synth, operators, algorithms, FM waveforms, harmonics, time envelope, envelope loop, FM filter, spread, modulation matrix, FM panel, randomize, device.randomize, instrument randomizer
type: spec
status: current
last_verified: 2026-11-04
covers:
  - devices/fm-synth/**
---

# FM Synth

Suhina's `fm-synth` (`devices/fm-synth`). Why it grew in place:
[ADR 0034](../decisions/0034-fm-synth-grows-in-place.md). The DX7 file format:
[dx7-voice-format.md](dx7-voice-format.md).

## Signal path

Each note runs its operators through the algorithm, then its own filter and shaper, then its pan.
Notes sum to a stereo pair, followed by tone and volume. The engine works in blocks of 64 frames on
a grid from the last reset, so the output does not depend on how the host divides a stream.

## Operators

Six operators, each with level, coarse, fine, detune, ratio or fixed frequency, on or off, keyboard
scaling and velocity sensitivity, as before, and new:

- **Wave:** sine (the default), triangle, saw, square, two noises, 4-bit and 8-bit sines, digital saw
  and square, or drawn. Band-limited tables are chosen by pitch.
- **Harmonics:** 64 amplitudes for the drawn wave, edited in 16, 32 or 64 partials: a press and a drag
  across the bars draws them, as one undo step, and switches the operator to the Drawn wave. Repeat
  continues the drawn partials upward as they fade; Normalize keeps the level steady.
- **Feedback** per operator, for an operator nothing else modulates; the voice's own feedback stays.
- **Phase** at the start of a note, **Retrigger**, and **frequency by velocity** with an optional
  quantize to whole ratios.

## Envelopes

Each operator selects **Rates and levels** (the DX7 model, for old projects and voice files) or
**Time**, which a newly added device starts with. A released note with a level 4 above zero holds it
for a second and then falls silent, so every note ends. Time has initial,
attack, peak, decay, sustain and release, in seconds and decibels, time by velocity and by key, and a
loop mode: off, loop (from the sustain, after the loop time), beat (at the repeat length from the note),
sync (the first repeat at the song's nearest sixteenth, then by the repeat length, as beat while the song
stops) or trigger (ignoring the key going up). A repeat starts from the level the envelope has, and a
release from wherever it is. Time does not move beat and sync repeats. The filter, pitch and LFO envelopes use the same model. Each has slopes and an
end level; with an operator's slopes at 0 its attack is linear and its decay and release exponential. **Time** on the device scales every time envelope.

## Filter and shaper

One per note, off by default: low-pass, high-pass, band-pass, notch and morph at 12 or 24 dB/octave,
in a clean circuit or one of four driven circuits of Suhina's own; frequency, resonance, drive,
envelope amount, frequency by velocity and by key; then a shaper with its own drive and dry/wet.
Resonance raises a low-pass's or high-pass's level at the cutoff evenly in decibels, from -6 dB to +28 dB,
at either slope: two sections share the Q, so the slope changes how fast it falls and not how loud a
resonance is.

## Moving a control on a sounding note

A drag sends a new value every frame of the window, far apart in sound. What can be heard stepping
glides there over about 15 ms instead: the filter's frequency, resonance, morph and drive, the shaper's
drive and dry/wet, Tone, Volume, Pan, a time envelope's sustain, a time-envelope operator's level and an
operator's feedback. With nothing sounding a change is simply there, and the rates-and-levels envelopes
keep their own way to a level, as a voice file's sound depends on it.

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

- **Device row (1078 by 168):** the operators as a table with its columns named once (number, ratio or Hz
  with what coarse and fine come to, coarse, fine, level, on), one display with the pages Env, Wave,
  Filter, LFO, Pitch, Algo, Mod and Voice and the eight dials that follow it, then the LFO's and the
  filter's main controls, in view whatever the display shows, Spread and Volume, Random with its amount
  and **More…**, and meters. The display has a line of what the page chooses above its picture, so a menu
  never covers it. The Mod page is the first route of every source, edited in place, in the room of the
  display and the dials.
- **Each operator has a colour:** on its row, in the algorithm's picture, and where something is sent to it.
- **Adding operators.** A new device starts with operator 1. An operator not in the sound shows only its
  number and its light, the first such row has **+ Add operator**, and in the algorithm's picture it is
  faint. An old project and a voice file keep their operators on.
- **Ratio or Hz.** In Hz the row's tuning stands out on a tint, coarse reads as its range of 1, 10, 100
  or 1000 Hz, fine as what it multiplies that by, and the frequency beside them comes to their product.
- **Larger view (1400 by 230, a strip above the device row, scaled to its width):** the header's Expand
  button and **More…** open it. The row stays in view and chooses the page, so the strip has no tabs of
  its own: it shows the page's picture large and only what the row has no room for. Env has the envelope's
  end, loop and slopes and the keyboard scaling; Wave the partials and how they repeat; Filter and LFO their
  envelopes, the shaper and the classic LFO; Pitch where it goes and the classic pitch envelope; Algo every
  algorithm as a card; Mod the whole matrix and the classic controllers; Voice the output, stereo, voices,
  pitch and quality; Random, reached from its **More…**, its sections.
- **Tooltips** say what a control is for, not how a mouse moves it.
- It opens on Env with operator 1 selected. Selected operator and page are hidden parameters.
- **What it shows.** Pictures use the scales hearing does: an operator's levels in decibels, the filter's
  response in decibels over a logarithmic frequency axis, partials as what makes the timbre. Phase, which
  the ear hardly hears, has a small place. A control that does nothing right now (Morph unless the type is Morph, Drive
  on the Clean circuit) is dimmed and still answers. A dot on a tab says that section is on.
- **Envelopes.** The picture moves with its times, levels and slopes, and each stage's end is one dot,
  dragged in time and level at once. The dot in a stage's middle lies on its curve and bends it as it is dragged up or down.
  **ADSR** and **Rates** choose the model. A rates-and-levels stage further right has a lower rate. A line
  marks where the key goes up.
- **The filter's picture** is its response, for each type and through Morph, with a line at the cutoff
  and frequency and resonance on one dot, or, with **Envelope** above it, its envelope, and the dials
  follow. On a low-pass or a high-pass the dot lies on the curve.
- **The LFO's picture** is its wave on a grid, quietly, and in colour as much of it as reaches the sound after its
  amount and its envelope. More cycles fit as its rate rises (two seconds' worth on the low range), and
  while it is on they run, at its rate up to three cycles a second. **Rate in** chooses Hz, Audio (hertz
  up to audio rate) or Sync (a note length). The classic LFO's rate reads in hertz too.
- **Selecting.** A click on an operator's number or its square in the algorithm picture selects it. The
  row is lighter while selected and the picture rings it.
- Algorithm pictures are computed from the engine's table. An operator heard directly is filled, one
  that modulates is outlined. A card on the Algo page takes the click anywhere on it.

**A theme changes styles only:** palette, `--device-display`, `--device-series-*`,
`--device-highlight`, `--device-spectrum` and fonts. Layout is fixed.

The header's Close closes the larger view, and Undo is the app's own (Ctrl+Z).

## Import

Importing the instrument from a Live Set waits for sets authored to observe it
([device-import.md](device-import.md)); until then it stays a placeholder.

## Randomizer

`device.randomize` takes the device, a seed and an amount, and sets the new values as one undo step. Each
section has a share and says whether it changes or keeps its values (its `random.*` parameters). Sections:
envelopes, levels, waves, filter, LFO, pitch envelope, modulation, stereo, ratios and algorithm; the last
two keep theirs by default, since they change a sound most. A value moves from where it is, by at most its
section's share of its range; only the
envelope model an operator uses moves, and the device's Time and level 4 stay. The device's rules keep a
result audible and playable: at least one carrier keeps its level, ratios stay whole unless asked, the
filter does not close entirely, and while the envelopes take part every carrier starts within about 2
seconds and ends soon after its key goes up. Random on the panel runs at once; its amount is beside it, and
**More…** shows its sections in the larger view.
