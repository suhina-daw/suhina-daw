// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests against the local, uncommitted Live Set corpus (`corpus/als`, see
//! `docs/spec/als-corpus.md`). Run with `cargo test -p suhina-als --features corpus`.
#![cfg(feature = "corpus")]
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;

use suhina_als::{Import, read_file};
use suhina_model::LaneData;

fn open(name: &str) -> Import {
    let path: PathBuf =
        [env!("CARGO_MANIFEST_DIR"), "..", "..", "corpus", "als", "00-base Project", &format!("{name}.als")]
            .iter()
            .collect();
    assert!(path.exists(), "{} is missing; author the corpus first (docs/spec/als-corpus.md)", path.display());
    read_file(&path).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn every_series_1_file_imports() {
    for name in [
        "00-base",
        "00b-base-resaved",
        "01-tempo-128",
        "02-midi-track",
        "03-midi-clip-empty",
        "04-midi-clip-one-note",
        "05-note-moved",
        "06-note-velocity",
        "07-note-key",
        "08-clip-length-2-bars",
        "09-clip-loop-off",
        "10-clip-name-color",
        "11-insert-scene",
        "12-session-clip",
        "13-second-session-clip",
        "14-arrangement-moved",
        "15-close-piano-roll",
        "16-extend-mixer",
        "17-track-volume-pan",
        "18-track-mute",
        "19-return-send",
        "20-clip-length-2-bars",
    ] {
        open(name);
    }
}

#[test]
fn tempo() {
    assert_eq!(open("00b-base-resaved").project.transport.tempo_bpm, 120.0);
    assert_eq!(open("01-tempo-128").project.transport.tempo_bpm, 128.0);
}

#[test]
fn arrangement_clip_moved_to_bar_3() {
    let p = open("14-arrangement-moved").project;
    let lane = p.arrangement.lanes.iter().find(|l| l.name == "2-MIDI").unwrap();
    assert_eq!(lane.clips.len(), 1);
    let clip = &lane.clips[0];
    assert_eq!((clip.start, clip.length), (8.0, 4.0));
    let LaneData::Notes(notes) = &p.content(clip.content).unwrap().lanes[0].data else { panic!() };
    assert_eq!((notes[0].key, notes[0].time, notes[0].duration), (64, 1.0, 2.0));
    assert!((notes[0].velocity * 127.0 - 64.2513).abs() < 1e-3);
}

#[test]
fn session_clips_and_inserted_scene() {
    let p = open("13-second-session-clip").project;
    assert_eq!(p.launcher.scenes.len(), 9);
    let column = &p.launcher.columns[0];
    let keys: Vec<u8> = column.slots[..2]
        .iter()
        .map(|s| {
            let clip = s.clip.as_ref().unwrap();
            let LaneData::Notes(notes) = &p.content(clip.content).unwrap().lanes[0].data else { panic!() };
            notes[0].key
        })
        .collect();
    assert_eq!(keys, [60, 67]);
}

#[test]
fn mixer_volume_pan_mute_send() {
    let audio = |name: &str| {
        let p = open(name).project;
        p.channels.into_iter().find(|c| c.name == "3-Audio").unwrap()
    };
    let c = audio("17-track-volume-pan");
    assert!((c.volume - 0.501_187).abs() < 1e-6);
    assert_eq!(c.pan, -1.0);
    assert!(!c.mute);
    assert!(audio("18-track-mute").mute);
    let c = audio("19-return-send");
    assert!(!c.mute);
    assert_eq!(c.sends.len(), 1);
    assert!((c.sends[0].level - 0.251_189).abs() < 1e-6);
}

#[test]
fn session_clip_length_2_bars() {
    let p = open("20-clip-length-2-bars").project;
    let clip = p.launcher.columns[0].slots[0].clip.as_ref().unwrap();
    assert_eq!(clip.loop_range.map(|r| r.end), Some(8.0));
    assert_eq!(clip.length, 8.0);
}

fn audio_clip(p: &suhina_model::Project, arrangement: bool) -> (suhina_model::Clip, suhina_model::AudioRegion) {
    let track = p.tracks.iter().find(|t| t.name == "4-loopwerk-demo").unwrap().id;
    let clip = if arrangement {
        p.arrangement.lanes.iter().find(|l| l.track == Some(track)).unwrap().clips[0].clone()
    } else {
        p.launcher.columns.iter().find(|c| c.track == track).unwrap().slots[0].clip.clone().unwrap()
    };
    let LaneData::Audio(audio) = &p.content(clip.content).unwrap().lanes[0].data else { panic!("expected audio") };
    (clip.clone(), audio.clone())
}

#[test]
fn audio_series_files_import() {
    for name in [
        "30-audio-clip",
        "31-audio-warp-off",
        "32-audio-warp-marker",
        "33-audio-gain",
        "34-audio-transpose",
        "35-audio-in-arrangement",
    ] {
        open(name);
    }
}

#[test]
fn audio_clip_file_and_default_warp() {
    let (clip, audio) = audio_clip(&open("30-audio-clip").project, false);
    assert_eq!(audio.file, "../loopwerk-demo.wav");
    assert_eq!(audio.sample_rate, Some(48_000));
    assert_eq!(audio.duration_seconds, Some(4.0));
    assert!(audio.warped);
    let markers: Vec<(f64, f64)> = audio.warp_markers.iter().map(|m| (m.time, m.audio_time)).collect();
    assert_eq!(markers, [(0.0, 0.0), (8.0, 4.0), (8.03125, 4.015625)]);
    assert_eq!(clip.loop_range.map(|r| r.end), Some(8.0));
}

#[test]
fn unwarped_audio_loop_is_converted_from_seconds() {
    let (_, audio) = audio_clip(&open("31-audio-warp-off").project, false);
    assert!(!audio.warped);
}

#[test]
fn moved_warp_marker_and_transpose() {
    let (_, audio) = audio_clip(&open("32-audio-warp-marker").project, false);
    assert!(audio.warp_markers.iter().any(|m| m.time == 1.125 && (m.audio_time - 0.500_146).abs() < 1e-6));
    let (_, audio) = audio_clip(&open("34-audio-transpose").project, false);
    assert_eq!(audio.transpose, 3.0);
    assert_eq!(audio.gain, 1.0);
}

#[test]
fn audio_clip_in_arrangement() {
    let (clip, audio) = audio_clip(&open("35-audio-in-arrangement").project, true);
    assert_eq!((clip.start, clip.length), (0.0, 8.0));
    assert_eq!(audio.file, "../loopwerk-demo.wav");
}

fn devices(name: &str, track: &str) -> Vec<suhina_model::Device> {
    let p = open(name).project;
    p.channels.into_iter().find(|c| c.name == track).unwrap().devices
}

#[test]
fn live_devices_become_placeholders() {
    use suhina_model::DeviceKind;
    let foreign = |id: &str| DeviceKind::Foreign { format: "als".into(), id: id.into() };
    let d = devices("40-midi-track-instrument", "1-Simpler");
    assert_eq!(d.iter().map(|d| d.kind.clone()).collect::<Vec<_>>(), [foreign("OriginalSimpler")]);
    let d = devices("42-device-param", "1-Simpler");
    // The EQ Eight after it comes in as Suhina's EQ (docs/spec/eq.md › Import).
    let eq = DeviceKind::Builtin("eq".into());
    assert_eq!(d.iter().map(|d| d.kind.clone()).collect::<Vec<_>>(), [foreign("OriginalSimpler"), eq]);
    assert!(d[1].enabled);
    assert!(!devices("43-device-off", "1-Simpler")[1].enabled);
}

#[test]
fn plugins_keep_id_and_state() {
    use suhina_model::DeviceKind;
    let d = devices("44-plugin-add-sylenth1-vst3", "1-Sylenth1");
    let names: Vec<&str> = d.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["Sylenth1", "EQ", "ShaperBox 3", "ShaperBox 3"]);

    let DeviceKind::Vst3 { id } = &d[0].kind else { panic!("Sylenth1 should be VST3: {:?}", d[0].kind) };
    assert_eq!(id.len(), 32);
    assert!(d[0].state.as_ref().is_some_and(|s| s.len() > 1000));

    let vst2 = d.iter().find(|d| matches!(d.kind, DeviceKind::Vst2 { .. })).unwrap();
    assert_eq!(vst2.kind, DeviceKind::Vst2 { id: 1_128_746_035 });
    // The plug-in's 3348-byte chunk, as a standard VST2 bank (a 160-byte header before it).
    let bank = vst2.state.as_deref().unwrap();
    assert_eq!((&bank[..4], &bank[8..12], bank.len()), (&b"CcnK"[..], &b"FBCh"[..], 3348 + 160));
}

