// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recording (`docs/spec/recording.md`): what the armed tracks play goes out stamped with the
//! beat that was heard; the count-in; an audio loopback lined up to the frame.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use suhina_model::*;

use crate::chain_tests::{BEAT, SR, blocks, device, project};
use crate::{AudioPool, LaunchCommand, LiveEvent, LiveFilter, RecordReceiver, RecordedNote, Renderer};

const BLOCK: usize = 256;

fn arm(controller: &mut crate::Controller, track: u32) {
    controller
        .send(LaunchCommand::SetArmed { track: TrackId(track), armed: true, input: LiveFilter::default() })
        .unwrap();
}

fn notes(receiver: &mut RecordReceiver) -> Vec<RecordedNote> {
    std::iter::from_fn(|| receiver.next_note()).collect()
}

/// A stopped renderer of one track with an instrument, armed, and its live input and recorder.
fn armed() -> (Renderer, crate::Controller, crate::LiveSender, RecordReceiver) {
    let project = project(Vec::new(), 4.0, vec![device("fm-synth", &[])]);
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    let live = controller.take_live_input().unwrap();
    let recorder = controller.take_recorder().unwrap();
    assert!(controller.take_recorder().is_none(), "taken once");
    arm(&mut controller, 1);
    (renderer, controller, live, recorder)
}

fn beats(frames: usize) -> f64 {
    frames as f64 / BEAT as f64
}

#[test]
fn keys_played_while_recording_go_out_at_the_beat_they_were_played() {
    let (mut renderer, mut controller, mut live, mut recorder) = armed();
    controller.send(LaunchCommand::Play).unwrap();
    blocks(&mut renderer, 4, BLOCK);
    assert!(notes(&mut recorder).is_empty(), "nothing while recording is off");
    live.send(LiveEvent::NoteOn { key: 60, velocity: 0.5 });
    blocks(&mut renderer, 1, BLOCK);
    assert!(notes(&mut recorder).is_empty());

    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    blocks(&mut renderer, 3, BLOCK);
    live.send(LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    blocks(&mut renderer, 10, BLOCK);
    live.send(LiveEvent::NoteOff { key: 64 });
    live.send(LiveEvent::NoteOff { key: 60 });
    blocks(&mut renderer, 1, BLOCK);
    let got = notes(&mut recorder);
    // Played at the first frame of block 8, let go at the first frame of block 18.
    let down = beats(8 * BLOCK);
    let up = beats(18 * BLOCK);
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(got[0].key == 64 && got[0].velocity == Some(1.0) && close(got[0].beat, down) && close(got[0].clock, down));
    assert!(got[1].key == 64 && got[1].velocity.is_none() && close(got[1].beat, up));
    // A key that went down before recording is let go in it: its end is recorded.
    assert!(got[2].key == 60 && got[2].velocity.is_none() && close(got[2].beat, up));
    assert!(got.iter().all(|n| n.track == TrackId(1)));

    controller.send(LaunchCommand::SetRecording { on: false }).unwrap();
    blocks(&mut renderer, 1, BLOCK);
    live.send(LiveEvent::NoteOn { key: 67, velocity: 1.0 });
    blocks(&mut renderer, 2, BLOCK);
    assert!(notes(&mut recorder).is_empty(), "off again");
}

#[test]
fn nothing_is_recorded_while_the_transport_is_stopped_or_from_an_unarmed_track() {
    let (mut renderer, mut controller, mut live, mut recorder) = armed();
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    blocks(&mut renderer, 2, BLOCK);
    assert!(notes(&mut recorder).is_empty(), "stopped");
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: false, input: LiveFilter::default() }).unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    blocks(&mut renderer, 1, BLOCK);
    live.send(LiveEvent::NoteOn { key: 62, velocity: 1.0 });
    blocks(&mut renderer, 2, BLOCK);
    assert!(notes(&mut recorder).is_empty(), "not armed");
}

