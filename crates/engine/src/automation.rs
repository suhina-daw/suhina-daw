// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation of built-in and `.sd` device parameters and of a track's mixer (#395,
//! `docs/spec/automation.md`). Plug-in parameters have their own events (`plugin_params`) but
//! share the [`Envelope`].
//!
//! Lanes are turned into arrangement frames when the graph is built. While the transport runs:
//!
//! - **A built-in or `.sd` device's lane** sets its parameter at the block's first frame, at every point
//!   inside the block and at every [`STEP`] frames of the arrangement while it moves, where the
//!   value changed. The chain splits the device's block there (ADR 0004, decision 5: the host
//!   splits, the device smooths).
//! - **A mixer lane** (volume, pan, mute) gives the track's gains at every [`MIX_STEP`] frames of
//!   the arrangement, and the gain moves in a straight line between them, frame by frame, so a
//!   fader move or a mute never clicks. A track with mixer lanes runs through a chain (one with
//!   no devices when it has none), whose gains apply after its devices.
//!
//! While the transport is stopped, device parameters keep their last value and mixer lanes give
//! their value at the play start marker. The grid is the arrangement's, not the block's, so the
//! sound does not depend on the block size. Nothing here allocates after the graph is built.

use suhina_device_api::{Device, ParamInfo, ParamKind};
use suhina_model::automation::{self as model, MAX_POINTS, MUTE, PAN, Taper, VOLUME, VolumeTaper, shape};
use suhina_model::{AutomationLane, Channel, ChannelRole, Project, TrackId};

/// Frames between the values a moving lane gives (1.3 ms at 48 kHz).
pub const STEP: usize = 64;
/// Frames between the gains a mixer lane gives (a third of a millisecond at 48 kHz), fine enough
/// for a smoothed jump to be an S rather than a few straight pieces.
pub const MIX_STEP: usize = 16;
/// How long a jump of a track's volume lane takes, in seconds: an S-shaped ramp centred on the
/// jump, in the fader's position (`docs/spec/automation.md` › How the engine plays them).
pub const VOLUME_SMOOTHING: f64 = 0.0044;
/// Most built-in and `.sd` device lanes one chain plays; more are left out.
const MAX_DEVICE_LANES: usize = 64;
/// Most parameter changes one block of a chain holds: each lane's at its first frame, its
/// [`STEP`]s in the longest block and up to [`MAX_LANE_POINTS`] points.
const MAX_LANE_POINTS: usize = 16;
const MAX_EVENTS: usize = MAX_DEVICE_LANES * (1 + crate::chain::MAX_BLOCK / STEP + MAX_LANE_POINTS);

/// Where a lane's values sit on the travel of their control, 0 to 1: a segment moves evenly
/// there, so what the lane draws straight is what plays (`docs/spec/automation.md` › Lanes and
/// points).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Position {
    /// The value itself: pan, plug-in parameters.
    Value,
    /// The volume fader, for a linear gain.
    Volume,
    /// A built-in device parameter, as its knob turns.
    Param(ParamInfo),
    /// A data-only device parameter on a logarithmic control: equal steps multiply the value.
    Log { min: f64, max: f64 },
}

impl Position {
    /// The position of a data-only device's parameter: logarithmic on a log control with a
    /// positive range, the value itself otherwise.
    pub(crate) fn sd(parameter: &suhina_device_sdk::Parameter) -> Self {
        use suhina_device_sdk::ParameterControl::{LogKnob, LogSlider};
        let (min, max) = (parameter.min, parameter.max);
        if matches!(parameter.control, LogKnob | LogSlider) && min > 0.0 && max > min {
            Self::Log { min, max }
        } else {
            Self::Value
        }
    }
}

