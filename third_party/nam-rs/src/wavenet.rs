//! Real-time WaveNet inference.
//!
//! [`WaveNet`] is built once from a parsed [`NamModel`] (which may allocate), then
//! run on the audio thread via [`WaveNet::process_buffer`], which never allocates.
//! All scratch buffers are pre-allocated in [`WaveNet::new`].
//!
//! The forward pass is a port of NAM's WaveNet, built bottom-up from the `conv`,
//! `layer`, `array`, and `head` submodules (each unit-tested) and validated end-to-end
//! against the reference in `tests/parity.rs`.
//!
//! Two top-level features are supported beyond the layer-array stack: an optional
//! **post-stack head** (an `activation → Conv1d` chain run after the arrays, with
//! `head_scale` scaling its input; its output is the model output) and an optional
//! **`condition_dsp`** (a nested standalone [`crate::Model`] whose output replaces the
//! raw mono input as the conditioning fed to every array — the raw input still drives
//! the first array's layer input, matching NAM Core). The `condition_dsp` may emit
//! several output channels, producing an N-wide planar conditioning fed to every array
//! (`condition_size` must equal that channel count); the outer WaveNet stays
//! mono-output.

use crate::error::Error;
use crate::model::{GatingMode, LayerArrayConfig, NamModel, WaveNetConfig};
use crate::reader::Reader;

mod activation;
mod array;
mod conv;
mod film;
mod gating;
mod head;
mod layer;

use activation::Activation;
use array::LayerArray;
use conv::{Conv1d, MAX_BLOCK};
use gating::Gating;
use head::PostStackHead;
use layer::{Layer, LayerDims, LayerWeights};

/// A ready-to-run WaveNet, with all scratch buffers pre-allocated.
#[derive(Debug)]
pub struct WaveNet {
    arrays: Vec<LayerArray>,
    /// Optional post-stack head: an `activation → Conv1d` chain run after the arrays.
    /// When present, `head_scale` scales the head's *input* and the chain's output is
    /// the model output; when absent, output = `head_scale · final_head_output`.
    post_stack_head: Option<Box<PostStackHead>>,
    /// Pre-allocated `[head_in_channels][MAX_BLOCK]` scratch holding the
    /// `head_scale`-scaled final head output fed into `post_stack_head`. Keeps the
    /// hot path allocation-free.
    head_scale_scratch: Vec<f32>,
    /// Optional nested `condition_dsp` model. When present, its output replaces the
    /// raw mono input as the conditioning fed to every array; the raw input still
    /// drives the first array's layer input (NAMCore semantics). Boxed to break the
    /// `Model → WaveNet → Model` type cycle. It is mono-in but may emit several output
    /// channels (`cond_out_ch`), which become the N-wide conditioning fed to the arrays.
    condition_dsp: Option<Box<crate::Model>>,
    /// Conditioning width fed to every array: `condition_dsp.num_output_channels()`
    /// when a `condition_dsp` is present, else `1` (the raw mono input). Every array's
    /// `condition_size` must equal this (validated in [`WaveNet::new`], mirroring
    /// NAMCore's assert).
    cond_out_ch: usize,
    /// Pre-allocated `cond_out_ch × MAX_BLOCK` planar `[ch][t]` scratch holding the
    /// conditioning (`condition_dsp(input)`, or a mirror of the raw input when there's
    /// no `condition_dsp`); reused each chunk so the hot path allocates nothing.
    cond_dsp_out: Vec<f32>,
    head_scale: f32,
    /// Samples of input history the deepest dilated tap reaches back over; equals
    /// the model's warmup length / processing latency in samples.
    receptive_field: usize,
    /// Head-accumulator width of the first array (its incoming head is silence this
    /// wide).
    head_in0: usize,
    /// Head signal carried between arrays (two buffers, ping-ponged).
    head_a: Vec<f32>,
    head_b: Vec<f32>,
    /// Layer signal carried between arrays (two buffers, ping-ponged).
    sig_a: Vec<f32>,
    sig_b: Vec<f32>,
    /// Planar `[width][MAX_BLOCK]` block-path twins of the carry buffers, plus a
    /// scratch copy of the conditioning chunk. Used by [`WaveNet::process_buffer`].
    head_a_blk: Vec<f32>,
    head_b_blk: Vec<f32>,
    sig_a_blk: Vec<f32>,
    sig_b_blk: Vec<f32>,
    cond_blk: Vec<f32>,
}

impl WaveNet {
    /// Build a runnable model from a parsed `.nam` file.
    ///
    /// All allocation happens here. Fails if the architecture is unsupported, an
    /// activation is unknown, or the flat weight blob does not match the config.
    ///
    /// A top-level model is mono-output: this rejects a config whose forward pass
    /// would emit more than one channel (a last-array `head_size > 1` with no
    /// post-stack head), which the mono [`process_buffer`](Self::process_buffer) would
    /// otherwise silently truncate to row 0. (A *multi-channel* WaveNet is only valid
    /// nested as a `condition_dsp`; that path is built via the internal
    /// `new_conditioning`.)
    pub fn new(model: &NamModel) -> Result<Self, Error> {
        Self::build(model, false)
    }

    /// Build a WaveNet for use as a nested `condition_dsp`, where a multi-channel
    /// output is expected (its N rows become the parent arrays' conditioning). Unlike
    /// [`Self::new`], this does not require mono output.
    pub(crate) fn new_conditioning(model: &NamModel) -> Result<Self, Error> {
        Self::build(model, true)
    }

    fn build(model: &NamModel, allow_multi_output: bool) -> Result<Self, Error> {
        let cfg = match &model.config {
            crate::model::ModelConfig::WaveNet(cfg) => cfg,
            crate::model::ModelConfig::Lstm(_) | crate::model::ModelConfig::Slimmable(_) => {
                return Err(Error::UnsupportedArchitecture(model.architecture.clone()))
            }
        };

        check_unsupported_features(cfg)?;

        // A top-level model is mono-output. With no post-stack head, a last-array
        // `head_size > 1` would have the mono `process_buffer` silently emit only row 0;
        // reject it up front. (`new_conditioning` sets `allow_multi_output` for the
        // nested condition_dsp path, where the N rows legitimately become the parent's
        // conditioning.) When a post-stack head IS present its builder enforces
        // `out_channels == 1`, which covers the head's output width for every path.
        if !allow_multi_output && cfg.post_stack_head.is_none() {
            let out_ch = cfg.layers.last().map_or(1, |la| la.head_size);
            if out_ch != 1 {
                return Err(Error::UnsupportedFeature(format!(
                    "top-level WaveNet must be mono-output, but produces {out_ch} channels \
                     (a multi-channel WaveNet is only valid as a nested condition_dsp)"
                )));
            }
        }

        // Build the nested condition_dsp eagerly (off the audio thread). It carries
        // its own weights in its nested `.nam` and consumes nothing from the parent
        // blob, so `expected_weight_count` / the `r.remaining() == 0` assert are
        // unaffected. A failing nested build fails fast here.
        let condition_dsp = match &cfg.condition_dsp {
            Some(nested) => Some(Box::new(crate::Model::from_nam_conditioning(nested)?)),
            None => None,
        };

        // Conditioning width: the condition_dsp's output-channel count (else mono).
        // NAMCore-parity validation (model.cpp ~line 594): every array's
        // `condition_size` must match the condition_dsp's output channels, else the
        // mixin conv would read the wrong number of conditioning rows. This is a
        // build-time check (off the audio thread), mirroring NAMCore's assert.
        let cond_out_ch = condition_dsp
            .as_ref()
            .map_or(1, |m| m.num_output_channels());
        if let Some(cdsp) = &condition_dsp {
            let n_out = cdsp.num_output_channels();
            for (i, la) in cfg.layers.iter().enumerate() {
                if la.condition_size != n_out {
                    return Err(Error::UnsupportedFeature(format!(
                        "condition_size of layer-array {i} ({}) != condition_dsp output \
                         channels ({n_out})",
                        la.condition_size
                    )));
                }
            }
        }

        let expected = expected_weight_count(cfg)?;
        if expected != model.weights.len() {
            return Err(Error::WeightCountMismatch {
                expected,
                found: model.weights.len(),
            });
        }

        let mut r = Reader::new(&model.weights);
        let mut arrays = Vec::with_capacity(cfg.layers.len());
        for la in &cfg.layers {
            arrays.push(build_array(&mut r, la)?);
        }
        // Head-carry invariant: each array's head *output* (its `head_size`-wide head
        // rechannel result) is seeded into the next array's head *accumulator*, whose
        // width is that array's `head_in` (`head1x1.active ? head1x1_out : bottleneck`).
        // So the producer's head width must match the consumer's accumulator width:
        // `arrays[i-1].head_size() == arrays[i].head_in()`. (For A1, head_in == channels
        // == head_size, so this reduces to the old `head_size == channels` chain.) Guard
        // it explicitly so a multi-array model chaining mismatched widths fails loudly
        // here instead of silently reading stale/garbage head rows.
        for i in 1..arrays.len() {
            let produced = arrays[i - 1].head_size();
            let consumed = arrays[i].head_in();
            if produced != consumed {
                return Err(Error::UnsupportedFeature(format!(
                    "layer-array head-carry width mismatch: array {} head_size {produced} \
                     != array {i} head_in {consumed}",
                    i - 1
                )));
            }
        }

        let post_stack_head = match &cfg.post_stack_head {
            Some(hc) => {
                let in_channels = arrays.last().map_or(0, LayerArray::head_size);
                Some(Box::new(build_post_stack_head(&mut r, hc, in_channels)?))
            }
            None => None,
        };

        let head_scale = r.take(1)[0];
        // The up-front check guarantees `expected == weights.len()`; this asserts the
        // other half of the invariant — that building consumed exactly `expected`, so
        // `expected_weight_count` and the `build_array` consumption order agree.
        // A hard assert (not `debug_assert`): if the count formula and the consumption
        // order ever drift, under-consumption would otherwise leave the model silently
        // mis-built in release. (Over-consumption already panics in `Reader::take`.)
        assert_eq!(
            r.remaining(),
            0,
            "build_array consumed fewer weights than expected_weight_count claimed"
        );

        let max_ch = arrays.iter().map(LayerArray::channels).max().unwrap_or(1);
        let max_head = arrays.iter().map(LayerArray::head_size).max().unwrap_or(1);
        let max_head_in = arrays.iter().map(LayerArray::head_in).max().unwrap_or(1);
        // The head carry buffer holds either a producer's `head_size`-wide output or a
        // consumer's `head_in`-wide accumulator seed, so size it to the max of both.
        let head_w = max_ch.max(max_head).max(max_head_in).max(1);
        let sig_w = max_ch.max(1);
        // First array's incoming head is silence of its accumulator width (`head_in`).
        let head_in0 = arrays.first().map_or(0, LayerArray::head_in);

        let head_in_channels = post_stack_head
            .as_ref()
            .map_or(0, |h| h.in_channels())
            .max(1);

        // condition_dsp's prewarm seeds the RF accumulator (else 1). Compute before
        // moving `condition_dsp` into the struct.
        let rf_base = condition_dsp.as_ref().map_or(1, |m| m.receptive_field());

        Ok(Self {
            arrays,
            post_stack_head,
            head_scale_scratch: vec![0.0; head_in_channels * MAX_BLOCK],
            condition_dsp,
            cond_out_ch,
            cond_dsp_out: vec![0.0; cond_out_ch * MAX_BLOCK],
            head_scale,
            receptive_field: receptive_field(cfg, rf_base),
            head_in0,
            head_a: vec![0.0; head_w],
            head_b: vec![0.0; head_w],
            sig_a: vec![0.0; sig_w],
            sig_b: vec![0.0; sig_w],
            head_a_blk: vec![0.0; head_w * MAX_BLOCK],
            head_b_blk: vec![0.0; head_w * MAX_BLOCK],
            sig_a_blk: vec![0.0; sig_w * MAX_BLOCK],
            sig_b_blk: vec![0.0; sig_w * MAX_BLOCK],
            cond_blk: vec![0.0; MAX_BLOCK],
        })
    }

