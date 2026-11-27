// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! EQ: a parametric equaliser of up to 24 bands (docs/spec/eq.md).
//!
//! The device has 24 band slots. A slot in use is a band: a low cut or high cut (12 or
//! 48 dB/octave), a low or high shelf, a bell, a notch, a tilt shelf or a flat tilt, with its
//! own frequency, gain and Q, and it can be switched off without leaving its slot. Each band
//! has a placement: Stereo runs it on both sides, Left or Right on one, Mid or Side on that part
//! of the mid/side encoding. The bands on left and right run first, in band order, then the mid
//! and side bands, also in band order, on state-variable filters ([`suhina_dsp::Svf`]) whose
//! responses are the cookbook ones. Scale multiplies the gains of the types with gain; Adaptive
//! Q narrows a bell as its gain grows past 6 dB and widens it below. The device adds no latency.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::spectral_dynamics::{self, SpectralDynamics};
use suhina_dsp::{BandDynamics, Smoother, Svf, SvfCoefficients};

/// Ramp for frequency, gain, Q, Scale and Output changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// How long a band takes to fade out or in when it is switched on or off or changes type, and
/// how long the output dips when the mode changes.
pub const FADE_SECONDS: f64 = 0.01;
/// While a band's frequency, gain or Q moves, its filters are retuned every this many frames.
const RETUNE_FRAMES: u32 = 16;
/// Adaptive Q multiplies a bell's Q by this to the power of its gain in decibels less
/// [`ADAPTIVE_Q_CENTER_DB`].
const ADAPTIVE_Q_BASE: f64 = 1.12;
const ADAPTIVE_Q_CENTER_DB: f64 = 6.0;
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
const GAIN: (f64, f64) = (-30.0, 30.0);
const Q_RANGE: (f64, f64) = (0.1, 18.0);

/// The band types, in [`param::TYPE`] order. The first eight are the ones the EQ always had,
/// at the same indices, so a saved or imported band keeps its type.
pub const TYPE_NAMES: [&str; 10] = [
    "Low Cut 48",
    "Low Cut 12",
    "Low Shelf",
    "Bell",
    "Notch",
    "High Shelf",
    "High Cut 12",
    "High Cut 48",
    "Tilt Shelf",
    "Flat Tilt",
];
/// The types whose gain shapes the response: the shelves, the bell and the two tilts. The cuts
/// and the notch have no gain.
pub const GAIN_TYPES: [usize; 5] = [2, 3, 5, 8, 9];
/// Where a band works in the stereo image, in [`param::PLACE`] order.
pub const PLACE_NAMES: [&str; 5] = ["Stereo", "Left", "Right", "Mid", "Side"];
/// A band's modes, in [`param::MODE`] order: Static as the EQ always was, Dynamic (its gain moves
/// with its level) or Spectral (only what stands out inside it moves).
pub const MODE_NAMES: [&str; 3] = ["Static", "Dynamic", "Spectral"];
/// What a Dynamic or Spectral band's detector hears, in [`param::TRIGGER`] order: the sound coming
/// into the band, or the device's key (its sidechain) through the band's own detector filter.
pub const TRIGGER_NAMES: [&str; 2] = ["Band", "Key"];
/// The readouts after the 24 bands' gain changes: the first Spectral band's live cut at
/// [`spectral_dynamics::LIVE_POINTS`] frequencies (`spectral_dynamics::LIVE_RANGE`).
pub const SPECTRAL_READOUTS: usize = spectral_dynamics::LIVE_POINTS;
/// The placements' short names, as the panel's placement button reads them.
pub const PLACE_LETTERS: [&str; 5] = ["St", "L", "R", "M", "S"];

/// The band slots. Each has an In Use flag apart from its On: a slot not in use is not shown
/// and does not run, and deleting a band frees its slot without renumbering the others.
pub const BANDS: usize = 24;
/// The slots in use in a new EQ (and in a project saved before there were 24): the first eight,
/// the bands the EQ always had.
pub const FIRST_BANDS: usize = 8;

/// Whether a type has a gain (one of [`GAIN_TYPES`]).
pub fn has_gain(type_index: usize) -> bool {
    GAIN_TYPES.contains(&type_index)
}

/// Parameter indices, in [`INFO`] order: the 24 bands' slots, then the global ones.
pub mod param {
    /// A band's parameters, in this order.
    pub const ON: usize = 0;
    pub const TYPE: usize = 1;
    pub const FREQUENCY: usize = 2;
    pub const GAIN: usize = 3;
    pub const Q: usize = 4;
    pub const IN_USE: usize = 5;
    pub const PLACE: usize = 6;
    pub const MODE: usize = 7;
    pub const RANGE: usize = 8;
    pub const THRESHOLD: usize = 9;
    pub const ATTACK: usize = 10;
    pub const RELEASE: usize = 11;
    pub const DETAIL: usize = 12;
    pub const TRIGGER: usize = 13;
    pub(crate) const PER_BAND: usize = 14;

