// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The surface-program supervisor end to end (`docs/spec/surface-programs.md` › Testing): the
//! real runner binary as a child of the real supervisor, driven through a [`Projects`]. The
//! waits are on the events the supervisor reports, never on a wall-clock while.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unreachable)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_session::extensions::programs;

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// An extension in a fresh store, sealed against its script, and a `Projects` over the store.
/// `projects` is dropped before `root` on purpose: the programs stop, and only then does the
/// store's folder go.
struct Fixture {
    projects: Projects,
    id: String,
    root: std::path::PathBuf,
}

use suhina_session::Projects;

const SCRIPT_OK: &str =
    "suhina.on_port_message(function(bytes, port) suhina.send(\"Pad\", suhina.midi.cc(1, 7, 100)) end)\n";
const SCRIPT_ERROR: &str = "suhina.on_port_message(function() error(\"pad broke\") end)\n";

impl Fixture {
    /// Makes `id` with `script` as its whole script, and a `Projects` whose extensions store
    /// is the only folder there.
    fn new(id: &str, script: &str) -> Self {
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
            "ports": ["Pad*"],
            "runner": "lua",
            "permissions": ["project.read"],
        });
        std::fs::write(root.join(id).join("extension.json"), manifest.to_string()).expect("the manifest is written");
        let projects = Projects::default().with_extensions_root(&root);
        Self { root, id: id.to_string(), projects }
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
