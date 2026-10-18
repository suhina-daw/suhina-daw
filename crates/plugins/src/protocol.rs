// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control messages between the engine and a plug-in host process (ADR 0006): one JSON
//! object per line over a local socket. Audio does not go here; it goes through `suhina-ipc`.
//!
//! Every message has an `op`. Engine to host: `hello`, `load`, `unload`, `ping`, `get_state`,
//! `set_state`, `get_params`, `set_param`, `open_editor`, `place_editor`, `close_editor`,
//! `quit`. Host to engine: `hello`, `loaded`, `unloaded`, `pong`, `state`, `params`, `param`,
//! `editor_opened`, `editor_closed`, `editor_resized`, `latency_changed`, `memory`, `error`.
//! Every message is answered, except `place_editor` and `quit` (so the engine never waits on
//! them). `editor_closed` also comes unasked, when the user closes an editor's window;
//! `editor_resized` only comes unasked, when a plug-in gives its editor a new size,
//! `latency_changed` only comes unasked, when a running plug-in reports another latency, and
//! `memory` only comes unasked, once a second.
//!
//! One host process runs several plug-in instances. The engine names each with an `instance`
//! number when it loads it; every message about one instance, and its reply, carries that
//! number. `hello`, `ping`/`pong` and `quit` are about the whole process. An `error` carries the
//! instance of the request it answers, or none for one about the process.

use serde_json::{Value, json};

/// Bumped whenever a message changes; both sides refuse a different version.
pub const VERSION: u64 = 9;
/// Longest line either side accepts; state blobs are hex, so this bounds them at 32 MiB.
pub const MAX_LINE: usize = 64 * 1024 * 1024;

/// Most parameters a plug-in's list carries.
pub const MAX_PARAMS: usize = 1 << 16;

/// One of a plug-in's parameters. Values are in the plug-in's own range, `min` to `max`: a
/// VST3 plug-in's are normalized (0 to 1), a CLAP plug-in's are its plain values.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    /// Where the plug-in groups it (a CLAP module path, a VST3 unit), or empty.
    pub group: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    /// The value as the plug-in shows it ("-3.2 dB"), or empty.
    pub text: String,
    /// Number of steps for a stepped parameter (a switch has 1), 0 for a continuous one.
    pub steps: u32,
    pub automatable: bool,
    pub read_only: bool,
    pub hidden: bool,
}

impl ParamInfo {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "group": self.group, "min": self.min, "max": self.max,
            "default": self.default, "value": self.value, "text": self.text, "steps": self.steps,
            "automatable": self.automatable, "read_only": self.read_only, "hidden": self.hidden,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| value.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
        let flag = |key: &str| value.get(key).and_then(Value::as_bool);
        Some(Self {
            id: u32::try_from(value.get("id")?.as_u64()?).ok()?,
            name: text("name")?,
            group: text("group")?,
            min: number("min")?,
            max: number("max")?,
            default: number("default")?,
            value: number("value")?,
            text: text("text")?,
            steps: u32::try_from(value.get("steps")?.as_u64()?).ok()?,
            automatable: flag("automatable")?,
            read_only: flag("read_only")?,
            hidden: flag("hidden")?,
        })
    }
}

/// Where an embedded editor sits in the window it is a child of: its origin, in the window's
/// own pixels from its top left (negative when the device panel is scrolled past it), and the
/// part of the window that shows it (the panel's visible area): what falls outside it is cut
/// off. An empty clip hides the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub clip_x: i32,
    pub clip_y: i32,
    pub clip_width: u32,
    pub clip_height: u32,
}

impl Placement {
    /// Nowhere: the editor is hidden.
    pub const HIDDEN: Self = Self { x: 0, y: 0, clip_x: 0, clip_y: 0, clip_width: 0, clip_height: 0 };

