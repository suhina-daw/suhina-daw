// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clips' colours and names as their track's (`docs/spec/arrangement-editing.md` › Clip colours).
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside test functions

#[test]
fn copied_and_consolidated_clips_do_not_take_creation_defaults() {
    let mut session = Session::new(project());
    let source = clips(&session, 1)[0].id;
    run(&mut session, "clip.copy", json!({"clip":source.0})).unwrap();
    run(&mut session, "clip.paste", json!({"track":1,"start":16.0})).unwrap();
    assert!(clips(&session, 1).iter().all(|clip| clip.color.is_none()));
    run(&mut session, "clip.consolidate", json!({"track":1,"start":16.0,"end":20.0})).unwrap();
    assert!(clips(&session, 1).iter().all(|clip| clip.color.is_none()));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(clips(&session, 1).iter().all(|clip| clip.color.is_none()));
}

use serde_json::json;
use suhina_model::*;
use suhina_session::{Session, blank_project, colours};
use suhina_test_support::ProjectBuilder;

use crate::support::run;

/// The blank project with two notes clips on track 1 and none on track 2.
fn project() -> Project {
    ProjectBuilder::from(blank_project())
        .notes(1, "Clip 1", 4.0, Vec::new())
        .notes(2, "Clip 2", 4.0, Vec::new())
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .clip(1, Clip::new(ContentId(2), 8.0, 4.0))
        .build_without_ids()
}

fn clips(session: &Session, track: u32) -> Vec<Clip> {
    let p = session.project();
    p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(track))).unwrap().clips.clone()
}

#[test]
fn paste_replaces_only_the_named_destination_and_undo_restores_it() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let target = clips(&session, 1)[1].id;
    let source = clips(&session, 1)[0].id;
    run(&mut session, "clip.copy", json!({"clip":source.0})).unwrap();
    run(&mut session, "clip.paste", json!({"track":1,"start":8.0,"replace":target.0,"stack":true})).unwrap();
    let after = clips(&session, 1);
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|clip| clip.id != target));
    assert_eq!(after[1].start, 8.0);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().arrangement, before.arrangement);
    assert_eq!(session.project().contents, before.contents);
    assert!(run(&mut session, "clip.paste", json!({"track":2,"start":8.0,"replace":target.0})).is_err());
    assert_eq!(session.project().arrangement, before.arrangement);
    assert_eq!(session.project().contents, before.contents);
}

#[test]
fn paste_replacement_accepts_names_and_rejects_ambiguous_or_wrong_track_targets() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let source = clips(&session, 1)[0].id;
    let target = clips(&session, 1)[1].id;
    run(&mut session, "clip.copy", json!({"clip":source.0})).unwrap();
    run(&mut session, "clip.paste", json!({"track":1,"start":8.0,"replace":"clip 2","stack":true})).unwrap();
    assert!(clips(&session, 1).iter().all(|clip| clip.id != target));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().arrangement, before.arrangement);
    for params in [
        json!({"track":2,"start":8.0,"replace":"Clip 2"}),
        json!({"track":1,"start":8.0,"replace":"missing"}),
        json!({"track":1,"scene":0,"replace":"Clip 2"}),
        json!({"track":1,"start":8.0,"replace":4294967296u64}),
    ] {
        assert!(run(&mut session, "clip.paste", params).is_err());
        assert_eq!(session.project().arrangement, before.arrangement);
        assert_eq!(session.project().contents, before.contents);
    }
    run(&mut session, "clip.paste", json!({"track":1,"start":16.0,"stack":true})).unwrap();
    let before_ambiguous = session.project().clone();
    assert!(run(&mut session, "clip.paste", json!({"track":1,"start":8.0,"replace":"Clip 1"})).is_err());
    assert_eq!(session.project().arrangement, before_ambiguous.arrangement);
    assert_eq!(session.project().contents, before_ambiguous.contents);
}

