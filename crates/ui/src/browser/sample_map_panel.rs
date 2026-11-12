// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sample map in the browser (`docs/spec/sample-map.md` › Where the map is shown): which
//! samples it holds (the browser's place), laying them out off the window's thread, and turning
//! what the user does on the map into the browser's selection, previews, drags and commands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use egui::{Rect, Ui};
use serde_json::json;
use suhina_session::Projects;
use suhina_soundmap::Map;
use suhina_soundmap::grid::Grid;
use suhina_soundmap::layout::Progress;

use crate::browser::sample_map::{self, Busy, Layout, MapEvent, MapPoint, MapView, StatusPart};
use crate::browser::{BrowserState, Place, ResultsMode};
use crate::interaction::drop::{BrowserItem, ItemKind};
use crate::settings::Settings;

/// A scope is laid out again at most this often while its analysis goes on.
const RELAYOUT_SECONDS: f64 = 2.0;
/// A layout shows its progress card only once it has taken this long, so the quick ones made
/// while samples are analysed do not flash it.
const BUSY_AFTER_SECONDS: f64 = 0.3;
/// How far the view's aspect ratio may drift from the grid's (as a fraction) before the grid is
/// made again.
const GRID_ASPECT_SLACK: f32 = 0.05;
/// How many let-go layout or grid workers may still be running when a new one starts: past
/// it the new one waits for them ([#1254] review: the computations have no point to stop at,
/// so obsolete ones finish, and only this many at a time).
const MAX_RETIRED_RUNNING: usize = 2;

/// What a layout was made from, to know when it is out of date. A tag place's scope follows
/// the tag assignments too: anything tagged in or out of a chosen tag re-lays it
/// ([#1254] review).
#[derive(Debug, Clone, PartialEq)]
struct ScopeKey {
    place: Option<String>,
    max_seconds: u32,
    layout_key: u64,
    tags_revision: u64,
}

/// A finished layout: the map, its grid and the view's aspect the grid was made for.
type Laid = (Map, Grid, f32);

/// A layout being computed on a thread of its own. The thread's handle rides along: an
/// obsolete job's thread is retired, its answer going to a receiver nobody reads, and
/// joined once it has finished — never waited for on the window's thread, where a large
/// map's layout or snap would hold the window for seconds ([#1254] review).
struct Job {
    key: ScopeKey,
    receiver: Receiver<Laid>,
    progress: Progress,
    /// When it started, in the window's time.
    started: f64,
    /// The worker's own join, taken once it has finished.
    worker: std::thread::JoinHandle<()>,
}

/// The map's state in the window.
pub struct Panel {
    pub view: MapView,
    map: Option<Map>,
    grid: Option<Grid>,
    /// The view's aspect the grid was made for.
    grid_aspect: f32,
    shown: Option<ScopeKey>,
    job: Option<Job>,
    /// A grid being made again for the view's new shape, on a thread of its own: the aspect it is
    /// made for, where it arrives, and the thread's own join ([#1254] review).
    grid_job: Option<(f32, Receiver<Grid>, std::thread::JoinHandle<()>)>,
    /// Workers whose answers nobody waits for any more, joined as each one finishes: the
    /// window's thread never waits on one ([#1254] review).
    retired: Vec<std::thread::JoinHandle<()>>,
    /// The analysis changes counted when the layout shown was asked for
    /// ([`Projects::sound_map_changes`]): a count past it lays the map out again.
    laid_changes: u64,
    last_layout: f64,
    points: Vec<MapPoint>,
    /// What the points' flags were made from: search, tags, selection, pins, project.
    flags_key: Option<String>,
    /// An item double-clicked, for the window to add to the selected track.
    pub add: Vec<BrowserItem>,
    /// Files Copy to folder… asked for, for the window to copy once a folder is picked (#865).
    pub copy_to: Vec<std::path::PathBuf>,
    /// Something to say in the status bar (a preview that cannot play).
    pub status: Option<String>,
    /// A kind was set by hand since the layout was made: the next refresh lays the scope out
    /// again, so the item moves to its kind's region.
    stale: bool,
    /// The library generation the shown state belongs to ([`Projects::library_generation`]):
    /// a replacement through any path — the window's or a caller's `projects_mut` — is
    /// detected here, where matching scopes and revisions could keep the old map ([#1254]
    /// review).
    library_gen: u64,
}

