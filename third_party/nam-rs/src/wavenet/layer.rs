//! A single WaveNet layer: a faithful port of NeuralAmpModelerCore v0.5.3
//! `detail::Layer::Process` (cross-checked against NAM's Python `_Layer`).
//!
//! The pipeline, in NAMCore's exact order, for input `x` and conditioning `c`:
//!
//! ```text
//! conv_in = conv_pre_film?(x, c) ?? x
//! conv_out = conv(conv_in) ; conv_post_film_?(conv_out, c)           // mid rows
//! mix_in  = input_mixin_pre_film?(c, c) ?? c
//! mix     = mixin(mix_in) ; input_mixin_post_film_?(mix, c)          // mid rows
//! z       = conv_out + mix ; activation_pre_film_?(z, c)             // mid rows
//! act     = gating(z) ; activation_post_film(act, c)                 // bottleneck rows
//! head_contrib = head1x1?(act) (+ head1x1_post_film_) ?? act
//! head_accum += head_contrib
//! res     = layer1x1?(act) (+ layer1x1_post_film_, BLENDED only) ; out = x + res
//!           // or, if layer1x1 inactive: out = x  (requires bottleneck == channels)
//! ```
//!
//! `mid = gating.input_rows()` (= `bottleneck` for NONE, `2·bottleneck` for
//! GATED/BLENDED). All scratch is pre-allocated in [`Layer::new`]; the `process_*`
//! methods never allocate.

use super::conv::{Conv1d, MAX_BLOCK};
use super::film::FiLM;
use super::gating::Gating;
use crate::model::GatingMode;

/// Per-layer dimensions and grouping, resolved from the typed config.
pub(super) struct LayerDims {
    pub channels: usize,
    pub bottleneck: usize,
    pub condition_size: usize,
    pub kernel: usize,
    pub dilation: usize,
    pub groups_input: usize,
    pub groups_input_mixin: usize,
    pub layer1x1_groups: usize,
    pub head1x1_groups: usize,
    /// `Some(out)` ⇒ head1x1 active with `out` output channels; `None` ⇒ inactive.
    pub head1x1_out: Option<usize>,
    pub film_shift: [bool; 8],
    pub film_groups: [usize; 8],
}

/// Raw weight tensors for one layer, consumed in NAMCore order.
pub(super) struct LayerWeights {
    pub conv_w: Vec<f32>,
    pub conv_b: Vec<f32>,
    pub mix_w: Vec<f32>,
    pub layer1x1_w: Option<Vec<f32>>,
    pub layer1x1_b: Option<Vec<f32>>,
    pub head1x1_w: Option<Vec<f32>>,
    pub head1x1_b: Option<Vec<f32>>,
    /// FiLM weights `(w, b)` per site, in NAMCore order; `None` when inactive.
    pub films: [Option<(Vec<f32>, Vec<f32>)>; 8],
}

/// One dilated WaveNet layer with all scratch buffers pre-allocated.
#[derive(Debug, Clone)]
pub(super) struct Layer {
    conv: Conv1d,
    mixin: Conv1d,
    layer1x1: Option<Conv1d>,
    head1x1: Option<Conv1d>,
    /// 8 FiLM sites, in NAMCore order: conv_pre, conv_post, input_mixin_pre,
    /// input_mixin_post, activation_pre, activation_post, layer1x1_post, head1x1_post.
    films: [Option<FiLM>; 8],
    gating: Gating,
    mode: GatingMode,
    channels: usize,
    bottleneck: usize,
    head_contrib_width: usize,

    // Per-sample scratch.
    conv_pre: Vec<f32>,     // channels
    conv_out: Vec<f32>,     // mid
    mix: Vec<f32>,          // mid
    mix_pre: Vec<f32>,      // condition_size
    z: Vec<f32>,            // mid
    act: Vec<f32>,          // bottleneck
    act_film: Vec<f32>,     // bottleneck (out-of-place activation_post_film scratch)
    head_contrib: Vec<f32>, // head_contrib_width
    res: Vec<f32>,          // channels

    // Planar block-path twins.
    conv_pre_blk: Vec<f32>,
    conv_out_blk: Vec<f32>,
    mix_blk: Vec<f32>,
    mix_pre_blk: Vec<f32>,
    z_blk: Vec<f32>,
    act_blk: Vec<f32>,
    act_film_blk: Vec<f32>,
    head_contrib_blk: Vec<f32>,
    res_blk: Vec<f32>,
}

