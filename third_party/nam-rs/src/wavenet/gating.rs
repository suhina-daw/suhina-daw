//! Configurable activation gating over a layer's pre-activation buffer.
//!
//! Generalizes NeuralAmpModelerCore `gating_activations::{GatingActivation,
//! BlendingActivation}` (and the plain ungated activation) into one value
//! parameterized by a [`GatingMode`], a primary activation, and a secondary
//! activation. The classic A1 path is `Gated` with `primary = Tanh`,
//! `secondary = Sigmoid`.
//!
//! Layout convention (parity-critical): for `bottleneck = bn` output rows, the
//! primary operand of channel `c` is `z[c]` and the secondary operand is
//! `z[c + bn]`. `None` needs `bn` input rows; `Gated`/`Blended` need `2·bn`.

use super::activation::Activation;
use crate::model::GatingMode;

/// Configurable activation gating over a layer's pre-activation buffer `z`.
#[derive(Debug, Clone)]
pub(super) struct Gating {
    mode: GatingMode,
    primary: Activation,
    secondary: Activation,
    bottleneck: usize,
}

impl Gating {
    /// Build from a resolved mode + activations and the bottleneck width `bn`
    /// (the number of output rows). `secondary` is ignored when `mode == None`.
    pub(super) fn new(
        mode: GatingMode,
        primary: Activation,
        secondary: Activation,
        bottleneck: usize,
    ) -> Self {
        Self {
            mode,
            primary,
            secondary,
            bottleneck,
        }
    }

    /// The configured gating mode (so callers can branch on NONE/GATED/BLENDED).
    pub(super) fn mode(&self) -> GatingMode {
        self.mode
    }

    /// Number of input rows `z` must supply: `None ⇒ bn`, otherwise `2·bn`.
    pub(super) fn input_rows(&self) -> usize {
        match self.mode {
            GatingMode::None => self.bottleneck,
            GatingMode::Gated | GatingMode::Blended => 2 * self.bottleneck,
        }
    }

    /// Number of output rows written: always `bn`. (The Layer reads `bottleneck`
    /// directly; this accessor documents the contract and is exercised by tests.)
    #[cfg(test)]
    pub(super) fn output_rows(&self) -> usize {
        self.bottleneck
    }

    /// Per-sample gating. `z` has [`Self::input_rows`] elements; `out` has
    /// `bottleneck`. See the module docs for the formulas.
    pub(super) fn process_sample(&self, z: &[f32], out: &mut [f32]) {
        let bn = self.bottleneck;
        match self.mode {
            GatingMode::None => {
                for c in 0..bn {
                    out[c] = self.primary.apply(z[c]);
                }
            }
            GatingMode::Gated => {
                for c in 0..bn {
                    let v = self.primary.apply(z[c]);
                    let s = self.secondary.apply(z[c + bn]);
                    out[c] = v * s;
                }
            }
            GatingMode::Blended => {
                for c in 0..bn {
                    let alpha = self.secondary.apply(z[c + bn]);
                    let v = self.primary.apply(z[c]);
                    out[c] = alpha * v + (1.0 - alpha) * z[c];
                }
            }
        }
    }

