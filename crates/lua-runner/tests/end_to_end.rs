// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The runner binary end to end (`docs/spec/surface-programs.md` › Testing): the harness plays
//! Suhina over a real local socket — handshake, `ready`, a port message in and its send-back
//! out, a command and its answer, a subscription, the error boundary, the version refusal and
//! `quit`. Every read blocks on the socket; no test waits a wall-clock while.

#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::io::{BufRead, BufReader, Write as IoWrite};
use std::process::{Child, Command, Stdio};

use interprocess::TryClone;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, prelude::*};
use serde_json::{Value, json};
use suhina_plugins::parse_address;

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The harness: the runner as a child process, and the two halves of its socket.
struct Harness {
    child: Child,
    reader: BufReader<Stream>,
    writer: Stream,
}

impl Harness {
    /// Starts the runner with `script` on its own socket and completes the handshake: one
    /// hello each, then `ports` from the harness — the runner applies it before the script
    /// runs. The script is left to load.
    fn new(script: &str) -> Self {
        let id = format!(
            "suhina-runner-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let address = if GenericNamespaced::is_supported() {
            format!("ns:{id}")
        } else {
            format!("fs:{}", std::env::temp_dir().join(&id).display())
        };
        let (_, name) = parse_address(&address).unwrap();
        let listener = ListenerOptions::new().name(name).create_sync().unwrap();
        let script_path = std::env::temp_dir().join(format!("{id}.lua"));
        std::fs::write(&script_path, script).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_suhina-lua-runner"))
            .args(["--control", &address, script_path.to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        // The child connects or exits; either ends the wait, and a hung start is a failure.
        let stream = loop {
            match listener.accept() {
                Ok(stream) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("accept: {e}"),
            }
            if child.try_wait().unwrap().is_some() {
                panic!("the runner exited while starting");
            }
        };
        let writer = stream.try_clone().unwrap();
        let mut harness = Self { child, reader: BufReader::new(stream), writer };
        harness.send(json!({ "op": "hello", "version": 1 }));
        assert_eq!(harness.line()["op"], "hello", "the runner's hello");
        harness.ports();
        harness
    }

    /// Expects the script to have loaded: `ready`.
    fn expect_ready(mut self) -> Self {
        assert_eq!(self.line()["op"], "ready", "the script loaded");
        self
    }

    /// One line from the runner, as JSON.
    fn line(&mut self) -> Value {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap_or_else(|e| panic!("a JSON line, got {line:?}: {e}"))
    }

    /// Sends one message to the runner.
    fn send(&mut self, message: Value) {
        writeln!(self.writer, "{message}").unwrap();
        self.writer.flush().unwrap();
    }

    /// The claimed ports the tests use: `Pad` as an output, `Pad in` as an input.
    fn ports(&mut self) {
        self.send(json!({ "op": "ports", "inputs": ["Pad in"], "outputs": ["Pad"] }));
    }

    /// Ends the run with `quit` and expects the runner to exit successfully.
    fn quit(mut self) {
        self.send(json!({ "op": "quit" }));
        let status = self.child.wait().unwrap();
        assert!(status.success(), "the runner exits on quit");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A port message in, its send-back out, pings answered, and `quit` ending it.
#[test]
fn a_port_message_comes_back_as_a_send() {
    let mut runner = Harness::new(
        r#"
        suhina.on_port_message(function(bytes, port, timestamp)
            if bytes == "\x90\x3c\x7f" then
                suhina.send("Pad", suhina.midi.cc(1, 7, 100))
            end
        end)
        "#,
    )
    .expect_ready();
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 1_234_567, "bytes": "903c7f" }));
    let sent = runner.line();
    assert_eq!(sent["op"], "send");
    assert_eq!(sent["port"], "Pad");
    assert_eq!(sent["bytes"], "b10764");
    // A note the script does not answer sends nothing: the next line is the pong.
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 1_234_568, "bytes": "803c00" }));
    runner.send(json!({ "op": "ping", "id": 7 }));
    assert_eq!(runner.line(), json!({ "op": "pong", "id": 7 }), "no send-back for an unanswered note");
    runner.quit();
}

