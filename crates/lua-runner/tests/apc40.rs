// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shipped APC40 MkII script's device section, against a stand-in Suhina: the eight knobs
//! turn the selected device's parameters in banks of eight, Bank left/right take the next or
//! previous eight, and Device left/right step along the chain (`docs/spec/selection.md` › What
//! controllers do with it, #1679). The script under test is the shipped file itself.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test harness

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, LuaString, Value, Variadic};

/// One command the script asked for, with the fields the device section sends.
#[derive(Debug, Default, Clone, PartialEq)]
struct Command {
    id: String,
    track: Option<u32>,
    tracks: Vec<u32>,
    track_name: Option<String>,
    device: Option<String>,
    param: Option<String>,
    value: Option<f64>,
    normalized: Option<f64>,
    assign: Option<String>,
    scene: Option<u64>,
    bpm: Option<f64>,
    db: Option<f64>,
    on: Option<bool>,
    level: Option<f64>,
    pan: Option<f64>,
    return_track: Option<u32>,
    gesture: Option<String>,
}

/// What the script sent to its output port.
type Sends = Rc<RefCell<Vec<(String, Vec<u8>)>>>;

/// A stand-in Suhina: records commands and send-backs, and answers the script's subscriptions.
struct Fake {
    lua: Lua,
    commands: Rc<RefCell<Vec<Command>>>,
    sends: Sends,
}

