// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device chains in the renderer: effects on a track, instruments playing its notes, launched
//! clips, the transport, graph swaps.

use crate::{RenderInputs, RenderOptions};
use suhina_model::*;

use crate::{LaunchCommand, Renderer, render_launches, render_offline};

pub(crate) const SR: u32 = 48_000;
/// Frames per beat at 120 bpm.
pub(crate) const BEAT: usize = SR as usize / 2;

pub(crate) fn note(time: f64, duration: f64, key: u8) -> Note {
    Note::new(time, duration, key, 1.0)
}

pub(crate) fn device(id: &str, params: &[(&str, f64)]) -> Device {
    Device {
        params: params.iter().map(|(p, v)| DeviceParam::new(*p, *v)).collect(),
        ..Device::new(id, DeviceKind::Builtin(id.into()))
    }
}

/// One track playing `notes` from an arrangement clip of `length` beats and, in its launcher
/// slot, a looping clip of the same notes; `devices` on its channel.
pub(crate) fn project(notes: Vec<Note>, length: f64, devices: Vec<Device>) -> Project {
    let clip = |start: f64, looping: bool| Clip {
        loop_range: looping.then_some(LoopRange { start: 0.0, end: length }),
        ..Clip::new(ContentId(1), start, length)
    };
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel { devices, ..Channel::new(ChannelId(1), "C") }],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(0.0, false)],
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            mappings: Vec::new(),
            scenes: vec![Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot { clip: Some(clip(0.0, true)), has_stop: true }],
            }],
        },
        ..Project::default()
    }
}

pub(crate) fn left(out: &[f32]) -> Vec<f32> {
    out.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect()
}

pub(crate) fn peak(out: &[f32]) -> f32 {
    out.iter().fold(0.0, |m, s| m.max(s.abs()))
}

pub(crate) fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()))
}

/// Frequency from upward zero crossings (mono).
fn frequency(signal: &[f32]) -> f64 {
    let crossings: Vec<usize> =
        signal.windows(2).enumerate().filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0).map(|(i, _)| i).collect();
    let (Some(first), Some(last)) = (crossings.first(), crossings.last()) else { return 0.0 };
    (crossings.len() - 1) as f64 * f64::from(SR) / (last - first) as f64
}

pub(crate) fn blocks(renderer: &mut Renderer, count: usize, frames: usize) -> Vec<f32> {
    let mut out = Vec::new();
    let mut block = vec![0.0f32; frames * 2];
    for _ in 0..count {
        renderer.process(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

#[test]
fn an_effect_on_a_track_processes_its_sound_before_the_fader() {
    let notes = vec![note(0.0, 2.0, 69)];
    let plain = render_offline(&project(notes.clone(), 2.0, vec![]), SR, 512).unwrap();
    let half =
        render_offline(&project(notes.clone(), 2.0, vec![device("utility", &[("gain", -6.0206)])]), SR, 512).unwrap();
    let neutral = render_offline(&project(notes.clone(), 2.0, vec![device("utility", &[])]), SR, 512).unwrap();
    // The chain adds a tail to the render.
    assert!(half.len() > plain.len());
    assert!(max_diff(&neutral[..plain.len()], &plain) < 1e-5, "{}", max_diff(&neutral[..plain.len()], &plain));
    let scaled: Vec<f32> = plain.iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&half[..plain.len()], &scaled) < 1e-4);
    assert!(peak(&half[plain.len()..]) < 1e-6, "the tail is silent");

    // The fader comes after the chain: pan moves the processed sound.
    let mut panned = project(notes.clone(), 2.0, vec![device("utility", &[])]);
    panned.channels[0].pan = 1.0;
    let panned = render_offline(&panned, SR, 512).unwrap();
    let (l, r): (Vec<f32>, Vec<f32>) = panned.as_chunks::<2>().0.iter().map(|[l, r]| (*l, *r)).unzip();
    assert!(peak(&l) < 1e-6 && peak(&r) > 0.1);
}

#[test]
fn an_sd_effect_plays_its_stored_parameter_value() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.gain", "Test Gain", suhina_device_sdk::DeviceRole::Effect);
    graph.parameters.push(suhina_device_sdk::Parameter::new("gain", "Gain", (0.0, 2.0), 1.0));
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("in"),
        suhina_device_sdk::Node::gain("gain", "gain"),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
    // Out of range values are limited as the parameter takes them: 3 plays as 2.
    for (stored, gain) in [(0.5, 0.5_f32), (3.0, 2.0)] {
        let mut effect = device("sd", &[("gain", stored)]);
        effect.kind = DeviceKind::Sd;
        effect.state = graph.to_sd().ok().map(String::into_bytes);
        let notes = vec![note(0.0, 1.0, 69)];
        let plain = render_offline(&project(notes.clone(), 1.0, vec![device("fm-synth", &[])]), SR, 512).unwrap();
        let processed = render_offline(&project(notes, 1.0, vec![device("fm-synth", &[]), effect]), SR, 512).unwrap();
        let expected: Vec<f32> = plain.iter().map(|sample| sample * gain).collect();
        assert!(max_diff(&processed[..plain.len()], &expected) < 1e-5);
    }
}

