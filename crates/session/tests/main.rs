// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every test of the session's commands, in one binary: one module per file in this folder.
//!
//! One binary is linked once, where a binary per file linked 40 times (most of this crate's
//! test build time). `autotests = false` in `Cargo.toml` keeps Cargo from building each file on
//! its own as well, so a new file is listed here; `every_test_file_is_a_module` fails until it is.

mod audio;
mod audio_input;
mod automation;
mod clip_color;
mod clip_edges;
mod clip_shape;
mod commands;
mod cue;
mod device_groups;
mod devices;
mod editing;
mod export;
mod files;
mod follow;
mod ids;
mod launch_mappings;
mod launch_settings;
mod launcher;
mod library;
mod meters;
mod midi;
mod note_gestures;
mod note_quantize;
mod note_tools;
mod notes;
mod plugins;
mod projects;
mod range;
mod recording;
mod relink;
mod routing;
mod sampler;
mod save;
mod scales;
mod scenes;
mod sends;
mod soundmap;
mod stems;
mod support;
mod temp_dir;
mod tempo;
mod trusted_audio;

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
