// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{RenderInputs, RenderOptions};
use std::sync::Arc;

use suhina_model::*;

use super::*;
use crate::{AudioData, render_launches};

pub(super) const SR: u32 = 48_000;
/// Frames per beat at 120 bpm.
pub(super) const BEAT: usize = SR as usize / 2;
pub(super) const BAR: usize = 4 * BEAT;

pub(super) fn note(time: f64, key: u8) -> Note {
    Note { time, duration: 0.25, key, velocity: 1.0, release_velocity: 0.5, channel: 0, muted: false }
}

/// A clip of `length` beats that loops over its whole length, or plays once.
pub(super) fn clip(content: u32, length: f64, looping: bool) -> Clip {
    Clip {
        id: Default::default(),
        content: ContentId(content),
        start: 0.0,
        length,
        offset: 0.0,
        loop_range: looping.then_some(LoopRange { start: 0.0, end: length }),
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

pub(super) fn notes(id: u32, notes: Vec<Note>) -> ClipContent {
    ClipContent {
        id: ContentId(id),
        name: format!("c{id}"),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
        extensions: Extensions::default(),
        scale: None,
    }
}

/// Tracks 1..=n with one launcher column each; `slots[column][scene]`.
pub(super) fn project(contents: Vec<ClipContent>, slots: Vec<Vec<Slot>>) -> Project {
    let n = u32::try_from(slots.len()).unwrap();
    let scenes = slots.iter().map(Vec::len).max().unwrap_or(0);
    Project {
        tracks: (1..=n)
            .map(|i| Track {
                id: TrackId(i),
                name: format!("T{i}"),
                color: None,
                content: ContentKinds { notes: true, audio: true },
                channel: Some(ChannelId(i)),
                parent: None,
                midi_input: Default::default(),
                audio_input: Default::default(),
                extensions: Extensions::default(),
            })
            .collect(),
        channels: (1..=n)
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
        contents,
        launcher: suhina_model::Launcher {
            mappings: Vec::new(),
            scenes: vec![Scene::default(); scenes],
            columns: slots
                .into_iter()
                .zip(1..)
                .map(|(slots, track)| LauncherColumn { track: TrackId(track), slots })
                .collect(),
        },
        ..Project::default()
    }
}

pub(super) fn with(clip: Clip) -> Slot {
    Slot { clip: Some(clip), has_stop: true }
}

/// Frames where sound starts after more than 100 silent frames.
pub(super) fn onsets(out: &[f32]) -> Vec<usize> {
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

pub(super) fn assert_onsets(out: &[f32], expected: &[usize]) {
    let actual = onsets(out);
    assert_eq!(actual.len(), expected.len(), "onsets {actual:?}, expected {expected:?}");
    for (a, e) in actual.iter().zip(expected) {
        assert!(a.abs_diff(*e) <= 2, "onsets {actual:?}, expected {expected:?}");
    }
}

pub(super) fn render(project: &Project, commands: &[(f64, LaunchCommand)], seconds: f64) -> Vec<f32> {
    render_launches(project, &AudioPool::new(), SR, commands, seconds).unwrap()
}

/// One column: scene 1 loops a note on every beat, scene 2 a note on every off-beat.
pub(super) fn two_loops() -> Project {
    project(
        vec![notes(1, vec![note(0.0, 60)]), notes(2, vec![note(0.5, 72)])],
        vec![vec![with(clip(1, 1.0, true)), with(clip(2, 1.0, true))]],
    )
}

#[test]
fn nothing_plays_until_something_is_launched() {
    assert!(render(&two_loops(), &[], 2.0).iter().all(|s| *s == 0.0));
}

#[test]
fn a_muted_note_is_never_planned_by_the_launcher() {
    // A one-beat loop with a note on the beat and one off it. Muting the off-beat note leaves
    // half the notes sounding; the launcher never plans a voice for it.
    let with_offbeat_muted = || {
        let mut content = notes(1, vec![note(0.0, 60), note(0.5, 72)]);
        if let Some(LaneData::Notes(list)) = content.lanes.first_mut().map(|lane| &mut lane.data)
            && let Some(note) = list.get_mut(1)
        {
            note.muted = true;
        }
        project(vec![content], vec![vec![with(clip(1, 1.0, true))]])
    };
    let unmuted = || project(vec![notes(1, vec![note(0.0, 60), note(0.5, 72)])], vec![vec![with(clip(1, 1.0, true))]]);
    let launch = [(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })];

    let heard = render(&unmuted(), &launch, 2.0);
    let quiet = render(&with_offbeat_muted(), &launch, 2.0);
    assert_onsets(
        &heard,
        &[0, BEAT / 2, BEAT, BEAT + BEAT / 2, 2 * BEAT, 2 * BEAT + BEAT / 2, 3 * BEAT, 3 * BEAT + BEAT / 2],
    );
    assert_onsets(&quiet, &[0, BEAT, 2 * BEAT, 3 * BEAT]);
    assert_eq!(onsets(&quiet).len() * 2, onsets(&heard).len());
}

#[test]
fn launching_while_stopped_starts_at_once_and_the_clip_loops() {
    let out = render(&two_loops(), &[(0.1, LaunchCommand::LaunchClip { column: 0, slot: 0 })], 2.1);
    let start = SR as usize / 10;
    assert_onsets(&out, &[start, start + BEAT, start + 2 * BEAT, start + 3 * BEAT]);
}

#[test]
fn a_launch_waits_for_the_next_bar_and_replaces_the_playing_clip() {
    let commands = [
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 }),
    ];
    let out = render(&two_loops(), &commands, 3.0);
    // Clip 1 on beats 0-3, then clip 2 from bar 2 on its off-beats.
    let off = BEAT / 2;
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + BEAT + off]);
}

