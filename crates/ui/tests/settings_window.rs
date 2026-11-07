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
    for label in ["Controls, meters and toolbar", "Native CSS", "Load compact Launcher geometry"] {
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
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((1200.0, 1200.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    let before = harness.state().session().unwrap().project().clone();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    harness.get_by_label("Edit current theme…").click();
    harness.run();
    wheel_to(&mut harness, "Track gradient", 40);
    harness.get_by_label("Track gradient").click();
    harness.run();
    wheel_to(&mut harness, "Enable Track gradient", 40);
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
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((1200.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
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
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((1200.0, 900.0))
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    harness.get_by_label("Theme: Suhina Light").click();
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
    wheel_to(&mut harness, "Track gradient", 40);
    harness.get_by_label("Track gradient").click();
    harness.run();
    wheel_to(&mut harness, "Enable Track gradient", 40);
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

/// A swatch is a rounded rectangle its colour fills to the edge, larger on the large card, and
/// a card without variants is as high as one with them.
#[test]
fn variant_swatches_fill_their_rectangle_and_cards_are_one_height() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size((1200.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    let sheet = |accent: &str| format!(":root {{ --accent: {accent}; --panel-background: #203040; }}");
    let (ink, warnings) = theme::from_folder(theme::ThemeMeta::new("Ink", "#112233"), &sheet("#123456"));
    assert!(warnings.is_empty(), "{warnings:?}");
    let (plain, warnings) = theme::from_folder(theme::ThemeMeta::new("Plain", "#445566"), &sheet("#654321"));
    assert!(warnings.is_empty(), "{warnings:?}");
    let dim = suhina_ui::kit::theme_vars::Variant { id: "dim".into(), name: "Dim".into(), values: Default::default() };
    let (ink_dim, _) = suhina_ui::kit::theme_vars::with_variant(&ink, Some(&dim));
    use suhina_ui::settings::themes::ThemeEntry;
    let entries = vec![
        ThemeEntry { id: "ink".into(), theme: ink },
        ThemeEntry { id: "ink@dim".into(), theme: ink_dim },
        ThemeEntry { id: "plain".into(), theme: plain },
    ];
    let ctx = harness.ctx.clone();
    harness.state_mut().set_themes(&ctx, entries, Some("themes".into()));
    harness.state_mut().use_theme(&ctx, "ink");
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    let swatches: Vec<egui::Rect> = harness.get_all_by_label("Variant: Dim").map(|node| node.rect()).collect();
    assert_eq!(swatches.len(), 2, "on the large card and on its card in the list");
    let (large, small) = (swatches[0], swatches[1]);
    assert!(large.width() > small.width() && large.height() > small.height(), "{large:?} {small:?}");
    assert!(
        small.width() > small.height() && small.height() >= 20.0,
        "a rectangle, much larger than before: {small:?}"
    );
    for swatch in [large, small] {
        assert!(
            harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(fill) if fill.fill != egui::Color32::TRANSPARENT
                    && fill.rect == swatch
                    && fill.corner_radius.nw > 0)),
            "a rounded fill covers the whole swatch {swatch:?}"
        );
    }
    let box_height = |h: &Harness<'static, App>, label: &str| {
        let card = h.get_by_label(label).rect();
        h.output()
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(outline)
                    if outline.stroke.width > 0.0
                        && outline.rect.min == card.min
                        && outline.rect.width() == card.width()
                        && outline.rect.height() > card.height() =>
                {
                    Some(outline.rect.height())
                }
                _ => None,
            })
            .next()
            .unwrap_or_else(|| panic!("{label} has no outline"))
    };
    assert_eq!(box_height(&harness, "Theme: Ink"), box_height(&harness, "Theme: Plain"));
}

/// However many variants a theme has, all their swatches stay on one line inside its card.
#[test]
fn many_variant_swatches_narrow_to_fit_on_one_line() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size((1200.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    use suhina_ui::kit::theme_vars::{Variant, with_variant};
    use suhina_ui::settings::themes::ThemeEntry;
    let (ink, warnings) = theme::from_folder(theme::ThemeMeta::new("Ink", "#112233"), ":root {}");
    assert!(warnings.is_empty(), "{warnings:?}");
    for count in [12, 80] {
        let mut entries = vec![ThemeEntry { id: "ink".into(), theme: ink.clone() }];
        for index in 0..count {
            let variant = Variant { id: format!("v{index}"), name: format!("V{index}"), values: Default::default() };
            let (chosen, _) = with_variant(&ink, Some(&variant));
            entries.push(ThemeEntry { id: format!("ink@v{index}"), theme: chosen });
        }
        let ctx = harness.ctx.clone();
        harness.state_mut().set_themes(&ctx, entries, Some("themes".into()));
        harness.state_mut().use_theme(&ctx, "ink");
        harness.get_by_label("Settings page: Themes").click();
        harness.run();
        for label in ["Theme in use: Ink", "Theme: Ink"] {
            let card = harness.get_by_label(label).rect();
            // On this card: its own swatch and one for each variant, in the row under its name.
            let row: Vec<egui::Rect> = (0..count)
                .map(|index| format!("Variant: V{index}"))
                .chain(["Variant: Default".to_owned()])
                .map(|name| {
                    harness
                        .get_all_by_label(&name)
                        .map(|node| node.rect())
                        .find(|rect| (rect.top() - card.bottom()).abs() < 1.0)
                        .unwrap_or_else(|| panic!("{label}: no {name} in its row"))
                })
                .collect();
            for swatch in &row {
                assert!(swatch.left() >= card.left() && swatch.right() <= card.right() + 0.01, "{label} {swatch:?}");
                assert_eq!(swatch.top(), row[0].top(), "on one line");
                assert!((swatch.width() - row[0].width()).abs() < 0.01, "one width");
            }
            for (a, b) in row[..count].iter().zip(&row[1..count]) {
                assert!(a.right() <= b.left() + 0.01, "{a:?} overlaps {b:?}");
            }
        }
    }
}

#[test]
fn theme_cards_show_a_thumbnail_or_a_card_in_the_swatch_colour() {
    let (mut plain, warnings) = theme::from_folder(theme::ThemeMeta::new("Plain", "#123456"), ":root {}");
    assert!(warnings.is_empty(), "{warnings:?}");
    plain.thumbnail = None;
    let mut app = App::new();
    let ctx = egui::Context::default();
    app.set_themes(&ctx, vec![suhina_ui::settings::themes::ThemeEntry { id: "plain".into(), theme: plain }], None);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size((1200.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    // A theme without a thumbnail is a blank card filled with its swatch colour.
    let card = harness.get_by_label("Theme: Plain").rect();
    let swatch = egui::Color32::from_rgb(0x12, 0x34, 0x56);
    assert!(
        harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if card.contains_rect(rect.rect) && rect.fill == swatch && rect.rect.width() > 100.0)),
        "Plain's card is filled with its swatch"
    );
    // A built-in theme's card shows its thumbnail, an SVG drawn with the theme's own values.
    let card = harness.get_by_label("Theme in use: Suhina Studio").rect();
    assert!(
        harness.output().shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Rect(rect) if card.contains_rect(rect.rect) && rect.fill_texture_id() != egui::TextureId::default())),
        "Suhina Studio's card shows its thumbnail"
    );
}