impl Taper for Position {
    fn position(&self, value: f64) -> f64 {
        match self {
            Self::Value => value,
            Self::Volume => VolumeTaper.position(value),
            Self::Param(info) => info.to_normalized(value),
            Self::Log { min, max } => (value.clamp(*min, *max) / min).ln() / (max / min).ln(),
        }
    }
    fn value(&self, position: f64) -> f64 {
        match self {
            Self::Value => position,
            Self::Volume => VolumeTaper.value(position),
            // A whole-number parameter moves along the line and the device rounds.
            Self::Param(info) if matches!(info.kind, ParamKind::Integer) => {
                info.min + position.clamp(0.0, 1.0) * (info.max - info.min)
            }
            Self::Param(info) => info.from_normalized(position),
            Self::Log { min, max } => min * (max / min).powf(position.clamp(0.0, 1.0)),
        }
    }
}

/// A lane in arrangement frames.
pub(crate) struct Envelope {
    points: Vec<Point>,
    stepped: bool,
    taper: Position,
}

#[derive(Debug, Clone, Copy)]
struct Point {
    frame: u64,
    value: f64,
    /// Where `value` sits on its control (`Position`).
    position: f64,
    curve: f64,
}

impl Envelope {
    /// `lane` at `frames_per_beat`, moving evenly in `taper`'s position, or `None` when it is
    /// off or has no point with finite numbers. With `stepped`, each value holds until the next
    /// point (on/off and choices). Allocates.
    pub(crate) fn new(lane: &AutomationLane, frames_per_beat: f64, stepped: bool, taper: Position) -> Option<Self> {
        if !lane.enabled {
            return None;
        }
        let mut points: Vec<Point> = lane
            .points
            .iter()
            .filter(|p| p.beat.is_finite() && p.value.is_finite())
            .take(MAX_POINTS)
            .map(|p| Point {
                // A float cast saturates, so a beat far out lands at the end of time.
                frame: (p.beat.max(0.0) * frames_per_beat).round() as u64,
                value: p.value,
                position: taper.position(p.value),
                curve: if p.curve.is_finite() { p.curve.clamp(-1.0, 1.0) } else { 0.0 },
            })
            .collect();
        // Stable, so two points at one beat keep their order (a jump).
        points.sort_by_key(|p| p.frame);
        (!points.is_empty()).then_some(Self { points, stepped, taper })
    }

    /// Straight lines through `(frame, value)` points, for tests.
    #[cfg(test)]
    pub(crate) fn linear(points: &[(u64, f64)]) -> Option<Self> {
        let points: Vec<Point> =
            points.iter().map(|&(frame, value)| Point { frame, value, position: value, curve: 0.0 }).collect();
        (!points.is_empty()).then_some(Self { points, stepped: false, taper: Position::Value })
    }

    /// The value at arrangement frame `time`: the first point's before it, the last's after it,
    /// along the curve of the segment between, evenly in position; at a frame with two points,
    /// the later one's.
    pub(crate) fn value_at(&self, time: u64) -> f64 {
        let after = self.points.partition_point(|p| p.frame <= time);
        let before = after.checked_sub(1).and_then(|i| self.points.get(i));
        match (before, self.points.get(after)) {
            (None, next) => next.map_or(0.0, |p| p.value),
            (Some(a), None) => a.value,
            (Some(a), Some(_)) if self.stepped => a.value,
            (Some(a), Some(b)) if a.value == b.value => a.value,
            (Some(a), Some(b)) => {
                // b.frame > time >= a.frame.
                let t = (time - a.frame) as f64 / (b.frame - a.frame) as f64;
                self.taper.value(a.position + (b.position - a.position) * shape(t, a.curve))
            }
        }
    }

    /// The position (`Position`) at arrangement frame `time`, as [`Self::value_at`] moves.
    fn position_at(&self, time: u64) -> f64 {
        let after = self.points.partition_point(|p| p.frame <= time);
        let before = after.checked_sub(1).and_then(|i| self.points.get(i));
        match (before, self.points.get(after)) {
            (None, next) => next.map_or(0.0, |p| p.position),
            (Some(a), None) => a.position,
            (Some(a), Some(_)) if self.stepped => a.position,
            (Some(a), Some(b)) => {
                let t = (time - a.frame) as f64 / (b.frame - a.frame) as f64;
                a.position + (b.position - a.position) * shape(t, a.curve)
            }
        }
    }

