// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn point(x: f32, y: f32, kind: Kind) -> MapPoint {
    MapPoint {
        name: format!("{x},{y}"),
        detail: String::new(),
        position: [x, y],
        kind,
        guessed: false,
        by_hand: false,
        in_use: false,
        selected: false,
        matches: true,
        preset: false,
        pinned: false,
        tags: Vec::new(),
    }
}

#[test]
fn the_density_waits_for_a_resize_to_settle_but_not_for_a_change_of_content() {
    let key = DensityKey { points: None, zoom: 1.0, centre: [0.5, 0.5], legend: Vec::new(), similar: None, spread: 1 };
    let small = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
    let large = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 300.0));
    assert_eq!(density_plan(None, &key, small, 0.0, 0.0), DensityPlan::Build, "nothing drawn yet");
    assert_eq!(density_plan(Some((&key, small)), &key, small, 0.0, 9.0), DensityPlan::Keep);
    assert_eq!(density_plan(Some((&key, small)), &key, large, 1.0, 1.1), DensityPlan::Scale, "being resized");
    assert_eq!(density_plan(Some((&key, small)), &key, large, 1.0, 1.2), DensityPlan::Build, "settled");
    let zoomed = DensityKey { zoom: 2.0, ..key.clone() };
    assert_eq!(density_plan(Some((&key, small)), &zoomed, large, 1.0, 1.0), DensityPlan::Build, "zoomed");
    let filtered = DensityKey { legend: vec![Kind::Kick], ..key.clone() };
    assert_eq!(density_plan(Some((&key, small)), &filtered, small, 0.0, 0.0), DensityPlan::Build);
}

#[test]
fn a_scaled_raster_keeps_the_points_where_they_are_drawn_now() {
    let view = MapView::default();
    let before = view.transform(Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(400.0, 300.0)));
    let after = view.transform(Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(800.0, 500.0)));
    let raster = before.rect;
    let moved = moved(raster, before, after);
    for p in [[0.2, 0.3], [0.5, 0.5], [0.9, 0.1]] {
        // Where a point sat in the old raster, as a fraction of it, is where it is drawn now.
        let old = before.to_screen(p);
        let fraction = (old - raster.min) / raster.size();
        let now = moved.min + fraction * moved.size();
        assert!((now - after.to_screen(p)).length() < 1e-3, "{p:?}: {now:?} against {:?}", after.to_screen(p));
    }
}

#[test]
fn resizing_the_cloud_keeps_the_counts_and_clusters_but_the_grid_works_them_out_again() {
    let points: Vec<MapPoint> = (0..50).map(|i| point(i as f32 / 50.0, 0.5, Kind::Kick)).collect();
    let positions: Vec<[f32; 2]> = points.iter().map(|p| p.position).collect();
    let mut view = MapView::default();
    view.refresh(&points, &positions, 1.0);
    let cloud = view.cache.key;
    view.refresh(&points, &positions, 1.6);
    assert_eq!(view.cache.key, cloud, "the cloud's positions do not follow the aspect");
    view.layout = Layout::Grid;
    view.refresh(&points, &positions, 1.0);
    let grid = view.cache.key;
    view.refresh(&points, &positions, 1.6);
    assert_ne!(view.cache.key, grid, "the grid's positions follow the aspect");
}

#[test]
fn transform_round_trips_and_zoom_keeps_the_point_under_the_pointer() {
    let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(400.0, 300.0));
    let mut view = MapView::default();
    let t = view.transform(rect);
    let p = [0.3, 0.8];
    let back = t.to_map(t.to_screen(p));
    assert!((back[0] - p[0]).abs() < 1e-5 && (back[1] - p[1]).abs() < 1e-5);
    let pointer = Pos2::new(150.0, 100.0);
    let before = view.transform(rect).to_map(pointer);
    view.zoom_at(rect, pointer, 2.0);
    let after = view.transform(rect).to_map(pointer);
    assert!((before[0] - after[0]).abs() < 1e-4 && (before[1] - after[1]).abs() < 1e-4);
    view.zoom_at(rect, pointer, 1e-6);
    assert_eq!(view.zoom, MIN_ZOOM);
    view.fit();
    assert_eq!((view.zoom, view.centre), (1.0, [0.5, 0.5]));
}

