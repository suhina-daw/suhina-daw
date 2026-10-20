// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Context menus the views share. Every entry is a command, the same the CLI and MCP run.

use egui::{Id, Key, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{
    AudioInput, ClipId, InputChannels, MidiInput, MidiPort, Monitor, Rgb, TimeSignature, TrackId,
};

use crate::interaction::colour_picker;
use crate::kit::theme;
use crate::kit::widgets::Action;
use crate::settings::keymap::shown;

/// What a track's menu needs to know.
pub struct TrackMenu<'a> {
    pub track: TrackId,
    pub name: &'a str,
    /// The colour the track shows, outlined among the swatches.
    pub colour: Rgb,
    pub is_group: bool,
    /// A return track: new tracks from its menu go after the regular tracks.
    pub is_return: bool,
    /// The tracks selected in the view; grouping takes them when this track is one of them.
    pub selection: &'a [TrackId],
    /// The groups the track can move into, with their names.
    pub into: &'a [(TrackId, String)],
    /// Takes the track out of its group, one level up; `None` when it is in none.
    pub out: Option<Action>,
    /// For a track that plays notes: what it hears while armed, and the MIDI ports there are.
    pub midi: Option<(&'a MidiInput, &'a [String])>,
    /// For a track that plays audio: its audio input, and the channels the input device has
    /// (0 while none is open).
    pub audio: Option<(&'a AudioInput, u16)>,
}

/// How a track's audio input reads: "No input", "In 3" or "In 1/2", with its monitoring.
pub fn audio_input_text(input: &AudioInput) -> String {
    let channels = match input.channels {
        InputChannels::None => return "No input".to_string(),
        InputChannels::Mono(c) => format!("In {c}"),
        InputChannels::Stereo(c) => format!("In {c}/{}", c.saturating_add(1)),
    };
    let monitor = match input.monitor {
        Monitor::Auto => "Auto",
        Monitor::In => "Monitor in",
        Monitor::Off => "Monitor off",
    };
    format!("{channels} · {monitor}")
}

/// The picks of a track's audio input (ADR 0017): none, each channel of the input device, each
/// neighbouring pair. A pick runs `track.setAudioInput`; with no device open, the first eight
/// appear, so a project's choices are setable before an interface is there. The list is what
/// the track's menu and the strip's dropdown both offer.
pub fn audio_input_picks(input: &AudioInput, channels: u16) -> Vec<(String, serde_json::Value)> {
    let mut picks = vec![("No input".to_string(), json!({ "input": "none" }))];
    let channels = if channels == 0 { 8 } else { channels };
    let chosen = match input.channels {
        InputChannels::None => 0,
        InputChannels::Mono(c) => c,
        InputChannels::Stereo(c) => c.saturating_add(1),
    };
    let shown = channels.max(chosen);
    for c in 1..=shown {
        picks.push((format!("In {c}"), json!({ "input": c.to_string() })));
    }
    for c in (1..shown).step_by(2) {
        let pair = format!("{c}/{}", c + 1);
        picks.push((format!("In {pair}"), json!({ "input": pair })));
    }
    picks
}

/// The three monitor choices, as the track's menu lists them (Auto first, reading like a
/// sentence about what is heard) and the strip's three buttons carry them.
pub fn audio_monitor_picks() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("Monitor auto (while armed)", json!({ "monitor": "auto" })),
        ("Monitor in", json!({ "monitor": "in" })),
        ("Monitor off", json!({ "monitor": "off" })),
    ]
}

/// Whether the params say what `input` listens to, so the menu and the strip tick their picks.
pub fn audio_is(input: &AudioInput, params: &serde_json::Value) -> bool {
    if let Some(monitor) = params.get("monitor").and_then(serde_json::Value::as_str) {
        return input.monitor
            == match monitor {
                "in" => Monitor::In,
                "off" => Monitor::Off,
                _ => Monitor::Auto,
            };
    }
    let text = params.get("input").and_then(serde_json::Value::as_str).unwrap_or("");
    match (text, input.channels) {
        ("none", InputChannels::None) => true,
        ("", _) => false,
        (text, InputChannels::Mono(c)) => text == c.to_string(),
        (text, InputChannels::Stereo(c)) => *text == format!("{c}/{}", c.saturating_add(1)),
        (_, InputChannels::None) => false,
    }
}