    /// The index of `field` (one of the constants above) of band `band` (from 0).
    pub const fn band(band: usize, field: usize) -> usize {
        band * PER_BAND + field
    }

    pub const OUTPUT: usize = super::BANDS * PER_BAND;
    pub const SCALE: usize = OUTPUT + 1;
    pub const ADAPTIVE_Q: usize = OUTPUT + 2;
    /// The band being auditioned (1 to 24), or 0. A project never stores it: only
    /// `device.audition` sets it, through the engine, while a band is held to listen.
    pub const AUDITION: usize = OUTPUT + 3;
    pub(crate) const COUNT: usize = OUTPUT + 4;
}

/// The parameters of every band, then the globals. A band's ids are `a<n><Field>`: bands 1 to 8
/// keep the ids they always had, so a saved project, its automation and an import address
/// them unchanged.
macro_rules! params {
    ($($n:literal: $on:literal, $kind:literal, $frequency:literal, $in_use:literal;)*) => {
        [
            $(
                ParamInfo::in_group(concat!("Band ", $n)).toggle(
                    concat!("a", $n, "On"),
                    concat!("Band ", $n, " On"),
                    $on,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).choice(
                    concat!("a", $n, "Type"),
                    concat!("Band ", $n, " Type"),
                    &TYPE_NAMES,
                    $kind,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Frequency"),
                    concat!("Band ", $n, " Frequency"),
                    FREQUENCY,
                    $frequency,
                    Unit::Hertz,
                    Scale::Logarithmic,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Gain"),
                    concat!("Band ", $n, " Gain"),
                    GAIN,
                    0.0,
                    Unit::Decibels,
                    Scale::Linear,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Q"),
                    concat!("Band ", $n, " Q"),
                    Q_RANGE,
                    DEFAULT_Q,
                    Unit::None,
                    Scale::Logarithmic,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).toggle(
                    concat!("a", $n, "InUse"),
                    concat!("Band ", $n, " In Use"),
                    $in_use,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).choice(
                    concat!("a", $n, "Place"),
                    concat!("Band ", $n, " Placement"),
                    &PLACE_NAMES,
                    0,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).choice(
                    concat!("a", $n, "Mode"),
                    concat!("Band ", $n, " Mode"),
                    &MODE_NAMES,
                    0,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Range"),
                    concat!("Band ", $n, " Range"),
                    GAIN,
                    -6.0,
                    Unit::Decibels,
                    Scale::Linear,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Threshold"),
                    concat!("Band ", $n, " Threshold"),
                    (-60.0, 0.0),
                    -24.0,
                    Unit::Decibels,
                    Scale::Linear,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Attack"),
                    concat!("Band ", $n, " Attack"),
                    (0.0005, 0.2),
                    0.01,
                    Unit::Seconds,
                    Scale::Logarithmic,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Release"),
                    concat!("Band ", $n, " Release"),
                    (0.01, 2.0),
                    0.12,
                    Unit::Seconds,
                    Scale::Logarithmic,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).continuous(
                    concat!("a", $n, "Detail"),
                    concat!("Band ", $n, " Detail"),
                    (0.0, 1.0),
                    0.5,
                    Unit::Percent,
                    Scale::Linear,
                ),
                ParamInfo::in_group(concat!("Band ", $n)).choice(
                    concat!("a", $n, "Trigger"),
                    concat!("Band ", $n, " Trigger"),
                    &TRIGGER_NAMES,
                    0,
                ),
            )*
            ParamInfo::continuous("output", "Output", (-12.0, 12.0), 0.0, Unit::Decibels, Scale::Linear),
            ParamInfo::continuous("scale", "Scale", (-2.0, 2.0), 1.0, Unit::Percent, Scale::Linear),
            ParamInfo::toggle("adaptiveQ", "Adaptive Q", true),
            ParamInfo::integer("audition", "Audition", (0, BANDS as i32), 0, Unit::None),
        ]
    };
}

/// Bands 1 to 8 keep the defaults the EQ always had; 9 to 16 take the old curve B's, so a
/// project saved with two curves renders the same once its curve B lands there (docs/spec/eq.md ›
/// Projects saved with two curves); 17 to 24 are bells at 1 kHz.
static PARAMS: [ParamInfo; param::COUNT] = params![
    1: true, 2, 30.0, true;
    2: true, 3, 200.0, true;
    3: true, 3, 1000.0, true;
    4: true, 5, 5000.0, true;
    5: false, 3, 100.0, true;
    6: false, 3, 10000.0, true;
    7: false, 3, 5000.0, true;
    8: false, 6, 18000.0, true;
    9: false, 1, 40.0, false;
    10: false, 2, 200.0, false;
    11: true, 3, 100.0, false;
    12: true, 3, 500.0, false;
    13: true, 3, 2000.0, false;
    14: true, 3, 10000.0, false;
    15: false, 5, 5000.0, false;
    16: false, 6, 18000.0, false;
    17: true, 3, 1000.0, false;
    18: true, 3, 1000.0, false;
    19: true, 3, 1000.0, false;
    20: true, 3, 1000.0, false;
    21: true, 3, 1000.0, false;
    22: true, 3, 1000.0, false;
    23: true, 3, 1000.0, false;
    24: true, 3, 1000.0, false;
];

