// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Previewing from the browser end to end, through a real audio output: a sample row pressed
//! starts playing, and keeps playing when the press turns into a drag. Needs an audio device, so it is ignored by default
//! (`cargo test -p suhina-ui --test browser_preview -- --ignored`).
#![allow(clippy::unwrap_used, clippy::panic)]

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_session::{Projects, Session, blank_project};
use suhina_ui::browser::BrowserState;
use suhina_ui::browser::browser_view;
use suhina_ui::settings::Settings;

struct State {
    projects: Projects,
    browser: BrowserState,
    settings: Settings,
}

#[test]
#[ignore]
fn a_clicked_sample_previews() {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let root =
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("browser-e2e-{}-{nanos}", std::process::id()));
    let samples = root.join("samples");
    std::fs::create_dir_all(&samples).unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(samples.join("tone.wav"), spec).unwrap();
    for i in 0..88_200 {
        writer.write_sample(((i as f32 * 0.05).sin() * 3000.0) as i16).unwrap();
    }
    writer.finalize().unwrap();

    let mut projects = Projects::new();
    projects.set_file_library(suhina_library::Library::new(Some(root.join("settings"))));
    let mut session = Session::new(blank_project());
    let player = suhina_output::open_chosen(
        session.project(),
        session.audio(),
        &suhina_engine::DeviceAssets::new(),
        &suhina_session::OutputChoice::default(),
        None,
    )
    .unwrap()
    .player;
    session.attach_player(player);
    projects.add(session, None).unwrap();
    projects.execute("library.addFolder", &json!({ "folder": samples.display().to_string() })).unwrap();
    let mut browser = BrowserState::default();
    browser.open.insert(samples.display().to_string());

    let state = State { projects, browser, settings: Settings::default() };
    let mut harness = Harness::builder().with_size(egui::vec2(400.0, 600.0)).build_ui_state(
        |ui, s: &mut State| {
            let area = ui.max_rect();
            let cx = browser_view::Context {
                projects: &mut s.projects,
                settings: &s.settings,
                selected_track: None,
                search_key: String::new(),
                other_results: None,
            };
            let out = browser_view::show(ui, area, &mut s.browser, cx);
            if let Some(status) = out.status {
                println!("status: {status:?}");
            }
        },
        state,
    );
    harness.run_steps(5);
    // A real click: the button goes down, a few frames pass, it comes up.
    let at = harness.get_by_label("Browser tone.wav").rect().center();
    let at = egui::pos2(at.x + 60.0, at.y);
    harness.hover_at(at);
    harness.step();
    harness.drag_at(at);
    // Held long enough for the press to count as the start of a drag: the preview goes on.
    for _ in 0..6 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.event(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    let mut playing = false;
    for _ in 0..200 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let preview = harness.state_mut().projects.execute("launcher.state", &json!({})).unwrap()["preview"].clone();
        if preview["playing"] == json!(true) {
            playing = true;
            break;
        }
    }
    assert!(playing, "the clicked sample previews");
    let _ = std::fs::remove_dir_all(root);
}
