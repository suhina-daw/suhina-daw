//! Optional post-stack head: a chain of `activation → Conv1d` applied to the
//! final layer-array head output after `head_scale` scaling.
//!
//! Ported from NeuralAmpModelerCore `wavenet::detail::Head` (`model.cpp`). Each
//! conv has dilation 1 and a bias; the head's single `activation` is applied
//! before *each* conv (in place over that conv's input), then the conv runs. The
//! last conv's output is the model output (no re-scaling). Mirrors the planar
//! `[ch][t]` block convention used by `Conv1d`/`Layer`/`LayerArray`.

use super::activation::Activation;
use super::conv::{Conv1d, MAX_BLOCK};

/// A post-stack head: `(activation, conv)` pairs run in chain order.
#[derive(Debug, Clone)]
pub(super) struct PostStackHead {
    convs: Vec<(Activation, Conv1d)>,
    in_channels: usize,
    out_channels: usize,
    /// Planar `[max_width][MAX_BLOCK]` ping-pong scratch holding each conv's output
    /// so the next conv reads it; sized to the widest conv output. Pre-allocated.
    scratch_a: Vec<f32>,
    scratch_b: Vec<f32>,
    /// `in_channels`-wide per-sample input row, so `process_sample` can delegate to
    /// `process_block` with `n = 1` without allocating.
    sample_in: Vec<f32>,
}

impl PostStackHead {
    pub(super) fn new(
        convs: Vec<(Activation, Conv1d)>,
        in_channels: usize,
        out_channels: usize,
    ) -> Self {
        // Widest conv output across the chain (interior convs emit `channels`, the
        // last emits `out_channels`); size scratch to the max so ping-pong fits.
        let max_w = convs
            .iter()
            .map(|(_, c)| c.out_ch())
            .max()
            .unwrap_or(out_channels)
            .max(in_channels)
            .max(1);
        Self {
            convs,
            in_channels,
            out_channels,
            scratch_a: vec![0.0; max_w * MAX_BLOCK],
            scratch_b: vec![0.0; max_w * MAX_BLOCK],
            sample_in: vec![0.0; in_channels.max(1)],
        }
    }

    pub(super) fn in_channels(&self) -> usize {
        self.in_channels
    }

    pub(super) fn out_channels(&self) -> usize {
        self.out_channels
    }

    /// Receptive field: `1 + Σ(kernel − 1)` over the chain (NAMCore
    /// `Head::receptive_field`). The top-level [`WaveNet`](crate::WaveNet) receptive
    /// field is computed from the config directly, so this is only a cross-check.
    #[cfg(test)]
    pub(super) fn receptive_field(&self) -> usize {
        let mut rf = 1;
        for (_, c) in &self.convs {
            rf += c.kernel() - 1;
        }
        rf
    }

    /// Per-sample. `work` is the (already `head_scale`-scaled) head input,
    /// `in_channels` wide. Returns the last conv's output, `out_channels` wide.
    pub(super) fn process_sample(&mut self, work: &[f32]) -> &[f32] {
        debug_assert_eq!(work.len(), self.in_channels);
        self.sample_in[..self.in_channels].copy_from_slice(work);
        // Move the input row out so we can borrow `self` mutably in process_block,
        // then put it back (no allocation; sample_in keeps its capacity).
        let mut row = std::mem::take(&mut self.sample_in);
        self.process_block(&mut row, 1);
        self.sample_in = row;
        // The block output lives in scratch_a/scratch_b; recompute which holds it.
        let out_ch = self.out_channels;
        if self.convs.len() % 2 == 1 {
            &self.scratch_a[..out_ch]
        } else {
            &self.scratch_b[..out_ch]
        }
    }