    fn to_json(self) -> Value {
        json!({
            "x": self.x, "y": self.y, "clip_x": self.clip_x, "clip_y": self.clip_y,
            "clip_width": self.clip_width, "clip_height": self.clip_height,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let signed = |key: &str| value.get(key).and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok());
        let unsigned = |key: &str| value.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
        Some(Self {
            x: signed("x")?,
            y: signed("y")?,
            clip_x: signed("clip_x")?,
            clip_y: signed("clip_y")?,
            clip_width: unsigned("clip_width")?,
            clip_height: unsigned("clip_height")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello {
        version: u64,
    },
    /// Load `plugin` as `instance`, activate it at `sample_rate` and serve audio on the link
    /// at `link`.
    Load {
        instance: u32,
        plugin: String,
        link: String,
        sample_rate: f64,
    },
    Loaded {
        instance: u32,
        latency: u32,
    },
    /// Stop and let go of `instance`; the other instances in the process go on.
    Unload {
        instance: u32,
    },
    Unloaded {
        instance: u32,
    },
    Ping {
        id: u64,
    },
    Pong {
        id: u64,
    },
    GetState {
        instance: u32,
    },
    /// `controller` is a VST3 plug-in's controller state, when it has one apart from the
    /// component's (`data`).
    SetState {
        instance: u32,
        data: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    State {
        instance: u32,
        data: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    GetParams {
        instance: u32,
    },
    Params {
        instance: u32,
        params: Vec<ParamInfo>,
    },
    /// Set parameter `id` to `value` (in its own range); answered with `Param`.
    SetParam {
        instance: u32,
        id: u32,
        value: f64,
    },
    Param {
        instance: u32,
        param: ParamInfo,
    },
    /// Open the plug-in's editor; answered with `EditorOpened` and the size of its inside.
    /// `owner` is the engine's window (a Win32 `HWND`, an X11 window id). Not `embedded`: the
    /// editor gets a window of its own that `owner` owns: it stays above it, has no taskbar
    /// entry of its own and is minimised with it (`None`: a window of its own). `embedded`:
    /// the editor is a child inside `owner` (which must then be given), hidden until the first
    /// `PlaceEditor` puts it somewhere.
    OpenEditor {
        instance: u32,
        owner: Option<u64>,
        embedded: bool,
    },
    EditorOpened {
        instance: u32,
        width: u32,
        height: u32,
    },
    /// Put an embedded editor at `placement` in its parent window. Not answered; sent again
    /// whenever the device panel moves, scrolls or resizes.
    PlaceEditor {
        instance: u32,
        placement: Placement,
    },
    /// The plug-in gave its editor a new size (unasked, in both modes).
    EditorResized {
        instance: u32,
        width: u32,
        height: u32,
    },
    /// The plug-in reports another latency, in frames, while it runs (unasked): the host
    /// restarted its processing as its format asks, and blocks come back that much later.
    LatencyChanged {
        instance: u32,
        latency: u32,
    },
    /// The host reports its process's resident memory, in bytes, once a second (unasked): the
    /// control bar adds every host's to the program's own RAM.
    Memory {
        bytes: u64,
    },
    CloseEditor {
        instance: u32,
    },
    EditorClosed {
        instance: u32,
    },
    /// What went wrong with a request about `instance`, or with the process (`None`).
    Error {
        instance: Option<u32>,
        message: String,
    },
    Quit,
}

impl Message {
    /// The instance the message is about; `None` for the process-wide ones.
    pub fn instance(&self) -> Option<u32> {
        match self {
            Self::Load { instance, .. }
            | Self::Loaded { instance, .. }
            | Self::Unload { instance }
            | Self::Unloaded { instance }
            | Self::GetState { instance }
            | Self::SetState { instance, .. }
            | Self::State { instance, .. }
            | Self::GetParams { instance }
            | Self::Params { instance, .. }
            | Self::SetParam { instance, .. }
            | Self::Param { instance, .. }
            | Self::OpenEditor { instance, .. }
            | Self::EditorOpened { instance, .. }
            | Self::PlaceEditor { instance, .. }
            | Self::EditorResized { instance, .. }
            | Self::LatencyChanged { instance, .. }
            | Self::CloseEditor { instance }
            | Self::EditorClosed { instance } => Some(*instance),
            Self::Error { instance, .. } => *instance,
            Self::Hello { .. } | Self::Ping { .. } | Self::Pong { .. } | Self::Memory { .. } | Self::Quit => None,
        }
    }

    /// One line of JSON, without the newline.
    pub fn encode(&self) -> String {
        let value = match self {
            Self::Hello { version } => json!({ "op": "hello", "version": version }),
            Self::Load { instance, plugin, link, sample_rate } => json!({
                "op": "load", "instance": instance, "plugin": plugin, "link": link, "sample_rate": sample_rate,
            }),
            Self::Loaded { instance, latency } => json!({ "op": "loaded", "instance": instance, "latency": latency }),
            Self::Unload { instance } => json!({ "op": "unload", "instance": instance }),
            Self::Unloaded { instance } => json!({ "op": "unloaded", "instance": instance }),
            Self::Ping { id } => json!({ "op": "ping", "id": id }),
            Self::Pong { id } => json!({ "op": "pong", "id": id }),
            Self::GetState { instance } => json!({ "op": "get_state", "instance": instance }),
            Self::SetState { instance, data, controller } => json!({
                "op": "set_state", "instance": instance, "data": hex(data), "controller": controller.as_deref().map(hex),
            }),
            Self::State { instance, data, controller } => json!({
                "op": "state", "instance": instance, "data": hex(data), "controller": controller.as_deref().map(hex),
            }),
            Self::Error { instance, message } => json!({ "op": "error", "instance": instance, "message": message }),
            Self::GetParams { instance } => json!({ "op": "get_params", "instance": instance }),
            Self::Params { instance, params } => json!({
                "op": "params", "instance": instance, "params": params.iter().map(ParamInfo::to_json).collect::<Vec<_>>(),
            }),
            Self::SetParam { instance, id, value } => {
                json!({ "op": "set_param", "instance": instance, "id": id, "value": value })
            }
            Self::Param { instance, param } => json!({ "op": "param", "instance": instance, "param": param.to_json() }),
            Self::OpenEditor { instance, owner, embedded } => {
                json!({ "op": "open_editor", "instance": instance, "owner": owner, "embedded": embedded })
            }
            Self::EditorOpened { instance, width, height } => {
                json!({ "op": "editor_opened", "instance": instance, "width": width, "height": height })
            }
            Self::PlaceEditor { instance, placement } => {
                json!({ "op": "place_editor", "instance": instance, "placement": placement.to_json() })
            }
            Self::EditorResized { instance, width, height } => {
                json!({ "op": "editor_resized", "instance": instance, "width": width, "height": height })
            }
            Self::LatencyChanged { instance, latency } => {
                json!({ "op": "latency_changed", "instance": instance, "latency": latency })
            }
            Self::Memory { bytes } => json!({ "op": "memory", "bytes": bytes }),
            Self::CloseEditor { instance } => json!({ "op": "close_editor", "instance": instance }),
            Self::EditorClosed { instance } => json!({ "op": "editor_closed", "instance": instance }),
            Self::Quit => json!({ "op": "quit" }),
        };
        value.to_string()
    }

    /// Parses one line; `None` for anything that is not a known, well-formed message.
    pub fn decode(line: &str) -> Option<Self> {
        if line.len() > MAX_LINE {
            return None;
        }
        let value: Value = serde_json::from_str(line).ok()?;
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| value.get(key).and_then(Value::as_u64);
        let small = |key: &str| number(key).and_then(|n| u32::try_from(n).ok());
        let bytes = |key: &str| value.get(key).and_then(Value::as_str).and_then(unhex);
        // Absent or null is `None`; anything else must be hex.
        let optional_bytes = |key: &str| match value.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(v) => v.as_str().and_then(unhex).map(Some),
        };
        let instance = || small("instance");
        Some(match value.get("op")?.as_str()? {
            "hello" => Self::Hello { version: number("version")? },
            "load" => Self::Load {
                instance: instance()?,
                plugin: text("plugin")?,
                link: text("link")?,
                sample_rate: value.get("sample_rate").and_then(Value::as_f64).filter(|r| r.is_finite() && *r > 0.0)?,
            },
            "loaded" => Self::Loaded { instance: instance()?, latency: small("latency")? },
            "unload" => Self::Unload { instance: instance()? },
            "unloaded" => Self::Unloaded { instance: instance()? },
            "ping" => Self::Ping { id: number("id")? },
            "pong" => Self::Pong { id: number("id")? },
            "get_state" => Self::GetState { instance: instance()? },
            "set_state" => Self::SetState {
                instance: instance()?,
                data: bytes("data")?,
                controller: optional_bytes("controller")?,
            },
            "state" => {
                Self::State { instance: instance()?, data: bytes("data")?, controller: optional_bytes("controller")? }
            }
            "error" => Self::Error {
                // Absent or null is the process; anything else must be an instance number.
                instance: match value.get("instance") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(u32::try_from(v.as_u64()?).ok()?),
                },
                message: text("message")?,
            },
            "get_params" => Self::GetParams { instance: instance()? },
            "params" => {
                let list = value.get("params")?.as_array()?;
                if list.len() > MAX_PARAMS {
                    return None;
                }
                Self::Params {
                    instance: instance()?,
                    params: list.iter().map(ParamInfo::from_json).collect::<Option<Vec<_>>>()?,
                }
            }
            "set_param" => Self::SetParam {
                instance: instance()?,
                id: small("id")?,
                value: value.get("value").and_then(Value::as_f64).filter(|v| v.is_finite())?,
            },
            "param" => Self::Param { instance: instance()?, param: ParamInfo::from_json(value.get("param")?)? },
            "open_editor" => Self::OpenEditor {
                instance: instance()?,
                owner: match value.get("owner") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.as_u64()?),
                },
                embedded: value.get("embedded")?.as_bool()?,
            },
            "editor_opened" => {
                Self::EditorOpened { instance: instance()?, width: small("width")?, height: small("height")? }
            }
            "place_editor" => {
                Self::PlaceEditor { instance: instance()?, placement: Placement::from_json(value.get("placement")?)? }
            }
            "editor_resized" => {
                Self::EditorResized { instance: instance()?, width: small("width")?, height: small("height")? }
            }
            "latency_changed" => Self::LatencyChanged { instance: instance()?, latency: small("latency")? },
            "memory" => Self::Memory { bytes: number("bytes")? },
            "close_editor" => Self::CloseEditor { instance: instance()? },
            "editor_closed" => Self::EditorClosed { instance: instance()? },
            "quit" => Self::Quit,
            _ => return None,
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16);
    pairs.iter().map(|[high, low]| Some((digit(*high)? * 16 + digit(*low)?) as u8)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain() -> ParamInfo {
        ParamInfo {
            id: 0,
            name: "Gain".into(),
            group: "Main".into(),
            min: 0.0,
            max: 1.0,
            default: 0.5,
            value: 0.25,
            text: "-12.0 dB".into(),
            steps: 0,
            automatable: true,
            read_only: false,
            hidden: false,
        }
    }

    #[test]
    fn every_message_round_trips() {
        assert_eq!(VERSION, 9);
        let messages = [
            Message::Hello { version: VERSION },
            Message::Load { instance: 3, plugin: "test:gain".into(), link: "/dev/shm/x".into(), sample_rate: 44_100.0 },
            Message::Loaded { instance: u32::MAX, latency: 64 },
            Message::Unload { instance: 0 },
            Message::Unloaded { instance: 7 },
            Message::Ping { id: u64::MAX },
            Message::Pong { id: 7 },
            Message::GetState { instance: 1 },
            Message::SetState { instance: 1, data: vec![0, 1, 0xab, 0xff], controller: None },
            Message::SetState { instance: 2, data: vec![1], controller: Some(vec![2, 3]) },
            Message::State { instance: 1, data: Vec::new(), controller: None },
            Message::State { instance: 9, data: vec![4], controller: Some(Vec::new()) },
            Message::GetParams { instance: 4 },
            Message::Params {
                instance: 4,
                params: vec![gain(), ParamInfo { id: u32::MAX, steps: 1, hidden: true, ..gain() }],
            },
            Message::Params { instance: 4, params: Vec::new() },
            Message::SetParam { instance: 5, id: 7, value: -0.5 },
            Message::Param { instance: 5, param: gain() },
            Message::OpenEditor { instance: 6, owner: None, embedded: false },
            Message::OpenEditor { instance: 6, owner: Some(0x0012_0F3A), embedded: true },
            Message::EditorOpened { instance: 6, width: 800, height: 600 },
            Message::PlaceEditor {
                instance: 6,
                placement: Placement { x: -40, y: 620, clip_x: 0, clip_y: 600, clip_width: 1280, clip_height: 0 },
            },
            Message::EditorResized { instance: 6, width: 1, height: 16_384 },
            Message::LatencyChanged { instance: 6, latency: 0 },
            Message::LatencyChanged { instance: u32::MAX, latency: u32::MAX },
            Message::Memory { bytes: 0 },
            Message::Memory { bytes: u64::MAX },
            Message::CloseEditor { instance: 6 },
            Message::EditorClosed { instance: 6 },
            Message::Error { instance: None, message: "no such plug-in: \"x\"\n".into() },
            Message::Error { instance: Some(8), message: "the plug-in has no editor".into() },
            Message::Quit,
        ];
        for message in messages {
            let line = message.encode();
            assert!(!line.contains('\n'), "{line}");
            assert_eq!(Message::decode(&line), Some(message), "{line}");
        }
    }

    #[test]
    fn messages_name_their_instance() {
        assert_eq!(Message::Unload { instance: 4 }.instance(), Some(4));
        assert_eq!(Message::EditorClosed { instance: 2 }.instance(), Some(2));
        assert_eq!(Message::LatencyChanged { instance: 3, latency: 64 }.instance(), Some(3));
        assert_eq!(Message::Error { instance: Some(1), message: String::new() }.instance(), Some(1));
        assert_eq!(Message::Error { instance: None, message: String::new() }.instance(), None);
        assert_eq!(Message::Ping { id: 1 }.instance(), None);
        assert_eq!(Message::Quit.instance(), None);
    }

    #[test]
    fn malformed_lines_are_refused() {
        for line in [
            "",
            "{",
            "[]",
            r#"{"op":"nope"}"#,
            r#"{"op":"ping"}"#,
            r#"{"op":"ping","id":-1}"#,
            r#"{"op":"loaded","instance":1,"latency":5000000000}"#,
            r#"{"op":"loaded","latency":5}"#,
            r#"{"op":"state","instance":1,"data":"abc"}"#,
            r#"{"op":"state","instance":1,"data":"zz"}"#,
            r#"{"op":"state","instance":1,"data":"","controller":5}"#,
            r#"{"op":"state","data":""}"#,
            r#"{"op":"set_state","instance":1,"data":"","controller":"z"}"#,
            r#"{"op":"load","instance":1,"plugin":"x","link":"y","sample_rate":0}"#,
            r#"{"op":"load","instance":1,"plugin":"x","link":"y"}"#,
            r#"{"op":"load","plugin":"x","link":"y","sample_rate":48000}"#,
            r#"{"op":"load","instance":-1,"plugin":"x","link":"y","sample_rate":48000}"#,
            r#"{"op":"load","instance":5000000000,"plugin":"x","link":"y","sample_rate":48000}"#,
            r#"{"op":42}"#,
            r#"{"op":"set_param","instance":1,"id":1}"#,
            r#"{"op":"set_param","instance":1,"id":-1,"value":0.5}"#,
            r#"{"op":"set_param","id":1,"value":0.5}"#,
            r#"{"op":"params","instance":1,"params":[{"id":1}]}"#,
            r#"{"op":"param","instance":1}"#,
            r#"{"op":"editor_opened","instance":1,"width":800}"#,
            r#"{"op":"get_state"}"#,
            r#"{"op":"get_params"}"#,
            r#"{"op":"open_editor"}"#,
            r#"{"op":"open_editor","instance":1}"#,
            r#"{"op":"open_editor","instance":1,"embedded":"yes"}"#,
            r#"{"op":"place_editor","instance":1}"#,
            r#"{"op":"place_editor","instance":1,"placement":{"x":0,"y":0,"clip_x":0,"clip_y":0,"clip_width":-1,"clip_height":0}}"#,
            r#"{"op":"place_editor","instance":1,"placement":{"x":3000000000,"y":0,"clip_x":0,"clip_y":0,"clip_width":1,"clip_height":1}}"#,
            r#"{"op":"editor_resized","instance":1,"height":2}"#,
            r#"{"op":"close_editor","instance":"1"}"#,
            r#"{"op":"latency_changed","instance":1}"#,
            r#"{"op":"latency_changed","latency":64}"#,
            r#"{"op":"latency_changed","instance":1,"latency":-1}"#,
            r#"{"op":"latency_changed","instance":1,"latency":5000000000}"#,
            r#"{"op":"editor_closed"}"#,
            r#"{"op":"unload"}"#,
            r#"{"op":"unloaded","instance":1.5}"#,
            r#"{"op":"error","instance":"x","message":"m"}"#,
            r#"{"op":"error","instance":-3,"message":"m"}"#,
            r#"{"op":"error","instance":1}"#,
        ] {
            assert_eq!(Message::decode(line), None, "{line}");
        }
    }
}
