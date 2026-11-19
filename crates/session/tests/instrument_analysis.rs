// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One sequential lifecycle owns the process-wide analysis admission budget. Completion uses
//! the worker's channel barrier; correctness never depends on how quickly the worker runs.
#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{AnalysisPhase, OpenOptions, Player, Projects, Session};

fn project() -> Project {
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Sample")
        }],
        channels: vec![Channel::new(ChannelId(1), "Sample")],
        ..Project::default()
    }
}

fn run(session: &mut Session, id: &str, mut params: Value) -> Value {
    params["track"] = json!(1);
    if id != "device.add" && params.get("device").is_none() {
        params["device"] = json!(0);
    }
    session.execute(id, &params).unwrap()
}

fn render(project: &Project, audio: &AudioPool) -> Vec<f32> {
    let mut renderer = Renderer::stopped(project, audio, 48000, suhina_session::devices()).unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    let mut session = Session::new(project.clone());
    session.set_audio(audio.clone());
    session.attach_player(Player::new(controller, None));
    session.execute("track.setArm", &json!({"track":1,"armed":true})).unwrap();
    live.send(suhina_engine::LiveEvent::NoteOn { key: 60, velocity: 0.8 });
    let mut output = vec![0.0; 8192];
    for block in output.chunks_mut(256) {
        renderer.process(block);
    }
    output
}

fn edit_while_held(project: &Project, audio: &AudioPool, adopt: bool) -> Vec<f32> {
    let mut renderer = Renderer::stopped(project, audio, 48000, suhina_session::devices()).unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    let mut session = Session::new(project.clone());
    session.set_audio(audio.clone());
    let proposal = if adopt {
        let job = run(&mut session, "instrument.analyzeSample", json!({}));
        session.sync_instrument_analysis();
        Some(job)
    } else {
        None
    };
    session.attach_player(Player::new(controller, None));
    session.execute("track.setArm", &json!({"track":1,"armed":true})).unwrap();
    live.send(suhina_engine::LiveEvent::NoteOn { key: 60, velocity: 0.8 });
    let mut output = vec![0.0; 8192];
    for (index, block) in output.chunks_mut(256).enumerate() {
        if index == 8 {
            if let Some(job) = &proposal {
                run(&mut session, "instrument.acceptAnalysis", json!({"job":job["job"]}));
            } else {
                run(&mut session, "device.setParam", json!({"param":"root","value":57}));
            }
        }
        renderer.process(block);
    }
    output
}

