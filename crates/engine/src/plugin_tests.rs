// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in slots in the renderer (ADR 0006) with stand-in processors: effects and instruments,
//! a plug-in that misses its deadline or is gone, latency compensation, processors arriving
//! while playing and carried through edits.

use crate::{RenderInputs, RenderOptions, render_offline_from};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use suhina_device_api::NoteEvent;
use suhina_model::*;

use crate::chain_tests::{BEAT, SR, blocks, device, left, max_diff, note, peak, project};
use crate::{
    AudioPool, OFFLINE_TIMEOUT, PluginProcessor, PluginSlotInfo, Plugins, Renderer, Sent, SongPosition, plugin_devices,
    render_offline,
};

/// What a stand-in plug-in does with a block.
#[derive(Clone, Copy)]
enum Behaviour {
    /// Scales the sound.
    Gain(f32),
    /// Plays a constant 0.5 on both sides from each note on until its note off.
    Instrument,
    /// Delays the sound by this many frames, as a plug-in with latency does.
    Delay(usize),
    /// Never answers.
    Gone,
}

struct Stand {
    behaviour: Behaviour,
    sounding: bool,
    history: Vec<f32>,
    /// Every event it got, with its frame in the renderer's output.
    seen: Arc<std::sync::Mutex<Vec<(usize, NoteEvent)>>>,
    /// Frames it has processed.
    frames: usize,
    /// Set false to make it stop answering.
    answering: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
    /// The song's place it was told of with each block.
    songs: Arc<std::sync::Mutex<Vec<SongPosition>>>,
}

impl Stand {
    fn new(behaviour: Behaviour) -> Self {
        Self {
            behaviour,
            sounding: false,
            history: Vec::new(),
            seen: Arc::default(),
            frames: 0,
            answering: Arc::new(AtomicBool::new(true)),
            dropped: Arc::default(),
            songs: Arc::default(),
        }
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl PluginProcessor for Stand {
    fn transport(&mut self, song: &SongPosition) {
        if let Ok(mut songs) = self.songs.lock() {
            songs.push(*song);
        }
    }

    fn process(&mut self, buffer: &mut [f32], events: &[(usize, NoteEvent)], _timeout: Duration) -> bool {
        if !self.answering.load(Ordering::SeqCst) {
            return false;
        }
        if let Ok(mut seen) = self.seen.lock() {
            seen.extend(events.iter().map(|&(at, e)| (self.frames + at, e)));
        }
        let frames = buffer.len() / 2;
        match self.behaviour {
            Behaviour::Gain(gain) => buffer.iter_mut().for_each(|s| *s *= gain),
            Behaviour::Instrument => {
                let mut next = events.iter().peekable();
                for (frame, pair) in buffer.chunks_mut(2).enumerate() {
                    while let Some((_, event)) = next.next_if(|(at, _)| *at <= frame) {
                        match event {
                            NoteEvent::On { .. } => self.sounding = true,
                            NoteEvent::Off { .. } | NoteEvent::AllNotesOff => self.sounding = false,
                            _ => {}
                        }
                    }
                    let value = if self.sounding { 0.5 } else { 0.0 };
                    pair.iter_mut().for_each(|s| *s = value);
                }
            }
            Behaviour::Delay(by) => {
                // `history` holds the last `by` frames.
                if self.history.len() != by * 2 {
                    self.history = vec![0.0; by * 2];
                }
                for pair in buffer.chunks_mut(2) {
                    self.history.extend_from_slice(pair);
                    let oldest: Vec<f32> = self.history.drain(..2).collect();
                    pair.copy_from_slice(&oldest);
                }
            }
            Behaviour::Gone => return false,
        }
        self.frames += frames;
        true
    }
}

fn plug(kind: DeviceKind) -> Device {
    let mut d = device("plug-in", &[]);
    d.kind = kind;
    d
}

fn vst3() -> DeviceKind {
    DeviceKind::Vst3 { id: "565354416263646566".into() }
}

/// The plug-ins of `project` (in order) running in `stands`.
fn plugins(project: &Project, stands: Vec<(Stand, PluginSlotInfo)>) -> Plugins {
    let mut plugins = Plugins::new();
    for (found, (stand, info)) in plugin_devices(project).into_iter().zip(stands) {
        plugins.insert(found.key, info, Box::new(stand));
    }
    plugins
}

const EFFECT: PluginSlotInfo = PluginSlotInfo { instrument: false, latency: 0 };
const INSTRUMENT: PluginSlotInfo = PluginSlotInfo { instrument: true, latency: 0 };

#[test]
fn a_plug_in_effect_processes_the_tracks_sound() {
    let notes = vec![note(0.0, 2.0, 69)];
    let p = project(notes.clone(), 2.0, vec![plug(vst3())]);
    let dry = render_offline(&project(notes, 2.0, vec![device("utility", &[])]), SR, 512).unwrap();
    let wet = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new())
            .with_plugins(plugins(&p, vec![(Stand::new(Behaviour::Gain(0.5)), EFFECT)])),
        SR,
        512,
        None,
    )
    .unwrap();
    assert_eq!(wet.len(), dry.len());
    let half: Vec<f32> = dry.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&wet, &half) < 1e-6, "{}", max_diff(&wet, &half));

    // A plug-in with built-in effects and instruments around it: one after the other.
    let p = project(vec![note(0.0, 2.0, 69)], 2.0, vec![device("utility", &[("gain", -6.0206)]), plug(vst3())]);
    let both = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new())
            .with_plugins(plugins(&p, vec![(Stand::new(Behaviour::Gain(0.5)), EFFECT)])),
        SR,
        333,
        None,
    )
    .unwrap();
    let quarter: Vec<f32> = dry.iter().map(|s| s * 0.25).collect();
    assert!(max_diff(&both, &quarter) < 1e-4, "{}", max_diff(&both, &quarter));
}

