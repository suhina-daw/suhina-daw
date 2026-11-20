// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A struck free-free bar: damped modes excited through their mode shapes by a mallet, either a
//! nonlinear Hunt–Crossley contact simulated against the bar's motion or a linear half-sine force
//! pulse. Each mode is a complex one-pole `z' = s z + b F` whose imaginary part is the mode's
//! displacement, so every force is integrated exactly over a sample (or a contact step) and a
//! free mode is a bounded, decaying rotation. Fixed capacity, no allocation, no panics.
//!
//! The physics follows Fletcher & Rossing (free-free bar modes), Chaigne & Doutaut 1997 (modal
//! bar with a mallet) and Hunt & Crossley 1975 (dissipative contact); see `docs/spec/instrument.md`
//! › Bar bodies for the device's use of it.

use std::f64::consts::{PI, TAU};

/// Contact steps per sample, at most.
pub const MAX_CONTACT_STEPS: u32 = 32;
/// Longest time a mallet presses on the bar, and longest it chases a receding surface.
pub const MAX_CONTACT_SECONDS: f64 = 0.020;
pub const MAX_APPROACH_SECONDS: f64 = 0.010;

/// The first sixteen roots of `cos(b) cosh(b) = 1`; higher ones are `(2n + 1) pi / 2` to double
/// precision.
const BETA: [f64; 16] = [
    4.730_040_744_862_704,
    7.853_204_624_095_838,
    10.995_607_838_001_67,
    14.137_165_491_257_46,
    17.278_759_657_399_48,
    20.420_352_245_626_06,
    23.561_944_902_040_455,
    26.703_537_555_508_188,
    29.845_130_209_103_253,
    32.986_722_862_692_83,
    36.128_315_516_282_62,
    39.269_908_169_872_416,
    42.411_500_823_462_205,
    45.553_093_477_052,
    48.694_686_130_641_8,
    51.836_278_784_231_59,
];

/// The `n`-th free-free wavenumber (times the bar's length), `n` from one; zero for `n == 0`.
pub fn beta(n: usize) -> f64 {
    match n {
        0 => 0.0,
        n => BETA.get(n - 1).copied().unwrap_or((2.0 * n as f64 + 1.0) * PI / 2.0),
    }
}

/// The uniform free-free bar's frequency ratio of mode `n` to mode 1: `(beta_n / beta_1)^2`.
pub fn uniform_ratio(n: usize) -> f64 {
    (beta(n) / BETA[0]).powi(2)
}

/// Mode `n`'s shape at `x` (0 to 1 along the bar), normalised to ±1 at the ends. Evaluated in a
/// form without the cancellation of `cosh - sinh` for high modes. Zero for `n == 0` or a
/// non-finite `x`; `x` is clamped to the bar.
pub fn mode_shape(n: usize, x: f64) -> f64 {
    if n == 0 || !x.is_finite() {
        return 0.0;
    }
    let b = beta(n);
    let x = x.clamp(0.0, 1.0);
    // 1 - s, with s = (cosh b - cos b) / (sinh b - sin b), written without cancellation.
    let one_minus_s = (-(-b).exp() - b.sin() + b.cos()) / (b.sinh() - b.sin());
    let s = 1.0 - one_minus_s;
    let z = b * x;
    // cosh z - s sinh z = e^-z (1 + s) / 2 + e^z (1 - s) / 2.
    let hyperbolic = (-z).exp() * (1.0 + s) / 2.0 + z.exp() * one_minus_s / 2.0;
    (hyperbolic + z.cos() - s * z.sin()) / 2.0
}

/// `sqrt(pi) * Gamma(1 + 1/(a+1)) / Gamma(1/2 + 1/(a+1))`: the contact-time integral of a mass
/// striking a rigid spring `K d^a`, `int_0^1 dx / sqrt(1 - x^(a+1))`.
pub fn contact_integral(exponent: f64) -> f64 {
    let k = 1.0 / (exponent.clamp(1.0, 5.0) + 1.0);
    PI.sqrt() * gamma(1.0 + k) / gamma(0.5 + k)
}

