// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Arrangement: clips, lanes, markers, the loop brace, the ruler, fades, headers and groups.

use super::*;

#[test]
fn dragging_a_clip_moves_it_on_the_bar_grid() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    // 8 points a beat by default, so 64 points are two bars.
    let to = from + egui::vec2(64.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(30.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let starts: Vec<f64> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect())
        .unwrap_or_default();
    assert!(starts.contains(&72.0), "{starts:?}");
    assert!(!starts.contains(&64.0), "{starts:?}");
}

#[test]
fn moving_the_view_with_the_middle_button_over_a_clip_leaves_the_clip_alone() {
    let mut harness = arrangement_app();
    let starts = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect::<Vec<_>>())
    };
    let before = starts(&harness);
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: Modifiers::NONE,
    };
    harness.hover_at(from);
    harness.run();
    harness.input_mut().events.push(button(from, true));
    harness.run();
    for step in 1..=4 {
        harness.hover_at(from + egui::vec2(-20.0 * step as f32, 0.0));
        harness.run();
    }
    let to = from + egui::vec2(-80.0, 0.0);
    harness.input_mut().events.push(button(to, false));
    harness.run();
    assert_eq!(starts(&harness), before);
}

/// Clicks the name bar of the clip labelled `label`, which picks it whole.
fn click_name_bar(harness: &mut Harness<'static, App>, label: &str) {
    let at = harness.get_by_label(label).rect().center_top() + egui::vec2(0.0, 6.0);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
}

/// Drags the clip labelled `label` by `by` points in the Arrangement view.
fn drag_clip(harness: &mut Harness<'static, App>, label: &str, by: egui::Vec2) {
    // By its name bar: its body selects time.
    let from = harness.get_by_label(label).rect().center_top() + egui::vec2(0.0, 6.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + by * 0.5);
    harness.run();
    harness.hover_at(from + by);
    harness.run();
    harness.drop_at(from + by);
    harness.run();
}

#[test]
fn a_dropped_clip_replaces_what_it_covers_unless_midi_stacking_is_on() {
    let starts = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
    };
    // Beat B (bars 17-28) moves two bars later, over Fill (bars 29-32), which is shortened.
    let mut harness = arrangement_app();
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    assert_eq!(starts(&harness).map(|s| s.contains(&(120.0, 8.0))), Some(true));

    let mut harness = arrangement_app();
    let settings = suhina_ui::settings::Settings { midi_stacking: true, ..Default::default() };
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    assert_eq!(starts(&harness).map(|s| s.contains(&(112.0, 16.0))), Some(true));
}

#[test]
fn dragging_a_clip_down_moves_it_to_the_next_track() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    // Lanes are 48 points high with a line between them.
    let to = from + egui::vec2(0.0, 49.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(0.0, 20.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let project = harness.state().session().map(|s| s.project().clone()).unwrap_or_default();
    let drums: Vec<f64> = project.arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    let bass: Vec<f64> = project.arrangement.lanes[1].clips.iter().map(|c| c.start).collect();
    assert!(!drums.contains(&64.0), "{drums:?}");
    assert!(bass.contains(&64.0), "{bass:?}");
}

/// The demo project in the Arrangement view with `renderer` (stopped, made from the demo
/// project) as its player, driven by the test instead of an audio device.
fn arrangement_app_with_engine(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// Renders a block, so the engine takes the commands sent so far.
fn render_a_block(renderer: &mut suhina_engine::Renderer) {
    renderer.process(&mut [0.0f32; 2 * 64]);
}

/// Markers under the ruler: M adds one at the play start marker, a right-click on the marker row
/// adds one there on the grid, a click on a marker jumps to it and loops to the next marker, its
/// menu deletes it.
#[test]
fn markers_are_added_jumped_to_and_deleted_from_the_marker_row() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let monitor = renderer.monitor();
    let markers = |h: &Harness<'static, App>| -> Vec<(f64, String)> {
        h.state()
            .session()
            .map_or(Vec::new(), |s| s.project().arrangement.markers.iter().map(|m| (m.beat, m.name.clone())).collect())
    };
    // The play start marker to bar 5 (beat 16), then M adds a marker there.
    let ruler = harness.get_by_label("Ruler").rect();
    let at = egui::pos2(ruler.left() + 130.0, ruler.center().y);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
    render_a_block(&mut renderer);
    harness.key_press(Key::M);
    harness.run();
    assert_eq!(markers(&harness), [(16.0, "Marker 1".to_string())]);

    // A right-click in the middle of the row offers a marker on the bar there (8 points a beat).
    let row = harness.get_by_label("Markers").rect();
    let bar = ((f64::from(row.center().x - row.left()) / 8.0) / 4.0).round() * 4.0;
    assert!(bar > 16.0, "{bar}");
    harness.get_by_label("Markers").click_secondary();
    harness.run();
    harness.get_by_label("Add marker here").click();
    harness.run();
    assert_eq!(markers(&harness), [(16.0, "Marker 1".to_string()), (bar, "Marker 2".to_string())]);

    // A click on a marker moves the play start marker there.
    harness.get_by_label("Marker Marker 2").click();
    harness.run();
    render_a_block(&mut renderer);
    assert_eq!(monitor.start_beat(), bar);
    let looped = |h: &Harness<'static, App>| {
        let transport = h.state().session().map(|s| s.project().transport.clone()).unwrap_or_default();
        (transport.loop_range.map(|r| (r.start, r.end)), transport.loop_enabled)
    };
    assert_ne!(looped(&harness), (Some((16.0, bar)), true), "the last marker only jumps");

    // A click on a marker with one after it loops from it to the next.
    harness.get_by_label("Marker Marker 1").click();
    harness.run();
    render_a_block(&mut renderer);
    assert_eq!(looped(&harness), (Some((16.0, bar)), true));
    assert_eq!(monitor.start_beat(), 16.0);

    // Its menu deletes it, and undo brings it back.
    harness.get_by_label("Marker Marker 1").click_secondary();
    harness.run();
    harness.get_by_label("Delete marker").click();
    harness.run();
    assert_eq!(markers(&harness), [(bar, "Marker 2".to_string())]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(markers(&harness).len(), 2);

    // Dragged two bars to the right (64 points), it lands on the bar grid there.
    let from = harness.get_by_label("Marker Marker 1").rect().left_center() + egui::vec2(4.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(30.0, 0.0));
    harness.run();
    harness.hover_at(from + egui::vec2(64.0, 0.0));
    harness.run();
    harness.drop_at(from + egui::vec2(64.0, 0.0));
    harness.run();
    assert_eq!(markers(&harness)[0], (24.0, "Marker 1".to_string()));
}

