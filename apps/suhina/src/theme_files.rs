// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme files: one folder per theme in the `themes` folder beside the settings file, named by the
//! theme's id (ADR 0031, amendment *Theme settings and variants*):
//!
//! - `theme.css`, the theme's style sheet as its author wrote it. Suhina reads it and never writes
//!   it again once it is there.
//! - `variants/<id>.json`, values for the theme's properties (`theme_vars::Variant`). The settings
//!   editors write these, and only these.
//! - `layout.css`, what Suhina's Edit interface and theme editors set beyond settings: placements,
//!   the toolbar, artwork and the name. Written by Suhina only.
//! - artwork, `background.png` and the README.
//!
//! A built-in theme's variants live in a folder of its id holding only `variants/`. A file saved
//! while Suhina runs is read again within a second, so a theme can be edited in any text editor
//! and seen right away. A theme from an earlier version (`night-blue.json` beside a folder, or a
//! `theme.css` that imports `advanced.css`) is migrated once: the new files are written, read back
//! and compared, and only then are the old ones moved into the theme's `legacy/` folder; nothing is
//! deleted. Backups and the theme template are zips written to the `exports` folder; theme files
//! and zips dropped on the window are brought in.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use suhina_ui::kit::theme::{self, Theme};
use suhina_ui::kit::theme_vars::{self, Variant};
use suhina_ui::settings::is_theme_id;
use suhina_ui::settings::themes::{ThemeEntry, slug};

/// Larger files are not themes; they are skipped instead of read.
const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_EXPANDED: u64 = 32 * 1024 * 1024;
/// A zip with more files than this is not a set of themes.
const MAX_ZIP_FILES: usize = 512;
/// How often the folder is looked at for changed files.
const CHECK_EVERY: Duration = Duration::from_secs(1);
/// The theme template's name in its zip; it is not brought in when the zip is dropped.
const TEMPLATE: &str = "template.json";
/// A theme's own style sheet: `<id>/theme.css`.
const THEME_CSS: &str = "theme.css";
/// What Suhina's editors set beyond settings: `<id>/layout.css`.
const LAYOUT_CSS: &str = "layout.css";
/// A theme's variants: `<id>/variants/<variant>.json`.
const VARIANTS: &str = "variants";
/// Most variants one theme may have, and the largest a variant file may be.
const MAX_VARIANTS: usize = 64;
const MAX_VARIANT_FILE: u64 = 64 * 1024;
/// Where a migrated theme keeps its JSON and old canonical CSS.
const LEGACY: &str = "legacy";
/// The folder of the template zip that holds the built-in themes as examples.
const EXAMPLES: &str = "examples/";

/// A variant file's text as last read or written, so a save never overwrites an edit made
/// outside Suhina since.
#[derive(Clone)]
struct Snapshot {
    sources: String,
}

/// What [`ThemeFiles::import_with`] does with a package whose theme is here already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Again {
    /// Write nothing and name it, for the app to ask; a theme that looks the same as one here
    /// is left out.
    Ask,
    /// Write it in place of the theme here, which goes with its variants.
    Replace,
    /// Write it under a new id, even when it looks the same as a theme here.
    Copy,
}

/// What an import brought in.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub ids: Vec<String>,
    pub warnings: Vec<String>,
    /// The name of a package's theme that is here already, when nothing was written for it.
    pub conflict: Option<String>,
}

pub struct ThemeFiles {
    folder: Option<PathBuf>,
    /// When each file was last changed, as last read or written.
    seen: HashMap<String, u64>,
    checked: Option<Instant>,
    snapshots: HashMap<String, Snapshot>,
    /// When `user.css` last changed, as last read (`Some(0)` when it was not there).
    user_seen: Option<u64>,
    /// The folders last named as not themes, so each is named once until it changes.
    not_themes: Vec<String>,
}

impl ThemeFiles {
    pub fn new(folder: Option<PathBuf>) -> Self {
        Self {
            folder,
            seen: HashMap::new(),
            checked: None,
            snapshots: HashMap::new(),
            user_seen: None,
            not_themes: Vec::new(),
        }
    }

    fn refresh_default_assets(&self, warnings: &mut Vec<String>) {
        if let Some(folder) = &self.folder
            && let Err(error) = read_default_assets(folder, warnings)
        {
            warnings.push(format!("Default SVG library: {error}"));
        }
    }

    /// Reads the theme `id` at `path` with its variants, one entry each: its `theme.css`, a
    /// theme from an earlier version (migrated), or a built-in theme's `variants` folder.
    fn load_synced(&mut self, id: &str, path: &Path, warnings: &mut Vec<String>) -> Vec<ThemeEntry> {
        let result = (|| -> Result<(Theme, Vec<(Variant, String)>), String> {
            let folder = self.folder.clone().ok_or("no themes folder")?;
            let dir = folder.join(id);
            if path.is_dir() {
                // A built-in theme's variants, kept without its CSS.
                let theme = theme::built_in_themes()
                    .into_iter()
                    .find(|(b, _)| b.eq_ignore_ascii_case(id))
                    .map(|(_, t)| t)
                    .ok_or("only a built-in theme has variants without a theme.css")?;
                return Ok((theme, read_variants(&dir, warnings)));
            }
            if is_css_theme(path) {
                let text = read_text(path)?;
                if !previous_format(&text) {
                    let (theme, problems) = read_css_theme(path)?;
                    warnings.extend(problems.into_iter().map(|p| format!("{id}: {p}")));
                    return Ok((theme, read_variants(&dir, warnings)));
                }
            }
            // A theme from an earlier version: JSON, or a theme.css that imports advanced.css.
            let previous = is_css_theme(path);
            let (theme, problems) = if previous { read_previous(path)? } else { read_loose(path)? };
            if !problems.is_empty() {
                // Rules it could not read would be lost in the new form, so it stays as it is.
                warnings.extend(problems.into_iter().map(|p| format!("{id}: {p}")));
                warnings.push(format!("{id}: kept in its earlier form, not migrated, until it reads without warnings"));
                let mut theme = theme;
                theme.vars.kept = Some("Not converted to theme.css: it reads with warnings".into());
                return Ok((theme, Vec::new()));
            }
            match migrate(&folder, id, &theme, previous) {
                Ok(()) => {
                    let (theme, problems) = read_css_theme(&dir.join(THEME_CSS))?;
                    warnings.extend(problems.into_iter().map(|p| format!("{id}: {p}")));
                    Ok((theme, read_variants(&dir, warnings)))
                }
                Err(error) => {
                    warnings.push(format!("{id}: kept in its earlier form, not migrated: {error}"));
                    let mut theme = theme;
                    theme.vars.kept = Some(format!("Not converted to theme.css: {error}").into());
                    Ok((theme, Vec::new()))
                }
            }
        })();
        match result {
            Ok((theme, variants)) => {
                let mut entries = vec![ThemeEntry { id: id.into(), theme: theme.clone() }];
                for (variant, text) in variants {
                    let key = format!("{id}@{}", variant.id);
                    let (chosen, problems) = theme_vars::with_variant(&theme, Some(&variant));
                    warnings.extend(problems.into_iter().map(|p| format!("{key}: {p}")));
                    self.snapshots.insert(key.clone(), Snapshot { sources: text });
                    entries.push(ThemeEntry { id: key, theme: chosen });
                }
                entries
            }
            Err(error) => {
                warnings.push(format!("{id}: {error}"));
                Vec::new()
            }
        }
    }

