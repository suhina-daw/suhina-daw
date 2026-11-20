---
description: Held (bowed, blown, sung) recordings fitted with vibrato, per-partial onsets and vibrato-coupled amplitude, a held noise layer, a measured release and a spectral envelope that stays in Hz
read_when: fitting a cello, violin, voice, flute, clarinet or organ sample; vibrato, tremolo, bow noise, breath noise, onset, release, formants, chipmunk transposition, sustained partials, ModelSet schema 3
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/instrument-analysis/src/sustain.rs
  - crates/instrument-analysis/src/sustain_tests.rs
  - crates/dsp/src/modal/sustain.rs
  - crates/dsp/src/modal/sustain_tests.rs
---

# Held tones in a fitted model

[Modal fitting](instrument-modal-fitting.md) describes a recording as decaying sines plus a
0.25 s attack. A bowed, blown or sung note is not that: its pitch and partial levels move with
vibrato, a noise layer (bow, breath, rosin) sounds while it is held, its partials swell in one
after another, it stops with a release of its own, and its body or formant resonances stay at
the same frequencies when the player changes note. Fitted as frozen sines, such a recording
sounds like an organ of sine waves, and transposed far from its key it sounds like a smaller
instrument.

This document adds a **held layer** to a fitted recording. It is measured from the same one
recording, stored beside the recording's modes in a revision-4 set, and played by the
Instrument while the key is held. A struck recording gets none and plays exactly as before.
Nothing here is a physical bowing or breath model: it reproduces measured statistics of one
performance.

## What is measured

Fitting algorithm version 3 runs these steps on the prepared 22050 Hz mono signal (after the
trailing-floor trim of version 2). All work is bounded by the eight-second analysis cap, uses
fixed frame sizes, and is cancellable between steps. Times are seconds from the start of the
analyzed interval.

### 1. Vibrato, before the modes are fitted

Vibrato smears a partial across several Fourier bins, so a fixed-frequency projection sees a
level that jumps up and down and a peak that is too wide; version 2 rejected such partials. The
pitch is therefore measured and removed first.

1. The recording needs at least 1.0 s after trimming. The **held region** starts at 0.15 s.
2. **Partials to track.** The mean power spectrum of 2048-sample Hann windows, hop 1024, over the
   held region (at most 64 windows) gives its local maxima between 60 and 2000 Hz; each centre is
   refined by parabolic interpolation of the log power. The strongest is the first tracked
   partial; the strongest whose frequency differs from it by more than 15 % is the second, kept
   only when its power is at least 1 % of the first's.
3. **Instantaneous frequency.** Each tracked partial is projected (the complex projection of
   modal fitting) on 1024-sample Hann frames every 256 samples at its centre frequency `f`. The
   phase advance between consecutive frames, minus the advance `2 pi f 256 / 22050` a steady
   partial makes, wrapped to (-pi, pi], gives the frame's frequency
   `f + delta 22050 / (2 pi 256)`, timed midway between the two frame centres; its deviation in
   cents is `1200 log2(f_frame / f)`. A frame whose amplitude is below 10 % of the partial's
   median is a gap and is filled linearly from its neighbours; more than 10 % gaps means no
   vibrato.
4. **Agreement.** Vibrato moves every partial of a tone by the same number of cents, while
   beating partials of a struck string wobble at their own rates. With a second partial, the two
   detrended cent curves must correlate by at least 0.7 and their RMS deviations must be within a
   factor of 1.5 of each other (two steady partials, both under 1 cent RMS, also agree). A lone
   tracked partial cannot be compared: a beating pair is told from vibrato by its level, which
   dips as its frequency wobbles, so a lone partial whose detrended dB level ripples by more than
   1.5 dB RMS has no pitch curve. The **pitch curve** `v` is the amplitude-weighted mean of the
   tracked curves.
5. **Rate.** After removing the least-squares line, the normalized autocorrelation of `v` is
   searched over lags for 3 to 10 Hz. A local maximum of at least 0.4 is the vibrato period,
   refined by parabolic interpolation (Herrera and Bonada; Rossignol et al.).
