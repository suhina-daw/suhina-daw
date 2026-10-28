// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Experimental homogeneous-loss tank, based on public topology; see reverb-prototype.md.
use crate::fdn::SettingsError;
use crate::fdn_diffuse::DiffuseSettings;
use crate::{DelayLine, FirstOrder, FirstOrderCoefficients};

#[derive(Debug)]
struct Section {
    line: DelayLine,
    frames: usize,
    loss: f32,
    gain: f32,
}
impl Section {
    fn new(frames: usize, rate: f64, decay: Option<f64>, gain: f32) -> Self {
        let loss = decay.map_or(1., |seconds| 10_f64.powf(-3. * frames as f64 / rate / seconds) as f32);
        Self { line: DelayLine::new(frames), frames, loss, gain }
    }
    fn read(&self) -> f32 {
        self.line.read(self.frames) * self.loss
    }
    fn allpass(&mut self, input: f32) -> f32 {
        let delayed = self.read();
        let state = input - self.gain * delayed;
        self.line.push(state);
        delayed + self.gain * state
    }
    fn bytes(&self) -> usize {
        (self.line.capacity() as usize + 3) * size_of::<f32>()
    }
}

/// Fixed research settings prepared off the audio thread. No runtime allocations.
#[derive(Debug)]
pub struct Tank {
    sections: [Section; 8],
    input_diffusers: [Section; 4],
    predelay: Section,
    pre_frames: usize,
    taps: [[(usize, usize, f32); 7]; 2],
    highpass: [FirstOrder; 2],
    lowpass: [FirstOrder; 2],
}
impl Tank {
    /// Constructs bounded state; unsupported or non-finite values fail before allocation.
    pub fn new(s: DiffuseSettings) -> Result<Self, SettingsError> {
        let rate = f64::from(s.core.sample_rate);
        if !s
            .tank_output_weights
            .is_none_or(|channels| channels.into_iter().flatten().all(|x| (-4. ..=4.).contains(&x)))
            || !(8000..=384000).contains(&s.core.sample_rate)
            || !(0.2..=20.).contains(&s.core.decay_seconds)
            || !(0.25..=2.).contains(&s.late_scale)
            || !(0. ..=0.8).contains(&s.diffusion)
            || !(0. ..=0.8).contains(&s.early_diffusion)
            || !(0. ..=100.).contains(&s.predelay_ms)
            || !(20. ..=1000.).contains(&s.low_cut_hz)
            || !(1000. ..=rate * 0.45).contains(&s.high_cut_hz)
        {
            return Err(SettingsError);
        }
        let frames = |n: f64| (n * rate / 29761. * s.late_scale).round().max(1.) as usize;
        let lengths = [672., 4453., 1800., 3720., 908., 4217., 2656., 3163.];
        let gains = [-s.diffusion, 0., s.early_diffusion, 0., -s.diffusion, 0., s.early_diffusion, 0.];
        let sections =
            std::array::from_fn(|i| Section::new(frames(lengths[i]), rate, Some(s.core.decay_seconds), gains[i]));
        let input_diffusers = [(142., 0.75), (107., 0.75), (379., 0.625), (277., 0.625)]
            .map(|(n, g)| Section::new(frames(n), rate, None, g));
        let raw_taps = [
            [
                (5, 266., 1.),
                (5, 2974., 1.),
                (6, 1913., -1.),
                (7, 1996., 1.),
                (1, 1990., -1.),
                (2, 187., -1.),
                (3, 1066., -1.),
            ],
            [
                (1, 353., 1.),
                (1, 3627., 1.),
                (2, 1228., -1.),
                (3, 2673., 1.),
                (5, 2111., -1.),
                (6, 335., -1.),
                (7, 121., -1.),
            ],
        ];
        let mut taps = raw_taps.map(|channel| {
            channel.map(|(index, n, sign)| {
                let f = frames(n);
                (index, f, sign * 0.6 * 10_f64.powf(-3. * f as f64 / rate / s.core.decay_seconds) as f32)
            })
        });
        if let Some(weights) = s.tank_output_weights {
            for (channel, gains) in taps.iter_mut().zip(weights) {
                for (tap, gain) in channel.iter_mut().zip(gains) {
                    tap.2 *= gain;
                }
            }
        }
        let pre_frames = (s.predelay_ms * rate / 1000.).round() as usize;
        Ok(Self {
            sections,
            input_diffusers,
            predelay: Section::new(pre_frames, rate, None, 0.),
            pre_frames,
            taps,
            highpass: [FirstOrder::new(FirstOrderCoefficients::highpass(rate, s.low_cut_hz)); 2],
            lowpass: [FirstOrder::new(FirstOrderCoefficients::lowpass(rate, s.high_cut_hz)); 2],
        })
    }
    /// Wet-only sample. The two last delay lines break both feedback dependencies.
    pub fn process_frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        let clean = input.map(|x| if x.is_finite() { x.clamp(-4., 4.) } else { 0. });
        let mono = (clean[0] + clean[1]) * 0.5;
        let mut x = if self.pre_frames == 0 { mono } else { self.predelay.read() };
        self.predelay.line.push(mono);
        for section in &mut self.input_diffusers {
            x = section.allpass(x);
        }
        let mut output = self.taps.map(|channel| {
            channel.iter().map(|(index, frames, gain)| self.sections[*index].line.read(*frames) * gain).sum::<f32>()
        });
        let feedback = [self.sections[7].read(), self.sections[3].read()];
        for (branch, returned) in feedback.into_iter().enumerate() {
            let i = branch * 4;
            let first = self.sections[i].allpass(x + returned);
            let delayed = self.sections[i + 1].read();
            self.sections[i + 1].line.push(first);
            let second = self.sections[i + 2].allpass(delayed);
            self.sections[i + 3].line.push(second);
        }
        for (i, y) in output.iter_mut().enumerate() {
            *y = self.lowpass[i].process(self.highpass[i].process(f64::from(*y))) as f32;
        }
        output
    }
    /// Interleaved stereo, preserving an incomplete final frame.
    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.as_chunks_mut::<2>().0 {
            *frame = self.process_frame(*frame);
        }
    }
    /// Allocated histories only, excluding allocator metadata.
    pub fn history_bytes(&self) -> usize {
        self.sections.iter().chain(&self.input_diffusers).map(Section::bytes).sum::<usize>() + self.predelay.bytes()
    }
    /// Clears history without allocating or releasing memory.
    pub fn reset(&mut self) {
        for s in self.sections.iter_mut().chain(&mut self.input_diffusers).chain(std::iter::once(&mut self.predelay)) {
            s.line.clear();
        }
        for f in self.highpass.iter_mut().chain(&mut self.lowpass) {
            f.reset();
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn tank_rejects_hostile_settings_and_resets_after_decay() {
        for value in [f64::NAN, f64::INFINITY, 0., 2.01] {
            assert!(Tank::new(DiffuseSettings { late_scale: value, ..DiffuseSettings::default() }).is_err());
        }
        let settings = DiffuseSettings {
            core: crate::fdn::Settings { decay_seconds: 0.5, ..Default::default() },
            late_scale: 0.5,
            diffusion: 0.4,
            early_diffusion: 0.5,
            ..Default::default()
        };
        let mut tank = Tank::new(settings).unwrap();
        let mut late = 0_f64;
        let mut energy = 0_f64;
        for n in 0..96000 {
            let input = if n == 0 {
                [0.25; 2]
            } else if n == 1 {
                [f32::NAN, f32::INFINITY]
            } else {
                [0.; 2]
            };
            let y = tank.process_frame(input);
            assert!(y.iter().all(|x| x.is_finite()));
            let power = y.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
            energy += power;
            if n > 72000 {
                late += power;
            }
        }
        assert!(energy > 1e-5);
        assert!(late < 1e-15);
        tank.reset();
        for _ in 0..24000 {
            assert_eq!(tank.process_frame([0.; 2]), [0.; 2]);
        }
    }
    #[test]
    fn impulse_matches_independently_evaluated_transfer_function() {
        // Independent frequency-domain lattice equations, not captured commercial audio.
        let expected = [
            (334, [5.868916e-16_f32, 1.473552e-14_f32]),
            (335, [-2.943431e-02_f32, 5.580891e-15_f32]),
            (336, [-2.510045e-02_f32, 4.408534e-15_f32]),
            (337, [4.214905e-03_f32, -2.569392e-15_f32]),
            (390, [3.492451e-04_f32, -1.284279e-14_f32]),
            (1000, [6.910082e-04_f32, -1.750491e-03_f32]),
            (5000, [-1.48257e-02_f32, -6.873593e-03_f32]),
            (20000, [5.990767e-03_f32, 8.788428e-03_f32]),
        ];
        let s = DiffuseSettings {
            core: crate::fdn::Settings { decay_seconds: 5.321, ..Default::default() },
            late_scale: 0.5,
            diffusion: 0.4,
            early_diffusion: 0.5,
            ..Default::default()
        };
        let mut tank = Tank::new(s).unwrap();
        for n in 0..=20000 {
            let y = tank.process_frame(if n == 0 { [1.; 2] } else { [0.; 2] });
            if let Some((_, reference)) = expected.iter().find(|(frame, _)| *frame == n) {
                for (actual, wanted) in y.iter().zip(reference) {
                    assert!((actual - wanted).abs() < 2e-7, "frame {n}: {actual} versus {wanted}");
                }
            }
        }
    }
    #[test]
    fn output_projection_rejects_nonfinite_and_scales_observations_only() {
        for bad in [f32::NAN, f32::INFINITY, 4.01] {
            let mut weights = [[1.; 7]; 2];
            weights[0][0] = bad;
            assert!(Tank::new(DiffuseSettings { tank_output_weights: Some(weights), ..Default::default() }).is_err());
        }
        let mut a = Tank::new(DiffuseSettings::default()).unwrap();
        let mut b = Tank::new(DiffuseSettings { tank_output_weights: Some([[0.5; 7], [2.; 7]]), ..Default::default() })
            .unwrap();
        for n in 0..24000 {
            let input = if n == 0 { [0.25; 2] } else { [0.; 2] };
            let x = a.process_frame(input);
            let y = b.process_frame(input);
            assert!((x[0] * 0.5 - y[0]).abs() < 1e-7);
            assert!((x[1] * 2. - y[1]).abs() < 1e-7);
        }
    }
}