    /// The user's own style sheet, `user.css` in the themes folder, when it changed since last
    /// asked (or always with `again`): `Ok(None)` once it is removed. It applies over every theme
    /// and stays when the theme changes. A sheet that does not parse is an error, and the one
    /// in use stays.
    pub fn user_css(&mut self, again: bool) -> Option<Result<Option<String>, String>> {
        use suhina_ui::kit::theme_css;
        let path = self.folder.as_ref()?.join("user.css");
        let present = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file());
        let modified = if present { fingerprint(&path).unwrap_or(1).max(1) } else { 0 };
        if !again && self.user_seen == Some(modified) {
            return None;
        }
        self.user_seen = Some(modified);
        if !present {
            return Some(Ok(None));
        }
        Some((|| {
            let css = String::from_utf8(read_bounded(&path, theme_css::MAX_CSS as u64)?).map_err(|e| e.to_string())?;
            suhina_ui::kit::native_css::inspect(&css).map_err(|e| format!("user.css: {e}"))?;
            Ok(Some(css))
        })())
    }

    /// The folder as the Settings page shows it.
    pub fn folder_text(&self) -> Option<String> {
        self.folder.as_ref().map(|f| f.display().to_string())
    }

    /// Where backups and the template go: `exports` beside the themes folder.
    fn exports(&self) -> Result<PathBuf, String> {
        self.folder
            .as_ref()
            .and_then(|f| f.parent())
            .map(|p| p.join("exports"))
            .ok_or_else(|| "there is no folder to keep themes in".to_string())
    }

    /// Reads every theme file, in the order of their names, with warnings about the ones that
    /// could not be read whole.
    pub fn read_all(&mut self) -> (Vec<ThemeEntry>, Vec<String>) {
        self.seen.clear();
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        self.refresh_default_assets(&mut warnings);
        for (id, path, modified) in self.list() {
            entries.extend(self.load_synced(&id, &path, &mut warnings));
            let path = self.current_path(&id, path);
            self.seen.insert(id, fingerprint(&path).unwrap_or(modified));
        }
        self.not_themes = self.not_themes();
        warnings.extend(self.not_themes.iter().cloned());
        warnings.extend(same_names(&entries));
        (entries, warnings)
    }

    /// Whether a theme read before is gone from the folder (deleted or renamed), so the list has
    /// to be read again whole.
    pub fn gone(&self) -> bool {
        // As often as the files are looked at for changes, not more.
        if self.checked.is_some_and(|at| at.elapsed() < CHECK_EVERY) {
            return false;
        }
        let ids: std::collections::HashSet<String> = self.list().into_iter().map(|(id, _, _)| id).collect();
        self.seen.keys().any(|id| !ids.contains(id))
    }

    /// Folders in the themes folder that are not themes because they have no `theme.css`
    /// (`theme.css.txt`, a sheet under another name, or one folder too deep), so a theme made
    /// by hand that does not appear says why.
    fn not_themes(&self) -> Vec<String> {
        let Some(Ok(dir)) = self.folder.as_ref().map(std::fs::read_dir) else { return Vec::new() };
        dir.filter_map(Result::ok)
            .take(4096)
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_owned();
                // Suhina's own folders, and a built-in theme's folder of variants.
                if name.starts_with('_') || name.starts_with('.') || theme::is_built_in(&name) || name == "exports" {
                    return None;
                }
                let path = entry.path();
                if path.join(THEME_CSS).is_file() {
                    return None;
                }
                let looks_like = std::fs::read_dir(&path)
                    .ok()?
                    .filter_map(Result::ok)
                    .take(256)
                    .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                    .find(|file| {
                        file.to_ascii_lowercase().ends_with(".css") || file.eq_ignore_ascii_case("theme.css.txt")
                    });
                Some(match looks_like {
                    Some(file) => format!("{name}: not a theme, it has {file} but no theme.css"),
                    None => format!("{name}: not a theme, it has no theme.css"),
                })
            })
            .collect()
    }

    /// Where the theme `id` is now: its `theme.css` once a JSON theme has been migrated.
    fn current_path(&self, id: &str, path: PathBuf) -> PathBuf {
        let css = self.folder.as_ref().map(|folder| folder.join(id).join(THEME_CSS));
        css.filter(|css| css.is_file()).unwrap_or(path)
    }

    /// Files added or changed since they were last read, at most once every second.
    pub fn changed(&mut self) -> (Vec<ThemeEntry>, Vec<String>) {
        let now = Instant::now();
        if self.checked.is_some_and(|at| now.duration_since(at) < CHECK_EVERY) {
            return (Vec::new(), Vec::new());
        }
        self.checked = Some(now);
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        self.refresh_default_assets(&mut warnings);
        for (id, path, modified) in self.list() {
            if self.seen.get(&id) == Some(&modified) {
                continue;
            }
            entries.extend(self.load_synced(&id, &path, &mut warnings));
            let path = self.current_path(&id, path);
            self.seen.insert(id, fingerprint(&path).unwrap_or(modified));
        }
        // A folder made by hand that is not a theme yet is named once, when it appears or changes.
        let not_themes = self.not_themes();
        warnings.extend(not_themes.iter().filter(|w| !self.not_themes.contains(w)).cloned());
        self.not_themes = not_themes;
        (entries, warnings)
    }

    /// Writes the theme `id` to its file.
    pub fn write(&mut self, id: &str, theme: &Theme) -> Result<(), String> {
        suhina_plugins::trace::timed("theme save", || self.write_inner(id, theme))
    }

    fn write_inner(&mut self, id: &str, theme: &Theme) -> Result<(), String> {
        let folder = self.folder.clone().ok_or("there is no folder to keep themes in")?;
        let (base, variant) = theme_vars::split_id(id);
        if !is_theme_id(base) || variant.is_some_and(|v| !theme_vars::is_variant_id(v)) {
            return Err(format!("{id} cannot name a theme file"));
        }
        if let Some(variant) = variant {
            // The settings editors write the variant's file, and nothing of the theme's own.
            let path = folder.join(base).join(VARIANTS).join(format!("{variant}.json"));
            if let Some(snapshot) = self.snapshots.get(id)
                && path.exists()
                && read_text(&path)? != snapshot.sources
            {
                return Err(format!(
                    "Variant {id} has external edits. Reload themes before saving; its file was not overwritten."
                ));
            }
            let mut file = Variant::of(theme).unwrap_or_else(|| Variant {
                id: variant.into(),
                name: variant.into(),
                light: None,
                values: theme.vars.values.clone(),
            });
            file.id = variant.into();
            let text = file.to_json();
            write_files(&folder, vec![(format!("{base}/{VARIANTS}/{variant}.json"), text.clone().into_bytes())])?;
            self.snapshots.insert(id.into(), Snapshot { sources: text });
        }
        if !theme::is_built_in(base) {
            suhina_plugins::trace::timed("theme package persist", || persist_theme(&folder, base, theme))?;
        }
        let path = self.current_path(base, folder.join(base).join(VARIANTS));
        if let Ok(modified) = fingerprint(&path) {
            self.seen.insert(base.to_string(), modified);
        }
        Ok(())
    }

    /// Renames the theme `from` to `to`, after the theme was renamed: its folder, and a JSON file
    /// not migrated yet. A theme not saved before is fine: it is written under its new name next.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("there is no folder to keep themes in")?;
        if !is_theme_id(from) || !is_theme_id(to) {
            return Err(format!("{from} or {to} cannot name a theme file"));
        }
        let pairs = [
            (folder.join(from), folder.join(to)),
            (folder.join(format!("{from}.json")), folder.join(format!("{to}.json"))),
        ];
        if pairs.iter().all(|(old, _)| !old.exists()) {
            return Ok(());
        }
        if !from.eq_ignore_ascii_case(to) && pairs.iter().any(|(old, new)| old.exists() && new.exists()) {
            return Err(format!("there is a theme {to} already"));
        }
        for (old, new) in pairs.iter().filter(|(old, _)| old.exists()) {
            std::fs::rename(old, new).map_err(|e| format!("could not rename {from}: {e}"))?;
        }
        self.seen.remove(from);
        self.snapshots.remove(from);
        if let Some((_, _, modified)) = self.list().into_iter().find(|(id, _, _)| id == to) {
            self.seen.insert(to.to_string(), modified);
        }
        Ok(())
    }

    /// Writes every theme file into `exports/suhina-themes-<date>.zip`. Returns its path.
    pub fn backup(&self) -> Result<PathBuf, String> {
        let files = self.list();
        if files.is_empty() {
            return Err("there are no theme files to back up yet".into());
        }
        let folder = self.folder.as_ref().ok_or("there is no folder to keep themes in")?;
        let mut contents = vec![("README.md".into(), theme::template_readme().into_bytes())];
        // Suhina's own kept copies (`_defaults`, the template) are no theme of the user's.
        for (id, path, _) in files.into_iter().filter(|(id, _, _)| !id.starts_with('_') && id != "template") {
            if is_css_theme(&path) || path.is_dir() {
                // A theme folder as it is: its sheets, variants and artwork.
                contents.extend(folder_files(folder, &id)?);
            } else {
                let (theme, _) = read_any(&path)?;
                contents.extend(portable_package_files(&id, &theme)?);
            }
        }
        // The local date and time, so the name says the day the user knows, and two backups on
        // one day are both kept.
        let name = format!("suhina-themes-{}.zip", chrono::Local::now().format("%Y-%m-%d-%H%M%S"));
        write_zip(&self.exports()?.join(name), &contents)
    }

    /// Writes `exports/suhina-theme-template.zip`: `my-theme/` (Suhina Dark's sheet with every
    /// colour written out, its artwork and an example variant), `README.md` on the format, and the
    /// built-in themes in `examples/`. Returns its path.
    pub fn export_template(&self) -> Result<PathBuf, String> {
        let mut template = Theme { name: "My theme".into(), ..Theme::default() };
        template.skin.assets = suhina_ui::kit::skin::inherited_assets();
        template.skin.toolbar = Some(Default::default());
        let mut contents = vec![("README.md".to_string(), theme::template_readme().into_bytes())];
        for (id, theme) in theme::built_in_themes() {
            for (name, data) in package_files(id, &theme)? {
                if !name.ends_with("/README.md") && !name.contains("/examples/") {
                    contents.push((format!("{EXAMPLES}{name}"), data));
                }
            }
        }
        contents.extend(portable_package_files("my-theme", &template)?);
        let example = Variant {
            id: "warm".into(),
            name: "Warm".into(),
            light: Some(false),
            values: [("--accent".to_string(), "#FFB86C".to_string())].into(),
        };
        contents.push((format!("my-theme/{VARIANTS}/warm.json"), example.to_json().into_bytes()));
        write_zip(&self.exports()?.join("suhina-theme-template.zip"), &contents)
    }

    /// Writes the theme `id` as a theme package, `exports/<id>.theme.suhina`: its folder's files,
    /// or for a theme without a folder (a built-in one) the files Suhina writes for it. Returns
    /// its path.
    pub fn export_package(&self, id: &str, theme: &Theme) -> Result<PathBuf, String> {
        let dir = self.folder.as_ref().map(|folder| folder.join(id)).filter(|dir| dir.join(THEME_CSS).is_file());
        let files = match dir {
            Some(dir) => suhina_theme_package::read_folder(&dir)?,
            None => {
                let prefix = format!("{id}/");
                package_files(id, theme)?
                    .into_iter()
                    .filter_map(|(path, bytes)| Some((path.strip_prefix(&prefix)?.to_owned(), bytes)))
                    .collect()
            }
        };
        // The author's guide travels with the theme, so whoever receives it can edit it.
        let mut files = files;
        files.entry("README.md".into()).or_insert_with(|| theme::template_readme().into_bytes());
        let id = slug(id);
        let bytes = suhina_theme_package::pack(&id, &theme.name, &files)?;
        let exports = self.exports()?;
        std::fs::create_dir_all(&exports).map_err(|e| format!("{}: {e}", exports.display()))?;
        let path = exports.join(format!("{id}{}", suhina_theme_package::EXTENSION));
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// Brings in a theme file or every theme in a zip (a backup, say; the template's own files
    /// are left out): each is written to the themes folder under an id no theme has yet, unless
    /// the same theme is there already. Returns the ids written and warnings.
    #[cfg(test)]
    pub fn import(&mut self, path: &Path) -> Result<(Vec<String>, Vec<String>), String> {
        self.import_with(path, Again::Ask).map(|imported| (imported.ids, imported.warnings))
    }

    /// Brings in a theme file or a zip of them, as the import from a drop does; a theme package whose id is a theme here
    /// already is written as `again` says: in its place, as a copy, or not at all (`Ask`, and
    /// its name is returned in `conflict` for the app to ask).
    pub fn import_with(&mut self, path: &Path, again: Again) -> Result<Imported, String> {
        let mut conflict = None;
        let mut replace = None;
        let is_zip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let mut found = Vec::new();
        let mut warnings = Vec::new();
        if suhina_theme_package::is_package(path) {
            // A theme package: its manifest's sums and every file are checked before anything
            // is written.
            let bytes = read_bounded(path, MAX_EXPANDED)?;
            let package = suhina_theme_package::read(&bytes).map_err(|e| format!("{}: {e}", package_name(path)))?;
            let text = |name: &str| package.files.get(name).map(|b| String::from_utf8_lossy(b).into_owned());
            let (theme, problems) =
                theme::from_sheet(&text(THEME_CSS).unwrap_or_default(), &text(LAYOUT_CSS).unwrap_or_default());
            warnings.extend(package.warnings.iter().chain(&problems).map(|p| format!("{}: {p}", package.name)));
            let here = self.list().into_iter().any(|(id, _, _)| id.eq_ignore_ascii_case(&package.id));
            match again {
                Again::Ask if here => {
                    conflict = Some(package.name.clone());
                    return Ok(Imported { ids: Vec::new(), warnings, conflict });
                }
                Again::Replace if here => replace = Some(package.id.clone()),
                _ => {}
            }
            found.push((package.id.clone(), theme, package.files.into_iter().collect()));
        } else if is_zip {
            for loaded in read_zip(path)? {
                warnings.extend(loaded.warnings.into_iter().map(|p| format!("{}: {p}", loaded.name)));
                found.push((stem(&loaded.name), loaded.theme, loaded.files));
            }
        } else {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            if std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len() > MAX_FILE {
                return Err(format!("{name} is larger than a theme can be"));
            }
            let (theme, problems) = read_any(path)?;
            if problems.iter().any(|p| p.contains("not JSON") || p.contains("JSON object")) {
                return Err(format!("{name} is not a theme: {}", problems.join("; ")));
            }
            warnings.extend(problems.into_iter().map(|p| format!("{name}: {p}")));
            // A sheet of today's form comes in as it is; one of an earlier version is written anew.
            let css = path.extension().is_some_and(|e| e == "css");
            let files = if css && !previous_format(&read_text(path)?) {
                vec![(THEME_CSS.to_string(), read_text(path)?.into_bytes())]
            } else {
                Vec::new()
            };
            found.push((stem(&name), theme, files));
        }
        // A theme replaced goes first, its variants with it, and the package's files take its folder.
        if let (Some(id), Some(folder)) = (&replace, &self.folder) {
            let dir = folder.join(id);
            if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
                std::fs::remove_dir_all(&dir).map_err(|e| format!("could not replace {id}: {e}"))?;
            }
        }
        let (existing, _) = self.read_all();
        let existing: Vec<ThemeEntry> = existing.into_iter().filter(|e| !e.id.contains('@')).collect();
        let mut written: Vec<ThemeEntry> = Vec::new();
        for (stem, theme, files) in found {
            if again == Again::Ask && existing.iter().chain(written.iter()).any(|e| same_look(&e.theme, &theme)) {
                continue;
            }
            let base = slug(if stem.is_empty() { &theme.name } else { &stem });
            let taken = |id: &str| {
                theme::is_built_in(id)
                    || existing.iter().chain(written.iter()).any(|e: &ThemeEntry| e.id.eq_ignore_ascii_case(id))
            };
            let mut id = base.clone();
            let mut number = 2;
            while taken(&id) {
                id = format!("{base}-{number}");
                number += 1;
            }
            if files.is_empty() {
                self.write(&id, &theme)?;
            } else {
                // Every import is checked as a package is: paths, sizes and what each file holds.
                let checked: std::collections::BTreeMap<String, Vec<u8>> = files.iter().cloned().collect();
                if let Err(error) = suhina_theme_package::validate(&checked) {
                    warnings.push(format!("{}: not brought in: {error}", theme.name));
                    continue;
                }
                let folder = self.folder.clone().ok_or("there is no folder to keep themes in")?;
                write_files(&folder, files.into_iter().map(|(path, bytes)| (format!("{id}/{path}"), bytes)).collect())?;
            }
            written.push(ThemeEntry { id, theme });
        }
        Ok(Imported { ids: written.into_iter().map(|e| e.id).collect(), warnings, conflict })
    }

    /// Shows the themes folder in the system's file manager, made first if it is not there.
    pub fn open_folder(&self) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("there is no folder to keep themes in")?;
        std::fs::create_dir_all(folder).map_err(|e| format!("could not make {}: {e}", folder.display()))?;
        reveal(folder, false)
    }

    /// Each theme in the folder: a `<id>/theme.css`, a `<id>.json` not migrated yet, or a
    /// built-in theme's `<id>/variants`; its id, path and when it last changed, by name. A folder
    /// with `theme.css` wins over a JSON file.
    fn list(&self) -> Vec<(String, PathBuf, u64)> {
        let Some(Ok(dir)) = self.folder.as_ref().map(std::fs::read_dir) else { return Vec::new() };
        let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
        for entry in dir.filter_map(Result::ok).take(4096) {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            // A theme's own folder may be a link (a junction on Windows) to a folder elsewhere,
            // such as one in Documents an editor can reach; what is inside it may not be.
            let found = if meta.is_dir() || linked_folder(&path) {
                let css = path.join(THEME_CSS);
                let file = std::fs::symlink_metadata(&css).is_ok_and(|m| m.is_file());
                let name = path.file_name().and_then(|n| n.to_str());
                let variants = path.join(VARIANTS);
                if file {
                    name.map(|id| (id.to_owned(), css))
                } else if name.is_some_and(theme::is_built_in)
                    && std::fs::symlink_metadata(&variants).is_ok_and(|m| m.is_dir())
                {
                    name.map(|id| (id.to_owned(), variants))
                } else {
                    None
                }
            } else if meta.is_file() && path.extension().and_then(|e| e.to_str()) == Some("json") {
                path.file_stem().and_then(|n| n.to_str()).map(|id| (id.to_owned(), path.clone()))
            } else {
                None
            };
            let Some((id, path)) = found.filter(|(id, _)| is_theme_id(id) && !id.contains('@')) else { continue };
            let modified = fingerprint(&path).unwrap_or(0);
            match files.iter_mut().find(|(other, _, _)| *other == id) {
                Some(existing) if is_css_theme(&path) => *existing = (id, path, modified),
                Some(_) => {}
                None => files.push((id, path, modified)),
            }
        }
        files.sort_by_key(|f| f.0.to_lowercase());
        files
    }
}

