// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every bundled theme, and the see-through test theme, is drawn in every scene: the scenes find
//! the controls they drive whatever the theme. Headless: the renders themselves need a GPU and are
//! in `render.rs`.

use super::see_through;
use suhina_theme_render::{Subject, headless, scenes};

#[test]
fn every_bundled_theme_is_driven_through_every_scene() {
    let mut themes = Subject::built_ins();
    assert!(themes.len() >= 10, "the built-in themes are found: {}", themes.len());
    themes.push(see_through().unwrap());
    // A theme to a thread: each window and the style engine it uses are the thread's own.
    let failed: Vec<String> = std::thread::scope(|scope| {
        let runs: Vec<_> = themes.iter().map(|theme| scope.spawn(move || drive_every_scene(theme))).collect();
        runs.into_iter().flat_map(|run| run.join().unwrap_or_else(|_| vec!["a thread panicked".into()])).collect()
    });
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Each scene opened with `theme` and driven; what could not be done in each.
fn drive_every_scene(theme: &Subject) -> Vec<String> {
    let mut failed = Vec::new();
    for scene in scenes::scenes() {
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
    for wanted in
        ["launcher", "launcher-hover", "launcher-selected", "arrangement", "piano-roll", "menu", "settings", "tooltip"]
    {
        assert!(names.iter().any(|n| n == wanted), "{wanted} in {names:?}");
    }
    assert!(names.iter().any(|n| n == "devices"), "every device in a row");
    assert!(names.iter().any(|n| n == "expanded-fm-synth"), "an expanded panel: {names:?}");
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "each scene has a name of its own: {names:?}");
}
