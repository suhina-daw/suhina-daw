// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: tabs for the open projects (ADR 0007), the control bar, the Session or
//! Arrangement view and the status bar around the active project.
//! Every change goes through `Session::execute` (AGENTS.md rule 6); the window only reads the
//! project and the engine's monitor to draw them.

pub mod loading_dialog;
pub mod missing_files;
pub mod project_dialog;
pub mod splash;
pub mod start_panel;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Align2, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::{CommandKind, PluginStatus, ProjectId, Projects, Session};

use crate::app::project_dialog::{
    AboutChoice, Choice, DialogState, Lists, RecoveryChoice, SaveChoice, SavePrompt, UnsavedChoice,
};
use crate::app::start_panel::{AudioSummary, StartChoice, StartState};
use crate::arrangement::arrangement_view::{self, ArrangementViewState};
use crate::arrangement::timeline::{Folds, Timeline};
use crate::browser::browser_view;
use crate::browser::{BrowserState, ResultsMode};
use crate::detail::detail_view::{self, ClipRef};
use crate::detail::follow_view;
use crate::detail::piano_roll::{self, PianoRollState};
use crate::interaction::drop::{self, BrowserItem, DropProbe, DropTarget};
use crate::interaction::grid::{Grid, SlotState};
use crate::interaction::pick::{PickField, PickRequest};
use crate::interaction::status::Status;
use crate::kit::motion;
use crate::kit::theme::{self, PAD};
use crate::kit::widgets::{self, Action, Drag};
use crate::session::session_view::{self, SessionViewState, Step};
use crate::settings::keymap::Keymap;
use crate::settings::settings_view::{self, About, PageState, PluginRequest, PluginsView, Sources};
use crate::settings::themes::{FileAction, ThemeEntry, Themes};
use crate::settings::{
    AtStart, DeviceLayout, EditorPlacement, MixerControls, OnOpen, PianoRollTop, PlayFrom, Settings, Side,
};

/// Launch quantisation choices, as `launcher.setQuantization` takes them; the Arrangement
/// view's grid follows the same value.
const QUANTIZATION: [&str; 14] = [
    "none", "8 bars", "4 bars", "2 bars", "1 bar", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32",
];
/// The quantisation of a session before anything is set.
const DEFAULT_QUANTIZATION: &str = "1 bar";

/// Height of the tab row, shown while two or more projects are open.
const TABS: f32 = 22.0;
/// How many recent projects File › Open recent lists.
const RECENT_IN_MENU: usize = 10;
/// The detail area's height in a row, by default, and the splitter between it and the view.
const DETAIL_HEIGHT: f32 = 200.0;
/// The least the view keeps above the detail area; pulled past it, the piano roll goes to the top.
const MIN_VIEW: f32 = 160.0;
const SPLITTER: f32 = 6.0;

/// How dark the window gets while a project file is dragged over it.
const DROP_DIM: f32 = 0.45;

/// How a project the user asked to open is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAs {
    NewTab,
    /// In place of the active project, which is closed.
    Replace,
}

/// A new project the user asked for, for the app to make and open in a new tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewProject {
    Blank,
    /// An untitled project made from this template file.
    Template(PathBuf),
}

/// A finished plug-in scan: the library with its new catalog, and how many files were read and
/// failed, or why the scan could not run.
struct ScanDone {
    library: suhina_plugins::library::Library,
    report: Result<(usize, usize), String>,
}

/// The handle between the browser and the view.
const BROWSER_HANDLE: f32 = 4.0;
/// How long a carried clip hovers over the other view's name before the window switches to it,
/// in seconds (`docs/spec/ui-session-view.md` › Between the views).
const SWITCH_HOVER: f64 = 0.5;

/// A drop whose samples are being decoded on another thread before its commands run.
struct PendingDrop {
    commands: Vec<(&'static str, serde_json::Value)>,
    /// Each decoded file (a Stems file once per audio track, with its stream number).
    decoded: std::sync::mpsc::Receiver<Vec<DecodedDrop>>,
}

type DecodedDrop = (PathBuf, Option<u16>, std::sync::Arc<suhina_engine::AudioData>, crate::kit::waveform::Peaks);

/// A project's view state, kept while another project's tab is showing.
#[derive(Default)]
struct Stash {
    view: SessionViewState,
    arrangement: Option<ArrangementViewState>,
    selected_device: Option<(TrackId, usize)>,
    revision: u64,
    timeline: Option<(u64, Folds, Timeline)>,
    waveforms: std::sync::Arc<crate::kit::waveform::Waveforms>,
}

/// A clip carried from one view to the other with the mouse: where it comes from, and its name
/// for the label beside the pointer.
#[derive(Debug, Clone, PartialEq)]
struct Carry {
    from: CarryFrom,
    name: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CarryFrom {
    Arrangement {
        track: TrackId,
        index: usize,
    },
    /// Scene from 0.
    Slot {
        track: TrackId,
        scene: usize,
    },
}

/// The main view; Tab switches between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Session,
    Arrangement,
}

/// The Settings dialog at most: the page list, the page and their margins wide, and tall enough
/// for the longest page on a common screen.
const SETTINGS_SIZE: Vec2 = Vec2::new(772.0, 720.0);

/// A project with unsaved changes the user is leaving, and what happens once they answered.
#[derive(Debug, Clone, PartialEq)]
struct Leaving {
    project: ProjectId,
    then: AfterLeaving,
}

#[derive(Debug, Clone, PartialEq)]
enum AfterLeaving {
    /// Close its tab.
    Close,
    /// Open this file in its place.
    Replace(PathBuf),
    /// Quit; the projects already answered for are not asked about again.
    Quit(Vec<ProjectId>),
}

/// A folder's packs ([docs/spec/nam-amp-and-cab.md] › Folders › Packs): its subfolders, each
/// with its `extension` files name-sorted (the pack is skipped when it holds none), then the
/// files beside the packs as `(entered directly)`. At most 2000 files a pack. The folder is
/// re-read only when `folder_writes` changed; a pack, when its own writes changed.
fn scan_sets(
    folder: &std::path::Path,
    extension: &str,
    previous: &[crate::detail::nam_panel::Set],
    pack_writes: &mut HashMap<String, Option<std::time::SystemTime>>,
) -> Vec<crate::detail::nam_panel::Set> {
    const CAP: usize = 2000;
    let files_of = |dir: &std::path::Path| -> Vec<(String, String)> {
        let mut entries =
            std::fs::read_dir(dir).map(|entries| entries.flatten().collect::<Vec<_>>()).unwrap_or_default();
        entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
        entries
            .into_iter()
            .filter_map(|entry| {
                let path = entry.path();
                let right = path.is_file()
                    && path.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(extension));
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                right.then_some((path.display().to_string(), name))
            })
            .take(CAP)
            .collect()
    };
    let mut packs: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut folders_here: Vec<std::path::PathBuf> = std::fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect())
        .unwrap_or_default();
    folders_here
        .sort_by_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default());
    for pack_folder in folders_here {
        let name = pack_folder.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let writes = std::fs::metadata(&pack_folder).ok().and_then(|m| m.modified().ok());
        let key = pack_folder.display().to_string();
        let scan = pack_writes.get(&key) != Some(&writes);
        pack_writes.insert(key, writes);
        let files = if scan {
            files_of(&pack_folder)
        } else {
            previous.iter().find(|pack| pack.name == name).map(|pack| pack.files.clone()).unwrap_or_default()
        };
        if let Some(files) = (!files.is_empty()).then_some(files) {
            packs.push((name, files));
        }
    }
    let loose = files_of(folder);
    if !loose.is_empty() {
        packs.push((crate::detail::nam_panel::LOOSE_PACK.to_owned(), loose));
    }
    packs.into_iter().map(|(name, files)| crate::detail::nam_panel::Set { name, files }).collect()
}