/// Shows `path` in the system's file manager: the folder, or the file selected where the system
/// can do that.
/// `/select,"<path>"` as Explorer reads it: quoted after the comma. Passed as one ordinary
/// argument, the whole of it is quoted, and Explorer then says the folder is not available when
/// the path has a space.
#[cfg(target_os = "windows")]
fn select_in_explorer(command: &mut std::process::Command, path: &Path) {
    use std::os::windows::process::CommandExt;
    command.raw_arg(format!("/select,\"{}\"", path.display()));
}

#[cfg(not(target_os = "windows"))]
fn select_in_explorer(command: &mut std::process::Command, path: &Path) {
    command.arg(format!("/select,{}", path.display()));
}

pub fn reveal(path: &Path, select: bool) -> Result<(), String> {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("explorer");
        if select {
            select_in_explorer(&mut c, path);
        } else {
            c.arg(path);
        }
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        if select {
            c.arg("-R");
        }
        c.arg(path);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(if select { path.parent().unwrap_or(path) } else { path });
        c
    };
    command.spawn().map(|_| ()).map_err(|e| format!("could not show {}: {e}", path.display()))
}

/// A theme's name in a package path: the file's stem, or the folder of a `theme.css`.
fn stem(name: &str) -> String {
    let mut parts = name.rsplit(['/', '\\']);
    let file = parts.next().unwrap_or(name);
    if file == THEME_CSS {
        return parts.next().unwrap_or_default().to_string();
    }
    file.strip_suffix(".json").unwrap_or(file).to_string()
}

