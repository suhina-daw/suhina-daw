// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The surface-program supervisor end to end (`docs/spec/surface-programs.md` › Testing): the
//! real runner binary as a child of the real supervisor, driven through a [`Projects`]. The
//! waits are on the events the supervisor reports, never on a wall-clock while. The tests live
//! beside the runner they spawn, so the binary under them is the one just built — a
//! session-only test run would spawn whatever a previous workspace build left behind.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)] // test helpers outside #[test] functions

use std::sync::{Arc, Condvar, Mutex};

use serde_json::{Value, json};
use suhina_session::extensions::programs;
use suhina_session::{LiveInput, MidiOut, MidiSystem, Projects, RawSink, Session, blank_project};

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// An extension in a fresh store, sealed against its script, and a `Projects` over the store.
/// `projects` is dropped before `root` on purpose: the programs stop, and only then does the
/// store's folder go.
struct Fixture {
    projects: Projects,
    id: String,
    root: std::path::PathBuf,
    midi: Arc<Midi>,
}

const SCRIPT_OK: &str =
    "suhina.on_port_message(function(bytes, port) suhina.send(\"Pad\", suhina.midi.cc(1, 7, 100)) end)\n";
const SCRIPT_ERROR: &str = "suhina.on_port_message(function() error(\"pad broke\") end)\n";
/// Echoes to the output port the router claims for it: the send only succeeds once the
/// program has heard its ports.
const SCRIPT_ECHO: &str =
    "suhina.on_port_message(function(bytes, port) suhina.send(\"APC out\", suhina.midi.cc(1, 7, 100)) end)\n";
/// Runs a command (a tempo change) and a read (the tempo back), and reports both answers on
/// one pad: the controller byte says whether the command ran, the value byte the tempo read.
const SCRIPT_COMMAND: &str = r#"
suhina.on_port_message(function(bytes, port)
    if bytes == "\x90\x3c\x7f" then
        local ok, err = suhina.command("transport.setTempo", { bpm = 110 })
        local value, read_err = suhina.read("project.launcher")
        local columns = value and #value.tracks or 0
        suhina.send("APC out", suhina.midi.cc(1, ok and 1 or 0, columns))
    end
end)
"#;
/// A command that does not exist: the answer says so, and nothing crashes.
const SCRIPT_REFUSED: &str = r#"
suhina.on_port_message(function(bytes, port)
    if bytes == "\x90\x3c\x7f" then
        local ok, err = suhina.command("no.such.command")
        suhina.send("APC out", suhina.midi.cc(1, ok and 1 or 0, 0))
    end
end)
"#;
/// Watches the launcher grid: the first state comes after the subscribe, the next when the
/// project changes; the controller byte counts the states, the value byte says the state came
/// with its token first and its value with it, as the spec's handler shape says.
const SCRIPT_SUBSCRIBE: &str = r#"
local count = 0
suhina.on_state(function(token, value)
    count = count + 1
    local ordered = (token == 1 and type(value) == "table" and value.tracks ~= nil) and 1 or 0
    suhina.send("APC out", suhina.midi.cc(1, count, ordered))
end)
suhina.subscribe("project.launcher")
"#;
/// Reads while it loads — in the chunk, before `ready` — and sends what it read: the load
/// only finishes when the session's answer has come, and the pad says the read was a real
/// one (the tracks' count as the value byte).
const SCRIPT_READ_AT_LOAD: &str = r#"
local value, err = suhina.read("project.launcher")
if err ~= nil then
    suhina.send("APC out", suhina.midi.cc(1, 9, 9))
    return
end
local columns = value and #value.tracks or 0
suhina.send("APC out", suhina.midi.cc(1, 1, columns))
"#;
/// At the subscription cap: 64 subscribes, the first token's state marked on the pad.
const SCRIPT_64_SUBSCRIBES: &str = r#"
suhina.on_state(function(token, value)
    if token == 1 then suhina.send("APC out", suhina.midi.cc(1, 0, 99)) end
end)
for i = 1, 64 do suhina.subscribe("project.launcher") end
"#;
/// An edit asked through `suhina.read`: the pad's controller byte says the read was refused
/// (1) or answered (0) — a read is a reading command, so this is 1.
const SCRIPT_READ_EDIT: &str = r#"
suhina.on_port_message(function(bytes, port)
    if bytes == "\x90\x3c\x7f" then
        local value, err = suhina.read("track.setVolume", { tracks = { 1 }, db = -3 })
        suhina.send("APC out", suhina.midi.cc(1, err ~= nil and 1 or 0, 0))
    end
end)
"#;
/// A subscription to an edit: its reads are refused, so the state's handler never sends.
const SCRIPT_SUBSCRIBE_EDIT: &str = r#"
suhina.on_state(function(token, value)
    suhina.send("APC out", suhina.midi.cc(1, 2, 0))
end)
suhina.subscribe("track.setVolume", { tracks = { 1 } })
"#;
/// One MiB of payload on every press: the send-back queue's byte budget is what a storm of
/// them is judged by, not the count alone.
const SCRIPT_BIG_SEND: &str = r#"
suhina.on_port_message(function(bytes, port)
    suhina.send("APC out", string.rep("a", 1024 * 1024))
end)
"#;
/// Two commands keyed by the press: one that fails (the stale answer's), one that works (the
/// fresh wait's). The controller byte says which answer the fresh wait heard.
const SCRIPT_TWO_COMMANDS: &str = r#"
suhina.on_port_message(function(bytes, port)
    if bytes == "\x90\x3c\x7f" then
        local ok, err = suhina.command("no.such.command")
        suhina.send("APC out", suhina.midi.cc(1, ok and 1 or 0, 0))
    elseif bytes == "\x90\x3d\x7f" then
        local ok, err = suhina.command("transport.setTempo", { bpm = 110 })
        suhina.send("APC out", suhina.midi.cc(2, ok and 1 or 0, 0))
    end
end)
"#;

impl Fixture {
    /// Makes `id` with `script` as its whole script, and a `Projects` whose extensions store
    /// is the only folder there.
    fn new(id: &str, script: &str) -> Self {
        Self::build(id, script, &["Pad*"], &[], &[])
    }

    /// A fixture whose program claims the stand-in's `APC*` ports: the round trip the MIDI
    /// router runs.
    fn claiming(id: &str, script: &str) -> Self {
        Self::build(id, script, &["APC*"], &["APC in", "Other in"], &["APC out", "Other out"])
    }

    /// A fixture whose program claims the stand-in's `APC*` ports, with none of them
    /// present yet: the device arrives later, as a controller plugged in after Suhina
    /// started.
    fn claiming_awaiting(id: &str, script: &str) -> Self {
        Self::build(id, script, &["APC*"], &[], &[])
    }

