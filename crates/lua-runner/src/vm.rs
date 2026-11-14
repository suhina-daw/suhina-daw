// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The runner's Lua 5.4 environment ([`docs/spec/surface-programs.md`](../../../docs/spec/surface-programs.md)
//! › The runner): the sandbox narrowed to what the surface-program protocol allows, the
//! conversions between Lua values and the protocol's JSON, and the `suhina.midi` helpers.
//!
//! Removed from the globals: `io`, `os`, `dofile`, `loadfile`, `package` and with it
//! `require`; `load` accepts text chunks only, and its chunk must be a string. The narrowing is
//! the runner's convenience — the boundary that is enforced is the process (ADR 0015 › Trust) —
//! but every removal is tested.

use serde_json::Value;
use suhina_plugins::surface::MAX_DEPTH;

use mlua::{Function, LightUserData, Lua, Table, Value as LuaValue};

/// The name the null sentinel is kept under in the registry; `suhina.null` is it, and a JSON
/// null arrives as it, so an array keeps its places.
/// The registry key the null sentinel lives under; `suhina.null` is it.
pub const NULL_KEY: &str = "suhina.null";
/// The pointed-at byte gives the null light userdata a stable, comparable address.
static NULL_BYTE: u8 = 0;

/// A fresh, sandboxed Lua state: the narrowed globals and nothing else changed on them.
pub fn environment() -> mlua::Result<Lua> {
    let lua = Lua::new();
    lua.set_named_registry_value(NULL_KEY, LuaValue::LightUserData(null_pointer()))?;
    let globals = lua.globals();
    for name in ["io", "os", "dofile", "loadfile", "package", "require"] {
        globals.raw_set(name, mlua::Nil)?;
    }
    // `load` reads text chunks only: the chunk must be a string, a mode that names binary is
    // refused, and the original load is always called with the mode "t".
    let real_load: Function = globals.get("load")?;
    globals.set(
        "load",
        lua.create_function(move |lua: &Lua, (chunk, name, mode, env): (LuaValue, LuaValue, LuaValue, LuaValue)| {
            let LuaValue::String(chunk) = chunk else {
                return Err(mlua::Error::runtime("load reads text chunks only: the chunk must be a string"));
            };
            // Any chunk starting with the binary signature is binary, as Lua itself says.
            if chunk.as_bytes().first() == Some(&0x1b) {
                return Err(mlua::Error::runtime("load reads text chunks only"));
            }
            if matches!(&mode, LuaValue::String(m) if m.as_bytes().contains(&b'b')) {
                return Err(mlua::Error::runtime("load reads text chunks only"));
            }
            let env = match env {
                LuaValue::Nil | LuaValue::Table(_) => env,
                _ => return Err(mlua::Error::runtime("load's fourth argument must be a table or nil")),
            };
            let text_only = lua.create_string("t")?;
            real_load.call::<LuaValue>((chunk, name, text_only, env))
        })?,
    )?;
    Ok(lua)
}

fn null_pointer() -> LightUserData {
    LightUserData(&NULL_BYTE as *const u8 as *mut core::ffi::c_void)
}

/// A protocol JSON value as Lua: an object a table, an array a sequence, null the null
/// sentinel. Nesting past the protocol's depth is an error.
pub fn json_to_lua(lua: &Lua, value: &Value) -> mlua::Result<LuaValue> {
    json_to_lua_at(lua, value, 0)
}

fn json_to_lua_at(lua: &Lua, value: &Value, depth: usize) -> mlua::Result<LuaValue> {
    if depth > MAX_DEPTH {
        return Err(mlua::Error::runtime("the value is nested deeper than the protocol allows"));
    }
    Ok(match value {
        Value::Null => LuaValue::LightUserData(null_pointer()),
        Value::Bool(b) => LuaValue::Boolean(*b),
        Value::Number(n) => number_to_lua(n.clone())?,
        Value::String(s) => LuaValue::String(lua.create_string(s)?),
        Value::Array(list) => {
            let table = lua.create_table()?;
            for (index, item) in list.iter().enumerate() {
                table.raw_set(index as i64 + 1, json_to_lua_at(lua, item, depth + 1)?)?;
            }
            LuaValue::Table(table)
        }
        Value::Object(map) => {
            let table = lua.create_table()?;
            for (key, item) in map {
                table.raw_set(key.as_str(), json_to_lua_at(lua, item, depth + 1)?)?;
            }
            LuaValue::Table(table)
        }
    })
}

