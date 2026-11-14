//! Feature-wise linear modulation (FiLM).
//!
//! A FiLM block modulates a signal by a per-channel scale (and optionally a
//! shift) computed from a conditioning signal. The scale/shift vector is the
//! output of a `1x1` convolution (`Conv1d` with kernel = 1, dilation = 1, bias)
//! over the conditioning input, producing `(shift ? 2 : 1) * input_dim` rows:
//! the top `input_dim` are the scale, the bottom `input_dim` (when `shift`) are
//! the shift. The modulation is then `out = in * scale (+ shift)`, elementwise.
//!
//! Ported from NeuralAmpModelerCore v0.5.3 `NAM/film.h` (`FiLM::Process` /
//! `Process_`). The conditioning `1x1` is the same `Conv1d` primitive used
//! everywhere else, so grouped FiLM reuses the block-diagonal grouped conv.
//!
//! Like every primitive here it exposes a per-sample path and a planar block
//! path (`[channel * n + t]`) that is bit-equivalent to `n` per-sample calls,
//! plus in-place (`_`-suffixed) variants. All scratch is pre-allocated in
//! [`FiLM::new`]; the `process_*` methods never allocate.

use super::conv::{Conv1d, MAX_BLOCK};

/// A FiLM block with all scratch buffers pre-allocated.
#[derive(Debug, Clone)]
pub(super) struct FiLM {
    /// `condition_dim -> (shift ? 2 : 1) * input_dim`, kernel 1, bias, grouped.
    cond_to_scale_shift: Conv1d,
    /// Width of the modulated signal (the scale width).
    input_dim: usize,
    /// Whether a shift term is added (scale-only when false).
    shift: bool,
    /// `out_rows`-wide scratch holding the conv'd scale (+ shift) for one sample.
    ss: Vec<f32>,
    /// Planar `[out_rows][MAX_BLOCK]` block-path twin of `ss`.
    ss_blk: Vec<f32>,
}

impl FiLM {
    /// Build a FiLM block. See the module-level contract for parameter semantics.
    pub(super) fn new(
        condition_dim: usize,
        input_dim: usize,
        shift: bool,
        groups: usize,
        weights: Vec<f32>,
        bias: Vec<f32>,
    ) -> Self {
        let out_rows = if shift { 2 * input_dim } else { input_dim };
        let cond_to_scale_shift =
            Conv1d::new_grouped(condition_dim, out_rows, 1, 1, groups, weights, Some(bias));
        Self {
            cond_to_scale_shift,
            input_dim,
            shift,
            ss: vec![0.0; out_rows],
            ss_blk: vec![0.0; out_rows * MAX_BLOCK],
        }
    }

    /// Out-of-place: `out = input ⊙ scale (+ shift)`. `input`/`out` are
    /// `input_dim` wide, `condition` is `condition_dim` wide. `out` must not
    /// alias `input`.
    pub(super) fn process_sample(&mut self, input: &[f32], condition: &[f32], out: &mut [f32]) {
        debug_assert_eq!(input.len(), self.input_dim);
        debug_assert_eq!(out.len(), self.input_dim);
        self.cond_to_scale_shift
            .process_sample(condition, &mut self.ss);
        let d = self.input_dim;
        if self.shift {
            for i in 0..d {
                out[i] = input[i] * self.ss[i] + self.ss[d + i];
            }
        } else {
            for i in 0..d {
                out[i] = input[i] * self.ss[i];
            }
        }
    }

    /// In-place: `buf = buf ⊙ scale (+ shift)`. `buf` is `input_dim` wide,
    /// `condition` is `condition_dim` wide.
    pub(super) fn process_sample_(&mut self, buf: &mut [f32], condition: &[f32]) {
        debug_assert_eq!(buf.len(), self.input_dim);
        self.cond_to_scale_shift
            .process_sample(condition, &mut self.ss);
        let d = self.input_dim;
        if self.shift {
            let (scale, sh) = self.ss.split_at(d);
            for ((b, &s), &h) in buf.iter_mut().zip(scale).zip(sh) {
                *b = *b * s + h;
            }
        } else {
            for (b, &s) in buf.iter_mut().zip(&self.ss) {
                *b *= s;
            }
        }
    }

