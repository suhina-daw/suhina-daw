// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio input (ADR 0017): each track hears its channel or pair of the input device, as its
//! monitoring says, through its chain and mix; a band recorded on separate tracks; the input
//! ring's clock drifting from the output's.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use suhina_model::*;

use crate::chain_tests::{SR, blocks, device};
use crate::{AudioPool, LaunchCommand, LiveFilter, Renderer};

const BLOCK: usize = 256;

/// Tracks `ids`, each on a channel of its own with `inputs` as its audio input, and no clips.
fn tracks_with_inputs(tracks: &[(u32, InputChannels, Monitor)]) -> Project {
    Project {
        tracks: tracks
            .iter()
            .map(|&(id, channels, monitor)| Track {
                content: ContentKinds { notes: false, audio: true },
                channel: Some(ChannelId(id)),
                audio_input: AudioInput { channels, monitor },
                ..Track::new(TrackId(id), format!("T{id}"))
            })
            .collect(),
        channels: tracks.iter().map(|&(id, _, _)| Channel::new(ChannelId(id), format!("C{id}"))).collect(),
        ..Project::default()
    }
}

/// A stopped renderer reading an input of `channels` channels, and the ring's writing end.
fn with_input(project: &Project, channels: usize) -> (Renderer, rtrb::Producer<f32>, Arc<AtomicU64>) {
    let mut renderer = Renderer::stopped(project, &AudioPool::new(), SR).unwrap();
    let (producer, consumer) = rtrb::RingBuffer::new(BLOCK * channels * 16);
    let underruns = Arc::new(AtomicU64::new(0));
    renderer.set_audio_input(consumer, channels, Arc::clone(&underruns));
    (renderer, producer, underruns)
}

/// Writes `frames` frames to the ring: `level` on channel `on` (from 0), silence on the rest.
fn feed(ring: &mut rtrb::Producer<f32>, channels: usize, frames: usize, on: usize, level: f32) {
    for _ in 0..frames {
        for channel in 0..channels {
            let _ = ring.push(if channel == on { level } else { 0.0 });
        }
    }
}

fn arm(controller: &mut crate::Controller, track: u32, armed: bool) {
    controller.send(LaunchCommand::SetArmed { track: TrackId(track), armed, input: LiveFilter::default() }).unwrap();
}

/// The last frame of one block, rendered after feeding one block of `level` on channel `on`.
fn one_block(
    renderer: &mut Renderer,
    ring: &mut rtrb::Producer<f32>,
    channels: usize,
    on: usize,
    level: f32,
) -> [f32; 2] {
    feed(ring, channels, BLOCK, on, level);
    let out = blocks(renderer, 1, BLOCK);
    [out[out.len() - 2], out[out.len() - 1]]
}

#[test]
fn auto_monitoring_is_heard_while_the_track_is_armed() {
    let project = tracks_with_inputs(&[(1, InputChannels::Mono(1), Monitor::Auto)]);
    let (mut renderer, mut ring, _) = with_input(&project, 2);
    let mut controller = renderer.controller();
    assert_eq!(one_block(&mut renderer, &mut ring, 2, 0, 0.5), [0.0, 0.0], "not armed");
    arm(&mut controller, 1, true);
    // A mono input in the middle of a track at unity: the input's level on both sides.
    let [l, r] = one_block(&mut renderer, &mut ring, 2, 0, 0.5);
    assert!((l - 0.5).abs() < 1e-4 && (r - 0.5).abs() < 1e-4, "{l} {r}");
    arm(&mut controller, 1, false);
    assert_eq!(one_block(&mut renderer, &mut ring, 2, 0, 0.5), [0.0, 0.0], "disarmed");
}

#[test]
fn in_is_always_heard_and_off_never() {
    let project =
        tracks_with_inputs(&[(1, InputChannels::Mono(1), Monitor::In), (2, InputChannels::Mono(2), Monitor::Off)]);
    let (mut renderer, mut ring, _) = with_input(&project, 2);
    let mut controller = renderer.controller();
    let [l, _] = one_block(&mut renderer, &mut ring, 2, 0, 0.5);
    assert!((l - 0.5).abs() < 1e-4, "In, not armed: {l}");
    arm(&mut controller, 2, true);
    assert_eq!(one_block(&mut renderer, &mut ring, 2, 1, 0.5), [0.0, 0.0], "Off, even armed");
}

#[test]
fn a_stereo_input_takes_its_pair_left_and_right() {
    let project = tracks_with_inputs(&[(1, InputChannels::Stereo(3), Monitor::In)]);
    let (mut renderer, mut ring, _) = with_input(&project, 4);
    let [l, r] = one_block(&mut renderer, &mut ring, 4, 2, 0.5);
    assert!((l - 0.5).abs() < 1e-4 && r.abs() < 1e-6, "channel 3 on the left: {l} {r}");
    let [l, r] = one_block(&mut renderer, &mut ring, 4, 3, 0.5);
    assert!(l.abs() < 1e-6 && (r - 0.5).abs() < 1e-4, "channel 4 on the right: {l} {r}");
    // Channels 1 and 2 are not this track's.
    assert_eq!(one_block(&mut renderer, &mut ring, 4, 0, 0.5), [0.0, 0.0]);
}

