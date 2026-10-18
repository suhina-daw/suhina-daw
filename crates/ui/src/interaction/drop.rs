// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dragging from the browser, or files from the system, into the project
//! (`docs/spec/browser.md` › Dragging into the project): what is dragged, where it can land,
//! and the commands a drop runs. The views say which target is under the pointer; this module
//! turns a drop on it into `clip.addFiles`, `track.add` and `device.add`/`device.addPlugin`, so
//! a drop is the same commands agents use.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_engine::model::TrackId;
use suhina_library::Item;
use suhina_media::info::FileKind;

/// What a dragged browser item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Folder,
    Sample,
    Midi,
    /// A preset file (its first voice) or one voice of a bank.
    Preset,
    Project,
    Instrument,
    Effect,
    AudioEffect,
    /// A heading in the tree (Instruments, Plug-ins, a vendor).
    Heading,
}

impl ItemKind {
    /// Whether it becomes a clip.
    pub fn is_file_clip(self) -> bool {
        matches!(self, Self::Sample | Self::Midi)
    }

    /// Whether it becomes a device on a track.
    pub fn is_device(self) -> bool {
        matches!(self, Self::Instrument | Self::Effect | Self::AudioEffect | Self::Preset)
    }
}

/// An item in the browser's results, as it is dragged.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserItem {
    /// The item's key (`docs/spec/browser.md` › Terms).
    pub key: String,
    pub name: String,
    pub kind: ItemKind,
    /// A plug-in rather than a built-in effect or instrument.
    pub plugin: bool,
}

impl BrowserItem {
    pub fn item(&self) -> Option<Item> {
        Item::parse(&self.key)
    }

    /// The file it is, for a file item.
    pub fn path(&self) -> Option<PathBuf> {
        match self.item()? {
            Item::File(path) | Item::Voice(path, _) => Some(path),
            _ => None,
        }
    }
}

/// A file dragged from the system as the browser item it would be, when it becomes clips: a
/// sample, a Stems file or a MIDI file. Projects, themes and folders have drops of their own.
pub fn system_file(path: &Path) -> Option<BrowserItem> {
    let kind = match suhina_media::info::kind_of(path)? {
        FileKind::Sample | FileKind::Stems => ItemKind::Sample,
        FileKind::Midi => ItemKind::Midi,
        FileKind::Preset | FileKind::Project | FileKind::Effect => return None,
    };
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(BrowserItem { key: Item::File(path.to_path_buf()).key(), name, kind, plugin: false })
}

/// Where a drag from the browser can land, as the view under the pointer says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DropTarget {
    /// Replace a sampler source, without creating a clip or a track.
    Sampler { track: TrackId, device: usize },
    /// An arrangement lane, at a beat (already snapped).
    Lane { track: TrackId, beat: f64 },
    /// A clip slot (scene from 0).
    Slot { track: TrackId, scene: usize },
    /// A track as a whole: its header, or the detail area showing its devices.
    Track(TrackId),
    /// The empty space after the tracks: new tracks, at a beat (Arrangement) or from a scene
    /// (Session).
    NewTracks { beat: Option<f64>, scene: Option<usize> },
}

/// What the views know about a drag in progress, and the target they found under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct DropProbe {
    pub pointer: egui::Pos2,
    /// The drag holds samples or MIDI files.
    pub files: bool,
    /// The drag holds devices, presets or plug-ins.
    pub devices: bool,
    /// The drag is a clip carried from a slot or the arrangement: it lands only in a slot or on
    /// a lane (`docs/spec/ui-session-view.md` › Between the views).
    pub clip: bool,
    pub target: Option<DropTarget>,
    /// The target's area, outlined while dragging.
    pub area: Option<egui::Rect>,
}

impl DropProbe {
    pub fn new(pointer: egui::Pos2, items: &[BrowserItem]) -> Self {
        Self {
            pointer,
            files: items.iter().any(|i| i.kind.is_file_clip()),
            devices: items.iter().any(|i| i.kind.is_device()),
            clip: false,
            target: None,
            area: None,
        }
    }

    /// A probe for a clip carried between the views.
    pub fn for_clip(pointer: egui::Pos2) -> Self {
        Self { pointer, files: true, devices: false, clip: true, target: None, area: None }
    }