#[test]
fn the_nearest_point_within_reach_is_hit() {
    let screen = [Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0), Pos2::new(4.0, 0.0)];
    assert_eq!(hit(&screen, Pos2::new(5.0, 1.0), 7.0), Some(2));
    assert_eq!(hit(&screen, Pos2::new(50.0, 50.0), 7.0), None);
    assert_eq!(hit(&[], Pos2::ZERO, 7.0), None);
}

#[test]
fn arrow_keys_step_to_the_nearest_point_that_way() {
    let positions = [[0.5, 0.5], [0.6, 0.5], [0.9, 0.52], [0.5, 0.2], [0.4, 0.9]];
    assert_eq!(step(&positions, 0, [1.0, 0.0]), Some(1));
    assert_eq!(step(&positions, 1, [1.0, 0.0]), Some(2));
    assert_eq!(step(&positions, 0, [0.0, -1.0]), Some(3));
    assert_eq!(step(&positions, 0, [0.0, 1.0]), Some(4));
    assert_eq!(step(&positions, 2, [1.0, 0.0]), None);
    assert_eq!(step(&positions, 99, [1.0, 0.0]), None);
}

#[test]
fn the_legend_counts_and_the_clusters_follow_the_points() {
    let mut points: Vec<MapPoint> = (0..10).map(|i| point(0.1 + i as f32 * 0.01, 0.1, Kind::Kick)).collect();
    points.extend((0..3).map(|i| point(0.9, 0.9 + i as f32 * 0.01, Kind::Snare)));
    let positions: Vec<[f32; 2]> = points.iter().map(|p| p.position).collect();
    let mut view = MapView::default();
    view.refresh(&points, &positions, 1.0);
    assert!(view.cache.counts.contains(&(Kind::Kick, 10)) && view.cache.counts.contains(&(Kind::Snare, 3)));
    // Only the kick cluster is large enough for a label.
    assert_eq!(view.cache.clusters.iter().map(|c| c.kind).collect::<Vec<_>>(), vec![Kind::Kick]);
    // Kept until the points change.
    for p in &mut points {
        p.matches = false;
    }
    view.refresh(&points, &positions, 1.0);
    assert_eq!(view.cache.clusters.len(), 1);
    view.points_changed();
    view.refresh(&points, &positions, 1.0);
    assert!(view.cache.clusters.is_empty());
}

#[test]
fn grid_positions_are_cell_centres() {
    let points = vec![point(0.0, 0.0, Kind::Kick), point(1.0, 1.0, Kind::Kick)];
    let grid = Grid { columns: 2, rows: 1, cells: vec![(0, 0), (1, 0)] };
    // A 2 by 1 grid sits in the middle of a square view.
    assert_eq!(positions(&points, Layout::Grid, Some(&grid), 1.0), vec![[0.25, 0.5], [0.75, 0.5]]);
    assert_eq!(positions(&points, Layout::Cloud, Some(&grid), 1.0), vec![[0.0, 0.0], [1.0, 1.0]]);
    // In a view twice as wide, a 4 by 2 grid spreads over the whole width.
    let wide: Vec<MapPoint> = (0..8).map(|_| point(0.5, 0.5, Kind::Kick)).collect();
    let grid = Grid { columns: 4, rows: 2, cells: (0..8).map(|i| (i % 4, i / 4)).collect() };
    let placed = positions(&wide, Layout::Grid, Some(&grid), 2.0);
    assert_eq!((placed[0], placed[7]), ([-0.25, 0.25], [1.25, 0.75]));
    // A grid that does not match the points falls back to the cloud.
    let wrong = Grid { columns: 1, rows: 1, cells: vec![(0, 0)] };
    assert_eq!(positions(&points, Layout::Grid, Some(&wrong), 1.0), vec![[0.0, 0.0], [1.0, 1.0]]);
}

#[test]
fn the_legend_shows_one_kind_or_several() {
    let mut legend = HashSet::new();
    toggle_legend(&mut legend, Kind::Kick, false);
    assert_eq!(legend, HashSet::from([Kind::Kick]));
    toggle_legend(&mut legend, Kind::Snare, true);
    assert_eq!(legend, HashSet::from([Kind::Kick, Kind::Snare]));
    toggle_legend(&mut legend, Kind::Hat, false);
    assert_eq!(legend, HashSet::from([Kind::Hat]));
    toggle_legend(&mut legend, Kind::Hat, false);
    assert!(legend.is_empty());
}