    fn build(id: &str, script: &str, patterns: &[&str], inputs: &[&str], outputs: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "suhina-programs-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(id)).expect("the folder is made");
        std::fs::write(root.join(id).join("script.lua"), script).expect("the script is written");
        let manifest = json!({
            "id": id,
            "name": "Test Pad",
            "version": "1.0",
            "provides": [{ "id": "surface", "label": "Test Pad surface" }],
            "license": "MIT",
            "license_source": "https://example.com/licence",
            "license_checked": "2026-11-13",
            "source": "user-provided",
            "protocol": programs::PROTOCOL,
            "programs": [{ "path": "script.lua", "sha256": sha(script.as_bytes()) }],
            "ports": patterns,
            "runner": "lua",
            "permissions": ["project.read"],
        });
        std::fs::write(root.join(id).join("extension.json"), manifest.to_string()).expect("the manifest is written");
        let midi = Arc::new(Midi {
            inputs: Arc::new(Mutex::new(inputs.iter().map(|n| (n.to_string(), format!("{n}#1"))).collect())),
            outputs: Arc::new(Mutex::new(outputs.iter().map(|n| (n.to_string(), format!("{n}#1"))).collect())),
            ..Midi::default()
        });
        let system: Arc<dyn MidiSystem> = midi.clone();
        let mut projects = Projects::default().with_extensions_root(&root);
        projects.set_midi_system(system);
        // An open session, so the scripts' commands and reads run on a project.
        projects.add(Session::new(blank_project()), None).expect("the session opens");
        Self { root, id: id.to_string(), projects, midi }
    }

    fn folder(&self) -> std::path::PathBuf {
        self.root.join(&self.id)
    }

    /// Starts the program through `extensions.restart`, the command a user has.
    fn start(&mut self) {
        let answer = self.projects.execute("extensions.restart", &json!({ "id": self.id })).expect("started");
        assert_eq!(answer["state"], "running", "{answer}");
    }

    /// Waits for the event that answers `which`, on the supervisor's signal — the store's one
    /// drain applies the states, so `extensions.list` sees them too. The failure says why the
    /// program is over, not just that the event never came.
    fn expect(&mut self, which: impl Fn(&programs::Event) -> bool, what: &str) -> programs::Event {
        self.projects
            .test_extensions_wait_for(which)
            .unwrap_or_else(|reason| panic!("the program ended without {what}: {reason}"))
    }

    /// Waits until the queue holds the event, leaving it there for the frame's drain — which
    /// is what writes a send-back out to its port. The failure says why the program is over.
    fn peek(&mut self, which: impl Fn(&programs::Event) -> bool) {
        self.projects
            .test_extensions_peek_for(which)
            .unwrap_or_else(|reason| panic!("the program ended before the waited event: {reason}"));
    }

    /// Services the session and waits until `done` says the waited thing is observable —
    /// in the queue, or already drained and written out by one of the looks — breathing on
    /// the programs' event signal between looks. What a test waits for is its own
    /// observable (a marker written to a port), not a frame count (AGENTS.md rule 7); a
    /// look may be what produces the waited thing (a script's command is answered where
    /// the frame drains), so the wait cannot be a plain one on the supervisor's signal.
    fn pump_until(&mut self, done: impl Fn(&Self) -> bool, what: &str) {
        for _ in 0..4_000 {
            if done(self) {
                return;
            }
            self.projects.poll_plugins();
            if done(self) {
                return;
            }
            if let Err(reason) = self.projects.test_extensions_breath() {
                panic!("the program ended without {what}: {reason}");
            }
        }
        panic!("{what} never became observable: {:?}", self.midi.written_for("APC out"));
    }

    /// Sends the claimed ports to the running program.
    fn ports(&mut self, inputs: Vec<String>, outputs: Vec<String>) {
        self.projects.test_extensions_program().expect("running").set_ports(inputs, outputs);
    }

    /// Sends a raw port message to the running program through the supervisor's test-only
    /// path, as the MIDI router will.
    fn port_message(&mut self, port: &str, bytes: &[u8]) {
        self.projects.test_extensions_program().expect("running").test_port_message(port, bytes);
    }

    fn tick(&mut self) {
        self.projects.test_extensions_program().expect("running").test_tick();
    }