impl Fake {
    fn new(script: &str) -> Self {
        let lua = Lua::new();
        let commands = Rc::new(RefCell::new(Vec::new()));
        let sends = Rc::new(RefCell::new(Vec::new()));
        let suhina = lua.create_table().unwrap();
        // JSON null: a value that is neither nil nor false, as the runner's `suhina.null`.
        suhina.set("null", lua.create_table().unwrap()).unwrap();

        let midi = lua.create_table().unwrap();
        midi.set(
            "parse",
            lua.create_function(|lua, bytes: LuaString| {
                let bytes = bytes.as_bytes();
                let message = lua.create_table()?;
                match bytes.first().copied() {
                    Some(status) if status & 0xF0 == 0x80 => {
                        message.set("kind", "note_off")?;
                        message.set("channel", status & 0x0F)?;
                        message.set("note", *bytes.get(1).unwrap_or(&0))?;
                        message.set("velocity", *bytes.get(2).unwrap_or(&0))?;
                    }
                    Some(status) if status & 0xF0 == 0x90 => {
                        message.set("kind", "note_on")?;
                        message.set("channel", status & 0x0F)?;
                        message.set("note", *bytes.get(1).unwrap_or(&0))?;
                        message.set("velocity", *bytes.get(2).unwrap_or(&0))?;
                    }
                    Some(status) if status & 0xF0 == 0xB0 => {
                        message.set("kind", "cc")?;
                        message.set("channel", status & 0x0F)?;
                        message.set("controller", *bytes.get(1).unwrap_or(&0))?;
                        message.set("value", *bytes.get(2).unwrap_or(&0))?;
                    }
                    _ => return Ok(Value::Nil),
                }
                Ok(Value::Table(message))
            })
            .unwrap(),
        )
        .unwrap();
        midi.set(
            "cc",
            lua.create_function(|lua, (channel, controller, value): (u8, u8, u8)| {
                lua.create_string([0xB0 | (channel & 0x0F), controller, value])
            })
            .unwrap(),
        )
        .unwrap();
        midi.set(
            "note_on",
            lua.create_function(|lua, (channel, note, velocity): (u8, u8, u8)| {
                lua.create_string([0x90 | (channel & 0x0F), note, velocity])
            })
            .unwrap(),
        )
        .unwrap();
        suhina.set("midi", midi).unwrap();

        let recorded = Rc::clone(&commands);
        suhina
            .set(
                "command",
                lua.create_function(move |_, args: Variadic<Value>| {
                    let id = args.first().and_then(text).unwrap_or_default();
                    let params = args.get(1).and_then(Value::as_table);
                    let field = |key: &str| params.and_then(|t| t.get::<Value>(key).ok());
                    let count = |key: &str| field(key).and_then(|v| number(&v)).map(|n| n as u32);
                    let text_field = |key: &str| field(key).and_then(|v| text(&v));
                    let tracks: Vec<u32> = field("tracks")
                        .and_then(|v| match v {
                            Value::Table(table) => Some(
                                table
                                    .sequence_values::<Value>()
                                    .filter_map(Result::ok)
                                    .filter_map(|v| number(&v))
                                    .map(|n| n as u32)
                                    .collect(),
                            ),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let scene = field("scene").and_then(|v| match v {
                        Value::Table(table) => table.get::<Value>("id").ok().and_then(|id| number(&id)),
                        _ => number(&v),
                    });
                    let device = field("device").and_then(|v| match v {
                        Value::Integer(n) => Some(n.to_string()),
                        Value::Number(n) => Some(format!("{n:?}")),
                        Value::String(s) => Some(s.to_string_lossy().to_string()),
                        _ => None,
                    });
                    recorded.borrow_mut().push(Command {
                        id,
                        track: count("track"),
                        tracks,
                        track_name: field("track").and_then(|v| text(&v)),
                        device,
                        param: text_field("param"),
                        value: field("value").and_then(|v| number(&v)),
                        normalized: field("normalized").and_then(|v| number(&v)),
                        assign: text_field("assign"),
                        scene: scene.map(|n| n as u64),
                        bpm: field("bpm").and_then(|v| number(&v)),
                        db: field("db").and_then(|v| number(&v)),
                        on: field("on").and_then(|v| match v {
                            Value::Boolean(on) => Some(on),
                            _ => None,
                        }),
                        level: field("level").and_then(|v| number(&v)),
                        pan: field("pan").and_then(|v| number(&v)),
                        return_track: count("return"),
                        gesture: args.get(2).and_then(text),
                    });
                    Ok(true)
                })
                .unwrap(),
            )
            .unwrap();
        let written = Rc::clone(&sends);
        suhina
            .set(
                "send",
                lua.create_function(move |_, (port, bytes): (String, LuaString)| {
                    written.borrow_mut().push((port.clone(), bytes.as_bytes().to_vec()));
                    Ok(true)
                })
                .unwrap(),
            )
            .unwrap();
        let next = Rc::new(RefCell::new(0u64));
        suhina
            .set(
                "subscribe",
                lua.create_function(move |_, _: Variadic<Value>| {
                    let mut next = next.borrow_mut();
                    *next += 1;
                    Ok(*next)
                })
                .unwrap(),
            )
            .unwrap();
        suhina.set("unsubscribe", lua.create_function(|_, _: Value| Ok(())).unwrap()).unwrap();
        suhina.set("read", lua.create_function(|_, _: Variadic<Value>| Ok((Value::Nil, Value::Nil))).unwrap()).unwrap();
        for name in ["on_port_message", "on_state", "on_ports"] {
            suhina
                .set(
                    name,
                    lua.create_function(move |lua, handler: mlua::Function| {
                        lua.set_named_registry_value(name, handler)
                    })
                    .unwrap(),
                )
                .unwrap();
        }
        lua.globals().set("suhina", suhina).unwrap();
        lua.load(script).exec().unwrap();
        Self { lua, commands, sends }
    }

    /// The ports opening, with one output to send LEDs on.
    fn ports(&self) {
        let inputs = self.lua.create_table().unwrap();
        let outputs = self.lua.create_table().unwrap();
        outputs.set(1, "APC40 mkII out").unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_ports").unwrap();
        handler.call::<()>((inputs, outputs)).unwrap();
    }

    /// A selection state: `track`'s `device` with the Utility's parameters.
    fn selection(&self, token: u64, track: u32, path: u32) {
        self.selection_with_gain(token, track, path, 0.0);
    }

    /// The same with the first dial's Gain at `gain`, for the ring guard.
    fn selection_with_gain(&self, token: u64, track: u32, path: u32, gain: f64) {
        let selection = self.lua.create_table().unwrap();
        selection.set("track", track).unwrap();
        selection.set("track_name", "Drums").unwrap();
        let device = self.lua.create_table().unwrap();
        device.set("path", path).unwrap();
        device.set("enabled", true).unwrap();
        // Nine dials with switches and a choice between them, in parameter order; the dials
        // list is the panel's own order, which a knob follows instead.
        let params = [
            ("gain", "continuous", -150.0, 35.0, gain),
            ("mono", "toggle", 0.0, 1.0, 0.0),
            ("balance", "continuous", -1.0, 1.0, 0.0),
            ("width", "continuous", 0.0, 4.0, 1.0),
            ("midSide", "continuous", -1.0, 1.0, 0.0),
            ("bassMonoFrequency", "continuous", 50.0, 500.0, 120.0),
            ("invertLeft", "toggle", 0.0, 1.0, 0.0),
            ("invertRight", "toggle", 0.0, 1.0, 0.0),
            ("channel", "choice", 0.0, 3.0, 0.0),
            ("dial6", "continuous", 0.0, 1.0, 0.0),
            ("dial7", "continuous", 0.0, 1.0, 0.0),
            ("dial8", "continuous", 0.0, 1.0, 0.0),
            ("dial9", "continuous", 0.0, 1.0, 0.0),
            ("attack", "continuous", 0.0005, 60.0, 0.001),
        ];
        let list = self.lua.create_table().unwrap();
        for (index, (id, kind, min, max, value)) in params.iter().enumerate() {
            let param = self.lua.create_table().unwrap();
            param.set("id", *id).unwrap();
            param.set("kind", *kind).unwrap();
            param.set("min", *min).unwrap();
            param.set("max", *max).unwrap();
            param.set("value", *value).unwrap();
            let normalized = if *id == "attack" { 0.5 } else { (*value - *min) / (*max - *min) };
            param.set("normalized", normalized).unwrap();
            param.set("unit", "").unwrap();
            list.set(index + 1, param).unwrap();
        }
        device.set("params", list).unwrap();
        let dials = self.lua.create_table().unwrap();
        for (index, id) in
            ["dial6", "gain", "balance", "width", "midSide", "bassMonoFrequency", "dial7", "dial8", "dial9", "attack"]
                .iter()
                .enumerate()
        {
            dials.set(index + 1, *id).unwrap();
        }
        device.set("dials", dials).unwrap();
        selection.set("device", device).unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((token, selection)).unwrap();
    }

    /// A running plug-in's selection, as `project.selection` gives it: numeric parameter ids,
    /// the switches among the sliders, and the sliders' ids in `dials`.
    fn plugin_selection(&self, token: u64, track: u32, path: u32) {
        let selection = self.lua.create_table().unwrap();
        selection.set("track", track).unwrap();
        selection.set("track_name", "Serum").unwrap();
        let device = self.lua.create_table().unwrap();
        device.set("path", path).unwrap();
        device.set("enabled", true).unwrap();
        let params = [
            (101, "Bypass", "toggle", 0.0, 1.0, 0.0),
            (102, "Cutoff", "continuous", 0.0, 1.0, 0.5),
            (103, "Mode", "continuous", 0.0, 3.0, 1.0),
        ];
        let list = self.lua.create_table().unwrap();
        for (index, (id, name, kind, min, max, value)) in params.iter().enumerate() {
            let param = self.lua.create_table().unwrap();
            param.set("id", *id).unwrap();
            param.set("name", *name).unwrap();
            param.set("kind", *kind).unwrap();
            param.set("min", *min).unwrap();
            param.set("max", *max).unwrap();
            param.set("value", *value).unwrap();
            param.set("unit", "").unwrap();
            list.set(index + 1, param).unwrap();
        }
        device.set("params", list).unwrap();
        let dials = self.lua.create_table().unwrap();
        dials.set(1, 102).unwrap();
        dials.set(2, 103).unwrap();
        device.set("dials", dials).unwrap();
        selection.set("device", device).unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((token, selection)).unwrap();
    }

    /// One port message: CC 16 or a device-section note.
    fn message(&self, bytes: &[u8]) {
        self.message_at(bytes, 0);
    }

    /// One port message with its arrival timestamp in microseconds, for the tap tempo.
    fn message_at(&self, bytes: &[u8], timestamp: u64) {
        let handler: mlua::Function = self.lua.named_registry_value("on_port_message").unwrap();
        let bytes = self.lua.create_string(bytes).unwrap();
        handler.call::<()>((bytes, "APC40 mkII in", timestamp)).unwrap();
    }

    /// The tracks the project's subscription lists, so the track select buttons have ids.
    fn strips(&self, ids: &[u32]) {
        let strips = self.lua.create_table().unwrap();
        for (index, id) in ids.iter().enumerate() {
            let strip = self.lua.create_table().unwrap();
            strip.set("id", *id).unwrap();
            strip.set("pan", 0.0).unwrap();
            strip.set("crossfade_assign", "off").unwrap();
            strips.set(index + 1, strip).unwrap();
        }
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((3u64, strips)).unwrap();
    }

    /// The strips with their crossfade assigns, then the state read as one look.
    fn strips_assigns(&self, ids: &[(u32, &str)]) {
        let strips = self.lua.create_table().unwrap();
        for (index, (id, assign)) in ids.iter().enumerate() {
            let strip = self.lua.create_table().unwrap();
            strip.set("id", *id).unwrap();
            strip.set("pan", 0.0).unwrap();
            strip.set("crossfade_assign", *assign).unwrap();
            strips.set(index + 1, strip).unwrap();
        }
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((3u64, strips)).unwrap();
    }

    /// The launcher grid's scenes, each with its id, and no clips: enough for the scene
    /// launch buttons and their lights.
    fn launcher_grid(&self, scenes: &[(u64, &str)]) {
        self.launcher_grid_with_color(scenes, None);
    }

    /// The same grid with the clips' colour, as `project.launcher` gives the slot's own or its
    /// track's.
    fn launcher_grid_with_color(&self, scenes: &[(u64, &str)], color: Option<&str>) {
        self.launcher_grid_slots(scenes, color, true);
    }

    /// A grid whose slots are empty, for the Session Record pad.
    fn launcher_grid_empty(&self, scenes: &[(u64, &str)]) {
        self.launcher_grid_slots(scenes, None, false);
    }

    fn launcher_grid_slots(&self, scenes: &[(u64, &str)], color: Option<&str>, clips: bool) {
        self.launcher_grid_tracks(scenes, color, clips, 8);
    }

    /// A wider grid, so the Bank Select arrows have somewhere to scroll.
    fn launcher_grid_big(&self, scenes: &[(u64, &str)]) {
        self.launcher_grid_tracks(scenes, None, true, 9);
    }

    /// Wide enough for the Shift arrow jumps, which move five scenes and eight tracks.
    fn launcher_grid_huge(&self, scenes: &[(u64, &str)]) {
        self.launcher_grid_tracks(scenes, None, true, 17);
    }

    fn launcher_grid_tracks(&self, scenes: &[(u64, &str)], color: Option<&str>, clips: bool, track_count: u32) {
        let grid = self.lua.create_table().unwrap();
        let list = self.lua.create_table().unwrap();
        for (index, (id, name)) in scenes.iter().enumerate() {
            let scene = self.lua.create_table().unwrap();
            scene.set("scene_id", *id).unwrap();
            scene.set("name", *name).unwrap();
            list.set(index + 1, scene).unwrap();
        }
        grid.set("scenes", list).unwrap();
        // Two clips in every scene's row, on the first two tracks, so a scene is playing only
        // when both play and a partly stopped row is not.
        let tracks = self.lua.create_table().unwrap();
        for track in 0..track_count {
            let column = self.lua.create_table().unwrap();
            column.set("track", track + 1).unwrap();
            let slots = self.lua.create_table().unwrap();
            for scene in 0..scenes.len() {
                let slot = self.lua.create_table().unwrap();
                if clips && (track < 2 || track_count > 9) {
                    slot.set("clip", "Clip").unwrap();
                    if let Some(color) = color {
                        slot.set("color", color).unwrap();
                    }
                }
                slot.set("stop_button", true).unwrap();
                slots.set(scene + 1, slot).unwrap();
            }
            column.set("slots", slots).unwrap();
            tracks.set(track + 1, column).unwrap();
        }
        grid.set("tracks", tracks).unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((1u64, grid)).unwrap();
    }

    /// The launcher's state: which track (0 to 7) plays which scene (0 to 4), the rest idle.
    fn launcher_state(&self, playing: &[(u32, u32)]) {
        self.launcher_state_tempo(playing, 120.0);
    }

    /// The launcher's state with the tempo its `tempo_bpm` reports.
    fn launcher_state_tempo(&self, playing: &[(u32, u32)], tempo_bpm: f64) {
        let state = self.lua.create_table().unwrap();
        state.set("running", true).unwrap();
        state.set("tempo_bpm", tempo_bpm).unwrap();
        let tracks = self.lua.create_table().unwrap();
        for track in 0..8u32 {
            let column = self.lua.create_table().unwrap();
            if let Some((_, scene)) = playing.iter().find(|(t, _)| *t == track) {
                let now = self.lua.create_table().unwrap();
                now.set("scene", *scene).unwrap();
                column.set("playing", now).unwrap();
            }
            tracks.set(track + 1, column).unwrap();
        }
        state.set("tracks", tracks).unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((2u64, state)).unwrap();
    }

    /// The record state, as `record.state` gives it: whether the arrangement records.
    fn record_state(&self, arrangement: bool) {
        self.record_state_with(arrangement, None);
    }

    /// The same with a take running into `track`'s slot of `scene`, for the second press.
    fn record_slot_take(&self, track: u32, scene: u32) {
        self.record_state_with(false, Some((track, scene)));
    }

    /// The audio output's state: the cue's level in dB, or null for silence.
    fn audio_info(&self, cue_db: Option<f64>) {
        let state = self.lua.create_table().unwrap();
        let chosen = self.lua.create_table().unwrap();
        if let Some(db) = cue_db {
            chosen.set("cue_db", db).unwrap();
        }
        state.set("chosen", chosen).unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((6u64, state)).unwrap();
    }

    fn record_state_with(&self, arrangement: bool, slot_take: Option<(u32, u32)>) {
        let state = self.lua.create_table().unwrap();
        state.set("arrangement", arrangement).unwrap();
        let takes = self.lua.create_table().unwrap();
        if let Some((track, scene)) = slot_take {
            let take = self.lua.create_table().unwrap();
            take.set("track", track).unwrap();
            let into = self.lua.create_table().unwrap();
            into.set("scene", scene).unwrap();
            take.set("into", into).unwrap();
            takes.set(1, take).unwrap();
        }
        state.set("takes", takes).unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((5u64, state)).unwrap();
    }

    /// Strips with their armed state and sends: each send's return id and level in dB.
    fn strips_mixer(&self, entries: &[(u32, bool, &[f64])]) {
        let strips = self.lua.create_table().unwrap();
        for track in 0..8u32 {
            let strip = self.lua.create_table().unwrap();
            let (id, armed, levels) = entries
                .get(track as usize)
                .map_or((track + 1, false, &[][..]), |(id, armed, levels)| (*id, *armed, *levels));
            strip.set("id", id).unwrap();
            strip.set("pan", 0.0).unwrap();
            strip.set("armed", armed).unwrap();
            strip.set("crossfade_assign", "off").unwrap();
            let sends = self.lua.create_table().unwrap();
            for (index, level) in levels.iter().enumerate() {
                let send = self.lua.create_table().unwrap();
                send.set("return", 100 + index as u32).unwrap();
                send.set("level_db", *level).unwrap();
                sends.set(index + 1, send).unwrap();
            }
            strip.set("sends", sends).unwrap();
            strips.set(track + 1, strip).unwrap();
        }
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((3u64, strips)).unwrap();
    }

    fn last_command(&self) -> Command {
        self.commands.borrow().last().cloned().unwrap_or_default()
    }

    fn commands(&self) -> Vec<String> {
        self.commands.borrow().iter().map(|c| c.id.clone()).collect()
    }

    fn knob(&self, knob: u8, value: u8) {
        self.message(&[0xB0, 16 + knob, value]);
    }

    fn encoder(&self, track: u8, value: u8) {
        self.message(&[0xB0, 48 + track, value]);
    }

    fn press(&self, note: u8) {
        self.message(&[0x90, note, 127]);
    }
}

#[test]
fn the_device_knobs_turn_the_selected_devices_parameters_in_banks_of_eight() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    // The selection the window and commands share: track 1, the Utility at path 0.
    fake.selection(4, 1, 0);

    // The first knob, full: the panel's first dial, not the parameter list's first.
    fake.knob(0, 127);
    let command = fake.last_command();
    assert_eq!(command.id, "device.setParam", "{command:?}");
    assert_eq!(command.track, Some(1), "{command:?}");
    assert_eq!(command.device.as_deref(), Some("0"), "{command:?}");
    assert_eq!(command.param.as_deref(), Some("dial6"), "{command:?}");
    assert_eq!(command.value, None, "a dial with a normalized position: {command:?}");
    assert!((command.normalized.unwrap_or_default() - 1.0).abs() < 1e-6, "{command:?}");
    assert_eq!(command.gesture.as_deref(), Some("apc40-mkii device 1:0:dial6"), "{command:?}");

    // The panel's second dial is Gain; the switch between them in the parameter list is skipped.
    fake.knob(1, 127);
    let command = fake.last_command();
    assert_eq!(command.param.as_deref(), Some("gain"), "{command:?}");
    assert_eq!(command.value, None, "{command:?}");
    assert!((command.normalized.unwrap_or_default() - 1.0).abs() < 1e-6, "{command:?}");
    // Bank right, then the first knob: the ninth dial.
    fake.press(61);
    fake.knob(0, 127);
    let command = fake.last_command();
    assert_eq!(command.param.as_deref(), Some("dial9"), "{command:?}");
    // Bank left, then the first knob at zero: the first dial at its minimum.
    fake.press(60);
    fake.knob(0, 0);
    let command = fake.last_command();
    assert_eq!(command.param.as_deref(), Some("dial6"), "{command:?}");
    assert!(command.normalized.unwrap_or_default() < 0.001, "{command:?}");
}

/// A plug-in's generated panel gives its sliders to the knobs, in the panel's order: a switch
/// between them stays on the panel, and a numeric parameter id reaches `device.setParam` as the
/// text it takes (#1679).
#[test]
fn a_plugins_sliders_take_the_device_knobs_in_panel_order() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.plugin_selection(4, 2, 1);
    // The first knob at half: the panel's first slider, not the switch before it.
    fake.knob(0, 64);
    let command = fake.last_command();
    assert_eq!(command.id, "device.setParam", "{command:?}");
    assert_eq!(command.track, Some(2), "{command:?}");
    assert_eq!(command.device.as_deref(), Some("1"), "{command:?}");
    assert_eq!(command.param.as_deref(), Some("102"), "the numeric id as text: {command:?}");
    assert_eq!(command.normalized, None, "a plug-in takes its own value: {command:?}");
    assert!((command.value.unwrap_or_default() - 64.0 / 127.0).abs() < 1e-6, "{command:?}");
    // The next slider, a list of four: its value is across the plug-in's range.
    fake.knob(1, 127);
    let command = fake.last_command();
    assert_eq!(command.param.as_deref(), Some("103"), "{command:?}");
    assert!((command.value.unwrap_or_default() - 3.0).abs() < 1e-6, "{command:?}");
}

