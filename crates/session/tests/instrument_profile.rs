// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Accepted modal data travels through the actual command, project file and stock renderer.
#![allow(clippy::unwrap_used)] // deterministic test helpers

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session};

fn session() -> Session {
    let mut device = Device::new("Instrument", DeviceKind::Builtin("instrument".into()));
    device.params = vec![DeviceParam::new("model_revision", 1.0), DeviceParam::new("source", 1.0)];
    device.set_sample_reference("absent-source.wav", "C:/absent-source.wav");
    let mut channel = Channel::new(ChannelId(1), "Instrument");
    channel.devices.push(device);
    Session::new(Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Instrument")
        }],
        channels: vec![channel],
        ..Project::default()
    })
}

fn profile() -> Value {
    json!({"referenceKey":60,"referenceHz":263.1,"modes":[
        {"ratio":1.0,"magnitude":0.7,"decaySeconds":1.2},
        {"ratio":2.756,"magnitude":0.3,"decaySeconds":0.35}
    ]})
}

fn adopt(session: &mut Session, profile: Value) -> Value {
    session.execute("instrument.adoptModalProfile", &json!({"track":1,"device":0,"profile":profile})).unwrap()
}

fn render(project: &Project, playing: bool) -> Vec<f32> {
    let audio = AudioPool::default();
    let mut renderer = Renderer::stopped(project, &audio, 48000, suhina_session::devices()).unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    let mut session = Session::new(project.clone());
    session.attach_player(Player::new(controller, None));
    session.execute("track.setArm", &json!({"track":1,"armed":true})).unwrap();
    if playing {
        session.execute("transport.play", &json!({})).unwrap();
    }
    live.send(suhina_engine::LiveEvent::NoteOn { key: 60, velocity: 0.8 });
    let mut output = vec![0.0; 8192];
    for block in output.chunks_mut(256) {
        renderer.process(block);
    }
    output
}

#[test]
fn adoption_overrides_affected_automation_atomically_and_saves_exact_sound_without_source() {
    let mut session = session();
    for (param, value) in [
        ("source", 1.0),
        ("body_decay", 10.0),
        ("brightness", 0.1),
        ("structure", 0.03),
        ("strike", 0.7),
        ("quality", 0.0),
    ] {
        session.execute("device.setParam", &json!({"track":1,"device":0,"param":param,"value":value})).unwrap();
        for beat in [0.0, 0.01] {
            session
                .execute("automation.addPoint", &json!({"track":1,"device":0,"param":param,"beat":beat,"value":value}))
                .unwrap();
        }
    }
    let before = session.project().clone();
    let result = adopt(&mut session, profile());
    assert_eq!(result["automation"], "off");
    assert_eq!(result["modelControlsReset"], true);
    let accepted = session.project().clone();
    let device = &accepted.channels[0].devices[0];
    assert_eq!(device.sample_file(), Some("absent-source.wav"));
    assert_eq!(device.state, before.channels[0].devices[0].state);
    let decoded = suhina_dsp::modal::profile::ModalProfile::decode(device.instrument_modal_profile().unwrap()).unwrap();
    assert_eq!(decoded.reference_hz(), 263.1);
    for (lane, old) in device.automation.iter().zip(&before.channels[0].devices[0].automation) {
        assert!(!lane.enabled);
        assert_eq!(lane.points, old.points);
    }
    let mut reference = accepted.clone();
    reference.channels[0].devices[0].automation.clear();
    for playing in [false, true] {
        let output = render(&accepted, playing);
        assert!(output.iter().any(|value| value.abs() > 0.001));
        assert_eq!(output, render(&reference, playing));
    }
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &before);
    session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(session.project(), &accepted);
    let folder = super::temp_dir::TempDir::new("modal-profile");
    let path = folder.join("profile.suhina");
    suhina_project_file::write_file(&path, &accepted).unwrap();
    let reopened = suhina_project_file::read_file(&path).unwrap();
    assert_eq!(reopened.channels[0].devices[0].instrument_modal_profile(), device.instrument_modal_profile());
    assert_eq!(render(&reopened, false), render(&accepted, false));
}

#[test]
fn malformed_profiles_and_wrong_targets_leave_project_and_history_unchanged() {
    let mut session = session();
    let before = session.project().clone();
    let invalid = [
        json!({}),
        json!({"referenceKey":60,"referenceHz":440,"modes":[]}),
        json!({"referenceKey":128,"referenceHz":440,"modes":profile()["modes"]}),
        json!({"referenceKey":60,"referenceHz":null,"modes":profile()["modes"]}),
        json!({"referenceKey":60,"referenceHz":440,"modes":vec![profile()["modes"][0].clone();17]}),
        json!({"referenceKey":60,"referenceHz":440,"modes":[{"ratio":1,"magnitude":0,"decaySeconds":1}]}),
        json!({"referenceKey":60,"referenceHz":440,"modes":[{"ratio":1,"magnitude":1,"decaySeconds":1,"extra":1}]}),
        json!({"referenceKey":60,"referenceHz":440,"modes":[{"ratio":2,"magnitude":1,"decaySeconds":1},{"ratio":1,"magnitude":1,"decaySeconds":1}]}),
    ];
    for profile in invalid {
        assert!(
            session.execute("instrument.adoptModalProfile", &json!({"track":1,"device":0,"profile":profile})).is_err()
        );
        assert_eq!(session.project(), &before);
    }
    for extra in [
        json!({"track":1,"device":99,"profile":profile()}),
        json!({"track":1,"device":0,"profile":profile(),"source":"Sample"}),
    ] {
        assert!(session.execute("instrument.adoptModalProfile", &extra).is_err());
        assert_eq!(session.project(), &before);
    }
}

#[test]
fn nested_device_adoption_and_distinct_profiles_reach_native_rendering() {
    let mut session = session();
    adopt(&mut session, profile());
    let first = render(session.project(), false);
    session.execute("device.group", &json!({"track":1,"device":0})).unwrap();
    let mut other = profile();
    other["referenceHz"] = json!(337.0);
    session.execute("instrument.adoptModalProfile", &json!({"track":1,"device":"0/0/0","profile":other})).unwrap();
    let nested = device_at(&session.project().channels[0].devices, &[0, 0, 0]).unwrap();
    assert!(nested.instrument_modal_profile().is_some());
    let second = render(session.project(), false);
    assert!(second.iter().any(|value| value.abs() > 0.001));
    assert_ne!(first, second);
}
