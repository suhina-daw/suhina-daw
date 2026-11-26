// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use super::*;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data").join(name)
}

/// Dominant frequency of the left channel, by counting upward zero crossings.
fn frequency(audio: &AudioData) -> f64 {
    let left: Vec<f32> = audio.samples.iter().step_by(usize::from(audio.channels)).copied().collect();
    let crossings = left.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    crossings as f64 / (left.len() as f64 / f64::from(audio.sample_rate))
}

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

#[test]
fn names_are_recognised_in_any_case() {
    assert!(is_stems_path(Path::new("a/Track.STEM.mp4")));
    assert!(is_stems_path(Path::new("track.stem.m4a")));
    assert!(!is_stems_path(Path::new("track.mp4")));
    assert!(!is_stems_path(Path::new("stem.mp4")));
    assert!(!is_stems_path(Path::new("track.stem.wav")));
}

#[test]
fn aac_file_decodes_every_track_trimmed_to_its_edit_list() {
    let streams = decode_streams(&data("tones-aac.stem.mp4")).unwrap();
    assert_eq!(streams.len(), 5);
    for (audio, expected) in streams.iter().zip([110.0, 220.0, 440.0, 880.0, 1760.0]) {
        assert_eq!((audio.sample_rate, audio.channels), (44_100, 2));
        // Exactly one second: the 1024 frames of encoder priming and the padding are gone.
        assert_eq!(audio.frames(), 44_100);
        assert!((frequency(audio) - expected).abs() < expected * 0.02, "{} Hz, wanted {expected}", frequency(audio));
    }
}

#[test]
fn trimming_puts_the_sound_at_the_start() {
    // Without the edit list the first 1024 frames would be the decoder's warm-up (near silence).
    let streams = decode_streams(&data("tones-aac.stem.mp4")).unwrap();
    let start_peak = streams[1].samples[..400].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(start_peak > 0.05, "{start_peak}");
}

#[test]
fn alac_file_decodes_losslessly_with_base64_metadata() {
    let path = data("tones-alac.stem.m4a");
    let streams = decode_streams(&path).unwrap();
    assert_eq!(streams.len(), 5);
    assert!(streams.iter().all(|s| s.sample_rate == 22_050 && matches!(s.frames(), 5_512 | 5_513)));
    let info = read_info(&path).unwrap();
    assert!(info.has_metadata);
    assert_eq!(info.stems[3].name, "Vox");
}

#[test]
fn metadata_gives_names_colors_and_mastering() {
    let info = read_info(&data("tones-aac.stem.mp4")).unwrap();
    assert_eq!(info.tracks, 5);
    let names: Vec<_> = info.stems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Drums", "Bass", "Other", "Vox"]);
    assert_eq!(info.stems[0].color, Some([0xFD, 0x4A, 0x4A]));
    let c = info.compressor.unwrap();
    assert!(c.enabled);
    assert_eq!((c.ratio, c.threshold, c.hp_cutoff, c.dry_wet), (Some(4.0), Some(-6.0), Some(120.0), Some(80.0)));
    assert_eq!((c.attack, c.release, c.input_gain, c.output_gain), (Some(0.005), Some(0.25), Some(0.5), Some(0.6)));
    let l = info.limiter.unwrap();
    assert!(l.enabled);
    assert_eq!((l.threshold, l.ceiling, l.release), (Some(-1.0), Some(-0.3), Some(0.05)));
}

#[test]
fn a_file_without_metadata_gets_numbered_stems() {
    let info = read_info(&data("tones-plain.stem.mp4")).unwrap();
    assert!(!info.has_metadata);
    let names: Vec<_> = info.stems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Stem 1", "Stem 2", "Stem 3", "Stem 4"]);
    assert!(info.compressor.is_none() && info.limiter.is_none());
}

