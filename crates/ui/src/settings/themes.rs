// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The themes the user can pick from: those that come with Suhina and those read from theme
//! files, and what Settings › Appearance › Themes does with them. The app reads and writes the
//! files; this keeps the list, says what to do with the files, and picks the colours to draw with.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::kit::theme::{self, BUILT_IN, Theme};
use crate::settings::settings_view::{ThemeInfo, ThemeRequest, ThemesView};

/// What the status bar says when something of a built-in theme was asked to change.
const LOCKED: &str = "Built-in themes and their variants can't be changed; make a new theme or variant from them";

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
    /// they change (`theme.json`, artwork, and the editors' rules in `theme.css`).
    Save { id: String, theme: Box<Theme> },
    /// Rename `<from>.json` to `<to>.json`, before it is saved under its new name.
    Rename { from: String, to: String },
    /// Read every theme file again.
    Reload,
    /// Show the themes folder in the system's file manager.
    OpenFolder,
    /// Write every theme file into one zip.
    Backup,
    /// Write the theme `id` as a theme package (`.theme.suhina`).
    ExportPackage { id: String, theme: Box<Theme> },
    /// Bring in a theme file or a zip of them dropped on the window. A package whose theme is
    /// here already is not written: the app asks whether to replace it or bring in a copy.
    Import(PathBuf),
    /// Bring in a theme package whose theme is here already: in place of it (`replace`), or as
    /// a copy under a new id.
    ImportAgain { path: PathBuf, replace: bool },
    /// Move the theme `id`'s folder, or for `<theme>@<variant>` the variant's file, to the
    /// system's Trash. The folder is read again whole afterwards, so the theme leaves the list
    /// only once it is gone.
    Trash(String),
    /// Show the theme `id`'s folder in the system's file manager.
    Reveal(String),
}

#[derive(Debug, Clone)]
pub struct Themes {
    /// The built-in themes first, then the files in the order the app gave them.
    entries: Vec<ThemeEntry>,
    built_in: usize,
    folder: Option<String>,
    /// Something to say once in the status bar about what a request did.
    notice: Option<String>,
    /// File actions not taken by the app yet.
    actions: Vec<Pending>,
    /// Folders in the themes folder that no theme here has (not themes, or not read): their names
    /// are not free for a new theme.
    reserved: Vec<String>,
    /// The variant being edited, drawn with in place of its entry until it is saved or dropped.
    draft: Option<Draft>,
    /// Variants a built-in theme ships whose place a file of the user's took (an edit made by an
    /// earlier version): the user's, which *Revert to built-in* moves to the Trash.
    overridden: Vec<String>,
    /// Each of the user's themes whose sheet has notices, with them (line, column, message).
    notices: Vec<(String, Vec<String>)>,
    /// Themes and variants this asked to move to the Trash: the ones whose going the app answers
    /// by choosing another, unlike a theme missing from one reading of the folder.
    trashed: Vec<String>,
}

/// The variant editor's unsaved edits of the variant `id`: the values set and the name typed, and
/// the theme they make, which [`Themes::get`] gives for `id` in place of its entry. The entries
/// stay as the files say, so a file read again, a save from elsewhere or the toolbar editor needs
/// no special case: the draft is made again over the entry.
#[derive(Debug, Clone)]
struct Draft {
    id: String,
    name: Option<String>,
    /// The colour scheme chosen in the editor, when one was.
    light: Option<bool>,
    values: BTreeMap<String, String>,
    theme: Theme,
    /// A new variant started from `id` (the + box): nothing is written until it is saved, as a
    /// variant of its own.
    new: bool,
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
        let entries: Vec<ThemeEntry> = theme::built_in_themes()
            .into_iter()
            .map(|(id, theme)| (id.to_string(), theme))
            .chain(theme::built_in_variants())
            .map(|(id, theme)| ThemeEntry { id, theme })
            .collect();
        Self {
            built_in: entries.len(),
            entries,
            folder: None,
            actions: Vec::new(),
            notice: None,
            reserved: Vec::new(),
            draft: None,
            overridden: Vec::new(),
            notices: Vec::new(),
            trashed: Vec::new(),
        }
    }
}

