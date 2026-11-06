// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`DeviceAt`]: where a device is on its track, as the views and the device commands name it.
//! Shared by the detail area and the Launcher view, so neither reaches into the other (#764 › 12).

use serde_json::json;

/// Where a device is on its track: its index, or its path through device groups
/// (`docs/spec/device-groups.md` › Commands).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DeviceAt(pub Vec<usize>);

impl DeviceAt {
    /// The `device` parameter of a device command: the index of a device on the track, or the
    /// path `"2/1/0"` of one inside a group.
    pub fn value(&self) -> serde_json::Value {
        match self.0.as_slice() {
            [index] => json!(index),
            path => json!(path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/")),
        }
    }

    /// Whether it is on the track itself, not inside a group.
    pub fn top(&self) -> bool {
        self.0.len() == 1
    }

    /// The device at `index` on the track itself.
    pub fn at(index: usize) -> Self {
        Self(vec![index])
    }

    /// From a device command's `device` parameter: an index, or a path `"2/1/0"`.
    pub fn parse(value: &serde_json::Value) -> Option<Self> {
        if let Some(index) = value.as_u64() {
            return usize::try_from(index).ok().map(Self::at);
        }
        let path: Option<Vec<usize>> = value.as_str()?.split('/').map(|n| n.trim().parse().ok()).collect();
        path.filter(|p| !p.is_empty()).map(Self)
    }

    /// Its index on the track, when it is on the track itself.
    pub fn index(&self) -> Option<usize> {
        match self.0.as_slice() {
            [index] => Some(*index),
            _ => None,
        }
    }

    /// The chain or branch it is in (the path to it without its own index), and its index there.
    pub fn split(&self) -> (&[usize], usize) {
        match self.0.split_last() {
            Some((last, parent)) => (parent, *last),
            None => (&[], 0),
        }
    }
}
