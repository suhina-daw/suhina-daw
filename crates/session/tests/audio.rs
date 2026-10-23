// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `audio.*` commands with a stand-in audio system: listing hosts, choosing an output,
//! reopening the playing project on it with the transport where it was, and reporting what
//! runs. Real devices are tested in `crates/output`.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use suhina_engine::{AudioPool, LaunchCommand, Renderer};
use suhina_model::Project;
use suhina_session::audio::{DeviceInfo, HostInfo, Opened};
use suhina_session::{
    AudioSystem, CommandKind, LiveOutput, OutputChoice, OutputInfo, Player, Projects, Session, audio_commands,
};

use crate::support::run;
use suhina_test_support::{WavFormat, write_wav};

const SR: u32 = 48_000;

/// Opens players on renderers it keeps, so a test can run them, and records each choice.
#[derive(Default)]
struct Stand {
    renderers: Mutex<Vec<Renderer>>,
    opened: Mutex<Vec<(OutputChoice, bool)>>,
    /// The device that is "not there": choosing it falls back with a notice. Besides
    /// "Default", the host lists "USB" unless it is the one missing.
    missing: Mutex<Option<String>>,
    /// Times the hosts were listed.
    listed: std::sync::atomic::AtomicUsize,
    /// Each opened output's error flag, to fail it as a driver would.
    failed: Mutex<Vec<Arc<std::sync::atomic::AtomicBool>>>,
}

/// An output: what it was opened on, its error flag, and whether it plays in place of the
/// device chosen.
struct Live(OutputChoice, Arc<std::sync::atomic::AtomicBool>, bool);

impl LiveOutput for Live {
    fn info(&self) -> OutputInfo {
        OutputInfo {
            host: self.0.host.clone().unwrap_or_else(|| "Stand".into()),
            device: self.0.device.clone().unwrap_or_else(|| "Default".into()),
            sample_rate: self.0.sample_rate.unwrap_or(SR),
            buffer_frames: self.0.buffer_frames.unwrap_or(512),
            engine_block: self.0.buffer_frames.unwrap_or(1024),
            latency_ms: 21.3,
            live: true,
            live_latency_ms: 26.6,
            underruns: 2,
            failed: self.1.load(std::sync::atomic::Ordering::Relaxed),
            fallback: self.2,
            ..OutputInfo::default()
        }
    }
}

impl AudioSystem for Stand {
    fn hosts(&self) -> Vec<HostInfo> {
        self.listed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let device = DeviceInfo {
            name: "Default".into(),
            default: true,
            channels: 2,
            sample_rates: vec![44_100, SR],
            default_sample_rate: Some(SR),
            buffer_sizes: vec![256, 512, 1024],
            buffer_range: Some((256, 1024)),
            exclusive: false,
        };
        let mut devices = vec![device.clone()];
        if self.missing.lock().unwrap().as_deref() != Some("USB") {
            devices.push(DeviceInfo { name: "USB".into(), default: false, ..device });
        }
        vec![HostInfo { name: "Stand".into(), default: true, devices, inputs: Vec::new(), error: None }]
    }

    fn open(
        &self,
        project: &Project,
        audio: &AudioPool,
        _assets: &suhina_engine::DeviceAssets,
        choice: &OutputChoice,
        previous: Option<Player>,
    ) -> Result<Opened, String> {
        let mut choice = choice.clone();
        let mut notice = None;
        let fallback = choice.device.is_some() && choice.device == *self.missing.lock().unwrap();
        if fallback {
            notice = Some("the audio device is not there; the default output plays".to_string());
            choice = OutputChoice::default();
        }
        self.opened.lock().unwrap().push((choice.clone(), previous.is_some()));
        drop(previous);
        let mut renderer = Renderer::stopped(project, audio, choice.sample_rate.unwrap_or(SR)).unwrap();
        let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.failed.lock().unwrap().push(Arc::clone(&failed));
        let player = Player::with_output(renderer.controller(), None, Arc::new(Live(choice, failed, fallback)));
        self.renderers.lock().unwrap().push(renderer);
        Ok(Opened { player, notice, plugins_kept: true })
    }
}

impl Stand {
    /// Runs the newest renderer for a few blocks, as its audio thread would.
    fn run_latest(&self) {
        let mut renderers = self.renderers.lock().unwrap();
        let renderer = renderers.last_mut().unwrap();
        let mut block = vec![0.0f32; 512 * 2];
        for _ in 0..4 {
            renderer.process(&mut block);
        }
    }
}

fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    suhina_session::blank_project()
}

fn with_stand(missing: Option<&str>) -> (Projects, Arc<Stand>) {
    let stand = Arc::new(Stand { missing: Mutex::new(missing.map(String::from)), ..Stand::default() });
    let mut projects = Projects::new();
    projects.set_audio_system(stand.clone());
    (projects, stand)
}

#[test]
fn audio_commands_are_listed_with_their_kind() {
    let ids: Vec<&str> = audio_commands().iter().map(|c| c.id).collect();
    assert_eq!(ids, ["audio.devices", "audio.setOutput", "output.setCuePair", "output.setCueVolume", "audio.info"]);
    assert!(audio_commands().iter().all(|c| c.kind == CommandKind::Audio));
    let listed = suhina_session::all_commands();
    assert!(listed.as_array().unwrap().iter().any(|c| c["id"] == "audio.setOutput"));
}

#[test]
fn without_an_audio_system_the_commands_say_so() {
    let mut projects = Projects::new();
    let err = run(&mut projects, "audio.devices", json!({})).unwrap_err();
    assert!(err.contains("no audio output"), "{err}");
    // Info and choosing still work: the choice waits for an output.
    assert_eq!(run(&mut projects, "audio.info", json!({})).unwrap()["playing"], json!(false));
    let set = run(&mut projects, "audio.setOutput", json!({ "bufferFrames": 256 })).unwrap();
    assert_eq!(set["chosen"]["buffer_frames"], json!(256));
    assert_eq!(projects.output_choice().buffer_frames, Some(256));
}

#[test]
fn devices_list_the_hosts_and_the_choice() {
    let (mut projects, _) = with_stand(None);
    let listed = run(&mut projects, "audio.devices", json!({})).unwrap();
    let device = &listed["hosts"][0]["devices"][0];
    assert_eq!(device["name"], json!("Default"));
    assert_eq!(device["buffer_sizes"], json!([256, 512, 1024]));
    assert_eq!(device["buffer_range"], json!({ "min": 256, "max": 1024 }));
    assert_eq!(listed["chosen"]["host"], Value::Null);
    assert!(run(&mut projects, "audio.devices", json!({ "host": "x" })).is_err(), "it takes no parameters");
}

#[test]
fn a_new_choice_reopens_the_playing_project_where_it_was() {
    let (mut projects, stand) = with_stand(None);
    projects.add(Session::new(project()), None).unwrap();
    assert_eq!(projects.open_output(None).unwrap(), None);
    stand.run_latest();

    // Play from beat 4 with the metronome on.
    {
        let player = projects.session_mut(None).unwrap();
        player.execute("transport.setPosition", &json!({ "beats": 4.0 })).unwrap();
        player.execute("transport.play", &json!({})).unwrap();
    }
    stand.run_latest();
    let before = projects.session_mut(None).unwrap().player().unwrap().monitor().beat();
    assert!(before >= 4.0, "{before}");

    let result = run(&mut projects, "audio.setOutput", json!({ "bufferFrames": 256, "sampleRate": 44_100 })).unwrap();
    assert_eq!(result["playing"], json!(true));
    assert_eq!(result["engine_block"], json!(256));
    assert_eq!(result["sample_rate"], json!(44_100));
    assert_eq!(result["notice"], Value::Null);
    let opened = stand.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 2);
    assert!(opened[1].1, "the old player was handed over to be closed");
    assert_eq!(opened[1].0.buffer_frames, Some(256));

    // The new engine plays on from where the old one was.
    stand.run_latest();
    let monitor_beat = projects.session_mut(None).unwrap().player().unwrap().monitor().beat();
    let running = projects.session_mut(None).unwrap().player().unwrap().monitor().running();
    assert!(running);
    assert!(monitor_beat >= before, "{monitor_beat} < {before}");

    let info = run(&mut projects, "audio.info", json!({})).unwrap();
    assert_eq!(info["latency_ms"], json!(21.3));
    assert_eq!((info["live"].clone(), info["live_latency_ms"].clone()), (json!(true), json!(26.6)));
    assert_eq!(info["underruns"], json!(2));
    assert_eq!(info["chosen"]["sample_rate"], json!(44_100));
}