#[test]
fn every_kind_has_its_own_colour() {
    let colours: HashSet<Color32> = Kind::ALL.iter().map(|k| kind_colour(*k)).collect();
    assert_eq!(colours.len(), Kind::ALL.len());
}

/// The Similar list's scroll bar is dragged like a device panel's, beside the map that takes
/// drags of its own (#867).
#[test]
fn the_similar_lists_scroll_bar_can_be_dragged() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;
    let points: Vec<MapPoint> = (0..60).map(|i| point(i as f32 / 60.0, 0.5, Kind::Kick)).collect();
    let mut view = MapView::default();
    view.show_similar(0, (0..60).collect());
    let mut harness = Harness::builder().with_size(egui::vec2(600.0, 340.0)).build_ui_state(
        |ui, (view, events): &mut (MapView, Vec<MapEvent>)| {
            show(ui, view, &points, None, "60 samples", &[], None, &[], events)
        },
        (view, Vec::new()),
    );
    harness.run();
    let first = points.first().map(|p| p.name.clone()).unwrap_or_default();
    let top = harness.get_by_label(&first).rect().top();
    let bar = harness.get_by_role(egui::accesskit::Role::ScrollBar).rect();
    let from = Pos2::new(bar.center().x, bar.top() + 4.0);
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    for y in [10.0, 30.0, 60.0] {
        harness.hover_at(from + Vec2::new(0.0, y));
        harness.step();
    }
    harness.drop_at(from + Vec2::new(0.0, 60.0));
    harness.run();
    let moved = harness.query_by_label(&first).map(|node| node.rect().top());
    assert!(moved.is_none_or(|moved| moved < top - 20.0), "the list follows the bar: {top} to {moved:?}");
    assert!(harness.state().1.is_empty(), "the map under the bar took nothing: {:?}", harness.state().1);
}

#[test]
fn hovering_previews_and_clicking_selects() {
    use egui_kittest::Harness;
    let points: Vec<MapPoint> =
        (0..20).map(|i| point((i % 5) as f32 / 4.0, (i / 5) as f32 / 3.0, Kind::Kick)).collect();
    let state = (MapView::default(), Vec::<MapEvent>::new());
    let mut harness = Harness::builder().with_size(egui::vec2(400.0, 340.0)).build_ui_state(
        |ui, (view, events): &mut (MapView, Vec<MapEvent>)| {
            show(ui, view, &points, None, "20 samples", &[], None, &[], events)
        },
        state,
    );
    harness.run();
    // Point 0 at map (0, 0): find it on screen from the same transform the view uses.
    let view = &harness.state().0;
    let at = view.transform(view.area).to_screen([0.0, 0.0]);
    harness.hover_at(at);
    harness.run();
    std::thread::sleep(std::time::Duration::from_millis(60));
    harness.hover_at(at + Vec2::new(0.5, 0.0));
    harness.run();
    assert!(harness.state().1.contains(&MapEvent::Preview(0)), "{:?}", harness.state().1);
    harness.state_mut().1.clear();
    harness.drag_at(at);
    harness.run();
    harness.drop_at(at);
    harness.run();
    assert!(harness.state().1.contains(&MapEvent::Select { items: vec![0], add: false }), "{:?}", harness.state().1);
}

