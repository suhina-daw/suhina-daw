// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn dropping_after_an_open_group_uses_its_last_visible_descendants_full_span() {
    let project = crate::demo::grouped_project();
    let timeline = Timeline::build(&project, None);
    let lane = |name: &str| timeline.lanes.iter().find(|l| l.name == name).unwrap();
    let row = |name: &str, top: f32, under: f32| {
        let time = Rect::from_min_size(Pos2::new(0.0, top), Vec2::new(600.0, 60.0));
        Row {
            track: lane(name).track,
            clips: lane(name).track,
            time,
            clip: time,
            span: egui::Rangef::new(top, top + 61.0 + under),
            folded: None,
        }
    };
    let rows = [row("Group", 0.0, 0.0), row("Drums", 61.0, 0.0), row("Bass", 122.0, 80.0), row("Keys", 263.0, 0.0)];
    assert_eq!(track_drop_after(&rows[0], &rows, &timeline.lanes), 262.5, "after Bass and its subordinate lanes");
    let folded = Row { folded: Some((0, 2)), ..rows[0] };
    assert_eq!(track_drop_after(&folded, &[folded, rows[3]], &timeline.lanes), 60.5);
    assert_eq!(track_drop_after(&rows[2], &rows, &timeline.lanes), 182.5, "an ordinary track keeps its row boundary");
}

#[test]
fn picked_clips_follow_their_move_to_another_track() {
    let mut state = ArrangementViewState::default();
    let (a, b, c) = (ClipId(1), ClipId(2), ClipId(3));
    state.select_clips(vec![(TrackId(1), a), (TrackId(1), b)]);
    state.clips_moved(&[(TrackId(2), a), (TrackId(2), b), (TrackId(2), c)]);
    assert_eq!(state.selected_clips(), [(TrackId(2), a), (TrackId(2), b)]);
    assert_eq!(state.selected_clip(), Some((TrackId(2), a)));
}

#[test]
fn shift_click_takes_the_rectangle_from_the_anchor_across_tracks() {
    // Two tracks alike, each with a clip at bar 1 and one at bar 5 (#694).
    let spot = |track: u32, clip: u32, row: usize, start: f64| ClipSpot {
        key: (TrackId(track), ClipId(clip)),
        row,
        start,
        end: start + 16.0,
    };
    let order = [spot(1, 1, 0, 0.0), spot(1, 2, 0, 16.0), spot(2, 3, 1, 0.0), spot(2, 4, 1, 16.0)];
    let mut state = ArrangementViewState::default();
    state.pick_clip((TrackId(1), ClipId(2)), false, false, &order);
    state.pick_clip((TrackId(2), ClipId(4)), true, false, &order);
    assert_eq!(state.selected, [(TrackId(1), ClipId(2)), (TrackId(2), ClipId(4))], "the later clip on both");
    state.pick_clip((TrackId(1), ClipId(2)), false, false, &order);
    state.pick_clip((TrackId(2), ClipId(3)), true, false, &order);
    assert_eq!(state.selected.len(), 4, "from bar 1 to bar 9 on both tracks");
}

#[test]
fn zoom_reaches_ten_times_further_in_than_before_and_stops() {
    let mut width = DEFAULT_BEAT_WIDTH;
    for _ in 0..100 {
        width = zoomed(width, 1.5, 1.0);
    }
    assert_eq!(width, 4000.0);
    for _ in 0..100 {
        width = zoomed(width, 1.0 / 1.5, 1.0);
    }
    assert_eq!(width, 1.0);
}

#[test]
fn a_long_project_zooms_out_until_it_is_seen_whole() {
    // 3 400 beats (eight songs end to end) in a view 1 200 points wide: 0.35 points a beat.
    let fits = 1_200.0 / 3_400.0;
    let mut width = DEFAULT_BEAT_WIDTH;
    for _ in 0..100 {
        width = zoomed(width, 1.0 / 1.5, fits);
    }
    assert!((width - fits).abs() < 1e-6, "{width}");
    // A short one stops at a point a beat, as before.
    assert_eq!(zoomed(1.0, 0.5, 30.0), 1.0);
}

#[test]
fn volume_sits_at_85_percent_for_0_db() {
    assert_eq!(volume_position(f64::NEG_INFINITY), 0.0);
    assert_eq!(volume_position(-70.0), 0.0);
    assert!((volume_position(0.0) - 0.85).abs() < 1e-6);
    assert!((volume_position(6.0) - 1.0).abs() < 1e-6);
    assert!(volume_position(-6.0) < volume_position(-3.0));
}

#[test]
fn alt_and_the_wheel_zoom_every_lane_within_its_limits() {
    let timeline = Timeline::build(&crate::demo::demo_project(), None);
    let tracks: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
    let mut state = ArrangementViewState::default();
    state.set_lane_height(tracks[0].track, 100.0);
    state.zoom_lanes(&timeline.lanes, 2.0, 0.0);
    assert_eq!(state.height(tracks[0]), 200.0);
    assert!(tracks[1..].iter().all(|l| state.height(l) == DEFAULT_LANE * 2.0));
    // A track added after the zoom comes in as tall as the zoomed ones, not at the default.
    assert_eq!(state.lane_height(TrackId(999)), DEFAULT_LANE * 2.0);
    // The content under the pointer stays put: what was 10 points down is now 20.
    state.scroll.y = 0.0;
    state.zoom_lanes(&timeline.lanes, 0.5, 10.0);
    assert_eq!(state.scroll.y, 0.0);
    state.scroll.y = 40.0;
    state.zoom_lanes(&timeline.lanes, 2.0, 10.0);
    assert!((state.scroll.y - 90.0).abs() < 1.0, "{}", state.scroll.y);
    for _ in 0..50 {
        state.zoom_lanes(&timeline.lanes, 0.5, 0.0);
    }
    assert!(tracks.iter().all(|l| state.height(l) == MIN_LANE));
    assert_eq!(state.lane_height(TrackId(999)), MIN_LANE);
    state.zoom_lanes(&timeline.lanes, f32::NAN, 0.0);
    assert!(tracks.iter().all(|l| state.height(l) == MIN_LANE));
}

fn audio_clip() -> ClipBox {
    let timeline = Timeline::build(&crate::demo::demo_project(), None);
    let clip = timeline.lanes.iter().flat_map(|l| &l.clips).next().cloned().unwrap();
    ClipBox { start: 2.0, length: 8.0, fade_in: 1.0, fade_out: 0.0, audio: true, ..clip }
}

#[test]
fn a_folded_group_hides_its_tracks_and_shows_the_one_it_stands_in_for() {
    let project = crate::demo::grouped_project();
    let timeline = Timeline::build(&project, None);
    let track = |name: &str| timeline.lanes.iter().position(|l| l.name == name).unwrap();
    let (group, drums, bass) = (track("Group"), track("Drums"), track("Bass"));
    let open = drawn_lanes(&timeline.lanes, &Folds::default());
    assert_eq!(
        open.iter().map(|d| d.lane).collect::<Vec<_>>(),
        [group, drums, bass, track("Keys"), track("Vox")],
        "a project with no folds draws every lane, and returns are not lanes"
    );
    assert_eq!(open[0].children, 2, "the group holds Drums and Bass");
    assert!(!open[0].folded);

    let mut folds = Folds::default();
    folds.fold(timeline.lanes[group].track);
    folds.reconcile(&project, None);
    let folded = drawn_lanes(&timeline.lanes, &folds);
    assert_eq!(folded.iter().map(|d| d.lane).collect::<Vec<_>>(), [group, track("Keys"), track("Vox")]);
    assert!(folded[0].folded);
    assert_eq!(folded[0].clips, drums, "its row shows the first track inside it");
    assert_eq!(folded[0].children, 2);

    // Working on Bass (its header, or its clip) turns the group's row to it.
    folds.reconcile(&project, Some(timeline.lanes[bass].track));
    assert_eq!(drawn_lanes(&timeline.lanes, &folds)[0].clips, bass);
}

