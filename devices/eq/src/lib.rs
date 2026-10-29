// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! EQ: an eight-band parametric equaliser (docs/spec/eq.md).
//!
//! Each band is a low cut or high cut (12 or 48 dB/octave), a low or high shelf, a bell or a
//! notch, with its own frequency, gain and Q, and can be switched off. The bands run in series
//! on state-variable filters ([`suhina_dsp::Svf`]) whose responses are the cookbook ones. Every
//! band exists twice, in curve A and curve B: Stereo runs curve A on both sides, Left/Right
//! curve A on the left and B on the right, Mid/Side curve A on the mid and B on the side. Scale
//! multiplies the gains of shelves and bells; Adaptive Q narrows a bell as its gain grows past
//! 6 dB and widens it below. The device adds no latency.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::{Smoother, Svf, SvfCoefficients};

/// Ramp for frequency, gain, Q, Scale and Output changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// How long a band takes to fade out or in when it is switched on or off or changes type, and
/// how long the output dips when the mode changes.
pub const FADE_SECONDS: f64 = 0.01;
/// While a band's frequency, gain or Q moves, its filters are retuned every this many frames.
const RETUNE_FRAMES: u32 = 16;
/// Adaptive Q multiplies a bell's Q by this to the power of its gain in decibels less
/// [`ADAPTIVE_Q_CENTRE_DB`].
const ADAPTIVE_Q_BASE: f64 = 1.12;
const ADAPTIVE_Q_CENTRE_DB: f64 = 6.0;
/// The Qs of the four sections of an 8th-order Butterworth filter, lowest first.
const BUTTERWORTH_8: [f64; 4] = [0.509_795_6, 0.601_344_9, 0.899_976_2, 2.562_915_4];
/// How much a 48 dB cut's section Qs grow with its Q: by `1 + SLOPE · ln(Q / Q₀)`.
const CUT_48_Q_SLOPE: f64 = 0.263;
/// The default Q, at which a cut is a Butterworth filter: 1/√2.
pub const DEFAULT_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// A shelf's Q is not the cookbook's: the measured shelves match a cookbook shelf whose Q is
/// `SHELF_Q_OFFSET + SHELF_Q_SLOPE · ln(1 + Q / SHELF_Q_KNEE)`, the same at every gain and
/// frequency (1/√2 stays 1/√2, Q 6 gives 1.50, Q 18 gives 1.96).
const SHELF_Q_OFFSET: f64 = 0.1687;
const SHELF_Q_SLOPE: f64 = 0.4281;
const SHELF_Q_KNEE: f64 = 0.281;

const FREQUENCY: (f64, f64) = (10.0, 22_000.0);
const GAIN: (f64, f64) = (-15.0, 15.0);
const Q_RANGE: (f64, f64) = (0.1, 18.0);

/// The band types, in [`param::TYPE`] order.
pub const TYPE_NAMES: [&str; 8] =
    ["Low Cut 48", "Low Cut 12", "Low Shelf", "Bell", "Notch", "High Shelf", "High Cut 12", "High Cut 48"];
/// How the two curves are used, in [`param::MODE`] order.
pub const MODE_NAMES: [&str; 3] = ["Stereo", "Left/Right", "Mid/Side"];

/// Bands per curve, and the two curves.
pub const BANDS: usize = 8;
const CURVES: usize = 2;

/// Parameter indices, in [`INFO`] order: curve A's eight bands, curve B's, then the global ones.
pub mod param {
    /// A band's parameters, in this order.
    pub const ON: usize = 0;
    pub const TYPE: usize = 1;
    pub const FREQUENCY: usize = 2;
    pub const GAIN: usize = 3;
    pub const Q: usize = 4;
    pub(crate) const PER_BAND: usize = 5;

    /// The index of `field` (one of the constants above) of band `band` (from 0) of curve
    /// `curve` (0 for A, 1 for B).
    pub const fn band(curve: usize, band: usize, field: usize) -> usize {
        (curve * super::BANDS + band) * PER_BAND + field
    }

