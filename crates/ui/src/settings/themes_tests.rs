// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::settings::settings_view::NewTheme;

/// Every colour token's colour in the colour map `colors`, Suhina Studio's where it sets none.
fn effective(colors: &theme::Colors) -> Vec<egui::Color32> {
    theme::TOKENS.iter().map(|(key, _)| theme::color_in(colors, &theme::variable_name(key))).collect()
}

/// The colours the theme `id` paints with: Suhina Studio's when there is no such theme.
fn shown(themes: &Themes, id: &str) -> Vec<egui::Color32> {
    effective(&themes.get(id).map(|theme| theme.colors.clone()).unwrap_or_default())
}

fn night() -> ThemeEntry {
    let mut colors = theme::Colors::new();
    colors.insert("accent".into(), egui::Color32::from_rgb(1, 2, 3));
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
        assert!(view.info.iter().any(|(key, info)| key == id && !info.default_variant.is_empty()), "{id}");
    }
    assert!(!view.built_in);
    assert!(!themes.put(fake));
    assert_eq!(shown(&themes, BUILT_IN), effective(&theme::Colors::new()));
    assert!(themes.view("nord").built_in);
}

#[test]
fn a_missing_theme_draws_with_suhina_studio() {
    let themes = Themes::default();
    assert_eq!(shown(&themes, "gone"), effective(&theme::Colors::new()));
    assert_eq!(themes.view("gone").current, BUILT_IN);
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
fn a_saved_edit_of_a_built_in_variant_wins_and_goes_when_its_file_goes() {
    let mut themes = Themes::default();
    let original = shown(&themes, "nord@snow-storm");
    let mut edited = themes.get("nord@snow-storm").unwrap().clone();
    edited.colors.insert("accent".into(), egui::Color32::from_rgb(1, 2, 3));
    themes.set(vec![ThemeEntry { id: "nord@snow-storm".into(), theme: edited.clone() }], None);
    assert_eq!(shown(&themes, "nord@snow-storm"), effective(&edited.colors));
    assert_eq!(themes.view("nord").variants.iter().filter(|(id, _)| id == "nord@snow-storm").count(), 1);
    // The user's copy can be saved and reverted (deleted); the variant itself could not.
    assert!(themes.saveable("nord@snow-storm"));
    assert_eq!(themes.view("nord").overridden, ["nord@snow-storm"]);
    assert!(!themes.view("nord").locked.contains(&"nord@snow-storm".to_string()));
    assert_eq!(themes.request("nord", ThemeRequest::Delete("nord@snow-storm".into())), None);
    assert_eq!(themes.take_actions(), [FileAction::Trash("nord@snow-storm".into())]);
    themes.set(Vec::new(), None);
    assert!(!themes.saveable("nord@snow-storm"));
    assert!(themes.view("nord").locked.contains(&"nord@snow-storm".to_string()));
    assert_eq!(shown(&themes, "nord@snow-storm"), original);
}

/// A theme of its own with one quick control, `--tint` (a colour the accent follows), and a variant
/// of it, Warm.
fn ink() -> Themes {
    let sheet = "@property --tint { syntax: \"<color>\"; initial-value: #112233; label: \"Accent\"; }\n\
                 :root { --accent: var(--tint); }";
    let (ink, warnings) = theme::from_folder(theme::ThemeMeta::new("Ink", theme::DEFAULT_SWATCH), sheet);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut warm = ink.clone();
    warm.vars.variant = Some("warm".into());
    warm.vars.variant_name = Some("Warm".into());
    warm.vars.values.insert("--tint".into(), "#884400".into());
    crate::kit::theme_vars::resolve(&mut warm);
    let mut themes = Themes::default();
    themes.set(
        vec![ThemeEntry { id: "ink".into(), theme: ink }, ThemeEntry { id: "ink@warm".into(), theme: warm }],
        None,
    );
    themes
}

fn tint(value: &str) -> ThemeRequest {
    ThemeRequest::EditValue { name: "--tint".into(), value: value.into() }
}

const RED: egui::Color32 = egui::Color32::from_rgb(255, 0, 0);

#[test]
fn an_edit_is_a_draft_drawn_at_once_and_written_only_on_save() {
    let mut themes = ink();
    assert_eq!(themes.request("ink@warm", tint("#FF0000")), None, "the variant stays in use");
    assert_eq!(themes.get("ink@warm").unwrap().color("accent"), RED, "drawn with the edit");
    assert!(themes.edited("ink@warm") && themes.view("ink@warm").edited);
    assert!(themes.take_actions().is_empty(), "nothing is written while editing");
    // A value its property does not accept changes nothing.
    themes.request("ink@warm", tint("not a colour"));
    assert_eq!(themes.get("ink@warm").unwrap().color("accent"), RED);
    assert_eq!(themes.request("ink@warm", ThemeRequest::Save), None);
    let actions = themes.take_actions();
    assert!(
        matches!(actions.as_slice(), [FileAction::Save { id, theme }] if id == "ink@warm" && theme.color("accent") == RED),
        "{actions:?}"
    );
    assert!(!themes.edited("ink@warm"));
    assert_eq!(themes.get("ink@warm").unwrap().color("accent"), RED, "saved");
}

#[test]
fn discard_and_leaving_drop_the_draft() {
    let mut themes = ink();
    let warm = themes.get("ink@warm").unwrap().color("accent");
    themes.request("ink@warm", tint("#FF0000"));
    themes.request("ink@warm", ThemeRequest::Discard);
    assert_eq!(themes.get("ink@warm").unwrap().color("accent"), warm);
    assert!(!themes.edited("ink@warm"));
    themes.request("ink@warm", tint("#FF0000"));
    themes.end_draft();
    assert_eq!(themes.get("ink@warm").unwrap().color("accent"), warm);
    assert!(themes.take_actions().is_empty());
}

#[test]
fn the_default_variant_is_saved_only_as_a_new_variant() {
    let mut themes = ink();
    let own = themes.get("ink").unwrap().color("accent");
    themes.request("ink", tint("#FF0000"));
    assert!(themes.edited("ink") && !themes.saveable("ink") && !themes.view("ink").saveable);
    assert_eq!(themes.request("ink", ThemeRequest::Save), None);
    assert!(themes.take_actions().is_empty(), "the theme's own values are not written");
    assert_eq!(themes.request("ink", ThemeRequest::SaveAsNew), Some("ink@default-copy".into()));
    assert_eq!(themes.get("ink@default-copy").unwrap().color("accent"), RED);
    assert_eq!(themes.view("ink@default-copy").variant.as_deref(), Some("Default copy"));
    assert_eq!(themes.get("ink").unwrap().color("accent"), own, "the theme keeps its values");
    assert_eq!(saved(&mut themes), [("ink@default-copy".into(), "Ink".into())]);
}

#[test]
fn save_as_new_variant_takes_the_name_typed() {
    let mut themes = ink();
    themes.request("ink@warm", tint("#FF0000"));
    themes.request("ink@warm", ThemeRequest::EditName("Hot".into()));
    assert_eq!(themes.view("ink@warm").variant_name, "Hot");
    assert_eq!(themes.request("ink@warm", ThemeRequest::SaveAsNew), Some("ink@hot".into()));
    assert_eq!(themes.view("ink@warm").variant.as_deref(), Some("Warm"), "the original is as it was");
    assert_ne!(themes.get("ink@warm").unwrap().color("accent"), RED);
    assert_eq!(themes.get("ink@hot").unwrap().color("accent"), RED);
}

#[test]
fn a_renamed_variant_is_edited_and_saved_with_its_name() {
    let mut themes = ink();
    themes.request("ink@warm", ThemeRequest::EditName("Ember".into()));
    assert!(themes.edited("ink@warm"));
    assert!(themes.view("ink@warm").variants.contains(&("ink@warm".to_string(), "Ember".to_string())));
    themes.request("ink@warm", ThemeRequest::EditName("Warm".into()));
    assert!(!themes.edited("ink@warm"), "typing the saved name back is no change");
    themes.request("ink@warm", ThemeRequest::EditName("Ember".into()));
    themes.request("ink@warm", ThemeRequest::Save);
    let actions = themes.take_actions();
    assert!(
        matches!(actions.as_slice(), [FileAction::Save { theme, .. }] if theme.vars.variant_name.as_deref() == Some("Ember")),
        "{actions:?}"
    );
}

#[test]
fn a_draft_outlives_its_file_being_read_again_but_not_its_file_going() {
    let mut themes = ink();
    themes.request("ink@warm", tint("#FF0000"));
    // The folder read again, the variant's file as it was: the edit stays over it.
    let entries: Vec<ThemeEntry> = ink().entries[ink().built_in..].to_vec();
    themes.set(entries.clone(), None);
    assert_eq!(themes.get("ink@warm").unwrap().color("accent"), RED);
    assert!(themes.edited("ink@warm"));
    // The variant's file gone: the draft goes with it, and the status bar says so.
    themes.set(entries.into_iter().filter(|e| e.id == "ink").collect(), None);
    assert!(themes.get("ink@warm").is_none() && !themes.edited("ink@warm"));
    assert!(themes.take_notice().is_some_and(|n| n.contains("unsaved changes were dropped")));
}

#[test]
fn a_built_in_variant_is_edited_but_never_saved_in_place() {
    let mut themes = Themes::default();
    assert!(themes.view("nord").locked.contains(&"nord@snow-storm".to_string()));
    let mut skin = themes.get("nord@snow-storm").unwrap().skin.clone();
    skin.segments = 9;
    // The toolbar editor's settings still go to a variant of the user's (Custom).
    assert_eq!(themes.request("nord", ThemeRequest::EditSkin(skin)), Some("nord@custom".into()));
    themes.take_actions();
    assert!(!themes.saveable("nord@snow-storm"));
    for request in [
        ThemeRequest::Save,
        ThemeRequest::Delete("nord@snow-storm".into()),
        ThemeRequest::Delete("nord".into()),
        ThemeRequest::EditInfo { id: "nord".into(), info: ThemeInfo { name: "Mine".into(), ..Default::default() } },
        ThemeRequest::AddDefaultArtwork("nord".into()),
    ] {
        assert_eq!(themes.request("nord@snow-storm", request.clone()), None, "{request:?}");
        assert!(themes.take_actions().is_empty(), "{request:?}");
    }
    // A variant the user added to it is the user's.
    assert!(themes.saveable("nord@custom"));
    assert_eq!(themes.request("nord", ThemeRequest::Delete("nord@custom".into())), None);
    assert_eq!(themes.take_actions(), [FileAction::Trash("nord@custom".into())]);
}

#[test]
fn deleting_waits_for_the_file_to_go() {
    let mut themes = ink();
    themes.request("ink@warm", ThemeRequest::Delete("ink".into()));
    assert_eq!(themes.take_actions(), [FileAction::Trash("ink".into())]);
    assert!(themes.get("ink").is_some(), "it stays until the folder is read again without it");
    themes.request("ink@warm", ThemeRequest::Delete("ink@warm".into()));
    assert_eq!(themes.take_actions(), [FileAction::Trash("ink@warm".into())]);
    themes.request("ink@warm", ThemeRequest::Delete("ink@nothing".into()));
    assert!(themes.take_actions().is_empty());
}

#[test]
fn a_new_variant_is_written_only_when_saved() {
    let mut themes = ink();
    // It starts as a draft of the variant it is made from, which is used.
    assert_eq!(themes.request("ink", ThemeRequest::NewVariant("ink@warm".into())), Some("ink@warm".into()));
    assert!(themes.take_actions().is_empty(), "nothing is written before Save");
    assert!(themes.edited("ink@warm"), "a new variant counts as unsaved");
    assert!(themes.view("ink@warm").new_variant);
    assert_eq!(themes.view("ink@warm").variant_name, "New variant");
    // Cancel leaves nothing behind.
    themes.request("ink@warm", ThemeRequest::Discard);
    assert!(!themes.edited("ink@warm"));
    assert_eq!(themes.view("ink@warm").variant_name, "Warm");
    assert!(themes.get("ink@new-variant").is_none());

    // Save makes it a variant of its own, with the values it started from and those changed.
    themes.request("ink@warm", ThemeRequest::NewVariant("ink@warm".into()));
    let warm = themes.get("ink@warm").unwrap().color("accent");
    themes.request("ink@warm", tint("#ff0000"));
    assert_eq!(themes.request("ink@warm", ThemeRequest::SaveAsNew), Some("ink@new-variant".into()));
    assert_eq!(saved(&mut themes), [("ink@new-variant".into(), "Ink".into())]);
    assert_eq!(themes.view("ink@new-variant").variant.as_deref(), Some("New variant"));
    assert_eq!(themes.get("ink@new-variant").unwrap().color("accent"), RED);
    assert_eq!(themes.get("ink@warm").unwrap().color("accent"), warm, "the variant it started from is as it was");
    assert!(!themes.edited("ink@warm"));
    // A second is numbered as its id is, and keeps the name typed.
    themes.request("ink", ThemeRequest::NewVariant("ink".into()));
    assert_eq!(themes.view("ink").variant_name, "New variant 2");
    themes.request("ink", ThemeRequest::EditName("Ember".into()));
    assert_eq!(themes.request("ink", ThemeRequest::SaveAsNew), Some("ink@ember".into()));

    assert_eq!(themes.request("ink", ThemeRequest::DuplicateVariant("ink@warm".into())), None);
    assert_eq!(themes.view("ink@warm-copy").variant.as_deref(), Some("Warm copy"));
}

#[test]
fn new_theme_from_a_theme_or_suhinas_defaults() {
    let mut themes = ink();
    let new = |from: Option<&str>, name: &str, artwork: bool| {
        ThemeRequest::NewTheme(NewTheme {
            from: from.map(str::to_owned),
            info: ThemeInfo {
                name: name.into(),
                author: " Ari ".into(),
                description: "Warm ink".into(),
                default_variant: String::new(),
            },
            artwork,
        })
    };
    let id = themes.request("ink", new(Some("ink@warm"), "Ember", false)).unwrap();
    assert_eq!(id, "ember");
    let ember = themes.get("ember").unwrap();
    assert_eq!(ember.name, "Ember");
    assert_eq!((ember.meta.author.as_deref(), ember.meta.description.as_deref()), (Some("Ari"), Some("Warm ink")));
    assert_eq!(ember.meta.default_variant(), "Warm", "made from a variant, its values are the theme's own");
    assert_eq!(ember.color("accent"), themes.get("ink@warm").unwrap().color("accent"));
    assert!(ember.vars.variant.is_none());
    assert!(themes.take_actions().iter().any(|a| matches!(a, FileAction::Save { id, .. } if id == "ember")));
    // From Suhina's defaults: the user-agent sheet, named and numbered as its id is when unnamed.
    let blank = themes.request("ink", new(None, "", false)).unwrap();
    assert_eq!(blank, "new-theme");
    assert_eq!(themes.get(&blank).unwrap().skin.css.as_deref(), Some(theme::NEW_THEME_SHEET));
    assert!(themes.get(&blank).unwrap().skin.assets.is_empty());
    let with = themes.request("ink", new(None, "", true)).unwrap();
    assert_eq!((with.as_str(), themes.get(&with).unwrap().name.as_str()), ("new-theme-2", "New theme 2"));
    assert_eq!(themes.get(&with).unwrap().skin.assets.len(), crate::kit::skin::roles().count());
    // From a built-in theme: a copy under a free id, the built-in one untouched.
    assert_eq!(themes.request("ink", new(Some("nord"), "Nord", false)), Some("nord-2".into()));
    assert!(themes.get("nord").is_some_and(|t| t.meta.author.as_deref() != Some("Ari")));
    assert_eq!(themes.get("nord-2").and_then(|t| t.meta.author.as_deref()), Some("Ari"));
}

#[test]
fn theme_info_writes_theme_json_and_keeps_the_folder() {
    let mut themes = ink();
    let info = ThemeInfo {
        name: " Nord ".into(),
        author: "Ari".into(),
        description: String::new(),
        default_variant: "Night".into(),
    };
    themes.request("ink", ThemeRequest::EditInfo { id: "ink@warm".into(), info: info.clone() });
    assert_eq!(saved(&mut themes), [("ink".into(), "Nord".into())]);
    for id in ["ink", "ink@warm"] {
        let theme = themes.get(id).unwrap();
        assert_eq!((theme.name.as_str(), theme.meta.author.as_deref()), ("Nord", Some("Ari")), "{id}");
        assert_eq!(theme.meta.default_variant(), "Night");
        assert_eq!(theme.meta.description, None);
    }
    assert!(themes.get("nord").is_some_and(|t| t.name == "Nord"), "the built-in Nord is untouched");
    assert_eq!(themes.view("ink").info.iter().find(|(id, _)| id == "ink").map(|(_, i)| i.name.as_str()), Some("Nord"));
    // An empty name is not a name.
    themes.request("ink", ThemeRequest::EditInfo { id: "ink".into(), info: ThemeInfo { name: " ".into(), ..info } });
    assert!(themes.take_actions().is_empty());
}

#[test]
fn using_and_file_actions() {
    let mut themes = Themes::default();
    themes.set(vec![night()], None);
    assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("night".into())), Some("night".into()));
    assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("nothing".into())), None);
    for request in [
        ThemeRequest::OpenFolder,
        ThemeRequest::Backup,
        ThemeRequest::Backup,
        ThemeRequest::Reveal("night".into()),
        ThemeRequest::Import,
    ] {
        assert_eq!(themes.request(BUILT_IN, request), None);
    }
    themes.import("night.json".into());
    assert_eq!(
        themes.take_actions(),
        [
            FileAction::OpenFolder,
            FileAction::Backup,
            FileAction::Reveal("night".into()),
            FileAction::Import("night.json".into())
        ]
    );
    themes.request(BUILT_IN, ThemeRequest::ExportPackage("night".into()));
    assert!(matches!(themes.take_actions().as_slice(), [FileAction::ExportPackage { id, .. }] if id == "night"));
}

