// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Offline renders of each launch mode, legato and per-clip quantisation
//! (`docs/spec/clip-launching.md` › Launch settings).

use suhina_model::*;

use super::super::tests::{BAR, BEAT, assert_onsets, clip, note, notes, project, render, two_loops, with};
use crate::{LaunchCommand, Quantization};

const PRESS: LaunchCommand = LaunchCommand::LaunchClip { column: 0, slot: 0 };
const RELEASE: LaunchCommand = LaunchCommand::ReleaseClip { column: 0, slot: 0 };

/// `two_loops` with the clip in scene 1 set to `settings`.
fn two_loops_with(settings: LaunchSettings) -> Project {
    let mut p = two_loops();
    if let Some(clip) = p.launcher.columns[0].slots[0].clip.as_mut() {
        clip.launch = settings;
    }
    p
}

fn mode(mode: LaunchMode) -> LaunchSettings {
    LaunchSettings { mode, ..LaunchSettings::default() }
}

#[test]
fn trigger_ignores_the_release() {
    let out = render(&two_loops(), &[(0.0, PRESS), (0.3, RELEASE)], 2.1);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, 4 * BEAT]);
}

#[test]
fn toggle_stops_on_the_second_press_and_a_third_takes_the_stop_back() {
    let p = two_loops_with(mode(LaunchMode::Toggle));
    // The second press stops the clip at the next bar.
    let out = render(&p, &[(0.0, PRESS), (0.3, PRESS)], 3.0);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT]);
    // A third press before that bar takes the stop back: the clip plays on.
    let out = render(&p, &[(0.0, PRESS), (0.3, PRESS), (0.4, PRESS)], 2.6);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, 4 * BEAT, 5 * BEAT]);
    // Toggled off, the next press launches it again at the next bar.
    let out = render(&p, &[(0.0, PRESS), (0.3, PRESS), (2.3, PRESS)], 4.6);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, 2 * BAR, 2 * BAR + BEAT]);
}

#[test]
fn toggle_on_a_queued_launch_takes_it_back() {
    let p = two_loops_with(mode(LaunchMode::Toggle));
    let other = LaunchCommand::LaunchClip { column: 0, slot: 1 };
    // Scene 2 plays; the toggle clip is pressed twice before the bar: scene 2 plays on.
    let out = render(&p, &[(0.0, other), (0.3, PRESS), (0.4, PRESS)], 2.6);
    let off = BEAT / 2;
    let expected: Vec<usize> = (0..5).map(|b| b * BEAT + off).collect();
    assert_onsets(&out, &expected);
}

#[test]
fn gate_plays_while_held_and_stops_at_the_boundary_after_the_release() {
    let p = two_loops_with(mode(LaunchMode::Gate));
    // Released in beat 2: the stop waits for the global bar.
    let out = render(&p, &[(0.0, PRESS), (0.8, RELEASE)], 3.0);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT]);
    // With its own quantisation of a quarter note, it stops at beat 2.
    let quarter = LaunchSettings { quantization: LaunchQuantization::Note(4), ..mode(LaunchMode::Gate) };
    let out = render(&two_loops_with(quarter), &[(0.0, PRESS), (0.8, RELEASE)], 3.0);
    assert_onsets(&out, &[0, BEAT]);
    // Held to the end, it plays on.
    let out = render(&p, &[(0.0, PRESS)], 2.6);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, 4 * BEAT, 5 * BEAT]);
}

#[test]
fn gate_released_before_its_launch_never_plays() {
    let p = two_loops_with(mode(LaunchMode::Gate));
    let other = LaunchCommand::LaunchClip { column: 0, slot: 1 };
    let out = render(&p, &[(0.0, other), (0.3, PRESS), (0.4, RELEASE)], 2.6);
    let off = BEAT / 2;
    let expected: Vec<usize> = (0..5).map(|b| b * BEAT + off).collect();
    assert_onsets(&out, &expected);
}

/// One column with a 4-beat loop that has a note only on its first beat.
fn one_note_bar(settings: LaunchSettings) -> Project {
    let mut long = clip(1, 4.0, true);
    long.launch = settings;
    project(vec![notes(1, vec![note(0.0, 60)])], vec![vec![with(long)]])
}

