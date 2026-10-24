// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded sample instrument runtime for SDK-authored native instruments.
use std::sync::Arc;
use suhina_device_api::{Device, DeviceError, DeviceInfo, NoteEvent, ParamInfo, ProcessContext, Role, Scale, Unit};
use suhina_dsp::{Biquad, Coefficients};

use crate::nodes::stretch as warp;

const fn continuous(id: &'static str, name: &'static str, min: f64, max: f64, default: f64, unit: Unit) -> ParamInfo {
    ParamInfo::continuous(id, name, (min, max), default, unit, Scale::Linear)
}
pub const PARAMS: &[ParamInfo] = &[
    ParamInfo::choice("mode", "Mode", &["Simple", "Grain"], 0),
    continuous("start", "Start", 0., 1., 0., Unit::Percent),
    continuous("end", "End", 0., 1., 1., Unit::Percent),
    continuous("loop_start", "Loop start", 0., 1., 0., Unit::Percent),
    continuous("loop_end", "Loop end", 0., 1., 1., Unit::Percent),
    continuous("pitch", "Semitones", -48., 48., 0., Unit::None),
    continuous("cents", "Cents", -100., 100., 0., Unit::None),
    ParamInfo::integer("poly", "Poly", (1, 32), 8, Unit::None),
    ParamInfo::choice("playback", "Playback", &["Gate", "One shot", "Loop", "Slices"], 0),
    ParamInfo::toggle("ping_pong", "Ping pong", false),
    ParamInfo::toggle("reverse", "Reverse", false),
    continuous("attack", "Attack", 0., 10., 0.005, Unit::Seconds),
    continuous("decay", "Decay", 0., 10., 0.22, Unit::Seconds),
    continuous("sustain", "Sustain", 0., 1., 0.87, Unit::Percent),
    continuous("release", "Release", 0., 10., 0.48, Unit::Seconds),
    ParamInfo::choice("filter", "Filter", &["Off", "Low pass", "High pass", "Band pass", "Notch"], 0),
    ParamInfo::continuous("cutoff", "Cutoff", (20., 20000.), 3100., Unit::Hertz, Scale::Logarithmic),
    continuous("resonance", "Resonance", 0.1, 10., 0.707, Unit::None),
    continuous("gain", "Gain", -60., 12., 0., Unit::Decibels),
    continuous("pan", "Pan", -1., 1., 0., Unit::Balance),
    continuous("position", "Position", 0., 1., 0.5, Unit::Percent),
    continuous("scan", "Scan", -4., 4., 0., Unit::None),
    continuous("size", "Size (ms)", 5., 500., 120., Unit::None),
    continuous("density", "Density (grains/s)", 1., 100., 8., Unit::None),
    continuous("jitter", "Jitter", 0., 1., 0.25, Unit::Percent),
    continuous("spread", "Spread", 0., 1., 0., Unit::Percent),
    ParamInfo::toggle("freeze", "Freeze", false),
    // Retained at its historical index for saved definitions; loop edges use fade_in/out.
    continuous("crossfade", "Loop fade (ms)", 0., 100., 5., Unit::None),
    continuous("velocity", "Velocity to volume", 0., 1., 1., Unit::Percent),
    continuous("glide", "Glide", 0., 2., 0., Unit::Seconds),
    ParamInfo::integer("root", "Root key", (0, 127), 60, Unit::None),
    ParamInfo::integer("key_low", "Lowest key", (0, 127), 0, Unit::None),
    ParamInfo::integer("key_high", "Highest key", (0, 127), 127, Unit::None),
    continuous("velocity_low", "Lowest velocity", 0., 1., 0., Unit::Percent),
    continuous("velocity_high", "Highest velocity", 0., 1., 1., Unit::Percent),
    continuous("filter_env", "Filter envelope (oct)", -8., 8., 0., Unit::Octaves),
    continuous("velocity_filter", "Velocity to filter (oct)", 0., 8., 0., Unit::Octaves),
    continuous("lfo_rate", "LFO rate", 0.01, 20., 1., Unit::Hertz),
    continuous("lfo_pitch", "LFO pitch (st)", 0., 12., 0., Unit::None),
    ParamInfo::toggle("grain_env", "Show grain amplitude envelope", false),
    ParamInfo::choice("voice_mode", "Voices", &["Poly", "Mono"], 0),
    continuous("pitch_env", "Pitch envelope (st)", -48., 48., 0., Unit::None),
    continuous("pitch_attack", "Pitch attack", 0., 10., 0., Unit::Seconds),
    continuous("pitch_decay", "Pitch decay", 0., 10., 0.2, Unit::Seconds),
    ParamInfo::choice("envelope_tab", "Envelope view", &["AMP", "PITCH"], 0),
    ParamInfo::choice("filter_slope", "Filter slope", &["12 dB/oct", "24 dB/oct"], 0),
    continuous("key_filter", "Cutoff < Key", -2., 2., 0., Unit::None),
    ParamInfo::toggle("retrigger", "Retrigger", true),
    continuous("filter_attack", "Filter attack", 0., 10., 0.005, Unit::Seconds),
    continuous("filter_decay", "Filter decay", 0., 10., 0.22, Unit::Seconds),
    continuous("filter_sustain", "Filter sustain", 0., 1., 0.87, Unit::Percent),
    continuous("filter_release", "Filter release", 0., 10., 0.48, Unit::Seconds),
    ParamInfo::choice("panel_view", "Panel view", &["Controls", "Envelopes", "LFO"], 0),
    continuous("lfo_filter", "LFO filter (oct)", 0., 8., 0., Unit::Octaves),
    continuous("lfo_volume", "LFO volume", 0., 1., 0., Unit::Percent),
    continuous("lfo_pan", "LFO pan", 0., 1., 0., Unit::Percent),
    ParamInfo::choice("lfo_shape", "LFO shape", &["Sine", "Triangle", "Saw up", "Saw down", "Square", "Random"], 0),
    continuous("fade_in", "Fade in (ms)", 0., 1000., 0., Unit::None),
    continuous("fade_out", "Fade out (ms)", 0., 1000., 0., Unit::None),
    continuous("sensitivity", "Sensitivity", 0., 1., 0.5, Unit::Percent),
    ParamInfo::choice("slice_by", "Slice by", &["Transient", "Beat", "Region", "Manual"], 0),
    continuous("slice_1", "Slice 1", 0., 1., 1., Unit::Percent),
    continuous("slice_2", "Slice 2", 0., 1., 1., Unit::Percent),
    continuous("slice_3", "Slice 3", 0., 1., 1., Unit::Percent),
    continuous("slice_4", "Slice 4", 0., 1., 1., Unit::Percent),
    continuous("slice_5", "Slice 5", 0., 1., 1., Unit::Percent),
    continuous("slice_6", "Slice 6", 0., 1., 1., Unit::Percent),
    continuous("slice_7", "Slice 7", 0., 1., 1., Unit::Percent),
    continuous("slice_8", "Slice 8", 0., 1., 1., Unit::Percent),
    continuous("slice_9", "Slice 9", 0., 1., 1., Unit::Percent),
    continuous("slice_10", "Slice 10", 0., 1., 1., Unit::Percent),
    continuous("slice_11", "Slice 11", 0., 1., 1., Unit::Percent),
    continuous("slice_12", "Slice 12", 0., 1., 1., Unit::Percent),
    continuous("slice_13", "Slice 13", 0., 1., 1., Unit::Percent),
    continuous("slice_14", "Slice 14", 0., 1., 1., Unit::Percent),
    continuous("slice_15", "Slice 15", 0., 1., 1., Unit::Percent),
    continuous("slice_16", "Slice 16", 0., 1., 1., Unit::Percent),
    ParamInfo::toggle("warp", "Warp", false),
    ParamInfo::choice(
        "warp_mode",
        "Warp mode",
        &["Repitch", "Rhythmic", "Tonal", "Texture", "Smooth", "HighQuality"],
        1,
    ),
    continuous("warp_beats", "Source length (beats)", 0.25, 1024., 4., Unit::None),
    ParamInfo::choice("lfo_rate_mode", "LFO rate mode", &["Hz", "Time", "Beat", "Triplet", "Dotted", "16th"], 0),
    continuous("lfo_time", "LFO time (ms)", 1., 20000., 1000., Unit::None),
    ParamInfo::integer("lfo_division", "LFO division", (1, 64), 4, Unit::None),
    continuous("source_bpm", "Source BPM", 0., 999., 0., Unit::None),
];
use crate::modulation::lfo_hz;