/// A device's file path (its state as UTF-8), when it is a file-running one
/// ([docs/spec/nam-amp-and-cab.md]).
fn device_file_path(device: &suhina_engine::model::Device) -> Option<String> {
    let suhina_engine::model::DeviceKind::Builtin(id) = &device.kind else { return None };
    if !suhina_device_registry::is_file_device(id) {
        return None;
    }
    device
        .state
        .as_deref()
        .and_then(|state| std::str::from_utf8(state).ok())
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

/// How long after the level meters last moved the window keeps looking for new levels.
const METERS_SETTLE_SECONDS: f64 = 0.3;

/// The NAM folders' packs the panels list ([docs/spec/nam-amp-and-cab.md] › Folders › Packs),
/// with each folder's writes when they were read and each pack's files keyed by the pack's
/// path with its writes.
#[derive(Default)]
struct NamCache {
    models: Vec<crate::detail::nam_panel::Set>,
    irs: Vec<crate::detail::nam_panel::Set>,
    models_writes: Option<std::time::SystemTime>,
    irs_writes: Option<std::time::SystemTime>,
    /// A pack's write time by its folder's path, so a new capture in one pack re-reads only
    /// that pack.
    pack_writes: HashMap<String, Option<std::time::SystemTime>>,
}

/// The window's state.
pub struct App {
    /// When the level meters last showed something new (`ui.input` time).
    meters_moved_at: f64,
    projects: Projects,
    /// The project whose view state is in the fields below; others are in `stashes`.
    shown_project: Option<ProjectId>,
    stashes: HashMap<ProjectId, Stash>,
    /// A dropped file waiting for the user to choose a new tab or replace.
    open_prompt: Option<PathBuf>,
    /// A project with unsaved changes waiting for Save, Don't save or Cancel before it is closed,
    /// replaced or the app quits.
    unsaved_prompt: Option<Leaving>,
    /// What to do once the project is saved, when Save in that question first asks for a name.
    save_then: Option<Leaving>,
    /// Every project with unsaved changes was answered for: the window may close.
    quit_confirmed: bool,
    /// Close the window on the next frame.
    quit_now: bool,
    /// The active project's missing audio files, their bar and panel.
    missing: crate::app::missing_files::MissingFiles,
    remember_choice: bool,
    /// A project that became active and needs an audio output.
    player_request: Option<ProjectId>,
    /// Projects closed since the app last asked, so it can let go of their audio.
    closed: Vec<ProjectId>,
    shown: View,
    view: SessionViewState,
    arrangement: ArrangementViewState,
    status: Status,
    tempo_drag: Option<f64>,
    open_request: Option<(PathBuf, OpenAs)>,
    /// Counts changes to the project, so views derived from it are rebuilt only when it changed.
    revision: u64,
    /// The Arrangement view's data, the revision it was built from and the folds it was built
    /// with: folding a group changes what the lanes are, without changing the project.
    timeline: Option<(u64, Folds, Timeline)>,
    /// Waveforms of the open project's audio files.
    waveforms: std::sync::Arc<crate::kit::waveform::Waveforms>,
    /// The plug-in library as Settings shows it, and a scan running in the background.
    plugins: PluginsView,
    plugins_read: bool,
    scan: Option<std::sync::mpsc::Receiver<ScanDone>>,
    /// Which keys run which action, and whether that changed and should be saved.
    keymap: Keymap,
    keymap_changed: bool,
    /// The detail area's Clip / Devices switch: for a clip with an editor, the editor or its
    /// track's devices (`docs/spec/ui-clip-editor.md` › Where it opens). Window state.
    detail_showing: detail_view::Showing,
    /// The device selected in the detail area (its panel's click): the track and the device's
    /// index, for the Delete key to take off the track. Window state.
    selected_device: Option<(TrackId, usize)>,
    /// A device panel was the last thing clicked (a click in the view clears it): Delete and the
    /// mute key act on the selected device then, even while a clip is selected too.
    device_focus: bool,
    /// The devices picked together (Ctrl or Shift clicks), on the selected device's track; the
    /// strips' lists and the chain light them, and Delete, copy and paste act on them.
    device_picks: Vec<(TrackId, usize)>,
    /// Actions picked outside the window (the macOS menu bar), run at the start of the next frame.
    queued: Vec<String>,
    /// A device removal awaiting its undo or redo: the track, the index it left, how long the
    /// chain was, and where the selection pointed before it followed the removal. Window state.
    pending_device_remove: Option<(TrackId, usize, usize, usize)>,
    /// The detail area is showing, and its height (in a row) or width (stacked).
    detail_open: bool,
    detail_height: f32,
    /// How tall the device chain's panels were last frame, when the detail area showed only the
    /// chain: the area is kept no taller.
    detail_cap: Option<f32>,
    /// A clip being renamed (Rename in its menu, Ctrl+R): the command's target, the name typed
    /// so far, and whether the field has had the keyboard yet.
    rename: Option<(serde_json::Value, String, bool)>,
    /// The project switcher Ctrl+Tab opens: the open project picked so far (its place among
    /// them). It switches when Ctrl is let go, as Studio One's and most programs' do.
    tab_switcher: Option<usize>,
    /// The Arrangement's clip selection last frame: when it changes while stopped, the play
    /// start marker goes to its first clip.
    clips_before: Vec<(TrackId, usize)>,
    /// The piano roll is pulled to the top (`docs/spec/ui-clip-editor.md` › Pulled to the top),
    /// and the detail area's height a splitter drag has reached so far, unclamped.
    detail_max: bool,
    detail_drag: Option<f32>,
    /// The detail area was closed as an editor (the splitter dragged down, the toolbar's ×, View ›
    /// Hide editor): selecting a clip opens it again. And the clip shown last frame, to tell when
    /// another is selected (`docs/spec/ui-clip-editor.md` › Least height and closing).
    editor_closed: bool,
    last_clip: Option<ClipRef>,
    /// The piano roll's own grid value, used unless Settings has it follow the control bar's.
    piano_grid: crate::detail::piano_roll_grid::PianoGrid,
    detail_width: f32,
    page: PageState,
    settings: Settings,
    /// The settings changed and should be saved.
    settings_changed: bool,
    /// The Settings page is showing instead of the view.
    settings_open: bool,
    /// The plug-in loading dialog was sent to the background for the project shown now.
    plugin_dialog_hidden: bool,
    about: About,
    /// When Settings › Audio last listed the MIDI ports, so a controller plugged in shows up.
    midi_listed: Option<std::time::Instant>,
    /// The themes to pick from; the settings say which is in use.
    themes: Themes,
    /// The project dialog (File › New or open) while it shows, and what it lists.
    project_dialog: Option<DialogState>,
    project_lists: Lists,
    /// The dialog or the start panel opened and their lists should be read again, by the app.
    lists_wanted: bool,
    /// The File menu showed last frame (File › Open recent reads its list as the menu opens).
    file_menu_open: bool,
    /// The start panel, shown while no project is open.
    start: StartState,
    /// The start panel's lists were asked for since it last appeared.
    start_lists: bool,
    /// A project the user took off the recent list, for the app to forget.
    forget_request: Option<PathBuf>,
    /// A system file or folder dialog for the app to show.
    pick_request: Option<PickRequest>,
    /// The name and folder asked for when a project is saved under a new name or as a template.
    save_prompt: Option<SavePrompt>,
    /// Help › About Suhina is open.
    about_open: bool,
    new_request: Option<NewProject>,
    /// A project file saved since the app last asked, for its recent list.
    saved_path: Option<PathBuf>,
    /// Where projects are saved and templates kept when the settings leave them empty.
    default_folders: (PathBuf, PathBuf),
    /// The NAM models' and IR folders while the settings leave them empty.
    nam_default_folders: (PathBuf, PathBuf),
    /// The NAM folders' files, read again when either folder's writes change
    /// ([docs/spec/nam-amp-and-cab.md] › Folders).
    nam_cache: NamCache,
    /// The window's title as last set: the active project, marked when it has unsaved changes.
    window_title: String,
    /// The browser panel (`docs/spec/browser.md`), the same for every project.
    pub browser: BrowserState,
    /// The sample map, in the browser's results, large in the main area or in a window of its own.
    pub sample_map: crate::browser::sample_map_panel::Panel,
    /// The map's own window: where it opened (kept while it is open, so the window is not moved
    /// back while the user moves it), a new place waiting to be saved, and a drag that started in
    /// it, which the main window finishes.
    map_window: MapWindow,
    pending_drop: Option<PendingDrop>,
    /// Where the detail area is this frame and the track it shows, for drops on its devices.
    detail_drop: Option<(Rect, TrackId)>,
    /// The piano roll a selected MIDI clip shows in the detail area.
    piano_roll: PianoRollState,
    /// Backups a crash left, offered one at a time (`docs/spec/backups.md` › The offer on
    /// start); the one chosen to open, and a backup to show in the file manager, for the app.
    recovery: Vec<suhina_session::backup::Leftover>,
    recovery_open: Option<suhina_session::backup::Leftover>,
    reveal: Option<PathBuf>,
    /// The program's RAM (resident), as last measured, and when to measure again: the control
    /// bar shows it and once a second is often enough for a number that moves that slowly.
    ram: Option<u64>,
    next_ram: Option<f64>,
    /// A clip carried between the views, and the view whose name it hovers over since when.
    carry: Option<Carry>,
    switch_hover: Option<(View, f64)>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            projects: Projects::new(),
            shown_project: None,
            stashes: HashMap::new(),
            open_prompt: None,
            unsaved_prompt: None,
            save_then: None,
            quit_confirmed: false,
            quit_now: false,
            missing: crate::app::missing_files::MissingFiles::default(),
            remember_choice: false,
            player_request: None,
            closed: Vec::new(),
            shown: View::Session,
            view: SessionViewState::default(),
            meters_moved_at: 0.0,
            arrangement: ArrangementViewState::default(),
            status: Status::Hint(start_panel::DROP_HINT.into()),
            tempo_drag: None,
            open_request: None,
            revision: 0,
            timeline: None,
            waveforms: std::sync::Arc::default(),
            plugins: PluginsView::default(),
            plugins_read: false,
            scan: None,
            keymap: Keymap::default(),
            keymap_changed: false,
            detail_showing: detail_view::Showing::Clip,
            selected_device: None,
            device_focus: false,
            device_picks: Vec::new(),
            queued: Vec::new(),
            pending_device_remove: None,
            detail_open: true,
            detail_height: DETAIL_HEIGHT,
            detail_cap: None,
            rename: None,
            tab_switcher: None,
            clips_before: Vec::new(),
            detail_max: false,
            detail_drag: None,
            editor_closed: false,
            last_clip: None,
            piano_grid: crate::detail::piano_roll_grid::PianoGrid::default(),
            detail_width: detail_view::STACK_WIDTH,
            page: PageState::default(),
            settings: Settings::default(),
            settings_changed: false,
            settings_open: false,
            plugin_dialog_hidden: false,
            about: About::default(),
            midi_listed: None,
            themes: Themes::default(),
            project_dialog: None,
            project_lists: Lists::default(),
            lists_wanted: false,
            file_menu_open: false,
            start: StartState::default(),
            start_lists: false,
            forget_request: None,
            pick_request: None,
            save_prompt: None,
            about_open: false,
            new_request: None,
            saved_path: None,
            default_folders: (PathBuf::from("Projects"), PathBuf::from("Templates")),
            nam_default_folders: (PathBuf::from("NAM models"), PathBuf::from("IRs")),
            nam_cache: NamCache::default(),
            window_title: String::new(),
            browser: BrowserState::default(),
            sample_map: crate::browser::sample_map_panel::Panel::default(),
            map_window: MapWindow::default(),
            pending_drop: None,
            detail_drop: None,
            piano_roll: PianoRollState::default(),
            recovery: Vec::new(),
            recovery_open: None,
            reveal: None,
            ram: None,
            next_ram: None,
            carry: None,
            switch_hover: None,
        }
    }

    /// Uses `library` for the browser and the `library.*` commands (ADR 0012); the app gives
    /// it Suhina's settings folder and starts it.
    pub fn set_file_library(&mut self, library: suhina_library::Library) {
        self.projects.set_file_library(library);
        self.browser.relist();
    }

    /// The open projects, to change (the browser's library, for the app).
    pub fn projects_mut(&mut self) -> &mut Projects {
        &mut self.projects
    }

    /// Puts a project opened or saved first among the browser's recent projects.
    pub fn record_project(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Err(e) =
            self.projects.file_library_mut().record(suhina_library::Group::Projects, &path.display().to_string())
        {
            self.status = Status::Error(e);
        }
    }

    /// A window whose `plugins.*` commands use `library` instead of the one in Suhina's
    /// settings folder (tests, portable setups).
    pub fn with_library(library: suhina_plugins::library::Library) -> Self {
        Self { projects: Projects::with_library(library), ..Self::new() }
    }

    /// Shows `session`, replacing the active project. `warnings` are what the importer skipped.
    pub fn set_session(&mut self, session: Session, warnings: &[String]) {
        self.open_project(session, warnings, OpenAs::Replace);
    }

    /// Opens `session` in a new tab or in place of the active project, and makes it active.
    /// Returns its id, or `None` when no more projects can be open (the status bar says why).
    pub fn open_project(&mut self, session: Session, warnings: &[String], how: OpenAs) -> Option<ProjectId> {
        if how == OpenAs::Replace
            && let Some(active) = self.projects.active()
            && self.projects.close(active).is_ok()
        {
            self.stashes.remove(&active);
            self.closed.push(active);
            if self.shown_project == Some(active) {
                self.shown_project = None;
            }
        }
        self.stash_shown();
        let name = session
            .source()
            .and_then(|p| p.file_name())
            .map_or("the project".into(), |n| n.to_string_lossy().to_string());
        self.status = match warnings.len() {
            0 => Status::Done(format!("Opened {name}")),
            1 => Status::Hint(format!("Opened {name}: {}", warnings.first().map_or("", String::as_str))),
            n => Status::Hint(format!(
                "Opened {name} with {n} warnings; the first: {}",
                warnings.first().map_or("", String::as_str)
            )),
        };
        if session.player().is_none() {
            self.status = Status::Hint(format!("Opened {name} without audio output; launching needs one"));
        }
        let id = match self.projects.add(session, None) {
            Ok(id) => id,
            Err(e) => {
                self.status = Status::Error(e.0);
                return None;
            }
        };
        self.restore(id, Stash::default());
        self.plugin_dialog_hidden = false;
        Some(id)
    }

    /// Puts the shown project's view state aside, under its id.
    fn stash_shown(&mut self) {
        let Some(id) = self.shown_project.take() else { return };
        if self.projects.get(id).is_none() {
            return;
        }
        let stash = Stash {
            view: std::mem::take(&mut self.view),
            arrangement: Some(std::mem::take(&mut self.arrangement)),
            selected_device: self.selected_device.take(),
            // The removal is another project's: its undo and redo say nothing here.
            revision: self.revision,
            timeline: self.timeline.take(),
            waveforms: std::mem::take(&mut self.waveforms),
        };
        self.pending_device_remove = None;
        self.stashes.insert(id, stash);
    }

    /// Shows project `id` with `stash` as its view state; the window's own preferences (the
    /// header column's side and width) carry over.
    fn restore(&mut self, id: ProjectId, stash: Stash) {
        let (headers_left, header_width) = (self.arrangement.headers_left, self.arrangement.header_width);
        self.view = stash.view;
        self.arrangement = stash.arrangement.unwrap_or_default();
        self.arrangement.headers_left = headers_left;
        self.arrangement.header_width = header_width;
        self.selected_device = stash.selected_device;
        self.revision = stash.revision.wrapping_add(1);
        self.timeline = stash.timeline;
        self.waveforms = stash.waveforms;
        self.shown_project = Some(id);
    }

    /// Follows a change of the active project (a tab switched or closed): swaps the view state
    /// and asks the app for an audio output when the newly active project has none.
    fn follow_active(&mut self) {
        let active = self.projects.active();
        if active == self.shown_project {
            return;
        }
        self.stash_shown();
        for gone in self.stashes.keys().copied().filter(|id| self.projects.get(*id).is_none()).collect::<Vec<_>>() {
            self.stashes.remove(&gone);
        }
        match active {
            Some(id) => {
                let stash = self.stashes.remove(&id).unwrap_or_default();
                self.restore(id, stash);
                if self.projects.get(id).is_some_and(|s| s.player().is_none()) {
                    self.player_request = Some(id);
                }
            }
            None => {
                self.shown_project = None;
                self.view = SessionViewState::default();
                self.timeline = None;
            }
        }
    }

    /// The window plug-in editors open over: the native handle of the app's main window (a
    /// Win32 `HWND`, an X11 window id), so they float above it (ADR 0006).
    pub fn set_editor_owner(&mut self, owner: Option<u64>) {
        self.projects.set_editor_owner(owner);
    }

    /// The open projects.
    pub fn projects(&self) -> &Projects {
        &self.projects
    }

    /// A project that became active and needs an audio output, once.
    pub fn take_player_request(&mut self) -> Option<ProjectId> {
        self.player_request.take()
    }

    /// Projects closed since the last call, so the app can let go of their audio.
    pub fn take_closed(&mut self) -> Vec<ProjectId> {
        std::mem::take(&mut self.closed)
    }

    /// Lets project `id` play through `player`.
    pub fn attach_player(&mut self, id: ProjectId, player: suhina_session::Player) {
        if let Some(session) = self.projects.get_mut(id) {
            session.attach_player(player);
        }
    }

    /// Asks to open `path`, as a dropped file does: at once when nothing is open or the
    /// settings say how, otherwise after the user chose in the window.
    pub fn request_open(&mut self, path: PathBuf) {
        if self.projects.is_empty() {
            self.open_request = Some((path, OpenAs::NewTab));
            return;
        }
        match self.settings.on_open {
            OnOpen::Ask => self.open_prompt = Some(path),
            OnOpen::NewTab => self.open_request = Some((path, OpenAs::NewTab)),
            OnOpen::Replace => self.open_as(path, OpenAs::Replace),
        }
    }

    /// Opens `path` as asked; in place of a project with unsaved changes only after the user
    /// answered whether to save them.
    fn open_as(&mut self, path: PathBuf, how: OpenAs) {
        if how == OpenAs::Replace
            && let Some(active) = self.projects.active()
        {
            self.check_plugin_edits(active);
        }
        match self.projects.active() {
            Some(active) if how == OpenAs::Replace && self.unsaved(active) => {
                self.unsaved_prompt = Some(Leaving { project: active, then: AfterLeaving::Replace(path) });
            }
            _ => self.open_request = Some((path, how)),
        }
    }

    fn unsaved(&self, id: ProjectId) -> bool {
        self.projects.get(id).is_some_and(Session::has_unsaved_changes)
    }

    /// Reads the project's running plug-ins' states now, so a change made only in a plug-in's
    /// editor counts as unsaved before the window asks whether to save.
    fn check_plugin_edits(&mut self, id: ProjectId) {
        if let Some(session) = self.projects.get_mut(id) {
            session.check_plugin_edits();
        }
    }

    /// The open projects with unsaved changes, in tab order.
    fn unsaved_projects(&self) -> Vec<ProjectId> {
        self.projects.ids().into_iter().filter(|id| self.unsaved(*id)).collect()
    }

    /// Whether the app is asking about unsaved changes, or for a name to save them under first.
    pub fn unsaved_prompt_open(&self) -> bool {
        self.unsaved_prompt.is_some() || self.save_then.is_some()
    }

    /// Before the window closes: when projects have unsaved changes, keeps it open and asks about
    /// each, then closes it once every one was answered.
    fn guard_quit(&mut self, ctx: &egui::Context) {
        if self.quit_now {
            self.quit_now = false;
            self.quit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !ctx.input(|i| i.viewport().close_requested()) || self.quit_confirmed {
            return;
        }
        if !self.unsaved_prompt_open() {
            for id in self.projects.ids() {
                self.check_plugin_edits(id);
            }
        }
        let Some(first) = self.unsaved_projects().first().copied() else { return };
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if !self.unsaved_prompt_open() {
            self.unsaved_prompt = Some(Leaving { project: first, then: AfterLeaving::Quit(Vec::new()) });
        }
    }

    /// Saves the project being left, then leaves it; asks for a name first when it has no file
    /// of its own yet.
    fn save_and_leave(&mut self, leaving: Leaving) {
        if self.projects.active() != Some(leaving.project) {
            self.run(&Action::new("projects.switch", json!({ "project": leaving.project.0 })));
        }
        match self.own_file(leaving.project) {
            Some(path) => {
                if self.save_to(&path, false) {
                    self.leave(leaving);
                }
            }
            None => {
                self.save_active(true);
                self.save_then = Some(leaving);
            }
        }
    }

    /// Does what leaving the project was for, once its changes were saved or let go.
    fn leave(&mut self, leaving: Leaving) {
        match leaving.then {
            AfterLeaving::Close => self.run(&Action::new("projects.close", json!({ "project": leaving.project.0 }))),
            AfterLeaving::Replace(path) => self.open_request = Some((path, OpenAs::Replace)),
            AfterLeaving::Quit(mut answered) => {
                answered.push(leaving.project);
                let next = self.unsaved_projects().into_iter().find(|id| !answered.contains(id));
                match next {
                    Some(project) => {
                        self.unsaved_prompt = Some(Leaving { project, then: AfterLeaving::Quit(answered) });
                    }
                    None => self.quit_now = true,
                }
            }
        }
    }

    /// The active project's session.
    pub fn session(&self) -> Option<&Session> {
        self.projects.active().and_then(|id| self.projects.get(id))
    }

    pub fn view(&self) -> View {
        self.shown
    }

    /// The clip selected in the Arrangement view: its track and index.
    pub fn selected_clip(&self) -> Option<(TrackId, usize)> {
        self.arrangement.selected_clip()
    }

    /// The Arrangement view's state: what is selected, how far it is scrolled and which groups are
    /// folded. The view draws itself from it, and the window's tests read it.
    pub fn arrangement(&self) -> &ArrangementViewState {
        &self.arrangement
    }

    /// `transport.stop`, with where the playhead goes from the settings.
    fn stop_action(&self) -> Action {
        Action::new("transport.stop", json!({ "return": self.settings.stop.param() }))
    }

    /// Play as the Play button and the play/stop key start it: from the start of the clip
    /// selected in the Arrangement view when the setting **Play from** says so (or `selected`
    /// asks for it whatever the setting), from the play start marker otherwise.
    fn play_action(&self, selected: bool) -> Action {
        let clip =
            (selected || self.settings.play_from == PlayFrom::Clip).then(|| self.selected_arrangement_clip()).flatten();
        match clip {
            Some((track, index)) => {
                Action::new("transport.play", json!({ "from": "clip", "track": track.0, "index": index }))
            }
            None => Action::new("transport.play", json!({})),
        }
    }

    /// The clip selected in the Arrangement view, while that view is shown.
    fn selected_arrangement_clip(&self) -> Option<(TrackId, usize)> {
        if self.shown == View::Arrangement { self.arrangement.selected_clip() } else { None }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Turns animations on or off (a setting; on by default).
    pub fn set_animations(ctx: &egui::Context, on: bool) {
        motion::set_enabled(ctx, on);
    }

    /// Looks for backups a crash left (in the backup folder the projects were given) and offers
    /// them, one at a time.
    pub fn offer_recovery(&mut self) {
        self.recovery = self.projects.leftover_backups();
    }

    /// A backup the user chose to open, for the app to open as an unsaved project tied to its
    /// project file (`Session::recover`).
    pub fn take_recovery_request(&mut self) -> Option<suhina_session::backup::Leftover> {
        self.recovery_open.take()
    }

    /// A backup to show in the file manager.
    pub fn take_reveal_request(&mut self) -> Option<PathBuf> {
        self.reveal.take()
    }

    /// Before the app closes: backs up what quitting needs and waits a little for the backups to
    /// be written. Returns whether they were.
    pub fn finish_backups(&mut self) -> bool {
        self.projects.finish_backups(std::time::Duration::from_secs(5))
    }

    /// Takes in finished backups and writes the timed ones that are due; a failed one is shown in
    /// the status bar. The window looks again when the next one will be due.
    fn tick_backups(&mut self, ctx: &egui::Context) {
        let tick = self.projects.tick_backups();
        if let Some(failed) = tick.notices.last() {
            self.status = Status::Error(failed.clone());
        }
        if let Some(next) = tick.next {
            ctx.request_repaint_after(next + std::time::Duration::from_millis(50));
        }
    }

    /// Uses `settings` from now on, as loaded from the settings file.
    pub fn set_settings(&mut self, ctx: &egui::Context, settings: Settings) {
        self.projects.set_backup_settings(settings.backup, settings.backup_clean_up);
        motion::set_enabled(ctx, settings.animations);
        self.arrangement.headers_left = settings.track_headers == Side::Left;
        if settings.map_window {
            self.sample_map.set_window(true, &mut self.browser);
        }
        self.settings = settings;
        theme::set_palette(ctx, self.themes.palette(&self.settings.theme));
    }

    /// Uses the themes read from the theme files in `folder`, and draws with the one the settings
    /// name (the built-in one when there is no such file).
    pub fn set_themes(&mut self, ctx: &egui::Context, files: Vec<ThemeEntry>, folder: Option<String>) {
        self.themes.set(files, folder);
        theme::set_palette(ctx, self.themes.palette(&self.settings.theme));
    }

    /// Draws with the theme `id` from now on and saves that in the settings, if there is such a
    /// theme.
    pub fn use_theme(&mut self, ctx: &egui::Context, id: &str) {
        if self.themes.get(id).is_some() {
            self.settings.theme = id.to_string();
            self.settings_changed = true;
            theme::set_palette(ctx, self.themes.palette(id));
        }
    }

    /// Where projects are saved and templates kept when the settings leave them empty.
    pub fn set_default_folders(&mut self, projects: PathBuf, templates: PathBuf) {
        self.default_folders = (projects, templates);
    }

    /// Where the NAM models' and IR folders sit when the settings leave them empty
    /// ([docs/spec/nam-amp-and-cab.md]): `Suhina/NAM models` and `Suhina/IRs` in the user's
    /// documents. The app makes both at start when they are not there.
    pub fn set_nam_default_folders(&mut self, models: PathBuf, irs: PathBuf) {
        self.nam_default_folders = (models, irs);
    }

    /// The NAM panels' lists ([docs/spec/nam-amp-and-cab.md] › Folders): the models' and IR
    /// folders' files, read again when a folder's writes change, and the session's shown names
    /// and failure notes for the devices' current files.
    pub fn nam_lists(&mut self) -> crate::detail::nam_panel::Lists {
        let (models_folder, irs_folder) = (self.nam_models_folder(), self.ir_folder());
        let mut cache = std::mem::take(&mut self.nam_cache);
        let mut folders = [
            (&models_folder, "nam", &mut cache.models, &mut cache.models_writes),
            (&irs_folder, "wav", &mut cache.irs, &mut cache.irs_writes),
        ];
        for (folder, extension, sets, cache_writes) in folders.iter_mut() {
            let writes = std::fs::metadata(&**folder).ok().and_then(|m| m.modified().ok());
            let scan = writes != **cache_writes || (**sets).is_empty();
            **cache_writes = writes;
            if scan {
                let previous = std::mem::take(&mut **sets);
                **sets = scan_sets(folder, extension, &previous, &mut cache.pack_writes);
            }
        }
        let lists = crate::detail::nam_panel::Lists {
            models: cache.models.clone(),
            irs: cache.irs.clone(),
            names: self.nam_names(),
            notes: self.nam_notes(),
        };
        self.nam_cache = cache;
        lists
    }

    /// The shown names of the session's NAM devices' current files, by path (a lookup per
    /// device file, no file reads).
    fn nam_names(&self) -> HashMap<String, String> {
        let mut names = HashMap::new();
        if let Some(session) = self.session() {
            let project = session.project();
            for track in project.tracks.iter() {
                let channel = track.channel.and_then(|ch| project.channels.iter().find(|c| c.id == ch));
                for device in channel.iter().flat_map(|c| c.devices.iter()) {
                    if let Some(path) = device_file_path(device) {
                        names.insert(path.clone(), session.device_asset_name(&path).unwrap_or_default());
                    }
                }
            }
        }
        names
    }

    /// The failure notes of the session's NAM devices' current files, by path.
    fn nam_notes(&self) -> HashMap<String, String> {
        let mut notes = HashMap::new();
        if let Some(session) = self.session() {
            let project = session.project();
            for track in project.tracks.iter() {
                let channel = track.channel.and_then(|ch| project.channels.iter().find(|c| c.id == ch));
                for device in channel.iter().flat_map(|c| c.devices.iter()) {
                    if let Some(path) =
                        device_file_path(device).filter(|path| session.device_asset_note(path).is_some())
                        && let Some(note) = session.device_asset_note(&path)
                    {
                        notes.insert(path, note.to_owned());
                    }
                }
            }
        }
        notes
    }

    /// The folder new projects are saved in: the setting, or the default.
    pub fn projects_folder(&self) -> PathBuf {
        match self.settings.projects_folder.trim() {
            "" => self.default_folders.0.clone(),
            folder => PathBuf::from(folder),
        }
    }

    /// The folder the NAM models list from: the setting, or the default
    /// ([docs/spec/nam-amp-and-cab.md]).
    pub fn nam_models_folder(&self) -> PathBuf {
        match self.settings.nam_models_folder.trim() {
            "" => self.nam_default_folders.0.clone(),
            folder => PathBuf::from(folder),
        }
    }

    /// The folder the IRs list from: the setting, or the default.
    pub fn ir_folder(&self) -> PathBuf {
        match self.settings.irs_folder.trim() {
            "" => self.nam_default_folders.1.clone(),
            folder => PathBuf::from(folder),
        }
    }

    /// The folder templates are kept in: the setting, or the default.
    pub fn templates_folder(&self) -> PathBuf {
        match self.settings.templates_folder.trim() {
            "" => self.default_folders.1.clone(),
            folder => PathBuf::from(folder),
        }
    }

    /// Shows the project dialog; `open` puts the keys in its path field (File › Open).
    pub fn open_project_dialog(&mut self, open: bool) {
        self.settings_open = false;
        self.save_prompt = None;
        self.project_dialog = Some(DialogState { path: String::new(), focus_path: open });
        self.lists_wanted = true;
    }

    /// Opens what Settings › General › At start says, for Suhina started without a project:
    /// the start panel, or asks for a new project, one from the start template, or the project
    /// opened last (`last`), which the app takes up as it does the user's own requests
    /// ([`App::take_new_request`], [`App::take_open_request`]). A template or last project that
    /// is not there leaves the start panel, with a hint saying why.
    pub fn open_at_start(&mut self, last: Option<PathBuf>) {
        let missing = match self.settings.at_start {
            AtStart::Panel => None,
            AtStart::Blank => {
                self.new_request = Some(NewProject::Blank);
                return;
            }
            AtStart::Template => match self.start_template() {
                Some(path) if path.is_file() => {
                    self.new_request = Some(NewProject::Template(path));
                    return;
                }
                Some(path) => Some(format!("The template to start from is not there: {}", path.display())),
                None => Some("No template is chosen to start from (Settings › General › Start template)".into()),
            },
            AtStart::Last => match last {
                Some(path) => {
                    self.request_open(path);
                    return;
                }
                None => Some("No project was opened before; choose one to start".into()),
            },
        };
        self.lists_wanted = true;
        if let Some(hint) = missing {
            self.set_status(Status::Hint(hint));
        }
    }

    /// The start panel, in the view's place while no project is open; its lists are read again
    /// each time it appears.
    fn start_panel(&mut self, ui: &mut Ui) {
        if !std::mem::replace(&mut self.start_lists, true) {
            self.lists_wanted = true;
        }
        let (rate, buffer) = (self.settings.audio_sample_rate, self.settings.audio_buffer);
        let audio = AudioSummary {
            device: if self.settings.audio_device.is_empty() {
                "Default".to_string()
            } else {
                self.settings.audio_device.clone()
            },
            sample_rate: if rate > 0 { format!("{rate} Hz") } else { "Default".to_string() },
            buffer: if buffer > 0 { format!("{buffer} samples") } else { "Default".to_string() },
        };
        let area = ui.max_rect();
        let now = std::time::SystemTime::now();
        let version = env!("CARGO_PKG_VERSION");
        match start_panel::show(ui, area, &mut self.start, &self.project_lists, &audio, version, now) {
            Some(StartChoice::NewBlank) => self.new_request = Some(NewProject::Blank),
            Some(StartChoice::NewFromTemplate(path)) => self.new_request = Some(NewProject::Template(path)),
            Some(StartChoice::Open(path)) => self.request_open(path),
            Some(StartChoice::Browse) => self.ask_pick(PickField::OpenProject),
            Some(StartChoice::AudioSettings) => {
                self.page.page = settings_view::Page::Audio;
                self.settings_open = true;
            }
            Some(StartChoice::Forget(path)) => {
                let lists = &mut self.project_lists;
                if let Some(index) = lists.recent.iter().position(|p| *p == path) {
                    lists.recent.remove(index);
                    if index < lists.saved.len() {
                        lists.saved.remove(index);
                    }
                }
                self.forget_request = Some(path);
            }
            None => {}
        }
    }

    /// The template At start makes a new project from: its path, a name in the templates folder.
    fn start_template(&self) -> Option<PathBuf> {
        match self.settings.start_template.trim() {
            "" => None,
            name => {
                let path = PathBuf::from(name);
                Some(if path.is_absolute() { path } else { self.templates_folder().join(path) })
            }
        }
    }

    pub fn project_dialog_open(&self) -> bool {
        self.project_dialog.is_some()
    }

    /// The templates folder to list, once each time the project dialog opens, for the app to
    /// read it and the recent projects and hand them back with [`App::set_project_lists`].
    pub fn take_lists_wanted(&mut self) -> Option<PathBuf> {
        std::mem::take(&mut self.lists_wanted).then(|| self.templates_folder())
    }

    /// The recent projects and the templates the project dialog and the start panel list.
    pub fn set_project_lists(&mut self, recent: Vec<PathBuf>, templates: Vec<PathBuf>) {
        let saved = recent.iter().map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).collect();
        self.project_lists =
            Lists { recent, saved, templates, templates_folder: self.templates_folder().display().to_string() };
    }

    /// Whether the start panel shows: no project is open.
    pub fn start_panel_open(&self) -> bool {
        self.projects.active().is_none()
    }

    /// Asks the app to show the system's dialog for `field`, opening in the folder it names.
    fn ask_pick(&mut self, field: PickField) {
        let folder_of = |value: &str| {
            let path = PathBuf::from(value.trim());
            (!value.trim().is_empty()).then_some(path)
        };
        let start = match field {
            PickField::ProjectsFolder | PickField::OpenProject => Some(self.projects_folder()),
            PickField::NamModelsFolder => Some(self.nam_models_folder()),
            PickField::IrFolder => Some(self.ir_folder()),
            PickField::TemplatesFolder | PickField::StartTemplate => Some(self.templates_folder()),
            PickField::SaveFolder => self.save_prompt.as_ref().and_then(|p| folder_of(&p.folder)),
            PickField::BrowserFolder => None,
        };
        self.pick_request = Some(PickRequest { field, start });
    }

    /// A dialog for the app to show with the system's file or folder picker; what is chosen comes
    /// back through [`App::set_picked`].
    pub fn take_pick_request(&mut self) -> Option<PickRequest> {
        self.pick_request.take()
    }

    /// Puts a path chosen in the system's dialog where it was asked for: a setting, the save
    /// prompt's folder, a project to open, or a folder for the browser.
    pub fn set_picked(&mut self, field: PickField, path: PathBuf) {
        let text = path.display().to_string();
        match field {
            PickField::ProjectsFolder => self.settings.projects_folder = text,
            PickField::NamModelsFolder => self.settings.nam_models_folder = text,
            PickField::IrFolder => self.settings.irs_folder = text,
            PickField::TemplatesFolder => self.settings.templates_folder = text,
            PickField::StartTemplate => self.settings.start_template = text,
            PickField::SaveFolder => {
                if let Some(prompt) = self.save_prompt.as_mut() {
                    prompt.folder = text;
                }
                return;
            }
            PickField::OpenProject => {
                self.project_dialog = None;
                self.request_open(path);
                return;
            }
            PickField::BrowserFolder => {
                match self.projects.execute("library.addFolder", &json!({ "folder": text })) {
                    Ok(_) => {
                        self.browser.relist();
                        self.status = Status::Done(format!("Added {text} to the browser"));
                    }
                    Err(e) => self.status = Status::Error(e.0),
                }
                return;
            }
        }
        self.settings_changed = true;
    }

    /// A project the user took off the recent list on the start panel, for the app to forget.
    pub fn take_forget_request(&mut self) -> Option<PathBuf> {
        self.forget_request.take()
    }

    /// A new project the user asked for, for the app to make and open in a new tab.
    pub fn take_new_request(&mut self) -> Option<NewProject> {
        self.new_request.take()
    }

    /// A project file saved since the app last asked, for the recent projects.
    pub fn take_saved_path(&mut self) -> Option<PathBuf> {
        self.saved_path.take()
    }

    /// Saves the active project: to its own project file (`.suhina`, or the `.locr` it was
    /// opened from), or, the first time and when `ask`, to a name and folder the user gives.
    fn save_active(&mut self, ask: bool) {
        let Some(id) = self.projects.active() else { return };
        match self.own_file(id) {
            Some(path) if !ask => {
                self.save_to(&path, false);
            }
            own => {
                let folder =
                    own.as_deref().and_then(Path::parent).map_or_else(|| self.projects_folder(), Path::to_path_buf);
                let name = self.projects.name(id).unwrap_or("Untitled").to_string();
                self.save_prompt =
                    Some(SavePrompt { template: false, name, folder: folder.display().to_string(), focus: true });
            }
        }
    }

    /// The project's own file (`.suhina`, or the `.locr` it was opened from), once it has been
    /// saved or was opened from one.
    fn own_file(&self, id: ProjectId) -> Option<PathBuf> {
        self.projects.get(id)?.source().filter(|s| suhina_session::is_project_file(s)).map(Path::to_path_buf)
    }

    /// Asks for a name to save the active project as a template under.
    fn save_template(&mut self) {
        let Some(id) = self.projects.active() else { return };
        let name = self.projects.name(id).unwrap_or("Template").to_string();
        let folder = self.templates_folder().display().to_string();
        self.save_prompt = Some(SavePrompt { template: true, name, folder, focus: true });
    }

    /// Runs `project.save` on the active project; a template is a copy. Returns whether it was
    /// saved.
    fn save_to(&mut self, path: &Path, template: bool) -> bool {
        let params = json!({ "path": path.display().to_string(), "copy": template });
        match self.projects.execute("project.save", &params) {
            Ok(saved) => {
                let written = saved["path"].as_str().map_or_else(|| path.to_path_buf(), PathBuf::from);
                let what = if template { "Saved the template" } else { "Saved" };
                self.status = Status::Done(format!("{what} {}", written.display()));
                if !template {
                    self.saved_path = Some(written);
                }
                true
            }
            Err(e) => {
                self.status = Status::Error(e.0);
                false
            }
        }
    }

    /// The project dialog and the save prompt, over the window, and what was chosen in them.
    fn project_dialogs(&mut self, ui: &Ui, window: Rect) {
        let name = self.projects.active().and_then(|id| self.projects.name(id)).unwrap_or("the project").to_string();
        if let Some(crate::app::missing_files::Outcome::Apply(files)) = self.missing.panel(ui, window, &name) {
            self.relink(files);
        }
        if let Some(mut state) = self.project_dialog.take() {
            match project_dialog::show(ui, window, &mut state, &self.project_lists) {
                Some(Choice::Open(path)) => self.request_open(path),
                Some(Choice::NewBlank) => self.new_request = Some(NewProject::Blank),
                Some(Choice::NewFromTemplate(path)) => self.new_request = Some(NewProject::Template(path)),
                Some(Choice::Browse) => {
                    self.project_dialog = Some(state);
                    self.ask_pick(PickField::OpenProject);
                }
                Some(Choice::Close) => {}
                None => self.project_dialog = Some(state),
            }
        }
        if let Some(mut prompt) = self.save_prompt.take() {
            match project_dialog::show_save(ui, window, &mut prompt) {
                Some(SaveChoice::Save(path)) => {
                    let saved = self.save_to(&path, prompt.template);
                    if let Some(leaving) = self.save_then.take()
                        && saved
                    {
                        self.leave(leaving);
                    }
                }
                // Cancelling the name also cancels closing or quitting.
                Some(SaveChoice::Cancel) => self.save_then = None,
                Some(SaveChoice::BrowseFolder) => {
                    self.save_prompt = Some(prompt);
                    self.ask_pick(PickField::SaveFolder);
                }
                None => self.save_prompt = Some(prompt),
            }
        }
        let other_dialog = self.unsaved_prompt.is_some() || self.save_prompt.is_some() || self.project_dialog.is_some();
        if !other_dialog && let Some(offer) = self.recovery.first().cloned() {
            let written = suhina_session::backup::date_time(offer.written);
            let written = written.get(..16).unwrap_or(&written);
            match project_dialog::show_recovery(ui, window, &offer.name(), written, offer.file.is_some()) {
                Some(RecoveryChoice::Open) => {
                    self.projects.forget_backup(&offer.backup);
                    self.recovery.remove(0);
                    self.recovery_open = Some(offer);
                }
                Some(RecoveryChoice::ShowFolder) => self.reveal = Some(offer.backup),
                Some(RecoveryChoice::Ignore) => {
                    self.projects.forget_backup(&offer.backup);
                    self.recovery.remove(0);
                }
                None => {}
            }
        }
        self.tab_switcher_ui(ui.ctx());
        if let Some((target, mut draft, focused)) = self.rename.take() {
            let (mut keep, mut apply) = (true, None);
            // The field sits on the clip's name, where the views drew it last frame: Enter or a
            // click elsewhere renames, Escape leaves the name as it was.
            let track = target.get("track").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let key = match (target.get("index"), target.get("scene")) {
                (Some(index), _) => egui::Id::new((NAME_RECT_CLIP, track, index.as_u64().unwrap_or(0))),
                (None, scene) => {
                    egui::Id::new((NAME_RECT_SLOT, track, scene.and_then(serde_json::Value::as_u64).unwrap_or(0)))
                }
            };
            let now = ui.ctx().cumulative_pass_nr();
            let place = ui
                .ctx()
                .data(|d| d.get_temp::<(Rect, u64)>(key))
                .filter(|(_, pass)| now.saturating_sub(*pass) <= 2)
                .map(|(rect, _)| rect);
            match place {
                None => keep = false,
                Some(rect) => {
                    egui::Area::new(egui::Id::new("inline-rename"))
                        .order(egui::Order::Foreground)
                        .fixed_pos(rect.min)
                        .show(ui.ctx(), |ui| {
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut draft)
                                    .id(egui::Id::new("inline-rename-field"))
                                    .font(theme::ui_font(theme::TEXT_SMALL))
                                    .desired_width(rect.width().max(120.0)),
                            );
                            field.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Clip name")
                            });
                            if !focused {
                                field.request_focus();
                            }
                            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            if escape {
                                keep = false;
                            } else if field.lost_focus() && focused {
                                let mut params = target.clone();
                                params["name"] = json!(draft.trim());
                                apply = Some(Action::new("clip.rename", params));
                                keep = false;
                            }
                        });
                }
            }
            if keep {
                self.rename = Some((target, draft, true));
            }
            if let Some(action) = apply {
                self.run(&action);
            }
        }
        if self.about_open {
            match project_dialog::show_about(ui, window) {
                Some(AboutChoice::Copy) => {
                    let line = project_dialog::version_line();
                    ui.ctx().copy_text(line.clone());
                    self.status = Status::Done(format!("Copied {line}"));
                }
                Some(AboutChoice::Close) => self.about_open = false,
                None => {}
            }
        }
        if let Some(leaving) = self.unsaved_prompt.take() {
            let name = self.projects.name(leaving.project).unwrap_or("Untitled").to_string();
            let doing = match leaving.then {
                AfterLeaving::Close => "closing it",
                AfterLeaving::Replace(_) => "opening another project in its place",
                AfterLeaving::Quit(_) => "quitting",
            };
            match project_dialog::show_unsaved(ui, window, &name, doing) {
                Some(UnsavedChoice::Save) => self.save_and_leave(leaving),
                Some(UnsavedChoice::DontSave) => self.leave(leaving),
                Some(UnsavedChoice::Cancel) => {}
                None => self.unsaved_prompt = Some(leaving),
            }
        }
    }

    /// The window's title: the active project, with a dot while it has unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.projects.active() {
            Some(id) => {
                let name = self.projects.name(id).unwrap_or("Untitled");
                let unsaved = self.projects.get(id).is_some_and(Session::has_unsaved_changes);
                format!("{name}{} - Suhina", if unsaved { " •" } else { "" })
            }
            None => "Suhina".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    /// A theme file read again after it changed on disk; the window redraws in it if it is the
    /// theme in use.
    pub fn put_theme(&mut self, ctx: &egui::Context, entry: ThemeEntry) {
        if self.themes.put(entry) {
            theme::set_palette(ctx, self.themes.palette(&self.settings.theme));
        }
    }

    /// What to do with the theme files, in order, once: themes changed in Settings to write, a
    /// file to rename, the files to read again, a backup or the template to write, a dropped
    /// file to bring in.
    pub fn take_theme_actions(&mut self) -> Vec<FileAction> {
        self.themes.take_actions()
    }

    /// The waveforms of the open project's audio files, once they are decoded.
    pub fn set_waveforms(&mut self, waveforms: crate::kit::waveform::Waveforms) {
        self.waveforms = std::sync::Arc::new(waveforms);
        self.timeline = None;
    }

    /// Uses `keymap` for the keys from now on, as loaded from the keymap file.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
    }

    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The keymap, once after each change, for the app to save.
    pub fn take_changed_keymap(&mut self) -> Option<Keymap> {
        std::mem::take(&mut self.keymap_changed).then(|| self.keymap.clone())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The settings, once after each change, for the app to save.
    pub fn take_changed_settings(&mut self) -> Option<Settings> {
        std::mem::take(&mut self.settings_changed).then(|| self.settings.clone())
    }

    /// Facts the Settings page shows, such as the audio output in use.
    pub fn set_about(&mut self, about: About) {
        self.about = about;
    }

    pub fn settings_open(&self) -> bool {
        self.settings_open
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }

    /// The devices picked now: the picks on the selected device's track, or that device alone.
    fn device_picks(&self) -> Vec<(TrackId, usize)> {
        let Some(primary) = self.selected_device else { return Vec::new() };
        if self.device_picks.contains(&primary) && self.device_picks.iter().all(|(t, _)| *t == primary.0) {
            self.device_picks.clone()
        } else {
            vec![primary]
        }
    }

    /// A device clicked in a strip's list or the chain: picked alone, added or taken out with
    /// Ctrl (Cmd), or with Shift the run from the device picked last, on the same track.
    fn pick_device(&mut self, track: TrackId, index: usize, add: bool, run: bool) {
        let mut picks = self.device_picks();
        let same_track = picks.first().is_some_and(|(t, _)| *t == track);
        if add && same_track {
            if let Some(at) = picks.iter().position(|p| *p == (track, index)) {
                if picks.len() > 1 {
                    picks.remove(at);
                }
            } else {
                picks.push((track, index));
            }
        } else if run
            && same_track
            && let Some((_, from)) = self.selected_device
        {
            picks = (from.min(index)..=from.max(index)).map(|i| (track, i)).collect();
        } else {
            picks = vec![(track, index)];
        }
        self.selected_device = Some((track, index));
        if !picks.contains(&(track, index)) {
            self.selected_device = picks.last().copied();
        }
        self.device_picks = picks;
        self.device_focus = true;
        // A fresh pick says where the user is now: a removal's undo or redo no longer moves the
        // selection for them.
        self.pending_device_remove = None;
    }

    /// The project switcher, while Ctrl+Tab holds it open: the open projects, the one picked
    /// lit. Letting go of Ctrl (Cmd) switches to it, a click on one switches there, Escape
    /// leaves the project as it is.
    fn tab_switcher_ui(&mut self, ctx: &egui::Context) {
        let Some(picked) = self.tab_switcher else { return };
        let ids = self.projects.ids();
        if ids.len() < 2 || picked >= ids.len() {
            self.tab_switcher = None;
            return;
        }
        let (held, escape) = ctx.input(|i| (i.modifiers.command, i.key_pressed(egui::Key::Escape)));
        if escape {
            self.tab_switcher = None;
            return;
        }
        let mut chosen = None;
        egui::Area::new(egui::Id::new("project-switcher"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(360.0);
                    ui.label(egui::RichText::new("Open projects").color(theme::text_hint()).size(theme::TEXT_SMALL));
                    for (place, id) in ids.iter().enumerate() {
                        let name = self.projects.name(*id).unwrap_or("Untitled").to_string();
                        let source = self
                            .projects
                            .get(*id)
                            .and_then(Session::source)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "Not saved yet".into());
                        let lit = place == picked;
                        let text = egui::RichText::new(format!("{name}\n{source}")).color(if lit {
                            theme::text_primary()
                        } else {
                            theme::text_secondary()
                        });
                        let row = ui.add(egui::Button::selectable(lit, text).min_size(Vec2::new(360.0, 34.0)));
                        row.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::Button, true, lit, format!("Switch to {name}"))
                        });
                        if row.clicked() {
                            chosen = Some(place);
                        }
                    }
                });
            });
        // Ctrl let go (or a click): to the picked project.
        let to = chosen.or((!held).then_some(picked));
        if let Some(place) = to {
            self.tab_switcher = None;
            if let Some(id) = ids.get(place)
                && self.projects.active() != Some(*id)
            {
                self.run(&Action::new("projects.switch", json!({ "project": id.0 })));
            }
        }
    }

    /// Runs a window action (`settings::keymap::actions`) on the next frame, as its key would:
    /// for menus outside the window, such as the macOS menu bar.
    pub fn queue_action(&mut self, id: &str) {
        self.queued.push(id.to_owned());
    }

    /// A project file the user asked to open, and how, for the app to open.
    pub fn take_open_request(&mut self) -> Option<(PathBuf, OpenAs)> {
        self.open_request.take()
    }

    /// The shown track's plug-in: the panel's pick when it is one of the track's plug-ins, else
    /// the first running plug-in in the chain. Its editor opens or closes, whichever state it
    /// has (the command and the placement machinery do the rest).
    fn toggle_plugin_editor(&mut self) {
        let shown = match self.shown {
            View::Session => self.view.selected,
            View::Arrangement => self.arrangement.selected_tracks.first().copied(),
        };
        let Some(track) = shown else {
            self.status = Status::Hint("Click a track first".into());
            return;
        };
        // The first running plug-in in the chain is the one the key toggles. (Now that the
        // device panel selects a device, the panel's pick can stand in its place; that comes
        // with the device selection (#554).)
        let target = {
            let Ok(session) = self.projects.session_mut(None) else { return };
            let chain = session
                .project()
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.channel)
                .and_then(|c| session.project().channels.iter().find(|ch| ch.id == c))
                .map_or(0, |c| c.devices.len());
            (0..chain)
                .filter_map(|index| Some((index, session.plugin_report(track, index)?)))
                .find(|(_, report)| report.status == PluginStatus::Running)
                .map(|(index, report)| (index, report.editor_open))
        };
        let Some((index, open)) = target else {
            self.status = Status::Hint("The selected track has no running plug-in".into());
            return;
        };
        let command = if open { "device.closeEditor" } else { "device.openEditor" };
        self.run(&Action::new(command, json!({ "track": track.0, "device": index })));
    }

    /// How many devices `track`'s chain holds, if the track is in the project.
    fn device_count(&self, track: TrackId) -> Option<usize> {
        self.session().and_then(|s| {
            s.project()
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.channel)
                .and_then(|c| s.project().channels.iter().find(|ch| ch.id == c))
                .map(|ch| ch.devices.len())
        })
    }

    /// Runs one command and shows its outcome in the status bar.
    pub fn run(&mut self, action: &Action) {
        if action.id == "ui.renameClip" {
            let mut target = action.params.clone();
            let name = target.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
            if let Some(object) = target.as_object_mut() {
                object.remove("name");
            }
            self.rename = Some((target, name, false));
            return;
        }
        if action.id.starts_with("projects.") {
            if let Err(e) = self.projects.execute(action.id, &action.params) {
                self.status = Status::Error(e.0);
            }
            return;
        }
        // Playing stops a sample previewing in the browser: the two would play over each other.
        if action.id == "transport.play" {
            browser_view::stop_preview(&mut self.browser, &mut self.projects);
        }
        // A critical operation is backed up first, when the settings say so.
        self.projects.back_up_before(action.id);
        let Ok(session) = self.projects.session_mut(None) else { return };
        match session.execute(action.id, &action.params) {
            Ok(result) => {
                // A clip made by a double-click is selected, ready to edit: in the Arrangement
                // the clip, in the Session its slot, so the detail area shows it.
                if action.id == "clip.create"
                    && let Some(track) = result["track"].as_u64().and_then(|t| u32::try_from(t).ok()).map(TrackId)
                {
                    let at = |key: &str| result[key].as_u64().and_then(|i| usize::try_from(i).ok());
                    match (at("index"), at("scene").and_then(|s| s.checked_sub(1))) {
                        (Some(index), _) => self.arrangement.select_clip(track, index),
                        (None, Some(scene)) => self.view.select_slot(track, scene),
                        (None, None) => self.view.selected = Some(track),
                    }
                }
                // Duplicated clips: the copies are selected, so the next Ctrl+D copies them on.
                if action.id == "clip.duplicate"
                    && let Some(made) = result["clips"].as_array()
                {
                    let copies: Vec<(TrackId, usize)> = made
                        .iter()
                        .filter_map(|c| {
                            let track = u32::try_from(c["track"].as_u64()?).ok()?;
                            Some((TrackId(track), usize::try_from(c["index"].as_u64()?).ok()?))
                        })
                        .collect();
                    if !copies.is_empty() {
                        self.arrangement.select_clips(copies);
                    }
                }
                let changes_project = suhina_session::commands()
                    .iter()
                    .find(|c| c.id == action.id)
                    .is_some_and(|c| matches!(c.kind, CommandKind::Edit | CommandKind::History));
                // Arming changes no project, but the track headers show it.
                if changes_project || action.id == "track.setArm" {
                    self.revision += 1;
                }
                self.fold_new_groups(&result);
                if let Some(done) = describe(action) {
                    self.status = Status::Done(done);
                }
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// A Stems file arrives as one group of its stems and its master (`clip.addFiles` reports
    /// the groups it made): the group opens folded, so the drop reads as a single lane until its
    /// triangle is clicked ([`crate::arrangement::arrangement_view`]).
    fn fold_new_groups(&mut self, result: &serde_json::Value) {
        for group in result["groups"].as_array().map(Vec::as_slice).unwrap_or_default() {
            if let Some(id) = group.as_u64().and_then(|n| u32::try_from(n).ok()) {
                self.arrangement.fold_group(TrackId(id));
            }
        }
    }

    /// Keeps the active project's plug-ins in line with it (ADR 0006) and shows what the user
    /// should hear about in the status bar. While the project has plug-ins, the window looks
    /// again a few times a second, so one that starts or crashes is noticed without input.
    fn poll_plugins(&mut self, ctx: &egui::Context) {
        self.projects.set_editor_embedding(self.settings.plugin_editors == EditorPlacement::DeviceView);
        let notices = self.projects.poll_plugins();
        if let Some(last) = notices.last() {
            self.status = Status::Hint(last.clone());
        }
        if self
            .session()
            .is_some_and(|s| s.player().is_some() && !suhina_engine::plugin_devices(s.project()).is_empty())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// While the active project's plug-ins start, a dialog over the window says how far they
    /// have got, until it is sent to the background or nothing is loading any more.
    fn plugin_dialog(&mut self, ui: &Ui, window: Rect) {
        if self.plugin_dialog_hidden || self.settings_open {
            return;
        }
        let Some(id) = self.projects.active() else { return };
        let Some(progress) = self.projects.get(id).map(Session::plugin_progress) else { return };
        if progress.loading.is_empty() {
            return;
        }
        let name = self.projects.name(id).unwrap_or("the project").to_string();
        if loading_dialog::show(ui, window, &name, &progress) {
            self.plugin_dialog_hidden = true;
        }
    }

    /// Draws the window into `ui` and runs what the user asked for.
    pub fn ui(&mut self, ui: &mut Ui) {
        if self.projects.active().is_some() {
            // The start panel reads its lists again when it next appears.
            self.start_lists = false;
        }
        let window = ui.max_rect();
        let mut actions = Vec::new();
        let revision = self.revision;
        ui.ctx().data_mut(|d| {
            d.insert_temp(egui::Id::new(crate::interaction::menus::NAME_FIELD), self.settings.view.menu_name_field)
        });
        self.piano_roll.notes_replace = self.settings.view.notes_replace;
        self.arrangement.time_select_ctrl = self.settings.view.time_select_ctrl;
        self.arrangement.stretch_alt = self.settings.view.stretch_alt;
        // Any press ends a device panel's hold on Delete and the mute key; a click on a panel
        // takes it again when the button comes up, whether or not the detail area is shown.
        if ui.input(|i| i.pointer.primary_pressed()) {
            self.device_focus = false;
        }
        self.follow_active();
        self.poll_scan(ui.ctx());
        self.poll_plugins(ui.ctx());
        self.tick_backups(ui.ctx());
        // The sample map's analysis goes on in the background; take in what it finished.
        self.projects.poll_sound_map();
        let dropped = self.dropped_files(ui);
        keys_for_shortcuts(ui.ctx());
        self.launch_keys(ui, &mut actions);
        for id in std::mem::take(&mut self.queued) {
            self.do_action(&id, &mut actions);
        }
        self.shortcuts(ui, &mut actions);
        self.apply_zoom(ui.ctx());
        self.sample_map_window(ui.ctx(), &mut actions);
        let scale = if self.settings.waveform_db {
            crate::kit::waveform::Scale::Db
        } else {
            crate::kit::waveform::Scale::Linear
        };
        crate::kit::waveform::set_scale(ui.ctx(), scale);

        let grid = self.session().map(|s| {
            let mut grid = Grid::build(s.project(), s.player().map(|p| p.monitor()));
            grid.midi_ports = self.projects.midi_ports().to_vec();
            grid.metronome = s.metronome();
            grid.input_channels =
                s.player().and_then(suhina_session::Player::output_info).map_or(0, |i| i.input_channels);
            for column in &mut grid.columns {
                column.armed = s.is_armed(column.track);
                column.recording = s.recording_slot(column.track);
            }
            grid
        });
        let progress = grid.as_ref().map(|g| self.progress(g)).unwrap_or_default();
        // The CPU load the control bar shows, from the engine's monitor, in per mille.
        let cpu = self.session().and_then(|s| s.player()).map(|p| p.cpu_monitor().load_milli());
        // The program's own RAM, the control bar shows it beside the CPU: measured once a
        // second, not every frame. The plug-in host processes report their own once a second,
        // and their share is added: the number is what Suhina and its hosts use together.
        if self.next_ram.is_none_or(|at| ui.ctx().time() >= at) {
            let own = memory_stats::memory_stats().map(|s| s.physical_mem as u64);
            let mut hosts: Option<u64> = None;
            for bytes in suhina_plugins::host_memory() {
                hosts = Some(hosts.unwrap_or(0).saturating_add(bytes));
            }
            self.ram = own.map_or(hosts, |own| Some(own.saturating_add(hosts.unwrap_or(0)))).or(hosts);
            self.next_ram = Some(ui.ctx().time() + 1.0);
        }
        let ram = self.ram;
        // Space pressed in a plug-in's editor window: play or stop, as Space here does.
        for _ in 0..suhina_plugins::take_transport_keys() {
            self.do_action("transport.toggle", &mut actions);
        }

        egui::Panel::top("menu-bar")
            .exact_size(theme::MENU_BAR)
            .frame(egui::Frame::NONE.fill(theme::panel_background()).inner_margin(egui::Margin::symmetric(4, 0)))
            .show(ui, |ui| self.menu_bar(ui, &mut actions));
        if self.projects.len() >= 2 {
            egui::Panel::top("tabs")
                .exact_size(TABS)
                .frame(egui::Frame::NONE.fill(theme::window_background()))
                .show(ui, |ui| self.tab_row(ui, &mut actions));
        }
        egui::Panel::top("control-bar")
            .exact_size(theme::CONTROL_BAR)
            .frame(egui::Frame::NONE.fill(theme::panel_background()))
            .show(ui, |ui| self.control_bar(ui, grid.as_ref(), cpu, ram, &mut actions));
        egui::Panel::bottom("status-bar")
            .exact_size(theme::STATUS_BAR)
            .frame(egui::Frame::NONE.fill(theme::panel_background()))
            .show(ui, |ui| self.status_bar(ui, grid.as_ref()));
        // Samples and MIDI files from the system: dropped this frame, or still over the window.
        let system = match (self.browser.drag.is_some(), dropped.is_empty()) {
            (true, _) => None,
            (false, false) => Some((dropped, true)),
            (false, true) => Some((self.hovered_files(ui), false)).filter(|(items, _)| !items.is_empty()),
        };
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(theme::border())).show(ui, |ui| {
            let area = ui.available_rect_before_wrap();
            // A 1 px gap shows the line colour between the bars and the view.
            let inner = Rect::from_min_max(area.min + Vec2::new(0.0, 1.0), area.max - Vec2::new(0.0, 1.0));
            self.missing.sync(&mut self.projects, self.revision);
            let missing_bar = self.open_prompt.is_none() && self.missing.bar_shows();
            let prompt_height = if self.open_prompt.is_some() || missing_bar { theme::ROW + 12.0 } else { 0.0 };
            let (prompt, inner) = (
                Rect::from_min_size(inner.min, Vec2::new(inner.width(), prompt_height)),
                Rect::from_min_max(inner.min + Vec2::new(0.0, prompt_height), inner.max),
            );
            if self.open_prompt.is_some() {
                ui.scope_builder(egui::UiBuilder::new().max_rect(prompt), |ui| self.open_prompt_bar(ui));
            } else if missing_bar {
                ui.scope_builder(egui::UiBuilder::new().max_rect(prompt), |ui| self.missing.bar(ui, prompt));
            }
            // With the detail area across the full width, the browser takes its side of what is
            // left above it (Settings › Detail area › Detail area layout).
            let full_width = grid.is_some()
                && self.settings.view.detail_layout == crate::settings::view_settings::DetailLayout::FullWidth
                && self.settings.device_layout != DeviceLayout::Stack;
            // The browser stays hidden on the start panel (nothing to drop into); its setting keeps
            // its value for when a project opens.
            let browser_shown = self.settings.browser_open && !self.start_panel_open();
            let inner = if browser_shown && !full_width { self.browser_panel(ui, inner) } else { inner };
            let mut view_area = inner;
            self.detail_drop = None;
            // A clip selected (or clicked) while the editor was closed by hand opens it again.
            let shown_clip = self.shown_clip();
            let clicked = std::mem::take(&mut self.arrangement.clip_clicked);
            if self.editor_closed
                && !self.detail_open
                && shown_clip.is_some()
                && (clicked || shown_clip != self.last_clip)
            {
                self.show_editor();
            }
            // Selecting another clip, or clicking the selected one, shows its editor again.
            if shown_clip.is_some() && (clicked || shown_clip != self.last_clip) {
                self.detail_showing = detail_view::Showing::Clip;
            }
            self.last_clip = shown_clip;
            let inner = if grid.is_some() && self.detail_open {
                self.detail_area(ui, inner, &mut actions)
            } else {
                self.place_editors(&[]);
                // A hidden piano roll does not keep the keyboard, nor the view to one track, and
                // a note its audition sounds goes off.
                self.piano_roll.focused = false;
                actions.extend(self.piano_roll.stop_audition());
                self.arrangement.focus = None;
                // In a row under the view, the splitter stays along the bottom, to drag it open.
                if grid.is_some() && self.settings.device_layout != DeviceLayout::Stack {
                    self.collapsed_splitter(ui, inner)
                } else {
                    inner
                }
            };
            let inner = if browser_shown && full_width {
                let rest = self.browser_panel(ui, inner);
                view_area = Rect::from_min_max(
                    Pos2::new(rest.left(), view_area.top()),
                    Pos2::new(rest.right(), view_area.bottom()),
                );
                rest
            } else {
                inner
            };
            // A drag from the browser, or files from the system: the views say what is under the
            // pointer.
            let dragged = self.browser.drag.as_ref().or(system.as_ref().map(|(items, _)| items));
            let probe =
                dragged.and_then(|items| self.drag_pointer(ui.ctx()).map(|pointer| DropProbe::new(pointer, items)));
            // A carried clip probes the same way, for a slot or a lane only.
            let probe = match (&self.carry, ui.ctx().pointer_latest_pos()) {
                (Some(_), Some(pointer)) => Some(DropProbe::for_clip(pointer)),
                _ => probe,
            };
            self.view.can_paste = self.session().is_some_and(|s| s.copied_clip_is_audio().is_some());
            self.view.drop = probe.clone();
            self.arrangement.drop = probe;
            let meters =
                crate::kit::meter::Meters::read(self.session().and_then(|s| s.player()).map(|p| p.meter_monitor()));
            // Meters fall for a moment after the sound stops: drawn again at once while they
            // move, and looked at again soon after they last moved, as the engine renders in
            // blocks that can be longer than a frame. An engine that stopped rendering leaves
            // the window at rest.
            let now = ui.input(|i| i.time);
            if meters != self.view.meters {
                self.meters_moved_at = now;
                ui.ctx().request_repaint();
            } else if meters.moving() && now - self.meters_moved_at < METERS_SETTLE_SECONDS {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
            self.view.meters = meters.clone();
            self.arrangement.meters = meters;
            // The large sample map takes the main area, except while something is dragged, so
            // the drop can land on a track.
            if self.sample_map.view.large && dragged.is_none() {
                let (panel, browser, projects, settings) =
                    (&mut self.sample_map, &mut self.browser, &mut self.projects, &mut self.settings);
                if panel.show(ui, inner, browser, projects, settings, self.revision) {
                    self.settings_changed = true;
                }
            } else {
                ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| match (&grid, self.shown) {
                    // The piano roll pulled to the top over the whole window: no view.
                    _ if inner.height() < 1.0 => {}
                    (Some(grid), View::Session) => {
                        self.view.keep_selection_on_launch = !self.settings.select_on_launch;
                        self.view.knobs = self.settings.mixer_controls == MixerControls::Knob;
                        // Both views fold the same groups: the Arrangement view keeps them.
                        self.view.folded = self.arrangement.folds.groups();
                        self.view.mixer_height = self.settings.view.mixer_height as f32;
                        self.view.strip_fader = self.settings.view.strip_fader as f32;
                        self.view.double_click_does_nothing = !self.settings.view.double_click_clip;
                        self.view.device_picks = self.device_picks();
                        session_view::show(ui, grid, &progress, &mut self.view, &mut actions);
                        match self.view.device_event.take() {
                            Some(session_view::DeviceEvent::Pick { track, index, add, run }) => {
                                self.pick_device(track, index, add, run);
                            }
                            Some(session_view::DeviceEvent::Open { track, index, plugin }) => {
                                self.pick_device(track, index, false, false);
                                if plugin {
                                    actions.push(Action::new(
                                        "device.openEditor",
                                        json!({ "track": track.0, "device": index }),
                                    ));
                                }
                            }
                            None => {}
                        }
                        self.arrangement.folds.set_groups(&self.view.folded);
                        self.session_view_done();
                    }
                    (Some(_), View::Arrangement) => {
                        if let Some(session) = self.projects.active().and_then(|id| self.projects.get(id)) {
                            self.arrangement.stack_audio = self.settings.sample_stacking;
                            self.arrangement.stack_midi = self.settings.midi_stacking;
                            self.arrangement.double_click_does_nothing = !self.settings.view.double_click_clip;
                            self.arrangement.knobs = self.settings.mixer_controls == MixerControls::Knob;
                            let monitor = session.player().map(|p| p.monitor());
                            // A folded group's row shows the track being worked on, so the folds
                            // are brought in line before the lanes are built from them.
                            self.arrangement.reconcile(session.project());
                            // Clips and their notes are rebuilt only after the project changed; the
                            // playhead is read every frame.
                            let timeline = match self.timeline.take() {
                                Some((revision, folds, mut timeline))
                                    if revision == self.revision && folds == self.arrangement.folds =>
                                {
                                    timeline.follow(monitor);
                                    timeline
                                }
                                before => {
                                    // Stacked clips keep the sub-rows the last build gave them.
                                    let rows = before.map(|(_, _, t)| t.stack_rows).unwrap_or_default();
                                    let mut timeline = Timeline::build_after(session.project(), monitor, &rows);
                                    for lane in &mut timeline.lanes {
                                        lane.armed = session.is_armed(lane.track);
                                    }
                                    timeline.waveforms = self.waveforms.clone();
                                    timeline
                                }
                            };
                            // The grid follows the value in the control bar, which each
                            // project keeps (`launcher.setQuantization`).
                            self.arrangement.grid = session.quantization();
                            let mut timeline = timeline;
                            timeline.midi_ports = self.projects.midi_ports().to_vec();
                            timeline.input_channels = session
                                .player()
                                .and_then(suhina_session::Player::output_info)
                                .map_or(0, |i| i.input_channels);
                            arrangement_view::show(ui, &timeline, &mut self.arrangement, &mut actions);
                            // Clips picked while stopped, however (a click, a rectangle, Ctrl+A, a
                            // paste): play starts from the first of them, as Ableton Live's does.
                            let picked = self.arrangement.selected_clips();
                            if picked != self.clips_before.as_slice() {
                                let first = picked
                                    .iter()
                                    .filter_map(|(track, index)| {
                                        timeline
                                            .lanes
                                            .iter()
                                            .find(|l| l.track == *track)?
                                            .clips
                                            .iter()
                                            .find(|c| c.index == *index)
                                            .map(|c| c.start)
                                    })
                                    .fold(f64::INFINITY, f64::min);
                                if !timeline.running && first.is_finite() && (first - timeline.start_beat).abs() > 1e-9
                                {
                                    actions.push(Action::new("transport.setPosition", json!({ "beats": first })));
                                }
                                self.clips_before = picked.to_vec();
                            }
                            self.devices_asked();
                            self.timeline = Some((self.revision, self.arrangement.folds.clone(), timeline));
                            // The headers can be moved from the view itself; that is the setting too.
                            let side = if self.arrangement.headers_left { Side::Left } else { Side::Right };
                            if side != self.settings.track_headers {
                                self.settings.track_headers = side;
                                self.settings_changed = true;
                            }
                        }
                    }
                    (None, _) => self.start_panel(ui),
                });
            }
            drop_overlay(ui, view_area);
        });
        self.browser_drag(ui);
        if let Some((items, dropped)) = system {
            self.system_drag(ui, items, dropped);
        }
        self.carry_clip(ui);
        self.poll_drop(ui.ctx());

        self.plugin_dialog(ui, window);
        self.project_dialogs(ui, window);
        if self.settings_open {
            // Embedded plug-in editors are windows of their own and would cover the dialog.
            self.place_editors(&[]);
            self.settings_dialog(ui, window);
        }
        self.update_title(ui.ctx());
        for action in &actions {
            // Closing a project with unsaved changes asks first (Save, Don't save, Cancel).
            let closing = (action.id == "projects.close")
                .then(|| action.params["project"].as_u64().and_then(|n| u32::try_from(n).ok()).map(ProjectId))
                .flatten();
            if let Some(project) = closing {
                self.check_plugin_edits(project);
            }
            if let Some(project) = closing
                && self.unsaved(project)
            {
                self.unsaved_prompt = Some(Leaving { project, then: AfterLeaving::Close });
                continue;
            }
            // What the chain was, before a removal about to run: its undo or redo later says
            // whether the chain is back and where the selection pointed.
            let before_removal = (action.id == "device.remove")
                .then(|| action.params["track"].as_u64().and_then(|t| u32::try_from(t).ok()).map(TrackId))
                .flatten()
                .and_then(|track| self.device_count(track).map(|count| (track, count)));
            self.run(action);
            // A removed device takes its selection with it, whichever way it was removed — and
            // the devices after it move up an index, so a selection below the removal follows.
            if action.id == "device.remove"
                && let Some((track, count)) = before_removal
                && let Some(index) = action.params["device"].as_u64().and_then(|i| usize::try_from(i).ok())
                && !matches!(self.status, Status::Error(_))
                && self.selected_device.is_some_and(|(t, _)| t == track)
            {
                let followed = self.selected_device;
                match self.selected_device {
                    Some((_, selected)) if selected == index => self.selected_device = None,
                    Some((t, selected)) if selected > index => self.selected_device = Some((t, selected - 1)),
                    _ => {}
                }
                // Its undo or redo then puts the chain back or takes it again: the selection
                // follows once, from where it pointed before the removal.
                if let Some((_, selected)) = followed {
                    self.pending_device_remove = Some((track, index, count, selected));
                }
            }
            // An undo or redo of that removal: undone, the chain is back and the selection is
            // where it was; redone, it moves as the removal did. The record stands either
            // way, until the next pick or removal says where the user is now.
            if matches!(action.id, "edit.undo" | "edit.redo")
                && let Some((track, index, count, selected)) = self.pending_device_remove
                && let Some(now) = self.device_count(track)
            {
                if now == count {
                    self.selected_device = Some((track, selected));
                } else if now.checked_add(1) == Some(count) {
                    self.selected_device =
                        (selected != index).then_some((track, if selected > index { selected - 1 } else { selected }));
                }
            }
            // The window gets Ctrl+V only as a paste of the system clipboard's text, which it
            // does not send while the clipboard holds no text: copying puts a line there, so
            // Ctrl+V reaches the keymap after a copy in Suhina (`Keymap::pressed`).
            if matches!(action.id, "note.copy" | "note.cut" | "clip.copy" | "clip.cut")
                && !matches!(self.status, Status::Error(_))
            {
                let what = if action.id.starts_with("note.") { "notes" } else { "a clip" };
                ui.ctx().copy_text(format!("Suhina: {what} copied"));
            }
        }
        self.guard_quit(ui.ctx());
        // The views drew before the actions and drops of this frame ran: when the project changed,
        // ask for a frame at once, so what the user just did (a moved clip, a file that landed)
        // is on screen without waiting for another click or pointer move.
        if self.revision != revision {
            ui.ctx().request_repaint();
        }
        if grid.is_some_and(|g| {
            g.running || g.columns.iter().any(|c| c.stop_queued || c.slots.iter().any(|s| s.state == SlotState::Queued))
        }) {
            ui.ctx().request_repaint();
        }
    }

    /// File › Open recent: the recent projects the project dialog lists, newest first, each opened
    /// as File › Open… opens a file. The list is read again each time the File menu opens.
    fn open_recent_menu(&mut self, ui: &mut Ui, was_open: bool) {
        self.file_menu_open = true;
        if !was_open {
            self.lists_wanted = true;
        }
        let recent: Vec<PathBuf> = self.project_lists.recent.iter().take(RECENT_IN_MENU).cloned().collect();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button("Open recent", |ui| {
                for path in recent {
                    let name =
                        path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        if path.is_file() {
                            self.request_open(path);
                        } else {
                            self.status = Status::Error(format!("{} is no longer there", path.display()));
                        }
                    }
                }
            });
        });
    }

    /// What the Session view asked for this frame that the window keeps: the mixer's height,
    /// saved in Settings, and the detail area opened from a strip's device list.
    fn session_view_done(&mut self) {
        let height = self.view.mixer_height.round() as u32;
        if height != self.settings.view.mixer_height {
            self.settings.view.mixer_height = height;
            self.settings_changed = true;
        }
        let fader = self.view.strip_fader.round() as u32;
        if fader != self.settings.view.strip_fader {
            self.settings.view.strip_fader = fader;
            self.settings_changed = true;
        }
        if std::mem::take(&mut self.view.open_detail) {
            self.show_editor();
        }
        self.devices_asked();
    }

    /// A double-click on a track (an Arrangement header, a Session column's name or channel
    /// strip) selects it and shows its devices, opening the detail area; a clip selected on
    /// another track is no longer selected (`docs/spec/ui-clip-editor.md` › Where it opens).
    fn devices_asked(&mut self) {
        if let Some(track) = self.arrangement.open_devices.take() {
            if self.arrangement.selected_clip().is_some_and(|(t, _)| t != track) {
                self.arrangement.clear_selected_clip();
            }
            if self.arrangement.selected_tracks.first() != Some(&track) {
                self.arrangement.selected_tracks = vec![track];
            }
            self.show_devices();
        }
        if let Some(track) = self.view.open_devices.take() {
            self.view.selected = Some(track);
            if self.view.selected_slot.is_some_and(|(t, _)| t != track) {
                self.view.selected_slot = None;
            }
            self.show_devices();
        }
    }

    /// Shows the shown track's devices in the detail area, opening it when it is hidden.
    fn show_devices(&mut self) {
        self.detail_showing = detail_view::Showing::Devices;
        self.show_editor();
    }

    /// The Clip / Devices switch's key and menu entry, for a clip with an editor: the other part,
    /// or the devices when the detail area is hidden, which opens it.
    fn toggle_devices(&mut self) {
        if self.piano_clip(self.shown_clip()).is_none() {
            return;
        }
        self.detail_showing = match self.detail_showing {
            detail_view::Showing::Clip => detail_view::Showing::Devices,
            _ if !self.detail_open => detail_view::Showing::Devices,
            detail_view::Showing::Devices => detail_view::Showing::Clip,
        };
        self.show_editor();
    }

    /// Lays out the detail area beside or under the view in `area`, draws it with its splitter,
    /// and returns what is left for the view.
    fn detail_area(&mut self, ui: &mut Ui, area: Rect, actions: &mut Vec<Action>) -> Rect {
        let stacked = self.settings.device_layout == DeviceLayout::Stack;
        // What it shows: the selected clip, or the selected track's devices.
        let (track, clip) = match self.shown {
            View::Session => (self.view.selected, self.view.selected_slot.map(|(t, s)| ClipRef::Slot(t, s))),
            View::Arrangement => (
                self.arrangement.selected_tracks.first().copied(),
                self.arrangement.selected_clip().map(|(t, i)| ClipRef::Arrangement(t, i)),
            ),
        };
        // Where each plug-in on the shown track stands, read first: reading it needs the session
        // itself, building the detail only the project; and an editor embedded in the panel
        // decides how tall the panel is.
        let mut reports = std::collections::HashMap::new();
        if let Some(shown_track) = clip.map(ClipRef::track).or(track)
            && let Ok(session) = self.projects.session_mut(None)
        {
            let devices = session
                .project()
                .tracks
                .iter()
                .find(|t| t.id == shown_track)
                .and_then(|t| t.channel)
                .and_then(|c| session.project().channels.iter().find(|ch| ch.id == c))
                .map_or(0, |c| c.devices.len());
            for index in 0..devices {
                if let Some(report) = session.plugin_report(shown_track, index) {
                    reports.insert((shown_track, index), report);
                }
            }
        }
        if !stacked {
            let scale = ui.ctx().pixels_per_point();
            let needed = reports
                .values()
                .filter_map(|r| r.editor_embedded)
                .map(|(_, height)| detail_view::row_height_with_editor(height, scale))
                .fold(0.0, f32::max);
            self.detail_height = self.detail_height.max(needed);
        }
        // A MIDI clip shows its notes in the piano roll beside its properties; under the
        // Arrangement view it can be pulled to the top.
        let mut piano = self.piano_clip(clip);
        // The notes of other clips the piano roll draws behind this one's (› Ghost notes).
        if let (Some(piano), Some(clip), Some(session)) = (piano.as_mut(), clip, self.session()) {
            piano.ghosts =
                crate::detail::piano_roll_ghosts::ghosts(session.project(), clip, self.piano_roll.ghost_source);
        }
        let least = self.least_detail_height();
        let pullable = !stacked && piano.is_some() && self.shown == View::Arrangement;
        let pulled = pullable && self.detail_max;
        // With the switch on Devices the track's devices take the editor's place, and nothing is
        // pulled to the top (› Where it opens in `docs/spec/ui-clip-editor.md`).
        let switch = piano.is_some().then_some(self.detail_showing);
        let piano = piano.filter(|_| self.detail_showing == detail_view::Showing::Clip);
        let (pullable, pulled) = (pullable && piano.is_some(), pulled && piano.is_some());
        let focus = match clip {
            Some(detail_view::ClipRef::Arrangement(track, _))
                if pulled && self.settings.piano_roll_top == PianoRollTop::Track =>
            {
                Some(track)
            }
            _ => None,
        };
        self.arrangement.focus = focus;
        let (view, splitter, detail) = if stacked {
            self.detail_width = self.detail_width.clamp(180.0, (area.width() - 240.0).max(180.0));
            let x = area.right() - self.detail_width;
            (
                Rect::from_min_max(area.min, Pos2::new(x - SPLITTER, area.bottom())),
                Rect::from_min_max(Pos2::new(x - SPLITTER, area.top()), Pos2::new(x, area.bottom())),
                Rect::from_min_max(Pos2::new(x, area.top()), area.max),
            )
        } else {
            // Only the chain shown: no taller than its panels.
            if piano.is_none()
                && clip.is_none()
                && let Some(cap) = self.detail_cap
            {
                self.detail_height = self.detail_height.min(cap.max(least));
            }
            self.detail_height = self.detail_height.clamp(least, (area.height() - MIN_VIEW).max(least));
            let y = if pulled {
                // Above it only the edited track, or nothing.
                let above = focus.map_or(0.0, |t| self.arrangement.focus_height(t));
                (area.top() + above.min(area.height() - least) + SPLITTER).min(area.bottom() - least)
            } else {
                area.bottom() - self.detail_height
            };
            (
                Rect::from_min_max(area.min, Pos2::new(area.right(), (y - SPLITTER).max(area.top()))),
                Rect::from_min_max(Pos2::new(area.left(), y - SPLITTER), Pos2::new(area.right(), y)),
                Rect::from_min_max(Pos2::new(area.left(), y), area.max),
            )
        };

        // The splitter: dragged to make the detail area bigger or smaller; under the
        // Arrangement view, past the view's least height the piano roll goes to the top, and a
        // double-click takes it there and back.
        let cursor = if stacked { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
        let response =
            ui.interact(splitter, Id::new("detail-splitter"), Sense::click_and_drag()).on_hover_cursor(cursor);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Detail area splitter"));
        if response.double_clicked() && pullable {
            self.detail_max = !self.detail_max;
        } else if response.double_clicked() {
            // Elsewhere a double-click puts the detail area back to its default size.
            self.detail_height = DETAIL_HEIGHT;
            self.detail_width = detail_view::STACK_WIDTH;
        }
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            if stacked {
                self.detail_width -= delta.x;
            } else {
                let height = self.detail_drag.unwrap_or(detail.height()) - delta.y;
                self.detail_drag = Some(height);
                self.detail_max = pullable && height > area.height() - MIN_VIEW;
                if height < least / 2.0 {
                    // Dragged down past half its least height, the detail area closes; the same
                    // drag going back up opens it again (`collapsed_splitter`).
                    self.detail_max = false;
                    self.hide_editor();
                } else if !self.detail_max {
                    self.detail_height = height.max(least);
                }
            }
        } else {
            self.detail_drag = None;
        }
        let painter = ui.painter_at(splitter);
        painter.rect_filled(splitter, 0.0, theme::control_background());
        let grip = if stacked { Vec2::new(2.0, 32.0) } else { Vec2::new(32.0, 2.0) };
        painter.rect_filled(Rect::from_center_size(splitter.center(), grip), 0.0, theme::text_hint());

        if let Some(shown_track) = clip.map(ClipRef::track).or(track) {
            self.detail_drop = Some((detail, shown_track));
        }
        let nam_lists = self.nam_lists();
        let mut shown = self.session().map_or(detail_view::Detail::Nothing, |s| {
            detail_view::build_with(s.project(), track, clip, &mut |t, i| reports.remove(&(t, i)), nam_lists)
        });
        // What the piano roll has selected, described for the clip column (› Clip column).
        if let (detail_view::Detail::Clip(info, _), Some(piano), Some(session)) = (&mut shown, &piano, self.session()) {
            info.selection = detail_view::SelectionInfo::of(
                &piano.notes,
                &self.piano_roll.selected,
                detail_view::bar_beats(session.project()),
            );
        }
        if let detail_view::Detail::Clip(info, _) = &mut shown {
            info.switch = switch;
        }
        // A selected slot clip's follow action comes first, the rest of the detail beside or
        // under it.
        let follow = match (self.shown, self.view.selected_slot, self.session()) {
            (View::Session, Some((t, scene)), Some(s)) => follow_view::FollowInfo::build(s.project(), t, scene),
            _ => None,
        };
        let detail = match &follow {
            Some(info) => {
                let used = ui.scope_builder(egui::UiBuilder::new().max_rect(detail), |ui| {
                    follow_view::show(ui, detail, info, stacked, actions)
                });
                if stacked {
                    let top = (used.inner.bottom() + 1.0).min(detail.bottom());
                    Rect::from_min_max(Pos2::new(detail.left(), top), detail.max)
                } else {
                    let left = (used.inner.right() + 1.0).min(detail.right());
                    Rect::from_min_max(Pos2::new(left, detail.top()), detail.max)
                }
            }
            None => detail,
        };
        if piano.is_none() {
            // A note the audition sounds goes off with the piano roll.
            actions.extend(self.piano_roll.stop_audition());
            self.piano_roll.hide();
        }
        let mut places = Vec::new();
        let options =
            piano_roll::Options { toolbar_size: self.settings.toolbar_size, workflow: self.keymap.workflow() };
        // The toolbar's tooltips show the keys the keymap has now, so a rebind (or a preset) is
        // visible on the button.
        let keymap = self.keymap.clone();
        let copied = self.session().and_then(Session::copied_notes);
        // While the transport runs, the piano roll shows where playback is: for a slot's clip,
        // the beats since its launch (its placement starts at the launch and never ends); for
        // an arrangement clip, the transport's beat through the clip's placement.
        let playhead = match self.shown {
            View::Session => self.view.selected_slot.and_then(|(track, scene)| self.slot_playhead(track, scene)),
            View::Arrangement => {
                self.session().and_then(|s| s.player()).map(|p| p.monitor()).filter(|m| m.running()).map(|m| m.beat())
            }
        };
        let picks = self.device_picks();
        let piano_roll = &mut self.piano_roll;
        let picked = ui.scope_builder(egui::UiBuilder::new().max_rect(detail), |ui| {
            if let (Some(piano), detail_view::Detail::Clip(info, _)) = (&piano, &shown) {
                let (mut rest, picked) = detail_view::show_clip_column(ui, info, stacked);
                let picked = detail_view::Picked { showing: picked.showing, ..detail_view::Picked::default() };
                // The MIDI tools column, between the clip's properties and the note grid. With the
                // detail area stacked there is no room beside the notes, so it is not drawn there
                // (the quantise settings and Ctrl+U still work: › Other note tools).
                if piano_roll.tools_open && !stacked {
                    let selected = piano_roll.selected.clone();
                    let tools = crate::detail::midi_tools::Tools {
                        address: &piano.address,
                        notes: &piano.notes,
                        selected: &selected,
                    };
                    rest = crate::detail::midi_tools::show(ui, rest, &tools, piano_roll, actions);
                }
                let cx = piano_roll::Context { options, keymap: &keymap, copied, playhead };
                piano_roll::show(ui, rest, piano, piano_roll, cx, actions);
                picked
            } else if stacked {
                detail_view::show_stack(ui, &shown, &picks, actions, &mut places)
            } else {
                detail_view::show_row(ui, &shown, &picks, actions, &mut places)
            }
        });
        if let Some(showing) = picked.inner.showing {
            self.detail_showing = showing;
        }
        self.detail_cap = picked.inner.content_height.map(|h| h as f32);
        if let Some((track, index)) = picked.inner.clicked {
            let (add, run) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            self.pick_device(track, index, add, run);
            // A fresh pick says where the user is now: a removal's undo or redo no longer
            // moves the selection for them.
            self.pending_device_remove = None;
        }
        self.place_editors(&places);
        // The toolbar's × hides the editor.
        if std::mem::take(&mut self.piano_roll.hide_requested) {
            self.hide_editor();
        }
        // A workflow mode or setting picked in the piano roll's toolbar: kept in the keymap.
        if let Some(picked) = self.piano_roll.workflow_picked.take() {
            picked.apply(&mut self.keymap);
            self.keymap_changed = true;
        }
        // A grid value picked in the piano roll's grid menu or with its keys: its own, or the
        // control bar's when the piano roll follows it.
        if let Some(picked) = self.piano_roll.grid_picked.take() {
            if self.settings.piano_grid_follows {
                if let Some(value) = picked.control_bar_value() {
                    actions.push(Action::new("launcher.setQuantization", json!({ "value": value })));
                }
            } else {
                self.piano_grid = picked;
            }
        }
        view
    }

    /// The splitter along the bottom while the detail area is hidden: dragged up past half the
    /// least height, the detail area opens at the height the drag reaches. Returns the room left
    /// above it for the view.
    fn collapsed_splitter(&mut self, ui: &mut Ui, area: Rect) -> Rect {
        let splitter = Rect::from_min_max(Pos2::new(area.left(), (area.bottom() - SPLITTER).max(area.top())), area.max);
        let response = ui
            .interact(splitter, Id::new("detail-splitter"), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Detail area splitter"));
        if response.dragged_by(egui::PointerButton::Primary) {
            let height = self.detail_drag.unwrap_or(0.0) - response.drag_delta().y;
            self.detail_drag = Some(height);
            let least = self.least_detail_height();
            if height >= least / 2.0 {
                self.show_editor();
                self.detail_height = height.max(least);
            }
        } else {
            self.detail_drag = None;
        }
        let painter = ui.painter_at(splitter);
        painter.rect_filled(splitter, 0.0, theme::control_background());
        painter.rect_filled(Rect::from_center_size(splitter.center(), Vec2::new(32.0, 2.0)), 0.0, theme::text_hint());
        Rect::from_min_max(area.min, Pos2::new(area.right(), splitter.top()))
    }

    /// The least height of the detail area in a row: the piano roll's while it shows one, else
    /// 80 px.
    fn least_detail_height(&self) -> f32 {
        let piano = self.settings.device_layout != DeviceLayout::Stack
            && self.detail_showing == detail_view::Showing::Clip
            && self.piano_clip(self.shown_clip()).is_some();
        if piano { piano_roll::LEAST_HEIGHT } else { 80.0 }
    }

    /// The clip the detail area shows: the Arrangement view's selected clip, or the Session view's
    /// selected slot's.
    fn shown_clip(&self) -> Option<ClipRef> {
        match self.shown {
            View::Session => self.view.selected_slot.map(|(t, s)| ClipRef::Slot(t, s)),
            View::Arrangement => self.arrangement.selected_clip().map(|(t, i)| ClipRef::Arrangement(t, i)),
        }
    }

    /// Hides the detail area as an editor: selecting a clip opens it again.
    fn hide_editor(&mut self) {
        self.detail_open = false;
        self.editor_closed = true;
    }

    fn show_editor(&mut self) {
        self.detail_open = true;
        self.editor_closed = false;
    }

    /// The piano roll for the selected arrangement clip, when it is a MIDI clip; its grid is its
    /// own grid value, or the control bar's when Settings says so.
    /// The piano roll for the shown clip: an arrangement clip's, or the Session view's selected
    /// slot's clip.
    fn piano_clip(&self, clip: Option<detail_view::ClipRef>) -> Option<piano_roll::PianoClip> {
        let follows = self.settings.piano_grid_follows;
        let control_bar = self.session().map_or(DEFAULT_QUANTIZATION, |s| quantization_label(s.quantization()));
        let quantization = suhina_session::parse_quantization(control_bar).ok()?;
        let mut piano = match clip? {
            detail_view::ClipRef::Arrangement(track, index) => {
                piano_roll::arrangement_clip(self.session()?.project(), track, index, quantization)?
            }
            detail_view::ClipRef::Slot(track, scene) => {
                piano_roll::launcher_clip(self.session()?.project(), track, scene, quantization)?
            }
        };
        if follows {
            piano.grid_menu = crate::detail::piano_roll_grid::PianoGrid::from_value(control_bar);
        } else {
            piano.grid_menu = self.piano_grid;
            piano.grid = self.piano_grid.fixed_step(piano.bar_beats);
        }
        piano.grid_follows = follows;
        Some(piano)
    }

    /// The params that name the clip the piano roll shows in a `note.*` command, if it shows one.
    pub fn piano_roll_address(&self) -> Option<serde_json::Value> {
        if self.detail_showing == detail_view::Showing::Devices {
            return None;
        }
        let clip = match self.shown {
            View::Session => self.view.selected_slot.map(|(t, s)| detail_view::ClipRef::Slot(t, s)),
            View::Arrangement => self.arrangement.selected_clip().map(|(t, i)| detail_view::ClipRef::Arrangement(t, i)),
        };
        self.piano_clip(clip).map(|c| c.address)
    }

    /// Whether the detail area shows a track's device chain — not a clip's properties and
    /// notes: the selected device's Delete key applies only while its panel is on show.
    fn detail_shows_devices(&self) -> bool {
        let clip = match self.shown {
            View::Session => self.view.selected_slot.map(|(t, s)| detail_view::ClipRef::Slot(t, s)),
            View::Arrangement => self.arrangement.selected_clip().map(|(t, i)| detail_view::ClipRef::Arrangement(t, i)),
        };
        clip.is_none() || self.detail_showing == detail_view::Showing::Devices
    }

    /// What a key does in the piano roll, when it shows a clip and has the keyboard.
    fn piano_key(&mut self, action: &str) -> Option<Action> {
        if self.detail_showing == detail_view::Showing::Devices {
            return None;
        }
        let clip = match self.shown {
            View::Session => self
                .view
                .selected_slot
                .map(|(t, s)| detail_view::ClipRef::Slot(t, s))
                .and_then(|clip| self.piano_clip(Some(clip)))?,
            View::Arrangement => self
                .arrangement
                .selected_clip()
                .map(|(t, i)| detail_view::ClipRef::Arrangement(t, i))
                .and_then(|clip| self.piano_clip(Some(clip)))?,
        };
        // Where playback is, in the clip's placement's input (the arrangement's beat, or the
        // beats since a slot clip's launch); 0.0 while the transport is stopped.
        let playhead = match self.shown {
            View::Arrangement => self.timeline.as_ref().map_or(0.0, |(_, _, t)| t.beat),
            View::Session => {
                let (track, scene) = self.view.selected_slot?;
                self.slot_playhead(track, scene).unwrap_or(0.0)
            }
        };
        let copied = self.session().and_then(Session::copied_notes);
        self.piano_roll.key_action(action, &clip, playhead, copied)
    }

    /// The beats the selected slot's clip has played, while the engine plays that very slot: the
    /// piano roll's playhead for a launcher clip, whose placement starts at the launch.
    fn slot_playhead(&self, track: TrackId, scene: usize) -> Option<f64> {
        let monitor = self.session()?.player()?.monitor();
        let column = self.session()?.project().launcher.columns.iter().position(|c| c.track == track)?;
        if !monitor.running() || monitor.column(column)?.playing != Some(scene) {
            return None;
        }
        monitor.elapsed_beats(column)
    }

    /// The piano roll's state: its zoom, scroll and selected notes.
    pub fn piano_roll(&self) -> &PianoRollState {
        &self.piano_roll
    }

    /// The piano roll's state, to change: the ignored render test opens its quantise block with
    /// this, where a click would toggle it back.
    #[doc(hidden)]
    pub fn piano_roll_mut(&mut self) -> &mut PianoRollState {
        &mut self.piano_roll
    }

    /// Tells the active project where the panel shows its embedded plug-in editors this frame
    /// (none: they are hidden).
    fn place_editors(&mut self, places: &[detail_view::EditorPlace]) {
        let Ok(session) = self.projects.session_mut(None) else { return };
        let shown: Vec<_> = places.iter().map(|p| (p.track, p.device, p.placement)).collect();
        session.place_editors(&shown);
    }

    /// The tab row: one tab per open project, the active one lit; a tab's × closes it.
    fn tab_row(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let active = self.projects.active();
        let mut x = bar.left();
        for id in self.projects.ids() {
            let name = self.projects.name(id).unwrap_or("Untitled").to_string();
            // A dot after the name while the project has unsaved changes.
            let unsaved = self.projects.get(id).is_some_and(Session::has_unsaved_changes);
            let shown = if unsaved { format!("{name} •") } else { name.clone() };
            let galley = widgets::fitted(&painter, &shown, theme::ui_font(theme::TEXT), theme::text_primary(), 180.0);
            let width = galley.size().x + PAD * 2.0 + 16.0;
            let tab = Rect::from_min_size(Pos2::new(x, bar.top()), Vec2::new(width, bar.height()));
            x = tab.right() + 1.0;
            let close = Rect::from_center_size(Pos2::new(tab.right() - PAD - 4.0, tab.center().y), Vec2::splat(12.0));
            let response = widgets::button_area(ui, tab, Id::new(("tab", id.0)), format!("Tab {name}"));
            let close_response = widgets::button_area(ui, close, Id::new(("close-tab", id.0)), format!("Close {name}"));
            if close_response.clicked() || response.middle_clicked() {
                actions.push(Action::new("projects.close", json!({ "project": id.0 })));
            } else if response.clicked() && Some(id) != active {
                actions.push(Action::new("projects.switch", json!({ "project": id.0 })));
            }
            let lit = motion::toward(ui.ctx(), response.id.with("active"), Some(id) == active, motion::VALUE);
            let idle = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
            painter.rect_filled(tab, 0.0, motion::mix(idle, theme::panel_background(), lit));
            let text = motion::mix(theme::text_secondary(), theme::text_primary(), lit);
            painter.galley(Pos2::new(tab.left() + PAD, tab.center().y - galley.size().y / 2.0), galley, text);
            let cross = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &close_response));
            let c = close.center();
            for (a, b) in [(Vec2::new(-3.0, -3.0), Vec2::new(3.0, 3.0)), (Vec2::new(-3.0, 3.0), Vec2::new(3.0, -3.0))] {
                painter.line_segment([c + a, c + b], theme::hairline(cross));
            }
            painter.vline(tab.right() + 0.5, bar.y_range(), theme::hairline(theme::border()));
        }
    }

    /// The question asked when a project is dropped while another is open.
    fn open_prompt_bar(&mut self, ui: &mut Ui) {
        let Some(path) = self.open_prompt.clone() else { return };
        let bar = ui.max_rect();
        ui.painter().rect_filled(bar, 0.0, theme::panel_background());
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
        let current = self.projects.active().and_then(|id| self.projects.name(id)).unwrap_or("the project").to_string();
        let mut choice = None;
        let mut cancel = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(PAD, 6.0))), |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(format!("Open {name}:")).color(theme::text_primary()));
                if ui.button("In a new tab").clicked() {
                    choice = Some(OpenAs::NewTab);
                }
                if ui.button(format!("In place of {current}")).clicked() {
                    choice = Some(OpenAs::Replace);
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
                ui.checkbox(&mut self.remember_choice, "Remember");
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        }
        if let Some(how) = choice {
            if self.remember_choice {
                self.settings.on_open = match how {
                    OpenAs::NewTab => OnOpen::NewTab,
                    OpenAs::Replace => OnOpen::Replace,
                };
                self.settings_changed = true;
            }
            self.open_as(path, how);
            self.open_prompt = None;
            self.remember_choice = false;
        } else if cancel {
            self.open_prompt = None;
            self.remember_choice = false;
        }
    }

    /// Reads the plug-in library through its commands, for the Settings page.
    fn read_plugins(&mut self) {
        self.plugins_read = true;
        let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let list = |v: &serde_json::Value| v.as_array().map(|a| a.iter().map(text).collect()).unwrap_or_default();
        match self.projects.execute("plugins.folders", &json!({})) {
            Ok(folders) => {
                self.plugins.standard = list(&folders["standard"]);
                self.plugins.added = list(&folders["added"]);
            }
            Err(e) => self.status = Status::Error(e.0),
        }
        match self.projects.execute("plugins.list", &json!({})) {
            Ok(found) => {
                let empty = Vec::new();
                self.plugins.found = found["plugins"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|p| [text(&p["name"]), text(&p["vendor"]), text(&p["format"]), text(&p["version"])])
                    .collect();
                self.plugins.failed = found["failed"]
                    .as_array()
                    .unwrap_or(&empty)
                    .iter()
                    .map(|f| (text(&f["file"]), text(&f["reason"])))
                    .collect();
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// Does what the Settings page asked of the plug-in library. Folders change through their
    /// commands; a scan runs `Library::scan` (what `plugins.scan` runs) on a copy of the library
    /// on another thread, so the window keeps drawing, and the result replaces the library.
    fn plugin_request(&mut self, request: PluginRequest) {
        match request {
            PluginRequest::AddFolder(folder) => {
                match self.projects.execute("plugins.addFolder", &json!({ "folder": folder })) {
                    Ok(_) => self.plugins.new_folder.clear(),
                    Err(e) => self.status = Status::Error(e.0),
                }
                self.plugins_read = false;
            }
            PluginRequest::RemoveFolder(folder) => {
                if let Err(e) = self.projects.execute("plugins.removeFolder", &json!({ "folder": folder })) {
                    self.status = Status::Error(e.0);
                }
                self.plugins_read = false;
            }
            PluginRequest::Scan { all } => {
                let mut library = self.projects.library_mut().clone();
                let (done, receive) = std::sync::mpsc::channel();
                let started = std::thread::Builder::new().name("suhina-plugin-scan".into()).spawn(move || {
                    let report = library.scan(all).map(|r| (r.read.len(), r.failed.len()));
                    let _unheard = done.send(ScanDone { library, report });
                });
                match started {
                    Ok(_) => {
                        self.scan = Some(receive);
                        self.plugins.scanning = true;
                        self.status = Status::Hint("Scanning for plug-ins".into());
                    }
                    Err(e) => self.status = Status::Error(format!("could not start the scan: {e}")),
                }
            }
        }
    }

    /// Picks up a finished background scan.
    fn poll_scan(&mut self, ctx: &egui::Context) {
        let Some(receive) = &self.scan else { return };
        match receive.try_recv() {
            Ok(ScanDone { library, report }) => {
                *self.projects.library_mut() = library;
                self.status = match report {
                    Ok((read, failed)) => Status::Done(format!("Scan done: {read} files read, {failed} failed")),
                    Err(e) => Status::Error(e),
                };
                self.scan = None;
                self.plugins.scanning = false;
                self.plugins_read = false;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.status = Status::Error("the plug-in scan stopped without a result".into());
                self.scan = None;
                self.plugins.scanning = false;
            }
        }
    }

    /// Lists the audio hosts and devices for Settings › Audio (`audio.devices`).
    fn list_audio_devices(&mut self) {
        match self.projects.execute("audio.devices", &json!({})) {
            Ok(_) => {
                self.about.audio_hosts = self.projects.audio_system().map(|s| s.hosts()).unwrap_or_default();
            }
            Err(e) => self.status = Status::Error(e.0),
        }
        self.list_midi_inputs();
    }

    /// Lists the MIDI input ports for Settings › Audio (`midi.inputs`).
    fn list_midi_inputs(&mut self) {
        self.midi_listed = Some(std::time::Instant::now());
        self.about.midi_inputs = self.projects.execute("midi.inputs", &json!({})).map_err(|e| e.0).map(|listed| {
            listed["inputs"]
                .as_array()
                .map(|ports| {
                    ports
                        .iter()
                        .map(|p| settings_view::MidiPort {
                            name: p["name"].as_str().unwrap_or_default().to_string(),
                            enabled: p["enabled"].as_bool().unwrap_or(false),
                            open: p["open"].as_bool().unwrap_or(false),
                            error: p["error"].as_str().map(String::from),
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
    }

    /// Turns a MIDI input port on or off (`midi.setInput`) and keeps the choice in the settings.
    fn set_midi_input(&mut self, port: &str, enabled: bool) {
        match self.projects.execute("midi.setInput", &json!({ "port": port, "enabled": enabled })) {
            Ok(_) => {
                self.settings.midi_inputs_off = self.projects.midi_inputs_off();
                self.settings_changed = true;
            }
            Err(e) => self.status = Status::Error(e.0),
        }
        self.list_midi_inputs();
    }

    /// Plays on the output `choice` names from now on (`audio.setOutput`): the active project's
    /// output opens again there, the transport where it was. When only the cue changed, the
    /// cue's command runs instead (`output.setCuePair`, `output.setCueVolume`).
    fn choose_output(&mut self, choice: &suhina_session::OutputChoice) {
        let current = self.projects.output_choice().clone();
        if choice.device_only() == current.device_only() && choice.cue_pair == current.cue_pair {
            let db =
                if choice.cue_gain > 0.0 { json!(20.0 * f64::from(choice.cue_gain).log10()) } else { json!("-inf") };
            match self.projects.execute("output.setCueVolume", &json!({ "db": db })) {
                Ok(result) => {
                    let db =
                        result["cue_db"].as_f64().map_or_else(|| "silent".to_string(), |db| format!("{db:+.0} dB"));
                    self.status = Status::Done(format!("Cue volume: {db}"));
                }
                Err(e) => self.status = Status::Error(e.0),
            }
            return;
        }
        if choice.device_only() == current.device_only() {
            match self.projects.execute("output.setCuePair", &json!({ "pair": choice.cue_pair.unwrap_or(0) })) {
                Ok(result) => {
                    self.status = match (result["notice"].as_str(), choice.cue_pair) {
                        (Some(notice), _) => Status::Hint(notice.to_string()),
                        (None, Some(pair)) => {
                            let (first, second) = suhina_session::audio::cue_channels(pair);
                            Status::Done(format!("Cue: outputs {first}/{second}"))
                        }
                        (None, None) => Status::Done("Cue: with the main output".to_string()),
                    };
                }
                Err(e) => self.status = Status::Error(e.0),
            }
            return;
        }
        let params = choice.to_json();
        let params = json!({
            "host": params["host"].as_str().unwrap_or(""),
            "device": params["device"].as_str().unwrap_or(""),
            "sample_rate": choice.sample_rate.unwrap_or(0),
            "buffer_frames": choice.buffer_frames.unwrap_or(0),
            "exclusive": choice.exclusive,
            "input": choice.input.as_text(),
        });
        match self.projects.execute("audio.setOutput", &params) {
            Ok(result) => {
                self.status = match result["notice"].as_str() {
                    Some(notice) => Status::Hint(notice.to_string()),
                    None => Status::Done(format!(
                        "Audio output: {}",
                        self.session()
                            .and_then(Session::player)
                            .and_then(suhina_session::Player::output_info)
                            .map_or_else(|| "chosen; it opens with the next project".to_string(), |i| i.describe())
                    )),
                };
            }
            Err(e) => self.status = Status::Error(e.0),
        }
    }

    /// Whether a plug-in scan is running.
    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Settings as a dialog over the window: the view stays behind it, dimmed, and its clicks
    /// are swallowed. Escape, Close and the Settings key close it.
    fn settings_dialog(&mut self, ui: &Ui, window: Rect) {
        egui::Area::new(Id::new("settings-dialog")).order(egui::Order::Foreground).fixed_pos(window.min).show(
            ui.ctx(),
            |ui| {
                ui.set_min_size(window.size());
                let behind = ui.interact(window, Id::new("settings-behind"), Sense::click_and_drag());
                let _swallowed = behind.clicked();
                ui.painter().rect_filled(window, 0.0, theme::window_background().gamma_multiply(0.6));
                let size = Vec2::new(
                    SETTINGS_SIZE.x.min(window.width() - 2.0 * PAD),
                    SETTINGS_SIZE.y.min(window.height() - 2.0 * PAD),
                );
                let panel = Rect::from_center_size(window.center(), size);
                ui.scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| self.settings_page(ui));
                ui.painter().rect_stroke(panel, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
            },
        );
    }

    /// The Settings page, applying each change as it is made.
    fn settings_page(&mut self, ui: &mut Ui) {
        if !self.plugins_read {
            self.read_plugins();
        }
        let themes = self.themes.view(&self.settings.theme);
        self.about.projects_default = self.default_folders.0.display().to_string();
        self.about.templates_default = self.default_folders.1.display().to_string();
        self.about.nam_models_default = self.nam_default_folders.0.display().to_string();
        self.about.irs_default = self.nam_default_folders.1.display().to_string();
        self.about.last_backup = match self.session().and_then(Session::last_backup) {
            Some(backup) => {
                format!("{} to {}", suhina_session::backup::date_time(backup.written), backup.path.display())
            }
            None => "None written for this project yet".into(),
        };
        let tag_limit = self.projects.file_library_mut().tag_limit();
        self.about.audio_live = self.session().and_then(Session::player).and_then(suhina_session::Player::output_info);
        if self.page.shows_devices() {
            // The latency and underruns change while it plays, and controllers come and go.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            if self.midi_listed.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(2)) {
                self.list_midi_inputs();
            }
        }
        let sources = Sources {
            about: &self.about,
            plugins: &mut self.plugins,
            themes: &themes,
            keymap: &mut self.keymap,
            tag_limit,
        };
        let outcome = settings_view::show(ui, &mut self.settings, sources, &mut self.page);
        if let Some(request) = outcome.themes.clone() {
            if let Some(id) = self.themes.request(&self.settings.theme, request) {
                self.settings.theme = id;
                self.settings_changed = true;
            }
            theme::set_palette(ui.ctx(), self.themes.palette(&self.settings.theme));
        }
        if outcome.keymap_changed {
            self.keymap_changed = true;
        }
        if let Some(limit) = outcome.tag_limit
            && let Err(e) = self.projects.execute("library.setTag", &json!({ "limit": limit }))
        {
            self.status = Status::Error(e.0);
        }
        if let Some(request) = outcome.plugins.clone() {
            self.plugin_request(request);
        }
        if outcome.audio_refresh {
            self.list_audio_devices();
        }
        if let Some(field) = outcome.pick {
            self.ask_pick(field);
        }
        if let Some(choice) = outcome.audio_output.clone() {
            self.choose_output(&choice);
        }
        if let Some((port, enabled)) = outcome.midi_input.clone() {
            self.set_midi_input(&port, enabled);
        }
        if outcome.changed {
            self.projects.set_backup_settings(self.settings.backup, self.settings.backup_clean_up);
            motion::set_enabled(ui.ctx(), self.settings.animations);
            self.arrangement.headers_left = self.settings.track_headers == Side::Left;
            self.settings_changed = true;
        }
        if outcome.close {
            self.settings_open = false;
        }
    }

    /// The menu bar: the same actions as the shortcuts and the views' controls.
    fn menu_bar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let has_project = self.session().is_some();
        let (can_undo, can_redo) = self.session().map_or((false, false), |s| (s.can_undo(), s.can_redo()));
        let several = self.projects.len() >= 2;
        let keymap = self.keymap.clone();
        let keys = move |action: &str| keymap.label(action).unwrap_or_default();
        ui.style_mut().visuals.widgets.inactive.fg_stroke = theme::hairline(theme::text_secondary());
        // Whether the File menu showed last frame, so its recent list is read once as it opens.
        let file_menu_was_open = std::mem::take(&mut self.file_menu_open);
        egui::MenuBar::new().ui(ui, |ui| {
            bar_menu(ui, "File", |ui| {
                // The start panel already offers new and open; the dialog would only cover it.
                if ui.add(egui::Button::new("New or open…").shortcut_text(keys("file.new"))).clicked()
                    && !self.start_panel_open()
                {
                    self.open_project_dialog(false);
                }
                if ui.add(egui::Button::new("Open…").shortcut_text(keys("file.open"))).clicked() {
                    self.open_project_dialog(true);
                }
                self.open_recent_menu(ui, file_menu_was_open);
                ui.separator();
                if ui.add_enabled(has_project, egui::Button::new("Save").shortcut_text(keys("file.save"))).clicked() {
                    self.save_active(false);
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Save as…").shortcut_text(keys("file.saveAs")))
                    .clicked()
                {
                    self.save_active(true);
                }
                if ui
                    .add_enabled(
                        has_project,
                        egui::Button::new("Save as template…").shortcut_text(keys("file.saveTemplate")),
                    )
                    .clicked()
                {
                    self.save_template();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            bar_menu(ui, "Edit", |ui| {
                if ui.add_enabled(can_undo, egui::Button::new("Undo").shortcut_text(keys("edit.undo"))).clicked() {
                    actions.push(Action::new("edit.undo", json!({})));
                }
                if ui.add_enabled(can_redo, egui::Button::new("Redo").shortcut_text(keys("edit.redo"))).clicked() {
                    actions.push(Action::new("edit.redo", json!({})));
                }
            });
            bar_menu(ui, "View", |ui| {
                if ui
                    .add_enabled(has_project, egui::Button::new("Session").shortcut_text(keys("view.session")))
                    .clicked()
                {
                    self.shown = View::Session;
                    self.settings_open = false;
                }
                if ui
                    .add_enabled(has_project, egui::Button::new("Arrangement").shortcut_text(keys("view.arrangement")))
                    .clicked()
                {
                    self.shown = View::Arrangement;
                    self.settings_open = false;
                }
                let label = if self.sample_map.view.window { "Dock sample map" } else { "Sample map in a window" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.sampleMapWindow"))).clicked() {
                    self.toggle_map_window();
                }
                let label = if self.sample_map.view.large { "Hide sample map" } else { "Sample map" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.sampleMap"))).clicked() {
                    self.settings_open = false;
                    self.toggle_large_map();
                }
                let missing = self.missing.rows.len();
                let label =
                    if missing == 0 { "Missing files…".to_string() } else { format!("Missing files ({missing})…") };
                if ui.add_enabled(missing > 0, egui::Button::new(label)).clicked() {
                    self.missing.open = true;
                    self.settings_open = false;
                }
                let label = if self.settings.browser_open { "Hide browser" } else { "Show browser" };
                if ui.add(egui::Button::new(label).shortcut_text(keys("view.browser"))).clicked() {
                    self.toggle_browser();
                }
                let label = if self.detail_open { "Hide detail area" } else { "Show detail area" };
                if ui.add_enabled(has_project, egui::Button::new(label).shortcut_text(keys("view.detail"))).clicked() {
                    self.detail_open = !self.detail_open;
                    self.editor_closed = false;
                }
                // The selected clip's editor (› Least height and closing in ui-clip-editor.md).
                let label = if self.detail_open { "Hide editor" } else { "Show editor" };
                let clip = has_project && self.shown_clip().is_some();
                if ui.add_enabled(clip, egui::Button::new(label).shortcut_text(keys("view.editor"))).clicked() {
                    self.toggle_editor();
                }
                // The detail area's Clip / Devices switch, for a clip with an editor.
                let devices = self.detail_open && self.detail_showing == detail_view::Showing::Devices;
                let label = if devices { "Show clip" } else { "Show devices" };
                let editor = has_project && self.piano_clip(self.shown_clip()).is_some();
                if ui.add_enabled(editor, egui::Button::new(label).shortcut_text(keys("view.devices"))).clicked() {
                    self.toggle_devices();
                }
                ui.separator();
                let (zoom, limits) = (self.settings.zoom, crate::settings::ZOOMS);
                if ui
                    .add_enabled(zoom < limits.1, egui::Button::new("Zoom in").shortcut_text(keys("view.zoomIn")))
                    .clicked()
                {
                    self.step_zoom(1);
                }
                if ui
                    .add_enabled(zoom > limits.0, egui::Button::new("Zoom out").shortcut_text(keys("view.zoomOut")))
                    .clicked()
                {
                    self.step_zoom(-1);
                }
                let reset = egui::Button::new(format!("Actual size ({zoom} %)")).shortcut_text(keys("view.zoomReset"));
                if ui.add_enabled(zoom != crate::settings::ZOOM, reset).clicked() {
                    self.settings.zoom = crate::settings::ZOOM;
                    self.settings_changed = true;
                }
            });
            bar_menu(ui, "Projects", |ui| {
                if ui
                    .add_enabled(several, egui::Button::new("Next project").shortcut_text(keys("projects.next")))
                    .clicked()
                {
                    actions.push(Action::new("projects.next", json!({})));
                }
                if ui
                    .add_enabled(
                        several,
                        egui::Button::new("Previous project").shortcut_text(keys("projects.previous")),
                    )
                    .clicked()
                {
                    actions.push(Action::new("projects.previous", json!({})));
                }
                if let Some(active) = self.projects.active()
                    && ui.button("Close project").clicked()
                {
                    actions.push(Action::new("projects.close", json!({ "project": active.0 })));
                }
            });
            bar_menu(ui, "Options", |ui| {
                if ui.add(egui::Button::new("Settings").shortcut_text(keys("settings.open"))).clicked() {
                    self.settings_open = true;
                }
            });
            bar_menu(ui, "Help", |ui| {
                if ui.add(egui::Button::new("About Suhina").shortcut_text(keys("help.about"))).clicked() {
                    self.about_open = true;
                }
            });
        });
    }

    /// Handles files dropped from the system: folders, themes and projects here; the samples and
    /// MIDI files are returned, to land where the views say they are dropped.
    fn dropped_files(&mut self, ui: &Ui) -> Vec<BrowserItem> {
        // The missing files panel takes a dropped folder as the folder to search.
        if self.missing.open {
            return Vec::new();
        }
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        let is = |path: &PathBuf, ext: &str| path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
        let mut opened = false;
        let mut clips = Vec::new();
        for path in dropped {
            if let Some(item) = drop::system_file(&path).filter(|_| !path.is_dir()) {
                clips.push(item);
            } else if path.is_dir() {
                match self.projects.execute("library.addFolder", &json!({ "folder": path.display().to_string() })) {
                    Ok(_) => {
                        self.settings.browser_open = true;
                        self.browser.relist();
                        self.status = Status::Done(format!("Added {} to the browser", path.display()));
                    }
                    Err(e) => self.status = Status::Error(e.0),
                }
            } else if is(&path, "json") || is(&path, "zip") {
                self.themes.import(path);
            } else if (is(&path, "als") || suhina_session::is_project_file(&path)) && !opened {
                opened = true;
                self.request_open(path);
            } else {
                let what = "is not a sample, a MIDI file, a project (.suhina), a Live Set (.als), a theme (.json), a zip of themes or a folder for the browser";
                self.status = Status::Error(format!("{} {what}", path.display()));
            }
        }
        clips
    }

    /// Samples and MIDI files dragged over the window from the system, while it reports them.
    fn hovered_files(&self, ui: &Ui) -> Vec<BrowserItem> {
        if self.missing.open {
            return Vec::new();
        }
        ui.input(|i| {
            i.raw
                .hovered_files
                .iter()
                .filter_map(|f| f.path.as_deref())
                .filter(|p| !p.is_dir())
                .filter_map(drop::system_file)
                .collect()
        })
    }

    /// Keys mapped to slots, scenes and stop buttons, and the keys of the mapping mode, before
    /// the shortcuts see them: only while the Session view is shown and nothing else has the
    /// keyboard (`docs/spec/ui-session-view.md` › Launch keys). A key held when that ends, or
    /// when the window loses focus, is released.
    fn launch_keys(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        let dialog = self.project_dialog.is_some() || self.save_prompt.is_some() || self.unsaved_prompt.is_some();
        let session_view = self.shown == View::Session && !self.sample_map.view.large;
        // A clicked slot or button keeps the focus; only a text field takes the keys away.
        let focused = session_view && !dialog && !self.settings_open && !ui.ctx().text_edit_focused();
        let lost = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::WindowFocused(false))));
        let Some(project) = self.projects.active().and_then(|id| self.projects.get(id)).map(Session::project) else {
            return;
        };
        let keys = &mut self.view.launch_keys;
        if !focused || lost {
            keys.release_all(project, actions);
        }
        if !session_view && keys.mode.on {
            keys.toggle_mode();
        }
        if !focused {
            return;
        }
        let events = ui.input(|i| crate::session::launch_keys::key_events(&i.events));
        let mut taken = Vec::new();
        for (index, event) in events {
            if keys.handle(project, &event, actions) == crate::session::launch_keys::Outcome::Taken {
                taken.push(index);
            }
        }
        if !taken.is_empty() {
            ui.input_mut(|i| {
                let mut index = 0;
                i.events.retain(|_| {
                    let keep = !taken.contains(&index);
                    index += 1;
                    keep
                });
            });
        }
    }

    /// Turns the mapping mode on (showing the Session view) or off.
    fn toggle_mapping_mode(&mut self) {
        let keys = &mut self.view.launch_keys;
        keys.toggle_mode();
        if keys.mode.on {
            self.shown = View::Session;
            self.close_large_map();
            self.status = Status::Hint(
                "Mapping keys: click a slot, a scene or a stop button, then press a key. Delete removes, Escape leaves"
                    .into(),
            );
        } else {
            self.status = Status::Hint("Mapping keys off".into());
        }
    }

    /// Runs the actions whose keys were pressed (the keymap), unless a text field has the
    /// keyboard or Settings is waiting for a key to bind.
    fn shortcuts(&mut self, ui: &Ui, actions: &mut Vec<Action>) {
        let dialog = self.project_dialog.is_some()
            || self.save_prompt.is_some()
            || self.unsaved_prompt.is_some()
            || self.about_open;
        if ui.ctx().egui_wants_keyboard_input() || dialog || (self.settings_open && self.page.capture.is_some()) {
            return;
        }
        // The piano roll's own keys only while it has the keyboard; the selected slot's keys
        // only while it does not, as the two share keys.
        let notes = self.piano_roll.focused;
        let pressed = ui.input_mut(|i| {
            self.keymap.pressed(i, |id| {
                if notes {
                    !crate::settings::keymap::not_in_piano_roll(id)
                } else {
                    !crate::settings::keymap::piano_roll_only(id)
                }
            })
        });
        for action in pressed {
            self.do_action(&action, actions);
        }
    }

    /// Does one keymap action: the window's own, one on the selection, or a command that needs
    /// no parameters.
    pub fn do_action(&mut self, action: &str, actions: &mut Vec<Action>) {
        let has_project = !self.projects.is_empty();
        match action {
            "settings.open" => self.settings_open = !self.settings_open,
            "help.about" => self.about_open = !self.about_open,
            "file.new" if self.start_panel_open() => {}
            "file.new" => self.open_project_dialog(false),
            // With clips selected in the Arrangement, the loop is exactly their span; with none,
            // Ctrl+L turns looping on or off as before.
            // A time selection loops exactly its span; a click on a clip ends it, so then the
            // clips' span is looped (below).
            "transport.setLoopEnabled"
                if self.shown == View::Arrangement && self.arrangement.time_selection().is_some() =>
            {
                if let Some(s) = self.arrangement.time_selection() {
                    actions.push(Action::new(
                        "transport.setLoop",
                        json!({ "start": s.start, "end": s.end, "enabled": true }),
                    ));
                }
            }
            "transport.setLoopEnabled"
                if self.shown == View::Arrangement && !self.arrangement.selected_clips().is_empty() =>
            {
                let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                let span = self
                    .arrangement
                    .selected_clips()
                    .iter()
                    .filter_map(|(track, index)| {
                        timeline?
                            .lanes
                            .iter()
                            .find(|l| l.track == *track)?
                            .clips
                            .iter()
                            .find(|c| c.index == *index)
                            .map(|c| (c.start, c.start + c.length))
                    })
                    .fold(None, |span: Option<(f64, f64)>, (start, end)| {
                        Some(span.map_or((start, end), |(s, e)| (s.min(start), e.max(end))))
                    });
                if let Some((start, end)) = span.filter(|(s, e)| e > s) {
                    actions
                        .push(Action::new("transport.setLoop", json!({ "start": start, "end": end, "enabled": true })));
                }
            }
            "file.open" => self.open_project_dialog(true),
            "view.browser" if self.keymap.keys("view.browser").is_empty() => {
                // A keymap preset (Draw-first) may leave the browser with no key: the window's own
                // action then does nothing, so the file and what happens on a key press agree.
            }
            "view.browser" => self.toggle_browser(),
            "view.sampleMap" => {
                self.settings_open = false;
                self.toggle_large_map();
            }
            "view.sampleMapWindow" => self.toggle_map_window(),
            "view.zoomIn" => self.step_zoom(1),
            "view.zoomOut" => self.step_zoom(-1),
            "view.zoomReset" => {
                if self.settings.zoom != crate::settings::ZOOM {
                    self.settings.zoom = crate::settings::ZOOM;
                    self.settings_changed = true;
                }
            }
            "browser.search" => {
                if !self.settings.browser_open {
                    self.toggle_browser();
                }
                self.settings_open = false;
                self.browser.focus_search = true;
            }
            _ if !has_project => {}
            "file.save" => self.save_active(false),
            "file.saveAs" => self.save_active(true),
            "file.saveTemplate" => self.save_template(),
            "view.toggle" => {
                self.shown = match self.shown {
                    View::Session => View::Arrangement,
                    View::Arrangement => View::Session,
                };
            }
            "view.session" => {
                self.shown = View::Session;
                self.close_large_map();
            }
            "view.detail" => {
                self.detail_open = !self.detail_open;
                self.editor_closed = false;
            }
            "view.editor" if self.shown_clip().is_some() => self.toggle_editor(),
            "view.editor" => {}
            "view.devices" => self.toggle_devices(),
            "view.plugin" => self.toggle_plugin_editor(),
            "view.mappingMode" => self.toggle_mapping_mode(),
            "track.addAudio" => actions.push(Action::new("track.add", json!({ "kind": "audio" }))),
            "track.addMidi" => actions.push(Action::new("track.add", json!({ "kind": "midi" }))),
            "view.automation" => {
                if let Some((_, _, timeline)) = self.timeline.as_ref() {
                    let tracks: Vec<TrackId> = timeline.lanes.iter().map(|l| l.track).collect();
                    self.arrangement.automation.toggle_all(&tracks, &timeline.automation);
                }
            }
            // An empty MIDI clip over the time selection on each of its MIDI tracks (Ableton
            // Live's Ctrl+Shift+M).
            "clip.insertMidi" => {
                let Some(span) = self.arrangement.time_selection().cloned() else {
                    self.status = Status::Hint("Select a time range in the Arrangement first".into());
                    return;
                };
                let notes: Vec<TrackId> = self.timeline.as_ref().map_or_else(Vec::new, |(_, _, t)| {
                    t.lanes.iter().filter(|l| l.notes && span.tracks.contains(&l.track)).map(|l| l.track).collect()
                });
                if notes.is_empty() {
                    self.status = Status::Hint("The time selection has no MIDI track".into());
                }
                for track in notes {
                    actions.push(Action::new(
                        "clip.create",
                        json!({ "track": track.0, "start": span.start, "length": span.end - span.start }),
                    ));
                }
            }
            "view.arrangement" => {
                self.shown = View::Arrangement;
                self.close_large_map();
            }
            "transport.toggle" => {
                let running = self.session().and_then(|s| s.player()).is_some_and(|p| p.monitor().running());
                actions.push(if running { self.stop_action() } else { self.play_action(false) });
            }
            "transport.playFromSelection" => actions.push(self.play_action(true)),
            // Quantise in the piano roll, while it shows a clip.
            "pianoRoll.quantize" => {
                let clip = match self.shown {
                    View::Session => self.view.selected_slot.map(|(t, s)| detail_view::ClipRef::Slot(t, s)),
                    View::Arrangement => {
                        self.arrangement.selected_clip().map(|(t, i)| detail_view::ClipRef::Arrangement(t, i))
                    }
                };
                let showing = self.detail_open
                    && self.detail_showing == detail_view::Showing::Clip
                    && self.piano_clip(clip).is_some();
                if showing && let Some(quantize) = self.piano_roll.quantize_selected() {
                    actions.push(quantize);
                }
            }
            // Delete in the piano roll deletes its selected notes, not the clip.
            "selection.delete" if let Some(delete) = self.piano_roll.delete_selected() => actions.push(delete),
            // Copy, cut, paste and duplicate in the piano roll act on its notes, when it has some
            // to act on; its own keys act only there.
            "selection.copy" | "selection.cut" | "selection.paste" | "selection.duplicate"
                if let Some(edit) = self.piano_key(action) =>
            {
                actions.push(edit)
            }
            _ if crate::settings::keymap::piano_roll_only(action) => actions.extend(self.piano_key(action)),
            // In the Session view copy and paste act on the selected clip slot.
            // Picked devices (a click on a strip's device or a panel of the chain): copy, cut
            // and paste act on them, before any clip.
            "selection.copy" | "selection.cut" if self.device_focus && self.selected_device.is_some() => {
                let picks = self.device_picks();
                let Some((track, _)) = picks.first().copied() else { return };
                let devices: Vec<usize> = picks.iter().map(|(_, i)| *i).collect();
                actions.push(Action::new("device.copy", json!({ "track": track.0, "devices": devices })));
                if action == "selection.cut" {
                    self.selected_device = None;
                    self.device_picks.clear();
                    actions.push(Action::new("device.removeMany", json!({ "track": track.0, "devices": devices })));
                }
            }
            "selection.paste"
                if self.device_focus
                    && self.selected_device.is_some()
                    && self.session().is_some_and(|s| s.copied_devices() > 0) =>
            {
                let picks = self.device_picks();
                let Some((track, _)) = picks.first().copied() else { return };
                let after = picks.iter().map(|(_, i)| *i).max().unwrap_or(0) + 1;
                actions.push(Action::new("device.paste", json!({ "track": track.0, "position": after })));
            }
            "selection.copy" | "selection.paste" if self.shown == View::Session => {
                let Some((track, scene)) = self.view.selected_slot else {
                    self.status = Status::Hint("Click a clip slot first".into());
                    return;
                };
                let params = json!({ "track": track.0, "scene": scene + 1 });
                actions.push(Action::new(if action == "selection.copy" { "clip.copy" } else { "clip.paste" }, params));
            }
            // Cut and duplicate of a slot's clip wait for launcher clip commands; Delete works,
            // in the arm below.
            "selection.cut" | "selection.duplicate" if self.shown == View::Session => {
                self.status = Status::Hint("In the Session view, a slot's clip can be copied and pasted".into());
            }
            // A time selection in the Arrangement: the keys act on what plays inside it, as in
            // Ableton Live and Studio One; clips picked by a click act whole (the arm below).
            "selection.copy"
            | "selection.cut"
            | "selection.duplicate"
            | "selection.delete"
            | "selection.split"
            | "selection.consolidate"
                if self.shown == View::Arrangement
                    && !self.device_focus
                    && self.arrangement.automation.selected_points().is_none()
                    && self.arrangement.time_selection().is_some() =>
            {
                let Some(span) = self.arrangement.time_selection().cloned() else { return };
                let tracks: Vec<u32> = span.tracks.iter().map(|t| t.0).collect();
                let params = json!({ "tracks": tracks, "start": span.start, "end": span.end });
                match action {
                    "selection.delete" => actions.push(Action::new("range.delete", params)),
                    "selection.cut" => actions.push(Action::new("range.cut", params)),
                    "selection.copy" => actions.push(Action::new("range.copy", params)),
                    "selection.split" => actions.push(Action::new("range.split", params)),
                    "selection.duplicate" => {
                        actions.push(Action::new("range.duplicate", params));
                        // The selection moves onto the copy, so Ctrl+D again goes on.
                        let length = span.end - span.start;
                        self.arrangement.set_time_selection(Some(
                            crate::arrangement::arrangement_view::TimeSelection {
                                start: span.end,
                                end: span.end + length,
                                ..span
                            },
                        ));
                    }
                    _ => {
                        // Each track of the span with something in it: the new clip runs over the
                        // whole span, its empty part silent, as Ableton Live consolidates.
                        let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                        let playing = |track: u32| {
                            timeline.and_then(|t| t.lanes.iter().find(|l| l.track.0 == track)).is_some_and(|l| {
                                l.clips.iter().any(|c| c.start < span.end && c.start + c.length > span.start)
                            })
                        };
                        for track in tracks.into_iter().filter(|t| playing(*t)) {
                            actions.push(Action::new(
                                "clip.consolidate",
                                json!({ "track": track, "start": span.start, "end": span.end }),
                            ));
                        }
                    }
                }
            }
            "selection.copy" | "selection.cut" | "selection.duplicate" | "selection.delete" => {
                // Points selected on an automation lane are deleted before any clip.
                if action == "selection.delete"
                    && let Some(delete) = self.arrangement.automation.delete_selected()
                {
                    actions.push(delete);
                    return;
                }
                // A device selected in the detail area (its panel's click) is deleted before any
                // clip, while the detail shows the chain.
                if action == "selection.delete"
                    && (self.detail_shows_devices() || self.device_focus)
                    && let Some((track, index)) = self.selected_device
                    && self.session().is_some_and(|s| {
                        let p = s.project();
                        p.tracks
                            .iter()
                            .find(|t| t.id == track)
                            .and_then(|t| t.channel)
                            .is_some_and(|c| p.channels.iter().any(|ch| ch.id == c && ch.devices.get(index).is_some()))
                    })
                {
                    let picks = self.device_picks();
                    self.selected_device = None;
                    self.device_picks.clear();
                    if picks.len() > 1 {
                        let devices: Vec<usize> = picks.iter().map(|(_, i)| *i).collect();
                        actions.push(Action::new("device.removeMany", json!({ "track": track.0, "devices": devices })));
                    } else {
                        actions.push(Action::new("device.remove", json!({ "track": track.0, "device": index })));
                    }
                    return;
                }
                // The Session view: Delete takes the selected slot's clip off its slot, and the
                // detail area falls back to the track's devices.
                if self.shown == View::Session {
                    if action == "selection.delete"
                        && let Some((track, scene)) = self.view.selected_slot
                        && self.session().is_some_and(|s| {
                            let p = s.project();
                            p.launcher
                                .columns
                                .iter()
                                .any(|c| c.track == track && c.slots.get(scene).is_some_and(|slot| slot.clip.is_some()))
                        })
                    {
                        self.view.selected_slot = None;
                        actions.push(Action::new("clip.delete", json!({ "track": track.0, "scene": scene + 1 })));
                    }
                    return;
                }
                let clips = self.selected_clip_list();
                if clips.is_empty() {
                    return;
                }
                let id = match action {
                    "selection.copy" => "clip.copy",
                    "selection.cut" => "clip.cut",
                    "selection.duplicate" => "clip.duplicate",
                    _ => "clip.delete",
                };
                if matches!(id, "clip.cut" | "clip.delete") {
                    self.arrangement.clear_selected_clip();
                }
                let mut params = json!({ "clips": clips });
                // A copy follows the stacking setting for its kind, as a moved clip does: with
                // clips of both kinds selected, only a setting that covers them all stacks.
                if id == "clip.duplicate" && self.stacks_every_selected_clip() {
                    params["stack"] = json!(true);
                }
                actions.push(Action::new(id, params));
            }
            "selection.rename" => {
                // The selected clip: the Arrangement's, or the Session view's slot.
                let target = match self.shown {
                    View::Arrangement => self.arrangement.selected_clip().map(|(track, index)| {
                        let name = self
                            .timeline
                            .as_ref()
                            .and_then(|(_, _, t)| t.lanes.iter().find(|l| l.track == track))
                            .and_then(|l| l.clips.iter().find(|c| c.index == index))
                            .map(|c| c.name.clone())
                            .unwrap_or_default();
                        (json!({ "track": track.0, "index": index }), name)
                    }),
                    View::Session => self.view.selected_slot.and_then(|(track, scene)| {
                        let grid = self.session().map(|s| Grid::build(s.project(), None))?;
                        let slot = grid.columns.iter().find(|c| c.track == track)?.slots.get(scene)?;
                        (slot.state != crate::interaction::grid::SlotState::Empty)
                            .then(|| (json!({ "track": track.0, "scene": scene + 1 }), slot.name.clone()))
                    }),
                };
                match target {
                    Some((target, name)) => self.rename = Some((target, name, false)),
                    None => self.status = Status::Hint("Select a clip first".into()),
                }
            }
            "selection.split" | "selection.consolidate" => {
                let Some((track, index)) = self.arrangement.selected_clip() else {
                    self.status = Status::Hint("Select a clip in the Arrangement first".into());
                    return;
                };
                let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                let beat = timeline.map_or(0.0, |t| t.beat);
                let clip = timeline
                    .and_then(|t| t.lanes.iter().find(|l| l.track == track))
                    .and_then(|l| l.clips.iter().find(|c| c.index == index))
                    .map(|c| (c.start, c.length));
                let Some((start, length)) = clip else { return };
                if action == "selection.consolidate" {
                    // The selected clips on its track, from the first start to the last end,
                    // become one new clip.
                    let lane = timeline.and_then(|t| t.lanes.iter().find(|l| l.track == track));
                    let (from, to) = self
                        .arrangement
                        .selected_clips()
                        .iter()
                        .filter(|(t, _)| *t == track)
                        .filter_map(|(_, i)| lane.and_then(|l| l.clips.iter().find(|c| c.index == *i)))
                        .fold((start, start + length), |(s, e), c| (s.min(c.start), e.max(c.start + c.length)));
                    self.arrangement.clear_selected_clip();
                    let params = json!({ "track": track.0, "start": from, "end": to });
                    actions.push(Action::new("clip.consolidate", params));
                } else if beat > start && beat < start + length {
                    actions.push(Action::new("clip.split", json!({ "track": track.0, "index": index, "at": beat })));
                } else {
                    self.status = Status::Hint("Move the playhead into the selected clip to split it there".into());
                }
            }
            "selection.mute" => {
                // A device panel clicked last: the key turns that device off or on again, as its
                // header's square does.
                if (self.detail_shows_devices() || self.device_focus)
                    && let Some((track, index)) = self.selected_device
                    && let Some(enabled) = self.session().and_then(|s| {
                        let p = s.project();
                        let channel = p.tracks.iter().find(|t| t.id == track).and_then(|t| t.channel)?;
                        p.channels.iter().find(|c| c.id == channel)?.devices.get(index).map(|d| d.enabled)
                    })
                {
                    actions.push(Action::new(
                        "device.setEnabled",
                        json!({ "track": track.0, "device": index, "enabled": !enabled }),
                    ));
                    return;
                }
                let clips = self.selected_clip_list();
                if clips.is_empty() {
                    return;
                }
                // What the clips are now decides which way the key turns them: any of them
                // playing turns the whole selection off, else all of them back on.
                let enabled = self.any_selected_clip_plays();
                actions.push(Action::new("clip.setEnabled", json!({ "clips": clips, "enabled": !enabled })));
            }
            "selection.selectAll" => {
                let clips: Vec<(TrackId, usize)> = self
                    .session()
                    .map(|session| {
                        session
                            .project()
                            .arrangement
                            .lanes
                            .iter()
                            .filter_map(|lane| lane.track.map(|track| (track, lane.clips.len())))
                            .flat_map(|(track, count)| (0..count).map(move |index| (track, index)))
                            .collect()
                    })
                    .unwrap_or_default();
                self.arrangement.select_clips(clips);
            }
            "selection.paste"
                if self.shown == View::Arrangement && self.session().is_some_and(Session::copied_from_tracks) =>
            {
                // Copied from several tracks: each part back on its own track, from the play
                // start marker, as Ableton Live pastes at its insert marker.
                let beat = self.timeline.as_ref().map_or(0.0, |(_, _, t)| t.beat);
                actions.push(Action::new("clip.paste", json!({ "start": beat })));
            }
            "selection.paste" => {
                // After the selected clip, or at the playhead on the selected track.
                let timeline = self.timeline.as_ref().map(|(_, _, t)| t);
                let target = match self.arrangement.selected_clip() {
                    Some((track, index)) => timeline
                        .and_then(|t| t.lanes.iter().find(|l| l.track == track))
                        .and_then(|l| l.clips.iter().find(|c| c.index == index))
                        .map(|c| (track, c.start + c.length)),
                    None => self.arrangement.selected_tracks.first().map(|t| (*t, timeline.map_or(0.0, |t| t.beat))),
                };
                match target {
                    Some((track, start)) => {
                        let mut params = json!({ "track": track.0, "start": start });
                        // The pasted clip follows the stacking setting for its kind, as a moved clip does.
                        let audio = self.session().and_then(Session::copied_clip_is_audio);
                        if audio.is_some_and(|audio| self.stacks(audio)) {
                            params["stack"] = json!(true);
                        }
                        actions.push(Action::new("clip.paste", params))
                    }
                    None => self.status = Status::Hint("Select a clip or a track to paste on".into()),
                }
            }
            "selection.group" => {
                if self.arrangement.selected_tracks.is_empty() {
                    return;
                }
                let tracks: Vec<u32> = self.arrangement.selected_tracks.iter().map(|t| t.0).collect();
                actions.push(Action::new("track.group", json!({ "tracks": tracks })));
                self.arrangement.selected_tracks.clear();
            }
            "marker.addAtPlayhead" => {
                // Where the playhead is while playing; stopped, where playback would start.
                let monitor = self.session().and_then(|s| s.player()).map(|p| p.monitor());
                let beat = monitor.map_or(0.0, |m| if m.running() { m.beat() } else { m.start_beat() });
                let beat = if beat.is_finite() { (beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 };
                actions.push(Action::new("marker.add", json!({ "beat": beat })));
            }
            "selection.launch" => {
                if let (View::Session, Some((track, scene))) = (self.shown, self.view.selected_slot) {
                    actions.push(Action::new("clip.launch", json!({ "track": track.0, "scene": scene + 1 })));
                }
            }
            "selection.up" | "selection.down" | "selection.left" | "selection.right" => {
                let (View::Session, Some(from)) = (self.shown, self.view.selected_slot) else { return };
                let step = match action {
                    "selection.up" => Step::Up,
                    "selection.down" => Step::Down,
                    "selection.left" => Step::Left,
                    _ => Step::Right,
                };
                let grid = self.session().map(|s| Grid::build(s.project(), None));
                if let Some((track, scene)) = grid.and_then(|g| session_view::step_slot(&g, from, step)) {
                    self.view.select_slot(track, scene);
                }
            }
            "projects.next" | "projects.previous" if self.projects.len() < 2 => {}
            // Ctrl+Tab opens the switcher, or moves on in it while Ctrl stays down; letting go
            // of Ctrl switches (`tab_switcher_ui`).
            "projects.next" | "projects.previous" => {
                let ids = self.projects.ids();
                let count = ids.len();
                let from = self
                    .tab_switcher
                    .or_else(|| self.projects.active().and_then(|a| ids.iter().position(|i| *i == a)))
                    .unwrap_or(0);
                let step = if action == "projects.next" { 1 } else { count - 1 };
                self.tab_switcher = Some((from + step) % count);
            }
            command => actions.push(Action::new_owned(command, json!({}))),
        }
    }

    /// Whether a clip of this kind (audio, or notes) leaves the clips it lands on as they are:
    /// Options › Settings › sample stacking for audio, MIDI stacking for notes.
    fn stacks(&self, audio: bool) -> bool {
        if audio { self.settings.sample_stacking } else { self.settings.midi_stacking }
    }

    /// The clips the Arrangement view has selected, as `clip.*` commands name them.
    fn selected_clip_list(&self) -> Vec<serde_json::Value> {
        self.arrangement
            .selected_clips()
            .iter()
            .map(|(track, index)| json!({ "track": track.0, "index": index }))
            .collect()
    }

    /// Whether any selected clip plays now. The window reads the clips it drew; a clip it cannot
    /// find counts as playing, so the first key press turns the selection off.
    fn any_selected_clip_plays(&self) -> bool {
        let Some((_, _, timeline)) = self.timeline.as_ref() else { return true };
        self.arrangement.selected_clips().iter().any(|(track, index)| {
            timeline
                .lanes
                .iter()
                .find(|l| l.track == *track)
                .and_then(|l| l.clips.iter().find(|c| c.index == *index))
                .is_none_or(|c| c.enabled)
        })
    }

    /// Whether the stacking setting covers every selected clip: with clips of both kinds picked,
    /// only when both settings are on does a copy leave what it lands on alone.
    fn stacks_every_selected_clip(&self) -> bool {
        let Some((_, _, timeline)) = self.timeline.as_ref() else { return false };
        let selected = self.arrangement.selected_clips();
        if selected.is_empty() {
            return false;
        }
        selected.iter().all(|(track, index)| {
            timeline
                .lanes
                .iter()
                .find(|l| l.track == *track)
                .and_then(|l| l.clips.iter().find(|c| c.index == *index))
                .is_none_or(|c| self.stacks(c.audio))
        })
    }

    /// Makes the whole window `steps` zoom steps larger or smaller and keeps that in the settings.
    fn step_zoom(&mut self, steps: i32) {
        if self.settings.step_zoom(steps) {
            self.settings_changed = true;
        }
    }

    /// Draws the window at the zoom the settings keep. The keymap owns the zoom keys, so the
    /// toolkit's own Ctrl and +, which the settings would not remember, is turned off.
    fn apply_zoom(&self, ctx: &egui::Context) {
        if ctx.options(|o| o.zoom_with_keyboard) {
            ctx.options_mut(|o| o.zoom_with_keyboard = false);
        }
        let wanted = self.settings.zoom_factor();
        if (ctx.zoom_factor() - wanted).abs() > f32::EPSILON {
            ctx.set_zoom_factor(wanted);
        }
    }

    /// Shows or hides the browser panel, as Ctrl+B does.
    pub fn set_browser_open(&mut self, open: bool) {
        if self.settings.browser_open != open {
            self.toggle_browser();
        }
    }

    /// Hides the selected clip's editor (the detail area), or shows it again.
    fn toggle_editor(&mut self) {
        if self.detail_open {
            self.hide_editor();
        } else {
            self.show_editor();
        }
    }

    fn toggle_browser(&mut self) {
        self.settings.browser_open = !self.settings.browser_open;
        self.settings_changed = true;
        if !self.settings.browser_open {
            browser_view::stop_preview(&mut self.browser, &mut self.projects);
            self.browser.drag = None;
        }
    }

    /// The track selected in the view shown.
    fn selected_track(&self) -> Option<TrackId> {
        match self.shown {
            View::Session => self.view.selected,
            View::Arrangement => self.arrangement.selected_tracks.first().copied(),
        }
    }

    /// Lays out the browser at the side of `area`, draws it with its handle, and returns what is
    /// left for the view.
    fn browser_panel(&mut self, ui: &mut Ui, area: Rect) -> Rect {
        let (low, high) = crate::settings::BROWSER_WIDTHS;
        let widest = (high as f32).min(area.width() * crate::settings::BROWSER_SHARE).max(low as f32);
        let width = (self.settings.browser_width as f32).clamp(low as f32, widest);
        let left = self.settings.browser_side == Side::Left;
        let (panel, handle, rest) = if left {
            let panel = Rect::from_min_max(area.min, Pos2::new(area.left() + width, area.bottom()));
            let handle = Rect::from_min_max(
                Pos2::new(panel.right(), area.top()),
                Pos2::new(panel.right() + BROWSER_HANDLE, area.bottom()),
            );
            (panel, handle, Rect::from_min_max(Pos2::new(handle.right(), area.top()), area.max))
        } else {
            let panel = Rect::from_min_max(Pos2::new(area.right() - width, area.top()), area.max);
            let handle = Rect::from_min_max(
                Pos2::new(panel.left() - BROWSER_HANDLE, area.top()),
                Pos2::new(panel.left(), area.bottom()),
            );
            (panel, handle, Rect::from_min_max(area.min, Pos2::new(handle.left(), area.bottom())))
        };
        let default_width = crate::settings::Settings::default().browser_width;
        if let Some(new) = browser_view::resize_handle(ui, handle, width, left, default_width as f32) {
            let new = new.round().clamp(low as f32, widest) as u32;
            if new != self.settings.browser_width {
                self.settings.browser_width = new;
                self.settings_changed = true;
            }
        }
        let search_key = self.keymap.label("browser.search").unwrap_or_default();
        let selected_track = self.selected_track();
        let revision = self.revision;
        let mut map_settings = self.settings.clone();
        let map_panel = &mut self.sample_map;
        let mut draw_map = |ui: &mut Ui, rect: Rect, state: &mut BrowserState, projects: &mut Projects| {
            map_panel.show(ui, rect, state, projects, &mut map_settings, revision);
        };
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(panel), |ui| {
                let cx = browser_view::Context {
                    projects: &mut self.projects,
                    settings: &self.settings,
                    selected_track,
                    search_key,
                    other_results: Some(&mut draw_map),
                };
                browser_view::show(ui, panel, &mut self.browser, cx)
            })
            .inner;
        if map_settings != self.settings {
            self.settings = map_settings;
            self.settings_changed = true;
        }
        if let Some(why) = self.sample_map.status.take() {
            self.status = Status::Hint(why);
        }
        let added = std::mem::take(&mut self.sample_map.add);
        if !added.is_empty() {
            self.add_to_selected(added);
        }
        if let Some(status) = out.status {
            self.status = status;
        }
        if out.pick_folder {
            self.ask_pick(PickField::BrowserFolder);
        }
        if out.toggle_autoplay {
            self.settings.preview_autoplay = !self.settings.preview_autoplay;
            self.settings_changed = true;
        }
        if let Some(path) = out.open_project {
            self.request_open(path);
        }
        if !out.add.is_empty() {
            self.add_to_selected(out.add);
        }
        rest
    }

    /// Shows the sample map large in the main area, or closes it back into the browser.
    fn toggle_large_map(&mut self) {
        if self.sample_map.view.window {
            self.close_map_window();
        }
        let large = !self.sample_map.view.large;
        self.sample_map.view.large = large;
        self.browser.mode = if large { ResultsMode::List } else { ResultsMode::Other };
    }

    /// Opens the sample map in a window of its own, or puts it back into the browser.
    fn toggle_map_window(&mut self) {
        if self.sample_map.view.window {
            self.close_map_window();
        } else {
            self.settings_open = false;
            self.sample_map.set_window(true, &mut self.browser);
            self.settings.map_window = true;
            self.settings_changed = true;
        }
    }

    fn close_map_window(&mut self) {
        self.sample_map.set_window(false, &mut self.browser);
        self.map_window.drag = None;
        if self.settings.map_window {
            self.settings.map_window = false;
            self.settings_changed = true;
        }
    }

    /// Where a drag from the browser or the map is, in the main window: the pointer, or for a
    /// drag that started in the map's own window, that window's pointer moved onto this one.
    fn drag_pointer(&self, ctx: &egui::Context) -> Option<Pos2> {
        match self.map_window.drag {
            Some(remote) => {
                let main = ctx.input(|i| i.viewport().inner_rect).map_or(Pos2::ZERO, |r| r.min);
                remote.screen.map(|p| p - main.to_vec2())
            }
            None => ctx.pointer_latest_pos(),
        }
    }

    /// Draws the sample map in its own window while it has one (`docs/spec/sample-map.md` ›
    /// Where the map is shown), keeps where the window is, and follows a drag that starts in it.
    fn sample_map_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.sample_map.view.window {
            self.map_window.opened_at = None;
            self.map_window.drag = None;
            return;
        }
        // The browser's own Map switch takes the map back.
        if self.browser.mode == ResultsMode::Other {
            self.close_map_window();
            return;
        }
        let opened_at = *self.map_window.opened_at.get_or_insert(self.settings.map_window_rect);
        let mut builder =
            egui::ViewportBuilder::default().with_title("Sample map - Suhina").with_min_inner_size([320.0, 240.0]);
        builder = match opened_at {
            Some([x, y, w, h]) => builder.with_position([x as f32, y as f32]).with_inner_size([w as f32, h as f32]),
            None => builder.with_inner_size(MAP_WINDOW_SIZE),
        };
        let revision = self.revision;
        let had_drag = self.browser.drag.is_some();
        let mut settings = self.settings.clone();
        let (panel, browser, projects, keymap) =
            (&mut self.sample_map, &mut self.browser, &mut self.projects, &self.keymap);
        let id = egui::ViewportId::from_hash_of("sample-map-window");
        let frame = ctx.show_viewport_immediate(id, builder, |ui, class| {
            let embedded = class == egui::ViewportClass::EmbeddedWindow;
            // Embedded (a toolkit without windows of its own, as in tests) the map gets a fixed size.
            let rect = if embedded {
                Rect::from_min_size(ui.available_rect_before_wrap().min, MAP_WINDOW_SIZE.into())
            } else {
                ui.max_rect()
            };
            ui.allocate_rect(rect, egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, theme::window_background());
            let changed = panel.show(ui, rect, browser, projects, &mut settings, revision);
            // The window has its own keyboard: its keys run the same actions as the main window's,
            // unless a field in it is being typed in. Embedded, the main window reads them.
            let keys = if embedded || ui.ctx().egui_wants_keyboard_input() {
                Vec::new()
            } else {
                ui.input_mut(|i| keymap.pressed(i, |id| !crate::settings::keymap::piano_roll_only(id)))
            };
            ui.input(|i| MapWindowFrame {
                changed,
                keys,
                embedded,
                close: i.viewport().close_requested(),
                place: match (i.viewport().outer_rect, i.viewport().inner_rect) {
                    (Some(outer), Some(inner)) => Some([
                        outer.min.x.round() as i32,
                        outer.min.y.round() as i32,
                        inner.width().round() as i32,
                        inner.height().round() as i32,
                    ]),
                    _ => None,
                },
                screen: i.viewport().inner_rect.zip(i.pointer.latest_pos()).map(|(r, p)| p + r.min.to_vec2()),
                released: !i.pointer.primary_down(),
                escape: i.key_pressed(Key::Escape),
                ctrl: i.modifiers.command,
                time: i.time,
            })
        });
        if frame.changed || settings != self.settings {
            self.settings = settings;
            self.settings_changed = true;
        }
        if let Some(why) = self.sample_map.status.take() {
            self.status = Status::Hint(why);
        }
        let added = std::mem::take(&mut self.sample_map.add);
        if !added.is_empty() {
            self.add_to_selected(added);
        }
        if frame.close {
            self.close_map_window();
            return;
        }
        for action in &frame.keys {
            self.do_action(action, actions);
        }
        if !self.sample_map.view.window {
            return;
        }
        // A drag that starts in the map's own window stays with that window until the button is
        // let go; the main window reads it from there. Embedded, the window shares the input.
        if !frame.embedded && self.browser.drag.is_some() && (self.map_window.drag.is_some() || !had_drag) {
            self.map_window.drag = Some(RemoteDrag {
                screen: frame.screen,
                released: frame.released,
                escape: frame.escape,
                ctrl: frame.ctrl,
            });
        }
        // Where the window is, saved once it has stayed put for a moment.
        if let Some(place) = frame.place.filter(|p| self.settings.map_window_rect != Some(*p)) {
            match self.map_window.moved {
                Some((p, since)) if p == place && frame.time - since >= MAP_WINDOW_SETTLE => {
                    self.settings.map_window_rect = Some(place);
                    self.settings_changed = true;
                    self.map_window.moved = None;
                }
                Some((p, _)) if p == place => {}
                _ => self.map_window.moved = Some((place, frame.time)),
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(MAP_WINDOW_SETTLE));
        }
    }

    fn close_large_map(&mut self) {
        if self.sample_map.view.large {
            self.toggle_large_map();
        }
    }

    /// Adds browser items to the selected track, as Enter and a double-click do: devices to its
    /// chain, files in the Arrangement at the play start marker, in the Session from its first
    /// empty slot; without a selected track, on new tracks.
    fn add_to_selected(&mut self, items: Vec<BrowserItem>) {
        let Some(session) = self.session() else {
            self.status = Status::Hint("Open a project to add to".into());
            return;
        };
        let project = session.project();
        let track = self.selected_track().filter(|t| project.tracks.iter().any(|x| x.id == *t));
        let files = items.iter().any(|i| i.kind.is_file_clip());
        let target = match (track, self.shown) {
            (Some(track), _) if !files => DropTarget::Track(track),
            (Some(track), View::Arrangement) => {
                let beat = session.player().map_or(0.0, |p| p.monitor().start_beat());
                DropTarget::Lane { track, beat }
            }
            (Some(track), View::Session) => {
                let column = project.launcher.columns.iter().find(|c| c.track == track);
                let scene = column
                    .and_then(|c| c.slots.iter().position(|s| s.clip.is_none()))
                    .unwrap_or(project.launcher.scenes.len());
                DropTarget::Slot { track, scene }
            }
            (None, View::Arrangement) => DropTarget::NewTracks { beat: Some(0.0), scene: None },
            (None, View::Session) => DropTarget::NewTracks { beat: None, scene: Some(0) },
        };
        self.drop_items(&items, target, false);
    }

    /// While items are dragged from the browser: the label beside the pointer, Escape to stop,
    /// and the drop when the button is let go.
    fn browser_drag(&mut self, ui: &Ui) {
        let Some(items) = self.browser.drag.clone() else { return };
        let (mut released, mut escape, mut ctrl, mut pointer) = ui.input(|i| {
            (!i.pointer.primary_down(), i.key_pressed(Key::Escape), i.modifiers.command, i.pointer.latest_pos())
        });
        // A drag from the map's own window: that window has the pointer until the button is let go.
        if let Some(remote) = self.map_window.drag {
            (released, escape, ctrl) = (remote.released, remote.escape, remote.ctrl);
            pointer = self.drag_pointer(ui.ctx());
        }
        let target = self.drop_target(&items, pointer);
        if escape {
            self.browser.drag = None;
            self.map_window.drag = None;
            return;
        }
        let separate = ctrl != self.settings.drop_separate;
        if let Some(pointer) = pointer {
            self.drag_label(ui, pointer, &items, target, separate);
        }
        if released {
            self.browser.drag = None;
            self.map_window.drag = None;
            self.view.drop = None;
            self.arrangement.drop = None;
            if let Some(target) = target {
                // Dropped into the project: the preview has done its job.
                browser_view::stop_preview(&mut self.browser, &mut self.projects);
                self.drop_items(&items, target, separate);
            }
        }
    }

    /// Samples and MIDI files from the system over the window: the label beside the pointer while
    /// they hover, and the drop where the views say. Without a pointer position (an OS drag on a
    /// platform that does not report one) they go where Enter puts them.
    fn system_drag(&mut self, ui: &Ui, items: Vec<BrowserItem>, dropped: bool) {
        let pointer = self.drag_pointer(ui.ctx());
        let target = self.drop_target(&items, pointer);
        let separate = ui.input(|i| i.modifiers.command) != self.settings.drop_separate;
        if !dropped {
            if let Some(pointer) = pointer {
                self.drag_label(ui, pointer, &items, target, separate);
            }
            return;
        }
        self.view.drop = None;
        self.arrangement.drop = None;
        match (target, pointer) {
            _ if self.session().is_none() => self.status = Status::Hint("Open a project to add the files to".into()),
            (Some(target), _) => self.drop_items(&items, target, separate),
            (None, None) => self.add_to_selected(items),
            (None, Some(_)) => {
                self.status = Status::Hint("Drop samples and MIDI files on a slot, a lane or the empty area".into());
            }
        }
    }

    /// The target the views found under the pointer, or the detail area's track for devices.
    fn drop_target(&self, items: &[BrowserItem], pointer: Option<Pos2>) -> Option<DropTarget> {
        let target = match self.shown {
            View::Session => self.view.drop.as_ref().and_then(|p| p.target),
            View::Arrangement => self.arrangement.drop.as_ref().and_then(|p| p.target),
        };
        match (target, pointer, self.detail_drop) {
            (None, Some(pointer), Some((detail, track)))
                if detail.contains(pointer) && items.iter().any(|i| i.kind.is_device()) =>
            {
                Some(DropTarget::Track(track))
            }
            _ => target,
        }
    }

    /// The label beside the pointer while dragging: what a drop there does, or what is dragged.
    fn drag_label(&self, ui: &Ui, pointer: Pos2, items: &[BrowserItem], target: Option<DropTarget>, separate: bool) {
        let label = target.and_then(|t| self.plan_drop(items, t, separate)).map(|p| p.label);
        let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("browser-drag"));
        let painter = ui.ctx().layer_painter(layer);
        let text = label.clone().unwrap_or_else(|| match items {
            [one] => one.name.clone(),
            many => format!("{} items", many.len()),
        });
        let font = theme::ui_font(theme::TEXT_SMALL);
        let galley = widgets::fitted(&painter, &text, font, theme::text_primary(), 280.0);
        let rect = Rect::from_min_size(pointer + Vec2::new(14.0, 10.0), galley.size() + Vec2::new(PAD * 2.0, 6.0));
        painter.rect_filled(rect, 0.0, theme::control_active());
        painter.rect_stroke(
            rect,
            0.0,
            theme::hairline(if label.is_some() { theme::accent() } else { theme::border_strong() }),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.min + Vec2::new(PAD, 3.0), galley, theme::text_primary());
        ui.ctx().request_repaint();
    }

    /// Switches to `hovered`, the view whose name a dragged or carried clip is over, once it has
    /// stayed there for [`SWITCH_HOVER`]; a clip dragged in the Arrangement view is carried from
    /// then on.
    fn hover_switch(&mut self, ctx: &egui::Context, hovered: Option<View>) {
        let Some(view) = hovered else {
            self.switch_hover = None;
            return;
        };
        let now = ctx.input(|i| i.time);
        let since = match self.switch_hover {
            Some((v, since)) if v == view => since,
            _ => {
                self.switch_hover = Some((view, now));
                now
            }
        };
        if now - since < SWITCH_HOVER {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(SWITCH_HOVER / 4.0));
            return;
        }
        self.switch_hover = None;
        if self.carry.is_none()
            && let Some((track, index)) = self.arrangement.dragged_clip()
        {
            let name = self
                .timeline
                .as_ref()
                .and_then(|(_, _, t)| t.lanes.iter().find(|l| l.track == track))
                .and_then(|l| l.clips.iter().find(|c| c.index == index))
                .map(|c| c.name.clone())
                .unwrap_or_default();
            self.arrangement.cancel_clip_drag();
            ctx.stop_dragging();
            self.carry = Some(Carry { from: CarryFrom::Arrangement { track, index }, name });
        }
        self.shown = view;
        self.close_large_map();
    }

    /// While a clip is carried between the views: the label beside the pointer, Escape to stop,
    /// and on release a copy of it in the slot or on the lane under the pointer
    /// (`docs/spec/ui-session-view.md` › Between the views). A clip slot's drag starts one.
    fn carry_clip(&mut self, ui: &Ui) {
        if let Some((track, scene)) = self.view.slot_drag.take() {
            let name = self
                .session()
                .and_then(|s| {
                    let p = s.project();
                    let column = p.launcher.columns.iter().find(|c| c.track == track)?;
                    let clip = column.slots.get(scene)?.clip.as_ref()?;
                    p.content(clip.content).map(|c| c.name.clone())
                })
                .unwrap_or_default();
            ui.ctx().stop_dragging();
            self.carry = Some(Carry { from: CarryFrom::Slot { track, scene }, name });
        }
        let carry = self.carry.clone();
        // Ctrl (Cmd on macOS) held at the drop: a copy, as the keymap's rule has it.
        let copy = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        let Some(carry) = carry else { return };
        let (released, escape, pointer) =
            ui.input(|i| (!i.pointer.primary_down(), i.key_pressed(Key::Escape), i.pointer.latest_pos()));
        let target = match self.shown {
            View::Session => self.view.drop.as_ref().and_then(|p| p.target),
            View::Arrangement => self.arrangement.drop.as_ref().and_then(|p| p.target),
        };
        if escape || released {
            self.carry = None;
            self.switch_hover = None;
            self.view.drop = None;
            self.arrangement.drop = None;
            if released && !escape {
                match target {
                    Some(target) => self.drop_carried(&carry, target, copy),
                    None => self.status = Status::Hint("Drop a clip on a clip slot or a track's lane".into()),
                }
            }
            return;
        }
        let label = target.and_then(|t| self.carry_label(&carry, t, copy));
        if let Some(pointer) = pointer {
            let name = if carry.name.is_empty() { "clip" } else { carry.name.as_str() };
            drag_label(ui.ctx(), pointer, label.as_deref().unwrap_or(name), label.is_some());
        }
    }

    /// What the label beside a carried clip says over `target`, or `None` where it cannot land.
    /// `copy`: Ctrl is held — a slot-to-slot drag says Copy instead of its move or swap.
    fn carry_label(&self, carry: &Carry, target: DropTarget, copy: bool) -> Option<String> {
        let project = self.session()?.project();
        let name = |t: TrackId| project.tracks.iter().find(|x| x.id == t).map_or_else(String::new, |x| x.name.clone());
        let clip = if carry.name.is_empty() { "clip" } else { carry.name.as_str() };
        match target {
            DropTarget::Slot { track, scene } => {
                if carry.from == (CarryFrom::Slot { track, scene }) {
                    return None;
                }
                // A slot-to-slot drag moves the clip, swapping with a target's clip — Ctrl
                // copies instead, as the cross-view drags do. The occupancy decides Move or
                // Swap: an occupied slot's clip may have no name, and the swap is still a swap.
                let (target_occupied, target_clip) = project
                    .launcher
                    .columns
                    .iter()
                    .find(|c| c.track == track)
                    .and_then(|c| c.slots.get(scene))
                    .map(|s| {
                        (
                            s.clip.is_some(),
                            s.clip
                                .as_ref()
                                .and_then(|placed| project.content(placed.content))
                                .map(|c| c.name.clone())
                                .unwrap_or_default(),
                        )
                    })
                    .unwrap_or((false, String::new()));
                match (copy, target_occupied) {
                    (false, false) => Some(format!("Move {clip} to {}, scene {}", name(track), scene + 1)),
                    (false, true) => {
                        let theirs = if target_clip.is_empty() { "clip" } else { target_clip.as_str() };
                        Some(format!("Swap {clip} and {theirs} in {}, scene {}", name(track), scene + 1))
                    }
                    (true, _) => Some(format!("Copy {clip} to {}, scene {}", name(track), scene + 1)),
                }
            }
            DropTarget::Lane { track, beat } => {
                let bar = (beat / self.bar_beats()).floor() as u64 + 1;
                Some(format!("Copy {clip} to {}, bar {bar}", name(track)))
            }
            _ => None,
        }
    }

    /// The carried clip's display name: "clip" when it has none.
    fn carry_name(&self, carry: &Carry) -> String {
        if carry.name.is_empty() { "clip".into() } else { carry.name.clone() }
    }

    /// A track's name, or its id when the project has none by that id.
    fn track_name(&self, track: TrackId) -> String {
        self.session()
            .and_then(|s| s.project().tracks.iter().find(|t| t.id == track).map(|t| t.name.clone()))
            .unwrap_or_else(|| track.0.to_string())
    }

    /// Beats in a bar of the shown project, 4 when it has no usable time signature.
    fn bar_beats(&self) -> f64 {
        self.session().map_or(4.0, |s| {
            let ts = s.project().transport.time_signature;
            if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) }
        })
    }

    /// Puts a copy of the carried clip on `target`: `clip.copy` from where it came, then
    /// `clip.paste` there, the same commands as Ctrl+C and Ctrl+V; the paste is one undo step.
    fn drop_carried(&mut self, carry: &Carry, target: DropTarget, copy: bool) {
        // A slot-to-slot drop moves the clip: one undo step, an occupied target's clip swaps
        // into the source's slot (nothing lost). Ctrl: a copy, as the cross-view drags are.
        if let (CarryFrom::Slot { track, scene }, DropTarget::Slot { track: to_track, scene: to_scene }) =
            (&carry.from, &target)
        {
            if track == to_track && scene == to_scene {
                return;
            }
            if !copy {
                let Ok(session) = self.projects.session_mut(None) else { return };
                match session.execute(
                    "clip.moveSlot",
                    &json!({
                        "track": track.0,
                        "scene": scene + 1,
                        "to_track": to_track.0,
                        "to_scene": to_scene + 1,
                    }),
                ) {
                    Ok(out) => {
                        // The command ran under this call, not under `run`: the revision bump
                        // is explicit, so the views rebuild with the moved clip.
                        self.revision += 1;
                        let verb = if out["swapped"].as_bool().unwrap_or(false) { "Swapped" } else { "Moved" };
                        let label = format!(
                            "{verb} {} to {}, scene {}",
                            self.carry_name(carry),
                            self.track_name(*to_track),
                            to_scene + 1
                        );
                        self.status = Status::Done(label);
                    }
                    Err(e) => self.status = Status::Error(e.0),
                }
                return;
            }
        }
        let copy = match carry.from {
            CarryFrom::Arrangement { track, index } => json!({ "clips": [{ "track": track.0, "index": index }] }),
            CarryFrom::Slot { track, scene } => json!({ "track": track.0, "scene": scene + 1 }),
        };
        let mut paste = match target {
            DropTarget::Slot { track, scene } => {
                if carry.from == (CarryFrom::Slot { track, scene }) {
                    return;
                }
                json!({ "track": track.0, "scene": scene + 1 })
            }
            DropTarget::Lane { track, beat } => json!({ "track": track.0, "start": beat }),
            _ => return,
        };
        let Ok(session) = self.projects.session_mut(None) else { return };
        if let Err(e) = session.execute("clip.copy", &copy) {
            self.status = Status::Error(e.0);
            return;
        }
        // On a lane the copy follows the stacking setting for its kind, as a moved clip does.
        let audio = session.copied_clip_is_audio();
        if paste.get("start").is_some() && audio.is_some_and(|audio| self.stacks(audio)) {
            paste["stack"] = json!(true);
        }
        self.status = Status::Hint(String::new());
        self.run(&Action::new("clip.paste", paste));
        if !matches!(self.status, Status::Error(_)) {
            let label = self.carry_label(carry, target, true).unwrap_or_default();
            self.status = Status::Done(label.replacen("Copy", "Copied", 1));
        }
    }

    fn plan_drop(&self, items: &[BrowserItem], target: DropTarget, separate: bool) -> Option<drop::DropPlan> {
        let project = self.session()?.project();
        let name =
            |t: TrackId| project.tracks.iter().find(|x| x.id == t).map_or_else(|| t.0.to_string(), |x| x.name.clone());
        let stack = self.settings.sample_stacking || self.settings.midi_stacking;
        drop::plan(items, target, separate, self.settings.warp_mode.param(), stack, &name)
    }

    /// Runs a drop: decodes its samples on another thread first, so the window never waits,
    /// then runs its commands (ADR 0011).
    fn drop_items(&mut self, items: &[BrowserItem], target: DropTarget, separate: bool) {
        let Some(plan) = self.plan_drop(items, target, separate) else {
            self.status = Status::Hint("Nothing to add there".into());
            return;
        };
        self.decode_then_run(plan.samples, plan.commands);
    }

    /// Relinks missing files (`project.relinkFiles`), their audio decoded on another thread first.
    fn relink(&mut self, files: Vec<(String, PathBuf)>) {
        let names: Vec<String> = files.iter().map(|(f, _)| f.clone()).collect();
        let to: Vec<String> = files.iter().map(|(_, p)| p.display().to_string()).collect();
        let samples = files.into_iter().map(|(_, p)| p).collect();
        self.decode_then_run(samples, vec![("project.relinkFiles", json!({ "files": names, "to": to }))]);
    }

    /// Decodes `samples` on another thread, so the window never waits, then runs `commands`
    /// with them at hand (ADR 0011).
    fn decode_then_run(&mut self, samples: Vec<PathBuf>, commands: Vec<(&'static str, serde_json::Value)>) {
        if samples.is_empty() {
            self.run_drop(commands);
            return;
        }
        let (send, decoded) = std::sync::mpsc::channel();
        let count = samples.len();
        let started = std::thread::Builder::new().name("suhina-drop".into()).spawn(move || {
            // A Stems file gives every audio track, each under its stream number.
            let done: Vec<_> = samples
                .into_iter()
                .flat_map(|path| {
                    let decoded: Vec<(Option<u16>, suhina_media::AudioData)> =
                        if suhina_media::stems::is_stems_path(&path) {
                            suhina_media::stems::decode_streams(&path)
                                .map(|all| all.into_iter().zip(0u16..).map(|(d, n)| (Some(n), d)).collect())
                                .unwrap_or_default()
                        } else {
                            suhina_media::decode_file(&path).map(|d| vec![(None, d)]).unwrap_or_default()
                        };
                    decoded.into_iter().map(move |(stream, data)| {
                        let data = std::sync::Arc::new(data);
                        let peaks = crate::kit::waveform::peaks(&data);
                        (path.clone(), stream, data, peaks)
                    })
                })
                .collect();
            let _ = send.send(done);
        });
        match started {
            Ok(_) => {
                self.status = Status::Hint(format!("Reading {count} sample{}", if count == 1 { "" } else { "s" }));
                self.pending_drop = Some(PendingDrop { commands, decoded });
            }
            Err(_) => self.run_drop(commands),
        }
    }

    /// Picks up the samples of a drop once they are decoded and runs its commands.
    fn poll_drop(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_drop else { return };
        match pending.decoded.try_recv() {
            Ok(decoded) => {
                let Some(pending) = self.pending_drop.take() else { return };
                let mut waveforms = (*self.waveforms).clone();
                if let Ok(session) = self.projects.session_mut(None) {
                    for (path, stream, data, peaks) in decoded {
                        let key = std::path::absolute(&path).unwrap_or(path.clone()).display().to_string();
                        let key = match stream {
                            Some(n) => {
                                session.provide_stream(&path, n, data);
                                suhina_engine::model::stream_key(&key, n)
                            }
                            None => {
                                session.provide_audio(&path, data);
                                key
                            }
                        };
                        waveforms.insert(key, std::sync::Arc::new(peaks));
                    }
                }
                self.waveforms = std::sync::Arc::new(waveforms);
                self.run_drop(pending.commands);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(30))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_drop = None;
                self.status = Status::Error("reading the samples stopped".into());
            }
        }
    }

    /// Runs a drop's commands through the open projects, so what was added becomes recent.
    fn run_drop(&mut self, commands: Vec<(&'static str, serde_json::Value)>) {
        for (id, params) in commands {
            match self.projects.execute(id, &params) {
                Ok(result) => {
                    self.revision += 1;
                    self.timeline = None;
                    // A Stems file arrives as one group: it opens folded, so the drop reads as a
                    // single lane until the group's triangle is clicked (`clip.addFiles`).
                    self.fold_new_groups(&result);
                    let skipped = result["skipped"].as_array().map_or(0, Vec::len);
                    let placed = result["clips"].as_array().map_or(0, Vec::len);
                    self.status = match (id, skipped) {
                        ("clip.addFiles", 0) => {
                            Status::Done(format!("Added {placed} clip{}", if placed == 1 { "" } else { "s" }))
                        }
                        ("clip.addFiles", n) => Status::Hint(format!(
                            "Added {placed} clips; {n} skipped: {}",
                            result["skipped"][0]["reason"].as_str().unwrap_or_default()
                        )),
                        _ => Status::Done("Added".into()),
                    };
                }
                Err(e) => {
                    self.status = Status::Error(e.0);
                    break;
                }
            }
        }
    }

    /// How far each column's playing clip is through its loop — or through itself, without one —
    /// from the engine's monitor: the beats since the clip's launch, folded through its offset
    /// and loop, as the piano roll's playhead maps them.
    fn progress(&self, grid: &Grid) -> Vec<Option<f32>> {
        let Some(monitor) = self.session().and_then(|s| s.player()).map(|p| p.monitor()) else {
            return Vec::new();
        };
        let project = self.session().map(|s| s.project());
        grid.columns.iter().map(|column| Self::clip_progress(project, monitor, column)).collect()
    }

    /// How far the clip in `column`'s playing slot is through its loop — or through itself,
    /// without one — from the engine's beats since its launch, folded through its offset and loop.
    fn clip_progress(
        project: Option<&suhina_engine::model::Project>,
        monitor: &suhina_engine::LauncherMonitor,
        column: &crate::interaction::grid::Column,
    ) -> Option<f32> {
        // The launcher column's index in the monitor, and the slot playing there.
        let index = column.launcher?;
        let scene = monitor.column(index)?.playing?;
        let clip = project?.launcher.columns.get(index)?.slots.get(scene)?.clip.as_ref()?;
        // A slot's clip plays from its launch: the elapsed beats through its offset and loop.
        let placement = piano_roll::Placement {
            start: 0.0,
            length: f64::INFINITY,
            offset: clip.offset,
            loop_range: clip.loop_range.map(|r| (r.start, r.end)),
        };
        let beat = placement.content_beat(monitor.elapsed_beats(index)?)?;
        // How far through the loop — or, without one, the clip — that is.
        let (from, span) = match clip.loop_range {
            Some(range) if range.end > range.start => (range.start, range.end - range.start),
            _ => (clip.offset, clip.length),
        };
        (span > 0.0).then(|| ((beat - from) / span).clamp(0.0, 1.0) as f32)
    }

    fn control_bar(
        &mut self,
        ui: &mut Ui,
        grid: Option<&Grid>,
        cpu: Option<u32>,
        ram: Option<u64>,
        actions: &mut Vec<Action>,
    ) {
        let bar = ui.max_rect();
        ui.allocate_rect(bar, Sense::hover());
        let painter = ui.painter_at(bar);
        let Some(grid) = grid else {
            widgets::text_in(
                &painter,
                bar.shrink2(Vec2::new(PAD, 0.0)),
                Align2::LEFT_CENTER,
                "Suhina",
                theme::ui_font(theme::TEXT),
                theme::text_secondary(),
            );
            return;
        };
        let row = |x: f32, width: f32| {
            Rect::from_min_size(Pos2::new(x, bar.center().y - theme::ROW / 2.0), Vec2::new(width, theme::ROW))
        };

        // Left: tempo, time signature, launch quantisation.
        let mut x = bar.left() + PAD;
        let tempo_rect = row(x, 56.0);
        let tempo_shown = self.tempo_drag.unwrap_or(grid.tempo_bpm);
        let response = widgets::value_area(ui, tempo_rect, Id::new("tempo"), "Tempo", format!("{tempo_shown:.2}"));
        match widgets::drag_value(ui, &response, &mut self.tempo_drag, grid.tempo_bpm, 0.1, 1.0, (20.0, 999.0), 120.0) {
            Drag::Done(bpm) => {
                actions.push(Action::new("transport.setTempo", json!({ "bpm": (bpm * 100.0).round() / 100.0 })))
            }
            Drag::Moving(_) | Drag::None => {}
        }
        painter.rect_filled(
            tempo_rect,
            0.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        let mono = theme::mono_font(theme::TEXT);
        widgets::text_in(
            &painter,
            tempo_rect.shrink2(Vec2::new(PAD, 0.0)),
            Align2::RIGHT_CENTER,
            &format!("{tempo_shown:.2}"),
            mono.clone(),
            theme::text_primary(),
        );
        x = tempo_rect.right() + PAD;
        // The time signature playing, like the tempo: a launched scene may have changed it.
        let ts = grid.time_signature();
        let signature =
            if self.session().is_some() { format!("{} / {}", ts.numerator, ts.denominator) } else { String::new() };
        let ts_rect = row(x, 40.0);
        widgets::text_in(&painter, ts_rect, Align2::CENTER_CENTER, &signature, mono.clone(), theme::text_secondary());
        x = ts_rect.right() + PAD;
        let small = theme::ui_font(theme::TEXT_SMALL);
        // At narrow widths the captions go, then the count-in and the metronome (the main strip
        // has one too), so nothing runs under the transport in the middle.
        let transport_left = bar.center().x - (28.0 * 3.0 + 2.0 + PAD + 64.0) / 2.0 - PAD;
        let captions = x + 26.0 + 64.0 + PAD + 50.0 + 64.0 + PAD + 28.0 <= transport_left;
        let grid_caption = row(x, if captions { 26.0 } else { 0.0 });
        if captions {
            widgets::text_in(&painter, grid_caption, Align2::LEFT_CENTER, "Grid", small.clone(), theme::text_hint());
        }
        let quant_rect = row(grid_caption.right(), 64.0);
        // One value for both: when launched clips start, and the Arrangement view's grid.
        let current = self.session().map_or(DEFAULT_QUANTIZATION, |s| quantization_label(s.quantization()));
        let mut chosen = current;
        ui.scope_builder(egui::UiBuilder::new().max_rect(quant_rect), |ui| {
            let response = egui::ComboBox::from_id_salt("quantization")
                .width(quant_rect.width())
                // Tall enough for every value at once, without a scroll bar.
                .height(QUANTIZATION.len() as f32 * 2.0 * theme::ROW)
                .selected_text(egui::RichText::new(chosen).font(mono.clone()).color(theme::text_value()))
                .show_ui(ui, |ui| {
                    for q in QUANTIZATION {
                        ui.selectable_value(&mut chosen, q, egui::RichText::new(q).font(mono.clone()));
                    }
                })
                .response;
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, QUANTIZATION_LABEL));
            response.on_hover_text(
                "Grid: when launched clips start, and the Arrangement view's grid lines and snapping (Alt: off the \
                 grid)",
            );
        });
        if chosen != current {
            actions.push(Action::new("launcher.setQuantization", json!({ "value": chosen })));
        }
        let count_caption = row(quant_rect.right() + PAD, if captions { 50.0 } else { 0.0 });
        if captions {
            widgets::text_in(&painter, count_caption, Align2::LEFT_CENTER, "Count-in", small, theme::text_hint());
        }
        let count_in = row(count_caption.right(), 64.0);
        let metronome = row(count_in.right() + PAD, 28.0);
        if metronome.right() <= transport_left {
            let bars = self.session().map_or(0, |s| s.count_in_bars());
            crate::session::record_controls::count_in_menu(ui, count_in, bars, actions);
            crate::session::record_controls::metronome_button(ui, &painter, metronome, grid.metronome, actions);
        }

        // Centre: play, stop, record, position.
        let transport_width = 28.0 * 3.0 + 2.0 + PAD + 64.0;
        let mut x = bar.center().x - transport_width / 2.0;
        let play = row(x, 28.0);
        let response = widgets::button_area(ui, play, Id::new("play"), "Play");
        if response.clicked() {
            actions.push(self.play_action(false));
        }
        let running = motion::toward(ui.ctx(), response.id.with("running"), grid.running, motion::VALUE);
        let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(play, 0.0, motion::mix(hover, theme::control_active(), running));
        let glyph = motion::mix(theme::text_primary(), theme::accent(), running);
        let c = play.center();
        painter.add(egui::Shape::convex_polygon(
            vec![c + Vec2::new(-3.5, -5.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(-3.5, 5.0)],
            glyph,
            egui::Stroke::NONE,
        ));
        x = play.right() + 1.0;
        let stop = row(x, 28.0);
        let response = widgets::button_area(ui, stop, Id::new("stop"), "Stop");
        if response.clicked() {
            actions.push(self.stop_action());
        }
        painter.rect_filled(
            stop,
            0.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        painter.rect_filled(Rect::from_center_size(stop.center(), Vec2::splat(8.0)), 0.0, theme::text_primary());
        let record = row(stop.right() + 1.0, 28.0);
        let recording = self.session().is_some_and(|s| s.records_arrangement());
        crate::session::record_controls::record_button(ui, &painter, record, recording, actions);
        x = record.right() + PAD;
        let position = row(x, 64.0);
        painter.rect_filled(position, 0.0, theme::window_background());
        widgets::text_in(
            &painter,
            position.shrink2(Vec2::new(PAD, 0.0)),
            Align2::LEFT_CENTER,
            &grid.position(),
            mono,
            theme::text_primary(),
        );

        // Right, from the edge in: the view switch, the current view underlined, so it is
        // the last control at the right end. Left of it the CPU load, from the engine's monitor
        // (per mille, smoothed): how much of the time the audio engine spends rendering. In a
        // narrow window a block that would reach the position readout is left out, the RAM
        // first, so nothing overlaps and the order stays.
        let centre_end = position.right() + PAD;
        let fits = |right: f32, width: f32| right - width >= centre_end;
        let mut right = bar.right() - PAD;
        let mut underline_at = None;
        // A clip dragged in the Arrangement view, or carried, switches the view when it hovers
        // over the other view's name for a moment.
        let carrying = self.carry.is_some() || self.arrangement.dragged_clip().is_some();
        let pointer = ui.ctx().pointer_latest_pos();
        let mut hovered_switch = None;
        for (view, name) in [(View::Arrangement, "Arrangement"), (View::Session, "Session")] {
            let color = if self.shown == view { theme::text_primary() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, name, theme::ui_font(theme::TEXT), color, 120.0);
            let size = galley.size();
            let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
            let response = widgets::button_area(ui, rect, Id::new(("view", name)), format!("{name} view"));
            if response.clicked() {
                self.shown = view;
            }
            if carrying && self.shown != view && pointer.is_some_and(|p| rect.expand(4.0).contains(p)) {
                hovered_switch = Some(view);
                painter.rect_stroke(rect.expand(3.0), 0.0, theme::hairline(theme::accent()), egui::StrokeKind::Inside);
            }
            let color = motion::mix(color, theme::text_primary(), widgets::hover(ui, &response));
            let pos = Pos2::new(rect.left(), rect.center().y - size.y / 2.0);
            painter.galley(pos, galley, color);
            if self.shown == view {
                underline_at = Some((pos.x, pos.y + size.y + 1.0, size.x));
            }
            right = rect.left() - 12.0;
        }
        right += 12.0 - PAD;
        if let Some(milli) = cpu.filter(|_| fits(right, 56.0)) {
            let rect = row(right - 56.0, 56.0);
            let percent = f64::from(milli.min(1000)) / 10.0;
            painter.rect_filled(rect, 0.0, theme::window_background());
            let inner = rect.shrink2(Vec2::new(PAD - 2.0, 0.0));
            widgets::text_in(
                &painter,
                inner,
                Align2::LEFT_CENTER,
                "CPU",
                theme::ui_font(theme::TEXT_TINY),
                theme::text_hint(),
            );
            widgets::text_in(
                &painter,
                inner,
                Align2::RIGHT_CENTER,
                &format!("{percent:.0}%"),
                theme::mono_font(theme::TEXT_SMALL),
                theme::text_secondary(),
            );
            let track = Rect::from_min_max(
                Pos2::new(rect.left() + 1.0, rect.bottom() - 3.0),
                Pos2::new(rect.right() - 1.0, rect.bottom() - 1.0),
            );
            painter.rect_filled(track, 0.0, theme::control_background());
            let fill = track.width() * milli.min(1000) as f32 / 1000.0;
            let full = Rect::from_min_max(track.min, Pos2::new(track.left() + fill, track.bottom()));
            let color = if milli >= 800 { theme::meter_hot() } else { theme::meter_normal() };
            painter.rect_filled(full, 0.0, color);
            let response = ui.interact(rect, Id::new("cpu-load"), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("CPU load {percent:.1}%"))
            });
            right = rect.left() - PAD;
        }
        // Left of that, the program's own RAM (its resident memory), in megabytes or gigabytes.
        if let Some(bytes) = ram.filter(|_| fits(right, 76.0)) {
            let rect = row(right - 76.0, 76.0);
            painter.rect_filled(rect, 0.0, theme::window_background());
            let inner = rect.shrink2(Vec2::new(PAD - 2.0, 0.0));
            widgets::text_in(
                &painter,
                inner,
                Align2::LEFT_CENTER,
                "RAM",
                theme::ui_font(theme::TEXT_TINY),
                theme::text_hint(),
            );
            widgets::text_in(
                &painter,
                inner,
                Align2::RIGHT_CENTER,
                &ram_text(bytes),
                theme::mono_font(theme::TEXT_SMALL),
                theme::text_secondary(),
            );
            let response = ui.interact(rect, Id::new("ram-usage"), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("RAM usage {}", ram_text(bytes)))
            });
            right = rect.left() - PAD;
        }
        // Left of that, the mapping mode's button, lit while the mode is on.
        if !self.projects.is_empty() {
            let on = self.view.launch_keys.mode.on;
            let color = if on { theme::accent() } else { theme::text_secondary() };
            let galley = widgets::fitted(&painter, "Map keys", theme::ui_font(theme::TEXT), color, 80.0);
            let size = galley.size();
            if fits(right, size.x) {
                let rect = Rect::from_min_max(Pos2::new(right - size.x, bar.top()), Pos2::new(right, bar.bottom()));
                let response = widgets::button_area(ui, rect, Id::new("map-keys"), "Map keys to slots and scenes");
                if response.clicked() {
                    self.toggle_mapping_mode();
                }
                let color = motion::mix(color, theme::text_primary(), widgets::hover(ui, &response));
                painter.galley(Pos2::new(rect.left(), rect.center().y - size.y / 2.0), galley, color);
            }
        }
        self.hover_switch(ui.ctx(), hovered_switch);
        // The underline slides to the view that was picked.
        if let Some((x, y, width)) = underline_at {
            let ctx = ui.ctx();
            let x = motion::value(ctx, Id::new("view-underline-x"), x, motion::SLIDE);
            let width = motion::value(ctx, Id::new("view-underline-width"), width, motion::SLIDE);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 1.0)),
                0.0,
                theme::text_primary(),
            );
        }
        painter.hline(bar.x_range(), bar.bottom() - 0.5, theme::hairline(theme::border()));
    }

    fn status_bar(&self, ui: &mut Ui, grid: Option<&Grid>) {
        let bar = ui.max_rect();
        let painter = ui.painter_at(bar);
        let (text, color) = match &self.status {
            Status::Hint(t) => (t.as_str(), theme::text_secondary()),
            Status::Done(t) => (t.as_str(), theme::text_secondary()),
            Status::Error(t) => (t.as_str(), theme::recording()),
        };
        let backed_up = self
            .session()
            .and_then(Session::last_backup)
            .map(|b| format!(" · backed up {}", suhina_session::backup::clock_time(b.written)));
        let right = grid.map(|g| {
            let tracks = g.columns.iter().filter(|c| c.kind != crate::interaction::grid::ColumnKind::Return).count();
            format!("{tracks} tracks · {} scenes{}", g.scenes.len(), backed_up.as_deref().unwrap_or(""))
        });
        let right_width = match (&right, &backed_up) {
            (None, _) => 0.0,
            (Some(_), None) => 140.0,
            (Some(_), Some(_)) => 240.0,
        };
        let left =
            Rect::from_min_max(bar.min + Vec2::new(PAD, 0.0), Pos2::new(bar.right() - right_width - PAD, bar.bottom()));
        widgets::text_in(&painter, left, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
        if let Some(right) = right {
            let rect =
                Rect::from_min_max(Pos2::new(bar.right() - right_width, bar.top()), bar.max - Vec2::new(PAD, 0.0));
            widgets::text_in(
                &painter,
                rect,
                Align2::RIGHT_CENTER,
                &right,
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_hint(),
            );
        }
    }
}

