// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Offline renders through a tempo map (`docs/spec/tempo-map.md`): notes, re-pitched and
//! time-stretched audio, launched clips and the metronome stay on the grid across a step and
//! through a ramp; quantisation and the metronome follow time-signature changes; a project whose
//! changes come after everything it plays renders exactly as without them.

use crate::{RenderInputs, render_offline_from};
use std::sync::Arc;

use suhina_model::*;

use crate::{AudioData, AudioPool, LaunchCommand, Quantization, Renderer, render_launches, render_offline};

const SR: u32 = 48_000;

fn step(beat: f64, bpm: f64) -> TempoPoint {
    TempoPoint { beat, bpm, ramp: false }
}

fn ramp(beat: f64, bpm: f64) -> TempoPoint {
    TempoPoint { beat, bpm, ramp: true }
}

fn clip(start: f64, length: f64) -> Clip {
    Clip::new(ContentId(1), start, length)
}

/// One track whose clip of `length` beats at beat 0 plays `lane`, on the arrangement and in its
/// first launcher slot.
fn project(lane: LaneData, length: f64) -> Project {
    let audio = matches!(lane, LaneData::Audio(_));
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: !audio, audio },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel::new(ChannelId(1), "C")],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length,
            lanes: vec![ContentLane { target: None, data: lane }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(0.0, length)],
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            scenes: vec![Scene { id: Default::default(), name: "1".into(), tempo_bpm: None, time_signature: None }],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot {
                    clip: Some(Clip { loop_range: Some(LoopRange { start: 0.0, end: length }), ..clip(0.0, length) }),
                    has_stop: true,
                }],
            }],
            mappings: Vec::new(),
        },
        ..Project::default()
    }
}

/// A short note on every beat from 0 to `beats - 1`.
fn beat_notes(beats: u32) -> LaneData {
    LaneData::Notes((0..beats).map(|b| Note::new(f64::from(b), 0.1, 69, 1.0)).collect())
}

/// Frames where sound rises above `threshold` after at least `gap` frames at or below it.
fn onsets_above(out: &[f32], threshold: f32, gap: usize) -> Vec<usize> {
    let mut result = Vec::new();
    let mut quiet = usize::MAX;
    for (i, frame) in out.as_chunks::<2>().0.iter().enumerate() {
        let loud = frame[0].abs().max(frame[1].abs()) > threshold;
        if loud && quiet >= gap {
            result.push(i);
        }
        quiet = if loud { 0 } else { quiet.saturating_add(1) };
    }
    result
}

/// Sine notes start at phase zero: the first audible frame is a frame or two after note-on.
fn onsets(out: &[f32]) -> Vec<usize> {
    onsets_above(out, 1e-6, 100)
}

/// The frame of each of `beats` beats through `transport`'s map at 48 kHz.
fn expected(transport: &Transport, beats: u32) -> Vec<usize> {
    let map = transport.tempo_map(f64::from(SR));
    (0..beats).map(|b| map.time_at(f64::from(b)).round() as usize).collect()
}

fn assert_near(found: &[usize], want: &[usize], tolerance: usize) {
    assert_eq!(found.len(), want.len(), "found {found:?}, want {want:?}");
    for (f, w) in found.iter().zip(want) {
        assert!(f.abs_diff(*w) <= tolerance, "found {found:?}, want {want:?}");
    }
}

#[test]
fn notes_stay_on_the_grid_across_a_step() {
    // 120 bpm (24 000 frames a beat) to beat 4, then 60 bpm (48 000): beat 5 at 144 000.
    let mut p = project(beat_notes(8), 8.0);
    p.transport.tempo_changes = vec![step(4.0, 60.0)];
    let out = render_offline(&p, SR, 256).unwrap();
    assert_near(&onsets(&out), &[0, 24_000, 48_000, 72_000, 96_000, 144_000, 192_000, 240_000], 2);
}

#[test]
fn notes_stay_on_the_grid_through_a_ramp() {
    // From 90 bpm at beat 0 up to 180 at beat 8, then down to 100 at beat 12.
    let mut p = project(beat_notes(16), 16.0);
    p.transport.tempo_bpm = 90.0;
    p.transport.tempo_changes = vec![ramp(8.0, 180.0), ramp(12.0, 100.0)];
    let out = render_offline(&p, SR, 256).unwrap();
    assert_near(&onsets(&out), &expected(&p.transport, 16), 2);
    // The ramp is not a step: beat 4 comes between where 90 and 180 bpm would put it.
    let beat_four = expected(&p.transport, 5)[4];
    assert!(beat_four > 4 * 16_000 && beat_four < 4 * 32_000, "{beat_four}");
}