    /// Receptive field in samples: how far back the deepest dilated tap reaches.
    ///
    /// This is the model's warmup length and its processing latency. The first
    /// `receptive_field()` output samples of a fresh (or freshly [`reset`](Self::reset))
    /// model are a startup transient computed against zero-filled history, so they
    /// reflect the streaming zero-init convention (matching NAM Core / NeuralAudio)
    /// rather than a training-time forward pass that pre-pads the whole input.
    pub fn receptive_field(&self) -> usize {
        self.receptive_field
    }

    #[cfg(test)]
    pub(super) fn has_condition_dsp(&self) -> bool {
        self.condition_dsp.is_some()
    }

    /// Process a buffer of mono samples in place.
    ///
    /// Runs the block kernel: each `MAX_BLOCK`-sized chunk is pushed through one
    /// array (and one layer) at a time, keeping each weight matrix hot across the
    /// whole chunk. Bit-for-bit equivalent to looping [`Self::process_sample`], and
    /// it shares the same streaming history, so the two are interchangeable.
    ///
    /// **Real-time contract:** no heap allocation, locks, or syscalls. Enforced by
    /// `tests/rt_safety.rs`.
    pub fn process_buffer(&mut self, io: &mut [f32]) {
        if self.arrays.is_empty() {
            for s in io.iter_mut() {
                *s *= self.head_scale;
            }
            return;
        }
        let mut off = 0;
        while off < io.len() {
            let n = (io.len() - off).min(MAX_BLOCK);
            self.process_chunk(&mut io[off..off + n], n);
            off += n;
        }
    }

    /// Number of output channels this WaveNet emits, matching NAMCore
    /// (`WaveNet::NumOutputChannels`): the post-stack head's `out_channels` when a head
    /// is present, else the last layer-array's `head_size`. The outer model is mono
    /// (`1`); this is consulted when this WaveNet is a nested `condition_dsp` whose
    /// rows become the parent's N-wide conditioning.
    pub(crate) fn num_output_channels(&self) -> usize {
        match &self.post_stack_head {
            Some(h) => h.out_channels(),
            None => self.arrays.last().map_or(1, LayerArray::head_size),
        }
    }

    /// Run one `n <= MAX_BLOCK` chunk through every array via the planar block path,
    /// leaving the last array's head output in `head_a_blk` (`last_head_size × n`,
    /// planar). **Requires** `self.cond_blk[..n]` to already hold the mono layer input.
    /// Shared by the mono `process_chunk` and the multi-channel `process_block_multi`.
    fn run_arrays_block(&mut self, n: usize) {
        // The conditioning fed to every array is `condition_dsp(input)` when present,
        // else the raw input (NAMCore `_process_condition`). Fill it uniformly into
        // `cond_dsp_out` so the array loop always reads the same `cond_ch × n` planar
        // slice — the condition_dsp may emit several rows (`cond_ch`).
        let cond_ch = self.cond_out_ch;
        if let Some(cdsp) = &mut self.condition_dsp {
            cdsp.process_block_multi(
                &self.cond_blk[..n],
                &mut self.cond_dsp_out[..cond_ch * n],
                n,
            );
        } else {
            self.cond_dsp_out[..n].copy_from_slice(&self.cond_blk[..n]); // cond_ch == 1
        }

        // First array: layer input is the raw signal; condition is the `cond_ch × n`
        // conditioning; the incoming head is silence (`head_in`-wide).
        self.head_a_blk[..self.head_in0 * n].fill(0.0);
        {
            let hin = self.arrays[0].head_in();
            let ch = self.arrays[0].channels();
            let hs = self.arrays[0].head_size();
            self.arrays[0].process_block(
                &self.cond_blk[..n],
                &self.cond_dsp_out[..cond_ch * n],
                &self.head_a_blk[..hin * n],
                &mut self.head_b_blk[..hs * n],
                &mut self.sig_b_blk[..ch * n],
                n,
            );
        }
        std::mem::swap(&mut self.head_a_blk, &mut self.head_b_blk);
        std::mem::swap(&mut self.sig_a_blk, &mut self.sig_b_blk);

        for i in 1..self.arrays.len() {
            let in_w = self.arrays[i - 1].channels();
            let hin = self.arrays[i].head_in();
            let ch = self.arrays[i].channels();
            let hs = self.arrays[i].head_size();
            self.arrays[i].process_block(
                &self.sig_a_blk[..in_w * n],
                &self.cond_dsp_out[..cond_ch * n],
                &self.head_a_blk[..hin * n],
                &mut self.head_b_blk[..hs * n],
                &mut self.sig_b_blk[..ch * n],
                n,
            );
            std::mem::swap(&mut self.head_a_blk, &mut self.head_b_blk);
            std::mem::swap(&mut self.sig_a_blk, &mut self.sig_b_blk);
        }
    }

    /// Run one `n <= MAX_BLOCK` chunk through every array via the planar block path.
    /// `chunk` is the mono input and is overwritten with the (mono) output.
    ///
    /// This is the in-place, mono-output specialization of [`Self::process_block_multi`]
    /// (the outer model's final head is one channel); it shares the array stack via
    /// [`Self::run_arrays_block`] and only the final emit differs.
    fn process_chunk(&mut self, chunk: &mut [f32], n: usize) {
        // The raw mono input drives the first array's *layer input* and (when there's
        // no condition_dsp) the conditioning; copy it out before we overwrite `chunk`.
        self.cond_blk[..n].copy_from_slice(chunk);
        self.run_arrays_block(n);

        // After the final swap, head_a_blk holds the last array's head output.
        match &mut self.post_stack_head {
            None => {
                // No post-stack head: output = head_scale · final head (head_size 1,
                // so row 0 is the per-sample head signal).
                for (t, s) in chunk.iter_mut().enumerate() {
                    *s = self.head_scale * self.head_a_blk[t];
                }
            }
            Some(head) => {
                // head_scale scales the head's INPUT, then the chain runs and its
                // output is the model output. The final head output is `in_ch × n`,
                // planar; scale it into pre-allocated scratch, then run the head.
                let in_ch = head.in_channels();
                let scaled = &mut self.head_scale_scratch[..in_ch * n];
                for (s, &h) in scaled.iter_mut().zip(&self.head_a_blk[..in_ch * n]) {
                    *s = self.head_scale * h;
                }
                let out = head.process_block(scaled, n); // [out_channels=1][n]
                chunk.copy_from_slice(&out[..n]);
            }
        }
    }

