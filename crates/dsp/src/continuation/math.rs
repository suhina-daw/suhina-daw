// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The small numbers a continuation is made of: a complex value, the third-order Butterworth
//! low-pass its diffuse parts and its jitter run through, and the seeded noise that drives them.
//! Everything here is plain state, allocation-free and panic-free on any input.

use std::f64::consts::{PI, TAU};
use std::ops::{Add, AddAssign, Mul, Sub};

/// A complex number.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    pub const ZERO: Self = Self { re: 0.0, im: 0.0 };

    pub const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    /// `exp(j angle)`.
    pub fn cis(angle: f64) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self { re: cos, im: sin }
    }

    pub fn norm_sqr(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    pub fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }

    pub fn conj(self) -> Self {
        Self { re: self.re, im: -self.im }
    }

    pub fn scale(self, factor: f64) -> Self {
        Self { re: self.re * factor, im: self.im * factor }
    }

    /// From a stored `[re, im]` pair.
    pub fn from_pair(pair: [f32; 2]) -> Self {
        Self { re: f64::from(pair[0]), im: f64::from(pair[1]) }
    }

    pub fn to_pair(self) -> [f32; 2] {
        [self.re as f32, self.im as f32]
    }

    /// The value, or zero when either part is not finite.
    pub fn finite(self) -> Self {
        if self.re.is_finite() && self.im.is_finite() { self } else { Self::ZERO }
    }
}

impl Add for C {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { re: self.re + other.re, im: self.im + other.im }
    }
}

impl AddAssign for C {
    fn add_assign(&mut self, other: Self) {
        self.re += other.re;
        self.im += other.im;
    }
}

impl Sub for C {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self { re: self.re - other.re, im: self.im - other.im }
    }
}

impl Mul for C {
    type Output = Self;
    fn mul(self, other: Self) -> Self {
        Self { re: self.re * other.re - self.im * other.im, im: self.re * other.im + self.im * other.re }
    }
}

/// A third-order Butterworth low-pass as a first-order section and a biquad (the bilinear
/// transform with the cutoff prewarped), run on complex values with real coefficients.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Butter3 {
    /// First-order section: `y = b (x + x1) - a y1`.
    b: f64,
    a: f64,
    /// Biquad: `y = g (x + 2 x1 + x2) - a1 y1 - a2 y2`.
    g: f64,
    a1: f64,
    a2: f64,
}

impl Butter3 {
    /// The low-pass at `cutoff` Hz for a signal at `rate` Hz; the cutoff is kept inside
    /// (0, 0.49 rate).
    pub fn new(cutoff: f64, rate: f64) -> Self {
        let rate = if rate.is_finite() && rate > 0.0 { rate } else { 1.0 };
        let cutoff = if cutoff.is_finite() { cutoff.clamp(rate * 1e-6, rate * 0.49) } else { rate * 0.25 };
        let k = (PI * cutoff / rate).tan();
        let b = k / (1.0 + k);
        let a = (k - 1.0) / (k + 1.0);
        let a0 = 1.0 + k + k * k;
        let g = k * k / a0;
        let a1 = (2.0 * k * k - 2.0) / a0;
        let a2 = (1.0 - k + k * k) / a0;
        Self { b, a, g, a1, a2 }
    }

    /// The squared magnitude at `frequency` Hz of the filter made for `cutoff` at `rate`.
    pub fn power_at(&self, frequency: f64, rate: f64) -> f64 {
        let z = C::cis(-TAU * frequency / rate);
        let z2 = z * z;
        let one = C::new(1.0, 0.0);
        let first = (one + z).scale(self.b);
        let first_den = one + z.scale(self.a);
        let second = (one + z.scale(2.0) + z2).scale(self.g);
        let second_den = one + z.scale(self.a1) + z2.scale(self.a2);
        let den = first_den.norm_sqr() * second_den.norm_sqr();
        if den > 0.0 { first.norm_sqr() * second.norm_sqr() / den } else { 0.0 }
    }
}

