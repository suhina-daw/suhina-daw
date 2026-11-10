// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::detail::piano_roll_grid::{GridMode, Width};

fn note(time: f64, duration: f64, key: u8) -> Note {
    Note { time, duration, key, velocity: 100.0 / 127.0, release_velocity: 0.5, channel: 0, muted: false }
}

#[test]
fn the_editor_spans_the_loop_or_the_clips_part_of_its_content() {
    assert_eq!(content_end(Some(4.0), 0.0, 48.0, 4.0), 4.0);
    assert_eq!(content_end(None, 2.0, 8.0, 16.0), 10.0);
    assert_eq!(content_end(None, 0.0, f64::NAN, 16.0), 16.0);
    assert_eq!(content_end(None, 0.0, 0.25, 16.0), 1.0);
    assert_eq!(content_end(None, 0.0, 1e300, 16.0), 120_000.0);
}

#[test]
fn keys_are_named_with_60_as_c3() {
    assert_eq!(key_name(60), "C3");
    assert_eq!(key_name(0), "C-2");
    assert_eq!(key_name(127), "G8");
    assert_eq!(key_name(61), "C#3");
    assert!(is_black(61) && !is_black(60));
}

/// A sixteenth grid in 4/4 at 40 points a beat.
fn sixteenths() -> TimelineGrid {
    TimelineGrid::new(0.25, 40.0, 4.0, MIN_LINE_SPACING)
}

#[test]
fn the_grid_follows_the_chosen_value_and_thins_out_when_lines_crowd() {
    assert_eq!(sixteenths(), TimelineGrid { step: 0.25, bar_beats: 4.0, snaps: true });
    // A sixteenth at 10 points a beat is 2.5 points: doubled twice to 10.
    assert_eq!(TimelineGrid::new(0.25, 10.0, 4.0, MIN_LINE_SPACING).step, 1.0);
    assert_eq!(
        TimelineGrid::new(0.0, 40.0, 4.0, MIN_LINE_SPACING),
        TimelineGrid { step: 1.0, bar_beats: 4.0, snaps: false }
    );
    let grid = sixteenths();
    assert_eq!(grid.place(1.3, false, false), 1.25);
    assert_eq!(grid.place(1.3, false, true), 1.25);
    assert_eq!(grid.place(1.45, false, true), 1.25);
    assert_eq!(grid.place(1.3, true, false), 1.3);
}

/// Whether, for `clip` at `beat_width`, the lines are on its first `bars` bars' bar lines
/// (every one while the step is shorter than a bar, else a whole number of bars apart) and
/// every beat there snaps (to the nearest line and to the line before it) onto a drawn line.
fn on_the_drawn_lines(clip: &PianoClip, beat_width: f32, bars: f64) -> bool {
    let grid = note_grid(clip, beat_width);
    let last = bars * clip.bar_beats;
    let drawn: Vec<i64> =
        grid.lines(0.0, last + grid.step, MAX_LINES).iter().map(|(b, _)| (b * 1000.0).round() as i64).collect();
    let bar_lines = if grid.step < clip.bar_beats {
        (0..=bars as i64).all(|n| drawn.contains(&((n as f64 * clip.bar_beats * 1000.0).round() as i64)))
    } else {
        let bars_apart = grid.step / clip.bar_beats;
        (bars_apart - bars_apart.round()).abs() < 1e-9
    };
    let snaps = (0..(last * 50.0) as i64).map(|n| n as f64 / 50.0).all(|beat| {
        [grid.place(beat, false, false), grid.place(beat, false, true)]
            .iter()
            .all(|b| drawn.contains(&((b * 1000.0).round() as i64)))
    });
    bar_lines && snaps
}

#[test]
fn zoomed_out_the_grid_stays_on_the_bar_lines_with_triplets_and_odd_bars() {
    // A 1/16 triplet in 4/4: the beat and then bars, never 1/3, 2/3, 4/3 or 8/3 beats.
    let mut piano = clip(Vec::new());
    piano.grid = 1.0 / 6.0;
    assert_eq!(note_grid(&piano, 20.0).step, 1.0);
    assert_eq!(note_grid(&piano, 3.0).step, 4.0);
    // 5/4 with a one-beat grid: the bar, then two bars, not 2, 4 or 8 beats.
    let mut five = clip(Vec::new());
    (five.bar_beats, five.grid) = (5.0, 1.0);
    assert_eq!(note_grid(&five, 3.0).step, 5.0);
    assert_eq!(note_grid(&five, 1.0).step, 10.0);
    // 3/8 with a 1/4 triplet: on every bar line, not every fourth.
    let mut three_eight = clip(Vec::new());
    (three_eight.bar_beats, three_eight.grid) = (1.5, 2.0 / 3.0);
    assert_eq!(note_grid(&three_eight, 2.0).step, 3.0);
    for beat_width in [100.0, 40.0, 20.0, 10.0, 5.0, 3.0, 2.0, 1.0] {
        for c in [&piano, &five, &three_eight] {
            assert!(on_the_drawn_lines(c, beat_width, 8.0), "{} beats a bar at {beat_width}", c.bar_beats);
        }
    }
    // An adaptive triplet grid too.
    piano.grid_menu = PianoGrid { mode: GridMode::Adaptive(Width::Narrowest), triplet: true, snap: true };
    piano.grid = 0.0;
    for beat_width in [100.0, 20.0, 5.0, 1.0] {
        assert!(on_the_drawn_lines(&piano, beat_width, 8.0), "adaptive at {beat_width}");
    }
    // A note's end dragged onto its start goes to the next drawn line: in 3/4 with a two-beat
    // grid, from beat 2 that is the bar line at 3, not beat 4.
    let mut three = clip(Vec::new());
    (three.bar_beats, three.grid) = (3.0, 2.0);
    let grabbed = note(2.0, 0.5, 60);
    assert_eq!(resize_by(-0.5, &grabbed, &[grabbed], note_grid(&three, 40.0), false), 0.5);
}