    /// Out-of-place block, planar `[channel * n + t]`. `input`/`out` are
    /// `input_dim * n`, `condition` is `condition_dim * n`, `n <= MAX_BLOCK`.
    /// Bit-equivalent to `n` [`Self::process_sample`] calls. `out` must not alias
    /// `input`.
    pub(super) fn process_block(
        &mut self,
        input: &[f32],
        condition: &[f32],
        out: &mut [f32],
        n: usize,
    ) {
        debug_assert!(n <= MAX_BLOCK);
        debug_assert_eq!(input.len(), self.input_dim * n);
        debug_assert_eq!(out.len(), self.input_dim * n);
        let d = self.input_dim;
        let out_rows = self.ss.len();
        let ss = &mut self.ss_blk[..out_rows * n];
        self.cond_to_scale_shift.process_block(condition, ss, n);
        if self.shift {
            for i in 0..d {
                let (srow, hrow, irow, orow) = (i * n, (d + i) * n, i * n, i * n);
                for t in 0..n {
                    out[orow + t] = input[irow + t] * ss[srow + t] + ss[hrow + t];
                }
            }
        } else {
            for i in 0..d {
                let row = i * n;
                for t in 0..n {
                    out[row + t] = input[row + t] * ss[row + t];
                }
            }
        }
    }

    /// In-place block, planar `[channel * n + t]`. `buf` is `input_dim * n`,
    /// `condition` is `condition_dim * n`, `n <= MAX_BLOCK`.
    pub(super) fn process_block_(&mut self, buf: &mut [f32], condition: &[f32], n: usize) {
        debug_assert!(n <= MAX_BLOCK);
        debug_assert_eq!(buf.len(), self.input_dim * n);
        let d = self.input_dim;
        let out_rows = self.ss.len();
        let ss = &mut self.ss_blk[..out_rows * n];
        self.cond_to_scale_shift.process_block(condition, ss, n);
        if self.shift {
            for i in 0..d {
                let (srow, hrow, brow) = (i * n, (d + i) * n, i * n);
                for t in 0..n {
                    buf[brow + t] = buf[brow + t] * ss[srow + t] + ss[hrow + t];
                }
            }
        } else {
            for i in 0..d {
                let row = i * n;
                for t in 0..n {
                    buf[row + t] *= ss[row + t];
                }
            }
        }
    }

    /// Clear the internal Conv1x1 history. A `1x1` conv has no cross-sample
    /// history, so this does not affect outputs; provided for symmetry with the
    /// other primitives and so `Layer::reset` can call it uniformly.
    pub(super) fn reset(&mut self) {
        self.cond_to_scale_shift.reset();
    }

