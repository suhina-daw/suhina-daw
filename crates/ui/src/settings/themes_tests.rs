// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn gradient_edits_copy_builtins_save_duplicate_and_reset_with_the_theme() {
    use crate::kit::gradient::{Gradient, Gradients};
    let mut themes = Themes::default();
    let gradients =
        Gradients { tracks: Some(Gradient::default()), clips: Some(Gradient::default()), ..Default::default() };
    let id = themes.request("nord", ThemeRequest::EditGradients(gradients.clone())).unwrap();
    assert_eq!(themes.get("nord").unwrap().gradients, Gradients::default());
    let edited = themes.get(&id).unwrap().clone();
    assert_eq!(edited.gradients, gradients);
    assert_eq!(Theme::from_json(&edited.to_json()).0.gradients, edited.gradients);
    let copy = themes.request(&id, ThemeRequest::Duplicate { artwork: false }).unwrap();
    assert_eq!(themes.get(&copy).unwrap().gradients, gradients);
    themes.request(
        &id,
        ThemeRequest::ResetEdits {
            skin: Default::default(),
            transparent_rgb: Default::default(),
            colors: edited.colors,
            liquid: edited.liquid,
            gradients: Gradients::default(),
            values: Default::default(),
        },
    );
    assert_eq!(themes.get(&id).unwrap().gradients, Gradients::default());
    assert_eq!(themes.get(&copy).unwrap().gradients, gradients);
}

#[test]
fn reset_changes_restores_a_themes_own_settings_and_what_follows_them() {
    let sheet = "@property --tint { syntax: \"<color>\"; initial-value: #112233; label: \"Accent\"; }\n\
                 :root { --accent: var(--tint); }";
    let (ink, warnings) = theme::from_folder(theme::ThemeMeta::new("Ink", theme::DEFAULT_SWATCH), sheet);
    assert!(warnings.is_empty(), "{warnings:?}");
    let original = ink.clone();
    let mut themes = Themes::default();
    themes.put(ThemeEntry { id: "ink".into(), theme: ink });
    let edit = |tint: &str| ThemeRequest::EditValue { name: "--tint".into(), value: tint.into() };
    let id = themes.request("ink", edit("#FF0000")).unwrap();
    assert_eq!(themes.get(&id).unwrap().colors.accent, egui::Color32::from_rgb(255, 0, 0));
    themes.request(
        &id,
        ThemeRequest::ResetEdits {
            colors: original.colors,
            liquid: original.liquid,
            gradients: original.gradients.clone(),
            transparent_rgb: original.transparent_rgb.clone(),
            skin: Box::new(original.skin.clone()),
            values: [("--tint".to_string(), "#112233".to_string())].into(),
        },
    );
    let reset = themes.get(&id).unwrap();
    assert_eq!(reset.colors.accent, original.colors.accent);
    assert!(reset.vars.values.is_empty(), "{:?}", reset.vars.values);
    // The setting still works afterwards.
    themes.request(&id, edit("#00FF00"));
    assert_eq!(themes.get(&id).unwrap().colors.accent, egui::Color32::from_rgb(0, 255, 0));
}

fn night() -> ThemeEntry {
    let mut colors = Palette::DARK;
    colors.accent = egui::Color32::from_rgb(1, 2, 3);
    ThemeEntry {
        id: "night".into(),
        theme: Theme {
            name: "Night".into(),
            colors,
            liquid: None,
            gradients: Default::default(),
            transparent_rgb: Default::default(),
            skin: Default::default(),
            vars: Default::default(),
            ..Default::default()
        },
    }
}

fn saved(themes: &mut Themes) -> Vec<(String, String)> {
    themes
        .take_actions()
        .into_iter()
        .filter_map(|a| match a {
            FileAction::Save { id, theme } => Some((id, theme.name)),
            _ => None,
        })
        .collect()
}

