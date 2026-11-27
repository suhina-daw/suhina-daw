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
    chord_notes_lifecycle();
    phrase_notes_lifecycle();
    recording_engine_lifecycle();
    continuation_lifecycle();
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

    // Files from anywhere run the same job: two of the notes, from their own paths.
    assert!(
        session
            .execute("instrument.fitFiles", &json!({"track":1,"device":0,"paths":[notes.join("readme.txt")]}))
            .is_err(),
        "only audio files"
    );
    let files =
        run(&mut session, "instrument.fitFiles", json!({"paths":[notes.join("c5.wav"), notes.join("c4-hard.wav")]}));
    assert_eq!(files["files"], 2);
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    let names: Vec<_> =
        status["folder"]["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, vec!["c4-hard.wav", "c5.wav"], "{status}");
    assert_eq!(status["folder"]["recordings"], 2);

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

/// A chord fits as its notes too: `notes` adopts one recording per key, a single note's fit and a
/// folder's refuse it.
fn chord_notes_lifecycle() {
    let folder = super::temp_dir::TempDir::new("instrument-chord-fit");
    let path = folder.join("authored-chord.wav");
    let (a, c_sharp, e) = (note(57, 1.0), note(61, 0.8), note(64, 0.7));
    let chord: Vec<f32> = a.iter().zip(&c_sharp).zip(&e).map(|((a, b), c)| (a + b + c) * 0.5).collect();
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Float32, &chord);
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    run(&mut session, "device.loadSample", json!({"path":path}));
    run(&mut session, "device.setParam", json!({"param":"root","value":57}));
    let job = run(&mut session, "instrument.fitSample", json!({}));
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    let keys: Vec<u64> =
        status["modal"]["notes"].as_array().unwrap().iter().map(|n| n["key"].as_u64().unwrap()).collect();
    assert_eq!(keys, [57, 61, 64], "{status}");
    let depth = session.undo_steps();
    let accepted = run(&mut session, "instrument.acceptFit", json!({"job":job["job"],"notes":true}));
    assert_eq!(accepted["recordings"], 3);
    assert_eq!(accepted["notes"], json!([57, 61, 64]));
    assert_eq!(session.undo_steps().0, depth.0 + 1);

    // One note has no chord to split.
    let single = folder.join("authored-note.wav");
    suhina_test_support::write_wav(&single, 48000, 1, suhina_test_support::WavFormat::Float32, &note(57, 1.0));
    run(&mut session, "device.loadSample", json!({"path":single}));
    let job = run(&mut session, "instrument.fitSample", json!({}));
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    assert_eq!(status["modal"]["notes"], json!([]), "{status}");
    assert!(
        session.execute("instrument.acceptFit", &json!({"track":1,"device":0,"job":job["job"],"notes":true})).is_err()
    );
}

/// A phrase of three notes in one sample: each held note becomes a recording of one model.
fn phrase_notes_lifecycle() {
    let folder = super::temp_dir::TempDir::new("instrument-phrase-fit");
    let path = folder.join("authored-phrase.wav");
    let length = 38400;
    let phrase: Vec<f32> = [57, 60, 64].iter().flat_map(|key| note(*key, 0.8).into_iter().take(length)).collect();
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Float32, &phrase);
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    run(&mut session, "device.loadSample", json!({"path":path}));
    let before = session.project().clone();
    let job = run(&mut session, "instrument.fitPhrase", json!({}));
    assert_eq!(job["kind"], "folder");
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    assert_eq!(status["status"], "ready", "{status}");
    assert_eq!(status["folder"]["phrase"], true, "{status}");
    let notes: Vec<_> = status["folder"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["key"].as_u64(), f["outcome"].as_str().unwrap()))
        .collect();
    assert_eq!(notes, vec![(Some(57), "kept"), (Some(60), "kept"), (Some(64), "kept")], "{status}");
    assert_eq!(session.project(), &before, "a phrase fit changes nothing until accepted");
    let depth = session.undo_steps();
    let accepted = run(&mut session, "instrument.acceptFit", json!({"job":job["job"]}));
    assert_eq!(accepted["recordings"], 3);
    assert_eq!(session.undo_steps().0, depth.0 + 1);
    // A Sampler has no fitted model to fit a phrase into.
    run(&mut session, "device.add", json!({"id":"sampler"}));
    assert!(session.execute("instrument.fitPhrase", &json!({"track":1,"device":1})).is_err());
}