pub static INFO: DeviceInfo = DeviceInfo::new("eq", "EQ", Role::Effect, &PARAMS);

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
    TiltShelf,
    FlatTilt,
}

/// How far a flat tilt's four shelves sit from its frequency, in octaves: two low shelves below
/// and two high shelves above, so the response rises evenly over about ten octaves.
const FLAT_TILT_OCTAVES: [f64; 4] = [-3.75, -1.25, 1.25, 3.75];
/// The flat tilt's shelves' Q: gentle, so the steps of the four shelves blend into one slope.
const FLAT_TILT_Q: f64 = 0.4;

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
            8 => Self::TiltShelf,
            9 => Self::FlatTilt,
            _ => Self::Bell,
        }
    }

    /// Whether the type's gain shapes its response, so a band of it with no gain passes the
    /// sound unchanged.
    fn has_gain(self) -> bool {
        matches!(self, Self::LowShelf | Self::HighShelf | Self::Bell | Self::TiltShelf | Self::FlatTilt)
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
                let q = if adaptive { q * ADAPTIVE_Q_BASE.powf(gain.abs() - ADAPTIVE_Q_CENTER_DB) } else { q };
                out[0] = SvfCoefficients::bell(sr, frequency, q, gain);
                1
            }
            // Half the gain down below the frequency and half up above it: the band tips the
            // spectrum around its frequency by the whole gain.
            Self::TiltShelf => {
                out[0] = SvfCoefficients::low_shelf(sr, frequency, shelf_q(q), -gain / 2.0);
                out[1] = SvfCoefficients::high_shelf(sr, frequency, shelf_q(q), gain / 2.0);
                2
            }
            // An even slope: half the gain down far below the frequency, half up far above,
            // about the same number of dB in every octave between.
            Self::FlatTilt => {
                for (section, octaves) in out.iter_mut().zip(FLAT_TILT_OCTAVES) {
                    let at = (frequency * 2f64.powf(octaves)).clamp(FREQUENCY.0, sr * 0.45);
                    *section = if octaves < 0.0 {
                        SvfCoefficients::low_shelf(sr, at, FLAT_TILT_Q, -gain / 4.0)
                    } else {
                        SvfCoefficients::high_shelf(sr, at, FLAT_TILT_Q, gain / 4.0)
                    };
                }
                4
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
/// curve (docs/spec/eq.md › Behavior): the type's index in [`TYPE_NAMES`] order, the
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

/// Where a band runs: on both sides of the left/right signal, on one of them, or on the mid or
/// the side of its mid/side encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Stereo,
    Left,
    Right,
    Mid,
    Side,
}

impl Place {
    fn from_index(index: f64) -> Self {
        match index.round() as usize {
            1 => Self::Left,
            2 => Self::Right,
            3 => Self::Mid,
            4 => Self::Side,
            _ => Self::Stereo,
        }
    }

    /// Whether the band runs on the mid/side encoding rather than on left and right.
    fn mid_side(self) -> bool {
        matches!(self, Self::Mid | Self::Side)
    }

    /// Whether the band runs on channel `side` (0 left or mid, 1 right or side) of its signal.
    fn runs_on(self, side: usize) -> bool {
        match self {
            Self::Stereo => true,
            Self::Left | Self::Mid => side == 0,
            Self::Right | Self::Side => side == 1,
        }
    }
}

/// What a band does with its level (docs/spec/eq.md › Dynamic and Spectral bands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Static,
    Dynamic,
    Spectral,
}

impl Mode {
    /// The mode a band of `kind` runs for the Mode parameter: Dynamic for the types with gain,
    /// Spectral for the bell and the shelves; any other band is Static whatever it asks.
    fn of(index: f64, kind: Kind) -> Self {
        match index.round() as usize {
            1 if kind.has_gain() => Self::Dynamic,
            2 if matches!(kind, Kind::Bell | Kind::LowShelf | Kind::HighShelf) => Self::Spectral,
            _ => Self::Static,
        }
    }
}