    /// The value at `time` with its jumps smoothed: each jump within half of `length` frames of
    /// `time` becomes an S-shaped ramp (smootherstep) in position, `length` frames long and
    /// centred on the jump; everything else plays as drawn. Never allocates.
    pub(crate) fn smoothed_at(&self, time: u64, length: u64) -> f64 {
        if length < 2 || self.stepped {
            return self.value_at(time);
        }
        let half = length / 2;
        let from = self.points.partition_point(|p| p.frame < time.saturating_sub(half));
        let to = self.points.partition_point(|p| p.frame <= time.saturating_add(half));
        let mut position = self.position_at(time);
        let mut moved = false;
        for i in from..to {
            let (Some(a), Some(b)) = (self.points.get(i), self.points.get(i + 1)) else { break };
            if a.frame != b.frame || a.position == b.position {
                continue;
            }
            // How far through the ramp `time` is, 0 to 1, and how far the jump has gone there.
            let start = a.frame as f64 - length as f64 / 2.0;
            let u = ((time as f64 - start) / length as f64).clamp(0.0, 1.0);
            let ramp = u * u * u * (u * (6.0 * u - 15.0) + 10.0);
            let done = if time >= a.frame { 1.0 } else { 0.0 };
            position += (b.position - a.position) * (ramp - done);
            moved = true;
        }
        if moved { self.taper.value(position) } else { self.value_at(time) }
    }

    /// The index of the first point after arrangement frame `time`.
    pub(crate) fn first_after(&self, time: u64) -> usize {
        self.points.partition_point(|p| p.frame <= time)
    }

    /// The frame of point `index`.
    pub(crate) fn frame(&self, index: usize) -> Option<u64> {
        self.points.get(index).map(|p| p.frame)
    }
}

/// Calls `emit` with the frames (from the block's start) and values of an envelope in a block
/// of `frames` frames from arrangement frame `time`: each point inside the block and every
/// [`STEP`] frames of the arrangement, at most `most` in all, and the first frame when the
/// block does not follow on from the last (`jumped`: the transport started or moved), so the
/// sound does not depend on where blocks start. `last` is the value sent before; only changes
/// are sent.
fn changes(
    envelope: &Envelope,
    time: u64,
    frames: usize,
    most: usize,
    jumped: bool,
    last: &mut f64,
    mut emit: impl FnMut(usize, f64),
) {
    let mut sent = 0;
    let mut send = |frame: usize, value: f64| {
        if value != *last {
            emit(frame, value);
            *last = value;
            return 1;
        }
        0
    };
    let step = STEP as u64;
    let on_point = time.checked_sub(1).is_some_and(|before| envelope.frame(envelope.first_after(before)) == Some(time));
    if jumped || time.is_multiple_of(step) || on_point {
        sent += send(0, envelope.value_at(time));
    }
    // The next frame of the arrangement's grid after `time`, from the block's start.
    let mut grid = usize::try_from(step - time % step).unwrap_or(STEP);
    let mut next_point = envelope.first_after(time);
    while sent < most {
        let point = envelope
            .frame(next_point)
            .map_or(usize::MAX, |f| usize::try_from(f.saturating_sub(time)).unwrap_or(usize::MAX));
        let frame = grid.min(point);
        if frame >= frames {
            break;
        }
        if grid == frame {
            grid += STEP;
        }
        while envelope.frame(next_point).is_some_and(|f| f.saturating_sub(time) <= frame as u64) {
            next_point += 1;
        }
        sent += send(frame, envelope.value_at(time.saturating_add(frame as u64)));
    }
}

