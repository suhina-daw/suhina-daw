// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip's gain envelope in the renderer (ADR 0035, `docs/spec/clip-panel.md` › Gain envelope):
//! straight lines in dB between points, over the clip's gain and under its fades; the same in the
//! direct, the stretched and the launched reader, at any block size, and repeated by a loop.

use suhina_model::*;

use crate::fade_tests::{left, project, region};
use crate::{AudioData, AudioPool, LaunchCommand, RenderInputs, render_launches, render_offline_from};
use std::sync::Arc;

/// A low rate keeps the renders small.
const SR: u32 = 8_000;
/// Frames per beat at 120 bpm.
const BEAT: usize = SR as usize / 2;
/// The click guard at the clip's ends, in frames (2 ms).
const GUARD: usize = 16;

/// Two seconds of a constant 0.25, so the render shows the gain itself.
fn constant() -> AudioPool {
    let mut pool = AudioPool::new();
    pool.files
        .insert("dc.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples: vec![0.25; 16_000] }));
    pool
}

fn points(list: &[(f64, f64)]) -> Vec<GainPoint> {
    list.iter().map(|&(beat, db)| GainPoint::new(beat, db)).collect()
}

/// A 4-beat unwarped clip of the constant with `envelope` and `gain`, rendered offline in blocks of
/// `block`: the left channel.
fn render(envelope: &[(f64, f64)], gain: f64, block: usize) -> Vec<f32> {
    let pool = constant();
    let mut p = project(region("dc.wav", false, StretchMode::Repitch, 2.0), 4.0, 0.0, 0.0);
    let clip = &mut p.arrangement.lanes[0].clips[0];
    clip.audio.gain_envelope = points(envelope);
    clip.audio.gain = gain;
    left(
        &render_offline_from(&p, RenderInputs::from_audio(&pool, suhina_device_registry::factory()), SR, block, None)
            .unwrap(),
    )
}

/// `db` as a gain, written out apart from the model's.
fn gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

#[test]
fn the_envelope_is_straight_in_db_between_its_points() {
    let full = f64::from(render(&[], 1.0, 512)[BEAT]);
    // 0 dB at beat 1, -24 dB at beat 3: held before the first and after the last.
    let out = render(&[(1.0, 0.0), (3.0, -24.0)], 1.0, 512);
    assert_eq!(out.len(), 4 * BEAT);
    for (frame, sample) in out.iter().enumerate().skip(GUARD).take(4 * BEAT - 2 * GUARD) {
        let beat = frame as f64 / BEAT as f64;
        let db = if beat <= 1.0 {
            0.0
        } else if beat >= 3.0 {
            -24.0
        } else {
            -24.0 * (beat - 1.0) / 2.0
        };
        let want = full * gain(db);
        assert!((f64::from(*sample) - want).abs() < 1e-6, "frame {frame}: {sample}, expected {want}");
    }
}

/// The fader moves the whole envelope: with the gain at -6 dB every frame is 6 dB lower.
#[test]
fn the_envelope_rides_on_the_clip_gain() {
    let envelope = [(0.0, 0.0), (2.0, -12.0), (2.0, 6.0)];
    let at_unity = render(&envelope, 1.0, 512);
    let lower = render(&envelope, gain(-6.0), 512);
    for (frame, (a, b)) in at_unity.iter().zip(&lower).enumerate() {
        assert!((f64::from(*b) - f64::from(*a) * gain(-6.0)).abs() < 1e-6, "frame {frame}");
    }
    // Two points on beat 2 make a step up from -12 dB to +6 dB.
    let full = f64::from(render(&[], 1.0, 512)[BEAT]);
    assert!((f64::from(at_unity[2 * BEAT - 1]) - full * gain(-12.0)).abs() < 1e-3);
    assert!((f64::from(at_unity[2 * BEAT + 1]) - full * gain(6.0)).abs() < 1e-6);
}

#[test]
fn the_envelope_does_not_depend_on_the_block_size() {
    let envelope = [(0.5, -3.0), (1.25, -30.0), (3.5, 2.0)];
    let reference = render(&envelope, 0.8, 512);
    for block in [1, 64, 333, 4096] {
        assert_eq!(render(&envelope, 0.8, block), reference, "block {block}");
    }
}

/// A loop plays the same stretch of the envelope on every pass: the content's beats, not the
/// clip's.
#[test]
fn a_loop_repeats_the_envelope() {
    let pool = constant();
    let mut p = project(region("dc.wav", false, StretchMode::Repitch, 2.0), 8.0, 0.0, 0.0);
    let clip = &mut p.arrangement.lanes[0].clips[0];
    clip.loop_range = Some(LoopRange { start: 0.0, end: 2.0 });
    clip.audio.gain_envelope = points(&[(0.0, -20.0), (2.0, 0.0)]);
    let out = left(
        &render_offline_from(&p, RenderInputs::from_audio(&pool, suhina_device_registry::factory()), SR, 512, None)
            .unwrap(),
    );
    for pass in 1..4 {
        let at = |beat: f64| out[pass * 2 * BEAT + (beat * BEAT as f64) as usize];
        assert!((at(0.5) - out[BEAT / 2]).abs() < 1e-6, "pass {pass}");
        assert!((at(1.5) - out[3 * BEAT / 2]).abs() < 1e-6, "pass {pass}");
    }
}

/// A clip's start marker past its first point: the envelope is read where the clip plays from.
#[test]
fn the_envelope_follows_the_start_marker() {
    let pool = constant();
    let mut p = project(region("dc.wav", false, StretchMode::Repitch, 2.0), 2.0, 0.0, 0.0);
    let clip = &mut p.arrangement.lanes[0].clips[0];
    clip.offset = 2.0;
    clip.audio.gain_envelope = points(&[(0.0, 0.0), (4.0, -40.0)]);
    let out = left(
        &render_offline_from(&p, RenderInputs::from_audio(&pool, suhina_device_registry::factory()), SR, 512, None)
            .unwrap(),
    );
    let full = f64::from(render(&[], 1.0, 512)[BEAT]);
    // Clip beat 1 is content beat 3: -30 dB.
    assert!((f64::from(out[BEAT]) - full * gain(-30.0)).abs() < 1e-6, "{}", out[BEAT]);
}

/// A warped clip in a stretching mode goes through the stretcher; the envelope is the same gain
/// over what it plays.
#[test]
fn a_stretched_clip_has_the_same_envelope() {
    let samples = (0..16_000).map(|i| (std::f64::consts::TAU * 220.0 * i as f64 / f64::from(SR)).sin() as f32 * 0.5);
    let mut pool = AudioPool::new();
    pool.files
        .insert("sine.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples: samples.collect() }));
    let render = |envelope: &[(f64, f64)]| {
        let mut p = project(region("sine.wav", true, StretchMode::Smooth, 2.0), 4.0, 0.0, 0.0);
        p.arrangement.lanes[0].clips[0].audio.gain_envelope = points(envelope);
        left(
            &render_offline_from(&p, RenderInputs::from_audio(&pool, suhina_device_registry::factory()), SR, 512, None)
                .unwrap(),
        )
    };
    let plain = render(&[]);
    let shaped = render(&[(0.0, 0.0), (4.0, -20.0)]);
    assert_eq!(plain.len(), shaped.len());
    for (frame, (p, s)) in plain.iter().zip(&shaped).enumerate() {
        let want = f64::from(*p) * gain(-20.0 * frame as f64 / (4 * BEAT) as f64);
        assert!((f64::from(*s) - want).abs() < 1e-5, "frame {frame}: {s}, expected {want}");
    }
}

/// A launched clip is shaped as the same clip on the arrangement.
#[test]
fn a_launched_clip_has_the_same_envelope_as_an_arranged_one() {
    let pool = constant();
    let mut p = project(region("dc.wav", false, StretchMode::Repitch, 2.0), 4.0, 0.0, 0.0);
    let envelope = points(&[(0.0, -6.0), (3.0, 0.0)]);
    p.arrangement.lanes[0].clips[0].audio.gain_envelope = envelope.clone();
    if let Some(clip) = p.launcher.columns[0].slots[0].clip.as_mut() {
        clip.audio.gain_envelope = envelope;
    }
    let arranged = left(
        &render_offline_from(&p, RenderInputs::from_audio(&pool, suhina_device_registry::factory()), SR, 512, None)
            .unwrap(),
    );
    p.arrangement.lanes.clear();
    let commands = [
        (0.0, LaunchCommand::SetQuantization(crate::Quantization::None)),
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    ];
    let launched = left(&render_launches(&p, &pool, SR, &commands, 2.0, suhina_device_registry::factory()).unwrap());
    for frame in [GUARD, BEAT, 2 * BEAT, 3 * BEAT] {
        assert!((launched[frame] - arranged[frame]).abs() < 1e-6, "frame {frame}");
    }
}