#[test]
fn a_selection_over_a_folded_groups_row_takes_every_track_inside_it() {
    let project = crate::demo::grouped_project();
    let timeline = Timeline::build(&project, None);
    let track = |name: &str| timeline.lanes.iter().position(|l| l.name == name).unwrap();
    let (group, drums, bass) = (track("Group"), track("Drums"), track("Bass"));
    let mut folds = Folds::default();
    folds.fold(timeline.lanes[group].track);
    folds.reconcile(&project, None);
    let shown = drawn_lanes(&timeline.lanes, &folds)[0];
    assert_eq!(shown.covered(), group..bass + 1);

    let time = Rect::from_min_size(Pos2::ZERO, Vec2::new(10_000.0, 40.0));
    let row = |folded| Row {
        track: timeline.lanes[group].track,
        clips: timeline.lanes[drums].track,
        time,
        clip: time,
        span: time.y_range(),
        folded,
    };
    let folded = row(Some((group, shown.children)));
    let tracks: Vec<TrackId> = [group, drums, bass].iter().map(|&i| timeline.lanes[i].track).collect();
    // Cut or Delete over the row's time acts on the group and both tracks, not the one it shows.
    assert_eq!(row_tracks(&folded, &timeline.lanes), tracks);
    assert_eq!(row_tracks(&row(None), &timeline.lanes), [timeline.lanes[drums].track]);

    // A selection rectangle over the whole row picks the clips of every track inside it.
    let x_of = |beat: f64| beat as f32 * 10.0;
    let picked = row_picks(&folded, &timeline.lanes, time, &x_of);
    for lane in [drums, bass] {
        let lane = &timeline.lanes[lane];
        assert!(!lane.clips.is_empty(), "{} has clips to pick", lane.name);
        assert!(lane.clips.iter().all(|c| picked.contains(&(lane.track, c.id))), "every clip of {}", lane.name);
    }
    assert!(row_picks(&row(None), &timeline.lanes, time, &x_of).iter().all(|(t, _)| *t == timeline.lanes[drums].track));
}

#[test]
fn a_fold_inside_a_fold_hides_exactly_what_it_covers() {
    let lane = |track: u32, kind: ColumnKind, depth: usize| Lane {
        folder: false,
        track: TrackId(track),
        name: format!("T{track}"),
        color: suhina_engine::model::Rgb(0, 0, 0),
        kind,
        notes: false,
        depth,
        parent: None,
        position: track as usize,
        mute: false,
        solo: false,
        cue: false,
        armable: false,
        armed: false,
        midi_input: Default::default(),
        plays_notes: false,
        plays_audio: false,
        audio_input: Default::default(),
        volume_db: 0.0,
        pan: 0.0,
        sends: Vec::new(),
        output: None,
        output_name: None,
        column: None,
        overrides: false,
        clips: Vec::new(),
    };
    // An outer group (10) holding a group (11) that holds a track (12), and a track (13)
    // after the outer one.
    let lanes = [
        lane(10, ColumnKind::Group, 0),
        lane(11, ColumnKind::Group, 1),
        lane(12, ColumnKind::Track, 2),
        lane(13, ColumnKind::Track, 0),
    ];
    let drawn = |folds: &Folds| drawn_lanes(&lanes, folds).iter().map(|d| d.lane).collect::<Vec<_>>();
    assert_eq!(drawn(&Folds::default()), [0, 1, 2, 3]);
    let mut outer = Folds::default();
    outer.fold(TrackId(10));
    assert_eq!(drawn(&outer), [0, 3], "an outer fold hides the group inside it and its track");
    assert_eq!(drawn_lanes(&lanes, &outer)[0].children, 2, "the whole family is inside it");
    let mut inner = Folds::default();
    inner.fold(TrackId(11));
    assert_eq!(drawn(&inner), [0, 1, 3], "an inner fold hides only its own track");
    // A track is not a group: asking to fold one changes nothing.
    let mut not_a_group = Folds::default();
    not_a_group.fold(TrackId(13));
    assert_eq!(drawn(&not_a_group), [0, 1, 2, 3]);
}

#[test]
fn a_dragged_gain_or_fade_is_what_the_clip_shows() {
    let clip = audio_clip();
    let key = (TrackId(1), ClipId(1));
    let mut state = ArrangementViewState::default();
    let linear = FadeCurve::default();
    assert_eq!(
        state.shown(key, &clip, Some(beats(1.0))),
        Shown { gain_db: 0.0, fade_in: 1.0, fade_out: 0.0, fade_in_curve: linear, fade_out_curve: linear }
    );
    // A curve being dragged bends that fade only.
    assert!(state.gesture.begin(Gesture::FadeCurve(CurveDrag { key, edge: FadeEdge::In, curve: 0.4, grab: 0.0 })));
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_in_curve.curve, 0.4);
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_out_curve.curve, 0.0);
    state.gesture.settle(false);
    // The waveform is drawn with the gain being dragged, not the one in the project.
    assert!(state.gesture.begin(Gesture::Gain((key, -12.0))));
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).gain_db, -12.0);
    assert_eq!(state.shown((TrackId(1), ClipId(2)), &clip, Some(beats(1.0))).gain_db, 0.0, "only on the dragged clip");
    // A fade being dragged: on the grid, or free with Alt.
    state.gesture.settle(false);
    assert!(state.gesture.begin(Gesture::Fade(FadeDrag { key, edge: FadeEdge::Out, raw: 2.6, from: 0.0 })));
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_out, 3.0);
    assert!((state.shown(key, &clip, None).fade_out - 2.6).abs() < 1e-9);
}

#[test]
fn a_second_drag_cannot_start_while_one_is_under_way() {
    let key = (TrackId(1), ClipId(1));
    let mut slot = GestureSlot::default();
    let edge = EdgeDrag { key, grab: EdgeGrab::Right, raw: 4.0, from: 4.0, origin: 100.0 };
    assert!(slot.begin(Gesture::Edge(edge)));
    // A fade handle, a marker or the loop pressed meanwhile starts nothing: the edge drag stays.
    assert!(!slot.begin(Gesture::Fade(FadeDrag { key, edge: FadeEdge::In, raw: 0.0, from: 0.0 })));
    assert!(!slot.begin(Gesture::Marker((0, 2.0))));
    assert_eq!(slot.edge(), Some(&edge));
    assert_eq!(slot.fade(), None);
    // Another clip's edge does not end it; its own does, handing back what it carried.
    let other = (TrackId(1), ClipId(2));
    assert_eq!(slot.end_edge(|d| d.key == other), None);
    assert!(slot.is_some());
    assert_eq!(slot.end_edge(|d| d.key == key), Some(edge));
    // Then the next drag starts.
    assert!(slot.begin(Gesture::Marker((0, 2.0))));
    // Escape ends it without handing anything back, and the rest of its press starts nothing.
    assert!(slot.cancel());
    assert!(!slot.is_some());
    assert!(slot.cancelled());
    assert!(!slot.begin(Gesture::Marker((1, 3.0))), "the press Escape let go of starts no drag");
    slot.settle(true);
    assert!(slot.cancelled(), "a button still held keeps it so");
    // Once every button is up the next press may start one; a drag that lost its release ends then.
    slot.settle(false);
    assert!(!slot.cancelled());
    assert!(slot.begin(Gesture::Marker((1, 3.0))));
    slot.settle(false);
    assert!(!slot.is_some());
}

#[test]
fn losing_the_window_focus_ends_the_drag_and_holds_no_press_back() {
    let mut slot = GestureSlot::default();
    assert!(slot.begin(Gesture::Marker((0, 2.0))));
    slot.lose_focus();
    assert!(!slot.is_some());
    assert!(!slot.cancelled());
    assert!(slot.begin(Gesture::Marker((1, 3.0))), "the first press after the focus returns starts a drag");
    // Escape, then the focus lost with the button still reported down: the press is over too.
    assert!(slot.cancel());
    slot.lose_focus();
    assert!(slot.begin(Gesture::Marker((2, 4.0))));
}

#[test]
fn escape_ends_the_drag_and_its_held_press() {
    let mut state = ArrangementViewState::default();
    assert!(!state.gesture_under_way());
    assert!(state.gesture.begin(Gesture::Track(crate::interaction::nesting::TrackDrag {
        source: TrackId(3),
        tracks: vec![TrackId(3)]
    })));
    state.pressed_clip = Some((TrackId(1), ClipId(1)));
    assert!(state.gesture_under_way());
    assert!(state.cancel_gesture());
    assert!(!state.gesture_under_way());
    assert_eq!(state.dragged_clip(), None);
}