    /// Planar block twin of [`Self::process_sample`]. `z` is `input_rows()·n`,
    /// `out` is `bottleneck·n`, `[row][t]`, `n ≤ MAX_BLOCK`. Allocation-free.
    pub(super) fn process_block(&self, z: &[f32], out: &mut [f32], n: usize) {
        let bn = self.bottleneck;
        match self.mode {
            GatingMode::None => {
                for c in 0..bn {
                    let base = c * n;
                    for t in 0..n {
                        out[base + t] = self.primary.apply(z[base + t]);
                    }
                }
            }
            GatingMode::Gated => {
                for c in 0..bn {
                    let (vrow, grow, orow) = (c * n, (c + bn) * n, c * n);
                    for t in 0..n {
                        let v = self.primary.apply(z[vrow + t]);
                        let s = self.secondary.apply(z[grow + t]);
                        out[orow + t] = v * s;
                    }
                }
            }
            GatingMode::Blended => {
                for c in 0..bn {
                    let (vrow, grow, orow) = (c * n, (c + bn) * n, c * n);
                    for t in 0..n {
                        let raw = z[vrow + t];
                        let alpha = self.secondary.apply(z[grow + t]);
                        let v = self.primary.apply(raw);
                        out[orow + t] = alpha * v + (1.0 - alpha) * raw;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_counts_follow_mode() {
        let g_none = Gating::new(GatingMode::None, Activation::Tanh, Activation::Sigmoid, 4);
        assert_eq!(g_none.input_rows(), 4);
        assert_eq!(g_none.output_rows(), 4);

        let g_gated = Gating::new(GatingMode::Gated, Activation::Tanh, Activation::Sigmoid, 4);
        assert_eq!(g_gated.input_rows(), 8);
        assert_eq!(g_gated.output_rows(), 4);

        let g_blend = Gating::new(
            GatingMode::Blended,
            Activation::Tanh,
            Activation::Sigmoid,
            3,
        );
        assert_eq!(g_blend.input_rows(), 6);
        assert_eq!(g_blend.output_rows(), 3);
    }

    #[test]
    fn process_sample_none_applies_primary_over_bn_rows() {
        // None over bn=2 rows with ReLU primary. z has bn rows.
        let g = Gating::new(GatingMode::None, Activation::Relu, Activation::Sigmoid, 2);
        let mut out = [0.0_f32; 2];
        g.process_sample(&[2.0, -3.0], &mut out);
        // relu(2)=2, relu(-3)=0
        assert_eq!(out, [2.0, 0.0]);
    }

    #[test]
    fn process_sample_gated_multiplies_primary_by_secondary() {
        // bn=1, Gated, ReLU primary, Sigmoid secondary. z = [value, gate] = [2.0, 0.0].
        // out[0] = relu(2) * sigmoid(0) = 2 * 0.5 = 1.0
        let g = Gating::new(GatingMode::Gated, Activation::Relu, Activation::Sigmoid, 1);
        let mut out = [0.0_f32];
        g.process_sample(&[2.0, 0.0], &mut out);
        assert_eq!(out, [1.0]);
    }

    #[test]
    fn process_sample_gated_tanh_sigmoid_reproduces_a1() {
        // The classic A1 gate: primary=Tanh, secondary=Sigmoid, bn=2.
        // z = [v0, v1, g0, g1] (primary first half, secondary second half).
        let g = Gating::new(GatingMode::Gated, Activation::Tanh, Activation::Sigmoid, 2);
        let z = [0.5_f32, -0.5, 1.0, -1.0];
        let mut out = [0.0_f32; 2];
        g.process_sample(&z, &mut out);
        let sig = |x: f32| 1.0_f32 / (1.0 + (-x).exp());
        let want0 = crate::fast_math::fast_tanh(0.5) * sig(1.0);
        let want1 = crate::fast_math::fast_tanh(-0.5) * sig(-1.0);
        assert!(
            (out[0] - want0).abs() < 1e-7,
            "out0={} want={}",
            out[0],
            want0
        );
        assert!(
            (out[1] - want1).abs() < 1e-7,
            "out1={} want={}",
            out[1],
            want1
        );
    }

    #[test]
    fn process_sample_blended_uses_raw_pre_activation_for_one_minus_alpha() {
        // bn=1, Blended, primary=ReLU, secondary=Sigmoid.
        // z = [-2.0, 0.0]: pre-activation = -2.0, alpha = sigmoid(0) = 0.5.
        // out = 0.5*relu(-2) + 0.5*(-2) = 0.5*0 + 0.5*(-2) = -1.0
        // (uses RAW z[c]=-2 for the (1-alpha) term, NOT relu(-2)=0).
        let g = Gating::new(
            GatingMode::Blended,
            Activation::Relu,
            Activation::Sigmoid,
            1,
        );
        let mut out = [0.0_f32];
        g.process_sample(&[-2.0, 0.0], &mut out);
        assert!((out[0] - (-1.0)).abs() < 1e-7, "out={}", out[0]);
    }

    #[test]
    fn process_sample_blended_two_channels_tanh_sigmoid() {
        // bn=2, primary=Tanh, secondary=Sigmoid. z=[v0,v1,g0,g1]=[0.5,-1.0,0.0,2.0].
        let g = Gating::new(
            GatingMode::Blended,
            Activation::Tanh,
            Activation::Sigmoid,
            2,
        );
        let z = [0.5_f32, -1.0, 0.0, 2.0];
        let mut out = [0.0_f32; 2];
        g.process_sample(&z, &mut out);
        let sig = |x: f32| 1.0_f32 / (1.0 + (-x).exp());
        let a0 = sig(0.0); // 0.5
        let a1 = sig(2.0);
        let want0 = a0 * crate::fast_math::fast_tanh(0.5) + (1.0 - a0) * 0.5;
        let want1 = a1 * crate::fast_math::fast_tanh(-1.0) - (1.0 - a1);
        assert!(
            (out[0] - want0).abs() < 1e-7,
            "out0={} want={}",
            out[0],
            want0
        );
        assert!(
            (out[1] - want1).abs() < 1e-7,
            "out1={} want={}",
            out[1],
            want1
        );
    }

    #[test]
    fn process_block_direct_gated_two_channels() {
        // bn=2, Gated, ReLU primary, Sigmoid secondary, n=2 (planar [row][t]).
        // rows: v0=[1,2], v1=[3,-1], g0=[0,0], g1=[0,0] (so sigmoid=0.5 everywhere).
        // z planar = [v0_t0,v0_t1, v1_t0,v1_t1, g0_t0,g0_t1, g1_t0,g1_t1].
        let g = Gating::new(GatingMode::Gated, Activation::Relu, Activation::Sigmoid, 2);
        let z = [1.0_f32, 2.0, 3.0, -1.0, 0.0, 0.0, 0.0, 0.0];
        let mut out = [0.0_f32; 4]; // bn*n = 4
        g.process_block(&z, &mut out, 2);
        // out[c*n+t] = relu(v)*0.5
        // c0: relu(1)*0.5=0.5, relu(2)*0.5=1.0 ; c1: relu(3)*0.5=1.5, relu(-1)*0.5=0.0
        assert_eq!(out, [0.5, 1.0, 1.5, 0.0]);
    }

    #[test]
    fn process_block_equals_process_sample_loop_all_modes() {
        let modes = [GatingMode::None, GatingMode::Gated, GatingMode::Blended];
        for mode in modes {
            let bn = 3usize;
            let g = Gating::new(mode, Activation::Tanh, Activation::Sigmoid, bn);
            let in_rows = g.input_rows();
            let n = 5usize;

            // Deterministic planar z: row r, time t -> value.
            let val = |r: usize, t: usize| (((r * 7 + t * 13) % 23) as f32 - 11.0) * 0.13;
            let mut z = vec![0.0_f32; in_rows * n];
            for r in 0..in_rows {
                for t in 0..n {
                    z[r * n + t] = val(r, t);
                }
            }

            // Reference: per-sample over each column t, with contiguous-row z slices.
            let mut want = vec![0.0_f32; bn * n];
            for t in 0..n {
                let zc: Vec<f32> = (0..in_rows).map(|r| z[r * n + t]).collect();
                let mut oc = vec![0.0_f32; bn];
                g.process_sample(&zc, &mut oc);
                for c in 0..bn {
                    want[c * n + t] = oc[c];
                }
            }

            let mut got = vec![0.0_f32; bn * n];
            g.process_block(&z, &mut got, n);
            for (i, (a, b)) in got.iter().zip(&want).enumerate() {
                assert!(
                    (a - b).abs() < 1e-6,
                    "mode={mode:?} idx{i}: block {a}, per-sample {b}"
                );
            }
        }
    }
}
