// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live's own devices that map onto Suhina's built-in effects and instruments (ADR 0004). Each mapping follows
//! `docs/spec/als-format.md` › Devices: the element Live stores, the unit of every value, and
//! the Suhina parameter it becomes. Parameter ids are the devices' stable ids
//! (`devices/<name>` in this repository).

use roxmltree::Node;
use suhina_model::DeviceParam;

use crate::{boolean, number, path};

/// Gain at or below this (in dB) is the Suhina utility's silence.
const SILENCE_DB: f64 = -70.0;

/// If `node` is a Live device with a Suhina counterpart: the Suhina device id, a default
/// name, and the parameter values. `warn` gets notes about values that could not be read.
pub(crate) fn builtin(
    node: Node,
    warn: &mut dyn FnMut(String),
) -> Option<(&'static str, &'static str, Vec<DeviceParam>)> {
    match node.tag_name().name() {
        "StereoGain" => Some(("utility", "Utility", utility(node, warn))),
        "Reverb" => Some(("reverb", "Reverb", reverb(node, warn))),
        "Delay" => Some(("delay", "Delay", delay(node, warn))),
        _ => None,
    }
}

/// An automatable value: `<name>/Manual@Value`.
fn manual(node: Node, name: &str) -> Option<f64> {
    number(node, &[name, "Manual"])
}

fn manual_flag(node: Node, name: &str) -> Option<bool> {
    boolean(node, &[name, "Manual"])
}