impl Themes {
    /// Uses the themes read from `folder`'s files. A file named like a built-in theme is left
    /// out, since the built-in one comes first; a variant's file takes the place of the built-in
    /// variant of its id, which is where editing that variant saves it.
    pub fn set(&mut self, files: Vec<ThemeEntry>, folder: Option<String>) {
        self.entries = Self::default().entries;
        self.overridden.clear();
        for entry in files {
            match self.entries.iter_mut().find(|e| same_id(&e.id, &entry.id)) {
                Some(old) if entry.id.contains('@') => {
                    self.overridden.push(old.id.clone());
                    *old = entry;
                }
                Some(_) => {}
                None => self.entries.push(entry),
            }
        }
        self.folder = folder;
        self.changed();
    }

    /// After the entries changed: the draft over them and the sheets' notices.
    fn changed(&mut self) {
        self.rebuild_draft();
        self.notices = self.entries[self.built_in..]
            .iter()
            .filter(|e| !e.id.contains('@'))
            .filter_map(|e| {
                let notices = crate::kit::native_css::inspect(e.theme.skin.css.as_deref()?).unwrap_or_default();
                let lines: Vec<String> = notices
                    .into_iter()
                    .map(|n| format!("Line {}, column {}: {}", n.line, n.column, n.message))
                    .collect();
                (!lines.is_empty()).then(|| (e.id.clone(), lines))
            })
            .collect();
    }

    /// The names of the folders in the themes folder; a new theme takes none of them.
    pub fn reserve(&mut self, names: Vec<String>) {
        self.reserved = names;
    }

    /// One theme file read again, added if it is new. Returns whether anything changed.
    pub fn put(&mut self, entry: ThemeEntry) -> bool {
        if theme::is_built_in(&entry.id) {
            return false;
        }
        let shipped = self.entries[..self.built_in].iter().find(|e| same_id(&e.id, &entry.id)).map(|e| e.id.clone());
        let changed = match self.entries.iter_mut().find(|e| same_id(&e.id, &entry.id)) {
            Some(old) if *old == entry => false,
            Some(old) => {
                *old = entry;
                // The user's copy of a variant a built-in theme ships, as `set` marks it.
                if let Some(id) = shipped.filter(|id| !self.overridden.contains(id)) {
                    self.overridden.push(id);
                }
                true
            }
            None => {
                self.entries.push(entry);
                true
            }
        };
        if changed {
            self.changed();
        }
        changed
    }

    /// The theme `id`, with the variant editor's unsaved edits when it is being edited.
    pub fn get(&self, id: &str) -> Option<&Theme> {
        if let Some(draft) = self.draft_for(id) {
            return Some(&draft.theme);
        }
        self.entries.iter().find(|e| same_id(&e.id, id)).map(|e| &e.theme)
    }

