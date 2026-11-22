// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the built-in themes in `theme_tests!` paint in each scene, against what `painted/<theme>.txt`
//! records (#1023): what moves the views onto elements, or the colours into the sheet, shows which
//! themes' looks it changes. Headless: the digest of [`painted::scene`]'s listing, no GPU.
//!
//! A drawing that differs from the recording is flagged, not failed: the test prints it and, with
//! `SUHINA_PAINTED_REPORT=<folder>`, writes it to `<folder>/<theme>.md`, which CI puts in the job
//! summary with a warning on the pull request (`.github/workflows/ci.yml`), and the reviewer
//! decides whether the change meant it. The listings that differ go to `<folder>/after/`; on a pull
//! request CI draws the flagged themes again on the base commit and shows the difference between
//! each scene's two listings. A drawing that fails still fails the test.
//!
//! A pull request never records the digests: nearly every change to the views moves digests in
//! every theme, so two pull requests that both recorded them always conflicted on these files.
//! `.github/workflows/docs-generated.yml` records them on the default branch after every merge with
//! `SUHINA_BLESS=1 cargo test -p suhina-theme-render --test main painted`, and CI fails a pull
//! request that edits them. To see what changed, write the listings before and after the change
//! with `SUHINA_PAINTED_DUMP=<folder>` and compare the two folders.
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
/// competing with the test runner. The same drawings are compared with both recordings.
///
/// `SUHINA_PAINTED_ONLY=<theme>,<theme>` draws only those themes and leaves the other tests
/// passing at once: CI draws the flagged themes again on the base commit to show what they paint
/// differently.
#[allow(clippy::unwrap_used)] // test helper
fn paint_and_check(theme: &str) {
    if let Some(only) = std::env::var_os("SUHINA_PAINTED_ONLY")
        && !only.to_string_lossy().split(',').any(|id| id.trim() == theme)
    {
        return;
    }
    let subject = Subject::built_in(theme).unwrap();
    let all: Drawings = scenes::scenes()
        .into_iter()
        .filter(|scene| !LEFT_OUT.contains(&scene.name.as_str()))
        .map(|scene| ((scene.name.clone(), theme.to_owned()), painted::scene(&subject, &scene)))
        .collect();
    let failed: Vec<String> = all
        .iter()
        .filter_map(|((scene, theme), drawing)| drawing.as_ref().err().map(|e| format!("{scene} in {theme}: {e}")))
        .collect();
    // A drawing that failed is never flagged as a look change, nor recorded as what it should draw.
    assert!(failed.is_empty(), "these drawings failed:\n{}", failed.join("\n"));
    let painted = painted_differences(theme, &all);
    let cut = super::cut::cut_differences(theme, &all);
    let flagged: Vec<String> = [painted, cut].into_iter().flatten().collect();
    if flagged.is_empty() {
        return;
    }
    let (summaries, details): (Vec<String>, Vec<String>) = flagged
        .iter()
        .map(|text| text.split_once('\n').map_or((text.clone(), String::new()), |(a, b)| (a.to_owned(), b.to_owned())))
        .unzip();
    let report = report(theme, &summaries.join("; "), &details.concat());
    println!("{report}");
    if let Some(folder) = std::env::var_os("SUHINA_PAINTED_REPORT") {
        let folder = PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(format!("{theme}.md")), report).unwrap();
    }
}