#[test]
fn moving_a_control_of_a_built_in_theme_copies_it() {
    let mut themes = Themes::default();
    let mut skin = themes.get("nord@snow-storm").unwrap().skin.clone();
    skin.toolbar_offsets.insert("transport".into(), 40);
    assert_eq!(themes.request("nord@snow-storm", ThemeRequest::EditSkin(skin)), Some("nord-custom".into()));
    assert_eq!(
        themes.get("nord-custom").unwrap().color("accent"),
        themes.get("nord@snow-storm").unwrap().color("accent"),
        "the copy keeps the variant's colours"
    );
}

/// A toolbar change to a built-in theme copies the theme as saved: the variant editor's unsaved
/// edits are not written into the copy.
#[test]
fn a_toolbar_copy_of_a_built_in_theme_leaves_the_unsaved_draft_out() {
    let mut themes = Themes::default();
    let saved = themes.get("nord@snow-storm").unwrap().color("accent");
    // Accent, the control that colours `accent`.
    let name = "--tint".to_string();
    let value = "#ff0000";
    assert_ne!(saved, RED, "Nord's accent is not red to begin with");
    themes.request("nord@snow-storm", ThemeRequest::EditValue { name, value: value.into() });
    assert_eq!(themes.get("nord@snow-storm").unwrap().color("accent"), RED, "the draft is drawn");
    assert!(themes.edited("nord@snow-storm"));
    let mut skin = themes.get("nord@snow-storm").unwrap().skin.clone();
    skin.toolbar_offsets.insert("transport".into(), 40);
    let copy = themes.request("nord@snow-storm", ThemeRequest::EditSkin(skin));
    assert_eq!(copy.as_deref(), Some("nord-custom"));
    let saves: Vec<_> = themes
        .take_actions()
        .into_iter()
        .filter_map(|a| match a {
            FileAction::Save { id, theme } => Some((id, theme)),
            _ => None,
        })
        .collect();
    assert_eq!(saves.len(), 1, "{:?}", saves.iter().map(|(id, _)| id).collect::<Vec<_>>());
    assert_eq!(saves[0].0, "nord-custom");
    assert_eq!(saves[0].1.skin.toolbar_offsets.get("transport"), Some(&40), "the toolbar change is written");
    assert_eq!(saves[0].1.color("accent"), saved, "the unsaved edit is not");
}

