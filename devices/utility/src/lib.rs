// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Utility: gain, balance, stereo width and mid/side, mono and bass mono, phase inversion,
//! channel selection, DC removal and mute, in one device.
//!
//! Signal path, in order: DC filter, then one 2×2 matrix that does channel selection, phase
//! inversion, width, mid/side and mono at once, then bass mono, then gain and balance (mute
//! included). Bass mono listen plays only the band below the bass mono frequency, as bass mono
//! leaves it, so the crossover can be set by ear. Every control is smoothed over [`RAMP_SECONDS`], toggles too, so automating any
//! of them does not click; the gain glides instead along its knob ([`GAIN_GLIDE_SECONDS`]), so a
//! jump in it takes a few milliseconds whatever its size. With every parameter at its default the
//! output equals the input exactly.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::{SILENCE_DB, db_to_gain_or_silence};
use suhina_dsp::{DcBlocker, Glide, LinkwitzRiley, Smoother};

/// How long a parameter change takes to arrive.
pub const RAMP_SECONDS: f64 = 0.02;
/// The time constant of the gain's glide along its knob: two of them in a row make a jump an
/// S-shaped move whose middle 80 % takes 2.2 ms, about 6 ms in all (`docs/spec/utility.md`).
pub const GAIN_GLIDE_SECONDS: f64 = 0.000_664;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const GAIN: usize = 0;
    pub const BALANCE: usize = 1;
    pub const WIDTH: usize = 2;
    pub const MID_SIDE: usize = 3;
    pub const MONO: usize = 4;
    pub const BASS_MONO: usize = 5;
    pub const BASS_MONO_FREQUENCY: usize = 6;
    pub const INVERT_LEFT: usize = 7;
    pub const INVERT_RIGHT: usize = 8;
    pub const CHANNEL: usize = 9;
    pub const DC_FILTER: usize = 10;
    pub const MUTE: usize = 11;
    pub const BASS_MONO_LISTEN: usize = 12;
    pub(crate) const COUNT: usize = 13;
}

