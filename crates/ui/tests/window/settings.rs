// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings: the dialog, audio and MIDI pages, themes, keys, zoom, the toolbar and the sample map window.

use super::*;

#[test]
fn ctrl_and_plus_or_minus_zoom_the_whole_window_and_the_zoom_is_saved() {
    let mut harness = window(demo_app());
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Plus);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(120));
    assert!((harness.ctx.zoom_factor() - 1.2).abs() < 1e-6);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
    harness.run();
    assert_eq!(harness.state().settings().zoom, 110);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num0);
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(100));
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
}

#[test]
fn a_saved_zoom_is_used_from_the_start_and_settings_can_change_it() {
    let mut app = demo_app();
    let ctx = egui::Context::default();
    let settings = suhina_ui::settings::Settings { zoom: 150, ..Default::default() };
    app.set_settings(&ctx, settings);
    let mut harness = window(app);
    assert!((harness.ctx.zoom_factor() - 1.5).abs() < 1e-6);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Appearance").click();
    harness.run();
    harness.get_by_label("Zoom: -").click();
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(140));
    harness.get_by_label("Zoom: 140 %").click();
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(100));
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
}

#[test]
fn settings_change_right_away_and_are_handed_out_to_be_saved() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    harness.get_by_label("Settings page: Appearance").click();
    harness.run();
    harness.get_by_label("Animations: Off").click();
    harness.run();
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.get_by_label("Track headers: Left").click();
    harness.run();
    let saved = harness.state_mut().take_changed_settings();
    assert_eq!(saved.as_ref().map(|s| s.animations), Some(false));
    assert_eq!(saved.map(|s| s.track_headers), Some(suhina_ui::settings::Side::Left));
    assert!(!suhina_ui::kit::motion::enabled(&harness.ctx));
    assert_eq!(harness.state_mut().take_changed_settings(), None);
    harness.get_by_label("Close settings").click();
    harness.run();
    assert!(!harness.state().settings_open());
}

#[test]
fn a_theme_picked_in_settings_colours_the_window_and_is_saved() {
    let mut harness = window(demo_app());
    let mut colors = theme::Palette::DARK;
    colors.accent = egui::Color32::from_rgb(0x12, 0xAB, 0x9F);
    let night = suhina_ui::settings::themes::ThemeEntry {
        id: "night".into(),
        theme: theme::Theme { name: "Night".into(), colors },
    };
    let ctx = harness.ctx.clone();
    harness.state_mut().set_themes(&ctx, vec![night], Some("themes".into()));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Theme").click();
    harness.run();
    assert!(harness.query_by_label("Theme: Suhina Dark").is_some());
    harness.get_by_label("Theme: Night").click();
    harness.run();
    assert_eq!(theme::accent(), colors.accent);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.theme), Some("night".into()));

    // A copy to change becomes the theme in use, and is handed out to be written.
    harness.get_by_label("Duplicate theme").click();
    harness.run();
    let saved: Vec<_> = harness
        .state_mut()
        .take_theme_actions()
        .into_iter()
        .map(|a| match a {
            FileAction::Save { id, theme } => (id, theme.name),
            other => (format!("{other:?}"), String::new()),
        })
        .collect();
    assert_eq!(saved, [("night-copy".to_string(), "Night copy".to_string())]);
    assert_eq!(harness.state().settings().theme, "night-copy");
    for button in ["Reload themes", "Open the themes folder", "Back up themes", "Export the theme template"] {
        harness.get_by_label(button).click();
        harness.run();
    }
    assert_eq!(
        harness.state_mut().take_theme_actions(),
        [FileAction::Reload, FileAction::OpenFolder, FileAction::Backup, FileAction::ExportTemplate]
    );

    // A theme file dropped on the window is handed out to be brought in.
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile("dark blue.json".into())));
    harness.run();
    assert_eq!(harness.state_mut().take_theme_actions(), [FileAction::Import("dark blue.json".into())]);

    // A file saved elsewhere redraws the window.
    colors.accent = egui::Color32::from_rgb(1, 2, 3);
    let edited = suhina_ui::settings::themes::ThemeEntry {
        id: "night-copy".into(),
        theme: theme::Theme { name: "Night copy".into(), colors },
    };
    harness.state_mut().put_theme(&ctx, edited);
    assert_eq!(theme::accent(), colors.accent);
}

