// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The themes the user can pick from: those that come with Suhina and those read from theme
//! files, and what Settings › Appearance › Themes does with them. The app reads and writes the
//! files; this keeps the list, says what to do with the files, and picks the colours to draw with.

use std::path::PathBuf;

use crate::kit::theme::{self, BUILT_IN, Palette, Theme};
use crate::settings::settings_view::{ThemeRequest, ThemesView};

/// A theme and its id: a built-in id, or the name of its file without `.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeEntry {
    pub id: String,
    pub theme: Theme,
}

/// What the app is asked to do with the theme files, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    /// Write the theme to `<id>.json`.
    Save { id: String, theme: Box<Theme> },
    /// Rename `<from>.json` to `<to>.json`, before it is saved under its new name.
    Rename { from: String, to: String },
    /// Read every theme file again.
    Reload,
    /// Show the themes folder in the system's file manager.
    OpenFolder,
    /// Write the theme template (a full theme and a README on the format) as a zip.
    ExportTemplate,
    /// Write every theme file into one zip.
    Backup,
    /// Bring in a theme file or a zip of them dropped on the window.
    Import(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Themes {
    /// The built-in themes first, then the files in the order the app gave them.
    entries: Vec<ThemeEntry>,
    built_in: usize,
    folder: Option<String>,
    /// File actions not taken by the app yet.
    actions: Vec<Pending>,
}

/// A file action as it waits: a theme to save is looked up when the app takes it, so a theme
/// changed many times in a frame is written once, as it is then.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Save(String),
    /// Boxed: a whole theme makes the action large.
    Other(Box<FileAction>),
}

impl Default for Themes {
    fn default() -> Self {
        let entries: Vec<ThemeEntry> =
            theme::built_in_themes().into_iter().map(|(id, theme)| ThemeEntry { id: id.to_string(), theme }).collect();
        Self { built_in: entries.len(), entries, folder: None, actions: Vec::new() }
    }
}

impl Themes {
    /// Uses the themes read from `folder`'s files. A file named like a built-in theme is left
    /// out, since the built-in one comes first.
    pub fn set(&mut self, files: Vec<ThemeEntry>, folder: Option<String>) {
        self.entries.truncate(self.built_in);
        for entry in files {
            if !self.entries.iter().any(|e| same_id(&e.id, &entry.id)) {
                self.entries.push(entry);
            }
        }
        self.folder = folder;
    }