/// A number of the protocol as a Lua value: an integer where it is one, and refused when it is
/// not finite or too large to survive the trip as a number.
fn number_to_lua(number: serde_json::Number) -> mlua::Result<LuaValue> {
    if let Some(i) = number.as_i64() {
        Ok(LuaValue::Integer(i))
    } else if let Some(i) = number.as_u64() {
        // Past i64's range: kept as a number, refused when the trip would not be exact.
        let as_f64 = i as f64;
        if as_f64 as u64 == i {
            Ok(LuaValue::Number(as_f64))
        } else {
            Err(mlua::Error::runtime("the number is too large to carry exactly"))
        }
    } else {
        number.as_f64().map(LuaValue::Number).ok_or_else(|| mlua::Error::runtime("the number is not finite"))
    }
}

/// A Lua value as protocol JSON: the inverse of [`json_to_lua`], with a table an array when it
/// is exactly the sequence 1..n and an object otherwise. `nil` is null, as an absent parameter
/// is. A script's own value, so an error names what a script wrote.
pub fn lua_to_json(value: &LuaValue) -> Result<Value, String> {
    lua_to_json_at(value, 0)
}

fn lua_to_json_at(value: &LuaValue, depth: usize) -> Result<Value, String> {
    if depth > MAX_DEPTH {
        return Err("the table is nested deeper than the protocol allows".into());
    }
    Ok(match value {
        LuaValue::Nil => Value::Null,
        LuaValue::Boolean(b) => Value::Bool(*b),
        LuaValue::Integer(i) => Value::Number((*i).into()),
        LuaValue::Number(n) => Value::Number(serde_json::Number::from_f64(*n).ok_or("the number is not finite")?),
        LuaValue::String(s) => {
            let bytes = s.as_bytes();
            let s = std::str::from_utf8(&bytes).map_err(|_| "the string is not UTF-8 text")?;
            Value::String(s.to_string())
        }
        LuaValue::LightUserData(p) if p.0 == null_pointer().0 => Value::Null,
        LuaValue::Table(table) => table_to_json(table, depth)?,
        _ => return Err("the value is not one the protocol carries (a function, a thread, userdata)".into()),
    })
}

/// A table as an array when its keys are integers within 1..max and nothing else, as an object
/// when they are exactly strings, and refused when it is mixed. A hole in a sequence is null,
/// so places are kept.
fn table_to_json(table: &Table, depth: usize) -> Result<Value, String> {
    let mut sequence: Vec<(i64, Value)> = Vec::new();
    let mut object = serde_json::Map::new();
    for entry in table.clone().pairs::<LuaValue, LuaValue>() {
        let (key, value) = entry.map_err(|e| e.to_string())?;
        let value = lua_to_json_at(&value, depth + 1)?;
        match key {
            LuaValue::Integer(index) if index >= 1 => sequence.push((index, value)),
            LuaValue::String(key) => {
                let key = key.to_str().map_err(|_| "a key is not UTF-8 text")?;
                object.insert(key.to_string(), value);
            }
            _ => return Err("a table is either an array or an object with string keys".into()),
        }
    }
    if !object.is_empty() && !sequence.is_empty() {
        return Err("a table is either an array or an object with string keys".into());
    }
    if object.is_empty() {
        let mut ordered = vec![Value::Null; sequence.iter().map(|(index, _)| *index).max().unwrap_or(0) as usize];
        for (index, value) in sequence {
            ordered[index as usize - 1] = value;
        }
        return Ok(Value::Array(ordered));
    }
    Ok(Value::Object(object))
}

