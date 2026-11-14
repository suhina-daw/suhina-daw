//! Causal, dilated 1-D convolution with a pre-allocated ring buffer.
//!
//! This is the one stateful primitive in the WaveNet forward pass. Weight layout
//! matches NAM's `export_weights` (PyTorch `Conv1d`): `[out_ch][in_ch][kernel]`
//! row-major, followed by an optional `[out_ch]` bias.
//!
//! A 1x1 convolution (`kernel = 1`, `dilation = 1`) degenerates to a per-sample
//! matrix multiply; the same type is reused for the rechannel / input-mixer /
//! 1x1 / head-rechannel layers.
//!
//! Two equivalent entry points share one streaming history (the ring): the
//! per-sample [`Conv1d::process_sample`], and the block [`Conv1d::process_block`],
//! which stages `[history ++ block]` into a contiguous planar scratch so the hot
//! loop runs sample-inner over a stationary weight row (cache-friendly, and the
//! compiler can autovectorize it). The block path is the lever behind
//! `WaveNet::process_buffer`.

/// Largest block the planar kernel processes in one call; `process_buffer` chunks
/// longer inputs to this. Scratch is sized to it up front (off the audio thread).
pub(super) const MAX_BLOCK: usize = 1024;

/// A dilated causal convolution that processes one sample at a time, keeping the
/// receptive-field history in a pre-allocated ring buffer (no allocation on the
/// audio thread).
#[derive(Debug, Clone)]
pub(super) struct Conv1d {
    in_ch: usize,
    out_ch: usize,
    kernel: usize,
    dilation: usize,
    /// `out_ch / groups`.
    out_per_group: usize,
    /// `in_ch / groups`.
    in_per_group: usize,
    /// `[out_ch][in_ch][kernel]`, row-major.
    weights: Vec<f32>,
    bias: Option<Vec<f32>>,
    /// Ring of the most recent `ring_len` input columns, `in_ch` values each.
    ring: Vec<f32>,
    ring_len: usize,
    /// Index of the column written most recently.
    pos: usize,
    /// Planar `[in_ch][hist_len + MAX_BLOCK]` staging for the block path, where
    /// `hist_len = ring_len - 1`. Pre-allocated; the inner loop reads it contiguously
    /// over time. `staged_stride` is the per-channel row length.
    staged: Vec<f32>,
    staged_stride: usize,
}

impl Conv1d {
    /// Dense (ungrouped) convolution — `groups = 1`.
    pub(super) fn new(
        in_ch: usize,
        out_ch: usize,
        kernel: usize,
        dilation: usize,
        weights: Vec<f32>,
        bias: Option<Vec<f32>>,
    ) -> Self {
        Self::new_grouped(in_ch, out_ch, kernel, dilation, 1, weights, bias)
    }

    /// Grouped convolution. Weights are **compact** (no off-block zeros), laid out in
    /// NAMCore order: group-major, then out-per-group, then in-per-group, then tap —
    /// i.e. `idx(g,i,j,k) = (((g*out_per_group + i)*in_per_group) + j)*kernel + k`.
    /// `groups = 1` reduces this to the dense `[out][in][kernel]` layout exactly.
    pub(super) fn new_grouped(
        in_ch: usize,
        out_ch: usize,
        kernel: usize,
        dilation: usize,
        groups: usize,
        weights: Vec<f32>,
        bias: Option<Vec<f32>>,
    ) -> Self {
        assert!(groups >= 1, "conv groups must be >= 1");
        assert_eq!(in_ch % groups, 0, "in_ch must be divisible by groups");
        assert_eq!(out_ch % groups, 0, "out_ch must be divisible by groups");
        assert_eq!(
            weights.len(),
            out_ch * in_ch * kernel / groups,
            "grouped conv weight count"
        );
        if let Some(b) = &bias {
            assert_eq!(b.len(), out_ch, "conv bias count");
        }
        let ring_len = (kernel - 1) * dilation + 1;
        let staged_stride = (ring_len - 1) + MAX_BLOCK;
        Self {
            in_ch,
            out_ch,
            kernel,
            dilation,
            out_per_group: out_ch / groups,
            in_per_group: in_ch / groups,
            weights,
            bias,
            ring: vec![0.0; in_ch * ring_len],
            ring_len,
            pos: ring_len - 1,
            staged: vec![0.0; in_ch * staged_stride],
            staged_stride,
        }
    }

    // Geometry accessors consumed by the post-stack head (`head.rs`).
    pub(super) fn out_ch(&self) -> usize {
        self.out_ch
    }

    pub(super) fn in_ch(&self) -> usize {
        self.in_ch
    }

