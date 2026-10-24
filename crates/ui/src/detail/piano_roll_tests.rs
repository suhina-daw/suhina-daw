// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
    assert_eq!(grid_step(Quantization::Beats(0.25), 4.0), 0.25);
    assert_eq!(grid_step(Quantization::Bars(1), 3.0), 3.0);
    assert_eq!(grid_step(Quantization::None, 4.0), 0.0);
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
        notes,
        length: 8.0,
        color: Color32::WHITE,
        bar_beats: 4.0,
        grid: 0.25,
        placement: Placement { start: 16.0, length: 8.0, offset: 0.0, loop_range: None },
        grid_menu: PianoGrid::default(),
        grid_follows: false,
        shape: ClipShape { offset: 0.0, length: 8.0, loop_range: None, room: None },
        ghosts: Ghosts::default(),
        scale: None,
    }
}

/// A layout 200 points tall at 10 points a row, scrolled to the top, with `rows`.
fn folded_layout(rows: KeyRows) -> Layout {
    let grid = Rect::from_min_size(Pos2::new(36.0, 24.0), Vec2::new(400.0, 200.0));
    Layout { keys: grid, ruler: grid, grid, lane: grid, scroll: Vec2::ZERO, beat_width: 40.0, key_height: 10.0, rows }
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
    assert_eq!(state.beat_width, BEAT_WIDTH_RANGE.1);
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
    // Following the control bar, a key whose grid it cannot hold does nothing.
    piano.grid_menu = PianoGrid::from_value("1/32");
    piano.grid_follows = true;
    state.key_action("pianoRoll.gridTriplet", &piano, 0.0, None);
    assert_eq!(state.grid_picked, None);
    state.key_action("pianoRoll.gridNarrow", &piano, 0.0, None);
    assert_eq!(state.grid_picked.map(|g| g.label()).as_deref(), Some("1/32"));
}

#[test]
fn paste_goes_after_the_selection_else_at_the_playhead_inside_the_clip() {
    let clip = clip(vec![note(1.0, 0.5, 60), note(2.0, 1.0, 64)]);
    let mut state = focused_on(&clip);
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
        params(state.key_action("pianoRoll.shorter", &clip, 0.0, None)).map(|p| p["by"].clone()),
        Some(json!(-0.25))
    );
    // Without a selection copy is left for the clip; Ctrl+A and Escape change only the view.
    state.selected.clear();
    assert!(state.key_action("selection.copy", &clip, 0.0, None).is_none());
    assert!(state.key_action("pianoRoll.selectAll", &clip, 0.0, None).is_none());
    assert_eq!(state.selected, BTreeSet::from([0, 1]));
    assert!(state.key_action("pianoRoll.deselect", &clip, 0.0, None).is_none());
    assert!(state.selected.is_empty());
}
