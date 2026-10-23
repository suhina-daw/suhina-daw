// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stems files placed with `clip.addFiles` (`docs/spec/stems-format.md`): a group of a muted
//! master and the stems, named and coloured from the file, with its mastering on the group.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};
use suhina_engine::RenderInputs;

use serde_json::json;
use suhina_model::*;
use suhina_session::{Session, blank_project};

use crate::support::run;

const SR: u32 = 48_000;

/// A copy of a test Stems file in a folder of its own.
fn stems_file(test: &str, name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-stems-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../media/tests/data").join(name);
    let to = dir.join(name);
    std::fs::copy(from, &to).unwrap();
    to
}

fn track<'a>(p: &'a Project, name: &str) -> &'a Track {
    p.tracks.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no track {name}"))
}

fn channel<'a>(p: &'a Project, t: &Track) -> &'a Channel {
    p.channels.iter().find(|c| Some(c.id) == t.channel).unwrap()
}

fn render(session: &Session) -> Vec<f32> {
    suhina_engine::render_offline_from(session.project(), RenderInputs::from_audio(session.audio()), SR, 512, None)
        .unwrap()
}

use suhina_test_support::peak;

#[test]
fn a_stems_file_becomes_a_group_of_a_muted_master_and_its_stems() {
    let path = stems_file("group", "tones-aac.stem.mp4");
    let mut session = Session::new(blank_project());
    let result =
        run(&mut session, "clip.addFiles", json!({ "paths": [path.display().to_string()], "start": 4.0 })).unwrap();
    assert_eq!(result["groups"].as_array().unwrap().len(), 1);
    assert_eq!(result["clips"].as_array().unwrap().len(), 5);
    let p = session.project();
    assert_eq!(p.validate(), Ok(()));

    let group = track(p, "tones-aac");
    let group_channel = channel(p, group);
    assert_eq!(group_channel.role, ChannelRole::Submix);
    let names: Vec<&str> = p.tracks.iter().filter(|t| t.parent == Some(group.id)).map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Master", "Drums", "Bass", "Other", "Vox"]);
    assert!(channel(p, track(p, "Master")).mute);
    assert!(!channel(p, track(p, "Drums")).mute);
    assert_eq!(track(p, "Drums").color, Some(Rgb(0xFD, 0x4A, 0x4A)));
    for name in ["Master", "Drums", "Vox"] {
        assert_eq!(channel(p, track(p, name)).output, Some(group_channel.id));
    }

    // The file's mastering on the group, in order, with its values.
    let devices: Vec<(&DeviceKind, bool)> = group_channel.devices.iter().map(|d| (&d.kind, d.enabled)).collect();
    assert_eq!(
        devices,
        [(&DeviceKind::Builtin("compressor".into()), true), (&DeviceKind::Builtin("limiter".into()), true)]
    );
    let ratio = group_channel.devices[0].params.iter().find(|p| p.id == "ratio").unwrap();
    assert_eq!(ratio.value, 4.0);

    // Each track plays its own audio track of the file, from the same beat.
    for (n, name) in ["Master", "Drums", "Bass", "Other", "Vox"].iter().enumerate() {
        let id = track(p, name).id;
        let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(id)).unwrap();
        let clip = &lane.clips[0];
        assert_eq!(clip.start, 4.0);
        let content = p.content(clip.content).unwrap();
        let LaneData::Audio(region) = &content.lanes[0].data else { panic!("not audio") };
        assert_eq!(region.stream, Some(n as u16));
        assert!(session.audio().files.contains_key(region.pool_key().as_ref()));
    }

    // One undo step.
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..blank_project() });
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn the_stems_play_through_the_group_and_the_master_does_not_sound_twice() {
    let path = stems_file("play", "tones-aac.stem.mp4");
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [path.display().to_string()], "start": 0.0 })).unwrap();
    let stems = render(&session);
    assert!(peak(&stems) > 0.01);

    // Only the master (stems muted): sound too. Everything muted: silence.
    let set = |session: &mut Session, name: &str, mute: bool| {
        let id = track(session.project(), name).id.0;
        run(session, "track.setMute", json!({ "tracks": [id], "mute": mute })).unwrap();
    };
    for name in ["Drums", "Bass", "Other", "Vox"] {
        set(&mut session, name, true);
    }
    assert!(peak(&render(&session)) < 1e-6, "the master is muted");
    set(&mut session, "Master", false);
    assert!(peak(&render(&session)) > 0.01);

    // The mastering runs on the sum: switching it off changes the sound.
    set(&mut session, "Master", true);
    for name in ["Drums", "Bass", "Other", "Vox"] {
        set(&mut session, name, false);
    }
    let group = track(session.project(), "tones-aac").id.0;
    for position in 0..2 {
        run(&mut session, "device.setEnabled", json!({ "track": group, "device": position, "enabled": false }))
            .unwrap();
    }
    let plain = render(&session);
    let common = plain.len().min(stems.len());
    let difference = plain[..common].iter().zip(&stems[..common]).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(difference > 1e-3, "{difference}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_stems_file_dropped_on_a_group_goes_into_it_and_into_a_scene() {
    let path = stems_file("nested", "tones-alac.stem.m4a");
    let mut session = Session::new(blank_project());
    let ids: Vec<u32> = session.project().tracks.iter().take(2).map(|t| t.id.0).collect();
    let outer = run(&mut session, "track.group", json!({ "tracks": ids, "name": "Set" })).unwrap();
    let outer = TrackId(outer["track"].as_u64().unwrap() as u32);
    run(&mut session, "clip.addFiles", json!({ "paths": [path.display().to_string()], "track": outer.0, "scene": 2 }))
        .unwrap();
    let p = session.project();
    assert_eq!(p.validate(), Ok(()));
    let group = track(p, "tones-alac");
    assert_eq!(group.parent, Some(outer));
    assert_eq!(channel(p, group).output, track(p, "Set").channel);
    // Inside the outer group, after its tracks.
    let position = |name: &str| p.tracks.iter().position(|t| t.name == name).unwrap();
    assert!(position("tones-alac") > position("2 MIDI"));
    let vox = track(p, "Vox");
    let column = p.launcher.columns.iter().find(|c| c.track == vox.id).unwrap();
    assert!(column.slots[1].clip.is_some());
    // Base64 metadata names the stems too.
    assert!(p.tracks.iter().any(|t| t.name == "Drums"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_stems_file_without_metadata_has_numbered_stems_and_no_mastering() {
    let path = stems_file("plain", "tones-plain.stem.mp4");
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [path.display().to_string()], "start": 0.0 })).unwrap();
    let p = session.project();
    let group = track(p, "tones-plain");
    assert!(channel(p, group).devices.is_empty());
    let names: Vec<&str> = p.tracks.iter().filter(|t| t.parent == Some(group.id)).map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Master", "Stem 1", "Stem 2", "Stem 3", "Stem 4"]);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_broken_stems_file_is_skipped_and_the_rest_placed() {
    let path = stems_file("broken", "tones-plain.stem.mp4");
    let broken = path.with_file_name("broken.stem.mp4");
    std::fs::write(&broken, b"\0\0\0\x10moovgarbage").unwrap();
    let mut session = Session::new(blank_project());
    let result = run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": [broken.display().to_string(), path.display().to_string()], "start": 0.0 }),
    )
    .unwrap();
    assert_eq!(result["skipped"].as_array().unwrap().len(), 1);
    assert_eq!(result["groups"].as_array().unwrap().len(), 1);
    let err = run(&mut session, "clip.addFiles", json!({ "paths": [broken.display().to_string()], "start": 0.0 }))
        .unwrap_err();
    assert!(err.contains("nothing could be placed"), "{err}");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