    /// What Settings shows, with `current` in use.
    pub fn view(&self, current: &str) -> ThemesView {
        let shown = self.entries.iter().find(|e| same_id(&e.id, current)).map_or(BUILT_IN, |e| e.id.as_str());
        let theme = self.get(shown).cloned().unwrap_or_default();
        let own = |e: &&ThemeEntry| !e.id.contains('@');
        let (base, variant) = crate::kit::theme_vars::split_id(shown);
        let saved_name = self.saved_name(shown);
        ThemesView {
            declared: theme.vars.declared.iter().filter(|p| p.label.is_some()).cloned().collect(),
            values: {
                let mut values = (*theme.vars.base).clone();
                values.extend(theme.vars.values.clone());
                values
            },
            variant: variant.and(theme.vars.variant_name.clone()),
            variant_name: self.draft_for(shown).and_then(|d| d.name.clone()).unwrap_or(saved_name),
            light: theme.light(),
            edited: self.edited(shown),
            saveable: self.saveable(shown),
            new_variant: self.draft_for(shown).is_some_and(|d| d.new),
            kept: self
                .entries
                .iter()
                .filter_map(|e| e.theme.vars.kept.as_ref().map(|why| (e.id.clone(), why.to_string())))
                .collect(),
            variants: self
                .entries
                .iter()
                .filter(|e| e.id.contains('@'))
                .map(|e| (e.id.clone(), self.get(&e.id).and_then(|t| t.vars.variant_name.clone()).unwrap_or_default()))
                .collect(),
            locked: self.entries.iter().filter(|e| self.shipped(&e.id)).map(|e| e.id.clone()).collect(),
            overridden: self.overridden.clone(),
            // The user's own themes first, then those that come with Suhina; variants are by their
            // theme's card.
            list: self.entries[self.built_in..]
                .iter()
                .filter(own)
                .map(|e| (e.id.clone(), e.theme.name.clone(), false))
                .chain(
                    self.entries[..self.built_in]
                        .iter()
                        .filter(own)
                        .map(|e| (e.id.clone(), e.theme.name.clone(), true)),
                )
                .collect(),
            current: shown.to_string(),
            swatches: self
                .entries
                .iter()
                .filter_map(|entry| Some((entry.id.clone(), self.get(&entry.id)?.swatch?)))
                .collect(),
            pictures: self
                .entries
                .iter()
                .filter_map(|entry| Some((entry.id.clone(), self.get(&entry.id)?.picture.clone()?)))
                .collect(),
            info: self
                .entries
                .iter()
                .filter(own)
                .map(|entry| {
                    let meta = &entry.theme.meta;
                    let info = ThemeInfo {
                        name: entry.theme.name.clone(),
                        author: meta.author.clone().unwrap_or_default(),
                        description: meta.description.clone().unwrap_or_default(),
                        default_variant: meta.default_variant().to_owned(),
                    };
                    (entry.id.clone(), info)
                })
                .collect(),
            notices: self.notices.clone(),
            built_in: theme::is_built_in(base),
            name: theme.name,
            colors: theme.colors,
            liquid: theme.liquid,
            folder: self.folder.clone(),
        }
    }