/// A drag carries the point the button went down on, even when the pointer is nearer
/// another point by the time the drag begins, and nothing is hovered or previewed on the way.
#[test]
fn a_drag_carries_the_pressed_point_not_the_one_under_the_pointer() {
    use egui_kittest::Harness;
    // A row of points 0.02 apart: a few pixels on screen, a dense cloud.
    let points: Vec<MapPoint> = (0..40).map(|i| point(0.1 + i as f32 * 0.02, 0.5, Kind::Kick)).collect();
    let state = (MapView::default(), Vec::<MapEvent>::new());
    let mut harness = Harness::builder().with_size(egui::vec2(400.0, 340.0)).build_ui_state(
        |ui, (view, events): &mut (MapView, Vec<MapEvent>)| {
            show(ui, view, &points, None, "40 samples", &[], None, &[], events)
        },
        state,
    );
    harness.run();
    let view = &harness.state().0;
    let transform = view.transform(view.area);
    let first = transform.to_screen([0.1, 0.5]);
    let far = transform.to_screen([0.1 + 30.0 * 0.02, 0.5]);
    harness.drag_at(first);
    harness.run();
    for step in 1..=10 {
        harness.hover_at(first + (far - first) * (step as f32 / 10.0));
        harness.run();
    }
    let events = &harness.state().1;
    let drags: Vec<&MapEvent> = events.iter().filter(|e| matches!(e, MapEvent::StartDrag(_))).collect();
    assert_eq!(drags, vec![&MapEvent::StartDrag(vec![0])], "{events:?}");
    let after = events.iter().position(|e| matches!(e, MapEvent::StartDrag(_))).unwrap_or(0);
    assert!(!events[after..].iter().any(|e| matches!(e, MapEvent::Preview(_) | MapEvent::Select { .. })), "{events:?}");
    harness.drop_at(far);
    harness.run();
}

/// However narrow the map, its controls never lie on top of each other: they wrap onto
/// more rows, and the map starts below them.
#[test]
fn the_controls_wrap_instead_of_overlapping_when_the_map_is_narrow() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let points: Vec<MapPoint> = (0..4).map(|i| point(i as f32 / 4.0, 0.5, Kind::Kick)).collect();
    let status = "153534 samples, 25947 too long, 4955 could not be read";
    let mut tops = Vec::new();
    for width in [180.0, 320.0, 900.0] {
        let mut harness = Harness::builder().with_size(egui::vec2(width, 400.0)).build_ui_state(
            |ui, view: &mut MapView| show(ui, view, &points, None, status, &[], None, &[], &mut Vec::new()),
            MapView::default(),
        );
        harness.run();
        let rects: Vec<(String, Rect)> = ["Cloud", "Grid", "+", "−", "Fit", "Large", "Window", "Size", "Volume"]
            .iter()
            .map(|label| (label.to_string(), harness.get_by_label(label).rect()))
            .collect();
        for (i, (a, ra)) in rects.iter().enumerate() {
            assert!(ra.right() <= width + 0.5, "{a} runs off a map {width} wide: {ra:?}");
            for (b, rb) in rects.iter().skip(i + 1) {
                assert!(!ra.shrink(0.5).intersects(rb.shrink(0.5)), "{a} and {b} overlap at {width}: {ra:?} {rb:?}");
            }
        }
        let lowest = rects.iter().map(|(_, r)| r.bottom()).fold(0.0f32, f32::max);
        assert!(harness.state().area.top() >= lowest, "the map starts below the controls at {width}");
        tops.push(harness.state().area.top());
    }
    assert!(tops[0] > tops[2], "a narrow map has more rows of controls than a wide one: {tops:?}");
}

#[test]
fn the_volume_slider_reports_a_new_level() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let points: Vec<MapPoint> = (0..4).map(|i| point(i as f32 / 4.0, 0.5, Kind::Kick)).collect();
    let state = (MapView::default(), Vec::<MapEvent>::new());
    let mut harness = Harness::builder().with_size(egui::vec2(900.0, 400.0)).build_ui_state(
        |ui, (view, events): &mut (MapView, Vec<MapEvent>)| {
            show(ui, view, &points, None, "4 samples", &[], None, &[], events)
        },
        state,
    );
    harness.run();
    let slider = harness.get_by_role(egui::accesskit::Role::Slider).rect();
    // The right end of the slider is the loudest.
    harness.drag_at(egui::pos2(slider.right() - 1.0, slider.center().y));
    harness.run();
    harness.drop_at(egui::pos2(slider.right() - 1.0, slider.center().y));
    harness.run();
    let (low, high) = crate::settings::PREVIEW_VOLUMES;
    assert!(harness.state().1.contains(&MapEvent::Volume(high)), "{:?}", harness.state().1);
    assert_eq!(harness.state().0.volume, high);
    assert!(high > low);
}

