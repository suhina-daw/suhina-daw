// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whole tracks cut, copied, pasted and duplicated (#919).
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;
use suhina_test_support::ProjectBuilder;

/// Bass with a clip on the arrangement and one in its first slot, and Lead after it.
fn session() -> Session {
    let project = ProjectBuilder::new()
        .midi_track(1, "Bass")
        .midi_track(2, "Lead")
        .notes(1, "Riff", 4.0, vec![Note::new(0.0, 1.0, 36, 1.0)])
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .slot(1, 0, Clip::new(ContentId(1), 0.0, 4.0))
        .build();
    Session::new(project)
}

fn names(session: &Session) -> Vec<String> {
    session.project().tracks.iter().map(|t| t.name.clone()).collect()
}

fn track(session: &Session, name: &str) -> Track {
    session.project().tracks.iter().find(|t| t.name == name).cloned().unwrap()
}

fn channel<'a>(session: &'a Session, name: &str) -> &'a Channel {
    let id = track(session, name).channel.unwrap();
    session.project().channels.iter().find(|c| c.id == id).unwrap()
}

/// The content ids of the clips on `name`'s arrangement lane and in its slots.
fn contents(session: &Session, name: &str) -> Vec<ContentId> {
    let id = track(session, name).id;
    let p = session.project();
    let lane = p.arrangement.lanes.iter().filter(|l| l.track == Some(id)).flat_map(|l| &l.clips);
    let slots = p
        .launcher
        .columns
        .iter()
        .filter(|c| c.track == id)
        .flat_map(|c| c.slots.iter().filter_map(|s| s.clip.as_ref()));
    lane.chain(slots).map(|c| c.content).collect()
}

#[test]
fn a_duplicated_track_comes_right_after_with_its_devices_sends_and_clips_of_its_own() {
    let mut session = session();
    run(&mut session, "device.add", json!({ "track": "Bass", "id": "utility" })).unwrap();
    run(&mut session, "return.add", json!({})).unwrap();
    run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "A Return", "level": -6 })).unwrap();
    let added = run(&mut session, "track.duplicate", json!({ "tracks": ["Bass"] })).unwrap();
    assert_eq!(added["tracks"].as_array().unwrap().len(), 1);
    assert_eq!(names(&session)[..3], ["Bass", "Bass (2)", "Lead"]);
    let (original, copy) = (channel(&session, "Bass").clone(), channel(&session, "Bass (2)").clone());
    assert_ne!(original.id, copy.id);
    assert_eq!(copy.devices.len(), 1);
    assert_eq!(copy.sends, original.sends);
    assert_eq!(copy.output, original.output);
    // The copy's clips play contents of their own, with the same notes.
    let (old, new) = (contents(&session, "Bass"), contents(&session, "Bass (2)"));
    assert_eq!(new.len(), 2);
    assert!(new.iter().all(|c| !old.contains(c)), "{old:?} {new:?}");
    let p = session.project();
    let notes = |id: ContentId| p.contents.iter().find(|c| c.id == id).map(|c| c.lanes.clone());
    assert_eq!(notes(new[0]), notes(old[0]));
    // Every clip has an id, none shared.
    let mut ids: Vec<ClipId> = p.all_clips().map(|c| c.id).collect();
    assert!(ids.iter().all(|id| id.is_some()));
    ids.sort_unstable();
    let count = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

#[test]
fn selected_tracks_are_duplicated_as_one_block_after_the_last_of_them() {
    let project = ProjectBuilder::new()
        .midi_track(1, "Bass")
        .midi_track(2, "Lead")
        .midi_track(3, "Pad")
        .midi_track(4, "Keys")
        .build();
    let mut session = Session::new(project);
    let added = run(&mut session, "track.duplicate", json!({ "tracks": ["Bass", "Keys"] })).unwrap();
    assert_eq!(names(&session)[..6], ["Bass", "Lead", "Pad", "Keys", "Bass (2)", "Keys (2)"]);
    // The copies, in order, so the window selects them.
    let added: Vec<u64> = added["tracks"].as_array().unwrap().iter().filter_map(serde_json::Value::as_u64).collect();
    let copies: Vec<u64> = ["Bass (2)", "Keys (2)"].iter().map(|n| u64::from(track(&session, n).id.0)).collect();
    assert_eq!(added, copies);
    // A copy of a copy takes the next number, in the suffix asked for.
    run(&mut session, "track.duplicate", json!({ "tracks": ["Bass (2)"], "suffix": "hash" })).unwrap();
    assert!(names(&session).contains(&"Bass #3".to_string()), "{:?}", names(&session));
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(&session)[..4], ["Bass", "Lead", "Pad", "Keys"]);
    assert!(run(&mut session, "track.duplicate", json!({ "tracks": ["Bass"], "suffix": "dots" })).is_err());
}

