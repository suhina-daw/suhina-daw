// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Explicit native-kernel preparation without an SDK-to-registry dependency.
use crate::{Connection, DeviceGraph, DeviceRole, DeviceSdkError, NodeKind};
use suhina_device_api::{Device, Role};

impl DeviceGraph {
    /// The implementation requested by a canonical whole-device wrapper.
    pub fn native_device_id(&self) -> Result<Option<&str>, DeviceSdkError> {
        let natives: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::NativeDevice { device } => Some((n, device.as_str())),
                _ => None,
            })
            .collect();
        if natives.is_empty() {
            return Ok(None);
        }
        self.validate()?;
        let [(native, id)] = natives.as_slice() else {
            return Err(DeviceSdkError("one native_device per wrapper is required".into()));
        };
        let output = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::AudioOutput)
            .ok_or_else(|| DeviceSdkError("missing native output".into()))?;
        let mut expected = vec![Connection::new(&native.id, &output.id)];
        let count = if self.role == DeviceRole::Effect {
            let input = self
                .nodes
                .iter()
                .find(|n| n.kind == NodeKind::AudioInput)
                .ok_or_else(|| DeviceSdkError("missing native input".into()))?;
            expected.push(Connection::new(&input.id, &native.id));
            3
        } else if self.role == DeviceRole::Instrument {
            2
        } else {
            return Err(DeviceSdkError("native MIDI effects are not supported".into()));
        };
        if self.nodes.len() != count
            || self.connections.len() != expected.len()
            || expected.iter().any(|c| !self.connections.contains(c))
        {
            return Err(DeviceSdkError(
                "native_device must be a whole-device wrapper without extra nodes, ports or feedback".into(),
            ));
        }
        Ok(Some(id))
    }

    /// Prepare trusted native DSP off the audio thread; the file never supplies code.
    pub fn prepare_native_at(
        &self,
        rate: f64,
        factory: &dyn Fn(&str) -> Option<Box<dyn Device>>,
    ) -> Result<Box<dyn Device>, DeviceSdkError> {
        let id = self.native_device_id()?.ok_or_else(|| DeviceSdkError("this graph has no native_device".into()))?;
        let mut device =
            factory(id).ok_or_else(|| DeviceSdkError(format!("native device {id:?} is not registered")))?;
        let info = device.info();
        let role = if info.role == Role::Instrument { DeviceRole::Instrument } else { DeviceRole::Effect };
        // Historic revisions retain their validated prefix; appended controls use defaults.
        let legacy_sampler_panel = id == "sampler"
            && matches!(self.parameters.len(), 52 | 53 | 56 | 59)
            && info.params.get(self.parameters.len()..).is_some_and(|tail| {
                tail.iter().all(|p| {
                    p.id.starts_with("slice_")
                        || matches!(
                            p.id,
                            "panel_view"
                                | "lfo_filter"
                                | "lfo_volume"
                                | "lfo_pan"
                                | "lfo_shape"
                                | "fade_in"
                                | "fade_out"
                                | "sensitivity"
                                | "slice_by"
                        )
                })
            });
        if self.role != role
            || (self.parameters.len() != info.params.len() && !legacy_sampler_panel)
            || self.parameters.iter().zip(info.params).any(|(a, b)| {
                a.id != b.id
                    || a.min != b.min
                    || (a.max != b.max && !(id == "sampler" && a.id == "playback" && a.max == 2. && b.max == 3.))
                    || b.clamp(a.default) != a.default
                    || a.smoothing_ms != 0.
            })
        {
            return Err(DeviceSdkError(
                "native wrapper role and parameter order/ranges must match its registered implementation".into(),
            ));
        }
        device.prepare(rate).map_err(|e| DeviceSdkError(e.to_string()))?;
        for (index, p) in self.parameters.iter().enumerate() {
            device.set_param(index, p.default);
        }
        device.reset();
        Ok(device)
    }
}
