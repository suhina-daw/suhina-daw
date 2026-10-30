// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finding the installed plug-in a project's device stands for (ADR 0006, decision 8).
//!
//! A project names a plug-in by the id it had when it was saved, which is often not one that is
//! installed now: the project used the VST2 build, or an older version. The catalog is searched
//! in order of how sure the answer is:
//!
//! 1. [`How::Same`]: the same class id (VST3) or plug-in id (CLAP).
//! 2. [`How::Declared`]: an installed VST3 plug-in says it replaces this one and opens its saved
//!    state ([`PluginInfo::replaces`]).
//! 3. [`How::Vst2Id`]: a VST3 plug-in whose class id is derived from the VST2 plug-in's id, the
//!    VST 3 SDK's documented convention for a VST3 version that stands in for a VST2 one.
//! 4. [`How::SameName`]: a VST3 plug-in with the same name once build words (`x64`, `64bit`)
//!    and a leading vendor name are set aside, so `Vendor Product 3 x64` finds `Product 3` by
//!    Vendor; or one whose name lacks the saved name's number but whose version starts with
//!    it (`Synth 5` finds a `Synth` whose version is 5.x).
//! 5. [`How::NewerVersion`]: the same name with a higher version number (`Synth` finds
//!    `Synth 2`, `Product 2` finds `Product 3`); the nearest newer version is taken.
//!
//! Only the first three promise that the saved state carries over; for the last two the state
//! is handed over and the plug-in decides whether it reads it.

use crate::scan::{Catalog, PluginInfo};

/// How the installed plug-in was found, most certain first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum How {
    Same,
    Declared,
    Vst2Id,
    SameName,
    NewerVersion,
    /// The scanned VST2 plug-in itself, loaded by the VST2 host (ADR 0022): the state is
    /// the plug-in's own form. A project's VST2 plug-in still prefers a VST3 or CLAP copy
    /// when one is installed (ADR 0006 decision 8), so this is the last resort.
    Itself,
}

impl How {
    pub fn name(self) -> &'static str {
        match self {
            How::Same => "same",
            How::Declared => "declared",
            How::Vst2Id => "vst2-id",
            How::SameName => "same-name",
            How::NewerVersion => "newer-version",
            How::Itself => "itself",
        }
    }

    /// Whether the plug-in promises to read the saved state.
    pub fn keeps_state(self) -> bool {
        matches!(self, How::Same | How::Declared | How::Vst2Id | How::Itself)
    }
}

/// An installed plug-in found for a project's device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match<'a> {
    pub plugin: &'a PluginInfo,
    pub how: How,
}

/// `VST` as the first three bytes of a class id derived from a VST2 id; controllers use `VSE`.
const VST2_PREFIXES: [&str; 2] = ["565354", "565345"];

/// The class id a VST3 version of the VST2 plug-in `id` named `name` gets by the VST 3 SDK's
/// convention: `VST`, the VST2 id, then the first nine bytes of the name in lower case (zeros
/// after its end), as 32 hex digits.
pub fn vst2_class_id(id: u32, name: &str) -> String {
    let mut text = format!("{}{id:08X}", VST2_PREFIXES[0]);
    let mut bytes = name.bytes().map(|b| b.to_ascii_lowercase()).chain(std::iter::repeat(0));
    for _ in 0..9 {
        text.push_str(&format!("{:02X}", bytes.next().unwrap_or(0)));
    }
    text
}

/// Whether `class_id` is derived from the VST2 id `id` (the name part is not compared: a
/// project keeps the name a host showed, which need not be the one the id was made from).
fn derived_from(class_id: &str, id: u32) -> bool {
    let wanted = format!("{id:08X}");
    VST2_PREFIXES.iter().any(|prefix| {
        class_id.get(..6).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
            && class_id.get(6..14).is_some_and(|i| i.eq_ignore_ascii_case(&wanted))
    })
}

/// Words that say how a plug-in was built rather than what it is.
const BUILD_WORDS: &[&str] =
    &["x64", "x86", "x86_64", "64", "64bit", "32bit", "vst", "vst2", "vst3", "(x64)", "stereo"];