/// The choices of a track's audio input (ADR 0017): none, each channel of the input device,
/// each neighbouring pair; then its monitoring. A pick runs `track.setAudioInput`.
pub fn audio_input_choices(ui: &mut Ui, track: TrackId, input: &AudioInput, channels: u16, actions: &mut Vec<Action>) {
    let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
    let mut pick = |ui: &mut Ui, label: &str, on: bool, params: serde_json::Value| {
        if ui.selectable_label(on, text(label)).clicked() && !on {
            let mut params = params;
            params["track"] = json!(track.0);
            actions.push(Action::new("track.setAudioInput", params));
            ui.close();
        }
    };
    for (label, params) in audio_input_picks(input, channels) {
        pick(ui, &label, audio_is(input, &params), params);
    }
    if channels == 0 {
        ui.label(text("No input device: choose one in Settings › Audio"));
    }
    ui.separator();
    for (label, params) in audio_monitor_picks() {
        pick(ui, label, audio_is(input, &params), params);
    }
}

/// How a track's MIDI input reads: "All ports", the port, or "No input", with the channel.
pub fn midi_input_text(input: &MidiInput) -> String {
    let port = match &input.port {
        MidiPort::All => "All ports",
        MidiPort::Named(name) => name.as_str(),
        MidiPort::Nothing => "No input",
    };
    match input.channel {
        Some(channel) if input.port != MidiPort::Nothing => format!("{port} · Ch {channel}"),
        _ => port.to_string(),
    }
}

/// The choices of a track's MIDI input (ADR 0017): every port, each port there is (and the one
/// chosen, if it is not there now), none; then every channel or one. A pick runs
/// `track.setMidiInput`.
pub fn midi_input_choices(ui: &mut Ui, track: TrackId, input: &MidiInput, ports: &[String], actions: &mut Vec<Action>) {
    let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
    let mut pick = |ui: &mut Ui, label: &str, on: bool, params: serde_json::Value| {
        if ui.selectable_label(on, text(label)).clicked() && !on {
            let mut params = params;
            params["track"] = json!(track.0);
            actions.push(Action::new("track.setMidiInput", params));
            ui.close();
        }
    };
    pick(ui, "All ports", input.port == MidiPort::All, json!({ "port": "all" }));
    let chosen = match &input.port {
        MidiPort::Named(name) if !ports.contains(name) => Some(name),
        _ => None,
    };
    for name in ports.iter().chain(chosen) {
        let label = if Some(name) == chosen { format!("{name} (not connected)") } else { name.clone() };
        pick(ui, &label, input.port == MidiPort::Named(name.clone()), json!({ "port": name }));
    }
    pick(ui, "No input", input.port == MidiPort::Nothing, json!({ "port": "none" }));
    ui.separator();
    pick(ui, "All channels", input.channel.is_none(), json!({ "channel": 0 }));
    egui::Grid::new(("midi-channels", track.0)).spacing(Vec2::splat(2.0)).show(ui, |ui| {
        for channel in 1..=16u8 {
            pick(ui, &format!("Ch {channel}"), input.channel == Some(channel), json!({ "channel": channel }));
            if channel % 4 == 0 {
                ui.end_row();
            }
        }
    });
}

