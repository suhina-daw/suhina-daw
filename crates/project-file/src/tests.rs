// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Round trips of every part of the model, and files that are not projects or do not hold
//! together.

use super::*;
use suhina_model::automation::Curve;

/// A project with every kind of thing the model holds: a group, an audio and a note track, a
/// return, sends, built-in and plug-in devices with state, shared and audio contents, loops,
/// fades, a follow action, scenes with a tempo, stop buttons and extensions.
fn everything() -> Project {
    let ext =
        |key: &str| Extensions(vec![Extension { format: "als".into(), key: key.into(), data: vec![0, 1, 2, 255] }]);
    let clip = |content: u32, start: f64| Clip {
        offset: 0.5,
        loop_range: Some(LoopRange { start: 0.0, end: 2.0 }),
        fade_in: 0.25,
        fade_out: 1.0,
        fade_in_curve: FadeCurve { shape: FadeShape::SCurve, curve: -0.5 },
        fade_out_curve: FadeCurve { shape: FadeShape::EqualPower, curve: 0.75 },
        launch: LaunchSettings { mode: LaunchMode::Gate, legato: true, quantization: LaunchQuantization::Triplet(8) },
        ..Clip::new(ContentId(content), start, 4.0)
    };
    let follow = FollowAction {
        enabled: true,
        time: 6.0,
        linked: false,
        action_a: FollowKind::Jump(1),
        action_b: FollowKind::Other,
        chance_a: 30,
        chance_b: 70,
    };
    let channel = |id: u32, role: ChannelRole, output: Option<u32>| Channel {
        role,
        volume: 0.5,
        pan: -0.25,
        mute: id == 2,
        cue: id == 2,
        output: output.map(ChannelId),
        ..Channel::new(ChannelId(id), format!("Channel {id}"))
    };
    let mut keys = channel(2, ChannelRole::Regular, Some(4));
    keys.sends.push(Send { target: ChannelId(3), level: 0.7, pre_fader: true });
    keys.automation = vec![
        AutomationLane {
            param: "volume".into(),
            points: vec![
                AutomationPoint { beat: 0.0, value: 0.0, curve: Curve::bend(0.5) },
                AutomationPoint::new(8.0, 1.25),
            ],
            enabled: true,
        },
        AutomationLane { param: "mute".into(), points: vec![AutomationPoint::new(2.0, 1.0)], enabled: false },
    ];
    keys.devices = vec![
        Device {
            params: vec![DeviceParam::new("volume", -6.0)],
            automation: vec![AutomationLane {
                param: "volume".into(),
                points: vec![
                    AutomationPoint { beat: 1.0, value: -12.0, curve: Curve::bend(-0.75) },
                    AutomationPoint::new(3.5, 0.0),
                ],
                enabled: false,
            }],
            ..Device::new("FM", DeviceKind::Builtin("fm-synth".into()))
        },
        Device {
            enabled: false,
            state: Some(vec![9, 8, 7, 0, 255]),
            extensions: ext("Vst3ControllerState"),
            ..Device::new("Verb", DeviceKind::Vst3 { id: "0123456789ABCDEF0123456789ABCDEF".into() })
        },
        Device {
            state: Some(br#"{"format":"suhina-device","version":1}"#.to_vec()),
            params: vec![DeviceParam::new("gain", 0.5)],
            automation: vec![AutomationLane {
                param: "gain".into(),
                points: vec![AutomationPoint::new(0.0, 0.5), AutomationPoint::new(4.0, 1.0)],
                enabled: true,
            }],
            ..Device::new("Community Gain", DeviceKind::Sd)
        },
        Device { state: Some(Vec::new()), ..Device::new("Old", DeviceKind::Vst2 { id: 1_234_567 }) },
        Device {
            state: Some(b"/home/me/Suhina/NAM models/edge.nam".to_vec()),
            params: vec![DeviceParam::new("inputGain", -3.5), DeviceParam::new("eqMidFreq", 800.0)],
            ..Device::new("Amp", DeviceKind::Builtin("nam-amp-modeler".into()))
        },
        Device {
            state: Some(b"/home/me/Suhina/IRs/marshall.wav".to_vec()),
            params: vec![DeviceParam::new("volume", -1.9)],
            ..Device::new("Cab", DeviceKind::Builtin("nam-cab-loader".into()))
        },
        Device {
            params: vec![DeviceParam::new("3", 0.25)],
            automation: vec![AutomationLane {
                param: "3".into(),
                points: vec![
                    // Two control points outside the mirrored family: from version 4.
                    AutomationPoint { beat: 0.0, value: 0.0, curve: Curve { x1: 0.1, y1: 0.7, x2: 0.825, y2: 0.525 } },
                    AutomationPoint::new(4.0, 1.0),
                    AutomationPoint::new(4.0, 0.5),
                ],
                enabled: true,
            }],
            ..Device::new("Clap", DeviceKind::Clap { id: "com.example.clap".into() })
        },
        Device {
            extensions: ext("GlueCompressor"),
            ..Device::new("Glue", DeviceKind::Foreign { format: "als".into(), id: "GlueCompressor".into() })
        },
    ];
    let track = |id: u32, notes: bool, channel: Option<u32>, parent: Option<u32>| Track {
        color: Some(Rgb(0xD0, 0x8F, 0x6E)),
        content: ContentKinds { notes, audio: !notes },
        channel: channel.map(ChannelId),
        parent: parent.map(TrackId),
        ..Track::new(TrackId(id), format!("Track {id}"))
    };
    let mut project = Project {
        transport: Transport {
            tempo_bpm: 128.5,
            time_signature: TimeSignature { numerator: 7, denominator: 8 },
            loop_range: Some(LoopRange { start: 4.0, end: 12.5 }),
            loop_enabled: true,
            tempo_changes: vec![
                TempoPoint { beat: 14.0, bpm: 90.0, ramp: false },
                TempoPoint { beat: 30.5, bpm: 140.25, ramp: true },
            ],
            time_signature_changes: vec![TimeSignatureChange {
                bar: 4,
                time_signature: TimeSignature { numerator: 3, denominator: 4 },
            }],
        },
        tracks: vec![
            Track {
                color: None,
                audio_input: AudioInput { channels: InputChannels::Mono(3), monitor: Monitor::Off },
                ..track(1, false, Some(4), None)
            },
            Track {
                midi_input: MidiInput {
                    port: MidiPort::Named("Keys: Port 1, \"A\"".into()),
                    channel: Some(16),
                    ..Default::default()
                },
                ..track(2, true, Some(2), Some(1))
            },
            Track {
                midi_input: MidiInput { port: MidiPort::Nothing, channel: None, ..Default::default() },
                audio_input: AudioInput { channels: InputChannels::Stereo(255), monitor: Monitor::In },
                ..track(3, false, Some(5), Some(1))
            },
            Track { extensions: ext("ReturnTrack"), ..track(4, false, Some(3), None) },
        ],
        channels: vec![
            channel(1, ChannelRole::Master, None),
            keys,
            channel(3, ChannelRole::Effect, None),
            channel(4, ChannelRole::Submix, None),
            channel(5, ChannelRole::Regular, Some(4)),
        ],
        contents: vec![
            ClipContent {
                id: ContentId(1),
                name: "Chords".into(),
                length: 8.0,
                lanes: vec![ContentLane {
                    target: None,
                    data: LaneData::Notes(vec![
                        Note::new(0.0, 1.0, 60, 0.75),
                        Note { release_velocity: 0.0, channel: 15, ..Note::new(1.5, 0.25, 127, 1.0) },
                    ]),
                }],
                extensions: ext("MidiClip"),
                scale: None,
            },
            ClipContent {
                id: ContentId(2),
                name: "Loop".into(),
                length: 16.0,
                lanes: vec![ContentLane {
                    target: Some(TrackId(3)),
                    data: LaneData::Audio(AudioRegion {
                        absolute_file: Some("C:/Music/Samples/loop.wav".into()),
                        sample_rate: Some(44_100),
                        channels: Some(2),
                        duration_seconds: Some(7.5),
                        warped: true,
                        stretch: StretchMode::HighQuality,
                        warp_markers: vec![
                            WarpMarker { time: 0.0, audio_time: 0.0 },
                            WarpMarker { time: 16.0, audio_time: 7.5 },
                        ],
                        gain: 0.8,
                        transpose: -1.5,
                        stream: Some(3),
                        ..AudioRegion::new("Samples/loop.wav")
                    }),
                }],
                extensions: Extensions::default(),
                scale: None,
            },
        ],
        arrangement: Arrangement {
            lanes: vec![
                ArrangementLane {
                    id: Default::default(),
                    name: "Keys".into(),
                    track: Some(TrackId(2)),
                    clips: vec![clip(1, 0.0), clip(1, 8.0)],
                },
                ArrangementLane {
                    id: Default::default(),
                    name: "Free".into(),
                    track: None,
                    clips: vec![Clip { enabled: false, loop_range: None, ..clip(2, 4.0) }],
                },
            ],
            markers: vec![Marker::new(0.0, "Intro"), Marker::new(16.0, "Verse"), Marker::new(16.0, "")],
        },
        launcher: Launcher {
            mappings: vec![
                LaunchMapping {
                    source: MappingSource::key("Q").unwrap(),
                    target: MappingTarget::Slot { track: TrackId(2), scene: 1 },
                },
                LaunchMapping {
                    source: MappingSource::note(10, 36).unwrap(),
                    target: MappingTarget::Scene { scene: 0 },
                },
                // Its track is gone: kept, unused.
                LaunchMapping {
                    source: MappingSource::key("Space").unwrap(),
                    target: MappingTarget::Stop { track: TrackId(99) },
                },
            ],
            scenes: vec![
                Scene {
                    id: Default::default(),
                    name: "Intro".into(),
                    tempo_bpm: Some(100.0),
                    time_signature: Some(TimeSignature { numerator: 7, denominator: 8 }),
                },
                Scene::default(),
            ],
            columns: vec![LauncherColumn {
                track: TrackId(2),
                slots: vec![
                    Slot { clip: Some(Clip { follow_action: follow, ..clip(1, 0.0) }), has_stop: true },
                    Slot { clip: None, has_stop: false },
                ],
            }],
        },
        extensions: ext("LiveSet"),
        default_scale: None,
        next_id: 0,
    };
    // Every lane, clip and scene gets an id, as a project in the app has.
    project.assign_ids().unwrap();
    project
}

#[test]
fn every_part_of_a_project_survives_a_round_trip() {
    let project = everything();
    assert_eq!(project.validate(), Ok(()));
    assert_eq!(from_json(&to_json(&project)), Ok(project));
}

/// A channel's cue is written only when on, so a project without one reads and writes as it
/// did before the cue existed.
#[test]
fn the_cue_is_written_only_when_on() {
    let text = to_json(&everything());
    assert_eq!(text.matches("\"cue\"").count(), 1, "{text}");
    let mut quiet = everything();
    for channel in &mut quiet.channels {
        channel.cue = false;
    }
    assert!(!to_json(&quiet).contains("\"cue\""));
    assert_eq!(from_json(&to_json(&quiet)), Ok(quiet));
}

/// Mappings are written under the launcher with 1-based scenes, and left out when there are none.
#[test]
fn launch_mappings_are_written_with_their_targets() {
    let written: Value = serde_json::from_str(&to_json(&everything())).unwrap();
    assert_eq!(
        written["project"]["launcher"]["mappings"],
        json!([
            { "source": "key:Q", "target": "slot", "track": 2, "scene": 2 },
            { "source": "note:10:36", "target": "scene", "scene": 1 },
            { "source": "key:Space", "target": "stop", "track": 99 },
        ])
    );
    let empty: Value = serde_json::from_str(&to_json(&Project::default())).unwrap();
    assert!(empty["project"]["launcher"].get("mappings").is_none(), "{empty}");
}

#[test]
fn an_empty_project_survives_a_round_trip() {
    assert_eq!(from_json(&to_json(&Project::default())), Ok(Project::default()));
}

/// The notes of content 1, to change.
fn notes_of(project: &mut Project) -> Option<&mut Vec<Note>> {
    match &mut project.contents.first_mut()?.lanes.first_mut()?.data {
        LaneData::Notes(notes) => Some(notes),
        LaneData::Audio(_) => None,
    }
}

#[test]
fn a_muted_note_survives_a_round_trip() {
    let mut project = everything();
    if let Some(note) = notes_of(&mut project).and_then(|notes| notes.get_mut(1)) {
        note.muted = true;
    }
    let text = to_json(&project);
    assert!(text.contains(r#""muted": true"#), "{text}");
    // Only the muted note names a mute.
    assert_eq!(text.matches(r#""muted""#).count(), 1, "{text}");
    assert_eq!(from_json(&text), Ok(project));
}

#[test]
fn a_project_without_muted_notes_writes_no_muted_key() {
    // The fixture holds notes, all of them unmuted: none of them names a mute, so a project
    // saved now is the file a version before muted notes wrote.
    let text = to_json(&everything());
    assert!(!text.contains(r#""muted""#), "{text}");
}

#[test]
fn a_note_without_the_muted_key_reads_as_unmuted() {
    let read = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "contents": [ { "id": 1, "length": 4,
        "lanes": [ { "notes": [ { "time": 0, "duration": 1, "key": 60 } ] } ] } ] } }"#,
    );
    let project = read.expect("a version-1 file reads");
    let muted = match project.contents.first().and_then(|c| c.lanes.first()).map(|lane| &lane.data) {
        Some(LaneData::Notes(notes)) => notes.first().map(|n| n.muted),
        _ => None,
    };
    assert_eq!(muted, Some(false), "a note from before this reads as unmuted");
}

#[test]
fn files_are_written_and_read() {
    let dir = std::env::temp_dir().join(format!("suhina-project-file-{}", std::process::id()));
    let path = dir.join("song.suhina");
    assert_eq!(write_file(&path, &everything()), Ok(()));
    assert_eq!(read_file(&path), Ok(everything()));
    // Nothing is left beside it.
    let names: Vec<_> =
        std::fs::read_dir(&dir).map(|d| d.filter_map(Result::ok).map(|e| e.file_name()).collect()).unwrap_or_default();
    assert_eq!(names, ["song.suhina"]);
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_names_the_suhina_format() {
    let text = to_json(&Project::default());
    let root = serde_json::from_str::<Value>(&text).unwrap_or_default();
    assert_eq!(root.get("format").and_then(Value::as_str), Some("suhina"));
    assert_eq!(root.get("version").and_then(Value::as_u64), Some(VERSION));
}

#[test]
fn the_format_from_before_the_rename_is_refused_by_name() {
    // Files saved before October 2026 named another format; it is no longer read (ADR 0010
    // decision 12), and the message says what the file named.
    let old = to_json(&everything()).replacen(r#""format": "suhina""#, r#""format": "locr""#, 1);
    let error = from_json(&old).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains(r#""locr""#), "{error}");
    // A format longer than a name is cut short in the message.
    let long = format!(r#"{{ "format": "{}", "version": 1, "project": {{}} }}"#, "x".repeat(10_000));
    let error = from_json(&long).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.len() < 200, "{error}");
    assert!(from_json(r#"{ "format": 1, "version": 1, "project": {} }"#).is_err(), "a format that is not text");
}

#[test]
fn project_files_are_known_by_their_extension() {
    for name in ["a.suhina", "a.SUHINA", "dir/b c.suhina"] {
        assert!(is_project_file(Path::new(name)), "{name}");
    }
    for name in ["a.locr", "a.LOCR", "a.als", "a.json", "a", "suhina", "a.suhina.saving"] {
        assert!(!is_project_file(Path::new(name)), "{name}");
    }
}

#[test]
fn what_is_not_a_project_is_refused() {
    let refused = |text: &str| from_json(text).is_err();
    assert!(refused("not json"));
    assert!(refused("[1, 2]"));
    assert!(refused(r#"{ "version": 1, "project": {} }"#), "no format");
    assert!(refused(r#"{ "format": "als", "version": 1, "project": {} }"#));
    assert!(refused(r#"{ "format": "suhina", "project": {} }"#), "no version");
    let newer = format!(r#"{{ "format": "suhina", "version": {}, "project": {{}} }}"#, VERSION + 1);
    assert!(refused(&newer), "a newer version");
    assert!(
        from_json(&newer).unwrap_err().to_string().contains(&format!("reads up to version {VERSION}")),
        "the message names the newest version this Suhina reads"
    );
    assert!(refused(r#"{ "format": "suhina", "version": 1 }"#), "no project");
    assert_eq!(from_json(r#"{ "format": "suhina", "version": 1, "project": {} }"#), Ok(Project::default()));
    assert!(refused(r#"{ "format": "locr", "version": 1, "project": {} }"#), "the format from before the rename");
}

#[test]
fn hostile_values_are_errors_not_crashes() {
    let with = |project: &str| from_json(&format!(r#"{{ "format": "suhina", "version": 1, "project": {project} }}"#));
    for bad in [
        r#"{ "tracks": [ { "id": -1 } ] }"#,
        r#"{ "tracks": [ { "id": 99999999999 } ] }"#,
        r#"{ "tracks": [ { "id": 1, "color": "red" } ] }"#,
        r#"{ "tracks": "many" }"#,
        r#"{ "transport": { "tempo_bpm": 0 } }"#,
        r#"{ "transport": { "time_signature": [0, 4] } }"#,
        r#"{ "transport": { "time_signature": [300, 4] } }"#,
        r#"{ "launcher": { "scenes": [ { "name": "A", "time_signature": [4, 0] } ], "columns": [] } }"#,
        r#"{ "launcher": { "scenes": [ { "name": "A", "time_signature": "4/4" } ], "columns": [] } }"#,
        r#"{ "transport": { "tempo_changes": 3 } }"#,
        r#"{ "transport": { "tempo_changes": [ { "beat": 0, "bpm": 100 } ] } }"#,
        r#"{ "transport": { "tempo_changes": [ { "beat": -4, "bpm": 100 } ] } }"#,
        r#"{ "transport": { "tempo_changes": [ { "beat": 1e9, "bpm": 100 } ] } }"#,
        r#"{ "transport": { "tempo_changes": [ { "beat": 4, "bpm": 1000 } ] } }"#,
        r#"{ "transport": { "tempo_changes": [ { "beat": 4 } ] } }"#,
        r#"{ "transport": { "tempo_changes": [ { "beat": 4, "bpm": 100, "ramp": "yes" } ] } }"#,
        r#"{ "transport": { "time_signature_changes": [ { "bar": 0, "time_signature": [3, 4] } ] } }"#,
        r#"{ "transport": { "time_signature_changes": [ { "bar": -2, "time_signature": [3, 4] } ] } }"#,
        r#"{ "transport": { "time_signature_changes": [ { "bar": 2, "time_signature": [3, 5] } ] } }"#,
        r#"{ "transport": { "time_signature_changes": [ { "bar": 2, "time_signature": [3] } ] } }"#,
        r#"{ "transport": { "time_signature_changes": [ { "bar": 2 } ] } }"#,
        r#"{ "channels": [ { "id": 1, "role": "boss" } ] }"#,
        r#"{ "channels": [ { "id": 1, "pan": 3 } ] }"#,
        r#"{ "channels": [ { "id": 1, "cue": "on" } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "vst9" } } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" }, "automation": [ { "points": [] } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" }, "automation": [ { "param": "1", "points": [ { "beat": "one", "value": 1 } ] } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" }, "automation": 5 } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": [ { "param": "pan", "points": [ { "beat": 0, "value": 0, "curve": 1.5 } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": [ { "param": "pan", "points": [ { "beat": 0, "value": 0, "curve": "bent" } ] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": [ { "param": "pan", "enabled": "yes", "points": [] } ] } ] }"#,
        r#"{ "channels": [ { "id": 1, "automation": { "param": "pan" } } ] }"#,
        r#"{ "channels": [ { "id": 1, "devices": [ { "kind": { "type": "vst3", "id": "x" }, "state": "***" } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": 4, "lanes": [ { "notes": [ { "time": 0, "duration": 1, "key": 200 } ] } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": 4, "lanes": [ { "notes": [ { "time": 0, "duration": 1, "key": 60, "muted": "yes" } ] } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": 4, "lanes": [ { } ] } ] }"#,
        r#"{ "contents": [ { "id": 1, "length": "long" } ] }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "loop": [3, 1] } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_in_shape": "wobbly" } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_out_shape": 3 } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_in_curve": 2 } ] } ] } }"#,
        r#"{ "arrangement": { "markers": [ { "beat": -1, "name": "before" } ] } }"#,
        r#"{ "transport": { "loop": { "start": 4, "end": 4 } } }"#,
        r#"{ "transport": { "loop": { "start": -1, "end": 4 } } }"#,
        r#"{ "transport": { "loop": { "start": 8, "end": 4, "enabled": true } } }"#,
        r#"{ "transport": { "loop": { "start": 0 } } }"#,
        r#"{ "transport": { "loop": { "start": 0, "end": 4, "enabled": "yes" } } }"#,
        r#"{ "transport": { "loop": [0, 4] } }"#,
        r#"{ "arrangement": { "markers": [ { "beat": "soon" } ] } }"#,
        r#"{ "arrangement": { "markers": [ { "name": "nowhere" } ] } }"#,
        r#"{ "arrangement": { "markers": [ { "beat": 1, "name": 7 } ] } }"#,
        r#"{ "arrangement": { "markers": { "beat": 1 } } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": true } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "time": 0 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "time": 1e9 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "a": "up" } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "b": "jump 0" } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "chance_a": 101 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "chance_b": -3 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "enabled": "on" } } ] } ] } }"#,
        r#"{ "tracks": [ { "id": 1, "midi_input": { "channel": 0 } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "midi_input": { "channel": 17 } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "midi_input": { "channel": "one" } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "midi_input": { "port": "some" } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "midi_input": { "port": { "name": " " } } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "midi_input": { "port": 3 } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "midi_input": "keys" } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "channels": [0] } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "channels": [257] } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "channels": [1, 3] } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "channels": [2, 1] } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "channels": [1, 2, 3] } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "channels": 1 } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "monitor": "loud" } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": { "monitor": 1 } } ] }"#,
        r#"{ "tracks": [ { "id": 1, "audio_input": [1] } ] }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "launch": 3 } ] } ] } }"#,
        r#"{ "launcher": { "mappings": 3 } }"#,
        r#"{ "launcher": { "mappings": [ 3 ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q" } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q", "target": "pad", "scene": 1 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q", "target": "slot", "scene": 1 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q", "target": "slot", "track": 1, "scene": 0 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q", "target": "scene", "scene": -1 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q", "target": "stop" } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q", "target": "stop", "track": 99999999999 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "Q", "target": "scene", "scene": 1 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": 81, "target": "scene", "scene": 1 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "note:17:60", "target": "scene", "scene": 1 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "note:1:128", "target": "scene", "scene": 1 } ] } }"#,
        r#"{ "launcher": { "mappings": [ { "source": "key:Q", "target": "scene", "scene": 1 }, { "source": "key:Q", "target": "scene", "scene": 2 } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "launch": { "mode": "hold" } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "launch": { "mode": 1 } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "launch": { "legato": "yes" } } ] } ] } }"#,
        r#"{ "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "launch": { "quantization": "3 bars" } } ] } ] } }"#,
    ] {
        assert!(with(bad).is_err(), "{bad} was read");
    }
}

/// A scene written before scenes had a time signature has none, so it keeps the one playing.
#[test]
fn a_scene_without_a_time_signature_has_none() {
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "launcher": { "scenes": [ { "name": "A" } ], "columns": [] } } }"#,
    )
    .unwrap();
    assert_eq!(project.launcher.scenes[0].time_signature, None);
}

/// A lane's points are read in beat order, points at one beat in the order written.
#[test]
fn automation_points_are_read_in_beat_order() {
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "channels": [ { "id": 1, "devices": [ { "kind": { "type": "clap", "id": "x" },
            "automation": [ { "param": "2", "points": [ { "beat": 4, "value": 1 }, { "beat": 0, "value": 0 }, { "beat": 4, "value": 0.5 } ] } ] } ] } ] } }"#,
    )
    .unwrap();
    let points = &project.channels[0].devices[0].automation[0].points;
    let read: Vec<(f64, f64)> = points.iter().map(|p| (p.beat, p.value)).collect();
    assert_eq!(read, [(0.0, 0.0), (4.0, 1.0), (4.0, 0.5)]);
}

/// A curve and an enabled flag are written only when they are not the default, so a file
/// with straight lanes that play keeps its shape; missing, they read as a straight line that
/// plays. A channel without lanes writes none.
#[test]
fn automation_defaults_are_left_out_and_read_back() {
    let lane = AutomationLane { param: "pan".into(), points: vec![AutomationPoint::new(0.0, 0.5)], enabled: true };
    let mut project =
        from_json(r#"{ "format": "suhina", "version": 1, "project": { "channels": [ { "id": 1 } ] } }"#).unwrap();
    assert!(project.channels[0].automation.is_empty());
    assert!(!to_json(&project).contains("automation"));
    project.channels[0].automation = vec![lane.clone()];
    let json = to_json(&project);
    assert!(json.contains("automation") && !json.contains("curve") && !json.contains("\"enabled\": false"), "{json}");
    assert_eq!(from_json(&json).unwrap().channels[0].automation, [lane]);
}

/// A track written before MIDI inputs existed hears every port and channel; the default is not
/// written.
#[test]
fn a_track_without_a_midi_input_hears_every_controller() {
    let project =
        from_json(r#"{ "format": "suhina", "version": 1, "project": { "tracks": [ { "id": 1 } ] } }"#).unwrap();
    assert_eq!(project.tracks[0].midi_input, MidiInput::default());
    assert_eq!(MidiInput::default(), MidiInput { port: MidiPort::All, channel: None, ..Default::default() });
    assert!(!to_json(&project).contains("midi_input"));
    assert_eq!(project.tracks[0].audio_input, AudioInput::default());
    assert_eq!(AudioInput::default(), AudioInput { channels: InputChannels::None, monitor: Monitor::Auto });
    assert!(!to_json(&project).contains("audio_input"));
    let all_on_one = r#"{ "format": "suhina", "version": 1, "project": { "tracks": [ { "id": 1, "midi_input": { "port": "all", "channel": 10 } } ] } }"#;
    assert_eq!(
        from_json(all_on_one).unwrap().tracks[0].midi_input,
        MidiInput { port: MidiPort::All, channel: Some(10), ..Default::default() }
    );
}

#[test]
fn midi_monitoring_round_trips_and_bad_modes_are_rejected() {
    let mut project =
        from_json(r#"{ "format": "suhina", "version": 1, "project": { "tracks": [ { "id": 1 } ] } }"#).unwrap();
    for mode in [Monitor::Auto, Monitor::In, Monitor::Off] {
        project.tracks[0].midi_input.monitor = mode;
        assert_eq!(from_json(&to_json(&project)).unwrap(), project);
    }
    for mode in [serde_json::json!("sometimes"), serde_json::json!(true), serde_json::json!(3)] {
        let text = serde_json::json!({"format": "suhina", "version": 1, "project": {"tracks": [{"id": 1, "midi_input": {"monitor": mode}}]}}).to_string();
        assert!(from_json(&text).is_err());
    }
}

/// A file written before MIDI monitoring has a `midi_input` without `monitor`: it reads as Auto
/// and keeps its port and channel through a round trip.
#[test]
fn a_midi_input_from_before_monitoring_is_monitored_automatically() {
    let older = r#"{ "format": "suhina", "version": 1, "project": { "tracks": [ { "id": 1, "midi_input": { "port": { "name": "Keys" }, "channel": 3 } } ] } }"#;
    let project = from_json(older).unwrap();
    let wanted = MidiInput { port: MidiPort::Named("Keys".into()), channel: Some(3), monitor: Monitor::Auto };
    assert_eq!(project.tracks[0].midi_input, wanted);
    let again = from_json(&to_json(&project)).unwrap();
    assert_eq!(again.tracks[0].midi_input, wanted);
}

/// A clip written before fade shapes existed has linear, unbent fades, so it sounds the same.
#[test]
fn a_clip_without_fade_shapes_has_linear_fades() {
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "contents": [ { "id": 1, "length": 4 } ],
            "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "fade_in": 1, "fade_out": 1 } ] } ] } } }"#,
    )
    .unwrap();
    let clip = &project.arrangement.lanes[0].clips[0];
    assert_eq!((clip.fade_in_curve, clip.fade_out_curve), (FadeCurve::default(), FadeCurve::default()));
    assert_eq!(FadeCurve::default(), FadeCurve { shape: FadeShape::Linear, curve: 0.0 });
}

/// A loop region is kept while looping is off; a project without one writes no `loop`.
#[test]
fn the_loop_region_is_kept_while_off_and_left_out_when_there_is_none() {
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "transport": { "loop": { "start": 2, "end": 6 } } } }"#,
    )
    .unwrap();
    assert_eq!(project.transport.loop_range, Some(LoopRange { start: 2.0, end: 6.0 }));
    assert!(!project.transport.loop_enabled, "off unless the file says so");
    assert_eq!(from_json(&to_json(&project)).unwrap().transport, project.transport);
    assert!(!to_json(&Project::default()).contains(r#""loop""#));
}

/// Markers come back in beat order, however the file lists them; a project without any writes
/// no `markers`, as files did before markers existed.
#[test]
fn markers_are_read_in_beat_order_and_left_out_when_there_are_none() {
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "arrangement": { "markers": [
            { "beat": 32, "name": "Outro" }, { "beat": 0.5 }, { "beat": 8, "name": "Verse" } ] } } }"#,
    )
    .unwrap();
    let markers: Vec<(f64, &str)> = project.arrangement.markers.iter().map(|m| (m.beat, m.name.as_str())).collect();
    assert_eq!(markers, [(0.5, ""), (8.0, "Verse"), (32.0, "Outro")]);
    assert!(!to_json(&Project::default()).contains("markers"));
    assert!(to_json(&project).contains(r#""markers""#));
}

/// A project without tempo or time-signature changes is written as it was before they existed,
/// and a file from before then reads with none.
#[test]
fn a_project_without_changes_writes_no_change_lists() {
    let text = to_json(&Project::default());
    assert!(!text.contains("tempo_changes") && !text.contains("time_signature_changes"), "{text}");
    let project = from_json(r#"{ "format": "suhina", "version": 1, "project": { "transport": { "tempo_bpm": 90 } } }"#);
    let transport = project.map(|p| p.transport).unwrap_or_default();
    assert!(transport.tempo_changes.is_empty() && transport.time_signature_changes.is_empty());
    let too_many = format!(
        r#"{{ "format": "suhina", "version": 1, "project": {{ "transport": {{ "tempo_changes": [{}] }} }} }}"#,
        vec![r#"{ "beat": 1, "bpm": 100 }"#; suhina_model::MAX_TEMPO_CHANGES + 1].join(",")
    );
    assert!(from_json(&too_many).is_err());
}

/// A clip without a follow action writes none, and a file written before follow actions reads
/// as off.
#[test]
fn a_clip_without_a_follow_action_has_it_off() {
    let mut project = everything();
    if let Some(clip) = project.launcher.columns[0].slots[0].clip.as_mut() {
        clip.follow_action = FollowAction::default();
    }
    let text = to_json(&project);
    assert!(!text.contains("\"follow\""), "{text}");
    let read = from_json(&text).unwrap();
    assert_eq!(read.launcher.columns[0].slots[0].clip.as_ref().unwrap().follow_action, FollowAction::default());
    // Only what is given changes from the default.
    let project = from_json(
        r#"{ "format": "suhina", "version": 1, "project": { "contents": [ { "id": 1, "length": 4 } ],
            "arrangement": { "lanes": [ { "clips": [ { "content": 1, "start": 0, "length": 4, "follow": { "enabled": true, "b": "STOP" } } ] } ] } } }"#,
    )
    .unwrap();
    let follow = project.arrangement.lanes[0].clips[0].follow_action;
    assert_eq!(follow, FollowAction { enabled: true, action_b: FollowKind::Stop, ..FollowAction::default() });
}

#[test]
fn nested_groups_survive_a_round_trip_and_a_group_cycle_is_refused() {
    // Track 3 moves into a group of its own (track 5), which sits inside group 1.
    let mut project = everything();
    let mut inner = project.tracks[0].clone();
    inner.id = TrackId(5);
    inner.channel = Some(ChannelId(6));
    inner.parent = Some(TrackId(1));
    project.tracks.insert(1, inner);
    project.tracks[3].parent = Some(TrackId(5));
    let mut bus = project.channels[3].clone();
    bus.id = ChannelId(6);
    bus.output = Some(ChannelId(4));
    project.channels.push(bus);
    project.channels[4].output = Some(ChannelId(6));
    assert_eq!(project.validate(), Ok(()));
    assert_eq!(project.group_depth(TrackId(3)), Some(2));
    assert_eq!(from_json(&to_json(&project)), Ok(project.clone()));

    project.tracks[0].parent = Some(TrackId(5));
    let error = from_json(&to_json(&project)).err().map(|e| e.0).unwrap_or_default();
    assert!(error.contains("inside itself"), "{error}");
}

/// A clip written before launch settings launches as before: trigger, no legato, the global
/// quantisation; one with some of the keys takes the defaults for the others. A clip with the
/// defaults is written without them.
#[test]
fn launch_settings_default_when_missing_and_are_written_only_when_changed() {
    let read = |launch: &str| {
        let text = format!(
            r#"{{ "format": "suhina", "version": 1, "project": {{ "contents": [ {{ "id": 1, "length": 4 }} ],
            "arrangement": {{ "lanes": [ {{ "clips": [ {{ "content": 1, "start": 0, "length": 4 {launch} }} ] }} ] }} }} }}"#
        );
        from_json(&text).unwrap().arrangement.lanes[0].clips[0].launch
    };
    assert_eq!(read(""), LaunchSettings::default());
    assert_eq!(read(r#", "launch": null"#), LaunchSettings::default());
    assert_eq!(
        read(r#", "launch": { "mode": "toggle" }"#),
        LaunchSettings { mode: LaunchMode::Toggle, ..LaunchSettings::default() }
    );
    assert_eq!(
        read(r#", "launch": { "legato": true, "quantization": "none" }"#),
        LaunchSettings { legato: true, quantization: LaunchQuantization::None, ..LaunchSettings::default() }
    );
    let mut project = everything();
    project.launcher.columns[0].slots[0].clip.as_mut().unwrap().launch = LaunchSettings::default();
    let json = to_json(&project);
    let written: Value = serde_json::from_str(&json).unwrap();
    let slot = &written["project"]["launcher"]["columns"][0]["slots"][0]["clip"];
    assert!(slot.get("launch").is_none(), "{slot}");
    let arrangement = &written["project"]["arrangement"]["lanes"][0]["clips"][0];
    assert_eq!(arrangement["launch"], json!({ "mode": "gate", "legato": true, "quantization": "1/8T" }));
}

#[test]
fn a_project_that_does_not_hold_together_is_refused() {
    let mut project = everything();
    project.tracks.push(project.tracks[0].clone());
    let error = from_json(&to_json(&project)).err().map(|e| e.0).unwrap_or_default();
    assert!(error.contains("used twice"), "{error}");
}

#[test]
fn numbers_that_are_not_finite_are_written_as_zero() {
    let mut project = everything();
    project.channels[0].volume = f64::NAN;
    let read = from_json(&to_json(&project)).map(|p| p.channels[0].volume);
    assert_eq!(read, Ok(0.0));
}

#[test]
fn audio_paths_follow_a_project_saved_elsewhere() {
    let base = std::env::temp_dir().join(format!("suhina-project-file-rebase-{}", std::process::id()));
    let samples = base.join("old").join("Samples");
    std::fs::create_dir_all(&samples).unwrap_or_default();
    std::fs::write(samples.join("loop.wav"), b"RIFF").unwrap_or_default();
    let mut project = everything();
    rebase_audio(&mut project, &base.join("old"), &base.join("new").join("songs"));
    let LaneData::Audio(region) = &project.contents[1].lanes[0].data else { return };
    assert_eq!(region.file, "../../old/Samples/loop.wav");
    let absolute = region.absolute_file.clone().unwrap_or_default();
    assert!(Path::new(&absolute).ends_with("old/Samples/loop.wav"), "{absolute}");
    // Saved beside it again: the path is relative to the same folder.
    rebase_audio(&mut project, &base.join("new").join("songs"), &base.join("old"));
    let LaneData::Audio(region) = &project.contents[1].lanes[0].data else { return };
    assert_eq!(region.file, "Samples/loop.wav");
    let _clean = std::fs::remove_dir_all(&base);
}

#[test]
fn a_missing_audio_file_is_found_by_its_absolute_path() {
    let base = std::env::temp_dir().join(format!("suhina-project-file-absolute-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap_or_default();
    let file = base.join("kept.wav");
    std::fs::write(&file, b"RIFF").unwrap_or_default();
    let mut project = everything();
    if let LaneData::Audio(region) = &mut project.contents[1].lanes[0].data {
        region.file = "gone/kept.wav".into();
        region.absolute_file = Some(file.display().to_string());
    }
    rebase_audio(&mut project, &base.join("elsewhere"), &base);
    let LaneData::Audio(region) = &project.contents[1].lanes[0].data else { return };
    assert_eq!(region.file, "kept.wav");
    let _clean = std::fs::remove_dir_all(&base);
}

#[test]
fn an_empty_audio_region_is_left_alone_by_rebase_audio() {
    // An empty audio clip has no file yet; saving elsewhere does not invent one.
    let mut project = everything();
    project.contents.push(ClipContent {
        id: ContentId(900),
        name: "Audio clip".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion { ..AudioRegion::new("") }) }],
        extensions: Extensions::default(),
        scale: None,
    });
    rebase_audio(&mut project, Path::new("/nowhere"), Path::new("/elsewhere"));
    let LaneData::Audio(region) = &project.contents[2].lanes[0].data else { return };
    assert_eq!(region.file, "", "the empty clip still names no file");
    assert!(region.absolute_file.is_none(), "no absolute path is invented either: {:?}", region.absolute_file);
}

#[test]
fn clip_scales_and_the_default_scale_survive_a_round_trip() {
    let mut project = everything();
    let d_dorian = Scale::new(2, ScaleMode::Dorian).unwrap();
    let f_sharp_blues = Scale::new(6, ScaleMode::Blues).unwrap();
    if let Some(content) = project.contents.first_mut() {
        content.scale = Some(d_dorian);
    }
    project.default_scale = Some(f_sharp_blues);
    let text = to_json(&project);
    let written: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(written["project"]["contents"][0]["scale"], json!({ "root": 2, "mode": "dorian" }));
    assert_eq!(written["project"]["default_scale"], json!({ "root": 6, "mode": "blues" }));
    // Only the content with a scale names one.
    assert_eq!(text.matches(r#""scale""#).count(), 1, "{text}");
    assert_eq!(from_json(&text), Ok(project));
    for mode in ScaleMode::ALL {
        let one = Project { default_scale: Scale::new(11, mode), ..Project::default() };
        assert_eq!(from_json(&to_json(&one)), Ok(one), "{mode:?}");
    }
}

#[test]
fn a_project_without_scales_is_written_as_before() {
    // No content has a scale and there is no default: neither key appears, so the file is the
    // one a version before scales wrote, and it reads back with none.
    let text = to_json(&everything());
    assert!(!text.contains(r#""scale""#) && !text.contains(r#""default_scale""#), "{text}");
    let read = from_json(&text).unwrap();
    assert!(read.default_scale.is_none() && read.contents.iter().all(|c| c.scale.is_none()));
}

#[test]
fn a_bad_scale_is_an_error_naming_where_it_is() {
    let with = |project: &str| from_json(&format!(r#"{{ "format": "suhina", "version": 1, "project": {project} }}"#));
    let content =
        |scale: &str| format!(r#"{{ "contents": [ {{ "id": 1, "length": 4, "lanes": [], "scale": {scale} }} ] }}"#);
    for (bad, says) in [
        (content(r#"{ "root": 12, "mode": "major" }"#), "project.contents[0].scale.root: expected 0 to 11, got 12"),
        (content(r#"{ "root": -1, "mode": "major" }"#), "project.contents[0].scale.root"),
        (content(r#"{ "root": "C", "mode": "major" }"#), "project.contents[0].scale.root"),
        (content(r#"{ "mode": "major" }"#), "project.contents[0].scale.root"),
        (content(r#"{ "root": 0, "mode": "Major" }"#), r#"project.contents[0].scale.mode: unknown scale mode "Major""#),
        (content(r#"{ "root": 0, "mode": "wholeTone" }"#), "unknown scale mode"),
        (content(r#"{ "root": 0 }"#), "project.contents[0].scale.mode"),
        (content(r#"{ "root": 0, "mode": 3 }"#), "project.contents[0].scale.mode"),
        (content(r#""C major""#), "project.contents[0].scale: expected an object"),
        (r#"{ "default_scale": { "root": 99999999999, "mode": "minor" } }"#.to_string(), "project.default_scale.root"),
        (r#"{ "default_scale": [0, "minor"] }"#.to_string(), "project.default_scale: expected an object"),
    ] {
        match with(&bad) {
            Err(e) => assert!(e.0.contains(says), "{bad}: {e}"),
            Ok(_) => panic!("{bad} was read"),
        }
    }
    // `null` is the same as no scale.
    let read = with(&content("null")).unwrap();
    assert_eq!(read.contents.first().and_then(|c| c.scale), None);
}

/// `project` as text with the ids of its lanes, clips and scenes and its `next_id` taken out: a
/// file written before ids. Tracks, channels and contents keep theirs.
fn without_ids(project: &Project) -> String {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("id");
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut file: Value = serde_json::from_str(&to_json(project)).unwrap();
    strip(&mut file["project"]["arrangement"]);
    strip(&mut file["project"]["launcher"]);
    file["project"].as_object_mut().unwrap().remove("next_id");
    file.to_string()
}

/// The first clip of the first arrangement lane of a project file.
fn first_clip(file: &mut Value) -> &mut Value {
    &mut file["project"]["arrangement"]["lanes"][0]["clips"][0]
}

#[test]
fn lanes_clips_and_scenes_keep_their_ids_through_a_round_trip() {
    let project = everything();
    let written: Value = serde_json::from_str(&to_json(&project)).unwrap();
    let lane = &written["project"]["arrangement"]["lanes"][0];
    assert_eq!(lane["id"], json!(project.arrangement.lanes[0].id.0));
    assert_eq!(lane["clips"][0]["id"], json!(project.arrangement.lanes[0].clips[0].id.0));
    assert_eq!(written["project"]["launcher"]["scenes"][1]["id"], json!(project.launcher.scenes[1].id.0));
    assert_eq!(written["project"]["next_id"], json!(project.next_id));

    // Ids chosen by hand, out of order and with gaps, stay as they are.
    let mut chosen = everything();
    chosen.arrangement.lanes[0].clips[0].id = ClipId(900);
    chosen.launcher.scenes[0].id = SceneId(40);
    chosen.next_id = 901;
    assert_eq!(from_json(&to_json(&chosen)), Ok(chosen));
}

#[test]
fn a_file_without_ids_gives_them_in_document_order_and_always_the_same() {
    let project = everything();
    let old = without_ids(&project);
    assert!(!old.contains("next_id"), "{old}");
    let read = from_json(&old).unwrap();
    assert_eq!(read, project, "everything() got its ids in document order, as the reader gives them");
    assert_eq!(from_json(&old).unwrap(), read, "the same file reads with the same ids every time");
    let lanes: Vec<u32> = read.arrangement.lanes.iter().map(|l| l.id.0).collect();
    let first_clips: Vec<u32> = read.arrangement.lanes[0].clips.iter().map(|c| c.id.0).collect();
    assert_eq!(lanes[0], 1);
    assert_eq!(first_clips.first(), Some(&2), "a lane's clips follow the lane");

    // The lane's two clips written the other way round, in a file without ids: ids come from
    // where things are written, so each place keeps its id and the clips swap them. The objects
    // after them (the scenes) keep theirs.
    let mut moved: Value = serde_json::from_str(&old).unwrap();
    let clips = moved["project"]["arrangement"]["lanes"][0]["clips"].as_array_mut().unwrap();
    assert_eq!(clips.len(), 2, "everything() has two clips on its first lane");
    clips.reverse();
    let moved = from_json(&moved.to_string()).unwrap();
    let (before, after) = (&read.arrangement.lanes[0].clips, &moved.arrangement.lanes[0].clips);
    assert_eq!((after[0].id, after[1].id), (before[0].id, before[1].id), "ids follow the places");
    assert_eq!((after[0].start, after[1].start), (before[1].start, before[0].start), "the clips swapped");
    assert_eq!(moved.launcher.scenes, read.launcher.scenes, "the scenes after the lanes keep their ids");
}

#[test]
fn an_object_without_an_id_in_a_file_with_ids_gets_one_after_them() {
    let mut file: Value = serde_json::from_str(&to_json(&everything())).unwrap();
    let next = file["project"]["next_id"].as_u64().unwrap();
    file["project"]["launcher"]["scenes"][0].as_object_mut().unwrap().remove("id");
    let read = from_json(&file.to_string()).unwrap();
    assert_eq!(u64::from(read.launcher.scenes[0].id.0), next, "the next id, not one of the others");
    assert_eq!(u64::from(read.next_id), next + 1);
    assert_eq!(read.validate(), Ok(()));

    // A next_id that falls behind the ids in use moves past them, so no new object repeats one.
    file["project"]["next_id"] = json!(1);
    let read = from_json(&file.to_string()).unwrap();
    assert!(read.next_id > read.max_object_id());
}

#[test]
fn bad_ids_are_errors_not_crashes() {
    let good: Value = serde_json::from_str(&to_json(&everything())).unwrap();
    let with = |change: &dyn Fn(&mut Value)| {
        let mut file = good.clone();
        change(&mut file);
        from_json(&file.to_string())
    };
    let clip = first_clip;
    let refused = |result: Result<Project, ProjectFileError>, says: &str| match result {
        Err(e) => assert!(e.0.contains(says), "{e} should say {says:?}"),
        Ok(_) => panic!("a file with a bad id was read; it should say {says:?}"),
    };

    // The same id twice: two clips, two scenes, two lanes.
    let other = good["project"]["arrangement"]["lanes"][0]["clips"][1]["id"].clone();
    refused(with(&|f| clip(f)["id"] = other.clone()), "is used twice");
    let scene = good["project"]["launcher"]["scenes"][0]["id"].clone();
    refused(with(&|f| f["project"]["launcher"]["scenes"][1]["id"] = scene.clone()), "scene id");
    let lane = good["project"]["arrangement"]["lanes"][0]["id"].clone();
    refused(with(&|f| f["project"]["arrangement"]["lanes"][1]["id"] = lane.clone()), "lane id");
    // A slot's clip shares the counter with the arrangement's.
    let launched = good["project"]["launcher"]["columns"][0]["slots"][0]["clip"]["id"].clone();
    refused(with(&|f| clip(f)["id"] = launched.clone()), "is used twice");
    // Lanes, clips and scenes share the counter: a clip with a lane's id is refused too.
    refused(with(&|f| clip(f)["id"] = lane.clone()), "by a clip, a lane or a scene at once");

    // 0 is no id, and the largest value is never one.
    refused(with(&|f| clip(f)["id"] = json!(0)), "clips[0].id");
    refused(with(&|f| f["project"]["launcher"]["scenes"][0]["id"] = json!(u32::MAX)), "scenes[0].id");
    for bad in [json!(-1), json!(1.5), json!("7"), json!(u64::from(u32::MAX) + 1), json!([1])] {
        refused(with(&|f| clip(f)["id"] = bad.clone()), "clips[0].id");
    }

    // A counter beyond u32, or one too near the end to give the objects without ids theirs.
    refused(with(&|f| f["project"]["next_id"] = json!(u64::from(u32::MAX) + 1)), "next_id");
    refused(with(&|f| f["project"]["next_id"] = json!(-3)), "next_id");
    refused(
        with(&|f| {
            f["project"]["next_id"] = json!(u32::MAX);
            clip(f).as_object_mut().unwrap().remove("id");
        }),
        "every object id",
    );
    // The largest id there is opens, and the project then says it can name nothing new.
    let mut full = with(&|f| clip(f)["id"] = json!(u32::MAX - 1)).unwrap();
    assert_eq!(full.next_id, u32::MAX);
    assert_eq!(full.new_clip_id(), Err(suhina_model::IdsExhausted));
}

/// A channel whose chain holds a device group with two branches, one holding a nested group,
/// mapped macros and extensions on the group and a branch.
fn with_a_group() -> Project {
    let ext = Extensions(vec![Extension { format: "als".into(), key: "Rack".into(), data: vec![9, 8] }]);
    let compressor = Device::new("Compressor", DeviceKind::Builtin("compressor".into()));
    let mut inner = Device::group("Inner", DeviceGroup::default());
    if let Some(g) = inner.as_group_mut() {
        g.branches[0].devices.push(Device {
            state: Some(vec![1, 2, 3]),
            ..Device::new("Synth", DeviceKind::Clap { id: "com.example.synth".into() })
        });
    }
    let branches = vec![
        Branch { name: "Dry".into(), ..Branch::new("") },
        Branch {
            name: "Crushed".into(),
            volume: 0.5,
            pan: -0.25,
            mute: true,
            solo: true,
            devices: vec![compressor, inner],
            extensions: ext.clone(),
        },
    ];
    let mut group = DeviceGroup::with_branches(branches);
    group.macros[0].name = "Crush".into();
    group.macros[0].mappings.push(MacroMapping { device: vec![1, 0], param: "threshold".into(), min: 0.8, max: 0.2 });
    group.macros[1].mappings.push(MacroMapping { device: vec![1, 1], param: "macro.1".into(), min: 0.0, max: 1.0 });
    let mut device = Device::group("Parallel crush", group);
    device.params = vec![DeviceParam::new("mix", 0.75), DeviceParam::new("macro.1", 0.4)];
    device.extensions = ext;
    let mut channel = Channel::new(ChannelId(1), "Drums");
    channel.devices = vec![Device::new("EQ", DeviceKind::Builtin("dj-eq".into())), device];
    Project { channels: vec![channel], ..Project::default() }
}

#[test]
fn a_device_group_survives_a_round_trip() {
    let project = with_a_group();
    assert_eq!(from_json(&to_json(&project)), Ok(project));
}

/// Every feature at once: [`everything`] with [`with_a_group`]'s device group on its first
/// channel. A field added to the model is added here too, so the shape test below sees it.
fn every_feature() -> Project {
    let mut project = everything();
    project.channels[0].devices.extend(with_a_group().channels[0].devices.clone());
    project
}

fn version_file(version: u64) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/versions/v{version}.suhina"))
}

/// One file per version, as that version wrote it, and every one of them still reads: version 1
/// without the device group it could not hold.
#[test]
fn a_file_of_every_version_reads() {
    for version in 1..=VERSION {
        let path = version_file(version);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut expected = every_feature();
        if version == 1 {
            expected.channels[0].devices.retain(|d| d.as_group().is_none());
        }
        if version < 4 {
            // A curve outside the mirrored family needs two control points.
            for point in expected
                .channels
                .iter_mut()
                .flat_map(|c| &mut c.devices)
                .flat_map(|d| &mut d.automation)
                .flat_map(|l| &mut l.points)
            {
                if point.curve.bend_of().is_none() {
                    point.curve = Curve::STRAIGHT;
                }
            }
        }
        assert_eq!(from_json(&text), Ok(expected), "version {version}");
    }
    assert!(!version_file(VERSION + 1).exists(), "a file for a version this Suhina does not write");
}

/// What a file holds is pinned to its version: a change to what is written fails here until
/// [`VERSION`] is raised and the new version's file added (`SUHINA_BLESS=1` writes it, never
/// over the file of a version that shipped), with a migration for the files before it.
#[test]
fn what_is_written_is_the_current_versions_file() {
    let path = version_file(VERSION);
    let written = to_json(&every_feature());
    if suhina_test_support::blessing() && !path.exists() {
        std::fs::write(&path, &written).unwrap();
        return;
    }
    let pinned = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; write it with SUHINA_BLESS=1", path.display()))
        .replace("\r\n", "\n");
    assert!(
        written == pinned,
        "what a .suhina file holds changed, and version {VERSION}'s file is what that version shipped: \
         raise VERSION, add tests/versions/v{}.suhina (SUHINA_BLESS=1 writes it) and say what changed \
         in docs/spec/suhina-format.md › Versions",
        VERSION + 1
    );
}

#[test]
fn a_group_reads_from_the_spec_example_with_defaults_filled_in() {
    let text = r#"{ "format": "suhina", "version": 2, "project": { "channels": [ { "id": 1, "devices": [ {
        "name": "Parallel crush",
        "kind": { "type": "group",
          "branches": [ { "name": "Dry", "devices": [] },
                        { "name": "Crushed", "volume": 0.5, "devices": [ { "name": "Compressor", "kind": { "type": "builtin", "id": "compressor" } } ] } ],
          "macros": [ { "name": "Crush", "mappings": [ { "device": [1, 0], "param": "threshold", "min": 0.8, "max": 0.2 } ] },
                      { "name": "Macro 2" }, { "name": "Macro 3" }, { "name": "Macro 4" },
                      { "name": "Macro 5" }, { "name": "Macro 6" }, { "name": "Macro 7" }, { "name": "Macro 8" } ] },
        "params": [ { "id": "mix", "value": 1.0 } ] } ] } ] } }"#;
    let project = from_json(text).expect("the spec's example reads");
    let group = project.channels[0].devices[0].as_group().cloned().unwrap_or_default();
    assert_eq!(group.branches.len(), 2);
    assert_eq!((group.branches[0].volume, group.branches[0].pan, group.branches[0].mute), (1.0, 0.0, false));
    assert_eq!(group.branches[1].volume, 0.5);
    assert_eq!(group.macros[0].mappings[0].device, vec![1, 0]);
    assert_eq!(group.macros[1].mappings, vec![]);
}

#[test]
fn malformed_groups_are_errors_not_crashes() {
    let with = |kind: &str| {
        from_json(&format!(
            r#"{{ "format": "suhina", "version": 2, "project": {{ "channels": [ {{ "id": 1, "devices": [ {{ "kind": {kind} }} ] }} ] }} }}"#
        ))
    };
    let eight = r#"[ {}, {}, {}, {}, {}, {}, {}, {} ]"#;
    let fine = format!(r#"{{ "type": "group", "branches": [ {{ "devices": [] }} ], "macros": {eight} }}"#);
    assert!(with(&fine).is_ok(), "{:?}", with(&fine));
    let mapped = |mapping: &str, devices: &str| {
        format!(
            r#"{{ "type": "group", "branches": [ {{ "devices": {devices} }} ], "macros": [ {{ "mappings": [ {mapping} ] }}, {{}}, {{}}, {{}}, {{}}, {{}}, {{}}, {{}} ] }}"#
        )
    };
    let one = r#"[ { "kind": { "type": "sd" } } ]"#;
    for bad in [
        r#"{ "type": "group" }"#.to_string(),
        r#"{ "type": "group", "branches": "two", "macros": [] }"#.to_string(),
        format!(r#"{{ "type": "group", "branches": [], "macros": {eight} }}"#),
        r#"{ "type": "group", "branches": [ { } ], "macros": [ {} ] }"#.to_string(),
        format!(r#"{{ "type": "group", "branches": [ {{ "volume": -1 }} ], "macros": {eight} }}"#),
        format!(r#"{{ "type": "group", "branches": [ {{ "volume": "loud" }} ], "macros": {eight} }}"#),
        format!(r#"{{ "type": "group", "branches": [ {{ "pan": 2 }} ], "macros": {eight} }}"#),
        format!(r#"{{ "type": "group", "branches": [ {{ "mute": 1 }} ], "macros": {eight} }}"#),
        format!(
            r#"{{ "type": "group", "branches": [ {{ "devices": [ {{ "kind": {{ "type": "warp-drive" }} }} ] }} ], "macros": {eight} }}"#
        ),
        mapped(r#"{ "device": [0, 0], "param": "gain" }"#, "[]"),
        mapped(r#"{ "device": [-1], "param": "gain" }"#, one),
        mapped(r#"{ "device": "0/0", "param": "gain" }"#, one),
        mapped(r#"{ "device": [0, 0] }"#, one),
        mapped(r#"{ "device": [0, 0], "param": "a", "min": 2 }"#, one),
    ] {
        assert!(with(&bad).is_err(), "{bad}");
    }
    assert!(with(&mapped(r#"{ "device": [0, 0], "param": "a" }"#, one)).is_ok());
}

#[test]
fn a_device_a_newer_suhina_knows_is_refused_by_name() {
    let text = r#"{ "format": "suhina", "version": 2, "project": { "channels": [ { "id": 1, "devices": [ { "kind": { "type": "hologram" } } ] } ] } }"#;
    let error = from_json(text).err().map(|e| e.0).unwrap_or_default();
    assert!(error.contains("this project uses a device a newer Suhina knows"), "{error}");
}

#[test]
fn groups_nested_past_the_parsers_limit_are_an_error() {
    // Deeper than serde_json reads at all: the parser refuses it before the reader recurses.
    let mut kind = r#"{ "type": "builtin", "id": "utility" }"#.to_string();
    for _ in 0..200 {
        kind = format!(r#"{{ "type": "group", "branches": [ {{ "devices": [ {{ "kind": {kind} }} ] }} ] }}"#);
    }
    let text = format!(
        r#"{{ "format": "suhina", "version": 2, "project": {{ "channels": [ {{ "id": 1, "devices": [ {{ "kind": {kind} }} ] }} ] }} }}"#
    );
    assert!(from_json(&text).is_err());
}