#[test]
fn a_plug_in_is_told_the_loop_in_beats_and_in_seconds_through_the_tempo_map() {
    // 120 BPM to beat 4, then 60: the loop from beat 2 to 8 is 1 s to 2 s + 4 s.
    let mut p = project(vec![note(0.0, 1.0, 60)], 8.0, vec![plug(vst3())]);
    p.transport.tempo_changes = vec![TempoPoint { beat: 4.0, bpm: 60.0, ramp: false }];
    p.transport.loop_range = Some(LoopRange { start: 2.0, end: 8.0 });
    p.transport.loop_enabled = true;
    let stand = Stand::new(Behaviour::Gain(1.0));
    let songs = Arc::clone(&stand.songs);
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, EFFECT)])),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    blocks(&mut renderer, 1, 512);
    let song = *songs.lock().unwrap().first().expect("told of the song");
    assert_eq!(song.cycle, Some((2.0, 8.0)));
    let (start, end) = song.cycle_seconds.expect("in seconds too");
    assert!((start - 1.0).abs() < 1e-3 && (end - 6.0).abs() < 1e-3, "{start} to {end}");
}

#[test]
fn an_offline_render_plays_through_the_loop_and_tells_plug_ins_of_no_cycle() {
    // A render does not repeat the loop (#873): the plug-ins see a song that plays straight on.
    let mut p = project(vec![note(0.0, 1.0, 60)], 8.0, vec![plug(vst3())]);
    p.transport.loop_range = Some(LoopRange { start: 2.0, end: 8.0 });
    p.transport.loop_enabled = true;
    let stand = Stand::new(Behaviour::Gain(1.0));
    let songs = Arc::clone(&stand.songs);
    render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, EFFECT)])),
        SR,
        512,
        None,
    )
    .unwrap();
    let songs = songs.lock().unwrap();
    assert!(!songs.is_empty(), "told of the song");
    assert!(songs.iter().all(|song| song.cycle.is_none() && song.cycle_seconds.is_none()));
}

#[test]
fn a_plug_in_instrument_gets_the_notes_at_their_frames() {
    let p = project(vec![note(0.0, 1.0, 60), note(1.5, 0.25, 64)], 2.0, vec![plug(vst3())]);
    let seen: Arc<std::sync::Mutex<Vec<(usize, NoteEvent)>>> = Arc::default();
    for block in [64, 512, 1_000] {
        let mut stand = Stand::new(Behaviour::Instrument);
        stand.seen = Arc::clone(&seen);
        let out = render_offline_from(
            &p,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, INSTRUMENT)])),
            SR,
            block,
            None,
        )
        .unwrap();
        let out = left(&out);
        let second = BEAT * 3 / 2;
        // No sine voices: the plug-in plays the notes, at the track's unity gain.
        assert!((out[0] - 0.5).abs() < 1e-6, "block {block}: {}", out[0]);
        assert_eq!(out[BEAT - 1], out[0]);
        assert_eq!(out[BEAT], 0.0, "released on its frame");
        assert_eq!(out[second - 1], 0.0);
        assert!(out[second] > 0.0, "the second note starts on its frame");
        let events = std::mem::take(&mut *seen.lock().unwrap());
        let ons: Vec<usize> =
            events.iter().filter(|(_, e)| matches!(e, NoteEvent::On { .. })).map(|(at, _)| *at).collect();
        assert_eq!(ons, [0, second], "block {block}");
    }
}

/// A plug-in instrument on an armed track gets what a MIDI controller plays besides notes: its
/// controllers, pitch bend and channel pressure (#812).
#[test]
fn a_plug_in_instrument_on_an_armed_track_gets_live_controllers() {
    use crate::{LaunchCommand, LiveEvent};
    let p = project(Vec::new(), 1.0, vec![plug(vst3())]);
    let stand = Stand::new(Behaviour::Instrument);
    let seen = Arc::clone(&stand.seen);
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, INSTRUMENT)])),
        SR,
        RenderOptions::STOPPED,
    )
    .unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    blocks(&mut renderer, 1, 256);
    live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    live.send(LiveEvent::PitchBend(0.5));
    live.send(LiveEvent::Controller { number: 1, value: 1.0 });
    live.send(LiveEvent::Pressure(0.25));
    blocks(&mut renderer, 4, 256);
    let got: Vec<NoteEvent> = seen.lock().unwrap().iter().map(|(_, e)| *e).collect();
    for wanted in [
        NoteEvent::On { key: 60, velocity: 1.0 },
        NoteEvent::PitchBend(0.5),
        NoteEvent::Controller { number: 1, value: 1.0 },
        NoteEvent::Pressure(0.25),
    ] {
        assert!(got.contains(&wanted), "{wanted:?} missing from {got:?}");
    }
}

