// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{RenderInputs, RenderOptions, render_offline_from};
use suhina_model::*;

const SR: u32 = 48_000;

fn note(time: f64, duration: f64, key: u8) -> Note {
    Note { time, duration, key, velocity: 1.0, release_velocity: 0.5, channel: 0, muted: false }
}

fn project_with(lanes: Vec<ContentLane>, clips: Vec<Clip>, tracks: u32) -> Project {
    Project {
        tracks: (1..=tracks)
            .map(|i| Track {
                id: TrackId(i),
                name: format!("T{i}"),
                color: None,
                content: ContentKinds { notes: true, audio: false },
                channel: Some(ChannelId(i)),
                parent: None,
                midi_input: Default::default(),
                audio_input: Default::default(),
                extensions: Extensions::default(),
            })
            .collect(),
        channels: (1..=tracks)
            .map(|i| Channel {
                id: ChannelId(i),
                name: format!("C{i}"),
                role: ChannelRole::Regular,
                volume: 1.0,
                pan: 0.0,
                mute: false,
                solo: false,
                cue: false,
                output: None,
                sends: Vec::new(),
                devices: Vec::new(),
                extensions: Extensions::default(),
                automation: Vec::new(),
            })
            .collect(),
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 4.0,
            lanes,
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane { id: Default::default(), name: "l".into(), track: Some(TrackId(1)), clips }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

fn clip(start: f64, length: f64) -> Clip {
    Clip {
        id: Default::default(),
        content: ContentId(1),
        start,
        length,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        color: None,
        extensions: Extensions::default(),
    }
}

fn untargeted(notes: Vec<Note>) -> Vec<ContentLane> {
    vec![ContentLane { target: None, data: LaneData::Notes(notes) }]
}

/// Frames where sound starts after more than 100 silent frames.
fn onsets(out: &[f32]) -> Vec<usize> {
    let mut result = Vec::new();
    let mut silent_run = usize::MAX;
    for (i, frame) in out.as_chunks::<2>().0.iter().enumerate() {
        let loud = frame.iter().any(|s| s.abs() > 1e-6);
        if loud && silent_run > 100 {
            result.push(i);
        }
        silent_run = if loud { 0 } else { silent_run.saturating_add(1) };
    }
    result
}

/// A sine starts at phase zero, so the first audible frame lands a frame or two after note-on.
fn assert_onsets(out: &[f32], expected: &[usize]) {
    let actual = onsets(out);
    assert_eq!(actual.len(), expected.len(), "onsets {actual:?}, expected {expected:?}");
    for (a, e) in actual.iter().zip(expected) {
        assert!(a.abs_diff(*e) <= 2, "onsets {actual:?}, expected {expected:?}");
    }
}

#[test]
fn empty_project_renders_nothing() {
    assert_eq!(render_offline(&Project::default(), SR, 256), Ok(Vec::new()));
}

#[test]
fn note_on_beat_two_starts_half_a_second_in_at_120_bpm() {
    let project = project_with(untargeted(vec![note(1.0, 1.0, 69)]), vec![clip(0.0, 4.0)], 1);
    let out = render_offline(&project, SR, 256).unwrap();
    assert_onsets(&out, &[SR as usize / 2]);
}

#[test]
fn a4_plays_at_440_hz() {
    let project = project_with(untargeted(vec![note(0.0, 2.0, 69)]), vec![clip(0.0, 4.0)], 1);
    let out = render_offline(&project, SR, 256).unwrap();
    // Count upward zero crossings over one second of the left channel.
    let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|f| f[0]).take(SR as usize).collect();
    let crossings = left.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    assert!((439..=441).contains(&crossings), "{crossings} crossings");
}

#[test]
fn block_size_does_not_change_the_output() {
    let project = project_with(
        untargeted(vec![note(0.0, 0.5, 60), note(0.25, 1.0, 64), note(1.5, 0.3, 67)]),
        vec![clip(0.0, 4.0)],
        1,
    );
    let reference = render_offline(&project, SR, 1).unwrap();
    for block in [7, 64, 256, 1024, 100_000] {
        assert_eq!(render_offline(&project, SR, block).unwrap(), reference, "block {block}");
    }
}

#[test]
fn muted_channel_is_silent() {
    let mut project = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![clip(0.0, 4.0)], 1);
    project.channels[0].mute = true;
    assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
}

/// Mutes note `index` of the first content's first note lane.
fn mute_note(project: &mut Project, index: usize) {
    if let Some(lane) = project.contents.first_mut().and_then(|c| c.lanes.first_mut())
        && let LaneData::Notes(notes) = &mut lane.data
        && let Some(note) = notes.get_mut(index)
    {
        note.muted = true;
    }
}

#[test]
fn a_muted_note_is_silent_and_does_not_shorten_the_render() {
    // The long first note covers the short second one's span, so the render lasts the same
    // whether the second note plays or not.
    let notes = vec![note(0.0, 2.0, 60), note(0.5, 0.5, 64)];
    let mut project = project_with(untargeted(notes.clone()), vec![clip(0.0, 4.0)], 1);
    let all = render_offline(&project, SR, 256).unwrap();
    let ends = SR as usize + (RELEASE_SECONDS * f64::from(SR)) as usize;
    assert_eq!(all.len() / 2, ends);

    mute_note(&mut project, 1);
    let muted = render_offline(&project, SR, 256).unwrap();
    assert_eq!(muted.len(), all.len(), "muting does not change the render length");
    assert!(rms(&muted, 0, muted.len() / 2) < rms(&all, 0, all.len() / 2));
    // The muted note is not there at all: the render is the other note's alone.
    let alone = project_with(untargeted(vec![notes[0]]), vec![clip(0.0, 4.0)], 1);
    assert_eq!(muted, render_offline(&alone, SR, 256).unwrap());

    // Every note muted: silence, and no length at all.
    mute_note(&mut project, 0);
    assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
}

