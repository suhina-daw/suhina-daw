// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clip fades in the renderer (`docs/spec/arrangement-editing.md` › Fades): golden renders of a
//! fade-in and a fade-out, the same envelope on stretched clips, block-size independence, and
//! launched clips (`docs/spec/clip-launching.md` › Fades).

use crate::{RenderInputs, render_offline_from};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use suhina_model::*;

use crate::{AudioData, AudioPool, LaunchCommand, render_launches};

/// A low rate keeps the golden files small.
const SR: u32 = 8_000;
/// Frames per beat at 120 bpm.
const BEAT: usize = SR as usize / 2;
/// The click guard at the clip's ends, in frames (2 ms).
const GUARD: f64 = 0.002 * SR as f64;

/// One audio track playing `file` from an arrangement clip of `length` beats with the given
/// fades, and the same clip in its launcher slot.
pub(crate) fn project(region: AudioRegion, length: f64, fade_in: f64, fade_out: f64) -> Project {
    let clip = Clip { fade_in, fade_out, ..Clip::new(ContentId(1), 0.0, length) };
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: false, audio: true },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Audio")
        }],
        channels: vec![Channel::new(ChannelId(1), "Audio")],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length,
            lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Audio".into(),
                track: Some(TrackId(1)),
                clips: vec![clip.clone()],
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            mappings: Vec::new(),
            scenes: vec![Scene::default()],
            columns: vec![LauncherColumn { track: TrackId(1), slots: vec![Slot { clip: Some(clip), has_stop: true }] }],
        },
        ..Project::default()
    }
}

pub(crate) fn region(file: &str, warped: bool, stretch: StretchMode, seconds: f64) -> AudioRegion {
    AudioRegion {
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(seconds),
        warped,
        stretch,
        warp_markers: vec![
            WarpMarker { time: 0.0, audio_time: 0.0 },
            WarpMarker { time: seconds * 2.0, audio_time: seconds },
        ],
        ..AudioRegion::new(file)
    }
}