/// A track that stops hearing a controller, disarmed or its port closed, has the pitch bend
/// and pressure it was given set back, so its instrument is not left bent or pressed (#812); a
/// source that never moved them sends nothing.
#[test]
fn letting_go_of_a_controller_sets_its_pitch_bend_and_pressure_back() {
    use crate::{LaunchCommand, LiveEvent};
    let bent_after = |let_go: &dyn Fn(&mut crate::Controller, &mut crate::LiveSender), moved: bool| {
        let p = project(Vec::new(), 1.0, vec![plug(vst3())]);
        let stand = Stand::new(Behaviour::Instrument);
        let seen = Arc::clone(&stand.seen);
        let mut renderer = Renderer::build(
            &p,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, INSTRUMENT)])),
            SR,
            RenderOptions::STOPPED,
        )
        .unwrap();
        let mut controller = renderer.controller();
        let mut live = controller.take_live_input().unwrap();
        controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
        blocks(&mut renderer, 1, 256);
        live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
        if moved {
            live.send(LiveEvent::PitchBend(0.5));
            live.send(LiveEvent::Pressure(0.25));
        }
        blocks(&mut renderer, 2, 256);
        seen.lock().unwrap().clear();
        let_go(&mut controller, &mut live);
        blocks(&mut renderer, 2, 256);
        let got: Vec<NoteEvent> = seen.lock().unwrap().iter().map(|(_, e)| *e).collect();
        got
    };
    let disarm = |controller: &mut crate::Controller, _: &mut crate::LiveSender| {
        controller
            .send(LaunchCommand::SetArmed { track: TrackId(1), armed: false, input: Default::default() })
            .unwrap();
    };
    let close = |_: &mut crate::Controller, live: &mut crate::LiveSender| {
        live.send(LiveEvent::AllNotesOff);
    };
    for (what, let_go) in [("disarmed", &disarm as &dyn Fn(&mut _, &mut _)), ("port closed", &close)] {
        let got = bent_after(let_go, true);
        for wanted in [NoteEvent::PitchBend(0.0), NoteEvent::Pressure(0.0)] {
            assert!(got.contains(&wanted), "{what}: {wanted:?} missing from {got:?}");
        }
        let got = bent_after(let_go, false);
        assert!(
            !got.iter().any(|e| matches!(e, NoteEvent::PitchBend(_) | NoteEvent::Pressure(_))),
            "{what}, never moved: {got:?}"
        );
    }
}

/// Splits blocks, and refuses one unless it may wait (an offline render), as a host still
/// busy with an earlier block does; a block let through plays as an effect that silences the
/// sound, or an instrument that plays 0.5.
struct Busy {
    instrument: bool,
    sends: Arc<AtomicUsize>,
    processes: Arc<AtomicUsize>,
    /// The patience each block was handed over with.
    patience: Arc<std::sync::Mutex<Vec<Duration>>>,
}

impl Busy {
    fn new(instrument: bool) -> Self {
        Self { instrument, sends: Arc::default(), processes: Arc::default(), patience: Arc::default() }
    }
}

impl PluginProcessor for Busy {
    fn process(&mut self, _: &mut [f32], _: &[(usize, NoteEvent)], _: Duration) -> bool {
        self.processes.fetch_add(1, Ordering::SeqCst);
        false
    }
    fn send(&mut self, _: &[f32], _: &[(usize, NoteEvent)], patience: Duration) -> Sent {
        self.sends.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut seen) = self.patience.lock() {
            seen.push(patience);
        }
        if patience.is_zero() { Sent::Refused } else { Sent::Out }
    }
    fn wait(&mut self, buffer: &mut [f32], _: Duration) -> bool {
        buffer.fill(if self.instrument { 0.5 } else { 0.0 });
        true
    }
}

#[test]
fn a_block_a_plug_in_refuses_is_one_miss_and_is_not_run_again() {
    let notes = vec![note(0.0, 4.0, 69)];
    let dry_project = project(notes.clone(), 4.0, vec![device("utility", &[])]);
    let mut dry =
        Renderer::build(&dry_project, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let dry = blocks(&mut dry, 20, 480);
    let p = project(notes, 4.0, vec![plug(vst3())]);
    let busy = Busy::new(false);
    let (sends, processes, patience) =
        (Arc::clone(&busy.sends), Arc::clone(&busy.processes), Arc::clone(&busy.patience));
    let mut plugins = Plugins::new();
    plugins.insert(plugin_devices(&p)[0].key, EFFECT, Box::new(busy));
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let out = blocks(&mut renderer, 20, 480);
    assert_eq!(sends.load(Ordering::SeqCst), 20, "one hand-over a block");
    assert!(patience.lock().unwrap().iter().all(|p| p.is_zero()), "in real time nothing waits for a busy host");
    assert_eq!(processes.load(Ordering::SeqCst), 0, "a refused block is not run again");
    // Each block here is one round trip, so one miss a block.
    assert_eq!(renderer.graph.chains[0].missed(), 20, "one miss a round trip");
    // The effect is skipped: the dry sound.
    assert!(max_diff(&out, &dry) < 1e-6);
}

/// An instrument whose block is refused is silent for it, as when it misses its deadline.
#[test]
fn an_instrument_that_refuses_a_block_is_silent_for_it() {
    let p = project(vec![note(0.0, 4.0, 60)], 4.0, vec![plug(vst3())]);
    let busy = Busy::new(true);
    let mut plugins = Plugins::new();
    plugins.insert(plugin_devices(&p)[0].key, INSTRUMENT, Box::new(busy));
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let out = blocks(&mut renderer, 10, 480);
    assert_eq!(peak(&out), 0.0, "no sine voice, no plug-in sound: silence");
    assert_eq!(renderer.graph.chains[0].missed(), 10);
}

/// An offline render waits for a host busy with an earlier block (up to `OFFLINE_TIMEOUT`), so
/// no block is lost to it: what a restarted host refuses in real time, an offline render gets
/// through (ADR 0006, decision 4).
#[test]
fn an_offline_render_waits_for_a_busy_plug_in_and_loses_no_block() {
    let notes = vec![note(0.0, 2.0, 69)];
    for (instrument, info) in [(false, EFFECT), (true, INSTRUMENT)] {
        let p = project(notes.clone(), 2.0, vec![plug(vst3())]);
        let busy = Busy::new(instrument);
        let (processes, patience) = (Arc::clone(&busy.processes), Arc::clone(&busy.patience));
        let mut plugins = Plugins::new();
        plugins.insert(plugin_devices(&p)[0].key, info, Box::new(busy));
        let out =
            render_offline_from(&p, RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins), SR, 480, None)
                .unwrap();
        let patience = patience.lock().unwrap().clone();
        assert!(!patience.is_empty());
        assert!(patience.iter().all(|p| *p == OFFLINE_TIMEOUT), "offline every block may wait");
        assert_eq!(processes.load(Ordering::SeqCst), 0);
        if instrument {
            // The plug-in played every block: 0.5 throughout the notes' length.
            assert!((left(&out)[0] - 0.5).abs() < 1e-6);
            assert!((left(&out)[BEAT * 2 - 1] - 0.5).abs() < 1e-6);
        } else {
            assert_eq!(peak(&out), 0.0, "every block went through the silencing effect");
        }
    }
}

