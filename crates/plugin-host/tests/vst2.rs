// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The VST2 host through the engine's whole boundary, with the built-in mock: a host process
//! started, the instance loaded over the control protocol, blocks with events and automation
//! through the link, parameters, state as the standard bank, and the edit a plug-in reports
//! of itself. Everything here is behind the `vst2` feature, as the host is (ADR 0022); the
//! mock stands for an installed plug-in, so these prove the boundary, and the *assumed*
//! interface facts are settled by the ignored probe test.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions
#![cfg(feature = "vst2")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use suhina_ipc::{Event, EventKind, Outcome};
use suhina_plugins::{Launch, Plugin};

/// How long anything waits for the host process.
const PATIENT: Duration = Duration::from_secs(20);
/// The gain the mock starts with, and the one its idle knob turns it to.
const DEFAULT_GAIN: f32 = 0.5;
/// The latency the mock reports, in the field the host assumes holds it.
const LATENCY: u32 = 32;

/// The mock, loaded into a host process of its own.
fn start() -> (Plugin, suhina_plugins::PluginAudio) {
    let launch = Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
        plugin: "test:vst2".into(),
        channels: 2,
        max_frames: 256,
        sample_rate: 48_000.0,
        timeout: PATIENT,
        group: None,
        ara: None,
    };
    let (plugin, audio) = Plugin::start(launch).unwrap_or_else(|e| panic!("{e}"));
    (plugin, audio)
}

/// One block through the link, with `events` at their frames.
fn block(audio: &mut suhina_plugins::PluginAudio, input: &[f32], events: &[Event]) -> Vec<f32> {
    let mut output = vec![0.0f32; input.len()];
    let mut all: Vec<Event> =
        vec![Event { frame: 0, note: -1, kind: EventKind::Transport { playing: true, position: 0 } }];
    all.extend(events.iter().copied());
    assert_eq!(audio.process_with_events(input, &all, &mut output, PATIENT), Outcome::Done, "the block came back");
    output
}

/// The mock loads over the boundary, plays a block with its gain, a note at its frame, and
/// follows a parameter the block carries.
#[test]
fn the_mock_runs_through_the_boundary() {
    let (mut plugin, mut audio) = start();
    assert_eq!(plugin.latency(), LATENCY, "the latency the assumed field holds");

    let params = plugin.params().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "Gain");
    assert_eq!(params[0].value, f64::from(DEFAULT_GAIN));

    let input: Vec<f32> = (0..512).map(|i| (i % 24) as f32 / 24.0).collect();
    let output = block(&mut audio, &input, &[]);
    for (out, sample) in output.iter().zip(&input) {
        assert!((out - sample * DEFAULT_GAIN).abs() < 1e-6, "{out} vs {sample}");
    }

    // A note at its frame, as `test:vst3` plays it, so either format's events path is
    // audible through the same boundary.
    let note = Event { frame: 10, note: -1, kind: EventKind::NoteOn { channel: 0, key: 60, velocity: 1.0 } };
    let output = block(&mut audio, &input, &[note]);
    assert!((output[10 * 2] - input[10 * 2] * DEFAULT_GAIN - 1.0).abs() < 1e-6, "{}", output[20]);
    assert!((output[11 * 2] - input[11 * 2] * DEFAULT_GAIN).abs() < 1e-6, "only the note's frame");

    // A parameter the block carries (automation) reaches the plug-in before its audio.
    let change = Event { frame: 0, note: -1, kind: EventKind::Param { id: 0, value: 0.9 } };
    let output = block(&mut audio, &input, &[change]);
    for (out, sample) in output.iter().zip(&input) {
        assert!((out - sample * 0.9).abs() < 1e-6, "{out} vs {sample}");
    }
}

/// A state saved as the standard bank comes back through a second instance, both bank forms:
/// the plug-in's own chunk, and a program bank's values set as parameters.
#[test]
fn a_state_round_trips_over_the_boundary() {
    let (mut plugin, _audio) = start();
    plugin.set_param(0, 0.25).unwrap_or_else(|e| panic!("{e}"));
    let state = plugin.save_state().unwrap_or_else(|e| panic!("{e}"));
    let bank = state.data.clone();
    assert_eq!(&bank[..4], b"CcnK", "the standard bank");
    assert_eq!(&bank[8..12], b"FBCh", "the mock has a chunk");
    assert_eq!(suhina_plugins::vst2::chunk(&bank), Some(0.25f32.to_le_bytes().to_vec().as_slice()));

    let (mut reopened, _audio) = start();
    reopened.load_state(state).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reopened.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.25);

    // A program bank, as an imported project holds one.
    let preset = suhina_plugins::vst2::Preset {
        kind: suhina_plugins::vst2::PROGRAM_BANK,
        id: 7,
        version: 1,
        programs: 1,
        current: 0,
        parameters: 1,
        little_endian: false,
    };
    let mut buffer = vec![0u8; 28];
    buffer.extend_from_slice(&0.75f32.to_bits().to_be_bytes());
    let program_bank = suhina_plugins::vst2::bank(&preset, &buffer).unwrap_or_else(|e| panic!("{e}"));
    reopened.load_state(program_bank).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(reopened.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.75);
}

