// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Settings page, drawn over the main view until it is closed. Its pages are listed down the
//! left under a search field; each setting is a row: a label, a short note and its control. What
//! is on which page is the table in `settings_rows.rs` (`docs/spec/ui-settings.md`).

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2, pos2};

use crate::interaction::pick::PickField;
use crate::kit::motion;
use crate::kit::theme::{self, Colors, PAD};
use crate::kit::widgets;
use crate::settings::keymap::{Keymap, Preset, Shortcut, actions};
use crate::settings::workflow::Setting;
use crate::settings::{Descriptions, Graphics, Settings};

const PAGE_WIDTH: f32 = 560.0;
/// The list of pages on the left, and the gap between it and the page.
const SIDEBAR: f32 = 140.0;
const SIDEBAR_GAP: f32 = 24.0;
const SIDEBAR_ROW: f32 = 20.0;
/// The search field over the list of pages.
const SEARCH_FIELD: f32 = 18.0;
const LABEL_WIDTH: f32 = 180.0;
/// The width a row's note has: the page's width less the name column, one line of small text
/// (a test guard measures the notes against it; a smaller window cuts a note).
#[cfg(test)]
pub(crate) const NOTE_WIDTH: f32 = PAGE_WIDTH - LABEL_WIDTH;
const SETTING_ROW: f32 = 34.0;
/// Where every setting's control starts: names in one column and controls in the next, lined
/// up down the page, so the eye runs straight down either.
const CONTROL_COLUMN: f32 = 240.0;
/// A setting's row when it is its name and its control alone; its description is the name's
/// tooltip.
const LINE_ROW: f32 = 24.0;
const SEGMENT: f32 = 15.0;
/// The Browse… button at the end of a path field.
const BROWSE_WIDTH: f32 = 60.0;
/// A dropdown on the right of its row (Settings › Audio).
const DROPDOWN_FIELD: f32 = 260.0;
/// Room right of the page for its scroll bar.
const SCROLL_GUTTER: f32 = 12.0;
/// The least height of a list that fills the space left on its page; a shorter window scrolls
/// the page to it.
const LIST_LEAST: f32 = 160.0;

/// Facts the page shows but does not change.
#[derive(Debug, Clone)]
pub struct About {
    /// What the active project plays on now, read every frame (`audio.info`).
    pub audio_live: Option<suhina_session::OutputInfo>,
    /// The name of the track whose chain came back late from the render workers most often
    /// (ADR 0038 › 3), when one did.
    pub audio_late_track: Option<String>,
    /// The audio hosts and their output devices, listed when Settings › Audio opens or is
    /// refreshed (`audio.devices`).
    pub audio_hosts: Vec<suhina_session::audio::HostInfo>,
    /// The MIDI input ports, listed with the audio devices (`midi.inputs`), or why they could
    /// not be listed.
    pub midi_inputs: Result<Vec<MidiPort>, String>,
    /// How the window is drawn right now.
    pub graphics_now: Option<Graphics>,
    /// Plug-in editors can open inside the device panel here (the window is a Win32 or X11
    /// one); elsewhere the setting for it is shown but does nothing.
    pub editors_embeddable: bool,
    /// The folders projects are saved in and templates kept in when the settings leave them
    /// empty.
    pub projects_default: String,
    pub templates_default: String,
    /// The themes folder used while Themes folder is empty.
    pub themes_default: String,
    /// Suhina's default data folder, used while Settings folder is empty.
    pub data_default: String,
    /// The NAM devices' folders ([docs/spec/nam-amp-and-cab.md]) the same way.
    pub nam_models_default: String,
    pub irs_default: String,
    /// The active project's newest backup, as Settings › Projects and backups shows it: when and where.
    pub last_backup: String,
    /// The active project's implicit MCP write folder, displayed alongside explicit grants.
    pub mcp_project_folder: Option<String>,
}

impl Default for About {
    fn default() -> Self {
        Self {
            audio_live: None,
            audio_late_track: None,
            audio_hosts: Vec::new(),
            midi_inputs: Ok(Vec::new()),
            graphics_now: None,
            editors_embeddable: false,
            projects_default: String::new(),
            templates_default: String::new(),
            themes_default: String::new(),
            data_default: String::new(),
            nam_models_default: String::new(),
            irs_default: String::new(),
            last_backup: Default::default(),
            mcp_project_folder: None,
        }
    }
}

/// A MIDI input port as Settings › Audio shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MidiPort {
    pub name: String,
    pub enabled: bool,
    pub open: bool,
    /// Why it could not be opened.
    pub error: Option<String>,
}

/// The plug-in library as the page and the plug-in manager show it, read through the
/// `plugin.*` commands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginsView {
    /// The folders scans may look in: the system's standard ones, then the ones added.
    pub folders: Vec<FolderRow>,
    /// Each plug-in found.
    pub found: Vec<PluginRow>,
    /// Files that failed to load, and why.
    pub failed: Vec<(String, String)>,
    /// Files built for another processor, and why they do not load here.
    pub incompatible: Vec<(String, String)>,
    /// A scan is running in the background.
    pub scanning: bool,
    /// The folder typed into the add field, and the format chosen for it.
    pub new_folder: String,
    pub new_format: suhina_plugins::scan::FolderFormat,
    /// The plug-in manager: whether it shows, its search and sort.
    pub manager: crate::settings::plugin_manager::ManagerState,
}

/// A plug-in folder as the page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRow {
    pub path: String,
    /// `any`, `vst3`, `clap` or `vst2`.
    pub format: String,
    pub enabled: bool,
    /// One of the system's: it can be switched off, not removed.
    pub standard: bool,
}

/// A plug-in as the manager lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginRow {
    pub name: String,
    pub vendor: String,
    /// `instrument` or `effect`.
    pub kind: String,
    pub format: String,
    pub version: String,
    pub file: String,
    /// How the commands name it (`plugin.setEnabled`).
    pub spec: String,
    pub enabled: bool,
    /// Marked as the user's own (`plugin.setOwned`).
    pub owned: bool,
}

/// What the page or the plug-in manager asks the app to do with the plug-in library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRequest {
    /// Scan new and changed files, or every file again.
    Scan {
        all: bool,
    },
    AddFolder(String, suhina_plugins::scan::FolderFormat),
    /// The add row with nothing typed: the app shows the system's folder picker.
    PickFolder,
    RemoveFolder(String),
    /// Switch a folder on or off for scans.
    SetFolderEnabled(String, bool),
    /// Switch a plug-in (by spec) on or off.
    SetEnabled(String, bool),
    /// Switch several plug-ins (by spec) on or off together (#725).
    SetEnabledMany(Vec<String>, bool),
    /// Mark several plug-ins (by spec) as owned, or take the mark away.
    SetOwnedMany(Vec<String>, bool),
    /// Read these files again now, one after another.
    ScanFiles(Vec<String>),
    /// Show a file in the system's file manager (Explorer, Finder), picked in its folder.
    ShowFile(String),
}

/// The themes as the page shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemesView {
    /// Each theme's id, name and whether it comes with Suhina: the user's own first.
    pub list: Vec<(String, String, bool)>,
    /// Each theme's and variant's swatch colour, by id (`theme_vars::swatch`).
    pub swatches: Vec<(String, egui::Color32)>,
    /// Each theme's and variant's thumbnail with its values substituted, by id; a theme without
    /// one shows a card filled with its swatch colour.
    pub pictures: Vec<(String, std::sync::Arc<str>)>,
    /// Each theme's `theme.json`: name, author, description and default variant's name, by id.
    pub info: Vec<(String, ThemeInfo)>,
    /// The id of the theme in use, whether it comes with Suhina, its name and its colours.
    pub current: String,
    pub built_in: bool,
    pub name: String,
    pub colors: Colors,
    /// The liquid strengths the theme in use sets itself (`window { liquid }`), which Appearance's
    /// sliders then do not.
    pub liquid: Option<crate::kit::liquid::Options>,
    /// The folder theme files are kept in, if there is one.
    pub folder: Option<String>,
    /// Every theme variant: its entry's id (`<theme>@<variant>`) and its name.
    pub variants: Vec<(String, String)>,
    /// The variants a built-in theme ships, which do not change.
    pub locked: Vec<String>,
    /// Variants a built-in theme ships that a file of the user's replaces (an earlier version's
    /// edit): *Revert to built-in* moves that file to the Trash.
    pub overridden: Vec<String>,
    /// Themes left in an earlier file format, by id, and why they were not converted.
    pub kept: Vec<(String, String)>,
    /// The user's themes whose sheet has notices, by id, with them.
    pub notices: Vec<(String, Vec<String>)>,
    /// The name of the variant in use, when one is.
    pub variant: Option<String>,
    /// The name of what is in use as the editor shows it: the variant's, or the default
    /// variant's, with a name typed and not saved yet.
    pub variant_name: String,
    /// What is in use has edits not saved yet (the variant editor's draft).
    pub edited: bool,
    /// The variant editor may save what is in use in place (not a theme's own values, nor a
    /// variant a built-in theme ships).
    pub saveable: bool,
    /// The editor holds a new variant (the + box), written as a variant of its own on Save.
    pub new_variant: bool,
    /// The settings the theme in use declares itself (`@property` with a label), in its order.
    pub declared: Vec<suhina_style::registered::Registered>,
    /// The value of every custom property of the theme in use, with its variant's.
    pub values: std::collections::BTreeMap<String, String>,
}

/// A theme's `theme.json` as *New theme…* and *Theme info…* edit it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeInfo {
    pub name: String,
    pub author: String,
    pub description: String,
    /// The default variant's name: the theme's own values.
    pub default_variant: String,
}

/// What *New theme…* makes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewTheme {
    /// The theme (or variant) it starts from; none for Suhina's defaults (the user-agent sheet).
    pub from: Option<String>,
    pub info: ThemeInfo,
    /// Include the default artwork: copy its SVGs into the theme's `assets/` with their rules.
    pub artwork: bool,
}

/// What the page asks the app to do with the themes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeRequest {
    /// Use the theme with this id (`<theme>@<variant>` for a variant).
    Use(String),
    /// Set one of the declared settings of the variant in use, in the editor's draft.
    EditValue { name: String, value: String },
    /// Name the variant in use so, in the editor's draft.
    EditName(String),
    /// Write the editor's draft to the variant in use.
    Save,
    /// Write the editor's draft (or the variant in use) as a new variant, and use it.
    SaveAsNew,
    /// Drop the editor's draft.
    Discard,
    /// A new variant of the theme of `id`, with the values of `id`, saved and used.
    NewVariant(String),
    /// A copy of the variant `id`, saved beside it.
    DuplicateVariant(String),
    /// Move the theme or variant `id` to the system's Trash.
    Delete(String),
    /// Make a theme and use it.
    NewTheme(NewTheme),
    /// Write the theme `id`'s `theme.json`.
    EditInfo { id: String, info: ThemeInfo },
    /// The toolbar editor's layout; a built-in theme is copied first, since it does not change.
    EditSkin(crate::kit::skin::Skin),
    /// Copy the default artwork the theme `id` lacks into its `assets/`, with a rule for each in
    /// its `theme.css`.
    AddDefaultArtwork(String),
    /// Choose a theme package or zip in the system's dialog and bring it in.
    Import,
    /// Show the themes folder in the file manager.
    OpenFolder,
    /// Show the theme `id`'s folder in the file manager.
    Reveal(String),
    /// Write every theme file into one zip.
    Backup,
    /// Write the theme `id` as a theme package to share (`.theme.suhina`).
    ExportPackage(String),
}

/// A question or form the Themes page asks in a box over Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeDialog {
    NewTheme(NewTheme),
    Info {
        id: String,
        info: ThemeInfo,
    },
    /// Move the theme or variant `id`, named `name`, to the Trash; `revert` for the user's copy
    /// of a variant a built-in theme ships.
    Delete {
        id: String,
        name: String,
        revert: bool,
    },
}

/// Where the user was going when the variant editor had unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    /// Back to the themes.
    Back,
    /// Another page of Settings.
    Page(Page),
    /// Settings closed.
    Close,
}

/// The pages of Settings, in the order the list on the left shows them
/// (`docs/spec/ui-settings.md` › Pages and sections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Projects,
    Audio,
    Midi,
    Plugins,
    Devices,
    Importing,
    Arrangement,
    Session,
    ClipEditor,
    Browser,
    Keys,
    Appearance,
    /// Appearance › Themes, a page of its own under Appearance in the list.
    Theme,
    Agents,
}

impl Page {
    /// Every page with its name.
    pub const ALL: [(Page, &'static str); 14] = [
        (Page::Projects, "Projects and backups"),
        (Page::Audio, "Audio"),
        (Page::Midi, "MIDI"),
        (Page::Plugins, "Plug-ins"),
        (Page::Devices, "Devices"),
        (Page::Importing, "Importing"),
        (Page::Arrangement, "Arrangement"),
        (Page::Session, "Launcher and mixer"),
        (Page::ClipEditor, "Clip editor"),
        (Page::Browser, "Browser"),
        (Page::Keys, "Keys"),
        (Page::Appearance, "Appearance"),
        (Page::Theme, "Themes"),
        (Page::Agents, "Agents and updates"),
    ];

    /// The page's name, as the list shows it.
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(page, _)| *page == self).map_or("", |(_, name)| name)
    }
}

/// What the Settings page keeps between frames: the page shown, and the action whose keys are
/// being set (the next key pressed becomes its key).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageState {
    pub page: Page,
    /// Settings lives in a separate native viewport until Dock is chosen.
    pub window: bool,
    pub capture: Option<String>,
    /// The variant editor, a child view of Themes, is open.
    pub theme_editor: bool,
    /// The variant editor had unsaved changes when this was asked: it asks whether to save first.
    pub theme_leave: Option<Leave>,
    /// *Discard changes* asks first.
    pub theme_discard: bool,
    /// A question or form shown in place of the gallery.
    pub theme_dialog: Option<ThemeDialog>,
    pub skin_editor: super::skin_editor::State,
    /// What the theme list is filtered by: part of a theme's or a variant's name, or its id.
    pub theme_search: String,
    /// A colour picker is open while its primary pointer button is held.
    pub theme_colour_held: bool,
    /// The audio devices were listed since the Audio page was opened.
    pub audio_listed: Option<bool>,
    /// A driver picked in the form, applied only once its device is chosen.
    pub audio_pending: Option<suhina_session::OutputChoice>,
    /// What is typed in the search field: while it has text, the settings that match it are shown
    /// in place of a page.
    pub search: String,
    /// The page and search the scroll position belongs to: another one opens at its top.
    pub scrolled: Option<(Page, String)>,
    /// The control with the keyboard focus when it was last scrolled into view.
    pub focused: Option<Id>,
    /// What is typed in the Shortcuts page's own filter: only the actions it matches are listed.
    pub key_filter: String,
    /// The pages whose Advanced group is unfolded.
    pub advanced: Vec<Page>,
    /// The Help dialog over the window is open: how the settings are explained.
    pub help_open: bool,
}

impl PageState {
    /// The words searched for, in lower case; none when the page is shown.
    fn words(&self) -> Vec<String> {
        self.search.split_whitespace().map(str::to_lowercase).collect()
    }

    /// The audio devices or the MIDI ports may be shown: their figures are read again while so.
    pub fn shows_devices(&self) -> bool {
        let words = self.words();
        if words.is_empty() {
            return matches!(self.page, Page::Audio | Page::Midi);
        }
        entries().iter().any(|e| matches!(e.page, Page::Audio | Page::Midi) && e.matches(&words))
    }
}

/// What the user did on the page this frame.
#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    pub changed: bool,
    /// A value control is being held or dragged, even if its value did not move this frame.
    pub adjusting: bool,
    /// Move the embedded panel by this frame's title drag; native windows use OS chrome.
    pub move_by: Vec2,
    pub close: bool,
    pub plugins: Option<PluginRequest>,
    pub themes: Option<ThemeRequest>,
    pub keymap_changed: bool,
    /// The most colour tags the browser may have, as chosen.
    pub tag_limit: Option<usize>,
    /// The audio output was chosen anew: the app runs `audio.setOutput` with it, or
    /// `output.setCuePair` or `output.setCueVolume` when only the cue changed.
    pub audio_output: Option<suhina_session::OutputChoice>,
    /// The audio devices should be listed again.
    pub audio_refresh: bool,
    /// Ask the active output's owner to open its native ASIO panel.
    pub audio_control_panel: bool,
    /// A MIDI input port was turned on or off: the app runs `midi.setInput`.
    pub midi_input: Option<(String, bool)>,
    /// A field's Browse… was clicked: the app shows the system's dialog for it.
    pub pick: Option<PickField>,
    /// A folder that moves its files asked to go back to its default (Reset).
    pub folder_reset: Option<PickField>,
}

/// What the page shows and changes besides the settings.
pub struct Sources<'a> {
    pub about: &'a About,
    pub plugins: &'a mut PluginsView,
    pub themes: &'a ThemesView,
    pub keymap: &'a mut Keymap,
    /// The most colour tags the browser may have now.
    pub tag_limit: usize,
}

/// A setting the window shows: an entry of the table in `settings_rows.rs`.
#[derive(Clone, Copy)]
pub struct Entry {
    pub page: Page,
    /// The heading it is under on its page.
    pub section: &'static str,
    /// Its name and note, as its row shows them; searched with the section and the page.
    pub name: &'static str,
    pub note: &'static str,
    /// The keys it is saved under in `settings.json` (`keymap.json` for keys); none when it is
    /// read only or saved elsewhere.
    pub keys: &'static [&'static str],
    pub draw: Draw,
    /// Whether it is shown with these settings (most are always).
    pub shown: fn(&Settings) -> bool,
    /// Whether it is a setting, a status readout or a row of commands.
    pub role: Role,
}

/// What an entry is. Only settings sit among the settings; a status or a row of commands is set
/// off at the top of its page (`docs/spec/ui-settings.md` › Adding a setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Setting,
    /// Something the program reports; drawn in a panel.
    Status,
    /// Buttons that do something once.
    Actions,
}

/// The heading of a page's folded group of rarely changed settings.
pub const ADVANCED: &str = "Advanced";