#[test]
fn active_theme_is_featured_and_holding_it_keeps_the_preview_clear() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    let id = harness.state().settings().theme.clone();
    let name = suhina_ui::kit::theme::built_in_themes().into_iter().find(|(key, _)| *key == id).unwrap().1.name;
    let rect = harness.get_by_label(&format!("Theme in use: {name}")).rect();
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
    assert!(harness.get_by_label("Theme in use: Nord").rect().width() > 500.0);
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
    wheel_to(&mut harness, "Use global liquid settings", 40);
    harness.get_by_label("Use global liquid settings").click();
    settle(&mut harness);
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.liquid == Some(global))));
    assert_eq!(harness.state().settings().view.liquid, global);
    harness.query_all_by_label("Theme panel liquid").next().unwrap().click();
    settle(&mut harness);
    let actions = harness.state_mut().take_theme_actions();
    assert!(actions.iter().any(|action| matches!(action, suhina_ui::settings::themes::FileAction::Save { theme, .. } if theme.liquid.is_some_and(|value| value.panels > global.panels))));
    wheel_to(&mut harness, "Use global liquid settings", 40);
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
    wheel_to(&mut harness, "Theme: Suhina Light", 40);
    harness.get_by_label("Theme: Suhina Light").click();
    settle(&mut harness);
    assert_eq!(harness.state().settings().theme, "suhina-light");
    let palette = theme::palette();
    // Choosing it left the page where it was: the card clicked is still in view.
    assert!(harness.get_by_label("Theme: Suhina Light").rect().is_positive());
    wheel_to(&mut harness, "Edit current theme…", 40);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    harness.get_by_label("Colour grid.major");
    assert!(harness.query_by_label("Theme: Suhina Light").is_none());
    assert_eq!(theme::palette(), palette, "opening the editor must not change the theme");
    harness.get_by_label("Back to themes").click();
    settle(&mut harness);
    harness.get_by_label("Theme: Suhina Light");
}

