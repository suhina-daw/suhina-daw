// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A filter before a nonlinearity and its exact inverse after it.

use crate::{Coefficients, flush};

/// How close the forward filter's output must be to its input for [`Emphasis::is_transparent`]
/// (-140 dB of full scale).
const TRANSPARENT: f64 = 1e-7;

/// The last two inputs and outputs of a filter in direct form I.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct History {
    x: [f64; 2],
    y: [f64; 2],
}

impl History {
    #[inline]
    fn push(&mut self, x: f64, y: f64) {
        self.x = [flush(x), self.x[0]];
        self.y = [flush(y), self.y[0]];
    }
}

/// A biquad and its exact inverse, which share one set of coefficients: [`emphasise`] filters
/// the signal on the way into a nonlinearity and [`deemphasise`] undoes it on the way out.
///
/// Both run in direct form I, which keeps the signal's own history rather than a mix of inputs
/// and outputs, and the inverse solves the forward filter's difference equation for its input.
/// So while what lies between them is linear, the pair passes the signal unchanged sample for
/// sample, even while the coefficients change; two [`Biquad`](crate::Biquad)s with inverse
/// responses cancel only while their coefficients hold still.
///
/// [`emphasise`]: Emphasis::emphasise
/// [`deemphasise`]: Emphasis::deemphasise
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Emphasis {
    c: Coefficients,
    forward: History,
    inverse: History,
}

impl Emphasis {
    pub fn new(coefficients: Coefficients) -> Self {
        let mut emphasis = Self::default();
        emphasis.set(coefficients);
        emphasis
    }

    /// Changes the coefficients of both filters and keeps their histories. Coefficients with a
    /// field that is not finite, or whose `b0` is so small that the inverse would overflow, are ignored.
    pub fn set(&mut self, coefficients: Coefficients) {
        let Coefficients { b0, b1, b2, a1, a2 } = coefficients;
        if [b0, b1, b2, a1, a2].iter().all(|c| c.is_finite()) && b0.abs() >= 1e-12 {
            self.c = coefficients;
        }
    }

    /// The forward filter.
    #[inline]
    pub fn emphasise(&mut self, x: f64) -> f64 {
        let Coefficients { b0, b1, b2, a1, a2 } = self.c;
        let History { x: [x1, x2], y: [y1, y2] } = self.forward;
        let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        self.forward.push(x, y);
        y
    }

    /// The inverse: the input the forward filter would have needed to give `y`.
    #[inline]
    pub fn deemphasise(&mut self, y: f64) -> f64 {
        let Coefficients { b0, b1, b2, a1, a2 } = self.c;
        // Here the history's x is what comes out (the forward filter's input) and y what goes in.
        let History { x: [x1, x2], y: [y1, y2] } = self.inverse;
        let x = (y + a1 * y1 + a2 * y2 - b1 * x1 - b2 * x2) / b0;
        self.inverse.push(x, y);
        x
    }

    /// Whether both filters now pass their input unchanged: the coefficients are those of no
    /// filter (a peaking filter at 0 dB, say) and what they held from before has died away. Only
    /// then can a caller stop running the pair without a jump in what lies between them.
    pub fn is_transparent(&self) -> bool {
        let Coefficients { b0, b1, b2, a1, a2 } = self.c;
        let settled = |h: &History| h.x.iter().zip(&h.y).all(|(x, y)| (x - y).abs() <= TRANSPARENT * x.abs().max(1.0));
        b0 == 1.0 && b1 == a1 && b2 == a2 && settled(&self.forward) && settled(&self.inverse)
    }