    /// Does what the page asked while `current` is in use. Returns the id of the theme to use
    /// from now on, when that changes.
    ///
    /// A setting changed in the variant editor changes a draft of the variant in use, which the
    /// app draws with at once and which is written only by `Save` or `SaveAsNew`. A change of
    /// layout or artwork (the toolbar editor) edits the theme itself; a built-in theme, which does
    /// not change, is copied first.
    pub fn request(&mut self, current: &str, request: ThemeRequest) -> Option<String> {
        match request {
            ThemeRequest::Use(id) => self.get(&id).is_some().then_some(id),
            ThemeRequest::EditValue { name, value } => {
                let accepted = self
                    .get(current)
                    .and_then(|theme| crate::kit::theme_vars::registered(theme, &name))
                    .is_some_and(|p| p.accepts(&value));
                if accepted {
                    self.open_draft(current)?;
                    let draft = self.draft.as_mut()?;
                    draft.values.insert(name, value);
                    self.rebuild_draft();
                }
                None
            }
            ThemeRequest::EditName(name) => {
                self.open_draft(current)?;
                let draft = self.draft.as_mut()?;
                draft.name = Some(name);
                self.rebuild_draft();
                None
            }
            ThemeRequest::EditScheme(light) => {
                self.open_draft(current)?;
                let draft = self.draft.as_mut()?;
                draft.light = Some(light);
                self.rebuild_draft();
                None
            }
            ThemeRequest::Save => {
                let real = self.real_id(current);
                if !self.saveable(&real) {
                    return None;
                }
                let draft = self.draft.take().filter(|d| d.id == real)?;
                let mut theme = draft.theme;
                if let Some(name) = draft.name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) {
                    theme.vars.variant_name = Some(name);
                }
                let entry = self.entries.iter_mut().find(|e| e.id == real)?;
                entry.theme = theme;
                self.actions.push(Pending::Save(real));
                None
            }
            ThemeRequest::SaveAsNew => {
                let real = self.real_id(current);
                let mut theme = self.get(&real)?.clone();
                let saved = self.saved_name(&real);
                let typed = self.draft_for(&real).and_then(|d| d.name.clone()).map(|n| n.trim().to_owned());
                let new = self.draft_for(&real).is_some_and(|d| d.new);
                let name = match typed.filter(|n| !n.is_empty()) {
                    Some(name) if new || name != saved => name,
                    _ if new => "New variant".to_owned(),
                    _ => format!("{saved} copy"),
                };
                self.draft = None;
                let base = crate::kit::theme_vars::split_id(&real).0.to_string();
                let id = self.free_variant(&base, &name);
                theme.vars.variant = crate::kit::theme_vars::split_id(&id).1.map(str::to_owned);
                theme.vars.variant_name = Some(name);
                self.entries.push(ThemeEntry { id: id.clone(), theme });
                self.actions.push(Pending::Save(id.clone()));
                Some(id)
            }
            ThemeRequest::Discard => {
                self.draft = None;
                None
            }
            ThemeRequest::NewVariant(from) => {
                // A draft of the variant it starts from, saved as a variant of its own only on
                // Save; Cancel leaves nothing behind.
                let from = self.real_id(&from);
                let theme = self.entries.iter().find(|e| e.id == from)?.theme.clone();
                let base = crate::kit::theme_vars::split_id(&from).0.to_string();
                let id = self.free_variant(&base, "New variant");
                // Numbered as its id would be, so two new variants are told apart by name too.
                let number = id.rsplit_once('-').and_then(|(_, n)| n.parse::<u32>().ok());
                let name = number.map_or_else(|| "New variant".to_owned(), |n| format!("New variant {n}"));
                self.draft = Some(Draft {
                    id: from.clone(),
                    name: Some(name),
                    light: None,
                    values: BTreeMap::new(),
                    theme,
                    new: true,
                });
                self.rebuild_draft();
                (!same_id(&from, current)).then_some(from)
            }
            ThemeRequest::DuplicateVariant(from) => {
                let from = self.real_id(&from);
                let mut theme = self.get(&from)?.clone();
                let name = format!("{} copy", self.saved_name(&from));
                let base = crate::kit::theme_vars::split_id(&from).0.to_string();
                let id = self.free_variant(&base, &name);
                theme.vars.variant = crate::kit::theme_vars::split_id(&id).1.map(str::to_owned);
                theme.vars.variant_name = Some(name);
                self.entries.push(ThemeEntry { id: id.clone(), theme });
                self.actions.push(Pending::Save(id));
                None
            }
            ThemeRequest::Delete(id) => {
                let real = self.real_id(&id);
                // Nothing to delete for an id the list does not have.
                self.get(&real)?;
                let (base, variant) = crate::kit::theme_vars::split_id(&real);
                if variant.is_none() && theme::is_built_in(base) || self.shipped(&real) {
                    self.notice = Some(LOCKED.into());
                    return None;
                }
                // The entry stays until the file is gone: the app reads the folder again then, and
                // what was in use gives way only once the move succeeded.
                let target = if variant.is_some() { real.clone() } else { base.to_owned() };
                if !self.trashed.contains(&target) {
                    self.trashed.push(target.clone());
                }
                self.file_action(FileAction::Trash(target))
            }
            ThemeRequest::NewTheme(new) => {
                let typed = new.info.name.trim();
                let (id, name) = if typed.is_empty() {
                    // Numbered as its id is, so two new themes are told apart by name too.
                    (1..=1024)
                        .map(|n| if n == 1 { "New theme".to_owned() } else { format!("New theme {n}") })
                        .map(|name| (self.free_id(&name), name))
                        .find(|(id, name)| {
                            *id == slug(name) && !self.entries.iter().any(|e| e.theme.name.eq_ignore_ascii_case(name))
                        })
                        .unwrap_or_else(|| (self.free_id("New theme"), "New theme".to_owned()))
                } else {
                    (self.free_id(typed), typed.to_owned())
                };
                let mut theme = match new.from.as_deref().map(|from| self.real_id(from)) {
                    Some(from) if self.get(&from).is_some() => {
                        let variant = crate::kit::theme_vars::split_id(&from).1.is_some();
                        let label = self.saved_name(&from);
                        let mut theme = self.copy_of(&from, false);
                        // A theme made from a variant has that variant's values as its own.
                        if variant {
                            theme.meta.default_variant = Some(label);
                        }
                        theme.meta.light = theme.light();
                        theme
                    }
                    _ => Theme::new_blank(&name),
                };
                theme.name.clone_from(&name);
                theme.meta.name.clone_from(&name);
                let text = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
                theme.meta.author = text(&new.info.author);
                theme.meta.description = text(&new.info.description);
                if let Some(default) = text(&new.info.default_variant) {
                    theme.meta.default_variant = Some(default);
                }
                if new.artwork {
                    theme.skin.assets.extend(crate::kit::skin::missing_defaults(&theme.skin.assets));
                }
                self.entries.push(ThemeEntry { id: id.clone(), theme });
                self.actions.push(Pending::Save(id.clone()));
                Some(id)
            }
            ThemeRequest::EditInfo { id, info } => {
                let base = crate::kit::theme_vars::split_id(&self.real_id(&id)).0.to_owned();
                let name = info.name.trim().to_owned();
                if theme::is_built_in(&base) || self.get(&base).is_none() || name.is_empty() {
                    return None;
                }
                let text = |s: &str| Some(s.trim().to_owned()).filter(|s| !s.is_empty());
                // The folder keeps its name: it is the theme's id, which the settings and its
                // variants refer to, and another folder may already have the name's id.
                self.each_of(&base, |theme| {
                    theme.name.clone_from(&name);
                    theme.meta.name.clone_from(&name);
                    theme.meta.author = text(&info.author);
                    theme.meta.description = text(&info.description);
                    // The name shown when none is set ("Default") is not written as one.
                    let typed = text(&info.default_variant);
                    if theme.meta.default_variant.is_some() || typed.as_deref() != Some(theme.meta.default_variant()) {
                        theme.meta.default_variant = typed;
                    }
                });
                self.actions.push(Pending::Save(base));
                None
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
            ThemeRequest::AddDefaultArtwork(id) => {
                let base = crate::kit::theme_vars::split_id(&self.real_id(&id)).0.to_owned();
                if theme::is_built_in(&base) {
                    self.notice = Some(LOCKED.into());
                    return None;
                }
                let missing = self.get(&base).map(|theme| crate::kit::skin::missing_defaults(&theme.skin.assets))?;
                if missing.is_empty() {
                    self.notice = Some("This theme already has all of the default artwork".into());
                    return None;
                }
                let count = missing.len();
                // Artwork is the theme's, not a variant's: the theme and each of its variants get
                // it, and the theme is saved, so a later save of the theme or of any variant (each
                // writes theme.css) keeps the rules (#1019 review).
                self.each_of(&base, |theme| {
                    let missing = crate::kit::skin::missing_defaults(&theme.skin.assets);
                    theme.skin.assets.extend(missing);
                });
                self.actions.push(Pending::Save(base));
                let images = if count == 1 { "image" } else { "images" };
                self.notice = Some(format!("Added {count} default {images} to the theme's assets folder"));
                None
            }
            // The app shows the system's file dialog for it.
            ThemeRequest::Import => None,
            ThemeRequest::OpenFolder => self.file_action(FileAction::OpenFolder),
            ThemeRequest::Reveal(id) => {
                let base = crate::kit::theme_vars::split_id(&self.real_id(&id)).0.to_owned();
                self.file_action(FileAction::Reveal(base))
            }
            ThemeRequest::Backup => self.file_action(FileAction::Backup),
            ThemeRequest::ExportPackage(id) => {
                let id = crate::kit::theme_vars::split_id(&self.real_id(&id)).0.to_owned();
                let theme = self.get(&id)?.clone();
                self.file_action(FileAction::ExportPackage { id, theme: Box::new(theme) })
            }
        }
    }

