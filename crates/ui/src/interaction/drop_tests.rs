// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_drop_of_files_one_track_each_says_how_many_new_tracks() {
    assert_eq!(spread("3 clips", 3, true, "Bass"), "Add 3 clips, one track each: Bass and 2 new tracks");
    assert_eq!(spread("2 clips", 2, true, "Bass"), "Add 2 clips, one track each: Bass and 1 new track");
    assert_eq!(spread("3 clips", 3, false, "Bass"), "Add 3 clips to Bass");
    assert_eq!(spread("a clip", 1, true, "Bass"), "Add a clip to Bass");
}

#[test]
fn a_device_let_go_along_its_chain_lands_before_the_panel_whose_middle_is_right_of_it() {
    let panel = |left: f32| egui::Rect::from_min_size(egui::pos2(left, 0.0), egui::vec2(100.0, 80.0));
    let slots = [(0, panel(0.0)), (1, panel(101.0)), (2, panel(202.0))];
    assert_eq!(device_slot_at(&slots, 10.0, false), Some(0));
    assert_eq!(device_slot_at(&slots, 60.0, false), Some(1));
    assert_eq!(device_slot_at(&slots, 240.0, false), Some(2));
    assert_eq!(device_slot_at(&slots, 260.0, false), Some(3), "after the last");
    assert_eq!(device_slot_at(&[], 10.0, false), None);
    // Down a strip's list, by height.
    let row = |top: f32| egui::Rect::from_min_size(egui::pos2(0.0, top), egui::vec2(100.0, 20.0));
    let rows = [(0, row(0.0)), (1, row(20.0)), (2, row(40.0))];
    assert_eq!(device_slot_at(&rows, 25.0, true), Some(1));
    assert_eq!(device_slot_at(&rows, 55.0, true), Some(3));
}

fn item(key: &str, kind: ItemKind) -> BrowserItem {
    BrowserItem { key: key.into(), name: key.rsplit(['/', ':']).next().unwrap_or(key).into(), kind, plugin: false }
}

fn name(t: TrackId) -> String {
    format!("T{}", t.0)
}