/// A name's words in lower case without build words; `Kickstart-64bit` and `OTT_x64` lose the
/// build suffix too.
fn words(name: &str) -> Vec<String> {
    let mut list = Vec::new();
    for word in name.split(|c: char| c.is_whitespace() || c == '_') {
        let mut word = word.to_lowercase();
        for suffix in ["-64bit", "-x64", "(x64)", ".64"] {
            if let Some(stem) = word.strip_suffix(suffix) {
                word = stem.to_string();
            }
        }
        if !word.is_empty() && !BUILD_WORDS.contains(&word.as_str()) {
            list.push(word);
        }
    }
    list
}

/// `name`'s words with `vendor`'s leading words taken off when the name starts with them.
fn product_words(name: &str, vendor: &str) -> Vec<String> {
    let name = words(name);
    // Vendor names carry company words ("Vendor DSP, LLC"); compare the first word only.
    let vendor = words(vendor).into_iter().next().map(|w| w.trim_end_matches(',').to_string());
    match (name.split_first(), vendor) {
        (Some((first, rest)), Some(vendor)) if *first == vendor && !rest.is_empty() => rest.to_vec(),
        _ => name,
    }
}

/// A product's words split into its name and version number: `synth 2` is (`synth`, 2), a
/// name without a number is version 1. A number joined to the last word counts (`Warmer2`), and
/// so does the one number word inside a name (`suite 8 imager` is (`suite imager`, 8): suites
/// put the version between the brand and the module).
fn versioned(words: &[String]) -> (Vec<String>, u32) {
    let Some((last, rest)) = words.split_last() else { return (Vec::new(), 1) };
    if let Ok(number) = last.parse::<u32>() {
        return (rest.to_vec(), number);
    }
    let numbers: Vec<usize> =
        (1..rest.len()).filter(|&i| rest.get(i).is_some_and(|w| w.parse::<u32>().is_ok())).collect();
    if let [at] = numbers.as_slice()
        && let Some(number) = rest.get(*at).and_then(|w| w.parse::<u32>().ok())
    {
        let base = words.iter().enumerate().filter(|(i, _)| i != at).map(|(_, w)| w.clone()).collect();
        return (base, number);
    }
    let stem = last.trim_end_matches(|c: char| c.is_ascii_digit());
    let digits = last.get(stem.len()..).unwrap_or_default();
    if (1..=2).contains(&digits.len())
        && stem.chars().last().is_some_and(char::is_alphabetic)
        && let Ok(number) = digits.parse::<u32>()
    {
        let mut base = rest.to_vec();
        base.push(stem.to_string());
        return (base, number);
    }
    (words.to_vec(), 1)
}