#[test]
fn a_note_is_placed_at_the_beat_heard_when_its_key_went_down() {
    // The key went down `delay` frames before the engine played it (the live notes' fixed
    // delay), and what played then reached the speaker `output` frames later: the beat heard
    // when the key went down is that much earlier than the frame it played at.
    let (mut renderer, mut controller, mut live, mut recorder) = armed();
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    let block_us = |n: usize| 1_000_000 + (n * BLOCK) as u64 * 1_000_000 / u64::from(SR);
    let (delay, output) = (512u32, 700u32);
    let mut block = vec![0.0f32; BLOCK * 2];
    for n in 0..40 {
        if n == 30 {
            // Played at the time of block 20.
            live.send_at(Default::default(), LiveEvent::NoteOn { key: 60, velocity: 1.0 }, block_us(20));
        }
        renderer.set_live_clock(block_us(n), delay);
        renderer.set_record_latency(output, 0);
        renderer.process(&mut block);
    }
    let got = notes(&mut recorder);
    assert_eq!(got.len(), 1);
    // It plays at the frame of block 30 (late: its frame had passed), so heard at that frame
    // less the delay and the output's latency.
    let expected = beats(30 * BLOCK) - beats(delay as usize + output as usize);
    assert!((got[0].beat - expected).abs() < 1e-9, "{} {expected}", got[0].beat);
}

#[test]
fn the_sustain_pedal_ends_a_recorded_note_when_it_comes_up() {
    let (mut renderer, mut controller, mut live, mut recorder) = armed();
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    live.send(LiveEvent::Sustain { on: true });
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    blocks(&mut renderer, 2, BLOCK);
    live.send(LiveEvent::NoteOff { key: 60 });
    blocks(&mut renderer, 2, BLOCK);
    assert_eq!(notes(&mut recorder).len(), 1, "the note is still sounding");
    live.send(LiveEvent::Sustain { on: false });
    blocks(&mut renderer, 1, BLOCK);
    let got = notes(&mut recorder);
    assert_eq!(got.len(), 1);
    assert!(got[0].velocity.is_none() && (got[0].beat - beats(4 * BLOCK)).abs() < 1e-9);
    // An all-notes-off ends what sounds.
    live.send(LiveEvent::NoteOn { key: 62, velocity: 1.0 });
    blocks(&mut renderer, 1, BLOCK);
    live.send(LiveEvent::AllNotesOff);
    blocks(&mut renderer, 1, BLOCK);
    let got = notes(&mut recorder);
    assert_eq!(got.iter().map(|n| (n.key, n.velocity.is_some())).collect::<Vec<_>>(), [(62, true), (62, false)]);
}

#[test]
fn the_count_in_clicks_its_beats_then_starts_the_transport() {
    let project = project(Vec::new(), 4.0, Vec::new());
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    // One bar of 4/4 at 120 bpm: four beats of half a second.
    controller.send(LaunchCommand::CountIn { beats: 4.0, bar_beats: 4.0 }).unwrap();
    let out = blocks(&mut renderer, 1, BLOCK);
    assert!(!renderer.running(), "counting, not playing");
    assert!((controller.monitor().count_in_beats() - (4.0 - beats(BLOCK))).abs() < 1e-6);
    let mut out = [out, blocks(&mut renderer, (4 * BEAT) / BLOCK, BLOCK)].concat();
    // The transport starts on the frame the count ends (here inside a block).
    assert!(renderer.running());
    assert_eq!(controller.monitor().count_in_beats(), 0.0);
    out.truncate(4 * BEAT * 2);
    // A click at the start of every beat, none between; the first is the bar's, higher.
    for beat in 0..4 {
        let click = &out[beat * BEAT * 2..(beat * BEAT + 1200) * 2];
        let quiet = &out[(beat * BEAT + 4000) * 2..(beat * BEAT + BEAT - 10) * 2];
        assert!(click.iter().any(|s| s.abs() > 0.05), "beat {beat} clicks");
        assert!(quiet.iter().all(|s| s.abs() < 1e-3), "beat {beat} then quiet");
    }
    let crossings = |from: usize| {
        let part = &out[from * 2..(from + 1000) * 2];
        part.chunks(2).zip(part.chunks(2).skip(1)).filter(|(a, b)| a[0] <= 0.0 && b[0] > 0.0).count()
    };
    assert!(crossings(0) > crossings(BEAT), "the bar's click is higher");
    // The playhead starts at the marker, 0, when the count ends.
    assert!(controller.monitor().beat() < 1.0);
}