    /// The runner process's id: the program is running, so the supervisor has spawned it.
    fn pid(&self) -> u32 {
        self.projects.test_extensions_program().and_then(|h| h.pid()).expect("the runner's pid")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Started, heard running, and stopped by the drop: the whole path a happy program takes.
#[test]
fn a_program_starts_runs_and_stops() {
    let mut fixture = Fixture::new("happy", SCRIPT_OK);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // The store says running, too.
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert_eq!(entry["state"], "running", "{entry}");
}

/// A program whose device is connected when Suhina starts runs on its own: the store's port
/// scan starts the installed, validated extension whose manifest's patterns match a port
/// present now (docs/spec/extensions.md › Running one). Nobody called `extensions.restart`.
#[test]
fn a_program_runs_when_its_device_is_connected_at_launch() {
    let mut fixture = Fixture::claiming("autopad", SCRIPT_ECHO);
    // The first look scans, starts the program on the match, and opens its claimed ports in
    // the same look.
    fixture.projects.poll_plugins();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert_eq!(entry["state"], "running", "{entry}");
    assert!(fixture.midi.raw_open("APC in"), "the claimed port opened for the program");
    assert!(!fixture.midi.live_open("APC in"), "a claimed port does not feed the armed tracks");
}

/// A program whose device arrives later starts then: the scan that found the first port
/// matching the manifest's patterns started it, the way a controller plugged in after
/// Suhina started works without a restart by hand. No device named, no program.
#[test]
fn a_program_starts_when_its_device_is_plugged_in_later() {
    let mut fixture = Fixture::claiming_awaiting("latepad", SCRIPT_ECHO);
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert_eq!(entry["state"], "available", "{entry}");
    // The device is plugged in: the same name the manifest claims.
    fixture.midi.inputs.lock().unwrap().push(("APC in".into(), "APC in#1".into()));
    fixture.midi.outputs.lock().unwrap().push(("APC out".into(), "APC out#1".into()));
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    fixture.expect(|e| *e == programs::Event::Running, "running");
    assert!(fixture.midi.raw_open("APC in"), "the claimed port opened for the program");
}

/// The device leaves, the program keeps running: its ports fall back to their profiles and
/// the scan does not stop what runs — `extensions.remove` is the way a program stops.
#[test]
fn a_program_whose_device_leaves_keeps_running() {
    let mut fixture = Fixture::claiming("keeps", SCRIPT_ECHO);
    fixture.projects.poll_plugins();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    assert!(fixture.midi.raw_open("APC in"));
    fixture.midi.inputs.lock().unwrap().retain(|(p, _)| p != "APC in");
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    assert!(fixture.midi.was_closed("APC in"), "the port closed with the device");
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert_eq!(entry["state"], "running", "{entry}");
}

/// A chunk that reads while it loads finishes: the session's answer to a startup request
/// reaches the chunk before `ready` — it cannot complete without it — so the load ends, and
/// the send the chunk made with what it read is written out before the program is running.
#[test]
fn a_chunk_that_reads_while_it_loads_gets_its_answer() {
    let mut fixture = Fixture::claiming("readchunk", SCRIPT_READ_AT_LOAD);
    // The auto-start began at construction; the looks the pump makes service the chunk's
    // read, the answer goes out the moment the session has it, and the chunk finishes with
    // it — its pad says the read was real: the blank project's four tracks as the value byte.
    fixture.pump_until(|f| written_contains(f, [0xB1, 1, 4]), "the chunk's send-back with what it read");
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert_eq!(entry["state"], "running", "{entry}");
}

/// A port message reaches the script and its send-back comes back to the session.
#[test]
fn a_port_message_comes_back_as_a_send_back_event() {
    let mut fixture = Fixture::new("echo", SCRIPT_OK);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // Open the output port the script sends on, as the MIDI slice will.
    fixture.ports(vec![], vec!["Pad".into()]);
    // A port message in: the supervisor's half of the protocol is the runner's client here,
    // so the test speaks to the supervisor's events, not the runner's socket.
    fixture.port_message("Pad in", &[0x90, 0x3c, 0x7f]);
    let event = fixture.expect(
        |e| matches!(e, programs::Event::SendBack { bytes, .. } if bytes == &vec![0xb1, 0x07, 0x64]),
        "the send-back",
    );
    let programs::Event::SendBack { port, .. } = event else { unreachable!() };
    assert_eq!(port, "Pad");
}

/// The host's heartbeat reaches the script: the tick the supervisor sends on the health
/// rhythm — on the tests' cue here, with the rhythm itself as the backstop — and the script's
/// periodic work (a controller's light re-assert) rides it out as a send-back.
const SCRIPT_TICK: &str = r#"
suhina.on_tick(function()
    suhina.send("Pad", "\176\7\100")
end)
"#;

#[test]
fn a_tick_reaches_the_script() {
    let mut fixture = Fixture::new("heart", SCRIPT_TICK);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.ports(vec![], vec!["Pad".into()]);
    fixture.tick();
    fixture.expect(
        |e| matches!(e, programs::Event::SendBack { bytes, .. } if bytes == &vec![0xb0, 0x07, 0x64]),
        "the tick's send-back",
    );
}

/// A script's error is reported and the process stays up; `extensions.restart` brings it back.
#[test]
fn a_script_error_is_reported_and_restart_brings_the_script_back() {
    let mut fixture = Fixture::new("broken", SCRIPT_ERROR);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // The handler only errors when it runs: a port message triggers it.
    fixture.port_message("Pad in", &[0x90, 0x3c, 0x7f]);
    let error = fixture.expect(|e| matches!(e, programs::Event::ScriptError(_)), "the script error");
    let programs::Event::ScriptError(message) = error else { unreachable!() };
    assert!(message.contains("pad broke"), "{message}");
    // The last error is on the extension, and the state still says running (the process is up).
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert!(entry["lastError"].as_str().expect("an error").contains("pad broke"), "{entry}");

    // The same script, fixed: a restart reads the folder again, so it runs what is there now.
    std::fs::write(fixture.folder().join("script.lua"), SCRIPT_OK).expect("written");
    let manifest = json!({
        "id": fixture.id,
        "name": "Test Pad",
        "version": "1.0",
        "provides": [{ "id": "surface", "label": "Test Pad surface" }],
        "license": "MIT",
        "license_source": "https://example.com/licence",
        "license_checked": "2026-11-13",
        "source": "user-provided",
        "protocol": programs::PROTOCOL,
        "programs": [{ "path": "script.lua", "sha256": sha(SCRIPT_OK.as_bytes()) }],
        "ports": ["Pad*"],
        "runner": "lua",
        "permissions": ["project.read"],
    });
    std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running again");
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert_eq!(entry["lastError"], Value::Null, "{entry}");
}

/// A killed process is seen as crashed and restarted by the supervisor itself.
#[test]
fn a_killed_process_is_restarted() {
    let mut fixture = Fixture::new("restartable", SCRIPT_OK);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // Kill the runner process the supervisor started, by the handle's pid.
    let pid = fixture.pid();
    let sent = fixture.projects.test_extensions_program().expect("running").test_kill(pid);
    assert!(sent, "the kill was sent");
    // The supervisor starts another one; the program is running again. A killed process is
    // seen as a closed connection or an exited status, either way a crash.
    fixture.expect(|e| matches!(e, programs::Event::Crashed(_)), "the crash");
    fixture.expect(|e| *e == programs::Event::Running, "running again");
}

/// A restarted program's subscriptions start over: the fresh script's first state arrives even
/// when nothing changed while the old process was dead. The old subscription records would
/// otherwise swallow it — their `last` already holds the value, so "no change" pushes nothing
/// and the new script's reads stay nil, its controller dark until something moves.
#[test]
fn a_restarted_program_hears_its_first_state_again() {
    let mut fixture = Fixture::claiming("resubscribe", SCRIPT_SUBSCRIBE);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // The first state came after the subscribe: the wait is on the state's marker, not a
    // frame count; the looks service the subscribe on the way.
    fixture.pump_until(|f| written_contains(f, [0xB1, 1, 1]), "the first state's marker");
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 1, 1]]);
    // The process dies and the supervisor starts another one; nothing changes in between.
    let pid = fixture.pid();
    let sent = fixture.projects.test_extensions_program().expect("running").test_kill(pid);
    assert!(sent, "the kill was sent");
    fixture.expect(|e| matches!(e, programs::Event::Crashed(_)), "the crash");
    fixture.expect(|e| *e == programs::Event::Running, "running again");
    // The fresh script heard the grid again, though nothing had moved: a second state pushed,
    // and the fresh script's counter counts it as its first.
    let states =
        |fixture: &Fixture| fixture.midi.written_for("APC out").iter().filter(|m| m.as_slice() == [0xB1, 1, 1]).count();
    fixture.pump_until(|f| states(f) >= 2, "the fresh script's marker");
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 1, 1], vec![0xB1, 1, 1]]);
}

