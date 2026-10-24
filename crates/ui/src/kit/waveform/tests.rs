// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::*;

const RATE: u32 = 8000;

/// Four seconds of stereo: a decaying 110 Hz sine on the left, a square wave at a quarter of full
/// scale on the right for the first half and silence after.
fn stereo() -> Arc<AudioData> {
    let mut samples = Vec::new();
    for frame in 0..RATE * 4 {
        let t = f64::from(frame) / f64::from(RATE);
        let left = ((t * 110.0 * std::f64::consts::TAU).sin() * (-t * 0.8).exp()) as f32;
        let right = if t < 2.0 { if (frame / 40) % 2 == 0 { 0.25 } else { -0.25 } } else { 0.0 };
        samples.push(left);
        samples.push(right);
    }
    Arc::new(AudioData { sample_rate: RATE, channels: 2, samples })
}

#[test]
fn levels_summarise_each_channel() {
    let data = stereo();
    let p = peaks(&data);
    assert_eq!((p.frames, p.lanes, p.sample_rate), (32_000, 2, RATE));
    assert_eq!(p.levels.len(), LEVELS.len());
    for level in &p.levels {
        assert_eq!(level.buckets.len(), 32_000_usize.div_ceil(level.frames) * 2, "{}", level.frames);
    }
    // A stretch of only the positive half: its lowest sample is not 0.
    let up = p.column(Lane::Channel(1), 0.0, 10.0).unwrap();
    assert_eq!((up.min, up.max), (0.25, 0.25));
    // The square wave on the right, read at every resolution.
    for width in [100.0, 300.0, 3000.0, 16_000.0] {
        let b = p.column(Lane::Channel(1), 0.0, width).unwrap();
        assert_eq!((b.min, b.max), (-0.25, 0.25), "{width}");
        // Coarse levels include the silence after the square wave in their last bucket.
        assert!((b.rms - 0.25).abs() < 5e-3, "{width}: {}", b.rms);
    }
    // Silence after two seconds; the left is loud at the start.
    let quiet = p.column(Lane::Channel(1), 20_000.0, 30_000.0).unwrap();
    assert_eq!((quiet.min, quiet.max, quiet.rms), (0.0, 0.0, 0.0));
    let loud = p.column(Lane::Channel(0), 0.0, 400.0).unwrap();
    assert!(loud.max > 0.95 && loud.min < -0.95 && loud.rms > 0.6);
    // Mixed: both lanes together.
    let both = p.column(Lane::Mixed, 0.0, 400.0).unwrap();
    assert_eq!((both.min, both.max), (loud.min, loud.max));
    // Outside the file, or nonsense.
    assert_eq!(p.column(Lane::Mixed, 40_000.0, 50_000.0), None);
    assert_eq!(p.column(Lane::Mixed, -100.0, -1.0), None);
    assert_eq!(p.column(Lane::Mixed, f64::NAN, 5.0), None);
    assert_eq!(p.column(Lane::Channel(2), 0.0, 5.0), None);
    // A stretch running off either end reads what is inside.
    assert!(p.column(Lane::Channel(1), -500.0, 100.0).is_some_and(|b| b.max == 0.25));
    // The old reading of the range in seconds.
    assert_eq!(p.range(2.5, 3.0).0, p.column(Lane::Mixed, 20_000.0, 24_000.0).unwrap().min);
    assert_eq!(p.range(10.0, 11.0), (0.0, 0.0));
}

#[test]
fn samples_and_odd_files() {
    let p = peaks(&stereo());
    assert_eq!(p.sample(Lane::Channel(1), 0), Some(0.25));
    assert_eq!(p.sample(Lane::Mixed, 0), Some(0.125));
    assert_eq!(p.sample(Lane::Mixed, 40_000), None);
    // Without the samples (summarised only), close-up reads use the finest level.
    let summary = summarise(&stereo());
    assert_eq!(summary.sample(Lane::Mixed, 0), None);
    assert!(summary.column(Lane::Channel(1), 0.0, 10.0).is_some_and(|b| b.max == 0.25));
    // Empty, mono, many channels, not-a-number samples: no panic, sensible lanes.
    let empty = peaks(&Arc::new(AudioData { sample_rate: 44_100, channels: 2, samples: Vec::new() }));
    assert_eq!(empty.column(Lane::Mixed, 0.0, 10.0), None);
    let mono = peaks(&Arc::new(AudioData { sample_rate: 44_100, channels: 1, samples: vec![0.5, -0.5, f32::NAN] }));
    assert_eq!(mono.lanes, 1);
    assert_eq!(mono.column(Lane::Mixed, 0.0, 3.0).map(|b| (b.min, b.max)), Some((-0.5, 0.5)));
    let six = peaks(&Arc::new(AudioData { sample_rate: 48_000, channels: 6, samples: vec![0.1; 600] }));
    assert_eq!((six.lanes, six.frames), (2, 100));
    let none = peaks(&Arc::new(AudioData { sample_rate: 0, channels: 0, samples: vec![1.0; 10] }));
    assert_eq!(none.lanes, 1);
}