/// The ports handler hears the ports as they are once the script has registered — before
/// `ready` — and again each time they change: a script that sends can start at once.
#[test]
fn the_ports_handler_hears_the_ports_as_they_are_and_again_when_they_change() {
    let mut runner = Harness::new(
        r#"
        suhina.on_ports(function(inputs, outputs)
            for _, out in ipairs(outputs) do
                suhina.send(out, suhina.midi.cc(1, 7, #inputs))
            end
        end)
        "#,
    );
    // The handshake's ports, to the handler as soon as the script has registered: its first
    // send-back leaves ahead of `ready`.
    let sent = runner.line();
    assert_eq!(sent["op"], "send", "{sent}");
    assert_eq!(sent["port"], "Pad");
    assert_eq!(sent["bytes"], "b10701");
    assert_eq!(runner.line()["op"], "ready", "the script loaded");
    // The ports replaced whole: the handler hears them again.
    runner.send(json!({ "op": "ports", "inputs": ["Pad in", "Other"], "outputs": ["Pad"] }));
    let sent = runner.line();
    assert_eq!(sent["op"], "send", "{sent}");
    assert_eq!(sent["bytes"], "b10702");
    runner.quit();
}

/// The API's command, read and subscribe, with the tokens carried through.
#[test]
fn commands_reads_and_subscriptions_round_trip() {
    let mut runner = Harness::new(
        r#"
        local ok = suhina.command("track.setVolume", { track = 3, volume = { dB = -3.5 } }, "pad")
        if ok then suhina.send("Pad", "moved") end
        local value = suhina.read("track.info", { track = 3 })
        suhina.send("Pad", "volume " .. value.volume)
        local token = suhina.subscribe("transport.info")
        suhina.send("Pad", "token " .. token)
        "#,
    );
    // The chunk blocks in the command until its answer comes, so the command is read first.
    assert_eq!(
        runner.line(),
        json!({ "op": "command", "token": 1, "id": "track.setVolume",
        "params": { "track": 3, "volume": { "dB": -3.5 } }, "gesture": "pad" })
    );
    runner.send(json!({ "op": "command_result", "token": 1, "ok": true, "error": null }));
    // The script's own order is kept: each send-back leaves before the call after it.
    assert_eq!(runner.line(), json!({ "op": "send", "port": "Pad", "bytes": hex(b"moved") }));
    assert_eq!(runner.line(), json!({ "op": "read", "token": 2, "id": "track.info", "params": { "track": 3 } }));
    runner
        .send(json!({ "op": "read_result", "token": 2, "ok": true, "value": { "volume": "dB -1.0" }, "error": null }));
    // The script's order is kept: the send-back goes out before the subscribe it precedes.
    assert_eq!(runner.line(), json!({ "op": "send", "port": "Pad", "bytes": hex(b"volume dB -1.0") }));
    assert_eq!(runner.line(), json!({ "op": "subscribe", "token": 3, "id": "transport.info", "params": null }));
    assert_eq!(runner.line(), json!({ "op": "send", "port": "Pad", "bytes": hex(b"token 3") }));
    // The chunk is done, so `ready` comes last.
    assert_eq!(runner.line()["op"], "ready", "the script loaded, after its chunk's calls");
    // The subscription's first state arrives with no handler set; the runner stays up.
    runner.send(json!({ "op": "state", "token": 3, "value": { "playing": false } }));
    runner.send(json!({ "op": "ping", "id": 1 }));
    assert_eq!(runner.line(), json!({ "op": "pong", "id": 1 }));
    runner.quit();
}

#[test]
fn a_failed_command_says_why() {
    let mut runner = Harness::new(
        r#"
        local ok, why = suhina.command("track.setVolume", { track = 999 })
        suhina.send("Pad", tostring(ok) .. ": " .. tostring(why))
        "#,
    );
    assert_eq!(runner.line()["op"], "command", "the command goes out first");
    runner.send(json!({ "op": "command_result", "token": 1, "ok": false, "error": "no such track" }));
    assert_eq!(runner.line(), json!({ "op": "send", "port": "Pad", "bytes": hex(b"false: no such track") }));
    assert_eq!(runner.line()["op"], "ready");
    runner.quit();
}

/// A chunk that errors is the error boundary: one report, pings still answered, `quit` works.
#[test]
fn a_chunk_error_reports_once_and_stays_up() {
    let mut runner = Harness::new(r#"error("boom")"#);
    let report = runner.line();
    assert_eq!(report["op"], "script_error");
    assert_eq!(report["handler"], "chunk");
    assert!(report["message"].as_str().unwrap().contains("boom"), "{}", report["message"]);
    runner.send(json!({ "op": "ping", "id": 2 }));
    assert_eq!(runner.line(), json!({ "op": "pong", "id": 2 }), "the runner stays up");
    runner.quit();
}

/// A handler that errors reports once; no handler runs after, and no second report comes.
#[test]
fn a_handler_error_reports_once_and_no_handler_runs_after() {
    let mut runner = Harness::new(
        r#"
        suhina.on_port_message(function() error("pad broke") end)
        "#,
    )
    .expect_ready();
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 1, "bytes": "903c7f" }));
    let report = runner.line();
    assert_eq!(report["op"], "script_error");
    assert_eq!(report["handler"], "port_message");
    assert!(report["message"].as_str().unwrap().contains("pad broke"), "{}", report["message"]);
    // A second port message runs nothing: the next line is the pong, with no second report.
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 2, "bytes": "903c7f" }));
    runner.send(json!({ "op": "ping", "id": 3 }));
    assert_eq!(runner.line(), json!({ "op": "pong", "id": 3 }));
    runner.quit();
}

