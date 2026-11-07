// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::app::project_dialog::Lists;
use crate::app::start_panel::{self, AudioSummary, StartChoice, StartState};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::time::SystemTime;

fn build() -> Build {
    Build {
        version: "0.0.1-nightly.20261106+abcdef1".into(),
        url: "https://example.invalid/suhina.zip".into(),
        size: 1_000_000,
        sha256: None,
        needs_sign_in: false,
        pr: None,
        commit: "abcdef1".into(),
        created: "2026-11-06T02:00:00Z".into(),
        page: "https://example.invalid/nightly".into(),
    }
}

/// What the start panel and the dialog drawn over it chose.
type Chosen = (Option<StartChoice>, Option<Choice>);

/// The window with no project open: the start panel with its update line for `phase`, and
/// Help › Check for updates… open over it, drawn in that order as the window draws them.
fn start_panel_under_the_dialog(phase: Phase) -> Harness<'static, Chosen> {
    let updates = Updates::default();
    let audio = AudioSummary { device: "Speakers".into(), sample_rate: "48000 Hz".into(), buffer: "256".into() };
    let mut state = StartState::default();
    Harness::builder().allow_missing_glyphs().with_size(Vec2::new(1000.0, 700.0)).build_ui_state(
        move |ui, (start, dialog): &mut Chosen| {
            let area = ui.max_rect();
            let notice = notice_of(phase.clone());
            let lists = Lists::default();
            if let Some(choice) = start_panel::show(
                ui,
                area,
                &mut state,
                &lists,
                &audio,
                VERSION,
                notice.as_ref(),
                SystemTime::UNIX_EPOCH,
            ) {
                *start = Some(choice);
            }
            if let Some(choice) = updates.dialog(ui.ctx(), area, Channel::Nightly, phase.clone()) {
                *dialog = Some(choice);
            }
        },
        (None, None),
    )
}

/// Clicks the dialog's button labelled `label`: the start panel's line has one of the same
/// label, nearer the top of the window.
fn click_the_dialogs(harness: &mut Harness<'static, Chosen>, label: &str) {
    harness.run();
    let lowest = harness.query_all_by_label(label).max_by(|a, b| a.rect().bottom().total_cmp(&b.rect().bottom()));
    lowest.unwrap_or_else(|| panic!("no {label} button")).click();
    harness.run();
}

/// #1229: with no project open, the start panel's update line and the dialog both show the
/// phase's button; a click on the dialog's reaches the dialog, not the panel under it.
#[test]
fn the_dialogs_update_buttons_work_over_the_start_panels_line() {
    for (phase, label, expected) in [
        (
            Phase::Checked { newer: Some(build()), builds: vec![build()] },
            "Download update now",
            Choice::Install(build()),
        ),
        (Phase::Installed(build()), "Restart now", Choice::Restart),
        (
            Phase::InstallFailed { build: build(), reason: "no connection".into() },
            "Try again",
            Choice::Install(build()),
        ),
    ] {
        let mut harness = start_panel_under_the_dialog(phase);
        assert_eq!(harness.query_all_by_label(label).count(), 2, "{label} on the start panel and in the dialog");
        click_the_dialogs(&mut harness, label);
        assert_eq!(harness.state(), &(None, Some(expected)), "{label}");
    }
}

/// The bar over an open project for `phase`, and what was chosen in it.
fn bar(phase: Phase) -> Harness<'static, Option<BarChoice>> {
    Harness::builder().allow_missing_glyphs().with_size(Vec2::new(1000.0, ROW + 12.0)).build_ui_state(
        move |ui, chosen: &mut Option<BarChoice>| {
            if let Some(choice) = bar_line(ui, ui.max_rect(), phase.clone()) {
                *chosen = Some(choice);
            }
        },
        None,
    )
}

/// A newer nightly is named as one in the bar over an open project, with its button, its page and
/// *Later*.
#[test]
fn a_newer_nightly_is_offered_in_the_bar_over_a_project() {
    let offer = Phase::Checked { newer: Some(build()), builds: vec![build()] };
    let text = notice_of(offer.clone()).map(|n| n.text).unwrap_or_default();
    assert_eq!(text, "New nightly available: Suhina 0.0.1-nightly.20261106+abcdef1");
    let mut harness = bar(offer);
    harness.run();
    harness.get_by_label("Download update now").click();
    harness.run();
    assert_eq!(harness.state(), &Some(BarChoice::Button));
    harness.get_by_label("What's new in the new version").click();
    harness.run();
    assert_eq!(harness.state(), &Some(BarChoice::Page(build().page)));
    harness.get_by_label("Hide the update offer").click();
    harness.run();
    assert_eq!(harness.state(), &Some(BarChoice::Later(build().version)));
}

/// A test build is not called a nightly.
#[test]
fn a_newer_test_build_is_named_as_one() {
    let test_build = Build { pr: Some(663), ..build() };
    let notice = notice_of(Phase::Checked { newer: Some(test_build.clone()), builds: vec![test_build] });
    let text = notice.map(|n| n.text).unwrap_or_default();
    assert!(text.starts_with("New test build available: the test build of #663"), "{text}");
}

/// *Later* hides the offer of that build only; a download or a failure shows again, and the bar has
/// nothing to show while nothing is newer.
#[test]
fn later_hides_only_the_offer_it_was_pressed_on() {
    let mut updates = Updates::default();
    let offer = Phase::Checked { newer: Some(build()), builds: vec![build()] };
    assert!(updates.bar_shows_for(&offer));
    updates.hidden = Some(build().version);
    assert!(!updates.bar_shows_for(&offer));
    let newer = Build { version: "0.0.1-nightly.20261107+1234567".into(), ..build() };
    assert!(updates.bar_shows_for(&Phase::Checked { newer: Some(newer.clone()), builds: vec![newer] }));
    assert!(updates.bar_shows_for(&Phase::InstallFailed { build: build(), reason: "no space".into() }));
    assert!(updates.bar_shows_for(&Phase::Installed(build())));
    assert!(!updates.bar_shows_for(&Phase::Checked { newer: None, builds: vec![build()] }));
    assert!(!updates.bar_shows_for(&Phase::Idle));
}