pub const INFO: DeviceInfo = DeviceInfo { id: "sampler", name: "Sampler", role: Role::Instrument, params: PARAMS };

#[derive(Clone, Copy, Default)]
struct Grain {
    window_step: f64,
    position: f64,
    age: usize,
    length: usize,
    step: f64,
    pan: f64,
}
#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    age: f64,
    key: u8,
    velocity: f64,
    position: f64,
    slice_start: f64,
    slice_end: f64,
    direction: f64,
    env: f64,
    stage: u8,
    released: f64,
    step: f64,
    scan: f64,
    next_grain: f64,
    grains: [Grain; 32],
    filters: [Biquad; 2],
    filters_24: [Biquad; 2],
    filter_env: f64,
    filter_released: f64,
    last: [f64; 2],
    fade_from: [f64; 2],
    fade: f64,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            age: 0.,
            key: 60,
            velocity: 1.,
            position: 0.,
            slice_start: 0.,
            slice_end: 1.,
            direction: 1.,
            env: 0.,
            stage: 0,
            released: 0.,
            step: 1.,
            scan: 0.,
            next_grain: 0.,
            grains: [Grain::default(); 32],
            filters: [Biquad::default(); 2],
            filters_24: [Biquad::default(); 2],
            filter_env: 0.,
            filter_released: 0.,
            last: [0.; 2],
            fade_from: [0.; 2],
            fade: 0.,
        }
    }
}
pub struct Sampler {
    window: [f64; 2049],
    values: [f64; PARAMS.len()],
    analysis: Arc<SliceAnalysis>,
    manual_slices: Option<Arc<[f64]>>,
    tempo: f64,
    rate: f64,
    source_rate: f64,
    channels: usize,
    source: Arc<dyn AsRef<[f32]> + Send + Sync>,
    voices: Vec<Voice>,
    warp: Vec<warp::Voice>,
    next_voice: usize,
    seed: u32,
    phase: f64,
    lfo_cycle: u32,
    bend: f64,
    gain: f64,
    grain_gain: f64,
    block_grain_limit: usize,
    block_grain_gain: f64,
    block_gain: f64,
    pan: f64,
}
impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}
impl Sampler {
    pub fn new() -> Self {
        Self {
            window: std::array::from_fn(|i| crate::nodes::granular::hann(i as f64 / 2048.)),
            values: std::array::from_fn(|i| PARAMS[i].default),
            analysis: Arc::new(SliceAnalysis::default()),
            manual_slices: None,
            tempo: 120.,
            rate: 48000.,
            source_rate: 48000.,
            channels: 1,
            source: Arc::new(Vec::<f32>::new()),
            voices: vec![Voice::default(); 32],
            warp: Vec::new(),
            next_voice: 0,
            seed: 1,
            phase: 0.,
            lfo_cycle: 0,
            bend: 0.,
            gain: 1.,
            grain_gain: 1.,
            block_grain_limit: 32,
            block_grain_gain: 1.,
            block_gain: 1.,
            pan: 0.,
        }
    }
    fn frames(&self) -> usize {
        self.source.as_ref().as_ref().len() / self.channels.max(1)
    }
}
fn read(source: &[f32], channels: usize, position: f64) -> [f64; 2] {
    let frames = source.len() / channels.max(1);
    if !(position >= 0. && position < frames as f64) {
        return [0.; 2];
    }
    let i = position as usize;
    let fraction = position.fract();
    std::array::from_fn(|ch| {
        let ch = ch.min(channels.saturating_sub(1));
        let a = source.get(i.saturating_mul(channels).saturating_add(ch)).copied().unwrap_or(0.) as f64;
        let b =
            source.get(i.saturating_add(1).saturating_mul(channels).saturating_add(ch)).copied().unwrap_or(0.) as f64;
        let value = a + (b - a) * fraction;
        if value.is_finite() { value } else { 0. }
    })
}
fn source_fade(position: f64, start: f64, end: f64, fade_in: f64, fade_out: f64, rate: f64) -> f64 {
    let fade = |distance: f64, ms: f64| if ms > 0. { (distance / (ms * rate / 1000.)).clamp(0., 1.) } else { 1. };
    fade(position - start, fade_in) * fade(end - 1. - position, fade_out)
}
fn random(seed: &mut u32) -> f64 {
    crate::nodes::random::bipolar(seed)
}
impl Device for Sampler {
    fn sample_position(&self) -> Option<f32> {
        self.sample_positions().into_iter().flatten().next()
    }
    fn sample_positions(&self) -> [Option<f32>; 32] {
        let frames = self.frames();
        std::array::from_fn(|i| {
            let v = self.voices.get(i).filter(|v| v.active && frames > 0)?;
            let position = if self.values[0] == 0. {
                v.position
            } else {
                let start = self.values[1] * frames as f64;
                let span = (self.values[2] - self.values[1]) * frames as f64;
                start + (self.values[20] * span + v.scan).rem_euclid(span.max(1.))
            };
            Some((position / frames as f64) as f32)
        })
    }
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }
    fn swap_sample_slices(&mut self, slices: &mut Option<Arc<[f64]>>) {
        std::mem::swap(&mut self.manual_slices, slices);
    }
    fn load_sample_slices(&mut self, slices: Option<Arc<[f64]>>) {
        self.manual_slices = slices.map(|points| {
            let mut points: Vec<_> = points.iter().copied().filter(|v| v.is_finite() && *v > 0. && *v < 1.).collect();
            points.sort_unstable_by(f64::total_cmp);
            points.dedup();
            Arc::from(points)
        });
    }
    fn load_sample(&mut self, rate: u32, channels: u16, samples: Arc<[f32]>) {
        self.load_shared_sample(rate, channels, Arc::new(samples));
    }
    fn load_shared_sample(&mut self, rate: u32, channels: u16, samples: Arc<dyn AsRef<[f32]> + Send + Sync>) {
        let pcm = samples.as_ref().as_ref();
        if !(1000..=384000).contains(&rate)
            || !(1..=32).contains(&channels)
            || !pcm.len().is_multiple_of(usize::from(channels))
        {
            return;
        }
        self.analysis = shared_analysis(&samples, channels, rate);
        self.source = samples;
        self.channels = usize::from(channels);
        self.source_rate = f64::from(rate);
        self.warp = (0..32).map(|_| warp::Voice::new(self.rate)).collect();
        self.reset();
    }
    fn prepare(&mut self, rate: f64) -> Result<(), DeviceError> {
        suhina_device_api::check_sample_rate(rate)?;
        self.rate = rate;
        if self.frames() > 0 {
            self.warp = (0..32).map(|_| warp::Voice::new(rate)).collect();
        }
        self.reset();
        Ok(())
    }
    fn reset(&mut self) {
        self.voices.fill(Voice::default());
        for voice in &mut self.warp {
            voice.restart();
        }
        self.next_voice = 0;
        self.seed = 1;
        self.phase = 0.;
        self.lfo_cycle = 0;
        self.bend = 0.;
        self.gain = 10f64.powf(self.values[18] / 20.);
        self.grain_gain = 1. / (self.values[23] * self.values[22] / 1000. * 0.5).clamp(1., 16.);
        self.pan = self.values[19];
    }
    fn set_param(&mut self, index: usize, value: f64) {
        if PARAMS.get(index).is_some_and(|p| p.id == "warp_beats")
            && self.values.get(index).is_some_and(|old| *old != value)
        {
            self.values[83] = 0.;
        }
        let warp_enabled = self.values[77] > 0.;
        if let Some((p, v)) = PARAMS.get(index).zip(self.values.get_mut(index)) {
            let next = p.clamp(value);
            if p.id == "reverse" && next != *v {
                for voice in &mut self.voices {
                    voice.direction = -voice.direction;
                    for grain in &mut voice.grains {
                        grain.step = -grain.step;
                    }
                }
            }
            if matches!(p.id, "reverse" | "mode") && next != *v {
                for voice in &mut self.warp {
                    voice.restart();
                }
            }
            if p.id == "playback" && next != *v {
                self.voices.fill(Voice::default());
                for voice in &mut self.warp {
                    voice.restart();
                }
            }
            let warp_transition =
                p.id == "warp" && next != *v || p.id == "warp_mode" && (next == 0.) != (*v == 0.) && warp_enabled;
            if warp_transition {
                for voice in &mut self.voices {
                    if voice.active {
                        voice.fade_from = voice.last;
                        voice.fade = 1.;
                    }
                }
                for voice in &mut self.warp {
                    voice.restart();
                }
            }
            *v = next;
        }
    }
    fn param(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied()
    }
    fn event(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::On { key, velocity } if key <= 127 && velocity.is_finite() && velocity > 0. => {
                let p = self.values;
                if f64::from(key) < p[31]
                    || f64::from(key) > p[32]
                    || f64::from(velocity) < p[33]
                    || f64::from(velocity) > p[34]
                {
                    return;
                }
                let count = if p[40] > 0. { 1 } else { p[7] as usize };
                let slices = p[0] == 0. && p[8] == 3.;
                let map = self.analysis.points(&p, self.frames(), self.source_rate, self.tempo);
                let index = usize::from(key).checked_sub(p[30] as usize);
                let slice = if p[60] == 3. && self.manual_slices.is_some() {
                    index.and_then(|i| {
                        self.manual_slices.as_ref().and_then(|points| manual_slice(points, p[1], p[2], i))
                    })
                } else {
                    index.and_then(|i| map.get(i..=i + 1).map(|s| (s[0], s[1])))
                };
                if slices && slice.is_none_or(|s| s.1 <= s.0) {
                    return;
                }
                let (slice_start, slice_end) = if slices { slice.unwrap_or((p[1], p[2])) } else { (p[1], p[2]) };
                let target_step = 2f64
                    .powf(((if slices { 0. } else { f64::from(key) - p[30] }) + p[5] + p[6] / 100.) / 12.)
                    * self.source_rate
                    / self.rate;
                let step = if p[29] > 0. {
                    self.voices
                        .get((self.next_voice + count - 1) % count)
                        .filter(|v| v.active)
                        .map_or(target_step, |v| v.step)
                } else {
                    target_step
                };
                let slot = self.voices.iter().take(count).position(|v| !v.active).unwrap_or(self.next_voice % count);
                self.next_voice = (slot + 1) % count;
                let frames = self.frames() as f64;
                if let Some(v) = self.voices.get_mut(slot) {
                    if v.active && p[47] == 0. && !slices {
                        v.key = key;
                        v.velocity = f64::from(velocity.clamp(0., 1.));
                        return;
                    }
                    let fade_from = v.last;
                    if let Some(warp) = self.warp.get_mut(slot) {
                        warp.restart();
                    }
                    let fade = if v.active { 1. } else { 0. };
                    let direction = if p[10] > 0. { -1. } else { 1. };
                    *v = Voice {
                        active: true,
                        fade_from,
                        fade,
                        key,
                        velocity: f64::from(velocity.clamp(0., 1.)),
                        position: if direction > 0. { slice_start * frames } else { (slice_end * frames - 1.).max(0.) },
                        slice_start,
                        slice_end,
                        direction,
                        step,
                        ..Voice::default()
                    };
                }
            }
            NoteEvent::Off { key, .. } | NoteEvent::On { key, velocity: 0. } => {
                if self.values[8] != 1. && self.values[8] != 3. {
                    for v in &mut self.voices {
                        if v.key == key && v.active {
                            v.stage = 3;
                            v.released = v.env;
                            v.filter_released = v.filter_env;
                        }
                    }
                }
            }
            NoteEvent::AllNotesOff => {
                for v in &mut self.voices {
                    v.stage = 3;
                    v.released = v.env;
                    v.filter_released = v.filter_env;
                }
            }
            NoteEvent::PitchBend(value) if value.is_finite() => {
                self.bend = f64::from(value.clamp(-1., 1.)) * 2.;
            }
            _ => {}
        }
    }
    fn set_context(&mut self, context: &ProcessContext) {
        if context.tempo.is_finite() && context.tempo > 0. {
            self.tempo = context.tempo;
        }
    }
    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        self.prepare_block(context);
        self.process_prepared(buffer);
    }
}