#[test]
fn time_signatures_come_from_the_automation_default() {
    let ts = |name: &str| {
        let t = open(name).project.transport.time_signature;
        (t.numerator, t.denominator)
    };
    assert_eq!(ts("50-time-signature-3-4"), (3, 4));
    assert_eq!(ts("51-time-signature-6-8"), (6, 8));
    assert_eq!(ts("52-time-signature-7-8"), (7, 8));
    assert_eq!(ts("53-time-signature-4-4"), (4, 4));
}

/// The session clip in 1-MIDI slot 1 through the marker edits of series 4.
#[test]
fn clip_start_loop_and_end_markers() {
    let clip = |name: &str| open(name).project.launcher.columns[0].slots[0].clip.clone().unwrap();
    let range = |c: &suhina_model::Clip| c.loop_range.map(|r| (r.start, r.end));

    // Start 1.2.1 inside a 2-bar loop from 1.1.1.
    let c = clip("54-clip-start-marker");
    assert_eq!((c.offset, range(&c)), (1.0, Some((0.0, 8.0))));
    // Loop moved to 1.3.1; Live moved the start to 3.2.1 (beat 9).
    let c = clip("55-clip-loop-position");
    assert_eq!((c.offset, range(&c)), (9.0, Some((2.0, 10.0))));
    // Loop off: plays from the start marker (3.2.1) to the end marker (3.3.1).
    let c = clip("56-clip-loop-off");
    assert_eq!((c.offset, c.length, range(&c)), (9.0, 1.0, None));
    // End marker moved to 3.4.1.
    let c = clip("57-clip-end-marker");
    assert_eq!((c.offset, c.length), (9.0, 2.0));
    assert!(c.enabled);
    assert!(!clip("58-clip-deactivated").enabled);
}