#[test]
fn an_sd_gain_graph_runs_in_the_offline_device_chain() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.gain", "Test Gain", suhina_device_sdk::DeviceRole::Effect);
    let mut gain = suhina_device_sdk::Parameter::new("gain", "Gain", (0.0, 1.0), 1.0);
    gain.control = suhina_device_sdk::ParameterControl::Toggle;
    graph.parameters.push(gain);
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("in"),
        suhina_device_sdk::Node::gain("gain", "gain"),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
    let mut effect = device("sd", &[]);
    effect.kind = DeviceKind::Sd;
    effect.state = graph.to_sd().ok().map(String::into_bytes);
    effect.automation = vec![AutomationLane {
        param: "gain".into(),
        points: vec![AutomationPoint::new(0.0, 1.0), AutomationPoint::new(0.5, 0.0)],
        enabled: true,
    }];
    let notes = vec![note(0.0, 1.0, 69)];
    let plain = render_offline(&project(notes.clone(), 1.0, vec![device("fm-synth", &[])]), SR, 512).unwrap();
    let processed = render_offline(&project(notes, 1.0, vec![device("fm-synth", &[]), effect]), SR, 512).unwrap();
    assert!(max_diff(&processed[..BEAT], &plain[..BEAT]) < 1e-5);
    assert!(processed[BEAT..].iter().all(|sample| sample.abs() < 1e-6));
}

#[test]
fn an_sd_midi_effect_transposes_notes_before_the_instrument() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.transpose", "Transpose", suhina_device_sdk::DeviceRole::MidiEffect);
    let mut transpose_param = suhina_device_sdk::Parameter::new("semitones", "Semitones", (-12.0, 12.0), 12.0);
    transpose_param.control = suhina_device_sdk::ParameterControl::Integer;
    graph.parameters.push(transpose_param);
    graph.nodes = vec![
        suhina_device_sdk::Node::new("in", suhina_device_sdk::NodeKind::MidiInput),
        suhina_device_sdk::Node::new(
            "up",
            suhina_device_sdk::NodeKind::MidiTransposeParameter { parameter: "semitones".into() },
        ),
        suhina_device_sdk::Node::new("out", suhina_device_sdk::NodeKind::MidiOutput),
    ];
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "up"), suhina_device_sdk::Connection::new("up", "out")];
    let mut transpose = device("sd", &[]);
    transpose.kind = DeviceKind::Sd;
    transpose.state = graph.to_sd().ok().map(String::into_bytes);
    transpose.automation = vec![AutomationLane {
        param: "semitones".into(),
        points: vec![AutomationPoint::new(0.0, 12.0), AutomationPoint::new(0.5, 0.0)],
        enabled: true,
    }];
    let notes = vec![note(0.0, 0.75, 48), note(0.8, 0.2, 48)];
    let original = render_offline(&project(notes.clone(), 1.0, vec![device("fm-synth", &[])]), SR, 512).unwrap();
    let shifted = render_offline(&project(notes, 1.0, vec![transpose, device("fm-synth", &[])]), SR, 512).unwrap();
    let expected = render_offline(
        &project(vec![note(0.0, 0.75, 60), note(0.8, 0.2, 48)], 1.0, vec![device("fm-synth", &[])]),
        SR,
        512,
    )
    .unwrap();
    assert!(peak(&shifted) > 1e-3);
    assert!(max_diff(&expected, &shifted) < 1e-6);
    assert!(max_diff(&original, &shifted) > 1e-3);
}