#[test]
fn quantisation_can_be_a_note_value_or_off() {
    let mut commands = vec![
        (0.0, LaunchCommand::SetQuantization(Quantization::Beats(1.0))),
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 }),
    ];
    let out = render(&two_loops(), &commands, 1.5);
    // The second launch waits for beat 2 (0.5 s at 120 bpm) instead of the bar.
    assert_onsets(&out, &[0, BEAT + BEAT / 2, 2 * BEAT + BEAT / 2]);

    commands[0].1 = LaunchCommand::SetQuantization(Quantization::None);
    commands[2].0 = 0.1;
    let out = render(&two_loops(), &commands, 1.0);
    // Clip 2 starts at 0.1 s from its beginning: its off-beat note follows a quarter second later.
    let at = SR as usize / 10;
    assert_onsets(&out, &[0, at + BEAT / 2, at + BEAT + BEAT / 2]);
}

#[test]
fn the_bar_follows_the_time_signature() {
    let mut p = two_loops();
    p.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
    let commands = [
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        (0.3, LaunchCommand::LaunchClip { column: 0, slot: 1 }),
    ];
    let out = render(&p, &commands, 2.0);
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT + BEAT / 2]);
}

#[test]
fn stops_are_quantised_and_transport_stop_is_immediate() {
    let out = render(
        &two_loops(),
        &[(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }), (0.3, LaunchCommand::StopColumn { column: 0 })],
        3.0,
    );
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT]);

    let out = render(
        &two_loops(),
        &[(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }), (0.6, LaunchCommand::Stop)],
        3.0,
    );
    assert_onsets(&out, &[0, BEAT]);
    // Silent soon after the stop: only the 50 ms release tail is left.
    let after = (0.6 + 0.06) * f64::from(SR);
    assert!(out.iter().skip(after as usize * 2).all(|s| *s == 0.0));
}

#[test]
fn scenes_launch_their_row_and_stop_buttons_stop_columns() {
    // Column 1: scene 1 clip, scene 2 empty with a stop button.
    // Column 2: scene 1 clip, scene 2 empty without one, so it keeps playing.
    let p = project(
        vec![notes(1, vec![note(0.0, 60)]), notes(2, vec![note(0.5, 72)])],
        vec![
            vec![with(clip(1, 1.0, true)), Slot { clip: None, has_stop: true }],
            vec![with(clip(2, 1.0, true)), Slot { clip: None, has_stop: false }],
        ],
    );
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::LaunchScene { scene: 0 });
    let monitor = renderer.monitor();
    assert!(monitor.running());
    let mut block = vec![0.0f32; 2 * 256];
    renderer.process(&mut block);
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    assert_eq!(monitor.column(1).unwrap().playing, Some(0));

    renderer.handle(LaunchCommand::LaunchScene { scene: 1 });
    assert_eq!(monitor.column(0).unwrap().queued, Some(Queued::Stop));
    assert_eq!(monitor.column(1).unwrap().queued, None);
    let mut bar = vec![0.0f32; 2 * BAR];
    renderer.process(&mut bar);
    assert_eq!(monitor.column(0).unwrap().playing, None);
    assert_eq!(monitor.column(1).unwrap().playing, Some(0));
    assert!(monitor.column(0).unwrap().overrides_arrangement);
}

/// Stopping a scene stops every track whose slot in the row has a clip, at the next
/// quantisation point; a track whose slot in the row is empty keeps playing, and a row
/// without clips stops nothing.
#[test]
fn stopping_a_scene_stops_its_rows_clips_and_leaves_the_empty_slots_tracks() {
    // Column 1: scene 1 clip, scene 2 empty with a stop button.
    // Column 2: scene 1 clip, scene 2 empty without one, so it keeps playing.
    let p = project(
        vec![notes(1, vec![note(0.0, 60)]), notes(2, vec![note(0.5, 72)])],
        vec![
            vec![with(clip(1, 1.0, true)), Slot { clip: None, has_stop: true }],
            vec![with(clip(2, 1.0, true)), Slot { clip: None, has_stop: false }],
        ],
    );
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::LaunchScene { scene: 0 });
    let mut block = vec![0.0f32; 2 * 256];
    renderer.process(&mut block);
    let monitor = renderer.monitor();
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    assert_eq!(monitor.column(1).unwrap().playing, Some(0));

    renderer.handle(LaunchCommand::StopScene { scene: 0 });
    assert_eq!(monitor.column(0).unwrap().queued, Some(Queued::Stop));
    assert_eq!(monitor.column(1).unwrap().queued, Some(Queued::Stop));
    let mut bar = vec![0.0f32; 2 * BAR];
    renderer.process(&mut bar);
    assert_eq!(monitor.column(0).unwrap().playing, None);
    assert_eq!(monitor.column(1).unwrap().playing, None);
    // Scene 2's row has no clips: stopping it touches nothing.
    renderer.handle(LaunchCommand::StopScene { scene: 1 });
    assert_eq!(monitor.column(0).unwrap().queued, None);
    assert_eq!(monitor.column(1).unwrap().queued, None);
}

