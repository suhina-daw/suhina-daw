// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI inputs and arming (ADR 0017) with stand-in MIDI ports and audio output: ports opened
//! and closed as they are turned on and off, `track.setArm`, and a note from a port heard
//! through the armed track's instrument, also after the output is opened again. Real ports are
//! tested in `crates/midi`.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use suhina_engine::{AudioPool, LiveEvent, Renderer};
use suhina_model::Project;
use suhina_session::audio::Opened;
use suhina_session::{
    AudioSystem, CommandKind, LiveInput, MidiSystem, OutputChoice, Player, Projects, Session, midi_commands,
};

const SR: u32 = 48_000;

/// Ports by name; opening one records it and keeps its input, dropping what it returned
/// records the close.
#[derive(Default)]
struct Ports {
    ports: Mutex<Vec<String>>,
    /// A port that cannot be opened.
    broken: Option<String>,
    opened: Mutex<Vec<String>>,
    closed: Arc<Mutex<Vec<String>>>,
    inputs: Mutex<Vec<Arc<LiveInput>>>,
}

struct Open(String, Arc<Mutex<Vec<String>>>);

impl Drop for Open {
    fn drop(&mut self) {
        self.1.lock().unwrap().push(self.0.clone());
    }
}

impl MidiSystem for Ports {
    fn ports(&self) -> Result<Vec<String>, String> {
        Ok(self.ports.lock().unwrap().clone())
    }

    fn open(&self, name: &str, input: Arc<LiveInput>) -> Result<Box<dyn std::any::Any + Send>, String> {
        if self.broken.as_deref() == Some(name) {
            return Err("taken by another program".into());
        }
        self.opened.lock().unwrap().push(name.to_string());
        self.inputs.lock().unwrap().push(input);
        Ok(Box::new(Open(name.to_string(), Arc::clone(&self.closed))))
    }
}

/// Opens players on renderers it keeps, so a test can run them.
#[derive(Default)]
struct Stand {
    renderers: Mutex<Vec<Renderer>>,
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
        let player = Player::new(renderer.controller(), None);
        self.renderers.lock().unwrap().push(renderer);
        Ok(Opened { player, notice: None, plugins_kept: true })
    }
}

