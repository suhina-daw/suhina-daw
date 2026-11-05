// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Project tabs: switching, keeping each project's edits, new, open and close, and asking before losing changes.

use super::*;

#[test]
fn ctrl_n_asks_for_a_new_project_and_ctrl_s_saves_one() {
    let dir = crate::temp_dir::TempDir::new("ui-save");
    let mut app = demo_app();
    app.set_default_folders(dir.join("Projects"), dir.join("Templates"));
    let mut harness = window(app);

    // Ctrl+N: the dialog, which asks for its lists; a blank project is handed out to be made.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    assert!(harness.state().project_dialog_open());
    assert_eq!(harness.state_mut().take_lists_wanted(), Some(dir.join("Templates")));
    harness.get_by_label("New blank project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank { returns: true }));
    assert!(!harness.state().project_dialog_open());

    // Ctrl+S on a project without a .suhina file asks for a name, in the projects folder.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run();
    harness.get_by_label("Name").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Name").type_text("Night drive");
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    let saved = dir.join("Projects").join("Night drive.suhina");
    assert!(saved.is_file(), "{:?}", harness.state().status());
    assert_eq!(harness.state_mut().take_saved_path(), Some(saved.clone()));
    let session = harness.state().session();
    assert_eq!(session.and_then(|s| s.source()), Some(saved.as_path()));

    // Saved again without asking; a template is a copy in the templates folder.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run();
    assert!(harness.query_by_label("Name").is_none(), "asked again");
    harness.state_mut().do_action("file.saveTemplate", &mut Vec::new());
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    assert!(dir.join("Templates").join("Night drive.suhina").is_file());
    assert_eq!(harness.state().session().and_then(|s| s.source()), Some(saved.as_path()));
}

#[test]
fn an_edit_asks_for_a_frame_so_the_views_show_it_at_once() {
    let mut harness = arrangement_app();
    harness.run();
    assert!(!harness.ctx.has_requested_repaint(), "nothing to draw");
    // A click on a header runs its command after the views drew: the window asks for one more
    // frame, so the new state is on screen without a second click or a pointer move.
    harness.get_by_label("Turn Bass off").click();
    harness.step();
    assert!(harness.ctx.has_requested_repaint(), "an edit asks for a frame");
    harness.run();
    assert!(harness.query_by_label("Turn Bass on").is_some(), "and the frame shows it");
}

fn two_projects() -> Harness<'static, App> {
    let mut app = App::new();
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    window(app)
}

#[test]
fn ctrl_tab_shows_the_open_projects_and_switches_when_ctrl_is_let_go() {
    let mut app = App::new();
    for _ in 0..3 {
        app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    }
    let mut harness = window(app);
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.get(2).copied());
    // Ctrl held, Tab tapped twice: the list shows, the pick moves on, nothing switches yet.
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.step();
    tab_with_ctrl_held(&mut harness);
    assert!(harness.query_by_label("Switch to Untitled 1").is_some(), "the open projects are listed");
    tab_with_ctrl_held(&mut harness);
    assert_eq!(harness.state().projects().active(), ids.get(2).copied(), "not while Ctrl is down");
    // Ctrl let go: to the second one along (wrapping from the third: the first, then the second).
    harness.event(egui::Event::ModifiersChanged(Modifiers::default()));
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());
    assert!(harness.query_by_label("Switch to Untitled 1").is_none(), "the list is gone");
}

/// Tab pressed and let go while Ctrl (Cmd) stays down.
fn tab_with_ctrl_held(harness: &mut Harness<'static, App>) {
    for pressed in [true, false] {
        harness.event(egui::Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
        harness.step();
    }
    harness.step();
    harness.step();
}

#[test]
fn tabs_switch_keep_each_projects_edits_and_close() {
    let mut harness = two_projects();
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());

    // Turn a track off in the second project, then show the first: it is untouched.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    harness.get_by_label("Tab Untitled 1").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.first().copied());
    assert_eq!(harness.state_mut().take_player_request(), ids.first().copied());
    assert!(harness.query_by_label("Turn Bass off").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Tab);
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());
    assert!(harness.query_by_label("Turn Bass on").is_some());

    // It has an edit, so closing it asks first.
    harness.get_by_label("Close Untitled 2").click();
    harness.run();
    harness.get_by_label("Don't save the changes").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().len(), 1);
    assert!(harness.query_by_label("Tab Untitled 1").is_none(), "one project shows no tabs");
}