fn is_css_theme(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == THEME_CSS)
}

/// A theme file of any kind: a `theme.css` (or any `.css` theme dropped on the window), one from
/// an earlier version, or JSON.
fn read_any(path: &Path) -> Result<(Theme, Vec<String>), String> {
    if path.extension().is_some_and(|e| e == "css") {
        if previous_format(&read_text(path)?) { read_previous(path) } else { read_css_theme(path) }
    } else {
        read_loose(path)
    }
}

/// A style sheet's text, at most a sheet's size.
fn read_text(path: &Path) -> Result<String, String> {
    use suhina_ui::kit::theme_css;
    String::from_utf8(read_bounded(path, theme_css::MAX_CSS as u64)?).map_err(|e| e.to_string())
}

/// Whether a `theme.css` is in the form of an earlier version: rules of a fixed schema (`meter`,
/// `liquid`, `session` ...) and an imported `advanced.css`, rather than custom properties.
fn previous_format(text: &str) -> bool {
    use suhina_ui::kit::theme_css;
    !theme_css::imports(text).is_empty()
        || text.lines().map(str::trim_start).any(|line| {
            // `meter { image: ... }` is the meter's artwork, in either form.
            [
                "meter {",
                "artwork {",
                "liquid {",
                "session {",
                "gradient-panels {",
                "gradient-tracks {",
                "gradient-clips {",
            ]
            .iter()
            .any(|rule| line.starts_with(rule) && !line.contains("image:"))
        })
}

/// Two themes with the same name, which the picker could not tell apart.
fn same_names(entries: &[ThemeEntry]) -> Vec<String> {
    let mut seen: Vec<(&str, &str)> = Vec::new();
    let mut warnings = Vec::new();
    for entry in entries.iter().filter(|e| !e.id.contains('@')) {
        match seen.iter().find(|(_, name)| name.eq_ignore_ascii_case(&entry.theme.name)) {
            Some((first, name)) => warnings.push(format!(
                "{} and {}: two themes are named {name}; their cards show their folder names",
                first, entry.id
            )),
            None => seen.push((&entry.id, &entry.theme.name)),
        }
    }
    warnings
}

/// A `theme.css` on disk with the `layout.css` beside it, artwork relative to its folder.
fn read_css_theme(path: &Path) -> Result<(Theme, Vec<String>), String> {
    let text = read_text(path)?;
    let folder = path.parent().unwrap_or(Path::new("."));
    let layout = folder.join(LAYOUT_CSS);
    let layout =
        if std::fs::symlink_metadata(&layout).is_ok_and(|m| m.is_file()) { read_text(&layout)? } else { String::new() };
    resolve_css(&text, &layout, |relative, max| read_resource(folder, relative, max))
}