#[test]
fn a_stop_during_the_count_in_ends_it_and_a_count_of_nothing_starts_at_once() {
    let project = project(Vec::new(), 4.0, Vec::new());
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    controller.send(LaunchCommand::SetPosition { beat: 8.0 }).unwrap();
    controller.send(LaunchCommand::CountIn { beats: 8.0, bar_beats: 4.0 }).unwrap();
    blocks(&mut renderer, 10, BLOCK);
    controller.send(LaunchCommand::Stop).unwrap();
    blocks(&mut renderer, 1, BLOCK);
    let after = blocks(&mut renderer, (8 * BEAT) / BLOCK, BLOCK);
    assert!(!renderer.running());
    assert!(after.iter().all(|s| *s == 0.0), "no more clicks");
    assert_eq!(controller.monitor().count_in_beats(), 0.0);
    assert_eq!(controller.monitor().start_beat(), 8.0, "the marker stays");

    for beats in [0.0, -3.0, f64::NAN, f64::INFINITY] {
        controller.send(LaunchCommand::CountIn { beats, bar_beats: f64::NAN }).unwrap();
        blocks(&mut renderer, 1, BLOCK);
        if beats.is_infinite() {
            // Clamped to the longest count-in.
            assert!(!renderer.running());
            controller.send(LaunchCommand::Play).unwrap();
            blocks(&mut renderer, 1, BLOCK);
            assert_eq!(controller.monitor().count_in_beats(), 0.0, "a start ends the count");
        }
        assert!(renderer.running(), "{beats}");
        controller.send(LaunchCommand::Stop).unwrap();
        blocks(&mut renderer, 1, BLOCK);
    }
    // While running, a count-in changes nothing.
    controller.send(LaunchCommand::Play).unwrap();
    controller.send(LaunchCommand::CountIn { beats: 4.0, bar_beats: 4.0 }).unwrap();
    blocks(&mut renderer, 1, BLOCK);
    assert!(renderer.running() && controller.monitor().count_in_beats() == 0.0);
}

/// A track that hears input channel 1, not monitored (so the loopback does not feed itself).
fn input_track() -> Project {
    Project {
        tracks: vec![Track {
            id: TrackId(1),
            name: "Mic".into(),
            color: None,
            content: ContentKinds { notes: false, audio: true },
            channel: Some(ChannelId(1)),
            parent: None,
            midi_input: MidiInput::default(),
            audio_input: AudioInput { channels: InputChannels::Mono(1), monitor: Monitor::Off },
            extensions: Extensions::default(),
        }],
        channels: vec![Channel {
            id: ChannelId(1),
            name: "Mic".into(),
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
        }],
        ..Project::default()
    }
}