#[test]
fn restoring_a_built_in_can_save_a_copy_without_overwriting_either_theme() {
    let mut themes = Themes::default();
    themes.set(vec![night()], None);
    let original = themes.get("night").cloned();
    let built_in = themes.palette(BUILT_IN);
    assert_eq!(
        themes.request("night", ThemeRequest::RestoreBuiltIn { id: BUILT_IN.into(), save_copy: true }),
        Some(BUILT_IN.into())
    );
    assert_eq!(themes.get("night"), original.as_ref());
    assert_eq!(themes.palette(BUILT_IN), built_in);
    let actions = themes.take_actions();
    assert!(
        matches!(actions.as_slice(), [FileAction::Save { theme, .. }] if theme.colors == original.as_ref().unwrap().colors)
    );
    assert_eq!(
        themes.request("night", ThemeRequest::RestoreBuiltIn { id: "nord".into(), save_copy: false }),
        Some("nord".into())
    );
    assert!(themes.take_actions().is_empty());
    assert_eq!(themes.request("night", ThemeRequest::RestoreBuiltIn { id: "night".into(), save_copy: true }), None);
    assert!(themes.take_actions().is_empty());
}

#[test]
fn the_built_in_themes_come_first_and_stay() {
    let mut themes = Themes::default();
    let fake = ThemeEntry { id: "Suhina-Studio".into(), theme: night().theme };
    themes.set(vec![night(), fake.clone(), night()], Some("themes".into()));
    let view = themes.view("night");
    assert_eq!(view.list.first(), Some(&("night".into(), "Night".into(), false)));
    assert_eq!(view.list.get(1), Some(&(BUILT_IN.into(), "Suhina Studio".into(), true)));
    assert!(view.list.iter().any(|(id, _, built_in)| id == "nord" && *built_in));
    assert_eq!(view.list.len(), theme::built_in_themes().len() + 1);
    assert_eq!(view.colors, night().theme.colors);
    // Every built-in theme has a swatch and a thumbnail for the picker, and names its default variant.
    for (id, _, built_in) in view.list.iter().filter(|(_, _, built_in)| *built_in) {
        assert!(view.swatches.iter().any(|(key, _)| key == id), "{id} ({built_in})");
        assert!(view.pictures.iter().any(|(key, svg)| key == id && !svg.contains("var(")), "{id}");
        assert!(view.default_variants.iter().any(|(key, name)| key == id && !name.is_empty()), "{id}");
    }
    assert!(!view.built_in);
    assert!(!themes.put(fake));
    assert_eq!(themes.palette(BUILT_IN), Palette::DARK);
    assert!(themes.view("nord").built_in);
}

#[test]
fn a_missing_theme_draws_with_suhina_studio() {
    let themes = Themes::default();
    assert_eq!(themes.palette("gone"), Palette::DARK);
    assert_eq!(themes.view("gone").current, BUILT_IN);
}

#[test]
fn editing_a_built_in_themes_colours_makes_a_variant_and_its_layout_a_copy() {
    let mut themes = Themes::default();
    let mut colors = themes.palette("nord");
    colors.border = egui::Color32::from_rgb(9, 9, 9);
    assert_eq!(themes.request("nord", ThemeRequest::Edit(colors)), Some("nord@custom".into()));
    assert_ne!(themes.palette("nord"), colors);
    assert_eq!(themes.palette("nord@custom"), colors);
    let custom = themes.get("nord@custom").unwrap();
    assert_eq!(custom.vars.variant_name.as_deref(), Some("Custom"));
    assert_eq!(custom.vars.values.keys().map(String::as_str).collect::<Vec<_>>(), ["--border"]);
    // A second edit changes the variant, not another one.
    colors.border = egui::Color32::from_rgb(8, 8, 8);
    assert_eq!(themes.request("nord@custom", ThemeRequest::Edit(colors)), None);
    assert_eq!(saved(&mut themes), [("nord@custom".into(), "Nord".into())]);
    assert_eq!(themes.palette("nord@custom"), colors);
    assert!(themes.take_actions().is_empty());
    // The variant is by Nord's card, not a theme of its own.
    let view = themes.view("nord@custom");
    assert!(!view.list.iter().any(|(id, _, _)| id.contains('@')));
    assert!(view.variants.contains(&("nord@custom".to_string(), "Custom".to_string())));
    assert!(
        view.variants.contains(&("nord@snow-storm".to_string(), "Snow Storm".to_string())),
        "its own variants stay"
    );
    assert_eq!(view.variant.as_deref(), Some("Custom"));
    // Moving a control is not a setting: the built-in theme is copied.
    let mut skin = themes.get("nord@custom").unwrap().skin.clone();
    skin.toolbar_offsets.insert("transport".into(), 40);
    assert_eq!(themes.request("nord@custom", ThemeRequest::EditSkin(skin)), Some("nord-custom".into()));
    assert_eq!(themes.palette("nord-custom"), colors, "the copy keeps the variant's colours");
}