#[test]
fn the_db_scale_shows_quiet_parts() {
    assert_eq!(Scale::Linear.apply(0.5), 0.5);
    assert_eq!(Scale::Linear.apply(3.0), 1.0);
    assert_eq!(Scale::Db.apply(1.0), 1.0);
    assert_eq!(Scale::Db.apply(-1.0), -1.0);
    assert_eq!(Scale::Db.apply(0.0), 0.0);
    // -24 dB is half way, -48 dB and quieter the middle.
    assert!((Scale::Db.apply(10f32.powf(-24.0 / 20.0)) - 0.5).abs() < 1e-4);
    assert_eq!(Scale::Db.apply(0.001), 0.0);
    assert_eq!(Scale::Db.apply(f32::NAN), 0.0);
}

/// Writes `geometry` as text: per lane its middle, then each column (every `step`th) or sample.
fn describe(geometry: &Geometry, step: usize) -> String {
    let mut out = String::new();
    for (middle, shape) in &geometry.lanes {
        let _ = writeln!(out, "lane {middle:.1}");
        match shape {
            LaneShape::Envelope(runs) => {
                for run in runs {
                    let _ = writeln!(out, " run {}", run.len());
                    for s in run.iter().step_by(step) {
                        let _ = writeln!(
                            out,
                            "  {:.1} {:.1} {:.1} {:.1} {:.1}",
                            s.x, s.top, s.bottom, s.rms_top, s.rms_bottom
                        );
                    }
                }
            }
            LaneShape::Samples { points, dots } => {
                let _ = writeln!(out, " samples {} dots {dots}", points.len());
                for p in points.iter().step_by(step) {
                    let _ = writeln!(out, "  {:.1} {:.1}", p.x, p.y);
                }
            }
        }
    }
    out
}

/// Compares `actual` with `tests/golden/<name>.txt`; `SUHINA_BLESS=1` writes it instead.
fn golden(name: &str, actual: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(format!("{name}.txt"));
    suhina_test_support::assert_golden_text(&path, actual);
}

/// The waveform at several zooms, as geometry: the whole file, a coarse level, between levels and
/// the samples, close up with and without dots, tall (two lanes) and short (mixed), linear and in
/// decibels.
#[test]
fn snapshots_at_several_zooms() {
    let p = peaks(&stereo());
    let seconds = |frames: f64| frames / f64::from(RATE);
    let cases: [(&str, f32, usize, f64, f64, Scale, usize); 6] = [
        // name, height, columns, from (frames), frames per column, scale, every n-th written
        ("waveform-whole-tall", 60.0, 100, 0.0, 320.0, Scale::Linear, 5),
        ("waveform-coarse-short", 30.0, 12, 0.0, 2500.0, Scale::Linear, 1),
        ("waveform-close-tall", 60.0, 40, 15_900.0, 10.0, Scale::Linear, 2),
        ("waveform-samples", 30.0, 40, 100.0, 0.25, Scale::Linear, 1),
        ("waveform-sample-dots", 30.0, 30, 100.0, 0.1, Scale::Linear, 1),
        ("waveform-whole-db", 30.0, 100, 0.0, 320.0, Scale::Db, 5),
    ];
    for (name, height, count, from, per_column, scale, step) in cases {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(count as f32, height));
        let columns = steady_columns(0.0, count, seconds(from), seconds(per_column), 1.0);
        let shape = geometry(&p, rect, &columns, scale);
        golden(name, &describe(&shape, step));
    }
}