    /// Planar block twin. `work` is `[in_channels][n]` (already `head_scale`-scaled,
    /// may be modified in place by the first activation); returns the last conv's
    /// output as `[out_channels][n]`. `n ≤ MAX_BLOCK`. Allocation-free.
    pub(super) fn process_block(&mut self, work: &mut [f32], n: usize) -> &[f32] {
        debug_assert!(n <= MAX_BLOCK);
        let nconvs = self.convs.len();
        for i in 0..nconvs {
            let (act, conv) = &mut self.convs[i];
            let in_ch = conv.in_ch();
            let out_ch = conv.out_ch();
            if i == 0 {
                for v in work[..in_ch * n].iter_mut() {
                    *v = act.apply(*v);
                }
                conv.process_block(&work[..in_ch * n], &mut self.scratch_a[..out_ch * n], n);
            } else {
                // Conv (i-1)'s output is in scratch_a when i is odd, scratch_b when
                // even. Activate it in place, then conv into the other buffer.
                let (src, dst): (&mut [f32], &mut [f32]) = if i % 2 == 1 {
                    let (a, b) = (&mut self.scratch_a, &mut self.scratch_b);
                    (a.as_mut_slice(), b.as_mut_slice())
                } else {
                    let (a, b) = (&mut self.scratch_b, &mut self.scratch_a);
                    (a.as_mut_slice(), b.as_mut_slice())
                };
                for v in src[..in_ch * n].iter_mut() {
                    *v = act.apply(*v);
                }
                conv.process_block(&src[..in_ch * n], &mut dst[..out_ch * n], n);
            }
        }
        // The last conv's output is in scratch_a if nconvs is odd, else scratch_b.
        let out_ch = self.out_channels;
        if nconvs % 2 == 1 {
            &self.scratch_a[..out_ch * n]
        } else {
            &self.scratch_b[..out_ch * n]
        }
    }

    pub(super) fn reset(&mut self) {
        for (_, c) in &mut self.convs {
            c.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wavenet::activation::Activation;

    // One-conv head: in=1, out=1, kernel=1, ReLU. conv weight=3, bias=0.5.
    // process_sample over scaled input s: out = 3*relu(s) + 0.5.
    #[test]
    fn single_conv_head_applies_activation_then_conv() {
        let convs = vec![(
            Activation::Relu,
            Conv1d::new(1, 1, 1, 1, vec![3.0], Some(vec![0.5])),
        )];
        let mut head = PostStackHead::new(convs, 1, 1);
        // s = 2.0 -> relu(2)=2 -> 3*2+0.5 = 6.5
        let work = [2.0_f32];
        let out = head.process_sample(&work);
        assert!((out[0] - 6.5).abs() < 1e-6, "got {}", out[0]);
        // s = -1.0 -> relu(-1)=0 -> 3*0+0.5 = 0.5
        let work2 = [-1.0_f32];
        let out2 = head.process_sample(&work2);
        assert!((out2[0] - 0.5).abs() < 1e-6, "got {}", out2[0]);
        assert_eq!(head.out_channels(), 1);
    }

    // Two-conv head: in=1 -> channels=1 -> out=1, kernels [1,1], ReLU each.
    // conv0 w=2 b=0 ; conv1 w=1 b=1. s -> relu(s) -> conv0=2*relu(s)
    //   -> relu(2*relu(s)) -> conv1 = 1*relu(2*relu(s)) + 1.
    #[test]
    fn two_conv_head_chains_activation_conv_activation_conv() {
        let convs = vec![
            (
                Activation::Relu,
                Conv1d::new(1, 1, 1, 1, vec![2.0], Some(vec![0.0])),
            ),
            (
                Activation::Relu,
                Conv1d::new(1, 1, 1, 1, vec![1.0], Some(vec![1.0])),
            ),
        ];
        let mut head = PostStackHead::new(convs, 1, 1);
        // s=3 -> relu=3 -> conv0=6 -> relu=6 -> conv1=6+1=7
        let work = [3.0_f32];
        let out = head.process_sample(&work);
        assert!((out[0] - 7.0).abs() < 1e-6, "got {}", out[0]);
    }

    #[test]
    fn receptive_field_sums_kernel_minus_one() {
        // kernels [16, 1] -> rf = 1 + (16-1) + (1-1) = 16.
        let convs = vec![
            (
                Activation::Relu,
                Conv1d::new(1, 1, 16, 1, vec![0.0; 16], Some(vec![0.0])),
            ),
            (
                Activation::Relu,
                Conv1d::new(1, 1, 1, 1, vec![0.0], Some(vec![0.0])),
            ),
        ];
        let head = PostStackHead::new(convs, 1, 1);
        assert_eq!(head.receptive_field(), 16);
    }
}
