// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use serde_json::json;

use super::*;

fn vst3(id: &str, name: &str, vendor: &str, replaces: &[&str]) -> serde_json::Value {
    json!({ "id": id, "name": name, "vendor": vendor, "version": "1", "features": [], "replaces": replaces })
}

fn catalog() -> Catalog {
    let entry = |file: &str, plugin: serde_json::Value| json!({ "path": PathBuf::from(file).display().to_string(), "modified": 1, "size": 1, "plugins": [plugin] });
    let text = json!({ "version": 2, "entries": [
            entry("/vst3/Room.vst3", vst3(&vst2_class_id(0x4861_6C52, "HallRoom"), "HallRoom", "Hall DSP, LLC", &[])),
            entry("/vst3/Bell-Q 3.vst3", vst3("11111111111111111111111111111103", "Bell-Q 3", "Acme",
                &["5653544251337061636D65706C756769"])),
            entry("/vst3/Bell-Q 4.vst3", vst3("11111111111111111111111111111104", "Bell-Q 4", "Acme", &[])),
            entry("/vst3/Wavey2.vst3", vst3("56534557617650776176657920320000", "Wavey 2", "Wave Works", &[])),
            entry("/vst3/Heater.vst3", vst3("22222222222222222222222222222201", "XY OldHeater", "XYaudio.example", &[])),
            entry("/vst3/Heater2.vst3", vst3("22222222222222222222222222222202", "XY OldHeater2", "XYaudio.example", &[])),
            entry("/clap/thing.clap", json!({ "id": "com.example.thing", "name": "Thing", "vendor": "Example" })),
            entry("/vst3/Suite 11 Imager.vst3", vst3("5653545375493173756974652050726F", "Suite 11 Imager", "Studio Maker", &[])),
            entry("/vst3/Suite 11.vst3", vst3("56535453754932737569746520313100", "Suite 11", "Studio Maker", &[])),
            entry("/vst3/Orbit.vst3", json!({ "id": "33333333333333333333333333333301", "name": "Orbit", "vendor": "Zeta",
                "version": "5.1.0", "features": ["Instrument"], "replaces": [] })),
        ]})
        .to_string();
    Catalog::from_json(&text)
}

fn found(m: Option<Match<'_>>) -> Option<(&str, How)> {
    m.map(|m| (m.plugin.name.as_str(), m.how))
}

/// A catalog with the scanned VST2 plug-in `id` named `name`, and optionally a VST3
/// stand-in for it.
fn vst2_catalog(id: u32, name: &str, stand_in: bool) -> Catalog {
    let file = |file: &str, plugin: serde_json::Value| json!({ "path": PathBuf::from(file).display().to_string(), "modified": 1, "size": 1, "plugins": [plugin] });
    // The VST2 file shape of this system (a `.vst` bundle on macOS, a `.dll` on Windows, a
    // `.so` on Linux), the one `format()` reads it as.
    let shape = if cfg!(target_os = "macos") {
        "Gain.vst"
    } else if cfg!(windows) {
        "Gain.dll"
    } else {
        "Gain.so"
    };
    let mut entries = vec![file(
        format!("/vst2/{shape}").as_str(),
        json!({ "id": format!("{id:08X}"), "name": name, "vendor": "Suhina", "version": "1", "features": ["audio-effect"], "replaces": [] }),
    )];
    if stand_in {
        entries.push(file("/vst3/Gain.vst3", vst3(&vst2_class_id(id, name), name, "Suhina", &[])));
    }
    let text = json!({ "version": 2, "entries": entries }).to_string();
    Catalog::from_json(&text)
}

/// A project's VST2 plug-in that no VST3 or CLAP copy stands in for is hosted itself.
#[test]
fn a_vst2_plug_in_without_a_standing_in_copy_runs_itself() {
    let (id, name) = (0x5368_5447, "Suhina Test Gain");
    let catalog = vst2_catalog(id, name, false);
    let matched = catalog.match_vst2(id, name);
    assert_eq!(found(matched), Some(("Suhina Test Gain", How::Itself)));
    assert!(matched.is_some_and(|m| m.how.keeps_state()), "the state is its own form");
    assert_eq!(matched.map(|m| m.plugin.format()), Some("vst2"), "the direct host loads it");
}

