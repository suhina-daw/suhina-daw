---
description: An experimental eight-line reverb core, reproducible listening files, acoustic analysis and CPU measurements
read_when: FDN prototype, reverb research, CPU budget, reverb measurement, listening renders
type: spec
status: planned
last_verified: 2026-10-23
covers:
  - crates/dsp/src/fdn.rs
  - crates/dsp/src/fdn_tests.rs
  - crates/dsp/src/fdn_scatter.rs
  - crates/dsp/src/fdn_diffuse.rs
  - crates/dsp/src/fdn_diffuse_tests.rs
  - crates/dsp/examples/fdn_probe.rs
  - crates/dsp/examples/fdn_probe/**
  - scripts/reverb/**
---

# Reverb prototype

The first research slice of [#897](https://github.com/suhina-daw/suhina-daw/issues/897)
is an experimental eight-line feedback delay network (FDN). It is a reusable DSP block
and a developer probe, not a registered device or an accepted production sound. The
existing `reverb` and its import mapping are unchanged. This probe processes the same
block that a future native device would use; integration through the engine, commands,
automation and the SDK panel remains to be done before the effect can ship.

## Signal and controls

Eight fixed integer delays feed a normalized Hadamard matrix followed by a cyclic
permutation. This transformation preserves energy. Each line's feedback gain is
`10^(-3 * delay_seconds / decay_seconds)`. An optional first-order high shelf makes
the high-frequency asymptotic decay a fraction of the low-frequency decay, with
delay-proportional loss. This simple baseline does not implement the two-stage
attenuation design from the references yet.
The shared shelf limits its per-trip gain to at least 0.001; extreme combinations
can therefore miss the requested high-decay ratio. The ratio is an experimental
target, not a guaranteed bandwise decay time; validate the resulting response.

The independent reference delays are 1429, 1777, 2081, 2383, 2719, 3083, 3469 and
3911 frames at 48 kHz, scaled to the sample rate and room scale. They are exploratory
choices, not extracted from a product or claimed to be perceptually optimal. Mono
excitation is equally distributed; two orthogonal output sign vectors provide
stereo. No early reflections, input diffusion or modulation are present initially.
Do not infer a production quality claim from a flat spectrum or an orthogonal matrix.

Construction accepts sample rates 8 through 384 kHz, decay 0.2 through 20 seconds,
room scale 0.5 through 2 and high-decay ratio 0.1 through 1. Invalid and non-finite
settings return errors before allocation. Construction allocates bounded delay
history; processing and reset do not allocate. Non-finite input becomes silence,
finite input is limited to +/-4, and tiny internal states are flushed to zero.
Output is wet-only stereo. The mono sum cancels opposite-polarity input; stereo
input preservation is outside this baseline. Odd interleaved trailing samples
are ignored. The core has no live parameter setters: reconstruct it off-thread.

## Probe and files

Run from a source checkout, with a release Rust toolchain:

```powershell
cargo run --release -p suhina-dsp --example fdn_probe -- target/reverb-prototype
python scripts/reverb/analyze.py target/reverb-prototype --output target/reverb-prototype/analysis.json
```

Use a Python environment with `pip install -r scripts/reverb/requirements.txt`.
Its NumPy and SciPy versions are recorded in the report. The JSON report has a CSV
companion. Existing report/output paths are refused rather than overwritten.

For a recorder capture of repeated impulses 20 seconds apart:

```powershell
python scripts/reverb/analyze.py capture.wav --confirm-impulse-response --period-seconds 20 --output capture-analysis.json
```

The first response onset is detected from the signal, or given with
`--first-onset-seconds`. Periodic splitting assumes constant spacing and no time
stretching. A partial final period is retained if it has at least half a second;
the decay validity check can reject its insufficient tail. An optional
`--baseline reference-impulse.wav` adds descriptive decay ratios to each result.
It expects a single impulse response, not a music file or a whole repeated capture.
These ratios are comparisons, not perceptual quality scores.
For a directory of external impulse WAVs, give the directory with
`--confirm-impulse-response`; this explicitly treats every WAV there as an IR.
The report includes octave-band energy in fixed tail windows, referenced to the
detected response onset. The filtering is causal; low-band ringing can affect
very short decay estimates and requires cross-checking before acceptance.

The probe creates original deterministic impulse, decaying noise percussion and
harmonic-tone stimuli, and wet-only and fixed dry/wet listening WAVs for a short
room, a two-second baseline and a long tail. These synthetic signals are not a
replacement for a maintainer listening to voice and recorded instruments. WAVs
are 32-bit float, stereo, at 48 kHz, without peak normalization. Stimulus amplitude,
settings, history bytes, build identity and analysis settings accompany results.
Listen quietly: wet levels differ, and listening files are not loudness matched.

For a controlled decay sweep, append `--flat-decay` to the probe command and use
a fresh output directory. This keeps room scale at 1 and high-decay ratio at 1
for all three decay targets (0.5, 2 and 6 seconds). Only decay time changes;
the ordinary listening presets above remain available. Compare wet-only responses
to captures made with frequency-dependent damping disabled. Matching numerical
controls does not establish equivalent algorithms or perceptual quality.

The analyzer also accepts a stereo or mono WAV path, including an impulse captured
by a recorder after an effect in a DAW. Explicitly select IR analysis only for an
actual impulse response: arbitrary music cannot establish the decay time of the
effect. In directory mode only named impulse responses are analyzed. It reports
Schroeder-integrated EDT/T20/T30 with fit quality and dynamic-range checks, bandwise
decay, stereo correlation and early echo-density windows. Invalid fits are null
with reasons. A truncated tail or noise floor can invalidate the result; silence
and non-finite input never produce a successful fit. This is not ISO certification.

The analysis environment needs NumPy and SciPy; these are development tools, not
Rust or application dependencies. Tests use analytical exponential decays, silent
and truncated responses and malformed input. REW/API cross-checking is a later
part of #897; the first probe must work without REW installed.

## Second research iteration: onset, density and tone

The `fdn_diffuse` wrapper retains the first core as a reproducible baseline. Before
implementation, local self-authored impulse observations identified a roughly
30 ms onset, sparse initial response, low channel correlation and excessive top
octave energy as useful hypotheses to test. The external captures have a much
earlier onset, denser initial response and a different spectral envelope. These
observations are descriptive, not proof of an underlying proprietary topology.
Raw captures and derived per-product reports stay outside the repository.

The original design for the next experiment adds a bounded predelay, a small
stereo multitap early-reflection path, four serial Schroeder input allpasses,
first-order output high/low-pass filters and mid/side width. The allpasses reuse
the shared `Allpass` block. They remain outside the FDN feedback loop so that
their phase delay does not require changing its per-loop attenuation formula.
Early-reflection delays and signs are independent design choices, not copied
impulse samples or a recovered topology. No reference IR is embedded or convolved.
No modulation is added in this experiment, to isolate changes in initial density.

The wrapper accepts finite predelay 0..100 ms, diffusion coefficient 0..0.8,
early-reflection level 0..2, width 0..1, low cut 20..1000 Hz and high cut
1000 Hz..45% of sample rate. It shares the core's construction validation,
bounded history, off-thread construction and allocation-free processing/reset.
Tone controls change the output spectrum without changing frequency-dependent
feedback loss. Decay and density still require measured verification.

The developer probe's `--diffused` mode renders and benchmarks this wrapper;
`--settings <json>` supplies optional local per-render settings, with explicit
measured decay targets and all wrapper settings recorded in its manifest.
`--render-only` skips CPU timing during parameter searches. An accepted candidate
must receive a fresh release CPU run after the search. The structural comparison
uses onset relative to the known stimulus, channel energy, crest factor,
time-windowed density, stereo correlation, normalized octave energy and decay.
It reports each metric separately: no aggregate score establishes sound quality.
Listening copies may receive one fixed gain per file to equalize full-tail energy;
original responses stay unchanged and gains are recorded. Equal energy is not
perceptual loudness matching, and no onset shift is applied to conceal timing.

Use `--impulse-only` to limit parameter searches to impulse files. Local settings
JSON has `short`, `baseline`, `long` objects (and an optional `benchmark` object),
with keys `decay_seconds`, `room_scale`, `high_decay_ratio`, `predelay_ms`,
`diffusion`, `early_level`, `early_tilt_db`, `early_diffusion`, `early_onset_gain`,
`late_scale`, `loop_diffusion`, `width`,
`low_cut_hz`, `high_cut_hz`.
Unknown setting keys and nonnumeric values are refused. Settings files are bounded
to 64 KiB. Do not infer a control mapping from three independently fitted captures.

```powershell
cargo run --release -p suhina-dsp --example fdn_probe -- target/reverb-diffused --flat-decay --diffused
python scripts/reverb/compare_ir.py reference.wav target/reverb-diffused/baseline-impulse-wet.wav --previous target/reverb-prototype/baseline-impulse-wet.wav --output target/reverb-comparison
python scripts/reverb/plot_comparison.py target/reverb-comparison/comparison.json --output target/reverb-comparison/diagnostics.png
```

The comparison requires identical sample rates and recording dimensions and assumes
the same known excitation and common input-impulse time (`--stimulus-onset`, default
one second). It refuses silent responses and existing output directories. Reports
use source names and hashes, without absolute machine paths. JSON includes curves
and bandwise data; CSV contains the scalar diagnostics. Matplotlib is a development
dependency for static plots. None of these tools performs a subjective listening test.

## Third iteration: short-response energy timing

Listening feedback on the short response described a trailing onset envelope.
Local measurements separate the full decay time from cumulative energy in the
first 10/20/50/100/200 ms. A matching T30 does not guarantee matching energy timing;
the initial fitted response retained too much energy for the late part of its tail.

Before implementation, the next design keeps the same FDN and diffusion and adds
`early_tilt_db` (-12..12 dB, default 0). It applies a gain ramp over the independent
early-reflection pattern: tap gain is multiplied by
`10^(early_tilt_db * tap_position / 20)`, with position 0 at the first tap and 1
at the last. Coefficients are prepared off-thread; processing still uses eight
fixed tap gains. A zero tilt reproduces the second iteration. Early level sets
the early/late balance, while tilt controls the distribution within the early
pattern. Width and decay are rechecked after fitting; other listening settings
remain unchanged unless new evidence calls for a change.

Development-only fitting uses independently rendered native early/late bases and
bounded optimization of cumulative-energy fractions and channel correlation.
It matches separate diagnostics, not waveform samples or a perceptual score.
Final files must be rendered by the native core and compared against the basis
prediction. Fitted reference-specific settings and raw captures remain local.
Quantile arrival times (10/50/90% of total channel energy) and the energy centroid
supplement the existing decay/density diagnostics. They retain the original input
clock and are distinct from reverberation decay-time estimates.

`scripts/reverb/fit_early.py` takes a reference IR, a native `early_level=0` render
and a native `early_level=1` render with otherwise identical settings, including
the stated nonzero `--basis-width`. It requires matched stereo sample rates and
dimensions, rejects silent/missing bases and unidentifiable parameters, and fits
early level 0..2 plus width 0.05..1. The finite-tilt search is performed by making
new unit-early native renders, not by altering a reference recording. Its output
records individual energy/correlation residuals and source hashes. The generated
settings are local experiment inputs, not built-in presets.
Plot labels can identify successive rounds using `--previous-label` and
`--candidate-label`, without changing the reported source identities.

## Fourth iteration experiment: independent early spreading

The early-reflection path can optionally pass through two short integer-delay
Schroeder allpasses per output channel before summing with the late network.
`early_diffusion` is a fixed construction setting in 0..0.8, default 0 (bypass).
The delays are 23 and 61 samples on the left and 31 and 73 on the right at 48 kHz,
scaled only by sample rate, not room scale. These are exploratory values.
This isolates early peak concentration from input diffusion and feedback decay.
The allpasses preserve isolated-path magnitude/energy in the ideal arithmetic
model but change phase and temporal distribution; early/late interference means
the combined response need not preserve energy or spectrum. They have no pure
predelay because of their direct term. They do not alter feedback attenuation.

The experiment is motivated by measured early concentration, not a claim that
more diffusion necessarily sounds better. Validate onset, cumulative energy,
peak concentration, stereo and tonal attack/release together. Zero early level
must leave the late path identical; default bypass must reproduce old renders.
Reset, block independence, finite bounds and history accounting cover the new
state. Re-measure CPU before accepting a nonzero setting.

### Independent late-network scale

`late_scale` (0.5..2, default 1) multiplies the core room scale only for FDN
construction. The resulting core scale must still be in 0.5..2. Early taps and
input diffusion retain the original room scale. This separates recurrence timing
from the early pattern in controlled experiments. Feedback attenuation is prepared
from the effective delay lengths, preserving the requested homogeneous decay.
The probe records both room_scale and late_scale; their product is the effective
network scale. It remains an experimental fixed setting, not a device control.

### Optional loss-calibrated feedback scattering

`loop_diffusion` is fixed in 0..0.7, default zero bypass. Each FDN delay output
passes through one short Schroeder section before output projection and feedback.
Section delays are [43,59,73,89,107,127,149,173] frames at 48 kHz, scaled by rate.
For section length d, gain g and homogeneous amplitude loss per sample gamma,
q = gamma^d and H(z) = (q z^-d - g)/(1 - g q z^-d).
The original delay retains gamma^m feedback attenuation. Thus every delay in the
lossless feedback characteristic has the same substitution z^-1 -> gamma z^-1;
the new internal storage does not silently lengthen homogeneous decay. At gamma=1
the section is allpass. At gamma<1 it is contractive, not magnitude-flat. Damping
with high_decay_ratio below 1 remains approximate and requires separate bandwise
validation. This optional experiment follows the IIR filter-feedback formulation
in Schlecht and Habets (2020), Scattering in Feedback Delay Networks, section III A;
its delay values and code are original exploratory choices.

### Early onset balance

`early_onset_gain` (0..8, default 1) scales only the first early tap, prepared
off-thread. This separates immediate early energy from later taps and the late
network. A bounded basis fit may vary it with early level and width using energy
windows, early crest and correlation. Tonal validation is kept separate from the
fit. It is an original envelope control, not an extracted proprietary parameter.

### Direct dry-file verification

The probe accepts `--input <path.wav> --profile short|baseline|long` alongside
`--settings`, `--flat-decay` and `--diffused`. This developer-only mode renders a
supplied stereo 32-bit IEEE float WAV through the native core, in fixed 128-frame
blocks, without normalizing or resetting between blocks. Input is bounded to
128 MiB and 8..384 kHz; malformed formats and non-finite samples are rejected.
`input-wet.wav` and `input-manifest.json` record the output and exact settings.
The input must already contain enough silence for the tail. No captured audio
or reference-fitted parameter files belong in the repository.

## Modal-excitation experiment

Two optional fixed angles, `input_rotation` and `output_rotation`, are bounded to
-pi..pi radians and default to zero. Preparation rotates the uniform input vector
into the alternating-sign unit vector. The left/right output vectors rotate into
two further mutually orthogonal Hadamard sign vectors. Each input/output vector
retains its Euclidean norm; the two output vectors remain orthogonal. These gains
are prepared off-thread, with no trigonometry or allocation during processing.
The feedback matrix, delay lengths and decay losses do not change. Thus the
experiment changes modal excitation/observation, not the pole decay law.

This original restricted parameterization follows the distinction between modal
frequencies and excitation in Heldmann and Schlecht (2021), sections 2 and 3,
and the joint input/output-gain optimization motivation in Dal Santo et al.
(2025), sections 2.4, 2.5 and 3. It does not reproduce either paper's optimizer
or claim an allpass FDN. Zero angles retain the previous transfer response.

Evaluate envelopes, tail-energy fractions and mono loss separately across several
source pitches. Separate tuning and validation pitches; do not accept improvement
on the tuning set alone. Fit local reference observations outside the public tree.
Check native renders against linear basis predictions, decay, reset, malformed
angles, and CPU before offering a listening candidate.

### Orthogonal feedback rotation

`feedback_rotation` (-pi..pi, default zero bypass) adds four independent Givens
plane rotations after the Hadamard/permutation mix, with angle multipliers
[1, 0.7, -0.9, -0.5]. Coefficients are prepared at construction. Each two-by-two
rotation is orthogonal, so the whole feedback transform remains orthogonal and
delay-proportional loss still controls homogeneous decay. Unlike input/output
rotation, this changes modal frequencies. The multipliers are original exploratory
choices, not measured or extracted from a reference product. This is a restricted
orthogonal parameterization motivated by Dal Santo et al. (2025), section 3.2;
it is not that paper's matrix-exponential optimizer. Zero bypass must preserve
existing renders. Nonzero operation adds bounded arithmetic and needs a new CPU
measurement. Validation data must remain separate from parameter selection.

## Cost gate

CPU timing begins with the first runnable core. The probe reports repeated release
measurements for 48/96 kHz, 64/128-frame blocks and 1/8/16 independent instances,
with active input, silence and a decaying tail. Preparation, signal generation,
allocation and disk writes are outside timed spans. A warm-up precedes each trial.
Median, p95, p99 and observed maximum are for the whole group of instances per
block; percentages compare that work to one block's deadline on one core. Report
the build, OS and CPU alongside the JSON; shared-machine noise is not a guarantee
of worst-case execution time. Memory is allocated delay history, not process RSS.

Initial engineering targets on the development reference machine (Windows,
AMD Ryzen 5 5600X, release build) are: median aggregate throughput at most 1% of
one core per instance at 48 kHz, p99 group time below 50% of the block deadline
for 16 instances at 48 kHz/64 frames, and less than 1 MiB delay history per instance
at 96 kHz/scale 2. These are provisional gates to measure, not claimed results or
portable thresholds. Acoustic quality and low CPU are independent requirements.
Do not add lines or modulation without remeasuring both. Actual app playback,
underruns and parameter-movement cost require the later device integration.

## Sources

Original implementation of published methods; no upstream code or paper text copied.

- Heldmann and Schlecht (2021), *The Role of Modal Excitation in Colorless
  Reverberation*, DAFx: modal excitation balance matters alongside modal density.
- Dal Santo, Prawda, Schlecht and Välimäki (2025), *Optimizing tiny colorless
  feedback delay networks*, DOI: 10.1186/s13636-025-00401-w: tune spectral
  coloration and temporal density together; validate perceptually.
- Välimäki, Prawda and Schlecht (2024), *Two-Stage Attenuation Filter for Artificial
  Reverberation*, DOI: 10.1109/LSP.2024.3352510: candidate future damping design.
- Dattorro (1997), *Effect Design, Part 1: Reverberator and Other Filters*: input
  diffusion and tone control as independent parts of a reverberator.
- Dal Santo et al. (2024), *RIR2FDN: An improved room impulse response analysis
  and synthesis*, DAFx24: evaluate energy decay, density and spectral shape together;
  matching measured responses remains an analysis and listening problem.
