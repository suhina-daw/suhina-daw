// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::demo::demo_project;

fn note(time: f64, duration: f64, key: u8) -> Note {
    Note { time, duration, key, velocity: 0.8, release_velocity: 0.5, channel: 0, muted: false }
}

fn at(start: f64, length: f64, offset: f64, loop_range: Option<(f64, f64)>) -> Placement {
    Placement { start, length, offset, loop_range }
}

/// The ghosts of `notes` laid out at `other` in the first pass of `edited`: (start, length, key).
fn mapped(edited: Placement, other: Placement, notes: &[Note]) -> Vec<(f64, f64, u8)> {
    let mut out = Vec::new();
    if let Some(window) = first_pass(&edited) {
        map_notes(&other, notes, window, &mut out, |s, l, k| (s, l, k));
    }
    out
}

#[test]
fn a_clip_at_the_same_place_maps_note_for_note() {
    let notes = [note(0.0, 1.0, 40), note(2.5, 0.5, 43)];
    let got = mapped(at(8.0, 4.0, 0.0, None), at(8.0, 4.0, 0.0, None), &notes);
    assert_eq!(got, vec![(0.0, 1.0, 40), (2.5, 0.5, 43)]);
}

#[test]
fn a_clip_starting_before_shows_only_what_sounds_inside() {
    // The other clip starts 2 beats earlier: its note at 1 sounds before, the one at 3 lands
    // on the edited clip's beat 1, and a note held across the edited clip's start is cut there.
    let notes = [note(1.0, 0.5, 40), note(3.0, 1.0, 41), note(1.5, 1.0, 42)];
    let got = mapped(at(10.0, 4.0, 0.0, None), at(8.0, 8.0, 0.0, None), &notes);
    assert_eq!(got, vec![(1.0, 1.0, 41), (0.0, 0.5, 42)]);
}

#[test]
fn a_clip_starting_after_lands_later_and_is_cut_at_the_end() {
    let notes = [note(0.0, 1.0, 40), note(1.5, 2.0, 41), note(3.0, 1.0, 42)];
    let got = mapped(at(0.0, 4.0, 0.0, None), at(2.0, 4.0, 0.0, None), &notes);
    // 0 -> 2; 1.5 -> 3.5, cut to half a beat at the edited clip's end; 3 -> 5 is past it.
    assert_eq!(got, vec![(2.0, 1.0, 40), (3.5, 0.5, 41)]);
}

#[test]
fn clips_that_do_not_overlap_show_nothing() {
    let notes = [note(0.0, 1.0, 40)];
    assert!(mapped(at(0.0, 4.0, 0.0, None), at(4.0, 4.0, 0.0, None), &notes).is_empty());
    assert!(mapped(at(8.0, 4.0, 0.0, None), at(0.0, 4.0, 0.0, None), &notes).is_empty());
}

#[test]
fn an_offset_moves_the_content_on_both_sides() {
    // The other clip starts playing its content at beat 2: its note at 2 sounds first. The
    // edited clip's offset 1 puts its first beat in time at content beat 1.
    let notes = [note(0.0, 1.0, 40), note(2.0, 1.0, 41), note(3.0, 1.0, 42)];
    let got = mapped(at(0.0, 4.0, 1.0, None), at(0.0, 4.0, 2.0, None), &notes);
    assert_eq!(got, vec![(1.0, 1.0, 41), (2.0, 1.0, 42)]);
}

#[test]
fn a_looping_clip_repeats_its_loop_through_the_window() {
    // A one-beat loop of one note under an eight-beat edited clip: eight ghosts, a beat apart,
    // each cut at the loop's end.
    let notes = [note(0.0, 2.0, 40), note(1.5, 0.5, 41)];
    let got = mapped(at(0.0, 8.0, 0.0, None), at(0.0, 16.0, 0.0, Some((0.0, 1.0))), &notes);
    assert_eq!(got.len(), 8);
    for (i, ghost) in got.iter().enumerate() {
        assert_eq!(*ghost, (i as f64, 1.0, 40));
    }
}

#[test]
fn a_looping_clip_with_an_offset_plays_its_first_part_once() {
    // Content 0..4 loops from 2 to 4, played from offset 1: beats 1, 2, 3 once, then 2, 3 round.
    let notes = [note(1.0, 0.5, 40), note(2.0, 0.5, 41), note(3.0, 0.5, 42)];
    let got = mapped(at(0.0, 7.0, 0.0, None), at(0.0, 7.0, 1.0, Some((2.0, 4.0))), &notes);
    let keys: Vec<(f64, u8)> = got.iter().map(|g| (g.0, g.2)).collect();
    assert_eq!(keys, vec![(0.0, 40), (1.0, 41), (2.0, 42), (3.0, 41), (4.0, 42), (5.0, 41), (6.0, 42)]);
}

