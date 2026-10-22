// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Settings window: each page scrolls to its last setting in a short window
//! (`docs/spec/ui-settings.md`).

use egui::{Key, Modifiers, Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::settings::settings_view::Page;
use suhina_ui::{App, kit::theme};

/// A window too short for most pages.
const SHORT: (f32, f32) = (1000.0, 420.0);

#[test]
fn meter_intensity_slider_saves_theme_without_changing_the_project() {
    use suhina_ui::settings::themes::FileAction;
    let mut harness = short_window();
    let before = harness.state().session().unwrap().project().clone();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    wheel_to(&mut harness, "Controls, meters and toolbar", 80);
    harness.get_by_label("Controls, meters and toolbar").click();
    settle(&mut harness);
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Meter intensity").scroll_to_me();
    settle(&mut harness);
    harness.state_mut().take_theme_actions();
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Meter intensity").click();
    settle(&mut harness);
    let actions = harness.state_mut().take_theme_actions();
    assert!(
        actions.iter().any(|action| matches!(action, FileAction::Save {theme, ..} if theme.skin.meter_intensity < 100)),
        "{actions:?}"
    );
    assert_eq!(harness.state().session().unwrap().project(), &before);
}

#[test]
fn compact_session_css_applies_to_a_png_skin_without_changing_artwork_or_project() {
    use suhina_ui::settings::themes::{FileAction, ThemeEntry};
    let mut harness = short_window();
    let ctx = harness.ctx.clone();
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNQ6nD5DwADrAHuJHH6EQAAAABJRU5ErkJggg==";
    let mut theme = theme::Theme { name: "PNG controls".into(), ..Default::default() };
    theme.skin.assets.insert("track.name".into(), png.into());
    harness.state_mut().put_theme(&ctx, ThemeEntry { id: "png-controls".into(), theme });
    harness.state_mut().use_theme(&ctx, "png-controls");
    let before = harness.state().session().unwrap().project().clone();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    for label in ["Controls, meters and toolbar", "Native CSS", "Load compact Session geometry"] {
        wheel_to(&mut harness, label, 80);
        harness.get_by_label(label).click();
        settle(&mut harness);
    }
    harness.state_mut().take_theme_actions();
    wheel_to(&mut harness, "Apply CSS", 80);
    harness.get_by_label("Apply CSS").click();
    settle(&mut harness);
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, FileAction::Save {theme, ..} if theme.skin.session.track_width == Some(86) && theme.skin.assets.get("track.name").is_some_and(|source| source == png))), "{actions:?}");
    assert_eq!(harness.state().session().unwrap().project(), &before);
}

#[test]
fn gradient_editor_enables_saves_and_resets_without_editing_the_project() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness =
        Harness::builder().with_size((1200.0, 1200.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    let before = harness.state().session().unwrap().project().clone();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    harness.get_by_label("Edit current theme…").click();
    harness.run();
    harness.get_by_label("Track gradient").click();
    harness.run();
    harness.get_by_label("Enable Track gradient").click();
    harness.run();
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.gradients.tracks.is_some() && theme::Theme::from_json(&theme.to_json()).0.gradients == theme.gradients)));
    assert_eq!(harness.state().session().unwrap().project(), &before);
    harness.get_by_label("Add Track colour stop").focus();
    harness.run();
    harness.get_by_label("Add Track colour stop").click();
    harness.run();
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.gradients.tracks.as_ref().is_some_and(|gradient| gradient.stops.len() == 3))));
    assert!(harness.query_by_label("Track stop 1 position").is_none());
    let stop = harness.get_by_label("Select Track stop 2").rect().center();
    harness.drag_at(stop);
    harness.hover_at(stop + Vec2::new(35.0, 0.0));
    harness.run();
    harness.drop_at(stop + Vec2::new(35.0, 0.0));
    harness.run();
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.gradients.tracks.as_ref().is_some_and(|gradient| gradient.stops.get(1).is_some_and(|stop| stop.position > 50)))));
    harness.get_by_label("Select Track stop 1").click();
    harness.run();
    assert!(harness.query_by_label("Track stop 2 position").is_none());
    assert!(harness.query_all_by_label("Track stop 1 position").next().is_some());
    harness.get_by_label("Select Track stop 2").click();
    harness.run();
    harness.get_by_label("Remove stop 2").focus();
    harness.run();
    harness.get_by_label("Remove stop 2").click();
    harness.run();
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.gradients.tracks.as_ref().is_some_and(|gradient| gradient.stops.len() == 2))));
    harness.get_by_label("Reset changes").focus();
    harness.run();
    harness.get_by_label("Reset changes").click();
    harness.run();
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.gradients == Default::default())));
    assert_eq!(harness.state().session().unwrap().project(), &before);
}

