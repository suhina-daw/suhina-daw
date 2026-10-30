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
            entry("/vst3/Room.vst3", vst3(&vst2_class_id(0x5275_756D, "ValhallaRoom"), "ValhallaRoom", "Valhalla DSP, LLC", &[])),
            entry("/vst3/Pro-Q 3.vst3", vst3("72C4DB717A4D459AB97E51745D84B39D", "Pro-Q 3", "FabFilter",
                &["5653544651337066616266696C746572"])),
            entry("/vst3/Pro-Q 4.vst3", vst3("ED57BD725C60467EA64DD2F400758B6F", "Pro-Q 4", "FabFilter", &[])),
            entry("/vst3/Serum2.vst3", vst3("56534558667350736572756D20320000", "Serum 2", "Xfer Records", &[])),
            entry("/vst3/Warmer.vst3", vst3("32ECDB19D0894B24AF976000F5D58DD7", "PSP VintageWarmer", "PSPaudioware.com", &[])),
            entry("/vst3/Warmer2.vst3", vst3("32ECDB21D0894B26AF976002F5D58DD9", "PSP VintageWarmer2", "PSPaudioware.com", &[])),
            entry("/clap/thing.clap", json!({ "id": "com.example.thing", "name": "Thing", "vendor": "Example" })),
            entry("/vst3/Ozone 11 Imager.vst3", vst3("5653545A41495A4F7A6F6E652050726F", "Ozone 11 Imager", "iZotope", &[])),
            entry("/vst3/Ozone 11.vst3", vst3("5653545A41495A4F7A6F6E6520313100", "Ozone 11", "iZotope", &[])),
            entry("/vst3/Nexus.vst3", json!({ "id": "ABCDEF019182FAEB726546584E455855", "name": "Nexus", "vendor": "reFX",
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
    let mut entries = vec![file(
        "/vst2/Gain.vst",
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

/// A copy that stands in still plays it (ADR 0006 decision 8), even though the plug-in
/// itself is installed.
#[test]
fn a_vst2_plug_in_with_a_standing_in_copy_prefers_it() {
    let (id, name) = (0x5368_5448, "Suhina Test Gain");
    let catalog = vst2_catalog(id, name, true);
    let matched = catalog.match_vst2(id, name);
    assert_eq!(found(matched).map(|(_, how)| how), Some(How::Vst2Id));
    assert_eq!(matched.map(|m| m.plugin.format()), Some("vst3"));
}

#[test]
fn a_version_inside_the_name_finds_the_newer_module() {
    // Ozone's modules put the version between the brand and the module's name.
    assert_eq!(
        found(catalog().match_vst2(0x5A4F_4938, "Ozone 8 Imager")),
        Some(("Ozone 11 Imager", How::NewerVersion))
    );
    assert_eq!(
        found(catalog().match_vst3("00000000000000000000000000000000", "Ozone 9 Imager")),
        Some(("Ozone 11 Imager", How::NewerVersion))
    );
    // The suite is another product than its module, and a newer saved version finds nothing.
    assert_eq!(found(catalog().match_vst2(0x5A4F_3800, "Ozone 8")), Some(("Ozone 11", How::NewerVersion)));
    assert_eq!(found(catalog().match_vst2(0x5A4F_4999, "Ozone 12 Imager")), None);
    assert_eq!(versioned(&["ozone".into(), "8".into(), "imager".into()]), (vec!["ozone".into(), "imager".into()], 8));
    // A leading number is part of the name, not a version.
    assert_eq!(versioned(&["2".into(), "caos".into()]), (vec!["2".into(), "caos".into()], 1));
}

#[test]
fn a_number_in_the_saved_name_is_the_installed_ones_version() {
    // Nexus 5's VST3 is called Nexus, with a class id of its own; its version says 5.
    assert_eq!(found(catalog().match_vst2(0x4E78_7335, "Nexus 5")), Some(("Nexus", How::SameName)));
    assert_eq!(found(catalog().match_vst2(0x4E78_7335, "reFX Nexus 5 x64")), Some(("Nexus", How::SameName)));
    assert_eq!(found(catalog().match_vst2(0x4E78_7334, "Nexus 4")), None);
    // A VST2 build named with `.64`.
    assert_eq!(found(catalog().match_vst2(0x4E78_7335, "Nexus.64")), Some(("Nexus", How::SameName)));
    assert_eq!(major_version("5.1.0"), Some(5));
    assert_eq!(major_version(" 12"), Some(12));
    assert_eq!(major_version("beta"), None);
}

#[test]
fn the_vst2_class_id_follows_the_sdk_convention() {
    // 'Test' named "VST 2 Plug-in": VST, the id, then "vst 2 plu" in lower case.
    assert_eq!(vst2_class_id(0x5465_7374, "VST 2 Plug-in"), "56535454657374767374203220706C75");
    // Short names are padded with zeros.
    assert_eq!(vst2_class_id(0x4F54_5421, "OTT"), "5653544F5454216F7474000000000000");
    assert!(derived_from("56534558667350736572756D20320000", 0x5866_7350));
    assert!(!derived_from("56534558667350736572756D20320000", 0x5866_7358));
}

#[test]
fn devices_are_found_most_certain_first() {
    let catalog = catalog();
    // The same VST3 class, and one a newer plug-in says it replaces.
    assert_eq!(found(catalog.match_vst3("ED57BD725C60467EA64DD2F400758B6F", "x")), Some(("Pro-Q 4", How::Same)));
    assert_eq!(found(catalog.match_vst3("5653544651337066616266696C746572", "x")), Some(("Pro-Q 3", How::Declared)));
    // A VST2 id that a VST3 plug-in declares, and one its class id is derived from.
    assert_eq!(found(catalog.match_vst2(0x4651_3370, "FabFilter Pro-Q 3 x64")), Some(("Pro-Q 3", How::Declared)));
    assert_eq!(found(catalog.match_vst2(0x5275_756D, "ValhallaRoom_x64")), Some(("ValhallaRoom", How::Vst2Id)));
    // The same name with a different id, with build words and the vendor set aside.
    assert_eq!(found(catalog.match_vst2(0x5070_5657, "PSP VintageWarmer")), Some(("PSP VintageWarmer", How::SameName)));
    assert_eq!(found(catalog.match_vst2(1, "FabFilter Pro-Q 4 x64")), Some(("Pro-Q 4", How::SameName)));
    // A newer version: the nearest one.
    assert_eq!(found(catalog.match_vst2(0x4651_3270, "FabFilter Pro-Q 2 x64")), Some(("Pro-Q 3", How::NewerVersion)));
    assert_eq!(found(catalog.match_vst2(0x5866_7358, "Serum_x64")), Some(("Serum 2", How::NewerVersion)));
    // CLAP by id; nothing for unknown plug-ins, and an older version is not offered.
    assert_eq!(found(catalog.match_clap("com.example.thing", "x")), Some(("Thing", How::Same)));
    assert_eq!(found(catalog.match_vst2(2, "Massive")), None);
    assert_eq!(found(catalog.match_vst2(3, "Pro-Q 1")), Some(("Pro-Q 3", How::NewerVersion)));
    assert_eq!(found(catalog.match_vst2(4, "")), None);
}

#[test]
fn names_lose_build_words_and_vendors() {
    assert_eq!(words("Kickstart-64bit"), ["kickstart"]);
    assert_eq!(words("The Glue 64"), ["the", "glue"]);
    assert_eq!(words("OTT_x64"), ["ott"]);
    assert_eq!(product_words("FabFilter Pro-Q 2 x64", "FabFilter"), ["pro-q", "2"]);
    assert_eq!(product_words("ValhallaRoom", "Valhalla DSP, LLC"), ["valhallaroom"]);
    // A name that is only the vendor keeps it.
    assert_eq!(product_words("Sylenth1", "Sylenth1"), ["sylenth1"]);
    assert_eq!(versioned(&words("Serum")), (vec!["serum".to_string()], 1));
    assert_eq!(versioned(&words("PSP VintageWarmer2")), (vec!["psp".to_string(), "vintagewarmer".to_string()], 2));
    assert_eq!(versioned(&words("CLA-2A")), (vec!["cla-2a".to_string()], 1));
}
