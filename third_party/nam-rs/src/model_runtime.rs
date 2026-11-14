//! Architecture-agnostic runtime: [`Model`] dispatches over the `.nam`'s declared
//! architecture so consumers run any supported model without branching.

use crate::error::Error;
use crate::lstm::Lstm;
use crate::model::{ModelConfig, NamModel};
use crate::wavenet::WaveNet;

/// A runnable NAM model of any supported architecture.
///
/// Build with [`Model::from_nam`]; then call [`Model::process_buffer`] on the audio
/// thread. `#[non_exhaustive]` so future architectures don't break downstream
/// `match`es.
#[non_exhaustive]
#[derive(Debug)]
pub enum Model {
    /// A WaveNet model. Boxed so the enum's variants are similarly sized (a `WaveNet`
    /// carries many pre-allocated scratch buffers); the indirection is one pointer
    /// hop off the build path, not the per-sample hot loop.
    WaveNet(Box<WaveNet>),
    /// An LSTM model.
    Lstm(Lstm),
    /// A width-selectable container of submodels.
    Slimmable(Slimmable),
}

/// A width-selectable set of pre-built submodels (NAM Core `SlimmableContainer`).
///
/// All submodels are built up front, so switching the active one is a single index
/// write — real-time-safe, no allocation, no rebuild. Each submodel keeps its own
/// streaming state, so switching mid-stream leaves a short warmup transient on the
/// newly-selected submodel (NAM Core behaves the same; it does not cross-feed the
/// inactive submodels). The container itself holds no weights and does no DSP.
#[derive(Debug)]
pub struct Slimmable {
    submodels: Vec<Model>,
    max_values: Vec<f32>,
    active: usize,
}

impl Slimmable {
    /// Number of submodels.
    pub fn len(&self) -> usize {
        self.submodels.len()
    }

    /// Always `false` (a built container has at least one submodel).
    pub fn is_empty(&self) -> bool {
        self.submodels.is_empty()
    }

    /// Index of the currently-active submodel.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Select a submodel by index, clamping out-of-range to the last (full) submodel
    /// — mirroring NAM Core's "else last" leniency. Real-time-safe.
    pub fn select(&mut self, index: usize) {
        self.active = index.min(self.submodels.len() - 1);
    }

    /// Set the width dial: activate the first submodel whose `max_value` exceeds
    /// `value`, else the last (full) submodel. Matches NAM Core `SetSlimmableSize`.
    /// Real-time-safe.
    pub fn set_slim_size(&mut self, value: f32) {
        self.active = self
            .max_values
            .iter()
            .position(|&m| m > value)
            .unwrap_or(self.submodels.len() - 1);
    }
}

impl Model {
    /// Build the runtime matching `model.architecture`. All allocation happens here.
    pub fn from_nam(model: &NamModel) -> Result<Self, Error> {
        match &model.config {
            ModelConfig::WaveNet(_) => Ok(Model::WaveNet(Box::new(WaveNet::new(model)?))),
            ModelConfig::Lstm(_) => Ok(Model::Lstm(Lstm::new(model)?)),
            ModelConfig::Slimmable(cfg) => {
                if cfg.submodels.is_empty() {
                    return Err(Error::UnsupportedFeature("empty SlimmableContainer".into()));
                }
                let mut submodels = Vec::with_capacity(cfg.submodels.len());
                let mut max_values = Vec::with_capacity(cfg.submodels.len());
                for sm in &cfg.submodels {
                    submodels.push(Model::from_nam(&sm.model)?);
                    max_values.push(sm.max_value);
                }
                let active = submodels.len() - 1; // default = full
                Ok(Model::Slimmable(Slimmable {
                    submodels,
                    max_values,
                    active,
                }))
            }
        }
    }

    /// Build a model for use as a nested `condition_dsp`, where a WaveNet may emit
    /// more than one output channel (its N rows feed the parent arrays' conditioning).
    /// Only WaveNet has a multi-channel output path; LSTM/Slimmable are always mono,
    /// so they fall back to [`Model::from_nam`].
    pub(crate) fn from_nam_conditioning(model: &NamModel) -> Result<Self, Error> {
        match &model.config {
            ModelConfig::WaveNet(_) => {
                Ok(Model::WaveNet(Box::new(WaveNet::new_conditioning(model)?)))
            }
            _ => Model::from_nam(model),
        }
    }

    /// Process a buffer of mono samples in place. Allocation-free.
    pub fn process_buffer(&mut self, io: &mut [f32]) {
        match self {
            Model::WaveNet(w) => w.process_buffer(io),
            Model::Lstm(l) => l.process_buffer(io),
            Model::Slimmable(s) => s.submodels[s.active].process_buffer(io),
        }
    }