#[test]
fn a_looping_clip_far_before_skips_to_the_passes_inside() {
    let notes = [note(0.5, 0.25, 40)];
    let got = mapped(at(1000.0, 2.0, 0.0, None), at(0.0, 2000.0, 0.0, Some((0.0, 1.0))), &notes);
    assert_eq!(got, vec![(0.5, 0.25, 40), (1.5, 0.25, 40)]);
}

#[test]
fn the_edited_clip_maps_its_first_pass_only() {
    // The edited clip loops 0..4 for 16 beats: only its first 4 beats in time take ghosts.
    let notes: Vec<Note> = (0..16).map(|b| note(f64::from(b), 0.5, 40 + b as u8)).collect();
    let edited = at(0.0, 16.0, 0.0, Some((0.0, 4.0)));
    assert_eq!(first_pass(&edited), Some(Window { start: 0.0, end: 4.0, content: 0.0 }));
    let got = mapped(edited, at(0.0, 16.0, 0.0, None), &notes);
    assert_eq!(got.iter().map(|g| g.2).collect::<Vec<_>>(), vec![40, 41, 42, 43]);
    // Played from offset 3, its first pass is a single beat at content beat 3.
    let edited = at(0.0, 16.0, 3.0, Some((0.0, 4.0)));
    assert_eq!(first_pass(&edited), Some(Window { start: 0.0, end: 1.0, content: 3.0 }));
    assert_eq!(mapped(edited, at(0.0, 16.0, 0.0, None), &notes), vec![(3.0, 0.5, 40)]);
}

#[test]
fn muted_notes_and_a_tiny_loop_never_run_away() {
    let notes = [note(0.0, 1.0, 40), Note { muted: true, ..note(0.0, 1.0, 41) }];
    let got = mapped(at(0.0, 1.0e6, 0.0, None), at(0.0, 1.0e6, 0.0, Some((0.0, 0.001))), &notes);
    assert_eq!(got.len(), MAX_GHOSTS);
    assert!(got.iter().all(|g| g.2 == 40));
    assert!(mapped(at(0.0, f64::NAN, 0.0, None), at(0.0, 4.0, 0.0, None), &notes).is_empty());
}

/// The demo clip at bar 17 on Drums (the second on its lane).
fn beat_b() -> ClipRef {
    arranged(&demo_project(), 1, 1)
}

/// The arrangement clip at `index` on track `track`'s lane.
fn arranged(project: &Project, track: u32, index: usize) -> ClipRef {
    let lane = project.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(track))).unwrap();
    ClipRef::Arrangement(TrackId(track), lane.clips[index].id)
}

#[test]
fn playing_together_shows_the_overlapping_clips_of_other_tracks() {
    let project = demo_project();
    let got = ghosts(&project, beat_b(), GhostSource::Together);
    // Walk on Bass, Chords 1 on Keys (both at 64): a one-bar figure each in the first pass.
    assert_eq!(got.sources, vec!["Bass".to_string(), "Keys".to_string()]);
    let bass: Vec<(f64, u8)> = got.notes.iter().filter(|g| g.source == 0).map(|g| (g.start, g.key)).collect();
    assert_eq!(bass, vec![(0.0, 40), (1.0, 47), (2.0, 43), (3.0, 45)]);
    assert!(got.notes.iter().all(|g| g.length == 0.5));
    assert!(!got.choices.iter().any(|c| c.clip == beat_b()));
    assert!(got.choices.iter().any(|c| c.name == "Walk on Bass at bar 17"));
    assert!(got.choices.iter().any(|c| c.name == "Walk on Bass in scene 2"));
}

#[test]
fn none_this_track_and_one_clip() {
    let project = demo_project();
    assert!(ghosts(&project, beat_b(), GhostSource::None).notes.is_empty());
    // The other three clips on Drums, each from the edited clip's start.
    let track = ghosts(&project, beat_b(), GhostSource::Track);
    assert_eq!(track.sources, vec!["Drums".to_string()]);
    assert_eq!(track.notes.len(), 12);
    let vox = ghosts(&project, beat_b(), GhostSource::Clip(ClipRef::Slot(TrackId(4), 2)));
    assert_eq!(vox.sources, vec!["Vox".to_string()]);
    assert_eq!(vox.notes.iter().map(|g| g.key).collect::<Vec<_>>(), vec![67, 74, 70, 72]);
    // The edited clip itself is never its own ghost.
    assert!(ghosts(&project, beat_b(), GhostSource::Clip(beat_b())).notes.is_empty());
}

#[test]
fn a_slot_clip_shows_the_other_slots_of_its_scene() {
    let project = demo_project();
    // Scene 3 (index 2): Beat B, Walk, Chords 2, Hook.
    let got = ghosts(&project, ClipRef::Slot(TrackId(1), 2), GhostSource::Together);
    assert_eq!(got.sources, vec!["Bass".to_string(), "Keys".to_string(), "Vox".to_string()]);
    assert_eq!(got.notes.len(), 12);
}