#[test]
fn a_move_goes_by_grid_steps_and_stops_at_the_limits() {
    let grid = sixteenths();
    let notes = [note(1.1, 1.0, 60), note(2.0, 1.0, 120)];
    // By whole steps, so an off-grid note stays as far off the grid.
    assert_eq!(move_delta(0.3, 0, &notes, grid, false), (0.25, 0));
    assert_eq!(move_delta(0.3, 0, &notes, grid, true), (0.3, 0));
    // Never before the start, never above key 127: the whole selection stops.
    assert_eq!(move_delta(-5.0, 0, &notes, grid, false), (-1.1, 0));
    assert_eq!(move_delta(0.0, 12, &notes, grid, false), (0.0, 7));
    assert_eq!(move_delta(0.0, -100, &notes, grid, false), (0.0, -60));
}

#[test]
fn a_resize_puts_the_end_on_the_grid_and_keeps_notes_long_enough() {
    let grid = sixteenths();
    let grabbed = note(1.0, 1.0, 60);
    let other = note(3.0, 0.5, 62);
    assert_eq!(resize_by(0.3, &grabbed, &[grabbed, other], grid, false), 0.25);
    assert!((resize_by(0.3, &grabbed, &[grabbed], grid, true) - 0.3).abs() < 1e-12);
    // Dragged onto its start: one step long.
    assert_eq!(resize_by(-1.0, &grabbed, &[grabbed], grid, false), -0.75);
    // The other selected note is shorter: it keeps the shortest length.
    assert!((resize_by(-0.75, &grabbed, &[grabbed, other], grid, false) - (MIN_LENGTH - 0.5)).abs() < 1e-12);
}

#[test]
fn the_playhead_is_found_in_the_clips_content_and_its_loop() {
    let plain = Placement { start: 8.0, length: 4.0, offset: 1.0, loop_range: None };
    assert_eq!(plain.content_beat(9.5), Some(2.5));
    assert_eq!(plain.content_beat(7.9), None);
    assert_eq!(plain.content_beat(12.0), None);
    // Looping beats 0 to 2 from offset 1: beat 2 of the content wraps to 0.
    let looped = Placement { start: 0.0, length: 8.0, offset: 1.0, loop_range: Some((0.0, 2.0)) };
    assert_eq!(looped.content_beat(0.5), Some(1.5));
    assert_eq!(looped.content_beat(1.5), Some(0.5));
    assert_eq!(looped.content_beat(f64::NAN), None);
}

#[test]
fn an_arrangement_beat_is_the_inverse_of_the_content_beat() {
    let plain = Placement { start: 8.0, length: 4.0, offset: 1.0, loop_range: None };
    assert_eq!(plain.arrangement_beat(2.5), Some(9.5));
    // Before the clip's offset, and past its end, the clip plays nothing there.
    assert_eq!(plain.arrangement_beat(0.5), None);
    assert_eq!(plain.arrangement_beat(5.0), None);
    let looped = Placement { start: 0.0, length: 8.0, offset: 1.0, loop_range: Some((0.0, 2.0)) };
    assert_eq!(looped.arrangement_beat(1.5), Some(0.5));
    assert_eq!(looped.arrangement_beat(1.0), Some(0.0));
    // Beat 2 of the content wraps into the loop's start, which the clip never plays (it
    // starts at the offset).
    assert_eq!(looped.arrangement_beat(2.0), None);
    assert_eq!(looped.arrangement_beat(f64::NAN), None);
    // Round trip through both mappings where the clip plays.
    for at in [8.0, 9.5, 11.5] {
        let content = plain.content_beat(at);
        assert_eq!(content.and_then(|beat| plain.arrangement_beat(beat)), Some(at), "at {at}");
    }
    // The loop repeats, so a later pass comes back to the loop's start rather than to `at`.
    for at in [0.0, 0.5] {
        let content = looped.content_beat(at);
        assert_eq!(content.and_then(|beat| looped.arrangement_beat(beat)), Some(at), "at {at}");
    }
}

#[test]
fn the_sixth_tool_is_mute_and_the_tools_stop_at_it() {
    assert_eq!(Tool::ALL.len(), 6);
    assert_eq!(Tool::ALL[5], (Tool::Mute, "Mute"));
    assert_eq!(tool_of("pianoRoll.tool6"), Some(Tool::Mute));
    assert_eq!(tool_of("pianoRoll.tool7"), None);
    // The wheel steps through the tools and stops at the last one, which is Mute.
    assert_eq!(step_tool(Tool::Slice, 1), Tool::Mute);
    assert_eq!(step_tool(Tool::Mute, 1), Tool::Mute);
    assert_eq!(step_tool(Tool::Select, -1), Tool::Select);
}