#[test]
fn a_new_clip_shows_its_tracks_colour_and_a_moved_one_keeps_what_it_showed() {
    let mut session = Session::new(project());
    assert!(clips(&session, 1).iter().all(|c| c.color.is_none()), "a new clip follows its track");
    // Moved to track 2, it keeps the colour it showed on track 1.
    run(&mut session, "clip.move", json!({ "track": 1, "index": 0, "start": 0.0, "toTrack": 2, "color": "#3366CC" }))
        .unwrap();
    assert_eq!(clips(&session, 2)[0].color, Some(Rgb(0x33, 0x66, 0xCC)));
    // Moved along its own track, the colour is not taken.
    run(&mut session, "clip.move", json!({ "track": 1, "index": 0, "start": 16.0, "color": "#3366CC" })).unwrap();
    assert!(clips(&session, 1)[0].color.is_none());
}

#[test]
fn a_tracks_clips_take_its_colour_and_name_again() {
    let mut session = Session::new(project());
    run(&mut session, "clip.setColor", json!({ "track": 1, "index": 1, "color": "#FF0000" })).unwrap();
    assert_eq!(clips(&session, 1)[1].color, Some(Rgb(255, 0, 0)));
    // The track menu's "Assign track color to all clips".
    run(&mut session, "clip.setColor", json!({ "track": 1, "color": "none" })).unwrap();
    assert!(clips(&session, 1).iter().all(|c| c.color.is_none()));
    // "Assign track name to clips": the track's name.
    let track = session.project().shown_track_name(TrackId(1));
    run(&mut session, "track.renameClips", json!({ "track": 1 })).unwrap();
    for content in [ContentId(1), ContentId(2)] {
        assert_eq!(session.project().content(content).unwrap().name, track);
    }
    assert!(run(&mut session, "clip.setColor", json!({ "track": 1, "color": "blue" })).is_err());
}

#[test]
fn a_clips_colour_survives_saving() {
    let mut p = project();
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(1))).unwrap().clips[0].color = Some(Rgb(1, 2, 3));
    let path = std::env::temp_dir().join(format!("suhina-clip-color-{}.suhina", std::process::id()));
    suhina_project_file::write_file(&path, &p).unwrap();
    let read = suhina_project_file::read_file(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let lane = read.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap();
    assert_eq!(lane.clips[0].color, Some(Rgb(1, 2, 3)));
    assert_eq!(lane.clips[1].color, None);
}

#[test]
fn new_tracks_take_suhina_s_colour_for_their_kind_and_keep_it() {
    let mut session = Session::new(blank_project());
    let existing: Vec<_> = session.project().tracks.iter().map(|track| track.id).collect();
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "track.add", json!({ "kind": "midi" })).unwrap();
    for track in session.project().tracks.iter().filter(|track| !existing.contains(&track.id)) {
        let audio = track.content.audio && !track.content.notes;
        assert_eq!(track.color, Some(if audio { colours::AUDIO } else { colours::MIDI }));
    }
    let before = session.project().tracks.iter().map(|t| (t.id, t.color)).collect::<Vec<_>>();
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    assert!(
        before.iter().all(|(id, colour)| session.project().tracks.iter().any(|t| t.id == *id && t.color == *colour))
    );
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().tracks.iter().map(|t| (t.id, t.color)).collect::<Vec<_>>(), before);
}

#[test]
fn a_session_without_a_window_colours_what_it_creates() {
    // The CLI and MCP run commands on a session no window has set up.
    let mut session = Session::new(project());
    session.set_new_clips_take_track_colour(false);
    run(&mut session, "clip.create", json!({"track":1,"start":16.0,"length":4.0})).unwrap();
    assert_eq!(clips(&session, 1)[2].color, Some(colours::MIDI));
    let tracks = session.project().tracks.clone();
    run(&mut session, "track.add", json!({"kind":"audio"})).unwrap();
    let added = session.project().tracks.iter().find(|track| !tracks.iter().any(|old| old.id == track.id));
    assert_eq!(added.unwrap().color, Some(colours::AUDIO));
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(clips(&session, 1).len(), 2);
    assert_eq!(session.project().tracks, tracks);
}

