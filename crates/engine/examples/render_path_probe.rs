// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sample-domain experiment, not a hardware latency or wall-clock benchmark.

use std::sync::Arc;

use suhina_engine::render_path_probe::{Config, Path, Probe};
use suhina_engine::{AudioData, AudioPool, LaunchCommand, Quantization, RenderInputs, RenderOptions, Renderer};
use suhina_model::{AudioRegion, Clip, ContentId, Device, DeviceKind, TrackId};
use suhina_test_support::ProjectBuilder;

const RATE: u32 = 48_000;
const BLOCK: usize = 128;

fn probe(lead_ms: u32, effects: &[&str]) -> Result<Probe, Box<dyn std::error::Error>> {
    let project = ProjectBuilder::new()
        .audio_track(1, "Probe")
        .with(|p| {
            if let Some(channel) = p.channels.first_mut() {
                channel.devices =
                    effects.iter().map(|id| Device::new(*id, DeviceKind::Builtin((*id).into()))).collect();
            }
        })
        .build();
    let renderer = Renderer::new(&project, RATE, suhina_device_registry::factory())?;
    Ok(Probe::new(renderer, TrackId(1), Config { block_frames: BLOCK, lead_ms })?)
}

fn signal(at: u64, out: &mut [f32]) {
    for (offset, pair) in out.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let value = (((at + offset as u64) % 251) as f32 - 125.0) / 512.0;
        pair.copy_from_slice(&[value, -value * 0.5]);
    }
}

fn constant(_: u64, out: &mut [f32]) {
    out.fill(0.25);
}

fn launcher() -> Result<Renderer, Box<dyn std::error::Error>> {
    let file = "probe-constant.wav";
    let region = AudioRegion {
        file: file.into(),
        absolute_file: None,
        sample_rate: Some(RATE),
        channels: Some(1),
        duration_seconds: Some(2.0),
        stream: None,
    };
    let project = ProjectBuilder::new()
        .audio_track(1, "Source")
        .tempo(120.0)
        .audio(1, "Constant", 4.0, region)
        .slot(1, 0, Clip::new(ContentId(1), 0.0, 4.0))
        .build();
    let mut audio = AudioPool::new();
    audio.files.insert(
        file.into(),
        Arc::new(AudioData { sample_rate: RATE, channels: 1, samples: vec![0.25; RATE as usize * 2] }),
    );
    let mut source = Renderer::build(
        &project,
        RenderInputs::from_audio(&audio, suhina_device_registry::factory()),
        RATE,
        RenderOptions::PLAYING,
    )?;
    source.set_offline(true);
    source.handle(LaunchCommand::SetQuantization(Quantization::Beats(1.0)));
    Ok(source)
}