    /// Whether `current` has edits not saved yet.
    pub fn edited(&self, current: &str) -> bool {
        let Some(draft) = self.draft_for(current) else { return false };
        let Some(entry) = self.entries.iter().find(|e| e.id == draft.id) else { return false };
        let renamed = typed_name(draft).is_some_and(|n| n != self.saved_name(&draft.id));
        draft.new
            || renamed
            || draft.theme.vars.values != entry.theme.vars.values
            || draft.theme.light() != entry.theme.light()
    }

    /// Whether the theme or variant `id` went because this moved it to the Trash.
    pub fn trashed(&self, id: &str) -> bool {
        self.trashed.iter().any(|t| same_id(t, id))
    }

    /// Drops the draft unless it is of `id`: another theme is used without the editor (a theme
    /// brought in, say), and edits left behind would come back unasked.
    pub fn keep_draft_of(&mut self, id: &str) {
        if self.draft.as_ref().is_some_and(|d| !same_id(&d.id, &self.real_id(id))) {
            self.draft = None;
        }
    }

    /// Drops the draft, unsaved: the editor was left without saving, or Settings closed.
    pub fn end_draft(&mut self) {
        self.draft = None;
    }

    /// Whether the variant editor may write `id` (`Save`): a variant of the user's, or the user's
    /// copy of one a built-in theme ships; not a theme's own values, nor a built-in variant.
    pub fn saveable(&self, id: &str) -> bool {
        let real = self.real_id(id);
        crate::kit::theme_vars::split_id(&real).1.is_some() && !self.shipped(&real)
    }