/// The `suhina.midi` table: builders for the bytes the protocol carries, and `parse`.
pub fn midi_table(lua: &Lua) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.raw_set(
        "note_on",
        lua.create_function(|lua, (channel, note, velocity): (i64, i64, i64)| {
            bytes(lua, &[0x90 | channel_byte(channel)?, data_byte(note)?, data_byte(velocity)?])
        })?,
    )?;
    table.raw_set(
        "note_off",
        lua.create_function(|lua, (channel, note, velocity): (i64, i64, i64)| {
            bytes(lua, &[0x80 | channel_byte(channel)?, data_byte(note)?, data_byte(velocity)?])
        })?,
    )?;
    table.raw_set(
        "cc",
        lua.create_function(|lua, (channel, controller, value): (i64, i64, i64)| {
            bytes(lua, &[0xB0 | channel_byte(channel)?, data_byte(controller)?, data_byte(value)?])
        })?,
    )?;
    table.raw_set(
        "program",
        lua.create_function(|lua, (channel, program): (i64, i64)| {
            bytes(lua, &[0xC0 | channel_byte(channel)?, data_byte(program)?])
        })?,
    )?;
    table.raw_set(
        "pitch_bend",
        lua.create_function(|lua, (channel, value): (i64, i64)| {
            let value = bend_value(value)?;
            bytes(lua, &[0xE0 | channel_byte(channel)?, (value & 0x7f) as u8, (value >> 7) as u8])
        })?,
    )?;
    table.raw_set(
        "sysex",
        lua.create_function(|lua, data: mlua::LuaString| {
            let data_bytes = &data.as_bytes()[..];
            if data_bytes.iter().any(|b| *b >= 0x80) {
                return Err(mlua::Error::runtime("a system exclusive's data bytes are 7-bit"));
            }
            let mut message = Vec::with_capacity(data_bytes.len() + 2);
            message.push(0xf0);
            message.extend_from_slice(data_bytes);
            message.push(0xf7);
            bytes(lua, &message)
        })?,
    )?;
    table.raw_set("parse", lua.create_function(|lua, message: mlua::LuaString| parse(lua, &message.as_bytes()))?)?;
    Ok(table)
}

/// A raw message as a Lua string; the length is within the protocol's cap by construction.
fn bytes(lua: &Lua, message: &[u8]) -> mlua::Result<mlua::LuaString> {
    lua.create_string(message)
}

fn channel_byte(channel: i64) -> mlua::Result<u8> {
    u8::try_from(channel).ok().filter(|b| *b <= 15).ok_or_else(|| mlua::Error::runtime("the channel is 0 to 15"))
}

fn data_byte(value: i64) -> mlua::Result<u8> {
    u8::try_from(value).ok().filter(|b| *b <= 127).ok_or_else(|| mlua::Error::runtime("the data byte is 0 to 127"))
}

fn bend_value(value: i64) -> mlua::Result<i64> {
    (0..=16383)
        .contains(&value)
        .then_some(value)
        .ok_or_else(|| mlua::Error::runtime("the pitch bend value is 0 to 16383, centre 8192"))
}

