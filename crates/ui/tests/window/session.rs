// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Session view: slots, scenes, launching, stop buttons, slot menus and drags between the views.

use super::*;

/// With more tracks than the view shows, the buttons that add a track stay whole at the end of
/// the sideways scroll instead of shrinking to nothing (reported on Discord).
#[test]
fn the_add_track_buttons_keep_their_width_with_many_tracks() {
    let mut session = Session::new(demo_project());
    for _ in 0..40 {
        session.execute("track.add", &json!({ "kind": "midi" })).unwrap();
    }
    let many = SessionHarness {
        grid: TrackGrid::build(session.project(), None),
        state: SessionViewState::default(),
        actions: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut SessionHarness| session_view::show(ui, &v.grid, &[], &mut v.state, &mut v.actions),
        many,
    );
    theme::install(&harness.ctx);
    harness.run();
    // Scrolled as far right as it goes.
    harness.state_mut().state.scroll.x = f32::MAX;
    harness.run();
    // As wide as with a few tracks.
    let few = view();
    for label in ["+ MIDI track", "+ Audio track", "+ Return track"] {
        let (width, wanted) = (harness.get_by_label(label).rect().width(), few.get_by_label(label).rect().width());
        assert!(width >= wanted - 0.5, "{label} is {width} px wide, not {wanted}");
    }
    assert_eq!(clicked(&mut harness, "+ MIDI track")[0].id, "track.add");
}

#[test]
fn clicking_a_clip_launches_it() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Launch Walk on Bass (scene 2)"),
        [Action::new("clip.launch", json!({ "track": 2, "scene": 2 }))]
    );
}

#[test]
fn clicking_an_empty_slot_with_a_stop_button_stops_the_track() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Stop Keys (scene 1)"),
        [Action::new("clip.launch", json!({ "track": 3, "scene": 1 }))]
    );
}

#[test]
fn scenes_and_stop_buttons_send_their_commands() {
    let mut harness = view();
    assert_eq!(clicked(&mut harness, "Launch scene 3 Build"), [Action::new("scene.launch", json!({ "scene": 3 }))]);
    // A playing row's click stops its clips.
    harness.state_mut().grid.scenes[2].playing = true;
    harness.run();
    assert_eq!(clicked(&mut harness, "Stop scene 3 Build"), [Action::new("scene.stop", json!({ "scene": 3 }))]);
    harness.state_mut().grid.scenes[2].playing = false;
    harness.run();
    assert_eq!(clicked(&mut harness, "Stop Drums"), [Action::new("track.stop", json!({ "track": 1 }))]);
    assert_eq!(clicked(&mut harness, "Stop all clips"), [Action::new("launcher.stopAll", json!({}))]);
    assert_eq!(
        clicked(&mut harness, "Turn Vox off"),
        [Action::new("track.setMute", json!({ "tracks": [4], "mute": true }))]
    );
}

#[test]
fn a_scene_menu_inserts_duplicates_and_deletes_scenes() {
    let mut harness = view();
    for (entry, action) in [
        ("Insert scene below", Action::new("scene.insert", json!({ "scene": 4 }))),
        ("Duplicate scene", Action::new("scene.duplicate", json!({ "scene": 3 }))),
        ("Delete scene", Action::new("scene.delete", json!({ "scene": 3 }))),
    ] {
        harness.state_mut().actions.clear();
        harness.get_by_label("Launch scene 3 Build").click_secondary();
        harness.run();
        harness.get_by_label(entry).click();
        harness.run();
        assert_eq!(harness.state().actions, [action], "{entry}");
    }
}

