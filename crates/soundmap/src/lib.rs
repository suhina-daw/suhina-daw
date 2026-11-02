// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sample map (docs/spec/sample-map.md, ADR 0013): features of samples, kinds from names,
//! the sound and the user, layouts of a scope by how its items sound, grids, similar items and
//! the feature cache.
//!
//! [`SoundMap`] keeps the caches of the library's folders and analyses new and changed files on
//! background threads; [`SoundMap::map`] lays out a scope from what is analysed.

pub mod cache;
pub mod classify;
pub mod features;
pub mod grid;
pub mod kinds;
pub mod layout;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use cache::{Analysis, Change, Entry, FolderCache};
use kinds::Kind;
use layout::Standardised;

/// A file larger than this is not analysed (spec › Limits).
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Items in one map (spec › Limits).
pub const MAX_ITEMS: usize = 200_000;
/// The default longest sample in the map, in seconds, and the range it may be set to.
pub const DEFAULT_MAX_SECONDS: f32 = 10.0;
pub const MAX_SECONDS_RANGE: (f32, f32) = (1.0, 60.0);
/// The most items `similar` returns.
pub const MAX_SIMILAR: usize = 100;
/// Analysed files gathered before a folder's cache is written again.
const SAVE_EVERY: usize = 256;
/// Layouts kept on disk; older ones are removed.
const KEPT_LAYOUTS: usize = 32;
/// The file of kinds set by hand, beside the caches, and the most it may hold.
const HAND_KINDS_FILE: &str = "kinds.txt";
const HAND_KINDS_HEADER: &str = "suhina-soundmap-kinds 1";
pub const MAX_HAND_KINDS: usize = 1_000_000;
const MAX_HAND_KINDS_BYTES: u64 = 256 * 1024 * 1024;

/// A sample file the library knows of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// The library folder it is in.
    pub folder: PathBuf,
    /// Its absolute path.
    pub path: PathBuf,
    pub size: u64,
    /// Modification time, in any unit, as long as it changes when the file does (the library
    /// gives nanoseconds since 1970).
    pub modified: u64,
}

/// Analysis progress of one folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderStatus {
    pub folder: PathBuf,
    pub analysed: usize,
    pub queued: usize,
    pub failed: usize,
}

/// One point of a map.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub path: PathBuf,
    /// Position in 0–1, cloud layout.
    pub position: [f32; 2],
    pub kind: Kind,
    /// The kind comes from the sound, not from a name or the user.
    pub guessed: bool,
    /// The kind was set by hand.
    pub by_hand: bool,
}

/// Files of a scope that could not be read for one reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// The reason, without what follows its first colon ("unsupported codec").
    pub reason: String,
    /// The whole reason given for the first of them ("unsupported codec: …").
    pub detail: String,
    pub count: usize,
    /// The first few of them.
    pub examples: Vec<PathBuf>,
}

/// How many files a [`Failure`] names.
const FAILURE_EXAMPLES: usize = 3;
/// How many reasons are told apart; the rest are counted under the last.
const FAILURE_REASONS: usize = 12;

/// Counts a failed file under its reason.
fn add_failure(failures: &mut Vec<Failure>, reason: &str, path: &Path) {
    let short = reason.split(':').next().unwrap_or(reason).trim();
    let short = if short.is_empty() { "unknown" } else { short };
    let index = match failures.iter().position(|f| f.reason == short) {
        Some(index) => index,
        None if failures.len() < FAILURE_REASONS => {
            failures.push(Failure {
                reason: short.to_string(),
                detail: reason.to_string(),
                count: 0,
                examples: Vec::new(),
            });
            failures.len() - 1
        }
        None => failures.len() - 1,
    };
    if let Some(failure) = failures.get_mut(index) {
        failure.count += 1;
        if failure.examples.len() < FAILURE_EXAMPLES {
            failure.examples.push(path.to_path_buf());
        }
    }
}

/// A scope laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Map {
    pub points: Vec<Point>,
    /// Items of the scope not analysed yet.
    pub pending: usize,
    /// Items left out because they are longer than the length limit.
    pub too_long: usize,
    /// Items that could not be read.
    pub failed: usize,
    /// Why they could not be read, the commonest reason first.
    pub failures: Vec<Failure>,
    /// Items left out over [`MAX_ITEMS`].
    pub over_limit: usize,
    key: u64,
    standardised: Standardised,
}

