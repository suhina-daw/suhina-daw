// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The surface-program supervisor end to end (`docs/spec/surface-programs.md` › Testing): the
//! real runner binary as a child of the real supervisor, driven through a [`Projects`]. The
//! waits are on the events the supervisor reports, never on a wall-clock while. The tests live
//! beside the runner they spawn, so the binary under them is the one just built — a
//! session-only test run would spawn whatever a previous workspace build left behind.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)] // test helpers outside #[test] functions

use std::sync::{Arc, Mutex};

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
            inputs: Arc::new(Mutex::new(inputs.iter().map(|n| n.to_string()).collect())),
            outputs: Arc::new(Mutex::new(outputs.iter().map(|n| n.to_string()).collect())),
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
    /// drain applies the states, so `extensions.list` sees them too.
    fn expect(&mut self, which: impl Fn(&programs::Event) -> bool, what: &str) -> programs::Event {
        self.projects.test_extensions_wait_for(which).unwrap_or_else(|| panic!("the program ended without {what}"))
    }

    /// Waits until the queue holds the event, leaving it there for the frame's drain — which
    /// is what writes a send-back out to its port.
    fn peek(&mut self, which: impl Fn(&programs::Event) -> bool) {
        self.projects.test_extensions_peek_for(which);
        // The wait returns only when the event is there; a program that died first would have
        // said so, and the wait would not have returned.
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

fn find(listed: &Value, id: &str) -> Option<Value> {
    listed["extensions"].as_array()?.iter().find(|e| e["id"] == id).cloned()
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
    assert_eq!(fixture.midi.raw_opens("APC in"), 1);
    fixture.projects.execute("extensions.restart", &json!({ "id": fixture.id })).expect("restarted");
    fixture.expect(|e| *e == programs::Event::Running, "running again");
    fixture.projects.poll_plugins();
    // The port was opened again for the new process: one close, one new open.
    assert_eq!(fixture.midi.raw_opens("APC in"), 2);
    assert!(fixture.midi.was_closed("APC in"));
    fixture.midi.play("APC in", 7, &[0xB0, 7, 1]);
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    assert_eq!(fixture.midi.written_for("APC out").len(), 1, "{:?}", fixture.midi.written_for("APC out"));
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
    assert_eq!(fixture.midi.written_for("APC out"), vec![vec![0xB1, 0x01, 0x01]]);
    // The project changes; the next look pushes again. Both states carried their token first.
    fixture.projects.execute("track.add", &json!({ "kind": "audio" })).expect("the track adds");
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
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

/// The device replugged: a port that vanished and came back re-enumerates, and its
/// connection is reopened fresh — the stale one died with the device, and the claim by token
/// would otherwise keep holding it.
#[test]
fn a_returned_port_is_reopened_fresh() {
    let mut fixture = Fixture::claiming("replug", SCRIPT_ECHO);
    fixture.start();
    fixture.expect(|e| *e == programs::Event::Running, "running");
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    assert_eq!(fixture.midi.raw_opens("APC in"), 1);
    // The device vanishes: the port leaves the system's list.
    fixture.midi.inputs.lock().unwrap().retain(|p| p != "APC in");
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    assert!(fixture.midi.was_closed("APC in"), "the port closed with the device");
    // The device comes back: the same name re-enumerates.
    fixture.midi.inputs.lock().unwrap().push("APC in".to_string());
    fixture.projects.execute("midi.inputs", &json!({})).expect("listed");
    assert_eq!(fixture.midi.raw_opens("APC in"), 2, "reopened fresh");
    // And it still plays into the program.
    fixture.midi.play("APC in", 1, &[0x90, 60, 100]);
    fixture.peek(|e| matches!(e, programs::Event::SendBack { .. }));
    fixture.projects.poll_plugins();
    assert_eq!(fixture.midi.written_for("APC out").len(), 1);
}

/// Stand-in MIDI ports: a claimed input port plays into the sink it was opened raw for, and
/// what a program writes to an output port is recorded with the port's name.
#[derive(Default)]
struct Midi {
    inputs: Arc<Mutex<Vec<String>>>,
    outputs: Arc<Mutex<Vec<String>>>,
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

/// An output port a program writes to: the bytes recorded with the port's name.
struct Out {
    name: String,
    written: Writes,
}

impl MidiOut for Out {
    fn send(&self, bytes: &[u8]) -> Result<(), String> {
        self.written.lock().unwrap().push((self.name.clone(), bytes.to_vec()));
        Ok(())
    }
}

impl MidiSystem for Midi {
    fn ports(&self) -> Result<Vec<String>, String> {
        Ok(self.inputs.lock().unwrap().clone())
    }

    fn outputs(&self) -> Result<Vec<String>, String> {
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
        Ok(Box::new(Out { name: name.to_string(), written: Arc::clone(&self.written) }))
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
}
