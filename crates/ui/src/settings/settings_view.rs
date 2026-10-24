// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Settings page, drawn over the main view until it is closed. Its pages are listed down the
//! left under a search field; each setting is a row: a label, a short note and its control. What
//! is on which page is the table in `settings_rows.rs` (`docs/spec/ui-settings.md`).

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::interaction::pick::PickField;
use crate::kit::motion;
use crate::kit::theme::{self, PAD, Palette};
use crate::kit::widgets;
use crate::settings::keymap::{Keymap, Preset, Shortcut, actions};
use crate::settings::workflow::Setting;
use crate::settings::{Graphics, Settings};

const PAGE_WIDTH: f32 = 560.0;
/// The list of pages on the left, and the gap between it and the page.
const SIDEBAR: f32 = 140.0;
const SIDEBAR_GAP: f32 = 24.0;
const SIDEBAR_ROW: f32 = 20.0;
/// The search field over the list of pages.
const SEARCH_FIELD: f32 = 18.0;
const LABEL_WIDTH: f32 = 180.0;
const SETTING_ROW: f32 = 34.0;
const SEGMENT: f32 = 15.0;
/// A folder field on the right of its row; empty shows the default folder.
const FOLDER_FIELD: f32 = 300.0;
/// The Browse… button at the end of a path field.
const BROWSE_WIDTH: f32 = 60.0;
/// Themes shown before their list scrolls.
const THEME_ROWS: usize = 8;
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
    /// The NAM devices' folders ([docs/spec/nam-amp-and-cab.md]) the same way.
    pub nam_models_default: String,
    pub irs_default: String,
    /// The active project's newest backup, as Settings › General shows it: when and where.
    pub last_backup: String,
    /// The active project's implicit MCP write folder, displayed alongside explicit grants.
    pub mcp_project_folder: Option<String>,
}

impl Default for About {
    fn default() -> Self {
        Self {
            audio_live: None,
            audio_hosts: Vec::new(),
            midi_inputs: Ok(Vec::new()),
            graphics_now: None,
            editors_embeddable: false,
            projects_default: String::new(),
            templates_default: String::new(),
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
}

/// What the page or the plug-in manager asks the app to do with the plug-in library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRequest {
    /// Scan new and changed files, or every file again.
    Scan {
        all: bool,
    },
    AddFolder(String, suhina_plugins::scan::FolderFormat),
    RemoveFolder(String),
    /// Switch a folder on or off for scans.
    SetFolderEnabled(String, bool),
    /// Switch a plug-in (by spec) on or off.
    SetEnabled(String, bool),
    /// Switch several plug-ins (by spec) on or off together (#725).
    SetEnabledMany(Vec<String>, bool),
    /// Read one file again now.
    ScanFile(String),
}

/// The themes as the page shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemesView {
    /// Each theme's id, name and whether it comes with Suhina: the user's own first.
    pub list: Vec<(String, String, bool)>,
    /// The id of the theme in use, whether it comes with Suhina, its name and its colours.
    pub current: String,
    pub built_in: bool,
    pub name: String,
    pub colors: Palette,
    /// The folder theme files are kept in, if there is one.
    pub folder: Option<String>,
}

/// What the page asks the app to do with the themes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeRequest {
    /// Use the theme with this id.
    Use(String),
    /// Use these colours; a built-in theme is copied first, since it does not change.
    Edit(Palette),
    /// Give the theme in use this name (and its file a name made from it).
    Rename(String),
    /// A copy of the theme in use, to change.
    Duplicate,
    /// Read the theme files again.
    Reload,
    /// Show the themes folder in the file manager.
    OpenFolder,
    /// Write every theme file into one zip.
    Backup,
    /// Write a theme template and a README on the format into a zip.
    ExportTemplate,
}

/// The pages of Settings, in the order the list on the left shows them
/// (`docs/spec/ui-settings.md` › Pages and sections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    General,
    Export,
    Audio,
    Midi,
    Plugins,
    Arrangement,
    Session,
    ClipEditor,
    Transport,
    DetailArea,
    Browser,
    Appearance,
    /// Appearance › Theme, a page of its own under Appearance in the list.
    Theme,
    Shortcuts,
}

impl Page {
    /// Every page with its name.
    pub const ALL: [(Page, &'static str); 14] = [
        (Page::General, "General"),
        (Page::Export, "Export"),
        (Page::Audio, "Audio"),
        (Page::Midi, "MIDI"),
        (Page::Plugins, "Plug-ins"),
        (Page::Arrangement, "Arrangement"),
        (Page::Session, "Session and mixer"),
        (Page::ClipEditor, "Clip editor"),
        (Page::Transport, "Transport"),
        (Page::DetailArea, "Detail area"),
        (Page::Browser, "Browser"),
        (Page::Appearance, "Appearance"),
        (Page::Theme, "Theme"),
        (Page::Shortcuts, "Shortcuts"),
    ];

    /// The page's name, as the list shows it.
    pub fn name(self) -> &'static str {
        Self::ALL.iter().find(|(page, _)| *page == self).map_or("", |(_, name)| name)
    }
}