/// How an entry draws itself.
#[derive(Clone, Copy)]
pub enum Draw {
    /// One row: what the function draws on the right, the name and note on the left.
    Row(fn(&mut Cx<'_>, Rect)),
    /// Several rows or a list from the height given, moving it past what was drawn.
    Block(fn(&mut Cx<'_>, &mut f32)),
}

fn always(_: &Settings) -> bool {
    true
}

/// A setting of one row.
pub(crate) const fn row(
    page: Page,
    section: &'static str,
    name: &'static str,
    note: &'static str,
    keys: &'static [&'static str],
    draw: fn(&mut Cx<'_>, Rect),
) -> Entry {
    Entry { page, section, name, note, keys, draw: Draw::Row(draw), shown: always, role: Role::Setting }
}

/// A setting drawn as several rows or a list.
pub(crate) const fn block(
    page: Page,
    section: &'static str,
    name: &'static str,
    note: &'static str,
    keys: &'static [&'static str],
    draw: fn(&mut Cx<'_>, &mut f32),
) -> Entry {
    Entry { page, section, name, note, keys, draw: Draw::Block(draw), shown: always, role: Role::Setting }
}

impl Entry {
    /// Shown only when `shown` says so.
    pub(crate) const fn when(self, shown: fn(&Settings) -> bool) -> Self {
        Self { shown, ..self }
    }

    /// A readout of what is happening, not a setting.
    pub(crate) const fn status(self) -> Self {
        Self { role: Role::Status, ..self }
    }

    /// A row of commands, not a setting.
    pub(crate) const fn actions(self) -> Self {
        Self { role: Role::Actions, ..self }
    }

    /// Every word is in its name, note, section or page; for the shortcuts, in an action's too.
    #[cfg(test)]
    pub(crate) fn matches_words(&self, words: &[String]) -> bool {
        self.matches(words)
    }

    fn matches(&self, words: &[String]) -> bool {
        let text = format!("{} {} {} {}", self.name, self.note, self.section, self.page.name()).to_lowercase();
        words.iter().all(|word| text.contains(word.as_str()))
            || (self.page == Page::Keys
                && actions().iter().any(|a| action_matches(&a.label, &a.id, a.group, "", words)))
    }
}

/// Every setting the window shows, in the order it shows them.
pub fn entries() -> &'static [Entry] {
    crate::settings::settings_rows::ROWS
}

/// What an entry draws with: the page, the settings and what else it shows, and what the user
/// asked for this frame.
pub struct Cx<'a> {
    pub ui: &'a mut Ui,
    pub painter: &'a egui::Painter,
    /// The page's width, its top at the top of what scrolls; its bottom where the window's space
    /// ends when it is scrolled to the top.
    pub page: Rect,
    pub settings: &'a mut Settings,
    pub about: &'a About,
    pub plugins: &'a mut PluginsView,
    pub themes: &'a ThemesView,
    pub keymap: &'a mut Keymap,
    pub tag_limit: usize,
    pub state: &'a mut PageState,
    pub outcome: &'a mut Outcome,
    /// The entry being drawn: its name names its controls.
    name: &'static str,
    /// A note in place of the entry's own, this frame.
    note: Option<String>,
    /// Where the name and note go when the control leaves less room for them.
    label: Option<Rect>,
}

impl Cx<'_> {
    /// The person's explicit MCP write grants; each includes all its subfolders.
    pub fn mcp_write_folders(&mut self, y: &mut f32) {
        let row = setting_row(self.page, y);
        label(self.painter, row, "More agent write folders", "Each allowed folder includes its subfolders");
        let mut remove = None;
        for (index, folder) in self.settings.mcp_write_folders.iter().enumerate() {
            let row = setting_row(self.page, y);
            let button =
                Rect::from_min_size(Pos2::new(row.right() - BROWSE_WIDTH, row.top()), Vec2::new(BROWSE_WIDTH, SEGMENT));
            let text = Rect::from_min_max(row.min, Pos2::new(button.left() - PAD, button.bottom()));
            widgets::text_in(
                self.painter,
                text,
                Align2::LEFT_CENTER,
                folder,
                theme::ui_font(theme::TEXT_SMALL),
                ink().name,
            );
            if widgets::button(self.ui, self.painter, button, "Remove", &format!("Remove MCP write folder {folder}")) {
                remove = Some(index);
            }
        }
        if let Some(index) = remove {
            self.settings.mcp_write_folders.remove(index);
            self.outcome.changed = true;
        }
        let row = setting_row(self.page, y);
        let button = Rect::from_min_size(row.min, Vec2::new(100.0, SEGMENT));
        if widgets::button(self.ui, self.painter, button, "Add folder…", "Add MCP write folder") {
            self.outcome.pick = Some(PickField::McpWriteFolder);
        }
    }

    /// Shows `note` under the name in place of the entry's own note.
    pub fn note(&mut self, note: impl Into<String>) {
        self.note = Some(note.into());
    }

    /// A segmented control on the right of `row` for the setting `field` gives: a click sets
    /// it and saves.
    pub fn choose<T: Copy + PartialEq>(
        &mut self,
        row: Rect,
        options: &[(&str, T)],
        field: impl FnOnce(&mut Settings) -> &mut T,
    ) {
        let value = field(self.settings);
        // Option buttons for up to five choices that exclude each other, a drop-down beyond that.
        if options.len() > MAX_BUTTONS {
            let listed: Vec<(String, T)> = options.iter().map(|(text, v)| (text.to_string(), *v)).collect();
            if let Some(picked) = choice_dropdown(self.ui, self.painter, row, self.name, &listed, value) {
                *value = picked;
                self.outcome.changed = true;
            }
            return;
        }
        let rect = options_rect(self.ui, row, options);
        if let Some(picked) = segmented(self.ui, self.painter, rect, self.name, options, *value) {
            *value = picked;
            self.outcome.changed = true;
        }
    }

    /// The default scale: its root and its mode, two drop-downs side by side.
    pub fn default_scale(&mut self, row: Rect) {
        let left = control_x(row).min(row.right() - DROPDOWN_FIELD - 60.0);
        let field = |from: f32, to: f32| {
            Rect::from_min_max(Pos2::new(from, row.top() + 1.0), Pos2::new(to, row.top() + SEGMENT + 3.0))
        };
        let split = (left + 56.0).min(row.right());
        let roots: Vec<(String, u8)> =
            (0..12u8).map(|i| (crate::detail::piano_roll::pitch_class_name(i).to_string(), i)).collect();
        let root = self.settings.default_scale_root;
        if let Some(picked) =
            choice_field(self.ui, self.painter, field(left, split - 4.0), "Default scale root", &roots, &root)
        {
            self.settings.default_scale_root = picked;
            self.outcome.changed = true;
        }
        let modes: Vec<(String, suhina_engine::model::ScaleMode)> =
            suhina_engine::model::ScaleMode::ALL.iter().map(|m| (m.label().to_string(), *m)).collect();
        let mode = self.settings.default_scale_mode;
        if let Some(picked) =
            choice_field(self.ui, self.painter, field(split, row.right()), "Default scale mode", &modes, &mode)
        {
            self.settings.default_scale_mode = picked;
            self.outcome.changed = true;
        }
    }

    /// A percentage applied live to its colour surface.
    pub fn colour_strength(&mut self, row: Rect, field: impl FnOnce(&mut Settings) -> &mut u8) {
        let value = field(self.settings);
        let rect = Rect::from_min_size(Pos2::new(row.right() - 190.0, row.top()), Vec2::new(190.0, SEGMENT));
        let response = self.ui.put(rect, egui::Slider::new(value, 0..=100).suffix(" %"));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, self.name));
        self.outcome.adjusting |= response.dragged() || response.is_pointer_button_down_on();
        self.outcome.changed |= response.changed();
    }

    pub fn background_blur(&mut self, row: Rect) {
        let rect = Rect::from_min_size(Pos2::new(row.right() - 190.0, row.top()), Vec2::new(190.0, SEGMENT));
        let response =
            self.ui.put(rect, egui::Slider::new(&mut self.settings.view.background_blur, 0..=40).suffix(" px"));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, self.name));
        self.outcome.adjusting |= response.dragged() || response.is_pointer_button_down_on();
        self.outcome.changed |= response.changed();
    }

    /// Per-surface material strength; saved even without a GPU provider.
    pub fn liquid_strength(&mut self, row: Rect, field: impl FnOnce(&mut Settings) -> &mut u8) {
        let inherited = self.themes.liquid.is_none();
        if !inherited {
            self.note("The theme in use sets its own liquid strengths, so these sliders are off");
        }
        let value = field(self.settings);
        let rect = Rect::from_min_size(Pos2::new(row.right() - 190.0, row.top()), Vec2::new(190.0, SEGMENT));
        let response =
            self.ui.add_enabled_ui(inherited, |ui| ui.put(rect, egui::Slider::new(value, 0..=100).suffix(" %"))).inner;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, inherited, self.name));
        if !crate::kit::liquid::available(self.ui.ctx()) {
            response.clone().on_hover_text("Saved for later. To see liquid, select GPU in Appearance › Advanced › Graphics renderer, enable Animations and restart Suhina.");
        }
        self.outcome.adjusting |= response.dragged() || response.is_pointer_button_down_on();
        self.outcome.changed |= response.changed();
    }

    /// The custom waveform and note ink, with the picker's RGB/hex controls.
    pub fn waveform_colour(&mut self, row: Rect) {
        let rect = Rect::from_min_size(Pos2::new(row.right() - 190.0, row.top()), Vec2::new(190.0, SEGMENT));
        let mut colour = self.settings.view.waveform.custom;
        let mut held = false;
        self.ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.horizontal(|ui| {
                let popup = ui.auto_id_with("popup");
                let response =
                    egui::color_picker::color_edit_button_srgba(ui, &mut colour, egui::color_picker::Alpha::Opaque);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ColorWell, true, self.name));
                held = egui::Popup::is_id_open(ui.ctx(), popup) && ui.input(|input| input.pointer.primary_down());
                ui.label(
                    egui::RichText::new(theme::hex(colour)).font(theme::mono_font(theme::TEXT_SMALL)).color(ink().name),
                );
            });
        });
        self.outcome.adjusting |= held;
        if colour != self.settings.view.waveform.custom {
            self.settings.view.waveform.custom = colour;
            self.outcome.changed = true;
        }
    }

    /// Tune a named size; numbers and units are reserved for Advanced mode.
    pub fn track_size_height(&mut self, row: Rect, size: crate::settings::track_sizes::TrackSize) {
        let sizes = self.settings.view.track_sizes;
        let (lower, upper) = sizes.bounds(size);
        let mut value = sizes.height(size);
        let rect = Rect::from_min_size(Pos2::new(row.right() - 190.0, row.top()), Vec2::new(190.0, SEGMENT));
        let response = if sizes.advanced {
            crate::kit::widgets::number_input_at(
                self.ui,
                rect,
                egui::DragValue::new(&mut value).range(lower..=upper).speed(1).suffix(" px"),
            )
        } else {
            self.ui.put(rect, egui::Slider::new(&mut value, lower..=upper).show_value(false))
        };
        let kind = if sizes.advanced { egui::Role::SpinButton } else { egui::Role::Slider };
        response.widget_info(|| egui::WidgetInfo::labeled(kind, true, self.name));
        self.outcome.adjusting |= response.dragged() || response.is_pointer_button_down_on();
        if response.changed() {
            self.settings.view.track_sizes.set_height(size, value);
            self.outcome.changed = true;
        }
    }

    /// The custom length of a take crossfade, in milliseconds.
    pub fn take_crossfade_custom(&mut self, row: Rect) {
        let mut value = self.settings.view.take_crossfade_custom;
        let rect = Rect::from_min_size(Pos2::new(row.right() - 190.0, row.top()), Vec2::new(190.0, SEGMENT));
        let response = crate::kit::widgets::number_input_at(
            self.ui,
            rect,
            egui::DragValue::new(&mut value).range(1..=500).speed(1).suffix(" ms"),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, self.name));
        self.outcome.adjusting |= response.dragged() || response.is_pointer_button_down_on();
        if response.changed() {
            self.settings.view.take_crossfade_custom = value;
            self.outcome.changed = true;
        }
    }

    /// A switch for the yes/no setting `field` gives.
    pub fn on_off(&mut self, row: Rect, field: impl FnOnce(&mut Settings) -> &mut bool) {
        let value = field(self.settings);
        if let Some(on) = switch(self.ui, self.painter, row, self.name, *value) {
            *value = on;
            self.outcome.changed = true;
        }
    }

    /// A switch that shows the yes/no setting `field` gives the other way round: on when it is no.
    pub fn off_on(&mut self, row: Rect, field: impl FnOnce(&mut Settings) -> &mut bool) {
        let value = field(self.settings);
        if let Some(on) = switch(self.ui, self.painter, row, self.name, !*value) {
            *value = !on;
            self.outcome.changed = true;
        }
    }

    /// A dropdown on the right of `row` for the setting `field` gives.
    pub fn dropdown<T: Clone + PartialEq>(
        &mut self,
        row: Rect,
        options: &[(String, T)],
        field: impl FnOnce(&mut Settings) -> &mut T,
    ) {
        let value = field(self.settings);
        if let Some(picked) = choice_dropdown(self.ui, self.painter, row, self.name, options, value) {
            *value = picked;
            self.outcome.changed = true;
        }
    }

    /// A path typed into a field on the right of `row`, `hint` shown while it is empty, ending
    /// in a Browse… button that asks the app for the system's dialog.
    pub fn folder(&mut self, row: Rect, hint: &str, pick: PickField, field: impl FnOnce(&mut Settings) -> &mut String) {
        let name = self.name;
        // The name is cut where the field starts.
        self.label = Some(Rect::from_min_size(row.min, Vec2::new(CONTROL_COLUMN - 8.0, row.height())));
        let browse = Rect::from_min_size(
            Pos2::new(row.right() - BROWSE_WIDTH, row.top() + 1.0),
            Vec2::new(BROWSE_WIDTH, SEGMENT),
        );
        if crate::kit::widgets::button(self.ui, self.painter, browse, "Browse…", &format!("Browse for {name}")) {
            self.outcome.pick = Some(pick);
        }
        let rect = Rect::from_min_size(
            Pos2::new(control_x(row), row.top()),
            Vec2::new((row.right() - BROWSE_WIDTH - 4.0 - control_x(row)).max(40.0), SEGMENT + 2.0),
        );
        let value = field(self.settings);
        let edit = self.ui.put(
            rect,
            egui::TextEdit::singleline(value)
                .frame(widgets::input_frame())
                .hint_text(hint)
                .font(theme::ui_font(theme::TEXT_SMALL)),
        );
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, name));
        widgets::input_line(self.ui, &edit);
        if edit.changed() {
            self.outcome.changed = true;
        }
    }

    /// A folder whose files move with it (the settings and the themes): shown, not typed into, so
    /// it changes only through Browse or Reset, each asking first.
    pub fn moved_folder(&mut self, row: Rect, hint: &str, pick: PickField, value: &str) {
        let name = self.name;
        // The name is cut where the field starts.
        self.label = Some(Rect::from_min_size(row.min, Vec2::new(CONTROL_COLUMN - 8.0, row.height())));
        let browse = Rect::from_min_size(
            Pos2::new(row.right() - BROWSE_WIDTH, row.top() + 1.0),
            Vec2::new(BROWSE_WIDTH, SEGMENT),
        );
        if crate::kit::widgets::button(self.ui, self.painter, browse, "Browse…", &format!("Browse for {name}")) {
            self.outcome.pick = Some(pick);
        }
        let reset_width = if value.is_empty() { 0.0 } else { BROWSE_WIDTH };
        let reset = Rect::from_min_size(
            Pos2::new(browse.left() - 4.0 - reset_width, browse.top()),
            Vec2::new(reset_width, SEGMENT),
        );
        if !value.is_empty()
            && crate::kit::widgets::button(
                self.ui,
                self.painter,
                reset,
                "Reset",
                &format!("Reset {name} to its default"),
            )
        {
            self.outcome.folder_reset = Some(pick);
        }
        let rect = Rect::from_min_max(
            Pos2::new(control_x(row), row.top()),
            Pos2::new((reset.left() - 4.0).max(control_x(row) + 40.0), row.top() + SEGMENT + 2.0),
        );
        let (text, color) = if value.is_empty() { (hint, ink().hint) } else { (value, ink().value) };
        self.painter.rect_stroke(rect, 2.0, theme::hairline(crate::kit::element::row_line()), egui::StrokeKind::Inside);
        crate::kit::widgets::text_in(
            self.painter,
            rect.shrink2(Vec2::new(6.0, 0.0)),
            Align2::LEFT_CENTER,
            text,
            theme::ui_font(theme::TEXT_SMALL),
            color,
        );
        let shown = self.ui.interact(rect, Id::unique(("moved-folder", name)), Sense::hover());
        shown.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{name}: {text}")));
    }

    /// The most colour tags the browser may have, kept by the library rather than the settings.
    pub fn tag_limit(&mut self, row: Rect, options: &[(&str, usize)]) {
        let rect = options_rect(self.ui, row, options);
        if let Some(limit) = segmented(self.ui, self.painter, rect, self.name, options, self.tag_limit) {
            self.outcome.tag_limit = Some(limit);
        }
    }

    /// Three buttons, none of them chosen: smaller, back to 100 %, larger.
    pub fn zoom(&mut self, row: Rect) {
        let actual = format!("{} %", self.settings.zoom);
        let options = [("-", Some(-1)), (actual.as_str(), Some(0)), ("+", Some(1))];
        match segmented(self.ui, self.painter, options_rect(self.ui, row, &options), self.name, &options, None) {
            Some(Some(0)) if self.settings.zoom != crate::settings::ZOOM => {
                self.settings.zoom = crate::settings::ZOOM;
                self.outcome.changed = true;
            }
            Some(Some(steps)) if steps != 0 => self.outcome.changed |= self.settings.step_zoom(steps),
            _ => {}
        }
    }
}

/// Form inputs share the surrounding material; individual fields draw their underline.
pub(crate) fn input_style(ui: &mut Ui) {
    let visuals = &mut ui.style_mut().visuals;
    visuals.text_edit_bg_color = Some(egui::Color32::TRANSPARENT);
}

