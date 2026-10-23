// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio files for a project: finds the files its audio clips refer to and decodes them into
//! memory with symphonia (MPL-2.0), so the engine can play them without touching the disk.

pub mod info;
pub mod midi;
pub mod stems;
pub mod tempo;
pub mod write;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use suhina_engine::model::{LaneData, Project};
pub use suhina_engine::{AudioData, AudioPool};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Longest decoded duration; the independent byte budget also applies.
pub(crate) const MAX_SECONDS: f64 = 60.0 * 60.0 * 2.0;
const MAX_DECODED_SAMPLES: usize = 1024 * 1024 * 1024 / std::mem::size_of::<f32>();
pub(crate) fn decode_limit(rate: u32, channels: usize) -> Result<usize, String> {
    if !(1000..=384000).contains(&rate) || !(1..=32).contains(&channels) {
        return Err("audio needs 1000..384000 Hz and 1..32 channels".into());
    }
    Ok(((MAX_SECONDS as usize)
        .checked_mul(rate as usize)
        .and_then(|n| n.checked_mul(channels))
        .ok_or("audio size overflow")?)
    .min(MAX_DECODED_SAMPLES))
}
pub(crate) fn reserve_audio(samples: &mut Vec<f32>, additional: usize, limit: usize) -> Result<(), String> {
    let needed = samples
        .len()
        .checked_add(additional)
        .filter(|n| *n <= limit)
        .ok_or("decoded audio exceeds the duration or 1 GiB memory budget")?;
    if needed > samples.capacity() {
        let capacity = samples.capacity().saturating_mul(2).max(needed).min(limit);
        samples.try_reserve_exact(capacity - samples.len()).map_err(|_| "cannot allocate decoded audio")?;
    }
    Ok(())
}

/// Loads every audio file the project uses. Same as [`decode_project_audio`].
pub fn load_project_audio(project: &Project, project_dir: &Path) -> (AudioPool, Vec<String>) {
    decode_project_audio(project, project_dir)
}

/// Loads every audio file the project uses, those outside its folder too when `trusted` names
/// them. Same as [`decode_project_audio_trusted`].
pub fn load_project_audio_trusted(
    project: &Project,
    project_dir: &Path,
    trusted: &Trusted,
) -> (AudioPool, Vec<String>) {
    decode_project_audio_trusted(project, project_dir, trusted)
}

/// Decodes every audio file the project uses, on all cores. `project_dir` is the folder the
/// project file is in; references are resolved only inside it, without following links.
/// External references require an explicit user load or relink. Files that cannot be found or decoded are reported in the returned warnings and
/// left out.
pub fn decode_project_audio(project: &Project, project_dir: &Path) -> (AudioPool, Vec<String>) {
    decode_project_audio_trusted(project, project_dir, &Trusted::default())
}

/// [`decode_project_audio`], reading too the files outside the project's folder that `trusted`
/// names (› Project file boundaries in `docs/spec/missing-files.md`).
pub fn decode_project_audio_trusted(
    project: &Project,
    project_dir: &Path,
    trusted: &Trusted,
) -> (AudioPool, Vec<String>) {
    // Each file once (once more when a Stems file is played whole as well as by track), with
    // where to look for it and whether all of its audio tracks are wanted.
    let mut wanted: Vec<(String, Vec<PathBuf>, bool)> = Vec::new();
    for device in project.channels.iter().flat_map(|c| &c.devices) {
        if let Some(file) = device.sample_file()
            && !wanted.iter().any(|(key, _, _)| key == file)
        {
            let candidates = locate(file, device.sample_absolute_file(), project_dir, trusted).into_iter().collect();
            wanted.push((file.into(), candidates, false));
        }
    }
    for content in &project.contents {
        for lane in &content.lanes {
            let LaneData::Audio(region) = &lane.data else { continue };
            // An empty audio clip has no file yet: nothing to decode, no warning.
            if region.file.is_empty() {
                continue;
            }
            let streams = region.stream.is_some();
            if wanted.iter().any(|(file, _, s)| *file == region.file && *s == streams) {
                continue;
            }
            let candidates =
                locate(&region.file, region.absolute_file.as_deref(), project_dir, trusted).into_iter().collect();
            wanted.push((region.file.clone(), candidates, streams));
        }
    }

    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(1, usize::from).clamp(1, 16).min(wanted.len().max(1));
    type Decoded = Result<AudioData, String>;
    let decoded: Vec<(String, Decoded)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while let Some((file, candidates, streams)) = wanted.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let Some(path) = candidates.iter().find(|p| p.is_file()) else {
                            done.push((file.clone(), Err(format!("audio file {file} not found"))));
                            continue;
                        };
                        let failed = |e: String| format!("audio file {}: {e}", path.display());
                        if !*streams {
                            done.push((file.clone(), decode_file(path).map_err(failed)));
                            continue;
                        }
                        match stems::decode_streams(path) {
                            Ok(all) => done.extend(
                                all.into_iter()
                                    .zip(0u16..)
                                    .map(|(data, n)| (suhina_engine::model::stream_key(file, n), Ok(data))),
                            ),
                            Err(e) => done.push((file.clone(), Err(failed(e)))),
                        }
                    }
                    done
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    });

    let mut pool = AudioPool::new();
    let mut warnings = Vec::new();
    for (file, result) in decoded {
        match result {
            Ok(data) => {
                pool.files.insert(file, Arc::new(data));
            }
            Err(e) => warnings.push(e),
        }
    }
    warnings.sort();
    (pool, warnings)
}