/// One band: what it is now and its smoothed controls.
#[derive(Clone, Copy)]
struct Band {
    /// The type it runs; a new type waits until the band has faded out.
    kind: Kind,
    /// Where it runs; a new placement waits until the band has faded out, as a type does.
    place: Place,
    /// Frequency in octaves above 1 Hz, gain in dB, Q as its natural logarithm: each glides
    /// evenly in that unit.
    octave: Smoother,
    gain: Smoother,
    log_q: Smoother,
    /// How much of the band is heard: 0 bypasses it, 1 is the filter alone.
    amount: Smoother,
    /// Waiting to fade out before its type or placement changes or it stops.
    pending: bool,
    /// Whether it runs at all (on and in use, or still fading out).
    active: bool,
    sections: [SvfCoefficients; 4],
    count: usize,
    /// A band of a type with gain and no gain passes the sound unchanged, so it is skipped.
    flat: bool,
    retune_in: u32,
    /// Its mode now; a new one waits until the band has faded out, as a type does.
    mode: Mode,
    /// A Dynamic band's detector and gain computer, the gain change its filters are tuned to
    /// (dB) and its settings: range and threshold in dB, Detail 0 to 1.
    dynamics: BandDynamics,
    dynamic_db: f64,
    range: f64,
    threshold: f64,
    detail: f64,
    /// Whether its detector hears the device's key instead of the band's own sound.
    keyed: bool,
    /// Frames to the next retune of a Dynamic or Spectral band's moving gain.
    dynamic_in: u32,
}

impl Band {
    fn new() -> Self {
        Self {
            kind: Kind::Bell,
            place: Place::Stereo,
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
            mode: Mode::Static,
            dynamics: BandDynamics::new(),
            dynamic_db: 0.0,
            range: -6.0,
            threshold: -24.0,
            detail: 0.5,
            keyed: false,
            dynamic_in: 0,
        }
    }

    fn controls_moving(&self) -> bool {
        !(self.octave.is_settled() && self.gain.is_settled() && self.log_q.is_settled())
    }

    /// Nothing about the band is changing.
    fn still(&self) -> bool {
        !self.pending && self.amount.is_settled() && !self.controls_moving()
    }

    /// Whether the band is heard on the mid/side encoding now.
    fn runs_mid_side(&self) -> bool {
        self.active && !self.flat && self.place.mid_side()
    }
}

pub struct Eq {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    bands: [Band; BANDS],
    /// Each band's filters for the two channels of its signal (left and right, or mid and
    /// side).
    filters: [[[Svf; 4]; 2]; BANDS],
    output: Smoother,
    scale: Smoother,
    /// The band heard alone while it is auditioned (from 0), the filter that picks out the
    /// range it works on, and how much of the output is that range (0 is the whole EQ).
    audition_band: Option<usize>,
    audition_filters: [Svf; 2],
    audition_mix: Smoother,
    /// A band to listen to next: the output fades back to the whole EQ first, then over to it.
    audition_next: Option<usize>,
    /// The Spectral bands' short-time transform, run after the filters while a band in use is
    /// Spectral (decided when the device is built, since it adds [`spectral_dynamics::FRAME`]
    /// frames of delay the chain compensates).
    spectral: Box<SpectralDynamics>,
    spectral_on: bool,
}

impl Default for Eq {
    fn default() -> Self {
        Self::new()
    }
}

impl Eq {
    pub fn new() -> Self {
        Self::at(DEFAULT_SAMPLE_RATE)
    }

    /// The same, made for `sample_rate` (ADR 0004 › Amendment: a device is made at its rate);
    /// fails for a rate a device cannot run at. Allocates.
    pub fn with_rate(sample_rate: f64) -> Result<Self, DeviceError> {
        check_sample_rate(sample_rate).map(Self::at)
    }