/// A processor that does not split blocks (the trait's default `send`) runs each block with
/// `process`, in real time and offline.
#[test]
fn a_processor_that_cannot_split_blocks_runs_them_whole() {
    struct Whole {
        processes: Arc<AtomicUsize>,
    }
    impl PluginProcessor for Whole {
        fn process(&mut self, buffer: &mut [f32], _: &[(usize, NoteEvent)], _: Duration) -> bool {
            self.processes.fetch_add(1, Ordering::SeqCst);
            buffer.fill(0.0);
            true
        }
    }
    let p = project(vec![note(0.0, 4.0, 69)], 4.0, vec![plug(vst3())]);
    let processes = Arc::new(AtomicUsize::new(0));
    let mut plugins = Plugins::new();
    plugins.insert(plugin_devices(&p)[0].key, EFFECT, Box::new(Whole { processes: Arc::clone(&processes) }));
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let out = blocks(&mut renderer, 10, 480);
    assert_eq!(processes.load(Ordering::SeqCst), 10, "one process a block");
    assert_eq!(peak(&out), 0.0);
    assert_eq!(renderer.graph.chains[0].missed(), 0);

    let processes = Arc::new(AtomicUsize::new(0));
    let mut plugins = Plugins::new();
    plugins.insert(plugin_devices(&p)[0].key, EFFECT, Box::new(Whole { processes: Arc::clone(&processes) }));
    let out = render_offline_from(&p, RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins), SR, 480, None)
        .unwrap();
    assert!(processes.load(Ordering::SeqCst) > 0);
    assert_eq!(peak(&out), 0.0);
}

#[test]
fn a_plug_in_that_misses_passes_the_sound_or_stays_silent_and_the_engine_plays_on() {
    let notes = vec![note(0.0, 4.0, 69)];
    let dry_project = project(notes.clone(), 4.0, vec![device("utility", &[])]);
    let mut dry =
        Renderer::build(&dry_project, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let dry = blocks(&mut dry, 40, 480);

    let p = project(notes.clone(), 4.0, vec![plug(vst3())]);
    let effect = Stand::new(Behaviour::Gain(0.5));
    let answering = Arc::clone(&effect.answering);
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(effect, EFFECT)])),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let mut out = blocks(&mut renderer, 20, 480);
    answering.store(false, Ordering::SeqCst);
    out.extend(blocks(&mut renderer, 20, 480));
    let at = 20 * 480 * 2;
    let half: Vec<f32> = dry[..at].iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&out[..at], &half) < 1e-6);
    // A crashed or hung effect: the dry sound, every block, without waiting.
    assert!(max_diff(&out[at..], &dry[at..]) < 1e-6);
    assert_eq!(renderer.graph.chains[0].missed(), 20);

    // An instrument that is gone is silent; a track without one still plays.
    let p = project(notes, 4.0, vec![plug(vst3())]);
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new())
            .with_plugins(plugins(&p, vec![(Stand::new(Behaviour::Gone), INSTRUMENT)])),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 20, 480)), 0.0);
}

#[test]
fn a_disabled_or_missing_plug_in_passes_the_sound_through() {
    let notes = vec![note(0.0, 1.0, 60)];
    let plain = render_offline(&project(notes.clone(), 1.0, vec![device("utility", &[])]), SR, 512).unwrap();
    let mut off = plug(vst3());
    off.enabled = false;
    let p = project(notes.clone(), 1.0, vec![off]);
    let out = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new())
            .with_plugins(plugins(&p, vec![(Stand::new(Behaviour::Gain(0.0)), EFFECT)])),
        SR,
        512,
        None,
    )
    .unwrap();
    assert!(max_diff(&out, &plain) < 1e-6);
    // Missing: no slot at all, the track plays as if it were not there.
    let p = project(notes, 1.0, vec![plug(vst3())]);
    let bare = render_offline(&p, SR, 512).unwrap();
    assert!(peak(&bare) > 0.1);
}

/// `notes` on two tracks: the first with `devices`, the second with a utility.
fn two_tracks(notes: Vec<Note>, devices: Vec<Device>) -> Project {
    let mut p = project(notes, 2.0, devices);
    let mut track = p.tracks[0].clone();
    track.id = TrackId(2);
    track.channel = Some(ChannelId(2));
    let mut channel = p.channels[0].clone();
    channel.id = ChannelId(2);
    channel.devices = vec![device("utility", &[])];
    let mut lane = p.arrangement.lanes[0].clone();
    lane.track = Some(TrackId(2));
    p.tracks.push(track);
    p.channels.push(channel);
    p.arrangement.lanes.push(lane);
    p.launcher.columns.clear();
    p
}

#[test]
fn latency_compensation_lines_every_track_up_with_the_slowest() {
    const LATENCY: usize = 300;
    let p = two_tracks(vec![note(0.0, 1.0, 69)], vec![plug(vst3())]);
    let out = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(
            &p,
            vec![(Stand::new(Behaviour::Delay(LATENCY)), PluginSlotInfo { latency: LATENCY as u32, ..EFFECT })],
        )),
        SR,
        256,
        None,
    )
    .unwrap();
    // Both tracks play the same note: lined up, the sum is twice one track, delayed by the
    // latency.
    let one = render_offline(&project(vec![note(0.0, 1.0, 69)], 2.0, vec![device("utility", &[])]), SR, 256).unwrap();
    let out = left(&out);
    let one = left(&one);
    assert!(peak(&out[..LATENCY]) == 0.0, "everything waits for the slowest track");
    let doubled: Vec<f32> = one[..BEAT].iter().map(|s| s * 2.0).collect();
    assert!(max_diff(&out[LATENCY..LATENCY + BEAT], &doubled) < 1e-5);
    let renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(
            &p,
            vec![(Stand::new(Behaviour::Gone), PluginSlotInfo { latency: LATENCY as u32, ..EFFECT })],
        )),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    assert_eq!(renderer.latency(), LATENCY as u32);
}