#[test]
fn repeat_relaunches_at_every_step_while_held_and_plays_on_after() {
    let repeat = LaunchSettings { quantization: LaunchQuantization::Note(4), ..mode(LaunchMode::Repeat) };
    // Held until beat 2.2: launched at beats 0, 1 and 2, then it plays on from there, and its
    // note comes round again a whole loop after the last launch.
    let out = render(&one_note_bar(repeat), &[(0.0, PRESS), (1.1, RELEASE)], 3.2);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 6 * BEAT]);
    // With the global bar it repeats every bar.
    let out = render(&one_note_bar(mode(LaunchMode::Repeat)), &[(0.0, PRESS)], 4.1);
    assert_onsets(&out, &[0, BAR, 2 * BAR]);
    // With no quantisation there is nothing to repeat on: it plays once, like trigger.
    let none = LaunchSettings { quantization: LaunchQuantization::None, ..mode(LaunchMode::Repeat) };
    let out = render(&one_note_bar(none), &[(0.0, PRESS)], 2.1);
    assert_onsets(&out, &[0, BAR]);
}

#[test]
fn another_launch_in_the_column_ends_the_hold() {
    let repeat = LaunchSettings { quantization: LaunchQuantization::Note(4), ..mode(LaunchMode::Repeat) };
    let mut p = one_note_bar(repeat);
    p.launcher.columns[0].slots.push(with(clip(2, 1.0, true)));
    p.contents.push(notes(2, vec![note(0.5, 72)]));
    let other = LaunchCommand::LaunchClip { column: 0, slot: 1 };
    // The repeat clip is held and relaunched at beat 1. Scene 2, launched in beat 2 on the
    // global bar, takes the place of the next repeat in the queue and plays from beat 4 on; the
    // held button no longer brings the repeat clip back.
    let out = render(&p, &[(0.0, PRESS), (0.6, other)], 3.1);
    let off = BEAT / 2;
    assert_onsets(&out, &[0, BEAT, BAR + off, BAR + BEAT + off]);
}

/// One column: scene 1 loops notes on beats 0 to 3, scene 2 has notes at 0 and 2.5.
fn legato_pair(legato: bool) -> Project {
    let mut second = clip(2, 4.0, true);
    second.launch = LaunchSettings { legato, quantization: LaunchQuantization::Note(4), ..LaunchSettings::default() };
    project(
        vec![notes(1, (0..4).map(|b| note(f64::from(b), 60)).collect()), notes(2, vec![note(0.0, 72), note(2.5, 72)])],
        vec![vec![with(clip(1, 4.0, true)), with(second)]],
    )
}

#[test]
fn legato_takes_over_the_position_of_the_clip_it_replaces() {
    let commands = [(0.0, PRESS), (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 })];
    let half = BEAT / 2;
    // Without legato scene 2 starts from its start at beat 1: notes at 1 and 3.5.
    let out = render(&legato_pair(false), &commands, 2.6);
    assert_onsets(&out, &[0, BEAT, 3 * BEAT + half, 5 * BEAT]);
    // With legato it starts at its own beat 1: notes at 2.5, then 4 when its loop comes round.
    let out = render(&legato_pair(true), &commands, 2.6);
    assert_onsets(&out, &[0, 2 * BEAT + half, 4 * BEAT]);
    // On an empty column there is nothing to take over: it starts from its start.
    let out = render(&legato_pair(true), &[(0.0, LaunchCommand::LaunchClip { column: 0, slot: 1 })], 1.5);
    assert_onsets(&out, &[0, 2 * BEAT + half]);
}