#[test]
fn colour_picker_keeps_preview_clear_while_held_and_reset_restores_the_palette() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    let ctx = harness.ctx.clone();
    harness.state_mut().use_theme(&ctx, theme::BUILT_IN);
    settle(&mut harness);
    let original = theme::palette();
    wheel_to(&mut harness, "Colour grid.major", 40);
    harness.get_by_label("Colour grid.major").click();
    settle(&mut harness);
    let square = harness
        .output()
        .shapes
        .iter()
        // egui paints the square as a rectangle filled with a gradient texture.
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(square) if square.brush.is_some() => {
                let rect = square.rect;
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

/// Choosing a theme leaves the window behind dimmed on every frame, so it does not flash clear.
#[test]
fn choosing_a_theme_keeps_the_window_behind_dimmed() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    wheel_to(&mut harness, "Theme: Nord", 40);
    let card = harness.get_by_label("Theme: Nord").rect().center();
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    harness.hover_at(card);
    harness.step();
    assert!(dimmed(&harness), "dimmed before the press");
    harness.input_mut().events.push(button(card, true));
    harness.step();
    assert!(dimmed(&harness), "dimmed on the press");
    harness.input_mut().events.push(button(card, false));
    for frame in 0..6 {
        harness.step();
        assert!(dimmed(&harness), "dimmed on frame {frame} after the click");
    }
    assert!(harness.get_by_label("Theme in use: Nord").rect().is_positive());
    theme::set_palette(&harness.ctx, theme::Palette::DARK);
}

