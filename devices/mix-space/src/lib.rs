// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mix Space: a view, not an effect (`docs/spec/mix-space.md`). The sound passes through it
//! untouched; its panel draws every track that plays through the track it is on as a line in a
//! box: left to right where the track sits in the stereo field, bottom to top how loud it is,
//! front to back its frequencies. Its parameters only choose what the panel shows.

use suhina_device_api::{
    Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit, check_sample_rate,
};

/// Parameter indices, in [`INFO`] order.
pub mod param {
    /// Where the tracks are tapped: [`super::point::POST_FADER`] or [`super::point::PRE_FADER`].
    pub const POINT: usize = 0;
    /// How muted tracks show: [`super::muted::DIM`] or [`super::muted::HIDE`].
    pub const MUTED: usize = 1;
    /// Which overlaps are marked: [`super::overlaps::OFF`], [`super::overlaps::MARK`] or
    /// [`super::overlaps::ONLY`].
    pub const OVERLAPS: usize = 2;
    /// How close two tracks' levels must be, in dB, for their meeting to count as an overlap.
    pub const OVERLAP_RANGE: usize = 3;
    /// How fast the lines fall, in seconds.
    pub const RELEASE: usize = 4;
    /// How each track is drawn: [`super::draw::MESH`], [`super::draw::SURFACE`] or [`super::draw::LINES`].
    pub const DRAW: usize = 5;
    /// How much the view lifts the highs, in dB per octave above 1 kHz (and lowers the lows).
    pub const TILT: usize = 6;
    /// How the levels move: [`super::response::FAST`] or [`super::response::SLOW`], a long
    /// average.
    pub const RESPONSE: usize = 7;
    /// The level at the box's floor, in dBFS: quieter sound is not drawn.
    pub const FLOOR: usize = 8;
    /// How finely each track's spectrum is drawn: its envelope over 1/3, 1/6 or 1/12 octave,
    /// or every 1/24-octave band.
    pub const DETAIL: usize = 9;
    pub(crate) const COUNT: usize = 10;
}

/// Options of the point parameter.
pub mod point {
    pub const POST_FADER: f64 = 0.0;
    pub const PRE_FADER: f64 = 1.0;
}

/// Options of the muted parameter.
pub mod muted {
    pub const DIM: f64 = 0.0;
    pub const HIDE: f64 = 1.0;
}

/// Options of the draw parameter.
pub mod draw {
    /// Every track blended into one landscape drawn as a see-through net, coloured by the
    /// tracks that make it.
    pub const MESH: f64 = 0.0;
    /// The same landscape as a solid, lit surface.
    pub const SURFACE: f64 = 1.0;
    /// Each track as one line through its bands.
    pub const LINES: f64 = 2.0;
}

/// Options of the response parameter.
pub mod response {
    /// Rises in 10 ms and falls at the release.
    pub const FAST: f64 = 0.0;
    /// Rises and falls over about 3 s: the tonal balance of a whole section.
    pub const SLOW: f64 = 1.0;
}

/// Options of the overlaps parameter.
pub mod overlaps {
    pub const OFF: f64 = 0.0;
    pub const MARK: f64 = 1.0;
    pub const ONLY: f64 = 2.0;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::choice("point", "Tap", &["Post-fader", "Pre-fader"], 0),
    ParamInfo::choice("muted", "Muted Tracks", &["Dim", "Hide"], 0),
    ParamInfo::choice("overlaps", "Overlaps", &["Off", "Mark", "Only Overlaps"], 0),
    ParamInfo::continuous("overlapRange", "Overlap Range", (1.0, 12.0), 6.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("release", "Release", (0.05, 2.0), 0.3, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::choice("draw", "Draw", &["Mesh", "Surface", "Lines"], 0),
    ParamInfo::continuous("tilt", "Tilt", (0.0, 6.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::choice("response", "Response", &["Fast", "Slow"], 0),
    ParamInfo::continuous("floor", "Floor", (-120.0, -12.0), -72.0, Unit::Decibels, Scale::Linear),
    ParamInfo::choice("detail", "Detail", &["1/3 Octave", "1/6 Octave", "1/12 Octave", "Every Band"], 0),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "mix-space", name: "Mix Space", role: Role::Effect, params: &PARAMS };

/// The device: its parameters, and the sound passed on as it came.
pub struct MixSpace {
    params: ParamBank<{ param::COUNT }>,
}

impl Default for MixSpace {
    fn default() -> Self {
        Self::new()
    }
}

impl MixSpace {
    pub fn new() -> Self {
        Self { params: ParamBank::new(&PARAMS) }
    }
}

impl Device for MixSpace {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        check_sample_rate(sample_rate)?;
        Ok(())
    }

    fn reset(&mut self) {}

    fn set_param(&mut self, index: usize, value: f64) {
        self.params.set(index, value);
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, _context: &ProcessContext, _buffer: &mut [f32]) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sound_passes_through_untouched_and_the_parameters_hold() {
        let mut device = MixSpace::new();
        device.prepare(44_100.0).expect("prepare");
        let mut buffer = [0.5, -0.25, 1.5, f32::MIN_POSITIVE];
        let before = buffer;
        device.process(&ProcessContext::default(), &mut buffer);
        assert_eq!(buffer, before);
        device.set_param(param::POINT, point::PRE_FADER);
        assert_eq!(device.param(param::POINT), Some(point::PRE_FADER));
        assert_eq!(device.param(param::OVERLAP_RANGE), Some(6.0));
        device.set_param(param::OVERLAP_RANGE, 99.0);
        assert_eq!(device.param(param::OVERLAP_RANGE), Some(12.0));
        assert!(device.prepare(f64::NAN).is_err());
    }

    #[test]
    fn it_meets_the_device_contract() {
        let failures = suhina_device_api::conformance::check(&|| Box::new(MixSpace::new()));
        assert!(failures.is_empty(), "{failures:?}");
    }
}