6. **Depth and onset.** Over each window of exactly one period centred on a frame, the
   projection of `v` on a sine and cosine at the rate gives the local depth `D` (the peak
   deviation in cents). The full depth is the median of `D` over the last 60 % of the frames,
   divided by the tracking's response to that rate: the 1024-frame Hann window's
   `sinc(x) / (1 - x^2)` with `x = rate 1024 / 22050`, times the 256-sample phase difference's
   `sinc(rate 256 / 22050)` (about 0.95 at 5.5 Hz). Vibrato needs a full depth of 5 to 200 cents.
   The delay and fade come from the first frames `t10` and `t90` where `D` reaches 10 % and 90 % of
   its own median. The one-period window smooths the ramp by about a period `P = 1 / rate`, which
   is removed in quadrature: `w = (t90 - t10) / 0.8`, `fade = sqrt(max(0, w^2 - P^2))`,
   `delay = (t10 + t90) / 2 - fade / 2`, each 0..4 s. When `D` reaches 90 % at the first measured
   frame both are 0, and when it reaches 10 % there the delay is 0.
7. **Drift.** A moving average of exactly one vibrato period (0.1 s without vibrato) removes the
   vibrato from `v`; after detrending, its RMS is the drift (cents, 0..20; below 1 cent it is
   none). The lag where its autocorrelation falls to 0.5 gives its bandwidth
   `ln 2 / (2 pi lag)`, 0.5..10 Hz. Drift is kept only for a held recording (below).
8. **Flattening.** When vibrato is found, the signal is resampled along the true curve, `v`
   divided by the tracking's response: with `rho(t) = 2^(v(t) / response / 1200)`, interpolated linearly between frame centres and held at the ends,
   warped time is `phi(t) = integral of rho`, and output sample `m` is the input at the `t` where
   `phi(t) = m / 22050`, read with a 16-tap Hann-windowed sinc. Every partial of the flattened
   signal stays at one frequency while its level keeps its modulation; modal fitting, the
   attack, the phases and every later step use it. The shift in time is about a millisecond and
   is ignored.

### 2. Held recordings

After the modes are fitted, a recording is **held** when its sustained modes (those modal
fitting marks sustained) hold at least half of its total magnitude. Only a held recording, or
one with vibrato, gets a held layer; a struck recording gets none.

For a held recording the fine envelope of every mode is measured on the flattened signal with
1024-sample Hann windows every 256 samples. The **held level** of a mode is the median of its
envelope from 0.3 s to the release start (or the end).

- **Onset and rise** (per sustained mode). `t10` and `t90` are the first frame centres where the
  envelope reaches 10 % and 90 % of the held level. The rendered level
  `1 - exp(-(t - onset) ln 10 / rise)` passes both: with `tau = (t90 - t10) / ln 9`,
  `onset = t10 - tau ln(10 / 9)` and `rise = t90 - onset`. A mode that reaches 90 % within 0.04 s
  gets neither: the attack holds that much. Onset 0..1 s, rise 0..4 s. In winds the
  higher partials arrive later (Benade); this is what a common envelope cannot reproduce
  (McAdams et al.).
- **Vibrato-coupled amplitude** (per sustained mode, with vibrato). As the pitch moves, a partial
  climbs or descends the slope of a body or formant resonance; one on a slope moves at the
  vibrato rate, one on a peak or in a valley at twice the rate (Fletcher and Sanders; Mellody and
  Wakefield). Over the frames where `D` is at least half the full depth, the mode's detrended dB
  envelope is fitted by least squares as `s v + q (v^2 - mean(v^2))`, with `s` in dB per cent
  (-0.5..0.5) and `q` in dB per cent squared (-0.01..0.01). Envelope and pitch curve use the same
  frame, so the window's smoothing does not bias `s`. This captures any tremolo at the vibrato
  rate too.
- **Tremolo** (without vibrato). The total RMS envelope's detrended dB curve is searched for a
  period of 3 to 10 Hz as in step 5. A peak of at least 0.4 with a sine fit of at least 0.5 dB
  gives `tremolo_hz` and the peak depth `10^(A / 20) - 1` (0..0.9).
