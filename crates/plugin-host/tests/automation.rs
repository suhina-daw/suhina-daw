// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in parameters from the engine (#109), with the plug-ins built into the host: an offline
//! render whose automation lane turns the test plug-ins' gain up (CLAP and VST3), and a session
//! that lists a running plug-in's parameters, sets one while it plays and automates it, by
//! command.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use suhina_engine::{RenderInputs, render_offline_from};

use serde_json::json;
use suhina_engine::{AudioPool, Renderer, render_offline};
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_session::{Player, PluginEnv, Session};

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;
const PATIENT: Duration = Duration::from_secs(20);

/// The host's test gains under their own names (`test:clap` has the id `clap`), as effects.
fn env() -> Arc<PluginEnv> {
    let mut catalog = Catalog::new();
    for name in ["clap", "vst3"] {
        let path = PathBuf::from(format!("test:{name}"));
        let info = PluginInfo {
            path: path.clone(),
            id: name.into(),
            name: format!("Test {name}"),
            vendor: "Suhina".into(),
            version: "1".into(),
            features: vec!["audio-effect".into()],
            replaces: Vec::new(),
            ara: false,
        };
        catalog.insert(path, vec![info]);
    }
    Arc::new(PluginEnv::new(catalog, PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))))
}

/// The test gain `id` with its gain (parameter 0) automated through `points` (beat, value).
fn gain(id: &str, points: &[(f64, f64)]) -> Device {
    Device {
        automation: vec![AutomationLane {
            param: "0".into(),
            points: points.iter().map(|&(beat, value)| AutomationPoint { beat, value, curve: 0.0 }).collect(),
            enabled: true,
        }],
        ..Device::new(format!("Test {id}"), DeviceKind::Clap { id: id.into() })
    }
}

/// One track playing a note for `length` beats through `devices`.
fn project(length: f64, devices: Vec<Device>) -> Project {
    let note = Note::new(0.0, length, 69, 1.0);
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
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note]) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, length)],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

/// The level of the left side of `out` over frames `from..to`.
fn rms(out: &[f32], from: usize, to: usize) -> f64 {
    let sum: f64 = out.chunks(2).skip(from).take(to - from).map(|f| f64::from(f[0]).powi(2)).sum();
    (sum / (to - from) as f64).sqrt()
}

#[test]
fn an_offline_render_plays_a_plug_ins_automation() {
    // The gain rises from 0 at beat 0 to 1 at beat 4, then jumps to 0.25 at beat 6.
    let points = [(0.0, 0.0), (4.0, 1.0), (6.0, 1.0), (6.0, 0.25)];
    let expected = |frame: usize| {
        let beat = frame as f64 / BEAT as f64;
        if beat < 4.0 {
            beat / 4.0
        } else if beat < 6.0 {
            1.0
        } else {
            0.25
        }
    };
    let dry = render_offline(&project(8.0, Vec::new()), SR, 512).unwrap();
    for id in ["clap", "vst3"] {
        let p = project(8.0, vec![gain(id, &points)]);
        let started = suhina_session::start_offline(&p, &AudioPool::new(), &env(), SR);
        assert_eq!(started.running(), 1, "{id}: {:?}", started.notices);
        let wet = render_offline_from(
            &p,
            RenderInputs::from_audio(&AudioPool::new()).with_plugins(started.plugins),
            SR,
            512,
            None,
        )
        .unwrap();
        // A tenth of a beat at a time, away from the jump: the level follows the lane.
        let window = BEAT / 10;
        for start in (0..8 * BEAT - window).step_by(window) {
            if (6 * BEAT - 2 * window..6 * BEAT + window).contains(&start) {
                continue;
            }
            let heard = rms(&wet, start, start + window) / rms(&dry, start, start + window);
            let wanted = expected(start + window / 2);
            assert!((heard - wanted).abs() < 0.03, "{id} at frame {start}: {heard} for {wanted}");
        }
    }
}

/// Plays `blocks` blocks of 480 frames.
fn play(renderer: &mut Renderer, blocks: usize) -> Vec<f32> {
    let mut out = Vec::new();
    let mut block = vec![0.0f32; 480 * 2];
    for _ in 0..blocks {
        renderer.process(&mut block);
        out.extend_from_slice(&block);
    }
    out
}

