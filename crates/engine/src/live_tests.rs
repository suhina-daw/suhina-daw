// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live input (ADR 0017): notes from a MIDI controller played by the armed tracks' instruments,
//! with the transport stopped or running, across graph swaps.

use suhina_model::*;

use crate::chain_tests::{SR, blocks, device, project};
use crate::{AudioPool, LIVE_QUEUE, LaunchCommand, LiveEvent, Renderer};

fn synth() -> Project {
    project(Vec::new(), 1.0, vec![device("fm-synth", &[])])
}

/// Two tracks with an instrument each, on channels 1 and 2.
fn two_synths() -> Project {
    let mut p = synth();
    let mut track = p.tracks[0].clone();
    track.id = TrackId(2);
    track.channel = Some(ChannelId(2));
    p.tracks.push(track);
    let mut channel = p.channels[0].clone();
    channel.id = ChannelId(2);
    p.channels.push(channel);
    p
}

fn stopped(project: &Project) -> Renderer {
    Renderer::stopped(project, &AudioPool::new(), SR).unwrap()
}

fn peak(out: &[f32]) -> f32 {
    out.iter().fold(0.0, |m, s| m.max(s.abs()))
}

#[test]
fn an_armed_tracks_instrument_plays_live_notes_with_the_transport_stopped() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    assert!(controller.take_live_input().is_none(), "taken once");
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 2, 256)), 0.0, "silent until a key goes down");

    assert!(live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 }));
    let first = blocks(&mut renderer, 1, 256);
    // Heard from the first frame of the next block.
    assert!(peak(first.get(..32).unwrap()) > 0.0, "the note starts at once");
    let held = peak(&blocks(&mut renderer, 20, 256));
    assert!(held > 0.01, "{held}");

    assert!(live.send(LiveEvent::NoteOff { key: 60 }));
    let after = blocks(&mut renderer, 400, 256);
    let late = peak(&after[380 * 512..]);
    assert!(late < held * 0.05, "released: {late} of {held}");
}

#[test]
fn a_track_that_is_not_armed_hears_nothing() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    assert_eq!(peak(&blocks(&mut renderer, 10, 256)), 0.0);
}

#[test]
fn every_armed_track_plays_and_a_disarmed_one_lets_go() {
    let mut both = stopped(&two_synths());
    let mut controller = both.controller();
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(2), armed: true, input: Default::default() }).unwrap();
    live.send(LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    let two = peak(&blocks(&mut both, 20, 256));

    let mut one = stopped(&two_synths());
    let mut controller_one = one.controller();
    let mut live_one = controller_one.take_live_input().unwrap();
    controller_one.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    live_one.send(LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    let single = peak(&blocks(&mut one, 20, 256));
    assert!(two > single * 1.5, "two instruments are louder than one: {two} {single}");

    // Disarming both releases the key still held down.
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: false, input: Default::default() }).unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(2), armed: false, input: Default::default() }).unwrap();
    let after = blocks(&mut both, 400, 256);
    assert!(peak(&after[380 * 512..]) < two * 0.05);
}

#[test]
fn a_track_armed_while_a_key_is_down_hears_the_next_key() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    blocks(&mut renderer, 1, 256);
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 4, 256)), 0.0, "the key already down is not started");
    live.send(LiveEvent::NoteOn { key: 62, velocity: 1.0 });
    assert!(peak(&blocks(&mut renderer, 4, 256)) > 0.0);
}

#[test]
fn a_key_held_across_a_graph_swap_is_released_by_its_note_off() {
    let project = synth();
    let mut renderer = stopped(&project);
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    let held = peak(&blocks(&mut renderer, 20, 256));

    // An edit that keeps the chain: the instrument and its held note carry over.
    let mut edited = project.clone();
    edited.channels[0].volume = 0.9;
    controller.update(&edited).unwrap();
    blocks(&mut renderer, 4, 256);
    // The new graph's track is still armed.
    live.send(LiveEvent::NoteOn { key: 67, velocity: 1.0 });
    assert!(peak(&blocks(&mut renderer, 4, 256)) > 0.0);
    live.send(LiveEvent::NoteOff { key: 60 });
    live.send(LiveEvent::NoteOff { key: 67 });
    let after = blocks(&mut renderer, 400, 256);
    assert!(peak(&after[380 * 512..]) < held * 0.05, "nothing hangs after the swap");
}

