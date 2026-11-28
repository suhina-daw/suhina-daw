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

/// What the startup wait hears: the runner's connection, its stderr reaching EOF first (it
/// exited), or the listener failing.
enum Startup {
    Connected(Stream),
    Exited(String),
    Refused(String),
}

/// Spawns the runner child for `address`'s socket and takes its connection. The accept blocks
/// on its own thread while a reader drains the child's stderr: a child that exits before it
/// connects ends the wait with its status and stderr, not a hang, and there is no clock in
/// the wait (AGENTS.md rule 7). The stderr reader forwards the child's lines and stays until
/// the process ends; the connection ends the startup wait.
fn spawn_and_connect(script_path: &std::path::Path, address: &str) -> (Child, Stream) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_suhina-lua-runner"));
    command.args(["--control", address, script_path.to_str().unwrap()]);
    spawn_and_wait(command, address)
}

/// [`spawn_and_connect`]'s wait, for any child: the runner's own command, or a test's child
/// that exits at once.
fn spawn_and_wait(mut command: Command, address: &str) -> (Child, Stream) {
    let (_, name) = parse_address(address).unwrap();
    let listener = ListenerOptions::new().name(name).create_sync().unwrap();
    let mut child = command.stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let (sender, wait) = std::sync::mpsc::channel();
    // The accept blocks: the listener was made before the child, so its connection is the
    // child's, or the wait is ended by the child's stderr closing.
    let accept = sender.clone();
    std::thread::spawn(move || {
        let result = match listener.accept() {
            Ok(stream) => Startup::Connected(stream),
            Err(e) => Startup::Refused(e.to_string()),
        };
        let _ = accept.send(result);
    });
    // The child's stderr is the explicit exit signal: its EOF ends the blocking accept wait
    // without a poll or a sleep. The lines go on to the test's stderr.
    let stderr = child.stderr.take().unwrap_or_else(|| panic!("the runner's stderr is piped"));
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut text = String::new();
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    text.push_str(&line);
                    eprint!("{line}");
                }
            }
        }
        let _ = sender.send(Startup::Exited(text));
    });
    match wait.recv().unwrap_or_else(|e| panic!("the startup signal ended: {e}")) {
        Startup::Connected(stream) => (child, stream),
        Startup::Exited(text) => {
            let status = child.wait().unwrap();
            panic!("the runner exited while starting: {status}\n{text}");
        }
        Startup::Refused(reason) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("accept: {reason}");
        }
    }
}

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
        let script_path = std::env::temp_dir().join(format!("{id}.lua"));
        std::fs::write(&script_path, script).unwrap();
        let (child, stream) = spawn_and_connect(&script_path, &address);
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

/// A payload at the per-message cap queues, one past it is refused without a copy, and the
/// refusal is the script's `false` — not a crash and not a silent wedge.
#[test]
fn a_send_at_the_message_cap_queues_and_one_past_it_is_refused() {
    let mut runner = Harness::new(
        r#"
        suhina.on_port_message(function(bytes, port)
            if bytes == "\x90\x3c\x7f" then
                suhina.send("Pad", string.rep("a", 1024 * 1024))
                local ok = suhina.send("Pad", string.rep("b", 1024 * 1024 + 1))
                suhina.send("Pad", suhina.midi.cc(1, ok and 0 or 1, 0))
            end
        end)
        "#,
    )
    .expect_ready();
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 1, "bytes": "903c7f" }));
    // The payload at the cap went out whole: 1 MiB of `a` as hex is 2 MiB of "61".
    let sent = runner.line();
    assert_eq!(sent["op"], "send", "{sent}");
    assert_eq!(sent["port"], "Pad");
    let bytes = sent["bytes"].as_str().expect("the payload");
    assert_eq!(bytes.len(), 2 * 1024 * 1024, "{} bytes of hex", bytes.len());
    assert!(bytes.starts_with("61"), "{}…", &bytes[..16.min(bytes.len())]);
    // One past the cap: refused, so the pad says so — the next line is the refusal's marker.
    assert_eq!(runner.line(), json!({ "op": "send", "port": "Pad", "bytes": "b10100" }));
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
    let script_path = std::env::temp_dir().join(format!("{id}.lua"));
    std::fs::write(&script_path, "return 1").unwrap();
    let (mut child, stream) = spawn_and_connect(&script_path, &address);
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