#[test]
fn theme_default_edits_preserve_existing_project_colours_and_opacity_saves() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder().with_size((1200.0, 900.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    let before = harness.state().session().unwrap().project().clone();
    let mut custom = theme::Theme { name: "New defaults".into(), ..Default::default() };
    custom.colors.midi_clip_default = egui::Color32::from_rgb(20, 30, 40);
    custom.colors.audio_clip_default = egui::Color32::from_rgb(50, 60, 70);
    let context = harness.ctx.clone();
    harness
        .state_mut()
        .put_theme(&context, suhina_ui::settings::themes::ThemeEntry { id: "new-defaults".into(), theme: custom });
    harness.state_mut().use_theme(&context, "new-defaults");
    harness.run();
    assert_eq!(harness.state().session().unwrap().project(), &before);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    harness.get_by_label("Edit current theme…").click();
    harness.run();
    harness.state_mut().take_theme_actions();
    let opacity = harness.get_by_label("Opacity grid.major").rect();
    let pointer = opacity.left_center() + Vec2::new(48.0, 0.0);
    harness.hover_at(pointer);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    assert!(theme::palette().grid_major.a() < 255);
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.colors.grid_major.a() < 255 && theme::Theme::from_json(&theme.to_json()).0.colors == theme.colors)));
    assert_eq!(harness.state().session().unwrap().project(), &before);
}

#[test]
#[ignore = "renders GPU review images; run explicitly after changing the theme UI"]
fn render_themes_gallery_and_editor_for_review() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness =
        Harness::builder().with_size((1200.0, 900.0)).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    harness.get_by_label("Theme: Suhina Studio Light").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/theme-gallery-review");
    std::fs::create_dir_all(&folder).unwrap();
    harness.render().unwrap().save(folder.join("themes.png")).unwrap();
    harness.get_by_label("Edit current theme…").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    harness.render().unwrap().save(folder.join("editor.png")).unwrap();
    harness.get_by_label("Track gradient").click();
    harness.run();
    harness.get_by_label("Enable Track gradient").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    harness.render().unwrap().save(folder.join("gradient-editor.png")).unwrap();
    harness.get_by_label("Track stop 1 colour").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    harness.render().unwrap().save(folder.join("colour-popup.png")).unwrap();
}

#[test]
fn theme_cards_keep_the_open_projects_clip_colour() {
    let mut project = demo_project();
    let source = suhina_engine::model::Rgb(35, 150, 125);
    project.arrangement.lanes[0].clips[0].color = Some(source);
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    let mut harness = Harness::builder().with_size((1200.0, 900.0)).build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    for name in ["Suhina Studio Light", "Suhina Dark"] {
        harness.get_by_label(&format!("Theme: {name}")).click();
        harness.run();
        let card = harness.get_by_label(&format!("Theme: {name}")).rect();
        let expected = suhina_ui::kit::motion::mix(
            theme::panel_background(),
            egui::Color32::from_rgb(source.0, source.1, source.2),
            0.82,
        );
        assert!(
            harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(shape) if card.contains_rect(shape.rect) && shape.fill == expected)),
            "{name} must preview the project's explicit clip colour on its own surface"
        );
    }
}

#[test]
fn active_theme_is_featured_and_holding_it_keeps_the_preview_clear() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    let id = harness.state().settings().theme.clone();
    let name = suhina_ui::kit::theme::built_in_themes().into_iter().find(|(key, _)| *key == id).unwrap().1.name;
    let rect = harness.get_by_label(&format!("Theme: {name}")).rect();
    assert!(rect.width() > 500.0 && rect.height() > 200.0);
    let pointer = rect.center_top() + Vec2::new(0.0, 20.0);
    harness.hover_at(pointer);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..6 {
        harness.step();
        assert!(!dimmed(&harness));
    }
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    assert!(dimmed(&harness));
    assert_eq!(harness.state().settings().theme, id);
}