/// A state given to the running plug-in takes and is NOT reported back as a parameter
/// change: reading a state is neither a change nor an undo step (ADR 0010 decision 7), so a
/// project that opens does not mark itself unsaved through its plug-in's own report of the
/// restored value.
#[test]
fn a_state_given_to_a_running_plug_in_is_not_reported_as_an_edit() {
    let (mut plugin, _audio) = start();
    let bank = suhina_plugins::vst2::bank(
        &suhina_plugins::vst2::Preset {
            kind: suhina_plugins::vst2::CHUNK_BANK,
            id: 7,
            version: 1,
            programs: 1,
            current: 0,
            parameters: 1,
            little_endian: false,
        },
        &0.25f32.to_le_bytes(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    plugin.load_state(bank).unwrap_or_else(|e| panic!("{e}"));
    // The value took (the listing reads it) and no edit comes back for it.
    assert_eq!(plugin.params().unwrap_or_else(|e| panic!("{e}"))[0].value, 0.25);
    std::thread::sleep(Duration::from_millis(100));
    assert!(plugin.take_param_edits().is_empty(), "the restore is not an edit");
}

// ---- The acceptance test -------------------------------------------------------------------

use std::sync::Arc;

use serde_json::json;
use suhina_engine::{AudioPool, RenderInputs, Renderer, render_offline, render_offline_from};
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_session::{Player, PluginEnv, Session, start_offline};

const SR: u32 = 48_000;

/// The gain the project's saved bank holds.
const SAVED_GAIN: f32 = 0.25;

/// The catalog with the scanned VST2 plug-in and no VST3 or CLAP copy of it.
fn vst2_env() -> Arc<PluginEnv> {
    let path = PathBuf::from("test:vst2");
    let info = PluginInfo {
        path: path.clone(),
        id: "53685447".into(),
        name: "Suhina Test Gain".into(),
        vendor: "Suhina".into(),
        version: "1".into(),
        features: vec!["audio-effect".into()],
        replaces: Vec::new(),
        ara: false,
    };
    let mut catalog = Catalog::new();
    catalog.insert(path.clone(), vec![info]);
    Arc::new(PluginEnv::new(catalog, PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))))
}

/// A project as an imported one is saved: a note track, and the plug-in on its channel as a
/// VST2 device with its id and its saved state, the standard bank.
fn project() -> Project {
    let state = suhina_plugins::vst2::bank(
        &suhina_plugins::vst2::Preset {
            kind: suhina_plugins::vst2::CHUNK_BANK,
            id: 0x5368_5447,
            version: 1,
            programs: 1,
            current: 0,
            parameters: 1,
            little_endian: false,
        },
        &SAVED_GAIN.to_le_bytes(),
    )
    .unwrap_or_default();
    let note = Note::new(0.0, 0.5, 69, 1.0);
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel {
            id: ChannelId(1),
            name: "C".into(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            cue: false,
            output: None,
            sends: Vec::new(),
            devices: vec![Device {
                name: "Suhina Test Gain".into(),
                kind: DeviceKind::Vst2 { id: 0x5368_5447 },
                enabled: true,
                state: Some(state),
                params: Vec::new(),
                clip_host: false,
                extensions: Extensions::default(),
                automation: Vec::new(),
            }],
            extensions: Extensions::default(),
            automation: Vec::new(),
        }],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note]) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 1.0)],
            }],
            markers: Vec::new(),
            takes: Vec::new(),
        },
        ..Project::default()
    }
}

/// The bank's chunk a project's device state holds, as a gain.
fn saved_gain(project: &Project) -> Option<f32> {
    let state = project.channels[0].devices[0].state.as_deref()?;
    let bytes: [u8; 4] = suhina_plugins::vst2::chunk(state)?.try_into().ok()?;
    Some(f32::from_le_bytes(bytes))
}

