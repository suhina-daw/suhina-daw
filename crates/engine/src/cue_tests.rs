// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cue bus: cued tracks and the preview heard apart from the main mix, which does not
//! change.

use crate::{RenderInputs, RenderOptions};
use std::sync::Arc;

use suhina_model::*;

use crate::chain_tests::{SR, device, max_diff, note, peak, project};
use crate::{
    AudioData, AudioPool, DeviceAssets, LaunchCommand, Plugins, Renderer, render_offline, render_offline_with_cue,
};

fn render_both(project: &Project, gain: f32, separate: bool) -> (Vec<f32>, Vec<f32>) {
    render_offline_with_cue(
        project,
        RenderInputs::from_audio(&AudioPool::new()).with_assets(&DeviceAssets::new()).with_plugins(Plugins::new()),
        SR,
        512,
        gain,
        separate,
    )
    .unwrap()
}

fn cued(mut project: Project) -> Project {
    for channel in &mut project.channels {
        channel.cue = true;
    }
    project
}

fn master(volume: f64) -> Channel {
    Channel { role: ChannelRole::Master, volume, ..Channel::new(ChannelId(99), "Main") }
}

#[test]
fn a_cued_track_is_heard_on_the_cue_and_the_main_mix_does_not_change() {
    let plain = project(vec![note(0.0, 1.0, 69)], 1.0, vec![]);
    let alone = render_offline(&plain, SR, 512).unwrap();
    let (main, cue) = render_both(&cued(plain.clone()), 1.0, true);
    // A cued track without devices goes through a passthrough chain: the same sound, no
    // longer (a passthrough adds no tail).
    assert_eq!(main.len(), alone.len());
    assert!(max_diff(&main, &alone) < 1e-6, "{}", max_diff(&main, &alone));
    assert!(peak(&cue) > 0.1);
    assert!(max_diff(&cue, &main) < 1e-6, "with no main volume the cue is the track as the mix has it");

    // A track whose cue is off is not on the cue.
    let (_, silent) = render_both(&plain, 1.0, true);
    assert_eq!(peak(&silent), 0.0);
}

#[test]
fn a_cued_track_with_devices_is_heard_after_them() {
    let effect = project(vec![note(0.0, 1.0, 69)], 1.0, vec![device("utility", &[("gain", -6.0206)])]);
    let alone = render_offline(&effect, SR, 512).unwrap();
    let (main, cue) = render_both(&cued(effect), 1.0, true);
    assert!(max_diff(&main, &alone) < 1e-6);
    assert!(max_diff(&cue, &main) < 1e-6);
}

#[test]
fn the_cue_volume_scales_the_cue_and_the_main_volume_does_not() {
    let mut quiet = cued(project(vec![note(0.0, 1.0, 69)], 1.0, vec![]));
    quiet.channels.push(master(0.5));
    let (main, cue) = render_both(&quiet, 1.0, true);
    // The main channel halves the mix, not the cue.
    assert!((peak(&main) * 2.0 - peak(&cue)).abs() < 1e-4, "{} {}", peak(&main), peak(&cue));
    let (_, half) = render_both(&quiet, 0.5, true);
    assert!(max_diff(&half.iter().map(|s| s * 2.0).collect::<Vec<_>>(), &cue) < 1e-5);
    // The track's own volume applies to the cue.
    let mut softer = quiet.clone();
    if let Some(channel) = softer.channels.first_mut() {
        channel.volume = 0.25;
    }
    let (_, soft) = render_both(&softer, 1.0, true);
    assert!((peak(&soft) * 4.0 - peak(&cue)).abs() < 1e-4);
}

#[test]
fn a_muted_cued_track_is_silent_on_the_cue_too() {
    let mut muted = cued(project(vec![note(0.0, 1.0, 69)], 1.0, vec![]));
    if let Some(channel) = muted.channels.first_mut() {
        channel.mute = true;
    }
    let (main, cue) = render_both(&muted, 1.0, true);
    assert_eq!((peak(&main), peak(&cue)), (0.0, 0.0));
}

/// A tenth of a second of a steady 0.5 at the engine's rate.
fn steady() -> Arc<AudioData> {
    Arc::new(AudioData { sample_rate: SR, channels: 2, samples: vec![0.5; SR as usize / 10 * 2] })
}

fn preview_both(separate: bool, gain: f32) -> (Vec<f32>, Vec<f32>) {
    let mut renderer = Renderer::stopped(&crate::chain_tests::empty_project(), &AudioPool::new(), SR).unwrap();
    let mut controller = renderer.controller();
    controller.send(LaunchCommand::SetCue { gain: Some(gain), separate: Some(separate) }).unwrap();
    controller.preview_sample(steady(), 0.0, 1.0).unwrap();
    let (mut main, mut cue) = (vec![0.0f32; 2048], vec![0.0f32; 2048]);
    renderer.process_with_cue(&mut main, &mut cue);
    (main, cue)
}

#[test]
fn the_preview_plays_on_the_cue_and_on_the_main_output_only_while_the_cue_plays_with_it() {
    let (main, cue) = preview_both(true, 1.0);
    assert_eq!(peak(&main), 0.0, "a cue pair of its own: the main output stays silent");
    assert!((cue[1000] - 0.5).abs() < 1e-6, "{}", cue[1000]);

    // With the main output, as before the cue existed: at the cue volume, and on the cue too.
    let (main, cue) = preview_both(false, 0.5);
    assert!((main[1000] - 0.25).abs() < 1e-6, "{}", main[1000]);
    assert!((cue[1000] - 0.25).abs() < 1e-6, "{}", cue[1000]);
}

#[test]
fn process_with_cue_matches_process_and_fills_a_short_cue_buffer_only_where_it_has_room() {
    let song = cued(project(vec![note(0.0, 1.0, 69)], 1.0, vec![]));
    let mut a =
        Renderer::build(&song, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut b =
        Renderer::build(&song, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut plain = vec![0.0f32; 4096];
    a.process(&mut plain);
    let (mut main, mut cue) = (vec![0.0f32; 4096], vec![9.0f32; 1000]);
    b.process_with_cue(&mut main, &mut cue);
    assert_eq!(plain, main);
    assert!(cue.iter().all(|s| s.abs() < 1.0), "every sample it had room for was written");
}

#[test]
fn the_cue_volume_is_kept_in_range() {
    let song = cued(project(vec![note(0.0, 1.0, 69)], 1.0, vec![]));
    let (_, unity) = render_both(&song, 1.0, true);
    let (_, loud) = render_both(&song, 100.0, true);
    let most = crate::MAX_CUE_GAIN;
    assert!((peak(&loud) - peak(&unity) * most).abs() < 1e-3);
    let (_, nothing) = render_both(&song, f32::NAN, true);
    assert_eq!(peak(&nothing), 0.0);
}