/// A theme from its `theme.css` and `layout.css` text, artwork and background read through
/// `resource`. The sheet is the theme's own values; its variants are read on their own.
fn resolve_css(
    text: &str,
    layout: &str,
    mut resource: impl FnMut(&str, u64) -> Result<Vec<u8>, String>,
) -> Result<(Theme, Vec<String>), String> {
    let (mut theme, warnings) = theme::from_sheet(text, layout);
    resolve_resources(&mut theme, &mut resource)?;
    Ok((theme, warnings))
}

/// A `theme.css` in the form of an earlier version, with its imported `advanced.css`.
fn read_previous(path: &Path) -> Result<(Theme, Vec<String>), String> {
    let text = read_text(path)?;
    let folder = path.parent().unwrap_or(Path::new("."));
    resolve_previous(&text, |relative, max| read_resource(folder, relative, max))
}

fn resolve_previous(
    text: &str,
    mut resource: impl FnMut(&str, u64) -> Result<Vec<u8>, String>,
) -> Result<(Theme, Vec<String>), String> {
    use suhina_ui::kit::theme_css;
    let mut theme = Theme::default();
    let warnings = theme_css::apply(&mut theme, text);
    let imports = theme_css::imports(text);
    if imports.len() > 1 {
        return Err("a theme imports at most one advanced stylesheet".into());
    }
    if let Some(import) = imports.first() {
        let css = String::from_utf8(resource(import, theme_css::MAX_CSS as u64)?).map_err(|e| e.to_string())?;
        suhina_ui::kit::native_css::inspect(&css)?;
        theme.skin.css = (!css.is_empty()).then(|| std::sync::Arc::from(css));
    }
    resolve_resources(&mut theme, &mut resource)?;
    Ok((theme, warnings))
}

/// The variants in `<dir>/variants`, by id, with each file's text; files that are not variants are
/// named in `warnings`.
fn read_variants(dir: &Path, warnings: &mut Vec<String>) -> Vec<(Variant, String)> {
    let Ok(entries) = std::fs::read_dir(dir.join(VARIANTS)) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.filter_map(Result::ok).take(4 * MAX_VARIANTS) {
        let path = entry.path();
        let (Some(stem), Some("json")) =
            (path.file_stem().and_then(|s| s.to_str()), path.extension().and_then(|e| e.to_str()))
        else {
            continue;
        };
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            continue;
        }
        if found.len() >= MAX_VARIANTS {
            warnings.push(format!("{}: only {MAX_VARIANTS} variants are read", dir.display()));
            break;
        }
        let read = read_bounded(&path, MAX_VARIANT_FILE)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
            .and_then(|text| Variant::from_json(stem, &text).map(|(variant, problems)| (variant, problems, text)));
        match read {
            Ok((variant, problems, text)) => {
                warnings.extend(problems);
                found.push((variant, text));
            }
            Err(error) => warnings.push(format!("{}: {error}", path.display())),
        }
    }
    found.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    found
}

/// Whether two themes look the same: every setting, the name, artwork and layout. Their sheets'
/// text may differ (a migrated theme's is new).
fn same_look(a: &Theme, b: &Theme) -> bool {
    let skin = |theme: &Theme| suhina_ui::kit::skin::Skin {
        css: None,
        variant: None,
        variables: Default::default(),
        ..theme.skin.clone()
    };
    theme_vars::core_values(a) == theme_vars::core_values(b) && a.name == b.name && skin(a) == skin(b)
}

/// Replaces artwork and background paths by their contents, within the package limits.
fn resolve_resources(
    theme: &mut Theme,
    resource: &mut impl FnMut(&str, u64) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    use suhina_ui::kit::skin;
    if let Some(background) = &theme.skin.background
        && !background.starts_with("data:image/png;base64,")
    {
        // A large picture (a 4K wallpaper) is scaled down to fit rather than refused.
        let bytes = resource(background, 64 * 1024 * 1024)?;
        let bytes = suhina_ui::kit::artwork_png::fit_background(&bytes)?;
        theme.skin.background = Some(skin::encode_png(&bytes)?);
    }
    let mut artwork_bytes = 0usize;
    for (role, source) in &mut theme.skin.assets {
        if !source.trim_start().starts_with('<') && !skin::png_artwork(source) {
            let png = skin::artwork_extension(source) == "png";
            let data = resource(source, if png { skin::MAX_PNG as u64 } else { skin::MAX_SVG as u64 })?;
            *source = if png {
                skin::encode_png(&data)?.to_string()
            } else {
                String::from_utf8(data).map_err(|e| e.to_string())?
            };
        }
        artwork_bytes = artwork_bytes
            .checked_add(source.len())
            .filter(|total| *total <= skin::MAX_ARTWORK_BYTES)
            .ok_or("combined artwork exceeds 16 MiB")?;
        skin::validate_artwork(source).map_err(|e| format!("{role}: {e}"))?;
    }
    Ok(())
}

/// Migrates the theme `id` of an earlier version, already read as `theme`: a JSON theme
/// (`previous` false) or a `theme.css` that imports `advanced.css`. The new `theme.css` (its values
/// as custom properties, then the free-form CSS it carried) and `layout.css` are read back and must
/// give the same theme; only then are the old files kept in `<id>/legacy/` and the new ones
/// written. On a failure nothing of the theme changes.
fn migrate(folder: &Path, id: &str, theme: &Theme, previous: bool) -> Result<(), String> {
    use suhina_ui::kit::theme_css;
    let dir = folder.join(id);
    let css = dir.join(THEME_CSS);
    if !previous && css.exists() {
        return Err("the theme already has a theme.css".into());
    }
    // The comments at the top of the old canonical sheet stay at the top of the new one.
    let old = if previous { Some(css.clone()) } else { Some(dir.join("style.css")).filter(|p| p.is_file()) };
    let comments = old.as_ref().map(|path| read_text(path)).transpose()?.map(|text| leading_comments(&text));
    let sheet = format!("{}{}", comments.unwrap_or_default(), theme_css::sheet(theme, theme.skin.css.as_deref()));
    let layout = theme_css::layout(theme, &theme::from_sheet(&sheet, "").0);
    write_files(folder, resource_files(id, theme)?)?;
    let (migrated, problems) = resolve_css(&sheet, &layout, |relative, max| read_resource(&dir, relative, max))?;
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    if !same_look(&migrated, theme) {
        return Err("the new theme.css does not read back as the same theme".into());
    }
    let legacy = dir.join(LEGACY);
    std::fs::create_dir_all(&legacy).map_err(|e| e.to_string())?;
    let keep = |from: &Path, name: &str, copy: bool| -> Result<(), String> {
        let mut to = legacy.join(name);
        let mut number = 2;
        while to.exists() {
            to = legacy.join(format!("{number}-{name}"));
            number += 1;
        }
        if copy { std::fs::copy(from, to).map(|_| ()) } else { std::fs::rename(from, to) }.map_err(|e| e.to_string())
    };
    if previous {
        // theme.css is replaced in one step below; its copy is kept first.
        keep(&css, THEME_CSS, true)?;
        for import in theme_css::imports(&read_text(&css)?) {
            let path = dir.join(&import);
            if path.is_file() {
                keep(&path, &import.replace('/', "-"), false)?;
            }
        }
    } else {
        if let Some(style) = old {
            keep(&style, "style.css", false)?;
        }
        keep(&folder.join(format!("{id}.json")), &format!("{id}.json"), false)?;
    }
    let mut files = vec![(format!("{id}/{THEME_CSS}"), sheet.into_bytes())];
    if !layout.is_empty() {
        files.push((format!("{id}/{LAYOUT_CSS}"), layout.into_bytes()));
    }
    files.push((format!("{id}/README.md"), theme::template_readme().into_bytes()));
    write_files(folder, files)
}