#[test]
fn a_declared_setting_is_set_in_the_variant_and_checked_against_its_type() {
    let sheet = "@property --glow { syntax: \"<number>\"; initial-value: 0.5; label: \"Glow\"; range: 0 1; }\n:root { --accent: #123456; }";
    let (theme, warnings) = crate::kit::theme::from_css(sheet);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut themes = Themes::default();
    themes.set(vec![ThemeEntry { id: "ink".into(), theme }], None);
    assert_eq!(themes.view("ink").declared.len(), 1);
    assert_eq!(themes.view("ink").values.get("--glow").map(String::as_str), Some("0.5"));
    let set = |value: &str| ThemeRequest::EditValue { name: "--glow".into(), value: value.into() };
    assert_eq!(themes.request("ink", set("2")), None, "outside its range");
    assert_eq!(themes.request("ink", set("0.2")), Some("ink@custom".into()));
    assert_eq!(themes.view("ink@custom").values.get("--glow").map(String::as_str), Some("0.2"));
    assert_eq!(themes.get("ink@custom").unwrap().skin.variant.as_deref(), Some(":root { --glow: 0.2; }"));
    assert_eq!(themes.request("ink@custom", ThemeRequest::NewVariant("Bright".into())), Some("ink@bright".into()));
    assert_eq!(themes.request("ink@bright", ThemeRequest::Rename("Brighter".into())), None);
    assert_eq!(themes.view("ink@bright").variant.as_deref(), Some("Brighter"));
}

#[test]
fn duplicates_get_their_own_ids() {
    let mut themes = Themes::default();
    themes.set(vec![night()], None);
    assert_eq!(themes.request("night", ThemeRequest::Duplicate { artwork: false }), Some("night-copy".into()));
    assert_eq!(themes.request("night", ThemeRequest::Duplicate { artwork: false }), Some("night-copy-2".into()));
    assert_eq!(themes.get("night-copy-2").map(|t| t.colors), Some(night().theme.colors));
    assert_eq!(saved(&mut themes).len(), 2);
}

#[test]
fn renaming_a_theme_keeps_its_folder() {
    let mut themes = Themes::default();
    themes.set(vec![night()], None);
    assert_eq!(themes.request(BUILT_IN, ThemeRequest::Rename("Mine".into())), None);
    assert!(themes.take_actions().is_empty());
    // Even a name whose id another theme has: the folder is the id and stays.
    assert_eq!(themes.request("night", ThemeRequest::Rename(" Nord ".into())), None);
    assert_eq!(saved(&mut themes), [("night".into(), "Nord".into())]);
    assert_eq!(themes.get("night").map(|t| t.name.as_str()), Some("Nord"));
    assert!(themes.get("nord").is_some_and(|t| t.name == "Nord"), "the built-in Nord is untouched");
}