- **Release.** The sum `S` of the fine envelopes of the three strongest sustained modes. The held
  part runs from 0.3 s to the last frame where `S` is at least half its maximum, so a long tail
  does not pull its median `L` into the decay. After the last frame at least 0.5 L (not earlier
  than 0.3 s), `S` must fall below 0.1 L before the recording ends, over at least four frames. A
  line through its dB values until it first falls below 0.01 L gives `T60 = -60 / slope`,
  0.01..20 s; with the four-frame rule the shortest measurable release is about 0.14 s, and a
  shorter one is left to the Release control. Strings ring on as their body decays; winds and
  voice stop within tens of milliseconds.
- **Noise.** From 0.25 s to the release start, 4096-sample four-term Blackman-Harris windows (its
  sidelobes are 92 dB down, so partial leakage does not read as noise), hop 2048, give the mean
  power per bin over `M` windows (at least one). Bins within 4.5 bins of a fitted mode or its
  partner, and of any local maximum 12 dB above the median of its 32 neighbours, are masked. Each
  band is a 2/3-octave band centred at `80 * 2^(2b/3)` Hz for `b = 0..10` (80 Hz to 8.1 kHz). Its
  power is the median of its unmasked bins (at least three, else the band is silent), divided by
  `1 - 1 / (3M)` (the median of an M-window mean is that much below its mean). The one-sided
  density `S = 2 P / (22050 sum(w^2))` is stored as the amplitude density `sqrt(S)` divided by
  the sum of the mode magnitudes, the same normalization as the modes and the attack (0..1 per
  square-root hertz). This is the stochastic part of Serra's sinusoids-plus-noise model at a
  2/3-octave resolution. Noise onset and rise are the medians of the sustained modes'.
- **Body envelope.** A held recording stores `body_fixed = 1`: its spectral envelope stays in Hz
  when the key moves (below). A struck recording has 0 and transposes all its partials, as a
  smaller or larger body would.

The attack residual of a held recording is computed against the modes with their onsets, so it
holds what the modes miss, not the opposite of their swell.

## How it is stored

A recording's held layer is a `suhina_dsp::modal::sustain::Sustain`:

| Field | Meaning | Bounds |
|---|---|---|
| `vibrato_hz`, `vibrato_cents` | rate and peak deviation | 0 for none, else 3..10 Hz and 5..200 cents |
| `vibrato_delay`, `vibrato_fade` | before the vibrato starts, and until it reaches full depth | 0..4 s |
| `tremolo_hz`, `tremolo_depth` | amplitude modulation without vibrato | 0 for none, else 3..10 Hz and 0..0.9 |
| `drift_cents`, `drift_hz` | slow random pitch wander | 0 for none, else 1..20 cents and 0.5..10 Hz |
| `release_seconds` | T60 after note-off | 0 for none, else 0.01..20 s |
| `body_fixed` | how much of the spectral envelope stays in Hz | 0..1 |
| `attack_seconds` | length of the recording's attack, for the noise hand-over | 0..0.25 s |
| `noise_onset`, `noise_rise` | the noise layer's onset and rise | 0..1 s, 0..4 s |
| `reserved` | kept for a later field | 0 |
| `noise[11]` | amplitude density per 2/3-octave band | 0..1 per sqrt(Hz) |
| `onset[64]`, `rise[64]` | per mode, as its index | 0..1 s, 0..4 s |
| `am_slope[64]`, `am_curve[64]` | per mode | -0.5..0.5 dB/cent, -0.01..0.01 dB/cent^2 |

Every value is finite; per-mode entries beyond the recording's mode count are zero. A layer whose
every value is zero is the same as none.

**Schema 3.** `ModelRecording` gains `sustain: Option<Sustain>`. A set whose recordings have no
held layer encodes exactly as schema 2, byte for byte. Otherwise its header carries schema 3
(same magic and revision 4), the recordings follow exactly as in schema 2, and then, for every
recording in order, a u8 flag (0 none, 1 present), three zero bytes and, when present, the 281
fields above as little-endian f32 in table order (per-mode arrays complete, 64 entries each, one
per possible mode of a recording).
Schema-2 bytes still decode, with no held layers; a schema-2 build rejects schema-3 bytes and
the device stays silent rather than playing the set without its layer. Decoding accepts only
canonical bytes, as for schema 2.

