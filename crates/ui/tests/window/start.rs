// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Starting the app: the start panel, recent projects, templates, backups a crash left and the Help menu.

use super::*;

#[test]
fn an_empty_window_asks_for_a_project() {
    let harness = window(App::new());
    assert!(harness.state().session().is_none());
    assert!(matches!(harness.state().status(), Status::Hint(_)));
}

/// A project saved in `dir` as `Song.suhina`, then changed and backed up by a run that never
/// ends normally (a crash), its backups and journal in `dir/settings`. Returns the project file
/// and the backup.
#[allow(clippy::unwrap_used)] // a test helper outside #[test] functions
fn crashed_run(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let file = dir.join("Song.suhina");
    Session::new(demo_project()).save(&file, false).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut projects = suhina_session::Projects::new();
    projects.set_backup_folder(Some(dir.join("settings")));
    projects.add(Session::open(&file).unwrap().0, None).unwrap();
    projects.execute("transport.setTempo", &json!({ "bpm": 97 })).unwrap();
    let written = projects.execute("project.autosave", &json!({ "now": true })).unwrap();
    (file, std::path::PathBuf::from(written["written"].as_str().unwrap()))
}

#[test]
fn a_backup_a_crash_left_is_offered_on_start_and_opens_tied_to_its_file() {
    let dir = crate::temp_dir::TempDir::new("ui-recovery");
    let (file, backup) = crashed_run(&dir);

    let mut app = App::new();
    app.set_browser_open(false);
    app.projects_mut().set_backup_folder(Some(dir.join("settings")));
    app.offer_recovery();
    let mut harness = window(app);
    assert!(harness.query_by_label("Open the backup").is_some());
    // Show the folder hands the backup to the app and keeps asking.
    harness.get_by_label("Show the backup's folder").click();
    harness.run();
    assert_eq!(harness.state_mut().take_reveal_request(), Some(backup.clone()));
    assert!(harness.query_by_label("Open the backup").is_some());
    harness.get_by_label("Open the backup").click();
    harness.run();
    assert!(harness.query_by_label("Open the backup").is_none());
    let offer = harness.state_mut().take_recovery_request().expect("the backup to open");
    assert_eq!((offer.backup.clone(), offer.file.clone()), (backup.clone(), Some(file.clone())));
    // What the app does with it.
    let (session, _) = Session::recover(&offer.backup, offer.file.as_deref()).unwrap();
    harness.state_mut().open_project(session, &[], OpenAs::NewTab);
    harness.run();
    let session = harness.state().session().unwrap();
    assert!(session.has_unsaved_changes());
    assert_eq!((session.source(), session.project().transport.tempo_bpm), (Some(file.as_path()), 97.0));

    // Offered once: the next start finds nothing.
    assert!(harness.state_mut().finish_backups());
    let mut next = App::new();
    next.projects_mut().set_backup_folder(Some(dir.join("settings")));
    next.offer_recovery();
    let harness = window(next);
    assert!(harness.query_by_label("Open the backup").is_none());
}

#[test]
fn ignoring_an_offered_backup_leaves_it_and_asks_no_more() {
    let dir = crate::temp_dir::TempDir::new("ui-recovery-ignore");
    let (_file, backup) = crashed_run(&dir);
    let mut app = App::new();
    app.projects_mut().set_backup_folder(Some(dir.join("settings")));
    app.offer_recovery();
    let mut harness = window(app);
    harness.get_by_label("Ignore the backup").click();
    harness.run();
    assert!(harness.query_by_label("Open the backup").is_none());
    assert!(harness.state_mut().take_recovery_request().is_none());
    assert!(harness.state().projects().is_empty());
    assert!(harness.state_mut().finish_backups());
    assert!(backup.is_file(), "the backup stays in its folder");
    let mut next = App::new();
    next.projects_mut().set_backup_folder(Some(dir.join("settings")));
    next.offer_recovery();
    assert!(window(next).query_by_label("Open the backup").is_none());
}

/// A window with nothing open that starts as `settings` say, given the last project `last`.
fn started(settings: suhina_ui::settings::Settings, last: Option<std::path::PathBuf>) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_settings(&egui::Context::default(), settings);
    app.open_at_start(last);
    window(app)
}

fn at_start(choice: suhina_ui::settings::AtStart) -> suhina_ui::settings::Settings {
    suhina_ui::settings::Settings { at_start: choice, ..Default::default() }
}

#[test]
fn at_start_the_start_panel_shows_unless_the_settings_say_otherwise() {
    use suhina_ui::settings::AtStart;
    let mut harness = started(at_start(AtStart::Panel), None);
    assert!(!harness.state().project_dialog_open());
    assert!(harness.state().start_panel_open());
    assert!(harness.query_by_label("New project").is_some(), "the start panel shows");
    assert!(harness.state_mut().take_lists_wanted().is_some(), "and asks for its lists");
    assert_eq!(harness.state_mut().take_new_request(), None);

    let mut harness = started(at_start(AtStart::Blank), None);
    assert!(!harness.state().project_dialog_open());
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank));
}

