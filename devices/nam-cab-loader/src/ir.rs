// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading `.wav` impulse responses (docs/spec/nam-amp-and-cab.md). I/O happens where this
//! module is called — the session, at command time or a project's load — never on the audio
//! thread. A file at another rate than the project's is resampled here, once at load, in whole
//! buffers, so the device's convolution always runs at the project's rate.

use std::path::Path;

use crate::Ir;
use suhina_dsp::convolver::MAX_FRAMES;

/// The largest `.wav` read, in bytes.
pub const MAX_BYTES: usize = 64 << 20;

/// The IR file at `path`, decoded and, when the project runs at another rate, resampled to
/// `project_rate`. A file over [`MAX_BYTES`], longer than [`MAX_FRAMES`], unreadable or not a
/// `.wav` Suhina reads comes back as the message the panel shows.
pub fn read(path: impl AsRef<Path>, project_rate: u32) -> Result<Ir, String> {
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
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("{} is not a wav file ({e})", path.display()))?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let frames = reader.duration() as usize;
    if frames > MAX_FRAMES {
        return Err(format!("{} is longer than the {MAX_FRAMES} frames Suhina reads", path.display()));
    }
    let mut samples: Vec<f32> = Vec::with_capacity(frames.saturating_mul(channels));
    let mut push = |value: Result<f32, hound::Error>| -> Result<(), String> {
        if samples.len() < frames.saturating_mul(channels) {
            samples.push(match value {
                Ok(v) => v,
                Err(e) => return Err(format!("{} is not a wav file Suhina reads ({e})", path.display())),
            });
        }
        Ok(())
    };
    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, bits) if bits == 32 || bits == 64 => {
            for sample in reader.samples::<f32>() {
                push(sample)?;
            }
        }
        (hound::SampleFormat::Int, bits) if bits == 16 || bits == 24 || bits == 32 => {
            for sample in reader.samples::<i32>() {
                // hound widens every integer to i32 at its own width: a 24-bit sample runs
                // ±8 388 608, so the divisor is the width's own range, not i32's (a 24-bit
                // file divided by i32::MAX would come out 48 dB too quiet).
                let scale = (1_i64 << (bits - 1)) as f64;
                push(sample.map(|v| (f64::from(v) / scale) as f32))?;
            }
        }
        _ => return Err(format!("{} is a wav format Suhina does not read", path.display())),
    }
    let mut ir = Ir::from_interleaved(spec.channels.max(1), frames, spec.sample_rate, &samples);
    if ir.sample_rate != project_rate.max(1) {
        ir = ir.resampled_to(project_rate.max(1));
    }
    Ok(ir)
}