#[test]
fn an_sd_instrument_renders_from_timestamped_note_events() {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.gated-tone", "Gated Tone", suhina_device_sdk::DeviceRole::Instrument);
    graph.nodes = vec![
        suhina_device_sdk::Node::new("gate", suhina_device_sdk::NodeKind::MidiGate),
        suhina_device_sdk::Node::new("frequency", suhina_device_sdk::NodeKind::Constant { value: 440.0 }),
        suhina_device_sdk::Node::new(
            "osc",
            suhina_device_sdk::NodeKind::Oscillator { shape: suhina_device_sdk::Waveform::Sine },
        ),
        suhina_device_sdk::Node::new("amp", suhina_device_sdk::NodeKind::Multiply),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.connections = vec![
        suhina_device_sdk::Connection::ports("frequency", "out", "osc", "frequency"),
        suhina_device_sdk::Connection::ports("gate", "out", "amp", "a"),
        suhina_device_sdk::Connection::ports("osc", "out", "amp", "b"),
        suhina_device_sdk::Connection::new("amp", "out"),
    ];
    let mut instrument = device("sd", &[]);
    instrument.kind = DeviceKind::Sd;
    instrument.state = graph.to_sd().ok().map(String::into_bytes);
    let audio = render_offline(&project(vec![note(0.0, 0.1, 60)], 1.0, vec![instrument]), SR, 512).unwrap();
    assert!(audio.iter().any(|sample| sample.abs() > 0.01));
    assert!(audio.iter().skip(BEAT / 10 * 2).all(|sample| sample.abs() < 1e-6));
}

#[test]
fn unknown_and_disabled_devices_pass_the_sound_through() {
    let notes = vec![note(0.0, 1.0, 60)];
    let plain = render_offline(&project(notes.clone(), 1.0, vec![]), SR, 512).unwrap();
    let mut off = device("utility", &[("gain", -40.0)]);
    off.enabled = false;
    let mut plugin = device("x", &[]);
    plugin.kind = DeviceKind::Clap { id: "com.example.reverb".into() };
    for devices in [vec![device("no-such-device", &[])], vec![off], vec![plugin]] {
        assert_eq!(render_offline(&project(notes.clone(), 1.0, devices), SR, 512).unwrap(), plain);
    }
}

#[test]
fn an_instrument_plays_the_tracks_notes() {
    let notes = vec![note(0.0, 1.0, 69), note(2.0, 1.0, 81)];
    let out = render_offline(&project(notes, 4.0, vec![device("fm-synth", &[])]), SR, 512).unwrap();
    let out = left(&out);
    // Key 69 is 440 Hz, key 81 an octave up; the synth's sine, not the fallback voices.
    assert!((frequency(&out[2_400..BEAT - 2_400]) - 440.0).abs() < 1.0);
    assert!((frequency(&out[BEAT * 2 + 2_400..BEAT * 3 - 2_400]) - 880.0).abs() < 2.0);
    // One operator at full level is -18 dB (an eighth), at the track's unity gain.
    let level = peak(&out[2_400..BEAT - 2_400]);
    assert!((0.11..0.14).contains(&level), "{level}");
    // Released after its beat: silent between the notes and at the end.
    assert!(peak(&out[BEAT + 4_800..BEAT * 2]) < 1e-4);
    assert!(peak(&out[BEAT * 3 + 4_800..]) < 1e-4);
}

#[test]
fn a_chain_sounds_the_same_in_any_block_size() {
    let notes = vec![note(0.0, 0.5, 60), note(0.31, 0.7, 64), note(1.13, 0.2, 67), note(1.5, 1.0, 60)];
    for devices in [
        vec![device("fm-synth", &[("feedback", 5.0)]), device("overdrive", &[])],
        vec![device("auto-pan", &[]), device("utility", &[("width", 0.0)])],
    ] {
        let p = project(notes.clone(), 3.0, devices);
        let reference = render_offline(&p, SR, 512).unwrap();
        for size in [1, 64, 333, 4_096] {
            let other = render_offline(&p, SR, size).unwrap();
            assert!(max_diff(&reference, &other) < 1e-6, "block {size}: {}", max_diff(&reference, &other));
        }
    }
}

#[test]
fn launched_clips_play_on_the_instrument_and_stop() {
    let p = project(vec![note(0.0, 0.5, 69)], 1.0, vec![device("fm-synth", &[])]);
    let commands = [
        (0.0, LaunchCommand::SetQuantization(crate::Quantization::None)),
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        (2.0, LaunchCommand::StopColumn { column: 0 }),
    ];
    let out = left(&render_launches(&p, &crate::AudioPool::new(), SR, &commands, 3.0).unwrap());
    // The clip loops every beat (0.5 s): a note in each.
    for beat in 0..4 {
        let from = beat * BEAT + 1_000;
        assert!(peak(&out[from..from + 4_000]) > 0.05, "beat {beat}");
    }
    assert!(peak(&out[SR as usize * 5 / 2..]) < 1e-4, "stopped");
}

#[test]
fn stopping_the_transport_releases_an_instruments_notes() {
    let p = project(vec![note(0.0, 8.0, 60)], 8.0, vec![device("fm-synth", &[])]);
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let before = blocks(&mut renderer, 20, 480);
    assert!(peak(&before[9_600..]) > 0.05);
    renderer.handle(LaunchCommand::Stop);
    let after = blocks(&mut renderer, 100, 480);
    assert!(peak(&after[24_000..]) < 1e-4, "{}", peak(&after[24_000..]));
}

#[test]
fn an_edit_keeps_a_chain_playing_and_takes_its_new_values() {
    let p = project(vec![note(0.0, 8.0, 69)], 8.0, vec![device("fm-synth", &[]), device("utility", &[])]);
    let mut edited = p.clone();
    edited.channels[0].devices[1].params = vec![DeviceParam::new("gain", -6.0206)];
    let mut renderer =
        Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 50, 480);
    controller.update(&edited).unwrap();
    out.extend(blocks(&mut renderer, 100, 480));
    let mut untouched =
        Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let reference = blocks(&mut untouched, 150, 480);
    // The note carries on through the swap (no new attack): after the utility's 20 ms ramp the
    // output is the untouched render at half the level.
    // No stretched clips: the new graph takes the next block.
    let at = 50 * 480;
    assert_eq!(max_diff(&out[..at * 2], &reference[..at * 2]), 0.0);
    let settled = (at + 2_400) * 2;
    let half: Vec<f32> = reference[settled..].iter().map(|s| s * 0.5).collect();
    assert!(max_diff(&out[settled..], &half) < 1e-4, "{}", max_diff(&out[settled..], &half));
}