#[test]
fn changes_after_everything_change_nothing() {
    // Golden renders hold for a constant tempo; the map's arithmetic before its first change is
    // the same, so a change after the song leaves the render bit for bit as it was.
    let plain = project(beat_notes(8), 8.0);
    let mut later = plain.clone();
    // A step (a ramp into beat 500 would glide from beat 0 on).
    later.transport.tempo_changes = vec![step(500.0, 200.0)];
    later.transport.time_signature_changes =
        vec![TimeSignatureChange { bar: 200, time_signature: TimeSignature { numerator: 7, denominator: 8 } }];
    assert_eq!(render_offline(&later, SR, 256), render_offline(&plain, SR, 256));
}

/// Eight seconds of a click every half second (a beat at 120 bpm), each click 5 ms of a full-scale
/// square wave at 1 kHz, as one file at 48 kHz.
fn click_track() -> (AudioRegion, AudioPool) {
    let samples: Vec<f32> = (0..8 * SR as usize)
        .map(|i| {
            let into_beat = i % (SR as usize / 2);
            if into_beat < 240 { if (into_beat / 24).is_multiple_of(2) { 1.0 } else { -1.0 } } else { 0.0 }
        })
        .collect();
    let region = AudioRegion {
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(8.0),
        warped: true,
        stretch: StretchMode::Repitch,
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 16.0, audio_time: 8.0 }],
        ..AudioRegion::new("clicks.wav")
    };
    let mut pool = AudioPool::new();
    pool.files.insert("clicks.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
    (region, pool)
}

#[test]
fn a_repitched_warped_clip_stays_on_the_grid() {
    let (region, pool) = click_track();
    let mut p = project(LaneData::Audio(region), 16.0);
    p.transport.tempo_changes = vec![step(4.0, 80.0), ramp(12.0, 150.0)];
    let out = render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 256, None).unwrap();
    assert_near(&onsets_above(&out, 0.01, 2_000), &expected(&p.transport, 16), 2);
}

#[test]
fn a_stretched_warped_clip_stays_on_the_grid() {
    let (region, pool) = click_track();
    let region = AudioRegion { stretch: StretchMode::Rhythmic, ..region };
    let mut p = project(LaneData::Audio(region), 16.0);
    p.transport.tempo_changes = vec![step(4.0, 80.0), ramp(12.0, 150.0)];
    let out = render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 256, None).unwrap();
    // The stretcher smears a click over a few milliseconds; each one starts within 6 ms of its
    // beat, so none drifts.
    assert_near(&onsets_above(&out, 0.1, 4_000), &expected(&p.transport, 16), 288);
}

#[test]
fn an_unwarped_clip_plays_at_its_own_speed_from_its_beat() {
    let (region, pool) = click_track();
    let region = AudioRegion { warped: false, ..region };
    let mut p = project(LaneData::Audio(region), 16.0);
    p.arrangement.lanes[0].clips[0].start = 4.0;
    p.transport.tempo_changes = vec![step(2.0, 60.0)];
    let out = render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 256, None).unwrap();
    // It starts at beat 4 (1 s at 120, then 2 s at 60) and clicks every half second after.
    let found = onsets_above(&out, 0.01, 2_000);
    assert!(found.first().is_some_and(|f| f.abs_diff(144_000) <= 2), "{found:?}");
    let want: Vec<usize> = (0..found.len()).map(|i| 144_000 + i * 24_000).collect();
    assert_near(&found, &want, 2);
}

#[test]
fn launched_clips_follow_the_map() {
    // The clip launched at the start loops a note on every beat; the arrangement is not playing
    // it (the launch overrides the track), so every onset is the launcher's.
    let mut p = project(beat_notes(4), 4.0);
    p.transport.tempo_bpm = 100.0;
    p.transport.tempo_changes = vec![ramp(6.0, 160.0), step(9.0, 70.0)];
    let launch = [(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })];
    let map = p.transport.tempo_map(f64::from(SR));
    let seconds = map.time_at(12.0) / f64::from(SR) + 0.1;
    let out = render_launches(&p, &AudioPool::new(), SR, &launch, seconds).unwrap();
    assert_near(&onsets(&out), &expected(&p.transport, 13), 2);
}