/// `extensions.restart` replaces the handle without the old one going away first: the fresh
/// script's subscriptions (the same script tokens) must not bind to the old handle's records,
/// whose `last` values would swallow the first push — the controller dark until something
/// moved in the project.
#[test]
fn a_restarted_by_hand_program_hears_its_first_state_again() {
    let mut fixture = Fixture::claiming("handrestart", SCRIPT_SUBSCRIBE);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // The first state came after the subscribe: the wait is on the state's marker, not a
    // frame count; the looks service the subscribe on the way.
    fixture.pump_until(|f| written_contains(f, [0xB1, 1, 1]), "the first state's marker");
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 1, 1]]);
    // The user's restart: the command a button and the CLI run.
    fixture.projects.execute("extensions.restart", &json!({ "id": "handrestart" })).expect("restarted");
    // The fresh script heard the grid again, though nothing had moved.
    let states =
        |fixture: &Fixture| fixture.midi.written_for("APC out").iter().filter(|m| m.as_slice() == [0xB1, 1, 1]).count();
    fixture.pump_until(|f| states(f) >= 2, "the fresh script's marker");
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 1, 1], vec![0xB1, 1, 1]]);
}

fn find(listed: &Value, id: &str) -> Option<Value> {
    listed["extensions"].as_array()?.iter().find(|e| e["id"] == id).cloned()
}

/// Whether the program's output port has written the marker yet: the observable a
/// pump-until waits for, still true after a look drained and wrote it.
fn written_contains(fixture: &Fixture, marker: [u8; 3]) -> bool {
    fixture.midi.written_for("APC out").iter().any(|m| m.as_slice() == marker)
}

/// A claimed port's message reaches the script, and its send-back leaves on the matched
/// output port. An unclaimed port opens the usual way; a claimed one does not feed the armed
/// tracks.
#[test]
fn a_claimed_port_reaches_the_program_and_its_send_back_goes_out() {
    let mut fixture = Fixture::claiming("echo", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    assert!(fixture.midi.raw_open("APC in"), "{:?}", fixture.midi.raw_list());
    assert!(!fixture.midi.live_open("APC in"), "a claimed port does not feed the armed tracks");
    assert!(fixture.midi.live_open("Other in"), "an unclaimed port opens the usual way");
    fixture.midi.play("APC in", 42, &[0x90, 60, 100]);
    fixture.peek(|e| matches!(e, programs::Event::SendBack { port, .. } if port == "APC out"));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 0x07, 0x64]]);
    assert!(fixture.midi.written_for("Other out").is_empty(), "{:?}", fixture.midi.written_for("Other out"));
}

