// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The texts each built-in theme and variant cuts off are those recorded in `cut/<theme>.txt` (#1548): a
//! label shortened with "…", wrapped onto another line, or painted partly outside its clip
//! ([`suhina_theme_render::cut`]). Headless, on the drawings the painted test makes
//! ([`super::painted::Drawings`]), once per theme for both checks.
//!
//! Some cuts are meant (a long path in a dialog, a name in a narrow column) and some are bugs not
//! yet fixed, so the test holds the list rather than an empty one: a change that cuts a new text,
//! or stops cutting one, fails until the list is recorded again with
//! `SUHINA_BLESS=1 cargo test -p suhina-theme-render --test main painted`, and the pull request says
//! why. A theme's cut is listed once, under the first scene that shows it by name: a text the
//! toolbar cuts in every scene is one line.
//!
//! The scrolled scenes are left out of the clipped texts, since a view scrolled along cuts what it
//! scrolls at its edge on purpose.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use suhina_theme_render::cut::{Cut, How};
use suhina_theme_render::scenes;

fn file(theme: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cut").join(format!("{theme}.txt"))
}

/// Whether `cut` in `scene` is one the test holds.
fn held(scene: &str, cut: &Cut) -> bool {
    cut.how != How::Clipped || !scenes::SCROLLED.iter().any(|(name, _)| *name == scene)
}

/// The file's lines for `drawings`: `theme scene: cut`, for each theme's cuts, under the first
/// scene that shows each.
fn lines<'a>(drawings: impl Iterator<Item = (&'a (String, String), &'a [Cut])>) -> Vec<String> {
    let mut first: BTreeMap<(&str, String), &str> = BTreeMap::new();
    for ((scene, theme), cuts) in drawings {
        for cut in cuts.iter().filter(|cut| held(scene, cut)) {
            let at = first.entry((theme.as_str(), cut.to_string())).or_insert(scene.as_str());
            if scene.as_str() < *at {
                *at = scene.as_str();
            }
        }
    }
    first.into_iter().map(|((theme, cut), scene)| format!("{theme} {scene}: {cut}")).collect()
}

/// Checked by the painted test (`painted.rs`), which draws the scenes once for both.
#[allow(clippy::unwrap_used)] // called by the painted test, outside a #[test] function
pub fn cuts_off_only_the_texts_recorded(theme: &str, all: &super::painted::Drawings) {
    let file = file(theme);
    let failed: Vec<String> = all
        .iter()
        .filter_map(|((scene, theme), drawing)| drawing.as_ref().err().map(|e| format!("{scene} in {theme}: {e}")))
        .collect();
    assert!(failed.is_empty(), "these drawings failed:\n{}", failed.join("\n"));
    let now = lines(all.iter().filter_map(|(key, drawing)| Some((key, drawing.as_ref().ok()?.cut.as_slice()))));
    let mut text = String::from(
        "# The texts each built-in theme cuts off, drawn as on Linux on every system: `theme scene: how\n\
         # \"text\"`, under the first scene that shows it. Written by\n\
         # `SUHINA_BLESS=1 cargo test -p suhina-theme-render --test main painted` (tests/cut.rs).\n",
    );
    for line in &now {
        text.push_str(line);
        text.push('\n');
    }
    if suhina_test_support::blessing() {
        std::fs::write(&file, text).unwrap();
        return;
    }
    let recorded = std::fs::read_to_string(&file).unwrap();
    let recorded: Vec<&str> = recorded.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).collect();
    let added: Vec<&String> = now.iter().filter(|line| !recorded.contains(&line.as_str())).collect();
    let gone: Vec<&&str> = recorded.iter().filter(|line| !now.iter().any(|n| n == **line)).collect();
    assert!(
        added.is_empty() && gone.is_empty(),
        "the texts cut off differ from {} (see tests/cut.rs for how to record them).\n\
         Cut now, not recorded:\n{}\nRecorded, not cut now:\n{}\n\n\
         The file as this run draws it, between the lines:\n----- {} -----\n{text}----- end -----",
        file.display(),
        added.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"),
        gone.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"),
        file.display(),
    );
}

#[test]
fn a_theme_s_cut_is_listed_once_under_its_first_scene() {
    let cut = |how, text: &str, shown: &str| Cut { how, text: text.into(), shown: shown.into() };
    let quantize = [cut(How::Shortened, "Quantize", "Qu…")];
    let edge = [cut(How::Clipped, "B", "B")];
    let key = |scene: &str, theme: &str| (scene.to_owned(), theme.to_owned());
    let keys = [
        key("menu", "nord"),
        key("launcher", "nord"),
        key("launcher", "suhina-studio"),
        key("launcher-scrolled", "nord"),
    ];
    let drawings = [&quantize[..], &quantize[..], &quantize[..], &edge[..]];
    let lines = lines(keys.iter().zip(drawings));
    // The scrolled scene's clipped text is not held.
    assert_eq!(
        lines,
        [
            "nord launcher: shortened \"Quantize\" as \"Qu…\"",
            "suhina-studio launcher: shortened \"Quantize\" as \"Qu…\""
        ]
    );
}
