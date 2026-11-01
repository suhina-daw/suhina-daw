---
description: The built-in stereo delay - its parameters, signal flow, how a time change sounds (Repitch, Fade, Jump), the LFO, ping-pong and freeze, how Live's Delay imports into it, and how close it comes to Live's renders
read_when: delay, echo, delay device, feedback, ping pong, tempo-synced delay, sixteenths, repitch, fade, jump, freeze, band-pass in the feedback loop, importing Live's Delay, DelayLine elements
type: spec
status: current
last_verified: 2026-10-19
covers:
  - devices/delay/**
  - crates/dsp/src/delay.rs
---

# Delay

Suhina's `delay` (`devices/delay`): a stereo delay with one line per side, a band-pass inside the
feedback loop, one LFO that moves the delay time and the band, three ways to follow a change of
the time, ping-pong and freeze. It is designed so that Live's Delay imports into it parameter for
parameter ([`device-import.md`](device-import.md)).

- **Observed with:** Live 12.4.6, 44.1 kHz. The parameters (ranges, defaults, steps, stored
  elements) come from the parameter dump and a saved set; the sound from behaviour runs:
  `delay-feedback` and `delay-filter`, rendered in Live and analysed by the measurement harness
  of the Suhina device notes (`live-12/audio-effects/29.11-delay.md` in the device-specs
  repository, which also lists the runs not made yet in `measurements/29.11-delay/runs.py`).
- **Sources:** Live 12 Reference Manual, 29.11 Delay (what each control is for); the measurements
  above (how it sounds). Everything else is this device's own design and is marked as a decision
  below.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable.

| # | Id | Name | Range | Default | Unit, scale |
|---|---|---|---|---|---|
| 0 | `link` | Link | off/on | on | toggle |
| 1 | `syncLeft` | Sync Left | off/on | on | toggle |
| 2 | `syncRight` | Sync Right | off/on | on | toggle |
| 3 | `sixteenthsLeft` | Sixteenths Left | 1, 2, 3, 4, 5, 6, 8, 16 | 3 | choice |
| 4 | `sixteenthsRight` | Sixteenths Right | as above | 4 | choice |
| 5 | `timeLeft` | Time Left | 1 ms – 5 s | 375 ms | seconds, logarithmic |
| 6 | `timeRight` | Time Right | 1 ms – 5 s | 375 ms | seconds, logarithmic |
| 7 | `offsetLeft` | Offset Left | −33.3 % – +33.3 % | 0 % | percent, linear |
| 8 | `offsetRight` | Offset Right | −33.3 % – +33.3 % | 0 % | percent, linear |
| 9 | `feedback` | Feedback | 0 – 95 % | 50 % | percent, linear |
| 10 | `freeze` | Freeze | off/on | off | toggle |
| 11 | `pingPong` | Ping Pong | off/on | off | toggle |
| 12 | `transition` | Transition | Repitch, Fade, Jump | Repitch | choice |
| 13 | `filter` | Filter | off/on | on | toggle |
| 14 | `filterFrequency` | Filter Frequency | 50 Hz – 18 kHz | 1 kHz | hertz, logarithmic |
| 15 | `filterWidth` | Filter Width | 0.5 – 9 octaves | 8 | octaves, linear |
| 16 | `lfoMode` | LFO Mode | Rate, Time, Synced, Triplet, Dotted, Sixteenths | Rate | choice |
| 17 | `lfoRate` | LFO Rate | 0.01 – 40 Hz | 0.5 Hz | hertz, logarithmic |
| 18 | `lfoTime` | LFO Time | 100 ms – 200 s | 1 s | seconds, logarithmic |
| 19 | `lfoSynced` | LFO Synced Rate | 8 Bars … 1/64 (22 values) | 2 Bars | choice |
| 20 | `lfoSixteenths` | LFO Sixteenths | 1 – 64 | 16 | integer |
| 21 | `lfoShape` | LFO Shape | Sine, Triangle, Ramp Up, Ramp Down, Square, Sample and Hold, Wander | Triangle | choice |
| 22 | `lfoMorph` | LFO Morph | −100 % – +100 % | 0 % | percent, linear |
| 23 | `lfoToTime` | LFO to Time | 0 – 100 % | 0 % | percent, linear |
| 24 | `lfoToFilter` | LFO to Filter | 0 – 100 % | 0 % | percent, linear |
| 25 | `mix` | Dry/Wet | 0 – 100 % | 50 % | percent, linear |
| 26 | `equalLoudness` | Equal-Loudness Mix | off/on | off | toggle |

Ranges, defaults and step lists are Live's (the defaults are what Live's Delay holds when it is
added, its factory preset). The synced LFO rates are, in bars: 8, 6, 4, 3, 2, 1.5, 1, 3/4, 1/2,
3/8, 1/3, 5/16, 1/4, 3/16, 1/6, 1/8, 1/12, 1/16, 1/24, 1/32, 1/48, 1/64.

## Behaviour

Per side and frame: the line is read at the delay time, which is the wet signal; what goes back
into the line is the input plus the wet signal times Feedback, through the band-pass. So the
first repeat passes the band-pass once and the n-th repeat n times, and every repeat is the one
before times Feedback (25 % is −12 dB a repeat, 95 % −0.45 dB). The two sides never feed each
other, except in ping-pong.

- **Delay time.** Sync: the sixteenths at the tempo, times (1 + offset): `n × 15 / BPM` seconds
  (at 120 BPM "4" is 500 ms, and +33.3 % makes it 666.5 ms). Time: the time in seconds; the offset
  does not apply. With Link on, the right side takes the left side's Sync, sixteenths and time,
  and keeps its own offset. The tempo is read at every block, so a tempo change is a change of
  the time and goes through the transition.
- **Band-pass.** A 12 dB/octave Butterworth high-pass at `frequency / 2^(width/2)` and a
  12 dB/octave Butterworth low-pass at `frequency × 2^(width/2)`, in series, each corner held
  between 50 Hz and 18 kHz (and below 0.49 of the sample rate). It has no resonance: with a narrow
  width the two slopes overlap and the passband drops (−1.9 dB at the centre for width 1).
- **Transition.** *Repitch*: the read position glides to the new time at most 0.5 seconds of
  delay per second, rounded off at both ends by a 20 ms one-pole, so the pitch bends by up to half
  while it moves (lengthening reads at half speed, shortening at one and a half). *Fade*: the old
  time fades out and the new one in over 50 ms (raised cosine); while the time keeps moving, a new
  fade starts each time the last one ends, which gives LFO modulation a grainy texture. *Jump*:
  the line is read at the new time at once, in whole frames, and clicks if the sound is not
  silent there.
- **LFO.** One LFO for both sides. Rate is in hertz; Time is the period in seconds; Synced is the
  period from the list above, Triplet two thirds of it and Dotted one and a half; Sixteenths is
  `n` sixteenths of a bar. The synced modes follow the song position while the transport runs, as
  the auto-pan does, and run on at the tempo when it is stopped. Morph moves the middle of the
  cycle: the first half of the waveform takes `(1 + morph) / 2` of the cycle (so a triangle turns
  into a ramp, a square into a pulse); it does nothing to Sample and Hold and Wander. The LFO's
  output passes a 2 ms one-pole so its edges do not click. LFO to Time swings the delay time by up
  to ±25 % of it; LFO to Filter swings the band's centre by up to ±2 octaves.
- **Ping-pong.** The left line takes the mono sum of the input, `(L + R) / 2`, plus the right
  wet signal times Feedback; the right line takes the left wet signal times Feedback. A click
  repeats left, right, left, … at each side's own time.
- **Freeze.** The lines write their wet signal back unchanged (no input, no band-pass, gain 1),
  so what they hold repeats until Freeze is off. Freeze ramps in and out over 20 ms.
- **Dry/Wet.** Linear: `dry × (1 − mix) + wet × mix`. Equal-Loudness: `dry × cos(mix·π/2) +
  wet × sin(mix·π/2)`. The dry signal is not delayed; the device adds no latency.
- **Changes while playing.** Feedback, Dry/Wet, Freeze, the filter switch, centre and width, the
  LFO depths and the mix law ramp over 20 ms; the band is retuned every 4 frames while it moves.
  Every block size gives the same output (the conformance tests check it).

## Panel layout

Built from the SDK's widgets ([custom panels](sdk-custom-ui.md)).

Delay uses independent SDK UI nodes throughout its 944 by 168 logical-pixel panel.
Including the host header and padding, it fits a 200-pixel device slot.
Time and Feedback occupy the left; Tone and Motion follow, with Mix at the far right. The only graph is Tone's filter response. All sections stay visible.

Left and Right timing columns flank the link icon in Time; the link button shares the
vertical center of the timing menus. Beats/ms buttons switch between
explicit note-length menus and millisecond fields. Link mirrors timing edits; each column has a synchronized offset field. Freeze sits
below Feedback and Ping Pong below Mix. Repitch/Fade/Jump remain in Time. Tone exposes filter enable,
centre frequency and octave width; Motion exposes rate mode/value, waveform,
delay/filter depth and morph. Speed appears once: a number in Hz/Time modes and
a ValueMenu in synchronized modes. The obsolete `lfo_expanded` parameter remains hidden for
saved-parameter compatibility and no longer affects presentation.

The host resolves widget text, surfaces, borders and accents from the active Suhina
theme at render time. No authored purple accent or device header is required.

## Import

Live's Delay is the element `Delay`; every value is `<Element>/Manual@Value`
([`als-format.md`](als-format.md) › Devices). `crates/als/src/devices.rs` maps it:

| Element | Stored | Suhina |
|---|---|---|
| `On` | true/false | the device's on/off |
| `DelayLine_SmoothingMode` | 0 Repitch, 1 Fade, 2 Jump | `transition` |
| `DelayLine_Link`, `DelayLine_PingPong` | true/false | `link`, `pingPong` |
| `DelayLine_SyncL`, `DelayLine_SyncR` | true/false | `syncLeft`, `syncRight` |
| `DelayLine_TimeL`, `DelayLine_TimeR` | seconds | `timeLeft`, `timeRight` |
| `DelayLine_SyncedSixteenthL`, `…R` | step index 0 … 7 | `sixteenthsLeft`, `sixteenthsRight` |
| `DelayLine_OffsetL`, `…R` | −0.333 … 0.333 | `offsetLeft`, `offsetRight` |
| `Feedback` | 0 … 0.95 | `feedback` |
| `Freeze`, `Filter_On` | true/false | `freeze`, `filter` |
| `Filter_Frequency`, `Filter_Bandwidth` | Hz, octaves | `filterFrequency`, `filterWidth` |
| `Modulation_TimeMode` | 0 … 5 | `lfoMode` |
| `Modulation_Frequency`, `Modulation_Time` | Hz, seconds | `lfoRate`, `lfoTime` |
| `Modulation_SyncedRate` | 0 … 21, a number the panel shows in steps | `lfoSynced`, rounded |
| `Modulation_Sixteenth` | 1 … 64 | `lfoSixteenths`, rounded |
| `Modulation_Waveform` | 0 … 6 | `lfoShape` |
| `Modulation_Morph` | −1 … 1 | `lfoMorph` |
| `Modulation_AmountTime`, `Modulation_AmountFilter` | 0 … 1 | `lfoToTime`, `lfoToFilter` |
| `DryWet` | 0 … 1 | `mix` |
| `DelayLine_SimpleDelayTime*`, `DelayLine_PingPongDelayTime*` | `100`, `1` | not mapped: no control on the panel |

A stepped value outside its list is left at the default with a warning. Live's Hi-Quality and
Equal-Loudness options (the context menu) are not stored as these elements; where Live keeps
them is not known, so an imported delay starts with the linear mix law.

## Decisions

Each entry: what was chosen, what it costs, why.

1. **Two delay lines with cubic (Catmull-Rom) interpolation, always.** Live offers a Hi-Quality
   switch for better interpolation; Suhina always reads with the cubic, which keeps the top
   octave that linear interpolation dulls in Repitch and Fade. Cost: about 3 ns more per read; the
   device stays under budget. There is therefore no Hi-Quality parameter.
2. **The band-pass is two Butterworth sections inside the loop,** as measured (to 0.03 dB in all
   seven cases). Cost: four biquads per frame.
3. **Repitch is a rate-limited glide (0.5 s of delay per second, 20 ms rounding).** Live's glide
   time was not measured (it needs automation during a render). A rate limit keeps the pitch bend
   between an octave down and a fifth up whatever the jump, which a one-pole glide does not (a long jump would
   read backwards). Candidate run: `delay-lfo-delay` (the transitions under LFO modulation).
4. **Fade is a 50 ms raised-cosine crossfade.** Not measured; "tens of milliseconds" was the
   note's guess. Same candidate run.
5. **The LFO's depths are ±25 % of the time and ±2 octaves of the band at 100 %.** Not measured.
   A depth relative to the time keeps a synced delay's modulation proportional to it. Candidate
   runs: `delay-lfo-delay`, `delay-lfo-filter`.
6. **Morph moves the middle of the cycle; Triplet and Dotted are 2/3 and 3/2 of the synced
   period; Sixteenths is `n`/16 bar.** Not measured (candidate run `delay-lfo-waves`).
7. **Offset scales the whole synced time and only in Sync mode,** as the older Simple Delay did.
   Whether Live's new Delay scales one sixteenth instead is open (candidate run `delay-offset`).
8. **Ping-pong feeds the mono sum of the input into the left line only.** A centred sound then
   still bounces, which the manual describes; a left-only sound is 6 dB quieter in its first
   repeat than without ping-pong. Candidate run: `delay-pingpong`.
9. **Freeze writes the wet signal back with no filter and gain 1.** Live's behaviour under Freeze
   was not measured (it needs a press during the render).
10. **The longest delay is 8 s with the LFO's swing.** A synced time longer than 6.4 s (below about
    38 BPM at 16 sixteenths with +33 %) stops there. Memory: about 2 MB per side at 48 kHz, 16 MB at
    384 kHz.
11. **The time knobs are logarithmic.** Live's time knob has its own curve (157 ms at the middle);
    values import exactly, only the knob's feel differs.
12. **Equal-Loudness is an equal-power law.** Live's law was not measured (candidate run
    `delay-drywet`).

## Known differences

None in what was measured: `compare.py` (the harness's comparison, default tolerances 0.5 dB,
10 %, 0.5 ms) finds **15 of 15** cases of `delay-feedback` and `delay-filter` within tolerance,
for the imported set rendered by `suhina-cli render` at 44.1 kHz. What was not measured is listed
under Decisions with its candidate run.

## Cost

One instance at its defaults: 0.4 % of one core at 48 kHz (60 s rendered in 0.24 s, release
build, Apple M-series), inside the 1 % budget. Run `cargo test --release -p suhina-device-delay
cost -- --ignored --nocapture` to measure it.

## Tests

`devices/delay/src/tests.rs`: the device contract (`device_api::conformance`); impulse tests for
the time, feedback, sync with offset and Link, a tempo change gliding the synced time, ping-pong,
freeze; the band-pass in the loop and held to its range; Repitch's pitch bend, the three
transitions, the LFO's swing (square LFO, Jump), synced LFO position, both mix laws, the longest
time at 8 kHz and 384 kHz; every control moved from one end to the other without a click; the same
repeats at 44.1 and 96 kHz.

`devices/delay/src/golden_tests.rs`: golden renders of eleven scenarios (the defaults, free and
synced times, ping-pong, both LFO kinds, each transition, freeze, the equal-loudness mix), compared
within 1e-6 with the files in `devices/delay/tests/golden`; its README lists them and how to make
them again when a change to the sound is intended.