/// The menu of a track header or column: name, colour, group, ungroup, move into or out of a
/// group, delete.
pub fn track_menu(ui: &mut Ui, menu: &TrackMenu<'_>, actions: &mut Vec<Action>) {
    let track = menu.track.0;
    // A compact menu: as wide as its widest item needs, never the window's width.
    ui.set_min_width(180.0);
    ui.set_max_width(220.0);
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.spacing_mut().button_padding.y = 2.0;

    // Name: edited in place, applied with Enter.
    let id = Id::new(("rename", track));
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| menu.name.to_string());
    let response = ui.add(egui::TextEdit::singleline(&mut text).hint_text("Name").desired_width(200.0));
    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        let name = text.trim();
        if !name.is_empty() && name != menu.name {
            actions.push(Action::new("track.rename", json!({ "track": track, "name": name })));
        }
        ui.data_mut(|d| d.remove::<String>(id));
        ui.close();
        return;
    }
    ui.data_mut(|d| d.insert_temp(id, text));

    // Colour: the palette every colour choice offers.
    // A colour picked with several tracks selected colours them all.
    if let Some(colour) = colour_picker::swatch_row(ui, Some(menu.colour)) {
        let several = menu.selection.contains(&menu.track) && menu.selection.len() > 1;
        if several {
            for track in menu.selection {
                actions.push(colour_picker::set_colour(*track, colour));
            }
        } else if colour != menu.colour {
            actions.push(colour_picker::set_colour(menu.track, colour));
        }
        ui.close();
    }
    // A group's tracks, at any depth, and their clips: its colour, so the group reads as one in
    // every view.
    if menu.is_group && ui.button("Assign track color to all tracks and clips").clicked() {
        // The colour the group shows: one it was never given is worked out from its place.
        let params = json!({ "track": track, "color": colour_picker::hex(menu.colour), "clips": true });
        actions.push(Action::new("track.colorGroup", params));
        ui.close();
    }
    // The track's clips: back to the track's colour, or named after it.
    if ui.button("Assign track color to all clips").clicked() {
        actions.push(Action::new("clip.setColor", json!({ "track": track, "color": "none" })));
        ui.close();
    }
    if ui.button("Rename all clips").clicked() {
        actions.push(Action::new("track.renameClips", json!({ "track": track })));
        ui.close();
    }
    if let Some((input, channels)) = menu.audio {
        ui.menu_button(format!("Audio input: {}", audio_input_text(input)), |ui| {
            audio_input_choices(ui, menu.track, input, channels, actions);
        });
    }
    if let Some((input, ports)) = menu.midi {
        ui.menu_button(format!("MIDI input: {}", midi_input_text(input)), |ui| {
            midi_input_choices(ui, menu.track, input, ports, actions);
        });
    }
    ui.separator();

    let selected = menu.selection.contains(&menu.track) && menu.selection.len() > 1;
    let group_label = if selected { "Group selected tracks" } else { "Group" };
    if ui.add(egui::Button::new(group_label).shortcut_text(shown("Ctrl+G"))).clicked() {
        let tracks: Vec<u32> = if selected { menu.selection.iter().map(|t| t.0).collect() } else { vec![track] };
        actions.push(Action::new("track.group", json!({ "tracks": tracks })));
        ui.close();
    }
    if menu.is_group && ui.button("Ungroup").clicked() {
        actions.push(Action::new("track.ungroup", json!({ "track": track })));
        ui.close();
    }
    if !menu.into.is_empty() {
        ui.menu_button("Move into group", |ui| {
            for (group, name) in menu.into {
                if ui.button(name).clicked() {
                    actions.push(Action::new("track.setGroup", json!({ "track": track, "group": group.0 })));
                    ui.close();
                }
            }
        });
    }
    if let Some(out) = &menu.out
        && ui.button("Move out of group").clicked()
    {
        actions.push(out.clone());
        ui.close();
    }
    ui.separator();
    insert_items(ui, (!menu.is_return).then_some(menu.track), actions);
    ui.separator();
    if ui.button("Delete track").clicked() {
        actions.push(Action::new("track.delete", json!({ "track": track })));
        ui.close();
    }
}

