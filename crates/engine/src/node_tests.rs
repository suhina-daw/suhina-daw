// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering one node of the track tree (`render_offline_from` with a source): the sum at a node — a track
//! or a group — is everything that plays into it and the node's own devices, and nothing after
//! it. The graph is the one playback builds, so a node render lines up with the whole mix and
//! the stems sum back to it (`docs/spec/export.md` › Sources).

use crate::{RenderInputs, render_offline_from};
use suhina_model::*;

use crate::buses_tests::grouped;
use crate::chain_tests::{SR, device, max_diff, note, peak};
use crate::{AudioPool, DeviceAssets, Plugins, Project, TrackId, render_offline};

fn render(project: &Project, source: u32) -> Vec<f32> {
    render_offline_from(
        project,
        RenderInputs::from_audio(&AudioPool::new()).with_assets(&DeviceAssets::new()).with_plugins(Plugins::new()),
        SR,
        512,
        Some(TrackId(source)),
    )
    .unwrap_or_else(|e| panic!("{e}"))
}

/// The node tests' project: tracks 1 and 2 play the same one-beat note into group track 3
/// (channel 3, `group_devices`), group 3 plays into premaster track 5 (channel 5,
/// `premaster_devices`), premaster 5 and track 4 play into the master (channel 6, role Master,
/// `master_devices`), and the master is the top.
fn tree(group_devices: Vec<Device>, premaster_devices: Vec<Device>, master_devices: Vec<Device>) -> Project {
    let channel = |id: u32, output: Option<u32>, role: ChannelRole, devices: Vec<Device>| Channel {
        output: output.map(ChannelId),
        role,
        devices,
        ..Channel::new(ChannelId(id), format!("C{id}"))
    };
    let track = |id: u32, parent: Option<u32>| Track {
        content: ContentKinds { notes: parent.is_some() || id == 4, audio: false },
        channel: Some(ChannelId(id)),
        parent: parent.map(TrackId),
        ..Track::new(TrackId(id), format!("T{id}"))
    };
    let clip = Clip::new(ContentId(1), 0.0, 1.0);
    Project {
        tracks: vec![track(1, Some(3)), track(2, Some(3)), track(3, Some(5)), track(5, None), track(4, None)],
        channels: vec![
            channel(1, Some(3), ChannelRole::Regular, vec![]),
            channel(2, Some(3), ChannelRole::Regular, vec![]),
            channel(3, Some(5), ChannelRole::Submix, group_devices),
            channel(5, Some(6), ChannelRole::Submix, premaster_devices),
            channel(6, None, ChannelRole::Master, master_devices),
            channel(4, Some(5), ChannelRole::Regular, vec![]),
        ],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note(0.0, 1.0, 69)]) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: [1u32, 2, 4]
                .iter()
                .map(|&t| ArrangementLane {
                    id: Default::default(),
                    name: format!("l{t}"),
                    track: Some(TrackId(t)),
                    clips: vec![clip.clone()],
                })
                .collect(),
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

/// `tree` with no devices, plus a return track 7 (channel 7, role Effect, into the master) that
/// channel 1 sends to at `level`, carrying `devices`.
fn with_send(level: f64, devices: Vec<Device>) -> Project {
    let mut project = tree(vec![], vec![], vec![]);
    project.tracks.push(Track { channel: Some(ChannelId(7)), ..Track::new(TrackId(7), "Echo") });
    project.channels.push(Channel {
        output: Some(ChannelId(6)),
        role: ChannelRole::Effect,
        devices,
        ..Channel::new(ChannelId(7), "C7")
    });
    project.channels[0].sends = vec![Send { target: ChannelId(7), level, pre_fader: false }];
    project
}

fn mix(files: &[&[f32]]) -> Vec<f32> {
    let mut out = files.first().map_or_else(Vec::new, |first| first.to_vec());
    for more in files.iter().skip(1) {
        for (into, from) in out.iter_mut().zip(*more) {
            *into += from;
        }
    }
    out
}

#[test]
fn a_group_renders_its_children_its_own_devices_and_nothing_after_them() {
    // The group's device doubles; the premaster's halves what is after it.
    let project =
        tree(vec![device("utility", &[("gain", 6.0206)])], vec![device("utility", &[("gain", -6.0206)])], vec![]);
    let single = render(&project, 1);
    assert!(peak(&single) > 0.05, "{}", peak(&single));
    let group = render(&project, 3);
    // Both tracks through the group's device; the premaster's device is after the node.
    assert!((peak(&group) - 4.0 * peak(&single)).abs() < 1e-4, "{} against {}", peak(&group), peak(&single));
    let premaster = render(&project, 5);
    // The premaster's own device is in, and nothing is outside it, so it is the whole mix.
    let whole = render_offline(&project, SR, 512).unwrap();
    assert!(max_diff(&premaster, &whole) < 1e-5, "{}", max_diff(&premaster, &whole));
    // The premaster's file is the group's through its own device, and track 4's through it too.
    let track4: Vec<f32> = render(&project, 4).iter().map(|s| s * 0.5).collect();
    let halved: Vec<f32> = group.iter().map(|s| s * 0.5).collect();
    let expected = mix(&[&halved, &track4]);
    assert!(max_diff(&expected, &premaster) < 1e-5, "{}", max_diff(&expected, &premaster));
    // One track: its own sound only, the group's device is after it.
    assert!((peak(&render(&project, 2)) - peak(&single)).abs() < 1e-6);
    // The node render is the whole mix's length, so the stems line up.
    assert_eq!(single.len(), whole.len());
}