/// Darkens the view while a project file is dragged over the window, so it is clear the window
/// takes the drop.
/// Keys are the shortcuts' (Tab switches the view, the arrows move the selection): egui's own
/// keyboard focus, which Tab and the arrows move from widget to widget, is kept to text fields.
/// A button that held it would also make every shortcut wait, as if the user were typing.
/// The sample map's own window when nothing says where it was, in points.
const MAP_WINDOW_SIZE: [f32; 2] = [900.0, 640.0];
/// How long the map's window must stay put before its place is saved, in seconds.
const MAP_WINDOW_SETTLE: f64 = 0.5;

/// The sample map's own window, as the main window keeps track of it.
#[derive(Debug, Default)]
struct MapWindow {
    /// Where it opened (`left, top, width, height`), set when it opens; `Some(None)` for the
    /// default place.
    opened_at: Option<Option<[i32; 4]>>,
    /// A place it moved to and when, saved when it stays there.
    moved: Option<([i32; 4], f64)>,
    /// A drag from the browser that started in this window.
    drag: Option<RemoteDrag>,
}

/// A drag that started in the map's own window, read from that window's input.
#[derive(Debug, Clone, Copy)]
struct RemoteDrag {
    /// The pointer on the desktop, in points.
    screen: Option<Pos2>,
    released: bool,
    escape: bool,
    ctrl: bool,
}