/// The loop brace row under the ruler: a drag draws a region on the bar grid and turns looping
/// on, Ctrl+L and a click on the brace turn it off and on, its end and its middle are dragged,
/// and undo takes an edit back (`docs/spec/arrangement-editing.md` › Loop region).
#[test]
fn a_loop_region_is_drawn_resized_moved_and_toggled_in_the_loop_brace_row() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let looped = |h: &Harness<'static, App>| {
        let transport = h.state().session().map(|s| s.project().transport.clone()).unwrap_or_default();
        (transport.loop_range.map(|r| (r.start, r.end)), transport.loop_enabled)
    };
    let drag = |h: &mut Harness<'static, App>, from: egui::Pos2, by: f32| {
        h.hover_at(from);
        h.run();
        h.drag_at(from);
        h.run();
        h.hover_at(from + egui::vec2(by / 2.0, 0.0));
        h.run();
        h.hover_at(from + egui::vec2(by, 0.0));
        h.run();
        h.drop_at(from + egui::vec2(by, 0.0));
        h.run();
    };
    // 8 points a beat: from beat 4 to beat 12, a little off the bars, lands on them.
    let row = harness.get_by_label("Loop brace").rect();
    let at_beat = |beat: f32| egui::pos2(row.left() + beat * 8.0, row.center().y);
    drag(&mut harness, at_beat(4.0) + egui::vec2(3.0, 0.0), 8.0 * 8.0 - 5.0);
    assert_eq!(looped(&harness), (Some((4.0, 12.0)), true));

    // Ctrl+L turns it off, a click on the brace on again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::L);
    harness.run();
    assert_eq!(looped(&harness), (Some((4.0, 12.0)), false));
    harness.hover_at(at_beat(8.0));
    harness.run();
    harness.drag_at(at_beat(8.0));
    harness.drop_at(at_beat(8.0));
    harness.run();
    assert_eq!(looped(&harness), (Some((4.0, 12.0)), true));

    // The end dragged two bars right, then the whole brace one bar right.
    drag(&mut harness, at_beat(12.0), 64.0);
    assert_eq!(looped(&harness), (Some((4.0, 20.0)), true));
    drag(&mut harness, at_beat(10.0), 32.0);
    assert_eq!(looped(&harness), (Some((8.0, 24.0)), true));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(looped(&harness), (Some((4.0, 20.0)), true));
}

#[test]
fn a_click_on_the_ruler_moves_the_play_start_marker_on_the_grid() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let monitor = renderer.monitor();
    let ruler = harness.get_by_label("Ruler").rect();
    // 8 points a beat by default, so the grid is a bar: 130 points in is beat 16.25, bar 5.
    let at = egui::pos2(ruler.left() + 130.0, ruler.center().y);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
    render_a_block(&mut renderer);
    assert_eq!(monitor.start_beat(), 16.0);
    assert!(!monitor.running());

    // Play starts there; Stop goes back to it, as the setting says by default.
    harness.get_by_label("Play").click();
    harness.run();
    render_a_block(&mut renderer);
    assert!(monitor.running() && monitor.beat() >= 16.0);
    for _ in 0..400 {
        render_a_block(&mut renderer);
    }
    harness.get_by_label("Stop").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat(), monitor.beat()), (false, 16.0, 16.0));

    // With Stop set to stay, the marker moves to where it stopped.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Transport").click();
    harness.run();
    harness.get_by_label("Stop: Stay").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_changed_settings().map(|s| s.stop),
        Some(suhina_ui::settings::StopReturn::Here)
    );
    harness.get_by_label("Close settings").click();
    harness.run();
    harness.get_by_label("Play").click();
    harness.run();
    for _ in 0..375 {
        render_a_block(&mut renderer);
    }
    harness.get_by_label("Stop").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    render_a_block(&mut renderer);
    // 375 blocks of 64 frames at 120 bpm are one beat, and the blocks around Play and Stop.
    assert!((monitor.start_beat() - 17.0).abs() < 0.03, "{}", monitor.start_beat());
}

