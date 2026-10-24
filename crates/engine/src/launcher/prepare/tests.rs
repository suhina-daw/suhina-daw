// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launched stretched clips: a golden render of launches, a legato take-over and a relaunch, and
//! the voices' preparation off the audio thread (ADR 0003, decision 7).

use crate::{RenderInputs, RenderOptions};
use std::sync::Arc;

use suhina_model::*;

use super::super::tests::{clip, project, with};
use crate::fade_tests::{assert_golden_within, left, max_diff};
use crate::{AudioData, AudioPool, LaunchCommand, Plugins, Renderer, render_launches};

/// A low rate keeps the golden file small.
const SR: u32 = 8_000;

/// A sine of `hz` lasting two seconds, under `file`.
fn sine(pool: &mut AudioPool, file: &str, hz: f64) {
    let samples =
        (0..2 * SR as usize).map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(SR)).sin() as f32 * 0.5);
    pool.files.insert(file.into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples: samples.collect() }));
}

/// Content `id`: `file` warped so that two beats play `seconds` of it.
fn warped(id: u32, file: &str, seconds: f64) -> ClipContent {
    let region = AudioRegion {
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(2.0),
        warped: true,
        stretch: StretchMode::Smooth,
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 2.0, audio_time: seconds }],
        ..AudioRegion::new(file)
    };
    ClipContent {
        id: ContentId(id),
        name: file.into(),
        length: 2.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    }
}

/// One column at 240 bpm (a bar is a second): slot 1 loops a 440 Hz sine stretched to 0.9 times
/// its speed, slot 2 a legato 330 Hz sine at 1.25 times.
pub(super) fn two_stretched() -> (Project, AudioPool) {
    let mut pool = AudioPool::new();
    sine(&mut pool, "a.wav", 440.0);
    sine(&mut pool, "b.wav", 330.0);
    let legato = Clip { launch: LaunchSettings { legato: true, ..LaunchSettings::default() }, ..clip(2, 2.0, true) };
    let mut p = project(
        vec![warped(1, "a.wav", 0.45), warped(2, "b.wav", 0.625)],
        vec![vec![with(clip(1, 2.0, true)), with(legato)]],
    );
    p.transport.tempo_bpm = 240.0;
    (p, pool)
}

/// Slot 1 at once, slot 2 at the next bar (legato, so from where slot 1 was), slot 1 again
/// from its start at the bar after.
pub(super) const LAUNCHES: [(f64, LaunchCommand); 3] = [
    (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 }),
    (1.3, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
];

/// Seconds rendered.
pub(super) const SECONDS: f64 = 2.75;

/// Upward zero crossings per second in `left[from..to]` (seconds).
fn hz(left: &[f32], from: f64, to: f64) -> f64 {
    let (a, b) = ((from * f64::from(SR)) as usize, (to * f64::from(SR)) as usize);
    let crossings = left[a..b].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    crossings as f64 / (to - from)
}

#[test]
fn launched_stretched_clips_render_like_their_golden_file() {
    let (p, pool) = two_stretched();
    let out = left(&render_launches(&p, &pool, SR, &LAUNCHES, SECONDS).unwrap());
    // Each clip keeps its pitch: stretched, not re-pitched (396 Hz and 412.5 Hz).
    assert!((hz(&out, 0.2, 0.9) - 440.0).abs() < 8.0, "{} Hz", hz(&out, 0.2, 0.9));
    assert!((hz(&out, 1.2, 1.9) - 330.0).abs() < 8.0, "{} Hz", hz(&out, 1.2, 1.9));
    assert!((hz(&out, 2.2, 2.7) - 440.0).abs() < 8.0, "{} Hz", hz(&out, 2.2, 2.7));
    // The stretcher's floating-point maths differs in the last digits between platforms.
    assert_golden_within("launched-stretched", &out, 1e-4);
}

