---
description: Bounded single-recording modal fitting, measured limits, temporal validation and explicit native Instrument adoption
read_when: fitting a sample, modal fitting, resonance estimation, decay estimation, instrument.fitSample, instrument.acceptFit
type: spec
status: planned
last_verified: 2026-11-20
covers:
  - crates/instrument-analysis/src/modal.rs
  - crates/instrument-analysis/src/modal_tests.rs
  - crates/session/src/instrument_analysis.rs
  - crates/session/src/commands/instrument.rs
  - crates/ui/src/detail/instrument_analysis.rs
---

# Fit a decaying recording

An Instrument with a loaded sample can fit an **experimental linear modal model** of one
isolated struck sound. The user sets the sample trim and root, chooses **Fit resonances**, then
inspects the number of retained modes and temporal envelope error before accepting. Fitting
does not change the instrument. Acceptance installs the canonical modal profile, selects Modal,
and resets Body decay, Brightness, Structure and Strike to the profile's neutral controls,
enabling the full sixteen-mode quality budget, in one
undo step. The sample remains available as the source. **Fit folder…** fits a folder of such
recordings in one job (§ Fitting a folder). Existing model-profile adoption and
project persistence are reused; reopening never starts fitting.

The reference key is locked to the sample's current root, with its equal-tempered reference Hz.
Measured resonance frequencies are stored as ratios to that reference. This does not declare
the lowest resonance or a missing fundamental to be the pitch. Root-note analysis can be used
first, and its proposal can be corrected before acceptance.

## Estimator and limits

Algorithm version 2 admits mono/stereo PCM at 1000..384000 Hz and at most the first eight
seconds of the selected trim. It checks finite input and chooses the channel with greatest AC
energy; the chosen zero-based channel and inspected source-frame range are reported. Opposite
stereo polarity therefore cannot cancel the source. Stereo radiation and width are not fitted.
A channel whose values do not vary (silence or a constant offset) is reported as silent.
The existing shared resampler prepares one normalized channel at 22050 Hz. A relative trailing
noise floor limits the useful analysis interval; the source itself is never trimmed or edited.