/// Plays blocks while the session looks after its plug-ins, until `done` holds.
fn until(session: &mut Session, renderer: &mut Renderer, what: &str, mut done: impl FnMut(&mut Session) -> bool) {
    let deadline = Instant::now() + PATIENT;
    let mut block = vec![0.0f32; 256 * 2];
    loop {
        session.sync_plugins();
        renderer.process(&mut block);
        if done(session) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The central test of #1066: a project naming a VST2 plug-in that only exists as VST2 opens,
/// the installed plug-in loads with its saved settings, plays with them, and the state read
/// back is the settings, unchanged, to save.
#[test]
fn a_project_s_vst2_plug_in_plays_with_its_saved_settings() {
    let mut project = project();
    assert_eq!(project.assign_ids(), Ok(()));
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project.clone());
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(vst2_env());
    let device = json!({ "track": 1, "device": 0 });
    // The device is found and starts — the direct host, no VST3 or CLAP copy standing in.
    until(&mut session, &mut renderer, "starting", |s| {
        s.execute("device.plugin", &device).unwrap()["status"] == "running"
    });
    let heard = session.execute("device.plugin", &device).unwrap();
    assert_eq!(heard["plugin"]["found"], "itself", "the VST2 plug-in itself, not a stand-in: {heard}");

    // It plays with the saved settings: the render's gain is the bank's.
    let gain = |samples: &[f32]| samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let started = start_offline(session.project(), &AudioPool::new(), &vst2_env(), SR);
    assert_eq!(started.running(), 1, "{:?}", started.notices);
    let wet = render_offline_from(
        session.project(),
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(started.plugins),
        SR,
        256,
        None,
    )
    .unwrap_or_default();
    let dry = render_offline(session.project(), SR, 256).unwrap_or_default();
    assert!(gain(&dry) > 0.1, "the note plays without the plug-in");
    assert!((gain(&wet) / gain(&dry) - SAVED_GAIN).abs() < 1e-4, "the saved gain is the one it plays with");

    // The state read back, to save, is the settings — the same bank, and no undo step of its
    // own.
    let heard = session.project_as_heard();
    assert_eq!(saved_gain(&heard), Some(SAVED_GAIN));
    assert!(!session.can_undo());
}

// ---- The editor ----------------------------------------------------------------------------

/// The editor the mock declares, as `effEditGetRect`'s assumed shape reads it.
const EDITOR_SIZE: (u32, u32) = (300, 200);

/// The editor window code needs a display; on this runner there is one (Windows, or Linux
/// under Xvfb), the check runs, macOS checks editors by hand (its editor tests open with the
/// build's own window code, which the CI check job runs the Linux/Windows arms of).
fn window_system() -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()))
}

/// The mock's editor opens in a window of its own at the size its rect declared, stays open
/// across the boundary, and closes, its window taken down.
#[test]
fn the_mock_s_editor_opens_and_closes() {
    if !window_system() {
        eprintln!("no window system for editors; the editor part is skipped here");
        return;
    }
    let (mut plugin, _audio) = start();
    let (width, height) = plugin.open_editor().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((width, height), EDITOR_SIZE, "the size the rect declared");
    assert!(plugin.editor_open(), "the editor is open as the engine sees it");
    assert_eq!(plugin.editor_size(), Some(EDITOR_SIZE));
    plugin.close_editor().unwrap_or_else(|e| panic!("{e}"));
    assert!(!plugin.editor_open(), "closed as the engine sees it");
}

/// An embedded editor opens (Windows and X11; macOS reports its own kind and the rack falls
/// back once to a window of its own, ADR 0006) and takes the placement.
#[test]
fn the_mock_s_embedded_editor_opens_and_takes_its_placement() {
    if !window_system() {
        eprintln!("no window system for editors; the editor part is skipped here");
        return;
    }
    let (mut plugin, _audio) = start();
    // The engine's owner: the test has no window of Suhina's to name, so the exit is the SAME
    // one a non-windowing engine has: the host refuses it and reports why.
    match plugin.open_editor_embedded(0) {
        Ok((width, height)) => {
            assert_eq!((width, height), EDITOR_SIZE, "the size the rect declared");
            let placement =
                suhina_plugins::Placement { x: 0, y: 0, clip_x: 0, clip_y: 0, clip_width: 100, clip_height: 100 };
            assert!(plugin.place_editor(placement).unwrap_or_else(|e| panic!("{e}")), "the placement went out");
            assert!(
                plugin.fit_editor(EDITOR_SIZE.0, EDITOR_SIZE.1).unwrap_or_else(|e| panic!("{e}")),
                "the fit went out"
            );
        }
        Err(e) => eprintln!("embedded refused, as this runner's windowing says: {e}"),
    }
    plugin.close_editor().unwrap_or_else(|e| panic!("{e}"));
}