#[test]
fn play_starts_from_the_selected_clip_with_shift_space_or_the_setting() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let monitor = renderer.monitor();
    let stop = |harness: &mut Harness<'static, App>, renderer: &mut suhina_engine::Renderer| {
        harness.get_by_label("Stop").click();
        harness.run_steps(2);
        render_a_block(renderer);
    };

    // By default Play starts from the selected clip: picked whole, by its name bar.
    click_name_bar(&mut harness, "Clip Beat B on Drums at bar 17");
    harness.get_by_label("Play").click();
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 64.0));
    stop(&mut harness, &mut renderer);

    // Shift+Space plays from the selected clip's start (bar 17, beat 64), and Stop returns there.
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Space);
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 64.0));
    for _ in 0..100 {
        render_a_block(&mut renderer);
    }
    stop(&mut harness, &mut renderer);
    assert_eq!((monitor.running(), monitor.beat()), (false, 64.0));

    // With Play from set to the selected clip, Space and the Play button start there too.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Transport").click();
    harness.run();
    harness.get_by_label("Play from: Marker").click();
    harness.run();
    harness.get_by_label("Play from: Selected clip").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_changed_settings().map(|s| s.play_from),
        Some(suhina_ui::settings::PlayFrom::Clip)
    );
    harness.get_by_label("Close settings").click();
    harness.run();
    click_name_bar(&mut harness, "Clip Beat A on Drums at bar 1");
    harness.key_press(Key::Space);
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 0.0));
    stop(&mut harness, &mut renderer);
    click_name_bar(&mut harness, "Clip Beat B on Drums at bar 17");
    harness.get_by_label("Play").click();
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 64.0));
    stop(&mut harness, &mut renderer);

    // A click in the body of the selected clip sets where play starts, the clip still picked:
    // two bars in (8 points a beat), Play starts there and not from the clip's start.
    let body = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().left_center() + egui::vec2(64.0, 0.0);
    harness.hover_at(body);
    harness.run();
    harness.drag_at(body);
    harness.drop_at(body);
    harness.run_steps(2);
    assert!(harness.state().arrangement().selected_clip().is_some(), "the clip stays picked");
    harness.get_by_label("Play").click();
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 72.0));
    stop(&mut harness, &mut renderer);

    // In the Session view the selection is not shown, so Play starts from the marker; stopping
    // again first puts the marker at the beginning.
    stop(&mut harness, &mut renderer);
    assert_eq!(monitor.start_beat(), 0.0);
    harness.get_by_label("Session view").click();
    harness.run();
    harness.key_press(Key::Space);
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 0.0));
}

#[test]
fn arrangement_headers_turn_tracks_off() {
    let mut harness = arrangement_app();
    harness.get_by_label("Turn Keys off").click();
    harness.run();
    assert_eq!(harness.state().session().map(|s| s.project().channels[2].mute), Some(true));
    assert!(harness.query_by_label("Keys volume").is_some());
    assert!(harness.query_by_label("Keys pan").is_some());
}

#[test]
fn ctrl_t_adds_an_audio_track_and_ctrl_shift_t_a_midi_one() {
    let mut harness = window(demo_app());
    // (audio tracks, MIDI tracks)
    let kinds = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        (p.tracks.iter().filter(|t| t.content.audio).count(), p.tracks.iter().filter(|t| t.content.notes).count())
    };
    let (audio, midi) = kinds(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::T);
    harness.run();
    assert_eq!(kinds(&harness), (audio + 1, midi), "Ctrl+T: an audio track");
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    harness.run();
    assert_eq!(kinds(&harness), (audio + 1, midi + 1), "Ctrl+Shift+T: a MIDI track");
}

#[test]
fn a_tall_header_routes_its_track_into_a_group() {
    struct Arrangement {
        timeline: Timeline,
        state: ArrangementViewState,
        actions: Vec<Action>,
    }
    let mut state = ArrangementViewState::default();
    state.set_lane_height(suhina_engine::model::TrackId(2), 90.0);
    let view = Arrangement { timeline: Timeline::build(&project_with_a_group(), None), state, actions: Vec::new() };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut Arrangement| arrangement_view::show(ui, &v.timeline, &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    // Only the tall lane shows its output.
    assert_eq!(harness.query_all_by_label_contains("output:").count(), 1);
    harness.get_by_label("Bass output: Main").click();
    harness.run();
    harness.get_by_label("Group").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("track.setOutput", json!({ "track": 2, "to": 20 }))]);
}

#[test]
fn selected_tracks_group_with_ctrl_g() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert_eq!(names(&harness)[..3], ["Group", "Drums", "Bass"]);
    assert!(harness.query_by_label("Track Group").is_some());
}

#[test]
fn shift_clicking_a_header_takes_the_run_of_tracks_between() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Keys").click_modifiers(Modifiers::SHIFT);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Group", "Drums", "Bass", "Keys"]);
}

/// How many arrangement clips each of the first four tracks has.
fn clip_counts(h: &Harness<'static, App>) -> Vec<usize> {
    h.state()
        .session()
        .map(|s| {
            let p = s.project();
            p.tracks
                .iter()
                .filter_map(|t| p.arrangement.lanes.iter().find(|l| l.track == Some(t.id)))
                .map(|l| l.clips.len())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn shift_clicking_a_clip_takes_the_run_between_and_the_key_mutes_it() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click_modifiers(Modifiers::SHIFT);
    harness.run();
    // The run from bar 1 to bar 17 on Drums: the first two clips, and no others.
    harness.key_press(Key::Num0);
    harness.run();
    let muted: Vec<bool> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.enabled).collect())
        .unwrap_or_default();
    assert_eq!(muted, [false, false, true, true], "the run went quiet, the rest plays on");

    // The same key again turns the selection back on.
    harness.key_press(Key::Num0);
    harness.run();
    let back: Vec<bool> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.enabled).collect())
        .unwrap_or_default();
    assert_eq!(back, [true, true, true, true]);
}

#[test]
fn ctrl_clicking_adds_a_clip_and_cut_takes_the_selection_in_one_step() {
    let mut harness = arrangement_app();
    let before = clip_counts(&harness);
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::X);
    harness.run();
    assert_eq!(clip_counts(&harness)[0], before[0] - 2, "both clicked clips went together");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(clip_counts(&harness), before, "one undo brings both back");
}