/// A device parameter change at a frame of the block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DeviceChange {
    pub(crate) frame: usize,
    /// The device's index among the chain's slots, and the parameter's index.
    pub(crate) slot: usize,
    pub(crate) param: usize,
    pub(crate) value: f64,
}

struct DeviceLane {
    slot: usize,
    param: usize,
    envelope: Envelope,
    last: f64,
}

/// The lanes of a chain's built-in and `.sd` devices and the changes they make in the current block.
pub(crate) struct DeviceLanes {
    lanes: Vec<DeviceLane>,
    /// This block's changes, in frame order.
    block: Vec<DeviceChange>,
    /// The arrangement frame the last block planned while playing ended at.
    next: Option<u64>,
}

impl DeviceLanes {
    pub(crate) fn none() -> Self {
        Self { lanes: Vec::new(), block: Vec::new(), next: None }
    }

    /// Adds the lanes of the device in slot `slot`, matched to its parameters by id (a lane of
    /// an unknown parameter is left out). Allocates.
    pub(crate) fn add(&mut self, slot: usize, device: &dyn Device, lanes: &[AutomationLane], frames_per_beat: f64) {
        let info = device.info();
        for lane in lanes {
            let Some(param) = info.param_index(&lane.param) else { continue };
            if self.lanes.len() >= MAX_DEVICE_LANES || self.lanes.iter().any(|l| l.slot == slot && l.param == param) {
                continue;
            }
            let stepped =
                info.params.get(param).is_some_and(|p| matches!(p.kind, ParamKind::Toggle | ParamKind::Choice(_)));
            let Some(info) = info.params.get(param) else { continue };
            if let Some(envelope) = Envelope::new(lane, frames_per_beat, stepped, Position::Param(*info)) {
                self.lanes.push(DeviceLane { slot, param, envelope, last: f64::NAN });
            }
        }
        if self.block.capacity() < MAX_EVENTS && !self.lanes.is_empty() {
            self.block.reserve_exact(MAX_EVENTS);
        }
    }

    /// Adds a data-only device's lanes, matched to its parameters by id. Allocates.
    pub(crate) fn add_sd(
        &mut self,
        slot: usize,
        parameters: &[suhina_device_sdk::Parameter],
        lanes: &[AutomationLane],
        frames_per_beat: f64,
    ) {
        for lane in lanes {
            let Some(param) = parameters.iter().position(|p| p.id == lane.param) else { continue };
            if self.lanes.len() >= MAX_DEVICE_LANES || self.lanes.iter().any(|l| l.slot == slot && l.param == param) {
                continue;
            }
            let Some(parameter) = parameters.get(param) else { continue };
            let stepped = parameter.control.is_stepped();
            if let Some(envelope) = Envelope::new(lane, frames_per_beat, stepped, Position::sd(parameter)) {
                self.lanes.push(DeviceLane { slot, param, envelope, last: f64::NAN });
            }
        }
        if self.block.capacity() < MAX_EVENTS && !self.lanes.is_empty() {
            self.block.reserve_exact(MAX_EVENTS);
        }
    }

    /// Makes room for a slot at `slot`: the lanes of that slot and the ones after it move up
    /// one.
    pub(crate) fn insert_slot(&mut self, slot: usize) {
        for lane in self.lanes.iter_mut().filter(|l| l.slot >= slot) {
            lane.slot += 1;
        }
    }

    /// Whether parameter `param` of slot `slot` follows a lane.
    pub(crate) fn covers(&self, slot: usize, param: usize) -> bool {
        self.lanes.iter().any(|l| l.slot == slot && l.param == param)
    }

    /// The value of parameter `param` of slot `slot` at arrangement frame 0, when it follows a
    /// lane: where a fresh device starts.
    pub(crate) fn initial(&self, slot: usize, param: usize) -> Option<f64> {
        self.lanes.iter().find(|l| l.slot == slot && l.param == param).map(|l| l.envelope.value_at(0))
    }