#[test]
fn at_start_a_new_project_comes_from_the_start_template_or_the_panel_says_why_not() {
    use suhina_ui::settings::AtStart;
    let folder = crate::temp_dir::TempDir::new("at-start");
    let template = folder.join("Four on the floor.suhina");
    std::fs::write(&template, b"{}").unwrap_or_else(|e| panic!("{e}"));
    let mut settings = at_start(AtStart::Template);
    settings.templates_folder = folder.display().to_string();
    // A name in the templates folder, or a whole path.
    for named in ["Four on the floor.suhina".to_string(), template.display().to_string()] {
        settings.start_template = named;
        let mut harness = started(settings.clone(), None);
        assert!(!harness.state().project_dialog_open());
        assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Template(template.clone())));
    }
    settings.start_template = "Gone.suhina".into();
    let mut harness = started(settings.clone(), None);
    assert!(!harness.state().project_dialog_open());
    assert!(harness.query_by_label("New project").is_some(), "the start panel shows");
    assert_eq!(harness.state_mut().take_new_request(), None);
    assert!(
        matches!(harness.state().status(), Status::Hint(h) if h.contains("not there")),
        "{:?}",
        harness.state().status()
    );
    settings.start_template = String::new();
    let harness = started(settings, None);
    assert!(harness.query_by_label("New project").is_some());
    assert!(matches!(harness.state().status(), Status::Hint(h) if h.contains("No template")));
}

#[test]
fn at_start_the_last_project_opens_or_the_panel_says_there_is_none() {
    use suhina_ui::settings::AtStart;
    let last = std::path::PathBuf::from("/songs/Last night.suhina");
    let mut harness = started(at_start(AtStart::Last), Some(last.clone()));
    assert!(!harness.state().project_dialog_open());
    assert_eq!(harness.state_mut().take_open_request(), Some((last, OpenAs::NewTab)));

    let mut harness = started(at_start(AtStart::Last), None);
    assert!(harness.query_by_label("New project").is_some());
    assert_eq!(harness.state_mut().take_open_request(), None);
    assert!(matches!(harness.state().status(), Status::Hint(h) if h.contains("No project was opened before")));
}

#[test]
fn at_start_is_chosen_in_settings_and_template_asks_for_the_template() {
    use suhina_ui::settings::AtStart;
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.query_by_label("Start template").is_none(), "only for a template");
    harness.get_by_label("At start: Template").click();
    harness.run();
    assert_eq!(harness.state().settings().at_start, AtStart::Template);
    assert!(harness.query_by_label("Start template").is_some());
    harness.get_by_label("At start: Last").click();
    harness.run();
    assert_eq!(harness.state().settings().at_start, AtStart::Last);
    let saved = harness.state_mut().take_changed_settings().map(|s| s.to_text()).unwrap_or_default();
    assert!(saved.contains("at_start = last"), "{saved}");
}

