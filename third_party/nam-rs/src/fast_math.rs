//! Suhina's change to the vendored crate (`VENDORED.md`): the rational approximations that
//! NeuralAmpModelerCore uses for `tanh` and the sigmoid when its fast mode is on
//! (`NAM/activations.h` `fast_tanh`, `fast_sigmoid`). The exact `f32::tanh` is a libm call per
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

/// The logistic sigmoid through [`fast_tanh`], as NeuralAmpModelerCore's `fast_sigmoid`.
#[inline]
pub(crate) fn fast_sigmoid(x: f32) -> f32 {
    0.5 * (fast_tanh(x * 0.5) + 1.0)
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
    fn fast_sigmoid_stays_close_to_the_sigmoid() {
        for i in -2000..=2000 {
            let x = i as f32 * 0.005;
            let exact = 1.0 / (1.0 + (-x).exp());
            assert!((fast_sigmoid(x) - exact).abs() < 1e-3, "at {x}");
        }
    }
}