#[test]
fn a_fade_ends_on_the_grid_and_leaves_room_for_the_other() {
    let clip = audio_clip();
    // The fade-in's end snaps to the grid in project beats: the clip starts at beat 2.
    assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, Some(beats(4.0))), 2.0);
    assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, None), 1.3);
    // The fade-out's start snaps: the clip ends at beat 10.
    assert_eq!(fade_length(1.3, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 2.0);
    // Never below 0, never past the other fade.
    assert_eq!(fade_length(-5.0, FadeEdge::In, &clip, 0.0, None), 0.0);
    assert_eq!(fade_length(7.5, FadeEdge::In, &clip, 3.0, None), 5.0);
    assert_eq!(fade_length(f64::NAN, FadeEdge::In, &clip, 0.0, None), 0.0);
    // The clip's own ends snap too: from beat 10 back to beat 2.6, nearer the clip's start
    // (beat 2) than the grid line at beat 4, the fade-out takes the whole clip, and the
    // fade-in from beat 2 to 9.5 takes the room the fade-out leaves.
    assert_eq!(fade_length(7.4, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 8.0);
    assert_eq!(fade_length(7.4, FadeEdge::Out, &clip, 1.0, Some(beats(4.0))), 7.0);
    assert_eq!(fade_length(7.5, FadeEdge::In, &clip, 0.0, Some(beats(4.0))), 8.0);
    assert_eq!(fade_length(0.4, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 0.0);
    // Nearer the grid line, the grid wins.
    assert_eq!(fade_length(6.5, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 6.0);
}

#[test]
fn the_envelope_button_gives_way_to_the_fade_in_handle() {
    let button = Rect::from_min_size(Pos2::new(104.0, 12.0), Vec2::splat(14.0));
    // The handle at the clip's corner lies over the button's left edge: the button keeps the rest.
    let corner = Rect::from_min_size(Pos2::new(98.0, 8.0), Vec2::splat(11.0));
    assert_eq!(beside(button, corner), Rect::from_min_max(Pos2::new(109.0, 12.0), button.max));
    // Over its right edge (a fade that ends there): the button keeps its left part.
    let right = Rect::from_min_size(Pos2::new(114.0, 8.0), Vec2::splat(11.0));
    assert_eq!(beside(button, right), Rect::from_min_max(button.min, Pos2::new(114.0, 26.0)));
    // Apart: all of it.
    let apart = Rect::from_min_size(Pos2::new(140.0, 8.0), Vec2::splat(11.0));
    assert_eq!(beside(button, apart), button);
}

#[test]
fn fade_handles_follow_the_fade_and_stay_inside_the_clip() {
    let clip = Rect::from_min_size(Pos2::new(100.0, 10.0), Vec2::new(80.0, 40.0));
    assert_eq!(fade_handle(clip, FadeEdge::In, 0.0).min, Pos2::new(100.0, 10.0));
    assert_eq!(fade_handle(clip, FadeEdge::Out, 0.0).max.x, 180.0);
    assert_eq!(fade_handle(clip, FadeEdge::In, 20.0).min.x, 120.0);
    assert_eq!(fade_handle(clip, FadeEdge::Out, 500.0).min.x, 100.0);
    assert_eq!(fade_handle(clip, FadeEdge::In, f32::NAN).min.x, 100.0);
    // Linear: half-way into the fade-in, half the level; full in between the fades.
    let linear = FadeCurve::default();
    let shown = Shown { gain_db: 0.0, fade_in: 1.0, fade_out: 2.0, fade_in_curve: linear, fade_out_curve: linear };
    assert_eq!(fade_gain(0.5, 8.0, &shown), 0.5);
    assert_eq!(fade_gain(4.0, 8.0, &shown), 1.0);
    assert_eq!(fade_gain(7.0, 8.0, &shown), 0.5);
    // Other shapes and curves follow the spec's formula.
    let slow = FadeCurve { shape: FadeShape::Slow, curve: 0.0 };
    let shown = Shown { fade_in_curve: slow, fade_out_curve: FadeCurve { curve: 1.0, ..slow }, ..shown };
    assert_eq!(fade_gain(0.5, 8.0, &shown), 0.25);
    assert!((fade_gain(7.0, 8.0, &shown) - 0.25f32.powf(0.25)).abs() < 1e-6);
}

/// A fade's line runs from the silent bottom corner to full level at the top along its
/// shape, and the curve handle sits on it half-way; dragging up bends towards +1 (#197).
#[test]
fn fade_lines_and_curve_handles_follow_the_shape() {
    let clip = Rect::from_min_size(Pos2::new(100.0, 10.0), Vec2::new(80.0, 42.0));
    let (top, bottom) = (12.0, 52.0);
    for shape in FadeShape::ALL {
        let curve = FadeCurve { shape, curve: 0.0 };
        let line = fade_line(clip, FadeEdge::In, 100.0, 140.0, curve);
        assert_eq!(line.first().copied(), Some(Pos2::new(100.0, bottom)), "{shape:?}");
        assert_eq!(line.last().copied(), Some(Pos2::new(140.0, top)), "{shape:?}");
        let out = fade_line(clip, FadeEdge::Out, 150.0, 180.0, curve);
        assert_eq!((out[0], out[out.len() - 1]), (Pos2::new(150.0, top), Pos2::new(180.0, bottom)), "{shape:?}");
        let handle = curve_handle(clip, FadeEdge::In, 40.0, curve);
        assert_eq!(handle.x, 120.0);
        assert!((handle.y - (bottom - curve.gain(0.5) as f32 * 40.0)).abs() < 1e-4, "{shape:?}");
    }
    assert_eq!(curve_handle(clip, FadeEdge::Out, 30.0, FadeCurve::default()).x, 165.0);
    // The clip's height is the whole range: half of it up is +1 from 0.
    // The handle follows the pointer: the curve set by a height puts the handle there.
    for shape in FadeShape::ALL {
        let unbent = curve_handle(clip, FadeEdge::In, 40.0, FadeCurve { shape, curve: 0.0 }).y;
        for y in [unbent - 6.0, unbent - 1.0, unbent, unbent + 1.0, unbent + 6.0] {
            let curve = FadeCurve { shape, curve: curve_at(clip, shape, y) };
            if (-1.0..=1.0).contains(&curve.curve) && curve.curve.abs() < 0.999 {
                let at = curve_handle(clip, FadeEdge::In, 40.0, curve).y;
                assert!((at - y).abs() < 0.01, "{shape:?}: handle at {at}, pointer at {y}");
            }
        }
        assert_eq!(curve_at(clip, shape, -500.0), 1.0, "{shape:?}: kept within -1 … +1");
        assert_eq!(curve_at(clip, shape, 500.0), -1.0, "{shape:?}");
    }
    assert!(curve_at(clip, FadeShape::Linear, f32::NAN).is_finite());
    // However wide the fade, its line has a bounded number of points.
    assert!(fade_line(clip, FadeEdge::In, 100.0, 1_000_000.0, FadeCurve::default()).len() <= FADE_LINE_POINTS);
    assert_eq!(shape_label(FadeShape::SCurve), "S-curve");
}

/// A grid of `step` beats at a zoom where its lines are far enough apart, in 4/4.
fn beats(step: f64) -> TimelineGrid {
    grid_of_step(step, 1000.0, 4.0)
}

/// The grid step for the control bar's `value`, at `beat_width` points a beat.
fn step(value: &str, beat_width: f32, bar_beats: f64) -> f64 {
    arrangement_grid(PianoGrid::from_value(value), beat_width, bar_beats).step
}

#[test]
fn each_choice_is_its_own_step_when_zoomed_in() {
    // 400 points a beat: every choice is far enough apart to be drawn as it is.
    let steps = [
        ("8 bars", 32.0),
        ("4 bars", 16.0),
        ("2 bars", 8.0),
        ("1 bar", 4.0),
        ("1/2", 2.0),
        ("1/4", 1.0),
        ("1/8", 0.5),
        ("1/16", 0.25),
        ("1/32", 0.125),
    ];
    for (value, beats) in steps {
        assert_eq!(step(value, 400.0, 4.0), beats, "{value}");
    }
    for (value, beats) in [("1/2T", 4.0 / 3.0), ("1/4T", 2.0 / 3.0), ("1/8T", 1.0 / 3.0), ("1/16T", 1.0 / 6.0)] {
        assert!((step(value, 400.0, 4.0) - beats).abs() < 1e-12, "{value}");
    }
    // Bars follow the time signature: a bar of 3/4 is three beats.
    assert_eq!(step("1 bar", 400.0, 3.0), 3.0);
    assert_eq!(step("2 bars", 400.0, 3.0), 6.0);
    assert!(arrangement_grid(PianoGrid::from_value("1/16"), 400.0, 4.0).snaps);
}

#[test]
fn a_straight_grid_coarsens_by_doubling_then_beats_then_bars() {
    // 1/16 is 0.25 beats: four lines a beat from 32 points a beat on.
    assert_eq!(step("1/16", 32.0, 4.0), 0.25);
    // Closer than 8 points: eighths, then quarters (the beat).
    assert_eq!(step("1/16", 31.0, 4.0), 0.5);
    assert_eq!(step("1/16", 16.0, 4.0), 0.5);
    assert_eq!(step("1/16", 15.0, 4.0), 1.0);
    assert_eq!(step("1/16", 8.0, 4.0), 1.0);
    // A beat under 8 points: the bar, then 2, 4, 8 bars.
    assert_eq!(step("1/16", 7.0, 4.0), 4.0);
    assert_eq!(step("1/16", 2.0, 4.0), 4.0);
    assert_eq!(step("1/16", 1.9, 4.0), 8.0);
    assert_eq!(step("1/16", 1.0, 4.0), 8.0);
    assert_eq!(step("1/16", 0.5, 4.0), 16.0);
    // In 3/4 the beat goes to a bar of three beats, then six.
    assert_eq!(step("1/8", 7.0, 3.0), 3.0);
    assert_eq!(step("1/8", 2.0, 3.0), 6.0);
    // A chosen value is never drawn finer than it is: 2 bars stays 2 bars zoomed in.
    assert_eq!(step("2 bars", 4000.0, 4.0), 8.0);
    assert_eq!(step("8 bars", 1.0, 4.0), 32.0);
    // A half note in 3/4 is not on the bar: once too narrow it goes to the bar.
    assert_eq!(step("1/2", 5.0, 3.0), 2.0);
    assert_eq!(step("1/2", 3.0, 3.0), 3.0);
}

#[test]
fn a_triplet_grid_coarsens_to_the_beat_so_lines_stay_on_the_bar() {
    // 1/16T is a sixth of a beat: drawn from 48 points a beat on.
    assert!((step("1/16T", 48.0, 4.0) - 1.0 / 6.0).abs() < 1e-12);
    // Narrower: straight to the beat, never to a third of a beat that would leave the bar.
    assert_eq!(step("1/16T", 47.0, 4.0), 1.0);
    assert_eq!(step("1/8T", 20.0, 4.0), 1.0);
    assert_eq!(step("1/4T", 11.0, 3.0), 1.0);
    assert_eq!(step("1/4T", 7.0, 3.0), 3.0);
    // 1/2T (4/3 beats) goes to the bar.
    assert_eq!(step("1/2T", 5.0, 4.0), 4.0);
}

#[test]
fn an_adaptive_grid_follows_the_zoom_and_the_keys_change_it() {
    let narrow = PianoGrid { snap: true, mode: GridMode::Adaptive(Width::Narrow), triplet: false };
    // Zoomed in the lines are finer than zoomed out, and they snap.
    let near = arrangement_grid(narrow, 400.0, 4.0);
    let far = arrangement_grid(narrow, 10.0, 4.0);
    assert!(near.step < far.step && near.snaps, "{} {}", near.step, far.step);
    // Ctrl+1 narrows, Ctrl+2 widens, Ctrl+3 triplets, Ctrl+4 snap.
    assert_eq!(grid_key("arrangement.gridNarrow", narrow), Some(narrow.narrower()));
    assert_eq!(grid_key("arrangement.gridWiden", narrow), Some(narrow.wider()));
    assert!(grid_key("arrangement.gridTriplet", narrow).is_some_and(|g| g.triplet));
    let off = grid_key("arrangement.gridSnap", narrow).unwrap();
    assert!(!arrangement_grid(off, 400.0, 4.0).snaps);
    assert_eq!(grid_key("arrangement.tool1", narrow), None);
}

#[test]
fn a_lanes_menu_changes_the_grid_and_sends_no_command() {
    use egui_kittest::kittest::Queryable;
    let mut h = egui_kittest::Harness::builder().allow_missing_glyphs().build_ui_state(
        |ui, (state, actions): &mut (ArrangementViewState, Vec<Action>)| {
            empty_lane_menu(ui, TrackId(1), true, false, state, actions);
        },
        (ArrangementViewState::default(), Vec::new()),
    );
    h.run();
    h.get_by_label("1/8").click_accesskit();
    h.run();
    let (state, actions) = h.state();
    assert_eq!(state.grid.label(), "1/8");
    assert!(actions.is_empty(), "{actions:?}");
}

#[test]
fn snap_off_draws_the_lines_and_does_not_snap() {
    // Snap off keeps its value's lines (1/16 here), as the piano roll's grid does.
    let grid = arrangement_grid(PianoGrid::from_value("none"), 400.0, 4.0);
    assert_eq!((grid.step, grid.snaps), (0.25, false));
    assert_eq!(grid_of_step(0.0, 7.0, 4.0).step, 4.0);
    assert_eq!(grid.place(2.1337, false, false), 2.1337);
    assert_eq!(ruler_beat(2.1337, grid, false), 2.134);
    let clip = audio_clip();
    assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, Some(grid)), 1.3);
}

#[test]
fn a_ruler_click_lands_on_the_grid_unless_alt_is_held() {
    // 1 bar at 8 points a beat: the grid is a bar.
    let bars = arrangement_grid(PianoGrid::from_value("1 bar"), 8.0, 4.0);
    assert_eq!(ruler_beat(17.9, bars, false), 16.0);
    assert_eq!(ruler_beat(18.1, bars, false), 20.0);
    // 1/16 at 60 points a beat: sixteenths.
    let sixteenths = arrangement_grid(PianoGrid::from_value("1/16"), 60.0, 4.0);
    assert_eq!(ruler_beat(2.13, sixteenths, false), 2.25);
    assert_eq!(ruler_beat(2.1337, sixteenths, true), 2.134);
    assert_eq!(ruler_beat(-3.0, bars, true), 0.0);
    assert_eq!(ruler_beat(f64::NAN, bars, false), 0.0);
}

#[test]
fn a_zoom_at_the_pointer_keeps_its_beat_under_it_past_the_content_end() {
    // A view 800 points wide at 20 points a beat, scrolled 400 in, the pointer 600 in: beat 50.
    let (offset, view) = (600.0, 800.0);
    let (mut width, mut scroll) = (20.0, 400.0);
    for factor in [1.25, 1.25, 0.8, 0.5, 2.0] {
        (width, scroll) = zoom_at(width, scroll, offset, factor, 1.0);
        let reach = f64::from((scroll + view) / width);
        // A short project: 40 beats of content, less than the pointer's beat.
        scroll = scroll.clamp(0.0, most_scroll_x(40.0, reach, width, view));
        assert!(((offset + scroll) / width - 50.0).abs() < 1e-3, "beat 50 stays under the pointer at {width}");
    }
    assert_eq!(most_scroll_x(40.0, 0.0, 20.0, 800.0), 0.0, "without a zoom the content alone limits it");
}

#[test]
fn a_tracks_take_lanes_show_under_it_and_fold_away() {
    let mut project = crate::demo::demo_project();
    let lane = project.arrangement.lanes.iter().find(|l| l.track.is_some() && !l.clips.is_empty()).unwrap();
    let (track, clip) = (lane.track.unwrap(), lane.clips[0].clone());
    let take = |name: &str| suhina_engine::model::TakeLane {
        track,
        name: name.into(),
        clips: vec![suhina_engine::model::Clip { id: ClipId::NONE, ..clip.clone() }],
        color: None,
    };
    project.arrangement.takes = vec![take("Take 1"), take("Take 2")];
    let timeline = Timeline::build(&project, None);
    let rows: Vec<(usize, String)> = timeline.takes.iter().map(|t| (t.number, t.name.clone())).collect();
    // Shown with their track's name, "Drums Take 1" (#989).
    let shown = project.shown_track_name(track);
    assert_eq!(
        rows,
        [(1, format!("{shown} Take 1")), (2, format!("{shown} Take 2"))],
        "numbered as take.comp names them"
    );
    assert_eq!(timeline.takes[0].clips.len(), 1);
    let mut state = ArrangementViewState::default();
    let automation = state.automation.height(track, &timeline.automation);
    // As tall as a track that has not been resized, each (#1078).
    let row = state.lane_height(TrackId(999));
    assert_eq!(state.below(track, &timeline), 2.0 * (row + 1.0) + automation);
    state.takes_hidden.insert(track);
    assert_eq!(state.below(track, &timeline), automation, "folded away");
}

/// While a take records, the passes already done add no lanes under the track (#1078).
#[test]
fn a_take_still_recording_has_no_lanes_under_its_track() {
    let project = crate::demo::demo_project();
    let track = project.arrangement.lanes.iter().find_map(|l| l.track).unwrap();
    let mut timeline = Timeline::build(&project, None);
    let state = ArrangementViewState::default();
    let before = state.below(track, &timeline);
    let pass = |finished| suhina_session::LiveTake {
        track,
        start: 0.0,
        end: 4.0,
        notes: Vec::new(),
        audio: false,
        peaks: Vec::new(),
        peaks_end: 0.0,
        finished,
    };
    timeline.live_takes = vec![pass(true), pass(true), pass(false)];
    assert_eq!(state.below(track, &timeline), before);
}

/// Each take lane has a height of its own, the lanes' zoom takes every one along, and a lane left
/// alone stays as tall as a track (#1078).
#[test]
fn take_lanes_size_and_zoom_like_tracks() {
    let mut project = crate::demo::demo_project();
    let lane = project.arrangement.lanes.iter().find(|l| l.track.is_some() && !l.clips.is_empty()).unwrap();
    let (track, clip) = (lane.track.unwrap(), lane.clips[0].clone());
    let take = |name: &str| suhina_engine::model::TakeLane {
        track,
        name: name.into(),
        clips: vec![suhina_engine::model::Clip { id: ClipId::NONE, ..clip.clone() }],
        color: None,
    };
    // A take deleted earlier: the lanes keep the numbers the commands know them by.
    project.arrangement.takes = vec![take("Take 2"), take("Take 3")];
    let timeline = Timeline::build(&project, None);
    assert_eq!(timeline.takes.iter().map(|t| t.number).collect::<Vec<_>>(), [2, 3]);
    let mut state = ArrangementViewState::default();
    let row = state.lane_height(TrackId(999));
    assert_eq!(state.take_row(track, 2), row);
    state.take_heights.insert((track, 2), 100.0);
    assert_eq!(state.take_row(track, 3), row, "the other lane stays");
    state.zoom_lanes(&timeline.lanes, 2.0, 0.0);
    assert_eq!(state.take_row(track, 2), 200.0);
    assert_eq!(state.take_row(track, 3), state.lane_height(TrackId(999)));
    assert!(state.take_row(track, 3) > row);
    assert_eq!(state.takes_height(track, &timeline), 200.0 + 1.0 + state.take_row(track, 3) + 1.0);
}

/// Small wheel steps add up on a take lane, as they do on a track: its height keeps the fraction.
#[test]
fn small_wheel_steps_add_up_on_the_take_lane_under_the_pointer() {
    let mut project = crate::demo::demo_project();
    let lane = project.arrangement.lanes.iter().find(|l| l.track.is_some() && !l.clips.is_empty()).unwrap();
    let (track, clip) = (lane.track.unwrap(), lane.clips[0].clone());
    let clips = vec![suhina_engine::model::Clip { id: ClipId::NONE, ..clip }];
    project.arrangement.takes =
        vec![suhina_engine::model::TakeLane { track, name: "Take 1".into(), clips, color: None }];
    let timeline = Timeline::build(&project, None);
    let tracks: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
    let mut state = ArrangementViewState::default();
    state.zoom_one = true;
    state.take_heights.insert((track, 1), 64.0);
    let above: f32 = tracks.iter().take_while(|l| l.track != track).map(|l| state.height(l) + 1.0).sum();
    let at = tracks.iter().find(|l| l.track == track).map_or(0.0, |l| state.height(l) + 1.0);
    let offset = above + at + 2.0;
    for _ in 0..100 {
        state.zoom_wheel(&tracks, &timeline, 1.004, offset);
    }
    assert!(state.take_row(track, 1) >= 90.0, "{}", state.take_row(track, 1));
}

#[test]
fn alt_and_the_wheel_can_change_only_the_lane_under_the_pointer() {
    let timeline = Timeline::build(&crate::demo::demo_project(), None);
    let tracks: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
    let mut state = ArrangementViewState::default();
    state.zoom_one = true;
    // Halfway down the second track's lane.
    let offset = state.height(tracks[0]) + 1.0 + state.height(tracks[1]) / 2.0;
    state.zoom_wheel(&tracks, &timeline, 2.0, offset);
    assert_eq!(state.height(tracks[1]), DEFAULT_LANE * 2.0);
    assert!(tracks.iter().enumerate().filter(|(i, _)| *i != 1).all(|(_, l)| state.height(l) == DEFAULT_LANE));
    // All lanes, as before, with the setting off.
    state.zoom_one = false;
    state.zoom_wheel(&tracks, &timeline, 2.0, 0.0);
    assert_eq!(state.height(tracks[0]), DEFAULT_LANE * 2.0);
}

/// Alt and the wheel over the returns at the bottom change the return under the pointer, not a
/// track of the scrolling lanes above them, and a track's lane says where it is now, so the view
/// can keep a lane made taller near the bottom in sight (#1322).
#[test]
fn alt_and_the_wheel_over_the_returns_change_the_return() {
    let timeline = Timeline::build(&crate::demo::demo_project(), None);
    let tracks: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
    let returns: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind == ColumnKind::Return).collect();
    assert!(!returns.is_empty(), "the demo has a return");
    let mut state = ArrangementViewState::default();
    state.zoom_one = true;
    let before = state.height(returns[0]);
    state.zoom_fixed(&returns, &timeline, 2.0, before / 2.0, 300.0);
    assert_eq!(state.height(returns[0]), (before * 2.0).clamp(MIN_LANE, MAX_LANE));
    assert!(tracks.iter().all(|l| state.height(l) == DEFAULT_LANE), "the tracks keep their height");

    // The second track's lane, made taller: its top and its new height.
    let top = state.height(tracks[0]) + 1.0;
    let at = state.zoom_wheel(&tracks, &timeline, 2.0, top + 2.0);
    assert_eq!(at, Some((top, DEFAULT_LANE * 2.0)));
}

