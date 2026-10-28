// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Developer-only render/CPU probe; see docs/spec/reverb-prototype.md.

use serde_json::{Value, json};
use std::{error::Error, hint::black_box, path::Path, time::Instant};
use suhina_dsp::fdn::Settings;
#[path = "fdn_probe/file.rs"]
mod file_probe;
#[path = "fdn_probe/core.rs"]
mod probe_core;
use probe_core::{Core, Options};

const RATE: u32 = 48_000;

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os().nth(1).ok_or("usage: fdn_probe <output-directory>")?;
    let output = Path::new(&output);
    let options = Options::read()?;
    if output.exists() {
        return Err("output directory already exists; choose a new run directory to preserve results".into());
    }
    std::fs::create_dir_all(output)?;
    if let Some(input) = &options.input {
        return file_probe::render(input, output, &options);
    }
    let mut renders = Vec::new();
    for (name, decay, scale, ratio) in [("short", 0.5, 0.7, 1.), ("baseline", 2., 1., 1.), ("long", 6., 1.5, 0.5)] {
        let (scale, ratio) = if options.flat_decay { (1., 1.) } else { (scale, ratio) };
        let settings =
            Settings { decay_seconds: decay, room_scale: scale, high_decay_ratio: ratio, ..Settings::default() };
        let developed = options.settings(name, settings)?;
        let settings = developed.core;
        for stimulus in ["impulse", "percussion", "tone"] {
            if options.impulse_only && stimulus != "impulse" {
                continue;
            }
            let dry = signal(stimulus, RATE, 20);
            let mut wet = dry.clone();
            let mut core = Core::new(developed, options.diffused, options.tank)?;
            core.process(&mut wet);
            write_wav(&output.join(format!("{name}-{stimulus}-dry.wav")), &dry)?;
            write_wav(&output.join(format!("{name}-{stimulus}-wet.wav")), &wet)?;
            let mixed: Vec<f32> = dry.iter().zip(&wet).map(|(a, b)| 0.8 * a + 0.2 * b).collect();
            write_wav(&output.join(format!("{name}-{stimulus}-mix.wav")), &mixed)?;
            renders.push(json!({"name": name, "stimulus": stimulus, "sample_rate": RATE,
                "decay_seconds": settings.decay_seconds, "room_scale": settings.room_scale, "high_decay_ratio": settings.high_decay_ratio,
                "damping_hz": settings.damping_hz, "history_bytes": core.history_bytes(),
                "tank": options.tank, "diffused": options.diffused, "wrapper_settings": probe_core::describe(developed),
                "input_peak": dry.iter().map(|v| v.abs()).fold(0_f32, f32::max), "onset_seconds": 1., "duration_seconds": 20,
                "mix": {"dry": 0.8, "wet": 0.2}, "normalized": false}));
        }
    }
    let metadata = json!({"schema": 1, "prototype": if options.tank { "cross-coupled-tank" } else if options.diffused { "diffused-fdn" } else { "eight-line-fdn" }, "build_profile": if cfg!(debug_assertions) {"debug"} else {"release"},
        "commit": command("git", &["rev-parse", "HEAD"]), "rustc": command("rustc", &["--version"]),
        "source_dirty": command("git", &["status", "--porcelain"]).map(|s| !s.is_empty()),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "cpu": std::env::var("PROCESSOR_IDENTIFIER").ok(),
        "cpu_override": std::env::var("SUHINA_PROBE_CPU").ok(),
        "renders": renders, "listening_level_matched": false});
    std::fs::write(output.join("manifest.json"), serde_json::to_vec_pretty(&metadata)?)?;
    if options.render_only {
        println!("Wrote render-only probe to {}", output.display());
        return Ok(());
    }
    let mut timings = Vec::new();
    for rate in [48_000, 96_000] {
        for block in [64, 128] {
            for instances in [1, 8, 16] {
                for scenario in ["active", "silence", "tail"] {
                    timings.push(benchmark(rate, block, instances, scenario, &options)?);
                }
            }
        }
    }
    std::fs::write(
        output.join("cpu.json"),
        serde_json::to_vec_pretty(&json!({"metadata": metadata,
        "timing_scope": "whole group DSP calls; buffer fill, allocation, disk I/O excluded",
        "trials": 3, "seconds_per_trial": 1, "warmup_frames": 8192, "results": timings}))?,
    )?;
    println!("Wrote listening WAVs, manifest.json and cpu.json to {}", output.display());
    Ok(())
}

