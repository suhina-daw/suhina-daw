// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{AudioPool, LaunchCommand, Renderer};
use suhina_model::*;

const SR: u32 = 48_000;
/// 120 bpm at 48 kHz: frames to a beat.
const BEAT: usize = 24_000;

#[test]
fn every_recorded_click_decodes() {
    for (index, bytes) in RECORDED.iter().enumerate() {
        let clip = decode_wav(bytes).unwrap_or_else(|| panic!("click {index} decodes"));
        assert_eq!(clip.sample_rate, 44_100.0, "click {index}");
        assert!(clip.samples.len() > 1_000, "click {index}: {} samples", clip.samples.len());
        let peak = clip.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.3 && peak <= 1.0, "click {index}: peak {peak}");
    }
}

#[test]
fn a_malformed_wav_is_none_and_never_panics() {
    let good = RECORDED[0];
    assert!(decode_wav(&[]).is_none());
    assert!(decode_wav(b"RIFF").is_none());
    assert!(decode_wav(b"RIFX\0\0\0\0WAVE").is_none());
    // Cut anywhere: never a panic.
    for end in 0..64.min(good.len()) {
        let _ = decode_wav(&good[..end]);
    }
    // A data chunk before any format.
    let mut no_format = b"RIFF\0\0\0\0WAVEdata\x04\0\0\0\0\0\0\0".to_vec();
    assert!(decode_wav(&no_format).is_none());
    // A chunk claiming more than the file holds is cut to it.
    no_format.truncate(12);
    no_format.extend_from_slice(b"fmt \x10\0\0\0\x03\0\x01\0\x44\xac\0\0\0\0\0\0\x04\0\x20\0data\xff\xff\xff\xff");
    no_format.extend_from_slice(&0.5f32.to_le_bytes());
    let cut = decode_wav(&no_format).expect("a short data chunk reads what is there");
    assert_eq!(cut.samples, [0.5]);
    // 24-bit samples are not read.
    let mut wide = no_format.clone();
    wide[34] = 24;
    assert!(decode_wav(&wide).is_none());
}

#[test]
fn names_round_trip() {
    for sound in ClickSound::ALL {
        assert_eq!(ClickSound::from_name(sound.name()), Some(sound));
    }
    for route in ClickRoute::ALL {
        assert_eq!(ClickRoute::from_name(route.name()), Some(route));
    }
    assert_eq!(ClickSound::from_name("cowbell"), None);
    assert_eq!(ClickRoute::from_name("pre"), None);
}

/// Frames where sound starts after more than 100 silent frames.
fn onsets(out: &[f32]) -> Vec<usize> {
    let mut result = Vec::new();
    let mut silent_run = usize::MAX;
    for (i, frame) in out.as_chunks::<2>().0.iter().enumerate() {
        let loud = frame.iter().any(|s| s.abs() > 1e-6);
        if loud && silent_run > 100 {
            result.push(i);
        }
        silent_run = if loud { 0 } else { silent_run.saturating_add(1) };
    }
    result
}

fn peak(out: &[f32], from: usize, to: usize) -> f32 {
    out[from * 2..to * 2].iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// The frames from `from` to the click's last sound within half a beat.
fn length(out: &[f32], from: usize) -> usize {
    out[from * 2..(from + BEAT / 2) * 2].as_chunks::<2>().0.iter().rposition(|f| f[0].abs() > 1e-6).unwrap_or(0)
}

/// `blocks` blocks of `frames` frames, the main output and the cue output.
fn play(renderer: &mut Renderer, blocks: usize, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut out, mut cue) = (Vec::new(), Vec::new());
    for _ in 0..blocks {
        let mut block = vec![0.0f32; frames * 2];
        let mut cue_block = vec![0.0f32; frames * 2];
        renderer.process_with_cue(&mut block, &mut cue_block);
        out.extend(block);
        cue.extend(cue_block);
    }
    (out, cue)
}