/// Two seconds of a constant 0.25, so the render shows the envelope itself.
fn constant() -> (AudioRegion, AudioPool) {
    let mut pool = AudioPool::new();
    pool.files
        .insert("dc.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples: vec![0.25; 16_000] }));
    (region("dc.wav", false, StretchMode::Repitch, 2.0), pool)
}

/// Two seconds of a 220 Hz sine.
fn sine() -> AudioPool {
    let samples = (0..16_000).map(|i| (std::f64::consts::TAU * 220.0 * i as f64 / f64::from(SR)).sin() as f32 * 0.5);
    let mut pool = AudioPool::new();
    pool.files
        .insert("sine.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples: samples.collect() }));
    pool
}

pub(crate) fn left(out: &[f32]) -> Vec<f32> {
    out.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect()
}

pub(crate) fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
}

/// The envelope the spec describes, written out independently of the engine: a linear fade-in
/// over `fade_in` frames, a linear fade-out over the last `fade_out` frames, and the click guard.
fn expected_gain(frame: usize, frames: usize, fade_in: f64, fade_out: f64) -> f64 {
    let since_start = frame as f64;
    // Frames after this one, so the last frame is 0 like the first.
    let until_end = (frames - frame - 1) as f64;
    let guard = (since_start / GUARD).min(until_end / GUARD).min(1.0);
    let fade_in = if fade_in > 0.0 { (since_start / fade_in).min(1.0) } else { 1.0 };
    let fade_out = if fade_out > 0.0 { (until_end / fade_out).min(1.0) } else { 1.0 };
    guard * fade_in * fade_out
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Compares `render` with the golden file `name`, little-endian f32. With `SUHINA_BLESS=1` the
/// file is written instead (see tests/golden/README.md).
pub(crate) fn assert_golden(name: &str, render: &[f32]) {
    assert_golden_within(name, render, 1e-6);
}

/// [`assert_golden`] with a tolerance of `tolerance`, for renders whose DSP is not bit-exact across
/// platforms (the time-stretcher's floating-point maths).
pub(crate) fn assert_golden_within(name: &str, render: &[f32], tolerance: f32) {
    suhina_test_support::assert_golden_f32(&golden_dir().join(format!("{name}.f32")), render, tolerance);
}

/// A 2-beat clip (1 s) with `fade_in` and `fade_out` beats, rendered offline: the left channel.
fn render_constant(fade_in: f64, fade_out: f64, block: usize) -> Vec<f32> {
    let (region, pool) = constant();
    left(
        &render_offline_from(
            &project(region, 2.0, fade_in, fade_out),
            RenderInputs::from_audio(&pool),
            SR,
            block,
            None,
        )
        .unwrap(),
    )
}

#[test]
fn a_fade_in_renders_like_its_golden_file() {
    let out = render_constant(1.0, 0.0, 512);
    assert_eq!(out.len(), 2 * BEAT);
    // The level the clip reaches once the fade is over.
    let full = out[BEAT + BEAT / 2];
    assert!(full > 0.0);
    for (frame, sample) in out.iter().enumerate() {
        let expected = f64::from(full) * expected_gain(frame, out.len(), BEAT as f64, 0.0);
        assert!((f64::from(*sample) - expected).abs() < 1e-6, "frame {frame}: {sample}, expected {expected}");
    }
    // Half-way through the fade, half the level.
    assert!((out[BEAT / 2] - full * 0.5).abs() < 1e-6);
    assert_golden("fade-in", &out);
}

#[test]
fn a_fade_out_renders_like_its_golden_file() {
    let out = render_constant(0.0, 0.5, 512);
    let full = out[BEAT / 2];
    for (frame, sample) in out.iter().enumerate() {
        let expected = f64::from(full) * expected_gain(frame, out.len(), 0.0, BEAT as f64 / 2.0);
        assert!((f64::from(*sample) - expected).abs() < 1e-6, "frame {frame}: {sample}, expected {expected}");
    }
    // The fade-out starts exactly half a beat before the end.
    assert_eq!(out[2 * BEAT - BEAT / 2 - 1], full);
    assert!(out[2 * BEAT - BEAT / 2 + 1] < full);
    assert_golden("fade-out", &out);
}

/// Every fade shape at curve −1, 0 and +1, rendered offline: the gain at the start, half-way and
/// the end of a one-beat fade-in and fade-out is the spec's `s(t)^(4^(−curve))`
/// (`docs/spec/arrangement-editing.md` › Fades).
#[test]
fn every_fade_shape_and_curve_renders_as_the_spec_says() {
    let (region, pool) = constant();
    let full = render_constant(0.0, 0.0, 512)[BEAT];
    for shape in FadeShape::ALL {
        for curve in [-1.0, 0.0, 1.0] {
            let fade = FadeCurve { shape, curve };
            let mut p = project(region.clone(), 2.0, 1.0, 1.0);
            let clip = &mut p.arrangement.lanes[0].clips[0];
            (clip.fade_in_curve, clip.fade_out_curve) = (fade, fade);
            let out = left(&render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 512, None).unwrap());
            assert_eq!(out.len(), 2 * BEAT);
            // The fade-in's t is the frames before a frame over the fade's frames, the
            // fade-out's the frames after it; the two meet in the middle of the clip.
            let last = out.len() - 1;
            let close = |frame: usize, t_in: f64| {
                let t_out = (last - frame) as f64 / BEAT as f64;
                let want = f64::from(full) * fade.gain(t_in.min(1.0)) * fade.gain(t_out.min(1.0));
                let got = f64::from(out[frame]);
                assert!((got - want).abs() < 1e-6, "{shape:?} {curve}: frame {frame} is {got}, expected {want}");
            };
            assert_eq!((out[0], out[last]), (0.0, 0.0), "{shape:?} {curve}");
            close(BEAT / 2, 0.5);
            close(BEAT, 1.0);
            close(last - BEAT / 2, 1.5);
            // Half-way through each fade, with the other fade not yet begun: s(½)^(4^(−curve)).
            let half = f64::from(out[BEAT / 2]) / f64::from(full);
            assert!((half - fade.gain(0.5)).abs() < 1e-6, "{shape:?} {curve}: {half}");
            let half_out = f64::from(out[last - BEAT / 2]) / f64::from(full);
            assert!((half_out - fade.gain(0.5)).abs() < 1e-6, "{shape:?} {curve}: {half_out}");
        }
    }
}

/// The spec's numbers, written out rather than taken from the model: the level half-way through
/// a one-beat fade-in, rendered offline, for each shape unbent and for the curve's ends.
#[test]
fn fades_render_the_specs_numbers_half_way() {
    let (region, pool) = constant();
    let full = f64::from(render_constant(0.0, 0.0, 512)[BEAT]);
    for (shape, curve, half) in [
        (FadeShape::Linear, 0.0, 0.5),
        (FadeShape::EqualPower, 0.0, std::f64::consts::FRAC_1_SQRT_2),
        (FadeShape::Fast, 0.0, 0.75),
        (FadeShape::Slow, 0.0, 0.25),
        (FadeShape::SCurve, 0.0, 0.5),
        (FadeShape::Linear, 1.0, 0.840_896_415),
        (FadeShape::Linear, -1.0, 0.0625),
        (FadeShape::Slow, -1.0, 0.003_906_25),
        (FadeShape::Fast, 1.0, 0.930_604_859),
    ] {
        let mut p = project(region.clone(), 2.0, 1.0, 0.0);
        p.arrangement.lanes[0].clips[0].fade_in_curve = FadeCurve { shape, curve };
        let out = left(&render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 512, None).unwrap());
        let got = f64::from(out[BEAT / 2]) / full;
        assert!((got - half).abs() < 1e-6, "{shape:?} at curve {curve}: {got}, the spec says {half}");
    }
}

