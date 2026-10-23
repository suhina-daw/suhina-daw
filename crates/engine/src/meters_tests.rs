// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Level meters: what a track, a group and the main output read while a project plays.

use suhina_model::*;

use crate::buses_tests::grouped;
use crate::chain_tests::{SR, blocks, device, peak};
use crate::{Level, PEAK_FALL_DB_PER_SECOND, Renderer};

/// The amplitude of one centred note at unity volume and full velocity (−12 dBFS).
const NOTE: f32 = 0.25;

/// `grouped`, with the note and its clips 16 beats (8 s) long: tracks 1 and 2 play into group
/// 3, track 4 plays straight to the output.
fn held(group_devices: Vec<Device>) -> Project {
    let mut project = grouped(group_devices, true);
    for content in &mut project.contents {
        content.length = 16.0;
        for lane in &mut content.lanes {
            if let LaneData::Notes(notes) = &mut lane.data {
                notes.iter_mut().for_each(|n| n.duration = 16.0);
            }
        }
    }
    for lane in &mut project.arrangement.lanes {
        lane.clips.iter_mut().for_each(|c| c.length = 16.0);
    }
    project
}

fn channel(project: &mut Project, id: u32) -> &mut Channel {
    project.channel_mut(ChannelId(id)).expect("channel")
}

/// Plays `seconds` of `project` in blocks of 256 frames; returns the renderer and the output.
fn play(project: &Project, seconds: f64) -> (Renderer, Vec<f32>) {
    let mut renderer = Renderer::new(project, SR).expect("renderer");
    let count = (seconds * f64::from(SR) / 256.0).round() as usize;
    let out = blocks(&mut renderer, count, 256);
    (renderer, out)
}

fn track(renderer: &Renderer, id: u32) -> Level {
    renderer.meter_monitor().track(TrackId(id)).expect("metered track")
}

fn close(value: f32, want: f32, tolerance: f32) -> bool {
    (value - want).abs() <= tolerance * want.abs().max(1e-6)
}

#[track_caller]
fn assert_level(level: Level, peak: [f32; 2], what: &str) {
    for side in 0..2 {
        assert!(close(level.peak[side], peak[side], 0.01), "{what} peak {:?}, want {peak:?}", level.peak);
        // A steady sine: its RMS is its peak over the square root of two.
        let rms = peak[side] / std::f32::consts::SQRT_2;
        assert!(close(level.rms[side], rms, 0.01), "{what} rms {:?}, want {rms}", level.rms);
    }
}

#[test]
fn a_render_reads_the_expected_peak_and_rms_of_every_track_group_and_the_main_output() {
    let (renderer, out) = play(&held(vec![]), 3.0);
    // Three notes in phase reach the output.
    let last_second = &out[out.len() - 2 * SR as usize..];
    assert!(close(peak(last_second), 3.0 * NOTE, 0.01), "{}", peak(last_second));
    assert_level(track(&renderer, 1), [NOTE; 2], "track 1");
    assert_level(track(&renderer, 2), [NOTE; 2], "track 2");
    // The group shows the sum of the two tracks inside it.
    assert_level(track(&renderer, 3), [2.0 * NOTE; 2], "group 3");
    assert_level(track(&renderer, 4), [NOTE; 2], "track 4");
    let main = renderer.meter_monitor().main();
    assert_level(main, [3.0 * NOTE; 2], "main");
    // The main meter's peak is the output's.
    assert!((main.peak[0] - peak(last_second)).abs() < 1e-3, "{:?}", main.peak);
    assert!(!main.clipped);
    let tracks: Vec<TrackId> = renderer.meter_monitor().tracks().iter().map(|(t, _)| *t).collect();
    assert_eq!(tracks, [TrackId(1), TrackId(2), TrackId(3), TrackId(4)]);
}

#[test]
fn a_track_meter_follows_its_own_volume_and_pan_not_the_group_or_main_volume() {
    let mut project = held(vec![]);
    channel(&mut project, 1).volume = 0.5;
    channel(&mut project, 3).volume = 0.5;
    channel(&mut project, 4).pan = 1.0;
    let template = channel(&mut project, 2).clone();
    project.channels.push(Channel {
        id: ChannelId(9),
        name: "Main".into(),
        role: ChannelRole::Master,
        volume: 0.5,
        ..template
    });
    let (renderer, _) = play(&project, 3.0);
    assert_level(track(&renderer, 1), [NOTE / 2.0; 2], "track 1 at half volume");
    assert_level(track(&renderer, 2), [NOTE; 2], "track 2");
    // The group: (half of one note plus one note) at the group's half volume.
    assert_level(track(&renderer, 3), [0.75 * NOTE; 2], "group 3 at half volume");
    // Fully right: nothing left, the right side up by the equal-power pan's 3 dB.
    let right = track(&renderer, 4);
    assert!(right.peak[0] < 1e-4, "{right:?}");
    assert!(close(right.peak[1], NOTE * std::f32::consts::SQRT_2, 0.01), "{right:?}");
    // The main channel's volume applies to the main output only (track 4 is on the right).
    let want = [0.5 * 0.75 * NOTE, 0.5 * (0.75 * NOTE + NOTE * std::f32::consts::SQRT_2)];
    let main = renderer.meter_monitor().main();
    assert!(close(main.peak[0], want[0], 0.01) && close(main.peak[1], want[1], 0.01), "{main:?}, want {want:?}");
}