/// What the Settings page keeps between frames: the page shown, and the action whose keys are
/// being set (the next key pressed becomes its key).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageState {
    pub page: Page,
    pub capture: Option<String>,
    /// The name being typed for a theme, with the theme's id.
    pub theme_name: Option<(String, String)>,
    /// The theme the list was last scrolled to, so it shows the one in use when that changes.
    pub theme_shown: Option<String>,
    /// The audio devices were listed since the Audio page was opened.
    pub audio_listed: Option<bool>,
    /// What is typed in the search field: while it has text, the settings that match it are shown
    /// in place of a page.
    pub search: String,
    /// The page and search the scroll position belongs to: another one opens at its top.
    pub scrolled: Option<(Page, String)>,
    /// The control with the keyboard focus when it was last scrolled into view.
    pub focused: Option<Id>,
    /// What is typed in the Shortcuts page's own filter: only the actions it matches are listed.
    pub key_filter: String,
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
    /// A MIDI input port was turned on or off: the app runs `midi.setInput`.
    pub midi_input: Option<(String, bool)>,
    /// A field's Browse… was clicked: the app shows the system's dialog for it.
    pub pick: Option<PickField>,
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
}

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
    Entry { page, section, name, note, keys, draw: Draw::Row(draw), shown: always }
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
    Entry { page, section, name, note, keys, draw: Draw::Block(draw), shown: always }
}

impl Entry {
    /// Shown only when `shown` says so.
    pub(crate) const fn when(self, shown: fn(&Settings) -> bool) -> Self {
        Self { shown, ..self }
    }

    /// Every word is in its name, note, section or page; for the shortcuts, in an action's too.
    fn matches(&self, words: &[String]) -> bool {
        let text = format!("{} {} {} {}", self.name, self.note, self.section, self.page.name()).to_lowercase();
        words.iter().all(|word| text.contains(word.as_str()))
            || (self.page == Page::Shortcuts
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
        label(self.painter, row, "MCP allowed write folders", "Each allowed folder includes its subfolders");
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
                theme::text_primary(),
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
        let rect = control_rect(row, options.len());
        if let Some(picked) = segmented(self.ui, self.painter, rect, self.name, options, *value) {
            *value = picked;
            self.outcome.changed = true;
        }
    }

    /// On and Off for the setting `field` gives.
    pub fn on_off(&mut self, row: Rect, field: impl FnOnce(&mut Settings) -> &mut bool) {
        self.choose(row, &[("On", true), ("Off", false)], field);
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
        // The note is cut where the field starts.
        self.label =
            Some(Rect::from_min_size(row.min, Vec2::new(row.width() - FOLDER_FIELD - 8.0 + LABEL_WIDTH, row.height())));
        let browse = Rect::from_min_size(
            Pos2::new(row.right() - BROWSE_WIDTH, row.top() + 1.0),
            Vec2::new(BROWSE_WIDTH, SEGMENT),
        );
        if crate::kit::widgets::button(self.ui, self.painter, browse, "Browse…", &format!("Browse for {name}")) {
            self.outcome.pick = Some(pick);
        }
        let rect = Rect::from_min_size(
            Pos2::new(row.right() - FOLDER_FIELD, row.top()),
            Vec2::new(FOLDER_FIELD - BROWSE_WIDTH - 4.0, SEGMENT + 2.0),
        );
        let value = field(self.settings);
        let edit = self
            .ui
            .put(rect, egui::TextEdit::singleline(value).hint_text(hint).font(theme::ui_font(theme::TEXT_SMALL)));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, name));
        if edit.changed() {
            self.outcome.changed = true;
        }
    }

    /// The most colour tags the browser may have, kept by the library rather than the settings.
    pub fn tag_limit(&mut self, row: Rect, options: &[(&str, usize)]) {
        let rect = control_rect(row, options.len());
        if let Some(limit) = segmented(self.ui, self.painter, rect, self.name, options, self.tag_limit) {
            self.outcome.tag_limit = Some(limit);
        }
    }

    /// Three buttons, none of them chosen: smaller, back to 100 %, larger.
    pub fn zoom(&mut self, row: Rect) {
        let actual = format!("{} %", self.settings.zoom);
        let options = [("-", Some(-1)), (actual.as_str(), Some(0)), ("+", Some(1))];
        match segmented(self.ui, self.painter, control_rect(row, 3), self.name, &options, None) {
            Some(Some(0)) if self.settings.zoom != crate::settings::ZOOM => {
                self.settings.zoom = crate::settings::ZOOM;
                self.outcome.changed = true;
            }
            Some(Some(steps)) if steps != 0 => self.outcome.changed |= self.settings.step_zoom(steps),
            _ => {}
        }
    }
}

