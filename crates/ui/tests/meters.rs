// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Level meters in the window: a playing track's meter moves, its clip light stays lit until
//! it is clicked, and the main output has a meter too, in both views.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

/// The label of the meter whose label starts with `name`.
fn meter(harness: &Harness<'static, App>, name: &str) -> String {
    let label = format!("{name} level:");
    harness.get_by_label_contains(&label).accesskit_node().label().unwrap_or_default()
}

/// Renders a quarter of a second, as the audio device would.
fn play_a_while(renderer: &mut suhina_engine::Renderer) {
    let mut block = [0.0f32; 2 * 256];
    for _ in 0..47 {
        renderer.process(&mut block);
    }
}

#[test]
fn a_playing_tracks_meter_moves_and_its_clip_light_turns_off_on_a_click() {
    let mut project = demo_project();
    // Drums at +18 dB, loud enough to go over full scale.
    project.channels[0].volume = 8.0;
    let mut renderer =
        suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000, suhina_session::devices())
            .unwrap();
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run_steps(4);
    assert_eq!(meter(&harness, "Drums"), "Drums level: silent");
    assert_eq!(meter(&harness, "Main"), "Main level: silent");

    harness.get_by_label("Play").click();
    harness.run_steps(2);
    play_a_while(&mut renderer);
    harness.run_steps(2);
    let drums = meter(&harness, "Drums");
    assert!(drums.ends_with("dB, clipped"), "{drums}");
    assert!(meter(&harness, "Main").ends_with(", clipped"));

    // A click on the meter turns its clip light off; the main output's stays lit.
    harness.get_by_label_contains("Drums level:").click();
    harness.run_steps(2);
    assert!(meter(&harness, "Drums").ends_with(" dB"), "{}", meter(&harness, "Drums"));
    assert!(meter(&harness, "Main").ends_with(", clipped"));

    // The Arrangement view's headers and main row show the same meters.
    harness.get_by_label("Arrangement view").click();
    harness.run_steps(2);
    assert!(meter(&harness, "Drums").ends_with(" dB"));
    harness.get_by_label_contains("Main level:").click();
    harness.run_steps(2);
    assert!(!meter(&harness, "Main").contains("clipped"), "{}", meter(&harness, "Main"));

    // Stopped, the meters fall back to silence; Drums clipped again while it played on, and its
    // light stays lit.
    harness.get_by_label("Stop").click();
    harness.run_steps(2);
    for _ in 0..30 {
        play_a_while(&mut renderer);
    }
    harness.run_steps(2);
    assert_eq!(meter(&harness, "Drums"), "Drums level: silent, clipped");
}