#[test]
fn devices_are_measured_after_they_run_on_a_track_and_on_a_group() {
    let mut project = held(vec![device("utility", &[("gain", 6.0)])]);
    channel(&mut project, 4).devices = vec![device("utility", &[("gain", -6.0)])];
    let (renderer, _) = play(&project, 3.0);
    let half = 10f32.powf(-6.0 / 20.0);
    assert_level(track(&renderer, 1), [NOTE; 2], "track 1");
    assert_level(track(&renderer, 3), [2.0 * NOTE / half; 2], "group with +6 dB");
    assert_level(track(&renderer, 4), [NOTE * half; 2], "track with -6 dB");
    let main = renderer.meter_monitor().main();
    assert_level(main, [2.0 * NOTE / half + NOTE * half; 2], "main");
}

#[test]
fn a_track_that_is_not_heard_reads_silence() {
    let mut project = held(vec![]);
    channel(&mut project, 1).mute = true;
    channel(&mut project, 4).solo = true;
    let (renderer, _) = play(&project, 3.0);
    for id in [1, 2, 3] {
        assert_eq!(track(&renderer, id), Level::default(), "track {id}");
    }
    assert_level(track(&renderer, 4), [NOTE; 2], "soloed track 4");
}

#[test]
fn a_clip_stays_lit_until_reset_and_peaks_fall_once_the_sound_stops() {
    let mut project = held(vec![]);
    // +18 dB: a note at twice full scale on track 4 and at the output.
    channel(&mut project, 4).volume = 8.0;
    for lane in &mut project.arrangement.lanes {
        lane.clips.iter_mut().for_each(|c| c.length = 2.0);
    }
    let (mut renderer, _) = play(&project, 0.5);
    let monitor = renderer.meter_monitor();
    assert!(track(&renderer, 4).clipped);
    assert!(monitor.main().clipped);
    assert!(!track(&renderer, 1).clipped && !track(&renderer, 3).clipped);

    // The notes end at 1 s; a second later the peak has fallen by the fall rate, and the clip
    // indicator is still lit.
    blocks(&mut renderer, (1.5 * f64::from(SR) / 256.0) as usize, 256);
    let level = track(&renderer, 4);
    assert!(level.clipped);
    let fallen = 2.0 * 10f32.powf(-PEAK_FALL_DB_PER_SECOND as f32 * 0.95 / 20.0);
    assert!(level.peak[0] > 0.0 && level.peak[0] < fallen, "{level:?}");
    // The RMS level falls more slowly, but never reads above the peak.
    assert!(level.rms[0] > 0.0 && level.rms[0] <= level.peak[0], "{level:?}");

    assert!(monitor.reset_clip(TrackId(4)));
    assert!(!track(&renderer, 4).clipped);
    assert!(monitor.main().clipped);
    monitor.reset_all_clips();
    assert!(!monitor.main().clipped);
    assert!(!monitor.reset_clip(TrackId(99)));

    // Long enough to fall below −120 dB: silence reads exactly zero.
    blocks(&mut renderer, (6.0 * f64::from(SR) / 256.0) as usize, 256);
    assert_eq!(track(&renderer, 4), Level::default());
    assert_eq!(monitor.main(), Level::default());
}

#[test]
fn meters_follow_their_tracks_through_an_edit_while_playing() {
    let project = held(vec![]);
    let mut renderer = Renderer::new(&project, SR).expect("renderer");
    let mut controller = renderer.controller();
    blocks(&mut renderer, 200, 256);
    // A new track first: every slot moves, and each meter stays with its track.
    let mut edited = project.clone();
    edited.tracks.insert(
        0,
        Track {
            id: TrackId(7),
            name: "new".into(),
            channel: Some(ChannelId(7)),
            parent: None,
            ..edited.tracks[3].clone()
        },
    );
    edited.channels.push(Channel { id: ChannelId(7), ..edited.channels[3].clone() });
    channel(&mut edited, 4).volume = 0.5;
    controller.update(&edited).expect("update");
    blocks(&mut renderer, 400, 256);
    let monitor = controller.meter_monitor();
    let tracks: Vec<TrackId> = monitor.tracks().iter().map(|(t, _)| *t).collect();
    assert_eq!(tracks, [TrackId(7), TrackId(1), TrackId(2), TrackId(3), TrackId(4)]);
    assert_eq!(monitor.track(TrackId(7)), Some(Level::default()));
    // The playing note on track 4 follows its new volume, and its meter with it.
    let level = monitor.track(TrackId(4)).expect("track 4");
    assert!(close(level.peak[0], NOTE / 2.0, 0.01), "{level:?}");
    assert!(close(monitor.track(TrackId(3)).expect("group").peak[0], 2.0 * NOTE, 0.01));
}

#[test]
fn a_launched_clip_moves_its_track_meter() {
    let project = crate::chain_tests::project(vec![crate::chain_tests::note(0.0, 4.0, 69)], 4.0, vec![]);
    let mut renderer = Renderer::stopped(&project, &crate::AudioPool::new(), SR).expect("renderer");
    let mut controller = renderer.controller();
    blocks(&mut renderer, 20, 256);
    assert_eq!(controller.meter_monitor().track(TrackId(1)), Some(Level::default()));
    controller.send(crate::LaunchCommand::LaunchClip { column: 0, slot: 0 }).expect("launch");
    blocks(&mut renderer, (3.0 * f64::from(SR) / 256.0) as usize, 256);
    assert_level(controller.meter_monitor().track(TrackId(1)).expect("track 1"), [NOTE; 2], "launched");
}