/// The program gone, its ports go back to the usual path.
#[test]
fn a_removed_programs_ports_go_back_to_the_usual_path() {
    let mut fixture = Fixture::claiming("gone", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    assert!(fixture.midi.raw_open("APC in"));
    let answer = fixture.projects.execute("extensions.remove", &json!({ "id": fixture.id })).expect("removed");
    assert_eq!(answer["removed"], true, "{answer}");
    assert!(!fixture.midi.raw_open("APC in"), "the claimed port is closed");
    assert!(fixture.midi.was_closed("APC in"));
    assert!(fixture.midi.live_open("APC in"), "the port plays to the armed tracks again");
}

/// A restart's new process hears its ports again: the port is opened for it, and its
/// send-backs leave as before.
#[test]
fn a_restarted_program_hears_its_ports_again() {
    let mut fixture = Fixture::claiming("again", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    // The count the restart is judged by is the one now: the store may have started the
    // program itself when the fixture made the device present, and the start by hand then
    // replaced it.
    let opens_at_start = fixture.midi.raw_opens("APC in");
    assert!(opens_at_start >= 1, "{opens_at_start} opens");
    fixture.projects.execute("extensions.restart", &json!({ "id": fixture.id })).expect("restarted");
    fixture.expect(|e| *e == programs::Event::Running, "running again");
    fixture.projects.poll_plugins();
    // The port was opened again for the new process: one close, one new open.
    assert_eq!(fixture.midi.raw_opens("APC in"), opens_at_start + 1);
    assert!(fixture.midi.was_closed("APC in"));
    fixture.midi.play("APC in", 7, &[0xB0, 7, 1]);
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    assert_eq!(fixture.midi.written_for("APC out").len(), 1, "{:?}", fixture.midi.written_for("APC out"));
}

/// A restart whose folder no longer validates leaves the running program alone: the manifest
/// is read before anything stops, so a checksum that moved under the program is refused with
/// its reason while the process keeps answering.
#[test]
fn a_restart_that_no_longer_validates_leaves_the_program_running() {
    let mut fixture = Fixture::claiming("stillruns", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // The manifest breaks under the running program.
    std::fs::write(fixture.folder().join("extension.json"), "{ not json").expect("written");
    let error = fixture.projects.execute("extensions.restart", &json!({ "id": fixture.id })).expect_err("refused");
    assert!(error.message().contains("manifest"), "{}", error.message());
    // The running program is untouched: it answers still, and the store says running.
    fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    fixture.pump_until(|f| written_contains(f, [0xB1, 7, 100]), "the echo of the untouched program");
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    assert_eq!(entry["state"], "running", "{entry}");
}

/// A manifest that changed under a running program is not what the process runs: the next
/// read of the store stops it, and the folder's manifest — the new script with it — runs
/// from then on. The old script's echoes do not keep coming.
#[test]
fn a_manifest_changed_under_a_running_program_stops_the_old_process() {
    // The old script echoes cc 7; the new one echoes cc 8, so the scripts are told apart.
    const SCRIPT_OLD: &str = SCRIPT_ECHO;
    const SCRIPT_NEW: &str =
        "suhina.on_port_message(function(bytes, port) suhina.send(\"APC out\", suhina.midi.cc(1, 8, 100)) end)\n";
    let mut fixture = Fixture::claiming("underneath", SCRIPT_OLD);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    fixture.pump_until(|f| written_contains(f, [0xB1, 7, 100]), "the old script's echo");
    let old_echoes = fixture.midi.written_for("APC out").iter().filter(|m| m.as_slice() == [0xB1, 7, 100]).count();
    assert_eq!(old_echoes, 1);

    // The folder changes: a new script, a manifest that seals it, and a new name.
    std::fs::write(fixture.folder().join("script.lua"), SCRIPT_NEW).expect("written");
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture.folder().join("extension.json")).expect("read"))
            .expect("the manifest");
    manifest["name"] = json!("Changed Pad");
    manifest["programs"] = json!([{ "path": "script.lua", "sha256": sha(SCRIPT_NEW.as_bytes()) }]);
    std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");

    // The store is read again when another extension is added; the running program's
    // manifest no longer matches, so its process stops.
    let second = std::env::temp_dir().join(format!("suhina-programs-second-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&second);
    std::fs::create_dir_all(second.join("second-pad")).expect("made");
    std::fs::write(
        second.join("second-pad").join("extension.json"),
        json!({
            "id": "second-pad",
            "name": "Second Pad",
            "version": "1.0",
            "provides": [{ "id": "surface", "label": "Second Pad surface" }],
            "license": "MIT",
            "license_source": "https://example.com/licence",
            "license_checked": "2026-11-13",
            "source": "user-provided",
            "protocol": programs::PROTOCOL,
            "programs": [{ "path": "script.lua", "sha256": sha("-- unused\n".as_bytes()) }],
            "permissions": ["project.read"]
        })
        .to_string(),
    )
    .expect("written");
    std::fs::write(second.join("second-pad").join("script.lua"), "-- unused\n").expect("written");
    let answer = fixture
        .projects
        .execute("extensions.add", &json!({ "folder": second.join("second-pad").display().to_string() }))
        .expect("added");
    assert_eq!(answer["id"], "second-pad", "{answer}");
    let _ = std::fs::remove_dir_all(&second);

    // What runs now is the folder's manifest: its script, echoing cc 8. The look starts it;
    // the message comes after it is running, so it reaches the new process.
    fixture.projects.poll_plugins();
    fixture.expect(|e| *e == programs::Event::Running, "the new program is running");
    fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    fixture.pump_until(|f| written_contains(f, [0xB1, 8, 100]), "the new script's echo");
    let echoes = fixture.midi.written_for("APC out");
    assert_eq!(echoes.iter().filter(|m| m.as_slice() == [0xB1, 7, 100]).count(), old_echoes, "the old script stopped");
    assert!(echoes.iter().any(|m| m.as_slice() == [0xB1, 8, 100]), "{echoes:?}");
}

/// A script at the subscription cap restarts into a working one: the old process's records
/// go before the cap is counted, so the fresh script's first subscribe is taken and its
/// first state arrives.
#[test]
fn a_script_at_the_subscription_cap_restarts_into_a_working_one() {
    let mut fixture = Fixture::claiming("full64", SCRIPT_64_SUBSCRIBES);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.pump_until(|f| written_contains(f, [0xB1, 0, 99]), "the first run's state");
    let first = fixture.midi.written_for("APC out").iter().filter(|m| m.as_slice() == [0xB1, 0, 99]).count();
    assert_eq!(first, 1, "{first} states");
    fixture.projects.execute("extensions.restart", &json!({ "id": fixture.id })).expect("restarted");
    fixture.expect(|e| *e == programs::Event::Running, "running again");
    fixture.pump_until(
        |f| f.midi.written_for("APC out").iter().filter(|m| m.as_slice() == [0xB1, 0, 99]).count() > 1,
        "the restarted script's first state",
    );
}

/// A script stopped at its error boundary holds no ports; a restart brings both back.
#[test]
fn a_script_that_stops_at_its_boundary_lets_its_ports_go() {
    let mut fixture = Fixture::claiming("broken", SCRIPT_ERROR);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    assert!(fixture.midi.raw_open("APC in"));
    fixture.midi.play("APC in", 1, &[0x90, 60, 100]);
    fixture.expect(|e| matches!(e, programs::Event::ScriptError(_)), "the script error");
    fixture.projects.poll_plugins();
    assert!(!fixture.midi.raw_open("APC in"), "the ports fall back while the script is stopped");
    assert!(fixture.midi.live_open("APC in"));
    fixture.projects.execute("extensions.restart", &json!({ "id": fixture.id })).expect("restarted");
    fixture.expect(|e| *e == programs::Event::Running, "running again");
    fixture.projects.poll_plugins();
    assert!(fixture.midi.raw_open("APC in"), "the restart claims the ports again");
}

/// The raw ports held open, with the sink each plays into.
type RawOpen = Arc<Mutex<Vec<(String, Arc<dyn RawSink>)>>>;
/// What programs wrote to the outputs, by port name, in order.
type Writes = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// The script's command runs the real one and is answered; its read answers the value. One
/// note in, the tempo set to 110 and read back out on a pad. Each stage of the round trip is
/// waited for on the supervisor's signal, each poll the one step that services it.
#[test]
fn a_programs_command_runs_and_its_read_answers() {
    let mut fixture = Fixture::claiming("commands", SCRIPT_COMMAND);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    fixture.peek(|e| matches!(e, programs::Event::Command { .. }));
    fixture.projects.poll_plugins();
    fixture.peek(|e| matches!(e, programs::Event::Read { .. }));
    fixture.projects.poll_plugins();
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    // The controller byte says the command ran; the value byte is the grid it read back: the
    // blank project's four tracks.
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 0x01, 4]]);
    // The tempo really changed on the project the session runs.
    let session = fixture.projects.session_mut(None).expect("the session");
    assert_eq!(session.project().transport.tempo_bpm, 110.0);
}

/// A command that does not exist is refused in the answer, and the script carries on.
#[test]
fn a_refused_command_says_so_in_its_answer() {
    let mut fixture = Fixture::claiming("refused", SCRIPT_REFUSED);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    fixture.peek(|e| matches!(e, programs::Event::Command { .. }));
    fixture.projects.poll_plugins();
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    // The controller byte says the command was refused; the script is not at its boundary.
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 0x00, 0x00]]);
    assert!(fixture.midi.raw_open("APC in"), "the program kept its ports");
}

/// A subscription's first state comes after the subscribe; the next when the project changes.
#[test]
fn a_subscription_pushes_its_first_state_and_the_next_change() {
    let mut fixture = Fixture::claiming("watching", SCRIPT_SUBSCRIBE);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // The subscribe was made as the script loaded: the first look reads and pushes, and the
    // state's handler sends — the peek waits for it on the supervisor's signal.
    fixture.projects.poll_plugins();
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 0x01, 0x01]]);
    // The project changes; the next look pushes again. Both states carried their token first.
    fixture.projects.execute("track.add", &json!({ "kind": "audio" })).expect("the track adds");
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 2);
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 0x01, 0x01], vec![0xB1, 0x02, 0x01]]);
}