/// The running state of a [`Butter3`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Butter3State {
    x1: C,
    y1: C,
    u1: C,
    u2: C,
    v1: C,
    v2: C,
}

impl Butter3State {
    pub fn process(&mut self, filter: &Butter3, x: C) -> C {
        let u = (x + self.x1).scale(filter.b) - self.y1.scale(filter.a);
        self.x1 = x;
        self.y1 = u;
        let v =
            (u + self.u1.scale(2.0) + self.u2).scale(filter.g) - self.v1.scale(filter.a1) - self.v2.scale(filter.a2);
        self.u2 = self.u1;
        self.u1 = u;
        self.v2 = self.v1;
        self.v1 = v;
        let finite = v.finite();
        if finite != v {
            *self = Self::default();
        }
        finite
    }
}

/// Seeded noise: xorshift64*, with uniform and Gaussian draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rng {
    state: u64,
    spare: Option<f64>,
}

impl Default for Rng {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Spread nearby seeds and keep the state nonzero.
        let state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0xD1B5_4A32_D192_ED03);
        Self { state: if state == 0 { 1 } else { state }, spare: None }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in (0, 1).
    pub fn uniform(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }

    /// Uniform in [-1, 1) scaled to unit variance.
    pub fn white(&mut self) -> f64 {
        (2.0 * self.uniform() - 1.0) * 3.0_f64.sqrt()
    }

    /// Standard normal (Box-Muller, the second value kept for the next draw).
    pub fn normal(&mut self) -> f64 {
        if let Some(value) = self.spare.take() {
            return value;
        }
        let radius = (-2.0 * self.uniform().ln()).sqrt();
        let angle = TAU * self.uniform();
        let (sin, cos) = angle.sin_cos();
        self.spare = Some(radius * sin);
        radius * cos
    }

    /// A complex normal whose real and imaginary parts each have variance `variance / 2`.
    pub fn complex(&mut self, variance: f64) -> C {
        let sd = (variance.max(0.0) / 2.0).sqrt();
        C::new(self.normal() * sd, self.normal() * sd)
    }
}

/// One step of an Ornstein-Uhlenbeck process with time constant `tau` seconds and standard
/// deviation `sd`, updated `rate` times a second.
pub fn ou_step(value: f64, tau: f64, sd: f64, rate: f64, rng: &mut Rng) -> f64 {
    if !(tau > 0.0 && rate > 0.0) {
        return 0.0;
    }
    let a = (-1.0 / (rate * tau)).exp();
    a * value + sd * (1.0 - a * a).sqrt() * rng.normal()
}

/// The power an AR process (`x = sum a_k x[n-k] + e`, innovation variance `innovation`, at
/// `rate` Hz) has after `filter`: the integral of `innovation |H|^2 / |A|^2` over the band,
/// on 512 points.
pub fn filtered_ar_power(ar: &[C], innovation: f64, filter: &Butter3, rate: f64) -> f64 {
    const POINTS: usize = 512;
    let mut sum = 0.0;
    for k in 0..POINTS {
        // Complex processes: the whole circle, -rate/2..rate/2.
        let f = (k as f64 + 0.5) / POINTS as f64 * rate - rate / 2.0;
        let z = C::cis(-TAU * f / rate);
        let mut denominator = C::new(1.0, 0.0);
        let mut power = z;
        for a in ar {
            denominator = denominator - *a * power;
            power = power * z;
        }
        let d = denominator.norm_sqr();
        if d > 1e-300 {
            sum += filter.power_at(f, rate) / d;
        }
    }
    innovation * sum / POINTS as f64
}

/// The power a real AR process has after `filter`, as [`filtered_ar_power`] for a real one.
pub fn filtered_real_ar_power(ar: &[f64], innovation: f64, filter: &Butter3, rate: f64) -> f64 {
    let complex: [C; 4] = std::array::from_fn(|k| C::new(ar.get(k).copied().unwrap_or(0.0), 0.0));
    let order = ar.len().min(4);
    filtered_ar_power(complex.get(..order).unwrap_or(&[]), innovation, filter, rate)
}
