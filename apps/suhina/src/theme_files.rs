// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme files: one folder per theme in the `themes` folder beside the settings file, named by the
//! theme's id (ADR 0031, amendment *Theme settings and variants*):
//!
//! - `theme.json`, the theme's metadata: its name, author, description, swatch and so on
//!   (`suhina_theme_package::meta`). Suhina writes it when the theme is renamed.
//! - `theme.css`, the theme's style and layout. What Suhina's theme editors set beyond settings
//!   (placements, the toolbar, artwork) goes into it through the source-preserving editor
//!   (`suhina_style::edit`), which changes only the declarations it edits.
//! - `variants/<id>.json`, values for the theme's declared controls (`theme_vars::Variant`). The
//!   settings editors write these.
//! - an optional `thumbnail.svg`, artwork in `assets/`, `background.png` and the README.
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
/// A theme's metadata: `<id>/theme.json`.
const THEME_JSON: &str = theme::META;
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
    /// The folder names last given to the window as taken, so a change is sent once.
    reserved: Vec<String>,
    /// How many entries of the themes folder are looked at for names.
    max_folders: usize,
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
            reserved: Vec::new(),
            max_folders: 4096,
        }
    }

    /// The names of every folder in the themes folder, themes or not: an id the app must not give
    /// a new theme, since writing there would change files that are not its own.
    fn folder_names(&self) -> Vec<String> {
        let Some(Ok(dir)) = self.folder.as_ref().map(std::fs::read_dir) else { return Vec::new() };
        let mut names: Vec<String> = dir
            .filter_map(Result::ok)
            .take(self.max_folders)
            .filter(|e| std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir()))
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect();
        names.sort();
        names
    }

    /// The folder names, when they differ from those last given.
    pub fn reserved_changed(&mut self) -> Option<Vec<String>> {
        let names = self.folder_names();
        (names != self.reserved).then(|| {
            self.reserved.clone_from(&names);
            names
        })
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
                // A folder with a theme.json is today's form, whatever its sheet holds: the file
                // is the theme's metadata, and a migration would write it anew.
                if dir.join(THEME_JSON).is_file() || !previous_format(&text) {
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
                    // A theme of the format before theme.json: its name and swatch are needed.
                    let migrated = read_text(&path.join(THEME_CSS)).is_ok_and(|text| previous_format(&text));
                    return (!migrated && !path.join(THEME_JSON).is_file()).then(|| {
                        format!("{name}: not a theme, it has a theme.css but no theme.json (see the theme guide)")
                    });
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
        if !theme::is_built_in(base) {
            // Before anything is written, a variant's file included.
            writable(&folder, base, true)?;
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
        // Suhina's own folders (starting with `_`) and the template are no theme of the user's.
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

    /// Writes `exports/suhina-theme-template.zip`: `my-theme/` (Suhina Studio's sheet with every
    /// colour written out, its artwork and an example variant), `README.md` on the format, and the
    /// built-in themes in `examples/`. Returns its path.
    pub fn export_template(&self) -> Result<PathBuf, String> {
        let mut template = Theme { name: "My theme".into(), ..Theme::default() };
        template.skin.assets = suhina_ui::kit::skin::inherited_assets();
        template.skin.toolbar = Some(Default::default());
        let mut contents = vec![("README.md".to_string(), theme::template_readme().into_bytes())];
        // The built-in themes as their folders are, as examples.
        for (id, _) in theme::built_in_themes() {
            for (name, data) in theme::built_in_files(id).unwrap_or_default() {
                contents.push((format!("{EXAMPLES}{id}/{name}"), data.to_vec()));
            }
        }
        contents.extend(portable_package_files("my-theme", &template)?);
        let example = Variant {
            id: "warm".into(),
            name: "Warm".into(),
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
        let built_in = theme::built_in_files(id).filter(|_| theme::is_built_in(id));
        let files = match (dir, built_in) {
            (Some(dir), _) => suhina_theme_package::read_folder(&dir)?,
            // A built-in theme's folder as it is built into the app.
            (None, Some(files)) => {
                let mut files: std::collections::BTreeMap<String, Vec<u8>> =
                    files.into_iter().map(|(path, bytes)| (path.to_owned(), bytes.to_vec())).collect();
                // The variants the user keeps for it travel with it (`<id>/variants/` on disk).
                let mut warnings = Vec::new();
                if let Some(folder) = &self.folder {
                    for (variant, text) in read_variants(&folder.join(id), &mut warnings) {
                        files.insert(format!("{VARIANTS}/{}.json", variant.id), text.into_bytes());
                    }
                }
                files
            }
            (None, None) => {
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
            let (theme, problems) =
                resolve_files(&package.files).map_err(|e| format!("{}: {e}", package_name(path)))?;
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
            let (loaded, skipped) = read_zip(path)?;
            warnings.extend(skipped);
            for loaded in loaded {
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
                let text = read_text(path)?;
                let mut files = vec![
                    (THEME_JSON.to_string(), theme::meta_json(&theme).into_bytes()),
                    (THEME_CSS.to_string(), text.clone().into_bytes()),
                ];
                // The artwork and thumbnail it names come with it, or the next reload would refuse it.
                files.extend(sheet_resources(path, &text)?);
                files
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
        // Every folder is taken, one that is not a theme (yet) as well: it is not the import's to write in.
        let folders = self.folder_names();
        let existing: Vec<ThemeEntry> = existing.into_iter().filter(|e| !e.id.contains('@')).collect();
        let mut written: Vec<ThemeEntry> = Vec::new();
        for (stem, theme, files) in found {
            if again == Again::Ask && existing.iter().chain(written.iter()).any(|e| same_look(&e.theme, &theme)) {
                continue;
            }
            let base = slug(if stem.is_empty() { &theme.name } else { &stem });
            let taken = |id: &str| {
                theme::is_built_in(id)
                    // The folder itself, in case the scan of the names above stopped short.
                    || self.folder.as_ref().is_some_and(|f| std::fs::symlink_metadata(f.join(id)).is_ok())
                    || folders.iter().any(|f| f.eq_ignore_ascii_case(id))
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
                let is_file = |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file());
                // A theme folder, or one of the form before `theme.json` that is migrated.
                let file = is_file(&css)
                    && (is_file(&path.join(THEME_JSON)) || read_text(&css).is_ok_and(|text| previous_format(&text)));
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
    let file = file.strip_suffix(".json").or_else(|| file.strip_suffix(".css")).unwrap_or(file);
    file.strip_suffix(".theme").unwrap_or(file).to_string()
}

fn is_css_theme(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == THEME_CSS)
}

/// A theme file of any kind: a `theme.css` (or any `.css` theme dropped on the window), one from
/// an earlier version, or JSON.
fn read_any(path: &Path) -> Result<(Theme, Vec<String>), String> {
    if path.extension().is_some_and(|e| e == "css") {
        let text = read_text(path)?;
        let today = is_css_theme(path) && path.with_file_name(THEME_JSON).is_file();
        if today {
            read_css_theme(path)
        } else if previous_format(&text) {
            read_previous(path)
        } else {
            read_lone_sheet(path, &text)
        }
    } else {
        read_loose(path)
    }
}

/// A style sheet dropped on its own, without a `theme.json`: a theme named after the file, with
/// the default swatch. Its artwork is relative to the file's folder.
fn read_lone_sheet(path: &Path, text: &str) -> Result<(Theme, Vec<String>), String> {
    let folder = path.parent().unwrap_or(Path::new("."));
    lone_sheet(path, text, |relative, max| read_resource(folder, relative, max))
}

/// [`read_lone_sheet`] with the files it reads given by `resource`.
fn lone_sheet(
    path: &Path,
    text: &str,
    mut resource: impl FnMut(&str, u64) -> Result<Vec<u8>, String>,
) -> Result<(Theme, Vec<String>), String> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Theme");
    let name = stem.strip_suffix(".theme").unwrap_or(stem);
    let name = if name == "theme" { "Theme" } else { name };
    let (mut theme, warnings) = theme::from_folder(theme::ThemeMeta::new(name, theme::DEFAULT_SWATCH), text);
    // The swatch as the colour it is here, since the sheet alone may not say every value it needs.
    theme.meta.swatch = theme::hex(theme.swatch.unwrap_or(theme.colors.panel_background));
    theme::resolve_resources(&mut theme, &mut resource)?;
    Ok((theme, warnings))
}

/// The files beside a dropped sheet that its theme reads (artwork, background, thumbnail), by path
/// relative to the sheet's folder, so an import keeps what the sheet names.
fn sheet_resources(path: &Path, text: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let folder = path.parent().unwrap_or(Path::new("."));
    let mut seen: Vec<(String, Vec<u8>)> = Vec::new();
    if is_css_theme(path) && path.with_file_name(THEME_JSON).is_file() {
        theme::from_files(|relative, max| {
            if !safe_relative(relative) {
                return Err(format!("{relative}: not a path inside the theme"));
            }
            if std::fs::symlink_metadata(folder.join(relative)).is_err() {
                return Ok(None);
            }
            let bytes = read_resource(folder, relative, max)?;
            seen.push((relative.to_owned(), bytes.clone()));
            Ok(Some(bytes))
        })?;
    } else {
        lone_sheet(path, text, |relative, max| {
            let bytes = read_resource(folder, relative, max)?;
            seen.push((relative.to_owned(), bytes.clone()));
            Ok(bytes)
        })?;
    }
    seen.retain(|(relative, _)| relative != THEME_JSON && relative != THEME_CSS);
    seen.dedup_by(|a, b| a.0 == b.0);
    Ok(seen)
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

/// A theme folder on disk, from its `theme.css`'s path: `theme.json`, the sheet, the thumbnail
/// and the artwork and background the sheet names, relative to the folder.
fn read_css_theme(path: &Path) -> Result<(Theme, Vec<String>), String> {
    let folder = path.parent().unwrap_or(Path::new("."));
    theme::from_files(|relative, max| {
        if !safe_relative(relative) {
            return Err(format!("{relative}: not a path inside the theme"));
        }
        if std::fs::symlink_metadata(folder.join(relative)).is_err() {
            return Ok(None);
        }
        read_resource(folder, relative, max).map(Some)
    })
}

/// A theme from its folder's files held in memory, by path relative to the folder.
fn resolve_files(files: &std::collections::BTreeMap<String, Vec<u8>>) -> Result<(Theme, Vec<String>), String> {
    theme::from_files(|relative, _| Ok(files.get(relative).cloned()))
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
    theme::resolve_resources(&mut theme, &mut resource)?;
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
/// text may differ (a migrated theme's is new), and so may the styles the style engine paints from
/// the sheet: only the `--suhina-*` ones are the app's own.
fn same_look(a: &Theme, b: &Theme) -> bool {
    let skin = |theme: &Theme| suhina_ui::kit::skin::Skin {
        css: None,
        variant: None,
        variables: Default::default(),
        styles: theme
            .skin
            .styles
            .iter()
            .map(|(selector, properties)| {
                let own = properties.iter().filter(|(property, _)| property.starts_with("--suhina-"));
                (selector.clone(), own.map(|(k, v)| (k.clone(), v.clone())).collect())
            })
            .filter(|(_, properties): &(String, std::collections::BTreeMap<String, String>)| !properties.is_empty())
            .collect(),
        ..theme.skin.clone()
    };
    theme_vars::core_values(a) == theme_vars::core_values(b) && a.name == b.name && skin(a) == skin(b)
}

/// Migrates the theme `id` of an earlier version, already read as `theme`: a JSON theme
/// (`previous` false) or a `theme.css` that imports `advanced.css`. The new `theme.json` and
/// `theme.css` (its values as custom properties, the free-form CSS it carried, then what its
/// editors set) are read back and must give the same theme; only then are the old files kept in
/// `<id>/legacy/` and the new ones written. On a failure nothing of the theme changes.
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
    let sheet = with_editor_rules(theme, sheet);
    let resources = resource_files(id, theme, &artwork_paths(theme, None, None))?;
    let prefix = format!("{id}/");
    let mut files: std::collections::BTreeMap<String, Vec<u8>> = resources
        .iter()
        .filter_map(|(path, bytes)| Some((path.strip_prefix(&prefix)?.to_owned(), bytes.clone())))
        .collect();
    files.insert(THEME_CSS.into(), sheet.clone().into_bytes());
    files.insert(THEME_JSON.into(), theme::meta_json(theme).into_bytes());
    let (migrated, problems) = resolve_files(&files)?;
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
    // The imports are read before theme.css is overwritten.
    let imports = if previous { theme_css::imports(&read_text(&css)?) } else { Vec::new() };
    if previous {
        // theme.css is overwritten below; its copy is kept first.
        keep(&css, THEME_CSS, true)?;
    }
    let mut files = resources;
    files.push((format!("{id}/{THEME_JSON}"), theme::meta_json(theme).into_bytes()));
    files.push((format!("{id}/{THEME_CSS}"), sheet.into_bytes()));
    files.push((format!("{id}/README.md"), theme::template_readme().into_bytes()));
    write_files(folder, files)?;
    // The old files move only after the new theme is written, so a failed write leaves the theme
    // as it was.
    if previous {
        for import in imports {
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
    Ok(())
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
/// values it had then written into it, or, for a theme of an earlier version or Suhina Studio, one
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

/// Where each of the theme's artwork files is kept: the path its sheet names for the role (an
/// edited image keeps its file), or `assets/<role>.svg` (`.png`). When several roles name one file
/// (`dir` is the theme's folder, to see what the file holds now), only a role whose artwork is
/// what the file holds keeps it; an edited one gets a file of its own, so it does not overwrite
/// the others' artwork.
fn artwork_paths(
    theme: &Theme,
    sheet: Option<&Theme>,
    dir: Option<&Path>,
) -> std::collections::BTreeMap<String, String> {
    use suhina_ui::kit::skin;
    let data = |source: &str| -> Option<Vec<u8>> {
        if skin::png_artwork(source) { skin::decode_png(source).ok() } else { Some(source.as_bytes().to_vec()) }
    };
    theme
        .skin
        .assets
        .iter()
        .map(|(role, source)| {
            let extension = skin::artwork_extension(source);
            let named = sheet.and_then(|sheet| sheet.skin.assets.get(role)).filter(|path| {
                skin::asset_path(path) && safe_relative(path) && skin::artwork_extension(path) == extension
            });
            let shared = |path: &String| {
                sheet.is_some_and(|sheet| sheet.skin.assets.iter().any(|(other, named)| other != role && named == path))
            };
            let named = named.filter(|path| {
                !shared(path)
                    || dir
                        .and_then(|dir| read_bounded(&dir.join(path), skin::MAX_PNG as u64).ok())
                        .is_some_and(|held| data(source).is_some_and(|mine| held == mine))
            });
            (role.clone(), named.cloned().unwrap_or_else(|| format!("assets/{role}.{extension}")))
        })
        .collect()
}

/// The theme's artwork and background as files of its folder, the artwork at `paths`.
fn resource_files(
    id: &str,
    theme: &Theme,
    paths: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
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
        let relative =
            paths.get(role).cloned().unwrap_or_else(|| format!("assets/{role}.{}", skin::artwork_extension(source)));
        let data = if skin::png_artwork(source) { skin::decode_png(source)? } else { source.as_bytes().to_vec() };
        files.push((format!("{id}/{relative}"), data));
    }
    Ok(files)
}

/// `sheet` with what Suhina's theme editors set that it does not say yet (placements, the toolbar,
/// artwork) appended, for a theme written anew.
fn with_editor_rules(theme: &Theme, sheet: String) -> String {
    use suhina_ui::kit::theme_css;
    let rules = theme_css::layout(theme, &theme::from_css(&sheet).0);
    if rules.is_empty() {
        sheet
    } else {
        format!("{}\n\n/* Set in Suhina's theme editors. */\n{rules}", sheet.trim_end())
    }
}

/// A theme as a package: `theme.json`, `theme.css`, the thumbnail, artwork, background, README
/// and examples.
fn package_files(id: &str, theme: &Theme) -> Result<Vec<(String, Vec<u8>)>, String> {
    use suhina_ui::kit::theme_css;
    let sheet = with_editor_rules(theme, new_sheet(theme));
    if sheet.len() > theme_css::MAX_CSS {
        return Err("theme.css exceeds 128 KiB".into());
    }
    let mut files = resource_files(id, theme, &artwork_paths(theme, None, None))?;
    files.push((format!("{id}/{THEME_JSON}"), theme::meta_json(theme).into_bytes()));
    files.push((format!("{id}/{THEME_CSS}"), sheet.into_bytes()));
    if let Some(thumbnail) = &theme.thumbnail {
        files.push((format!("{id}/{}", theme::THUMBNAIL), thumbnail.as_bytes().to_vec()));
    }
    files.push((format!("{id}/README.md"), theme::template_readme().into_bytes()));
    files.push((format!("{id}/examples/dark-gloss.css"), theme::GLOSS_EXAMPLE.as_bytes().to_vec()));
    files.push((format!("{id}/examples/compact-session.css"), theme::COMPACT_SESSION_EXAMPLE.as_bytes().to_vec()));
    Ok(files)
}

/// Whether the theme `id`'s folder is the theme's to write in: not there, empty, or a theme with a
/// `theme.json`. A folder with a `theme.css` and no `theme.json`, or with other files and no
/// `theme.css`, is not a theme (yet), and what is in it is not ours to edit. `first` is the check
/// before a save writes anything: a `variants` folder that is there then is not ours either, while
/// after a variant's file is written it is the save's own.
fn writable(folder: &Path, id: &str, first: bool) -> Result<(), String> {
    let dir = folder.join(id);
    let is_file = |name: &str| std::fs::symlink_metadata(dir.join(name)).is_ok_and(|m| m.is_file());
    if is_file(THEME_CSS) {
        if !is_file(THEME_JSON) {
            return Err(format!("{id} has a theme.css but no theme.json, so it was not changed"));
        }
    } else if holds_other_files(&dir, first) {
        return Err(format!("{id} is a folder that is not a theme, so nothing was written into it"));
    }
    Ok(())
}

/// Whether `dir` exists and holds something, but for a `variants` folder when `variants_too` is not
/// set (a variant saved just before the theme's own files makes one).
fn holds_other_files(dir: &Path, variants_too: bool) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return std::fs::symlink_metadata(dir).is_ok() };
    entries.filter_map(Result::ok).any(|e| variants_too || e.file_name() != VARIANTS)
}

/// Writes the theme `id`'s own files from `theme`. A theme without a `theme.css` yet gets all of
/// them. Otherwise what its editors changed (placements, the toolbar, artwork) is edited into its
/// `theme.css` in place, leaving the rest of the author's text as it is; `theme.json` is written
/// when the name changed; artwork and the background are written. Unchanged files and an
/// installed README are left alone.
fn persist_theme(folder: &Path, id: &str, theme: &Theme) -> Result<(), String> {
    use suhina_ui::kit::theme_css;
    let dir = folder.join(id);
    let sheet_path = dir.join(THEME_CSS);
    writable(folder, id, false)?;
    if !std::fs::symlink_metadata(&sheet_path).is_ok_and(|m| m.is_file()) {
        return write_files(folder, package_files(id, theme)?);
    }
    let source = read_text(&sheet_path)?;
    let sheet = theme::from_css(&source).0;
    let paths = artwork_paths(theme, Some(&sheet), Some(&dir));
    let mut files = resource_files(id, theme, &paths)?;
    // The editors' rules as the sheet says them now and as the theme has them, the artwork at the
    // paths it is kept at; only the declarations that differ are edited.
    let mut kept = theme.clone();
    for (role, source) in &mut kept.skin.assets {
        source.clone_from(&paths[role]);
    }
    let nothing = Theme::default();
    let (before, after) = (theme_css::layout(&sheet, &nothing), theme_css::layout(&kept, &nothing));
    if before != after {
        let edited = suhina_ui::kit::theme_css::edit_sheet(&source, &before, &after)
            .map_err(|e| format!("{id}/theme.css was not changed: {e}"))?;
        if edited != source {
            files.push((format!("{id}/{THEME_CSS}"), edited.into_bytes()));
        }
    }
    let written = read_resource(&dir, THEME_JSON, theme::MAX_META)
        .ok()
        .and_then(|bytes| theme::ThemeMeta::parse(&bytes).ok())
        .map(|(meta, _)| meta);
    if written != Some(theme::file_meta(theme)) {
        files.push((format!("{id}/{THEME_JSON}"), theme::meta_json(theme).into_bytes()));
    }
    write_files(folder, files)
}

/// The files of the theme folder `id` as they are, for a backup: its metadata, sheet, thumbnail,
/// variants, artwork, background and README.
fn folder_files(folder: &Path, id: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    use suhina_ui::kit::{skin, theme_css};
    let dir = folder.join(id);
    let mut files = Vec::new();
    for (name, max) in [
        (THEME_JSON, theme::MAX_META),
        (THEME_CSS, theme_css::MAX_CSS as u64),
        (theme::THUMBNAIL, skin::MAX_SVG as u64),
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
    theme::resolve_resources(&mut theme, &mut resource)?;
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

/// The themes in a zip (a backup or a package), and a warning for each folder that is not brought
/// in, so one that cannot be read does not keep the others out.
fn read_zip(path: &Path) -> Result<(Vec<LoadedTheme>, Vec<String>), String> {
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
    let mut skipped = Vec::new();
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
            && name.rsplit('/').next() != Some(THEME_JSON)
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
        let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        let today = files.contains_key(&format!("{prefix}{THEME_JSON}"));
        let (theme, warnings, own) = if css && (today || !previous_format(text)) {
            let own: Vec<(String, Vec<u8>)> = files
                .iter()
                .filter_map(|(path, bytes)| Some((path.strip_prefix(&prefix)?.to_owned(), bytes.clone())))
                .filter(|(path, _)| !path.starts_with(&format!("{LEGACY}/")) && !path.starts_with(EXAMPLES))
                .collect();
            match resolve_files(&own.iter().cloned().collect()) {
                Ok((theme, warnings)) => (theme, warnings, own),
                Err(error) => {
                    skipped.push(format!(
                        "{}: not brought in: {error}",
                        if folder.is_empty() { THEME_CSS } else { folder }
                    ));
                    continue;
                }
            }
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
        return Err(if skipped.is_empty() { "there are no themes in this package".into() } else { skipped.join("; ") });
    }
    Ok((themes, skipped))
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
