---
description: The built-in algorithmic reverb - its parameters, signal flow (input band, early reflections and their families, a measured feedback delay network per Density, chorus, freeze, stereo), how Live's Reverb imports into it, and how close it comes to Live's renders
read_when: reverb, algorithmic reverb, room, decay, early reflections, predelay, feedback delay network, FDN, damping, freeze, diffusion, importing Live's Reverb, Reverb elements
type: spec
status: current
last_verified: 2026-10-31
covers:
  - devices/reverb/**
  - crates/dsp/src/first_order.rs
---

# Reverb

Suhina's `reverb` (`devices/reverb`): an algorithmic room reverb with an input band, early
reflections, a feedback delay network for the late tail with damping in the loop, a chorus on the
network, freeze and a stereo width. Live's Reverb imports into it parameter for parameter
([`device-import.md`](device-import.md)), and the device is built to sound like Live's: its tap
patterns, network lengths and filter laws are read from Live's renders, not designed.

- **Observed with:** Live 12.4.6 at 44.1 kHz; the lengths and their rounding also in Live 12.4.15
  at 48 and 96 kHz. The parameters come from a parameter dump and a saved
  set; the sound from renders of many hundreds of cases (impulses, noise, tones and clicks) analysed
  by the measurement harness of the Suhina device notes (`live-12/audio-effects/29.32-reverb.md`
  and `measurements/29.32-reverb/` in the device-specs repository, which hold the cases, the fits
  and the findings behind every constant here).
- **Sources:** Live 12 Reference Manual, 29.32 Reverb (what each control is for); the measurements
  above (how it sounds). Constants in the code are those measurements, named in their doc comments.

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
| 21 | `size` | Size | 0.222 – 500 (shown as 0.22) | 100 | plain number (a room volume), logarithmic |
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
in -> mono sum -> roll-off -> Low Cut -> High Cut -> predelay line -+-> direct taps (Spin) -+-> families --- Reflect -+
                                                                    |                       +--------------------------+-> Stereo -> Dry/Wet
                                                                    +-> direct taps -> Shape entry -> network --- Diffuse -+
                                                                        (lines, allpasses, damping, Chorus, Freeze)
```

**Everything scales with the room.** The *size factor* is `(Size / 100)^(1/3)`, as for a room's
volume; Size 100 is the *reference room*, in which every delay below is given. Delays are quoted in
frames of 44.1 kHz. At another rate each converts to the session's frames by time first and is
rounded after, so the blocks, the added frame and the hundredth below are the session's frames
(read at 44.1, 48 and 96 kHz).

### Input

- **Mono.** Both sides are summed, `(L + R) / 2`: Live's Reverb gives the same output for a
  left-only and a right-only input.
- **Roll-off.** The input always passes first-order high-passes at 127 Hz and 19 Hz.
- **Input band.** Low Cut is a first-order high-pass at 2.5 times the band's lower edge,
  `frequency / 2^(width/2)`, plus two first-order low shelves at 25 and 250 Hz whose gains follow the
  edge (Design notes). It does nothing with the edge below 44.2 Hz. High Cut is a 12 dB/octave
  Butterworth low-pass at `frequency × 2^(width/2)`, never above 18 kHz. With High Cut off the input
  still passes the same low-pass at 18 kHz, as Live's does (left out where 18 kHz is above 0.45 of
  the rate). Both filters always run and their switches crossfade.
- **Predelay** delays the wet signal. The wet signal, reflections and tail alike, is also quieter
  by `0.0113 / decay` dB per ms of predelay (decay in seconds).

### Size, Smooth and rounding

Every delay after the predelay, except the Shape entry and Stereo's delay, is its reference-room
length times the size factor. **Smooth** glides the
factor with a time constant of 45 ms (Slow) or 15 ms (Fast), or not at all (None), where Size
changes glitch as in Live. However fast the glide, no line or reflection changes by more than half a
frame a frame: faster, a line is read backwards and buzzes.

**Rounding.** Live rounds every delay below that is said to be *rounded* (the network's lines,
allpasses and families, the Shape entry and Stereo's delay) the same way: under Smooth Fast and
Slow (Slow renders as Fast) down to a whole frame, `floor(x)`; under None to whole 8-frame blocks,
up unless it is less than a tenth of a block past one, `8 × floor(x / 8 + 0.9)`. `x` is the length
in the session's frames: at 48 kHz Low's A line is 2499.91 frames per unit of size factor, not a
round 2500, and keeps its fraction under Fast at every rate. While Size or Scale move, a length
follows the straight line through the middle of its steps.

### Early reflections

- **Direct taps.** Each Density has Live's own: 4 at Sparse, the same ten times at Low, Mid and High,
  each with a side and a level. Each time is a whole number of frames plus the fraction of a frame at
  which Live rounds it, and the predelay and a tap's time round to a frame together. The first tap
  arrives one frame before the predelay, at every rate (frame 119 at 48 kHz and 239 at 96 kHz at the
  default 2.5 ms), at a gain of 0.0942.
- **Level.** Every tap is `0.92594 + 0.00074059 × Size` of its reference level (0.926 at the
  smallest Size, 1.296 at 500), and fades by `(0.0328 + 0.0668 × Shape) / decay` dB per ms of its
  delay.
- **Families.** Every other reflection is a direct tap heard again, through a low-pass, after one of
  ten *family* delays, frames to its peak in the reference room (rounded; under Fast 0 to 6 frames
  sooner):

  | Family | A | B | C | D | F | R | E | H | J | K |
  |---|---|---|---|---|---|---|---|---|---|---|
  | Peak (frames) | 401 | 3193 | 1641 | 1337 | 2985 | 2337 | 961 | 2073 | 2393 | 2497 |

  Low's six take the names of its lines; the four only Mid and High have fill the alphabet after
  them, leaving out G (a part of R's line) and I. A family is heard on one side, and each Density
  has a table of how loud each family repeats each direct tap: Low has A, B, C, D, F and R, Sparse
  F, B, R and C, Mid all but H and J, High all ten.
  So the reflections are played as one mix of the direct taps per family, delayed, and fading over
  the family's delay as the taps do. The network hears the same family lines, an entry later (Late
  reverb).
- **The low-pass** is a cookbook low-pass of Q 0.5 per side. Its double pole is that of 11.98 kHz
  plus `1.115 × (1 − exp(−t × size factor / decay))`, with `t` 0.0512 s at Low, 0.2092 at Sparse,
  0.3200 at Mid and 0.2657 at High: the corner falls with a shorter Decay, faster the denser the
  reflections (at Size 100, Low 8.5 kHz at 0.2 s and 11.3 at 1.2 s; Mid 1.4 and 8.4 kHz), and a
  smaller room's reflections are brighter.
- **Stereo's delay.** Below Stereo 120 the reflections heard, not the tail and not the network's
  input, come `0.625 ms × (1 − stereo / 120)` later in the session's frames, rounded (4 and 8 frames
  at the default 100 under Fast and None at 44.1 kHz). Where that is a whole number of frames,
  Fast's is one frame less: at Stereo 0 and 60, 29 and 14 frames at 48 kHz, 59 and 29 at 96 kHz
  (None's 32, 16, 64 and 32).
- **Spin** swings each direct tap's whole delay `T` (predelay included) by
  `0.01 × amount × reach(predelay) × T × sin(2π × m × rate × t + φ)`, where `reach` is 1 up to 5 ms
  of predelay and `2 / (predelay − 3)` above it (predelay in ms), so a long predelay swings every
  tap by about the same 2 ms × 0.01 × amount. Each direct tap has its own LFO, rate `m` times Spin
  Rate (`m` 0.9897, 1.0430, 0.9981, 0.9825, 1.0230, 1.0071, 1.0300, 1.0150, 1.0150 and 0.9917 from
  the first tap to the tenth) but never slower than 0.074 Hz, and a phase `φ` that restarts with the
  transport at 40, 100, 70, 90, 30, 50, 60, 10, 80 and 20°. The LFOs are read 0.15 ms ahead of the
  frame they move. Once the room and the predelay have settled, each tap swings around its own delay
  rounded to a whole frame; while they move, around the exact delay. A family's reflection swings as
  its direct tap did when it was read, and the tail's input with the reflections it comes from.
  Moving reflections are read by linear interpolation, as Live's (they lose 0.4 dB at 6 kHz to
  1.6 dB at 15 kHz as soon as Spin moves them); static ones are read at whole frames.

### Late reverb

Live's network, identified from its renders, with a design per Density. A **line** is a delay and
a Schroeder allpass with Diffusion as its gain (two allpasses in series at Mid and High). Lines are
mixed after every trip by one matrix, orthogonal between the lines as they are heard.

| Density | Lines | Direct taps feed | Sides (left/right) |
|---|---|---|---|
| Sparse | 4 | one line each | 2 / 2 |
| Low | 6 (A, B, C, D, F, R) | six groups of taps | B, D, F / A, C, R |
| Mid | 8 | eight groups | 4 / 4 |
| High | 10 | ten taps | 5 / 5 |

- **Input.** Each line takes its own mix of the direct taps, after the **Shape entry**: `1543.5 ×
  Shape` frames (35 ms a unit), rounded, less a frame, at every Size and Density (−1, 391, 775, 1159
  and 1543 frames at Shape 0 to 1 by quarters under None, −1, 384, 770, 1156 and 1542 under Fast).
  The taps come at their reflection levels and fades, so Shape and Decay shape the tail's onset as
  they shape the reflections. The network hears no side of the input, and the input passes no
  damping. Its level is `0.2945 × Size level × Network level` times each line's input weight, with
  *Size level* the reflections' law above and *Network level*
  `0.935054 + 0.000751857 × Size − 0.000000350508 × Size²`: close to the reflections' law up to Size
  100, 0.45 dB below it at 500.
- **Families.** Each line has a family. Its filtered reflections, read from the same family line a
  Shape entry later, through the same low-pass and 1.70 times quieter than they are heard (at the
  network's level), join that line's output in the mix: Live's reflections and the network's copies
  are one signal at a fixed ratio of 1.69 to 1.71 at every Density. At Low a line's input is its own
  family.
- **Line levels.** Sparse's, Mid's and High's lines are heard at levels up to 1 % apart (Low's at
  one level). The device sums every line at one level and runs the mix between them as
  `level_i × mix_ij / level_j`, so it stays orthogonal between the lines taken at their levels.
- **Lengths.** A line's delay and each allpass are rounded, plus a frame: the delay
  `y × size factor` blocks, an allpass `c × Scale × size factor` (Scale in %), with y and c per line
  (Low) or per family (Sparse, Mid and High: a Mid or High line has its family's delay and both of
  its allpasses; a Sparse line its family's shorter allpass, and the C-family line A's delay). Under
  Fast the delays of the A and B families' lines keep their fraction of a frame, read by linear
  interpolation, most of the highs that setting loses. At the default Scale in the reference room
  Low's A, B, D and F are 57, 71, 113 and 170 ms, C 1640 frames then 83 ms, R 228 ms.
- **Reading.** Under None the A and B families' lines are a hundredth of a frame longer, read by
  linear interpolation, which is all the highs the network loses on a pass. Other lines are whole
  frames and not interpolated, except with Chorus, where they are read with cubic (Hermite)
  interpolation.
- **Decay.** The network is tuned to a T60 of 0.9 of the Decay setting. Every path fades with its
  whole time: each line's gain covers its delay, and its allpass loses its own delay's gain on every
  pass (`y = a·w[n−L] − g·w`, `w = x + g·a·w[n−L]`). With the mix orthogonal, nothing else is lost on
  a pass, so the tail falls by the same dB per second at every Size.

### Damping

In each line, after its gain, with the loss specified per second so it does not depend on Size:

- **High Damping, Shelf:** a second-order high shelf (Q 0.70) half way at 0.98 of the frequency.
  **Low Damping:** a second-order low shelf (Q 0.70) half way at the frequency over 0.98.
  The Gain is a linear factor per 0.17 s trip, raised to the power of the line's length plus its
  allpasses', in seconds, over 0.17 s. Sparse's shelves lose 1.37 times as much.
- **High Damping, Low-Pass:** one whole one-pole low-pass `y += (1 − p)(x − y)` with
  `p = e^(−2π × frequency / sample rate)` on every pass through every line, whatever its length;
  Gain does nothing with it, in Live's renders as here.

### Chorus

Swings every line's delay either way by up to `0.38 ms × amount` (at most half the line), each line
with its own phase of one LFO at the Chorus rate. Each side's lines are spread evenly round the
cycle, the right's half a step after the left's, so their Doppler shifts cancel in what the side
hears.

### Freeze

The loop's gain is set so the tail does not fall at all with Flat on, and 0.03 dB/s with Flat off
(Live's frozen tail falls 0.25 and 0.4 dB/s in all, nearly all of it from A's and B's fractions of a
frame and the high bands). With Flat on the damping is left out while frozen; with Flat off it
stays in. Cut decides whether new input still enters the network (it fades out over 20 ms, the
reflections with it); if it does, levels above 2 are compressed toward 4, so the tail cannot grow
without end. Otherwise the early reflections go on.

### Output

Each side of the tail is the sum of the lines assigned to it, at one level. The reflections (times
Reflect) and the tail (times Diffuse) are added and rotated by `β = 45° × (1 − stereo / 120)`:
`L' = cos β L + sin β R`, `R' = sin β L + cos β R`. So 120 keeps the sides apart, 100 gives a
correlation of about 0.25 (Live's 0.25) and 0 makes them mono. Dry/Wet is an equal-power crossfade,
`dry × cos(mix·π/2) + wet × sin(mix·π/2)`. The dry signal is not delayed; the device adds no
latency.

### Changes while playing

Levels, mix, Stereo, every switch, the Spin and Chorus swings, Diffusion, Shape, Scale, Decay, the
input band and the damping frequencies and gains ramp over 20 ms; Predelay over 100 ms (a long jump
bends the pitch on the way). Delay lengths and gains are recomputed every 32 frames and
interpolated frame by frame in between. Switching Density starts or stops lines at once and the new
lines start from silence, with no crossfade: this was listened to in the app (2026-10-18) and no
click could be heard.

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
| `RoomSize`, `SizeSmoothing` | 0.222 … 500, 0 None / 1 Slow / 2 Fast | `size`, `smooth` |
| `FreezeOn`, `FlatOn`, `CutOn` | true/false | `freeze`, `flat`, `cut` |
| `StereoSeparation` | 0 … 120 | `stereo` |
| `RoomType` | 0 Sparse … 3 High | `density` |
| `MixReflect`, `MixDiffuse` | linear gain 0.03 … 1.995 | `reflect`, `diffuse`: 20·log10 of it, dB |
| `MixDirect` | 0 … 1 | `mix` |

A stepped value outside its list is left at the default with a warning. Sets saved before Live
11.2 named some of these differently; only 12.4.6's names are read.


## Design notes

Why the device is built as it is. Where each constant was read is in its doc comment in the code.

- **Live's network, read from its renders, over a textbook feedback delay network.** A designed
  network (Householder or Hadamard mixing, one allpass per line) reached Live's class of tail but
  could not match its first arrivals, which are as strong as the reflections, nor the texture of its
  onset: Live's echoes stay distinct and sparse for hundreds of milliseconds, where a Hadamard
  network turns to noise in two trips. Impulse responses with Diffusion at its minimum (an allpass
  is then a plain delay), Decay at its maximum and several Scales show each arrival as one sample
  once the input's fixed filters are divided out, and tell lines, allpasses, mix and outputs apart.
  Cost: the constants are fitted values, not derivations; they are exact where read and
  extrapolated by the same laws elsewhere.
- **One rounding for every delay.** Lines, allpasses, families, the Shape entry and Stereo's delay
  all fit the same rule (Size, Smooth and rounding) at every Size, Scale, Density and Smooth read:
  under None it is Fast's length rounded up to whole blocks, but for the last tenth of a block. Each
  length's constant is the middle of what both settings' reads allow. Lengths settle on whole frames
  without interpolation: fractional delays in the loop cut the 8 kHz octave's decay time by 10 to
  38 %. The rounding happens in the session's frames, after the conversion: rounded at 44.1 kHz and
  converted after, Low's A line at Size 100 under None was 3571 frames from its first reflection
  at 48 kHz against Live's 3569, and up to 26 frames long at 96 kHz; converted first, all 30 reads of
  A's and B's first passes at 44.1, 48 and 96 kHz are within 0.01 frame. The 48 and 96 kHz reads
  also narrow constants the 44.1 kHz reads left loose: an allpass's blocks per % moves its length
  2.2 times as many frames at 96 kHz, so Mid's and High's lines there pinned seven constants (A's,
  B's, C's and H's longer allpass, D's delay and shorter allpass, J's shorter allpass) and F's
  family delay, each to the middle of what every read allows.
- **Families are one signal.** A family's reflections and the copy the network hears are a direct
  tap delayed again by the family's delay, through the same low-pass, at a fixed ratio of 1.70. So
  each Density keeps one table of family levels and derives the copies from it. Sparse's, Mid's and
  High's tables are their reflections read tap by tap; Low's are its line inputs, which match Live's
  reflections and tail more closely than its own read taps. One delay line per family serves both:
  the reflections read it at the family's delay, the network an entry later, which renders the same
  as separate copy lines in every comparison set, and costs a delay line per family instead of one
  read per tap and a second set of lines.
- **Tap times keep Live's fraction of a frame.** Live steps each tap to the next frame at its own
  fraction as the predelay moves; without it four taps were a frame late at most predelays. Two
  direct taps (17.90 and 41.12 ms) sit at 0.59 of a frame rather than the 0.65 their predelay steps
  read, so that every direct tap lands on Live's frame at every Size read, 0.222 to 500.
- **Nothing is lost on a pass but the decay.** Live's tail falls a fixed dB per second at every
  frequency the damping does not touch, at every Size. A loss per pass cannot be told apart from a
  loss per second at one Size, but at Size 0.222 it let the tail grow without end. So the mix is
  exactly orthogonal between the lines at their levels, every path fades with its whole time (an
  allpass holds echoes back for several of its delays, so it loses its own), and the one real loss
  of the highs is A's and B's fraction of a frame.
- **Damping is a gain per 0.17 s trip, applied in proportion to each line's length.** The trip is
  the one that fits every band Live was measured at, and it makes the loss per second independent of
  Size. Shelves are second-order: first-order ones had to sit off the set frequency and were 13 to
  28 % off in octave decay time. The Low-Pass is a whole one-pole per pass, because that fits Live's
  decay per third-octave where an analog first-order low-pass needs separate corners. The input
  passes no damping: with an input pass, the first arrivals were 3 dB too dark.
- **Low Cut is a roll-off plus two shelves.** Live's Lo Cut has a first-order corner at 2.33 to 2.5
  times its band edge, and below it a gentle low shelf. Two shelves at 25 and 250 Hz take gains (dB)
  of (10.57, 1.17), (7.09, 1.30), (4.60, 0.71), (2.65, −3.49), (4.38, −7.11) and (7.58, −12.42) at
  edges of 132, 192, 264, 500, 658 and 841 Hz, joined linearly against the edge's logarithm and held
  beyond; they match Live's six edges to within 0.9 dB in every third-octave band. These are fitted
  values, not a law. The fixed 127 Hz and 19 Hz roll-off is the shape of every reflection in Live's
  renders with both cuts off.
- **Spin swings each direct tap's whole delay, by a share that falls with predelay.** Tracked in
  Live's renders of clicks at Predelay 0.5 to 250 ms, Amount 17.5 and 55, three Rates and three
  Sizes, every tap follows a sine of peak `0.01 × amount × reach × T`. Swinging only the delay
  after the predelay left the first tap still and over-swung the late ones at long predelays;
  swinging each tap by its own delay overstated the tone's FM 2.6 times at the default. The
  network's input moves with its reflections, because Live's tail with Spin is uncorrelated with its
  tail without. The LFOs' rates and phases were fitted against the Spin Rate the sets store (0.2977
  Hz at the default, not 0.30); fitted against 0.30 they needed a 1.0053 scale and an 8 ms lead,
  which made up for only part of the error. At 0.07 Hz every tap swings at 0.074 Hz and at 0.15 Hz at the Rate
  itself, so the law is a floor. A tap swung around its fractional delay lands up to half a frame
  from Live's.
- **Linear interpolation for Spin, cubic for the lines.** Live reads Spin's reflections linearly, so
  this device does too. The lines move with Chorus, where linear interpolation would dull and
  modulate the highs for a saving of about 0.8 % of a core.
- **Chorus swings by 0.38 ms per unit of Amount**, fitted so a 1 kHz tone through the tail swings as
  far as through Live's (12 to 343 cents at five Amounts and Rates; Suhina 12 to 397). The loop adds
  each trip's shift to the last, so reading the swing as one line's gave 1.8 ms and a hiss.
- **Decay is tuned to 0.9 of the setting**, which is what Live's renders show (0.87 to 0.9 from 0.8
  to 3.5 s).
- **Smooth's glide times** (45 ms Slow, 15 ms Fast) are what one point of Live's glide gives for an
  exponential glide; the curve itself was not measured.
- **Freeze, Flat and Cut** follow Live's renders per band: with Flat on the frozen tail falls 0.1 dB/s
  at 1 kHz and up to 1.6 dB/s at 16 kHz; with Flat off the damping stays in. The soft limit with Cut
  off is Suhina's.

## How close it is to Live

Residual against Live's renders of the same settings (the difference's energy against Live's, dB;
lower is closer), imported and rendered by `suhina-cli render` at 44.1 kHz, impulse responses with
Smooth None unless the row says Fast:

| Density | Settings | 0–80 ms | 0.08–0.5 s | 0.5–3 s | 3–8 s |
|---|---|---|---|---|---|
| Low | Size 100, every Scale and Shape | −31 to −48 | −31 to −44 | −30 to −43 | −41 to −43 |
| Low | Size 1.53 to 50 | −43 to −48 | −37 to −44 | −36 to −43 | −29 to −43 |
| Low | Size 250 to 500 | −43 to −44 | −43 to −44 | −43 to −44 | −42 to −43 |
| Low | Size 0.222 and 500, None and Fast | −43 to −44 | −37 to −44 | −30 to −44 | −26 to −43 |
| Sparse | Size 100, Scale 5 to 100 % | −42 | −42 | −42 to −43 | −42 |
| Mid | Size 100, Scale 5 to 100 % | −44 | −41 to −47 | −42 | −42 |
| High | Size 100, Scale 5 to 100 % | −44 | −40 to −48 | −41 | −41 |
| Mid, High | Size 300 and 400 | −43 | −40 to −42 | −40 to −42 | −41 to −42 |
| Mid, High | Decay 0.83 to 60 s, Diffusion 0.1 to 60 % | −42 to −44 | −38 to −49 | −36 to −42 | −36 to −42 |
| Sparse, Mid, High | Size 0.222 and 500, None and Fast | −39 to −44 | −31 to −46 | −28 to −42 | −20 to −42 |

Tap by tap, every reflection is within 0.5 ms and 0.5 dB of Live's at every Density, and the
network's level within 0.05 dB from Size 0.222 to 500.

### What is further off

- **Smooth Fast at the ends of the Size range.** Sparse's, Mid's and High's tails drift slowly from
  Live's under Fast, where under None they do not: −29 dB by 3 s at Sparse at Size 0.222 and −20 to
  −29 dB at every Density at Size 500 by 8 s (Low's too, −26 dB).
- **Sparse's tail with the default damping** (`reverb-er-decay`, Decay 0.83 to 60 s): −26 dB from
  0.5 s and −18 to −23 dB from 3 s, against −36 at the other Densities. At the default damping the 8
  to 16 kHz bands of Mid and High decay 1.4 to 1.6 dB/s too slowly.
- **Low beyond the settings read.** Between Size 100 and 250 only Size 200's reflections were read.
  The Shape entry was read at Size 100 and Density Low only; that it holds at every Size is inferred
  from Shape 0.5's being the same at every Size.
- **Stereo's delay** was read at 44.1, 48 and 96 kHz, and not while Stereo is automated. Why Fast's
  is a frame short where it is whole is not known; the device floors it a millionth of a frame
  low.
- **Other rates.** Low's lines, the reflections' times, the Shape entry and Stereo's delay were read
  at 48 and 96 kHz and follow the laws above. Mid's and High's lines were read line by line there
  at Scale 40 % and Diffusion 0.1 %: they follow the same law once seven of their constants are
  narrowed within what the 44.1 kHz reads allow (Design notes › One rounding for every delay), and
  are then −47 to −48 dB from Live's under None. Still off at these rates:
  - E's line under Fast is 1 frame shorter than this device's at 48 kHz and 2 at
    96 kHz, which no length the 44.1 kHz reads allow gives; Mid and High under Fast stay −18 to
    −21 dB from Live's from 80 ms on, −47 dB with that line shortened.
  - At Diffusion 60 % and Scale 100 %, Mid's and High's tails are −1 to −8 dB from Live's from
    80 ms on. No constant Scale 40 % allows makes them match: at Scale 100 % D's shorter allpass
    would need 1.7374 blocks per % at 48 kHz, where Scale 40 % at 48 and 96 kHz allows 1.7351 to
    1.7353. How an allpass's length depends on Scale (or Diffusion) at these rates is not known.
- **Low Cut** is extrapolated for edges between 44.2 and 132 Hz and above 841 Hz.
- **Spin.** Clicks through the moving reflections are about −41 dB from Live's at Amount 17.5 and
  Predelay 100 ms, at 0.07 Hz too, and white noise −21 to −27 dB at the default. Still off:
  - At Amount 55 and a short predelay the reflections stay about −12 dB from Live's (−13.5 dB at
    0.5 ms), though the taps' arrivals are within 0.33 frame rms of Live's with no lag left that
    follows their speed. The interpolation of fast-moving reads or the families may be the cause.
  - Where a tap's delay is exactly half a frame (Predelay 25 ms, 1102.5 frames), Live rounds taps 0
    and 9 down and tap 1 up, though their fractions match; this device rounds all three up. The
    first window at Predelay 25 ms is −12 and −9 dB from Live's, and Spin off at 5 ms of predelay
    differs the same way. It may come from float precision in Live's tap times.
  - The tail with Spin (`reverb-network-spin`) stays −10 to −13 dB from Live's in every window,
    the first 0.08 s included.
  - The 0.074 Hz floor rests on 0.07 and 0.15 Hz only, and the 0.15 ms lead may belong to the input
    filters' delay instead (High Cut was off where it was read).
  - Live's moving reflections carry a flat floor of noise from 16 to 22 kHz that follows Spin's
    speed (−50 dB at the default to −29 dB at the extreme, against 1 kHz), which this device's reads
    do not make.

  Without Spin, a 1 kHz tone through Low is −43 dB from Live's; a stronger R family from the sixth
  direct tap would bring it to −44 dB but takes impulses up to 4.5 dB further from Live's.
- **Echo texture.** The tail's echoes thicken differently from Live's: kurtosis 26 from 100 to 150 ms
  and 5.8 from 150 to 200 against Live's 15 and 10.5.
- **Smooth None against Fast at Decay 3.46 s** (Sparse, Mid, High): Live's two settings differ by up
  to 2 dB by 2.4 s; this device follows Fast, the preset's setting, there.
- **Long and small-room highs** (Sparse, Mid, High): a fixed shelf per trip cannot follow Live's
  highs, which fall fastest in the first seconds and more slowly after.

## Cost

One instance at its defaults: 1.9 % of one core at 48 kHz (60 s rendered in 1.13 s, release build,
Apple M-series, 2026-10-31), over the 1 % budget as the device notes allow for reverbs: ten direct
taps, up to ten family lines (heard and fed to the network), and 4 to 10 modulated lines with one allpass (two at Mid and High) and
damping each, every frame. Run `cargo test --release -p suhina-device-reverb cost -- --ignored
--nocapture` to measure it.

## Tests

`devices/reverb/src/tests.rs` checks the device contract and what the reverb must do against these
laws: decay times, levels, Stereo, the mix, the reflections' times, fades, Spin and families,
Freeze and Cut, and that no control clicks. `devices/reverb/src/network.rs` checks every measured
length, entry and family delay against Live's reads. `devices/reverb/src/golden_tests.rs` compares
nine scenarios within 1e-6 with the renders in `devices/reverb/tests/golden`, whose README lists
them and how to make them again when a change to the sound is intended.