impl Layer {
    /// Build a layer from typed dims, a [`Gating`] value (which owns the primary
    /// activation), and the raw weight tensors (split per sub-block in NAMCore order).
    pub(super) fn new(dims: LayerDims, gating: Gating, w: LayerWeights) -> Self {
        // `mid` is the conv/mixin output width; derive it from the Gating contract so
        // it cannot drift from the gating module.
        let mid = gating.input_rows();
        let mode = gating.mode();

        let conv = Conv1d::new_grouped(
            dims.channels,
            mid,
            dims.kernel,
            dims.dilation,
            dims.groups_input,
            w.conv_w,
            Some(w.conv_b),
        );
        let mixin = Conv1d::new_grouped(
            dims.condition_size,
            mid,
            1,
            1,
            dims.groups_input_mixin,
            w.mix_w,
            None,
        );

        let layer1x1 = match (w.layer1x1_w, w.layer1x1_b) {
            (Some(lw), lb) => Some(Conv1d::new_grouped(
                dims.bottleneck,
                dims.channels,
                1,
                1,
                dims.layer1x1_groups,
                lw,
                lb,
            )),
            _ => None,
        };

        let head1x1_out = dims.head1x1_out.unwrap_or(dims.channels);
        let head1x1 = match (dims.head1x1_out, w.head1x1_w, w.head1x1_b) {
            (Some(out), Some(hw), hb) => Some(Conv1d::new_grouped(
                dims.bottleneck,
                out,
                1,
                1,
                dims.head1x1_groups,
                hw,
                hb,
            )),
            _ => None,
        };

        let head_contrib_width = if head1x1.is_some() {
            head1x1_out
        } else {
            dims.bottleneck
        };

        // FiLM input_dim per site (the dim table).
        let film_input_dim = [
            dims.channels,       // conv_pre
            mid,                 // conv_post
            dims.condition_size, // input_mixin_pre
            mid,                 // input_mixin_post
            mid,                 // activation_pre
            dims.bottleneck,     // activation_post
            dims.channels,       // layer1x1_post
            head1x1_out,         // head1x1_post
        ];
        let mut films: [Option<FiLM>; 8] = Default::default();
        for (i, slot) in w.films.into_iter().enumerate() {
            if let Some((fw, fb)) = slot {
                films[i] = Some(FiLM::new(
                    dims.condition_size,
                    film_input_dim[i],
                    dims.film_shift[i],
                    dims.film_groups[i],
                    fw,
                    fb,
                ));
            }
        }

        Self {
            conv,
            mixin,
            layer1x1,
            head1x1,
            films,
            gating,
            mode,
            channels: dims.channels,
            bottleneck: dims.bottleneck,
            head_contrib_width,
            conv_pre: vec![0.0; dims.channels],
            conv_out: vec![0.0; mid],
            mix: vec![0.0; mid],
            mix_pre: vec![0.0; dims.condition_size],
            z: vec![0.0; mid],
            act: vec![0.0; dims.bottleneck],
            act_film: vec![0.0; dims.bottleneck],
            head_contrib: vec![0.0; head_contrib_width],
            res: vec![0.0; dims.channels],
            conv_pre_blk: vec![0.0; dims.channels * MAX_BLOCK],
            conv_out_blk: vec![0.0; mid * MAX_BLOCK],
            mix_blk: vec![0.0; mid * MAX_BLOCK],
            mix_pre_blk: vec![0.0; dims.condition_size * MAX_BLOCK],
            z_blk: vec![0.0; mid * MAX_BLOCK],
            act_blk: vec![0.0; dims.bottleneck * MAX_BLOCK],
            act_film_blk: vec![0.0; dims.bottleneck * MAX_BLOCK],
            head_contrib_blk: vec![0.0; head_contrib_width * MAX_BLOCK],
            res_blk: vec![0.0; dims.channels * MAX_BLOCK],
        }
    }

    /// Width of this layer's head contribution: `head1x1_out` when head1x1 is
    /// active, else `bottleneck`.
    pub(super) fn head_contrib_width(&self) -> usize {
        self.head_contrib_width
    }

    /// Process one sample.
    ///
    /// - `input`: this layer's input, `channels` wide.
    /// - `condition`: the conditioning signal, `condition_size` wide.
    /// - `head_accum`: `head_contrib_width`-wide head accumulator; the head
    ///   contribution is *added* to it.
    /// - `out`: `channels`-wide residual output for the next layer.
    pub(super) fn process_sample(
        &mut self,
        input: &[f32],
        condition: &[f32],
        head_accum: &mut [f32],
        out: &mut [f32],
    ) {
        // 1. conv (with optional pre/post FiLM).
        let conv_in: &[f32] = if let Some(f) = &mut self.films[0] {
            f.process_sample(input, condition, &mut self.conv_pre);
            &self.conv_pre
        } else {
            input
        };
        self.conv.process_sample(conv_in, &mut self.conv_out);
        if let Some(f) = &mut self.films[1] {
            f.process_sample_(&mut self.conv_out, condition);
        }

        // 2. mixin (with optional pre/post FiLM).
        let mix_in: &[f32] = if let Some(f) = &mut self.films[2] {
            f.process_sample(condition, condition, &mut self.mix_pre);
            &self.mix_pre
        } else {
            condition
        };
        self.mixin.process_sample(mix_in, &mut self.mix);
        if let Some(f) = &mut self.films[3] {
            f.process_sample_(&mut self.mix, condition);
        }

        // 3. z = conv_out + mix (with optional activation_pre FiLM).
        for (zi, (c, m)) in self.z.iter_mut().zip(self.conv_out.iter().zip(&self.mix)) {
            *zi = *c + *m;
        }
        if let Some(f) = &mut self.films[4] {
            f.process_sample_(&mut self.z, condition);
        }

        // 4. activation / gating into `act` (bottleneck rows), then activation_post FiLM.
        self.gating.process_sample(&self.z, &mut self.act);
        if let Some(f) = &mut self.films[5] {
            if self.mode == GatingMode::None {
                f.process_sample_(&mut self.act, condition);
            } else {
                // GATED/BLENDED: NAMCore applies this out-of-place then copies back.
                f.process_sample(&self.act, condition, &mut self.act_film);
                self.act.copy_from_slice(&self.act_film);
            }
        }

        // 5. head contribution.
        if let Some(h) = &mut self.head1x1 {
            h.process_sample(&self.act, &mut self.head_contrib);
            if let Some(f) = &mut self.films[7] {
                f.process_sample_(&mut self.head_contrib, condition);
            }
        } else {
            self.head_contrib.copy_from_slice(&self.act);
        }
        for (a, c) in head_accum.iter_mut().zip(&self.head_contrib) {
            *a += *c;
        }

        // 6. residual.
        if let Some(l) = &mut self.layer1x1 {
            l.process_sample(&self.act, &mut self.res);
            if self.mode == GatingMode::Blended {
                if let Some(f) = &mut self.films[6] {
                    f.process_sample_(&mut self.res, condition);
                }
            }
            for (o, (r, x)) in out.iter_mut().zip(self.res.iter().zip(input)) {
                *o = *r + *x;
            }
        } else {
            out.copy_from_slice(input);
        }
    }

