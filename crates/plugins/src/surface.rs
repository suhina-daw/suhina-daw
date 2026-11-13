// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The surface-program protocol (ADR 0019 › 10 and its Lua-runner amendment): the control
//! messages between Suhina and a surface program's runner process, one JSON object per line
//! over a local socket, like the plug-in control protocol ([`super::protocol`]). Behaviour
//! spec: [`docs/spec/surface-programs.md`](../../../docs/spec/surface-programs.md).
//!
//! A surface program is an extension that drives a control surface; Suhina ships one runner for
//! it, `suhina-lua-runner`, which runs the program's Lua script in a process of its own. The
//! runner is the only peer today, but the messages are the program's, not the language's: the
//! amendment's list — port messages in, send-backs out, commands, reading commands,
//! subscriptions — is the whole list, and every message here serves one item of it.
//!
//! Suhina → runner: `hello`, `ports`, `port_message`, `state`, `command_result`, `read_result`,
//! `ping`, `quit`, `error`. Runner → Suhina: `hello`, `ready`, `send`, `command`, `read`,
//! `subscribe`, `unsubscribe`, `script_error`, `pong`, `error`. The handshake is one `hello`
//! each; a peer whose version is not spoken is refused with `error` and the connection closes.
//! A send-back is never answered, and neither are `port_message`, `state`, the results, the
//! reports, `unsubscribe` and `quit`; `quit`'s answer is the runner process exiting.

use serde_json::{Value, json};

/// Bumped whenever a message changes; both sides refuse a different version.
pub const VERSION: u64 = 1;
/// Most bytes one `port_message` or `send` carries (a system-exclusive block, generously).
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Longest line either side accepts: a bytes field at its cap is twice that in hex, with the
/// JSON's keys around it, so the line cap is set past that, never below it.
pub const MAX_LINE: usize = 2 * MAX_MESSAGE_BYTES + 64 * 1024;
/// Most port names one `ports` message lists (the manifest's `ports` cap, extensions.md).
pub const MAX_PORTS: usize = 64;
/// Longest one port name, command id or gesture id.
pub const MAX_NAME: usize = 256;
/// Most subscriptions one program may keep live; the runner refuses the rest.
pub const MAX_SUBSCRIPTIONS: usize = 64;
/// Deepest JSON nesting a `params` object or a `state` value may have.
pub const MAX_DEPTH: usize = 32;

/// One message between Suhina and a surface program's runner.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello {
        version: u64,
    },
    /// The program's claimed ports that are open now; replaced whole when it changes.
    Ports {
        inputs: Vec<String>,
        outputs: Vec<String>,
    },
    /// Raw bytes read from a claimed input port, with the driver's arrival timestamp in
    /// microseconds.
    PortMessage {
        port: String,
        timestamp: u64,
        bytes: Vec<u8>,
    },
    /// A subscription's value as it is now; the first one answers the `subscribe`.
    State {
        token: u64,
        value: Value,
    },
    /// The answer to a `command`; `error` names the reason when `ok` is false.
    CommandResult {
        token: u64,
        ok: bool,
        error: Option<String>,
    },
    /// The answer to a `read`: the reading command's value, or the reason.
    ReadResult {
        token: u64,
        ok: bool,
        value: Option<Value>,
        error: Option<String>,
    },
    Ping {
        id: u64,
    },
    /// The runner stops the script and exits; the exit is the answer.
    Quit,
    /// The script loaded and registered its handlers; port messages may start.
    Ready,
    /// Raw bytes to write on one of the program's open output ports. Never answered: Suhina
    /// queues it and drains the queue.
    Send {
        port: String,
        bytes: Vec<u8>,
    },
    /// Run a command — the same command the window, CLI and MCP dispatch, undo included.
    /// `gesture` groups the stream into one undo step, ended by `history.endGesture`.
    Command {
        token: u64,
        id: String,
        params: Value,
        gesture: Option<String>,
    },
    /// Run a reading command; answered with `read_result`.
    Read {
        token: u64,
        id: String,
        params: Value,
    },
    /// Watch a reading command's result: the first `state` answers with the value now, then one
    /// comes each time the result changes from the last one sent.
    Subscribe {
        token: u64,
        id: String,
        params: Value,
    },
    /// Stop watching a subscription; never answered.
    Unsubscribe {
        token: u64,
    },
    /// The error boundary's one report: which handler or the chunk failed, and the Lua error.
    ScriptError {
        handler: String,
        message: String,
    },
    Pong {
        id: u64,
    },
    /// Why the handshake failed or the last message was refused; either side sends it, then
    /// closes.
    Error {
        message: String,
    },
}

