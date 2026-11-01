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
        "Saturator" => Some(("saturator", "Saturator", saturator(node, warn))),
        "Eq8" => Some(("eq", "EQ", eq(node, warn))),
        "FilterEQ3" => Some(("dj-eq", "DJ EQ", eq3(node, warn))),
        "MidiPitcher" => Some(("pitch", "Pitch", pitch(node, warn))),
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

/// Color Low and High at ±100 %: Live stores their gains in decibels.
const SATURATOR_COLOR_LOW_DB: f64 = 36.0;
const SATURATOR_COLOR_HIGH_DB: f64 = 24.0;

/// Live's Saturator as Suhina's `saturator` (`docs/spec/saturator.md` › Import).
fn saturator(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut p = Params::default();
    let flag = |name| manual_flag(node, name).map(toggle);
    p.set("drive", manual(node, "PreDrive"));
    p.set("type", step(node, "Type", 8, "Saturator curve type", warn));
    p.set("threshold", manual(node, "BassShaperThreshold"));
    p.set("postClip", step(node, "PostClip", 3, "Saturator post clip mode", warn));
    p.set("output", manual(node, "PostDrive"));
    p.set("mix", manual(node, "DryWet"));
    p.set("color", flag("ColorOn"));
    // The Color amounts are stored as the filters' gains in dB; ours are fractions of the most.
    p.set("colorLow", manual(node, "BaseDrive").map(|db| db / SATURATOR_COLOR_LOW_DB));
    p.set("colorHigh", manual(node, "ColorDepth").map(|db| db / SATURATOR_COLOR_HIGH_DB));
    p.set("colorFrequency", manual(node, "ColorFrequency"));
    p.set("colorWidth", manual(node, "ColorWidth"));
    p.set("shaperDrive", manual(node, "WsDrive"));
    p.set("shaperCurve", manual(node, "WsCurve"));
    p.set("shaperDepth", manual(node, "WsDepth"));
    p.set("shaperLinear", manual(node, "WsLin"));
    p.set("shaperDamp", manual(node, "WsDamp"));
    p.set("shaperPeriod", manual(node, "WsPeriod"));
    p.set("dcFilter", flag("PreDcFilter"));
    p.0
}

/// Bands of Live's EQ Eight, and the two parameter sets of each band, as Suhina's `eq` names its
/// curves.
const EQ_BANDS: usize = 8;
const EQ_CURVES: [(&str, &str); 2] = [("ParameterA", "a"), ("ParameterB", "b")];

/// Live's EQ Eight (`Eq8`) as Suhina's `eq` (`docs/spec/eq.md` › Import).
fn eq(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut p = Params::default();
    p.set("output", manual(node, "GlobalGain"));
    p.set("scale", manual(node, "Scale"));
    p.set("adaptiveQ", manual_flag(node, "AdaptiveQ").map(toggle));
    // The mode is a plain value on the device, not an automatable one.
    if path(node, &["Mode"]).is_some() {
        match number(node, &["Mode"]).map(f64::round) {
            Some(mode) if (0.0..3.0).contains(&mode) => p.set("mode", Some(mode)),
            _ => warn("EQ Eight mode is not understood; using stereo".to_string()),
        }
    }
    if number(node, &["Precision"]).is_some_and(|v| v != 0.0) {
        warn("EQ Eight oversampling is not available; the EQ runs at the project's rate".to_string());
    }
    for band in 0..EQ_BANDS {
        let element = format!("Bands.{band}");
        let Some(bands) = path(node, &[element.as_str()]) else {
            continue;
        };
        for (set, curve) in EQ_CURVES {
            let Some(values) = path(bands, &[set]) else {
                continue;
            };
            let id = |field: &str| format!("{curve}{}{field}", band + 1);
            p.set(&id("On"), manual_flag(values, "IsOn").map(toggle));
            p.set(&id("Type"), step(values, "Mode", 8, "EQ Eight filter type", warn));
            p.set(&id("Frequency"), manual(values, "Freq"));
            p.set(&id("Gain"), manual(values, "Gain"));
            p.set(&id("Q"), manual(values, "Q"));
        }
    }
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

/// Live's EQ Three (`FilterEQ3`) as Suhina's `dj-eq` (`docs/spec/dj-eq.md` › Import).
fn eq3(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut p = Params::default();
    // The band gains are stored as linear gains; ours are decibels, held in the device's range
    // (silence at the bottom, +6 dB at the top), whatever the project file says.
    let gain = |name: &str| {
        manual(node, name).map(|g| {
            (if g > 0.0 { 20.0 * g.log10() } else { SILENCE_DB })
                .clamp(suhina_device_dj_eq::GAIN_RANGE.0, suhina_device_dj_eq::GAIN_RANGE.1)
        })
    };
    p.set("low", gain("GainLo"));
    p.set("mid", gain("GainMid"));
    p.set("high", gain("GainHi"));
    // A stored band switch says the band is on; ours is its kill.
    let kill = |name: &str| manual_flag(node, name).map(|on| toggle(!on));
    p.set("lowKill", kill("LowOn"));
    p.set("midKill", kill("MidOn"));
    p.set("highKill", kill("HighOn"));
    p.set("lowFrequency", manual(node, "FreqLo"));
    p.set("highFrequency", manual(node, "FreqHi"));
    // Stored 0 is 24 dB/octave and 1 is 48 dB, the same order as ours.
    // Unlike the other step values, a slope between the two settings is not rounded into one.
    let slope = match manual(node, "Slope") {
        Some(stored) if stored.fract() != 0.0 => {
            warn(format!("EQ Three slope {stored} is not understood; using the default"));
            None
        }
        _ => step(node, "Slope", 2, "EQ Three slope", warn),
    };
    p.set("slope", slope);
    p.0
}

/// Live's Pitch (`MidiPitcher`) as Suhina's `pitch`. Every value is stored in the unit ours
/// takes (semitones, scale degrees, MIDI keys); the step widths have no counterpart yet.
fn pitch(node: Node, warn: &mut dyn FnMut(String)) -> Vec<DeviceParam> {
    let mut params = Vec::new();
    let mut set = |id: &str, value: Option<f64>| {
        if let Some(value) = value {
            params.push(DeviceParam::new(id, value));
        }
    };
    set("pitch", manual(node, "Pitch"));
    set("pitchDegrees", manual(node, "PitchScaleDegrees"));
    set("followScale", manual_flag(node, "UseCurrentScale").map(toggle));
    set("lowest", manual(node, "Lowest"));
    set("range", manual(node, "Range"));
    // 0 Block, 1 Fold, 2 Limit, the same order as ours.
    let mode = manual(node, "RangeMode").map(|mode| match mode as i64 {
        mode @ 0..=2 => mode as f64,
        other => {
            warn(format!("Pitch mode {other} is not understood; using Block"));
            0.0
        }
    });
    set("mode", mode);
    params
}

#[cfg(test)]
#[path = "devices_tests.rs"]
mod tests;
