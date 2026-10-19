---
description: The built-in algorithmic reverb - its parameters, signal flow (input band, early reflections, a damped feedback delay network, chorus, freeze, stereo), how Live's Reverb imports into it, and how close it comes to Live's renders
read_when: reverb, algorithmic reverb, room, decay, early reflections, predelay, feedback delay network, FDN, damping, freeze, diffusion, importing Live's Reverb, Reverb elements
type: spec
status: current
last_verified: 2026-10-18
covers:
  - devices/reverb/**
  - crates/dsp/src/first_order.rs
---

# Reverb

Suhina's `reverb` (`devices/reverb`): an algorithmic room reverb with an input band, early
reflections, a feedback delay network for the late tail with damping in the loop, a chorus on the
network, freeze and a stereo width. It is designed so that Live's Reverb imports into it
parameter for parameter ([`device-import.md`](device-import.md)).

- **Observed with:** Live 12.4.6, 44.1 kHz. The parameters come from the parameter dump and a
  saved set; the sound from five behaviour runs of 98 cases (`reverb-structure`, `-decay`,
  `-filters`, `-texture`, `-size`), rendered in Live and analysed by the measurement harness of
  the Suhina device notes (`live-12/audio-effects/29.32-reverb.md` and
  `measurements/29.32-reverb/` in the device-specs repository; candidate runs not made yet are in
  its `runs.py`).
- **Sources:** Live 12 Reference Manual, 29.32 Reverb (what each control is for); the
  measurements above (how it sounds); the feedback delay network of Jot and Chaigne (1991) and
  the Schroeder allpass, textbook designs. The delay lengths, the tap pattern and every constant
  are this device's own; nothing is copied from Live's renders but the measured laws below.

## Parameters

Values are plain values in their unit (ADR 0004). Ids are stable.

| # | Id | Name | Range | Default | Unit, scale |
|---|---|---|---|---|---|
| 0 | `predelay` | Predelay | 0.5 – 250 ms | 2.5 ms | seconds, logarithmic |
| 1 | `lowCut` | Low Cut | off/on | off | toggle |
| 2 | `highCut` | High Cut | off/on | on | toggle |
| 3 | `inputFrequency` | Input Frequency | 50 Hz – 18 kHz | 830 Hz | hertz, logarithmic |
| 4 | `inputWidth` | Input Width | 0.5 – 9 octaves | 5.85 | octaves, linear |
| 5 | `spin` | Spin | off/on | on | toggle |
| 6 | `spinRate` | Spin Rate | 0.07 – 1.3 Hz | 0.3 Hz | hertz, logarithmic |
| 7 | `spinAmount` | Spin Amount | 2 – 55 | 17.5 | plain number, logarithmic |
| 8 | `shape` | Shape | 0 – 1 | 0.5 | plain number, linear |
| 9 | `highDamping` | High Damping | off/on | on | toggle |
| 10 | `highDampingType` | High Damping Type | Shelf, Low-Pass | Shelf | choice |
| 11 | `highDampingFrequency` | High Damping Frequency | 20 Hz – 16 kHz | 4.5 kHz | hertz, logarithmic |
| 12 | `highDampingGain` | High Damping Gain | 0.2 – 1 | 0.7 | linear factor |
| 13 | `lowDamping` | Low Damping | off/on | off | toggle |
| 14 | `lowDampingFrequency` | Low Damping Frequency | 20 Hz – 15 kHz | 90 Hz | hertz, logarithmic |
| 15 | `lowDampingGain` | Low Damping Gain | 0.2 – 1 | 0.75 | linear factor |
| 16 | `diffusion` | Diffusion | 0.1 – 96 % | 60 % | percent, linear |
| 17 | `scale` | Scale | 5 – 100 % | 40 % | percent, linear |
| 18 | `chorus` | Chorus | off/on | on | toggle |
| 19 | `chorusRate` | Chorus Rate | 0.01 – 8 Hz | 0.02 Hz | hertz, logarithmic |
| 20 | `chorusAmount` | Chorus Amount | 0.01 – 4 | 0.02 | plain number, linear |
| 21 | `size` | Size | 0.22 – 500 | 100 | plain number (a room volume), logarithmic |
| 22 | `smooth` | Smooth | None, Slow, Fast | Fast | choice |
| 23 | `decay` | Decay | 0.2 – 60 s | 1.2 s | seconds, logarithmic |
| 24 | `freeze` | Freeze | off/on | off | toggle |
| 25 | `flat` | Flat | off/on | on | toggle |
| 26 | `cut` | Cut | off/on | on | toggle |
| 27 | `stereo` | Stereo | 0 – 120 | 100 | plain number (degrees), linear |
| 28 | `density` | Density | Sparse, Low, Mid, High | Low | choice |
| 29 | `reflect` | Reflect | −30.46 – +6 dB | 0 dB | decibels, linear |
| 30 | `diffuse` | Diffuse | −30.46 – +6 dB | 0 dB | decibels, linear |
| 31 | `mix` | Dry/Wet | 0 – 100 % | 55 % | percent, linear |

Ranges, defaults and step lists are Live's (the defaults are its factory preset). Reflect and
Diffuse go down to a linear gain of 0.03, which Live shows as −30 dB.

## Behaviour

```text
in -> mono sum -> 90 Hz roll-off -> Low Cut -> High Cut -> predelay line -+-> 32 early-reflection taps (Spin) ---- Reflect -+
                                                        |                                               +-> Stereo -> Dry/Wet
                                                        +-> Shape delay -> 4 allpasses (Diffusion, Scale)  |
                                                            -> feedback delay network (Size, Decay,      |
                                                               damping, Chorus, Freeze) ----- Diffuse -+
```

- **Mono input.** Both sides are summed, `(L + R) / 2`: Live's Reverb gives the same output for a
  left-only and a right-only input.
- **Low roll-off.** The input always passes a first-order high-pass at 90 Hz: Live's reverb,
  reflections and tail alike, holds 3 to 8 dB less at 63 Hz and 2 to 3 dB less at 125 Hz than its
  input, with Lo Cut off.
- **Input band.** Low Cut is a first-order shelf 14 dB deep, half way at
  `frequency / 2^(width/2)`; High Cut a 12 dB/octave Butterworth low-pass at
  `frequency × 2^(width/2)`, left out when that is above 20 kHz or 0.45 of the sample rate. Both
  always run and their switches crossfade.
- **Size.** Every delay of the reverb (taps, Shape delay, diffusers, lines) is its length in the
  reference room times `(size / 100)^(1/3)`, as for a room's volume. Smooth glides that factor
  with a time constant of 0.5 s (Slow) or 0.1 s (Fast), or not at all (None), which makes Size
  changes glitch as in Live.
- **Early reflections.** 32 taps over 131 ms (reference room) after the predelay, each on one side,
  from the predelay line. The first tap is at the predelay with a gain of 0.064 (−23.9 dB). The
  taps fade by `(0.020 + 0.055 × shape) × 1.2 s / decay` dB per ms after the first, and the whole
  pattern is 0.057 dB quieter per ms of predelay; the taps after the first are 2 dB louder than
  that law gives, which matches Live's energy in its first 150 ms (it has more, smaller taps). Spin swings every tap's delay either way by up
  to `0.7 ms × amount` (the earliest taps by at most 0.9 of their delay, so they never read ahead
  of the input), each with its own phase of one LFO at the Spin rate.
- **Network input.** From the predelay line after the Shape delay, `10 + (14 + 34 × (shape −
  0.5)) × size factor` ms, then four Schroeder allpasses in series with the Diffusion as their
  gain and delays of 4.7, 7.3, 11.1 and 16.9 ms times Scale and the size factor. Its level is
  `1.525 × 10^(−3 × 0.027 s / decay)`, so short decays start quieter, as Live's do; in rooms
  larger than the reference it rises with the size factor to the power 0.75, and Density Sparse
  and High are trimmed by −4 and +1 dB, so the tail is as loud in every room and Density as
  Live's. Freeze with Cut on fades it out (20 ms).
- **Network.** 4, 8, 12 or 16 lines (Density), 33 to 140 ms long in the reference room, mixed after
  every trip by an orthogonal matrix in which every line feeds every other one equally (a Hadamard
  matrix, for 12 lines a Hadamard matrix of 4 within three groups and a Householder reflection
  across them, followed by a shift of one line). The input goes into every line, the longer ones
  taking more of it so the tail builds up over about 100 ms. Each line's gain per trip is
  `10^(−3 × length / (0.9 × decay))`, so every line decays in the same time whatever its length:
  the tail falls 60 dB in 0.9 of the Decay setting, as Live's mid bands do.
- **Damping.** In each line, after its gain: High Damping as a second-order high shelf (the
  cookbook shelf, Q 0.707, half way at the frequency) or a first-order low-pass at the frequency;
  Low Damping as a second-order low shelf (the same, below the frequency). Their gain is a linear factor per trip round a
  0.17 s loop, raised to the power of the line's length (in the room as it is) over 0.17 s, so
  the loss per second does not depend on Size. A fixed loss of the highs is
  always in the loop: a first-order high shelf half way at 8 kHz, 0.96 dB per 0.17 s trip, less for
  decays over 3.46 s (by the cube root of the ratio). Lines are whole frames long once Size and
  Decay have settled, so a line without chorus is not interpolated.
- **Chorus.** Swings every line's delay either way by up to `1.8 ms × amount` (at most half the
  line), each with its own phase of one LFO at the Chorus rate; lines are read with cubic
  interpolation.
- **Output.** The left and right tail are sums of the lines with two orthogonal sign patterns
  chosen so lines of similar length carry the same energy into each side: the two sides come out
  uncorrelated. The reflections (times Reflect) and the tail (times Diffuse) are added and rotated
  by `β = 45° × (1 − stereo / 120)`: `L' = cos β L + sin β R`, `R' = sin β L + cos β R`. So 120 keeps
  them apart, 100 gives a correlation of 0.26 and 0 makes them mono. Dry/Wet is an equal-power
  crossfade, `dry × cos(mix·π/2) + wet × sin(mix·π/2)`. The dry signal is not delayed; the device
  adds no latency.
- **Freeze.** Every line's gain goes to 1 and the fixed loss of the highs is left out; with Flat on
  the damping is left out too, with Flat off it goes on taking its bands away. Cut decides whether
  new input still enters the network; if it does, levels above 2 are compressed toward 4 so the
  tail cannot grow without end. The early reflections go on.
- **Changes while playing.** Levels, mix, Stereo, every switch, the Spin and Chorus swings,
  Diffusion, Shape, Scale, Decay, the input band and the damping frequencies and gains ramp over
  20 ms, Predelay over 100 ms (a long jump bends the pitch on the way); delay lengths and gains are recomputed every 32 frames and interpolated frame
  by frame in between. Switching Density starts or stops lines at once, and the mixing matrix
  changes with them; the new lines start from silence. This is deliberate and has no crossfade:
  changing Density while playing was listened to in the app (2026-10-18) and no click could be
  heard. The `size-change` render covers the moment (Density Mid at 1 s).

## Import

Live's Reverb is the element `Reverb`; every value is `<Element>/Manual@Value`
([`als-format.md`](als-format.md) › Devices). `crates/als/src/devices.rs` maps it:

| Element | Stored | Suhina |
|---|---|---|
| `On` | true/false | the device's on/off |
| `PreDelay`, `DecayTime` | ms | `predelay`, `decay` (seconds) |
| `BandHighOn`, `BandLowOn` | true/false | `lowCut`, `highCut`: the names are the other way round |
| `BandFreq`, `BandWidth` | Hz, octaves | `inputFrequency`, `inputWidth` |
| `SpinOn`, `EarlyReflectModFreq`, `EarlyReflectModDepth` | flag, Hz, 2 … 55 | `spin`, `spinRate`, `spinAmount` |
| `DiffuseDelay` | 0 … 1 | `shape` |
| `ShelfHighOn`, `HighFilterType`, `ShelfHiFreq`, `ShelfHiGain` | flag, 0 Shelf / 1 Lowpass, Hz, 0.2 … 1 | `highDamping`, `highDampingType`, `highDampingFrequency`, `highDampingGain` |
| `ShelfLowOn`, `ShelfLoFreq`, `ShelfLoGain` | flag, Hz, 0.2 … 1 | `lowDamping`, `lowDampingFrequency`, `lowDampingGain` |
| `AllPassGain`, `AllPassSize` | 0.001 … 0.96, 0.05 … 1 | `diffusion`, `scale` |
| `ChorusOn`, `SizeModFreq`, `SizeModDepth` | flag, Hz, 0.01 … 4 | `chorus`, `chorusRate`, `chorusAmount` |
| `RoomSize`, `SizeSmoothing` | 0.22 … 500, 0 None / 1 Slow / 2 Fast | `size`, `smooth` |
| `FreezeOn`, `FlatOn`, `CutOn` | true/false | `freeze`, `flat`, `cut` |
| `StereoSeparation` | 0 … 120 | `stereo` |
| `RoomType` | 0 Sparse … 3 High | `density` |
| `MixReflect`, `MixDiffuse` | linear gain 0.03 … 1.995 | `reflect`, `diffuse`: 20·log10 of it, dB |
| `MixDirect` | 0 … 1 | `mix` |

A stepped value outside its list is left at the default with a warning. Sets saved before Live
11.2 named some of these differently; only 12.4.6's names are read.

## Decisions

Each entry: what was chosen, what it costs, why.

1. **A feedback delay network of 8 lines (Density Low) with input allpasses and a 32-tap
   early-reflection stage,** over a Schroeder reverb of combs: a denser, smoother tail with the
   same decay at every frequency the damping does not touch, in the same class as Live's (whose
   manual describes filters and modulated delay lines). Cost: see 15. Dragonfly Reverb (GPL-3.0),
   the ROADMAP's earlier candidate, was not ported: its algorithms and controls do not map onto
   Live's stages one to one, and an own network of textbook parts, tuned to the measurements, is
   smaller and needs no port.
2. **Hadamard mixing (with a shift) over a Householder reflection.** At 8 and more lines a
   Householder reflection sends most of each line back into itself (three quarters at 8 lines),
   so the lines keep unequal energy and the echo density builds slowly; a Hadamard matrix sends
   every line into every other one equally. Cost: N·log N additions instead of 2N.
3. **The two output sign patterns** are chosen so lines of similar length enter the sides once
   with the same and once with opposite signs. With plain alternating signs the sides came out
   correlated (−0.18 against Live's +0.22 at the default Stereo, which is Live's uncorrelated tail
   after its rotation).
4. **Cubic interpolation for the 32 taps, the diffusers and the lines.** Linear interpolation
   would save about 0.8 % of a core but dulls and modulates the highs while Spin or Chorus moves.
   Without Spin, and with nothing moving, the taps are read at whole frames.
5. **The tail is tuned to 0.9 of the Decay setting** and its level follows
   `10^(−3 × 0.027 s / decay)`: both are what Live's renders show (0.87 to 0.9 of the setting
   from 0.8 to 3.5 s).
6. **The damping is a gain per trip round a 0.17 s loop** (the loop length that fits every
   measured band of Live's), applied to each line in proportion to its length in the room as it
   is, so it sets the same decay per band whatever the line and the Size. Live's renders keep the
   top octaves' decay with Size; a loss per trip would shorten it in small rooms. The shelves
   are second-order: first-order ones, the simpler choice, had to sit off the set frequency to fit
   it and still left the octave band either side 13 to 28 % off in decay time; second-order ones
   at the set frequency fit within 8 %. Cost: two biquads instead of two first-order sections per
   line. The low-pass type is first-order, as measured.
7. **A fixed 90 Hz roll-off at the input,** first-order, the simplest that brings the lowest two
   octaves within about 2 dB of Live's in every case (they were 3 to 8 dB too loud without it).
8. **The fixed loss of the highs** (an 8 kHz shelf, 0.96 dB a trip, less above 3.46 s) is there
   because Live's tail is shorter above 2 kHz even with both filters off.
9. **Low Cut is a 14 dB first-order shelf** half way at the band's lower edge. Live's Lo Cut fits
   no simple filter (3 to 8 dB/octave, flattening at the bottom); at 1 kHz, width 5.85 the shelf is
   within 0.6 dB rms of its measured response (1.3 dB at worst, at 57 Hz), and at 5 kHz it is
   0.6 dB above Live's −3 dB point and 1.3 dB above its −6 dB point.
10. **Spin and Chorus depths** are 0.7 ms and 1.8 ms of peak delay swing per unit, as measured
   (to about 20 %), either way round the tap or line, so the pattern's timing does not move on
   average. Every tap and line has its own phase, so they never move together.
11. **Level corrections for large rooms and for Density** (the size factor to the power 0.75
    above the reference room; −4 dB for Sparse, +1 dB for High) are fitted to Live's renders: a
    larger network spreads the same energy over longer lines and comes out quieter, which Live's
    does not.
12. **The tap pattern, line lengths and diffuser delays are this device's own.** Live's exact tap
   times could be read from the renders, but copying them would copy its sound design rather than
   its behaviour; the measured laws (times with the cube root of Size, fade with Shape and Decay,
   first tap at the predelay) are kept.
13. **Smooth's glide times (0.5 s and 0.1 s) are a choice.** Live's were not measured: the export
    ignores a Size set through the API while Smooth is on, a quirk of the export that Suhina does
    not copy. Candidate run: `reverb-smooth-slow`.
14. **Freeze, Flat and Cut** follow the manual; Live's behaviour was not measured (it needs a press
    during a render). The soft limit with Cut off is Suhina's, so a frozen tail fed for minutes
    cannot overflow. Candidate run: `reverb-freeze-static`.
15. **Cost: about 3.3 % of a core at the defaults,** over the 1 % budget, as the device notes allow
    for reverbs: 32 cubic taps moving with Spin and 8 modulated lines with three to four filters
    each run every frame. Spin off saves a third (2.1 %); Density Sparse costs 2.8 %, High 4.5 %.
    Cutting further would mean fewer taps or linear interpolation (Decisions 4).

## Known differences

Measured against Live's renders of the same sets imported and rendered by `suhina-cli render` at
44.1 kHz: energy in five windows of the impulse response (0–0.15, 0.15–0.5, 0.5–1, 1–2, 2–4 s),
the decay time overall and in octave bands, and the correlation of the sides.

Of the 93 cases with the device on (98 less the five bypasses), 14 are left out: Live's export ignored Size while Smooth was Fast, so the Size,
Diffusion and Scale cases of `reverb-structure`, `reverb-decay` and `reverb-texture` ran at Size
100 (`reverb-size` repeats them with Smooth None, and those are compared). Over the other 79:

| Measure | Within | Median |
|---|---|---|
| Energy in the five windows (290) | 191 within 1 dB, 266 within 2 dB | 0.7 dB |
| Decay time, whole signal (79) | 68 within 10 % | 0.98 of Live's |
| Decay time per octave band, 125 Hz – 16 kHz (447 that both fit) | 302 within 10 %, 378 within 20 % | |
| Tone: noise response in octave bands, 63 Hz – 16 kHz (657) | 366 within 1 dB, 539 within 2 dB, 626 within 3 dB | 0.9 dB |
| Correlation of the sides | 0.17 – 0.25 against Live's 0.20 – 0.26 at Stereo 100; 1.00 and 0.69 at 0 and 60, as Live | |

`compare.py` at its default tolerances finds 5 of 103 within tolerance (the five bypasses): it compares the 16 strongest taps one by one (time within 0.5 ms), the largest
difference anywhere in the 1/6-octave responses and the sine's stereo levels, which for a
reverb depend on the fine structure of its own delays. These cannot match without copying
Live's delay lengths (Decisions 12), and are not used.

What is further off, and why it is left:

- **Low Cut at high settings:** with the band's lower edge above about 500 Hz (Input Frequency
  5 kHz, or Width 0.5 at 1 kHz) the lowest octaves are 15 to 25 dB louder than Live's: the shelf
  stops at −14 dB while Live's Lo Cut goes on falling. Its law is not understood (Decisions 9).
- **Spin:** the reflections' tone with Spin on is 2 to 8 dB under Live's between 250 Hz and
  4 kHz, and at Amount 55 more of their energy falls after 150 ms (+8 dB from 0.15 to 0.5 s):
  Live's swing at large amounts is smaller than 0.7 ms per unit, or not a plain delay swing.
- **The decay of the early reflections alone** (Diffuse at its minimum) reads 0.56 to 0.9 of
  Live's in `reverb-size`: the 32 taps end at 131 ms where Live's reflections thin out more slowly.
- **Short decays (0.2 s):** 2 to 3.5 dB too much from 0.15 to 0.5 s and up to 17 % longer.
- **Size 100 and 500 in `reverb-size`:** 1.5 to 2.5 dB too little after 0.5 s (decay 0.95 of
  Live's). The same settings in `reverb-decay`, where Live's render is 1 to 1.6 dB quieter than
  in `reverb-size`, match within 0.7 dB; part of this is Live's run-to-run difference.
- **Shape 0:** 2 to 2.7 dB too little after 0.5 s.
- **High damping as a low-pass:** the bands above 2 kHz decay faster than Live's (0.47 to 0.8 of its
  decay time), where Live's are near the limit of the measurement.
- **Density Mid:** decay 0.87 of Live's; the others within 3 %.
- **Not measured at all:** Freeze, Flat, Cut, Smooth's glide and anything moving during a render
  (Decisions 13 and 14).

## Cost

One instance at its defaults: 3.3 % of one core at 48 kHz (60 s rendered in 1.96 s, release build,
Apple M-series); 2.1 % with Spin off, 2.8 % at Density Sparse, 4.5 % at High; see Decisions 15. Run `cargo test --release -p suhina-device-reverb cost
-- --ignored --nocapture` to measure it.

## Tests

`devices/reverb/src/tests.rs`: the device contract (`device_api::conformance`); the mixing matrix
orthogonal and full for every Density; the decay time at 1 and 3 s within 10 % at 44.1 and 96 kHz
alike; the same level at both rates; Stereo's correlation at 0, 100 and 120; Reflect and Diffuse
independent; the dry signal undelayed and the mix equal-power; the first reflection at the
predelay; Freeze holding the tail and Cut keeping new input out; silence decaying to exact zeros;
every control moved from one end to the other without a click.

`devices/reverb/src/golden_tests.rs`: golden renders of nine scenarios (the defaults, no
modulation, the densities, damping, Chorus and Spin, Reflect and Diffuse, Freeze with and without
Cut, a size change), compared within 1e-6 with the files in `devices/reverb/tests/golden`; its
README lists them and how to make them again when a change to the sound is intended.