/// A slot whose clip is deactivated does nothing when launched, alone or in a scene: the
/// column playing keeps playing, where an empty slot's stop button would stop it (#154).
#[test]
fn a_deactivated_clip_neither_plays_nor_stops_its_column() {
    let off = Clip { enabled: false, ..clip(2, 1.0, true) };
    let p = project(
        vec![notes(1, vec![note(0.0, 60)]), notes(2, vec![note(0.5, 72)])],
        vec![vec![with(clip(1, 1.0, true)), with(off)]],
    );
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::LaunchScene { scene: 0 });
    let mut out = vec![0.0f32; 2 * BAR];
    renderer.process(&mut out);
    renderer.handle(LaunchCommand::LaunchScene { scene: 1 });
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 1 });
    let monitor = renderer.monitor();
    assert_eq!(monitor.column(0).unwrap().queued, None);
    let mut out = vec![0.0f32; 2 * 2 * BAR];
    renderer.process(&mut out);
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    assert_onsets(&out, &(0..8).map(|b| b * BEAT).collect::<Vec<_>>());
}

/// A launched note with a NaN velocity is silent instead of making the mix NaN (#161).
#[test]
fn a_nan_velocity_note_leaves_the_launcher_finite() {
    let bad = Note { velocity: f32::NAN, ..note(0.0, 60) };
    let p = project(vec![notes(1, vec![bad, note(0.5, 72)])], vec![vec![with(clip(1, 1.0, true))]]);
    let out = render(&p, &[(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })], 1.0);
    assert!(out.iter().all(|s| s.is_finite()));
    assert_onsets(&out, &[BEAT / 2, BEAT + BEAT / 2]);
}

/// A launched clip whose offset is past its loop end starts at the loop start (#166).
#[test]
fn a_launched_offset_past_the_loop_end_starts_at_the_loop_start() {
    let late = Clip { offset: 1.75, ..clip(1, 1.0, true) };
    let p = project(vec![notes(1, vec![note(0.0, 60), note(0.5, 72)])], vec![vec![with(late)]]);
    let out = render(&p, &[(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })], 1.0);
    assert_onsets(&out, &[0, BEAT / 2, BEAT, BEAT + BEAT / 2]);
}

#[test]
fn a_scene_tempo_changes_the_tempo_at_the_launch() {
    let mut p = two_loops();
    p.launcher.scenes[1].tempo_bpm = Some(60.0);
    let commands = [(0.0, LaunchCommand::LaunchScene { scene: 0 }), (0.3, LaunchCommand::LaunchScene { scene: 1 })];
    let out = render(&p, &commands, 5.0);
    // From bar 2 a beat lasts a second: off-beats at 2.5 s and 3.5 s.
    let second = SR as usize;
    let off = second / 2;
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + second + off, BAR + 2 * second + off]);
}

#[test]
fn a_clip_without_a_loop_plays_once_and_the_column_stops() {
    let p = project(vec![notes(1, vec![note(0.0, 60), note(1.0, 64)])], vec![vec![with(clip(1, 2.0, false))]]);
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let monitor = renderer.monitor();
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
    let mut out = vec![0.0f32; 2 * 3 * BEAT];
    renderer.process(&mut out);
    assert_onsets(&out, &[0, BEAT]);
    assert_eq!(monitor.column(0).unwrap().playing, None);
    // The transport keeps running.
    assert!(monitor.running());
}

#[test]
fn notes_are_cut_at_the_loop_end() {
    let long = Note { duration: 4.0, ..note(0.0, 60) };
    let p = project(vec![notes(1, vec![long])], vec![vec![with(clip(1, 1.0, true))]]);
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
    let mut out = vec![0.0f32; 2 * (BEAT + 100)];
    renderer.process(&mut out);
    // Two voices: the first pass releasing and the second pass sounding.
    let sounding = renderer.voices.iter().filter(|v| v.active).count();
    assert_eq!(sounding, 2);
    assert!(renderer.voices.iter().filter(|v| v.active).any(|v| v.end <= BEAT as u64 + 1));
}

#[test]
fn a_launched_clip_takes_its_track_over_from_the_arrangement() {
    let mut p = two_loops();
    let looped = Clip { length: 32.0, ..clip(1, 1.0, true) };
    p.arrangement.lanes.push(ArrangementLane {
        id: Default::default(),
        name: String::new(),
        track: Some(TrackId(1)),
        clips: vec![looped],
    });
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    // The arrangement plays every beat of bar 1, then the off-beat clip takes over at bar 2.
    let mut out = vec![0.0f32; 2 * (BAR + 2 * BEAT)];
    let (first, rest) = out.split_at_mut(2 * (BEAT / 4));
    renderer.process(first);
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 1 });
    renderer.process(rest);
    let off = BEAT / 2;
    assert_onsets(&out, &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + BEAT + off]);

    // Stopping the clip (at the next bar) leaves the track silent until it goes back to the
    // arrangement.
    renderer.handle(LaunchCommand::StopColumn { column: 0 });
    let mut out = vec![0.0f32; 2 * 2 * BAR];
    renderer.process(&mut out);
    assert_eq!(onsets(&out).len(), 2, "{:?}", onsets(&out));
    renderer.handle(LaunchCommand::BackToArrangement);
    let mut out = vec![0.0f32; 2 * BAR];
    renderer.process(&mut out);
    assert_eq!(onsets(&out).len(), 4);
}

