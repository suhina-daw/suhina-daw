// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in parameters from the engine (#109) with a stand-in processor: set values, values set
//! while playing, and automation lanes, heard at their frames in offline renders and in real
//! time.

use crate::{RenderInputs, RenderOptions, render_offline_from};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use suhina_device_api::NoteEvent;
use suhina_model::*;

use crate::chain_tests::{BEAT, SR, blocks, device, note, project};
use crate::plugin_params::value_at;
use crate::{AudioPool, ParamEvent, PluginProcessor, PluginSlotInfo, Plugins, RAMP_STEP, Renderer, plugin_devices};

/// The parameter values a stand-in got: frame in the output, id, value.
type Seen = Arc<Mutex<Vec<(usize, u32, f64)>>>;

/// A stand-in plug-in that plays its parameter 0 as a constant on both sides, changing it at
/// the frame each value arrives: what it plays is what the engine automated, frame by frame.
struct Level {
    value: f32,
    pending: Vec<ParamEvent>,
    /// Every parameter value it got, at its frame in the output.
    seen: Seen,
    frames: usize,
}

impl Level {
    fn new() -> (Self, Seen) {
        let seen = Arc::default();
        (Self { value: 1.0, pending: Vec::new(), seen: Arc::clone(&seen), frames: 0 }, seen)
    }
}

impl PluginProcessor for Level {
    fn process(&mut self, buffer: &mut [f32], _events: &[(usize, NoteEvent)], _timeout: Duration) -> bool {
        if let Ok(mut seen) = self.seen.lock() {
            seen.extend(self.pending.iter().map(|e| (self.frames + e.frame, e.id, e.value)));
        }
        let mut next = self.pending.iter().peekable();
        for (frame, pair) in buffer.chunks_mut(2).enumerate() {
            while let Some(event) = next.next_if(|e| e.frame <= frame) {
                if event.id == 0 {
                    self.value = event.value as f32;
                }
            }
            pair.iter_mut().for_each(|s| *s = self.value);
        }
        self.frames += buffer.len() / 2;
        true
    }

    fn automate(&mut self, params: &[ParamEvent]) {
        self.pending.clear();
        self.pending.extend_from_slice(params);
    }
}

fn clap() -> DeviceKind {
    DeviceKind::Clap { id: "org.suhina.test.level".into() }
}

/// A track playing a note for `beats` beats, with the stand-in on it: `params` set, `lane`
/// automating parameter 0.
fn automated(beats: f64, params: &[(&str, f64)], lane: &[(f64, f64)]) -> Project {
    let mut plug = device("plug-in", params);
    plug.kind = clap();
    if !lane.is_empty() {
        plug.automation = vec![AutomationLane {
            param: "0".into(),
            points: lane.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect(),
            enabled: true,
        }];
    }
    project(vec![note(0.0, beats, 69)], beats, vec![plug])
}

fn with_level(project: &Project) -> (Plugins, Seen) {
    let (level, seen) = Level::new();
    let mut plugins = Plugins::new();
    if let Some(found) = plugin_devices(project).first() {
        plugins.insert(found.key, PluginSlotInfo { instrument: false, latency: 0 }, Box::new(level));
    }
    (plugins, seen)
}

#[test]
fn an_offline_render_plays_the_automation() {
    // Parameter 0 rises from 0 at beat 1 to 1 at beat 3, then jumps to 0.5 at beat 3.
    let lane = [(1.0, 0.0), (3.0, 1.0), (3.0, 0.5)];
    let project = automated(4.0, &[], &lane);
    let points: Vec<(u64, f64)> = lane.iter().map(|&(beat, v)| ((beat * BEAT as f64) as u64, v)).collect();
    // Steepest change in RAMP_STEP frames.
    let step = RAMP_STEP as f64 / (2.0 * BEAT as f64);
    for block in [512, 333, 1024] {
        let (plugins, seen) = with_level(&project);
        let out = render_offline_from(
            &project,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
            SR,
            block,
            None,
        )
        .unwrap();
        // The track's gains scale what the plug-in plays; a frame that played 0.5 tells by how
        // much.
        let unit = f64::from(out[(3 * BEAT + 100) * 2]) / 0.5;
        assert!(unit > 0.1, "{unit}");
        for frame in 0..4 * BEAT {
            let heard = f64::from(out[frame * 2]) / unit;
            let wanted = value_at(&points, frame as u64).unwrap_or(1.0);
            assert!((heard - wanted).abs() <= step + 1e-4, "block {block}, frame {frame}: {heard} for {wanted}");
        }
        // The values came at their frames: every point exactly, the ramp every RAMP_STEP.
        let seen = seen.lock().unwrap();
        assert!(seen.iter().any(|&(frame, id, v)| frame == BEAT * 3 && id == 0 && v == 0.5), "the jump at its frame");
        assert!(seen.iter().all(|&(frame, _, v)| (v - value_at(&points, frame as u64).unwrap_or(v)).abs() < 1e-9));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "in frame order");
    }
}

#[test]
fn set_values_go_at_the_start_and_a_lane_wins_while_it_plays() {
    // Parameter 0 set to 0.25, parameter 4 to 0.75: both at the first frame.
    let project = automated(1.0, &[("0", 0.25), ("4", 0.75)], &[]);
    let (plugins, seen) = with_level(&project);
    let out =
        render_offline_from(&project, RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins), SR, 512, None)
            .unwrap();
    assert!(out.iter().step_by(2).take(BEAT).all(|&s| (s - out[0]).abs() < 1e-7 && s > 0.0));
    let seen = seen.lock().unwrap().clone();
    assert_eq!(&seen[..2], [(0, 0, 0.25), (0, 4, 0.75)]);

    // With a lane on parameter 0, the lane's value follows the set one at the first frame.
    let project = automated(1.0, &[("0", 0.25)], &[(0.0, 0.5)]);
    let (plugins, seen) = with_level(&project);
    let _ =
        render_offline_from(&project, RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins), SR, 512, None)
            .unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(&seen[..2], [(0, 0, 0.25), (0, 0, 0.5)]);
}

#[test]
fn a_value_set_while_playing_is_heard_from_the_next_block() {
    let project = automated(4.0, &[], &[]);
    let (plugins, seen) = with_level(&project);
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let mut controller = renderer.controller();
    let mut out = blocks(&mut renderer, 10, 480);
    // Device 0, parameter id 0.
    controller.set_device_param(TrackId(1), 0, 0, 0.25).unwrap();
    out.extend(blocks(&mut renderer, 10, 480));
    let before = out[9 * 480 * 2];
    let after = out[10 * 480 * 2];
    assert!(before > 0.0 && (after / before - 0.25).abs() < 1e-6, "{before} {after}");
    assert_eq!(seen.lock().unwrap().as_slice(), [(4_800, 0, 0.25)]);
}

#[test]
fn an_edit_resends_only_the_values_that_changed() {
    let project = automated(4.0, &[("0", 0.5), ("1", 0.5)], &[]);
    let (plugins, seen) = with_level(&project);
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    let mut controller = renderer.controller();
    let _ = blocks(&mut renderer, 10, 480);
    let mut edited = project.clone();
    edited.channels[0].devices[0].params[1].value = 0.9;
    controller.update(&edited).unwrap();
    let _ = blocks(&mut renderer, 40, 480);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert_eq!((seen[2].1, seen[2].2), (1, 0.9));
}