#[test]
fn the_stems_sum_to_the_master_and_the_children_to_the_group() {
    let project = tree(vec![], vec![], vec![]);
    let (s1, s2, s4) = (render(&project, 1), render(&project, 2), render(&project, 4));
    let whole = render_offline(&project, SR, 512).unwrap();
    let stems = mix(&[&s1, &s2, &s4]);
    assert!(max_diff(&stems, &whole) < 1e-5, "{}", max_diff(&stems, &whole));
    // The group, which only sums here, is its children's sum; it and track 4 sum to the master.
    let group = render(&project, 3);
    let children = mix(&[&s1, &s2]);
    assert!(max_diff(&children, &group) < 1e-6, "{}", max_diff(&children, &group));
    let group_stems = mix(&[&group, &s4]);
    assert!(max_diff(&group_stems, &whole) < 1e-5, "{}", max_diff(&group_stems, &whole));
}

#[test]
fn a_channel_routed_into_the_master_without_naming_it_does_not_carry_its_fader() {
    // A channel with `output: None` plays into the master without a link to it; the master's
    // fader and mute are not in a node render all the same. The premaster (channel 5) is the
    // node, and the master (channel 6) sits at half volume.
    let mut project = tree(vec![], vec![], vec![]);
    project.channels[3].output = None;
    project.channels[4].volume = 0.5;
    let node = render(&project, 5);
    let whole = render_offline(&project, SR, 512).unwrap();
    // The mix is the node's render through the master's fader: half of it.
    let halved: Vec<f32> = node.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&halved, &whole) < 1e-5, "{}", max_diff(&halved, &whole));
    // The master's mute silences the mix and not the node's render.
    project.channels[4].mute = true;
    let silenced = render_offline(&project, SR, 512).unwrap();
    assert!(peak(&silenced) < 1e-6, "{}", peak(&silenced));
    assert!(peak(&render(&project, 5)) > 0.05, "{}", peak(&render(&project, 5)));
}

#[test]
fn latency_compensation_keeps_the_stems_in_line() {
    // Track 1's overdrive adds latency (31 frames); the other tracks have none.
    let mut project = tree(vec![], vec![], vec![]);
    project.channels[0].devices = vec![device("overdrive", &[])];
    let (s1, s2, s4) = (render(&project, 1), render(&project, 2), render(&project, 4));
    let whole = render_offline(&project, SR, 512).unwrap();
    // The stems sum to the master sample for sample, so they start at the same frame however
    // much their devices delay them.
    let stems = mix(&[&s1, &s2, &s4]);
    assert!(max_diff(&stems, &whole) < 1e-4, "{}", max_diff(&stems, &whole));
}

#[test]
fn a_node_render_starts_late_by_its_ancestors_latency() {
    let onset_of =
        |file: &[f32]| file.as_chunks::<2>().0.iter().take_while(|&[l, r]| l.abs() < 1e-4 && r.abs() < 1e-4).count();
    // Overdrive on the group (31 frames of latency): tracks 1 and 2 play through it into the
    // master, and undelayed track 4 plays the same content. A child's render, a top-level
    // track's render and the whole mix now start at the same frame: the ancestors' latency is
    // the child's chain's own delay, without their effects or mixer settings.
    let project = tree(vec![device("overdrive", &[])], vec![], vec![]);
    let (child, top) = (render(&project, 1), render(&project, 4));
    let whole = render_offline(&project, SR, 512).unwrap();
    let (child_onset, top_onset, whole_onset) = (onset_of(&child), onset_of(&top), onset_of(&whole));
    assert!(child_onset > 0, "the ancestor's latency delays the child's file: {child_onset}");
    assert_eq!(child_onset, top_onset, "the child keeps the ancestor's timing");
    assert_eq!(child_onset, whole_onset);
    // Without the ancestor's device there is nothing to align, with the same frame as ever.
    let plain = tree(vec![], vec![], vec![]);
    assert_eq!(onset_of(&render(&plain, 1)), onset_of(&render(&plain, 4)));
    // The ancestor's own sound is in the whole mix and the group's own render, and not in the
    // child's file.
    let group = render(&project, 3);
    assert!(max_diff(&group, &child) > 0.01, "{}", max_diff(&group, &child));
}