/// Draws the page into the space left in `ui`.
pub fn show(ui: &mut Ui, settings: &mut Settings, sources: Sources<'_>, state: &mut PageState) -> Outcome {
    let mut outcome = Outcome::default();
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());

    let width = SIDEBAR + SIDEBAR_GAP + PAGE_WIDTH;
    let whole = Rect::from_min_max(
        Pos2::new(area.center().x - width / 2.0, area.top()),
        Pos2::new(area.center().x + width / 2.0, area.bottom()),
    )
    .intersect(area.shrink2(Vec2::new(PAD * 2.0, 0.0)));
    let y = area.top() + 24.0;

    // Title and close.
    let title = Rect::from_min_size(Pos2::new(whole.left(), y), Vec2::new(whole.width(), theme::ROW));
    widgets::text_in(
        &painter,
        title,
        Align2::LEFT_CENTER,
        "Settings",
        theme::ui_font(theme::TEXT + 2.0),
        theme::text_primary(),
    );
    let close = Rect::from_min_max(Pos2::new(title.right() - 52.0, title.top()), title.max);
    let response = widgets::button_area(ui, close, Id::new("settings-close"), "Close settings");
    let hover = widgets::hover(ui, &response);
    painter.rect_filled(close, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    widgets::text_in(
        &painter,
        close,
        Align2::CENTER_CENTER,
        "Close",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let top = title.bottom() + 16.0;

    // The search field over the list of pages: Escape in it empties it.
    let field = Rect::from_min_size(Pos2::new(whole.left(), top), Vec2::new(SIDEBAR, SEARCH_FIELD));
    let edit = ui.put(
        field,
        egui::TextEdit::singleline(&mut state.search)
            .hint_text("Search settings")
            .font(theme::ui_font(theme::TEXT_SMALL)),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search settings"));
    let escaped = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
    if escaped {
        state.search.clear();
    }
    let searching = !state.search.trim().is_empty();
    // Escape closes the page, unless it is cancelling a key being set or a search.
    if response.clicked() || (!escaped && state.capture.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        outcome.close = true;
    }

    // The pages down the left, the one shown beside them; none is lit while searching.
    let sidebar = Rect::from_min_max(
        Pos2::new(whole.left(), field.bottom() + 8.0),
        Pos2::new(whole.left() + SIDEBAR, whole.bottom()),
    );
    if let Some(picked) = page_list(ui, &painter, sidebar, (!searching).then_some(state.page)) {
        state.page = picked;
        state.search.clear();
        state.capture = None;
        state.audio_listed = None;
        state.theme_shown = None;
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
    for entry in entries() {
        let listed = if words.is_empty() { entry.page == shown } else { entry.matches(&words) };
        if !listed || !(entry.shown)(cx.settings) {
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
            section_title(&painter, page, &mut y, &title);
            heading = Some((entry.page, entry.section));
        }
        cx.name = entry.name;
        cx.note = None;
        cx.label = None;
        match entry.draw {
            Draw::Row(draw) => {
                let r = setting_row(page, &mut y);
                draw(&mut cx, r);
                let note = cx.note.take();
                label(&painter, cx.label.take().unwrap_or(r), entry.name, note.as_deref().unwrap_or(entry.note));
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
            widgets::button_area(ui, row, Id::new(("settings-page", index)), format!("Settings page: {name}"));
        if response.clicked() && Some(page) != shown {
            picked = Some(page);
        }
        let selected = motion::toward(ui.ctx(), response.id.with("selected"), Some(page) == shown, motion::VALUE);
        let idle = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
        painter.rect_filled(row, 0.0, motion::mix(idle, theme::control_background(), selected));
        // A page under another (Theme under Appearance) is stepped in.
        let indent = if page == Page::Theme { PAD * 2.0 } else { 0.0 };
        let text = Rect::from_min_max(Pos2::new(row.left() + PAD + indent, row.top()), row.max);
        let color = motion::mix(theme::text_secondary(), theme::text_primary(), selected);
        widgets::text_in(painter, text, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), color);
    }
    picked
}

/// Appearance › Theme: the themes, the theme in use and its colours.
pub(crate) fn theme_block(cx: &mut Cx<'_>, y: &mut f32) {
    let bottom = cx.page.bottom();
    cx.outcome.themes = theme_page(cx.ui, cx.painter, cx.page, y, bottom, cx.themes, cx.state);
}

/// Shortcuts › Keys: every action's keys and the preset.
pub(crate) fn shortcuts_block(cx: &mut Cx<'_>, y: &mut f32) {
    let bottom = cx.page.bottom();
    cx.outcome.keymap_changed |= shortcuts_page(cx.ui, cx.painter, cx.page, y, bottom, cx.keymap, cx.state);
}

/// Plug-ins › Library: folders, scanning and what was found.
pub(crate) fn plugin_library(cx: &mut Cx<'_>, y: &mut f32) {
    let bottom = cx.page.bottom();
    cx.outcome.plugins = plugin_section(cx.ui, cx.painter, cx.page, y, bottom, cx.plugins);
}

/// Clip editor › Workflow: the workflow settings (the Mouse-first switches among them), their own
/// rows, kept in the keymap rather than in the settings, so they are saved beside the keys.
pub(crate) fn mouse_switches(cx: &mut Cx<'_>, y: &mut f32) {
    for setting in Setting::ALL {
        let r = setting_row(cx.page, y);
        label(cx.painter, r, setting.name(), setting.note());
        let options: Vec<(&str, usize)> = setting.short_choices().iter().enumerate().map(|(i, c)| (*c, i)).collect();
        let current = cx.keymap.workflow().get(setting);
        let rect = control_rect(r, options.len());
        if let Some(index) = segmented(cx.ui, cx.painter, rect, setting.name(), &options, current) {
            cx.keymap.set_workflow(setting, index);
            cx.outcome.keymap_changed = true;
        }
    }
    let note = Rect::from_min_size(Pos2::new(cx.page.left(), *y), Vec2::new(cx.page.width(), 14.0));
    widgets::text_in(
        cx.painter,
        note,
        Align2::LEFT_CENTER,
        "Kept in keymap.json with the keys; changing one makes the workflow Custom",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
    *y = note.bottom();
}

/// MIDI › MIDI inputs: each port with On and Off.
pub(crate) fn midi_inputs(cx: &mut Cx<'_>, y: &mut f32) {
    if let Some(port) = midi_section(cx.ui, cx.painter, cx.page, y, cx.about) {
        cx.outcome.midi_input = Some(port);
    }
}

/// Appearance › Theme: the themes to pick from, what can be done with them, and the colours of the
/// one in use. Returns what the user asked for.
fn theme_page(
    ui: &mut Ui,
    painter: &egui::Painter,
    page: Rect,
    at: &mut f32,
    bottom: f32,
    themes: &ThemesView,
    state: &mut PageState,
) -> Option<ThemeRequest> {
    let mut y = *at;
    let mut request = None;
    let small = theme::ui_font(theme::TEXT_SMALL);

    // The themes, in a list that scrolls once it is taller than eight rows: a click uses one.
    let rows = themes.list.len().clamp(1, THEME_ROWS) as f32;
    let list = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), rows * theme::ROW));
    y = list.bottom() + 8.0;
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("themes").auto_shrink([false, false]).show(ui, |ui| {
            for (id, name, built_in) in &themes.list {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), theme::ROW), Sense::hover());
                let response = widgets::button_area(ui, r, Id::new(("theme", id)), format!("Theme: {name}"));
                let current = *id == themes.current;
                let selected = motion::toward(ui.ctx(), response.id.with("selected"), current, motion::VALUE);
                let idle = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
                let painter = ui.painter();
                painter.rect_filled(r, 0.0, motion::mix(idle, theme::control_background(), selected));
                let inner = r.shrink2(Vec2::new(PAD, 0.0));
                let color = motion::mix(theme::text_secondary(), theme::text_primary(), selected);
                widgets::text_in(painter, inner, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), color);
                let file = if *built_in { "built-in".to_string() } else { format!("{id}.json") };
                widgets::text_in(painter, inner, Align2::RIGHT_CENTER, &file, small.clone(), theme::text_hint());
                if response.clicked() && !current {
                    request = Some(ThemeRequest::Use(id.clone()));
                }
                if current && state.theme_shown.as_ref() != Some(id) {
                    response.scroll_to_me(None);
                    state.theme_shown = Some(id.clone());
                }
            }
        });
    });

    // The theme in use: its name, typed over and kept on Enter or leaving the field (a built-in
    // theme's name does not change).
    let r = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), SETTING_ROW));
    y += SETTING_ROW;
    let note = match &themes.folder {
        Some(folder) => format!("Kept in {folder}; drop a theme file or a zip of them on the window to add it"),
        None => "There is no folder to keep theme files in".to_string(),
    };
    if themes.built_in {
        label(painter, r, &format!("{} (built-in; changing a colour makes a copy)", themes.name), &note);
    } else {
        if state.theme_name.as_ref().is_none_or(|(id, _)| *id != themes.current) {
            state.theme_name = Some((themes.current.clone(), themes.name.clone()));
        }
        if let Some((_, name)) = state.theme_name.as_mut() {
            let field = Rect::from_min_size(r.min, Vec2::new(r.width() / 2.0, 16.0));
            let edit = ui.put(field, egui::TextEdit::singleline(name).font(theme::ui_font(theme::TEXT)));
            edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Theme name"));
            if edit.lost_focus() && name.trim() != themes.name && !name.trim().is_empty() {
                request = Some(ThemeRequest::Rename(name.trim().to_string()));
            }
        }
        let line = Rect::from_min_size(Pos2::new(r.left(), r.top() + 18.0), Vec2::new(r.width(), 14.0));
        widgets::text_in(painter, line, Align2::LEFT_CENTER, &note, small.clone(), theme::text_secondary());
    }

    // What can be done with the themes.
    let actions = [
        ("Duplicate", "Duplicate theme", ThemeRequest::Duplicate),
        ("Reload", "Reload themes", ThemeRequest::Reload),
        ("Open folder", "Open the themes folder", ThemeRequest::OpenFolder),
        ("Back up", "Back up themes", ThemeRequest::Backup),
        ("Export template", "Export the theme template", ThemeRequest::ExportTemplate),
    ];
    let mut x = page.left();
    for (index, (text, name, asked)) in actions.into_iter().enumerate() {
        let width = widgets::text_width(ui, text, small.clone()) + 2.0 * PAD + 8.0;
        let cell = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, SEGMENT + 2.0));
        x = cell.right() + 1.0;
        let response = widgets::button_area(ui, cell, Id::new(("theme-action", index)), name);
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(cell, 0.0, fill);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, small.clone(), theme::text_primary());
        if response.clicked() {
            request = Some(asked);
        }
    }
    y += SEGMENT + 2.0 + 12.0;

    // Its colours, in the space left.
    let r = Rect::from_min_size(Pos2::new(page.left(), y), Vec2::new(page.width(), 16.0));
    widgets::text_in(painter, r, Align2::LEFT_CENTER, "Colours", small.clone(), theme::text_hint());
    painter.hline(page.x_range(), r.bottom() + 2.5, theme::hairline(theme::border()));
    y = r.bottom() + 6.0;
    let list =
        Rect::from_min_max(Pos2::new(page.left(), y), Pos2::new(page.right(), (bottom - 12.0).max(y + LIST_LEAST)));
    *at = list.bottom();
    let colors = themes.colors;
    let mut edited = colors;
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("colours").auto_shrink([false, false]).show(ui, |ui| {
            let left = egui::Layout::left_to_right(egui::Align::Center);
            for (key, purpose, color) in colors.tokens() {
                ui.horizontal(|ui| {
                    let mut picked = color;
                    let swatch =
                        egui::color_picker::color_edit_button_srgba(ui, &mut picked, egui::color_picker::Alpha::Opaque);
                    // An outline, so a swatch the colour of the page still shows where it is.
                    ui.painter().rect_stroke(
                        swatch.rect,
                        0.0,
                        theme::hairline(theme::border_strong()),
                        egui::StrokeKind::Inside,
                    );
                    let name = format!("Colour {key}");
                    swatch.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, &name));
                    if picked != color
                        && let Some(slot) = edited.token_mut(key)
                    {
                        *slot = picked;
                    }
                    ui.allocate_ui_with_layout(Vec2::new(64.0, 18.0), left, |ui| {
                        ui.set_min_width(64.0);
                        let text = egui::RichText::new(theme::hex(picked))
                            .font(theme::mono_font(theme::TEXT_SMALL))
                            .color(theme::text_value());
                        ui.label(text);
                    });
                    ui.allocate_ui_with_layout(Vec2::new(170.0, 18.0), left, |ui| {
                        ui.set_min_width(170.0);
                        let text = egui::RichText::new(key).font(theme::mono_font(theme::TEXT_SMALL));
                        ui.label(text.color(theme::text_primary()));
                    });
                    let text = egui::RichText::new(purpose).font(small.clone()).color(theme::text_hint());
                    ui.add(egui::Label::new(text).truncate());
                });
            }
        });
    });
    if edited != colors {
        request = Some(ThemeRequest::Edit(edited));
    }
    request
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

    // A key being set: the next key pressed (with its modifiers) is the action's key.
    if let Some(action) = state.capture.clone() {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some(Shortcut::new(*modifiers, *key)),
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
            .hint_text("Filter shortcuts")
            .font(theme::ui_font(theme::TEXT_SMALL)),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Filter shortcuts"));
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
    let response = widgets::button_area(ui, reset, Id::new("reset-keys"), "Reset all keys");
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(reset, 0.0, fill);
    widgets::text_in(painter, reset, Align2::CENTER_CENTER, "Reset all", small.clone(), theme::text_primary());
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
    widgets::text_in(
        painter,
        preset_label,
        Align2::LEFT_CENTER,
        "Preset",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
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
    widgets::text_in(painter, note, Align2::LEFT_CENTER, current.note(), small.clone(), theme::text_secondary());
    top += 30.0;
    for (key, ids) in keymap.conflicts() {
        let r = Rect::from_min_size(Pos2::new(page.left(), top), Vec2::new(page.width(), 16.0));
        let text = format!("{} runs {} at once", key.display(), ids.join(" and "));
        widgets::text_in(painter, r, Align2::LEFT_CENTER, &text, small.clone(), theme::recording());
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
                    ui.label(egui::RichText::new(group).font(small.clone()).color(theme::text_hint()));
                }
                ui.horizontal(|ui| {
                    let keys: Vec<String> = keymap.keys(&action.id).iter().map(Shortcut::display).collect();
                    let capturing = state.capture.as_deref() == Some(action.id.as_str());
                    let left = egui::Layout::left_to_right(egui::Align::Center);
                    ui.allocate_ui_with_layout(Vec2::new(300.0, 18.0), left, |ui| {
                        ui.set_min_width(300.0);
                        let text = egui::RichText::new(&action.label).font(small.clone()).color(theme::text_primary());
                        ui.add(egui::Label::new(text).truncate());
                    });
                    let shown = if capturing {
                        "Press keys".to_string()
                    } else if keys.is_empty() {
                        "-".into()
                    } else {
                        keys.join(", ")
                    };
                    let color = if capturing { theme::accent() } else { theme::text_value() };
                    ui.allocate_ui_with_layout(Vec2::new(130.0, 18.0), left, |ui| {
                        ui.set_min_width(130.0);
                        let text = egui::RichText::new(shown).font(theme::mono_font(theme::TEXT_SMALL)).color(color);
                        ui.add(egui::Label::new(text).truncate());
                    });
                    let change = ui.add(egui::Button::new(egui::RichText::new("Change").font(small.clone())));
                    let label = format!("Change keys of {}", action.id);
                    change.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                    if change.clicked() {
                        state.capture = Some(action.id.clone());
                    }
                    let clear = ui.add(egui::Button::new(egui::RichText::new("Clear").font(small.clone())));
                    let label = format!("Clear keys of {}", action.id);
                    clear.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                    if clear.clicked() && !keys.is_empty() {
                        keymap.set(&action.id, Vec::new());
                        changed = true;
                    }
                });
            }
            if listed == 0 {
                ui.label(
                    egui::RichText::new("No action matches the filter").font(small.clone()).color(theme::text_hint()),
                );
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

/// Folders, scanning, what was found in a line, and the button that opens the plug-in manager,
/// which lists it all. Returns what the user asked for.
fn plugin_section(
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
        _ => "CLAP and VST3",
    };

    // Folders: each on or off for scans, with the format taken from it; the added ones can be
    // removed.
    let r = line(y, 18.0);
    widgets::text_in(painter, r, Align2::LEFT_CENTER, "Folders", theme::ui_font(theme::TEXT), theme::text_primary());
    for folder in &plugins.folders {
        let r = line(y, 18.0);
        let check = Rect::from_min_size(Pos2::new(r.left(), r.top() + 1.0), Vec2::splat(16.0));
        let mut enabled = folder.enabled;
        // Off while a scan runs: its result replaces the library, switches too.
        let response = ui.put(check, |ui: &mut egui::Ui| {
            ui.add_enabled(!plugins.scanning, egui::Checkbox::without_text(&mut enabled))
        });
        let label = format!("Scan {}", folder.path);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, folder.enabled, &label));
        if response.changed() {
            request = Some(PluginRequest::SetFolderEnabled(folder.path.clone(), enabled));
        }
        let right = if folder.standard { r.right() } else { r.right() - 68.0 };
        let text = Rect::from_min_max(Pos2::new(check.right() + 6.0, r.top()), Pos2::new(right - 96.0, r.bottom()));
        let color = if folder.enabled { theme::text_secondary() } else { theme::text_hint() };
        let shown = if folder.standard { format!("{}  (standard)", folder.path) } else { folder.path.clone() };
        widgets::text_in(painter, text, Align2::LEFT_CENTER, &shown, small.clone(), color);
        let format = Rect::from_min_max(Pos2::new(right - 92.0, r.top()), Pos2::new(right - 4.0, r.bottom()));
        widgets::text_in(
            painter,
            format,
            Align2::RIGHT_CENTER,
            format_name(&folder.format),
            small.clone(),
            theme::text_hint(),
        );
        if !folder.standard {
            let remove =
                Rect::from_min_max(Pos2::new(r.right() - 64.0, r.top() + 1.0), Pos2::new(r.right(), r.bottom() - 1.0));
            let response = widgets::button_area(
                ui,
                remove,
                Id::new(("remove-folder", &folder.path)),
                format!("Remove {}", folder.path),
            );
            let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
            painter.rect_filled(remove, 0.0, fill);
            widgets::text_in(painter, remove, Align2::CENTER_CENTER, "Remove", small.clone(), theme::text_secondary());
            if response.clicked() {
                request = Some(PluginRequest::RemoveFolder(folder.path.clone()));
            }
        }
    }
    // A folder to add, and the format it holds: VST2 shows, for when it is hosted.
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
            .hint_text("A folder to look in as well")
            .font(small.clone()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Plug-in folder to add"));
    let width = formats.width() / 4.0;
    for (index, (format, title)) in [
        (FolderFormat::Any, "Both"),
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
        let label = if format.hosted() {
            format!("Folder format: {}", format_name(format.name()))
        } else {
            "Folder format: VST2 (not hosted yet)".to_string()
        };
        let response = widgets::button_area(ui, cell, Id::new(("folder-format", index)), label);
        let chosen = plugins.new_format == format;
        if response.clicked() && format.hosted() {
            plugins.new_format = format;
        }
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(cell, 0.0, if chosen { theme::text_value() } else { idle });
        let color = if chosen {
            theme::window_background()
        } else if format.hosted() {
            theme::text_secondary()
        } else {
            theme::text_hint()
        };
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, title, small.clone(), color);
    }
    let response = widgets::button_area(ui, add, Id::new("add-folder"), "Add plug-in folder");
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(add, 0.0, fill);
    widgets::text_in(painter, add, Align2::CENTER_CENTER, "Add", small.clone(), theme::text_primary());
    let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if (response.clicked() || entered) && !plugins.new_folder.trim().is_empty() {
        request = Some(PluginRequest::AddFolder(plugins.new_folder.trim().to_string(), plugins.new_format));
    }
    *y += 8.0;

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
        let response = widgets::button_area(ui, cell, Id::new(("scan", index)), text);
        let base = if plugins.scanning { theme::panel_background() } else { theme::control_background() };
        let fill = motion::mix(
            base,
            theme::control_hover(),
            if plugins.scanning { 0.0 } else { widgets::hover(ui, &response) },
        );
        painter.rect_filled(cell, 0.0, fill);
        let color = if plugins.scanning { theme::text_hint() } else { theme::text_primary() };
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, small.clone(), color);
        if response.clicked() && !plugins.scanning {
            request = Some(PluginRequest::Scan { all });
        }
    }

    // The plug-in manager: every plug-in and every file that gave none, in one table.
    let r = line(y, SETTING_ROW);
    label(painter, r, "Plug-in manager", "Every plug-in found, switched on or off, and why files failed");
    let open = control_rect(r, 2);
    let response = widgets::button_area(ui, open, Id::new("open-plugin-manager"), "Open the plug-in manager");
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(open, 0.0, fill);
    widgets::text_in(painter, open, Align2::CENTER_CENTER, "Manage plug-ins", small, theme::text_primary());
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
    let choice = settings.output_choice();
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
    let picked = output_rows(cx.ui, cx.painter, cx.page, y, cx.settings, cx.about);
    use_output(cx, picked);
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

