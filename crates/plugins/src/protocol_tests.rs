// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn ara_document() -> AraDocument {
    AraDocument {
        tempo: vec![(0.0, 0.0), (0.5, 1.0), (2.5, 4.0)],
        bars: vec![(4, 4, 0.0), (3, 4, 8.0)],
        name: "Song".into(),
        document: "4242-1".into(),
        sequence: "Vocal".into(),
        order: 3,
        tempo_bpm: 120.0,
        sources: vec![AraSource { id: "content-7".into(), name: "take.wav".into(), address: "/dev/shm/a".into() }],
        regions: vec![AraRegion {
            id: "clip-12".into(),
            name: "take".into(),
            source: 0,
            modification: "content-7".into(),
            start: 2.0,
            duration: 4.0,
            offset: 0.5,
            clone_of: Some("content-3".into()),
        }],
    }
}

#[test]
fn a_malformed_ara_document_refuses_the_load() {
    let load = |document: Value| {
        let line = json!({ "op": "load", "instance": 1, "plugin": "vst3:/T.vst3", "link": "/dev/shm/x",
                "sample_rate": 48000.0, "ara": document })
        .to_string();
        Message::decode(&line)
    };
    let good = ara_document().to_json();
    assert!(load(good.clone()).is_some());
    let mut bad_source = good.clone();
    bad_source["regions"][0]["source"] = json!(1);
    assert!(load(bad_source).is_none(), "a region of a source not in the list");
    let mut backwards = good.clone();
    backwards["regions"][0]["duration"] = json!(-1.0);
    assert!(load(backwards).is_none());
    let mut endless = good.clone();
    endless["tempo_bpm"] = json!(0.0);
    assert!(load(endless).is_none());
    let mut long = good.clone();
    long["name"] = json!("x".repeat(MAX_ARA_TEXT + 1));
    assert!(load(long).is_none());
    let mut longest_bar = good.clone();
    longest_bar["bars"][1][0] = json!(99);
    assert!(load(longest_bar).is_some(), "a 99/4 bar, which a project can hold");
    let mut too_long = good.clone();
    too_long["bars"][1][0] = json!(100);
    assert!(load(too_long).is_none());
    let mut many = good;
    many["sources"] = json!(vec![json!({ "id": "a", "name": "a", "address": "a" }); MAX_ARA_OBJECTS + 1]);
    assert!(load(many).is_none());
}

fn gain() -> ParamInfo {
    ParamInfo {
        id: 0,
        name: "Gain".into(),
        group: "Main".into(),
        min: 0.0,
        max: 1.0,
        default: 0.5,
        value: 0.25,
        text: "-12.0 dB".into(),
        steps: 0,
        automatable: true,
        read_only: false,
        hidden: false,
    }
}

#[test]
fn every_message_round_trips() {
    assert_eq!(VERSION, 13);
    let messages = [
        Message::Hello { version: VERSION },
        Message::TransportKey,
        Message::AraPlayback(AraPlayback::Start),
        Message::AraPlayback(AraPlayback::Stop),
        Message::AraPlayback(AraPlayback::Position(12.5)),
        Message::AraNotes {
            instance: 4,
            region: "clip-12".into(),
            notes: vec![AraNote { start: 0.5, length: 0.25, pitch: 62, volume: 0.75 }],
            truncated: false,
        },
        Message::AraNotes { instance: 4, region: "clip-13".into(), notes: Vec::new(), truncated: true },
        Message::Load {
            instance: 3,
            plugin: "test:gain".into(),
            link: "/dev/shm/x".into(),
            sample_rate: 44_100.0,
            ara: None,
        },
        Message::Load {
            instance: 4,
            plugin: "vst3:/Tune.vst3#ABCDEF0123456789ABCDEF0123456789".into(),
            link: "/dev/shm/y".into(),
            sample_rate: 48_000.0,
            ara: Some(ara_document()),
        },
        Message::Loaded { instance: u32::MAX, latency: 64, edits_clips: false },
        Message::Loaded { instance: 1, latency: 0, edits_clips: true },
        Message::Unload { instance: 0 },
        Message::Unloaded { instance: 7 },
        Message::Ping { id: u64::MAX },
        Message::Pong { id: 7 },
        Message::GetState { instance: 1 },
        Message::SetState { instance: 1, data: vec![0, 1, 0xab, 0xff], controller: None },
        Message::SetState { instance: 2, data: vec![1], controller: Some(vec![2, 3]) },
        Message::State { instance: 1, data: Vec::new(), controller: None },
        Message::State { instance: 9, data: vec![4], controller: Some(Vec::new()) },
        Message::GetParams { instance: 4 },
        Message::Params {
            instance: 4,
            params: vec![gain(), ParamInfo { id: u32::MAX, steps: 1, hidden: true, ..gain() }],
        },
        Message::Params { instance: 4, params: Vec::new() },
        Message::SetParam { instance: 5, id: 7, value: -0.5 },
        Message::Param { instance: 5, param: gain() },
        Message::OpenEditor { instance: 6, owner: None, embedded: false },
        Message::OpenEditor { instance: 6, owner: Some(0x0012_0F3A), embedded: true },
        Message::EditorOpened { instance: 6, width: 800, height: 600 },
        Message::PlaceEditor {
            instance: 6,
            placement: Placement { x: -40, y: 620, clip_x: 0, clip_y: 600, clip_width: 1280, clip_height: 0 },
        },
        Message::AraSelect { instance: 6, region: Some("clip-7".into()) },
        Message::AraExport { instance: 6, modification: "content-3".into(), source: "C:\\a\\take.wav".into() },
        Message::AraExported { instance: 6, archive_id: "com.example.edits".into(), data: vec![1, 2, 0xff] },
        Message::AraImport {
            instance: 7,
            archive_id: "com.example.edits".into(),
            data: vec![1, 2, 0xff],
            from: "content-3".into(),
            to: "content-9".into(),
            source: "C:\\a\\take.wav".into(),
        },
        Message::AraImported { instance: 7 },
        Message::AraSelect { instance: 6, region: None },
        Message::AraUpdate { instance: 6, document: ara_document() },
        Message::AraUpdated { instance: 6 },
        Message::FitEditor { instance: 6, width: 900, height: 340 },
        Message::EditorResized { instance: 6, width: 1, height: 16_384 },
        Message::LatencyChanged { instance: 6, latency: 0 },
        Message::LatencyChanged { instance: u32::MAX, latency: u32::MAX },
        Message::Memory { bytes: 0 },
        Message::Memory { bytes: u64::MAX },
        Message::CloseEditor { instance: 6 },
        Message::EditorClosed { instance: 6 },
        Message::Error { instance: None, message: "no such plug-in: \"x\"\n".into() },
        Message::Error { instance: Some(8), message: "the plug-in has no editor".into() },
        Message::Quit,
    ];
    for message in messages {
        let line = message.encode();
        assert!(!line.contains('\n'), "{line}");
        assert_eq!(Message::decode(&line), Some(message), "{line}");
    }
}