/// One selection at a time (#1322): a range painted on automation lanes lets go of the time
/// selection and the picked clips, and a time selection or a clip picked after it lets go of it.
#[test]
fn the_last_selection_made_is_the_only_one() {
    use crate::arrangement::automation_lanes::{LaneKey, LaneRange};
    let range = || LaneRange {
        track: TrackId(1),
        key: LaneKey { device: None, param: "volume".into() },
        start: 4.0,
        end: 8.0,
        more: Vec::new(),
    };
    let time = || TimeSelection { tracks: vec![TrackId(2)], start: 0.0, end: 4.0, row: None };
    let mut state = ArrangementViewState::default();
    state.set_time_selection(Some(time()));
    state.selected = vec![(TrackId(2), ClipId(5))];
    state.settle_selections();
    assert!(state.time_selection().is_some() && !state.selected.is_empty(), "nothing else was there");

    state.automation.range = Some(range());
    state.settle_selections();
    assert!(state.time_selection().is_none() && state.selected.is_empty(), "the range came last");
    assert!(state.automation.range.is_some());
    state.settle_selections();
    assert!(state.automation.range.is_some(), "it stays as long as nothing else is made");

    state.set_time_selection(Some(time()));
    state.settle_selections();
    assert!(state.automation.range.is_none(), "the time selection came last");
    assert!(state.time_selection().is_some());

    state.automation.range = Some(range());
    state.settle_selections();
    state.select_clips(vec![(TrackId(2), ClipId(5))]);
    state.settle_selections();
    assert!(state.automation.range.is_none(), "a clip picked came last");
}