/// A fader storm: one gesture, twelve values queued back to back. The volume lands at the
/// last value's dB, the earlier commands are answered (the runner carries on — a dropped
/// command's handler would wedge it otherwise), and the program is alive after.
#[test]
fn a_fader_storm_lands_at_its_last_value() {
    let mut fixture = Fixture::claiming("fader", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    let grid = fixture.projects.execute("project.launcher", &json!({})).expect("read");
    let id = grid["tracks"][0]["track"].as_u64().expect("the track's id");
    for value in 0..12 {
        let db = -60.0 + (value as f64) * 5.5;
        fixture.projects.test_extensions_program().expect("running").test_command(
            value + 1,
            "track.setVolume",
            &json!({ "tracks": [id], "db": db }),
            Some("apc40-mkii volume"),
        );
    }
    // One look drains the whole storm: the last command dispatches, the others are answered.
    fixture.projects.poll_plugins();
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["volume_db"], json!(0.5), "{tracks}");
    // The program is alive after the storm, and its handlers all completed: a later press
    // still dispatches (a wedged runner would hang here instead).
    fixture.projects.test_extensions_program().expect("running").test_command(
        99,
        "track.setVolume",
        &json!({ "tracks": [id], "db": 2.5 }),
        Some("apc40-mkii volume"),
    );
    fixture.projects.poll_plugins();
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["volume_db"], json!(2.5), "{tracks}");
}

/// One gesture on two tracks is two streams: both commands dispatch, so both tracks land at
/// the value — the first is not swallowed by the second's answer.
#[test]
fn one_gesture_on_two_tracks_sets_both() {
    let mut fixture = Fixture::claiming("twotargets", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    for (token, track) in [(1, 1), (2, 2)] {
        fixture.projects.test_extensions_program().expect("running").test_command(
            token,
            "track.setVolume",
            &json!({ "tracks": [track], "db": -10.0 }),
            Some("one drag"),
        );
    }
    fixture.projects.poll_plugins();
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["volume_db"], json!(-10.0), "{tracks}");
    assert_eq!(tracks[1]["volume_db"], json!(-10.0), "{tracks}");
}

/// An adjust adds: its calls run each their own, whatever gesture the protocol named — the
/// command takes no `gesture` parameter, so two +1 dB moves are +2 dB, not one.
#[test]
fn an_adjusts_calls_run_each_their_own() {
    let mut fixture = Fixture::claiming("adds", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    for token in 1..=2 {
        fixture.projects.test_extensions_program().expect("running").test_command(
            token,
            "track.adjustVolume",
            &json!({ "tracks": [1], "db": 1.0 }),
            Some("one drag"),
        );
    }
    fixture.projects.poll_plugins();
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["volume_db"], json!(2.0), "{tracks}");
}

/// A read is a reading command: an edit asked through `suhina.read` is refused with its
/// reason, and the volume it would have set is unchanged.
#[test]
fn a_read_of_an_edit_is_refused() {
    let mut fixture = Fixture::claiming("readedit", SCRIPT_READ_EDIT);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    fixture.pump_until(|f| written_contains(f, [0xB1, 0x01, 0x00]), "the refusal's marker");
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["volume_db"], json!(0.0), "{tracks}");
}

/// A subscription is a reading command too: one to an edit is registered but its reads are
/// refused, so no state is pushed — the edit does not happen on every look.
#[test]
fn a_subscription_of_an_edit_pushes_nothing() {
    let mut fixture = Fixture::claiming("subedit", SCRIPT_SUBSCRIBE_EDIT);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // The looks the subscription's reads would have run on: no state is pushed.
    for _ in 0..200 {
        fixture.projects.poll_plugins();
        if let Err(reason) = fixture.projects.test_extensions_breath() {
            panic!("the program ended: {reason}");
        }
    }
    assert!(fixture.midi.written_for("APC out").is_empty(), "{:?}", fixture.midi.written_for("APC out"));
}

/// The send-back queue's byte budget holds: the queue never holds more than its eight MiB,
/// whatever the runner sends — the drop is said on the extension, and what a drain writes
/// out is within the budget.
#[test]
fn the_send_back_queue_holds_at_most_eight_mib() {
    let mut fixture = Fixture::claiming("bigsend", SCRIPT_BIG_SEND);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    // Twenty one-MiB sends with no drain between them: the eighth queues, and the ninth's
    // enqueue — the first past the byte budget — is the drop the wait is on.
    for _ in 0..20 {
        fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    }
    fixture.expect(|e| matches!(e, programs::Event::Dropped(_)), "the byte budget's drop");
    fixture.projects.poll_plugins();
    let written = fixture.midi.written_for("APC out");
    let bytes: usize = written.iter().map(|m| m.len()).sum();
    assert!(written.len() <= 8, "{} send-backs written", written.len());
    assert!(bytes <= 8 * 1024 * 1024, "{bytes} bytes written");
    let listed = fixture.projects.execute("extensions.list", &json!({})).expect("listed");
    let entry = find(&listed, &fixture.id).expect("listed");
    let error = entry["lastError"].as_str().expect("the drop is said");
    assert!(error.contains("dropped"), "{error}");
}

/// A dead process's answer never completes the replacement's wait: the stale command's
/// answer is queued while the restart's fresh script happens to wait on the same token
/// number, and what reaches it is the answer of its own command — the pad says which.
#[test]
fn a_dead_processs_answer_does_not_complete_the_replacements_wait() {
    let mut fixture = Fixture::claiming("stale", SCRIPT_TWO_COMMANDS);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    // The press whose answer will be the stale one: queued, never dispatched.
    fixture.midi.play("APC in", 1, &[0x90, 0x3C, 0x7F]);
    // The process dies and the supervisor starts another one.
    let pid = fixture.pid();
    let sent = fixture.projects.test_extensions_program().expect("running").test_kill(pid);
    assert!(sent, "the kill was sent");
    fixture.expect(|e| matches!(e, programs::Event::Crashed(_)), "the crash");
    fixture.expect(|e| *e == programs::Event::Running, "running again");
    // The fresh script's press, queued behind the stale event: one look dispatches both.
    fixture.midi.play("APC in", 1, &[0x90, 0x3D, 0x7F]);
    fixture.projects.poll_plugins();
    // The fresh wait heard its own command's answer — the stale one is not its answer.
    fixture.pump_until(|f| written_contains(f, [0xB2, 0x01, 0x00]), "the fresh command's ok");
    let written = fixture.midi.written_for("APC out");
    assert!(!written.iter().any(|m| m.as_slice() == [0xB2, 0x00, 0x00]), "{written:?}");
}