/// The rows of Audio › Output; returns a new choice.
fn output_rows(
    ui: &Ui,
    painter: &egui::Painter,
    page: Rect,
    y: &mut f32,
    settings: &Settings,
    about: &About,
) -> Option<suhina_session::OutputChoice> {
    let Chosen { choice, default_host, host, device } = chosen(settings, about);
    let row = |y: &mut f32| setting_row(page, y);
    let mut picked: Option<suhina_session::OutputChoice> = None;
    // Host.
    let r = row(y);
    label(painter, r, "Driver", "WASAPI is Windows' own; ASIO talks to the interface's driver directly");
    let mut options: Vec<(String, Option<String>)> =
        vec![(format!("Default ({})", default_host.map_or("none", |h| h.name.as_str())), None)];
    options.extend(about.audio_hosts.iter().map(|h| (h.name.clone(), Some(h.name.clone()))));
    if let Some(host) = choice_dropdown(ui, painter, r, "Driver", &options, &choice.host) {
        picked = choice.with_params(&serde_json::json!({ "host": host.unwrap_or_default() })).ok();
    }
    // Device.
    let r = row(y);
    let note = match (&choice.device, device) {
        (Some(name), None) => format!("{name} is not there; the default output plays"),
        _ => "The output device on that driver".to_string(),
    };
    label(painter, r, "Device", &note);
    let default_device = host.and_then(|h| h.devices.iter().find(|d| d.default)).map_or("none", |d| d.name.as_str());
    let mut options: Vec<(String, Option<String>)> = vec![(format!("Default ({default_device})"), None)];
    if let Some(host) = host {
        options.extend(host.devices.iter().map(|d| (d.name.clone(), Some(d.name.clone()))));
    }
    if let Some(name) = choice_dropdown(ui, painter, r, "Device", &options, &choice.device) {
        picked = choice.with_params(&serde_json::json!({ "device": name.unwrap_or_default() })).ok();
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
                format!("The device's own buffer, from its driver: {min} to {max} frames")
            }
            Some((min, _)) => format!("The device's own buffer; its driver allows {min} frames only"),
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
    label(painter, r, name, &note);
    let mut options: Vec<(String, Option<u32>)> = vec![("Default (1024 frames)".to_string(), None)];
    options.extend(sizes.iter().map(|n| (format!("{n} frames ({:.1} ms)", f64::from(*n) * 1000.0 / rate), Some(*n))));
    if let Some(frames) = choice_dropdown(ui, painter, r, name, &options, &choice.buffer_frames) {
        picked = choice.with_params(&serde_json::json!({ "bufferFrames": frames.unwrap_or(0) })).ok();
    }
    // Exclusive mode, WASAPI only.
    let r = row(y);
    let note = if !wasapi {
        "WASAPI only; ASIO always has the device to itself"
    } else if device.is_some_and(|d| !d.exclusive) {
        "This device does not allow it; it plays shared"
    } else {
        "The device plays only Suhina, with less delay; other programs are not heard on it"
    };
    label(painter, r, "Exclusive mode", note);
    let on_off = [("On", true), ("Off", false)];
    if let Some(on) = segmented(ui, painter, control_rect(r, 2), "Exclusive mode", &on_off, choice.exclusive) {
        picked = choice.with_params(&serde_json::json!({ "exclusive": on })).ok();
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

    picked
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
        None => ("None: no project is playing".to_string(), "-".to_string()),
    };
    label(painter, r, "Output", &output);
    let refresh = Rect::from_min_size(Pos2::new(r.right() - 120.0, r.top() + 1.0), Vec2::new(120.0, SEGMENT));
    let response = widgets::button_area(ui, refresh, Id::new("audio-refresh"), "List the audio devices again");
    let hover = widgets::hover(ui, &response);
    painter.rect_filled(refresh, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    widgets::text_in(
        painter,
        refresh,
        Align2::CENTER_CENTER,
        "List devices again",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
    let r = row(y);
    label(painter, r, "Latency", &format!("Engine to speaker, measured: {latency}"));
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
        None => "-".to_string(),
    };
    label(painter, r, "Key to sound", &key);
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
        None => "-".to_string(),
    };
    label(painter, r, "Input now", &input);
    response.clicked()
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
        let response = widgets::button_area(ui, segment, Id::new(("keymap-preset", *text)), format!("Preset: {text}"));
        let fill = if chosen { theme::control_active() } else { theme::control_background() };
        painter.rect_filled(segment, 0.0, motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response)));
        let color = if chosen { theme::text_primary() } else { theme::text_secondary() };
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
            label(painter, r, "None", "No MIDI controller is connected; one plugged in is found within seconds");
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
        let on_off = [("On", true), ("Off", false)];
        let name = format!("MIDI input {}", port.name);
        if let Some(on) = segmented(ui, painter, control_rect(r, 2), &name, &on_off, port.enabled) {
            picked = Some((port.name.clone(), on));
        }
    }
    picked
}