#[test]
fn a_clip_quantisation_overrides_the_global_one() {
    let other = LaunchCommand::LaunchClip { column: 0, slot: 1 };
    let set = |q: LaunchQuantization| {
        let mut p = two_loops();
        if let Some(clip) = p.launcher.columns[0].slots[1].clip.as_mut() {
            clip.launch.quantization = q;
        }
        p
    };
    let off = BEAT / 2;
    // No quantisation: scene 2 starts at 0.3 s although the global grid is a bar.
    let at = SR_FRAMES_03;
    let out = render(&set(LaunchQuantization::None), &[(0.0, PRESS), (0.3, other)], 1.3);
    assert_onsets(&out, &[0, at + off, at + BEAT + off]);
    // A bar, while the global grid is a quarter note: it waits for beat 4.
    let quarter = LaunchCommand::SetQuantization(Quantization::Beats(1.0));
    let out = render(&set(LaunchQuantization::Bars(1)), &[(0.0, quarter), (0.0, PRESS), (0.3, other)], 2.3);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off]);
    // Global follows the launcher's grid as it is set.
    let commands = [(0.0, LaunchCommand::SetQuantization(Quantization::Beats(1.0))), (0.0, PRESS), (0.3, other)];
    let out = render(&set(LaunchQuantization::Global), &commands, 1.3);
    assert_onsets(&out, &[0, BEAT + off, 2 * BEAT + off]);
    // A scene launch launches each clip on its own grid.
    let scene = [(0.0, PRESS), (0.3, LaunchCommand::LaunchScene { scene: 1 })];
    let out = render(&set(LaunchQuantization::None), &scene, 1.3);
    assert_onsets(&out, &[0, at + off, at + BEAT + off]);
}

/// 0.3 seconds, in frames.
const SR_FRAMES_03: usize = super::super::tests::SR as usize * 3 / 10;

#[test]
fn hostile_presses_and_releases_are_ignored() {
    let p = two_loops_with(mode(LaunchMode::Gate));
    let commands = [
        (0.0, LaunchCommand::ReleaseClip { column: 0, slot: 0 }),
        (0.0, LaunchCommand::ReleaseClip { column: 9, slot: 0 }),
        (0.0, LaunchCommand::ReleaseClip { column: 0, slot: usize::MAX }),
        (0.0, PRESS),
        // A release of a slot that was not pressed does nothing.
        (0.1, LaunchCommand::ReleaseClip { column: 0, slot: 1 }),
    ];
    let out = render(&p, &commands, 1.1);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT]);
    assert!(out.iter().all(|s| s.is_finite()));
}

#[test]
fn legato_audio_continues_from_the_replaced_clips_position() {
    use std::sync::Arc;

    use crate::{AudioData, AudioPool, render_launches};
    let sr = super::super::tests::SR;
    // A one-second ramp: the sample value tells the file position in seconds.
    let samples = (0..sr).map(|i| i as f32 / sr as f32).collect();
    let mut audio = AudioPool::new();
    audio.files.insert("ramp.wav".into(), Arc::new(AudioData { sample_rate: sr, channels: 1, samples }));
    let content = |id: u32| ClipContent {
        id: ContentId(id),
        name: "ramp".into(),
        length: 2.0,
        lanes: vec![ContentLane {
            target: None,
            data: LaneData::Audio(AudioRegion {
                sample_rate: Some(sr),
                channels: Some(1),
                duration_seconds: Some(1.0),
                ..AudioRegion::new("ramp.wav")
            }),
        }]
        .into(),
        extensions: Extensions::default(),
        scale: None,
    };
    let render_with = |legato: bool| {
        let mut second = clip(2, 2.0, true);
        second.launch = LaunchSettings { legato, quantization: LaunchQuantization::None, ..LaunchSettings::default() };
        second.audio.gain = 0.5;
        let p = project(vec![content(1), content(2)], vec![vec![with(clip(1, 2.0, true)), with(second)]]);
        let commands = [(0.0, PRESS), (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 })];
        render_launches(&p, &audio, sr, &commands, 0.5, suhina_device_registry::factory()).unwrap()
    };
    let left = |out: &[f32], seconds: f64| out[2 * (seconds * f64::from(sr)) as usize];
    // At 0.4 s the second clip (at half the level) is 0.1 s into its file without legato, and
    // 0.4 s into it with legato.
    let out = render_with(false);
    assert!((left(&out, 0.4) - 0.05).abs() < 0.005, "{}", left(&out, 0.4));
    let out = render_with(true);
    assert!((left(&out, 0.4) - 0.2).abs() < 0.005, "{}", left(&out, 0.4));
}