    pub const OUTPUT: usize = 80;
    pub const SCALE: usize = 81;
    pub const ADAPTIVE_Q: usize = 82;
    pub const MODE: usize = 83;
    pub(crate) const COUNT: usize = 84;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::toggle("a1On", "Band 1 On", true).in_group("Curve A"),
    ParamInfo::choice("a1Type", "Band 1 Type", &TYPE_NAMES, 2).in_group("Curve A"),
    ParamInfo::continuous("a1Frequency", "Band 1 Frequency", FREQUENCY, 30.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a1Gain", "Band 1 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a1Q", "Band 1 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("a2On", "Band 2 On", true).in_group("Curve A"),
    ParamInfo::choice("a2Type", "Band 2 Type", &TYPE_NAMES, 3).in_group("Curve A"),
    ParamInfo::continuous("a2Frequency", "Band 2 Frequency", FREQUENCY, 200.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a2Gain", "Band 2 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a2Q", "Band 2 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("a3On", "Band 3 On", true).in_group("Curve A"),
    ParamInfo::choice("a3Type", "Band 3 Type", &TYPE_NAMES, 3).in_group("Curve A"),
    ParamInfo::continuous("a3Frequency", "Band 3 Frequency", FREQUENCY, 1000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a3Gain", "Band 3 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a3Q", "Band 3 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("a4On", "Band 4 On", true).in_group("Curve A"),
    ParamInfo::choice("a4Type", "Band 4 Type", &TYPE_NAMES, 5).in_group("Curve A"),
    ParamInfo::continuous("a4Frequency", "Band 4 Frequency", FREQUENCY, 5000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a4Gain", "Band 4 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a4Q", "Band 4 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("a5On", "Band 5 On", false).in_group("Curve A"),
    ParamInfo::choice("a5Type", "Band 5 Type", &TYPE_NAMES, 3).in_group("Curve A"),
    ParamInfo::continuous("a5Frequency", "Band 5 Frequency", FREQUENCY, 100.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a5Gain", "Band 5 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a5Q", "Band 5 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("a6On", "Band 6 On", false).in_group("Curve A"),
    ParamInfo::choice("a6Type", "Band 6 Type", &TYPE_NAMES, 3).in_group("Curve A"),
    ParamInfo::continuous("a6Frequency", "Band 6 Frequency", FREQUENCY, 10000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a6Gain", "Band 6 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a6Q", "Band 6 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("a7On", "Band 7 On", false).in_group("Curve A"),
    ParamInfo::choice("a7Type", "Band 7 Type", &TYPE_NAMES, 3).in_group("Curve A"),
    ParamInfo::continuous("a7Frequency", "Band 7 Frequency", FREQUENCY, 5000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a7Gain", "Band 7 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a7Q", "Band 7 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("a8On", "Band 8 On", false).in_group("Curve A"),
    ParamInfo::choice("a8Type", "Band 8 Type", &TYPE_NAMES, 6).in_group("Curve A"),
    ParamInfo::continuous("a8Frequency", "Band 8 Frequency", FREQUENCY, 18000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve A"),
    ParamInfo::continuous("a8Gain", "Band 8 Gain", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve A"),
    ParamInfo::continuous("a8Q", "Band 8 Q", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve A"),
    ParamInfo::toggle("b1On", "Band 1 On B", false).in_group("Curve B"),
    ParamInfo::choice("b1Type", "Band 1 Type B", &TYPE_NAMES, 1).in_group("Curve B"),
    ParamInfo::continuous("b1Frequency", "Band 1 Frequency B", FREQUENCY, 40.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b1Gain", "Band 1 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b1Q", "Band 1 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::toggle("b2On", "Band 2 On B", false).in_group("Curve B"),
    ParamInfo::choice("b2Type", "Band 2 Type B", &TYPE_NAMES, 2).in_group("Curve B"),
    ParamInfo::continuous("b2Frequency", "Band 2 Frequency B", FREQUENCY, 200.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b2Gain", "Band 2 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b2Q", "Band 2 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::toggle("b3On", "Band 3 On B", true).in_group("Curve B"),
    ParamInfo::choice("b3Type", "Band 3 Type B", &TYPE_NAMES, 3).in_group("Curve B"),
    ParamInfo::continuous("b3Frequency", "Band 3 Frequency B", FREQUENCY, 100.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b3Gain", "Band 3 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b3Q", "Band 3 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::toggle("b4On", "Band 4 On B", true).in_group("Curve B"),
    ParamInfo::choice("b4Type", "Band 4 Type B", &TYPE_NAMES, 3).in_group("Curve B"),
    ParamInfo::continuous("b4Frequency", "Band 4 Frequency B", FREQUENCY, 500.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b4Gain", "Band 4 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b4Q", "Band 4 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::toggle("b5On", "Band 5 On B", true).in_group("Curve B"),
    ParamInfo::choice("b5Type", "Band 5 Type B", &TYPE_NAMES, 3).in_group("Curve B"),
    ParamInfo::continuous("b5Frequency", "Band 5 Frequency B", FREQUENCY, 2000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b5Gain", "Band 5 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b5Q", "Band 5 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::toggle("b6On", "Band 6 On B", true).in_group("Curve B"),
    ParamInfo::choice("b6Type", "Band 6 Type B", &TYPE_NAMES, 3).in_group("Curve B"),
    ParamInfo::continuous("b6Frequency", "Band 6 Frequency B", FREQUENCY, 10000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b6Gain", "Band 6 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b6Q", "Band 6 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::toggle("b7On", "Band 7 On B", false).in_group("Curve B"),
    ParamInfo::choice("b7Type", "Band 7 Type B", &TYPE_NAMES, 5).in_group("Curve B"),
    ParamInfo::continuous("b7Frequency", "Band 7 Frequency B", FREQUENCY, 5000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b7Gain", "Band 7 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b7Q", "Band 7 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::toggle("b8On", "Band 8 On B", false).in_group("Curve B"),
    ParamInfo::choice("b8Type", "Band 8 Type B", &TYPE_NAMES, 6).in_group("Curve B"),
    ParamInfo::continuous("b8Frequency", "Band 8 Frequency B", FREQUENCY, 18000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Curve B"),
    ParamInfo::continuous("b8Gain", "Band 8 Gain B", GAIN, 0.0, Unit::Decibels, Scale::Linear).in_group("Curve B"),
    ParamInfo::continuous("b8Q", "Band 8 Q B", Q_RANGE, DEFAULT_Q, Unit::None, Scale::Logarithmic).in_group("Curve B"),
    ParamInfo::continuous("output", "Output", (-12.0, 12.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("scale", "Scale", (-2.0, 2.0), 1.0, Unit::Percent, Scale::Linear),
    ParamInfo::toggle("adaptiveQ", "Adaptive Q", true),
    ParamInfo::choice("mode", "Mode", &MODE_NAMES, 0),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "eq", name: "EQ", role: Role::Effect, params: &PARAMS };

/// One of the band types of [`TYPE_NAMES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    LowCut48,
    LowCut12,
    LowShelf,
    Bell,
    Notch,
    HighShelf,
    HighCut12,
    HighCut48,
}

impl Kind {
    fn from_index(index: f64) -> Self {
        match index as usize {
            0 => Self::LowCut48,
            1 => Self::LowCut12,
            2 => Self::LowShelf,
            4 => Self::Notch,
            5 => Self::HighShelf,
            6 => Self::HighCut12,
            7 => Self::HighCut48,
            _ => Self::Bell,
        }
    }

    /// The sections this type runs for a frequency, gain (Scale applied) and Q, and how many.
    fn sections(self, sr: f64, frequency: f64, gain: f64, q: f64, adaptive: bool) -> ([SvfCoefficients; 4], usize) {
        let mut out = [SvfCoefficients::IDENTITY; 4];
        let count = match self {
            Self::LowCut48 | Self::HighCut48 => {
                let factor = (1.0 + CUT_48_Q_SLOPE * (q / DEFAULT_Q).ln()).max(0.1);
                for (section, base) in out.iter_mut().zip(BUTTERWORTH_8) {
                    *section = if self == Self::LowCut48 {
                        SvfCoefficients::highpass(sr, frequency, base * factor)
                    } else {
                        SvfCoefficients::lowpass(sr, frequency, base * factor)
                    };
                }
                4
            }
            Self::LowCut12 => {
                out[0] = SvfCoefficients::highpass(sr, frequency, q);
                1
            }
            Self::HighCut12 => {
                out[0] = SvfCoefficients::lowpass(sr, frequency, q);
                1
            }
            Self::LowShelf => {
                out[0] = SvfCoefficients::low_shelf(sr, frequency, shelf_q(q), gain);
                1
            }
            Self::HighShelf => {
                out[0] = SvfCoefficients::high_shelf(sr, frequency, shelf_q(q), gain);
                1
            }
            Self::Notch => {
                out[0] = SvfCoefficients::notch(sr, frequency, q);
                1
            }
            Self::Bell => {
                let q = if adaptive { q * ADAPTIVE_Q_BASE.powf(gain.abs() - ADAPTIVE_Q_CENTRE_DB) } else { q };
                out[0] = SvfCoefficients::bell(sr, frequency, q, gain);
                1
            }
        };
        (out, count)
    }
}

/// The cookbook Q that gives a shelf the shape its Q control has.
fn shelf_q(q: f64) -> f64 {
    SHELF_Q_OFFSET + SHELF_Q_SLOPE * (q / SHELF_Q_KNEE).ln_1p()
}

/// The filter sections the band with these values runs, for a panel drawing the response
/// curve (docs/spec/eq.md › Behaviour): the type's index in [`TYPE_NAMES`] order, the
/// frequency in Hz, the gain in decibels before Scale, the Q, the Scale factor (1 is 100 %)
/// and whether Adaptive Q is on. The first `count` of the sections are the band's filters;
/// the rest are identity. The panel multiplies the sections' magnitudes to draw the band's
/// response, so the curve is the device's own maths, never a second copy of it.
pub fn band_sections(
    type_index: usize,
    sample_rate: f64,
    frequency: f64,
    gain_db: f64,
    q: f64,
    scale: f64,
    adaptive: bool,
) -> ([SvfCoefficients; 4], usize) {
    Kind::from_index(type_index as f64).sections(sample_rate, frequency, gain_db * scale, q, adaptive)
}

/// One band of one curve: what it is now and its smoothed controls.
#[derive(Clone, Copy)]
struct Band {
    /// The type it runs; a new type waits until the band has faded out.
    kind: Kind,
    /// Frequency in octaves above 1 Hz, gain in dB, Q as its natural logarithm: each glides
    /// evenly in that unit.
    octave: Smoother,
    gain: Smoother,
    log_q: Smoother,
    /// How much of the band is heard: 0 bypasses it, 1 is the filter alone.
    amount: Smoother,
    /// Waiting to fade out before its type changes or it stops.
    pending: bool,
    /// Whether it runs at all (on, or still fading out).
    active: bool,
    sections: [SvfCoefficients; 4],
    count: usize,
    /// A shelf or bell with no gain passes the sound unchanged, so it is skipped.
    flat: bool,
    retune_in: u32,
}

impl Band {
    fn new() -> Self {
        Self {
            kind: Kind::Bell,
            octave: Smoother::new(0.0, 1),
            gain: Smoother::new(0.0, 1),
            log_q: Smoother::new(0.0, 1),
            amount: Smoother::new(0.0, 1),
            pending: false,
            active: false,
            sections: [SvfCoefficients::IDENTITY; 4],
            count: 0,
            flat: true,
            retune_in: 0,
        }
    }

    fn controls_moving(&self) -> bool {
        !(self.octave.is_settled() && self.gain.is_settled() && self.log_q.is_settled())
    }

    /// Nothing about the band is changing.
    fn still(&self) -> bool {
        !self.pending && self.amount.is_settled() && !self.controls_moving()
    }
}

pub struct Eq {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    bands: [[Band; BANDS]; CURVES],
    /// The filters each side runs, band by band.
    filters: [[[Svf; 4]; BANDS]; 2],
    output: Smoother,
    scale: Smoother,
    /// The mode in use; a new one waits until the output has dipped.
    mode: usize,
    mode_fade: Smoother,
    mode_pending: bool,
}

impl Default for Eq {
    fn default() -> Self {
        Self::new()
    }
}

impl Eq {
    pub fn new() -> Self {
        let mut eq = Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            params: ParamBank::new(&PARAMS),
            bands: [[Band::new(); BANDS]; CURVES],
            filters: [[[Svf::default(); 4]; BANDS]; 2],
            output: Smoother::new(1.0, 1),
            scale: Smoother::new(1.0, 1),
            mode: 0,
            mode_fade: Smoother::new(1.0, 1),
            mode_pending: false,
        };
        eq.rebuild();
        eq
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn band_value(&self, curve: usize, band: usize, field: usize) -> f64 {
        self.value(param::band(curve, band, field))
    }

    fn mode_param(&self) -> usize {
        (self.value(param::MODE).round() as usize).min(MODE_NAMES.len() - 1)
    }

    /// The curve side `side` runs in the current mode.
    fn curve_of(&self, side: usize) -> usize {
        if self.mode == 0 { 0 } else { side.min(CURVES - 1) }
    }

    /// Sets the band's smoothers' targets from its parameters.
    fn target_band(&mut self, curve: usize, band: usize) {
        let octave = self.band_value(curve, band, param::FREQUENCY).log2() as f32;
        let gain = self.band_value(curve, band, param::GAIN) as f32;
        let log_q = self.band_value(curve, band, param::Q).ln() as f32;
        if let Some(b) = self.bands.get_mut(curve).and_then(|c| c.get_mut(band)) {
            b.octave.set_target(octave);
            b.gain.set_target(gain);
            b.log_q.set_target(log_q);
        }
    }

    /// Recomputes a band's filters from its smoothed controls and gives them to the sides that
    /// run its curve.
    fn tune(&mut self, curve: usize, band: usize) {
        let sr = self.sample_rate;
        let scale = f64::from(self.scale.current());
        let adaptive = self.value(param::ADAPTIVE_Q) >= 0.5;
        let Some(b) = self.bands.get_mut(curve).and_then(|c| c.get_mut(band)) else {
            return;
        };
        let frequency = 2f64.powf(f64::from(b.octave.current()));
        let gain = f64::from(b.gain.current()) * scale;
        let q = f64::from(b.log_q.current()).exp();
        let (sections, count) = b.kind.sections(sr, frequency, gain, q, adaptive);
        let was_flat = b.flat;
        b.sections = sections;
        b.count = count;
        b.flat = matches!(b.kind, Kind::LowShelf | Kind::HighShelf | Kind::Bell) && gain.abs() < 1e-9;
        let wakes = was_flat && !b.flat;
        self.give(curve, band);
        if wakes {
            // Skipped while flat, its state is stale: it starts from silence, as its gain
            // rises from nothing.
            self.clear(curve, band);
        }
    }

    /// Hands a band's coefficients to the sides that run its curve.
    fn give(&mut self, curve: usize, band: usize) {
        let Some(sections) = self.bands.get(curve).and_then(|c| c.get(band)).map(|b| b.sections) else {
            return;
        };
        for side in 0..2 {
            if self.curve_of(side) != curve {
                continue;
            }
            if let Some(filters) = self.filters.get_mut(side).and_then(|f| f.get_mut(band)) {
                for (filter, c) in filters.iter_mut().zip(sections) {
                    filter.set(c);
                }
            }
        }
    }

    /// Clears the state of a band's filters on the sides that run its curve.
    fn clear(&mut self, curve: usize, band: usize) {
        for side in 0..2 {
            if self.curve_of(side) == curve
                && let Some(filters) = self.filters.get_mut(side).and_then(|f| f.get_mut(band))
            {
                filters.iter_mut().for_each(Svf::reset);
            }
        }
    }

    /// Every smoother, band and filter for the current sample rate and parameters, the state
    /// cleared.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let fade = Smoother::ramp_frames(FADE_SECONDS, sr);
        self.mode = self.mode_param();
        self.mode_pending = false;
        self.mode_fade = Smoother::new(1.0, fade);
        self.output = Smoother::new(db_to_gain(self.value(param::OUTPUT) as f32), ramp);
        self.scale = Smoother::new(self.value(param::SCALE) as f32, ramp);
        self.filters = [[[Svf::default(); 4]; BANDS]; 2];
        for curve in 0..CURVES {
            for band in 0..BANDS {
                let on = self.band_value(curve, band, param::ON) >= 0.5;
                let kind = Kind::from_index(self.band_value(curve, band, param::TYPE));
                let octave = self.band_value(curve, band, param::FREQUENCY).log2() as f32;
                let gain = self.band_value(curve, band, param::GAIN) as f32;
                let log_q = self.band_value(curve, band, param::Q).ln() as f32;
                if let Some(b) = self.bands.get_mut(curve).and_then(|c| c.get_mut(band)) {
                    *b = Band {
                        kind,
                        octave: Smoother::new(octave, ramp),
                        gain: Smoother::new(gain, ramp),
                        log_q: Smoother::new(log_q, ramp),
                        amount: Smoother::new(if on { 1.0 } else { 0.0 }, fade),
                        pending: false,
                        active: on,
                        ..Band::new()
                    };
                }
                self.tune(curve, band);
            }
        }
    }

    /// After a band's On or Type changed: fade it to the new state.
    fn switch_band(&mut self, curve: usize, band: usize) {
        let on = self.band_value(curve, band, param::ON) >= 0.5;
        let kind = Kind::from_index(self.band_value(curve, band, param::TYPE));
        let Some(b) = self.bands.get_mut(curve).and_then(|c| c.get_mut(band)) else {
            return;
        };
        if kind != b.kind || !on {
            // Fade out first; the new type or the stop comes when it is silent.
            b.pending = true;
            b.amount.set_target(0.0);
        } else {
            b.pending = false;
            // A band that was off was not retuned while it was silent (Scale may have moved
            // since): retune it before it fades in, so it never plays with stale coefficients.
            let wakes = !b.active;
            b.active = true;
            b.amount.set_target(1.0);
            if wakes {
                self.tune(curve, band);
            }
        }
    }

    /// A band that has faded out: takes its new type, and fades in again if it is on.
    fn settle_band(&mut self, curve: usize, band: usize) {
        let on = self.band_value(curve, band, param::ON) >= 0.5;
        let kind = Kind::from_index(self.band_value(curve, band, param::TYPE));
        if let Some(b) = self.bands.get_mut(curve).and_then(|c| c.get_mut(band)) {
            b.pending = false;
            b.kind = kind;
            b.active = on;
            if on {
                b.amount.set_target(1.0);
            }
        }
        self.clear(curve, band);
        self.tune(curve, band);
    }

    /// The output has dipped: takes the new mode, with every side's filters cleared and tuned
    /// from its new curve.
    fn settle_mode(&mut self) {
        self.mode = self.mode_param();
        self.mode_pending = false;
        self.filters = [[[Svf::default(); 4]; BANDS]; 2];
        for curve in 0..CURVES {
            for band in 0..BANDS {
                self.give(curve, band);
            }
        }
        self.mode_fade.set_target(1.0);
    }
}

impl Device for Eq {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        self.sample_rate = check_sample_rate(sample_rate)?;
        self.rebuild();
        Ok(())
    }

    fn reset(&mut self) {
        self.rebuild();
    }

    fn set_param(&mut self, index: usize, value: f64) {
        if self.params.set(index, value).is_none() {
            return;
        }
        match index {
            param::OUTPUT => self.output.set_target(db_to_gain(self.value(param::OUTPUT) as f32)),
            param::SCALE => self.scale.set_target(self.value(param::SCALE) as f32),
            param::ADAPTIVE_Q => {
                for curve in 0..CURVES {
                    for band in 0..BANDS {
                        self.tune(curve, band);
                    }
                }
            }
            param::MODE => {
                if self.mode_param() != self.mode {
                    self.mode_pending = true;
                    self.mode_fade.set_target(0.0);
                }
            }
            _ => {
                let (curve, rest) = (index / (BANDS * param::PER_BAND), index % (BANDS * param::PER_BAND));
                let (band, field) = (rest / param::PER_BAND, rest % param::PER_BAND);
                match field {
                    param::ON | param::TYPE => self.switch_band(curve, band),
                    _ => self.target_band(curve, band),
                }
            }
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        for frame in buffer.as_chunks_mut::<2>().0 {
            // Controls: glide, retune what moved, and finish fades.
            let scale_moving = !self.scale.is_settled();
            self.scale.next_value();
            for curve in 0..CURVES {
                for band in 0..BANDS {
                    let Some(b) = self.bands.get_mut(curve).and_then(|c| c.get_mut(band)) else {
                        continue;
                    };
                    if b.still() && !(scale_moving && b.active) {
                        continue;
                    }
                    let moving = b.controls_moving() || (scale_moving && b.active);
                    b.octave.next_value();
                    b.gain.next_value();
                    b.log_q.next_value();
                    b.amount.next_value();
                    let finished = b.pending && b.amount.is_settled() && b.amount.current() == 0.0;
                    let retune = if moving {
                        let due = b.retune_in == 0;
                        b.retune_in = if due { RETUNE_FRAMES - 1 } else { b.retune_in - 1 };
                        // The last step of a glide lands on the target.
                        due || !b.controls_moving()
                    } else {
                        false
                    };
                    if finished {
                        self.settle_band(curve, band);
                    } else if retune {
                        self.tune(curve, band);
                    }
                }
            }
            self.mode_fade.next_value();
            if self.mode_pending && self.mode_fade.is_settled() {
                self.settle_mode();
            }
            let level = f64::from(self.output.next_value()) * f64::from(self.mode_fade.current());

            // Audio: encode, run each side's bands in series, decode.
            let [l, r] = frame.map(f64::from);
            let mut sides = if self.mode == 2 { [(l + r) * 0.5, (l - r) * 0.5] } else { [l, r] };
            for (side, x) in sides.iter_mut().enumerate() {
                let curve = self.curve_of(side);
                for band in 0..BANDS {
                    let Some(b) = self.bands.get(curve).and_then(|c| c.get(band)) else {
                        continue;
                    };
                    if !b.active || b.flat {
                        continue;
                    }
                    let (amount, count) = (f64::from(b.amount.current()), b.count);
                    let Some(filters) = self.filters.get_mut(side).and_then(|f| f.get_mut(band)) else {
                        continue;
                    };
                    let y = filters.iter_mut().take(count).fold(*x, |v, f| f.process(v));
                    *x += amount * (y - *x);
                }
            }
            let [a, b] = sides;
            let (l, r) = if self.mode == 2 { (a + b, a - b) } else { (a, b) };
            frame[0] = (l * level) as f32;
            frame[1] = (r * level) as f32;
        }
    }
}

#[cfg(test)]
mod golden_tests;
pub mod panel;
#[cfg(test)]
mod tests;