    /// Whether `id` is a variant a built-in theme ships that the user has no copy of.
    fn shipped(&self, id: &str) -> bool {
        let real = self.real_id(id);
        let (base, variant) = crate::kit::theme_vars::split_id(&real);
        variant.is_some()
            && theme::is_built_in(base)
            && self.entries[..self.built_in].iter().any(|e| e.id == real)
            && !self.overridden.iter().any(|o| same_id(o, &real))
    }

    /// The saved name of the variant `id`: a variant's own, or the theme's default variant's.
    fn saved_name(&self, id: &str) -> String {
        let real = self.real_id(id);
        let Some(entry) = self.entries.iter().find(|e| e.id == real) else { return String::new() };
        match crate::kit::theme_vars::split_id(&real).1 {
            Some(variant) => entry.theme.vars.variant_name.clone().unwrap_or_else(|| variant.to_owned()),
            None => entry.theme.meta.default_variant().to_owned(),
        }
    }

    fn draft_for(&self, id: &str) -> Option<&Draft> {
        self.draft.as_ref().filter(|d| same_id(&d.id, id))
    }

    /// Starts a draft of `current` when there is none for it.
    fn open_draft(&mut self, current: &str) -> Option<()> {
        let real = self.real_id(current);
        if self.draft_for(&real).is_none() {
            let theme = self.entries.iter().find(|e| e.id == real)?.theme.clone();
            self.draft = Some(Draft { id: real, name: None, light: None, values: BTreeMap::new(), theme, new: false });
        }
        Some(())
    }

    /// Makes the draft's theme again from its entry as saved and the edits, so a file read again
    /// keeps the edits over what it now says. A draft whose variant is gone is dropped.
    fn rebuild_draft(&mut self) {
        let Some(draft) = self.draft.as_mut() else { return };
        let Some(entry) = self.entries.iter().find(|e| e.id == draft.id) else {
            self.notice = Some(format!("{} was removed, so its unsaved changes were dropped", draft.id));
            self.draft = None;
            return;
        };
        let mut theme = entry.theme.clone();
        for (name, value) in &draft.values {
            if theme.vars.base.get(name) == Some(value) && crate::kit::theme_vars::split_id(&draft.id).1.is_none() {
                theme.vars.values.remove(name);
            } else {
                theme.vars.values.insert(name.clone(), value.clone());
            }
        }
        if let Some(name) = typed_name(draft)
            && crate::kit::theme_vars::split_id(&draft.id).1.is_some()
        {
            theme.vars.variant_name = Some(name);
        }
        if let Some(light) = draft.light {
            theme.vars.light = Some(light);
        }
        crate::kit::theme_vars::resolve(&mut theme);
        // A theme read from a sheet keeps its edited settings as values over the sheet's.
        if !theme.vars.base.is_empty() {
            crate::kit::theme_vars::sync(&mut theme);
            crate::kit::theme_vars::refresh_view(&mut theme);
        }
        draft.theme = theme;
    }