fn command(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn signal(kind: &str, rate: u32, seconds: usize) -> Vec<f32> {
    let mut result = vec![0.; rate as usize * seconds * 2];
    let mut seed = 0x51f15e_u32;
    for (frame, stereo) in result.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let t = frame as f64 / f64::from(rate) - 1.;
        let value = if t < 0. {
            0.
        } else {
            match kind {
                "impulse" => {
                    if frame == rate as usize {
                        0.25
                    } else {
                        0.
                    }
                }
                "percussion" if t < 0.3 => {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    0.25 * (f64::from(seed) / f64::from(u32::MAX) * 2. - 1.) * (-35. * t).exp()
                }
                "tone" if t < 1. => {
                    let env = (t * 100.).min(1.) * ((1. - t) * 100.).min(1.);
                    0.125
                        * env
                        * ((std::f64::consts::TAU * 220. * t).sin() + 0.5 * (std::f64::consts::TAU * 330. * t).sin())
                }
                _ => 0.,
            }
        } as f32;
        stereo.fill(value);
    }
    result
}

fn write_wav(path: &Path, samples: &[f32]) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for value in samples {
        writer.write_sample(*value)?;
    }
    writer.finalize()
}

fn benchmark(
    rate: u32,
    block: usize,
    instances: usize,
    scenario: &str,
    options: &Options,
) -> Result<Value, Box<dyn Error>> {
    // The tail case also exercises a larger room with stronger damping.
    let settings = Settings {
        sample_rate: rate,
        room_scale: if scenario == "tail" { 2. } else { 1. },
        decay_seconds: if scenario == "tail" { 6. } else { 2. },
        high_decay_ratio: if scenario == "tail" { 0.4 } else { 1. },
        ..Settings::default()
    };
    let developed = options.settings("benchmark", settings)?;
    let settings = developed.core;
    let mut times = Vec::new();
    let blocks = (rate as usize).div_ceil(block);
    let mut trial_percent = Vec::new();
    let mut history_bytes = 0;
    for _ in 0..3 {
        let mut cores = (0..instances)
            .map(|_| Core::new(developed, options.diffused, options.tank))
            .collect::<Result<Vec<_>, _>>()?;
        let mut buffers = vec![vec![0.; block * 2]; instances];
        history_bytes = cores.first().map_or(0, Core::history_bytes);
        for core in &mut cores {
            if scenario == "tail" {
                core.process_frame([0.25; 2]);
            }
            for _ in 0..8192 {
                black_box(core.process_frame([0.; 2]));
            }
        }
        let mut total = 0.;
        for k in 0..blocks {
            // Input buffer copies/fills and signal generation are outside the timed section.
            for buffer in &mut buffers {
                for (i, frame) in buffer.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                    let x = if scenario == "active" {
                        (std::f64::consts::TAU * 220. * (k * block + i) as f64 / f64::from(rate)).sin() as f32 * 0.1
                    } else {
                        0.
                    };
                    frame.fill(x);
                }
            }
            let start = Instant::now();
            for (core, buffer) in cores.iter_mut().zip(&mut buffers) {
                core.process(black_box(buffer));
            }
            let elapsed = start.elapsed().as_secs_f64();
            total += elapsed;
            times.push(elapsed);
            black_box(&buffers);
        }
        trial_percent.push(100. * total / (blocks * block) as f64 * f64::from(rate));
    }
    times.sort_by(f64::total_cmp);
    let percentile = |p: f64| times.get(((times.len() - 1) as f64 * p).ceil() as usize).copied().unwrap_or(0.);
    let deadline = block as f64 / f64::from(rate);
    let total: f64 = times.iter().sum();
    let group_percent = 100. * total / (3 * blocks * block) as f64 * f64::from(rate);
    let mut trial_sorted = trial_percent.clone();
    trial_sorted.sort_by(f64::total_cmp);
    let trial_median = trial_sorted.get(1).copied().unwrap_or(group_percent);
    Ok(json!({"sample_rate": rate, "block_frames": block, "instances": instances, "scenario": scenario,
        "decay_seconds": settings.decay_seconds, "room_scale": settings.room_scale,
        "high_decay_ratio": settings.high_decay_ratio, "damping_hz": settings.damping_hz,
        "history_bytes_per_instance": history_bytes, "trial_group_percent_one_core": trial_percent,
        "tank": options.tank, "diffused": options.diffused, "wrapper_settings": probe_core::describe(developed),
        "group_percent_one_core": group_percent, "per_instance_percent_one_core": group_percent / instances as f64,
        "median_trial_per_instance_percent_one_core": trial_median / instances as f64,
        "real_time_factor": group_percent / 100., "block_deadline_us": deadline * 1e6,
        "median_us": percentile(0.5) * 1e6, "p95_us": percentile(0.95) * 1e6,
        "p99_us": percentile(0.99) * 1e6, "max_us": percentile(1.) * 1e6,
        "p99_deadline_percent": 100. * percentile(0.99) / deadline,
        "max_deadline_percent": 100. * percentile(1.) / deadline,
        "throughput_gate_pass": trial_median / instances as f64 <= 1.,
        "group_p99_gate_pass": percentile(0.99) / deadline < 0.5,
        "history_gate_pass": history_bytes < 1024 * 1024,
        "warning": "observed timings on a shared machine; no hard real-time guarantee"}))
}