impl Default for Panel {
    fn default() -> Self {
        Panel {
            view: MapView::default(),
            map: None,
            grid: None,
            grid_aspect: 1.0,
            shown: None,
            job: None,
            library_gen: 0,
            grid_job: None,
            retired: Vec::new(),
            laid_changes: 0,
            last_layout: f64::NEG_INFINITY,
            points: Vec::new(),
            flags_key: None,
            add: Vec::new(),
            copy_to: Vec::new(),
            status: None,
            stale: false,
        }
    }
}

impl Panel {
    /// Ends the layout and the grid jobs that are on: the answers they send from here on go
    /// to a receiver nobody reads, and their threads are retired, not waited for
    /// ([#1254] review: joining an unfinished snap held the window until it was done).
    fn end_jobs(&mut self) {
        self.end_layout_job();
        self.end_grid_job();
    }

    /// Ends the layout job alone, its thread retired.
    fn end_layout_job(&mut self) {
        if let Some(Job { worker, .. }) = self.job.take() {
            self.retire(worker);
        }
    }

    /// Ends the grid job alone, its thread retired: a grid's end leaves the layout on.
    fn end_grid_job(&mut self) {
        if let Some((_, _, worker)) = self.grid_job.take() {
            self.retire(worker);
        }
    }

    /// Lets a worker go: joined at once when it has finished, else when a later frame finds
    /// it finished ([`Panel::reap`]).
    fn retire(&mut self, worker: std::thread::JoinHandle<()>) {
        self.retired.push(worker);
        self.reap();
    }

    /// Lets go the layout being made for a scope that is no longer asked for, before it is
    /// waited for: its map is never shown, and the asked one starts at once ([#1254] review:
    /// the old job's map was waited for and shown before the new one started).
    fn end_stale_layout(&mut self, place: Option<&str>, max_seconds: u32, tags_revision: u64) {
        let stale = self.job.as_ref().is_some_and(|job| {
            job.key.place.as_deref() != place
                || job.key.max_seconds != max_seconds
                || job.key.tags_revision != tags_revision
        });
        if stale {
            self.end_layout_job();
        }
    }

    /// Whether as many let-go workers are still running as may run beside a new one: a new
    /// layout or grid waits, the frame asking again soon, so obsolete work never piles up
    /// past [`MAX_RETIRED_RUNNING`] ([#1254] review).
    fn busy_retiring(&mut self, ui: &Ui) -> bool {
        self.reap();
        let busy = self.retired.len() >= MAX_RETIRED_RUNNING;
        if busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        busy
    }

    /// Joins the retired workers that have finished; the others are left running, their
    /// answers going nowhere.
    fn reap(&mut self) {
        let (finished, running): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.retired).into_iter().partition(|worker| worker.is_finished());
        for worker in finished {
            let _ = worker.join();
        }
        self.retired = running;
    }

    /// The library went under a replacement (tests set one, and reopening does): the
    /// layouts the panel kept are for its files and its keys only, and a new library can
    /// read the same place and the same revisions, so they are dropped with it
    /// ([#1254] review). What the user holds (the view's largeness, a window) stays.
    pub fn library_replaced(&mut self) {
        self.end_jobs();
        self.map = None;
        self.grid = None;
        self.grid_aspect = 1.0;
        self.shown = None;
        self.last_layout = f64::NEG_INFINITY;
        self.points.clear();
        self.flags_key = None;
        self.stale = true;
        // Show similar's list and a hover, a preview or a pressed point are indices into
        // the old points list; left with a new map's, they read as unrelated samples
        // ([#1254] review). The view's own largeness and window stay.
        self.view.forget_points();
    }
}

/// Where the point `index` of `old` is in `new`, found by its file; `None` when the file is
/// not on the new map.
fn rebound(old: &Map, new: &Map, index: usize) -> Option<usize> {
    let path = &old.points.get(index)?.path;
    new.points.iter().position(|point| &point.path == path)
}

fn item_of(path: &Path) -> BrowserItem {
    BrowserItem {
        key: path.display().to_string(),
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        kind: ItemKind::Sample,
        plugin: false,
    }
}