/// The plug-in itself, when it is installed, plays its project before a VST3 whose class id
/// only follows the id convention (ADR 0006 decision 8, Amendment: the VST2 itself first): such a
/// copy promises nothing about reading the saved state. Without the plug-in itself, the copy
/// plays it.
#[test]
fn an_installed_vst2_plug_in_comes_before_a_copy_found_by_the_id_convention() {
    let (id, name) = (0x5368_5448, "Suhina Test Gain");
    let catalog = vst2_catalog(id, name, true);
    let matched = catalog.match_vst2(id, name);
    assert_eq!(found(matched).map(|(_, how)| how), Some(How::Itself));
    assert_eq!(matched.map(|m| m.plugin.format()), Some("vst2"));
    let copy_only = json!({ "version": 2, "entries": [json!({
        "path": PathBuf::from("/vst3/Gain.vst3").display().to_string(), "modified": 1, "size": 1,
        "plugins": [vst3(&vst2_class_id(id, name), name, "Suhina", &[])],
    })] });
    let catalog = Catalog::from_json(&copy_only.to_string());
    let matched = catalog.match_vst2(id, name);
    assert_eq!(found(matched).map(|(_, how)| how), Some(How::Vst2Id));
    assert_eq!(matched.map(|m| m.plugin.format()), Some("vst3"));
}

/// The exact VST2 plug-in is installed, and VST3s only match it by name: those promise nothing
/// about its state, so the plug-in itself plays it.
#[test]
fn the_exact_vst2_plug_in_comes_before_a_vst3_found_by_name() {
    let (id, name) = (0x5368_5449, "Suhina Synth");
    let file = |file: &str, plugin: serde_json::Value| json!({ "path": PathBuf::from(file).display().to_string(), "modified": 1, "size": 1, "plugins": [plugin] });
    let shape = if cfg!(target_os = "macos") {
        "Synth.vst"
    } else if cfg!(windows) {
        "Synth.dll"
    } else {
        "Synth.so"
    };
    let vst2 = |id: u32| json!({ "id": format!("{id:08X}"), "name": name, "vendor": "Suhina", "version": "1", "features": [], "replaces": [] });
    let newer = file("/vst3/Synth2.vst3", vst3("0123456789ABCDEF0123456789ABCDEF", "Suhina Synth 2", "Suhina", &[]));
    let same = file("/vst3/Synth.vst3", vst3("FEDCBA9876543210FEDCBA9876543210", name, "Suhina", &[]));
    for other in [newer.clone(), same.clone()] {
        let text = json!({ "version": 2, "entries": [file(format!("/vst2/{shape}").as_str(), vst2(id)), other] });
        let catalog = Catalog::from_json(&text.to_string());
        assert_eq!(found(catalog.match_vst2(id, name)).map(|(_, how)| how), Some(How::Itself));
    }
    // Duplicate installation paths for the same product must not force a guessed replacement.
    let text = json!({ "version": 2, "entries": [
        file(format!("/vst2/{shape}").as_str(), vst2(id)),
        file(format!("/other/{shape}").as_str(), vst2(id)),
        same,
    ]});
    let catalog = Catalog::from_json(&text.to_string());
    assert_eq!(found(catalog.match_vst2(id, name)).map(|(_, how)| how), Some(How::Itself));
    // A genuine id collision stays ambiguous; do not pick either product by path order.
    let mut collision = text.clone();
    collision["entries"][1]["plugins"][0]["name"] = json!("Unrelated Synth");
    let catalog = Catalog::from_json(&collision.to_string());
    assert_eq!(found(catalog.match_vst2(id, name)).map(|(_, how)| how), Some(How::SameName));
}