/// Insert audio track, Insert MIDI track (after `after`, or after the regular tracks) and Insert
/// return track, as the track menus and the empty space's menu offer them.
pub fn insert_items(ui: &mut Ui, after: Option<TrackId>, actions: &mut Vec<Action>) {
    for (kind, label, keys) in
        [("audio", "Insert audio track", "Ctrl+T"), ("midi", "Insert MIDI track", "Ctrl+Shift+T")]
    {
        if ui.add(egui::Button::new(label).shortcut_text(shown(keys))).clicked() {
            let mut params = json!({ "kind": kind });
            if let Some(after) = after {
                params["after"] = json!(after.0);
            }
            actions.push(Action::new("track.add", params));
            ui.close();
        }
    }
    if ui.button("Insert return track").clicked() {
        actions.push(Action::new("return.add", json!({})));
        ui.close();
    }
}

/// Whether clip menus carry the name field to rename in place (Settings › Arrangement › Name
/// field in clip menus); off, they carry Rename, which asks for the name. The window sets it.
pub const NAME_FIELD: &str = "menu-name-field";

fn name_field_on(ui: &Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(Id::new(NAME_FIELD))).unwrap_or(false)
}

/// The Rename item: the window asks for the clip's new name (`ui.renameClip`), as Ctrl+R does.
fn rename_item(ui: &mut Ui, target: serde_json::Value, name: &str, actions: &mut Vec<Action>) {
    if ui.add(egui::Button::new("Rename").shortcut_text(shown("Ctrl+R"))).clicked() {
        let mut params = target;
        params["name"] = json!(name);
        actions.push(Action::new("ui.renameClip", params));
        ui.close();
    }
}

/// The menu of arrangement clips; `stack` is true when the stacking setting for the clips' kind
/// is on, so their copies leave the clips they land on as they are. `clips` is the selection the
/// click fell in, or the one clip it fell on; `enabled` is whether the clicked clip plays now.
pub fn clip_menu(
    ui: &mut Ui,
    clips: &[(TrackId, ClipId)],
    stack: bool,
    enabled: bool,
    name: &str,
    actions: &mut Vec<Action>,
) {
    if clips.is_empty() {
        return;
    }
    let list: Vec<serde_json::Value> = clips.iter().map(|(_, clip)| json!({ "clip": clip.0 })).collect();
    ui.set_min_width(140.0);
    // The clips' colour: a swatch or any colour, or back to their tracks' colour.
    // One command for them all: one undo step, and a large selection takes it at once.
    if let Some(colour) = colour_picker::swatch_row(ui, None) {
        actions.push(Action::new(
            "clip.setColorMany",
            json!({ "clips": list.clone(), "color": colour_picker::hex(colour) }),
        ));
        ui.close();
    }
    if ui.button("Track colour").clicked() {
        actions.push(Action::new("clip.setColorMany", json!({ "clips": list.clone(), "color": "none" })));
        ui.close();
    }
    ui.separator();
    // One clip in the menu (a click on it, or the only one selected): Rename, or with the
    // setting on a field that renames it, applied with Enter or Rename, as a slot's clip's is.
    if let [(_, clip)] = clips
        && !name_field_on(ui)
    {
        rename_item(ui, json!({ "clip": clip.0 }), name, actions);
        ui.separator();
    } else if let [(_, clip)] = clips {
        let id = Id::new(("clip-rename", clip.0));
        let mut draft: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| name.to_string());
        let field = ui.add(egui::TextEdit::singleline(&mut draft).id(id).desired_width(140.0));
        ui.ctx().data_mut(|d| d.insert_temp(id, draft.clone()));
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Rename clip {}", name))
        });
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        // An empty draft clears the name: the command takes it.
        if (enter || ui.button("Rename").clicked()) && draft.trim() != name.trim() {
            actions.push(Action::new("clip.rename", json!({ "clip": clip.0, "name": draft.trim() })));
            ui.ctx().data_mut(|d| d.remove::<String>(id));
            ui.close();
        }
        ui.separator();
    }
    if ui.add(egui::Button::new("Copy").shortcut_text(shown("Ctrl+C"))).clicked() {
        actions.push(Action::new("clip.copy", json!({ "clips": list.clone() })));
        ui.close();
    }
    if ui.add(egui::Button::new("Duplicate").shortcut_text(shown("Ctrl+D"))).clicked() {
        let mut params = json!({ "clips": list.clone() });
        if stack {
            params["stack"] = json!(true);
        }
        actions.push(Action::new("clip.duplicate", params));
        ui.close();
    }
    let mute = if enabled { "Mute" } else { "Unmute" };
    if ui.add(egui::Button::new(mute).shortcut_text("0")).clicked() {
        actions.push(Action::new("clip.setEnabled", json!({ "clips": list.clone(), "enabled": !enabled })));
        ui.close();
    }
    if ui.add(egui::Button::new("Delete").shortcut_text("Del")).clicked() {
        actions.push(Action::new("clip.delete", json!({ "clips": list })));
        ui.close();
    }
}

