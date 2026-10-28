// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Core, Options, Settings};
use std::{error::Error, path::Path};

fn validate(spec: hound::WavSpec, bytes: u64) -> Result<(), Box<dyn Error>> {
    if bytes > 128 * 1024 * 1024
        || spec.channels != 2
        || spec.sample_format != hound::SampleFormat::Float
        || spec.bits_per_sample != 32
        || !(8000..=384_000).contains(&spec.sample_rate)
    {
        return Err("expected bounded 8..384 kHz stereo float32 WAV".into());
    }
    Ok(())
}

pub fn render(input: &Path, output: &Path, options: &Options) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::metadata(input)?.len();
    if bytes > 128 * 1024 * 1024 {
        return Err("input exceeds 128 MiB".into());
    }
    let mut reader = hound::WavReader::open(input)?;
    let spec = reader.spec();
    validate(spec, bytes)?;
    if reader.len() == 0 || reader.len() % 2 != 0 {
        return Err("input needs complete stereo frames".into());
    }
    let decay = match options.profile.as_str() {
        "short" => 0.5,
        "baseline" => 2.,
        "long" => 6.,
        _ => return Err("unsupported profile".into()),
    };
    let settings = options.settings(
        &options.profile,
        Settings { sample_rate: spec.sample_rate, decay_seconds: decay, ..Settings::default() },
    )?;
    let mut core = Core::new(settings, options.diffused, options.tank)?;
    let mut writer = hound::WavWriter::create(output.join("input-wet.wav"), spec)?;
    let mut samples = reader.samples::<f32>();
    let mut count = 0_u64;
    let mut peak = 0_f32;
    loop {
        let mut block = [0_f32; 256];
        let mut used = 0;
        for slot in &mut block {
            let Some(value) = samples.next() else { break };
            let value = value?;
            if !value.is_finite() {
                return Err("non-finite input sample".into());
            }
            *slot = value;
            used += 1;
        }
        if used == 0 {
            break;
        }
        let slice = block.get_mut(..used).ok_or("invalid block length")?;
        core.process(slice);
        for value in slice {
            if !value.is_finite() {
                return Err("non-finite output sample".into());
            }
            peak = peak.max(value.abs());
            writer.write_sample(*value)?;
            count += 1;
        }
    }
    writer.finalize()?;
    let manifest = serde_json::json!({
        "sample_rate": spec.sample_rate, "frames": count / 2, "peak": peak,
        "profile": options.profile, "decay_seconds": settings.core.decay_seconds,
        "room_scale": settings.core.room_scale, "high_decay_ratio": settings.core.high_decay_ratio,
        "settings": super::probe_core::describe(settings), "normalized": false,
        "commit": super::command("git", &["rev-parse", "HEAD"]),
        "source_dirty": super::command("git", &["status", "--porcelain"]).map(|s| !s.is_empty()),
        "history_bytes": core.history_bytes(), "tank": options.tank, "diffused": options.diffused,
    });
    std::fs::write(output.join("input-manifest.json"), serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_formats_and_oversized_inputs() {
        let valid = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        assert!(validate(valid, 1024).is_ok());
        assert!(validate(valid, 128 * 1024 * 1024 + 1).is_err());
        assert!(validate(hound::WavSpec { channels: 1, ..valid }, 1024).is_err());
        assert!(validate(hound::WavSpec { sample_rate: 0, ..valid }, 1024).is_err());
        assert!(validate(hound::WavSpec { sample_format: hound::SampleFormat::Int, ..valid }, 1024).is_err());
    }
}
