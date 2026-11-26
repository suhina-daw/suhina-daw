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
    assert_eq!(slot.commands[0].1["scene"], 2);
    let empty =
        plan(&items, DropTarget::NewTracks { beat: None, scene: Some(0) }, true, "tonal", false, &name).unwrap();
    assert!(empty.commands[0].1.get("track").is_none());
    assert_eq!((empty.commands[0].1["scene"].as_u64(), empty.commands[0].1["warp"].as_str()), (Some(0), Some("tonal")));
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
    // A preset that ships with Suhina, on a track or on a new one.
    let stock = item("stock:fm-synth/warm-fm-bass", ItemKind::Preset);
    let on_track =
        plan(std::slice::from_ref(&stock), DropTarget::Track(TrackId(2)), false, "rhythmic", false, &name).unwrap();
    assert_eq!(on_track.commands, vec![("device.add", json!({ "stock": "fm-synth/warm-fm-bass", "track": 2 }))]);
    let new = plan(&[stock], DropTarget::NewTracks { beat: Some(0.0), scene: None }, false, "rhythmic", false, &name)
        .unwrap();
    assert_eq!(new.commands, vec![("track.add", json!({ "stock": "fm-synth/warm-fm-bass" }))]);
    assert!(
        plan(&[item("/s/dir", ItemKind::Folder)], DropTarget::Track(TrackId(1)), false, "rhythmic", false, &name)
            .is_none()
    );
}

/// A carried file's clip length is what `clip.addFiles` gives it: a sample over its own tempo
/// or the project's, a MIDI file in whole bars, unknown until the browser has read it.
#[test]
fn a_dropped_file_is_as_long_as_the_clip_it_becomes() {
    let project = (120.0, 4.0);
    let at_its_tempo = DroppedFile::new(true, "loop".into(), Some(8.0), Some(90.0), 0.0, project);
    assert_eq!(at_its_tempo.beats, Some(12.0), "8 s at 90 BPM");
    let at_the_projects = DroppedFile::new(true, "hit".into(), Some(1.5), None, 0.0, project);
    assert_eq!(at_the_projects.beats, Some(3.0), "1.5 s at the project's 120 BPM");
    let midi = DroppedFile::new(false, "riff".into(), None, None, 5.0, project);
    assert_eq!(midi.beats, Some(8.0), "5 beats of notes make two whole bars");
    assert_eq!(DroppedFile::new(true, "unread".into(), None, None, 0.0, project).beats, None);
}

/// The ghost clips land where `clip.addFiles` puts the clips: the target's kind on it, end to
/// end, or one each with `separate`; the rest on new tracks below, one per kind or one each.
#[test]
fn carried_files_are_laid_out_as_the_drop_places_them() {
    let file = |audio: bool, name: &str, beats: Option<f64>| DroppedFile { audio, name: name.into(), beats };
    let files =
        [file(true, "a", Some(4.0)), file(true, "b", Some(8.0)), file(false, "m", Some(4.0)), file(true, "c", None)];
    let rows =
        |ghosts: &[GhostClip]| ghosts.iter().map(|g| (g.row, g.start, g.length, g.name.clone())).collect::<Vec<_>>();

    // On an audio track, one track: the samples end to end on it, the MIDI file on a new track.
    assert_eq!(
        rows(&ghost_layout(&files, Some(true), false, 2.0, 4.0)),
        [(0, 2.0, 4.0, "a".into()), (0, 6.0, 8.0, "b".into()), (0, 14.0, 4.0, "c".into()), (1, 2.0, 4.0, "m".into())]
    );
    // One track each: the first sample on it, every other file on a new track of its own.
    assert_eq!(
        rows(&ghost_layout(&files, Some(true), true, 2.0, 4.0)),
        [(0, 2.0, 4.0, "a".into()), (1, 2.0, 8.0, "b".into()), (2, 2.0, 4.0, "c".into()), (3, 2.0, 4.0, "m".into())]
    );
    // Under the tracks: new tracks only, one for the samples and one for the MIDI file.
    assert_eq!(
        rows(&ghost_layout(&files, None, false, 0.0, 4.0)),
        [(1, 0.0, 4.0, "a".into()), (1, 4.0, 8.0, "b".into()), (1, 12.0, 4.0, "c".into()), (2, 0.0, 4.0, "m".into())]
    );
}