#[test]
fn a_recorded_click_sounds_on_every_beat_and_shorter_and_higher_on_the_bar() {
    for sound in [ClickSound::Soft, ClickSound::Muted, ClickSound::Deep, ClickSound::Tick, ClickSound::Knock] {
        let mut project = Project::default();
        project.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
        let mut renderer =
            Renderer::stopped(&project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
        renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(1.0) });
        renderer.handle(LaunchCommand::SetMetronomeSound(sound));
        renderer.handle(LaunchCommand::Play);
        let (out, _) = play(&mut renderer, 200, 480);
        let found = onsets(&out);
        assert_eq!(found.len(), 4, "{sound:?}: {found:?}");
        for (got, beat) in found.iter().zip([0, BEAT, 2 * BEAT, 3 * BEAT]) {
            // A low passed click rises slowly: its first audible frame comes a few frames in.
            assert!(got.abs_diff(beat) <= 4, "{sound:?}: {found:?}");
        }
        assert!(peak(&out, 0, 4_000) > 0.2, "{sound:?} is heard");
        // Played faster on the bar: shorter.
        let (bar, beat) = (length(&out, 0), length(&out, BEAT));
        assert!(bar > 0 && (bar as f64) < beat as f64 * 0.8, "{sound:?}: bar {bar}, beat {beat} frames");
    }
}

