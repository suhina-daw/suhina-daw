---
description: A held (bowed, blown, sung, whistled) note continued past the end of its recording by a stochastic harmonic model fitted to its held part, in stereo, with width and vibrato controls, transposition that keeps the spectral envelope, and a morph between two models
read_when: a note held longer than its recording, sustain beyond the sample, continuing a cello or flute note, stochastic harmonic model, phase-locked demodulation, diffuse partials, AR process, side signal, stereo width, vibrato depth, vibrato rate, formant-preserving transposition, morph between instruments
type: spec
status: planned
last_verified: 2026-11-27
covers:
  - crates/dsp/src/continuation.rs
  - crates/dsp/src/continuation/**
  - crates/instrument-analysis/src/continuation.rs
  - crates/instrument-analysis/src/continuation_tests.rs
  - crates/device-sdk/src/sample_continuation.rs
  - crates/device-sdk/src/sample_continuation_tests.rs
---

# A held note continued past its recording

A sampled cello note stops when its recording stops. A loop of its held part is heard as a loop
and other holds were set aside ([Sampler](sampler.md) › Sustain): they all play the recording's
own moments again. This document continues a held note with **new material of the same kind**: a
model fitted to the recording's held part generates, for as long as the key is held, a tone whose
pitch, vibrato, partial levels, partial roughness, noise and stereo image have the statistics of
the held part, starting exactly where the recording is left.

The model is a sum of harmonic partials on one shared pitch track. Each partial is a fixed
complex level (its **carrier**) plus a narrowband random process (its **diffuse part**), the
bow's or breath's irregularity that makes a partial a band rather than a line. Pitch is the mean
pitch plus a vibrato and a small jitter. What the partials do not explain is a shaped noise. This
is the deterministic-plus-stochastic decomposition (Serra) with each partial widened by its own
modelled noise (bandwidth-enhanced partials, Fitz and Haken), measured by phase-locked
(heterodyne) demodulation (Moorer).

It reproduces measured statistics of one performance. It is not a physical model of a bow or a
breath, and it does not decay: struck and plucked recordings (piano, guitar) need a decaying
model and get none.

It belongs to the Instrument's Sample layer: **Continue**, the fifth Sample playback, plays the
recording like Sustain and lets the continuation take over where Sustain would hold in segments.
`instrument.fitContinuation` fits it on the loop region, `instrument.acceptContinuation` stores it
on the Instrument (below).

## What is fitted

The fit runs on the analysis worker on the recording at its own sample rate (8..384 kHz), mono
(the mean of its channels) for everything but the side (below). The **held region** `[t0, t1]` is
the Instrument's loop region (the marker further right its end), at least 0.4 s; of a longer one
the last 4 s are analyzed. The pitch range is `fmin = root / 1.3`, `fmax = root * 1.3` around the
root key's frequency (`440 * 2^((root - 69) / 12)` Hz). 0.15 s on each side of the region (or what
the recording has) is analyzed with it and dropped, so the filters' edges fall outside. Progress
is reported in eight steps, and returning false from it cancels.

1. **Pitch track.** A normalized cross-correlation pitch track (50 ms windows every 4 ms, the
   best lag between `sr / fmax` and `sr / fmin`, refined by a parabola through its neighbours),
   interpolated linearly to the sample rate and clipped to `[fmin, fmax]`, gives `f0(t)`. A region
   whose windows correlate with themselves a period later by less than 0.5 (the median) has no
   pitch, and the fit fails. The track is refined three times: with
   `phi(t) = 2 pi integral f0`, each of the first eight harmonics is demodulated
   (`a_h = 2 LP(x exp(-j h phi))`, `LP` a third-order Butterworth low-pass at 60 Hz run forward
   and backward); the slope of each harmonic's unwrapped phase divided by `h`, weighted by
   `|a_h|^2` and low-passed at 12 Hz, is added to `f0`.
2. **Partials.** `H = floor(0.45 sr / median f0)` harmonics (at most 160) are demodulated on the
   refined `phi` with the same 60 Hz low-pass, added back for the residual and kept decimated to
   the control rate (every `floor(sr / 300)`-th sample). The mean pitch `f0m` is the geometric
   mean of `f0` over `[t0, J]`; `Psi(t) = phi(t) - 2 pi f0m t` is the phase the pitch deviation
   has added.
3. **Vibrato and jitter.** The deviation in cents, `1200 log2(f0 / f0m)`, low-passed at 12 Hz, is
   fitted by least squares with a sine of 3.5..9 Hz (0.02 Hz steps): its rate `vr`, depth `vD`
   (peak cents) and phase. The rest (deviation minus the low-passed curve) is the **jitter**,
   decimated to 300 Hz and fitted with a real AR(4) model (Yule-Walker); its standard deviation
   is kept.
4. **Vibrato participation.** A partial does not always follow the pitch fully: on a cello the
   measured share was 0.2..0.8 for the strong partials. Per harmonic, the frequency of `a_h`
   (its phase advance per control frame) is regressed on `h (f0 - f0m)`, weighted by `|a_h|^2`;
   `gamma_h = 1 + slope`, clipped to 0..1.3 and replaced by the median of itself and its two
   same-parity neighbours. Each `a_h` is multiplied by `exp(j (1 - gamma_h) h Psi)`, so what
   remains is the partial's own level and phase movement.
5. **Carrier and diffuse part.** The partials are decimated to 300 Hz. Per harmonic the complex
   least-squares fit of `a_h` on `[1, v, v' / omega]` (the fitted vibrato and its derivative)
   gives the carrier `Mh` (the coupling terms are fitted, but the approved hold plays them at 0;
   the level shaping below gives the vibrato-locked level movement instead). The diffuse part is `c_h = a_h - Mh`. Its spectrum
   (Welch, 128-point Hann frames, hop 32), averaged above harmonic 12 with its same-parity
   neighbours two on each side at half weight, sets a complex AR(6) model with `c_h`'s power
   (Yule-Walker on the spectrum's autocorrelation; poles pulled inside radius 0.985). Its
   **bandwidth** is half the width where the spectrum stays within 10 dB of its peak, at least
   1 Hz. The last 30 frames of `c_h` are kept as its **history**, to continue from.
6. **Level statistics.** Per harmonic the mean of `ln |a_h|` and the standard deviation of its dB
   level over the region (`sigma_h`).
7. **Residual noise.** The signal minus the resynthesized partials, in 4096-point Hann frames
   with half overlap: its mean power spectrum and RMS.
8. **Side.** For a stereo recording the side `s = (L - R) / 2` is demodulated on the **mid's**
   `phi`, so pitch, vibrato and jitter stay shared, and corrected by the same `gamma_h`. Per
   harmonic the side partial is a complex multiple of the mid's (a panning: level and phase
   difference), `rho_h = sum(s_h conj(a_h)) / sum |a_h|^2`, plus an independent diffuse part
   `e_h = s_h - rho_h a_h`, fitted like `c_h` (its own AR(6), power, bandwidth and history). The
   side's residual noise is the mid's noise spectrum times the side-to-mid power ratio, smoothed
   in log frequency (each bin the mean of the log ratio over ±12 % of its index, at least two
   bins). The side's own spectrum, shaped by itself, made a cello's lowest octave 5..8 dB too wide.
   A mono recording, or one whose channels are equal, has no side.

### The join

The model continues the recording from a **join** time `J` near the region's end. A join where the
diffuse parts happen to be large starts the model from an unusual moment, heard as a bump. Join
candidates every 0.05 s over the last 40 % of the held region (from the region's end back, none
closer than 0.3 s to its start) are scored on the demodulated partials, each with its mean over
`[t0, J]` as the carrier, and the one with the smallest
`D = sum_h |mean of (a_h - mean) over its last 6 frames|^2 / sum_h |mean|^2` is kept; the fit
then runs on `[t0, J]`.

The recording's own release starts at `Ts`, the first 20 ms window (every 10 ms) after `J` that is
6 dB below the mean of the 15 windows before it, taken 30 ms earlier, when the level then falls
20 dB below that mean before the recording ends; otherwise there is none.

### Trial renders

Some gains depend on what the model generates, so the fit renders it with the generator a note
plays (without its gains) and fixed seeds, starting from the recording's state at `J` as a played
note does:

- the **level gain** `G`: the recording's RMS over the 0.25 s before `J`, in dB, minus the mean
  RMS of six seeded renders from 0.3 s to 2.5 s;
- the **spread factor** `beta_h` per harmonic: `sigma_h` divided by the dB standard deviation of
  the rendered partial after its first second, clipped to 0.25..1 (a partial whose diffuse part
  swings wider than the source's is narrowed toward its mean level);
- the **vibrato level coefficients**: the total level (dB, 20 ms windows every 5 ms) regressed on
  the vibrato's position, velocity and squared position (with a quadratic trend), once on the
  recording's region (`ct`, from its cents low-passed at 12 Hz and their slope over
  `2 pi vr`) and as the mean over ten seeded renders from 0.1 s to 2.9 s (`cm`, from each
  frame's deviation and velocity).

## How it plays

With Sample playback Continue and a continuation stored, a forward note plays the recording from
Start as Sustain does; when its read position reaches `J` the model takes over through a 40 ms
`sin^2` crossfade (the recording reads on through it). The model runs in control frames of
`round(sr / 300)` samples; each partial's level is interpolated between frames by a Catmull-Rom
curve, and its phase advances with its frequency linear across a frame. A note claims one of
eight generators at note-on; a ninth note held at once holds in Sustain's segments. Everything
after the join is generated without allocating.

The model depends on nothing later than its controls, so it is generated about 0.6 s ahead of
what is heard, and the level shaping and the event limit see the windows an offline render sees:
before the join the voice generates two control frames per frame's time, so the lookahead is
full by the join (a note that reaches it sooner generates the rest at once).

- **Pitch.** The vibrato continues the recording's phase. Its depth is `vD exp(x_d)` and its rate
  `vr exp(x_r)`, with `x_d` and `x_r` Ornstein-Uhlenbeck processes (standard deviation 0.15 and
  0.04, time constants 1.2 s and 1.5 s); the phase is the integral of the rate, so it never
  jumps. The jitter is the AR(4) process driven by seeded noise, through a causal third-order
  low-pass at 25 Hz, at a gain computed from the two to give the measured standard deviation.
  `f0 = b 2^((vibrato + jitter) / 1200)`, `b` the model's pitch times what the pitch wheel, the
  LFO and the voice's glide move the note by; the phase continues from the recording's `phi` and
  `Psi` one sample after `J`, and `Psi` counts from `b`, so a bend moves every partial alike.
- **Partials.** Each harmonic below 0.46 of the sample rate is
  `Re(amp_h exp(j (h phi - (1 - gamma_h) h Psi)))`. Its diffuse part runs its AR(6) from its
  history (run through its low-pass first), so it starts at the recording's own state, through a
  causal third-order low-pass at twice its bandwidth (5 Hz..60 Hz; 60 Hz from harmonic 12 on),
  with a power correction (the AR process's power over its power through the low-pass, computed
  from both on 512 points) that fades in with a 0.25 s time constant. `amp_h = Mh + c_h`, then
  pulled toward the mean level by
  `beta_h` (fading in with 0.25 s), then its log deviation from the mean softly bounded,
  `K tanh(d / K)` with `K = 2 sigma_h` (at least 0.25 neper). The first 20 ms of each partial
  match the recording's level just before `J` and relax with a 0.12 s time constant.
- **Slow level.** All partials share a level `exp(x_l)`, `x_l` an Ornstein-Uhlenbeck process of
  0.6 dB over 2 s.
- **Noise.** Seeded white noise through a 1025-tap linear-phase filter (Hann window) whose
  response follows the residual's spectrum, scaled to its RMS, times the slow level.
- **Side.** Each side partial is `rho_h amp_h + e_h`, `e_h` continuing from its history like
  `c_h`, on the mid partial's phase; the side noise has its own spectrum and seed. Left is
  `mid + side`, right `mid - side`. Every gain below applies to mid and side alike.
- **Gains.** In order: the **level shaping** removes slow level swells (the dB level band-passed
  at 2.5..4.5 Hz, subtracted) and moves the vibrato-locked level movement from `cm` to `ct`; the
  level gain `G`, ramped in over 0.3 s; the **event limit**, which pulls a level departure of more
  than 2 dB from the 0.5 s level back toward that edge by 0.85 of its excess (at most 6 dB),
  leaving the vibrato's smaller movement alone. Each gain ramps from 0 dB over its first 0.1 s.
- **Release.** Note-off (or pedal-up) releases the amplitude envelope as for Gate. A recording
  with its own release after the held part (a bow leaving the string) also splices into it: the
  release's first 40 ms, read from `Ts` at the note's read step, are lined up with the model's
  next 0.25 s (normalized correlation, every eighth lag, then sample by sample around the best),
  and at that lag the recording takes over through a 40 ms `sin^2` crossfade at a gain matching
  their RMS (0.7..1.4); it plays on from there, the generator free for another note. Without a
  release of its own the model goes on under the envelope's release.
- **Determinism.** Every random process has a seed derived from the note, so a render is the
  same for every block split.

### Controls

Each is a native Instrument parameter, automatable, on the expanded editor's Hold page. The vibrato
controls are smoothed with a 0.6 s time constant from the value they had (at the join, from the
recording's own), so no change steps; Width applies at once; Hold morph applies to the next note.

| Control (`id`) | Range | Default | What it does |
|---|---|---|---|
| Width (`width`) | 0..2 | 1 | Scales the side of everything: the model and the recording before the join and after a release splice. 0 is mono. |
| Vibrato depth (`vibrato_depth`) | 0..4 | 1 | Multiplies the vibrato's depth. Up to 1 the recording's vibrato keeps each partial's participation `gamma_h`; the vibrato added beyond 1 moves every partial fully, as a played vibrato does, so 2..4 is heard (with `gamma_h` alone, 1.6 moved a 786 Hz partial by only 16 cents). |
| Vibrato rate (`vibrato_rate`) | 0..10 Hz | 0 | Moves the vibrato's rate to the value (at least 1 Hz); 0 keeps the recorded rate. The phase is the rate's integral. |
| Hold morph (`continuation_morph`) | 0..1 | 0 | Morphs toward the morph target (below). |

The level shaping uses the vibrato's played rate.

### Transposition that keeps the spectral envelope

A note away from the recording's key plays a **transposed model** at `f0m r` (`r` the key ratio):
harmonic `k` (frequency `k f0m r`) takes the recording's values at that frequency, interpolated at
the fractional harmonic index `k r` between its two neighbours: log carrier magnitude, mean log
level, `sigma_h` and the diffuse power (in log); its carrier phase, AR shape, bandwidth,
participation and, for the side, `rho_h` and the side AR come from the nearest harmonic. The
harmonic count is the smaller of `0.46 sr / (f0m r)` and `H / r`, so no harmonic is taken from
beyond the recording. Formants and a body's resonances stay at their frequencies. The diffuse parts have no history to start from: a transposed model's
first second is generated and dropped (spread over the time before the join). The recording before
the join plays resampled to the note, as Sustain plays it.

### Morph

Two models (two recordings, of one instrument or of two), each transposed to the note, are mixed
at a common pitch with a Morph amount `w` (0..1). Carrier magnitudes, mean levels, `sigma_h` and
diffuse powers mix in log; `gamma_h`, `rho_h`, vibrato depth and rate linearly. A harmonic's
carrier phase, AR shape and bandwidth come from the model that carries more of that harmonic's
power at `w` (`(1 - w) P_A` against `w P_B`): a model with nothing there cannot shape it (taken
from the trombone, whose side is numerically zero and so broadband, the line shapes put an odd
noise behind the cello's side partials, 56 dB too strong at 100..200 Hz).
The side's independent power is mixed as a ratio to its partial's own power and applied to the
mixed partial, so the side follows the mid wherever it moves. The noise spectra mix in power: the
two noises play at once, at `sqrt(1 - w)` and `sqrt(w)`. The level is the mean of the two notes'
levels, in dB, from their expected RMS. The morph target is another Instrument's continuation,
copied with `instrument.setContinuationMorph`; the recording before the join is the Instrument's
own.

## How it is stored

A continuation is its own encoding, stored on the Instrument as the extension
`instrument_continuation` beside the modal profile (and the morph target as
`instrument_continuation_morph`), at most 1 MiB, saved with the project and part of the device's
identity in the engine. The bytes: `SHMCONT1`, version 1 (u16), the partial count (u16, 1..160),
the stereo flag (`[0|1, 0, 0, 0]`); then little-endian `f32`: 26 scalars (the recording's rate,
`f0m`, `vr`, `vD`, the vibrato phase, `phi` and `Psi` at the model's first sample, `J`, `Ts`, the
jitter's AR(4), innovation, standard deviation and gain, `G`, `ct`, `cm`, both noise RMS values
and a reserved 0); the noise and side-noise spectra (dB relative to their peak on a 1/12-octave
grid from 20 Hz, 121 points); per partial 161 values (carrier, `gamma_h`, mean log level,
`sigma_h`, `beta_h`, the start level, the diffuse part: AR(6), innovation, power, low-pass,
power fix and 30-frame history; the side's `rho_h` and diffuse part). About 104 KiB for 160 stereo
partials. Decoding accepts only canonical, in-bounds bytes; a mono continuation's side is all
zeros. A continuation that does not decode leaves held notes holding in segments.

## Commands

- `instrument.fitContinuation` starts the fit on the selected Instrument's loaded sample, loop
  region and root key, as a session analysis job (`kind` `continuation`); it changes nothing.
  `instrument.analysis` reports the fitted `continuation` (`f0`, `vibratoHz`, `vibratoCents`,
  `joinSeconds`, `releaseSeconds`, `partials`, `stereo`). A change of the sample, its trim, root or
  loop region makes the job stale.
- `instrument.acceptContinuation` stores the current fit, sets Source to Sample and Sample
  playback to Continue, overriding their automation lanes, in one undo step.
- `instrument.setContinuationMorph` copies another Instrument's continuation (`fromTrack`,
  `fromDevice`) as this one's morph target, or removes it with `clear`, in one undo step.

The Sample page's analysis area has Fit hold and, for a ready fit, its summary and Accept
continuation.

## Prototype results

An offline pure-Python prototype was listened to on a cello's C3 (a wide stereo patch whose held
part is the short end of a slide), a cello's G4, a trombone (mono), a whistle and a flute. The
listener approved the hold, the stereo, the three controls, the transposition and the morph.
Measured:

- side-to-mid level per octave band over the held part: within 1..3 dB of the recording on the
  cello's C patch and 1..2 dB on G4, the whistle and the flute, except the G4's 100..200 Hz band
  (-8.6 dB against the recording's -14.1); the trombone stays mono;
- a 786 Hz partial's pitch swing (10..90 %) at vibrato depth 0, 1, 2, 3 and 4: 13.6, 14.9, 24.6,
  41.5 and 57.3 cents, its level spread 9.6..10.5 dB;
- vibrato rate set to 4.0, 6.5 and 8.0 Hz measured 4.44, 7.12 and 8.75 Hz on one seed (the rate's
  own wander is ±4 %; other seeds not measured);
- transposed by -5..+5 semitones, the trombone's spectral centroid stays near 1000 Hz, where
  resampling moves it from 899 to 1619 Hz;
- morph from cello to trombone: side to mid at 200..400 Hz -11, -13, -16, -20.5 dB and mono at
  `w` 0, 0.25, 0.5, 0.75 and 1.

### Where the instrument differs from the prototype

The gains look ahead in the generated model as the prototype's did over the whole note, so they
are not changed. These differ and are to be heard against the approved renders:

- `cm` is measured with the coupling played (0; the prototype measured it at 0.4), and `beta_h`
  and `G` once at fit time rather than per render;
- the release splice looks only after note-off (the prototype searched 0.3 s each way), and a
  recording without its own release fades with the envelope (the prototype borrowed another
  recording's release curves);
- the jitter's low-pass is causal; the pitch track is interpolated linearly;
- the held region is the loop region and the join is chosen on one demodulation of it (the
  prototype's regions were chosen by hand and each candidate was fitted again);
- a transposed note plays the recording resampled up to the join and then the transposed model
  (the prototype played the model alone with a 40 ms fade-in).

## Validation

Deterministic tests with self-authored signals: the encoding round trips and refuses hostile bytes
and values out of bounds; a transposed model takes the envelope at each harmonic's frequency; a
morph at 0 and 1 is each model; a note renders the same however far ahead it was generated;
its level is within 3 dB of the expected level and its side present only in stereo; the vibrato
depth and rate controls are read back from a lone partial's zero crossings. The fit of a 440 Hz
tone with a 25-cent vibrato finds its pitch, vibrato, carriers falling 4 dB per harmonic and full
participation, no side for equal channels, a release after the held part, and a model that plays
within 2 dB of the recording and within 3 dB of its side-to-mid ratio; bad input and
cancellation are refused. In the sample runtime a held note sounds past its recording's end
within 4 dB of it, renders the same for every block split, and fades on release; the session
fits, accepts and undoes in one step and sets a morph target.

## Not yet

Not yet listened to in the instrument (#1624). Short held parts (a whistle's or flute's under
0.5 s), a release for recordings without their own, continuations at several keys (each note from
the nearest), playing in reverse, and the compact panel's controls are open.

## Sources

All are public papers and books; no code from other products is used.

- X. Serra, *A System for Sound Analysis/Transformation/Synthesis based on a Deterministic plus
  Stochastic Decomposition*, PhD thesis, Stanford (CCRMA), 1989.
- J. A. Moorer, "The heterodyne filter as a tool for analysis of transient waveforms", Stanford
  Artificial Intelligence Laboratory memo AIM-208, 1973: demodulating each harmonic on a pitch
  track.
- K. Fitz and L. Haken, "On the use of time-frequency reassignment in additive sound modeling",
  JAES 50(11), 2002: bandwidth-enhanced partials, sinusoids widened by noise.
- S. M. Kay, *Modern Spectral Estimation: Theory and Application*, Prentice Hall, 1988:
  autoregressive models from the autocorrelation (Yule-Walker).
- A. Röbel and X. Rodet, "Efficient spectral envelope estimation and its application to pitch
  shifting and envelope preservation", DAFx 2005: keeping the envelope while the pitch moves.
- P. Herrera and J. Bonada, "Vibrato extraction and parameterization in the Spectral Modeling
  Synthesis framework", DAFx 1998.
