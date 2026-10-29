// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group buses: a group with devices processes the sum of the tracks routed into it.

use suhina_model::*;

use crate::chain_tests::{SR, blocks, device, left, max_diff, note, peak};
use crate::{RenderInputs, RenderOptions, Renderer, render_offline};

/// Tracks 1 and 2 each play an A4 for a beat into group track 3 (channel 3), whose channel
/// holds `group_devices`; track 4 plays the same note straight to the output, unless `outside`
/// is false.
pub(crate) fn grouped(group_devices: Vec<Device>, outside: bool) -> Project {
    let channel = |id: u32, output: Option<u32>, devices: Vec<Device>| Channel {
        output: output.map(ChannelId),
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
    let players: Vec<u32> = if outside { vec![1, 2, 4] } else { vec![1, 2] };
    let mut tracks = vec![track(1, Some(3)), track(2, Some(3)), track(3, None)];
    let mut channels = vec![channel(1, Some(3), vec![]), channel(2, Some(3), vec![]), channel(3, None, group_devices)];
    if outside {
        tracks.push(track(4, None));
        channels.push(channel(4, None, vec![]));
    }
    Project {
        tracks,
        channels,
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note(0.0, 1.0, 69)]) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: players
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

fn first_sound(out: &[f32]) -> usize {
    left(out).iter().position(|s| s.abs() > 1e-6).unwrap_or(usize::MAX)
}

fn db(value: f64) -> f32 {
    10f64.powf(value / 20.0) as f32
}

#[test]
fn a_limiter_on_a_group_limits_the_sum_of_its_tracks() {
    let plain = render_offline(&grouped(vec![], false), SR, 512).unwrap();
    // Two tracks of one note in phase: twice one track's level.
    let single = peak(&plain) / 2.0;
    let ceiling = (single * 1.2).log10() * 20.0;
    let limited = render_offline(
        &grouped(vec![device("limiter", &[("threshold", f64::from(ceiling)), ("ceiling", f64::from(ceiling))])], false),
        SR,
        512,
    )
    .unwrap();
    // Each track alone is under the ceiling; their sum is not, and the bus brings it down to it.
    // Limiting each track on its own would have left the sum at twice the single level.
    let reached = peak(&limited);
    assert!(reached <= db(f64::from(ceiling)) * 1.001, "{reached} over {}", db(f64::from(ceiling)));
    assert!(reached > single, "{reached}");
    assert!(peak(&plain) > reached * 1.5);
}

#[test]
fn a_group_without_devices_sounds_as_before_and_a_neutral_bus_changes_nothing() {
    let plain = render_offline(&grouped(vec![], true), SR, 512).unwrap();
    let neutral = render_offline(&grouped(vec![device("utility", &[])], true), SR, 512).unwrap();
    // The bus's chain adds a tail to the render, nothing else.
    assert!(neutral.len() > plain.len());
    assert!(max_diff(&neutral[..plain.len()], &plain) < 1e-5, "{}", max_diff(&neutral[..plain.len()], &plain));
    assert!(peak(&neutral[plain.len()..]) < 1e-6);
}

#[test]
fn the_group_volume_applies_after_its_devices_and_the_tracks_volume_before() {
    let plain = render_offline(&grouped(vec![], false), SR, 512).unwrap();
    let mut project = grouped(vec![device("utility", &[("gain", -6.0206)])], false);
    project.channels[2].volume = 0.5;
    project.channels[0].volume = 0.0;
    let out = render_offline(&project, SR, 512).unwrap();
    // One of two tracks left, halved by the device and halved again by the group's fader.
    let expected: Vec<f32> = plain.iter().map(|s| s * 0.125).collect();
    assert!(max_diff(&out[..plain.len()], &expected) < 1e-4, "{}", max_diff(&out[..plain.len()], &expected));
}

#[test]
fn latency_on_a_bus_and_in_it_is_compensated_on_every_path() {
    let reference = first_sound(&render_offline(&grouped(vec![], true), SR, 512).unwrap());
    // A limiter (lookahead) on the group, another on track 1 inside it: track 1 is delayed by
    // both, track 2 by the group's, track 4 by neither; all must still start together.
    let mut project = grouped(vec![device("limiter", &[])], true);
    project.channels[0].devices.push(device("limiter", &[]));
    let out = render_offline(&project, SR, 512).unwrap();
    let renderer = Renderer::new(&project, SR).unwrap();
    let lookahead = renderer.latency() / 2;
    assert!(lookahead > 0);
    assert_eq!(first_sound(&out), reference + renderer.latency() as usize);

    // Each track heard alone (the others at zero volume, so the routing stays) starts at the
    // same frame as all together.
    for alone in [0usize, 1, 3] {
        let mut one = project.clone();
        for (i, channel) in one.channels.iter_mut().enumerate() {
            if i != alone && i != 2 {
                channel.volume = 0.0;
            }
        }
        let out = render_offline(&one, SR, 512).unwrap();
        assert_eq!(first_sound(&out), reference + renderer.latency() as usize, "track index {alone}");
    }
}

#[test]
fn nested_groups_and_muting() {
    // Group 3 plays into group 5, which has a device; group 3 has one too.
    let mut project = grouped(vec![device("utility", &[("gain", -6.0206)])], false);
    project.channels[2].output = Some(ChannelId(5));
    project.tracks.push(Track {
        id: TrackId(5),
        name: "G2".into(),
        parent: None,
        channel: Some(ChannelId(5)),
        ..project.tracks[2].clone()
    });
    project.tracks[2].parent = Some(TrackId(5));
    project.channels.push(Channel {
        id: ChannelId(5),
        output: None,
        devices: vec![device("utility", &[("gain", -6.0206)])],
        ..project.channels[2].clone()
    });
    let plain = render_offline(&grouped(vec![], false), SR, 512).unwrap();
    let out = render_offline(&project, SR, 512).unwrap();
    let expected: Vec<f32> = plain.iter().map(|s| s * 0.25).collect();
    assert!(max_diff(&out[..plain.len()], &expected) < 1e-4, "{}", max_diff(&out[..plain.len()], &expected));

    project.channels[3].mute = true;
    assert_eq!(render_offline(&project, SR, 512), Ok(Vec::new()));
}

#[test]
fn a_bus_does_not_depend_on_the_block_size() {
    let mut project = grouped(vec![device("limiter", &[("threshold", -12.0)])], true);
    project.channels[1].devices.push(device("utility", &[("gain", 3.0)]));
    let reference = render_offline(&project, SR, 1024).unwrap();
    for size in [1usize, 37, 256, 4096] {
        let out = render_offline(&project, SR, size).unwrap();
        assert!(max_diff(&out, &reference) < 1e-6, "block {size}: {}", max_diff(&out, &reference));
    }
    // And played live, block by block.
    let mut renderer = Renderer::new(&project, SR).unwrap();
    let live = blocks(&mut renderer, reference.len() / 2 / 300 + 1, 300);
    assert!(max_diff(&live[..reference.len()], &reference) < 1e-6);
}

/// A watched device on a bus fills its ring like one on a track: the bus's chain never runs
/// through `advance` (the stages plug-ins share), only through `complete`, and the capture
/// follows it there too (docs/spec/eq.md › The panel › The analyser).
#[test]
fn a_watched_device_on_a_bus_fills_its_ring() {
    let project = grouped(vec![device("utility", &[])], true);
    let mut renderer =
        Renderer::build(&project, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING)
            .unwrap();
    let monitor = renderer.controller().meter_monitor_arc();
    let spectrum = monitor.spectrum();
    spectrum.watch(0, TrackId(3), 0);
    let out = blocks(&mut renderer, 4, 480);
    assert!(peak(&out) > 0.05, "the tone reaches the output through the bus");
    let (samples, valid) = spectrum.samples(TrackId(3), 0).expect("the bus's watched device has output");
    assert!(valid >= 4 * 480, "{valid} frames of a 4-block play");
    let ring = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(ring > 0.01, "the bus device's output is in the ring, peak {ring}");
}

/// A device that follows the song position (auto-pan synced to the beat) gets the block's own
/// transport wherever it is: on a track that plays into a group, on one that plays straight to
/// the output (both run in the stages plug-ins share) and on the group's bus (run after them).
/// Given the previous block's, its LFO would jump back a block at every block boundary, and the
/// render would depend on the block size.
#[test]
fn a_synced_device_gets_its_blocks_own_transport_on_a_track_and_on_a_bus() {
    let synced = || device("auto-pan", &[("sync", 1.0), ("amount", 1.0)]);
    for place in ["track in the group", "track to the output", "group"] {
        let mut project = grouped(vec![], true);
        match place {
            "track in the group" => project.channels[0].devices.push(synced()),
            "track to the output" => project.channels[3].devices.push(synced()),
            _ => project.channels[2].devices.push(synced()),
        }
        let reference = render_offline(&project, SR, 1024).unwrap();
        for size in [1usize, 256, 333, 4096] {
            let out = render_offline(&project, SR, size).unwrap();
            assert!(max_diff(&out, &reference) < 1e-6, "{place}, block {size}: {}", max_diff(&out, &reference));
        }
    }
}

#[test]
fn a_routing_cycle_through_buses_is_silence_not_a_hang() {
    let mut project = grouped(vec![device("utility", &[])], false);
    project.channels[2].output = Some(ChannelId(1));
    assert_eq!(render_offline(&project, SR, 512), Ok(Vec::new()));
}

/// Three groups, one inside the other: group 10 inside group 11 inside group 12. Track 1 plays
/// into 10, track 2 into 11, track 3 into 12, track 4 straight to the output; each plays the
/// same A4 for a beat. `devices` are the chains of groups 10, 11 and 12.
fn nested(devices: [Vec<Device>; 3]) -> Project {
    let mut project = grouped(vec![], true);
    let [inner, middle, outer] = devices;
    let template = project.channels[2].clone();
    let group_track = project.tracks[2].clone();
    project.tracks.truncate(2);
    project.tracks.push(project.tracks[1].clone());
    project.tracks.push(Track {
        id: TrackId(4),
        channel: Some(ChannelId(4)),
        parent: None,
        ..project.tracks[0].clone()
    });
    project.channels.truncate(2);
    project.channels.push(Channel { id: ChannelId(3), ..project.channels[1].clone() });
    project.channels.push(Channel { id: ChannelId(4), output: None, ..project.channels[1].clone() });
    for (i, (group, chain)) in [(10u32, inner), (11, middle), (12, outer)].into_iter().enumerate() {
        let parent = (group < 12).then_some(group + 1);
        project.tracks.push(Track {
            id: TrackId(group),
            name: format!("G{group}"),
            channel: Some(ChannelId(group)),
            parent: parent.map(TrackId),
            ..group_track.clone()
        });
        project.channels.push(Channel {
            id: ChannelId(group),
            role: ChannelRole::Submix,
            output: parent.map(ChannelId),
            devices: chain,
            ..template.clone()
        });
        // Track i + 1 plays into this group.
        project.tracks[i].id = TrackId(i as u32 + 1);
        project.tracks[i].channel = Some(ChannelId(i as u32 + 1));
        project.tracks[i].parent = Some(TrackId(group));
        project.channels[i].output = Some(ChannelId(group));
    }
    project.arrangement.lanes = (1..=4)
        .map(|t| ArrangementLane {
            id: Default::default(),
            name: format!("l{t}"),
            track: Some(TrackId(t)),
            clips: project.arrangement.lanes[0].clips.clone(),
        })
        .collect();
    project
}

/// One track playing the note straight to the output: what every path above is a multiple of.
fn one_track() -> Vec<f32> {
    let mut project = grouped(vec![], true);
    project.tracks.retain(|t| t.id == TrackId(4));
    project.channels.retain(|c| c.id == ChannelId(4));
    project.arrangement.lanes.retain(|l| l.track == Some(TrackId(4)));
    render_offline(&project, SR, 512).unwrap()
}

fn half() -> Vec<Device> {
    vec![device("utility", &[("gain", -6.0206)])]
}

fn assert_scaled(out: &[f32], reference: &[f32], scale: f32, what: &str) {
    assert!(out.len() >= reference.len(), "{what}: {} frames", out.len() / 2);
    let expected: Vec<f32> = reference.iter().map(|s| s * scale).collect();
    let diff = max_diff(&out[..reference.len()], &expected);
    assert!(diff < 1e-4, "{what}: off by {diff}");
    assert!(peak(&out[reference.len()..]) < 1e-4, "{what}: sound after the note");
}

#[test]
fn three_nested_buses_sum_each_level_through_every_chain_above_it() {
    let project = nested([half(), half(), half()]);
    assert_eq!(project.validate(), Ok(()));
    assert_eq!(project.group_depth(TrackId(1)), Some(3));
    let reference = one_track();
    // Track 1 is halved by three buses, track 2 by two, track 3 by one, track 4 by none.
    let out = render_offline(&project, SR, 512).unwrap();
    assert_scaled(&out, &reference, 0.125 + 0.25 + 0.5 + 1.0, "all four");
    for (alone, scale) in [(0usize, 0.125f32), (1, 0.25), (2, 0.5), (3, 1.0)] {
        let mut one = project.clone();
        for (i, channel) in one.channels.iter_mut().enumerate().take(4) {
            if i != alone {
                channel.volume = 0.0;
            }
        }
        assert_scaled(&render_offline(&one, SR, 512).unwrap(), &reference, scale, &format!("track {}", alone + 1));
    }
}

#[test]
fn a_level_without_devices_between_buses_is_a_gain() {
    // The middle group has no devices: a fader at a quarter between two buses.
    let mut project = nested([half(), vec![], half()]);
    project.channels[5].volume = 0.25;
    let out = render_offline(&project, SR, 512).unwrap();
    // Track 1: 0.5 * 0.25 * 0.5; track 2: 0.25 * 0.5; track 3: 0.5; track 4: 1.
    assert_scaled(&out, &one_track(), 0.0625 + 0.125 + 0.5 + 1.0, "mixed levels");
    // The fader of the outer group scales everything inside it, after its devices.
    project.channels[6].volume = 0.0;
    assert_scaled(&render_offline(&project, SR, 512).unwrap(), &one_track(), 1.0, "outer group down");
}

#[test]
fn solo_and_mute_follow_the_nesting() {
    let project = nested([half(), half(), half()]);
    let reference = one_track();
    let render = |p: &Project| render_offline(p, SR, 512).unwrap();

    // A soloed track deep inside is heard through every group above it, and nothing else is.
    let mut solo = project.clone();
    solo.channels[0].solo = true;
    assert_scaled(&render(&solo), &reference, 0.125, "track 1 soloed");

    // A soloed group plays everything inside it at any depth.
    let mut solo = project.clone();
    solo.channels[5].solo = true;
    assert_scaled(&render(&solo), &reference, 0.125 + 0.25, "middle group soloed");
    solo.channels[6].solo = true;
    assert_scaled(&render(&solo), &reference, 0.125 + 0.25 + 0.5, "middle and outer soloed");

    // A muted group silences everything inside it, soloed or not.
    let mut muted = project.clone();
    muted.channels[5].mute = true;
    assert_scaled(&render(&muted), &reference, 0.5 + 1.0, "middle group muted");
    muted.channels[0].solo = true;
    assert_eq!(render(&muted), Vec::<f32>::new());
}

#[test]
fn latency_through_nested_buses_is_compensated_on_every_path() {
    let reference = first_sound(&render_offline(&nested([vec![], vec![], vec![]]), SR, 512).unwrap());
    // A limiter (lookahead) on the inner and the outer group: track 1 passes both, track 2 and
    // 3 only the outer one, track 4 neither; all still start together.
    let project = nested([vec![device("limiter", &[])], vec![], vec![device("limiter", &[])]]);
    let latency = Renderer::new(&project, SR).unwrap().latency() as usize;
    assert!(latency > 0);
    for alone in 0..4 {
        let mut one = project.clone();
        for (i, channel) in one.channels.iter_mut().enumerate().take(4) {
            if i != alone {
                channel.volume = 0.0;
            }
        }
        let out = render_offline(&one, SR, 512).unwrap();
        assert_eq!(first_sound(&out), reference + latency, "track {}", alone + 1);
    }
    // And the render does not depend on the block size.
    let whole = render_offline(&project, SR, 1024).unwrap();
    for size in [1usize, 300, 4096] {
        assert!(max_diff(&render_offline(&project, SR, size).unwrap(), &whole) < 1e-6, "block {size}");
    }
}