#[test]
fn a_plug_in_arrives_while_playing_and_stays_through_edits_and_restarts() {
    let notes = vec![note(0.0, 8.0, 69)];
    let p = project(notes.clone(), 8.0, vec![plug(vst3())]);
    let key = plugin_devices(&p)[0].key;
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let mut info = std::collections::HashMap::new();
    info.insert(key, EFFECT);
    controller.set_plugin_info(info);
    // Before it has loaded, the plug-in is not there: the sound passes.
    let before = blocks(&mut renderer, 5, 480);
    assert!(peak(&before) > 0.1);
    // The plug-in loads: it waits for a graph with its slot, then plays.
    let first = Stand::new(Behaviour::Gain(0.0));
    let first_dropped = Arc::clone(&first.dropped);
    controller.send_plugin(key, Box::new(first)).unwrap();
    blocks(&mut renderer, 2, 480);
    controller.update(&p).unwrap();
    blocks(&mut renderer, 2, 480);
    assert_eq!(peak(&blocks(&mut renderer, 5, 480)), 0.0, "the plug-in silences the track");

    // An edit around it (a device added in front) keeps it running.
    let mut edited = p.clone();
    edited.channels[0].devices.insert(0, device("utility", &[]));
    controller.update(&edited).unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 5, 480)), 0.0);
    controller.collect();
    assert_eq!(first_dropped.load(Ordering::SeqCst), 0, "the plug-in carried over");

    // A restarted plug-in replaces it; the old one goes back to be dropped off the audio
    // thread.
    controller.send_plugin(key, Box::new(Stand::new(Behaviour::Gain(1.0)))).unwrap();
    assert!(peak(&blocks(&mut renderer, 5, 480)) > 0.1);
    controller.collect();
    assert_eq!(first_dropped.load(Ordering::SeqCst), 1);

    // Removing the device ends it with the graph it was in.
    let mut removed = edited.clone();
    removed.channels[0].devices.remove(1);
    let second_dropped = {
        let stand = Stand::new(Behaviour::Gain(1.0));
        let dropped = Arc::clone(&stand.dropped);
        controller.send_plugin(key, Box::new(stand)).unwrap();
        dropped
    };
    blocks(&mut renderer, 2, 480);
    controller.update(&removed).unwrap();
    blocks(&mut renderer, 4, 480);
    controller.update(&removed).unwrap();
    blocks(&mut renderer, 4, 480);
    controller.collect();
    assert_eq!(second_dropped.load(Ordering::SeqCst), 1);
}

/// A muted track (or one left out by a solo) keeps its chain, so its plug-in keeps its
/// process and sounds again when the track does; a chain left out of the graph would give the
/// processor back and the plug-in would stay silent for good.
#[test]
fn a_plug_in_on_a_muted_track_keeps_running_and_sounds_again() {
    let p = project(vec![note(0.0, 16.0, 60)], 16.0, vec![plug(vst3())]);
    let key = plugin_devices(&p)[0].key;
    let stand = Stand::new(Behaviour::Instrument);
    let dropped = Arc::clone(&stand.dropped);
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, INSTRUMENT)])),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let mut controller = renderer.controller();
    assert!(peak(&blocks(&mut renderer, 5, 480)) > 0.1);
    let mut muted = p.clone();
    muted.channels[0].mute = true;
    controller.update(&muted).unwrap();
    blocks(&mut renderer, 2, 480);
    assert_eq!(peak(&blocks(&mut renderer, 5, 480)), 0.0, "muted");
    controller.update(&p).unwrap();
    blocks(&mut renderer, 2, 480);
    assert!(peak(&blocks(&mut renderer, 5, 480)) > 0.1, "sounds again");
    controller.collect();
    assert_eq!(dropped.load(Ordering::SeqCst), 0, "the plug-in carried over");
    let _ = key;
}

/// A stopped preview's note-off reaches the track's instrument on the first frame of the block
/// the stop is applied in, whatever the block size (#156); it came one block late.
#[test]
fn a_stopped_preview_releases_its_note_on_the_frame_the_stop_applies() {
    for size in [512usize, 300] {
        let p = project(Vec::new(), 8.0, vec![plug(vst3())]);
        let stand = Stand::new(Behaviour::Instrument);
        let seen = Arc::clone(&stand.seen);
        let mut renderer = Renderer::build(
            &p,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, INSTRUMENT)])),
            SR,
            RenderOptions::PLAYING,
        )
        .unwrap();
        let mut controller = renderer.controller();
        let notes = crate::preview_notes([(0.0, 64.0, 60, 1.0)], 120.0, SR, Some(TrackId(1)), None);
        controller.preview_notes(notes, 1.0).unwrap();
        blocks(&mut renderer, 2, size);
        controller.preview_stop().unwrap();
        blocks(&mut renderer, 2, size);
        let seen = seen.lock().unwrap();
        let off = seen.iter().find(|(_, e)| matches!(e, NoteEvent::Off { key: 60, .. })).map(|(at, _)| *at);
        assert_eq!(off, Some(2 * size), "block {size}: {seen:?}");
    }
}