## How it plays

`ModelSet::voice` fuses the held layers of the participating recordings like their modes:
per-mode onsets, rises and the amplitude coupling over matched modes as weighted means (a
recording without a layer counts zero); the noise bands and depths as weighted means over all
participants; rates, the drift bandwidth and the release as weighted geometric means over the
participants that have them (the release only when they hold at least half the weight). The
anchor's `body_fixed` and attack length are used.

**Body envelope under transposition.** When a set is built, each recording's spectral envelope
`E` is tabulated off the audio thread on a 1/8-octave grid of ratios from 2^-8 to 2^8, as an
upper envelope through its partial peaks (in the manner of the true envelope): a mode more than
6 dB below the straight line (dB over log frequency) between its neighbours is a dip that belongs
to the source and moves with the pitch (a clarinet's weak even partials), and is left out, the
deepest first, until none is; straight lines join the remaining modes. Above its highest mode it
continues with the tilt (dB per octave, -24..0) of the modes in its top octave; below its lowest,
flat. An average over the partials instead (a 1/3-octave Gaussian was tried) widens the formants,
and the correction then undershoots: a peak moved by 0.4 to 0.7 octaves when the key moved by
one. A played note transposed by `T = 2^((key - anchor key) / 12)` scales
each fused mode by `10^(c / 20)` with
`c = body_fixed * (E(ratio * T) - E(ratio))`, clamped to +-24 dB: the partial takes the envelope
level of the frequency it now has. This is the additive form of formant preservation (Röbel and
Rodet); with recordings at several keys, the nearest recording anchors the note and the
correction stays small.

**While the key is held** a revision-4 voice with a held layer updates its control every 32
samples and ramps each sine's level linearly between updates (`suhina_dsp::modal::sustain::Performer`,
fixed storage, no allocation):

- **Pitch.** The deviation in cents is `e(t) vibrato_cents sin(2 pi vibrato_hz t)` plus the drift,
  where `e` is 0 before the delay and rises linearly to 1 over the fade. The drift is seeded,
  one-pole low-passed noise at `drift_hz` with standard deviation `drift_cents`. The voice's bank
  is retuned to the pitch wheel plus this deviation; retuning keeps phase.
- **Partial levels.** Each sine's level is its onset envelope
  `1 - exp(-(t - onset) ln 10 / rise)` (1 with no rise) times
  `10^(clamp(s u + q (u^2 - e^2 vibrato_cents^2 / 2), -12, 12) / 20)`, where `u` is the vibrato
  deviation, times the tremolo `1 + tremolo_depth sin(2 pi tremolo_hz t)`.
- **Noise.** Eleven seeded white-noise generators feed band-pass state-variable filters at the
  band centres with Q 2.145 (a 2/3-octave half-power width). The bands tile log frequency, where
  a unity-peak band-pass's squared response integrates to `I = 2a acos(a) / sqrt(1 - a^2)` with
  `a = 1 / (2Q)`; a band's gain is its density times `sqrt(sample_rate ln 2 / (3 I))`, about
  `0.6007 sqrt(sample_rate)`, so the overlapping bands sum to the measured density (within
  0.6 dB between 300 Hz and 3 kHz, falling about 4 dB beyond the outer band centres). Bands at
  or above 0.45 of the sample rate are silent. The layer follows
  its onset envelope and, while the recording's attack fades out (its second half), a raised
  cosine from 0 to 1, since the attack already carries the noise of the start.
- **Release.** Revision 4's release is a damper that caps every sine's T60 at Release on note-off
  (or pedal-up). With a measured release the cap is `release_seconds * Release / 0.25` instead, so
  the recording's own release plays at the control's default and the control still scales it;
  without one it is Release. The noise layer falls with the same T60 and stops at -60 dB. The
  onset of the whole note stays with the shared amplitude envelope (Attack, Decay, Sustain): the
  held layer adds no second envelope, only per-partial onsets on top of it.

A voice without a held layer plays exactly as before. The pitch wheel offset is applied together
with the vibrato. Rendering is identical for every block split, since control updates count
samples from the note-on.