/// A held tone's continuation: fitted on the loop region without changing the project, accepted
/// in one undo step with Sample playback set to Continue, and taken as another Instrument's
/// morph target.
fn continuation_lifecycle() {
    let folder = super::temp_dir::TempDir::new("instrument-continuation");
    let path = folder.join("authored-held-vibrato-tone.wav");
    let rate = 16_000.0;
    let mut phase = 0.0_f64;
    let tone: Vec<f32> = (0..32_000)
        .map(|i| {
            let t = i as f64 / rate;
            phase += std::f64::consts::TAU * 220.0 * 2f64.powf(20.0 * (std::f64::consts::TAU * 5.5 * t).sin() / 1200.0)
                / rate;
            ((1..=8).map(|k| (phase * k as f64).sin() / k as f64).sum::<f64>() * 0.2) as f32
        })
        .collect();
    suhina_test_support::write_wav(&path, 16_000, 1, suhina_test_support::WavFormat::Float32, &tone);
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    run(&mut session, "device.loadSample", json!({"path":path}));
    for (param, value) in [("root", 57.0), ("loop_start", 0.25), ("loop_end", 0.9)] {
        run(&mut session, "device.setParam", json!({"param":param,"value":value}));
    }
    let before = session.project().clone();
    let job = run(&mut session, "instrument.fitContinuation", json!({}));
    assert_eq!(job["kind"], "continuation");
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    assert_eq!(status["status"], "ready", "{status}");
    let f0 = status["continuation"]["f0"].as_f64().unwrap();
    assert!((f0 - 220.0).abs() < 3.0, "{status}");
    assert_eq!(session.project(), &before, "a continuation fit changes nothing until accepted");
    let depth = session.undo_steps();
    let accepted = run(&mut session, "instrument.acceptContinuation", json!({"job":job["job"]}));
    assert_eq!(accepted["playback"], "continue", "{accepted}");
    assert_eq!(session.undo_steps().0, depth.0 + 1);
    let device = &session.project().channels[0].devices[0];
    let bytes = device.instrument_continuation().unwrap().to_vec();
    let continuation = suhina_dsp::continuation::Continuation::decode(&bytes).unwrap();
    assert!(continuation.join > 1.0 && continuation.join <= 1.8, "{}", continuation.join);
    assert_eq!(device.params.iter().find(|p| p.id == "playback").map(|p| p.value), Some(4.0));
    // A second Instrument morphs toward the first one's continuation, wherever the chain put it.
    run(&mut session, "device.add", json!({"id":"instrument"}));
    let devices = &session.project().channels[0].devices;
    let fitted = devices.iter().position(|d| d.instrument_continuation().is_some()).unwrap();
    let other = devices.iter().position(|d| d.instrument_continuation().is_none()).unwrap();
    run(&mut session, "instrument.setContinuationMorph", json!({"device":other,"fromTrack":1,"fromDevice":fitted}));
    let morph = session.project().channels[0].devices[other].instrument_continuation_morph();
    assert_eq!(morph, Some(bytes.as_slice()));
    run(&mut session, "instrument.setContinuationMorph", json!({"device":other,"clear":true}));
    assert!(session.project().channels[0].devices[other].instrument_continuation_morph().is_none());
    // An Instrument without a continuation is no morph target.
    let refused = session.execute(
        "instrument.setContinuationMorph",
        &json!({"track":1,"device":fitted,"fromTrack":1,"fromDevice":other}),
    );
    assert!(refused.is_err());
    // Undo (the clear, the morph, the second Instrument, the accept) takes the continuation away.
    for _ in 0..4 {
        session.execute("edit.undo", &json!({})).unwrap();
    }
    assert!(session.project().channels[0].devices.iter().all(|d| d.instrument_continuation().is_none()));
}