impl Map {
    /// The `count` items most similar to `path` by features, nearest first, with distances.
    pub fn similar(&self, path: &Path, count: usize) -> Vec<(PathBuf, f32)> {
        let Some(index) = self.points.iter().position(|p| p.path == path) else { return Vec::new() };
        self.standardised
            .similar(index, count.min(MAX_SIMILAR))
            .into_iter()
            .filter_map(|(i, d)| Some((self.points.get(i)?.path.clone(), d)))
            .collect()
    }

    /// The grid version of the layout, for a view of the given aspect (width / height).
    pub fn grid(&self, aspect: f32) -> grid::Grid {
        let positions: Vec<[f32; 2]> = self.points.iter().map(|p| p.position).collect();
        grid::snap(&positions, aspect)
    }
}

type Job = (PathBuf, SourceFile);
type Done = (SourceFile, Analysis);

struct Queue {
    jobs: Mutex<(VecDeque<Job>, bool)>,
    ready: Condvar,
}

/// A folder's cache to write whole, or changes to append to its journal, and where.
enum CacheSave {
    Whole(PathBuf, Arc<FolderCache>),
    Changes(PathBuf, Vec<Change>),
}

/// The analysis of a library's samples, cached per folder.
pub struct SoundMap {
    data_folder: Option<PathBuf>,
    /// Shared, so an update worked out on a thread of its own reads them without a copy.
    caches: HashMap<PathBuf, Arc<FolderCache>>,
    /// Files queued or being analysed, by absolute path.
    queued: HashSet<PathBuf>,
    /// The changes of each folder's cache not written yet, and whether it must be written whole.
    unsaved: HashMap<PathBuf, (Vec<Change>, bool)>,
    queue: Arc<Queue>,
    results: Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
    sender: Sender<Done>,
    threads: usize,
    /// Layouts by the hash of their items, so a scope opens with the map it had.
    layouts: HashMap<u64, Vec<[f32; 2]>>,
    /// Kinds set by hand, by absolute path; read from the data folder when first needed.
    hand_kinds: Option<HashMap<PathBuf, Kind>>,
    /// The thread that writes caches to disk, started with the first save: a cache written on
    /// the window's thread (serialised, written and synced) held it up for half a second.
    saver: Option<(Sender<CacheSave>, JoinHandle<()>)>,
    /// Whether updates are worked out on a thread of their own ([`SoundMap::read_in_background`]).
    background_reads: bool,
    /// An update being worked out there ([`SoundMap::update_in_background`]).
    diffing: Option<Receiver<Diff>>,
}

/// An update worked out on a thread of its own: the caches read from disk for folders not
/// read yet, the files to analyse, each folder's files that are gone, and the folders there are.
struct Diff {
    loaded: Vec<(PathBuf, FolderCache)>,
    work: Vec<Job>,
    gone: Vec<(PathBuf, Vec<PathBuf>)>,
    folders: HashSet<PathBuf>,
}

impl SoundMap {
    /// A sound map keeping its caches in `data_folder` (none: in memory only). Analysis
    /// threads start with the first [`SoundMap::update`] that finds work, at most `threads`.
    pub fn new(data_folder: Option<PathBuf>, threads: usize) -> Self {
        let (sender, results) = channel();
        SoundMap {
            data_folder,
            caches: HashMap::new(),
            queued: HashSet::new(),
            unsaved: HashMap::new(),
            queue: Arc::new(Queue { jobs: Mutex::new((VecDeque::new(), false)), ready: Condvar::new() }),
            results,
            workers: Vec::new(),
            sender,
            threads: threads.max(1),
            layouts: HashMap::new(),
            hand_kinds: None,
            saver: None,
            background_reads: false,
            diffing: None,
        }
    }

    /// Works out updates on a thread of their own ([`SoundMap::update_in_background`]): for the
    /// window, which a large library held up for a second. Off, the caller updates at once
    /// (tests, the CLI).
    pub fn read_in_background(&mut self, on: bool) {
        self.background_reads = on;
    }

    /// Whether updates are worked out on a thread of their own.
    pub fn reads_in_background(&self) -> bool {
        self.background_reads
    }