    /// Everything for `sample_rate`, which has been checked.
    fn at(sample_rate: f64) -> Self {
        let mut eq = Self {
            sample_rate,
            params: ParamBank::new(&PARAMS),
            bands: [Band::new(); BANDS],
            filters: [[[Svf::default(); 4]; 2]; BANDS],
            output: Smoother::new(1.0, 1),
            scale: Smoother::new(1.0, 1),
            audition_band: None,
            audition_filters: [Svf::default(); 2],
            audition_mix: Smoother::new(0.0, 1),
            audition_next: None,
            spectral: Box::new(SpectralDynamics::new(sample_rate)),
            spectral_on: false,
        };
        eq.rebuild();
        eq
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn band_value(&self, band: usize, field: usize) -> f64 {
        self.value(param::band(band, field))
    }

    /// Whether the band should be heard: switched on, and its slot in use.
    fn band_wanted(&self, band: usize) -> bool {
        self.band_value(band, param::ON) >= 0.5 && self.band_value(band, param::IN_USE) >= 0.5
    }

    /// Sets the band's smoothers' targets from its parameters.
    fn target_band(&mut self, band: usize) {
        let octave = self.band_value(band, param::FREQUENCY).log2() as f32;
        let gain = self.band_value(band, param::GAIN) as f32;
        let log_q = self.band_value(band, param::Q).ln() as f32;
        if let Some(b) = self.bands.get_mut(band) {
            b.octave.set_target(octave);
            b.gain.set_target(gain);
            b.log_q.set_target(log_q);
        }
    }

    /// A band's dynamics settings from its parameters: range, threshold, Detail and the
    /// follower's times, for the band and its spectral parts.
    fn configure_dynamics(&mut self, band: usize) {
        let sr = self.sample_rate;
        let range = self.band_value(band, param::RANGE);
        let threshold = self.band_value(band, param::THRESHOLD);
        let detail = self.band_value(band, param::DETAIL);
        let keyed = self.band_value(band, param::TRIGGER) >= 0.5;
        let attack = self.band_value(band, param::ATTACK);
        let release = self.band_value(band, param::RELEASE);
        if let Some(b) = self.bands.get_mut(band) {
            b.range = range;
            b.threshold = threshold;
            b.detail = detail;
            b.keyed = keyed;
            b.dynamics.set_times(attack, release, sr);
        }
    }

    /// Points a band's detector, and its spectral parts, at the range it works on now: a band
    /// pass at a bell's frequency and Q, a low pass under a low shelf's frequency, a high pass
    /// over a high shelf's, a broad band pass for a tilt. The parts spread over the bell's width,
    /// or the three octaves a shelf lifts.
    fn aim_dynamics(&mut self, band: usize, frequency: f64, q: f64) {
        let sr = self.sample_rate;
        let Some(b) = self.bands.get_mut(band) else { return };
        if b.mode == Mode::Static {
            return;
        }
        let top = sr * 0.45;
        let frequency = frequency.min(top);
        let detector = match b.kind {
            Kind::LowShelf => SvfCoefficients::lowpass(sr, frequency, DEFAULT_Q),
            Kind::HighShelf => SvfCoefficients::highpass(sr, frequency, DEFAULT_Q),
            Kind::Bell => SvfCoefficients::bandpass(sr, frequency, q),
            _ => SvfCoefficients::bandpass(sr, frequency, 0.3),
        };
        b.dynamics.set_detector(detector);
    }

    /// Hands a Spectral band's settings to the transform (anything else is off there).
    fn give_spectral(&mut self, band: usize, frequency: f64, q: f64) {
        let scale = f64::from(self.scale.current());
        let Some(b) = self.bands.get(band) else { return };
        let shape = match b.kind {
            Kind::LowShelf => spectral_dynamics::Shape::LowShelf,
            Kind::HighShelf => spectral_dynamics::Shape::HighShelf,
            _ => spectral_dynamics::Shape::Bell,
        };
        let place = match b.place {
            Place::Stereo => spectral_dynamics::Place::Stereo,
            Place::Left => spectral_dynamics::Place::Left,
            Place::Right => spectral_dynamics::Place::Right,
            Place::Mid => spectral_dynamics::Place::Mid,
            Place::Side => spectral_dynamics::Place::Side,
        };
        let settings = spectral_dynamics::Settings {
            on: b.active && b.mode == Mode::Spectral,
            shape,
            frequency,
            q,
            // Scale multiplies the range as it does every gain.
            range: b.range * scale,
            threshold: b.threshold,
            attack: self.band_value(band, param::ATTACK),
            release: self.band_value(band, param::RELEASE),
            density: b.detail,
            tilt: true,
            place,
            keyed: b.keyed,
        };
        self.spectral.set(band, settings);
    }

    /// Recomputes a band's filters from its smoothed controls and gives them to its filters.
    fn tune(&mut self, band: usize) {
        let sr = self.sample_rate;
        let scale = f64::from(self.scale.current());
        let adaptive = self.value(param::ADAPTIVE_Q) >= 0.5;
        let Some(b) = self.bands.get_mut(band) else {
            return;
        };
        let frequency = 2f64.powf(f64::from(b.octave.current()));
        // A Dynamic band's gain moves by its gain change; any other band's change is 0.
        let gain = (f64::from(b.gain.current()) + b.dynamic_db) * scale;
        let q = f64::from(b.log_q.current()).exp();
        let (sections, count) = b.kind.sections(sr, frequency, gain, q, adaptive);
        let was_flat = b.flat;
        b.sections = sections;
        b.count = count;
        // A band with no gain passes the sound unchanged, unless its gain can still move.
        b.flat = b.kind.has_gain() && gain.abs() < 1e-9 && b.mode == Mode::Static;
        let wakes = was_flat && !b.flat;
        if let Some(filters) = self.filters.get_mut(band) {
            for side in filters.iter_mut() {
                for (filter, c) in side.iter_mut().zip(sections) {
                    filter.set(c);
                }
            }
            if wakes {
                // Skipped while flat, its state is stale: it starts from silence, as its gain
                // rises from nothing.
                filters.iter_mut().flatten().for_each(Svf::reset);
            }
        }
        if self.audition_band == Some(band) {
            self.tune_audition();
        }
        self.aim_dynamics(band, frequency, q);
        // Every band tells the transform whether it is Spectral now, so one that stopped being
        // Spectral (or stopped) is off there too.
        self.give_spectral(band, frequency, q);
    }

    /// The band [`param::AUDITION`] names, when it names one in use.
    fn auditioned(&self) -> Option<usize> {
        let value = self.value(param::AUDITION).round();
        (value >= 1.0)
            .then(|| value as usize - 1)
            .filter(|&band| band < BANDS && self.band_value(band, param::IN_USE) >= 0.5)
    }

    /// Tunes the audition filter to the range the auditioned band works on: a band pass at its
    /// frequency and Q for a bell, a notch, a shelf or a tilt (without its boost or cut); for a
    /// low cut what it removes (a low pass at its frequency), for a high cut a high pass.
    fn tune_audition(&mut self) {
        let Some(band) = self.audition_band else { return };
        let Some(b) = self.bands.get(band) else { return };
        let sr = self.sample_rate;
        let frequency = 2f64.powf(f64::from(b.octave.current())).min(sr * 0.49);
        let q = f64::from(b.log_q.current()).exp();
        let coefficients = match b.kind {
            Kind::LowCut12 | Kind::LowCut48 => SvfCoefficients::lowpass(sr, frequency, DEFAULT_Q),
            Kind::HighCut12 | Kind::HighCut48 => SvfCoefficients::highpass(sr, frequency, DEFAULT_Q),
            Kind::Bell | Kind::Notch => SvfCoefficients::bandpass(sr, frequency, q),
            _ => SvfCoefficients::bandpass(sr, frequency, DEFAULT_Q),
        };
        for filter in &mut self.audition_filters {
            filter.set(coefficients);
        }
    }

    /// The auditioned band's range picked out of the input `[l, r]`, on its placement: both sides
    /// for Stereo, one side for Left or Right (the other silent), or the mid or the side alone.
    fn audition(&mut self, [l, r]: [f64; 2], place: Place) -> [f64; 2] {
        let [left, right] = &mut self.audition_filters;
        match place {
            Place::Stereo => [left.process(l), right.process(r)],
            Place::Left => [left.process(l), 0.0],
            Place::Right => [0.0, right.process(r)],
            Place::Mid => {
                let m = left.process((l + r) * 0.5);
                [m, m]
            }
            Place::Side => {
                let s = right.process((l - r) * 0.5);
                [s, -s]
            }
        }
    }

    /// Clears the state of a band's filters.
    fn clear(&mut self, band: usize) {
        if let Some(filters) = self.filters.get_mut(band) {
            filters.iter_mut().flatten().for_each(Svf::reset);
        }
    }

    /// Every smoother, band and filter for the current sample rate and parameters, the state
    /// cleared.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let fade = Smoother::ramp_frames(FADE_SECONDS, sr);
        self.output = Smoother::new(db_to_gain(self.value(param::OUTPUT) as f32), ramp);
        self.scale = Smoother::new(self.value(param::SCALE) as f32, ramp);
        self.filters = [[[Svf::default(); 4]; 2]; BANDS];
        self.audition_band = self.auditioned();
        self.audition_next = None;
        self.spectral_on = (0..BANDS).any(|band| {
            self.band_value(band, param::IN_USE) >= 0.5
                && Mode::of(self.band_value(band, param::MODE), Kind::from_index(self.band_value(band, param::TYPE)))
                    == Mode::Spectral
        });
        self.spectral.reset();
        self.audition_filters = [Svf::default(); 2];
        self.audition_mix = Smoother::new(if self.audition_band.is_some() { 1.0 } else { 0.0 }, fade);
        for band in 0..BANDS {
            let on = self.band_wanted(band);
            let kind = Kind::from_index(self.band_value(band, param::TYPE));
            let place = Place::from_index(self.band_value(band, param::PLACE));
            let octave = self.band_value(band, param::FREQUENCY).log2() as f32;
            let gain = self.band_value(band, param::GAIN) as f32;
            let log_q = self.band_value(band, param::Q).ln() as f32;
            let mode = Mode::of(self.band_value(band, param::MODE), kind);
            if let Some(b) = self.bands.get_mut(band) {
                *b = Band {
                    kind,
                    place,
                    mode,
                    octave: Smoother::new(octave, ramp),
                    gain: Smoother::new(gain, ramp),
                    log_q: Smoother::new(log_q, ramp),
                    amount: Smoother::new(if on { 1.0 } else { 0.0 }, fade),
                    pending: false,
                    active: on,
                    ..Band::new()
                };
            }
            self.configure_dynamics(band);
            self.tune(band);
        }
    }