/// Linear and unbent is what fades were before shapes: the goldens hold.
#[test]
fn the_default_fade_is_the_linear_one() {
    assert_eq!(FadeCurve::default(), FadeCurve { shape: FadeShape::Linear, curve: 0.0 });
    let (region, pool) = constant();
    let mut p = project(region, 2.0, 1.0, 0.5);
    let plain = left(&render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 512, None).unwrap());
    p.arrangement.lanes[0].clips[0].fade_in_curve = FadeCurve { shape: FadeShape::Linear, curve: 0.0 };
    assert_eq!(left(&render_offline_from(&p, RenderInputs::from_audio(&pool), SR, 512, None).unwrap()), plain);
    assert_eq!(plain, render_constant(1.0, 0.5, 512));
}

#[test]
fn fades_do_not_depend_on_the_block_size() {
    let reference = render_constant(0.75, 0.5, 512);
    for block in [1, 64, 333, 4096] {
        assert_eq!(render_constant(0.75, 0.5, block), reference, "block {block}");
    }
}

#[test]
fn bad_fades_are_an_error_not_a_crash() {
    let (region, pool) = constant();
    for (fade_in, fade_out) in [(3.0, 1.0), (-1.0, 0.0), (0.0, f64::NAN)] {
        let result = render_offline_from(
            &project(region.clone(), 2.0, fade_in, fade_out),
            RenderInputs::from_audio(&pool),
            SR,
            512,
            None,
        );
        assert_eq!(
            result.err(),
            Some(crate::RenderError::InvalidProject(vec![ValidationError::BadFades {
                lane: suhina_model::LaneId::NONE,
                clip: suhina_model::ClipId::NONE
            }])),
            "fades {fade_in} and {fade_out}"
        );
    }
}

#[test]
fn stretched_clips_have_the_same_fades() {
    let pool = sine();
    let warped = region("sine.wav", true, StretchMode::Smooth, 2.0);
    let render = |fade_in, fade_out| {
        left(
            &render_offline_from(
                &project(warped.clone(), 4.0, fade_in, fade_out),
                RenderInputs::from_audio(&pool),
                SR,
                512,
                None,
            )
            .unwrap(),
        )
    };
    let plain = render(0.0, 0.0);
    let faded = render(1.0, 1.5);
    assert_eq!(plain.len(), faded.len());
    // The stretcher's output is the same; only the envelope over it changes.
    let frames = plain.len();
    let (fade_in, fade_out) = (BEAT as f64, 1.5 * BEAT as f64);
    for (frame, (p, f)) in plain.iter().zip(&faded).enumerate() {
        let fades = (frame as f64 / fade_in).min(1.0) * ((frames - frame - 1) as f64 / fade_out).min(1.0);
        let expected = f64::from(*p) * fades;
        assert!((f64::from(*f) - expected).abs() < 1e-5, "frame {frame}: {f}, expected {expected}");
    }
}

/// The click guard ramps to 0 on a clip's last frame as it starts from 0 on its first, in the
/// direct, the stretched and the launched reader, so no clip ends with a step (#163).
#[test]
fn a_clip_ends_on_silence_like_it_starts() {
    let (region, pool) = constant();
    let direct = render_constant(0.0, 0.0, 512);
    assert_eq!(direct.len(), 2 * BEAT);
    let last = direct.len() - 1;
    assert_eq!((direct[0], direct[last]), (0.0, 0.0));
    for k in 0..GUARD as usize + 2 {
        assert_eq!(direct[k], direct[last - k], "the guard is symmetric, {k} frames in");
    }

    let sines = sine();
    let warped = region_for_stretch();
    let stretched = left(
        &render_offline_from(&project(warped, 4.0, 0.0, 0.0), RenderInputs::from_audio(&sines), SR, 512, None).unwrap(),
    );
    assert_eq!(stretched.last().copied(), Some(0.0));
    assert!(stretched[stretched.len() - 2].abs() < 0.5 / GUARD as f32 + 1e-6);

    let commands = [
        (0.0, LaunchCommand::SetQuantization(crate::Quantization::None)),
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    ];
    let mut p = project(region, 2.0, 0.0, 0.0);
    p.arrangement.lanes.clear();
    let launched = left(&render_launches(&p, &pool, SR, &commands, 1.5).unwrap());
    // The clip plays once, two beats: its last frame is silent, and so is everything after.
    assert!(launched[2 * BEAT - 2] > 0.0);
    assert!(launched[2 * BEAT - 1..].iter().all(|s| *s == 0.0), "{:?}", &launched[2 * BEAT - 3..2 * BEAT + 1]);
}

