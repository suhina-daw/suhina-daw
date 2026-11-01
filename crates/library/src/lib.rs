// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser's library (`docs/spec/browser.md`, ADR 0012): the folders the user added, an
//! index of the files in them kept on disk and up to date, and the pins, recent items and
//! colour tags. It belongs to Suhina, not to one project; the session's `library.*` commands
//! run on it.

pub mod find;
pub mod index;
pub mod store;
mod worker;

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

pub use suhina_media::info::FileKind;

pub use crate::index::FolderIndex;
use crate::store::Store;
pub use crate::store::{Group, MAX_FOLDERS, MAX_RECENT, MAX_TAG_LIMIT, Tag};
pub use crate::worker::{FolderState, POLL_EVERY, on_network_drive};
use crate::worker::{FromWorker, ToWorker};

/// Results a search gives at most.
pub const MAX_RESULTS: usize = 500;
/// Entries a folder listing gives at most.
pub const MAX_LISTING: usize = 10_000;
const STORE_FILE: &str = "browser.json";
const INDEX_FOLDER: &str = "library";

/// What a browser item is, from its key (`docs/spec/browser.md` › Terms): a file by its
/// absolute path, a voice of a preset bank as `<path>#<n>`, a built-in effect or instrument as
/// `device:<id>`, a plug-in as `plugin:<spec>`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Item {
    File(PathBuf),
    Voice(PathBuf, usize),
    Device(String),
    Plugin(String),
}

impl Item {
    pub fn parse(key: &str) -> Option<Self> {
        if key.is_empty() {
            return None;
        }
        if let Some(id) = key.strip_prefix("device:") {
            return (!id.is_empty()).then(|| Self::Device(id.to_string()));
        }
        if let Some(spec) = key.strip_prefix("plugin:") {
            return (!spec.is_empty()).then(|| Self::Plugin(spec.to_string()));
        }
        if let Some((path, voice)) = key.rsplit_once('#')
            && suhina_media::info::kind_of(Path::new(path)) == Some(FileKind::Preset)
            && let Ok(voice) = voice.parse::<usize>()
        {
            return (1..=32).contains(&voice).then(|| Self::Voice(PathBuf::from(path), voice));
        }
        Some(Self::File(PathBuf::from(key)))
    }

    pub fn key(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Voice(path, voice) => format!("{}#{voice}", path.display()),
            Self::Device(id) => format!("device:{id}"),
            Self::Plugin(spec) => format!("plugin:{spec}"),
        }
    }

    /// The kind of file it is, for a file or a preset voice.
    pub fn file_kind(&self) -> Option<FileKind> {
        match self {
            Self::File(path) => suhina_media::info::kind_of(path),
            Self::Voice(..) => Some(FileKind::Preset),
            _ => None,
        }
    }
}

/// A folder in the library and how it is kept up to date.
#[derive(Debug, Clone)]
pub struct FolderStatus {
    pub path: PathBuf,
    pub state: FolderState,
    pub files: usize,
    /// Why its index is not complete, when a limit was reached.
    pub note: Option<String>,
}

/// A file in the index of one of the folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub folder: PathBuf,
    /// Absolute.
    pub path: PathBuf,
    /// Below the folder, with `/` between parts.
    pub rel: String,
    pub kind: FileKind,
    pub size: u64,
    /// Nanoseconds since 1970.
    pub modified: u64,
}

/// Every file of `kind` (or of every kind) in an [`Library::index_snapshot`].
pub fn files_in(
    snapshot: &[(PathBuf, Arc<FolderIndex>)],
    kind: Option<FileKind>,
) -> impl Iterator<Item = IndexedFile> + '_ {
    snapshot.iter().flat_map(move |(folder, index)| {
        index.files.iter().filter(move |(_, e)| kind.is_none_or(|k| k == e.kind)).map(move |(rel, e)| IndexedFile {
            folder: folder.clone(),
            path: index::path_of(folder, rel),
            rel: rel.clone(),
            kind: e.kind,
            size: e.size,
            modified: e.modified,
        })
    })
}

impl IndexedFile {
    /// The file's name.
    pub fn name(&self) -> &str {
        self.rel.rsplit('/').next().unwrap_or(&self.rel)
    }
}

/// What is directly in a folder on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub path: PathBuf,
    pub name: String,
    /// `None` for a sub-folder.
    pub kind: Option<FileKind>,
    pub size: u64,
}