/// Back to arrangement while a launched clip plays stops that clip, so the track plays its
/// arrangement alone rather than both at once (#153).
#[test]
fn back_to_arrangement_stops_the_launched_clip() {
    let mut p = two_loops();
    let looped = Clip { length: 32.0, ..clip(1, 1.0, true) };
    p.arrangement.lanes.push(ArrangementLane {
        id: Default::default(),
        name: String::new(),
        track: Some(TrackId(1)),
        clips: vec![looped],
    });
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut out = vec![0.0f32; 2 * (BAR + BEAT)];
    let (first, rest) = out.split_at_mut(2 * (BEAT / 4));
    renderer.process(first);
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 1 });
    renderer.process(rest);
    renderer.handle(LaunchCommand::BackToArrangement);
    let status = renderer.monitor().column(0).unwrap();
    assert_eq!((status.playing, status.queued, status.overrides_arrangement), (None, None, false));
    // Only the arrangement's on-beat notes sound from here, none of the clip's off-beats.
    let mut out = vec![0.0f32; 2 * BAR];
    renderer.process(&mut out);
    let found = onsets(&out);
    assert!(found.len() >= 3, "{found:?}");
    for onset in &found {
        let from_beat = (onset + BEAT / 2) % BEAT;
        assert!(from_beat.abs_diff(BEAT / 2) <= 2, "an off-beat onset at {onset}: {found:?}");
    }
}

/// A clip resumed on a new renderer from what the monitor reports plays on exactly where
/// the old one was, and the play start marker stays (#178).
#[test]
fn a_resumed_clip_plays_on_where_it_was() {
    const BLOCK: usize = 512;
    let p = project(
        vec![notes(1, vec![note(0.0, 60), note(1.0, 62), note(2.0, 64), note(3.0, 65)])],
        vec![vec![with(clip(1, 4.0, true))]],
    );
    let blocks = 9 * BEAT / BLOCK;
    let start = |renderer: &mut Renderer| {
        renderer.handle(LaunchCommand::SetPosition { beat: 1.0 });
        renderer.handle(LaunchCommand::Play);
        renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 0 });
    };
    let mut reference = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    start(&mut reference);
    let mut expected = vec![0.0f32; 2 * BLOCK * blocks];
    for block in expected.chunks_mut(2 * BLOCK) {
        reference.process(block);
    }

    // The old renderer plays 5.3 beats from the marker, the clip launched at the next bar
    // (beat 4).
    let mut old = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    start(&mut old);
    let split = 53 * BEAT / 10 / BLOCK;
    let mut out = vec![0.0f32; 2 * BLOCK * blocks];
    for block in out.chunks_mut(2 * BLOCK).take(split) {
        old.process(block);
    }
    let monitor = old.monitor();
    let (beat, marker) = (monitor.beat(), monitor.start_beat());
    let elapsed = monitor.elapsed_beats(0).unwrap();
    assert!(elapsed > 2.0 && elapsed < 2.5, "{elapsed}");
    assert_eq!(marker, 1.0);

    // The monitor's numbers are the last block's start: the new renderer plays from there.
    let mut new = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    for command in [
        LaunchCommand::SetPosition { beat: marker },
        LaunchCommand::Play,
        LaunchCommand::Locate { beat },
        LaunchCommand::ResumeColumn { column: 0, slot: Some(0), elapsed_beats: elapsed },
    ] {
        new.handle(command);
    }
    for block in out.chunks_mut(2 * BLOCK).skip(split - 1) {
        new.process(block);
    }
    let expected_onsets = onsets(&expected);
    assert_eq!(expected_onsets.len(), 6, "{expected_onsets:?}");
    assert_onsets(&out, &expected_onsets);
    let monitor = new.monitor();
    assert_eq!(monitor.start_beat(), 1.0, "the marker stays");
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    let status = monitor.column(0).unwrap();
    assert!(status.overrides_arrangement);
    // Stopping goes back to the marker, not to where the new renderer started.
    new.handle(LaunchCommand::Stop);
    assert_eq!(new.monitor().beat(), 1.0);
}

#[test]
fn locate_moves_the_playhead_but_not_the_marker() {
    let mut renderer = Renderer::stopped(&two_loops(), &AudioPool::new(), SR).unwrap();
    renderer.handle(LaunchCommand::SetPosition { beat: 2.0 });
    // Stopped: the playhead is at the marker, and stays there.
    renderer.handle(LaunchCommand::Locate { beat: 8.0 });
    assert_eq!(renderer.monitor().beat(), 2.0);
    renderer.handle(LaunchCommand::Play);
    renderer.handle(LaunchCommand::Locate { beat: 8.0 });
    renderer.handle(LaunchCommand::Locate { beat: f64::NAN });
    let mut block = vec![0.0f32; 2 * 512];
    renderer.process(&mut block);
    let monitor = renderer.monitor();
    assert!((8.0..8.1).contains(&monitor.beat()), "{}", monitor.beat());
    assert_eq!(monitor.start_beat(), 2.0);
    assert_eq!(monitor.elapsed_beats(0), None, "nothing plays");
}