/// A logarithmic dial sends the reading's normalized position, not a linear value: its knob
/// position is the panel's, and its ring draws that same position (#1718).
#[test]
fn a_log_dial_sends_and_draws_its_normalized_position() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.selection(4, 1, 0);
    // Bank right: its second knob is the stand-in's tenth dial, the logarithmic attack.
    let before = fake.sends.borrow().len();
    fake.press(61);
    assert!(
        fake.sends.borrow()[before..].iter().any(|(_, bytes)| bytes.as_slice() == [0xB0, 17, 64].as_slice()),
        "the reading's 0.5 draws at 64: {:?}",
        &fake.sends.borrow()[before..]
    );
    let after_knob = fake.sends.borrow().len();
    fake.knob(1, 64);
    let command = fake.last_command();
    assert_eq!(command.param.as_deref(), Some("attack"), "{command:?}");
    assert_eq!(command.value, None, "{command:?}");
    assert!((command.normalized.unwrap_or_default() - 64.0 / 127.0).abs() < 1e-6, "{command:?}");
    assert!(
        !fake.sends.borrow()[after_knob..].iter().any(|(_, bytes)| bytes.as_slice() == [0xB0, 17, 64].as_slice()),
        "the turn does not write the ring: {:?}",
        &fake.sends.borrow()[after_knob..]
    );
}

