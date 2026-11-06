// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared audio processing nodes; DSP comes from suhina-dsp.
use super::{NodeInfo, PortInfo};
use crate::{NodeKind, ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
pub(crate) const DC_BLOCK: NodeInfo = NodeInfo {
    kind: "dc_block",
    name: "DCBlock",
    rust_variant: "DcBlock",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "Stereo DC removal through the shared 5 Hz DSP blocker.",
    configuration: "none",
    example: r#"{"kind":"dc_block"}"#,
};
struct Kernel([suhina_dsp::DcBlocker; 2]);
pub(crate) fn prepare(kind: &NodeKind, rate: f64) -> Option<Box<dyn NativeNode>> {
    matches!(kind, NodeKind::DcBlock)
        .then(|| Box::new(Kernel([suhina_dsp::DcBlocker::new(rate); 2])) as Box<dyn NativeNode>)
}
impl NativeNode for Kernel {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (i, sample) in output.iter_mut().enumerate() {
            let x = inputs[0].get(i).copied().unwrap_or(0.);
            let x = if x.is_finite() { f64::from(x) } else { 0. };
            *sample = self.0[i % 2].process(x).clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32;
        }
    }
    fn event(&mut self, _: NoteEvent) {}
    fn reset(&mut self) {
        for channel in &mut self.0 {
            channel.reset();
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