fn region_for_stretch() -> AudioRegion {
    region("sine.wav", true, StretchMode::Smooth, 2.0)
}

/// Launches the project's slot at once and renders `seconds`, with `more` commands after the
/// launch: the left channel. The arrangement is cleared, so only the launched clip sounds.
fn launch(mut p: Project, pool: &AudioPool, seconds: f64, more: &[(f64, LaunchCommand)]) -> Vec<f32> {
    p.arrangement.lanes.clear();
    let mut commands = vec![
        (0.0, LaunchCommand::SetQuantization(crate::Quantization::None)),
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    ];
    commands.extend_from_slice(more);
    left(&render_launches(&p, pool, SR, &commands, seconds).unwrap())
}

/// `p` with its slot clip looping over its whole length.
fn looping(mut p: Project) -> Project {
    if let Some(clip) = p.launcher.columns[0].slots[0].clip.as_mut() {
        clip.loop_range = Some(LoopRange { start: 0.0, end: clip.length });
    }
    p
}

/// A launched clip without a loop fades in from the launch and out at its end marker, the same
/// envelope an arrangement clip has.
#[test]
fn a_launched_clip_fades_in_and_out_like_an_arrangement_clip() {
    let (region, pool) = constant();
    let out = launch(project(region, 2.0, 1.0, 0.5), &pool, 1.5, &[]);
    let arranged = render_constant(1.0, 0.5, 512);
    assert_eq!(&out[..arranged.len()], &arranged[..]);
    assert!(out[arranged.len()..].iter().all(|s| *s == 0.0));
    let full = render_constant(0.0, 0.0, 512)[BEAT];
    for (frame, sample) in arranged.iter().enumerate() {
        let expected = f64::from(full) * expected_gain(frame, arranged.len(), BEAT as f64, BEAT as f64 / 2.0);
        assert!((f64::from(*sample) - expected).abs() < 1e-6, "frame {frame}: {sample}, expected {expected}");
    }
}

/// A looping launched clip fades in once, from the launch; the loop passes after it play at
/// full level and its fade-out never sounds, since it never ends by itself.
#[test]
fn a_looping_launched_clip_fades_in_once_like_its_golden_file() {
    let (region, pool) = constant();
    // A 2-beat loop with a 1-beat fade-in and a half-beat fade-out, for three passes.
    let out = launch(looping(project(region, 2.0, 1.0, 0.5)), &pool, 3.0, &[]);
    assert_eq!(out.len(), 6 * BEAT);
    let full = render_constant(0.0, 0.0, 512)[BEAT];
    for (frame, sample) in out.iter().enumerate() {
        let guard = (frame as f64 / GUARD).min(1.0);
        let expected = f64::from(full) * guard * (frame as f64 / BEAT as f64).min(1.0);
        assert!((f64::from(*sample) - expected).abs() < 1e-6, "frame {frame}: {sample}, expected {expected}");
    }
    assert_golden("launched-loop-fade-in", &out);
}

/// Stopping a looping launched clip uses the short stop fade, not the clip's fade-out, so the
/// stop stays where it was quantised.
#[test]
fn a_stopped_launched_clip_does_not_play_its_fade_out() {
    let (region, pool) = constant();
    let stop = [(1.5, LaunchCommand::StopColumn { column: 0 })];
    let with_fade_out = launch(looping(project(region.clone(), 2.0, 0.0, 1.0)), &pool, 2.0, &stop);
    let without = launch(looping(project(region, 2.0, 0.0, 0.0)), &pool, 2.0, &stop);
    assert_eq!(with_fade_out, without);
    // Silent within the 5 ms stop fade.
    let silent_from = (1.5 * f64::from(SR)) as usize + (0.005 * f64::from(SR)) as usize + 1;
    assert!(with_fade_out[silent_from..].iter().all(|s| *s == 0.0));
}

/// A launched warped clip, read through the stretcher, has the same fade-in.
#[test]
fn a_launched_stretched_clip_fades_in() {
    let pool = sine();
    let warped = looping(project(region_for_stretch(), 4.0, 1.0, 0.0));
    let plain = looping(project(region_for_stretch(), 4.0, 0.0, 0.0));
    let (faded, plain) = (launch(warped, &pool, 1.5, &[]), launch(plain, &pool, 1.5, &[]));
    assert!(plain[BEAT / 2..BEAT].iter().any(|s| s.abs() > 0.1), "the stretcher plays");
    for (frame, (p, f)) in plain.iter().zip(&faded).enumerate() {
        let expected = f64::from(*p) * (frame as f64 / BEAT as f64).min(1.0);
        assert!((f64::from(*f) - expected).abs() < 1e-5, "frame {frame}: {f}, expected {expected}");
    }
}