/// The Settings page's words by how much they stand out, `label` and its kinds read once a frame
/// inside the page's element (a row of a page is drawn a hundred times: its words are one kind of
/// each, not one element each).
#[derive(Clone, Copy)]
pub(crate) struct Inks {
    /// A setting's name and a value: `label`, inherited from the page's `color` too.
    pub name: egui::Color32,
    /// What it says under the name: `label[kind="caption"]`, its own `color`.
    pub caption: egui::Color32,
    /// Headings, notes and the dim: `label[kind="hint"]`, its own `color`.
    pub hint: egui::Color32,
    /// A value shown as words, a key: `label[kind="value"]`.
    pub value: egui::Color32,
    /// Something wrong: `label[kind="error"]`.
    pub error: egui::Color32,
}

impl Inks {
    /// The inks of the element that is entered now.
    pub(crate) fn read() -> Self {
        use crate::kit::element::{caption_ink, error_ink, hint_ink, name_ink, value_ink};
        Self { name: name_ink(), caption: caption_ink(), hint: hint_ink(), value: value_ink(), error: error_ink() }
    }
}

thread_local! {
    /// The page's inks while it is drawn.
    static INKS: std::cell::Cell<Option<Inks>> = const { std::cell::Cell::new(None) };
}

/// The page's inks: those `show` read this frame, else read now (a part drawn on its own).
pub(crate) fn ink() -> Inks {
    INKS.get().unwrap_or_else(Inks::read)
}

/// Clears the page's inks when the frame's drawing of it ends, however it ends.
struct InksGuard;

impl Drop for InksGuard {
    fn drop(&mut self) {
        INKS.set(None);
    }
}

/// What the theme picker's cards and swatches paint, read once a frame for all of them: the
/// `theme-card` (`[selected]` for the theme in use) and its `theme-swatch`es.
#[derive(Clone, Copy)]
struct Cards {
    /// The tint under the theme in use.
    tint: egui::Color32,
    /// The card's edge: the theme in use, under the pointer, and otherwise.
    in_use: egui::Color32,
    hovered: egui::Color32,
    plain: egui::Color32,
}

impl Cards {
    fn read() -> Self {
        use crate::kit::element::{Selected, THEME_CARD};
        let in_use = THEME_CARD.attr(Selected, true).look();
        let hovered = THEME_CARD.pointer(true, false, false).look();
        let plain = THEME_CARD.look();
        Self {
            // The tint under the theme in use: its `background-color`, else a little of its accent.
            tint: in_use.background_or(in_use.accent().gamma_multiply(0.14)),
            in_use: in_use.border(),
            hovered: hovered.border(),
            plain: plain.border(),
        }
    }
}

thread_local! {
    /// The cards' colours while the picker is drawn.
    static CARDS: std::cell::Cell<Option<Cards>> = const { std::cell::Cell::new(None) };
}

/// Clears the cards' colours when the picker's drawing ends, however it ends.
struct CardsGuard;

impl Drop for CardsGuard {
    fn drop(&mut self) {
        CARDS.set(None);
    }
}

/// Draws the page into the space left in `ui`.
pub fn show(ui: &mut Ui, settings: &mut Settings, sources: Sources<'_>, state: &mut PageState) -> Outcome {
    let style_class = crate::kit::element::SETTINGS.enter();
    INKS.set(Some(Inks::read()));
    let _inks = InksGuard;
    let mut styled_ui = style_class.ui(ui);
    let ui = &mut *styled_ui;
    let mut outcome = Outcome::default();
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    let panel_base = crate::kit::liquid::panel_base(ui.ctx());
    painter.rect_filled(area, 0.0, panel_base);
    if !state.window {
        crate::kit::liquid::paint(&painter, area, &style_class, panel_base, crate::kit::liquid::Surface::Panel);
    }
    // Its border, and the page in its content box, inside its border and padding (#1336).
    let area = style_class.paint_border(&painter, area);
    input_style(ui);

    let width = SIDEBAR + SIDEBAR_GAP + PAGE_WIDTH;
    let whole = Rect::from_min_max(
        Pos2::new(area.center().x - width / 2.0, area.top()),
        Pos2::new(area.center().x + width / 2.0, area.bottom()),
    )
    .intersect(area.shrink2(Vec2::new(PAD * 2.0, 0.0)));
    let y = area.top() + 24.0;

    // Title and close.
    let title = Rect::from_min_size(Pos2::new(whole.left(), y), Vec2::new(whole.width(), theme::ROW));
    if !state.window {
        let grip = Rect::from_min_max(title.min, pos2(title.right() - 180.0, title.bottom()));
        let drag =
            ui.interact(grip, Id::unique("settings-title-drag"), Sense::drag()).on_hover_cursor(egui::CursorIcon::Grab);
        drag.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Move Settings"));
        outcome.move_by = drag.drag_delta();
    }
    widgets::text_in(&painter, title, Align2::LEFT_CENTER, "Settings", theme::ui_font(theme::TEXT + 2.0), ink().name);
    let close = Rect::from_min_max(Pos2::new(title.right() - 52.0, title.top()), title.max);
    // Help, beside Close: how each setting's description is shown.
    let help = Rect::from_min_size(Pos2::new(close.left() - 58.0, close.top()), Vec2::new(52.0, close.height()));
    let help_response = widgets::button_area(ui, help, Id::unique("settings-help"), "Settings help");
    widgets::flat_button(ui, &painter, help, &help_response, "Help", (theme::TEXT, None));
    if help_response.clicked() {
        state.help_open = true;
    }
    let response = widgets::button_area(ui, close, Id::unique("settings-close"), "Close settings");
    widgets::flat_button(ui, &painter, close, &response, "Close", (theme::TEXT, None));
    // Undock or dock, left of Help.
    let toggle = Rect::from_min_max(pos2(help.left() - 116.0, title.top()), pos2(help.left() - 6.0, title.bottom()));
    let label = if state.window { "Dock" } else { "Open in window" };
    if ui.put(toggle, egui::Button::new(label)).clicked() {
        state.window = !state.window;
    }
    let top = title.bottom() + 16.0;

    // The search field over the list of pages: Escape in it empties it.
    let field = Rect::from_min_size(Pos2::new(whole.left(), top), Vec2::new(SIDEBAR, SEARCH_FIELD));
    let edit = ui.put(
        field,
        egui::TextEdit::singleline(&mut state.search)
            .frame(widgets::input_frame())
            .hint_text("Search settings")
            .font(theme::ui_font(theme::TEXT_SMALL)),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Search settings"));
    widgets::input_line(ui, &edit);
    let escaped = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escaped {
        state.search.clear();
    }
    let searching = !state.search.trim().is_empty();
    // Escape closes the page, unless it is cancelling a key being set or a search, closing Help or
    // a menu, or answering a question of the Themes page (which handles its own Escape).
    let theme_question = state.theme_dialog.is_some() || state.theme_leave.is_some() || state.theme_discard;
    let escape = !escaped
        && state.capture.is_none()
        && !state.help_open
        && !theme_question
        && !egui::Popup::is_any_open(ui.ctx())
        && ui.input(|i| i.key_pressed(egui::Key::Escape));
    // Unsaved changes in the variant editor are asked about first.
    let unsaved = state.theme_editor && sources.themes.edited;
    if response.clicked() || escape {
        if unsaved {
            ask_leave(state, Leave::Close);
        } else {
            outcome.close = true;
            state.audio_pending = None;
        }
    }
    if state.help_open {
        help_dialog(ui, settings, state, &mut outcome);
    }

    // The pages down the left, the one shown beside them; none is lit while searching.
    let sidebar = Rect::from_min_max(
        Pos2::new(whole.left(), field.bottom() + 8.0),
        Pos2::new(whole.left() + SIDEBAR, whole.bottom()),
    );
    if let Some(picked) = page_list(ui, &painter, sidebar, (!searching).then_some(state.page)) {
        if unsaved {
            ask_leave(state, Leave::Page(picked));
        } else {
            open_page(state, picked);
        }
    }

    // The page shown, beside the list: it scrolls (wheel, scroll bar, a control given the focus)
    // when it is taller than the room left under the title.
    let viewport = Rect::from_min_max(
        Pos2::new(whole.left() + SIDEBAR + SIDEBAR_GAP, top),
        Pos2::new((whole.right() + SCROLL_GUTTER).min(area.right()), area.bottom().max(top)),
    );
    let width = whole.right() - viewport.left();
    let shown = (state.page, state.search.trim().to_string());
    let to_top = state.scrolled.as_ref() != Some(&shown);
    state.scrolled = Some(shown);
    ui.scope_builder(egui::UiBuilder::new().max_rect(viewport), |ui| {
        let mut scroll = egui::ScrollArea::vertical().id_salt("settings-page").auto_shrink([false, false]);
        if to_top {
            scroll = scroll.vertical_scroll_offset(0.0);
        }
        scroll.show(ui, |ui| {
            let page = Rect::from_min_size(ui.max_rect().min, Vec2::new(width, viewport.height()));
            let end = page_body(ui, settings, sources, state, &mut outcome, page);
            ui.allocate_rect(Rect::from_min_max(page.min, Pos2::new(page.right(), end + 2.0 * PAD)), Sense::hover());
            keep_focus_in_view(ui, state);
        });
    });
    outcome
}

/// Shows the page `page`, leaving the Themes page's editor and questions.
fn open_page(state: &mut PageState, page: Page) {
    state.page = page;
    state.theme_editor = false;
    state.theme_leave = None;
    state.theme_discard = false;
    state.theme_dialog = None;
    state.skin_editor.forget_drafts();
    state.search.clear();
    state.capture = None;
    state.audio_listed = None;
}

/// Asks, in the variant editor, whether to save its changes before going to `to`.
pub fn ask_leave(state: &mut PageState, to: Leave) {
    state.page = Page::Theme;
    state.theme_editor = true;
    state.theme_leave = Some(to);
    state.theme_discard = false;
    state.search.clear();
    state.scrolled = None;
}

/// The ways a setting's description can be shown, as the Help dialog offers them.
const DESCRIPTION_CHOICES: [(Descriptions, &str); 3] = [
    (Descriptions::Tooltip, "Tooltip: point at a setting's name to read what it does"),
    (Descriptions::Icon, "? icon: point at the ? next to a setting to read what it does"),
    (Descriptions::Shown, "Below the name: each setting's description is always shown"),
];

/// Help over the Settings window: what the names and descriptions are, and how a description is
/// shown, one choice per line. Escape or Close closes it.
fn help_dialog(ui: &Ui, settings: &mut Settings, state: &mut PageState, outcome: &mut Outcome) {
    let modal = crate::kit::floating::modal(ui.ctx(), Id::unique("settings-help-dialog")).show(ui.ctx(), |ui| {
        ui.set_width(360.0);
        ui.label(egui::RichText::new("Help").font(theme::ui_font(theme::TEXT + 2.0)).color(ink().name));
        ui.add_space(6.0);
        let text = "Each setting's name says what it does. Choose how to see the longer description of each setting:";
        ui.label(egui::RichText::new(text).font(theme::ui_font(theme::TEXT_SMALL)).color(ink().caption));
        ui.add_space(6.0);
        for (choice, label) in DESCRIPTION_CHOICES {
            let text = egui::RichText::new(label).font(theme::ui_font(theme::TEXT_SMALL));
            if ui.radio(settings.setting_descriptions == choice, text).clicked()
                && settings.setting_descriptions != choice
            {
                settings.setting_descriptions = choice;
                outcome.changed = true;
            }
        }
        ui.add_space(8.0);
        ui.button(egui::RichText::new("Close").font(theme::ui_font(theme::TEXT))).clicked()
    });
    if modal.inner || modal.should_close() {
        state.help_open = false;
    }
}

/// A control that takes the keyboard focus (Tab) is scrolled into view.
fn keep_focus_in_view(ui: &Ui, state: &mut PageState) {
    let focused = ui.memory(|m| m.focused());
    if focused == state.focused {
        return;
    }
    state.focused = focused;
    if let Some(response) = focused.and_then(|id| ui.ctx().read_response(id))
        && ui.min_rect().intersects(response.rect)
    {
        ui.scroll_to_rect(response.rect, None);
    }
}

/// Draws the page shown, or the settings that match the search, into `page`, its top at the
/// top of what scrolls; returns where it ends.
fn page_body(
    ui: &mut Ui,
    settings: &mut Settings,
    sources: Sources<'_>,
    state: &mut PageState,
    outcome: &mut Outcome,
    page: Rect,
) -> f32 {
    let Sources { about, plugins, themes, keymap, tag_limit } = sources;
    let painter = ui.painter().clone();
    let words = state.words();
    let shown = state.page;
    let mut cx = Cx {
        ui,
        painter: &painter,
        page,
        settings,
        about,
        plugins,
        themes,
        keymap,
        tag_limit,
        state,
        outcome,
        name: "",
        note: None,
        label: None,
    };
    let mut y = page.top();
    let mut heading: Option<(Page, &str)> = None;
    // Size defaults, tuning and the exact-heights switch under Advanced form one interdependent
    // group in search results.
    let sizes = |e: &Entry| {
        e.page == Page::Arrangement && (e.section == "Track sizes" || e.keys == ["arrangement_sizes_advanced"])
    };
    let sizes_match = !words.is_empty() && entries().iter().any(|e| sizes(e) && e.matches(&words));
    for entry in entries() {
        let listed =
            if words.is_empty() { entry.page == shown } else { entry.matches(&words) || (sizes_match && sizes(entry)) };
        if !listed || !(entry.shown)(cx.settings) {
            continue;
        }
        let advanced = entry.section == ADVANCED && words.is_empty();
        let unfolded = cx.state.advanced.contains(&entry.page);
        if advanced && !unfolded && heading == Some((entry.page, ADVANCED)) {
            continue;
        }
        // A section title over a hairline where the section changes; the first sits level with
        // the top of the list, and a search names the page as well.
        if heading != Some((entry.page, entry.section)) {
            if heading.is_some() {
                y += 8.0;
            }
            let title = if words.is_empty() {
                entry.section.to_string()
            } else {
                format!("{} › {}", entry.page.name(), entry.section)
            };
            if advanced {
                // The group is folded until its heading is clicked.
                let rect = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), SIDEBAR_ROW));
                let text = if unfolded { "Hide advanced settings" } else { "Show advanced settings" };
                let response =
                    widgets::button_area(cx.ui, rect, Id::unique(("settings-advanced", entry.page.name())), text);
                let marker = if unfolded { "v  Advanced" } else { ">  Advanced" };
                let color = motion::mix(ink().hint, ink().caption, widgets::hover(cx.ui, &response));
                widgets::text_in(&painter, rect, Align2::LEFT_CENTER, marker, theme::ui_font(theme::TEXT_SMALL), color);
                painter.hline(page.x_range(), rect.bottom() + 3.5, theme::hairline(crate::kit::element::row_line()));
                y = rect.bottom() + 8.0;
                if response.clicked() {
                    if unfolded {
                        cx.state.advanced.retain(|p| *p != entry.page);
                    } else {
                        cx.state.advanced.push(entry.page);
                    }
                }
            } else {
                section_title(&painter, page, &mut y, &title);
            }
            heading = Some((entry.page, entry.section));
            if advanced && !unfolded {
                continue;
            }
        }
        cx.name = entry.name;
        cx.note = None;
        cx.label = None;
        match entry.draw {
            Draw::Row(draw) => {
                let r = setting_row(page, &mut y);
                draw(&mut cx, r);
                let status = cx.note.take();
                let name_rect = cx.label.take().unwrap_or(r);
                let how = cx.settings.setting_descriptions;
                // Shown under the name, the description is the row's second line, with what the
                // setting reports now (the last backup) after it.
                let second = match (how, status) {
                    (Descriptions::Shown, Some(status)) => Some(format!("{}: {status}", entry.note)),
                    (Descriptions::Shown, None) => Some(entry.note.to_string()),
                    (_, status) => status,
                };
                let height = wrapped_label(&painter, r, name_rect, entry.name, second.as_deref());
                describe(cx.ui, &painter, r, entry.name, entry.note, how);
                y = r.top() + height;
            }
            Draw::Block(draw) if entry.role == Role::Status => {
                // A readout in a panel of its own, set off from the settings below.
                let backdrop = painter.add(egui::Shape::Noop);
                let top = y;
                y += 6.0;
                draw(&mut cx, &mut y);
                y += 6.0;
                let panel = Rect::from_min_max(Pos2::new(page.left() - 6.0, top), Pos2::new(page.right(), y));
                painter.set(
                    backdrop,
                    egui::Shape::rect_filled(
                        panel,
                        0.0,
                        crate::kit::style_class::surface_or(crate::kit::element::app_fill("panel")),
                    ),
                );
                y += 4.0;
            }
            Draw::Block(draw) => draw(&mut cx, &mut y),
        }
    }
    if heading.is_none() && !words.is_empty() {
        let r = setting_row(page, &mut y);
        label(&painter, r, "No setting matches", "Try another word, or pick a page on the left");
    }
    y
}

/// The list of pages, `shown` filled. Returns the page clicked, if it is not the one shown.
fn page_list(ui: &Ui, painter: &egui::Painter, rect: Rect, shown: Option<Page>) -> Option<Page> {
    let mut picked = None;
    for (index, (page, name)) in Page::ALL.into_iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + index as f32 * SIDEBAR_ROW),
            Vec2::new(rect.width(), SIDEBAR_ROW),
        );
        let response =
            widgets::button_area(ui, row, Id::unique(("settings-page", index)), format!("Settings page: {name}"));
        if response.clicked() && Some(page) != shown {
            picked = Some(page);
        }
        let selected = motion::toward(ui.ctx(), response.id.with("selected"), Some(page) == shown, motion::VALUE);
        let hover = widgets::hover(ui, &response);
        // `settings-nav`, one row per page: `:hover`, and `[checked]` for the page shown.
        let nav = crate::kit::element::SETTINGS_NAV
            .response(&response)
            .attr(crate::kit::element::Checked, Some(page) == shown)
            .enter();
        let own = nav.style().filter(|c| c.authored.background);
        if let Some(own) = &own {
            painter.rect_filled(row, own.radius, own.background);
        } else {
            painter.rect_filled(row, 0.0, crate::kit::element::app_fill("panel-hover").gamma_multiply(0.25 * hover));
            painter.rect_filled(row, 0.0, crate::kit::element::app_fill("control").gamma_multiply(0.8 * selected));
        }
        // A page under another (Themes under Appearance) is stepped in.
        let indent = if page == Page::Theme { PAD * 2.0 } else { 0.0 };
        let text = Rect::from_min_max(Pos2::new(row.left() + PAD + indent, row.top()), row.max);
        // Its ink, `[checked]` for the page shown; while that changes, a fade between the two.
        let settled = if Some(page) == shown { selected >= 1.0 } else { selected <= 0.0 };
        let color = if settled {
            nav.color()
        } else {
            use crate::kit::element::{Checked, SETTINGS_NAV};
            let (off, on) = (SETTINGS_NAV.probe().color(), SETTINGS_NAV.attr(Checked, true).probe().color());
            motion::mix(off, on, selected)
        };
        widgets::text_in(painter, text, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), color);
    }
    picked
}