#[test]
fn select_all_and_delete_clear_the_arrangement_in_one_step() {
    let mut harness = arrangement_app();
    let before = clip_counts(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert!(clip_counts(&harness).iter().all(|n| *n == 0), "{:?}", clip_counts(&harness));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(clip_counts(&harness), before, "one undo brings the whole arrangement back");
}

#[test]
fn a_clips_menu_mutes_the_clip_it_fell_on() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click_secondary();
    harness.run();
    harness.get_by_label_contains("Mute").click();
    harness.run();
    assert!(!harness.state().session().is_some_and(|s| s.project().arrangement.lanes[0].clips[0].enabled));

    // With the clip off, the menu offers to turn it back on.
    harness.get_by_label("Clip Beat A on Drums at bar 1").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("Unmute").is_some());
}

#[test]
fn a_groups_fold_triangle_hides_its_tracks_and_draws_their_clips_on_its_row() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some(), "a group's tracks keep their lanes");

    harness.get_by_label("Fold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Group").is_some(), "the group keeps its row");
    assert!(harness.query_by_label("Track Drums").is_none(), "folding takes its tracks' lanes away");
    assert!(harness.query_by_label("Track Bass").is_none());
    // The group's row draws every track inside it as a thin row of its clips' marks, so it shows
    // how much is in there and where; the clips themselves are picked once it is unfolded.
    assert!(harness.query_by_label_contains("Clip Walk on Bass").is_none(), "marks, not clips to pick");

    harness.get_by_label("Unfold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some(), "unfolding brings the lanes back");
}

#[test]
fn menus_delete_a_track_and_duplicate_a_clip() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label("Delete track").click();
    harness.run();
    assert!(!names(&harness).contains(&"Keys".to_string()));

    let clips =
        |h: &Harness<'static, App>| h.state().session().map_or(0, |s| s.project().arrangement.lanes[0].clips.len());
    let before = clips(&harness);
    harness.get_by_label("Clip Fill on Drums at bar 29").click_secondary();
    harness.run();
    harness.get_by_label_contains("Duplicate").click();
    harness.run();
    // The copy sits after Fill and shortens the clip it lands on, so there is one more clip.
    assert_eq!(clips(&harness), before + 1);
    let fill_copies = harness.state().session().map_or(0, |s| {
        s.project().arrangement.lanes[0].clips.iter().filter(|c| c.start == 128.0 && c.length == 16.0).count()
    });
    assert_eq!(fill_copies, 1, "the copy of Fill sits at bar 33");
}

#[test]
fn dragging_a_header_moves_the_track() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Track Vox").rect().center();
    // Three lanes up (49 points each), into the upper half of Drums.
    let to = from - egui::vec2(0.0, 3.0 * 49.0 + 14.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(0.0, 40.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Vox", "Drums", "Bass", "Keys"]);
}

fn parent_name(h: &Harness<'static, App>, name: &str) -> Option<String> {
    let session = h.state().session()?;
    let p = session.project();
    let parent = p.tracks.iter().find(|t| t.name == name)?.parent?;
    p.track(parent).map(|t| t.name.clone())
}

#[test]
fn groups_nest_fold_and_take_tracks_from_the_menu_and_by_dragging() {
    let mut harness = arrangement_app();
    // Drums into a group, and that group into another.
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    harness.get_by_label("Track Group").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert_eq!(names(&harness)[..3], ["Group", "Group", "Drums"]);
    assert_eq!(parent_name(&harness, "Drums").as_deref(), Some("Group"));

    // Keys into the outer group from its menu.
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label_contains("Move into group").click();
    harness.run();
    harness.get_all_by_label("Group").next().unwrap().click();
    harness.run();
    assert!(parent_name(&harness, "Keys").is_some());

    // Folding the outer group hides everything inside it; unfolding shows it again.
    assert_eq!(harness.query_all_by_label("Fold Group").count(), 2);
    harness.get_all_by_label("Fold Group").next().unwrap().click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_none());
    assert!(harness.query_by_label("Track Keys").is_none());
    assert_eq!(harness.query_all_by_label("Track Group").count(), 1);
    harness.get_by_label("Unfold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some());

    // Keys out of its group again, from its menu.
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label("Move out of group").click();
    harness.run();
    assert_eq!(parent_name(&harness, "Keys"), None);
    assert_eq!(harness.state().session().map(|s| s.project().validate()), Some(Ok(())));
}

#[test]
fn dragging_a_header_below_a_group_header_puts_the_track_in_the_group() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    let from = harness.get_by_label("Track Vox").rect().center();
    let group = harness.get_by_label("Track Group").rect();
    // The lower half of the group's header.
    let to = egui::pos2(from.x, group.bottom() - 6.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(0.0, 40.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..3], ["Group", "Vox", "Drums"]);
    assert_eq!(parent_name(&harness, "Vox").as_deref(), Some("Group"));
}

#[test]
fn dragging_an_audio_clips_gain_sets_it() {
    let mut harness = window(audio_app());
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let field = harness.get_by_label("Gain of Loop on Loop").rect().center();
    harness.hover_at(field);
    harness.run();
    harness.drag_at(field);
    harness.run();
    harness.hover_at(field - egui::vec2(0.0, 15.0));
    harness.run();
    harness.hover_at(field - egui::vec2(0.0, 30.0));
    harness.run();
    harness.drop_at(field - egui::vec2(0.0, 30.0));
    harness.run();
    let gain = harness.state().session().and_then(|s| {
        s.project().content(suhina_engine::model::ContentId(300)).and_then(|c| match &c.lanes[0].data {
            suhina_engine::model::LaneData::Audio(region) => Some(region.gain),
            suhina_engine::model::LaneData::Notes(_) => None,
        })
    });
    // 30 points up at 0.1 dB a point: +3 dB.
    assert!(gain.is_some_and(|g| (g - 1.4125).abs() < 1e-3), "{gain:?}");
}

