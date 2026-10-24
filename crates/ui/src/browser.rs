// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser's state and what it lists (`docs/spec/browser.md`): the search, the category,
//! the place chosen in the sidebar, the open folders of the tree, the selection, and what is
//! known about files (length, tempo, key, waveform), read on a thread of its own so the window
//! never waits for the disk. `browser_view` draws it.

pub mod browser_view;
pub mod sample_map;
mod sample_map_crowd;
pub mod sample_map_panel;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use suhina_device_api::Role;
use suhina_engine::AudioData;
use suhina_library::{FileKind, Group, Item, Listing};
use suhina_media::info::{Tempo, TempoSource};
use suhina_session::{Category, Found, Projects};

use crate::interaction::drop::{BrowserItem, ItemKind};
use crate::kit::waveform::{Peaks, peaks};

/// What the sidebar can show instead of the whole tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Place {
    Pinned,
    /// The recent items: of one group, or of every group (`None`, #711).
    Recent(Option<Group>),
    Folder(PathBuf),
}

impl Place {
    /// As `library.search` takes it.
    pub fn param(&self) -> String {
        match self {
            Self::Pinned => "pinned".into(),
            Self::Recent(Some(group)) => format!("recent:{}", group.name()),
            Self::Recent(None) => "recent:all".into(),
            Self::Folder(path) => path.display().to_string(),
        }
    }
}

impl Place {
    /// The category a recent group's place belongs to: choosing the place chooses it, so the
    /// category never hides everything the place lists. `None` for the places that hold any
    /// kind (the pins, a folder, the recent projects).
    pub fn category(&self) -> Option<Category> {
        match self {
            Self::Recent(Some(Group::Samples)) => Some(Category::Samples),
            Self::Recent(Some(Group::Midi)) => Some(Category::Midi),
            Self::Recent(Some(Group::Instruments)) => Some(Category::Instruments),
            Self::Recent(Some(Group::Effects)) => Some(Category::Effects),
            Self::Recent(Some(Group::Plugins)) => Some(Category::Plugins),
            Self::Recent(Some(Group::Presets)) => Some(Category::Presets),
            Self::Recent(Some(Group::Projects) | None) | Self::Pinned | Self::Folder(_) => None,
        }
    }
}

/// How the results area shows what was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsMode {
    /// The list or tree of rows.
    #[default]
    List,
    /// Another view, drawn by `browser_view::Context::other_results`: the sample map, chosen
    /// with its own button in the category row.
    Other,
}

/// One row of the results.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub item: BrowserItem,
    pub depth: usize,
    /// It opens (a folder, a heading, a preset bank).
    pub opens: bool,
    pub open: bool,
    /// What it is shown under in a flat list (the folder a search found it in).
    pub context: Option<String>,
    /// The columns the plug-ins' flat list shows, for a plug-in row there.
    pub columns: Option<PluginColumns>,
    /// Where it is in the tree, when that is not its key alone: what opens and closes it and
    /// tells it from another row of the same item. A library folder inside another library
    /// folder is listed twice, at the top and inside the other one, and each opens on its own
    /// (#869). `None` for a row listed once (a heading, a top-level folder, a search's row).
    pub node: Option<String>,
}

impl Row {
    /// What opens and closes this row ([`BrowserState::toggle`]) and identifies it on screen.
    pub fn node(&self) -> &str {
        self.node.as_deref().unwrap_or(&self.item.key)
    }
}

/// The node of `key` listed inside the folder whose node is `parent` (see [`Row::node`]).
fn child_node(parent: &str, key: &str) -> String {
    format!("{parent}\u{1}{key}")
}

/// The columns of one row of the Plug-ins category's list besides the name
/// ([the plug-in browsing spec](../../docs/spec/plugin-browsing.md) › Instruments and Effects).
/// The vendor, version and file are the plug-in manager's (Settings › Plug-ins).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginColumns {
    /// `vst3`, `clap`, …
    pub format: &'static str,
}

