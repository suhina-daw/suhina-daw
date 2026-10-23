// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Missing audio files (`docs/spec/missing-files.md`): listing the ones a project names but
//! cannot find, finding candidates by name, and pointing the clips at the files found.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize};

use serde_json::{Value, json};
use suhina_library::find::{MAX_CANDIDATES, MAX_NAMES, rank_candidates, walk_for_names};
use suhina_model::LaneData;

use crate::{CommandError, Projects, Session};

/// Files relinked by one `project.relinkFiles` at most.
pub const MAX_RELINK: usize = 4096;

fn texts(params: &Value, name: &str) -> Result<Vec<String>, CommandError> {
    params
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{name} must be a list of texts"))?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("{name} must be a list of texts").into()))
        .collect()
}

impl Session {
    /// The audio files the project's clips name that cannot be found.
    pub fn missing_files(&self) -> Vec<suhina_media::MissingAudio> {
        missing_of(self.project(), self.audio_folder(), &self.audio.pool, self.trusted())
    }
}

/// The missing files of `project`, whose audio `folder` sits beside and whose decoded files
/// `audio` holds: a file already decoded is missing only when its last path left the disk.
/// The export's warnings name the same files, from [`crate::export::ExportData`]'s copies.
pub(crate) fn missing_of(
    project: &suhina_model::Project,
    folder: Option<&Path>,
    audio: &suhina_engine::AudioPool,
    trusted: &suhina_media::Trusted,
) -> Vec<suhina_media::MissingAudio> {
    let folder = folder.map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    suhina_media::missing_audio_trusted(project, &folder, trusted)
        .into_iter()
        .filter(|missing| {
            // Already decoded paths were explicitly loaded or safely resolved on open.
            let loaded = audio.files.contains_key(&missing.file)
                || audio.files.contains_key(&suhina_model::stream_key(&missing.file, 0));
            !loaded || !missing.last_path.is_file()
        })
        .collect()
}

/// `project.missingFiles`.
pub(crate) fn missing_files(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let files: Vec<Value> = session
        .missing_files()
        .into_iter()
        .map(|m| json!({ "file": m.file, "lastPath": m.last_path.display().to_string(), "clips": m.clips }))
        .collect();
    Ok(json!({ "files": files }))
}

/// `project.relinkFiles`: every audio region naming one of `files` plays the file at the same
/// place in `to` instead. One undo step (an edit).
pub(crate) fn relink_files(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let files = texts(params, "files")?;
    let to = texts(params, "to")?;
    if files.len() != to.len() {
        return Err("files and to must be as long as each other".into());
    }
    if files.is_empty() {
        return Err("files is empty".into());
    }
    if files.len() > MAX_RELINK {
        return Err(format!("at most {MAX_RELINK} files at once").into());
    }
    let mut relinked = Vec::new();
    for (file, target) in files.iter().zip(&to) {
        let target = crate::files::absolute(Path::new(target));
        if !target.is_file() {
            return Err(format!("{} is not a file", target.display()).into());
        }
        // The streams of a Stems file that clips play, so each is decoded with it.
        let mut streams: Option<u16> = None;
        let mut named = session
            .project()
            .channels
            .iter()
            .flat_map(|c| &c.devices)
            .any(|d| d.sample_file() == Some(file.as_str()) || d.sample_absolute_file() == Some(file.as_str()));
        for content in &session.project().contents {
            for lane in &content.lanes {
                if let LaneData::Audio(region) = &lane.data
                    && (region.file == *file || region.absolute_file.as_deref() == Some(file.as_str()))
                {
                    named = true;
                    if let Some(n) = region.stream {
                        streams = Some(streams.map_or(n, |m| m.max(n)));
                    }
                }
            }
        }
        if !named {
            return Err(format!("no audio clip names {file}").into());
        }
        // Decoded before anything changes, so a file that cannot be read changes nothing.
        match streams {
            Some(last) => {
                session
                    .decoded_streams(&target, usize::from(last) + 1)
                    .map_err(|e| format!("{}: {e}", target.display()))?;
            }
            None => {
                session.decoded_audio(&target).map_err(|e| format!("{}: {e}", target.display()))?;
            }
        }
        let key = target.display().to_string();
        let mut regions = 0;
        for device in session.project.channels.iter_mut().flat_map(|c| &mut c.devices) {
            if device.sample_file() == Some(file.as_str()) || device.sample_absolute_file() == Some(file.as_str()) {
                device.set_sample_file(&key);
                regions += 1;
            }
        }
        for content in &mut session.project.contents {
            for lane in &mut content.lanes {
                if let LaneData::Audio(region) = &mut lane.data
                    && (region.file == *file || region.absolute_file.as_deref() == Some(file.as_str()))
                {
                    region.file = key.clone();
                    region.absolute_file = Some(key.clone());
                    regions += 1;
                }
            }
        }
        relinked.push(json!({ "file": file, "to": key, "regions": regions }));
    }
    Ok(json!({ "relinked": relinked }))
}