    /// Applies `change` to the theme `base` and each of its variants.
    fn each_of(&mut self, base: &str, mut change: impl FnMut(&mut Theme)) {
        for entry in self.entries.iter_mut().filter(|e| crate::kit::theme_vars::split_id(&e.id).0 == base) {
            change(&mut entry.theme);
        }
        self.rebuild_draft();
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
        let mut change = Some(change);
        let real = self.real_id(current);
        self.get(&real)?;
        let (base, variant) = crate::kit::theme_vars::split_id(&real);
        let (id, switched) = if structure && theme::is_built_in(base) {
            let name = format!("{} (custom)", self.get(current).map_or("Suhina Studio", |t| &t.name));
            (self.copy(current, &name, true), true)
        } else if variant.is_some() || !settings {
            (real.clone(), false)
        } else {
            let base = base.to_string();
            let id = self.free_variant(&base, "Custom");
            let mut theme = self.entries.iter().find(|e| e.id == real)?.theme.clone();
            theme.vars.variant = crate::kit::theme_vars::split_id(&id).1.map(str::to_owned);
            theme.vars.variant_name = Some("Custom".into());
            // A change that changes nothing (a reset of what was not edited) makes no variant.
            let mut changed = theme.clone();
            if let Some(change) = change.take() {
                change(&mut changed);
            }
            if changed == theme {
                return None;
            }
            self.entries.push(ThemeEntry { id: id.clone(), theme: changed });
            (id, true)
        };
        let entry = self.entries.iter_mut().find(|e| e.id == id)?;
        if let Some(change) = change.take() {
            change(&mut entry.theme);
        }
        // A theme read from a sheet keeps its edited settings as values over the sheet's.
        if !entry.theme.vars.base.is_empty() {
            crate::kit::theme_vars::sync(&mut entry.theme);
            crate::kit::theme_vars::refresh_view(&mut entry.theme);
        }
        self.actions.push(Pending::Save(id.clone()));
        // A draft over it is made again over the changed entry.
        self.rebuild_draft();
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

    /// The answer to a package whose theme is here already: replace it, or bring in a copy.
    pub fn import_again(&mut self, path: PathBuf, replace: bool) {
        self.actions.push(Pending::Other(Box::new(FileAction::ImportAgain { path, replace })));
    }

    /// Something to say once in the status bar about the last request.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    /// What the app should do with the theme files, in order, once.
    pub fn take_actions(&mut self) -> Vec<FileAction> {
        let pending = std::mem::take(&mut self.actions);
        let mut actions: Vec<FileAction> = Vec::new();
        for item in pending {
            let action = match item {
                // What the entry holds, never an unsaved draft over it: Save puts the draft into
                // the entry first.
                Pending::Save(id) => match self.entries.iter().find(|e| e.id == id) {
                    Some(entry) => FileAction::Save { id, theme: Box::new(entry.theme.clone()) },
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

    /// Adds a copy of the theme `from` named `name`, under a new id; of what is saved when `saved`,
    /// else with the unsaved edits. Returns the id.
    fn copy(&mut self, from: &str, name: &str, saved: bool) -> String {
        let original = self.copy_of(from, saved);
        let id = self.free_id(name);
        self.entries.push(ThemeEntry { id: id.clone(), theme: Theme { name: name.to_string(), ..original } });
        id
    }

    /// The theme `from` as a theme of its own: its variant's values become part of its sheet.
    /// Of what is saved when `saved`: a change made apart from the variant editor (the toolbar's)
    /// takes none of the editor's unsaved edits with it.
    fn copy_of(&self, from: &str, saved: bool) -> Theme {
        let kept = saved.then(|| self.entries.iter().find(|e| same_id(&e.id, from)).map(|e| &e.theme)).flatten();
        let mut theme = kept.or_else(|| self.get(from)).cloned().unwrap_or_default();
        theme.vars.variant = None;
        theme.vars.variant_name = None;
        theme
    }

    /// An id made from `name` that no theme has yet.
    pub fn free_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut number = 2;
        while self.get(&id).is_some()
            || theme::is_built_in(&id)
            || self.reserved.iter().any(|r| r.eq_ignore_ascii_case(&id))
        {
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

/// What of a skin is its structure rather than settings: artwork, the toolbar and the background
/// image itself.
fn structure(skin: &crate::kit::skin::Skin) -> impl PartialEq + use<> {
    (skin.assets.clone(), skin.toolbar.clone(), skin.toolbar_offsets.clone(), skin.background.clone())
}

/// Ids name files, and file names do not differ by case on every system.
fn same_id(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
#[path = "themes_tests.rs"]
mod tests;

/// The name typed in the editor, trimmed; none when the field was emptied, which keeps the name
/// the variant has.
fn typed_name(draft: &Draft) -> Option<String> {
    draft.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned)
}
