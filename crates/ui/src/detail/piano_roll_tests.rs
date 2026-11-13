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

/// Every toolbar icon drawn from artwork parses and rasters, so none falls back unseen.
#[test]
fn the_toolbar_icons_raster_from_their_artwork() {
    use crate::detail::piano_roll::IconButton::*;
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        for button in
            [Select, Draw, Paint, Erase, Slice, Mute, Cut, Copy, Paste, Duplicate, Quantise, MidiTools, Audition]
        {
            let svg = button.svg().unwrap_or_default();
            assert!(crate::kit::skin::parse_svg(svg).is_ok(), "{:?}", button.name());
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(14.0));
            assert!(crate::kit::skin::paint_icon(ui.painter(), rect, svg, Color32::WHITE), "{}", button.name());
        }
        assert!(Hide.svg().is_none(), "the × is drawn in code");
    });
    output.textures_delta.clear();
}

#[test]
fn keys_are_named_with_60_as_c3() {
    assert_eq!(key_name(60), "C3");
    assert_eq!(key_name(0), "C-2");
    assert_eq!(key_name(127), "G8");
    assert_eq!(key_name(61), "C#3");
    assert!(is_black(61) && !is_black(60));
}

/// With the setting at flats, black keys are named as the flat of the key above, everywhere
/// `key_name` and `pitch_class_name` name them; white keys are named as before.
#[test]
fn black_keys_are_named_as_sharps_or_flats_by_the_setting() {
    use crate::settings::NoteSpelling;
    set_note_spelling(NoteSpelling::Flats);
    let flats = ([key_name(61), key_name(70), key_name(60)], pitch_class_name(6));
    set_note_spelling(NoteSpelling::Sharps);
    assert_eq!(flats, (["Db3".to_string(), "Bb3".to_string(), "C3".to_string()], "Gb"));
    assert_eq!((key_name(61), pitch_class_name(6)), ("C#3".to_string(), "F#"));
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
fn the_drawing_tools_hide_the_pointer_for_their_icon() {
    // The windowing layer has no pencil, brush, eraser, blade or mute cursor, so the pointer is
    // hidden over the grid and the tool's toolbar icon is drawn in its place.
    for (tool, icon) in [
        (Tool::Draw, IconButton::Draw),
        (Tool::Paint, IconButton::Paint),
        (Tool::Erase, IconButton::Erase),
        (Tool::Slice, IconButton::Slice),
        (Tool::Mute, IconButton::Mute),
    ] {
        assert_eq!(cursor_for(tool), Some(CursorIcon::None), "{tool:?}");
        let (drawn, hotspot) = tool_pointer(tool).unwrap();
        assert_eq!(drawn, icon);
        assert!((0.0..=1.0).contains(&hotspot.x) && (0.0..=1.0).contains(&hotspot.y), "{tool:?}");
    }
    assert_eq!(cursor_for(Tool::Select), None);
    assert!(tool_pointer(Tool::Select).is_none());
    // The pencil's tip is at the bottom left: its square stands up and to the right of the pointer.
    let (_, hotspot) = tool_pointer(Tool::Draw).unwrap();
    let rect = tool_pointer_rect(Pos2::new(100.0, 100.0), hotspot, 18.0);
    assert!(rect.left() < 100.0 && rect.right() > 115.0 && rect.bottom() > 100.0 && rect.top() < 85.0, "{rect:?}");
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
        Inputs::default(),
    );
    assert!(matches!(moved, Effect::Move(_, 20, Some(_))), "{moved:?}");
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

/// Folded to the scale, Up and Down take each selected note to the next key the rows show, so
/// a note never lands on a key the fold hides; unfolded they go a semitone.
#[test]
fn folded_up_and_down_keep_the_notes_in_the_scale() {
    let mut c = clip(vec![note(0.0, 1.0, 60), note(0.0, 1.0, 64), note(0.0, 1.0, 67)]);
    c.scale = Scale::new(0, suhina_engine::model::ScaleMode::Major);
    let mut state = focused_on(&c);
    state.selected = BTreeSet::from([0, 1, 2]);
    // The toolbar's Scale folds the rows to the clip's scale.
    state.fold = Fold::Scale;
    state.audition = true;
    let up = state.key_action("pianoRoll.up", &c, 0.0, None).map(|a| a.params);
    assert_eq!(state.audition_pending, [62, 65, 69], "the notes sound where they go");
    state.audition_pending.clear();
    // C, E, G go to D, F, A: two, one and two semitones.
    assert_eq!(up.as_ref().and_then(|p| p.get("keys")).cloned(), Some(json!([62, 65, 69])), "{up:?}");
    let down = state.key_action("pianoRoll.down", &c, 0.0, None).map(|a| a.params);
    assert_eq!(down.as_ref().and_then(|p| p.get("keys")).cloned(), Some(json!([59, 62, 65])), "{down:?}");
    // A drag folded: each note steps the same number of shown rows from its own key.
    let rows = scale_view::rows(&c, Fold::Scale);
    let dragged: Vec<u8> = [57u8, 60, 64].iter().map(|k| edits::moved_key(&rows, *k, 1, Some(1))).collect();
    assert_eq!(dragged, [59, 62, 65], "A, C, E up a row: B, D, F");
    assert_eq!(edits::moved_key(&rows, 60, 1, None), 61, "unfolded, the semitones");
    let mut plain_clip = c.clone();
    plain_clip.scale = None;
    let plain = state.key_action("pianoRoll.up", &plain_clip, 0.0, None).map(|a| a.params);
    assert_eq!(plain.as_ref().and_then(|p| p.get("semitones")).cloned(), Some(json!(1)), "{plain:?}");
}

/// A drag of a note off the grid is magnetic: free until the first grid line, then it snaps to a
/// line or to its own offset, whichever is nearer; a note on the grid goes by grid steps.
#[test]
fn a_note_off_the_grid_drags_magnetically() {
    let c = clip(vec![note(1.03, 1.0, 60)]);
    // A sixteenth grid in 4/4.
    let grid = TimelineGrid { step: 0.25, bar_beats: 4.0, snaps: true };
    let notes = &c.notes;
    let moved = |raw: f64| edits::magnetic(1.03, raw, notes, grid);
    // Before the first line (1.25) it moves freely.
    assert!((moved(0.1) - 0.1).abs() < 1e-9);
    // Past it, near a line it lands on the line.
    assert!((moved(0.225) - 0.22).abs() < 1e-9, "onto 1.25: {}", moved(0.225));
    // Near its own offset a step on (1.28), it keeps the offset.
    assert!((moved(0.26) - 0.25).abs() < 1e-9, "1.28 keeps the feel: {}", moved(0.26));
    // Back to the left it lands on 1.0.
    assert!((moved(-0.04) + 0.03).abs() < 1e-9, "onto 1.0: {}", moved(-0.04));
    // A note on the grid goes by whole steps.
    let on = clip(vec![note(1.0, 1.0, 60)]);
    assert_eq!(edits::magnetic(1.0, 0.3, &on.notes, grid), 0.25);
    // The end of an off-grid note is dragged the same way: free, then onto a line or its offset.
    let humanised = note(1.0, 1.03, 60);
    assert!((resize_by(0.1, &humanised, &[humanised], grid, false) - 0.1).abs() < 1e-9, "free first");
    assert!((resize_by(0.225, &humanised, &[humanised], grid, false) - 0.22).abs() < 1e-9, "onto 2.25");
    // And its start.
    let late = note(1.03, 1.0, 60);
    assert!((start_by(-0.04, &late, &[late], grid, false) + 0.03).abs() < 1e-9, "onto 1.0");
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
fn paste_goes_to_the_selections_start_else_at_the_playhead_inside_the_clip() {
    let clip = clip(vec![note(1.0, 0.5, 60), note(2.0, 1.0, 64)]);
    // Stacked notes allowed, so the params are only where the notes go.
    let mut state = PianoRollState { notes_replace: false, ..focused_on(&clip) };
    state.selected = BTreeSet::from([0]);
    let paste = state.key_action("selection.paste", &clip, 0.0, Some(2)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 1.0 })), "on the selected note");
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