/// The folder [`collect`] copies into, inside the project's folder.
pub const COLLECTED: [&str; 2] = ["Samples", "Collected"];

/// `project.collect {folder?}`: copies the audio the project plays from outside `folder` (the
/// project's own folder by default) into `Samples/Collected` there and points every clip and
/// device at the copies, as one undo step, so the folder holds everything the project needs
/// (› Collect and save in `docs/spec/missing-files.md`). A copy of the same name and size that
/// is already there is used; another of the same name gets a number.
pub(crate) fn collect(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let folder = match params.get("folder") {
        None | Some(Value::Null) => session
            .source()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or("the project has no folder yet; give the folder to collect into")?,
        Some(v) => PathBuf::from(v.as_str().ok_or("folder must be a text")?),
    };
    let folder = crate::files::absolute(&folder);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let own = folder.canonicalize().map_err(|e| format!("{}: {e}", folder.display()))?;
    let into = COLLECTED.iter().fold(folder.clone(), |p, part| p.join(part));
    let (mut files, mut to) = (Vec::new(), Vec::new());
    for (key, path) in session.loaded_audio() {
        let Ok(real) = path.canonicalize() else { continue };
        if real.starts_with(&own) || !real.is_file() {
            continue;
        }
        std::fs::create_dir_all(&into).map_err(|e| format!("{}: {e}", into.display()))?;
        let copy = copy_name(&into, &real)?;
        if !copy.exists() {
            std::fs::copy(&real, &copy).map_err(|e| format!("{}: {e}", copy.display()))?;
        }
        // The copy plays what the original did, decoded once.
        if let Some(data) = session.audio.pool.files.get(&key).cloned() {
            session.audio.pool.files.insert(copy.display().to_string(), data);
        }
        files.push(key);
        to.push(copy.display().to_string());
    }
    if files.is_empty() {
        return Ok(json!({ "collected": [], "folder": into.display().to_string() }));
    }
    let relinked = relink_files(session, &json!({ "files": files, "to": to }))?;
    Ok(json!({ "collected": relinked["relinked"], "folder": into.display().to_string() }))
}

/// Where in `into` a copy of `file` goes: its own name, or with a number when another file of
/// that name and other contents is there already (two kits' `Kick.wav` of one length).
fn copy_name(into: &Path, file: &Path) -> Result<PathBuf, CommandError> {
    let name = file.file_name().ok_or("the file has no name")?;
    let size = std::fs::metadata(file).map(|m| m.len()).ok();
    let same = |p: &Path| !p.exists() || (std::fs::metadata(p).map(|m| m.len()).ok() == size && same_contents(p, file));
    let first = into.join(name);
    if same(&first) {
        return Ok(first);
    }
    let path = Path::new(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 2..10_000 {
        let candidate = into.join(format!("{stem} ({n}){extension}"));
        if same(&candidate) {
            return Ok(candidate);
        }
    }
    Err(format!("no free name for {} in {}", path.display(), into.display()).into())
}

/// Whether files `a` and `b` hold the same bytes, read in chunks; `false` when either cannot
/// be read.
fn same_contents(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    /// Reads into `buffer` until it is full or the file ends; how much it read.
    fn fill(file: &mut std::fs::File, buffer: &mut [u8]) -> Option<usize> {
        let mut read = 0;
        while let Some(rest) = buffer.get_mut(read..).filter(|r| !r.is_empty()) {
            match file.read(rest).ok()? {
                0 => break,
                n => read += n,
            }
        }
        Some(read)
    }
    let (Ok(mut a), Ok(mut b)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut x, mut y) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let (Some(n), Some(m)) = (fill(&mut a, &mut x), fill(&mut b, &mut y)) else { return false };
        if n != m || x.get(..n) != y.get(..m) {
            return false;
        }
        if n == 0 {
            return true;
        }
    }
}

