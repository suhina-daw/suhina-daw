// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading `.nam` model files (docs/spec/nam-amp-and-cab.md). I/O happens where this module is
//! called — the session, at command time or a project's load — never on the audio thread.

use std::path::Path;

use suhina_device_api::SAMPLE_RATE_RANGE;

use crate::NamModel;

/// How far a model's rate may sit from the project's, as one rate over the other. Above it
/// the two resampler legs' staging was sized with the ratio unbounded ([AGENTS.md] rule 2's
/// hostile-input cap gone) and the delivery ring could not always keep every frame they
/// produced. Real projects run 8 to 384 kHz and real models 44.1 or 48 kHz, so the widest
/// real pair — a 44.1 kHz model in a 384 kHz project — sits at 8.7, well inside this; the
/// refusing limit exists for a broken or hostile file declaring a low rate.
///
/// [AGENTS.md]: https://github.com/suhina-daw/suhina-daw/blob/main/AGENTS.md#1-golden-rules
pub const MAX_RATE_RATIO: f64 = 16.0;

/// Whether the model's rate is one the device can run against `project_rate`: `Err` with
/// the message the panel's note row shows for a rate that is not a number, outside the
/// rates Suhina runs ([`SAMPLE_RATE_RANGE`]) or more than [`MAX_RATE_RATIO`] away from the
/// project's, `Ok` otherwise.
///
/// The check runs twice by design: once when the file is chosen, so the note names the
/// fault at once, and again at every `prepare`, so a project's rate changing afterwards
/// keeps the device safe — the same parsed file can become unrunnable when the project
/// moves from 48 kHz to 384 kHz.
pub fn runnable_rate(project_rate: f64, model_rate: f64) -> Result<(), String> {
    if !model_rate.is_finite() {
        return Err("the model's sample rate is not a number".to_owned());
    }
    if !(SAMPLE_RATE_RANGE.0..=SAMPLE_RATE_RANGE.1).contains(&model_rate) {
        return Err(format!(
            "the model's sample rate is {model_rate} Hz, outside the rates Suhina runs (8 to 384 kHz)"
        ));
    }
    let ratio = model_rate / project_rate;
    if !ratio.is_finite() || ratio > MAX_RATE_RATIO || 1.0 / ratio > MAX_RATE_RATIO {
        return Err(format!(
            "the model runs at {model_rate} Hz, too far from the project's {project_rate} Hz to resample"
        ));
    }
    Ok(())
}

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