    #[cfg(test)]
    pub(super) fn kernel(&self) -> usize {
        self.kernel
    }

    /// Push one input column (`in_ch` values) and write the convolution result
    /// (`out_ch` values) into `out`.
    pub(super) fn process_sample(&mut self, input: &[f32], out: &mut [f32]) {
        debug_assert_eq!(input.len(), self.in_ch);
        debug_assert_eq!(out.len(), self.out_ch);

        // Advance and store the newest column.
        self.pos = (self.pos + 1) % self.ring_len;
        let base = self.pos * self.in_ch;
        self.ring[base..base + self.in_ch].copy_from_slice(input);

        let opg = self.out_per_group;
        let ipg = self.in_per_group;
        for o in 0..self.out_ch {
            let mut acc = self.bias.as_ref().map_or(0.0, |b| b[o]);
            // `in_base` = first input channel of this output's group; `wo` = start of
            // output o's rows in the compact buffer (same offset the block path uses).
            let g = o / opg;
            let in_base = g * ipg;
            let wo = o * ipg * self.kernel;
            for k in 0..self.kernel {
                let back = (self.kernel - 1 - k) * self.dilation;
                let col = (self.pos + self.ring_len - back) % self.ring_len;
                let rbase = col * self.in_ch;
                for jl in 0..ipg {
                    // jl is the local (in-group) input index; jl*kernel+k walks taps.
                    acc +=
                        self.weights[wo + jl * self.kernel + k] * self.ring[rbase + in_base + jl];
                }
            }
            out[o] = acc;
        }
    }

    /// Process `n` input columns at once, in **planar** layout: `block_in` is
    /// `in_ch * n` laid out `[channel * n + t]`, and `block_out` is `out_ch * n` the
    /// same way. Bit-for-bit equivalent to `n` calls of [`Self::process_sample`] and
    /// leaves the streaming history in the identical state, so the two entry points
    /// are freely interchangeable across calls.
    ///
    /// `n` must be `<= MAX_BLOCK`; callers chunk longer runs. Allocation-free.
    pub(super) fn process_block(&mut self, block_in: &[f32], block_out: &mut [f32], n: usize) {
        debug_assert!(n <= MAX_BLOCK);
        debug_assert_eq!(block_in.len(), self.in_ch * n);
        debug_assert_eq!(block_out.len(), self.out_ch * n);
        if n == 0 {
            return;
        }

        let hist_len = self.ring_len - 1;
        let s = self.staged_stride;

        // Stage the history tail (chronological: oldest at time 0, newest at
        // hist_len-1) followed by this block, one contiguous row per input channel.
        for j in 0..self.in_ch {
            let row = j * s;
            for h in 0..hist_len {
                let col = (self.pos + self.ring_len - (hist_len - 1) + h) % self.ring_len;
                self.staged[row + h] = self.ring[col * self.in_ch + j];
            }
            let src = &block_in[j * n..j * n + n];
            self.staged[row + hist_len..row + hist_len + n].copy_from_slice(src);
        }

        // Compute. Weight-stationary: for each (out channel, tap, in channel) the
        // inner loop streams contiguously over time in both `staged` and `block_out`.
        for o in 0..self.out_ch {
            let b = self.bias.as_ref().map_or(0.0, |bias| bias[o]);
            block_out[o * n..o * n + n].fill(b);
        }
        let opg = self.out_per_group;
        let ipg = self.in_per_group;
        for o in 0..self.out_ch {
            let g = o / opg;
            let in_base = g * ipg;
            let wo = o * ipg * self.kernel; // compact: rows are opg-by-ipg per group, contiguous in o
            let out = &mut block_out[o * n..o * n + n];
            for k in 0..self.kernel {
                let back = (self.kernel - 1 - k) * self.dilation;
                for jl in 0..ipg {
                    let w = self.weights[wo + jl * self.kernel + k];
                    // staged time for output t is `hist_len + t`; tap k reads `- back`.
                    let base = (in_base + jl) * s + hist_len - back;
                    let src = &self.staged[base..base + n];
                    for t in 0..n {
                        out[t] += w * src[t];
                    }
                }
            }
        }

        // Advance the ring by pushing every block column (state update only, so a
        // later `process_sample`/`process_block` continues seamlessly).
        for t in 0..n {
            self.pos = (self.pos + 1) % self.ring_len;
            let base = self.pos * self.in_ch;
            for j in 0..self.in_ch {
                self.ring[base + j] = block_in[j * n + t];
            }
        }
    }