/// A file the project's audio clips name that is found neither at its path relative to the
/// project's folder nor at its absolute path (`docs/spec/missing-files.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingAudio {
    /// The file as the clips name it (their relative path).
    pub file: String,
    /// Where it was looked for last: its absolute path when the project has one, otherwise its
    /// path from the project's folder.
    pub last_path: PathBuf,
    /// How many clips (in the arrangement and the clip slots) play it.
    pub clips: usize,
}

/// Audio files outside a project's folder that opening it may read (› Project file boundaries
/// in `docs/spec/missing-files.md`): in the desktop app, any file on a local disk (`local`);
/// elsewhere (the CLI, the MCP server) the files the person loaded into a project on this machine
/// and saved it with, and the browser's library folders. Anything else outside the folder stays
/// missing until the person chooses it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trusted {
    /// Folders whose files may be read: the browser's library folders.
    pub folders: Vec<PathBuf>,
    /// Single files that may be read, by absolute path.
    pub files: std::collections::BTreeSet<PathBuf>,
    /// Any file on a local disk may be read, as a project the person opens names it: the
    /// desktop app. A network share or a device path still is not.
    pub local: bool,
}

impl Trusted {
    /// `path` when it names a trusted file, or a file inside a trusted folder, that is there. It
    /// is decided from the path alone before anything is read, so a path that is neither (a
    /// network share a project names) is never touched.
    pub fn find(&self, path: &Path) -> Option<PathBuf> {
        if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return None;
        }
        if self.local && on_local_disk(path) {
            return path.is_file().then(|| path.to_path_buf());
        }
        if !self.files.contains(path) {
            let folder = self.folders.iter().find(|f| f.is_absolute() && path.starts_with(f))?;
            // Inside the folder as it is on disk too, links followed.
            let canonical_folder = folder.canonicalize().ok()?;
            if !path.canonicalize().ok()?.starts_with(&canonical_folder) {
                return None;
            }
        }
        path.is_file().then(|| path.to_path_buf())
    }
}

/// Whether `path` is on a local disk, decided from the path alone: a drive letter on Windows (not
/// a network share or a device path); any absolute path elsewhere.
fn on_local_disk(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    match path.components().next() {
        Some(Component::Prefix(prefix)) => matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)),
        Some(Component::RootDir) => !cfg!(windows),
        _ => false,
    }
}

/// `file` from `root` with its `..` taken away by name, without reading anything.
fn joined(root: &Path, file: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for part in Path::new(file).components() {
        match part {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => out.push(name),
            _ => return None,
        }
    }
    Some(out)
}

/// A project-named file inside its folder, with no parent traversal or symbolic links; or,
/// outside it, one that `trusted` names.
pub fn locate(file: &str, absolute: Option<&str>, project_dir: &Path, trusted: &Trusted) -> Option<PathBuf> {
    let root = std::path::absolute(project_dir).ok()?;
    inside(file, absolute, &root).or_else(|| {
        let from_root = joined(&root, file);
        [absolute.map(PathBuf::from), from_root].into_iter().flatten().find_map(|path| trusted.find(&path))
    })
}

/// [`locate`]'s part inside the project's folder `root`.
fn inside(file: &str, absolute: Option<&str>, root: &Path) -> Option<PathBuf> {
    let root = root.to_path_buf();
    let canonical_root = root.canonicalize().ok()?;
    [Some(file), absolute].into_iter().flatten().find_map(|name| {
        let path = Path::new(name);
        if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return None;
        }
        let candidate = root.join(path);
        if !candidate.starts_with(&root) {
            return None;
        }
        let mut component = root.clone();
        for part in candidate.strip_prefix(&root).ok()?.components() {
            component.push(part);
            if std::fs::read_link(&component).is_ok() {
                return None;
            }
        }
        let canonical = candidate.canonicalize().ok()?;
        (canonical.starts_with(&canonical_root) && canonical.is_file()).then_some(candidate)
    })
}