/// Shift+Right goes to the next grid line over the next note of the key, as far as a step goes:
/// with stacked notes off that note makes room once the selection is let go.
#[test]
fn a_longer_key_goes_to_the_next_line_over_the_next_note_of_the_key() {
    let notes = [note(0.0, 0.3, 60), note(0.4, 0.25, 60), note(0.2, 1.0, 62)];
    let grid = TimelineGrid::new(0.25, 400.0, 4.0, MIN_LINE_SPACING);
    let end = |index: usize, longer| key_step_end(&notes[index], grid, longer);
    // Off the grid at 0.3: the next line is 0.5, over the next note of its key at 0.4.
    assert!((end(0, true) - 0.5).abs() < 1e-9);
    // Off the grid at 0.65, the next line is 0.75.
    assert!((end(1, true) - 0.75).abs() < 1e-9);
    // Touching the next note: it goes on to the line all the same.
    let touching = note(0.0, 0.4, 60);
    assert!((key_step_end(&touching, grid, true) - 0.5).abs() < 1e-9);
    assert!((end(2, true) - 1.25).abs() < 1e-9);
    // Shorter: the line before, and never less than a step from the start.
    assert!((end(2, false) - 0.75 - 0.25).abs() < 1e-9);
    assert!((end(1, false) - 0.65).abs() < 1e-9, "a step long already");
}

