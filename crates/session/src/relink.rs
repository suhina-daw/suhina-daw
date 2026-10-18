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
        let folder = self.audio_folder().map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        suhina_media::missing_audio(self.project(), &folder)
            .into_iter()
            .filter(|missing| {
                // Already decoded paths were explicitly loaded or safely resolved on open.
                let loaded = self.audio().files.contains_key(&missing.file)
                    || self.audio().files.contains_key(&suhina_model::stream_key(&missing.file, 0));
                !loaded || !missing.last_path.is_file()
            })
            .collect()
    }
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
    let (mut found, note) = match folder {
        Some(folder) => {
            let walked = walk_for_names(folder, names, stop, walked).map_err(CommandError)?;
            (walked.found, walked.note)
        }
        None => {
            let mut found = projects.file_library_mut().find_by_names(names);
            let mut note = None;
            if let Some(dir) = project_folder.filter(|d| d.is_dir())
                && let Ok(more) = walk_for_names(&dir, names, stop, walked)
            {
                note = more.note;
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
