// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The renderer's tests, in one binary (as `crates/ui/tests/main.rs` says why): a module per file.
//! `budget` and `scenes` run headless; `render` needs a GPU and runs only when asked.

use std::path::Path;

use suhina_theme_render::Subject;

mod budget;
mod render;
mod scenes;

/// The see-through test theme beside this crate (translucent colours everywhere, and round button
/// and fader artwork), with the warnings from reading it.
fn see_through_with_warnings() -> Result<(Subject, Vec<String>), String> {
    Subject::folder(&Path::new(env!("CARGO_MANIFEST_DIR")).join("themes/see-through"))
}

/// The see-through test theme.
fn see_through() -> Result<Subject, String> {
    see_through_with_warnings().map(|(subject, _)| subject)
}

#[test]
fn the_see_through_test_theme_reads_without_warnings() {
    let (subject, warnings) = see_through_with_warnings().unwrap();
    assert!(subject.theme.is_some());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn every_test_file_is_a_module() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let main = std::fs::read_to_string(folder.join("main.rs")).unwrap();
    for entry in std::fs::read_dir(&folder).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        if let Some(module) = name.strip_suffix(".rs").filter(|m| *m != "main") {
            assert!(main.contains(&format!("mod {module};")), "tests/main.rs lists {module}");
        }
    }
}