#[test]
fn messages_name_their_instance() {
    assert_eq!(Message::Unload { instance: 4 }.instance(), Some(4));
    assert_eq!(Message::EditorClosed { instance: 2 }.instance(), Some(2));
    assert_eq!(Message::LatencyChanged { instance: 3, latency: 64 }.instance(), Some(3));
    assert_eq!(Message::Error { instance: Some(1), message: String::new() }.instance(), Some(1));
    assert_eq!(Message::Error { instance: None, message: String::new() }.instance(), None);
    assert_eq!(Message::Ping { id: 1 }.instance(), None);
    assert_eq!(Message::Quit.instance(), None);
}

#[test]
fn malformed_lines_are_refused() {
    for line in [
        "",
        "{",
        "[]",
        r#"{"op":"nope"}"#,
        r#"{"op":"ping"}"#,
        r#"{"op":"ping","id":-1}"#,
        r#"{"op":"loaded","instance":1,"latency":5000000000}"#,
        r#"{"op":"loaded","latency":5}"#,
        r#"{"op":"state","instance":1,"data":"abc"}"#,
        r#"{"op":"state","instance":1,"data":"zz"}"#,
        r#"{"op":"state","instance":1,"data":"","controller":5}"#,
        r#"{"op":"state","data":""}"#,
        r#"{"op":"set_state","instance":1,"data":"","controller":"z"}"#,
        r#"{"op":"load","instance":1,"plugin":"x","link":"y","sample_rate":0}"#,
        r#"{"op":"load","instance":1,"plugin":"x","link":"y"}"#,
        r#"{"op":"load","plugin":"x","link":"y","sample_rate":48000}"#,
        r#"{"op":"load","instance":-1,"plugin":"x","link":"y","sample_rate":48000}"#,
        r#"{"op":"load","instance":5000000000,"plugin":"x","link":"y","sample_rate":48000}"#,
        r#"{"op":42}"#,
        r#"{"op":"set_param","instance":1,"id":1}"#,
        r#"{"op":"set_param","instance":1,"id":-1,"value":0.5}"#,
        r#"{"op":"set_param","id":1,"value":0.5}"#,
        r#"{"op":"params","instance":1,"params":[{"id":1}]}"#,
        r#"{"op":"param","instance":1}"#,
        r#"{"op":"editor_opened","instance":1,"width":800}"#,
        r#"{"op":"get_state"}"#,
        r#"{"op":"get_params"}"#,
        r#"{"op":"open_editor"}"#,
        r#"{"op":"open_editor","instance":1}"#,
        r#"{"op":"open_editor","instance":1,"embedded":"yes"}"#,
        r#"{"op":"place_editor","instance":1}"#,
        r#"{"op":"place_editor","instance":1,"placement":{"x":0,"y":0,"clip_x":0,"clip_y":0,"clip_width":-1,"clip_height":0}}"#,
        r#"{"op":"place_editor","instance":1,"placement":{"x":3000000000,"y":0,"clip_x":0,"clip_y":0,"clip_width":1,"clip_height":1}}"#,
        r#"{"op":"editor_resized","instance":1,"height":2}"#,
        r#"{"op":"ara_select","region":"clip-1"}"#,
        r#"{"op":"ara_update","instance":1}"#,
        r#"{"op":"ara_update","instance":1,"document":{}}"#,
        r#"{"op":"ara_updated"}"#,
        r#"{"op":"fit_editor","instance":1,"width":900}"#,
        r#"{"op":"ara_select","instance":1,"region":7}"#,
        r#"{"op":"close_editor","instance":"1"}"#,
        r#"{"op":"latency_changed","instance":1}"#,
        r#"{"op":"latency_changed","latency":64}"#,
        r#"{"op":"latency_changed","instance":1,"latency":-1}"#,
        r#"{"op":"latency_changed","instance":1,"latency":5000000000}"#,
        r#"{"op":"editor_closed"}"#,
        r#"{"op":"unload"}"#,
        r#"{"op":"unloaded","instance":1.5}"#,
        r#"{"op":"error","instance":"x","message":"m"}"#,
        r#"{"op":"error","instance":-3,"message":"m"}"#,
        r#"{"op":"error","instance":1}"#,
    ] {
        assert_eq!(Message::decode(line), None, "{line}");
    }
}
