// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-theme-files-{name}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    dir
}

/// Writes a theme folder by hand: `theme.json` naming it `name` and `theme.css`.
pub(super) fn hand_written(dir: &Path, name: &str, css: &str) {
    std::fs::create_dir_all(dir).unwrap();
    let meta = theme::ThemeMeta::new(name, "#102030");
    std::fs::write(dir.join(THEME_JSON), meta.to_json()).unwrap();
    std::fs::write(dir.join(THEME_CSS), css).unwrap();
}

fn night() -> Theme {
    let mut theme = Theme { name: "Night".into(), ..Theme::default() };
    theme.colors.accent = suhina_ui::kit::theme::parse_hex("#123456").unwrap_or_default();
    theme
}

#[test]
fn themes_are_written_read_and_seen_when_they_change() {
    let dir = folder("round-trip");
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    assert_eq!(files.read_all(), (Vec::new(), Vec::new()));
    assert_eq!(files.write("night", &night()), Ok(()));
    assert!(files.write("../night", &night()).is_err());
    // Written here, so not read back as a change.
    assert!(files.changed().0.is_empty());

    let themes = dir.join("themes");
    std::fs::write(themes.join("b-bad.json"), "{ \"colors\": { \"accent\": \"red\" } }").unwrap_or_default();
    std::fs::write(themes.join("notes.txt"), "not a theme").unwrap_or_default();
    let (entries, warnings) = files.read_all();
    let ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, ["b-bad", "night"]);
    assert!(entries.get(1).is_some_and(|e| same_look(&e.theme, &night())));
    // The theme with a colour it cannot read, and that it stays JSON until it reads whole.
    assert_eq!(warnings.len(), 2, "{warnings:?}");

    files.checked = None;
    std::fs::write(themes.join("c-new.json"), "{ \"name\": \"New\" }").unwrap_or_default();
    let (changed, _) = files.changed();
    assert_eq!(changed.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["c-new"]);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_renamed_theme_keeps_its_file() {
    let dir = folder("rename");
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    assert_eq!(files.rename("never-saved", "other"), Ok(()));
    assert_eq!(files.write("night", &night()), Ok(()));
    assert_eq!(files.write("day", &Theme::default()), Ok(()));
    assert!(files.rename("night", "day").is_err());
    assert_eq!(files.rename("night", "deep-sea"), Ok(()));
    let ids: Vec<_> = files.read_all().0.into_iter().map(|e| e.id).collect();
    assert_eq!(ids, ["day", "deep-sea"]);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_backup_brings_the_themes_back() {
    let dir = folder("backup");
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    assert!(files.backup().is_err(), "nothing to back up yet");
    assert_eq!(files.write("night", &night()), Ok(()));
    let backup = files.backup().unwrap_or_default();
    assert!(backup.starts_with(dir.join("exports")), "{}", backup.display());

    // A backup holds each theme's folder as it is.
    let mut other = ThemeFiles::new(Some(dir.join("other")));
    assert_eq!(other.import(&backup), Ok((vec!["night".to_string()], Vec::new())));
    let (restored, _) = other.read_all();
    assert_eq!(restored.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["night"]);
    assert!(same_look(&restored[0].theme, &night()));
    // The same themes again are not doubled; a different one with a taken id gets its own.
    assert_eq!(other.import(&backup), Ok((Vec::new(), Vec::new())));
    let changed = Theme { name: "Night 2".into(), ..night() };
    let file = dir.join("night.json");
    std::fs::write(&file, changed.to_json()).unwrap_or_default();
    assert_eq!(other.import(&file).map(|r| r.0), Ok(vec!["night-2".to_string()]));
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_template_holds_a_theme_a_readme_and_examples() {
    let dir = folder("template");
    let files = ThemeFiles::new(Some(dir.join("themes")));
    let zip = files.export_template().unwrap_or_default();
    let file = std::fs::File::open(&zip).unwrap_or_else(|e| panic!("{e}"));
    let mut archive = zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("{e}"));
    let mut names: Vec<String> = archive.file_names().filter_map(Result::ok).map(|n| n.to_string()).collect();
    names.sort();
    for name in ["README.md", "my-theme/theme.css", "my-theme/variants/warm.json", "examples/nord/theme.css"] {
        assert!(names.contains(&name.to_string()), "{name}: {names:?}");
    }
    let json = |n: &&String| n.ends_with(".json") && !n.contains("/variants/") && !n.ends_with("/theme.json");
    assert!(!names.iter().any(|n| json(&n)), "only variants and theme.json are JSON: {names:?}");
    assert!(names.contains(&"my-theme/theme.json".to_string()) && !names.iter().any(|n| n.ends_with("layout.css")));
    let mut template = String::new();
    archive
        .by_name("my-theme/theme.css")
        .unwrap_or_else(|e| panic!("{e}"))
        .read_to_string(&mut template)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(theme::from_css(&template).1, Vec::<String>::new());
    // The editable example is importable; reference palettes remain examples only.
    let mut into = ThemeFiles::new(Some(dir.join("into")));
    let (ids, warnings) = into.import(&zip).unwrap();
    assert_eq!(ids, ["my-theme"]);
    assert!(warnings.is_empty());
    let (entries, warnings) = into.read_all();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["my-theme", "my-theme@warm"]);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_is_not_a_theme_is_refused() {
    let dir = folder("refused");
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    let huge = dir.join("huge.json");
    std::fs::write(&huge, vec![b' '; MAX_FILE as usize + 1]).unwrap_or_default();
    assert!(files.import(&huge).is_err());
    let list = dir.join("list.json");
    std::fs::write(&list, "[1, 2]").unwrap_or_default();
    assert!(files.import(&list).is_err());
    let fake = dir.join("fake.zip");
    std::fs::write(&fake, "not a zip").unwrap_or_default();
    assert!(files.import(&fake).is_err());
    assert!(files.read_all().0.is_empty());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn huge_files_in_the_folder_are_skipped() {
    let dir = folder("huge");
    std::fs::create_dir_all(&dir).unwrap_or_default();
    std::fs::write(dir.join("huge.json"), vec![b' '; MAX_FILE as usize + 1]).unwrap_or_default();
    let (entries, warnings) = ThemeFiles::new(Some(dir.clone())).read_all();
    assert!(entries.is_empty());
    assert_eq!(warnings.len(), 1);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn without_a_folder_nothing_is_read_or_written() {
    let mut files = ThemeFiles::new(None);
    assert_eq!(files.read_all(), (Vec::new(), Vec::new()));
    assert!(files.write("night", &Theme::default()).is_err());
    assert!(files.backup().is_err() && files.export_template().is_err() && files.open_folder().is_err());
    assert_eq!(files.folder_text(), None);
}

#[test]
fn a_folder_without_theme_css_and_two_themes_with_one_name_are_named() {
    let dir = folder("not-themes");
    let themes = dir.join("themes");
    std::fs::create_dir_all(themes.join("hidden-extension")).unwrap();
    std::fs::write(themes.join("hidden-extension/theme.css.txt"), ":root {}").unwrap();
    hand_written(&themes.join("first"), "Twin", ":root {}");
    hand_written(&themes.join("second"), "twin", ":root {}");
    // A folder of the format before theme.json.
    std::fs::create_dir_all(themes.join("older")).unwrap();
    std::fs::write(themes.join("older/theme.css"), "theme { name: \"Older\"; }").unwrap();
    let mut files = ThemeFiles::new(Some(themes));
    let (entries, warnings) = files.read_all();
    let mut ids: Vec<_> = entries.iter().map(|e| e.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["first", "second"]);
    assert!(warnings.iter().any(|w| w.contains("hidden-extension") && w.contains("theme.css.txt")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("two themes are named")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("older") && w.contains("no theme.json")), "{warnings:?}");
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_renamed_folder_leaves_the_list_and_an_empty_one_is_named_once() {
    let dir = folder("renamed");
    let themes = dir.join("themes");
    hand_written(&themes.join("kierros3"), "Kierros 3", ":root {}");
    let mut files = ThemeFiles::new(Some(themes.clone()));
    let (entries, _) = files.read_all();
    assert_eq!(entries.len(), 1);
    files.checked = None;
    assert!(!files.gone());
    std::fs::rename(themes.join("kierros3"), themes.join("test-round-3")).unwrap();
    assert!(files.gone(), "the old id is gone");
    let (entries, _) = files.read_all();
    assert_eq!(entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["test-round-3"]);
    std::fs::create_dir_all(themes.join("empty")).unwrap();
    files.checked = None;
    let (_, warnings) = files.changed();
    assert!(warnings.iter().any(|w| w.contains("empty") && w.contains("no theme.css")), "{warnings:?}");
    files.checked = None;
    let (_, again) = files.changed();
    assert!(!again.iter().any(|w| w.contains("empty")), "named once: {again:?}");
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_exported_package_brings_the_theme_back_and_a_damaged_one_is_refused() {
    let dir = folder("package");
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    assert_eq!(files.write("night", &night()), Ok(()));
    let package = files.export_package("night", &night()).unwrap();
    assert!(package.to_string_lossy().ends_with("night.theme.suhina"), "{}", package.display());
    let mut other = ThemeFiles::new(Some(dir.join("other")));
    let read = suhina_theme_package::read(&std::fs::read(&package).unwrap()).unwrap();
    assert!(read.files.contains_key("README.md"), "the guide goes with the theme");
    let (ids, _) = other.import(&package).unwrap();
    assert_eq!(ids, ["night"]);
    assert!(same_look(&other.read_all().0[0].theme, &night()));
    // A built-in theme has no folder; it is exported from the files Suhina writes for it.
    let built_in = files.export_package("suhina-studio", &Theme::default()).unwrap();
    assert!(suhina_theme_package::read(&std::fs::read(&built_in).unwrap()).is_ok());
    let mut bytes = std::fs::read(&package).unwrap();
    let last = bytes.len() / 2;
    bytes[last] ^= 0xFF;
    let damaged = dir.join("damaged.theme.suhina");
    std::fs::write(&damaged, bytes).unwrap();
    assert!(other.import(&damaged).is_err());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_package_brought_in_again_asks_then_replaces_or_copies() {
    let dir = folder("package-again");
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    assert_eq!(files.write("night", &night()), Ok(()));
    let package = files.export_package("night", &night()).unwrap();
    // The theme is here already, so nothing is written until the answer comes.
    let asked = files.import_with(&package, Again::Ask).unwrap();
    assert_eq!((asked.ids.len(), asked.conflict.as_deref()), (0, Some(night().name.as_str())));
    let replaced = files.import_with(&package, Again::Replace).unwrap();
    assert_eq!(replaced.ids, ["night"]);
    let (themes, _) = files.read_all();
    assert!(same_look(&themes.iter().find(|e| e.id == "night").unwrap().theme, &night()));
    assert!(!themes.iter().any(|e| e.id == "night-2"), "no copy");
    let copied = files.import_with(&package, Again::Copy).unwrap();
    assert_eq!(copied.ids, ["night-2"], "a copy even of the same look, as asked");
    let _clean = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_theme_folder_linked_from_elsewhere_is_read_and_written() {
    let dir = folder("linked");
    let mut elsewhere = ThemeFiles::new(Some(dir.join("work")));
    assert_eq!(elsewhere.write("night", &night()), Ok(()));
    std::fs::create_dir_all(dir.join("themes")).unwrap();
    std::os::unix::fs::symlink(dir.join("work/night"), dir.join("themes/night")).unwrap();
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    let (themes, warnings) = files.read_all();
    assert!(themes.iter().any(|e| e.id == "night"), "{warnings:?}");
    // A variant is written into the linked folder; a link inside it is still refused.
    let mut edited = night();
    edited.colors.accent = eframe::egui::Color32::from_rgb(1, 2, 3);
    assert_eq!(files.write("night@custom", &edited), Ok(()));
    assert!(dir.join("work/night/variants/custom.json").is_file());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_background_picture_in_the_assets_folder_is_read() {
    let dir = folder("background-assets");
    let theme = dir.join("themes/pictured");
    std::fs::create_dir_all(theme.join("assets")).unwrap();
    let png = b"\x89PNG\r\n\x1a\nfake-data-for-the-path-only".to_vec();
    std::fs::write(theme.join("assets/bg-4k.png"), &png).unwrap();
    hand_written(&theme, "Pictured", "window { backdrop-image: url(\"assets/bg-4k.png\"); }");
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    let (themes, warnings) = files.read_all();
    let pictured = themes.iter().find(|e| e.id == "pictured").expect("the theme reads");
    assert!(
        pictured.theme.skin.background.as_deref().is_some_and(|b| b.starts_with("data:image/png")),
        "{:?} {warnings:?}",
        pictured.theme.skin.background.as_deref().map(|b| &b[..b.len().min(40)])
    );
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_built_in_themes_package_carries_the_variants_kept_beside_it() {
    let dir = folder("built-in-variants");
    let themes = dir.join("themes");
    let variants = themes.join("suhina-studio").join("variants");
    std::fs::create_dir_all(&variants).unwrap();
    std::fs::write(variants.join("warm.json"), r##"{"name":"Warm","values":{"--accent":"#FFB86C"}}"##).unwrap();
    let files = ThemeFiles::new(Some(themes));
    let package = files.export_package("suhina-studio", &Theme::default()).unwrap();
    let read = suhina_theme_package::read(&std::fs::read(&package).unwrap()).unwrap();
    assert!(read.files.contains_key("variants/warm.json"), "{:?}", read.files.keys().collect::<Vec<_>>());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_with_theme_json_is_never_migrated_whatever_its_sheet_holds() {
    let dir = folder("keep-json");
    let themes = dir.join("themes");
    let mut meta = theme::ThemeMeta::new("Night", "#102030");
    meta.author = Some("Someone".into());
    std::fs::create_dir_all(themes.join("night")).unwrap();
    std::fs::write(themes.join("night").join(THEME_JSON), meta.to_json()).unwrap();
    std::fs::write(themes.join("night").join(THEME_CSS), "@import \"extra.css\";\n:root { --accent: #123456; }\n")
        .unwrap();
    std::fs::write(themes.join("night").join("extra.css"), "").unwrap();
    let mut files = ThemeFiles::new(Some(themes.clone()));
    let _read = files.read_all();
    let kept = std::fs::read_to_string(themes.join("night").join(THEME_JSON)).unwrap();
    assert!(kept.contains("Night") && kept.contains("Someone"), "{kept}");
    assert!(!themes.join("night").join(LEGACY).exists(), "nothing was migrated");
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sheet_dropped_on_the_window_brings_the_artwork_it_names() {
    let dir = folder("import-artwork");
    let drop = dir.join("drop");
    std::fs::create_dir_all(drop.join("assets")).unwrap();
    std::fs::write(
        drop.join("night.css"),
        ":root { --accent: #123456; }\nbutton.play { image: url(\"assets/play.svg\"); }\n",
    )
    .unwrap();
    std::fs::write(
        drop.join("assets/play.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4"/></svg>"#,
    )
    .unwrap();
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    let imported = files.import_with(&drop.join("night.css"), Again::Copy).unwrap();
    assert_eq!(imported.ids.len(), 1, "{:?}", imported.warnings);
    let id = &imported.ids[0];
    assert!(dir.join("themes").join(id).join("assets/play.svg").is_file(), "{:?}", imported.warnings);
    let (entries, warnings) = files.read_all();
    assert!(entries.iter().any(|e| &e.id == id), "the theme reads back: {warnings:?}");
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_backup_with_a_folder_that_cannot_be_read_brings_in_the_rest() {
    let dir = folder("backup-skip");
    let mut files = ThemeFiles::new(Some(dir.join("themes")));
    assert_eq!(files.write("night", &night()), Ok(()));
    let mut contents = suhina_theme_package::read_folder(&dir.join("themes").join("night")).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> =
        contents.iter_mut().map(|(path, bytes)| (format!("night/{path}"), std::mem::take(bytes))).collect();
    // A folder of the format before theme.json: a theme.css and no theme.json.
    entries.push(("old/theme.css".into(), b":root { --accent: red; }".to_vec()));
    let zip = dir.join("backup.zip");
    write_zip(&zip, &entries).unwrap();
    let mut other = ThemeFiles::new(Some(dir.join("other")));
    let imported = other.import_with(&zip, Again::Copy).unwrap();
    assert_eq!(imported.ids, ["night"]);
    assert!(
        imported.warnings.iter().any(|w| w.contains("old") && w.contains("not brought in")),
        "{:?}",
        imported.warnings
    );
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn roles_that_share_an_artwork_file_do_not_overwrite_each_other_on_save() {
    let dir = folder("shared-artwork");
    let themes = dir.join("themes");
    let svg = |n: u32| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{n}" height="{n}"><rect width="{n}" height="{n}"/></svg>"#
        )
    };
    let css = "button.play { image: url(\"assets/transport.svg\"); }\nbutton.stop { image: url(\"assets/transport.svg\"); }\n";
    hand_written(&themes.join("night"), "Night", css);
    std::fs::create_dir_all(themes.join("night/assets")).unwrap();
    std::fs::write(themes.join("night/assets/transport.svg"), svg(4)).unwrap();
    let mut files = ThemeFiles::new(Some(themes.clone()));
    let (entries, warnings) = files.read_all();
    let mut theme = entries.iter().find(|e| e.id == "night").unwrap_or_else(|| panic!("{warnings:?}")).theme.clone();
    assert_eq!(theme.skin.assets.len(), 2, "{:?}", theme.skin.assets.keys().collect::<Vec<_>>());
    let (edited, other) = {
        let mut roles = theme.skin.assets.keys().cloned();
        (roles.next().unwrap(), roles.next().unwrap())
    };
    theme.skin.assets.insert(edited.clone(), svg(8));
    assert_eq!(files.write("night", &theme), Ok(()));
    let (entries, warnings) = files.read_all();
    let back = &entries.iter().find(|e| e.id == "night").unwrap_or_else(|| panic!("{warnings:?}")).theme;
    assert_eq!(back.skin.assets[&edited], svg(8), "the edit is kept");
    assert_eq!(back.skin.assets[&other], svg(4), "the other role keeps its artwork");
    let _clean = std::fs::remove_dir_all(&dir);
}

/// A folder as nightlies since 2026-10-26 wrote it: `theme.css`, `layout.css`, no `theme.json`.
fn nightly_folder(themes: &Path, id: &str) -> Vec<(PathBuf, Vec<u8>)> {
    let dir = themes.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let css = format!(":root {{ --background: #101010; }}\n/* {id} */\n");
    std::fs::write(dir.join(THEME_CSS), &css).unwrap();
    std::fs::write(dir.join("layout.css"), "toolbar { order: 1; }\n").unwrap();
    vec![(dir.join(THEME_CSS), css.into_bytes()), (dir.join("layout.css"), b"toolbar { order: 1; }\n".to_vec())]
}

fn unchanged(before: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in before {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "{} was changed", path.display());
    }
}

#[test]
fn a_folder_that_is_not_a_theme_is_not_written_in_by_a_save_or_an_import() {
    let dir = folder("hidden-folder");
    let themes = dir.join("themes");
    let before = nightly_folder(&themes, "nord-copy");
    let mut files = ThemeFiles::new(Some(themes.clone()));
    // Not listed, but its name is taken, and the window is told so for Duplicate and New.
    assert!(files.read_all().0.is_empty());
    assert_eq!(files.reserved_changed(), Some(vec!["nord-copy".to_string()]));
    assert_eq!(files.reserved_changed(), None);

    // A save under its id is refused, and its files are as they were.
    assert!(files.write("nord-copy", &night()).is_err());
    unchanged(&before);
    assert!(!themes.join("nord-copy").join(THEME_JSON).exists());

    // A dropped sheet of the same name takes the next id.
    let drop = dir.join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    let sheet = drop.join("nord-copy.css");
    std::fs::write(&sheet, ":root { --background: #202020; }\n").unwrap();
    let imported = files.import_with(&sheet, Again::Copy).unwrap();
    assert_eq!(imported.ids, ["nord-copy-2"]);
    unchanged(&before);

    // The same for a package.
    let packed = {
        let mut other = ThemeFiles::new(Some(dir.join("other")));
        other.write("nord-copy", &night()).unwrap();
        other.export_package("nord-copy", &night()).unwrap()
    };
    let imported = files.import_with(&packed, Again::Copy).unwrap();
    assert_eq!(imported.ids, ["nord-copy-3"]);
    unchanged(&before);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_does_not_write_into_a_folder_with_other_files_and_no_theme_css() {
    let dir = folder("foreign-folder");
    let themes = dir.join("themes");
    std::fs::create_dir_all(themes.join("photos")).unwrap();
    std::fs::write(themes.join("photos").join("background.png"), b"mine").unwrap();
    let mut files = ThemeFiles::new(Some(themes.clone()));
    assert!(files.write("photos", &night()).is_err());
    assert_eq!(std::fs::read(themes.join("photos").join("background.png")).unwrap(), b"mine");
    assert!(!themes.join("photos").join(THEME_JSON).exists());
    // An empty folder is a theme's to take.
    std::fs::create_dir_all(themes.join("empty")).unwrap();
    assert_eq!(files.write("empty", &night()), Ok(()));
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_variant_save_does_not_write_into_a_folder_that_is_not_a_theme() {
    let dir = folder("foreign-variant");
    let themes = dir.join("themes");
    let photos = themes.join("photos");
    std::fs::create_dir_all(photos.join(VARIANTS)).unwrap();
    std::fs::write(photos.join("background.png"), b"mine").unwrap();
    std::fs::write(photos.join(VARIANTS).join("custom.json"), b"{\"mine\":true}").unwrap();
    let mut files = ThemeFiles::new(Some(themes));
    assert!(files.write("photos@custom", &night()).is_err());
    assert_eq!(std::fs::read(photos.join(VARIANTS).join("custom.json")).unwrap(), b"{\"mine\":true}");
    assert_eq!(std::fs::read(photos.join("background.png")).unwrap(), b"mine");
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_import_looks_at_its_destination_when_the_scan_of_names_stops_short() {
    let dir = folder("scan-limit");
    let themes = dir.join("themes");
    let before: Vec<_> = ["taken-a", "taken-b"].iter().flat_map(|id| nightly_folder(&themes, id)).collect();
    let mut files = ThemeFiles::new(Some(themes));
    // The scan sees one folder of the two, so one of them is not in the names.
    files.max_folders = 1;
    let drop = dir.join("drop");
    std::fs::create_dir_all(&drop).unwrap();
    for (id, expected) in [("taken-a", "taken-a-2"), ("taken-b", "taken-b-2")] {
        let sheet = drop.join(format!("{id}.css"));
        std::fs::write(&sheet, ":root { --background: #202020; }\n").unwrap();
        assert_eq!(files.import_with(&sheet, Again::Copy).unwrap().ids, [expected]);
    }
    unchanged(&before);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_folder_of_variants_alone_is_not_written_in_by_a_save() {
    let dir = folder("variants-only");
    let themes = dir.join("themes");
    let photos = themes.join("photos");
    std::fs::create_dir_all(photos.join(VARIANTS)).unwrap();
    std::fs::write(photos.join(VARIANTS).join("custom.json"), b"{\"mine\":true}").unwrap();
    let mut files = ThemeFiles::new(Some(themes.clone()));
    assert!(files.write("photos@custom", &night()).is_err());
    assert!(files.write("photos", &night()).is_err());
    assert_eq!(std::fs::read(photos.join(VARIANTS).join("custom.json")).unwrap(), b"{\"mine\":true}");
    assert!(!photos.join(THEME_JSON).exists());
    // A variant of a new theme is still written with the theme's own files, and of a built-in theme alone.
    assert_eq!(files.write("fresh@custom", &night()), Ok(()));
    assert!(
        themes.join("fresh").join(THEME_JSON).exists()
            && themes.join("fresh").join(VARIANTS).join("custom.json").exists()
    );
    let _clean = std::fs::remove_dir_all(&dir);
}
