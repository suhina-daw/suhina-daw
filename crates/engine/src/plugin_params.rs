// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in parameters from the engine (#109): the values a project sets on a plug-in device,
//! values set while it plays (`device.setParam`), and its automation lanes, turned into
//! parameter events at their frames in the block a plug-in slot hands over (ADR 0006,
//! decision 3), so automation is sample-accurate where the format allows.
//!
//! Each block a slot plans its events once ([`SlotParams::plan`]), before any of the block
//! runs, and hands each span it runs the ones that fall in it ([`SlotParams::span`]). Nothing
//! here allocates after the graph is built.
//!
//! - **Set values** (the project's `Device::params`, by the plug-in's parameter id) go at the
//!   start of the first blocks a processor runs, [`BASE_PER_BLOCK`] a block, and again after a
//!   processor is replaced (a restart), over the state the plug-in loaded.
//! - **A value set while playing** goes at the start of the next block.
//! - **A lane** sends its value at the start of every block while the transport runs (so a
//!   jump or a loop lands at once), then, inside the block, a value at every point and every
//!   [`RAMP_STEP`] frames while it moves. A VST3 plug-in ramps between the values itself; a
//!   CLAP plug-in gets them as steps. While the transport is stopped a lane sends nothing and
//!   the plug-in keeps the last value.

use suhina_model::{Device, DeviceParam};

use crate::automation::{Envelope, Position};

/// A parameter value for a plug-in at a frame of the block it goes with, in the plug-in's own
/// range (a VST3 plug-in's normalized, a CLAP plug-in's plain): automation, or a value set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamEvent {
    pub frame: usize,
    pub id: u32,
    pub value: f64,
}

/// Most parameter events one block of a slot carries; the latest ones beyond are dropped.
pub const MAX_BLOCK_PARAMS: usize = 512;
/// Frames between the values a moving lane sends inside a block.
pub const RAMP_STEP: usize = 64;
/// Set values sent in one block when a processor starts; the rest go in the blocks after.
pub(crate) const BASE_PER_BLOCK: usize = 128;
/// Most values one lane sends in a block (a VST3 queue holds 16 points; more lose the middle).
const MAX_LANE_EVENTS: usize = 32;
/// Values set while playing that wait for the next block; more are dropped.
const MAX_QUEUED: usize = 256;
/// Most lanes and set values a slot keeps (a project's numbers are not trusted).
const MAX_LANES: usize = 256;
const MAX_BASE: usize = 4096;
/// One automation lane, in arrangement frames.
struct Lane {
    id: u32,
    envelope: Envelope,
}

/// The parameter values a plug-in slot sends: what the project sets, what is set while it
/// plays, and its automation lanes.
pub(crate) struct SlotParams {
    /// The project's values, by parameter id.
    base: Vec<(u32, f64)>,
    /// How many of `base` have gone to the running processor.
    base_sent: usize,
    lanes: Vec<Lane>,
    queued: Vec<(u32, f64)>,
    /// This block's events, in frame order (frames from the block's start).
    block: Vec<ParamEvent>,
    /// The events of the span running now, frames from the span's start.
    span: Vec<ParamEvent>,
}

/// The plug-in parameter id a project names, `None` when it is not one.
pub fn param_id(id: &str) -> Option<u32> {
    id.trim().parse().ok()
}

impl SlotParams {
    /// The values and lanes of `device`, its lanes in frames at `frames_per_beat`. Allocates:
    /// call it off the audio thread.
    pub(crate) fn new(device: &Device, frames_per_beat: f64) -> Self {
        let mut base: Vec<(u32, f64)> = Vec::new();
        for DeviceParam { id, value } in &device.params {
            let (Some(id), true) = (param_id(id), value.is_finite()) else { continue };
            if let Some(entry) = base.iter_mut().find(|(i, _)| *i == id) {
                entry.1 = *value;
            } else if base.len() < MAX_BASE {
                base.push((id, *value));
            }
        }
        let mut lanes: Vec<Lane> = Vec::new();
        for lane in &device.automation {
            let Some(id) = param_id(&lane.param) else { continue };
            if lanes.len() >= MAX_LANES || lanes.iter().any(|l| l.id == id) {
                continue;
            }
            // A lane that is off (#395) or has no points is left out.
            if let Some(envelope) = Envelope::new(lane, frames_per_beat, false, Position::Value) {
                lanes.push(Lane { id, envelope });
            }
        }
        // Room for every value set while playing, so setting one never allocates.
        base.reserve(MAX_QUEUED);
        Self {
            base,
            base_sent: 0,
            lanes,
            queued: Vec::with_capacity(MAX_QUEUED),
            block: Vec::with_capacity(MAX_BLOCK_PARAMS),
            span: Vec::with_capacity(MAX_BLOCK_PARAMS),
        }
    }

    /// A new processor runs the slot: it gets the set values again. Never allocates.
    pub(crate) fn restart(&mut self) {
        self.base_sent = 0;
    }