/// The files the active project's audio clips use, as absolute paths.
fn files_in_use(projects: &Projects) -> HashSet<PathBuf> {
    let Some(session) = projects.active().and_then(|id| projects.get(id)) else { return HashSet::new() };
    let dir = session.source().and_then(Path::parent).map(Path::to_path_buf);
    let mut out = HashSet::new();
    for content in &session.project().contents {
        for lane in &content.lanes {
            let suhina_engine::model::LaneData::Audio(region) = &lane.data else { continue };
            if let Some(absolute) = &region.absolute_file {
                out.insert(PathBuf::from(absolute));
            }
            if let Some(dir) = &dir {
                out.insert(dir.join(&region.file));
            }
            out.insert(PathBuf::from(&region.file));
        }
    }
    out
}

impl Panel {
    /// The browser's place as the `soundmap.*` commands take it. A tag's place is one the
    /// commands take: `tag:#<id>`, joined for a place of several ([#1254] review: the map
    /// follows the tags, #862).
    fn place(state: &BrowserState) -> Option<String> {
        match &state.place {
            None => None,
            Some(Place::Pinned) => Some("pinned".into()),
            Some(place @ (Place::Recent(_) | Place::Tag(_))) => Some(place.param()),
            Some(Place::Folder(path)) => Some(path.display().to_string()),
        }
    }