#[test]
fn muting_one_of_two_notes_halves_the_sounding_notes() {
    // Two notes sounding together: muting one leaves a single voice, so its RMS is about
    // 1/sqrt(2) of the pair's, and the render is what the other note alone would give.
    let notes = vec![note(0.0, 1.0, 60), note(0.0, 1.0, 64)];
    let mut project = project_with(untargeted(notes.clone()), vec![clip(0.0, 4.0)], 1);
    let all = render_offline(&project, SR, 256).unwrap();
    let all_rms = rms(&all, 0, all.len() / 2);

    mute_note(&mut project, 0);
    let muted = render_offline(&project, SR, 256).unwrap();
    let muted_rms = rms(&muted, 0, muted.len() / 2);
    assert!(muted_rms < all_rms * 0.9 && muted_rms > all_rms * 0.5, "one of two voices: {muted_rms} against {all_rms}");

    let alone = project_with(untargeted(vec![notes[1]]), vec![clip(0.0, 4.0)], 1);
    assert_eq!(muted, render_offline(&alone, SR, 256).unwrap(), "the muted note is not there");
    assert_eq!(onsets(&all).len(), 1, "the two notes start together");
}

#[test]
fn hard_left_pan_leaves_right_silent() {
    let mut project = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![clip(0.0, 4.0)], 1);
    project.channels[0].pan = -1.0;
    let out = render_offline(&project, SR, 256).unwrap();
    assert!(out.as_chunks::<2>().0.iter().all(|f| f[1].abs() < 1e-6));
    assert!(out.as_chunks::<2>().0.iter().any(|f| f[0].abs() > 0.1));
}

#[test]
fn clip_cuts_notes_at_its_end_and_skips_notes_after_it() {
    let project = project_with(untargeted(vec![note(0.0, 4.0, 60), note(2.0, 1.0, 64)]), vec![clip(0.0, 1.0)], 1);
    let out = render_offline(&project, SR, 256).unwrap();
    // One beat at 120 bpm plus the release tail.
    let expected = SR as usize / 2 + (RELEASE_SECONDS * f64::from(SR)) as usize;
    assert_eq!(out.len() / 2, expected);
    assert_eq!(onsets(&out).len(), 1);
}

#[test]
fn loop_range_repeats_until_the_clip_ends() {
    let mut looped = clip(0.0, 4.0);
    looped.loop_range = Some(LoopRange { start: 0.0, end: 1.0 });
    let project = project_with(untargeted(vec![note(0.0, 0.25, 60)]), vec![looped], 1);
    let out = render_offline(&project, SR, 256).unwrap();
    let beat = SR as usize / 2;
    assert_onsets(&out, &[0, beat, 2 * beat, 3 * beat]);
}

/// A shared pattern drives two tracks and is placed twice on a free lane.
#[test]
fn shared_pattern_plays_every_placement_on_its_target_tracks() {
    let lanes = vec![
        ContentLane { target: Some(TrackId(1)), data: LaneData::Notes(vec![note(0.0, 0.25, 36)]) },
        ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(0.5, 0.25, 38)]) },
    ];
    let mut project = project_with(lanes, vec![clip(0.0, 1.0), clip(2.0, 1.0)], 2);
    project.arrangement.lanes[0].track = None;
    let out = render_offline(&project, SR, 256).unwrap();
    let beat = SR as usize / 2;
    assert_onsets(&out, &[0, beat / 2, 2 * beat, 2 * beat + beat / 2]);

    // Muting track 2 removes only its notes.
    project.channels[1].mute = true;
    let out = render_offline(&project, SR, 256).unwrap();
    assert_onsets(&out, &[0, 2 * beat]);
}

#[test]
fn solo_leaves_only_soloed_tracks_their_groups_and_returns() {
    let lanes = vec![
        ContentLane { target: Some(TrackId(1)), data: LaneData::Notes(vec![note(0.0, 0.25, 36)]) },
        ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(0.5, 0.25, 38)]) },
    ];
    let mut project = project_with(lanes, vec![clip(0.0, 1.0)], 2);
    project.arrangement.lanes[0].track = None;
    let beat = SR as usize / 2;

    project.channels[1].solo = true;
    assert_onsets(&render_offline(&project, SR, 256).unwrap(), &[beat / 2]);

    // A return track stays audible.
    project.channels[0].role = ChannelRole::Effect;
    assert_onsets(&render_offline(&project, SR, 256).unwrap(), &[0, beat / 2]);
    project.channels[0].role = ChannelRole::Regular;

    // Soloing a group plays what is routed into it.
    project.channels[1].solo = false;
    project.channels.push(Channel { id: ChannelId(5), solo: true, ..project.channels[1].clone() });
    project.channels[0].output = Some(ChannelId(5));
    assert_onsets(&render_offline(&project, SR, 256).unwrap(), &[0]);

    // Muting still wins over solo.
    project.channels[0].mute = true;
    assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
}

#[test]
fn group_and_master_volume_and_mute_apply_to_the_tracks_routed_through_them() {
    let mut project = project_with(untargeted(vec![note(0.0, 1.0, 69)]), vec![clip(0.0, 4.0)], 2);
    let peak = |p: &Project| render_offline(p, SR, 256).unwrap().iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let alone = peak(&project);

    // Track 1 plays into channel 2 (a group) at half volume.
    project.channels[0].output = Some(ChannelId(2));
    project.channels[1].volume = 0.5;
    assert!((peak(&project) - alone * 0.5).abs() < 1e-3);

    project.channels[1].mute = true;
    assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
    project.channels[1].mute = false;

    // A master channel scales everything that does not route to it explicitly.
    project.channels.push(Channel {
        id: ChannelId(9),
        role: ChannelRole::Master,
        volume: 0.5,
        ..project.channels[1].clone()
    });
    assert!((peak(&project) - alone * 0.25).abs() < 1e-3);

    // A routing cycle is silence, not a hang.
    project.channels[1].output = Some(ChannelId(1));
    assert_eq!(render_offline(&project, SR, 256), Ok(Vec::new()));
}