/// The comments before a sheet's first rule (past its `@import` statements), each on its own line.
fn leading_comments(text: &str) -> String {
    let mut comments = String::new();
    let mut rest = text.trim_start();
    loop {
        if rest.starts_with("@import")
            && let Some(end) = rest.find(';')
        {
            rest = rest[end + 1..].trim_start();
            continue;
        }
        let Some(comment) = rest.strip_prefix("/*") else { break };
        let Some(end) = comment.find("*/") else { break };
        let body = &comment[..end];
        if !body.contains("Suhina native theme CSS") {
            comments.push_str(&format!("/*{body}*/\n"));
        }
        rest = comment[end + 2..].trim_start();
    }
    comments
}

/// A theme's own sheet for a theme that has none yet: the sheet it was copied with, the variant
/// values it had then written into it, or, for a theme of an earlier version or Suhina Dark, one
/// written from its values.
fn new_sheet(theme: &Theme) -> String {
    use suhina_ui::kit::theme_css;
    match &theme.skin.css {
        Some(css) if !theme.vars.base.is_empty() => {
            let mut text = css.to_string();
            if let Some(values) = theme_vars::variant_sheet(&theme.vars.values) {
                text.push_str("\n/* The values of the variant this theme was made from. */\n");
                text.push_str(&values);
                text.push('\n');
            }
            text
        }
        css => theme_css::sheet(theme, css.as_deref()),
    }
}

/// The theme's artwork and background as files of its folder.
fn resource_files(id: &str, theme: &Theme) -> Result<Vec<(String, Vec<u8>)>, String> {
    use suhina_ui::kit::skin;
    theme
        .skin
        .assets
        .values()
        .try_fold(0usize, |total, source| total.checked_add(source.len()))
        .filter(|total| *total <= skin::MAX_ARTWORK_BYTES)
        .ok_or("combined artwork exceeds 16 MiB")?;
    let mut files = Vec::new();
    if let Some(background) = &theme.skin.background {
        files.push((format!("{id}/background.png"), skin::decode_png(background)?));
    }
    for (role, source) in &theme.skin.assets {
        if !skin::valid_role(role) {
            return Err(format!("unknown SVG role {role}"));
        }
        skin::validate_artwork(source)?;
        let relative = format!("assets/{role}.{}", skin::artwork_extension(source));
        let data = if skin::png_artwork(source) { skin::decode_png(source)? } else { source.as_bytes().to_vec() };
        files.push((format!("{id}/{relative}"), data));
    }
    Ok(files)
}

/// A theme as a package: `theme.css`, `layout.css`, artwork, background, README and examples.
fn package_files(id: &str, theme: &Theme) -> Result<Vec<(String, Vec<u8>)>, String> {
    use suhina_ui::kit::theme_css;
    let sheet = new_sheet(theme);
    if sheet.len() > theme_css::MAX_CSS {
        return Err("theme.css exceeds 128 KiB".into());
    }
    let layout = theme_css::layout(theme, &theme::from_sheet(&sheet, "").0);
    let mut files = resource_files(id, theme)?;
    files.push((format!("{id}/{THEME_CSS}"), sheet.into_bytes()));
    if !layout.is_empty() {
        files.push((format!("{id}/{LAYOUT_CSS}"), layout.into_bytes()));
    }
    files.push((format!("{id}/README.md"), theme::template_readme().into_bytes()));
    files.push((format!("{id}/examples/dark-gloss.css"), theme::GLOSS_EXAMPLE.as_bytes().to_vec()));
    files.push((format!("{id}/examples/compact-session.css"), theme::COMPACT_SESSION_EXAMPLE.as_bytes().to_vec()));
    Ok(files)
}

/// Writes the theme `id`'s own files from `theme`: `theme.css` only when it has none yet,
/// `layout.css` (removed when the theme's sheet already says everything), artwork and
/// background. An installed README and unchanged files are left alone.
fn persist_theme(folder: &Path, id: &str, theme: &Theme) -> Result<(), String> {
    use suhina_ui::kit::theme_css;
    let dir = folder.join(id);
    let sheet_path = dir.join(THEME_CSS);
    let existing = std::fs::symlink_metadata(&sheet_path).is_ok_and(|m| m.is_file());
    let mut files = if existing { resource_files(id, theme)? } else { package_files(id, theme)? };
    if existing {
        let sheet = read_text(&sheet_path)?;
        let layout = theme_css::layout(theme, &theme::from_sheet(&sheet, "").0);
        let path = dir.join(LAYOUT_CSS);
        if layout.is_empty() {
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
        } else {
            files.push((format!("{id}/{LAYOUT_CSS}"), layout.into_bytes()));
        }
    }
    write_files(folder, files)
}

/// The files of the theme folder `id` as they are, for a backup: its sheets, variants, artwork,
/// background and README.
fn folder_files(folder: &Path, id: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    use suhina_ui::kit::{skin, theme_css};
    let dir = folder.join(id);
    let mut files = Vec::new();
    for (name, max) in [
        (THEME_CSS, theme_css::MAX_CSS as u64),
        (LAYOUT_CSS, theme_css::MAX_CSS as u64),
        ("background.png", skin::MAX_PNG as u64),
        ("README.md", MAX_FILE),
    ] {
        if std::fs::symlink_metadata(dir.join(name)).is_ok_and(|m| m.is_file()) {
            files.push((format!("{id}/{name}"), read_resource(&dir, name, max)?));
        }
    }
    for (sub, max, limit) in
        [(VARIANTS, MAX_VARIANT_FILE, MAX_VARIANTS), ("assets", skin::MAX_PNG as u64, skin::MAX_PACKAGE_ASSETS)]
    {
        let Ok(entries) = std::fs::read_dir(dir.join(sub)) else { continue };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .take(limit)
            .collect();
        names.sort();
        for name in names {
            files.push((format!("{id}/{sub}/{name}"), read_resource(&dir, &format!("{sub}/{name}"), max)?));
        }
    }
    Ok(files)
}