#[test]
fn all_notes_off_releases_every_key() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    for key in [48, 52, 55, 60] {
        live.send(LiveEvent::NoteOn { key, velocity: 0.8 });
    }
    let held = peak(&blocks(&mut renderer, 20, 256));
    live.send(LiveEvent::AllNotesOff);
    let after = blocks(&mut renderer, 400, 256);
    assert!(peak(&after[380 * 512..]) < held * 0.05);
}

#[test]
fn a_full_queue_drops_notes_and_a_gone_renderer_is_noticed() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    let mut sent = 0;
    for i in 0..LIVE_QUEUE + 100 {
        let key = (i % 128) as u8;
        let event = if i % 2 == 0 { LiveEvent::NoteOn { key, velocity: 1.0 } } else { LiveEvent::NoteOff { key } };
        if live.send(event) {
            sent += 1;
        }
    }
    assert_eq!(sent, LIVE_QUEUE);
    assert!(blocks(&mut renderer, 4, 256).iter().all(|s| s.is_finite()));
    assert!(live.is_connected());
    drop(renderer);
    assert!(!live.is_connected());
    assert!(!live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 }));
}

#[test]
fn hostile_live_values_are_made_safe() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(99), armed: true, input: Default::default() }).unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(99), armed: false, input: Default::default() }).unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(5), armed: false, input: Default::default() }).unwrap();
    live.send(LiveEvent::NoteOn { key: 200, velocity: 1.0 });
    live.send(LiveEvent::NoteOff { key: 255 });
    live.send(LiveEvent::NoteOn { key: 60, velocity: f32::NAN });
    live.send(LiveEvent::NoteOn { key: 61, velocity: f32::INFINITY });
    live.send(LiveEvent::NoteOn { key: 62, velocity: -3.0 });
    let out = blocks(&mut renderer, 8, 256);
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(peak(&out) > 0.0);
}

#[test]
fn more_tracks_than_can_be_armed_are_left_unarmed() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    for id in 100..100 + crate::MAX_ARMED as u32 {
        controller
            .send(LaunchCommand::SetArmed { track: TrackId(id), armed: true, input: Default::default() })
            .unwrap();
    }
    blocks(&mut renderer, 1, 256);
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    assert_eq!(peak(&blocks(&mut renderer, 4, 256)), 0.0, "no room for track 1");
    controller.send(LaunchCommand::SetArmed { track: TrackId(100), armed: false, input: Default::default() }).unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    live.send(LiveEvent::NoteOn { key: 62, velocity: 1.0 });
    assert!(peak(&blocks(&mut renderer, 4, 256)) > 0.0);
}

#[test]
fn the_sustain_pedal_holds_released_keys_until_it_comes_up() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    live.send(LiveEvent::Sustain { on: true });
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    let held = peak(&blocks(&mut renderer, 20, 256));
    live.send(LiveEvent::NoteOff { key: 60 });
    let sustained = blocks(&mut renderer, 400, 256);
    assert!(peak(&sustained[380 * 512..]) > held * 0.05, "the pedal keeps it sounding");

    live.send(LiveEvent::Sustain { on: false });
    let after = blocks(&mut renderer, 400, 256);
    assert!(peak(&after[380 * 512..]) < held * 0.05, "released with the pedal");

    // A key still held down when the pedal comes up keeps sounding.
    live.send(LiveEvent::Sustain { on: true });
    live.send(LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    live.send(LiveEvent::Sustain { on: false });
    let still = blocks(&mut renderer, 400, 256);
    assert!(peak(&still[380 * 512..]) > held * 0.05);
    live.send(LiveEvent::NoteOff { key: 64 });
    let gone = blocks(&mut renderer, 400, 256);
    assert!(peak(&gone[380 * 512..]) < held * 0.05);
}