#[test]
fn the_launcher_clock_keeps_to_the_arrangement_through_a_ramp() {
    let mut p = project(beat_notes(1), 1.0);
    p.transport.tempo_changes = vec![ramp(64.0, 240.0)];
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let monitor = renderer.monitor();
    renderer.handle(LaunchCommand::Play);
    let map = p.transport.tempo_map(f64::from(SR));
    let mut out = vec![0.0f32; 512 * 2];
    let mut played = 0u64;
    // Over a minute of a ramp the playhead's beat stays where the map puts the frame. The
    // monitor reports the beat at the start of the block just rendered.
    while played < 60 * u64::from(SR) {
        renderer.process(&mut out);
        let want = map.beat_at(played as f64);
        played += 512;
        assert!((monitor.beat() - want).abs() < 1e-6, "at frame {played}: {} vs {want}", monitor.beat());
    }
    assert!((monitor.tempo_bpm() - map.bpm_at(monitor.beat())).abs() < 0.01);
}

#[test]
fn the_metronome_marks_the_bars_of_the_meter() {
    // 4/4 for bar 1, then 3/4: the high clicks are on beats 0, 4 and 7.
    let mut p = Project::default();
    p.transport.time_signature_changes =
        vec![TimeSignatureChange { bar: 1, time_signature: TimeSignature { numerator: 3, denominator: 4 } }];
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
    renderer.handle(LaunchCommand::Play);
    let mut out = vec![0.0f32; 9 * 24_000 * 2];
    for block in out.chunks_mut(480 * 2) {
        renderer.process(block);
    }
    let hz = |from: usize| {
        let left: Vec<f32> = out[from * 2..(from + 2_000) * 2].iter().step_by(2).copied().collect();
        left.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f64 * f64::from(SR) / 2_000.0
    };
    let high: Vec<usize> = (0..9).filter(|b| hz(b * 24_000) > 1_300.0).collect();
    assert_eq!(high, [0, 4, 7]);
}