/// Renders `commands` (each at its time in seconds) with `renderer` for `seconds`, in blocks of
/// up to 512 frames that end where a command is due: the loop of [`render_launches`].
fn play(renderer: &mut Renderer, commands: &[(f64, LaunchCommand)], seconds: f64) -> Vec<f32> {
    let frame_of = |at: f64| (at * f64::from(SR)).round() as usize;
    let total = frame_of(seconds);
    let mut out = vec![0.0f32; total * 2];
    let mut done = 0;
    let mut pending = commands.iter().peekable();
    while done < total {
        while let Some((_, command)) = pending.next_if(|(at, _)| frame_of(*at) <= done) {
            renderer.handle(*command);
        }
        let next = pending.peek().map_or(total, |(at, _)| frame_of(*at));
        let end = next.clamp(done + 1, total).min(done + 512);
        renderer.process(&mut out[done * 2..end * 2]);
        done = end;
    }
    out
}

/// Whether each of the launcher's stretch voices is prepared, in slot order.
fn prepared(renderer: &Renderer) -> Vec<bool> {
    let slots = renderer.graph.launcher.columns.iter().flat_map(|c| &c.slots);
    let audio = slots.flat_map(|s| s.clip.iter().flat_map(|plan| &plan.audio));
    audio.filter_map(|a| a.stretch.as_ref()).map(|v| v.is_prepared()).collect()
}

/// `two_stretched` with slot 2's audio warped differently: an edit that changes one slot only.
fn second_slot_edited(p: &Project) -> Project {
    let mut edited = p.clone();
    if let Some(LaneData::Audio(region)) =
        edited.contents.get_mut(1).and_then(|c| c.lanes.first_mut()).map(|l| &mut l.data)
    {
        region.warp_markers[1].audio_time = 0.6;
    }
    edited
}

#[test]
fn building_or_rebuilding_a_graph_prepares_no_launcher_voice() {
    let (p, pool) = two_stretched();
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    assert_eq!(prepared(&renderer), [false, false], "nothing is built for slots that do not play");
    // An edit builds a new graph; it builds no stretcher either, on any thread, until a launch.
    let mut controller = renderer.controller();
    controller.update(&second_slot_edited(&p)).unwrap();
    renderer.process(&mut [0.0; 2 * 256]);
    assert_eq!(prepared(&renderer), [false, false]);
    assert_eq!(renderer.graph.launcher.voices.built(), 0);
}