/// The first number of a version (`5` of `5.1.0`).
fn major_version(version: &str) -> Option<u32> {
    let digits: String = version.trim().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

impl Catalog {
    fn vst3_plugins(&self) -> impl Iterator<Item = &PluginInfo> {
        self.plugins().filter(|p| p.format() == "vst3")
    }

    /// The installed plug-in for a VST3 device with class id `id`, saved under `name`.
    pub fn match_vst3(&self, id: &str, name: &str) -> Option<Match<'_>> {
        let found = |how, plugin| Some(Match { plugin, how });
        if let Some(plugin) = self.vst3_plugins().find(|p| p.id.eq_ignore_ascii_case(id)) {
            return found(How::Same, plugin);
        }
        if let Some(plugin) = self.vst3_plugins().find(|p| p.replaces.iter().any(|r| r.eq_ignore_ascii_case(id))) {
            return found(How::Declared, plugin);
        }
        self.match_name(name)
    }

    /// The installed plug-in for a VST2 device with id `id`, saved under `name`.
    pub fn match_vst2(&self, id: u32, name: &str) -> Option<Match<'_>> {
        let found = |how, plugin| Some(Match { plugin, how });
        if let Some(plugin) = self.vst3_plugins().find(|p| p.replaces.iter().any(|r| derived_from(r, id))) {
            return found(How::Declared, plugin);
        }
        let exact = vst2_class_id(id, name);
        let derived = || self.vst3_plugins().filter(|p| derived_from(&p.id, id));
        if let Some(plugin) = derived().find(|p| p.id.eq_ignore_ascii_case(&exact)).or_else(|| derived().next()) {
            return found(How::Vst2Id, plugin);
        }
        self.match_name(name).or_else(|| {
            // No VST3 or CLAP copy stands in: the scanned VST2 plug-in itself, by its id
            // (decision 8: the copies are preferred while any is installed).
            self.plugins()
                .find(|p| p.format() == "vst2" && p.id == format!("{id:08X}"))
                .map(|plugin| Match { plugin, how: How::Itself })
        })
    }

    /// The installed plug-in for a CLAP device with id `id`, saved under `name`.
    pub fn match_clap(&self, id: &str, name: &str) -> Option<Match<'_>> {
        match self.plugins().find(|p| p.format() == "clap" && p.id == id) {
            Some(plugin) => Some(Match { plugin, how: How::Same }),
            None => self.match_name(name),
        }
    }

    /// A VST3 plug-in by name: the same product, or else its nearest newer version.
    fn match_name(&self, name: &str) -> Option<Match<'_>> {
        if words(name).is_empty() {
            return None;
        }
        // The saved name may itself start with the vendor ("Vendor Product 2"), so each
        // installed plug-in's vendor is taken off both sides.
        let same = |p: &PluginInfo| product_words(name, &p.vendor) == product_words(&p.name, &p.vendor);
        if let Some(plugin) = self.vst3_plugins().find(|p| same(p)) {
            return Some(Match { plugin, how: How::SameName });
        }
        // The number in the saved name is the installed one's version: `Synth 5` is `Synth`
        // 5.x.
        let numbered = |p: &PluginInfo| {
            let saved = product_words(name, &p.vendor);
            let (base, number) = versioned(&saved);
            base.len() < saved.len()
                && base == product_words(&p.name, &p.vendor)
                && major_version(&p.version) == Some(number)
        };
        if let Some(plugin) = self.vst3_plugins().find(|p| numbered(p)) {
            return Some(Match { plugin, how: How::SameName });
        }
        self.vst3_plugins()
            .filter_map(|p| {
                let (base, old) = versioned(&product_words(name, &p.vendor));
                let (installed, new) = versioned(&product_words(&p.name, &p.vendor));
                (!base.is_empty() && base == installed && new > old).then_some((new, p))
            })
            .min_by_key(|(version, _)| *version)
            .map(|(_, plugin)| Match { plugin, how: How::NewerVersion })
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(
            versioned(&["ozone".into(), "8".into(), "imager".into()]),
            (vec!["ozone".into(), "imager".into()], 8)
        );
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
        assert_eq!(
            found(catalog.match_vst3("5653544651337066616266696C746572", "x")),
            Some(("Pro-Q 3", How::Declared))
        );
        // A VST2 id that a VST3 plug-in declares, and one its class id is derived from.
        assert_eq!(found(catalog.match_vst2(0x4651_3370, "FabFilter Pro-Q 3 x64")), Some(("Pro-Q 3", How::Declared)));
        assert_eq!(found(catalog.match_vst2(0x5275_756D, "ValhallaRoom_x64")), Some(("ValhallaRoom", How::Vst2Id)));
        // The same name with a different id, with build words and the vendor set aside.
        assert_eq!(
            found(catalog.match_vst2(0x5070_5657, "PSP VintageWarmer")),
            Some(("PSP VintageWarmer", How::SameName))
        );
        assert_eq!(found(catalog.match_vst2(1, "FabFilter Pro-Q 4 x64")), Some(("Pro-Q 4", How::SameName)));
        // A newer version: the nearest one.
        assert_eq!(
            found(catalog.match_vst2(0x4651_3270, "FabFilter Pro-Q 2 x64")),
            Some(("Pro-Q 3", How::NewerVersion))
        );
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
}