    /// Sets parameter `id` to `value` from the next block, and keeps it for a restart. Never
    /// allocates.
    pub(crate) fn set(&mut self, id: u32, value: f64) {
        if !value.is_finite() {
            return;
        }
        // A value new to the list goes out with the queued ones, not again as a set value.
        let all_sent = self.base_sent >= self.base.len();
        set_in(&mut self.base, id, value);
        if all_sent {
            self.base_sent = self.base.len();
        }
        if self.queued.len() < self.queued.capacity().min(MAX_QUEUED) || self.queued.iter().any(|(i, _)| *i == id) {
            set_in(&mut self.queued, id, value);
        }
    }

    /// Takes the values and lanes of `fresh` (the same device in a graph built after an edit)
    /// while keeping what this one sent: values that changed, and the set values of
    /// parameters whose lane is gone, go at the start of the next block. Never allocates.
    pub(crate) fn adopt(&mut self, fresh: &mut SlotParams) {
        let all_sent = self.base_sent >= self.base.len();
        for &(id, value) in &fresh.base {
            let same = self.base.iter().any(|&(i, v)| i == id && v == value);
            if !same && self.queued.len() < self.queued.capacity().min(MAX_QUEUED) {
                set_in(&mut self.queued, id, value);
            }
        }
        // A parameter whose lane is gone goes back to its set value.
        for lane in &self.lanes {
            let kept = fresh.lanes.iter().any(|l| l.id == lane.id);
            let set = fresh.base.iter().find(|(id, _)| *id == lane.id);
            if let (false, Some(&(id, value))) = (kept, set)
                && self.queued.len() < self.queued.capacity().min(MAX_QUEUED)
            {
                set_in(&mut self.queued, id, value);
            }
        }
        std::mem::swap(&mut self.base, &mut fresh.base);
        std::mem::swap(&mut self.lanes, &mut fresh.lanes);
        self.base_sent = if all_sent { self.base.len() } else { self.base_sent.min(self.base.len()) };
    }