/// What a frame of the map's own window reports back.
struct MapWindowFrame {
    changed: bool,
    /// Keymap actions whose keys were pressed in that window.
    keys: Vec<String>,
    embedded: bool,
    close: bool,
    place: Option<[i32; 4]>,
    screen: Option<Pos2>,
    released: bool,
    escape: bool,
    ctrl: bool,
    time: f64,
}

fn keys_for_shortcuts(ctx: &egui::Context) {
    let focused = ctx.memory(|m| m.focused());
    let typing = focused.is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
    if !typing {
        ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::None);
            if let Some(id) = focused {
                m.surrender_focus(id);
            }
        });
    }
}

fn drop_overlay(ui: &Ui, area: Rect) {
    let is = |f: &egui::HoveredFile, ext: &str| {
        f.path.as_ref().is_none_or(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
    };
    let (project, themes) = ui.input(|i| {
        let files = &i.raw.hovered_files;
        (
            files.iter().any(|f| is(f, "als") || f.path.as_deref().is_some_and(suhina_session::is_project_file)),
            files.iter().any(|f| is(f, "json") || is(f, "zip")),
        )
    });
    let shown = motion::toward(ui.ctx(), Id::new("drop-overlay"), project || themes, motion::HOVER);
    if shown <= 0.0 {
        return;
    }
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background().gamma_multiply(DROP_DIM * shown));
    let text = theme::text_primary().gamma_multiply(shown);
    // A file whose name is not known yet counts as both; a project is the likelier.
    let what = if themes && !project { "Drop to add the themes" } else { "Drop to open" };
    painter.text(area.center(), Align2::CENTER_CENTER, what, theme::ui_font(theme::TEXT + 2.0), text);
}