type SharedPcm = Arc<dyn AsRef<[f32]> + Send + Sync>;
type CachedAnalysis = (std::sync::Weak<dyn AsRef<[f32]> + Send + Sync>, u16, u32, std::sync::Weak<SliceAnalysis>);

// Loading is off the audio thread; weak entries do not retain unused PCM or analyses.
fn shared_analysis(source: &SharedPcm, channels: u16, rate: u32) -> Arc<SliceAnalysis> {
    static CACHE: std::sync::Mutex<Vec<CachedAnalysis>> = std::sync::Mutex::new(Vec::new());
    let build = || Arc::new(SliceAnalysis::new(source.as_ref().as_ref(), usize::from(channels), f64::from(rate)));
    let Ok(mut cache) = CACHE.lock() else { return build() };
    cache.retain(|(pcm, _, _, analysis)| pcm.strong_count() > 0 && analysis.strong_count() > 0);
    if let Some(analysis) = cache.iter().find_map(|(pcm, ch, hz, analysis)| {
        (*ch == channels && *hz == rate && pcm.ptr_eq(&Arc::downgrade(source))).then(|| analysis.upgrade()).flatten()
    }) {
        return analysis;
    }
    let analysis = build();
    cache.push((Arc::downgrade(source), channels, rate, Arc::downgrade(&analysis)));
    analysis
}

