# nam-rs

[![crates.io](https://img.shields.io/crates/v/nam-rs.svg)](https://crates.io/crates/nam-rs)
[![docs.rs](https://docs.rs/nam-rs/badge.svg)](https://docs.rs/nam-rs)
[![CI](https://github.com/OpenSauce/nam-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/OpenSauce/nam-rs/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/nam-rs.svg)](LICENSE)

Pure-Rust, real-time-safe inference for [Neural Amp Modeler](https://www.neuralampmodeler.com/) (NAM) `.nam` models.

`nam-rs` loads a `.nam` model file and runs its neural-network forward pass — a whole
buffer at a time (WaveNet uses a cache-friendly block kernel) or one sample at a time —
with **no heap allocation on the audio thread**, suitable for use inside a JACK
callback, a VST3/CLAP `process()`, or any real-time audio graph.

**WaveNet**, **LSTM**, and **SlimmableContainer** (NAM "A2") models all load through
one entry point, `Model::from_nam`, which dispatches on the file's declared
architecture.

## Design contract

1. **Parity with the reference.** Output must match the canonical Python/C++ NAM
   implementations within `1e-5` per sample for the same model and input. Enforced by
   `tests/parity.rs` against reference-generated fixtures.
2. **Real-time safety.** `process_buffer` (every architecture, reached via `Model`)
   performs zero heap allocation, locks, or syscalls; all scratch buffers are
   pre-allocated at construction. Enforced by `tests/rt_safety.rs` via
   `assert_no_alloc`.

## Install

```bash
cargo add nam-rs
```

MSRV: Rust 1.74. No C/C++ toolchain or native dependencies needed.

## Usage

```rust
use nam_rs::{Model, NamModel};

// Off the audio thread: load + allocate. `Model::from_nam` dispatches on the
// model's architecture, so the same code runs WaveNet and LSTM `.nam` files.
let model = NamModel::from_file("twin_reverb.nam")?;
let mut amp = Model::from_nam(&model)?;

// On the audio thread: in-place, allocation-free. Call once per audio block;
// state carries across calls, so block-wise output matches one whole-buffer call.
let mut audio_buffer = vec![0.0_f32; 512]; // your host's block, filled with input
amp.process_buffer(&mut audio_buffer);
```

To smoke-test a model file without writing any code:

```bash
cargo run --release --example run_model -- path/to/model.nam
```

(`examples/streaming.rs` shows the block-wise hot-path loop in full.)

For WaveNet models, the first `Model::receptive_field()` output samples are a startup
transient (the dilated stack filling against zero-history) — the model's inherent
latency, the same convention NAM Core / NeuralAudio use. LSTM models have no such
warmup. Call `Model::reset` to return to silence.

**Sample rate.** A `.nam` expects audio at the rate it was captured
(`NamModel::expected_sample_rate()`, 48 kHz if the file omits it). `nam-rs` does not
resample: feed the model audio at that rate, or resample in your host first. A
mismatched rate produces silently wrong output, since the model's dilations and
recurrence are defined in samples, not seconds.

**Processing boundary.** `nam-rs` runs only the model's forward pass. The reference
NAM plugin additionally applies a DC blocker (high-pass) and, optionally, loudness
normalization on the output — those belong to the host's audio graph, not the model.
The calibration accessors (`NamModel::loudness()` etc.) give you the numbers for that
gain-staging.

## Metadata

`NamModel::metadata_typed()` parses the file's `metadata` block into a `Metadata`
struct — everything a model browser needs to show, none of it used by the forward
pass. Every field is `Option`, since any of them may be absent.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Option<String>` | The author's name for the model, often better than the filename |
| `modeled_by` | `Option<String>` | Who captured it |
| `gear_make` | `Option<String>` | e.g. `"Marshall"` |
| `gear_model` | `Option<String>` | e.g. `"JMP-50"` |
| `gear_type` | `Option<String>` | What was captured, and how much of the chain: `amp`, `pedal`, `amp_cab`, `full-rig`, … |
| `tone_type` | `Option<String>` | `clean`, `overdrive`, `crunch`, `hi_gain`, `fuzz`, … |
| `trainer` | `Option<String>` | Which trainer produced the file, e.g. `"TONE3000"` |
| `date` | `Option<Date>` | Export timestamp (`year`…`second`; ordered chronologically) |
| `loudness` | `Option<f32>` | How loud the model is against NAM's standardized input, in dBFS RMS — not LUFS, and not perceptual |
| `input_level_dbu`, `output_level_dbu` | `Option<f32>` | Analog dBu at 0 dBFS — the calibration numbers for gain-staging |
| `gain` | `Option<f32>` | The trainer's `0.0..=1.0` estimate of how much gain/compression the model has |
| `training` | `Option<serde_json::Value>` | The trainer's raw training record, left untyped |

Call `metadata_typed()` once and keep the struct: the single-field shortcuts on
`NamModel` (`loudness()`, `input_level_dbu()`, `output_level_dbu()`) each re-parse the
raw JSON. All of it is load-time, never the audio thread. The unparsed block stays
available as `NamModel::metadata` if you need a key we don't type, and parsing is
per-field lenient — one malformed entry yields `None` for itself instead of discarding
the whole block.

`gear_type` and `tone_type` are `String`, not enums, because there is no single
vocabulary: NAM's trainer and TONE3000 use overlapping-but-different sets, TONE3000
has already deprecated values that exist in files on disk, and real captures write
values in neither (`"vintage"`, `"T3K-Null"`). An enum would reject those files.

For the one question that actually changes a signal chain — is a speaker cab already
baked into this capture, so that adding an IR would be a second one? —
`Metadata::includes_cab()` (also on `NamModel`) classifies `gear_type` across both
vocabularies, ignoring case, surrounding whitespace, and `-` vs `_`:

```rust
let model = NamModel::from_file("model.nam")?;

match model.includes_cab() {
    Some(true) => println!("file says the cab is baked in — a second one would be an IR too many"),
    Some(false) => println!("file claims no cab — but `amp` is also the do-nothing default"),
    None => println!("unknown — don't touch the signal chain"),
}
```

It returns `None` rather than guessing at a value it doesn't recognize, and it only
ever reports what the file claims: `gear_type` is author-supplied and captures do
mislabel themselves, so treat the answer as a default to offer the user, not a fact to
reroute audio on silently. The two answers are not equally strong — `Some(true)`
required someone to actively pick a cab-ish value, while `Some(false)` is mostly
`"amp"`, which is what you get by leaving the field alone. One of the three real
captures in this repo's test corpus is a full rig tagged `gear_type: "amp"`.

## Supported architectures

- **WaveNet** (A1 and A2 single models) — dilated-conv forward pass, parity-tested.
- **LSTM** — recurrent forward pass, parity-tested.
- **SlimmableContainer** (NAM "A2") — a set of complete standalone submodels (any mix
  of WaveNet/LSTM) with a runtime width dial as a CPU/quality trade-off. Select via
  `as_slimmable_mut()` → `set_slim_size` or `select`; switching is real-time-safe.
  See the [crate docs](https://docs.rs/nam-rs) for the selection semantics.

The A2 feature set is covered: FiLM, gating, bottleneck, grouped convs, multi-tap conv
heads, the optional post-stack head, and `condition_dsp` (a nested model that generates
the conditioning signal, multi-channel included). A few restrictions remain —
multi-channel *input*, a post-stack head with more than one output channel, mixed
gating modes within one array, and unrecognized activations — and these are rejected
with a descriptive error at load time rather than silently mis-run.

## Performance

Rough numbers from the included Criterion bench (`cargo bench`), standard-size
fixture models, one x86-64 desktop core, release + LTO:

- Standard WaveNet capture: ≈1.9 µs/sample via `process_buffer` (≈11× real-time at
  48 kHz). The block path is ~3.5× faster than per-sample, so prefer whole blocks.
- Standard LSTM capture: ≈1.2 µs/sample (≈17× real-time).

Numbers vary with CPU and model size — run `cargo bench` on your own target.

## Development

```bash
cargo test                                  # parser, parity, and RT-safety tests
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Parity fixtures are committed under `tests/fixtures/`; regenerate them from Python NAM
with `tests/fixtures/gen_fixtures.py` (see `tests/fixtures/README.md`).

## Attribution & license

`nam-rs` is MIT-licensed (see [`LICENSE`](LICENSE)). It is a **derivative work**: the
algorithm and `.nam` weight layout are ported from the projects below. Their license
texts are reproduced in [`NOTICE`](NOTICE).

| Project | Role | License |
| --- | --- | --- |
| [neural-amp-modeler](https://github.com/sdatkinson/neural-amp-modeler) | Reference trainer + `.nam` exporter (source of truth for weight/config layout) | MIT |
| [NeuralAmpModelerCore](https://github.com/sdatkinson/NeuralAmpModelerCore) | Canonical C++ inference library | MIT |
| [NeuralAudio](https://github.com/mikeoliphant/NeuralAudio) | High-performance C++ NAM runtime; primary porting reference | MIT |

`.nam` model files are licensed separately by whoever captured them; `nam-rs` ships
no model files.
