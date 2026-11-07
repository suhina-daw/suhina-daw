// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Follow actions in the launcher (`docs/spec/clip-launching.md` › Follow actions): offline
//! renders of chains of clips, sample-accurate at the follow time.

use suhina_model::*;

use crate::{AudioPool, LaunchCommand, Quantization, Renderer, render_launches};

const SR: u32 = 48_000;
/// Frames per beat at 120 bpm.
const BEAT: usize = SR as usize / 2;

fn note(time: f64, key: u8) -> Note {
    Note::new(time, 0.25, key, 1.0)
}

/// A slot clip of `length` beats playing `notes`, looping or once, with `follow`.
fn slot(content: u32, length: f64, looping: bool, follow: FollowAction) -> Slot {
    Slot {
        clip: Some(Clip {
            loop_range: looping.then_some(LoopRange { start: 0.0, end: length }),
            follow_action: follow,
            ..Clip::new(ContentId(content), 0.0, length)
        }),
        has_stop: true,
    }
}

/// One track whose column holds `slots`; content `i + 1` plays `notes[i]`.
fn project(notes: Vec<Vec<Note>>, slots: Vec<Slot>) -> Project {
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: true },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel::new(ChannelId(1), "C")],
        contents: notes
            .into_iter()
            .zip(1..)
            .map(|(notes, id)| ClipContent {
                id: ContentId(id),
                name: format!("c{id}"),
                length: 4.0,
                lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes.into()) }].into(),
                extensions: Extensions::default(),
                scale: None,
            })
            .collect(),
        launcher: Launcher {
            scenes: vec![Scene::default(); slots.len()],
            columns: vec![LauncherColumn { track: TrackId(1), slots }],
        },
        ..Project::default()
    }
}

fn follow(a: FollowKind) -> FollowAction {
    FollowAction { enabled: true, action_a: a, ..FollowAction::default() }
}

fn after(beats: f64, a: FollowKind) -> FollowAction {
    FollowAction { linked: false, time: beats, ..follow(a) }
}

