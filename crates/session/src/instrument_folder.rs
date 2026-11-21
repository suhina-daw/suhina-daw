// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Folder fitting: one note per audio file, velocity layers by level, fused into one fitted
//! model; and a phrase's held notes, one recording each. See
//! `docs/spec/instrument-modal-fitting.md` › Fitting a folder, › Fitting a phrase.

use std::ops::Range;
use std::path::{Path, PathBuf};

use suhina_dsp::modal::model::{MAX_RECORDINGS, ModelRecording};
use suhina_instrument_analysis::{Error, Outcome, modal};
use suhina_media::info::{FileKind, kind_of};
use suhina_model::AudioData;

use crate::CommandError;

/// At most this many audio files are listed from one folder.
pub(crate) const MAX_FILES: usize = 128;
/// Decoded per file: root analysis and fitting read at most eight seconds.
const PREFIX_SECONDS: f64 = 10.0;

/// What became of one file of the folder.
#[derive(Debug, Clone, PartialEq)]
pub enum FileOutcome {
    /// Its recording is in the report's set.
    Kept,
    /// Fitted, but left out to keep the set within 32 recordings.
    LeftOut,
    /// Not fitted, with the reason.
    Skipped(String),
}

impl FileOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::LeftOut => "leftOut",
            Self::Skipped(_) => "skipped",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Skipped(reason) => Some(reason),
            Self::Kept | Self::LeftOut => None,
        }
    }
}

/// One file of the folder: its name, the key it was fitted at and the velocity it stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderFile {
    pub name: String,
    pub key: Option<u8>,
    pub velocity: Option<f32>,
    pub outcome: FileOutcome,
}

/// A folder fit: every listed file, and the recordings a set would hold (sorted by key, velocity).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderReport {
    pub files: Vec<FolderFile>,
    pub recordings: Vec<ModelRecording>,
    /// The entries are a phrase's held notes, not files.
    pub phrase: bool,
}

impl FolderReport {
    pub fn kept(&self) -> usize {
        self.files.iter().filter(|f| f.outcome == FileOutcome::Kept).count()
    }
}

/// A list of audio files from anywhere, sorted by file name: each must be an existing audio file.
pub(crate) fn listed(paths: &[std::path::PathBuf]) -> Result<Vec<PathBuf>, CommandError> {
    if paths.is_empty() {
        return Err("paths is empty".into());
    }
    if paths.len() > MAX_FILES {
        return Err(format!("at most {MAX_FILES} files can be fitted at once").into());
    }
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        if !path.is_file() || kind_of(path) != Some(FileKind::Sample) {
            return Err(format!("{} is not an audio file", path.display()).into());
        }
        files.push(path.clone());
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()).then_with(|| a.cmp(b)));
    files.dedup();
    Ok(files)
}

/// The folder's audio files (not its subfolders), sorted by name.
pub(crate) fn list(folder: &Path) -> Result<Vec<PathBuf>, CommandError> {
    let entries = std::fs::read_dir(folder).map_err(|error| format!("could not read the folder: {error}"))?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || kind_of(&path) != Some(FileKind::Sample) {
            continue;
        }
        files.push(path);
        if files.len() > MAX_FILES {
            return Err(format!("the folder has more than {MAX_FILES} audio files; fit a smaller folder").into());
        }
    }
    if files.is_empty() {
        return Err("the folder has no audio files".into());
    }
    files.sort();
    Ok(files)
}

enum Skip {
    Cancelled,
    Reason(String),
}

impl From<Error> for Skip {
    fn from(error: Error) -> Self {
        if error == Error::Cancelled { Self::Cancelled } else { Self::Reason(error.to_string()) }
    }
}