#[test]
fn files_on_a_lane_become_one_add_files_command() {
    let items =
        [item("/s/a.wav", ItemKind::Sample), item("/s/b.mid", ItemKind::Midi), item("/s/dir", ItemKind::Folder)];
    let plan = plan(&items, DropTarget::Lane { track: TrackId(3), beat: 8.0 }, false, "rhythmic", true, &name).unwrap();
    assert_eq!(plan.commands.len(), 1);
    let (id, params) = &plan.commands[0];
    assert_eq!(*id, "clip.addFiles");
    assert_eq!(params["paths"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        (params["track"].as_u64(), params["start"].as_f64(), params["stack"].as_bool()),
        (Some(3), Some(8.0), Some(true))
    );
    assert_eq!(plan.samples.len(), 1);
    assert_eq!(plan.label, "Add 2 clips to T3");
}

#[test]
fn sampler_drop_replaces_source_without_creating_clips() {
    let target = DropTarget::Sampler { track: TrackId(3), device: 0 };
    let items = [item("/s/a.wav", ItemKind::Sample)];
    let result = plan(&items, target, false, "rhythmic", false, &name).unwrap();
    assert_eq!(result.commands, vec![("device.loadSample", json!({"track":3, "device":0, "path":"/s/a.wav"}))]);
    assert!(plan(&[item("/s/a.mid", ItemKind::Midi)], target, false, "rhythmic", false, &name).is_none());
    assert!(plan(&[items[0].clone(), items[0].clone()], target, false, "rhythmic", false, &name).is_none());
}

#[test]
fn slots_and_empty_space_take_scenes_and_new_tracks() {
    let items = [item("/s/a.wav", ItemKind::Sample)];
    let slot = plan(&items, DropTarget::Slot { track: TrackId(1), scene: 2 }, false, "rhythmic", false, &name).unwrap();
    assert_eq!(slot.commands[0].1["scene"], 3);
    let empty =
        plan(&items, DropTarget::NewTracks { beat: None, scene: Some(0) }, true, "tonal", false, &name).unwrap();
    assert!(empty.commands[0].1.get("track").is_none());
    assert_eq!((empty.commands[0].1["scene"].as_u64(), empty.commands[0].1["warp"].as_str()), (Some(1), Some("tonal")));
    assert_eq!(empty.label, "New audio track");
    assert!(
        plan(&items, DropTarget::Track(TrackId(1)), false, "rhythmic", false, &name).is_none(),
        "a file needs a place"
    );
}

#[test]
fn system_files_that_become_clips_are_items_and_the_rest_are_not() {
    let wav = system_file(Path::new("/s/Kick.WAV")).unwrap();
    assert_eq!((wav.kind, wav.name.as_str(), wav.path()), (ItemKind::Sample, "Kick.WAV", Some("/s/Kick.WAV".into())));
    assert_eq!(system_file(Path::new("/s/groove.mid")).map(|i| i.kind), Some(ItemKind::Midi));
    assert_eq!(system_file(Path::new("/s/song.stem.mp4")).map(|i| i.kind), Some(ItemKind::Sample));
    for other in
        ["/s/effect.sd", "/s/song.suhina", "/s/set.als", "/s/theme.json", "/s/bank.syx", "/s/notes.txt", "/s/noext", ""]
    {
        assert!(system_file(Path::new(other)).is_none(), "{other}");
    }
    // The same plan as a browser drag of that file.
    let plan = plan(&[wav], DropTarget::Slot { track: TrackId(1), scene: 0 }, false, "rhythmic", false, &name).unwrap();
    assert_eq!(plan.commands[0].1["paths"], json!(["/s/Kick.WAV"]));
}

#[test]
fn devices_go_on_the_track_or_make_one() {
    let synth = item("device:fm-synth", ItemKind::Instrument);
    let mut plug = item("plugin:clap:/x.clap#id", ItemKind::Effect);
    plug.plugin = true;
    let bank = item("/p/bank.syx#5", ItemKind::Preset);
    let on_track = plan(
        &[synth.clone(), plug.clone(), bank.clone()],
        DropTarget::Track(TrackId(2)),
        false,
        "rhythmic",
        false,
        &name,
    )
    .unwrap();
    let ids: Vec<&str> = on_track.commands.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, ["device.add", "device.addPlugin", "device.add"]);
    assert_eq!(on_track.commands[2].1["voice"], 5);
    let sd = item("C:/effects/gain.sd", ItemKind::AudioEffect);
    let sd_plan = plan(&[sd], DropTarget::Track(TrackId(2)), false, "rhythmic", false, &name).unwrap();
    assert_eq!(sd_plan.commands, vec![("device.addSd", json!({ "path": "C:/effects/gain.sd", "track": 2 }))]);
    let new = plan(&[synth], DropTarget::NewTracks { beat: Some(0.0), scene: None }, false, "rhythmic", false, &name)
        .unwrap();
    assert_eq!(new.commands, vec![("track.add", json!({ "device": "fm-synth" }))]);
    // A MIDI effect is a device like the others: the session puts it before the instrument.
    let pitch = item("device:pitch", ItemKind::MidiEffect);
    let on_track =
        plan(std::slice::from_ref(&pitch), DropTarget::Track(TrackId(2)), false, "rhythmic", false, &name).unwrap();
    assert_eq!(on_track.commands, vec![("device.add", json!({ "id": "pitch", "track": 2 }))]);
    let new = plan(&[pitch], DropTarget::NewTracks { beat: Some(0.0), scene: None }, false, "rhythmic", false, &name)
        .unwrap();
    assert_eq!(new.commands, vec![("track.add", json!({ "device": "pitch" }))]);
    assert!(
        plan(&[item("/s/dir", ItemKind::Folder)], DropTarget::Track(TrackId(1)), false, "rhythmic", false, &name)
            .is_none()
    );
}
