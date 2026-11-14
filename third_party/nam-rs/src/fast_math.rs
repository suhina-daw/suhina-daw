// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: MIT

//! Suhina's change to the vendored crate (`VENDORED.md`): the rational approximation that
//! NeuralAmpModelerCore uses for `tanh` when its fast mode is on (`NAM/activations.h`
//! `fast_tanh`), for the WaveNet; and near-exact, vectorisable `exp`, `tanh` and sigmoid for
//! the LSTM. The exact `f32::tanh` is a libm call per
//! activation that cannot be vectorised; a standard WaveNet makes about 240 of them per sample,
//! and they took most of a model's time.

/// `tanh(x)` as NeuralAmpModelerCore's `fast_tanh`: within 5e-4 for |x| <= 10, and at most 1 %
/// past ±1 far beyond, where a model's activations do not go.
#[inline]
pub(crate) fn fast_tanh(x: f32) -> f32 {
    let ax = x.abs();
    let x2 = x * x;
    (x * (2.455_507_5 + 2.455_507_5 * ax + (0.893_229_85 + 0.821_226_67 * ax) * x2))
        / (2.445_066_3 + (2.445_066_3 + x2) * (x + 0.814_642_7 * x * ax).abs())
}

/// `e^x` to about one unit in the last place (Cephes' `expf`: a Cody-Waite reduction to
/// `x = n·ln2 + r` and a degree-6 polynomial in `r`), written so a loop over it vectorises.
#[inline(always)]
pub(crate) fn exp_precise(x: f32) -> f32 {
    const LOG2E: f32 = std::f32::consts::LOG2_E;
    let x = x.clamp(-87.3, 88.3);
    let n = (x * LOG2E + 0.5).floor();
    let r = x - n * 0.693_359_4 - n * -2.121_944_4e-4;
    let r2 = r * r;
    let p = (((((1.987_569_1e-4 * r + 1.398_199_9e-3) * r + 8.333_452e-3) * r + 4.166_579_6e-2) * r
        + 0.166_666_65)
        * r
        + 0.5)
        * r2
        + r
        + 1.0;
    // 2^n through the exponent bits; the clamp keeps n + 127 within 1..=254.
    p * f32::from_bits(((n as i32 + 127) as u32) << 23)
}

/// `tanh(x)` within about 1e-7, through [`exp_precise`]; vectorises.
#[inline(always)]
pub(crate) fn tanh_precise(x: f32) -> f32 {
    let e = exp_precise(2.0 * x.clamp(-9.0, 9.0));
    (e - 1.0) / (e + 1.0)
}

/// The logistic sigmoid within about 1e-7, through [`exp_precise`]; vectorises.
#[inline(always)]
pub(crate) fn sigmoid_precise(x: f32) -> f32 {
    1.0 / (1.0 + exp_precise(-x))
}

/// [`sigmoid_precise`] over a slice in place, with AVX2 picked at run time: the LSTM's
/// gates. The LSTM keeps near-exact activations, because its recurrence carries an
/// approximation's error forward (with [`fast_tanh`] and its sigmoid a pack model came out 39 dB
/// below the signal; these leave it at float precision).
#[multiversion::multiversion(targets("x86_64+avx+avx2+fma"))]
pub(crate) fn sigmoid_in_place(v: &mut [f32]) {
    for x in v.iter_mut() {
        *x = sigmoid_precise(*x);
    }
}

/// [`tanh_precise`] over a slice in place, with AVX2 picked at run time.
#[multiversion::multiversion(targets("x86_64+avx+avx2+fma"))]
pub(crate) fn tanh_in_place(v: &mut [f32]) {
    for x in v.iter_mut() {
        *x = tanh_precise(*x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_tanh_stays_close_to_tanh() {
        let worst = |limit: f32| {
            (-20_000..=20_000)
                .map(|i| i as f32 * limit / 20_000.0)
                .map(|x| (fast_tanh(x) - x.tanh()).abs())
                .fold(0.0f32, f32::max)
        };
        assert!(worst(10.0) < 5e-4, "within ±10: {}", worst(10.0));
        assert!(worst(10_000.0) < 1e-2, "far out: {}", worst(10_000.0));
        assert_eq!(fast_tanh(0.0), 0.0);
        assert_eq!(fast_tanh(-1.5), -fast_tanh(1.5));
    }

    #[test]
    fn the_precise_functions_stay_at_float_precision() {
        let mut worst = [0.0f64; 3];
        for i in -400_000..=400_000 {
            let x = i as f32 * 0.0001; // -40..40
            let xd = f64::from(x);
            worst[0] = worst[0].max((f64::from(tanh_precise(x)) - xd.tanh()).abs());
            worst[1] = worst[1].max((f64::from(sigmoid_precise(x)) - 1.0 / (1.0 + (-xd).exp())).abs());
            if x.abs() < 20.0 {
                let rel = (f64::from(exp_precise(x)) - xd.exp()).abs() / xd.exp();
                worst[2] = worst[2].max(rel);
            }
        }
        assert!(worst[0] < 3e-7, "tanh: {}", worst[0]);
        assert!(worst[1] < 2e-7, "sigmoid: {}", worst[1]);
        assert!(worst[2] < 3e-7, "exp (relative): {}", worst[2]);
        let mut v = [-3.0f32, -0.5, 0.0, 0.25, 4.0];
        tanh_in_place(&mut v);
        assert_eq!(v, [-3.0f32, -0.5, 0.0, 0.25, 4.0].map(tanh_precise));
    }

}