/// The acceptance of #267: on one 13-channel interface, singer A on track 1 from input 1,
/// singer B on track 2 from input 2, the drums on tracks 10 to 20 from inputs 3 to 13, all armed
/// at once with Auto monitoring; each input is heard through its own track and no other.
#[test]
fn a_band_is_heard_on_separate_tracks_each_from_its_own_input() {
    const CHANNELS: usize = 13;
    let mut layout = vec![(1, InputChannels::Mono(1), Monitor::Auto), (2, InputChannels::Mono(2), Monitor::Auto)];
    layout.extend((10..=20u32).map(|track| (track, InputChannels::Mono((track - 7) as u16), Monitor::Auto)));
    let tracks: Vec<u32> = layout.iter().map(|(t, _, _)| *t).collect();
    for (input, &track) in tracks.iter().enumerate() {
        // Every track armed; only this input plays. Then the same with this track muted.
        for muted in [false, true] {
            let mut project = tracks_with_inputs(&layout);
            if let Some(channel) = project.channel_mut(ChannelId(track)) {
                channel.mute = muted;
            }
            let (mut renderer, mut ring, _) = with_input(&project, CHANNELS);
            let mut controller = renderer.controller();
            for &t in &tracks {
                arm(&mut controller, t, true);
            }
            blocks(&mut renderer, 1, BLOCK);
            let [l, _] = one_block(&mut renderer, &mut ring, CHANNELS, input, 0.5);
            let expected = if muted { 0.0 } else { 0.5 };
            assert!((l - expected).abs() < 1e-4, "input {} on track {track}, muted {muted}: {l}", input + 1);
        }
    }
}

#[test]
fn the_input_goes_through_the_tracks_chain() {
    let mut project = tracks_with_inputs(&[(1, InputChannels::Mono(1), Monitor::In)]);
    project.channels[0].devices = vec![device("utility", &[("gain", -6.0206)])];
    let (mut renderer, mut ring, _) = with_input(&project, 1);
    let mut level = 0.0;
    for _ in 0..8 {
        level = one_block(&mut renderer, &mut ring, 1, 0, 0.5)[0];
    }
    assert!((level - 0.25).abs() < 0.01, "6 dB down through the utility: {level}");
}

#[test]
fn too_little_input_is_silence_counted_and_too_much_is_dropped() {
    let project = tracks_with_inputs(&[(1, InputChannels::Mono(1), Monitor::In)]);
    let (mut renderer, mut ring, underruns) = with_input(&project, 1);
    feed(&mut ring, 1, BLOCK / 2, 0, 0.5);
    let out = blocks(&mut renderer, 1, BLOCK);
    assert!((out[0] - 0.5).abs() < 1e-4 && out[out.len() - 2] == 0.0, "the half there, then silence");
    assert_eq!(underruns.load(Ordering::Relaxed), 1);

    // The input's clock ran ahead: ten blocks wait. One block later at most two remain.
    feed(&mut ring, 1, BLOCK * 10, 0, 0.5);
    blocks(&mut renderer, 1, BLOCK);
    assert!(ring.buffer().capacity() - ring.slots() <= BLOCK * 2, "{}", ring.buffer().capacity() - ring.slots());
    assert_eq!(underruns.load(Ordering::Relaxed), 1);
}

#[test]
fn hostile_input_is_made_safe() {
    let project = tracks_with_inputs(&[
        (1, InputChannels::Mono(1), Monitor::In),
        (2, InputChannels::Stereo(200), Monitor::In),
        (3, InputChannels::Mono(256), Monitor::In),
    ]);
    let (mut renderer, mut ring, _) = with_input(&project, 2);
    for sample in [f32::NAN, f32::INFINITY, -f32::INFINITY, 0.25] {
        for _ in 0..BLOCK * 2 {
            let _ = ring.push(sample);
        }
        let out = blocks(&mut renderer, 1, BLOCK);
        assert!(out.iter().all(|s| s.is_finite()), "{sample}");
    }
    // A renderer without an input device, and one told of 0 or 1000 channels, stay silent and
    // finite.
    let mut silent = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    assert!(blocks(&mut silent, 2, BLOCK).iter().all(|s| *s == 0.0));
    for channels in [0, 1000] {
        let (mut producer, consumer) = rtrb::RingBuffer::new(1024);
        let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
        renderer.set_audio_input(consumer, channels, Arc::new(AtomicU64::new(0)));
        for _ in 0..1000 {
            let _ = producer.push(0.5);
        }
        assert!(blocks(&mut renderer, 2, BLOCK).iter().all(|s| s.is_finite()), "{channels}");
    }
}

#[test]
fn an_offline_render_hears_no_input() {
    let project = tracks_with_inputs(&[(1, InputChannels::Mono(1), Monitor::In)]);
    let out = crate::render_offline(&project, SR, BLOCK).unwrap();
    assert!(out.iter().all(|s| *s == 0.0));
}