/// Gamma on (0, 3) by Lanczos (g = 7, nine terms), accurate to about 1e-15 there.
fn gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let x = x - 1.0;
    let mut sum = C[0];
    for (i, c) in C.iter().enumerate().skip(1) {
        sum += c / (x + i as f64);
    }
    let t = x + G + 0.5;
    (TAU).sqrt() * t.powf(x + 0.5) * (-t).exp() * sum
}

/// A mallet: head mass, the stiffness and exponent of `F = K d^a (1 + lambda d')`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mallet {
    pub mass: f64,
    pub stiffness: f64,
    pub exponent: f64,
    /// Hunt–Crossley dissipation `lambda`, in seconds per metre.
    pub dissipation: f64,
}

impl Mallet {
    /// The mallet whose contact against a rigid surface at 1 m/s lasts `rigid_contact_seconds`.
    pub fn from_contact_time(mass: f64, exponent: f64, rigid_contact_seconds: f64, dissipation: f64) -> Self {
        let mass = finite_or(mass, 0.025).clamp(1e-4, 10.0);
        let exponent = finite_or(exponent, 2.5).clamp(1.0, 5.0);
        let tau = finite_or(rigid_contact_seconds, 0.001).clamp(1e-5, 0.1);
        let compression = tau / (2.0 * contact_integral(exponent));
        let stiffness = (exponent + 1.0) * mass / (2.0 * compression.powf(exponent + 1.0));
        Self { mass, stiffness, exponent, dissipation: finite_or(dissipation, 0.0).clamp(0.0, 10.0) }
    }

    /// Largest compression of a body of `mass` striking this rigid spring at `speed`.
    pub fn max_compression(&self, mass: f64, speed: f64) -> f64 {
        ((self.exponent + 1.0) * mass * speed * speed / (2.0 * self.stiffness)).powf(1.0 / (self.exponent + 1.0))
    }

    /// The contact's duration for a body of `mass` at `speed`, without dissipation.
    pub fn contact_time(&self, mass: f64, speed: f64) -> f64 {
        2.0 * contact_integral(self.exponent) * self.max_compression(mass, speed) / speed
    }

    /// Hunt–Crossley's restitution at `speed`, `max(0, 1 - 2 lambda v / 3)`.
    pub fn restitution(&self, speed: f64) -> f64 {
        (1.0 - 2.0 * self.dissipation * speed / 3.0).max(0.0)
    }
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

/// One mode of a bar, in physical units.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BarMode {
    pub frequency_hz: f64,
    /// Time to decay by 60 dB, in seconds (capped at 60).
    pub decay_seconds: f64,
    /// The mode shape at the contact point.
    pub shape: f64,
    /// The mode's modal mass, in kilograms.
    pub modal_mass: f64,
    /// The weight of the mode's velocity in the output.
    pub output: f64,
}

/// How a stroke's force reaches the modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Excitation {
    /// Nonlinear contact simulated against the bar's motion.
    Contact,
    /// Linear half-sine force pulse, added to what the bar already does.
    Pulse,
}