    /// Block twin of [`Self::process_sample`]. All slices are **planar** `[row][t]`,
    /// `n <= MAX_BLOCK`: `input`/`out` are `channels * n`, `condition` is
    /// `condition_size * n`, `head_accum` is `head_contrib_width * n`. Equivalent to
    /// `n` per-sample calls; allocation-free.
    pub(super) fn process_block(
        &mut self,
        input: &[f32],
        condition: &[f32],
        head_accum: &mut [f32],
        out: &mut [f32],
        n: usize,
    ) {
        let mid = self.conv_out.len();
        let ch = self.channels;
        let bn = self.bottleneck;
        let hcw = self.head_contrib_width;
        // Conditioning width: `condition_size * n` (mix_pre is one `condition_size` row).
        let cond_w = self.mix_pre.len() * n;

        // 1. conv.
        let conv_in: &[f32] = if let Some(f) = &mut self.films[0] {
            let buf = &mut self.conv_pre_blk[..ch * n];
            f.process_block(input, condition, buf, n);
            &self.conv_pre_blk[..ch * n]
        } else {
            input
        };
        let conv_out = &mut self.conv_out_blk[..mid * n];
        self.conv.process_block(conv_in, conv_out, n);
        if let Some(f) = &mut self.films[1] {
            f.process_block_(&mut self.conv_out_blk[..mid * n], condition, n);
        }

        // 2. mixin.
        let mix_in: &[f32] = if let Some(f) = &mut self.films[2] {
            let buf = &mut self.mix_pre_blk[..cond_w];
            f.process_block(condition, condition, buf, n);
            &self.mix_pre_blk[..cond_w]
        } else {
            condition
        };
        let mix = &mut self.mix_blk[..mid * n];
        self.mixin.process_block(mix_in, mix, n);
        if let Some(f) = &mut self.films[3] {
            f.process_block_(&mut self.mix_blk[..mid * n], condition, n);
        }

        // 3. z = conv_out + mix.
        {
            let (z, conv_out, mix) = (
                &mut self.z_blk[..mid * n],
                &self.conv_out_blk[..mid * n],
                &self.mix_blk[..mid * n],
            );
            for (zi, (c, m)) in z.iter_mut().zip(conv_out.iter().zip(mix)) {
                *zi = *c + *m;
            }
        }
        if let Some(f) = &mut self.films[4] {
            f.process_block_(&mut self.z_blk[..mid * n], condition, n);
        }

        // 4. gating into `act`, then activation_post FiLM.
        {
            let (z, act) = (&self.z_blk[..mid * n], &mut self.act_blk[..bn * n]);
            self.gating.process_block(z, act, n);
        }
        if let Some(f) = &mut self.films[5] {
            if self.mode == GatingMode::None {
                f.process_block_(&mut self.act_blk[..bn * n], condition, n);
            } else {
                let act_film = &mut self.act_film_blk[..bn * n];
                f.process_block(&self.act_blk[..bn * n], condition, act_film, n);
                self.act_blk[..bn * n].copy_from_slice(&self.act_film_blk[..bn * n]);
            }
        }

        // 5. head contribution.
        if let Some(h) = &mut self.head1x1 {
            let (act, hc) = (
                &self.act_blk[..bn * n],
                &mut self.head_contrib_blk[..hcw * n],
            );
            h.process_block(act, hc, n);
            if let Some(f) = &mut self.films[7] {
                f.process_block_(&mut self.head_contrib_blk[..hcw * n], condition, n);
            }
        } else {
            self.head_contrib_blk[..hcw * n].copy_from_slice(&self.act_blk[..bn * n]);
        }
        for (a, c) in head_accum.iter_mut().zip(&self.head_contrib_blk[..hcw * n]) {
            *a += *c;
        }

        // 6. residual.
        if let Some(l) = &mut self.layer1x1 {
            l.process_block(&self.act_blk[..bn * n], &mut self.res_blk[..ch * n], n);
            if self.mode == GatingMode::Blended {
                if let Some(f) = &mut self.films[6] {
                    f.process_block_(&mut self.res_blk[..ch * n], condition, n);
                }
            }
            for (o, (r, x)) in out.iter_mut().zip(self.res_blk[..ch * n].iter().zip(input)) {
                *o = *r + *x;
            }
        } else {
            out.copy_from_slice(input);
        }
    }