#[test]
fn a_stretch_goes_past_the_audio_and_a_midi_clip_before_its_notes() {
    use suhina_engine::model::{AudioRegion, ClipContent, ContentId, ContentLane, Extensions, LaneData};
    let mut project = crate::demo::demo_project();
    let audio = ContentId(900);
    project.contents.push(ClipContent {
        linked: false,
        id: audio,
        name: "Loop".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("loop.wav")) }].into(),
        extensions: Extensions::default(),
        scale: None,
    });
    let drums = project.tracks.first().map(|t| t.id).unwrap_or(TrackId(1));
    if let Some(lane) = project.arrangement.lanes.iter_mut().find(|l| l.track == Some(drums)) {
        lane.clips = vec![suhina_engine::model::Clip::new(audio, 100.0, 4.0)];
    }
    project.assign_ids().unwrap();
    let timeline = Timeline::build(&project, None);
    let clip = timeline.lanes.iter().find(|l| l.track == drums).and_then(|l| l.clips.first()).unwrap();
    let grid = arrangement_grid(PianoGrid::from_value("1/4"), 24.0, 4.0);
    let right = EdgeDrag { key: (drums, clip.id), grab: EdgeGrab::Right, raw: 112.0, from: 104.0, origin: 0.0 };
    // A plain drag stops where the audio ends; a stretch goes on (#833).
    assert_eq!(edge_extent(clip, right, &grid, false, false), (100.0, 104.0));
    assert_eq!(edge_extent(clip, right, &grid, false, true), (100.0, 112.0));
    let left = EdgeDrag { key: (drums, clip.id), grab: EdgeGrab::Left, raw: 96.0, from: 100.0, origin: 0.0 };
    assert_eq!(edge_extent(clip, left, &grid, false, false).0, 100.0);
    assert_eq!(edge_extent(clip, left, &grid, false, true).0, 96.0);
    // A MIDI clip's left edge goes before its notes (#832).
    let demo = Timeline::build(&crate::demo::demo_project(), None);
    let midi = demo
        .lanes
        .iter()
        .flat_map(|l| l.clips.iter().map(move |c| (l.track, c)))
        .find(|(_, c)| !c.audio && c.start >= 4.0)
        .unwrap();
    let before = EdgeDrag {
        key: (midi.0, midi.1.id),
        grab: EdgeGrab::Left,
        raw: midi.1.start - 2.0,
        from: midi.1.start,
        origin: 0.0,
    };
    assert_eq!(edge_extent(midi.1, before, &grid, false, false).0, midi.1.start - 2.0);
}