    /// Half the cores, at least one (ADR 0013 › Threads).
    pub fn default_threads() -> usize {
        std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1))
    }

    /// A folder's cache to change: read first when it is not yet; copied only while an update
    /// on another thread still reads the one shared.
    fn cache(&mut self, folder: &Path) -> &mut FolderCache {
        Arc::make_mut(self.shared_cache(folder))
    }

    /// A folder's cache to read: read first when it is not yet.
    fn cache_ref(&mut self, folder: &Path) -> &FolderCache {
        self.shared_cache(folder)
    }

    fn shared_cache(&mut self, folder: &Path) -> &mut Arc<FolderCache> {
        let data_folder = self.data_folder.clone();
        self.caches.entry(folder.to_path_buf()).or_insert_with(|| {
            Arc::new(match &data_folder {
                Some(dir) => FolderCache::load(&dir.join(cache::file_name(folder))),
                None => FolderCache::default(),
            })
        })
    }

    /// Whether an update from [`SoundMap::update_in_background`] is still being worked out, so
    /// another would be refused: a caller can skip gathering its files meanwhile.
    pub fn updating(&self) -> bool {
        self.diffing.is_some()
    }

    /// Works out an update like [`SoundMap::update`] on a thread of its own, from the files
    /// `files` lists there, and takes it in at a later [`SoundMap::poll`]: the library's files
    /// listed, the caches read and compared off the caller's thread. Returns `false` while the
    /// one before is still being worked out (asked again later).
    pub fn update_in_background(&mut self, files: impl FnOnce() -> Vec<SourceFile> + Send + 'static) -> bool {
        if self.diffing.is_some() {
            return false;
        }
        let shared: HashMap<PathBuf, Arc<FolderCache>> = self.caches.clone();
        let queued = self.queued.clone();
        let data_folder = self.data_folder.clone();
        let (to, from) = channel();
        let spawned = std::thread::Builder::new().name("sound map update".into()).spawn(move || {
            let files = files();
            let mut loaded: HashMap<PathBuf, FolderCache> = HashMap::new();
            let mut present: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
            let mut work = Vec::new();
            for file in &files {
                let relative = relative(&file.path, &file.folder);
                present.entry(file.folder.clone()).or_default().insert(relative.clone());
                if !shared.contains_key(&file.folder) && !loaded.contains_key(&file.folder) {
                    let cache = match &data_folder {
                        Some(dir) => FolderCache::load(&dir.join(cache::file_name(&file.folder))),
                        None => FolderCache::default(),
                    };
                    loaded.insert(file.folder.clone(), cache);
                }
                let cache = shared.get(&file.folder).map(|c| &**c).or_else(|| loaded.get(&file.folder));
                let current = cache.is_some_and(|c| c.current(&relative, file.size, file.modified).is_some());
                if !current && !queued.contains(&file.path) {
                    work.push((relative, file.clone()));
                }
            }
            let caches = shared.iter().map(|(f, c)| (f, &**c)).chain(loaded.iter());
            let gone = caches
                .filter_map(|(folder, cache)| {
                    let keep = present.get(folder);
                    let gone: Vec<PathBuf> =
                        cache.entries.keys().filter(|p| !keep.is_some_and(|k| k.contains(*p))).cloned().collect();
                    (!gone.is_empty()).then(|| (folder.clone(), gone))
                })
                .collect();
            let folders = present.into_keys().collect();
            let _ = to.send(Diff { loaded: loaded.into_iter().collect(), work, gone, folders });
        });
        if spawned.is_ok() {
            self.diffing = Some(from);
        }
        spawned.is_ok()
    }

    /// Takes in an update worked out on another thread, once it is done. Returns whether one
    /// came in.
    fn take_diff(&mut self, wait: bool) -> bool {
        let Some(from) = &self.diffing else { return false };
        // One read: a second `try_recv` could take (and drop) an update sent just after the first.
        let diff = if wait {
            from.recv().ok()
        } else {
            match from.try_recv() {
                Ok(diff) => Some(diff),
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                // The thread ended without sending (it failed).
                Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
            }
        };
        let Some(diff) = diff else {
            self.diffing = None;
            return false;
        };
        self.diffing = None;
        let Diff { loaded, work, gone, folders } = diff;
        for (folder, cache) in loaded {
            // Read here meanwhile (a status asked for): that one stands.
            self.caches.entry(folder).or_insert_with(|| Arc::new(cache));
        }
        for (folder, paths) in gone {
            let Some(cache) = self.caches.get_mut(&folder) else { continue };
            let cache = Arc::make_mut(cache);
            for path in paths {
                if cache.entries.remove(&path).is_some() {
                    self.unsaved.entry(folder.clone()).or_default().0.push(Change::Remove(path));
                }
            }
        }
        self.caches.retain(|folder, _| folders.contains(folder));
        self.unsaved.retain(|folder, _| folders.contains(folder));
        let work: Vec<Job> = work.into_iter().filter(|(_, file)| !self.queued.contains(&file.path)).collect();
        if !work.is_empty() {
            self.enqueue(work);
        }
        true
    }

    /// Brings the caches in line with the library's files: queues new and changed files, and
    /// forgets files and folders that are gone.
    pub fn update(&mut self, files: &[SourceFile]) {
        let mut present: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
        let mut work = Vec::new();
        for file in files {
            let relative = relative(&file.path, &file.folder);
            present.entry(file.folder.clone()).or_default().insert(relative.clone());
            let current = self.cache_ref(&file.folder).current(&relative, file.size, file.modified).is_some();
            if !current && !self.queued.contains(&file.path) {
                work.push((relative, file.clone()));
            }
        }
        for (folder, cache) in self.caches.iter_mut() {
            let keep = present.get(folder);
            let gone: Vec<PathBuf> =
                cache.entries.keys().filter(|p| !keep.is_some_and(|k| k.contains(*p))).cloned().collect();
            if gone.is_empty() {
                continue;
            }
            let cache = Arc::make_mut(cache);
            for path in gone {
                cache.entries.remove(&path);
                self.unsaved.entry(folder.clone()).or_default().0.push(Change::Remove(path));
            }
        }
        self.caches.retain(|folder, _| present.contains_key(folder));
        self.unsaved.retain(|folder, _| present.contains_key(folder));
        if !work.is_empty() {
            self.enqueue(work);
        }
        self.save_due(true);
    }

    /// Analyses `folder` again, ignoring its cache.
    pub fn reanalyse(&mut self, folder: &Path, files: &[SourceFile]) {
        if let Some(cache) = self.caches.get_mut(folder) {
            Arc::make_mut(cache).entries.clear();
            self.unsaved.entry(folder.to_path_buf()).or_default().1 = true;
        }
        let work: Vec<Job> = files
            .iter()
            .filter(|f| f.folder == folder && !self.queued.contains(&f.path))
            .map(|f| (relative(&f.path, &f.folder), f.clone()))
            .collect();
        self.enqueue(work);
    }

    fn enqueue(&mut self, work: Vec<Job>) {
        for (_, file) in &work {
            self.queued.insert(file.path.clone());
        }
        if let Ok(mut jobs) = self.queue.jobs.lock() {
            jobs.0.extend(work);
        }
        self.queue.ready.notify_all();
        while self.workers.len() < self.threads {
            let queue = Arc::clone(&self.queue);
            let sender = self.sender.clone();
            match std::thread::Builder::new().name("soundmap-analysis".into()).spawn(move || work_loop(&queue, &sender))
            {
                Ok(handle) => self.workers.push(handle),
                Err(_) => break,
            }
        }
    }

    /// Takes in finished analyses and writes caches that have gathered enough. Returns whether
    /// anything changed. Called often (each frame of the UI, or before a command answers).
    pub fn poll(&mut self) -> bool {
        let mut changed = self.take_diff(false);
        while let Ok((file, analysis)) = self.results.try_recv() {
            self.queued.remove(&file.path);
            let relative = relative(&file.path, &file.folder);
            let folder = file.folder.clone();
            let entry = Entry { size: file.size, modified: file.modified, analysis };
            self.cache(&folder).entries.insert(relative.clone(), entry.clone());
            self.unsaved.entry(folder).or_default().0.push(Change::Put(relative, entry));
            changed = true;
        }
        self.save_due(self.queued.is_empty());
        changed
    }

    /// Waits until nothing is queued (for tests and the CLI).
    pub fn wait(&mut self) {
        self.take_diff(true);
        while !self.queued.is_empty() {
            match self.results.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(done) => {
                    let (file, analysis) = done;
                    self.queued.remove(&file.path);
                    let relative = relative(&file.path, &file.folder);
                    let folder = file.folder.clone();
                    let entry = Entry { size: file.size, modified: file.modified, analysis };
                    self.cache(&folder).entries.insert(relative.clone(), entry.clone());
                    self.unsaved.entry(folder).or_default().0.push(Change::Put(relative, entry));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        self.save_due(true);
        self.finish_saves();
    }

    /// Waits until every cache handed to the saving thread is on disk.
    fn finish_saves(&mut self) {
        if let Some((to, thread)) = self.saver.take() {
            drop(to);
            let _ = thread.join();
        }
    }

    fn save_due(&mut self, all: bool) {
        let Some(dir) = self.data_folder.clone() else {
            self.unsaved.clear();
            return;
        };
        let due: Vec<PathBuf> = self
            .unsaved
            .iter()
            .filter(|(_, (changes, whole))| *whole || (!changes.is_empty() && (all || changes.len() >= SAVE_EVERY)))
            .map(|(f, _)| f.clone())
            .collect();
        if !due.is_empty() && std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        if !due.is_empty() && self.saver.is_none() {
            // Written on a thread of its own, in order, so a later save of a folder lands last.
            let (to, from) = channel::<CacheSave>();
            let thread = std::thread::Builder::new().name("sound map saves".into()).spawn(move || {
                for save in from {
                    let _ = match save {
                        CacheSave::Whole(path, cache) => cache.save(&path),
                        CacheSave::Changes(path, changes) => cache::append(&path, &changes),
                    };
                }
            });
            if let Ok(thread) = thread {
                self.saver = Some((to, thread));
            }
        }
        for folder in due {
            let Some((changes, whole)) = self.unsaved.remove(&folder) else { continue };
            let Some(shared) = self.caches.get_mut(&folder) else { continue };
            let path = dir.join(cache::file_name(&folder));
            // Appended in a few bytes; the file is written whole again only when its journal has
            // grown past a quarter of its entries (or it was emptied).
            let journal = shared.journal + changes.len();
            let save = if whole || journal > shared.entries.len() / 4 + 64 {
                Arc::make_mut(shared).journal = 0;
                // Shared with the saving thread, not copied.
                CacheSave::Whole(path, Arc::clone(shared))
            } else {
                Arc::make_mut(shared).journal = journal;
                CacheSave::Changes(path, changes)
            };
            let unsent = match &self.saver {
                Some((to, _)) => to.send(save).err().map(|e| e.0),
                None => Some(save),
            };
            // No saving thread: written here.
            let _ = match unsent {
                Some(CacheSave::Whole(path, cache)) => cache.save(&path),
                Some(CacheSave::Changes(path, changes)) => cache::append(&path, &changes),
                None => Ok(()),
            };
        }
    }

    /// Progress per folder, for the files given.
    pub fn status(&mut self, files: &[SourceFile]) -> Vec<FolderStatus> {
        let mut out: Vec<FolderStatus> = Vec::new();
        for file in files {
            let relative = relative(&file.path, &file.folder);
            let queued = self.queued.contains(&file.path);
            let analysis = self.cache_ref(&file.folder).current(&relative, file.size, file.modified).cloned();
            let index = match out.iter().position(|s| s.folder == file.folder) {
                Some(i) => i,
                None => {
                    out.push(FolderStatus { folder: file.folder.clone(), ..Default::default() });
                    out.len() - 1
                }
            };
            let Some(status) = out.get_mut(index) else { continue };
            match analysis {
                _ if queued => status.queued += 1,
                Some(Analysis::Analysed { .. }) => status.analysed += 1,
                Some(Analysis::Failed(_)) => status.failed += 1,
                None => status.queued += 1,
            }
        }
        out
    }

    /// The analysis of one file, when it is current.
    pub fn analysis(&mut self, file: &SourceFile) -> Option<Analysis> {
        let relative = relative(&file.path, &file.folder);
        self.cache_ref(&file.folder).current(&relative, file.size, file.modified).cloned()
    }

    /// Lays out a scope: the analysed files among `files` no longer than `max_seconds`. A new
    /// layout is computed here (seconds for tens of thousands of items); the window uses
    /// [`SoundMap::prepare`] and [`Prepared::compute`] to compute it off its thread.
    pub fn map(&mut self, files: &[SourceFile], max_seconds: f32) -> Map {
        let map = self.prepare(files, max_seconds).compute();
        self.keep(&map);
        map
    }

    /// What a scope's layout needs, taken from the caches: quick, on the caller's thread. The
    /// layout itself is [`Prepared::compute`], which may run anywhere; [`SoundMap::keep`] then
    /// keeps a new one.
    pub fn prepare(&mut self, files: &[SourceFile], max_seconds: f32) -> Prepared {
        let max_seconds = max_seconds.clamp(MAX_SECONDS_RANGE.0, MAX_SECONDS_RANGE.1);
        let mut chosen: Vec<(&SourceFile, features::Features, Option<Kind>)> = Vec::new();
        let (mut pending, mut too_long, mut failed, mut over_limit) = (0, 0, 0, 0);
        let mut failures: Vec<Failure> = Vec::new();
        let mut sorted: Vec<&SourceFile> = files.iter().collect();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        sorted.dedup_by(|a, b| a.path == b.path);
        for file in sorted {
            let relative = relative(&file.path, &file.folder);
            match self.cache_ref(&file.folder).current(&relative, file.size, file.modified) {
                Some(Analysis::Analysed { seconds, .. }) if *seconds > max_seconds => too_long += 1,
                Some(Analysis::Analysed { features, .. }) => {
                    if chosen.len() < MAX_ITEMS {
                        chosen.push((file, *features, None));
                    } else {
                        over_limit += 1;
                    }
                }
                Some(Analysis::Failed(reason)) => {
                    failed += 1;
                    add_failure(&mut failures, reason, &file.path);
                }
                None => pending += 1,
            }
        }
        let hand = self.hand_kinds();
        for (file, _, kind) in chosen.iter_mut() {
            *kind = hand.get(&file.path).copied();
        }
        let key = layout_key(chosen.iter().map(|(f, _, kind)| (*f, *kind)));
        let cached = self.cached_layout(key, chosen.len());
        failures.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.reason.cmp(&b.reason)));
        Prepared {
            items: chosen
                .into_iter()
                .map(|(f, features, kind)| Item {
                    path: f.path.clone(),
                    folder: f.folder.clone(),
                    features,
                    hand: kind,
                })
                .collect(),
            pending,
            too_long,
            failed,
            failures,
            over_limit,
            key,
            cached,
        }
    }

    /// Keeps a map's layout, so its scope opens with it next time.
    pub fn keep(&mut self, map: &Map) {
        if self.layouts.get(&map.key).is_none_or(|p| p.len() != map.points.len()) {
            let positions: Vec<[f32; 2]> = map.points.iter().map(|p| p.position).collect();
            self.keep_layout(map.key, &positions);
        }
    }
}