/// The Loop clip's fade-in and fade-out in the project.
fn loop_fades(harness: &Harness<'static, App>) -> Option<(f64, f64)> {
    let session = harness.state().session()?;
    let lane =
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(suhina_engine::model::TrackId(30)))?;
    lane.clips.first().map(|clip| (clip.fade_in, clip.fade_out))
}

/// Drags the control labelled `label` sideways by `dx` points.
fn drag_sideways(harness: &mut Harness<'static, App>, label: &str, dx: f32) {
    let from = harness.get_by_label(label).rect().center();
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(dx / 2.0, 0.0));
    harness.run();
    harness.hover_at(from + egui::vec2(dx, 0.0));
    harness.run();
    harness.drop_at(from + egui::vec2(dx, 0.0));
    harness.run();
}

#[test]
fn dragging_a_fade_handle_sets_the_fade_on_the_grid_and_a_double_click_removes_it() {
    let mut harness = window_with_step(audio_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    // The handles show on the selected clip.
    assert!(harness.query_by_label("Fade-in of Loop on Loop").is_none());
    harness.get_by_label("Clip Loop on Loop at bar 1").click();
    harness.run();

    // 8 points a beat and a bar grid at this zoom: 3 beats right ends the fade on bar 2.
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 24.0);
    assert_eq!(loop_fades(&harness), Some((4.0, 0.0)));
    // The fade-out, 3 beats left from the clip's end at beat 16: on bar 4, 4 beats long.
    drag_sideways(&mut harness, "Fade-out of Loop on Loop", -24.0);
    assert_eq!(loop_fades(&harness), Some((4.0, 4.0)));
    // Each drag is one undo step.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(loop_fades(&harness), Some((4.0, 0.0)));

    // A fade cannot reach past the other one: the fade-in stops where the clip has room.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run();
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 200.0);
    assert_eq!(loop_fades(&harness), Some((12.0, 4.0)));

    // A double-click removes the fade.
    let handle = harness.get_by_label("Fade-in of Loop on Loop").rect().center();
    harness.hover_at(handle);
    harness.run();
    // Press and release in one frame each, a few frames apart: a double-click.
    for _ in 0..2 {
        harness.hover_at(handle);
        harness.drag_at(handle);
        harness.drop_at(handle);
        harness.step();
    }
    harness.run();
    assert_eq!(loop_fades(&harness), Some((0.0, 4.0)));
}

/// The Loop clip's fade-in shape and curve in the project.
fn loop_fade_in_curve(harness: &Harness<'static, App>) -> Option<suhina_engine::model::FadeCurve> {
    let session = harness.state().session()?;
    let lane =
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(suhina_engine::model::TrackId(30)))?;
    lane.clips.first().map(|clip| clip.fade_in_curve)
}

/// A right-click on a fade handle picks the shape; the curve handle on the fade's line bends it
/// with a vertical drag and a double-click straightens it (#197).
#[test]
fn a_fades_shape_is_picked_from_its_menu_and_its_curve_dragged() {
    use suhina_engine::model::{FadeCurve, FadeShape};
    let mut harness = window_with_step(audio_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Clip Loop on Loop at bar 1").click();
    harness.run();
    // A 4-beat fade-in, 32 points wide: wide enough for a curve handle.
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 24.0);
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve::default()));

    harness.get_by_label("Fade-in of Loop on Loop").click_secondary();
    harness.run();
    harness.get_by_label("Equal power").click();
    harness.run();
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve { shape: FadeShape::EqualPower, curve: 0.0 }));

    // Down by 12 points: the handle follows the pointer, and the curve bends towards −.
    let handle = harness.get_by_label("Fade-in curve of Loop on Loop").rect();
    let from = handle.center();
    let to = from + egui::vec2(0.0, 12.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from.lerp(to, 0.5));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let bent = loop_fade_in_curve(&harness).map(|c| c.curve).unwrap_or_default();
    assert!(bent < -0.05 && bent > -1.0, "{bent}");
    let now = harness.get_by_label("Fade-in curve of Loop on Loop").rect().center();
    assert!((now.y - to.y).abs() < 1.0, "the handle is at {now:?}, the pointer let go at {to:?}");
    assert_eq!(loop_fade_in_curve(&harness).map(|c| c.shape), Some(FadeShape::EqualPower));

    // A double-click sets the curve back to 0 (half a second after the drag, as a person would).
    let handle = harness.get_by_label("Fade-in curve of Loop on Loop").rect().center();
    for _ in 0..30 {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(handle);
        harness.drag_at(handle);
        harness.drop_at(handle);
        harness.step();
    }
    harness.run();
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve { shape: FadeShape::EqualPower, curve: 0.0 }));
}

#[test]
fn keys_copy_paste_and_delete_the_selected_clip() {
    let mut harness = arrangement_app();
    let spans = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    // The copy lands right after Fill (bar 33) and shortens what it covers.
    assert!(spans(&harness).contains(&(128.0, 16.0)), "{:?}", spans(&harness));

    let before = spans(&harness).len();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(spans(&harness).len(), before - 1);
    assert!(!spans(&harness).contains(&(0.0, 64.0)));
}

