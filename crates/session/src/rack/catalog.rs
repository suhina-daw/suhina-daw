// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Matching a project's plug-in devices with the installed plug-ins in the catalog, and what
//! a plug-in's scan says it is.

use super::*;

/// The installed plug-in for `device` and whether it is an instrument; `None` when nothing
/// installed stands for it (or it is not a plug-in).
pub(super) fn find<'a>(catalog: &'a Catalog, device: &Device) -> Option<(suhina_plugins::matching::Match<'a>, bool)> {
    let (found, _) = crate::plugins::device_plugin(catalog, device)?;
    let instrument = is_instrument(found.plugin);
    Some((found, instrument))
}

/// Whether the plug-in plays notes, from the features its scan found (CLAP `instrument`, VST3
/// subcategories such as `Instrument|Synth`).
pub fn is_instrument(plugin: &PluginInfo) -> bool {
    plugin.features.iter().any(|f| f.to_ascii_lowercase().contains("instrument"))
}

/// Suhina's group for the plug-in, from the format's own sub-categories, as the browser's
/// group-by-type view shows it ([the plug-in browsing spec](../docs/spec/plugin-browsing.md)):
/// `None` is the type's **Other**. The first of the spec's tables that matches wins, so a
/// VST3's `Fx|Dynamics|Compressor` is Dynamics, not one group per word.
pub fn plugin_group(plugin: &PluginInfo) -> Option<&'static str> {
    // The words one format or the other uses for each group, lower-cased, whole words.
    const GROUPS: [(&str, &[&str]); 12] = [
        ("Synth", &["synth", "synthesizer"]),
        ("Sampler", &["sampler"]),
        ("Drums", &["drum", "drum-machine"]),
        ("Reverb", &["reverb"]),
        ("Delay", &["delay"]),
        ("EQ", &["eq"]),
        ("Filter", &["filter"]),
        ("Dynamics", &["dynamics", "compressor", "limiter", "gate"]),
        ("Distortion", &["distortion", "saturation"]),
        ("Modulation", &["modulator", "chorus", "flanger", "phaser", "tremolo", "vibrato"]),
        ("Analyzer", &["analyzer"]),
        ("Utility", &["utility", "spatial"]),
    ];
    // The features arrive as separate words already (a VST3's sub-categories are split on
    // their `|` in the scan, a CLAP's features are one word each); they are matched whole,
    // case-insensitively. The type words ("Fx", "Instrument", "audio-effect") name no group.
    let features: Vec<String> = plugin.features.iter().map(|f| f.to_ascii_lowercase()).collect();
    GROUPS.iter().find_map(|(group, words)| features.iter().any(|f| words.contains(&f.as_str())).then_some(*group))
}

/// The device kind a project stores for the installed plug-in `plugin`.
pub fn device_kind(plugin: &PluginInfo) -> DeviceKind {
    match plugin.format() {
        "vst3" => DeviceKind::Vst3 { id: plugin.id.clone() },
        "vst2" => DeviceKind::Vst2 { id: u32::from_str_radix(&plugin.id, 16).unwrap_or(0) },
        _ => DeviceKind::Clap { id: plugin.id.clone() },
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn plugin(features: &[&str]) -> PluginInfo {
        PluginInfo {
            path: PathBuf::from("/vst3/Thing.vst3"),
            id: "00000000000000000000000000000000".into(),
            name: "Thing".into(),
            vendor: "Maker".into(),
            version: "1".into(),
            features: features.iter().map(|f| (*f).into()).collect(),
            replaces: Vec::new(),
            ara: false,
        }
    }

    #[test]
    fn a_plugins_group_comes_from_its_formats_categories() {
        // A VST3's sub-categories, as the scan splits them; the first group in the table wins.
        assert_eq!(plugin_group(&plugin(&["Fx", "Dynamics", "Compressor"])), Some("Dynamics"));
        assert_eq!(plugin_group(&plugin(&["Instrument", "Synth"])), Some("Synth"));
        // Case-insensitive, whole words: "Spectrum" is no group, so Other.
        assert_eq!(plugin_group(&plugin(&["Fx", "Spectrum"])), None);
        assert_eq!(plugin_group(&plugin(&["ANALYZER"])), Some("Analyzer"));
        // A CLAP's features.
        assert_eq!(plugin_group(&plugin(&["audio-effect", "reverb"])), Some("Reverb"));
        assert_eq!(plugin_group(&plugin(&["instrument", "synthesizer"])), Some("Synth"));
        assert_eq!(plugin_group(&plugin(&["instrument", "drum-machine"])), Some("Drums"));
        // The type words name no group; a plug-in with none is Other.
        assert_eq!(plugin_group(&plugin(&["audio-effect"])), None);
        assert_eq!(plugin_group(&plugin(&["Instrument", "External"])), None);
        assert_eq!(plugin_group(&plugin(&[])), None);
    }
}

/// Imported parameter indices cannot be sent to a replacement in another plug-in format.
/// A saved state being compatible does not establish a parameter-id mapping.
pub(super) fn parameter_identity_matches(device: &Device, installed: &PluginInfo) -> bool {
    if !device.extensions.0.iter().any(|e| e.format == "flp" && e.key == "wrapper") {
        return true;
    }
    match &device.kind {
        DeviceKind::Vst2 { id } => {
            installed.format() == "vst2" && u32::from_str_radix(&installed.id, 16).ok() == Some(*id)
        }
        DeviceKind::Vst3 { id } => installed.format() == "vst3" && installed.id.eq_ignore_ascii_case(id),
        _ => true,
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    #[test]
    fn imported_indices_require_the_same_identity_and_format() {
        let mut d = Device::new("Synth", DeviceKind::Vst2 { id: 0x12345678 });
        d.extensions.0.push(suhina_model::Extension { format: "flp".into(), key: "wrapper".into(), data: vec![] });
        let mut p = PluginInfo {
            path: PathBuf::from("/Synth.dll"),
            id: "12345678".into(),
            name: "Synth".into(),
            vendor: String::new(),
            version: String::new(),
            features: vec![],
            replaces: vec![],
            ara: false,
        };
        assert!(parameter_identity_matches(&d, &p));
        p.path = PathBuf::from("/Synth.vst3");
        assert!(!parameter_identity_matches(&d, &p));
        d.extensions.0.clear();
        assert!(parameter_identity_matches(&d, &p));
    }
}

#[cfg(test)]
mod replacement_host_tests {
    use super::*;
    #[test]
    fn a_guessed_newer_version_cannot_join_the_originals_host_group() {
        use suhina_plugins::matching::{How, Match};
        let env = PluginEnv::new(Catalog::default(), PathBuf::from("host"));
        let plugin = PluginInfo {
            path: PathBuf::from("/Synth.vst3"),
            id: "0123456789ABCDEF0123456789ABCDEF".into(),
            name: "Synth 2".into(),
            vendor: "Suhina".into(),
            version: "2".into(),
            features: vec![],
            replaces: vec![],
            ara: false,
        };
        for how in [How::Same, How::Declared, How::Vst2Id, How::Itself, How::SameName] {
            assert_eq!(env.launch_group(&Match { plugin: &plugin, how }), (env.group)(&plugin));
        }
        assert!(env.launch_group(&Match { plugin: &plugin, how: How::NewerVersion }).is_none());
    }
}
