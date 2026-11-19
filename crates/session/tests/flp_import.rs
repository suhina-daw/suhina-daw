// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Authored import fixtures exercised through the session boundary and offline playback.
#![allow(clippy::unwrap_used)] // fixture helpers outside test functions

use std::sync::Arc;
use suhina_engine::{AudioData, AudioPool, RenderInputs, RenderOptions, Renderer};
use suhina_model::*;
use suhina_session::{OpenOptions, Projects};
use suhina_test_support::{WavFormat, write_wav};

use crate::temp_dir::TempDir;

fn event(out: &mut Vec<u8>, id: u8, data: &[u8]) {
    out.push(id);
    if id >= 192 {
        let mut n = data.len();
        loop {
            let byte = (n & 127) as u8;
            n >>= 7;
            out.push(byte | if n > 0 { 128 } else { 0 });
            if n == 0 {
                break;
            }
        }
    }
    out.extend_from_slice(data);
}

fn text(out: &mut Vec<u8>, id: u8, value: &str) {
    event(out, id, &value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<_>>());
}

fn events() -> Vec<u8> {
    let mut out = Vec::new();
    event(&mut out, 199, b"26.1.0\0");
    // Ten beats per second keeps the playback regressions short without changing their beats.
    event(&mut out, 156, &600000u32.to_le_bytes());
    out
}