/// A warped audio clip of a sine at `hz` (44.1 kHz file) whose warp markers put `seconds` of
/// audio on `beats` beats, played by a 48 kHz renderer at `bpm`.
fn stretched_project(hz: f64, seconds: f64, beats: f64, bpm: f64, mode: StretchMode) -> (Project, AudioPool) {
    let file_rate = 44_100u32;
    let frames = (seconds * f64::from(file_rate)) as usize;
    let samples = (0..frames)
        .map(|i| (std::f64::consts::TAU * hz * i as f64 / f64::from(file_rate)).sin() as f32 * 0.5)
        .collect();
    let data = AudioData { sample_rate: file_rate, channels: 1, samples };
    let region = AudioRegion {
        file: "sine.wav".into(),
        absolute_file: None,
        sample_rate: Some(file_rate),
        channels: Some(1),
        duration_seconds: Some(seconds),
        warped: true,
        stretch: mode,
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: beats, audio_time: seconds }],
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    let mut project =
        project_with(vec![ContentLane { target: None, data: LaneData::Audio(region) }], vec![clip(0.0, beats)], 1);
    project.transport.tempo_bpm = bpm;
    let mut pool = AudioPool::new();
    pool.files.insert("sine.wav".into(), std::sync::Arc::new(data));
    (project, pool)
}

/// Upward zero crossings per second over the middle half of the left channel.
fn frequency(out: &[f32]) -> f64 {
    let left: Vec<f32> = out.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
    let middle = &left[left.len() / 4..left.len() * 3 / 4];
    let ups = middle.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    ups as f64 / (middle.len() as f64 / f64::from(SR))
}

#[test]
fn warped_audio_is_stretched_in_time_not_pitch() {
    // 4 s of audio on 8 beats at 100 bpm lasts 4.8 s: 0.83× speed, pitch must stay 440 Hz.
    let (project, pool) = stretched_project(440.0, 4.0, 8.0, 100.0, StretchMode::Smooth);
    let out = render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 512, None).unwrap();
    assert!((out.len() as f64 / 2.0 / f64::from(SR) - 4.8).abs() < 0.01);
    let hz = frequency(&out);
    assert!((hz - 440.0).abs() < 440.0 * 0.01, "{hz} Hz");

    // Re-pitch for comparison: the same clip plays 0.83× lower.
    let (project, pool) = stretched_project(440.0, 4.0, 8.0, 100.0, StretchMode::Repitch);
    let hz = frequency(&render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 512, None).unwrap());
    assert!((hz - 440.0 * 100.0 / 120.0).abs() < 440.0 * 0.01, "{hz} Hz");
}

#[test]
fn rhythmic_mode_keeps_pitch_within_its_range() {
    // 2 s on 4 beats at 126 bpm: 1.05× speed.
    let (project, pool) = stretched_project(330.0, 2.0, 4.0, 126.0, StretchMode::Rhythmic);
    let hz = frequency(&render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 512, None).unwrap());
    assert!((hz - 330.0).abs() < 330.0 * 0.02, "{hz} Hz");
}

#[test]
fn stretched_audio_does_not_depend_on_the_block_size() {
    let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
    let reference = render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 1024, None).unwrap();
    for block in [64, 333, 4096] {
        let out = render_offline_from(&project, RenderInputs::from_audio(&pool), SR, block, None).unwrap();
        assert_eq!(out.len(), reference.len());
        let worst = out.iter().zip(&reference).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "block {block}: differs by {worst}");
    }
}

/// A NaN velocity is silence for its note, not NaN in the whole mix (#161).
#[test]
fn a_nan_velocity_note_leaves_the_mix_finite() {
    let mut bad = note(0.0, 1.0, 60);
    bad.velocity = f32::NAN;
    let project = project_with(untargeted(vec![bad, note(1.0, 1.0, 69)]), vec![clip(0.0, 4.0)], 1);
    let out = render_offline(&project, SR, 256).unwrap();
    assert!(out.iter().all(|s| s.is_finite()));
    assert_onsets(&out, &[SR as usize / 2]);
}

#[test]
fn hostile_input_returns_errors_or_silence_instead_of_panicking() {
    assert_eq!(render_offline(&Project::default(), 0, 256).err(), Some(RenderError::InvalidSampleRate(0)));
    let mut project = Project::default();
    project.transport.tempo_bpm = f64::NAN;
    assert!(matches!(render_offline(&project, SR, 256), Err(RenderError::InvalidTempo(_))));

    let weird = vec![note(f64::NAN, 1.0, 60), note(0.0, f64::INFINITY, 127), note(-5.0, 1.0, 0)];
    let mut project = project_with(untargeted(weird), vec![clip(0.0, 1.0)], 1);
    project.channels[0].volume = f64::NAN;
    assert!(render_offline(&project, SR, 256).is_ok());

    let project = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![clip(0.0, 1e12)], 1);
    let mut looped = clip(0.0, 1e12);
    looped.loop_range = Some(LoopRange { start: 0.0, end: 1e-9 });
    let looping = project_with(untargeted(vec![note(0.0, 1.0, 60)]), vec![looped], 1);
    assert!(render_offline(&project, SR, 256).is_ok());
    assert!(render_offline(&looping, SR, 256).is_ok());
}

