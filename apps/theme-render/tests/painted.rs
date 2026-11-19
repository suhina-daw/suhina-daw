// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every built-in theme and variant paints each scene as recorded in `painted/<theme>.txt` (#1023): what
//! moves the views onto elements, or the colours into the sheet, changes no theme's look unless
//! it means to. Headless: the digest of [`painted::scene`]'s listing, no GPU.
//!
//! A change that means to change a look records the new digests:
//! `SUHINA_BLESS=1 cargo test -p suhina-theme-render --test main painted`, and says why in its pull
//! request. To see what changed, write the listings before and after the change with
//! `SUHINA_PAINTED_DUMP=<folder>` and compare the two folders.
//!
//! The same recordings on every system: the scenes are drawn with the window looking like Linux
//! ([`suhina_ui::kit::platform`]), so shortcuts say Ctrl and Settings names Linux's plug-in formats
//! on a Mac and on Windows too, and no system draws the title bar Suhina draws on Windows. A
//! recording made on any system holds on the others.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use suhina_theme_render::painted::Drawing;
use suhina_theme_render::{Subject, painted, scenes};

/// The scenes left out: the About dialog shows the build's commit.
const LEFT_OUT: [&str; 1] = ["about-dialog"];

pub type Drawings = BTreeMap<(String, String), Result<Drawing, String>>;

fn file(theme: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/painted").join(format!("{theme}.txt"))
}

/// The recorded digests: `scene theme digest` a line.
fn recorded(text: &str) -> BTreeMap<(String, String), String> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some(((words.next()?.to_owned(), words.next()?.to_owned()), words.next()?.to_owned()))
        })
        .collect()
}

/// One theme per test lets nextest spread the drawings across CI partitions. Each scene still
/// gets a fresh thread for its window state in `painted::scene`; there is no worker pool here
/// competing with the test runner. The same drawings check both recordings, even if one differs.
#[allow(clippy::unwrap_used)] // test helper
fn paint_and_check(theme: &str) {
    let subject = Subject::built_in(theme).unwrap();
    let all: Drawings = scenes::scenes()
        .into_iter()
        .filter(|scene| !LEFT_OUT.contains(&scene.name.as_str()))
        .map(|scene| ((scene.name.clone(), theme.to_owned()), painted::scene(&subject, &scene)))
        .collect();
    let painted = std::panic::catch_unwind(|| paints_each_scene_as_recorded(theme, &all));
    super::cut::cuts_off_only_the_texts_recorded(theme, &all);
    if let Err(panic) = painted {
        std::panic::resume_unwind(panic);
    }
}

// The table declares both the tests and the IDs the coverage test checks, so a theme cannot be
// listed as covered without having a runnable test. Variants are independent cases too.
macro_rules! theme_tests {
    ($($name:ident => $id:literal),+ $(,)?) => {
        const TESTED_THEMES: &[&str] = &[$($id),+];

        mod themes {
            $(
                #[test]
                fn $name() {
                    super::paint_and_check($id);
                }
            )+
        }
    };
}

theme_tests! {
    catppuccin_latte => "catppuccin-latte",
    catppuccin_mocha => "catppuccin-mocha",
    catppuccin_macchiato => "catppuccin-mocha@macchiato",
    high_contrast => "high-contrast",
    high_contrast_light => "high-contrast@light",
    nord => "nord",
    nord_snow_storm => "nord@snow-storm",
    suhina_blocks => "suhina-blocks",
    suhina_glass => "suhina-glass",
    suhina_glass_frost => "suhina-glass@frost",
    suhina_glass_midnight => "suhina-glass@midnight",
    suhina_light => "suhina-light",
    suhina_meadow => "suhina-meadow",
    suhina_sheet => "suhina-sheet",
    suhina_studio => "suhina-studio",
    suhina_studio_blue => "suhina-studio@blue",
    suhina_studio_darker => "suhina-studio@darker",
    suhina_studio_darkest => "suhina-studio@darkest",
    suhina_studio_forest => "suhina-studio@forest",
    suhina_studio_light => "suhina-studio@light",
}

#[test]
fn every_built_in_theme_and_variant_has_one_painted_test() {
    let mut available: Vec<String> = Subject::built_ins()
        .into_iter()
        .map(|subject| subject.id)
        .chain(suhina_ui::kit::theme::built_in_variants().into_iter().map(|(id, _)| id))
        .collect();
    let mut tested: Vec<String> = TESTED_THEMES.iter().map(|id| (*id).to_owned()).collect();
    available.sort();
    tested.sort();
    assert_eq!(tested, available, "every built-in theme and variant has exactly one test in theme_tests!");
}

