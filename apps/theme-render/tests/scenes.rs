// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The scenes find the controls they drive whatever the theme: every bundled theme that lays out,
//! and the see-through test theme, is drawn in every scene, and every other bundled theme, which
//! sets only colours, in one. Headless: the renders themselves need a GPU and are in `render.rs`.

use std::path::Path;

use super::see_through;
use suhina_theme_render::{Subject, headless, scenes};

/// The bundled themes whose sheets size or place what they style, or draw it with artwork of their
/// own: their rules can move or hide a control a scene drives. The others set colours alone, which
/// cannot, and driving them through all 50 scenes too took 550 windows and two minutes of a test
/// run (2026-11-18). `the_other_bundled_themes_set_only_colours` keeps the list true.
const LAYOUT_THEMES: [&str; 3] = ["suhina-sheet", "suhina-meadow", "suhina-blocks"];

/// The CSS properties that size or place what a sheet styles.
const LAYOUT_PROPERTIES: [&str; 12] = [
    "display",
    "width",
    "height",
    "padding",
    "margin",
    "gap",
    "font-size",
    "visibility",
    "position",
    "order",
    "flex",
    "grid",
];

#[test]
fn every_theme_that_lays_out_is_driven_through_every_scene() {
    let mut themes: Vec<Subject> = LAYOUT_THEMES
        .iter()
        .map(|id| Subject::built_in(id).unwrap_or_else(|| panic!("{id} is a built-in theme")))
        .collect();
    themes.push(see_through().unwrap());
    // A theme to a thread: each window and the style engine it uses are the thread's own.
    let failed: Vec<String> = std::thread::scope(|scope| {
        let runs: Vec<_> = themes.iter().map(|theme| scope.spawn(move || drive_every_scene(theme))).collect();
        runs.into_iter().flat_map(|run| run.join().unwrap_or_else(|_| vec!["a thread panicked".into()])).collect()
    });
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A theme that sets only colours is chosen and drives the launcher.
#[test]
fn every_theme_that_sets_only_colours_is_driven_through_the_launcher() {
    let themes = Subject::built_ins();
    assert!(themes.len() >= 10, "the built-in themes are found: {}", themes.len());
    let launcher: Vec<_> = scenes::scenes().into_iter().filter(|scene| scene.name == "launcher").collect();
    assert_eq!(launcher.len(), 1, "the launcher scene");
    let failed: Vec<String> = themes
        .iter()
        .filter(|theme| !LAYOUT_THEMES.contains(&theme.id.as_str()))
        .flat_map(|theme| drive_scenes(theme, &launcher))
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The themes left out of [`LAYOUT_THEMES`] declare no property that sizes or places what they
/// style, and carry no artwork: a theme that starts to goes in the list.
#[test]
fn the_other_bundled_themes_set_only_colours() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/ui/themes");
    let ids: Vec<String> = Subject::built_ins().into_iter().map(|theme| theme.id).collect();
    for id in LAYOUT_THEMES {
        assert!(ids.iter().any(|built_in| built_in == id), "{id} is a built-in theme");
    }
    for id in ids.iter().filter(|id| !LAYOUT_THEMES.contains(&id.as_str())) {
        let sheet = std::fs::read_to_string(folder.join(id).join("theme.css")).unwrap();
        for property in LAYOUT_PROPERTIES {
            let declared = sheet.match_indices(&format!("{property}:")).any(|(at, _)| {
                !sheet[..at].chars().next_back().is_some_and(|before| before.is_alphanumeric() || before == '-')
            });
            assert!(!declared, "{id} declares `{property}`: list it in LAYOUT_THEMES");
        }
        assert!(!folder.join(id).join("assets").exists(), "{id} draws with artwork: list it in LAYOUT_THEMES");
    }
}

/// Each scene opened with `theme` and driven; what could not be done in each.
fn drive_every_scene(theme: &Subject) -> Vec<String> {
    drive_scenes(theme, &scenes::scenes())
}

/// `scenes`, each opened with `theme` and driven; what could not be done in each.
fn drive_scenes(theme: &Subject, list: &[scenes::Scene]) -> Vec<String> {
    let mut failed = Vec::new();
    for scene in list {
        let app = match (scene.app)() {
            Ok(app) => app,
            Err(e) => {
                failed.push(format!("{} in {}: {e}", theme.id, scene.name));
                continue;
            }
        };
        let mut h = headless(app, theme, scene.width);
        assert_eq!(h.state().settings().theme, theme.id, "{} is chosen", theme.id);
        if let Err(e) = (scene.drive)(&mut h) {
            failed.push(format!("{} in {}: {e}", theme.id, scene.name));
        }
    }
    failed
}

#[test]
fn the_scenes_cover_the_views_the_devices_and_the_floating_surfaces() {
    let names: Vec<String> = scenes::scenes().into_iter().map(|s| s.name).collect();
    for wanted in [
        "launcher",
        "launcher-hover",
        "launcher-selected",
        "arrangement",
        "piano-roll",
        "menu",
        "context-menu",
        "settings",
        "settings-menu",
        "tooltip",
        "export-dialog",
        "about-dialog",
        "settings-audio",
        "settings-plug-ins",
        "settings-keys",
        "settings-themes",
        "project-dialog",
        "unsaved-dialog",
        "quick-add",
        "missing-files",
        "start-panel",
        "control-bar-toggles",
        "grid-menu",
    ] {
        assert!(names.iter().any(|n| n == wanted), "{wanted} in {names:?}");
    }
    assert!(names.iter().any(|n| n == "launcher-scrolled"), "the launcher scrolled: {names:?}");
    assert!(names.iter().any(|n| n == "arrangement-scrolled"), "the Arrangement view scrolled: {names:?}");
    assert!(names.iter().any(|n| n == "devices"), "every device in a row");
    assert!(names.iter().any(|n| n == "expanded-fm-synth"), "an expanded panel: {names:?}");
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "each scene has a name of its own: {names:?}");
}

/// The fixture's and some installed plug-ins' names that the browser's plug-in list shows.
fn plug_in_names(scene: &str) -> Result<Vec<String>, String> {
    use egui_kittest::kittest::Queryable;
    let scene = scenes::find(scene).ok_or("no such scene")?;
    let theme = Subject::built_in("suhina-studio").ok_or("no such theme")?;
    let mut h = headless((scene.app)()?, &theme, scene.width);
    (scene.drive)(&mut h)?;
    let mut shown: Vec<String> = ["Test Synth", "Test Sampler", "Test Delay", "Lemon Sampler", "Surge XT", "Vital"]
        .into_iter()
        .filter(|name| h.query_all_by_label_contains(name).next().is_some())
        .map(str::to_owned)
        .collect();
    shown.sort();
    Ok(shown)
}

/// #1438: the scenes started from `App::new()`, which reads the machine's own plug-in catalogue,
/// so the plug-in list drew the installed plug-ins here and none on CI.
#[test]
fn the_plug_in_scene_lists_the_fixture_plug_ins_and_nothing_of_the_machine() {
    assert_eq!(plug_in_names("browser-plug-ins").unwrap(), ["Test Sampler", "Test Synth"]);
}

/// The catalogue the plug-in lists draw from is the fixture's alone: nothing a scan of the
/// machine's plug-in folders or its settings folder left.
#[test]
fn the_scenes_plug_in_catalogue_is_the_fixture_alone() {
    let mut app = suhina_theme_render::fixed_app();
    let mut names: Vec<String> = app.projects_mut().plugin_env().catalog.plugins().map(|p| p.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["Test Delay", "Test Sampler", "Test Synth"]);
}
