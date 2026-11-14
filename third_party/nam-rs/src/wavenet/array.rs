//! A layer-array: an input rechannel, a stack of dilated [`Layer`]s, and a head
//! rechannel that maps the accumulated head signal to this array's `head_size`.
//!
//! Ported from NeuralAudio `WaveNetLayerArrayT`. The head accumulator is seeded
//! with the *incoming* head (the previous array's head output, or silence for the
//! first array), each layer adds its post-activation to it, and the head
//! rechannel projects the result:
//!
//! ```text
//! x = rechannel(input)
//! head_accum = head_in            // channels wide
//! for layer: x, head_accum = layer(x, condition, head_accum)
//! head_out = head_rechannel(head_accum)
//! ```

use super::conv::{Conv1d, MAX_BLOCK};
use super::layer::Layer;

/// A stack of dilated layers sharing channel/kernel parameters.
#[derive(Debug, Clone)]
pub(super) struct LayerArray {
    rechannel: Conv1d,
    layers: Vec<Layer>,
    head_rechannel: Conv1d,
    channels: usize,
    /// Width of the per-layer head contribution (`head1x1.active ? head1x1_out :
    /// bottleneck`). The head accumulator and the head-rechannel input are this wide,
    /// which decouples them from `channels`.
    head_in: usize,
    head_size: usize,
    /// `channels`-wide scratch: current layer-signal.
    cur: Vec<f32>,
    /// `channels`-wide scratch: next layer-signal (ping-ponged with `cur`).
    nxt: Vec<f32>,
    /// `head_in`-wide head accumulator.
    head_accum: Vec<f32>,
    /// Planar `[channels][MAX_BLOCK]` block-path twins of `cur`/`nxt`, and
    /// `[head_in][MAX_BLOCK]` for `head_accum`.
    cur_blk: Vec<f32>,
    nxt_blk: Vec<f32>,
    head_accum_blk: Vec<f32>,
}