#[test]
fn a_stopped_project_keeps_its_marker_on_the_new_output() {
    let (mut projects, stand) = with_stand(None);
    projects.add(Session::new(project()), None).unwrap();
    projects.open_output(None).unwrap();
    projects.session_mut(None).unwrap().execute("transport.setPosition", &json!({ "beats": 8.0 })).unwrap();
    stand.run_latest();
    run(&mut projects, "audio.setOutput", json!({ "bufferFrames": 512 })).unwrap();
    stand.run_latest();
    let monitor = projects.session_mut(None).unwrap().player().unwrap().monitor();
    assert!(!monitor.running());
    assert_eq!(monitor.start_beat(), 8.0);
}

#[test]
fn a_device_that_is_gone_falls_back_with_a_notice() {
    let (mut projects, _) = with_stand(Some("Unplugged"));
    projects.add(Session::new(project()), None).unwrap();
    projects.open_output(None).unwrap();
    let result = run(&mut projects, "audio.setOutput", json!({ "device": "Unplugged" })).unwrap();
    assert!(result["notice"].as_str().unwrap().contains("not there"));
    assert_eq!(result["device"], json!("Default"));
    assert_eq!(result["fallback"], json!(true));
    // The choice is kept, for when the device is back.
    assert_eq!(projects.output_choice().device.as_deref(), Some("Unplugged"));
}

#[test]
fn nothing_reopens_while_no_project_plays() {
    let (mut projects, stand) = with_stand(None);
    projects.add(Session::new(project()), None).unwrap();
    run(&mut projects, "audio.setOutput", json!({ "exclusive": true })).unwrap();
    assert!(stand.opened.lock().unwrap().is_empty());
    assert!(run(&mut projects, "audio.setOutput", json!({ "bufferFrames": 10 })).is_err());
    assert!(projects.output_choice().exclusive);
    // The next output opens as chosen.
    projects.open_output(None).unwrap();
    assert!(stand.opened.lock().unwrap()[0].0.exclusive);
    // A transport command needs no reopen to reach the new player.
    let mut player = projects.session_mut(None).unwrap().detach_player().unwrap();
    player.send(LaunchCommand::Play).unwrap();
}

#[test]
fn an_output_whose_device_fails_opens_again_but_not_every_frame() {
    let (mut projects, stand) = with_stand(None);
    projects.add(Session::new(project()), None).unwrap();
    projects.open_output(None).unwrap();
    assert!(projects.check_output().is_empty(), "nothing to do while it plays");
    // The driver asks for a reset.
    stand.failed.lock().unwrap()[0].store(true, std::sync::atomic::Ordering::Relaxed);
    let said = projects.poll_plugins();
    assert!(said.iter().any(|s| s.contains("opened again")), "{said:?}");
    assert_eq!(stand.opened.lock().unwrap().len(), 2);
    // The new one fails at once too: it is tried again later, not on the next frame.
    stand.failed.lock().unwrap()[1].store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(projects.check_output().is_empty());
    assert_eq!(stand.opened.lock().unwrap().len(), 2);
}

#[test]
fn the_output_goes_back_to_the_chosen_device_when_it_is_plugged_in_again() {
    let (mut projects, stand) = with_stand(Some("USB"));
    projects.add(Session::new(project()), None).unwrap();
    projects.open_output(None).unwrap();
    let chose = run(&mut projects, "audio.setOutput", json!({ "device": "USB" })).unwrap();
    assert_eq!((chose["device"].clone(), chose["fallback"].clone()), (json!("Default"), json!(true)));
    {
        let session = projects.session_mut(None).unwrap();
        session.execute("transport.setPosition", &json!({ "beats": 2.0 })).unwrap();
        session.execute("transport.play", &json!({})).unwrap();
    }
    stand.run_latest();

    // Not there yet: one look, and nothing opens.
    let listed = stand.listed.load(std::sync::atomic::Ordering::Relaxed);
    assert!(projects.poll_plugins().is_empty());
    assert_eq!(stand.listed.load(std::sync::atomic::Ordering::Relaxed), listed + 1);
    assert_eq!(stand.opened.lock().unwrap().len(), 2);
    // The next look waits a few seconds, not a frame.
    assert!(projects.check_output().is_empty());
    assert_eq!(stand.listed.load(std::sync::atomic::Ordering::Relaxed), listed + 1);

    // Choosing again looks at once; now the device is back and the output moves to it,
    // playing on where it was.
    *stand.missing.lock().unwrap() = None;
    projects.set_output_choice(projects.output_choice().clone());
    let before = projects.session_mut(None).unwrap().player().unwrap().monitor().beat();
    let said = projects.check_output();
    assert_eq!(said, ["the audio device USB is back; it plays again"]);
    let opened = stand.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 3);
    assert_eq!(opened[2].0.device.as_deref(), Some("USB"));
    stand.run_latest();
    let info = run(&mut projects, "audio.info", json!({})).unwrap();
    assert_eq!((info["device"].clone(), info["fallback"].clone()), (json!("USB"), json!(false)));
    let monitor = projects.session_mut(None).unwrap().player().unwrap().monitor();
    assert!(monitor.running());
    assert!(monitor.beat() >= before, "{} < {before}", monitor.beat());
    assert_eq!(monitor.start_beat(), 2.0, "the marker stays");
    // On the chosen device there is nothing to look for.
    assert!(projects.check_output().is_empty());
    assert_eq!(stand.opened.lock().unwrap().len(), 3);
}

