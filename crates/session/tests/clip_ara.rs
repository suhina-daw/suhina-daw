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