/// The label beside the pointer while something is dragged: outlined in the accent colour when
/// it can land where the pointer is.
fn drag_label(ctx: &egui::Context, pointer: Pos2, text: &str, lands: bool) {
    let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("browser-drag"));
    let painter = ctx.layer_painter(layer);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let galley = widgets::fitted(&painter, text, font, theme::text_primary(), 280.0);
    let rect = Rect::from_min_size(pointer + Vec2::new(14.0, 10.0), galley.size() + Vec2::new(PAD * 2.0, 6.0));
    painter.rect_filled(rect, 0.0, theme::control_active());
    painter.rect_stroke(
        rect,
        0.0,
        theme::hairline(if lands { theme::accent() } else { theme::border_strong() }),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + Vec2::new(PAD, 3.0), galley, theme::text_primary());
    ctx.request_repaint();
}

/// What the status bar says after a command ran, or nothing for silent ones.
/// The control bar's value box, as screen readers and tests name it.
const QUANTIZATION_LABEL: &str = "Grid and launch quantisation";

/// The control bar's text for `quantization`: the choice that parses to it.
fn quantization_label(quantization: suhina_engine::Quantization) -> &'static str {
    QUANTIZATION
        .into_iter()
        .find(|q| suhina_session::parse_quantization(q).ok() == Some(quantization))
        .unwrap_or(DEFAULT_QUANTIZATION)
}