/// Appearance › Themes: the themes, the theme in use and its colours.
pub(crate) fn theme_block(cx: &mut Cx<'_>, y: &mut f32) {
    let (request, close) = theme_page(cx.ui, cx.page, y, cx.themes, cx.state);
    cx.outcome.themes = request;
    if close {
        cx.outcome.close = true;
        cx.state.audio_pending = None;
    }
    cx.outcome.adjusting |= cx.state.theme_colour_held;
}

/// Shortcuts › Keys: every action's keys and the preset.
pub(crate) fn shortcuts_block(cx: &mut Cx<'_>, y: &mut f32) {
    let bottom = cx.page.bottom();
    cx.outcome.keymap_changed |= shortcuts_page(cx.ui, cx.painter, cx.page, y, bottom, cx.keymap, cx.state);
}

/// Plug-ins › Library: folders, scanning and what was found.
pub(crate) fn plugin_library(cx: &mut Cx<'_>, y: &mut f32) {
    let bottom = cx.page.bottom();
    if let Some(request) = plugin_folders(cx.ui, cx.painter, cx.page, y, bottom, cx.plugins) {
        cx.outcome.plugins = Some(request);
    }
}

/// Plug-ins › Actions: scanning and the plug-in manager.
pub(crate) fn plugin_buttons(cx: &mut Cx<'_>, y: &mut f32) {
    if let Some(request) = plugin_actions(cx.ui, cx.painter, cx.page, y, cx.plugins) {
        cx.outcome.plugins = Some(request);
    }
}

/// Clip editor › Workflow: the workflow settings (the Mouse-first switches among them), their own
/// rows, kept in the keymap rather than in the settings, so they are saved beside the keys.
pub(crate) fn mouse_switches(cx: &mut Cx<'_>, y: &mut f32) {
    for setting in Setting::ALL {
        let r = setting_row(cx.page, y);
        let options: Vec<(&str, usize)> = setting.short_choices().iter().enumerate().map(|(i, c)| (*c, i)).collect();
        let current = cx.keymap.workflow().get(setting);
        let rect = options_rect(cx.ui, r, &options);
        if let Some(index) = segmented(cx.ui, cx.painter, rect, setting.name(), &options, current) {
            cx.keymap.set_workflow(setting, index);
            cx.outcome.keymap_changed = true;
        }
        let how = cx.settings.setting_descriptions;
        let second = (how == Descriptions::Shown).then(|| setting.note());
        let height = wrapped_label(cx.painter, r, r, setting.name(), second);
        describe(cx.ui, cx.painter, r, setting.name(), setting.note(), how);
        *y = r.top() + height;
    }
    let color = ink().caption;
    let text = "Kept in keymap.json with the keys. Changing any of these makes the workflow Custom.";
    let galley = cx.painter.layout(text.to_string(), theme::ui_font(theme::TEXT_SMALL), color, cx.page.width() - 8.0);
    let height = galley.size().y;
    cx.painter.galley(Pos2::new(cx.page.left(), *y), galley, color);
    *y += height + 8.0;
}

/// MIDI › MIDI inputs: each port with On and Off.
pub(crate) fn midi_inputs(cx: &mut Cx<'_>, y: &mut f32) {
    if let Some(port) = midi_section(cx.ui, cx.painter, cx.page, y, cx.about) {
        cx.outcome.midi_input = Some(port);
    }
}

/// Themes: the gallery, or the variant editor as a child view, or a question or form in place of
/// them. Returns what to do with the themes, and whether Settings closes (an answer to the
/// editor's question about unsaved changes).
fn theme_page(
    ui: &mut Ui,
    page: Rect,
    at: &mut f32,
    themes: &ThemesView,
    state: &mut PageState,
) -> (Option<ThemeRequest>, bool) {
    state.theme_colour_held = false;
    CARDS.set(Some(Cards::read()));
    let _cards = CardsGuard;
    let mut request = None;
    let mut close = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(page.left(), *at), page.max)), |ui| {
        ui.set_width(page.width());
        if state.theme_editor {
            (request, close) = theme_editor(ui, themes, state);
        } else {
            request = theme_gallery(ui, page, themes, state);
        }
        *at = ui.min_rect().bottom();
        if let Some(dialog) = state.theme_dialog.take() {
            let (kept, asked) = theme_dialog(ui, themes, dialog);
            state.theme_dialog = kept;
            request = asked.or(request.take());
        }
    });
    (request, close)
}

/// Why something of a built-in theme cannot be changed, for a greyed item's hover.
const BUILT_IN_REASON: &str = "Built-in themes can't be changed; make a new theme from it";
const SHIPPED_REASON: &str =
    "This variant comes with a built-in theme and can't be changed; duplicate it to make your own";
const DEFAULT_REASON: &str = "The default variant is the theme's own values; it can't be deleted";

/// A menu item or button that is greyed with `reason` on hover when it does not apply.
fn item(ui: &mut Ui, label: &str, reason: Option<&str>) -> bool {
    let response = ui.add_enabled(reason.is_none(), egui::Button::new(label));
    match reason {
        Some(reason) => {
            response.on_disabled_hover_text(reason);
            false
        }
        None => response.clicked(),
    }
}

/// The name of the variant `id`: its own, or for a theme's own values its default variant's.
fn variant_label<'a>(themes: &'a ThemesView, id: &str) -> &'a str {
    match themes.variants.iter().find(|(variant, _)| variant == id) {
        Some((_, name)) => name,
        None => themes.info.iter().find(|(theme, _)| theme == id).map_or("Default", |(_, i)| &i.default_variant),
    }
}

/// Why the variant `id` cannot be deleted, if it cannot.
fn delete_refused(themes: &ThemesView, id: &str) -> Option<&'static str> {
    if crate::kit::theme_vars::split_id(id).1.is_none() {
        Some(DEFAULT_REASON)
    } else if themes.locked.iter().any(|locked| locked == id) {
        Some(SHIPPED_REASON)
    } else {
        None
    }
}

/// The question that deletes the variant `id` (or reverts the user's copy of a built-in one).
fn delete_variant(themes: &ThemesView, id: &str) -> ThemeDialog {
    ThemeDialog::Delete {
        id: id.to_owned(),
        name: variant_label(themes, id).to_owned(),
        revert: themes.overridden.iter().any(|o| o == id),
    }
}

/// *New theme…*, starting from `from`.
fn new_theme(from: Option<String>) -> ThemeDialog {
    ThemeDialog::NewTheme(NewTheme { from, ..Default::default() })
}

/// The gallery: the theme in use with its variants, Edit current variant and More, then every
/// theme in a grid with a search.
fn theme_gallery(ui: &mut Ui, page: Rect, themes: &ThemesView, state: &mut PageState) -> Option<ThemeRequest> {
    let mut request = None;
    let current = crate::kit::theme_vars::split_id(&themes.current).0.to_owned();
    ui.horizontal(|ui| {
        if ui.button("Edit current variant").clicked() {
            state.theme_editor = true;
            state.scrolled = None;
        }
        ui.menu_button("More", |ui| {
            if ui.button("New theme…").clicked() {
                state.theme_dialog = Some(new_theme(None));
                ui.close();
            }
            for (label, action) in [
                ("Import theme…", ThemeRequest::Import),
                ("Export theme…", ThemeRequest::ExportPackage(current.clone())),
                ("Open the themes folder", ThemeRequest::OpenFolder),
                ("Back up themes", ThemeRequest::Backup),
            ] {
                if ui.button(label).clicked() {
                    request = Some(action);
                    ui.close();
                }
            }
            ui.separator();
            let refused = delete_refused(themes, &themes.current);
            let label = egui::RichText::new("Delete variant…").color(if refused.is_none() {
                crate::kit::element::error_ink()
            } else {
                ink().hint
            });
            let response = ui.add_enabled(refused.is_none(), egui::Button::new(label));
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, refused.is_none(), "Delete variant…"));
            if let Some(reason) = refused {
                response.on_disabled_hover_text(reason);
            } else if response.clicked() {
                state.theme_dialog = Some(delete_variant(themes, &themes.current));
                ui.close();
            }
        });
    });
    ui.add_space(10.0);
    let gap = 10.0;
    let width = ((page.width() - gap) / 2.0).max(80.0);
    if let Some((_, name, built_in)) = themes.list.iter().find(|(id, _, _)| *id == current) {
        let (active, asked) = theme_card(ui, themes, state, &themes.current, name, *built_in, page.width(), true);
        state.theme_colour_held |= active.is_pointer_button_down_on() && ui.input(|input| input.pointer.primary_down());
        request = asked.or(request);
        ui.add_space(gap);
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("All themes").font(theme::ui_font(theme::TEXT)).color(ink().name));
        let search = ui.add(
            egui::TextEdit::singleline(&mut state.theme_search)
                .hint_text("Search themes")
                .frame(widgets::input_frame())
                .desired_width(200.0),
        );
        search.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Search themes"));
        widgets::input_line(ui, &search);
    });
    let query = state.theme_search.trim().to_lowercase();
    // Every theme stays in the list, the one in use too (marked), so choosing one leaves the list as
    // it is and the page where it was; a search shows only what matches.
    let mut others =
        themes.list.iter().filter(|(id, name, _)| theme_matches(&query, id, name, &themes.variants)).peekable();
    if others.peek().is_none() && !query.is_empty() {
        ui.label(egui::RichText::new("No themes match").color(ink().hint));
    }
    while let Some(first) = others.next() {
        let second = others.next();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (id, name, built_in) in std::iter::once(first).chain(second) {
                let (_, asked) = theme_card(ui, themes, state, id, name, *built_in, width, false);
                request = asked.or(request.take());
            }
        });
        ui.add_space(gap);
    }
    request
}

/// A question or form in a box over Settings. Returns it while it stays, and what its answer
/// asks for; Escape or a click beside the box cancels it.
fn theme_dialog(ui: &Ui, themes: &ThemesView, mut dialog: ThemeDialog) -> (Option<ThemeDialog>, Option<ThemeRequest>) {
    let mut request = None;
    let mut done = false;
    let cancelled = theme_modal(ui, "theme-dialog", |ui| match &mut dialog {
        ThemeDialog::NewTheme(new) => {
            dialog_title(ui, "New theme");
            let start = new.from.as_deref().map_or("Suhina's defaults".to_owned(), |from| {
                let base = crate::kit::theme_vars::split_id(from).0;
                let name = themes.list.iter().find(|(id, _, _)| id == base).map_or(base, |(_, name, _)| name);
                match crate::kit::theme_vars::split_id(from).1 {
                    Some(_) => format!("{name} · {}", variant_label(themes, from)),
                    None => name.to_owned(),
                }
            });
            ui.horizontal(|ui| {
                ui.add_sized([140.0, theme::ROW], egui::Label::new("Start from"));
                let label = format!("Start from {start}");
                let combo = egui::ComboBox::from_id_salt("new-theme-from").selected_text(start).width(260.0);
                let response = combo.show_ui(ui, |ui| {
                    ui.selectable_value(&mut new.from, None, "Suhina's defaults");
                    for (id, name, _) in &themes.list {
                        ui.selectable_value(&mut new.from, Some(id.clone()), name);
                    }
                });
                response.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, &label));
            });
            dialog_field(ui, "Name", "Theme name", &mut new.info.name);
            dialog_field(ui, "Author", "Theme author", &mut new.info.author);
            dialog_field(ui, "Description", "Theme description", &mut new.info.description);
            ui.checkbox(&mut new.artwork, "Include the default artwork");
            ui.label(
                egui::RichText::new(
                    "Copies the built-in control images into the theme's assets folder, with a rule for each in its \
                     theme.css, so you can edit them.",
                )
                .small()
                .color(ink().caption),
            );
            ui.horizontal(|ui| {
                if ui.button("Create theme").clicked() {
                    request = Some(ThemeRequest::NewTheme(new.clone()));
                    done = true;
                }
                done |= ui.button("Cancel").clicked();
            });
        }
        ThemeDialog::Info { id, info } => {
            dialog_title(ui, "Theme info");
            dialog_field(ui, "Name", "Theme name", &mut info.name);
            dialog_field(ui, "Author", "Theme author", &mut info.author);
            dialog_field(ui, "Description", "Theme description", &mut info.description);
            dialog_field(ui, "Default variant", "Default variant name", &mut info.default_variant);
            ui.horizontal(|ui| {
                if ui.add_enabled(!info.name.trim().is_empty(), egui::Button::new("Save")).clicked() {
                    request = Some(ThemeRequest::EditInfo { id: id.clone(), info: info.clone() });
                    done = true;
                }
                done |= ui.button("Cancel").clicked();
            });
        }
        ThemeDialog::Delete { id, name, revert } => {
            let theme = crate::kit::theme_vars::split_id(id).1.is_none();
            let (question, what, action) = match (*revert, theme) {
                (true, _) => (format!("Revert {name} to the built-in variant?"), "Your copy of it", "Revert"),
                (false, true) => (format!("Delete the theme {name}?"), "Its folder", "Delete"),
                (false, false) => (format!("Delete {name}?"), "Its file", "Delete"),
            };
            dialog_title(ui, &question);
            ui.label(format!("{what} will be moved to the Trash."));
            ui.horizontal(|ui| {
                let button = egui::Button::new(egui::RichText::new(action).color(crate::kit::element::error_ink()));
                if ui.add(button).clicked() {
                    request = Some(ThemeRequest::Delete(id.clone()));
                    done = true;
                }
                done |= ui.button("Cancel").clicked();
            });
        }
    })
    .1;
    done |= cancelled;
    ((!done).then_some(dialog), request)
}

/// A box over Settings with the window dimmed behind it, for a question or form of the Themes
/// page. Returns what `add` returns, and whether it was cancelled (Escape, or a click beside it).
fn theme_modal<R>(ui: &Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let modal = crate::kit::floating::modal(ui.ctx(), Id::unique(("theme-modal", id))).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        add(ui)
    });
    let cancelled = modal.should_close();
    (modal.inner, cancelled)
}

fn dialog_title(ui: &mut Ui, title: &str) {
    ui.label(egui::RichText::new(title).font(theme::ui_font(theme::TEXT + 1.0)).color(ink().name));
}

/// A labelled one-line field of a form: `shown` beside it, `label` for screen readers and tests.
fn dialog_field(ui: &mut Ui, shown: &str, label: &str, text: &mut String) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_sized([140.0, theme::ROW], egui::Label::new(shown));
        let edit = ui.add(egui::TextEdit::singleline(text).frame(widgets::input_frame()).desired_width(260.0));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
        widgets::input_line(ui, &edit);
        edit
    })
    .inner
}

/// A variant's swatch on the large card and on a card of the grid, the space between swatches,
/// and the margin around their row.
const THEME_SWATCH_LARGE: Vec2 = Vec2::new(72.0, 36.0);
const THEME_SWATCH_SMALL: Vec2 = Vec2::new(40.0, 22.0);
const THEME_SWATCH_GAP: f32 = 6.0;
const THEME_SWATCH_MARGIN: f32 = 8.0;

/// Whether a theme shows for `query` (lower case): an empty query, or part of its name, its id or
/// the name of one of its variants.
fn theme_matches(query: &str, id: &str, name: &str, variants: &[(String, String)]) -> bool {
    query.is_empty()
        || name.to_lowercase().contains(query)
        || id.to_lowercase().contains(query)
        || variants.iter().any(|(entry, variant)| {
            crate::kit::theme_vars::split_id(entry).0 == id && variant.to_lowercase().contains(query)
        })
}