#[test]
fn the_eraser_cursor_is_hidden_so_its_shape_can_be_drawn() {
    // The windowing layer has no eraser, so the pointer is hidden over the grid and the
    // Erase button's shape is drawn in its place.
    assert_eq!(cursor_for(Tool::Erase), Some(CursorIcon::None));
    assert_eq!(cursor_for(Tool::Mute), None);
    assert_eq!(cursor_for(Tool::Select), None);
    assert_eq!(cursor_for(Tool::Draw), Some(CursorIcon::Crosshair));
}

fn clip(notes: Vec<Note>) -> PianoClip {
    PianoClip {
        address: json!({ "track": 1, "index": 0 }),
        notes: notes.into(),
        length: 8.0,
        color: Color32::WHITE,
        bar_beats: 4.0,
        grid: 0.25,
        placement: Placement { start: 16.0, length: 8.0, offset: 0.0, loop_range: None },
        grid_menu: PianoGrid::default(),
        shape: ClipShape { offset: 0.0, length: 8.0, loop_range: None, room: None },
        ghosts: Ghosts::default(),
        scale: None,
    }
}

/// A layout 200 points tall at 10 points a row, scrolled to the top, with `rows`.
fn folded_layout(rows: KeyRows) -> Layout {
    let grid = Rect::from_min_size(Pos2::new(36.0, 24.0), Vec2::new(400.0, 200.0));
    Layout {
        strip: grid,
        keys: grid,
        ruler: grid,
        grid,
        lane: grid,
        scroll: Vec2::ZERO,
        beat_width: 40.0,
        key_height: 10.0,
        rows,
    }
}

#[test]
fn a_fold_maps_heights_to_the_shown_keys_and_back() {
    use suhina_engine::model::ScaleMode;
    let mut c = clip(vec![note(0.0, 1.0, 40), note(1.0, 1.0, 64), note(2.0, 1.0, 60)]);
    // Fold: three rows, highest first; every height in a row is its key and back.
    let layout = folded_layout(scale_view::rows(&c, Fold::Notes));
    assert_eq!(layout.rows.len(), 3);
    for (row, key) in [(0, 64), (1, 60), (2, 40)] {
        let y = 24.0 + row as f32 * 10.0;
        assert_eq!(layout.y_of(key), y);
        assert_eq!(layout.key_at(y + 5.0), Some(key));
    }
    assert_eq!(layout.key_at(24.0 + 35.0), None, "below the last row is no key");
    assert!(layout.y_of(62) < -1000.0, "a hidden key is far above the grid");
    // A note is hit and a double-click adds on the row's key.
    let state = PianoRollState::default();
    let grid = note_grid(&c, 40.0);
    assert_eq!(note_at(&layout, &c.notes, Pos2::new(36.0 + 100.0, 24.0 + 15.0)).map(|(i, _)| i), Some(2));
    let added = state.new_note(&layout, &c, grid, Pos2::new(36.0 + 140.0, 24.0 + 25.0), false).unwrap();
    assert_eq!(added.key, 40);
    // A drag up one row from 40 moves by the 20 semitones to 60.
    let moved = effect(
        &Gesture::Move { from: Pos2::new(60.0, 50.0), grabbed: 0, copy: false },
        Pos2::new(60.0, 40.0),
        &layout,
        &c,
        &c.notes[..1],
        grid,
        false,
    );
    assert!(matches!(moved, Effect::Move(_, 20)), "{moved:?}");
    // Fold to scale: C major's keys and every key with a note; without a scale, every key.
    assert_eq!(scale_view::rows(&c, Fold::Scale).len(), 128);
    c.scale = Scale::new(0, ScaleMode::Major);
    let rows = scale_view::rows(&c, Fold::Scale);
    assert!(rows.shows(60) && rows.shows(62) && !rows.shows(61));
    assert!(rows.shows(40), "E is in C major");
    c.notes.push(note(3.0, 1.0, 61));
    assert!(scale_view::rows(&c, Fold::Scale).shows(61), "a note outside the scale keeps its row");
    assert_eq!(scale_view::rows(&c, Fold::Off).len(), 128);
    // Unfolded, the rows are the keys as they always were.
    let layout = folded_layout(KeyRows::default());
    assert_eq!(layout.y_of(127), 24.0);
    assert_eq!(layout.key_at(24.0 + 67.0 * 10.0 + 1.0), Some(60));
}

#[test]
fn z_fits_the_selection_and_shift_z_the_whole_clip() {
    let clip = clip(vec![note(2.0, 1.0, 60), note(3.0, 1.0, 62), note(6.0, 1.0, 64)]);
    let mut state = focused_on(&clip);
    state.view_width = 400.0;
    state.selected = BTreeSet::from([0, 1]);
    assert!(state.key_action("pianoRoll.zoomSelection", &clip, 0.0, None).is_none());
    // Beats 2 to 4 fill 400 points.
    assert_eq!((state.beat_width, state.scroll.x), (200.0, 400.0));
    assert!(state.key_action("pianoRoll.zoomClip", &clip, 0.0, None).is_none());
    assert_eq!((state.beat_width, state.scroll.x), (50.0, 0.0));
    // Nothing selected: Z fits the clip too.
    state.selected.clear();
    state.beat_width = 300.0;
    state.key_action("pianoRoll.zoomSelection", &clip, 0.0, None);
    assert_eq!(state.beat_width, 50.0);
    // A tiny selection stops at the zoom range.
    state.selected = BTreeSet::from([0]);
    state.view_width = 100_000.0;
    state.key_action("pianoRoll.zoomSelection", &clip, 0.0, None);
    assert_eq!(state.beat_width, *PIANO_ROLL_BEAT_WIDTHS.end());
}

