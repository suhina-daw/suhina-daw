// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`DeviceFiles`]: the files a project's devices load, read off the audio thread.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use suhina_engine::DeviceAssets;

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
    /// be (`Session::device_file_location`).
    pub(crate) chosen: HashSet<String>,
}