#[test]
fn settings_audio_chooses_the_input_device_and_saves_it() {
    let mut app = demo_app();
    app.projects_mut().set_audio_system(std::sync::Arc::new(StandAudio("Stand")));
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Audio").click();
    for _ in 0..3 {
        harness.step();
    }
    harness.get_by_label("Input: Off").click();
    harness.step();
    harness.step();
    harness.get_by_label("Stand In (2 in)").click();
    harness.step();
    harness.step();
    let chosen = harness.state().projects().output_choice().input.clone();
    assert_eq!(chosen, suhina_session::InputChoice::Named("Stand In".into()));
    let saved = harness.state_mut().take_changed_settings().map(|s| s.audio_input);
    assert_eq!(saved.as_deref(), Some("Stand In"));
}

#[test]
fn settings_audio_lists_the_midi_inputs_and_one_turned_off_is_saved() {
    let mut app = demo_app();
    app.projects_mut().set_audio_system(std::sync::Arc::new(StandAudio("Stand")));
    app.projects_mut().set_midi_system(std::sync::Arc::new(StandMidi(vec!["Keys In".into(), "Pads In".into()])));
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: MIDI").click();
    for _ in 0..3 {
        harness.step();
    }
    harness.get_by_label("MIDI input Pads In: Off").click();
    harness.step();
    harness.step();
    assert_eq!(harness.state().projects().midi_inputs_off(), ["Pads In"]);
    let saved = harness.state_mut().take_changed_settings().map(|s| s.midi_inputs_off);
    assert_eq!(saved, Some(vec!["Pads In".to_string()]));
}

#[test]
fn the_waveform_zoom_draws_every_audio_clip_larger_and_is_kept_as_a_setting() {
    let mut harness = arrangement_app();
    assert_eq!(harness.state().arrangement().waveform_zoom, 1.0);
    // One zoom for the whole Arrangement: the keys step it, the setting keeps it.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowUp);
    harness.run_steps(2);
    assert!((harness.state().arrangement().waveform_zoom - 1.25).abs() < 1e-6);
    assert_eq!(harness.state().settings().view.waveform_zoom, 125);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown);
    harness.run_steps(2);
    assert_eq!(harness.state().arrangement().waveform_zoom, 1.0, "never below the clips' own size");
    // The slider by the ruler, plain to see: a click at its right end zooms all the way.
    let slider = harness.get_by_label("Waveform zoom").rect();
    assert!(slider.width() >= 80.0 && slider.height() >= 10.0, "a slider to see and grab: {slider:?}");
    harness.hover_at(egui::pos2(slider.right() - 1.0, slider.center().y));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: egui::pos2(slider.right() - 1.0, slider.center().y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }
    harness.run_steps(2);
    let zoom = harness.state().arrangement().waveform_zoom;
    assert!(zoom > 12.0, "{zoom}");
    // The wheel over the slider steps it, and the view behind it stays where it is.
    let scroll = harness.state().arrangement().scroll;
    harness.hover_at(slider.center());
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(-1.0, -1.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    // The wheel is smoothed over frames: long enough for all of it to land.
    harness.run_steps(30);
    assert!((harness.state().arrangement().waveform_zoom - zoom / 1.25).abs() < 1e-4);
    assert_eq!(harness.state().arrangement().scroll, scroll, "the lanes did not scroll");
}