#[test]
fn select_all_spans_every_track_from_the_first_clip_to_the_last() {
    let mut state = ArrangementViewState::default();
    let lanes = vec![
        (TrackId(1), vec![(ClipId(1), 8.0, 12.0)]),
        (TrackId(2), vec![(ClipId(2), 4.0, 6.0), (ClipId(3), 16.0, 20.0)]),
        (TrackId(3), Vec::new()),
    ];
    state.select_all_clips(&lanes);
    assert_eq!(state.selected.len(), 3);
    let span = state.time_selection().expect("a time selection");
    assert_eq!((span.start, span.end), (4.0, 20.0));
    assert_eq!(span.tracks, vec![TrackId(1), TrackId(2), TrackId(3)]);
    // An empty project leaves a selection already painted.
    let painted = TimeSelection { tracks: vec![TrackId(1)], start: 1.0, end: 2.0, row: None };
    state.set_time_selection(Some(painted));
    state.select_all_clips(&[(TrackId(1), Vec::new())]);
    assert_eq!(state.time_selection().map(|s| s.start), Some(1.0));
}

#[test]
fn a_click_in_a_clip_body_stays_inside_the_clip() {
    let bar = arrangement_grid(PianoGrid::from_value("1 bar"), 40.0, 4.0);
    // Late in a bar-long clip: the nearest line is its end, so the line before the click.
    assert_eq!(clip_body_beat(7.5, 4.0, 8.0, bar, false), 4.0);
    // A short clip inside the bar: the nearest line is past its end and the one before is
    // before its start, so its start.
    assert_eq!(clip_body_beat(6.5, 6.0, 7.5, bar, false), 6.0);
    // The nearest line inside the clip, after the click too.
    assert_eq!(clip_body_beat(7.5, 6.0, 10.0, bar, false), 8.0);
    let quarter = arrangement_grid(PianoGrid::from_value("1/4"), 40.0, 4.0);
    assert_eq!(clip_body_beat(5.6, 4.0, 8.0, quarter, false), 6.0, "a click that misses a line goes to the nearest");
    assert_eq!(clip_body_beat(5.4, 4.0, 8.0, quarter, false), 5.0);
    let sixteenth = arrangement_grid(PianoGrid::from_value("1/16"), 200.0, 4.0);
    assert_eq!(clip_body_beat(5.3, 4.0, 8.0, sixteenth, false), 5.25);
    assert_eq!(clip_body_beat(5.3, 4.0, 8.0, sixteenth, true), 5.3);
}

#[test]
fn a_clip_off_the_grid_keeps_its_place_until_it_moves_half_a_step() {
    let grid = arrangement_grid(PianoGrid::from_value("1/4"), 40.0, 4.0);
    assert_eq!(clip::drag_place(5.3, 5.3, grid, false), 5.3);
    assert_eq!(clip::drag_place(5.6, 5.3, grid, false), 5.3);
    assert_eq!(clip::drag_place(6.2, 5.3, grid, false), 6.0);
    assert_eq!(clip::drag_place(5.6, 5.3, grid, true), 5.6);
}

#[test]
fn clip_notes_give_way_only_to_crowded_pitch_rows() {
    // Only the pitch rows decide: 40 pitches in a 30-point strip are under a point a row.
    assert!(clip::pitch_rows_crowded(40, 30.0), "rows under a point: the name instead");
    assert!(!clip::pitch_rows_crowded(20, 30.0), "a point and a half a row: drawn");
    assert!(!clip::pitch_rows_crowded(0, 30.0));
}

#[test]
fn a_clips_notes_get_a_row_per_pitch_they_use_and_give_way_when_rows_are_too_thin() {
    // A drum part: kick, snare and hat, each a row, whatever lies between them.
    assert_eq!(clip::used_keys([42, 36, 38, 36, 42].into_iter()), vec![36, 38, 42]);
    // The 13 points under the name bar of a 36 px lane hold thirteen pitches; a pluck's twenty
    // fit in 30 points, rows overlapping a little; forty do not.
    assert!(!clip::pitch_rows_crowded(4, 13.0), "4 rows of 3.25 points");
    assert!(!clip::pitch_rows_crowded(20, 30.0), "20 rows of 1.5 points");
    assert!(clip::pitch_rows_crowded(40, 30.0), "40 rows of 0.75 points");
    assert!(!clip::pitch_rows_crowded(0, 13.0));
}

#[test]
fn dragging_a_selected_track_resizes_every_selected_track() {
    let mut state = ArrangementViewState::default();
    state.set_lane_height(TrackId(1), 100.0);
    state.set_lane_height(TrackId(2), 60.0);
    state.set_lane_height(TrackId(3), 80.0);
    state.selected_tracks = vec![TrackId(1), TrackId(2)];
    state.resize_lanes(TrackId(1), 100.0, 20.0);
    assert_eq!(state.lane_height(TrackId(1)), 120.0);
    assert_eq!(state.lane_height(TrackId(2)), 80.0);
    assert_eq!(state.lane_height(TrackId(3)), 80.0);
    // A track not among them goes alone.
    state.resize_lanes(TrackId(3), 80.0, 10.0);
    assert_eq!(state.lane_height(TrackId(1)), 120.0);
    assert_eq!(state.lane_height(TrackId(3)), 90.0);
}

#[test]
fn a_drag_starts_from_the_height_drawn() {
    // A return row drawn shorter than a track's kept height does not jump when dragged.
    let mut state = ArrangementViewState::default();
    state.resize_lanes(TrackId(9), 14.0, 2.0);
    assert_eq!(state.lane_height(TrackId(9)), MIN_LANE.max(16.0));
}

/// A time selection gone on over more time and tracks holds both.
#[test]
fn a_time_selection_goes_on_over_both() {
    let first = TimeSelection { tracks: vec![TrackId(1)], start: 4.0, end: 6.0, row: Some((0, 2)) };
    let same = first.gone_on(&[TrackId(1)], 8.0, 10.0, Some((0, 2)));
    assert_eq!((same.start, same.end, same.row), (4.0, 10.0, Some((0, 2))));
    assert_eq!(first.gone_on(&[TrackId(1)], 8.0, 10.0, Some((1, 2))).row, None, "into another row of the stack");
    let more = first.gone_on(&[TrackId(1), TrackId(2)], 0.0, 5.0, None);
    assert_eq!((more.start, more.end, more.row), (0.0, 6.0, None));
    assert_eq!(more.tracks, [TrackId(1), TrackId(2)]);
}

/// A clip a few points wide, as far zoomed out: no name or ellipsis, and nothing it draws leaves
/// its own rect for the lane or the clip beside it (#1198). Wide enough, the name is drawn.
#[test]
fn a_narrow_clip_draws_no_name_and_nothing_outside_itself() {
    let timeline = Timeline::build(&crate::demo::demo_project(), None);
    let lane = timeline.lanes.iter().find(|l| l.clips.iter().any(|c| !c.name.is_empty())).unwrap();
    let clip = lane.clips.iter().find(|c| !c.name.is_empty()).cloned().unwrap();
    let view = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(800.0, 200.0));
    let draw = |width: f32| {
        let rect = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(width, 60.0));
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let lanes = ui.painter().with_clip_rect(view);
            let beat_width = width / clip.length.max(1e-3) as f32;
            let x_of = |beat: f64| rect.left() + (beat - clip.start) as f32 * beat_width;
            let look = ArrangementViewState::default().clip_look();
            paint_clip(
                &lanes,
                rect,
                lane,
                &clip,
                Shown::of(&clip),
                clip.start,
                &x_of,
                beat_width,
                &timeline,
                true,
                &look,
            );
        });
        output.textures_delta.clear();
        // The clip's own shapes: those painted within the lanes' view.
        let shapes: Vec<egui::epaint::ClippedShape> =
            output.shapes.into_iter().filter(|s| view.contains_rect(s.clip_rect)).collect();
        (rect, shapes)
    };
    let (rect, shapes) = draw(6.0);
    assert!(!shapes.is_empty(), "the clip's body is drawn");
    for shape in &shapes {
        assert!(rect.expand(0.5).contains_rect(shape.clip_rect), "{:?} leaves {rect:?}", shape.clip_rect);
    }
    assert!(!shapes.iter().any(|s| matches!(s.shape, egui::Shape::Text(_))), "no name, no lone ellipsis");
    let (_, shapes) = draw(240.0);
    assert!(shapes.iter().any(|s| matches!(s.shape, egui::Shape::Text(_))), "a wide clip shows its name");
}