/// A section's title over a hairline, as the other pages draw it.
fn section_title(painter: &egui::Painter, page: Rect, y: &mut f32, name: &str) {
    let rect = Rect::from_min_size(Pos2::new(page.left(), *y), Vec2::new(page.width(), SIDEBAR_ROW));
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT_SMALL), theme::text_hint());
    painter.hline(page.x_range(), rect.bottom() + 3.5, theme::hairline(theme::border()));
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
    let field = Rect::from_min_size(
        Pos2::new(row.right() - DROPDOWN_FIELD, row.top() + 1.0),
        Vec2::new(DROPDOWN_FIELD, SEGMENT + 2.0),
    );
    let shown = options.iter().find(|(_, v)| v == current).map_or("-", |(text, _)| text.as_str());
    let response = widgets::button_area(ui, field, Id::new(("audio-setting", name)), format!("{name}: {shown}"));
    widgets::dropdown(ui, painter, field, &response, shown);
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

/// A setting's name and a note under it, on the left of its row.
fn label(painter: &egui::Painter, row: Rect, name: &str, note: &str) {
    let top = Rect::from_min_size(row.min, Vec2::new(row.width(), 16.0));
    let bottom = Rect::from_min_size(Pos2::new(row.left(), top.bottom()), Vec2::new(row.width() - LABEL_WIDTH, 14.0));
    widgets::text_in(painter, top, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), theme::text_primary());
    widgets::text_in(
        painter,
        bottom,
        Align2::LEFT_CENTER,
        note,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
}

