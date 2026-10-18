// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio files for a project: finds the files its audio clips refer to and decodes them into
//! memory with symphonia (MPL-2.0), so the engine can play them without touching the disk.

pub mod info;
pub mod midi;
pub mod stems;
pub mod tempo;

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

/// Decodes every audio file the project uses, on all cores. `project_dir` is the folder the
/// project file is in; references are resolved only inside it, without following links.
/// External references require an explicit user load or relink. Files that cannot be found or decoded are reported in the returned warnings and
/// left out.
pub fn decode_project_audio(project: &Project, project_dir: &Path) -> (AudioPool, Vec<String>) {
    // Each file once (once more when a Stems file is played whole as well as by track), with
    // where to look for it and whether all of its audio tracks are wanted.
    let mut wanted: Vec<(String, Vec<PathBuf>, bool)> = Vec::new();
    for device in project.channels.iter().flat_map(|c| &c.devices) {
        if let Some(file) = device.sample_file()
            && !wanted.iter().any(|(key, _, _)| key == file)
        {
            let candidates = locate(file, device.sample_absolute_file(), project_dir).into_iter().collect();
            wanted.push((file.into(), candidates, false));
        }
    }
    for content in &project.contents {
        for lane in &content.lanes {
            let LaneData::Audio(region) = &lane.data else { continue };
            let streams = region.stream.is_some();
            if wanted.iter().any(|(file, _, s)| *file == region.file && *s == streams) {
                continue;
            }
            let candidates = locate(&region.file, region.absolute_file.as_deref(), project_dir).into_iter().collect();
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

/// A project-named file inside its folder, with no parent traversal or symbolic links.
pub fn locate(file: &str, absolute: Option<&str>, project_dir: &Path) -> Option<PathBuf> {
    let root = std::path::absolute(project_dir).ok()?;
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
    let mut out: Vec<MissingAudio> = Vec::new();
    for device in project.channels.iter().flat_map(|c| &c.devices) {
        if let Some(file) = device.sample_file()
            && locate(file, device.sample_absolute_file(), project_dir).is_none()
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
            if found.contains(&region.file.as_str()) || out.iter().any(|m| m.file == region.file) {
                continue;
            }
            if locate(&region.file, region.absolute_file.as_deref(), project_dir).is_some() {
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
mod tests {
    use super::*;
    use suhina_engine::model::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("suhina-media-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("set")).unwrap();
        dir
    }

    fn write_wav(path: &Path, channels: u16, frames: usize) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames * usize::from(channels) {
            w.write_sample(if i % 2 == 0 { 16_384i16 } else { -16_384 }).unwrap();
        }
        w.finalize().unwrap();
    }

    fn project_with_audio(file: &str, absolute: Option<String>) -> Project {
        Project {
            contents: vec![ClipContent {
                id: ContentId(1),
                name: "a".into(),
                length: 4.0,
                lanes: vec![ContentLane {
                    target: None,
                    data: LaneData::Audio(AudioRegion { absolute_file: absolute, ..AudioRegion::new(file) }),
                }],
                extensions: Extensions::default(),
                scale: None,
            }],
            ..Project::default()
        }
    }

    #[test]
    fn decode_budgets_reject_hostile_formats_and_growth_before_allocating() {
        assert!(decode_limit(1_000_000, 64).is_err());
        assert!(decode_limit(999, 1).is_err());
        assert_eq!(decode_limit(384000, 32).unwrap(), MAX_DECODED_SAMPLES);
        let mut samples = vec![0.; 8];
        let capacity = samples.capacity();
        assert!(reserve_audio(&mut samples, 9, 16).is_err());
        assert!(reserve_audio(&mut samples, usize::MAX, 16).is_err());
        assert_eq!(samples.capacity(), capacity);
        reserve_audio(&mut samples, 8, 16).unwrap();
        assert!(samples.capacity() <= 16);
    }

    #[test]
    fn project_sources_cannot_escape_by_absolute_path_parent_or_symlink() {
        let dir = temp_dir("confined");
        write_wav(&dir.join("outside.wav"), 1, 16);
        for name in ["../outside.wav".to_string(), dir.join("outside.wav").display().to_string()] {
            let mut project = project_with_audio(&name, Some(name.clone()));
            project.channels.push(Channel::new(ChannelId(1), "Sampler"));
            project.channels[0].devices.push(Device {
                name: "Sampler".into(),
                kind: DeviceKind::Builtin("sampler".into()),
                enabled: true,
                state: None,
                params: vec![],
                automation: vec![],
                extensions: Extensions::default(),
            });
            project.channels[0].devices[0].set_sample_file(&name);
            let (pool, warnings) = load_project_audio(&project, &dir.join("set"));
            assert!(pool.files.is_empty() && !warnings.is_empty());
            assert!(!missing_audio(&project, &dir.join("set")).is_empty());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("outside.wav"), dir.join("set/link.wav")).unwrap();
            assert!(locate("link.wav", None, &dir.join("set")).is_none());
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_file(dir.join("outside.wav"), dir.join("set/link.wav")).is_ok() {
                assert!(locate("link.wav", None, &dir.join("set")).is_none());
            }
            assert!(locate(r"\\untrusted-server\share\sound.wav", None, &dir.join("set")).is_none());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn decodes_wav_to_interleaved_f32() {
        let dir = temp_dir("decode");
        let path = dir.join("tone.wav");
        write_wav(&path, 2, 1000);
        let data = decode_file(&path).unwrap();
        assert_eq!((data.sample_rate, data.channels, data.frames()), (44_100, 2, 1000));
        assert!((data.samples[0] - 0.5).abs() < 1e-3 && (data.samples[1] + 0.5).abs() < 1e-3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_paths_stay_inside_the_project_folder() {
        let dir = temp_dir("resolve");
        write_wav(&dir.join("shared.wav"), 1, 100);
        // A shared project cannot read a sibling folder.
        let (pool, warnings) = load_project_audio(&project_with_audio("../shared.wav", None), &dir.join("set"));
        assert!(pool.files.is_empty() && warnings.len() == 1);
        write_wav(&dir.join("set/shared.wav"), 1, 100);

        // Relative path broken, absolute path still good.
        let absolute = dir.join("set/shared.wav").display().to_string();
        let (pool, warnings) =
            load_project_audio(&project_with_audio("moved/away.wav", Some(absolute)), &dir.join("set"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(pool.files.contains_key("moved/away.wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_files_are_listed_with_their_clips() {
        let dir = std::env::temp_dir().join(format!("suhina-media-missing-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("set")).unwrap();
        write_wav(&dir.join("set").join("here.wav"), 1, 100);
        let mut project =
            project_with_audio("gone.wav", Some(dir.join("elsewhere").join("gone.wav").display().to_string()));
        let missing = missing_audio(&project, &dir.join("set"));
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].file, "gone.wav");
        assert_eq!(missing[0].last_path, dir.join("elsewhere").join("gone.wav"));
        assert_eq!(missing[0].clips, project.arrangement.lanes.iter().map(|l| l.clips.len()).sum::<usize>());
        project = project_with_audio("here.wav", None);
        assert!(missing_audio(&project, &dir.join("set")).is_empty());
        assert_eq!(locate("here.wav", None, &dir.join("set")), Some(dir.join("set").join("here.wav")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_and_broken_files_are_warnings() {
        let dir = temp_dir("broken");
        std::fs::write(dir.join("set").join("noise.wav"), b"definitely not audio").unwrap();
        let (pool, warnings) = load_project_audio(&project_with_audio("gone.wav", None), &dir.join("set"));
        assert!(pool.files.is_empty() && warnings[0].contains("not found"));
        let (pool, warnings) = load_project_audio(&project_with_audio("noise.wav", None), &dir.join("set"));
        assert!(pool.files.is_empty() && warnings.len() == 1, "{warnings:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stems_file_is_decoded_once_with_a_key_per_track() {
        let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data");
        let mut project = project_with_audio("tones-aac.stem.mp4", None);
        let lane = project.contents[0].lanes[0].clone();
        project.contents[0].lanes.clear();
        // Stems 1 and 3, and the same file played whole (its first track, the master).
        for stream in [Some(1), Some(3), None] {
            let mut lane = lane.clone();
            if let LaneData::Audio(region) = &mut lane.data {
                region.stream = stream;
            }
            project.contents[0].lanes.push(lane);
        }
        let (pool, warnings) = load_project_audio(&project, &data);
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut keys: Vec<_> = pool.files.keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "tones-aac.stem.mp4",
                "tones-aac.stem.mp4#0",
                "tones-aac.stem.mp4#1",
                "tones-aac.stem.mp4#2",
                "tones-aac.stem.mp4#3",
                "tones-aac.stem.mp4#4"
            ]
        );
        assert_eq!(pool.files["tones-aac.stem.mp4#1"].frames(), 44_100);
    }
}