#[test]
fn a_session_sets_and_automates_a_running_plug_ins_parameter() {
    let mut device = gain("clap", &[]);
    device.automation.clear();
    let p = project(512.0, vec![device]);
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    renderer.set_offline(true);
    let mut session = Session::new(p);
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    session.execute("transport.play", &json!({})).unwrap();
    let device = json!({ "track": 1, "device": 0 });
    let deadline = Instant::now() + PATIENT;
    while session.execute("device.plugin", &device).unwrap()["status"] != "running" {
        session.sync_plugins();
        play(&mut renderer, 2);
        assert!(Instant::now() < deadline, "the plug-in did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    play(&mut renderer, 10);

    // Its parameters, with their ids and ranges.
    let listed = session.execute("device.pluginParams", &device).unwrap();
    assert_eq!(listed[0]["id"], 0);
    assert_eq!((listed[0]["min"].as_f64(), listed[0]["max"].as_f64()), (Some(0.0), Some(1.0)));

    // Set while it plays: heard from the next block, kept in the project, limited to the range.
    let level = |out: &[f32]| rms(out, 0, out.len() / 2);
    let before = level(&play(&mut renderer, 10));
    let set = session.execute("device.setParam", &json!({ "track": 1, "device": 0, "param": "0", "value": 0.25 }));
    assert_eq!(set.unwrap()["value"], 0.25);
    let after = level(&play(&mut renderer, 10));
    assert!((after / before - 0.5).abs() < 0.02, "{before} then {after}");
    assert_eq!(session.project().channels[0].devices[0].params, [DeviceParam::new("0", 0.25)]);
    let set = session.execute("device.setParam", &json!({ "track": 1, "device": 0, "param": "0", "value": 7.0 }));
    assert_eq!(set.unwrap()["value"], 1.0);
    let error = session.execute("device.setParam", &json!({ "track": 1, "device": 0, "param": "9", "value": 0.5 }));
    assert!(error.unwrap_err().message().to_owned().contains("no parameter 9"));
    let error = session.execute("device.setParam", &json!({ "track": 1, "device": 0, "param": "gain", "value": 0.5 }));
    assert!(error.unwrap_err().message().to_owned().contains("not a plug-in parameter id"));

    // A lane: the gain at 0 from here on, heard once the new graph plays.
    let automate = json!({ "track": 1, "device": 0, "param": "0", "points": [0, 0.0, 1000, 0.0] });
    assert_eq!(session.execute("device.setAutomation", &automate).unwrap()["points"], 2);
    play(&mut renderer, 100);
    assert!(level(&play(&mut renderer, 10)) < 1e-6);
    let listed = session.execute("track.devices", &json!({ "track": 1 })).unwrap();
    assert_eq!(listed[0]["automation"][0]["param"], 0);
    // Undone: the gain as set before comes back.
    session.execute("edit.undo", &json!({})).unwrap();
    play(&mut renderer, 100);
    assert!(level(&play(&mut renderer, 10)) > 0.01);
    for bad in [json!([0, 0.5, 1]), json!([-1, 0.5]), json!([0, "x"]), json!("0, 1")] {
        let error =
            session.execute("device.setAutomation", &json!({ "track": 1, "device": 0, "param": "0", "points": bad }));
        assert!(error.is_err(), "{bad}");
    }
}

/// The window has a shown plug-in list its parameters in the background for its generated
/// panel (#679): the report carries the list once it answers, and commands meanwhile never find
/// the plug-in busy.
#[test]
fn a_shown_plug_ins_parameters_are_listed_in_the_background_for_its_panel() {
    let mut device = gain("clap", &[]);
    device.automation.clear();
    let p = project(512.0, vec![device]);
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    renderer.set_offline(true);
    let mut session = Session::new(p);
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    session.execute("transport.play", &json!({})).unwrap();
    let device = json!({ "track": 1, "device": 0 });
    let deadline = Instant::now() + PATIENT;
    while session.execute("device.plugin", &device).unwrap()["status"] != "running" {
        session.sync_plugins();
        play(&mut renderer, 2);
        assert!(Instant::now() < deadline, "the plug-in did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    let track = suhina_model::TrackId(1);
    assert_eq!(session.plugin_report_at(track, &[0]).unwrap().params, None, "nothing listed yet");
    session.list_plugin_params_at(track, &[0]);
    // While it lists, commands still answer.
    assert!(
        session.execute("device.setParam", &json!({ "track": 1, "device": 0, "param": "0", "value": 0.5 })).is_ok()
    );
    let listed = loop {
        session.sync_plugins();
        play(&mut renderer, 2);
        if let Some(list) = session.plugin_report_at(track, &[0]).unwrap().params {
            break list;
        }
        assert!(Instant::now() < deadline, "the list did not come");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!((listed[0].id, listed[0].min, listed[0].max), (0, 0.0, 1.0));
    assert!(session.execute("device.pluginParams", &device).is_ok());
    // Asked again at once, it is not listed again: the list is fresh for a couple of seconds.
    session.list_plugin_params_at(track, &[0]);
    assert!(session.execute("device.pluginParams", &device).is_ok());
    // A restart forgets the old process's list: the panel waits for the new one's. Rendered in
    // real time from here, a block does not wait the offline timeout on the stopped process, and
    // the new one has its own deadline.
    renderer.set_offline(false);
    session.execute("device.restart", &device).unwrap();
    assert_eq!(session.plugin_report_at(track, &[0]).unwrap().params, None, "a restarting plug-in lists nothing");
    let deadline = Instant::now() + PATIENT;
    while session.execute("device.plugin", &device).unwrap()["status"] != "running" {
        session.sync_plugins();
        play(&mut renderer, 2);
        assert!(Instant::now() < deadline, "the plug-in did not start again");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(session.plugin_report_at(track, &[0]).unwrap().params, None, "the old list is gone");
}