impl Stand {
    /// Runs the newest renderer for a few blocks, as its audio thread would, and returns the
    /// loudest sample.
    fn run_latest(&self) -> f32 {
        let mut renderers = self.renderers.lock().unwrap();
        let renderer = renderers.last_mut().unwrap();
        let mut block = vec![0.0f32; 256 * 2];
        let mut peak = 0.0f32;
        for _ in 0..8 {
            renderer.process(&mut block);
            peak = block.iter().fold(peak, |m, s| m.max(s.abs()));
        }
        peak
    }
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Result<Value, String> {
    projects.execute(id, &params).map_err(|e| e.0)
}

fn ports(names: &[&str], broken: Option<&str>) -> Arc<Ports> {
    Arc::new(Ports {
        ports: Mutex::new(names.iter().map(|n| n.to_string()).collect()),
        broken: broken.map(String::from),
        ..Ports::default()
    })
}

/// The ids of the blank project's tracks, in order.
fn track_ids(projects: &mut Projects) -> Vec<u64> {
    let tracks = run(projects, "project.tracks", json!({})).unwrap();
    tracks.as_array().unwrap().iter().map(|t| t["id"].as_u64().unwrap()).collect()
}

fn armed(projects: &mut Projects) -> Vec<bool> {
    let tracks = run(projects, "project.tracks", json!({})).unwrap();
    tracks.as_array().unwrap().iter().map(|t| t["armed"].as_bool().unwrap()).collect()
}

#[test]
fn midi_commands_are_listed_with_their_kind() {
    let ids: Vec<&str> = midi_commands().iter().map(|c| c.id).collect();
    assert_eq!(ids, ["midi.inputs", "midi.setInput"]);
    assert!(midi_commands().iter().all(|c| c.kind == CommandKind::Audio));
    let listed = suhina_session::all_commands();
    for id in ["midi.inputs", "midi.setInput", "track.setArm"] {
        assert!(listed.as_array().unwrap().iter().any(|c| c["id"] == id), "{id}");
    }
}

#[test]
fn without_midi_the_commands_say_so() {
    let mut projects = Projects::new();
    let err = run(&mut projects, "midi.inputs", json!({})).unwrap_err();
    assert!(err.contains("no MIDI inputs"), "{err}");
    assert!(run(&mut projects, "midi.setInput", json!({ "port": "x", "enabled": false })).is_err());
}

#[test]
fn every_port_opens_until_turned_off() {
    let midi = ports(&["Keys", "Pads"], None);
    let mut projects = Projects::new();
    assert!(projects.set_midi_system(midi.clone()).is_empty());
    assert_eq!(*midi.opened.lock().unwrap(), ["Keys", "Pads"]);

    let listed = run(&mut projects, "midi.inputs", json!({})).unwrap();
    assert_eq!(listed["inputs"][0], json!({ "name": "Keys", "enabled": true, "open": true, "error": null }));
    assert_eq!(listed["connected"], json!(false), "no project plays");

    let off = run(&mut projects, "midi.setInput", json!({ "port": "Pads", "enabled": false })).unwrap();
    assert_eq!(off["inputs"][1]["open"], json!(false));
    assert_eq!(*midi.closed.lock().unwrap(), ["Pads"]);
    assert_eq!(projects.midi_inputs_off(), ["Pads"]);

    run(&mut projects, "midi.setInput", json!({ "port": "Pads", "enabled": true })).unwrap();
    assert_eq!(*midi.opened.lock().unwrap(), ["Keys", "Pads", "Pads"]);
    assert!(projects.midi_inputs_off().is_empty());

    // A port plugged in later opens the next time the ports are looked at; one taken out closes.
    midi.ports.lock().unwrap().retain(|p| p != "Keys");
    midi.ports.lock().unwrap().push("Drums".into());
    let listed = run(&mut projects, "midi.inputs", json!({})).unwrap();
    let names: Vec<&str> = listed["inputs"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Pads", "Drums"]);
    assert!(midi.closed.lock().unwrap().contains(&"Keys".to_string()));
    assert_eq!(midi.opened.lock().unwrap().last().map(String::as_str), Some("Drums"));
}

#[test]
fn ports_turned_off_in_the_settings_stay_closed() {
    let midi = ports(&["Keys", "Pads"], None);
    let mut projects = Projects::new();
    projects.set_midi_inputs_off(["Pads".to_string(), "Unplugged".to_string()]);
    projects.set_midi_system(midi.clone());
    assert_eq!(*midi.opened.lock().unwrap(), ["Keys"]);
    // A port that is not there can still be turned off, and on again once it is off.
    run(&mut projects, "midi.setInput", json!({ "port": "Gone", "enabled": false })).unwrap();
    run(&mut projects, "midi.setInput", json!({ "port": "Gone", "enabled": true })).unwrap();
    let err = run(&mut projects, "midi.setInput", json!({ "port": "Gone", "enabled": true })).unwrap_err();
    assert!(err.contains("no MIDI input"), "{err}");
    for bad in [json!({ "port": "Keys" }), json!({ "port": 3, "enabled": true }), json!({ "enabled": "no" })] {
        assert!(run(&mut projects, "midi.setInput", bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn a_port_that_cannot_be_opened_is_said_once_and_listed_with_why() {
    let midi = ports(&["Keys", "Busy"], Some("Busy"));
    let mut projects = Projects::new();
    let notices = projects.set_midi_system(midi.clone());
    assert_eq!(notices, ["the MIDI input Busy could not be opened: taken by another program"]);
    let listed = run(&mut projects, "midi.inputs", json!({})).unwrap();
    assert_eq!(listed["inputs"][1]["error"], json!("taken by another program"));
    assert_eq!(listed["inputs"][1]["open"], json!(false));
    assert!(listed.get("notices").is_none(), "said once");
    assert_eq!(*midi.opened.lock().unwrap(), ["Keys"]);
}

#[test]
fn arming_is_exclusive_when_asked_and_is_not_an_edit() {
    let mut projects = Projects::new();
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let ids = track_ids(&mut projects);
    assert_eq!(armed(&mut projects)[..4], [false; 4]);

    let first = run(&mut projects, "track.setArm", json!({ "track": ids[0], "armed": true })).unwrap();
    assert_eq!(first["armed_tracks"], json!([ids[0]]));
    assert_eq!(first["playing"], json!(false), "arming works without an engine");
    run(&mut projects, "track.setArm", json!({ "track": ids[1], "armed": true })).unwrap();
    assert_eq!(armed(&mut projects)[..4], [true, true, false, false]);
    run(&mut projects, "track.setArm", json!({ "track": "3 Audio", "armed": true, "exclusive": true })).unwrap();
    assert_eq!(armed(&mut projects)[..4], [false, false, true, false]);
    run(&mut projects, "track.setArm", json!({ "track": ids[2], "armed": false })).unwrap();
    assert_eq!(armed(&mut projects)[..4], [false; 4]);

    let session = projects.session_mut(None).unwrap();
    assert!(!session.can_undo(), "arming is not an undo step");
    assert!(!session.has_unsaved_changes(), "and does not change the project");

    for bad in [
        json!({ "track": ids[0] }),
        json!({ "track": ids[0], "armed": "yes" }),
        json!({ "track": 999, "armed": true }),
        json!({ "track": ids[0], "armed": true, "exclusive": 1 }),
    ] {
        assert!(run(&mut projects, "track.setArm", bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn a_removed_track_is_no_longer_armed() {
    let mut projects = Projects::new();
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let ids = track_ids(&mut projects);
    run(&mut projects, "track.setArm", json!({ "track": ids[0], "armed": true })).unwrap();
    run(&mut projects, "track.delete", json!({ "track": ids[0] })).unwrap();
    assert!(projects.session_mut(None).unwrap().armed().is_empty());
    run(&mut projects, "edit.undo", json!({})).unwrap();
    // Undo brings the track back; it was armed and still is, since arming lives beside the
    // project.
    assert!(projects.session_mut(None).unwrap().is_armed(suhina_model::TrackId(ids[0] as u32)));
}

#[test]
fn a_note_from_a_port_plays_the_armed_tracks_instrument_also_on_a_new_output() {
    let midi = ports(&["Keys"], None);
    let stand = Arc::new(Stand::default());
    let mut projects = Projects::new();
    projects.set_audio_system(stand.clone());
    projects.set_midi_system(midi.clone());
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let ids = track_ids(&mut projects);
    run(&mut projects, "device.add", json!({ "track": ids[0], "id": "fm-synth" })).unwrap();
    projects.open_output(None).unwrap();
    projects.poll_plugins();
    let input = Arc::clone(&midi.inputs.lock().unwrap()[0]);
    assert!(input.is_connected(), "the playing engine takes the port's notes");
    assert!(Arc::ptr_eq(&input, &projects.live_input()));

    // Not armed: nothing is heard.
    assert!(input.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 }));
    assert_eq!(stand.run_latest(), 0.0);
    input.send(LiveEvent::NoteOff { key: 60 });

    run(&mut projects, "track.setArm", json!({ "track": ids[0], "armed": true })).unwrap();
    stand.run_latest();
    input.send(LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    assert!(stand.run_latest() > 0.0, "the armed track plays the note");
    input.send(LiveEvent::NoteOff { key: 64 });

    // A new output is a new engine: the notes follow it, and it knows the armed track.
    run(&mut projects, "audio.setOutput", json!({ "buffer_frames": 256 })).unwrap();
    projects.poll_plugins();
    assert!(input.is_connected());
    stand.run_latest();
    input.send(LiveEvent::NoteOn { key: 67, velocity: 1.0 });
    assert!(stand.run_latest() > 0.0, "heard on the new output");
}

#[test]
fn a_tracks_midi_input_is_an_edit_that_undo_takes_back() {
    let mut projects = Projects::new();
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let ids = track_ids(&mut projects);
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[0]["midi_input"], json!({ "port": "all", "channel": null }));

    let set =
        run(&mut projects, "track.setMidiInput", json!({ "track": ids[0], "port": "Pads", "channel": 10 })).unwrap();
    assert_eq!(set["midi_input"], json!({ "port": "Pads", "channel": 10 }));
    // A part left out stays.
    run(&mut projects, "track.setMidiInput", json!({ "track": ids[0], "channel": 0 })).unwrap();
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[0]["midi_input"], json!({ "port": "Pads", "channel": null }));
    run(&mut projects, "track.setMidiInput", json!({ "track": ids[1], "port": "none" })).unwrap();
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[1]["midi_input"]["port"], json!("none"));

    run(&mut projects, "edit.undo", json!({})).unwrap();
    run(&mut projects, "edit.undo", json!({})).unwrap();
    let tracks = run(&mut projects, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[0]["midi_input"], json!({ "port": "Pads", "channel": 10 }));
    assert_eq!(tracks[1]["midi_input"]["port"], json!("all"));

    for bad in [
        json!({ "track": ids[0], "port": "" }),
        json!({ "track": ids[0], "port": 3 }),
        json!({ "track": ids[0], "channel": 17 }),
        json!({ "track": ids[0], "channel": -1 }),
        json!({ "track": ids[0], "channel": "ten" }),
        json!({ "track": 999, "port": "all" }),
    ] {
        assert!(run(&mut projects, "track.setMidiInput", bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn two_controllers_play_two_armed_tracks_each_its_own() {
    use suhina_engine::{LiveSource, port_id};
    let midi = ports(&["Keys", "Pads"], None);
    let stand = Arc::new(Stand::default());
    let mut projects = Projects::new();
    projects.set_audio_system(stand.clone());
    projects.set_midi_system(midi.clone());
    projects.add(Session::new(suhina_session::blank_project()), None).unwrap();
    let ids = track_ids(&mut projects);
    for id in &ids[..2] {
        run(&mut projects, "device.add", json!({ "track": id, "id": "fm-synth" })).unwrap();
    }
    run(&mut projects, "track.setMidiInput", json!({ "track": ids[0], "port": "Keys" })).unwrap();
    run(&mut projects, "track.setMidiInput", json!({ "track": ids[1], "port": "Pads", "channel": 10 })).unwrap();
    projects.open_output(None).unwrap();
    projects.poll_plugins();
    run(&mut projects, "track.setArm", json!({ "track": ids[0], "armed": true })).unwrap();
    run(&mut projects, "track.setArm", json!({ "track": ids[1], "armed": true, "exclusive": false })).unwrap();
    let input = projects.live_input();
    let note = suhina_engine::LiveEvent::NoteOn { key: 60, velocity: 1.0 };
    let off = suhina_engine::LiveEvent::AllNotesOff;
    let keys = LiveSource { port: port_id("Keys"), channel: 0 };
    let pads = LiveSource { port: port_id("Pads"), channel: 9 };

    // Hear one track at a time by muting the other, and play each controller.
    let heard = |projects: &mut Projects, muted: u64, source: LiveSource| {
        run(projects, "track.setMute", json!({ "tracks": [muted], "mute": true })).unwrap();
        stand.run_latest();
        input.send_from(source, note);
        let level = stand.run_latest();
        input.send_from(source, off);
        stand.run_latest();
        run(projects, "track.setMute", json!({ "tracks": [muted], "mute": false })).unwrap();
        // Let the release fade before the next look.
        for _ in 0..60 {
            stand.run_latest();
        }
        level
    };
    assert!(heard(&mut projects, ids[1], keys) > 0.0, "the keyboard plays track 1");
    assert_eq!(heard(&mut projects, ids[0], keys), 0.0, "not track 2");
    assert!(heard(&mut projects, ids[0], pads) > 0.0, "the pads on channel 10 play track 2");
    assert_eq!(heard(&mut projects, ids[1], pads), 0.0, "not track 1");
    assert_eq!(heard(&mut projects, ids[1], LiveSource { channel: 0, ..pads }), 0.0, "pads on channel 1: nobody");

    // Changing an armed track's input reaches the engine without arming again.
    run(&mut projects, "track.setMidiInput", json!({ "track": ids[1], "port": "all", "channel": 0 })).unwrap();
    assert!(heard(&mut projects, ids[0], keys) > 0.0, "track 2 now hears the keyboard too");
}
