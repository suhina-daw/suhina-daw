// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`DeviceFactory`]: what makes the devices a project names, handed to the engine as a value so
//! the engine builds without knowing any concrete device (#764 › 11, #1147). The device registry
//! implements it for every built-in; the session hands the registry's to the engine.

use std::any::Any;
use std::sync::Arc;

use suhina_device_api::Device;

use crate::sample_resource::PreparedSamples;
use crate::{DeviceGraph, DeviceSdkError, GraphDevice};

/// The file a device runs, already read and parsed or decoded off the audio thread
/// (docs/spec/nam-amp-and-cab.md): a model for the Amp Modeler, an impulse response for the Cab
/// Loader. The session keeps a map of these by the file's path; the engine hands the device the
/// map's entry when it builds its chains. Only the factory that read it knows what it holds.
#[derive(Clone)]
pub struct DeviceAsset {
    device: &'static str,
    data: Arc<dyn Any + Send + Sync>,
}

impl DeviceAsset {
    /// What the device `device` read: `data` is its own type, which only it reads back.
    pub fn new(device: &'static str, data: Arc<dyn Any + Send + Sync>) -> Self {
        Self { device, data }
    }

    /// The id of the device that runs it.
    pub fn device(&self) -> &'static str {
        self.device
    }

    /// What the device read.
    pub fn data(&self) -> &(dyn Any + Send + Sync) {
        &*self.data
    }
}

impl std::fmt::Debug for DeviceAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceAsset").field("device", &self.device).finish_non_exhaustive()
    }
}

/// Makes the devices a project names: the built-ins by id, `.sd` graphs, and the instrument that
/// plays previewed notes. Called off the audio thread; what it returns may then run on it.
pub trait DeviceFactory: Send + Sync {
    /// A new built-in device `id`, running `asset` when it is the file this device reads, or
    /// `None` for an unknown id.
    fn create(&self, id: &str, asset: Option<&DeviceAsset>) -> Option<Box<dyn Device>>;

    /// A `.sd` graph as a device prepared at `rate`, playing `samples`.
    fn graph_device(
        &self,
        graph: &DeviceGraph,
        rate: f64,
        samples: PreparedSamples,
    ) -> Result<GraphDevice, DeviceSdkError>;

    /// Whether device `id` runs a file whose path its state holds.
    fn runs_a_file(&self, id: &str) -> bool;

    /// The instrument that plays previewed notes on a track with none, unprepared.
    fn preview_instrument(&self) -> Option<Box<dyn Device>>;
}

/// A factory shared by every renderer, graph and updater that builds with it.
pub type Devices = Arc<dyn DeviceFactory>;