/// A theme's card. The theme in use is featured once, large, with its name as a heading and its
/// variant's name under it, and a swatch per variant followed by + (a new variant); every theme
/// has a card in the grid, with a row of swatches when it has variants (a card without keeps the
/// row's room, so all the cards of the grid are one height). Clicking a card or a swatch uses it;
/// right-clicking opens what can be done to it. Returns the card's response and what was asked.
#[allow(clippy::too_many_arguments)]
fn theme_card(
    ui: &mut Ui,
    themes: &ThemesView,
    state: &mut PageState,
    id: &str,
    name: &str,
    built_in: bool,
    width: f32,
    active: bool,
) -> (egui::Response, Option<ThemeRequest>) {
    let base = crate::kit::theme_vars::split_id(id).0;
    let variants: Vec<(&str, &str)> = std::iter::once((base, variant_label(themes, base)))
        .chain(
            themes
                .variants
                .iter()
                .filter(|(variant, _)| crate::kit::theme_vars::split_id(variant).0 == base)
                .map(|(variant, label)| (variant.as_str(), label.as_str())),
        )
        .collect();
    let swatch_size = if active { THEME_SWATCH_LARGE } else { THEME_SWATCH_SMALL };
    let swatches = swatch_size.y + THEME_SWATCH_MARGIN;
    // A thumbnail is 16:9 on every card and at most 170 points high, so the cards of a wide page
    // are not taller than the one in use.
    let preview_width = (width - 10.0).max(16.0);
    let preview_height = (preview_width * 9.0 / 16.0).min(170.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, preview_height + 50.0 + swatches), Sense::hover());
    let card = Rect::from_min_size(rect.min, Vec2::new(width, preview_height + 50.0));
    let response = ui.interact(
        card,
        Id::unique(("theme", base, active)),
        if active { Sense::click_and_drag() } else { Sense::click() },
    );
    // The one in use shows twice: large at the top, and in its place in the list.
    let label = if active { format!("Theme in use: {name}") } else { format!("Theme: {name}") };
    let in_use = base == crate::kit::theme_vars::split_id(&themes.current).0;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, in_use, label.clone()));
    let mut asked = None;
    let kept = themes.kept.iter().find(|(kept, _)| kept == base).map(|(_, why)| why.as_str());
    let notices = themes.notices.iter().find(|(theme, _)| theme == base).map(|(_, notices)| notices);
    // Two themes with one name: the folder tells them apart.
    let twin = themes.list.iter().any(|(other, other_name, _)| other != base && other_name.eq_ignore_ascii_case(name));
    let name = if twin { format!("{name} ({base})") } else { name.to_string() };
    let cards = CARDS.get().unwrap_or_else(Cards::read);
    let name_rect =
        Rect::from_min_size(pos2(rect.left() + 8.0, rect.top() + preview_height + 9.0), Vec2::new(width - 16.0, 18.0));
    // The warning badge of a theme whose sheet has notices, at the end of its name's line.
    let badge = Rect::from_center_size(pos2(name_rect.right() - 8.0, name_rect.center().y), Vec2::splat(16.0));
    if ui.is_rect_visible(rect) {
        // The theme in use stands out, on the large card and in its place in the list.
        if in_use {
            ui.painter().rect_filled(rect, 3.0, cards.tint);
        }
        let swatch = swatch_of(themes, id);
        let preview = Rect::from_min_size(
            rect.min + Vec2::new((width - preview_height * 16.0 / 9.0) / 2.0, 5.0),
            Vec2::new(preview_height * 16.0 / 9.0, preview_height),
        );
        let picture = themes.pictures.iter().find(|(key, _)| key == id).map(|(_, svg)| svg);
        if !picture.is_some_and(|svg| crate::kit::skin::paint_svg(ui.painter(), preview, svg, 2.0)) {
            // A theme without a thumbnail: a blank card in its swatch colour.
            ui.painter().rect_filled(preview, 2.0, swatch);
        }
        let border = if in_use {
            egui::Stroke::new(2.0, cards.in_use)
        } else if response.hovered() {
            theme::hairline(cards.hovered)
        } else {
            theme::hairline(cards.plain)
        };
        ui.painter().rect_stroke(rect, 3.0, border, egui::StrokeKind::Inside);
        // The theme in use: its name as a heading (the page-title size), its variant's under it.
        let (title, size) = if active { (theme::TEXT + 1.0, theme::TEXT) } else { (theme::TEXT, theme::TEXT_SMALL) };
        widgets::text_in(ui.painter(), name_rect, Align2::LEFT_CENTER, &name, theme::ui_font(title), ink().name);
        let caption = if active {
            themes.variant_name.as_str()
        } else if kept.is_some() {
            "Not converted"
        } else if built_in {
            "Built-in"
        } else {
            "Custom"
        };
        widgets::text_in(
            ui.painter(),
            name_rect.translate(Vec2::new(0.0, 18.0)),
            Align2::LEFT_CENTER,
            caption,
            theme::ui_font(size),
            ink().caption,
        );
        if notices.is_some() {
            let warning = crate::kit::element::error_ink();
            ui.painter().circle_stroke(badge.center(), 7.0, egui::Stroke::new(1.5, warning));
            widgets::text_in(
                ui.painter(),
                badge,
                Align2::CENTER_CENTER,
                "!",
                theme::ui_font(theme::TEXT_SMALL),
                warning,
            );
        }
    }
    if let Some(notices) = notices {
        let count = notices.len();
        let badge = ui.interact(badge, Id::unique(("theme-notices", base, active)), Sense::hover());
        let label = if count == 1 { "1 style sheet notice".to_owned() } else { format!("{count} style sheet notices") };
        badge.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, label.clone()));
        badge.on_hover_text(format!("{label}; the rest of the sheet still applies:\n{}", notices.join("\n")));
    }
    let response = match kept {
        Some(why) => response.on_hover_text(why),
        None => response,
    };
    if response.clicked() {
        asked = Some(ThemeRequest::Use(base.to_owned()));
    }
    response.context_menu(|ui| {
        // The large card is the theme in use already.
        if !active && ui.button("Use").clicked() {
            asked = Some(ThemeRequest::Use(base.to_owned()));
            ui.close();
        }
        if ui.button("New theme from this…").clicked() {
            let from = if in_use { themes.current.clone() } else { base.to_owned() };
            state.theme_dialog = Some(new_theme(Some(from)));
            ui.close();
        }
        let refused = built_in.then_some(BUILT_IN_REASON);
        if item(ui, "Theme info…", refused) {
            let info = themes.info.iter().find(|(theme, _)| theme == base).map(|(_, i)| i.clone()).unwrap_or_default();
            state.theme_dialog = Some(ThemeDialog::Info { id: base.to_owned(), info });
            ui.close();
        }
        if ui.button("Export theme…").clicked() {
            asked = Some(ThemeRequest::ExportPackage(base.to_owned()));
            ui.close();
        }
        if item(
            ui,
            "Show in folder",
            built_in.then_some("Built-in themes are inside Suhina; Export theme… writes one's folder"),
        ) {
            asked = Some(ThemeRequest::Reveal(base.to_owned()));
            ui.close();
        }
        if item(ui, "Add default artwork", refused) {
            asked = Some(ThemeRequest::AddDefaultArtwork(base.to_owned()));
            ui.close();
        }
        ui.separator();
        if item(ui, "Delete theme…", refused) {
            state.theme_dialog = Some(ThemeDialog::Delete { id: base.to_owned(), name: name.clone(), revert: false });
            ui.close();
        }
    });
    // The large card always shows its row, for its +; a card of the grid only for variants.
    if active || variants.len() > 1 {
        // All the swatches stay on the row: when they do not fit, they and the spaces between
        // them narrow together.
        let count = variants.len() as f32 + if active { 1.0 } else { 0.0 };
        let wanted = count * swatch_size.x + (count - 1.0) * THEME_SWATCH_GAP;
        let scale = ((width - 2.0 * THEME_SWATCH_MARGIN).max(0.0) / wanted).min(1.0);
        let size = Vec2::new(swatch_size.x * scale, swatch_size.y);
        let gap = THEME_SWATCH_GAP * scale;
        let place = |index: usize| {
            let left = rect.left() + THEME_SWATCH_MARGIN + index as f32 * (size.x + gap);
            Rect::from_min_size(pos2(left, rect.top() + preview_height + 50.0), size)
        };
        for (index, (variant, variant_name)) in variants.iter().enumerate() {
            let area = place(index);
            // The theme in use shows its swatches twice (its large card and its card in the
            // list), so each card's swatch has an id of its own.
            let swatch = ui.interact(area, Id::unique(("theme-variant", *variant, active)), Sense::click());
            swatch.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Variant: {variant_name}"))
            });
            let swatch = swatch.on_hover_text(*variant_name);
            if swatch.clicked() {
                asked = Some(ThemeRequest::Use((*variant).to_string()));
            }
            swatch.context_menu(|ui| {
                if ui.button("Use").clicked() {
                    asked = Some(ThemeRequest::Use((*variant).to_string()));
                    ui.close();
                }
                if ui.button("Edit…").clicked() {
                    asked = Some(ThemeRequest::Use((*variant).to_string()));
                    state.theme_editor = true;
                    state.scrolled = None;
                    ui.close();
                }
                if ui.button("Duplicate").clicked() {
                    asked = Some(ThemeRequest::DuplicateVariant((*variant).to_string()));
                    ui.close();
                }
                ui.separator();
                let revert = themes.overridden.iter().any(|o| o == variant);
                let label = if revert { "Revert to built-in…" } else { "Delete…" };
                if item(ui, label, delete_refused(themes, variant)) {
                    state.theme_dialog = Some(delete_variant(themes, variant));
                    ui.close();
                }
            });
            if ui.is_rect_visible(area) {
                let painter = ui.painter();
                let radius = paint_swatch(painter, area, swatch_of(themes, variant));
                let selected = *variant == themes.current;
                let ring = if selected {
                    cards.in_use
                } else if swatch.hovered() {
                    cards.hovered
                } else {
                    cards.plain
                };
                painter.rect_stroke(
                    area,
                    radius,
                    egui::Stroke::new(if selected { 2.0 } else { 1.0 }, ring),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if active {
            // + adds a variant: greyed when the theme has no settings a variant could hold.
            let area = place(variants.len());
            let enabled = !themes.declared.is_empty();
            let plus = ui.interact(area, Id::unique(("theme-new-variant", base)), Sense::click());
            plus.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, "New variant"));
            let plus = if enabled {
                plus.on_hover_text("New variant from the values in use")
            } else {
                plus.on_hover_text("This theme has no settings a variant could change")
            };
            if enabled && plus.clicked() {
                asked = Some(ThemeRequest::NewVariant(themes.current.clone()));
                state.theme_editor = true;
                state.scrolled = None;
            }
            if ui.is_rect_visible(area) {
                // Drawn as the theme's buttons are: their fill, edge and text, lighter under the
                // pointer and pressed, faded when greyed.
                let visuals = if enabled { *ui.style().interact(&plus) } else { ui.visuals().widgets.inactive };
                let fade = |color: egui::Color32| if enabled { color } else { ui.visuals().disable(color) };
                let corner = 6.0_f32.min(area.height() / 2.0);
                let painter = ui.painter();
                painter.rect_filled(area, corner, fade(visuals.weak_bg_fill));
                painter.rect_stroke(
                    area,
                    corner,
                    egui::Stroke::new(visuals.bg_stroke.width.max(1.0), fade(visuals.bg_stroke.color)),
                    egui::StrokeKind::Inside,
                );
                let text = fade(visuals.fg_stroke.color);
                widgets::text_in(painter, area, Align2::CENTER_CENTER, "+", theme::ui_font(theme::TEXT + 4.0), text);
            }
        }
    }
    (response, asked)
}

/// The swatch colour of the theme or variant `id`, or the panels' colour when it has none.
fn swatch_of(themes: &ThemesView, id: &str) -> egui::Color32 {
    themes
        .swatches
        .iter()
        .find(|(key, _)| key == id)
        .map_or_else(|| theme::color_in(&themes.colors, "panel-background"), |(_, color)| *color)
}

/// A swatch of `color`: a `theme-swatch` element with that `color` attribute, as the theme in use
/// styles it, or a rounded rectangle of the colour without a theme sheet. Returns its corner
/// radius.
fn paint_swatch(painter: &egui::Painter, rect: Rect, color: egui::Color32) -> f32 {
    let radius = |r: f32| r.clamp(0.0, rect.width().min(rect.height()) / 2.0);
    match crate::kit::native_css::theme_swatch(color) {
        Some(style) => {
            let corner = radius(style.radius);
            painter.rect_filled(rect, corner, style.background);
            // Each side at its own width.
            crate::kit::native_css::border(painter, rect, corner, &style);
            corner
        }
        None => {
            let corner = radius(6.0);
            painter.rect_filled(rect, corner, color);
            corner
        }
    }
}

/// The variant editor: the variant's name and the theme's quick controls, edited in a draft the
/// app draws with at once and writes only on Save or Save as new variant. Leaving it with unsaved
/// changes asks first, in place. Returns what to do with the themes, and whether Settings closes.
fn theme_editor(ui: &mut Ui, themes: &ThemesView, state: &mut PageState) -> (Option<ThemeRequest>, bool) {
    let mut request = None;
    let mut close = false;
    if ui.button("Back to themes").clicked() {
        if themes.edited {
            state.theme_leave = Some(Leave::Back);
        } else {
            state.theme_editor = false;
            state.scrolled = None;
            state.skin_editor.forget_drafts();
            return (None, close);
        }
    }
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} · {}", themes.name, themes.variant_name))
                .font(theme::ui_font(theme::TEXT + 1.0))
                .color(ink().name),
        );
        if themes.edited && !themes.new_variant {
            ui.label(egui::RichText::new("Edited").color(ink().caption));
        }
    });
    ui.horizontal(|ui| {
        if themes.new_variant {
            // Nothing is written yet: Save makes the variant, Cancel leaves nothing behind.
            if ui.button("Save").clicked() {
                request = Some(ThemeRequest::SaveAsNew);
                state.skin_editor.forget_drafts();
            }
            if ui.button("Cancel").clicked() {
                request = Some(ThemeRequest::Discard);
                state.skin_editor.forget_drafts();
                state.theme_editor = false;
                state.scrolled = None;
            }
            return;
        }
        let refused = (!themes.saveable).then_some(if themes.built_in && themes.variant.is_some() {
            SHIPPED_REASON
        } else {
            "The default variant is the theme's own values; save your changes as a new variant"
        });
        let save = ui.add_enabled(themes.saveable && themes.edited, egui::Button::new("Save"));
        if let Some(reason) = refused {
            save.on_disabled_hover_text(reason);
        } else if save.clicked() {
            request = Some(ThemeRequest::Save);
        }
        if ui.button("Save as new variant").clicked() {
            request = Some(ThemeRequest::SaveAsNew);
            state.skin_editor.forget_drafts();
        }
        if ui.add_enabled(themes.edited, egui::Button::new("Discard changes")).clicked() {
            state.theme_discard = true;
        }
    });
    ui.add_space(8.0);
    let mut name = themes.variant_name.clone();
    if dialog_field(ui, "Name", "Variant name", &mut name).changed() {
        request = Some(ThemeRequest::EditName(name));
    }
    ui.add_space(8.0);
    // A variant holds values for the quick controls its theme declares, and the editor shows only
    // those: the theme's `@property` rules with a label.
    if themes.declared.is_empty() {
        ui.label(
            egui::RichText::new(
                "This theme offers no quick controls. A theme offers them in its theme.css with @property and \
                 a label.",
            )
            .color(ink().caption),
        );
    }
    if let Some(edit) = super::skin_editor::declared(ui, themes, &mut state.skin_editor, &mut state.theme_colour_held) {
        request = Some(edit);
    }
    if let Some(to) = state.theme_leave {
        if themes.edited {
            let (answer, cancelled) = theme_modal(ui, "theme-leave", |ui| {
                let question = if themes.new_variant {
                    format!("Save the new variant {}?", themes.variant_name)
                } else {
                    format!("Save changes to {}?", themes.variant_name)
                };
                dialog_title(ui, &question);
                ui.label("Your changes are shown but not saved yet.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (label, save) = if themes.saveable && !themes.new_variant {
                        ("Save", ThemeRequest::Save)
                    } else if themes.new_variant {
                        ("Save", ThemeRequest::SaveAsNew)
                    } else {
                        ("Save as new variant", ThemeRequest::SaveAsNew)
                    };
                    if ui.button(label).clicked() {
                        Some(Some(save))
                    } else if ui.button("Don't save").clicked() {
                        Some(Some(ThemeRequest::Discard))
                    } else if ui.button("Cancel").clicked() {
                        Some(None)
                    } else {
                        None
                    }
                })
                .inner
            });
            match (answer, cancelled) {
                (Some(Some(answer)), _) => {
                    request = Some(answer);
                    state.theme_leave = None;
                    close = leave(state, to);
                }
                (Some(None), _) | (None, true) => state.theme_leave = None,
                (None, false) => {}
            }
        } else {
            // Nothing to ask about any more (saved, or the variant went).
            state.theme_leave = None;
            close = leave(state, to);
        }
    }
    if state.theme_discard {
        let (answer, cancelled) = theme_modal(ui, "theme-discard", |ui| {
            dialog_title(ui, &format!("Discard your changes to {}?", themes.variant_name));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let button = egui::Button::new(egui::RichText::new("Discard").color(crate::kit::element::error_ink()));
                if ui.add(button).clicked() {
                    Some(true)
                } else if ui.button("Cancel").clicked() {
                    Some(false)
                } else {
                    None
                }
            })
            .inner
        });
        if answer == Some(true) {
            request = Some(ThemeRequest::Discard);
            state.skin_editor.forget_drafts();
        }
        if answer.is_some() || cancelled {
            state.theme_discard = false;
        }
    }
    (request, close)
}

/// Goes where the user was going when the editor asked about its changes. Returns whether
/// Settings closes.
fn leave(state: &mut PageState, to: Leave) -> bool {
    state.skin_editor.forget_drafts();
    match to {
        Leave::Back => {
            state.theme_editor = false;
            state.scrolled = None;
            false
        }
        Leave::Page(page) => {
            open_page(state, page);
            false
        }
        Leave::Close => {
            state.theme_editor = false;
            true
        }
    }
}