#[test]
fn restoring_a_built_in_waits_for_the_save_choice_and_can_be_cancelled() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    let original = harness.state().settings().theme.clone();
    harness.get_by_label("Restore built-in…").click();
    settle(&mut harness);
    harness.get_by_label("Restore Nord").click();
    settle(&mut harness);
    assert_eq!(harness.state().settings().theme, original);
    harness.get_by_label("Cancel restore").click();
    settle(&mut harness);
    assert_eq!(harness.state().settings().theme, original);
    harness.get_by_label("Restore built-in…").click();
    settle(&mut harness);
    harness.get_by_label("Restore Nord").click();
    settle(&mut harness);
    harness.get_by_label("Save a copy & restore").click();
    settle(&mut harness);
    assert_eq!(harness.state().settings().theme, "nord");
    assert!(harness.get_by_label("Theme: Nord").rect().width() > 500.0);
    harness.get_by_label(&format!(
        "Theme: {name} copy",
        name = suhina_ui::kit::theme::built_in_themes().into_iter().find(|(id, _)| *id == original).unwrap().1.name
    ));
}

#[test]
fn theme_liquid_overrides_save_without_changing_global_settings() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    let global = harness.state().settings().view.liquid;
    harness.get_by_label("Use global liquid settings").click();
    settle(&mut harness);
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.liquid == Some(global))));
    assert_eq!(harness.state().settings().view.liquid, global);
    harness.query_all_by_label("Theme panel liquid").next().unwrap().click();
    settle(&mut harness);
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.liquid.is_some_and(|value| value.panels > global.panels))));
    harness.get_by_label("Use global liquid settings").click();
    settle(&mut harness);
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(
        |action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.liquid.is_none())
    ));
    assert_eq!(harness.state().settings().view.liquid, global);
    assert!(harness.query_by_label("Colour track.1").is_none());
    wheel_to(&mut harness, "Colour clip.midiDefault", 40);
    harness.get_by_label("Colour clip.midiDefault");
    harness.get_by_label("Colour clip.audioDefault");
}

#[test]
fn colour_explanations_wrap_below_the_token_instead_of_overlapping_it() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    let purpose = theme::palette().tokens().into_iter().find(|(key, _, _)| *key == "window.background").unwrap().1;
    wheel_to(&mut harness, purpose, 40);
    let token = harness.get_by_label("window.background").rect();
    let description = harness.get_by_label(purpose).rect();
    assert!(description.top() >= token.bottom(), "{token:?} overlaps {description:?}");
    assert!(description.width() <= 560.0, "description escapes the editor width");
}

#[test]
fn themes_gallery_switches_immediately_and_opens_a_separate_editor() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Colour grid.major").is_none());
    wheel_to(&mut harness, "Theme: Suhina Studio Light", 40);
    harness.get_by_label("Theme: Suhina Studio Light").click();
    settle(&mut harness);
    assert_eq!(harness.state().settings().theme, "suhina-studio-light");
    let palette = theme::palette();
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    harness.get_by_label("Colour grid.major");
    assert!(harness.query_by_label("Theme: Suhina Studio Light").is_none());
    assert_eq!(theme::palette(), palette, "opening the editor must not change the theme");
    harness.get_by_label("Back to themes").click();
    settle(&mut harness);
    harness.get_by_label("Theme: Suhina Studio Light");
}

#[test]
fn colour_picker_keeps_preview_clear_while_held_and_reset_restores_the_palette() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    let original = theme::palette();
    wheel_to(&mut harness, "Colour grid.major", 40);
    harness.get_by_label("Colour grid.major").click();
    settle(&mut harness);
    let square = harness
        .output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) => {
                let rect = mesh.calc_bounds();
                (rect.width() > 250.0 && rect.width() < 300.0 && (rect.width() - rect.height()).abs() < 1.0)
                    .then_some(rect)
            }
            _ => None,
        })
        .expect("the actual colour picker's saturation/value square");
    assert!(
        harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(background) if background.fill.is_opaque()
            && background.rect.contains_rect(square)
            && background.rect.top() < square.top() - 20.0)),
        "colour picker needs an opaque frame covering its header and colour square"
    );
    let pointer = square.center();
    harness.hover_at(pointer);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..6 {
        harness.step();
    }
    assert_ne!(theme::palette().grid_major, original.grid_major);
    assert!(!dimmed(&harness), "stationary held colour picker must keep the background clear");
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    assert!(dimmed(&harness), "releasing the colour picker must restore the dimmer immediately");
    egui::Popup::close_all(&harness.ctx);
    harness.step();
    wheel_to(&mut harness, "Reset changes", 40);
    harness.get_by_label("Reset changes").click();
    settle(&mut harness);
    assert_eq!(theme::palette(), original);
}