pub use crate::nodes::slicing::SliceAnalysis;

fn manual_slice(points: &[f64], start: f64, end: f64, index: usize) -> Option<(f64, f64)> {
    if end <= start {
        return None;
    }
    let first = points.partition_point(|p| *p <= start);
    let last = points.partition_point(|p| *p < end);
    let count = last.saturating_sub(first);
    if index > count {
        return None;
    }
    Some((
        if index == 0 { start } else { *points.get(first.checked_add(index)?.checked_sub(1)?)? },
        if index == count { end } else { *points.get(first.checked_add(index)?)? },
    ))
}

#[cfg(test)]
mod shared_source_tests {
    use super::*;
    #[test]
    fn preparations_share_pcm_and_analysis_but_distinguish_format_and_source() {
        let source: SharedPcm = Arc::new(vec![0.25; 96000]);
        let mut a = Sampler::new();
        let mut b = Sampler::new();
        a.load_shared_sample(48000, 1, source.clone());
        b.load_shared_sample(48000, 1, source.clone());
        assert!(Arc::ptr_eq(&a.source, &source));
        assert!(Arc::ptr_eq(&a.analysis, &b.analysis));
        b.load_shared_sample(24000, 1, source.clone());
        assert!(!Arc::ptr_eq(&a.analysis, &b.analysis));
        b.load_shared_sample(48000, 2, source.clone());
        assert!(!Arc::ptr_eq(&a.analysis, &b.analysis));
        b.load_shared_sample(48000, 1, Arc::new(vec![0.25; 96000]));
        assert!(!Arc::ptr_eq(&a.analysis, &b.analysis));
        drop(a);
        drop(b);
        assert_eq!(Arc::strong_count(&source), 1, "cache must not retain PCM");
    }
}