/// Every action with its keys: Change takes the next key pressed, Clear unbinds, Reset all
/// goes back to the defaults. Returns whether the keymap changed.
fn shortcuts_page(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    at: &mut f32,
    bottom: f32,
    keymap: &mut Keymap,
    state: &mut PageState,
) -> bool {
    let mut changed = false;
    let small = theme::ui_font(theme::TEXT_SMALL);

    // A key being set: the next key pressed (with its modifiers) is the action's key. Shift,
    // Ctrl and Alt going down are pressed keys of their own: the capture waits for the key they
    // are held with.
    if let Some(action) = state.capture.clone() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. }
                    if !crate::settings::keymap::is_modifier(*key) =>
                {
                    Some(Shortcut::new(*modifiers, *key))
                }
                _ => None,
            })
        });
        match pressed {
            Some(s) if s.key == egui::Key::Escape && s.modifiers.is_none() => state.capture = None,
            Some(s) => {
                // Kept portable: Ctrl on Windows and Linux, Cmd on macOS, both written "Ctrl".
                let mut modifiers = egui::Modifiers::NONE;
                if s.modifiers.command || s.modifiers.ctrl || s.modifiers.mac_cmd {
                    modifiers = modifiers.plus(egui::Modifiers::COMMAND);
                }
                if s.modifiers.shift {
                    modifiers = modifiers.plus(egui::Modifiers::SHIFT);
                }
                if s.modifiers.alt {
                    modifiers = modifiers.plus(egui::Modifiers::ALT);
                }
                let shortcut = Shortcut::new(modifiers, s.key);
                keymap.set(&action, vec![shortcut]);
                state.capture = None;
                changed = true;
            }
            None => {}
        }
    }

    let head = Rect::from_min_size(Pos2::new(page.left(), *at), Vec2::new(page.width(), SETTING_ROW));
    label(painter, head, "Shortcuts", "Change, then press the keys; Escape cancels. Kept in keymap.json.");
    // The page's own filter, left of Reset all: by action, group or key ("zoom", "ctrl+alt").
    let filter = Rect::from_min_max(
        Pos2::new(head.right() - 80.0 - 8.0 - 180.0, head.top() + 1.0),
        Pos2::new(head.right() - 80.0 - 8.0, head.top() + 1.0 + SEGMENT),
    );
    let edit = ui.put(
        filter,
        egui::TextEdit::singleline(&mut state.key_filter)
            .frame(widgets::input_frame())
            .hint_text("Filter shortcuts")
            .font(theme::ui_font(theme::TEXT_SMALL)),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Filter shortcuts"));
    widgets::input_line(ui, &edit);
    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.key_filter.clear();
    }
    // The settings search narrows the list too, so a search shows the actions it found.
    let words: Vec<String> =
        state.key_filter.split_whitespace().chain(state.search.split_whitespace()).map(str::to_lowercase).collect();
    let reset = Rect::from_min_max(
        Pos2::new(head.right() - 80.0, head.top() + 1.0),
        Pos2::new(head.right(), head.top() + 1.0 + SEGMENT),
    );
    let response = widgets::button_area(ui, reset, Id::unique("reset-keys"), "Reset all keys");
    widgets::flat_button(ui, painter, reset, &response, "Reset all", (theme::TEXT_SMALL, None));
    if response.clicked() {
        *keymap = Keymap::default();
        state.capture = None;
        changed = true;
    }

    // The workflow mode: three presets, one for drawing notes first, one for selecting first and
    // one for working with the mouse, and Custom. A preset sets the keys and the workflow settings
    // it names; single keys can still be changed below.
    let mut top = head.bottom() + 4.0;
    let preset_row = Rect::from_min_size(Pos2::new(page.left(), top), Vec2::new(page.width(), 24.0));
    let preset_label = Rect::from_min_size(preset_row.min, Vec2::new(LABEL_WIDTH, 20.0));
    widgets::text_in(painter, preset_label, Align2::LEFT_CENTER, "Preset", theme::ui_font(theme::TEXT), ink().name);
    let current = keymap.preset();
    // 64 px a part, as the measurements give every segmented control in Settings.
    let control = Rect::from_min_size(
        Pos2::new(preset_row.left() + LABEL_WIDTH, preset_row.top()),
        Vec2::new(64.0 * Preset::MODES.len() as f32, SEGMENT),
    );
    let options: Vec<(&str, Preset)> = Preset::MODES.iter().map(|p| ((*p).name(), *p)).collect();
    if let Some(picked) = preset_segmented(ui, painter, control, &options, current) {
        keymap.set_preset(picked);
        state.capture = None;
        changed = true;
    }
    let note = Rect::from_min_size(
        Pos2::new(preset_row.left(), preset_row.top() + SEGMENT + 3.0),
        Vec2::new(page.width(), 16.0),
    );
    widgets::text_in(painter, note, Align2::LEFT_CENTER, current.note(), small.clone(), ink().caption);
    top += 30.0;
    for (key, ids) in keymap.conflicts() {
        let r = Rect::from_min_size(Pos2::new(page.left(), top), Vec2::new(page.width(), 16.0));
        let text = format!("{} runs {} at once", key.display(), ids.join(" and "));
        widgets::text_in(painter, r, Align2::LEFT_CENTER, &text, small.clone(), ink().error);
        top += 16.0;
    }

    let list = Rect::from_min_max(
        Pos2::new(page.left(), top + 4.0),
        Pos2::new(page.right(), (bottom - 12.0).max(top + 4.0 + LIST_LEAST)),
    );
    *at = list.bottom();
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let mut group = "";
            let mut listed = 0;
            for action in actions() {
                // As shown and as written ("⌘⌥↑" and "Ctrl+Alt+Up" on macOS), so either finds it.
                let keys_text: Vec<String> =
                    keymap.keys(&action.id).iter().map(|s| format!("{} {}", s.display(), s.text())).collect();
                if !action_matches(&action.label, &action.id, action.group, &keys_text.join(" "), &words) {
                    continue;
                }
                listed += 1;
                if action.group != group {
                    group = action.group;
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(group).font(small.clone()).color(ink().hint));
                }
                ui.horizontal(|ui| {
                    let keys: Vec<String> = keymap.keys(&action.id).iter().map(Shortcut::display).collect();
                    let capturing = state.capture.as_deref() == Some(action.id.as_str());
                    let left = egui::Layout::left_to_right(egui::Align::Center);
                    ui.allocate_ui_with_layout(Vec2::new(300.0, 18.0), left, |ui| {
                        ui.set_min_width(300.0);
                        let text = egui::RichText::new(&action.label).font(small.clone()).color(ink().name);
                        ui.add(egui::Label::new(text).truncate());
                    });
                    let shown = if capturing {
                        "Press keys".to_string()
                    } else if keys.is_empty() {
                        "-".into()
                    } else {
                        keys.join(", ")
                    };
                    let color = if capturing { crate::kit::element::accent_ink() } else { ink().value };
                    ui.allocate_ui_with_layout(Vec2::new(130.0, 18.0), left, |ui| {
                        ui.set_min_width(130.0);
                        let text = egui::RichText::new(shown).font(theme::mono_font(theme::TEXT_SMALL)).color(color);
                        ui.add(egui::Label::new(text).truncate());
                    });
                    let change = ui.add(egui::Button::new(egui::RichText::new("Change").font(small.clone())));
                    let label = format!("Change keys of {}", action.id);
                    change.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if change.clicked() {
                        state.capture = Some(action.id.clone());
                    }
                    let clear = ui.add(egui::Button::new(egui::RichText::new("Clear").font(small.clone())));
                    let label = format!("Clear keys of {}", action.id);
                    clear.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                    if clear.clicked() && !keys.is_empty() {
                        keymap.set(&action.id, Vec::new());
                        changed = true;
                    }
                });
            }
            if listed == 0 {
                ui.label(egui::RichText::new("No action matches the filter").font(small.clone()).color(ink().hint));
            }
        });
    });
    changed
}

/// Every word is in the action's label, id, group or keys (as shown and as written, "Ctrl+Alt+Up").
fn action_matches(label: &str, id: &str, group: &str, keys: &str, words: &[String]) -> bool {
    let text = format!("{label} {id} {group} {keys}").to_lowercase();
    words.iter().all(|word| text.contains(word.as_str()))
}

/// The plug-in folders, each on or off, and a field to add one. Returns what the user asked for.
/// The plug-in manager's Scan locations draws the same list.
pub(crate) fn plugin_folders(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    y: &mut f32,
    _bottom: f32,
    plugins: &mut PluginsView,
) -> Option<PluginRequest> {
    use suhina_plugins::scan::FolderFormat;
    let mut request = None;
    let small = theme::ui_font(theme::TEXT_SMALL);
    let line = |y: &mut f32, height: f32| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), height));
        *y += height;
        rect
    };
    let format_name = |format: &str| match format {
        "vst3" => "VST3",
        "clap" => "CLAP",
        "vst2" => "VST2",
        "any" if cfg!(target_os = "macos") => "CLAP, VST3 and VST2",
        _ => "CLAP and VST3",
    };

    // Folders: each on or off for scans, with the format taken from it; the added ones can be
    // removed.
    let r = line(y, 18.0);
    widgets::text_in(painter, r, Align2::LEFT_CENTER, "Folders", theme::ui_font(theme::TEXT), ink().name);
    for folder in &plugins.folders {
        let r = line(y, 18.0);
        let check = Rect::from_min_size(Pos2::new(r.left(), r.top() + 1.0), Vec2::splat(16.0));
        let mut enabled = folder.enabled;
        // Off while a scan runs: its result replaces the library, switches too.
        let response = ui.put(check, |ui: &mut egui::Ui| {
            ui.add_enabled(!plugins.scanning, egui::Checkbox::without_text(&mut enabled))
        });
        let label = format!("Scan {}", folder.path);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, folder.enabled, &label));
        if response.changed() {
            request = Some(PluginRequest::SetFolderEnabled(folder.path.clone(), enabled));
        }
        let right = if folder.standard { r.right() } else { r.right() - 68.0 };
        let text = Rect::from_min_max(Pos2::new(check.right() + 6.0, r.top()), Pos2::new(right - 96.0, r.bottom()));
        let color = if folder.enabled { ink().caption } else { ink().hint };
        let shown = if folder.standard { format!("{}  (standard)", folder.path) } else { folder.path.clone() };
        widgets::text_in(painter, text, Align2::LEFT_CENTER, &shown, small.clone(), color);
        let format = Rect::from_min_max(Pos2::new(right - 92.0, r.top()), Pos2::new(right - 4.0, r.bottom()));
        widgets::text_in(painter, format, Align2::RIGHT_CENTER, format_name(&folder.format), small.clone(), ink().hint);
        if !folder.standard {
            let remove =
                Rect::from_min_max(Pos2::new(r.right() - 64.0, r.top() + 1.0), Pos2::new(r.right(), r.bottom() - 1.0));
            let response = widgets::button_area(
                ui,
                remove,
                Id::unique(("remove-folder", &folder.path)),
                format!("Remove {}", folder.path),
            );
            widgets::flat_button(ui, painter, remove, &response, "Remove", (theme::TEXT_SMALL, Some("secondary")));
            if response.clicked() {
                request = Some(PluginRequest::RemoveFolder(folder.path.clone()));
            }
        }
    }
    // A folder to add, and the format it holds. Add with
    // nothing typed asks for the system's folder picker instead.
    *y += 4.0;
    let r = line(y, 20.0);
    let add = Rect::from_min_max(Pos2::new(r.right() - 64.0, r.top()), r.max);
    let formats = Rect::from_min_max(
        Pos2::new(add.left() - 4.0 - 4.0 * 52.0, r.top() + 2.0),
        Pos2::new(add.left() - 4.0, r.bottom() - 2.0),
    );
    let field = Rect::from_min_max(r.min, Pos2::new(formats.left() - 4.0, r.bottom()));
    let edit = ui.put(
        field,
        egui::TextEdit::singleline(&mut plugins.new_folder)
            .frame(widgets::input_frame())
            .hint_text("A folder to look in as well")
            .font(small.clone()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Plug-in folder to add"));
    widgets::input_line(ui, &edit);
    let width = formats.width() / 4.0;
    for (index, (format, title)) in [
        (FolderFormat::Any, "All"),
        (FolderFormat::Vst3, "VST3"),
        (FolderFormat::Clap, "CLAP"),
        (FolderFormat::Vst2, "VST2"),
    ]
    .into_iter()
    .enumerate()
    {
        let cell = Rect::from_min_size(
            Pos2::new(formats.left() + index as f32 * width, formats.top()),
            Vec2::new(width - 1.0, formats.height()),
        );
        let response = widgets::button_area(
            ui,
            cell,
            Id::unique(("folder-format", index)),
            format!("Folder format: {}", format_name(format.name())),
        );
        let chosen = plugins.new_format == format;
        if response.clicked() {
            plugins.new_format = format;
        }
        // A `button[kind="option"]`, `[checked]` for the format chosen.
        let (_, color) = option_face(ui, painter, cell, &response, chosen, "option");
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, title, small.clone(), color);
    }
    let response = widgets::button_area(ui, add, Id::unique("add-folder"), "Add plug-in folder");
    widgets::flat_button(ui, painter, add, &response, "Add", (theme::TEXT_SMALL, None));
    let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if response.clicked() || entered {
        // Nothing typed: the system's folder picker, so Add always does something.
        let folder = plugins.new_folder.trim();
        request = if folder.is_empty() {
            Some(PluginRequest::PickFolder)
        } else {
            Some(PluginRequest::AddFolder(folder.to_string(), plugins.new_format))
        };
    }
    *y += 8.0;
    request
}

/// Scan, Rescan all, and the button that opens the plug-in manager, which lists it all. Returns
/// what the user asked for.
fn plugin_actions(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    y: &mut f32,
    plugins: &mut PluginsView,
) -> Option<PluginRequest> {
    let mut request = None;
    let small = theme::ui_font(theme::TEXT_SMALL);
    let line = |y: &mut f32, height: f32| {
        let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), height));
        *y += height;
        rect
    };
    // Scanning.
    let r = line(y, SETTING_ROW);
    let summary = if plugins.scanning {
        "Scanning: each file is loaded in a separate process, so a plug-in that crashes is only reported".to_string()
    } else {
        let off = plugins.found.iter().filter(|p| !p.enabled).count();
        let mut summary = format!("{} plug-ins found", plugins.found.len());
        if off > 0 {
            summary.push_str(&format!(" ({off} switched off)"));
        }
        summary.push_str(&format!(", {} files failed to load", plugins.failed.len()));
        if !plugins.incompatible.is_empty() {
            summary.push_str(&format!(", {} built for another processor", plugins.incompatible.len()));
        }
        summary
    };
    label(painter, r, "Scan", &summary);
    let buttons = control_rect(r, 2);
    let half = (buttons.width() - 1.0) / 2.0;
    for (index, (text, all)) in [("Scan", false), ("Rescan all", true)].into_iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(buttons.left() + index as f32 * (half + 1.0), buttons.top()),
            Vec2::new(half, buttons.height()),
        );
        let response = widgets::button_area(ui, cell, Id::unique(("scan", index)), text);
        let el = crate::kit::element::BUTTON.response(&response).enter();
        // While a scan runs, the buttons stand back: the region's fill under the hint's ink.
        let fill = if plugins.scanning {
            el.background_or(crate::kit::style_class::surface_or(crate::kit::element::app_fill("panel")))
        } else {
            el.hover_fade(widgets::hover(ui, &response), crate::kit::element::Look::background)
        };
        painter.rect_filled(cell, 0.0, fill);
        let color = if plugins.scanning { ink().hint } else { el.color() };
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, small.clone(), color);
        if response.clicked() && !plugins.scanning {
            request = Some(PluginRequest::Scan { all });
        }
    }

    // The plug-in manager: every plug-in and every file that gave none, in one table.
    let r = line(y, SETTING_ROW);
    label(painter, r, "Plug-in manager", "Every plug-in found, switched on or off, and why files failed");
    let open = control_rect(r, 2);
    let response = widgets::button_area(ui, open, Id::unique("open-plugin-manager"), "Open the plug-in manager");
    widgets::flat_button(ui, painter, open, &response, "Manage plug-ins", (theme::TEXT_SMALL, None));
    if response.clicked() {
        plugins.manager.open = true;
    }
    request
}

/// The audio output chosen, with the host and device it names among those listed.
struct Chosen<'a> {
    choice: suhina_session::OutputChoice,
    default_host: Option<&'a suhina_session::audio::HostInfo>,
    host: Option<&'a suhina_session::audio::HostInfo>,
    device: Option<&'a suhina_session::audio::DeviceInfo>,
}

fn chosen<'a>(settings: &Settings, about: &'a About) -> Chosen<'a> {
    chosen_output(settings.output_choice(), about)
}

fn chosen_output(choice: suhina_session::OutputChoice, about: &About) -> Chosen<'_> {
    let default_host = about.audio_hosts.iter().find(|h| h.default).or(about.audio_hosts.first());
    let host = match &choice.host {
        Some(name) => about.audio_hosts.iter().find(|h| h.name.eq_ignore_ascii_case(name)),
        None => default_host,
    };
    let device = host.and_then(|h| match &choice.device {
        Some(name) => h.devices.iter().find(|d| d.name.eq_ignore_ascii_case(name)),
        None => h.devices.iter().find(|d| d.default).or(h.devices.first()),
    });
    Chosen { choice, default_host, host, device }
}

/// An output chosen anew is kept in the settings, and the app plays on it.
fn use_output(cx: &mut Cx<'_>, picked: Option<suhina_session::OutputChoice>) {
    if let Some(choice) = picked {
        cx.settings.set_output_choice(&choice);
        cx.outcome.audio_output = Some(choice);
        cx.outcome.changed = true;
    }
}

/// Audio › Output: host, device, sample rate and buffer size as dropdowns of what the driver
/// allows, exclusive mode and the input device. The devices are listed when it first shows.
pub(crate) fn audio_output(cx: &mut Cx<'_>, y: &mut f32) {
    if cx.state.audio_listed != Some(true) {
        cx.outcome.audio_refresh = true;
        cx.state.audio_listed = Some(true);
    }
    let active = cx.settings.output_choice();
    let shown = cx.state.audio_pending.as_ref().unwrap_or(&active);
    let picked = output_rows(cx.ui, cx.painter, cx.page, y, shown, cx.about, cx.state.audio_pending.is_some());
    if let Some(picked) = picked {
        if matches!(picked, OutputPick::ControlPanel) {
            cx.outcome.audio_control_panel = true;
            return;
        }
        let picked = restore_driver(picked, cx.settings, cx.about);
        let picked = restore_input(picked, &active, cx.settings);
        let apply = select_output(&mut cx.state.audio_pending, &active, picked);
        use_output(cx, apply);
    }
}

/// Audio › Output › Default track input (#1191): none, or a channel or a stereo pair of the
/// input device, listed from its channels. Each audio device (Device) keeps its own with its
/// Input, so choosing another one brings both back; the app runs `audio.setTrackInput`.
pub(crate) fn default_track_input(cx: &mut Cx<'_>, r: Rect) {
    let choice = cx.settings.output_choice();
    let channels = match &cx.about.audio_live {
        Some(live) if live.input.is_some() => live.input_channels,
        _ => {
            let host = chosen(cx.settings, cx.about).host;
            let inputs = host.map_or(&[][..], |h| h.inputs.as_slice());
            match &choice.input {
                suhina_session::InputChoice::Off => 0,
                suhina_session::InputChoice::Default => inputs.iter().find(|i| i.default).map_or(0, |i| i.channels),
                suhina_session::InputChoice::Named(name) => {
                    inputs.iter().find(|i| i.name.eq_ignore_ascii_case(name)).map_or(0, |i| i.channels)
                }
            }
        }
    };
    if choice.input == suhina_session::InputChoice::Off {
        cx.note("Choose an input device under Input first: each audio device keeps its own default track input");
        return;
    }
    let current = suhina_session::audio::input_channels_text(cx.settings.track_input());
    let options = track_input_options(channels, &current);
    cx.note("What a new audio track hears, and an armed track that hears nothing; each audio device keeps its own with its Input, tracks that are there keep theirs");
    if let Some(picked) = choice_dropdown(cx.ui, cx.painter, r, cx.name, &options, &current)
        && let Some(slot) = cx.settings.track_input_slot()
    {
        *slot = picked;
        cx.outcome.changed = true;
    }
}