/// `library.findFiles`: candidates for each name, best first for the active project's missing
/// file of that name.
pub(crate) fn find_files(projects: &mut Projects, params: &Value) -> Result<Value, CommandError> {
    let names = texts(params, "names")?;
    if names.len() > MAX_NAMES {
        return Err(format!("at most {MAX_NAMES} names at once").into());
    }
    let folder = match params.get("folder") {
        None | Some(Value::Null) => None,
        Some(v) => Some(PathBuf::from(v.as_str().ok_or("folder must be a text")?)),
    };
    let (found, note) = search(projects, &names, folder.as_deref(), &AtomicBool::new(false), &AtomicUsize::new(0))?;
    let items: Vec<Value> = found
        .into_iter()
        .map(|(name, paths)| {
            json!({ "name": name, "paths": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() })
        })
        .collect();
    Ok(json!({ "found": items, "note": note }))
}

/// Candidates for each name: with `folder`, from walking it; otherwise from the browser's index
/// and a walk of the active project's folder, without repeats. Each name's candidates come best
/// first for where the active project's missing file of that name was, at most
/// [`MAX_CANDIDATES`].
pub fn search(
    projects: &mut Projects,
    names: &[String],
    folder: Option<&Path>,
    stop: &AtomicBool,
    walked: &AtomicUsize,
) -> Result<(suhina_library::find::Found, Option<String>), CommandError> {
    let session = projects.active().and_then(|id| projects.get(id));
    let last_paths: Vec<(String, PathBuf)> = session
        .map(|s| {
            s.missing_files()
                .into_iter()
                .map(|m| {
                    (
                        Path::new(&m.file).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default(),
                        m.last_path,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let project_folder = session.and_then(|s| s.audio_folder()).map(Path::to_path_buf);
    // The folders this project's audio was loaded from: a file moved inside one is found there.
    let source = session.and_then(|s| s.source()).and_then(|s| std::path::absolute(s).ok());
    let source_folders = source.map(|s| projects.file_library_mut().project_folders(&s)).unwrap_or_default();
    let (mut found, note) = match folder {
        Some(folder) => {
            let walked = walk_for_names(folder, names, stop, walked).map_err(CommandError)?;
            (walked.found, walked.note)
        }
        None => {
            let mut found = projects.file_library_mut().find_by_names(names);
            let mut note = None;
            let dirs = project_folder.into_iter().chain(source_folders).filter(|d| d.is_dir());
            for dir in dirs {
                let Ok(more) = walk_for_names(&dir, names, stop, walked) else { continue };
                note = note.or(more.note);
                for ((_, paths), (_, extra)) in found.iter_mut().zip(more.found) {
                    for path in extra {
                        if !paths.contains(&path) {
                            paths.push(path);
                        }
                    }
                }
            }
            (found, note)
        }
    };
    for (name, paths) in &mut found {
        let last = last_paths.iter().find(|(n, _)| *n == name.to_lowercase()).map(|(_, p)| p.clone());
        rank_candidates(&last.unwrap_or_else(|| PathBuf::from(name.as_str())), paths);
        // The index and the project folder give up to MAX_CANDIDATES each; the best of both stay.
        paths.truncate(MAX_CANDIDATES);
    }
    Ok((found, note))
}

#[cfg(test)]
mod copy_name_tests {
    use super::*;

    #[test]
    fn a_file_of_the_same_name_and_size_but_other_bytes_gets_a_number() {
        let dir = std::env::temp_dir().join(format!("suhina-copy-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let into = dir.join("Collected");
        for path in [&into, &dir.join("KitA"), &dir.join("KitB")] {
            std::fs::create_dir_all(path).unwrap_or_else(|e| panic!("{e}"));
        }
        let (a, b) = (dir.join("KitA").join("Kick.wav"), dir.join("KitB").join("Kick.wav"));
        std::fs::write(&a, [1u8; 100]).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&b, [2u8; 100]).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(into.join("Kick.wav"), [1u8; 100]).unwrap_or_else(|e| panic!("{e}"));
        let name = |file: &Path| copy_name(&into, file).unwrap_or_else(|e| panic!("{}", e.0));
        assert_eq!(name(&a), into.join("Kick.wav"), "the same bytes: the copy there is reused");
        assert_eq!(name(&b), into.join("Kick (2).wav"), "other bytes: a name of its own");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
