// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every test of the window and its views, in one binary: one module per file in this folder.
//!
//! One binary is linked once, where a binary per file linked 30 times (most of this crate's
//! test build time). `autotests = false` in `Cargo.toml` keeps Cargo from building each file on
//! its own as well, so a new file is listed here; `every_test_file_is_a_module` fails until it is.
//! A test that needs a process of its own (`trace_actions.rs`: the trace file is one per process)
//! stays a binary of its own, a `[[test]]` of its own in `Cargo.toml`.

mod agents;
mod arrangement_editing;
mod auto_arm;
mod automation_lanes;
mod browser;
mod browser_preview;
mod detail_devices;
mod follow;
mod launch_keys;
mod meters;
mod mixer_strip;
mod open_recent;
mod piano_roll;
mod piano_roll_fixes;
mod piano_roll_gestures;
mod piano_roll_ghosts;
mod piano_roll_grid;
mod piano_roll_quantize;
mod piano_roll_ruler;
mod piano_roll_scale;
mod piano_roll_step4;
mod piano_roll_tools;
mod piano_roll_workflow;
mod recording;
mod session_menus;
mod settings_window;
mod short_panes;
mod slot_launch;
mod temp_dir;
mod track_colour;
mod waveforms;
mod window;

#[test]
fn every_test_file_is_a_module() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let listed = include_str!("main.rs").replace('\r', "");
    let own_binaries = include_str!("../Cargo.toml").replace('\r', "");
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(&folder).unwrap() {
        let path = entry.unwrap().path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if path.extension().is_some_and(|e| e == "rs")
            && stem != "main"
            && !listed.contains(&format!("\nmod {stem};\n"))
            && !own_binaries.contains(&format!("path = \"tests/{stem}.rs\""))
        {
            missing.push(stem.to_string());
        }
    }
    assert!(missing.is_empty(), "add `mod NAME;` to tests/main.rs (or a `[[test]]` to Cargo.toml) for: {missing:?}");
}