/// The selection names a clip by its id (ADR 0025): when a clip before it moves away, the
/// selected clip's index changes, and the key still acts on the clip that was clicked.
#[test]
fn the_selected_clip_stays_selected_when_a_clip_before_it_moves() {
    let mut harness = arrangement_app();
    let drums = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().arrangement.lanes[0].clips.clone()).unwrap_or_default()
    };
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    let fill = drums(&harness).iter().find(|c| c.start == 112.0).map(|c| c.id).unwrap();
    assert_eq!(harness.state().selected_clip(), Some((TrackId(1), fill)));
    let index = |h: &Harness<'static, App>| drums(h).iter().position(|c| c.id == fill);
    let before = index(&harness);

    // Beat A, the first clip on Drums, moves to the end: Fill is one place earlier on the lane.
    let beat_a = drums(&harness)[0].id;
    harness.state_mut().run(&Action::new("clip.move", json!({ "clip": beat_a.0, "start": 400 })));
    harness.run();
    assert_eq!(index(&harness), before.map(|i| i - 1));
    assert_eq!(harness.state().selected_clip(), Some((TrackId(1), fill)), "still Fill");

    // Delete takes away Fill, not the clip that is now where Fill was.
    harness.key_press(Key::Delete);
    harness.run();
    let left = drums(&harness);
    assert!(left.iter().all(|c| c.id != fill), "Fill is gone");
    assert!(left.iter().any(|c| c.id == beat_a), "Beat A is still there");
}

#[test]
fn paste_and_duplicate_keys_follow_the_midi_stacking_setting() {
    let spans = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    for key in [Key::V, Key::D] {
        let mut harness = arrangement_app();
        let settings = suhina_ui::settings::Settings { midi_stacking: true, ..Default::default() };
        let ctx = harness.ctx.clone();
        harness.state_mut().set_settings(&ctx, settings);
        harness.run();
        let before = spans(&harness);
        harness.get_by_label("Clip Beat B on Drums at bar 17").click();
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, key);
        harness.run();
        // The copy of Beat B (bars 17-28) lies over Fill (bars 29-32), which keeps its span.
        let after = spans(&harness);
        assert!(after.contains(&(112.0, 48.0)), "{key:?}: {after:?}");
        assert_eq!(after.len(), before.len() + 1, "{key:?}: {after:?}");
        assert!(before.iter().all(|s| after.contains(s)), "{key:?}: {before:?} -> {after:?}");
    }
}

/// The audio ask is the three of them at once: the double-click makes the empty audio clip,
/// the menu's name field renames it, and the drag to another slot moves it, like a MIDI clip.
#[test]
fn an_audio_clip_renames_and_moves_like_a_midi_one() {
    let mut harness = window_with_step(demo_app_with_name_field(), 1.0 / 60.0);
    // An audio track and its empty clip, made with the double-click.
    harness.get_by_label("+ Audio track").click();
    harness.run();
    let at = harness.get_by_label("Stop Audio 1 (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    harness.run();
    assert!(harness.query_by_label("Launch Audio clip on Audio 1 (scene 1)").is_some());

    // Renaming through the slot's menu: click into the field, select all, type, Enter.
    harness.get_by_label("Launch Audio clip on Audio 1 (scene 1)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Rename clip Audio clip").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label_contains("Rename clip Audio clip").type_text("Break");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Launch Break on Audio 1 (scene 1)").is_some(), "the audio clip took the name");

    // Dragging it to the next scene moves it.
    let from = harness.get_by_label("Launch Break on Audio 1 (scene 1)").rect().center();
    let to = harness.get_by_label("Stop Audio 1 (scene 2)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
    assert!(harness.query_by_label("Launch Break on Audio 1 (scene 2)").is_some(), "the audio clip moved by drag");
    assert!(harness.query_by_label("Stop Audio 1 (scene 1)").is_some(), "the source emptied");
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Moved")),
        "{:?}",
        harness.state().status()
    );
}

#[test]
fn a_double_click_on_an_empty_spot_of_an_audio_lane_makes_a_clip_there() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    // An audio track next to the demo's MIDI ones; its empty lane shows in the arrangement.
    harness.get_by_label("+ Audio track").click();
    harness.run();
    assert!(harness.query_by_label("Lane Audio 1").is_some(), "the lane is there");
    let at = harness.get_by_label("Lane Audio 1").rect().center();
    double_click_at(&mut harness, at);
    let made = harness.state().session().and_then(|s| {
        s.project()
            .arrangement
            .lanes
            .iter()
            .find(|l| l.name == "Audio 1")
            .map(|l| l.clips.len().max(1) == l.clips.len() && !l.clips.is_empty())
    });
    assert!(made.unwrap_or(false), "the empty audio clip is in the lane");
    // The made clip is selected; the detail area shows it.
    assert!(harness.query_all_by_label_contains("Audio clip").next().is_some(), "it is selected");
}

#[test]
fn a_double_click_on_an_empty_spot_of_a_midi_lane_makes_a_bar_long_clip_on_the_grid_and_selects_it() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let lane = harness.get_by_label("Lane Keys").rect();
    let clips: Vec<egui::Rect> = harness.query_all_by_label_contains(" on Keys at bar ").map(|n| n.rect()).collect();
    // The rightmost spot of the lane that no clip covers, a little into a bar (8 points a beat).
    let at = (0..200)
        .map(|i| egui::pos2(lane.right() - 12.0 - i as f32 * 8.0, lane.center().y))
        .find(|p| lane.contains(*p) && !clips.iter().any(|c| c.expand(2.0).contains(*p)))
        .unwrap();
    let count = |h: &Harness<'static, App>| {
        h.state().session().map_or(0, |s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            p.arrangement.lanes.iter().filter(|l| l.track == keys).map(|l| l.clips.len()).sum::<usize>()
        })
    };
    let before = count(&harness);
    double_click_at(&mut harness, at);
    assert_eq!(count(&harness), before + 1);
    let session = harness.state().session().unwrap();
    let p = session.project();
    let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id).unwrap();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(keys)).unwrap();
    let made = lane.clips.iter().find(|c| p.content(c.content).is_some_and(|x| x.name == "MIDI clip")).unwrap();
    assert_eq!(made.length, 4.0, "one bar of 4/4");
    assert_eq!(made.start % 4.0, 0.0, "on the bar grid at this zoom: {}", made.start);
    assert_eq!(harness.state().selected_clip(), Some((keys, made.id)), "the new clip is selected");
}