fn dimmed(harness: &Harness<'static, App>) -> bool {
    let tint = theme::window_background().gamma_multiply(0.6);
    harness.output().shapes.iter().any(|shape| {
        matches!(&shape.shape,
        egui::Shape::Rect(shape) if shape.fill == tint && shape.rect.width() > 900.0)
    })
}

#[test]
fn embedded_settings_can_be_moved_by_its_title() {
    let mut harness = short_window();
    let before = harness.get_by_label("Move Settings").rect();
    let from = before.center();
    harness.hover_at(from);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    let to = from + Vec2::new(60.0, 0.0);
    harness.hover_at(to);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    let after = harness.get_by_label("Move Settings").rect();
    assert!((after.left() - before.left() - 60.0).abs() < 1.0, "{before:?} -> {after:?}");
    assert!(dimmed(&harness), "moving the title must not trigger a value preview");
}

#[test]
fn dragging_the_modal_background_does_not_toggle_live_preview() {
    let mut harness = short_window();
    let from = Pos2::new(10.0, 100.0);
    harness.hover_at(from);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.hover_at(Pos2::new(40.0, 125.0));
    harness.step();
    assert!(dimmed(&harness), "a background drag unexpectedly hid the dimmer");
}

#[test]
fn holding_a_real_slider_keeps_the_live_preview_visible() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("Track header colour strength");
    settle(&mut harness);
    let rect = harness.get_by_label("Track header colour strength").rect();
    let pointer = rect.center();
    harness.hover_at(pointer);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..6 {
        harness.step();
    }
    assert!(!dimmed(&harness), "held slider lost its preview");
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    assert!(dimmed(&harness), "dimmer must return on the release frame without a delay");
}

#[test]
fn settings_can_undock_and_dock_without_losing_the_page() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Appearance").click();
    settle(&mut harness);
    harness.get_by_label("Open in window").click();
    settle(&mut harness);
    assert!(harness.state().settings_open());
    harness.get_by_label("Track liquid");
    harness.get_by_label("Dock").click();
    settle(&mut harness);
    harness.get_by_label("Open in window");
    harness.get_by_label("Track liquid");
    harness.get_by_label("Close settings").click();
    settle(&mut harness);
    assert!(!harness.state().settings_open());
}

#[test]
fn liquid_sliders_change_independent_saved_strengths_without_restarting() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size((1200.0, 850.0))
        .with_step_dt(0.25)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Appearance").click();
    harness.run();
    harness.get_by_label("Panel liquid").click();
    harness.run();
    let panels = harness.state().settings().view.liquid.panels;
    assert!(panels > 0);
    assert_eq!(harness.state().settings().view.liquid.tracks, 0);
    assert_eq!(harness.state().settings().view.liquid.clips, 0);
    harness.get_by_label("Track liquid").click();
    harness.run();
    let tracks = harness.state().settings().view.liquid.tracks;
    assert!(tracks > 0);
    assert_eq!(harness.state().settings().view.liquid.panels, panels);
    assert_eq!(harness.state().settings().view.liquid.clips, 0);
    harness.get_by_label("Clip liquid").click();
    harness.run();
    let changed = harness.state_mut().take_changed_settings().expect("slider persists changed settings");
    assert!(changed.view.liquid.clips > 0);
    assert_eq!(changed.view.liquid.panels, panels);
    assert_eq!(changed.view.liquid.tracks, tracks);
    let (read, warnings) = suhina_ui::settings::Settings::from_text(&changed.to_text());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(read.view.liquid, changed.view.liquid);
}

#[test]
fn export_permissions_are_added_by_picker_removed_and_saved() {
    use suhina_ui::interaction::pick::PickField;
    let mut harness = short_window();
    harness.get_by_label("Settings page: Export").click();
    harness.run();
    harness.get_by_label("Add MCP write folder").click();
    harness.run();
    assert_eq!(harness.state_mut().take_pick_request().map(|r| r.field), Some(PickField::McpWriteFolder));
    let folder = std::env::temp_dir().join("Suhina exports");
    harness.state_mut().set_picked(PickField::McpWriteFolder, folder.clone());
    harness.run();
    let settings = harness.state().settings().clone();
    assert_eq!(settings.mcp_write_folders, vec![folder.display().to_string()]);
    assert_eq!(suhina_ui::settings::Settings::from_text(&settings.to_text()).0, settings);
    harness.get_by_label(&format!("Remove MCP write folder {}", folder.display())).click();
    harness.run();
    assert!(harness.state().settings().mcp_write_folders.is_empty());
    harness.get_by_label("Allow the project's folder: Off").click();
    harness.run();
    assert!(!harness.state().settings().mcp_project_folder);
}