#[test]
fn using_and_file_actions() {
    let mut themes = Themes::default();
    themes.set(vec![night()], None);
    assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("night".into())), Some("night".into()));
    assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("nothing".into())), None);
    for request in [ThemeRequest::Reload, ThemeRequest::Backup, ThemeRequest::Backup, ThemeRequest::ExportTemplate] {
        assert_eq!(themes.request(BUILT_IN, request), None);
    }
    themes.import("night.json".into());
    assert_eq!(
        themes.take_actions(),
        [FileAction::Reload, FileAction::Backup, FileAction::ExportTemplate, FileAction::Import("night.json".into())]
    );
}

#[test]
fn ids_are_made_from_names() {
    let themes = Themes::default();
    assert_eq!(themes.free_id("  Night Blue!! "), "night-blue");
    assert_eq!(themes.free_id("../.."), "theme");
    assert_eq!(themes.free_id("Suhina Studio"), "suhina-studio-2");
    assert_eq!(slug("Café Thème"), "café-thème");
}

#[test]
fn a_folder_that_is_not_a_theme_keeps_its_id_from_a_copy() {
    let mut themes = Themes::default();
    themes.reserve(vec!["nord-copy".into(), "Night".into()]);
    assert_eq!(themes.free_id("Nord copy"), "nord-copy-2");
    assert_eq!(themes.free_id("night"), "night-2");
    assert_eq!(themes.free_id("Other"), "other");
}

#[test]
fn a_copy_of_a_theme_with_placed_controls_says_so() {
    let mut themes = Themes::default();
    let mut placed = night();
    placed.theme.skin.controls.insert("arrangement-volume".into(), Default::default());
    themes.set(vec![placed], None);
    assert_eq!(themes.request("night", ThemeRequest::Duplicate { artwork: false }), Some("night-copy".into()));
    assert!(themes.take_notice().is_some_and(|n| n.contains("1 placed control of")), "the copy says what it keeps");
    assert_eq!(themes.take_notice(), None, "once");
    themes.set(vec![night()], None);
    themes.request("night", ThemeRequest::Duplicate { artwork: false });
    assert_eq!(themes.take_notice(), None, "nothing placed, nothing said");
}

#[test]
fn editing_a_palette_colour_refreshes_the_swatch_and_thumbnail_of_its_card() {
    for id in ["nord", BUILT_IN] {
        a_palette_edit_shows_on_the_card(id);
    }
}

fn a_palette_edit_shows_on_the_card(from: &str) {
    let mut themes = Themes::default();
    let before = themes.get(from).unwrap().clone();
    let mut colors = themes.palette(from);
    colors.accent = egui::Color32::from_rgb(250, 10, 120);
    colors.window_background = egui::Color32::from_rgb(10, 250, 120);
    let id = themes
        .request(from, ThemeRequest::EditRgba { colors, transparent_rgb: before.transparent_rgb })
        .expect("a built-in theme's edit makes a variant");
    let edited = themes.get(&id).unwrap().clone();
    // What the card shows is what loading the theme again would show.
    let mut loaded = edited.clone();
    crate::kit::theme_vars::resolve(&mut loaded);
    assert_eq!(edited.skin.variables, loaded.skin.variables);
    assert_eq!(edited.swatch, loaded.swatch);
    assert_eq!(edited.picture, loaded.picture);
    assert!(edited.swatch != before.swatch || edited.picture != before.picture, "the edit shows on the card");
}

#[test]
fn a_saved_edit_of_a_built_in_variant_wins_and_goes_when_its_file_goes() {
    let mut themes = Themes::default();
    let original = themes.palette("nord@snow-storm");
    let mut edited = themes.get("nord@snow-storm").unwrap().clone();
    edited.colors.accent = egui::Color32::from_rgb(1, 2, 3);
    themes.set(vec![ThemeEntry { id: "nord@snow-storm".into(), theme: edited.clone() }], None);
    assert_eq!(themes.palette("nord@snow-storm"), edited.colors);
    assert_eq!(themes.view("nord").variants.iter().filter(|(id, _)| id == "nord@snow-storm").count(), 1);
    themes.set(Vec::new(), None);
    assert_eq!(themes.palette("nord@snow-storm"), original);
}