/// A theme's flagged differences as Markdown for the job summary: one line that says what differs,
/// the lines themselves folded under it.
fn report(theme: &str, summary: &str, details: &str) -> String {
    format!("<details><summary><b>{theme}</b>: {summary}</summary>\n\n```text\n{details}```\n\n</details>\n")
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

// One theme for each way a theme is made, not every built-in one: most built-in themes are the same
// sheet with other colour values (catppuccin, high contrast, Suhina Light), and a variant is a set
// of colours over its theme, so drawing them all repeated the same shapes in other colours and
// flagged one change twenty times. Kept: a dark and a second palette on the plain sheet, a variant
// (light), the glass material, and the three themes drawn from their own artwork.
theme_tests! {
    nord => "nord",
    suhina_blocks => "suhina-blocks",
    suhina_glass => "suhina-glass",
    suhina_meadow => "suhina-meadow",
    suhina_sheet => "suhina-sheet",
    suhina_studio => "suhina-studio",
    suhina_studio_light => "suhina-studio@light",
}

/// Each theme drawn is a built-in theme or variant, listed once, and the recordings hold the themes
/// drawn and no others (a theme taken out of the list takes its files with it).
#[test]
fn the_themes_drawn_are_built_in_and_alone_recorded() {
    let available: Vec<String> = Subject::built_ins()
        .into_iter()
        .map(|subject| subject.id)
        .chain(suhina_ui::kit::theme::built_in_variants().into_iter().map(|(id, _)| id))
        .collect();
    let missing: Vec<&&str> = TESTED_THEMES.iter().filter(|id| !available.iter().any(|a| a == **id)).collect();
    assert!(missing.is_empty(), "not a built-in theme or variant: {missing:?}");
    let mut tested: Vec<&str> = TESTED_THEMES.to_vec();
    tested.sort_unstable();
    tested.dedup();
    assert_eq!(tested.len(), TESTED_THEMES.len(), "a theme is listed twice in theme_tests!");
    for folder in ["painted", "cut"] {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(folder);
        let stale: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.strip_suffix(".txt").is_none_or(|theme| !TESTED_THEMES.contains(&theme)))
            .collect();
        assert!(stale.is_empty(), "tests/{folder}/ holds files of themes not drawn: {stale:?}");
    }
}

/// The drawings that differ from `painted/<theme>.txt`: a first line that counts them, then a line
/// each; `None` when every drawing is as recorded. Records them instead when blessing.
#[allow(clippy::unwrap_used)] // test helper
fn painted_differences(theme: &str, all: &Drawings) -> Option<String> {
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
        return None;
    }
    // A theme added in this change has no recording yet: every drawing of it differs.
    let recorded = recorded(&std::fs::read_to_string(&file).unwrap_or_default());
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
            format!("{scene} in {theme} as this run draws it, between the lines:\n----- listing -----\n{listing}----- end -----")
        });
    if differ.is_empty() {
        return None;
    }
    println!("{shown}");
    // The listings that differ, named as `SUHINA_PAINTED_DUMP` names them, for CI to compare with
    // the same scenes drawn on the base commit; a scene recorded but no longer drawn is an empty file.
    if let Some(folder) = std::env::var_os("SUHINA_PAINTED_REPORT") {
        let folder = PathBuf::from(folder).join("after");
        std::fs::create_dir_all(&folder).unwrap();
        for key in now.iter().filter(|(key, digest)| recorded.get(*key) != Some(digest)).map(|(key, _)| key) {
            if let Some(Ok(listing)) = listings.get(key) {
                std::fs::write(folder.join(format!("{}.{}.txt", key.0, key.1)), listing).unwrap();
            }
        }
        for (scene, theme) in recorded.keys().filter(|key| !now.contains_key(*key)) {
            std::fs::write(folder.join(format!("{scene}.{theme}.txt")), "").unwrap();
        }
    }
    Some(format!(
        "{} of {} drawings differ from tests/painted/{theme}.txt\n{}\n",
        differ.len(),
        now.len(),
        differ.join("\n")
    ))
}

#[test]
fn a_report_folds_the_lines_under_what_differs() {
    let report = report("nord", "2 of 60 drawings differ", "menu in nord: 01, recorded 02\n");
    assert_eq!(
        report,
        "<details><summary><b>nord</b>: 2 of 60 drawings differ</summary>\n\n\
         ```text\nmenu in nord: 01, recorded 02\n```\n\n</details>\n"
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