/// The window behind stays clear from press to release, also while the pointer rests, not only
/// on the frames where the setting's value changes.
#[test]
fn holding_a_themes_own_setting_keeps_the_window_clear_until_release() {
    let mut harness = short_window();
    let sheet = "@property --glow { syntax: \"<integer>\"; initial-value: 50; label: \"Glow\"; range: 0 100; }\n\
                 @property --tint { syntax: \"<color>\"; initial-value: #336699; label: \"Tint\"; }\n\
                 :root { --accent: var(--tint); }";
    let (ink, warnings) = theme::from_folder(theme::ThemeMeta::new("Ink", theme::DEFAULT_SWATCH), sheet);
    assert!(warnings.is_empty(), "{warnings:?}");
    let entries = vec![suhina_ui::settings::themes::ThemeEntry { id: "ink".into(), theme: ink }];
    let ctx = harness.ctx.clone();
    harness.state_mut().set_themes(&ctx, entries, Some("themes".into()));
    harness.state_mut().use_theme(&ctx, "ink");
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };

    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Glow").scroll_to_me();
    settle(&mut harness);
    let slider = harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Glow").rect();
    let start = slider.center();
    harness.hover_at(start);
    harness.step();
    assert!(dimmed(&harness), "dimmed before the press");
    harness.input_mut().events.push(button(start, true));
    harness.step();
    assert!(!dimmed(&harness), "clear on the press");
    let end = start + Vec2::new(40.0, 0.0);
    harness.hover_at(end);
    harness.step();
    assert!(!dimmed(&harness), "clear while it moves");
    for _ in 0..6 {
        harness.step();
        assert!(!dimmed(&harness), "clear while the pointer rests");
    }
    harness.input_mut().events.push(button(end, false));
    harness.step();
    assert!(dimmed(&harness), "dimmed again on the release");
    settle(&mut harness);

    harness.get_by_role_and_label(egui::accesskit::Role::ColorWell, "Tint").click();
    settle(&mut harness);
    let square = harness
        .output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(square) if square.brush.is_some() => {
                let rect = square.rect;
                (rect.width() > 250.0 && rect.width() < 300.0 && (rect.width() - rect.height()).abs() < 1.0)
                    .then_some(rect)
            }
            _ => None,
        })
        .expect("the colour picker's square");
    let pointer = square.center();
    harness.hover_at(pointer);
    harness.step();
    harness.input_mut().events.push(button(pointer, true));
    for _ in 0..6 {
        harness.step();
        assert!(!dimmed(&harness), "clear while the colour is held");
    }
    harness.input_mut().events.push(button(pointer, false));
    harness.step();
    assert!(dimmed(&harness), "dimmed again when the colour is let go");

    // The colours that follow the setting follow it every time, not only the first.
    let first = theme::accent();
    for (fraction, x) in [(0.2, 0.8), (0.8, 0.3)] {
        let before = theme::accent();
        let pointer = square.min + Vec2::new(square.width() * x, square.height() * fraction);
        harness.hover_at(pointer);
        harness.step();
        harness.input_mut().events.push(button(pointer, true));
        harness.step();
        harness.input_mut().events.push(button(pointer, false));
        harness.step();
        harness.step();
        assert_ne!(theme::accent(), before, "the accent follows the colour picked at {pointer:?}");
        assert_ne!(theme::accent(), first);
    }
}

/// Reset changes puts a theme's own settings back, and they still work afterwards.
#[test]
fn reset_changes_restores_a_themes_own_settings() {
    let mut harness = short_window();
    let sheet = "@property --glow { syntax: \"<integer>\"; initial-value: 50; label: \"Glow\"; range: 0 100; }\n\
                 @property --tint { syntax: \"<color>\"; initial-value: #336699; label: \"Tint\"; }\n\
                 :root { --accent: var(--tint); }";
    let (ink, warnings) = theme::from_folder(theme::ThemeMeta::new("Ink", theme::DEFAULT_SWATCH), sheet);
    assert!(warnings.is_empty(), "{warnings:?}");
    let entries = vec![suhina_ui::settings::themes::ThemeEntry { id: "ink".into(), theme: ink }];
    let ctx = harness.ctx.clone();
    harness.state_mut().set_themes(&ctx, entries, Some("themes".into()));
    harness.state_mut().use_theme(&ctx, "ink");
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Edit current theme…").click();
    settle(&mut harness);
    let original = theme::accent();

    // A setting that no colour follows counts as a change too.
    let glow = |h: &Harness<'static, App>| {
        h.get_by_role_and_label(egui::accesskit::Role::Slider, "Glow").accesskit_node().numeric_value()
    };
    let before = glow(&harness);
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Glow").scroll_to_me();
    settle(&mut harness);
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Glow").click();
    settle(&mut harness);
    assert_ne!(glow(&harness), before, "the click moved the slider");
    harness.get_by_role_and_label(egui::accesskit::Role::ColorWell, "Tint").click();
    settle(&mut harness);
    let square = harness
        .output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(square) if square.brush.is_some() => {
                let rect = square.rect;
                (rect.width() > 250.0 && rect.width() < 300.0 && (rect.width() - rect.height()).abs() < 1.0)
                    .then_some(rect)
            }
            _ => None,
        })
        .expect("the colour picker's square");
    let press = |h: &mut Harness<'static, App>, pressed| {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: square.center(),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    };
    harness.hover_at(square.center());
    harness.step();
    press(&mut harness, true);
    press(&mut harness, false);
    harness.step();
    assert_ne!(theme::accent(), original);
    egui::Popup::close_all(&harness.ctx);
    settle(&mut harness);

    harness.get_by_label("Reset changes").scroll_to_me();
    settle(&mut harness);
    harness.get_by_label("Reset changes").click();
    settle(&mut harness);
    assert_eq!(theme::accent(), original, "the colour that follows the setting is back");
    assert_eq!(glow(&harness), before, "the slider is back");
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
    harness.get_by_label("Animations");
    harness.get_by_label("Dock").click();
    settle(&mut harness);
    harness.get_by_label("Open in window");
    harness.get_by_label("Animations");
    harness.get_by_label("Close settings").click();
    settle(&mut harness);
    assert!(!harness.state().settings_open());
}

