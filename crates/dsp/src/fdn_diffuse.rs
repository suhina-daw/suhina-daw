// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Experimental diffusion/early-reflection wrapper; see the reverb prototype spec.

use crate::fdn::{Fdn, Fdn16, Settings, SettingsError};
use crate::{Allpass, DelayLine, FirstOrder, FirstOrderCoefficients};

/// Construct off-thread; no live parameter mutation in this research core.
#[derive(Debug, Clone, Copy)]
pub struct DiffuseSettings {
    pub core: Settings,
    /// Research feedback-network order, eight or sixteen.
    pub fdn_lines: u8,
    /// Research tank output-tap multipliers; unused by the FDN wrapper.
    pub tank_output_weights: Option<[[f32; 7]; 2]>,
    /// Scales only the late network, leaving early taps and input diffusion fixed.
    pub late_scale: f64,
    pub predelay_ms: f64,
    pub diffusion: f32,
    pub early_level: f32,
    /// Fixed independent early-output gains, prepared off-thread.
    pub early_output_weights: [f32; 2],
    /// Paired custom stereo tap times, in milliseconds before room scaling.
    pub early_tap_times_ms: Option<[[f64; 8]; 2]>,
    /// Paired custom signed gains; overrides the default pattern and onset/tilt.
    pub early_tap_weights: Option<[[f32; 8]; 2]>,
    /// Relative gain of the first early reflection, prepared off-thread.
    pub early_onset_gain: f32,
    /// Gain tilt in dB from the first to the last early reflection.
    pub early_tilt_db: f64,
    /// Independent short spreading of early reflections; zero bypasses it.
    pub early_diffusion: f32,
    pub width: f32,
    pub low_cut_hz: f64,
    pub high_cut_hz: f64,
}

impl Default for DiffuseSettings {
    fn default() -> Self {
        Self {
            core: Settings::default(),
            fdn_lines: 8,
            tank_output_weights: None,
            late_scale: 1.,
            predelay_ms: 2.5,
            diffusion: 0.3,
            early_level: 0.15,
            early_output_weights: [1.; 2],
            early_tap_times_ms: None,
            early_tap_weights: None,
            early_onset_gain: 1.,
            early_tilt_db: 0.,
            early_diffusion: 0.,
            width: 0.82,
            low_cut_hz: 120.,
            high_cut_hz: 14_000.,
        }
    }
}

/// Native shared blocks only; no captured reference samples or runtime I/O.
#[derive(Debug)]
pub struct DiffusedFdn {
    core: Core,
    predelay: DelayLine,
    pre_frames: usize,
    early: DelayLine,
    taps: [usize; 8],
    early_gains: [f32; 8],
    custom_early: Option<EarlyPattern>,
    diffusers: [(Allpass, f64); 4],
    early_diffusers: [[(Allpass, f64); 2]; 2],
    highpass: [FirstOrder; 2],
    lowpass: [FirstOrder; 2],
    settings: DiffuseSettings,
}

#[derive(Debug)]
struct EarlyPattern {
    taps: [[usize; 8]; 2],
    gains: [[f32; 8]; 2],
}

impl DiffusedFdn {
    /// Allocates bounded history. All input settings are checked before allocation.
    pub fn new(s: DiffuseSettings) -> Result<Self, SettingsError> {
        let rate = f64::from(s.core.sample_rate);
        if !(0.5..=2.).contains(&s.core.room_scale)
            || !(0.5..=2.).contains(&s.late_scale)
            || !(0. ..=100.).contains(&s.predelay_ms)
            || !(0. ..=0.8).contains(&s.diffusion)
            || !(0. ..=2.).contains(&s.early_level)
            || !s.early_output_weights.into_iter().all(|x| (-4. ..=4.).contains(&x))
            || !(0. ..=8.).contains(&s.early_onset_gain)
            || !(-12. ..=12.).contains(&s.early_tilt_db)
            || !(0. ..=0.8).contains(&s.early_diffusion)
            || !(0. ..=1.).contains(&s.width)
            || !(20. ..=1000.).contains(&s.low_cut_hz)
            || !(1000. ..=rate * 0.45).contains(&s.high_cut_hz)
        {
            return Err(SettingsError);
        }
        let custom_early = match (s.early_tap_times_ms, s.early_tap_weights) {
            (None, None) => None,
            (Some(times), Some(gains))
                if times.iter().flatten().all(|x| (0. ..=100.).contains(x))
                    && gains.iter().flatten().all(|x| (-4. ..=4.).contains(x)) =>
            {
                Some(EarlyPattern {
                    taps: times
                        .map(|channel| channel.map(|ms| (ms * rate * s.core.room_scale / 1000.).round() as usize)),
                    gains,
                })
            }
            _ => return Err(SettingsError),
        };
        let settings = Settings { room_scale: s.core.room_scale * s.late_scale, ..s.core };
        let core = match s.fdn_lines {
            8 => Core::Eight(Box::new(Fdn::new(settings)?)),
            16 => Core::Sixteen(Box::new(Fdn16::new(settings)?)),
            _ => return Err(SettingsError),
        };
        let pre_frames = (s.predelay_ms * rate / 1000.).round() as usize;
        let tap_ms = [0., 3.7, 7.9, 12.7, 18.1, 25.3, 34.9, 46.7];
        let taps = tap_ms.map(|ms| (ms * rate * s.core.room_scale / 1000.).round() as usize);
        let base_gains = [0.16_f32, 0.6, 0.5, 0.43, 0.36, 0.31, 0.25, 0.2];
        let mut early_gains =
            std::array::from_fn(|i| base_gains[i] * 10_f64.powf(s.early_tilt_db * tap_ms[i] / 46.7 / 20.) as f32);
        early_gains[0] *= s.early_onset_gain;
        let diffusers = [127., 193., 307., 421.].map(|base| {
            let frames = (base * rate / 48_000. * s.core.room_scale).round().max(2.);
            (Allpass::for_seconds(frames / rate, rate), frames)
        });
        let early_diffusers = [[23., 61.], [31., 73.]].map(|delays| {
            delays.map(|base| {
                let frames = (base * rate / 48_000.).round().max(2.);
                (Allpass::for_seconds(frames / rate, rate), frames)
            })
        });
        let early_capacity =
            custom_early.as_ref().map_or(taps[7], |pattern| pattern.taps.iter().flatten().copied().max().unwrap_or(0));
        Ok(Self {
            core,
            predelay: DelayLine::new(pre_frames),
            pre_frames,
            early: DelayLine::new(early_capacity),
            taps,
            early_gains,
            custom_early,
            diffusers,
            early_diffusers,
            highpass: [FirstOrder::new(FirstOrderCoefficients::highpass(rate, s.low_cut_hz)); 2],
            lowpass: [FirstOrder::new(FirstOrderCoefficients::lowpass(rate, s.high_cut_hz)); 2],
            settings: s,
        })
    }

