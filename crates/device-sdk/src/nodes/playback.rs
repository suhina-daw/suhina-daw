// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-neutral position and control kernels; metadata is shared by all authors.
use crate::native_node::NativeNode;
use crate::{NodeInfo, NodeKind, PortInfo, ProcessContext};
use suhina_device_api::NoteEvent;
pub(crate) const PLAYBACK_POSITION: NodeInfo = NodeInfo {
    kind: "playback_position",
    name: "PlaybackPosition",
    rust_variant: "PlaybackPosition",
    inputs: &[
        PortInfo { name: "rate", default: Some(1.0), unit: "signal" },
        PortInfo { name: "start", default: Some(0.0), unit: "signal" },
        PortInfo { name: "reset", default: Some(0.0), unit: "signal" },
        PortInfo { name: "gate", default: Some(1.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Signed frame position with rising-edge retrigger and gate.",
    configuration: "none",
    example: r#"{"kind":"playback_position"}"#,
};
pub(crate) const PLAYBACK_BOUNDS: NodeInfo = NodeInfo {
    kind: "playback_bounds",
    name: "PlaybackBounds",
    rust_variant: "PlaybackBounds",
    inputs: &[
        PortInfo { name: "position", default: Some(0.0), unit: "signal" },
        PortInfo { name: "start", default: Some(0.0), unit: "signal" },
        PortInfo { name: "end", default: Some(1.0), unit: "signal" },
        PortInfo { name: "gate", default: Some(1.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Half-open position bounds; -1 means inactive.",
    configuration: "none",
    example: r#"{"kind":"playback_bounds"}"#,
};
pub(crate) const LOOP_POSITION: NodeInfo = NodeInfo {
    kind: "loop_position",
    name: "LoopPosition",
    rust_variant: "LoopPosition",
    inputs: &[
        PortInfo { name: "position", default: Some(0.0), unit: "signal" },
        PortInfo { name: "start", default: Some(0.0), unit: "signal" },
        PortInfo { name: "end", default: Some(1.0), unit: "signal" },
        PortInfo { name: "ping_pong", default: Some(0.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Forward or ping-pong position wrapping for signed positions.",
    configuration: "none",
    example: r#"{"kind":"loop_position"}"#,
};
pub(crate) const PLAYBACK_RATE: NodeInfo = NodeInfo {
    kind: "playback_rate",
    name: "PlaybackRate",
    rust_variant: "PlaybackRate",
    inputs: &[
        PortInfo { name: "key", default: Some(60.0), unit: "signal" },
        PortInfo { name: "root", default: Some(60.0), unit: "signal" },
        PortInfo { name: "fine", default: Some(0.0), unit: "signal" },
        PortInfo { name: "bend", default: Some(0.0), unit: "signal" },
        PortInfo { name: "ratio", default: Some(1.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Pitch and sample-rate ratio to signed frame advancement.",
    configuration: "none",
    example: r#"{"kind":"playback_rate"}"#,
};
pub(crate) const REGION_FADE: NodeInfo = NodeInfo {
    kind: "region_fade",
    name: "RegionFade",
    rust_variant: "RegionFade",
    inputs: &[
        PortInfo { name: "position", default: Some(0.0), unit: "signal" },
        PortInfo { name: "length", default: Some(1.0), unit: "signal" },
        PortInfo { name: "fade", default: Some(0.0), unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Linear source-region edge gain.",
    configuration: "none",
    example: r#"{"kind":"region_fade"}"#,
};

struct Kernel {
    kind: NodeKind,
    position: [f64; 2],
    trigger: [bool; 2],
    note: bool,
    finished: [[f32; 2]; 64],
}
pub(crate) fn prepare(kind: &NodeKind) -> Option<Box<dyn NativeNode>> {
    match kind {
        NodeKind::ScanPosition
        | NodeKind::PositionReset
        | NodeKind::PlaybackFinished
        | NodeKind::PlaybackPosition
        | NodeKind::PlaybackBounds
        | NodeKind::LoopPosition
        | NodeKind::PlaybackRate
        | NodeKind::RegionFade => Some(Box::new(Kernel {
            kind: kind.clone(),
            position: [0.; 2],
            trigger: [false; 2],
            note: false,
            finished: [[0.; 2]; 64],
        })),
        _ => None,
    }
}
impl NativeNode for Kernel {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            for (channel, sample) in pair.iter_mut().enumerate() {
                let x: [f64; 5] = std::array::from_fn(|port| {
                    let value = inputs[port].get(frame * 2 + channel).copied().unwrap_or(0.);
                    if value.is_finite() { f64::from(value) } else { 0. }
                });
                let value = match self.kind {
                    NodeKind::PlaybackPosition | NodeKind::ScanPosition => {
                        let trigger = x[2] > 0.;
                        if self.note || (trigger && !self.trigger[channel]) {
                            self.position[channel] = x[1];
                        }
                        self.trigger[channel] = trigger;
                        let position = self.position[channel];
                        if x[3] > 0. {
                            self.position[channel] += x[0];
                        }
                        if !self.position[channel].is_finite() {
                            self.position[channel] = 0.;
                        }
                        position
                    }
                    NodeKind::PositionReset => {
                        let trigger = x[1] > 0.;
                        let value = if self.note || (trigger && !self.trigger[channel]) { x[2] } else { x[0] };
                        self.trigger[channel] = trigger;
                        value
                    }
                    NodeKind::PlaybackFinished => {
                        let active = x[3] > 0. && x[0] >= x[1] && x[0] < x[2];
                        let pulse = self.trigger[channel] && !active;
                        self.trigger[channel] = active;
                        self.finished[frame][channel] = f32::from(!active);
                        f64::from(pulse)
                    }
                    NodeKind::PlaybackBounds => {
                        if x[3] > 0. && x[0] >= x[1] && x[0] < x[2] {
                            x[0]
                        } else {
                            -1.
                        }
                    }
                    NodeKind::LoopPosition => {
                        let length = x[2] - x[1];
                        if length <= 0. {
                            x[1]
                        } else if x[3] > 0. {
                            let span = (length - 1.).max(0.);
                            if span == 0. {
                                x[1]
                            } else {
                                let phase = (x[0] - x[1]).rem_euclid(span * 2.);
                                x[1] + if phase < span { phase } else { span * 2. - phase }
                            }
                        } else {
                            x[1] + (x[0] - x[1]).rem_euclid(length)
                        }
                    }
                    NodeKind::PlaybackRate => {
                        2f64.powf(((x[0] - x[1] + x[2] / 100. + x[3]) / 12.).clamp(-20., 20.)) * x[4]
                    }
                    NodeKind::RegionFade => {
                        if x[1] <= 0. || x[0] < 0. || x[0] >= x[1] {
                            0.
                        } else {
                            let fade = x[2].clamp(0., x[1] / 2.);
                            if fade == 0. { 1. } else { (x[0] / fade).min((x[1] - x[0]) / fade).clamp(0., 1.) }
                        }
                    }
                    _ => 0.,
                };
                *sample =
                    if value.is_finite() { value.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32 } else { 0. };
            }
            self.note = false;
        }
    }
    fn output(&self, _: usize, frame: usize) -> [f32; 2] {
        self.finished.get(frame).copied().unwrap_or([0.; 2])
    }
    fn event(&mut self, event: NoteEvent) {
        if matches!(event, NoteEvent::On { key: 0..=127, velocity } if velocity.is_finite() && velocity > 0.) {
            self.note = true;
        }
    }
    fn reset(&mut self) {
        self.position = [0.; 2];
        self.trigger = [false; 2];
        self.note = false;
        self.finished.fill([0.; 2]);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub(crate) const SCAN_POSITION: NodeInfo = NodeInfo {
    kind: "scan_position",
    name: "ScanPosition",
    rust_variant: "ScanPosition",
    example: r#"{"kind":"scan_position"}"#,
    ..PLAYBACK_POSITION
};
pub(crate) const POSITION_RESET: NodeInfo = NodeInfo {
    kind: "position_reset",
    name: "PositionReset",
    rust_variant: "PositionReset",
    inputs: &[
        PortInfo { name: "position", default: Some(0.), unit: "frames" },
        PortInfo { name: "trigger", default: Some(0.), unit: "signal" },
        PortInfo { name: "target", default: Some(0.), unit: "frames" },
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
    ],
    outputs: &["out", "left", "right"],
    description: "Event-timed or rising-trigger position replacement.",
    configuration: "none",
    example: r#"{"kind":"position_reset"}"#,
};
pub(crate) const PLAYBACK_FINISHED: NodeInfo = NodeInfo {
    kind: "playback_finished",
    name: "PlaybackFinished",
    rust_variant: "PlaybackFinished",
    outputs: &["out", "left", "right", "finished"],
    description: "One-frame completion pulse and inactive level when leaving playback bounds.",
    example: r#"{"kind":"playback_finished"}"#,
    ..PLAYBACK_BOUNDS
};

#[cfg(test)]
mod tests {
    use super::*;
    fn run(kind: NodeKind, values: [f32; 5]) -> [f32; 2] {
        let mut kernel = crate::nodes::prepare(&kind, 48000.).unwrap();
        let inputs = values.map(|v| [v; 2]);
        let mut output = [0.; 2];
        kernel.process(inputs.each_ref().map(|v| v.as_slice()), &mut output);
        output
    }
    #[test]
    fn pitch_mapping_fades_and_window_formulas() {
        for (kind, values, expected) in [
            (NodeKind::PlaybackRate, [72., 60., 0., 0., 0.5], 1.),
            (NodeKind::PlaybackBounds, [5., 0., 5., 1., 0.], -1.),
            (NodeKind::LoopPosition, [-1., 0., 4., 0., 0.], 3.),
            (NodeKind::RegionFade, [1., 10., 2., 0., 0.], 0.5),
            (NodeKind::RangeMap, [0.5, 0., 1., -1., 1.], 0.),
            (NodeKind::Curve, [0.5, 2., 0., 0., 0.], 0.25),
            (NodeKind::BeatToPosition, [2., 120., 48000., 0., 0.], 48000.),
            (NodeKind::TempoRatio, [120., 180., 0., 0., 0.], 1.5),
            (NodeKind::GrainWindow, [2., 4., 0., 0., 0.], 1.),
        ] {
            assert_eq!(run(kind, values), [expected; 2]);
        }
    }
    #[test]
    fn position_is_sample_accurate_and_reset_is_private() {
        let mut kernel = prepare(&NodeKind::PlaybackPosition).unwrap();
        let rate = [1.; 6];
        let start = [4.; 6];
        let reset = [0.; 6];
        let gate = [1.; 6];
        let mut output = [0.; 6];
        kernel.process([&rate, &start, &reset, &gate, &reset], &mut output);
        assert_eq!(output, [0., 0., 1., 1., 2., 2.]);
        kernel.event(NoteEvent::On { key: 60, velocity: 1. });
        kernel.process([&rate, &start, &reset, &gate, &reset], &mut output);
        assert_eq!(output, [4., 4., 5., 5., 6., 6.]);
        kernel.reset();
        kernel.process([&rate, &start, &reset, &gate, &reset], &mut output);
        assert_eq!(output, [0., 0., 1., 1., 2., 2.]);
    }
}