#[test]
fn settings_add_and_remove_plugin_folders_and_scan_in_the_background() {
    let base = crate::temp_dir::TempDir::new("ui-plugins");
    let folder = base.join("my plug-ins");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    let library = suhina_plugins::library::Library::new(Some(base.join("settings")), base.join("no-such-host"))
        .without_standard_folders();
    let mut app = App::with_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Plug-ins").click();
    harness.run();

    let field = harness.get_by_label("Plug-in folder to add");
    field.focus();
    field.type_text(&folder.display().to_string());
    harness.run();
    harness.get_by_label("Add plug-in folder").click();
    harness.run();
    harness.run();
    let remove = harness.query_all_by_label_contains("Remove ").count();
    assert_eq!(remove, 1, "the added folder is listed with a Remove button");

    harness.get_by_label("Scan").click();
    // The window keeps redrawing while the scan runs, so it is stepped, not run until still.
    harness.step();
    for _ in 0..200 {
        if !harness.state().scanning() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    match harness.state().status() {
        Status::Error(message) => assert!(message.contains("plug-in host"), "{message}"),
        other => panic!("expected the missing host to be reported, got {other:?}"),
    }

    harness.get_by_label_contains("Remove ").click();
    harness.run();
    harness.run();
    assert_eq!(harness.query_all_by_label_contains("Remove ").count(), 0);
}

#[test]
fn a_key_set_in_settings_runs_its_action_and_is_saved() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Shortcuts").click();
    harness.run();
    harness.get_by_label("Change keys of view.arrangement").click();
    harness.run();
    harness.key_press(Key::A);
    harness.run();
    assert_eq!(harness.state().keymap().binding_text("view.arrangement").as_deref(), Some("A"));
    assert!(harness.state_mut().take_changed_keymap().is_some());

    harness.get_by_label("Close settings").click();
    harness.run();
    harness.key_press(Key::A);
    harness.run();
    assert_eq!(harness.state().view(), View::Arrangement);
}

#[test]
fn settings_set_the_toolbar_size_and_the_mouse_switches() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Clip editor").click();
    harness.run();
    // The piano roll's row shows the size in use, Large by default.
    assert!(harness.query_by_label("Toolbar size: Large").is_some(), "the row reads its value");
    assert!(harness.query_by_label("Toolbar size: Normal").is_some());
    assert!(harness.query_by_label("Toolbar size: Larger").is_some());
    assert_eq!(harness.state().settings().toolbar_size, suhina_ui::settings::ToolbarSize::Large);
    harness.get_by_label("Toolbar size: Larger").click();
    harness.run();
    assert_eq!(harness.state().settings().toolbar_size, suhina_ui::settings::ToolbarSize::Larger);
    assert_eq!(
        harness.state_mut().take_changed_settings().map(|s| s.toolbar_size),
        Some(suhina_ui::settings::ToolbarSize::Larger)
    );

    // The workflow settings write to the keymap, which is handed out to be saved. They are the
    // page's last rows: the wheel scrolls them into view.
    for _ in 0..20 {
        harness.hover_at(egui::pos2(SIZE.0 / 2.0, SIZE.1 / 2.0));
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        harness.run();
    }
    assert!(
        harness.query_by_label("Right click on a note: Every tool").is_some(),
        "each setting is a row of its short choices"
    );
    assert!(!harness.state().keymap().mouse().right_deletes, "off until it is turned on");
    // Under the window's edge until the page is scrolled to it.
    harness.get_by_label("Right click on a note: Every tool").scroll_to_me();
    harness.run();
    harness.get_by_label("Right click on a note: Every tool").click();
    harness.run();
    assert!(harness.state().keymap().mouse().right_deletes);
    assert!(harness.state_mut().take_changed_keymap().is_some(), "the keymap is saved with it");
    assert_eq!(harness.state().keymap().preset(), suhina_ui::settings::keymap::Preset::Custom, "the mode is Custom");
    // And back again.
    harness.get_by_label("Right click on a note: Nothing").click();
    harness.run();
    assert!(!harness.state().keymap().mouse().right_deletes);
    // Every setting has its row.
    for row in [
        "Click on empty grid: Draws",
        "Shift+right drag: Splits",
        "Alt+right button: Plays",
        "Duplicate key: Ctrl+B",
        "Double-click on empty grid: Nothing",
        "Double-click on a note: Nothing",
    ] {
        assert!(harness.query_by_label(row).is_some(), "{row}");
    }
    // A preset chosen on the Shortcuts page turns all four mouse switches on.
    harness.get_by_label("Settings page: Shortcuts").click();
    harness.run();
    harness.get_by_label("Preset: Mouse-first").click();
    harness.run();
    assert!(harness.state().keymap().mouse().all_on());
    assert_eq!(
        harness.state_mut().take_changed_keymap().map(|k| k.mouse()),
        Some(suhina_ui::settings::keymap::MouseSettings::ALL_ON)
    );
    assert!(harness.query_by_label("Preset: Custom").is_some(), "Custom is one of the modes there too");
    harness.get_by_label("Settings page: Clip editor").click();
    harness.run();
    assert!(harness.query_by_label("Right click on a note: Every tool").is_some());
    harness.get_by_label("Close settings").click();
    harness.run();
}