/// The project's audio files that cannot be found, each once, in the order the project names
/// them. Only looks at whether files exist; nothing is decoded.
pub fn missing_audio(project: &Project, project_dir: &Path) -> Vec<MissingAudio> {
    missing_audio_trusted(project, project_dir, &Trusted::default())
}

/// [`missing_audio`], with the files outside the project's folder that `trusted` names found.
pub fn missing_audio_trusted(project: &Project, project_dir: &Path, trusted: &Trusted) -> Vec<MissingAudio> {
    let mut out: Vec<MissingAudio> = Vec::new();
    for device in project.channels.iter().flat_map(|c| &c.devices) {
        if let Some(file) = device.sample_file()
            && locate(file, device.sample_absolute_file(), project_dir, trusted).is_none()
            && !out.iter().any(|m| m.file == file)
        {
            out.push(MissingAudio {
                file: file.into(),
                last_path: device.sample_absolute_file().map_or_else(|| project_dir.join(file), PathBuf::from),
                clips: 0,
            });
        }
    }
    let mut found: Vec<&str> = Vec::new();
    for content in &project.contents {
        for lane in &content.lanes {
            let LaneData::Audio(region) = &lane.data else { continue };
            // An empty audio clip has no file yet: it is not a missing file.
            if region.file.is_empty() {
                continue;
            }
            if found.contains(&region.file.as_str()) || out.iter().any(|m| m.file == region.file) {
                continue;
            }
            if locate(&region.file, region.absolute_file.as_deref(), project_dir, trusted).is_some() {
                found.push(&region.file);
                continue;
            }
            let last_path = region.absolute_file.as_ref().map_or_else(|| project_dir.join(&region.file), PathBuf::from);
            out.push(MissingAudio { file: region.file.clone(), last_path, clips: 0 });
        }
    }
    // Count the clips that play each missing file.
    let uses = |content: suhina_engine::model::ContentId, file: &str| {
        project
            .contents
            .iter()
            .filter(|c| c.id == content)
            .any(|c| c.lanes.iter().any(|l| matches!(&l.data, LaneData::Audio(r) if r.file == file)))
    };
    let clips: Vec<suhina_engine::model::ContentId> = project
        .arrangement
        .lanes
        .iter()
        .flat_map(|l| l.clips.iter().map(|c| c.content))
        .chain(
            project
                .launcher
                .columns
                .iter()
                .flat_map(|c| c.slots.iter().filter_map(|s| s.clip.as_ref().map(|c| c.content))),
        )
        .collect();
    for missing in &mut out {
        missing.clips = clips.iter().filter(|c| uses(**c, &missing.file)).count();
    }
    out
}

/// Decodes a whole audio file (WAV, AIFF, FLAC, MP3, Ogg Vorbis) to interleaved f32.
pub fn decode_file(path: &Path) -> Result<AudioData, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("unsupported or unreadable format: {e}"))?;
    let track = format.default_track(TrackType::Audio).ok_or("no audio track")?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("no audio codec parameters")?.clone();
    let sample_rate = params.sample_rate.ok_or("unknown sample rate")?;
    let channels =
        params.channels.as_ref().map(|c| c.count()).filter(|c| (1..=32).contains(c)).ok_or("unknown channel count")?;
    let channels = u16::try_from(channels).map_err(|_| "too many channels")?;
    let max_samples = decode_limit(sample_rate, usize::from(channels))?;
    if track
        .num_frames
        .is_some_and(|frames| frames.checked_mul(u64::from(channels)).is_none_or(|n| n > max_samples as u64))
    {
        return Err("audio header exceeds the duration or 1 GiB memory budget".into());
    }

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("unsupported codec: {e}"))?;
    let mut samples = Vec::new();
    let mut packet_samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("read error: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let count = decoded.samples_interleaved();
                reserve_audio(&mut samples, count, max_samples)?;
                packet_samples.clear();
                reserve_audio(&mut packet_samples, count, max_samples)?;
                decoded.copy_to_vec_interleaved(&mut packet_samples);
                samples.extend_from_slice(&packet_samples);
            }
            // A damaged packet is skipped, as players do.
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => continue,
            Err(e) => return Err(format!("decode error: {e}")),
        }
    }
    if samples.is_empty() {
        return Err("no audio decoded".into());
    }
    Ok(AudioData { sample_rate, channels, samples })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