/// A carry names a slot in the project it was made in: switching projects while it is held lets
/// it go, so its release never copies the last project's clip into the new one.
#[test]
fn switching_project_while_a_clip_is_carried_lets_it_go() {
    let mut harness = two_projects();
    // The active one is the second. Press a slot's clip and Tab: the carry is live.
    let from = harness.get_by_label("Select Walk on Bass (scene 2)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.key_press(Key::Tab);
    harness.step();
    assert_eq!(harness.state().view(), View::Arrangement);
    // Ctrl+Tab switches to the first project while the press is still held. The view is the
    // one shown, so the release probes the lanes of the project on screen.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Tab);
    harness.run();
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.first().copied(), "the first project is active");
    assert_eq!(harness.state().view(), View::Arrangement);
    let bass_before: Vec<(f64, f64)> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[1].clips.iter().map(|c| (c.start, c.length)).collect())
        .unwrap_or_default();
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
    assert_eq!(bass, bass_before, "the switch let the carry go: nothing is copied");
    assert!(!matches!(harness.state().status(), Status::Error(_)), "{:?}", harness.state().status());
}

/// An arrangement clip's drag held through a project switch lets go with the switch: its view
/// state, brought back with the project later, carries nothing into the Launcher view
/// (CodeRabbit review).
#[test]
fn switching_project_during_an_arrangement_clip_drag_cancels_it() {
    let mut harness = two_projects();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(10.0, 0.0));
    harness.step();
    // Ctrl+Tab to the first project with the drag held, and the button let go there.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Tab);
    harness.run();
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.first().copied(), "the first project is active");
    harness.drop_at(from + egui::vec2(10.0, 0.0));
    harness.run();
    harness.get_by_label("Launcher view").click();
    harness.run();
    // Back to the second project with the pointer over an empty slot, in the Launcher view.
    let slot = harness.get_by_label("Stop Drums (scene 5)").rect().center();
    harness.hover_at(slot);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Tab);
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied(), "the second project is active");
    let drums_slot = |h: &Harness<'static, App>| {
        h.state().session().and_then(|s| {
            let p = s.project();
            let clip = p.launcher.columns.first()?.slots.get(4)?.clip.as_ref()?;
            p.content(clip.content).map(|c| c.name.clone())
        })
    };
    assert_eq!(drums_slot(&harness), None, "the old drag carried nothing");
}