/// Renders `blocks` blocks of `frames` from `renderer`, calling `between` before each.
fn blocks(renderer: &mut Renderer, blocks: usize, frames: usize, mut between: impl FnMut(usize)) -> Vec<f32> {
    let mut out = Vec::new();
    let mut block = vec![0.0f32; frames * 2];
    for i in 0..blocks {
        between(i);
        renderer.process(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
}

#[test]
fn a_stretch_worker_renders_the_same_audio_as_offline() {
    let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
    let mut offline = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut offline, 200, 441, |_| {});
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut worker = renderer.take_stretch_worker().unwrap();
    assert!(renderer.take_stretch_worker().is_none());
    let out = blocks(&mut renderer, 200, 441, |_| worker.fill());
    assert!(reference.iter().any(|s| s.abs() > 0.1));
    assert_eq!(max_diff(&reference, &out), 0.0);
    assert_eq!(renderer.stretch_underruns(), 0);
}

#[test]
fn a_late_stretch_worker_costs_silence_but_not_timing() {
    let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
    let mut offline = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut offline, 200, 441, |_| {});
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut worker = renderer.take_stretch_worker().unwrap();
    // The worker stalls for 1 s (blocks 0-99), longer than it renders ahead.
    let out = blocks(&mut renderer, 200, 441, |i| {
        if i >= 100 {
            worker.fill();
        }
    });
    assert!(renderer.stretch_underruns() > 0);
    let stalled = &out[2 * 441 * 70..2 * 441 * 100];
    assert!(stalled.iter().all(|s| *s == 0.0), "a stalled worker plays silence");
    // Once it catches up the clip plays where it should, not where it left off: the voice
    // jumped ahead of the playhead rather than rendering the second it missed, so it sounds
    // again within a few blocks, as loud as the clip is there. Its stretcher started again
    // at the jump, so the samples are not the reference's to the bit.
    let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
    let block = |samples: &[f32], i: usize| samples[2 * 441 * i..2 * 441 * (i + 1)].to_vec();
    for i in 115..200 {
        let (heard, wanted) = (rms(&block(&out, i)), rms(&block(&reference, i)));
        assert!((heard - wanted).abs() <= wanted * 0.1 + 1e-3, "block {i}: {heard} against {wanted}");
    }
}

#[test]
fn a_stretch_worker_rewinds_with_the_transport() {
    let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut worker = renderer.take_stretch_worker().unwrap();
    let first = blocks(&mut renderer, 50, 441, |_| worker.fill());
    renderer.handle(LaunchCommand::Stop);
    // The rewind handshake: the worker pauses, the audio side empties the rings, the worker
    // rewinds and fills them again, all while the transport is stopped.
    blocks(&mut renderer, 3, 441, |_| worker.fill());
    renderer.handle(LaunchCommand::Play);
    let again = blocks(&mut renderer, 50, 441, |_| worker.fill());
    assert_eq!(max_diff(&first, &again), 0.0);
    assert_eq!(renderer.stretch_underruns(), 0);
}

/// A jump of the playhead while playing is silent until the worker has moved its voices, but
/// that silence is intended, not an underrun (#169). Two block sizes, since the engine's
/// block follows the device's buffer.
#[test]
fn a_transport_jump_is_not_counted_as_underruns() {
    let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
    for frames in [128, 441] {
        let mut renderer =
            Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
        let mut worker = renderer.take_stretch_worker().unwrap();
        blocks(&mut renderer, 20, frames, |_| worker.fill());
        renderer.handle(LaunchCommand::SetPosition { beat: 1.0 });
        let after = blocks(&mut renderer, 40, frames, |_| worker.fill());
        assert!(after.iter().any(|s| s.abs() > 0.1), "the clip plays on from the new place");
        assert_eq!(renderer.stretch_underruns(), 0, "blocks of {frames}");
    }
}

#[test]
fn a_stretch_worker_thread_keeps_up() {
    let (project, pool) = stretched_project(440.0, 2.0, 4.0, 110.0, StretchMode::Smooth);
    let mut offline = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut offline, 100, 441, |_| {});
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    controller.run_stretch_worker(&mut renderer).unwrap();
    // Paced like a device: one 10 ms block every 10 ms.
    let out = blocks(&mut renderer, 100, 441, |_| std::thread::sleep(std::time::Duration::from_millis(10)));
    assert_eq!(controller.stretch_underruns(), 0);
    assert_eq!(max_diff(&reference, &out), 0.0);
}

/// RMS of the left channel over frames `from..to`.
fn rms(out: &[f32], from: usize, to: usize) -> f64 {
    let left: Vec<f64> = out[from * 2..to * 2].iter().step_by(2).map(|s| f64::from(*s)).collect();
    (left.iter().map(|s| s * s).sum::<f64>() / left.len() as f64).sqrt()
}

/// Frame where an update sent after `blocks` blocks of 480 frames takes over, for a project
/// with stretched clips; one without takes over at the next block, `blocks * 480`.
fn swap_frame(blocks: usize) -> usize {
    blocks * 480 + (0.15 * f64::from(SR)) as usize
}

#[test]
fn an_edit_is_heard_from_the_frame_it_takes_over() {
    // Read directly (re-pitch), and stretched.
    for mode in [StretchMode::Repitch, StretchMode::Smooth] {
        let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, mode);
        let mut edited = project.clone();
        edited.channels[0].volume = 0.5;
        let mut renderer =
            Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
        let mut controller = renderer.controller();
        let mut out = blocks(&mut renderer, 50, 480, |_| {});
        controller.update(&edited).unwrap();
        out.extend(blocks(&mut renderer, 100, 480, |_| {}));
        // Read directly, there are no stretch voices to prepare: the edit takes the next block.
        let at = if mode == StretchMode::Repitch { 50 * 480 } else { swap_frame(50) };
        let ratio = rms(&out, at, at + 9_600) / rms(&out, at - 9_600, at);
        assert!((ratio - 0.5).abs() < 0.01, "{mode:?}: level changed by {ratio}");
        let mut original =
            Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
        let original = blocks(&mut original, 150, 480, |_| {});
        assert_eq!(max_diff(&out[..at * 2], &original[..at * 2]), 0.0, "{mode:?}: before the swap");
        if mode == StretchMode::Repitch {
            // Read directly, the edited project's own render is the oracle, sample for sample.
            let mut reference =
                Renderer::build(&edited, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
            let reference = blocks(&mut reference, 150, 480, |_| {});
            assert_eq!(max_diff(&out[at * 2..], &reference[at * 2..]), 0.0);
        }
    }
}