/// What to search for.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub text: String,
    /// Only files of these kinds (all kinds when empty).
    pub kinds: Vec<FileKind>,
    /// Only files in this folder (one of the library's, or a folder inside one).
    pub within: Option<PathBuf>,
    /// Only items carrying any of these tags.
    pub tags: Vec<String>,
    pub limit: usize,
}

/// A search taken from a [`Library`] as it is now, to run away from it: the window's thread must
/// not walk a large library at every key (`docs/spec/browser.md` › Results).
pub struct FileSearch {
    folders: Vec<(PathBuf, Arc<index::FolderIndex>)>,
    tagged: std::collections::BTreeMap<String, Vec<String>>,
    query: Query,
}

impl FileSearch {
    /// How many files it looks through, whatever matches.
    pub fn files(&self) -> usize {
        self.folders.iter().map(|(_, index)| index.files.len()).sum()
    }

    /// The files matching the query, best first; nothing once `cancel` is set, which it looks at
    /// as it goes, so a search the next key made stale ends soon.
    pub fn run(&self, cancel: &AtomicBool) -> Vec<IndexedFile> {
        let query = &self.query;
        let words = words(&query.text);
        let limit = if query.limit == 0 { MAX_RESULTS } else { query.limit.min(MAX_RESULTS) };
        let tagged = |key: &str| {
            query.tags.is_empty()
                || self.tagged.get(key).is_some_and(|names| names.iter().any(|n| query.tags.contains(n)))
        };
        let mut hits: Vec<(u8, IndexedFile)> = Vec::new();
        for (folder, index) in &self.folders {
            for (seen, (rel, entry)) in index.files.iter().enumerate() {
                if seen % 1024 == 0 && cancel.load(AtomicOrdering::Relaxed) {
                    return Vec::new();
                }
                if !query.kinds.is_empty() && !query.kinds.contains(&entry.kind) {
                    continue;
                }
                let name = rel.rsplit('/').next().unwrap_or(rel);
                let Some(rank) = rank(&words, name, rel) else { continue };
                let path = index::path_of(folder, rel);
                if query.within.as_ref().is_some_and(|w| !path.starts_with(w)) {
                    continue;
                }
                if !tagged(&path.display().to_string()) {
                    continue;
                }
                hits.push((
                    rank,
                    IndexedFile {
                        folder: folder.clone(),
                        path,
                        rel: rel.clone(),
                        kind: entry.kind,
                        size: entry.size,
                        modified: entry.modified,
                    },
                ));
            }
        }
        let order = |a: &(u8, IndexedFile), b: &(u8, IndexedFile)| {
            a.0.cmp(&b.0).then(a.1.name().len().cmp(&b.1.name().len())).then(natural_cmp(a.1.name(), b.1.name()))
        };
        // Only what is shown needs its order: a short word matches most of a large library.
        if limit > 0 && hits.len() > limit {
            hits.select_nth_unstable_by(limit - 1, order);
            hits.truncate(limit);
        }
        hits.sort_by(order);
        hits.into_iter().map(|(_, f)| f).collect()
    }
}

struct Folder {
    path: PathBuf,
    index: Arc<FolderIndex>,
    state: FolderState,
}

struct Running {
    to: Sender<ToWorker>,
    from: Receiver<FromWorker>,
    /// Requests sent and not answered yet.
    outstanding: usize,
}

pub struct Library {
    /// Suhina's settings folder; nothing is kept on disk when it is `None`.
    settings: Option<PathBuf>,
    loaded: bool,
    store: Store,
    warnings: Vec<String>,
    folders: Vec<Folder>,
    running: Option<Running>,
    revision: u64,
}

impl Default for Library {
    /// A library that is not kept anywhere (tests); the window and the MCP server give it
    /// Suhina's settings folder with [`Library::new`].
    fn default() -> Self {
        Self::new(None)
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        if let Some(running) = &self.running {
            let _ = running.to.send(ToWorker::Stop);
        }
    }
}

/// A short stable name for a folder's index file: FNV-1a of its path.
fn index_name(root: &Path) -> String {
    let hash = root
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    format!("index-{hash:016x}.tsv")
}

impl Library {
    /// The library kept in `settings` (`browser.json` there, indexes in `library/` below it).
    pub fn new(settings: Option<PathBuf>) -> Self {
        Self {
            settings,
            loaded: false,
            store: Store::default(),
            warnings: Vec::new(),
            folders: Vec::new(),
            running: None,
            revision: 0,
        }
    }