fn render(project: &Project, commands: &[(f64, LaunchCommand)], seconds: f64) -> Vec<f32> {
    render_launches(project, &AudioPool::new(), SR, commands, seconds, suhina_device_registry::factory()).unwrap()
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

fn assert_onsets(out: &[f32], expected: &[usize]) {
    let actual = onsets(out);
    assert_eq!(actual.len(), expected.len(), "onsets {actual:?}, expected {expected:?}");
    for (a, e) in actual.iter().zip(expected) {
        assert!(a.abs_diff(*e) <= 2, "onsets {actual:?}, expected {expected:?}");
    }
}

const LAUNCH: LaunchCommand = LaunchCommand::LaunchClip { column: 0, slot: 0 };

#[test]
fn a_chain_of_clips_follows_on_at_each_clip_length() {
    // 1 beat, then 2 beats, then 1 beat that stops the column. Without follow actions the first
    // clip would loop on every beat.
    let p = project(
        vec![vec![note(0.0, 60)], vec![note(0.0, 64)], vec![note(0.0, 67)]],
        vec![
            slot(1, 1.0, true, follow(FollowKind::Next)),
            slot(2, 2.0, true, follow(FollowKind::Next)),
            slot(3, 1.0, true, follow(FollowKind::Stop)),
        ],
    );
    let out = render(&p, &[(0.0, LAUNCH)], 4.0);
    assert_onsets(&out, &[0, BEAT, 3 * BEAT]);
}

#[test]
fn the_follow_time_counts_from_the_quantised_launch() {
    // The transport runs; the launch at 0.1 s waits for bar 2 (beat 4). Two beats later the
    // clip jumps to scene 3, whose note is on the off-beat.
    let p = project(
        vec![vec![note(0.0, 60)], vec![], vec![note(0.5, 72)]],
        vec![
            slot(1, 1.0, true, after(2.0, FollowKind::Jump(2))),
            slot(2, 1.0, true, FollowAction::default()),
            slot(3, 1.0, true, FollowAction::default()),
        ],
    );
    let commands = [(0.0, LaunchCommand::Play), (0.1, LAUNCH)];
    let out = render(&p, &commands, 4.0);
    let half = BEAT / 2;
    assert_onsets(&out, &[4 * BEAT, 5 * BEAT, 6 * BEAT + half, 7 * BEAT + half]);

    // Quantisation off: the launch at 0.1 s happens at once, and so the follow action 2 beats later.
    let commands =
        [(0.0, LaunchCommand::SetQuantization(Quantization::None)), (0.0, LaunchCommand::Play), (0.1, LAUNCH)];
    let out = render(&p, &commands, 2.0);
    let start = SR as usize / 10;
    assert_onsets(&out, &[start, start + BEAT, start + 2 * BEAT + half, start + 3 * BEAT + half]);
}

#[test]
fn a_clip_that_plays_once_follows_on_at_its_end_at_the_latest() {
    // 2 beats without a loop, follow time 8 beats: "again" at its end marker, every 2 beats.
    let p = project(vec![vec![note(0.0, 60)]], vec![slot(1, 2.0, false, after(8.0, FollowKind::Again))]);
    let out = render(&p, &[(0.0, LAUNCH)], 3.0);
    assert_onsets(&out, &[0, 2 * BEAT, 4 * BEAT]);

    // "nothing": it stops at its end, as without a follow action.
    let p = project(vec![vec![note(0.0, 60)]], vec![slot(1, 2.0, false, follow(FollowKind::Nothing))]);
    let out = render(&p, &[(0.0, LAUNCH)], 3.0);
    assert_onsets(&out, &[0]);
}

#[test]
fn the_chances_choose_between_a_and_b() {
    let only_b = FollowAction { action_b: FollowKind::Stop, chance_a: 0, chance_b: 100, ..follow(FollowKind::Again) };
    let p = project(vec![vec![note(0.0, 60)]], vec![slot(1, 1.0, true, only_b)]);
    assert_onsets(&render(&p, &[(0.0, LAUNCH)], 2.0), &[0]);

    let only_a = FollowAction { chance_a: 100, chance_b: 0, ..only_b };
    let p = project(vec![vec![note(0.0, 60)]], vec![slot(1, 1.0, true, only_a)]);
    assert_onsets(&render(&p, &[(0.0, LAUNCH)], 1.9), &[0, BEAT, 2 * BEAT, 3 * BEAT]);
}

#[test]
fn a_launch_queued_by_hand_wins_over_the_follow_action() {
    // The clip would stop after 2 beats, but a launch of scene 2 is queued for the next bar.
    let p = project(
        vec![vec![note(0.0, 60)], vec![note(0.5, 72)]],
        vec![slot(1, 1.0, true, after(2.0, FollowKind::Stop)), slot(2, 1.0, true, FollowAction::default())],
    );
    let commands = [(0.0, LAUNCH), (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 })];
    let out = render(&p, &commands, 2.9);
    let half = BEAT / 2;
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, 4 * BEAT + half, 5 * BEAT + half]);
}

#[test]
fn random_actions_repeat_with_the_same_seed() {
    // Four clips with their note on a different fifth of the beat; "any" picks one every beat.
    let notes: Vec<Vec<Note>> = (0..4).map(|i| vec![note(f64::from(i) * 0.2, 60)]).collect();
    let slots = (1..=4).map(|c| slot(c, 1.0, true, follow(FollowKind::Any))).collect();
    let p = project(notes, slots);
    let seeded = |seed| render(&p, &[(0.0, LaunchCommand::SeedRandom(seed)), (0.0, LAUNCH)], 8.0);
    let first = seeded(7);
    assert_eq!(first, seeded(7));
    // The picks differ from beat to beat: not every onset is on the same fifth.
    let fifths: Vec<usize> = onsets(&first).iter().map(|f| (f % BEAT) / (BEAT / 5)).collect();
    assert!(fifths.iter().any(|s| *s != fifths[1]), "{fifths:?}");
    assert_ne!(onsets(&first), onsets(&seeded(8)));
}

#[test]
fn the_monitor_shows_the_slot_a_follow_action_launched() {
    let p = project(
        vec![vec![note(0.0, 60)], vec![note(0.0, 64)]],
        vec![slot(1, 1.0, true, follow(FollowKind::Next)), slot(2, 1.0, true, FollowAction::default())],
    );
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
    renderer.set_offline(true);
    renderer.handle(LAUNCH);
    let mut block = vec![0.0f32; 2 * BEAT];
    let monitor = renderer.monitor();
    renderer.process(&mut block[..2 * (BEAT - 10)]);
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    renderer.process(&mut block[..2 * 20]);
    assert_eq!(monitor.column(0).unwrap().playing, Some(1));
    // Scene 2 has no follow action: it keeps playing.
    for _ in 0..4 {
        renderer.process(&mut block);
    }
    assert_eq!(monitor.column(0).unwrap().playing, Some(1));
}