#[test]
fn a_plug_in_instrument_is_released_when_the_transport_stops() {
    let p = project(vec![note(0.0, 8.0, 60)], 8.0, vec![plug(vst3())]);
    let stand = Stand::new(Behaviour::Instrument);
    let seen = Arc::clone(&stand.seen);
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(stand, INSTRUMENT)])),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    assert!(peak(&blocks(&mut renderer, 10, 480)) > 0.1);
    renderer.handle(crate::LaunchCommand::Stop);
    assert_eq!(peak(&blocks(&mut renderer, 10, 480)[960..]), 0.0);
    // Each held key is released on its own before the all-notes-off, for instruments that
    // ignore it.
    let seen = seen.lock().unwrap();
    let off = seen.iter().position(|(_, e)| matches!(e, NoteEvent::Off { key: 60, .. }));
    let all = seen.iter().position(|(_, e)| *e == NoteEvent::AllNotesOff);
    assert!(off.is_some() && all.is_some() && off < all, "{seen:?}");
}

/// A new audio output (another buffer size, the same sample rate) builds a new renderer: the
/// old one gives its running processors back, including one still in its queue, and the new
/// one plays them without a restart.
#[test]
fn a_renderer_hands_its_plug_ins_to_the_next_one() {
    let notes = vec![note(0.0, 8.0, 69)];
    let p = project(notes, 8.0, vec![plug(vst3())]);
    let key = plugin_devices(&p)[0].key;
    let first = Stand::new(Behaviour::Gain(0.0));
    let dropped = Arc::clone(&first.dropped);
    let mut renderer = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(&p, vec![(first, EFFECT)])),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 4, 480)), 0.0, "the plug-in silences the track");

    let plugins = renderer.into_plugins();
    assert_eq!(dropped.load(Ordering::SeqCst), 0, "handed over, not dropped");
    assert_eq!(plugins.info().get(&key), Some(&EFFECT));
    let mut next = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    assert_eq!(peak(&blocks(&mut next, 4, 256)), 0.0, "the same plug-in plays in the new renderer");

    // One sent but not placed yet comes back too.
    let mut controller = next.controller();
    let queued = Stand::new(Behaviour::Gain(1.0));
    let queued_dropped = Arc::clone(&queued.dropped);
    controller.send_plugin(key, Box::new(queued)).unwrap();
    let back = next.into_plugins();
    assert_eq!(queued_dropped.load(Ordering::SeqCst), 0);
    assert_eq!(dropped.load(Ordering::SeqCst), 1, "the queued one, newer, replaced it");
    let mut third =
        Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()).with_plugins(back), SR, RenderOptions::PLAYING)
            .unwrap();
    assert!(peak(&blocks(&mut third, 4, 256)) > 0.1, "the queued plug-in plays");
}

/// A plug-in that arrived while playing (its slot told to the controller after the renderer
/// was built, as the session does) is handed over too, with what its slot knew.
#[test]
fn a_plug_in_that_arrived_while_playing_is_handed_over_with_its_slot() {
    let p = project(vec![note(0.0, 8.0, 69)], 8.0, vec![plug(vst3())]);
    let key = plugin_devices(&p)[0].key;
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let mut info = std::collections::HashMap::new();
    let latency = PluginSlotInfo { instrument: false, latency: 32 };
    info.insert(key, latency);
    controller.set_plugin_info(info);
    let stand = Stand::new(Behaviour::Gain(0.0));
    let dropped = Arc::clone(&stand.dropped);
    controller.send_plugin(key, Box::new(stand)).unwrap();
    blocks(&mut renderer, 2, 480);
    controller.update(&p).unwrap();
    blocks(&mut renderer, 4, 480);
    assert_eq!(peak(&blocks(&mut renderer, 4, 480)), 0.0, "the plug-in plays");

    let plugins = renderer.into_plugins();
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(plugins.info().get(&key), Some(&latency));
    let mut next = Renderer::build(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    assert_eq!(peak(&blocks(&mut next, 40, 256)), 0.0, "and plays on in the next renderer");
    drop(controller);
}

/// A stand-in for a plug-in in a host process that runs several: it scales the sound by its
/// gain, and runs the plug-ins a joint block names after it in the same round trip, as the
/// host does.
struct Member {
    joint: crate::Joint,
    gain: f32,
    /// The gains of every member of its process, by number.
    process: Arc<std::sync::Mutex<std::collections::HashMap<u32, f32>>>,
    /// Round trips to its process (any member's).
    trips: Arc<AtomicUsize>,
    output: Vec<f32>,
    followed: Arc<std::sync::Mutex<Vec<crate::Followed>>>,
    /// Set false to make its process stop answering.
    answering: bool,
    /// The parameter events it was given ([`PluginProcessor::automate`]), by id.
    automated: Arc<std::sync::Mutex<Vec<u32>>>,
}

impl Member {
    fn new(
        process: u64,
        member: u32,
        gain: f32,
        gains: &Arc<std::sync::Mutex<std::collections::HashMap<u32, f32>>>,
        trips: &Arc<AtomicUsize>,
    ) -> Self {
        gains.lock().unwrap().insert(member, gain);
        Self {
            joint: crate::Joint { process, member },
            gain,
            process: Arc::clone(gains),
            trips: Arc::clone(trips),
            output: Vec::new(),
            followed: Arc::default(),
            answering: true,
            automated: Arc::default(),
        }
    }
}

impl PluginProcessor for Member {
    fn process(&mut self, buffer: &mut [f32], _events: &[(usize, NoteEvent)], _timeout: Duration) -> bool {
        self.trips.fetch_add(1, Ordering::SeqCst);
        buffer.iter_mut().for_each(|s| *s *= self.gain);
        true
    }

    fn joint(&self) -> Option<crate::Joint> {
        Some(self.joint)
    }

    fn send(&mut self, buffer: &[f32], events: &[(usize, NoteEvent)], patience: Duration) -> Sent {
        self.send_joint(buffer, events, &[], patience)
    }

    fn send_joint(
        &mut self,
        buffer: &[f32],
        _events: &[(usize, NoteEvent)],
        followers: &[u32],
        _patience: Duration,
    ) -> Sent {
        self.trips.fetch_add(1, Ordering::SeqCst);
        let gains = self.process.lock().unwrap();
        // One plug-in after the other, as the host runs them.
        self.output = buffer.iter().map(|s| s * self.gain).collect();
        for follower in followers {
            let gain = gains.get(follower).copied().unwrap_or(1.0);
            self.output.iter_mut().for_each(|s| *s *= gain);
        }
        Sent::Out
    }

    fn wait(&mut self, buffer: &mut [f32], _timeout: Duration) -> bool {
        if self.answering {
            buffer.copy_from_slice(&self.output);
        }
        self.answering
    }

    fn followed(&mut self, _events: &[(usize, NoteEvent)], how: crate::Followed) {
        self.followed.lock().unwrap().push(how);
    }

    fn automate(&mut self, params: &[crate::ParamEvent]) {
        self.automated.lock().unwrap().extend(params.iter().map(|p| p.id));
    }
}

#[test]
fn consecutive_plug_ins_of_one_process_share_a_round_trip_and_sound_the_same() {
    let clap = |id: &str| plug(DeviceKind::Clap { id: id.into() });
    let mut off = clap("c");
    off.enabled = false;
    // a, b, (c disabled), d share process 1; a built-in device; e in process 1; f in process 2.
    let devices = vec![clap("a"), clap("b"), off, clap("d"), device("utility", &[]), clap("e"), clap("f")];
    let p = project(vec![note(0.0, 2.0, 69)], 2.0, devices);
    let gains = [0.5, 0.8, 0.1, 0.9, 0.7, 0.6];
    let render = |processes: [u64; 6]| {
        let table = Arc::default();
        let trips = Arc::new(AtomicUsize::new(0));
        let mut plugins = Plugins::new();
        let mut followed = Vec::new();
        for (n, found) in plugin_devices(&p).into_iter().enumerate() {
            let member = Member::new(processes[n], n as u32 + 1, gains[n], &table, &trips);
            followed.push(Arc::clone(&member.followed));
            plugins.insert(found.key, EFFECT, Box::new(member));
        }
        let out =
            render_offline_from(&p, RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins), SR, 512, None)
                .unwrap();
        let hows: Vec<Vec<crate::Followed>> = followed.iter().map(|f| f.lock().unwrap().clone()).collect();
        (out, trips.load(Ordering::SeqCst), hows)
    };
    // Each in a process of its own: five round trips a block (the disabled one makes none).
    let (alone, alone_trips, alone_followed) = render([1, 2, 3, 4, 5, 6]);
    // a, b and d in one round trip; e alone (after a built-in device); f in its own process.
    let (joint, joint_trips, followed) = render([1, 1, 1, 1, 1, 2]);
    let blocks = alone_trips / 5;
    assert!(blocks > 0);
    assert_eq!(alone_trips, blocks * 5);
    assert_eq!(joint_trips, blocks * 3);
    assert!(alone_followed.iter().all(Vec::is_empty));
    // b and d followed a in every block and were answered; nobody else followed anyone.
    for (n, hows) in followed.iter().enumerate() {
        let expected = if n == 1 || n == 3 { blocks } else { 0 };
        assert_eq!(hows.len(), expected, "plug-in {n}");
        assert!(hows.iter().all(|h| *h == crate::Followed::Answered));
    }
    // Sample for sample the same sound.
    assert_eq!(alone.len(), joint.len());
    assert!(alone.iter().zip(&joint).all(|(a, b)| a.to_bits() == b.to_bits()));
    assert!(peak(&joint) > 0.0);
}

