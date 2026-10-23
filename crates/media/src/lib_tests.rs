// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_engine::model::*;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-media-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("set")).unwrap();
    dir
}

fn write_wav(path: &Path, channels: u16, frames: usize) {
    let spec =
        hound::WavSpec { channels, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
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
        assert!(locate("link.wav", None, &dir.join("set"), &Trusted::default()).is_none());
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(dir.join("outside.wav"), dir.join("set/link.wav")).is_ok() {
            assert!(locate("link.wav", None, &dir.join("set"), &Trusted::default()).is_none());
        }
        assert!(locate(r"\\untrusted-server\share\sound.wav", None, &dir.join("set"), &Trusted::default()).is_none());
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
    let (pool, warnings) = load_project_audio(&project_with_audio("moved/away.wav", Some(absolute)), &dir.join("set"));
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
    assert_eq!(locate("here.wav", None, &dir.join("set"), &Trusted::default()), Some(dir.join("set").join("here.wav")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_region_names_no_file_so_nothing_is_decoded_or_missed() {
    // An empty audio clip (clip.create on an audio track) has no file yet.
    let (pool, warnings) = load_project_audio(&project_with_audio("", None), Path::new("nowhere"));
    assert!(pool.files.is_empty() && warnings.is_empty(), "{warnings:?}");
    assert!(missing_audio(&project_with_audio("", None), Path::new("nowhere")).is_empty());
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