#[test]
fn the_device_arrows_select_the_previous_and_next_device() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.selection(4, 1, 0);
    fake.press(59);
    assert_eq!(fake.last_command().id, "device.selectNext", "{:?}", fake.commands());
    fake.press(58);
    assert_eq!(fake.last_command().id, "device.selectPrevious", "{:?}", fake.commands());
}

#[test]
fn the_track_and_master_select_buttons_set_the_selection() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.strips(&[7, 8]);
    // Track select button 1, then master: the commands the window and MCP run.
    fake.press(51);
    let command = fake.last_command();
    assert_eq!(command.id, "track.select", "{command:?}");
    assert_eq!(command.track, Some(7), "{command:?}");
    fake.press(80);
    let command = fake.last_command();
    assert_eq!(command.id, "track.select", "{command:?}");
    assert_eq!(command.track_name.as_deref(), Some("main"), "{command:?}");
}

/// A clip pad takes the colour the window gives its clip — the clip's own or its track's —
/// snapped to the hardware's palette, not a fixed white (#1677).
#[test]
fn the_clip_pads_take_the_colours_the_window_shows() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    let before = fake.sends.borrow().len();
    fake.launcher_grid_with_color(&[(10, "Intro")], Some("#ff0000"));
    let sent = fake.sends.borrow()[before..].to_vec();
    assert!(
        sent.iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 32, 5].as_slice()),
        "the first track's clip is red: {sent:?}"
    );
    assert!(
        sent.iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 33, 5].as_slice()),
        "the second track's clip is red too: {sent:?}"
    );
    let before = fake.sends.borrow().len();
    fake.launcher_grid_with_color(&[(10, "Intro")], Some("#00ff00"));
    let sent = fake.sends.borrow()[before..].to_vec();
    assert!(
        sent.iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 32, 21].as_slice()),
        "the palette's green: {sent:?}"
    );
    // A clip the project gives no colour is the old white.
    let before = fake.sends.borrow().len();
    fake.launcher_grid_with_color(&[(10, "Intro")], None);
    assert!(
        fake.sends.borrow()[before..].iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 32, 3].as_slice()),
        "no colour is white: {:?}",
        &fake.sends.borrow()[before..]
    );
}