#[cfg(test)]
mod rate_tests {
    #[test]
    fn rate_modes_match_delay_and_follow_tempo() {
        for (mode, expected) in [(0., 5.), (1., 2.), (2., 2.), (3., 3.), (4., 4. / 3.), (5., 2.)] {
            let division = 4.;
            assert!((super::lfo_hz(5., 500., division, mode, 120.) - expected).abs() < 1e-12);
            if mode >= 2. {
                assert!((super::lfo_hz(5., 500., division, mode, 60.) - expected / 2.).abs() < 1e-12);
            }
        }
    }
}

fn grain_window(table: &[f64; 2049], phase: f64) -> f64 {
    let index = (phase as usize).min(2047);
    let a = table.get(index).copied().unwrap_or(0.);
    let b = table.get(index + 1).copied().unwrap_or(0.);
    a + (b - a) * (phase - index as f64).clamp(0., 1.)
}

#[cfg(test)]
mod grain_window_tests {
    #[test]
    fn grain_normalization_smooths_voice_count_changes_independently_of_master_gain() {
        use suhina_device_api::{Device, NoteEvent, ProcessContext};
        let mut sampler = super::Sampler::new();
        sampler.load_sample(48000, 1, vec![0.25; 48000].into());
        sampler.prepare(48000.).unwrap();
        for (id, value) in [("mode", 1.), ("poly", 32.), ("size", 500.), ("density", 100.)] {
            sampler.set_param(super::INFO.param_index(id).unwrap(), value);
        }
        for key in 48..56 {
            sampler.event(NoteEvent::On { key, velocity: 1. });
        }
        let context = ProcessContext::default();
        sampler.process(&context, &mut [0.; 4800]);
        let before = sampler.grain_gain;
        let master = sampler.gain;
        sampler.event(NoteEvent::On { key: 56, velocity: 1. });
        sampler.process(&context, &mut [0.; 2]);
        let target = 1. / 14.;
        assert!(sampler.grain_gain > before && sampler.grain_gain < target);
        assert!((sampler.grain_gain - before) < (target - before) * 0.01);
        assert_eq!(sampler.gain, master);
        sampler.process(&context, &mut [0.; 4800]);
        assert!((sampler.grain_gain - target).abs() < 1e-6);
    }