    /// Rows the internal Conv1x1 emits: `(shift ? 2 : 1) * input_dim`.
    #[cfg(test)]
    pub(super) fn out_rows(&self) -> usize {
        self.ss.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_scale_only_and_scale_shift_dims() {
        // scale-only: out rows = input_dim = 2, cond_dim = 3, groups = 1.
        let f = FiLM::new(3, 2, false, 1, vec![0.0; 2 * 3], vec![0.0; 2]);
        assert_eq!(f.out_rows(), 2);
        // scale+shift: out rows = 2*input_dim = 4.
        let g = FiLM::new(3, 2, true, 1, vec![0.0; 4 * 3], vec![0.0; 4]);
        assert_eq!(g.out_rows(), 4);
    }

    #[test]
    fn process_sample_scale_only() {
        // input_dim=2, condition_dim=1, no shift, groups=1.
        // Conv1x1 W = [out=2][in=1][k=1] = [3.0, 4.0], bias = [1.0, -1.0].
        // cond = [2.0] -> scale = [3*2+1, 4*2-1] = [7.0, 7.0].
        // input = [10.0, 100.0] -> out = [70.0, 700.0].
        let mut f = FiLM::new(1, 2, false, 1, vec![3.0, 4.0], vec![1.0, -1.0]);
        let mut out = vec![0.0; 2];
        f.process_sample(&[10.0, 100.0], &[2.0], &mut out);
        assert_eq!(out, vec![70.0, 700.0]);
    }

    #[test]
    fn process_sample_scale_and_shift() {
        // input_dim=2, condition_dim=1, shift=true -> out_rows=4.
        // Conv1x1 W = [out=4][in=1][k=1] = [3,4,5,6], bias = [0,0,0,0].
        // cond = [2.0] -> ss = [6, 8, 10, 12]; scale = ss[0..2] = [6,8],
        //   shift = ss[2..4] = [10,12].
        // input = [1.0, 1.0] -> out = [1*6+10, 1*8+12] = [16.0, 20.0].
        let mut f = FiLM::new(1, 2, true, 1, vec![3.0, 4.0, 5.0, 6.0], vec![0.0; 4]);
        let mut out = vec![0.0; 2];
        f.process_sample(&[1.0, 1.0], &[2.0], &mut out);
        assert_eq!(out, vec![16.0, 20.0]);
    }

    #[test]
    fn process_sample_in_place_matches_out_of_place() {
        // Same params as scale_and_shift, applied in place.
        // ss = [6,8,10,12]; buf=[1,1] -> [16, 20].
        let mut f = FiLM::new(1, 2, true, 1, vec![3.0, 4.0, 5.0, 6.0], vec![0.0; 4]);
        let mut buf = vec![1.0, 1.0];
        f.process_sample_(&mut buf, &[2.0]);
        assert_eq!(buf, vec![16.0, 20.0]);
    }

    #[test]
    fn process_sample_in_place_scale_only_uses_old_input() {
        // Guard the read-then-write order: scale-only, input_dim=1.
        // W=[2.0] bias=[0], cond=[3] -> scale=6; buf=5 -> 30.
        let mut f = FiLM::new(1, 1, false, 1, vec![2.0], vec![0.0]);
        let mut buf = vec![5.0];
        f.process_sample_(&mut buf, &[3.0]);
        assert_eq!(buf, vec![30.0]);
    }

    #[test]
    fn process_block_scale_and_shift_matches_hand_values() {
        // input_dim=2, condition_dim=1, shift=true, groups=1, n=3.
        // W=[3,4,5,6], bias=[0;4]. Conv1x1 over each frame's cond:
        //   ss[:,t] = [3*c, 4*c, 5*c, 6*c]; scale=ss[0..2], shift=ss[2..4].
        // cond frames (planar, 1 row): [c0,c1,c2] = [1.0, 2.0, 0.5].
        // input planar [2 rows][3 frames]:
        //   row0 = [1, 1, 1], row1 = [2, 2, 2].
        // t0 c=1: out0 = 1*3 + 5 = 8 ; out1 = 2*4 + 6 = 14
        // t1 c=2: out0 = 1*6 + 10 = 16 ; out1 = 2*8 + 12 = 28
        // t2 c=0.5: out0 = 1*1.5 + 2.5 = 4 ; out1 = 2*2 + 3 = 7
        let mut f = FiLM::new(1, 2, true, 1, vec![3.0, 4.0, 5.0, 6.0], vec![0.0; 4]);
        let n = 3;
        let cond = vec![1.0, 2.0, 0.5]; // 1 row * 3
        let input = vec![1.0, 1.0, 1.0, /* row1 */ 2.0, 2.0, 2.0]; // 2 rows * 3
        let mut out = vec![0.0; 2 * n];
        f.process_block(&input, &cond, &mut out, n);
        // planar [row*n + t]: row0 = [8,16,4], row1 = [14,28,7]
        assert_eq!(out, vec![8.0, 16.0, 4.0, 14.0, 28.0, 7.0]);
    }

    #[test]
    fn process_block_in_place_matches_out_of_place() {
        // Same params as process_block_scale_and_shift_matches_hand_values.
        let mut f = FiLM::new(1, 2, true, 1, vec![3.0, 4.0, 5.0, 6.0], vec![0.0; 4]);
        let n = 3;
        let cond = vec![1.0, 2.0, 0.5];
        let mut buf = vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0];
        f.process_block_(&mut buf, &cond, n);
        assert_eq!(buf, vec![8.0, 16.0, 4.0, 14.0, 28.0, 7.0]);
    }