/// The time selection over a clip's track lights the stretch of its body under it in the selection
/// colour, under its name bar, as a selected clip's body is; the rest of the clip keeps its colour.
#[test]
fn the_time_selection_lights_a_clips_body_in_the_selection_colour() {
    let timeline = Timeline::build(&crate::demo::demo_project(), None);
    let clip =
        timeline.lanes.iter().flat_map(|l| l.clips.iter()).find(|c| !c.audio && c.length >= 4.0).cloned().unwrap();
    let lane = timeline.lanes.iter().find(|l| l.clips.iter().any(|c| c.id == clip.id)).unwrap();
    let rect = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(20.0 * clip.length as f32, 60.0));
    let fill = crate::kit::theme::studio("selection-background");
    let lit = |time_lit: Option<(f64, f64)>| {
        crate::kit::skin::configure(&crate::kit::skin::Skin { css: Some("".into()), ..Default::default() });
        let ctx = egui::Context::default();
        let mut shown = Vec::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let _frame = crate::kit::native_css::frame_with_motion(ui, false);
                let x_of = |beat: f64| rect.left() + (beat - clip.start) as f32 * 20.0;
                let look = ClipLook { time_lit, ..ArrangementViewState::default().clip_look() };
                paint_clip(
                    ui.painter(),
                    rect,
                    lane,
                    &clip,
                    Shown::of(&clip),
                    clip.start,
                    &x_of,
                    20.0,
                    &timeline,
                    false,
                    &look,
                );
            });
            output.textures_delta.clear();
            shown = output.shapes;
        }
        crate::kit::skin::configure(&crate::kit::skin::Skin::default());
        shown
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::Rect(r) if r.fill == fill => Some(r.rect),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let shown = lit(Some((clip.start + 1.0, clip.start + 3.0)));
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!((shown[0].left(), shown[0].right()), (120.0, 160.0), "beats 1 to 3 of the clip");
    assert!(shown[0].top() > rect.top(), "under the name bar");
    assert!(lit(None).is_empty(), "no time selection, nothing lit");
}

/// The piano roll's time selection is lit on the clip it edits, where those beats of the content
/// play from the clip's start, within the clip, and on no other clip.
#[test]
fn the_piano_rolls_time_selection_is_lit_on_its_clip() {
    let timeline = Timeline::build(&crate::demo::demo_project(), None);
    let mut clip = timeline.lanes.iter().flat_map(|l| l.clips.iter()).find(|c| !c.audio).cloned().unwrap();
    clip.offset = 1.0;
    clip.length = 8.0;
    assert_eq!(editor_span_in(Some((clip.id, 2.0, 4.0)), &clip), Some((1.0, 3.0)), "from where it plays");
    assert_eq!(editor_span_in(Some((clip.id, 0.0, 20.0)), &clip), Some((0.0, 8.0)), "within the clip");
    assert_eq!(editor_span_in(Some((clip.id, 0.0, 1.0)), &clip), None, "before it plays");
    assert_eq!(editor_span_in(Some((ClipId(clip.id.0 + 1), 2.0, 4.0)), &clip), None, "another clip");
    assert_eq!(editor_span_in(None, &clip), None);

    let lane = timeline.lanes.iter().find(|l| l.clips.iter().any(|c| c.id == clip.id)).unwrap();
    let rect = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(160.0, 60.0));
    let tint = crate::kit::theme::studio("accent").gamma_multiply(0.45);
    let lit = |span: Option<(ClipId, f64, f64)>| {
        // The base sheet's colours, as the style engine gives them over a few frames.
        crate::kit::skin::configure(&crate::kit::skin::Skin { css: Some("".into()), ..Default::default() });
        let ctx = egui::Context::default();
        let mut found = Vec::new();
        let mut shown = Vec::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let _frame = crate::kit::native_css::frame_with_motion(ui, false);
                let x_of = |beat: f64| rect.left() + (beat - clip.start) as f32 * 20.0;
                let look = ClipLook { editor_span: span, ..ArrangementViewState::default().clip_look() };
                paint_clip(
                    ui.painter(),
                    rect,
                    lane,
                    &clip,
                    Shown::of(&clip),
                    clip.start,
                    &x_of,
                    20.0,
                    &timeline,
                    true,
                    &look,
                );
            });
            output.textures_delta.clear();
            shown = output.shapes;
        }
        crate::kit::skin::configure(&crate::kit::skin::Skin::default());
        for shape in shown {
            if let egui::Shape::Rect(r) = shape.shape
                && r.fill == tint
            {
                found.push(r.rect);
            }
        }
        found
    };
    let shown = lit(Some((clip.id, 2.0, 4.0)));
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!((shown[0].left(), shown[0].right()), (120.0, 160.0), "beats 1 to 3 of the clip");
    assert!(lit(None).is_empty());
}

#[test]
fn a_muted_clip_is_the_base_sheet_grey_whatever_its_colour() {
    // #1393: dimming the colour left a muted clip looking like any other clip on a grey track.
    let surface = crate::kit::theme::studio("panel-background");
    let grey = clip::muted_clip_grey(false);
    assert_eq!(grey, Color32::from_rgb(0xA9, 0xA6, 0xA0), "the base sheet's clip[muted]");
    assert_eq!(clip::muted_clip_grey(true), Color32::from_rgb(0xBD, 0xBA, 0xB4), "clip[muted][selected]");
    for colour in [Color32::from_rgb(0xFF, 0x9A, 0x3C), Color32::from_rgb(0x4A, 0x4A, 0x4A), grey] {
        assert_eq!(clip::clip_body(surface, colour, 82, true, false), grey, "{colour:?}");
        let selected = clip::clip_body(surface, colour, 82, true, true);
        assert!(
            selected.to_srgba_unmultiplied()[0] > grey.to_srgba_unmultiplied()[0],
            "selection lifts a muted clip's brightness"
        );
    }
    // A clip that plays keeps its hue, and a grey one is not the muted grey.
    let orange = clip::clip_body(surface, Color32::from_rgb(0xFF, 0x9A, 0x3C), 82, false, false);
    let [red, _, blue, _] = orange.to_srgba_unmultiplied();
    assert!(red > blue + 60, "{orange:?}");
    assert_ne!(clip::clip_body(surface, Color32::from_rgb(0x4A, 0x4A, 0x4A), 82, false, false), grey);
}

#[test]
fn a_clip_on_a_muted_track_keeps_its_colour_in_its_name_bar() {
    let surface = crate::kit::theme::studio("panel-background");
    let orange = Color32::from_rgb(0xFF, 0x9A, 0x3C);
    let grey = clip::muted_clip_grey(false);
    // On a muted track: the bar is the colour a clip that plays has.
    let on_track = clip::name_bar_fill(grey, surface, orange, 82, true, false);
    assert_eq!(on_track, clip::clip_body(surface, orange, 82, false, false));
    // Turned off itself: the bar is grey, a hair off the body.
    let off = clip::name_bar_fill(grey, surface, orange, 82, false, false);
    assert_ne!(off, on_track);
    let [red, _, blue, _] = off.to_srgba_unmultiplied();
    let grey_red = grey.to_srgba_unmultiplied()[0];
    assert!(red.abs_diff(grey_red) < 16 && red.abs_diff(blue) < 16, "{off:?}");
}

#[test]
fn a_muted_track_header_is_the_base_sheet_grey() {
    assert_eq!(header::muted_header_grey(false), Color32::from_rgb(0xA9, 0xA6, 0xA0), "track-header[muted]");
    assert_eq!(header::muted_header_grey(true), Color32::from_rgb(0xB5, 0xB2, 0xAC), "track-header[muted][selected]");
}

#[test]
fn a_clip_on_a_muted_track_is_drawn_muted_too() {
    assert!(clip::drawn_muted(true, true), "on a muted track");
    assert!(clip::drawn_muted(false, false), "turned off");
    assert!(!clip::drawn_muted(false, true));
}

#[test]
fn a_clip_too_low_for_a_name_bar_still_has_its_mute_mark() {
    // #1399 review: the mark was drawn only inside the name bar, so a clip on a low lane turned
    // off was the same as one on a muted track, or one whose colour is grey.
    let low = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(120.0, 24.0));
    assert!(clip::clip_name_bar(low).is_none(), "24 points is below the name bar's least height");
    let centre = clip::mute_mark_centre(low, None, 107.0).expect("a 24-point clip has room for the mark");
    assert!(low.contains(centre), "{centre:?}");
    // The body shows the name there (#1381): the mark goes before it, on its line, and the name
    // starts after the mark, never under it.
    assert_eq!(centre.y, low.center().y, "on the name's line: {centre:?}");
    let name = clip::after_mute_mark(107.0, Some(centre));
    assert!(name > centre.x + 4.5, "the name starts after the mark: {name} vs {centre:?}");
    assert_eq!(clip::after_mute_mark(107.0, None), 107.0, "a clip turned on keeps its name's place");

    // With a name bar, it stays before the name, in the middle of the bar.
    let tall = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(120.0, 60.0));
    let bar = clip::clip_name_bar(tall).expect("a 60-point clip has a name bar");
    let centre = clip::mute_mark_centre(tall, Some(bar), 107.0).expect("mark in the bar");
    assert_eq!(centre.y, bar.center().y);

    // A clip too small for it has none, rather than a mark spilling out of it.
    let sliver = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(120.0, 10.0));
    assert_eq!(clip::mute_mark_centre(sliver, None, 107.0), None);
    let narrow = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(12.0, 24.0));
    assert_eq!(clip::mute_mark_centre(narrow, None, 107.0), None);
}