#[test]
fn deactivated_notes_are_skipped() {
    let p = open("59-note-deactivated").project;
    let clip = p.launcher.columns[0].slots[1].clip.as_ref().unwrap();
    let LaneData::Notes(notes) = &p.content(clip.content).unwrap().lanes[0].data else { panic!() };
    assert!(notes.is_empty());
}

#[test]
fn group_track_routes_its_members() {
    let p = open("60-group-track").project;
    let group = p.tracks.iter().find(|t| t.name == "3-Group").unwrap();
    for name in ["4-Audio", "5-loopwerk-demo"] {
        let track = p.tracks.iter().find(|t| t.name == name).unwrap();
        assert_eq!(track.parent, Some(group.id));
        let channel = p.channels.iter().find(|c| Some(c.id) == track.channel).unwrap();
        assert_eq!(channel.output, group.channel);
    }
}

#[test]
fn unwarped_audio_times_are_seconds() {
    let audio_column = |name: &str| {
        let p = open(name).project;
        let track = p.tracks.iter().find(|t| t.name == "5-loopwerk-demo").unwrap().id;
        let clip = p.launcher.columns.iter().find(|c| c.track == track).unwrap().slots[0].clip.clone().unwrap();
        (p.transport.tempo_bpm, clip)
    };
    // Warp off: Live stored the end as 3.9376 s (beat 8 through the moved warp marker).
    let (bpm, clip) = audio_column("61-audio-warp-off");
    assert!((clip.length - 3.937_645_8 * bpm / 60.0).abs() < 1e-6);
    // Start moved to 1 s.
    let (bpm, clip) = audio_column("62-audio-unwarped-start");
    assert!((clip.offset - bpm / 60.0).abs() < 1e-9);
}

/// The Utility of a file in corpus series 6 (`corpus/devices`, docs/spec/als-corpus.md), in
/// whichever Live project folder under `corpus/devices` holds it.
fn utility(name: &str) -> suhina_model::Device {
    let root: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "..", "corpus", "devices"].iter().collect();
    let file = format!("{name}.als");
    let path = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}; author series 6 first (docs/spec/als-corpus.md)", root.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path().join(&file)))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("{file} is missing under {}", root.display()));
    let project = read_file(&path).unwrap_or_else(|e| panic!("{name}: {e}")).project;
    let devices: Vec<_> = project.channels.into_iter().flat_map(|c| c.devices).collect();
    devices.into_iter().find(|d| d.kind == suhina_model::DeviceKind::Builtin("utility".into())).unwrap()
}

fn param(device: &suhina_model::Device, id: &str) -> f64 {
    device.params.iter().find(|p| p.id == id).unwrap().value
}

#[test]
fn utility_series_maps_onto_the_suhina_utility() {
    let default = utility("110-utility-default.als");
    assert_eq!(default.name, "Utility");
    assert!(default.enabled);
    assert_eq!(param(&default, "gain"), 0.0);
    assert_eq!(param(&default, "channel"), 0.0);
    assert!((param(&utility("111-utility-gain-minus12"), "gain") + 12.0).abs() < 1e-6);
    assert_eq!(param(&utility("113-utility-gain-inf"), "gain"), -70.0);
    assert!((param(&utility("114-utility-gain-max"), "gain") - 35.0).abs() < 1e-4);
    assert_eq!(param(&utility("116-utility-balance-100r"), "balance"), 1.0);
    assert_eq!(param(&utility("119-utility-width-max"), "width"), 4.0);
    // The files were made as a chain: Mid/Side mode stays on from 120 to the end.
    let mid = utility("121-utility-midside-centre");
    assert_eq!((param(&mid, "width"), param(&mid, "midSide")), (1.0, -1.0));
    assert_eq!(param(&utility("120-utility-midside-mid-50S"), "midSide"), 0.5);
    assert_eq!(param(&utility("123-utility-mono"), "mono"), 1.0);
    assert_eq!(param(&utility("126-utility-bassmono-max"), "bassMonoFrequency"), 500.0);
    let both = utility("128-utility-phase-right");
    assert_eq!((param(&both, "invertLeft"), param(&both, "invertRight")), (1.0, 1.0));
    assert_eq!(param(&utility("130-utility-channel-left"), "channel"), 1.0);
    assert_eq!(param(&utility("132-utility-channel-swap"), "channel"), 3.0);
    assert_eq!(param(&utility("133-utility-dc"), "dcFilter"), 1.0);
    assert_eq!(param(&utility("134-utility-mute"), "mute"), 1.0);
    assert_eq!(param(&utility("135-utility-bassmono-listen"), "bassMonoListen"), 1.0);
}
