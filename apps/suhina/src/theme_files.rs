// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme files: one JSON file per theme in the `themes` folder beside the settings file, named
//! by the theme's id (`night-blue.json`). A file saved while Suhina runs is read again within a
//! second, so a theme can be edited in any text editor and seen right away. Backups and the theme
//! template are zips written to the `exports` folder beside it; theme files and zips dropped on
//! the window are brought in.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use suhina_ui::kit::theme::{self, Theme};
use suhina_ui::settings::is_theme_id;
use suhina_ui::settings::themes::{ThemeEntry, slug};

/// Larger files are not themes; they are skipped instead of read.
const MAX_FILE: u64 = 256 * 1024;
/// A zip with more files than this is not a set of themes.
const MAX_ZIP_FILES: usize = 512;
/// How often the folder is looked at for changed files.
const CHECK_EVERY: Duration = Duration::from_secs(1);
/// The theme template's name in its zip; it is not brought in when the zip is dropped.
const TEMPLATE: &str = "template.json";
/// The folder of the template zip that holds the built-in themes as examples.
const EXAMPLES: &str = "examples/";

pub struct ThemeFiles {
    folder: Option<PathBuf>,
    /// When each file was last changed, as last read or written.
    seen: HashMap<String, SystemTime>,
    checked: Option<Instant>,
}

impl ThemeFiles {
    pub fn new(folder: Option<PathBuf>) -> Self {
        Self { folder, seen: HashMap::new(), checked: None }
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
            if let Some(entry) = read(&id, &path, &mut warnings) {
                entries.push(entry);
            }
            self.seen.insert(id, modified);
        }
        (entries, warnings)
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
            if let Some(entry) = read(&id, &path, &mut warnings) {
                entries.push(entry);
            }
            self.seen.insert(id, modified);
        }
        (entries, warnings)
    }

    /// Writes the theme `id` to its file.
    pub fn write(&mut self, id: &str, theme: &Theme) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("there is no folder to keep themes in")?;
        if !is_theme_id(id) {
            return Err(format!("{id} cannot name a theme file"));
        }
        let path = folder.join(format!("{id}.json"));
        let failed = |e: std::io::Error| crate::save_error::message(&format!("the theme {id}"), &e);
        std::fs::create_dir_all(folder).map_err(failed)?;
        std::fs::write(&path, theme.to_json()).map_err(failed)?;
        if let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) {
            self.seen.insert(id.to_string(), modified);
        }
        Ok(())
    }

    /// Renames the file of the theme `from` to `to`, after the theme was renamed. A file that is
    /// not there yet (a theme not saved before) is fine: it is written under its new name next.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("there is no folder to keep themes in")?;
        if !is_theme_id(from) || !is_theme_id(to) {
            return Err(format!("{from} or {to} cannot name a theme file"));
        }
        let (old, new) = (folder.join(format!("{from}.json")), folder.join(format!("{to}.json")));
        if !old.exists() {
            return Ok(());
        }
        if new.exists() && !from.eq_ignore_ascii_case(to) {
            return Err(format!("there is a theme file {to}.json already"));
        }
        std::fs::rename(&old, &new).map_err(|e| format!("could not rename {from}.json: {e}"))?;
        self.seen.remove(from);
        if let Ok(modified) = std::fs::metadata(&new).and_then(|m| m.modified()) {
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
        let mut contents = Vec::new();
        for (id, path, _) in files {
            let text = std::fs::read(&path).map_err(|e| format!("could not read {id}.json: {e}"))?;
            contents.push((format!("{id}.json"), text));
        }
        let name = format!("suhina-themes-{}.zip", date(SystemTime::now()));
        write_zip(&self.exports()?.join(name), &contents)
    }

    /// Writes `exports/suhina-theme-template.zip`: `template.json` (Suhina Dark with every
    /// colour written out), `README.md` on the format, and the built-in themes in `examples/`.
    /// Returns its path.
    pub fn export_template(&self) -> Result<PathBuf, String> {
        let template = Theme { name: "My theme".into(), ..Theme::default() };
        let mut contents = vec![
            ("README.md".to_string(), theme::template_readme().into_bytes()),
            (TEMPLATE.to_string(), template.to_json().into_bytes()),
        ];
        for (id, theme) in theme::built_in_themes() {
            contents.push((format!("{EXAMPLES}{id}.json"), theme.to_json().into_bytes()));
        }
        write_zip(&self.exports()?.join("suhina-theme-template.zip"), &contents)
    }

    /// Brings in a theme file or every theme in a zip (a backup, say; the template's own files
    /// are left out): each is written to the themes folder under an id no theme has yet, unless
    /// the same theme is there already. Returns the ids written and warnings.
    pub fn import(&mut self, path: &Path) -> Result<(Vec<String>, Vec<String>), String> {
        let is_zip = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        let mut found = Vec::new();
        let mut warnings = Vec::new();
        if is_zip {
            for (name, text) in read_zip(path)? {
                let (theme, problems) = Theme::from_json(&text);
                warnings.extend(problems.into_iter().map(|p| format!("{name}: {p}")));
                found.push((stem(&name), theme));
            }
        } else {
            let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            if std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?.len() > MAX_FILE {
                return Err(format!("{name} is larger than a theme can be"));
            }
            let text = std::fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
            let (theme, problems) = Theme::from_json(&text);
            if problems.iter().any(|p| p.contains("not JSON") || p.contains("JSON object")) {
                return Err(format!("{name} is not a theme: {}", problems.join("; ")));
            }
            warnings.extend(problems.into_iter().map(|p| format!("{name}: {p}")));
            found.push((stem(&name), theme));
        }
        let (existing, _) = self.read_all();
        let mut written = Vec::new();
        for (stem, theme) in found {
            if existing.iter().any(|e| e.theme == theme) {
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
            self.write(&id, &theme)?;
            written.push(ThemeEntry { id, theme });
        }
        Ok((written.into_iter().map(|e| e.id).collect(), warnings))
    }

    /// Shows the themes folder in the system's file manager, made first if it is not there.
    pub fn open_folder(&self) -> Result<(), String> {
        let folder = self.folder.as_ref().ok_or("there is no folder to keep themes in")?;
        std::fs::create_dir_all(folder).map_err(|e| format!("could not make {}: {e}", folder.display()))?;
        reveal(folder, false)
    }

    /// Each `.json` file in the folder: its id, path and when it last changed, by name.
    fn list(&self) -> Vec<(String, PathBuf, SystemTime)> {
        let Some(Ok(dir)) = self.folder.as_ref().map(std::fs::read_dir) else { return Vec::new() };
        let mut files: Vec<_> = dir
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    return None;
                }
                let id = path.file_stem()?.to_str()?.to_string();
                let meta = entry.metadata().ok().filter(|m| m.is_file())?;
                Some((id, path, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
            })
            .filter(|(id, _, _)| is_theme_id(id))
            .collect();
        files.sort_by_key(|f| f.0.to_lowercase());
        files
    }
}

