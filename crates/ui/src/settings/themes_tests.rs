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
    let copy = themes.request(&id, ThemeRequest::Duplicate).unwrap();
    assert_eq!(themes.get(&copy).unwrap().gradients, gradients);
    themes.request(
        &id,
        ThemeRequest::ResetEdits {
            skin: Default::default(),
            transparent_rgb: Default::default(),
            colors: edited.colors,
            liquid: edited.liquid,
            gradients: Gradients::default(),
        },
    );
    assert_eq!(themes.get(&id).unwrap().gradients, Gradients::default());
    assert_eq!(themes.get(&copy).unwrap().gradients, gradients);
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
    let fake = ThemeEntry { id: "Suhina-Dark".into(), theme: night().theme };
    themes.set(vec![night(), fake.clone(), night()], Some("themes".into()));
    let view = themes.view("night");
    assert_eq!(view.list.first(), Some(&("night".into(), "Night".into(), false)));
    assert_eq!(view.list.get(1), Some(&(BUILT_IN.into(), "Suhina Dark".into(), true)));
    assert!(view.list.iter().any(|(id, _, built_in)| id == "nord" && *built_in));
    assert_eq!(view.list.len(), theme::built_in_themes().len() + 1);
    assert_eq!(view.colors, night().theme.colors);
    assert_eq!(view.previews.len(), view.list.len());
    for (id, _, _) in &view.list {
        assert_eq!(
            view.previews.iter().find(|(key, _)| key == id).map(|(_, colors)| *colors),
            Some(themes.palette(id))
        );
    }
    assert!(!view.built_in);
    assert!(!themes.put(fake));
    assert_eq!(themes.palette(BUILT_IN), Palette::DARK);
    assert!(themes.view("nord").built_in);
}

#[test]
fn a_missing_theme_draws_with_suhina_dark() {
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
    assert_eq!(view.variants, [("nord@custom".to_string(), "Custom".to_string())]);
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
    let (theme, warnings) = crate::kit::theme::from_sheet(sheet, "");
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
    assert_eq!(themes.request("night", ThemeRequest::Duplicate), Some("night-copy".into()));
    assert_eq!(themes.request("night", ThemeRequest::Duplicate), Some("night-copy-2".into()));
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
    assert_eq!(themes.free_id("Suhina Dark"), "suhina-dark-2");
    assert_eq!(slug("Café Thème"), "café-thème");
}

#[test]
fn a_copy_of_a_theme_with_placed_controls_says_so() {
    let mut themes = Themes::default();
    let mut placed = night();
    placed.theme.skin.controls.insert("arrangement-volume".into(), Default::default());
    themes.set(vec![placed], None);
    assert_eq!(themes.request("night", ThemeRequest::Duplicate), Some("night-copy".into()));
    assert!(themes.take_notice().is_some_and(|n| n.contains("1 placed controls")), "the copy says what it keeps");
    assert_eq!(themes.take_notice(), None, "once");
    themes.set(vec![night()], None);
    themes.request("night", ThemeRequest::Duplicate);
    assert_eq!(themes.take_notice(), None, "nothing placed, nothing said");
}