#[test]
fn a_choice_is_found_by_its_host_and_device() {
    let stand = Stand::default();
    let hosts = stand.hosts();
    let choice = |host: Option<&str>, device: Option<&str>| OutputChoice {
        host: host.map(String::from),
        device: device.map(String::from),
        ..OutputChoice::default()
    };
    assert!(choice(None, None).found_in(&hosts));
    assert!(choice(None, Some(" usb ")).found_in(&hosts));
    assert!(choice(Some("stand"), Some("USB")).found_in(&hosts));
    assert!(!choice(None, Some("Headphones")).found_in(&hosts));
    assert!(!choice(Some("ASIO"), None).found_in(&hosts));
    assert!(!choice(None, None).found_in(&[]));
}

/// A project with two tracks whose first launcher slots hold looping clips.
fn launcher_project() -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = launcher_project_built();
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn launcher_project_built() -> Project {
    let mut p = project_built();
    let note = suhina_model::Note::new(0.0, 0.25, 60, 1.0);
    p.contents.push(suhina_model::ClipContent {
        id: suhina_model::ContentId(900),
        name: "loop".into(),
        length: 4.0,
        lanes: vec![suhina_model::ContentLane { target: None, data: suhina_model::LaneData::Notes(vec![note]) }],
        extensions: suhina_model::Extensions::default(),
        scale: None,
    });
    let clip = suhina_model::Clip {
        loop_range: Some(suhina_model::LoopRange { start: 0.0, end: 4.0 }),
        ..suhina_model::Clip::new(suhina_model::ContentId(900), 0.0, 4.0)
    };
    for column in p.launcher.columns.iter_mut().take(2) {
        if let Some(slot) = column.slots.first_mut() {
            slot.clip = Some(clip.clone());
        }
    }
    p
}