/// A scene launch button lights green while any track plays its scene, and a press on a lit
/// one stops that scene; a blank one launches it (#1677).
#[test]
fn the_scene_launch_buttons_light_and_toggle_their_scene() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.launcher_grid(&[(10, "Intro"), (11, "Verse"), (12, "Chorus"), (13, "Bridge"), (14, "Out")]);
    // Both tracks play scene 1 (the second row): its launch button is green, the first blank.
    let before = fake.sends.borrow().len();
    fake.launcher_state(&[(0, 1), (1, 1)]);
    let sent = fake.sends.borrow()[before..].to_vec();
    assert!(
        sent.iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 83, 21].as_slice()),
        "the playing scene lights green: {sent:?}"
    );
    assert!(
        !sent.iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 82, 21].as_slice()),
        "a stopped scene is not lit: {sent:?}"
    );
    // A press on the lit scene stops the row it launched.
    fake.press(83);
    let command = fake.last_command();
    assert_eq!(command.id, "scene.stop", "{command:?}");
    assert_eq!(command.scene, Some(11), "{command:?}");
    // Stopped, it goes blank, and a press launches it again.
    let before = fake.sends.borrow().len();
    fake.launcher_state(&[]);
    assert!(
        fake.sends.borrow()[before..].iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 83, 0].as_slice()),
        "the stopped scene blanks: {:?}",
        &fake.sends.borrow()[before..]
    );
    fake.press(83);
    let command = fake.last_command();
    assert_eq!(command.id, "scene.launch", "{command:?}");
    assert_eq!(command.scene, Some(11), "{command:?}");
}