#[test]
fn the_renderer_says_whether_a_track_is_armed() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    assert!(!renderer.live_armed());
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    blocks(&mut renderer, 1, 64);
    assert!(renderer.live_armed());
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: false, input: Default::default() }).unwrap();
    blocks(&mut renderer, 1, 64);
    assert!(!renderer.live_armed());
}

#[test]
fn a_live_note_is_heard_from_the_start_of_the_next_block_at_any_block_size() {
    // The engine's part of a key's latency is at most one block: the note waits for the next
    // block and sounds from its first frames, whatever the block size.
    for frames in [64, 128, 256, 1024] {
        let mut renderer = stopped(&synth());
        let mut controller = renderer.controller();
        let mut live = controller.take_live_input().unwrap();
        controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
        assert_eq!(peak(&blocks(&mut renderer, 1, frames)), 0.0);
        live.send(LiveEvent::NoteOn { key: 69, velocity: 1.0 });
        let out = blocks(&mut renderer, 1, frames);
        let first = out.iter().position(|s| s.abs() > 1e-6).map(|i| i / 2);
        assert!(first.is_some_and(|f| f < 8), "{frames}: first sound at {first:?}");
    }
}

fn armed_with(controller: &mut crate::Controller, track: u32, input: crate::LiveFilter) {
    controller.send(LaunchCommand::SetArmed { track: TrackId(track), armed: true, input }).unwrap();
}

fn keys() -> crate::LiveSource {
    crate::LiveSource { port: crate::port_id("Keys"), channel: 0 }
}

fn pads() -> crate::LiveSource {
    crate::LiveSource { port: crate::port_id("Pads"), channel: 9 }
}

/// Renders a project of two synth tracks with track 1 hearing `one` and track 2 `two`, plays
/// `notes`, and returns how loud each track is alone (its channel muted otherwise).
fn loudness_per_track(
    one: crate::LiveFilter,
    two: crate::LiveFilter,
    notes: &[(crate::LiveSource, LiveEvent)],
) -> (f32, f32) {
    let solo = |muted: usize| {
        let mut project = two_synths();
        project.channels[muted].mute = true;
        let mut renderer = stopped(&project);
        let mut controller = renderer.controller();
        let mut live = controller.take_live_input().unwrap();
        armed_with(&mut controller, 1, one);
        armed_with(&mut controller, 2, two);
        blocks(&mut renderer, 1, 256);
        for (source, event) in notes {
            live.send_from(*source, *event);
        }
        peak(&blocks(&mut renderer, 20, 256))
    };
    (solo(1), solo(0))
}

#[test]
fn each_armed_track_hears_only_the_port_and_channel_it_is_given() {
    use crate::{LiveFilter, LivePorts, port_id};
    let on_keys = LiveFilter { ports: LivePorts::One(port_id("Keys")), channel: None };
    let on_pads_ten = LiveFilter { ports: LivePorts::One(port_id("Pads")), channel: Some(9) };
    let note = LiveEvent::NoteOn { key: 60, velocity: 1.0 };

    let (one, two) = loudness_per_track(on_keys, on_pads_ten, &[(keys(), note)]);
    assert!(one > 0.0 && two == 0.0, "the keyboard plays track 1 only: {one} {two}");
    let (one, two) = loudness_per_track(on_keys, on_pads_ten, &[(pads(), note)]);
    assert!(one == 0.0 && two > 0.0, "the pads play track 2 only: {one} {two}");
    // The pads on another channel than track 2's: nobody.
    let (one, two) = loudness_per_track(on_keys, on_pads_ten, &[(crate::LiveSource { channel: 0, ..pads() }, note)]);
    assert_eq!((one, two), (0.0, 0.0));
    // Every port, and no port at all.
    let all = LiveFilter::default();
    let nothing = LiveFilter { ports: LivePorts::Nothing, channel: None };
    let (one, two) = loudness_per_track(all, nothing, &[(pads(), note), (keys(), note)]);
    assert!(one > 0.0 && two == 0.0, "{one} {two}");
}