/// What is known about a file, as it is read in the background.
#[derive(Debug, Clone, Default)]
pub struct FileInfo {
    pub seconds: Option<f64>,
    pub tempo: Option<Tempo>,
    pub key: Option<String>,
    /// The decoded sample and its waveform, once read whole (only for the selected file).
    pub data: Option<Arc<AudioData>>,
    pub peaks: Option<Arc<Peaks>>,
    /// A MIDI file's notes as (start, length, key) in beats, and its length in beats.
    pub notes: Option<Arc<Vec<(f64, f64, u8)>>>,
    pub beats: f64,
    pub error: Option<String>,
}

enum Request {
    /// The header: length, rate, tempo and key from metadata or the name.
    Header(PathBuf),
    /// Everything: decoded, with its waveform and a detected tempo.
    Whole(PathBuf),
}

struct Answer {
    path: PathBuf,
    whole: bool,
    info: FileInfo,
}

/// Lets go of a file's decoded audio, keeping its waveform (without the samples it would keep
/// alive).
fn forget_audio(info: &mut FileInfo) {
    info.data = None;
    if let Some(peaks) = info.peaks.as_mut().filter(|p| p.has_samples()) {
        *peaks = Arc::new(peaks.without_samples());
    }
}

/// Files whose headers are kept; the oldest are forgotten first.
const MAX_INFO: usize = 4096;
/// Samples longer than this get no tempo detection in the browser.
const DETECT_SECONDS: f64 = 60.0;

/// Reads files on a thread of its own.
struct Reader {
    to: Option<Sender<Request>>,
    from: Option<Receiver<Answer>>,
}