/// A plain (not automatable) flag: `<name>@Value`.
fn plain_flag(node: Node, name: &str) -> Option<bool> {
    path(node, &[name]).and_then(|n| n.attribute("Value")).and_then(|v| match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
}

fn toggle(on: bool) -> f64 {
    if on { 1.0 } else { 0.0 }
}

/// A stepped value stored as a number (`<name>/Manual@Value`) as an option index below `count`;
/// anything else is noted and left out, so the device keeps its default.
fn step(node: Node, name: &str, count: usize, what: &str, warn: &mut dyn FnMut(String)) -> Option<f64> {
    let stored = manual(node, name)?;
    let index = stored.round();
    if (0.0..count as f64).contains(&index) {
        Some(index)
    } else {
        warn(format!("{what} {stored} is not understood; using the default"));
        None
    }
}

/// Collects a device's values, leaving out the ones that are missing.
#[derive(Default)]
struct Params(Vec<DeviceParam>);

impl Params {
    /// Adds `value` under `id` when there is one and it is finite.
    fn set(&mut self, id: &str, value: Option<f64>) {
        if let Some(value) = value.filter(|v| v.is_finite()) {
            self.0.push(DeviceParam::new(id, value));
        }
    }
}

/// Lowest level of the reverb's Reflect and Diffuse in dB: Live stores them as linear gains from
/// 0.03 up.
const REVERB_MIN_LEVEL_DB: f64 = -30.4576;

/// Live's Reverb as Suhina's `reverb` (`docs/spec/reverb.md` › Import).
fn reverb(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut p = Params::default();
    let flag = |name| manual_flag(node, name).map(toggle);
    // Times are stored in milliseconds; ours are seconds.
    p.set("predelay", manual(node, "PreDelay").map(|ms| ms / 1_000.0));
    // The input band's two switches are named the other way round: BandHighOn is the low cut.
    p.set("lowCut", flag("BandHighOn"));
    p.set("highCut", flag("BandLowOn"));
    p.set("inputFrequency", manual(node, "BandFreq"));
    p.set("inputWidth", manual(node, "BandWidth"));
    p.set("spin", flag("SpinOn"));
    p.set("spinRate", manual(node, "EarlyReflectModFreq"));
    p.set("spinAmount", manual(node, "EarlyReflectModDepth"));
    p.set("shape", manual(node, "DiffuseDelay"));
    p.set("highDamping", flag("ShelfHighOn"));
    p.set("highDampingType", step(node, "HighFilterType", 2, "Reverb high filter type", warn));
    p.set("highDampingFrequency", manual(node, "ShelfHiFreq"));
    p.set("highDampingGain", manual(node, "ShelfHiGain"));
    p.set("lowDamping", flag("ShelfLowOn"));
    p.set("lowDampingFrequency", manual(node, "ShelfLoFreq"));
    p.set("lowDampingGain", manual(node, "ShelfLoGain"));
    p.set("diffusion", manual(node, "AllPassGain"));
    p.set("scale", manual(node, "AllPassSize"));
    p.set("chorus", flag("ChorusOn"));
    p.set("chorusRate", manual(node, "SizeModFreq"));
    p.set("chorusAmount", manual(node, "SizeModDepth"));
    p.set("size", manual(node, "RoomSize"));
    p.set("smooth", step(node, "SizeSmoothing", 3, "Reverb size smoothing", warn));
    p.set("decay", manual(node, "DecayTime").map(|ms| ms / 1_000.0));
    p.set("freeze", flag("FreezeOn"));
    p.set("flat", flag("FlatOn"));
    p.set("cut", flag("CutOn"));
    p.set("stereo", manual(node, "StereoSeparation"));
    p.set("density", step(node, "RoomType", 4, "Reverb density", warn));
    // Reflect and Diffuse are stored as linear gains; ours are decibels.
    let level = |g: f64| if g > 0.0 { (20.0 * g.log10()).max(REVERB_MIN_LEVEL_DB) } else { REVERB_MIN_LEVEL_DB };
    p.set("reflect", manual(node, "MixReflect").map(level));
    p.set("diffuse", manual(node, "MixDiffuse").map(level));
    p.set("mix", manual(node, "MixDirect"));
    p.0
}

/// Live's Delay as Suhina's `delay` (`docs/spec/delay.md` › Import).
fn delay(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut p = Params::default();
    let flag = |name| manual_flag(node, name).map(toggle);
    p.set("transition", step(node, "DelayLine_SmoothingMode", 3, "Delay transition mode", warn));
    p.set("link", flag("DelayLine_Link"));
    p.set("pingPong", flag("DelayLine_PingPong"));
    p.set("syncLeft", flag("DelayLine_SyncL"));
    p.set("syncRight", flag("DelayLine_SyncR"));
    p.set("timeLeft", manual(node, "DelayLine_TimeL"));
    p.set("timeRight", manual(node, "DelayLine_TimeR"));
    p.set("sixteenthsLeft", step(node, "DelayLine_SyncedSixteenthL", 8, "Delay sync step", warn));
    p.set("sixteenthsRight", step(node, "DelayLine_SyncedSixteenthR", 8, "Delay sync step", warn));
    p.set("offsetLeft", manual(node, "DelayLine_OffsetL"));
    p.set("offsetRight", manual(node, "DelayLine_OffsetR"));
    p.set("feedback", manual(node, "Feedback"));
    p.set("freeze", flag("Freeze"));
    p.set("filter", flag("Filter_On"));
    p.set("filterFrequency", manual(node, "Filter_Frequency"));
    p.set("filterWidth", manual(node, "Filter_Bandwidth"));
    p.set("lfoMode", step(node, "Modulation_TimeMode", 6, "Delay LFO mode", warn));
    p.set("lfoRate", manual(node, "Modulation_Frequency"));
    p.set("lfoTime", manual(node, "Modulation_Time"));
    // The synced rate is stored as a number from 0 to 21 that the panel shows in steps.
    p.set("lfoSynced", step(node, "Modulation_SyncedRate", 22, "Delay LFO synced rate", warn));
    p.set("lfoSixteenths", manual(node, "Modulation_Sixteenth").map(f64::round));
    p.set("lfoShape", step(node, "Modulation_Waveform", 7, "Delay LFO waveform", warn));
    p.set("lfoMorph", manual(node, "Modulation_Morph"));
    p.set("lfoToTime", manual(node, "Modulation_AmountTime"));
    p.set("lfoToFilter", manual(node, "Modulation_AmountFilter"));
    p.set("mix", manual(node, "DryWet"));
    p.0
}

/// Live's Utility (`StereoGain`) as Suhina's `utility`.
fn utility(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut params = Vec::new();
    let mut set = |id: &str, value: Option<f64>| {
        if let Some(value) = value {
            params.push(DeviceParam::new(id, value));
        }
    };

    // Gain is stored as a linear factor; ours is in decibels, with silence at the bottom.
    set("gain", manual(node, "Gain").map(|g| if g > 0.0 { (20.0 * g.log10()).max(SILENCE_DB) } else { SILENCE_DB }));
    set("balance", manual(node, "Balance"));

    // Mid/Side is a mode of Live's Width knob, with its own value 0 (only mid) .. 2 (only
    // side). Ours is a separate control from -1 to 1, next to a width that stays at 100 %.
    if plain_flag(node, "MidSideBalanceOn") == Some(true) {
        set("width", Some(1.0));
        set("midSide", manual(node, "MidSideBalance").map(|v| v - 1.0));
    } else {
        set("width", manual(node, "StereoWidth"));
        set("midSide", Some(0.0));
    }

    set("mono", manual_flag(node, "Mono").map(toggle));
    set("bassMono", manual_flag(node, "BassMono").map(toggle));
    set("bassMonoFrequency", manual(node, "BassMonoFrequency"));
    set("bassMonoListen", plain_flag(node, "BassMonoAudition").map(toggle));
    set("invertLeft", manual_flag(node, "PhaseInvertL").map(toggle));
    set("invertRight", manual_flag(node, "PhaseInvertR").map(toggle));

    // Live: 0 Left, 1 Stereo, 2 Right, 3 Swap. Ours: 0 Stereo, 1 Left, 2 Right, 3 Swap.
    let channel = manual(node, "ChannelMode").map(|mode| match mode as i64 {
        0 => 1.0,
        1 => 0.0,
        2 => 2.0,
        3 => 3.0,
        other => {
            warn(format!("Utility channel mode {other} is not understood; using stereo"));
            0.0
        }
    });
    set("channel", channel);
    set("dcFilter", manual_flag(node, "DcFilter").map(toggle));
    set("mute", manual_flag(node, "Mute").map(toggle));
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utility_from(inner: &str) -> (Vec<DeviceParam>, Vec<String>) {
        let xml = format!("<StereoGain>{inner}</StereoGain>");
        let doc = roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{e}"));
        let mut warnings = Vec::new();
        let (id, _, params) =
            builtin(doc.root_element(), &mut |w| warnings.push(w)).unwrap_or_else(|| panic!("not mapped"));
        assert_eq!(id, "utility");
        (params, warnings)
    }

    fn param(params: &[DeviceParam], id: &str) -> f64 {
        params.iter().find(|p| p.id == id).map(|p| p.value).unwrap_or_else(|| panic!("{id} missing: {params:?}"))
    }

    fn manual_xml(name: &str, value: &str) -> String {
        format!(r#"<{name}><Manual Value="{value}"/><AutomationTarget Id="1"/></{name}>"#)
    }

    #[test]
    fn gain_becomes_decibels() {
        for (stored, db) in
            [("1", 0.0), ("0.2511886358", -12.0), ("1.99526238", 6.0), ("56.2341309", 35.0), ("0", -70.0)]
        {
            let (params, _) = utility_from(&manual_xml("Gain", stored));
            assert!((param(&params, "gain") - db).abs() < 1e-3, "{stored}: {}", param(&params, "gain"));
        }
    }

    #[test]
    fn width_or_mid_side_mode() {
        let (params, _) = utility_from(&(manual_xml("StereoWidth", "4") + &manual_xml("MidSideBalance", "0.5")));
        assert_eq!((param(&params, "width"), param(&params, "midSide")), (4.0, 0.0));
        let on = r#"<MidSideBalanceOn Value="true"/>"#;
        for (stored, ours) in [("0", -1.0), ("0.5", -0.5), ("1", 0.0), ("1.5", 0.5), ("2", 1.0)] {
            let (params, _) =
                utility_from(&(manual_xml("StereoWidth", "4") + &manual_xml("MidSideBalance", stored) + on));
            assert_eq!((param(&params, "width"), param(&params, "midSide")), (1.0, ours), "{stored}");
        }
    }

    #[test]
    fn channel_modes_are_reordered() {
        for (stored, ours) in [("0", 1.0), ("1", 0.0), ("2", 2.0), ("3", 3.0)] {
            let (params, warnings) = utility_from(&manual_xml("ChannelMode", stored));
            assert_eq!(param(&params, "channel"), ours);
            assert!(warnings.is_empty());
        }
        let (params, warnings) = utility_from(&manual_xml("ChannelMode", "7"));
        assert_eq!(param(&params, "channel"), 0.0);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn switches_and_frequency() {
        let xml = [
            manual_xml("Mono", "true"),
            manual_xml("BassMono", "true"),
            manual_xml("BassMonoFrequency", "500"),
            manual_xml("PhaseInvertL", "true"),
            manual_xml("PhaseInvertR", "false"),
            manual_xml("DcFilter", "true"),
            manual_xml("Mute", "false"),
            manual_xml("Balance", "-1"),
            r#"<BassMonoAudition Value="true"/>"#.to_string(),
        ]
        .concat();
        let (params, _) = utility_from(&xml);
        let expected = [
            ("mono", 1.0),
            ("bassMono", 1.0),
            ("bassMonoFrequency", 500.0),
            ("invertLeft", 1.0),
            ("invertRight", 0.0),
            ("dcFilter", 1.0),
            ("mute", 0.0),
            ("balance", -1.0),
            ("bassMonoListen", 1.0),
        ];
        for (id, value) in expected {
            assert_eq!(param(&params, id), value, "{id}");
        }
    }

    #[test]
    fn missing_and_malformed_values_are_left_out() {
        let (params, _) = utility_from(&(manual_xml("Gain", "lots") + &manual_xml("Mono", "maybe")));
        assert!(params.iter().all(|p| p.id != "gain" && p.id != "mono"), "{params:?}");
        assert!(params.iter().all(|p| p.value.is_finite()));
    }

    fn mapped(tag: &str, inner: &str) -> (&'static str, Vec<DeviceParam>, Vec<String>) {
        let xml = format!("<{tag}>{inner}</{tag}>");
        let doc = roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{e}"));
        let mut warnings = Vec::new();
        let (id, _, params) =
            builtin(doc.root_element(), &mut |w| warnings.push(w)).unwrap_or_else(|| panic!("{tag} not mapped"));
        (id, params, warnings)
    }

    /// Live's Reverb as a freshly loaded preset stores it (measurements of the device notes).
    const REVERB_PRESET: [(&str, &str); 32] = [
        ("PreDelay", "2.5"),
        ("BandHighOn", "false"),
        ("BandLowOn", "true"),
        ("BandFreq", "829.999756"),
        ("BandWidth", "5.8499999"),
        ("SpinOn", "true"),
        ("EarlyReflectModFreq", "0.2977000177"),
        ("EarlyReflectModDepth", "17.5"),
        ("DiffuseDelay", "0.5"),
        ("ShelfHighOn", "true"),
        ("HighFilterType", "0"),
        ("ShelfHiFreq", "4500.00146"),
        ("ShelfHiGain", "0.6999999881"),
        ("ShelfLowOn", "false"),
        ("ShelfLoFreq", "90.0000076"),
        ("ShelfLoGain", "0.75"),
        ("ChorusOn", "true"),
        ("SizeModFreq", "0.02000000142"),
        ("SizeModDepth", "0.01999999955"),
        ("DecayTime", "1200.00012"),
        ("AllPassGain", "0.6000000238"),
        ("AllPassSize", "0.400000006"),
        ("FreezeOn", "false"),
        ("FlatOn", "true"),
        ("CutOn", "true"),
        ("RoomSize", "99.9999924"),
        ("SizeSmoothing", "2"),
        ("StereoSeparation", "100"),
        ("RoomType", "1"),
        ("MixReflect", "1"),
        ("MixDiffuse", "0.02999999933"),
        ("MixDirect", "0.5500000119"),
    ];

    #[test]
    fn reverb_takes_every_parameter_in_our_units() {
        let xml: String = REVERB_PRESET.iter().map(|(name, value)| manual_xml(name, value)).collect();
        let (id, params, warnings) = mapped("Reverb", &xml);
        assert_eq!(id, "reverb");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(params.len(), 32);
        let near = |id: &str, expected: f64| {
            let got = param(&params, id);
            assert!((got - expected).abs() < 1e-3 * expected.abs().max(1.0), "{id}: {got} against {expected}");
        };
        near("predelay", 0.0025);
        near("decay", 1.2);
        // The band's switches are stored under each other's names.
        near("lowCut", 0.0);
        near("highCut", 1.0);
        near("inputFrequency", 830.0);
        near("spinRate", 0.2977);
        near("shape", 0.5);
        near("highDampingType", 0.0);
        near("diffusion", 0.6);
        near("scale", 0.4);
        near("size", 100.0);
        near("smooth", 2.0);
        near("density", 1.0);
        near("reflect", 0.0);
        // The lowest stored level, 0.03, is the bottom of our range in decibels.
        near("diffuse", REVERB_MIN_LEVEL_DB);
        near("mix", 0.55);
        near("chorusAmount", 0.02);
    }

    #[test]
    fn reverb_steps_outside_their_lists_are_left_out() {
        let xml = manual_xml("RoomType", "7") + &manual_xml("SizeSmoothing", "-1") + &manual_xml("MixReflect", "0");
        let (_, params, warnings) = mapped("Reverb", &xml);
        assert!(params.iter().all(|p| p.id != "density" && p.id != "smooth"), "{params:?}");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert_eq!(param(&params, "reflect"), REVERB_MIN_LEVEL_DB);
    }

    #[test]
    fn delay_takes_every_parameter_in_our_units() {
        let xml: String = [
            ("DelayLine_SmoothingMode", "1"),
            ("DelayLine_Link", "false"),
            ("DelayLine_PingPong", "true"),
            ("DelayLine_SyncL", "true"),
            ("DelayLine_SyncR", "false"),
            ("DelayLine_TimeL", "0.3749999404"),
            ("DelayLine_TimeR", "0.01287333336"),
            ("DelayLine_SyncedSixteenthL", "2"),
            ("DelayLine_SyncedSixteenthR", "7"),
            ("DelayLine_OffsetL", "-0.1332000017"),
            ("DelayLine_OffsetR", "0.3330000043"),
            ("Feedback", "0.9499999881"),
            ("Freeze", "false"),
            ("Filter_On", "true"),
            ("Filter_Frequency", "999.999878"),
            ("Filter_Bandwidth", "8"),
            ("Modulation_TimeMode", "2"),
            ("Modulation_Frequency", "0.5"),
            ("Modulation_Time", "1.00000024"),
            ("Modulation_SyncedRate", "4.4"),
            ("Modulation_Sixteenth", "16"),
            ("Modulation_Waveform", "6"),
            ("Modulation_Morph", "-1"),
            ("Modulation_AmountTime", "0.25"),
            ("Modulation_AmountFilter", "1"),
            ("DryWet", "0.5"),
            ("DelayLine_SimpleDelayTimeL", "100"),
        ]
        .iter()
        .map(|(name, value)| manual_xml(name, value))
        .collect();
        let (id, params, warnings) = mapped("Delay", &xml);
        assert_eq!(id, "delay");
        assert!(warnings.is_empty(), "{warnings:?}");
        // Every element but the legacy simple-delay time.
        assert_eq!(params.len(), 26);
        let near = |id: &str, expected: f64| {
            let got = param(&params, id);
            assert!((got - expected).abs() < 1e-6, "{id}: {got} against {expected}");
        };
        near("transition", 1.0);
        near("link", 0.0);
        near("pingPong", 1.0);
        near("timeLeft", 0.3749999404);
        near("sixteenthsRight", 7.0);
        near("offsetLeft", -0.1332000017);
        near("feedback", 0.9499999881);
        near("filterWidth", 8.0);
        near("lfoMode", 2.0);
        near("lfoSynced", 4.0);
        near("lfoShape", 6.0);
        near("lfoMorph", -1.0);
        near("mix", 0.5);
    }

    #[test]
    fn delay_steps_outside_their_lists_are_left_out() {
        let xml = manual_xml("DelayLine_SmoothingMode", "3") + &manual_xml("Modulation_Waveform", "nine");
        let (_, params, warnings) = mapped("Delay", &xml);
        assert!(params.iter().all(|p| p.id != "transition" && p.id != "lfoShape"), "{params:?}");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    #[test]
    fn other_devices_are_not_mapped() {
        let doc = roxmltree::Document::parse("<Eq8/>").unwrap_or_else(|e| panic!("{e}"));
        assert!(builtin(doc.root_element(), &mut |_| {}).is_none());
    }
}
