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
    invalid.tracks[0].midi_input.channel = Some(0);
    assert!(invalid.validate().is_err(), "the rollback fixture must actually fail project validation");
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
    modal_fitting_lifecycle();
    folder_fitting_lifecycle();
}

fn strike(scale: f64) -> Vec<f32> {
    (0..96000)
        .map(|i| {
            let t = i as f64 / 48000.0;
            (0.5 * (-1000.0_f64.ln() * t / 1.6).exp() * (std::f64::consts::TAU * 447.3 * scale * t).sin()
                + 0.25 * (-1000.0_f64.ln() * t / 0.9).exp() * (std::f64::consts::TAU * 1237.9 * scale * t + 0.4).sin())
                as f32
        })
        .collect()
}

/// A decaying harmonic note at `key`, `scale` loud: three partials with their own decays.
fn note(key: u8, scale: f64) -> Vec<f32> {
    let hz = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
    (0..96000)
        .map(|i| {
            let t = i as f64 / 48000.0;
            let partial = |n: f64, t60: f64, amplitude: f64| {
                amplitude * (-1000.0_f64.ln() * t / t60).exp() * (std::f64::consts::TAU * hz * n * t + 0.3 * n).sin()
            };
            (scale * (partial(1.0, 3.0, 0.5) + partial(2.0, 2.0, 0.25) + partial(3.0, 1.2, 0.12))) as f32
        })
        .collect()
}

fn folder_fitting_lifecycle() {
    let folder = super::temp_dir::TempDir::new("instrument-folder-fit");
    let notes = folder.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    for (name, key, scale) in [("c4-soft.wav", 60, 0.25), ("c4-hard.wav", 60, 0.9), ("c5.wav", 72, 0.6)] {
        suhina_test_support::write_wav(
            &notes.join(name),
            48000,
            1,
            suhina_test_support::WavFormat::Float32,
            &note(key, scale),
        );
    }
    std::fs::write(notes.join("broken.wav"), b"not audio").unwrap();
    std::fs::write(notes.join("readme.txt"), b"").unwrap();
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    let before = session.project().clone();
    let depth = session.undo_steps();
    assert!(
        session.execute("instrument.fitFolder", &json!({"track":1,"device":0,"path":folder.join("missing")})).is_err()
    );
    let job = run(&mut session, "instrument.fitFolder", json!({"path":notes}));
    assert_eq!(job["kind"], "folder");
    assert_eq!(job["files"], 4, "audio files only: {job}");
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    assert_eq!(status["status"], "ready", "{status}");
    let files = status["folder"]["files"].as_array().unwrap();
    let summary: Vec<_> = files
        .iter()
        .map(|f| {
            (f["name"].as_str().unwrap(), f["key"].as_u64(), f["velocity"].as_f64(), f["outcome"].as_str().unwrap())
        })
        .collect();
    let single = f64::from(suhina_instrument_analysis::modal::DEFAULT_VELOCITY);
    assert_eq!(
        summary,
        vec![
            ("broken.wav", None, None, "skipped"),
            ("c4-hard.wav", Some(60), Some(1.0), "kept"),
            ("c4-soft.wav", Some(60), Some(0.5), "kept"),
            ("c5.wav", Some(72), Some(single), "kept"),
        ],
        "{status}"
    );
    assert!(files[0]["reason"].as_str().unwrap().contains("decoded"), "{status}");
    assert_eq!(status["folder"]["recordings"], 3);
    assert_eq!(session.project(), &before, "a folder fit changes nothing until accepted");
    assert_eq!(session.undo_steps(), depth);
    assert!(
        session.execute("instrument.addFit", &json!({"track":1,"device":0,"job":job["job"]})).is_err(),
        "add needs an accepted model"
    );
    assert!(
        session
            .execute("instrument.auditionFit", &json!({"track":1,"device":0,"job":job["job"],"which":"fitted"}))
            .is_err()
    );
    let accepted = run(&mut session, "instrument.acceptFit", json!({"job":job["job"]}));
    assert_eq!(accepted["accepted"], true);
    assert_eq!(accepted["recordings"], 3);
    assert_eq!(session.undo_steps().0, depth.0 + 1);
    assert_eq!(session.instrument_analysis_view().unwrap().phase, AnalysisPhase::Accepted);
    let device = &session.project().channels[0].devices[0];
    let set = suhina_dsp::modal::model::ModelSet::decode(device.instrument_modal_profile().unwrap()).unwrap();
    let layers: Vec<_> = set.recordings().iter().map(|r| (r.key, r.velocity)).collect();
    assert_eq!(layers, vec![(60, 0.5), (60, 1.0), (72, suhina_instrument_analysis::modal::DEFAULT_VELOCITY)]);
    let accepted_project = session.project().clone();
    let sound = render(&accepted_project, session.audio());
    assert!(sound.iter().all(|x| x.is_finite()) && sound.iter().any(|x| x.abs() > 1e-5));

    // The same folder again would duplicate every key and velocity: nothing is added.
    let again = run(&mut session, "instrument.fitFolder", json!({"path":notes}));
    session.sync_instrument_analysis();
    assert!(
        session.execute("instrument.addFit", &json!({"track":1,"device":0,"job":again["job"],"velocity":0.5})).is_err(),
        "a folder's velocities come from its layers"
    );
    assert!(session.execute("instrument.addFit", &json!({"track":1,"device":0,"job":again["job"]})).is_err());
    assert_eq!(session.project(), &accepted_project);
    // As another source (instrument) all three join, in one undo step.
    assert!(
        session
            .execute("instrument.addFit", &json!({"track":1,"device":0,"job":again["job"],"source":1,"newSource":true}))
            .is_err()
    );
    let added = run(&mut session, "instrument.addFit", json!({"job":again["job"],"newSource":true}));
    assert_eq!(added["added"], true);
    assert_eq!(added["recordings"], 6);
    assert_eq!(added["recordingSource"], 1);
    assert_eq!(session.undo_steps().0, depth.0 + 2);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &accepted_project);

    // A ready folder fit is stale once the Instrument's fitted model changes.
    let stale = run(&mut session, "instrument.fitFolder", json!({"path":notes}));
    session.execute("edit.undo", &json!({})).unwrap();
    session.sync_instrument_analysis();
    assert_eq!(session.instrument_analysis_view().unwrap().phase, AnalysisPhase::Stale);
    assert!(session.execute("instrument.acceptFit", &json!({"track":1,"device":0,"job":stale["job"]})).is_err());
    assert_eq!(session.project(), &before);
}