/// One item of a scope to lay out.
#[derive(Debug, Clone)]
struct Item {
    path: PathBuf,
    /// The library folder it is in.
    folder: PathBuf,
    features: features::Features,
    /// Its kind set by hand.
    hand: Option<Kind>,
}

/// A scope ready to be laid out.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The items, by path.
    items: Vec<Item>,
    pending: usize,
    too_long: usize,
    failed: usize,
    failures: Vec<Failure>,
    over_limit: usize,
    key: u64,
    cached: Option<Vec<[f32; 2]>>,
}

impl Prepared {
    /// Whether the layout is known already, so [`Prepared::compute`] is quick.
    pub fn is_laid_out(&self) -> bool {
        self.cached.is_some() || self.items.len() < 2
    }

    /// The scope's key: the same items with the same sizes and times give the same key.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// Finds the kinds, then lays the scope out by sound and kind (or takes the kept layout).
    pub fn compute(self) -> Map {
        self.compute_with_progress(&layout::Progress::new())
    }

    /// [`Prepared::compute`], counting how far the layout is in `progress`.
    pub fn compute_with_progress(self, progress: &layout::Progress) -> Map {
        let features: Vec<features::Features> = self.items.iter().map(|item| item.features).collect();
        let standardised = Standardised::new(&features);
        let evidence: Vec<Option<(Kind, classify::Evidence)>> = self
            .items
            .iter()
            .map(|item| match item.hand {
                Some(kind) => Some((kind, classify::Evidence::Hand)),
                None => classify::named(&item.path, &item.folder),
            })
            .collect();
        let kinds = classify::classify(standardised.rows(), &evidence);
        let positions = match self.cached {
            Some(positions) => positions,
            None => {
                let by_kind: Vec<Kind> = kinds.iter().map(|(kind, _)| *kind).collect();
                let input = Standardised::from_rows(classify::layout_rows(standardised.rows(), &by_kind));
                layout::layout_with_progress(
                    &input,
                    scope_seed(self.items.iter().map(|item| item.path.as_path())),
                    progress,
                )
            }
        };
        let points = self
            .items
            .into_iter()
            .zip(positions)
            .zip(kinds)
            .map(|((item, position), (kind, from))| Point {
                path: item.path,
                position,
                kind,
                guessed: from == classify::From::Sound,
                by_hand: from == classify::From::Hand,
            })
            .collect();
        Map {
            points,
            pending: self.pending,
            too_long: self.too_long,
            failed: self.failed,
            failures: self.failures,
            over_limit: self.over_limit,
            key: self.key,
            standardised,
        }
    }
}