fn file(events: &[u8]) -> Vec<u8> {
    let mut out = b"FLhd".to_vec();
    out.extend(6u32.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out.extend(3u16.to_le_bytes());
    out.extend(96u16.to_le_bytes());
    out.extend(b"FLdt");
    out.extend((events.len() as u32).to_le_bytes());
    out.extend(events);
    out
}

fn placement(out: &mut Vec<u8>, source: u16, length: u32, offset: u32) {
    let mut clip = [0u8; 32];
    clip[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
    clip[6..8].copy_from_slice(&source.to_le_bytes());
    clip[8..12].copy_from_slice(&length.to_le_bytes());
    clip[12..14].copy_from_slice(&499u16.to_le_bytes());
    clip[24..28].copy_from_slice(&offset.to_le_bytes());
    clip[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    event(out, 233, &clip);
}

fn audio_source(out: &mut Vec<u8>, id: u16, sample: &str, mixer: u16, stretch: bool) {
    event(out, 64, &id.to_le_bytes());
    event(out, 21, &[4]);
    event(out, 104, &mixer.to_le_bytes());
    text(out, 196, sample);
    if stretch {
        let mut settings = [0u8; 112];
        settings[96..100].copy_from_slice(&(2f32 * 192.).to_le_bytes());
        settings[108..112].copy_from_slice(&5u32.to_le_bytes());
        event(out, 215, &settings);
    }
    event(out, 51, &[0]);
}

fn region(project: &Project) -> &AudioRegion {
    project
        .contents
        .iter()
        .flat_map(|c| c.lanes.iter())
        .find_map(|lane| if let LaneData::Audio(region) = &lane.data { Some(region) } else { None })
        .unwrap()
}

#[test]
fn imported_headers_and_stretch_obey_frontend_media_trust_without_audio_decoding() {
    let root = TempDir::new("import-media-trust");
    let folder = root.join("FL Studio/Projects/Song");
    let samples = root.join("Downloads");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&samples).unwrap();
    let sample = samples.join("loop.wav");
    write_wav(&sample, 8000, 1, WavFormat::Int16, &[0.25; 4000]);
    let path = folder.join("authored.flp");
    for stored in [sample.to_string_lossy().into_owned(), "%FLStudioDownloadsData%/loop.wav".into()] {
        let mut bytes = events();
        audio_source(&mut bytes, 0, &stored, 0, true);
        placement(&mut bytes, 0, 192, 0);
        std::fs::write(&path, file(&bytes)).unwrap();

        // This is the same open path used by CLI and headless MCP. No decoded audio is needed
        // for an importer to read a header, so the policy must already apply during import.
        let (restricted, _) = Projects::new().open_path(&path, OpenOptions { audio: false }).unwrap();
        let metadata = region(restricted.project());
        assert_eq!((metadata.duration_seconds, metadata.sample_rate, metadata.channels), (None, None, None));
        assert!(!restricted.project().arrangement.lanes[0].clips[0].audio.warped);

        for desktop in [false, true] {
            let mut allowed = Projects::new();
            if desktop {
                allowed.read_local_audio(true);
            } else {
                allowed.file_library_mut().add_folder(&samples).unwrap();
            }
            let (opened, _) = allowed.open_path(&path, OpenOptions { audio: false }).unwrap();
            let metadata = region(opened.project());
            assert_eq!(metadata.duration_seconds, Some(0.5), "{stored}, desktop={desktop}");
            assert_eq!(metadata.sample_rate, Some(8000));
            assert!(opened.project().arrangement.lanes[0].clips[0].audio.warped);
        }

        // A resolver's candidate does not grant authority by itself.
        let imported = suhina_flp::import_file_with_samples(&path, &suhina_media::Trusted::default(), |files| {
            vec![Some(sample.clone()); files.len()]
        })
        .unwrap();
        assert_eq!(region(&imported.project).duration_seconds, None);
    }
    // Project-local media remains readable with the restrictive policy.
    std::fs::copy(&sample, folder.join("loop.wav")).unwrap();
    let (local, _) = Projects::new().open_path(&path, OpenOptions { audio: false }).unwrap();
    assert_eq!(region(local.project()).duration_seconds, Some(0.5));
}

// Render only the frames each assertion needs; the whole-arrangement helper also renders
// two seconds of device tails, which none of these regressions exercise.
fn render(project: &Project, pool: &AudioPool, frames: usize) -> Vec<f32> {
    let mut renderer = Renderer::build(
        project,
        RenderInputs::from_audio(pool, suhina_session::devices()),
        8000,
        RenderOptions::PLAYING,
    )
    .unwrap();
    renderer.set_offline(true);
    renderer.set_follow_loop(false);
    let mut out = vec![0.; frames * 2];
    for block in out.chunks_mut(128) {
        renderer.process(block);
    }
    out
}

fn peak(out: &[f32], from: f64, to: f64) -> f32 {
    out.get((from * 16000.) as usize..(to * 16000.) as usize)
        .unwrap_or_default()
        .iter()
        .map(|s| s.abs())
        .fold(0., f32::max)
}

#[test]
fn an_untrimmed_pattern_plays_late_notes_but_an_explicit_window_stays_trimmed() {
    let mut base = events();
    event(&mut base, 65, &1u16.to_le_bytes());
    let mut notes = Vec::new();
    for at in [0u32, 192] {
        let mut note = [0u8; 24];
        note[..4].copy_from_slice(&at.to_le_bytes());
        note[8..12].copy_from_slice(&48u32.to_le_bytes());
        note[12] = 60;
        note[16] = 120;
        note[20] = 64;
        note[21] = 128;
        notes.extend(note);
    }
    event(&mut base, 224, &notes);
    event(&mut base, 64, &0u16.to_le_bytes());
    event(&mut base, 21, &[0]);
    text(&mut base, 196, "note.wav");
    event(&mut base, 135, &60u32.to_le_bytes());
    event(&mut base, 51, &[0]);
    let mut pool = AudioPool::new();
    pool.files
        .insert("note.wav".into(), Arc::new(AudioData { sample_rate: 8000, channels: 1, samples: vec![0.25; 160] }));
    for (offset, expected_length, late_note) in [(u32::MAX, 2.5, true), (0, 1., false), (192, 1., false)] {
        let mut bytes = base.clone();
        placement(&mut bytes, 0x5001, 96, offset);
        let project = suhina_flp::import_bytes(&file(&bytes)).unwrap().project;
        let clip = &project.arrangement.lanes[0].clips[0];
        assert_eq!(clip.length, expected_length);
        assert_eq!(clip.offset, if offset == u32::MAX { 0. } else { f64::from(offset) / 96. });
        let out = render(&project, &pool, 2400);
        assert!(peak(&out, 0.004, 0.016) > 0.01);
        assert_eq!(peak(&out, 0.204, 0.216) > 0.01, late_note);
        let restored = suhina_project_file::from_json(&suhina_project_file::to_json(&project)).unwrap();
        assert_eq!(render(&restored, &pool, 2400), out);
    }
}

fn keyed_project() -> Project {
    let mut bytes = events();
    audio_source(&mut bytes, 0, "key.wav", 1, false);
    audio_source(&mut bytes, 1, "pad.wav", 2, false);
    // The key fader stays at zero for two beats, then jumps to unity for two beats.
    event(&mut bytes, 64, &2u16.to_le_bytes());
    event(&mut bytes, 21, &[5]);
    let mut curve = vec![0u8; 21];
    curve[17..21].copy_from_slice(&2u32.to_le_bytes());
    for (delta, value) in [(0f64, 0f64), (2., 0.8)] {
        curve.extend(delta.to_le_bytes());
        curve.extend(value.to_le_bytes());
        curve.extend([0; 4]);
        curve.extend([2, 0, 0, 0]);
    }
    event(&mut bytes, 234, &curve);
    event(&mut bytes, 51, &[0]);
    let mut link = [0u8; 20];
    link[2..4].copy_from_slice(&2u16.to_le_bytes());
    link[8..12].copy_from_slice(&0x70401fc0u32.to_le_bytes());
    event(&mut bytes, 227, &link);
    for source in 0..3 {
        placement(&mut bytes, source, 384, 0);
    }
    // An authored wrapper with a valid empty VST2 chunk and input port 1. Playback below
    // substitutes a native detector, retaining the importer's key assignment and routing.
    let mut wrapper = 10u32.to_le_bytes().to_vec();
    let mut record = |id: u32, data: &[u8]| {
        wrapper.extend(id.to_le_bytes());
        wrapper.extend((data.len() as u64).to_le_bytes());
        wrapper.extend(data);
    };
    record(51, &0x12345678u32.to_le_bytes());
    record(54, b"Detector");
    let mut state = (-9i32).to_le_bytes().to_vec();
    state.push(13);
    state.extend((-2i32).to_le_bytes());
    state.extend([0; 12]);
    record(53, &state);
    record(31, &[0i32, 1, 0, 1, 1, 0].into_iter().flat_map(i32::to_le_bytes).collect::<Vec<_>>());
    for bus in 0..4 {
        event(&mut bytes, 236, &[0, 0, 0, 0, 12, 0, 0, 0]);
        if bus == 1 {
            event(&mut bytes, 235, &[0, 0, 1]);
        } else if bus == 2 {
            text(&mut bytes, 201, "Fruity Wrapper");
            event(&mut bytes, 213, &wrapper);
            event(&mut bytes, 98, &0u16.to_le_bytes());
            event(&mut bytes, 235, &[1, 0, 0]);
        }
    }
    let mut level = [0u8; 12];
    level[4] = 2;
    level[5] = 0x20;
    level[6..8].copy_from_slice(&0x7040u16.to_le_bytes());
    event(&mut bytes, 225, &level);
    let mut project = suhina_flp::import_bytes(&file(&bytes)).unwrap().project;
    let detector = project
        .channels
        .iter_mut()
        .flat_map(|c| &mut c.devices)
        .find(|d| matches!(d.kind, DeviceKind::Vst2 { .. }))
        .unwrap();
    assert_eq!(detector.inputs.len(), 1);
    detector.kind = DeviceKind::Builtin("compressor".into());
    detector.state = None;
    detector.params = [("threshold", -40.), ("ratio", 20.), ("knee", 0.), ("attack", 0.0001)]
        .into_iter()
        .map(|(id, value)| DeviceParam::new(id, value))
        .collect();
    project.validate().unwrap();
    project
}

#[test]
fn a_sidechain_only_source_follows_imported_fader_automation_and_fader_edits() {
    let mut project = keyed_project();
    let mut pool = AudioPool::new();
    for (name, level) in [("key.wav", 0.5), ("pad.wav", 0.001)] {
        // A 500 Hz key survives the compressor's detector high-pass; a DC key would
        // exercise only the transient when its fader changes. The attack above is in seconds.
        let samples =
            (0..3200).map(|frame| if name == "key.wav" && frame % 16 >= 8 { -level } else { level }).collect();
        pool.files.insert(name.into(), Arc::new(AudioData { sample_rate: 8000, channels: 1, samples }));
    }
    let out = render(&project, &pool, 3200);
    let quiet_key = peak(&out, 0.04, 0.08);
    let loud_key = peak(&out, 0.24, 0.28);
    assert!(quiet_key > 0.0001);
    assert!(loud_key < quiet_key * 0.5, "automated key: {loud_key} vs {quiet_key}");
    project.channel_mut(ChannelId(0)).unwrap().automation.clear();
    project.channel_mut(ChannelId(0)).unwrap().volume = 0.;
    let zero = render(&project, &pool, 800);
    project.channel_mut(ChannelId(0)).unwrap().volume = 1.;
    let one = render(&project, &pool, 800);
    assert!(peak(&one, 0.04, 0.08) < peak(&zero, 0.04, 0.08) * 0.5);
    // Remove the pad: the key alone never reaches Main, even with its fader at unity.
    pool.files
        .insert("pad.wav".into(), Arc::new(AudioData { sample_rate: 8000, channels: 1, samples: vec![0.; 3200] }));
    assert!(render(&project, &pool, 800).iter().all(|sample| *sample == 0.));
}
