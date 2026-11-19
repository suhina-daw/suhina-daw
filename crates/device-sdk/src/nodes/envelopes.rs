// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Gate and completion adapters for shared DSP linear envelopes.
use super::{NodeInfo, PortInfo};
use crate::{NodeKind, ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
use suhina_dsp::envelope::{LinearEnvelope, LinearEnvelopeSettings};
pub(crate) const ADSR: NodeInfo = NodeInfo {
    kind: "adsr",
    name: "ADSR",
    rust_variant: "Adsr",
    inputs: &[
        PortInfo { name: "gate", default: Some(0.0), unit: "signal" },
        PortInfo { name: "attack", default: Some(0.005), unit: "seconds" },
        PortInfo { name: "decay", default: Some(0.2), unit: "seconds" },
        PortInfo { name: "sustain", default: Some(0.8), unit: "signal" },
        PortInfo { name: "release", default: Some(0.2), unit: "seconds" },
    ],
    outputs: &["out", "left", "right", "finished"],
    description: "Linear ADSR with timed stages and explicit completion.",
    configuration: "none",
    example: r#"{"kind":"adsr"}"#,
};
pub(crate) const AD: NodeInfo = NodeInfo {
    kind: "ad",
    name: "AD",
    rust_variant: "Ad",
    inputs: &[
        PortInfo { name: "trigger", default: Some(0.0), unit: "signal" },
        PortInfo { name: "attack", default: Some(0.005), unit: "seconds" },
        PortInfo { name: "decay", default: Some(0.2), unit: "seconds" },
    ],
    outputs: &["out", "left", "right", "finished"],
    description: "Linear AD with timed stages and explicit completion.",
    configuration: "none",
    example: r#"{"kind":"ad"}"#,
};

struct Kernel {
    ad: bool,
    rate: f64,
    envelopes: [LinearEnvelope; 2],
    gate: [bool; 2],
    active: [bool; 2],
    finished: [[f32; 2]; 64],
}
pub(crate) fn prepare(kind: &NodeKind, rate: f64) -> Option<Box<dyn NativeNode>> {
    matches!(kind, NodeKind::Adsr | NodeKind::Ad).then(|| {
        Box::new(Kernel {
            ad: matches!(kind, NodeKind::Ad),
            rate,
            envelopes: [LinearEnvelope::default(); 2],
            gate: [false; 2],
            active: [false; 2],
            finished: [[1.; 2]; 64],
        }) as Box<dyn NativeNode>
    })
}
impl NativeNode for Kernel {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            for (channel, sample) in pair.iter_mut().enumerate() {
                let x: [f64; 5] = std::array::from_fn(|p| {
                    let v = inputs[p].get(frame * 2 + channel).copied().unwrap_or(0.);
                    if v.is_finite() { f64::from(v) } else { 0. }
                });
                let gate = x[0] > 0.;
                if gate && !self.gate[channel] {
                    self.envelopes[channel].start();
                    self.active[channel] = true;
                }
                if !self.ad && !gate && self.gate[channel] && self.active[channel] {
                    self.envelopes[channel].release();
                }
                self.gate[channel] = gate;
                if self.active[channel] {
                    let settings = LinearEnvelopeSettings {
                        attack: x[1],
                        decay: x[2],
                        sustain: if self.ad { 0.0 } else { x[3] },
                        release: x[4],
                    };
                    self.envelopes[channel].next(&settings, self.rate);
                    self.active[channel] = !self.envelopes[channel].is_done();
                    if self.ad && self.envelopes[channel].is_sustaining() {
                        self.active[channel] = false;
                    }
                }
                *sample = self.envelopes[channel].value() as f32;
                if let Some(row) = self.finished.get_mut(frame) {
                    row[channel] = f32::from(!self.active[channel]);
                }
            }
        }
    }
    fn output(&self, _: usize, frame: usize) -> [f32; 2] {
        self.finished.get(frame).copied().unwrap_or([1.; 2])
    }
    fn event(&mut self, _: NoteEvent) {}
    fn reset(&mut self) {
        self.envelopes = [LinearEnvelope::default(); 2];
        self.gate = [false; 2];
        self.active = [false; 2];
        self.finished.fill([1.; 2]);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_gates_release_retrigger_and_report_completion_independently() {
        let mut node = prepare(&NodeKind::Adsr, 8.0).unwrap();
        let mut next = |gate: [f32; 2], sustain: f32| {
            let mut out = [0.0; 2];
            node.process([&gate, &[0.5; 2], &[0.5; 2], &[sustain; 2], &[0.5; 2]], &mut out);
            (out, node.output(3, 0))
        };
        assert_eq!(next([0.0; 2], 0.5), ([0.0; 2], [1.0; 2]));
        assert_eq!(next([1.0, 0.0], 0.5), ([0.25, 0.0], [0.0, 1.0]));
        assert_eq!(next([1.0; 2], 0.5), ([0.5, 0.25], [0.0; 2]));
        assert_eq!(next([0.0, 1.0], 0.5), ([0.375, 0.5], [0.0; 2]));
        assert_eq!(next([0.0; 2], 0.5), ([0.25, 0.375], [0.0; 2]));
        assert_eq!(next([1.0, 0.0], 0.5), ([0.25, 0.25], [0.0; 2]));
        assert_eq!(next([1.0, 0.0], 0.5), ([0.5, 0.125], [0.0; 2]));
        assert_eq!(next([1.0, 0.0], 0.5), ([0.75, 0.0], [0.0, 1.0]));
        for _ in 0..5 {
            next([1.0, 0.0], 0.5);
        }
        assert_eq!(next([1.0, 0.0], 0.9), ([0.5, 0.0], [0.0, 1.0]));
        node.reset();
        let mut out = [1.0; 2];
        node.process([&[0.0; 2]; 5], &mut out);
        assert_eq!(out, [0.0; 2]);
        assert_eq!(node.output(3, 0), [1.0; 2]);
    }

    #[test]
    fn ad_ignores_gate_release_and_finishes_at_sustain() {
        let mut node = prepare(&NodeKind::Ad, 4.0).unwrap();
        let mut actual = Vec::new();
        for frame in 0..6 {
            let mut out = [0.0; 2];
            let gate = if frame == 0 { 1.0 } else { 0.0 };
            node.process([&[gate; 2], &[0.5; 2], &[0.5; 2], &[], &[]], &mut out);
            actual.push((out[0], node.output(3, 0)[0]));
        }
        assert_eq!(actual, [(0.5, 0.0), (1.0, 0.0), (0.5, 0.0), (0.0, 1.0), (0.0, 1.0), (0.0, 1.0)]);
    }
}