    /// One theme file read again, added if it is new. Returns whether anything changed.
    pub fn put(&mut self, entry: ThemeEntry) -> bool {
        if theme::is_built_in(&entry.id) {
            return false;
        }
        match self.entries.iter_mut().find(|e| same_id(&e.id, &entry.id)) {
            Some(old) if *old == entry => false,
            Some(old) => {
                *old = entry;
                true
            }
            None => {
                self.entries.push(entry);
                true
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&Theme> {
        self.entries.iter().find(|e| same_id(&e.id, id)).map(|e| &e.theme)
    }

    /// The colours of the theme `id`, or Suhina Dark's when there is no such theme.
    pub fn palette(&self, id: &str) -> Palette {
        self.get(id).map_or(Palette::DARK, |t| t.colors)
    }

    /// What Settings shows, with `current` in use.
    pub fn view(&self, current: &str) -> ThemesView {
        let shown = self.entries.iter().find(|e| same_id(&e.id, current)).map_or(BUILT_IN, |e| e.id.as_str());
        let theme = self.get(shown).cloned().unwrap_or_default();
        ThemesView {
            skin: theme.skin,
            transparent_rgb: theme.transparent_rgb,
            // The user's own themes first, then those that come with Suhina.
            list: self.entries[self.built_in..]
                .iter()
                .map(|e| (e.id.clone(), e.theme.name.clone(), false))
                .chain(self.entries[..self.built_in].iter().map(|e| (e.id.clone(), e.theme.name.clone(), true)))
                .collect(),
            current: shown.to_string(),
            previews: self.entries.iter().map(|entry| (entry.id.clone(), entry.theme.colors)).collect(),
            preview_scene: Default::default(),
            built_in: theme::is_built_in(shown),
            name: theme.name,
            colors: theme.colors,
            liquid: theme.liquid,
            gradients: theme.gradients,
            global_liquid: Default::default(),
            folder: self.folder.clone(),
        }
    }

    /// Does what the page asked while `current` is in use. Returns the id of the theme to use
    /// from now on, when that changes.
    pub fn request(&mut self, current: &str, request: ThemeRequest) -> Option<String> {
        let editable = self.get(current).is_some() && !theme::is_built_in(current);
        match request {
            ThemeRequest::Use(id) => self.get(&id).is_some().then_some(id),
            ThemeRequest::ResetEdits { colors, liquid, gradients, transparent_rgb, skin } => {
                let switched = self.request(current, ThemeRequest::EditRgba { colors, transparent_rgb });
                let id = switched.as_deref().unwrap_or(current);
                self.request(id, ThemeRequest::EditLiquid(liquid));
                self.request(id, ThemeRequest::EditGradients(gradients));
                self.request(id, ThemeRequest::EditSkin(*skin));
                switched
            }
            ThemeRequest::EditRgba { colors, transparent_rgb } => {
                let switched = self.request(current, ThemeRequest::Edit(colors));
                let id = switched.as_deref().unwrap_or(current);
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.theme.transparent_rgb = transparent_rgb;
                }
                switched
            }
            ThemeRequest::Edit(colors) => {
                let (id, copied) = if editable {
                    (self.real_id(current), false)
                } else {
                    let name = format!("{} (custom)", self.get(current).map_or("Suhina Dark", |t| &t.name));
                    (self.copy(current, &name), true)
                };
                if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                    entry.theme.colors = colors;
                }
                self.actions.push(Pending::Save(id.clone()));
                copied.then_some(id)
            }
            ThemeRequest::EditLiquid(options) => {
                let (id, copied) = if editable {
                    (self.real_id(current), false)
                } else {
                    let name = format!("{} (custom)", self.get(current).map_or("Suhina Dark", |t| &t.name));
                    (self.copy(current, &name), true)
                };
                if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                    entry.theme.liquid = options;
                }
                self.actions.push(Pending::Save(id.clone()));
                copied.then_some(id)
            }
            ThemeRequest::EditCss(css) => {
                let mut theme = self.get(current).cloned().unwrap_or_default();
                if !crate::kit::theme_css::apply(&mut theme, &css).is_empty() {
                    return None;
                }
                let switched = self.request(
                    current,
                    ThemeRequest::EditRgba { colors: theme.colors, transparent_rgb: theme.transparent_rgb },
                );
                let id = switched.as_deref().unwrap_or(current);
                self.request(id, ThemeRequest::EditSkin(theme.skin));
                switched
            }
            ThemeRequest::EditSkin(skin) => {
                let (id, copied) = if editable {
                    (self.real_id(current), false)
                } else {
                    let name = format!("{} (custom)", self.get(current).map_or("Suhina Dark", |t| &t.name));
                    (self.copy(current, &name), true)
                };
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.theme.skin = skin;
                }
                self.actions.push(Pending::Save(id.clone()));
                copied.then_some(id)
            }
            ThemeRequest::EditGradients(gradients) => {
                let (id, copied) = if editable {
                    (self.real_id(current), false)
                } else {
                    let name = format!("{} (custom)", self.get(current).map_or("Suhina Dark", |t| &t.name));
                    (self.copy(current, &name), true)
                };
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.theme.gradients = gradients;
                }
                self.actions.push(Pending::Save(id.clone()));
                copied.then_some(id)
            }
            ThemeRequest::Rename(name) => {
                let name = name.trim().to_string();
                if !editable || name.is_empty() {
                    return None;
                }
                let old = self.real_id(current);
                let new = if same_id(&slug(&name), &old) { old.clone() } else { self.free_id(&name) };
                let entry = self.entries.iter_mut().find(|e| e.id == old)?;
                entry.theme.name = name;
                entry.id = new.clone();
                if new != old {
                    self.actions.push(Pending::Other(Box::new(FileAction::Rename { from: old, to: new.clone() })));
                }
                self.actions.push(Pending::Save(new.clone()));
                (!same_id(&new, current)).then_some(new)
            }
            ThemeRequest::Duplicate => {
                let name = format!("{} copy", self.get(current).map_or("Suhina Dark", |t| t.name.as_str()));
                let id = self.copy(current, &name);
                self.actions.push(Pending::Save(id.clone()));
                Some(id)
            }
            ThemeRequest::RestoreBuiltIn { id, save_copy } => {
                if !theme::is_built_in(&id) || self.get(&id).is_none() {
                    return None;
                }
                if save_copy {
                    let name = format!("{} copy", self.get(current).map_or("Theme", |t| t.name.as_str()));
                    let copy = self.copy(current, &name);
                    self.actions.push(Pending::Save(copy));
                }
                Some(id)
            }
            ThemeRequest::Reload => self.file_action(FileAction::Reload),
            ThemeRequest::OpenFolder => self.file_action(FileAction::OpenFolder),
            ThemeRequest::ExportTemplate => self.file_action(FileAction::ExportTemplate),
            ThemeRequest::Backup => self.file_action(FileAction::Backup),
        }
    }

    /// A theme file or zip dropped on the window, for the app to bring in.
    pub fn import(&mut self, path: PathBuf) {
        self.actions.push(Pending::Other(Box::new(FileAction::Import(path))));
    }

    /// What the app should do with the theme files, in order, once.
    pub fn take_actions(&mut self) -> Vec<FileAction> {
        let pending = std::mem::take(&mut self.actions);
        let mut actions: Vec<FileAction> = Vec::new();
        for item in pending {
            let action = match item {
                Pending::Save(id) => match self.get(&id) {
                    Some(theme) => FileAction::Save { id, theme: Box::new(theme.clone()) },
                    None => continue,
                },
                Pending::Other(action) => *action,
            };
            if !actions.contains(&action) {
                actions.push(action);
            }
        }
        actions
    }

    fn file_action(&mut self, action: FileAction) -> Option<String> {
        self.actions.push(Pending::Other(Box::new(action)));
        None
    }

    /// The id as the list spells it.
    fn real_id(&self, id: &str) -> String {
        self.entries.iter().find(|e| same_id(&e.id, id)).map_or_else(|| id.to_string(), |e| e.id.clone())
    }

    /// Adds a copy of the theme `from` named `name`, under a new id. Returns the id.
    fn copy(&mut self, from: &str, name: &str) -> String {
        let original = self.get(from).cloned().unwrap_or_default();
        let id = self.free_id(name);
        self.entries.push(ThemeEntry { id: id.clone(), theme: Theme { name: name.to_string(), ..original } });
        id
    }

    /// An id made from `name` that no theme has yet.
    pub fn free_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut number = 2;
        while self.get(&id).is_some() || theme::is_built_in(&id) {
            id = format!("{base}-{number}");
            number += 1;
        }
        id
    }
}

