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
    let mut renderer = Renderer::stopped(project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
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
    // The half there, then the hold that carries its last delivered frame (a splice to
    // silence would crack through everything downstream).
    assert!((out[0] - 0.5).abs() < 1e-4 && (out[out.len() - 2] - 0.5).abs() < 1e-4, "the half there, then its hold");
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
    let mut silent = Renderer::stopped(&project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
    assert!(blocks(&mut silent, 2, BLOCK).iter().all(|s| *s == 0.0));
    for channels in [0, 1000] {
        let (mut producer, consumer) = rtrb::RingBuffer::new(1024);
        let mut renderer =
            Renderer::stopped(&project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
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
    let out = crate::render_offline(&project, SR, BLOCK, suhina_device_registry::factory()).unwrap();
    assert!(out.iter().all(|s| *s == 0.0));
}

#[test]
fn input_diagnostics_distinguish_empty_and_partial_chunks_of_a_device_block() {
    let project = Project::default();
    let (mut renderer, mut ring, underruns) = with_input(&project, 16);
    let diagnostics = renderer.input_diagnostics();
    let mut output = vec![0.0; 2048 * 2];
    feed(&mut ring, 16, 2048, 0, 0.5);
    renderer.process(&mut output);
    assert!(diagnostics.snapshot().short_reads.is_empty());
    // A second render before the next input callback reads two empty engine chunks.
    renderer.process(&mut output);
    assert_eq!(diagnostics.snapshot().short_reads, [[1, 0, 1024], [2, 0, 1024]]);
    feed(&mut ring, 16, 1280, 0, 0.5);
    renderer.process(&mut output);
    assert_eq!(diagnostics.snapshot().short_reads, [[1, 0, 1024], [2, 0, 1024], [3, 256, 1024]]);
    assert_eq!(underruns.load(Ordering::Relaxed), 3);
}

#[test]
fn input_diagnostics_count_backlog_frames_without_changing_the_drop_policy() {
    let (mut ring, consumer) = rtrb::RingBuffer::new(8192 * 16);
    let mut input = crate::input::Input::new();
    input.attach(consumer, 16, Arc::new(AtomicU64::new(0)));
    feed(&mut ring, 16, 6144, 0, 0.5);
    input.begin_block(1024);
    input.read(1024);
    let snapshot = input.diagnostics.snapshot();
    assert_eq!((snapshot.backlog_drops, snapshot.backlog_frames), (1, 4096));
    input.read(1024);
    assert!(input.diagnostics.snapshot().short_reads.is_empty());
    input.read(1024);
    assert_eq!(input.diagnostics.snapshot().short_reads, [[1, 0, 1024]]);
    assert_eq!(input.diagnostics.snapshot().backlog_drops, 1);
}

#[test]
fn input_diagnostics_keep_only_the_latest_32_short_reads() {
    let (_producer, consumer) = rtrb::RingBuffer::new(2048);
    let mut input = crate::input::Input::new();
    input.attach(consumer, 1, Arc::new(AtomicU64::new(0)));
    for frames in 1..=40 {
        input.read(frames);
    }
    let expected: Vec<_> = (9..=40).map(|sequence| [sequence, 0, sequence]).collect();
    assert_eq!(input.diagnostics.snapshot().short_reads, expected);
    let mut detached = crate::input::Input::new();
    detached.read(1024);
    input.read(0);
    assert!(detached.diagnostics.snapshot().short_reads.is_empty());
    assert_eq!(input.diagnostics.snapshot().short_reads, expected);
}

#[test]
fn a_starved_pair_holds_each_channels_last_state_not_the_previous_blocks_bytes() {
    // The hold's span starts `take` samples in — `take` is a sample count, twice the frame a
    // pair's hold starts at. The old loop took it as a frame index: for a shallow starvation
    // its range was empty and the block's tail kept the scratch's previous block, for a deep
    // one the wrong middle span stayed stale. The held slot's channel is its remainder, and
    // the held state is the last frame the ring delivered, never the scratch's bytes.
    let (mut ring, consumer) = rtrb::RingBuffer::new(4096);
    let mut input = crate::input::Input::new();
    let underruns = Arc::new(AtomicU64::new(0));
    input.attach(consumer, 2, Arc::clone(&underruns));
    const FRAMES: usize = 256;
    // One block of a rising ramp, the sign saying which channel: the scratch now holds the
    // ramp, so a stale hold would read 128 to 254 at the frames below.
    for frame in 0..FRAMES {
        ring.push(f32::from(frame as u16)).unwrap();
        ring.push(-f32::from(frame as u16)).unwrap();
    }
    input.read(FRAMES);
    // Half a block: the first half reads fresh, the rest holds the fresh half's last
    // delivered frame (0.75) on both channels — never the scratch's ramp.
    for _ in 0..FRAMES / 2 {
        ring.push(0.75).unwrap();
        ring.push(-0.75).unwrap();
    }
    input.read(FRAMES);
    for frame in 0..FRAMES {
        let (l, r) = (input.sample(frame, 0), input.sample(frame, 1));
        assert!((l - 0.75).abs() < 1e-6 && (r + 0.75).abs() < 1e-6, "frame {frame}: {l} {r}");
    }
    // A deeper starvation: a quarter block fresh, the rest the hold again — still 0.75 at its
    // first frame, and the hold's level fades (the starvation sustains): each held frame below
    // the one before, the last one at nine tenths of the hold.
    for _ in 0..FRAMES / 4 {
        ring.push(0.75).unwrap();
        ring.push(-0.75).unwrap();
    }
    input.read(FRAMES);
    let mut previous_l = 0.75;
    let mut previous_r = -0.75;
    for frame in 0..FRAMES {
        let (l, r) = (input.sample(frame, 0), input.sample(frame, 1));
        if frame < FRAMES / 4 {
            assert!((l - 0.75).abs() < 1e-6 && (r + 0.75).abs() < 1e-6, "frame {frame}: {l} {r}");
        } else {
            assert!(l <= previous_l + 1e-6 && r >= previous_r - 1e-6, "frame {frame}: {l} {r} fades down");
        }
        previous_l = l;
        previous_r = r;
    }
    assert!((previous_l - 0.75 * crate::input::HOLD_DECAY).abs() < 1e-3, "last held {previous_l}");
    assert_eq!(underruns.load(Ordering::Relaxed), 2);
}

#[test]
fn a_starved_input_that_never_returns_fades_its_hold_to_silence() {
    // An interface unplugged mid-session stops calling back: every later block holds, and
    // nothing resets the hold. It fades toward zero block by block, so the DC offset the first
    // held block carried does not run through monitoring, the chain and the take for as long
    // as the transport runs.
    let (mut ring, consumer) = rtrb::RingBuffer::new(4096);
    let mut input = crate::input::Input::new();
    let underruns = Arc::new(AtomicU64::new(0));
    input.attach(consumer, 1, Arc::clone(&underruns));
    const FRAMES: usize = 256;
    for _ in 0..FRAMES {
        ring.push(0.5).unwrap();
    }
    input.read(FRAMES);
    // The first starved block holds the signal's last state whole.
    input.read(FRAMES);
    for frame in 0..FRAMES {
        assert!((input.sample(frame, 0) - 0.5).abs() < 1e-6, "frame {frame}");
    }
    // The blocks after fade toward zero, each one continuing the last one's end level: the
    // span's last held sample is exactly the decayed level, its first one between the levels.
    let mut level = 1.0;
    for _ in 0..64 {
        let start = level;
        level *= crate::input::HOLD_DECAY;
        input.read(FRAMES);
        let first = input.sample(0, 0);
        let last = input.sample(FRAMES - 1, 0);
        assert!((last - 0.5 * level).abs() < 1e-6, "last {last}");
        assert!(first <= 0.5 * start + 1e-6 && first >= 0.5 * level - 1e-6, "first {first}");
    }
    // After the fade the hold is at silence for good.
    for _ in 0..128 {
        input.read(FRAMES);
    }
    assert!(input.sample(0, 0).abs() < 1e-4, "{}", input.sample(0, 0));
    // Input back: the hold is at full strength again, and the block reads the ring whole.
    for _ in 0..FRAMES {
        ring.push(0.25).unwrap();
    }
    input.read(FRAMES);
    for frame in 0..FRAMES {
        assert!((input.sample(frame, 0) - 0.25).abs() < 1e-6, "frame {frame}");
    }
}

/// A starved block's held frames may still be delivered late rather than dropped (driver
/// jitter): they stay in the ring while every later block reads its own `frames`, so each read
/// runs that count of device time behind — a take written after it runs that many frames late
/// against the beat. A block that finds this much more waiting than before the starvation
/// skips them: the held frames' input time is already covered by the starved block's window.
#[test]
fn a_late_delivery_of_held_frames_is_skipped_and_the_rest_runs_on_time() {
    let (mut ring, consumer) = rtrb::RingBuffer::new(BLOCK * 8);
    let mut input = crate::input::Input::new();
    let underruns = Arc::new(AtomicU64::new(0));
    input.attach(consumer, 1, Arc::clone(&underruns));
    const HALF: usize = BLOCK / 2;
    let mut delivered = 0.0f32;
    let mut feed = |ring: &mut rtrb::Producer<f32>, frames: usize| {
        for _ in 0..frames {
            delivered += 1.0;
            ring.push(delivered * 0.001).unwrap();
        }
    };
    // The device's block: half before the render thread's read, the rest after it. The starved
    // block reads the first half and holds the rest; the device delivers the rest late.
    feed(&mut ring, HALF);
    input.read(BLOCK);
    assert_eq!(input.sample(0, 0), 0.001, "the first delivered frame");
    assert_eq!(input.sample(HALF - 1, 0), HALF as f32 * 0.001, "the last delivered frame");
    for frame in HALF..BLOCK {
        assert_eq!(input.sample(frame, 0), HALF as f32 * 0.001, "frame {frame} holds the last delivered");
    }
    assert_eq!(underruns.load(Ordering::Relaxed), 1);
    // The device's late delivery: the held frames arrive in the ring, and the next block's
    // frames arrive after them — the ring is deeper by the held count than before the
    // starvation.
    feed(&mut ring, HALF);
    feed(&mut ring, BLOCK);
    input.read(BLOCK);
    // The skip dropped the late frames (the device's frames HALF..BLOCK): the block reads the
    // device's frames BLOCK..BLOCK + BLOCK, so the rest of the take runs on the beat. Without
    // the skip it would read the device's frames HALF..BLOCK + HALF, and the rest of the take
    // would run HALF frames late.
    assert_eq!(input.sample(0, 0), (BLOCK + 1) as f32 * 0.001, "the device's frames BLOCK..BLOCK + BLOCK");
    assert_eq!(input.sample(BLOCK - 1, 0), (BLOCK + BLOCK) as f32 * 0.001, "the device's frames BLOCK..BLOCK + BLOCK");
    assert_eq!(underruns.load(Ordering::Relaxed), 1, "the skip is not a starvation");
    // The device's next block, whole: the block reads it whole.
    feed(&mut ring, BLOCK);
    input.read(BLOCK);
    assert_eq!(
        input.sample(0, 0),
        (BLOCK + BLOCK + 1) as f32 * 0.001,
        "the device's frames BLOCK + BLOCK..BLOCK + BLOCK + BLOCK"
    );
    assert_eq!(
        input.sample(BLOCK - 1, 0),
        (BLOCK + BLOCK + BLOCK) as f32 * 0.001,
        "the device's frames BLOCK + BLOCK..BLOCK + BLOCK + BLOCK"
    );
}

/// The late delivery is skipped on a stereo input too: the held count is frames, not their
/// samples — a hold of half a stereo block makes up half a block's frames, `channels` times
/// that many samples, and the skip's check and its drop read frames. With the count in
/// samples the check never passed for more than one channel, and the rest of the take ran
/// the held count late for good (the second review, finding 1).
#[test]
fn a_late_delivery_of_held_frames_is_skipped_on_a_stereo_input_too() {
    let (mut ring, consumer) = rtrb::RingBuffer::new(BLOCK * 8 * 2);
    let mut input = crate::input::Input::new();
    let underruns = Arc::new(AtomicU64::new(0));
    input.attach(consumer, 2, Arc::clone(&underruns));
    const HALF: usize = BLOCK / 2;
    let mut delivered = 0.0f32;
    let mut feed = |ring: &mut rtrb::Producer<f32>, frames: usize| {
        for _ in 0..frames {
            delivered += 1.0;
            for _ in 0..2 {
                ring.push(delivered * 0.001).unwrap();
            }
        }
    };
    // The device's block: half before the render thread's read, the rest after it. The starved
    // block reads the first half and holds the rest; the device delivers the rest late.
    feed(&mut ring, HALF);
    input.read(BLOCK);
    assert_eq!(input.sample(0, 0), 0.001, "the first delivered frame");
    assert_eq!(input.sample(HALF - 1, 0), HALF as f32 * 0.001, "the last delivered frame");
    for frame in HALF..BLOCK {
        assert_eq!(input.sample(frame, 0), HALF as f32 * 0.001, "frame {frame} holds the last delivered");
    }
    assert_eq!(underruns.load(Ordering::Relaxed), 1);
    // The device's late delivery: the held frames arrive in the ring, and the next block's
    // frames arrive after them.
    feed(&mut ring, HALF);
    feed(&mut ring, BLOCK);
    input.read(BLOCK);
    // The skip dropped the late frames (the device's frames HALF..BLOCK): the block reads the
    // device's frames BLOCK..BLOCK + BLOCK on both channels, so the rest of the take runs on
    // the beat. With the held count in samples the check never fired here, and every later
    // block read HALF frames late.
    assert_eq!(input.sample(0, 0), (BLOCK + 1) as f32 * 0.001, "the device's frames BLOCK..BLOCK + BLOCK");
    assert_eq!(input.sample(BLOCK - 1, 1), (BLOCK + BLOCK) as f32 * 0.001, "the second channel reads the same frames");
    assert_eq!(underruns.load(Ordering::Relaxed), 1, "the skip is not a starvation");
    // The device's next block, whole: the block reads it whole.
    feed(&mut ring, BLOCK);
    input.read(BLOCK);
    assert_eq!(
        input.sample(0, 0),
        (BLOCK + BLOCK + 1) as f32 * 0.001,
        "the device's frames BLOCK + BLOCK..BLOCK + BLOCK + BLOCK"
    );
    assert_eq!(
        input.sample(BLOCK - 1, 0),
        (BLOCK + BLOCK + BLOCK) as f32 * 0.001,
        "the device's frames BLOCK + BLOCK..BLOCK + BLOCK + BLOCK"
    );
}

/// A real loss is not mistaken for a late delivery: the held frames never come, the ring
/// carries a block's production over nothing (the starvation emptied it), and the skip — which
/// fires on a whole block's jump over the leftover a whole read leaves — never does. The read
/// keeps main's behaviour, the take runs the loss's own shape, and one starvation cannot set a
/// skip that starves every block after it.
#[test]
fn a_lost_holds_frames_are_not_skipped_as_a_late_delivery() {
    let (mut ring, consumer) = rtrb::RingBuffer::new(BLOCK * 8);
    let mut input = crate::input::Input::new();
    let underruns = Arc::new(AtomicU64::new(0));
    input.attach(consumer, 1, Arc::clone(&underruns));
    const HALF: usize = BLOCK / 2;
    let mut delivered = 0.0f32;
    let mut feed = |ring: &mut rtrb::Producer<f32>, frames: usize| {
        for _ in 0..frames {
            delivered += 1.0;
            ring.push(delivered * 0.001).unwrap();
        }
    };
    // The starved block: half delivered, the rest lost for good.
    feed(&mut ring, HALF);
    input.read(BLOCK);
    assert_eq!(underruns.load(Ordering::Relaxed), 1);
    // The device moved on without the held frames: the next block's production is a block
    // deep, which is what a whole read leaves nothing of — the skip does not fire, and the
    // block reads the whole production.
    feed(&mut ring, BLOCK);
    input.read(BLOCK);
    assert_eq!(input.sample(0, 0), (HALF + 1) as f32 * 0.001, "the device's frames HALF..HALF + BLOCK");
    assert_eq!(input.sample(BLOCK - 1, 0), (HALF + BLOCK) as f32 * 0.001);
    assert_eq!(underruns.load(Ordering::Relaxed), 1, "the block was whole, not starved");
    // The pace goes on whole, and no block starves again: one starvation's owed never makes a
    // skip that keeps starving.
    feed(&mut ring, BLOCK);
    input.read(BLOCK);
    assert_eq!(input.sample(0, 0), (HALF + BLOCK + 1) as f32 * 0.001);
    assert_eq!(underruns.load(Ordering::Relaxed), 1, "still one starvation, no skip starving on");
}

#[test]
fn looping_input_never_discards_the_rest_of_the_device_block() {
    // The hardware trace's first failure followed a 136-frame loop-boundary chunk:
    // the old policy kept only 272 input frames and starved the 1024 + 888 after it.
    for device_frames in [128, 2048, 4096] {
        for boundary in [1, 68, 136] {
            let mut project = tracks_with_inputs(&[(1, InputChannels::Mono(1), Monitor::In)]);
            project.transport.loop_range = Some(LoopRange { start: 0.0, end: 1.0 });
            project.transport.loop_enabled = true;
            let mut renderer =
                Renderer::stopped(&project, &AudioPool::new(), 44_100, suhina_device_registry::factory()).unwrap();
            renderer.set_offline(true);
            let (mut ring, consumer) = rtrb::RingBuffer::new(device_frames * 16 * 8);
            let underruns = Arc::new(AtomicU64::new(0));
            renderer.set_audio_input(consumer, 16, Arc::clone(&underruns));
            renderer.handle(LaunchCommand::SetPosition { beat: 1.0 - f64::from(boundary) / 22_050.0 });
            renderer.handle(LaunchCommand::Play);
            let mut out = vec![0.0; device_frames * 2];
            // One input callback then one render, as in the trace. Several loop wraps,
            // without a hardware clock, sleep or scheduler dependency.
            for _ in 0..(44_100 / device_frames + 2) {
                feed(&mut ring, 16, device_frames, 0, 0.5);
                renderer.process(&mut out);
            }
            assert_eq!(underruns.load(Ordering::Relaxed), 0, "block {device_frames}, boundary {boundary}");
            assert_eq!(renderer.input_diagnostics().snapshot().backlog_drops, 0);
            assert_eq!(ring.buffer().capacity() - ring.slots(), 0);
        }
    }
}

#[test]
fn backlog_trim_preserves_two_whole_batches_across_short_chunks() {
    let (mut ring, consumer) = rtrb::RingBuffer::new(12_288 * 16);
    let mut input = crate::input::Input::new();
    let underruns = Arc::new(AtomicU64::new(0));
    input.attach(consumer, 16, Arc::clone(&underruns));
    for frame in 0..10_240 {
        for _ in 0..16 {
            ring.push(frame as f32).unwrap();
        }
    }
    // Empty and hostile batch lengths must neither overflow nor discard input.
    input.begin_block(0);
    input.begin_block(usize::MAX);
    assert_eq!(input.diagnostics.snapshot().backlog_drops, 0);
    input.begin_block(2048);
    let snapshot = input.diagnostics.snapshot();
    assert_eq!((snapshot.backlog_drops, snapshot.backlog_frames), (1, 6144));
    let mut next_frame = 6144;
    for _ in 0..2 {
        for frames in [136, 1024, 888] {
            input.read(frames);
            assert_eq!(input.waiting(), (10_240 - next_frame) as u64);
            for frame in 0..frames {
                assert_eq!(input.sample(frame, 0), (next_frame + frame) as f32);
                assert_eq!(input.sample(frame, 15), (next_frame + frame) as f32);
            }
            next_frame += frames;
        }
        input.begin_block(2048);
    }
    assert_eq!(underruns.load(Ordering::Relaxed), 0);
    assert_eq!(input.diagnostics.snapshot().backlog_drops, 1);
    assert_eq!(ring.buffer().capacity() - ring.slots(), 0);
}