#[test]
fn liquid_sliders_change_independent_saved_strengths_without_restarting() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((1200.0, 850.0))
        .with_step_dt(0.25)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    // The liquid sliders are under Appearance › Advanced; a search lists them unfolded.
    harness.get_by_label("Search settings").focus();
    harness.run();
    harness.get_by_label("Search settings").type_text("liquid");
    harness.run();
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Panel liquid").click();
    harness.run();
    let panels = harness.state().settings().view.liquid.panels;
    assert!(panels > 0);
    assert_eq!(harness.state().settings().view.liquid.tracks, 0);
    assert_eq!(harness.state().settings().view.liquid.clips, 0);
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Track liquid").click();
    harness.run();
    let tracks = harness.state().settings().view.liquid.tracks;
    assert!(tracks > 0);
    assert_eq!(harness.state().settings().view.liquid.panels, panels);
    assert_eq!(harness.state().settings().view.liquid.clips, 0);
    harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Clip liquid").click();
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
    harness.get_by_label("Settings page: Agents and updates").click();
    harness.run();
    harness.get_by_label("Add MCP write folder").scroll_to_me();
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
    harness.get_by_label(&format!("Remove MCP write folder {}", folder.display())).scroll_to_me();
    harness.run();
    harness.get_by_label(&format!("Remove MCP write folder {}", folder.display())).click();
    harness.run();
    assert!(harness.state().settings().mcp_write_folders.is_empty());
    harness.get_by_label("Project folder for agents").scroll_to_me();
    harness.run();
    harness.get_by_label("Project folder for agents").click();
    harness.run();
    assert!(!harness.state().settings().mcp_project_folder);
}

/// A small-window visual check with a long folder name; run with --ignored.
#[test]
#[ignore = "writes a settings screenshot for visual inspection; needs a GPU"]
fn export_permissions_snapshot() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Agents and updates").click();
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
        .allow_missing_glyphs()
        .allow_missing_glyphs()
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
    // The panel ends above the status bar.
    rect.top() > title.bottom() && rect.bottom() < SHORT.1 - theme::STATUS_BAR - theme::PAD
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
            source: egui::MouseWheelSource::Unknown,
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
        ("Projects and backups", Some("Delete old backups")),
        ("Audio", Some("Cue volume: +6 dB")),
        ("MIDI", None),
        ("Plug-ins", Some("Plug-in editors open in: Device view")),
        ("Devices", Some("Detail area width: Full width")),
        ("Importing", Some("Warp mode for samples: Rhythmic")),
        ("Arrangement", Some("Show advanced settings")),
        ("Launcher and mixer", Some("Mixer control style: Knob")),
        ("Clip editor", Some("Piano roll toolbar size: Larger")),
        ("Browser", Some("Colour tag limit: 1024")),
        ("Keys", Some("Change keys of file.new")),
        ("Appearance", Some("Show advanced settings")),
        ("Themes", Some("Browse for Themes folder")),
        ("Agents and updates", Some("Show advanced settings")),
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
fn a_long_page_needs_the_wheel_and_another_page_opens_at_its_top() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    let last = "Show advanced settings";
    assert!(!shown(&harness, last), "the short window shows the whole Arrangement page");
    assert!(wheel_to(&mut harness, last, 40) > 0);
    // Scrolled down, the page's first setting is out of view; another page starts at its top.
    assert!(!shown(&harness, "Let audio clips overlap"));
    harness.get_by_label("Settings page: Browser").click();
    harness.run();
    assert!(shown(&harness, "Browser side: Left"));
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    assert!(shown(&harness, "Let audio clips overlap"));
}