#[test]
fn z_stops_at_the_lower_zoom_bound_for_a_selection_longer_than_the_view() {
    let long = clip(vec![note(0.0, 200.0, 60)]);
    let mut state = focused_on(&long);
    state.view_width = 400.0;
    state.selected = BTreeSet::from([0]);
    state.key_action("pianoRoll.zoomSelection", &long, 0.0, None);
    assert_eq!(state.beat_width, *PIANO_ROLL_BEAT_WIDTHS.start());
}

fn focused_on(clip: &PianoClip) -> PianoRollState {
    PianoRollState { shown: Some(clip.address.clone()), focused: true, ..PianoRollState::default() }
}

#[test]
fn the_grid_keys_leave_a_new_grid_for_the_window_and_snap_off_snaps_nothing() {
    let mut piano = clip(Vec::new());
    let mut state = focused_on(&piano);
    assert_eq!(state.key_action("pianoRoll.gridWiden", &piano, 0.0, None), None);
    assert_eq!(state.grid_picked.take().map(|g| g.label()).as_deref(), Some("1/8"));
    state.key_action("pianoRoll.gridSnap", &piano, 0.0, None);
    let free = state.grid_picked.take().unwrap_or_default();
    assert!(!free.snap);
    piano.grid_menu = free;
    let grid = note_grid(&piano, 40.0);
    assert_eq!((grid.step, grid.snaps), (0.25, false), "the lines stay, nothing snaps");
    assert_eq!(nearest_line(grid, 1.1), 1.1);
    // A grid with no control bar value, such as a 1/32 triplet, is as good as any other.
    piano.grid_menu = PianoGrid::from_value("1/32");
    state.key_action("pianoRoll.gridTriplet", &piano, 0.0, None);
    assert_eq!(state.grid_picked.take().map(|g| g.label()).as_deref(), Some("1/32T"));
}

#[test]
fn paste_goes_after_the_selection_else_at_the_playhead_inside_the_clip() {
    let clip = clip(vec![note(1.0, 0.5, 60), note(2.0, 1.0, 64)]);
    // Stacked notes allowed, so the params are only where the notes go.
    let mut state = PianoRollState { notes_replace: false, ..focused_on(&clip) };
    state.selected = BTreeSet::from([0]);
    let paste = state.key_action("selection.paste", &clip, 0.0, Some(2)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 1.5 })));
    assert_eq!(state.selected, BTreeSet::from([2, 3]));
    // Nothing selected: the playhead at beat 19 of the arrangement is beat 3 of the clip.
    state.selected.clear();
    let paste = state.key_action("selection.paste", &clip, 19.0, Some(1)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 3.0 })));
    // Outside the clip: no time, so the command puts it right after the copied notes.
    state.selected.clear();
    let paste = state.key_action("selection.paste", &clip, 40.0, Some(1)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0 })));
    // Nothing copied, or no keyboard: the key is left for the clip.
    assert!(state.key_action("selection.paste", &clip, 0.0, None).is_none());
    state.focused = false;
    assert!(state.key_action("selection.paste", &clip, 0.0, Some(1)).is_none());
}

#[test]
fn keys_stop_at_the_limits_and_never_make_notes_shorter_than_a_step() {
    let clip = clip(vec![note(0.1, 0.25, 127), note(2.0, 1.0, 60)]);
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([0]);
    let params = |a: Option<Action>| a.map(|a| a.params);
    // Earlier stops at the content's start; up past key 127 and an octave up do nothing.
    assert_eq!(
        params(state.key_action("pianoRoll.earlier", &clip, 0.0, None)).map(|p| p["beats"].clone()),
        Some(json!(-0.1))
    );
    assert!(state.key_action("pianoRoll.up", &clip, 0.0, None).is_none());
    assert!(state.key_action("pianoRoll.octaveUp", &clip, 0.0, None).is_none());
    assert!(state.key_action("pianoRoll.shorter", &clip, 0.0, None).is_none());
    state.selected = BTreeSet::from([1]);
    assert_eq!(
        params(state.key_action("pianoRoll.shorter", &clip, 0.0, None)).map(|p| p["lengths"].clone()),
        Some(json!([0.75]))
    );
    // Without a selection copy does nothing here; Ctrl+A and Escape change only the view.
    state.selected.clear();
    assert!(state.key_action("selection.copy", &clip, 0.0, None).is_none());
    assert!(state.key_action("pianoRoll.selectAll", &clip, 0.0, None).is_none());
    assert_eq!(state.selected, BTreeSet::from([0, 1]));
    assert!(state.key_action("pianoRoll.deselect", &clip, 0.0, None).is_none());
    assert!(state.selected.is_empty());
}

