// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed immutable sample resources; all binding and map validation occurs off-thread.
use super::{NodeInfo, PortInfo};
use crate::native_node::NativeNode;
use crate::sample_resource::{PreparedSamples, SampleAsset};
use crate::{DeviceSdkError, NodeKind, ProcessContext, SampleInfoField};
use std::sync::Arc;
use suhina_device_api::NoteEvent;
pub(crate) const SAMPLE_ASSET: NodeInfo = NodeInfo {
    kind: "sample_asset",
    name: "SampleAsset",
    rust_variant: "SampleAsset",
    inputs: &[],
    outputs: &["out"],
    description: "Prepared immutable sample binding.",
    configuration: "see resource contract",
    example: r#"{"kind":"sample_asset","sample":"sample"}"#,
};
pub(crate) const SAMPLE_INFO: NodeInfo = NodeInfo {
    kind: "sample_info",
    name: "SampleInfo",
    rust_variant: "SampleInfo",
    inputs: &[PortInfo { name: "asset", default: None, unit: "sample" }],
    outputs: &["out", "left", "right"],
    description: "Prepared sample metadata selected by field.",
    configuration: "see resource contract",
    example: r#"{"kind":"sample_info","field":"frames"}"#,
};
pub(crate) const SAMPLE_REGION: NodeInfo = NodeInfo {
    kind: "sample_region",
    name: "SampleRegion",
    rust_variant: "SampleRegion",
    inputs: &[
        PortInfo { name: "asset", default: None, unit: "sample" },
        PortInfo { name: "start", default: Some(0.0), unit: "frames" },
        PortInfo { name: "end", default: Some(-1.0), unit: "frames" },
    ],
    outputs: &["out"],
    description: "Checked half-open frame region, negative end means full sample.",
    configuration: "see resource contract",
    example: r#"{"kind":"sample_region"}"#,
};
pub(crate) const SAMPLE_READ: NodeInfo = NodeInfo {
    kind: "sample_read",
    name: "SampleRead",
    rust_variant: "SampleRead",
    inputs: &[
        PortInfo { name: "asset", default: None, unit: "sample" },
        PortInfo { name: "position", default: Some(0.0), unit: "frames" },
        PortInfo { name: "region", default: None, unit: "region" },
    ],
    outputs: &["out", "left", "right"],
    description: "Linear sample read within a checked region.",
    configuration: "see resource contract",
    example: r#"{"kind":"sample_read"}"#,
};
pub(crate) const SAMPLE_TEMPO: NodeInfo = NodeInfo {
    kind: "sample_tempo",
    name: "SampleTempo",
    rust_variant: "SampleTempo",
    inputs: &[PortInfo { name: "asset", default: None, unit: "sample" }],
    outputs: &["out", "left", "right"],
    description: "Source BPM or duration in beats; unknown tempo is zero.",
    configuration: "see resource contract",
    example: r#"{"kind":"sample_tempo","beats":false}"#,
};
pub(crate) const SLICE_MAP: NodeInfo = NodeInfo {
    kind: "slice_map",
    name: "SliceMap",
    rust_variant: "SliceMap",
    inputs: &[],
    outputs: &["out", "count"],
    description: "Prepared immutable manual slice boundaries.",
    configuration: "see resource contract",
    example: r#"{"kind":"slice_map","sample":"sample","boundaries":[0,1]}"#,
};
pub(crate) const SLICE_SELECT: NodeInfo = NodeInfo {
    kind: "slice_select",
    name: "SliceSelect",
    rust_variant: "SliceSelect",
    inputs: &[
        PortInfo { name: "map", default: None, unit: "slice_map" },
        PortInfo { name: "index", default: Some(0.0), unit: "index" },
    ],
    outputs: &["out"],
    description: "Select a half-open slice region, or empty for invalid indices.",
    configuration: "see resource contract",
    example: r#"{"kind":"slice_select"}"#,
};