/// Pointing at the status shows why samples are missing.
#[test]
fn the_status_tooltip_says_why_samples_are_missing() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let points: Vec<MapPoint> = (0..4).map(|i| point(i as f32 / 4.0, 0.5, Kind::Kick)).collect();
    let details = vec![StatusPart {
        heading: "2 could not be read".into(),
        lines: vec!["2: unsupported codec: adpcm".into(), "    a.wav, b.wav".into()],
    }];
    let status = "4 samples, 2 could not be read";
    let mut harness = Harness::builder().with_size(egui::vec2(600.0, 340.0)).build_ui_state(
        |ui, view: &mut MapView| show(ui, view, &points, None, status, &details, None, &[], &mut Vec::new()),
        MapView::default(),
    );
    harness.run();
    assert!(harness.query_by_label("2: unsupported codec: adpcm").is_none());
    let at = harness.get_by_label(status).rect().center();
    harness.hover_at(at);
    for _ in 0..30 {
        harness.step();
    }
    std::thread::sleep(std::time::Duration::from_millis(600));
    harness.run();
    harness.get_by_label("2 could not be read");
    harness.get_by_label("2: unsupported codec: adpcm");
    harness.get_by_label("    a.wav, b.wav");
}

/// The card over the map while it is arranged is only painted: a point under it is still
/// previewed and selected.
#[test]
fn the_busy_card_does_not_block_the_map_under_it() {
    use egui_kittest::Harness;
    let points: Vec<MapPoint> = (0..20).map(|_| point(0.5, 0.5, Kind::Kick)).collect();
    let busy = Busy { title: "Arranging the map".into(), detail: "42 %".into(), fraction: 0.42 };
    let state = (MapView::default(), Vec::<MapEvent>::new());
    let mut harness = Harness::builder().with_size(egui::vec2(400.0, 340.0)).build_ui_state(
        |ui, (view, events): &mut (MapView, Vec<MapEvent>)| {
            show(ui, view, &points, None, "20 samples", &[], Some(&busy), &[], events)
        },
        state,
    );
    harness.run();
    // Every point is in the middle, under the card.
    let view = &harness.state().0;
    let at = view.transform(view.area).to_screen([0.5, 0.5]);
    harness.drag_at(at);
    harness.run();
    harness.drop_at(at);
    harness.run();
    assert!(
        harness.state().1.iter().any(|e| matches!(e, MapEvent::Select { items, .. } if !items.is_empty())),
        "{:?}",
        harness.state().1
    );
}

/// The wheel and pinch zoom the map, but not through a dialog over it (#232).
#[test]
fn the_wheel_zooms_only_when_no_dialog_is_on_top() {
    use egui_kittest::Harness;
    let points: Vec<MapPoint> =
        (0..20).map(|i| point((i % 5) as f32 / 4.0, (i / 5) as f32 / 3.0, Kind::Kick)).collect();
    let size = egui::vec2(400.0, 340.0);
    // The view, and whether a dialog covers it.
    let state = (MapView::default(), false);
    let mut harness = Harness::builder().with_size(size).build_ui_state(
        |ui, (view, dialog): &mut (MapView, bool)| {
            show(ui, view, &points, None, "20 samples", &[], None, &[], &mut Vec::new());
            if *dialog {
                egui::Area::new(egui::Id::new("dialog")).order(egui::Order::Foreground).fixed_pos(Pos2::ZERO).show(
                    ui.ctx(),
                    |ui| {
                        ui.set_min_size(size);
                        let _ = ui.interact(Rect::from_min_size(Pos2::ZERO, size), ui.id(), Sense::click());
                    },
                );
            }
        },
        state,
    );
    harness.run();
    let wheel = |harness: &mut Harness<'_, (MapView, bool)>| {
        let at = harness.state().0.area.center();
        harness.hover_at(at);
        harness.run();
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, 120.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        harness.input_mut().events.push(egui::Event::Zoom(1.5));
        harness.run();
        harness.state().0.zoom
    };
    harness.state_mut().1 = true;
    harness.run();
    assert_eq!(wheel(&mut harness), 1.0, "the dialog takes the wheel");
    harness.state_mut().1 = false;
    harness.run();
    assert!(wheel(&mut harness) > 1.0, "without it the map zooms");
}
