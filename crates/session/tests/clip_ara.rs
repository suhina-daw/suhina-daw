// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip's ARA plug-in (#990): `clip.setAra` on a track that has a clip host.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;
use suhina_test_support::ProjectBuilder;

use crate::support::run;

/// "Vox": two audio clips and a MIDI clip, its channel's first device a clip host.
fn session() -> Session {
    let mut project = ProjectBuilder::new()
        .audio_track(1, "Vox")
        .audio(1, "Take", 8.0, AudioRegion::new("take.wav"))
        .notes(2, "Riff", 4.0, vec![Note::new(0.0, 1.0, 60, 1.0)])
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .clip(1, Clip::new(ContentId(1), 4.0, 4.0))
        .clip(1, Clip::new(ContentId(2), 8.0, 4.0))
        .build();
    let channel = project.tracks[0].channel.unwrap();
    let mut host = Device::new("Melodyne", DeviceKind::Vst3 { id: "melodyne".into() });
    host.clip_host = true;
    project.channels.iter_mut().find(|c| c.id == channel).unwrap().devices.insert(0, host);
    Session::new(project)
}

fn flags(session: &Session) -> Vec<bool> {
    session.project().arrangement.lanes[0].clips.iter().map(|c| c.ara).collect()
}

#[test]
fn a_clip_plays_through_the_clip_host_and_back_and_a_midi_clip_is_refused() {
    let mut s = session();
    run(&mut s, "clip.setAra", json!({ "clips": [{ "track": 1, "index": 1 }], "on": true })).unwrap();
    assert_eq!(flags(&s), [false, true, false]);
    run(&mut s, "clip.setAra", json!({ "clips": [{ "track": 1, "index": 1 }], "on": false })).unwrap();
    assert_eq!(flags(&s), [false, false, false]);
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert_eq!(flags(&s), [false, true, false], "one undo step");
    let midi = run(&mut s, "clip.setAra", json!({ "clips": [{ "track": 1, "index": 2 }], "on": true }));
    assert!(midi.is_err());
    assert_eq!(flags(&s), [false, true, false], "nothing changed");
    // A copy keeps it.
    let copied = s.project().arrangement.lanes[0].clips[1].clone();
    assert!(copied.ara);
}

/// A device put before the clip host goes after it: the host stays first, where it plays (#990).
#[test]
fn the_clip_host_stays_first_in_the_chain() {
    let mut s = session();
    run(&mut s, "device.add", json!({ "track": 1, "id": "utility", "position": 0 })).unwrap();
    let channel = s.project().tracks[0].channel.unwrap();
    let devices = &s.project().channels.iter().find(|c| c.id == channel).unwrap().devices;
    assert!(devices[0].clip_host, "{:?}", devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>());
    assert_eq!(devices[1].name, "Utility");
}

/// A track whose ARA plug-in is an insert in its chain (ADR 0033, amendment) gets a clip host of
/// its own when a clip is edited with it: the insert stays where it is, not bound to the clips.
#[test]
fn editing_a_clip_adds_a_clip_host_beside_an_insert_of_the_same_plug_in() {
    use suhina_plugins::scan::{Catalog, PluginInfo};
    let project = ProjectBuilder::new()
        .audio_track(1, "Vox")
        .audio(1, "Take", 8.0, AudioRegion::new("take.wav"))
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .build();
    let mut s = Session::new(project);
    let tune = PluginInfo {
        path: "Tune.vst3".into(),
        id: "00112233445566778899AABBCCDDEEFF".into(),
        name: "Tune".into(),
        vendor: "Vendor".into(),
        version: "1.0".into(),
        features: vec!["Fx|Pitch Shift".into()],
        replaces: Vec::new(),
        ara: true,
    };
    let mut catalog = Catalog::new();
    catalog.insert(tune.path.clone(), vec![tune]);
    s.rack_mut().set_env(std::sync::Arc::new(suhina_session::PluginEnv::new(catalog, "no-such-host".into())));
    run(&mut s, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    run(&mut s, "device.addPlugin", json!({ "track": 1, "plugin": "Tune" })).unwrap();
    assert_eq!(flags(&s), [false], "an insert binds no clip");
    run(&mut s, "clip.setAra", json!({ "clips": [{ "track": 1, "index": 0 }], "on": true, "plugin": "Tune" })).unwrap();
    assert_eq!(flags(&s), [true]);
    let channel = s.project().tracks[0].channel.unwrap();
    let devices = &s.project().channels.iter().find(|c| c.id == channel).unwrap().devices;
    let chain: Vec<(&str, bool)> = devices.iter().map(|d| (d.name.as_str(), d.clip_host)).collect();
    assert_eq!(chain, [("Tune", true), ("Utility", false), ("Tune", false)]);
}

/// **Edit with** on a clip when the scan did not mark the installed Melodyne as ARA: its VST3
/// is added as the clip host all the same (the host binds it when it loads, or says why it
/// cannot), and keeps the mark through a device edit made before any host has answered; a
/// plug-in of another format is refused, and the refusal changes nothing.
#[test]
fn a_vst3_the_scan_did_not_mark_becomes_the_clip_host() {
    use suhina_plugins::scan::{Catalog, PluginInfo};
    let project = ProjectBuilder::new()
        .audio_track(1, "Vox")
        .audio(1, "Take", 8.0, AudioRegion::new("take.wav"))
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .build();
    let plugin = |path: &str, id: &str, name: &str| PluginInfo {
        path: path.into(),
        id: id.into(),
        name: name.into(),
        vendor: "Vendor".into(),
        version: "1.0".into(),
        features: vec!["Fx".into()],
        replaces: Vec::new(),
        ara: false,
    };
    let mut catalog = Catalog::new();
    for info in [
        plugin("Melodyne.vst3", "00112233445566778899AABBCCDDEEFF", "Melodyne"),
        plugin("Verb.clap", "com.vendor.verb", "Verb"),
    ] {
        catalog.insert(info.path.clone(), vec![info]);
    }
    let mut s = Session::new(project);
    s.rack_mut().set_env(std::sync::Arc::new(suhina_session::PluginEnv::new(catalog, "no-such-host".into())));
    let (_, melodyne) = s.ara_plugin().unwrap();
    let refused =
        run(&mut s, "clip.setAra", json!({ "clips": [{ "track": 1, "index": 0 }], "on": true, "plugin": "Verb" }));
    assert!(refused.unwrap_err().contains("does not edit clips"));
    assert_eq!(flags(&s), [false], "nothing changed");
    run(&mut s, "clip.setAra", json!({ "clips": [{ "track": 1, "index": 0 }], "on": true, "plugin": melodyne }))
        .unwrap();
    assert_eq!(flags(&s), [true]);
    let channel = s.project().tracks[0].channel.unwrap();
    let devices = &s.project().channels.iter().find(|c| c.id == channel).unwrap().devices;
    assert!(devices[0].clip_host && devices[0].name == "Melodyne");
    // No host has answered (there is none here): a device edit keeps it the clip host.
    run(&mut s, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    let devices = &s.project().channels.iter().find(|c| c.id == channel).unwrap().devices;
    assert!(devices[0].clip_host && devices[0].name == "Melodyne", "the mark stays");
    assert_eq!(flags(&s), [true], "the clip still plays through it");
}
