// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Manual fixed-workload benchmark. Run in release mode; never used as a timing CI gate.
use std::{hint::black_box, sync::Arc, time::Instant};
use suhina_device_api::{Device, NoteEvent, ProcessContext};
use suhina_device_instrument::{Instrument, MAX_VOICES, param};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for block_frames in [64, 256] {
        for (source, name) in [(0.0, "Modal"), (1.0, "Sample"), (2.0, "Hybrid")] {
            for retrigger in [false, true] {
                let mut instrument = Instrument::with_rate(48000.0)?;
                let sample: Arc<[f32]> = (0..4800).map(|n| (n as f32 * 0.057).sin() * 0.1).collect();
                if let Some(player) = instrument.samples() {
                    player.load_sample(48000, 1, sample);
                }
                instrument.set_param(param::SOURCE, source);
                instrument.set_param(param::DECAY, 20.0);
                instrument.set_param(param::RELEASE, 10.0);
                instrument.set_param(param::BRIGHTNESS, 1.0);
                instrument.set_param(param::PLAYBACK, 2.0);
                instrument.reset();
                for key in 36..36 + MAX_VOICES as u8 {
                    instrument.event(NoteEvent::On { key, velocity: 1.0 });
                }
                let mut block = vec![0.0; block_frames * 2];
                let blocks = 48000 * 4 / block_frames;
                let mut durations = Vec::with_capacity(blocks);
                for iteration in 0..blocks {
                    let start = Instant::now();
                    // Dense release tails and sixteen new excitations per block deliberately
                    // stress the bounded allocator and coefficient setup as well as rendering.
                    if retrigger {
                        for key in 36..36 + MAX_VOICES as u8 {
                            instrument.event(NoteEvent::Off { key, velocity: 0.5 });
                            instrument.event(NoteEvent::On { key, velocity: 1.0 });
                        }
                    }
                    instrument.event(NoteEvent::PitchBend(if iteration % 2 == 0 { -1.0 } else { 1.0 }));
                    instrument.process(&ProcessContext::default(), black_box(&mut block));
                    durations.push(start.elapsed().as_secs_f64());
                }
                durations.sort_by(f64::total_cmp);
                let mean = durations.iter().sum::<f64>() / blocks as f64;
                let p99 = durations.get(blocks * 99 / 100).copied().unwrap_or_default();
                println!(
                    "{name} rate=48000 block={block_frames} voices={MAX_VOICES} modes=16 retrigger={retrigger} mean_us={:.2} p99_us={:.2} core_percent={:.2}",
                    mean * 1e6,
                    p99 * 1e6,
                    mean * 48000.0 / block_frames as f64 * 100.0
                );
            }
        }
    }
    println!("inline_core_bytes={} (excludes SDK prepared sample/warp allocations)", std::mem::size_of::<Instrument>());
    Ok(())
}