#[test]
fn resetting_a_skin_only_edit_makes_no_variant_and_restores_the_theme_itself() {
    let (ink, _) =
        theme::from_folder(theme::ThemeMeta::new("Ink", theme::DEFAULT_SWATCH), ":root { --accent: #112233; }");
    let original = ink.clone();
    let mut edited = ink.clone();
    edited.skin.toolbar_offsets.insert("tempo".into(), 40);
    let mut themes = Themes::default();
    themes.put(ThemeEntry { id: "ink".into(), theme: edited });
    assert_ne!(themes.get("ink").unwrap().skin.toolbar_offsets, original.skin.toolbar_offsets);
    let switched = themes.request(
        "ink",
        ThemeRequest::ResetEdits {
            colors: original.colors,
            liquid: original.liquid,
            gradients: original.gradients.clone(),
            transparent_rgb: original.transparent_rgb.clone(),
            skin: Box::new(original.skin.clone()),
            values: [("--accent".to_string(), "#112233".to_string())].into(),
        },
    );
    assert_eq!(themes.get("ink").unwrap().skin.toolbar_offsets, original.skin.toolbar_offsets);
    assert!(themes.get("ink@custom").is_none(), "a reset of what was not edited made a variant");
    assert_ne!(switched.as_deref(), Some("ink@custom"));
}

/// Duplicate with *Include the default artwork* gives the copy every image Suhina ships, to be
/// written to its `assets/` with a rule each; without it the copy has only the artwork it had.
#[test]
fn duplicate_can_include_the_default_artwork() {
    let mut themes = Themes::default();
    let plain = themes.request("nord", ThemeRequest::Duplicate { artwork: false }).unwrap();
    assert!(themes.get(&plain).unwrap().skin.assets.is_empty());
    let with = themes.request("nord", ThemeRequest::Duplicate { artwork: true }).unwrap();
    let assets = &themes.get(&with).unwrap().skin.assets;
    assert_eq!(assets.len(), crate::kit::skin::roles().count());
    let sheet = crate::kit::theme_css::export(themes.get(&with).unwrap());
    assert!(sheet.contains("knob-body { image: url(\"assets/knob.body.svg\"); }"), "{sheet}");
    assert!(sheet.contains("icon[kind=\"play\"] { image: url(\"assets/play.svg\"); }"), "{sheet}");
}

/// New theme makes a theme of its own from nothing but the starting sheet, and saves it.
#[test]
fn new_theme_starts_from_the_user_agent_sheet() {
    let mut themes = Themes::default();
    let id = themes.request("nord", ThemeRequest::NewTheme { artwork: false }).unwrap();
    assert_eq!(id, "new-theme");
    let theme = themes.get(&id).unwrap();
    assert_eq!(theme.name, "New theme");
    assert_eq!(theme.skin.css.as_deref(), Some(theme::NEW_THEME_SHEET));
    assert!(theme.skin.assets.is_empty());
    assert!(
        themes.take_actions().iter().any(|action| matches!(action, FileAction::Save { id, .. } if id == "new-theme"))
    );
    let with = themes.request("nord", ThemeRequest::NewTheme { artwork: true }).unwrap();
    assert_eq!(with, "new-theme-2");
    assert_eq!(themes.get(&with).unwrap().name, "New theme 2", "named as its id is numbered");
    assert_eq!(themes.get(&with).unwrap().skin.assets.len(), crate::kit::skin::roles().count());
}