impl SoundMap {
    fn cached_layout(&mut self, key: u64, count: usize) -> Option<Vec<[f32; 2]>> {
        if let Some(positions) = self.layouts.get(&key).filter(|p| p.len() == count) {
            return Some(positions.clone());
        }
        let path = self.data_folder.as_ref()?.join(layout_file_name(key));
        let bytes = std::fs::read(path).ok()?;
        let positions = layout_from_bytes(&bytes).filter(|p| p.len() == count)?;
        self.layouts.insert(key, positions.clone());
        Some(positions)
    }

    fn keep_layout(&mut self, key: u64, positions: &[[f32; 2]]) {
        self.layouts.insert(key, positions.to_vec());
        let Some(dir) = self.data_folder.clone() else { return };
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(layout_file_name(key));
        let _unkept = suhina_fs::write_atomic(&path, layout_to_bytes(positions));
        // Keep only the newest layouts.
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        let mut layouts: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with("layout-"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        layouts.sort_by_key(|l| std::cmp::Reverse(l.0));
        for (_, old) in layouts.into_iter().skip(KEPT_LAYOUTS) {
            let _ = std::fs::remove_file(old);
        }
    }
}

const LAYOUT_MAGIC: &[u8; 4] = b"LWSL";

fn layout_file_name(key: u64) -> String {
    format!("layout-{key:016x}.bin")
}

fn layout_to_bytes(positions: &[[f32; 2]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + positions.len() * 8);
    out.extend_from_slice(LAYOUT_MAGIC);
    out.extend_from_slice(&(positions.len() as u32).to_le_bytes());
    for p in positions {
        out.extend_from_slice(&p[0].to_le_bytes());
        out.extend_from_slice(&p[1].to_le_bytes());
    }
    out
}

fn layout_from_bytes(bytes: &[u8]) -> Option<Vec<[f32; 2]>> {
    let count = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    if bytes.get(..4)? != LAYOUT_MAGIC || count > MAX_ITEMS || bytes.len() != 8 + count * 8 {
        return None;
    }
    let positions: Vec<[f32; 2]> = bytes
        .get(8..)?
        .as_chunks::<8>()
        .0
        .iter()
        .map(|[a, b, c, d, e, f, g, h]| [f32::from_le_bytes([*a, *b, *c, *d]), f32::from_le_bytes([*e, *f, *g, *h])])
        .collect();
    positions.iter().all(|p| p.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))).then_some(positions)
}

