// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The themes the user can pick from: those that come with Suhina and those read from theme
//! files, and what Settings › Appearance › Themes does with them. The app reads and writes the
//! files; this keeps the list, says what to do with the files, and picks the colours to draw with.

use std::path::PathBuf;

use crate::kit::theme::{self, BUILT_IN, Palette, Theme};
use crate::settings::settings_view::{ThemeRequest, ThemesView};

/// A theme and its id: a built-in id or the name of its folder for the theme's own values, and
/// `<theme>@<variant>` for one of its variants (`theme_vars::split_id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeEntry {
    pub id: String,
    pub theme: Theme,
}

/// What the app is asked to do with the theme files, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    /// Write the theme: for `<theme>@<variant>` the variant's file, and the theme's own files when
    /// they change (`layout.css`, artwork; `theme.css` only when it has none yet).
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
    /// Write the theme `id` as a theme package (`.theme.suhina`).
    ExportPackage { id: String, theme: Box<Theme> },
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
        let own = |e: &&ThemeEntry| !e.id.contains('@');
        ThemesView {
            declared: theme.vars.declared.iter().filter(|p| p.label.is_some()).cloned().collect(),
            values: {
                let mut values = (*theme.vars.base).clone();
                values.extend(theme.vars.values.clone());
                values
            },
            variant: theme.vars.variant_name.clone(),
            kept: self
                .entries
                .iter()
                .filter_map(|e| e.theme.vars.kept.as_ref().map(|why| (e.id.clone(), why.to_string())))
                .collect(),
            variants: self
                .entries
                .iter()
                .filter(|e| e.id.contains('@'))
                .map(|e| (e.id.clone(), e.theme.vars.variant_name.clone().unwrap_or_default()))
                .collect(),
            skin: theme.skin,
            transparent_rgb: theme.transparent_rgb,
            // The user's own themes first, then those that come with Suhina; variants are by their
            // theme's card.
            list: self.entries[self.built_in..]
                .iter()
                .filter(own)
                .map(|e| (e.id.clone(), e.theme.name.clone(), false))
                .chain(self.entries[..self.built_in].iter().map(|e| (e.id.clone(), e.theme.name.clone(), true)))
                .collect(),
            current: shown.to_string(),
            previews: self.entries.iter().map(|entry| (entry.id.clone(), entry.theme.colors)).collect(),
            preview_scene: Default::default(),
            built_in: theme::is_built_in(crate::kit::theme_vars::split_id(shown).0),
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
    ///
    /// A setting edits the variant in use; on a theme's own values it makes a variant, *Custom*,
    /// first, so a theme's sheet is never rewritten (a built-in theme's variant needs no copy of
    /// it). A change of layout or artwork edits the theme itself; a built-in theme, which does not
    /// change, is copied first.
    pub fn request(&mut self, current: &str, request: ThemeRequest) -> Option<String> {
        match request {
            ThemeRequest::Use(id) => self.get(&id).is_some().then_some(id),
            ThemeRequest::ResetEdits { colors, liquid, gradients, transparent_rgb, skin } => {
                let switched = self.request(current, ThemeRequest::EditRgba { colors, transparent_rgb });
                let id = switched.as_deref().unwrap_or(current).to_string();
                let switched = self.request(&id, ThemeRequest::EditLiquid(liquid)).or(switched);
                let id = switched.as_deref().unwrap_or(current).to_string();
                let switched = self.request(&id, ThemeRequest::EditGradients(gradients)).or(switched);
                let id = switched.as_deref().unwrap_or(current).to_string();
                self.request(&id, ThemeRequest::EditSkin(*skin)).or(switched)
            }
            ThemeRequest::EditRgba { colors, transparent_rgb } => self.edit(current, false, true, |theme| {
                theme.colors = colors;
                theme.transparent_rgb = transparent_rgb;
            }),
            ThemeRequest::Edit(colors) => self.edit(current, false, true, |theme| theme.colors = colors),
            ThemeRequest::EditLiquid(options) => self.edit(current, false, true, |theme| theme.liquid = options),
            ThemeRequest::EditGradients(gradients) => {
                self.edit(current, false, true, |theme| theme.gradients = gradients)
            }
            ThemeRequest::EditValue { name, value } => {
                let theme = self.get(current)?;
                let accepted = crate::kit::theme_vars::registered(theme, &name).is_some_and(|p| p.accepts(&value));
                if !accepted {
                    return None;
                }
                self.edit(current, false, true, |theme| {
                    theme.vars.values.insert(name, value);
                    crate::kit::theme_vars::resolve(theme);
                })
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
                let id = switched.as_deref().unwrap_or(current).to_string();
                self.request(&id, ThemeRequest::EditSkin(theme.skin)).or(switched)
            }
            ThemeRequest::EditSkin(skin) => {
                let before = self.get(current)?.clone();
                let structure = structure(&before.skin) != structure(&skin);
                let mut after = before.clone();
                after.skin = skin;
                let settings =
                    crate::kit::theme_vars::core_values(&after) != crate::kit::theme_vars::core_values(&before);
                if !structure && !settings {
                    return None;
                }
                let skin = after.skin;
                self.edit(current, structure, settings, |theme| {
                    let (css, variant) = (theme.skin.css.clone(), theme.skin.variant.clone());
                    theme.skin = skin;
                    theme.skin.css = css;
                    theme.skin.variant = variant;
                })
            }
            ThemeRequest::NewVariant(name) => {
                let name = name.trim().to_string();
                let original = self.get(current)?.clone();
                let base = crate::kit::theme_vars::split_id(&self.real_id(current)).0.to_string();
                let id = self.free_variant(&base, &name);
                let mut theme = original;
                theme.vars.variant = crate::kit::theme_vars::split_id(&id).1.map(str::to_owned);
                theme.vars.variant_name = Some(if name.is_empty() { "Custom".into() } else { name });
                self.entries.push(ThemeEntry { id: id.clone(), theme });
                self.actions.push(Pending::Save(id.clone()));
                Some(id)
            }
            ThemeRequest::Rename(name) => {
                let name = name.trim().to_string();
                let old = self.real_id(current);
                if name.is_empty() || self.get(&old).is_none() {
                    return None;
                }
                let (base, variant) = crate::kit::theme_vars::split_id(&old);
                if variant.is_some() {
                    // A variant keeps its file; only its name changes.
                    let entry = self.entries.iter_mut().find(|e| e.id == old)?;
                    entry.theme.vars.variant_name = Some(name);
                    self.actions.push(Pending::Save(old));
                    return None;
                }
                if theme::is_built_in(base) {
                    return None;
                }
                // The folder keeps its name: it is the theme's id, which the settings and its
                // variants refer to, and another folder may already have the name's id.
                for entry in self.entries.iter_mut().filter(|e| crate::kit::theme_vars::split_id(&e.id).0 == old) {
                    entry.theme.name = name.clone();
                }
                self.actions.push(Pending::Save(old));
                None
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
            ThemeRequest::ExportPackage => {
                let id = crate::kit::theme_vars::split_id(&self.real_id(current)).0.to_owned();
                let theme = self.get(&id)?.clone();
                self.file_action(FileAction::ExportPackage { id, theme: Box::new(theme) })
            }
        }
    }

    /// Applies `change` to the entry an edit of `current` goes to, keeps the variant's values in
    /// step with it and saves it. A change of `structure` (layout, artwork) to a built-in theme
    /// copies the theme; a change of `settings` to a theme's own values makes a variant, *Custom*.
    /// Returns the id to use from now on, when that changes.
    fn edit(
        &mut self,
        current: &str,
        structure: bool,
        settings: bool,
        change: impl FnOnce(&mut Theme),
    ) -> Option<String> {
        let real = self.real_id(current);
        self.get(&real)?;
        let (base, variant) = crate::kit::theme_vars::split_id(&real);
        let (id, switched) = if structure && theme::is_built_in(base) {
            let name = format!("{} (custom)", self.get(current).map_or("Suhina Dark", |t| &t.name));
            (self.copy(current, &name), true)
        } else if variant.is_some() || !settings {
            (real.clone(), false)
        } else {
            let base = base.to_string();
            let id = self.free_variant(&base, "Custom");
            let mut theme = self.get(&real)?.clone();
            theme.vars.variant = crate::kit::theme_vars::split_id(&id).1.map(str::to_owned);
            theme.vars.variant_name = Some("Custom".into());
            self.entries.push(ThemeEntry { id: id.clone(), theme });
            (id, true)
        };
        let entry = self.entries.iter_mut().find(|e| e.id == id)?;
        change(&mut entry.theme);
        // A theme read from a sheet keeps its edited settings as values over the sheet's.
        if !entry.theme.vars.base.is_empty() {
            crate::kit::theme_vars::sync(&mut entry.theme);
        }
        self.actions.push(Pending::Save(id.clone()));
        switched.then_some(id)
    }

    /// `<base>@<id>` for a new variant named `name`, under an id no variant of `base` has.
    fn free_variant(&self, base: &str, name: &str) -> String {
        let stem = slug(name);
        let mut id = format!("{base}@{stem}");
        let mut number = 2;
        while self.get(&id).is_some() {
            id = format!("{base}@{stem}-{number}");
            number += 1;
        }
        id
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
        let mut original = self.get(from).cloned().unwrap_or_default();
        // The copy is a theme of its own: its variant's values become part of its sheet.
        original.vars.variant = None;
        original.vars.variant_name = None;
        original.vars.light = None;
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

/// What of a skin is its structure rather than settings: artwork, the toolbar, placements and
/// styles, and the background image itself.
fn structure(skin: &crate::kit::skin::Skin) -> impl PartialEq + use<> {
    (
        skin.assets.clone(),
        skin.toolbar.clone(),
        skin.toolbar_offsets.clone(),
        skin.controls.clone(),
        skin.styles.clone(),
        skin.background.clone(),
    )
}

/// Ids name files, and file names do not differ by case on every system.
fn same_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
#[path = "themes_tests.rs"]
mod tests;