    pub(super) fn reset(&mut self) {
        self.conv.reset();
        self.mixin.reset();
        if let Some(l) = &mut self.layer1x1 {
            l.reset();
        }
        if let Some(h) = &mut self.head1x1 {
            h.reset();
        }
        for f in self.films.iter_mut().flatten() {
            f.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::activation::Activation;
    use super::super::gating::Gating;
    use super::*;
    use crate::model::GatingMode;

    // A1-style layer: NONE/GATED gating, no FiLM, no head1x1, layer1x1 active, groups 1.
    #[allow(clippy::too_many_arguments)]
    fn a1_layer(
        channels: usize,
        condition_size: usize,
        kernel: usize,
        dilation: usize,
        primary: Activation,
        mode: GatingMode,
        conv_w: Vec<f32>,
        conv_b: Vec<f32>,
        mix_w: Vec<f32>,
        one_w: Vec<f32>,
        one_b: Vec<f32>,
    ) -> Layer {
        let gating = Gating::new(mode, primary, Activation::Sigmoid, channels);
        Layer::new(
            LayerDims {
                channels,
                bottleneck: channels,
                condition_size,
                kernel,
                dilation,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w,
                conv_b,
                mix_w,
                layer1x1_w: Some(one_w),
                layer1x1_b: Some(one_b),
                head1x1_w: None,
                head1x1_b: None,
                films: [None, None, None, None, None, None, None, None],
            },
        )
    }

    #[test]
    fn relu_layer_residual_and_head_accumulate() {
        // channels=1, condition=1, kernel=1, dilation=1, ReLU, not gated.
        // conv: block = 2*input + 0.5 ; mixin: mix = 1*condition ; z = block+mix
        // post = relu(z) ; out = 3*post + 0.1 + input (residual) ; head += post
        let mut layer = a1_layer(
            1,
            1,
            1,
            1,
            Activation::Relu,
            GatingMode::None,
            vec![2.0],
            vec![0.5],
            vec![1.0],
            vec![3.0],
            vec![0.1],
        );

        let mut head = vec![0.0];
        let mut out = vec![0.0];

        // Sample 1: input=0.5, cond=0.5 -> z = 1.0+0.5+0.5 = 2.0, relu=2.0
        layer.process_sample(&[0.5], &[0.5], &mut head, &mut out);
        assert_eq!(out, vec![6.6]); // 3*2 + 0.1 + 0.5
        assert_eq!(head, vec![2.0]);

        // Sample 2: input=0.5, cond=-3.0 -> z = 1.0+0.5-3.0 = -1.5, relu=0
        layer.process_sample(&[0.5], &[-3.0], &mut head, &mut out);
        assert_eq!(out, vec![0.6]); // 3*0 + 0.1 + 0.5
        assert_eq!(head, vec![2.0]); // unchanged: += 0
    }

    #[test]
    fn gated_layer_multiplies_tanh_path_by_sigmoid_gate() {
        // channels=1, gated -> mid=2. Make the gate branch (block[1]) == 0 so
        // sigmoid(0)=0.5 exactly; ReLU on the value branch keeps it exact.
        // conv weights [out=2][in=1][k=1] = [value=2.0, gate=0.0]; bias [0,0].
        let mut layer = a1_layer(
            1,
            1,
            1,
            1,
            Activation::Relu,
            GatingMode::Gated,
            vec![2.0, 0.0],
            vec![0.0, 0.0],
            vec![0.0, 0.0],
            vec![1.0],
            vec![0.0],
        );

        let mut head = vec![0.0];
        let mut out = vec![0.0];
        // input=1, cond=0 -> block=[2,0]; post = relu(2)*sigmoid(0) = 2*0.5 = 1
        layer.process_sample(&[1.0], &[0.0], &mut head, &mut out);
        assert_eq!(head, vec![1.0]);
        assert_eq!(out, vec![2.0]); // 1*post + input residual = 1 + 1
    }

    #[test]
    fn tanh_activation_matches_reference_value() {
        let mut layer = a1_layer(
            1,
            1,
            1,
            1,
            Activation::Tanh,
            GatingMode::None,
            vec![2.0],
            vec![0.5],
            vec![1.0],
            vec![3.0],
            vec![0.1],
        );
        let mut head = vec![0.0];
        let mut out = vec![0.0];
        // z = 2.0 ; post = tanh(2.0) (the vendored fast tanh) ; out = 3*post + 0.1 + 0.5
        layer.process_sample(&[0.5], &[0.5], &mut head, &mut out);
        let post = crate::fast_math::fast_tanh(2.0);
        assert!((head[0] - post).abs() < 1e-6, "head={}", head[0]);
        assert!((out[0] - (3.0 * post + 0.6)).abs() < 1e-6, "out={}", out[0]);
    }

    #[test]
    fn head_in_width_decouples_from_channels() {
        // head1x1 active out=1 with channels=2, bottleneck=2: head contribution is
        // width 1, decoupled from channels=2.
        let gating = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 2);
        let layer = Layer::new(
            LayerDims {
                channels: 2,
                bottleneck: 2,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: Some(1),
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![1.0, 0.0, 0.0, 1.0],
                conv_b: vec![0.0, 0.0],
                mix_w: vec![0.0, 0.0],
                layer1x1_w: Some(vec![1.0, 0.0, 0.0, 1.0]),
                layer1x1_b: Some(vec![0.0, 0.0]),
                head1x1_w: Some(vec![1.0, 1.0]),
                head1x1_b: Some(vec![0.0]),
                films: [None, None, None, None, None, None, None, None],
            },
        );
        assert_eq!(layer.head_contrib_width(), 1);
    }

    #[test]
    fn blended_layer_matches_hand_values() {
        // channels=1, bottleneck=1 (mid=2), no FiLM, no head1x1, layer1x1 active.
        // z = [v, g] = [-2.0, 0.0]: conv [out=2][in=1][k=1] = [-2.0, 0.0], bias [0,0],
        // mix [0,0], input 1.0, cond 0. alpha=sigmoid(0)=0.5;
        // act = 0.5*relu(-2) + 0.5*(-2) = -1.0. head_accum += -1.0.
        // residual: out = input + layer1x1(act) = 1.0 + 1.0*(-1.0) = 0.0.
        let gating = Gating::new(
            GatingMode::Blended,
            Activation::Relu,
            Activation::Sigmoid,
            1,
        );
        let mut layer = Layer::new(
            LayerDims {
                channels: 1,
                bottleneck: 1,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![-2.0, 0.0],
                conv_b: vec![0.0, 0.0],
                mix_w: vec![0.0, 0.0],
                layer1x1_w: Some(vec![1.0]),
                layer1x1_b: Some(vec![0.0]),
                head1x1_w: None,
                head1x1_b: None,
                films: [None, None, None, None, None, None, None, None],
            },
        );
        let mut head = vec![0.0];
        let mut out = vec![0.0];
        layer.process_sample(&[1.0], &[0.0], &mut head, &mut out);
        assert!((head[0] - (-1.0)).abs() < 1e-6, "head={}", head[0]);
        assert!((out[0] - 0.0).abs() < 1e-6, "out={}", out[0]);
    }

    #[test]
    fn bottleneck_smaller_than_channels_routes_through_layer1x1() {
        // channels=2, bottleneck=1, NONE, ReLU. mid=1. layer1x1 maps bn(1)->channels(2).
        // conv [out=1][in=2][k=1] = [1.0, 0.0] bias [0]; mix [0,0]; input=[3.0, 7.0] cond 0.
        // z = 1*3 + 0*7 = 3 ; act = relu(3) = 3. layer1x1 W [out=2][in=1] = [2.0, -1.0] b [0,0].
        // res = [2*3, -1*3] = [6, -3]. out = input + res = [3+6, 7-3] = [9, 4]. head += [3].
        let gating = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 1);
        let mut layer = Layer::new(
            LayerDims {
                channels: 2,
                bottleneck: 1,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![1.0, 0.0],
                conv_b: vec![0.0],
                mix_w: vec![0.0],
                layer1x1_w: Some(vec![2.0, -1.0]),
                layer1x1_b: Some(vec![0.0, 0.0]),
                head1x1_w: None,
                head1x1_b: None,
                films: [None, None, None, None, None, None, None, None],
            },
        );
        let mut head = vec![0.0]; // head_contrib width = bottleneck = 1
        let mut out = vec![0.0, 0.0];
        layer.process_sample(&[3.0, 7.0], &[0.0], &mut head, &mut out);
        assert_eq!(head, vec![3.0]);
        assert_eq!(out, vec![9.0, 4.0]);
    }