/// The layout cache's key: the items with their sizes, times and kinds set by hand, and the
/// feature version.
fn layout_key<'a>(files: impl Iterator<Item = (&'a SourceFile, Option<Kind>)>) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ u64::from(features::FEATURE_VERSION);
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for (file, hand) in files {
        eat(file.path.to_string_lossy().as_bytes());
        eat(&file.size.to_le_bytes());
        eat(&file.modified.to_le_bytes());
        if let Some(kind) = hand {
            eat(b"\0kind:");
            eat(kind.id().as_bytes());
        }
    }
    hash
}

impl SoundMap {
    /// The kinds set by hand, read from the data folder the first time.
    fn hand_kinds(&mut self) -> &HashMap<PathBuf, Kind> {
        let data_folder = self.data_folder.clone();
        self.hand_kinds.get_or_insert_with(|| match data_folder {
            Some(dir) => read_hand_kinds(&dir.join(HAND_KINDS_FILE)),
            None => HashMap::new(),
        })
    }

    /// The kind set by hand for a file, if any.
    pub fn kind_by_hand(&mut self, path: &Path) -> Option<Kind> {
        self.hand_kinds().get(path).copied()
    }

    /// Sets a file's kind by hand (`None`: its name and sound decide again), and keeps it in
    /// the data folder. It wins over the name and the sound, and survives analysing the file
    /// again and new layouts; the next layout of a scope that holds the file puts it with its
    /// kind.
    pub fn set_kind(&mut self, path: &Path, kind: Option<Kind>) -> Result<(), String> {
        let text = path.to_str().ok_or("the path is not valid text")?;
        if text.contains(['\n', '\r']) {
            return Err("the path holds a line break".into());
        }
        self.hand_kinds();
        let kinds = self.hand_kinds.get_or_insert_with(HashMap::new);
        match kind {
            Some(kind) => {
                if kinds.len() >= MAX_HAND_KINDS && !kinds.contains_key(path) {
                    return Err(format!("at most {MAX_HAND_KINDS} kinds can be set by hand"));
                }
                kinds.insert(path.to_path_buf(), kind);
            }
            None => {
                kinds.remove(path);
            }
        }
        let Some(dir) = self.data_folder.clone() else { return Ok(()) };
        std::fs::create_dir_all(&dir).map_err(|e| format!("could not keep the kind: {e}"))?;
        let file = dir.join(HAND_KINDS_FILE);
        suhina_fs::write_atomic(&file, hand_kinds_to_text(kinds)).map_err(|e| format!("could not keep the kind: {e}"))
    }
}