/// A note under selected notes that are to make room shows what `note.makeRoom` will leave of it.
#[test]
fn notes_under_selected_ones_show_what_making_room_leaves() {
    let n = note(1.0, 2.0, 60);
    let cover = |from: f64, to: f64| (60u8, from, to);
    for whole in [false, true] {
        assert_eq!(left_after(&n, &[], whole), Some(n));
        assert_eq!(left_after(&n, &[(62, 0.0, 4.0)], whole), Some(n), "another key's note covers nothing");
        assert_eq!(left_after(&n, &[cover(0.5, 3.5)], whole), None, "covered whole");
        assert_eq!(left_after(&n, &[cover(1.0, 3.0)], whole), None, "covered exactly");
        // Starting first: what comes before the first that covers it.
        let head = left_after(&n, &[cover(2.5, 4.0), cover(1.5, 2.0)], whole).unwrap();
        assert!((head.time - 1.0).abs() < 1e-9 && (head.duration - 0.5).abs() < 1e-9, "{head:?}");
        // Touching it, not covering it.
        assert_eq!(left_after(&n, &[cover(3.0, 4.0), cover(0.0, 1.0)], whole), Some(n));
    }
    // Covered from its start: what runs on past it, or (Delete them) nothing.
    let tail = left_after(&n, &[cover(1.0, 2.0)], false).unwrap();
    assert!((tail.time - 2.0).abs() < 1e-9 && (tail.duration - 1.0).abs() < 1e-9, "{tail:?}");
    assert_eq!(left_after(&n, &[cover(1.0, 2.0)], true), None);
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
    // A paste lands on the notes it copies, to be moved off them: it makes room once it is let go.
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 1.0 })));
    assert!(state.settle_pending);
    // A duplicate goes after its notes, onto whatever is there, and replaces it at once.
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([0]);
    let duplicate = state.key_action("selection.duplicate", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(duplicate.map(|p| p["replace"].clone()), Some(json!(true)));
    assert!(!state.settle_pending);
    // With nothing selected a command replaces at once.
    let mut state = focused_on(&clip);
    let quantise = state.quantize_selected().map(|a| a.params);
    assert_eq!(quantise.map(|p| p["replace"].clone()), Some(json!(true)));
    assert!(!state.settle_pending);
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
fn a_paste_goes_to_a_time_selections_first_note() {
    let clip = clip(vec![note(1.0, 0.5, 60)]);
    let mut state = PianoRollState { notes_replace: false, ..span_state(&clip, 0.0, 2.0) };
    let paste = state.key_action("selection.paste", &clip, 0.0, Some(1)).map(|a| a.params);
    assert_eq!(paste, Some(json!({ "track": 1, "index": 0, "time": 1.0 })));
    assert_eq!(state.span.as_ref().map(|s| (s.start, s.end)), Some((0.0, 2.0)), "the time selection stays");
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

/// The stretch a rectangle selected follows the notes selected with it: made longer or shorter,
/// an end on theirs goes along; an empty part before them stays, growing to take in a note that
/// now starts earlier; moved by a key, it goes along once, not again when the notes land.
#[test]
fn a_time_selection_follows_the_notes_selected_with_it() {
    let chord = |length: f64| clip(vec![note(1.0, length, 60), note(1.0, length, 64)]);
    let selected = |start: f64, end: f64| {
        let mut state = focused_on(&chord(0.25));
        state.selected = BTreeSet::from([0, 1]);
        state.select_span(start, end);
        state.settle_selection(&chord(0.25).notes);
        state
    };
    let span = |state: &PianoRollState| state.span.as_ref().map(|s| (s.start, s.end));
    let mut state = selected(1.0, 1.25);
    // Shift and Right made both a beat longer, then shorter again.
    state.settle_selection(&chord(1.25).notes);
    assert_eq!(span(&state), Some((1.0, 2.25)), "the end follows the notes'");
    assert_eq!(state.span_shown(&chord(1.25)), Some((1.0, 2.25)), "and so does the bar");
    state.settle_selection(&chord(0.25).notes);
    assert_eq!(span(&state), Some((1.0, 1.25)));

    // An empty part before the notes stays, and the stretch grows to keep a note now earlier in.
    let mut state = selected(0.5, 1.25);
    state.settle_selection(&clip(vec![note(0.25, 1.0, 60), note(1.0, 0.25, 64)]).notes);
    assert_eq!(span(&state), Some((0.25, 1.25)));

    // A key moves it along at once; the notes landing there move it no further.
    let mut state = selected(1.0, 1.25);
    let beats = state.key_action("pianoRoll.later", &chord(0.25), 0.0, None).and_then(|a| a.params["beats"].as_f64());
    let beats = beats.unwrap_or_default();
    assert!(beats > 0.0);
    assert_eq!(span(&state), Some((1.0 + beats, 1.25 + beats)));
    state.settle_selection(&clip(vec![note(1.0 + beats, 0.25, 60), note(1.0 + beats, 0.25, 64)]).notes);
    assert_eq!(span(&state), Some((1.0 + beats, 1.25 + beats)), "moved once");
}

/// A longer key from a triplet line a command rounded goes on to the next line.
#[test]
fn a_longer_key_goes_on_along_triplet_lines() {
    let grid = TimelineGrid { step: 1.0 / 3.0, bar_beats: 4.0, snaps: true };
    let first = note(0.0, 0.333333, 60);
    assert!((key_step_end(&first, grid, true) - 2.0 / 3.0).abs() < 1e-9);
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

/// A step longer runs over the next note of the key, which, with stacked notes off, makes room once
/// the note is let go.
#[test]
fn a_fine_step_longer_runs_over_the_next_note_of_the_key() {
    let clip = clip(vec![note(1.0, 1.24, 60), note(2.25, 1.0, 60)]);
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([0]);
    let longer = state.key_action("pianoRoll.longerFine", &clip, 0.0, None).map(|a| a.params);
    let length = longer.and_then(|p| p["lengths"][0].as_f64()).unwrap_or(0.0);
    assert!((length - 1.27125).abs() < 1e-6, "{length}");
    assert!(state.settle_pending, "the next note makes room once it is let go");
}

#[test]
fn a_key_resize_makes_room_once_let_go_unless_notes_stack() {
    let clip = clip(vec![note(0.0, 1.0, 60)]);
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([0]);
    let longer = state.key_action("pianoRoll.longer", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(longer.map(|p| p["replace"].clone()), Some(Value::Null), "not while selected");
    assert!(state.settle_pending, "but once let go");
    state.settle_pending = false;
    state.notes_replace = false;
    let longer = state.key_action("pianoRoll.longer", &clip, 0.0, None).map(|a| a.params);
    assert_eq!(longer.map(|p| p["replace"].clone()), Some(Value::Null));
    assert!(!state.settle_pending, "stacked notes allowed: nothing to make room for");
}

/// Selected notes edited with stacked notes off make room under them once they are let go, and
/// only where they overlap a note of their key: a move through other notes erases nothing on
/// the way.
#[test]
fn selected_notes_make_room_once_let_go_and_only_where_they_overlap() {
    let on_top = clip(vec![note(0.0, 1.0, 62), note(0.0, 1.0, 62)]);
    let mut state = focused_on(&on_top);
    state.selected = BTreeSet::from([0]);
    state.settle_pending = true;
    let mut actions = Vec::new();
    state.settle_frame(&on_top, &mut actions);
    state.settle_frame(&on_top, &mut actions);
    assert!(actions.is_empty(), "nothing while the note stays selected");
    state.selected.clear();
    state.settle_frame(&on_top, &mut actions);
    assert_eq!(actions.iter().map(|a| a.id).collect::<Vec<_>>(), ["note.makeRoom"]);
    assert_eq!(actions[0].params["notes"].as_array().map(Vec::len), Some(1), "by id");
    // Moved off again before being let go: nothing to make room for.
    let apart = clip(vec![note(0.0, 1.0, 62), note(0.0, 1.0, 60)]);
    let mut state = focused_on(&apart);
    state.selected = BTreeSet::from([0]);
    state.settle_pending = true;
    let mut actions = Vec::new();
    state.settle_frame(&apart, &mut actions);
    state.settle_frame(&apart, &mut actions);
    state.selected.clear();
    state.settle_frame(&apart, &mut actions);
    assert!(actions.is_empty(), "no overlap, no command");
    // Deleted while waiting: nothing to make room for.
    let mut state = focused_on(&on_top);
    state.selected = BTreeSet::from([0]);
    state.settle_pending = true;
    let mut actions = Vec::new();
    state.settle_frame(&on_top, &mut actions);
    state.settle_frame(&on_top, &mut actions);
    assert!(state.delete_selected().is_some());
    state.settle_frame(&on_top, &mut actions);
    assert!(actions.is_empty(), "a deleted note makes no room");
}

/// Notes a key moves out of the grid are scrolled back into view, on the frame the move lands and
/// the one after; notes in view leave the scroll alone.
#[test]
fn notes_a_key_moves_stay_in_view() {
    let far = clip(vec![note(30.0, 1.0, 20)]);
    let mut state = focused_on(&far);
    state.selected = BTreeSet::from([0]);
    let grid = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0));
    let rows = KeyRows::default();
    assert!(state.key_action("pianoRoll.later", &far, 0.0, None).is_some());
    assert_eq!(state.reveal_selection, 2);
    layout::reveal_selection(&mut state, &far, &rows, grid);
    let (x, y) = (state.scroll.x, state.scroll.y);
    let end = 31.0 * state.beat_width;
    assert!(x <= 30.0 * state.beat_width && end <= x + 400.0, "beats 30 to 31 in view: {x}");
    let row = rows.row_of(20).unwrap_or(0) as f32 * state.key_height;
    assert!(y <= row && row + state.key_height <= y + 200.0, "the key's row in view: {y}");
    // Once it is in view, the next frame leaves the scroll as it is; then nothing follows.
    layout::reveal_selection(&mut state, &far, &rows, grid);
    assert_eq!((state.scroll.x, state.scroll.y), (x, y));
    assert_eq!(state.reveal_selection, 0);
}

/// A move dragged past the grid's right edge scrolls the view, and the drag's start moves back by as
/// much, so the notes go on following the pointer; a drag inside the grid scrolls nothing.
#[test]
fn a_drag_at_the_grid_edge_scrolls_and_its_start_follows() {
    let grid = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0));
    let mut state = PianoRollState {
        gesture: Some(Gesture::Move { from: Pos2::new(100.0, 50.0), grabbed: 0, copy: false }),
        ..PianoRollState::default()
    };
    layout::scroll_at_edges(&mut state, grid, Pos2::new(200.0, 100.0));
    assert_eq!(state.scroll, Vec2::ZERO, "inside: nothing");
    layout::scroll_at_edges(&mut state, grid, Pos2::new(440.0, 100.0));
    assert!(state.scroll.x > 0.0);
    let Some(Gesture::Move { from, .. }) = state.gesture else { panic!("still moving") };
    assert_eq!(from.x, 100.0 - state.scroll.x, "the start moves back by the scroll");
    // A paint stroke does not scroll.
    state.gesture = Some(Gesture::Paint);
    let before = state.scroll;
    layout::scroll_at_edges(&mut state, grid, Pos2::new(440.0, 100.0));
    assert_eq!(state.scroll, before);
}