#[test]
fn a_folded_group_hides_its_columns_in_the_session_view() {
    // Drums (track 1) inside the group.
    let mut project = project_with_a_group();
    project.tracks[0].parent = Some(suhina_engine::model::TrackId(20));
    let group = project.tracks.pop().unwrap();
    project.tracks.insert(0, group);
    let view =
        SessionHarness { grid: TrackGrid::build(&project, None), state: SessionViewState::default(), actions: vec![] };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut SessionHarness| session_view::show(ui, &v.grid, &[], &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some());
    harness.get_by_label("Fold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_none());
    assert!(harness.query_by_label("Track Bass").is_some());
    harness.get_by_label("Unfold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some());
    assert!(harness.state().actions.is_empty());
}

#[test]
fn dragging_a_session_column_header_moves_the_track() {
    let mut harness = window(demo_app());
    let from = harness.get_by_label("Track Vox").rect().center();
    // Three columns left (117 points each), into the left half of Drums.
    let to = from - egui::vec2(3.0 * 117.0 + 30.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(60.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Vox", "Drums", "Bass", "Keys"]);
}

/// The name of each clip's content in Keys' slots, `None` for an empty slot.
fn keys_slots(harness: &Harness<'static, App>) -> Vec<Option<String>> {
    let Some(session) = harness.state().session() else { return Vec::new() };
    let p = session.project();
    let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
    let column = p.launcher.columns.iter().find(|c| Some(c.track) == keys);
    column
        .map(|c| {
            c.slots
                .iter()
                .map(|s| s.clip.as_ref().and_then(|clip| p.content(clip.content)).map(|c| c.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_double_click_on_an_empty_slot_of_a_midi_track_makes_an_empty_clip_there() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    assert_eq!(keys_slots(&harness).first(), Some(&None));
    let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    assert_eq!(keys_slots(&harness).first(), Some(&Some("MIDI clip".to_string())));
    assert!(harness.query_by_label("Launch MIDI clip on Keys (scene 1)").is_some());
    // One undo step takes it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(keys_slots(&harness).first(), Some(&None));
    // A double-click on a slot with a clip launches it and makes nothing.
    let before = harness.state().session().map(|s| s.project().clone());
    let at = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    double_click_at(&mut harness, at);
    assert_eq!(harness.state().session().map(|s| s.project().clone()), before);
}

#[test]
fn a_double_click_on_an_empty_slot_of_an_audio_track_makes_an_empty_clip_there() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    // An audio track next to the demo's MIDI ones.
    harness.get_by_label("+ Audio track").click();
    harness.run();
    assert!(harness.query_by_label("Stop Audio 1 (scene 1)").is_some(), "the track's empty slot is there");
    let at = harness.get_by_label("Stop Audio 1 (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    assert!(
        harness.query_by_label("Launch Audio clip on Audio 1 (scene 1)").is_some(),
        "the empty audio clip is there"
    );
    // One undo step takes it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Stop Audio 1 (scene 1)").is_some(), "the slot is empty again");
}

#[test]
fn the_delete_key_takes_a_selected_slot_clip_off_its_slot() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    // A double-click makes the empty clip in Keys' slot; its name area selects the clip.
    let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    harness.get_by_label("Select MIDI clip on Keys (scene 1)").click();
    harness.run();
    // Delete takes the selected clip off its slot: the empty slot's stop control is back.
    harness.key_press(Key::Delete);
    harness.run();
    assert!(harness.query_by_label("Stop Keys (scene 1)").is_some(), "the slot is empty again");
    assert!(harness.query_by_label("Launch MIDI clip on Keys (scene 1)").is_none(), "the clip is gone");
    // One undo puts it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch MIDI clip on Keys (scene 1)").is_some(), "the clip is back");
}

#[test]
fn a_clips_name_selects_it_and_its_mark_launches_it() {
    let mut harness = view();
    let walk = (TrackId(2), 1);
    assert!(clicked(&mut harness, "Select Walk on Bass (scene 2)").is_empty(), "selecting does not launch");
    assert_eq!(harness.state().state.selected_slot, Some(walk));
    assert_eq!(harness.state().state.selected, Some(TrackId(2)), "its track is selected with it");

    // A launch selects the clip too, unless Select on launch is off.
    assert_eq!(
        clicked(&mut harness, "Launch Chords 1 on Keys (scene 2)"),
        [Action::new("clip.launch", json!({ "track": 3, "scene": 2 }))]
    );
    assert_eq!(harness.state().state.selected_slot, Some((TrackId(3), 1)));
    harness.state_mut().state.keep_selection_on_launch = true;
    clicked(&mut harness, "Launch Walk on Bass (scene 2)");
    assert_eq!(harness.state().state.selected_slot, Some((TrackId(3), 1)));

    // A track's header selects the track and ends the clip's selection.
    clicked(&mut harness, "Track Drums");
    assert_eq!((harness.state().state.selected_slot, harness.state().state.selected), (None, Some(TrackId(1))));
}

#[test]
fn the_arrows_step_through_the_slots_of_tracks_and_stop_at_the_edges() {
    use suhina_ui::session::session_view::{Step, step_slot};
    let grid = TrackGrid::build(&demo_project(), None);
    let (drums, bass, vox) = (TrackId(1), TrackId(2), TrackId(4));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Up), Some((bass, 0)));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Down), Some((bass, 2)));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Left), Some((drums, 1)));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Right), Some((TrackId(3), 1)));
    assert_eq!(step_slot(&grid, (bass, 0), Step::Up), None);
    assert_eq!(step_slot(&grid, (bass, 5), Step::Down), None);
    assert_eq!(step_slot(&grid, (drums, 2), Step::Left), None);
    // Return tracks have no slots, so the last track is the edge.
    assert_eq!(step_slot(&grid, (vox, 2), Step::Right), None);
    assert_eq!(step_slot(&grid, (TrackId(10), 0), Step::Down), None, "a return track is not in the grid of slots");
}