impl LayerArray {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        input_size: usize,
        channels: usize,
        head_in: usize,
        head_size: usize,
        head_kernel_size: usize,
        rechannel_w: Vec<f32>,
        layers: Vec<Layer>,
        head_w: Vec<f32>,
        head_b: Option<Vec<f32>>,
    ) -> Self {
        Self {
            rechannel: Conv1d::new(input_size, channels, 1, 1, rechannel_w, None),
            layers,
            head_rechannel: Conv1d::new(head_in, head_size, head_kernel_size, 1, head_w, head_b),
            channels,
            head_in,
            head_size,
            cur: vec![0.0; channels],
            nxt: vec![0.0; channels],
            head_accum: vec![0.0; head_in],
            cur_blk: vec![0.0; channels * MAX_BLOCK],
            nxt_blk: vec![0.0; channels * MAX_BLOCK],
            head_accum_blk: vec![0.0; head_in * MAX_BLOCK],
        }
    }

    pub(super) fn channels(&self) -> usize {
        self.channels
    }

    /// Width of the head accumulator / incoming-head buffer (`head1x1.active ?
    /// head1x1_out : bottleneck`). The previous array's `head_size` output is seeded
    /// into a buffer of this width.
    pub(super) fn head_in(&self) -> usize {
        self.head_in
    }

    pub(super) fn head_size(&self) -> usize {
        self.head_size
    }

    /// Process one sample.
    ///
    /// - `input`: `input_size` wide.
    /// - `condition`: conditioning signal.
    /// - `head_prev`: `head_in`-wide incoming head (silence for the first array).
    /// - `head_out`: `head_size`-wide head output.
    /// - `array_out`: `channels`-wide output for the next array.
    pub(super) fn process_sample(
        &mut self,
        input: &[f32],
        condition: &[f32],
        head_prev: &[f32],
        head_out: &mut [f32],
        array_out: &mut [f32],
    ) {
        self.rechannel.process_sample(input, &mut self.cur);
        self.head_accum.copy_from_slice(head_prev);

        for i in 0..self.layers.len() {
            self.layers[i].process_sample(
                &self.cur,
                condition,
                &mut self.head_accum,
                &mut self.nxt,
            );
            std::mem::swap(&mut self.cur, &mut self.nxt);
        }

        self.head_rechannel
            .process_sample(&self.head_accum, head_out);
        array_out.copy_from_slice(&self.cur);
    }

    /// Block twin of [`Self::process_sample`]. All slices are **planar** `[ch][t]`,
    /// `n <= MAX_BLOCK`: `input` is `input_size * n`, `head_prev` is `head_in * n`,
    /// `array_out` is `channels * n`, `head_out` is `head_size * n`. Allocation-free.
    pub(super) fn process_block(
        &mut self,
        input: &[f32],
        condition: &[f32],
        head_prev: &[f32],
        head_out: &mut [f32],
        array_out: &mut [f32],
        n: usize,
    ) {
        let ch = self.channels;
        let hin = self.head_in;
        self.rechannel
            .process_block(input, &mut self.cur_blk[..ch * n], n);
        self.head_accum_blk[..hin * n].copy_from_slice(head_prev);

        for i in 0..self.layers.len() {
            // Split-borrow the two ping-pong scratch rows for this block length.
            let (cur, nxt) = (&self.cur_blk[..ch * n], &mut self.nxt_blk[..ch * n]);
            // Layer reads `cur`, writes `nxt`; head accumulates in place.
            self.layers[i].process_block(
                cur,
                condition,
                &mut self.head_accum_blk[..hin * n],
                nxt,
                n,
            );
            std::mem::swap(&mut self.cur_blk, &mut self.nxt_blk);
        }

        self.head_rechannel
            .process_block(&self.head_accum_blk[..hin * n], head_out, n);
        array_out.copy_from_slice(&self.cur_blk[..ch * n]);
    }

    pub(super) fn reset(&mut self) {
        self.rechannel.reset();
        self.head_rechannel.reset();
        for layer in &mut self.layers {
            layer.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::activation::Activation;
    use super::super::gating::Gating;
    use super::super::layer::{Layer, LayerDims, LayerWeights};
    use super::*;
    use crate::model::GatingMode;

    // A1-style layer (NONE gating, no FiLM, no head1x1, layer1x1 active, groups 1).
    #[allow(clippy::too_many_arguments)]
    fn a1_layer(
        channels: usize,
        condition_size: usize,
        kernel: usize,
        dilation: usize,
        primary: Activation,
        conv_w: Vec<f32>,
        conv_b: Vec<f32>,
        mix_w: Vec<f32>,
        one_w: Vec<f32>,
        one_b: Vec<f32>,
    ) -> Layer {
        let gating = Gating::new(GatingMode::None, primary, Activation::Sigmoid, channels);
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

    fn relu_layer(
        channels: usize,
        conv_w: Vec<f32>,
        conv_b: Vec<f32>,
        mix_w: Vec<f32>,
        one_w: Vec<f32>,
        one_b: Vec<f32>,
    ) -> Layer {
        a1_layer(
            channels,
            1,
            1,
            1,
            Activation::Relu,
            conv_w,
            conv_b,
            mix_w,
            one_w,
            one_b,
        )
    }

    /// Block path reproduces the per-sample path for a multi-layer, dilated,
    /// multi-channel array, with a per-sample incoming head.
    #[test]
    fn process_block_equals_process_sample_loop() {
        let input_size = 1usize;
        let channels = 3usize;
        let head_size = 2usize;
        let cond_sz = 1usize;
        let kernel = 3usize;
        let mk = |len: usize, salt: usize| -> Vec<f32> {
            (0..len)
                .map(|i| (((i * 13 + salt * 5) % 19) as f32 - 9.0) * 0.06)
                .collect()
        };
        let tanh_layer = |dilation: usize, salt: usize| {
            a1_layer(
                channels,
                cond_sz,
                kernel,
                dilation,
                Activation::Tanh,
                mk(channels * channels * kernel, salt),
                mk(channels, salt + 1),
                mk(channels * cond_sz, salt + 2),
                mk(channels * channels, salt + 3),
                mk(channels, salt + 4),
            )
        };
        let mk_array = || {
            LayerArray::new(
                input_size,
                channels,
                channels,
                head_size,
                1,
                mk(channels * input_size, 20),
                vec![tanh_layer(1, 30), tanh_layer(2, 40), tanh_layer(4, 50)],
                mk(head_size * channels, 60),
                Some(mk(head_size, 70)),
            )
        };

        let total = 120usize;
        let inp: Vec<Vec<f32>> = (0..total)
            .map(|t| {
                (0..input_size)
                    .map(|c| ((t + c) as f32 * 0.23).sin())
                    .collect()
            })
            .collect();
        let cond: Vec<Vec<f32>> = (0..total)
            .map(|t| {
                (0..cond_sz)
                    .map(|c| ((t + c) as f32 * 0.19).cos())
                    .collect()
            })
            .collect();
        let head_in: Vec<Vec<f32>> = (0..total)
            .map(|t| {
                (0..channels)
                    .map(|c| ((t * 2 + c) as f32) * 0.013)
                    .collect()
            })
            .collect();

        // Reference: per-sample.
        let mut a = mk_array();
        let mut head_ref = vec![vec![0.0; head_size]; total];
        let mut out_ref = vec![vec![0.0; channels]; total];
        for t in 0..total {
            let mut ho = vec![0.0; head_size];
            let mut ao = vec![0.0; channels];
            a.process_sample(&inp[t], &cond[t], &head_in[t], &mut ho, &mut ao);
            head_ref[t] = ho;
            out_ref[t] = ao;
        }

        // Under test: block path in uneven chunks.
        let mut b = mk_array();
        let mut lo = 0usize;
        for &len in &[33usize, 1, 86] {
            let mut bin = vec![0.0; input_size * len];
            let mut bcond = vec![0.0; cond_sz * len];
            let mut bhead = vec![0.0; channels * len];
            for lt in 0..len {
                for c in 0..input_size {
                    bin[c * len + lt] = inp[lo + lt][c];
                }
                for c in 0..cond_sz {
                    bcond[c * len + lt] = cond[lo + lt][c];
                }
                for c in 0..channels {
                    bhead[c * len + lt] = head_in[lo + lt][c];
                }
            }
            let mut bho = vec![0.0; head_size * len];
            let mut bao = vec![0.0; channels * len];
            b.process_block(&bin, &bcond, &bhead, &mut bho, &mut bao, len);
            for lt in 0..len {
                for c in 0..head_size {
                    let g = bho[c * len + lt];
                    assert!(
                        (g - head_ref[lo + lt][c]).abs() < 1e-5,
                        "t{} head c{c}: got {g}, want {}",
                        lo + lt,
                        head_ref[lo + lt][c]
                    );
                }
                for c in 0..channels {
                    let g = bao[c * len + lt];
                    assert!(
                        (g - out_ref[lo + lt][c]).abs() < 1e-5,
                        "t{} out c{c}: got {g}, want {}",
                        lo + lt,
                        out_ref[lo + lt][c]
                    );
                }
            }
            lo += len;
        }
    }

    #[test]
    fn single_layer_array_rechannels_and_projects_head() {
        // rechannel r=1 ; layer: z=2*x+0.5+cond ; relu ; out=3*post+0.1+x ;
        // head_rechannel h=0.5, no bias ; head_in = silence.
        let layer = relu_layer(1, vec![2.0], vec![0.5], vec![1.0], vec![3.0], vec![0.1]);
        let mut array = LayerArray::new(1, 1, 1, 1, 1, vec![1.0], vec![layer], vec![0.5], None);

        let mut head_out = vec![0.0];
        let mut array_out = vec![0.0];
        array.process_sample(&[0.5], &[0.5], &[0.0], &mut head_out, &mut array_out);

        // cur=0.5 ; z=2.0 ; post=2.0 ; array_out=3*2+0.1+0.5=6.6 ; head=0.5*2=1.0
        assert_eq!(array_out, vec![6.6]);
        assert_eq!(head_out, vec![1.0]);
    }

    #[test]
    fn head_accumulates_across_stacked_layers() {
        // Two identity-ish layers (conv w=1,b=0 ; ignore cond ; one w=1,b=0 ; relu).
        // rechannel=1 ; head_rechannel=1, no bias ; head_in=silence.
        let l0 = relu_layer(1, vec![1.0], vec![0.0], vec![0.0], vec![1.0], vec![0.0]);
        let l1 = relu_layer(1, vec![1.0], vec![0.0], vec![0.0], vec![1.0], vec![0.0]);
        let mut array = LayerArray::new(1, 1, 1, 1, 1, vec![1.0], vec![l0, l1], vec![1.0], None);

        let mut head_out = vec![0.0];
        let mut array_out = vec![0.0];
        array.process_sample(&[2.0], &[0.0], &[0.0], &mut head_out, &mut array_out);

        // cur=2 ; L0: post=2, head=2, out=2+2=4 ; L1: post=4, head=6, out=4+4=8
        assert_eq!(array_out, vec![8.0]);
        assert_eq!(head_out, vec![6.0]); // 1 * (2 + 4)
    }

    #[test]
    fn incoming_head_is_carried_and_head_bias_applies() {
        // One layer that contributes 0 to head (relu of negative). head_in=[10].
        // head_rechannel weight=2, bias=[1] -> head_out = 2*10 + 1 = 21.
        let layer = relu_layer(1, vec![1.0], vec![0.0], vec![0.0], vec![1.0], vec![0.0]);
        let mut array = LayerArray::new(
            1,
            1,
            1,
            1,
            1,
            vec![1.0],
            vec![layer],
            vec![2.0],
            Some(vec![1.0]),
        );

        let mut head_out = vec![0.0];
        let mut array_out = vec![0.0];
        // input=-5 -> cur=-5 ; z=-5 ; relu=0 ; head stays 10 ; out=0 + (-5) = -5
        array.process_sample(&[-5.0], &[0.0], &[10.0], &mut head_out, &mut array_out);

        assert_eq!(head_out, vec![21.0]);
        assert_eq!(array_out, vec![-5.0]);
    }

    #[test]
    fn channels_and_head_size_reported() {
        let layer = relu_layer(1, vec![1.0], vec![0.0], vec![0.0], vec![1.0], vec![0.0]);
        let array = LayerArray::new(1, 1, 1, 2, 1, vec![1.0], vec![layer], vec![1.0, 1.0], None);
        assert_eq!(array.channels(), 1);
        assert_eq!(array.head_size(), 2);
    }

    #[test]
    fn multitap_head_convolves_accumulated_head_over_time() {
        // channels=1, head_size=1, head_kernel_size=2, no head bias. One identity-ish
        // layer so the head accumulator each sample equals relu(input). The head
        // rechannel is a 2-tap conv over the head-accum time series with weights
        // [w_old, w_cur] = [10, 1]: head_out[t] = 1*acc[t] + 10*acc[t-1].
        // Layer: conv w=1 b=0, ignore cond (mix w=0), 1x1 w=1 b=0, ReLU.
        let layer = relu_layer(1, vec![1.0], vec![0.0], vec![0.0], vec![1.0], vec![0.0]);
        // head_rechannel weights [out=1][in=1][k=2] = [w@oldest_tap, w@current_tap] = [10, 1].
        let mut array =
            LayerArray::new(1, 1, 1, 1, 2, vec![1.0], vec![layer], vec![10.0, 1.0], None);

        let mut head_out = vec![0.0];
        let mut array_out = vec![0.0];
        // t0: input=2 -> acc=2 ; head = 1*2 + 10*0(silence) = 2
        array.process_sample(&[2.0], &[0.0], &[0.0], &mut head_out, &mut array_out);
        assert!((head_out[0] - 2.0).abs() < 1e-6, "t0 head={}", head_out[0]);
        // t1: input=3 -> acc=3 ; head = 1*3 + 10*2 = 23
        array.process_sample(&[3.0], &[0.0], &[0.0], &mut head_out, &mut array_out);
        assert!((head_out[0] - 23.0).abs() < 1e-6, "t1 head={}", head_out[0]);
    }
}