/// A theme's name as an id: lower case, words joined by `-`; `theme` when nothing is left.
pub fn slug(name: &str) -> String {
    let mut id = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            id.extend(c.to_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_end_matches('-');
    if id.is_empty() { "theme".into() } else { id.to_string() }
}

/// Ids name files, and file names do not differ by case on every system.
fn same_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_edits_copy_builtins_save_duplicate_and_reset_with_the_theme() {
        use crate::kit::gradient::{Gradient, Gradients};
        let mut themes = Themes::default();
        let gradients =
            Gradients { tracks: Some(Gradient::default()), clips: Some(Gradient::default()), ..Default::default() };
        let id = themes.request("nord", ThemeRequest::EditGradients(gradients.clone())).unwrap();
        assert_eq!(themes.get("nord").unwrap().gradients, Gradients::default());
        let edited = themes.get(&id).unwrap().clone();
        assert_eq!(edited.gradients, gradients);
        assert_eq!(Theme::from_json(&edited.to_json()), (edited.clone(), Vec::new()));
        let copy = themes.request(&id, ThemeRequest::Duplicate).unwrap();
        assert_eq!(themes.get(&copy).unwrap().gradients, gradients);
        themes.request(
            &id,
            ThemeRequest::ResetEdits {
                skin: Default::default(),
                transparent_rgb: Default::default(),
                colors: edited.colors,
                liquid: edited.liquid,
                gradients: Gradients::default(),
            },
        );
        assert_eq!(themes.get(&id).unwrap().gradients, Gradients::default());
        assert_eq!(themes.get(&copy).unwrap().gradients, gradients);
    }

    fn night() -> ThemeEntry {
        let mut colors = Palette::DARK;
        colors.accent = egui::Color32::from_rgb(1, 2, 3);
        ThemeEntry {
            id: "night".into(),
            theme: Theme {
                name: "Night".into(),
                colors,
                liquid: None,
                gradients: Default::default(),
                transparent_rgb: Default::default(),
                skin: Default::default(),
            },
        }
    }

    fn saved(themes: &mut Themes) -> Vec<(String, String)> {
        themes
            .take_actions()
            .into_iter()
            .filter_map(|a| match a {
                FileAction::Save { id, theme } => Some((id, theme.name)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn restoring_a_built_in_can_save_a_copy_without_overwriting_either_theme() {
        let mut themes = Themes::default();
        themes.set(vec![night()], None);
        let original = themes.get("night").cloned();
        let built_in = themes.palette(BUILT_IN);
        assert_eq!(
            themes.request("night", ThemeRequest::RestoreBuiltIn { id: BUILT_IN.into(), save_copy: true }),
            Some(BUILT_IN.into())
        );
        assert_eq!(themes.get("night"), original.as_ref());
        assert_eq!(themes.palette(BUILT_IN), built_in);
        let actions = themes.take_actions();
        assert!(
            matches!(actions.as_slice(), [FileAction::Save { theme, .. }] if theme.colors == original.as_ref().unwrap().colors)
        );
        assert_eq!(
            themes.request("night", ThemeRequest::RestoreBuiltIn { id: "nord".into(), save_copy: false }),
            Some("nord".into())
        );
        assert!(themes.take_actions().is_empty());
        assert_eq!(themes.request("night", ThemeRequest::RestoreBuiltIn { id: "night".into(), save_copy: true }), None);
        assert!(themes.take_actions().is_empty());
    }

    #[test]
    fn the_built_in_themes_come_first_and_stay() {
        let mut themes = Themes::default();
        let fake = ThemeEntry { id: "Suhina-Dark".into(), theme: night().theme };
        themes.set(vec![night(), fake.clone(), night()], Some("themes".into()));
        let view = themes.view("night");
        assert_eq!(view.list.first(), Some(&("night".into(), "Night".into(), false)));
        assert_eq!(view.list.get(1), Some(&(BUILT_IN.into(), "Suhina Dark".into(), true)));
        assert!(view.list.iter().any(|(id, _, built_in)| id == "nord" && *built_in));
        assert_eq!(view.list.len(), theme::built_in_themes().len() + 1);
        assert_eq!(view.colors, night().theme.colors);
        assert_eq!(view.previews.len(), view.list.len());
        for (id, _, _) in &view.list {
            assert_eq!(
                view.previews.iter().find(|(key, _)| key == id).map(|(_, colors)| *colors),
                Some(themes.palette(id))
            );
        }
        assert!(!view.built_in);
        assert!(!themes.put(fake));
        assert_eq!(themes.palette(BUILT_IN), Palette::DARK);
        assert!(themes.view("nord").built_in);
    }

    #[test]
    fn a_missing_theme_draws_with_suhina_dark() {
        let themes = Themes::default();
        assert_eq!(themes.palette("gone"), Palette::DARK);
        assert_eq!(themes.view("gone").current, BUILT_IN);
    }

    #[test]
    fn editing_a_built_in_theme_makes_a_copy() {
        let mut themes = Themes::default();
        let mut colors = themes.palette("nord");
        colors.border = egui::Color32::from_rgb(9, 9, 9);
        assert_eq!(themes.request("nord", ThemeRequest::Edit(colors)), Some("nord-custom".into()));
        assert_ne!(themes.palette("nord"), colors);
        assert_eq!(themes.palette("nord-custom"), colors);
        // A second edit changes the copy, not another one.
        colors.border = egui::Color32::from_rgb(8, 8, 8);
        assert_eq!(themes.request("nord-custom", ThemeRequest::Edit(colors)), None);
        assert_eq!(saved(&mut themes), [("nord-custom".into(), "Nord (custom)".into())]);
        assert_eq!(themes.palette("nord-custom"), colors);
        assert!(themes.take_actions().is_empty());
    }

    #[test]
    fn duplicates_get_their_own_ids() {
        let mut themes = Themes::default();
        themes.set(vec![night()], None);
        assert_eq!(themes.request("night", ThemeRequest::Duplicate), Some("night-copy".into()));
        assert_eq!(themes.request("night", ThemeRequest::Duplicate), Some("night-copy-2".into()));
        assert_eq!(themes.get("night-copy-2").map(|t| t.colors), Some(night().theme.colors));
        assert_eq!(saved(&mut themes).len(), 2);
    }

    #[test]
    fn renaming_a_theme_renames_its_file() {
        let mut themes = Themes::default();
        themes.set(vec![night()], None);
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Rename("Mine".into())), None);
        assert!(themes.take_actions().is_empty());
        assert_eq!(themes.request("night", ThemeRequest::Rename(" Deep Sea ".into())), Some("deep-sea".into()));
        assert_eq!(
            themes.take_actions(),
            [
                FileAction::Rename { from: "night".into(), to: "deep-sea".into() },
                FileAction::Save {
                    id: "deep-sea".into(),
                    theme: Box::new(Theme {
                        name: "Deep Sea".into(),
                        colors: night().theme.colors,
                        liquid: None,
                        gradients: Default::default(),
                        transparent_rgb: Default::default(),
                        skin: Default::default(),
                    })
                },
            ]
        );
        // A name that makes the same id keeps the file.
        assert_eq!(themes.request("deep-sea", ThemeRequest::Rename("Deep  sea!".into())), None);
        assert_eq!(saved(&mut themes), [("deep-sea".into(), "Deep  sea!".into())]);
        // A built-in theme's id is not taken.
        assert_eq!(themes.request("deep-sea", ThemeRequest::Rename("Nord".into())), Some("nord-2".into()));
    }

    #[test]
    fn using_and_file_actions() {
        let mut themes = Themes::default();
        themes.set(vec![night()], None);
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("night".into())), Some("night".into()));
        assert_eq!(themes.request(BUILT_IN, ThemeRequest::Use("nothing".into())), None);
        for request in [ThemeRequest::Reload, ThemeRequest::Backup, ThemeRequest::Backup, ThemeRequest::ExportTemplate]
        {
            assert_eq!(themes.request(BUILT_IN, request), None);
        }
        themes.import("night.json".into());
        assert_eq!(
            themes.take_actions(),
            [
                FileAction::Reload,
                FileAction::Backup,
                FileAction::ExportTemplate,
                FileAction::Import("night.json".into())
            ]
        );
    }

    #[test]
    fn ids_are_made_from_names() {
        let themes = Themes::default();
        assert_eq!(themes.free_id("  Night Blue!! "), "night-blue");
        assert_eq!(themes.free_id("../.."), "theme");
        assert_eq!(themes.free_id("Suhina Dark"), "suhina-dark-2");
        assert_eq!(slug("Café Thème"), "café-thème");
    }
}