fn modal_fitting_lifecycle() {
    let folder = super::temp_dir::TempDir::new("instrument-modal-fit");
    let path = folder.join("authored-strike.wav");
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Float32, &strike(1.0));
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    run(&mut session, "device.loadSample", json!({"path":path}));
    run(&mut session, "device.setParam", json!({"param":"body_decay","value":4}));
    run(&mut session, "device.setParam", json!({"param":"quality","value":0}));
    run(&mut session, "automation.addPoint", json!({"param":"body_decay","beat":0,"value":4}));
    let before = session.project().clone();
    let depth = session.undo_steps();
    let sample_sound = render(&before, session.audio());
    let job = run(&mut session, "instrument.fitSample", json!({}));
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    assert_eq!(status["kind"], "modal");
    assert_eq!(status["status"], "ready");
    assert_eq!(status["modal"]["outcome"], "fitted", "{status}");
    assert_eq!(status["modal"]["validationScope"], "sameRecordingTemporalEnvelopes");
    assert_eq!(status["modal"]["phaseReconstructed"], true, "phases are measured from the recording");
    assert!(status["modal"]["recording"]["modeCount"].as_u64().unwrap() >= 2, "{status}");
    assert_eq!(session.project(), &before);
    assert_eq!(session.undo_steps(), depth);
    assert_eq!(render(session.project(), session.audio()), sample_sound);
    assert!(session.execute("instrument.acceptAnalysis", &json!({"track":1,"device":0,"job":job["job"]})).is_err());
    assert!(session.execute("instrument.acceptFit", &json!({"track":1,"device":0,"job":"old"})).is_err());
    let accepted = run(&mut session, "instrument.acceptFit", json!({"job":job["job"]}));
    assert_eq!(accepted["accepted"], true);
    assert_eq!(accepted["automation"], "off");
    assert_eq!(session.undo_steps().0, depth.0 + 1);
    assert_eq!(session.instrument_analysis_view().unwrap().phase, AnalysisPhase::Accepted);
    // Level-matched A/B: without an audio output it reports the comparison and plays nothing.
    let steps = session.undo_steps();
    let audition = run(&mut session, "instrument.auditionFit", json!({"job":job["job"],"which":"fitted"}));
    assert_eq!(audition["played"], false);
    assert!(audition["meanDistanceDb"].as_f64().unwrap() < 6.0, "{audition}");
    assert!(audition["levelDifferenceDb"].as_f64().unwrap().abs() < 3.0, "{audition}");
    assert!(session.instrument_analysis_view().unwrap().comparison.is_some());
    assert!(
        session
            .execute("instrument.auditionFit", &json!({"track":1,"device":0,"job":job["job"],"which":"both"}))
            .is_err()
    );
    assert_eq!(session.undo_steps(), steps, "auditioning is not an edit");
    let fitted = session.project().clone();
    let device = &fitted.channels[0].devices[0];
    assert_eq!(device.sample_file(), before.channels[0].devices[0].sample_file());
    assert_eq!(device.state, before.channels[0].devices[0].state);
    let set = suhina_dsp::modal::model::ModelSet::decode(device.instrument_modal_profile().unwrap()).unwrap();
    assert_eq!(set.recordings().len(), 1);
    assert_eq!(set.recordings()[0].key, 60);
    assert!(set.recordings()[0].modes.len() >= 2);
    assert_eq!(accepted["revision"], 4);
    assert_eq!(device.params.iter().find(|p| p.id == "model_revision").unwrap().value, 4.0);
    assert_eq!(device.params.iter().find(|p| p.id == "quality").unwrap().value, 1.0);
    assert_eq!(device.params.iter().find(|p| p.id == "body_decay").unwrap().value, 2.0);
    assert_eq!(device.automation[0].points, before.channels[0].devices[0].automation[0].points);
    assert!(!device.automation[0].enabled);
    let model_sound = render(&fitted, session.audio());
    assert!(model_sound.iter().any(|x| x.abs() > 1e-5));
    assert_ne!(model_sound, sample_sound);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &before);
    session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(session.project(), &fitted);
    assert_eq!(render(session.project(), session.audio()), model_sound);

    // A recording of another note joins the accepted model, fused by key, as one undo step.
    let higher = folder.join("authored-strike-octave.wav");
    suhina_test_support::write_wav(&higher, 48000, 1, suhina_test_support::WavFormat::Float32, &strike(2.0));
    run(&mut session, "device.loadSample", json!({"path":higher}));
    run(&mut session, "device.setParam", json!({"param":"root","value":72}));
    assert!(
        session.execute("instrument.addFit", &json!({"track":1,"device":0,"job":job["job"]})).is_err(),
        "an accepted or stale fit cannot be added"
    );
    let second = run(&mut session, "instrument.fitSample", json!({}));
    session.sync_instrument_analysis();
    assert_eq!(session.instrument_analysis_view().unwrap().model_recordings, Some(1));
    let before_add = session.project().clone();
    let add_depth = session.undo_steps();
    assert!(
        session
            .execute("instrument.addFit", &json!({"track":1,"device":0,"job":second["job"],"velocity":1.5}))
            .is_err()
    );
    assert_eq!(session.project(), &before_add);
    let added = run(&mut session, "instrument.addFit", json!({"job":second["job"],"velocity":0.9}));
    assert_eq!(added["recordings"], 2);
    assert_eq!(session.undo_steps().0, add_depth.0 + 1);
    let device = &session.project().channels[0].devices[0];
    let set = suhina_dsp::modal::model::ModelSet::decode(device.instrument_modal_profile().unwrap()).unwrap();
    let keys: Vec<_> = set.recordings().iter().map(|r| (r.key, r.velocity)).collect();
    assert_eq!(keys, vec![(60, 0.8), (72, 0.9)]);
    assert_eq!(device.params.iter().find(|p| p.id == "source").unwrap().value, 0.0, "Add selects Modal");
    let fused = render(session.project(), session.audio());
    assert!(fused.iter().all(|x| x.is_finite()) && fused.iter().any(|x| x.abs() > 1e-5));
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &before_add);
    for _ in 0..2 {
        session.execute("edit.undo", &json!({})).unwrap();
    }
    assert_eq!(session.project(), &fitted);

    // An invalid transaction must leave both the project and the ready candidate intact.
    let mut invalid = before.clone();
    invalid.tracks[0].midi_input.channel = Some(0);
    assert!(invalid.validate().is_err());
    let mut rejected = Session::new(invalid.clone());
    rejected.set_audio(session.audio().clone());
    let proposal = run(&mut rejected, "instrument.fitSample", json!({}));
    rejected.sync_instrument_analysis();
    assert!(rejected.execute("instrument.acceptFit", &json!({"track":1,"device":0,"job":proposal["job"]})).is_err());
    assert_eq!(rejected.project(), &invalid);
    assert_eq!(rejected.instrument_analysis_view().unwrap().phase, AnalysisPhase::Ready);

    let cancelled = run(&mut session, "instrument.fitSample", json!({}));
    run(&mut session, "instrument.cancelAnalysis", json!({"job":cancelled["job"]}));
    session.sync_instrument_analysis();
    assert_eq!(session.instrument_analysis_view().unwrap().phase, AnalysisPhase::Cancelled);
    assert_eq!(session.project(), &fitted);
    let stale = run(&mut session, "instrument.fitSample", json!({}));
    run(
        &mut session,
        "instrument.adoptModalProfile",
        json!({"profile":{"referenceKey":60,"referenceHz":261.6255653,
        "modes":[{"ratio":1.0,"magnitude":0.5,"decaySeconds":1.0}]}}),
    );
    session.sync_instrument_analysis();
    assert_eq!(session.instrument_analysis_view().unwrap().phase, AnalysisPhase::Stale);
    assert!(session.execute("instrument.acceptFit", &json!({"track":1,"device":0,"job":stale["job"]})).is_err());
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &fitted);

    let plugin_library = suhina_plugins::library::Library::new(Some(folder.join("plugins")), folder.join("no-host"));
    let mut projects = Projects::with_library(plugin_library);
    projects.set_file_library(suhina_library::Library::new(Some(folder.join("settings"))));
    projects.add(session, None).unwrap();
    let song = folder.join("fitted.suhina");
    projects.execute("project.save", &json!({"path":song})).unwrap();
    let (mut reopened, warnings) = projects.open_path(&song, OpenOptions { audio: true }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(render(reopened.project(), reopened.audio()), model_sound);
    assert!(reopened.instrument_analysis_view().is_none());
}
