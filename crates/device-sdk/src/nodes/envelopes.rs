// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Linear timed envelope stages shared by graph envelopes and the sample instrument.
use super::{NodeInfo, PortInfo};
use crate::{NodeKind, ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
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

/// Returns false only when release reaches zero. Stage numbers preserve the sampler contract.
pub(crate) fn step(level: &mut f64, stage: &mut u8, released: f64, params: [f64; 4], rate: f64) -> bool {
    match *stage {
        0 => {
            *level = (*level + 1. / (params[0] * rate).max(1.)).min(1.);
            if *level >= 1. {
                *stage = 1;
            }
        }
        1 => {
            *level = (*level - (1. - params[2]) / (params[1] * rate).max(1.)).max(params[2]);
            if *level <= params[2] {
                *stage = 2;
            }
        }
        3 => {
            *level = (*level - released.max(1e-12) / (params[3] * rate).max(1.)).max(0.);
            if *level <= 0. {
                return false;
            }
        }
        _ => {}
    }
    true
}
struct Kernel {
    ad: bool,
    rate: f64,
    level: [f64; 2],
    stage: [u8; 2],
    released: [f64; 2],
    gate: [bool; 2],
    active: [bool; 2],
    finished: [[f32; 2]; 64],
}
pub(crate) fn prepare(kind: &NodeKind, rate: f64) -> Option<Box<dyn NativeNode>> {
    matches!(kind, NodeKind::Adsr | NodeKind::Ad).then(|| {
        Box::new(Kernel {
            ad: matches!(kind, NodeKind::Ad),
            rate,
            level: [0.; 2],
            stage: [2; 2],
            released: [0.; 2],
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
                    self.level[channel] = 0.;
                    self.stage[channel] = 0;
                    self.active[channel] = true;
                }
                if !self.ad && !gate && self.gate[channel] && self.active[channel] {
                    self.released[channel] = self.level[channel];
                    self.stage[channel] = 3;
                }
                self.gate[channel] = gate;
                if self.active[channel] {
                    let params = [
                        x[1].clamp(0., 60.),
                        x[2].clamp(0., 60.),
                        if self.ad { 0. } else { x[3].clamp(0., 1.) },
                        x[4].clamp(0., 60.),
                    ];
                    self.active[channel] = step(
                        &mut self.level[channel],
                        &mut self.stage[channel],
                        self.released[channel],
                        params,
                        self.rate,
                    );
                    if self.ad && self.stage[channel] == 2 {
                        self.active[channel] = false;
                    }
                }
                *sample = self.level[channel] as f32;
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
        self.level = [0.; 2];
        self.stage = [2; 2];
        self.released = [0.; 2];
        self.gate = [false; 2];
        self.active = [false; 2];
        self.finished.fill([1.; 2]);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
