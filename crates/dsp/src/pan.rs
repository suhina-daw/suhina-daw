// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The equal-power pan law every pan control in Suhina uses.

/// The left and right gains of `pan` (-1.0 fully left, 0.0 centre, 1.0 fully right) by the
/// equal-power law: the cosine and sine of `(pan + 1) · π/4`, so both are `√½` at the centre and
/// their squares always add up to 1. A pan outside the range counts as its end, and one that is
/// not a finite number as the centre. Callers scale the result to their own centre (by `√2` for
/// unity at the centre).
pub fn gains(pan: f64) -> (f64, f64) {
    let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_power_and_hostile_pans_stay_in_range() {
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let (left, right) = gains(0.0);
        assert!((left - half).abs() < 1e-15 && (right - half).abs() < 1e-15);
        let (left, right) = gains(-1.0);
        assert!((left - 1.0).abs() < 1e-15 && right.abs() < 1e-15);
        for pan in [-0.7, -0.1, 0.3, 0.99] {
            let (left, right) = gains(pan);
            assert!((left * left + right * right - 1.0).abs() < 1e-12, "{pan}");
        }
        assert_eq!(gains(5.0), gains(1.0));
        assert_eq!(gains(f64::NAN), gains(0.0));
        assert_eq!(gains(f64::NEG_INFINITY), gains(0.0));
    }
}