#[test]
fn a_slots_menu_copies_its_clip_and_pastes_once_something_is_copied() {
    let mut harness = view();
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    assert_eq!(harness.state().state.selected_slot, Some((suhina_engine::model::TrackId(2), 1)));
    harness.state_mut().actions.clear();
    harness.get_by_label_contains("Copy").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("clip.copy", json!({ "track": 2, "scene": 2 }))]);

    // Nothing copied: Paste is there but does nothing.
    harness.get_by_label("Stop Keys (scene 1)").click_secondary();
    harness.run();
    harness.state_mut().actions.clear();
    harness.get_by_label_contains("Paste").click();
    harness.run();
    assert!(harness.state().actions.is_empty());
    harness.key_press(Key::Escape);
    harness.run();

    harness.state_mut().state.can_paste = true;
    harness.run();
    harness.get_by_label("Stop Keys (scene 1)").click_secondary();
    harness.run();
    harness.state_mut().actions.clear();
    harness.get_by_label_contains("Paste").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("clip.paste", json!({ "track": 3, "scene": 1 }))]);
}

#[test]
fn a_playing_trigger_clips_click_stops_it() {
    let mut harness = view();
    // Walk's slot in scene 2, playing: its mark is the stop square, and the click stops the
    // track (a trigger clip listens for no release, so the press stops instead of relaunching).
    harness.state_mut().grid.columns[1].slots[1].state = SlotState::Playing;
    harness.run();
    assert_eq!(
        clicked(&mut harness, "Stop Walk on Bass (scene 2)"),
        [Action::new("track.stop", json!({ "track": 2 }))]
    );
}

#[test]
fn clicking_a_slot_selects_it_and_still_launches_or_stops() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Stop Keys (scene 1)"),
        [Action::new("clip.launch", json!({ "track": 3, "scene": 1 }))]
    );
    assert_eq!(harness.state().state.selected_slot, Some((suhina_engine::model::TrackId(3), 0)));
    clicked(&mut harness, "Launch Walk on Bass (scene 2)");
    assert_eq!(harness.state().state.selected_slot, Some((suhina_engine::model::TrackId(2), 1)));
}

/// What Drums' slot in `scene` (from 0) holds: its clip's content name.
fn drums_slot(harness: &Harness<'static, App>, scene: usize) -> Option<String> {
    let session = harness.state().session()?;
    let p = session.project();
    let clip = p.launcher.columns[0].slots[scene].clip.as_ref()?;
    p.content(clip.content).map(|c| c.name.clone())
}

#[test]
fn an_arrangement_clip_copied_with_ctrl_c_pastes_into_the_selected_slot() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.key_press(Key::Tab);
    harness.run();
    assert_eq!(harness.state().view(), View::Session);
    // Ctrl+V with no slot picked says what to do.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert!(matches!(harness.state().status(), Status::Hint(_)));
    harness.get_by_label("Stop Drums (scene 5)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert_eq!(drums_slot(&harness, 4).as_deref(), Some("Fill"));
    let arranged = harness.state().session().map_or(0, |s| s.project().arrangement.lanes[0].clips.len());
    assert_eq!(arranged, 4, "the arrangement keeps its clips");
    // One undo step empties the slot again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(drums_slot(&harness, 4), None);

    // A slot's clip copies with Ctrl+C and pastes into another slot.
    harness.get_by_label("Launch Fill on Drums (scene 4)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.get_by_label("Stop Drums (scene 5)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert_eq!(drums_slot(&harness, 4).as_deref(), Some("Fill"));
}

/// Holds the pointer still for a second, a frame at a time, so a hover that waits fires.
fn wait_over(harness: &mut Harness<'static, App>, at: egui::Pos2) {
    for _ in 0..4 {
        harness.hover_at(at);
        harness.step();
    }
}

#[test]
fn an_arrangement_clip_dragged_over_the_session_switch_drops_into_a_slot() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Fill on Drums at bar 29").rect().center_top() + egui::vec2(0.0, 6.0);
    let switch = harness.get_by_label("Session view").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(10.0, 0.0));
    harness.step();
    wait_over(&mut harness, switch);
    assert_eq!(harness.state().view(), View::Session);
    let slot = harness.get_by_label("Stop Drums (scene 5)").rect().center();
    harness.hover_at(slot);
    harness.step();
    harness.drop_at(slot);
    harness.run();
    assert_eq!(drums_slot(&harness, 4).as_deref(), Some("Fill"));
    let starts: Vec<f64> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect())
        .unwrap_or_default();
    assert_eq!(starts, [0.0, 64.0, 112.0, 128.0], "the arrangement clip stays where it was");
}