/// Add default artwork adds only what is missing; a built-in theme is copied first, and a theme
/// with all of it is left as it is with a notice.
#[test]
fn add_default_artwork_adds_only_what_is_missing() {
    let mut themes = Themes::default();
    let copy = themes.request("suhina-light", ThemeRequest::AddDefaultArtwork).unwrap();
    assert_ne!(copy, "suhina-light");
    assert!(themes.get("suhina-light").unwrap().skin.assets.is_empty());
    assert_eq!(themes.get(&copy).unwrap().skin.assets.len(), crate::kit::skin::roles().count());
    // Again: nothing is missing.
    themes.take_actions();
    assert_eq!(themes.request(&copy, ThemeRequest::AddDefaultArtwork), None);
    assert!(themes.take_actions().is_empty());
    // A theme with artwork of its own for a role keeps it.
    let id = themes.request("nord", ThemeRequest::Duplicate { artwork: false }).unwrap();
    themes.request(
        &id,
        ThemeRequest::EditSkin(crate::kit::skin::Skin {
            assets: [("knob.body".to_owned(), "assets/mine.svg".to_owned())].into(),
            ..Default::default()
        }),
    );
    assert_eq!(themes.request(&id, ThemeRequest::AddDefaultArtwork), None);
    let assets = &themes.get(&id).unwrap().skin.assets;
    assert_eq!(assets.get("knob.body").map(String::as_str), Some("assets/mine.svg"));
    assert_eq!(assets.len(), crate::kit::skin::roles().count());
}

/// Add default artwork on a theme of the user's in use with a variant adds the artwork to the
/// theme itself and to each of its variants, and saves the theme, not only the variant.
#[test]
fn add_default_artwork_on_a_variant_goes_to_its_theme() {
    let sheet = "@property --glow { syntax: \"<number>\"; initial-value: 0.5; label: \"Glow\"; range: 0 1; }";
    let (theme, warnings) = crate::kit::theme::from_css(sheet);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut themes = Themes::default();
    themes.set(vec![ThemeEntry { id: "ink".into(), theme }], None);
    let set = ThemeRequest::EditValue { name: "--glow".into(), value: "0.2".into() };
    assert_eq!(themes.request("ink", set), Some("ink@custom".into()));
    themes.take_actions();
    assert_eq!(themes.request("ink@custom", ThemeRequest::AddDefaultArtwork), None, "the variant stays in use");
    let all = crate::kit::skin::roles().count();
    for id in ["ink", "ink@custom"] {
        assert_eq!(themes.get(id).unwrap().skin.assets.len(), all, "{id}");
    }
    let actions = themes.take_actions();
    assert!(actions.iter().any(|action| matches!(action, FileAction::Save { id, .. } if id == "ink")), "{actions:?}");
    assert_eq!(themes.take_notice().as_deref(), Some("Added 16 default images to the theme's assets folder"));
}

/// Add default artwork on a theme of the user's in use as itself adds the artwork to its variants
/// too, so a later save of a variant, which writes the theme's theme.css, keeps the rules.
#[test]
fn add_default_artwork_on_a_theme_goes_to_its_variants() {
    let sheet = "@property --glow { syntax: \"<number>\"; initial-value: 0.5; label: \"Glow\"; range: 0 1; }";
    let (theme, warnings) = crate::kit::theme::from_css(sheet);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut themes = Themes::default();
    themes.set(vec![ThemeEntry { id: "ink".into(), theme }], None);
    let set = ThemeRequest::EditValue { name: "--glow".into(), value: "0.2".into() };
    assert_eq!(themes.request("ink", set), Some("ink@custom".into()));
    themes.take_actions();
    assert_eq!(themes.request("ink", ThemeRequest::AddDefaultArtwork), None, "the theme stays in use");
    let all = crate::kit::skin::roles().count();
    for id in ["ink", "ink@custom"] {
        assert_eq!(themes.get(id).unwrap().skin.assets.len(), all, "{id}");
    }
    let actions = themes.take_actions();
    assert!(actions.iter().any(|action| matches!(action, FileAction::Save { id, .. } if id == "ink")), "{actions:?}");
}
