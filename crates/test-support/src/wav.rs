// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! WAV files for tests that import, decode or play audio.

use std::path::Path;

/// How [`write_wav`] stores samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormat {
    /// 16-bit integers, as most sample packs ship.
    Int16,
    /// 32-bit floats, which keep the samples exactly.
    Float32,
}

/// Writes interleaved `samples` with `channels` channels at `sample_rate` to `path`, creating its
/// folder.
pub fn write_wav(path: &Path, sample_rate: u32, channels: u16, format: WavFormat, samples: &[f32]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let (bits_per_sample, sample_format) = match format {
        WavFormat::Int16 => (16, hound::SampleFormat::Int),
        WavFormat::Float32 => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec { channels, sample_rate, bits_per_sample, sample_format };
    let mut writer = hound::WavWriter::create(path, spec).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    for &s in samples {
        match format {
            WavFormat::Int16 => writer.write_sample((s.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16),
            WavFormat::Float32 => writer.write_sample(s),
        }
        .unwrap();
    }
    writer.finalize().unwrap();
}