    /// Run one `n <= MAX_BLOCK` chunk through every array, emitting
    /// `num_output_channels() × n` planar `[ch][t]` into `out` from mono `input[..n]`.
    ///
    /// This is the multi-channel-output twin of [`Self::process_chunk`]: used when this
    /// WaveNet is a nested `condition_dsp` whose rows become the parent's conditioning
    /// (the outer model emits one channel and uses the mono path). Allocation-free.
    pub(crate) fn process_block_multi(&mut self, input: &[f32], out: &mut [f32], n: usize) {
        if self.arrays.is_empty() {
            // No arrays: output = head_scale · input, mono (n_out == 1).
            for (o, &x) in out[..n].iter_mut().zip(&input[..n]) {
                *o = self.head_scale * x;
            }
            return;
        }
        self.cond_blk[..n].copy_from_slice(&input[..n]);
        self.run_arrays_block(n);

        // After the final swap, head_a_blk holds the last array's head output.
        match &mut self.post_stack_head {
            None => {
                // No post-stack head: output = head_scale · final head, all `oc` rows.
                let oc = self.arrays.last().map_or(1, LayerArray::head_size);
                for (o, &h) in out[..oc * n].iter_mut().zip(&self.head_a_blk[..oc * n]) {
                    *o = self.head_scale * h;
                }
            }
            Some(head) => {
                // head_scale scales the head's INPUT; the chain runs and its full
                // `out_channels × n` output is the conditioning rows.
                let in_ch = head.in_channels();
                let scaled = &mut self.head_scale_scratch[..in_ch * n];
                for (s, &h) in scaled.iter_mut().zip(&self.head_a_blk[..in_ch * n]) {
                    *s = self.head_scale * h;
                }
                let oc = head.out_channels();
                let produced = head.process_block(scaled, n); // [out_channels][n]
                out[..oc * n].copy_from_slice(&produced[..oc * n]);
            }
        }
    }

    /// Process a single mono sample, returning one output sample.
    ///
    /// Equivalent to a one-element [`Self::process_buffer`]; convenient for
    /// callers that are not buffer-oriented. Allocation-free.
    pub fn process_sample(&mut self, x: f32) -> f32 {
        // `input` is the raw mono sample (first array's layer input). The conditioning
        // is `condition_dsp(x)` when present, else `x` (NAMCore semantics). Route it
        // through the same `process_block_multi(n=1)` path the block kernel uses, so
        // per-sample ≡ block for the condition_dsp too; the (possibly multi-row,
        // `cond_ch`-wide) result lives in `cond_dsp_out`.
        let input = [x];
        let cond_ch = self.cond_out_ch;
        if let Some(cdsp) = &mut self.condition_dsp {
            cdsp.process_block_multi(&input, &mut self.cond_dsp_out[..cond_ch], 1);
        } else {
            self.cond_dsp_out[0] = x;
        }
        let n = self.arrays.len();
        if n == 0 {
            return self.head_scale * x;
        }

        // First array: layer input is the raw sample, condition is the `cond_ch`-wide
        // conditioning; the incoming head is silence of the array's head-accumulator
        // width (`head_in`).
        self.head_a[..self.head_in0].fill(0.0);
        {
            let hin = self.arrays[0].head_in();
            let ch = self.arrays[0].channels();
            let hs = self.arrays[0].head_size();
            self.arrays[0].process_sample(
                &input,
                &self.cond_dsp_out[..cond_ch],
                &self.head_a[..hin],
                &mut self.head_b[..hs],
                &mut self.sig_b[..ch],
            );
        }
        std::mem::swap(&mut self.head_a, &mut self.head_b);
        std::mem::swap(&mut self.sig_a, &mut self.sig_b);

        for i in 1..n {
            let in_w = self.arrays[i - 1].channels();
            let hin = self.arrays[i].head_in();
            let ch = self.arrays[i].channels();
            let hs = self.arrays[i].head_size();
            self.arrays[i].process_sample(
                &self.sig_a[..in_w],
                &self.cond_dsp_out[..cond_ch],
                &self.head_a[..hin],
                &mut self.head_b[..hs],
                &mut self.sig_b[..ch],
            );
            std::mem::swap(&mut self.head_a, &mut self.head_b);
            std::mem::swap(&mut self.sig_a, &mut self.sig_b);
        }

        // After the final swap, head_a holds the last array's head output.
        match &mut self.post_stack_head {
            None => self.head_scale * self.head_a[0],
            Some(head) => {
                let in_ch = head.in_channels();
                let scaled = &mut self.head_scale_scratch[..in_ch];
                for (s, &h) in scaled.iter_mut().zip(&self.head_a[..in_ch]) {
                    *s = self.head_scale * h;
                }
                head.process_sample(scaled)[0]
            }
        }
    }

    /// Reset all internal state (ring buffers) to silence.
    pub fn reset(&mut self) {
        for a in &mut self.arrays {
            a.reset();
        }
        if let Some(h) = &mut self.post_stack_head {
            h.reset();
        }
        if let Some(c) = &mut self.condition_dsp {
            c.reset();
        }
        self.head_a.fill(0.0);
        self.head_b.fill(0.0);
        self.sig_a.fill(0.0);
        self.sig_b.fill(0.0);
    }
}

/// Reject WaveNet features whose forward pass is not implemented yet, with a clear
/// [`Error::UnsupportedFeature`] (rather than silently mis-running). The per-layer A2
/// features (grouped convs, head1x1, bottleneck≠channels, all 8 FiLM sites, BLENDED
/// gating, non-sigmoid secondaries, inactive layer1x1) are fully supported by
/// [`Layer`], and the two top-level features — the post-stack head and the
/// `condition_dsp` (incl. a multi-channel-output one feeding `condition_size > 1`
/// arrays) — are now supported (their build paths consume/own their weights and run on
/// the hot path). The remaining unsupported cases, all rejected here or at their build
/// site: multi-channel input (`in_channels != 1`); within-array mixed gating; and a
/// post-stack head with `out_channels != 1` (rejected in its builder). The
/// `condition_size == condition_dsp` output-channel match is validated post-build in
/// [`WaveNet::new`] (it needs the built nested model's channel count).
fn check_unsupported_features(cfg: &WaveNetConfig) -> Result<(), Error> {
    if cfg.in_channels != 1 {
        return Err(Error::UnsupportedFeature("in_channels != 1".into()));
    }
    // A WaveNet with no layer-arrays would build, but every hot path treats the
    // empty-array case as a passthrough and ignores a configured post-stack head /
    // condition_dsp — i.e. silently wrong for a structurally-valid config. Reject it
    // so the runtime can rely on at least one array.
    if cfg.layers.is_empty() {
        return Err(Error::UnsupportedFeature(
            "WaveNet with no layer-arrays".into(),
        ));
    }
    for la in &cfg.layers {
        // The array builds one `Gating` from the uniform `gating_mode()`; a layer-array
        // mixing modes across its layers is still unsupported.
        let first = la.gating_mode();
        if la.gating_modes.iter().any(|&g| g != first) {
            return Err(Error::UnsupportedFeature("mixed gating modes".into()));
        }
    }
    Ok(())
}

/// Receptive field implied by `config`: per layer `(kernel_size - 1)·dilation`,
/// plus `(head_kernel_size - 1)` per array for the (possibly multi-tap) head, plus
/// `Σ(kernel - 1)` for the post-stack head. The accumulator starts at `base`, which
/// is `1` for a plain model and the nested `condition_dsp`'s receptive field when one
/// is present (NAMCore: prewarm = `condition_dsp->PrewarmSamples()` instead of 1).
fn receptive_field(cfg: &WaveNetConfig, base: usize) -> usize {
    let mut rf = base;
    for la in &cfg.layers {
        for (k, &d) in la.kernel_sizes.iter().zip(&la.dilations) {
            rf += (k - 1) * d;
        }
        rf += la.head_kernel_size - 1;
    }
    if let Some(head) = &cfg.post_stack_head {
        for &k in &head.kernel_sizes {
            rf += k - 1;
        }
    }
    rf
}