    /// Plans the events of a block of `frames` frames that starts at arrangement frame `time`
    /// (`None` while the transport is stopped). Never allocates.
    pub(crate) fn plan(&mut self, time: Option<u64>, frames: usize) {
        let Self { base, base_sent, lanes, queued, block, .. } = self;
        block.clear();
        let room = block.capacity().min(MAX_BLOCK_PARAMS);
        let mut push = |event: ParamEvent| {
            if block.len() < room {
                block.push(event);
            }
        };
        let end = base.len().min(base_sent.saturating_add(BASE_PER_BLOCK));
        for &(id, value) in base.get(*base_sent..end).unwrap_or_default() {
            push(ParamEvent { frame: 0, id, value });
        }
        *base_sent = end.max(*base_sent);
        for (id, value) in queued.drain(..) {
            push(ParamEvent { frame: 0, id, value });
        }
        if let Some(time) = time {
            for lane in lanes.iter() {
                lane_events(&lane.envelope, time, frames, |frame, value| {
                    push(ParamEvent { frame, id: lane.id, value })
                });
            }
        }
        // In frame order, keeping the order of events at one frame (a lane after a set value).
        // An insertion sort: no allocation, and nearly sorted already.
        for i in 1..block.len() {
            let mut j = i;
            while j > 0 && block.get(j - 1).zip(block.get(j)).is_some_and(|(a, b)| a.frame > b.frame) {
                block.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// The events of the block that fall in its frames `from..to`, with frames counted from
    /// `from`. Never allocates.
    pub(crate) fn span(&mut self, from: usize, to: usize) -> &[ParamEvent] {
        self.span.clear();
        for event in self.block.iter().filter(|e| e.frame >= from && e.frame < to) {
            if self.span.len() < self.span.capacity() {
                self.span.push(ParamEvent { frame: event.frame - from, ..*event });
            }
        }
        &self.span
    }

    #[cfg(test)]
    pub(crate) fn block(&self) -> &[ParamEvent] {
        &self.block
    }
}

/// Sets `id` to `value` in `list`, within its capacity.
fn set_in(list: &mut Vec<(u32, f64)>, id: u32, value: f64) {
    if let Some(entry) = list.iter_mut().find(|(i, _)| *i == id) {
        entry.1 = value;
    } else if list.len() < list.capacity() {
        list.push((id, value));
    }
}

/// A lane's value at arrangement frame `time`: a straight line between the points around it,
/// the first point's before it and the last's after.
#[cfg(test)]
pub(crate) fn value_at(points: &[(u64, f64)], time: u64) -> Option<f64> {
    Envelope::linear(points).map(|e| e.value_at(time))
}

/// The values a lane sends in a block of `frames` frames from arrangement frame `time`: its
/// value at the first frame, then at each point inside the block and every [`RAMP_STEP`]
/// frames, where it changed.
fn lane_events(envelope: &Envelope, time: u64, frames: usize, mut emit: impl FnMut(usize, f64)) {
    let first = envelope.value_at(time);
    emit(0, first);
    let mut last = first;
    let mut sent = 1;
    let mut grid = RAMP_STEP;
    let mut next_point = envelope.first_after(time);
    while sent < MAX_LANE_EVENTS {
        let point = envelope.frame(next_point).map_or(usize::MAX, |f| usize::try_from(f - time).unwrap_or(usize::MAX));
        let frame = grid.min(point);
        if frame >= frames {
            break;
        }
        if grid == frame {
            grid += RAMP_STEP;
        }
        while envelope.frame(next_point).is_some_and(|f| f - time <= frame as u64) {
            next_point += 1;
        }
        let value = envelope.value_at(time.saturating_add(frame as u64));
        if value != last {
            emit(frame, value);
            last = value;
            sent += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{AutomationLane, AutomationPoint, DeviceKind};

    fn device(params: &[(&str, f64)], lanes: &[(&str, &[(f64, f64)])]) -> Device {
        Device {
            params: params.iter().map(|(id, v)| DeviceParam::new(*id, *v)).collect(),
            automation: lanes
                .iter()
                .map(|(param, points)| AutomationLane {
                    param: (*param).into(),
                    points: points.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect(),
                    enabled: true,
                })
                .collect(),
            ..Device::new("P", DeviceKind::Clap { id: "x".into() })
        }
    }

    #[test]
    fn a_lane_is_a_line_between_its_points() {
        let points = [(100, 0.0), (200, 1.0), (200, 0.25), (300, 0.25)];
        assert_eq!(value_at(&points, 0), Some(0.0));
        assert_eq!(value_at(&points, 150), Some(0.5));
        // Two points at one frame: a jump, the later wins from there on.
        assert_eq!(value_at(&points, 199), Some(0.99));
        assert_eq!(value_at(&points, 200), Some(0.25));
        assert_eq!(value_at(&points, 1_000), Some(0.25));
        assert_eq!(value_at(&[], 5), None);
    }

    #[test]
    fn set_values_go_first_then_the_lanes_at_their_frames() {
        // 1 beat = 100 frames.
        let mut params =
            SlotParams::new(&device(&[("7", 0.5), ("x", 1.0)], &[("3", &[(1.0, 0.0), (2.0, 1.0)])]), 100.0);
        params.plan(None, 256);
        // Stopped: the set value only (the id that is not a number is left out).
        assert_eq!(params.block(), [ParamEvent { frame: 0, id: 7, value: 0.5 }]);
        params.plan(None, 256);
        assert!(params.block().is_empty(), "sent once");
        // Playing from frame 0: flat until frame 100, then a ramp to frame 200, sent every
        // RAMP_STEP frames and at the points.
        params.plan(Some(0), 256);
        let frames: Vec<usize> = params.block().iter().map(|e| e.frame).collect();
        assert_eq!(frames, [0, 128, 192, 200]);
        let values: Vec<f64> = params.block().iter().map(|e| e.value).collect();
        assert_eq!(values, [0.0, 0.28, 0.92, 1.0]);
        // A span of the block gets its own events, from its start.
        assert_eq!(
            params.span(150, 256),
            [ParamEvent { frame: 42, id: 3, value: 0.92 }, ParamEvent { frame: 50, id: 3, value: 1.0 }]
        );
        // A restart sends the set values again; a value set while playing goes next block.
        params.restart();
        params.set(9, 0.75);
        params.plan(None, 64);
        assert_eq!(
            params.block(),
            [
                ParamEvent { frame: 0, id: 7, value: 0.5 },
                ParamEvent { frame: 0, id: 9, value: 0.75 },
                ParamEvent { frame: 0, id: 9, value: 0.75 }
            ]
        );
    }

    #[test]
    fn an_edit_sends_only_what_changed() {
        let mut running = SlotParams::new(&device(&[("1", 0.5), ("2", 0.5)], &[]), 100.0);
        running.plan(None, 64);
        let mut fresh = SlotParams::new(&device(&[("1", 0.5), ("2", 0.9)], &[("1", &[(0.0, 0.1)])]), 100.0);
        running.adopt(&mut fresh);
        running.plan(Some(0), 64);
        assert_eq!(
            running.block(),
            [ParamEvent { frame: 0, id: 2, value: 0.9 }, ParamEvent { frame: 0, id: 1, value: 0.1 }]
        );
    }

    #[test]
    fn hostile_lanes_stay_within_bounds() {
        let many: Vec<(f64, f64)> = (0..5_000).map(|i| (f64::from(i) * 0.001, f64::from(i % 2))).collect();
        let wild: &[(f64, f64)] = &[(f64::NAN, 1.0), (-5.0, f64::INFINITY), (1e300, 0.5)];
        let lanes: Vec<(String, &[(f64, f64)])> =
            (0..300).map(|i| (i.to_string(), if i == 0 { wild } else { many.as_slice() })).collect();
        let lanes: Vec<(&str, &[(f64, f64)])> = lanes.iter().map(|(p, l)| (p.as_str(), *l)).collect();
        let mut params = SlotParams::new(&device(&[], &lanes), 22_050.0);
        for time in [0, 1, u64::MAX / 2, u64::MAX - 10] {
            params.plan(Some(time), 1024);
            assert!(params.block().len() <= MAX_BLOCK_PARAMS);
            assert!(params.block().windows(2).all(|w| w[0].frame <= w[1].frame));
            assert!(params.block().iter().all(|e| e.frame < 1024 && e.value.is_finite()));
        }
    }
}