    /// Where the indexes are kept: `library/` in the settings folder.
    pub fn data_folder(&self) -> Option<PathBuf> {
        self.settings.as_ref().map(|s| s.join(INDEX_FOLDER))
    }

    fn index_file(&self, root: &Path) -> Option<PathBuf> {
        self.data_folder().map(|d| d.join(index_name(root)))
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(file) = self.settings.as_ref().map(|s| s.join(STORE_FILE)) {
            let (store, warnings) = Store::load(&file);
            self.store = store;
            self.warnings.extend(warnings);
        }
        for root in self.store.folders.clone() {
            let index = self.index_file(&root).and_then(|f| FolderIndex::load(&root, &f)).unwrap_or_default();
            let state = if index::dir_time(&root).is_some() { FolderState::Idle } else { FolderState::Missing };
            self.folders.push(Folder { path: root, index: Arc::new(index), state });
        }
    }

    fn save(&mut self) -> Result<(), String> {
        let Some(file) = self.settings.as_ref().map(|s| s.join(STORE_FILE)) else { return Ok(()) };
        self.store.save(&file).map_err(|e| format!("could not save the browser's library: {e}"))
    }

    /// What was skipped when `browser.json` was read; taken once.
    pub fn take_warnings(&mut self) -> Vec<String> {
        self.load();
        std::mem::take(&mut self.warnings)
    }

    /// Starts keeping the folders up to date on a thread of its own: each is checked against its
    /// saved index (or read for the first time), then watched or polled (ADR 0012). Until this is
    /// called, folders are only read when added or refreshed.
    pub fn start(&mut self) -> Result<(), String> {
        self.load();
        if self.running.is_some() {
            return Ok(());
        }
        let (to, inbox) = std::sync::mpsc::channel();
        let (out, from) = std::sync::mpsc::channel();
        worker::spawn(out, inbox, to.clone()).map_err(|e| format!("could not start the library's thread: {e}"))?;
        let mut running = Running { to, from, outstanding: 0 };
        for folder in &mut self.folders {
            let saved = (!folder.index.dirs.is_empty()).then(|| (*folder.index).clone());
            let file = self.settings.as_ref().map(|s| s.join(INDEX_FOLDER).join(index_name(&folder.path)));
            if running.to.send(ToWorker::Add { root: folder.path.clone(), saved, file }).is_ok() {
                running.outstanding += 1;
            }
            folder.state = FolderState::Scanning;
        }
        self.running = Some(running);
        Ok(())
    }

    /// Takes in what the background thread has done. Returns whether anything changed.
    pub fn poll(&mut self) -> bool {
        let Some(running) = self.running.as_mut() else { return false };
        let mut changed = false;
        loop {
            match running.from.try_recv() {
                Ok(FromWorker::Index(root, index)) => {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.path == root) {
                        folder.index = index;
                        changed = true;
                    }
                }
                Ok(FromWorker::State(root, state)) => {
                    if let Some(folder) = self.folders.iter_mut().find(|f| f.path == root)
                        && folder.state != state
                    {
                        folder.state = state;
                        changed = true;
                    }
                }
                Ok(FromWorker::Idle) => running.outstanding = running.outstanding.saturating_sub(1),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.running = None;
                    break;
                }
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        changed
    }