/// The editing part of an arrangement clip's menu: split it at `at`, the beat right-clicked on
/// (when it is inside the clip), and consolidate `joined` (the start and end of the selected
/// clips on its track, or of it alone) into a new clip.
#[allow(clippy::too_many_arguments)]
pub fn clip_edit_menu(
    ui: &mut Ui,
    track: TrackId,
    clip: ClipId,
    span: (f64, f64),
    joined: (f64, f64),
    at: Option<f64>,
    actions: &mut Vec<Action>,
) {
    let (start, length) = span;
    ui.separator();
    let inside = at.filter(|beat| *beat > start && *beat < start + length);
    if ui.add_enabled(inside.is_some(), egui::Button::new("Split here")).clicked()
        && let Some(beat) = inside
    {
        actions.push(Action::new("clip.split", json!({ "clip": clip.0, "at": beat })));
        ui.close();
    }
    if ui.add(egui::Button::new("Consolidate").shortcut_text(shown("Ctrl+J"))).clicked() {
        actions.push(Action::new("clip.consolidate", json!({ "track": track.0, "start": joined.0, "end": joined.1 })));
        ui.close();
    }
}

/// The part of an audio clip's menu on a track whose plug-in edits clips (ARA): open the clip
/// in that plug-in's editor (`view.araEditor`, which the window does).
pub fn ara_menu(ui: &mut Ui, track: TrackId, clip: ClipId, plugin: &str, actions: &mut Vec<Action>) {
    ui.separator();
    if ui.add(egui::Button::new(format!("Open in {plugin}")).shortcut_text("Alt+M")).clicked() {
        actions.push(Action::new("view.araEditor", json!({ "track": track.0, "clip": clip.0 })));
        ui.close();
    }
}

/// What a scene's menu needs to know.
pub struct SceneMenu<'a> {
    /// Counted from 1, as commands take it.
    pub number: usize,
    pub name: &'a str,
    pub tempo_bpm: Option<f64>,
    pub time_signature: Option<TimeSignature>,
    /// The scene is the only one, so it cannot be deleted.
    pub only: bool,
}

/// The menu of a scene's launch button: name, tempo, time signature, insert, duplicate, delete.
pub fn scene_menu(ui: &mut Ui, menu: &SceneMenu<'_>, actions: &mut Vec<Action>) {
    let scene = menu.number;
    ui.set_min_width(180.0);
    let tempo = menu.tempo_bpm.map(|bpm| format!("{bpm}")).unwrap_or_default();
    let signature = menu.time_signature.map(|ts| format!("{}/{}", ts.numerator, ts.denominator)).unwrap_or_default();
    let fields = [
        ("name", menu.name.to_string(), "Name"),
        ("tempo", tempo, "Tempo (bpm), empty for none"),
        ("signature", signature, "Time signature, such as 3/4"),
    ];
    for (key, current, hint) in fields {
        let id = Id::new(("scene-field", key, scene));
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.clone());
        let response = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(f32::INFINITY));
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            if text.trim() != current {
                actions.extend(scene_field_action(key, scene, &text));
            }
            ui.data_mut(|d| d.remove::<String>(id));
            ui.close();
            return;
        }
        ui.data_mut(|d| d.insert_temp(id, text));
    }
    ui.separator();
    if ui.button("Insert scene below").clicked() {
        actions.push(Action::new("scene.insert", json!({ "scene": scene + 1 })));
        ui.close();
    }
    if ui.button("Duplicate scene").clicked() {
        actions.push(Action::new("scene.duplicate", json!({ "scene": scene })));
        ui.close();
    }
    if ui.add_enabled(!menu.only, egui::Button::new("Delete scene")).clicked() {
        actions.push(Action::new("scene.delete", json!({ "scene": scene })));
        ui.close();
    }
}