#[test]
fn an_ordinary_audio_file_is_not_a_stems_file() {
    let dir = std::env::temp_dir().join(format!("suhina-stems-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // One audio track only.
    let one = dir.join("one.stem.mp4");
    let moov = bx(b"moov", &bx(b"trak", &bx(b"mdia", &bx(b"hdlr", b"\0\0\0\0\0\0\0\0soun"))));
    std::fs::write(&one, moov).unwrap();
    assert!(read_info(&one).unwrap_err().contains("at least one stem"));
    // Not MP4 at all, and a box that claims to run past the end.
    let junk = dir.join("junk.stem.mp4");
    std::fs::write(&junk, b"RIFF....WAVEfmt ").unwrap();
    assert!(read_info(&junk).is_err());
    std::fs::write(&junk, [0, 0, 0x10, 0, b'm', b'o', b'o', b'v', 1, 2]).unwrap();
    assert!(read_info(&junk).is_err());
    std::fs::write(&junk, []).unwrap();
    assert!(read_info(&junk).is_err());
    assert!(decode_streams(&junk).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_boxes_never_panic() {
    // Sizes that are zero, too small, huge or past the end, at every nesting level.
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![0, 0, 0, 7, b't', b'r', b'a', b'k'],
        vec![0, 0, 0, 1, b't', b'r', b'a', b'k', 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        vec![0xFF, 0xFF, 0xFF, 0xFF, b't', b'r', b'a', b'k'],
        bx(b"trak", &bx(b"tkhd", &[1])),
        bx(b"trak", &[bx(b"tkhd", &[0; 3]), bx(b"mdia", &bx(b"hdlr", b"\0\0\0\0\0\0\0\0soun"))].concat()),
        bx(b"udta", &bx(b"stem", &[0xFF; 40])),
        bx(b"trak", &bx(b"edts", &bx(b"elst", &[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]))),
    ];
    for case in cases {
        let _ = parse_moov(&case).and_then(|b| info_from(&b));
    }
    let mut noise = 0x1234_5678u32;
    for _ in 0..2000 {
        let bytes: Vec<u8> = (0..64)
            .map(|_| {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                (noise >> 24) as u8
            })
            .collect();
        let _ = parse_moov(&bytes);
    }
}

#[test]
fn edit_lists_of_both_versions_are_read() {
    // Version 0: one empty edit (a delay) then 2 s of media from 2112.
    let mut v0 = vec![0, 0, 0, 0, 0, 0, 0, 2];
    v0.extend_from_slice(&[0, 0, 0x03, 0xE8, 0xFF, 0xFF, 0xFF, 0xFF, 0, 1, 0, 0]);
    v0.extend_from_slice(&[0, 0, 0x07, 0xD0, 0, 0, 0x08, 0x40, 0, 1, 0, 0]);
    assert_eq!(first_edit(&v0, 1000, 48_000), Some(Edit { skip: 2112, seconds: Some(2.0), timescale: 48_000 }));
    // Version 1, length 0 (to the end).
    let mut v1 = vec![1, 0, 0, 0, 0, 0, 0, 1];
    v1.extend_from_slice(&[0; 8]);
    v1.extend_from_slice(&1024u64.to_be_bytes());
    v1.extend_from_slice(&[0, 1, 0, 0]);
    assert_eq!(first_edit(&v1, 600, 44_100), Some(Edit { skip: 1024, seconds: None, timescale: 44_100 }));
    // A count that claims more entries than there are.
    assert_eq!(first_edit(&[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF], 1000, 44_100), None);
}

#[test]
fn trimming_is_bounded_by_what_was_decoded() {
    let mut samples = vec![1.0f32; 20];
    trim(&mut samples, 10, 2, Edit { skip: 100, seconds: Some(5.0), timescale: 10 });
    assert!(samples.is_empty());
    let mut samples: Vec<f32> = (0..20).map(|i| i as f32).collect();
    trim(&mut samples, 10, 2, Edit { skip: 2, seconds: Some(0.5), timescale: 10 });
    assert_eq!(samples, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0]);
    let mut samples = vec![0.0f32; 8];
    trim(&mut samples, 10, 2, Edit { skip: 0, seconds: Some(f64::NAN), timescale: 0 });
    assert_eq!(samples.len(), 8);
}

#[test]
fn metadata_text_is_cleaned() {
    assert_eq!(parse_color("#00e8E8"), Some([0, 0xE8, 0xE8]));
    assert_eq!(parse_color("00E8E8"), None);
    assert_eq!(parse_color("#00E8E"), None);
    assert_eq!(parse_color("#ééé"), None);
    assert_eq!(clean_name(" Dr\u{0}ums\n "), "Drums");
    assert_eq!(clean_name(&"x".repeat(500)).len(), 64);
    assert!(parse_json(b"{\"stems\": []}\0\0").is_some());
    assert!(parse_json(b"[1, 2]").is_none());
    assert!(parse_json(b"not json").is_none());
}

#[test]
fn mastering_settings_become_compressor_and_limiter_values() {
    let info = read_info(&data("tones-aac.stem.mp4")).unwrap();
    let devices = mastering_devices(&info);
    assert_eq!(devices.len(), 2);
    let compressor = &devices[0];
    assert_eq!((compressor.id, compressor.enabled), ("compressor", true));
    let value = |d: &DeviceSetting, id: &str| d.params.iter().find(|(p, _)| *p == id).map(|(_, v)| *v);
    assert_eq!(value(compressor, "ratio"), Some(4.0));
    assert_eq!(value(compressor, "threshold"), Some(-6.0));
    assert_eq!(value(compressor, "attack"), Some(0.005));
    assert_eq!(value(compressor, "sidechainHighPass"), Some(120.0));
    assert_eq!(value(compressor, "mix"), Some(0.8));
    assert_eq!(value(compressor, "input"), Some(0.0));
    assert!((value(compressor, "output").unwrap() - 2.4).abs() < 1e-9);
    let limiter = &devices[1];
    assert_eq!(
        (limiter.id, value(limiter, "ceiling"), value(limiter, "threshold")),
        ("limiter", Some(-0.3), Some(-1.0))
    );

    // Every id and value is one the devices accept.
    for device in &devices {
        let info = match device.id {
            "compressor" => &suhina_device_compressor::INFO,
            _ => &suhina_device_limiter::INFO,
        };
        for (id, value) in &device.params {
            let index = info.param_index(id).unwrap_or_else(|| panic!("{} has no {id}", device.id));
            let param = &info.params[index];
            assert_eq!(param.clamp(*value), *value, "{}.{id} = {value} is out of range", device.id);
        }
    }

    // Hostile values are kept in range, and a file without metadata has no devices.
    let wild = StemsInfo {
        compressor: Some(CompressorSettings {
            ratio: Some(1e9),
            threshold: Some(40.0),
            dry_wet: Some(-5.0),
            ..Default::default()
        }),
        limiter: Some(LimiterSettings { ceiling: Some(f64::NAN), release: Some(-1.0), ..Default::default() }),
        ..info
    };
    let devices = mastering_devices(&wild);
    assert_eq!(value(&devices[0], "ratio"), Some(20.0));
    assert_eq!(value(&devices[0], "threshold"), Some(0.0));
    assert_eq!(value(&devices[0], "mix"), Some(0.0));
    assert_eq!(value(&devices[1], "ceiling"), None);
    assert!(mastering_devices(&read_info(&data("tones-plain.stem.mp4")).unwrap()).is_empty());
}