/// Options of the channel parameter.
pub mod channel {
    pub const STEREO: f64 = 0.0;
    pub const LEFT: f64 = 1.0;
    pub const RIGHT: f64 = 2.0;
    pub const SWAP: f64 = 3.0;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("gain", "Gain", (SILENCE_DB as f64, 35.0), 0.0, Unit::Decibels, Scale::Gain),
    ParamInfo::continuous("balance", "Balance", (-1.0, 1.0), 0.0, Unit::Balance, Scale::Linear),
    ParamInfo::continuous("width", "Width", (0.0, 4.0), 1.0, Unit::Percent, Scale::Linear),
    // -1 keeps only the mid signal, 1 only the side signal.
    ParamInfo::continuous("midSide", "Mid/Side", (-1.0, 1.0), 0.0, Unit::None, Scale::Linear),
    ParamInfo::toggle("mono", "Mono", false),
    ParamInfo::toggle("bassMono", "Bass Mono", false),
    ParamInfo::continuous(
        "bassMonoFrequency",
        "Bass Mono Frequency",
        (50.0, 500.0),
        120.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::toggle("invertLeft", "Invert Left", false),
    ParamInfo::toggle("invertRight", "Invert Right", false),
    ParamInfo::choice("channel", "Channel", &["Stereo", "Left", "Right", "Swap"], 0),
    ParamInfo::toggle("dcFilter", "DC Filter", false),
    ParamInfo::toggle("mute", "Mute", false),
    ParamInfo::toggle("bassMonoListen", "Bass Mono Listen", false),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "utility", name: "Utility", role: Role::Effect, params: &PARAMS };

pub struct Utility {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    /// The channel, inversion, width and mono matrix: `[ll, lr, rl, rr]`.
    matrix: [Smoother; 4],
    /// Where the gain's knob is, gliding to where it is set (0 to 1, `Scale::Gain`).
    gain: Glide,
    /// The gain at the glide's last value, kept while it rests.
    gain_factor: f32,
    /// Balance and mute, per side.
    balance_left: Smoother,
    balance_right: Smoother,
    dc_mix: Smoother,
    bass_mix: Smoother,
    listen_mix: Smoother,
    dc: [DcBlocker; 2],
    crossover: [LinkwitzRiley; 2],
}

impl Default for Utility {
    fn default() -> Self {
        Self::new()
    }
}

impl Utility {
    pub fn new() -> Self {
        let params = ParamBank::new(&PARAMS);
        let sr = DEFAULT_SAMPLE_RATE;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let frequency = params.get(param::BASS_MONO_FREQUENCY);
        let mut utility = Self {
            sample_rate: sr,
            params,
            matrix: [Smoother::new(0.0, ramp); 4],
            gain: Glide::new(PARAMS[param::GAIN].to_normalized(0.0), GAIN_GLIDE_SECONDS, sr),
            gain_factor: 1.0,
            balance_left: Smoother::new(1.0, ramp),
            balance_right: Smoother::new(1.0, ramp),
            dc_mix: Smoother::new(0.0, ramp),
            bass_mix: Smoother::new(0.0, ramp),
            listen_mix: Smoother::new(0.0, ramp),
            dc: [DcBlocker::new(sr); 2],
            crossover: [LinkwitzRiley::new(sr, frequency); 2],
        };
        utility.retarget();
        utility.reset();
        utility
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn on(&self, index: usize) -> bool {
        self.params.on(index)
    }

    fn smoothers(&mut self) -> impl Iterator<Item = &mut Smoother> {
        let rest = [
            &mut self.balance_left,
            &mut self.balance_right,
            &mut self.dc_mix,
            &mut self.bass_mix,
            &mut self.listen_mix,
        ];
        self.matrix.iter_mut().chain(rest)
    }

    /// Sets every smoother's target from the parameter values.
    fn retarget(&mut self) {
        let [ll, lr, rl, rr] = self.target_matrix();
        let (left, right) = self.target_balance();
        self.gain.set_target(PARAMS[param::GAIN].to_normalized(self.value(param::GAIN)));
        let dc = if self.on(param::DC_FILTER) { 1.0 } else { 0.0 };
        let bass = if self.on(param::BASS_MONO) { 1.0 } else { 0.0 };
        let listen = if self.on(param::BASS_MONO_LISTEN) { 1.0 } else { 0.0 };
        // A filter that has been fully off starts again from silence, not from stale state.
        if dc > 0.0 && self.dc_mix.current() == 0.0 && self.dc_mix.is_settled() {
            self.dc.iter_mut().for_each(DcBlocker::reset);
        }
        let idle = |s: &Smoother| s.current() == 0.0 && s.is_settled();
        if bass + listen > 0.0 && idle(&self.bass_mix) && idle(&self.listen_mix) {
            self.crossover.iter_mut().for_each(LinkwitzRiley::reset);
        }
        let targets = [ll, lr, rl, rr, left, right, dc, bass, listen];
        for (smoother, target) in self.smoothers().zip(targets) {
            smoother.set_target(target);
        }
    }

    /// `[ll, lr, rl, rr]` for: channel selection, then phase inversion, then width, mid/side
    /// and mono.
    fn target_matrix(&self) -> [f32; 4] {
        let selected: [f32; 4] = match self.value(param::CHANNEL) {
            c if c == channel::LEFT => [1.0, 0.0, 1.0, 0.0],
            c if c == channel::RIGHT => [0.0, 1.0, 0.0, 1.0],
            c if c == channel::SWAP => [0.0, 1.0, 1.0, 0.0],
            _ => [1.0, 0.0, 0.0, 1.0],
        };
        let sign = |index| if self.on(index) { -1.0f32 } else { 1.0 };
        let (left_sign, right_sign) = (sign(param::INVERT_LEFT), sign(param::INVERT_RIGHT));
        let [a, b, c, d] = selected;
        let [a, b, c, d] = [a * left_sign, b * left_sign, c * right_sign, d * right_sign];

        let mid_side = self.value(param::MID_SIDE) as f32;
        let mid = (1.0 - mid_side).min(1.0);
        let mono = if self.on(param::MONO) { 0.0 } else { 1.0 };
        let side = self.value(param::WIDTH) as f32 * (1.0 + mid_side).min(1.0) * mono;
        // In left/right terms, scaling mid by `mid` and side by `side` is this matrix.
        let (same, cross) = ((mid + side) / 2.0, (mid - side) / 2.0);
        [same * a + cross * c, same * b + cross * d, cross * a + same * c, cross * b + same * d]
    }

    /// The factor of each side for balance and mute (the gain glides apart).
    fn target_balance(&self) -> (f32, f32) {
        let on = if self.on(param::MUTE) { 0.0 } else { 1.0 };
        let balance = self.value(param::BALANCE) as f32;
        (on * (1.0 - balance).min(1.0), on * (1.0 + balance).min(1.0))
    }

    /// The gain for this sample: the glide's knob position through the knob's law.
    #[inline]
    fn next_gain(&mut self) -> f32 {
        if !self.gain.is_settled() || self.gain_factor.is_nan() {
            let position = self.gain.next_value();
            self.gain_factor = db_to_gain_or_silence(PARAMS[param::GAIN].from_normalized(position) as f32);
        }
        self.gain_factor
    }
}

impl Device for Utility {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        let sr = check_sample_rate(sample_rate)?;
        self.sample_rate = sr;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        self.smoothers().for_each(|s| s.set_ramp(ramp));
        self.gain.set_time(GAIN_GLIDE_SECONDS, sr);
        self.dc = [DcBlocker::new(sr); 2];
        let frequency = self.value(param::BASS_MONO_FREQUENCY);
        self.crossover = [LinkwitzRiley::new(sr, frequency); 2];
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        self.dc.iter_mut().for_each(DcBlocker::reset);
        self.crossover.iter_mut().for_each(LinkwitzRiley::reset);
        self.smoothers().for_each(|s| s.reset(s.target()));
        self.gain.reset(self.gain.target());
        self.gain_factor = f32::NAN;
        self.next_gain();
    }