#[allow(clippy::unwrap_used)] // test helper
fn paints_each_scene_as_recorded(theme: &str, all: &Drawings) {
    let file = file(theme);
    let listings: BTreeMap<&(String, String), Result<&str, &str>> = all
        .iter()
        .map(|(key, drawing)| (key, drawing.as_ref().map(|d| d.listing.as_str()).map_err(String::as_str)))
        .collect();
    if let Some(folder) = std::env::var_os("SUHINA_PAINTED_DUMP") {
        let folder = PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        for ((scene, theme), listing) in &listings {
            let text = listing.map(str::to_owned).unwrap_or_else(|e| format!("error: {e}\n"));
            std::fs::write(folder.join(format!("{scene}.{theme}.txt")), text).unwrap();
        }
    }
    let now: BTreeMap<(String, String), String> = listings
        .iter()
        .map(|(key, listing)| {
            let digest = match listing {
                Ok(text) => format!("{:016x}", painted::digest(text)),
                Err(e) => format!("error:{}", e.replace(char::is_whitespace, "_")),
            };
            ((*key).clone(), digest)
        })
        .collect();
    let mut text = String::from(
        "# What each built-in theme paints in each scene, drawn as on Linux on every system: `scene theme\n\
         # digest`, written by `SUHINA_BLESS=1 cargo test -p suhina-theme-render --test main painted`\n\
         # (tests/painted.rs).\n",
    );
    for ((scene, theme), digest) in &now {
        text.push_str(&format!("{scene} {theme} {digest}\n"));
    }
    let failed: Vec<String> = listings
        .iter()
        .filter_map(|((scene, theme), listing)| listing.as_ref().err().map(|e| format!("{scene} in {theme}: {e}")))
        .collect();
    if suhina_test_support::blessing() {
        // A drawing that failed is never recorded as what it should draw.
        assert!(failed.is_empty(), "not recorded, these drawings failed:\n{}", failed.join("\n"));
        std::fs::write(&file, text).unwrap();
        return;
    }
    let recorded = recorded(&std::fs::read_to_string(&file).unwrap());
    let differ: Vec<String> = now
        .iter()
        .filter(|(key, digest)| recorded.get(*key) != Some(digest))
        .map(|((scene, theme), digest)| {
            format!(
                "{scene} in {theme}: {digest}, recorded {}",
                recorded.get(&(scene.clone(), theme.clone())).map_or("nothing", |d| d)
            )
        })
        .chain(
            recorded
                .keys()
                .filter(|key| !now.contains_key(*key))
                .map(|(scene, theme)| format!("{scene} in {theme}: recorded, not drawn")),
        )
        .collect();
    // The smallest drawing that differs, listed whole: what a machine drew where it differs from
    // the one that recorded it, to compare with a listing written there (`SUHINA_PAINTED_DUMP`).
    let shown = now
        .iter()
        .filter(|(key, digest)| recorded.get(*key) != Some(digest))
        .filter_map(|(key, _)| Some((key, listings.get(key)?.as_ref().ok()?)))
        .min_by_key(|(_, listing)| listing.len())
        .map_or_else(String::new, |((scene, theme), listing)| {
            format!("\n\n{scene} in {theme} as this run draws it, between the lines:\n----- listing -----\n{listing}----- end -----")
        });
    assert!(
        differ.is_empty(),
        "{} of {} drawings differ from {} (see tests/painted.rs for how to compare or record):\n{}{shown}\n\n\
         The file as this run draws it, between the lines:\n----- {} -----\n{text}----- end -----",
        differ.len(),
        now.len(),
        file.display(),
        differ.join("\n"),
        file.display()
    );
}

/// The same scene in the same theme lists the same twice: nothing in a listing depends on the time
/// or on what ran before.
#[test]
fn a_listing_is_the_same_each_time() {
    let scene = scenes::find("launcher").unwrap();
    let subject = Subject::built_in("nord").unwrap();
    let first = painted::scene(&subject, &scene).unwrap().listing;
    let second = painted::scene(&subject, &scene).unwrap().listing;
    assert!(first.lines().count() > 100, "the launcher paints: {first}");
    let differ: Vec<(usize, (&str, &str))> =
        first.lines().zip(second.lines()).enumerate().filter(|(_, (a, b))| a != b).take(5).collect();
    assert!(differ.is_empty() && first.len() == second.len(), "the second listing differs: {differ:#?}");
}

#[test]
fn the_recorded_digests_read() {
    let recorded = recorded("# a note\nlauncher nord 0123456789abcdef\n\narrangement suhina-studio fedcba9876543210\n");
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[&("launcher".to_owned(), "nord".to_owned())], "0123456789abcdef");
}