/// Pan, Sends and User take the eight encoders: pan, a return's send (paged by another Sends
/// press), and nothing in User mode, where the encoders are left alone. Their buttons light the
/// open mode.
#[test]
fn the_encoder_modes_take_pan_sends_and_user() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.strips_mixer(&[(7, false, &[-6.0, -12.0])]);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 87, 127].as_slice()),
        "Pan is the open mode: {:?}",
        fake.sends.borrow()
    );
    // Pan: the first encoder at full left.
    fake.encoder(0, 0);
    let command = fake.last_command();
    assert_eq!(command.id, "track.setPan", "{command:?}");
    assert_eq!(command.tracks, [7], "{command:?}");
    assert!((command.pan.unwrap_or_default() + 1.0).abs() < 1e-6, "{command:?}");
    // Sends: the first send, full up.
    fake.press(88);
    fake.encoder(0, 127);
    let command = fake.last_command();
    assert_eq!(command.id, "track.setSend", "{command:?}");
    assert_eq!(command.track, Some(7), "{command:?}");
    assert_eq!(command.return_track, Some(100), "{command:?}");
    assert!((command.level.unwrap_or_default()).abs() < 1e-6, "{command:?}");
    assert_eq!(command.gesture.as_deref(), Some("apc40-mkii send 7"), "{command:?}");
    // Sends again pages to the second return; its bottom is silence.
    fake.press(88);
    fake.encoder(0, 0);
    let command = fake.last_command();
    assert_eq!(command.id, "track.setSend", "{command:?}");
    assert_eq!(command.return_track, Some(101), "{command:?}");
    assert_eq!(command.level, None, "silence is -inf: {command:?}");
    // Pan again, and the knob is a pan once more.
    fake.press(87);
    fake.encoder(0, 127);
    let command = fake.last_command();
    assert_eq!(command.id, "track.setPan", "{command:?}");
    assert_eq!(command.tracks, [7], "{command:?}");
    assert!(command.pan.unwrap_or_default() > 0.9, "{command:?}");
    // User: lit, the rings off, and the encoders send no command at all.
    fake.press(89);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 89, 127].as_slice()),
        "User lights: {:?}",
        fake.sends.borrow()
    );
    let before = fake.commands.borrow().len();
    fake.encoder(0, 64);
    assert_eq!(fake.commands.borrow().len(), before, "User leaves the encoder alone: {:?}", fake.commands());
}

/// The Record button records the arrangement and lights while it does; Session Record arms
/// empty pads on armed tracks to record and lights while it is armed or a slot take runs.
#[test]
fn the_record_and_session_buttons_toggle_their_lights() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.strips_mixer(&[(7, true, &[])]);
    fake.record_state(false);
    fake.press(93);
    let command = fake.last_command();
    assert_eq!(command.id, "transport.record", "{command:?}");
    assert_eq!(command.on, Some(true), "{command:?}");
    fake.record_state(true);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 93, 127].as_slice()),
        "the record light follows the arrangement: {:?}",
        fake.sends.borrow()
    );
    fake.press(102);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 102, 127].as_slice()),
        "Session Record lights: {:?}",
        fake.sends.borrow()
    );
    // An empty pad on the armed first track records instead of launching.
    fake.launcher_grid_empty(&[(10, "Intro")]);
    fake.press(32);
    let command = fake.last_command();
    assert_eq!(command.id, "clip.record", "{command:?}");
    assert_eq!(command.track, Some(7), "{command:?}");
    assert_eq!(command.scene, Some(0), "{command:?}");
    // The take running, the same pad ends it; the Session Record light already says a take runs.
    fake.record_slot_take(7, 0);
    fake.press(32);
    assert_eq!(fake.last_command().id, "clip.launch", "{:?}", fake.commands());
    // Disarmed for session record, the same empty pad with a stop button stops the track.
    fake.record_state(false);
    fake.press(102);
    fake.press(32);
    assert_eq!(fake.last_command().id, "clip.launch", "{:?}", fake.commands());
    // The record button stops the arrangement once the state says it records.
    fake.record_state(true);
    fake.press(93);
    let command = fake.last_command();
    assert_eq!(command.id, "transport.record", "{command:?}");
    assert_eq!(command.on, Some(false), "{command:?}");
}