/// One MIDI message as a Lua table (`kind`, `channel`, its fields), or `nil` when the bytes
/// are not one complete message the runner names. Running status is not resolved: `parse` is
/// for whole messages, as `port_message` carries them.
fn parse(lua: &Lua, message: &[u8]) -> mlua::Result<Option<LuaValue>> {
    let Some(status) = message.first().copied() else { return Ok(None) };
    let channel = i64::from(status & 0x0f);
    let table = |kind: &str, fields: &[(&str, i64)]| -> mlua::Result<LuaValue> {
        let table = lua.create_table()?;
        table.raw_set("kind", kind)?;
        table.raw_set("channel", channel)?;
        for (key, value) in fields {
            table.raw_set(*key, *value)?;
        }
        Ok(LuaValue::Table(table))
    };
    let parsed = match status {
        0x80..=0x8f if message.len() == 3 => {
            table("note_off", &[("note", i64::from(message[1])), ("velocity", i64::from(message[2]))])?
        }
        0x90..=0x9f if message.len() == 3 => {
            table("note_on", &[("note", i64::from(message[1])), ("velocity", i64::from(message[2]))])?
        }
        0xa0..=0xaf if message.len() == 3 => {
            table("poly_pressure", &[("note", i64::from(message[1])), ("value", i64::from(message[2]))])?
        }
        0xb0..=0xbf if message.len() == 3 => {
            table("cc", &[("controller", i64::from(message[1])), ("value", i64::from(message[2]))])?
        }
        0xc0..=0xcf if message.len() == 2 => table("program", &[("program", i64::from(message[1]))])?,
        0xd0..=0xdf if message.len() == 2 => table("channel_pressure", &[("value", i64::from(message[1]))])?,
        0xe0..=0xef if message.len() == 3 => {
            let value = i64::from(message[1]) | (i64::from(message[2]) << 7);
            table("pitch_bend", &[("value", value)])?
        }
        0xf0 if message.len() >= 2 && message[message.len() - 1] == 0xf7 => {
            let data = &message[1..message.len() - 1];
            let table = lua.create_table()?;
            table.raw_set("kind", "sysex")?;
            table.raw_set("data", lua.create_string(data)?)?;
            LuaValue::Table(table)
        }
        _ => return Ok(None),
    };
    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions
    use super::*;
    use serde_json::json;

    fn setup() -> Lua {
        environment().unwrap()
    }

    #[test]
    fn the_removed_globals_are_gone() {
        let lua = setup();
        let globals = lua.globals();
        for name in ["io", "os", "dofile", "loadfile", "package", "require"] {
            let gone: Option<bool> = globals.get(name).unwrap();
            assert_eq!(gone, None, "{name} is removed");
        }
        // The rest of the standard library stays.
        let pairs: LuaValue = globals.get("pairs").unwrap();
        assert!(matches!(pairs, LuaValue::Function(_)));
    }

    #[test]
    fn load_reads_text_chunks_only() {
        let lua = setup();
        let load: Function = lua.globals().get("load").unwrap();
        // A text chunk loads and runs.
        let made: Function = load.call::<Function>("return 1 + 1").unwrap();
        assert_eq!(made.call::<i64>(()).unwrap(), 2);
        // A chunk that is not a string is refused, function reader included.
        let error = load.call::<mlua::Value>(lua.create_function(|_, ()| Ok(mlua::Value::Nil)).unwrap()).unwrap_err();
        assert!(error.to_string().contains("text chunks only"), "{error}");
        // A binary chunk (Lua 5.4's signature byte) is refused, mode asked for or not.
        let binary = "\x1bLua\x55\x00\x00\x01".to_string();
        let error = load.call::<mlua::Value>(binary.clone()).unwrap_err();
        assert!(error.to_string().contains("text chunks only"), "{error}");
        let error = load.call::<mlua::Value>((binary, "b")).unwrap_err();
        assert!(error.to_string().contains("text chunks only"), "{error}");
        // A mode that names binary is refused even over a text chunk.
        let error = load.call::<mlua::Value>(("return 1", mlua::Value::Nil, "bt")).unwrap_err();
        assert!(error.to_string().contains("text chunks only"), "{error}");
    }

    #[test]
    fn json_comes_to_lua_and_back() {
        let lua = setup();
        let value = json!({ "a": [1, null, "two", { "b": false }], "c": -3 });
        let as_lua = json_to_lua(&lua, &value).unwrap();
        // An array keeps its places: null is the sentinel, not nil.
        let LuaValue::Table(ref table) = as_lua else { panic!("a table") };
        let inner: LuaValue = table.get("a").unwrap();
        let LuaValue::Table(ref inner) = inner else { panic!("a table") };
        assert_eq!(inner.raw_len(), 4);
        let middle: LuaValue = inner.get(2).unwrap();
        assert!(matches!(middle, LuaValue::LightUserData(_)), "null is the sentinel, not nil");
        // And back, unchanged.
        let back = lua_to_json(&as_lua).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn lua_tables_go_back_as_arrays_or_objects() {
        let lua = setup();
        // A sequence 1..n is an array, holes null.
        let script = "return {1, nil, 3}";
        let value: LuaValue = lua.load(script).eval().unwrap();
        assert_eq!(lua_to_json(&value).unwrap(), json!([1, null, 3]));
        // A string-keyed table is an object.
        let script = "return { volume = { dB = -3.5 }, on = true }";
        let value: LuaValue = lua.load(script).eval().unwrap();
        assert_eq!(lua_to_json(&value).unwrap(), json!({ "volume": { "dB": -3.5 }, "on": true }));
        // Mixed is refused, and so are the values the protocol does not carry.
        let script = "return { 1, x = 2 }";
        let value: LuaValue = lua.load(script).eval().unwrap();
        assert!(lua_to_json(&value).is_err(), "mixed array and object");
        let script = "return { print }";
        let value: LuaValue = lua.load(script).eval().unwrap();
        assert!(lua_to_json(&value).is_err(), "a function is not a protocol value");
        // nil is null, as an absent parameter is.
        assert_eq!(lua_to_json(&LuaValue::Nil).unwrap(), Value::Null);
        // Past the protocol's depth, refused both ways.
        let script = "local t = 5; for i = 1, 40 do t = { t } end; return t";
        let value: LuaValue = lua.load(script).eval().unwrap();
        assert!(lua_to_json(&value).is_err(), "nested too deep");
        let deep = json!({ "in": json!({ "in": json!([1]) }) });
        assert!(json_to_lua(&lua, &deep).is_ok(), "within the depth is fine");
    }

    #[test]
    fn the_midi_builders_make_the_bytes() {
        let lua = setup();
        let midi: Table = midi_table(&lua).unwrap();
        fn build(midi: &Table, name: &str, args: impl mlua::IntoLuaMulti) -> mlua::LuaString {
            let make: Function = midi.get(name).unwrap();
            make.call(args).unwrap()
        }
        assert_eq!(build(&midi, "note_on", (0, 60, 100)).as_bytes(), &[0x90, 0x3c, 0x64]);
        assert_eq!(build(&midi, "note_on", (15, 0, 127)).as_bytes(), &[0x9f, 0x00, 0x7f]);
        assert_eq!(build(&midi, "note_off", (1, 60, 0)).as_bytes(), &[0x81, 0x3c, 0x00]);
        assert_eq!(build(&midi, "cc", (2, 7, 100)).as_bytes(), &[0xb2, 0x07, 0x64]);
        assert_eq!(build(&midi, "program", (3, 88)).as_bytes(), &[0xc3, 0x58]);
        assert_eq!(build(&midi, "pitch_bend", (0, 8192)).as_bytes(), &[0xe0, 0x00, 0x40]);
        assert_eq!(build(&midi, "pitch_bend", (5, 16383)).as_bytes(), &[0xe5, 0x7f, 0x7f]);
        assert_eq!(
            build(&midi, "sysex", lua.create_string(&b"\x7e\x7f\x09"[..]).unwrap()).as_bytes(),
            &[0xf0, 0x7e, 0x7f, 0x09, 0xf7]
        );
        // Out of range is an error, never a wrapped byte.
        let make: Function = midi.get("note_on").unwrap();
        assert!(make.call::<mlua::LuaString>((16, 60, 100)).is_err(), "channel 16");
        assert!(make.call::<mlua::LuaString>((0, 128, 100)).is_err(), "note 128");
        assert!(make.call::<mlua::LuaString>((0.5, 60, 100)).is_err(), "a fractional note");
        // Sysex data bytes are 7-bit.
        let make: Function = midi.get("sysex").unwrap();
        assert!(make.call::<mlua::LuaString>(lua.create_string([0xf0u8]).unwrap()).is_err());
    }

    #[test]
    fn parse_names_whole_messages() {
        let lua = setup();
        let midi: Table = midi_table(&lua).unwrap();
        let parse: Function = midi.get("parse").unwrap();
        let note_on: Table =
            parse.call::<Option<Table>>(lua.create_string([0x91u8, 0x3c, 0x40]).unwrap()).unwrap().unwrap();
        assert_eq!(note_on.get::<String>("kind").unwrap(), "note_on");
        assert_eq!(note_on.get::<i64>("channel").unwrap(), 1);
        assert_eq!(note_on.get::<i64>("note").unwrap(), 0x3c);
        assert_eq!(note_on.get::<i64>("velocity").unwrap(), 0x40);
        let bend: Table =
            parse.call::<Option<Table>>(lua.create_string([0xe0u8, 0x00, 0x40]).unwrap()).unwrap().unwrap();
        assert_eq!(bend.get::<i64>("value").unwrap(), 8192);
        let sysex: Table =
            parse.call::<Option<Table>>(lua.create_string([0xf0u8, 0x7e, 0xf7]).unwrap()).unwrap().unwrap();
        assert_eq!(sysex.get::<String>("kind").unwrap(), "sysex");
        assert_eq!(sysex.get::<mlua::LuaString>("data").unwrap().as_bytes(), &[0x7e]);
        // Not a whole message the runner names: nil, never a guess.
        assert_eq!(parse.call::<Option<Table>>(lua.create_string([0xb0u8, 0x07]).unwrap()).unwrap(), None);
        assert_eq!(parse.call::<Option<Table>>(lua.create_string([0x00u8]).unwrap()).unwrap(), None);
        assert_eq!(parse.call::<Option<Table>>(lua.create_string([]).unwrap()).unwrap(), None);
    }
}