/// The file of kinds set by hand: a header line, then a line per file, its kind's id and its
/// path, split by a tab, sorted by path.
fn hand_kinds_to_text(kinds: &HashMap<PathBuf, Kind>) -> String {
    let mut lines: Vec<(&str, Kind)> = kinds.iter().filter_map(|(p, k)| Some((p.to_str()?, *k))).collect();
    lines.sort();
    let mut out = String::from(HAND_KINDS_HEADER);
    out.push('\n');
    for (path, kind) in lines {
        out.push_str(kind.id());
        out.push('\t');
        out.push_str(path);
        out.push('\n');
    }
    out
}

/// Reads the kinds set by hand. A missing, unreadable or too large file, or one with another
/// header, holds none; a line that does not parse is left out.
fn hand_kinds_from_text(text: &str) -> HashMap<PathBuf, Kind> {
    let mut lines = text.lines();
    if lines.next() != Some(HAND_KINDS_HEADER) {
        return HashMap::new();
    }
    lines
        .filter_map(|line| {
            let (id, path) = line.split_once('\t')?;
            Some((PathBuf::from(path), Kind::from_id(id)?)).filter(|(p, _)| !p.as_os_str().is_empty())
        })
        .take(MAX_HAND_KINDS)
        .collect()
}

fn read_hand_kinds(path: &Path) -> HashMap<PathBuf, Kind> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() <= MAX_HAND_KINDS_BYTES => {}
        _ => return HashMap::new(),
    }
    std::fs::read_to_string(path).map(|t| hand_kinds_from_text(&t)).unwrap_or_default()
}