/// The name bar's look in one frame with `css` as the theme's sheet.
fn name_bar_look_with(css: &str, bar_fill: Color32) -> clip::NameBarLook {
    crate::kit::skin::configure(&crate::kit::skin::Skin { css: Some(std::sync::Arc::from(css)), ..Default::default() });
    let ctx = egui::Context::default();
    let mut look = None;
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = crate::kit::native_css::frame_with_motion(ui, false);
            look = Some(clip::name_bar_look(&crate::kit::element::CLIP_NAME.probe(), bar_fill));
        })
        .textures_delta
        .clear();
    }
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
    look.expect("ran a frame")
}

#[test]
fn the_mute_mark_takes_the_ink_of_the_name_bar_as_drawn() {
    // #1399 review: the mark's ink came from the clip's own bar colour, so on a theme's dark bar
    // it was dark on dark while the name was readable.
    let grey = clip::muted_clip_grey(false);
    let dark = name_bar_look_with("clip-name { background-color: #000000; }", grey);
    assert_eq!(dark.fill, Color32::BLACK);
    assert_eq!(dark.ink, clip::ink_on(Color32::BLACK), "light ink on the theme's black bar");
    assert_ne!(dark.ink, clip::ink_on(grey));

    let coloured = name_bar_look_with("clip-name { background-color: #000000; color: #FFCC00; }", grey);
    assert_eq!(coloured.ink, Color32::from_rgb(0xFF, 0xCC, 0x00), "the theme's own name colour");

    // Unstyled, the bar is the clip's own colour and the ink reads on it, as before.
    let plain = name_bar_look_with("", grey);
    assert_eq!((plain.fill, plain.ink), (grey, clip::ink_on(grey)));
}

/// The shapes `paint_clip` draws for `clip` in `rect` under `css`, `selected` or not, as the
/// style engine gives them after a few frames.
fn painted_clip(css: &str, rect: Rect, selected: bool) -> Vec<egui::epaint::ClippedShape> {
    let mut timeline = Timeline::build(&crate::demo::demo_project(), None);
    let clip = &mut timeline.lanes.iter_mut().find(|l| !l.clips.is_empty()).unwrap().clips[0];
    clip.take = Some(Default::default());
    let length = clip.length;
    painted_clip_span(&timeline, css, rect, length, selected)
}

fn painted_clip_span(
    timeline: &Timeline,
    css: &str,
    rect: Rect,
    length: f64,
    selected: bool,
) -> Vec<egui::epaint::ClippedShape> {
    let lane = timeline.lanes.iter().find(|l| !l.clips.is_empty()).unwrap();
    let clip = &lane.clips[0];
    let beat_width = rect.width() / length.max(1e-3) as f32;
    crate::kit::skin::configure(&crate::kit::skin::Skin { css: Some(css.into()), ..Default::default() });
    let ctx = egui::Context::default();
    let mut shown = Vec::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let _frame = crate::kit::native_css::frame_with_motion(ui, false);
            let x_of = |beat: f64| rect.left() + (beat - clip.start) as f32 * beat_width;
            let look = ArrangementViewState::default().clip_look();
            paint_clip(
                ui.painter(),
                rect,
                lane,
                clip,
                Shown::of(clip),
                clip.start,
                &x_of,
                beat_width,
                timeline,
                selected,
                &look,
            );
        });
        output.textures_delta.clear();
        shown = output.shapes;
    }
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
    shown
}

#[test]
fn a_right_edge_preview_reveals_notes_and_their_tails_before_release() {
    use suhina_engine::model::{LaneData, Note};

    let mut project = crate::demo::demo_project();
    let placed = &mut project.arrangement.lanes[0].clips[0];
    placed.loop_range = None;
    placed.offset = 0.;
    placed.length = 1.;
    let content_id = placed.content;
    let content = project.contents.iter_mut().find(|c| c.id == content_id).unwrap();
    content.lanes[0].data =
        LaneData::Notes(vec![Note::new(0., 2., 60, 1.), Note::new(1., 0.5, 64, 1.), Note::new(2., 1., 67, 1.)].into());
    let before = Timeline::build(&project, None);
    let draw = |timeline: &Timeline, length: f64| {
        let rect = Rect::from_min_size(Pos2::new(100., 40.), Vec2::new(length as f32 * 80., 60.));
        painted_clip_span(timeline, "", rect, length, false)
            .into_iter()
            .filter_map(|s| match s.shape {
                egui::Shape::Rect(r) if r.rect.height() == 2. => Some(r.rect),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let short = draw(&before, 1.);
    assert_eq!(short.len(), 1, "notes beyond the edge stay hidden");
    let held = draw(&before, 3.);
    assert_eq!(held.len(), 3, "the same cached timeline shows notes while the right edge is held");
    assert!(held[0].width() > short[0].width(), "a note crossing the old edge shows its tail");
    assert_eq!(project.arrangement.lanes[0].clips[0].length, 1., "the drag has not committed");

    project.arrangement.lanes[0].clips[0].length = 3.;
    assert_eq!(draw(&Timeline::build(&project, None), 3.), held, "release leaves the notes in place");

    // Hidden source pitches do not squeeze the single visible pitch into a smaller row.
    project.arrangement.lanes[0].clips[0].length = 1.;
    let content = project.contents.iter_mut().find(|c| c.id == content_id).unwrap();
    content.lanes[0].data = LaneData::Notes(vec![Note::new(0., 2., 60, 1.)].into());
    assert_eq!(draw(&Timeline::build(&project, None), 1.), short);
}

/// A clip's name bar reaches its right edge, selected or not (#1487 B11): it stopped 2 points
/// short, and the body's lighter selected shade showed in the gap.
#[test]
fn a_clips_name_bar_reaches_its_right_edge() {
    let rect = Rect::from_min_size(Pos2::new(100.0, 40.0), Vec2::new(160.0, 60.0));
    for selected in [false, true] {
        let shapes = painted_clip("clip-name { background-color: #00FF00; }", rect, selected);
        let bar = shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == Color32::from_rgb(0, 0xFF, 0) => Some(r.rect),
                _ => None,
            })
            .expect("the name bar");
        assert_eq!(bar.right(), rect.right(), "selected: {selected}");
    }
}

/// The take mark's three bars are evenly spaced on whole pixels, wherever the clip's bottom
/// falls (#1487 B14): 2.5 points apart, each rounded on its own, they were 2 and 3 apart.
#[test]
fn the_take_marks_bars_are_evenly_spaced() {
    for bottom in [100.0, 100.25, 100.5, 100.75] {
        let rect = Rect::from_min_max(Pos2::new(100.0, 40.0), Pos2::new(260.0, bottom));
        let shapes = painted_clip("", rect, false);
        let mut ys: Vec<f32> = shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::LineSegment { points, .. }
                    if points[0].y == points[1].y
                        && points[0].x == rect.left() + 4.0
                        && points[1].x == rect.left() + 11.0 =>
                {
                    Some(points[0].y)
                }
                _ => None,
            })
            .collect();
        ys.sort_by(f32::total_cmp);
        assert_eq!(ys.len(), 3, "bottom {bottom}: {ys:?}");
        assert_eq!(ys[1] - ys[0], ys[2] - ys[1], "bottom {bottom}: {ys:?}");
        assert_eq!(ys[0].fract(), 0.5, "on a pixel's centre: {ys:?}");
    }
}

#[test]
fn a_loop_bars_ends_take_a_press_ten_points_out_and_in_and_a_short_bar_keeps_a_middle() {
    use LoopBarPart::{Body, End, Start};
    let part = |x| loop_bar_part(x, 100.0, 300.0);
    assert_eq!([part(89.0), part(91.0), part(110.0), part(111.0)], [None, Some(Start), Some(Start), Some(Body)]);
    assert_eq!([part(289.0), part(290.0), part(310.0), part(311.0)], [Some(Body), Some(End), Some(End), None]);
    // 15 points wide: 5 points in from each end take it, the middle 5 move the bar.
    let short = |x| loop_bar_part(x, 100.0, 115.0);
    assert_eq!([short(105.0), short(107.5), short(110.0)], [Some(Start), Some(Body), Some(End)]);
}