    /// Delay history only; includes core, early path, predelay and all allpasses.
    pub fn history_bytes(&self) -> usize {
        self.core.history_bytes()
            + [&self.predelay, &self.early]
                .iter()
                .map(|line| (line.capacity() as usize + 3) * size_of::<f32>())
                .sum::<usize>()
            + self.diffusers.iter().map(|(ap, _)| ap.history_bytes()).sum::<usize>()
            + self.early_diffusers.iter().flatten().map(|(ap, _)| ap.history_bytes()).sum::<usize>()
    }

    /// Clears all state without allocation or freeing history.
    pub fn reset(&mut self) {
        self.core.reset();
        self.predelay.clear();
        self.early.clear();
        for (ap, _) in &mut self.diffusers {
            ap.clear();
        }
        for (ap, _) in self.early_diffusers.iter_mut().flatten() {
            ap.clear();
        }
        for filter in self.highpass.iter_mut().chain(&mut self.lowpass) {
            filter.reset();
        }
    }

    /// Wet-only stereo; invalid input is silenced before entering any recursive state.
    pub fn process_frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        let clean = input.map(|x| if x.is_finite() { x.clamp(-4., 4.) } else { 0. });
        let mono = (clean[0] + clean[1]) * 0.5;
        let delayed = if self.pre_frames == 0 { mono } else { self.predelay.read(self.pre_frames) };
        self.predelay.push(mono);
        let mut early = [0.; 2];
        if let Some(pattern) = &self.custom_early {
            for ((sample, taps), gains) in early.iter_mut().zip(&pattern.taps).zip(&pattern.gains) {
                for (frames, gain) in taps.iter().zip(gains) {
                    let x = if *frames == 0 { delayed } else { self.early.read(*frames) };
                    *sample += x * gain;
                }
            }
        } else {
            // Independent sparse early pattern; the first tap is the current predelayed sample.
            for (i, frames) in self.taps.iter().enumerate() {
                let x = if *frames == 0 { delayed } else { self.early.read(*frames) };
                let gain = self.early_gains[i];
                early[0] += x * gain * if i % 3 == 1 { -1. } else { 1. };
                early[1] += x * gain * if i % 3 == 2 { -1. } else { 1. };
            }
        }
        self.early.push(delayed);
        if self.settings.early_diffusion > 0. {
            for (sample, chain) in early.iter_mut().zip(&mut self.early_diffusers) {
                for (ap, frames) in chain {
                    *sample = ap.process(*sample, *frames, self.settings.early_diffusion);
                }
            }
        }
        let mut diffuse = delayed;
        if self.settings.diffusion > 0. {
            for (ap, frames) in &mut self.diffusers {
                diffuse = ap.process(diffuse, *frames, self.settings.diffusion);
            }
        }
        let mut output = self.core.process_frame([diffuse; 2]);
        for i in 0..2 {
            let x = output[i] + self.settings.early_level * self.settings.early_output_weights[i] * early[i];
            output[i] = self.lowpass[i].process(self.highpass[i].process(f64::from(x))) as f32;
        }
        let mid = (output[0] + output[1]) * 0.5;
        let side = (output[0] - output[1]) * 0.5 * self.settings.width;
        [mid + side, mid - side]
    }

    /// Stereo interleaved frames; leaves an incomplete final frame unchanged.
    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.as_chunks_mut::<2>().0 {
            *frame = self.process_frame(*frame);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
#[path = "fdn_diffuse_tests.rs"]
mod tests;

#[derive(Debug)]
enum Core {
    Eight(Box<Fdn>),
    Sixteen(Box<Fdn16>),
}
impl Core {
    fn process_frame(&mut self, x: [f32; 2]) -> [f32; 2] {
        match self {
            Self::Eight(c) => c.process_frame(x),
            Self::Sixteen(c) => c.process_frame(x),
        }
    }
    fn reset(&mut self) {
        match self {
            Self::Eight(c) => c.reset(),
            Self::Sixteen(c) => c.reset(),
        }
    }
    fn history_bytes(&self) -> usize {
        match self {
            Self::Eight(c) => c.history_bytes(),
            Self::Sixteen(c) => c.history_bytes(),
        }
    }
}