/// The window with Settings › Audio open, on `host` as its only audio host. (`Result` so that a
/// failure is raised inside the test, where a panic is expected, and not in this helper.)
fn settings_audio_on(host: &'static str) -> Result<Harness<'static, App>, String> {
    let mut app = demo_app();
    app.projects_mut().set_audio_system(std::sync::Arc::new(StandAudio(host)));
    app.projects_mut().open_output(None).map_err(|e| e.message().to_owned())?;
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Audio").click();
    // The page redraws while it shows the latency, so it is stepped, not run until still.
    for _ in 0..3 {
        harness.step();
    }
    Ok(harness)
}

#[test]
fn settings_audio_lists_the_devices_and_a_buffer_picked_is_chosen_and_saved() {
    let mut harness = settings_audio_on("Stand").unwrap_or_else(|e| panic!("{e}"));
    assert!(harness.query_by_label("Driver: Default (Stand)").is_some());
    assert!(harness.query_by_label("Device: Default (Stand Out)").is_some());
    harness.get_by_label("Buffer size: Default (1024 frames)").click();
    harness.step();
    harness.step();
    harness.get_by_label("512 frames (10.7 ms)").click();
    harness.step();
    harness.step();
    assert_eq!(harness.state().projects().output_choice().buffer_frames, Some(512));
    let saved = harness.state_mut().take_changed_settings().map(|s| s.audio_buffer);
    assert_eq!(saved, Some(512));
    assert!(harness.query_by_label("Buffer size: 512 frames (10.7 ms)").is_some());
}

/// WASAPI shared mode plays the period Windows gives the device, so the Settings row is Suhina's
/// own engine block there: it never claims the device takes a size, whatever the driver does (an
/// Apollo's takes 2048 frames at most). Exclusive mode does ask the device, so the row is the
/// device's own buffer then.
#[test]
fn settings_audio_names_the_engine_block_on_wasapi_shared() {
    let mut harness = settings_audio_on("WASAPI").unwrap_or_else(|e| panic!("{e}"));
    assert!(harness.query_by_label("Engine block: Default (1024 frames)").is_some());
    assert!(harness.query_by_label("Buffer size: Default (1024 frames)").is_none());
    // Exclusive mode takes the device for Suhina alone and asks it for the size, so the row says
    // what the number is there.
    harness.get_by_label("Exclusive mode: On").click();
    for _ in 0..3 {
        harness.step();
    }
    assert!(harness.state().projects().output_choice().exclusive);
    assert!(harness.query_by_label("Buffer size: Default (1024 frames)").is_some());
    assert!(harness.query_by_label("Engine block: Default (1024 frames)").is_none());
}

/// A driver that declares no buffer range of its own keeps its own size (Suhina asks it for
/// none), so the Settings row is the engine's block there too, and the sizes on offer are
/// Suhina's own.
#[test]
fn settings_audio_names_the_engine_block_when_the_driver_declares_no_sizes() {
    let harness = settings_audio_on("ALSA").unwrap_or_else(|e| panic!("{e}"));
    assert!(harness.query_by_label("Engine block: Default (1024 frames)").is_some());
    assert!(harness.query_by_label("Buffer size: Default (1024 frames)").is_none());
}

#[test]
fn the_sample_map_opens_in_a_window_of_its_own_and_docks_back() {
    // The map lays itself out on a thread and asks for frames until it is done, which takes
    // longer than `run` waits on a slow machine: step a fixed number of frames instead.
    let mut harness = window(demo_app());
    // Hiding the browser for the test changed a setting already.
    let _ = harness.state_mut().take_changed_settings();
    assert!(harness.query_by_label("Dock").is_none());
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::M);
    harness.run_steps(4);
    // Without windows of their own (as here) the toolkit draws it as a window inside this one.
    assert!(harness.state().sample_map.view.window);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(true));
    assert!(harness.query_by_label("Dock").is_some(), "the map's bar offers to dock it");
    assert!(harness.query_by_label("Large").is_none(), "no large view while it has a window");
    harness.get_by_label("Dock").click();
    harness.run_steps(4);
    assert!(!harness.state().sample_map.view.window);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(false));
    assert!(harness.query_by_label("Dock").is_none());

    // The View menu does the same.
    harness.get_by_label("View").click();
    harness.run_steps(4);
    harness.get_by_label_contains("Sample map in a window").click();
    harness.run_steps(4);
    assert!(harness.state().sample_map.view.window);
}