/// Fit every file in turn; `progress(done, total)` returns false to cancel.
pub(crate) fn fit_folder(
    files: &[PathBuf],
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<FolderReport, Error> {
    let total = files.len();
    let mut entries = Vec::with_capacity(total);
    let mut fitted = Vec::new();
    for (index, path) in files.iter().enumerate() {
        if !progress(index, total) {
            return Err(Error::Cancelled);
        }
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut file = FolderFile { name, key: None, velocity: None, outcome: FileOutcome::Kept };
        match fit_file(path, &mut |_, _| progress(index, total)) {
            Ok(recording) => {
                file.key = Some(recording.key);
                fitted.push((index, recording));
            }
            Err(Skip::Cancelled) => return Err(Error::Cancelled),
            Err(Skip::Reason(reason)) => file.outcome = FileOutcome::Skipped(reason),
        }
        entries.push(file);
    }
    if !progress(total, total) {
        return Err(Error::Cancelled);
    }
    let recordings = assemble(fitted, &mut entries);
    Ok(FolderReport { files: entries, recordings, phrase: false })
}

/// Fit each held note of the phrase in `range` of `pcm` as its own recording at its key: the
/// root analysis's held notes, each from its start until the next note starts (its release
/// included) or the range ends. `progress(done, total)` returns false to cancel.
pub(crate) fn fit_phrase(
    pcm: &AudioData,
    range: Range<usize>,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<FolderReport, Error> {
    let root = suhina_instrument_analysis::analyze(
        &pcm.samples,
        pcm.sample_rate,
        pcm.channels,
        range.clone(),
        &mut |_, _| progress(0, 1),
    )?;
    let notes: Vec<_> = root.notes.iter().take(MAX_FILES).copied().collect();
    let total = notes.len() + 1;
    let rate = f64::from(pcm.sample_rate);
    let frame = |seconds: f32| range.start.saturating_add((f64::from(seconds) * rate) as usize).min(range.end);
    let mut entries = Vec::with_capacity(notes.len());
    let mut fitted = Vec::new();
    for (index, note) in notes.iter().enumerate() {
        if !progress(index + 1, total) {
            return Err(Error::Cancelled);
        }
        let start = frame(note.start_seconds);
        let end = notes.get(index + 1).map_or(range.end, |next| frame(next.start_seconds));
        let name = format!("note {} at {:.2} s", index + 1, note.start_seconds);
        let mut file = FolderFile { name, key: Some(note.root), velocity: None, outcome: FileOutcome::Kept };
        if start >= end {
            file.outcome = FileOutcome::Skipped("is empty".into());
        } else {
            let fit = modal::fit(&pcm.samples, pcm.sample_rate, pcm.channels, start..end, note.root, &mut |_, _| {
                progress(index + 1, total)
            });
            match fit {
                Ok(report) => match report.recording {
                    Some(recording) => fitted.push((index, recording)),
                    None => file.outcome = FileOutcome::Skipped(unfitted(report.outcome).into()),
                },
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) => file.outcome = FileOutcome::Skipped(error.to_string()),
            }
        }
        entries.push(file);
    }
    if !progress(total, total) {
        return Err(Error::Cancelled);
    }
    let recordings = assemble(fitted, &mut entries);
    Ok(FolderReport { files: entries, recordings, phrase: true })
}

/// Why a fit gave no recording.
fn unfitted(outcome: modal::Outcome) -> &'static str {
    match outcome {
        modal::Outcome::Silent => "is silent",
        modal::Outcome::TooShort => "is too short to fit",
        modal::Outcome::NoDecayingModes => "has no decaying resonances",
        modal::Outcome::Fitted => "its fit could not form a recording",
    }
}