#[test]
fn a_sheet_with_notices_is_named_with_them() {
    let (theme, _) =
        theme::from_folder(theme::ThemeMeta::new("Odd", theme::DEFAULT_SWATCH), "browser { font-family: 'a'; }");
    let mut themes = Themes::default();
    themes.set(vec![ThemeEntry { id: "odd".into(), theme }, night()], None);
    let view = themes.view(BUILT_IN);
    assert_eq!(view.notices.len(), 1, "{:?}", view.notices);
    let (id, notices) = &view.notices[0];
    assert_eq!(id, "odd");
    assert!(notices[0].starts_with("Line 1, column"), "{notices:?}");
}

/// Add default artwork adds only what is missing, to the theme and each of its variants, and
/// saves the theme; a theme with all of it is left as it is with a notice.
#[test]
fn add_default_artwork_adds_only_what_is_missing() {
    let mut themes = ink();
    assert_eq!(themes.request("ink@warm", ThemeRequest::AddDefaultArtwork("ink@warm".into())), None);
    let all = crate::kit::skin::roles().count();
    for id in ["ink", "ink@warm"] {
        assert_eq!(themes.get(id).unwrap().skin.assets.len(), all, "{id}");
    }
    let actions = themes.take_actions();
    assert!(actions.iter().any(|action| matches!(action, FileAction::Save { id, .. } if id == "ink")), "{actions:?}");
    assert_eq!(themes.take_notice().as_deref(), Some("Added 17 default images to the theme's assets folder"));
    // Again: nothing is missing.
    assert_eq!(themes.request("ink", ThemeRequest::AddDefaultArtwork("ink".into())), None);
    assert!(themes.take_actions().is_empty());
    assert_eq!(themes.take_notice().as_deref(), Some("This theme already has all of the default artwork"));
}

