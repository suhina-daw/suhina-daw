// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: tabs for the open projects (ADR 0007), the control bar, the Session or
//! Arrangement view and the status bar around the active project.
//! Every change goes through `Session::execute` (AGENTS.md rule 6); the window only reads the
//! project and the engine's monitor to draw them.

mod actions;
mod application_bar;
mod audio_settings;
mod background;
mod browser_panel;
pub mod closing_dialog;
mod control_bar;
mod detail_area;
mod drops;
pub use drops::is_theme_package;
pub mod export_dialog;
mod exports;
mod keys;
pub mod loading_dialog;
mod menu_bar;
pub mod menus;
pub mod missing_files;
mod plugin_host;
pub mod project_dialog;
pub mod splash;
pub mod start_panel;
mod tabs;
mod themed_toolbar;
pub mod updates;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::kit::surface::Backgrounds;
use egui::{Align2, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{ClipId, TrackId};
use suhina_session::{CommandKind, ExportGather, PluginStatus, ProjectId, Projects, Session};

use crate::app::export_dialog::{ExportChoice, ExportDialog, ExportJob};
use crate::app::project_dialog::{
    AboutChoice, AgentChoice, AraEditsChoice, Choice, DialogState, Lists, RecoveryChoice, SaveChoice, SavePrompt,
    UnsavedChoice,
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
use crate::interaction::grid::{SlotState, TrackGrid};
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

use self::actions::*;
use self::browser_panel::*;
use self::control_bar::*;
use self::drops::*;
use self::keys::*;

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
/// The projects a closing window is letting go of: their names and threads.
type LettingGo = Vec<(String, std::thread::JoinHandle<()>)>;

/// How long a closing window waits for its projects' plug-ins to be let go of before it closes
/// anyway ([`closing_dialog`]).
const QUIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
/// The detail area's height in a row, by default, and the splitter between it and the view.
const DETAIL_HEIGHT: f32 = 200.0;
/// How long the browser takes to slide in or out, with animations on (#708).
const BROWSER_SLIDE: f32 = 0.18;
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
    project: Option<ProjectId>,
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
        clip: ClipId,
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
/// What auto-arm (Settings › MIDI, #834) last acted on.
#[derive(Default)]
struct AutoArm {
    /// The project and its selected track when auto-arm last looked; `None` while it is off, so
    /// turning it on arms the track selected then.
    seen: Option<(ProjectId, Option<TrackId>)>,
    /// The track auto-arm armed in each project, disarmed when another is selected there. A
    /// track armed by hand is never one of them.
    armed: std::collections::BTreeMap<ProjectId, TrackId>,
}

pub struct App {
    background: background::Background,
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
    /// Clips a plug-in edits about to go to another track, waiting for Keep edits or Copy as
    /// new (ADR 0028 step 6): the move, and the plug-in's name.
    ara_prompt: Option<(Action, String)>,
    /// What to do once the project is saved, when Save in that question first asks for a name.
    save_then: Option<Leaving>,
    /// Every project with unsaved changes was answered for: the window may close.
    quit_confirmed: bool,
    /// Close the window on the next frame.
    quit_now: bool,
    /// Projects whose state checks are still on control workers before the save prompt.
    quit_checks: Option<Vec<ProjectId>>,
    /// The window is closing: the projects being let go of and since when; it closes once they
    /// are, or after [`QUIT_WAIT`].
    quitting: Option<(std::time::Instant, LettingGo)>,
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
    /// The arrangement clip the plug-ins that edit clips (ARA) on its track were last told is
    /// selected, and the one whose ARA editor was last opened on its own when it was selected:
    /// once per clip, so an editor the user closes stays closed.
    ara_told: Option<(TrackId, Option<ClipId>)>,
    /// The clip whose ARA editor the user closed while it was shown: not opened again on its
    /// own until another clip is shown or Alt+M asks (#719).
    ara_opened: Option<(TrackId, ClipId)>,
    /// The clip whose ARA editor was open last frame, and when the editor was last asked to
    /// open for which clip (seconds of the window's clock): a refused or lost open is asked
    /// again a second later, as a plug-in that is busy or reloading misses it (#719).
    ara_was_open: Option<(TrackId, ClipId)>,
    ara_open_asked: Option<((TrackId, ClipId), f64)>,
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
    /// Items picked from the system's menu bar, run with the next frame (`run_menu_item`).
    queued_menu_items: Vec<String>,
    /// A device removal awaiting its undo or redo: the track, the index it left, how long the
    /// chain was, and where the selection pointed before it followed the removal. Window state.
    pending_device_remove: Option<(TrackId, usize, usize, usize)>,
    /// The detail area is showing, and its height (in a row) or width (stacked).
    detail_open: bool,
    detail_height: f32,
    /// How tall the device chain's panels were last frame, when the detail area showed only the
    /// chain: the area is kept no taller.
    detail_cap: Option<f32>,
    /// Where each frame's time goes, written to the trace log when tracing is on.
    frame_trace: crate::kit::frame_trace::FrameTrace,
    /// What auto-arm (Settings › MIDI) last saw and armed.
    auto_arm: AutoArm,
    /// The embedded plug-in editors the detail area has grown to fit, once each as they open:
    /// after that its height is the user's, and the editor is fitted to it (#689).
    fitted_editors: std::collections::HashSet<(TrackId, Vec<usize>)>,
    /// An editor is embedded in the chain shown: the area may be taller than the panels.
    editor_in_chain: bool,
    /// A clip being renamed (Rename in its menu, Ctrl+R): the command's target, the name typed
    /// so far, and whether the field has had the keyboard yet.
    rename: Option<(serde_json::Value, String, bool)>,
    /// The project switcher Ctrl+Tab opens: the open project picked so far (its place among
    /// them). It switches when Ctrl is let go, as most programs' do.
    tab_switcher: Option<usize>,
    /// The Arrangement's clip selection last frame: when it changes while stopped, the play
    /// start marker goes to its first clip.
    clips_before: Vec<(TrackId, ClipId)>,
    /// The piano roll is pulled to the top (`docs/spec/ui-clip-editor.md` › Pulled to the top),
    /// and the detail area's height a splitter drag has reached so far, unclamped.
    detail_max: bool,
    detail_drag: Option<f32>,
    /// The app's own window (`set_editor_owner`): a press on its controls takes the keyboard
    /// back from an embedded plug-in editor (`kit::keyboard`).
    window: Option<u64>,
    /// The height of the system's title bar drawn over the top of the window (macOS, when the
    /// window is see-through), kept free and given a background; 0 when it is outside the window.
    title_bar: f32,
    /// The radius of the window's rounded corners, which a see-through window does not clip to:
    /// the bars at the top and bottom round theirs to match. 0 when the system clips them.
    window_corner: u8,
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
    /// The export dialog (File › Export…) while it shows.
    export_dialog: Option<ExportDialog>,
    /// The export running on its worker (ADR 0011's previews, the scan's thread): the result
    /// lands here, and the dialog stays up showing Exporting… until it does.
    export: Option<std::sync::mpsc::Receiver<Result<serde_json::Value, String>>>,
    /// The project the running export belongs to, its name as it stood at the launch: the
    /// feedback names it whatever tab shows after, and once it is closed, too.
    export_project: Option<(ProjectId, String)>,
    /// An export whose plug-in states are being gathered over frames ([`App::gather_export`]):
    /// the project's id, the job, the deadline the one-second read wait keeps, and the
    /// project's name at the launch, for the feedback that outlives the tab.
    export_gather: Option<(suhina_session::ProjectId, ExportJob, std::time::Instant, String)>,
    /// Help › About Suhina is open.
    about_open: bool,
    /// A theme package whose theme is here already, by its file and the theme's name, waiting
    /// for the answer: replace it or bring in a copy.
    theme_import: Option<(PathBuf, String)>,
    /// A folder whose files move with it (the settings, the themes), chosen and waiting for the
    /// answer to "move them there?": the new folder, or `None` for the default.
    folder_move: Option<(PickField, Option<PathBuf>)>,
    /// The themes moved from one folder to another, for the app to move their files.
    theme_move: Option<(PathBuf, PathBuf)>,
    /// The update check, Help › Check for updates… and the start panel's update line.
    updates: crate::app::updates::Updates,
    /// Restart now was pressed: the window starts the updated build as it closes.
    restart: bool,
    new_request: Option<NewProject>,
    /// A project file saved since the app last asked, for its recent list.
    saved_path: Option<PathBuf>,
    /// Where projects are saved and templates kept when the settings leave them empty.
    default_folders: (PathBuf, PathBuf),
    /// The themes folder used while Settings › Themes › Themes folder is empty.
    themes_default_folder: PathBuf,
    /// Suhina's default data folder, shown while Settings folder is empty.
    data_default_folder: PathBuf,
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
    /// The device whose expanded panel is open over the main area this frame, as the detail
    /// area found it (ADR 0021 › Amendment: device panels, 6).
    expanded_panel: Option<(TrackId, detail_view::DeviceBox)>,
    sample_drops: Vec<crate::detail::device_canvas::SampleTarget>,
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
    /// The audio's health counters last traced: device underruns, late stretched frames and late plug-in blocks.
    audio_traced: (u64, u64, u64),
    /// A clip carried between the views, and the view whose name it hovers over since when.
    carry: Option<Carry>,
    /// A device carried from a chain or a strip's list onto another track (#688): its track and
    /// index.
    device_carry: Option<(TrackId, usize)>,
    /// How much of the browser shows, 0 to 1, while it slides in or out (#708).
    browser_part: f32,
    /// The hosts' reader threads wake this window ([`suhina_plugins::set_waker`]).
    waker_set: bool,
    switch_hover: Option<(View, f64)>,
    /// Agents (ADR 0028): connections waiting for the person's answer, the answers for the app,
    /// the agents let in, and those the person disconnected, for the app.
    agent_asks: Vec<(u64, String)>,
    agent_answers: Vec<(u64, bool)>,
    agents: Vec<(u64, String)>,
    agent_disconnects: Vec<u64>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        // The window opens the person's own projects: their audio is read wherever it is on a
        // local disk, as projects name it (docs/spec/missing-files.md › Project file boundaries).
        let mut projects = Projects::new();
        projects.read_local_audio(true);
        Self {
            projects,
            background: background::Background::default(),
            shown_project: None,
            stashes: HashMap::new(),
            open_prompt: None,
            unsaved_prompt: None,
            ara_prompt: None,
            save_then: None,
            quit_confirmed: false,
            quit_now: false,
            quit_checks: None,
            quitting: None,
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
            ara_told: None,
            ara_opened: None,
            ara_was_open: None,
            ara_open_asked: None,
            selected_device: None,
            device_focus: false,
            device_picks: Vec::new(),
            queued: Vec::new(),
            queued_menu_items: Vec::new(),
            pending_device_remove: None,
            detail_open: true,
            detail_height: DETAIL_HEIGHT,
            detail_cap: None,
            frame_trace: crate::kit::frame_trace::FrameTrace::default(),
            auto_arm: AutoArm::default(),
            fitted_editors: std::collections::HashSet::new(),
            editor_in_chain: false,
            rename: None,
            tab_switcher: None,
            clips_before: Vec::new(),
            detail_max: false,
            detail_drag: None,
            window: None,
            title_bar: 0.0,
            window_corner: 0,
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
            export_dialog: None,
            export: None,
            export_project: None,
            export_gather: None,
            about_open: false,
            theme_import: None,
            folder_move: None,
            theme_move: None,
            updates: crate::app::updates::Updates::default(),
            restart: false,
            new_request: None,
            saved_path: None,
            default_folders: (PathBuf::from("Projects"), PathBuf::from("Templates")),
            themes_default_folder: PathBuf::from("themes"),
            data_default_folder: PathBuf::new(),
            nam_default_folders: (PathBuf::from("NAM models"), PathBuf::from("IRs")),
            nam_cache: NamCache::default(),
            window_title: String::new(),
            browser: BrowserState::default(),
            sample_map: crate::browser::sample_map_panel::Panel::default(),
            map_window: MapWindow::default(),
            pending_drop: None,
            detail_drop: None,
            expanded_panel: None,
            sample_drops: Vec::new(),
            piano_roll: PianoRollState::default(),
            recovery: Vec::new(),
            recovery_open: None,
            reveal: None,
            ram: None,
            next_ram: None,
            audio_traced: (0, 0, 0),
            carry: None,
            device_carry: None,
            browser_part: 1.0,
            waker_set: false,
            switch_hover: None,
            agent_asks: Vec::new(),
            agent_answers: Vec::new(),
            agents: Vec::new(),
            agent_disconnects: Vec::new(),
        }
    }

    /// Asks the person whether to let `client`, an agent that has connected as `connection`,
    /// work on the open projects (ADR 0028); the answer comes back from
    /// [`App::take_agent_answers`].
    pub fn ask_agent(&mut self, connection: u64, client: String) {
        self.agent_asks.push((connection, client));
    }

    /// A connection ended: its question, if still open, goes away.
    pub fn agent_gone(&mut self, connection: u64) {
        self.agent_asks.retain(|(c, _)| *c != connection);
    }

    /// The person's answers to [`App::ask_agent`] since the last call: allowed or not.
    pub fn take_agent_answers(&mut self) -> Vec<(u64, bool)> {
        std::mem::take(&mut self.agent_answers)
    }

    /// The agents the person has let in, which the control bar shows.
    pub fn set_agents(&mut self, agents: Vec<(u64, String)>) {
        self.agents = agents;
    }

    /// The agents the person disconnected from the control bar since the last call.
    pub fn take_agent_disconnects(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.agent_disconnects)
    }

    /// The projects changed outside the window's own commands (an agent's, ADR 0028): what the
    /// views keep from them is read again.
    pub fn note_outside_change(&mut self) {
        self.revision += 1;
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

    /// A window whose `plugin.*` commands use `library` instead of the one in Suhina's
    /// settings folder (tests, portable setups).
    pub fn with_library(library: suhina_plugins::library::Library) -> Self {
        let mut projects = Projects::with_library(library);
        projects.read_local_audio(true);
        Self { projects, ..Self::new() }
    }

    /// Shows `session`, replacing the active project. `warnings` are what the importer skipped.
    pub fn set_session(&mut self, session: Session, warnings: &[String]) {
        self.open_project(session, warnings, OpenAs::Replace);
    }

    /// Opens `session` in a new tab or in place of the active project, and makes it active.
    /// Returns its id, or `None` when no more projects can be open (the status bar says why).
    pub fn open_project(&mut self, session: Session, warnings: &[String], how: OpenAs) -> Option<ProjectId> {
        let mut replaced = None;
        if how == OpenAs::Replace
            && let Some(active) = self.projects.active()
            && self.projects.close(active).is_ok()
        {
            replaced = Some(active);
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
                self.status = Status::Error(e.message().to_owned());
                return None;
            }
        };
        // Opening is no command (ADR 0007), so it has a line of its own beside the `action`
        // lines; one opened in place of another says which it closed (#821). The name is quoted
        // and escaped: a file name may hold a line break, which would start a line of its own.
        match replaced {
            Some(old) => suhina_plugins::trace!("project opened {} {name:?} in place of {}", id.0, old.0),
            None => suhina_plugins::trace!("project opened {} {name:?} in a new tab", id.0),
        }
        self.restore(id, Stash::default());
        self.plugin_dialog_hidden = false;
        Some(id)
    }

    /// The window plug-in editors open over: the native handle of the app's main window (a
    /// Win32 `HWND`, an X11 window id), so they float above it (ADR 0006).
    pub fn set_editor_owner(&mut self, owner: Option<u64>) {
        self.window = owner;
        self.projects.set_editor_owner(owner);
    }

    /// The height of the system's title bar over the top of the window (0 when it is not in it)
    /// and the radius of the window's corners, when the window does not clip to them.
    pub fn set_window_frame(&mut self, title_bar: f32, corner: u8) {
        self.title_bar = title_bar.max(0.0);
        self.window_corner = corner;
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
        // A theme package opened with Suhina (a double-click on it) is brought in, not opened.
        if is_theme_package(&path) {
            self.status = Status::Hint(format!("Bringing in the theme {}", path.display()));
            self.themes.import(path);
            return;
        }
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
        if let Some((started, going)) = &self.quitting {
            // The dialog stays until the window is gone: the frames before that would show the
            // window without its projects (the start panel).
            if going.iter().all(|(_, g)| g.is_finished()) || started.elapsed() >= QUIT_WAIT {
                if !self.quit_confirmed {
                    let gone = going.iter().all(|(_, g)| g.is_finished());
                    suhina_plugins::trace!(
                        "quit: projects {} {:?}",
                        if gone { "let go in" } else { "not let go, closing anyway after" },
                        started.elapsed()
                    );
                }
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            return;
        }
        if self.quit_now {
            self.quit_now = false;
            self.start_quitting(ctx);
            return;
        }
        if self.quit_confirmed {
            return;
        }
        let requested = ctx.input(|i| i.viewport().close_requested());
        if !requested && self.quit_checks.is_none() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if !self.unsaved_prompt_open() {
            let mut pending = self.quit_checks.take().unwrap_or_else(|| self.projects.ids());
            pending.retain(|id| self.projects.get_mut(*id).is_some_and(|s| !s.poll_plugin_edits()));
            if !pending.is_empty() {
                self.quit_checks = Some(pending);
                ctx.request_repaint_after(std::time::Duration::from_millis(10));
                return;
            }
        }
        let Some(first) = self.unsaved_projects().first().copied() else {
            self.start_quitting(ctx);
            return;
        };
        if !self.unsaved_prompt_open() {
            self.unsaved_prompt = Some(Leaving { project: first, then: AfterLeaving::Quit(Vec::new()) });
        }
    }

    /// Every project was answered for: their backups are written, and they are let go of while
    /// the window says so ([`closing_dialog`]); it closes once they are, or after [`QUIT_WAIT`].
    fn start_quitting(&mut self, ctx: &egui::Context) {
        let _finished_in_time = self.finish_backups();
        // Embedded plug-in editors are windows of their own and would cover the dialog.
        self.place_editors(&[]);
        self.quitting = Some((std::time::Instant::now(), self.projects.let_go_all()));
        ctx.request_repaint();
    }

    /// Whether the window is closing ([`App::start_quitting`]).
    pub fn quitting(&self) -> bool {
        self.quitting.is_some()
    }

    /// Saves the project being left, then leaves it; asks for a name first when it has no file
    /// of its own yet.
    fn save_and_leave(&mut self, leaving: Leaving) {
        if self.projects.active() != Some(leaving.project) {
            self.run(&Action::new("project.switch", json!({ "project": leaving.project.0 })));
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
            AfterLeaving::Close => self.run(&Action::new("project.close", json!({ "project": leaving.project.0 }))),
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

    /// The clip selected in the Arrangement view: its track and id.
    pub fn selected_clip(&self) -> Option<(TrackId, ClipId)> {
        self.arrangement.selected_clip()
    }

    /// The slot selected in the Session view: its track and scene (from 0).
    pub fn selected_slot(&self) -> Option<(TrackId, usize)> {
        self.view.selected_slot
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
        // A clip an ARA plug-in edits plays from where its editor and a click set the position,
        // never from the clip's start (the plug-in's timeline is where the user points).
        // A click in the clip's body set where play starts; only a clip picked whole plays from
        // its start.
        let clip = (selected || self.settings.play_from == PlayFrom::Clip)
            .then(|| self.selected_arrangement_clip())
            .flatten()
            .filter(|_| self.arrangement.picked_whole)
            .filter(|(track, _)| !self.ara_track(*track));
        match clip {
            Some((_, clip)) => Action::new("transport.play", json!({ "from": "clip", "clip": clip.0 })),
            None => Action::new("transport.play", json!({})),
        }
    }

    /// Whether `track` has a plug-in that edits its clips (ARA, ADR 0028): the play-from-clip
    /// rules give way there, always.
    fn ara_track(&self, track: TrackId) -> bool {
        self.session().is_some_and(|s| s.ara_device(track).is_some())
    }

    /// The clip selected in the Arrangement view, while that view is shown.
    fn selected_arrangement_clip(&self) -> Option<(TrackId, ClipId)> {
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

    /// Quitting, after [`App::finish_backups`]: lets go of the projects still open (the window
    /// let go of them already when it was closed from itself), and waits up to [`QUIT_WAIT`].
    pub fn let_go_of_projects(&mut self) -> bool {
        self.projects.close_all(QUIT_WAIT)
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

    /// With tracing on, once a second: the audio's health when it went wrong since the last
    /// look (`docs/debugging.md` › Audio dropouts): device buffers not filled in time, stretched
    /// clips' frames not ready (they play as silence), and the engine's load when it is high.
    fn trace_audio(&mut self) {
        if !suhina_plugins::trace::on() {
            return;
        }
        let Some(player) = self.session().and_then(Session::player) else { return };
        let info = player.output_info();
        let device = info.as_ref().map_or(0, |i| i.underruns);
        let stretched = player.stretch_underruns();
        let moves = player.stretch_moves();
        let (misses, worst, worst_misses) = player.plugin_misses();
        let worst_name = self
            .session()
            .and_then(|s| suhina_session::plugin_device_name(s.project(), worst))
            .unwrap_or_else(|| "none".into());
        let cpu = player.cpu_monitor().load_milli();
        let running = player.monitor().running();
        let (was_device, was_stretched, was_misses) = self.audio_traced;
        if device > was_device || stretched > was_stretched || misses > was_misses || cpu >= 700 {
            suhina_plugins::trace!(
                "audio: device underruns {device} (+{}), stretched frames late {stretched} (+{}), edits that restarted stretched clips {moves}, plug-in blocks late {misses} (+{}, most {worst_name}: {worst_misses}), engine load {:.1} %, {}, buffer {} frames, block {}, latency {:.1} ms{}",
                device.saturating_sub(was_device),
                stretched.saturating_sub(was_stretched),
                misses.saturating_sub(was_misses),
                f64::from(cpu) / 10.0,
                if running { "playing" } else { "stopped" },
                info.as_ref().map_or(0, |i| i.buffer_frames),
                info.as_ref().map_or(0, |i| i.engine_block),
                info.as_ref().map_or(0.0, |i| i.latency_ms),
                if info.as_ref().is_some_and(|i| i.failed) { ", the device failed" } else { "" },
            );
        }
        self.audio_traced = (device, stretched, misses);
    }

    /// Uses `settings` from now on, as loaded from the settings file.
    pub fn set_settings(&mut self, ctx: &egui::Context, settings: Settings) {
        self.projects.set_backup_settings(settings.backup, settings.backup_clean_up);
        motion::set_enabled(ctx, settings.animations);
        self.arrangement.headers_left = headers_left(settings.track_headers);
        self.arrangement.returns_hidden = settings.view.returns_hidden;
        self.arrangement.waveform_zoom = settings.view.waveform_zoom as f32 / 100.0;
        self.arrangement.waveform_appearance = settings.view.waveform;
        self.arrangement.audio_colour_strength = settings.view.audio_colour_strength;
        self.arrangement.header_colour_strength = settings.view.header_colour_strength;
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
        // A variant in use whose file went: its theme's own values take its place.
        if self.themes.get(&self.settings.theme).is_none()
            && let Some((base, _)) = self.settings.theme.split_once('@')
            && self.themes.get(base).is_some()
        {
            self.settings.theme = base.to_owned();
            self.settings_changed = true;
        }
        theme::set_palette(ctx, self.themes.palette(&self.settings.theme));
    }

    /// Draws with the theme `id` from now on and saves that in the settings, if there is such a
    /// theme.
    /// Asks whether the theme package at `path`, whose theme `name` is here already, replaces it
    /// or comes in as a copy.
    /// The themes folder that changed after the person agreed to move them: from and to.
    pub fn take_theme_move(&mut self) -> Option<(PathBuf, PathBuf)> {
        self.theme_move.take()
    }

    pub fn ask_theme_import(&mut self, path: PathBuf, name: String) {
        self.theme_import = Some((path, name));
    }

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
                let channel = track.channel.and_then(|ch| project.channel(ch));
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
                let channel = track.channel.and_then(|ch| project.channel(ch));
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
    /// The themes folder used while Settings › Themes › Themes folder is empty.
    pub fn set_default_themes_folder(&mut self, folder: PathBuf) {
        self.themes_default_folder = folder;
    }

    /// Suhina's default data folder, shown while Settings › Projects and backups › Settings folder is empty.
    pub fn set_default_data_folder(&mut self, folder: PathBuf) {
        self.data_default_folder = folder;
    }

    /// Settings › Projects and backups › Settings folder back to `folder` after a move to another one failed
    /// or was refused, so the field shows where the data is and no later change of settings
    /// tries the move again.
    pub fn revert_data_folder(&mut self, folder: String) {
        self.settings.data_folder = folder;
    }

    /// Where themes are kept: Settings › Themes › Themes folder, or the default.
    pub fn themes_folder(&self) -> PathBuf {
        match self.settings.themes_folder.trim() {
            "" => self.themes_default_folder.clone(),
            folder => PathBuf::from(folder),
        }
    }

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

    /// Opens what Settings › Projects and backups › Startup project says, for Suhina started without a project:
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
                None => Some(
                    "No template is chosen to start from (Settings › Projects and backups › Startup template)".into(),
                ),
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

    /// Checks for updates as Settings › Agents and updates › Updates says; the desktop app calls it once
    /// the window is up, tests never do (`docs/spec/updates.md`).
    pub fn check_updates_at_start(&mut self, ctx: &egui::Context) {
        self.updates.at_start(ctx, &self.settings.updates);
    }

    /// Restart now was pressed and the window is closing: the updated build is to be started.
    pub fn restart_wanted(&self) -> bool {
        self.restart
    }

    fn updates_wanted(&mut self, ctx: &egui::Context, wanted: crate::app::updates::Wanted) {
        match wanted {
            crate::app::updates::Wanted::Restart => {
                self.restart = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            crate::app::updates::Wanted::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
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
        let notice = self.updates.notice();
        match start_panel::show(ui, area, &mut self.start, &self.project_lists, &audio, version, notice.as_ref(), now) {
            Some(StartChoice::Update) => {
                if let Some(wanted) = self.updates.notice_pressed(ui.ctx()) {
                    self.updates_wanted(ui.ctx(), wanted);
                }
            }
            Some(StartChoice::UpdatePage(url)) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
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

    /// The template a new project is made from when Suhina starts with one: its path, a name in the templates folder.
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
            PickField::ThemesFolder => Some(self.themes_folder()),
            PickField::DataFolder => Some(match self.settings.data_folder.trim() {
                "" => self.data_default_folder.clone(),
                folder => PathBuf::from(folder),
            }),
            PickField::IrFolder => Some(self.ir_folder()),
            PickField::TemplatesFolder | PickField::StartTemplate => Some(self.templates_folder()),
            PickField::SaveFolder => self.save_prompt.as_ref().and_then(|p| folder_of(&p.folder)),
            // The export dialog's file, opening in the folder its text names.
            PickField::ExportFile => self
                .export_dialog
                .as_ref()
                .map(|d| d.path.trim().to_string())
                .filter(|text| !text.is_empty())
                .and_then(|text| Path::new(&text).parent().map(Path::to_path_buf)),
            PickField::BrowserFolder | PickField::McpWriteFolder | PickField::BackgroundImage => None,
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
            PickField::BackgroundImage => {
                self.settings.background_image = text;
                self.background.retry_failed();
            }
            PickField::McpWriteFolder => {
                if !self.settings.mcp_write_folders.contains(&text) {
                    self.settings.mcp_write_folders.push(text);
                }
            }
            PickField::NamModelsFolder => self.settings.nam_models_folder = text,
            // A folder whose files move with it asks first.
            PickField::ThemesFolder | PickField::DataFolder => self.folder_move = Some((field, Some(path))),
            PickField::IrFolder => self.settings.irs_folder = text,
            PickField::TemplatesFolder => self.settings.templates_folder = text,
            PickField::StartTemplate => self.settings.start_template = text,
            PickField::SaveFolder => {
                if let Some(prompt) = self.save_prompt.as_mut() {
                    prompt.folder = text;
                }
                return;
            }
            PickField::ExportFile => {
                if let Some(dialog) = self.export_dialog.as_mut() {
                    dialog.accept_pick(&path);
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
                    Err(e) => self.status = Status::Error(e.message().to_owned()),
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

    /// Saves the active project: to its own `.suhina` file, or, the first time and when `ask`,
    /// to a name and folder the user gives.
    fn save_active(&mut self, ask: bool) {
        self.save_active_collecting(ask, false);
    }

    /// File › Collect and save: the audio the project plays from outside its folder is copied
    /// into it, then the project is saved (asking for a name when it has no file yet).
    fn collect_and_save(&mut self) {
        self.save_active_collecting(false, true);
    }

    /// Copies the project's audio from outside `path`'s folder into it (`project.collect`).
    /// Returns whether that went well.
    fn collect_into(&mut self, path: &Path) -> bool {
        let folder = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
        match self.projects.execute("project.collect", &json!({ "folder": folder.display().to_string() })) {
            Ok(_) => true,
            Err(e) => {
                self.status = Status::Error(e.message().to_owned());
                false
            }
        }
    }

    fn save_active_collecting(&mut self, ask: bool, collect: bool) {
        let Some(id) = self.projects.active() else { return };
        match self.own_file(id) {
            Some(path) if !ask => {
                if !collect || self.collect_into(&path) {
                    self.save_to(&path, false);
                }
            }
            own => {
                let folder =
                    own.as_deref().and_then(Path::parent).map_or_else(|| self.projects_folder(), Path::to_path_buf);
                let name = self.projects.name(id).unwrap_or("Untitled").to_string();
                self.save_prompt = Some(SavePrompt {
                    template: false,
                    name,
                    folder: folder.display().to_string(),
                    focus: true,
                    collect,
                });
            }
        }
    }

    /// The project's own `.suhina` file, once it has been saved or was opened from one.
    fn own_file(&self, id: ProjectId) -> Option<PathBuf> {
        self.projects.get(id)?.source().filter(|s| suhina_session::is_project_file(s)).map(Path::to_path_buf)
    }

    /// Asks for a name to save the active project as a template under.
    fn save_template(&mut self) {
        let Some(id) = self.projects.active() else { return };
        let name = self.projects.name(id).unwrap_or("Template").to_string();
        let folder = self.templates_folder().display().to_string();
        self.save_prompt = Some(SavePrompt { template: true, name, folder, focus: true, collect: false });
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
                self.status = Status::Error(e.message().to_owned());
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
                    let collected = !prompt.collect || self.collect_into(&path);
                    let saved = collected && self.save_to(&path, prompt.template);
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
        if let Some(mut state) = self.export_dialog.take() {
            match export_dialog::show_export(ui, window, &mut state) {
                // The dialog stays up while the export runs on its worker (Exporting…), and a
                // failure keeps it with its settings.
                Some(ExportChoice::Export(job)) => {
                    self.export_dialog = Some(state);
                    self.start_export(job);
                }
                Some(ExportChoice::Browse) => {
                    self.export_dialog = Some(state);
                    self.ask_pick(PickField::ExportFile);
                }
                // The dialog closed: over a gathering export the close cancels the gather;
                // over a rendering one the worker goes on, its result still taken.
                Some(ExportChoice::Cancel) => self.cancel_export_gather(),
                None => self.export_dialog = Some(state),
            }
        }
        // A recovery offer, and the agent prompt after it, wait for the dialogs that ask about
        // a project, the question about a clip's plug-in edits among them.
        let other_dialog = self.unsaved_prompt.is_some()
            || self.save_prompt.is_some()
            || self.project_dialog.is_some()
            || self.ara_prompt.is_some();
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
            // A track by its id, an arrangement clip by its id, a slot's clip by its track and scene.
            let renamed_track = target.get("rename_track").and_then(serde_json::Value::as_u64);
            let key = match (renamed_track, target.get("clip").and_then(serde_json::Value::as_u64)) {
                (Some(track), _) => egui::Id::new((NAME_RECT_TRACK, track)),
                (None, Some(clip)) => egui::Id::new((NAME_RECT_CLIP, clip)),
                (None, None) => {
                    let scene = target.get("scene").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    egui::Id::new((NAME_RECT_SLOT, track, scene))
                }
            };
            let what = if renamed_track.is_some() { "Track name" } else { "Clip name" };
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
                            // An opaque back, so the name the view drew under the field does not
                            // show through it (its font and place differ from the field's).
                            let back = ui.painter().add(egui::Shape::Noop);
                            let field = crate::kit::widgets::text_input(
                                ui,
                                egui::TextEdit::singleline(&mut draft)
                                    .id(egui::Id::new("inline-rename-field"))
                                    .font(theme::ui_font(theme::TEXT_SMALL))
                                    .desired_width(rect.width().max(120.0)),
                            );
                            field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, what));
                            let covered = field.rect.union(rect);
                            ui.painter().set(
                                back,
                                egui::Shape::rect_filled(covered, 2.0, theme::control_background().to_opaque()),
                            );
                            if !focused {
                                field.request_focus();
                            }
                            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            if escape {
                                keep = false;
                            } else if field.lost_focus() && focused {
                                let name = draft.trim();
                                apply = match (renamed_track, target.get("clips")) {
                                    // A track keeps a name: an empty one leaves it as it was.
                                    (Some(_), _) if name.is_empty() => None,
                                    (Some(track), _) => {
                                        Some(Action::new("track.rename", json!({ "track": track, "name": name })))
                                    }
                                    (None, Some(clips)) => Some(Action::new(
                                        "clip.renameMany",
                                        json!({ "clips": clips.clone(), "name": name }),
                                    )),
                                    (None, None) => {
                                        let mut params = target.clone();
                                        params["name"] = json!(name);
                                        Some(Action::new("clip.rename", params))
                                    }
                                };
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
        if self.updates.open
            && let Some(wanted) = self.updates.show(ui, window, &self.settings.updates)
        {
            self.updates_wanted(ui.ctx(), wanted);
        }
        if let Some((field, to)) = self.folder_move.clone() {
            let what =
                if field == PickField::DataFolder { "settings, keys, recent projects and themes" } else { "themes" };
            let target = to.clone().unwrap_or_else(|| {
                if field == PickField::DataFolder {
                    self.data_default_folder.clone()
                } else {
                    self.themes_default_folder.clone()
                }
            });
            match project_dialog::show_folder_move(ui, window, what, &target.display().to_string()) {
                Some(true) => {
                    let text = to.map_or_else(String::new, |p| p.display().to_string());
                    if field == PickField::ThemesFolder {
                        let from = self.themes_folder();
                        self.settings.themes_folder = text;
                        self.theme_move = Some((from, self.themes_folder()));
                    } else {
                        self.settings.data_folder = text;
                    }
                    self.settings_changed = true;
                    self.folder_move = None;
                }
                Some(false) => self.folder_move = None,
                None => {}
            }
        }
        if let Some((path, name)) = self.theme_import.clone() {
            match project_dialog::show_theme_import(ui, window, &name) {
                Some(project_dialog::ImportChoice::Replace) => {
                    self.themes.import_again(path, true);
                    self.theme_import = None;
                }
                Some(project_dialog::ImportChoice::Copy) => {
                    self.themes.import_again(path, false);
                    self.theme_import = None;
                }
                Some(project_dialog::ImportChoice::Cancel) => self.theme_import = None,
                None => {}
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
        // The agent prompt waits for every other dialog, About and Settings among them.
        let other_dialog = other_dialog
            || self.unsaved_prompt.is_some()
            || !self.recovery.is_empty()
            || self.about_open
            || self.theme_import.is_some()
            || self.folder_move.is_some()
            || self.settings_open;
        if !other_dialog && let Some((connection, client)) = self.agent_asks.first().cloned() {
            match project_dialog::show_agent(ui, window, &client) {
                Some(AgentChoice::Allow) => {
                    self.agent_asks.remove(0);
                    self.agent_answers.push((connection, true));
                    self.status = Status::Done(format!("{client} may work on the open projects"));
                }
                Some(AgentChoice::Deny) => {
                    self.agent_asks.remove(0);
                    self.agent_answers.push((connection, false));
                    self.status = Status::Done(format!("{client} was not let in"));
                }
                None => {}
            }
        }
        if let Some((action, plugin)) = self.ara_prompt.take() {
            match project_dialog::show_ara_edits(ui, window, &plugin) {
                Some(AraEditsChoice::Keep) => self.run(&self.with_ara_edits(&action, "keep")),
                Some(AraEditsChoice::New) => self.run(&self.with_ara_edits(&action, "new")),
                Some(AraEditsChoice::Cancel) => {}
                None => self.ara_prompt = Some((action, plugin)),
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
                // Restart now waits for the next Restart now once the quit is called off.
                Some(UnsavedChoice::Cancel) => self.restart = false,
                None => self.unsaved_prompt = Some(leaving),
            }
        }
    }

    /// The name of the plug-in that edits clips which `action` moves or copies to another track
    /// (ADR 0028 step 6), so Keep edits or Copy as new is asked first; `None` otherwise, and for
    /// a move that already says (`ara_edits`).
    fn ara_clips_leaving(&self, action: &Action) -> Option<String> {
        if !matches!(action.id, "clip.move" | "clip.moveMany") || !action.params["araEdits"].is_null() {
            return None;
        }
        let p = &action.params;
        let to = p["toTrack"].as_u64().and_then(|t| u32::try_from(t).ok()).map(TrackId);
        let other_track = to.is_some() || p["tracksBy"].as_i64().is_some_and(|by| by != 0) || p["toNewTrack"] == true;
        if !other_track {
            return None;
        }
        let session = self.session()?;
        let project = session.project();
        let ids: Vec<u32> = match action.id {
            "clip.move" => p["clip"].as_u64().and_then(|c| u32::try_from(c).ok()).into_iter().collect(),
            _ => p["clips"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["clip"].as_u64().and_then(|c| u32::try_from(c).ok()))
                .collect(),
        };
        ids.iter().find_map(|id| {
            let lane = project.arrangement.lanes.iter().find(|l| l.clips.iter().any(|c| c.id.0 == *id))?;
            let track = lane.track?;
            if to == Some(track) {
                return None;
            }
            let device = session.ara_device(track)?;
            let channel = project.track(track)?.channel?;
            let channel = project.channel(channel)?;
            channel.devices.get(device).map(|d| d.name.clone())
        })
    }

    /// `action` (a clip's move or a selection's) as `clip.moveMany` with `ara_edits` set.
    fn with_ara_edits(&self, action: &Action, edits: &str) -> Action {
        let mut params = action.params.clone();
        if action.id == "clip.move" {
            let id = params["clip"].as_u64().unwrap_or_default();
            let start = self
                .session()
                .and_then(|s| {
                    s.project()
                        .arrangement
                        .lanes
                        .iter()
                        .flat_map(|l| &l.clips)
                        .find(|c| u64::from(c.id.0) == id)
                        .map(|c| c.start)
                })
                .unwrap_or_default();
            let beats = params["start"].as_f64().unwrap_or(start) - start;
            if let Some(map) = params.as_object_mut() {
                map.remove("clip");
                map.remove("start");
            }
            params["clips"] = json!([{ "clip": id }]);
            params["beats"] = json!(beats);
        }
        params["araEdits"] = json!(edits);
        Action::new("clip.moveMany", params)
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

    /// Takes `error` off the status bar if it is still what it shows.
    pub fn clear_error(&mut self, error: &str) {
        if matches!(&self.status, Status::Error(shown) if shown == error) {
            self.status = Status::Hint(String::new());
        }
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

    /// A project file the user asked to open, and how, for the app to open.
    pub fn take_open_request(&mut self) -> Option<(PathBuf, OpenAs)> {
        self.open_request.take()
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
        // The open projects' own commands (project.list, project.switch ...) run on all of them.
        if suhina_session::project_commands().iter().any(|c| c.id == action.id) {
            // Traced like every other command, so a trace shows tabs switched and closed (#821).
            suhina_plugins::trace!("action {} {}", action.id, action.params);
            if let Err(e) = self.projects.execute(action.id, &action.params) {
                self.status = Status::Error(e.message().to_owned());
            }
            return;
        }
        // Playing stops a sample previewing in the browser: the two would play over each other.
        if action.id == "transport.play" {
            browser_view::stop_preview(&mut self.browser, &mut self.projects);
        }
        // A critical operation is backed up first, when the settings say so.
        self.projects.back_up_before(action.id);
        let mut no_input = false;
        let Ok(session) = self.projects.session_mut(None) else { return };
        // An audio track armed while it hears no input gets the device's first input (a stereo
        // pair where there are two channels), so Record has something to record and stays on
        // (#682). Without one, recording refuses the track.
        if action.id == "track.setArm"
            && action.params["armed"].as_bool() == Some(true)
            && let Some(track) = action.params["track"].as_u64().and_then(|t| u32::try_from(t).ok()).map(TrackId)
            && session.project().tracks.iter().any(|t| {
                t.id == track && !t.content.notes && t.audio_input.channels == suhina_engine::model::InputChannels::None
            })
        {
            let inputs = session.player().and_then(suhina_session::Player::output_info).map_or(0, |i| i.input_channels);
            let input = match inputs {
                0 => None,
                1 => Some("1"),
                _ => Some("1/2"),
            };
            if let Some(input) = input {
                let _ = session.execute("track.setAudioInput", &json!({ "track": track.0, "input": input }));
            } else {
                // Armed with nothing to hear: say so, rather than a button that seems not to work.
                no_input = true;
            }
        }
        suhina_plugins::trace!("action {} {}", action.id, action.params);
        match session.execute(action.id, &action.params) {
            Ok(_) if no_input => {
                self.revision += 1;
                self.status = Status::Hint(
                    "Armed, but the audio device has no input: choose an input device in Settings › Audio, then pick it under Audio From"
                        .into(),
                );
            }
            Ok(result) => {
                // A clip made by a double-click is selected, ready to edit: in the Arrangement
                // the clip, in the Session its slot, so the detail area shows it.
                if action.id == "clip.create"
                    && let Some(track) = result["track"].as_u64().and_then(|t| u32::try_from(t).ok()).map(TrackId)
                {
                    let at = |key: &str| result[key].as_u64().and_then(|i| usize::try_from(i).ok());
                    let clip = result["clip"].as_u64().and_then(|c| u32::try_from(c).ok()).map(ClipId);
                    match (clip.filter(|_| at("scene").is_none()), at("scene").and_then(|s| s.checked_sub(1))) {
                        (Some(clip), _) => self.arrangement.select_clip(track, clip),
                        (None, Some(scene)) => self.view.select_slot(track, scene),
                        (None, None) => self.view.selected = Some(track),
                    }
                }
                // Duplicated devices: the copies are picked, so the next Ctrl+D copies them on.
                if action.id == "device.duplicateMany"
                    && let Some(track) = action.params["track"].as_u64().and_then(|t| u32::try_from(t).ok())
                    && let Some(made) = result["devices"].as_array()
                {
                    let copies: Vec<(TrackId, usize)> = made
                        .iter()
                        .filter_map(|d| Some((TrackId(track), usize::try_from(d.as_u64()?).ok()?)))
                        .collect();
                    if let Some(last) = copies.last().copied() {
                        self.selected_device = Some(last);
                        self.device_picks = copies;
                    }
                }
                // Duplicated clips: the copies are selected, so the next Ctrl+D copies them on.
                // So are the copies a Ctrl-drag made, on whichever track they landed (#692).
                if (action.id == "clip.duplicate"
                    || (action.id == "clip.moveMany" && action.params["copy"].as_bool() == Some(true)))
                    && let Some(made) = result["clips"].as_array()
                {
                    let copies: Vec<(TrackId, ClipId)> = made
                        .iter()
                        .filter_map(|c| {
                            let track = u32::try_from(c["track"].as_u64()?).ok()?;
                            Some((TrackId(track), ClipId(u32::try_from(c["clip"].as_u64()?).ok()?)))
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
            // Recording and consolidating audio write files next to the project: an unsaved one
            // is saved first, so the next try works (#682, #704).
            Err(e) if e.message().to_owned().starts_with("save the project first") => {
                self.status = Status::Hint(format!(
                    "Save the project first: {}",
                    e.message().to_owned().trim_start_matches("save the project first: ")
                ));
                self.save_active(false);
            }
            Err(e) => self.status = Status::Error(e.message().to_owned()),
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

    /// Draws the window into `ui` and runs what the user asked for.
    pub fn ui(&mut self, ui: &mut Ui) {
        // Following the theme, the headers move when the theme does.
        if self.settings.track_headers.is_none() {
            self.arrangement.headers_left = headers_left(None);
        }
        // The frame's placed controls, for anchors between them (`--suhina-anchor-*`).
        let _capture = crate::kit::control_layout::Capture::begin(ui.ctx());
        self.ui_content(ui);
    }

    fn ui_content(&mut self, ui: &mut Ui) {
        application_bar::resize_edges(ui);
        if let Some(theme) = self.themes.get(&self.settings.theme) {
            crate::kit::skin::configure(&theme.skin);
        } else {
            crate::kit::skin::configure(&crate::kit::skin::Skin::default());
        }
        let _css_frame = crate::kit::native_css::frame_with_motion(
            ui,
            self.settings.animations && self.settings.theme != "high-contrast",
        );
        let _window_style = crate::kit::style_class::enter("window");
        let mut styled_ui = crate::kit::style_class::ui(ui);
        let ui = &mut *styled_ui;
        if let Err(error) = self.background.update(ui, &self.settings, self.themes.palette(&self.settings.theme)) {
            self.status = Status::Error(error);
        }
        let liquid =
            self.themes.get(&self.settings.theme).and_then(|theme| theme.liquid).unwrap_or(self.settings.view.liquid);
        crate::kit::liquid::configure(ui.ctx(), liquid);
        crate::kit::surface::begin_frame(ui.ctx());
        crate::kit::liquid::begin_frame(
            ui.ctx(),
            self.themes.get(&self.settings.theme).map(|theme| theme.gradients.clone()).unwrap_or_default(),
        );
        crate::kit::liquid::set_effects_allowed(
            ui.ctx(),
            self.settings.animations && self.settings.theme != "high-contrast",
        );
        self.frame_trace.start();
        self.frame_trace.causes(ui.ctx());
        // Closing: the dialog alone, so nothing of the window without its projects shows behind.
        if let Some((_, going)) = &self.quitting {
            let waiting: Vec<String> =
                going.iter().filter(|(_, g)| !g.is_finished()).map(|(name, _)| name.clone()).collect();
            closing_dialog::show(ui, ui.max_rect(), &waiting);
            self.guard_quit(ui.ctx());
            self.frame_trace.finish();
            return;
        }
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
        self.arrangement.name_bar_moves = !self.settings.view.clip_halves;
        self.arrangement.grid_in_clips = self.settings.view.grid_in_clips;
        self.arrangement.set_size_settings(self.settings.view.track_sizes);
        if let Some((_, _, timeline)) = &self.timeline {
            self.arrangement.apply_size_defaults(timeline);
        }
        self.arrangement.stretch_alt = self.settings.view.stretch_alt;
        // Any press ends a device panel's hold on Delete and the mute key; a click on a panel
        // takes it again when the button comes up, whether or not the detail area is shown.
        if ui.input(|i| i.pointer.primary_pressed()) {
            self.device_focus = false;
        }
        self.frame_trace.mark("input");
        self.follow_active();
        self.frame_trace.mark("follow project");
        self.poll_scan(ui.ctx());
        self.frame_trace.mark("plug-in scan");
        self.poll_export(ui.ctx());
        self.frame_trace.mark("export");
        self.poll_plugins(ui.ctx());
        self.frame_trace.mark("plug-ins");
        self.tick_backups(ui.ctx());
        self.frame_trace.mark("backups");
        // The sample map's analysis goes on in the background; take in what it finished.
        suhina_plugins::trace::timed("sample map analysis poll", || self.projects.poll_sound_map());
        self.frame_trace.mark("sample map analysis");
        let dropped = self.dropped_files(ui);
        let placing = crate::kit::control_layout::editing(ui.ctx());
        if !placing {
            keys_for_shortcuts(ui.ctx());
            self.launch_keys(ui, &mut actions);
        }
        // A menu bar item chosen while a dialog asks something is passed over, as its key is.
        let queued = std::mem::take(&mut self.queued);
        let menu_items = std::mem::take(&mut self.queued_menu_items);
        if !self.dialog_holds_actions() {
            for id in queued {
                self.do_action(&id, &mut actions);
            }
            for id in menu_items {
                self.run_menu_item(&id, ui.ctx(), &mut actions);
            }
        }
        if !placing {
            self.shortcuts(ui, &mut actions);
        }
        self.apply_zoom(ui.ctx());
        self.frame_trace.mark("keys");
        self.sample_map_window(ui.ctx(), &mut actions);
        self.frame_trace.mark("sample map window");
        let scale = if self.settings.waveform_db {
            crate::kit::waveform::Scale::Db
        } else {
            crate::kit::waveform::Scale::Linear
        };
        crate::kit::waveform::set_scale(ui.ctx(), scale);
        self.frame_trace.mark("input");

        let grid = self.session().map(|s| {
            let mut grid = TrackGrid::build(s.project(), s.player().map(|p| p.monitor()));
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
            self.trace_audio();
        }
        let ram = self.ram;
        self.frame_trace.mark("grid");
        // A press on the window's own controls: keys typed next are the window's, not those of an
        // embedded plug-in editor clicked before.
        if ui.input(|i| i.pointer.any_pressed())
            && let Some(window) = self.window
        {
            suhina_plugins::take_keyboard_back(window);
        }
        // Space pressed in a plug-in's editor window: play or stop, as Space here does.
        for _ in 0..suhina_plugins::take_transport_keys() {
            self.do_action("transport.toggle", &mut actions);
        }
        // What an ARA plug-in asked of the transport: its play and stop buttons, a click in its
        // timeline, in seconds of song time through the tempo map (#717).
        for request in suhina_plugins::take_ara_playback() {
            use suhina_plugins::protocol::AraPlayback;
            match request {
                AraPlayback::Start => actions.push(Action::new("transport.play", json!({}))),
                AraPlayback::Stop => actions.push(Action::new("transport.stop", json!({}))),
                AraPlayback::Position(seconds) => {
                    if let Some(session) = self.session() {
                        let beats = session.project().transport.tempo_map(1.0).beat_at(seconds);
                        suhina_plugins::trace!("app: ara position {seconds:.3}s -> beat {beats:.3}");
                        actions.push(Action::new("transport.setPosition", json!({ "beats": beats })));
                    }
                }
            }
        }
        self.frame_trace.mark("plug-in requests");

        // Under the system's title bar: the window's background, as behind the bars below it.
        // Full screen hides the title bar, and the room with it.
        let full_screen = ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false));
        let corner = if full_screen { 0 } else { self.window_corner };
        if self.title_bar > 0.0 && !full_screen {
            egui::Panel::top("title-bar").exact_size(self.title_bar).frame(egui::Frame::NONE).show(ui, |ui| {
                let top = egui::CornerRadius { nw: corner, ne: corner, sw: 0, se: 0 };
                ui.painter().fill_background(ui.max_rect(), top, theme::panel_background());
            });
        }
        let project_info = self
            .projects
            .active()
            .map(|id| {
                let name = self.projects.name(id).unwrap_or("Untitled");
                let dirty = self.projects.get(id).is_some_and(Session::has_unsaved_changes);
                format!("{name}{}", if dirty { " •" } else { "" })
            })
            .unwrap_or_default();
        let timing = self
            .session()
            .map(|session| {
                let transport = &session.project().transport;
                format!(
                    "{:.1} BPM · {}/{}",
                    transport.tempo_bpm, transport.time_signature.numerator, transport.time_signature.denominator
                )
            })
            .unwrap_or_default();
        // The application bar is the window's title bar where Suhina draws its own (Windows); where
        // the system draws one, it would be a second.
        if cfg!(target_os = "windows") {
            with_bar_line(ui, "application-bar", |ui| {
                egui::Panel::top("application-bar")
                    .exact_size(28.0)
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| application_bar::show(ui, self.settings.view, &project_info, &timing))
            });
        }
        // The window's own menu bar, unless Settings turns it off (by default on macOS, whose bar
        // at the top of the screen has the same menus).
        if self.settings.view.menu_bar {
            with_bar_line(ui, "menu-bar", |ui| {
                egui::Panel::top("menu-bar")
                    .exact_size(theme::MENU_BAR)
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 0)))
                    .show(ui, |ui| {
                        let _style_class = crate::kit::style_class::enter("menu-bar");
                        let bar = ui.max_rect().expand2(Vec2::new(4.0, 0.0));
                        ui.painter().fill_background(bar, 0.0, theme::panel_background());
                        crate::kit::native_css::background(ui.painter(), bar);
                        self.menu_bar(ui, &mut actions)
                    })
            });
        }
        self.frame_trace.mark("menu bar");
        if self.projects.len() >= 2 {
            egui::Panel::top("tabs").exact_size(TABS).frame(egui::Frame::NONE).show(ui, |ui| {
                ui.painter().fill_background(ui.max_rect(), 0.0, theme::window_background());
                self.tab_row(ui, &mut actions)
            });
        }
        with_bar_line(ui, "toolbar", |ui| {
            egui::Panel::top("control-bar").exact_size(theme::CONTROL_BAR).frame(egui::Frame::NONE).show(ui, |ui| {
                let _style_class = crate::kit::style_class::enter("toolbar");
                ui.painter().fill_background(ui.max_rect(), 0.0, theme::panel_background());
                crate::kit::native_css::background(ui.painter(), ui.max_rect());
                self.control_bar(ui, grid.as_ref(), cpu, ram, &mut actions)
            })
        });
        self.frame_trace.mark("control bar");
        egui::Panel::bottom("status-bar").exact_size(theme::STATUS_BAR).frame(egui::Frame::NONE).show(ui, |ui| {
            let bottom = egui::CornerRadius { nw: 0, ne: 0, sw: corner, se: corner };
            ui.painter().fill_background(ui.max_rect(), bottom, theme::panel_background());
            self.status_bar(ui, grid.as_ref())
        });
        self.frame_trace.mark("bars");
        // Samples and MIDI files from the system: dropped this frame, or still over the window.
        let system = match (self.browser.drag.is_some(), dropped.is_empty()) {
            (true, _) => None,
            (false, false) => Some((dropped, true)),
            (false, true) => Some((self.hovered_files(ui), false)).filter(|(items, _)| !items.is_empty()),
        };
        // A private image is already painted underneath the workspace. Keep its
        // transparent regions visible instead of covering them with separator ink.
        let workspace_base = if crate::kit::liquid::background_image(ui.ctx()).is_some() {
            egui::Color32::TRANSPARENT
        } else {
            // `#workspace { border-color }` is this line's own colour.
            let _workspace = crate::kit::style_class::enter_named("workspace", "workspace");
            theme::border()
        };
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(workspace_base)).show(ui, |ui| {
            let _workspace_style = crate::kit::style_class::enter_named("workspace", "workspace");
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
            // left above it (Settings › Devices › Detail area width).
            let full_width = grid.is_some()
                && self.settings.view.detail_layout == crate::settings::view_settings::DetailLayout::FullWidth
                && self.settings.device_layout != DeviceLayout::Stack;
            // The browser stays hidden on the start panel (nothing to drop into); its setting keeps
            // its value for when a project opens.
            let browser_wanted = self.settings.browser_open && !self.start_panel_open();
            // It slides in and out from its side, with animations on (#708).
            self.browser_part = motion::toward(ui.ctx(), Id::new("browser-shown"), browser_wanted, BROWSER_SLIDE);
            let browser_shown = self.browser_part > 0.001;
            let stacked = self.settings.device_layout == DeviceLayout::Stack;
            let css_workspace = crate::kit::native_css::workspace(
                inner.size(),
                browser_shown.then_some(self.settings.browser_width as f32),
                (grid.is_some() && self.detail_open)
                    .then_some((stacked, if stacked { self.detail_width } else { self.detail_height })),
            )
            .map(|layout| layout.translated(inner.min.to_vec2()));
            self.frame_trace.mark("bars");
            let inner = if let Some(layout) = &css_workspace {
                if let Some(panel) = layout.browser {
                    over_view(ui, "panel-browser", panel, layout.main, |ui| {
                        self.browser_panel(ui, inner, Some((panel, layout.main)));
                    });
                }
                inner
            } else if browser_shown && !full_width {
                self.browser_panel(ui, inner, None)
            } else {
                inner
            };
            self.frame_trace.mark("browser");
            let mut view_area = inner;
            self.detail_drop = None;
            self.sample_drops.clear();
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    crate::detail::device_canvas::sample_targets_id(),
                    Vec::<crate::detail::device_canvas::SampleTarget>::new(),
                )
            });
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
            self.expanded_panel = None;
            let inner = if grid.is_some()
                && self.detail_open
                && css_workspace.as_ref().is_none_or(|layout| layout.detail.is_some())
            {
                let geometry =
                    css_workspace.as_ref().and_then(|layout| layout.detail.map(|detail| (layout.main, detail)));
                match geometry {
                    Some((main, detail)) => over_view(ui, "panel-detail", detail, main, |ui| {
                        self.detail_area(ui, inner, &mut actions, geometry)
                    }),
                    None => self.detail_area(ui, inner, &mut actions, geometry),
                }
            } else {
                self.place_editors(&[]);
                // A hidden piano roll does not keep the keyboard, nor the view to one track, and
                // a note its audition sounds goes off.
                self.piano_roll.focused = false;
                actions.extend(self.piano_roll.stop_audition());
                self.arrangement.focus = None;
                // In a row under the view, the splitter stays along the bottom, to drag it open.
                if let Some(layout) = &css_workspace {
                    layout.main
                } else if grid.is_some() && self.settings.device_layout != DeviceLayout::Stack {
                    self.collapsed_splitter(ui, inner)
                } else {
                    inner
                }
            };
            self.frame_trace.mark("detail");
            let inner = if browser_shown && full_width && css_workspace.is_none() {
                let rest = self.browser_panel(ui, inner, None);
                view_area = Rect::from_min_max(
                    Pos2::new(rest.left(), view_area.top()),
                    Pos2::new(rest.right(), view_area.bottom()),
                );
                rest
            } else {
                inner
            };
            if css_workspace.is_some() {
                view_area = inner;
            }
            self.frame_trace.mark("browser");
            let _main_style = crate::kit::style_class::enter_named("workspace-panel", "panel-main");
            // A clip whose drag or held press the view switch carried becomes the carry first,
            // so the probe below reads it on the switch's first frame.
            self.begin_clip_carry(ui.ctx());
            // A drag from the browser, or files from the system: the views say what is under the
            // pointer.
            let dragged = self.browser.drag.as_ref().or(system.as_ref().map(|(items, _)| items));
            let probe =
                dragged.and_then(|items| self.drag_pointer(ui.ctx()).map(|pointer| DropProbe::new(pointer, items)));
            // A carried clip probes the same way, for a slot or a lane only; a carried device for a
            // track, as a dropped one does.
            if let Some(start) = ui.ctx().data_mut(|d| {
                let id = Id::new(drop::DEVICE_CARRY);
                let start = d.get_temp::<(TrackId, usize)>(id);
                d.remove::<(TrackId, usize)>(id);
                start
            }) {
                self.device_carry = Some(start);
            }
            let probe = match (&self.carry, self.device_carry, ui.ctx().pointer_latest_pos()) {
                (Some(_), _, Some(pointer)) => Some(DropProbe::for_clip(pointer)),
                (None, Some(_), Some(pointer)) => Some(DropProbe::for_device(pointer)),
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
            if !meters.shows_same(&self.view.meters) {
                self.meters_moved_at = now;
                ui.ctx().request_repaint();
            } else if meters.moving() && now - self.meters_moved_at < METERS_SETTLE_SECONDS {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
            }
            self.view.meters = meters.clone();
            self.arrangement.meters = meters;
            // The large sample map takes the main area, except while something is dragged, so
            // the drop can land on a track.
            let styled_ui = crate::kit::style_class::ui(ui);
            let mut styled_ui = if css_workspace.is_some() { styled_ui.clipped_to(inner) } else { styled_ui };
            let ui = &mut *styled_ui;
            if self.sample_map.view.large && dragged.is_none() && inner.is_positive() {
                let (panel, browser, projects, settings) =
                    (&mut self.sample_map, &mut self.browser, &mut self.projects, &mut self.settings);
                if panel.show(ui, inner, browser, projects, settings, self.revision) {
                    self.settings_changed = true;
                }
                // A close button at its top right, and Escape, put the views back (#710).
                let close =
                    Rect::from_min_size(Pos2::new(inner.right() - 28.0, inner.top() + 4.0), Vec2::new(24.0, 20.0));
                let over =
                    ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("sample-map-close")));
                let response = widgets::button_area(ui, close, Id::new("sample-map-close"), "Close sample map")
                    .on_hover_text("Close the sample map (Esc)");
                let fill =
                    motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
                over.fill_background(close, 0.0, fill);
                let ink = theme::text_primary();
                for (from, to) in
                    [(Vec2::new(-4.0, -4.0), Vec2::new(4.0, 4.0)), (Vec2::new(-4.0, 4.0), Vec2::new(4.0, -4.0))]
                {
                    over.line_segment([close.center() + from, close.center() + to], egui::Stroke::new(1.3, ink));
                }
                let escape = !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(Key::Escape));
                if response.clicked() || escape {
                    self.close_large_map();
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
                        self.frame_trace.mark("view setup");
                        session_view::show(ui, grid, &progress, &mut self.view, &mut actions);
                        self.frame_trace.mark("session view");
                        self.frame_trace.note(|| format!("session view, {} tracks", grid.columns.len()));
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
                        // The tracks whose plug-in edits clips: their audio clips' menus open
                        // the clip in it.
                        // An audio clip on another track opens in the installed one, which is
                        // added to its track first.
                        self.arrangement.ara_default =
                            self.session().and_then(Session::ara_plugin).map(|(name, _)| name);
                        self.arrangement.ara_tracks = self.session().map_or_else(Vec::new, |s| {
                            s.ara_tracks()
                                .into_iter()
                                .filter_map(|(track, device)| {
                                    let project = s.project();
                                    let channel = project.track(track)?.channel?;
                                    let name = project
                                        .channels
                                        .iter()
                                        .find(|c| c.id == channel)?
                                        .devices
                                        .get(device)?
                                        .name
                                        .clone();
                                    Some((track, name))
                                })
                                .collect()
                        });
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
                            self.frame_trace.mark("view setup");
                            let mut rebuilt = false;
                            let timeline = match self.timeline.take() {
                                Some((revision, folds, mut timeline))
                                    if revision == self.revision && folds == self.arrangement.folds =>
                                {
                                    timeline.follow(monitor);
                                    timeline
                                }
                                before => {
                                    rebuilt = true;
                                    // Stacked clips keep the sub-rows the last build gave them.
                                    let rows = before.map(|(_, _, t)| t.stack_rows).unwrap_or_default();
                                    let mut timeline = Timeline::build_after(session.project(), monitor, &rows);
                                    for lane in &mut timeline.lanes {
                                        lane.armed = session.is_armed(lane.track);
                                    }
                                    // A file a command put in the pool (a consolidated range)
                                    // was never decoded by the opening or a drop: its waveform is
                                    // made here, once, or the clip showed none and looked muted.
                                    let made: Vec<(String, std::sync::Arc<crate::kit::waveform::Peaks>)> = timeline
                                        .lanes
                                        .iter()
                                        .flat_map(|l| &l.clips)
                                        .filter_map(|c| c.audio_view.as_ref())
                                        .filter(|v| !self.waveforms.contains_key(&v.file))
                                        .filter_map(|v| {
                                            let data = session.audio().files.get(&v.file)?;
                                            Some((
                                                v.file.clone(),
                                                std::sync::Arc::new(crate::kit::waveform::peaks(data)),
                                            ))
                                        })
                                        .collect();
                                    if !made.is_empty() {
                                        let mut waveforms = (*self.waveforms).clone();
                                        waveforms.extend(made);
                                        self.waveforms = std::sync::Arc::new(waveforms);
                                    }
                                    timeline.waveforms = self.waveforms.clone();
                                    timeline
                                }
                            };
                            // The grid follows the value in the control bar, which each
                            // project keeps (`launcher.setQuantization`).
                            self.arrangement.grid = session.quantization();
                            let mut timeline = timeline;
                            timeline.follow_automation(session.project());
                            timeline.recording_spans = session.arrangement_recording_spans();
                            timeline.midi_ports = self.projects.midi_ports().to_vec();
                            // What ARA plug-ins found in the audio clips, as it is now (#720).
                            timeline.ara_notes = timeline
                                .lanes
                                .iter()
                                .flat_map(|l| &l.clips)
                                .filter(|c| c.audio)
                                .filter_map(|c| Some((c.id, session.clip_notes(c.id)?.clone())))
                                .collect();
                            timeline.ara_activity = timeline
                                .lanes
                                .iter()
                                .filter(|l| l.clips.iter().any(|c| c.audio))
                                .filter_map(|l| Some((l.track, session.ara_activity(l.track)?)))
                                .collect();
                            if !timeline.ara_activity.is_empty() {
                                // Its progress moves without input.
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                            }
                            timeline.input_channels = session
                                .player()
                                .and_then(suhina_session::Player::output_info)
                                .map_or(0, |i| i.input_channels);
                            self.frame_trace.mark("timeline");
                            arrangement_view::show(ui, &timeline, &mut self.arrangement, &mut actions);
                            self.frame_trace.mark("arrangement view");
                            self.frame_trace.note(|| {
                                let clips: usize = timeline.lanes.iter().map(|l| l.clips.len()).sum();
                                format!(
                                    "arrangement, {} lanes, {clips} clips, {} running, timeline {}",
                                    timeline.lanes.len(),
                                    if timeline.running { "playing" } else { "stopped" },
                                    if rebuilt { "rebuilt" } else { "kept" },
                                )
                            });
                            // Clips picked while stopped, however (a click, a rectangle, Ctrl+A, a
                            // paste): play starts from the first of them, as the setting says (#721).
                            let picked = self.arrangement.selected_clips();
                            // Never for a clip an ARA plug-in edits: its editor and a click on
                            // the clip set the position there.
                            let ara = picked.iter().any(|(track, _)| session.ara_device(*track).is_some());
                            // Nor for a clip picked by a click in its body: play starts where
                            // that click put it.
                            let body = !self.arrangement.picked_whole;
                            if picked != self.clips_before.as_slice()
                                && (!self.settings.view.play_from_clip || ara || body)
                            {
                                self.clips_before = picked.to_vec();
                            } else if picked != self.clips_before.as_slice() {
                                let first = picked
                                    .iter()
                                    .filter_map(|(track, id)| {
                                        timeline
                                            .lanes
                                            .iter()
                                            .find(|l| l.track == *track)?
                                            .clips
                                            .iter()
                                            .find(|c| c.id == *id)
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
                            // Moved there by hand: the user's side from now on, not the theme's.
                            if self.arrangement.headers_left != headers_left(self.settings.track_headers) {
                                let side = if self.arrangement.headers_left { Side::Left } else { Side::Right };
                                self.settings.track_headers = Some(side);
                                self.settings_changed = true;
                            }
                            if self.arrangement.returns_hidden != self.settings.view.returns_hidden {
                                self.settings.view.returns_hidden = self.arrangement.returns_hidden;
                                self.settings_changed = true;
                            }
                            let zoom = (self.arrangement.waveform_zoom * 100.0).round() as u32;
                            if zoom != self.settings.view.waveform_zoom {
                                self.settings.view.waveform_zoom = zoom;
                                self.settings_changed = true;
                            }
                        }
                    }
                    (None, _) => self.start_panel(ui),
                });
            }
            // A device's expanded panel, over the views (and the large sample map).
            if let Some((track, device)) = self.expanded_panel.take() {
                detail_view::show_expanded(ui, inner, track, &device, &mut actions);
            }
            drop_overlay(ui, view_area);
        });
        self.browser_drag(ui);
        self.device_drag(ui);
        if let Some((items, dropped)) = system {
            self.system_drag(ui, items, dropped);
        }
        self.frame_trace.mark("views");
        self.carry_clip(ui);
        suhina_plugins::trace::timed("drop poll", || self.poll_drop(ui.ctx()));
        self.frame_trace.mark("drags");

        crate::kit::liquid::finish_surfaces(ui.ctx());
        // Actions collected above wait for a dialog that asks something, as the keys and the menu
        // bar's queue do: they are dropped while one is open, and once an action opens one.
        let actions_held = self.dialog_holds_actions();
        self.plugin_dialog(ui, window);
        self.project_dialogs(ui, window);
        if self.settings_open {
            // Embedded plug-in editors are windows of their own and would cover the dialog.
            if !self.page.window {
                self.place_editors(&[]);
                crate::kit::liquid::prepare(ui.ctx());
            }
            self.settings_dialog(ui, window);
        }
        self.update_title(ui.ctx());
        self.frame_trace.mark("dialogs");
        let actions: Vec<Action> = actions.into_iter().map(|action| self.remove_every_pick(action)).collect();
        let ran = actions.len();
        for action in &actions {
            if actions_held || self.dialog_holds_actions() {
                break;
            }
            // Closing a project with unsaved changes asks first (Save, Don't save, Cancel).
            let closing = (action.id == "project.close")
                .then(|| action.params["project"].as_u64().and_then(|n| u32::try_from(n).ok()).map(ProjectId))
                .flatten();
            if let Some(project) = closing {
                self.check_plugin_edits(project);
            }
            // The ARA editor's Float and Dock: the window's own, not an undo step.
            if action.id == "view.araFloat" {
                let number = |key: &str| action.params[key].as_u64();
                let track = number("track").and_then(|t| u32::try_from(t).ok()).map(TrackId);
                let device = number("device").and_then(|d| usize::try_from(d).ok());
                let float = action.params["float"].as_bool().unwrap_or(false);
                if let (Some(track), Some(device), Ok(session)) = (track, device, self.projects.session_mut(None))
                    && let Err(e) = session.set_ara_floating(track, device, float)
                {
                    self.status = Status::Error(e.message().to_owned());
                }
                continue;
            }
            // A clip's menu opens it in its track's plug-in that edits clips: the window does it.
            if action.id == "view.araEditor" {
                let id = |key: &str| action.params[key].as_u64().and_then(|n| u32::try_from(n).ok());
                if let (Some(track), Some(clip)) = (id("track"), id("clip")) {
                    self.arrangement.select_clip(TrackId(track), ClipId(clip));
                }
                self.open_ara_editor();
                continue;
            }
            if let Some(project) = closing
                && self.unsaved(project)
            {
                self.unsaved_prompt = Some(Leaving { project, then: AfterLeaving::Close });
                continue;
            }
            // Clips a plug-in edits, going to another track: Keep edits or Copy as new first.
            if let Some(plugin) = self.ara_clips_leaving(action) {
                self.ara_prompt = Some((action.clone(), plugin));
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
        // A selection this frame's commands or clicks made arms its track.
        self.follow_selection_with_arm();
        self.frame_trace.mark("commands");
        self.frame_trace.note(|| format!("{ran} commands"));
        self.guard_quit(ui.ctx());
        // Reading the plug-ins' states before asking whether to save, when the window is closed.
        self.frame_trace.mark("quit check");
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
        self.frame_trace.finish();
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

    /// Where the plug-in device `device` of `track` stands, in the active project.
    fn session_mut_report(&mut self, track: TrackId, device: usize) -> Option<suhina_session::PluginReport> {
        self.projects.session_mut(None).ok()?.plugin_report(track, device)
    }
}

/// The label beside the pointer while something is dragged: outlined in the accent colour when
/// it can land where the pointer is.
fn drag_label(ctx: &egui::Context, pointer: Pos2, text: &str, lands: bool) {
    let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("browser-drag"));
    let painter = ctx.layer_painter(layer);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let galley = widgets::fitted(&painter, text, font, theme::text_primary(), 280.0);
    let rect = Rect::from_min_size(pointer + Vec2::new(14.0, 10.0), galley.size() + Vec2::new(PAD * 2.0, 6.0));
    painter.fill_background(rect, 0.0, theme::control_active());
    painter.rect_stroke(
        rect,
        0.0,
        theme::hairline(if lands { theme::accent() } else { theme::border_strong() }),
        egui::StrokeKind::Inside,
    );
    widgets::paint_text(&painter, rect.min + Vec2::new(PAD, 3.0), galley, theme::text_primary());
    ctx.request_repaint();
}

/// Where an arrangement clip's name is drawn, by (track, index): the inline rename's place.
pub(crate) const NAME_RECT_CLIP: &str = "clip-name-rect";
/// Where a clip slot's name is drawn, by (track, scene from 1).
pub(crate) const NAME_RECT_SLOT: &str = "slot-name-rect";
/// Where a track's name is drawn in its header, by track id, in the view shown (#696).
pub(crate) const NAME_RECT_TRACK: &str = "track-name-rect";

/// Shows a bar with `show`, its separator line in the bar's own `border-color` (`.toolbar {
/// border-color }`), else the theme's border.
fn with_bar_line<R>(ui: &mut Ui, class: &str, show: impl FnOnce(&mut Ui) -> R) -> R {
    let line = {
        let _bar = crate::kit::style_class::enter(class);
        let own = crate::kit::style_class::computed();
        // A bar with no line (`border-width: 0`, or a transparent border) fills the line's pixel
        // with its own background, so no gap shows the window behind it.
        let lineless = own.as_ref().is_some_and(|c| {
            (c.authored.border_width && c.border_width <= 0.0) || (c.authored.border && c.border.a() == 0)
        });
        if lineless {
            own.as_ref().filter(|c| c.authored.background).map_or_else(theme::panel_background, |c| c.background)
        } else {
            theme::border()
        }
    };
    let saved = ui.visuals().widgets.noninteractive.bg_stroke;
    ui.visuals_mut().widgets.noninteractive.bg_stroke.color = line;
    let shown = show(ui);
    ui.visuals_mut().widgets.noninteractive.bg_stroke = saved;
    shown
}

/// Draws a workspace panel with `draw`; when the theme's grid lays `panel` over the `main` view
/// (both in the same cells), on a layer above the view, so it shows and takes the pointer there.
fn over_view<R>(ui: &mut Ui, name: &str, panel: Rect, main: Rect, draw: impl FnOnce(&mut Ui) -> R) -> R {
    if panel.intersect(main).is_positive() {
        let layer = egui::LayerId::new(egui::Order::Middle, Id::new(("workspace-overlay", name)));
        ui.scope_builder(egui::UiBuilder::new().layer_id(layer), draw).inner
    } else {
        draw(ui)
    }
}

/// Whether the track headers are on the left: on the user's side, or else on the theme's, or else
/// on the right.
fn headers_left(side: Option<Side>) -> bool {
    match side {
        Some(side) => side == Side::Left,
        None => crate::kit::style_class::theme_track_headers_left().unwrap_or(false),
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