#[test]
fn the_playhead_keeps_its_beat_when_the_tempo_changes() {
    // A short note on every beat, at 120 bpm (24 000 frames per beat).
    let notes = (0..8).map(|b| note(f64::from(b), 0.05, 69)).collect();
    let project = project_with(untargeted(notes), vec![clip(0.0, 8.0)], 1);
    let mut slower = project.clone();
    slower.transport.tempo_bpm = 60.0;
    let mut renderer =
        Renderer::build(&project, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 90, 480, |_| {});
    controller.update(&slower).unwrap();
    out.extend(blocks(&mut renderer, 285, 480, |_| {}));
    // With no stretched clips the swap takes the next block: frame 43 200, beat 1.8; beat 2
    // then comes 0.2 beats of 60 bpm later.
    let at = 90 * 480;
    assert_eq!(at, 43_200);
    let expected = [1, 24_001, at + 9_601, at + 9_601 + 48_000, at + 9_601 + 96_000];
    assert_eq!(onsets(&out)[..5], expected);
}

#[test]
fn a_launched_clip_plays_on_through_an_edit() {
    let (mut project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
    // The clip goes to the launcher instead of the arrangement; a second, silent track gets
    // the edit.
    let looped =
        Clip { loop_range: Some(LoopRange { start: 0.0, end: 8.0 }), ..project.arrangement.lanes[0].clips[0].clone() };
    project.arrangement.lanes[0].clips.clear();
    project.launcher.columns =
        vec![LauncherColumn { track: TrackId(1), slots: vec![Slot { clip: Some(looped), has_stop: true }] }];
    project.launcher.scenes = vec![Scene::default()];
    let mut edited = project.clone();
    edited.transport.time_signature = project.transport.time_signature;
    edited.tracks[0].name = "renamed".into();
    let render = |update: bool| {
        let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
        let mut controller = renderer.controller();
        renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
        let mut out = blocks(&mut renderer, 50, 480, |_| {});
        if update {
            controller.update(&edited).unwrap();
        }
        out.extend(blocks(&mut renderer, 100, 480, |_| {}));
        out
    };
    let (plain, edited_out) = (render(false), render(true));
    assert!(rms(&plain, 30_000, 40_000) > 0.1);
    // The playing clip keeps its stretch voice: not a sample differs.
    assert_eq!(max_diff(&plain, &edited_out), 0.0);
}

#[test]
fn an_edit_while_stopped_is_heard_from_the_next_start() {
    let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
    let mut edited = project.clone();
    edited.channels[0].volume = 0.25;
    let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
    let mut controller = renderer.controller();
    controller.update(&edited).unwrap();
    blocks(&mut renderer, 2, 480, |_| {});
    renderer.handle(LaunchCommand::Play);
    let out = blocks(&mut renderer, 50, 480, |_| {});
    let mut reference = Renderer::build(&edited, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut reference, 50, 480, |_| {});
    assert_eq!(max_diff(&out, &reference), 0.0);
}

#[test]
fn stretched_clips_carry_on_through_a_tempo_edit() {
    let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
    let mut slower = project.clone();
    slower.transport.tempo_bpm = 95.0;
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    controller.run_stretch_worker(&mut renderer).unwrap();
    // Paced like a device, with the worker on its own thread.
    let pace = |_| std::thread::sleep(std::time::Duration::from_millis(10));
    let mut out = blocks(&mut renderer, 50, 480, pace);
    controller.update(&slower).unwrap();
    out.extend(blocks(&mut renderer, 100, 480, pace));
    // The new graph's stretch voices were ready where the playhead went: no gap.
    assert_eq!(renderer.stretch_underruns(), 0);
    let at = swap_frame(50);
    assert!(rms(&out, at, at + 4_800) > 0.9 * rms(&out, at - 4_800, at));
}

#[test]
fn a_stretched_clip_keeps_its_voice_through_an_edit_that_leaves_it_alone() {
    // An edit elsewhere (a solo, a fader) sends a new graph: the clip's voice carries over to
    // it, stretcher and ring, so what plays is the same to the sample (#743).
    let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
    let mut offline = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut offline, 150, 480, |_| {});
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    controller.run_stretch_worker(&mut renderer).unwrap();
    let pace = |_| std::thread::sleep(std::time::Duration::from_millis(10));
    let mut out = blocks(&mut renderer, 50, 480, pace);
    controller.update(&project).unwrap();
    out.extend(blocks(&mut renderer, 100, 480, pace));
    assert_eq!(renderer.stretch_underruns(), 0);
    assert_eq!(controller.stretch_moves(), 0);
    assert_eq!(max_diff(&reference, &out), 0.0, "the voice went on as it was");
}

#[test]
fn an_edit_refused_by_a_full_queue_leaves_the_playing_voices_alone() {
    // Edits faster than the audio thread takes them (a fader drag) fill the swap queue; the
    // one refused must not hand the playing clip's voice to a graph that is then dropped.
    let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
    let mut offline = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut offline, 150, 480, |_| {});
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    controller.run_stretch_worker(&mut renderer).unwrap();
    let pace = |_| std::thread::sleep(std::time::Duration::from_millis(10));
    let mut out = blocks(&mut renderer, 50, 480, pace);
    let mut refused = false;
    for _ in 0..64 {
        // A frame apart, as a drag sends them: the stretch thread takes each worker in.
        std::thread::sleep(std::time::Duration::from_millis(16));
        if controller.update(&project).is_err() {
            refused = true;
            // Time for the stretch thread to act on anything it was handed.
            std::thread::sleep(std::time::Duration::from_millis(50));
            break;
        }
    }
    assert!(refused, "the queue filled up");
    out.extend(blocks(&mut renderer, 100, 480, pace));
    assert_eq!(renderer.stretch_underruns(), 0);
    assert_eq!(max_diff(&reference, &out), 0.0, "the voice went on as it was");
}