    /// Clear the history to silence.
    pub(super) fn reset(&mut self) {
        self.ring.iter_mut().for_each(|x| *x = 0.0);
        self.pos = self.ring_len - 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(conv: &mut Conv1d, xs: &[&[f32]]) -> Vec<Vec<f32>> {
        let mut out = vec![0.0; conv.out_ch()];
        xs.iter()
            .map(|x| {
                conv.process_sample(x, &mut out);
                out.clone()
            })
            .collect()
    }

    /// Expected values taken verbatim from NeuralAmpModelerCore's
    /// `tools/test/test_conv1d.cpp` (MIT) — an oracle independent of our numpy
    /// fixture generator.
    #[test]
    fn matches_namcore_conv1d_vectors() {
        // test_process_basic: weights {1,2}, input [1,2,3,4] -> [2,5,8,11].
        let mut basic = Conv1d::new(1, 1, 2, 1, vec![1.0, 2.0], None);
        assert_eq!(
            run(&mut basic, &[&[1.0], &[2.0], &[3.0], &[4.0]]),
            vec![vec![2.0], vec![5.0], vec![8.0], vec![11.0]]
        );

        // test_process_with_bias: weights {1,0}, bias 5, input [2,3] -> [5,7].
        let mut biased = Conv1d::new(1, 1, 2, 1, vec![1.0, 0.0], Some(vec![5.0]));
        assert_eq!(
            run(&mut biased, &[&[2.0], &[3.0]]),
            vec![vec![5.0], vec![7.0]]
        );

        // test_process_dilation: weights {1,2}, dilation 2, [1,2,3,4] -> [2,4,7,10].
        let mut dil = Conv1d::new(1, 1, 2, 2, vec![1.0, 2.0], None);
        assert_eq!(
            run(&mut dil, &[&[1.0], &[2.0], &[3.0], &[4.0]]),
            vec![vec![2.0], vec![4.0], vec![7.0], vec![10.0]]
        );
    }

    #[test]
    fn kernel2_dilation1_single_channel() {
        // out[t] = a*x[t-1] + b*x[t] + c, history starts at silence.
        // weights [out][in][k] = [a (oldest tap), b (current tap)].
        let mut conv = Conv1d::new(1, 1, 2, 1, vec![0.5, 2.0], Some(vec![0.1]));
        let got = run(&mut conv, &[&[1.0], &[2.0], &[3.0]]);
        assert_eq!(got, vec![vec![2.1], vec![4.6], vec![7.1]]);
    }

    #[test]
    fn kernel2_dilation2_skips_a_sample() {
        // out[t] = 0.5*x[t-2] + 2*x[t], no bias.
        let mut conv = Conv1d::new(1, 1, 2, 2, vec![0.5, 2.0], None);
        let got = run(&mut conv, &[&[1.0], &[2.0], &[3.0], &[4.0]]);
        assert_eq!(got, vec![vec![2.0], vec![4.0], vec![6.5], vec![9.0]]);
    }

    #[test]
    fn one_by_one_is_a_matmul() {
        // kernel=1, dilation=1: out[o] = sum_j W[o][j]*x[j] + bias[o].
        // W (2x2) = [[1,2],[3,4]], bias=[10,20].
        let mut conv = Conv1d::new(2, 2, 1, 1, vec![1.0, 2.0, 3.0, 4.0], Some(vec![10.0, 20.0]));
        let got = run(&mut conv, &[&[1.0, 1.0], &[2.0, 0.0]]);
        // x=[1,1]: [1+2+10, 3+4+20] = [13,27]; x=[2,0]: [2+10, 6+20]=[12,26]
        assert_eq!(got, vec![vec![13.0, 27.0], vec![12.0, 26.0]]);
    }

    /// Independent naive causal convolution oracle (single channel).
    fn naive(xs: &[f32], w: &[f32], dilation: usize) -> Vec<f32> {
        let k = w.len();
        (0..xs.len())
            .map(|t| {
                (0..k)
                    .map(|tap| {
                        let back = (k - 1 - tap) * dilation;
                        let x = if t >= back { xs[t - back] } else { 0.0 };
                        w[tap] * x
                    })
                    .sum()
            })
            .collect()
    }

    #[test]
    fn ring_buffer_matches_naive_over_long_signal() {
        // kernel=3, dilation=4 -> ring_len=9; feed 32 samples to wrap repeatedly.
        let w = vec![0.3, -1.1, 2.0];
        let mut conv = Conv1d::new(1, 1, 3, 4, w.clone(), None);
        let xs: Vec<f32> = (0..32).map(|i| (i as f32 * 0.37).sin()).collect();
        let got: Vec<f32> = xs
            .iter()
            .map(|&x| {
                let mut o = [0.0];
                conv.process_sample(&[x], &mut o);
                o[0]
            })
            .collect();
        let want = naive(&xs, &w, 4);
        for (g, e) in got.iter().zip(&want) {
            assert!((g - e).abs() < 1e-6, "got {g}, want {e}");
        }
    }

    /// The block path must reproduce the per-sample path exactly, including history
    /// carried across successive (differently sized) blocks. Planar in/out.
    #[test]
    fn process_block_equals_process_sample_loop() {
        // A few shapes: multi-channel, kernels 1..3, dilations that wrap the ring.
        let cases = [
            (1_usize, 1_usize, 1_usize, 1_usize),
            (2, 3, 1, 1),
            (3, 2, 2, 1),
            (2, 2, 3, 4),
            (4, 5, 2, 7),
        ];
        for (in_ch, out_ch, kernel, dilation) in cases {
            let wlen = out_ch * in_ch * kernel;
            // Deterministic pseudo-random weights/bias/input.
            let w: Vec<f32> = (0..wlen)
                .map(|i| ((i * 37 % 23) as f32 - 11.0) * 0.1)
                .collect();
            let bias: Vec<f32> = (0..out_ch).map(|o| (o as f32 + 1.0) * 0.05).collect();
            let total = 200_usize;
            let xs: Vec<Vec<f32>> = (0..total)
                .map(|t| {
                    (0..in_ch)
                        .map(|j| ((t * in_ch + j) as f32 * 0.31).sin())
                        .collect()
                })
                .collect();

            // Reference: per-sample.
            let mut a = Conv1d::new(
                in_ch,
                out_ch,
                kernel,
                dilation,
                w.clone(),
                Some(bias.clone()),
            );
            let mut want = vec![0.0; out_ch];
            let want_all: Vec<Vec<f32>> = xs
                .iter()
                .map(|x| {
                    a.process_sample(x, &mut want);
                    want.clone()
                })
                .collect();

            // Under test: block path, split into uneven chunks to exercise history.
            let mut b = Conv1d::new(in_ch, out_ch, kernel, dilation, w, Some(bias));
            let chunks = [50usize, 1, 99, 50];
            let mut t0 = 0;
            for &len in &chunks {
                // Planar block_in: [channel][time].
                let mut bin = vec![0.0; in_ch * len];
                for (lt, x) in xs[t0..t0 + len].iter().enumerate() {
                    for (j, &v) in x.iter().enumerate() {
                        bin[j * len + lt] = v;
                    }
                }
                let mut bout = vec![0.0; out_ch * len];
                b.process_block(&bin, &mut bout, len);
                for lt in 0..len {
                    for o in 0..out_ch {
                        let got = bout[o * len + lt];
                        let exp = want_all[t0 + lt][o];
                        assert!(
                            (got - exp).abs() < 1e-5,
                            "shape {in_ch}x{out_ch} k{kernel} d{dilation} t{} o{o}: got {got}, want {exp}",
                            t0 + lt
                        );
                    }
                }
                t0 += len;
            }
        }
    }

    #[test]
    fn grouped_constructor_accepts_compact_weight_count() {
        // out=4, in=4, kernel=2, groups=2 -> out_per_group=2, in_per_group=2.
        // Compact weight count = 4*4*2 / 2 = 16. Plus 4 bias entries.
        let w: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let conv = Conv1d::new_grouped(4, 4, 2, 1, 2, w, Some(vec![0.0; 4]));
        assert_eq!(conv.out_ch(), 4);
    }

    #[test]
    fn grouped_process_block_equals_process_sample_loop() {
        // (in_ch, out_ch, kernel, dilation, groups) — all divisible by groups.
        let cases = [
            (4_usize, 4_usize, 1_usize, 1_usize, 2_usize),
            (4, 4, 2, 2, 2), // grouped, dilated
            (4, 4, 3, 1, 4), // depthwise (groups == in == out)
            (6, 4, 2, 1, 2), // out_per_group=2, in_per_group=3
            (4, 6, 2, 3, 2), // out_per_group=3, in_per_group=2, dilation wraps
        ];
        for (in_ch, out_ch, kernel, dilation, groups) in cases {
            let wlen = out_ch * in_ch * kernel / groups;
            let w: Vec<f32> = (0..wlen)
                .map(|i| ((i * 37 % 23) as f32 - 11.0) * 0.1)
                .collect();
            let bias: Vec<f32> = (0..out_ch).map(|o| (o as f32 + 1.0) * 0.05).collect();
            let total = 200_usize;
            let xs: Vec<Vec<f32>> = (0..total)
                .map(|t| {
                    (0..in_ch)
                        .map(|j| ((t * in_ch + j) as f32 * 0.31).sin())
                        .collect()
                })
                .collect();

            // Reference: per-sample.
            let mut a = Conv1d::new_grouped(
                in_ch,
                out_ch,
                kernel,
                dilation,
                groups,
                w.clone(),
                Some(bias.clone()),
            );
            let mut want = vec![0.0; out_ch];
            let want_all: Vec<Vec<f32>> = xs
                .iter()
                .map(|x| {
                    a.process_sample(x, &mut want);
                    want.clone()
                })
                .collect();

            // Under test: block path, uneven chunks to exercise history.
            let mut b = Conv1d::new_grouped(in_ch, out_ch, kernel, dilation, groups, w, Some(bias));
            let chunks = [50usize, 1, 99, 50];
            let mut t0 = 0;
            for &len in &chunks {
                let mut bin = vec![0.0; in_ch * len];
                for (lt, x) in xs[t0..t0 + len].iter().enumerate() {
                    for (j, &v) in x.iter().enumerate() {
                        bin[j * len + lt] = v;
                    }
                }
                let mut bout = vec![0.0; out_ch * len];
                b.process_block(&bin, &mut bout, len);
                for lt in 0..len {
                    for o in 0..out_ch {
                        let got = bout[o * len + lt];
                        let exp = want_all[t0 + lt][o];
                        assert!(
                            (got - exp).abs() < 1e-5,
                            "shape {in_ch}x{out_ch} k{kernel} d{dilation} g{groups} t{} o{o}: got {got}, want {exp}",
                            t0 + lt
                        );
                    }
                }
                t0 += len;
            }
        }
    }

    #[test]
    fn grouped_1x1_is_block_diagonal_matmul() {
        // in=4, out=4, kernel=1, groups=2. out_per_group = in_per_group = 2.
        // Block-diagonal: out[0,1] read in[0,1]; out[2,3] read in[2,3].
        // Compact weights (group-major, out_pg, in_pg, tap):
        //   g0: W[[1,2],[3,4]]   g1: W[[5,6],[7,8]]
        let w = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let bias = vec![10.0, 20.0, 30.0, 40.0];
        let mut conv = Conv1d::new_grouped(4, 4, 1, 1, 2, w, Some(bias));
        let mut out = vec![0.0; 4];
        conv.process_sample(&[1.0, 1.0, 1.0, 1.0], &mut out);
        // out0 = 1*1 + 2*1 + 10 = 13 ; out1 = 3*1 + 4*1 + 20 = 27
        // out2 = 5*1 + 6*1 + 30 = 41 ; out3 = 7*1 + 8*1 + 40 = 55
        assert_eq!(out, vec![13.0, 27.0, 41.0, 55.0]);

        // Cross-group isolation: perturbing in[2,3] must not change out[0,1].
        let mut out2 = vec![0.0; 4];
        conv.reset();
        conv.process_sample(&[1.0, 1.0, 9.0, -9.0], &mut out2);
        assert_eq!(out2[0], 13.0);
        assert_eq!(out2[1], 27.0);
    }

    #[test]
    fn grouped_depthwise_kernel2() {
        // Depthwise: in=out=groups=2, kernel=2 -> out_per_group=in_per_group=1.
        // Channel 0 weights {1,2}; channel 1 weights {10,20}. No bias.
        // Compact order: g0(i0,j0): taps {1,2}; g1(i0,j0): taps {10,20}.
        let w = vec![1.0, 2.0, 10.0, 20.0];
        let mut conv = Conv1d::new_grouped(2, 2, 2, 1, 2, w, None);
        let mut out = vec![0.0; 2];
        // t=0, history silent: ch0 = 2*1 = 2 ; ch1 = 20*3 = 60
        conv.process_sample(&[1.0, 3.0], &mut out);
        assert_eq!(out, vec![2.0, 60.0]);
        // t=1: ch0 = 1*1 + 2*5 = 11 ; ch1 = 10*3 + 20*7 = 170
        conv.process_sample(&[5.0, 7.0], &mut out);
        assert_eq!(out, vec![11.0, 170.0]);
    }

    #[test]
    fn reset_clears_history() {
        let mut conv = Conv1d::new(1, 1, 2, 1, vec![0.5, 2.0], None);
        let _ = run(&mut conv, &[&[1.0], &[2.0]]);
        conv.reset();
        // After reset, history is silence again: out[0] = 2*1 = 2.0
        let got = run(&mut conv, &[&[1.0]]);
        assert_eq!(got, vec![vec![2.0]]);
    }
}