#[test]
fn a_loopback_records_the_output_back_on_its_own_frames() {
    // The output (the metronome) goes straight back into the input `delay` frames later: the
    // output's latency, the input device's and the block waiting in the ring. Recorded, each
    // frame of it lines up with the frame it was played at, to the frame.
    for (delay, output, input) in [(BLOCK * 3 + 17, 500u32, 29u32), (BLOCK, 0, 0), (BLOCK * 9, 2048, 0)] {
        let mut renderer = Renderer::stopped(&input_track(), &AudioPool::new(), SR).unwrap();
        let (mut ring, consumer) = rtrb::RingBuffer::new(BLOCK * 64);
        renderer.set_audio_input(consumer, 1, Arc::new(AtomicU64::new(0)));
        let mut controller = renderer.controller();
        let mut recorder = controller.take_recorder().unwrap();
        arm(&mut controller, 1);
        controller.send(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) }).unwrap();
        controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
        controller.send(LaunchCommand::Play).unwrap();
        assert_eq!(output as usize + input as usize + BLOCK, delay, "the latencies add up to the delay");
        let mut played: Vec<f32> = Vec::new();
        let mut block = vec![0.0f32; BLOCK * 2];
        let count = (3 * BEAT) / BLOCK;
        for n in 0..count {
            // What the input hears during this block: the output `delay` frames ago (silence
            // before the start).
            for frame in 0..BLOCK {
                let at = (n * BLOCK + frame) as i64 - delay as i64;
                let sample = usize::try_from(at).ok().and_then(|at| played.get(at)).copied().unwrap_or(0.0);
                ring.push(sample).unwrap();
            }
            renderer.set_record_latency(output, input);
            renderer.process(&mut block);
            played.extend(block.chunks(2).map(|f| f[0]));
        }
        let mut compared = 0;
        let mut clicks = 0;
        while let Some(chunk) = recorder.next_audio() {
            assert_eq!((chunk.track, chunk.channels), (TrackId(1), 1));
            let position = chunk.position as usize;
            if position < delay {
                continue;
            }
            assert!((chunk.beat - beats(position)).abs() < 1e-9, "stamped with its beat");
            for (i, sample) in chunk.samples().iter().enumerate() {
                assert_eq!(*sample, played[position + i], "{delay}: frame {}", position + i);
                compared += 1;
                clicks += usize::from(sample.abs() > 0.05);
            }
        }
        assert!(compared > 2 * BEAT && clicks > 0, "{delay}: {compared} frames, {clicks} loud");
        assert_eq!(recorder.lost(), (0, 0));
    }
}

#[test]
fn input_is_recorded_only_from_armed_tracks_while_recording_and_playing() {
    let mut renderer = Renderer::stopped(&input_track(), &AudioPool::new(), SR).unwrap();
    let (mut ring, consumer) = rtrb::RingBuffer::new(BLOCK * 64);
    renderer.set_audio_input(consumer, 2, Arc::new(AtomicU64::new(0)));
    let mut controller = renderer.controller();
    let mut recorder = controller.take_recorder().unwrap();
    let feed = |ring: &mut rtrb::Producer<f32>| {
        for _ in 0..BLOCK * 2 {
            let _ = ring.push(0.25);
        }
    };
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    feed(&mut ring);
    blocks(&mut renderer, 1, BLOCK);
    assert!(recorder.next_audio().is_none(), "not armed");
    arm(&mut controller, 1);
    feed(&mut ring);
    blocks(&mut renderer, 1, BLOCK);
    let chunks: Vec<_> = std::iter::from_fn(|| recorder.next_audio()).collect();
    assert_eq!(chunks.len(), BLOCK / crate::RECORD_CHUNK);
    assert!(chunks.iter().all(|c| c.samples().iter().all(|s| *s == 0.25)));
    assert_eq!(chunks.iter().map(|c| usize::from(c.frames)).sum::<usize>(), BLOCK);
    controller.send(LaunchCommand::Stop).unwrap();
    // After the stop only the latency's frames still on their way come in (#989), then nothing.
    for _ in 0..8 {
        feed(&mut ring);
        blocks(&mut renderer, 1, BLOCK);
    }
    while recorder.next_audio().is_some() {}
    feed(&mut ring);
    blocks(&mut renderer, 1, BLOCK);
    assert!(recorder.next_audio().is_none(), "stopped");
}