#[test]
fn hostile_notes_and_devices_do_not_break_a_render() {
    let mut notes: Vec<Note> = (0..200).map(|i| note(f64::from(i) * 0.001, 0.5, (i % 128) as u8)).collect();
    notes.push(Note { release_velocity: 0.0, channel: 9, ..Note::new(0.1, f64::NAN, 200, f32::NAN) });
    notes.push(Note { release_velocity: 0.0, ..Note::new(0.2, -1.0, 60, -3.0) });
    let devices = vec![
        device("fm-synth", &[("algorithm", 1e9), ("op1.level", f64::NAN), ("volume", f64::INFINITY)]),
        device("overdrive", &[("drive", -5.0)]),
        device("fm-synth", &[]),
    ];
    let out = render_offline(&project(notes, 1.0, devices), SR, 300).unwrap();
    assert!(out.iter().all(|s| s.is_finite() && s.abs() < 100.0));
}

/// A project with one empty track, for tests that only need its channel.
pub(crate) fn empty_project() -> Project {
    project(Vec::new(), 1.0, Vec::new())
}

#[path = "../../device-sdk/tests/common/mod.rs"]
mod sdk_examples;

fn sdk_device(name: &str) -> Device {
    let graph = sdk_examples::build(name).unwrap();
    let mut stored = device("sd", &[]);
    stored.kind = DeviceKind::Sd;
    stored.state = Some(graph.to_sd().unwrap().into_bytes());
    stored
}

#[test]
fn sdk_effects_run_in_the_engine_independently_of_host_blocks() {
    for name in ["stereo-utility", "auto-pan", "filter", "distortion", "delay", "compressor"] {
        let p = project(vec![note(0.01, 0.2, 69)], 0.5, vec![device("fm-synth", &[]), sdk_device(name)]);
        let reference = render_offline(&p, SR, 512).unwrap();
        let odd = render_offline(&p, SR, 31).unwrap();
        assert!(max_diff(&reference, &odd) < 1e-5, "{name}");
        assert!(reference.iter().all(|v| v.is_finite()));
        assert!(peak(&reference) > 1e-4, "{name} is silent");
    }
}

#[test]
fn sdk_delay_tail_and_oscillator_phase_carry_through_graph_swaps() {
    for name in ["auto-pan", "delay"] {
        let p = project(vec![note(0.0, 0.4, 69)], 2.0, vec![device("fm-synth", &[]), sdk_device(name)]);
        let mut edited = p.clone();
        edited.tracks[0].name = "Renamed".into();
        let mut renderer =
            Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING)
                .unwrap();
        let mut controller = renderer.controller();
        let mut out = blocks(&mut renderer, 40, 480);
        controller.update(&edited).unwrap();
        out.extend(blocks(&mut renderer, 80, 480));
        let mut untouched =
            Renderer::build(&p, RenderInputs::from_audio(&crate::AudioPool::new()), SR, RenderOptions::PLAYING)
                .unwrap();
        let reference = blocks(&mut untouched, 120, 480);
        assert!(max_diff(&out, &reference) < 1e-5, "{name}: {}", max_diff(&out, &reference));
    }
}
