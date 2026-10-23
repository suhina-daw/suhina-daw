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

/// What a layout was made from, to know when it is out of date.
#[derive(Debug, Clone, PartialEq)]
struct ScopeKey {
    place: Option<String>,
    max_seconds: u32,
    layout_key: u64,
}

/// A finished layout: the map, its grid and the view's aspect the grid was made for.
type Laid = (Map, Grid, f32);

/// A layout being computed on a thread of its own.
struct Job {
    key: ScopeKey,
    receiver: Receiver<Laid>,
    progress: Progress,
    /// When it started, in the window's time.
    started: f64,
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
    /// made for, and where it arrives.
    grid_job: Option<(f32, Receiver<Grid>)>,
    last_layout: f64,
    points: Vec<MapPoint>,
    /// What the points' flags were made from: search, tags, selection, pins, project.
    flags_key: Option<String>,
    /// An item double-clicked, for the window to add to the selected track.
    pub add: Vec<BrowserItem>,
    /// Something to say in the status bar (a preview that cannot play).
    pub status: Option<String>,
    /// A kind was set by hand since the layout was made: the next refresh lays the scope out
    /// again, so the item moves to its kind's region.
    stale: bool,
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
            grid_job: None,
            last_layout: f64::NEG_INFINITY,
            points: Vec::new(),
            flags_key: None,
            add: Vec::new(),
            status: None,
            stale: false,
        }
    }
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
    /// The browser's place as the `soundmap.*` commands take it.
    fn place(state: &BrowserState) -> Option<String> {
        match &state.place {
            None => None,
            Some(Place::Pinned) => Some("pinned".into()),
            Some(place @ Place::Recent(_)) => Some(place.param()),
            Some(Place::Folder(path)) => Some(path.display().to_string()),
        }
    }

    /// Keeps the layout in line with the place, the analysis and the settings: takes in a
    /// finished layout and starts a new one when it is out of date.
    fn refresh(&mut self, ui: &Ui, state: &BrowserState, projects: &mut Projects, settings: &Settings) {
        let analysed = projects.poll_sound_map();
        if let Some(job) = &self.job {
            match job.receiver.try_recv() {
                Ok((map, grid, aspect)) => {
                    projects.sound_map_mut().keep(&map);
                    self.shown = Some(job.key.clone());
                    self.map = Some(map);
                    self.grid = Some(grid);
                    self.grid_aspect = aspect;
                    self.flags_key = None;
                    self.job = None;
                    self.grid_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.job = None,
            }
        }
        let now = ui.input(|i| i.time);
        let place = Self::place(state);
        let scope_changed = self.stale
            || self.shown.as_ref().is_none_or(|s| s.place != place || s.max_seconds != settings.map_max_seconds);
        if !scope_changed && !(analysed && now - self.last_layout >= RELAYOUT_SECONDS) {
            if analysed {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(RELAYOUT_SECONDS));
            }
            return;
        }
        let Ok(files) = suhina_session::scope_files(projects, place.as_deref()) else { return };
        let prepared = projects.sound_map_mut().prepare(&files, settings.map_max_seconds as f32);
        self.stale = false;
        let key = ScopeKey { place, max_seconds: settings.map_max_seconds, layout_key: prepared.key() };
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
        if spawned.is_ok() {
            self.job = Some(Job { key, receiver, progress, started: now });
        }
    }

    /// Remakes the grid when the view's shape changed noticeably since it was made, so the grid
    /// keeps the view's aspect ratio. Snapping a large map takes a while (over a second for
    /// 150 000 samples), so it is done on a thread of its own, one at a time, and only while the
    /// grid is shown; the old grid is drawn stretched meanwhile.
    fn fit_grid(&mut self, ui: &Ui) {
        if let Some((aspect, receiver)) = &self.grid_job {
            match receiver.try_recv() {
                Ok(grid) => {
                    self.grid_aspect = *aspect;
                    self.grid = Some(grid);
                    self.grid_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.grid_job = None,
            }
        }
        let aspect = self.view.aspect();
        if self.view.layout != Layout::Grid
            || self.grid.is_none()
            || (aspect / self.grid_aspect - 1.0).abs() <= GRID_ASPECT_SLACK
        {
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
        if spawned.is_ok() {
            self.grid_job = Some((aspect, receiver));
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
            "{}\u{1}{:?}\u{1}{:?}\u{1}{}\u{1}{}\u{1}{}",
            state.search,
            state.tag_filter,
            selected.iter().collect::<std::collections::BTreeSet<_>>(),
            pinned.len(),
            revision,
            projects.file_library_mut().revision(),
        );
        if self.flags_key.as_deref() == Some(key.as_str()) && self.points.len() == map.points.len() {
            return;
        }
        let words = suhina_library::words(&state.search);
        let in_use = files_in_use(projects);
        let library = projects.file_library_mut();
        self.points = map
            .points
            .iter()
            .map(|p| {
                let path_key = p.path.display().to_string();
                let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let tags = library.item_tags(&path_key);
                let text_matches = words.is_empty() || suhina_library::rank(&words, &name, &path_key).is_some();
                let tag_matches = state.tag_filter.is_empty() || tags.iter().any(|t| state.tag_filter.contains(t));
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
                    "Options › Settings › Browser › Map: longest sample changes the limit (5 to 60 s).".into(),
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
        self.view.volume = settings.preview_volume;
        let tags: Vec<String> = projects.file_library_mut().tags().into_iter().map(|t| t.name).collect();
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