#[test]
fn a_control_given_the_focus_is_scrolled_into_view() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    let last = "Show advanced settings";
    assert!(!shown(&harness, last));
    harness.get_by_label(last).focus();
    harness.run();
    assert!(shown(&harness, last), "the focused control stays out of view");
}

#[test]
fn the_advanced_group_is_folded_until_it_is_opened_and_a_search_finds_inside_it() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Appearance").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Graphics renderer: OpenGL").is_none(), "folded");
    wheel_to(&mut harness, "Show advanced settings", 40);
    harness.get_by_label("Show advanced settings").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Graphics renderer: OpenGL").is_some(), "unfolded");
    harness.get_by_label("Hide advanced settings").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Graphics renderer: OpenGL").is_none());
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("renderer");
    settle(&mut harness);
    assert!(harness.query_by_label("Graphics renderer: OpenGL").is_some(), "a search shows it");
}

#[test]
fn settings_reopen_on_the_page_used_last() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Browser").click();
    settle(&mut harness);
    harness.get_by_label("Close settings").click();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    settle(&mut harness);
    assert!(harness.query_by_label("Browser side: Left").is_some());
}

#[test]
fn a_search_shows_the_settings_that_match_from_every_page() {
    let mut harness = short_window();
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("preview");
    settle(&mut harness);
    // Previews in the browser and on the map, from one page; settings that do not match are gone.
    assert!(harness.query_by_label("Preview selected file").is_some());
    assert!(harness.query_by_label("Preview on map hover").is_some());
    assert!(harness.query_by_label("Browser side: Left").is_none());
    assert!(harness.query_by_label("Open projects in: Ask").is_none());
    // A change made in the results is made as on its page.
    harness.get_by_label("Preview selected file").click();
    settle(&mut harness);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.preview_autoplay), Some(false));

    // Words match anywhere: the page and the section count too.
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Search settings").type_text("arrangement playhead");
    settle(&mut harness);
    assert!(harness.query_by_label("Playhead after Stop: Stay").is_some());
    assert!(
        harness.query_by_label("Play from: Marker").is_none(),
        "only Stop has both words (playhead is in its note)"
    );

    // Nothing matches.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Search settings").type_text("nothing like this");
    settle(&mut harness);
    assert!(harness.query_by_label("Playhead after Stop: Stay").is_none());

    // A page clicked empties the search and shows the page.
    harness.get_by_label("Settings page: Arrangement").click();
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
    assert!(harness.query_by_label("Window zoom: +").is_some());
    harness.key_press(Key::Escape);
    settle(&mut harness);
    assert!(harness.state().settings_open(), "Escape in the search closed Settings");
    assert!(harness.query_by_label("Window zoom: +").is_none(), "the General page shows again");
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
    harness.get_by_label("Settings page: Keys").click();
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of file.save").is_some(), "the whole list without a filter");
    harness.get_by_label("Filter shortcuts").focus();
    settle(&mut harness);
    harness.get_by_label("Filter shortcuts").type_text("ctrl+alt+up");
    settle(&mut harness);
    assert!(harness.query_by_label("Change keys of view.waveformZoomIn").is_some());
    assert!(harness.query_by_label("Change keys of file.save").is_none());
}