#[test]
fn a_longer_key_goes_to_the_next_line_or_the_next_note_of_the_key() {
    let notes = [note(0.0, 0.3, 60), note(0.4, 0.25, 60), note(1.0, 0.25, 60), note(0.2, 1.0, 62)];
    let grid = TimelineGrid::new(0.25, 400.0, 4.0, MIN_LINE_SPACING);
    let end = |index: usize, longer, stops| key_step_end(&notes[index], &notes, grid, longer, stops);
    // Off the grid at 0.3: the next line would be 0.5, and the next note of its key is at 0.4.
    assert!((end(0, true, true) - 0.4).abs() < 1e-9);
    // Allowed to stack, the next note does not stop it.
    assert!((end(0, true, false) - 0.5).abs() < 1e-9);
    // Off the grid at 0.65, the next line is 0.75 and the next note far enough.
    assert!((end(1, true, true) - 0.75).abs() < 1e-9);
    // Touching the next note, or the last of its key: it stays, or goes on to the line.
    let touching = [note(0.0, 0.4, 60), note(0.4, 0.25, 60)];
    assert!((key_step_end(&touching[0], &touching, grid, true, true) - 0.4).abs() < 1e-9);
    assert!((end(3, true, true) - 1.25).abs() < 1e-9);
    // Shorter: the line before, and never less than a step from the start.
    assert!((end(3, false, true) - 0.75 - 0.25).abs() < 1e-9);
    assert!((end(1, false, true) - 0.65).abs() < 1e-9, "a step long already");
}

#[test]
fn replaced_notes_leave_the_selection_on_the_same_notes() {
    let mut state = PianoRollState { selected: BTreeSet::from([1, 3, 5]), ..PianoRollState::default() };
    state.notes_replaced(&[2, 3]);
    assert_eq!(state.selected, BTreeSet::from([1, 3]));
}

#[test]
fn which_keys_are_named() {
    use crate::settings::PianoKeyNames::{All, OnlyC, White};
    assert!(named(60, OnlyC, false, 12.0) && !named(62, OnlyC, false, 12.0));
    assert!(named(62, White, false, 12.0) && !named(61, White, false, 12.0));
    assert!(named(61, All, false, 12.0));
    assert!(named(61, OnlyC, true, 12.0), "folded rows are all named");
    assert!(!named(60, All, true, 7.0), "low rows have no room");
}

#[test]
fn with_stacked_notes_off_what_a_key_places_replaces_what_is_under_it() {
    let clip = clip(vec![note(1.0, 0.5, 60), note(2.0, 1.0, 64)]);
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([0]);
    let paste = state.key_action("selection.paste", &clip, 0.0, Some(2)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 1.5, "replace": true })));
}

fn span_state(clip: &PianoClip, start: f64, end: f64) -> PianoRollState {
    let mut state = focused_on(clip);
    state.select_span(start, end);
    state.settle_selection(&clip.notes);
    state
}

#[test]
fn a_time_selection_acts_on_the_notes_that_start_in_it() {
    let clip = clip(vec![note(0.5, 2.0, 60), note(1.0, 0.5, 62), note(2.5, 0.5, 64)]);
    let mut state = span_state(&clip, 1.0, 3.0);
    assert_eq!(state.acting(), &BTreeSet::from([1, 2]), "the one that started before it stays out");
    let copy = state.key_action("selection.copy", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(copy, Some(json!({ "track": 1, "index": 0, "notes": [1, 2] })));
    let delete = state.delete_selected().map(|a| a.params);
    assert_eq!(delete, Some(json!({ "track": 1, "index": 0, "notes": [1, 2] })));
}

#[test]
fn a_time_selection_with_no_notes_duplicates_nothing() {
    let clip = clip(vec![note(0.0, 1.0, 60)]);
    let mut state = span_state(&clip, 2.0, 3.0);
    assert!(state.key_action("selection.duplicate", &clip, 0.0, None).is_none());
    assert_eq!(state.span.as_ref().map(|s| s.start), Some(2.0), "and stays");
}

#[test]
fn a_duplicate_carries_the_time_selection_on_and_an_undo_brings_it_back() {
    let one = clip(vec![note(1.0, 0.5, 60)]);
    let mut state = PianoRollState { notes_replace: false, ..span_state(&one, 0.0, 2.0) };
    let copy = state.key_action("selection.duplicate", &one, 0.0, None).map(|a| a.params);
    assert_eq!(copy, Some(json!({ "track": 1, "index": 0, "notes": [0], "beats": 2.0 })));
    assert_eq!(state.selected, BTreeSet::from([1]));
    let span = state.span.clone().unwrap();
    assert_eq!((span.start, span.end), (2.0, 4.0));

    // The next frame has the copy; the keys go on from it.
    let two = clip(vec![note(1.0, 0.5, 60), note(3.0, 0.5, 60)]);
    state.selecting_new = false;
    state.settle_selection(&two.notes);
    assert_eq!(state.span.as_ref().map(|s| s.end), Some(4.0), "the copies are selected with it");
    let again = state.key_action("selection.duplicate", &two, 0.0, None).map(|a| a.params);
    assert_eq!(again, Some(json!({ "track": 1, "index": 0, "notes": [1], "beats": 2.0 })));
    let span = state.span.clone().unwrap();
    assert_eq!((span.start, span.end), (4.0, 6.0));

    // Undone, one at a time, to where it began.
    state.selecting_new = false;
    state.undo_ran();
    state.settle_selection(&two.notes);
    assert_eq!(state.selected, BTreeSet::from([1]));
    assert_eq!(state.span.as_ref().map(|s| s.start), Some(2.0));
    state.undo_ran();
    state.settle_selection(&one.notes);
    assert!(state.selected.is_empty());
    assert_eq!(state.span.as_ref().map(|s| (s.start, s.end)), Some((0.0, 2.0)));
    assert_eq!(state.acting(), &BTreeSet::from([0]));
}

#[test]
fn selecting_something_else_ends_the_time_selection() {
    let clip = clip(vec![note(1.0, 0.5, 60)]);
    let mut state = span_state(&clip, 0.0, 2.0);
    state.selected = BTreeSet::from([0]);
    state.settle_selection(&clip.notes);
    assert!(state.span.is_none());
}

#[test]
fn a_paste_goes_on_from_a_time_selection_as_far_in_as_the_notes_were() {
    let clip = clip(vec![note(1.0, 0.5, 60)]);
    let mut state = PianoRollState { notes_replace: false, ..span_state(&clip, 0.0, 2.0) };
    let paste = state.key_action("selection.paste", &clip, 0.0, Some(1)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 3.0 })));
    assert_eq!(state.span.as_ref().map(|s| s.start), Some(2.0));
}

