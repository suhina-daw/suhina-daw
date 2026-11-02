// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A multimode filter for a voice or a device: low-pass, high-pass, band-pass, notch or a morph
//! through them, at 12 or 24 dB/octave, with a clean circuit or a driven one in front of it, built
//! from [`Svf`] sections, so its frequency can move while sound passes.

use crate::{Svf, SvfCoefficients};

/// The response of a [`Multimode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Response {
    #[default]
    LowPass,
    HighPass,
    BandPass,
    Notch,
    /// Through low-pass, band-pass, high-pass, notch and back as its position goes from 0 to 1.
    Morph,
}

/// What drive does in front of the filter. [`Circuit::Clean`] ignores drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Circuit {
    #[default]
    Clean,
    /// Rounded, symmetric.
    Warm,
    /// A harder knee.
    Edge,
    /// Asymmetric, with even harmonics.
    Bite,
    /// Folding back at high levels.
    Grit,
}

impl Circuit {
    fn shape(self, x: f64) -> f64 {
        match self {
            Self::Clean => x,
            Self::Warm => x.tanh(),
            Self::Edge => crate::shape::knee_clip(x.abs(), 0.6).copysign(x),
            Self::Bite => {
                if x >= 0.0 {
                    x.tanh()
                } else {
                    (1.6 * x).tanh() / 1.6
                }
            }
            Self::Grit => (x * std::f64::consts::FRAC_PI_2).sin(),
        }
    }
}

/// The settings of a [`Multimode`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MultimodeSettings {
    pub sample_rate: f64,
    pub response: Response,
    /// Two sections rather than one.
    pub steep: bool,
    pub frequency: f64,
    pub q: f64,
    /// Position of [`Response::Morph`], 0 to 1.
    pub morph: f64,
    pub circuit: Circuit,
    /// Gain in front of a driven circuit, 1 is none.
    pub drive: f64,
}

/// See the module. `Default` passes the signal through.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Multimode {
    sections: [Svf; 2],
    steep: bool,
    circuit: Circuit,
    drive: f64,
}

impl Multimode {
    /// Takes new settings and keeps the state.
    pub fn set(&mut self, s: &MultimodeSettings) {
        let c = match s.response {
            Response::LowPass => SvfCoefficients::lowpass(s.sample_rate, s.frequency, s.q),
            Response::HighPass => SvfCoefficients::highpass(s.sample_rate, s.frequency, s.q),
            Response::BandPass => SvfCoefficients::bandpass(s.sample_rate, s.frequency, s.q),
            Response::Notch => SvfCoefficients::notch(s.sample_rate, s.frequency, s.q),
            Response::Morph => SvfCoefficients::morph(s.sample_rate, s.frequency, s.q, s.morph),
        };
        for section in &mut self.sections {
            section.set(c);
        }
        self.steep = s.steep;
        self.circuit = s.circuit;
        self.drive = if s.drive.is_finite() { s.drive.clamp(0.0, 1_000.0) } else { 1.0 };
    }

    /// One sample through the circuit and the filter.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let x = match self.circuit {
            Circuit::Clean => x,
            circuit => circuit.shape(x * self.drive),
        };
        let [first, second] = &mut self.sections;
        let y = first.process(x);
        if self.steep { second.process(y) } else { y }
    }

    /// Silences it and keeps the settings.
    pub fn reset(&mut self) {
        for section in &mut self.sections {
            section.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48_000.0;

    fn settings(response: Response) -> MultimodeSettings {
        MultimodeSettings {
            sample_rate: SR,
            response,
            steep: false,
            frequency: 1_000.0,
            q: 0.707,
            morph: 0.0,
            circuit: Circuit::Clean,
            drive: 1.0,
        }
    }

    fn gain_at(s: &MultimodeSettings, hz: f64) -> f64 {
        let mut f = Multimode::default();
        f.set(s);
        let mut peak = 0.0f64;
        for i in 0..48_000 {
            let y = f.process((std::f64::consts::TAU * hz * i as f64 / SR).sin());
            if i > 24_000 {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn each_response_passes_what_it_should() {
        assert!(gain_at(&settings(Response::LowPass), 100.0) > 0.9);
        assert!(gain_at(&settings(Response::LowPass), 10_000.0) < 0.05);
        assert!(gain_at(&settings(Response::HighPass), 100.0) < 0.05);
        assert!(gain_at(&settings(Response::HighPass), 10_000.0) > 0.9);
        assert!(gain_at(&settings(Response::BandPass), 1_000.0) > 0.9);
        assert!(gain_at(&settings(Response::BandPass), 100.0) < 0.3);
        assert!(gain_at(&settings(Response::Notch), 1_000.0) < 0.01);
    }

    #[test]
    fn two_sections_are_steeper() {
        let mut steep = settings(Response::LowPass);
        steep.steep = true;
        assert!(gain_at(&steep, 4_000.0) < gain_at(&settings(Response::LowPass), 4_000.0) * 0.5);
    }

    #[test]
    fn the_morph_starts_and_ends_as_a_low_pass_and_passes_through_the_others() {
        let at = |morph: f64, hz: f64| {
            let mut s = settings(Response::Morph);
            s.morph = morph;
            gain_at(&s, hz)
        };
        assert!(at(0.0, 10_000.0) < 0.05);
        assert!(at(1.0, 10_000.0) < 0.05);
        assert!(at(0.5, 100.0) < 0.05);
        assert!(at(0.75, 1_000.0) < 0.01);
    }

    #[test]
    fn a_driven_circuit_adds_harmonics_and_clean_ignores_drive() {
        let mut clean = settings(Response::LowPass);
        clean.drive = 8.0;
        assert!((gain_at(&clean, 100.0) - gain_at(&settings(Response::LowPass), 100.0)).abs() < 1e-9);
        for circuit in [Circuit::Warm, Circuit::Edge, Circuit::Bite, Circuit::Grit] {
            let mut driven = settings(Response::LowPass);
            driven.circuit = circuit;
            driven.drive = 8.0;
            let g = gain_at(&driven, 100.0);
            assert!(g.is_finite() && g < 2.0, "{circuit:?}: {g}");
        }
    }

    #[test]
    fn fast_movement_and_broken_settings_stay_finite() {
        let mut f = Multimode::default();
        let mut peak = 0.0f64;
        for i in 0..48_000 {
            if i % 16 == 0 {
                let mut s = settings(Response::LowPass);
                s.steep = true;
                s.q = 18.0;
                s.frequency = if (i / 16) % 2 == 0 { 40.0 } else { 16_000.0 };
                f.set(&s);
            }
            peak = peak.max(f.process((i as f64 * 0.07).sin() * 0.5).abs());
        }
        assert!(peak.is_finite());
        let mut s = settings(Response::Morph);
        s.frequency = f64::NAN;
        s.q = f64::INFINITY;
        s.morph = f64::NAN;
        s.drive = f64::NAN;
        f.set(&s);
        assert!(f.process(0.5).is_finite());
    }
}