/// The storm through the real pipeline: the CCs arrive through the supervisor, the commands
/// queue, and the session coalesces each look. The volume lands at the last value's dB and the
/// program stays alive. The test pumps the frame while the pipeline catches up — each pump
/// services the session, and the assertion (the landed volume) does not depend on when the
/// threads run.
#[test]
fn a_fader_storm_through_the_pipeline_lands_at_its_last_value() {
    let mut fixture = Fixture::claiming("pipeline", SCRIPT_FADER_PIPELINE);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.poll_plugins();
    // The storm: 100 CCs of one gesture, injected as fast as the fake device can.
    for value in 0..100 {
        let cc = 64 + (value % 64);
        fixture.midi.play("APC in", 1, &[0xB0, 0x07, cc]);
    }
    // The pipeline is done when the last CC's value marker comes out on the output port: the
    // script sends it after its command came back, so the newest value has landed. What
    // landed is the oracle: CC 99 is -60 + 99/127 of the fader's range, -8.6 dB — a queue
    // check could not be waited on, being drained by the very look that writes it out. The
    // storm's byte 99 plays twice (values 35 and 99), so the marker is waited for twice:
    // the second is the last handler's, and the handlers run in the messages' order, so
    // everything ahead of it is done.
    let markers = |fixture: &Fixture| {
        fixture.midi.written_for("APC out").iter().filter(|m| m.as_slice() == [0xB1, 99, 1]).count()
    };
    fixture.pump_until(|f| markers(f) >= 1, "the last CC's marker");
    fixture.pump_until(|f| markers(f) >= 2, "the last CC's marker again");
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["volume_db"], json!(-8.6), "{tracks}");
}

/// A controller's command wakes the window when it queues, the way the plug-in host wakes it
/// for unasked news (#714): while nothing plays the window asks for no frames of its own, and
/// without the wake the command would sit until the background repaints — the fader's lag when
/// the transport is stopped. The window's waker is the process's one, so a parallel test's
/// events count too; the assertion can only ever pass early, never fail a working wake.
#[test]
fn a_programs_command_wakes_the_window() {
    let woken = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    suhina_plugins::set_waker({
        let woken = std::sync::Arc::clone(&woken);
        move || {
            woken.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });
    let mut fixture = Fixture::claiming("pipeline", SCRIPT_FADER_PIPELINE);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    let before = woken.load(std::sync::atomic::Ordering::Relaxed);
    fixture.midi.play("APC in", 1, &[0xB0, 0x07, 64]);
    // The pipeline is done when the value marker comes out on the output port: the script
    // sends it after its command came back, so the volume has landed. CC 64 is -60 + 64/127
    // of the fader's range, -26.8 dB. The landed volume is the oracle: the runner's marker
    // for the command is drained by the very look that writes it out, so a queue check would
    // depend on the order the threads ran in; the wait itself is on the supervisor's signal.
    fixture.pump_until(|f| written_contains(f, [0xB1, 64, 1]), "the CC's marker");
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["volume_db"], json!(-26.8), "{tracks}");
    let after = woken.load(std::sync::atomic::Ordering::Relaxed);
    assert!(after > before, "the command's queueing woke no frame");
}

/// The fader through the pipeline: each CC of one gesture runs the volume command and marks
/// its value back, so a test can wait for the storm's last one.
const SCRIPT_FADER_PIPELINE: &str = r#"
suhina.on_port_message(function(bytes, port, timestamp)
    local message = suhina.midi.parse(bytes)
    if message ~= nil and message.kind == "cc" and message.controller == 7 then
        local value = message.value
        -- The entry marker (controller 0) says the handler ran; the value marker (after the
        -- command) says the command came back.
        suhina.send("APC out", suhina.midi.cc(1, 0, 1))
        local db = "-inf"
        if value > 0 then db = math.floor((-60 + (value / 127) * 66) * 10) / 10 end
        local ok, err = suhina.command("track.setVolume", { tracks = { 1 }, db = db }, "apc40-mkii volume")
        suhina.send("APC out", suhina.midi.cc(1, value, ok and 1 or 0))
    end
end)
"#;
/// A controller's toggle presses are standalone commands: two of the same press inside one
/// look both dispatch, and the session ends at the second press's value. The storm coalescer
/// only merges gestured streams - dropping the middle of a toggle made the second press
/// vanish from the session while the hardware's LED (written at press time) had already
/// flipped.
const SCRIPT_TOGGLE: &str = r#"
local sent = false
suhina.on_port_message(function(bytes, port, timestamp)
    local message = suhina.midi.parse(bytes)
    if message ~= nil and message.kind == "note_on" and message.velocity > 0 and message.note == 49 then
        sent = not sent
        local ok = suhina.command("track.setSolo", { tracks = { 1 }, solo = sent })
        -- The marker says the press's command came back: the controller byte is the state.
        suhina.send("APC out", suhina.midi.cc(1, sent and 1 or 0, ok and 1 or 0))
    end
end)
"#;

#[test]
fn toggle_presses_are_not_coalesced_away() {
    let mut fixture = Fixture::claiming("toggles", SCRIPT_TOGGLE);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    // Two presses inside one look; the wait is on the second press's marker (its command
    // came back with the solo off), not a frame count. The looks service the commands on
    // the way.
    fixture.midi.play("APC in", 1, &[0x90, 49, 127]);
    fixture.midi.play("APC in", 1, &[0x90, 49, 127]);
    fixture.pump_until(|f| written_contains(f, [0xB1, 0, 1]), "the second press's marker");
    // Both dispatched: the session ends at the second press's value, false.
    let tracks = fixture.projects.execute("project.tracks", &json!({})).expect("read");
    assert_eq!(tracks[0]["solo"], json!(false), "{tracks}");
}

