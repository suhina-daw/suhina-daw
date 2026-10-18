// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cue: `track.setCue` (a project edit, undone like one), and `output.setCuePair` and
//! `output.setCueVolume` with a stand-in eight-channel audio device. How the cue sounds is
//! tested in the engine; where its samples go on a device, in `crates/output`.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::Project;
use suhina_session::audio::{DeviceInfo, HostInfo, Opened, cue_channels};
use suhina_session::{AudioSystem, LiveOutput, OutputChoice, OutputInfo, Player, Projects, Session};

const SR: u32 = 48_000;

/// An eight-channel interface: it opens players on renderers it keeps and records each choice.
#[derive(Default)]
struct Interface {
    renderers: Mutex<Vec<Renderer>>,
    opened: Mutex<Vec<OutputChoice>>,
}

struct Live(OutputChoice);

impl LiveOutput for Live {
    fn info(&self) -> OutputInfo {
        OutputInfo {
            host: "Stand".into(),
            device: "Interface".into(),
            sample_rate: SR,
            engine_block: 512,
            channels: 8,
            cue_pair: self.0.cue_pair,
            ..OutputInfo::default()
        }
    }
}

impl AudioSystem for Interface {
    fn hosts(&self) -> Vec<HostInfo> {
        vec![HostInfo {
            name: "Stand".into(),
            default: true,
            devices: vec![DeviceInfo {
                name: "Interface".into(),
                default: true,
                channels: 8,
                sample_rates: vec![SR],
                default_sample_rate: Some(SR),
                buffer_sizes: vec![512],
                buffer_range: None,
                exclusive: false,
            }],
            inputs: Vec::new(),
            error: None,
        }]
    }

    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        _assets: &suhina_engine::DeviceAssets,
        choice: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String> {
        self.opened.lock().unwrap().push(choice.clone());
        drop(previous);
        let mut renderer = Renderer::stopped(project, audio, SR).unwrap();
        let player = Player::with_output(renderer.controller(), None, Arc::new(Live(choice.clone())));
        self.renderers.lock().unwrap().push(renderer);
        Ok(Opened { player, notice: None, plugins_kept: true })
    }
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Result<Value, String> {
    projects.execute(id, &params).map_err(|e| e.0)
}

fn playing() -> (Projects, Arc<Interface>) {
    let interface = Arc::new(Interface::default());
    let mut projects = Projects::new();
    projects.set_audio_system(interface.clone());
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    projects.open_output(None).unwrap();
    (projects, interface)
}

#[test]
fn a_track_cue_is_an_edit_that_undoes() {
    let mut session = Session::new(suhina_session::blank_project());
    let track = session.project().tracks[0].id.0;
    let cued = |s: &Session| s.project().channels.iter().any(|c| c.cue);
    session.execute("track.setCue", &json!({ "track": track, "cue": true })).unwrap();
    assert!(cued(&session));
    session.execute("edit.undo", &json!({})).unwrap();
    assert!(!cued(&session));
    session.execute("edit.redo", &json!({})).unwrap();
    assert!(cued(&session));
    assert!(session.execute("track.setCue", &json!({ "track": track, "cue": "yes" })).is_err());
    assert!(session.execute("track.setCue", &json!({ "track": 999, "cue": true })).is_err());
}

#[test]
fn the_cue_pair_reopens_the_output_on_the_pair_and_refuses_one_the_device_lacks() {
    let (mut projects, interface) = playing();
    let result = run(&mut projects, "output.setCuePair", json!({ "pair": 2 })).unwrap();
    assert_eq!(result["cue_pair"], json!(2));
    assert_eq!(result["channels"], json!(8));
    assert_eq!(result["chosen"]["cue_pair"], json!(2));
    assert_eq!(interface.opened.lock().unwrap().last().unwrap().cue_pair, Some(2));
    assert_eq!(cue_channels(2), (5, 6));

    // Eight channels hold pairs 1 to 3; the choice stays as it was.
    let err = run(&mut projects, "output.setCuePair", json!({ "pair": 4 })).unwrap_err();
    assert!(err.contains("pairs 1 to 3"), "{err}");
    assert_eq!(projects.output_choice().cue_pair, Some(2));
    for bad in [json!({ "pair": -1 }), json!({ "pair": 32 }), json!({ "pair": "3/4" }), json!({})] {
        assert!(run(&mut projects, "output.setCuePair", bad.clone()).is_err(), "{bad}");
    }

    // Pair 0: back with the main output.
    let back = run(&mut projects, "output.setCuePair", json!({ "pair": 0 })).unwrap();
    assert_eq!(back["cue_pair"], Value::Null);
    assert_eq!(projects.output_choice().cue_pair, None);
    assert_eq!(interface.opened.lock().unwrap().len(), 3, "each new pair opened the output again");
}

#[test]
fn the_cue_volume_changes_without_reopening_the_output() {
    let (mut projects, interface) = playing();
    let result = run(&mut projects, "output.setCueVolume", json!({ "db": -6 })).unwrap();
    assert_eq!(result["cue_db"], json!(-6.0));
    assert!((projects.output_choice().cue_gain - 0.501_187).abs() < 1e-5);
    assert_eq!(interface.opened.lock().unwrap().len(), 1);

    let silent = run(&mut projects, "output.setCueVolume", json!({ "db": "-inf" })).unwrap();
    assert_eq!(silent["cue_db"], Value::Null);
    assert_eq!(projects.output_choice().cue_gain, 0.0);
    for bad in [json!({ "db": 13 }), json!({ "db": "loud" }), json!({})] {
        assert!(run(&mut projects, "output.setCueVolume", bad.clone()).is_err(), "{bad}");
    }
    // The volume goes with the next output that opens.
    run(&mut projects, "output.setCueVolume", json!({ "db": 0 })).unwrap();
    run(&mut projects, "output.setCuePair", json!({ "pair": 1 })).unwrap();
    assert_eq!(interface.opened.lock().unwrap().last().unwrap().cue_gain, 1.0);
}

#[test]
fn without_a_playing_output_the_cue_is_only_chosen() {
    let mut projects = Projects::new();
    // No device to check the pair against: it is chosen, for the output that opens next.
    let chosen = run(&mut projects, "output.setCuePair", json!({ "pair": 1 })).unwrap();
    assert_eq!((chosen["playing"].clone(), chosen["chosen"]["cue_pair"].clone()), (json!(false), json!(1)));
    run(&mut projects, "output.setCueVolume", json!({ "db": 6 })).unwrap();
    assert!((projects.output_choice().cue_gain - 1.995_262).abs() < 1e-5);
}
