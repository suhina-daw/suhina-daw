// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live's own devices that map onto Suhina's built-in effects and instruments (ADR 0004). Each mapping follows
//! `docs/spec/als-format.md` › Devices: the element Live stores, the unit of every value, and
//! the Suhina parameter it becomes. Parameter ids are the devices' stable ids
//! (`devices/<name>` in this repository).

use roxmltree::Node;
use suhina_model::DeviceParam;

use crate::{boolean, number, path, value};

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

/// What a Simpler or a one-zone Sampler becomes: the Suhina sampler's values, its sample file
/// (as stored, relative when Live stored it so, and absolute) and, for Manual slicing, its
/// boundaries as fractions of the file.
pub(crate) struct SamplerImport {
    pub(crate) params: Vec<DeviceParam>,
    pub(crate) file: Option<(String, Option<String>)>,
    pub(crate) slices: Vec<f64>,
}

/// The voices Live offers, by the index `Globals/NumVoices` stores (observed: index 5 showed 6).
const VOICES: [f64; 14] = [1., 2., 3., 4., 5., 6., 8., 10., 12., 14., 16., 20., 24., 32.];

/// An automatable value: `<names…>/Manual@Value`.
fn auto(node: Node, names: &[&str]) -> Option<f64> {
    let mut full = names.to_vec();
    full.push("Manual");
    number(node, &full)
}

fn auto_on(node: Node, names: &[&str]) -> bool {
    let mut full = names.to_vec();
    full.push("Manual");
    boolean(node, &full).unwrap_or(false)
}

/// A time stored in milliseconds under `base`, in seconds up to Suhina's 10.
fn seconds(node: Node, base: &[&str], name: &str) -> Option<f64> {
    let mut full = base.to_vec();
    full.push(name);
    auto(node, &full).map(|ms| (ms / 1000.).clamp(0., 10.))
}

/// A level of 0 … 1 under `base`.
fn level(node: Node, base: &[&str], name: &str) -> Option<f64> {
    let mut full = base.to_vec();
    full.push(name);
    auto(node, &full).map(|l| l.clamp(0., 1.))
}