/// Number of weights one layer-array consumes from the flat blob, in exactly
/// [`build_array`]'s `take` order. This is the **single arithmetic source** for the
/// weight layout: [`expected_weight_count`] sums it across arrays for the up-front
/// validation, and [`build_array`] asserts it consumes precisely this many, so the
/// count formula and the consumption order cannot silently drift apart.
fn array_weight_count(la: &LayerArrayConfig) -> Result<usize, Error> {
    let mul = |a: usize, b: usize| a.checked_mul(b).ok_or(Error::ConfigTooLarge);
    let add = |a: usize, b: usize| a.checked_add(b).ok_or(Error::ConfigTooLarge);

    let gated = la.gating_mode() != GatingMode::None;
    let mid = if gated {
        mul(2, la.bottleneck)?
    } else {
        la.bottleneck
    };
    let head1x1_out = la.head1x1.out_channels.unwrap_or(la.channels);
    let cond = la.condition_size;

    // Grouped Conv1d weight count: out*in*kernel/groups (compact). Caller adds bias.
    // The block-diagonal layout needs `out` and `in` each divisible by `groups`
    // (NAMCore's `% groups` precondition); reject a non-dividing config here as a
    // clean `Err` rather than letting `Conv1d::new_grouped` assert-panic at build.
    // (`groups >= 1` is already guaranteed by `normalize`, so no divide-by-zero.)
    let conv_w = |out: usize, in_ch: usize, k: usize, groups: usize| -> Result<usize, Error> {
        if out % groups != 0 || in_ch % groups != 0 {
            return Err(Error::UnsupportedFeature(format!(
                "grouped conv: out ({out}) and in ({in_ch}) must both be divisible by groups ({groups})"
            )));
        }
        Ok(mul(mul(out, in_ch)?, k)? / groups)
    };
    // FiLM: out_rows = (shift?2:1)*input_dim; weights out_rows*cond/groups + out_rows bias.
    let film = |f: &crate::model::FilmConfig, input_dim: usize| -> Result<usize, Error> {
        if !f.active {
            return Ok(0);
        }
        let out_rows = if f.shift {
            mul(2, input_dim)?
        } else {
            input_dim
        };
        add(conv_w(out_rows, cond, 1, f.groups)?, out_rows)
    };

    let mut total = mul(la.channels, la.input_size)?; // rechannel (no bias)

    for &k in &la.kernel_sizes {
        let conv = add(conv_w(mid, la.channels, k, la.groups_input)?, mid)?;
        let mixin = conv_w(mid, cond, 1, la.groups_input_mixin)?; // no bias
        let mut layer = add(conv, mixin)?;
        if la.layer1x1.active {
            let l = add(
                conv_w(la.channels, la.bottleneck, 1, la.layer1x1.groups)?,
                la.channels,
            )?;
            layer = add(layer, l)?;
        }
        if la.head1x1.active {
            let h = add(
                conv_w(head1x1_out, la.bottleneck, 1, la.head1x1.groups)?,
                head1x1_out,
            )?;
            layer = add(layer, h)?;
        }
        // 8 FiLMs in NAMCore order.
        layer = add(layer, film(&la.conv_pre_film, la.channels)?)?;
        layer = add(layer, film(&la.conv_post_film, mid)?)?;
        layer = add(layer, film(&la.input_mixin_pre_film, cond)?)?;
        layer = add(layer, film(&la.input_mixin_post_film, mid)?)?;
        layer = add(layer, film(&la.activation_pre_film, mid)?)?;
        layer = add(layer, film(&la.activation_post_film, la.bottleneck)?)?;
        layer = add(layer, film(&la.layer1x1_post_film, la.channels)?)?;
        layer = add(layer, film(&la.head1x1_post_film, head1x1_out)?)?;
        total = add(total, layer)?;
    }

    // head rechannel reads head_in rows.
    let head_in = if la.head1x1.active {
        head1x1_out
    } else {
        la.bottleneck
    };
    total = add(
        total,
        mul(mul(la.head_size, head_in)?, la.head_kernel_size)?,
    )?;
    if la.head_bias {
        total = add(total, la.head_size)?;
    }
    Ok(total)
}

/// Weights one post-stack head consumes from the flat blob, in NAMCore conv-chain
/// order. Conv `i` is `[out][in][k]` + `[out]` bias; in/out follow the chain
/// `in_channels → channels → … → channels → out_channels`. `in_channels` is the
/// last layer-array's `head_size`.
fn post_stack_head_weight_count(
    head: &crate::model::PostStackHeadConfig,
    in_channels: usize,
) -> Result<usize, Error> {
    let mul = |a: usize, b: usize| a.checked_mul(b).ok_or(Error::ConfigTooLarge);
    let add = |a: usize, b: usize| a.checked_add(b).ok_or(Error::ConfigTooLarge);
    if head.kernel_sizes.is_empty() {
        return Err(Error::UnsupportedFeature(
            "post-stack head with no convs".into(),
        ));
    }
    let n = head.kernel_sizes.len();
    let mut total = 0usize;
    let mut cin = in_channels;
    for (i, &k) in head.kernel_sizes.iter().enumerate() {
        let cout = if i + 1 == n {
            head.out_channels
        } else {
            head.channels
        };
        total = add(total, add(mul(mul(cout, cin)?, k)?, cout)?)?; // weights + bias
        cin = cout;
    }
    Ok(total)
}

/// Number of `f32`s `config` implies in the flat weight blob, including the final
/// `head_scale`.
///
/// Uses checked arithmetic: an absurd or adversarial config whose dimensions overflow
/// `usize` returns [`Error::ConfigTooLarge`] rather than panicking (debug) or wrapping
/// to a wrong, small count (release).
fn expected_weight_count(cfg: &WaveNetConfig) -> Result<usize, Error> {
    let add = |a: usize, b: usize| a.checked_add(b).ok_or(Error::ConfigTooLarge);
    let mut total = 0usize;
    for la in &cfg.layers {
        total = add(total, array_weight_count(la)?)?;
    }
    if let Some(head) = &cfg.post_stack_head {
        let in_ch = cfg.layers.last().map_or(0, |la| la.head_size);
        total = add(total, post_stack_head_weight_count(head, in_ch)?)?;
    }
    add(total, 1) // head_scale
}

/// Build the post-stack head, consuming its convs from the flat blob in NAMCore
/// chain order (matching [`post_stack_head_weight_count`]): conv `i` reads
/// `[out][in][k]` weights then `[out]` bias, with `in/out` following
/// `in_channels → channels → … → channels → out_channels`. Each conv has dilation 1
/// and bias.
fn build_post_stack_head(
    r: &mut Reader,
    hc: &crate::model::PostStackHeadConfig,
    in_channels: usize,
) -> Result<PostStackHead, Error> {
    if hc.out_channels != 1 {
        return Err(Error::UnsupportedFeature(
            "post-stack head out_channels != 1".into(),
        ));
    }
    let n = hc.kernel_sizes.len();
    let activation = Activation::from_spec(&hc.activation)?;
    let mut convs = Vec::with_capacity(n);
    let mut cin = in_channels;
    for (i, &k) in hc.kernel_sizes.iter().enumerate() {
        let cout = if i + 1 == n {
            hc.out_channels
        } else {
            hc.channels
        };
        let w = r.take(cout * cin * k);
        let b = r.take(cout);
        convs.push((activation, Conv1d::new(cin, cout, k, 1, w, Some(b))));
        cin = cout;
    }
    Ok(PostStackHead::new(convs, in_channels, hc.out_channels))
}