    #[test]
    fn head1x1_produces_separate_head_contribution() {
        // channels=2, bottleneck=2, NONE, ReLU. head1x1 active out=1, groups 1.
        // conv identity: [out=2][in=2][k=1] = [1,0, 0,1] bias [0,0]; mix [0;2]; cond 0.
        // input=[2.0, 3.0] -> z=[2,3] -> act=relu=[2,3].
        // head1x1 W [out=1][in=2] = [1.0, 10.0] b [0] -> head_contrib = 2*1 + 3*10 = 32.
        // layer1x1 W [out=2][in=2] = identity [1,0,0,1] b [0,0] -> res=[2,3]; out=input+res=[4,6].
        let gating = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 2);
        let mut layer = Layer::new(
            LayerDims {
                channels: 2,
                bottleneck: 2,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: Some(1),
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![1.0, 0.0, 0.0, 1.0],
                conv_b: vec![0.0, 0.0],
                mix_w: vec![0.0, 0.0],
                layer1x1_w: Some(vec![1.0, 0.0, 0.0, 1.0]),
                layer1x1_b: Some(vec![0.0, 0.0]),
                head1x1_w: Some(vec![1.0, 10.0]),
                head1x1_b: Some(vec![0.0]),
                films: [None, None, None, None, None, None, None, None],
            },
        );
        assert_eq!(layer.head_contrib_width(), 1);
        let mut head = vec![0.0]; // head_contrib width = head1x1_out = 1
        let mut out = vec![0.0, 0.0];
        layer.process_sample(&[2.0, 3.0], &[0.0], &mut head, &mut out);
        assert_eq!(head, vec![32.0]);
        assert_eq!(out, vec![4.0, 6.0]);
    }

    #[test]
    fn inactive_layer1x1_is_identity_residual() {
        // channels=2, bottleneck=2 (must equal channels), NONE, ReLU, layer1x1 inactive.
        // out = input directly; head_contrib = act.
        // conv [out=2][in=2][k=1]=[1,0,0,1] b[0,0]; mix[0;2]; input=[5,-1] cond 0.
        // z=[5,-1] -> act=relu=[5,0]. head += [5,0]. out = input = [5,-1].
        let gating = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 2);
        let mut layer = Layer::new(
            LayerDims {
                channels: 2,
                bottleneck: 2,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![1.0, 0.0, 0.0, 1.0],
                conv_b: vec![0.0, 0.0],
                mix_w: vec![0.0, 0.0],
                layer1x1_w: None,
                layer1x1_b: None,
                head1x1_w: None,
                head1x1_b: None,
                films: [None, None, None, None, None, None, None, None],
            },
        );
        let mut head = vec![0.0, 0.0];
        let mut out = vec![0.0, 0.0];
        layer.process_sample(&[5.0, -1.0], &[0.0], &mut head, &mut out);
        assert_eq!(head, vec![5.0, 0.0]);
        assert_eq!(out, vec![5.0, -1.0]);
    }

    #[test]
    fn conv_post_film_scales_conv_output_before_sum() {
        // channels=1 bottleneck=1 mid=1 NONE ReLU. conv [1]=[2.0] b[0]; input=1 -> conv_out=2.
        // conv_post_film[1] scale-only: cond_dim=1, input_dim=mid=1, W=[3.0] b=[0]; cond=1
        //   -> scale=3 -> conv_out=6. mix=0. z=6. act=relu(6)=6. head+=6.
        //   layer1x1 W[1]=[1] b[0] -> res=6. out = input + res = 7.
        let gating = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 1);
        let mut films: [Option<(Vec<f32>, Vec<f32>)>; 8] =
            [None, None, None, None, None, None, None, None];
        films[1] = Some((vec![3.0], vec![0.0])); // conv_post_film, scale-only (shift=false)
        let mut layer = Layer::new(
            LayerDims {
                channels: 1,
                bottleneck: 1,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![2.0],
                conv_b: vec![0.0],
                mix_w: vec![0.0],
                layer1x1_w: Some(vec![1.0]),
                layer1x1_b: Some(vec![0.0]),
                head1x1_w: None,
                head1x1_b: None,
                films,
            },
        );
        let mut head = vec![0.0];
        let mut out = vec![0.0];
        layer.process_sample(&[1.0], &[1.0], &mut head, &mut out);
        assert_eq!(head, vec![6.0]);
        assert_eq!(out, vec![7.0]);
    }

    #[test]
    fn input_mixin_pre_film_modulates_condition_before_mixer() {
        // channels=1 bottleneck=1 mid=1 NONE ReLU. conv [1]=[0.0] b[0] (conv_out=0).
        // input_mixin_pre_film[2]: input_dim=condition_size=1 scale-only W=[5.0] b[0]; cond=2
        //   -> FiLM modulates the condition: scale = W*cond = 5*2 = 10; mix_in = cond*scale
        //   = 2*10 = 20. mixin W[in=1,out=1]=[1.0] -> mix=20. z=0+20=20.
        //   act=relu(20)=20. head+=20. layer1x1 W=[1] b[0] -> res=20. out=input(0)+20=20.
        let gating = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 1);
        let mut films: [Option<(Vec<f32>, Vec<f32>)>; 8] =
            [None, None, None, None, None, None, None, None];
        films[2] = Some((vec![5.0], vec![0.0])); // input_mixin_pre_film, scale-only
        let mut layer = Layer::new(
            LayerDims {
                channels: 1,
                bottleneck: 1,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![0.0],
                conv_b: vec![0.0],
                mix_w: vec![1.0],
                layer1x1_w: Some(vec![1.0]),
                layer1x1_b: Some(vec![0.0]),
                head1x1_w: None,
                head1x1_b: None,
                films,
            },
        );
        let mut head = vec![0.0];
        let mut out = vec![0.0];
        layer.process_sample(&[0.0], &[2.0], &mut head, &mut out);
        assert_eq!(head, vec![20.0]);
        assert_eq!(out, vec![20.0]);
    }

    #[test]
    fn layer1x1_post_film_applies_only_in_blended_branch() {
        // BLENDED channels=1 bottleneck=1 mid=2. Reuse blended_layer values:
        // z=[-2,0] -> act=-1. layer1x1 W[1]=[1] b[0] -> res=-1.
        // layer1x1_post_film[6] scale-only W=[3.0] b[0], cond=1 -> res = -1*3 = -3.
        // out = input + res = 1 + (-3) = -2. head += act = -1.
        let gating = Gating::new(
            GatingMode::Blended,
            Activation::Relu,
            Activation::Sigmoid,
            1,
        );
        let mut films: [Option<(Vec<f32>, Vec<f32>)>; 8] =
            [None, None, None, None, None, None, None, None];
        films[6] = Some((vec![3.0], vec![0.0]));
        let mut layer = Layer::new(
            LayerDims {
                channels: 1,
                bottleneck: 1,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating,
            LayerWeights {
                conv_w: vec![-2.0, 0.0],
                conv_b: vec![0.0, 0.0],
                mix_w: vec![0.0, 0.0],
                layer1x1_w: Some(vec![1.0]),
                layer1x1_b: Some(vec![0.0]),
                head1x1_w: None,
                head1x1_b: None,
                films,
            },
        );
        let mut head = vec![0.0];
        let mut out = vec![0.0];
        layer.process_sample(&[1.0], &[1.0], &mut head, &mut out);
        assert!((head[0] - (-1.0)).abs() < 1e-6, "head={}", head[0]);
        assert!((out[0] - (-2.0)).abs() < 1e-6, "out={}", out[0]);

        // Same films[6] present, but NONE gating: the layer1x1_post_film must be IGNORED.
        let gating_n = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 1);
        let mut films_n: [Option<(Vec<f32>, Vec<f32>)>; 8] =
            [None, None, None, None, None, None, None, None];
        films_n[6] = Some((vec![3.0], vec![0.0]));
        let mut layer_n = Layer::new(
            LayerDims {
                channels: 1,
                bottleneck: 1,
                condition_size: 1,
                kernel: 1,
                dilation: 1,
                groups_input: 1,
                groups_input_mixin: 1,
                layer1x1_groups: 1,
                head1x1_groups: 1,
                head1x1_out: None,
                film_shift: [false; 8],
                film_groups: [1; 8],
            },
            gating_n,
            LayerWeights {
                conv_w: vec![3.0],
                conv_b: vec![0.0],
                mix_w: vec![0.0],
                layer1x1_w: Some(vec![1.0]),
                layer1x1_b: Some(vec![0.0]),
                head1x1_w: None,
                head1x1_b: None,
                films: films_n,
            },
        );
        // input=1 -> conv_out=3 -> z=3 -> act=relu(3)=3. res=1*3=3 (NO post_film scaling).
        // out = input + res = 1 + 3 = 4.
        let mut head_n = vec![0.0];
        let mut out_n = vec![0.0];
        layer_n.process_sample(&[1.0], &[1.0], &mut head_n, &mut out_n);
        assert!((out_n[0] - 4.0).abs() < 1e-6, "NONE out={}", out_n[0]);
    }

    /// Block ≡ per-sample with several FiLM sites active (conv_pre scale, conv_post
    /// shift, activation_pre scale) under GATED gating.
    #[test]
    fn process_block_equals_per_sample_with_films_active() {
        let channels = 2usize;
        let bottleneck = 2usize;
        let cond_sz = 2usize;
        let kernel = 3usize;
        let dilation = 2usize;
        let mid = 2 * bottleneck; // GATED
        let mk = |len: usize, salt: usize| -> Vec<f32> {
            (0..len)
                .map(|i| (((i * 17 + salt * 11) % 31) as f32 - 15.0) * 0.05)
                .collect()
        };

        let mut film_shift = [false; 8];
        film_shift[1] = true; // conv_post_film uses shift
        let mk_layer = || {
            let gating = Gating::new(
                GatingMode::Gated,
                Activation::Tanh,
                Activation::Sigmoid,
                bottleneck,
            );
            let mut films: [Option<(Vec<f32>, Vec<f32>)>; 8] =
                [None, None, None, None, None, None, None, None];
            // conv_pre_film[0]: input_dim=channels, scale-only.
            films[0] = Some((mk(channels * cond_sz, 11), mk(channels, 12)));
            // conv_post_film[1]: input_dim=mid, shift -> out_rows=2*mid.
            films[1] = Some((mk(2 * mid * cond_sz, 13), mk(2 * mid, 14)));
            // activation_pre_film[4]: input_dim=mid, scale-only.
            films[4] = Some((mk(mid * cond_sz, 15), mk(mid, 16)));
            Layer::new(
                LayerDims {
                    channels,
                    bottleneck,
                    condition_size: cond_sz,
                    kernel,
                    dilation,
                    groups_input: 1,
                    groups_input_mixin: 1,
                    layer1x1_groups: 1,
                    head1x1_groups: 1,
                    head1x1_out: None,
                    film_shift,
                    film_groups: [1; 8],
                },
                gating,
                LayerWeights {
                    conv_w: mk(mid * channels * kernel, 1),
                    conv_b: mk(mid, 2),
                    mix_w: mk(mid * cond_sz, 3),
                    layer1x1_w: Some(mk(channels * bottleneck, 4)),
                    layer1x1_b: Some(mk(channels, 5)),
                    head1x1_w: None,
                    head1x1_b: None,
                    films,
                },
            )
        };

        let total = 130usize;
        let inp: Vec<Vec<f32>> = (0..total)
            .map(|t| {
                (0..channels)
                    .map(|c| ((t * 3 + c) as f32 * 0.19).sin())
                    .collect()
            })
            .collect();
        let cond: Vec<Vec<f32>> = (0..total)
            .map(|t| {
                (0..cond_sz)
                    .map(|c| ((t * 5 + c) as f32 * 0.13).cos())
                    .collect()
            })
            .collect();
        let seed: Vec<Vec<f32>> = (0..total)
            .map(|t| (0..bottleneck).map(|c| ((t + c) as f32) * 0.01).collect())
            .collect();

        let mut a = mk_layer();
        let mut out_ref = vec![vec![0.0; channels]; total];
        let mut head_ref = vec![vec![0.0; bottleneck]; total];
        for t in 0..total {
            let mut head = seed[t].clone();
            let mut out = vec![0.0; channels];
            a.process_sample(&inp[t], &cond[t], &mut head, &mut out);
            out_ref[t] = out;
            head_ref[t] = head;
        }

        let mut b = mk_layer();
        for (lo, len) in [(0usize, 70usize), (70, 60)] {
            let mut bin = vec![0.0; channels * len];
            let mut bcond = vec![0.0; cond_sz * len];
            let mut bhead = vec![0.0; bottleneck * len];
            for lt in 0..len {
                for c in 0..channels {
                    bin[c * len + lt] = inp[lo + lt][c];
                }
                for c in 0..bottleneck {
                    bhead[c * len + lt] = seed[lo + lt][c];
                }
                for c in 0..cond_sz {
                    bcond[c * len + lt] = cond[lo + lt][c];
                }
            }
            let mut bout = vec![0.0; channels * len];
            b.process_block(&bin, &bcond, &mut bhead, &mut bout, len);
            for lt in 0..len {
                for c in 0..channels {
                    let go = bout[c * len + lt];
                    assert!(
                        (go - out_ref[lo + lt][c]).abs() < 1e-5,
                        "t{} c{c} out: got {go}, want {}",
                        lo + lt,
                        out_ref[lo + lt][c]
                    );
                }
                for c in 0..bottleneck {
                    let gh = bhead[c * len + lt];
                    assert!(
                        (gh - head_ref[lo + lt][c]).abs() < 1e-5,
                        "t{} c{c} head: got {gh}, want {}",
                        lo + lt,
                        head_ref[lo + lt][c]
                    );
                }
            }
        }
    }

    /// Block path reproduces the per-sample path for a full layer, gated and not,
    /// multi-channel, dilated, with a per-sample head seed carried in planar form.
    #[test]
    fn process_block_equals_process_sample_loop() {
        for mode in [GatingMode::None, GatingMode::Gated] {
            let channels = 3usize;
            let cond_sz = 2usize;
            let kernel = 3usize;
            let dilation = 4usize;
            let mid = if mode == GatingMode::None {
                channels
            } else {
                2 * channels
            };
            let mk = |len: usize, salt: usize| -> Vec<f32> {
                (0..len)
                    .map(|i| (((i * 31 + salt * 7) % 29) as f32 - 14.0) * 0.07)
                    .collect()
            };
            let conv_w = mk(mid * channels * kernel, 1);
            let conv_b = mk(mid, 2);
            let mix_w = mk(mid * cond_sz, 3);
            let one_w = mk(channels * channels, 4);
            let one_b = mk(channels, 5);

            let total = 130usize;
            let inp: Vec<Vec<f32>> = (0..total)
                .map(|t| {
                    (0..channels)
                        .map(|c| ((t * 3 + c) as f32 * 0.21).sin())
                        .collect()
                })
                .collect();
            let cond: Vec<Vec<f32>> = (0..total)
                .map(|t| {
                    (0..cond_sz)
                        .map(|c| ((t * 5 + c) as f32 * 0.17).cos())
                        .collect()
                })
                .collect();
            let seed: Vec<Vec<f32>> = (0..total)
                .map(|t| (0..channels).map(|c| ((t + c) as f32) * 0.01).collect())
                .collect();

            let mk_layer = || {
                a1_layer(
                    channels,
                    cond_sz,
                    kernel,
                    dilation,
                    Activation::Tanh,
                    mode,
                    conv_w.clone(),
                    conv_b.clone(),
                    mix_w.clone(),
                    one_w.clone(),
                    one_b.clone(),
                )
            };

            // Reference: per-sample.
            let mut a = mk_layer();
            let mut out_ref = vec![vec![0.0; channels]; total];
            let mut head_ref = vec![vec![0.0; channels]; total];
            for t in 0..total {
                let mut head = seed[t].clone();
                let mut out = vec![0.0; channels];
                a.process_sample(&inp[t], &cond[t], &mut head, &mut out);
                out_ref[t] = out;
                head_ref[t] = head;
            }

            // Under test: block path in two chunks.
            let mut b = mk_layer();
            for (lo, len) in [(0usize, 70usize), (70, 60)] {
                let mut bin = vec![0.0; channels * len];
                let mut bcond = vec![0.0; cond_sz * len];
                let mut bhead = vec![0.0; channels * len];
                for lt in 0..len {
                    for c in 0..channels {
                        bin[c * len + lt] = inp[lo + lt][c];
                        bhead[c * len + lt] = seed[lo + lt][c];
                    }
                    for c in 0..cond_sz {
                        bcond[c * len + lt] = cond[lo + lt][c];
                    }
                }
                let mut bout = vec![0.0; channels * len];
                b.process_block(&bin, &bcond, &mut bhead, &mut bout, len);
                for lt in 0..len {
                    for c in 0..channels {
                        let go = bout[c * len + lt];
                        let gh = bhead[c * len + lt];
                        assert!(
                            (go - out_ref[lo + lt][c]).abs() < 1e-5,
                            "mode={mode:?} t{} c{c} out: got {go}, want {}",
                            lo + lt,
                            out_ref[lo + lt][c]
                        );
                        assert!(
                            (gh - head_ref[lo + lt][c]).abs() < 1e-5,
                            "mode={mode:?} t{} c{c} head: got {gh}, want {}",
                            lo + lt,
                            head_ref[lo + lt][c]
                        );
                    }
                }
            }
        }
    }

    /// Grouped sweep: the block path equals the per-sample path across gating modes
    /// and group counts for conv / mixin / layer1x1, with compact grouped weights.
    #[test]
    fn process_block_equals_process_sample_loop_grouped() {
        let channels = 4usize;
        let bottleneck = 4usize;
        let cond_sz = 2usize;
        let kernel = 3usize;
        let dilation = 4usize;
        let mk = |len: usize, salt: usize| -> Vec<f32> {
            (0..len)
                .map(|i| (((i * 31 + salt * 7) % 29) as f32 - 14.0) * 0.07)
                .collect()
        };

        for mode in [GatingMode::None, GatingMode::Gated] {
            let mid = if mode == GatingMode::None {
                bottleneck
            } else {
                2 * bottleneck
            };
            for gi in [1usize, 2] {
                for gim in [1usize, 2] {
                    for gl in [1usize, 2] {
                        let conv_w = mk(mid * channels * kernel / gi, 1);
                        let conv_b = mk(mid, 2);
                        let mix_w = mk(mid * cond_sz / gim, 3);
                        let one_w = mk(channels * bottleneck / gl, 4);
                        let one_b = mk(channels, 5);

                        let mk_layer = || {
                            let gating = Gating::new(
                                mode,
                                Activation::Tanh,
                                Activation::Sigmoid,
                                bottleneck,
                            );
                            Layer::new(
                                LayerDims {
                                    channels,
                                    bottleneck,
                                    condition_size: cond_sz,
                                    kernel,
                                    dilation,
                                    groups_input: gi,
                                    groups_input_mixin: gim,
                                    layer1x1_groups: gl,
                                    head1x1_groups: 1,
                                    head1x1_out: None,
                                    film_shift: [false; 8],
                                    film_groups: [1; 8],
                                },
                                gating,
                                LayerWeights {
                                    conv_w: conv_w.clone(),
                                    conv_b: conv_b.clone(),
                                    mix_w: mix_w.clone(),
                                    layer1x1_w: Some(one_w.clone()),
                                    layer1x1_b: Some(one_b.clone()),
                                    head1x1_w: None,
                                    head1x1_b: None,
                                    films: [None, None, None, None, None, None, None, None],
                                },
                            )
                        };

                        let total = 130usize;
                        let inp: Vec<Vec<f32>> = (0..total)
                            .map(|t| {
                                (0..channels)
                                    .map(|c| ((t * 3 + c) as f32 * 0.21).sin())
                                    .collect()
                            })
                            .collect();
                        let cond: Vec<Vec<f32>> = (0..total)
                            .map(|t| {
                                (0..cond_sz)
                                    .map(|c| ((t * 5 + c) as f32 * 0.17).cos())
                                    .collect()
                            })
                            .collect();
                        let seed: Vec<Vec<f32>> = (0..total)
                            .map(|t| (0..bottleneck).map(|c| ((t + c) as f32) * 0.01).collect())
                            .collect();

                        // Reference: per-sample.
                        let mut a = mk_layer();
                        let mut out_ref = vec![vec![0.0; channels]; total];
                        let mut head_ref = vec![vec![0.0; bottleneck]; total];
                        for t in 0..total {
                            let mut head = seed[t].clone();
                            let mut out = vec![0.0; channels];
                            a.process_sample(&inp[t], &cond[t], &mut head, &mut out);
                            out_ref[t] = out;
                            head_ref[t] = head;
                        }

                        // Under test: block path in two chunks.
                        let mut b = mk_layer();
                        for (lo, len) in [(0usize, 70usize), (70, 60)] {
                            let mut bin = vec![0.0; channels * len];
                            let mut bcond = vec![0.0; cond_sz * len];
                            let mut bhead = vec![0.0; bottleneck * len];
                            for lt in 0..len {
                                for c in 0..channels {
                                    bin[c * len + lt] = inp[lo + lt][c];
                                }
                                for c in 0..bottleneck {
                                    bhead[c * len + lt] = seed[lo + lt][c];
                                }
                                for c in 0..cond_sz {
                                    bcond[c * len + lt] = cond[lo + lt][c];
                                }
                            }
                            let mut bout = vec![0.0; channels * len];
                            b.process_block(&bin, &bcond, &mut bhead, &mut bout, len);
                            for lt in 0..len {
                                for c in 0..channels {
                                    let go = bout[c * len + lt];
                                    assert!(
                                        (go - out_ref[lo + lt][c]).abs() < 1e-5,
                                        "mode={mode:?} gi{gi} gim{gim} gl{gl} t{} c{c} out: \
                                         got {go}, want {}",
                                        lo + lt,
                                        out_ref[lo + lt][c]
                                    );
                                }
                                for c in 0..bottleneck {
                                    let gh = bhead[c * len + lt];
                                    assert!(
                                        (gh - head_ref[lo + lt][c]).abs() < 1e-5,
                                        "mode={mode:?} gi{gi} gim{gim} gl{gl} t{} c{c} head: \
                                         got {gh}, want {}",
                                        lo + lt,
                                        head_ref[lo + lt][c]
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
