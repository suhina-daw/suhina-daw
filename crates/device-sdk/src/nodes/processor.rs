// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared stereo loop for stateless scalar nodes; formulas live in their type modules.
use crate::{ProcessContext, native_node::NativeNode};
use suhina_device_api::NoteEvent;
struct Scalar(fn([f64; 5]) -> f64);
pub(super) fn boxed(render: fn([f64; 5]) -> f64) -> Box<dyn NativeNode> {
    Box::new(Scalar(render))
}
impl NativeNode for Scalar {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (index, sample) in output.iter_mut().enumerate() {
            let x = std::array::from_fn(|port| {
                let v = inputs[port].get(index).copied().unwrap_or(0.);
                if v.is_finite() { f64::from(v) } else { 0. }
            });
            let v = (self.0)(x);
            *sample = if v.is_finite() { v.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32 } else { 0. };
        }
    }
    fn event(&mut self, _: NoteEvent) {}
    fn reset(&mut self) {}
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
