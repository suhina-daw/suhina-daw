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
    for frame in 0..24000 {
        wav.write_sample(((std::f32::consts::TAU * 220.0 * frame as f32 / 48000.0).sin() * 12000.0) as i16).unwrap();
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
    assert_eq!(h.state().session().unwrap().project(), &before);
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
    assert_eq!(h.state().session().unwrap().project(), &before);
    h.get_by_label("Analyze sample").click();
    h.run_steps(2);
    h.state_mut().projects.session_mut(None).unwrap().sync_instrument_analysis();
    h.run_steps(4);
    assert!(h.query_by_label("Accept root").is_some());
    h.state_mut().run(&Action::new("device.setParam", json!({"track":1,"device":0,"param":"start","value":0.1})));
    h.run_steps(4);
    assert!(h.query_by_label("Accept root").is_none());
    assert!(h.query_by_label("the device, source or trim changed; analyze the current source again").is_some());
    std::fs::remove_file(path).unwrap();
}