    /// Waits up to `timeout` until the background thread has done everything asked of it.
    pub fn wait_idle(&mut self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        loop {
            self.poll();
            if self.running.as_ref().is_none_or(|r| r.outstanding == 0) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Bumped whenever any folder's index or state changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn folders(&mut self) -> Vec<FolderStatus> {
        self.load();
        self.folders
            .iter()
            .map(|f| FolderStatus {
                path: f.path.clone(),
                state: f.state,
                files: f.index.files.len(),
                note: f.index.note.clone(),
            })
            .collect()
    }

    /// Adds a folder. It is read at once (on the background thread when the library is
    /// started).
    pub fn add_folder(&mut self, path: &Path) -> Result<PathBuf, String> {
        self.load();
        let path = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if index::dir_time(&path).is_none() {
            return Err(format!("{} is not a folder", path.display()));
        }
        if self.folders.iter().any(|f| f.path == path) {
            return Err(format!("{} is in the library already", path.display()));
        }
        if self.folders.len() >= MAX_FOLDERS {
            return Err(format!("the library holds at most {MAX_FOLDERS} folders"));
        }
        self.store.folders.push(path.clone());
        self.save()?;
        let file = self.index_file(&path);
        match self.running.as_mut() {
            Some(running) => {
                if running.to.send(ToWorker::Add { root: path.clone(), saved: None, file }).is_ok() {
                    running.outstanding += 1;
                }
                self.folders.push(Folder { path: path.clone(), index: Arc::default(), state: FolderState::Scanning });
            }
            None => {
                let index = FolderIndex::scan(&path);
                if let Some(file) = file {
                    let _ = index.save(&path, &file);
                }
                self.folders.push(Folder { path: path.clone(), index: Arc::new(index), state: FolderState::Idle });
            }
        }
        self.revision = self.revision.wrapping_add(1);
        Ok(path)
    }

    pub fn remove_folder(&mut self, path: &Path) -> Result<(), String> {
        self.load();
        let found = self
            .folders
            .iter()
            .position(|f| f.path == path || std::path::absolute(path).is_ok_and(|a| a == f.path))
            .ok_or_else(|| format!("{} is not in the library", path.display()))?;
        let folder = self.folders.remove(found);
        self.store.folders.retain(|f| *f != folder.path);
        if let Some(file) = self.index_file(&folder.path) {
            let _ = std::fs::remove_file(file);
        }
        if let Some(running) = self.running.as_mut()
            && running.to.send(ToWorker::Remove(folder.path.clone())).is_ok()
        {
            running.outstanding += 1;
        }
        self.revision = self.revision.wrapping_add(1);
        self.save()
    }

    /// Reads a folder (or every folder) again, whole.
    pub fn refresh(&mut self, path: Option<&Path>) -> Result<(), String> {
        self.load();
        let root = match path {
            Some(p) => Some(
                self.folders
                    .iter()
                    .find(|f| f.path == p || std::path::absolute(p).is_ok_and(|a| a == f.path))
                    .map(|f| f.path.clone())
                    .ok_or_else(|| format!("{} is not in the library", p.display()))?,
            ),
            None => None,
        };
        match self.running.as_mut() {
            Some(running) => {
                if running.to.send(ToWorker::Refresh(root)).is_ok() {
                    running.outstanding += 1;
                }
            }
            None => {
                let files: Vec<Option<PathBuf>> = self.folders.iter().map(|f| self.index_file(&f.path)).collect();
                for (folder, file) in self.folders.iter_mut().zip(files) {
                    if root.as_ref().is_some_and(|r| *r != folder.path) {
                        continue;
                    }
                    let index = FolderIndex::scan(&folder.path);
                    if let Some(file) = file {
                        let _ = index.save(&folder.path, &file);
                    }
                    folder.index = Arc::new(index);
                    folder.state =
                        if index::dir_time(&folder.path).is_some() { FolderState::Idle } else { FolderState::Missing };
                }
                self.revision = self.revision.wrapping_add(1);
            }
        }
        Ok(())
    }

    /// Each folder with its index as it is now, shared: to list its files on another thread
    /// ([`files_in`]) without copying them here.
    pub fn index_snapshot(&mut self) -> Vec<(PathBuf, Arc<FolderIndex>)> {
        self.load();
        self.folders.iter().map(|f| (f.path.clone(), Arc::clone(&f.index))).collect()
    }

    /// Every indexed file, of `kind` or of every kind.
    pub fn files(&mut self, kind: Option<FileKind>) -> impl Iterator<Item = IndexedFile> + '_ {
        self.load();
        self.folders.iter().flat_map(move |folder| {
            folder.index.files.iter().filter(move |(_, e)| kind.is_none_or(|k| k == e.kind)).map(move |(rel, e)| {
                IndexedFile {
                    folder: folder.path.clone(),
                    path: index::path_of(&folder.path, rel),
                    rel: rel.clone(),
                    kind: e.kind,
                    size: e.size,
                    modified: e.modified,
                }
            })
        })
    }

    /// Whether the index of the folders shows any file of `kinds` in or below `path` (for the
    /// category filters); `None` when `path` is not in an indexed folder.
    pub fn holds(&mut self, path: &Path, kinds: &[FileKind]) -> Option<bool> {
        self.load();
        let folder = self.folders.iter().find(|f| path.starts_with(&f.path))?;
        if folder.index.dirs.is_empty() {
            return None;
        }
        let rest = path.strip_prefix(&folder.path).ok()?;
        let rel = rest.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        let prefix = if rel.is_empty() { String::new() } else { format!("{rel}/") };
        Some(
            folder
                .index
                .files
                .range(prefix.clone()..)
                .take_while(|(k, _)| k.starts_with(&prefix))
                .any(|(_, e)| kinds.is_empty() || kinds.contains(&e.kind)),
        )
    }