#[test]
fn a_joint_block_that_misses_is_a_miss_for_every_plug_in_in_it() {
    let clap = |id: &str| plug(DeviceKind::Clap { id: id.into() });
    let p = project(vec![note(0.0, 2.0, 69)], 2.0, vec![clap("a"), clap("b")]);
    let (table, trips) = (Arc::default(), Arc::new(AtomicUsize::new(0)));
    let mut first = Member::new(1, 1, 0.5, &table, &trips);
    first.answering = false;
    let second = Member::new(1, 2, 0.5, &table, &trips);
    let followed = Arc::clone(&second.followed);
    let found = plugin_devices(&p);
    let mut plugins = Plugins::new();
    plugins.insert(found[0].key, EFFECT, Box::new(first));
    // The second is an instrument: a miss leaves it silent, as when it misses on its own.
    plugins.insert(found[1].key, INSTRUMENT, Box::new(second));
    let out = render_offline_from(&p, RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins), SR, 512, None)
        .unwrap();
    assert_eq!(peak(&out), 0.0);
    let hows = followed.lock().unwrap();
    assert!(!hows.is_empty());
    assert!(hows.iter().all(|h| *h == crate::Followed::Late));
    assert_eq!(trips.load(Ordering::SeqCst), hows.len(), "one round trip a block, for both");
}

#[test]
fn a_plug_in_with_parameter_events_runs_on_its_own_and_gets_them() {
    let clap = |id: &str| plug(DeviceKind::Clap { id: id.into() });
    // b has a value set for parameter 7: it goes to b at the start of the first block.
    let mut set = clap("b");
    set.params = vec![DeviceParam::new("7", 0.5)];
    let p = project(vec![note(0.0, 2.0, 69)], 2.0, vec![clap("a"), set]);
    let (table, trips) = (Arc::default(), Arc::new(AtomicUsize::new(0)));
    let first = Member::new(1, 1, 0.5, &table, &trips);
    let second = Member::new(1, 2, 0.8, &table, &trips);
    let (followed, automated) = (Arc::clone(&second.followed), Arc::clone(&second.automated));
    let found = plugin_devices(&p);
    let mut plugins = Plugins::new();
    plugins.insert(found[0].key, EFFECT, Box::new(first));
    plugins.insert(found[1].key, EFFECT, Box::new(second));
    render_offline_from(&p, RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins), SR, 512, None).unwrap();
    assert_eq!(*automated.lock().unwrap(), [7], "the value reached b, once");
    // The first block runs a and b in a round trip each, as the joint block's events are a's;
    // every later block runs them in one.
    let joint = followed.lock().unwrap().len();
    assert!(joint > 0);
    assert_eq!(trips.load(Ordering::SeqCst), joint + 2);
}