    pub fn reset(&mut self) {
        self.forward = History::default();
        self.inverse = History::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Biquad;

    const SR: f64 = 48_000.0;

    #[test]
    fn coefficients_without_an_inverse_or_not_finite_are_ignored() {
        let good = Coefficients { b0: 1.0, b1: 0.5, b2: 0.0, a1: -0.2, a2: 0.0 };
        for bad in [
            Coefficients { b0: 0.0, ..good },
            Coefficients { b0: 1e-320, ..good },
            Coefficients { a2: f64::NAN, ..good },
            Coefficients { b1: f64::INFINITY, ..good },
        ] {
            assert_eq!(Emphasis::new(bad), Emphasis::default());
            let mut e = Emphasis::new(good);
            e.set(bad);
            assert_eq!(e, Emphasis::new(good));
        }
    }

    /// A repeatable signal with every frequency in it.
    fn noise(n: usize) -> f64 {
        let h = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 11;
        h as f64 / (1u64 << 53) as f64 - 0.5
    }

    #[test]
    fn the_inverse_undoes_the_filter_while_it_is_retuned() {
        // A +36 dB bell glides from 0 dB, retuned every 16 samples, with a gain between the two
        // filters: the pair stays exact. Two biquads with inverse responses leave an error.
        let mut pair = Emphasis::default();
        let (mut bell, mut unbell) = (Biquad::default(), Biquad::default());
        let (mut exact, mut separate) = (0.0f64, 0.0f64);
        for n in 0..9_600 {
            if n % 16 == 0 {
                let db = 36.0 * (n as f64 / 960.0).min(1.0);
                pair.set(Coefficients::peak(SR, 41.4, 0.103, db));
                bell.set(Coefficients::peak(SR, 41.4, 0.103, db));
                unbell.set(Coefficients::peak(SR, 41.4, 0.103, -db));
            }
            let x = noise(n);
            let driven = 0.5 * pair.emphasise(x);
            exact = exact.max((pair.deemphasise(driven) - 0.5 * x).abs());
            separate = separate.max((unbell.process(0.5 * bell.process(x)) - 0.5 * x).abs());
        }
        assert!(exact < 1e-9, "{exact}");
        assert!(separate > 1e-4, "{separate}");
    }

    #[test]
    fn filters_like_a_biquad() {
        let c = Coefficients::peak(SR, 1_000.0, 0.7, 12.0);
        let (mut pair, mut biquad) = (Emphasis::new(c), Biquad::new(c));
        for n in 0..4_800 {
            let x = noise(n);
            assert!((pair.emphasise(x) - biquad.process(x)).abs() < 1e-12);
        }
    }

    #[test]
    fn is_transparent_once_a_flat_filter_has_settled() {
        let mut pair = Emphasis::default();
        assert!(pair.is_transparent());
        pair.set(Coefficients::peak(SR, 41.4, 0.103, 36.0));
        assert!(!pair.is_transparent());
        for n in 0..4_800 {
            let y = pair.emphasise(noise(n));
            pair.deemphasise(y);
        }
        assert!(!pair.is_transparent());
        // Back at 0 dB the filter still rings with what it held for a while.
        pair.set(Coefficients::peak(SR, 41.4, 0.103, 0.0));
        let mut settled = None;
        for n in 0..48_000 {
            let y = pair.emphasise(noise(n));
            pair.deemphasise(y);
            if settled.is_none() && pair.is_transparent() {
                settled = Some(n);
            }
        }
        assert!(settled.is_some_and(|n| n > 10), "{settled:?}");
    }

    #[test]
    fn broken_input_does_not_stick() {
        let mut pair = Emphasis::new(Coefficients::peak(SR, 100.0, 0.5, 24.0));
        pair.set(Coefficients { b0: 0.0, ..Coefficients::default() });
        let mut round_trip = |x| {
            let y = pair.emphasise(x);
            pair.deemphasise(y)
        };
        for x in [f64::NAN, f64::INFINITY, 1e30] {
            round_trip(x);
        }
        let out: Vec<f64> = (0..4_800).map(|n| round_trip(noise(n))).collect();
        assert!(out[4_000..].iter().all(|v| v.is_finite() && v.abs() < 1.0));
    }
}