#[test]
fn a_critical_command_from_the_window_is_backed_up_first() {
    let dir = crate::temp_dir::TempDir::new("ui-critical");
    let file = dir.join("Song.suhina");
    Session::new(demo_project()).save(&file, false).unwrap();
    let mut app = App::new();
    app.projects_mut().set_backup_folder(Some(dir.join("settings")));
    app.projects_mut().set_backup_settings(suhina_session::backup::Interval::Critical, true);
    app.set_session(Session::open(&file).unwrap().0, &[]);
    app.run(&Action::new("transport.setTempo", json!({ "bpm": 101 })));
    assert!(!dir.join(".backup").exists(), "an ordinary edit is not critical");
    let track = app.session().unwrap().project().tracks.first().unwrap().id.0;
    app.run(&Action::new("track.delete", json!({ "track": track })));
    let tracks = app.session().unwrap().project().tracks.len();
    // The backup thread finishes, and quitting is critical too: a second backup has the delete.
    assert!(app.finish_backups());
    let mut kept: Vec<usize> = std::fs::read_dir(dir.join(".backup"))
        .unwrap()
        .map(|e| Session::open(&e.unwrap().path()).unwrap().0.project().tracks.len())
        .collect();
    kept.sort();
    assert_eq!(kept, [tracks, tracks + 1]);
}

#[test]
fn escape_or_a_drop_on_nothing_leaves_the_project_alone() {
    let project = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().clone());
    let mut harness = window(demo_app());
    let before = project(&harness);
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(0.0, 12.0));
    harness.step();
    harness.key_press(Key::Escape);
    harness.run();
    harness.drop_at(from + egui::vec2(0.0, 12.0));
    harness.run();
    assert_eq!(project(&harness), before);
    // Dropped back on its own slot, or on a track header: nothing either.
    for to in [from, harness.get_by_label("Track Keys").rect().center()] {
        harness.hover_at(from);
        harness.step();
        harness.drag_at(from);
        harness.step();
        harness.hover_at(from + egui::vec2(0.0, 12.0));
        harness.step();
        harness.hover_at(to);
        harness.step();
        harness.drop_at(to);
        harness.run();
        assert_eq!(project(&harness), before);
    }
}

#[test]
fn an_empty_rename_field_clears_the_clips_name() {
    let mut harness = window(demo_app_with_name_field());
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.key_press(Key::Backspace);
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Launch clip on Bass (scene 2)").is_some(), "the empty draft clears the name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "the undo brings the name back");
}

#[test]
fn a_rename_draft_dies_with_its_menu() {
    let mut harness = window(demo_app_with_name_field());
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").click();
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").type_text("X");
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label_contains("Rename clip").is_none(), "the menu closed");
    // Reopening starts from the name as it is now, not the typing that was never applied.
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    let field = harness.get_by_label_contains("Rename clip Walk");
    assert_eq!(field.value().as_deref(), Some("Walk"), "the reopened field carries the name, not the stale draft");
}

#[test]
fn ctrl_r_on_an_arrangement_clip_edits_its_name_on_the_clip() {
    let mut harness = arrangement_app();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    let field = harness.get_by_label("Clip name").rect();
    assert!((field.top() - clip.top()).abs() < 4.0, "the field is on the clip: {field:?} on {clip:?}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Clip name").type_text("Fill");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Clip Fill on Drums at bar 17").is_some(), "renamed in place");
}

#[test]
fn dragging_one_of_several_selected_clips_moves_them_all() {
    let mut harness = arrangement_app();
    let starts = |h: &Harness<'static, App>| {
        h.state().session().map(|s| {
            s.project().arrangement.lanes.iter().flat_map(|l| l.clips.iter().map(|c| c.start)).collect::<Vec<_>>()
        })
    };
    let before = starts(&harness).unwrap();
    assert!(before.len() > 1);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    // Two bars later: 64 points at 8 points a beat.
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    let after = starts(&harness).unwrap();
    let mut want: Vec<f64> = before.iter().map(|s| s + 8.0).collect();
    let mut got = after.clone();
    want.sort_by(f64::total_cmp);
    got.sort_by(f64::total_cmp);
    assert_eq!(got, want, "every selected clip moved by two bars");
}