#[test]
fn a_version_inside_the_name_finds_the_newer_module() {
    // A suite's modules put the version between the brand and the module's name.
    assert_eq!(
        found(catalog().match_vst2(0x5375_4938, "Suite 8 Imager")),
        Some(("Suite 11 Imager", How::NewerVersion))
    );
    assert_eq!(
        found(catalog().match_vst3("00000000000000000000000000000000", "Suite 9 Imager")),
        Some(("Suite 11 Imager", How::NewerVersion))
    );
    // The suite is another product than its module, and a newer saved version finds nothing.
    assert_eq!(found(catalog().match_vst2(0x5375_3800, "Suite 8")), Some(("Suite 11", How::NewerVersion)));
    assert_eq!(found(catalog().match_vst2(0x5375_4999, "Suite 12 Imager")), None);
    assert_eq!(versioned(&["suite".into(), "8".into(), "imager".into()]), (vec!["suite".into(), "imager".into()], 8));
    // A leading number is part of the name, not a version.
    assert_eq!(versioned(&["2".into(), "bands".into()]), (vec!["2".into(), "bands".into()], 1));
}

#[test]
fn a_number_in_the_saved_name_is_the_installed_ones_version() {
    // Orbit 5's VST3 is called Orbit, with a class id of its own; its version says 5.
    assert_eq!(found(catalog().match_vst2(0x4F72_6235, "Orbit 5")), Some(("Orbit", How::SameName)));
    assert_eq!(found(catalog().match_vst2(0x4F72_6235, "Zeta Orbit 5 x64")), Some(("Orbit", How::SameName)));
    assert_eq!(found(catalog().match_vst2(0x4F72_6234, "Orbit 4")), None);
    // A VST2 build named with `.64`.
    assert_eq!(found(catalog().match_vst2(0x4F72_6235, "Orbit.64")), Some(("Orbit", How::SameName)));
    assert_eq!(major_version("5.1.0"), Some(5));
    assert_eq!(major_version(" 12"), Some(12));
    assert_eq!(major_version("beta"), None);
}

#[test]
fn the_vst2_class_id_follows_the_sdk_convention() {
    // 'Test' named "VST 2 Plug-in": VST, the id, then "vst 2 plu" in lower case.
    assert_eq!(vst2_class_id(0x5465_7374, "VST 2 Plug-in"), "56535454657374767374203220706C75");
    // Short names are padded with zeros.
    assert_eq!(vst2_class_id(0x5859_5A21, "XYZ"), "56535458595A2178797A000000000000");
    assert!(derived_from("56534557617650776176657920320000", 0x5761_7650));
    assert!(!derived_from("56534557617650776176657920320000", 0x5761_7658));
}

#[test]
fn devices_are_found_most_certain_first() {
    let catalog = catalog();
    // The same VST3 class, and one a newer plug-in says it replaces.
    assert_eq!(found(catalog.match_vst3("11111111111111111111111111111104", "x")), Some(("Bell-Q 4", How::Same)));
    assert_eq!(found(catalog.match_vst3("5653544251337061636D65706C756769", "x")), Some(("Bell-Q 3", How::Declared)));
    // A VST2 id that a VST3 plug-in declares, and one its class id is derived from.
    assert_eq!(found(catalog.match_vst2(0x4251_3370, "Acme Bell-Q 3 x64")), Some(("Bell-Q 3", How::Declared)));
    assert_eq!(found(catalog.match_vst2(0x4861_6C52, "HallRoom_x64")), Some(("HallRoom", How::Vst2Id)));
    // The same name with a different id, with build words and the vendor set aside.
    assert_eq!(found(catalog.match_vst2(0x5879_4F48, "XY OldHeater")), Some(("XY OldHeater", How::SameName)));
    assert_eq!(found(catalog.match_vst2(1, "Acme Bell-Q 4 x64")), Some(("Bell-Q 4", How::SameName)));
    // A newer version: the nearest one.
    assert_eq!(found(catalog.match_vst2(0x4251_3270, "Acme Bell-Q 2 x64")), Some(("Bell-Q 3", How::NewerVersion)));
    assert_eq!(found(catalog.match_vst2(0x5761_7658, "Wavey_x64")), Some(("Wavey 2", How::NewerVersion)));
    // CLAP by id; nothing for unknown plug-ins, and an older version is not offered.
    assert_eq!(found(catalog.match_clap("com.example.thing", "x")), Some(("Thing", How::Same)));
    assert_eq!(found(catalog.match_vst2(2, "Unknown")), None);
    assert_eq!(found(catalog.match_vst2(3, "Bell-Q 1")), Some(("Bell-Q 3", How::NewerVersion)));
    assert_eq!(found(catalog.match_vst2(4, "")), None);
}

