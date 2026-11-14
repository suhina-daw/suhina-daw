// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn round_trip(message: Message) -> Message {
    let line = message.encode();
    assert!(line.len() <= MAX_LINE, "every message encodes within the line cap");
    Message::decode(&line).unwrap_or_else(|| panic!("{message:?} decodes back"))
}

#[test]
fn every_message_round_trips() {
    let value = || json!({ "track": 3, "volume": { "dB": -3.5 } });
    let messages = [
        Message::Hello { version: VERSION },
        Message::Ports {
            inputs: vec!["Launchpad Pro 16".into(), "in:midi 20".into()],
            outputs: vec!["Launchpad Pro 16".into()],
        },
        Message::PortMessage { port: "Launchpad Pro 16".into(), timestamp: 1_234_567, bytes: vec![0x90, 0x3c, 0x7f] },
        Message::State { token: 7, value: value() },
        Message::CommandResult { token: 7, ok: true, error: None },
        Message::CommandResult { token: 8, ok: false, error: Some("no such track".into()) },
        Message::ReadResult { token: 9, ok: true, value: Some(value()), error: None },
        Message::ReadResult { token: 10, ok: false, value: None, error: Some("not a reading command".into()) },
        Message::Ping { id: 42 },
        Message::Quit,
        Message::Ready,
        Message::Send { port: "Launchpad Pro 16".into(), bytes: vec![0xf0, 0x7e, 0xf7] },
        Message::Command { token: 11, id: "track.setVolume".into(), params: value(), gesture: Some("pad-gain".into()) },
        Message::Command { token: 12, id: "track.stop".into(), params: Value::Null, gesture: None },
        Message::Read { token: 13, id: "track.info".into(), params: value() },
        Message::Subscribe { token: 14, id: "transport.info".into(), params: Value::Null },
        Message::Unsubscribe { token: 14 },
        Message::ScriptError { handler: "port_message".into(), message: "attempt to index a nil value".into() },
        Message::Pong { id: 42 },
        Message::Error { message: "version 9 is not spoken".into() },
    ];
    for message in messages {
        assert_eq!(round_trip(message.clone()), message);
    }
}

#[test]
fn an_empty_ports_list_round_trips_and_params_stay_absent() {
    let message = round_trip(Message::Ports { inputs: Vec::new(), outputs: Vec::new() });
    assert_eq!(message, Message::Ports { inputs: Vec::new(), outputs: Vec::new() });
    // A `params` object survives; an absent one stays absent, null in, null out.
    let line = json!({ "op": "command", "token": 1, "id": "x", "params": {} }).to_string();
    assert_eq!(
        Message::decode(&line),
        Some(Message::Command { token: 1, id: "x".into(), params: json!({}), gesture: None })
    );
    let line = json!({ "op": "command", "token": 1, "id": "x" }).to_string();
    assert_eq!(
        Message::decode(&line),
        Some(Message::Command { token: 1, id: "x".into(), params: Value::Null, gesture: None })
    );
}

#[test]
fn a_wrong_field_refuses_the_message() {
    let decode = |line: Value| Message::decode(&line.to_string());
    // An unknown op.
    assert_eq!(decode(json!({ "op": "record_audio" })), None);
    // Missing and wrongly typed fields.
    assert_eq!(decode(json!({ "op": "port_message", "port": "p", "timestamp": 1 })), None, "no bytes");
    assert_eq!(decode(json!({ "op": "port_message", "port": "p", "timestamp": 1, "bytes": "zz" })), None, "not hex");
    assert_eq!(decode(json!({ "op": "port_message", "port": "p", "timestamp": -1, "bytes": "" })), None, "not a u64");
    assert_eq!(decode(json!({ "op": "send", "port": "", "bytes": "" })), None, "an empty port name");
    assert_eq!(decode(json!({ "op": "send", "port": "p", "bytes": "f0f" })), None, "an odd number of digits");
    assert_eq!(
        decode(json!({ "op": "command", "token": 1, "id": "x", "params": 3 })),
        None,
        "params are an object or absent"
    );
    assert_eq!(decode(json!({ "op": "command", "token": 1, "id": "x", "params": [], "gesture": 4 })), None);
    assert_eq!(decode(json!({ "op": "command_result", "token": 1, "ok": "yes" })), None);
    assert_eq!(decode(json!({ "op": "state", "token": 1 })), None, "no value");
    assert_eq!(decode(json!({ "op": "read_result", "token": 1, "ok": true, "value": 1.5, "error": 2 })), None);
    assert_eq!(decode(json!({ "op": "error" })), None, "no message");
    assert_eq!(decode(json!({ "op": "ports", "inputs": "all", "outputs": [] })), None);
    assert_eq!(decode(json!({ "op": "ports", "inputs": [""], "outputs": [] })), None, "an empty port name");
    assert_eq!(decode(json!({ "op": "ping" })), None, "no id");
    // Not JSON, not an object, no op.
    assert_eq!(Message::decode("3"), None);
    assert_eq!(Message::decode("[1, 2]"), None);
    assert_eq!(Message::decode("{}"), None);
}

#[test]
fn the_caps_hold() {
    let ok = |line: Value| Message::decode(&line.to_string()).is_some();
    // Bytes: a send of one more than the cap is refused, the cap itself passes.
    let bytes = vec![0u8; MAX_MESSAGE_BYTES];
    assert!(ok(json!({ "op": "send", "port": "p", "bytes": hex(&bytes) })), "the bytes cap passes");
    let too_long = vec![0u8; MAX_MESSAGE_BYTES + 1];
    assert!(!ok(json!({ "op": "send", "port": "p", "bytes": hex(&too_long) })), "one past the bytes cap");
    // Ports: the cap and one past it.
    let names = (0..MAX_PORTS).map(|i| format!("port {i}")).collect::<Vec<_>>();
    assert!(ok(json!({ "op": "ports", "inputs": names, "outputs": [] })));
    let mut names = names;
    names.push("one past".into());
    assert!(!ok(json!({ "op": "ports", "inputs": names, "outputs": [] })));
    // Names: a name of the cap's length passes, one longer is refused.
    let name = "p".repeat(MAX_NAME);
    assert!(ok(json!({ "op": "send", "port": name, "bytes": "" })));
    let name = "p".repeat(MAX_NAME + 1);
    assert!(!ok(json!({ "op": "send", "port": name, "bytes": "" })));
    // Nesting: a value MAX_DEPTH deep passes, one deeper is refused.
    let deep = |levels: usize| {
        let mut value = json!(null);
        for _ in 0..levels {
            value = json!({ "in": value });
        }
        value
    };
    assert!(ok(json!({ "op": "state", "token": 1, "value": deep(MAX_DEPTH) })), "MAX_DEPTH of nesting passes");
    assert!(!ok(json!({ "op": "state", "token": 1, "value": deep(MAX_DEPTH + 1) })), "one deeper is refused");
}

#[test]
fn a_line_over_the_cap_is_refused() {
    // A script_error's message may be long, but the line cap ends the connection before decode.
    let long = Message::ScriptError { handler: "chunk".into(), message: "x".repeat(MAX_LINE + 1) };
    assert!(Message::decode(&long.encode()).is_none(), "the encoder's line is over the cap, decode refuses it");
}
