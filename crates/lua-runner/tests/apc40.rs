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
    track_name: Option<String>,
    device: Option<String>,
    param: Option<String>,
    value: Option<f64>,
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
                    let device = field("device").and_then(|v| match v {
                        Value::Integer(n) => Some(n.to_string()),
                        Value::Number(n) => Some(format!("{n:?}")),
                        Value::String(s) => Some(s.to_string_lossy().to_string()),
                        _ => None,
                    });
                    recorded.borrow_mut().push(Command {
                        id,
                        track: count("track"),
                        track_name: field("track").and_then(|v| text(&v)),
                        device,
                        param: text_field("param"),
                        value: field("value").and_then(|v| number(&v)),
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
        let selection = self.lua.create_table().unwrap();
        selection.set("track", track).unwrap();
        selection.set("track_name", "Drums").unwrap();
        let device = self.lua.create_table().unwrap();
        device.set("path", path).unwrap();
        device.set("enabled", true).unwrap();
        // The Utility's parameters in the order its panel lists them: the first bank's eight,
        // then Invert Left (the ninth) and the rest.
        let params = [
            ("gain", -150.0, 35.0, 0.0),
            ("balance", -1.0, 1.0, 0.0),
            ("width", 0.0, 4.0, 1.0),
            ("midSide", -1.0, 1.0, 0.0),
            ("mono", 0.0, 1.0, 0.0),
            ("bassMono", 0.0, 1.0, 0.0),
            ("bassMonoFrequency", 50.0, 500.0, 120.0),
            ("invertLeft", 0.0, 1.0, 0.0),
            ("invertRight", 0.0, 1.0, 0.0),
            ("channel", 0.0, 3.0, 0.0),
            ("dcFilter", 0.0, 1.0, 0.0),
            ("mute", 0.0, 1.0, 0.0),
            ("bassMonoListen", 0.0, 1.0, 0.0),
        ];
        let list = self.lua.create_table().unwrap();
        for (index, (id, min, max, value)) in params.iter().enumerate() {
            let param = self.lua.create_table().unwrap();
            param.set("id", *id).unwrap();
            param.set("min", *min).unwrap();
            param.set("max", *max).unwrap();
            param.set("value", *value).unwrap();
            param.set("unit", "").unwrap();
            list.set(index + 1, param).unwrap();
        }
        device.set("params", list).unwrap();
        selection.set("device", device).unwrap();
        let handler: mlua::Function = self.lua.named_registry_value("on_state").unwrap();
        handler.call::<()>((token, selection)).unwrap();
    }

    /// One port message: CC 16 or a device-section note.
    fn message(&self, bytes: &[u8]) {
        let handler: mlua::Function = self.lua.named_registry_value("on_port_message").unwrap();
        let bytes = self.lua.create_string(bytes).unwrap();
        handler.call::<()>((bytes, "APC40 mkII in", 0u64)).unwrap();
    }

    /// The tracks the project's subscription lists, so the track select buttons have ids.
    fn strips(&self, ids: &[u32]) {
        let strips = self.lua.create_table().unwrap();
        for (index, id) in ids.iter().enumerate() {
            let strip = self.lua.create_table().unwrap();
            strip.set("id", *id).unwrap();
            strip.set("pan", 0.0).unwrap();
            strips.set(index + 1, strip).unwrap();
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

    fn knob(&self, value: u8) {
        self.message(&[0xB0, 16, value]);
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

    // The first knob, full: the Utility's Gain at its maximum, in dB.
    fake.knob(127);
    let command = fake.last_command();
    assert_eq!(command.id, "device.setParam", "{command:?}");
    assert_eq!(command.track, Some(1), "{command:?}");
    assert_eq!(command.device.as_deref(), Some("0"), "{command:?}");
    assert_eq!(command.param.as_deref(), Some("gain"), "{command:?}");
    assert!((command.value.unwrap_or_default() - 35.0).abs() < 1e-6, "{command:?}");
    assert_eq!(command.gesture.as_deref(), Some("apc40-mkii device 1:0:gain"), "{command:?}");

    // Bank right, then the same knob: the ninth parameter, Invert Right.
    fake.press(61);
    fake.knob(127);
    let command = fake.last_command();
    assert_eq!(command.param.as_deref(), Some("invertRight"), "{command:?}");
    assert!((command.value.unwrap_or_default() - 1.0).abs() < 1e-6, "{command:?}");

    // Bank left, then the first knob at zero: Gain at its minimum.
    fake.press(60);
    fake.knob(0);
    let command = fake.last_command();
    assert_eq!(command.param.as_deref(), Some("gain"), "{command:?}");
    assert!(command.value.unwrap_or_default() < -100.0, "{command:?}");
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
    assert!(cc(16, 103), "the first knob's position: {:?}", *sends);
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
