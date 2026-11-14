// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`DeviceFiles`]: the files a project's devices load, read off the audio thread.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use suhina_engine::DeviceAssets;
use suhina_model::{Device, DeviceKind};

/// The file a file-backed device's state names (a NAM model or IR), with the device's id; `None`
/// for any other device or an empty state.
pub(crate) fn named_file(device: &Device) -> Option<(&str, &str)> {
    let DeviceKind::Builtin(id) = &device.kind else { return None };
    if !suhina_device_registry::is_file_device(id) {
        return None;
    }
    let path = device.state.as_deref().and_then(|state| std::str::from_utf8(state).ok())?;
    (!path.is_empty()).then_some((path, id.as_str()))
}

/// The parsed models and impulse responses the project's NAM devices run
/// (`docs/spec/nam-amp-and-cab.md`), by the path each device's state holds.
#[derive(Default)]
pub(crate) struct DeviceFiles {
    pub(crate) assets: DeviceAssets,
    /// Assets added since the player last got them.
    pub(crate) changed: bool,
    /// Why a device's file is not in `assets`, by the path: what a read gave.
    pub(crate) notes: HashMap<String, String>,
    /// When each loaded file was last written, so a rewritten file is reloaded.
    pub(crate) modified: HashMap<String, Option<SystemTime>>,
    /// The files the person chose for a device in this session (`device.setNamFile`), which are
    /// read wherever they are; a path the project names is read only from where its audio may
    /// be (`Session::resolve_device_file`).
    pub(crate) chosen: HashSet<String>,
    /// How many loads the syncs have made: a parse and, for an IR, a resample each, on the
    /// session's thread. A diagnostic, read by [`crate::Session::device_file_loads`].
    pub(crate) loads: u64,
    /// The rate the loads in `assets` were made at (an IR joins the project at it). A sync at
    /// another rate, because the output opened at one the session did not know yet, reads
    /// every file again.
    pub(crate) rate: u32,
}