#[test]
fn sample_proposals_are_explicit_atomic_cancellable_and_tied_to_the_current_source() {
    let folder = super::temp_dir::TempDir::new("instrument-analysis");
    let path = folder.join("authored-a3.wav");
    let samples: Vec<_> =
        (0..24000).map(|i| (std::f32::consts::TAU * 220.0 * i as f32 / 48000.0).sin() * 0.4).collect();
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Int16, &samples);
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    assert!(session.execute("instrument.analyzeSample", &json!({"track":1,"device":0})).is_err());
    run(&mut session, "device.loadSample", json!({"path":path}));
    run(&mut session, "automation.addPoint", json!({"param":"root","beat":0,"value":60}));
    let before = session.project().clone();
    let depth = session.undo_steps();
    let before_sound = render(&before, session.audio());
    let first = run(&mut session, "instrument.analyzeSample", json!({}));
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    assert_eq!(status["status"], "ready");
    assert_eq!(status["report"]["proposal"]["root"], 57);
    assert_eq!(status["report"]["sourceAssumption"], "singleIsolatedPitchedNote");
    assert_eq!(status["report"]["polyphonyAssessed"], false);
    assert_eq!(session.project(), &before, "a completed proposal never changes the playable device");
    assert_eq!(session.undo_steps(), depth);
    assert_eq!(render(session.project(), session.audio()), before_sound);
    // Regression: an unrelated invalid project makes acceptance fail even when its explicit
    // corrected root equals the original root. Value equality is not transaction success.
    let mut invalid = before.clone();
    invalid.transport.tempo_bpm = -1.0;
    let mut rejected = Session::new(invalid.clone());
    rejected.set_audio(session.audio().clone());
    let proposal = run(&mut rejected, "instrument.analyzeSample", json!({}));
    rejected.sync_instrument_analysis();
    let rejected_depth = rejected.undo_steps();
    assert!(
        rejected
            .execute("instrument.acceptAnalysis", &json!({"track":1,"device":0,"job":proposal["job"],"root":60}))
            .is_err()
    );
    assert_eq!(rejected.project(), &invalid);
    assert_eq!(rejected.undo_steps(), rejected_depth);
    assert_eq!(rejected.instrument_analysis_view().unwrap().phase, AnalysisPhase::Ready);
    for params in [
        json!({"track":1,"device":0,"job":"outdated"}),
        json!({"track":1,"device":0,"job":first["job"],"root":128}),
        json!({"track":1,"device":0,"job":first["job"],"root":57.5}),
    ] {
        assert!(session.execute("instrument.acceptAnalysis", &params).is_err());
        assert_eq!(session.project(), &before);
        assert_eq!(session.undo_steps(), depth);
    }
    let result = run(&mut session, "instrument.acceptAnalysis", json!({"job":first["job"]}));
    assert_eq!(result["root"], 57);
    assert_eq!(result["automation"], "off");
    assert_eq!(session.undo_steps().0, depth.0 + 1);
    let accepted = session.project().clone();
    let accepted_sound = render(&accepted, session.audio());
    assert_ne!(accepted_sound, before_sound);
    assert_eq!(
        accepted.channels[0].devices[0].automation[0].points,
        before.channels[0].devices[0].automation[0].points
    );
    assert!(!accepted.channels[0].devices[0].automation[0].enabled);
    assert_eq!(session.instrument_analysis_view().unwrap().accepted_root, Some(57));
    let mut reference = Session::new(before.clone());
    reference.set_audio(session.audio().clone());
    run(&mut reference, "device.setParam", json!({"param":"root","value":57}));
    assert_eq!(render(reference.project(), reference.audio()), accepted_sound);
    let mut held_project = before.clone();
    held_project.channels[0].devices[0].automation.clear();
    assert_eq!(
        edit_while_held(&held_project, session.audio(), true),
        edit_while_held(&held_project, session.audio(), false),
        "adoption preserves held voices exactly as the ordinary root control does"
    );
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &before);
    session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(session.project(), &accepted);

    // A cancelled worker may already have completed: either ordering keeps the project untouched.
    let second = run(&mut session, "instrument.analyzeSample", json!({}));
    run(&mut session, "instrument.cancelAnalysis", json!({"job":second["job"]}));
    session.sync_instrument_analysis();
    assert_eq!(session.instrument_analysis_view().unwrap().phase, AnalysisPhase::Cancelled);
    assert!(session.execute("instrument.acceptAnalysis", &json!({"track":1,"device":0,"job":second["job"]})).is_err());
    assert_eq!(session.project(), &accepted);

    let third = run(&mut session, "instrument.analyzeSample", json!({}));
    run(&mut session, "device.setParam", json!({"param":"start","value":0.1}));
    session.sync_instrument_analysis();
    assert_eq!(session.instrument_analysis_view().unwrap().phase, AnalysisPhase::Stale);
    let trimmed = session.project().clone();
    assert!(session.execute("instrument.acceptAnalysis", &json!({"track":1,"device":0,"job":third["job"]})).is_err());
    assert_eq!(session.project(), &trimmed);
    run(&mut session, "device.setParam", json!({"param":"start","value":0}));

    // A corrected root is explicit; status names the root actually accepted, not the estimate.
    let fourth = run(&mut session, "instrument.analyzeSample", json!({}));
    session.sync_instrument_analysis();
    run(&mut session, "instrument.acceptAnalysis", json!({"job":fourth["job"],"root":58}));
    assert_eq!(run(&mut session, "instrument.analysis", json!({}))["acceptedRoot"], 58);
    let expected = render(session.project(), session.audio());
    let plugin_library = suhina_plugins::library::Library::new(Some(folder.join("plugins")), folder.join("no-host"));
    let mut projects = Projects::with_library(plugin_library);
    projects.set_file_library(suhina_library::Library::new(Some(folder.join("settings"))));
    projects.add(session, None).unwrap();
    let song = folder.join("instrument.suhina");
    projects.execute("project.save", &json!({"path":song})).unwrap();
    let (mut reopened, warnings) = projects.open_path(&song, OpenOptions { audio: true }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(render(reopened.project(), reopened.audio()), expected);
    assert!(reopened.instrument_analysis_view().is_none(), "reopen never starts analysis");

    // A group move preserves identity and the source. Both a full path and stable id reach it.
    let stable = reopened.project().channels[0].devices[0].id;
    let moved = run(&mut reopened, "instrument.analyzeSample", json!({}));
    run(&mut reopened, "device.group", json!({}));
    reopened.sync_instrument_analysis();
    assert_eq!(run(&mut reopened, "instrument.analysis", json!({"device":"0/0/0"}))["status"], "ready");
    run(&mut reopened, "instrument.acceptAnalysis", json!({"device":{"id":stable.0},"job":moved["job"]}));
    run(&mut reopened, "device.ungroup", json!({}));
    assert_eq!(reopened.project().channels[0].devices[0].id, stable);

    // Replacing decoded PCM under the same file name invalidates the source snapshot too.
    let replaced_pcm = run(&mut reopened, "instrument.analyzeSample", json!({}));
    let mut audio = reopened.audio().clone();
    let file = reopened.project().channels[0].devices[0].sample_file().unwrap();
    let pcm = audio.files.get(file).unwrap();
    audio.files.insert(file.to_owned(), std::sync::Arc::new((**pcm).clone()));
    reopened.set_audio(audio);
    reopened.sync_instrument_analysis();
    assert_eq!(reopened.instrument_analysis_view().unwrap().phase, AnalysisPhase::Stale);
    assert!(
        reopened
            .execute("instrument.acceptAnalysis", &json!({"track":1,"device":0,"job":replaced_pcm["job"]}))
            .is_err()
    );
    let changed_root = run(&mut reopened, "instrument.analyzeSample", json!({}));
    run(&mut reopened, "device.setParam", json!({"param":"root","value":59}));
    reopened.sync_instrument_analysis();
    assert_eq!(reopened.instrument_analysis_view().unwrap().phase, AnalysisPhase::Stale);
    assert!(
        reopened
            .execute("instrument.acceptAnalysis", &json!({"track":1,"device":0,"job":changed_root["job"]}))
            .is_err()
    );

    // Device removal/replacement must not accept a late proposal even at the same list position.
    let old_id = reopened.project().channels[0].devices[0].id;
    let fifth = run(&mut reopened, "instrument.analyzeSample", json!({"device":{"id":old_id.0}}));
    run(&mut reopened, "device.remove", json!({}));
    run(&mut reopened, "device.add", json!({"id":"instrument"}));
    reopened.sync_instrument_analysis();
    assert_ne!(reopened.project().channels[0].devices[0].id, old_id);
    assert_eq!(reopened.instrument_analysis_view().unwrap().phase, AnalysisPhase::Stale);
    let replaced = reopened.project().clone();
    assert!(reopened.execute("instrument.acceptAnalysis", &json!({"track":1,"device":0,"job":fifth["job"]})).is_err());
    assert_eq!(reopened.project(), &replaced);
}