fn write_zip(path: &Path, contents: &[(String, Vec<u8>)]) -> Result<PathBuf, String> {
    let failed = |e: &dyn std::fmt::Display| format!("could not write {}: {e}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| failed(&e))?;
    }
    let file = std::fs::File::create(path).map_err(|e| failed(&e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in contents {
        zip.start_file(name.as_str(), options).map_err(|e| failed(&e))?;
        zip.write_all(data).map_err(|e| failed(&e))?;
    }
    zip.finish().map_err(|e| failed(&e))?;
    Ok(path.to_path_buf())
}

/// A theme found in a package: where it is, the theme, warnings, and for a CSS theme its folder's
/// files as they are (relative to the folder), so bringing it in keeps its variants and layout.
struct LoadedTheme {
    name: String,
    theme: Theme,
    warnings: Vec<String>,
    files: Vec<(String, Vec<u8>)>,
}

fn package_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > max {
        return Err(format!("{} exceeds its theme size limit", path.display()));
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("theme file grew beyond its size limit".into());
    }
    Ok(bytes)
}

fn read_loose(path: &Path) -> Result<(Theme, Vec<String>), String> {
    let text = String::from_utf8(read_bounded(path, MAX_FILE)?).map_err(|e| e.to_string())?;
    let root = path.parent().unwrap_or(Path::new("."));
    resolve_theme(&text, |relative, max| {
        if !safe_relative(relative) {
            return Err("invalid relative theme resource path".into());
        }
        let mut file = root.to_path_buf();
        for part in relative.split('/') {
            file.push(part);
            let meta = std::fs::symlink_metadata(&file).map_err(|e| format!("{relative}: {e}"))?;
            if meta.file_type().is_symlink() {
                return Err("theme resources may not be symlinks".into());
            }
        }
        read_bounded(&file, max)
    })
}

fn resolve_theme(
    text: &str,
    mut resource: impl FnMut(&str, u64) -> Result<Vec<u8>, String>,
) -> Result<(Theme, Vec<String>), String> {
    use suhina_ui::kit::theme_css;
    let root: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("invalid theme JSON: {e}"))?;
    if !root.is_object() {
        return Err("a theme must be a JSON object".into());
    }
    let (mut theme, mut warnings) = Theme::from_json(text);
    if let Some(path) = root.get("advanced-stylesheet") {
        let path = path
            .as_str()
            .filter(|p| safe_relative(p) && p.ends_with(".css"))
            .ok_or("advanced-stylesheet must name a relative CSS file")?;
        let css = String::from_utf8(resource(path, theme_css::MAX_CSS as u64)?).map_err(|e| e.to_string())?;
        suhina_ui::kit::native_css::inspect(&css)?;
        theme.skin.css = (!css.is_empty()).then(|| std::sync::Arc::from(css));
    }
    if let Some(path) = root.get("stylesheet") {
        let path = path
            .as_str()
            .filter(|p| safe_relative(p) && p.ends_with(".css"))
            .ok_or("stylesheet must name a relative CSS file")?;
        let css = String::from_utf8(resource(path, theme_css::MAX_CSS as u64)?).map_err(|e| e.to_string())?;
        warnings.extend(theme_css::apply(&mut theme, &css));
        if let Some((folder, _)) = path.rsplit_once('/') {
            let mut css_assets = Theme::default();
            theme_css::apply(&mut css_assets, &css);
            if let Some(background) = css_assets.skin.background {
                theme.skin.background = Some(std::sync::Arc::from(format!("{folder}/{background}")));
            }
            for role in css_assets.skin.assets.keys() {
                if let Some(source) = theme.skin.assets.get_mut(role) {
                    *source = format!("{folder}/{source}");
                }
            }
        }
    }
    resolve_resources(&mut theme, &mut resource)?;
    Ok((theme, warnings))
}

/// Writes `files` under `folder` atomically, `theme.css` last, leaving unchanged files and an
/// installed README alone.
fn write_files(folder: &Path, mut files: Vec<(String, Vec<u8>)>) -> Result<(), String> {
    // theme.css commits the snapshot last. Temp files live beside their destinations for atomic renames.
    files.sort_by_key(|(name, _)| name.ends_with(THEME_CSS));
    for (name, data) in files {
        if !safe_relative(&name) {
            return Err("invalid theme destination".into());
        }
        let path = folder.join(&name);
        let mut current = folder.to_path_buf();
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        for (depth, component) in name.split('/').enumerate() {
            current.push(component);
            // The theme's own folder may be a linked one (see `list`); nothing under it may.
            if depth == 0 && linked_folder(&current) {
                continue;
            }
            if std::fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("theme destination may not be a symlink".into());
            }
        }
        // Installed author guides belong to their package version. New packages
        // get the current guide; editing a colour must not replace a user's copy.
        if name.ends_with("/README.md") && path.is_file() {
            continue;
        }
        // Keep unchanged artwork and documentation in place. In particular, forcing
        // dozens of identical SVGs to disk made each theme slider tick take 100+ ms.
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() == data.len() as u64)
            && read_bounded(&path, data.len() as u64).is_ok_and(|existing| existing == data)
        {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        if temporary.exists() {
            return Err(format!("unfinished theme write: {}", temporary.display()));
        }
        use std::fs::OpenOptions;
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&data).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A portable theme carries its effective artwork; local files retain sparse overrides.
fn portable_package_files(id: &str, theme: &Theme) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut portable = theme.clone();
    let mut assets = suhina_ui::kit::skin::inherited_assets();
    for role in suhina_ui::kit::skin::ROLES {
        if theme.skin.assets.contains_key(*role) {
            for suffix in [".hover", ".active", ".disabled"] {
                assets.remove(&format!("{role}{suffix}"));
            }
        }
    }
    assets.extend(theme.skin.assets.clone());
    portable.skin.assets = assets;
    package_files(id, &portable)
}