/// Stand-in MIDI ports: a claimed input port plays into the sink it was opened raw for, and a port that vanished and came back re-enumerates, and its
/// connection is reopened fresh — the stale one died with the device, and the claim by token
/// would otherwise keep holding it.
#[test]
fn a_returned_port_is_reopened_fresh() {
    let mut fixture = Fixture::claiming("replug", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    // The store may have started the program itself when the fixture made the device
    // present, and the start by hand then replaced it — the count the replug is judged by
    // is the one now.
    let opens_at_start = fixture.midi.raw_opens("APC in");
    assert!(opens_at_start >= 1, "{opens_at_start} opens");
    // The device vanishes: the port leaves the system's list.
    fixture.midi.inputs.lock().unwrap().retain(|(p, _)| p != "APC in");
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    assert!(fixture.midi.was_closed("APC in"), "the port closed with the device");
    // The device comes back: the same name re-enumerates.
    fixture.midi.inputs.lock().unwrap().push(("APC in".into(), "APC in#2".into()));
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    assert_eq!(fixture.midi.raw_opens("APC in"), opens_at_start + 1, "reopened fresh");
    // And it still plays into the program.
    fixture.midi.play("APC in", 1, &[0x90, 60, 100]);
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    assert_eq!(fixture.midi.written_for("APC out").len(), 1);
}

/// A port that re-enumerates without ever leaving the system's list — the device blinked off
/// the bus and back inside one scan's window, so the name never went away — is reopened fresh
/// all the same: the system's unique id for the name changed, and the old connection died
/// with the device either way. The program's ports are told again, so its LED cache resets
/// and the lights come back without a restart by hand.
#[test]
fn a_port_that_re_enumerates_under_the_scan_is_reopened_fresh() {
    let mut fixture = Fixture::claiming("blink", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    // The count the blink is judged by is the one now: the store may have started the
    // program itself when the fixture made the device present, and the start by hand then
    // replaced it.
    let opens_at_start = fixture.midi.raw_opens("APC in");
    assert!(opens_at_start >= 1, "{opens_at_start} opens");
    // The blink: the same name, the system's id for it renewed.
    *fixture.midi.inputs.lock().unwrap() = vec![("APC in".into(), "APC in#9".into())];
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    assert_eq!(fixture.midi.raw_opens("APC in"), opens_at_start + 1, "reopened without the name ever leaving");
    // And the fresh connection plays into the program, whose ports were told again.
    fixture.midi.play("APC in", 1, &[0x90, 60, 100]);
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    fixture.midi.await_written("APC out", 1);
    assert_eq!(fixture.midi.written_for("APC out").len(), 1);
}

/// Stand-in MIDI ports: a claimed input port plays into the sink it was opened raw for, and
/// what a program writes to an output port is recorded with the port's name.
#[derive(Default)]
struct Midi {
    inputs: Arc<Mutex<Vec<(String, String)>>>,
    outputs: Arc<Mutex<Vec<(String, String)>>>,
    /// The input ports opened raw for a program now, with the sink each plays into.
    raw: RawOpen,
    /// Every raw open, ever: a restart opens the port again for the new process.
    raw_log: Arc<Mutex<Vec<String>>>,
    /// The input ports opened the usual way (for the armed tracks) now.
    live: Arc<Mutex<Vec<String>>>,
    /// Every port closed, however it was opened.
    closed: Arc<Mutex<Vec<String>>>,
    /// What programs wrote to the outputs, by port name, in order.
    written: Writes,
    /// Signalled for every write, so the tests wait on the writes' arrival instead of
    /// polling the log.
    signal: Arc<Condvar>,
}

/// A port held open: dropping it closes the port, taking it out of `open` and logging it.
struct Keep {
    name: String,
    open: Arc<Mutex<Vec<String>>>,
    closed: Arc<Mutex<Vec<String>>>,
}

impl Drop for Keep {
    fn drop(&mut self) {
        self.open.lock().unwrap().retain(|p| p != &self.name);
        self.closed.lock().unwrap().push(self.name.clone());
    }
}

/// A raw port held open: the same, over the (port, sink) list.
struct RawKeep {
    name: String,
    open: RawOpen,
    closed: Arc<Mutex<Vec<String>>>,
}

impl Drop for RawKeep {
    fn drop(&mut self) {
        self.open.lock().unwrap().retain(|(p, _)| p != &self.name);
        self.closed.lock().unwrap().push(self.name.clone());
    }
}

/// An output port a program writes to: the bytes recorded with the port's name. The session's
/// writes are the worker's — the record signals the waiters, so a test's `await_written` is
/// not a poll.
struct Out {
    name: String,
    written: Writes,
    signal: Arc<Condvar>,
}

impl MidiOut for Out {
    fn send(&self, bytes: &[u8]) -> Result<(), String> {
        let mut written = self.written.lock().unwrap();
        written.push((self.name.clone(), bytes.to_vec()));
        self.signal.notify_all();
        Ok(())
    }
}

impl MidiSystem for Midi {
    fn ports(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self.inputs.lock().unwrap().clone())
    }

    fn outputs(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self.outputs.lock().unwrap().clone())
    }

    fn open(&self, name: &str, _: Arc<LiveInput>) -> Result<Box<dyn std::any::Any + Send>, String> {
        self.live.lock().unwrap().push(name.to_string());
        Ok(Box::new(Keep { name: name.to_string(), open: Arc::clone(&self.live), closed: Arc::clone(&self.closed) }))
    }

    fn open_raw(&self, name: &str, sink: Arc<dyn RawSink>) -> Result<Box<dyn std::any::Any + Send>, String> {
        self.raw.lock().unwrap().push((name.to_string(), sink));
        self.raw_log.lock().unwrap().push(name.to_string());
        Ok(Box::new(RawKeep { name: name.to_string(), open: Arc::clone(&self.raw), closed: Arc::clone(&self.closed) }))
    }

    fn open_output(&self, name: &str) -> Result<Box<dyn MidiOut>, String> {
        Ok(Box::new(Out {
            name: name.to_string(),
            written: Arc::clone(&self.written),
            signal: Arc::clone(&self.signal),
        }))
    }
}

impl Midi {
    /// A claimed port plays, as the driver would: the whole message to its sink.
    fn play(&self, port: &str, stamp: u64, bytes: &[u8]) {
        let raw = self.raw.lock().unwrap();
        let sink = raw
            .iter()
            .find(|(name, _)| name == port)
            .map(|(_, sink)| Arc::clone(sink))
            .unwrap_or_else(|| panic!("the port {port} is open raw"));
        sink.message(stamp, bytes);
    }

    fn raw_open(&self, port: &str) -> bool {
        self.raw.lock().unwrap().iter().any(|(name, _)| name == port)
    }

    fn raw_list(&self) -> Vec<String> {
        self.raw.lock().unwrap().iter().map(|(name, _)| name.clone()).collect()
    }

    fn live_open(&self, port: &str) -> bool {
        self.live.lock().unwrap().iter().any(|p| p == port)
    }

    fn was_closed(&self, port: &str) -> bool {
        self.closed.lock().unwrap().iter().any(|p| p == port)
    }

    fn raw_opens(&self, port: &str) -> usize {
        self.raw_log.lock().unwrap().iter().filter(|p| *p == port).count()
    }

    fn written_for(&self, port: &str) -> Vec<Vec<u8>> {
        self.written.lock().unwrap().iter().filter(|(name, _)| name == port).map(|(_, bytes)| bytes.clone()).collect()
    }

    /// Waits for the output port's write count to reach `count`, on the writes' signal —
    /// the session's writes are the worker's, so the log is read once the write landed.
    fn await_written(&self, port: &str, count: usize) {
        let mut written = self.written.lock().unwrap();
        while written.iter().filter(|(name, _)| name == port).count() < count {
            written = self.signal.wait(written).unwrap();
        }
    }
}