/// Where a segmented control with `parts` parts sits on the right of a row.
fn control_rect(row: Rect, parts: usize) -> Rect {
    let width = 64.0 * parts as f32;
    Rect::from_min_size(Pos2::new(row.right() - width, row.top() + 1.0), Vec2::new(width, SEGMENT))
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
        let response = widgets::button_area(ui, cell, Id::new(("setting", name, index)), format!("{name}: {text}"));
        if response.clicked() && *value != current {
            picked = Some(*value);
        }
        let selected = motion::toward(ui.ctx(), response.id.with("selected"), *value == current, motion::VALUE);
        let hover = widgets::hover(ui, &response);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), hover);
        painter.rect_filled(cell, 0.0, motion::mix(idle, theme::text_value(), selected));
        let color = motion::mix(theme::text_secondary(), theme::window_background(), selected);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_now_says_which_chosen_device_it_waits_for() {
        let info = suhina_session::OutputInfo {
            host: "WASAPI".into(),
            device: "Speakers".into(),
            sample_rate: 48_000,
            engine_block: 512,
            ..Default::default()
        };
        let usb = suhina_session::OutputChoice { device: Some("USB Audio".into()), ..Default::default() };
        assert_eq!(now_playing(&info, &usb), "Speakers (WASAPI, 48000 Hz, 512 frames)");
        let waiting = suhina_session::OutputInfo { fallback: true, ..info };
        assert_eq!(now_playing(&waiting, &usb), "Speakers (WASAPI, 48000 Hz, 512 frames), until USB Audio is back");
        let asio = suhina_session::OutputChoice { host: Some("ASIO".into()), ..Default::default() };
        assert!(now_playing(&waiting, &asio).ends_with("until ASIO is back"));
    }
}
