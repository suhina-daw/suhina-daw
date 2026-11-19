// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_flp::import_bytes;
use suhina_model::{DeviceKind, LaneData};

fn event(out: &mut Vec<u8>, id: u8, payload: &[u8]) {
    out.push(id);
    if id >= 192 {
        let mut n = payload.len();
        loop {
            let b = (n & 127) as u8;
            n >>= 7;
            out.push(b | if n > 0 { 128 } else { 0 });
            if n == 0 {
                break;
            }
        }
    }
    out.extend(payload);
}

fn text(out: &mut Vec<u8>, id: u8, s: &str) {
    event(out, id, &s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<_>>());
}

fn fixture(note_length: u32, clip_size: usize) -> Vec<u8> {
    let mut events = Vec::new();
    event(&mut events, 199, b"26.1.0\0");
    event(&mut events, 156, &120000u32.to_le_bytes());
    event(&mut events, 65, &1u16.to_le_bytes());
    text(&mut events, 193, "Two voices");
    let mut notes = Vec::new();
    for channel in 0..2u16 {
        let mut note = [0u8; 24];
        note[6..8].copy_from_slice(&channel.to_le_bytes());
        note[8..12].copy_from_slice(&note_length.to_le_bytes());
        note[12..14].copy_from_slice(&(60 + channel).to_le_bytes());
        note[16] = 120;
        note[20] = 64;
        note[21] = 100;
        notes.extend(note);
    }
    event(&mut events, 224, &notes);
    for channel in 0..2u16 {
        event(&mut events, 64, &channel.to_le_bytes());
        event(&mut events, 21, &[0]);
        event(&mut events, 104, &1u16.to_le_bytes());
        text(&mut events, 203, &format!("Voice {channel}"));
        text(&mut events, 196, "own.wav");
        event(&mut events, 135, &60u32.to_le_bytes());
        event(&mut events, 51, &[0]);
    }
    event(&mut events, 99, &0u16.to_le_bytes());
    let mut clips = Vec::new();
    for start in [0u32, 384] {
        let mut clip = vec![0u8; clip_size];
        clip[0..4].copy_from_slice(&start.to_le_bytes());
        clip[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
        clip[6..8].copy_from_slice(&0x5001u16.to_le_bytes());
        clip[8..12].copy_from_slice(&384u32.to_le_bytes());
        clip[12..14].copy_from_slice(&499u16.to_le_bytes());
        clip[24..32].fill(255);
        clips.extend(clip);
    }
    event(&mut events, 233, &clips);
    // Unknown data must survive together with its original position and context.
    event(&mut events, 250, &[7, 4, 9]);
    let mut file = b"FLhd".to_vec();
    file.extend(6u32.to_le_bytes());
    file.extend(0u16.to_le_bytes());
    file.extend(2u16.to_le_bytes());
    file.extend(96u16.to_le_bytes());
    file.extend(b"FLdt");
    file.extend((events.len() as u32).to_le_bytes());
    file.extend(events);
    file
}

fn appended(mut file: Vec<u8>, events: &[u8]) -> Vec<u8> {
    file.extend_from_slice(events);
    let length = (file.len() - 22) as u32;
    file[18..22].copy_from_slice(&length.to_le_bytes());
    file
}

#[test]
fn automatic_pattern_length_survives_metadata_order_and_rounds_to_whole_bars() {
    for (note_end, explicit, bar, expected) in [
        (3072, None, (4, 4), 32.),
        (3072, Some(0), (4, 4), 32.),
        (3064, Some(0), (4, 4), 32.),
        (3073, Some(0), (4, 4), 36.),
        (1, Some(0), (4, 4), 4.),
        (3072, Some(288), (4, 4), 3.),
        (480, Some(0), (3, 4), 6.),
        (480, Some(0), (7, 2), 7.),
    ] {
        for after_notes in [false, true] {
            let original = fixture(note_end, 32);
            let flp = suhina_flp::read_bytes(&original).unwrap();
            let mut bytes = Vec::new();
            event(&mut bytes, 17, &[bar.0]);
            event(&mut bytes, 18, &[bar.1]);
            for e in flp.events {
                use suhina_flp::Payload;
                let payload = match e.payload {
                    Payload::Byte(v) => vec![v],
                    Payload::Word(v) => v.to_le_bytes().to_vec(),
                    Payload::Dword(v) => v.to_le_bytes().to_vec(),
                    Payload::Data(v) => v,
                };
                event(&mut bytes, e.id, &payload);
                if e.id == if after_notes { 224 } else { 65 }
                    && let Some(length) = explicit
                {
                    event(&mut bytes, 164, &u32::to_le_bytes(length));
                }
            }
            let p = import_bytes(&appended(original[..22].to_vec(), &bytes)).unwrap().project;
            assert!(!p.contents.is_empty());
            for content in &p.contents {
                assert_eq!(content.length, expected, "note end {note_end}, explicit {explicit:?}, after {after_notes}");
                let LaneData::Notes(notes) = &content.lanes[0].data else { panic!("notes") };
                assert_eq!(notes[0].duration, f64::from(note_end) / 96., "source notes stay intact");
            }
            for clip in p.arrangement.lanes.iter().flat_map(|l| &l.clips) {
                assert_eq!(clip.length, expected, "untrimmed placements use the resolved pattern length");
            }
            p.validate().unwrap();
        }
    }
}

#[test]
fn playlist_colors_take_precedence_over_source_and_pattern_colors() {
    let original = fixture(96, 32);
    let flp = suhina_flp::read_bytes(&original).unwrap();
    let mut bytes = Vec::new();
    for e in flp.events {
        use suhina_flp::Payload;
        let payload = match e.payload {
            Payload::Byte(v) => vec![v],
            Payload::Word(v) => v.to_le_bytes().to_vec(),
            Payload::Dword(v) => v.to_le_bytes().to_vec(),
            Payload::Data(v) => v,
        };
        event(&mut bytes, e.id, &payload);
        if e.id == 64 {
            event(&mut bytes, 41, &[1]);
            event(&mut bytes, 128, &0x00202260u32.to_le_bytes());
        }
        if e.id == 65 {
            event(&mut bytes, 150, &0xffb98249u32.to_le_bytes());
        }
    }
    let mut row = [0u8; 66];
    row[..4].copy_from_slice(&1u32.to_le_bytes());
    row[4..8].copy_from_slice(&0x00332211u32.to_le_bytes());
    row[12] = 1;
    event(&mut bytes, 238, &row);
    event(&mut bytes, 43, &[1]);
    let p = import_bytes(&appended(original[..22].to_vec(), &bytes)).unwrap().project;
    for track in p.tracks.iter().filter(|t| t.id.0 < 2) {
        assert_eq!(track.color, Some(suhina_model::Rgb(17, 34, 51)));
    }
    for clip in p.arrangement.lanes.iter().flat_map(|l| &l.clips) {
        assert_eq!(clip.color, Some(suhina_model::Rgb(17, 34, 51)));
    }
    *bytes.last_mut().unwrap() = 0;
    let fallback = import_bytes(&appended(original[..22].to_vec(), &bytes)).unwrap().project;
    for track in fallback.tracks.iter().filter(|t| t.id.0 < 2) {
        assert_eq!(track.color, Some(suhina_model::Rgb(96, 34, 32)));
    }
    for clip in fallback.arrangement.lanes.iter().flat_map(|l| &l.clips) {
        assert_eq!(clip.color, Some(suhina_model::Rgb(73, 130, 185)));
    }
}

#[test]
fn playlist_folder_preserves_mixer_routing_and_source_ownership() {
    let original = import_bytes(&fixture(96, 32)).unwrap().project;
    let mut bytes = Vec::new();
    for index in [1u32, 2] {
        let mut row = [0u8; 66];
        row[..4].copy_from_slice(&index.to_le_bytes());
        row[12] = 1;
        row[46] = u8::from(index == 2);
        event(&mut bytes, 238, &row);
        if index == 1 {
            text(&mut bytes, 239, "Voices");
        }
    }
    let p = import_bytes(&appended(fixture(96, 32), &bytes)).unwrap().project;
    assert_eq!(p.channels, original.channels);
    let folder = p.tracks.iter().find(|t| t.is_folder()).unwrap();
    assert_eq!(folder.name, "Voices");
    assert_eq!(p.arrangement.lanes.len(), original.arrangement.lanes.len());
    for lane in &p.arrangement.lanes {
        let mut id = lane.track.unwrap();
        while let Some(parent) = p.track(id).unwrap().parent {
            id = parent;
        }
        assert_eq!(id, folder.id);
    }
    p.validate().unwrap();
}

#[test]
fn audio_placement_fades_are_editable_and_gain_survives() {
    let mut events = Vec::new();
    event(&mut events, 64, &2u16.to_le_bytes());
    event(&mut events, 21, &[4]);
    text(&mut events, 196, "voice.wav");
    event(&mut events, 51, &[0]);
    let mut record = [0u8; 60];
    record[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
    record[6..8].copy_from_slice(&2u16.to_le_bytes());
    record[8..12].copy_from_slice(&384u32.to_le_bytes());
    record[12..14].copy_from_slice(&498u16.to_le_bytes());
    record[36..40].copy_from_slice(&250f32.to_le_bytes());
    record[40..44].copy_from_slice(&3.36f32.to_le_bytes());
    record[44..48].copy_from_slice(&500f32.to_le_bytes());
    record[48..52].copy_from_slice(&(-4.83f32).to_le_bytes());
    record[52..56].copy_from_slice(&0.5f32.to_le_bytes());
    event(&mut events, 233, &record);
    let p = import_bytes(&appended(fixture(96, 32), &events)).unwrap().project;
    let clip = &p.arrangement.lanes.iter().find(|l| l.track == Some(suhina_model::TrackId(2))).unwrap().clips[0];
    assert_eq!((clip.fade_in, clip.fade_out), (0.5, 1.));
    assert_eq!(clip.fade_in_curve.shape, suhina_model::FadeShape::Cosine);
    assert!((clip.fade_in_curve.gain(0.5) - 0.676).abs() < 0.0005);
    assert!((clip.fade_out_curve.gain(0.5) - 0.293).abs() < 0.0005);
    assert_eq!(clip.audio.gain, 0.5);
    assert!(clip.audio.gain_envelope.is_empty());
    record[36..40].copy_from_slice(&f32::NAN.to_le_bytes());
    let mut malformed = Vec::new();
    event(&mut malformed, 233, &record);
    assert!(import_bytes(&appended(appended(fixture(96, 32), &events), &malformed)).is_err());
}

#[test]
fn mixed_playlist_rows_form_a_group_and_keep_audio_targets() {
    let mut e = Vec::new();
    for (id, kind) in [(2u16, 4), (3, 5)] {
        event(&mut e, 64, &id.to_le_bytes());
        event(&mut e, 21, &[kind]);
        text(&mut e, 203, "Source");
        if kind == 4 {
            text(&mut e, 196, "voice.wav");
        }
        event(&mut e, 51, &[0]);
    }
    let mut clips = Vec::new();
    for source in [2u16, 3] {
        let mut c = [0u8; 32];
        c[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
        c[6..8].copy_from_slice(&source.to_le_bytes());
        c[8..12].copy_from_slice(&384u32.to_le_bytes());
        c[12..14].copy_from_slice(&499u16.to_le_bytes());
        c[24..28].copy_from_slice(&250f32.to_le_bytes());
        clips.extend(c);
    }
    event(&mut e, 233, &clips);
    let p = import_bytes(&appended(fixture(96, 32), &e)).unwrap().project;
    let children: Vec<_> = p.tracks.iter().filter(|t| t.parent.is_some()).collect();
    assert_eq!(children.len(), 3);
    assert!(children.iter().all(|t| t.parent == children[0].parent));
    let parent = p.track(children[0].parent.unwrap()).unwrap();
    assert_eq!(p.channel(parent.channel.unwrap()).unwrap().role, suhina_model::ChannelRole::Submix);
    let content = p.content(suhina_model::ContentId(65_536 + 2)).unwrap();
    assert_eq!(content.lanes[0].target, None);
    assert!(children.iter().all(|t| p.channel_of_track(t.id).unwrap().output == parent.channel));
    let audio = p.arrangement.lanes.iter().flat_map(|l| &l.clips).find(|c| c.content == content.id).unwrap();
    assert_eq!(audio.offset, 0.5, "250ms at 120bpm");
    assert!(!p.arrangement.lanes.iter().flat_map(|l| &l.clips).any(|c| c.content.0 == 65_539));
    assert!(p.track(suhina_model::TrackId(3)).is_none());
}

#[test]
fn old_mixer_places_keep_one_shared_chain_and_levels() {
    let mut e = Vec::new();
    for bus in 0..3 {
        event(&mut e, 149, &0xff332211u32.to_le_bytes());
        event(&mut e, 42, &[1]);
        text(&mut e, 204, &format!("Bus {bus}"));
        event(&mut e, 236, &[0, 0, 0, 0, 12, 0, 0, 0]);
        if bus == 1 {
            text(&mut e, 201, "Unknown effect");
            event(&mut e, 213, &[1, 2, 3]);
            event(&mut e, 98, &2u16.to_le_bytes());
            event(&mut e, 235, &[1, 0, 0]);
        }
    }
    let mut params = Vec::new();
    for (pid, location, value) in
        [(0xc0, 0x2040u16, 8000i32), (64, 0x2040, 4000), (0, 0x2042, 0), (0xd0, 0x2040, -1800)]
    {
        let mut r = [0u8; 12];
        r[4] = pid;
        r[5] = 0x1f;
        r[6..8].copy_from_slice(&location.to_le_bytes());
        r[8..12].copy_from_slice(&value.to_le_bytes());
        params.extend(r);
    }
    event(&mut e, 225, &params);
    let p = import_bytes(&appended(fixture(96, 32), &e)).unwrap().project;
    assert_eq!(p.channels[0].output, p.channels[1].output);
    let bus = p.channel(p.channels[0].output.unwrap()).unwrap();
    assert_eq!(p.tracks.iter().find(|t| t.channel == Some(bus.id)).unwrap().color, Some(suhina_model::Rgb(17, 34, 51)));
    assert_eq!(bus.role, suhina_model::ChannelRole::Submix, "primary buses are selectable output groups");
    assert!((bus.volume - (11f64.powf(8000. / 12800.) - 1.) / 10.).abs() < 1e-12);
    assert!(bus.sends.is_empty());
    let gain = bus.devices.last().unwrap().params.iter().find(|p| p.id == "gain").unwrap().value;
    assert!((10f64.powf(gain / 20.) - (11f64.powf(4000. / 12800.) - 1.) / 10.).abs() < 1e-12);
    assert_eq!(bus.devices.len(), 3);
    assert_eq!(bus.devices[0].kind, DeviceKind::Builtin("eq".into()));
    assert_eq!(bus.devices[0].params.iter().find(|p| p.id == "a1Gain").unwrap().value, -15.);
    assert!(!bus.devices[1].enabled);
    assert_eq!(bus.devices[1].state.as_deref(), Some(&[1, 2, 3][..]));
    assert_eq!(p.channel(bus.output.unwrap()).unwrap().role, suhina_model::ChannelRole::Master);
    let bus_id = bus.id;
    let mut row = [0u8; 66];
    row[..4].copy_from_slice(&1u32.to_le_bytes());
    row[4..8].copy_from_slice(&0xff665544u32.to_le_bytes());
    event(&mut e, 238, &row);
    event(&mut e, 43, &[1]);
    let recolored = import_bytes(&appended(fixture(96, 32), &e)).unwrap().project;
    assert_eq!(
        recolored.tracks.iter().find(|t| t.channel == Some(bus_id)).unwrap().color,
        Some(suhina_model::Rgb(68, 85, 102)),
        "playlist beats the authored mixer colour"
    );
}

#[test]
fn source_level_is_a_utility_before_the_shared_mixer() {
    let p = import_bytes(&fixture(96, 32)).unwrap().project;
    for channel in &p.channels {
        assert_eq!((channel.volume, channel.pan), (1., 0.));
        let utility = channel.devices.last().unwrap();
        assert_eq!(utility.kind, DeviceKind::Builtin("utility".into()));
        let gain = utility.params.iter().find(|p| p.id == "gain").unwrap().value;
        assert!((gain - 20. * ((11f64.powf(10000. / 12800.) - 1.) / 10.).log10()).abs() < 1e-10);
    }
}

#[test]
fn unknown_instruments_do_not_turn_into_default_tones() {
    let mut bytes = fixture(96, 32);
    let parsed = suhina_flp::read_bytes(&bytes).unwrap();
    for event in parsed.events.iter().filter(|e| e.id == 21) {
        bytes[event.offset + 1] = 2;
    }
    let p = import_bytes(&bytes).unwrap().project;
    assert!(p.channels.iter().all(|c| !c.mute && c.devices[0].is_missing_instrument()));
    assert!(p.channels.iter().all(|c| matches!(c.devices[0].kind, DeviceKind::Foreign { .. })));
}

#[test]
fn source_device_off_and_mixer_mute_are_independent() {
    for original in [fixture(96, 32), wrapper_fixture(false, false), wrapper_fixture(true, false), patch_fixture(false)]
    {
        for enabled in [false, true] {
            let mut bytes = original.clone();
            let parsed = suhina_flp::read_bytes(&bytes).unwrap();
            for e in parsed.events.iter().rev().filter(|e| e.id == 21) {
                bytes.splice(e.offset + 2..e.offset + 2, [0, u8::from(enabled)]);
            }
            let mut mixer = Vec::new();
            for flags in [12, 4, 12] {
                event(&mut mixer, 236, &[0, 0, 0, 0, flags, 0, 0, 0]);
            }
            let p = import_bytes(&appended(bytes, &mixer)).unwrap().project;
            for source in p.channels.iter().filter(|c| c.id.0 < 131_072) {
                assert!(!source.mute, "instrument state must not become source track mute");
                assert_eq!(source.devices[0].enabled, enabled);
                assert!(source.devices.last().unwrap().enabled, "source utility stays independent");
            }
            assert!(p.channel(suhina_model::ChannelId(131_073)).unwrap().mute);
        }
    }
}

#[test]
fn parallel_mixer_sends_keep_one_shared_group() {
    let mut events = Vec::new();
    for bus in 0..4 {
        event(&mut events, 236, &[0, 0, 0, 0, 12, 0, 0, 0]);
        if bus == 1 {
            event(&mut events, 235, &[1, 0, 1]);
        }
    }
    let p = import_bytes(&appended(fixture(96, 32), &events)).unwrap().project;
    let source = p.channel(suhina_model::ChannelId(131_073)).unwrap();
    assert_eq!(source.sends.len(), 1);
    assert_eq!(source.output, Some(suhina_model::ChannelId(131_072)));
    assert!(p.channel(suhina_model::ChannelId(132_001)).is_none(), "no duplicate Main return");
    assert_eq!(source.role, suhina_model::ChannelRole::Submix);
    assert_eq!(p.channel(suhina_model::ChannelId(131_074)).unwrap().role, suhina_model::ChannelRole::Effect);
}

#[test]
fn audio_volume_automation_becomes_a_gain_envelope_but_pan_and_muted_curves_do_not() {
    fn automated(parameter: u16, muted: bool) -> Vec<u8> {
        let mut e = Vec::new();
        let mut link = [0u8; 20];
        link[2..4].copy_from_slice(&3u16.to_le_bytes());
        link[8..12].copy_from_slice(&((2u32 << 16) | u32::from(parameter)).to_le_bytes());
        event(&mut e, 227, &link);
        event(&mut e, 64, &2u16.to_le_bytes());
        event(&mut e, 21, &[4]);
        text(&mut e, 196, "audio.wav");
        event(&mut e, 51, &[0]);
        event(&mut e, 64, &3u16.to_le_bytes());
        event(&mut e, 21, &[5]);
        let mut curve = vec![0u8; 21];
        curve[4..8].copy_from_slice(&64u32.to_le_bytes()); // Default amount, with LFO switched off.
        curve[17..21].copy_from_slice(&2u32.to_le_bytes());
        for (delta, value) in [(0f64, 0.5f64), (4., 1.)] {
            curve.extend(delta.to_le_bytes());
            curve.extend(value.to_le_bytes());
            curve.extend(0f32.to_le_bytes());
            curve.extend([0, 0, 0, 2]);
        }
        event(&mut e, 234, &curve);
        event(&mut e, 51, &[0]);
        let mut clips = Vec::new();
        for source in [2u16, 3] {
            let mut c = [0u8; 32];
            c[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
            c[6..8].copy_from_slice(&source.to_le_bytes());
            c[8..12].copy_from_slice(&384u32.to_le_bytes());
            c[12..14].copy_from_slice(&499u16.to_le_bytes());
            if source == 3 && muted {
                c[19] = 0x20;
            }
            clips.extend(c);
        }
        event(&mut e, 233, &clips);
        appended(fixture(96, 32), &e)
    }
    // Playlist mute is independent of the clip's mute flag, and applies to automation too.
    let mut row = [0u8; 13];
    row[..4].copy_from_slice(&1u32.to_le_bytes());
    let mut extra = Vec::new();
    event(&mut extra, 238, &row);
    let muted_row = import_bytes(&appended(automated(0x11, false), &extra)).unwrap();
    assert!(
        muted_row
            .project
            .arrangement
            .lanes
            .iter()
            .flat_map(|l| &l.clips)
            .all(|c| !c.enabled && c.audio.gain_envelope.is_empty())
    );
    for (parameter, muted, mapped) in [(0x11, false, true), (1, false, false), (0x11, true, false)] {
        let result = import_bytes(&automated(parameter, muted)).unwrap();
        let clip =
            result.project.arrangement.lanes.iter().flat_map(|l| &l.clips).find(|c| c.content.0 == 65538).unwrap();
        assert_eq!(!clip.audio.gain_envelope.is_empty(), mapped);
        if mapped {
            assert!(suhina_model::envelope_db_at(&clip.audio.gain_envelope, 0.).abs() < 1e-10);
            assert!((suhina_model::envelope_db_at(&clip.audio.gain_envelope, 4.) - 20. * 12f64.log10()).abs() < 1e-10);
            assert!(
                (suhina_model::envelope_db_at(&clip.audio.gain_envelope, 2.)
                    - 20. * ((11f64.powf(1.5) - 1.) / 10.).log10())
                .abs()
                    < 1e-10
            );
        }
    }
}

#[test]
fn samples_are_found_beside_the_project_or_reported_missing_without_writing_sources() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let folder = std::env::temp_dir().join(format!("suhina-import-local-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(folder.join("Samples")).unwrap();
    let path = folder.join("authored.flp");
    let bytes = fixture(96, 32);
    std::fs::write(&path, &bytes).unwrap();
    let missing = suhina_flp::import_file(&path).unwrap();
    assert!(missing.warnings.iter().any(|w| w.contains("Sample not found")));
    let library_file = folder.join("library.wav");
    std::fs::write(&library_file, b"authored library fixture").unwrap();
    let library = suhina_flp::import_file_with_samples(
        &path,
        &suhina_media::Trusted { local: true, ..Default::default() },
        |files| {
            assert_eq!(files, &["own.wav".to_string()]);
            vec![Some(library_file.clone())]
        },
    )
    .unwrap();
    assert_eq!(
        library.project.channels[0].devices[0].sample_file(),
        Some(library_file.to_string_lossy().replace('\\', "/").as_str())
    );
    assert!(!library.warnings.iter().any(|w| w.contains("Sample not found")));
    std::fs::write(folder.join("Samples/own.wav"), b"authored existence fixture").unwrap();
    let found = suhina_flp::import_file(&path).unwrap();
    assert_eq!(found.project.channels[0].devices[0].sample_file(), Some("Samples/own.wav"));
    assert!(!found.warnings.iter().any(|w| w.contains("Sample not found")));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read(folder.join("Samples/own.wav")).unwrap(), b"authored existence fixture");
}

#[test]
fn shared_pattern_keeps_targets_positions_and_sampler_source() {
    for size in [32, 60, 80, 88] {
        let result = import_bytes(&fixture(96, size)).unwrap();
        let p = result.project;
        assert_eq!(p.tracks.len(), 2);
        assert_eq!(p.contents.len(), 2);
        assert_eq!(p.arrangement.lanes.len(), 2);
        for (i, placed) in p.arrangement.lanes.iter().enumerate() {
            assert_eq!(placed.track, Some(p.tracks[i].id));
            assert!(p.channel_of_track(p.tracks[i].id).is_some());
            let clips = &placed.clips;
            assert_eq!(clips.len(), 2);
            assert_eq!(clips[0].content, clips[1].content);
            assert_eq!((clips[0].start, clips[1].start), (0., 4.));
            let content = p.content(clips[0].content).unwrap();
            assert_eq!(content.lanes.len(), 1);
            assert_eq!(content.lanes[0].target, None);
            let LaneData::Notes(notes) = &content.lanes[0].data else { panic!("notes") };
            assert_eq!(notes[0].duration, 1.);
            assert_eq!(notes[0].key, 60 + i as u8);
        }
        assert_eq!(p.channels[0].devices[0].kind, DeviceKind::Builtin("sampler".into()));
        assert_eq!(p.channels[0].devices[0].sample_file(), Some("own.wav"));
        assert!(!result.warnings.is_empty());
        p.validate().unwrap();
    }
}

#[test]
fn source_and_shared_content_survive_project_file_roundtrip() {
    let bytes = fixture(96, 88);
    let project = import_bytes(&bytes).unwrap().project;
    let serialized = suhina_project_file::to_json(&project);
    let back = suhina_project_file::from_json(&serialized).unwrap();
    assert_eq!(back, project);
    assert_eq!(back.extensions.0[0].data, bytes);
}

#[test]
fn step_and_zero_length_notes_do_not_stop_the_import() {
    for length in [0, u32::MAX] {
        let imported = import_bytes(&fixture(length, 88)).unwrap();
        let notes: usize = imported
            .project
            .contents
            .iter()
            .flat_map(|c| c.lanes.iter())
            .map(|l| if let LaneData::Notes(n) = &l.data { n.len() } else { 0 })
            .sum();
        assert_eq!(notes, 0, "nothing is invented for a note without a length");
        assert!(imported.warnings.iter().any(|w| w.starts_with("2 step-sequencer")), "{:?}", imported.warnings);
        assert!(imported.project.extensions.0.iter().any(|e| e.format == "flp" && e.key == "source"));
    }
}

#[test]
fn hostile_and_unsupported_inputs_return_errors() {
    let mut file = fixture(96, 88);
    file[12..14].fill(0);
    assert!(import_bytes(&file).unwrap_err().to_string().contains("PPQ"));
    let file = fixture(96, 88);
    for end in 0..file.len() {
        assert!(import_bytes(&file[..end]).is_err());
    }
}

#[test]
fn repeated_sources_from_separate_rows_merge_without_losing_placements() {
    let mut e = Vec::new();
    let mut clip = [0u8; 32];
    clip[..4].copy_from_slice(&768u32.to_le_bytes());
    clip[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
    clip[6..8].copy_from_slice(&0x5001u16.to_le_bytes());
    clip[8..12].copy_from_slice(&384u32.to_le_bytes());
    clip[12..14].copy_from_slice(&498u16.to_le_bytes());
    event(&mut e, 233, &clip);
    let p = import_bytes(&appended(fixture(96, 32), &e)).unwrap().project;
    assert_eq!(p.arrangement.lanes.len(), 2);
    assert_eq!(p.channels.len(), 2, "never duplicate an instrument");
    for lane in &p.arrangement.lanes {
        assert_eq!(lane.clips.iter().map(|c| c.start).collect::<Vec<_>>(), [0., 4., 8.]);
        assert!(lane.clips.iter().all(|c| c.content == lane.clips[0].content));
        assert_eq!(lane.clips[2].extensions.0.iter().find(|e| e.key == "playlist-row").unwrap().data, b"Playlist 2");
        // A pattern placed several times comes in as linked copies (#1414).
        assert!(p.content(lane.clips[0].content).unwrap().linked);
    }
    // Audio sources stay ordinary shared contents.
    assert!(
        p.contents.iter().filter(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_)))).all(|c| !c.linked)
    );
}

#[test]
fn file_import_fits_audio_to_stored_beats_and_converts_source_trim() {
    let folder = std::env::temp_dir().join(format!("suhina-stretch-authored-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    // One second, 8 kHz mono PCM, authored here; no external sample fixture.
    let mut wav = b"RIFF".to_vec();
    wav.extend(16036u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(8000u32.to_le_bytes());
    wav.extend(16000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(16000u32.to_le_bytes());
    wav.resize(16044, 0);
    std::fs::create_dir_all(folder.join("Library")).unwrap();
    std::fs::write(folder.join("Library/authored.wav"), &wav).unwrap();
    for stored_time in [(4f32 * 192.).to_bits(), 4u32 * 192] {
        let mut e = Vec::new();
        event(&mut e, 64, &2u16.to_le_bytes());
        event(&mut e, 21, &[4]);
        text(&mut e, 196, "authored.wav");
        let mut settings = [0u8; 112];
        settings[96..100].copy_from_slice(&stored_time.to_le_bytes());
        settings[108..112].copy_from_slice(&5u32.to_le_bytes());
        event(&mut e, 215, &settings);
        event(&mut e, 51, &[0]);
        let mut clip = [0u8; 32];
        clip[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
        clip[6..8].copy_from_slice(&2u16.to_le_bytes());
        clip[8..12].copy_from_slice(&192u32.to_le_bytes());
        clip[12..14].copy_from_slice(&499u16.to_le_bytes());
        clip[24..28].copy_from_slice(&250f32.to_le_bytes());
        event(&mut e, 233, &clip);
        let bytes = appended(fixture(96, 32), &e);
        let path = folder.join("authored.flp");
        std::fs::write(&path, &bytes).unwrap();
        let result = suhina_flp::import_file_with_samples(
            &path,
            &suhina_media::Trusted { local: true, ..Default::default() },
            |files| files.iter().map(|f| (f == "authored.wav").then(|| folder.join("Library/authored.wav"))).collect(),
        )
        .unwrap();
        let lane = result.project.arrangement.lanes.iter().find(|l| l.track == Some(suhina_model::TrackId(2))).unwrap();
        let clip = &lane.clips[0];
        assert!(clip.audio.warped);
        assert_eq!(clip.audio.stretch, suhina_model::StretchMode::Smooth);
        assert_eq!(clip.offset, 1., "quarter second source trim maps to one of four beats");
        assert_eq!((clip.audio.warp_markers[1].time, clip.audio.warp_markers[1].audio_time), (4., 1.));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(std::fs::read(folder.join("Library/authored.wav")).unwrap(), wav);
    }
}

#[test]
fn a_sample_path_outside_the_project_is_never_read_for_its_stretch() {
    let root = std::env::temp_dir().join(format!("suhina-stretch-outside-{}", std::process::id()));
    let folder = root.join("Project");
    std::fs::create_dir_all(&folder).unwrap();
    // A readable file one step above the project: the stored path names it with `..`.
    let mut wav = b"RIFF".to_vec();
    wav.extend(16036u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(8000u32.to_le_bytes());
    wav.extend(16000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(16000u32.to_le_bytes());
    wav.resize(16044, 0);
    std::fs::write(root.join("outside.wav"), &wav).unwrap();
    let mut e = Vec::new();
    event(&mut e, 64, &2u16.to_le_bytes());
    event(&mut e, 21, &[4]);
    text(&mut e, 196, "../outside.wav");
    let mut settings = [0u8; 112];
    settings[96..100].copy_from_slice(&(4u32 * 192).to_le_bytes());
    settings[108..112].copy_from_slice(&5u32.to_le_bytes());
    event(&mut e, 215, &settings);
    event(&mut e, 51, &[0]);
    let mut clip = [0u8; 32];
    clip[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
    clip[6..8].copy_from_slice(&2u16.to_le_bytes());
    clip[8..12].copy_from_slice(&192u32.to_le_bytes());
    clip[12..14].copy_from_slice(&499u16.to_le_bytes());
    event(&mut e, 233, &clip);
    let path = folder.join("authored.flp");
    std::fs::write(&path, appended(fixture(96, 32), &e)).unwrap();
    let result =
        suhina_flp::import_file_with_samples(&path, &suhina_media::Trusted::default(), |files| vec![None; files.len()])
            .unwrap();
    let lane = result.project.arrangement.lanes.iter().find(|l| l.track == Some(suhina_model::TrackId(2))).unwrap();
    assert!(!lane.clips[0].audio.warped, "a path out of the project folder is not opened");
    assert!(result.warnings.iter().any(|w| w.contains("sample duration unavailable")), "{:?}", result.warnings);
    assert!(result.warnings.iter().any(|w| w.contains("../outside.wav")), "{:?}", result.warnings);
}

fn wrapper_record(out: &mut Vec<u8>, id: u32, bytes: &[u8]) {
    out.extend(id.to_le_bytes());
    out.extend((bytes.len() as u64).to_le_bytes());
    out.extend(bytes);
}

fn wrapper_fixture(vst3: bool, broken: bool) -> Vec<u8> {
    let mut wrapper = 10u32.to_le_bytes().to_vec();
    wrapper_record(&mut wrapper, 54, b"Authored synth");
    let mut state = Vec::new();
    if vst3 {
        wrapper_record(&mut wrapper, 55, b"C:/Plugins/Authored.vst3");
        wrapper_record(
            &mut wrapper,
            52,
            &[0x44, 0x33, 0x22, 0x11, 0x66, 0x55, 0x88, 0x77, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0],
        );
        state.extend(1u32.to_le_bytes());
        wrapper_record(&mut state, 3, b"authored processor");
        wrapper_record(&mut state, 2, b"authored controller");
        wrapper_record(&mut state, 4, &[2u32, 99, 123].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>());
    } else {
        wrapper_record(&mut wrapper, 51, &0x12345678u32.to_le_bytes());
        // Some VST2 wrappers also store the replacement VST3 class id.
        wrapper_record(&mut wrapper, 52, &[7; 16]);
        state.extend((-9i32).to_le_bytes());
        state.push(13);
        state.extend((-2i32).to_le_bytes());
        state.extend(5u32.to_le_bytes());
        state.extend([0; 8]);
        state.extend(b"chunk");
    }
    if broken {
        state.pop();
    }
    wrapper_record(&mut wrapper, 53, &state);
    let mut e = Vec::new();
    event(&mut e, 64, &2u16.to_le_bytes());
    event(&mut e, 21, &[2]);
    text(&mut e, 203, "Renamed source");
    text(&mut e, 201, "Fruity Wrapper");
    event(&mut e, 213, &wrapper);
    event(&mut e, 51, &[0]);
    appended(fixture(96, 32), &e)
}

#[test]
fn wrapped_instruments_keep_their_identity_and_host_state() {
    for vst3 in [false, true] {
        let imported = import_bytes(&wrapper_fixture(vst3, false)).unwrap();
        let channel = imported.project.channels.iter().find(|c| c.name == "Renamed source").unwrap();
        let device = &channel.devices[0];
        assert!(!channel.mute, "supported instruments retain the source mute state");
        assert_eq!(device.name, "Authored synth");
        assert!(!device.is_missing_instrument());
        if vst3 {
            assert_eq!(device.kind, DeviceKind::Vst3 { id: "112233445566778899AABBCCDDEEFF00".into() });
            assert_eq!(device.state.as_deref(), Some(b"authored processor".as_slice()));
            assert!(
                device.extensions.0.iter().any(|e| e.key == "Vst3ControllerState" && e.data == b"authored controller")
            );
        } else {
            assert_eq!(device.kind, DeviceKind::Vst2 { id: 0x12345678 });
            let state = device.state.as_deref().unwrap();
            assert_eq!(&state[..4], b"CcnK");
            assert_eq!(&state[8..12], b"FBCh");
            assert_eq!(&state[160..], b"chunk");
        }
        let loaded = suhina_project_file::from_json(&suhina_project_file::to_json(&imported.project)).unwrap();
        assert_eq!(loaded, imported.project);
    }
}

#[test]
fn corrupt_wrapped_state_stays_a_silent_preserved_placeholder() {
    for vst3 in [false, true] {
        let imported = import_bytes(&wrapper_fixture(vst3, true)).unwrap();
        let channel = imported.project.channels.iter().find(|c| c.name == "Renamed source").unwrap();
        assert!(channel.devices[0].is_missing_instrument());
        assert!(channel.devices[0].state.is_some());
    }
}

#[test]
fn mixer_wrappers_use_the_same_decoder_and_preserve_bypass() {
    for vst3 in [false, true] {
        let file = wrapper_fixture(vst3, false);
        let parsed = suhina_flp::read_bytes(&file).unwrap();
        let wrapper = parsed.events.iter().find(|e| e.id == 213).unwrap();
        let suhina_flp::Payload::Data(wrapper) = &wrapper.payload else {
            panic!("state data");
        };
        let mut events = Vec::new();
        let mut flags = [0u8; 8];
        flags[4] = 8; // channel on, effects bypassed
        event(&mut events, 236, &flags);
        text(&mut events, 204, "Main");
        text(&mut events, 201, "Fruity Wrapper");
        event(&mut events, 213, wrapper);
        event(&mut events, 98, &0u16.to_le_bytes());
        event(&mut events, 236, &flags);
        text(&mut events, 204, "Bus");
        event(&mut events, 236, &flags);
        let imported = import_bytes(&appended(fixture(96, 32), &events)).unwrap();
        let main = imported.project.channels.iter().find(|c| c.role == suhina_model::ChannelRole::Master).unwrap();
        let device = &main.devices[0];
        assert!(matches!(device.kind, DeviceKind::Vst2 { .. } | DeviceKind::Vst3 { .. }));
        assert!(!device.enabled);
        assert!(!device.is_external_instrument());
    }
}

#[test]
fn existing_absolute_sample_beats_unrelated_project_local_names() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("suhina-import-absolute-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(root.join("Library")).unwrap();
    std::fs::create_dir_all(root.join("Project")).unwrap();
    let original = root.join("Library/same.wav");
    std::fs::write(&original, b"original").unwrap();
    std::fs::write(root.join("Project/same.wav"), b"unrelated").unwrap();
    let mut events = Vec::new();
    for (id, kind) in [(2u16, 0u8), (3, 4)] {
        event(&mut events, 64, &id.to_le_bytes());
        event(&mut events, 21, &[kind]);
        text(&mut events, 196, &original.to_string_lossy());
        event(&mut events, 51, &[0]);
    }
    let path = root.join("Project/authored.flp");
    std::fs::write(&path, appended(fixture(96, 32), &events)).unwrap();
    let imported = suhina_flp::import_file(&path).unwrap();
    let original = original.to_string_lossy().replace('\\', "/");
    assert!(
        imported.project.channels.iter().flat_map(|c| &c.devices).any(|d| d.sample_file() == Some(original.as_str()))
    );
    assert!(
        imported
            .project
            .contents
            .iter()
            .flat_map(|c| c.lanes.iter())
            .any(|l| matches!(&l.data,LaneData::Audio(r) if r.file==original))
    );
}

fn automation_events(target: u32) -> Vec<u8> {
    let mut e = Vec::new();
    let mut link = [0u8; 20];
    link[2..4].copy_from_slice(&3u16.to_le_bytes());
    link[8..12].copy_from_slice(&target.to_le_bytes());
    event(&mut e, 227, &link);
    event(&mut e, 64, &3u16.to_le_bytes());
    event(&mut e, 21, &[5]);
    let mut c = vec![0u8; 21];
    c[17..21].copy_from_slice(&2u32.to_le_bytes());
    for (delta, v) in [(0f64, 0.25f64), (2., 0.75)] {
        c.extend(delta.to_le_bytes());
        c.extend(v.to_le_bytes());
        c.extend([0; 4]);
        c.extend([2, 0, 0, 0]);
    }
    event(&mut e, 234, &c);
    event(&mut e, 51, &[0]);
    for start in [0u32, 384] {
        let mut c = [0u8; 32];
        c[..4].copy_from_slice(&start.to_le_bytes());
        c[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
        c[6..8].copy_from_slice(&3u16.to_le_bytes());
        c[8..12].copy_from_slice(&192u32.to_le_bytes());
        c[12..14].copy_from_slice(&498u16.to_le_bytes());
        event(&mut e, 233, &c);
    }
    e
}
#[test]
fn plugin_automation_is_on_its_owner_and_holds_across_gaps() {
    let bytes = appended(wrapper_fixture(false, false), &automation_events((2 << 16) | 0x8001));
    let p = import_bytes(&bytes).unwrap().project;
    assert!(p.track(suhina_model::TrackId(3)).is_none());
    let device = &p.channel(suhina_model::ChannelId(2)).unwrap().devices[0];
    let lane = &device.automation[0];
    assert_eq!(lane.param, "1");
    assert!(lane.enabled);
    assert!(lane.points.len() <= 8, "linear clips must remain editable: {} points", lane.points.len());
    let at: Vec<_> = lane.points.iter().filter(|p| p.beat == 4.).map(|p| p.value).collect();
    assert_eq!(at, vec![0.75, 0.25]);
    assert_eq!(lane.points.iter().filter(|p| p.beat == 2.).map(|p| p.value).collect::<Vec<_>>(), vec![0.25, 0.75]);
    let encoded = suhina_project_file::to_json(&p);
    assert_eq!(suhina_project_file::from_json(&encoded).unwrap(), p);
}
#[test]
fn neutral_layers_fan_out_to_existing_children_and_cycles_fail() {
    let mut base = fixture(96, 32);
    let parsed = suhina_flp::read_bytes(&base).unwrap();
    // Make the first source a layer and insert its children/levels at the end of its block.
    let first = parsed.events.iter().find(|e| e.id == 21).unwrap();
    base[first.offset + 1] = 3;
    let end = parsed.events.iter().find(|e| e.id == 51).unwrap().offset;
    let mut extra = Vec::new();
    event(&mut extra, 94, &1u16.to_le_bytes());
    let mut levels = Vec::new();
    for v in [6400u32, 12800, 0] {
        levels.extend(v.to_le_bytes());
    }
    event(&mut extra, 219, &levels);
    base.splice(end..end, extra);
    let len = (base.len() - 22) as u32;
    base[18..22].copy_from_slice(&len.to_le_bytes());
    let p = import_bytes(&base).unwrap().project;
    assert!(p.track(suhina_model::TrackId(0)).is_none());
    assert_eq!(p.channels.len(), 1);
    let content = &p.contents[0];
    let LaneData::Notes(notes) = &content.lanes[0].data else { panic!() };
    assert_eq!(notes.len(), 2);
    assert_eq!(notes.iter().map(|n| n.key).collect::<Vec<_>>(), [60, 61]);
    let parsed = suhina_flp::read_bytes(&base).unwrap();
    let child = parsed.events.iter().find(|e| e.id == 94).unwrap().offset;
    base[child + 1..child + 3].copy_from_slice(&0u16.to_le_bytes());
    assert!(import_bytes(&base).unwrap_err().to_string().contains("cyclic"));
}
#[cfg(test)]
fn patch_fixture(bad: bool) -> Vec<u8> {
    patch_fixture_links(if bad { 1 } else { 2 })
}

#[cfg(test)]
fn patch_fixture_links(output_flags: u32) -> Vec<u8> {
    let wrapper = wrapper_fixture(false, false);
    let parsed = suhina_flp::read_bytes(&wrapper).unwrap();
    let state = parsed.events.iter().find(|e| e.id == 213).unwrap();
    let suhina_flp::Payload::Data(state) = &state.payload else { panic!() };
    let mut patch = 4u32.to_le_bytes().to_vec();
    for (id, name, class) in [(1u32, "Notes", 1u32), (2, "Audio output", 0)] {
        let mut group = Vec::new();
        let mut port = vec![0; 28];
        port[..4].copy_from_slice(&id.to_le_bytes());
        port[8..12].copy_from_slice(&class.to_le_bytes());
        wrapper_record(&mut group, 1400, &port);
        wrapper_record(
            &mut group,
            1402,
            &name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<_>>(),
        );
        wrapper_record(&mut patch, 0, &group);
    }
    for n in 0..3u32 {
        let input = 10 + n * 2;
        let output = input + 1;
        let mut group = Vec::new();
        let mut header = vec![0; 16];
        header[..4].copy_from_slice(&3u32.to_le_bytes());
        header[4..8].copy_from_slice(&(100 + n).to_le_bytes());
        header[12..16].copy_from_slice(&(if n == 1 { 12u32 } else { 8u32 }).to_le_bytes());
        wrapper_record(&mut group, 5000, &header);
        wrapper_record(
            &mut group,
            5004,
            &"Fruity Wrapper".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect::<Vec<_>>(),
        );
        wrapper_record(&mut group, 5005, state);
        for (id, class, flags) in [(input, 1u32, 3u32), (output, 0, 2)] {
            let mut port = vec![0; 28];
            port[..4].copy_from_slice(&class.to_le_bytes());
            port[8..12].copy_from_slice(&id.to_le_bytes());
            port[16..20].copy_from_slice(&flags.to_le_bytes());
            wrapper_record(&mut group, 5009, &port);
        }
        wrapper_record(&mut patch, 0, &group);
        for (from, to, gain) in [(1, input, 1f32), (output, 2, 0.5)] {
            let mut link = vec![0; 32];
            link[..4].copy_from_slice(&to.to_le_bytes());
            link[8..12].copy_from_slice(&from.to_le_bytes());
            link[16..20].copy_from_slice(&gain.to_le_bytes());
            link[20..24].copy_from_slice(&2u32.to_le_bytes());
            if to == 2 {
                link[20..24].copy_from_slice(&output_flags.to_le_bytes());
            }
            let mut group = Vec::new();
            wrapper_record(&mut group, 1300, &link);
            wrapper_record(&mut patch, 0, &group);
        }
    }
    wrapper_record(&mut patch, 1001, &[]);
    let mut e = Vec::new();
    event(&mut e, 64, &2u16.to_le_bytes());
    event(&mut e, 21, &[2]);
    text(&mut e, 201, "Patcher");
    event(&mut e, 213, &patch);
    event(&mut e, 51, &[0]);
    appended(fixture(96, 32), &e)
}
#[test]
fn patcher_parallel_instruments_keep_states_and_cable_gains() {
    let p = import_bytes(&patch_fixture(false)).unwrap().project;
    let channel = p.channel(suhina_model::ChannelId(2)).unwrap();
    assert!(!channel.mute);
    let group = channel.devices[0].as_group().unwrap();
    assert_eq!(group.branches.len(), 3);
    for branch in &group.branches {
        assert!(!branch.mute);
        assert_eq!(branch.volume, 0.5);
        assert_eq!(branch.devices.len(), 1);
        assert!(branch.devices[0].is_external_instrument());
        assert!(branch.devices[0].state.is_some());
    }
    let p = import_bytes(&patch_fixture(true)).unwrap().project;
    let channel = p.channel(suhina_model::ChannelId(2)).unwrap();
    assert!(!channel.mute);
    assert!(channel.devices[0].is_missing_instrument());
    assert!(channel.devices[0].as_group().is_none());
}

#[test]
fn balance_automation_keeps_slot_identity_after_mixer_eq_insertion() {
    let mut e = Vec::new();
    for bus in 0..3 {
        event(&mut e, 236, &[0, 0, 0, 0, 12, 0, 0, 0]);
        if bus == 1 {
            text(&mut e, 201, "Fruity Balance");
            event(&mut e, 213, &[0; 8]);
            event(&mut e, 98, &2u16.to_le_bytes());
            event(&mut e, 235, &[1, 0, 0]);
        }
    }
    let mut param = [0u8; 12];
    param[4] = 0xd0;
    param[5] = 0x1f;
    param[6..8].copy_from_slice(&0x7040u16.to_le_bytes());
    param[8..12].copy_from_slice(&300i32.to_le_bytes());
    event(&mut e, 225, &param);
    e.extend(automation_events(0x70428001));
    let p = import_bytes(&appended(fixture(96, 32), &e)).unwrap().project;
    let bus = p.channel(suhina_model::ChannelId(131073)).unwrap();
    assert!(bus.devices[0].automation.is_empty());
    let d = &bus.devices[1];
    let lane = &d.automation[0];
    assert_eq!(lane.param, "gain");
    let expected = 20. * ((11f64.powf(0.25 * 1.25) - 1.) / 10.).log10();
    assert!((lane.points[1].value - expected).abs() < 1e-9);
    assert_eq!(d.params.iter().find(|p| p.id == "mute").unwrap().value, 0.);
}

#[test]
fn mixer_automation_uses_full_fader_travel() {
    let mut e = Vec::new();
    for bus in 0..3 {
        event(&mut e, 236, &[0, 0, 0, 0, 12, 0, 0, 0]);
        if bus == 1 {
            event(&mut e, 235, &[1, 0, 0]);
        }
    }
    e.extend(automation_events(0x70401fc0));
    let p = import_bytes(&appended(fixture(96, 32), &e)).unwrap().project;
    let lane = &p.channel(suhina_model::ChannelId(131073)).unwrap().automation[0];
    assert_eq!(lane.param, "volume");
    let after_jump = lane.points.iter().rfind(|p| p.beat == 0.).unwrap();
    assert!((after_jump.value - (11f64.powf(0.25 * 1.25) - 1.) / 10.).abs() < 1e-12);
}

#[test]
fn vst3_automation_uses_saved_parameter_ids_not_list_indices() {
    let bytes = appended(wrapper_fixture(true, false), &automation_events((2 << 16) | 0x8001));
    let p = import_bytes(&bytes).unwrap().project;
    assert_eq!(p.channel(suhina_model::ChannelId(2)).unwrap().devices[0].automation[0].param, "123");
}

#[test]
fn patcher_disconnected_audio_keeps_muted_instruments_without_a_dry_path() {
    let p = import_bytes(&patch_fixture_links(3)).unwrap().project;
    let group = p.channel(suhina_model::ChannelId(2)).unwrap().devices[0].as_group().unwrap();
    assert_eq!(group.branches.len(), 3);
    assert!(group.branches.iter().all(|b| b.mute && b.devices[0].is_external_instrument()));
}

#[test]
fn an_empty_sampler_is_a_sampler_without_an_invented_sound() {
    let mut e = Vec::new();
    event(&mut e, 64, &2u16.to_le_bytes());
    event(&mut e, 21, &[0]);
    text(&mut e, 203, "Empty sampler");
    event(&mut e, 51, &[0]);
    let result = import_bytes(&appended(fixture(96, 32), &e)).unwrap();
    let channel = result.project.channels.iter().find(|c| c.name == "Empty sampler").unwrap();
    let sampler = &channel.devices[0];
    assert_eq!(sampler.kind, DeviceKind::Builtin("sampler".into()));
    assert!(sampler.sample_file().is_none());
}

/// Mixer tracks Main, Kick, Pad and Bus: Kick plays to Main and sidechains Bus (a route at level
/// 0); Pad plays into Bus; Bus holds a wrapped plug-in whose sidechain hears Bus's input `port`
/// (1: Kick, 2: Pad, the tracks routed into Bus in track order).
fn sidechain_fixture(port: i32) -> Option<Vec<u8>> {
    sidechain_fixture_with(port, &[0, 3], 3, 3)
}

/// [`sidechain_fixture`] with Kick routed to `kick` (its route to `silent` at level 0) and the
/// wrapped plug-in on mixer track `on` (0 is Main).
fn sidechain_fixture_with(port: i32, kick: &[usize], silent: u8, on: usize) -> Option<Vec<u8>> {
    let file = wrapper_fixture(false, false);
    let parsed = suhina_flp::read_bytes(&file).ok()?;
    let wrapper = parsed.events.iter().find(|e| e.id == 213)?;
    let suhina_flp::Payload::Data(wrapper) = &wrapper.payload else { return None };
    let mut wrapper = wrapper.clone();
    // Two input buses: the main one (port 0) and the sidechain (`port`), both active.
    let inputs: Vec<u8> = [[0i32, 1, 0], [port, 1, 0]].iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    wrapper_record(&mut wrapper, 31, &inputs);
    let mut events = Vec::new();
    let flags = [0u8, 0, 0, 0, 12, 0, 0, 0];
    let routes = |to: &[usize]| {
        let mut on = [0u8; 4];
        for &t in to {
            on[t] = 1;
        }
        on
    };
    // The last bus is the current (monitor) one FL stores after the inserts.
    for (index, name) in ["Main", "Kick", "Pad", "Bus", "Current"].into_iter().enumerate() {
        text(&mut events, 204, name);
        event(&mut events, 236, &flags);
        if index == on {
            text(&mut events, 201, "Fruity Wrapper");
            event(&mut events, 213, &wrapper);
            event(&mut events, 98, &0u16.to_le_bytes());
        }
        match index {
            1 => event(&mut events, 235, &routes(kick)),
            2 => event(&mut events, 235, &routes(&[3])),
            3 => event(&mut events, 235, &routes(&[0])),
            _ => {}
        }
    }
    // Kick's route to `silent` at level 0: a sidechain, no sound.
    let mut level = [0u8; 12];
    level[4] = silent;
    level[5] = 0x20;
    level[6..8].copy_from_slice(&(0x7000u16 + 64).to_le_bytes());
    event(&mut events, 225, &level);
    // Voice 0 plays into Kick, Voice 1 into Pad (the fixture sends both to track 1).
    let mut file = fixture(96, 32);
    let second = file.windows(3).enumerate().filter(|(_, w)| *w == [104, 1, 0]).nth(1).map(|(i, _)| i)?;
    *file.get_mut(second + 1)? = 2;
    Some(appended(file, &events))
}

#[test]
fn a_sidechain_route_keys_the_plugin_that_picks_it_after_the_source_fader() {
    // Kick and Pad each play one voice, so they fold into Voice 0 and Voice 1: the key follows.
    let kick = suhina_model::ChannelId(0);
    let pad = suhina_model::ChannelId(1);
    let bus = suhina_model::ChannelId(131_075);
    for (port, source) in [(1, kick), (2, pad)] {
        let imported = import_bytes(&sidechain_fixture(port).unwrap()).unwrap();
        let p = &imported.project;
        let channel = p.channel(bus).unwrap();
        let device = channel.devices.iter().find(|d| matches!(d.kind, DeviceKind::Vst2 { .. })).unwrap();
        let key = device.inputs.first().unwrap_or_else(|| panic!("port {port}: {:?}", channel.devices));
        assert_eq!(
            key.source,
            suhina_model::SignalPoint::Channel { channel: source, tap: suhina_model::TapPoint::PostFader }
        );
        assert_eq!((key.port.as_str(), key.gain), ("key", 1.0));
        // The level-0 route carries no sound: it is no send.
        assert!(p.channel(kick).unwrap().sends.iter().all(|s| s.target != bus), "{:?}", p.channel(kick));
        assert_eq!(p.validate(), Ok(()));
        let encoded = suhina_project_file::to_json(p);
        assert_eq!(suhina_project_file::from_json(&encoded).unwrap().channel(bus).unwrap().devices, channel.devices);
    }
    // An input Bus does not have: no key, and the import says so.
    let imported = import_bytes(&sidechain_fixture(3).unwrap()).unwrap();
    let channel = imported.project.channel(bus).unwrap();
    assert!(channel.devices.iter().all(|d| d.inputs.is_empty()));
    assert!(imported.warnings.iter().any(|w| w.contains("sidechain")), "{:?}", imported.warnings);
    // None picked: nothing.
    let imported = import_bytes(&sidechain_fixture(0).unwrap()).unwrap();
    assert!(imported.project.channel(bus).unwrap().devices.iter().all(|d| d.inputs.is_empty()));
}

#[test]
fn a_track_routed_only_at_level_0_has_a_silent_output_and_a_live_post_fader_key() {
    // Kick's only route is to Bus at level 0: it plays nothing, and the plug-in on Bus hears it
    // after its fader, including subsequent fader edits and automation.
    let imported = import_bytes(&sidechain_fixture_with(1, &[3], 3, 3).unwrap()).unwrap();
    let p = &imported.project;
    let kick = suhina_model::ChannelId(0);
    let channel = p.channel(kick).unwrap();
    assert!(!channel.mute, "{channel:?}");
    let output = p.channel(channel.output.unwrap()).unwrap();
    assert_eq!(output.volume, 0.0);
    assert!(channel.sends.is_empty());
    let bus = p.channel(suhina_model::ChannelId(131_075)).unwrap();
    let device = bus.devices.iter().find(|d| matches!(d.kind, DeviceKind::Vst2 { .. })).unwrap();
    let key = device.inputs.first().unwrap_or_else(|| panic!("{:?}", bus.devices));
    assert_eq!(
        key.source,
        suhina_model::SignalPoint::Channel { channel: kick, tap: suhina_model::TapPoint::PostFader }
    );
    assert_eq!(key.gain, 1.0);
    assert_eq!(p.validate(), Ok(()));
}

#[test]
fn a_plugin_on_main_keys_from_a_track_routed_into_main() {
    // Main's inputs, in track order: Kick (1) and Bus (3); Pad plays into Bus.
    let main = suhina_model::ChannelId(131_072);
    let imported = import_bytes(&sidechain_fixture_with(1, &[0], 0, 0).unwrap()).unwrap();
    let p = &imported.project;
    let channel = p.channel(main).unwrap();
    let device = channel.devices.iter().find(|d| matches!(d.kind, DeviceKind::Vst2 { .. })).unwrap();
    let key = device.inputs.first().unwrap_or_else(|| panic!("{:?}", channel.devices));
    assert_eq!(key.source.channel(), suhina_model::ChannelId(0), "Kick, folded into Voice 0");
    assert_eq!(p.validate(), Ok(()));
    let imported = import_bytes(&sidechain_fixture_with(2, &[0], 0, 0).unwrap()).unwrap();
    let channel = imported.project.channel(main).unwrap();
    let device = channel.devices.iter().find(|d| matches!(d.kind, DeviceKind::Vst2 { .. })).unwrap();
    assert_eq!(device.inputs.first().map(|k| k.source.channel()), Some(suhina_model::ChannelId(131_075)));
}

/// A plug-in generator's channel plays its plug-in 3 dB above the level curve, as FL renders a
/// wrapped instrument at its default 78 % centred (`docs/spec/flp-model-mapping.md` › Channel
/// levels); a sampler channel stays on the curve.
#[test]
fn a_plug_in_generator_channel_plays_3_db_above_the_level_curve() {
    let imported = import_bytes(&wrapper_fixture(false, false)).unwrap();
    let channel = imported.project.channels.iter().find(|c| c.name == "Renamed source").unwrap();
    let utility = channel.devices.iter().find(|d| d.name == "Source level").unwrap();
    let gain = utility.params.iter().find(|p| p.id == "gain").unwrap().value;
    let curve = 20. * ((11f64.powf(10000. / 12800.) - 1.) / 10.).log10();
    assert!((gain - (curve + 20. * std::f64::consts::SQRT_2.log10())).abs() < 1e-9, "{gain} vs {curve}");
}

/// `original`'s events written again, each through `f`, which may change its payload or add
/// events after it.
fn rewritten(original: &[u8], mut f: impl FnMut(&mut Vec<u8>, u8, Vec<u8>)) -> Option<Vec<u8>> {
    let flp = suhina_flp::read_bytes(original).ok()?;
    let mut bytes = Vec::new();
    for e in flp.events {
        use suhina_flp::Payload;
        let payload = match e.payload {
            Payload::Byte(v) => vec![v],
            Payload::Word(v) => v.to_le_bytes().to_vec(),
            Payload::Dword(v) => v.to_le_bytes().to_vec(),
            Payload::Data(v) => v,
        };
        f(&mut bytes, e.id, payload);
    }
    Some(appended(original[..22].to_vec(), &bytes))
}

/// A channel's colour without the flag event (projects from FL versions before it) is its own;
/// FL's untouched grey is no choice, so the track gets the colour a new Suhina MIDI track gets.
#[test]
fn channel_colours_without_a_flag_are_kept_and_fl_grey_becomes_suhinas_default() {
    let with = |value: u32| {
        rewritten(&fixture(96, 32), |out, id, payload| {
            event(out, id, &payload);
            if id == 64 {
                event(out, 128, &value.to_le_bytes());
            }
        })
        .unwrap()
    };
    let own = import_bytes(&with(0x00202260)).unwrap().project;
    assert!(own.tracks.iter().filter(|t| t.id.0 < 2).all(|t| t.color == Some(suhina_model::Rgb(96, 34, 32))));
    // (92, 101, 106): FL's default channel colour.
    let grey = import_bytes(&with(0x006A655C)).unwrap().project;
    assert!(grey.tracks.iter().filter(|t| t.id.0 < 2).all(|t| t.color == Some(suhina_model::NEW_MIDI_COLOUR)));
    // A playlist row in FL's grey does not colour its clips or tracks either.
    let mut row = [0u8; 66];
    row[..4].copy_from_slice(&1u32.to_le_bytes());
    row[4..8].copy_from_slice(&0x00565148u32.to_le_bytes());
    row[12] = 1;
    let mut events = Vec::new();
    event(&mut events, 238, &row);
    let rowed = import_bytes(&appended(with(0x00202260), &events)).unwrap().project;
    assert!(rowed.tracks.iter().filter(|t| t.id.0 < 2).all(|t| t.color == Some(suhina_model::Rgb(96, 34, 32))));
}

/// The playlist may place a pattern with no notes, which has no pattern records: the clip plays
/// nothing and is left out with a warning, instead of stopping the import.
#[test]
fn a_placed_pattern_with_no_notes_is_left_out_with_a_warning() {
    let bytes = rewritten(&fixture(96, 32), |out, id, mut payload| {
        if id == 233 {
            // The second placement plays pattern 2, which has no records.
            payload[32 + 6..32 + 8].copy_from_slice(&0x5002u16.to_le_bytes());
        }
        event(out, id, &payload);
    })
    .unwrap();
    let imported = import_bytes(&bytes).unwrap();
    // The other placement stays, split over the pattern's two voices.
    assert_eq!(imported.project.arrangement.lanes.iter().flat_map(|l| &l.clips).count(), 2);
    assert!(
        imported.warnings.iter().any(|w| w.starts_with("1 playlist clips with nothing to play")),
        "{:?}",
        imported.warnings
    );
}

/// A sample recorded into a folder beside the project (FL's `Audio`) is found there after the
/// project moved, through the folder its stored path names.
#[test]
fn a_moved_projects_samples_are_found_in_their_own_folder_beside_it() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let folder = std::env::temp_dir().join(format!("suhina-import-audio-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(folder.join("Audio")).unwrap();
    let bytes = rewritten(&fixture(96, 32), |out, id, payload| {
        if id == 196 {
            text(out, 196, "Z:/gone/Project/Audio/take.wav");
        } else {
            event(out, id, &payload);
        }
    })
    .unwrap();
    let path = folder.join("moved.flp");
    std::fs::write(&path, &bytes).unwrap();
    std::fs::write(folder.join("Audio/take.wav"), b"authored existence fixture").unwrap();
    let found = suhina_flp::import_file(&path).unwrap();
    assert_eq!(found.project.channels[0].devices[0].sample_file(), Some("Audio/take.wav"));
    assert!(!found.warnings.iter().any(|w| w.contains("Sample not found")));
    let _ = std::fs::remove_dir_all(&folder);
}

/// A sample stored under FL's downloads variable is found in FL's shared data folder above the
/// project, and its audio clip knows its file's length, so its tempo can be set.
#[test]
fn a_sample_under_fls_downloads_variable_is_found_with_its_length() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let data = std::env::temp_dir().join(format!("suhina-fl-data-{}-{nonce}", std::process::id()));
    let project = data.join("FL Studio/Projects/Song");
    std::fs::create_dir_all(&project).unwrap();
    // One second, 8 kHz mono PCM, authored here.
    let mut wav = b"RIFF".to_vec();
    wav.extend(16036u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(8000u32.to_le_bytes());
    wav.extend(16000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(16000u32.to_le_bytes());
    wav.resize(16044, 0);
    std::fs::create_dir_all(data.join("Downloads/FL CLOUD Sounds")).unwrap();
    std::fs::write(data.join("Downloads/FL CLOUD Sounds/loop.wav"), &wav).unwrap();
    let mut e = Vec::new();
    event(&mut e, 64, &2u16.to_le_bytes());
    event(&mut e, 21, &[4]);
    text(&mut e, 196, "%FLStudioDownloadsData%/FL CLOUD Sounds/loop.wav");
    event(&mut e, 51, &[0]);
    let mut clip = [0u8; 32];
    clip[4..6].copy_from_slice(&0x5000u16.to_le_bytes());
    clip[6..8].copy_from_slice(&2u16.to_le_bytes());
    clip[8..12].copy_from_slice(&192u32.to_le_bytes());
    clip[12..14].copy_from_slice(&499u16.to_le_bytes());
    event(&mut e, 233, &clip);
    let path = project.join("song.flp");
    std::fs::write(&path, appended(fixture(96, 32), &e)).unwrap();
    let imported = suhina_flp::import_file(&path).unwrap();
    assert!(!imported.warnings.iter().any(|w| w.contains("loop.wav")), "{:?}", imported.warnings);
    let region = imported
        .project
        .contents
        .iter()
        .flat_map(|c| c.lanes.iter())
        .find_map(|l| if let LaneData::Audio(r) = &l.data { Some(r.clone()) } else { None })
        .unwrap();
    assert!(region.file.ends_with("Downloads/FL CLOUD Sounds/loop.wav"), "{}", region.file);
    assert_eq!(region.duration_seconds, Some(1.0));
    assert_eq!(region.sample_rate, Some(8000));
    let _ = std::fs::remove_dir_all(&data);
}