#[test]
fn an_unwarped_clip_carries_on_through_a_tempo_edit() {
    // Unwarped audio plays at its own speed: a tempo edit must not move it in its file.
    let (mut project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
    for content in &mut project.contents {
        for lane in &mut content.lanes {
            if let LaneData::Audio(region) = &mut lane.data {
                region.warped = false;
            }
        }
    }
    let mut slower = project.clone();
    slower.transport.tempo_bpm = 90.0;
    let mut original = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let original = blocks(&mut original, 150, 480, |_| {});
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 50, 480, |_| {});
    controller.update(&slower).unwrap();
    out.extend(blocks(&mut renderer, 100, 480, |_| {}));
    assert!(rms(&out, 40_000, 70_000) > 0.1);
    // The same file positions as without the edit (to rounding).
    assert!(max_diff(&out, &original) < 1e-6);
}

/// Zero crossings per second of the left channel over frames `from..to`.
fn hz_between(out: &[f32], from: usize, to: usize) -> f64 {
    let left: Vec<f32> = out[from * 2..to * 2].iter().step_by(2).copied().collect();
    let ups = left.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    ups as f64 / ((to - from) as f64 / f64::from(SR))
}

#[test]
fn the_metronome_clicks_on_every_beat_and_marks_the_bar() {
    // An empty project in 3/4 at 120 bpm: 24 000 frames to a beat.
    let mut project = Project::default();
    project.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
    renderer.handle(LaunchCommand::Play);
    let out = blocks(&mut renderer, 200, 480, |_| {});
    assert_eq!(onsets(&out), [1, 24_001, 48_001, 72_001]);
    // The bar's first beat is higher than the others.
    let (bar, beat) = (hz_between(&out, 0, 2_000), hz_between(&out, 24_000, 26_000));
    assert!((bar - 1_600.0).abs() < 50.0 && (beat - 1_000.0).abs() < 50.0, "{bar} Hz, {beat} Hz");
    assert!(rms(&out, 0, 2_400) > 0.05);
    // Off: silence, including through a restart.
    renderer.handle(LaunchCommand::SetMetronome { on: false, gain: None });
    renderer.handle(LaunchCommand::Stop);
    renderer.handle(LaunchCommand::Play);
    assert!(blocks(&mut renderer, 200, 480, |_| {}).iter().all(|s| *s == 0.0));
}

#[test]
fn the_metronome_follows_a_tempo_edit() {
    let mut project = Project::default();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
    renderer.handle(LaunchCommand::Play);
    let mut out = blocks(&mut renderer, 90, 480, |_| {});
    project.transport.tempo_bpm = 60.0;
    controller.update(&project).unwrap();
    out.extend(blocks(&mut renderer, 285, 480, |_| {}));
    // As with notes: the swap at frame 43 200 is beat 1.8; beat 2 comes 0.2 s later at 60 bpm.
    // The beat clock adds up per frame, so a click may land a frame late.
    let found = onsets(&out);
    for (got, expected) in found.iter().zip([1, 24_001, 52_801, 100_801]) {
        assert!(got.abs_diff(expected) <= 1, "{found:?}");
    }
}

fn with_utility(project: &mut Project) {
    project.channels[0].devices.push(Device {
        name: "Utility".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    });
}

#[test]
fn edits_sent_together_all_take_the_next_block() {
    // Audio read directly: a volume edit is heard on the playing clip at once.
    let (project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Repitch);
    let mut half = project.clone();
    half.channels[0].volume = 0.5;
    let mut quarter = project.clone();
    quarter.channels[0].volume = 0.25;
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let first = blocks(&mut renderer, 20, 480, |_| {});
    controller.update(&half).unwrap();
    controller.update(&quarter).unwrap();
    // One long block: both graphs are taken at its first frame, the second one winning.
    let next = blocks(&mut renderer, 1, 24_000, |_| {});
    let ratio = rms(&next, 0, 24_000) / rms(&first, 4_800, 9_600);
    assert!((ratio - 0.25).abs() < 0.01, "{ratio}");
}

#[test]
fn a_device_parameter_is_heard_from_the_next_block_and_outlives_an_older_graph() {
    // Stretched audio, so a new graph waits 0.15 s before it takes over.
    let (mut project, pool) = stretched_project(440.0, 4.0, 8.0, 110.0, StretchMode::Smooth);
    with_utility(&mut project);
    let mut renderer = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 50, 480, |_| {});
    // A graph built from the project before the change (here with no other edit), then the
    // parameter: -6 dB on the utility's gain, index 0.
    controller.update(&project).unwrap();
    controller.set_device_param(TrackId(1), 0, 0, -6.0206).unwrap();
    out.extend(blocks(&mut renderer, 100, 480, |_| {}));
    let mut untouched = Renderer::build(&project, RenderInputs::from_audio(&pool), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut untouched, 150, 480, |_| {});
    let now = 50 * 480;
    // Heard from the next block, after the utility's 20 ms ramp...
    let soon = rms(&out, now + 1_200, now + 6_000) / rms(&reference, now + 1_200, now + 6_000);
    assert!((soon - 0.5).abs() < 0.01, "{soon}");
    // ... and still after the older graph took over at 0.15 s.
    let at = swap_frame(50);
    let later = rms(&out, at + 2_400, at + 12_000) / rms(&reference, at + 2_400, at + 12_000);
    assert!((later - 0.5).abs() < 0.01, "{later}");
}