    /// The files matching `query`, best first (`docs/spec/browser.md` › Results).
    pub fn search(&mut self, query: &Query) -> Vec<IndexedFile> {
        self.prepare_search(query).run(&AtomicBool::new(false))
    }

    /// The search for `query` over the index as it is now, for a thread of its own.
    pub fn prepare_search(&mut self, query: &Query) -> FileSearch {
        self.load();
        let folders = self.folders.iter().map(|f| (f.path.clone(), Arc::clone(&f.index))).collect();
        let tagged = if query.tags.is_empty() { Default::default() } else { self.store.tagged.clone() };
        FileSearch { folders, tagged, query: query.clone() }
    }

    /// What is directly in the folder `path` on disk: sub-folders first, then files of the
    /// kinds the browser lists, in natural order. Links are not followed.
    pub fn list(&self, path: &Path) -> Result<Vec<Listing>, String> {
        let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut out = Vec::new();
        for entry in entries.flatten() {
            if out.len() >= MAX_LISTING {
                break;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
            if name.is_empty() || name.len() > index::MAX_NAME || name.starts_with('.') {
                continue;
            }
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_dir() {
                out.push(Listing { path: entry.path(), name, kind: None, size: 0 });
            } else if file_type.is_file()
                && let Some(kind) = suhina_media::info::kind_of(Path::new(&name))
            {
                let size = entry.metadata().map_or(0, |m| m.len());
                out.push(Listing { path: entry.path(), name, kind: Some(kind), size });
            }
        }
        out.sort_by(|a, b| b.kind.is_none().cmp(&a.kind.is_none()).then(natural_cmp(&a.name, &b.name)));
        Ok(out)
    }

    /// The audio files outside projects' folders that opening a project may read.
    pub fn trusted_audio(&mut self) -> Vec<PathBuf> {
        self.load();
        self.store.trusted_audio.clone()
    }

    /// Keeps `files` (absolute paths) as trusted audio, newest first, and saves when that
    /// changed anything. Returns whether it did.
    pub fn trust_audio(&mut self, files: Vec<PathBuf>) -> Result<bool, String> {
        self.load();
        let trusted = &mut self.store.trusted_audio;
        let before = trusted.clone();
        for file in files.into_iter().filter(|f| f.is_absolute()) {
            trusted.retain(|t| *t != file);
            trusted.insert(0, file);
        }
        trusted.truncate(store::MAX_TRUSTED_AUDIO);
        if *trusted == before {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// The folders outside `project` (its project file) that its audio was loaded from.
    pub fn project_folders(&mut self, project: &Path) -> Vec<PathBuf> {
        self.load();
        self.store.project_folders.get(&project.display().to_string()).cloned().unwrap_or_default()
    }

    /// Adds `folders` (absolute paths) to `project`'s source folders, newest first, and saves
    /// when that changed anything. Returns whether it did.
    pub fn add_project_folders(&mut self, project: &Path, folders: Vec<PathBuf>) -> Result<bool, String> {
        self.load();
        let key = project.display().to_string();
        let mut list = self.store.project_folders.get(&key).cloned().unwrap_or_default();
        let before = list.clone();
        for folder in folders.into_iter().filter(|f| f.is_absolute()) {
            list.retain(|f| *f != folder);
            list.insert(0, folder);
        }
        list.truncate(store::MAX_PROJECT_FOLDERS);
        if list == before {
            return Ok(false);
        }
        if !self.store.project_folders.contains_key(&key)
            && self.store.project_folders.len() >= store::MAX_PROJECTS_WITH_FOLDERS
            && let Some(oldest) = self.store.project_folders.keys().next().cloned()
        {
            self.store.project_folders.remove(&oldest);
        }
        self.store.project_folders.insert(key, list);
        self.save()?;
        Ok(true)
    }

    pub fn pinned(&mut self) -> Vec<String> {
        self.load();
        self.store.pins.clone()
    }

    pub fn pin(&mut self, item: &str) -> Result<bool, String> {
        self.load();
        let changed = self.store.pin(item);
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    pub fn unpin(&mut self, item: &str) -> Result<bool, String> {
        self.load();
        let changed = self.store.unpin(item);
        if changed {
            self.save()?;
        }
        Ok(changed)
    }

    /// A file moved from `from` to `to` (#943): its pin, recent places and tags go with it, and
    /// the folders that held it and hold it now are read again, so the index follows.
    pub fn relocate(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        self.load();
        if self.store.relocate(&Item::File(from.to_path_buf()).key(), &Item::File(to.to_path_buf()).key()) {
            self.save()?;
        }
        let roots: Vec<PathBuf> = self
            .folders
            .iter()
            .filter(|f| from.starts_with(&f.path) || to.starts_with(&f.path))
            .map(|f| f.path.clone())
            .collect();
        for root in roots {
            self.refresh(Some(&root))?;
        }
        Ok(())
    }

    pub fn recent(&mut self, group: Group) -> Vec<String> {
        self.load();
        self.store.recent.get(&group).cloned().unwrap_or_default()
    }

    /// Puts `item` first among the recent ones of `group`.
    pub fn record(&mut self, group: Group, item: &str) -> Result<(), String> {
        self.load();
        self.store.record(group, item);
        self.save()
    }

    pub fn tags(&mut self) -> Vec<Tag> {
        self.load();
        self.store.tags.clone()
    }

    pub fn tag_limit(&mut self) -> usize {
        self.load();
        self.store.tag_limit
    }

    pub fn set_tag_limit(&mut self, limit: usize) -> Result<(), String> {
        self.load();
        self.store.set_tag_limit(limit)?;
        self.save()
    }

    pub fn set_tag(&mut self, name: &str, color: Option<&str>, rename: Option<&str>) -> Result<(), String> {
        self.load();
        self.store.set_tag(name, color, rename)?;
        self.save()
    }

    pub fn remove_tag(&mut self, name: &str) -> Result<(), String> {
        self.load();
        self.store.remove_tag(name)?;
        self.save()
    }

    pub fn tag(&mut self, item: &str, tag: &str, on: bool) -> Result<(), String> {
        self.load();
        self.store.tag(item, tag, on)?;
        self.save()
    }

    /// The names of the tags `item` carries.
    pub fn item_tags(&mut self, item: &str) -> Vec<String> {
        self.load();
        self.store.tagged.get(item).cloned().unwrap_or_default()
    }

    /// How the browser shows the installed plug-ins.
    pub fn plugins_view(&mut self) -> crate::store::PluginsView {
        self.load();
        self.store.plugins_view
    }

    /// Sets how the browser shows the installed plug-ins, kept for the next start.
    pub fn set_plugins_view(&mut self, view: crate::store::PluginsView) -> Result<(), String> {
        self.load();
        self.store.plugins_view = view;
        self.save()
    }

    /// Every tagged item with its tags.
    pub fn tagged(&mut self) -> Vec<(String, Vec<String>)> {
        self.load();
        self.store.tagged.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// How many items carry the tag `name`, counted where the tags are kept: no item key and no
    /// tag list is copied, so a hover over a tag's dot costs no allocation.
    pub fn tagged_count(&mut self, name: &str) -> usize {
        self.load();
        self.store.tagged.values().filter(|names| names.iter().any(|n| n == name)).count()
    }
}

/// The words of a search, lower-cased.
pub fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).filter(|w| !w.is_empty()).take(16).collect()
}

/// How well an item called `name`, at `path` below its folder, matches `words` (lower-cased):
/// `None` when a word is in neither; 0 for the exact name, 1 when the name starts with the
/// first word, 2 when a word in the name does, 3 otherwise. No words match everything at 3.
pub fn rank(words: &[String], name: &str, path: &str) -> Option<u8> {
    if words.is_empty() {
        return Some(3);
    }
    let name_lower = name.to_lowercase();
    let path_lower = path.to_lowercase();
    if !words.iter().all(|w| name_lower.contains(w.as_str()) || path_lower.contains(w.as_str())) {
        return None;
    }
    let stem = name_lower.rsplit_once('.').map_or(name_lower.as_str(), |(s, _)| s);
    let first = words.first().map_or("", String::as_str);
    if words.len() == 1 && (stem == first || name_lower == first) {
        return Some(0);
    }
    if name_lower.starts_with(first) {
        return Some(1);
    }
    let word_start =
        name_lower.split(|c: char| !c.is_alphanumeric()).any(|part| !part.is_empty() && part.starts_with(first));
    Some(if word_start { 2 } else { 3 })
}

/// Natural order: digits compare as numbers ("Kick 2" before "Kick 10"), letters without
/// regard to case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (m, n) = (take(&mut x), take(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(c), Some(d)) => {
                let order = c.to_lowercase().cmp(d.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests;