/// Shows `path` in the system's file manager: the folder, or the file selected where the system
/// can do that.
pub fn reveal(path: &Path, select: bool) -> Result<(), String> {
    let mut command = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("explorer");
        if select {
            c.arg(format!("/select,{}", path.display()));
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

/// Reads one theme file; problems become warnings naming the file.
fn read(id: &str, path: &Path, warnings: &mut Vec<String>) -> Option<ThemeEntry> {
    let name = path.file_name().map_or_else(|| id.to_string(), |n| n.to_string_lossy().into_owned());
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_FILE => {
            warnings.push(format!("{name}: larger than a theme can be, skipped"));
            return None;
        }
        Ok(_) => {}
        Err(e) => {
            warnings.push(format!("{name}: {e}"));
            return None;
        }
    }
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let (theme, problems) = Theme::from_json(&text);
            warnings.extend(problems.into_iter().map(|p| format!("{name}: {p}")));
            Some(ThemeEntry { id: id.to_string(), theme })
        }
        Err(e) => {
            warnings.push(format!("{name}: {e}"));
            None
        }
    }
}

/// A file name's last part without `.json`.
fn stem(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file.strip_suffix(".json").unwrap_or(file).to_string()
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

/// The theme files in a zip, as names and text: every `.json` but the template's own, each at
/// most [`MAX_FILE`] long.
fn read_zip(path: &Path) -> Result<Vec<(String, String)>, String> {
    let name = path.display();
    let file = std::fs::File::open(path).map_err(|e| format!("could not open {name}: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{name} is not a zip: {e}"))?;
    if archive.len() > MAX_ZIP_FILES {
        return Err(format!("{name} has more files than a set of themes"));
    }
    let mut themes = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("{name}: {e}"))?;
        let inside = entry.name().map_err(|e| format!("{name}: {e}"))?.replace('\\', "/");
        let wanted = entry.is_file()
            && inside.to_ascii_lowercase().ends_with(".json")
            && inside != TEMPLATE
            && !inside.starts_with(EXAMPLES);
        if !wanted {
            continue;
        }
        if entry.size() > MAX_FILE {
            return Err(format!("{inside} in {name} is larger than a theme can be"));
        }
        let mut text = String::new();
        (&mut entry).take(MAX_FILE).read_to_string(&mut text).map_err(|e| format!("{inside} in {name}: {e}"))?;
        themes.push((inside, text));
    }
    if themes.is_empty() {
        return Err(format!("there are no themes in {name}"));
    }
    Ok(themes)
}

/// `YYYY-MM-DD` of `time`, in UTC.
fn date(time: SystemTime) -> String {
    let days = time.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-theme-files-{name}-{}", std::process::id()));
        let _fresh = std::fs::remove_dir_all(&dir);
        dir
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
        assert_eq!(entries.get(1).map(|e| &e.theme), Some(&night()));
        assert_eq!(warnings.len(), 1, "{warnings:?}");

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

        // Into an empty folder: written as they were.
        let mut other = ThemeFiles::new(Some(dir.join("other")));
        assert_eq!(other.import(&backup), Ok((vec!["night".to_string()], Vec::new())));
        assert_eq!(other.read_all().0, vec![ThemeEntry { id: "night".into(), theme: night() }]);
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
        assert!(names.contains(&"README.md".to_string()) && names.contains(&TEMPLATE.to_string()));
        assert!(names.contains(&"examples/nord.json".to_string()));
        let mut template = String::new();
        archive
            .by_name(TEMPLATE)
            .unwrap_or_else(|e| panic!("{e}"))
            .read_to_string(&mut template)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(Theme::from_json(&template).1, Vec::<String>::new());
        // The template's own files are not themes to bring in.
        let mut into = ThemeFiles::new(Some(dir.join("into")));
        assert!(into.import(&zip).is_err());
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
    fn dates_are_written_year_first() {
        let day = |d: u64| date(SystemTime::UNIX_EPOCH + Duration::from_secs(d * 86_400));
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(11_016), "2000-02-29");
        assert_eq!(day(20_738), "2026-10-12");
    }
}