/// Help, beside Close, opens a dialog that chooses how a setting's description is shown; the
/// choice is kept.
#[test]
fn help_chooses_how_descriptions_are_shown() {
    use suhina_ui::settings::Descriptions;
    let mut harness = short_window();
    assert_eq!(harness.state().settings().setting_descriptions, Descriptions::Tooltip, "a tooltip by default");
    harness.get_by_label("Settings help").click();
    harness.run();
    harness.get_by_label("Below the name: each setting's description is always shown").click();
    harness.run();
    assert_eq!(harness.state().settings().setting_descriptions, Descriptions::Shown);
    harness.get_by_label("? icon: point at the ? next to a setting to read what it does").click();
    harness.run();
    let settings = harness.state().settings().clone();
    assert_eq!(settings.setting_descriptions, Descriptions::Icon);
    assert_eq!(
        suhina_ui::settings::Settings::from_text(&settings.to_text()).0.setting_descriptions,
        Descriptions::Icon
    );
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
    wheel_to(&mut harness, "Use global liquid settings", 40);
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
    wheel_to(&mut harness, "Use global liquid settings", 40);
    harness.get_by_label("Use global liquid settings").click();
    settle(&mut harness);
    harness.get_by_label("Search settings").focus();
    settle(&mut harness);
    harness.get_by_label("Search settings").type_text("Panel liquid");
    settle(&mut harness);
    assert!(!harness.get_by_label("Panel liquid").accesskit_node().is_disabled());
    assert_eq!(harness.state().settings().view.liquid, global);
}

/// A real pointer click on Cancel in *Move the folder?* leaves the themes where they are: the
/// themes folder keeps its value and no move is asked for (round 6, step 63).
#[test]
fn a_pointer_click_on_cancel_in_move_the_folder_moves_nothing() {
    let mut harness = short_window();
    let mut settings = harness.state().settings().clone();
    settings.themes_folder = "/tmp/suhina-themes-elsewhere".into();
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Reset Themes folder to its default").scroll_to_me();
    settle(&mut harness);
    let reset = harness.get_by_label("Reset Themes folder to its default").rect().center();
    harness.drag_at(reset);
    harness.drop_at(reset);
    settle(&mut harness);
    let cancel = harness.get_by_label("Leave them where they are").rect().center();
    harness.hover_at(cancel);
    settle(&mut harness);
    harness.drag_at(cancel);
    harness.drop_at(cancel);
    for _ in 0..4 {
        settle(&mut harness);
    }
    assert_eq!(harness.state_mut().take_theme_move(), None);
    assert_eq!(harness.state().settings().themes_folder, "/tmp/suhina-themes-elsewhere");
    assert!(harness.query_by_label("Leave them where they are").is_none());
}

/// A search no theme matches says so, the theme in use included (round 6, step 17).
#[test]
fn a_theme_search_nothing_matches_says_no_themes_match() {
    let mut harness = short_window();
    harness.get_by_label("Settings page: Themes").click();
    settle(&mut harness);
    harness.get_by_label("Search themes").focus();
    harness.run();
    harness.get_by_label("Search themes").type_text("zzz");
    for _ in 0..3 {
        settle(&mut harness);
    }
    let search = harness.get_by_label("Search themes").value();
    assert_eq!(search.as_deref(), Some("zzz"));
    assert!(harness.query_by_label("No themes match").is_some());
}

#[test]
fn the_theme_in_use_is_marked_as_selected_in_the_list() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    // No GPU: the CI machines have none, and nothing here is drawn to an image.
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size((1200.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Themes").click();
    harness.run();
    let selected = |harness: &Harness<'static, App>, label: &str| {
        harness.get_by_label(label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
    };
    assert!(selected(&harness, "Theme in use: Suhina Studio"));
    assert!(selected(&harness, "Theme: Suhina Studio"));
    assert!(!selected(&harness, "Theme: Suhina Light"));
    wheel_to(&mut harness, "Theme: Suhina Light", 40);
    harness.get_by_label("Theme: Suhina Light").click();
    harness.run();
    assert!(selected(&harness, "Theme: Suhina Light"));
    assert!(!selected(&harness, "Theme: Suhina Studio"));
    theme::set_palette(&harness.ctx, theme::Palette::DARK);
}
