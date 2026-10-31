// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
        width = zoomed(width, 1.5);
    }
    assert_eq!(width, 4000.0);
    for _ in 0..100 {
        width = zoomed(width, 1.0 / 1.5);
    }
    assert_eq!(width, 1.0);
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
fn a_fold_inside_a_fold_hides_exactly_what_it_covers() {
    let lane = |track: u32, kind: ColumnKind, depth: usize| Lane {
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
    state.curve_drag = Some(CurveDrag { key, edge: FadeEdge::In, curve: 0.4, grab: 0.0 });
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_in_curve.curve, 0.4);
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_out_curve.curve, 0.0);
    state.curve_drag = None;
    // The waveform is drawn with the gain being dragged, not the one in the project.
    state.gain_drag = Some((key, -12.0));
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).gain_db, -12.0);
    assert_eq!(state.shown((TrackId(1), ClipId(2)), &clip, Some(beats(1.0))).gain_db, 0.0, "only on the dragged clip");
    // A fade being dragged: on the grid, or free with Alt.
    state.fade_drag = Some(FadeDrag { key, edge: FadeEdge::Out, raw: 2.6, from: 0.0 });
    assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_out, 3.0);
    assert!((state.shown(key, &clip, None).fade_out - 2.6).abs() < 1e-9);
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
        (width, scroll) = zoom_at(width, scroll, offset, factor);
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
    assert_eq!(state.below(track, &timeline), 2.0 * (TAKE_ROW + 1.0) + automation);
    state.takes_hidden.insert(track);
    assert_eq!(state.below(track, &timeline), automation, "folded away");
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

#[test]
fn a_stretch_goes_past_the_audio_and_a_midi_clip_before_its_notes() {
    use suhina_engine::model::{AudioRegion, ClipContent, ContentId, ContentLane, Extensions, LaneData};
    let mut project = crate::demo::demo_project();
    let audio = ContentId(900);
    project.contents.push(ClipContent {
        id: audio,
        name: "Loop".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("loop.wav")) }],
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
    // Late in a bar-long clip: its start, not the next bar.
    assert_eq!(clip_body_beat(7.5, 4.0, bar, false), 4.0);
    // A clip starting inside the bar: never before its start.
    assert_eq!(clip_body_beat(7.5, 6.0, bar, false), 6.0);
    let sixteenth = arrangement_grid(PianoGrid::from_value("1/16"), 200.0, 4.0);
    assert_eq!(clip_body_beat(5.3, 4.0, sixteenth, false), 5.25);
    assert_eq!(clip_body_beat(5.3, 4.0, sixteenth, true), 5.3);
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
fn dragging_a_selected_track_resizes_every_selected_track() {
    let mut state = ArrangementViewState::default();
    state.set_lane_height(TrackId(1), 100.0);
    state.set_lane_height(TrackId(2), 60.0);
    state.set_lane_height(TrackId(3), 80.0);
    state.selected_tracks = vec![TrackId(1), TrackId(2)];
    state.resize_lanes(TrackId(1), 20.0);
    assert_eq!(state.lane_height(TrackId(1)), 120.0);
    assert_eq!(state.lane_height(TrackId(2)), 80.0);
    assert_eq!(state.lane_height(TrackId(3)), 80.0);
    // A track not among them goes alone.
    state.resize_lanes(TrackId(3), 10.0);
    assert_eq!(state.lane_height(TrackId(1)), 120.0);
    assert_eq!(state.lane_height(TrackId(3)), 90.0);
}