    /// Process a single mono sample. Allocation-free.
    pub fn process_sample(&mut self, x: f32) -> f32 {
        match self {
            Model::WaveNet(w) => w.process_sample(x),
            Model::Lstm(l) => l.process_sample(x),
            Model::Slimmable(s) => s.submodels[s.active].process_sample(x),
        }
    }

    /// Reset all internal state to the model's initial conditions.
    pub fn reset(&mut self) {
        match self {
            Model::WaveNet(w) => w.reset(),
            Model::Lstm(l) => l.reset(),
            // Reset EVERY submodel: `reset` is a full clean slate, and a later
            // `select` must not surface stale state from a previously-active submodel.
            // Iterating a `Vec` allocates nothing, so this stays real-time-safe.
            Model::Slimmable(s) => s.submodels.iter_mut().for_each(Model::reset),
        }
    }

    /// The model's processing latency in samples.
    ///
    /// For WaveNet this is the receptive field: the first this-many output samples
    /// of a fresh (or freshly [`reset`](Self::reset)) model are a startup transient
    /// computed against zero history. A host can report it as plugin latency and/or
    /// discard that many leading samples. LSTM has no warmup, so this is `0`.
    pub fn receptive_field(&self) -> usize {
        match self {
            Model::WaveNet(w) => w.receptive_field(),
            Model::Lstm(_) => 0,
            Model::Slimmable(s) => s.submodels[s.active].receptive_field(),
        }
    }

    /// Number of output channels this model emits, matching NAM Core. WaveNet defers to
    /// its post-stack head / last layer-array; LSTM is always mono. Used when this model
    /// is a nested `condition_dsp` whose rows become the parent's N-wide conditioning.
    pub(crate) fn num_output_channels(&self) -> usize {
        match self {
            Model::WaveNet(w) => w.num_output_channels(),
            Model::Lstm(_) => 1,
            Model::Slimmable(s) => s.submodels[s.active].num_output_channels(),
        }
    }

    /// Run a mono `input[..n]` chunk, writing `num_output_channels() × n` planar
    /// `[ch][t]` into `out`. Allocation-free; used to produce a nested `condition_dsp`'s
    /// multi-channel conditioning for the parent WaveNet.
    pub(crate) fn process_block_multi(&mut self, input: &[f32], out: &mut [f32], n: usize) {
        match self {
            Model::WaveNet(w) => w.process_block_multi(input, out, n),
            Model::Lstm(l) => {
                // LSTM is always mono-out: copy the input into `out` and run in place.
                out[..n].copy_from_slice(&input[..n]);
                l.process_buffer(&mut out[..n]);
            }
            Model::Slimmable(s) => s.submodels[s.active].process_block_multi(input, out, n),
        }
    }

    /// The width-selectable container, if this model is one. Use it to drive the
    /// slim dial ([`Slimmable::select`] / [`Slimmable::set_slim_size`]); plain
    /// WaveNet/LSTM models return `None`.
    pub fn as_slimmable(&self) -> Option<&Slimmable> {
        match self {
            Model::Slimmable(s) => Some(s),
            _ => None,
        }
    }

    /// Mutable variant of [`Model::as_slimmable`], for setting the active submodel.
    pub fn as_slimmable_mut(&mut self) -> Option<&mut Slimmable> {
        match self {
            Model::Slimmable(s) => Some(s),
            _ => None,
        }
    }
}

// Compile-time guarantee that the runtime types stay `Send + Sync`: a real-time
// consumer builds the model off the audio thread and moves it onto the audio thread.
// If a future field drops either auto-trait (e.g. an `Rc` or `Cell` creeps in), this
// fails to compile instead of breaking downstream code.
const _: () = {
    fn assert_send_sync<T: Send + Sync>() {}
    let _ = assert_send_sync::<Model>;
    let _ = assert_send_sync::<WaveNet>;
    let _ = assert_send_sync::<Lstm>;
    let _ = assert_send_sync::<Slimmable>;
};

#[cfg(test)]
mod tests {
    use super::*;