/// The choices of Default track input for a device with `channels` inputs: None, each channel,
/// then each stereo pair as the track's Audio From lists them (1/2, 3/4, …); and `current` when
/// it is not among them: as it is when the device has its channels (a pair such as 2/3 set from
/// MCP), marked when it does not.
pub(crate) fn track_input_options(channels: u16, current: &str) -> Vec<(String, String)> {
    use suhina_engine::model::InputChannels;
    let channels = channels.min(suhina_engine::model::MAX_INPUT_CHANNEL);
    let mut options = vec![("None".to_string(), "none".to_string())];
    options.extend((1..=channels).map(|c| (format!("In {c}"), c.to_string())));
    options.extend((1..channels).step_by(2).map(|c| (format!("In {c}/{}", c + 1), format!("{c}/{}", c + 1))));
    if !options.iter().any(|(_, value)| value == current) {
        let last = match suhina_session::audio::input_channels_from_text(current) {
            Some(InputChannels::Mono(c)) => Some(c),
            Some(InputChannels::Stereo(c)) => c.checked_add(1),
            _ => None,
        };
        let label = if last.is_some_and(|last| last <= channels) {
            format!("In {current}")
        } else {
            format!("In {current} (not on this device)")
        };
        options.push((label, current.to_string()));
    }
    options
}

/// Audio › Cue: the cue's output pair and volume.
pub(crate) fn audio_cue(cx: &mut Cx<'_>, y: &mut f32) {
    let picked = cue_rows(cx.ui, cx.painter, cx.page, y, cx.settings, cx.about);
    use_output(cx, picked);
}

/// Audio › Now: what plays now with its latency and underruns, and List devices again.
pub(crate) fn audio_now(cx: &mut Cx<'_>, y: &mut f32) {
    cx.outcome.audio_refresh |= now_rows(cx.ui, cx.painter, cx.page, y, cx.settings, cx.about);
}

fn native_panel_visible(
    pending: bool,
    host: Option<&str>,
    device: Option<&str>,
    live: Option<&suhina_session::OutputInfo>,
) -> bool {
    !pending
        && host == Some("ASIO")
        && live.is_some_and(|live| live.host == "ASIO" && device == Some(live.device.as_str()))
}

/// The native panel belongs beside the device it configures.
fn driver_panel_row(ui: &Ui, painter: &egui::Painter, r: Rect) -> bool {
    label(painter, r, "Device settings", "Open the active driver's buffer and hardware settings");
    let rect = Rect::from_min_size(Pos2::new(r.right() - 150.0, r.top() + 1.0), Vec2::new(150.0, SEGMENT));
    let response =
        widgets::button_area(ui, rect, Id::unique("asio-control-panel"), "Open the active ASIO driver's control panel");
    widgets::flat_button(ui, painter, rect, &response, "Control panel…", (theme::TEXT_SMALL, None));
    response.clicked()
}

/// Selecting a driver only changes the form. The device commits the pair through
/// the same audio.setOutput command as every other output change.
enum OutputPick {
    ControlPanel,
    Driver(suhina_session::OutputChoice),
    Device(suhina_session::OutputChoice),
    Setting(suhina_session::OutputChoice),
}

/// A remembered, available pair can be committed atomically; an unknown or missing
/// device still requires an explicit choice, never an intermediate default open.
fn restore_driver(picked: OutputPick, settings: &Settings, about: &About) -> OutputPick {
    let OutputPick::Driver(mut choice) = picked else { return picked };
    if choice.host == settings.output_choice().host {
        return OutputPick::Driver(choice);
    }
    let default = about.audio_hosts.iter().find(|h| h.default);
    let host = choice
        .host
        .as_ref()
        .and_then(|name| about.audio_hosts.iter().find(|h| h.name.eq_ignore_ascii_case(name)))
        .or_else(|| choice.host.is_none().then_some(default).flatten());
    let remembered = settings.remembered_device(choice.host.as_deref(), default.map(|h| h.name.as_str()));
    if let (Some(host), Some(device)) = (host, remembered) {
        let available = match &device {
            Some(name) => host.devices.iter().any(|d| &d.name == name),
            None => host.devices.iter().any(|d| d.default),
        };
        if available {
            choice.device = device;
            return OutputPick::Device(choice);
        }
    }
    OutputPick::Driver(choice)
}

/// Another audio device brings back the input it was last chosen with, so its default track
/// input comes back with it (#1191); a device not chosen before keeps the input there is.
fn restore_input(picked: OutputPick, active: &suhina_session::OutputChoice, settings: &Settings) -> OutputPick {
    let OutputPick::Device(mut choice) = picked else { return picked };
    if choice.device_key() != active.device_key()
        && let Some(input) = settings.remembered_input(&choice)
    {
        choice.input = input;
    }
    OutputPick::Device(choice)
}

fn select_output(
    pending: &mut Option<suhina_session::OutputChoice>,
    active: &suhina_session::OutputChoice,
    picked: OutputPick,
) -> Option<suhina_session::OutputChoice> {
    match picked {
        OutputPick::ControlPanel => None,
        OutputPick::Driver(choice) => {
            *pending = (choice.host != active.host).then_some(choice);
            None
        }
        OutputPick::Device(choice) => {
            *pending = None;
            Some(choice)
        }
        OutputPick::Setting(choice) if pending.is_some() => {
            *pending = Some(choice);
            None
        }
        OutputPick::Setting(choice) => Some(choice),
    }
}

/// The rows of Audio › Output; returns a new choice.
fn output_rows(
    ui: &Ui,
    painter: &egui::Painter,
    page: Rect,
    y: &mut f32,
    choice: &suhina_session::OutputChoice,
    about: &About,
    pending: bool,
) -> Option<OutputPick> {
    let Chosen { choice, default_host, host, device } = chosen_output(choice.clone(), about);
    let row = |y: &mut f32| setting_row(page, y);
    let mut picked: Option<suhina_session::OutputChoice> = None;
    // Host.
    let r = row(y);
    label(painter, r, "Driver", "WASAPI is Windows' own; ASIO talks to the interface's driver directly");
    let mut options: Vec<(String, Option<String>)> =
        vec![(format!("Default ({})", default_host.map_or("none", |h| h.name.as_str())), None)];
    options.extend(about.audio_hosts.iter().map(|h| (h.name.clone(), Some(h.name.clone()))));
    if let Some(host) = choice_dropdown(ui, painter, r, "Driver", &options, &choice.host) {
        return choice
            .with_params(&serde_json::json!({ "host": host.unwrap_or_default() }))
            .ok()
            .map(OutputPick::Driver);
    }
    // Device.
    let r = row(y);
    let note = if pending {
        "Choose a device to apply the driver change".to_string()
    } else {
        match (&choice.device, device) {
            (Some(name), None) => format!("{name} is not there; the default output plays"),
            _ => device_note(host, &about.audio_hosts),
        }
    };
    label(painter, r, "Device", &note);
    let default_device = host.and_then(|h| h.devices.iter().find(|d| d.default)).map_or("none", |d| d.name.as_str());
    let mut options: Vec<(String, Option<String>)> = vec![(format!("Default ({default_device})"), None)];
    if let Some(host) = host {
        options.extend(host.devices.iter().map(|d| (d.name.clone(), Some(d.name.clone()))));
    }
    let mut device_options: Vec<_> = options.into_iter().map(|(label, value)| (label, Some(value))).collect();
    let selected = if pending {
        device_options.insert(0, ("Choose a device…".into(), None));
        None
    } else {
        Some(choice.device.clone())
    };
    if let Some(Some(name)) = choice_dropdown(ui, painter, r, "Device", &device_options, &selected) {
        return choice
            .with_params(&serde_json::json!({ "device": name.unwrap_or_default() }))
            .ok()
            .map(OutputPick::Device);
    }
    // Only expose the panel for the device actually running, never a pending choice.
    if native_panel_visible(
        pending,
        host.map(|h| h.name.as_str()),
        device.map(|d| d.name.as_str()),
        about.audio_live.as_ref(),
    ) && driver_panel_row(ui, painter, row(y))
    {
        return Some(OutputPick::ControlPanel);
    }
    // Sample rate.
    let r = row(y);
    label(painter, r, "Sample rate", "The rates the device offers");
    let default_rate = device.and_then(|d| d.default_sample_rate);
    let mut options: Vec<(String, Option<u32>)> =
        vec![(default_rate.map_or_else(|| "Device's own".to_string(), |r| format!("Device's own ({r} Hz)")), None)];
    if let Some(device) = device {
        options.extend(device.sample_rates.iter().map(|r| (format!("{r} Hz"), Some(*r))));
    }
    if let Some(rate) = choice_dropdown(ui, painter, r, "Sample rate", &options, &choice.sample_rate) {
        picked = choice.with_params(&serde_json::json!({ "sampleRate": rate.unwrap_or(0) })).ok();
    }
    // The buffer row says what the number is, and it is the device's own buffer only where it is
    // asked of the driver: ASIO (when its driver declared a size the engine can render in) and
    // WASAPI exclusive mode. Everywhere else it is Suhina's render block alone: WASAPI shared
    // mode plays the period Windows gives the device, whatever is asked, and a driver that
    // declared no size Suhina can render in keeps its own (ADR 0006). The exclusive row below is
    // WASAPI's alone.
    let wasapi = host.is_some_and(|h| h.name == "WASAPI");
    let sizes = device.map_or_else(Vec::new, |d| d.buffer_sizes.clone());
    let declares = device.and_then(|d| d.buffer_range);
    let exclusive = wasapi && choice.exclusive && device.is_some_and(|d| d.exclusive);
    // The number is the device's own only where one of its declared sizes is asked of it.
    let device_sets = exclusive || (!wasapi && declares.is_some() && !sizes.is_empty());
    // The period Windows gives the device is a duration, but WASAPI reports it in frames at the
    // device's own rate, so it is said in milliseconds: the same duration at any rate.
    let period_ms = device
        .and_then(|d| Some((d.buffer_range?, d.default_sample_rate?)))
        .filter(|((min, max), _)| min == max)
        .map(|((min, _), rate)| f64::from(min) * 1000.0 / f64::from(rate.max(1)));
    let r = row(y);
    let rate = f64::from(choice.sample_rate.or(default_rate).unwrap_or(48_000));
    let (name, note) = if exclusive {
        ("Buffer size", "The device's own buffer; Windows asks the device for it".to_string())
    } else if device_sets {
        let note = match declares {
            Some((min, max)) if min < max => {
                format!("Requested driver buffer: {min} to {max} frames")
            }
            Some((min, _)) => format!("Requested driver buffer: {min} frames only"),
            None => "The device's own buffer; its driver sets the size".to_string(),
        };
        ("Buffer size", note)
    } else if wasapi {
        let note = match period_ms {
            Some(ms) => format!("Suhina's render block; Windows plays this device in {ms:.1} ms periods"),
            None => "Suhina's render block; Windows sets the device's buffer".to_string(),
        };
        ("Engine block", note)
    } else {
        ("Engine block", "Suhina's render block; the driver keeps its own buffer size".to_string())
    };
    match buffer_row(&sizes, device_sets, host.is_some_and(|h| h.name == "ASIO")) {
        // A driver that takes one size, as one whose size is set in its own control panel: the
        // size is said, not offered, as there is nothing to choose. A size chosen before stays
        // in the settings and plays again on a device that takes it.
        BufferRow::Only(frames) => {
            label(painter, r, name, "Set in the driver's own control panel (Control panel… above)");
            let value = Rect::from_min_size(Pos2::new(control_x(r), r.top() + 1.0), Vec2::new(260.0, SEGMENT));
            let text = format!("{frames} frames ({:.1} ms)", f64::from(frames) * 1000.0 / rate.max(1.0));
            widgets::text_in(painter, value, Align2::LEFT_CENTER, &text, theme::ui_font(theme::TEXT_SMALL), ink().name);
        }
        BufferRow::Choose => {
            label(painter, r, name, &note);
            // With none chosen an ASIO driver plays the size it prefers (the one set in its
            // control panel); elsewhere the engine renders in its own 1024-frame block.
            let default_frames = device.and_then(|d| d.default_buffer).unwrap_or(1024);
            let options = buffer_options(&sizes, default_frames, choice.buffer_frames, rate, wasapi);
            if let Some(frames) = choice_dropdown(ui, painter, r, name, &options, &choice.buffer_frames) {
                picked = choice.with_params(&serde_json::json!({ "bufferFrames": frames.unwrap_or(0) })).ok();
            }
        }
    }
    // Exclusive mode belongs to WASAPI, not to the ASIO driver panel.
    if wasapi {
        let r = row(y);
        let note = if device.is_some_and(|d| !d.exclusive) {
            "This device does not allow it; it plays shared"
        } else {
            "The device plays only Suhina; other programs are not heard on it"
        };
        label(painter, r, "Exclusive mode", note);
        if let Some(on) = switch(ui, painter, r, "Exclusive mode", choice.exclusive) {
            picked = choice.with_params(&serde_json::json!({ "exclusive": on })).ok();
        }
    }

    // Input device (ADR 0017): off until chosen, so nothing opens a microphone unasked.
    let r = row(y);
    let inputs = host.map_or(&[][..], |h| h.inputs.as_slice());
    let note = match &choice.input {
        suhina_session::InputChoice::Named(name)
            if !inputs.iter().any(|i| suhina_session::InputChoice::Named(i.name.clone()) == choice.input) =>
        {
            format!("{name} is not there on this driver")
        }
        _ => "What tracks with an audio input hear: microphones, an interface's inputs".to_string(),
    };
    label(painter, r, "Input", &note);
    let default_input = inputs.iter().find(|i| i.default).map_or("none", |i| i.name.as_str());
    let mut options: Vec<(String, suhina_session::InputChoice)> = vec![
        ("Off".to_string(), suhina_session::InputChoice::Off),
        (format!("Default ({default_input})"), suhina_session::InputChoice::Default),
    ];
    options.extend(inputs.iter().map(|i| {
        let label = format!("{} ({} in)", i.name, i.channels);
        (label, suhina_session::InputChoice::Named(i.name.clone()))
    }));
    if let Some(input) = choice_dropdown(ui, painter, r, "Input", &options, &choice.input) {
        picked = choice.with_params(&serde_json::json!({ "input": input.as_text() })).ok();
    }

    picked.map(OutputPick::Setting)
}

/// How the Buffer size row shows: a dropdown to choose from, or the one size the driver takes,
/// said as text.
#[derive(Debug, PartialEq)]
enum BufferRow {
    Choose,
    Only(u32),
}

/// The Buffer size row for a device offering `sizes`, when the size is the device's own
/// (`device_sets`): an ASIO driver that takes one size, as one whose size is set in its own
/// control panel, has it said, not offered; with none chosen that size plays and the engine
/// renders in it. Elsewhere a size chosen sets the engine's block even where the device keeps
/// its own, so there is always a choice.
fn buffer_row(sizes: &[u32], device_sets: bool, asio: bool) -> BufferRow {
    match sizes {
        [frames] if device_sets && asio => BufferRow::Only(*frames),
        _ => BufferRow::Choose,
    }
}

/// The Buffer size list: Default (`default_frames`, what plays with none chosen), then `sizes`
/// with their length at `rate`. A size chosen before that the device does not offer now stays
/// chosen and is listed in its place: on WASAPI (`engine_block`), where the number is the
/// engine's block, it plays when it lies within the sizes offered; elsewhere the list says the
/// device does not take it, and the nearest size it takes plays until it does.
fn buffer_options(
    sizes: &[u32],
    default_frames: u32,
    chosen: Option<u32>,
    rate: f64,
    engine_block: bool,
) -> Vec<(String, Option<u32>)> {
    let ms = |n: u32| f64::from(n) * 1000.0 / rate.max(1.0);
    let mut listed: Vec<(u32, String)> = sizes.iter().map(|n| (*n, format!("{n} frames ({:.1} ms)", ms(*n)))).collect();
    if let Some(kept) = chosen.filter(|n| !sizes.contains(n)) {
        let plays = engine_block
            && sizes.first().is_some_and(|least| kept >= *least)
            && sizes.last().is_some_and(|most| kept <= *most);
        let label = if plays {
            format!("{kept} frames ({:.1} ms)", ms(kept))
        } else {
            format!("{kept} frames, chosen; the device does not take it now")
        };
        listed.push((kept, label));
        listed.sort_by_key(|(n, _)| *n);
    }
    std::iter::once((format!("Default ({default_frames} frames)"), None))
        .chain(listed.into_iter().map(|(n, label)| (label, Some(n))))
        .collect()
}

/// What the Device row says about the driver chosen: why it lists nothing, and where a driver's
/// list leaves out the outputs people look for first.
fn device_note(host: Option<&suhina_session::audio::HostInfo>, hosts: &[suhina_session::audio::HostInfo]) -> String {
    let beside = |name: &str| hosts.iter().any(|h| h.name == name);
    match host {
        Some(host) if host.error.is_some() => {
            format!("{} cannot list its devices: {}", host.name, host.error.as_deref().unwrap_or_default())
        }
        Some(host) if host.name == "ASIO" && beside("WASAPI") => {
            "Interfaces with an ASIO driver; Bluetooth headphones are under WASAPI".to_string()
        }
        Some(host) if host.name == "ALSA" && beside("PulseAudio") => {
            "The sound cards; Bluetooth headphones are under PulseAudio".to_string()
        }
        _ => "The output device on that driver".to_string(),
    }
}