#[test]
fn tracks_keep_the_colour_they_show_when_tracks_come_and_go() {
    // Tracks saved without a colour of their own, as in a project from before tracks stored one.
    let mut session = Session::new(blank_project());
    for _ in 0..3 {
        run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    }
    let mut project = session.project().clone();
    for track in &mut project.tracks {
        track.color = None;
    }
    let mut session = Session::new(project);
    let shown = |s: &Session| {
        s.project()
            .tracks
            .iter()
            .enumerate()
            .map(|(place, t)| (t.id, colours::track_colour(t.color, place)))
            .collect::<Vec<_>>()
    };
    let before = shown(&session);
    // A track taken out of the middle: the others keep the colour they showed.
    let middle = before[before.len() / 2].0;
    run(&mut session, "track.delete", json!({ "track": middle.0 })).unwrap();
    let after: Vec<_> = before.iter().filter(|(id, _)| *id != middle).copied().collect();
    assert_eq!(shown(&session), after);
    // A new one added: the same, and the new one keeps its own from then on.
    run(&mut session, "track.add", json!({ "kind": "midi" })).unwrap();
    let now = shown(&session);
    assert!(after.iter().all(|kept| now.contains(kept)), "{after:?} in {now:?}");
    assert!(session.project().tracks.iter().all(|t| t.color.is_some()), "every colour is stored");
    // One undo step each: the delete's undo brings the colours back as they were.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(shown(&session), before);
}

#[test]
fn with_one_colour_new_tracks_all_take_the_midi_colour() {
    let mut session = Session::new(blank_project());
    session.set_one_new_track_colour(true);
    let before: Vec<_> = session.project().tracks.iter().map(|t| t.id).collect();
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "track.add", json!({ "kind": "midi" })).unwrap();
    let p = session.project();
    let new: Vec<_> = p.tracks.iter().filter(|t| !before.contains(&t.id)).map(|t| t.color).collect();
    assert_eq!(new, [Some(colours::MIDI), Some(colours::MIDI)]);
}

#[test]
fn a_new_clip_can_take_its_track_s_colour() {
    let mut session = Session::new(project());
    run(&mut session, "clip.setColor", json!({ "track": 1, "color": "none" })).unwrap();
    run(&mut session, "track.setColor", json!({ "track": 1, "color": "#F08C1E" })).unwrap();
    let track = session.project().tracks.iter().find(|t| t.id == TrackId(1)).unwrap().color;
    assert_eq!(track, Some(Rgb(0xF0, 0x8C, 0x1E)));
    // Taking the track's colour is the default.
    run(&mut session, "clip.create", json!({"track":1,"start":16.0,"length":4.0})).unwrap();
    assert_eq!(clips(&session, 1).last().unwrap().color, track);
    session.set_new_clips_take_track_colour(false);
    run(&mut session, "clip.create", json!({"track":1,"start":24.0,"length":4.0})).unwrap();
    assert_eq!(clips(&session, 1).last().unwrap().color, Some(colours::MIDI));
}

#[test]
fn a_track_saved_without_a_colour_keeps_the_one_it_shows_and_its_copy_takes_it() {
    // A project from before tracks stored their colour.
    let mut project = blank_project();
    for track in &mut project.tracks {
        track.color = None;
    }
    let mut session = Session::new(project);
    assert!(!session.has_unsaved_changes(), "giving the colour is not an edit");
    let first = session.project().tracks[0].clone();
    let second = session.project().tracks[1].color;
    assert_eq!(first.color, Some(colours::LEGACY[0]));
    assert_eq!(second, Some(colours::LEGACY[1]));
    // Its copy shows the colour it shows, not the default for its kind.
    run(&mut session, "track.duplicate", json!({ "tracks": [first.id.0] })).unwrap();
    let copy = session.project().tracks.iter().find(|t| t.id != first.id && t.name.starts_with(&first.name));
    assert_eq!(copy.and_then(|t| t.color), first.color);
    assert_ne!(first.color, Some(colours::MIDI), "the test tells the two apart");
}
