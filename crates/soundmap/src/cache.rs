// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The feature cache of one library folder (ADR 0013 › Cache): `features-<hash>.bin` beside
//! the library's index.
//!
//! Layout, little-endian: the magic `LWSM`, the file version (u32), the feature version (u32),
//! the entry count (u32), then per entry: the path below the folder (u16 length, UTF-8), size
//! (u64), modification time (u64), a status byte (0 analysed, 1 failed) and, when analysed,
//! the file's length in seconds (f32) and [`FEATURE_COUNT`] f32 values; when failed, the reason
//! (u16 length, UTF-8).
//!
//! Beside it, `features-<hash>.log` keeps the changes since the file was written: the magic
//! `LWSJ`, the two versions, then records, each a tag (0 an entry put, as above; 1 an entry
//! removed, its path as above). New results are appended there in a few bytes, and the whole
//! file is written again only when the journal has grown past a share of it; a torn last record
//! (a crash while appending) is left out, and cut off when the journal is read, so the records
//! appended after it are read too.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::features::{FEATURE_COUNT, FEATURE_VERSION, Features};

const MAGIC: &[u8; 4] = b"LWSM";
const JOURNAL_MAGIC: &[u8; 4] = b"LWSJ";
const FILE_VERSION: u32 = 1;
/// A cache file (or its journal) larger than this is not read (spec › Limits). A library of a
/// quarter of a million samples takes about 63 MiB.
pub const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;

/// A change to a folder's cache since its file was written, kept in its journal.
// Nearly every change puts an entry, so boxing it would only add an allocation to each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Put(PathBuf, Entry),
    Remove(PathBuf),
}

/// What is known of one file.
// Nearly every entry is analysed, so boxing the features would only add an allocation to each.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Analysis {
    /// The features and the file's whole length in seconds.
    Analysed {
        features: Features,
        seconds: f32,
    },
    Failed(String),
}

/// One cached file: the size and time it had when analysed, and the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub size: u64,
    pub modified: u64,
    pub analysis: Analysis,
}

/// The cache of one folder, by path below the folder.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderCache {
    pub entries: HashMap<PathBuf, Entry>,
    /// Records in its journal on disk, past the file: the file is written again when they
    /// pass a share of the entries.
    pub journal: usize,
}

/// Whether an entry can be written (its path and reason fit their u16 lengths).
fn writable(path: &Path, entry: &Entry) -> bool {
    path.to_str().is_some_and(|s| s.len() <= usize::from(u16::MAX))
        && match &entry.analysis {
            Analysis::Failed(reason) => reason.len() <= usize::from(u16::MAX),
            Analysis::Analysed { .. } => true,
        }
}

fn write_path(out: &mut Vec<u8>, path: &str) {
    out.extend_from_slice(&(path.len() as u16).to_le_bytes());
    out.extend_from_slice(path.as_bytes());
}

