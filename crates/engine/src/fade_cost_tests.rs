// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What clip fades cost the audio thread (issue #227): a fade-heavy project rendered block by
//! block with each fade shape at curve 0 and ±1, against the same project without fades. The
//! numbers are in `docs/spec/arrangement-editing.md` › What fades cost the audio thread.
//! Timing depends on the machine, so these only report; run them in release:
//!
//! ```sh
//! cargo test --release -p suhina-engine fade_cost -- --ignored --nocapture --test-threads=1
//! ```

use crate::{RenderInputs, RenderOptions};
use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use suhina_model::*;

use crate::audio::Fades;
use crate::{AudioData, AudioPool, Renderer};

const SR: u32 = 48_000;
const BLOCK: usize = 512;
/// Audio tracks, each playing one clip after another.
const TRACKS: u32 = 64;
/// Clips per track, `CLIP_BEATS` long, back to back.
const CLIPS: u32 = 16;
const CLIP_BEATS: f64 = 4.0;
/// Rounds of the whole render per case; the fastest is reported, as the least disturbed.
const ROUNDS: usize = 3;

/// `TRACKS` audio tracks of `CLIPS` clips each, every clip faded in over its first half and out
/// over its second, so every frame of every clip is inside a fade.
fn project(fades: Option<(FadeShape, f64)>) -> Project {
    let fade = fades.map_or(0.0, |_| CLIP_BEATS / 2.0);
    let curve = fades.map_or(FadeCurve::default(), |(shape, curve)| FadeCurve { shape, curve });
    let clip = |i: u32| Clip {
        fade_in: fade,
        fade_out: fade,
        fade_in_curve: curve,
        fade_out_curve: curve,
        ..Clip::new(ContentId(1), f64::from(i) * CLIP_BEATS, CLIP_BEATS)
    };
    let region = AudioRegion {
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(2.0),
        stretch: StretchMode::Repitch,
        ..AudioRegion::new("noise.wav")
    };
    Project {
        tracks: (1..=TRACKS)
            .map(|t| Track {
                content: ContentKinds { notes: false, audio: true },
                channel: Some(ChannelId(t)),
                ..Track::new(TrackId(t), format!("Audio {t}"))
            })
            .collect(),
        channels: (1..=TRACKS)
            .map(|t| Channel { volume: 0.1, ..Channel::new(ChannelId(t), format!("Audio {t}")) })
            .collect(),
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "noise".into(),
            length: CLIP_BEATS,
            lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: (1..=TRACKS)
                .map(|t| ArrangementLane {
                    id: Default::default(),
                    name: format!("Audio {t}"),
                    track: Some(TrackId(t)),
                    clips: (0..CLIPS).map(clip).collect(),
                })
                .collect(),
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

/// Two seconds of noise at `SR`, as long as a clip at 120 bpm.
fn pool() -> AudioPool {
    let mut state = 0x2545_f491_u32;
    let samples = (0..2 * SR as usize)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (f64::from(state) / f64::from(u32::MAX) - 0.5) as f32
        })
        .collect();
    let mut pool = AudioPool::new();
    pool.files.insert("noise.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
    pool
}

struct Timings {
    mean: Duration,
    p99: Duration,
    worst: Duration,
}

/// Renders `project` to its end in `BLOCK`-frame blocks and times each `process` call.
fn time_blocks(project: &Project, pool: &AudioPool) -> Timings {
    let mut best: Option<Timings> = None;
    for _ in 0..ROUNDS {
        let Ok(mut renderer) = Renderer::build(project, RenderInputs::from_audio(pool), SR, RenderOptions::PLAYING)
        else {
            panic!("the project renders");
        };
        let blocks = renderer.length().div_ceil(BLOCK as u64) as usize;
        let mut out = vec![0.0f32; BLOCK * 2];
        let mut times = Vec::with_capacity(blocks);
        for _ in 0..blocks {
            let start = Instant::now();
            renderer.process(&mut out);
            times.push(start.elapsed());
            black_box(&out);
        }
        times.sort();
        let total: Duration = times.iter().sum();
        let timings = Timings {
            mean: total / times.len().max(1) as u32,
            p99: times[times.len() * 99 / 100],
            worst: times[times.len() - 1],
        };
        if best.as_ref().is_none_or(|b| timings.mean < b.mean) {
            best = Some(timings);
        }
    }
    best.unwrap_or(Timings { mean: Duration::ZERO, p99: Duration::ZERO, worst: Duration::ZERO })
}

const CASES: [(&str, Option<(FadeShape, f64)>); 11] = [
    ("no fades", None),
    ("linear, curve 0", Some((FadeShape::Linear, 0.0))),
    ("linear, curve +1", Some((FadeShape::Linear, 1.0))),
    ("linear, curve -1", Some((FadeShape::Linear, -1.0))),
    ("equal power, curve 0", Some((FadeShape::EqualPower, 0.0))),
    ("equal power, curve +1", Some((FadeShape::EqualPower, 1.0))),
    ("equal power, curve -1", Some((FadeShape::EqualPower, -1.0))),
    ("s-curve, curve 0", Some((FadeShape::SCurve, 0.0))),
    ("s-curve, curve +1", Some((FadeShape::SCurve, 1.0))),
    ("fast, curve +0.5", Some((FadeShape::Fast, 0.5))),
    ("slow, curve -0.5", Some((FadeShape::Slow, -0.5))),
];

#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
fn fade_cost_per_block() {
    let pool = pool();
    let budget = Duration::from_secs_f64(BLOCK as f64 / f64::from(SR));
    println!(
        "{TRACKS} tracks x {CLIPS} clips, every frame inside a fade; {BLOCK}-frame blocks at {SR} Hz, budget {:.2} ms",
        budget.as_secs_f64() * 1e3
    );
    println!("| Case | Mean block | Load | 99th percentile | Worst |");
    println!("|---|---|---|---|---|");
    for (name, fades) in CASES {
        let t = time_blocks(&project(fades), &pool);
        println!(
            "| {name} | {:.1} µs | {:.1} % | {:.1} µs | {:.1} µs |",
            t.mean.as_secs_f64() * 1e6,
            100.0 * t.mean.as_secs_f64() / budget.as_secs_f64(),
            t.p99.as_secs_f64() * 1e6,
            t.worst.as_secs_f64() * 1e6,
        );
    }
}

/// `Fades::gain` alone, per frame inside a fade: what the shape and curve cost without the rest
/// of the renderer around them.
#[test]
#[ignore = "a measurement; run in release with --ignored --nocapture"]
fn fade_cost_per_frame() {
    const FRAMES: usize = 4_000_000;
    let frames_per_beat = f64::from(SR) / 2.0;
    let length = FRAMES as f64;
    println!("| Case | ns per frame |");
    println!("|---|---|");
    for (name, fades) in CASES {
        let project = project(fades);
        let Some(clip) = project.arrangement.lanes.first().and_then(|l| l.clips.first()) else {
            panic!("the project has a clip");
        };
        // The clip's fades stretched over the whole run, so every frame is inside one.
        let whole = if clip.fade_in > 0.0 { length / frames_per_beat } else { 0.0 };
        let clip = Clip { length: 2.0 * length / frames_per_beat, fade_in: whole, fade_out: whole, ..clip.clone() };
        let fades = Fades::new(&clip, frames_per_beat, SR);
        let mut best = Duration::MAX;
        for _ in 0..ROUNDS {
            let start = Instant::now();
            let mut sum = 0.0f32;
            for frame in 0..FRAMES {
                let since_start = black_box(frame as f64);
                sum += fades.gain(since_start, 2.0 * length - since_start);
            }
            black_box(sum);
            best = best.min(start.elapsed());
        }
        println!("| {name} | {:.2} |", best.as_secs_f64() * 1e9 / FRAMES as f64);
    }
}