/// A held tone's fit chooses the recording engine: accepting it plays the Sample source with
/// Sustain playback over the held part, in the same undo step, the fitted model kept.
fn recording_engine_lifecycle() {
    let folder = super::temp_dir::TempDir::new("instrument-recording-engine");
    let path = folder.join("authored-held-tone.wav");
    let hz = 440.0 * 2.0_f64.powf((60.0 - 69.0) / 12.0);
    let tone: Vec<f32> = (0..96_000)
        .map(|i| {
            let t = i as f64 / 48000.0;
            ((1..=6).map(|k| (std::f64::consts::TAU * hz * k as f64 * t).sin() / k as f64).sum::<f64>() * 0.2) as f32
        })
        .collect();
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Float32, &tone);
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    run(&mut session, "device.loadSample", json!({"path":path}));
    run(&mut session, "device.setParam", json!({"param":"root","value":60}));
    let job = run(&mut session, "instrument.fitSample", json!({}));
    session.sync_instrument_analysis();
    let status = run(&mut session, "instrument.analysis", json!({}));
    assert_eq!(status["modal"]["engine"], "recording", "{status}");
    let depth = session.undo_steps();
    let accepted = run(&mut session, "instrument.acceptFit", json!({"job":job["job"]}));
    assert_eq!(accepted["engine"], "recording", "{accepted}");
    assert_eq!(session.undo_steps().0, depth.0 + 1);
    let device = &session.project().channels[0].devices[0];
    let value = |id: &str| device.params.iter().find(|p| p.id == id).map(|p| p.value);
    assert_eq!(value("source"), Some(1.0));
    assert_eq!(value("playback"), Some(3.0));
    let (low, high) = (value("loop_start").unwrap(), value("loop_end").unwrap());
    assert!(low >= 0.12 && high > low + 0.1 && high <= 1.0, "{low} {high}");
    assert!(device.instrument_modal_profile().is_some(), "the fitted model stays for Source Modal");
    assert!(accepted.get("attack").is_none(), "a note from the recording's start keeps its Attack: {accepted}");

    // G3 for half a second, then C4 held: the note plays from C4, the middle of a sounding
    // tone, so it comes in over at least 0.1 s.
    let path = folder.join("authored-step-into-held-tone.wav");
    let mut phase = 0.0_f64;
    let tone: Vec<f32> = (0..96_000)
        .map(|i| {
            let note = if i < 24_000 { 55.0 } else { 60.0 };
            phase += std::f64::consts::TAU * 440.0 * 2.0_f64.powf((note - 69.0) / 12.0) / 48000.0;
            ((1..=6).map(|k| (phase * k as f64).sin() / k as f64).sum::<f64>() * 0.2) as f32
        })
        .collect();
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Float32, &tone);
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({"id":"instrument"}));
    run(&mut session, "device.loadSample", json!({"path":path}));
    run(&mut session, "device.setParam", json!({"param":"root","value":60}));
    let job = run(&mut session, "instrument.fitSample", json!({}));
    session.sync_instrument_analysis();
    let accepted = run(&mut session, "instrument.acceptFit", json!({"job":job["job"]}));
    assert_eq!(accepted["engine"], "recording", "{accepted}");
    let start = accepted["start"].as_f64().unwrap();
    assert!((0.24..0.5).contains(&start), "{accepted}");
    assert_eq!(accepted["attack"], 0.1, "{accepted}");
    let device = &session.project().channels[0].devices[0];
    let attack = device.params.iter().find(|p| p.id == "attack").map(|p| p.value);
    assert_eq!(attack, Some(0.1));
}