fn build_array(r: &mut Reader, la: &LayerArrayConfig) -> Result<LayerArray, Error> {
    let mode = la.gating_mode();
    let gated = mode != GatingMode::None;
    let mid = if gated {
        2 * la.bottleneck
    } else {
        la.bottleneck
    };
    let head1x1_out = la.head1x1.out_channels.unwrap_or(la.channels);
    let cond = la.condition_size;

    // FiLM site descriptors in NAMCore order: (config, input_dim).
    let film_sites = [
        (&la.conv_pre_film, la.channels),
        (&la.conv_post_film, mid),
        (&la.input_mixin_pre_film, cond),
        (&la.input_mixin_post_film, mid),
        (&la.activation_pre_film, mid),
        (&la.activation_post_film, la.bottleneck),
        (&la.layer1x1_post_film, la.channels),
        (&la.head1x1_post_film, head1x1_out),
    ];
    let film_shift: [bool; 8] = std::array::from_fn(|i| film_sites[i].0.shift);
    let film_groups: [usize; 8] = std::array::from_fn(|i| film_sites[i].0.groups);

    let before = r.remaining();
    let rechannel_w = r.take(la.channels * la.input_size);
    let mut layers = Vec::with_capacity(la.dilations.len());
    for (i, &d) in la.dilations.iter().enumerate() {
        let k = la.kernel_sizes[i];
        let primary = Activation::from_spec(&la.activations[i])?;
        let secondary = Activation::from_spec(&la.secondary_activations[i])?;

        // Per-layer weights, consumed in NAMCore `set_weights_` order.
        let conv_w = r.take(mid * la.channels * k / la.groups_input);
        let conv_b = r.take(mid);
        let mix_w = r.take(mid * cond / la.groups_input_mixin);
        let (layer1x1_w, layer1x1_b) = if la.layer1x1.active {
            let w = r.take(la.channels * la.bottleneck / la.layer1x1.groups);
            let b = r.take(la.channels);
            (Some(w), Some(b))
        } else {
            (None, None)
        };
        let (head1x1_w, head1x1_b) = if la.head1x1.active {
            let w = r.take(head1x1_out * la.bottleneck / la.head1x1.groups);
            let b = r.take(head1x1_out);
            (Some(w), Some(b))
        } else {
            (None, None)
        };
        let mut films: [Option<(Vec<f32>, Vec<f32>)>; 8] = Default::default();
        for (j, (f, input_dim)) in film_sites.iter().enumerate() {
            if f.active {
                let out_rows = if f.shift { 2 * input_dim } else { *input_dim };
                let w = r.take(out_rows * cond / f.groups);
                let b = r.take(out_rows);
                films[j] = Some((w, b));
            }
        }

        let gating = Gating::new(mode, primary, secondary, la.bottleneck);
        layers.push(Layer::new(
            LayerDims {
                channels: la.channels,
                bottleneck: la.bottleneck,
                condition_size: cond,
                kernel: k,
                dilation: d,
                groups_input: la.groups_input,
                groups_input_mixin: la.groups_input_mixin,
                layer1x1_groups: la.layer1x1.groups,
                head1x1_groups: la.head1x1.groups,
                head1x1_out: if la.head1x1.active {
                    Some(head1x1_out)
                } else {
                    None
                },
                film_shift,
                film_groups,
            },
            gating,
            LayerWeights {
                conv_w,
                conv_b,
                mix_w,
                layer1x1_w,
                layer1x1_b,
                head1x1_w,
                head1x1_b,
                films,
            },
        ));
    }

    // Head accumulator / head-rechannel input width: uniform across an array's layers.
    let head_in = layers[0].head_contrib_width();
    debug_assert!(
        layers.iter().all(|l| l.head_contrib_width() == head_in),
        "layers in one array must share head-contribution width"
    );

    let head_w = r.take(la.head_size * head_in * la.head_kernel_size);
    let head_b = if la.head_bias {
        Some(r.take(la.head_size))
    } else {
        None
    };

    // Self-check: this array must have consumed exactly what the single count source
    // claims. Fires immediately (and locally) if a future edit changes the `take`
    // order here without updating `array_weight_count`, or vice versa.
    debug_assert_eq!(
        before - r.remaining(),
        array_weight_count(la)?,
        "build_array consumption drifted from array_weight_count"
    );

    Ok(LayerArray::new(
        la.input_size,
        la.channels,
        head_in,
        la.head_size,
        la.head_kernel_size,
        rechannel_w,
        layers,
        head_w,
        head_b,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a normalized LayerArrayConfig for tests via the parser (keeps it honest).
    fn mk_layer(json: serde_json::Value) -> crate::model::LayerArrayConfig {
        let raw: crate::model::RawLayerArrayConfig = serde_json::from_value(json).unwrap();
        raw.normalize().unwrap()
    }

    // 1 array, 1 layer, 1 channel, ReLU. Weight order:
    // rechannel=1, conv_w=2, conv_b=0.5, mix_w=1, one_w=3, one_b=0.1,
    // head_rechannel=0.5, head_scale=10.
    const TINY: &str = r#"{
        "version": "0.5.4",
        "architecture": "WaveNet",
        "config": {
            "layers": [{
                "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                "gated": false, "head_bias": false
            }],
            "head": null, "head_scale": 10.0
        },
        "weights": [1.0, 2.0, 0.5, 1.0, 3.0, 0.1, 0.5, 10.0]
    }"#;

    const TINY_HEAD: &str = r#"{
        "version":"0.6.0","architecture":"WaveNet","config":{
            "layers":[{"input_size":1,"condition_size":1,"channels":1,"head_size":1,
                "kernel_size":1,"dilations":[1],"activation":"ReLU",
                "gated":false,"head_bias":false}],
            "head":{"channels":1,"out_channels":1,"kernel_sizes":[1],"activation":"ReLU"},
            "head_scale":2.0},
        "weights":[]}"#;

    #[test]
    fn receptive_field_includes_condition_dsp_prewarm() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/condition_dsp_mono.nam");
        let json = std::fs::read_to_string(path).expect("condition_dsp_mono.nam");
        let model = NamModel::from_json_str(&json).expect("parse");
        let cfg = match &model.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        // Expected: nested condition_dsp rf + Σ array rf-terms (+ head, none here).
        let nested = crate::Model::from_nam(cfg.condition_dsp.as_ref().unwrap()).unwrap();
        let mut want = nested.receptive_field();
        for la in &cfg.layers {
            for (k, &d) in la.kernel_sizes.iter().zip(&la.dilations) {
                want += (k - 1) * d;
            }
            want += la.head_kernel_size - 1;
        }
        assert_eq!(WaveNet::new(&model).unwrap().receptive_field(), want);
    }

    #[test]
    fn condition_dsp_block_equals_per_sample() {
        // The mono condition_dsp fixture: block path must equal the per-sample path,
        // proving the conditioning replacement is applied consistently across both.
        // Absolute parity is covered by the parity suite.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/condition_dsp_mono.nam");
        let json = std::fs::read_to_string(path).expect("condition_dsp_mono.nam");
        let model = NamModel::from_json_str(&json).expect("parse");

        let len = MAX_BLOCK + 173;
        let signal: Vec<f32> = (0..len).map(|i| (i as f32 * 0.017).sin() * 0.4).collect();

        let mut per_sample = WaveNet::new(&model).unwrap();
        let want: Vec<f32> = signal
            .iter()
            .map(|&x| per_sample.process_sample(x))
            .collect();
        let mut block = WaveNet::new(&model).unwrap();
        let mut got = signal.clone();
        block.process_buffer(&mut got);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(
                (g - w).abs() < 1e-5,
                "sample {i}: block {g}, per-sample {w}"
            );
        }
    }

    #[test]
    fn condition_dsp_model_builds() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/condition_dsp_mono.nam");
        let json = std::fs::read_to_string(path).expect("condition_dsp_mono.nam");
        let model = NamModel::from_json_str(&json).expect("parse");
        let wn = WaveNet::new(&model).expect("condition_dsp model builds");
        assert!(
            wn.has_condition_dsp(),
            "nested condition_dsp must be present"
        );
    }

    #[test]
    fn multi_channel_condition_dsp_builds_and_block_equals_per_sample() {
        // The `wavenet_condition_dsp.nam` example feeds arrays with `condition_size == 3`,
        // fed by a nested WaveNet emitting 3 output channels. It MUST build (no rejection)
        // and the block path must equal the per-sample path, proving the N-wide
        // conditioning is applied consistently across both. Absolute parity vs the oracle
        // is covered by the parity suite.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wavenet_condition_dsp.nam");
        let json = std::fs::read_to_string(path).expect("wavenet_condition_dsp.nam");
        let model = NamModel::from_json_str(&json).expect("parse condition_dsp model");

        let len = MAX_BLOCK + 173;
        let signal: Vec<f32> = (0..len).map(|i| (i as f32 * 0.017).sin() * 0.4).collect();

        let mut per_sample = WaveNet::new(&model).expect("multi-channel condition_dsp builds");
        assert!(per_sample.has_condition_dsp());
        let want: Vec<f32> = signal
            .iter()
            .map(|&x| per_sample.process_sample(x))
            .collect();
        let mut block = WaveNet::new(&model).unwrap();
        let mut got = signal.clone();
        block.process_buffer(&mut got);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(
                (g - w).abs() < 1e-5,
                "sample {i}: block {g}, per-sample {w}"
            );
        }
    }

    #[test]
    fn weight_count_includes_post_stack_head() {
        let model0 = NamModel::from_json_str(TINY_HEAD).unwrap();
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        assert_eq!(expected_weight_count(cfg).unwrap(), 10);
    }

    #[test]
    fn post_stack_head_no_longer_rejected() {
        let model0 = NamModel::from_json_str(TINY_HEAD).unwrap();
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        // Fill exact weight count (computed by expected_weight_count after Task 4).
        let n = expected_weight_count(cfg).unwrap();
        let model = NamModel {
            version: "0.6.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights: vec![0.0; n],
            sample_rate: None,
            metadata: None,
        };
        // The build must NOT fail with the old post-stack-head guard.
        match WaveNet::new(&model) {
            Err(Error::UnsupportedFeature(f)) if f.contains("post-stack head") => {
                panic!("post-stack head should no longer be guarded");
            }
            _ => {} // ok (builds, or fails for another reason until Tasks 3-5 land)
        }
    }

    #[test]
    fn receptive_field_includes_post_stack_head_kernels() {
        // Array: kernel 3, dilations [1,2] -> array rf-term = (3-1)*1+(3-1)*2 = 6.
        // head kernel 1 -> +0. Post-stack head kernels [16, 1] -> +15+0 = 15.
        // total rf = 1 + 6 + 0 + 15 = 22.
        let json = r#"{
            "version":"0.6.0","architecture":"WaveNet","config":{
                "layers":[{"input_size":1,"condition_size":1,"channels":1,"head_size":1,
                    "kernel_size":3,"dilations":[1,2],"activation":"ReLU",
                    "gated":false,"head_bias":false}],
                "head":{"channels":2,"out_channels":1,"kernel_sizes":[16,1],"activation":"ReLU"},
                "head_scale":1.0},
            "weights":[]}"#;
        let model0 = NamModel::from_json_str(json).unwrap();
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n = expected_weight_count(cfg).unwrap();
        let model = NamModel {
            version: "0.6.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights: vec![0.0; n],
            sample_rate: None,
            metadata: None,
        };
        assert_eq!(WaveNet::new(&model).unwrap().receptive_field(), 22);
    }

    #[test]
    fn post_stack_head_forward_matches_hand_computed() {
        // TINY array weights produce final_head=1.0 for x=0.5 (see TINY test).
        // head_scale=2.0 scales head input to 2.0; ReLU head conv w=3,b=0.5 -> 6.5.
        // Weight blob order: [array(7), post_head_conv_w(1), post_head_conv_b(1), head_scale(1)]
        let json = r#"{
            "version":"0.6.0","architecture":"WaveNet","config":{
                "layers":[{"input_size":1,"condition_size":1,"channels":1,"head_size":1,
                    "kernel_size":1,"dilations":[1],"activation":"ReLU",
                    "gated":false,"head_bias":false}],
                "head":{"channels":1,"out_channels":1,"kernel_sizes":[1],"activation":"ReLU"},
                "head_scale":2.0},
            "weights":[1.0, 2.0, 0.5, 1.0, 3.0, 0.1, 0.5, 3.0, 0.5, 2.0]}"#;
        let model = NamModel::from_json_str(json).unwrap();
        let mut wn = WaveNet::new(&model).unwrap();
        let mut buf = [0.5_f32];
        wn.process_buffer(&mut buf);
        assert!((buf[0] - 6.5).abs() < 1e-5, "got {}", buf[0]);

        // And the per-sample path agrees with the block path.
        let mut wn2 = WaveNet::new(&model).unwrap();
        let got = wn2.process_sample(0.5);
        assert!((got - 6.5).abs() < 1e-5, "per-sample got {}", got);
    }

    #[test]
    fn post_stack_head_multichannel_out_rejected() {
        let json = r#"{
            "version":"0.6.0","architecture":"WaveNet","config":{
                "layers":[{"input_size":1,"condition_size":1,"channels":1,"head_size":1,
                    "kernel_size":1,"dilations":[1],"activation":"ReLU",
                    "gated":false,"head_bias":false}],
                "head":{"channels":2,"out_channels":2,"kernel_sizes":[1],"activation":"ReLU"},
                "head_scale":1.0},
            "weights":[]}"#;
        let model0 = NamModel::from_json_str(json).unwrap();
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n = expected_weight_count(cfg).unwrap();
        let model = NamModel {
            version: "0.6.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights: vec![0.0; n],
            sample_rate: None,
            metadata: None,
        };
        assert!(matches!(
            WaveNet::new(&model),
            Err(Error::UnsupportedFeature(f)) if f.contains("out_channels != 1")
        ));
    }

    #[test]
    fn post_stack_head_builds_and_consumes_exact_weights() {
        let model0 = NamModel::from_json_str(TINY_HEAD).unwrap();
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n = expected_weight_count(cfg).unwrap(); // 10
        let weights: Vec<f32> = (0..n).map(|i| (i as f32 + 1.0) * 0.1).collect();
        let model = NamModel {
            version: "0.6.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights,
            sample_rate: None,
            metadata: None,
        };
        assert!(WaveNet::new(&model).is_ok(), "post-stack head model builds");
    }

    #[test]
    fn default_path_unchanged_baseline() {
        // No post-stack head, no condition_dsp: output = head_scale * final_head.
        // TINY: x=0.5 -> out=10.0 (pinned in tiny_model_matches_hand_computed_forward).
        let model = NamModel::from_json_str(TINY).unwrap();
        let mut wn = WaveNet::new(&model).unwrap();
        let mut buf = [0.5_f32];
        wn.process_buffer(&mut buf);
        assert!((buf[0] - 10.0).abs() < 1e-5, "got {}", buf[0]);
        // And the config really has neither feature.
        let cfg = match &model.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        assert!(cfg.post_stack_head.is_none());
        assert!(cfg.condition_dsp.is_none());
    }

    #[test]
    fn tiny_model_matches_hand_computed_forward() {
        let model = NamModel::from_json_str(TINY).unwrap();
        let mut wn = WaveNet::new(&model).unwrap();

        // x=0.5, cond=0.5: z = 2*0.5 + 0.5 + 1*0.5 = 2.0 ; relu=2.0
        // head = 0.5*2.0 = 1.0 ; out = head_scale * 1.0 = 10.0
        let mut buf = [0.5_f32];
        wn.process_buffer(&mut buf);
        assert!((buf[0] - 10.0).abs() < 1e-5, "got {}", buf[0]);
    }

    #[test]
    fn array_weight_count_includes_a2_subblocks() {
        // channels=4, bottleneck=2, condition=1, GATED (mid=2*bn=4), kernel 3 x1 layer,
        // head_size=1 head_kernel=1 no head_bias, groups all 1, head1x1 active out=3,
        // layer1x1 active, conv_post_film (shift) + activation_post_film (no shift) active.
        // Per-layer weights (NAMCore order):
        //   conv:     mid*channels*k + mid          = 4*4*3 + 4   = 52
        //   mixin:    mid*condition                 = 4*1         = 4
        //   layer1x1: channels*bottleneck + channels= 4*2 + 4     = 12
        //   head1x1:  head1x1_out*bottleneck + out   = 3*2 + 3     = 9
        //   conv_post_film: shift -> out_rows=2*mid=8; 8*condition + 8 = 8+8 = 16
        //   activation_post_film: no shift -> out_rows=bottleneck=2; 2*1 + 2 = 4
        // rechannel: channels*input = 4*1 = 4
        // head_rechannel: head_size*head_in*head_k = 1*3*1 = 3 (head_in=head1x1_out=3, no bias)
        // array total = 4 + (52+4+12+9+16+4) + 3 = 4 + 97 + 3 = 104
        let la = mk_layer(serde_json::json!({
            "input_size": 1, "condition_size": 1, "channels": 4, "bottleneck": 2,
            "kernel_sizes": [3], "dilations": [1],
            "activation": [{"type":"Tanh"}],
            "gating_mode": ["gated"],
            "layer1x1": {"active": true, "groups": 1},
            "head1x1": {"active": true, "out_channels": 3, "groups": 1},
            "head": {"out_channels": 1, "kernel_size": 1, "bias": false},
            "conv_post_film": {"active": true, "shift": true, "groups": 1},
            "activation_post_film": {"active": true, "shift": false, "groups": 1}
        }));
        assert_eq!(array_weight_count(&la).unwrap(), 104);
    }

    #[test]
    fn receptive_field_sums_dilated_taps() {
        // rf = 1 + Σ(k-1)·d + Σ(head_kernel_size-1) over all arrays. Here the head
        // kernel is 1 so its term vanishes; mirrors the reference model: kernel 3,
        // dilations [1,2] then [8].
        let cfg = WaveNetConfig {
            layers: vec![
                mk_layer(serde_json::json!({
                    "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                    "kernel_size": 3, "dilations": [1, 2], "activation": "Tanh",
                    "gated": false, "head_bias": false
                })),
                mk_layer(serde_json::json!({
                    "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                    "kernel_size": 3, "dilations": [8], "activation": "Tanh",
                    "gated": false, "head_bias": false
                })),
            ],
            post_stack_head: None,
            head_scale: 1.0,
            in_channels: 1,
            condition_dsp: None,
        };
        // (3-1)*1 + (3-1)*2 + (3-1)*8 = 2 + 4 + 16 = 22, + 1 = 23.
        assert_eq!(receptive_field(&cfg, 1), 23);

        // TINY (kernel 1, dilation 1) reaches back over no past samples: rf = 1.
        let model = NamModel::from_json_str(TINY).unwrap();
        assert_eq!(WaveNet::new(&model).unwrap().receptive_field(), 1);
    }

    #[test]
    fn reset_restores_from_fresh_result() {
        let model = NamModel::from_json_str(TINY).unwrap();
        let mut wn = WaveNet::new(&model).unwrap();
        let mut warm = [0.3_f32, -0.7, 0.2];
        wn.process_buffer(&mut warm);
        wn.reset();
        let mut a = [0.5_f32];
        wn.process_buffer(&mut a);
        assert!((a[0] - 10.0).abs() < 1e-5, "got {}", a[0]);
    }

    #[test]
    fn wrong_weight_count_is_rejected() {
        let bad = TINY.replace(
            "[1.0, 2.0, 0.5, 1.0, 3.0, 0.1, 0.5, 10.0]",
            "[1.0, 2.0, 0.5, 1.0, 3.0, 0.1, 0.5]",
        );
        let model = NamModel::from_json_str(&bad).unwrap();
        match WaveNet::new(&model) {
            Err(Error::WeightCountMismatch { expected, found }) => {
                assert_eq!(expected, 8);
                assert_eq!(found, 7);
            }
            other => panic!("expected WeightCountMismatch, got {other:?}"),
        }
    }

    /// A structurally valid config whose dimensions overflow `usize` must return
    /// `ConfigTooLarge`, not panic (debug) or wrap to a wrong count (release).
    #[test]
    fn absurd_dimensions_error_instead_of_overflowing() {
        let json = TINY.replace("\"channels\": 1", "\"channels\": 4294967296");
        let model = NamModel::from_json_str(&json).unwrap();
        assert!(matches!(WaveNet::new(&model), Err(Error::ConfigTooLarge)));
    }

    /// Pins the weight-count invariant `take` relies on: `expected_weight_count` must
    /// equal exactly what `build_array` (+ head_scale) consumes, across config shapes.
    /// Building with that many weights succeeds (and the `debug_assert` in `new` fires
    /// if consumption drifts below it); one fewer / one more is a count mismatch.
    #[test]
    fn weight_count_matches_consumption_across_shapes() {
        let layer_sets: Vec<Vec<crate::model::LayerArrayConfig>> = vec![
            vec![mk_layer(serde_json::json!({
                "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                "kernel_size": 1, "dilations": [1], "activation": "Tanh",
                "gated": false, "head_bias": false
            }))],
            vec![mk_layer(serde_json::json!({
                "input_size": 1, "condition_size": 1, "channels": 2, "head_size": 1,
                "kernel_size": 3, "dilations": [1, 2], "activation": "Tanh",
                "gated": false, "head_bias": false
            }))],
            vec![mk_layer(serde_json::json!({
                "input_size": 1, "condition_size": 1, "channels": 4, "head_size": 2,
                "kernel_size": 3, "dilations": [1, 2, 4], "activation": "Tanh",
                "gated": true, "head_bias": false
            }))], // gated
            vec![mk_layer(serde_json::json!({
                "input_size": 1, "condition_size": 1, "channels": 3, "head_size": 1,
                "kernel_size": 3, "dilations": [1], "activation": "Tanh",
                "gated": false, "head_bias": true
            }))], // head_bias
            // two arrays: the second takes the first's channels as its input_size,
            // and the first's head_size must equal the second's channels (the
            // head-carry invariant guarded in `WaveNet::new`).
            vec![
                mk_layer(serde_json::json!({
                    "input_size": 1, "condition_size": 1, "channels": 4, "head_size": 2,
                    "kernel_size": 3, "dilations": [1, 2], "activation": "Tanh",
                    "gated": false, "head_bias": false
                })),
                mk_layer(serde_json::json!({
                    "input_size": 4, "condition_size": 1, "channels": 2, "head_size": 1,
                    "kernel_size": 3, "dilations": [1], "activation": "Tanh",
                    "gated": true, "head_bias": true
                })),
            ],
        ];
        for layers in layer_sets {
            let cfg = WaveNetConfig {
                layers,
                post_stack_head: None,
                head_scale: 1.0,
                in_channels: 1,
                condition_dsp: None,
            };
            let n = expected_weight_count(&cfg).unwrap();
            let mk_model = |count: usize| NamModel {
                version: "0".into(),
                architecture: "WaveNet".into(),
                config: crate::model::ModelConfig::WaveNet(cfg.clone()),
                weights: vec![0.0; count],
                sample_rate: None,
                metadata: None,
            };
            // Build via `new_conditioning`: this pins the weight-count *consumption*
            // invariant across shapes, including multi-output ones (head_size > 1 with
            // no post-stack head), which the mono `new` rightly rejects. Both share the
            // same build/weight-accounting path, so the count check is identical.
            assert!(
                WaveNet::new_conditioning(&mk_model(n)).is_ok(),
                "exact count n={n}"
            );
            assert!(matches!(
                WaveNet::new_conditioning(&mk_model(n - 1)),
                Err(Error::WeightCountMismatch { .. })
            ));
            assert!(matches!(
                WaveNet::new_conditioning(&mk_model(n + 1)),
                Err(Error::WeightCountMismatch { .. })
            ));
        }
    }

    /// Build a `NamModel` wrapping a `WaveNetConfig` with the given layers (no
    /// post-stack head / condition_dsp), padded with as many zero weights as it claims
    /// to need so the build reaches the validation under test rather than failing on a
    /// weight-count mismatch first.
    fn wavenet_model(layers: Vec<crate::model::LayerArrayConfig>) -> NamModel {
        let cfg = WaveNetConfig {
            layers,
            post_stack_head: None,
            head_scale: 1.0,
            in_channels: 1,
            condition_dsp: None,
        };
        let count = expected_weight_count(&cfg).unwrap_or(1);
        NamModel {
            version: "0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg),
            weights: vec![0.0; count],
            sample_rate: None,
            metadata: None,
        }
    }

    #[test]
    fn empty_layers_is_rejected() {
        // A WaveNet with no arrays would build but every hot path is a passthrough
        // that ignores a post-stack head / condition_dsp — reject it at build time.
        let model = wavenet_model(vec![]);
        assert!(matches!(
            WaveNet::new(&model),
            Err(Error::UnsupportedFeature(f)) if f.contains("no layer-arrays")
        ));
    }

    #[test]
    fn top_level_multi_output_is_rejected_but_allowed_when_nested() {
        // A single array with head_size 2 and no post-stack head emits 2 channels;
        // the mono `process_buffer` would silently keep only row 0. The top-level
        // `new` must reject it, while `new_conditioning` (the nested cdsp path) accepts
        // it — there the 2 rows become the parent's conditioning.
        let layers = vec![mk_layer(serde_json::json!({
            "input_size": 1, "condition_size": 1, "channels": 2, "head_size": 2,
            "kernel_size": 1, "dilations": [1], "activation": "Tanh",
            "gated": false, "head_bias": false
        }))];
        let model = wavenet_model(layers);
        assert!(matches!(
            WaveNet::new(&model),
            Err(Error::UnsupportedFeature(f)) if f.contains("mono-output")
        ));
        assert!(
            WaveNet::new_conditioning(&model).is_ok(),
            "multi-output is valid for a nested condition_dsp"
        );
    }

    #[test]
    fn non_divisible_groups_is_rejected_cleanly_not_panicking() {
        // groups_input = 2 with channels = 3 (3 % 2 != 0): the block-diagonal layout
        // can't be formed. This must surface as a clean `Err`, not a `Conv1d` panic.
        let layers = vec![mk_layer(serde_json::json!({
            "input_size": 1, "condition_size": 1, "channels": 3, "head_size": 1,
            "kernel_size": 1, "dilations": [1], "activation": "Tanh",
            "gated": false, "head_bias": false, "groups_input": 2
        }))];
        let model = wavenet_model(layers);
        assert!(matches!(
            WaveNet::new(&model),
            Err(Error::UnsupportedFeature(f)) if f.contains("divisible by groups")
        ));
    }

    #[test]
    fn wavenet_new_rejects_non_wavenet() {
        let lstm = r#"{
            "version": "0.5.4", "architecture": "LSTM",
            "config": { "input_size": 1, "hidden_size": 4, "num_layers": 1 },
            "weights": [0.0]
        }"#;
        let model = NamModel::from_json_str(lstm).unwrap();
        assert!(matches!(
            WaveNet::new(&model),
            Err(Error::UnsupportedArchitecture(_))
        ));
    }

    /// End-to-end on the realistic standard model: the block `process_buffer` must
    /// equal a per-sample `process_sample` loop over the same signal, including
    /// across `MAX_BLOCK` chunk boundaries. `tests/parity.rs` drives `process_buffer`
    /// (the block path) and pins it to the reference NAM oracle within 1e-5; this
    /// test additionally ties the block path to the per-sample path, so the two are
    /// transitively guaranteed equivalent and both oracle-correct.
    #[test]
    fn process_buffer_equals_process_sample_loop_on_standard_model() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/reference_standard.nam");
        let json = std::fs::read_to_string(path).expect("read standard fixture");
        let model = NamModel::from_json_str(&json).expect("parse standard fixture");

        // A signal longer than MAX_BLOCK so chunking is exercised.
        let len = 2 * MAX_BLOCK + 137;
        let signal: Vec<f32> = (0..len)
            .map(|i| (i as f32 * 0.013).sin() * 0.5 + (i as f32 * 0.27).sin() * 0.2)
            .collect();

        let mut per_sample = WaveNet::new(&model).unwrap();
        let want: Vec<f32> = signal
            .iter()
            .map(|&x| per_sample.process_sample(x))
            .collect();

        let mut block = WaveNet::new(&model).unwrap();
        let mut got = signal.clone();
        block.process_buffer(&mut got);

        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(
                (g - w).abs() < 1e-5,
                "sample {i}: block {g}, per-sample {w}"
            );
        }
    }

    #[test]
    fn a2_leaky_relu_and_conv_head_parse_and_build() {
        // LeakyReLU + multi-tap conv head (kernel_size=16) are now fully supported.
        // Verify the config parses and builds (weight count determines valid input).
        let json = r#"{
            "version":"0.7.0","architecture":"WaveNet","config":{
                "layers":[{"input_size":1,"condition_size":1,"channels":2,"bottleneck":2,
                    "dilations":[1,2],"kernel_sizes":[3,3],
                    "activation":[{"type":"LeakyReLU"},{"type":"LeakyReLU"}],
                    "head":{"out_channels":1,"kernel_size":16,"bias":true},
                    "layer1x1":{"active":true,"groups":1},
                    "gating_mode":["none","none"]}],
                "head":null,"head_scale":0.5},
            "weights":[]}"#;
        let model0 = NamModel::from_json_str(json).expect("parses cleanly now");
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n = expected_weight_count(cfg).unwrap();
        let model = NamModel {
            version: "0.7.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights: vec![0.0; n],
            sample_rate: None,
            metadata: None,
        };
        assert!(
            WaveNet::new(&model).is_ok(),
            "LeakyReLU + multi-tap conv head should now build"
        );
    }

    #[test]
    fn a1_still_builds_and_runs_after_typed_config() {
        let model = NamModel::from_json_str(TINY).unwrap();
        let mut wn = WaveNet::new(&model).unwrap();
        let mut buf = [0.5_f32];
        wn.process_buffer(&mut buf);
        assert!((buf[0] - 10.0).abs() < 1e-5, "got {}", buf[0]);
    }

    /// Non-power-of-2 and out-of-order dilations with kernel 6 must size buffers
    /// correctly: receptive field is order-independent, and the block path equals the
    /// per-sample path. Mirrors A2's `[1,5,29,97,227]`-style dilations.
    #[test]
    fn non_pow2_out_of_order_dilations_size_correctly() {
        let json = r#"{
            "version":"0.7.0","architecture":"WaveNet","config":{"layers":[{
                "input_size":1,"condition_size":1,"channels":2,"head_size":1,
                "kernel_size":6,"dilations":[97,1,227,5,29],"activation":"ReLU",
                "gated":false,"head_bias":false}],"head":null,"head_scale":0.5},
            "weights":[]}"#;
        // Parse the config, then fill weights to the exact expected count so build succeeds.
        let model0 = NamModel::from_json_str(json).unwrap();
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n = expected_weight_count(cfg).unwrap();
        let weights: Vec<f32> = (0..n).map(|i| ((i % 7) as f32 - 3.0) * 0.05).collect();
        let model = NamModel {
            version: "0.7.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights,
            sample_rate: None,
            metadata: None,
        };

        // rf = 1 + (k-1)*sum(dilations), order-independent.
        let want_rf = 1 + (6 - 1) * (97 + 1 + 227 + 5 + 29);
        let mut per_sample = WaveNet::new(&model).unwrap();
        assert_eq!(per_sample.receptive_field(), want_rf);

        // block path == per-sample path over a signal longer than MAX_BLOCK.
        let len = MAX_BLOCK + 200;
        let signal: Vec<f32> = (0..len).map(|i| (i as f32 * 0.017).sin() * 0.4).collect();
        let want: Vec<f32> = signal
            .iter()
            .map(|&x| per_sample.process_sample(x))
            .collect();
        let mut block = WaveNet::new(&model).unwrap();
        let mut got = signal.clone();
        block.process_buffer(&mut got);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(
                (g - w).abs() < 1e-5,
                "sample {i}: block {g}, per-sample {w}"
            );
        }
    }

    #[test]
    fn multitap_head_config_now_builds() {
        // Weight count: rechannel 2 + 2 layers*(conv 2*2*3+2=14, mix 2, one 2*2+2=6 =>22)
        // + head(1*2*4=8 + bias 1 =9) + head_scale 1 = 56.
        let json = r#"{
            "version":"0.7.0","architecture":"WaveNet","config":{
                "layers":[{"input_size":1,"condition_size":1,"channels":2,"bottleneck":2,
                    "dilations":[1,2],"kernel_sizes":[3,3],
                    "activation":[{"type":"ReLU"},{"type":"ReLU"}],
                    "head":{"out_channels":1,"kernel_size":4,"bias":true},
                    "layer1x1":{"active":true,"groups":1},
                    "gating_mode":["none","none"]}],
                "head":null,"head_scale":0.5},
            "weights":[]}"#;
        let model0 = NamModel::from_json_str(json).unwrap();
        let cfg = match &model0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n = expected_weight_count(cfg).unwrap();
        assert_eq!(n, 56, "expected 56 weights for this conv-head config");
        let weights: Vec<f32> = (0..n).map(|i| ((i % 5) as f32 - 2.0) * 0.1).collect();
        let model = NamModel {
            version: "0.7.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights,
            sample_rate: None,
            metadata: None,
        };
        let mut wn = WaveNet::new(&model).expect("conv-head model builds");
        let signal: Vec<f32> = (0..256).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let want: Vec<f32> = {
            let mut w = WaveNet::new(&model).unwrap();
            signal.iter().map(|&x| w.process_sample(x)).collect()
        };
        let mut got = signal.clone();
        wn.process_buffer(&mut got);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!(
                (g - w).abs() < 1e-5,
                "sample {i}: block {g} vs per-sample {w}"
            );
        }
    }

    #[test]
    fn formerly_guarded_a2_features_now_build() {
        // grouped input conv + head1x1 + bottleneck<channels + FiLM + BLENDED + non-sigmoid
        // secondary, in one array. Weight count drives the (zeroed) blob length.
        let json = r#"{
            "version":"0.7.0","architecture":"WaveNet","config":{"layers":[{
                "input_size":1,"condition_size":1,"channels":4,"bottleneck":2,
                "dilations":[1,2],"kernel_sizes":[3,3],
                "activation":[{"type":"Tanh"},{"type":"Tanh"}],
                "gating_mode":["blended","blended"],
                "secondary_activation":[{"type":"Tanh"},{"type":"Tanh"}],
                "groups_input":2,"groups_input_mixin":1,
                "layer1x1":{"active":true,"groups":2},
                "head1x1":{"active":true,"out_channels":3,"groups":1},
                "head":{"out_channels":1,"kernel_size":1,"bias":false},
                "conv_post_film":{"active":true,"shift":true,"groups":1},
                "activation_post_film":{"active":true,"shift":false,"groups":1},
                "layer1x1_post_film":{"active":true,"shift":false,"groups":1}
            }],"head":null,"head_scale":0.5},"weights":[]}"#;
        let m0 = NamModel::from_json_str(json).unwrap();
        let cfg = match &m0.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n = expected_weight_count(cfg).unwrap();
        let weights: Vec<f32> = (0..n).map(|i| ((i % 7) as f32 - 3.0) * 0.02).collect();
        let model = NamModel {
            version: "0.7.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(cfg.clone()),
            weights,
            sample_rate: None,
            metadata: None,
        };
        assert!(
            WaveNet::new(&model).is_ok(),
            "full A2 feature layer must build now"
        );

        // Inactive-layer1x1 (bottleneck==channels) also builds.
        let json2 = r#"{"version":"0.7.0","architecture":"WaveNet","config":{"layers":[{
            "input_size":1,"condition_size":1,"channels":2,"bottleneck":2,
            "dilations":[1],"kernel_sizes":[3],"activation":[{"type":"ReLU"}],
            "gating_mode":["none"],"layer1x1":{"active":false,"groups":1},
            "head":{"out_channels":1,"kernel_size":1,"bias":false}}],
            "head":null,"head_scale":0.5},"weights":[]}"#;
        let m2 = NamModel::from_json_str(json2).unwrap();
        let c2 = match &m2.config {
            crate::model::ModelConfig::WaveNet(c) => c,
            _ => unreachable!(),
        };
        let n2 = expected_weight_count(c2).unwrap();
        let model2 = NamModel {
            version: "0.7.0".into(),
            architecture: "WaveNet".into(),
            config: crate::model::ModelConfig::WaveNet(c2.clone()),
            weights: vec![0.0; n2],
            sample_rate: None,
            metadata: None,
        };
        assert!(
            WaveNet::new(&model2).is_ok(),
            "inactive layer1x1 must build now"
        );
    }
}