/// Maps Live's Simpler (`OriginalSimpler`) or a Sampler (`MultiSampler`) with one sample onto
/// the Suhina sampler (`docs/spec/als-format.md` › Simpler and Sampler). `None` for a Sampler
/// with several samples, which Suhina's sampler cannot hold yet (#739).
pub(crate) fn sampler(node: Node, warn: &mut dyn FnMut(String)) -> Option<SamplerImport> {
    let parts: Vec<Node> = path(node, &["Player", "MultiSampleMap", "SampleParts"])
        .map(|p| p.children().filter(|n| n.has_tag_name("MultiSamplePart")).collect())
        .unwrap_or_default();
    if parts.len() > 1 {
        warn(format!("a Sampler with {} samples is imported as a placeholder (one sample only so far)", parts.len()));
        return None;
    }
    let mut p = Params::default();
    let mut file = None;
    let mut slices = Vec::new();
    let part = parts.first().copied();
    let part_number = |names: &[&str]| part.and_then(|part| number(part, names));
    let loop_on = auto_on(node, &["Player", "LoopModulators", "LoopOn"]);
    if let Some(part) = part {
        let file_ref = path(part, &["SampleRef", "FileRef"]);
        let relative = file_ref.and_then(|f| value(f, &["RelativePath"])).filter(|p| !p.is_empty());
        let absolute = file_ref.and_then(|f| value(f, &["Path"])).filter(|p| !p.is_empty());
        file = relative.or(absolute).map(|f| (f.to_string(), absolute.map(str::to_string)));
        let duration = number(part, &["SampleRef", "DefaultDuration"]).filter(|d| *d >= 2.);
        let rate = number(part, &["SampleRef", "DefaultSampleRate"]).filter(|r| *r > 0.);
        if let Some(duration) = duration {
            // The zone's region in frames, and Simpler's start and length within it.
            let first = number(part, &["SampleStart"]).unwrap_or(0.).clamp(0., duration);
            let last = (number(part, &["SampleEnd"]).unwrap_or(duration - 1.) + 1.).clamp(first, duration);
            let span = last - first;
            let modulators = ["Player", "LoopModulators"];
            let start_at = auto(node, &[&modulators[..], &["SampleStart"]].concat()).unwrap_or(0.).clamp(0., 1.);
            let length = auto(node, &[&modulators[..], &["SampleLength"]].concat()).unwrap_or(1.).clamp(0., 1.);
            let start = first + span * start_at;
            let end = (start + span * length).min(last);
            p.set("start", Some(start / duration));
            p.set("end", Some(end / duration));
            if loop_on {
                let loop_length = auto(node, &[&modulators[..], &["LoopLength"]].concat()).unwrap_or(1.).clamp(0., 1.);
                p.set("loop_start", Some((end - (end - start) * loop_length) / duration));
                p.set("loop_end", Some(end / duration));
            }
            if let Some(rate) = rate
                && number(part, &["SlicingStyle"]) == Some(3.)
            {
                slices = path(part, &["ManualSlicePoints"])
                    .map(|m| {
                        m.children()
                            .filter(|n| n.has_tag_name("SlicePoint"))
                            .filter_map(|s| s.attribute("TimeInSeconds").and_then(|t| t.trim().parse::<f64>().ok()))
                            .map(|t| t * rate / duration)
                            .filter(|f| f.is_finite() && *f > 0. && *f < 1.)
                            .collect()
                    })
                    .unwrap_or_default();
            }
        }
        p.set("root", part_number(&["RootKey"]).map(|k| k.clamp(0., 127.)));
        p.set("key_low", part_number(&["KeyRange", "Min"]).map(|k| k.clamp(0., 127.)));
        p.set("key_high", part_number(&["KeyRange", "Max"]).map(|k| k.clamp(0., 127.)));
        p.set("velocity_low", part_number(&["VelocityRange", "Min"]).map(|v| (v / 127.).clamp(0., 1.)));
        p.set("velocity_high", part_number(&["VelocityRange", "Max"]).map(|v| (v / 127.).clamp(0., 1.)));
        if boolean(part, &["SampleWarpProperties", "IsWarped"]) == Some(true) {
            p.set("warp", Some(1.));
            match value(part, &["SampleWarpProperties", "WarpMode"]) {
                // Observed on clips: WarpMode 4 is Complex, Suhina's Smooth (index 4).
                Some("4") => p.set("warp_mode", Some(4.)),
                other => warn(format!("sampler warp mode {other:?} is not mapped yet; using the default")),
            }
            // The sample's own tempo, from its first and last warp markers; without them the
            // tempo the file itself says is used when it loads.
            let markers: Vec<(f64, f64)> = path(part, &["SampleWarpProperties", "WarpMarkers"])
                .map(|m| {
                    m.children()
                        .filter(|n| n.has_tag_name("WarpMarker"))
                        .filter_map(|n| {
                            let read = |name: &str| n.attribute(name).and_then(|t| t.trim().parse::<f64>().ok());
                            Some((read("SecTime")?, read("BeatTime")?))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let (Some(first), Some(last)) = (markers.first(), markers.last()) {
                let bpm = 60. * (last.1 - first.1) / (last.0 - first.0);
                p.set("source_bpm", Some(bpm).filter(|b| b.is_finite() && *b > 0.).map(|b| b.clamp(1., 999.)));
            }
        }
    }

    // Classic (0), One-Shot (1) or Slicing (2); a Sampler stores 0. Trigger (0) or Gate (1).
    let mode = number(node, &["Globals", "PlaybackMode"]).unwrap_or(0.);
    let gate = auto(node, &["VolumeAndPan", "OneShotEnvelope", "SustainMode"]).unwrap_or(0.) > 0.;
    match mode as i64 {
        1 => {
            p.set("playback", Some(if gate { 0. } else { 1. }));
            let fades = ["VolumeAndPan", "OneShotEnvelope"];
            p.set("fade_in", auto(node, &[&fades[..], &["FadeInTime"]].concat()).map(|ms| ms.clamp(0., 1000.)));
            p.set("fade_out", auto(node, &[&fades[..], &["FadeOutTime"]].concat()).map(|ms| ms.clamp(0., 1000.)));
        }
        2 => {
            let part_value = |name: &str| part.and_then(|part| number(part, &[name]));
            p.set("mode", Some(2.));
            p.set("slice_by", part_value("SlicingStyle").filter(|s| (0. ..=3.).contains(s)));
            p.set("sensitivity", part_value("SlicingThreshold").map(|t| (t / 100.).clamp(0., 1.)));
            p.set("slice_regions", part_value("SlicingRegions").map(|r| r.clamp(1., 128.)));
            p.set(
                "slice_overlap",
                number(node, &["SimplerSlicing", "PlaybackMode"]).filter(|m| (0. ..=2.).contains(m)),
            );
            p.set("slice_release", Some(toggle(gate)));
            if part_value("SlicingStyle") == Some(1.) {
                warn("sampler beat slicing division is not mapped yet; using the default".into());
            }
        }
        _ => p.set("playback", Some(if loop_on { 2. } else { 0. })),
    }

    p.set("reverse", Some(toggle(auto_on(node, &["Player", "Reverse"]))));
    p.set("pitch", auto(node, &["Pitch", "TransposeKey"]).map(|st| st.clamp(-48., 48.)));
    let fine = auto(node, &["Pitch", "TransposeFine"]).unwrap_or(0.) + part_number(&["Detune"]).unwrap_or(0.);
    p.set("cents", Some(fine.clamp(-100., 100.)));
    // A zone at zero is silent whatever the device's gain; one with no volume is at unity.
    let part_gain = part_number(&["Volume"]).unwrap_or(1.);
    let volume = auto(node, &["VolumeAndPan", "Volume"]).unwrap_or(0.) + 20. * part_gain.max(1e-9).log10();
    p.set("gain", Some(if part_gain > 0. { volume.clamp(-60., 12.) } else { -60. }));
    let pan = auto(node, &["VolumeAndPan", "Panorama"]).unwrap_or(0.) + part_number(&["Panorama"]).unwrap_or(0.);
    p.set("pan", Some(pan.clamp(-1., 1.)));
    p.set("velocity", auto(node, &["VolumeAndPan", "VolumeVelScale"]).map(|v| v.clamp(0., 1.)));

    // The amplitude envelope: times in ms, the sustain a level of 0 … 1.
    let envelope = ["VolumeAndPan", "Envelope"];
    p.set("attack", seconds(node, &envelope, "AttackTime"));
    p.set("decay", seconds(node, &envelope, "DecayTime"));
    p.set("sustain", level(node, &envelope, "SustainLevel"));
    p.set("release", seconds(node, &envelope, "ReleaseTime"));

    // The filter: Low-pass, High-pass, Band-pass, Notch, Morph (0 … 4) → Suhina's 1 … 4.
    let filter = ["Filter", "Slot", "Value", "SimplerFilter"];
    let field = |name: &str| auto(node, &[&filter[..], &[name]].concat());
    let cutoff = field("Freq").unwrap_or(20000.);
    let filter_env = ["Filter", "Slot", "Value", "SimplerFilter", "Envelope"];
    let env_amount = if auto_on(node, &[&filter_env[..], &["IsOn"]].concat()) {
        auto(node, &[&filter_env[..], &["Amount"]].concat())
    } else {
        None
    };
    let kind = field("Type").unwrap_or(0.) as i64;
    let open_low_pass = kind == 0 && cutoff >= 20000. && env_amount.unwrap_or(0.) == 0.;
    if !auto_on(node, &["Filter", "IsOn"]) || open_low_pass {
        // Off, or a low-pass open above hearing: the same sound with no filter.
        p.set("filter", Some(0.));
    } else {
        let mapped = match kind {
            0..=3 => kind as f64 + 1.,
            _ => {
                warn("sampler filter Morph is not mapped yet; using a low-pass".into());
                1.
            }
        };
        p.set("filter", Some(mapped));
        p.set("cutoff", Some(cutoff.clamp(20., 20000.)));
        p.set("filter_slope", Some(toggle(auto_on(node, &[&filter[..], &["Slope"]].concat()))));
        // Resonance 0 … 1.25 onto a Q from 0.707 (none) to about 10 at 1.
        p.set("resonance", field("Res").map(|r| (0.707 * 14.14f64.powf(r.clamp(0., 1.25))).clamp(0.1, 10.)));
        if let Some(amount) = env_amount {
            p.set("filter_env", Some((amount / 12.).clamp(-8., 8.)));
            p.set("filter_attack", seconds(node, &filter_env, "AttackTime"));
            p.set("filter_decay", seconds(node, &filter_env, "DecayTime"));
            p.set("filter_sustain", level(node, &filter_env, "SustainLevel"));
            p.set("filter_release", seconds(node, &filter_env, "ReleaseTime"));
        }
    }

    // The pitch envelope: an amount in semitones, and its attack and decay.
    let pitch_env = ["Pitch", "Envelope", "Slot", "Value", "SimplerPitchEnvelope"];
    if auto_on(node, &["Pitch", "Envelope", "IsOn"]) {
        p.set("pitch_env", auto(node, &[&pitch_env[..], &["Amount"]].concat()).map(|st| st.clamp(-48., 48.)));
        p.set("pitch_attack", seconds(node, &pitch_env, "AttackTime"));
        p.set("pitch_decay", seconds(node, &pitch_env, "DecayTime"));
    }
    if auto_on(node, &["Lfo", "IsOn"]) {
        warn("sampler LFO is not mapped yet; it is left off".into());
    }

    // Voices, retrigger and glide.
    if let Some(voices) = number(node, &["Globals", "NumVoices"]).and_then(|i| VOICES.get(i as usize).copied()) {
        p.set("poly", Some(voices));
        p.set("voice_mode", Some(toggle(voices == 1.)));
    }
    p.set("retrigger", boolean(node, &["Globals", "RetriggerMode"]).map(toggle));
    if auto(node, &["Globals", "PortamentoMode"]).unwrap_or(0.) > 0. {
        p.set("glide", auto(node, &["Globals", "PortamentoTime"]).map(|ms| (ms / 1000.).clamp(0., 2.)));
    }
    Some(SamplerImport { params: p.0, file, slices })
}