/// Stacked notes turned on while edited notes wait to make room: once let go, they make none, and
/// nothing waits for a later let-go either.
#[test]
fn turning_stacked_notes_on_drops_the_room_waiting_to_be_made() {
    let on_top = clip(vec![note(0.0, 1.0, 62), note(0.0, 1.0, 62)]);
    let mut state = focused_on(&on_top);
    state.selected = BTreeSet::from([0]);
    state.settle_pending = true;
    let mut actions = Vec::new();
    state.settle_frame(&on_top, &mut actions);
    state.settle_frame(&on_top, &mut actions);
    state.notes_replace = false;
    state.selected.clear();
    state.settle_frame(&on_top, &mut actions);
    assert!(actions.is_empty(), "{actions:?}");
    state.notes_replace = true;
    state.settle_frame(&on_top, &mut actions);
    assert!(actions.is_empty(), "nothing left to settle later: {actions:?}");
    assert!(state.settle_on_hide().is_none());
}

/// Notes let go on the first frame after their edit still make room, and so do notes whose clip
/// is no longer shown (another clip picked): by id, in their own clip.
#[test]
fn edited_notes_make_room_when_let_go_at_once_or_when_another_clip_shows() {
    let on_top = clip(vec![note(0.0, 1.0, 62), note(0.0, 1.0, 62)]);
    // Let go (Escape) on the very first frame after the edit.
    let mut state = focused_on(&on_top);
    state.selected = BTreeSet::from([0]);
    state.settle_pending = true;
    let mut actions = Vec::new();
    state.settle_frame(&on_top, &mut actions);
    state.selected.clear();
    state.settle_frame(&on_top, &mut actions);
    assert_eq!(actions.iter().map(|a| a.id).collect::<Vec<_>>(), ["note.makeRoom"]);
    // Another clip shown before the notes were let go.
    let mut state = focused_on(&on_top);
    state.selected = BTreeSet::from([0]);
    state.settle_pending = true;
    let mut actions = Vec::new();
    state.settle_frame(&on_top, &mut actions);
    state.settle_frame(&on_top, &mut actions);
    let mut other = clip(vec![note(0.0, 1.0, 62)]);
    other.address = json!({ "track": 2, "index": 0 });
    state.selected.clear();
    state.settle_frame(&other, &mut actions);
    assert_eq!(actions.iter().map(|a| a.id).collect::<Vec<_>>(), ["note.makeRoom"]);
    assert_eq!(actions[0].params["track"], json!(1), "in the clip the notes are in");
    assert_eq!(actions[0].params["notes"].as_array().map(Vec::len), Some(1), "by id");
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

/// Alt+L is the Notes tab's Legato: the selected notes until the next note after them, or every
/// note of the clip with none selected, the last until the clip's end.
#[test]
fn the_legato_key_acts_on_the_selection_or_the_whole_clip() {
    let two = clip_of_two();
    let mut state = focused_on(&two);
    state.selected = BTreeSet::from([0]);
    let legato = state.key_action("pianoRoll.legato", &two, 0.0, None).expect("a command");
    assert_eq!(legato.id, "note.legato");
    assert_eq!((&legato.params["notes"], &legato.params["until"]), (&json!([0]), &json!(5.0)));
    state.selected.clear();
    let whole = state.key_action("pianoRoll.legato", &two, 0.0, None).expect("a command");
    assert!(whole.params.get("notes").is_none(), "the whole clip: {}", whole.params);
    assert_eq!(whole.params["until"], json!(8.0));
    // An empty clip gives it nothing to do.
    let empty = clip(Vec::new());
    let mut state = focused_on(&empty);
    assert!(state.key_action("pianoRoll.legato", &empty, 0.0, None).is_none());
}

/// Ctrl+L in the piano roll is the clip's own loop: over the time selection, off again when the
/// loop is on over that very stretch, and with no time selection the clip's loop on or off.
#[test]
fn the_loop_key_loops_the_clip_over_the_time_selection_or_switches_its_loop() {
    let mut two = clip_of_two();
    let mut state = focused_on(&two);
    let on = state.key_action("pianoRoll.loop", &two, 0.0, None).expect("a command");
    assert_eq!(on.id, "clip.setLoop");
    assert_eq!(on.params["loop"], json!(true), "no loop yet: on");
    assert!(on.params.get("start").is_none());
    two.shape.loop_range = Some((0.0, 8.0));
    let off = state.key_action("pianoRoll.loop", &two, 0.0, None).expect("a command");
    assert_eq!(off.params["loop"], json!(false), "looping: off");

    let mut state = span_state(&two, 2.0, 4.0);
    let over = state.key_action("pianoRoll.loop", &two, 0.0, None).expect("a command");
    assert_eq!((&over.params["start"], &over.params["end"]), (&json!(2.0), &json!(4.0)));
    assert!(over.params.get("loop").is_none(), "a range turns the loop on by itself: {}", over.params);
    two.shape.loop_range = Some((2.0, 4.0));
    let again = state.key_action("pianoRoll.loop", &two, 0.0, None).expect("a command");
    assert_eq!(again.params["loop"], json!(false), "the same stretch again takes the loop away");
    assert!(again.params.get("start").is_none());
}

/// What the Arrangement lights on the clip: a rectangle still being drawn as it covers now, else
/// the time selection; nothing without either.
#[test]
fn the_time_span_is_the_rectangle_while_it_is_drawn() {
    let one = clip(vec![note(1.0, 0.5, 60)]);
    let mut state = focused_on(&one);
    assert_eq!(state.time_span(), None);
    state.live_span = Some((0.5, 1.5));
    assert_eq!(state.time_span(), Some((0.5, 1.5)), "as the rectangle grows");
    let mut state = span_state(&one, 2.0, 4.0);
    assert_eq!(state.time_span(), Some((2.0, 4.0)), "let go: the time selection");
    state.live_span = Some((0.0, 1.0));
    assert_eq!(state.time_span(), Some((0.0, 1.0)), "a new rectangle over it");
}

/// Invert selection: the notes that were not selected, and no others; the time selection ends.
#[test]
fn invert_selection_selects_the_others() {
    let clip = clip(vec![note(0.0, 0.5, 60), note(1.0, 0.5, 62), note(2.0, 0.5, 64)]);
    let mut state = focused_on(&clip);
    state.selected = BTreeSet::from([1]);
    state.select_span(1.0, 1.5);
    state.invert_selection(&clip);
    assert_eq!(state.selected, BTreeSet::from([0, 2]));
    assert!(state.span.is_none());
}

/// The pointer's icon is the toolbar button's: the same theme role, so a theme's artwork for the
/// button is the pointer's too.
#[test]
fn the_tool_pointer_takes_the_toolbars_theme_role() {
    assert_eq!(tool_role(IconButton::Draw), "piano.draw");
    assert_eq!(tool_role(IconButton::Slice), "piano.slice");
}

/// Join, Fit, Crop and Split clip (#1284): the keys send their commands, and do nothing where
/// they have nothing to act on (Split clip never at the playhead, only at a time selection).
#[test]
fn the_clip_edit_keys_send_their_commands() {
    let notes = vec![note(0.0, 1.0, 60), note(1.0, 1.0, 60), note(4.0, 1.0, 64)];
    let piano = clip(notes);
    let mut state = focused_on(&piano);
    assert!(state.key_action("pianoRoll.join", &piano, 0.0, None).is_none(), "nothing selected");
    state.selected = BTreeSet::from([0, 1]);
    let join = state.key_action("pianoRoll.join", &piano, 0.0, None).expect("note.join");
    assert_eq!((join.id, join.params["notes"].clone()), ("note.join", json!([0, 1])));
    // No time selection: Fit, Crop and Split clip do nothing, whatever the playhead.
    state.selected = BTreeSet::from([0, 1]);
    for key in ["pianoRoll.fitToTime", "pianoRoll.crop", "pianoRoll.splitClip"] {
        assert!(state.key_action(key, &piano, 18.0, None).is_none(), "{key}");
    }
    // A time selection with no note selected: the notes that start in it (the third) fill it. A
    // selection's own time selection always holds its notes, so it is left out here.
    state.selected.clear();
    state.select_span(2.0, 6.0);
    let fit = state.key_action("pianoRoll.fitToTime", &piano, 0.0, None).expect("note.fitToTime");
    assert_eq!((fit.params["start"].as_f64(), fit.params["end"].as_f64()), (Some(2.0), Some(6.0)));
    assert_eq!(fit.params["notes"], json!([2]));
    state.selected.clear();
    state.select_span(2.0, 6.0);
    // The clip plays from arrangement beat 16: its content's beats 2 and 6 are beats 18 and 22.
    let split = state.key_action("pianoRoll.splitClip", &piano, 0.0, None).expect("clip.split");
    assert_eq!((split.id, split.params["ats"].clone()), ("clip.split", json!([18.0, 22.0])));
    state.selected.clear();
    state.select_span(2.0, 6.0);
    let crop = state.key_action("pianoRoll.crop", &piano, 0.0, None).expect("clip.crop");
    assert_eq!(crop.params, json!({ "track": 1, "index": 0, "start": 2.0, "end": 6.0 }));
}

/// A slot's clip has no lane to put the parts on: Split clip leaves it alone.
#[test]
fn split_clip_leaves_a_slot_clip_alone() {
    let mut piano = clip(vec![note(0.0, 1.0, 60)]);
    piano.address = json!({ "track": 1, "scene": 0 });
    let mut state = focused_on(&piano);
    state.selected = BTreeSet::from([0]);
    state.select_span(1.0, 2.0);
    assert!(state.key_action("pianoRoll.splitClip", &piano, 0.0, None).is_none());
}

/// Join and Crop leave the selection alone where the command would refuse them: notes that do not
/// touch, a time selection outside the part of the clip that plays.
#[test]
fn join_and_crop_keep_the_selection_where_they_would_be_refused() {
    let apart = clip(vec![note(0.0, 1.0, 60), note(2.0, 1.0, 60), note(0.0, 1.0, 64)]);
    let mut state = focused_on(&apart);
    state.selected = BTreeSet::from([0, 1, 2]);
    assert!(state.key_action("pianoRoll.join", &apart, 0.0, None).is_none());
    assert_eq!(state.selected, BTreeSet::from([0, 1, 2]), "kept to try again");
    state.selected.clear();
    state.select_span(9.0, 10.0);
    assert!(state.key_action("pianoRoll.crop", &apart, 0.0, None).is_none());
    assert!(state.span.is_some(), "the time selection stays");
    // Sent, the selection stays too: a crop the command refuses (a stretch too short) keeps it.
    state.selected = BTreeSet::from([0]);
    state.select_span(0.0, 1.0);
    assert!(state.key_action("pianoRoll.crop", &apart, 0.0, None).is_some());
    assert_eq!(state.selected, BTreeSet::from([0]));
    assert!(state.span.is_some());
    assert!(PianoRollState::joins(&[note(0.0, 1.0, 60), note(1.0, 1.0, 60)], &BTreeSet::from([0, 1])));
}