#[test]
fn a_column_resumes_without_a_clip_and_the_tempo_carries_over() {
    let mut p = two_loops();
    let looped = Clip { length: 32.0, ..clip(1, 1.0, true) };
    p.arrangement.lanes.push(ArrangementLane {
        id: Default::default(),
        name: String::new(),
        track: Some(TrackId(1)),
        clips: vec![looped],
    });
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    // Stopped: ignored.
    renderer.handle(LaunchCommand::ResumeColumn { column: 0, slot: None, elapsed_beats: 0.0 });
    assert!(!renderer.monitor().column(0).unwrap().overrides_arrangement);
    renderer.handle(LaunchCommand::Play);
    for hostile in [
        LaunchCommand::ResumeColumn { column: 9, slot: None, elapsed_beats: 0.0 },
        LaunchCommand::ResumeColumn { column: 0, slot: Some(7), elapsed_beats: 1.0 },
        LaunchCommand::SetLaunchTempo { bpm: f64::NAN },
        LaunchCommand::SetLaunchTempo { bpm: 0.0 },
    ] {
        renderer.handle(hostile);
    }
    assert!(!renderer.monitor().column(0).unwrap().overrides_arrangement);
    assert_eq!(renderer.monitor().tempo_bpm(), 120.0);
    renderer.handle(LaunchCommand::ResumeColumn { column: 0, slot: None, elapsed_beats: f64::INFINITY });
    renderer.handle(LaunchCommand::SetLaunchTempo { bpm: 90.0 });
    let status = renderer.monitor().column(0).unwrap();
    assert_eq!((status.playing, status.overrides_arrangement), (None, true));
    // The track's arrangement stays silent, as after a stop of its launched clip.
    let mut out = vec![0.0f32; 2 * BAR];
    renderer.process(&mut out);
    assert!(onsets(&out).is_empty(), "{:?}", onsets(&out));
    assert_eq!(renderer.monitor().tempo_bpm(), 90.0);
}

#[test]
fn commands_from_a_controller_take_the_same_path() {
    let p = two_loops();
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    controller.send(LaunchCommand::LaunchClip { column: 0, slot: 0 }).unwrap();
    let mut out = vec![0.0f32; 2 * 2 * BEAT];
    for block in out.chunks_mut(2 * 128) {
        renderer.process(block);
    }
    assert_onsets(&out, &[0, BEAT]);
    let monitor = controller.monitor();
    assert!(monitor.running());
    assert!((monitor.beat() - 2.0).abs() < 0.01, "{}", monitor.beat());
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
}

/// A track added while the engine plays reports its column: the monitor other threads hold
/// is the first graph's, and it must grow with the grid (#155).
#[test]
fn a_column_added_while_playing_reports_its_state() {
    let p = two_loops();
    let mut grown = p.clone();
    let mut more = project(
        vec![notes(1, vec![note(0.0, 60)]), notes(2, vec![note(0.5, 72)])],
        vec![
            vec![with(clip(1, 1.0, true)), with(clip(2, 1.0, true))],
            vec![with(clip(1, 1.0, true)), with(clip(2, 1.0, true))],
        ],
    );
    std::mem::swap(&mut grown.tracks, &mut more.tracks);
    std::mem::swap(&mut grown.channels, &mut more.channels);
    std::mem::swap(&mut grown.launcher, &mut more.launcher);
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    controller.send(LaunchCommand::LaunchClip { column: 0, slot: 0 }).unwrap();
    let mut block = vec![0.0f32; 2 * 512];
    renderer.process(&mut block);
    assert_eq!(controller.monitor().columns(), 1);
    controller.update(&grown).unwrap();
    for _ in 0..200 {
        renderer.process(&mut block);
    }
    controller.send(LaunchCommand::LaunchClip { column: 1, slot: 1 }).unwrap();
    for _ in 0..200 {
        renderer.process(&mut block);
    }
    let monitor = controller.monitor();
    assert_eq!(monitor.columns(), 2);
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    assert_eq!(monitor.column(1).unwrap().playing, Some(1));
    assert_eq!(monitor.column(2), None);
}

#[test]
fn a_bar_before_where_a_scene_set_the_bars_starts_before_the_playhead() {
    // Off the map (a scene's time signature), bars count from beat 8; a locate back to beat
    // 5 is in the bar from beat 4, not at beat 8 after the playhead.
    let p = project(Vec::new(), vec![Vec::new()]);
    let map = Arc::new(p.transport.tempo_map(f64::from(SR)));
    let mut launcher = super::Launcher::new(&p, &AudioPool::new(), SR, &[], map);
    launcher.follows_map = false;
    launcher.bar_beats = 4.0;
    launcher.bar_origin = 8.0;
    launcher.bars_before = 2.0;
    launcher.beat = 5.0;
    let (bar, start, _) = launcher.bar();
    assert_eq!((bar, start), (1, 4.0));
    launcher.beat = 9.0;
    assert_eq!(launcher.bar().0, 2);
    assert_eq!(launcher.bar().1, 8.0);
}