#[test]
fn a_paste_goes_to_the_insert_mark_with_nothing_selected() {
    let clip = clip(vec![note(1.0, 0.5, 60)]);
    let mut state = PianoRollState { notes_replace: false, insert_at: Some(5.0), ..focused_on(&clip) };
    let paste = state.key_action("selection.paste", &clip, 0.0, Some(1)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 5.0 })));
    state.selecting_new = false;
    let two = clip_of_two();
    state.settle_selection(&two.notes);
    assert_eq!(state.insert_at, None, "the mark is gone once notes are selected");
}

fn clip_of_two() -> PianoClip {
    clip(vec![note(1.0, 0.5, 60), note(5.0, 0.5, 60)])
}

#[test]
fn replaced_notes_leave_the_time_selection_on_the_same_notes() {
    let clip = clip(vec![note(1.0, 0.5, 60), note(3.0, 0.5, 60)]);
    let mut state = span_state(&clip, 0.0, 2.0);
    let _ = state.key_action("selection.duplicate", &clip, 0.0, None);
    // The copy, index 2, replaced the note at index 1.
    state.notes_replaced(&[1]);
    assert_eq!(state.selected, BTreeSet::from([1]));
    assert_eq!(state.span.as_ref().map(|s| s.with.clone()), Some(BTreeSet::from([1])));
}

#[test]
fn a_rectangle_covers_whole_grid_steps() {
    let layout = Layout {
        strip: Rect::NOTHING,
        keys: Rect::NOTHING,
        ruler: Rect::NOTHING,
        grid: Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.0)),
        lane: Rect::NOTHING,
        scroll: Vec2::ZERO,
        beat_width: 100.0,
        key_height: 12.0,
        rows: scale_view::rows(&clip(Vec::new()), Fold::default()),
    };
    let grid = TimelineGrid { step: 0.5, bar_beats: 4.0, snaps: true };
    let rect = Rect::from_min_max(Pos2::new(62.5, 10.0), Pos2::new(125.0, 20.0));
    assert_eq!(swept_span(grid, &layout, &[], rect), (0.5, 1.5));
    let on_lines = Rect::from_min_max(Pos2::new(50.0, 10.0), Pos2::new(150.0, 20.0));
    assert_eq!(swept_span(grid, &layout, &[], on_lines), (0.5, 1.5));
    let free = TimelineGrid { snaps: false, ..grid };
    assert_eq!(swept_span(free, &layout, &[], rect), (0.625, 1.25));
    // A note it touches is covered whole, however little of it the rectangle reached.
    let long = note(0.25, 3.0, 60);
    let middle = layout.note_rect(&long).center();
    let sliver = Rect::from_min_max(Pos2::new(100.0, middle.y - 1.0), Pos2::new(110.0, middle.y + 1.0));
    assert_eq!(swept_span(grid, &layout, &[long], sliver), (0.25, 3.25));
    let clear = Rect::from_min_max(Pos2::new(100.0, middle.y + 40.0), Pos2::new(110.0, middle.y + 42.0));
    assert_eq!(swept_span(grid, &layout, &[long], clear), (1.0, 1.5), "one it misses is not");
}

#[test]
fn the_velocity_lane_stays_through_a_zoom() {
    assert_eq!(velocity_lane_height(300.0, DEFAULT_KEY_HEIGHT, None), velocity_lane_height(300.0, 160.0, None));
}

/// A copy that replaced as many notes as it made leaves the note count as it was: the undo is told
/// by the notes being back.
#[test]
fn undoing_a_copy_that_replaced_a_note_brings_the_selection_back() {
    let clip = clip(vec![note(1.0, 0.5, 60), note(3.0, 0.5, 60)]);
    let mut state = span_state(&clip, 0.0, 2.0);
    let _ = state.key_action("selection.duplicate", &clip, 0.0, None);
    state.notes_replaced(&[1]);
    state.selecting_new = false;
    // The copy in the replaced note's place, as many notes as before.
    let mut after = clip.notes.clone();
    if let Some(copy) = after.get_mut(1) {
        copy.velocity = 0.5;
    }
    state.settle_selection(&after);
    assert_eq!(state.selected, BTreeSet::from([1]), "the copy stays selected");
    // Notes that came out the same are no undo: only an undo brings the selection back.
    state.settle_selection(&clip.notes);
    assert_eq!(state.selected, BTreeSet::from([1]), "no undo ran");
    state.undo_ran();
    state.settle_selection(&clip.notes);
    assert!(state.selected.is_empty());
    assert_eq!(state.span.as_ref().map(|s| (s.start, s.end)), Some((0.0, 2.0)));
    assert_eq!(state.acting(), &BTreeSet::from([0]));
}