**Budget.** A voice's held state (per-sine levels, ramps, onsets and couplings for 128 sines,
eleven filters and the control state) adds under 4.5 KiB: a voice stays under 12 KiB, 16 voices
under 192 KiB, within the runtime's 256 KiB inline target. The 32-sample control update retunes
at most 128 sines. A vibrato of 50 cents at 6 Hz moves less than one cent between updates.

## Validation

Deterministic tests with self-authored synthetic signals of known parameters:

- harmonic tones with sine vibrato of known rate (5.5 Hz), depth (25 and 60 cents), delay and
  fade, at 130.8 and 440 Hz: rate within 0.1 Hz, depth within 15 % or 3 cents, delay and fade
  within 0.15 s; the flattened tone keeps under 3 cents of deviation, and a whole fit keeps its
  harmonics as sustained modes with the vibrato in its layer;
- struck tones with beating pairs, and a held pair beating at 5 Hz: no vibrato;
- a partial whose level follows the vibrato by 0.1 dB per cent: its coupling within 0.03;
- a tremolo of known rate and depth without vibrato;
- tones with partials swelling at known onsets and rises: within 0.05 s or 20 %;
- partials plus white noise of known density: each band within 3 dB; a pure tone: every band at
  least 40 dB below the partials;
- a held tone with an exponential release of known T60: within 20 %;
- schema-2 bytes decode unchanged, schema-3 round trips, every hostile byte and bound fails;
- the device: a held layer renders the same for every block split, without allocation; a set
  without layers renders bit-identically to revision 4 before; vibrato's rate and depth and the
  noise density measured back from rendered output; the envelope correction keeps a formant peak
  at its frequency an octave up and down, and bridges a clarinet's even partials.

The formulas above were checked first in a throwaway pure-Python model with the same synthetic
signals: the vibrato rate came back within 0.003 Hz and its depth within 0.7 %, the flattened
partial within 0.7 cents, every noise band within 0.9 dB and the release T60 within 1 %. The
fade correction, the warp's response division, the lone-partial rule, the noise gain constant and
the held part of the release came from that check.

These are claims about the measured statistics of one recording, not listening results. Noise
pulsing with the pitch period (Chafe), per-partial release times, shimmer, a control for
`body_fixed` and A/B listening against the source remain open in #1624.

## Sources

All are public papers and books; no code from other products is used. Suhina's implementation is
original Rust with self-authored test signals.


- X. Serra, *A System for Sound Analysis/Transformation/Synthesis based on a Deterministic plus
  Stochastic Decomposition*, PhD thesis, Stanford (CCRMA), 1989: sinusoids plus a residual
  modelled as filtered noise.
- P. Herrera and J. Bonada, "Vibrato extraction and parameterization in the Spectral Modeling
  Synthesis framework", DAFx 1998; S. Rossignol et al., "Feature extraction and temporal
  segmentation of acoustic signals", DAFx 1999: vibrato rate from the f0 track's periodicity.
- N. H. Fletcher and L. M. Sanders, "Quality of violin vibrato tone", JASA 41, 1967; M. Mellody
  and G. Wakefield, "The time-frequency characteristics of violin vibrato", JASA 107, 2000:
  partial amplitude modulation at one and two times the vibrato rate.
- R. Maher and J. Beauchamp, "An investigation of vocal vibrato for synthesis", Applied
  Acoustics 30, 1990: spectral modulation matters more than random rate variation.
- S. McAdams, J. Beauchamp and S. Meneguzzi, "Discrimination of musical instrument sounds
  resynthesized with simplified spectrotemporal parameters", JASA 105, 1999: smoothed envelopes
  and a common amplitude envelope are easily heard.
- A. Röbel and X. Rodet, "Efficient spectral envelope estimation and its application to pitch
  shifting and envelope preservation", DAFx 2005.
- A. H. Benade, *Fundamentals of Musical Acoustics*, 1976: wind partials growing at different
  rates.
- F. J. Harris, "On the use of windows for harmonic analysis with the discrete Fourier
  transform", Proc. IEEE 66, 1978: the four-term Blackman-Harris window.
- A. Simper, "Linear trapezoidal integrated state variable filter", Cytomic technical paper,
  2013: the band-pass sections (already Suhina's `Svf`).