impl Reader {
    fn start() -> Self {
        let (to, requests) = std::sync::mpsc::channel::<Request>();
        let (answers, from) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new().name("suhina-browser-files".into()).spawn(move || {
            while let Ok(request) = requests.recv() {
                let (path, whole) = match request {
                    Request::Header(path) => (path, false),
                    Request::Whole(path) => (path, true),
                };
                let info = read_info(&path, whole);
                // The decoded audio goes back at once, so a preview starts without waiting for
                // the tempo; a short sample's tempo is detected after and sent on its own.
                let detect = info
                    .data
                    .clone()
                    .filter(|_| info.tempo.is_none() && info.seconds.is_some_and(|s| s <= DETECT_SECONDS));
                if answers.send(Answer { path: path.clone(), whole, info }).is_err() {
                    return;
                }
                if let Some(data) = detect {
                    let tempo =
                        suhina_media::tempo::detect(&data).map(|bpm| Tempo { bpm, source: TempoSource::Detected });
                    if tempo.is_some() {
                        let info = FileInfo { tempo, ..FileInfo::default() };
                        if answers.send(Answer { path, whole: false, info }).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        match started {
            Ok(_) => Self { to: Some(to), from: Some(from) },
            Err(_) => Self { to: None, from: None },
        }
    }
}

fn read_info(path: &Path, whole: bool) -> FileInfo {
    let mut info = FileInfo::default();
    match suhina_media::info::kind_of(path) {
        Some(FileKind::Sample | FileKind::Stems) => {
            match suhina_media::info::sample_info(path) {
                Ok(header) => {
                    info.seconds = header.duration_seconds;
                    info.tempo = header.tempo;
                    info.key = header.key;
                }
                Err(e) => info.error = Some(e),
            }
            if whole {
                match suhina_media::decode_file(path) {
                    Ok(data) => {
                        let seconds = data.frames() as f64 / f64::from(data.sample_rate.max(1));
                        info.seconds = Some(seconds);
                        let data = Arc::new(data);
                        info.peaks = Some(Arc::new(peaks(&data)));
                        info.data = Some(data);
                        info.error = None;
                    }
                    Err(e) => info.error = Some(e),
                }
            }
        }
        Some(FileKind::Midi) => match suhina_media::midi::read_file(path) {
            Ok(midi) => {
                let notes: Vec<(f64, f64, u8)> =
                    midi.tracks.iter().flat_map(|t| t.notes.iter().map(|n| (n.time, n.duration, n.key))).collect();
                info.beats = midi.tracks.iter().map(|t| t.end()).fold(0.0, f64::max);
                let bpm = midi.tempo_bpm.unwrap_or(120.0);
                info.seconds = Some(info.beats * 60.0 / bpm);
                info.tempo = midi.tempo_bpm.map(|bpm| Tempo { bpm, source: TempoSource::Metadata });
                info.notes = Some(Arc::new(notes));
            }
            Err(e) => info.error = Some(e),
        },
        _ => {}
    }
    info
}

/// The browser panel's state. One for the window: it is the same whichever project shows.
pub struct BrowserState {
    pub mode: ResultsMode,
    pub search: String,
    pub category: Category,
    pub place: Option<Place>,
    /// Open nodes: folders by path, headings and vendors by name.
    pub open: HashSet<String>,
    pub selected: Vec<BrowserItem>,
    /// The row last clicked, for Shift+click.
    pub anchor: Option<usize>,
    /// Only items carrying any of these tags.
    pub tag_filter: Vec<String>,
    /// The tag editor is open (the browser footer's Edit tags button toggles it).
    pub tag_editor: bool,
    /// The tag whose Remove is armed in the editor: the second click removes it.
    pub(crate) tag_editor_confirm: Option<String>,
    /// How many items carry each tag, as last counted (the editor opened, or a tag changed).
    pub(crate) tag_counts: Option<HashMap<String, usize>>,
    /// The Plug-ins category's list sorts by this column (0 name, 1 format), `true`
    /// descending. The window's own, not kept between runs.
    pub plugin_sort: (u8, bool),
    /// Put the cursor in the search field on the next frame.
    pub focus_search: bool,
    /// And select what it says, so typing replaces it (Ctrl+F).
    pub select_search: bool,
    /// The search field had the keyboard last frame.
    pub search_focused: bool,
    /// Escape in the search field asked for the preview to stop.
    pub stop_preview: bool,
    /// The sidebar's field for a folder to add, while it shows.
    pub new_folder: Option<String>,
    /// Items being dragged into the project.
    pub drag: Option<Vec<BrowserItem>>,
    /// The file previewing, by key.
    pub previewing: Option<String>,
    /// A sample waiting to be read whole before it previews.
    pending_preview: Option<(PathBuf, f64)>,
    /// The row a button press selected, so its release does not select it again.
    pub(crate) handled_press: Option<String>,
    /// Why the sample waiting to preview could not be read, to show once.
    preview_error: Option<String>,
    pub scroll: f32,
    rows: Vec<Row>,
    rows_key: Option<String>,
    listings: HashMap<PathBuf, Arc<Vec<Listing>>>,
    voices: HashMap<PathBuf, Arc<Vec<(usize, String)>>>,
    info: HashMap<PathBuf, FileInfo>,
    asked: HashSet<(PathBuf, bool)>,
    reader: Option<Reader>,
    /// Bumped when the tree must be listed again (a folder opened, refreshed).
    generation: u64,
    /// The results and a selection changed this frame.
    pub selection_changed: bool,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            mode: ResultsMode::List,
            search: String::new(),
            category: Category::All,
            place: None,
            open: HashSet::new(),
            selected: Vec::new(),
            anchor: None,
            tag_filter: Vec::new(),
            tag_editor: false,
            tag_editor_confirm: None,
            tag_counts: None,
            plugin_sort: (0, false),
            focus_search: false,
            select_search: false,
            search_focused: false,
            stop_preview: false,
            new_folder: None,
            drag: None,
            previewing: None,
            pending_preview: None,
            preview_error: None,
            handled_press: None,
            scroll: 0.0,
            rows: Vec::new(),
            rows_key: None,
            listings: HashMap::new(),
            voices: HashMap::new(),
            info: HashMap::new(),
            asked: HashSet::new(),
            reader: None,
            generation: 0,
            selection_changed: false,
        }
    }
}

fn item_kind(found: &Found) -> ItemKind {
    match found.kind {
        "sample" => ItemKind::Sample,
        "midi" => ItemKind::Midi,
        "preset" => ItemKind::Preset,
        "project" => ItemKind::Project,
        "instrument" => ItemKind::Instrument,
        "effect" => ItemKind::Effect,
        "audio_effect" => ItemKind::AudioEffect,
        "midi_effect" => ItemKind::MidiEffect,
        _ => ItemKind::Heading,
    }
}

fn file_kind_item(kind: FileKind) -> ItemKind {
    match kind {
        FileKind::Sample | FileKind::Stems => ItemKind::Sample,
        FileKind::Midi => ItemKind::Midi,
        FileKind::Preset => ItemKind::Preset,
        FileKind::Project => ItemKind::Project,
        FileKind::Effect => ItemKind::AudioEffect,
    }
}

fn heading(key: &str, name: &str) -> BrowserItem {
    BrowserItem { key: key.into(), name: name.into(), kind: ItemKind::Heading, plugin: false }
}

impl BrowserState {
    /// The rows as last built.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Forgets what was listed, so folders are read again (after a refresh or a change).
    pub fn relist(&mut self) {
        self.listings.clear();
        self.voices.clear();
        self.generation = self.generation.wrapping_add(1);
        self.rows_key = None;
    }

    /// Opens or closes a node of the tree.
    pub fn toggle(&mut self, node: &str) {
        if !self.open.remove(node) {
            self.open.insert(node.to_string());
        }
        self.rows_key = None;
    }

    /// Selects `items`, replacing the selection (for a view that is not the results list, such
    /// as another way of showing the files).
    pub fn select(&mut self, items: Vec<BrowserItem>) {
        self.selected = items;
        self.selection_changed = true;
    }

    /// The selected items.
    pub fn selected(&self) -> &[BrowserItem] {
        &self.selected
    }

    /// Starts dragging `items` into the project, as dragging rows of the results does.
    pub fn start_drag(&mut self, items: Vec<BrowserItem>) {
        if !items.is_empty() {
            self.drag = Some(items);
        }
    }

    /// Builds the rows when what they come from changed.
    pub fn refresh_rows(&mut self, projects: &mut Projects) -> Result<(), String> {
        let revision = projects.file_library_mut().revision();
        let plugins_view = projects.file_library_mut().plugins_view();
        let key = format!(
            "{}\u{1}{:?}\u{1}{:?}\u{1}{:?}\u{1}{revision}\u{1}{}\u{1}{}\u{1}{:?}",
            self.search,
            self.category,
            self.place,
            self.tag_filter,
            self.generation,
            plugins_view.name(),
            self.plugin_sort
        );
        if self.rows_key.as_deref() == Some(key.as_str()) {
            return Ok(());
        }
        self.rows_key = Some(key);
        let flat = !self.search.trim().is_empty()
            || !self.tag_filter.is_empty()
            || matches!(self.place, Some(Place::Pinned | Place::Recent(_)));
        self.rows = if flat { self.flat_rows(projects)? } else { self.tree_rows(projects) };
        // What is no longer listed is no longer selected.
        let listed: HashSet<&str> = self.rows.iter().map(|r| r.item.key.as_str()).collect();
        let before = self.selected.len();
        self.selected.retain(|s| listed.contains(s.key.as_str()));
        if before != self.selected.len() {
            self.selection_changed = true;
        }
        Ok(())
    }

    fn flat_rows(&mut self, projects: &mut Projects) -> Result<Vec<Row>, String> {
        let place = self.place.as_ref().map(Place::param);
        let found =
            suhina_session::search_items(projects, &self.search, self.category, place.as_deref(), &self.tag_filter, 0)
                .map_err(String::from)?;
        Ok(found
            .into_iter()
            .map(|f| {
                let context = f.folder.as_ref().and_then(|folder| {
                    let path = match &f.item {
                        Item::File(p) => p.parent().map(Path::to_path_buf),
                        _ => None,
                    }?;
                    let below = path.strip_prefix(folder.parent().unwrap_or(folder)).ok()?;
                    Some(below.display().to_string())
                });
                let context = context.or_else(|| f.detail.clone());
                let item =
                    BrowserItem { key: f.item.key(), name: f.name.clone(), kind: item_kind(&f), plugin: f.plugin };
                Row { item, depth: 0, opens: false, open: false, context, columns: None, node: None }
            })
            .collect())
    }

    fn tree_rows(&mut self, projects: &mut Projects) -> Vec<Row> {
        let mut rows = Vec::new();
        let category = self.category;
        match self.place.clone() {
            Some(Place::Folder(folder)) => {
                let node = folder.display().to_string();
                self.folder_rows(projects, &folder, &node, 0, &mut rows);
            }
            _ => {
                let devices = matches!(category, Category::All | Category::Instruments | Category::Effects);
                if devices && category != Category::Effects {
                    self.device_rows("group:instruments", "Instruments", Role::Instrument, &mut rows);
                }
                if devices && category != Category::Instruments {
                    self.device_rows("group:effects", "Effects", Role::Effect, &mut rows);
                    self.device_rows("group:midi-effects", "MIDI Effects", Role::MidiEffect, &mut rows);
                }
                // The Plug-ins category lists the instruments or the effects as one list; the
                // other categories keep the plug-ins in the tree under their vendors.
                if category == Category::Plugins {
                    self.plugin_rows_listed(projects, &mut rows);
                } else if matches!(category, Category::All | Category::Instruments | Category::Effects) {
                    self.plugin_rows_grouped(projects, &mut rows);
                }
                if !category.file_kinds().is_empty() {
                    let folders: Vec<PathBuf> =
                        projects.file_library_mut().folders().into_iter().map(|f| f.path).collect();
                    for folder in folders {
                        let name = folder
                            .file_name()
                            .map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().into_owned());
                        let key = folder.display().to_string();
                        let open = self.open.contains(&key);
                        let item = BrowserItem { key: key.clone(), name, kind: ItemKind::Folder, plugin: false };
                        rows.push(Row { item, depth: 0, opens: true, open, context: None, columns: None, node: None });
                        if open {
                            self.folder_rows(projects, &folder, &key, 1, &mut rows);
                        }
                    }
                }
            }
        }
        rows
    }

    /// Adds the heading `name` for the built-in devices of `role` and, when it is open, a row per
    /// device under it.
    fn device_rows(&self, node: &str, name: &str, role: Role, rows: &mut Vec<Row>) {
        let open = self.open.contains(node);
        rows.push(Row {
            item: heading(node, name),
            depth: 0,
            opens: true,
            open,
            context: None,
            columns: None,
            node: None,
        });
        if !open {
            return;
        }
        let kind = match role {
            Role::Instrument => ItemKind::Instrument,
            Role::Effect => ItemKind::Effect,
            Role::MidiEffect => ItemKind::MidiEffect,
        };
        for info in suhina_device_registry::DEVICES.iter().filter(|info| info.role == role) {
            let item = BrowserItem { key: format!("device:{}", info.id), name: info.name.into(), kind, plugin: false };
            rows.push(Row { item, depth: 1, opens: false, open: false, context: None, columns: None, node: None });
        }
    }

    /// The plug-ins the category asks for (in the Plug-ins category, the instruments or the
    /// effects its switch chose), with their type. Plug-ins switched off are not among them.
    fn listed_plugins(&self, projects: &mut Projects) -> Vec<(suhina_plugins::scan::PluginInfo, bool)> {
        let view = projects.file_library_mut().plugins_view();
        let env = projects.plugin_env();
        let wanted = |instrument: bool| match self.category {
            Category::Instruments => instrument,
            Category::Effects => !instrument,
            Category::Plugins => instrument == (view == suhina_library::store::PluginsView::Instruments),
            _ => true,
        };
        env.catalog
            .plugins()
            .filter(|p| wanted(suhina_session::is_instrument(p)))
            .map(|p| (p.clone(), suhina_session::is_instrument(p)))
            .collect()
    }

    fn plugin_row(item: BrowserItem, depth: usize, columns: Option<PluginColumns>) -> Row {
        Row { item, depth, opens: false, open: false, context: None, columns, node: None }
    }

    fn plugin_rows_grouped(&self, projects: &mut Projects, rows: &mut Vec<Row>) {
        let mut vendors: Vec<(String, Vec<BrowserItem>)> = Vec::new();
        for (plugin, instrument) in self.listed_plugins(projects) {
            let vendor = if plugin.vendor.trim().is_empty() {
                "Unknown vendor".to_string()
            } else {
                plugin.vendor.trim().to_string()
            };
            let kind = if instrument { ItemKind::Instrument } else { ItemKind::Effect };
            let item =
                BrowserItem { key: format!("plugin:{}", plugin.spec()), name: plugin.name.clone(), kind, plugin: true };
            match vendors.iter_mut().find(|(v, _)| *v == vendor) {
                Some((_, list)) => list.push(item),
                None => vendors.push((vendor, vec![item])),
            }
        }
        vendors.sort_by(|a, b| {
            (a.0 == "Unknown vendor").cmp(&(b.0 == "Unknown vendor")).then(suhina_library::natural_cmp(&a.0, &b.0))
        });
        let node = "group:plugins";
        let open = self.open.contains(node);
        rows.push(Row {
            item: heading(node, "Plug-ins"),
            depth: 0,
            opens: true,
            open,
            context: None,
            columns: None,
            node: None,
        });
        if !open {
            return;
        }
        for (vendor, mut list) in vendors {
            let key = format!("vendor:{vendor}");
            let vendor_open = self.open.contains(&key);
            rows.push(Row {
                item: heading(&key, &vendor),
                depth: 1,
                opens: true,
                open: vendor_open,
                context: None,
                columns: None,
                node: None,
            });
            if vendor_open {
                list.sort_by(|a, b| suhina_library::natural_cmp(&a.name, &b.name));
                for item in list {
                    rows.push(Self::plugin_row(item, 2, None));
                }
            }
        }
    }

    /// The Plug-ins category's list: one row per plug-in, no headings, sorted by the chosen
    /// column (name in natural order, or format), the name breaking ties.
    fn plugin_rows_listed(&self, projects: &mut Projects, rows: &mut Vec<Row>) {
        let mut list: Vec<(BrowserItem, PluginColumns)> = self
            .listed_plugins(projects)
            .into_iter()
            .map(|(plugin, instrument)| {
                let kind = if instrument { ItemKind::Instrument } else { ItemKind::Effect };
                let item = BrowserItem {
                    key: format!("plugin:{}", plugin.spec()),
                    name: plugin.name.clone(),
                    kind,
                    plugin: true,
                };
                (item, PluginColumns { format: plugin.format() })
            })
            .collect();
        let (column, descending) = self.plugin_sort;
        list.sort_by(|a, b| {
            let order = match column {
                1 => a.1.format.cmp(b.1.format),
                _ => suhina_library::natural_cmp(&a.0.name, &b.0.name),
            };
            order.then_with(|| suhina_library::natural_cmp(&a.0.name, &b.0.name)).then_with(|| a.0.key.cmp(&b.0.key))
        });
        if descending {
            list.reverse();
        }
        for (item, columns) in list {
            rows.push(Self::plugin_row(item, 0, Some(columns)));
        }
    }

    /// Lists `folder`, whose node in the tree is `parent` ([`Row::node`]), and the folders open
    /// in it.
    fn folder_rows(&mut self, projects: &mut Projects, folder: &Path, parent: &str, depth: usize, rows: &mut Vec<Row>) {
        if depth > suhina_library::index::MAX_DEPTH {
            return;
        }
        let listing = match self.listings.get(folder) {
            Some(listing) => Arc::clone(listing),
            None => {
                let listing = Arc::new(
                    suhina_plugins::trace::timed("library folder listing", || projects.file_library_mut().list(folder))
                        .unwrap_or_default(),
                );
                self.listings.insert(folder.to_path_buf(), Arc::clone(&listing));
                listing
            }
        };
        let kinds = self.category.file_kinds();
        for entry in listing.iter() {
            let key = entry.path.display().to_string();
            let node = child_node(parent, &key);
            match entry.kind {
                None => {
                    // A folder the index shows holds nothing of the category is left out.
                    if self.category != Category::All
                        && projects.file_library_mut().holds(&entry.path, kinds) == Some(false)
                    {
                        continue;
                    }
                    let open = self.open.contains(&node);
                    let item = BrowserItem {
                        key: key.clone(),
                        name: entry.name.clone(),
                        kind: ItemKind::Folder,
                        plugin: false,
                    };
                    rows.push(Row {
                        item,
                        depth,
                        opens: true,
                        open,
                        context: None,
                        columns: None,
                        node: Some(node.clone()),
                    });
                    if open {
                        self.folder_rows(projects, &entry.path, &node, depth + 1, rows);
                    }
                }
                Some(kind) if kinds.contains(&kind) => {
                    let bank = kind == FileKind::Preset && self.preset_voices(&entry.path).len() > 1;
                    let open = bank && self.open.contains(&node);
                    let item = BrowserItem {
                        key: key.clone(),
                        name: entry.name.clone(),
                        kind: file_kind_item(kind),
                        plugin: false,
                    };
                    rows.push(Row {
                        item,
                        depth,
                        opens: bank,
                        open,
                        context: None,
                        columns: None,
                        node: Some(node.clone()),
                    });
                    if open {
                        for (voice, name) in self.preset_voices(&entry.path).iter() {
                            let item = BrowserItem {
                                key: format!("{key}#{voice}"),
                                name: format!("{voice}  {name}"),
                                kind: ItemKind::Preset,
                                plugin: false,
                            };
                            let node = child_node(&node, &item.key);
                            rows.push(Row {
                                item,
                                depth: depth + 1,
                                opens: false,
                                open: false,
                                context: None,
                                columns: None,
                                node: Some(node),
                            });
                        }
                    }
                }
                Some(_) => {}
            }
        }
    }

    /// The voices of a preset file (read once; at most 64 KiB).
    pub fn preset_voices(&mut self, path: &Path) -> Arc<Vec<(usize, String)>> {
        if let Some(voices) = self.voices.get(path) {
            return Arc::clone(voices);
        }
        let voices = std::fs::metadata(path)
            .ok()
            .filter(|m| m.len() <= suhina_device_registry::MAX_PRESET_BYTES as u64)
            .and_then(|_| std::fs::read(path).ok())
            .and_then(|data| suhina_device_registry::preset_voices(&data).ok())
            .unwrap_or_default();
        let voices = Arc::new(voices);
        self.voices.insert(path.to_path_buf(), Arc::clone(&voices));
        voices
    }

    /// What is known about a file; asks the reader for it when it is not known yet. `whole`
    /// asks for the decoded sample and its waveform too.
    pub fn info(&mut self, path: &Path, whole: bool) -> Option<&FileInfo> {
        let known =
            self.info.get(path).is_some_and(|i| !whole || i.data.is_some() || i.notes.is_some() || i.error.is_some());
        if !known && self.asked.insert((path.to_path_buf(), whole)) {
            let reader = self.reader.get_or_insert_with(Reader::start);
            if let Some(to) = &reader.to {
                let request =
                    if whole { Request::Whole(path.to_path_buf()) } else { Request::Header(path.to_path_buf()) };
                let _ = to.send(request);
            }
        }
        self.info.get(path)
    }

    /// Takes in what the reader has read. Returns whether anything arrived.
    pub fn poll(&mut self) -> bool {
        let Some(from) = self.reader.as_ref().and_then(|r| r.from.as_ref()) else { return false };
        let mut any = false;
        while let Ok(answer) = from.try_recv() {
            any = true;
            self.asked.remove(&(answer.path.clone(), answer.whole));
            if self.info.len() >= MAX_INFO {
                self.info.clear();
            }
            // Decoded audio is kept only for the selected file and the one waiting to preview (the
            // sample map previews what the pointer is over, which is not selected).
            let selected = self.selected.iter().any(|s| s.path().as_deref() == Some(answer.path.as_path()));
            let waiting = self.pending_preview.as_ref().is_some_and(|(p, _)| *p == answer.path);
            let mut info = answer.info;
            if !selected && !waiting {
                forget_audio(&mut info);
            }
            match self.info.get_mut(&answer.path) {
                Some(known) if !answer.whole => {
                    known.seconds = known.seconds.or(info.seconds);
                    known.key = known.key.clone().or(info.key);
                    known.tempo = known.tempo.or(info.tempo);
                }
                _ => {
                    self.info.insert(answer.path, info);
                }
            }
        }
        any
    }

    /// Drops the decoded audio of files no longer selected.
    pub fn forget_unselected(&mut self) {
        let selected: HashSet<PathBuf> = self.selected.iter().filter_map(BrowserItem::path).collect();
        for (path, info) in &mut self.info {
            if !selected.contains(path) {
                forget_audio(info);
            }
        }
    }

    /// Asks for `path`'s waveform (decoding it in the background), for the sample map's hover;
    /// `None` until it is read.
    pub fn waveform(&mut self, path: &Path) -> Option<Arc<Peaks>> {
        self.info(path, true).and_then(|i| i.peaks.clone())
    }

    /// Asks for `path` to preview from `from` seconds once it is decoded.
    pub fn want_preview(&mut self, path: PathBuf, from: f64) {
        self.pending_preview = Some((path, from));
    }

    /// A preview whose sample is decoded now, taken once.
    pub fn take_ready_preview(&mut self) -> Option<(PathBuf, f64, Arc<AudioData>)> {
        let (path, from) = self.pending_preview.clone()?;
        let info = self.info.get(&path)?;
        if let Some(e) = &info.error {
            self.preview_error = Some(format!("{}: {e}", path.display()));
            self.pending_preview = None;
            return None;
        }
        let data = Arc::clone(info.data.as_ref()?);
        self.pending_preview = None;
        Some((path, from, data))
    }

    /// Why the last sample asked to preview could not be read, once.
    pub fn take_preview_error(&mut self) -> Option<String> {
        self.preview_error.take()
    }

    pub fn cancel_preview(&mut self) {
        self.pending_preview = None;
        self.previewing = None;
    }

    /// Whether a sample waits to be decoded before it previews.
    pub fn preview_pending(&self) -> bool {
        self.pending_preview.is_some()
    }
}

/// Text for a length in seconds: `0:04.2`, `3:21`.
pub fn duration_text(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return String::new();
    }
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    if seconds < 60.0 { format!("0:{rest:04.1}") } else { format!("{}:{:02}", minutes as u64, rest.floor() as u64) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(duration_text(4.2), "0:04.2");
        assert_eq!(duration_text(0.06), "0:00.1");
        assert_eq!(duration_text(201.7), "3:21");
        assert_eq!(duration_text(f64::NAN), "");
    }

    /// A sample previewed without being selected (the sample map previews what the pointer is
    /// over) keeps its decoded audio until it plays.
    #[test]
    fn an_unselected_sample_waiting_to_preview_keeps_its_audio() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("suhina-browser-preview-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hat.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..800 {
            writer.write_sample(((i % 50) * 400) as i16).unwrap();
        }
        writer.finalize().unwrap();

        let mut state = BrowserState::default();
        assert!(state.selected().is_empty());
        let _ = state.info(&path, true);
        state.want_preview(path.clone(), 0.0);
        let mut ready = None;
        for _ in 0..400 {
            state.poll();
            ready = state.take_ready_preview();
            if ready.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let (got, from, data) = ready.expect("the preview's audio arrives");
        assert_eq!((got, from, data.frames()), (path, 0.0, 800));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A loop's audio arrives for its preview, and its tempo is detected and taken in too (it is
    /// sent after the audio, so the preview does not wait for it).
    #[test]
    fn a_previewed_loop_gets_its_tempo_too() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("suhina-browser-tempo-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("loop.wav");
        let rate = 44_100u32;
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // Four bars of clicks at 120 BPM: 8 seconds, a beat every half second.
        for i in 0..(rate * 8) {
            let since_beat = i % (rate / 2);
            let s = if since_beat < 400 { ((since_beat as f32 * 0.3).sin() * 20_000.0) as i16 } else { 0 };
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();

        let mut state = BrowserState::default();
        let started = std::time::Instant::now();
        let _ = state.info(&path, true);
        state.want_preview(path.clone(), 0.0);
        let mut ready_after = None;
        for _ in 0..2000 {
            state.poll();
            if ready_after.is_none() && state.take_ready_preview().is_some() {
                ready_after = Some(started.elapsed());
            }
            if ready_after.is_some() && state.info.get(&path).and_then(|i| i.tempo).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(ready_after.is_some(), "the preview's audio arrives");
        let detected = state.info.get(&path).and_then(|i| i.tempo).expect("the tempo follows");
        assert!((detected.bpm - 120.0).abs() < 1.0, "{}", detected.bpm);
        assert_eq!(detected.source, TempoSource::Detected);
        println!("audio ready after {:?}, tempo after {:?}", ready_after.unwrap_or_default(), started.elapsed());
        let _ = std::fs::remove_dir_all(dir);
    }
}