#[test]
fn names_lose_build_words_and_vendors() {
    assert_eq!(words("Pump-64bit"), ["pump"]);
    assert_eq!(words("The Paste 64"), ["the", "paste"]);
    assert_eq!(words("XYZ_x64"), ["xyz"]);
    assert_eq!(product_words("Acme Bell-Q 2 x64", "Acme"), ["bell-q", "2"]);
    assert_eq!(product_words("HallRoom", "Hall DSP, LLC"), ["hallroom"]);
    // A name that is only the vendor keeps it.
    assert_eq!(product_words("Zap1", "Zap1"), ["zap1"]);
    assert_eq!(versioned(&words("Wavey")), (vec!["wavey".to_string()], 1));
    assert_eq!(versioned(&words("XY OldHeater2")), (vec!["xy".to_string(), "oldheater".to_string()], 2));
    assert_eq!(versioned(&words("ABC-2A")), (vec!["abc-2a".to_string()], 1));
}

/// A PE file's header for `machine` (0x8664 for x86-64, 0x014C for 32-bit x86).
fn pe(machine: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 0x100];
    bytes[0..2].copy_from_slice(b"MZ");
    bytes[0x3C..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
    bytes
}

/// A 64-bit and a 32-bit copy of one plug-in (the second loaded through the 32-bit bridge)
/// are one plug-in: a project finds the copy that runs natively, by VST2 id, VST3 class id
/// and CLAP id alike, whichever the catalog lists first.
#[test]
fn of_a_64_bit_and_a_32_bit_copy_the_native_one_is_found() {
    if !(cfg!(windows) && cfg!(target_arch = "x86_64")) {
        return;
    }
    let dir = std::env::temp_dir().join(format!("suhina-matching-bridge-{}", std::process::id()));
    // The 32-bit copy's path sorts first, as `Program Files (x86)` does before `Program Files`.
    let (old, new) = (dir.join("a-x86"), dir.join("b-x64"));
    for folder in [&old, &new] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let (id, name) = (0x476C_4374_u32, "Cataract");
    let vst3_id = "56534544474C6C4C696D69746C657373";
    let mut entries = Vec::new();
    for (folder, machine) in [(&old, 0x014C_u16), (&new, 0x8664)] {
        for file in ["Cataract.dll", "Limitless.vst3", "Thing.clap"] {
            std::fs::write(folder.join(file), pe(machine)).unwrap();
        }
        let entry = |file: &str, plugin: serde_json::Value| json!({ "path": folder.join(file).display().to_string(), "modified": 1, "size": 1, "plugins": [plugin] });
        entries.push(entry("Cataract.dll", json!({ "id": format!("{id:08X}"), "name": name, "vendor": "G", "version": "1", "features": [], "replaces": [] })));
        entries.push(entry("Limitless.vst3", vst3(vst3_id, "Limitless", "D", &[])));
        entries.push(entry("Thing.clap", json!({ "id": "com.example.thing", "name": "Thing", "vendor": "Example" })));
    }
    let catalog = Catalog::from_json(&json!({ "version": 2, "entries": entries }).to_string());
    let path = |m: Option<Match<'_>>| m.map(|m| m.plugin.path.clone());
    assert_eq!(path(catalog.match_vst2(id, name)), Some(new.join("Cataract.dll")));
    assert_eq!(path(catalog.match_vst3(vst3_id, "Limitless")), Some(new.join("Limitless.vst3")));
    assert_eq!(path(catalog.match_clap("com.example.thing", "Thing")), Some(new.join("Thing.clap")));
    let _ = std::fs::remove_dir_all(&dir);
}