    /// After a band's On, In Use, Type or Placement changed: fade it to the new state.
    fn switch_band(&mut self, band: usize) {
        let on = self.band_wanted(band);
        let kind = Kind::from_index(self.band_value(band, param::TYPE));
        let place = Place::from_index(self.band_value(band, param::PLACE));
        let mode = Mode::of(self.band_value(band, param::MODE), kind);
        let Some(b) = self.bands.get_mut(band) else {
            return;
        };
        if kind != b.kind || place != b.place || mode != b.mode || !on {
            // Fade out first; the new type, placement or the stop comes when it is silent.
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
                self.tune(band);
            }
        }
    }

    /// A band that has faded out: takes its new type and placement, and fades in again if it
    /// is on.
    fn settle_band(&mut self, band: usize) {
        let on = self.band_wanted(band);
        let kind = Kind::from_index(self.band_value(band, param::TYPE));
        let place = Place::from_index(self.band_value(band, param::PLACE));
        let mode = Mode::of(self.band_value(band, param::MODE), kind);
        if let Some(b) = self.bands.get_mut(band) {
            b.pending = false;
            b.kind = kind;
            b.place = place;
            // The band is silent here, after it stopped or before a new type, placement or mode:
            // its dynamics start again at rest, so a band never fades in already moving from
            // what it heard before.
            b.mode = mode;
            b.dynamic_db = 0.0;
            b.dynamics.reset();
            b.active = on;
            if on {
                b.amount.set_target(1.0);
            }
        }
        self.clear(band);
        self.tune(band);
    }

    /// Runs the bands of one signal (left and right, or mid and side when `mid_side`) on
    /// `sides`, in band order. A Dynamic band's detector hears the sound coming into it, and its
    /// gain change retunes it every [`RETUNE_FRAMES`]; a Spectral band's own filter runs here and
    /// its dynamics in the transform after the filters.
    fn run_bands(&mut self, sides: &mut [f64; 2], key: Option<[f64; 2]>, mid_side: bool) {
        for band in 0..BANDS {
            let Some(b) = self.bands.get_mut(band) else { continue };
            if !b.active || b.flat || b.place.mid_side() != mid_side {
                continue;
            }
            let listen = [b.place.runs_on(0), b.place.runs_on(1)];
            let mut retune = false;
            if b.mode != Mode::Static {
                let due = b.dynamic_in == 0;
                b.dynamic_in = if due { RETUNE_FRAMES - 1 } else { b.dynamic_in - 1 };
                if b.mode == Mode::Dynamic {
                    // A keyed band's detector hears the key (on the same part of the signal).
                    let heard = if b.keyed { key.unwrap_or(*sides) } else { *sides };
                    let change = b.dynamics.process(heard, listen, b.threshold, b.range);
                    retune = due && (change - b.dynamic_db).abs() > 0.01;
                    if retune {
                        b.dynamic_db = change;
                    }
                }
            }
            if retune {
                self.tune(band);
            }
            let Some(b) = self.bands.get_mut(band) else { continue };
            let Some(filters) = self.filters.get_mut(band) else { continue };
            let amount = f64::from(b.amount.current());
            for ((side, x), chain) in sides.iter_mut().enumerate().zip(filters.iter_mut()) {
                if !b.place.runs_on(side) {
                    continue;
                }
                let y = chain.iter_mut().take(b.count).fold(*x, |v, f| f.process(v));
                *x += amount * (y - *x);
            }
        }
    }

    /// Each band's gain change now, in dB: a Dynamic band's; a Spectral band's deepest; 0 for a
    /// Static band.
    fn gain_changes(&self) -> [f32; BANDS] {
        std::array::from_fn(|band| {
            self.bands.get(band).map_or(0.0, |b| match b.mode {
                Mode::Static => 0.0,
                Mode::Dynamic => b.dynamic_db as f32,
                Mode::Spectral => {
                    self.spectral.live(band).iter().copied().fold(0.0f32, |m, g| if g.abs() > m.abs() { g } else { m })
                }
            })
        })
    }
}

