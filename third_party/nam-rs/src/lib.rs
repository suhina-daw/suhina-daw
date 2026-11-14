#![deny(missing_docs)]
//! # nam-rs
//!
//! Pure-Rust, real-time-safe inference for [Neural Amp Modeler] (NAM) `.nam` models.
//!
//! This crate loads a `.nam` model file and runs its neural network forward pass —
//! a whole buffer at a time (WaveNet uses a cache-friendly block kernel), or one
//! sample at a time — with no heap allocation on the audio (hot) path. It is a
//! from-scratch Rust port of NAM's inference, written against the reference
//! implementations below and validated for **parity** against them (per-sample
//! error within `1e-5`, the same tolerance NeuralAudio uses).
//!
//! Three model shapes are supported through the architecture-agnostic [`Model`] enum,
//! which dispatches on the `.nam`'s declared architecture so you never branch on it
//! yourself: **WaveNet**, **LSTM**, and **SlimmableContainer** (NAM "A2"). A
//! `SlimmableContainer` is a thin multiplexer over a set of complete standalone
//! submodels (each WaveNet or LSTM); a width dial selects the active one as a
//! CPU/quality trade-off. Drive it via [`Model::as_slimmable_mut`] →
//! [`Slimmable::set_slim_size`] / [`Slimmable::select`]. Switching is real-time-safe
//! (a single index write); each submodel keeps its own state, so switching mid-stream
//! leaves a short warmup transient on the newly-selected submodel.
//!
//! The A2 feature set is supported: FiLM, gating, bottleneck, grouped convs, the
//! per-array head (incl. multi-tap conv heads), the optional post-stack head
//! (`activation → Conv1d` chain run after the arrays, with `head_scale` scaling its
//! input), and a `condition_dsp` (a nested model whose output replaces the
//! conditioning fed to every array — including a multi-channel-output one, whose N rows
//! become the N-wide conditioning fed to every array). The remaining restrictions —
//! multi-channel input (`in_channels != 1`), a post-stack head with `out_channels != 1`,
//! mixed gating modes within one array, and exotic
//! activations — are **rejected** with [`Error::UnsupportedFeature`] (or
//! [`Error::UnsupportedActivation`]) rather than silently mis-run.
//!
//! **Sample rate.** A `.nam` is captured at a specific rate
//! ([`NamModel::expected_sample_rate`], 48 kHz when the file does not say). `nam-rs`
//! does *not* resample — you must feed the model audio at that rate, or resample in
//! your host first. A mismatched rate produces silently wrong output, because the
//! model's dilations and recurrence are defined in samples, not seconds.
//!
//! **Processing boundary.** `nam-rs` runs only the model's forward pass (plus its
//! `head_scale`). The reference NAM plugin additionally applies a DC blocker
//! (high-pass) and, optionally, loudness normalization on the output — those are the
//! host's responsibility, not the model's, so they live in your audio graph, not here.
//! The calibration accessors ([`NamModel::loudness`] etc.) give you the numbers to do
//! that gain-staging yourself.
//!
//! **Metadata.** [`NamModel::metadata_typed`] parses the file's `metadata` block into
//! a [`Metadata`] struct — name, who captured it, gear make/model/type, tone type,
//! trainer, export date, loudness and calibration levels — everything a model browser
//! shows and nothing the forward pass uses. See [`Metadata`] for the full field list;
//! all fields are optional and parsed leniently, and the unparsed block remains on
//! [`NamModel::metadata`]. One derived helper, [`Metadata::includes_cab`], answers the
//! question that changes a signal chain: whether the capture already contains a
//! speaker cabinet, and so whether an impulse response after it would be a second one.
//!
//! ## Design contract
//!
//! 1. **Parity with the reference.** The forward pass must produce output equal
//!    (within float tolerance) to the canonical Python/C++ NAM implementations for
//!    the same `.nam` file and input. This is enforced by `tests/parity.rs`.
//! 2. **Real-time safety.** The runtime's `process_buffer` (on both [`WaveNet`] and
//!    [`Lstm`], reached via [`Model`]) performs zero heap allocations, locks, or
//!    system calls. All scratch buffers are pre-allocated at construction. This is
//!    enforced by `tests/rt_safety.rs`.
//!
//! ## Example
//!
//! ```
//! use nam_rs::{NamModel, Model};
//!
//! // From disk you'd use `NamModel::from_file("model.nam")?`.
//! // Here we use a tiny in-line model for illustration.
//! let json = r#"{
//!     "version": "0.5.4", "architecture": "WaveNet",
//!     "config": {
//!         "layers": [{
//!             "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
//!             "kernel_size": 1, "dilations": [1], "activation": "ReLU",
//!             "gated": false, "head_bias": false
//!         }],
//!         "head": null, "head_scale": 1.0
//!     },
//!     "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]
//! }"#;
//!
//! let model = NamModel::from_json_str(json)?;
//! let mut amp = Model::from_nam(&model)?;   // picks the architecture from the file
//!
//! // On the audio thread: process in place, no allocation.
//! let mut buffer = [0.1_f32, 0.2, 0.3, 0.4];
//! amp.process_buffer(&mut buffer);
//! # Ok::<(), nam_rs::Error>(())
//! ```
//!
//! ## Attribution
//!
//! This is a derivative work. The algorithm and weight layout are ported from the
//! following projects (see `NOTICE` for license texts):
//!
//! - [neural-amp-modeler] — Steven Atkinson's reference trainer + `.nam` exporter
//!   (Python, MIT). The source of truth for `export_weights` / `export_config`.
//! - [NeuralAmpModelerCore] — the canonical C++ inference library (MIT).
//! - [NeuralAudio] — Mike Oliphant's high-performance C++ NAM/RTNeural runtime,
//!   designed to match NAM Core exactly (MIT). Primary porting reference.
//!
//! [Neural Amp Modeler]: https://www.neuralampmodeler.com/
//! [neural-amp-modeler]: https://github.com/sdatkinson/neural-amp-modeler
//! [NeuralAmpModelerCore]: https://github.com/sdatkinson/NeuralAmpModelerCore
//! [NeuralAudio]: https://github.com/mikeoliphant/NeuralAudio

mod error;
mod fast_math;
mod lstm;
mod model;
mod model_runtime;
mod reader;
mod wavenet;

pub use error::Error;
pub use lstm::Lstm;
pub use model::{
    ActivationSpec, Date, FilmConfig, GatingMode, Head1x1Config, Layer1x1Config, LayerArrayConfig,
    LstmConfig, Metadata, ModelConfig, NamModel, PostStackHeadConfig, SlimmableConfig,
    SlimmableSubmodel, WaveNetConfig, DEFAULT_SAMPLE_RATE,
};
pub use model_runtime::{Model, Slimmable};
pub use wavenet::WaveNet;