pub(crate) const GRAIN_READ: NodeInfo = NodeInfo {
    kind: "grain_read",
    name: "GrainRead",
    rust_variant: "GrainRead",
    inputs: &[
        PortInfo { name: "asset", default: None, unit: "sample" },
        PortInfo { name: "position", default: Some(0.0), unit: "frames" },
        PortInfo { name: "rate", default: Some(1.0), unit: "frames/sample" },
        PortInfo { name: "duration", default: Some(0.12), unit: "seconds" },
        PortInfo { name: "events", default: Some(0.0), unit: "midi" },
    ],
    outputs: &["out", "left", "right", "age", "finished", "position"],
    description: "Triggered one-shot grain read with age and completion.",
    configuration: "see sample playback contracts",
    example: r#"{"kind":"grain_read"}"#,
};
pub(crate) const SLICE_PLAYBACK: NodeInfo = NodeInfo {
    kind: "slice_playback",
    name: "SlicePlayback",
    rust_variant: "SlicePlayback",
    inputs: &[
        PortInfo { name: "region", default: None, unit: "region" },
        PortInfo { name: "rate", default: Some(1.0), unit: "frames/sample" },
        PortInfo { name: "gate", default: Some(0.0), unit: "signal" },
        PortInfo { name: "events", default: Some(0.0), unit: "midi" },
    ],
    outputs: &["out", "left", "right", "position", "finished"],
    description: "Triggered selected-region playback in gate or one-shot mode.",
    configuration: "see sample playback contracts",
    example: r#"{"kind":"slice_playback","sample":"sample","one_shot":true}"#,
};
pub(crate) const LOOP_CROSSFADE: NodeInfo = NodeInfo {
    kind: "loop_crossfade",
    name: "LoopCrossfade",
    rust_variant: "LoopCrossfade",
    inputs: &[
        PortInfo { name: "asset", default: None, unit: "sample" },
        PortInfo { name: "position", default: Some(0.0), unit: "frames" },
        PortInfo { name: "region", default: None, unit: "region" },
        PortInfo { name: "fade", default: Some(0.0), unit: "frames" },
    ],
    outputs: &["out", "left", "right", "primary", "secondary", "mix"],
    description: "Periodic two-head loop reader with a linear overlap crossfade.",
    configuration: "see sample playback contracts",
    example: r#"{"kind":"loop_crossfade"}"#,
};
/// The longest sample a resource graph binds, in frames: positions travel as f32 signals, which
/// count frames exactly up to 2^24 (349 s at 48 kHz).
pub(crate) const MAX_SAMPLE_FRAMES: usize = 16_777_216;
pub(crate) struct Resources {
    assets: Vec<(String, Arc<SampleAsset>)>,
    maps: Vec<(String, Vec<u64>)>,
}
impl Resources {
    pub(crate) fn asset(&self, name: &str) -> Result<Arc<SampleAsset>, DeviceSdkError> {
        self.assets
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, a)| a.clone())
            .ok_or_else(|| DeviceSdkError(format!("missing sample {name:?}")))
    }
    pub(crate) fn new(graph: &crate::DeviceGraph, samples: &PreparedSamples) -> Result<Arc<Self>, DeviceSdkError> {
        let mut total = 0usize;
        for (_, asset) in &samples.assets {
            if asset.frames() > MAX_SAMPLE_FRAMES {
                // Positions travel as f32 signals, exact only up to 2^24 frames.
                let rate = f64::from(asset.sample_rate());
                return Err(DeviceSdkError(format!(
                    "the sample is {:.0} s long; at its {} Hz a sample graph takes at most {:.0} s ({MAX_SAMPLE_FRAMES} frames)",
                    asset.frames() as f64 / rate,
                    asset.sample_rate(),
                    MAX_SAMPLE_FRAMES as f64 / rate,
                )));
            }
            total = total
                .checked_add(
                    asset
                        .frames()
                        .checked_mul(asset.channels())
                        .ok_or_else(|| DeviceSdkError("sample size overflow".into()))?,
                )
                .ok_or_else(|| DeviceSdkError("sample pool size overflow".into()))?;
        }
        if total > 64 * 1024 * 1024 {
            return Err(DeviceSdkError("sample resource pool exceeds 256 MiB".into()));
        }
        let mut maps = Vec::new();
        for node in &graph.nodes {
            if let NodeKind::SliceMap { sample, boundaries } = &node.kind {
                let asset = samples
                    .assets
                    .iter()
                    .find(|(id, _)| id == sample)
                    .ok_or_else(|| DeviceSdkError(format!("missing sample {sample:?}")))?;
                if boundaries.len() < 2
                    || boundaries.len() > 4096
                    || boundaries.windows(2).any(|p| p[0] >= p[1])
                    || boundaries.last().is_some_and(|&v| v > asset.1.frames() as u64)
                {
                    return Err(DeviceSdkError(
                        "slice boundaries must be sorted, within the sample, and 2..4096 entries".into(),
                    ));
                }
                maps.push((sample.clone(), boundaries.clone()));
            }
        }
        Ok(Arc::new(Self { assets: samples.assets.clone(), maps }))
    }
}
struct Kernel {
    kind: NodeKind,
    resources: Arc<Resources>,
    handle: usize,
    rate: f64,
    age: u64,
    active: bool,
    pending: bool,
    event_gate: bool,
    gate: bool,
    position: f64,
    cache: [[[f32; 2]; 64]; 3],
}
pub(crate) fn prepare(
    kind: &NodeKind,
    resources: Arc<Resources>,
    rate: f64,
) -> Result<Box<dyn NativeNode>, DeviceSdkError> {
    let handle = match kind {
        NodeKind::SampleAsset { sample } | NodeKind::SlicePlayback { sample, .. } => resources
            .assets
            .iter()
            .position(|(id, _)| id == sample)
            .ok_or_else(|| DeviceSdkError(format!("missing sample {sample:?}")))?,
        NodeKind::SliceMap { sample, boundaries } => resources
            .maps
            .iter()
            .position(|(id, points)| id == sample && points == boundaries)
            .ok_or_else(|| DeviceSdkError("missing prepared slice map".into()))?,
        _ => 0,
    };
    Ok(Box::new(Kernel {
        kind: kind.clone(),
        resources,
        handle,
        rate,
        age: 0,
        active: false,
        pending: false,
        event_gate: false,
        gate: false,
        position: 0.,
        cache: [[[0.; 2]; 64]; 3],
    }))
}
fn index(value: f32) -> Option<usize> {
    (value.is_finite() && value >= 0. && value.fract() == 0. && value < 128.).then_some(value as usize)
}
impl NativeNode for Kernel {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let read = |port: usize, channel: usize| inputs[port].get(frame * 2 + channel).copied().unwrap_or(0.);
            let asset = if matches!(self.kind, NodeKind::SlicePlayback { .. }) {
                self.resources.assets.get(self.handle).map(|(_, a)| a.as_ref())
            } else {
                index(read(0, 0)).and_then(|i| self.resources.assets.get(i)).map(|(_, a)| a.as_ref())
            };
            *pair = match &self.kind {
                NodeKind::GrainRead => {
                    if self.pending {
                        self.age = 0;
                        self.active = true;
                        self.pending = false;
                    }
                    let duration = (f64::from(read(3, 0)).clamp(0., 60.) * self.rate).ceil() as u64;
                    let position = f64::from(read(1, 0)) + self.age as f64 * f64::from(read(2, 0));
                    let data = if let Some(asset) = asset {
                        if self.active && self.age < duration && position >= 0. && position < asset.frames() as f64 {
                            asset.read_region_stereo(0, asset.frames(), position)
                        } else {
                            self.active = false;
                            [0.; 2]
                        }
                    } else {
                        self.active = false;
                        [0.; 2]
                    };
                    self.cache[0][frame] = [(self.age as f64 / self.rate) as f32; 2];
                    self.cache[1][frame] = [f32::from(!self.active); 2];
                    self.cache[2][frame] = [position as f32; 2];
                    if self.active {
                        self.age = self.age.saturating_add(1);
                    }
                    data
                }
                NodeKind::SlicePlayback { one_shot, .. } => {
                    let start = f64::from(read(0, 0));
                    let end = f64::from(read(0, 1));
                    let rate = f64::from(read(1, 0));
                    let gate = read(2, 0) > 0. || self.event_gate;
                    if self.pending || (gate && !self.gate) {
                        self.position = if rate >= 0. { start } else { end - 1. };
                        self.active = true;
                        self.pending = false;
                    }
                    self.gate = gate;
                    if !one_shot && !gate {
                        self.active = false;
                    }
                    let data = if let Some(asset) = asset {
                        if self.active
                            && start >= 0.
                            && end <= asset.frames() as f64
                            && self.position >= start
                            && self.position < end
                        {
                            asset.read_region_stereo(start as usize, end as usize, self.position - start)
                        } else {
                            self.active = false;
                            [0.; 2]
                        }
                    } else {
                        self.active = false;
                        [0.; 2]
                    };
                    self.cache[0][frame] = [self.position as f32; 2];
                    self.cache[1][frame] = [f32::from(!self.active); 2];
                    if self.active {
                        self.position += rate;
                    }
                    data
                }
                NodeKind::LoopCrossfade => asset.map_or([0.; 2], |a| {
                    for cache in &mut self.cache {
                        cache[frame] = [0.; 2];
                    }
                    let start = f64::from(read(2, 0));
                    let end = f64::from(read(2, 1));
                    let length = end - start;
                    if start < 0. || end > a.frames() as f64 || length <= 0. {
                        return [0.; 2];
                    }
                    let fade = f64::from(read(3, 0)).clamp(0., length / 2.);
                    let period = length - fade;
                    let phase = (f64::from(read(1, 0)) - start).rem_euclid(period);
                    let primary = start + fade + phase;
                    let mix = if fade > 0. { ((phase - (length - 2. * fade)) / fade).clamp(0., 1.) } else { 0. };
                    let secondary = if mix > 0. { start + phase - (length - 2. * fade) } else { primary };
                    self.cache[0][frame] = [primary as f32; 2];
                    self.cache[1][frame] = [secondary as f32; 2];
                    self.cache[2][frame] = [mix as f32; 2];
                    let first = a.read_region_stereo(start as usize, end as usize, primary - start);
                    let second = a.read_region_stereo(start as usize, end as usize, secondary - start);
                    std::array::from_fn(|c| ((1. - mix) * f64::from(first[c]) + mix * f64::from(second[c])) as f32)
                }),
                NodeKind::SampleAsset { .. } | NodeKind::SliceMap { .. } => [self.handle as f32; 2],
                NodeKind::SampleInfo { field } => {
                    let v = asset.map_or(0., |a| match field {
                        SampleInfoField::Frames => a.frames() as f64,
                        SampleInfoField::Duration => a.duration_seconds(),
                        SampleInfoField::SampleRate => f64::from(a.sample_rate()),
                        SampleInfoField::Channels => a.channels() as f64,
                    });
                    [v as f32; 2]
                }
                NodeKind::SampleTempo { beats } => {
                    let v = asset
                        .and_then(|a| {
                            a.source_bpm().map(|bpm| if *beats { a.duration_seconds() * bpm / 60. } else { bpm })
                        })
                        .unwrap_or(0.);
                    [v as f32; 2]
                }
                NodeKind::SampleRegion => asset.map_or([0.; 2], |a| {
                    let start = read(1, 0);
                    let end = read(2, 0);
                    let end = if end < 0. { a.frames() as f32 } else { end };
                    if !start.is_finite() || !end.is_finite() || start < 0. || end < start || end > a.frames() as f32 {
                        [0.; 2]
                    } else {
                        [start.floor(), end.floor()]
                    }
                }),
                NodeKind::SampleRead => asset.map_or([0.; 2], |a| {
                    let start = read(2, 0);
                    let end = read(2, 1);
                    if !start.is_finite() || !end.is_finite() || start < 0. || end < 0. {
                        [0.; 2]
                    } else {
                        a.read_region_stereo(start as usize, end as usize, f64::from(read(1, 0)))
                    }
                }),
                NodeKind::SliceSelect => index(read(0, 0))
                    .and_then(|i| self.resources.maps.get(i))
                    .and_then(|(_, points)| {
                        let n = read(1, 0);
                        if !n.is_finite() || n < 0. || n.fract() != 0. || n >= 4096. {
                            None
                        } else {
                            let i = n as usize;
                            points.get(i).zip(points.get(i + 1)).map(|(&a, &b)| [a as f32, b as f32])
                        }
                    })
                    .unwrap_or([0.; 2]),
                _ => [0.; 2],
            };
        }
    }
    fn output(&self, port: usize, frame: usize) -> [f32; 2] {
        if matches!(self.kind, NodeKind::SliceMap { .. }) {
            [self.resources.maps.get(self.handle).map_or(0, |(_, p)| p.len().saturating_sub(1)) as f32; 2]
        } else {
            self.cache.get(port).and_then(|p| p.get(frame)).copied().unwrap_or([0.; 2])
        }
    }
    fn event(&mut self, event: NoteEvent) {
        if !matches!(self.kind, NodeKind::GrainRead | NodeKind::SlicePlayback { .. }) {
            return;
        }
        match super::events::normalized(event) {
            Some(NoteEvent::On { .. }) => {
                self.pending = true;
                self.event_gate = true;
            }
            Some(NoteEvent::Off { .. }) => self.event_gate = false,
            Some(NoteEvent::AllNotesOff) => {
                self.pending = false;
                self.active = false;
                self.event_gate = false;
            }
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.age = 0;
        self.active = false;
        self.pending = false;
        self.event_gate = false;
        self.gate = false;
        self.position = 0.;
        self.cache.fill([[0.; 2]; 64]);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