#[derive(Debug, Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn mul(self, o: Self) -> Self {
        Self::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
    fn scale(self, k: f64) -> Self {
        Self::new(self.re * k, self.im * k)
    }
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
    fn exp(self) -> Self {
        let r = self.re.exp();
        Self::new(r * self.im.cos(), r * self.im.sin())
    }
    fn div(self, o: Self) -> Self {
        let d = o.re * o.re + o.im * o.im;
        Self::new((self.re * o.re + self.im * o.im) / d, (self.im * o.re - self.re * o.im) / d)
    }
    /// `(e^x - 1) / x`, accurate near zero.
    fn phi1(self) -> Self {
        if self.re.abs() + self.im.abs() < 1e-4 {
            let x2 = self.mul(self);
            return Self::new(1.0, 0.0)
                .add(self.scale(0.5))
                .add(x2.scale(1.0 / 6.0))
                .add(x2.mul(self).scale(1.0 / 24.0));
        }
        let half = (self.im / 2.0).sin();
        let expm1 = Self::new(self.re.exp_m1() * self.im.cos() - 2.0 * half * half, self.re.exp() * self.im.sin());
        expm1.div(self)
    }
    fn is_finite(self) -> bool {
        self.re.is_finite() && self.im.is_finite()
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ModeState {
    /// `Im z` is the displacement; the velocity is `Im(s z)`.
    z: Complex,
    /// `e^(s / sample_rate)`.
    pole: Complex,
    /// Decay rate `sigma` and angular frequency `omega` of `s = -sigma + i omega`.
    sigma: f64,
    omega: f64,
    base_omega: f64,
    shape: f64,
    mass: f64,
    output: f64,
}

impl ModeState {
    fn s(&self) -> Complex {
        Complex::new(-self.sigma, self.omega)
    }
    /// The force-to-state gain `shape / (mass * omega)`.
    fn input(&self) -> f64 {
        self.shape / (self.mass * self.omega)
    }
    fn velocity(&self) -> f64 {
        self.omega * self.z.re - self.sigma * self.z.im
    }
    fn enabled(&self) -> bool {
        self.omega > 0.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
enum Stroke {
    #[default]
    Idle,
    Contact {
        mallet: Mallet,
        position: f64,
        velocity: f64,
        touched: bool,
        steps: u32,
        samples: u32,
    },
    Pulse {
        amplitude: f64,
        omega: f64,
        duration: f64,
        samples: u32,
    },
}

/// A struck bar of at most `N` modes. Modes at or above 0.45 of the sample rate, or with invalid
/// values, are silent and take no part in the contact.
#[derive(Debug, Clone, Copy)]
pub struct StruckBar<const N: usize> {
    modes: [ModeState; N],
    sample_rate: f64,
    stroke: Stroke,
    active: bool,
    last_stroke_steps: u64,
}

impl<const N: usize> Default for StruckBar<N> {
    fn default() -> Self {
        Self {
            modes: [ModeState::default(); N],
            sample_rate: 48000.0,
            stroke: Stroke::Idle,
            active: false,
            last_stroke_steps: 0,
        }
    }
}

impl<const N: usize> StruckBar<N> {
    /// Set the bar's modes, keeping each mode's motion (a fresh bar is at rest). A mode that is
    /// invalid, inaudible or missing from `modes` stops. Undoes any damping and transposition.
    pub fn set_modes(&mut self, sample_rate: f64, modes: &[BarMode]) {
        if !sample_rate.is_finite() || !(1000.0..=384000.0).contains(&sample_rate) {
            self.reset();
            return;
        }
        self.sample_rate = sample_rate;
        let mut given = modes.iter();
        for state in &mut self.modes {
            let mode = given.next().copied().unwrap_or_default();
            let valid = mode.frequency_hz.is_finite()
                && mode.frequency_hz > 0.0
                && mode.frequency_hz < sample_rate * 0.45
                && mode.decay_seconds.is_finite()
                && mode.decay_seconds > 0.0
                && mode.shape.is_finite()
                && mode.modal_mass.is_finite()
                && mode.modal_mass > 0.0
                && mode.output.is_finite();
            if !valid {
                *state = ModeState::default();
                continue;
            }
            state.base_omega = TAU * mode.frequency_hz;
            state.omega = state.base_omega;
            state.sigma = 1000.0_f64.ln() / mode.decay_seconds.clamp(0.001, 60.0);
            state.shape = mode.shape.clamp(-2.0, 2.0);
            state.mass = mode.modal_mass.clamp(1e-4, 1000.0);
            state.output = mode.output;
            state.pole = state.s().scale(1.0 / sample_rate).exp();
        }
        self.refresh_active();
    }

    /// Cap every mode's time to decay by 60 dB at `decay_seconds`, keeping its phase.
    pub fn damp(&mut self, decay_seconds: f64) {
        if !decay_seconds.is_finite() || decay_seconds <= 0.0 {
            return;
        }
        let sigma = 1000.0_f64.ln() / decay_seconds.clamp(0.001, 60.0);
        for mode in self.modes.iter_mut().filter(|mode| mode.enabled()) {
            if sigma > mode.sigma {
                mode.sigma = sigma;
                mode.pole = mode.s().scale(1.0 / self.sample_rate).exp();
            }
        }
    }

    /// Retune by an absolute offset from the frequencies of `set_modes`, keeping phase. A mode
    /// crossing the anti-alias margin stops; it is never folded down or resurrected.
    pub fn transpose(&mut self, semitones: f64) {
        if !semitones.is_finite() {
            return;
        }
        let ratio = 2.0_f64.powf(semitones.clamp(-96.0, 96.0) / 12.0);
        let limit = TAU * self.sample_rate * 0.45;
        for mode in self.modes.iter_mut().filter(|mode| mode.enabled()) {
            let omega = mode.base_omega * ratio;
            if omega >= limit {
                *mode = ModeState::default();
                continue;
            }
            mode.omega = omega;
            mode.pole = mode.s().scale(1.0 / self.sample_rate).exp();
        }
        self.refresh_active();
    }

    /// The bar's mass at the contact point, `1 / sum(shape^2 / mass)`, over its sounding modes.
    pub fn point_mass(&self) -> f64 {
        let compliance: f64 =
            self.modes.iter().filter(|mode| mode.enabled()).map(|mode| mode.shape * mode.shape / mode.mass).sum();
        if compliance > 0.0 { 1.0 / compliance } else { f64::INFINITY }
    }

    /// Strike with `mallet` at `speed` (m/s). Replaces an unfinished stroke.
    pub fn strike(&mut self, excitation: Excitation, mallet: Mallet, speed: f64) {
        self.stroke = Stroke::Idle;
        self.last_stroke_steps = 0;
        if !speed.is_finite() || speed <= 0.0 || !mallet.stiffness.is_finite() || mallet.stiffness <= 0.0 {
            return;
        }
        let speed = speed.min(100.0);
        let point = self.point_mass();
        if !point.is_finite() {
            return;
        }
        let reduced = mallet.mass * point / (mallet.mass + point);
        match excitation {
            Excitation::Contact => {
                let compression = mallet.max_compression(reduced, speed);
                let stiffness = mallet.stiffness * mallet.exponent * compression.powf(mallet.exponent - 1.0);
                let omega = (stiffness / reduced).sqrt();
                let steps = (4.0 * omega / self.sample_rate).ceil();
                let steps = if steps.is_finite() { steps.clamp(1.0, f64::from(MAX_CONTACT_STEPS)) as u32 } else { 1 };
                let surface: f64 = self.modes.iter().map(|mode| mode.shape * mode.z.im).sum();
                self.stroke =
                    Stroke::Contact { mallet, position: surface, velocity: speed, touched: false, steps, samples: 0 };
            }
            Excitation::Pulse => {
                let duration = mallet.contact_time(reduced, speed);
                if !duration.is_finite() || duration <= 0.0 {
                    return;
                }
                let impulse = (1.0 + mallet.restitution(speed)) * reduced * speed;
                self.stroke = Stroke::Pulse {
                    amplitude: PI * impulse / (2.0 * duration),
                    omega: PI / duration,
                    duration,
                    samples: 0,
                };
            }
        }
        self.active = true;
    }

    /// Advance one sample. The output is this sample's weighted mode velocities, before the
    /// stroke's force acts on it.
    pub fn next_sample(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let mut output = 0.0;
        for mode in &self.modes {
            output += mode.output * mode.velocity();
        }
        match self.stroke {
            Stroke::Idle => {
                for mode in &mut self.modes {
                    mode.z = mode.pole.mul(mode.z);
                }
            }
            Stroke::Pulse { .. } => self.pulse_sample(),
            Stroke::Contact { .. } => self.contact_sample(),
        }
        let mut active = !matches!(self.stroke, Stroke::Idle);
        let mut finite = true;
        for mode in &mut self.modes {
            if !mode.z.is_finite() {
                finite = false;
            } else if mode.z.re.abs() + mode.z.im.abs() < 1e-12 {
                mode.z = Complex::default();
            } else {
                active = true;
            }
        }
        if !finite || !output.is_finite() {
            self.reset();
            return 0.0;
        }
        self.active = active;
        output as f32
    }

    fn pulse_sample(&mut self) {
        let Stroke::Pulse { amplitude, omega, duration, samples } = self.stroke else {
            return;
        };
        let step = 1.0 / self.sample_rate;
        let start = f64::from(samples) * step;
        let length = (duration - start).min(step);
        if length <= 0.0 {
            self.stroke = Stroke::Idle;
            for mode in &mut self.modes {
                mode.z = mode.pole.mul(mode.z);
            }
            return;
        }
        let up = Complex::new(0.0, omega * start).exp();
        let down = Complex::new(up.re, -up.im);
        for mode in self.modes.iter_mut().filter(|mode| mode.enabled()) {
            let s = mode.s();
            // int_{t0}^{t0+L} sin(w t) e^{s (t0 + step - t)} dt, from e^{+-iwt} = cos +- i sin.
            let rise = up.mul(Complex::new(0.0, omega).sub(s).scale(length).phi1()).scale(length);
            let fall = down.mul(Complex::new(0.0, -omega).sub(s).scale(length).phi1()).scale(length);
            // (rise - fall) / 2i
            let difference = rise.sub(fall);
            let integral = mode.pole.mul(Complex::new(difference.im / 2.0, -difference.re / 2.0));
            mode.z = mode.pole.mul(mode.z).add(integral.scale(amplitude * mode.input()));
        }
        self.stroke = if start + step >= duration {
            Stroke::Idle
        } else {
            Stroke::Pulse { amplitude, omega, duration, samples: samples.saturating_add(1) }
        };
    }

    fn contact_sample(&mut self) {
        let Stroke::Contact { mallet, mut position, mut velocity, mut touched, steps, samples } = self.stroke else {
            return;
        };
        let h = 1.0 / (self.sample_rate * f64::from(steps));
        let mut step_poles = [(Complex::default(), Complex::default()); N];
        for (mode, coefficients) in self.modes.iter().zip(step_poles.iter_mut()) {
            if mode.enabled() {
                let sh = mode.s().scale(h);
                // Exact response to a force held over the step: b (e^{sh} - 1) / s = b h phi1(sh).
                *coefficients = (sh.exp(), sh.phi1().scale(h * mode.input()));
            }
        }
        let mut separated = false;
        for _ in 0..steps {
            let mut surface = 0.0;
            let mut surface_velocity = 0.0;
            for mode in &self.modes {
                surface += mode.shape * mode.z.im;
                surface_velocity += mode.shape * mode.velocity();
            }
            let compression = position - surface;
            let force = if compression > 0.0 {
                touched = true;
                let rate = velocity - surface_velocity;
                (mallet.stiffness * compression.powf(mallet.exponent) * (1.0 + mallet.dissipation * rate)).max(0.0)
            } else {
                0.0
            };
            velocity -= force / mallet.mass * h;
            position += velocity * h;
            for (mode, (pole, gain)) in self.modes.iter_mut().zip(step_poles.iter()) {
                mode.z = pole.mul(mode.z).add(gain.scale(force));
            }
            if touched && compression <= 0.0 && velocity < surface_velocity {
                separated = true;
            }
        }
        self.last_stroke_steps = self.last_stroke_steps.saturating_add(u64::from(steps));
        let samples = samples.saturating_add(1);
        let elapsed = f64::from(samples) / self.sample_rate;
        let limit = if touched { MAX_CONTACT_SECONDS + MAX_APPROACH_SECONDS } else { MAX_APPROACH_SECONDS };
        self.stroke = if separated || elapsed >= limit {
            Stroke::Idle
        } else {
            Stroke::Contact { mallet, position, velocity, touched, steps, samples }
        };
    }

    /// The bar's vibrational energy, `sum(mass * (velocity^2 + (sigma^2 + omega^2) displacement^2) / 2)`.
    pub fn energy(&self) -> f64 {
        self.modes
            .iter()
            .filter(|mode| mode.enabled())
            .map(|mode| {
                let stiffness = mode.sigma * mode.sigma + mode.omega * mode.omega;
                0.5 * mode.mass * (mode.velocity().powi(2) + stiffness * mode.z.im.powi(2))
            })
            .sum()
    }

    /// Contact steps the latest stroke has taken.
    pub fn last_stroke_steps(&self) -> u64 {
        self.last_stroke_steps
    }

    /// Whether a contact or pulse is still acting on the bar.
    pub fn is_struck(&self) -> bool {
        !matches!(self.stroke, Stroke::Idle)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn reset(&mut self) {
        let sample_rate = self.sample_rate;
        *self = Self { sample_rate, ..Self::default() };
    }

    fn refresh_active(&mut self) {
        self.active = !matches!(self.stroke, Stroke::Idle)
            || self.modes.iter().any(|mode| mode.enabled() && mode.z.re.abs() + mode.z.im.abs() >= 1e-12);
    }
}

#[cfg(test)]
#[path = "bar_tests.rs"]
mod tests;