/// A small-window visual check with a long folder name; run with --ignored.
#[test]
#[ignore = "writes a settings screenshot for visual inspection; needs a GPU"]
fn export_permissions_snapshot() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Export").click();
    harness.run();
    harness.state_mut().set_picked(
        suhina_ui::interaction::pick::PickField::McpWriteFolder,
        std::path::PathBuf::from("C:/Music/Projects/Export destinations/Albums and arrangements/Final mixes"),
    );
    harness.run();
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap();
    harness.render().unwrap().save(folder.join("export-permissions.png")).unwrap();
}

fn short_window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SHORT)
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness
}

/// Runs frames until the window rests; the Audio page repaints all the time, so it never does.
fn settle(harness: &mut Harness<'static, App>) {
    let _ = harness.run_ok();
}

/// Whether the control named `label` is inside the window, under the Settings title.
fn shown(harness: &Harness<'static, App>, label: &str) -> bool {
    let title = harness.get_by_label("Close settings").rect();
    let rect = harness.get_by_label(label).rect();
    rect.top() > title.bottom() && rect.bottom() < SHORT.1
}

/// Turns the wheel over the page until `label` shows, at most `turns` times; returns how often.
fn wheel_to(harness: &mut Harness<'static, App>, label: &str, turns: usize) -> usize {
    let title = harness.get_by_label("Close settings").rect();
    // The middle of the page, right of the list of pages.
    let over = Pos2::new(title.right() - 200.0, (title.bottom() + SHORT.1) / 2.0);
    for turn in 0..turns {
        if shown(harness, label) {
            return turn;
        }
        harness.hover_at(over);
        harness.step();
        let delta = if harness.get_by_label(label).rect().top() < title.bottom() { 80.0 } else { -80.0 };
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, delta),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        settle(harness);
    }
    turns
}

#[test]
fn every_page_scrolls_to_its_last_setting_in_a_short_window() {
    let mut harness = short_window();
    // Each page, and the control at its end (MIDI has none with no controller connected).
    let pages = [
        ("General", Some("Warp mode: Re-pitch")),
        ("Export", Some("Add MCP write folder")),
        ("Audio", Some("List the audio devices again")),
        ("MIDI", None),
        ("Plug-ins", Some("Open the plug-in manager")),
        ("Arrangement", Some("Large height")),
        ("Session and mixer", Some("Mixer controls: Knob")),
        ("Clip editor", Some("Double-click on a note: Nothing")),
        ("Transport", Some("Stop: Stay")),
        ("Detail area", Some("Detail area layout: Full width")),
        ("Browser", Some("Tag limit: 1024")),
        ("Appearance", Some("Renderer: OpenGL")),
        ("Themes", Some("Edit current theme…")),
        ("Shortcuts", Some("Change keys of file.new")),
    ];
    let names: Vec<&str> = Page::ALL.iter().map(|(_, name)| *name).collect();
    assert_eq!(pages.map(|(page, _)| page).to_vec(), names, "every page is checked");
    for (page, last) in pages {
        harness.get_by_label(&format!("Settings page: {page}")).click();
        settle(&mut harness);
        let Some(last) = last else { continue };
        let turns = wheel_to(&mut harness, last, 40);
        assert!(shown(&harness, last), "{page}: {last} stays out of view");
        assert!(turns < 40, "{page}: the wheel did not reach {last}");
    }
}

#[test]
fn the_general_page_needs_the_wheel_and_another_page_opens_at_its_top() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: General").click();
    harness.run();
    let last = "Warp mode: Re-pitch";
    assert!(!shown(&harness, last), "the short window shows the whole General page");
    assert!(wheel_to(&mut harness, last, 40) > 0);
    // Scrolled down, the page's first setting is out of view; another page starts at its top.
    assert!(!shown(&harness, "When opening a project: Ask"));
    harness.get_by_label("Settings page: Browser").click();
    harness.run();
    assert!(shown(&harness, "Browser side: Left"));
    harness.get_by_label("Settings page: General").click();
    harness.run();
    assert!(shown(&harness, "When opening a project: Ask"));
}