/// The command a scene menu field sends when its text is applied: a rename, a tempo or a time
/// signature, cleared by empty text. Text that is not a number or a time signature is sent as
/// it is, so the command says what is wrong with it.
pub fn scene_field_action(field: &str, scene: usize, text: &str) -> Option<Action> {
    let text = text.trim();
    match field {
        "name" => Some(Action::new("scene.rename", json!({ "scene": scene, "name": text }))),
        "tempo" if text.is_empty() => Some(Action::new("scene.setTempo", json!({ "scene": scene }))),
        "tempo" => {
            let bpm = text.parse::<f64>().map_or_else(|_| json!(text), |bpm| json!(bpm));
            Some(Action::new("scene.setTempo", json!({ "scene": scene, "bpm": bpm })))
        }
        "signature" if text.is_empty() => Some(Action::new("scene.setTimeSignature", json!({ "scene": scene }))),
        "signature" => {
            let (numerator, denominator) = text.split_once('/').unwrap_or((text, ""));
            let part = |p: &str| p.trim().parse::<u64>().map_or_else(|_| json!(p.trim()), |n| json!(n));
            Some(Action::new(
                "scene.setTimeSignature",
                json!({ "scene": scene, "numerator": part(numerator), "denominator": part(denominator) }),
            ))
        }
        _ => None,
    }
}

/// A clip slot's right-click menu: copy its clip, or paste the copied clip into it (`scene`
/// from 1). Copy is greyed out on an empty slot, Paste while nothing has been copied.
/// The menu of a device in a track's chain: removing it takes it off the track, settings and
/// all.
/// A device's menu in the detail area; `branches` is how many branches it has when it is a
/// device group (`docs/spec/device-groups.md` › The device panel).
pub fn device_menu(
    ui: &mut Ui,
    track: TrackId,
    at: &crate::detail::detail_view::DeviceAt,
    branches: Option<usize>,
    actions: &mut Vec<Action>,
) {
    ui.set_min_width(140.0);
    let device = at.value();
    if ui.add(egui::Button::new("Group").shortcut_text(shown("Ctrl+G"))).clicked() {
        actions.push(Action::new("device.group", json!({ "track": track.0, "device": device })));
        ui.close();
    }
    if let Some(branches) = branches {
        if ui.add_enabled(branches == 1, egui::Button::new("Ungroup").shortcut_text(shown("Ctrl+Shift+G"))).clicked() {
            actions.push(Action::new("device.ungroup", json!({ "track": track.0, "device": device })));
            ui.close();
        }
        if ui.button("Add branch").clicked() {
            actions.push(Action::new("branch.add", json!({ "track": track.0, "device": device })));
            ui.close();
        }
    }
    if ui.button("Duplicate device").clicked() {
        actions.push(Action::new("device.duplicate", json!({ "track": track.0, "device": device })));
        ui.close();
    }
    if ui.button("Remove device").clicked() {
        actions.push(Action::new("device.remove", json!({ "track": track.0, "device": device })));
        ui.close();
    }
}