#[test]
fn a_mix_edit_reaches_a_sounding_note_at_the_next_block() {
    let project = project_with(untargeted(vec![note(0.0, 8.0, 69)]), vec![clip(0.0, 8.0)], 1);
    for (edit, expected) in [
        ((0.5, 0.0, false), (0.5, 0.5)),
        ((1.0, 1.0, false), (0.0, std::f64::consts::SQRT_2)),
        ((1.0, 0.0, true), (0.0, 0.0)),
    ] {
        let mut edited = project.clone();
        (edited.channels[0].volume, edited.channels[0].pan, edited.channels[0].mute) = edit;
        let mut renderer =
            Renderer::build(&project, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
        let mut controller = renderer.controller();
        let before = blocks(&mut renderer, 20, 480, |_| {});
        controller.update(&edited).unwrap();
        let after = blocks(&mut renderer, 10, 480, |_| {});
        let side = |out: &[f32], right: usize| {
            let s: Vec<f64> = out.iter().skip(right).step_by(2).map(|s| f64::from(*s)).collect();
            (s.iter().map(|x| x * x).sum::<f64>() / s.len() as f64).sqrt()
        };
        for (right, want) in [(0, expected.0), (1, expected.1)] {
            let ratio = side(&after, right) / side(&before[4_800 * 2..], right);
            assert!((ratio - want).abs() < 0.01, "{edit:?} side {right}: {ratio}");
        }
    }
}

/// Renders `frames` frames from `renderer` in 333-frame blocks.
fn frames(renderer: &mut Renderer, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    for block in out.chunks_mut(333 * 2) {
        renderer.process(block);
    }
    out
}

/// Notes before beat 2 end well before it; notes from beat 2 on.
fn notes_around_beat_two() -> Project {
    let notes = vec![note(0.0, 0.25, 60), note(1.0, 0.25, 62), note(2.0, 0.5, 64), note(3.25, 0.5, 67)];
    project_with(untargeted(notes), vec![clip(0.0, 4.0)], 1)
}

/// A note sounding at the loop end is released there, and the notes of the loop region
/// start again from the loop start (`docs/spec/arrangement-editing.md` › Loop region).
#[test]
fn a_note_over_the_loop_end_is_released_and_the_loop_plays_its_notes_again() {
    let beat = SR as usize / 2;
    let notes = vec![note(0.0, 0.25, 60), note(1.0, 1.5, 62)];
    let mut project = project_with(untargeted(notes), vec![clip(0.0, 4.0)], 1);
    project.transport.loop_range = Some(LoopRange { start: 1.0, end: 2.0 });
    project.transport.loop_enabled = true;
    let mut renderer = Renderer::new(&project, SR).unwrap();
    let out = frames(&mut renderer, 4 * beat);
    // Once the cut note's release has faded, each pass is the first pass again.
    let release = (RELEASE_SECONDS * f64::from(SR)) as usize + 1;
    let first = &out[(beat + release) * 2..2 * beat * 2];
    for pass in 1..3 {
        let at = (beat + pass * beat + release) * 2;
        assert!(max_diff(&out[at..at + first.len()], first) < 1e-6, "pass {pass}");
    }
    assert!(first.iter().any(|s| s.abs() > 0.1));
}

#[test]
fn playing_from_a_beat_renders_the_tail_of_the_whole_render() {
    let beat = SR as usize / 2;
    let project = notes_around_beat_two();
    let whole = render_offline(&project, SR, 256).unwrap();
    // Running from the start, jumped to beat 2 before the first block.
    let mut renderer = Renderer::new(&project, SR).unwrap();
    renderer.handle(LaunchCommand::SetPosition { beat: 2.0 });
    let tail = frames(&mut renderer, whole.len() / 2 - 2 * beat);
    assert!(tail.iter().any(|s| s.abs() > 0.1));
    assert_eq!(max_diff(&tail, &whole[2 * beat * 2..]), 0.0);
    // Stopped, the marker set, then started: the same.
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::SetPosition { beat: 2.0 });
    renderer.handle(LaunchCommand::Play);
    let started = frames(&mut renderer, whole.len() / 2 - 2 * beat);
    assert_eq!(max_diff(&started, &whole[2 * beat * 2..]), 0.0);
}

#[test]
fn audio_clips_play_from_their_middle_when_the_transport_starts_inside_them() {
    let beat = SR as usize / 2;
    for mode in [StretchMode::Repitch, StretchMode::Smooth] {
        // 4 s on 8 beats at 120 bpm: the clip runs from beat 0 to 8.
        let (project, pool) = stretched_project(440.0, 4.0, 8.0, 120.0, mode);
        let whole = render_offline_from(&project, RenderInputs::from_audio(&pool), SR, 333, None).unwrap();
        let mut renderer = Renderer::stopped(&project, &pool, SR).unwrap();
        renderer.handle(LaunchCommand::SetPosition { beat: 3.0 });
        renderer.handle(LaunchCommand::Play);
        let tail = frames(&mut renderer, whole.len() / 2 - 3 * beat);
        let reference = &whole[3 * beat * 2..];
        assert!(tail.iter().any(|s| s.abs() > 0.1), "{mode:?}");
        let worst = max_diff(&tail, reference);
        // A stretcher started mid-clip has no history yet: close, not identical.
        let allowed = if mode == StretchMode::Repitch { 0.0 } else { 0.05 };
        assert!(worst <= allowed, "{mode:?}: differs by {worst}");
        assert_eq!(renderer.stretch_underruns(), 0, "{mode:?}");
    }
}