#[test]
fn a_slot_clip_dragged_over_the_arrangement_switch_drops_on_a_lane() {
    let mut harness = window(demo_app());
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let switch = harness.get_by_label("Arrangement view").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(0.0, 12.0));
    harness.step();
    wait_over(&mut harness, switch);
    assert_eq!(harness.state().view(), View::Arrangement);
    // Bars 1 to 8 of Bass are empty; Root starts at bar 9, 8 points a beat.
    let root = harness.get_by_label("Clip Root on Bass at bar 9").rect();
    let to = egui::pos2(root.left() - 16.0 * 8.0, root.center().y);
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
    let bass: Vec<(f64, f64)> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[1].clips.iter().map(|c| (c.start, c.length)).collect())
        .unwrap_or_default();
    assert!(bass.contains(&(16.0, 4.0)), "{bass:?}");
    assert!(matches!(harness.state().status(), Status::Done(text) if text.contains("Copied Walk to Bass")));
}

/// Needs an audio device, so it is ignored by default, like the preview test.
#[test]
#[ignore]
fn launching_a_clip_greys_the_arrangement_until_back_to_arrangement() {
    let mut session = Session::new(demo_project());
    let (player, _) = suhina_output::open_player(session.project(), session.audio()).unwrap();
    session.attach_player(player);
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    // Launch a clip while the transport plays, on a fine grid so it fires at once.
    harness.state_mut().projects_mut().execute("transport.play", &json!({})).unwrap();
    harness.state_mut().projects_mut().execute("launcher.setQuantization", &json!({ "value": "1/32" })).unwrap();
    harness.state_mut().projects_mut().execute("clip.launch", &json!({ "track": 1, "scene": 1 })).unwrap();
    for _ in 0..30 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    eprintln!(
        "LAUNCHER STATE: {:?}",
        harness.state_mut().projects_mut().execute("launcher.state", &json!({})).unwrap()
    );
    // The Arrangement view: the Drums lane dims and Back to arrangement shows in the ruler.
    harness.get_by_label("Arrangement view").click();
    harness.run_steps(5);
    assert!(
        harness.query_by_label("Back to arrangement").is_some(),
        "the button shows while a track plays launched clips"
    );
    // Clicking it returns every track to its arrangement clips: the launched clip stops at
    // the next boundary (a 32nd here) with its fade, so wait for it.
    harness.get_by_label("Back to arrangement").click();
    let mut back = true;
    for _ in 0..200 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        if harness.query_by_label("Back to arrangement").is_none() {
            back = false;
            break;
        }
    }
    assert!(!back, "the arrangement is back");
}

#[test]
fn a_slot_drag_to_another_slot_moves_it_ctrl_drags_copy_and_an_occupied_one_swaps() {
    let slots = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| {
                let p = s.project();
                let of = |track: u32, scene: usize| {
                    p.launcher
                        .columns
                        .iter()
                        .find(|c| c.track == TrackId(track))
                        .and_then(|c| c.slots.get(scene))
                        .and_then(|s| s.clip.as_ref())
                        .map(|c| c.content)
                };
                (of(2, 1), of(2, 3), of(1, 0))
            })
            .unwrap_or((None, None, None))
    };
    let mut harness = window(demo_app());
    // Walk (Bass, scene 2) dragged to Bass' empty scene 4: it moves, the source empties.
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let to = harness.get_by_label("Stop Bass (scene 4)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
    let (first, target, _) = slots(&harness);
    assert!(first.is_none(), "the source slot emptied");
    assert!(target.is_some(), "the clip is in scene 4 now");
    // The grid rebuilt: the moved clip's slot shows it, the emptied one is a stop control.
    assert!(harness.query_by_label("Launch Walk on Bass (scene 4)").is_some(), "the grid shows the moved clip");
    assert!(harness.query_by_label("Stop Bass (scene 2)").is_some(), "the source's slot is empty again");
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Moved")),
        "{:?}",
        harness.state().status()
    );
    // One undo moves it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    let (first, target, walk) = slots(&harness);
    assert!(first.is_some() && target.is_none(), "the clip is back");
    let _ = walk;
    // Ctrl+drag copies: both slots hold a clip (the copies' contents are their own). The
    // modifier is held over the drop: its state is set in the input directly (kittest's own
    // key presses reset the modifiers right after), and egui's state keeps it until changed.
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let to = harness.get_by_label("Stop Bass (scene 4)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.step();
    harness.drop_at(to);
    // The drop's frame runs with the modifier still held; the reset comes after it.
    harness.run();
    harness.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::default()));
    harness.run();
    let (_, target, _) = slots(&harness);
    assert!(target.is_some(), "the copy landed");
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "the source kept its clip");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    // A drag onto an occupied slot swaps the two: Walk to Beat's slot, Beat to Walk's.
    let (walk_content, _, beat_content) = slots(&harness);
    let walk = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let beat = harness.get_by_label("Launch Beat A on Drums (scene 1)").rect().center();
    harness.hover_at(walk);
    harness.step();
    harness.drag_at(walk);
    harness.step();
    harness.hover_at(beat);
    harness.step();
    harness.drop_at(beat);
    harness.run();
    let (bass, _, drums) = slots(&harness);
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Swapped")),
        "{:?}",
        harness.state().status()
    );
    assert_eq!((bass, drums), (beat_content, walk_content), "Beat A is in Walk's slot and Walk in Beat A's");
}