#[test]
fn geometry_follows_the_zoom() {
    let p = peaks(&stereo());
    let rect = |h| Rect::from_min_size(Pos2::ZERO, egui::vec2(50.0, h));
    let at = |per_column: f64| steady_columns(0.0, 50, 0.0, per_column / f64::from(RATE), 1.0);
    let tall = geometry(&p, rect(60.0), &at(300.0), Scale::Linear);
    assert_eq!(tall.lanes.len(), 2);
    assert!(matches!(tall.lanes[0].1, LaneShape::Envelope(_)));
    assert_eq!(geometry(&p, rect(30.0), &at(300.0), Scale::Linear).lanes.len(), 1);
    let close = geometry(&p, rect(30.0), &at(0.5), Scale::Linear);
    assert!(matches!(&close.lanes[0].1, LaneShape::Samples { points, dots: false } if points.len() == 25));
    let closer = geometry(&p, rect(30.0), &at(0.1), Scale::Linear);
    assert!(matches!(&closer.lanes[0].1, LaneShape::Samples { dots: true, .. }));
    // Past the end, the run stops; gain scales it; silence is a point tall.
    let past = steady_columns(0.0, 50, 3.9, 0.01, 1.0);
    let LaneShape::Envelope(runs) = &geometry(&p, rect(30.0), &past, Scale::Linear).lanes[0].1 else { panic!() };
    assert_eq!(runs.iter().map(Vec::len).sum::<usize>(), 10);
    let quiet = steady_columns(0.0, 10, 3.0, 0.01, 0.0);
    let LaneShape::Envelope(runs) = &geometry(&p, rect(30.0), &quiet, Scale::Linear).lanes[0].1 else { panic!() };
    assert!(runs[0].iter().all(|s| (s.bottom - s.top - 1.0).abs() < 1e-4));
    // Every span is ordered: top ≤ RMS top ≤ RMS bottom ≤ bottom, inside the lane.
    let LaneShape::Envelope(runs) = &tall.lanes[0].1 else { panic!() };
    for s in runs.iter().flatten() {
        assert!(s.top <= s.rms_top && s.rms_top <= s.rms_bottom && s.rms_bottom <= s.bottom, "{s:?}");
        assert!(s.top >= 0.0 && s.bottom <= 30.0, "{s:?}");
    }
}

#[test]
fn painting_makes_meshes_and_lines() {
    let ctx = egui::Context::default();
    let p = peaks(&stereo());
    let out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), egui::vec2(100.0, 60.0));
        let geometry = geometry(&p, rect, &steady_columns(10.0, 100, 0.0, 0.04, 1.0), Scale::Linear);
        paint(ui.painter(), &geometry, Colors::from(Color32::LIGHT_BLUE));
    });
    let shapes = out.shapes.len();
    let mut textures = out.textures_delta;
    textures.clear();
    // Two lanes, each a peak band, an RMS band and two edges.
    assert!(shapes >= 8, "{shapes}");
}

/// A ten-minute stereo file at 44.1 kHz, drawn 2000 columns wide at every zoom from the whole
/// file to single samples, stays inside a frame (16.7 ms at 60 frames a second).
#[test]
fn a_ten_minute_file_draws_within_a_frame_budget() {
    let rate = 44_100u32;
    let frames = rate as usize * 600;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut x = 0x2545_f491_u32;
    for _ in 0..frames {
        // A cheap, steady noise: xorshift.
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let v = (x as f32 / u32::MAX as f32) * 2.0 - 1.0;
        samples.push(v * 0.8);
        samples.push(-v * 0.5);
    }
    let data = Arc::new(AudioData { sample_rate: rate, channels: 2, samples });
    let started = Instant::now();
    let p = peaks(&data);
    let summarised = started.elapsed();

    let ctx = egui::Context::default();
    // The first frame builds the font atlas; a drawn frame is what is measured.
    let mut textures = ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta;
    textures.clear();
    let width = 2000usize;
    let mut worst = Duration::ZERO;
    let file_seconds = frames as f64 / f64::from(rate);
    for frames_per_column in [frames as f64 / width as f64, 100_000.0, 4096.0, 1000.0, 255.0, 64.0, 2.0, 0.5, 0.05] {
        let per_column = frames_per_column / f64::from(rate);
        let from = (file_seconds / 2.0 - per_column * width as f64 / 2.0).max(0.0);
        for height in [30.0, 80.0] {
            let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(width as f32, height));
            // The best of a few frames, so other tests running alongside do not count.
            let mut took = Duration::MAX;
            let (mut laid, mut painted) = (Duration::ZERO, Duration::ZERO);
            for _ in 0..5 {
                let begun = Instant::now();
                let columns = steady_columns(0.0, width, from, per_column, 1.0);
                let shape = geometry(&p, rect, &columns, Scale::Db);
                let after_geometry = begun.elapsed();
                // Painted and turned into triangles, as a frame does.
                let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    paint(ui.painter(), &shape, Colors::from(Color32::WHITE));
                });
                let after_paint = begun.elapsed();
                let _meshes = ctx.tessellate(out.shapes, 1.0);
                let mut textures = out.textures_delta;
                textures.clear();
                if begun.elapsed() < took {
                    took = begun.elapsed();
                    (laid, painted) = (after_geometry, after_paint - after_geometry);
                }
            }
            eprintln!(
                "{frames_per_column:>12.2} frames a column, {height} high: {took:?} (geometry {laid:?}, painting {painted:?})"
            );
            worst = worst.max(took);
        }
    }
    eprintln!("ten minutes: summarised in {summarised:?}, the slowest draw took {worst:?}");
    // A frame at 60 frames a second; on the machine this was written on the slowest draw took
    // about 6 ms in the dev profile, under 2 ms in release.
    assert!(worst < Duration::from_micros(16_667), "the slowest draw took {worst:?}");
}