impl Device for Eq {
    fn info(&self) -> &DeviceInfo {
        &INFO
    }

    /// Each band's gain change now, in band order, for the panel's live curve.
    fn readouts(&self) -> Option<[f32; suhina_device_api::MAX_READOUTS]> {
        let changes = self.gain_changes();
        // After the 24 bands, the first Spectral band's live cut (`SPECTRAL_READOUTS`).
        let live =
            self.bands.iter().position(|b| b.active && b.mode == Mode::Spectral).map(|band| self.spectral.live(band));
        Some(std::array::from_fn(|i| match i.checked_sub(BANDS) {
            None => changes.get(i).copied().unwrap_or(0.0),
            Some(point) => live.and_then(|l| l.get(point).copied()).unwrap_or(0.0),
        }))
    }

    /// The transform's delay while a band in use is Spectral; none otherwise.
    fn latency(&self) -> u32 {
        if self.spectral_on { SpectralDynamics::latency() as u32 } else { 0 }
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
                for band in 0..BANDS {
                    self.tune(band);
                }
            }
            param::AUDITION => {
                // A new band to listen to starts its filter from silence and fades the output
                // over to it; letting go fades back to the whole EQ.
                // Another band while one is heard fades out first and swaps at silence.
                match self.auditioned() {
                    Some(band) if self.audition_band == Some(band) => {
                        self.audition_next = None;
                        self.audition_mix.set_target(1.0);
                    }
                    Some(band) if self.audition_band.is_none() || self.audition_mix.current() == 0.0 => {
                        self.audition_next = None;
                        self.audition_filters = [Svf::default(); 2];
                        self.audition_band = Some(band);
                        self.tune_audition();
                        self.audition_mix.set_target(1.0);
                    }
                    Some(band) => {
                        self.audition_next = Some(band);
                        self.audition_mix.set_target(0.0);
                    }
                    None => {
                        self.audition_next = None;
                        self.audition_mix.set_target(0.0);
                    }
                }
            }
            _ => {
                let (band, field) = (index / param::PER_BAND, index % param::PER_BAND);
                // Deleting the band being heard alone fades back to the whole EQ, and the
                // audition is let go so a rebuild cannot bring it back.
                if field == param::IN_USE && value < 0.5 && self.audition_band == Some(band) {
                    let _ = self.params.set(param::AUDITION, 0.0);
                    self.audition_next = None;
                    self.audition_mix.set_target(0.0);
                }
                match field {
                    param::ON | param::TYPE | param::IN_USE | param::PLACE | param::MODE => self.switch_band(band),
                    param::RANGE
                    | param::THRESHOLD
                    | param::ATTACK
                    | param::RELEASE
                    | param::DETAIL
                    | param::TRIGGER => {
                        self.configure_dynamics(band);
                        self.tune(band);
                    }
                    _ => self.target_band(band),
                }
            }
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        self.run(buffer, None);
    }

    /// The same, with the device's key: a band whose Trigger is Key hears it (a frame the key does
    /// not cover is silence, as the compressor reads it).
    fn process_keyed(&mut self, _context: &ProcessContext, key: &[f32], buffer: &mut [f32]) {
        self.run(buffer, Some(key));
    }
}