#[test]
fn a_worker_thread_builds_the_voices_and_launches_sound_as_offline() {
    let (p, pool) = two_stretched();
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    let mut controller = renderer.controller();
    controller.run_stretch_worker(&mut renderer).unwrap();
    assert!(!renderer.graph.launcher.voices.inline(), "the thread took the preparer");
    // Stopped, the renderer takes the voices in as they arrive.
    let began = std::time::Instant::now();
    while prepared(&renderer) != [true, true] {
        assert!(began.elapsed() < std::time::Duration::from_secs(10), "the worker never built the voices");
        renderer.process(&mut [0.0; 2 * 256]);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let out = left(&play(&mut renderer, &LAUNCHES, SECONDS));
    let offline = left(&render_launches(&p, &pool, SR, &LAUNCHES, SECONDS).unwrap());
    assert_eq!(max_diff(&out, &offline), 0.0, "the same launches, sample for sample");
}

#[test]
fn a_launch_before_its_voice_is_ready_starts_on_time_and_its_stretched_audio_joins_where_the_clip_is() {
    let (p, pool) = two_stretched();
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    // A worker that has not got round to it yet: nothing builds the voice on the audio side.
    let mut preparer = renderer.graph.launcher.take_preparer().unwrap();
    let quarter = SR as usize / 4;
    let mut out = vec![0.0f32; 2 * 2 * quarter];
    let (silent, joined) = out.split_at_mut(2 * quarter);
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
    renderer.process(silent);
    assert!(silent.iter().all(|s| *s == 0.0), "the stretched audio waits for its voice");
    assert_eq!(renderer.graph.launcher.columns[0].playing.map(|p| p.slot), Some(0), "launched on time");
    // The launch asked for its voice first: the worker builds that one before slot 2's.
    assert!(preparer.work(true));
    assert!(!preparer.work(true), "only the voice asked for");
    renderer.process(joined);
    assert_eq!(prepared(&renderer), [true, false]);
    // It plays from where the clip is, a quarter of a second in: the offline render's frames
    // from there on, after the stretcher's start-up.
    let offline = left(&render_launches(&p, &pool, SR, &LAUNCHES[..1], 0.5).unwrap());
    let joined = left(joined);
    assert!((hz(&joined, 0.05, 0.25) - 440.0).abs() < 10.0, "{} Hz", hz(&joined, 0.05, 0.25));
    let level = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!((level(&joined[400..]) - level(&offline[quarter + 400..])).abs() < 0.05);
}

#[test]
fn a_swap_takes_over_the_voices_of_unchanged_slots_and_the_worker_builds_only_the_changed_one() {
    let (p, pool) = two_stretched();
    let mut old =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    let mut new = Renderer::build(
        &second_slot_edited(&p),
        RenderInputs::from_audio(&pool).with_plugins(Plugins::new()),
        SR,
        RenderOptions::STOPPED,
    )
    .unwrap();
    let (mut before, mut after) =
        (old.graph.launcher.take_preparer().unwrap(), new.graph.launcher.take_preparer().unwrap());
    while before.work(false) {}
    old.graph.launcher.receive_voices();
    assert_eq!(prepared(&old), [true, true]);
    // The new graph's preparer joins the thread before its graph is swapped in: slot 1's voice
    // may be taken over, so it waits; slot 2's audio changed and is built at once.
    after.follow(&before);
    while after.work(false) {}
    assert_eq!(new.graph.launcher.voices.built(), 1);
    new.graph.launcher.adopt(&mut old.graph.launcher, None);
    assert!(before.finished(), "the old graph is done with");
    // Swapped in: slot 1's voice was taken over, so there is nothing left to build.
    assert!(!after.work(false));
    new.graph.launcher.receive_voices();
    assert_eq!(prepared(&new), [true, true]);
    assert_eq!(new.graph.launcher.voices.built(), 1, "slot 1's stretcher was not built again");
}

#[test]
fn a_voice_the_worker_claimed_first_is_not_taken_over_and_arrives_from_the_worker() {
    let (p, pool) = two_stretched();
    let mut old =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    let mut new =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    let (mut before, mut after) =
        (old.graph.launcher.take_preparer().unwrap(), new.graph.launcher.take_preparer().unwrap());
    while before.work(false) {}
    old.graph.launcher.receive_voices();
    // Not told about the graph before, the worker builds both voices before the swap.
    while after.work(false) {}
    new.graph.launcher.adopt(&mut old.graph.launcher, None);
    // The swap left them to the worker: the old graph's go with it, these arrive.
    assert_eq!(prepared(&new), [false, false]);
    new.graph.launcher.receive_voices();
    assert_eq!(prepared(&new), [true, true]);
}

#[test]
fn a_scene_inserted_above_keeps_the_voices_with_their_clips() {
    let (p, pool) = two_stretched();
    let mut shifted = p.clone();
    shifted.launcher.scenes.insert(0, Scene::default());
    shifted.launcher.columns[0].slots.insert(0, Slot { clip: None, has_stop: true });
    let shift = crate::SceneShift::between(&p.launcher, &shifted.launcher);
    assert_eq!(shift, Some(crate::SceneShift::Inserted(0)));
    let mut old =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    let mut new = Renderer::build(
        &shifted,
        RenderInputs::from_audio(&pool).with_plugins(Plugins::new()),
        SR,
        RenderOptions::STOPPED,
    )
    .unwrap();
    let mut before = old.graph.launcher.take_preparer().unwrap();
    while before.work(false) {}
    old.graph.launcher.receive_voices();
    new.graph.launcher.adopt(&mut old.graph.launcher, shift);
    assert_eq!(prepared(&new), [true, true], "each clip's voice moved down a row with it");
}

#[test]
fn a_playing_clip_plays_on_through_its_own_voice_across_an_edit_of_another_slot() {
    let (p, pool) = two_stretched();
    let launch = [(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })];
    let reference = left(&render_launches(&p, &pool, SR, &launch, 1.0).unwrap());
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&pool).with_plugins(Plugins::new()), SR, RenderOptions::STOPPED)
            .unwrap();
    renderer.set_offline(true);
    let mut controller = renderer.controller();
    let mut out = play(&mut renderer, &launch, 0.5);
    // Edited while it plays: the swap takes the playing voice over instead of starting a new one.
    controller.update(&second_slot_edited(&p)).unwrap();
    out.extend(play(&mut renderer, &[], 0.5));
    assert_eq!(max_diff(&left(&out), &reference), 0.0, "no break at the swap");
}
