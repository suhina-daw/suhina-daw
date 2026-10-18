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

/// What a track's menu needs to know.
pub struct TrackMenu<'a> {
    pub track: TrackId,
    pub name: &'a str,
    /// The colour the track shows, outlined among the swatches.
    pub colour: Rgb,
    pub is_group: bool,
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
    pick(ui, "No input", input.channels == InputChannels::None, json!({ "input": "none" }));
    if channels == 0 {
        ui.label(text("No input device: choose one in Settings › Audio"));
    }
    // The device's channels, and the one chosen if the device has fewer now; without a device,
    // the first eight, so a project can be set up before the interface is there.
    let channels = if channels == 0 { 8 } else { channels };
    let chosen = match input.channels {
        InputChannels::None => 0,
        InputChannels::Mono(c) => c,
        InputChannels::Stereo(c) => c.saturating_add(1),
    };
    let shown = channels.max(chosen);
    egui::Grid::new(("audio-channels", track.0)).spacing(Vec2::splat(2.0)).show(ui, |ui| {
        for c in 1..=shown {
            pick(ui, &format!("In {c}"), input.channels == InputChannels::Mono(c), json!({ "input": c.to_string() }));
            if c % 4 == 0 {
                ui.end_row();
            }
        }
    });
    egui::Grid::new(("audio-pairs", track.0)).spacing(Vec2::splat(2.0)).show(ui, |ui| {
        for c in (1..shown).step_by(2) {
            let pair = format!("{c}/{}", c + 1);
            pick(ui, &format!("In {pair}"), input.channels == InputChannels::Stereo(c), json!({ "input": pair }));
            if c % 8 == 7 {
                ui.end_row();
            }
        }
    });
    ui.separator();
    for (label, monitor, value) in [
        ("Monitor auto (while armed)", Monitor::Auto, "auto"),
        ("Monitor in", Monitor::In, "in"),
        ("Monitor off", Monitor::Off, "off"),
    ] {
        pick(ui, label, input.monitor == monitor, json!({ "monitor": value }));
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
    ui.set_min_width(180.0);

    // Name: edited in place, applied with Enter.
    let id = Id::new(("rename", track));
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| menu.name.to_string());
    let response = ui.add(egui::TextEdit::singleline(&mut text).hint_text("Name").desired_width(f32::INFINITY));
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
    if let Some(colour) = colour_picker::swatch_row(ui, Some(menu.colour)) {
        if colour != menu.colour {
            actions.push(colour_picker::set_colour(menu.track, colour));
        }
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
    if ui.add(egui::Button::new(group_label).shortcut_text("Ctrl+G")).clicked() {
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
    if ui.button("Delete track").clicked() {
        actions.push(Action::new("track.delete", json!({ "track": track })));
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
    // One clip in the menu (a click on it, or the only one selected): a field renames it,
    // applied with Enter or Rename, as a slot's clip's is.
    if let [(_, clip)] = clips {
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
    if ui.add(egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
        actions.push(Action::new("clip.copy", json!({ "clips": list.clone() })));
        ui.close();
    }
    if ui.add(egui::Button::new("Duplicate").shortcut_text("Ctrl+D")).clicked() {
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
/// (when it is inside the clip), and consolidate it into a new clip.
pub fn clip_edit_menu(
    ui: &mut Ui,
    track: TrackId,
    clip: ClipId,
    span: (f64, f64),
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
    if ui.add(egui::Button::new("Consolidate").shortcut_text("Ctrl+J")).clicked() {
        actions
            .push(Action::new("clip.consolidate", json!({ "track": track.0, "start": start, "end": start + length })));
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
    // The clip's name, typed to rename: applied with Enter or Rename, as a marker's is.
    if has_clip {
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
    if ui.add_enabled(has_clip, egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
        actions.push(Action::new("clip.copy", json!({ "track": track.0, "scene": scene })));
        ui.close();
    }
    if ui.add_enabled(can_paste, egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
        actions.push(Action::new("clip.paste", json!({ "track": track.0, "scene": scene })));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