    fn set_param(&mut self, index: usize, value: f64) {
        let Some(value) = self.params.set(index, value) else { return };
        if index == param::BASS_MONO_FREQUENCY {
            let (sr, frequency) = (self.sample_rate, value);
            self.crossover.iter_mut().for_each(|c| c.set_frequency(sr, frequency));
        }
        self.retarget();
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        for [left, right] in buffer.as_chunks_mut::<2>().0 {
            let (mut l, mut r) = (*left, *right);

            let dc_mix = self.dc_mix.next_value();
            if dc_mix > 0.0 {
                let [dc_left, dc_right] = &mut self.dc;
                l += (dc_left.process(f64::from(l)) as f32 - l) * dc_mix;
                r += (dc_right.process(f64::from(r)) as f32 - r) * dc_mix;
            }

            let [ll, lr, rl, rr] = self.matrix.each_mut().map(Smoother::next_value);
            (l, r) = (ll * l + lr * r, rl * l + rr * r);

            let bass_mix = self.bass_mix.next_value();
            let listen_mix = self.listen_mix.next_value();
            if bass_mix > 0.0 || listen_mix > 0.0 {
                let [left_bands, right_bands] = &mut self.crossover;
                let (low_l, high_l) = left_bands.split(f64::from(l));
                let (low_r, high_r) = right_bands.split(f64::from(r));
                let low = (low_l + low_r) / 2.0;
                l += ((high_l + low) as f32 - l) * bass_mix;
                r += ((high_r + low) as f32 - r) * bass_mix;
                // The low band alone, centred as far as bass mono is on.
                let mix = f64::from(bass_mix);
                let (only_l, only_r) = (low_l + (low - low_l) * mix, low_r + (low - low_r) * mix);
                l += (only_l as f32 - l) * listen_mix;
                r += (only_r as f32 - r) * listen_mix;
            }

            let gain = self.next_gain();
            *left = l * gain * self.balance_left.next_value();
            *right = r * gain * self.balance_right.next_value();
        }
    }
}

#[cfg(test)]
mod tests;