/// One file: decode, propose its key from a stable single note, fit it at that key.
fn fit_file(path: &Path, progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<ModelRecording, Skip> {
    let audio = suhina_media::decode_file_prefix(path, PREFIX_SECONDS)
        .map_err(|error| Skip::Reason(format!("could not be decoded: {error}")))?;
    if !(1..=2).contains(&audio.channels) {
        return Err(Skip::Reason("has more than two channels".into()));
    }
    let frames = audio.frames();
    if frames == 0 {
        return Err(Skip::Reason("has no audio".into()));
    }
    let root = suhina_instrument_analysis::analyze(
        &audio.samples,
        audio.sample_rate,
        audio.channels,
        0..frames,
        &mut *progress,
    )?;
    let key = match (root.outcome, root.proposal) {
        (Outcome::Stable, Some(proposal)) => proposal.root,
        _ => return Err(Skip::Reason("has no stable single pitch".into())),
    };
    let report = modal::fit(&audio.samples, audio.sample_rate, audio.channels, 0..frames, key, &mut *progress)?;
    report.recording.ok_or_else(|| Skip::Reason(unfitted(report.outcome).into()))
}

/// Velocity layers by level within each key, then at most 32 recordings, evenly spaced over
/// the recordings ordered by key and velocity. Marks each file's velocity and outcome.
pub(crate) fn assemble(mut fitted: Vec<(usize, ModelRecording)>, files: &mut [FolderFile]) -> Vec<ModelRecording> {
    fitted.sort_by(|(a_index, a), (b_index, b)| {
        a.key.cmp(&b.key).then(a.level.total_cmp(&b.level)).then(a_index.cmp(b_index))
    });
    for layer in fitted.chunk_by_mut(|(_, a), (_, b)| a.key == b.key) {
        let layers = layer.len();
        for (i, (index, recording)) in layer.iter_mut().enumerate() {
            recording.velocity = if layers == 1 { modal::DEFAULT_VELOCITY } else { (i + 1) as f32 / layers as f32 };
            if let Some(file) = files.get_mut(*index) {
                file.velocity = Some(recording.velocity);
            }
        }
    }
    let count = fitted.len();
    let mut keep = vec![count <= MAX_RECORDINGS; count];
    if count > MAX_RECORDINGS {
        let last = (MAX_RECORDINGS - 1) as f64;
        for i in 0..MAX_RECORDINGS {
            let at = (i as f64 * (count - 1) as f64 / last).round() as usize;
            if let Some(slot) = keep.get_mut(at) {
                *slot = true;
            }
        }
    }
    fitted
        .into_iter()
        .zip(keep)
        .filter_map(|((index, recording), kept)| {
            if !kept && let Some(file) = files.get_mut(index) {
                file.outcome = FileOutcome::LeftOut;
            }
            kept.then_some(recording)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_dsp::modal::model::ModelMode;

    fn recording(key: u8, level: f64) -> ModelRecording {
        ModelRecording {
            key,
            source: 0,
            velocity: 0.8,
            level,
            modes: vec![ModelMode::plain(1.0, 0.5, 1.0)],
            attack_rate: 0,
            attack: Vec::new(),
            sustain: None,
            stereo: None,
            tail: None,
        }
    }

    fn files(count: usize) -> Vec<FolderFile> {
        (0..count)
            .map(|i| FolderFile { name: format!("{i}.wav"), key: None, velocity: None, outcome: FileOutcome::Kept })
            .collect()
    }

    #[test]
    fn files_of_one_key_are_velocity_layers_ordered_by_level() {
        let mut entries = files(4);
        let fitted =
            vec![(0, recording(60, 0.9)), (1, recording(72, 0.5)), (2, recording(60, 0.2)), (3, recording(60, 0.5))];
        let set = assemble(fitted, &mut entries);
        let layers: Vec<_> = set.iter().map(|r| (r.key, r.velocity, r.level)).collect();
        let third = 1.0 / 3.0;
        assert_eq!(layers, vec![(60, third, 0.2), (60, 2.0 * third, 0.5), (60, 1.0, 0.9), (72, 0.8, 0.5)]);
        assert_eq!(entries[0].velocity, Some(1.0));
        assert_eq!(entries[1].velocity, Some(0.8), "a key with one file stands for 0.8");
        assert_eq!(entries[2].velocity, Some(third));
        assert!(entries.iter().all(|f| f.outcome == FileOutcome::Kept));
        assert!(suhina_dsp::modal::model::ModelSet::new(set).is_ok(), "layers never duplicate a key and velocity");
    }

    #[test]
    fn more_than_32_recordings_keep_an_even_selection_across_the_keys() {
        let mut entries = files(40);
        let fitted: Vec<_> = (0..40).map(|i| (i, recording(30 + i as u8, 0.5))).collect();
        let set = assemble(fitted, &mut entries);
        assert_eq!(set.len(), MAX_RECORDINGS);
        assert_eq!(set.first().map(|r| r.key), Some(30), "the lowest key stays");
        assert_eq!(set.last().map(|r| r.key), Some(69), "the highest key stays");
        let left_out = entries.iter().filter(|f| f.outcome == FileOutcome::LeftOut).count();
        assert_eq!(left_out, 40 - MAX_RECORDINGS);
        assert!(set.windows(2).all(|w| w[0].key < w[1].key));
        assert!(suhina_dsp::modal::model::ModelSet::new(set).is_ok());
    }

    #[test]
    fn a_folder_lists_its_audio_files_by_name_and_refuses_an_empty_one() {
        let folder = std::env::temp_dir().join(format!("suhina-folder-fit-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        assert!(list(&folder).is_err(), "no audio files");
        for name in ["b.wav", "a.flac", "notes.txt", "nested/c.wav"] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        let names: Vec<_> =
            list(&folder).unwrap().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["a.flac", "b.wav"]);
        assert!(list(&folder.join("missing")).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn listed_files_from_anywhere_are_sorted_by_name_and_must_be_audio() {
        let folder = std::env::temp_dir().join(format!("suhina-files-fit-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("a")).unwrap();
        std::fs::create_dir_all(folder.join("b")).unwrap();
        for name in ["b/c4.wav", "a/c5.flac", "a/notes.txt"] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        let paths = [folder.join("b/c4.wav"), folder.join("a/c5.flac"), folder.join("b/c4.wav")];
        let names: Vec<_> =
            listed(&paths).unwrap().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["c4.wav", "c5.flac"], "sorted by name, each once");
        assert!(listed(&[folder.join("a/notes.txt")]).is_err());
        assert!(listed(&[folder.join("a/missing.wav")]).is_err());
        assert!(listed(&[]).is_err());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn unreadable_and_unpitched_files_are_skipped_with_a_reason() {
        let folder = std::env::temp_dir().join(format!("suhina-folder-fit-skip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("broken.wav"), b"not audio").unwrap();
        let mut state = 1u32;
        let noise: Vec<f32> = (0..48000)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (f64::from(state >> 8) / f64::from(1u32 << 24) - 0.5) as f32 * 0.5
            })
            .collect();
        suhina_test_support::write_wav(
            &folder.join("noise.wav"),
            48000,
            1,
            suhina_test_support::WavFormat::Float32,
            &noise,
        );
        let paths = list(&folder).unwrap();
        let report = fit_folder(&paths, &mut |_, _| true).unwrap();
        assert!(report.recordings.is_empty());
        assert_eq!(report.files.len(), 2);
        assert!(report.files.iter().all(|f| f.outcome.reason().is_some()), "{report:?}");
        assert!(fit_folder(&paths, &mut |_, _| false).is_err_and(|e| e == Error::Cancelled));
        let _ = std::fs::remove_dir_all(&folder);
    }
}