fn read_default_assets(folder: &Path, warnings: &mut Vec<String>) -> Result<(), String> {
    use suhina_ui::kit::skin;
    let mut assets = skin::default_assets();
    let mut edited = Vec::new();
    let mut invalid = Vec::new();
    let mut directory = folder.to_path_buf();
    for component in ["_defaults", "assets"] {
        directory.push(component);
        if std::fs::symlink_metadata(&directory).is_ok_and(|meta| meta.file_type().is_symlink() || !meta.is_dir()) {
            return Err("default SVG directories must be ordinary directories".into());
        }
    }
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let backup = folder.join("_defaults/suhina-default.zip");
    let mut pristine = Theme { name: "Suhina Default".into(), ..Default::default() };
    pristine.skin.assets = assets.clone();
    match std::fs::symlink_metadata(&backup) {
        Ok(meta) if !meta.file_type().is_file() => return Err("default backup must be an ordinary file".into()),
        Ok(_) => {
            if !read_zip(&backup).is_ok_and(|entries| entries.len() == 1 && same_look(&entries[0].theme, &pristine)) {
                let saved = (1..=9999)
                    .map(|n| folder.join(format!("_defaults/suhina-default-backup-recovery-{n}.zip")))
                    .find(|p| std::fs::symlink_metadata(p).is_err())
                    .ok_or("too many backup recovery files")?;
                std::fs::rename(&backup, &saved).map_err(|e| e.to_string())?;
                write_zip(&backup, &package_files("suhina-default", &pristine)?)?;
                warnings
                    .push(format!("Changed default backup preserved in {}; pristine backup restored", saved.display()));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            write_zip(&backup, &package_files("suhina-default", &pristine)?)?;
        }
        Err(error) => return Err(error.to_string()),
    }
    for (role, source) in skin::DEFAULT_SVGS {
        let path = directory.join(format!("{role}.svg"));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => file.write_all(source.as_bytes()).map_err(|e| e.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    for role in skin::ROLES {
        for suffix in ["", ".hover", ".active", ".disabled"] {
            let key = format!("{role}{suffix}");
            let path = directory.join(format!("{key}.svg"));
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    warnings.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            let result: Result<String, String> = (|| {
                if !metadata.file_type().is_file() {
                    return Err("default SVG must be an ordinary file".into());
                }
                let source =
                    String::from_utf8(read_bounded(&path, skin::MAX_SVG as u64)?).map_err(|e| e.to_string())?;
                skin::parse_svg(&source)?;
                Ok(source)
            })();
            match result {
                Ok(source) => {
                    if assets.get(&key) != Some(&source) {
                        edited.push((key.clone(), path, source.clone()));
                        assets.insert(key, source);
                    }
                }
                Err(error) => {
                    warnings.push(format!("{}: {error}; using embedded default", path.display()));
                    if metadata.file_type().is_file()
                        && let Ok(bytes) = read_bounded(&path, skin::MAX_SVG as u64)
                    {
                        invalid.push((key, path, bytes));
                    }
                }
            }
        }
    }
    if !invalid.is_empty() {
        let exports = folder.parent().ok_or("no theme recovery folder")?.join("exports");
        let recovery = (1..=9999)
            .map(|n| exports.join(format!("suhina-default-recovery-{n}.zip")))
            .find(|p| std::fs::symlink_metadata(p).is_err())
            .ok_or("too many default recovery archives")?;
        let contents: Vec<_> =
            invalid.iter().map(|(key, _, bytes)| (format!("assets/{key}.svg"), bytes.clone())).collect();
        write_zip(&recovery, &contents)?;
        for (key, path, bytes) in invalid {
            if !restore_default_asset(&path, &key, &bytes)? {
                warnings.push("Default SVG changed again during recovery; newer edit was kept".into());
            }
        }
        warnings
            .push(format!("Malformed default SVG edits preserved in {} before restoring defaults", recovery.display()));
    }
    if !edited.is_empty() {
        let number = (1..=9999)
            .find(|number| {
                let id = format!("suhina-default-modified-{number}");
                std::fs::symlink_metadata(folder.join(format!("{id}.json"))).is_err()
                    && std::fs::symlink_metadata(folder.join(&id)).is_err()
            })
            .ok_or("too many modified default themes")?;
        let id = format!("suhina-default-modified-{number}");
        let mut modified = Theme { name: format!("Suhina Default (modified {number})"), ..Default::default() };
        modified.skin.assets = assets;
        // Commit the user's copy first. A failure leaves every edited original intact.
        persist_theme(folder, &id, &modified)?;
        for (key, path, source) in edited {
            if !restore_default_asset(&path, &key, source.as_bytes())? {
                warnings.push("Default SVG changed again during recovery; newer edit was kept".into());
            }
        }
        warnings.push(format!("Default SVG edits saved as {} ({id}); original defaults restored", modified.name));
    }
    Ok(())
}

fn restore_default_asset(path: &Path, key: &str, expected: &[u8]) -> Result<bool, String> {
    use suhina_ui::kit::skin;
    if !std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file()) {
        return Ok(false);
    }
    if read_bounded(path, skin::MAX_SVG as u64)? != expected {
        return Ok(false);
    }
    if let Some((_, original)) = skin::DEFAULT_SVGS.iter().find(|(role, _)| *role == key) {
        let temporary = path.with_extension(format!("restore-{}", std::process::id()));
        let mut file =
            std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(original.as_bytes()).map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(temporary, path).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(true)
}

fn read_zip(path: &Path) -> Result<Vec<LoadedTheme>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    if archive.len() > MAX_ZIP_FILES {
        return Err("too many theme package files".into());
    }
    let mut files = std::collections::BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|e| e.to_string())?.to_string();
        if !safe_relative(&name) || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(format!("unsafe theme package path {name}"));
        }
        let limit = if name.ends_with(".svg") {
            suhina_ui::kit::skin::MAX_SVG as u64
        } else if name.ends_with(".png") {
            suhina_ui::kit::skin::MAX_PNG as u64
        } else if name.ends_with(".css") {
            suhina_ui::kit::theme_css::MAX_CSS as u64
        } else {
            MAX_FILE
        };
        total = total.checked_add(entry.size()).ok_or("theme package size overflow")?;
        if entry.size() > limit || total > MAX_EXPANDED {
            return Err("theme package exceeds expanded size limit".into());
        }
        let mut bytes = Vec::new();
        (&mut entry).take(limit + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > limit {
            return Err("theme package entry exceeds size limit".into());
        }
        if files.insert(name, bytes).is_some() {
            return Err("duplicate theme package path".into());
        }
    }
    let mut themes = Vec::new();
    // A package holds CSS themes (`<id>/theme.css`) or, from older versions, JSON themes; a JSON
    // theme in a folder that also has a theme.css is that theme's legacy copy.
    let css_folders: std::collections::BTreeSet<&str> = files
        .keys()
        .filter_map(|name| name.strip_suffix(THEME_CSS))
        .map(|folder| folder.trim_end_matches('/'))
        .collect();
    for (name, bytes) in &files {
        let css = name.ends_with(&format!("/{THEME_CSS}")) || name == THEME_CSS;
        let json = name.ends_with(".json")
            && name != TEMPLATE
            && !name.contains(&format!("/{LEGACY}/"))
            && !name.contains(&format!("{VARIANTS}/"))
            && !css_folders.contains(name.strip_suffix(".json").unwrap_or_default());
        if (!css && !json) || name.starts_with(EXAMPLES) {
            continue;
        }
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let folder = name.rsplit_once('/').map_or("", |(folder, _)| folder);
        let resource = |relative: &str, max: u64| {
            if !safe_relative(relative) {
                return Err("invalid relative resource path".to_string());
            }
            let path = if folder.is_empty() { relative.to_string() } else { format!("{folder}/{relative}") };
            let data = files.get(&path).ok_or_else(|| format!("missing theme resource {path}"))?;
            if data.len() as u64 > max {
                return Err(format!("{path} exceeds resource size limit"));
            }
            Ok(data.clone())
        };
        let (theme, warnings, own) = if css && !previous_format(text) {
            let layout = files
                .get(&if folder.is_empty() { LAYOUT_CSS.to_string() } else { format!("{folder}/{LAYOUT_CSS}") })
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                .unwrap_or_default();
            let (theme, warnings) = resolve_css(text, &layout, resource)?;
            let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
            let own: Vec<(String, Vec<u8>)> = files
                .iter()
                .filter_map(|(path, bytes)| Some((path.strip_prefix(&prefix)?.to_owned(), bytes.clone())))
                .filter(|(path, _)| !path.starts_with(&format!("{LEGACY}/")) && !path.starts_with(EXAMPLES))
                .collect();
            (theme, warnings, own)
        } else if css {
            let (theme, warnings) = resolve_previous(text, resource)?;
            (theme, warnings, Vec::new())
        } else {
            let (theme, warnings) = resolve_theme(text, resource)?;
            (theme, warnings, Vec::new())
        };
        themes.push(LoadedTheme { name: name.clone(), theme, warnings, files: own });
    }
    if themes.is_empty() {
        return Err("there are no themes in this package".into());
    }
    Ok(themes)
}

/// Whether `path` is a link (a symbolic link, or a junction on Windows) to a folder.
fn linked_folder(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
        && std::fs::metadata(path).is_ok_and(|m| m.is_dir())
}

#[cfg(test)]
#[path = "theme_files_tests.rs"]
mod tests;

fn read_resource(folder: &Path, relative: &str, max: u64) -> Result<Vec<u8>, String> {
    if !safe_relative(relative) {
        return Err("invalid relative theme path".to_string());
    }
    let mut path = folder.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        if std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?.file_type().is_symlink() {
            return Err("theme resources may not be symlinks".into());
        }
    }
    read_bounded(&path, max)
}

/// A bounded resource fingerprint catches CSS and SVG edits as well as JSON edits.
fn fingerprint(path: &Path) -> Result<u64, String> {
    use std::hash::{Hash, Hasher};
    let mut paths = vec![path.to_path_buf()];
    // A CSS theme is its folder; a JSON theme has its resources in the folder of its name.
    let folder = if is_css_theme(path) {
        path.parent().map_or_else(|| path.with_extension(""), Path::to_path_buf)
    } else {
        path.with_extension("")
    };
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.take(MAX_ZIP_FILES).filter_map(Result::ok) {
            let meta = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if meta.file_type().is_symlink() {
                continue;
            }
            paths.push(entry.path());
            if meta.is_dir()
                && let Ok(children) = std::fs::read_dir(entry.path())
            {
                paths.extend(
                    children
                        .take(suhina_ui::kit::skin::MAX_PACKAGE_ASSETS)
                        .filter_map(Result::ok)
                        .map(|child| child.path()),
                );
            }
        }
    }
    paths.sort();
    let mut hash = std::hash::DefaultHasher::new();
    for path in paths {
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        path.hash(&mut hash);
        meta.len().hash(&mut hash);
        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH).hash(&mut hash);
    }
    Ok(hash.finish())
}

#[cfg(test)]
#[path = "theme_files_skin_tests.rs"]
mod skin_package_tests;