#[test]
fn a_key_never_moves_a_note_past_the_furthest_start() {
    let limit = f64::from(BEAT_LIMIT);
    let clip = clip(vec![note(limit, 0.5, 60)]);
    let mut state = span_state(&clip, limit, limit + 1.0);
    assert!(state.key_action("pianoRoll.later", &clip, 0.0, None).is_none());
    assert_eq!(state.span.as_ref().map(|s| s.start), Some(limit), "the stretch stays with it");
}

/// A rectangle that reached notes keeps its whole stretch: a copy goes after the empty part too.
#[test]
fn a_rectangle_that_reached_notes_copies_by_its_whole_stretch() {
    let clip = clip(vec![note(0.0, 0.5, 60), note(5.0, 0.5, 60)]);
    let mut state = PianoRollState { notes_replace: false, ..focused_on(&clip) };
    state.selected = BTreeSet::from([0]);
    state.select_span(0.0, 2.0);
    state.settle_selection(&clip.notes);
    assert_eq!(state.acting(), &BTreeSet::from([0]));
    let copy = state.key_action("selection.duplicate", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(copy, Some(json!({ "track": 1, "index": 0, "notes": [0], "beats": 2.0 })));
    assert_eq!(state.span.as_ref().map(|s| (s.start, s.end)), Some((2.0, 4.0)));
}

/// A longer key from a triplet line a command rounded goes on to the next line.
#[test]
fn a_longer_key_goes_on_along_triplet_lines() {
    let grid = TimelineGrid { step: 1.0 / 3.0, bar_beats: 4.0, snaps: true };
    let first = note(0.0, 0.333333, 60);
    assert!((key_step_end(&first, &[first], grid, true, true) - 2.0 / 3.0).abs() < 1e-9);
}

/// Alt and an arrow key goes a fine step, off the grid; with Shift it changes the length that way.
#[test]
fn alt_and_an_arrow_key_go_a_fine_step_off_the_grid() {
    let clip = clip(vec![note(1.0, 1.0, 60), note(2.25, 1.0, 60)]);
    let mut state = PianoRollState { notes_replace: false, ..focused_on(&clip) };
    state.selected = BTreeSet::from([0]);
    let params = |action: Option<Action>| action.map(|a| (a.id, a.params));
    let (id, later) = params(state.key_action("pianoRoll.laterFine", &clip, 0.0, None)).unwrap();
    assert_eq!((id, later["beats"].as_f64()), ("note.move", Some(0.03125)));
    let (_, earlier) = params(state.key_action("pianoRoll.earlierFine", &clip, 0.0, None)).unwrap();
    assert_eq!(earlier["beats"].as_f64(), Some(-0.03125));
    let (id, longer) = params(state.key_action("pianoRoll.longerFine", &clip, 0.0, None)).unwrap();
    assert_eq!((id, longer["lengths"][0].as_f64()), ("note.resize", Some(1.03125)));
    let (_, shorter) = params(state.key_action("pianoRoll.shorterFine", &clip, 0.0, None)).unwrap();
    assert_eq!(shorter["lengths"][0].as_f64(), Some(0.96875));
}

#[test]
fn a_fine_step_longer_stops_at_the_next_note_of_the_key() {
    let clip = clip(vec![note(1.0, 1.24, 60), note(2.25, 1.0, 60)]);
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([0]);
    let longer = state.key_action("pianoRoll.longerFine", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(longer.and_then(|p| p["lengths"][0].as_f64()), Some(1.25));
}

#[test]
fn a_key_resize_replaces_what_is_under_it_unless_notes_stack() {
    let clip = clip(vec![note(0.0, 1.0, 60)]);
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([0]);
    let longer = state.key_action("pianoRoll.longer", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(longer.map(|p| p["replace"].clone()), Some(json!(true)));
    state.notes_replace = false;
    let longer = state.key_action("pianoRoll.longer", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(longer.map(|p| p["replace"].clone()), Some(Value::Null));
}

/// An arrow key takes the first note to the next line, the rest keeping their distance to it, and
/// from there a line at a time.
#[test]
fn an_arrow_key_brings_notes_off_the_grid_onto_it() {
    let beats = |notes: Vec<Note>, action: &str| {
        let clip = clip(notes);
        let mut state = focused_on(&clip);
        state.selected = (0..clip.notes.len()).collect();
        let step = note_grid(&clip, state.beat_width).step;
        let moved = state.key_action(action, &clip, 0.0, None).map(|a| a.params["beats"].as_f64());
        (moved.flatten().unwrap_or(0.0), step)
    };
    let (moved, step) = beats(vec![note(4.0, 0.25, 60)], "pianoRoll.later");
    assert_eq!(moved, step, "on a line: a whole step");
    let (moved, step) = beats(vec![note(4.0, 0.25, 60)], "pianoRoll.earlier");
    assert_eq!(moved, -step);

    let off = step / 4.0;
    let (moved, _) = beats(vec![note(4.0 - off, 0.25, 60), note(6.0, 0.25, 62)], "pianoRoll.later");
    assert!((moved - off).abs() < 1e-6, "the first note reaches the line: {moved}");
    let (moved, _) = beats(vec![note(4.0 + off, 0.25, 60), note(6.0, 0.25, 62)], "pianoRoll.earlier");
    assert!((moved + off).abs() < 1e-6, "the first note goes back to the line: {moved}");
}

/// A time selection that holds no note start gives Quantise nothing, not the whole clip.
#[test]
fn quantise_leaves_the_clip_alone_when_a_time_selection_holds_no_note() {
    let clip = clip(vec![note(0.1, 0.5, 60)]);
    assert!(focused_on(&clip).quantize_selected().is_some(), "with no selection at all: the whole clip");
    assert_eq!(span_state(&clip, 8.0, 9.0).quantize_selected(), None);
    let found = span_state(&clip, 0.0, 1.0).quantize_selected().map(|a| a.params["notes"].clone());
    assert_eq!(found, Some(json!([0])));
}

/// Legato's last notes go as far as the stretch selected in time.
#[test]
fn a_time_selection_says_where_legato_ends() {
    let clip = clip(vec![note(0.0, 0.5, 60)]);
    assert_eq!(focused_on(&clip).span_end(), None);
    assert_eq!(span_state(&clip, 0.0, 4.0).span_end(), Some(4.0));
}

/// A copy the command refused never came: the selection and the stretch before it are back.
#[test]
fn a_refused_copy_gives_the_selection_back() {
    let clip = clip(vec![note(0.0, 0.5, 60)]);
    let mut state = span_state(&clip, 0.0, 2.0);
    state.selected = BTreeSet::from([0]);
    if let Some(span) = &mut state.span {
        span.with = state.selected.clone();
    }
    state.key_action("selection.duplicate", &clip, 0.0, None);
    assert_eq!(state.selected, BTreeSet::from([1]));
    state.selecting_new = false;
    state.settle_selection(&clip.notes);
    assert_eq!(state.selected, BTreeSet::from([0]));
    assert_eq!(state.span.as_ref().map(|s| (s.start, s.end)), Some((0.0, 2.0)));
}

/// Legato's last notes end at a time selection's end, at the next note after them, or at the clip's.
#[test]
fn legato_ends_the_last_notes_where_the_clip_goes_on() {
    let notes = [note(0.0, 0.5, 60), note(1.0, 0.5, 62), note(2.0, 0.5, 64)];
    let address = json!({});
    let until = |selected: &BTreeSet<usize>, span| {
        crate::detail::midi_tools::Tools { address: &address, notes: &notes, selected, length: 8.0, nothing: false }
            .legato_until(span)
    };
    let none = BTreeSet::new();
    assert_eq!(until(&none, None), 8.0, "every note: to the clip's end");
    assert_eq!(until(&BTreeSet::from([0, 1]), None), 2.0, "the next note that is not among them");
    assert_eq!(until(&none, Some(4.0)), 4.0, "a time selection's end comes first");
}

/// A rectangle with Shift keeps what was selected in time, and the stretch runs over both.
#[test]
fn a_rectangle_that_adds_goes_on_from_the_time_selection() {
    let clip = clip(vec![note(0.0, 0.5, 60), note(4.0, 0.5, 62)]);
    let layout = folded_layout(KeyRows::default());
    let mut state = focused_on(&clip);
    state.select_span(0.0, 1.0);
    let rect = Rect::from_two_pos(
        Pos2::new(layout.x_of(2.0), layout.grid.top() + 1.0),
        Pos2::new(layout.x_of(3.0), layout.grid.top() + 2.0),
    );
    let mut actions = Vec::new();
    let gesture = Gesture::Select { from: rect.min, add: true };
    commit(&gesture, Effect::Select(rect), &clip, &mut state, &mut actions, &layout);
    let span = state.span.as_ref().map(|s| (s.start, s.end));
    assert_eq!(span.map(|s| s.0), Some(0.0), "the old start stays");
    assert!(span.is_some_and(|s| s.1 >= 3.0), "the new end counts: {span:?}");
}

/// The notes of a ghost say which clip they come from, and the last drawn is the one found.
#[test]
fn a_ghost_note_finds_its_clip() {
    use crate::detail::detail_view::ClipRef;
    use suhina_engine::model::TrackId;
    let (a, b) = (ClipRef::Slot(TrackId(1), 0), ClipRef::Slot(TrackId(2), 3));
    let ghost =
        |clip, start| ghost_view::GhostNote { start, length: 1.0, key: 60, color: Color32::RED, source: 0, clip };
    let ghosts = ghost_view::Ghosts { notes: vec![ghost(a, 0.0), ghost(b, 0.0), ghost(a, 8.0)], ..Default::default() };
    let rect_of =
        |g: &ghost_view::GhostNote| Rect::from_min_size(Pos2::new(g.start as f32 * 10.0, 0.0), Vec2::new(10.0, 10.0));
    assert_eq!(ghost_view::ghost_at(&ghosts, Pos2::new(5.0, 5.0), rect_of), Some(b));
    assert_eq!(ghost_view::ghost_at(&ghosts, Pos2::new(85.0, 5.0), rect_of), Some(a));
    assert_eq!(ghost_view::ghost_at(&ghosts, Pos2::new(45.0, 5.0), rect_of), None);
}

/// A selected note takes the accent, unless the note's colour is close to it.
#[test]
fn a_selected_note_stands_out_from_its_colour() {
    let (accent, plain) = (Color32::from_rgb(0xA6, 0xA3, 0xFF), Color32::from_rgb(0xEC, 0xE8, 0xE1));
    assert_eq!(selected_note_color(Color32::from_rgb(0xF2, 0xD5, 0x41), accent, plain), accent);
    assert_eq!(selected_note_color(Color32::from_rgb(0xE0, 0x30, 0x30), accent, plain), accent);
    assert_eq!(selected_note_color(Color32::from_rgb(0x90, 0x90, 0xF0), accent, plain), plain);
}
