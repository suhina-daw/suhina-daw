// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendered images of the window, the piano roll, the start panel and every theme.

use super::*;

/// Renders the window with the demo project and the piano roll of the **Beat B** clip on **Drums**
/// to `target/ui-snapshots/piano-roll-*.png`, to look the toolbar, the note clipboard buttons and
/// the quantise block over: the row layout, the quantise block open in it, and the stacked detail
/// area with the block open. Needs a GPU adapter, so it runs only when asked:
/// `cargo test -p suhina-ui --test window -- --ignored render_the_piano_roll`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_piano_roll() {
    let mut renderer = suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000)
        .unwrap_or_else(|e| panic!("{e}"));
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = window_with_step(app, 0.05);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click();
    harness.run();
    // A note selected, so no clipboard button is greyed out.
    let note = harness
        .query_all_by_label_contains("Note ")
        .next()
        .unwrap_or_else(|| panic!("a note in the grid"))
        .rect()
        .center();
    harness.hover_at(note);
    harness.step();
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: note,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }
    harness.run();
    harness.remove_cursor();
    harness.run();

    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    let save = |harness: &mut Harness<'static, App>, name: &str| {
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(name)).unwrap_or_else(|e| panic!("{e}"));
    };
    save(&mut harness, "piano-roll.png");

    harness.state_mut().piano_roll_mut().quantize_open = true;
    harness.run();
    assert!(harness.state().piano_roll().quantize_open, "the block is open");
    assert!(harness.query_by_label("Quantise notes").is_some(), "and its controls are drawn");
    save(&mut harness, "piano-roll-quantise.png");

    let ctx = harness.ctx.clone();
    let stacked =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, stacked);
    harness.run();
    harness.state_mut().piano_roll_mut().quantize_open = true;
    harness.run();
    save(&mut harness, "piano-roll-quantise-stacked.png");
}

/// Renders the window with the demo project to `target/ui-snapshots/session.png` and
/// `arrangement.png`, to compare with the mockups in `docs/design/reference/`. Needs a GPU adapter, so it runs only
/// when asked: `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_session_window() {
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), demo_app());
    theme::install(&harness.ctx);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    image.save(folder.join("session.png")).unwrap_or_else(|e| panic!("{e}"));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("arrangement.png")).unwrap_or_else(|e| panic!("{e}"));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.state_mut().set_project_lists(
        vec!["C:/Music/Suhina/Projects/Night drive.suhina".into(), "C:/Music/Sets/Old jam.als".into()],
        vec!["C:/Music/Suhina/Templates/Beat starter.suhina".into(), "C:/Music/Suhina/Templates/Live rig.als".into()],
    );
    harness.state_mut().open_project_dialog(false);
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("project-dialog.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press(Key::Escape);
    harness.run();
    harness.state_mut().do_action("file.saveAs", &mut Vec::new());
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("save-prompt.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press(Key::Escape);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Theme").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings-theme.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings-arrangement.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: Shortcuts").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("shortcuts.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: General").click();
    harness.run();

    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), audio_app());
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("arrangement-audio.png")).unwrap_or_else(|e| panic!("{e}"));

    // The detail area with a track's devices, in a row and stacked.
    let mut harness = Harness::builder().with_size(SIZE).with_pixels_per_point(2.0).wgpu().build_ui_state(
        |ui, app: &mut App| app.ui(ui),
        {
            let mut app = App::new();
            app.set_session(Session::new(project_with_a_device()), &[]);
            app
        },
    );
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("detail-row.png")).unwrap_or_else(|e| panic!("{e}"));
    let ctx = harness.ctx.clone();
    let settings =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("detail-stack.png")).unwrap_or_else(|e| panic!("{e}"));

    // A Live Set dragged over the window darkens it.
    harness.input_mut().hovered_files.push(egui::HoveredFile { path: Some("set.als".into()), ..Default::default() });
    harness.run_steps(12);
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("drop.png")).unwrap_or_else(|e| panic!("{e}"));
}

/// Renders the Session and Arrangement views in every theme that comes with Suhina, to
/// `target/ui-snapshots/theme-<id>-session.png` and `-arrangement.png`, to look them over. Needs
/// a GPU adapter: `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_every_built_in_theme() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    for (id, _) in theme::built_in_themes() {
        let mut harness =
            Harness::builder().with_size(SIZE).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), demo_app());
        theme::install(&harness.ctx);
        let ctx = harness.ctx.clone();
        harness.state_mut().use_theme(&ctx, id);
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(format!("theme-{id}-session.png"))).unwrap_or_else(|e| panic!("{e}"));
        harness.get_by_label("Arrangement view").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(format!("theme-{id}-arrangement.png"))).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// Renders the Arrangement view with an audio clip, linear and in decibels, to
/// `target/ui-snapshots/arrangement-waveform*.png`, to look at the clip waveform in the window:
/// `cargo test -p suhina-ui --test window -- --ignored render_the_clip_waveform`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_clip_waveform() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    for db in [false, true] {
        let mut app = audio_app();
        app.set_browser_open(false);
        let mut harness = Harness::builder().with_size(SIZE).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        theme::install(&harness.ctx);
        let ctx = harness.ctx.clone();
        let mut settings = harness.state().settings().clone();
        settings.waveform_db = db;
        harness.state_mut().set_settings(&ctx, settings);
        harness.run();
        harness.get_by_label("Arrangement view").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        let name = if db { "arrangement-waveform-db.png" } else { "arrangement-waveform.png" };
        image.save(folder.join(name)).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// Renders the start panel to `target/ui-snapshots/start-panel.png`. Needs a GPU adapter:
/// `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_start_panel() {
    let mut app = App::new();
    app.set_project_lists(
        vec![
            "C:/Music/Suhina/Projects/Night drive.suhina".into(),
            "C:/Music/Suhina/Projects/Morning.suhina".into(),
            "C:/Music/Sets/Old jam.als".into(),
        ],
        vec!["C:/Music/Suhina/Templates/Beat starter.suhina".into()],
    );
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    image.save(folder.join("start-panel.png")).unwrap_or_else(|e| panic!("{e}"));
}