#[test]
fn bar_quantisation_waits_for_the_meter_s_next_bar() {
    // 4/4 for bar 1, then 7/8 bars of 3.5 beats: bar lines at 0, 4, 7.5, 11.
    let mut p = project(beat_notes(1), 1.0);
    p.transport.time_signature_changes =
        vec![TimeSignatureChange { bar: 1, time_signature: TimeSignature { numerator: 7, denominator: 8 } }];
    let at = |beat: f64| beat * 0.5;
    let commands = [
        (0.0, LaunchCommand::Play),
        (0.0, LaunchCommand::SetQuantization(Quantization::Bars(1))),
        // Launched at beat 4.5, inside the first 7/8 bar: it waits for beat 7.5.
        (at(4.5), LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    ];
    let out = render_launches(&p, &AudioPool::new(), SR, &commands, at(8.0)).unwrap();
    // Notes from the arrangement on beat 0, then the launched clip's on 7.5.
    assert_near(&onsets(&out), &[0, 180_000], 2);
}

#[test]
fn an_edit_to_the_map_while_playing_keeps_the_beat() {
    // Playing at 120 bpm; at frame 43 200 (beat 1.8) a step to 60 bpm at beat 1 arrives. The
    // playhead stays on beat 1.8, which is now 0.2 beats (0.2 s) from beat 2, and every beat
    // after takes a second.
    let p = project(beat_notes(5), 5.0);
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    renderer.handle(LaunchCommand::Play);
    let mut out = vec![0.0f32; 90 * 480 * 2];
    for block in out.chunks_mut(480 * 2) {
        renderer.process(block);
    }
    let mut edited = p.clone();
    edited.transport.tempo_changes = vec![step(1.0, 60.0)];
    controller.update(&edited).unwrap();
    let mut rest = vec![0.0f32; 300 * 480 * 2];
    for block in rest.chunks_mut(480 * 2) {
        renderer.process(block);
    }
    out.extend(rest);
    assert_near(&onsets(&out), &[0, 24_000, 52_800, 100_800, 148_800], 2);
}

#[test]
fn a_loop_with_a_tempo_change_inside_keeps_its_beats() {
    // 120 bpm to beat 4, then 60; the loop region is beats 0 to 6. Each pass is 4 beats of half a
    // second and 2 beats of a second (4 s); the wrap lands on the loop end's frame through the map.
    let mut p = project(beat_notes(8), 8.0);
    p.transport.tempo_changes = vec![step(4.0, 60.0)];
    p.transport.loop_range = Some(LoopRange { start: 0.0, end: 6.0 });
    p.transport.loop_enabled = true;
    let out = render_launches(&p, &AudioPool::new(), SR, &[(0.0, LaunchCommand::Play)], 8.1).unwrap();
    let pass = [0, 24_000, 48_000, 72_000, 96_000, 144_000];
    let want: Vec<usize> =
        (0..3).flat_map(|n| pass.iter().map(move |f| f + n * 192_000)).filter(|f| *f < 388_800).collect();
    assert_near(&onsets(&out), &want, 2);
}

/// A clip in the first slot that loops one note a beat, a scene with a tempo of 60 bpm, and a
/// tempo map at 120 bpm that changes far later (so the clock reads its beat from the map).
fn scene_over_map() -> Project {
    let mut p = project(beat_notes(1), 1.0);
    p.transport.tempo_changes = vec![step(64.0, 240.0)];
    if let Some(scene) = p.launcher.scenes.first_mut() {
        scene.tempo_bpm = Some(60.0);
    }
    p
}

#[test]
fn a_scene_tempo_overrides_the_map_until_follow_tempo_map() {
    // The scene's 60 bpm drives the launched clip (a note a second) although the map says 120.
    // transport.followTempoMap at 3.75 s puts the clock on the map at the playhead's beat: the
    // clip plays on from where it was, its next note a quarter beat later at 120 bpm.
    let p = scene_over_map();
    let commands = [(0.0, LaunchCommand::LaunchScene { scene: 0 }), (3.75, LaunchCommand::FollowTempoMap)];
    let out = render_launches(&p, &AudioPool::new(), SR, &commands, 6.0).unwrap();
    assert_near(&onsets(&out), &[0, 48_000, 96_000, 144_000, 186_000, 210_000, 234_000, 258_000, 282_000], 2);
}

#[test]
fn the_map_takes_over_again_on_follow_on_a_start_and_on_an_edit() {
    let p = scene_over_map();
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    let monitor = renderer.monitor();
    let mut out = vec![0.0f32; 480 * 2];
    let mut play = |renderer: &mut Renderer, blocks: usize| {
        for _ in 0..blocks {
            renderer.process(&mut out);
        }
    };
    let on_map = |monitor: &crate::LauncherMonitor| monitor.meter().follows_map;

    renderer.handle(LaunchCommand::LaunchScene { scene: 0 });
    play(&mut renderer, 200); // 2 s: beat 2 at 60 bpm, beat 4 on the map
    assert!(!on_map(&monitor));
    assert_eq!(monitor.tempo_bpm(), 60.0);
    assert!((monitor.beat() - 2.0).abs() < 0.02, "{}", monitor.beat());

    // The command: the map's tempo, and the clock at the playhead's beat.
    renderer.handle(LaunchCommand::FollowTempoMap);
    play(&mut renderer, 1);
    assert!(on_map(&monitor));
    assert_eq!(monitor.tempo_bpm(), 120.0);
    assert!((monitor.beat() - 4.0).abs() < 0.03, "{}", monitor.beat());

    // A stop keeps the scene's tempo; the next start plays the map again.
    renderer.handle(LaunchCommand::LaunchScene { scene: 0 });
    play(&mut renderer, 300);
    assert!(!on_map(&monitor));
    renderer.handle(LaunchCommand::Stop);
    play(&mut renderer, 1);
    assert_eq!(monitor.tempo_bpm(), 60.0);
    renderer.handle(LaunchCommand::Play);
    play(&mut renderer, 1);
    assert!(on_map(&monitor));
    assert_eq!(monitor.tempo_bpm(), 120.0);

    // An edit to the map while a scene's tempo plays takes the clock back too.
    renderer.handle(LaunchCommand::LaunchScene { scene: 0 });
    play(&mut renderer, 300);
    assert!(!on_map(&monitor));
    let mut edited = p.clone();
    edited.transport.tempo_changes = vec![step(64.0, 200.0)];
    controller.update(&edited).unwrap();
    play(&mut renderer, 2);
    assert!(on_map(&monitor));
    assert_eq!(monitor.tempo_bpm(), 120.0);
}
