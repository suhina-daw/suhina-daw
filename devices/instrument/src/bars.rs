// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The experimental bar profiles of revision 3 (`docs/spec/instrument.md` › Bar bodies): per-note
//! mode ratios, material decay laws, bar masses and the mallet, turned into `suhina_dsp::bar`
//! modes. The DSP itself lives in the shared crate.

use suhina_dsp::bar::{BarMode, Mallet, Tube, mode_shape, uniform_ratio};

/// Impact speed at full velocity, in m/s.
pub const MAX_SPEED: f64 = 4.0;
/// Output scale of a bar's weighted mode velocities.
pub const OUTPUT_SCALE: f64 = 0.12;
/// Quality factor of every bar's tube.
const TUBE_QUALITY: f64 = 40.0;
const MALLET_MASS: f64 = 0.025;
const MALLET_EXPONENT: f64 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Marimba,
    Vibraphone,
}

/// A tuned mode: its target ratio and the keys between which its register weight falls from 1 to 0.
struct Tuned {
    target: f64,
    full_until: f64,
    none_from: f64,
}

struct Material {
    reference_decay: f64,
    corner_hz: f64,
    exponent: f64,
    reference_mass: f64,
    /// The first mode's added decay with a full tube, as a time to lose 60 dB.
    radiation_seconds: f64,
}

impl Profile {
    /// From the Body parameter's value; `None` for Struck.
    pub fn from_body(body: f64) -> Option<Self> {
        match body as u8 {
            1 => Some(Self::Marimba),
            2 => Some(Self::Vibraphone),
            _ => None,
        }
    }

    fn tuned(self) -> &'static [Tuned] {
        match self {
            Self::Marimba => &[
                Tuned { target: 4.0, full_until: 84.0, none_from: 96.0 },
                Tuned { target: 10.0, full_until: 60.0, none_from: 79.0 },
            ],
            Self::Vibraphone => &[Tuned { target: 4.0, full_until: f64::INFINITY, none_from: f64::INFINITY }],
        }
    }

    fn material(self) -> Material {
        match self {
            Self::Marimba => Material {
                reference_decay: 1.2,
                corner_hz: 300.0,
                exponent: 1.4,
                reference_mass: 0.4,
                radiation_seconds: 1.2,
            },
            Self::Vibraphone => Material {
                reference_decay: 8.0,
                corner_hz: 1500.0,
                exponent: 1.0,
                reference_mass: 0.5,
                radiation_seconds: 4.0,
            },
        }
    }

    /// The tube under each bar for Resonator `amount` (0 to 1); `None` at 0.
    pub fn tube(self, amount: f64) -> Option<Tube> {
        let amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
        (amount > 0.0).then(|| Tube {
            amount,
            quality: TUBE_QUALITY,
            radiation_seconds: self.material().radiation_seconds,
        })
    }

    /// Whether note-off damps the bar (unless the sustain pedal holds it).
    pub fn damps_on_release(self) -> bool {
        self == Self::Vibraphone
    }

    /// Mode `n`'s frequency ratio to the fundamental at `key`, `n` from one.
    pub fn ratio(self, n: usize, key: f64) -> f64 {
        let tuned = self.tuned();
        let tuned_ratio = |n: usize| {
            let Some(mode) = n.checked_sub(2).and_then(|index| tuned.get(index)) else {
                return uniform_ratio(n);
            };
            let weight = if key <= mode.full_until {
                1.0
            } else if key >= mode.none_from {
                0.0
            } else {
                (mode.none_from - key) / (mode.none_from - mode.full_until)
            };
            mode.target.powf(weight) * uniform_ratio(n).powf(1.0 - weight)
        };
        let last = tuned.len() + 1;
        if n <= last { tuned_ratio(n) } else { tuned_ratio(last) * uniform_ratio(n) / uniform_ratio(last) }
    }

    /// Time for a mode at `frequency_hz` to lose 60 dB, with Body decay `decay`.
    pub fn decay_seconds(self, frequency_hz: f64, decay: f64) -> f64 {
        let m = self.material();
        let law = |f: f64| 1.0 + (f / m.corner_hz).powf(m.exponent);
        m.reference_decay * law(261.63) / law(frequency_hz) * decay / 2.0
    }

    /// The bar's mass at `key`.
    pub fn bar_mass(self, key: f64) -> f64 {
        self.material().reference_mass * 2.0_f64.powf(-(key - 60.0) / 24.0)
    }

    /// The bar's modes at `key`: `count` modes for Brightness `brightness`, Strike position
    /// `strike` and Body decay `decay`.
    pub fn modes<const N: usize>(
        self,
        key: u8,
        count: usize,
        decay: f64,
        brightness: f64,
        strike: f64,
    ) -> [BarMode; N] {
        let key = f64::from(key);
        let fundamental = 440.0 * 2.0_f64.powf((key - 69.0) / 12.0);
        let contact = 0.5 * (0.98 - strike) / 0.96;
        let mass = self.bar_mass(key);
        let level = OUTPUT_SCALE * (mass / self.material().reference_mass).sqrt();
        let mut modes = [BarMode::default(); N];
        for (index, mode) in modes.iter_mut().take(count).enumerate() {
            let n = index + 1;
            let ratio = self.ratio(n, key);
            let frequency_hz = fundamental * ratio;
            *mode = BarMode {
                frequency_hz,
                decay_seconds: self.decay_seconds(frequency_hz, decay),
                shape: mode_shape(n, contact),
                modal_mass: mass / 4.0,
                output: level * ratio.powf(brightness - 0.5),
            };
        }
        modes
    }
}

/// The mallet for Mallet hardness `hardness` (0 soft to 1 hard).
pub fn mallet(hardness: f64) -> Mallet {
    let hardness = hardness.clamp(0.0, 1.0);
    Mallet::from_contact_time(
        MALLET_MASS,
        MALLET_EXPONENT,
        0.0025 * 0.1_f64.powf(hardness),
        0.05 + 0.45 * (1.0 - hardness),
    )
}