#[test]
fn launched_audio_loops_from_its_start_marker() {
    // A one-second ramp: the sample value tells the file position in seconds.
    let samples = (0..SR).map(|i| i as f32 / SR as f32).collect();
    let mut audio = AudioPool::new();
    audio.files.insert("ramp.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
    // Warped at the file's own speed, so it is read directly rather than stretched.
    let region = AudioRegion {
        file: "ramp.wav".into(),
        absolute_file: None,
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(1.0),
        warped: true,
        stretch: StretchMode::Smooth,
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 2.0, audio_time: 1.0 }],
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    let content = ClipContent {
        id: ContentId(1),
        name: "ramp".into(),
        length: 2.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    };
    // Loop beats 0..1 (0..0.5 s of the file), starting at beat 0.5.
    let c = Clip { offset: 0.5, ..clip(1, 1.0, true) };
    let p = project(vec![content], vec![vec![with(c)]]);
    let launch = [(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })];
    let out = render_launches(&p, &audio, SR, &launch, 1.0).unwrap();
    // A centred track at unity gain: the left sample is the ramp value.
    let left = |frame: usize| out[2 * frame];
    assert!((left(BEAT / 4) - 0.375).abs() < 0.01, "{}", left(BEAT / 4));
    // After half a beat the loop wraps to its start.
    assert!((left(BEAT / 2 + BEAT / 4) - 0.125).abs() < 0.01, "{}", left(BEAT / 2 + BEAT / 4));
    assert!((left(BEAT + BEAT / 2 + BEAT / 4) - 0.125).abs() < 0.01);
}

#[test]
fn a_launched_clip_s_plan_leaves_its_stretcher_to_the_stretch_thread() {
    // Stretched to 1.5 times its length, so it needs a stretcher. A graph build makes the
    // plan without one: the preparer builds it on the stretch thread (`prepare`), never the
    // audio thread, and not at every graph build.
    let samples = (0..SR).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let mut audio = AudioPool::new();
    audio.files.insert("sine.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
    let region = AudioRegion {
        file: "sine.wav".into(),
        absolute_file: None,
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(1.0),
        warped: true,
        stretch: StretchMode::Smooth,
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 3.0, audio_time: 1.0 }],
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    let content = ClipContent {
        id: ContentId(1),
        name: "sine".into(),
        length: 3.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    };
    let c = clip(1, 3.0, true);
    let p = project(vec![content], vec![vec![with(c.clone())]]);
    let Some(plan) = plan_clip(&p, &audio, &[], &c, p.tracks[0].id, BEAT as f64, SR) else { panic!("no plan") };
    let Some(voice) = plan.audio.first().and_then(|a| a.stretch.as_ref()) else { panic!("no stretch voice") };
    assert!(!voice.is_prepared());
}

#[test]
fn launched_unwarped_audio_loops_over_the_whole_file() {
    // A sample with no known tempo in a slot (`docs/spec/browser.md` › Warping): unwarped,
    // looping over its whole length. A 0.75 s ramp is 1.5 beats at 120 bpm, not a whole bar.
    let frames = SR as usize * 3 / 4;
    let samples = (0..frames).map(|i| i as f32 / SR as f32).collect();
    let mut audio = AudioPool::new();
    audio.files.insert("hit.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
    let region = AudioRegion {
        file: "hit.wav".into(),
        absolute_file: None,
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(0.75),
        warped: false,
        stretch: StretchMode::default(),
        warp_markers: Vec::new(),
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    let content = ClipContent {
        id: ContentId(1),
        name: "hit".into(),
        length: 1.5,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    };
    let launch = [(0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 })];
    let looping = project(vec![content.clone()], vec![vec![with(clip(1, 1.5, true))]]);
    let out = render_launches(&looping, &audio, SR, &launch, 2.0).unwrap();
    let left = |out: &[f32], seconds: f64| out[2 * (seconds * f64::from(SR)) as usize];
    assert!((left(&out, 0.25) - 0.25).abs() < 0.01, "{}", left(&out, 0.25));
    // Past the end of the file it starts again from its beginning, pass after pass.
    assert!((left(&out, 1.0) - 0.25).abs() < 0.01, "{}", left(&out, 1.0));
    assert!((left(&out, 1.85) - 0.35).abs() < 0.01, "{}", left(&out, 1.85));
    // With its loop off the same clip plays once and the column falls silent.
    let once = project(vec![content], vec![vec![with(clip(1, 1.5, false))]]);
    let out = render_launches(&once, &audio, SR, &launch, 2.0).unwrap();
    assert!((left(&out, 0.25) - 0.25).abs() < 0.01);
    assert!(out[2 * SR as usize..].iter().all(|s| *s == 0.0), "silent after the end");
}

/// A project whose only slot loops a 440 Hz sine warped to play at 0.9 times its speed.
fn stretched_sine() -> (Project, AudioPool) {
    let samples = (0..2 * SR as usize)
        .map(|i| (std::f64::consts::TAU * 440.0 * i as f64 / f64::from(SR)).sin() as f32 * 0.5)
        .collect();
    let mut audio = AudioPool::new();
    audio.files.insert("sine.wav".into(), Arc::new(AudioData { sample_rate: SR, channels: 1, samples }));
    let region = AudioRegion {
        file: "sine.wav".into(),
        absolute_file: None,
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(2.0),
        warped: true,
        stretch: StretchMode::Smooth,
        // Two beats (1 s at 120 bpm) play 0.9 s of the file.
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 2.0, audio_time: 0.9 }],
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    let content = ClipContent {
        id: ContentId(1),
        name: "sine".into(),
        length: 2.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    };
    (project(vec![content], vec![vec![with(clip(1, 2.0, true))]]), audio)
}

