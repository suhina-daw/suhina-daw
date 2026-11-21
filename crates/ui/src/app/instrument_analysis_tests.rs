// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Actual SDK analysis controls dispatch the same session commands as CLI/MCP.
#![allow(clippy::unwrap_used)]

use super::*;
use egui_kittest::{Harness, kittest::Queryable};

#[test]
fn instrument_analysis_controls_require_explicit_acceptance_and_reject_stale_proposals() {
    let path = std::env::temp_dir().join(format!("suhina-analysis-ui-{}.wav", std::process::id()));
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for frame in 0..96000 {
        let time = frame as f64 / 48000.0;
        wav.write_sample(
            ((std::f64::consts::TAU * 220.0 * time).sin() * (-1000.0_f64.ln() * time / 1.8).exp() * 12000.0) as i16,
        )
        .unwrap();
    }
    wav.finalize().unwrap();
    let mut session = Session::new(suhina_session::blank_project());
    session.execute("device.add", &json!({"track":1,"id":"instrument"})).unwrap();
    session.execute("device.loadSample", &json!({"track":1,"device":0,"path":path})).unwrap();
    let before = session.project().clone();
    let depth = session.undo_steps();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    app.view.selected = Some(TrackId(1));
    app.detail_open = true;
    app.detail_showing = detail_view::Showing::Devices;
    let mut h = Harness::builder()
        .allow_missing_glyphs()
        .with_size(Vec2::new(1280.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    crate::kit::theme::install(&h.ctx);
    h.run_steps(4);
    h.get_by_label("Open editor").click();
    h.run_steps(4);
    h.get_by_label("Sample page").click();
    h.run_steps(4);
    h.get_by_label("Analyze sample").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    h.state_mut().projects.session_mut(None).unwrap().sync_instrument_analysis();
    h.run_steps(4);
    assert!(h.query_by_label("Proposed root key").is_some());
    // Only the page the panel shows is kept (view state, no undo step); the sound is unchanged.
    let shown = h.state().session().unwrap().project().clone();
    assert_eq!(suhina_engine::model::Project { panel_pages: before.panel_pages.clone(), ..shown }, before);
    assert_eq!(h.state().session().unwrap().undo_steps(), depth);
    h.get_by_label("Accept root").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let accepted = h.state().session().unwrap();
    assert_eq!(accepted.undo_steps().0, depth.0 + 1);
    assert_eq!(accepted.project().channels[0].devices[0].params.iter().find(|p| p.id == "root").unwrap().value, 57.0);
    assert!(h.query_by_label("Accept root").is_none());
    h.state_mut().run(&Action::new("edit.undo", json!({})));
    h.run_steps(4);
    // Undo takes the root back and leaves the page the panel shows.
    let undone = h.state().session().unwrap().project().clone();
    assert_eq!(undone.panel_page(undone.channels[0].devices[0].id), 2, "the Sample page stays");
    assert_eq!(suhina_engine::model::Project { panel_pages: before.panel_pages.clone(), ..undone }, before);
    h.get_by_label("Analyze sample").click();
    h.run_steps(2);
    h.state_mut().projects.session_mut(None).unwrap().sync_instrument_analysis();
    h.run_steps(4);
    assert!(h.query_by_label("Accept root").is_some());
    h.state_mut().run(&Action::new("device.setParam", json!({"track":1,"device":0,"param":"start","value":0.1})));
    h.run_steps(4);
    assert!(h.query_by_label("Accept root").is_none());
    assert!(h.query_by_label("the device, source or trim changed; analyze the current source again").is_some());
    h.state_mut().run(&Action::new("device.setParam", json!({"track":1,"device":0,"param":"start","value":0})));
    h.run_steps(4);
    let fit_before = h.state().session().unwrap().project().clone();
    let fit_depth = h.state().session().unwrap().undo_steps();
    h.get_by_label("Fit resonances").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    h.state_mut().projects.session_mut(None).unwrap().sync_instrument_analysis();
    h.run_steps(4);
    assert!(h.query_by_label("Accept model").is_some());
    assert_eq!(h.state().session().unwrap().project(), &fit_before);
    assert_eq!(h.state().session().unwrap().undo_steps(), fit_depth);
    h.get_by_label("Accept model").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert_eq!(h.state().session().unwrap().undo_steps().0, fit_depth.0 + 1);
    assert!(h.state().session().unwrap().project().channels[0].devices[0].instrument_modal_profile().is_some());
    assert!(h.query_by_label("Accept model").is_none());
    // With a fitted model accepted, the next fit offers to join it.
    h.get_by_label("Fit resonances").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    h.state_mut().projects.session_mut(None).unwrap().sync_instrument_analysis();
    h.run_steps(4);
    assert!(h.query_by_label("Add to model (1)").is_some());
    // Another instrument's recording joins as a source of its own, for Source morph.
    h.get_by_label("Add as new source").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let device = &h.state().session().unwrap().project().channels[0].devices[0];
    let set = suhina_dsp::modal::model::ModelSet::decode(device.instrument_modal_profile().unwrap()).unwrap();
    assert_eq!(set.recordings().iter().map(|r| r.source).collect::<Vec<_>>(), vec![0, 1]);
    h.state_mut().run(&Action::new("edit.undo", json!({})));
    h.run_steps(4);
    // Fit folder… asks for the folder with the system's dialog before anything runs.
    h.get_by_label("Fit folder…").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(
        h.state_mut().take_pick_request().map(|request| request.field),
        Some(crate::interaction::pick::PickField::InstrumentFitFolder)
    );
    h.state_mut().run(&Action::new("edit.undo", json!({})));
    h.run_steps(4);
    assert_eq!(h.state().session().unwrap().project(), &fit_before);
    std::fs::remove_file(path).unwrap();
}