#[test]
fn a_control_given_the_focus_is_scrolled_into_view() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: General").click();
    harness.run();
    let last = "Warp mode: Re-pitch";
    assert!(!shown(&harness, last));
    harness.get_by_label(last).focus();
    harness.run();
    assert!(shown(&harness, last), "the focused control stays out of view");
}

#[test]
fn a_search_shows_the_settings_that_match_from_every_page() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("preview");
    settle(&mut harness);
    // Previews in the browser and on the map, from one page; settings that do not match are gone.
    assert!(harness.query_by_label("Auto-play: On").is_some());
    assert!(harness.query_by_label("Map preview on hover: On").is_some());
    assert!(harness.query_by_label("Browser side: Left").is_none());
    assert!(harness.query_by_label("When opening a project: Ask").is_none());
    // A change made in the results is made as on its page.
    harness.get_by_label("Auto-play: Off").click();
    settle(&mut harness);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.preview_autoplay), Some(false));

    // Words match anywhere: the page and the section count too.
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Search settings").type_text("transport stay");
    settle(&mut harness);
    assert!(harness.query_by_label("Stop: Stay").is_some());
    assert!(harness.query_by_label("Play from: Marker").is_none(), "only Stop has both words (Stay is its note)");

    // Nothing matches.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Search settings").type_text("nothing like this");
    settle(&mut harness);
    assert!(harness.query_by_label("Stop: Stay").is_none());

    // A page clicked empties the search and shows the page.
    harness.get_by_label("Settings page: Transport").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Play from: Marker").is_some());
    assert_eq!(harness.get_by_label("Search settings").value().as_deref(), Some(""));
    assert!(harness.state().settings_open());
}

#[test]
fn escape_in_the_search_empties_it_and_keeps_settings_open() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("zoom");
    settle(&mut harness);
    assert!(harness.query_by_label("Zoom: +").is_some());
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().settings_open(), "Escape in the search closed Settings");
    assert!(harness.query_by_label("Zoom: +").is_none(), "the General page shows again");
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(!harness.state().settings_open());
}

/// The search finds single shortcuts, and the Shortcuts page filters its own list by action,
/// group or key.
#[test]
fn a_search_finds_shortcuts_and_the_shortcuts_page_filters_its_list() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("waveform");
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of view.waveformZoomIn").is_some(), "the action is found");
    assert!(harness.query_by_label("Change keys of file.save").is_none(), "the rest are not listed");

    // On the page, its own filter: by the keys too.
    harness.get_by_label("Settings page: Shortcuts").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of file.save").is_some(), "the whole list without a filter");
    harness.get_by_label("Filter shortcuts").focus();
    settle(&mut harness);
    harness.get_by_label("Filter shortcuts").type_text("ctrl+alt+up");
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of view.waveformZoomIn").is_some());
    assert!(harness.query_by_label("Change keys of file.save").is_none());
}

#[test]
fn holding_background_blur_keeps_the_live_preview_visible() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("Background blur");
    settle(&mut harness);
    let rect = harness.get_by_label("Background blur").rect();
    let pointer = rect.center();
    harness.hover_at(pointer);
    harness.step();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    for _ in 0..6 {
        harness.step();
    }
    assert!(!dimmed(&harness), "held slider lost its preview");
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    assert!(dimmed(&harness), "dimmer must return on the release frame without a delay");
}

#[test]
fn theme_override_disables_global_liquid_sliders_until_inheritance_returns() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    harness.get_by_label("Use global liquid settings").click();
    settle(&mut harness);
    let global = harness.state().settings().view.liquid;
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("Panel liquid");
    settle(&mut harness);
    assert!(harness.get_by_label("Panel liquid").accesskit_node().is_disabled());
    assert_eq!(harness.state().settings().view.liquid, global);
    harness.get_by_label("Search settings").focus();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.key_press(Key::Backspace);
    settle(&mut harness);
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Use global liquid settings").click();
    settle(&mut harness);
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("Panel liquid");
    settle(&mut harness);
    assert!(!harness.get_by_label("Panel liquid").accesskit_node().is_disabled());
    assert_eq!(harness.state().settings().view.liquid, global);
}