    /// Keeps the layout in line with the place, the analysis and the settings: takes in a
    /// finished layout and starts a new one when it is out of date.
    fn refresh(&mut self, ui: &Ui, state: &BrowserState, projects: &mut Projects, settings: &Settings) {
        // A replaced library first, before any of the kept state is read: its scopes and
        // revisions can read as the new library's own ([#1254] review).
        let library_gen = projects.library_generation();
        if self.library_gen != library_gen {
            self.library_gen = library_gen;
            self.library_replaced();
        }
        projects.poll_sound_map();
        // Samples analysed since the layout shown was made, whoever polled first: the
        // window polls every frame too, and the notice that poll took is still seen here
        // ([#1254] review: the map kept a sample out until something else re-laid it).
        let changes = projects.sound_map_changes();
        let analysed = changes != self.laid_changes;
        self.reap();
        let place = Self::place(state);
        // A tag place's map follows the tag assignments as well: anything tagged in or out
        // of a chosen tag re-lays it ([#1254] review). The revision read once, and only for
        // the places it matters to; the rest of the tags' doings are none of the map's.
        let tags_revision = matches!(&state.place, Some(Place::Tag(_)))
            .then(|| projects.file_library_mut().tag_revision())
            .unwrap_or_default();
        self.end_stale_layout(place.as_deref(), settings.map_max_seconds, tags_revision);
        if let Some(job) = &self.job {
            match job.receiver.try_recv() {
                Ok((map, grid, aspect)) => {
                    projects.sound_map_mut().keep(&map);
                    self.shown = Some(job.key.clone());
                    // The view's point indices follow their files into the new map: a tag
                    // dropped or a file analysed shifts the indices, and Show similar's list
                    // read unrelated samples ([#1254] review).
                    match &self.map {
                        Some(old) => self.view.rebind_points(|i| rebound(old, &map, i)),
                        None => self.view.forget_points(),
                    }
                    self.map = Some(map);
                    self.grid = Some(grid);
                    self.grid_aspect = aspect;
                    self.flags_key = None;
                    self.end_jobs();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.end_jobs(),
            }
        }
        let now = ui.input(|i| i.time);
        let scope_changed = self.stale
            || self.shown.as_ref().is_none_or(|s| {
                s.place != place || s.max_seconds != settings.map_max_seconds || s.tags_revision != tags_revision
            });
        if !scope_changed && !(analysed && now - self.last_layout >= RELAYOUT_SECONDS) {
            if analysed {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(RELAYOUT_SECONDS));
            }
            return;
        }
        // Too many let-go workers still running: the next layout waits for them to finish,
        // a frame at a time, never on the window's thread ([#1254] review: replacing scopes
        // fast piled workers up beside the new one). The layout asked for stays asked: a
        // notification of analysed samples comes once, and a frame that waits keeps it
        // ([#1254] review).
        if self.busy_retiring(ui) {
            self.stale = true;
            return;
        }
        let Ok(files) = suhina_session::scope_files(projects, place.as_deref()) else {
            // The place names a tag or folder the library no longer holds — a caller that
            // never ran the browser's reconcile (a `projects_mut` removal, [#1254] review).
            // The map on the wall is none of this place's, so it goes with the jobs; the
            // next scope that reads lays its own.
            if self.map.is_some() || self.shown.is_some() {
                self.map = None;
                self.end_jobs();
                self.shown = None;
            }
            self.stale = false;
            return;
        };
        let prepared = projects.sound_map_mut().prepare(&files, settings.map_max_seconds as f32);
        self.stale = false;
        self.laid_changes = changes;
        let key = ScopeKey { place, max_seconds: settings.map_max_seconds, layout_key: prepared.key(), tags_revision };
        self.last_layout = now;
        if self.shown.as_ref() == Some(&key) {
            return;
        }
        // Even a kept layout is finished on a thread of its own: finding the kinds and the grid of
        // a large library takes seconds, which the window must not wait for.
        let (sender, receiver) = channel();
        let ctx = ui.ctx().clone();
        let aspect = self.view.aspect();
        let progress = Progress::new();
        let counter = progress.clone();
        let spawned = std::thread::Builder::new().name("soundmap-layout".into()).spawn(move || {
            let map = prepared.compute_with_progress(&counter);
            let grid = map.grid(aspect);
            let _ = sender.send((map, grid, aspect));
            ctx.request_repaint();
        });
        match spawned {
            Ok(worker) => {
                // The job it replaces is obsolete from here on: its thread is retired, its
                // answer read by nobody, and the window does not wait for it ([#1254] review).
                self.end_layout_job();
                self.job = Some(Job { key, receiver, progress, started: now, worker });
            }
            Err(_) => {
                // No thread to lay it out on now: the layout stays asked and is tried again
                // ([#1254] review: the request was cleared and the old map kept for good).
                self.stale = true;
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(RELAYOUT_SECONDS));
            }
        }
    }

    /// Remakes the grid when the view's shape changed noticeably since it was made, so the grid
    /// keeps the view's aspect ratio. Snapping a large map takes a while (over a second for
    /// 150 000 samples), so it is done on a thread of its own, one at a time, and only while the
    /// grid is shown; the old grid is drawn stretched meanwhile.
    fn fit_grid(&mut self, ui: &Ui) {
        if let Some(borrowed) = self.grid_job.as_ref() {
            let aspect = borrowed.0;
            match borrowed.1.try_recv() {
                Ok(grid) => {
                    self.grid_aspect = aspect;
                    self.grid = Some(grid);
                    self.end_grid_job();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                    return;
                }
                // The grid's worker is gone: its job ends, the layout's goes on.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.end_grid_job(),
            }
        }
        let aspect = self.view.aspect();
        if self.view.layout != Layout::Grid
            || self.grid.is_none()
            || (aspect / self.grid_aspect - 1.0).abs() <= GRID_ASPECT_SLACK
        {
            return;
        }
        if self.busy_retiring(ui) {
            return;
        }
        let Some(map) = &self.map else { return };
        let positions: Vec<[f32; 2]> = map.points.iter().map(|p| p.position).collect();
        let (sender, receiver) = channel();
        let ctx = ui.ctx().clone();
        let spawned = std::thread::Builder::new().name("soundmap-grid".into()).spawn(move || {
            let _ = sender.send(suhina_soundmap::grid::snap(&positions, aspect));
            ctx.request_repaint();
        });
        if let Ok(worker) = spawned {
            self.end_grid_job();
            self.grid_job = Some((aspect, receiver, worker));
        }
    }

    /// Rebuilds the points when the map changed, and their flags when the search, the tags,
    /// the selection, the pins or the project changed.
    fn update_points(&mut self, state: &BrowserState, projects: &mut Projects, revision: u64) {
        let Some(map) = &self.map else {
            self.points.clear();
            return;
        };
        let selected: HashSet<&str> = state.selected().iter().map(|i| i.key.as_str()).collect();
        let pinned: HashSet<String> = projects.file_library_mut().pinned().into_iter().collect();
        let key = format!(
            "{}\u{1}{:?}\u{1}{:?}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
            state.search,
            state.tag_filter,
            selected.iter().collect::<std::collections::BTreeSet<_>>(),
            pinned.len(),
            revision,
            projects.file_library_mut().revision(),
            // A tag drop re-lights (or dims) the matching point even when nothing else in the
            // library moved (#1196).
            projects.file_library_mut().tag_revision(),
        );
        if self.flags_key.as_deref() == Some(key.as_str()) && self.points.len() == map.points.len() {
            return;
        }
        let words = suhina_library::words(&state.search);
        let in_use = files_in_use(projects);
        let library = projects.file_library_mut();
        let flat_tags = library.tag_flat();
        self.points = map
            .points
            .iter()
            .map(|p| {
                let path_key = p.path.display().to_string();
                let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let carried = library.item_tags(&path_key);
                let text_matches = words.is_empty() || suhina_library::rank(&words, &name, &path_key).is_some();
                let tag_matches = state.tag_filter.is_empty() || carried.iter().any(|id| state.tag_filter.contains(id));
                let tags =
                    flat_tags.iter().filter(|tag| carried.contains(&tag.id)).map(|tag| tag.name.clone()).collect();
                let folder = p.path.parent().map(|f| f.display().to_string()).unwrap_or_default();
                MapPoint {
                    detail: folder,
                    name,
                    position: p.position,
                    kind: p.kind,
                    guessed: p.guessed,
                    by_hand: p.by_hand,
                    in_use: in_use.contains(&p.path),
                    selected: selected.contains(path_key.as_str()),
                    matches: text_matches && tag_matches,
                    preset: false,
                    pinned: pinned.contains(&path_key),
                    tags,
                }
            })
            .collect();
        self.flags_key = Some(key);
        self.view.points_changed();
    }

    /// How far the layout being computed is, as the status says it.
    fn arranging(&self) -> Option<String> {
        let job = self.job.as_ref()?;
        let fraction = job.progress.fraction();
        Some(if fraction > 0.0 { format!("arranging {} %", percent(fraction)) } else { "arranging…".into() })
    }

    fn status(&self) -> String {
        let Some(map) = &self.map else {
            return match self.arranging() {
                Some(arranging) => capitalised(&arranging),
                None => "No samples analysed yet".into(),
            };
        };
        let mut parts = vec![format!("{} samples", map.points.len())];
        if let Some(arranging) = self.arranging() {
            parts.push(arranging);
        }
        if map.pending > 0 {
            parts.push(format!("analysing, {} to go", map.pending));
        }
        if map.too_long > 0 {
            parts.push(format!("{} too long", map.too_long));
        }
        if map.failed > 0 {
            parts.push(format!("{} could not be read", map.failed));
        }
        if map.over_limit > 0 {
            parts.push(format!("{} left out", map.over_limit));
        }
        parts.join(", ")
    }

    /// What the status's tooltip says beyond the status: why samples are missing from the map.
    fn status_details(&self, max_seconds: u32) -> Vec<StatusPart> {
        let Some(map) = &self.map else { return Vec::new() };
        let mut parts = Vec::new();
        if map.pending > 0 {
            parts.push(StatusPart {
                heading: format!("{} still to analyse", map.pending),
                lines: vec!["Each sample is analysed once and kept; the map is arranged again as they come in.".into()],
            });
        }
        if map.too_long > 0 {
            parts.push(StatusPart {
                heading: format!("{} too long", map.too_long),
                lines: vec![
                    format!("Longer than {max_seconds} s: the map is for one-shots and short loops."),
                    "Options › Settings › Browser › Map sample length limit changes the limit (5 to 60 s).".into(),
                ],
            });
        }
        if map.failed > 0 {
            let mut lines = Vec::new();
            for failure in &map.failures {
                lines.push(format!("{}: {}", failure.count, failure.detail));
                let names: Vec<String> = failure
                    .examples
                    .iter()
                    .filter_map(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect();
                if !names.is_empty() {
                    let more = if failure.count > names.len() { ", …" } else { "" };
                    lines.push(format!("    {}{more}", names.join(", ")));
                }
            }
            parts.push(StatusPart { heading: format!("{} could not be read", map.failed), lines });
        }
        if map.over_limit > 0 {
            parts.push(StatusPart {
                heading: format!("{} left out", map.over_limit),
                lines: vec![format!(
                    "A map holds at most {} samples; choose a folder in the sidebar to see the rest.",
                    suhina_soundmap::MAX_ITEMS
                )],
            });
        }
        parts
    }

    /// The card in the middle of the map: the analysis while samples are left to analyse, else a
    /// layout that has been running for a moment.
    fn busy(&self, now: f64) -> Option<Busy> {
        if let Some(map) = &self.map
            && map.pending > 0
        {
            let done = map.points.len();
            let total = done.saturating_add(map.pending);
            return Some(Busy {
                title: "Analysing samples".into(),
                detail: format!("{done} of {total}; the map is arranged as it goes"),
                fraction: done as f32 / total.max(1) as f32,
            });
        }
        let job = self.job.as_ref()?;
        if now - job.started < BUSY_AFTER_SECONDS {
            return None;
        }
        let fraction = job.progress.fraction();
        Some(Busy {
            title: "Arranging the map".into(),
            detail: if fraction > 0.0 { format!("{} %", percent(fraction)) } else { "Getting the map ready".into() },
            fraction,
        })
    }

    /// Moves the map into a window of its own (`true`) or back into the browser. While it has a
    /// window the browser shows its list, as it does for the large map.
    pub fn set_window(&mut self, window: bool, state: &mut BrowserState) {
        self.view.window = window;
        self.view.large = false;
        state.mode = if window { ResultsMode::List } else { ResultsMode::Other };
    }

    /// Draws the map in `rect` (the browser's results area, or the main area when large).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        state: &mut BrowserState,
        projects: &mut Projects,
        settings: &mut Settings,
        revision: u64,
    ) -> bool {
        self.refresh(ui, state, projects, settings);
        self.view.layout = if settings.map_grid { Layout::Grid } else { Layout::Cloud };
        self.fit_grid(ui);
        suhina_plugins::trace::timed("sample map points", || self.update_points(state, projects, revision));
        self.view.preview_on_hover = settings.map_hover_preview;
        self.view.dot_size = settings.map_dot_size as f32 / 100.0;
        self.view.label_size = settings.map_label_size as f32 / 100.0;
        self.view.volume = settings.preview_volume;
        let tags: Vec<String> = projects.file_library_mut().tag_flat().into_iter().map(|t| t.name).collect();
        let status = self.status();
        let details = self.status_details(settings.map_max_seconds);
        let busy = self.busy(ui.input(|i| i.time));
        // The waveform for the tooltip of the point the pointer rests on, read in the background.
        let now = ui.input(|i| i.time);
        let resting = self.view.resting(now);
        self.view.hover_waveform = resting.and_then(|i| Some((i, state.waveform(&self.path(i)?)?)));
        if resting.is_some() && self.view.hover_waveform.is_none() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        let mut events = Vec::new();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            sample_map::show(
                ui,
                &mut self.view,
                &self.points,
                self.grid.as_ref(),
                &status,
                &details,
                busy.as_ref(),
                &tags,
                &mut events,
            );
        });
        let mut settings_changed = false;
        for event in events {
            settings_changed |= self.handle(event, state, projects, settings);
        }
        settings_changed
    }

    fn path(&self, index: usize) -> Option<PathBuf> {
        self.map.as_ref()?.points.get(index).map(|p| p.path.clone())
    }

    /// Acts on one event; returns whether a setting changed.
    fn handle(
        &mut self,
        event: MapEvent,
        state: &mut BrowserState,
        projects: &mut Projects,
        settings: &mut Settings,
    ) -> bool {
        match event {
            MapEvent::Preview(i) => {
                if let Some(path) = self.path(i)
                    && let Err(why) = preview(state, projects, path)
                {
                    self.status = Some(why);
                }
            }
            MapEvent::Select { items, add } => {
                let picked: Vec<BrowserItem> =
                    items.iter().filter_map(|i| self.path(*i)).map(|p| item_of(&p)).collect();
                let selection = if add {
                    let mut current = state.selected().to_vec();
                    for item in picked {
                        match current.iter().position(|c| c.key == item.key) {
                            Some(at) => {
                                current.remove(at);
                            }
                            None => current.push(item),
                        }
                    }
                    current
                } else {
                    picked
                };
                // The map previews by itself; the selection must not preview again.
                state.select(selection);
                state.selection_changed = false;
            }
            MapEvent::StartDrag(items) => {
                let items: Vec<BrowserItem> = items.iter().filter_map(|i| self.path(*i)).map(|p| item_of(&p)).collect();
                state.start_drag(items);
            }
            MapEvent::Add(i) => {
                if let Some(path) = self.path(i) {
                    self.add.push(item_of(&path));
                }
            }
            MapEvent::Similar(i) => {
                if let (Some(map), Some(path)) = (&self.map, self.path(i)) {
                    let similar: Vec<usize> = map
                        .similar(&path, 16)
                        .into_iter()
                        .filter_map(|(p, _)| map.points.iter().position(|q| q.path == p))
                        .collect();
                    self.view.show_similar(i, similar);
                }
            }
            MapEvent::Pin(i) => {
                if let Some(path) = self.path(i) {
                    let key = path.display().to_string();
                    let pinned = projects.file_library_mut().pinned().contains(&key);
                    let id = if pinned { "library.unpin" } else { "library.pin" };
                    let _ = projects.execute(id, &json!({ "item": key }));
                    self.flags_key = None;
                }
            }
            MapEvent::Tag { item, tag, on } => {
                if let Some(path) = self.path(item) {
                    let _ = projects
                        .execute("library.tag", &json!({ "item": path.display().to_string(), "tag": tag, "on": on }));
                    self.flags_key = None;
                }
            }
            MapEvent::CopyTo(i) => {
                if let Some(path) = self.path(i) {
                    self.copy_to = crate::browser::browser_view::copy_files(&item_of(&path), state.selected());
                }
            }
            MapEvent::ShowInFolder(i) => {
                if let Some(path) = self.path(i) {
                    if let Some(folder) = path.parent() {
                        state.place = Some(Place::Folder(folder.to_path_buf()));
                    }
                    state.mode = ResultsMode::List;
                    self.view.large = false;
                    state.select(vec![item_of(&path)]);
                    state.selection_changed = false;
                }
            }
            MapEvent::SetKind { item, kind } => {
                if let Some(path) = self.path(item) {
                    let id = kind.map_or("auto", |k| k.id());
                    match projects
                        .execute("soundmap.setKind", &json!({ "item": path.display().to_string(), "kind": id }))
                    {
                        Ok(_) => {
                            // Shown at once; the new layout follows.
                            if let (Some(kind), Some(point)) =
                                (kind, self.map.as_mut().and_then(|m| m.points.get_mut(item)))
                            {
                                point.kind = kind;
                                point.guessed = false;
                                point.by_hand = true;
                            }
                            self.flags_key = None;
                            self.stale = true;
                        }
                        Err(e) => self.status = Some(e.message().to_owned()),
                    }
                }
            }
            MapEvent::Layout(layout) => {
                settings.map_grid = layout == Layout::Grid;
                return true;
            }
            MapEvent::DotSize(percent) => {
                settings.map_dot_size = percent;
                return true;
            }
            MapEvent::LabelSize(percent) => {
                settings.map_label_size = percent;
                return true;
            }
            MapEvent::Volume(db) => {
                settings.preview_volume = db;
                if state.previewing.is_some() {
                    let _ = projects.execute("preview.setGain", &json!({ "gain": db }));
                }
                return true;
            }
            MapEvent::ToggleLarge => {
                self.view.large = !self.view.large;
                // The large map takes the main area; the browser shows its list meanwhile.
                state.mode = if self.view.large { ResultsMode::List } else { ResultsMode::Other };
            }
            MapEvent::ToggleWindow => {
                self.set_window(!self.view.window, state);
                settings.map_window = self.view.window;
                return true;
            }
        }
        false
    }
}

/// A fraction from 0 to 1 as a whole percentage.
fn percent(fraction: f32) -> u32 {
    (fraction.clamp(0.0, 1.0) * 100.0).floor() as u32
}

/// `text` with its first letter in upper case.
fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Previews a sample as the browser does: decoded on the browser's reader thread, then played.
fn preview(state: &mut BrowserState, projects: &mut Projects, path: PathBuf) -> Result<(), String> {
    crate::browser::browser_view::can_preview(projects)?;
    crate::browser::browser_view::preview_sample(state, projects, path, 0.0);
    Ok(())
}

#[cfg(test)]
#[path = "sample_map_panel_tests.rs"]
mod tests;
