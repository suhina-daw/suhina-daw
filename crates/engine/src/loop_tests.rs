// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The arrangement's loop region (`docs/spec/arrangement-editing.md` › Loop region): playback
//! wraps from the loop end to the loop start on the exact frame, whatever the block size, and a
//! render of the whole arrangement does not repeat it.

use crate::{RenderInputs, render_offline_from};
use std::sync::Arc;

use suhina_model::*;

use crate::fade_tests::{assert_golden, left, max_diff, project, region};
use crate::{AudioData, AudioPool, LaunchCommand, Renderer, render_launches};

const SR: u32 = 8_000;
/// Frames per beat at 120 bpm.
const BEAT: usize = SR as usize / 2;

/// Two seconds (4 beats) of a ramp from 0 up, so every frame says where in the clip it is.
fn ramp() -> (AudioRegion, AudioPool) {
    let mut pool = AudioPool::new();
    let samples = (0..16_000).map(|i| i as f32 / 32_000.0).collect();
    pool.files.insert("ramp.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
    (region("ramp.wav", false, StretchMode::Repitch, 2.0), pool)
}

/// The ramp as a 4-beat arrangement clip, with the loop region `start`…`end` in beats.
fn looped(start: f64, end: f64, enabled: bool) -> (Project, AudioPool) {
    let (region, pool) = ramp();
    let mut project = project(region, 4.0, 0.0, 0.0);
    project.transport.loop_range = Some(LoopRange { start, end });
    project.transport.loop_enabled = enabled;
    (project, pool)
}

/// Plays `project` from the start for `frames` frames, `block` frames at a time.
fn play(project: &Project, pool: &AudioPool, frames: usize, block: usize) -> Vec<f32> {
    let mut renderer = Renderer::stopped(project, pool, SR).unwrap();
    renderer.set_offline(true);
    renderer.handle(LaunchCommand::Play);
    let mut out = vec![0.0f32; frames * 2];
    for chunk in out.chunks_mut(block * 2) {
        renderer.process(chunk);
    }
    left(&out)
}

#[test]
fn a_looped_span_renders_like_its_golden_file() {
    let (project, pool) = looped(1.0, 2.0, true);
    let render = left(&render_launches(&project, &pool, SR, &[(0.0, LaunchCommand::Play)], 3.0).unwrap());
    assert_golden("loop-1-2", &render);

    // Up to the loop end it is the clip as it plays without a loop; after it, each pass is the
    // clip from beat 1 to beat 2 again, from its first frame.
    let once = left(&render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 256, None).unwrap());
    assert_eq!(max_diff(&render[..2 * BEAT], &once[..2 * BEAT]), 0.0);
    for pass in 0..4 {
        let at = 2 * BEAT + pass * BEAT;
        let span = &render[at..at + BEAT];
        assert_eq!(max_diff(span, &once[BEAT..2 * BEAT]), 0.0, "pass {pass}");
    }
    assert!(render[2 * BEAT - 1] > 0.24 && render[2 * BEAT] < 0.126, "wraps on the exact frame");
}

#[test]
fn the_wrap_does_not_depend_on_the_block_size() {
    let (project, pool) = looped(0.5, 1.75, true);
    let reference = play(&project, &pool, 5 * BEAT, 1);
    for block in [7, 333, 512, 4_096] {
        assert_eq!(max_diff(&play(&project, &pool, 5 * BEAT, block), &reference), 0.0, "{block}-frame blocks");
    }
}

#[test]
fn looping_off_or_a_whole_render_plays_on_past_the_loop_end() {
    let (on, pool) = looped(1.0, 2.0, true);
    let (off, _) = looped(1.0, 2.0, false);
    let once = left(&render_offline_from(&on, RenderInputs::from_audio(&pool), SR, 256, None).unwrap());
    assert_eq!(once.len(), 4 * BEAT, "the whole arrangement, once");
    assert_eq!(max_diff(&play(&off, &pool, 4 * BEAT, 333), &once), 0.0);
    assert_eq!(
        max_diff(&left(&render_offline_from(&off, RenderInputs::from_audio(&pool), SR, 256, None).unwrap()), &once),
        0.0
    );
}

#[test]
fn a_playhead_moved_past_the_loop_end_plays_on() {
    let (project, pool) = looped(1.0, 2.0, true);
    let once = left(&render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 256, None).unwrap());
    let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
    renderer.set_offline(true);
    renderer.handle(LaunchCommand::SetPosition { beat: 2.5 });
    renderer.handle(LaunchCommand::Play);
    let mut out = vec![0.0f32; BEAT * 2];
    renderer.process(&mut out);
    let half = BEAT / 2;
    // Started mid-clip, the first frames have the click guard; after it, the clip from 2.5 on.
    assert_eq!(max_diff(&left(&out)[half..], &once[5 * half + half..6 * half + half]), 0.0);
}

#[test]
fn the_playhead_reads_inside_the_loop_and_stop_returns_to_the_play_start_marker() {
    let (project, pool) = looped(1.0, 2.0, true);
    let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
    let monitor = renderer.monitor();
    renderer.handle(LaunchCommand::SetPosition { beat: 0.5 });
    renderer.handle(LaunchCommand::Play);
    let mut out = vec![0.0f32; 256 * 2];
    // 3.25 beats in: the loop has wrapped twice (at 2 beats played twice), the playhead is at 1.75.
    for _ in 0..(13 * BEAT / 4 / 256) {
        renderer.process(&mut out);
    }
    let beat = monitor.beat();
    assert!((1.0..2.0).contains(&beat), "{beat}");
    renderer.handle(LaunchCommand::Stop);
    assert_eq!((monitor.start_beat(), renderer.running()), (0.5, false));
}