/// The tempo knob is relative and steps a BPM a click; the nudge buttons step it too; the tap
/// tempo takes the spacing of two taps.
#[test]
fn the_tempo_knob_nudge_and_tap_tempo_set_the_tempo() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.launcher_state_tempo(&[], 120.0);
    fake.message_at(&[0xB0, 13, 1], 1);
    let command = fake.last_command();
    assert_eq!(command.id, "transport.setTempo", "{command:?}");
    assert!((command.bpm.unwrap_or_default() - 121.0).abs() < 1e-6, "{command:?}");
    assert_eq!(command.gesture.as_deref(), Some("apc40-mkii tempo"), "{command:?}");
    fake.launcher_state_tempo(&[], 121.0);
    fake.message_at(&[0xB0, 13, 127], 2);
    assert!((fake.last_command().bpm.unwrap_or_default() - 120.0).abs() < 1e-6, "{:?}", fake.last_command());
    fake.launcher_state_tempo(&[], 120.0);
    fake.press(101);
    assert!((fake.last_command().bpm.unwrap_or_default() - 121.0).abs() < 1e-6, "{:?}", fake.last_command());
    fake.launcher_state_tempo(&[], 121.0);
    fake.press(100);
    assert!((fake.last_command().bpm.unwrap_or_default() - 120.0).abs() < 1e-6, "{:?}", fake.last_command());
    // Two taps half a second apart are 120 BPM.
    fake.message_at(&[0x90, 99, 127], 1_000_000);
    fake.message_at(&[0x90, 99, 127], 1_500_000);
    let command = fake.last_command();
    assert_eq!(command.id, "transport.setTempo", "{command:?}");
    assert!((command.bpm.unwrap_or_default() - 120.0).abs() < 1e-6, "{command:?}");
}

/// The cue level is a relative knob: a dB a click, silence at its bottom and +12 at its top.
#[test]
fn the_cue_knob_steps_the_cue_volume() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.audio_info(Some(0.0));
    fake.message(&[0xB0, 47, 1]);
    let command = fake.last_command();
    assert_eq!(command.id, "output.setCueVolume", "{command:?}");
    assert!((command.db.unwrap_or_default() - 1.0).abs() < 1e-6, "{command:?}");
    fake.audio_info(Some(-6.0));
    fake.message(&[0xB0, 47, 127]);
    let command = fake.last_command();
    assert!((command.db.unwrap_or_default() + 7.0).abs() < 1e-6, "{command:?}");
    fake.audio_info(Some(12.0));
    fake.message(&[0xB0, 47, 1]);
    assert!((fake.last_command().db.unwrap_or_default() - 12.0).abs() < 1e-6, "{:?}", fake.last_command());
    // Silence climbs one step to -59 dB.
    fake.audio_info(None);
    fake.message(&[0xB0, 47, 1]);
    assert!((fake.last_command().db.unwrap_or_default() + 59.0).abs() < 1e-6, "{:?}", fake.last_command());
}

/// The four Bank Select arrows move the 5x8 window over the launcher: scenes up and down,
/// tracks left and right.
#[test]
fn the_bank_select_arrows_move_the_window() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.launcher_grid_big(&[(10, "A"), (11, "B"), (12, "C"), (13, "D"), (14, "E"), (15, "F")]);
    fake.press(82);
    assert_eq!(fake.last_command().scene, Some(10), "{:?}", fake.last_command());
    fake.press(95);
    fake.press(82);
    assert_eq!(fake.last_command().scene, Some(11), "{:?}", fake.last_command());
    fake.press(94);
    fake.press(82);
    assert_eq!(fake.last_command().scene, Some(10), "{:?}", fake.last_command());
    fake.press(96);
    fake.press(32);
    let command = fake.last_command();
    assert_eq!(command.id, "clip.launch", "{command:?}");
    assert_eq!(command.track, Some(2), "{command:?}");
    fake.press(97);
    fake.press(32);
    assert_eq!(fake.last_command().track, Some(1), "{:?}", fake.last_command());
}

/// Device Lock keeps the device section on its device while the selection moves; without it
/// the device buttons follow the selection.
#[test]
fn the_device_buttons_follow_the_device_lock() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.selection(4, 1, 0);
    fake.press(62);
    let command = fake.last_command();
    assert_eq!(command.id, "device.setEnabled", "{command:?}");
    assert_eq!(command.track, Some(1), "{command:?}");
    fake.press(63);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 63, 127].as_slice()),
        "Device Lock lights: {:?}",
        fake.sends.borrow()
    );
    fake.selection(4, 2, 1);
    fake.press(62);
    let command = fake.last_command();
    assert_eq!(command.track, Some(1), "the locked track: {command:?}");
    assert_eq!(command.device.as_deref(), Some("0"), "{command:?}");
    // Unlocked, the same press follows the selection again.
    fake.press(63);
    fake.press(62);
    let command = fake.last_command();
    assert_eq!(command.track, Some(2), "{command:?}");
    assert_eq!(command.device.as_deref(), Some("1"), "{command:?}");
    // Clip / Devices and Detail View ask the window's own actions.
    fake.press(64);
    assert_eq!(fake.last_command().id, "view.devices", "{:?}", fake.commands());
    fake.press(65);
    assert_eq!(fake.last_command().id, "view.detail", "{:?}", fake.commands());
}

/// Bank Lock keeps the bank across a device change; without it another device starts at its
/// first bank.
#[test]
fn the_bank_lock_holds_the_parameter_bank() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.selection(4, 1, 0);
    fake.press(61);
    fake.press(103);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 103, 127].as_slice()),
        "Bank Lock lights: {:?}",
        fake.sends.borrow()
    );
    fake.selection(4, 2, 1);
    fake.knob(0, 127);
    assert_eq!(fake.last_command().param.as_deref(), Some("dial9"), "{:?}", fake.last_command());
    // Unlocked, the next device's bank starts at its first dial.
    fake.press(103);
    fake.selection(4, 2, 0);
    fake.knob(0, 127);
    assert_eq!(fake.last_command().param.as_deref(), Some("dial6"), "{:?}", fake.last_command());
}