    #[test]
    fn reset_is_a_noop_for_outputs_but_callable() {
        // A Conv1x1 (ring_len 1) carries no cross-sample history, so reset cannot
        // change a result; this test just pins that reset exists and is harmless.
        let mut f = FiLM::new(1, 1, false, 1, vec![2.0], vec![0.0]);
        let mut a = vec![5.0];
        f.process_sample_(&mut a, &[3.0]);
        f.reset();
        let mut b = vec![5.0];
        f.process_sample_(&mut b, &[3.0]);
        assert_eq!(a, b);
    }

    #[test]
    fn process_block_equals_process_sample_loop() {
        // (condition_dim, input_dim, groups) — all divisible by groups where grouped.
        // shift toggled across both values. Conv1x1 internally (kernel=1, dilation=1).
        let cases = [
            (1usize, 1usize, 1usize),
            (3, 2, 1),
            (4, 4, 2), // grouped: cond_dim 4, scale rows = input_dim 4, groups 2
            (4, 2, 2), // grouped, shift doubles out rows to 4 (4 % 2 == 0)
            (6, 3, 3), // grouped depthwise-ish
        ];
        for (cond_dim, input_dim, groups) in cases {
            for shift in [false, true] {
                let out_rows = if shift { 2 * input_dim } else { input_dim };
                // Compact grouped weight count: out_rows * cond_dim * 1 / groups.
                let wlen = out_rows * cond_dim / groups;
                let w: Vec<f32> = (0..wlen)
                    .map(|i| ((i * 37 % 23) as f32 - 11.0) * 0.1)
                    .collect();
                let bias: Vec<f32> = (0..out_rows).map(|o| (o as f32 + 1.0) * 0.05).collect();

                let total = 200usize;
                let inp: Vec<Vec<f32>> = (0..total)
                    .map(|t| {
                        (0..input_dim)
                            .map(|c| ((t * input_dim + c) as f32 * 0.31).sin())
                            .collect()
                    })
                    .collect();
                let cond: Vec<Vec<f32>> = (0..total)
                    .map(|t| {
                        (0..cond_dim)
                            .map(|c| ((t * 5 + c) as f32 * 0.17).cos())
                            .collect()
                    })
                    .collect();

                let mk = || FiLM::new(cond_dim, input_dim, shift, groups, w.clone(), bias.clone());

                // Reference: per-sample, out-of-place.
                let mut a = mk();
                let mut want: Vec<Vec<f32>> = Vec::with_capacity(total);
                let mut o = vec![0.0; input_dim];
                for t in 0..total {
                    a.process_sample(&inp[t], &cond[t], &mut o);
                    want.push(o.clone());
                }

                // Under test: block path, uneven chunks to exercise conv history.
                let mut b = mk();
                let chunks = [50usize, 1, 99, 50];
                let mut t0 = 0;
                for &len in &chunks {
                    let mut bin = vec![0.0; input_dim * len];
                    let mut bcond = vec![0.0; cond_dim * len];
                    for lt in 0..len {
                        for c in 0..input_dim {
                            bin[c * len + lt] = inp[t0 + lt][c];
                        }
                        for c in 0..cond_dim {
                            bcond[c * len + lt] = cond[t0 + lt][c];
                        }
                    }
                    let mut bout = vec![0.0; input_dim * len];
                    b.process_block(&bin, &bcond, &mut bout, len);
                    for lt in 0..len {
                        for c in 0..input_dim {
                            let got = bout[c * len + lt];
                            let exp = want[t0 + lt][c];
                            assert!(
                                (got - exp).abs() < 1e-5,
                                "cond{cond_dim} in{input_dim} g{groups} shift{shift} t{} c{c}: got {got}, want {exp}",
                                t0 + lt
                            );
                        }
                    }
                    t0 += len;
                }
            }
        }
    }
}