#[test]
fn a_name_with_the_track_number_is_copied_as_it_is_and_an_old_default_takes_it() {
    let project = ProjectBuilder::new().midi_track(1, "# Bass").midi_track(2, "MIDI 3").build();
    let mut session = Session::new(project);
    run(&mut session, "track.duplicate", json!({ "tracks": ["# Bass", "MIDI 3"] })).unwrap();
    assert_eq!(names(&session)[..4], ["# Bass", "MIDI 3", "# Bass", "# MIDI"]);
    let p = session.project();
    assert_eq!(p.shown_track_name(p.tracks[2].id), "3 Bass");
    // A new track is named with its number.
    run(&mut session, "track.add", json!({ "kind": "midi" })).unwrap();
    let p = session.project();
    let last = p.tracks.iter().rev().find(|t| t.name.contains("MIDI")).unwrap();
    assert_eq!(last.name, "# MIDI");
    assert_eq!(p.shown_track_name(last.id), format!("{} MIDI", p.track_number(last.id).unwrap()));
}

#[test]
fn cut_and_paste_move_a_track_and_undo_puts_it_back() {
    let mut session = session();
    run(&mut session, "track.cut", json!({ "tracks": ["Bass"] })).unwrap();
    assert_eq!(names(&session)[..1], ["Lead"]);
    assert_eq!(session.copied_tracks(), 1);
    run(&mut session, "track.paste", json!({ "after": "Lead" })).unwrap();
    assert_eq!(names(&session)[..2], ["Lead", "Bass"]);
    assert_eq!(contents(&session, "Bass").len(), 2);
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(&session)[..2], ["Bass", "Lead"]);
}

#[test]
fn copying_a_group_copies_its_tracks_into_the_copy() {
    let mut session = session();
    run(&mut session, "track.group", json!({ "tracks": ["Bass", "Lead"], "name": "Band" })).unwrap();
    run(&mut session, "track.copy", json!({ "tracks": ["Band"] })).unwrap();
    assert_eq!(session.copied_tracks(), 3);
    run(&mut session, "track.paste", json!({})).unwrap();
    let copy = track(&session, "Band (2)");
    let inside: Vec<Track> = session.project().tracks.iter().filter(|t| t.parent == Some(copy.id)).cloned().collect();
    assert_eq!(inside.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Bass (2)", "Lead (2)"]);
    // They play into the copied group, not the original.
    for t in &inside {
        assert_eq!(channel(&session, &t.name).output, copy.channel);
    }
    assert_eq!(names(&session).iter().filter(|n| n.as_str() == "Bass").count(), 1);
}

#[test]
fn returns_are_not_copied_and_an_empty_clipboard_does_not_paste() {
    let mut session = session();
    assert!(run(&mut session, "track.paste", json!({})).is_err());
    run(&mut session, "return.add", json!({})).unwrap();
    assert!(run(&mut session, "track.copy", json!({ "tracks": ["A Return"] })).is_err());
}

#[test]
fn copied_tracks_paste_into_another_open_project() {
    let mut projects = suhina_session::Projects::new();
    let first = projects.add(session(), None).unwrap();
    let second = projects.add(Session::new(ProjectBuilder::new().midi_track(1, "Keys").build()), None).unwrap();
    projects.execute_in(Some(first), "track.copy", &json!({ "tracks": ["Bass"] })).unwrap();
    projects.execute_in(Some(second), "track.paste", &json!({})).unwrap();
    let tracks = projects.execute_in(Some(second), "project.tracks", &json!({})).unwrap();
    let listed: Vec<&str> = tracks.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(listed.contains(&"Bass"), "{listed:?}");
}

/// Clips copied in one tab paste onto a track of another, with ids of that project (#919).
#[test]
fn copied_clips_paste_into_another_open_project() {
    let mut projects = suhina_session::Projects::new();
    let first = projects.add(session(), None).unwrap();
    let second = projects.add(Session::new(ProjectBuilder::new().midi_track(1, "Keys").build()), None).unwrap();
    projects.execute_in(Some(first), "clip.copy", &json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    let pasted = projects.execute_in(Some(second), "clip.paste", &json!({ "track": "Keys", "start": 8.0 })).unwrap();
    assert_eq!(pasted["clips"].as_array().map(Vec::len), Some(1), "{pasted}");
    let clips = projects.execute_in(Some(second), "project.clips", &json!({})).unwrap();
    assert_eq!(clips.as_array().map(Vec::len), Some(1), "{clips}");
    // The source tracks are the first project's: a paste without a track asks for one.
    assert!(projects.execute_in(Some(second), "clip.paste", &json!({ "start": 0.0 })).is_err());
}
