// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sends and returns: a track's sends reach its returns, which run their devices over the sum.

use suhina_model::*;

use crate::chain_tests::{SR, blocks, device, left, max_diff, peak};
use crate::{Renderer, render_offline};

fn channel(id: u32, role: ChannelRole, devices: Vec<Device>) -> Channel {
    Channel { role, devices, ..Channel::new(ChannelId(id), format!("C{id}")) }
}

fn track(id: u32, notes: bool) -> Track {
    Track {
        content: ContentKinds { notes, audio: false },
        channel: Some(ChannelId(id)),
        ..Track::new(TrackId(id), format!("T{id}"))
    }
}

/// Track 1 plays an A4 for a beat and sends to return track 2 at `level`, pre- or post-fader;
/// the return's channel holds `return_devices`. Track 3 is a second return, fed by nothing.
fn sending(level: f64, pre_fader: bool, return_devices: Vec<Device>) -> Project {
    let mut source = channel(1, ChannelRole::Regular, vec![]);
    source.sends.push(Send { target: ChannelId(2), level, pre_fader });
    let clip = Clip::new(ContentId(1), 0.0, 1.0);
    Project {
        tracks: vec![track(1, true), track(2, false), track(3, false)],
        channels: vec![
            source,
            channel(2, ChannelRole::Effect, return_devices),
            channel(3, ChannelRole::Effect, vec![]),
        ],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 1.0,
            lanes: vec![ContentLane {
                target: None,
                data: LaneData::Notes(vec![crate::chain_tests::note(0.0, 1.0, 69)]),
            }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![clip],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

/// The track alone, without its send.
fn dry() -> Vec<f32> {
    render_offline(&sending(0.0, false, vec![]), SR, 512).unwrap_or_default()
}

fn first_sound(out: &[f32]) -> usize {
    left(out).iter().position(|s| s.abs() > 1e-6).unwrap_or(usize::MAX)
}

/// `out` against `expected`, over `expected`'s length; what `out` has beyond it must be silent.
fn assert_sounds_like(out: &[f32], expected: &[f32]) {
    let n = expected.len().min(out.len());
    assert!(max_diff(&out[..n], &expected[..n]) < 1e-5, "{}", max_diff(&out[..n], &expected[..n]));
    assert!(peak(&out[n..]) < 1e-6, "{}", peak(&out[n..]));
}

#[test]
fn a_send_at_unity_reaches_its_return_and_doubles_the_track() {
    let plain = dry();
    assert!(peak(&plain) > 0.1);
    let out = render_offline(&sending(1.0, false, vec![]), SR, 512).unwrap();
    let expected: Vec<f32> = plain.iter().map(|s| s * 2.0).collect();
    assert_sounds_like(&out, &expected);
}

#[test]
fn the_return_runs_its_devices_over_what_it_is_sent() {
    let plain = dry();
    // The track's own fader at zero; a pre-fader send still reaches the return, whose device
    // halves it.
    let mut project = sending(1.0, true, vec![device("utility", &[("gain", -6.0206)])]);
    project.channels[0].volume = 0.0;
    let out = render_offline(&project, SR, 512).unwrap();
    let expected: Vec<f32> = plain.iter().map(|s| s * 0.5).collect();
    assert_sounds_like(&out, &expected);
}

#[test]
fn a_post_fader_send_follows_the_track_volume_and_a_pre_fader_one_does_not() {
    let plain = dry();
    let mut post = sending(0.5, false, vec![]);
    post.channels[0].volume = 0.5;
    let out = render_offline(&post, SR, 512).unwrap();
    // The track at half, and the send at half of that.
    let expected: Vec<f32> = plain.iter().map(|s| s * 0.75).collect();
    assert_sounds_like(&out, &expected);

    let mut pre = sending(0.5, true, vec![]);
    pre.channels[0].volume = 0.5;
    let out = render_offline(&pre, SR, 512).unwrap();
    let expected: Vec<f32> = plain.iter().map(|s| s * 1.0).collect();
    assert_sounds_like(&out, &expected);

    post.channels[0].volume = 0.0;
    let out = render_offline(&post, SR, 512).unwrap();
    assert!(peak(&out) < 1e-6);
}

#[test]
fn a_post_fader_send_follows_the_pan_and_a_pre_fader_one_stays_centred() {
    let right = |out: &[f32]| out.as_chunks::<2>().0.iter().fold(0.0f32, |m, [_, r]| m.max(r.abs()));
    let mut post = sending(1.0, false, vec![]);
    post.channels[0].pan = -1.0;
    assert!(right(&render_offline(&post, SR, 512).unwrap()) < 1e-6);
    let mut pre = sending(1.0, true, vec![]);
    pre.channels[0].pan = -1.0;
    assert!(right(&render_offline(&pre, SR, 512).unwrap()) > 0.1);
}

#[test]
fn the_return_fader_mute_and_output_apply_to_what_it_plays() {
    let plain = dry();
    let mut project = sending(1.0, false, vec![]);
    project.channels[1].volume = 0.5;
    let out = render_offline(&project, SR, 512).unwrap();
    let expected: Vec<f32> = plain.iter().map(|s| s * 1.5).collect();
    assert_sounds_like(&out, &expected);

    project.channels[1].mute = true;
    assert_sounds_like(&render_offline(&project, SR, 512).unwrap(), &plain);

    // A muted track sends nothing.
    let mut muted = sending(1.0, true, vec![]);
    muted.channels[0].mute = true;
    assert!(peak(&render_offline(&muted, SR, 512).unwrap()) < 1e-6);
}

#[test]
fn a_return_plays_into_a_group_bus_like_any_track() {
    let plain = dry();
    // Return 2 plays into group track 4, whose device halves what reaches it.
    let mut project = sending(1.0, false, vec![]);
    project.tracks.push(track(4, false));
    project.channels.push(channel(4, ChannelRole::Submix, vec![device("utility", &[("gain", -6.0206)])]));
    project.channels[1].output = Some(ChannelId(4));
    let out = render_offline(&project, SR, 512).unwrap();
    let expected: Vec<f32> = plain.iter().map(|s| s * 1.5).collect();
    let n = expected.len();
    assert!(max_diff(&out[..n], &expected) < 1e-5, "{}", max_diff(&out[..n], &expected));
}

#[test]
fn latency_on_a_return_is_compensated_on_the_dry_path() {
    let reference = first_sound(&dry());
    // A limiter's lookahead on the return: the dry track waits for it, so both arrive together.
    let project = sending(1.0, false, vec![device("limiter", &[])]);
    let renderer = Renderer::new(&project, SR).unwrap();
    let latency = renderer.latency() as usize;
    assert!(latency > 0);
    let out = render_offline(&project, SR, 512).unwrap();
    assert_eq!(first_sound(&out), reference + latency);
    // The return alone (the track's fader down, its send before the fader) plus the dry track
    // delayed by the lookahead.
    let mut wet = project.clone();
    wet.channels[0].volume = 0.0;
    wet.channels[0].sends[0].pre_fader = true;
    let wet = render_offline(&wet, SR, 512).unwrap();
    let plain = dry();
    let expected: Vec<f32> = (0..wet.len())
        .map(|i| wet[i] + i.checked_sub(latency * 2).and_then(|j| plain.get(j)).copied().unwrap_or(0.0))
        .collect();
    let n = expected.len().min(out.len());
    assert!(max_diff(&out[..n], &expected[..n]) < 1e-5, "{}", max_diff(&out[..n], &expected[..n]));
}

#[test]
fn latency_on_the_sending_track_delays_its_send_too() {
    let reference = first_sound(&dry());
    // The limiter is on the track: the return gets what the limiter made, and nothing is
    // delayed twice.
    let mut project = sending(1.0, false, vec![]);
    project.channels[0].devices.push(device("limiter", &[]));
    let renderer = Renderer::new(&project, SR).unwrap();
    let latency = renderer.latency() as usize;
    assert!(latency > 0);
    let out = render_offline(&project, SR, 512).unwrap();
    assert_eq!(first_sound(&out), reference + latency);
    let mut alone = project.clone();
    alone.channels[0].sends.clear();
    let doubled: Vec<f32> = render_offline(&alone, SR, 512).unwrap().iter().map(|s| s * 2.0).collect();
    assert_sounds_like(&out, &doubled);
}

#[test]
fn a_return_can_send_to_another_return_but_not_in_a_loop() {
    let plain = dry();
    // 1 → 2 → 3, and 3 → 2 back, and 2 to itself: the loop sends are left out.
    let mut project = sending(1.0, false, vec![]);
    project.channels[1].sends.push(Send { target: ChannelId(3), level: 1.0, pre_fader: false });
    project.channels[1].sends.push(Send { target: ChannelId(2), level: 1.0, pre_fader: false });
    project.channels[2].sends.push(Send { target: ChannelId(2), level: 1.0, pre_fader: false });
    let out = render_offline(&project, SR, 512).unwrap();
    // The track, return 2 and return 3: three times the track.
    let expected: Vec<f32> = plain.iter().map(|s| s * 3.0).collect();
    assert_sounds_like(&out, &expected);
}

#[test]
fn sends_to_what_is_not_a_return_and_hostile_levels_are_ignored() {
    let plain = dry();
    for level in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        let out = render_offline(&sending(level, false, vec![]), SR, 512).unwrap();
        assert_sounds_like(&out, &plain);
    }
    let mut project = sending(1.0, false, vec![]);
    project.channels[1].role = ChannelRole::Regular;
    assert_sounds_like(&render_offline(&project, SR, 512).unwrap(), &plain);
    // A send to a channel that no track has.
    let mut project = sending(1.0, false, vec![]);
    project.tracks.retain(|t| t.id != TrackId(2));
    assert_sounds_like(&render_offline(&project, SR, 512).unwrap(), &plain);
}

#[test]
fn solo_on_the_track_keeps_its_sends_and_solo_elsewhere_silences_them() {
    let plain = dry();
    let mut project = sending(1.0, false, vec![]);
    project.channels[0].solo = true;
    let expected: Vec<f32> = plain.iter().map(|s| s * 2.0).collect();
    assert_sounds_like(&render_offline(&project, SR, 512).unwrap(), &expected);
    // Soloing the unrelated return 3: track 1 is not heard, so neither is its send.
    project.channels[0].solo = false;
    project.channels[2].solo = true;
    assert!(peak(&render_offline(&project, SR, 512).unwrap()) < 1e-6);
}

#[test]
fn block_size_does_not_change_a_render_with_sends() {
    let mut project = sending(0.7, false, vec![device("limiter", &[])]);
    project.channels[0].devices.push(device("utility", &[("gain", -3.0)]));
    project.channels[1].sends.push(Send { target: ChannelId(3), level: 0.5, pre_fader: true });
    let reference = render_offline(&project, SR, 512).unwrap();
    for frames in [1, 64, 333, 2048] {
        let out = render_offline(&project, SR, frames).unwrap();
        assert!(max_diff(&out, &reference) < 1e-6, "{frames}: {}", max_diff(&out, &reference));
    }
    let mut renderer = Renderer::new(&project, SR).unwrap();
    let live = blocks(&mut renderer, reference.len() / 2 / 256 + 1, 256);
    let n = reference.len().min(live.len());
    assert!(max_diff(&live[..n], &reference[..n]) < 1e-6);
}

/// As in a project the app makes: a main channel, and the return playing into it (#705).
fn sending_through_main(level: f64, pre_fader: bool) -> Project {
    let mut project = sending(level, pre_fader, vec![]);
    project.tracks.push(Track { channel: Some(ChannelId(9)), ..Track::new(TrackId(9), "Main") });
    project.channels.push(channel(9, ChannelRole::Master, vec![]));
    for c in project.channels.iter_mut().filter(|c| c.role != ChannelRole::Master) {
        c.output = None;
    }
    project.channels[1].output = Some(ChannelId(9));
    project
}

#[test]
fn a_send_to_an_empty_return_playing_into_main_makes_the_track_louder_on_main() {
    let plain = render_offline(&sending_through_main(0.0, false), SR, 512).unwrap();
    assert!(peak(&plain) > 0.1);
    for pre_fader in [false, true] {
        let out = render_offline(&sending_through_main(1.0, pre_fader), SR, 512).unwrap();
        let expected: Vec<f32> = plain.iter().map(|s| s * 2.0).collect();
        assert_sounds_like(&out, &expected);
        let half = render_offline(&sending_through_main(0.5, pre_fader), SR, 512).unwrap();
        let expected: Vec<f32> = plain.iter().map(|s| s * 1.5).collect();
        assert_sounds_like(&half, &expected);
    }
    // Main's own volume scales the return's share as it does the track's.
    let mut quiet = sending_through_main(1.0, false);
    quiet.channels.iter_mut().find(|c| c.role == ChannelRole::Master).unwrap().volume = 0.5;
    let out = render_offline(&quiet, SR, 512).unwrap();
    let expected = plain.clone();
    assert_sounds_like(&out, &expected);
}

/// Plays `project` for 10 blocks, sends `device`'s value through the controller, plays 30 more:
/// the last block, against `expected`'s same frames.
fn turned(project: &Project, device: usize, param: usize, value: f64, expected: &Project) -> (Vec<f32>, Vec<f32>) {
    let mut renderer = Renderer::new(project, SR).unwrap();
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 10, 480);
    controller.set_device_param(TrackId(1), device, param, value).unwrap();
    out.extend(blocks(&mut renderer, 30, 480));
    let mut reference = Renderer::new(expected, SR).unwrap();
    let whole = blocks(&mut reference, 40, 480);
    let last = 39 * 480 * 2;
    (out[last..].to_vec(), whole[last..].to_vec())
}

#[test]
fn a_send_turned_while_playing_reaches_its_level_without_a_new_graph() {
    // From unity to a quarter: once the smoothing has settled, as a graph built at a quarter.
    let (live, built) = turned(&sending(1.0, false, vec![]), crate::MIXER_SEND, 2, 0.25, &sending(0.25, false, vec![]));
    assert!(peak(&built) > 0.1);
    assert!(max_diff(&live, &built) < 1e-3, "{}", max_diff(&live, &built));
}

#[test]
fn a_send_or_volume_turned_past_the_maximum_plays_at_the_maximum_and_stays_finite() {
    // A level past +6 dB, up to one whose ratio to the built level overflows an f32 (#872): the
    // send or the fader stops at +6 dB, as a graph built there plays.
    let max = suhina_model::automation::MAX_VOLUME;
    let at_max_send = sending(max, false, vec![]);
    let mut at_max_volume = sending(1.0, false, vec![]);
    at_max_volume.channels[0].volume = max;
    for level in [f64::MAX, 1e300, max] {
        let (live, built) = turned(&sending(1.0, false, vec![]), crate::MIXER_SEND, 2, level, &at_max_send);
        assert!(live.iter().all(|s| s.is_finite()), "send at {level}");
        assert!(max_diff(&live, &built) < 1e-3, "send at {level}: {}", max_diff(&live, &built));
        let (live, built) = turned(&sending(1.0, false, vec![]), crate::MIXER_VOLUME, 0, level, &at_max_volume);
        assert!(live.iter().all(|s| s.is_finite()), "volume at {level}");
        assert!(max_diff(&live, &built) < 1e-3, "volume at {level}: {}", max_diff(&live, &built));
    }
}

#[test]
fn a_live_ratio_that_overflows_an_f32_is_refused() {
    assert_eq!(crate::chain::live_ratio(1.0, 1e-300), None);
    assert_eq!(crate::chain::live_ratio(0.5, 1.0), Some(0.5));
    assert_eq!(crate::chain::live_ratio(f64::MAX, 1.0), Some(suhina_model::automation::MAX_VOLUME as f32));
}

#[test]
fn a_pan_turned_while_playing_moves_the_track_and_its_post_fader_send() {
    let mut panned = sending(1.0, false, vec![]);
    panned.channels[0].pan = -0.5;
    let (live, built) = turned(&sending(1.0, false, vec![]), crate::MIXER_PAN, 0, -0.5, &panned);
    assert!(peak(&built) > 0.1);
    assert!(max_diff(&live, &built) < 1e-3, "{}", max_diff(&live, &built));
    // To either end and back, without a new graph.
    let mut hard = sending(1.0, false, vec![]);
    hard.channels[0].pan = -1.0;
    let (live, built) = turned(&sending(1.0, false, vec![]), crate::MIXER_PAN, 0, -1.0, &hard);
    assert!(max_diff(&live, &built) < 1e-3, "{}", max_diff(&live, &built));
    let (live, built) = turned(&hard, crate::MIXER_PAN, 0, 0.3, &{
        let mut back = sending(1.0, false, vec![]);
        back.channels[0].pan = 0.3;
        back
    });
    assert!(max_diff(&live, &built) < 1e-3, "{}", max_diff(&live, &built));
}

#[test]
fn a_pan_turned_while_playing_moves_the_track_on_the_cue_too() {
    // Cued, the track is heard on the cue bus after its own volume and pan (cue spec): a pan
    // turned live moves it there as a graph built at the new pan has it.
    let cued = |pan: f64| {
        let mut p = sending(1.0, false, vec![]);
        p.channels[0].cue = true;
        p.channels[0].pan = pan;
        p
    };
    let cue_of = |renderer: &mut Renderer, blocks: usize| {
        let mut cue = Vec::new();
        for _ in 0..blocks {
            let (mut main, mut block) = (vec![0.0f32; 480 * 2], vec![0.0f32; 480 * 2]);
            renderer.process_with_cue(&mut main, &mut block);
            cue.extend(block);
        }
        cue
    };
    let mut renderer = Renderer::new(&cued(0.0), SR).unwrap();
    let mut controller = renderer.controller();
    let mut live = cue_of(&mut renderer, 10);
    controller.set_device_param(TrackId(1), crate::MIXER_PAN, 0, -0.5).unwrap();
    live.extend(cue_of(&mut renderer, 30));
    let mut reference = Renderer::new(&cued(-0.5), SR).unwrap();
    let built = cue_of(&mut reference, 40);
    let last = 39 * 480 * 2;
    assert!(peak(&built[last..]) > 0.05, "the cue hears the track");
    assert!(max_diff(&live[last..], &built[last..]) < 1e-3, "{}", max_diff(&live[last..], &built[last..]));
}