/// A mapping command's target in words: "track 2 scene 1", "scene 3", "the stop button of track 2".
fn mapping_target(p: &serde_json::Value) -> String {
    match p["target"].as_str() {
        Some("slot") => format!("track {} scene {}", p["track"], p["scene"]),
        Some("scene") => format!("scene {}", p["scene"]),
        _ => format!("the stop button of track {}", p["track"]),
    }
}

fn describe(action: &Action) -> Option<String> {
    let p = &action.params;
    Some(match action.id {
        "clip.launch" => format!("Launched scene {} on track {}", p["scene"], p["track"]),
        "scene.launch" => format!("Launched scene {}", p["scene"]),
        "track.stop" => format!("Stopping track {}", p["track"]),
        "launcher.stopAll" => "Stopping all clips".into(),
        "launcher.addMapping" => format!(
            "Mapped {} to {}",
            p["source"].as_str().unwrap_or_default().trim_start_matches("key:"),
            mapping_target(p)
        ),
        "launcher.removeMapping" => format!("Removed the keys of {}", mapping_target(p)),
        "launcher.setQuantization" => format!("Grid {}", p["value"].as_str().unwrap_or_default()),
        "clip.setLaunchMode" => format!("Launch mode {}", p["mode"].as_str().unwrap_or_default()),
        "clip.setQuantization" => format!("Clip launch quantisation {}", p["value"].as_str().unwrap_or_default()),
        "clip.setLegato" => {
            if p["on"] == true {
                "Legato on".into()
            } else {
                "Legato off".into()
            }
        }
        "transport.setTempo" => format!("Tempo {}", p["bpm"]),
        "clip.copy" => "Copied the clip".into(),
        "clip.paste" if p.get("scene").is_some() => format!("Pasted into scene {} on track {}", p["scene"], p["track"]),
        "clip.paste" => "Pasted the clip".into(),
        "edit.undo" => "Undone".into(),
        "edit.redo" => "Redone".into(),
        _ => return None,
    })
}