fn grouped(branches: Vec<Vec<Device>>) -> Device {
    let branches = branches.into_iter().map(|devices| Branch { devices, ..Branch::new("B") }).collect();
    Device::group("G", DeviceGroup::with_branches(branches))
}

#[test]
fn a_plug_in_runs_in_a_group_branch_beside_a_dry_one() {
    let notes = vec![note(0.0, 2.0, 69)];
    let dry = render_offline(&project(notes.clone(), 2.0, vec![device("utility", &[])]), SR, 512).unwrap();
    let p = project(notes, 2.0, vec![device("utility", &[]), grouped(vec![vec![plug(vst3())], vec![]])]);
    let out = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new())
            .with_plugins(plugins(&p, vec![(Stand::new(Behaviour::Gain(0.5)), EFFECT)])),
        SR,
        512,
        None,
    )
    .unwrap();
    let len = dry.len().min(out.len());
    let one_and_a_half: Vec<f32> = dry[..len].iter().map(|s| s * 1.5).collect();
    assert!(max_diff(&out[..len], &one_and_a_half) < 1e-5, "{}", max_diff(&out[..len], &one_and_a_half));
}

#[test]
fn a_plug_ins_latency_is_compensated_inside_its_group() {
    let notes = vec![note(0.0, 2.0, 69)];
    let delayed = PluginSlotInfo { instrument: false, latency: 64 };
    let single = project(notes.clone(), 2.0, vec![device("utility", &[]), plug(vst3())]);
    let reference = render_offline_from(
        &single,
        RenderInputs::from_audio(&AudioPool::new())
            .with_plugins(plugins(&single, vec![(Stand::new(Behaviour::Delay(64)), delayed)])),
        SR,
        512,
        None,
    )
    .unwrap();
    let p = project(notes, 2.0, vec![device("utility", &[]), grouped(vec![vec![plug(vst3())], vec![]])]);
    let out = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new())
            .with_plugins(plugins(&p, vec![(Stand::new(Behaviour::Delay(64)), delayed)])),
        SR,
        512,
        None,
    )
    .unwrap();
    let len = reference.len().min(out.len());
    let doubled: Vec<f32> = reference[..len].iter().map(|s| s * 2.0).collect();
    assert!(peak(&reference) > 0.1);
    assert!(max_diff(&out[..len], &doubled) < 1e-5, "{}", max_diff(&out[..len], &doubled));
}

#[test]
fn plug_in_instruments_in_a_group_are_layered_and_a_gone_one_is_silent() {
    let notes = vec![note(0.0, 1.0, 60)];
    let p = project(notes, 2.0, vec![grouped(vec![vec![plug(vst3())], vec![plug(vst3())]])]);
    let out = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(
            &p,
            vec![(Stand::new(Behaviour::Instrument), INSTRUMENT), (Stand::new(Behaviour::Instrument), INSTRUMENT)],
        )),
        SR,
        512,
        None,
    )
    .unwrap();
    // Each plays 0.5 while the note sounds; the track's gains scale the sum the same for both.
    let one = render_offline_from(
        &project(vec![note(0.0, 1.0, 60)], 2.0, vec![plug(vst3())]),
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(
            &project(vec![note(0.0, 1.0, 60)], 2.0, vec![plug(vst3())]),
            vec![(Stand::new(Behaviour::Instrument), INSTRUMENT)],
        )),
        SR,
        512,
        None,
    )
    .unwrap();
    let len = one.len().min(out.len());
    let doubled: Vec<f32> = one[..len].iter().map(|s| s * 2.0).collect();
    assert!(peak(&one) > 0.1);
    assert!(max_diff(&out[..len], &doubled) < 1e-5);
    // One of them gone: the other plays alone, the gone one is silent and the render goes on.
    let half = render_offline_from(
        &p,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins(
            &p,
            vec![(Stand::new(Behaviour::Instrument), INSTRUMENT), (Stand::new(Behaviour::Gone), INSTRUMENT)],
        )),
        SR,
        512,
        None,
    )
    .unwrap();
    assert!(max_diff(&half[..len], &one[..len]) < 1e-5, "{}", max_diff(&half[..len], &one[..len]));
}

#[test]
fn a_plug_in_keeps_running_when_it_moves_into_a_group() {
    let notes = vec![note(0.0, 8.0, 69)];
    let p = project(notes, 8.0, vec![plug(vst3())]);
    let key = plugin_devices(&p)[0].key;
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let mut info = std::collections::HashMap::new();
    info.insert(key, EFFECT);
    controller.set_plugin_info(info);
    let stand = Stand::new(Behaviour::Gain(0.0));
    let dropped = Arc::clone(&stand.dropped);
    controller.send_plugin(key, Box::new(stand)).unwrap();
    blocks(&mut renderer, 2, 480);
    controller.update(&p).unwrap();
    blocks(&mut renderer, 2, 480);
    assert_eq!(peak(&blocks(&mut renderer, 5, 480)), 0.0, "the plug-in silences the track");

    // Into a group of one branch: the same key, the same process, still silencing.
    let mut edited = p.clone();
    let moved = edited.channels[0].devices.remove(0);
    edited.channels[0].devices.push(grouped(vec![vec![moved]]));
    assert_eq!(plugin_devices(&edited)[0].key, key);
    controller.update(&edited).unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 10, 480)), 0.0);
    controller.collect();
    assert_eq!(dropped.load(Ordering::SeqCst), 0, "the plug-in carried over into the group");

    // A restart reaches it inside the group.
    controller.send_plugin(key, Box::new(Stand::new(Behaviour::Gain(1.0)))).unwrap();
    assert!(peak(&blocks(&mut renderer, 5, 480)) > 0.1);
}