/// Upward zero crossings per second in the left channel of `out[from..to]` (frames).
fn hz(out: &[f32], from: usize, to: usize) -> f64 {
    let left: Vec<f32> = out[2 * from..2 * to].iter().step_by(2).copied().collect();
    let crossings = left.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    crossings as f64 * f64::from(SR) / (to - from) as f64
}

#[test]
fn launched_warped_clips_are_stretched_and_keep_their_pitch() {
    let (p, audio) = stretched_sine();
    let launches = [
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        (1.0, LaunchCommand::Stop),
        (1.5, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    ];
    let out = render_launches(&p, &audio, SR, &launches, 2.5).unwrap();
    // Read directly at 0.9 times its speed the sine would sound at 396 Hz.
    let first = hz(&out, SR as usize / 5, 4 * SR as usize / 5);
    assert!((first - 440.0).abs() < 8.0, "{first} Hz");
    // Relaunched after a stop, the rewound voice plays again from the start.
    let again = hz(&out, 17 * SR as usize / 10, 23 * SR as usize / 10);
    assert!((again - 440.0).abs() < 8.0, "{again} Hz");
    // Between the stop and the relaunch: silence once the fade is over.
    assert!(out[2 * (11 * SR as usize / 10)..2 * (14 * SR as usize / 10)].iter().all(|s| *s == 0.0));
}

#[test]
fn after_a_tempo_change_warped_clips_follow_by_re_pitching() {
    let (mut p, audio) = stretched_sine();
    p.launcher.scenes[0].tempo_bpm = Some(60.0);
    let launches = [(0.0, LaunchCommand::LaunchScene { scene: 0 })];
    let out = render_launches(&p, &audio, SR, &launches, 2.0).unwrap();
    // At half the tempo the direct reader plays the file at 0.9 x 0.5 = 0.45 times its speed:
    // 198 Hz. (Keeping the pitch at another tempo needs
    // real-time retargeting: docs/spec/clip-launching.md › Tempo changes.)
    let pitch = hz(&out, SR as usize / 5, 9 * SR as usize / 5);
    assert!((pitch - 198.0).abs() < 6.0, "{pitch} Hz");
}

#[test]
fn stretched_arrangement_clips_rewind_with_the_transport_and_give_way_to_launches() {
    let (mut p, audio) = stretched_sine();
    let looped = p.launcher.columns[0].slots[0].clip.clone().unwrap();
    p.arrangement.lanes.push(ArrangementLane {
        id: Default::default(),
        name: String::new(),
        track: Some(TrackId(1)),
        clips: vec![Clip { length: 16.0, ..looped }],
    });
    // A second scene with a silent clip, to take the track over without sound of its own.
    p.contents.push(notes(2, Vec::new()));
    p.launcher.scenes.push(Scene::default());
    p.launcher.columns[0].slots.push(with(clip(2, 1.0, true)));
    let mut renderer = Renderer::build(&p, RenderInputs::from_audio(&audio), SR, RenderOptions::PLAYING).unwrap();
    let half = SR as usize / 2;
    let mut first = vec![0.0f32; 2 * half];
    renderer.process(&mut first);
    assert!((hz(&first, half / 4, half) - 440.0).abs() < 10.0);

    // Stop and play again: the stretched clip starts over, sample for sample.
    renderer.handle(LaunchCommand::Stop);
    let mut gap = vec![0.0f32; 2 * 4800];
    renderer.process(&mut gap);
    renderer.handle(LaunchCommand::Play);
    let mut again = vec![0.0f32; 2 * half];
    renderer.process(&mut again);
    let diff = first.iter().zip(&again).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(diff < 1e-4, "restart differs by {diff}");

    // Launching the silent clip at the next bar takes the track over: silence from there.
    renderer.handle(LaunchCommand::LaunchClip { column: 0, slot: 1 });
    let mut rest = vec![0.0f32; 2 * 2 * SR as usize];
    renderer.process(&mut rest);
    let bar = BAR - half;
    assert!(rest[..2 * (bar - 10)].iter().any(|s| s.abs() > 0.1));
    assert!(rest[2 * bar..].iter().all(|s| *s == 0.0), "the arrangement clip still sounds");
}

#[test]
fn hostile_commands_are_ignored() {
    let p = two_loops();
    let commands = [
        (f64::NAN, LaunchCommand::StopAll),
        (0.0, LaunchCommand::LaunchClip { column: 99, slot: 0 }),
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: usize::MAX }),
        (0.0, LaunchCommand::LaunchScene { scene: 7 }),
        (0.0, LaunchCommand::StopColumn { column: usize::MAX }),
        (0.0, LaunchCommand::SetQuantization(Quantization::Beats(f64::NAN))),
        (0.1, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    ];
    let out = render(&p, &commands, 0.5);
    assert_eq!(onsets(&out).len(), 1);
    assert!(render_launches(&p, &AudioPool::new(), SR, &[], f64::INFINITY).is_err());
}