/// The RAM as the control bar shows it: megabytes, or gigabytes with a decimal from 1 GiB on.
fn ram_text(bytes: u64) -> String {
    let mib = bytes as f64 / (1024.0 * 1024.0);
    if mib >= 1024.0 { format!("{:.1} GB", mib / 1024.0) } else { format!("{mib:.0} MB") }
}

/// Where an arrangement clip's name is drawn, by (track, index): the inline rename's place.
pub(crate) const NAME_RECT_CLIP: &str = "clip-name-rect";
/// Where a clip slot's name is drawn, by (track, scene from 1).
pub(crate) const NAME_RECT_SLOT: &str = "slot-name-rect";

/// One menu of the menu bar. While another of the bar's menus is open, the pointer over this
/// one's title opens it, as system menu bars do.
fn bar_menu<R>(ui: &mut Ui, title: &str, content: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<Option<R>> {
    let shown = ui.menu_button(title, content);
    let id = egui::Popup::default_response_id(&shown.response);
    if shown.response.hovered() && !egui::Popup::is_id_open(ui.ctx(), id) && egui::Popup::is_any_open(ui.ctx()) {
        egui::Popup::open_id(ui.ctx(), id);
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame with the main window's content at `main` on the desktop; returns where a drag
    /// is in the main window.
    fn drag_pointer_in(app: &App, main: Rect, pointer: Option<Pos2>) -> Option<Pos2> {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        if let Some(info) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            info.inner_rect = Some(main);
        }
        if let Some(p) = pointer {
            input.events.push(egui::Event::PointerMoved(p));
        }
        let mut found = None;
        let mut output = ctx.run_ui(input, |ui| found = app.drag_pointer(ui.ctx()));
        output.textures_delta.clear();
        found
    }

    #[test]
    fn files_from_the_system_outline_the_target_they_hover_and_change_nothing_until_dropped() {
        use egui_kittest::kittest::Queryable;
        let mut app = App::new();
        app.set_session(suhina_session::Session::new(crate::demo::demo_project()), &[]);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(Vec2::new(1280.0, 800.0))
            .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        harness.run();
        let before = harness.state().session().map(|s| s.project().clone());
        let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
        harness.hover_at(at);
        harness.run();
        let file = egui::HoveredFile { path: Some(PathBuf::from("/x/Groove.mid")), ..Default::default() };
        harness.input_mut().hovered_files.push(file);
        harness.step();
        let target = harness.state().view.drop.as_ref().and_then(|p| p.target);
        assert!(matches!(target, Some(DropTarget::Slot { scene: 0, .. })), "{target:?}");
        // A project or a theme hovering is not a drag into the project.
        let file = egui::HoveredFile { path: Some(PathBuf::from("/x/Song.suhina")), ..Default::default() };
        harness.input_mut().hovered_files = vec![file];
        harness.step();
        assert_eq!(harness.state().view.drop, None);
        // Taken away again (the windowing layer clears them): nothing outlined, nothing added.
        harness.input_mut().hovered_files.clear();
        harness.step();
        assert_eq!(harness.state().view.drop, None);
        assert_eq!(harness.state().session().map(|s| s.project().clone()), before);
    }

    #[test]
    fn a_drag_from_the_map_window_lands_where_the_pointer_is_on_the_main_window() {
        let mut app = App::new();
        let main = Rect::from_min_size(Pos2::new(1920.0, 100.0), Vec2::new(1280.0, 800.0));
        // A drag in the main window itself: its own pointer.
        assert_eq!(drag_pointer_in(&app, main, Some(Pos2::new(40.0, 50.0))), Some(Pos2::new(40.0, 50.0)));
        // From the map's window: the desktop position, moved onto the main window.
        app.map_window.drag =
            Some(RemoteDrag { screen: Some(Pos2::new(2020.0, 400.0)), released: false, escape: false, ctrl: false });
        assert_eq!(drag_pointer_in(&app, main, None), Some(Pos2::new(100.0, 300.0)));
        // Where the map's window cannot tell (no pointer over it yet), nothing is under the drag.
        app.map_window.drag = Some(RemoteDrag { screen: None, released: false, escape: false, ctrl: false });
        assert_eq!(drag_pointer_in(&app, main, None), None);
    }

    #[test]
    fn ram_is_shown_in_megabytes_or_gigabytes() {
        assert_eq!(ram_text(0), "0 MB");
        assert_eq!(ram_text(512 * 1024 * 1024), "512 MB");
        assert_eq!(ram_text(700_000_000), "668 MB");
        // From a gibibyte on, gigabytes with a decimal.
        assert_eq!(ram_text(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(ram_text(2 * 1024 * 1024 * 1024 + 555_745_280), "2.5 GB");
        // The measurement itself answers on this machine.
        assert!(memory_stats::memory_stats().is_some_and(|s| s.physical_mem > 0));
    }
}