    /// Plans the changes of a block of `frames` frames from arrangement frame `time` (`None`
    /// while the transport is stopped: none). Never allocates.
    pub(crate) fn plan(&mut self, time: Option<u64>, frames: usize) {
        let Self { lanes, block, next } = self;
        block.clear();
        let Some(time) = time else {
            *next = None;
            return;
        };
        let jumped = *next != Some(time);
        *next = Some(time.saturating_add(frames as u64));
        let room = block.capacity();
        for lane in lanes.iter_mut() {
            let DeviceLane { slot, param, envelope, last } = lane;
            let most = 1 + frames / STEP + MAX_LANE_POINTS;
            changes(envelope, time, frames, most, jumped, last, |frame, value| {
                if block.len() < room {
                    block.push(DeviceChange { frame, slot: *slot, param: *param, value });
                }
            });
        }
        // In frame order, keeping the order at one frame. An insertion sort: no allocation.
        for i in 1..block.len() {
            let mut j = i;
            while j > 0 && block.get(j - 1).zip(block.get(j)).is_some_and(|(a, b)| a.frame > b.frame) {
                block.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// The changes of slot `slot` in the block's frames `from..to`, in frame order.
    pub(crate) fn changes(&self, slot: usize, from: usize, to: usize) -> impl Iterator<Item = &DeviceChange> {
        self.block.iter().filter(move |c| c.slot == slot && c.frame >= from && c.frame < to)
    }
}

/// The gains of a track whose mixer follows lanes: the gains the rest of its way applies (its
/// own channel's automated parameters at their neutral values: volume 1, centred, not muted),
/// scaled frame by frame by the lanes.
pub(crate) struct MixLanes {
    volume: Option<Envelope>,
    /// How many frames a jump of the volume is smoothed over ([`VOLUME_SMOOTHING`]).
    smoothing: u64,
    pan: Option<Envelope>,
    mute: Option<Envelope>,
    /// Whether the pan applies (not on a group's bus, as for a group without lanes).
    pan_applies: bool,
    /// How far the chain's output is delayed for latency compensation: the lanes are read that
    /// much earlier, so they line up with what is heard.
    latency: u64,
    /// This block's factors, interleaved left and right per frame.
    gains: Vec<f32>,
}

impl MixLanes {
    /// The lanes of `channel` at `frames_per_beat` and `sample_rate`; `None` when none plays.
    /// Allocates.
    pub(crate) fn new(channel: &Channel, frames_per_beat: f64, pan_applies: bool, sample_rate: f64) -> Option<Self> {
        let lane = |param: &str, stepped: bool, taper: Position| {
            channel.lane(param).and_then(|lane| Envelope::new(lane, frames_per_beat, stepped, taper))
        };
        let volume = lane(VOLUME, false, Position::Volume);
        let (pan, mute) = (lane(PAN, false, Position::Value), lane(MUTE, true, Position::Value));
        if volume.is_none() && pan.is_none() && mute.is_none() {
            return None;
        }
        let smoothing = (VOLUME_SMOOTHING * sample_rate).round();
        let smoothing = if smoothing.is_finite() && smoothing >= 0.0 { smoothing as u64 } else { 0 };
        Some(Self {
            volume,
            smoothing,
            pan,
            mute,
            pan_applies,
            latency: 0,
            gains: vec![1.0; crate::chain::MAX_BLOCK * 2],
        })
    }

    pub(crate) fn set_latency(&mut self, frames: u32) {
        self.latency = u64::from(frames);
    }

    /// The left and right factors at arrangement frame `time`.
    fn factors(&self, time: u64) -> (f32, f32) {
        let value = |envelope: &Option<Envelope>| envelope.as_ref().map(|e| e.value_at(time));
        if value(&self.mute).is_some_and(|m| m >= 0.5) {
            return (0.0, 0.0);
        }
        let volume =
            self.volume.as_ref().map_or(1.0, |e| e.smoothed_at(time, self.smoothing).clamp(0.0, model::MAX_VOLUME));
        let (left, right) = match value(&self.pan).filter(|_| self.pan_applies) {
            // Equal power, relative to the centre the rest of the gains were taken at.
            Some(pan) => {
                let (left, right) = suhina_dsp::pan::gains(pan);
                (left * std::f64::consts::SQRT_2, right * std::f64::consts::SQRT_2)
            }
            None => (1.0, 1.0),
        };
        ((volume * left) as f32, (volume * right) as f32)
    }

    /// Fills the factors of a block of `frames` frames from arrangement frame `time`: at every
    /// [`MIX_STEP`] frames of the arrangement the lanes' value, in a straight line between, so a
    /// frame's factor depends on its time only. While the transport is stopped (`running`
    /// false), every frame gets the factor at `time`. Never allocates.
    pub(crate) fn plan(&mut self, time: u64, frames: usize, running: bool) {
        let time = time.saturating_sub(self.latency);
        let frames = frames.min(self.gains.len() / 2);
        let step = MIX_STEP as u64;
        let mut frame = 0usize;
        while frame < frames {
            let at = if running { time.saturating_add(frame as u64) } else { time };
            let grid = at - at % step;
            let (a, b) = (self.factors(grid), self.factors(grid.saturating_add(step)));
            let until =
                if running { frame.saturating_add(MIX_STEP - (at % step) as usize).min(frames) } else { frames };
            for f in frame..until {
                let t = if running { time.saturating_add(f as u64) } else { time };
                let x = t.saturating_sub(grid) as f32 / MIX_STEP as f32;
                if let Some([l, r]) =
                    self.gains.get_mut(f * 2..f * 2 + 2).and_then(|s| <&mut [f32; 2]>::try_from(s).ok())
                {
                    *l = a.0 + (b.0 - a.0) * x;
                    *r = a.1 + (b.1 - a.1) * x;
                }
            }
            frame = until.max(frame + 1);
        }
    }

    /// The factors of frame `frame` of the block.
    pub(crate) fn gains(&self, frame: usize) -> (f32, f32) {
        match self.gains.get(frame * 2..frame * 2 + 2) {
            Some(&[l, r]) => (l, r),
            _ => (0.0, 0.0),
        }
    }
}

/// Whether `track`'s own channel has a mixer lane that plays.
pub(crate) fn mixer_automated(project: &Project, track: TrackId) -> bool {
    channel_of(project, track).is_some_and(|c| c.automation.iter().any(|l| l.enabled && !l.points.is_empty()))
}

pub(crate) fn channel_of(project: &Project, track: TrackId) -> Option<&Channel> {
    let id = project.track(track)?.channel?;
    project.channel(id)
}

/// `project` with every mixer parameter that follows a lane at its neutral value (volume 1,
/// centred, not muted), so the graph is built with the lanes' tracks heard and their gains
/// applied after their chains; `None` when no lane plays (the project as it is). Only the
/// tracks' own channels: the master's lanes do not play yet. Allocates.
pub(crate) fn neutral(project: &Project) -> Option<Project> {
    let automated = |c: &Channel| c.role != ChannelRole::Master && model::MIXER_PARAMS.iter().any(|p| c.automated(p));
    if !project.channels.iter().any(automated) {
        return None;
    }
    let mut out = project.clone();
    for channel in out.channels.iter_mut().filter(|c| automated(c)) {
        if channel.automated(VOLUME) {
            channel.volume = 1.0;
        }
        if channel.automated(PAN) {
            channel.pan = 0.0;
        }
        if channel.automated(MUTE) {
            channel.mute = false;
        }
    }
    Some(out)
}

#[cfg(test)]
#[path = "automation_tests.rs"]
mod tests;