/// Held Shift makes the Bank Select arrows jump five scenes and eight tracks.
#[test]
fn shift_makes_the_bank_select_arrows_jump() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    let scenes: Vec<(u64, &str)> = (0..11).map(|i| (10 + i, "Scene")).collect();
    fake.launcher_grid_huge(&scenes);
    fake.press(98);
    fake.press(95);
    fake.press(82);
    assert_eq!(fake.last_command().scene, Some(15), "five scenes down: {:?}", fake.last_command());
    fake.press(94);
    fake.press(82);
    assert_eq!(fake.last_command().scene, Some(10), "five scenes up: {:?}", fake.last_command());
    fake.press(96);
    fake.press(32);
    assert_eq!(fake.last_command().track, Some(9), "eight tracks right: {:?}", fake.last_command());
    fake.message(&[0x80, 98, 0]);
    fake.press(97);
    fake.press(32);
    assert_eq!(fake.last_command().track, Some(8), "one track left after Shift: {:?}", fake.last_command());
}

/// CC 15 sets the crossfader, and a track's note 66 button toggles A and B; the strip reading
/// lights it yellow on A and red on B.
#[test]
fn the_crossfader_and_the_ab_button_drive_their_commands() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.selection(4, 1, 0);
    fake.strips_assigns(&[(7, "off")]);
    fake.message(&[0xB0, 15, 127]);
    let command = fake.last_command();
    assert_eq!(command.id, "mixer.setCrossfader", "{command:?}");
    assert!((command.value.unwrap_or_default() - 1.0).abs() < 1e-6, "{command:?}");
    assert_eq!(command.gesture.as_deref(), Some("apc40-mkii crossfader"), "{command:?}");
    // The A/B button on the first track's channel (0) turns off into A.
    fake.press(66);
    let command = fake.last_command();
    assert_eq!(command.id, "track.setCrossfadeAssign", "{command:?}");
    assert_eq!(command.assign.as_deref(), Some("a"), "{command:?}");
    // The reading says A: the button burns yellow. Then A turns into B, red.
    fake.strips_assigns(&[(7, "a")]);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 66, 1].as_slice()),
        "A lights yellow: {:?}",
        fake.sends.borrow()
    );
    fake.press(66);
    assert_eq!(fake.last_command().assign.as_deref(), Some("b"));
    fake.strips_assigns(&[(7, "b")]);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 66, 2].as_slice()),
        "B lights red: {:?}",
        fake.sends.borrow()
    );
    // B turns off, and the LED goes dark; off starts again at A.
    fake.press(66);
    assert_eq!(fake.last_command().assign.as_deref(), Some("off"));
    fake.strips_assigns(&[(7, "off")]);
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.as_slice() == [0x90, 66, 0].as_slice()),
        "off is dark: {:?}",
        fake.sends.borrow()
    );
    fake.press(66);
    assert_eq!(fake.last_command().assign.as_deref(), Some("a"));
}

/// The alternate Live mode hands every LED to the host, so a knob's ring follows its parameter
/// without the device's own ring updates fighting it.
#[test]
fn the_program_uses_the_live_mode() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    let mode = [0xF0, 0x47, 0x7F, 0x29, 0x60, 0x00, 0x04, 0x41];
    assert!(
        fake.sends.borrow().iter().any(|(_, bytes)| bytes.starts_with(&mode)),
        "the alternate Live mode is sent: {:?}",
        fake.sends.borrow()
    );
}

#[test]
fn a_knobs_own_report_is_not_written_back_to_its_ring() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.selection(4, 1, 0);
    // The gain is the panel's second dial: its knob reports 64, and the session's gain trails
    // with the same position on that knob's ring.
    fake.knob(1, 64);
    let after_knob = fake.sends.borrow().len();
    let gain = -150.0 + (64.0 / 127.0) * 185.0;
    // The turn has to settle before a reading can write; the knob's own position never is.
    for _ in 0..12 {
        fake.selection_with_gain(4, 1, 0, gain);
    }
    assert!(
        !fake.sends.borrow()[after_knob..].iter().any(|(_, bytes)| bytes.as_slice() == [0xB0, 17, 64].as_slice()),
        "the knob's own ring is not yanked: {:?}",
        &fake.sends.borrow()[after_knob..]
    );
    // A value the knob never reported — the window set the gain to its maximum — writes.
    fake.selection_with_gain(4, 1, 0, 35.0);
    assert!(
        fake.sends.borrow()[after_knob..].iter().any(|(_, bytes)| bytes.as_slice() == [0xB0, 17, 127].as_slice()),
        "a change from elsewhere writes the ring: {:?}",
        &fake.sends.borrow()[after_knob..]
    );
}

#[test]
fn the_knobs_ring_shows_the_parameters_position() {
    let fake = Fake::new(include_str!("../../../packs/apc40-mkii/script.lua"));
    fake.ports();
    fake.selection(4, 1, 0);
    // The first knob's ring: the single-dot style, then the gain's position (0 dB of -150..35,
    // about 109 of 127).
    let sends = fake.sends.borrow();
    let cc = |controller: u8, value: u8| {
        sends.iter().any(|(_, bytes)| bytes.as_slice() == [0xB0, controller, value].as_slice())
    };
    assert!(cc(24, 1), "the first knob's ring style: {:?}", *sends);
    assert!(cc(16, 0), "the first dial's position: {:?}", *sends);
    assert!(cc(17, 103), "the second dial, Gain, at 0 dB: {:?}", *sends);
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.to_string_lossy().to_string()),
        _ => None,
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(n) => Some(*n as f64),
        Value::Number(n) => Some(*n),
        _ => None,
    }
}
