---
description: How stem separation behaves: an audio clip split offline into stems by a model the user supplies, the chunking, resampling and channels around the model, the test backend, limits, and what lands in the project
read_when: stem separation, separate stems, vocals drums bass other, separation crate, separation model, model manifest, inference backend, tract, ort, ONNX, overlap-add, chunking, window weighting, band splitter, test backend
type: spec
status: planned
last_verified: 2026-10-15
covers:
  - crates/separation/**
---

# Stem separation

An audio clip is split offline into its stems (vocals, drums, bass, other, or a two-stem split),
and the result lands as a group of new audio tracks that behave like any other audio. The user
chooses which model runs; each model states what stems it gives, where it came from and under what
licence.

- **Sources:** own design, from issue [#146](https://github.com/suhina-daw/suhina-daw/issues/146)
  and the research posted on it (public papers, the models' own repositories and licence
  statements). No other program was observed for it.
- **Status:** planned, built in steps (#146 lists them). Parts marked **(implemented)** exist in
  `crates/separation`; everything else is **(planned)**. Nothing a user can do changes yet: there
  is no command, no model and no menu item.

## Naming

This feature is *stem separation*; what it gives are *separated stems*. A *Stems file* is something
else (`.stem.mp4`, [`stems-format.md`](stems-format.md)), and nothing here is called one.

## Models and their licences (planned)

No widely used separation model is published under terms that allow both its code and its weights
to be shipped with a GPL-3.0 program: the weights are either for research only, under a
non-commercial licence, or of unstated licence, and most are trained on a corpus that is itself
non-commercial (#146 › The licence question). Therefore:

- Suhina ships no weights and downloads none. A user adds a model from a file they have
  (`separation.addModel`); it is kept in `<settings>/models/<id>/`.
- A model is described by a **manifest**, data and not code: id, name, architecture (which backend
  runs it), its stems (id, name, color), sample rate, channels, window, hop and weighting, each
  file with its SHA-256 and size, the rights holder's licence with its source URL and the date it
  was checked, the training data, and any credit it requires.
- A model whose licence is missing or not compatible with GPL-3.0 is listed with the reason and
  cannot run. A file whose SHA-256 differs from its manifest is refused by name, and nothing is
  written.
- Weights Suhina may ship one day are trained on permissively licensed or self-made audio; that is
  its own issue.

## The pipeline

The model sees one window of audio at its own sample rate and channel count, and gives each stem's
share of that window. Everything around it belongs to Suhina, whatever runtime runs the model, and
is tested without one.

```text
source clip ─resample─> model rate ─windows─> backend ─weighted overlap-add─> stems ─resample─> source rate
```

### Channels (implemented)

| Source | Model | What runs |
|---|---|---|
| mono | mono, or stereo | stereo model: the mono signal on both inputs, the two outputs averaged back to one |
| stereo | stereo | one pass |
| stereo | mono | one pass per side, so the stems stay stereo |

A source has one or two channels. Each stem comes back with the source's channels, sample rate and
length.

### Resampling (implemented)

A source at another rate than the model's is resampled to the model's rate and the stems back to
the source's. Band-limited interpolation: a Blackman-windowed sinc with 16 zero crossings on each
side, cut off at 0.94 of the lower Nyquist frequency, read from a table of 512 points per input
frame with linear interpolation, its gain at each position divided out so a constant stays
constant. The position of each output frame is computed in integers, so nothing drifts over a long
file and repeated runs agree to the bit. Above the cut-off the source cannot reach the model, so
the stems add up to the source as the model's rate can hold it.

### Chunking (implemented)

- **Windows.** The model's manifest gives a window length and a hop in frames at its rate; windows
  start at frame 0 and every hop after it, and overlap by `window − hop` frames.
- **The padded tail.** The last window starts at the first hop position from which it reaches the
  final frame; past the end of the source it is filled with silence, and that part of its output
  is dropped. A source shorter than one window is one padded window.
- **Weighting.** Each window's output is multiplied by a weighting before the windows are added
  up, and the sum of the weights at each frame is divided out, so the gain is exactly one at every
  frame whatever the weighting and the overlap. Three weightings: *flat*, *triangle* (rising from
  the window's edges to its centre, as htdemucs weights its windows) and *Hann* (sampled between
  its zeros, as MDX-Net weights its windows). Each is above zero at every frame, so the first and
  last frames, which one window alone covers, divide out exactly. With overlap, a weighting that
  falls towards the edges crosses neighbouring windows over gradually, so windows whose outputs
  differ meet without a click; with *flat* they meet with a step.
- **Memory.** One sum per stem and channel and one sum of weights per frame, shared by all of
  them: about `stems × channels × frames × 4` bytes, roughly 0.5 GB for six minutes of stereo at
  44.1 kHz and four stems. No stem is complete before the last window, so nothing is written until
  the run ends.

### Backends (implemented: the interface and the test backend)

A backend runs a model on one window: it states the model's shape and turns `[channel][frame]`
samples into `[stem][channel][frame]` samples. Windows are independent; the same window always
gives the same output. A failure in a backend stops the run with its message and the window's
number; a sample that is not a finite number stops it too, since a model that gives NaN gives
nothing usable.

- **The test backend (implemented).** A band splitter with no weights: second-order Butterworth
  low-passes at given crossovers, run over each window from silence; the first stem is the lowest
  low-pass, each next one the difference of two neighbouring low-passes, the last the input minus
  the highest. Its stems add up to its input by construction, so the pipeline's own error can be
  measured, and it runs the whole pipeline in `cargo test`.
- **The first real backend (planned).** An ONNX runtime behind a cargo feature, as `asio` is
  gated: `tract` (pure Rust) first, `ort` if a real model does not load or is too slow. The models
  that run today have no STFT in their graphs, so the STFT, its inverse and the masking for a given
  architecture are written in this crate, beside the chunking. An ADR records the runtime, how a
  model is delivered and checked, and where the separated audio lives.

### Progress and cancel (implemented in the crate; the job is planned)

Progress is windows done out of windows in total, over every pass, starting at 0. Cancel is
checked before every window; a cancelled run keeps nothing, since resuming would mean keeping the
sums (about the size of the stems). The job that runs it (one at a time, on a pool of half the
cores, a model kept loaded between jobs) belongs to the session (planned).

### Limits (implemented)

- Sample rates of source and model from 8 kHz to 384 kHz; a model takes one or two channels and
  gives 1 to 16 stems.
- A window from 16 to 2²⁴ frames; a hop from 1 frame to the window, and no smaller than the window
  divided by 16 (no frame under more than 16 windows).
- A source of one or two channels of equal, non-zero length, every sample finite, at most 30
  minutes long, and whose stems fit 2³⁰ samples at both rates. Everything is checked before any
  work starts.

## In the project (planned)

- **Output on disk.** Each stem is written as a WAV file, since a clip plays decoded audio found by
  its file path and an in-memory clip would not survive saving, reopening or relinking. Whether the
  files go beside the project or into the settings folder is the ADR's call; the missing-files flow
  ([`missing-files.md`](missing-files.md)) is the safety net either way.
- **Placement** as Stems files are placed: a group named after the source clip, one audio track per
  stem named and colored from the manifest, each clip at the source clip's position, and the
  source clip left where it is. One undo step removes the whole group.
- **Commands**, the same from the window, the CLI and MCP:
  - `separation.models`: every model with its stems, licence, source, whether it is installed and
    why it is unavailable;
  - `separation.addModel { path }`, `separation.removeModel { model }`;
  - `separation.separate { clip, model, stems? }`: starts a job and returns its id;
  - `separation.status { job? }`, `separation.cancel { job }`.
- **Window:** a clip context-menu item, a model picker and progress. Expect minutes per song on a
  desktop CPU, and say so.

## Tests

- **(implemented)** Chunking: unity gain on a constant for every weighting and several overlaps; an
  identity model gives the source back, the padded tail included; the padded tail window holds the
  last frames at its start and silence after them; windows whose outputs differ meet without a
  click (and the same windows with *flat* weighting show the step); mono and stereo in every
  combination; a source at another rate than the model's.
- **(implemented)** The stems of the test backend add up to the source within 1e-5 at the model's
  rate, and within 1e-3 after resampling there and back for a source below 0.4 times the lower
  rate; a golden render of the test backend over a fixed input
  (`crates/separation/tests/golden/README.md`); the same run twice is byte-identical.
- **(implemented)** Hostile input: every shape out of bounds, sources that cannot be separated,
  sources over the limits, a failing backend and one that gives NaN; progress from 0 to the window
  count, and cancel at the first, a middle and the last window.
- **(planned)** Hostile manifests (missing files, a SHA-256 mismatch, an unknown architecture, a
  stem list that does not match the model's output count); the job (progress, cancel, one undo
  step, two jobs at once refused, a project closed mid-run); a real model behind `#[ignore]` and an
  environment variable that names its file.

## Not in v1

- Training, fine-tuning or shipping any weights.
- Writing a Stems file from the result.
- GPU inference; CPU first.
- Separating while playing, on input or while recording.
- RoFormer-family models: they cut windows differently and the widely used checkpoints carry no
  licence.
