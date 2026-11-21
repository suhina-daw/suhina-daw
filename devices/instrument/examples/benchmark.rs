// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Manual fixed-workload benchmark. Run in release mode; never used as a timing CI gate.
use std::{hint::black_box, sync::Arc, time::Instant};
use suhina_device_api::{Device, NoteEvent, ProcessContext};
use suhina_device_instrument::{Instrument, MAX_VOICES, param};

/// Times `blocks` blocks of `block_frames`, calling `events` before each, and prints one line.
fn measure(
    name: &str,
    instrument: &mut Instrument,
    block_frames: usize,
    mut events: impl FnMut(&mut Instrument, usize),
) -> (f64, f64) {
    let mut block = vec![0.0; block_frames * 2];
    let blocks = 48000 * 4 / block_frames;
    let mut durations = Vec::with_capacity(blocks);
    for iteration in 0..blocks {
        let start = Instant::now();
        events(instrument, iteration);
        instrument.process(&ProcessContext::default(), black_box(&mut block));
        durations.push(start.elapsed().as_secs_f64());
    }
    durations.sort_by(f64::total_cmp);
    let mean = durations.iter().sum::<f64>() / blocks as f64;
    let p99 = durations.get(blocks * 99 / 100).copied().unwrap_or_default();
    let deadline = block_frames as f64 / 48000.0;
    println!(
        "{name} rate=48000 block={block_frames} voices={MAX_VOICES} modes=16 mean_us={:.2} p99_us={:.2} core_percent={:.2} p99_of_deadline={:.1}%",
        mean * 1e6,
        p99 * 1e6,
        mean / deadline * 100.0,
        p99 / deadline * 100.0
    );
    (mean, p99)
}

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
                let label = format!("{name} retrigger={retrigger}");
                measure(&label, &mut instrument, block_frames, |instrument, iteration| {
                    // Dense release tails and sixteen new excitations per block deliberately
                    // stress the bounded allocator and coefficient setup as well as rendering.
                    if retrigger {
                        for key in 36..36 + MAX_VOICES as u8 {
                            instrument.event(NoteEvent::Off { key, velocity: 0.5 });
                            instrument.event(NoteEvent::On { key, velocity: 1.0 });
                        }
                    }
                    instrument.event(NoteEvent::PitchBend(if iteration % 2 == 0 { -1.0 } else { 1.0 }));
                });
            }
        }
        // E6: rolls on sixteen bars, each bar its own ringing body. "roll" strikes every bar
        // every 30 ms, staggered (a fast two-mallet roll on each); "every block" strikes all
        // sixteen bars in every block, so contacts run almost continuously. No motor exists yet.
        for (body, body_name) in [(1.0, "Marimba bar"), (2.0, "Vibraphone bar")] {
            for (model, model_name) in [(0.0, "Contact"), (1.0, "Pulse")] {
                for every_block in [false, true] {
                    let mut instrument = Instrument::with_rate(48000.0)?;
                    instrument.set_param(param::REVISION, 3.0);
                    instrument.set_param(param::BODY, body);
                    instrument.set_param(param::MALLET_MODEL, model);
                    instrument.set_param(param::HARDNESS, 1.0);
                    instrument.set_param(param::DECAY, 20.0);
                    instrument.set_param(param::BRIGHTNESS, 1.0);
                    instrument.reset();
                    let keys = 48..48 + MAX_VOICES as u8;
                    for key in keys.clone() {
                        instrument.event(NoteEvent::On { key, velocity: 1.0 });
                    }
                    let period = (0.030 * 48000.0) as usize;
                    let label = format!(
                        "{body_name} {model_name} {}",
                        if every_block { "strokes=every block" } else { "strokes=roll every 30 ms" }
                    );
                    let mut frame = 0usize;
                    measure(&label, &mut instrument, block_frames, |instrument, iteration| {
                        for (index, key) in keys.clone().enumerate() {
                            let offset = index * period / MAX_VOICES;
                            let due =
                                (frame + block_frames + period - offset) / period != (frame + period - offset) / period;
                            if every_block || due {
                                instrument.event(NoteEvent::On { key, velocity: 0.6 + 0.4 * (index % 2) as f32 });
                            }
                        }
                        instrument.event(NoteEvent::PitchBend(if iteration % 2 == 0 { -0.1 } else { 0.1 }));
                        frame += block_frames;
                    });
                }
            }
        }
    }
    println!("inline_core_bytes={} (excludes SDK prepared sample/warp allocations)", std::mem::size_of::<Instrument>());
    Ok(())
}