impl Drop for SoundMap {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.queue.jobs.lock() {
            jobs.0.clear();
            jobs.1 = true;
        }
        self.queue.ready.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        self.save_due(true);
        self.finish_saves();
    }
}

fn work_loop(queue: &Queue, sender: &Sender<Done>) {
    loop {
        let job = {
            let Ok(mut jobs) = queue.jobs.lock() else { return };
            loop {
                if jobs.1 {
                    return;
                }
                if let Some(job) = jobs.0.pop_front() {
                    break job;
                }
                jobs = match queue.ready.wait(jobs) {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
            }
        };
        let (_, file) = job;
        let analysis = analyse_file(&file.path, file.size);
        if sender.send((file, analysis)).is_err() {
            return;
        }
    }
}

/// Decodes and analyses one file.
pub fn analyse_file(path: &Path, size: u64) -> Analysis {
    if size > MAX_FILE_BYTES {
        return Analysis::Failed(format!("larger than {} MiB", MAX_FILE_BYTES / 1024 / 1024));
    }
    match suhina_media::decode_file(path) {
        Ok(audio) => {
            let seconds = audio.frames() as f32 / audio.sample_rate.max(1) as f32;
            match features::analyse(&audio.samples, audio.channels, audio.sample_rate) {
                Some(features) => Analysis::Analysed { features, seconds },
                None => Analysis::Failed("silent".into()),
            }
        }
        Err(reason) => Analysis::Failed(reason),
    }
}

fn relative(path: &Path, folder: &Path) -> PathBuf {
    path.strip_prefix(folder).map(Path::to_path_buf).unwrap_or_else(|_| path.to_path_buf())
}

/// A seed from the scope's items, so the same scope always gives the same layout.
fn scope_seed<'a>(paths: impl Iterator<Item = &'a Path>) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for path in paths {
        for byte in path.to_string_lossy().as_bytes().iter().chain(&[0u8]) {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests;