/// The toolbar editor saves what the file holds plus its change, never the variant editor's
/// unsaved draft, and the draft is made again over the changed entry.
#[test]
fn a_toolbar_edit_during_a_draft_saves_the_entry_not_the_draft() {
    let mut themes = ink();
    themes.request("ink@warm", tint("#ff0000"));
    let mut skin = themes.get("ink@warm").unwrap().skin.clone();
    skin.segments = 9;
    themes.request("ink@warm", ThemeRequest::EditSkin(skin));
    let saves: Vec<_> = themes
        .take_actions()
        .into_iter()
        .filter_map(|a| match a {
            FileAction::Save { id, theme } => Some((id, theme.skin.segments, theme.color("accent"))),
            _ => None,
        })
        .collect();
    assert_eq!(saves.len(), 1, "{saves:?}");
    assert_eq!(saves[0].0, "ink@warm");
    assert_eq!(saves[0].1, 9, "the toolbar change is written");
    assert_ne!(saves[0].2, RED, "the unsaved colour is not");
    let drawn = themes.get("ink@warm").unwrap();
    assert_eq!((drawn.skin.segments, drawn.color("accent")), (9, RED), "the draft keeps both");

    // A new variant's draft stays a draft: the variant it started from keeps its name.
    themes.request("ink@warm", ThemeRequest::Discard);
    themes.request("ink@warm", ThemeRequest::NewVariant("ink@warm".into()));
    let skin = themes.get("ink@warm").unwrap().skin.clone();
    themes.request("ink@warm", ThemeRequest::EditSkin(crate::kit::skin::Skin { segments: 7, ..skin }));
    let names: Vec<_> = themes
        .take_actions()
        .into_iter()
        .filter_map(|a| match a {
            FileAction::Save { theme, .. } => theme.vars.variant_name,
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Warm"]);
}

/// An emptied Name keeps the name the variant has.
#[test]
fn an_emptied_name_is_not_a_change_and_is_not_written() {
    let mut themes = ink();
    themes.request("ink@warm", ThemeRequest::EditName("  ".into()));
    assert!(!themes.edited("ink@warm"));
    themes.request("ink@warm", tint("#ff0000"));
    themes.request("ink@warm", ThemeRequest::Save);
    let names: Vec<_> = themes
        .take_actions()
        .into_iter()
        .filter_map(|a| match a {
            FileAction::Save { theme, .. } => theme.vars.variant_name,
            _ => None,
        })
        .collect();
    assert_eq!(names, ["Warm"]);
}

/// Only what Settings moved to the Trash counts as gone on purpose.
#[test]
fn a_theme_moved_to_the_trash_is_known_as_trashed() {
    let mut themes = ink();
    assert!(!themes.trashed("ink"));
    themes.request("ink", ThemeRequest::Delete("ink".into()));
    assert!(themes.trashed("ink"));
    assert!(!themes.trashed("ink@warm"));
}

/// Theme info leaves the default variant's name unset when it was and still reads "Default".
#[test]
fn theme_info_writes_no_default_variant_name_it_did_not_have() {
    let mut themes = ink();
    let info = themes.view("ink").info.into_iter().find(|(id, _)| id == "ink").unwrap().1;
    assert_eq!(info.default_variant, "Default");
    themes.request("ink", ThemeRequest::EditInfo { id: "ink".into(), info: ThemeInfo { author: "Me".into(), ..info } });
    assert_eq!(themes.get("ink").unwrap().meta.default_variant, None);
    assert_eq!(themes.get("ink").unwrap().meta.author.as_deref(), Some("Me"));
}

/// Another theme used without the editor drops the draft; the draft's own theme keeps it.
#[test]
fn using_another_theme_drops_a_draft_of_this_one() {
    let mut themes = ink();
    themes.request("ink@warm", tint("#ff0000"));
    themes.keep_draft_of("ink@warm");
    assert!(themes.edited("ink@warm"));
    themes.keep_draft_of("ink");
    assert!(!themes.edited("ink@warm"));
}

/// A user's copy of a shipped variant read alone (not with the whole folder) is the user's too.
#[test]
fn a_users_copy_of_a_shipped_variant_read_alone_can_be_saved() {
    let mut themes = Themes::default();
    assert!(!themes.saveable("nord@snow-storm"));
    let theme = themes.get("nord@snow-storm").unwrap().clone();
    let mut copy = theme.clone();
    copy.vars.variant_name = Some("Mine".into());
    themes.put(ThemeEntry { id: "nord@snow-storm".into(), theme: copy });
    assert!(themes.saveable("nord@snow-storm"));
    assert!(themes.view("nord").overridden.contains(&"nord@snow-storm".to_string()));
}