#[test]
fn dragging_a_midi_clips_right_edge_makes_it_longer_on_the_grid() {
    let mut harness = arrangement_app();
    let beat_b = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
        let lane = p.arrangement.lanes.iter().find(|l| l.track == track).unwrap();
        lane.clips.iter().find(|c| (c.start - 64.0).abs() < 1e-6).map(|c| c.length).unwrap()
    };
    let before = beat_b(&harness);
    let edge = harness.get_by_label("Right edge of Beat B on Drums").rect().center();
    // One bar: 32 points at 8 points a beat.
    let to = edge + egui::vec2(32.0, 0.0);
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.run();
    harness.hover_at(edge + egui::vec2(16.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(beat_b(&harness), before + 4.0, "a bar longer");
}

/// The clip is drawn at its new length from the frame the button comes up on. In the window the
/// trim runs after that frame is drawn, and the clip showed as it was before the drag for that
/// one frame; the test harness draws the frame again once the trim ran, so it can only check that
/// the clip ends where it was let go.
#[test]
fn a_released_clip_edge_stays_where_it_was_let_go() {
    let mut harness = arrangement_app();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let edge = harness.get_by_label("Right edge of Beat B on Drums").rect().center();
    let to = edge + egui::vec2(32.0, 0.0);
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.run();
    harness.hover_at(edge + egui::vec2(16.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    let body_ends = |h: &Harness<'static, App>| -> Vec<f32> {
        let mut out = Vec::new();
        for clipped in &h.output().shapes {
            let mut stack = vec![clipped.shape.clone()];
            while let Some(shape) = stack.pop() {
                match shape {
                    egui::Shape::Vec(inner) => stack.extend(inner),
                    egui::Shape::Rect(r)
                        if r.fill.a() > 0
                            && (r.rect.left() - clip.left()).abs() < 1.5
                            && (r.rect.top() - clip.top()).abs() < 1.5 =>
                    {
                        out.push(r.rect.right());
                    }
                    _ => {}
                }
            }
        }
        out
    };
    let dragged = body_ends(&harness).into_iter().fold(f32::MIN, f32::max);
    harness.drop_at(to);
    harness.step();
    let released = body_ends(&harness).into_iter().fold(f32::MIN, f32::max);
    assert!((released - dragged).abs() < 1.5, "drawn at {released} the frame it was let go, {dragged} while dragged");
    assert!(released > clip.right() + 16.0, "not back at its old end {}", clip.right());
}

#[test]
fn ctrl_l_loops_the_time_selection_even_with_clips_in_it_selected() {
    let mut harness = arrangement_app();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    // A selection rectangle from empty space before the clip into it picks the clip too.
    let ruler = harness.get_by_label("Ruler").rect();
    let (from, to) =
        (egui::pos2(clip.left() - 64.0, ruler.center().y), egui::pos2(clip.left() + 32.0, ruler.center().y));
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let span = harness.state().arrangement().time_selection().map(|s| (s.start, s.end)).unwrap();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::L);
    harness.run();
    let transport = harness.state().session().unwrap().project().transport.clone();
    assert!(transport.loop_enabled);
    assert_eq!(transport.loop_range.map(|r| (r.start, r.end)), Some(span), "the loop is the selection");
}

#[test]
fn touching_a_short_clips_edge_with_a_coarse_grid_changes_nothing() {
    let mut harness = arrangement_app();
    let length = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
        let lane = p.arrangement.lanes.iter().find(|l| l.track == track).unwrap();
        lane.clips.iter().find(|c| (c.start - 64.0).abs() < 1e-6).map(|c| c.length).unwrap()
    };
    let before = length(&harness);
    let edge = harness.get_by_label("Right edge of Beat B on Drums").rect().center();
    // A press and a let-go two points along: the edge stays where it was.
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.run();
    harness.hover_at(edge + egui::vec2(2.0, 0.0));
    harness.run();
    harness.drop_at(edge + egui::vec2(2.0, 0.0));
    harness.run();
    assert_eq!(length(&harness), before);
}

#[test]
fn ctrl_dragging_a_clip_copies_it_and_leaves_the_original() {
    let mut harness = arrangement_app();
    let starts = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
        p.arrangement.lanes.iter().find(|l| l.track == track).unwrap().clips.iter().map(|c| c.start).collect::<Vec<_>>()
    };
    let before = starts(&harness);
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    let to = from + egui::vec2(256.0, 0.0);
    harness.hover_at(from);
    harness.run();
    // Ctrl (Cmd) held through the drag.
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(128.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::default()));
    harness.run();
    let after = starts(&harness);
    // (The copy replaces what it lands on, as a moved clip does.)
    assert!(after.contains(&64.0), "the original stays at bar 17: {before:?} -> {after:?}");
    assert!(after.contains(&96.0), "the copy is 8 bars later");
}

#[test]
fn a_drag_on_the_ruler_selects_time_and_delete_removes_only_what_is_inside() {
    let mut harness = arrangement_app();
    let drums = |h: &Harness<'static, App>| {
        h.state().session().map(|s| {
            let p = s.project();
            let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
            p.arrangement
                .lanes
                .iter()
                .find(|l| l.track == track)
                .map(|l| l.clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
        })
    };
    let before = drums(&harness).flatten().unwrap();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let ruler = harness.get_by_label("Ruler").rect();
    // One bar into the clip to one bar before its end, on the bar grid (32 points a bar).
    let (from, to) =
        (egui::pos2(clip.left() + 33.0, ruler.center().y), egui::pos2(clip.right() - 33.0, ruler.center().y));
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert!(harness.state().arrangement().time_selection().is_some(), "the ruler's drag selected time");
    harness.key_press(Key::Delete);
    harness.run();
    let after = drums(&harness).flatten().unwrap();
    assert_eq!(after.len(), before.len() + 1, "the clip split around the gap: {before:?} -> {after:?}");
    let total = |c: &[(f64, f64)]| c.iter().map(|(_, l)| l).sum::<f64>();
    assert!(total(&after) < total(&before), "the inside went");
}

#[test]
fn ctrl_r_after_a_header_click_renames_the_track_in_its_header() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Bass").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    assert!(harness.query_by_label("Track name").is_some(), "Ctrl+R edits the track's name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Track name").type_text("Sub");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let names: Vec<String> =
        harness.state().session().unwrap().project().tracks.iter().map(|t| t.name.clone()).collect();
    assert!(names.contains(&"Sub".to_string()) && !names.contains(&"Bass".to_string()), "{names:?}");
}

#[test]
fn ctrl_r_with_several_clips_selected_renames_them_all() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    let count = harness.state().arrangement().selected_clips().len();
    assert!(count > 1, "Ctrl+A selects several");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Clip name").type_text("Same");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let project = harness.state().session().unwrap().project().clone();
    let named = project
        .arrangement
        .lanes
        .iter()
        .flat_map(|l| &l.clips)
        .filter(|c| project.contents.iter().any(|x| x.id == c.content && x.name == "Same"))
        .count();
    assert_eq!(named, count);
}
