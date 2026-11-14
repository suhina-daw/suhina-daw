// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every built-in theme and variant paints each scene as recorded in `painted/<system>.txt` (#1023): what
//! moves the views onto elements, or the palette into the sheet, changes no theme's look unless
//! it means to. Headless: the digest of [`painted::scene`]'s listing, no GPU.
//!
//! A change that means to change a look records the new digests:
//! `SUHINA_BLESS=1 cargo test -p suhina-theme-render --test main painted`, and says why in its pull
//! request. To see what changed, write the listings before and after the change with
//! `SUHINA_PAINTED_DUMP=<folder>` and compare the two folders.
//!
//! Each system has its own file (`macos.txt`, `linux.txt`): some text differs between them (shortcuts
//! say Cmd or Ctrl, Settings names the plug-in formats each system has). A change records the file
//! of the system it runs on; when another system's file must change, its CI run fails and prints
//! that whole file, ready to replace it. Not on Windows, where the window draws its own title bar
//! (the application bar): its scenes differ in that bar alone, and the other systems check the rest.

use std::collections::BTreeMap;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(any(target_os = "macos", target_os = "linux"))]
use suhina_theme_render::scenes::Scene;
use suhina_theme_render::{Subject, painted, scenes};

#[cfg(any(target_os = "macos", target_os = "linux"))]
/// The scenes left out: the About dialog shows the build's commit.
const LEFT_OUT: [&str; 1] = ["about-dialog"];

#[cfg(any(target_os = "macos", target_os = "linux"))]
/// A theme to draw, and whether the style engine runs for it when it has no sheet of its own.
struct Drawn {
    name: String,
    subject: Subject,
    engine: bool,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn drawn() -> Vec<Drawn> {
    let mut drawn: Vec<Drawn> = Subject::built_ins()
        .into_iter()
        .map(|subject| Drawn { name: subject.id.clone(), subject, engine: false })
        .collect();
    drawn.extend(suhina_ui::kit::theme::built_in_variants().into_iter().filter_map(|(id, _)| {
        Subject::built_in(&id).map(|subject| Drawn { name: subject.id.clone(), subject, engine: false })
    }));
    // Suhina Studio's look with the style engine on, as it runs for every theme once the palette
    // has gone: what checks the fallbacks of the default theme.
    if let Some(subject) = Subject::built_in(suhina_ui::kit::theme::BUILT_IN) {
        drawn.push(Drawn { name: format!("{}+engine", subject.id), subject, engine: true });
    }
    drawn
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/painted/{}.txt", std::env::consts::OS))
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
/// Every (scene, theme) listing, drawn by as many workers as the machine has cores; each drawing
/// is on a thread of its own ([`painted::scene`]).
fn listings(scenes: &[Scene], drawn: &[Drawn]) -> BTreeMap<(String, String), Result<String, String>> {
    let jobs: Vec<(&Scene, &Drawn)> = scenes.iter().flat_map(|scene| drawn.iter().map(move |d| (scene, d))).collect();
    let next = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while let Some((scene, d)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        done.push(((scene.name.clone(), d.name.clone()), painted::scene(&d.subject, scene, d.engine)));
                    }
                    done
                })
            })
            .collect();
        handles.into_iter().flat_map(|handle| handle.join().unwrap_or_default()).collect()
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn every_built_in_theme_paints_each_scene_as_recorded() {
    let scenes: Vec<Scene> = scenes::scenes().into_iter().filter(|s| !LEFT_OUT.contains(&s.name.as_str())).collect();
    let drawn = drawn();
    let listings = listings(&scenes, &drawn);
    if let Some(folder) = std::env::var_os("SUHINA_PAINTED_DUMP") {
        let folder = PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        for ((scene, theme), listing) in &listings {
            let text = listing.clone().unwrap_or_else(|e| format!("error: {e}\n"));
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
            (key.clone(), digest)
        })
        .collect();
    let mut text = format!(
        "# What each built-in theme paints in each scene on {}: `scene theme digest`, written by\n\
         # `SUHINA_BLESS=1 cargo test -p suhina-theme-render --test main painted` (tests/painted.rs).\n",
        std::env::consts::OS
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
        std::fs::write(file(), text).unwrap();
        return;
    }
    let recorded = recorded(&std::fs::read_to_string(file()).unwrap_or_default());
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
    assert!(
        differ.is_empty(),
        "{} of {} drawings differ from {} (see tests/painted.rs for how to compare or record):\n{}\n\n\
         The file as this run draws it, between the lines:\n----- {} -----\n{text}----- end -----",
        differ.len(),
        now.len(),
        file().display(),
        differ.join("\n"),
        file().display()
    );
}

/// The same scene in the same theme lists the same twice: nothing in a listing depends on the time
/// or on what ran before.
#[test]
fn a_listing_is_the_same_each_time() {
    let scene = scenes::find("launcher").unwrap();
    let subject = Subject::built_in("nord").unwrap();
    let first = painted::scene(&subject, &scene, false).unwrap();
    let second = painted::scene(&subject, &scene, false).unwrap();
    assert!(first.lines().count() > 100, "the launcher paints: {first}");
    let differ: Vec<(usize, (&str, &str))> =
        first.lines().zip(second.lines()).enumerate().filter(|(_, (a, b))| a != b).take(5).collect();
    assert!(differ.is_empty() && first.len() == second.len(), "the second listing differs: {differ:#?}");
}

#[test]
fn the_recorded_digests_read() {
    let recorded =
        recorded("# a note\nlauncher nord 0123456789abcdef\n\narrangement suhina-studio+engine fedcba9876543210\n");
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[&("launcher".to_owned(), "nord".to_owned())], "0123456789abcdef");
}
