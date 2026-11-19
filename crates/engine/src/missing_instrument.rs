// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An unavailable instrument must not turn notes into the empty track's fallback tone.
use suhina_device_api::{Device, DeviceError, DeviceInfo, ProcessContext, Role, check_sample_rate};

pub(crate) struct MissingInstrument;
static INFO: DeviceInfo = DeviceInfo::new("missing-instrument", "Unavailable instrument", Role::Instrument, &[]);

impl Device for MissingInstrument {
    fn info(&self) -> &DeviceInfo {
        &INFO
    }
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        check_sample_rate(sample_rate).map(|_| ())
    }
    fn reset(&mut self) {}
    fn set_param(&mut self, _: usize, _: f64) {}
    fn param(&self, _: usize) -> Option<f64> {
        None
    }
    fn process(&mut self, _: &ProcessContext, buffer: &mut [f32]) {
        buffer.fill(0.);
    }
}