/// The rows of Audio › Cue; returns a new choice.
fn cue_rows(
    ui: &Ui,
    painter: &egui::Painter,
    page: Rect,
    y: &mut f32,
    settings: &Settings,
    about: &About,
) -> Option<suhina_session::OutputChoice> {
    let Chosen { choice, device, .. } = chosen(settings, about);
    let row = |y: &mut f32| setting_row(page, y);
    let mut picked = None;
    // The cue's output pair: the device's pairs after the main output's.
    let r = row(y);
    let channels = device.map_or(2, |d| d.channels);
    let note = match choice.cue_pair {
        Some(pair) if choice.check_cue_pair(device.map(|d| d.channels)).is_err() => {
            let (first, second) = suhina_session::audio::cue_channels(pair);
            format!("The device has no {first}/{second}; the cue plays with the main output")
        }
        _ if channels < 4 => "The device has two outputs: previews play with the main output".to_string(),
        _ => "Where previews and cued tracks (C) play, such as headphones".to_string(),
    };
    label(painter, r, "Cue output", &note);
    let mut options: Vec<(String, Option<u16>)> = vec![("Same as main (1/2)".to_string(), None)];
    let pairs = (channels / 2).saturating_sub(1).min(suhina_session::audio::MAX_CUE_PAIR);
    options.extend((1..=pairs).map(|pair| {
        let (first, second) = suhina_session::audio::cue_channels(pair);
        (format!("Outputs {first}/{second}"), Some(pair))
    }));
    if let Some(pair) = choice.cue_pair.filter(|p| *p > pairs) {
        let (first, second) = suhina_session::audio::cue_channels(pair);
        options.push((format!("Outputs {first}/{second} (not there)"), Some(pair)));
    }
    if let Some(pair) = choice_dropdown(ui, painter, r, "Cue output", &options, &choice.cue_pair) {
        picked = choice.with_params(&serde_json::json!({ "cue_pair": pair.unwrap_or(0) })).ok();
    }
    // The cue volume.
    let r = row(y);
    label(painter, r, "Cue volume", "How loud the cue plays; the main output does not change");
    let options = [("-12 dB", -12), ("-6 dB", -6), ("0 dB", 0), ("+6 dB", 6)];
    let db = settings.audio_cue_volume;
    if let Some(db) = segmented(ui, painter, control_rect(r, 4), "Cue volume", &options, db) {
        picked = choice.with_cue_volume(&serde_json::json!({ "db": db })).ok();
    }

    picked
}

/// The rows of Audio › Now; returns whether List devices again was clicked.
fn now_rows(ui: &Ui, painter: &egui::Painter, page: Rect, y: &mut f32, settings: &Settings, about: &About) -> bool {
    let choice = settings.output_choice();
    let row = |y: &mut f32| setting_row(page, y);
    let r = row(y);
    let (output, latency) = match &about.audio_live {
        Some(info) => (
            now_playing(info, &choice),
            format!(
                "{:.1} ms, {} underrun{}{}",
                info.latency_ms,
                info.underruns,
                if info.underruns == 1 { "" } else { "s" },
                if info.failed { "; the device reported an error" } else { "" }
            ),
        ),
        None => ("No project is playing".to_string(), "Not playing".to_string()),
    };
    label(painter, r, "Playing on", &output);
    let refresh = Rect::from_min_size(Pos2::new(r.right() - 120.0, r.top() + 1.0), Vec2::new(120.0, SEGMENT));
    let response = widgets::button_area(ui, refresh, Id::unique("audio-refresh"), "List the audio devices again");
    widgets::flat_button(ui, painter, refresh, &response, "List devices again", (theme::TEXT_SMALL, None));
    let r = row(y);
    label(painter, r, "Delay to the speakers", &format!("Measured from the engine to the speakers: {latency}"));
    let r = row(y);
    let buffers = about.audio_live.as_ref().map_or_else(
        || "No active output".to_string(),
        |info| format!("Engine: {} frames · Driver callback: {} frames", info.engine_block, info.buffer_frames),
    );
    label(painter, r, "Active buffers", &buffers);
    // A key on a MIDI controller (ADR 0017): the engine keeps less ahead while a track is armed.
    let r = row(y);
    let key = match &about.audio_live {
        Some(info) if info.live => {
            format!("Up to {:.1} ms; a track is armed, so the engine keeps one block ahead", info.live_latency_ms)
        }
        Some(info) => format!(
            "Up to {:.1} ms now; arming a track keeps one block ahead, so less. Smaller buffers are quicker",
            info.live_latency_ms
        ),
        None => "Not playing".to_string(),
    };
    label(painter, r, "Delay when you play a key", &key);
    let r = row(y);
    let input = match &about.audio_live {
        Some(info) => match &info.input {
            Some(name) => format!(
                "{name}, {} channel{}; {} underrun{}",
                info.input_channels,
                if info.input_channels == 1 { "" } else { "s" },
                info.input_underruns,
                if info.input_underruns == 1 { "" } else { "s" }
            ),
            None => "None: no input device is open".to_string(),
        },
        None => "Not playing".to_string(),
    };
    label(painter, r, "Recording input", &input);
    let r = row(y);
    label(painter, r, "Render threads", &render_threads_now(about));
    response.clicked()
}

/// The threads that render each block and the blocks they missed (ADR 0038): "1 thread" when
/// the render thread works alone.
pub(crate) fn render_threads_now(about: &About) -> String {
    let Some(info) = &about.audio_live else { return "Not playing".to_string() };
    let threads = info.render_threads.max(1);
    let plural = |n: u64| if n == 1 { "" } else { "s" };
    if threads == 1 {
        return "1 thread: the render thread alone".to_string();
    }
    match (info.render_misses, &about.audio_late_track) {
        (0, _) => format!("{threads} threads; no block late"),
        (late, Some(track)) => format!(
            "{threads} threads; {late} block{} late, most on {track} ({})",
            plural(late),
            info.worst_render_misses
        ),
        (late, None) => format!("{threads} threads; {late} block{} late", plural(late)),
    }
}

/// A segmented control for the keymap preset: the sets of keys, with the chosen one lit.
/// Returns the preset clicked, if it is not the current one.
fn preset_segmented(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    options: &[(&str, Preset)],
    current: Preset,
) -> Option<Preset> {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let count = options.len().max(1) as f32;
    let part = (rect.width() - (count - 1.0)) / count;
    let mut picked = None;
    for (index, (text, value)) in options.iter().enumerate() {
        let x = rect.left() + index as f32 * (part + 1.0);
        let segment = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(part, rect.height()));
        let chosen = *value == current;
        let response =
            widgets::button_area(ui, segment, Id::unique(("keymap-preset", *text)), format!("Preset: {text}"));
        // A `button[kind="preset"]`, `[checked]` for the chosen preset.
        let (_, color) = option_face(ui, painter, segment, &response, chosen, "preset");
        widgets::text_in(painter, segment, Align2::CENTER_CENTER, text, small.clone(), color);
        if response.clicked() && !chosen {
            picked = Some(*value);
        }
    }
    picked
}

/// Settings › Audio › Now › Output: the device that plays, and, when it plays in place of the
/// chosen one that is not there, which device it goes back to once it is.
fn now_playing(info: &suhina_session::OutputInfo, choice: &suhina_session::OutputChoice) -> String {
    let wanted = choice.device.as_deref().or(choice.host.as_deref());
    match wanted {
        Some(wanted) if info.fallback => format!("{}, until {wanted} is back", info.describe()),
        _ => info.describe(),
    }
}

/// MIDI › MIDI inputs: each port with On and Off. What the ports that are on play
/// goes to the armed tracks. Returns a port turned on or off.
fn midi_section(ui: &Ui, painter: &egui::Painter, page: Rect, y: &mut f32, about: &About) -> Option<(String, bool)> {
    let mut picked = None;
    let ports = match &about.midi_inputs {
        Ok(ports) if ports.is_empty() => {
            let r = setting_row(page, y);
            label(painter, r, "No MIDI controller found", "Plug one in and it appears here within a few seconds");
            return None;
        }
        Ok(ports) => ports,
        Err(e) => {
            let r = setting_row(page, y);
            label(painter, r, "Not available", e);
            return None;
        }
    };
    for port in ports {
        let r = setting_row(page, y);
        let note = match (&port.error, port.enabled, port.open) {
            (Some(e), true, _) => format!("Could not be opened: {e}"),
            (_, true, true) => "Plays the armed tracks".to_string(),
            (_, true, false) => "Opening…".to_string(),
            (_, false, _) => "Off".to_string(),
        };
        label(painter, r, &port.name, &note);
        let name = format!("MIDI input {}", port.name);
        if let Some(on) = switch(ui, painter, r, &name, port.enabled) {
            picked = Some((port.name.clone(), on));
        }
    }
    picked
}

/// A section's title over a hairline, as the other pages draw it.
fn section_title(painter: &egui::Painter, page: Rect, y: &mut f32, name: &str) {
    let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SIDEBAR_ROW));
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT_SMALL), ink().hint);
    painter.hline(page.x_range(), rect.bottom() + 3.5, theme::hairline(crate::kit::element::row_line()));
    *y = rect.bottom() + 8.0;
}

/// A setting's row under `y`, moving `y` past it.
fn setting_row(page: Rect, y: &mut f32) -> Rect {
    let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SETTING_ROW));
    *y += SETTING_ROW;
    rect
}

/// A dropdown on the right of `row` listing `options` (text, value). Returns the value picked,
/// if it is not the current one.
fn choice_dropdown<T: Clone + PartialEq>(
    ui: &Ui,
    painter: &egui::Painter,
    row: Rect,
    name: &str,
    options: &[(String, T)],
    current: &T,
) -> Option<T> {
    let left = control_x(row).min(row.right() - DROPDOWN_FIELD);
    let field = Rect::from_min_size(Pos2::new(left, row.top() + 1.0), Vec2::new(row.right() - left, SEGMENT + 2.0));
    choice_field(ui, painter, field, name, options, current)
}

/// A drop-down in `field`, named `name`: the value of `options` that is `current`, the list under
/// it. Returns the value picked.
fn choice_field<T: Clone + PartialEq>(
    ui: &Ui,
    painter: &egui::Painter,
    field: Rect,
    name: &str,
    options: &[(String, T)],
    current: &T,
) -> Option<T> {
    let shown = options.iter().find(|(_, v)| v == current).map_or("-", |(text, _)| text.as_str());
    let response = widgets::button_area(ui, field, Id::unique(("audio-setting", name)), format!("{name}: {shown}"));
    widgets::form_dropdown(ui, painter, field, &response, shown);
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        for (label, value) in options {
            if ui.selectable_label(value == current, text(label)).clicked() && value != current {
                picked = Some(value.clone());
            }
        }
    });
    picked
}

/// A setting's name in the first column of `row`, cut to `name_rect`, and under it `status` when
/// the setting reports one (the last backup, a folder that is missing). Returns the height the
/// row needs. The setting's description is its name's tooltip (`describe`), not a line of its own.
fn wrapped_label(painter: &egui::Painter, row: Rect, name_rect: Rect, name: &str, status: Option<&str>) -> f32 {
    let width = name_rect.width().min(CONTROL_COLUMN - 8.0);
    let top = Rect::from_min_size(name_rect.min, Vec2::new(width, 16.0));
    widgets::text_in(painter, top, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), ink().name);
    let Some(status) = status.filter(|s| !s.is_empty()) else { return LINE_ROW };
    let color = ink().caption;
    let galley =
        painter.layout(status.to_string(), theme::ui_font(theme::TEXT_SMALL), color, (row.width() - 8.0).max(40.0));
    let height = galley.size().y;
    painter.galley(Pos2::new(row.left(), row.top() + 18.0), galley, color);
    LINE_ROW.max(18.0 + height + 6.0)
}

/// The description of the setting named `name`, as Settings › Help
/// asks: a tooltip on its name, or on a ? drawn after the name. (Shown under the name, it is the
/// row's second line instead, drawn by [`wrapped_label`].)
fn describe(ui: &Ui, painter: &egui::Painter, row: Rect, name: &str, description: &str, how: Descriptions) {
    let font = theme::ui_font(theme::TEXT);
    let name_width = widgets::text_width(ui, name, font).min(CONTROL_COLUMN - 26.0);
    let area = match how {
        Descriptions::Tooltip => Rect::from_min_size(row.min, Vec2::new(CONTROL_COLUMN - 8.0, LINE_ROW)),
        Descriptions::Icon => {
            let icon =
                Rect::from_center_size(Pos2::new(row.left() + name_width + 12.0, row.top() + 8.0), Vec2::splat(12.0));
            painter.circle_stroke(icon.center(), 5.5, egui::Stroke::new(1.0, ink().hint));
            widgets::text_in(
                painter,
                icon,
                Align2::CENTER_CENTER,
                "?",
                theme::ui_font(theme::TEXT_TINY),
                ink().caption,
            );
            icon.expand(3.0)
        }
        Descriptions::Shown => return,
    };
    let response = ui.interact(area, Id::unique(("setting-description", name)), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, description));
    response.on_hover_text(description);
}

/// A setting's name and a note under it, on the left of its row.
fn label(painter: &egui::Painter, row: Rect, name: &str, note: &str) {
    let top = Rect::from_min_size(row.min, Vec2::new(row.width(), 16.0));
    let bottom = Rect::from_min_size(Pos2::new(row.left(), top.bottom()), Vec2::new(row.width() - LABEL_WIDTH, 14.0));
    widgets::text_in(painter, top, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), ink().name);
    widgets::text_in(painter, bottom, Align2::LEFT_CENTER, note, theme::ui_font(theme::TEXT_SMALL), ink().caption);
}

/// The most choices shown as option buttons; more are a drop-down.
const MAX_BUTTONS: usize = 5;
/// The width of a switch.
const SWITCH_WIDTH: f32 = 34.0;

/// A switch on the right of `row` for a yes/no setting: one control, labelled with the setting's
/// name. Returns the new value when it is clicked.
fn switch(ui: &Ui, painter: &egui::Painter, row: Rect, name: &str, on: bool) -> Option<bool> {
    let track = Rect::from_min_size(Pos2::new(control_x(row), row.top() + 1.0), Vec2::new(SWITCH_WIDTH, SEGMENT));
    let response = ui.interact(track, Id::unique(("setting-switch", name)), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, name));
    // A `button[kind="switch"]`, `[checked]` while on: its track's fill.
    let lit = widgets::lit_toggle_kind(
        ui,
        &response,
        crate::kit::element::BUTTON.builder(),
        &[crate::kit::element::Checked],
        on,
        "switch",
    );
    let (fill, _) = lit.colors();
    lit.el.paint_over(painter, track, crate::kit::element::Native { radius: 0.0, fill }, lit.toggle.image(lit.lit));
    let widgets::Lit { el, lit, .. } = lit;
    drop(el);
    let knob_width = SEGMENT - 4.0;
    let x = track.left() + 2.0 + lit * (track.width() - 4.0 - knob_width);
    let knob = Rect::from_min_size(Pos2::new(x, track.top() + 2.0), Vec2::new(knob_width, SEGMENT - 4.0));
    // The knob's lit colour contrasts with the lit track: an opaque ground, else the inverse text.
    let ground = crate::kit::style_class::opaque_surface_or(crate::kit::element::app_fill("window"));
    let ground = if ground.a() == u8::MAX { ground } else { crate::kit::element::app_fill("knob") };
    painter.rect_filled(knob, 0.0, motion::mix(ink().caption, ground, lit));
    response.clicked().then_some(!on)
}

/// Where a segmented control for `options` sits on the right of a row: every part wide enough
/// for the longest text, so none is cut off.
fn options_rect<T>(ui: &Ui, row: Rect, options: &[(&str, T)]) -> Rect {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let widest = options.iter().map(|(text, _)| widgets::text_width(ui, text, font.clone())).fold(0.0_f32, f32::max);
    let part = (widest + 16.0).max(48.0).ceil();
    let count = options.len().max(1) as f32;
    let width = part * count + (count - 1.0);
    Rect::from_min_size(Pos2::new(control_x(row), row.top() + 1.0), Vec2::new(width, SEGMENT))
}

/// Where a setting's control starts in `row`: the second column.
fn control_x(row: Rect) -> f32 {
    row.left() + CONTROL_COLUMN
}

/// Where a segmented control with `parts` parts sits on the right of a row.
fn control_rect(row: Rect, parts: usize) -> Rect {
    let width = 64.0 * parts as f32;
    Rect::from_min_size(Pos2::new(control_x(row), row.top() + 1.0), Vec2::new(width, SEGMENT))
}

/// A segmented control (components › Dropdown and segmented control). Returns the option
/// clicked, if it is not the current one.
fn segmented<T: Copy + PartialEq>(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    name: &str,
    options: &[(&str, T)],
    current: T,
) -> Option<T> {
    let count = options.len().max(1) as f32;
    let part = (rect.width() - (count - 1.0)) / count;
    let mut picked = None;
    for (index, (text, value)) in options.iter().enumerate() {
        let x = rect.left() + index as f32 * (part + 1.0);
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(part, rect.height()));
        let response = widgets::button_area(ui, cell, Id::unique(("setting", name, index)), format!("{name}: {text}"));
        if response.clicked() && *value != current {
            picked = Some(*value);
        }
        // A part is a `button[kind="option"]`, `[checked]` when chosen.
        let (_, color) = option_face(ui, painter, cell, &response, *value == current, "option");
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}

/// One part of a row of choices, a `button` of `kind` (`option`, `preset`), `[checked]` while it
/// is the one chosen: its face painted in `rect` and its fill and ink, which fade with the choice
/// and the pointer.
fn option_face(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    response: &egui::Response,
    chosen: bool,
    kind: &'static str,
) -> (egui::Color32, egui::Color32) {
    let lit = widgets::lit_toggle_kind(
        ui,
        response,
        crate::kit::element::BUTTON.builder(),
        &[crate::kit::element::Checked],
        chosen,
        kind,
    );
    let (fill, color) = lit.colors();
    lit.el.paint_over(painter, rect, crate::kit::element::Native { radius: 0.0, fill }, lit.toggle.image(lit.lit));
    (fill, color)
}

#[cfg(test)]
#[path = "settings_view_tests.rs"]
mod tests;
