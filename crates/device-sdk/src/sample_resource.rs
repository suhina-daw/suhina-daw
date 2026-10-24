// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Immutable host-prepared PCM and checked regions; no file I/O or instrument state.
use crate::DeviceSdkError;
use std::sync::Arc;

type Pcm = Arc<dyn AsRef<[f32]> + Send + Sync>;

/// Construct, replace and drop off the audio thread. Reads do not allocate.
pub struct SampleAsset {
    pcm: Pcm,
    sample_rate: u32,
    channels: usize,
    frames: usize,
    bpm: Option<f64>,
}

/// Checked half-open interval, with positions relative to its start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRegion {
    start: usize,
    end: usize,
}

impl SampleAsset {
    pub fn new(pcm: Pcm, sample_rate: u32, channels: u16) -> Result<Self, DeviceSdkError> {
        let channels = usize::from(channels);
        if !(8000..=384000).contains(&sample_rate) || !(1..=32).contains(&channels) {
            return Err(DeviceSdkError("invalid sample rate or channel count".into()));
        }
        let samples = pcm.as_ref().as_ref().len();
        if !samples.is_multiple_of(channels) {
            return Err(DeviceSdkError("sample contains an incomplete frame".into()));
        }
        Ok(Self { pcm, sample_rate, channels, frames: samples / channels, bpm: None })
    }
    pub fn with_tempo(mut self, bpm: f64) -> Result<Self, DeviceSdkError> {
        if !bpm.is_finite() || !(1.0..=1000.0).contains(&bpm) {
            return Err(DeviceSdkError("invalid source BPM".into()));
        }
        self.bpm = Some(bpm);
        Ok(self)
    }
    pub fn source_bpm(&self) -> Option<f64> {
        self.bpm
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub(crate) fn shared_pcm(&self) -> Pcm {
        self.pcm.clone()
    }
    pub fn frames(&self) -> usize {
        self.frames
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    pub fn duration_seconds(&self) -> f64 {
        self.frames as f64 / f64::from(self.sample_rate)
    }
    pub fn region(&self, start: usize, end: usize) -> Result<SampleRegion, DeviceSdkError> {
        if start > end || end > self.frames {
            return Err(DeviceSdkError("sample region is outside prepared PCM".into()));
        }
        Ok(SampleRegion { start, end })
    }
    /// Allocation-free checked read for dynamic graph bounds, including invalid inputs.
    pub fn read_region_stereo(&self, start: usize, end: usize, position: f64) -> [f32; 2] {
        if start > end || end > self.frames {
            return [0.; 2];
        }
        self.read_stereo(SampleRegion { start, end }, position)
    }
    /// Linear read in region-relative frames. Regions from another asset are rechecked.
    pub fn read_stereo(&self, region: SampleRegion, position: f64) -> [f32; 2] {
        let length = region.end.saturating_sub(region.start);
        if region.end > self.frames
            || length == 0
            || !position.is_finite()
            || position < 0.
            || position >= length as f64
        {
            return [0.; 2];
        }
        let first = position.floor() as usize;
        let second = first.saturating_add(1).min(length - 1);
        let fraction = position - first as f64;
        let read = |frame: usize, channel: usize| {
            let index = region
                .start
                .checked_add(frame)
                .and_then(|v| v.checked_mul(self.channels))
                .and_then(|v| v.checked_add(channel));
            let value = index.and_then(|v| self.pcm.as_ref().as_ref().get(v)).copied().unwrap_or(0.);
            if value.is_finite() { f64::from(value) } else { 0. }
        };
        std::array::from_fn(|channel| {
            let channel = channel.min(self.channels - 1);
            let a = read(first, channel);
            let b = read(second, channel);
            (a + (b - a) * fraction) as f32
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pcm(values: Vec<f32>) -> Pcm {
        Arc::new(values)
    }
    #[test]
    fn validates_resources_and_regions() {
        assert!(SampleAsset::new(pcm(vec![]), 0, 1).is_err());
        assert!(SampleAsset::new(pcm(vec![]), 48000, 0).is_err());
        assert!(SampleAsset::new(pcm(vec![1.]), 48000, 2).is_err());
        let sample = SampleAsset::new(pcm(vec![1., 3.]), 48000, 1).unwrap();
        assert!(sample.region(2, 1).is_err());
        assert!(sample.region(0, usize::MAX).is_err());
        assert_eq!(sample.read_stereo(sample.region(0, 2).unwrap(), 0.5), [2.; 2]);
        assert_eq!(sample.read_stereo(sample.region(1, 2).unwrap(), 0.5), [3.; 2]);
        assert_eq!(sample.read_stereo(sample.region(0, 0).unwrap(), 0.), [0.; 2]);
        for position in [-1., f64::NAN, f64::INFINITY, 2.] {
            assert_eq!(sample.read_stereo(sample.region(0, 2).unwrap(), position), [0.; 2]);
        }
    }
    #[test]
    fn stereo_and_foreign_regions_are_checked() {
        let sample = SampleAsset::new(pcm(vec![1., -1., 3., -3.]), 48000, 2).unwrap();
        let region = sample.region(0, 2).unwrap();
        assert_eq!(sample.read_stereo(region, 0.5), [2., -2.]);
        let small = SampleAsset::new(pcm(vec![f32::NAN]), 48000, 1).unwrap();
        assert_eq!(small.read_stereo(region, 0.), [0.; 2]);
        assert_eq!(small.read_stereo(small.region(0, 1).unwrap(), 0.), [0.; 2]);
    }
}

/// Stable host bindings; insertion and graph preparation belong off the audio thread.
#[derive(Default, Clone)]
pub struct PreparedSamples {
    pub(crate) assets: Vec<(String, Arc<SampleAsset>)>,
}
impl PreparedSamples {
    pub fn insert(&mut self, name: &str, sample: Arc<SampleAsset>) -> Result<(), DeviceSdkError> {
        if name.is_empty() || name.len() > 128 {
            return Err(DeviceSdkError("invalid sample binding name".into()));
        }
        if let Some(entry) = self.assets.iter_mut().find(|(id, _)| id == name) {
            entry.1 = sample;
            return Ok(());
        }
        if self.assets.len() >= 128 {
            return Err(DeviceSdkError("at most 128 sample bindings".into()));
        }
        self.assets.push((name.into(), sample));
        Ok(())
    }
}
impl crate::DeviceGraph {
    pub fn compile_with_samples_at(
        &self,
        rate: f64,
        samples: &PreparedSamples,
    ) -> Result<crate::PreparedEffect, DeviceSdkError> {
        self.compile_with_samples_and_native_at(rate, samples, |id, _| {
            Err(DeviceSdkError(format!("unregistered native effect {id:?}")))
        })
    }
    pub fn compile_with_samples_and_native_at(
        &self,
        rate: f64,
        samples: &PreparedSamples,
        mut resolve: impl FnMut(&str, f64) -> Result<Box<dyn crate::native_node::NativeNode>, DeviceSdkError>,
    ) -> Result<crate::PreparedEffect, DeviceSdkError> {
        self.validate()?;
        if self.role == crate::DeviceRole::MidiEffect {
            return Err(DeviceSdkError("sample nodes require an audio graph".into()));
        }
        crate::native_node::compile_graph(self, rate, Some(samples), &mut resolve)
    }
}

/// Off-thread analysis modes. The resulting .sd stores resolved boundaries, not analysis work.
#[derive(Debug, Clone)]
pub enum SliceMode {
    Manual(Vec<u64>),
    Transients { sensitivity: f64 },
    TimeGrid { seconds: f64 },
    BeatGrid { beats: f64, bpm: Option<f64> },
}
impl PreparedSamples {
    pub fn slice_map(&self, name: &str, mode: SliceMode) -> Result<crate::NodeKind, DeviceSdkError> {
        let asset = self
            .assets
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, a)| a)
            .ok_or_else(|| DeviceSdkError(format!("missing sample {name:?}")))?;
        let frames = asset.frames() as u64;
        if frames == 0 {
            return Err(DeviceSdkError("an empty sample has no slices".into()));
        }
        let mut boundaries = vec![0, frames];
        match mode {
            SliceMode::Manual(points) => {
                if points.len() > 4094 || points.iter().any(|&v| v > frames) {
                    return Err(DeviceSdkError("invalid manual slice points".into()));
                }
                boundaries.extend(points);
            }
            SliceMode::Transients { sensitivity } => {
                if !sensitivity.is_finite() || !(0. ..=1.).contains(&sensitivity) {
                    return Err(DeviceSdkError("slice sensitivity must be 0..1".into()));
                }
                let pcm = asset.shared_pcm();
                let analysis = crate::nodes::slicing::SliceAnalysis::new(
                    pcm.as_ref().as_ref(),
                    asset.channels(),
                    f64::from(asset.sample_rate()),
                );
                let mut parameters = [0.; 61];
                parameters[2] = 1.;
                parameters[59] = sensitivity;
                boundaries.extend(
                    analysis
                        .points(&parameters, asset.frames(), f64::from(asset.sample_rate()), 120.)
                        .into_iter()
                        .map(|v| (v * frames as f64).round() as u64),
                );
            }
            mode => {
                let seconds = match mode {
                    SliceMode::TimeGrid { seconds } => seconds,
                    SliceMode::BeatGrid { beats, bpm } => {
                        let bpm = bpm
                            .or(asset.source_bpm())
                            .ok_or_else(|| DeviceSdkError("beat slices need source BPM".into()))?;
                        if !bpm.is_finite() || bpm <= 0. || !beats.is_finite() || beats <= 0. {
                            return Err(DeviceSdkError("invalid beat grid".into()));
                        }
                        beats * 60. / bpm
                    }
                    _ => 0.,
                };
                if !seconds.is_finite() || seconds <= 0. {
                    return Err(DeviceSdkError("invalid time grid".into()));
                }
                let step = (seconds * f64::from(asset.sample_rate())).round().max(1.) as u64;
                let mut frame = step;
                while frame < frames {
                    if boundaries.len() >= 4096 {
                        return Err(DeviceSdkError("slice grid exceeds 4096 boundaries".into()));
                    }
                    boundaries.push(frame);
                    let Some(next) = frame.checked_add(step) else {
                        break;
                    };
                    frame = next;
                }
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        Ok(crate::NodeKind::SliceMap { sample: name.into(), boundaries })
    }
}
