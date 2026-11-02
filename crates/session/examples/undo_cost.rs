// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What one edit and its undo step cost on a heavy project (#763 › 3): eight tracks with 4 MiB of
//! plug-in state each and eight clips of 100 000 notes, then 256 renames and 256 undos.
//!
//! ```sh
//! cargo run --release -p suhina-session --example undo_cost
//! ```
//!
//! A measurement to read, not a test: it prints times and the process's memory.

use std::time::Instant;

use serde_json::json;
use suhina_model::{Clip, ContentId, Device, DeviceKind, Extensions, Note};
use suhina_session::Session;
use suhina_test_support::ProjectBuilder;

const TRACKS: u32 = 8;
const STATE_BYTES: usize = 4 << 20;
const NOTES: usize = 100_000;
const EDITS: usize = 256;

fn main() {
    let mut builder = ProjectBuilder::new();
    for id in 1..=TRACKS {
        let notes = (0..NOTES).map(|n| Note::new(n as f64 * 0.25, 0.25, 36 + (n % 48) as u8, 0.8)).collect();
        let length = NOTES as f64 * 0.25;
        builder = builder
            .midi_track(id, &format!("Track {id}"))
            .notes(id, &format!("Clip {id}"), length, notes)
            .clip(id, Clip::new(ContentId(id), 0.0, length))
            .with(|project| {
                if let Some(channel) = project.channels.iter_mut().find(|c| c.id.0 == id) {
                    channel.devices.push(Device {
                        id: suhina_model::DeviceId::NONE,
                        name: format!("Plug-in {id}"),
                        kind: DeviceKind::Clap { id: format!("org.example.plugin{id}") },
                        enabled: true,
                        state: Some(vec![id as u8; STATE_BYTES].into()),
                        params: Vec::new(),
                        automation: Vec::new(),
                        clip_host: false,
                        extensions: Extensions::default(),
                    });
                }
            });
    }
    let mut session = Session::new(builder.build());
    let rss_start = rss_mib();

    let start = Instant::now();
    for n in 0..EDITS {
        let track = (n as u32 % TRACKS) + 1;
        if let Err(e) = session.execute("track.rename", &json!({"track": track, "name": format!("Edit {n}")})) {
            eprintln!("edit {n} failed: {e:?}");
            return;
        }
    }
    let edits = start.elapsed();

    let start = Instant::now();
    for n in 0..EDITS {
        if let Err(e) = session.execute("edit.undo", &json!({})) {
            eprintln!("undo {n} failed: {e:?}");
            return;
        }
    }
    let undos = start.elapsed();

    println!("{EDITS} edits: {:.3} ms each", edits.as_secs_f64() * 1e3 / EDITS as f64);
    println!("{EDITS} undos: {:.3} ms each", undos.as_secs_f64() * 1e3 / EDITS as f64);
    println!("resident memory: {rss_start:.0} MiB after loading, {:.0} MiB after the edits and undos", rss_mib());
}

/// The process's resident memory in MiB, from `ps` (0 where it cannot be read).
fn rss_mib() -> f64 {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map_or(0.0, |kib| kib / 1024.0)
}