/// A device drag that starts on the frame a project switch is taken up names a device of the
/// project being left: it lets go with the switch, and moves nothing in the new project
/// (CodeRabbit review).
#[test]
fn a_device_drag_started_on_a_project_switch_lets_it_go() {
    // Two projects with a device on Drums.
    let mut app = App::new();
    app.open_project(Session::new(project_with_a_device()), &[], OpenAs::NewTab);
    app.open_project(Session::new(project_with_a_device()), &[], OpenAs::NewTab);
    let mut harness = window(app);
    let chains = |h: &Harness<'static, App>| -> Vec<Vec<String>> {
        let Some(s) = h.state().session() else { return Vec::new() };
        let p = s.project();
        p.tracks
            .iter()
            .filter_map(|t| p.channel(t.channel?))
            .map(|c| c.devices.iter().map(|d| d.name.clone()).collect())
            .collect()
    };
    let (drums, index) = {
        let p = harness.state().session().map(|s| s.project().clone()).unwrap_or_default();
        let track = p.tracks.iter().find(|t| t.name == "Drums").expect("Drums");
        let channel = track.channel.and_then(|c| p.channel(c));
        (track.id, channel.map_or(0, |c| c.devices.len().saturating_sub(1)))
    };
    let to = harness.get_by_label("Track Bass").rect().center();
    harness.hover_at(to);
    harness.run();
    let ids = harness.state().projects().ids();
    let first = ids.first().copied().expect("two projects");
    let before = {
        let p = harness.state().projects().get(first).map(|s| s.project().clone()).unwrap_or_default();
        p.tracks
            .iter()
            .filter_map(|t| p.channel(t.channel?))
            .map(|c| c.devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    // The drag's start and the switch reach the window on the same frame.
    let ctx = harness.ctx.clone();
    suhina_ui::interaction::drop::start_device_carry(&ctx, drums, index);
    let _ = harness.state_mut().projects_mut().switch(first);
    harness.step();
    harness.run();
    assert_eq!(harness.state().projects().active(), Some(first));
    assert_eq!(chains(&harness), before, "the drag of the last project's device moved nothing here");
}

#[test]
fn opening_while_a_project_is_open_asks_and_can_remember() {
    let mut harness = window(demo_app());
    harness.state_mut().request_open("other.als".into());
    harness.run();
    assert!(harness.state_mut().take_open_request().is_none());
    harness.get_by_label("Remember").click();
    harness.run();
    harness.get_by_label("In a new tab").click();
    harness.run();
    let request = harness.state_mut().take_open_request();
    assert_eq!(request, Some(("other.als".into(), OpenAs::NewTab)));
    assert_eq!(harness.state().settings().on_open, suhina_ui::settings::OnOpen::NewTab);

    // Remembered: the next one opens in a new tab without asking.
    harness.state_mut().request_open("third.als".into());
    harness.run();
    assert_eq!(harness.state_mut().take_open_request(), Some(("third.als".into(), OpenAs::NewTab)));
}

/// The label of the active project's Close button in the tab row.
fn active_close_label(harness: &Harness<'static, App>) -> String {
    let projects = harness.state().projects();
    let name = projects.active().and_then(|id| projects.name(id)).unwrap_or_default();
    format!("Close {name}")
}

/// Whether the window asked to close itself in its last frame.
fn asked_to_close(harness: &Harness<'static, App>) -> bool {
    harness.output().viewport_output.values().any(|v| v.commands.contains(&egui::ViewportCommand::Close))
}

#[test]
fn closing_or_quitting_with_unsaved_changes_asks_first() {
    let dir = crate::temp_dir::TempDir::new("ui-unsaved");
    let mut app = demo_app();
    app.set_default_folders(dir.join("Projects"), dir.join("Templates"));
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    let mut harness = window(app);
    let open = |h: &Harness<'static, App>| h.state().projects().ids().len();
    let unsaved = |h: &Harness<'static, App>| h.state().session().is_some_and(Session::has_unsaved_changes);
    assert_eq!(open(&harness), 2);

    // An edit in the second project; closing its tab asks first, and Cancel keeps it open.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    assert!(unsaved(&harness));
    let label = active_close_label(&harness);
    if let Some(close) = harness.get_all_by_label(&label).last() {
        close.click();
    }
    harness.run();
    assert!(harness.query_by_label("Don't save the changes").is_some());
    harness.get_by_label("Cancel and keep the project open").click();
    harness.run();
    assert_eq!(open(&harness), 2);
    assert!(harness.query_by_label("Don't save the changes").is_none());

    // Don't save closes it.
    let label = active_close_label(&harness);
    if let Some(close) = harness.get_all_by_label(&label).last() {
        close.click();
    }
    harness.run();
    harness.get_by_label("Don't save the changes").click();
    harness.run();
    assert_eq!(open(&harness), 1);

    // A project without changes closes without asking; quitting with one that has them asks,
    // and Save first asks for a name, saves, then lets the window close.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    harness.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    harness.run();
    assert!(!asked_to_close(&harness));
    assert!(harness.state().unsaved_prompt_open());
    harness.get_by_label("Save the changes").click();
    harness.run();
    harness.get_by_label("Name").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Name").type_text("Before quitting");
    harness.run();
    harness.get_by_label("Save").click();
    harness.step();
    harness.step();
    let saved = dir.join("Projects").join("Before quitting.suhina");
    assert!(saved.is_file(), "{:?}", harness.state().status());
    assert!(!unsaved(&harness));
    assert!(!harness.state().unsaved_prompt_open());
    // The window lets go of the projects off its own thread, then closes.
    let mut closed = false;
    for _ in 0..300 {
        if asked_to_close(&harness) {
            closed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    assert!(closed, "the window closes once its projects are let go of");
    assert!(harness.state().projects().ids().is_empty());
}

#[test]
fn each_project_tab_keeps_its_own_launch_quantisation() {
    let mut harness = two_projects();
    // The browser has tests of its own; the views get the whole width.
    harness.state_mut().set_browser_open(false);
    harness.run();
    let ids = harness.state().projects().ids();
    // The second project shows: 1/16 there.
    choose_grid(&mut harness, "1/16");
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));
    // The first keeps 1 bar.
    harness.get_by_label("Tab Untitled 1").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.first().copied());
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Bars(1)));
    // Back in the second, its 1/16 is still chosen.
    harness.get_by_label("Tab Untitled 2").click();
    harness.run();
    harness.run();
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));
}