**Finding resonances.** A Hann-windowed FFT over the start of the analyzed interval, with the
largest power-of-two length up to 8192 samples that fits it, proposes at most 32 prominent
peaks: a local maximum within 60 dB of the strongest that stands 20 dB above the median of its
neighbours 3..8 bins away. This detection window is independent of the envelope windows, so
the partials of a low note in a short trim are resolved (version 1 searched only the first
envelope window, as short as 1024 samples, where neighbouring partials of a low note hid each
other). Candidates closer than two envelope-window bins (the Hann main lobe's half width) are
merged into the stronger one. Bounded interval refinement on the detection window estimates
each peak frequency.

**Partials along the string.** A piano note has dozens of partials, a bass note a hundred under
10 kHz, and most are far weaker than the few that prominence finds; a model of only those
sounds like a few sine waves (published modal pianos use about 100 partials for a bass note,
Bank, Zambon and Fontana 2010; a differentiable piano model uses 96 and found an explicit
stiff-string inharmonicity essential, Renault, Mignot and Roebel 2022). So after the prominent
peaks are refined, the fit predicts the rest from the stiff string, whose partials lie at
`k * f0 * sqrt(1 + B * k^2)`. The measured first partial `f1` is itself stretched
(`f0 * sqrt(1 + B)`), so against it the k-th partial is `k * f1 * sqrt((1 + B * k^2) / (1 + B))`:

1. `f1` is the refined peak nearest the reference key's frequency, within 70 cents; without
   one, no partials are predicted (a bell or bar is left to prominence).
2. With `r = f / f1`, `B` is the least-squares fit through the origin of
   `(r/k)^2 - 1 = B * (k^2 - (r/k)^2)` over the other refined peaks within 5 % of a whole
   multiple `k` of 2..32 of `f1`, from at least three such peaks, clamped to 0..0.01;
   otherwise 0.
3. For each `k` from 1 while the predicted partial is below the analysis ceiling (at most 128 partials), when no
   candidate lies within the candidate separation or 1 % of it, the strongest detection bin
   within 1 % of it becomes a candidate if it is a local maximum within 80 dB of the
   strongest bin and stands 6 dB above the median of its neighbours 3..8 bins away; candidates
   stop at 96 in all.

Every candidate is then measured and fitted alike (a predicted partial is kept, loose or
validated, only when its envelope says so).

**Measuring decay.** The interval is cut into 6..12 consecutive non-overlapping Hann windows
with a power-of-two size from 1024 to 8192. A complex projection at the mode's frequency
measures its amplitude in each window. Each mode uses its own span: it starts at its loudest of
the first three windows (an attack inside the first window is not a decay), and ends before the
first window more than 60 dB below that peak or 6 dB above the lowest level so far (the
recording's floor, or the next strike in the trim). Within the span, even windows supply
estimation data and odd windows are held out for validation. Least squares on log amplitude
versus time estimates exponential decay; T60 is minus ln(1000) divided by the slope. The
window's damping-dependent amplitude bias is compensated before storing relative magnitudes.

**Accepting a mode.** A mode is kept when it has at least three estimation and three validation
windows, its slope decays with a T60 within 0.001..60 s, the fitted level falls at least 1 dB
across the estimation windows and at least three times the larger RMS envelope error, the
estimation error is at most 2.5 dB and the validation error at most 3 dB. A real strike rarely
decays as one exponential: beating partials and a fast early decay followed by a slower one
leave decibels of error that version 1's limits (1.0 dB estimation, 1.5 dB validation) rejected,
so a recorded strike could yield no model at all. When a span of nine or more windows fails,
the fit is repeated once on its last two thirds, which describes the sustained part after a fast
early decay. A mode whose fitted level changes by at most 3 dB across its whole span without
clearly decaying, within the same error limits, is **sustained** (the late retry never makes a
mode sustained, since beating can flatten a decaying partial's late windows): a bowed, blown or held partial
(a cello, an organ, a voice) is stored with the profile's longest T60, 60 s, and is reported as
sustained; note-off releases it like any modal voice. This approximates a held tone by its
spectrum and is not a bowing or breath model. The result contains at most 16 modes within the
canonical profile's bounds. Rising envelopes, noise and short sounds yield no model; the
playable sample remains usable.

**The fitted recording.** Besides the schema-1 profile, a fit returns a `ModelRecording` for a
revision-4 set ([modal profiles](instrument-modal-profile.md#fitted-model-sets-revision-4)):

- **Up to 64 modes.** Detection and the string's partials propose at most 96 candidates; the
  64 with the most energy (`magnitude^2 * T60`) are kept. The schema-1 profile keeps the 16 largest magnitudes.
- **Phase.** The complex projection of a mode's first span window gives its phase at that
  window, carried back to the strike (the start of the analyzed interval) by its frequency. With
  the phases, the modes start where the recording's partials do, so the attack below hands over
  to them without cancelling.
- **Loose partials.** A candidate whose envelope fails every fit above (a deep beat between
  two or three strings of one note dips more than the error limits allow, and its rise after a
  dip ends the span early) is still a partial of the sound; dropping it leaves a few smooth
  partials that sound like sine waves. It is kept **loose** when, from its loudest of the first
  three windows until 60 dB below that peak (no rise rule), it has at least six windows, a
  least-squares line through them falls by at least 6 dB with a T60 within 0.001..60 s, and the
  line's RMS envelope error is at most 15 dB, and the mean level of each third of those windows
  is at least 1 dB below the third before it (a burst followed by a flat floor is no decay). A
  loose mode joins the fitted recording with that
  T60 (and may get a beating partner below), but never the schema-1 profile or its reported
  validation errors, and a sound with loose modes only has no model.
- **Two-part decay.** When a span of nine or more windows has a valid fit on its last two thirds
  (the slow part), the excess of the earlier windows over that line is fitted as a second,
  faster exponential. It is kept as `fast_decay_seconds` and `fast_share` when its T60 is under
  half the slow one, it has at least two windows, and over the whole span it lowers the RMS
  envelope error by at least 1 dB compared with one exponential, or leaves at most a third of an
  error of at least 0.25 dB, or one exponential failed. (A fast part shorter than a window still
  bends the first windows: one exponential then fits within half a decibel yet misses it.)
- **Beating.** Each kept mode's amplitude is also measured on 1024-sample Hann windows every 256
  samples. After removing the fitted decay, the residual log envelope's normalized
  autocorrelation is searched over beat periods of 0.1 to 5 s (shorter than half the measured
  span). A peak of at least 0.5 with an RMS ripple of at least 1 dB is a beat: its rate gives
  `beat_ratio = 1 + rate / frequency`, and the ripple's peak-to-peak depth `D = 2 * sqrt(2) * RMS`
  dB gives the partner's share `a2 / (a1 + a2)` from `(a1 + a2) / (a1 - a2) = 10^(D / 20)`, at most
  0.5. The sign of the detuning is not observable and is taken as positive.
- **Level.** `level` is the sum of the modes' magnitudes in the source's own full-scale units,
  so the fitted model at the recording's key is as loud as its sample.
- **Attack.** The fitted modes, with their phases, two-part decays and partners, are rendered at
  22050 Hz over the first 0.25 s of the interval (or less, if shorter); the difference from the
  prepared signal, faded out with a raised cosine over its second half and divided by `level`,
  is the attack. It is kept only when its energy is at least 0.1 % (-30 dB) of the signal's in
  that window, so a purely modal sound carries none. The attack holds what the modes miss, so the
  body response is not applied twice.

The report lists per mode the fast part, the beat and the phase, and the attack's duration.
Acceptance and addition to a set are described with the revision-4 commands.

Report errors are RMS dB differences of a mode's amplitude envelope, not waveform errors,
probabilities, realism percentages or independent listening results. Odd windows belong to
the **same recording**: they do not establish held-out note, velocity, gesture or session
performance. Mode discovery itself has a fixed small search budget; no converged global
optimum is claimed. Initial phases and the attack are measured from the one recording, not
reconstructed as excitation or contact physics; room response, nonlinear dynamics and coupled
modes are not separated, and the attack and modes may carry the room. Fitted resonances must
not be presented as a uniquely recovered physical body.

## Fitting a folder

A sampled instrument usually comes as a folder with one note per file, often at several
velocities. `instrument.fitFolder` takes an Instrument and a folder `path` and starts one job
on the shared preparation worker (the same two-worker admission budget, cancellation and target
checks as a single fit):

1. It lists the folder's audio files (not its subfolders), sorted by name. A folder without
   audio files, or with more than 128, is refused before any work starts.
2. For each file, on the worker: decode it; if it has more than two channels or no frames it is
   skipped with that reason. Root analysis over the first eight seconds proposes its key: a
   stable single-note proposal, or else the file is skipped as having no stable pitch (a folder
   of loops is not an instrument). The file is then fitted exactly as a single sample with that
   key as its reference, keeping the fitted recording; a file without a fit is skipped with the
   fit's reason.
3. Files fitted at the same key are **velocity layers**: ordered by their level, quietest first,
   they stand for velocities `(i + 1) / n`; a key with one file stands for 0.8.
4. At most 32 recordings fit a set. With more, an evenly spaced selection of the recordings
   ordered by key and velocity is kept (`round(i * (n - 1) / 31)` for `i` = 0..31) and the rest are
   reported as left out for capacity.

Recordings of one instrument are not always in one folder. `instrument.fitFiles` takes a list
of audio files (`paths`, at most 128, from anywhere) instead of a folder and runs the same job
on them, sorted by file name; a path that is not an audio file refuses the command before any
work starts. Dropping two or more audio files at once on an Instrument's sample waveform runs
it: the drop reads "Fit N samples into one model", and nothing is loaded as the sample.

The job's report lists every file with its key, velocity and outcome (kept, left out, or the
reason it was skipped), and the recordings kept. Nothing changes until the person accepts:
`instrument.acceptFit` with a folder job adopts all its recordings as a new set (as for one
recording, with the model controls reset); `instrument.addFit` adds them all to the device's
existing set (all or none: duplicates of a key, velocity and source already there, or more than
32 in all, refuse the whole addition), with an optional `source` or `newSource` for all of them. The folder job
is stale once the device is removed or its fitted model changes; the device's own sample, trim
and root do not concern it. The Sample page offers **Fit folder…**, which asks for the folder
with the system's dialog.

## Audition and comparison

Before accepting, the person can listen to the fit against its source. `instrument.auditionFit`
takes the ready (or just accepted) fit's job and `which` (`original` or `fitted`) and plays, through
the existing preview player at the preview level:

- **original**: the analyzed range of the measured channel, as recorded;
- **fitted**: the fitted recording rendered by the native Instrument (revision 4, a one-recording
  set, Attack 0, Sustain 1, Gain 0 dB) at the source's sample rate, its key and velocity held for
  the same duration.

Both are compared before playing, and the fitted render is played at the original's RMS level so
the comparison is of timbre, not loudness (a level-matched A/B). The command reports, and the
Sample page shows, the level difference before matching and a **spectral distance**: average power
spectra (2048-sample Hann windows, hop 1024, at most eight seconds) summed into octave bands
centred on 63 Hz..16 kHz below Nyquist; for each band within 60 dB of the original's loudest,
the absolute difference in dB after level matching; and their mean. Smaller is closer. These are
measurements of the same recording, not a realism or listening score. Without an audio output the
command still reports the comparison and says it played nothing. Auditioning changes neither the
project nor its undo history.

## Shared jobs and commands

`instrument.fitSample` accepts the same track and full device identity as root analysis, but
requires Instrument. It shares the session's single retained preparation job and process-wide
two-worker admission budget. `instrument.analysis` reports job kind, settings/version, progress,
result and limits; `instrument.cancelAnalysis` cancels either kind. `instrument.acceptFit`
requires the exact current ready job and uses the shared profile-adoption helper. A removed or
replaced device, changed source/PCM/trim/root or newly adopted profile invalidates the proposal.
Failed edits do not consume it. Cancellation is checked between bounded scanning chunks,
resampling chunks, frequency refinements and mode measurements; no UI join or audio-thread work.

The canonical fixed-capacity model, validated codec, revision routing and native render path
belong to the modal-profile prerequisite. The fitter creates that data; it has no second
renderer or state format. Source buffers are borrowed through immutable handles. Working
buffers and candidate counts have fixed caps independent of the complete source duration.
The FFT dependency is the already-used, permitted RustFFT version in this workspace; preparation
alone invokes it. Ordinary use is offline.

## Validation and remaining work

Deterministic CI fixtures use original analytic inharmonic decays with known frequencies and
T60, not the fitter's own synthesized output as the sole oracle. They cover hostile inputs,
silence, growing and noisy sources, sustained (constant) tones, repeatability, opposite-polarity stereo, cancellation
and temporal estimation limits. Session tests cover explicit/stale adoption, transaction
rollback, undo/redo, save/reopen and native render consistency. Builds and test execution for
this change occur in CI only at the maintainer's request.

This is the bounded first fitting slice tracked by #1646, extended in #1599 with attack
residuals, two-part decays, beating partners and parameter-space fusion of up to 32 recordings
by key and velocity. Full fitting #1624 retains feature locks beyond root/trim, morphing
controls, room and microphone separation, independent held-out notes and gestures, level-matched
candidate audition before acceptance and musician evidence.
F11's calibrated-model, content and release gates remain open.

## Sources

- Lagrange and Scherrer, [Two-Step Modal Identification for Increased Resolution Analysis of
  Percussive Sounds](https://www.dafx.de/paper-archive/2008/papers/dafx08_21.pdf), DAFx 2008,
  sections 2–5: damped mode parameters, Fourier candidates, temporal decay fitting and limits
  of unresolved modes. Suhina implements its own bounded Fourier approximation, not their
  ARMA/ESPRIT second stage.
- Skare and Abel, [Rapid Approximation of Weakly-Nonlinear Mode Parameters](https://aes.org/publications/elibrary/elibrary-page/?id=20974),
  AES 2020, public abstract: separated-window estimates trade precision for speed. No private
  paper, code or recordings are used.
- [RustFFT documentation](https://docs.rs/rustfft/6.4.1/rustfft/): fixed-size planning, explicit
  scratch and unnormalized Fourier output.
- Suhina's shared modal bank, resampler, sample preparation jobs and profile codec; original
  Rust implementation and analytic test signals.