impl Eq {
    /// Every frame of `buffer`, with the device's key when it has one. Never allocates.
    fn run(&mut self, buffer: &mut [f32], key: Option<&[f32]>) {
        let keys = key.map(|k| k.as_chunks::<2>().0);
        for (index, frame) in buffer.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let key_frame = keys.map(|k| k.get(index).map_or([0.0, 0.0], |f| [f64::from(f[0]), f64::from(f[1])]));
            // Controls: glide, retune what moved, and finish fades.
            let scale_moving = !self.scale.is_settled();
            self.scale.next_value();
            let mut mid_side = false;
            for band in 0..BANDS {
                let Some(b) = self.bands.get_mut(band) else {
                    continue;
                };
                if b.still() && !(scale_moving && b.active) {
                    mid_side |= b.runs_mid_side();
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
                    self.settle_band(band);
                } else if retune {
                    self.tune(band);
                }
                mid_side |= self.bands.get(band).is_some_and(Band::runs_mid_side);
            }
            let level = f64::from(self.output.next_value());
            let mix = f64::from(self.audition_mix.next_value());
            let input = frame.map(f64::from);

            // Audio: the bands on left and right first, then, only while a mid or side band
            // runs, the mid/side encoding with its bands, decoded again.
            let mut sides = frame.map(f64::from);
            self.run_bands(&mut sides, key_frame, false);
            if mid_side {
                let [l, r] = sides;
                let mut encoded = [(l + r) * 0.5, (l - r) * 0.5];
                let key_encoded = key_frame.map(|[kl, kr]| [(kl + kr) * 0.5, (kl - kr) * 0.5]);
                self.run_bands(&mut encoded, key_encoded, true);
                let [m, s] = encoded;
                sides = [m + s, m - s];
            }
            if self.spectral_on {
                sides = self.spectral.process(sides, key_frame);
            }
            let mut out = [sides[0] * level, sides[1] * level];
            if mix > 0.0
                && let Some(place) = self.audition_band.and_then(|band| self.bands.get(band)).map(|b| b.place)
            {
                let heard = self.audition(input, place);
                for (o, h) in out.iter_mut().zip(heard) {
                    *o += mix * (h - *o);
                }
            } else if self.audition_mix.is_settled() && mix == 0.0 {
                match self.audition_next.take() {
                    Some(band) => {
                        self.audition_filters = [Svf::default(); 2];
                        self.audition_band = Some(band);
                        self.tune_audition();
                        self.audition_mix.set_target(1.0);
                    }
                    None => self.audition_band = None,
                }
            }
            frame[0] = out[0] as f32;
            frame[1] = out[1] as f32;
        }
    }
}

#[cfg(test)]
mod golden_tests;
pub mod panel;
#[cfg(test)]
mod tests;
