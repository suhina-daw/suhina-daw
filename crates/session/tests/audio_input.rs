// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A track's audio input (ADR 0017) through the session: `track.setAudioInput` as an edit, and
//! an input device's frames heard through the armed track with a stand-in output.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::Project;
use suhina_session::audio::Opened;
use suhina_session::{AudioSystem, OutputChoice, Player, Projects, Session};

const SR: u32 = 48_000;
const BLOCK: usize = 256;

/// Opens players on renderers it keeps, each reading a two-channel input from a ring the test
/// writes to.
#[derive(Default)]
struct Stand {
    renderers: Mutex<Vec<(Renderer, rtrb::Producer<f32>)>>,
}

impl AudioSystem for Stand {
    fn hosts(&self) -> Vec<suhina_session::audio::HostInfo> {
        Vec::new()
    }

    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        _assets: &suhina_engine::DeviceAssets,
        _: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String> {
        drop(previous);
        let mut renderer = Renderer::stopped(project, audio, SR).unwrap();
        let (producer, consumer) = rtrb::RingBuffer::new(BLOCK * 2 * 64);
        renderer.set_audio_input(consumer, 2, Arc::new(AtomicU64::new(0)));
        let player = Player::new(renderer.controller(), None);
        self.renderers.lock().unwrap().push((renderer, producer));
        Ok(Opened { player, notice: None, plugins_kept: true })
    }
}

impl Stand {
    /// Feeds 0.5 on input channel 1 and runs the newest renderer for a few blocks, returning
    /// the loudest sample of the last one.
    fn run_with_input(&self) -> f32 {
        let mut renderers = self.renderers.lock().unwrap();
        let (renderer, ring) = renderers.last_mut().unwrap();
        let mut block = vec![0.0f32; BLOCK * 2];
        for _ in 0..4 {
            for _ in 0..BLOCK {
                let _ = ring.push(0.5);
                let _ = ring.push(0.0);
            }
            renderer.process(&mut block);
        }
        block.iter().fold(0.0, |m, s| m.max(s.abs()))
    }
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Result<Value, String> {
    projects.execute(id, &params).map_err(|e| e.0)
}

fn track_ids(projects: &mut Projects) -> Vec<u64> {
    let tracks = run(projects, "project.tracks", json!({})).unwrap();
    tracks.as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect()
}

#[test]
fn a_tracks_audio_input_is_an_edit_that_undo_takes_back() {
    let mut projects = Projects::new();
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let ids = track_ids(&mut projects);
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[2]["audio_input"], json!({ "input": "none", "monitor": "auto" }));

    let set = run(&mut projects, "track.setAudioInput", json!({ "track": ids[2], "input": "3" })).unwrap();
    assert_eq!(set["audio_input"], json!({ "input": "3", "monitor": "auto" }));
    run(&mut projects, "track.setAudioInput", json!({ "track": ids[3], "input": "1/2", "monitor": "in" })).unwrap();
    run(&mut projects, "track.setAudioInput", json!({ "track": ids[3], "monitor": "off" })).unwrap();
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[3]["audio_input"], json!({ "input": "1/2", "monitor": "off" }), "a part left out stays");

    run(&mut projects, "edit.undo", json!({})).unwrap();
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[3]["audio_input"]["monitor"], json!("in"));

    for bad in [
        json!({ "track": ids[2], "input": "0" }),
        json!({ "track": ids[2], "input": "257" }),
        json!({ "track": ids[2], "input": "1/3" }),
        json!({ "track": ids[2], "input": "2/1" }),
        json!({ "track": ids[2], "input": "left" }),
        json!({ "track": ids[2], "input": 1 }),
        json!({ "track": ids[2], "monitor": "loud" }),
        json!({ "track": 999, "input": "1" }),
    ] {
        assert!(run(&mut projects, "track.setAudioInput", bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn an_armed_track_hears_its_input_and_monitoring_follows_the_setting() {
    let stand = Arc::new(Stand::default());
    let mut projects = Projects::new();
    projects.set_audio_system(stand.clone());
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let ids = track_ids(&mut projects);
    let audio_track = ids[2];
    run(&mut projects, "track.setAudioInput", json!({ "track": audio_track, "input": "1" })).unwrap();
    projects.open_output(None).unwrap();
    assert_eq!(stand.run_with_input(), 0.0, "Auto, not armed");

    run(&mut projects, "track.setArm", json!({ "track": audio_track, "armed": true })).unwrap();
    assert!(stand.run_with_input() > 0.1, "Auto, armed: the microphone is heard");

    run(&mut projects, "track.setAudioInput", json!({ "track": audio_track, "monitor": "off" })).unwrap();
    assert_eq!(stand.run_with_input(), 0.0, "Off: not heard, even armed");

    run(&mut projects, "track.setArm", json!({ "track": audio_track, "armed": false })).unwrap();
    run(&mut projects, "track.setAudioInput", json!({ "track": audio_track, "monitor": "in" })).unwrap();
    assert!(stand.run_with_input() > 0.1, "In: heard without arming");

    // Input 2 is silent in this test: a track on it hears nothing.
    run(&mut projects, "track.setAudioInput", json!({ "track": audio_track, "input": "2" })).unwrap();
    assert_eq!(stand.run_with_input(), 0.0);
}