#[test]
fn a_full_ring_counts_what_it_dropped() {
    let mut renderer = Renderer::stopped(&input_track(), &AudioPool::new(), SR).unwrap();
    let (_ring, consumer) = rtrb::RingBuffer::new(16);
    renderer.set_audio_input(consumer, 1, Arc::new(AtomicU64::new(0)));
    let mut controller = renderer.controller();
    let recorder = controller.take_recorder().unwrap();
    arm(&mut controller, 1);
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    let blocks_count = crate::RECORD_CHUNKS * crate::RECORD_CHUNK / BLOCK + 4;
    let out = blocks(&mut renderer, blocks_count, BLOCK);
    assert!(out.iter().all(|s| s.is_finite()));
    assert_eq!(recorder.lost().1, (4 * BLOCK) as u64, "what found the ring full is counted");
}

#[test]
fn a_jump_of_the_playhead_while_recording_goes_out_with_its_frame_and_beats() {
    let (mut renderer, mut controller, mut live, mut recorder) = armed();
    controller.send(LaunchCommand::Play).unwrap();
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    blocks(&mut renderer, 8, BLOCK);
    assert!(recorder.next_jump().is_none(), "playing on is no jump");
    controller.send(LaunchCommand::SetPosition { beat: 0.5 }).unwrap();
    blocks(&mut renderer, 2, BLOCK);
    let jump = recorder.next_jump().expect("the jump back");
    assert!((jump.beat - 0.5).abs() < 1e-9, "{jump:?}");
    assert!([8 * BLOCK, 9 * BLOCK].contains(&(jump.position as usize)), "{jump:?}");
    let at = beats(jump.position as usize);
    assert!((jump.from - at).abs() < 1e-6, "where it would have been: {jump:?}");
    assert!(recorder.next_jump().is_none());
    // A note after the jump carries a frame past it, so it is known to be in the second pass.
    live.send(LiveEvent::NoteOn { key: 60, velocity: 0.5 });
    blocks(&mut renderer, 1, BLOCK);
    let got = notes(&mut recorder);
    assert!(got.first().is_some_and(|n| n.position >= jump.position), "{got:?}");
}

/// After a stop the input goes on being recorded for the latency, so the last frames played reach
/// the take; the frames go on from where the recording stopped, and then nothing more comes.
#[test]
fn the_last_frames_played_are_recorded_after_a_stop() {
    let (output, input) = (BLOCK as u32 * 2, 0u32);
    let mut renderer = Renderer::stopped(&input_track(), &AudioPool::new(), SR).unwrap();
    let (mut ring, consumer) = rtrb::RingBuffer::new(BLOCK * 64);
    renderer.set_audio_input(consumer, 1, Arc::new(AtomicU64::new(0)));
    let mut controller = renderer.controller();
    let mut recorder = controller.take_recorder().unwrap();
    arm(&mut controller, 1);
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    let mut block = vec![0.0f32; BLOCK * 2];
    let mut run = |renderer: &mut Renderer, ring: &mut rtrb::Producer<f32>, count: usize| {
        for _ in 0..count {
            for _ in 0..BLOCK {
                ring.push(0.25).unwrap();
            }
            renderer.set_record_latency(output, input);
            renderer.process(&mut block);
        }
    };
    run(&mut renderer, &mut ring, 8);
    controller.send(LaunchCommand::Stop).unwrap();
    run(&mut renderer, &mut ring, 6);
    let chunks: Vec<_> = std::iter::from_fn(|| recorder.next_audio()).collect();
    let frames: usize = chunks.iter().map(|c| c.samples().len()).sum();
    // Eight blocks played, and the latency after the stop: two more blocks' worth (and the
    // block that waited in the ring).
    let latency = output as usize + input as usize + BLOCK;
    assert_eq!(frames, 8 * BLOCK + latency, "{frames}");
    // In order, with no gap: each chunk starts where the last ended (from where the latency is
    // past, before which they count from the start).
    for pair in chunks.windows(2).filter(|p| p[0].position > 0) {
        assert_eq!(pair[1].position, pair[0].position + pair[0].samples().len() as u64);
    }
}
