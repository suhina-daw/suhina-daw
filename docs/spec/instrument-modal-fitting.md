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
undo step. The sample remains available as the source. Existing model-profile adoption and
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
early decay. The result contains at most 16 modes within the canonical profile's bounds.
Constant tones, rising envelopes, noise and short sounds yield no model; the playable sample
remains usable.

Report errors are RMS dB differences of a mode's amplitude envelope, not waveform errors,
probabilities, realism percentages or independent listening results. Odd windows belong to
the **same recording**: they do not establish held-out note, velocity, gesture or session
performance. Mode discovery itself has a fixed small search budget; no converged global
optimum is claimed. Initial phases, attack waveform, excitation/contact physics, room response,
nonlinear dynamics and coupled modes are not reconstructed. The source may contain these
components; fitted resonances must not be presented as a uniquely recovered physical body.

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
silence, growing/constant/noisy sources, repeatability, opposite-polarity stereo, cancellation
and temporal estimation limits. Session tests cover explicit/stale adoption, transaction
rollback, undo/redo, save/reopen and native render consistency. Builds and test execution for
this change occur in CI only at the maintainer's request.

This is the bounded first fitting slice tracked by #1646. Full fitting #1624 retains coherent
multi-recording/dynamic fitting, feature locks beyond root/trim, independent held-out notes and
gestures, level-matched candidate audition, residual/excitation modelling and musician evidence.
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