#[test]
fn a_saved_map_window_opens_at_the_start() {
    let mut app = demo_app();
    let settings = suhina_ui::settings::Settings {
        map_window: true,
        map_window_rect: Some([100, 80, 700, 500]),
        browser_open: false,
        ..Default::default()
    };
    app.set_settings(&egui::Context::default(), settings);
    // The map is laid out on a thread of its own, which a slow machine takes longer than a
    // second of frames for: it is given up to ten seconds of real time to finish.
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let settle = |harness: &mut Harness<'static, App>| {
        for _ in 0..200 {
            if harness.try_run().is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    settle(&mut harness);
    assert!(harness.state().sample_map.view.window);
    assert!(harness.query_by_label("Dock").is_some());
    // The large view takes the map out of its window; the map's layout repaints on the way,
    // so the frames are stepped and not run to rest (a repaint each frame would out-run
    // `Harness::run`'s max steps).
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT, Key::M);
    harness.run_steps(3);
    assert!(!harness.state().sample_map.view.window);
    assert!(harness.state().sample_map.view.large);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(false));
}

#[test]
fn settings_open_as_a_dialog_over_the_view_and_escape_closes_them() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    assert!(harness.query_by_label("Close settings").is_some());
    // The view stays behind the dialog.
    assert!(harness.query_by_label("Turn Bass off").is_some(), "the view is still drawn");
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().settings_open());
}

/// The wheel and pinch over an open dialog reach only the dialog: the view behind it keeps its
/// scroll and zoom (#232).
#[test]
fn the_wheel_over_open_settings_leaves_the_arrangement_alone() {
    let mut harness = arrangement_app();
    let clip = |h: &Harness<'static, App>| h.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let before = clip(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    // Over the dialog's backdrop, where the clip is, and over the dialog itself.
    for at in [before.center(), egui::pos2(SIZE.0 / 2.0, SIZE.1 / 2.0)] {
        harness.hover_at(at);
        harness.run();
        for modifiers in [Modifiers::NONE, Modifiers::COMMAND, Modifiers::ALT] {
            harness.input_mut().events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-120.0, -120.0),
                phase: egui::TouchPhase::Move,
                modifiers,
            });
            harness.input_mut().events.push(egui::Event::Zoom(1.5));
            harness.run();
        }
    }
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().settings_open());
    assert_eq!(clip(&harness), before, "the arrangement neither scrolled nor zoomed");
}