/// Real launcher commands at the same play frame, with the source at its render cursor.
/// The late case exposes the drain policy's limit; it must not be silently called a pass.
fn launch_measurement(lead: u32, request_block: usize) -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let mut source = launcher()?;
    let mut live_source = launcher()?;
    let mut switched = probe(lead, &["utility"])?;
    let mut plain = probe(0, &["utility"])?;
    if lead != 0 {
        switched.set_path(Path::Ahead)?;
    }
    let mut actual = [0.0; BLOCK * 2];
    let mut expected = actual;
    let mut actual_start = None;
    let mut expected_start = None;
    let mut identical = true;
    let mut queued_at_request = 0;
    for block in 0..600 {
        if block == request_block {
            queued_at_request = switched.status().queued_frames;
            let command = LaunchCommand::LaunchClip { column: 0, slot: 0 };
            source.handle(command);
            live_source.handle(command);
            switched.set_path(Path::Live)?;
        }
        if block == 400 && lead != 0 {
            switched.set_path(Path::Ahead)?;
        }
        switched.process(&mut actual, true, &mut |_, out| source.process(out))?;
        plain.process(&mut expected, false, &mut |_, out| live_source.process(out))?;
        identical &= actual.iter().zip(expected).all(|(a, b)| a.to_bits() == b.to_bits());
        for (i, (a, b)) in actual.as_chunks::<2>().0.iter().zip(expected.as_chunks::<2>().0.iter()).enumerate() {
            if actual_start.is_none() && a.iter().any(|sample| *sample != 0.0) {
                actual_start = Some(block * BLOCK + i);
            }
            if expected_start.is_none() && b.iter().any(|sample| *sample != 0.0) {
                expected_start = Some(block * BLOCK + i);
            }
        }
    }
    let (Some(actual_start), Some(expected_start)) = (actual_start, expected_start) else {
        return Err("a quantized launch produced no audio".into());
    };
    let request_frame = request_block * BLOCK;
    let next_beat = (request_frame + queued_at_request).div_ceil(RATE as usize / 2) * (RATE as usize / 2);
    if expected_start != RATE as usize / 2 || actual_start != next_beat {
        return Err(format!("unexpected launch positions: live {expected_start}, switched {actual_start}").into());
    }
    if actual_start == expected_start && !identical {
        return Err("a timely quantized launch changed samples across the path switch".into());
    }
    Ok((actual_start, actual_start.abs_diff(expected_start)))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let lead = match (args.next().as_deref(), args.next()) {
        (None, None) => 0,
        (Some("--lead-ms"), Some(value)) => value.parse::<u32>()?,
        _ => return Err("usage: render_path_probe [--lead-ms 0|50|100|200]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let mut switched = probe(lead, &["utility", "delay", "reverb"])?;
    let mut plain = probe(0, &["utility", "delay", "reverb"])?;
    let target = switched.status().target_frames;
    let phase = target / BLOCK + 8;
    let mut actual = [0.0; BLOCK * 2];
    let mut expected = actual;
    for block in 0..phase * 4 {
        if lead != 0 && (block == 2 || block == phase * 2) {
            switched.set_path(Path::Ahead)?;
        }
        if block == phase || block == phase * 3 {
            switched.set_path(Path::Live)?;
        }
        switched.process(&mut actual, true, &mut signal)?;
        plain.process(&mut expected, false, &mut signal)?;
        if !actual.iter().zip(expected).all(|(a, b)| a.to_bits() == b.to_bits()) {
            return Err(format!("switch changed audio in block {block}").into());
        }
    }

    let mut device = probe(lead, &["utility"])?;
    let mut mixer = probe(lead, &["utility"])?;
    if lead != 0 {
        device.set_path(Path::Ahead)?;
        mixer.set_path(Path::Ahead)?;
    }
    for _ in 0..phase {
        device.process(&mut actual, true, &mut constant)?;
        mixer.process(&mut expected, true, &mut constant)?;
    }
    let queued = device.status().queued_frames;
    device.set_device_param(0, 0, -12.0);
    device.set_path(Path::Live)?;
    mixer.set_volume(0.25);
    let mut device_frame = None;
    let mut mixer_frame = None;
    for block in 0..phase {
        device.process(&mut actual, false, &mut constant)?;
        mixer.process(&mut expected, false, &mut constant)?;
        for (frame, (a, b)) in actual.as_chunks::<2>().0.iter().zip(expected.as_chunks::<2>().0.iter()).enumerate() {
            let at = block * BLOCK + frame;
            if a.first().is_some_and(|sample| *sample != 0.25) && device_frame.is_none() {
                device_frame = Some(at);
            }
            if b.first().is_some_and(|sample| *sample != 0.25) && mixer_frame.is_none() {
                mixer_frame = Some(at);
            }
        }
    }
    let (Some(device_frame), Some(mixer_frame)) = (device_frame, mixer_frame) else {
        return Err("a control produced no changed sample".into());
    };
    if device_frame != queued || mixer_frame >= BLOCK {
        return Err("the controls did not follow their render/play cursors".into());
    }
    let ms = |frames: usize| frames as f64 * 1000.0 / f64::from(RATE);
    let (early_launch_frame, early_launch_drift) = launch_measurement(lead, 96)?;
    let (late_launch_frame, late_launch_drift) = launch_measurement(lead, 180)?;
    println!(
        "{{\"kind\":\"sample-domain\",\"platform\":\"{}\",\"sample_rate\":{RATE},\"block_frames\":{BLOCK},\"lead_ms\":{lead},\"rounded_lead_frames\":{target},\"bit_identical\":true,\"device_response_frames\":{device_frame},\"device_response_ms\":{:.6},\"mixer_response_frames\":{mixer_frame},\"mixer_response_ms\":{:.6},\"early_launch_frame\":{early_launch_frame},\"early_launch_drift_frames\":{early_launch_drift},\"late_launch_frame\":{late_launch_frame},\"late_launch_drift_frames\":{late_launch_drift}}}",
        std::env::consts::OS,
        ms(device_frame),
        ms(mixer_frame),
    );
    Ok(())
}
