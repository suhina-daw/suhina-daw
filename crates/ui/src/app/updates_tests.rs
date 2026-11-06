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