/// A project with a main channel at `volume`, muted when `mute`, holding the built-in devices
/// `devices` (a Utility's gain is set to half by [`click`]).
fn with_main(volume: f64, mute: bool, devices: &[&str]) -> Project {
    let mut project = Project::default();
    project.channels.push(Channel {
        id: ChannelId(9),
        name: "Main".into(),
        role: ChannelRole::Master,
        volume,
        pan: 0.0,
        mute,
        solo: false,
        cue: false,
        output: None,
        sends: Vec::new(),
        devices: devices
            .iter()
            .map(|id| Device {
                instrument: false,
                inputs: Vec::new(),
                id: Default::default(),
                name: (*id).into(),
                kind: DeviceKind::Builtin((*id).into()),
                enabled: true,
                state: None,
                params: Vec::new(),
                automation: Vec::new(),
                clip_host: false,
                extensions: Extensions::default(),
            })
            .collect(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    });
    project
}

/// The first click's peak on the main output and on the cue output, and the first onset on the
/// main output, with the click at `route`.
fn click(project: &Project, route: ClickRoute, separate_cue: bool) -> (f32, f32, Option<usize>) {
    let mut renderer = Renderer::stopped(project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
    if project.main_channel().is_some_and(|m| m.devices.iter().any(|d| d.name == "utility")) {
        // The Utility's gain to half (-6 dB), heard from the next block.
        renderer.controller().set_device_param(TrackId::MAIN, 0, 0, -6.0206).unwrap();
        play(&mut renderer, 1, 480);
    }
    renderer.handle(LaunchCommand::SetCue { gain: Some(1.0), separate: Some(separate_cue) });
    renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
    renderer.handle(LaunchCommand::SetClickRoute(route));
    renderer.handle(LaunchCommand::Play);
    let (out, cue) = play(&mut renderer, 20, 480);
    (peak(&out, 0, 9_600), peak(&cue, 0, 9_600), onsets(&out).first().copied())
}

#[test]
fn before_the_fader_the_main_channel_leaves_the_click_alone() {
    let (alone, _, _) = click(&Project::default(), ClickRoute::PreFader, false);
    assert!(alone > 0.4, "{alone}");
    for project in
        [with_main(0.25, false, &["utility"]), with_main(0.25, true, &[]), with_main(0.25, true, &["utility"])]
    {
        let (heard, cue, _) = click(&project, ClickRoute::PreFader, false);
        assert!((heard - alone).abs() < 1e-4, "{heard} against {alone}");
        assert_eq!(cue, 0.0, "not on a cue of its own");
    }
}

#[test]
fn after_the_fader_the_click_follows_the_main_volume_and_mute_but_not_its_devices() {
    let (alone, _, _) = click(&Project::default(), ClickRoute::PostFader, false);
    for devices in [&[][..], &["utility"][..]] {
        let (heard, _, _) = click(&with_main(0.25, false, devices), ClickRoute::PostFader, false);
        assert!((heard / alone - 0.25).abs() < 0.01, "{devices:?}: {heard} against {alone}");
        let (muted, _, _) = click(&with_main(0.25, true, devices), ClickRoute::PostFader, false);
        assert!(muted < 1e-6, "{devices:?}: muted {muted}");
    }
}

#[test]
fn through_the_main_channel_its_devices_apply_too() {
    let (alone, _, _) = click(&Project::default(), ClickRoute::ThroughMain, false);
    let (heard, _, _) = click(&with_main(0.25, false, &["utility"]), ClickRoute::ThroughMain, false);
    assert!((heard / alone - 0.125).abs() < 0.01, "{heard} against {alone}");
    // Without devices, as after the fader.
    let (heard, _, _) = click(&with_main(0.25, false, &[]), ClickRoute::ThroughMain, false);
    assert!((heard / alone - 0.25).abs() < 0.01, "{heard} against {alone}");
    let (muted, _, _) = click(&with_main(0.25, true, &["utility"]), ClickRoute::ThroughMain, false);
    assert!(muted < 1e-6, "{muted}");
}

#[test]
fn to_the_cue_the_click_leaves_the_main_output_when_the_cue_has_outputs_of_its_own() {
    let project = with_main(0.25, true, &["utility"]);
    let (main, cue, _) = click(&project, ClickRoute::Cue, true);
    assert_eq!(main, 0.0);
    assert!(cue > 0.4, "{cue}");
    // A cue playing with the main mix: the click is in the main output, before the fader.
    let (main, cue, _) = click(&project, ClickRoute::Cue, false);
    assert!(main > 0.4, "{main}");
    let _ = cue;
}

#[test]
fn the_click_is_as_late_as_the_main_channel_makes_the_mix() {
    // A limiter on Main delays what goes through it by its lookahead; the click joining after
    // it is delayed as much, so it lands where it would through Main.
    let project = with_main(1.0, false, &["limiter"]);
    let through = click(&project, ClickRoute::ThroughMain, false).2.expect("a click through Main");
    assert!(through > 10, "the limiter delays it: {through}");
    for route in [ClickRoute::PreFader, ClickRoute::PostFader] {
        let at = click(&project, route, false).2.expect("a click");
        assert!(at.abs_diff(through) <= 2, "{route:?}: {at} against {through}");
    }
}

/// The first onset on the cue output, its cue a pair of outputs of its own and the click routed
/// there when `metronome`.
fn first_on_the_cue(project: &Project, metronome: bool) -> Option<usize> {
    let mut renderer = Renderer::stopped(project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
    renderer.handle(LaunchCommand::SetCue { gain: Some(1.0), separate: Some(true) });
    renderer.handle(LaunchCommand::SetMetronome { on: metronome, gain: Some(0.5) });
    renderer.handle(LaunchCommand::SetClickRoute(ClickRoute::Cue));
    renderer.handle(LaunchCommand::Play);
    // The first onsets are all it needs: a few hundred frames, past the group's lookahead.
    let (_, cue) = play(&mut renderer, 4, 480);
    onsets(&cue).first().copied()
}

#[test]
fn on_the_cue_the_click_lines_up_with_a_cued_track_whatever_its_group_does() {
    // Track 1 plays into a group with two limiters (lookahead) on it, its cue on. The cue takes
    // the track where it enters the group, so the group's latency is not in it, nor in the click.
    let limiter = crate::chain_tests::device("limiter", &[]);
    let mut project = crate::buses_tests::grouped(vec![limiter.clone(), limiter], false);
    project.channels[0].cue = true;
    let latency = Renderer::new(&project, SR, suhina_device_registry::factory()).unwrap().latency();
    assert!(latency > 10, "the group delays the mix: {latency}");
    let track = first_on_the_cue(&project, false).expect("the cued track on the cue");
    let mut silent = project.clone();
    silent.arrangement.lanes.clear();
    let at = first_on_the_cue(&silent, true).expect("the click on the cue");
    assert!(at.abs_diff(track) <= 4, "click at {at}, the cued track at {track}");
}

#[test]
fn a_click_left_in_a_delay_never_plays_when_the_route_comes_back() {
    // A limiter on Main: the click past it waits in a delay as long as its lookahead.
    let project = with_main(1.0, false, &["limiter"]);
    for (away, separate) in [(ClickRoute::ThroughMain, false), (ClickRoute::Cue, true)] {
        let mut renderer =
            Renderer::stopped(&project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
        renderer.handle(LaunchCommand::SetCue { gain: Some(1.0), separate: Some(separate) });
        renderer.handle(LaunchCommand::SetMetronome { on: true, gain: Some(0.5) });
        renderer.handle(LaunchCommand::Play);
        let (out, _) = play(&mut renderer, 1, 8);
        assert_eq!(peak(&out, 0, 8), 0.0, "{away:?}: the click is still in the delay");
        // Away and off: the delay it was in does not run.
        renderer.handle(LaunchCommand::SetClickRoute(away));
        renderer.handle(LaunchCommand::SetMetronome { on: false, gain: None });
        play(&mut renderer, 1, 512);
        renderer.handle(LaunchCommand::SetClickRoute(ClickRoute::PreFader));
        let (out, cue) = play(&mut renderer, 1, 512);
        assert_eq!(peak(&out, 0, 512), 0.0, "{away:?}: no old click on the main output");
        assert_eq!(peak(&cue, 0, 512), 0.0, "{away:?}: none on the cue");
    }
}