#[test]
fn the_start_panel_opens_recent_projects_forgets_them_and_comes_back_when_the_last_project_closes() {
    let night = std::path::PathBuf::from("/songs/Night drive.suhina");
    let morning = std::path::PathBuf::from("/songs/Morning.suhina");
    let mut app = App::new();
    app.set_project_lists(vec![night.clone(), morning.clone()], Vec::new());
    let mut harness = window(app);
    assert!(harness.state().start_panel_open());
    harness.get_by_label("Open Night drive").click();
    harness.run();
    assert_eq!(harness.state_mut().take_open_request(), Some((night, OpenAs::NewTab)));

    harness.get_by_label("Open Morning").click_secondary();
    harness.run();
    harness.get_by_label("Remove from list").click();
    harness.run();
    assert_eq!(harness.state_mut().take_forget_request(), Some(morning));
    assert!(harness.query_by_label("Open Morning").is_none(), "it is gone from the panel at once");

    harness.get_by_label("Audio settings").click();
    // The Audio page repaints while it shows, for its live figures.
    harness.run_steps(2);
    assert!(harness.state().settings_open());
    harness.key_press(Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().settings_open());

    harness.get_by_label("New project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank));

    // With a project open the panel is gone; when the last one closes it is back, with its lists
    // read again.
    let _ = harness.state_mut().take_lists_wanted();
    harness.state_mut().set_session(Session::new(demo_project()), &[]);
    harness.run();
    assert!(harness.query_by_label("New project").is_none());
    harness.get_by_label("Projects").click();
    harness.run();
    harness.get_by_label("Close project").click();
    harness.run();
    if let Some(dont) = harness.query_by_label("Don't save the changes") {
        dont.click();
        harness.run();
    }
    assert!(harness.state().start_panel_open());
    assert!(harness.query_by_label("New project").is_some());
    assert!(harness.state_mut().take_lists_wanted().is_some());
}

#[test]
fn browse_buttons_ask_for_the_systems_dialog_and_fill_their_fields() {
    use suhina_ui::interaction::pick::PickField;
    // Settings › General: the folders and the start template.
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Browse for Projects folder").click();
    harness.run();
    let request = harness.state_mut().take_pick_request().unwrap_or_else(|| panic!("no dialog asked for"));
    assert_eq!(request.field, PickField::ProjectsFolder);
    assert!(request.field.folder());
    let _ = harness.state_mut().take_changed_settings();
    harness.state_mut().set_picked(PickField::ProjectsFolder, "/music/projects".into());
    harness.run();
    assert_eq!(harness.state().settings().projects_folder, "/music/projects");
    let saved = harness.state_mut().take_changed_settings().map(|s| s.to_text()).unwrap_or_default();
    assert!(saved.contains("projects_folder = /music/projects"), "{saved}");
    harness.get_by_label("At start: Template").click();
    harness.run();
    harness.get_by_label("Browse for Start template").click();
    harness.run();
    let request = harness.state_mut().take_pick_request().unwrap_or_else(|| panic!("no dialog asked for"));
    assert_eq!(request.field, PickField::StartTemplate);
    assert_eq!(request.field.extensions(), ["suhina", "als"]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();

    // Save as: the folder; the prompt stays open meanwhile.
    harness.state_mut().do_action("file.saveAs", &mut Vec::new());
    harness.run();
    harness.get_by_label("Browse for the folder").click();
    harness.run();
    assert_eq!(harness.state_mut().take_pick_request().map(|r| r.field), Some(PickField::SaveFolder));
    harness.state_mut().set_picked(PickField::SaveFolder, "/music/songs".into());
    harness.run();
    assert_eq!(harness.get_by_label("Folder").value().as_deref(), Some("/music/songs"));
    harness.key_press(Key::Escape);
    harness.run();

    // File › Open…: a project, which then opens; cancelling the dialog keeps File › Open.
    harness.state_mut().open_project_dialog(true);
    harness.run();
    harness.get_by_label("Browse for a project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_pick_request().map(|r| r.field), Some(PickField::OpenProject));
    assert!(harness.state().project_dialog_open(), "the dialog stays while the system's is shown");
    harness.state_mut().set_picked(PickField::OpenProject, "/music/songs/Night.suhina".into());
    harness.run();
    assert!(!harness.state().project_dialog_open());
    // A project is open, so Suhina asks how to open it, as for any other way of opening one.
    harness.get_by_label("In a new tab").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_open_request().map(|(path, _)| path),
        Some(std::path::PathBuf::from("/music/songs/Night.suhina"))
    );
}

#[test]
fn open_on_the_start_panel_shows_the_systems_dialog() {
    use suhina_ui::interaction::pick::PickField;
    let mut harness = window(App::new());
    harness.get_by_label("Open a project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_pick_request().map(|r| r.field), Some(PickField::OpenProject));
    harness.state_mut().set_picked(PickField::OpenProject, "/music/Old jam.als".into());
    harness.run();
    assert_eq!(
        harness.state_mut().take_open_request().map(|(path, _)| path),
        Some(std::path::PathBuf::from("/music/Old jam.als"))
    );
}

#[test]
fn help_about_shows_the_version_and_copies_it() {
    let mut harness = window(demo_app());
    let version = suhina_ui::app::project_dialog::VERSION;
    let build = suhina_ui::app::project_dialog::BUILD;
    assert!(!build.is_empty(), "the build script sets the build string");
    assert!(harness.query_by_label_contains(version).is_none());
    harness.get_by_label("Help").click();
    harness.run();
    harness.get_by_label_contains("About Suhina").click();
    harness.run();
    let line = suhina_ui::app::project_dialog::version_line();
    assert!(line.starts_with(&format!("Suhina {version} build ")), "{line}");
    assert!(line.contains(build), "the copied line carries the build string: {line}");
    harness.get_by_label(&format!("Copy version: {line}")).click();
    harness.run();
    assert_eq!(harness.state().status(), &Status::Done(format!("Copied {line}")));
    // Still open after copying; Escape closes it.
    assert!(harness.query_by_label_contains(version).is_some());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label_contains(version).is_none());
}

#[test]
fn help_check_for_updates_opens_the_dialog_and_offers_no_gh_on_the_nightly_channel() {
    let mut harness = window(demo_app());
    harness.get_by_label("Help").click();
    harness.run();
    harness.get_by_label_contains("Check for updates").click();
    harness.run();
    harness.run();
    assert!(harness.query_by_label("Check for updates now").is_some());
    // The GitHub CLI is only offered for test builds, and only connected on a click.
    assert!(harness.query_by_label("Connect Suhina to your gh").is_none());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Check for updates now").is_none());
}
