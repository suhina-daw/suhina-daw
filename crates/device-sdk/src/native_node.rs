// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native graph kernel contract and off-thread factory.
use crate::{DeviceSdkError, NodeKind, PreparedEffect, ProcessContext};
use std::any::Any;
use suhina_device_api::NoteEvent;

/// Prepared native stereo graph kernel. All methods except resource access run without allocation,
/// locking or I/O. Construction and disposal belong off the audio thread.
pub trait NativeNode: Send {
    fn prepare_block(&mut self, context: &ProcessContext);
    fn process(&mut self, inputs: [&[f32]; 5], buffer: &mut [f32]);
    /// Read a zero-based named output for a frame of the last process call (at most 64 frames).
    fn output(&self, _port: usize, _frame: usize) -> [f32; 2] {
        [0.; 2]
    }
    /// Route one event into a bounded caller-owned batch. Default kernels consume and pass it on.
    fn route_event(&mut self, event: NoteEvent, output: &mut [NoteEvent]) -> usize {
        self.event(event);
        if let Some(slot) = output.first_mut() {
            *slot = event;
            1
        } else {
            0
        }
    }
    fn set_external_inputs(&mut self, _inputs: [&[f32]; 5]) {}
    /// Off-thread input validation and connection setup.
    fn connected_inputs(&mut self, _mask: [bool; 5]) -> Result<(), DeviceSdkError> {
        Ok(())
    }
    fn parameter_range(&self, _id: &str) -> Option<(f64, f64)> {
        None
    }
    fn parameter_input(&self, _id: &str) -> Option<usize> {
        None
    }
    fn set_parameter(&mut self, _id: &str, _value: f64) {}
    fn voice_finished(&self) -> Option<bool> {
        None
    }
    fn event(&mut self, event: NoteEvent);
    fn reset(&mut self);
    /// Host resource binding only; never call in an audio callback.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub(crate) fn prepare(kind: &NodeKind, rate: f64) -> Result<Option<Box<dyn NativeNode>>, DeviceSdkError> {
    if kind.requires_samples() {
        return Err(DeviceSdkError("sample resource nodes require compile_with_samples_at".into()));
    }
    if let Some(node) = crate::nodes::sample::prepare(kind, rate)? {
        return Ok(Some(node));
    }
    if let Some(node) = crate::nodes::random::prepare(kind) {
        return Ok(Some(node));
    }
    if let Some(node) = crate::nodes::slicing::prepare(kind) {
        return Ok(Some(node));
    }
    if let Some(node) = crate::nodes::component::prepare_input(kind) {
        return Ok(Some(node));
    }
    if let Some(node) = crate::nodes::events::prepare(kind, rate)? {
        return Ok(Some(node));
    }
    if let Some(node) = crate::nodes::voice::prepare_control(kind, rate) {
        return Ok(Some(node));
    }
    if let Some(node) = crate::nodes::envelopes::prepare(kind, rate) {
        return Ok(Some(node));
    }
    Ok(crate::nodes::prepare(kind, rate))
}

impl crate::DeviceGraph {
    /// Prepare an audio graph with trusted host kernels. The resolver runs only here, off-thread;
    /// it must return a fresh, sample-rate-prepared kernel or reject an unknown identifier.
    /// This API does not load plug-ins or accept legacy whole-device wrappers.
    pub fn compile_with_native_at(
        &self,
        sample_rate: f64,
        mut resolve: impl FnMut(&str, f64) -> Result<Box<dyn NativeNode>, DeviceSdkError>,
    ) -> Result<PreparedEffect, DeviceSdkError> {
        self.validate()?;
        if self.role == crate::DeviceRole::MidiEffect {
            return Err(DeviceSdkError("native kernels require an audio graph".into()));
        }
        PreparedEffect::new_with_native(self, sample_rate, &mut resolve)
    }
}

pub(crate) fn prepare_with(
    kind: &NodeKind,
    rate: f64,
    resolve: &mut impl FnMut(&str, f64) -> Result<Box<dyn NativeNode>, DeviceSdkError>,
) -> Result<Option<Box<dyn NativeNode>>, DeviceSdkError> {
    match kind {
        NodeKind::NativeEffect { effect } => Ok(Some(resolve(effect, rate)?)),
        _ => prepare(kind, rate),
    }
}

pub(crate) use crate::nodes::factory::compile_graph;
