// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Slice index selection; region maps and PCM stay in prepared resources.
use super::{NodeInfo, PortInfo};
use crate::{NodeKind, ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
pub(crate) const KEY_TO_SLICE: NodeInfo = NodeInfo {
    kind: "key_to_slice",
    name: "KeyToSlice",
    rust_variant: "KeyToSlice",
    inputs: &[
        PortInfo { name: "key", default: Some(60.0), unit: "signal" },
        PortInfo { name: "root", default: Some(60.0), unit: "signal" },
        PortInfo { name: "count", default: Some(16.0), unit: "signal" },
        PortInfo { name: "wrap", default: Some(0.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Key-to-index mapping with optional wrap.",
    configuration: "see slicing contracts",
    example: r#"{"kind":"key_to_slice"}"#,
};
pub(crate) const SLICE_SEQUENCE: NodeInfo = NodeInfo {
    kind: "slice_sequence",
    name: "SliceSequence",
    rust_variant: "SliceSequence",
    inputs: &[
        PortInfo { name: "count", default: Some(16.0), unit: "signal" },
        PortInfo { name: "trigger", default: Some(0.0), unit: "signal" },
        PortInfo { name: "step", default: Some(1.0), unit: "signal" },
        PortInfo { name: "start", default: Some(0.0), unit: "signal" },
        PortInfo { name: "events", default: Some(0.0), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Signed slice sequence advanced by triggers or note-ons.",
    configuration: "see slicing contracts",
    example: r#"{"kind":"slice_sequence"}"#,
};
pub(crate) const RANDOM_SLICE: NodeInfo = NodeInfo {
    kind: "random_slice",
    name: "RandomSlice",
    rust_variant: "RandomSlice",
    inputs: &[
        PortInfo { name: "count", default: Some(16.0), unit: "signal" },
        PortInfo { name: "trigger", default: Some(0.0), unit: "signal" },
        PortInfo { name: "events", default: Some(0.0), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Seeded random slice index, bounded by count.",
    configuration: "see slicing contracts",
    example: r#"{"kind":"random_slice","seed":1}"#,
};

struct Kernel {
    kind: NodeKind,
    initial: u32,
    seed: u32,
    current: i32,
    started: bool,
    trigger: bool,
    note: bool,
}
pub(crate) fn prepare(kind: &NodeKind) -> Option<Box<dyn NativeNode>> {
    if !matches!(kind, NodeKind::KeyToSlice | NodeKind::SliceSequence | NodeKind::RandomSlice { .. }) {
        return None;
    }
    let seed = if let NodeKind::RandomSlice { seed } = kind { *seed } else { 0 };
    Some(Box::new(Kernel {
        kind: kind.clone(),
        initial: seed,
        seed,
        current: -1,
        started: false,
        trigger: false,
        note: false,
    }))
}
impl NativeNode for Kernel {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let x: [f64; 5] = std::array::from_fn(|p| {
                let v = inputs[p].get(frame * 2).copied().unwrap_or(0.);
                if v.is_finite() { f64::from(v) } else { 0. }
            });
            let value = if matches!(self.kind, NodeKind::KeyToSlice) {
                let count = x[2].clamp(0., 4096.).floor() as i32;
                let index = (x[0] - x[1]).round().clamp(-8192., 8192.) as i32;
                if count <= 0 {
                    -1
                } else if x[3] > 0. {
                    index.rem_euclid(count)
                } else if (0..count).contains(&index) {
                    index
                } else {
                    -1
                }
            } else {
                let count = x[0].clamp(0., 4096.).floor() as i32;
                let trigger = x[1] > 0.;
                let fire = self.note || (trigger && !self.trigger);
                self.trigger = trigger;
                self.note = false;
                if count <= 0 {
                    self.current = -1;
                } else if fire {
                    self.current = if matches!(self.kind, NodeKind::RandomSlice { .. }) {
                        (((super::random::bipolar(&mut self.seed) + 1.) * 0.5 * f64::from(count)).floor() as i32)
                            .min(count - 1)
                    } else if self.started {
                        (self.current + x[2].round().clamp(-4096., 4096.) as i32).rem_euclid(count)
                    } else {
                        (x[3].round().clamp(-4096., 4096.) as i32).rem_euclid(count)
                    };
                    self.started = true;
                } else if self.current >= count {
                    self.current = self.current.rem_euclid(count);
                }
                self.current
            };
            *pair = [value as f32; 2];
        }
    }
    fn event(&mut self, event: NoteEvent) {
        if matches!(super::events::normalized(event), Some(NoteEvent::On { .. })) {
            self.note = true;
        }
    }
    fn reset(&mut self) {
        self.seed = self.initial;
        self.current = -1;
        self.started = false;
        self.trigger = false;
        self.note = false;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Source analysis happens on loading, never on the audio thread.
#[derive(Clone)]
pub struct SliceAnalysis {
    attacks: [(f64, f64); 127],
}
impl Default for SliceAnalysis {
    fn default() -> Self {
        Self { attacks: [(1., 0.); 127] }
    }
}
impl SliceAnalysis {
    /// Identity of a candidate before applying user corrections.
    pub fn identity(p: &[f64], frames: usize, rate: f64, point: f64) -> i64 {
        let value = |i: usize, default| p.get(i).copied().unwrap_or(default);
        match value(60, 0.) as u8 {
            1 => {
                let division = crate::sample_axis::SampleAxis::division(
                    value(88, 3.) as usize,
                    value(91, 4.),
                    value(92, 1.) as usize,
                );
                crate::sample_axis::SampleAxis::new(frames, rate, value(83, 0.), value(85, 0.))
                    .map_or(0, |axis| (axis.beat(point) / division).round() as i64)
            }
            2 => (point * value(89, 16.).round().clamp(1., 128.)).round() as i64,
            _ => (point * frames as f64).round() as i64,
        }
    }

    /// Merge prepared corrections without allocating; only points inside the trim are emitted.
    pub fn edited_points<'a>(
        &self,
        p: &[f64],
        frames: usize,
        rate: f64,
        tempo: f64,
        edits: &'a suhina_device_api::sample_edits::SampleEdits,
    ) -> EditedPoints<'a> {
        let start = p.get(1).copied().unwrap_or(0.);
        let end = p.get(2).copied().unwrap_or(1.);
        let method = p.get(60).copied().unwrap_or(0.) as u8;
        let mut automatic = self.points(p, frames, rate, tempo);
        for point in &mut automatic {
            if method == 3
                || *point <= start
                || *point >= end
                || edits.excludes(method, Self::identity(p, frames, rate, *point))
            {
                *point = end;
            }
        }
        automatic.sort_unstable_by(f64::total_cmp);
        EditedPoints {
            automatic,
            pins: edits.pins(),
            auto_index: 0,
            pin_index: edits.pins().partition_point(|point| *point <= start),
            end,
            last: start,
        }
    }
    pub fn new(source: &[f32], channels: usize, rate: f64) -> Self {
        let mut result = Self::default();
        let channels = channels.max(1);
        let frames = source.len() / channels;
        let window = (rate * 0.005).max(1.) as usize;
        let mut previous = 0.;
        for (i, block) in source.chunks(window.saturating_mul(channels).max(1)).enumerate() {
            let peak = block.iter().filter(|v| v.is_finite()).fold(0f64, |p, v| p.max(f64::from(v.abs())));
            let rise = (peak - previous).max(0.);
            previous = peak;
            if i == 0 || rise <= 0. {
                continue;
            }
            if let Some(slot) = result.attacks.iter_mut().min_by(|a, b| a.1.total_cmp(&b.1))
                && rise > slot.1
            {
                *slot = ((i.saturating_mul(window)) as f64 / frames.max(1) as f64, rise);
            }
        }
        result
    }
    pub fn points(&self, p: &[f64], frames: usize, rate: f64, tempo: f64) -> [f64; 129] {
        let value = |i: usize, default| p.get(i).copied().unwrap_or(default);
        let start = value(1, 0.);
        let end = value(2, 1.);
        let sensitivity = value(59, 0.5).clamp(0., 1.);
        let mut points = [end; 129];
        points[0] = start;
        let mut n = 1;
        let mut add = |point: f64| {
            if point > start && point < end && n < 128 {
                points[n] = point;
                n += 1;
            }
        };
        match value(60, 0.) as u8 {
            0 => {
                let maximum = self.attacks.iter().fold(0f64, |v, a| v.max(a.1));
                for &(point, strength) in &self.attacks {
                    if strength > maximum * (1. - sensitivity) && strength > 0. {
                        add(point);
                    }
                }
            }
            1 => {
                let beats = [4., 2., 1., 0.5, 0.25, 0.125];
                let beat = beats.get((sensitivity * 5.).round() as usize).copied().unwrap_or(1.);
                if value(0, 0.) == 2. {
                    if let Some(axis) = crate::sample_axis::SampleAxis::new(frames, rate, value(83, 0.), value(85, 0.))
                    {
                        let division = crate::sample_axis::SampleAxis::division(
                            value(88, 3.) as usize,
                            value(91, 4.),
                            value(92, 1.) as usize,
                        );
                        let first = (axis.beat(start) / division).floor() + 1.;
                        for i in 0..127 {
                            add(axis.point((first + f64::from(i)) * division));
                        }
                    }
                } else {
                    // Saved legacy playback retains its project-tempo slicing behavior.
                    let step = beat * 60. / tempo.max(1.) * rate / frames.max(1) as f64;
                    for i in 1..128 {
                        add(start + i as f64 * step);
                    }
                }
            }
            2 => {
                let count = if value(0, 0.) == 2. {
                    value(89, 16.).round().clamp(1., 128.) as usize
                } else {
                    1 + (sensitivity * 31.).round() as usize
                };
                for i in 1..count {
                    add(if value(0, 0.) == 2. {
                        i as f64 / count as f64
                    } else {
                        start + (end - start) * i as f64 / count as f64
                    });
                }
            }
            _ => {
                for i in 61..77 {
                    add(value(i, 1.));
                }
            }
        }
        points.sort_unstable_by(f64::total_cmp);
        let mut unique = [end; 129];
        let mut count = 0;
        for point in points {
            if count == 0 || unique[count - 1] != point {
                unique[count] = point;
                count += 1;
            }
        }
        unique
    }
}

pub struct EditedPoints<'a> {
    automatic: [f64; 129],
    pins: &'a [f64],
    auto_index: usize,
    pin_index: usize,
    end: f64,
    last: f64,
}
impl Iterator for EditedPoints<'_> {
    type Item = f64;
    fn next(&mut self) -> Option<f64> {
        loop {
            let automatic = self.automatic.get(self.auto_index).copied().unwrap_or(self.end);
            let pinned = self.pins.get(self.pin_index).copied().unwrap_or(self.end);
            let point = automatic.min(pinned);
            if point >= self.end {
                return None;
            }
            if automatic <= pinned {
                self.auto_index += 1;
            }
            if pinned <= automatic {
                self.pin_index += 1;
            }
            if point > self.last {
                self.last = point;
                return Some(point);
            }
        }
    }
}

#[cfg(test)]
#[path = "slicing_tests.rs"]
mod tests;