pub fn slot_menu(
    ui: &mut Ui,
    track: TrackId,
    scene: usize,
    name: &str,
    has_clip: bool,
    can_paste: bool,
    actions: &mut Vec<Action>,
) {
    ui.set_min_width(140.0);
    // Rename, or with the setting on the clip's name typed to rename: applied with Enter or
    // Rename, as a marker's is.
    if has_clip && !name_field_on(ui) {
        rename_item(ui, json!({ "track": track.0, "scene": scene }), name, actions);
    } else if has_clip {
        let id = Id::new(("slot-rename", track, scene));
        let mut draft: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| name.to_string());
        let field = ui.add(egui::TextEdit::singleline(&mut draft).id(id).desired_width(140.0));
        ui.ctx().data_mut(|d| d.insert_temp(id, draft.clone()));
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Rename clip {}", name))
        });
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        // An empty draft clears the name: the command takes it.
        if (enter || ui.button("Rename").clicked()) && draft.trim() != name.trim() {
            actions.push(Action::new("clip.rename", json!({ "track": track.0, "scene": scene, "name": draft.trim() })));
            ui.ctx().data_mut(|d| d.remove::<String>(id));
            ui.close();
        }
    }
    if ui.add_enabled(has_clip, egui::Button::new("Copy").shortcut_text(shown("Ctrl+C"))).clicked() {
        actions.push(Action::new("clip.copy", json!({ "track": track.0, "scene": scene })));
        ui.close();
    }
    if ui.add_enabled(can_paste, egui::Button::new("Paste").shortcut_text(shown("Ctrl+V"))).clicked() {
        actions.push(Action::new("clip.paste", json!({ "track": track.0, "scene": scene })));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strip's dropdown and the track's menu both offer the same list, pairs included: with
    /// no device open, the first eight channels, mono and one pair each, are there to set a
    /// project with before the interface arrives.
    #[test]
    fn audio_input_picks_offer_monos_and_pairs() {
        let picks = audio_input_picks(&AudioInput::default(), 0);
        let labels: Vec<&str> = picks.iter().map(|(l, _)| l.as_str()).collect();
        assert!(labels.contains(&"In 1"), "{labels:?}");
        assert!(labels.contains(&"In 8"), "{labels:?}");
        assert!(labels.contains(&"In 1/2"), "{labels:?}");
        assert!(labels.contains(&"In 3/4"), "{labels:?}");
        let ticked = audio_is(&AudioInput::default(), &picks[0].1);
        assert!(ticked, "no device yet, so no input ticked");
        let monitors = audio_monitor_picks();
        assert_eq!(monitors.len(), 3);
    }

    #[test]
    fn an_audio_clip_opens_in_its_tracks_ara_plug_in() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let mut harness = Harness::builder().with_size(egui::vec2(300.0, 120.0)).build_ui_state(
            |ui, actions: &mut Vec<Action>| ara_menu(ui, TrackId(2), ClipId(7), "Tune", actions),
            Vec::new(),
        );
        harness.run();
        harness.get_by_label_contains("Open in Tune").click();
        harness.run();
        let action = &harness.state()[0];
        assert_eq!((action.id, action.params.clone()), ("view.araEditor", json!({ "track": 2, "clip": 7 })));
    }

    #[test]
    fn scene_fields_become_commands() {
        let action = |field, text| scene_field_action(field, 2, text).map(|a| (a.id, a.params));
        assert_eq!(action("name", " Drop "), Some(("scene.rename", json!({ "scene": 2, "name": "Drop" }))));
        assert_eq!(action("tempo", "128"), Some(("scene.setTempo", json!({ "scene": 2, "bpm": 128.0 }))));
        assert_eq!(action("tempo", ""), Some(("scene.setTempo", json!({ "scene": 2 }))));
        assert_eq!(
            action("signature", "7/8"),
            Some(("scene.setTimeSignature", json!({ "scene": 2, "numerator": 7, "denominator": 8 })))
        );
        assert_eq!(action("signature", " "), Some(("scene.setTimeSignature", json!({ "scene": 2 }))));
        // Not a number: sent as text, so the command's error says what is wrong.
        assert_eq!(action("tempo", "fast"), Some(("scene.setTempo", json!({ "scene": 2, "bpm": "fast" }))));
        assert_eq!(action("colour", "red"), None);
    }
}