    /// Takes `target` when the pointer is in `area` and nothing took it yet.
    pub fn offer(&mut self, area: egui::Rect, target: DropTarget) -> bool {
        if self.target.is_none() && area.contains(self.pointer) {
            self.target = Some(target);
            self.area = Some(area);
            return true;
        }
        false
    }
}

/// What the window does for a drop, besides the commands.
#[derive(Debug, Clone, PartialEq)]
pub struct DropPlan {
    /// Commands to run in order; `clip.addFiles` is one of them when files were dropped.
    pub commands: Vec<(&'static str, Value)>,
    /// The samples that will be decoded before the commands run.
    pub samples: Vec<PathBuf>,
    /// What the label beside the pointer says.
    pub label: String,
}

/// The commands a drop of `items` on `target` runs. `separate` is Ctrl held, already flipped
/// by the setting; `warp` the stretch mode for samples with a tempo; `stack` whether
/// arrangement clips may stack. `None` when nothing can land there.
pub fn plan(
    items: &[BrowserItem],
    target: DropTarget,
    separate: bool,
    warp: &str,
    stack: bool,
    track_name: &dyn Fn(TrackId) -> String,
) -> Option<DropPlan> {
    if let DropTarget::Sampler { track, device } = target {
        let [sample] = items else { return None };
        if sample.kind != ItemKind::Sample {
            return None;
        }
        let path = sample.path()?;
        return Some(DropPlan {
            commands: vec![(
                "device.loadSample",
                json!({"track": track.0, "device": device, "path": path.display().to_string()}),
            )],
            samples: vec![path],
            label: "Replace sampler source".into(),
        });
    }
    let files: Vec<&BrowserItem> = items.iter().filter(|i| i.kind.is_file_clip()).collect();
    let devices: Vec<&BrowserItem> = items.iter().filter(|i| i.kind.is_device()).collect();
    if files.is_empty() && devices.is_empty() {
        return None;
    }
    let mut commands = Vec::new();
    let mut label = Vec::new();
    let samples: Vec<PathBuf> = files.iter().filter(|i| i.kind == ItemKind::Sample).filter_map(|i| i.path()).collect();
    if !files.is_empty() {
        let paths: Vec<String> = files.iter().filter_map(|i| i.path()).map(|p| p.display().to_string()).collect();
        let mut params = json!({ "paths": paths, "separate": separate, "warp": warp });
        let count = if files.len() == 1 { "a clip".to_string() } else { format!("{} clips", files.len()) };
        match target {
            DropTarget::Lane { track, beat } => {
                params["track"] = json!(track.0);
                params["start"] = json!(beat);
                params["stack"] = json!(stack);
                label.push(format!("Add {count} to {}", track_name(track)));
            }
            DropTarget::Slot { track, scene } => {
                params["track"] = json!(track.0);
                params["scene"] = json!(scene + 1);
                label.push(format!("Add {count} to {}", track_name(track)));
            }
            DropTarget::Track(_) | DropTarget::Sampler { .. } => return None,
            DropTarget::NewTracks { beat, scene } => {
                match (beat, scene) {
                    (_, Some(scene)) => params["scene"] = json!(scene + 1),
                    (beat, None) => params["start"] = json!(beat.unwrap_or(0.0)),
                }
                let kinds =
                    [files.iter().any(|f| f.kind == ItemKind::Sample), files.iter().any(|f| f.kind == ItemKind::Midi)];
                label.push(match (separate && files.len() > 1, kinds) {
                    (true, _) => format!("{} new tracks", files.len()),
                    (false, [true, true]) => "New audio and MIDI tracks".into(),
                    (false, [false, true]) => "New MIDI track".into(),
                    _ => "New audio track".into(),
                });
            }
        }
        commands.push(("clip.addFiles", params));
    }
    for device in devices {
        let item = device.item()?;
        let mut params = json!({});
        let id: &'static str = match (&item, target) {
            (Item::Voice(path, voice), _) => {
                params["preset"] = json!(path.display().to_string());
                params["voice"] = json!(voice);
                "device.add"
            }
            (Item::File(path), _) => {
                if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("sd")) {
                    params["path"] = json!(path.display().to_string());
                    "device.addSd"
                } else {
                    params["preset"] = json!(path.display().to_string());
                    "device.add"
                }
            }
            (Item::Device(id), _) => {
                params["id"] = json!(id);
                "device.add"
            }
            (Item::Plugin(spec), _) => {
                params["plugin"] = json!(spec);
                "device.addPlugin"
            }
        };
        match target {
            DropTarget::Lane { track, .. }
            | DropTarget::Slot { track, .. }
            | DropTarget::Track(track)
            | DropTarget::Sampler { track, .. } => {
                params["track"] = json!(track.0);
                commands.push((id, params));
                label.push(format!("Add {} to {}", device.name, track_name(track)));
            }
            DropTarget::NewTracks { .. } => {
                // A new track with the device: `track.add` takes the same names.
                let mut add = json!({});
                if let Some(path) = params.get("path") {
                    add["sd"] = path.clone();
                } else if let Some(preset) = params.get("preset") {
                    add["preset"] = preset.clone();
                    if let Some(voice) = params.get("voice") {
                        add["voice"] = voice.clone();
                    }
                } else if let Some(id) = params.get("id").or_else(|| params.get("plugin")) {
                    add["device"] = id.clone();
                }
                commands.push(("track.add", add));
                let kind =
                    if matches!(device.kind, ItemKind::Effect | ItemKind::AudioEffect) { "audio" } else { "MIDI" };
                label.push(format!("New {kind} track with {}", device.name));
            }
        }
    }
    Some(DropPlan { commands, samples, label: label.join(", ") })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let plan =
            plan(&items, DropTarget::Lane { track: TrackId(3), beat: 8.0 }, false, "rhythmic", true, &name).unwrap();
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
        let slot =
            plan(&items, DropTarget::Slot { track: TrackId(1), scene: 2 }, false, "rhythmic", false, &name).unwrap();
        assert_eq!(slot.commands[0].1["scene"], 3);
        let empty =
            plan(&items, DropTarget::NewTracks { beat: None, scene: Some(0) }, true, "tonal", false, &name).unwrap();
        assert!(empty.commands[0].1.get("track").is_none());
        assert_eq!(
            (empty.commands[0].1["scene"].as_u64(), empty.commands[0].1["warp"].as_str()),
            (Some(1), Some("tonal"))
        );
        assert_eq!(empty.label, "New audio track");
        assert!(
            plan(&items, DropTarget::Track(TrackId(1)), false, "rhythmic", false, &name).is_none(),
            "a file needs a place"
        );
    }

    #[test]
    fn system_files_that_become_clips_are_items_and_the_rest_are_not() {
        let wav = system_file(Path::new("/s/Kick.WAV")).unwrap();
        assert_eq!(
            (wav.kind, wav.name.as_str(), wav.path()),
            (ItemKind::Sample, "Kick.WAV", Some("/s/Kick.WAV".into()))
        );
        assert_eq!(system_file(Path::new("/s/groove.mid")).map(|i| i.kind), Some(ItemKind::Midi));
        assert_eq!(system_file(Path::new("/s/song.stem.mp4")).map(|i| i.kind), Some(ItemKind::Sample));
        for other in [
            "/s/effect.sd",
            "/s/song.suhina",
            "/s/set.als",
            "/s/theme.json",
            "/s/bank.syx",
            "/s/notes.txt",
            "/s/noext",
            "",
        ] {
            assert!(system_file(Path::new(other)).is_none(), "{other}");
        }
        // The same plan as a browser drag of that file.
        let plan =
            plan(&[wav], DropTarget::Slot { track: TrackId(1), scene: 0 }, false, "rhythmic", false, &name).unwrap();
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
        let new =
            plan(&[synth], DropTarget::NewTracks { beat: Some(0.0), scene: None }, false, "rhythmic", false, &name)
                .unwrap();
        assert_eq!(new.commands, vec![("track.add", json!({ "device": "fm-synth" }))]);
        assert!(
            plan(&[item("/s/dir", ItemKind::Folder)], DropTarget::Track(TrackId(1)), false, "rhythmic", false, &name)
                .is_none()
        );
    }
}