    #[test]
    fn many_grain_voices_share_one_bounded_budget() {
        use suhina_device_api::{Device, NoteEvent, ProcessContext};
        let mut sampler = super::Sampler::new();
        sampler.load_sample(48000, 1, vec![0.25; 48000].into());
        sampler.prepare(48000.).unwrap();
        for (id, value) in [("mode", 1.), ("poly", 32.), ("size", 500.), ("density", 100.)] {
            sampler.set_param(super::INFO.param_index(id).unwrap(), value);
        }
        for key in 48..80 {
            sampler.event(NoteEvent::On { key, velocity: 1. });
        }
        let mut block = [0.; 512];
        for _ in 0..100 {
            sampler.process(&ProcessContext::default(), &mut block);
            assert!(sampler.voices.iter().flat_map(|v| v.grains).filter(|g| g.age < g.length).count() <= 256);
        }
        assert_eq!(sampler.voices.iter().filter(|v| v.active).count(), 32);
        assert!(block.iter().all(|v| v.is_finite()));
    }

    #[test]
    #[ignore = "local performance measurement, not a portable timing gate"]
    fn measure_maximum_grain_workload() {
        use suhina_device_api::{Device, NoteEvent, ProcessContext};
        let mut sampler = super::Sampler::new();
        sampler.load_sample(48000, 1, vec![0.25; 48000].into());
        sampler.prepare(48000.).unwrap();
        for (id, value) in [("mode", 1.), ("poly", 32.), ("size", 500.), ("density", 100.)] {
            sampler.set_param(super::INFO.param_index(id).unwrap(), value);
        }
        for key in 48..80 {
            sampler.event(NoteEvent::On { key, velocity: 1. });
        }
        let mut block = [0.; 512];
        let context = ProcessContext::default();
        for _ in 0..100 {
            sampler.process(&context, &mut block);
        }
        assert_eq!(sampler.voices.iter().filter(|v| v.active).count(), 32);
        assert!(sampler.voices.iter().flat_map(|v| v.grains).filter(|g| g.age < g.length).count() >= 32 * 7);
        let start = std::time::Instant::now();
        for _ in 0..200 {
            sampler.process(&context, &mut block);
            std::hint::black_box(&block);
        }
        let elapsed = start.elapsed().as_secs_f64();
        println!(
            "32 voices, bounded to 256 grains total, 48 kHz: {:.3} s for {:.3} s audio ({:.1}% real-time budget)",
            elapsed,
            51200. / 48000.,
            elapsed / (51200. / 48000.) * 100.
        );
        assert!(block.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn cached_hann_matches_the_formula_including_endpoints() {
        let sampler = super::Sampler::new();
        for length in [2, 17, 480, 24000] {
            for age in 0..length {
                let phase = age as f64 / (length - 1) as f64;
                let expected = 0.5 - 0.5 * (std::f64::consts::TAU * phase).cos();
                assert!((super::grain_window(&sampler.window, phase * 2048.) - expected).abs() < 1e-6);
            }
        }
    }
}

impl Sampler {
    pub(crate) fn prepare_block(&mut self, context: &ProcessContext) {
        self.set_context(context);
        let p = self.values;
        let grain_limit = (256 / self.voices.iter().filter(|v| v.active).count().max(1)).min(32);
        for voice in &mut self.voices {
            for grain in voice.grains.iter_mut().skip(grain_limit) {
                grain.length = 0;
            }
        }
        let target_grain_gain = 1. / (p[23] * p[22] / 1000. * 0.5).min(grain_limit as f64 * 0.5).max(1.);
        let target_gain = 10f64.powf(p[18] / 20.);
        self.block_grain_limit = grain_limit;
        self.block_grain_gain = target_grain_gain;
        self.block_gain = target_gain;
    }

    pub(crate) fn process_prepared(&mut self, buffer: &mut [f32]) {
        let p = self.values;
        let total = self.frames() as f64;
        let start = p[1] * total;
        let end = p[2] * total;
        let lo = (p[3] * total).max(start);
        let hi = (p[4] * total).min(end);
        let looping = p[8] == 2. && hi > lo + 1.;
        let grain_limit = self.block_grain_limit;
        let target_grain_gain = self.block_grain_gain;
        let target_gain = self.block_gain;
        for frame in buffer.as_chunks_mut::<2>().0 {
            let mut out = [0.; 2];
            let phase = self.phase + lfo_hz(p[37], p[81], p[82], p[80], self.tempo) / self.rate;
            if phase >= 1. {
                self.lfo_cycle = self.lfo_cycle.wrapping_add(1);
            }
            self.phase = phase.fract();
            let shape =
                suhina_dsp::lfo::Shape::ALL.get(p[56] as usize).copied().unwrap_or(suhina_dsp::lfo::Shape::Sine);
            let modulation =
                suhina_dsp::lfo::wave(shape, if p[56] == 5. { f64::from(self.lfo_cycle) } else { self.phase });
            let lfo = modulation * p[38];
            let volume = 1. - p[54] * (modulation + 1.) * 0.5;
            self.gain += (target_gain - self.gain) * (1. / (0.005 * self.rate));
            self.grain_gain += (target_grain_gain - self.grain_gain) * (1. / (0.005 * self.rate));
            self.pan += (p[19] - self.pan) * (1. / (0.005 * self.rate));
            for (index, v) in self.voices.iter_mut().enumerate() {
                if !v.active {
                    continue;
                }
                if index >= (if p[40] > 0. { 1 } else { p[7] as usize }) || end <= start + 1. || total < 2. {
                    v.active = false;
                    continue;
                }
                let slices = p[0] == 0. && p[8] == 3.;
                let (start, end) = if slices { (v.slice_start * total, v.slice_end * total) } else { (start, end) };
                if !crate::nodes::envelopes::step(
                    &mut v.env,
                    &mut v.stage,
                    v.released,
                    [p[11], p[12], p[13], p[14]],
                    self.rate,
                ) {
                    v.active = false;
                    continue;
                }
                let pitch_env = if v.age < p[42] {
                    v.age / p[42]
                } else {
                    (1. - (v.age - p[42]) / p[43].max(1. / self.rate)).max(0.)
                };
                v.age += 1. / self.rate;
                let target_step = 2f64.powf(
                    ((if slices { 0. } else { f64::from(v.key) - p[30] })
                        + p[5]
                        + p[6] / 100.
                        + self.bend
                        + lfo
                        + pitch_env * p[41])
                        / 12.,
                ) * self.source_rate
                    / self.rate;
                v.step += (target_step - v.step) / (p[29] * self.rate).max(1.);
                let mut signal = [0.; 2];
                if p[0] == 0. {
                    if v.position < start || v.position >= end {
                        v.active = false;
                        continue;
                    }
                    let speed = if p[83] > 0. {
                        self.tempo / p[83]
                    } else {
                        (p[2] - p[1]) * total / self.source_rate * self.tempo / (60. * p[79])
                    }
                    .clamp(1e-6, 1e6);
                    let warped = p[77] > 0.;
                    let stretched = warped && p[78] > 0. && (0.25..=4.).contains(&speed);
                    signal = if stretched {
                        self.warp.get_mut(index).map_or([0.; 2], |warp| {
                            warp.next(speed, v.step * self.rate / self.source_rate, |ahead| {
                                let mut position = v.position + ahead * self.source_rate / self.rate * v.direction;
                                if looping
                                    && ((v.direction > 0. && position >= hi) || (v.direction < 0. && position < lo))
                                {
                                    let span = hi - lo;
                                    if p[9] > 0. {
                                        let phase = (position - lo).rem_euclid(2. * (span - 1.));
                                        position =
                                            lo + if phase < span - 1. { phase } else { 2. * (span - 1.) - phase };
                                    } else if position >= hi || position < lo {
                                        position = lo + (position - lo).rem_euclid(span);
                                    }
                                }
                                if position < start || position >= end {
                                    [0.; 2]
                                } else {
                                    read(self.source.as_ref().as_ref(), self.channels, position)
                                }
                            })
                        })
                    } else {
                        read(self.source.as_ref().as_ref(), self.channels, v.position)
                    };
                    let (fade_start, fade_end) =
                        if looping && v.position >= lo && v.position < hi { (lo, hi) } else { (start, end) };
                    let fade = source_fade(v.position, fade_start, fade_end, p[57], p[58], self.source_rate);
                    signal.iter_mut().for_each(|sample| *sample *= fade);
                    v.position +=
                        if warped { speed * self.source_rate / self.rate * v.direction } else { v.step * v.direction };
                    let boundary = if p[9] > 0. { hi - 1. } else { hi };
                    if looping
                        && ((v.direction > 0. && v.position >= boundary) || (v.direction < 0. && v.position < lo))
                    {
                        let length = boundary - lo;
                        if p[9] > 0. {
                            let phase = (v.position - lo).rem_euclid(2. * length);
                            v.position = lo + if phase < length { phase } else { 2. * length - phase };
                            v.position = v.position.min(boundary);
                            v.direction *= if phase < length { 1. } else { -1. };
                        } else {
                            v.position = lo + (v.position - lo).rem_euclid(length);
                        }
                    }
                } else {
                    if p[26] == 0. {
                        v.scan += p[21] * self.source_rate / self.rate;
                    }
                    if crate::nodes::granular::tick(&mut v.next_grain, self.rate / p[23])
                        && let Some(g) = v.grains.iter_mut().take(grain_limit).find(|g| g.age >= g.length)
                    {
                        let length = (p[22] * self.rate / 1000.).max(2.) as usize;
                        *g = Grain {
                            window_step: 2048. / (length - 1) as f64,
                            position: start
                                + ((p[20] * (end - start)
                                    + v.scan
                                    + random(&mut self.seed) * p[24] * (end - start) * 0.5)
                                    .rem_euclid(end - start)),
                            age: 0,
                            length,
                            step: v.step * v.direction,
                            pan: random(&mut self.seed) * p[25],
                        };
                    }
                    for g in v.grains.iter_mut().take(grain_limit) {
                        if g.age >= g.length {
                            continue;
                        }
                        let window = grain_window(&self.window, g.age as f64 * g.window_step);
                        let position = start + (g.position - start).rem_euclid(end - start);
                        let data = read(self.source.as_ref().as_ref(), self.channels, position);
                        let window = window * source_fade(position, start, end, p[57], p[58], self.source_rate);
                        signal[0] += data[0] * window * (1. - g.pan.max(0.)) * self.grain_gain;
                        signal[1] += data[1] * window * (1. + g.pan.min(0.)) * self.grain_gain;
                        g.age += 1;
                        g.position += g.step;
                    }
                }
                v.filter_env = if v.stage == 3 {
                    (v.filter_env - v.filter_released / (p[51] * self.rate).max(1.)).max(0.)
                } else if v.age < p[48] {
                    v.age / p[48].max(1. / self.rate)
                } else {
                    (1. - (v.age - p[48]) / p[49].max(1. / self.rate) * (1. - p[50])).max(p[50])
                };
                if p[15] > 0. {
                    let freq = (p[16]
                        * 2f64.powf(
                            v.filter_env * p[35]
                                + v.velocity * p[36]
                                + (f64::from(v.key) - p[30]) / 12. * p[46]
                                + modulation * p[53],
                        ))
                    .clamp(20., (self.rate * 0.49).min(20000.));
                    let c = match p[15] as u8 {
                        1 => Coefficients::lowpass(self.rate, freq, p[17]),
                        2 => Coefficients::highpass(self.rate, freq, p[17]),
                        3 => Coefficients::bandpass(self.rate, freq, p[17]),
                        _ => {
                            let bp = Coefficients::bandpass(self.rate, freq, p[17]);
                            Coefficients { b0: 1. - bp.b0, b1: bp.a1 - bp.b1, b2: bp.a2 - bp.b2, ..bp }
                        }
                    };
                    for (ch, sample) in signal.iter_mut().enumerate() {
                        if let (Some(filter), Some(second)) = (v.filters.get_mut(ch), v.filters_24.get_mut(ch)) {
                            filter.set(c);
                            second.set(c);
                            *sample = filter.process(*sample);
                            if p[45] > 0. {
                                *sample = second.process(*sample);
                            }
                        }
                    }
                }
                for ch in 0..2 {
                    let next = signal[ch] * v.env * (1. - p[28] + p[28] * v.velocity);
                    v.last[ch] = next * (1. - v.fade) + v.fade_from[ch] * v.fade;
                    out[ch] += v.last[ch];
                }
                v.fade = (v.fade - 1. / (0.005 * self.rate)).max(0.);
            }
            let pan = (self.pan + modulation * p[55]).clamp(-1., 1.);
            let limit = f64::from(f32::MAX);
            frame[0] = (out[0] * self.gain * volume * (1. - pan.max(0.))).clamp(-limit, limit) as f32;
            frame[1] = (out[1] * self.gain * volume * (1. + pan.min(0.))).clamp(-limit, limit) as f32;
        }
    }
}