#[test]
fn a_slots_context_menu_renames_its_clip() {
    let mut harness = window(demo_app_with_name_field());
    // Walk's slot in Bass, scene 2: a right-click opens the menu with the clip's name field
    // (the menu stays open on a click inside: CloseOnClickOutside).
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("Rename clip Walk").is_some(), "the menu carries the clip's name field");
    harness.get_by_label_contains("Rename clip Walk").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").type_text("Walk the bass");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    // The slot's label carries the new name; one undo brings Walk back.
    assert!(harness.query_by_label("Launch Walk the bass on Bass (scene 2)").is_some(), "the slot shows the new name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "the undo brings the name back");
}

#[test]
fn rename_in_a_slots_menu_edits_the_name_on_the_slot_and_ctrl_r_does_too() {
    let mut harness = window(demo_app());
    // By default the menu carries Rename, not a name field.
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("Rename clip Walk").is_none(), "no field in the menu by default");
    // The item's label carries its key too ("Rename" and Ctrl+R).
    harness.get_by_label_contains("Rename").click();
    harness.run();
    assert!(harness.query_by_label("Clip name").is_some(), "Rename puts a name field on the slot");
    assert!(harness.query_by_label("Cancel").is_none(), "no dialog: the field is all there is");
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Clip name").is_none(), "Escape leaves the name as it was");
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some());
    // Ctrl+R on the selected slot does the same; Enter renames.
    harness.get_by_label("Launch Walk on Bass (scene 2)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    assert!(harness.query_by_label("Clip name").is_some(), "Ctrl+R edits the selected clip's name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Clip name").type_text("Riff");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Launch Riff on Bass (scene 2)").is_some(), "the selected slot's clip is renamed");
}

/// A slot's right-click menu deletes its clip, MIDI or audio, as the Delete key does, in one
/// undo step; an empty slot's Delete is greyed out (#842).
#[test]
fn a_slots_context_menu_deletes_its_clip() {
    // A MIDI clip: Walk, in Bass's second slot.
    let mut harness = window(demo_app());
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Delete").click();
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_none(), "the clip is gone");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "one undo brings it back");

    // An empty slot: nothing to delete.
    harness.get_by_label("Stop Keys (scene 1)").click_secondary();
    harness.run();
    assert!(egui_kittest::kittest::NodeT::accesskit_node(&harness.get_by_label_contains("Delete")).is_disabled());
    harness.key_press(Key::Escape);
    harness.run();

    // An audio clip: the Loop track's, put in its first slot.
    let (mut project, waveforms) = project_with_audio();
    let clip = project.arrangement.lanes.last().and_then(|l| l.clips.first()).cloned().unwrap();
    project.launcher.columns.push(suhina_engine::model::LauncherColumn {
        track: suhina_engine::model::TrackId(30),
        slots: vec![suhina_engine::model::Slot { clip: Some(clip), has_stop: true }],
    });
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    app.set_waveforms(waveforms);
    let mut harness = window(app);
    harness.get_by_label("Launch Loop on Loop (scene 1)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Delete").click();
    harness.run();
    assert!(harness.query_by_label("Launch Loop on Loop (scene 1)").is_none(), "the audio clip is gone");
}