    const TINY_WAVENET: &str = r#"{
        "version": "0.5.4", "architecture": "WaveNet",
        "config": { "layers": [{
            "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
            "kernel_size": 1, "dilations": [1], "activation": "ReLU",
            "gated": false, "head_bias": false
        }], "head": null, "head_scale": 10.0 },
        "weights": [1.0, 2.0, 0.5, 1.0, 3.0, 0.1, 0.5, 10.0]
    }"#;

    const TINY_LSTM: &str = r#"{
        "version": "0.5.4", "architecture": "LSTM",
        "config": { "input_size": 1, "hidden_size": 1, "num_layers": 1 },
        "weights": [1.0,0.0, 0.0,0.0, 2.0,0.0, 0.0,0.0, 0.0,0.0,0.0,0.0, 0.0, 0.0, 3.0, 0.5]
    }"#;

    #[test]
    fn from_nam_builds_wavenet() {
        let m = NamModel::from_json_str(TINY_WAVENET).unwrap();
        let mut model = Model::from_nam(&m).unwrap();
        assert!(matches!(model, Model::WaveNet(_)));
        let mut buf = [0.5_f32];
        model.process_buffer(&mut buf);
        assert!((buf[0] - 10.0).abs() < 1e-5, "got {}", buf[0]);
    }

    #[test]
    fn receptive_field_zero_for_lstm_warmup_for_wavenet() {
        // TINY_WAVENET: kernel 1, dilation 1 -> rf = 1. LSTM has no warmup -> 0.
        let wn = Model::from_nam(&NamModel::from_json_str(TINY_WAVENET).unwrap()).unwrap();
        assert_eq!(wn.receptive_field(), 1);
        let lstm = Model::from_nam(&NamModel::from_json_str(TINY_LSTM).unwrap()).unwrap();
        assert_eq!(lstm.receptive_field(), 0);
    }

    #[test]
    fn from_nam_builds_lstm() {
        let m = NamModel::from_json_str(TINY_LSTM).unwrap();
        let mut model = Model::from_nam(&m).unwrap();
        assert!(matches!(model, Model::Lstm(_)));
        let mut buf = [0.5_f32];
        model.process_buffer(&mut buf);
        assert!((buf[0] - 1.1623).abs() < 1e-3, "got {}", buf[0]);
    }

    fn container() -> Model {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/slimmable_container.nam");
        let json = std::fs::read_to_string(path).expect("read container");
        let m = NamModel::from_json_str(&json).expect("parse container");
        Model::from_nam(&m).expect("build container")
    }

    #[test]
    fn from_nam_builds_slimmable_default_full() {
        let mut model = container();
        let s = model.as_slimmable_mut().expect("is slimmable");
        assert_eq!(s.len(), 3);
        assert_eq!(s.active_index(), 2, "default = last/full submodel");
    }

    #[test]
    fn select_clamps_out_of_range() {
        let mut model = container();
        let s = model.as_slimmable_mut().unwrap();
        s.select(0);
        assert_eq!(s.active_index(), 0);
        s.select(99);
        assert_eq!(s.active_index(), 2, "clamped to last");
    }

    #[test]
    fn set_slim_size_picks_first_threshold_above_value() {
        let mut model = container();
        let s = model.as_slimmable_mut().unwrap();
        // max_values = [0.33, 0.66, 1.0]; first max_value > v, else last.
        s.set_slim_size(0.0);
        assert_eq!(s.active_index(), 0); // 0.33 > 0.0
        s.set_slim_size(0.5);
        assert_eq!(s.active_index(), 1); // 0.33 !> 0.5, 0.66 > 0.5
        s.set_slim_size(0.99);
        assert_eq!(s.active_index(), 2); // only 1.0 > 0.99
        s.set_slim_size(5.0);
        assert_eq!(s.active_index(), 2); // none > 5.0 -> last
    }

    #[test]
    fn reset_clears_all_submodels_not_just_active() {
        // reset() must restore EVERY submodel to initial conditions, not only the
        // active one — Model::reset's contract is a full clean slate. The LSTM
        // submodel (index 0, receptive field 1) is ideal: state shows up immediately.
        let mut model = container();

        // Probe value a fresh (never-processed) submodel-0 produces.
        let mut fresh = container();
        fresh.as_slimmable_mut().unwrap().select(0);
        let mut probe_fresh = vec![0.3_f32; 8];
        fresh.process_buffer(&mut probe_fresh);

        // Dirty submodel 0, switch away, reset, switch back: it must be clean again.
        model.as_slimmable_mut().unwrap().select(0);
        let mut warm = vec![0.5_f32; 16];
        model.process_buffer(&mut warm);
        model.as_slimmable_mut().unwrap().select(2);
        model.reset();
        model.as_slimmable_mut().unwrap().select(0);
        let mut probe = vec![0.3_f32; 8];
        model.process_buffer(&mut probe);

        for (i, (got, want)) in probe.iter().zip(&probe_fresh).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "reset left submodel 0 dirty at sample {i}: {got} vs fresh {want}"
            );
        }
    }

    #[test]
    fn slimmable_processes_through_active_submodel() {
        let mut model = container();
        model.as_slimmable_mut().unwrap().select(0); // LSTM submodel
        let mut a = vec![0.1_f32; 32];
        model.process_buffer(&mut a);
        model.as_slimmable_mut().unwrap().select(2); // full WaveNet submodel
        let mut b = vec![0.1_f32; 32];
        model.process_buffer(&mut b);
    }

    #[test]
    fn as_slimmable_none_for_plain_models() {
        let mut wn = Model::from_nam(&NamModel::from_json_str(TINY_WAVENET).unwrap()).unwrap();
        assert!(wn.as_slimmable_mut().is_none());
    }
}
