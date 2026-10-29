// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Limiter: a stereo-linked brickwall peak limiter with a short lookahead.
//!
//! The threshold works as a drive: the input is raised by `ceiling - threshold` decibels, so a
//! peak at the threshold comes out at the ceiling, and nothing comes out above the ceiling.
//! Per frame, the gain needed to keep the louder side at the ceiling is worked out as the
//! frame goes into a delay line [`LOOKAHEAD_SECONDS`] long. The gain that is applied is the
//! smallest needed gain in the lookahead window, let back up with the release time (a time
//! constant), then averaged over the window. Every gain averaged for a frame is at or below
//! the gain that frame needs, so the average is too: the gain starts to fall a whole window
//! before a peak and reaches what the peak needs exactly when it comes out, a smooth fade
//! instead of a clip. A final clip at the ceiling only catches rounding and a ceiling moving
//! down while frames are in the delay line. This is the textbook lookahead design (a sliding
//! minimum and a moving average); the code is our own.
//!
//! The device always reports [`Limiter::latency`] frames: the lookahead, rounded up to whole
//! frames at the sample rate. Under the threshold the output is the input delayed exactly,
//! times the drive.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::{AttackRelease, Smoother};

/// How far ahead the limiter looks, in seconds.
pub const LOOKAHEAD_SECONDS: f64 = 0.0015;
/// Ramp for threshold and ceiling changes.
pub const RAMP_SECONDS: f64 = 0.02;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const THRESHOLD: usize = 0;
    pub const CEILING: usize = 1;
    pub const RELEASE: usize = 2;
    pub(crate) const COUNT: usize = 3;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("threshold", "Threshold", (-30.0, 0.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("ceiling", "Ceiling", (-12.0, 0.0), -0.3, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("release", "Release", (0.001, 1.0), 0.05, Unit::Seconds, Scale::Logarithmic),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "limiter", name: "Limiter", role: Role::Effect, params: &PARAMS };

/// Lookahead in frames at `sample_rate`: [`LOOKAHEAD_SECONDS`] rounded up, at least one.
pub fn lookahead_frames(sample_rate: f64) -> usize {
    // The small margin keeps a product such as 144.00000000000003 from rounding up a frame.
    let frames = (LOOKAHEAD_SECONDS * sample_rate - 1e-6).ceil();
    if frames.is_finite() && frames >= 1.0 { (frames as usize).min(1 << 16) } else { 1 }
}

/// The smallest of the last `window` values pushed, in constant time per value: a queue of the
/// values that can still become the smallest, in rising order, in a ring of fixed size.
struct SlidingMinimum {
    /// (frame number, value) pairs; the queue occupies `len` slots from `head`.
    slots: Vec<(u64, f64)>,
    head: usize,
    len: usize,
    window: u64,
}

impl SlidingMinimum {
    fn new(window: usize) -> Self {
        Self { slots: vec![(0, 1.0); window.max(1)], head: 0, len: 0, window: window.max(1) as u64 }
    }

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    fn slot(&self, offset: usize) -> usize {
        (self.head + offset) % self.slots.len().max(1)
    }

    /// Adds `value` as frame `frame` and returns the smallest value of the window ending there.
    fn push(&mut self, frame: u64, value: f64) -> f64 {
        // Values larger than the new one can never be the smallest again.
        while self.len > 0 && self.slots.get(self.slot(self.len - 1)).is_some_and(|s| s.1 >= value) {
            self.len -= 1;
        }
        // The oldest leaves when it falls out of the window.
        while self.len > 0 && self.slots.get(self.head).is_some_and(|s| s.0 + self.window <= frame) {
            self.head = self.slot(1);
            self.len -= 1;
        }
        if self.len < self.slots.len() {
            let at = self.slot(self.len);
            if let Some(slot) = self.slots.get_mut(at) {
                *slot = (frame, value);
                self.len += 1;
            }
        }
        self.slots.get(self.head).map_or(value, |s| s.1.min(value))
    }
}

pub struct Limiter {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    /// Linear gain into the limiter (`ceiling - threshold` dB).
    drive: Smoother,
    /// The ceiling as a linear level.
    ceiling: Smoother,
    /// An instant attack, so the gain is never above what the window needs, and the release.
    dynamics: AttackRelease,
    lookahead: usize,
    /// Delay line for both sides, `lookahead` frames.
    delay: Vec<[f32; 2]>,
    minimum: SlidingMinimum,
    /// The last `lookahead + 1` released gains, and their sum.
    gains: Vec<f64>,
    gain_sum: f64,
    /// Where the next frame goes in `delay` and `gains`.
    at: usize,
    /// Frames since the last reset.
    frame: u64,
    /// The gain released from the window minimum.
    envelope: f64,
    /// Gain applied to the last frame that came out.
    applied: f64,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Limiter {
    pub fn new() -> Self {
        let mut limiter = Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            params: ParamBank::new(&PARAMS),
            drive: Smoother::new(1.0, 1),
            ceiling: Smoother::new(1.0, 1),
            dynamics: AttackRelease::default(),
            lookahead: 1,
            delay: Vec::new(),
            minimum: SlidingMinimum::new(1),
            gains: Vec::new(),
            gain_sum: 0.0,
            at: 0,
            frame: 0,
            envelope: 1.0,
            applied: 1.0,
        };
        limiter.allocate();
        limiter.rebuild();
        limiter
    }

    /// The gain reduction applied to the last frame that came out, in decibels (zero or
    /// positive), for a meter.
    pub fn gain_reduction_db(&self) -> f64 {
        if self.applied > 0.0 { (-20.0 * self.applied.log10()).max(0.0) } else { f64::INFINITY }
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn targets(&self) -> [f32; 2] {
        let (threshold, ceiling) = (self.value(param::THRESHOLD), self.value(param::CEILING));
        [db_to_gain((ceiling - threshold) as f32), db_to_gain(ceiling as f32)]
    }

    fn tune(&mut self) {
        self.dynamics = AttackRelease::new(0.0, self.value(param::RELEASE), self.sample_rate);
    }

    /// Buffers for the current sample rate. Allocates: only from `new` and `prepare`.
    fn allocate(&mut self) {
        self.lookahead = lookahead_frames(self.sample_rate);
        self.delay = vec![[0.0; 2]; self.lookahead];
        self.gains = vec![1.0; self.lookahead + 1];
        self.minimum = SlidingMinimum::new(self.lookahead + 1);
    }

    /// Smoothers and gains at their values, every state cleared. Does not allocate.
    fn rebuild(&mut self) {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, self.sample_rate);
        let [drive, ceiling] = self.targets();
        for (smoother, target) in [(&mut self.drive, drive), (&mut self.ceiling, ceiling)] {
            smoother.set_ramp(ramp);
            smoother.reset(target);
        }
        self.tune();
        self.delay.fill([0.0; 2]);
        self.gains.fill(1.0);
        self.gain_sum = self.gains.len() as f64;
        self.minimum.clear();
        self.at = 0;
        self.frame = 0;
        self.envelope = 1.0;
        self.applied = 1.0;
    }
}

impl Device for Limiter {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        self.sample_rate = check_sample_rate(sample_rate)?;
        self.allocate();
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
        let [drive, ceiling] = self.targets();
        self.drive.set_target(drive);
        self.ceiling.set_target(ceiling);
        if index == param::RELEASE {
            self.tune();
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn latency(&self) -> u32 {
        u32::try_from(self.lookahead).unwrap_or(u32::MAX)
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        let window = self.gains.len();
        if self.delay.is_empty() || window == 0 {
            return;
        }
        for frame in buffer.as_chunks_mut::<2>().0 {
            let drive = self.drive.next_value();
            let ceiling = self.ceiling.next_value();
            // Raised input; anything that is not a number goes in as silence.
            let raised = frame.map(|s| {
                let x = s * drive;
                if x.is_finite() { x } else { 0.0 }
            });
            let peak = f64::from(raised[0].abs().max(raised[1].abs()));
            let needed = if peak > f64::from(ceiling) { f64::from(ceiling) / peak } else { 1.0 };

            let smallest = self.minimum.push(self.frame, needed);
            self.envelope = self.dynamics.fall(self.envelope, smallest);
            let old = self.gains.get(self.at % window).copied().unwrap_or(1.0);
            if let Some(slot) = self.gains.get_mut(self.at % window) {
                *slot = self.envelope;
            }
            self.gain_sum += self.envelope - old;
            // Summed afresh once per window, so rounding never builds up.
            if self.at % window == window - 1 {
                self.gain_sum = self.gains.iter().sum();
            }
            let gain = (self.gain_sum / window as f64).clamp(0.0, 1.0);

            let slot_at = self.at % self.delay.len();
            let delayed = self.delay.get(slot_at).copied().unwrap_or([0.0; 2]);
            if let Some(slot) = self.delay.get_mut(slot_at) {
                *slot = raised;
            }
            for (sample, x) in frame.iter_mut().zip(delayed) {
                *sample = ((f64::from(x) * gain) as f32).max(-ceiling).min(ceiling);
            }
            self.applied = gain;
            self.frame += 1;
            self.at = (self.at + 1) % (self.delay.len() * window);
        }
    }
}

#[cfg(test)]
mod tests;
