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
    /// A built-in MIDI effect: it goes before a MIDI track's instrument.
    MidiEffect,
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
        matches!(self, Self::Instrument | Self::Effect | Self::AudioEffect | Self::MidiEffect | Self::Preset)
    }
}

/// Where a device carried from a chain or a strip's list started (#688): the panel or row puts
/// its track and index here as the drag starts, and the window takes it from there.
pub const DEVICE_CARRY: &str = "device-carry-start";

/// Says that a drag of the top-level device `index` on `track` started this frame.
pub fn start_device_carry(ctx: &egui::Context, track: suhina_engine::model::TrackId, index: usize) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(DEVICE_CARRY), (track, index)));
}

/// Where the shown chain's top-level devices are this frame: the track, the chain's area and
/// each device's index and panel, left to right. A device carried and let go over it moves to
/// the place under the pointer (`device.move`).
pub const DEVICE_SLOTS: &str = "device-slots";

/// The device being carried, set by the window every frame while it is, so the chain can show
/// where it would go.
pub const DEVICE_CARRYING: &str = "device-carrying";

/// One shown list of a track's top-level devices: the detail area's chain (left to right) or a
/// mixer strip's list (top to bottom), drawn in frame `pass`.
#[derive(Debug, Clone)]
pub struct DeviceSlots {
    pub track: suhina_engine::model::TrackId,
    pub area: egui::Rect,
    pub slots: Vec<(usize, egui::Rect)>,
    pub vertical: bool,
    pub pass: u64,
}

impl DeviceSlots {
    /// Where a device let go at `pointer` goes along this list ([`device_slot_at`]).
    pub fn at(&self, pointer: egui::Pos2) -> Option<usize> {
        device_slot_at(&self.slots, if self.vertical { pointer.y } else { pointer.x }, self.vertical)
    }
}

/// Adds `list` to this frame's device lists (the lists of earlier frames go).
pub fn note_device_slots(ctx: &egui::Context, list: DeviceSlots) {
    ctx.data_mut(|d| {
        let lists = d.get_temp_mut_or_default::<Vec<DeviceSlots>>(egui::Id::new(DEVICE_SLOTS));
        lists.retain(|l| l.pass == list.pass && !(l.track == list.track && l.vertical == list.vertical));
        lists.push(list);
    });
}

/// This frame's or the last frame's device list of `track` under `pointer`, if one is.
pub fn device_slots_under(
    ctx: &egui::Context,
    track: suhina_engine::model::TrackId,
    pointer: egui::Pos2,
) -> Option<DeviceSlots> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<Vec<DeviceSlots>>(egui::Id::new(DEVICE_SLOTS)))?
        .into_iter()
        .find(|l| l.track == track && l.pass + 1 >= pass && l.area.contains(pointer))
}

/// Where in `slots` a device let go at `at` (x along a chain, y down a list) goes: the index of
/// the device it lands before, as `device.move` takes it (the first whose middle is past `at`,
/// or after the last).
pub fn device_slot_at(slots: &[(usize, egui::Rect)], at: f32, vertical: bool) -> Option<usize> {
    let last = slots.last()?;
    let middle = |r: &egui::Rect| if vertical { r.center().y } else { r.center().x };
    Some(slots.iter().find(|(_, r)| at < middle(r)).map_or(last.0 + 1, |(i, _)| *i))
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
    /// A track as a whole: its header.
    Track(TrackId),
    /// The detail area showing a track's devices: a sample dropped there on a MIDI track with no
    /// devices becomes a Sampler playing it; elsewhere it is a clip.
    Chain(TrackId),
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
    /// The files go onto one track each (Ctrl held, or the setting): the new tracks after the
    /// first are shown too, below the tracks.
    pub separate: bool,
}

impl DropProbe {
    /// A device carried from a chain or a strip's list onto another track (#688): it lands on a
    /// track as a dropped device does.
    pub fn for_device(pointer: egui::Pos2) -> Self {
        Self { pointer, files: false, devices: true, clip: false, target: None, area: None, separate: false }
    }

    pub fn new(pointer: egui::Pos2, items: &[BrowserItem]) -> Self {
        Self {
            pointer,
            files: items.iter().any(|i| i.kind.is_file_clip()),
            devices: items.iter().any(|i| i.kind.is_device()),
            clip: false,
            target: None,
            area: None,
            separate: false,
        }
    }

    /// A probe for a clip carried between the views.
    pub fn for_clip(pointer: egui::Pos2) -> Self {
        Self { pointer, files: true, devices: false, clip: true, target: None, area: None, separate: false }
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

/// What a drop of `files` files on a track says: onto that track, or with `separate` (one track
/// each) the first there and the others on as many new tracks after it.
fn spread(count: &str, files: usize, separate: bool, track: &str) -> String {
    match files {
        2.. if separate => {
            let more = files - 1;
            format!("Add {count}, one track each: {track} and {more} new track{}", if more == 1 { "" } else { "s" })
        }
        _ => format!("Add {count} to {track}"),
    }
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
                label.push(spread(&count, files.len(), separate, &track_name(track)));
            }
            DropTarget::Slot { track, scene } => {
                params["track"] = json!(track.0);
                params["scene"] = json!(scene + 1);
                label.push(spread(&count, files.len(), separate, &track_name(track)));
            }
            DropTarget::Track(_) | DropTarget::Chain(_) | DropTarget::Sampler { .. } => return None,
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
            | DropTarget::Chain(track)
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
#[path = "drop_tests.rs"]
mod tests;
