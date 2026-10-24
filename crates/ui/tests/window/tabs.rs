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
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank));
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
fn each_project_tab_keeps_its_own_grid() {
    let mut harness = two_projects();
    // The browser has tests of its own; the views get the whole width.
    harness.state_mut().set_browser_open(false);
    harness.run();
    let ids = harness.state().projects().ids();
    // The second project shows: 1/16 there.
    choose_grid(&mut harness, "1/16");
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));

    // The first keeps 1 bar: in its Arrangement a short drag at a fine zoom lands on a bar.
    harness.get_by_label("Tab Untitled 1").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.first().copied());
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Bars(1)));
    harness.get_by_label("Arrangement view").click();
    harness.run();
    zoom_in_at_beat_b(&mut harness);
    drag_beat_b(&mut harness, egui::vec2(160.0, 0.0));
    assert_eq!(beat_b_start(&harness), Some(68.0));

    // Back in the second, its 1/16 is still chosen and is its grid.
    harness.get_by_label("Tab Untitled 2").click();
    harness.run();
    harness.run();
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));
    harness.get_by_label("Arrangement view").click();
    harness.run();
    zoom_in_at_beat_b(&mut harness);
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    assert_eq!(beat_b_start(&harness), Some(65.25));
}
