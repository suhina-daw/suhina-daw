// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading `.nam` model files (docs/spec/nam-amp-and-cab.md). I/O happens where this module is
//! called — the session, at command time or a project's load — never on the audio thread.

use std::path::Path;

use crate::NamModel;

/// The largest `.nam` file read, in bytes.
pub const MAX_BYTES: usize = 256 << 20;

/// The model file at `path`, parsed. A file over [`MAX_BYTES`], an unreadable path or one that
/// is not a `.nam` JSON comes back as the message the panel shows.
pub fn read(path: impl AsRef<Path>) -> Result<NamModel, String> {
    let path = path.as_ref();
    let bytes = match std::fs::metadata(path) {
        Ok(info) => info.len(),
        Err(e) => return Err(format!("{} ({e})", path.display())),
    };
    if bytes > MAX_BYTES as u64 {
        let megabytes = (bytes as f64 / f64::from(1u32 << 20) * 10.0).round() / 10.0;
        return Err(format!(
            "{} is {megabytes} MiB, larger than Suhina reads ({} MiB)",
            path.display(),
            MAX_BYTES >> 20
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{} ({e})", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{} is not a UTF-8 file", path.display()))?;
    NamModel::from_json_str(&text).map_err(|e| format!("{} is not a NAM model ({e})", path.display()))
}

/// The model's own name from its metadata, as the panel shows it.
pub fn display_name(model: &NamModel) -> Option<String> {
    model.metadata_typed().name
}

/// Whether the model's metadata says the capture already contains a speaker cabinet, so an IR
/// loader after it would put two cabinets in series. `None` when the file says nothing.
pub fn includes_cab(model: &NamModel) -> Option<bool> {
    model.includes_cab()
}
