// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Track names with the track's number in them and names that follow the instrument (#946).
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_session::{Session, blank_project};

use crate::support::run;

fn shown(session: &Session) -> Vec<String> {
    let p = session.project();
    p.tracks.iter().map(|t| p.shown_track_name(t.id)).collect()
}

#[test]
fn new_tracks_read_their_number_and_follow_the_tracks_order() {
    let mut session = Session::new(blank_project());
    assert_eq!(shown(&session)[..4], ["1 MIDI", "2 MIDI", "3 Audio", "4 Audio"]);
    run(&mut session, "track.rename", json!({ "track": "2 MIDI", "name": "# Bass" })).unwrap();
    run(&mut session, "track.move", json!({ "track": "2 Bass", "to": 0 })).unwrap();
    assert_eq!(shown(&session)[..2], ["1 Bass", "2 MIDI"]);
    run(&mut session, "track.rename", json!({ "track": "1 Bass", "name": "5 Bass" })).unwrap();
    run(&mut session, "track.move", json!({ "track": "5 Bass", "to": 2 })).unwrap();
    assert!(shown(&session).contains(&"5 Bass".to_string()), "{:?}", shown(&session));
}

#[test]
fn a_track_never_renamed_takes_its_instruments_name_until_the_user_names_it() {
    let mut session = Session::new(blank_project());
    run(&mut session, "device.add", json!({ "track": "1 MIDI", "id": "fm-synth" })).unwrap();
    let name = |s: &Session| s.project().tracks.first().map(|t| t.name.clone()).unwrap_or_default();
    assert_eq!(name(&session), "# FM Synth", "named after it, with the track's number");
    assert_eq!(shown(&session)[0], "1 FM Synth");
    // Undo takes the instrument and the name back in one step.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(name(&session), "# MIDI");
    run(&mut session, "edit.redo", json!({})).unwrap();
    // Removing the instrument brings the new tracks' name back.
    run(&mut session, "device.remove", json!({ "track": "1 FM Synth", "device": 0 })).unwrap();
    assert_eq!(name(&session), "# MIDI");
    // A name the user chose stays.
    run(&mut session, "track.rename", json!({ "track": "1 MIDI", "name": "Lead" })).unwrap();
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "fm-synth" })).unwrap();
    assert_eq!(name(&session), "Lead");
}

#[test]
fn several_empty_tracks_are_added_at_once_in_one_undo_step() {
    let mut session = Session::new(blank_project());
    let added = run(&mut session, "track.add", json!({ "kind": "audio", "count": 3, "after": "1 MIDI" })).unwrap();
    assert_eq!(added["tracks"].as_array().map(Vec::len), Some(3));
    assert_eq!(shown(&session)[..5], ["1 MIDI", "2 Audio", "3 Audio", "4 Audio", "5 MIDI"]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(shown(&session)[..2], ["1 MIDI", "2 MIDI"]);
    assert!(run(&mut session, "track.add", json!({ "kind": "audio", "count": 65 })).is_err());
    assert!(run(&mut session, "track.add", json!({ "device": "fm-synth", "count": 2 })).is_err());
}

/// An empty track takes the kind of what lands on it, and a track that holds clips keeps its
/// own (#945).
#[test]
fn an_empty_track_takes_the_kind_of_the_clip_moved_onto_it() {
    let mut session = Session::new(suhina_test_support::with_ids(blank_project()));
    // A MIDI clip on 1 MIDI; 3 Audio is empty.
    run(&mut session, "clip.create", json!({ "track": "1 MIDI", "start": 0.0, "length": 4.0 })).unwrap();
    let midi_clip = session.project().arrangement.lanes.iter().find_map(|l| l.clips.first()).map(|c| c.id.0).unwrap();
    run(&mut session, "clip.move", json!({ "clip": midi_clip, "start": 0.0, "toTrack": "3 Audio" })).unwrap();
    let p = session.project();
    let third = &p.tracks[2];
    assert!(third.content.notes && !third.content.audio, "3 Audio became a MIDI track");
    assert_eq!(p.shown_track_name(third.id), "3 MIDI");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().tracks[2].content.audio, "undo makes it an audio track again");
    // track.setKind: refused on a track with clips, done on an empty one, its instrument dropped.
    assert!(run(&mut session, "track.setKind", json!({ "track": "1 MIDI", "kind": "audio" })).is_err());
    run(&mut session, "device.add", json!({ "track": "2 MIDI", "id": "fm-synth" })).unwrap();
    run(&mut session, "device.add", json!({ "track": "2 FM Synth", "id": "utility" })).unwrap();
    run(&mut session, "track.setKind", json!({ "track": "2 FM Synth", "kind": "audio" })).unwrap();
    let p = session.project();
    let second = &p.tracks[1];
    assert!(second.content.audio);
    let devices: Vec<&str> = p.channel_of_track(second.id).unwrap().devices.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(devices, ["Utility"], "the instrument goes, the effect stays");
    assert_eq!(p.shown_track_name(second.id), "2 Audio", "and its name with it");
}