#[test]
fn the_master_fader_and_mute_are_not_in_a_node_render() {
    let loud = tree(vec![], vec![], vec![]);
    let mut quiet = tree(vec![], vec![], vec![]);
    quiet.channels[4].volume = 0.5;
    // The whole mix is halved by the master's fader...
    let loud_mix = render_offline(&loud, SR, 512).unwrap();
    let quiet_mix = render_offline(&quiet, SR, 512).unwrap();
    let halved: Vec<f32> = loud_mix.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&quiet_mix, &halved) < 1e-6, "{}", max_diff(&quiet_mix, &halved));
    // ...and a node render is not: it is the same file whatever the master's fader is.
    for source in [1u32, 3, 5, 4] {
        let from_loud = render(&loud, source);
        let from_quiet = render(&quiet, source);
        assert!(max_diff(&from_loud, &from_quiet) == 0.0, "source {source}");
    }
    // The master muted silences the mix and not the node's file.
    let mut muted = tree(vec![], vec![], vec![]);
    muted.channels[4].mute = true;
    assert!(peak(&render_offline(&muted, SR, 512).unwrap()) < 1e-9);
    assert_eq!(render(&loud, 3), render(&muted, 3));
}

#[test]
fn a_muted_child_is_silent_in_the_group_render_and_the_group_mute_is_not_in_a_track_render() {
    let mut one_out = tree(vec![], vec![], vec![]);
    one_out.channels[1].mute = true;
    // The group's render hears track 1 alone.
    let group = render(&one_out, 3);
    let loud = tree(vec![], vec![], vec![]);
    assert!(max_diff(&group, &render(&loud, 1)) < 1e-6, "{}", max_diff(&group, &render(&loud, 1)));
    // The group muted is silence in its own render and nothing in track 1's.
    let mut group_out = tree(vec![], vec![], vec![]);
    group_out.channels[2].mute = true;
    assert!(peak(&render(&group_out, 3)) < 1e-9);
    assert!(peak(&render(&group_out, 1)) > 0.05);
}

#[test]
fn a_send_to_a_return_is_not_in_a_node_render() {
    let sent = with_send(0.5, vec![device("utility", &[("gain", 6.0206)])]);
    let unsent = tree(vec![], vec![], vec![]);
    // The whole mix carries the return; the group's render does not.
    let whole_sent = render_offline(&sent, SR, 512).unwrap();
    let whole_unsent = render_offline(&unsent, SR, 512).unwrap();
    assert!(max_diff(&whole_sent, &whole_unsent) > 0.01, "{}", max_diff(&whole_sent, &whole_unsent));
    let group_sent = render(&sent, 3);
    let group_unsent = render(&unsent, 3);
    assert!(max_diff(&group_sent, &group_unsent) == 0.0, "{}", max_diff(&group_sent, &group_unsent));
    // The return as the source: nothing inside it feeds it, so its file is silent.
    assert!(peak(&render(&sent, 7)) < 1e-9);
}

#[test]
fn an_unknown_source_is_refused() {
    let project = tree(vec![], vec![], vec![]);
    let error = crate::render_offline_from(
        &project,
        RenderInputs::from_audio(&AudioPool::new()).with_assets(&DeviceAssets::new()).with_plugins(Plugins::new()),
        SR,
        512,
        Some(TrackId(99)),
    )
    .unwrap_err();
    assert!(matches!(error, crate::RenderError::UnknownSource(TrackId(99))), "{error}");
}

#[test]
fn a_node_render_is_the_same_in_any_block_size() {
    let project =
        tree(vec![device("utility", &[("gain", 6.0206)])], vec![device("utility", &[("gain", -6.0206)])], vec![]);
    let small = crate::render_offline_from(
        &project,
        RenderInputs::from_audio(&AudioPool::new()).with_assets(&DeviceAssets::new()).with_plugins(Plugins::new()),
        SR,
        64,
        Some(TrackId(3)),
    )
    .unwrap();
    let large = crate::render_offline_from(
        &project,
        RenderInputs::from_audio(&AudioPool::new()).with_assets(&DeviceAssets::new()).with_plugins(Plugins::new()),
        SR,
        2048,
        Some(TrackId(3)),
    )
    .unwrap();
    assert_eq!(small.len(), large.len());
    assert!(max_diff(&small, &large) < 1e-6, "{}", max_diff(&small, &large));
}

#[test]
fn a_node_of_the_plain_project_renders_what_grouped_tests_know() {
    // In a project with no premaster and no master channel, the group render is still the sum
    // of the tracks inside it, and the track outside is not in it.
    let project = grouped(vec![], true);
    let group = render(&project, 3);
    let single = render(&project, 1);
    // The outside track is silent in the group's file and the whole mix is louder.
    let whole = render_offline(&project, SR, 512).unwrap();
    assert!(peak(&whole) > peak(&group) * 1.2);
    assert!((peak(&group) - 2.0 * peak(&single)).abs() < 1e-4);
}