/// A child that exits before connecting ends the wait with its status, not a hang: the
/// stderr reader's EOF is the explicit signal, so no clock is involved.
#[test]
#[should_panic(expected = "the runner exited while starting")]
fn a_child_that_exits_before_connecting_is_reported() {
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
    #[cfg(unix)]
    let command = Command::new("/usr/bin/true");
    #[cfg(windows)]
    let command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "exit", "1"]);
        command
    };
    let _ = spawn_and_wait(command, &address);
}

/// Bytes as the protocol carries them: lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A control storm outrunning the answers is not a wedge: the port messages past the wait
/// queue's cap are dropped and counted, and the runner stays up, answers pings, and carries on
/// when the answer arrives.
#[test]
fn a_control_storm_drops_messages_instead_of_dying() {
    let mut runner = Harness::new(
        r#"
        suhina.on_port_message(function(bytes, port, timestamp)
            suhina.command("transport.setTempo", { bpm = 110 })
        end)
        "#,
    )
    .expect_ready();
    // The first press runs the handler, which waits on its command; the harness holds the
    // answer, so every later message queues — and past the cap, drops.
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 1, "bytes": "903c7f" }));
    let command = runner.line();
    assert_eq!(command["op"], "command", "the handler's command");
    let _ = command["token"].as_u64().expect("the command's token");
    for stamp in 2..(1024 + 4) {
        runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": stamp, "bytes": "903c7f" }));
    }
    // The runner is alive through the storm: the ping is answered.
    runner.send(json!({ "op": "ping", "id": 9 }));
    assert_eq!(runner.line(), json!({ "op": "pong", "id": 9 }), "alive through the storm");
    // The answer lands, and one more line wakes the drain: the queued storm's commands
    // emit in one burst — 1023 fast answers as the queue drains, and the last queued
    // message waiting on its answer like the first. A ping rides wherever the runner's
    // reading finds it; the harness counts the burst's 1024 commands, skips the pong,
    // and answers only the last command, the one that waited. The runner never dies.
    runner.send(json!({ "op": "command_result", "token": command["token"], "ok": true, "error": null }));
    runner.send(json!({ "op": "ping", "id": 10 }));
    let mut last = None;
    let mut commands = 0;
    while commands < 1024 {
        let line = runner.line();
        if line["op"] == "pong" {
            continue;
        }
        assert_eq!(line["op"], "command", "the storm drains as commands");
        last = Some(line);
        commands += 1;
    }
    runner.send(
        json!({ "op": "command_result", "token": last.expect("the last command")["token"], "ok": true, "error": null }),
    );
    runner.quit();
}

/// The full wait queue keeps a stream's last value: a final value that arrives when the
/// queue is full takes the same stream's last queued value's place, so what the handler
/// finally drains ends at the newest — stamp 1026's marker, not the dropped 1025's.
#[test]
fn a_final_value_past_the_full_queue_takes_its_streams_place() {
    let mut runner = Harness::new(
        r#"
        suhina.on_port_message(function(bytes, port, stamp)
            if stamp == 1 then
                suhina.command("transport.setTempo", { bpm = 110 })
            end
            suhina.send("Pad", suhina.midi.cc(stamp % 16, stamp % 128, 1))
        end)
        "#,
    )
    .expect_ready();
    // The first press runs the handler, which waits on its command; the harness holds the
    // answer, so every later message queues.
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 1, "bytes": "903c7f" }));
    let command = runner.line();
    assert_eq!(command["op"], "command", "the handler's command");
    // The queue fills: stamps 2 to 1025, 1024 messages.
    for stamp in 2..=1025 {
        runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": stamp, "bytes": "903c7f" }));
    }
    // The final value, different from the last queued one: it takes stamp 1025's place.
    runner.send(json!({ "op": "port_message", "port": "Pad in", "timestamp": 1026, "bytes": "903c7f" }));
    runner.send(json!({ "op": "command_result", "token": command["token"], "ok": true, "error": null }));
    // The waiting handler's send, then the queued ones in order — 1024 of them, the queue's
    // last being stamp 1026, which took stamp 1025's place: the last marker is cc 2
    // (1026 mod 128), which stamp 1025's drop would have made cc 1.
    let mut last = None;
    for _ in 0..1025 {
        let mut line = runner.line();
        assert_eq!(line["op"], "send", "{line}");
        last = Some(line["bytes"].take());
    }
    assert_eq!(last.expect("the last value's marker"), json!("b20201"), "the newest value is what stands");
    runner.quit();
}