#[test]
fn a_jump_while_playing_plays_on_from_the_new_beat() {
    let beat = SR as usize / 2;
    let project = notes_around_beat_two();
    let whole = render_offline(&project, SR, 256).unwrap();
    let mut renderer = Renderer::new(&project, SR).unwrap();
    // Half a beat in (nothing sounding), jump to beat 2.
    let before = frames(&mut renderer, beat / 2);
    assert_eq!(max_diff(&before, &whole[..beat]), 0.0);
    renderer.handle(LaunchCommand::SetPosition { beat: 2.0 });
    let after = frames(&mut renderer, whole.len() / 2 - 2 * beat);
    assert_eq!(max_diff(&after, &whole[2 * beat * 2..]), 0.0);
    let monitor = renderer.monitor();
    assert!(monitor.running());
    assert_eq!(monitor.start_beat(), 2.0);
}

/// A jump into a note while playing plays it, whether the marker moves with it
/// (`SetPosition`, a click on the ruler) or not (`Locate`).
#[test]
fn a_jump_into_a_sounding_note_plays_it_either_way() {
    let beat = SR as usize / 2;
    let project = notes_around_beat_two();
    let jumped = |command: LaunchCommand| {
        let mut renderer = Renderer::new(&project, SR).unwrap();
        frames(&mut renderer, beat / 2);
        renderer.handle(command);
        // A quarter of a beat: the note from beat 2 to 2.5, nothing else.
        frames(&mut renderer, beat / 4)
    };
    let set = jumped(LaunchCommand::SetPosition { beat: 2.25 });
    assert!(set.iter().any(|s| s.abs() > 0.1), "the note from beat 2 sounds");
    assert_eq!(max_diff(&set, &jumped(LaunchCommand::Locate { beat: 2.25 })), 0.0);
}

#[test]
fn stop_returns_to_the_marker_and_a_second_stop_to_the_start() {
    let project = notes_around_beat_two();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let monitor = renderer.monitor();
    renderer.handle(LaunchCommand::SetPosition { beat: 1.5 });
    frames(&mut renderer, 10);
    assert_eq!((monitor.start_beat(), monitor.beat()), (1.5, 1.5), "stopped, the playhead is at the marker");
    renderer.handle(LaunchCommand::Play);
    frames(&mut renderer, SR as usize / 2);
    assert!((monitor.beat() - 2.5).abs() < 1e-3, "{}", monitor.beat());
    renderer.handle(LaunchCommand::Stop);
    frames(&mut renderer, 10);
    assert_eq!((monitor.start_beat(), monitor.beat()), (1.5, 1.5));
    renderer.handle(LaunchCommand::Stop);
    frames(&mut renderer, 10);
    assert_eq!((monitor.start_beat(), monitor.beat()), (0.0, 0.0));
}

#[test]
fn stop_here_moves_the_marker_to_the_playhead() {
    let project = notes_around_beat_two();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let monitor = renderer.monitor();
    renderer.handle(LaunchCommand::SetPosition { beat: 1.0 });
    renderer.handle(LaunchCommand::Play);
    frames(&mut renderer, SR as usize);
    renderer.handle(LaunchCommand::StopHere);
    frames(&mut renderer, 10);
    assert!((monitor.start_beat() - 3.0).abs() < 1e-3, "{}", monitor.start_beat());
    // The next start carries on from there.
    renderer.handle(LaunchCommand::Play);
    frames(&mut renderer, 10);
    assert!(monitor.beat() > 3.0 - 1e-3 && monitor.beat() < 3.01, "{}", monitor.beat());
    // Stopped already, it goes to the start like Stop.
    renderer.handle(LaunchCommand::StopHere);
    renderer.handle(LaunchCommand::StopHere);
    frames(&mut renderer, 10);
    assert_eq!(monitor.start_beat(), 0.0);
}

#[test]
fn hostile_positions_are_ignored_or_kept_in_range() {
    let project = notes_around_beat_two();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let monitor = renderer.monitor();
    for (beat, want) in [(f64::NAN, 0.0), (-4.0, 0.0), (f64::INFINITY, 0.0), (1e300, MAX_START_BEAT)] {
        renderer.handle(LaunchCommand::SetPosition { beat });
        frames(&mut renderer, 1);
        assert_eq!(monitor.start_beat(), want, "{beat}");
    }
    // Far past the end: plays silence, never panics.
    renderer.handle(LaunchCommand::Play);
    assert!(frames(&mut renderer, 1000).iter().all(|s| *s == 0.0));
}

#[test]
fn the_metronome_waits_for_the_next_beat_after_a_start_between_beats() {
    let project = project_with(untargeted(Vec::new()), vec![clip(0.0, 4.0)], 1);
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
    renderer.handle(LaunchCommand::SetPosition { beat: 1.5 });
    renderer.handle(LaunchCommand::Play);
    let out = frames(&mut renderer, SR as usize / 2);
    // The first click is at beat 2, a quarter of a second in.
    assert_onsets(&out, &[SR as usize / 4]);
}

#[test]
fn the_cpu_load_is_measured_only_in_real_time() {
    let mut renderer = Renderer::stopped(&Project::default(), &AudioPool::new(), SR).unwrap();
    let controller = renderer.controller();
    let mut block = vec![0.0f32; 2 * 256];
    // Offline renders run flat out: nothing is measured.
    renderer.set_offline(true);
    for _ in 0..100 {
        renderer.process(&mut block);
    }
    assert!(!renderer.cpu.measured_any());
    assert_eq!(controller.cpu_monitor().load_milli(), 0);
    // Real-time playback is measured, and what it measures reaches the controller. The
    // block's own work is tiny, so the test gives the meter known busy times, as an
    // audio thread busy with a big project would have.
    renderer.set_offline(false);
    renderer.process(&mut block);
    assert!(renderer.cpu.measured_any());
    for _ in 0..30 {
        let started = Instant::now();
        renderer.process(&mut block);
        renderer.cpu.measured(started, started + Duration::from_millis(10));
    }
    assert!(controller.cpu_monitor().load_milli() > 200);
}
