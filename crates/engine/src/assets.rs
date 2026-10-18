// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files a project's built-in devices run ([docs/spec/nam-amp-and-cab.md]): parsed models
//! for NAM Amp Modelers and decoded impulse responses for NAM Cab Loaders, by the file path
//! each device's state holds. The session builds this map — reading files never happens in the
//! engine — and the graphs build with it, the way they build with the audio pool (ADR 0011).

use std::collections::HashMap;

use suhina_device_registry::DeviceAsset;

/// The parsed models and IRs a project's file-backed devices run, keyed by the file's path
/// string (the project holds the same string in `Device::state`). The engine reads it by the
/// path and never touches the disk.
#[derive(Debug, Clone, Default)]
pub struct DeviceAssets {
    pub entries: HashMap<String, DeviceAsset>,
}

impl DeviceAssets {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, path: &str) -> Option<&DeviceAsset> {
        self.entries.get(path)
    }

    pub fn set(&mut self, path: impl Into<String>, asset: DeviceAsset) {
        self.entries.insert(path.into(), asset);
    }

    /// Drops every entry not in `keep`'s key set, when a project closes or reopens.
    pub fn keep_only(&mut self, keep: &std::collections::HashSet<String>) {
        self.entries.retain(|key, _| keep.contains(key));
    }
}

/// The file path a device's state holds, if it is a path a device with `id` can run.
pub(crate) fn stored_path<'a>(id: &str, state: Option<&'a [u8]>) -> Option<&'a str> {
    if !suhina_device_registry::is_file_device(id) {
        return None;
    }
    std::str::from_utf8(state?).ok().filter(|p| !p.is_empty())
}