#[test]
fn a_reopen_keeps_the_marker_the_launched_clips_and_the_quantization() {
    let (mut projects, stand) = with_stand(None);
    projects.add(Session::new(launcher_project()), None).unwrap();
    projects.open_output(None).unwrap();
    {
        let session = projects.session_mut(None).unwrap();
        session.execute("launcher.setQuantization", &json!({ "value": "1/4" })).unwrap();
        session.execute("transport.setPosition", &json!({ "beats": 4.0 })).unwrap();
        session.execute("transport.play", &json!({})).unwrap();
        session.execute("clip.launch", &json!({ "track": 1, "scene": 1 })).unwrap();
    }
    // Long enough for the launch at the next beat, and a bit of the clip.
    for _ in 0..30 {
        stand.run_latest();
    }
    {
        let session = projects.session_mut(None).unwrap();
        let monitor = session.player().unwrap().monitor();
        assert_eq!(monitor.column(0).unwrap().playing, Some(0), "the clip plays");
        // A launch on the second track waits for the next beat when the output reopens.
        session.execute("clip.launch", &json!({ "track": 2, "scene": 1 })).unwrap();
    }
    stand.run_latest();
    let (beat, elapsed) = {
        let monitor = projects.session_mut(None).unwrap().player().unwrap().monitor();
        (monitor.beat(), monitor.elapsed_beats(0).unwrap())
    };
    assert!(elapsed > 0.0);

    run(&mut projects, "audio.setOutput", json!({ "bufferFrames": 256 })).unwrap();
    stand.run_latest();
    let monitor = projects.session_mut(None).unwrap().player().unwrap().monitor();
    assert!(monitor.running());
    assert_eq!(monitor.start_beat(), 4.0, "the marker stays where it was");
    assert!(monitor.beat() > beat, "{} <= {beat}", monitor.beat());
    let first = monitor.column(0).unwrap();
    assert_eq!((first.playing, first.overrides_arrangement), (Some(0), true), "the launched clip plays on");
    let on = monitor.elapsed_beats(0).unwrap();
    assert!(on >= elapsed && on - elapsed < 0.5, "{on} after {elapsed}");
    // The launch that was waiting waits again, for the next quarter note: the quantisation
    // carried over. A run is 2048 frames, so twelve are just over a beat; the default, a bar,
    // would wait until beat 8, well past them.
    assert_eq!(monitor.column(1).unwrap().queued, Some(suhina_engine::Queued::Launch(0)));
    assert!(beat < 6.9, "{beat}");
    for _ in 0..12 {
        stand.run_latest();
    }
    let monitor = projects.session_mut(None).unwrap().player().unwrap().monitor();
    assert_eq!(monitor.column(1).unwrap().playing, Some(0));
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-audio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A mono WAV file of a second at a steady level of 0.5.
fn wav(dir: &std::path::Path) -> String {
    let path = dir.join("listen.wav");
    write_wav(&path, SR, 1, WavFormat::Float32, &[0.5; SR as usize]);
    path.display().to_string()
}

/// A MIDI file with one note, middle C for a beat.
fn midi(dir: &std::path::Path) -> String {
    let path = dir.join("tune.mid");
    let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\0\x60".to_vec();
    let events = [0x00, 0x90, 60, 100, 0x60, 0x80, 60, 0, 0x00, 0xFF, 0x2F, 0x00];
    bytes.extend_from_slice(b"MTrk");
    bytes.extend_from_slice(&(events.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&events);
    std::fs::write(&path, bytes).unwrap();
    path.display().to_string()
}

#[test]
fn previews_play_with_no_project_open_on_an_output_of_their_own() {
    let dir = temp_dir("preview");
    let (mut projects, stand) = with_stand(None);
    assert!(projects.preview_session().is_none());
    run(&mut projects, "preview.stop", json!({})).unwrap();
    assert!(stand.opened.lock().unwrap().is_empty(), "stopping opens no output");

    let out = run(&mut projects, "preview.play", json!({ "path": wav(&dir), "gain": 0.0 })).unwrap();
    assert_eq!(out["kind"], "sample");
    assert_eq!(stand.opened.lock().unwrap().len(), 1, "one output, opened on the first preview");
    {
        let mut renderers = stand.renderers.lock().unwrap();
        let mut block = vec![0.0f32; 1024];
        renderers.last_mut().unwrap().process(&mut block);
        assert!(block[600].abs() > 0.4, "the preview is heard: {}", block[600]);
    }
    let out = run(&mut projects, "preview.play", json!({ "path": midi(&dir) })).unwrap();
    assert_eq!(out["kind"], "midi");
    run(&mut projects, "preview.stop", json!({})).unwrap();
    assert_eq!(stand.opened.lock().unwrap().len(), 1, "later previews use the same output");
    assert!(projects.preview_session().is_some_and(|s| s.player().is_some()));
    assert!(run(&mut projects, "transport.play", json!({})).is_err(), "only previews run with no project");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_preview_output_closes_before_a_project_or_another_output_opens() {
    let dir = temp_dir("preview-close");
    let (mut projects, stand) = with_stand(None);
    run(&mut projects, "preview.play", json!({ "path": wav(&dir) })).unwrap();
    projects.close_preview_output();
    assert!(projects.preview_session().is_none(), "closed before a project's output opens");

    run(&mut projects, "preview.play", json!({ "path": wav(&dir) })).unwrap();
    run(&mut projects, "audio.setOutput", json!({ "bufferFrames": 256 })).unwrap();
    assert!(projects.preview_session().is_none(), "another output: it opens there when next needed");
    run(&mut projects, "preview.play", json!({ "path": wav(&dir) })).unwrap();
    let last = stand.opened.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last.0.buffer_frames, Some(256));

    projects.add(Session::new(project()), None).unwrap();
    assert!(projects.preview_session().is_some_and(|s| s.player().is_none()), "the project's session now");
    run(&mut projects, "preview.play", json!({ "path": wav(&dir) })).unwrap_err();
    let _ = std::fs::remove_dir_all(&dir);
}