#[test]
fn one_controllers_pedal_does_not_hold_another_controllers_notes() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    armed_with(&mut controller, 1, crate::LiveFilter::default());
    live.send_from(pads(), LiveEvent::Sustain { on: true });
    live.send_from(keys(), LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    let held = peak(&blocks(&mut renderer, 20, 256));
    live.send_from(keys(), LiveEvent::NoteOff { key: 60 });
    let after = blocks(&mut renderer, 400, 256);
    assert!(peak(&after[380 * 512..]) < held * 0.05, "the pads' pedal leaves the keyboard's note alone");
}

#[test]
fn a_track_that_stops_hearing_a_port_lets_go_of_its_keys() {
    use crate::{LiveFilter, LivePorts, port_id};
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    armed_with(&mut controller, 1, LiveFilter::default());
    live.send_from(keys(), LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    let held = peak(&blocks(&mut renderer, 20, 256));
    // Now it hears the pads only: the keyboard's note goes, the pads' play.
    armed_with(&mut controller, 1, LiveFilter { ports: LivePorts::One(port_id("Pads")), channel: None });
    let after = blocks(&mut renderer, 400, 256);
    assert!(peak(&after[380 * 512..]) < held * 0.05);
    let tail = peak(&after[399 * 512..]);
    live.send_from(keys(), LiveEvent::NoteOn { key: 62, velocity: 1.0 });
    assert!(peak(&blocks(&mut renderer, 4, 256)) <= tail + 1e-6, "no new keyboard notes");
    live.send_from(pads(), LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    assert!(peak(&blocks(&mut renderer, 4, 256)) > 0.0);
}

#[test]
fn many_sources_reuse_the_ones_that_are_silent() {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    armed_with(&mut controller, 1, crate::LiveFilter::default());
    // More sources than are followed, each pressing and releasing a key.
    for port in 0..(crate::MAX_SOURCES as u64 * 3) {
        let source = crate::LiveSource { port: port + 1, channel: (port % 16) as u8 };
        live.send_from(source, LiveEvent::NoteOn { key: 60, velocity: 1.0 });
        live.send_from(source, LiveEvent::NoteOff { key: 60 });
    }
    let held = peak(&blocks(&mut renderer, 4, 256));
    let after = blocks(&mut renderer, 400, 256);
    assert!(after.iter().all(|s| s.is_finite()));
    assert!(peak(&after[380 * 512..]) < held.max(1e-3) * 0.05, "nothing hangs");
    assert_ne!(crate::port_id(""), 0);
    assert_eq!(crate::port_id("Keys"), crate::port_id("Keys"));
    assert_ne!(crate::port_id("Keys"), crate::port_id("Pads"));
}

/// The first frame of `out` (interleaved stereo) that is not silent.
fn onset(out: &[f32]) -> Option<usize> {
    out.iter().position(|s| s.abs() > 1e-6).map(|i| i / 2)
}

/// Renders `count` blocks of `frames`, telling the renderer before each that it renders at
/// `now(block)` on the live clock, with a delay of `delay` frames; `send` is called before the
/// block of that index is rendered.
fn clocked(
    renderer: &mut Renderer,
    count: usize,
    frames: usize,
    delay: u32,
    now: impl Fn(usize) -> u64,
    mut send: impl FnMut(usize),
) -> Vec<f32> {
    let mut out = Vec::new();
    let mut block = vec![0.0f32; frames * 2];
    for index in 0..count {
        send(index);
        renderer.set_live_clock(now(index), delay);
        renderer.process(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

fn armed_synth() -> (Renderer, crate::LiveSender) {
    let mut renderer = stopped(&synth());
    let mut controller = renderer.controller();
    let live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    (renderer, live)
}

/// Microseconds of `frames` frames.
fn micros(frames: usize) -> u64 {
    (frames as u64 * 1_000_000).div_ceil(u64::from(SR))
}

#[test]
fn a_stamped_note_plays_a_fixed_delay_after_its_time_at_any_block_size() {
    // #269: a note played between two blocks used to land on the next block's first frame, up
    // to a block late. Stamped, it lands on the frame where its time and the delay fall,
    // whatever the block size and wherever in the block it was played.
    for frames in [64usize, 128, 256, 1024] {
        let delay = frames as u32 * 2;
        for into_block in [0.0, 0.1, 0.37, 0.5, 0.93] {
            let (mut renderer, mut live) = armed_synth();
            let start = 1_000_000;
            let block_us = |n: usize| start + micros(frames * n);
            // Played some way into the time of block 4, before block 5 is rendered.
            let played = block_us(4) + (micros(frames) as f64 * into_block) as u64;
            let out = clocked(&mut renderer, 12, frames, delay, block_us, |index| {
                if index == 5 {
                    live.send_at(Default::default(), LiveEvent::NoteOn { key: 69, velocity: 1.0 }, played);
                }
            });
            // Where the note was played, in frames from the first block's time.
            let at = (played - start) as f64 * f64::from(SR) / 1e6 + f64::from(delay);
            let [frame] = renderer.live_played()[..] else { panic!("{frames} {into_block}: one note") };
            assert!((frame as f64 - at).abs() <= 1.0, "{frames} {into_block}: {frame} for {at}");
            // And it is heard from there (the synth starts notes on its own 64-frame steps).
            let heard = onset(&out).unwrap_or_else(|| panic!("{frames} {into_block}: no sound")) as u64;
            assert!((frame..frame + 72).contains(&heard), "{frames} {into_block}: heard at {heard}, played at {frame}");
        }
    }
}

#[test]
fn a_stamped_note_waits_for_a_later_block_and_keeps_its_order() {
    let (mut renderer, mut live) = armed_synth();
    let frames = 64;
    let block_us = |n: usize| micros(frames * n);
    // A long delay: the note's frame is three blocks later than the block that takes it.
    let delay = (frames * 4) as u32;
    let out = clocked(&mut renderer, 10, frames, delay, block_us, |index| {
        if index == 1 {
            live.send_at(Default::default(), LiveEvent::NoteOn { key: 69, velocity: 1.0 }, block_us(1));
        }
    });
    assert_eq!(renderer.live_played(), [(frames * 5) as u64]);
    assert!(onset(&out).is_some_and(|heard| heard >= frames * 5));
    // A note-off sent with no time after a waiting note-on does not overtake it: the key ends
    // up released, not stuck.
    let (mut renderer, mut live) = armed_synth();
    let out = clocked(&mut renderer, 400, frames, delay, block_us, |index| {
        if index == 1 {
            live.send_at(Default::default(), LiveEvent::NoteOn { key: 69, velocity: 1.0 }, block_us(1));
            live.send(LiveEvent::NoteOff { key: 69 });
        }
    });
    assert_eq!(renderer.live_played(), [(frames * 5) as u64; 2], "the note-off waits for its note-on");
    assert!(peak(&out[390 * frames * 2..]) < 1e-3, "released");
}

#[test]
fn notes_without_a_time_or_too_late_play_at_the_next_blocks_first_frame() {
    for time in [0, 1] {
        let (mut renderer, mut live) = armed_synth();
        let frames = 256;
        let block_us = |n: usize| 10_000_000 + micros(frames * n);
        let out = clocked(&mut renderer, 4, frames, (frames * 2) as u32, block_us, |index| {
            if index == 2 {
                // Without a time, or from long ago.
                live.send_at(Default::default(), LiveEvent::NoteOn { key: 69, velocity: 1.0 }, time);
            }
        });
        assert_eq!(renderer.live_played(), [(frames * 2) as u64], "{time}");
        assert!(onset(&out).is_some_and(|heard| heard >= frames * 2));
    }
}

#[test]
fn a_jittery_render_thread_does_not_move_the_notes() {
    // The render thread wakes late by up to half a block, and sometimes renders two blocks at
    // once; the notes keep their spacing to within a few frames, where they used to jitter by a
    // whole block.
    let frames = 256usize;
    let delay = (frames * 3) as u32;
    let jitter = |n: usize| [0, 40, 1500, 2600, 300, 0, 2000, 900][n % 8];
    let block_us = move |n: usize| 5_000_000 + micros(frames * n) + jitter(n);
    let mut distances = Vec::new();
    for (index, into) in [(20usize, 0.2), (31, 0.8), (40, 0.5), (55, 0.05)] {
        let (mut renderer, mut live) = armed_synth();
        let played = 5_000_000 + micros(frames * (index - 1)) + (micros(frames) as f64 * into) as u64;
        clocked(&mut renderer, index + 8, frames, delay, block_us, |n| {
            if n == index {
                live.send_at(Default::default(), LiveEvent::NoteOn { key: 69, velocity: 1.0 }, played);
            }
        });
        let [heard] = renderer.live_played()[..] else { panic!("one note") };
        distances.push(heard as f64 - (played - 5_000_000) as f64 * f64::from(SR) / 1e6);
    }
    let (low, high) = distances.iter().fold((f64::MAX, f64::MIN), |(l, h), d| (l.min(*d), h.max(*d)));
    assert!(high - low < frames as f64 / 4.0, "{distances:?}");
}

#[test]
fn a_drivers_stamps_are_placed_on_the_live_clock() {
    let mut clock = crate::StampClock::default();
    // The driver's clock starts at 0; messages arrive 300 to 900 µs after their stamps.
    assert_eq!(clock.time(1_000, 50_900), 50_900, "the first one as it arrived");
    assert_eq!(clock.time(2_000, 51_300), 51_300, "a shorter gap moves the clock");
    assert_eq!(clock.time(3_000, 52_800), 52_300, "a longer gap is a late arrival");
    // Never after it arrived, and never 0.
    assert_eq!(clock.time(4_000, 10), 10);
    let mut fresh = crate::StampClock::default();
    assert_eq!(fresh.time(5, 0), 1);
    // A stamp that goes back (the driver started its clock again) starts over.
    let mut clock = crate::StampClock::default();
    clock.time(1_000_000, 2_000_000);
    assert_eq!(clock.time(10, 2_100_000), 2_100_000);
    // Hostile stamps neither panic nor go past the arrival.
    let mut clock = crate::StampClock::default();
    for (stamp, arrived) in [(u64::MAX, 5), (0, u64::MAX), (u64::MAX, u64::MAX), (7, 7)] {
        let time = clock.time(stamp, arrived);
        assert!(time >= 1 && time <= arrived.max(1), "{stamp} {arrived}: {time}");
    }
    assert!(crate::live_now() >= 1);
}

/// Notes from a MIDI controller pass through the pitch effect before the instrument, sound as
/// the transposed key played straight in, and end when the key is let go.
#[test]
fn live_notes_go_through_the_midi_effects_before_the_instrument_and_end() {
    // A key through the pitch effect sounds as the transposed key played straight in.
    let play = |devices: Vec<Device>, key: u8| {
        let mut renderer = stopped(&project(Vec::new(), 1.0, devices));
        let mut controller = renderer.controller();
        let mut live = controller.take_live_input().unwrap();
        controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
        live.send(LiveEvent::NoteOn { key, velocity: 1.0 });
        let mut out = blocks(&mut renderer, 20, 256);
        live.send(LiveEvent::NoteOff { key });
        out.extend(blocks(&mut renderer, 200, 256));
        out
    };
    let pitch = || device("pitch", &[("pitch", 7.0)]);
    for (pitched, plain) in [
        (play(vec![pitch(), device("fm-synth", &[])], 60), play(vec![device("fm-synth", &[])], 67)),
        // Without an instrument the sine tones play what the effect sends (a pitch effect at 0
        // still brings the tones in for the key played straight).
        (play(vec![pitch()], 60), play(vec![device("pitch", &[])], 67)),
    ] {
        assert!(peak(&plain) > 0.01);
        let diff = pitched.iter().zip(&plain).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(diff < 1e-6, "{diff}");
        assert!(peak(&pitched[pitched.len() - 512..]) < 1e-4, "the note ends");
    }
}
