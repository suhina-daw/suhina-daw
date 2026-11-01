// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

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
    for (stored, db) in [("1", 0.0), ("0.2511886358", -12.0), ("1.99526238", 6.0), ("56.2341309", 35.0), ("0", -70.0)] {
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
        let (params, _) = utility_from(&(manual_xml("StereoWidth", "4") + &manual_xml("MidSideBalance", stored) + on));
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

/// Live's Saturator with every value away from its default, as the calibration set of the
/// device notes stores it.
const SATURATOR_CHANGED: [(&str, &str); 18] = [
    ("PreDrive", "-14.4"),
    ("PreDcFilter", "true"),
    ("Type", "1"),
    ("ColorOn", "false"),
    ("BaseDrive", "-14.4"),
    ("ColorFrequency", "206.1277004"),
    ("ColorWidth", "0.7"),
    ("ColorDepth", "1"),
    ("PostClip", "1"),
    ("PostDrive", "-1"),
    ("DryWet", "0"),
    ("BassShaperThreshold", "-49"),
    ("WsDrive", "0"),
    ("WsLin", "0.3"),
    ("WsCurve", "0.7"),
    ("WsDamp", "1"),
    ("WsPeriod", "1"),
    ("WsDepth", "1"),
];

#[test]
fn saturator_takes_every_parameter_in_our_units() {
    let xml: String = SATURATOR_CHANGED.iter().map(|(name, value)| manual_xml(name, value)).collect();
    let (id, params, warnings) = mapped("Saturator", &xml);
    assert_eq!(id, "saturator");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(params.len(), 18);
    let expected = [
        ("drive", -14.4),
        ("dcFilter", 1.0),
        ("type", 1.0),
        ("color", 0.0),
        // -14.4 dB of the shelf's 36 is -40 %; 1 dB of the bell's 24 is about 4.2 %.
        ("colorLow", -0.4),
        ("colorFrequency", 206.1277004),
        ("colorWidth", 0.7),
        ("colorHigh", 1.0 / 24.0),
        ("postClip", 1.0),
        ("output", -1.0),
        ("mix", 0.0),
        ("threshold", -49.0),
        ("shaperDrive", 0.0),
        ("shaperLinear", 0.3),
        ("shaperCurve", 0.7),
        ("shaperDamp", 1.0),
        ("shaperPeriod", 1.0),
        ("shaperDepth", 1.0),
    ];
    for (id, value) in expected {
        let got = param(&params, id);
        assert!((got - value).abs() < 1e-9, "{id}: {got} against {value}");
    }
}

#[test]
fn saturator_steps_outside_their_lists_are_left_out() {
    let xml = manual_xml("Type", "8") + &manual_xml("PostClip", "-1") + &manual_xml("PreDrive", "loud");
    let (_, params, warnings) = mapped("Saturator", &xml);
    assert!(params.is_empty(), "{params:?}");
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

/// The parameters and warnings `builtin` maps from a `MidiPitcher` element holding `inner`,
/// checked to land on Suhina's pitch effect.
fn pitch_from(inner: &str) -> (Vec<DeviceParam>, Vec<String>) {
    let xml = format!("<MidiPitcher>{inner}</MidiPitcher>");
    let doc = roxmltree::Document::parse(&xml).unwrap_or_else(|e| panic!("{e}"));
    let mut warnings = Vec::new();
    let (id, name, params) =
        builtin(doc.root_element(), &mut |w| warnings.push(w)).unwrap_or_else(|| panic!("not mapped"));
    assert_eq!((id, name), ("pitch", "Pitch"));
    (params, warnings)
}

/// Every value Live stores for its pitch effect lands on a pitch parameter, without warnings,
/// and the step widths, which have no parameter, are left out.
#[test]
fn pitch_takes_every_stored_value() {
    let xml = [
        manual_xml("Pitch", "-24"),
        manual_xml("PitchScaleDegrees", "3"),
        manual_xml("Lowest", "48"),
        manual_xml("Range", "11"),
        manual_xml("RangeMode", "1"),
        manual_xml("UseCurrentScale", "true"),
        manual_xml("StepWidth", "12"),
        manual_xml("StepWidthScaleDegrees", "7"),
    ]
    .concat();
    let (params, warnings) = pitch_from(&xml);
    let expected = [
        ("pitch", -24.0),
        ("pitchDegrees", 3.0),
        ("lowest", 48.0),
        ("range", 11.0),
        ("mode", 1.0),
        ("followScale", 1.0),
    ];
    for (id, value) in expected {
        assert_eq!(param(&params, id), value, "{id}");
    }
    assert_eq!(params.len(), expected.len(), "the step widths have nowhere to go: {params:?}");
    assert!(warnings.is_empty());
    let ids: Vec<&str> = suhina_device_pitch::INFO.params.iter().map(|p| p.id).collect();
    assert!(params.iter().all(|p| ids.contains(&p.id.as_str())), "{params:?}");
}

/// The three range modes map one to one; an unknown mode falls back to the first with a warning,
/// and values that are not numbers are left out.
#[test]
fn pitch_modes_and_bad_values() {
    for (stored, ours) in [("0", 0.0), ("2", 2.0)] {
        assert_eq!(param(&pitch_from(&manual_xml("RangeMode", stored)).0, "mode"), ours);
    }
    let (params, warnings) = pitch_from(&manual_xml("RangeMode", "5"));
    assert_eq!((param(&params, "mode"), warnings.len()), (0.0, 1));
    let (params, _) = pitch_from(&(manual_xml("Pitch", "up") + &manual_xml("UseCurrentScale", "maybe")));
    assert!(params.is_empty(), "{params:?}");
}

/// Live's EQ Three (`FilterEQ3`) as a freshly loaded preset stores it: the gains as linear
/// gains, the band switches as bools, the slope as an index where 1 is the steeper setting.
#[test]
fn eq_three_takes_every_stored_value() {
    let xml = [
        manual_xml("GainLo", "1"),
        manual_xml("GainMid", "1.99526238"),
        manual_xml("GainHi", "0.5011872"),
        manual_xml("LowOn", "false"),
        manual_xml("MidOn", "true"),
        manual_xml("HighOn", "true"),
        manual_xml("FreqLo", "250"),
        manual_xml("FreqHi", "2500"),
        manual_xml("Slope", "1"),
    ]
    .concat();
    let (id, params, warnings) = mapped("FilterEQ3", &xml);
    assert_eq!(id, "dj-eq");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(params.len(), 9);
    let near = |id: &str, expected: f64| {
        let got = param(&params, id);
        assert!((got - expected).abs() < 1e-3, "{id}: {got} against {expected}");
    };
    near("low", 0.0);
    near("mid", 6.0);
    near("high", -6.0);
    near("lowKill", 1.0);
    near("midKill", 0.0);
    near("highKill", 0.0);
    near("lowFrequency", 250.0);
    near("highFrequency", 2500.0);
    near("slope", 1.0);
    let ids: Vec<&str> = suhina_device_dj_eq::INFO.params.iter().map(|p| p.id).collect();
    assert!(params.iter().all(|p| ids.contains(&p.id.as_str())), "{params:?}");
}

/// A stored gain of 0 is silence, and a slope outside its two settings is noted and left out.
#[test]
fn eq_three_zero_gains_and_odd_slopes() {
    let xml = manual_xml("GainLo", "0") + &manual_xml("Slope", "7");
    let (_, params, warnings) = mapped("FilterEQ3", &xml);
    assert_eq!(param(&params, "low"), SILENCE_DB);
    assert!(params.iter().all(|p| p.id != "slope"), "{params:?}");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

/// A slope between two settings is not rounded into one of them: it is noted and left out.
#[test]
fn eq_three_fractional_slope_is_not_rounded() {
    let (_, params, warnings) = mapped("FilterEQ3", &manual_xml("Slope", "1.25"));
    assert!(params.iter().all(|p| p.id != "slope"), "{params:?}");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
}

/// A gain past the device's range is held there, whatever the project file says.
#[test]
fn eq_three_out_of_range_gains_are_clamped() {
    let xml = manual_xml("GainHi", "16") + &manual_xml("GainLo", "0.01") + &manual_xml("GainMid", "0.000001");
    let (_, params, warnings) = mapped("FilterEQ3", &xml);
    assert_eq!(param(&params, "high"), 6.0, "+24 dB is held at the device's top");
    assert_eq!(param(&params, "low"), -40.0, "−40 dB is inside the range and is kept");
    assert_eq!(param(&params, "mid"), SILENCE_DB, "−120 dB is held at silence");
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// One band's two parameter sets as a saved set stores them.
fn eq_band(band: usize, a: [&str; 5], b: [&str; 5]) -> String {
    let set = |name: &str, v: [&str; 5]| {
        let fields: String =
            ["IsOn", "Mode", "Freq", "Gain", "Q"].iter().zip(v).map(|(f, v)| manual_xml(f, v)).collect();
        format!("<{name}>{fields}</{name}>")
    };
    format!("<Bands.{band}>{}{}</Bands.{band}>", set("ParameterA", a), set("ParameterB", b))
}

#[test]
fn eq_eight_takes_both_parameter_sets_of_every_band() {
    // Plain values may come with spaces or as 0.0; they read as numbers.
    let mut xml = String::from(r#"<Precision Value="0.0" /><Mode Value=" 2" /><EditMode Value="false" />"#);
    xml += &manual_xml("GlobalGain", "-3.5");
    xml += &manual_xml("Scale", "-0.5");
    xml += &manual_xml("AdaptiveQ", "false");
    for band in 0..8 {
        let freq = format!("{}", 100 * (band + 1));
        xml += &eq_band(band, ["true", "3", &freq, "4.5", "2"], ["false", "7", "18000", "0", "0.7071067095"]);
    }
    let (id, params, warnings) = mapped("Eq8", &xml);
    assert_eq!(id, "eq");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(params.len(), 84);
    let near = |id: &str, expected: f64| {
        let got = param(&params, id);
        assert!((got - expected).abs() < 1e-9, "{id}: {got} against {expected}");
    };
    near("output", -3.5);
    near("scale", -0.5);
    near("adaptiveQ", 0.0);
    near("mode", 2.0);
    near("a1On", 1.0);
    near("a1Type", 3.0);
    near("a8Frequency", 800.0);
    near("a3Gain", 4.5);
    near("a5Q", 2.0);
    near("b2On", 0.0);
    near("b7Type", 7.0);
    near("b8Q", 0.7071067095);
}

#[test]
fn eq_eight_odd_values_are_noted() {
    let xml = format!(
        r#"<Precision Value="1" /><Mode Value="5" />{}"#,
        eq_band(0, ["true", "9", "100", "0", "1"], ["true", "1", "x", "0", "1"])
    );
    let (_, params, warnings) = mapped("Eq8", &xml);
    // Oversampling, the mode and band 1's type are noted; the type and the unreadable
    // frequency are left out.
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(params.iter().all(|p| p.id != "mode" && p.id != "a1Type" && p.id != "b1Frequency"), "{params:?}");
    assert_eq!(param(&params, "b1Type"), 1.0);
}

#[test]
fn other_devices_are_not_mapped() {
    let doc = roxmltree::Document::parse("<Compressor2/>").unwrap_or_else(|e| panic!("{e}"));
    assert!(builtin(doc.root_element(), &mut |_| {}).is_none());
}