/// The sandbox holds in a real process: the removed globals are gone, `require` fails.
#[test]
fn the_sandbox_holds_in_a_running_script() {
    let mut runner = Harness::new(
        r#"
        local removed = tostring(io == nil) .. tostring(os == nil) .. tostring(package == nil)
        local required = tostring(pcall(require, "io"))
        suhina.send("Pad", removed .. " " .. required)
        "#,
    );
    // A chunk's send-back leaves before `ready`, in the order the script made it.
    let sent = runner.line();
    assert_eq!(sent["op"], "send");
    assert_eq!(sent["bytes"], hex(b"truetruetrue false"), "io, os and package are nil; require fails");
    assert_eq!(runner.line()["op"], "ready", "the script loaded, after its chunk's calls");
    runner.quit();
}

/// A peer whose version is not spoken is refused with `error`, and the runner exits.
#[test]
fn a_wrong_version_is_refused() {
    // The harness speaks version 9; the runner refuses before the script loads.
    let id = format!(
        "suhina-runner-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let address = if GenericNamespaced::is_supported() {
        format!("ns:{id}")
    } else {
        format!("fs:{}", std::env::temp_dir().join(&id).display())
    };
    let (_, name) = parse_address(&address).unwrap();
    let listener = ListenerOptions::new().name(name).create_sync().unwrap();
    let script_path = std::env::temp_dir().join(format!("{id}.lua"));
    std::fs::write(&script_path, "return 1").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_suhina-lua-runner"))
        .args(["--control", &address, script_path.to_str().unwrap()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stream = listener.accept().unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    // The runner speaks first: its hello, then the refusal of ours, then the exit.
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(line.trim()).unwrap(), json!({ "op": "hello", "version": 1 }));
    writeln!(writer, r#"{{ "op": "hello", "version": 9 }}"#).unwrap();
    writer.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let answer: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(answer, json!({ "op": "error", "message": "version 9 is not spoken; this runner speaks 1" }));
    let status = child.wait().unwrap();
    assert!(!status.success(), "the runner exits after the refusal");
}

/// Bytes as the protocol carries them: lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