impl Message {
    /// One line of JSON, without the newline.
    pub fn encode(&self) -> String {
        let value = match self {
            Self::Hello { version } => json!({ "op": "hello", "version": version }),
            Self::Ports { inputs, outputs } => json!({
                "op": "ports", "inputs": inputs, "outputs": outputs,
            }),
            Self::PortMessage { port, timestamp, bytes } => json!({
                "op": "port_message", "port": port, "timestamp": timestamp, "bytes": hex(bytes),
            }),
            Self::State { token, value } => json!({ "op": "state", "token": token, "value": value }),
            Self::CommandResult { token, ok, error } => {
                json!({ "op": "command_result", "token": token, "ok": ok, "error": error })
            }
            Self::ReadResult { token, ok, value, error } => {
                json!({ "op": "read_result", "token": token, "ok": ok, "value": value, "error": error })
            }
            Self::Ping { id } => json!({ "op": "ping", "id": id }),
            Self::Quit => json!({ "op": "quit" }),
            Self::Ready => json!({ "op": "ready" }),
            Self::Send { port, bytes } => json!({ "op": "send", "port": port, "bytes": hex(bytes) }),
            Self::Command { token, id, params, gesture } => {
                json!({ "op": "command", "token": token, "id": id, "params": params, "gesture": gesture })
            }
            Self::Read { token, id, params } => {
                json!({ "op": "read", "token": token, "id": id, "params": params })
            }
            Self::Subscribe { token, id, params } => {
                json!({ "op": "subscribe", "token": token, "id": id, "params": params })
            }
            Self::Unsubscribe { token } => json!({ "op": "unsubscribe", "token": token }),
            Self::ScriptError { handler, message } => {
                json!({ "op": "script_error", "handler": handler, "message": message })
            }
            Self::Pong { id } => json!({ "op": "pong", "id": id }),
            Self::Error { message } => json!({ "op": "error", "message": message }),
        };
        value.to_string()
    }

    /// The message one line of JSON carries, or `None` when the line is over the cap, is not
    /// one message, or breaks a cap or a field's kind.
    pub fn decode(line: &str) -> Option<Self> {
        if line.len() > MAX_LINE {
            return None;
        }
        let value: Value = serde_json::from_str(line).ok()?;
        let text = |key: &str, cap: usize| {
            value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= cap).map(str::to_string)
        };
        let number = |key: &str| value.get(key).and_then(Value::as_u64);
        // Absent or null is `None`; anything else must be a string within the cap.
        let optional_text = |key: &str, cap: usize| match value.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(v) => v.as_str().filter(|s| s.len() <= cap).map(str::to_string).map(Some),
        };
        let bytes = |key: &str| {
            value.get(key).and_then(Value::as_str).filter(|s| s.len() <= 2 * MAX_MESSAGE_BYTES).and_then(unhex)
        };
        // Absent is no parameters; present, an object: a command's parameters are its object.
        let params = |key: &str| match value.get(key) {
            None => Some(Value::Null),
            Some(v) => match v {
                Value::Null | Value::Object(_) => Some(v.clone()),
                _ => None,
            },
        };
        Some(match value.get("op")?.as_str()? {
            "hello" => Self::Hello { version: number("version")? },
            "ports" => {
                let inputs = ports(value.get("inputs")?)?;
                let outputs = ports(value.get("outputs")?)?;
                Self::Ports { inputs, outputs }
            }
            "port_message" => Self::PortMessage {
                port: text("port", MAX_NAME)?,
                timestamp: number("timestamp")?,
                bytes: bytes("bytes")?,
            },
            "state" => Self::State { token: number("token")?, value: depth_ok(value.get("value")?)?.clone() },
            "command_result" => Self::CommandResult {
                token: number("token")?,
                ok: value.get("ok")?.as_bool()?,
                error: optional_text("error", MAX_LINE)?,
            },
            "read_result" => Self::ReadResult {
                token: number("token")?,
                ok: value.get("ok")?.as_bool()?,
                value: match value.get("value") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(depth_ok(v)?.clone()),
                },
                error: optional_text("error", MAX_LINE)?,
            },
            "ping" => Self::Ping { id: number("id")? },
            "quit" => Self::Quit,
            "ready" => Self::Ready,
            "send" => Self::Send { port: text("port", MAX_NAME)?, bytes: bytes("bytes")? },
            "command" => Self::Command {
                token: number("token")?,
                id: text("id", MAX_NAME)?,
                params: params("params")?,
                gesture: optional_text("gesture", MAX_NAME)?,
            },
            "read" => Self::Read { token: number("token")?, id: text("id", MAX_NAME)?, params: params("params")? },
            "subscribe" => {
                Self::Subscribe { token: number("token")?, id: text("id", MAX_NAME)?, params: params("params")? }
            }
            "unsubscribe" => Self::Unsubscribe { token: number("token")? },
            "script_error" => {
                Self::ScriptError { handler: text("handler", MAX_NAME)?, message: text("message", MAX_LINE)? }
            }
            "pong" => Self::Pong { id: number("id")? },
            "error" => Self::Error { message: text("message", MAX_LINE)? },
            _ => return None,
        })
    }
}

/// A `ports` field: an array of at most [`MAX_PORTS`] non-empty names within [`MAX_NAME`].
fn ports(value: &Value) -> Option<Vec<String>> {
    let list = value.as_array()?;
    if list.len() > MAX_PORTS {
        return None;
    }
    list.iter().map(|v| v.as_str().filter(|s| !s.is_empty() && s.len() <= MAX_NAME).map(str::to_string)).collect()
}

/// A JSON value within [`MAX_DEPTH`] of nesting.
fn depth_ok(value: &Value) -> Option<&Value> {
    if depth(value) > MAX_DEPTH {
        return None;
    }
    Some(value)
}

fn depth(value: &Value) -> usize {
    match value {
        Value::Array(list) => 1 + list.iter().map(depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        _ => 0,
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
#[path = "surface_tests.rs"]
mod tests;