fn write_entry(out: &mut Vec<u8>, path: &str, entry: &Entry) {
    write_path(out, path);
    out.extend_from_slice(&entry.size.to_le_bytes());
    out.extend_from_slice(&entry.modified.to_le_bytes());
    match &entry.analysis {
        Analysis::Analysed { features, seconds } => {
            out.push(0);
            out.extend_from_slice(&seconds.to_le_bytes());
            for v in features {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        Analysis::Failed(reason) => {
            out.push(1);
            out.extend_from_slice(&(reason.len() as u16).to_le_bytes());
            out.extend_from_slice(reason.as_bytes());
        }
    }
}

fn read_path(r: &mut Reader<'_>) -> Option<PathBuf> {
    let length = usize::from(r.u16()?);
    Some(PathBuf::from(std::str::from_utf8(r.take(length)?).ok()?))
}

fn read_entry(r: &mut Reader<'_>) -> Option<(PathBuf, Entry)> {
    let path = read_path(r)?;
    let size = r.u64()?;
    let modified = r.u64()?;
    let analysis = match r.take(1)?.first()? {
        0 => {
            let seconds = f32::from_le_bytes(r.take(4)?.try_into().ok()?);
            if !seconds.is_finite() || seconds < 0.0 {
                return None;
            }
            let mut features = [0.0f32; FEATURE_COUNT];
            for slot in features.iter_mut() {
                *slot = f32::from_le_bytes(r.take(4)?.try_into().ok()?);
                if !slot.is_finite() {
                    return None;
                }
            }
            Analysis::Analysed { features, seconds }
        }
        1 => {
            let length = usize::from(r.u16()?);
            Analysis::Failed(std::str::from_utf8(r.take(length)?).ok()?.to_string())
        }
        _ => return None,
    };
    Some((path, Entry { size, modified, analysis }))
}

/// The journal beside the cache file at `path`.
pub fn journal_path(path: &Path) -> PathBuf {
    path.with_extension("log")
}

/// Appends `changes` to the journal beside the cache file at `path`, started with its header
/// when there is none yet.
pub fn append(path: &Path, changes: &[Change]) -> std::io::Result<()> {
    let journal = journal_path(path);
    let mut out = Vec::new();
    if std::fs::metadata(&journal).map_or(true, |m| m.len() == 0) {
        out.extend_from_slice(JOURNAL_MAGIC);
        out.extend_from_slice(&FILE_VERSION.to_le_bytes());
        out.extend_from_slice(&FEATURE_VERSION.to_le_bytes());
    }
    for change in changes {
        match change {
            Change::Put(path, entry) if writable(path, entry) => {
                out.push(0);
                write_entry(&mut out, path.to_str().unwrap_or_default(), entry);
            }
            Change::Remove(path) if path.to_str().is_some_and(|s| s.len() <= usize::from(u16::MAX)) => {
                out.push(1);
                write_path(&mut out, path.to_str().unwrap_or_default());
            }
            _ => {}
        }
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&journal)?;
    file.write_all(&out)?;
    file.flush()
}

impl FolderCache {
    /// The entry for a file, when it was analysed at this size and time.
    pub fn current(&self, relative: &Path, size: u64, modified: u64) -> Option<&Analysis> {
        self.entries.get(relative).filter(|e| e.size == size && e.modified == modified).map(|e| &e.analysis)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FILE_VERSION.to_le_bytes());
        out.extend_from_slice(&FEATURE_VERSION.to_le_bytes());
        let mut entries: Vec<(&PathBuf, &Entry)> = self.entries.iter().filter(|(p, e)| writable(p, e)).collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        for (path, entry) in entries {
            write_entry(&mut out, path.to_str().unwrap_or_default(), entry);
        }
        out
    }

    /// Reads a cache. Anything malformed, of another version or too large gives `None`: the
    /// folder is then analysed again.
    pub fn from_bytes(bytes: &[u8]) -> Option<FolderCache> {
        if bytes.len() as u64 > MAX_CACHE_BYTES {
            return None;
        }
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != MAGIC || r.u32()? != FILE_VERSION || r.u32()? != FEATURE_VERSION {
            return None;
        }
        let count = r.u32()? as usize;
        // Every entry takes at least 19 bytes, so a count the file cannot hold is refused before
        // anything is allocated for it.
        if count > bytes.len() / 19 {
            return None;
        }
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            let (path, entry) = read_entry(&mut r)?;
            entries.insert(path, entry);
        }
        (r.at == bytes.len()).then_some(FolderCache { entries, journal: 0 })
    }

    /// Applies a journal's bytes: its records in order, up to a torn or malformed one. Returns
    /// how many were applied, and how many bytes they and the header take; a journal of other
    /// versions applies none and takes none.
    pub fn apply_journal(&mut self, bytes: &[u8]) -> (usize, usize) {
        let mut r = Reader { bytes, at: 0 };
        let header =
            (|| Some(r.take(4)? == JOURNAL_MAGIC && r.u32()? == FILE_VERSION && r.u32()? == FEATURE_VERSION))();
        if header != Some(true) {
            return (0, 0);
        }
        let (mut applied, mut good) = (0, r.at);
        while r.at < bytes.len() {
            let record = (|| match r.take(1)?.first()? {
                0 => read_entry(&mut r).map(|(path, entry)| Change::Put(path, entry)),
                1 => read_path(&mut r).map(Change::Remove),
                _ => None,
            })();
            let Some(change) = record else { break };
            match change {
                Change::Put(path, entry) => {
                    self.entries.insert(path, entry);
                }
                Change::Remove(path) => {
                    self.entries.remove(&path);
                }
            }
            applied += 1;
            good = r.at;
        }
        (applied, good)
    }

    /// Reads the cache at `path` and the journal beside it; a missing or unreadable file is
    /// empty, and the journal's changes are applied to what there is. What follows the journal's
    /// last whole record (a write cut short by a crash, or a journal of other versions) is cut
    /// off, so that records appended later follow a whole one and are read.
    pub fn load(path: &Path) -> FolderCache {
        let read = |path: &Path| match std::fs::metadata(path) {
            Ok(m) if m.len() <= MAX_CACHE_BYTES => std::fs::read(path).ok(),
            _ => None,
        };
        let mut cache = read(path).and_then(|b| FolderCache::from_bytes(&b)).unwrap_or_default();
        let journal = journal_path(path);
        if let Some(bytes) = read(&journal) {
            let (applied, good) = cache.apply_journal(&bytes);
            cache.journal = applied;
            if good < bytes.len() {
                let _cut_next_time = std::fs::OpenOptions::new().write(true).open(&journal).and_then(|file| {
                    // Unless it grew meanwhile: those bytes are not the ones read.
                    if file.metadata()?.len() == bytes.len() as u64 { file.set_len(good as u64) } else { Ok(()) }
                });
            }
        }
        cache
    }

    /// Writes the cache to a file beside `path` and moves it over `path`; the journal, whose
    /// changes are in it now, goes.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let temporary = path.with_extension("bin.tmp");
        {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&self.to_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&temporary, path)?;
        match std::fs::remove_file(journal_path(path)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// The cache file name of a folder: a stable hash of its path (FNV-1a), so it does not change
/// between runs or versions of Rust.
pub fn file_name(folder: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in folder.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("features-{hash:016x}.bin")
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FolderCache {
        let mut cache = FolderCache::default();
        let mut features = [0.0f32; FEATURE_COUNT];
        features.iter_mut().enumerate().for_each(|(i, v)| *v = i as f32 * 0.5 - 3.0);
        cache.entries.insert(
            PathBuf::from("Kicks/kick 1.wav"),
            Entry { size: 1234, modified: 99, analysis: Analysis::Analysed { features, seconds: 0.75 } },
        );
        cache.entries.insert(
            PathBuf::from("broken.wav"),
            Entry { size: 5, modified: 7, analysis: Analysis::Failed("no audio decoded".into()) },
        );
        cache
    }

    #[test]
    fn round_trips() {
        let cache = sample();
        assert_eq!(FolderCache::from_bytes(&cache.to_bytes()), Some(cache));
        let empty = FolderCache::default();
        assert_eq!(FolderCache::from_bytes(&empty.to_bytes()), Some(empty));
    }

    #[test]
    fn a_changed_file_is_not_current() {
        let cache = sample();
        let path = Path::new("Kicks/kick 1.wav");
        assert!(cache.current(path, 1234, 99).is_some());
        assert!(cache.current(path, 1234, 100).is_none());
        assert!(cache.current(path, 1235, 99).is_none());
        assert!(cache.current(Path::new("other.wav"), 1, 1).is_none());
    }

    #[test]
    fn malformed_caches_are_refused() {
        let bytes = sample().to_bytes();
        // Every truncation, and every single changed header byte.
        for end in 0..bytes.len() {
            assert_eq!(FolderCache::from_bytes(&bytes[..end]), None, "cut at {end}");
        }
        for i in 0..16 {
            let mut changed = bytes.clone();
            changed[i] ^= 0xff;
            assert_eq!(FolderCache::from_bytes(&changed), None, "byte {i}");
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(FolderCache::from_bytes(&trailing), None);
        // A huge count with no entries behind it allocates nothing and is refused.
        let mut huge = bytes[..12].to_vec();
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(FolderCache::from_bytes(&huge), None);
        // Arbitrary bytes never panic.
        let mut state = 3u32;
        for _ in 0..200 {
            let noise: Vec<u8> = (0..64)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect();
            let mut with_header = bytes[..12].to_vec();
            with_header.extend_from_slice(&noise);
            let _ = FolderCache::from_bytes(&with_header);
        }
    }

    #[test]
    fn saves_and_loads() {
        let folder = crate::tests::unique_dir("cache-file");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(file_name(Path::new("/samples")));
        let cache = sample();
        cache.save(&path).unwrap();
        assert_eq!(FolderCache::load(&path), cache);
        std::fs::write(&path, b"garbage").unwrap();
        assert_eq!(FolderCache::load(&path), FolderCache::default());
        assert_eq!(FolderCache::load(&folder.join("missing.bin")), FolderCache::default());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Changes appended to the journal are read back over the file; the next full save takes
    /// them in and removes the journal; a torn last record is left out.
    #[test]
    fn the_journal_keeps_changes_until_the_file_is_written_again() {
        let folder = crate::tests::unique_dir("cache-journal");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(file_name(Path::new("/samples")));
        let mut cache = sample();
        cache.save(&path).unwrap();
        let added = Entry { size: 9, modified: 3, analysis: Analysis::Failed("short".into()) };
        let changes =
            vec![Change::Put(PathBuf::from("new.wav"), added.clone()), Change::Remove(PathBuf::from("broken.wav"))];
        append(&path, &changes).unwrap();
        cache.entries.insert(PathBuf::from("new.wav"), added);
        cache.entries.remove(Path::new("broken.wav"));
        let loaded = FolderCache::load(&path);
        assert_eq!(loaded.entries, cache.entries);
        assert_eq!(loaded.journal, 2);
        // A torn record at the end: the ones before it stand.
        let mut bytes = std::fs::read(journal_path(&path)).unwrap();
        bytes.extend_from_slice(&[0, 5, 0, b'a']);
        std::fs::write(journal_path(&path), &bytes).unwrap();
        assert_eq!(FolderCache::load(&path).entries, cache.entries);
        // Reading it cut the torn bytes off: a record appended afterwards is read back.
        let later = Entry { size: 4, modified: 8, analysis: Analysis::Failed("later".into()) };
        append(&path, &[Change::Put(PathBuf::from("later.wav"), later.clone())]).unwrap();
        cache.entries.insert(PathBuf::from("later.wav"), later);
        let loaded = FolderCache::load(&path);
        assert_eq!((&loaded.entries, loaded.journal), (&cache.entries, 3));
        // A full save takes the changes in and the journal goes.
        loaded.save(&path).unwrap();
        assert!(!journal_path(&path).exists());
        assert_eq!(FolderCache::load(&path).entries, cache.entries);
        // A journal with no file before it: its changes over an empty cache.
        std::fs::remove_file(&path).unwrap();
        append(&path, &changes[..1]).unwrap();
        assert_eq!(FolderCache::load(&path).entries.len(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn file_names_are_stable() {
        assert_eq!(file_name(Path::new("/samples")), file_name(Path::new("/samples")));
        assert_ne!(file_name(Path::new("/samples")), file_name(Path::new("/samples2")));
        assert!(file_name(Path::new("C:\\x")).starts_with("features-"));
    }
}