#[test]
fn a_scene_time_signature_changes_the_bar_at_the_launch() {
    let mut p = two_loops();
    p.launcher.scenes[1].time_signature = Some(TimeSignature { numerator: 3, denominator: 4 });
    // Scene 2 waits for bar 2 (beat 4) and switches to 3/4 there, so the launch of scene 1
    // at beat 4.4 waits for beat 7, one 3/4 bar later, not for beat 8.
    let commands = [
        (0.0, LaunchCommand::LaunchScene { scene: 0 }),
        (0.3, LaunchCommand::LaunchScene { scene: 1 }),
        (2.2, LaunchCommand::LaunchScene { scene: 0 }),
    ];
    let out = render(&p, &commands, 4.5);
    let off = BEAT / 2;
    assert_onsets(
        &out,
        &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + BEAT + off, BAR + 2 * BEAT + off, 7 * BEAT, 8 * BEAT],
    );
}

#[test]
fn a_clip_s_own_bar_quantisation_counts_bars_from_a_scene_time_signature() {
    let mut p = two_loops();
    p.launcher.scenes[1].time_signature = Some(TimeSignature { numerator: 3, denominator: 4 });
    if let Some(clip) = p.launcher.columns[0].slots[0].clip.as_mut() {
        clip.launch.quantization = LaunchQuantization::Bars(1);
    }
    // Scene 2 switches to 3/4 at beat 4. With the global quantisation a quarter note, the
    // first clip, quantised to a bar of its own, launched at beat 5.2 waits for the 3/4 bar
    // line at beat 7, not for beat 6 (a 3-beat grid from beat 0).
    let commands = [
        (0.0, LaunchCommand::LaunchScene { scene: 0 }),
        (0.3, LaunchCommand::LaunchScene { scene: 1 }),
        (2.2, LaunchCommand::SetQuantization(Quantization::Beats(1.0))),
        (2.6, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
    ];
    let out = render(&p, &commands, 4.3);
    let off = BEAT / 2;
    assert_onsets(
        &out,
        &[0, BEAT, 2 * BEAT, 3 * BEAT, BAR + off, BAR + BEAT + off, BAR + 2 * BEAT + off, 7 * BEAT, 8 * BEAT],
    );
}

#[test]
fn the_meter_counts_bars_across_a_scene_time_signature() {
    let mut p = two_loops();
    p.launcher.scenes[1].time_signature = Some(TimeSignature { numerator: 3, denominator: 4 });
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let monitor = renderer.monitor();
    assert_eq!(monitor.meter(), PlayingMeter::default());
    renderer.handle(LaunchCommand::LaunchScene { scene: 1 });
    let mut out = vec![0.0f32; 2 * 5 * BEAT];
    renderer.process(&mut out);
    let meter = monitor.meter();
    let project = p.transport.meter();
    assert!(!meter.follows_map, "the scene's time signature takes the clock off the map");
    assert_eq!(meter.time_signature, TimeSignature { numerator: 3, denominator: 4 });
    assert_eq!((meter.origin, meter.bars_before), (0.0, 0.0));
    // Beat 5 is the second beat of bar 2 in 3/4.
    assert_eq!(meter.locate(5.0, &project), (2, 2.0));
    // A meter that changed at beat 6, after one 4/4 bar and half of another, counts both.
    let changed = PlayingMeter { origin: 6.0, bars_before: 2.0, ..meter };
    assert_eq!(changed.locate(6.0, &project), (3, 0.0));
    assert_eq!(changed.locate(10.0, &project), (4, 1.0));
    // Stopping keeps the time signature and counts bars from the start again.
    renderer.handle(LaunchCommand::Stop);
    renderer.process(&mut out[..2]);
    assert_eq!(monitor.meter().time_signature, TimeSignature { numerator: 3, denominator: 4 });
    // Starting again plays the project's bars: beat 5 is in bar 2 of 4/4.
    renderer.handle(LaunchCommand::Play);
    renderer.process(&mut out[..2]);
    let meter = monitor.meter();
    assert!(meter.follows_map);
    assert_eq!(meter.locate(5.0, &project), (2, 1.0));
}

#[test]
fn a_scene_shift_is_found_between_two_launchers() {
    let p = two_loops();
    let before = &p.launcher;
    let mut inserted = before.clone();
    inserted.scenes.insert(1, Scene { name: "New".into(), ..Scene::default() });
    for column in &mut inserted.columns {
        column.slots.insert(1, Slot { clip: None, has_stop: true });
    }
    assert_eq!(SceneShift::between(before, &inserted), Some(SceneShift::Inserted(1)));
    assert_eq!(SceneShift::between(&inserted, before), Some(SceneShift::Removed(1)));
    // A duplicate below its scene: the scene above keeps its index.
    let mut duplicated = before.clone();
    duplicated.scenes.insert(1, duplicated.scenes[0].clone());
    for column in &mut duplicated.columns {
        column.slots.insert(1, column.slots[0].clone());
    }
    assert_eq!(SceneShift::between(before, &duplicated), Some(SceneShift::Inserted(1)));
    // Anything else changed, or no scene count change: no shift.
    assert_eq!(SceneShift::between(before, before), None);
    let mut renamed = inserted.clone();
    renamed.scenes[0].name = "Other".into();
    assert_eq!(SceneShift::between(before, &renamed), None);
    assert_eq!(SceneShift::Removed(1).map(1), None);
    assert_eq!(SceneShift::Removed(1).map(2), Some(1));
    assert_eq!(SceneShift::Inserted(1).map(0), Some(0));
    assert_eq!(SceneShift::Inserted(1).map(usize::MAX), None);
}
