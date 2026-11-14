// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

type Panel = (StartState, Option<StartChoice>);

fn panel(lists: Lists, size: Vec2) -> Harness<'static, Panel> {
    panel_with(lists, size, None)
}

fn panel_with(lists: Lists, size: Vec2, notice: Option<Notice>) -> Harness<'static, Panel> {
    let audio = AudioSummary { device: "Speakers".into(), sample_rate: "48000 Hz".into(), buffer: "256".into() };
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    Harness::builder().allow_missing_glyphs().with_size(size).build_ui_state(
        move |ui, (state, chosen): &mut Panel| {
            let area = ui.max_rect();
            if let Some(choice) = show(ui, area, state, &lists, &audio, "0.26.0", notice.as_ref(), now) {
                *chosen = Some(choice);
            }
        },
        (StartState::default(), None),
    )
}

fn lists() -> Lists {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    Lists {
        recent: vec![PathBuf::from("songs/Night drive.suhina"), PathBuf::from("songs/Morning.suhina")],
        saved: vec![Some(now - Duration::from_secs(3 * 86_400)), None],
        templates: vec![PathBuf::from("templates/Beat.suhina")],
        templates_folder: "templates".into(),
        new_returns: true,
    }
}

#[test]
fn the_panel_makes_new_projects_and_opens_recent_ones() {
    let mut harness = panel(lists(), Vec2::new(900.0, 600.0));
    harness.run();
    harness.get_by_label("New project").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::NewBlank));
    harness.get_by_label("Open Night drive").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::Open("songs/Night drive.suhina".into())));
    harness.get_by_label("Open a project").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::Browse));
    harness.get_by_label("Audio settings").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::AudioSettings));
    // From a template shows the templates; one of them starts a new project.
    harness.get_by_label("From a template").click();
    harness.run();
    assert_eq!(harness.state().0.tab, Tab::Templates);
    harness.get_by_label("Start from Beat").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::NewFromTemplate("templates/Beat.suhina".into())));
    harness.get_by_label("Recent projects").click();
    harness.run();
    assert_eq!(harness.state().0.tab, Tab::Recent);
}

/// Two recent projects of the same name in different folders, and two templates of the same
/// name: each row opens its own (#1229).
#[test]
fn rows_of_the_same_name_open_their_own_project() {
    let lists = Lists {
        recent: vec![PathBuf::from("a/Song.suhina"), PathBuf::from("b/Song.suhina")],
        saved: vec![None, None],
        templates: vec![PathBuf::from("templates/Song.suhina"), PathBuf::from("templates/Song.als")],
        templates_folder: "templates".into(),
        new_returns: true,
    };
    let mut harness = panel(lists, Vec2::new(900.0, 600.0));
    harness.run();
    for (at, path) in [(0, "a/Song.suhina"), (1, "b/Song.suhina")] {
        let mut rows: Vec<_> = harness.query_all_by_label("Open Song").collect();
        assert_eq!(rows.len(), 2);
        rows.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
        rows[at].click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::Open(path.into())));
    }
    harness.get_by_label("Templates projects").click();
    harness.run();
    for (at, path) in [(0, "templates/Song.suhina"), (1, "templates/Song.als")] {
        let mut rows: Vec<_> = harness.query_all_by_label("Start from Song").collect();
        assert_eq!(rows.len(), 2);
        rows.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
        rows[at].click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::NewFromTemplate(path.into())));
    }
}

#[test]
fn a_newer_build_is_offered_under_the_actions() {
    let notice = Notice {
        text: "New nightly available: Suhina 0.0.1-nightly.20261018+c176a33".into(),
        button: Some("Download update now"),
        progress: None,
        error: false,
        page: Some("https://example.invalid/nightly".into()),
    };
    let mut harness = panel_with(lists(), Vec2::new(900.0, 600.0), Some(notice));
    harness.run();
    assert!(harness.query_by_label("Open Night drive").is_some());
    harness.get_by_label("Download update now").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::Update));
    harness.get_by_label("What's new in the new version").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::UpdatePage("https://example.invalid/nightly".into())));
}

#[test]
fn the_search_field_filters_the_list_by_name() {
    let mut harness = panel(lists(), Vec2::new(900.0, 600.0));
    harness.run();
    assert!(harness.query_by_label("Open Morning").is_some());
    harness.get_by_label("Search projects").click();
    harness.run();
    harness.get_by_label("Search projects").type_text("NIGHT");
    harness.run();
    assert!(harness.query_by_label("Open Night drive").is_some());
    assert!(harness.query_by_label("Open Morning").is_none());
}

#[test]
fn a_recent_project_is_taken_off_the_list_from_its_menu() {
    let mut harness = panel(lists(), Vec2::new(900.0, 600.0));
    harness.run();
    harness.get_by_label("Open Morning").click_secondary();
    harness.run();
    harness.get_by_label("Remove from list").click();
    harness.run();
    assert_eq!(harness.state().1, Some(StartChoice::Forget("songs/Morning.suhina".into())));
}

#[test]
fn the_panel_fits_a_narrow_window_and_lists_nothing_without_projects() {
    let mut harness = panel(Lists::default(), Vec2::new(420.0, 520.0));
    harness.run();
    assert!(harness.query_by_label("Audio settings").is_some(), "the audio card goes under the list");
    assert!(harness.query_by_label("New project").is_some());
}

#[test]
fn ages_read_in_words() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    let before = |s: u64| ago(now, now - Duration::from_secs(s));
    assert_eq!(before(10), "Just now");
    assert_eq!(before(60), "A minute ago");
    assert_eq!(before(5 * 60), "5 minutes ago");
    assert_eq!(before(3600), "An hour ago");
    assert_eq!(before(5 * 3600), "5 hours ago");
    assert_eq!(before(30 * 3600), "Yesterday");
    assert_eq!(before(3 * 86_400), "3 days ago");
    assert_eq!(before(8 * 86_400), "Last week");
    assert_eq!(before(15 * 86_400), "2 weeks ago");
    assert_eq!(before(40 * 86_400), "Last month");
    assert_eq!(before(100 * 86_400), "3 months ago");
    assert_eq!(before(400 * 86_400), "Last year");
    assert_eq!(before(800 * 86_400), "2 years ago");
    // A file saved in the future (a clock set wrong) is just now.
    assert_eq!(ago(now, now + Duration::from_secs(100)), "Just now");
}
