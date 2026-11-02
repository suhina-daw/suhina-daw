// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The FM synth's parameters: 22 per operator (operators 1 to 6), the voice's global ones in
//! the order a voice stores them, then performance settings (bend, portamento, what the
//! wheels and pedals do, volume), then the parts the DX7 format does not have (ADR 0034): each
//! is neutral by default, so an old project and a voice file sound as they did.

use suhina_device_api::{ParamInfo, Scale, Unit};

pub const OP_PARAMS: usize = 22;
/// Index of the first global voice parameter.
pub const GLOBAL: usize = 6 * OP_PARAMS;
/// Global voice parameters, stored one after another from the voice's pitch envelope on.
pub const GLOBAL_COUNT: usize = 19;
/// Index of the first performance parameter.
pub const PERFORMANCE: usize = GLOBAL + GLOBAL_COUNT;

pub const PITCH_BEND_RANGE: usize = PERFORMANCE;
pub const PITCH_BEND_STEP: usize = PERFORMANCE + 1;
pub const PORTAMENTO: usize = PERFORMANCE + 2;
pub const GLISSANDO: usize = PERFORMANCE + 3;
pub const PORTAMENTO_TIME: usize = PERFORMANCE + 4;
/// Four parameters (range, pitch, amp, EG bias) per source, in [`SOURCES`] order.
pub const ROUTES: usize = PERFORMANCE + 5;
pub const VOLUME: usize = ROUTES + 16;
/// Index of the first parameter a voice file does not hold.
pub const EXTENDED: usize = VOLUME + 1;
pub const PAN: usize = EXTENDED;
pub const PAN_BY_KEY: usize = EXTENDED + 1;
pub const PAN_RANDOM: usize = EXTENDED + 2;
pub const SPREAD: usize = EXTENDED + 3;
pub const VOICE_COUNT: usize = EXTENDED + 4;
pub const TONE: usize = EXTENDED + 5;
/// Scales every time envelope.
pub const TIME: usize = EXTENDED + 6;
pub const RETRIGGER: usize = EXTENDED + 7;
pub const ANTIALIAS: usize = EXTENDED + 8;
pub const INTERPOLATION: usize = EXTENDED + 9;
const GLOBAL_EXT: usize = 10;

/// The filter's fields, then its envelope.
pub const FILTER: usize = EXTENDED + GLOBAL_EXT;
pub const FILTER_FIELDS: usize = 14;
pub const FILTER_ENV: usize = FILTER + FILTER_FIELDS;
/// The LFO's fields, then its envelope.
pub const LFO: usize = FILTER_ENV + ENV_FIELDS;
pub const LFO_FIELDS: usize = 18;
pub const LFO_ENV: usize = LFO + LFO_FIELDS;
/// The pitch envelope's routing, then the envelope.
pub const PITCH: usize = LFO_ENV + ENV_FIELDS;
pub const PITCH_FIELDS: usize = 11;
pub const PITCH_ENV: usize = PITCH + PITCH_FIELDS;
/// Four parameters per matrix source: first destination, its amount, second, its amount.
pub const MATRIX: usize = PITCH_ENV + ENV_FIELDS;
pub const MATRIX_SOURCES: usize = 9;
/// Per operator, operators 1 to 6: [`OP_EXT_FIELDS`] fields ending in its envelope.
pub const OP_EXT: usize = MATRIX + MATRIX_SOURCES * 4;
pub const OP_EXT_FIELDS: usize = 12 + ENV_FIELDS;
/// Per operator, the amounts of its [`HARMONICS`] partials.
pub const DRAWN: usize = OP_EXT + 6 * OP_EXT_FIELDS;
pub const HARMONICS: usize = 64;
/// Parameters that do not touch the sound: what the panel shows and the randomizer's settings.
/// A voice leaves them alone.
pub const PANEL: usize = DRAWN + 6 * HARMONICS;
pub const SELECTED_OP: usize = PANEL;
pub const PAGE: usize = PANEL + 1;
pub const EXPANDED_PAGE: usize = PANEL + 2;
pub const RANDOM_AMOUNT: usize = PANEL + 3;
pub const RANDOM_SEED: usize = PANEL + 4;
/// Three per randomizer section: its percentage, whether it takes part, whether it is locked.
pub const RANDOM_SECTIONS: usize = PANEL + 5;
pub const SECTIONS: [&str; 10] =
    ["Envelopes", "Levels", "Waves", "Filter", "LFO", "Pitch envelope", "Modulation", "Stereo", "Ratios", "Algorithm"];
const PANEL_COUNT: usize = 5 + 3 * SECTIONS.len();
pub const COUNT: usize = PANEL + PANEL_COUNT;

/// Fields of a time envelope, in parameter order.
pub mod env {
    pub const ATTACK: usize = 0;
    pub const DECAY: usize = 1;
    pub const RELEASE: usize = 2;
    pub const INITIAL: usize = 3;
    pub const PEAK: usize = 4;
    pub const SUSTAIN: usize = 5;
    pub const END: usize = 6;
    pub const LOOP: usize = 7;
    pub const LOOP_TIME: usize = 8;
    pub const REPEAT: usize = 9;
    pub const TIME_BY_VEL: usize = 10;
    pub const TIME_BY_KEY: usize = 11;
    pub const ATTACK_SLOPE: usize = 12;
    pub const DECAY_SLOPE: usize = 13;
    pub const RELEASE_SLOPE: usize = 14;
}
pub const ENV_FIELDS: usize = 15;

/// Fields of the operators' extended block, in parameter order; the envelope follows them.
pub mod opx {
    pub const WAVE: usize = 0;
    pub const FEEDBACK: usize = 1;
    pub const PHASE: usize = 2;
    pub const RETRIGGER: usize = 3;
    pub const FREQ_BY_VEL: usize = 4;
    pub const FREQ_QUANTIZE: usize = 5;
    pub const LEVEL_BY_KEY: usize = 6;
    pub const ENV_MODE: usize = 7;
    pub const PARTIALS: usize = 8;
    pub const REPEAT: usize = 9;
    pub const NORMALIZE: usize = 10;
    pub const ODD_EVEN: usize = 11;
    pub const ENV: usize = 12;
}

/// Fields of the filter, in parameter order.
pub mod filter {
    pub const ON: usize = 0;
    pub const TYPE: usize = 1;
    pub const SLOPE: usize = 2;
    pub const CIRCUIT: usize = 3;
    pub const FREQ: usize = 4;
    pub const RES: usize = 5;
    pub const MORPH: usize = 6;
    pub const DRIVE: usize = 7;
    pub const ENV: usize = 8;
    pub const BY_VEL: usize = 9;
    pub const BY_KEY: usize = 10;
    pub const SHAPER: usize = 11;
    pub const SHAPER_DRIVE: usize = 12;
    pub const SHAPER_MIX: usize = 13;
}

/// Fields of the LFO, in parameter order.
pub mod lfo {
    pub const ON: usize = 0;
    pub const WAVE: usize = 1;
    pub const RANGE: usize = 2;
    pub const RATE: usize = 3;
    pub const SYNC: usize = 4;
    pub const RETRIGGER: usize = 5;
    pub const BY_KEY: usize = 6;
    pub const BY_VEL: usize = 7;
    pub const AMOUNT: usize = 8;
    /// Six toggles, operators 1 to 6.
    pub const TO_OP: usize = 9;
    pub const TO_FILTER: usize = 15;
    pub const DEST_B: usize = 16;
    pub const AMOUNT_B: usize = 17;
}

/// Fields of the pitch envelope's routing, in parameter order.
pub mod pitch {
    pub const ON: usize = 0;
    pub const AMOUNT: usize = 1;
    /// Six toggles, operators 1 to 6.
    pub const TO_OP: usize = 2;
    pub const TO_LFO: usize = 8;
    pub const DEST_B: usize = 9;
    pub const AMOUNT_B: usize = 10;
}

/// The operator waves. The sine is the engine's own; `wave.rs` holds the rest in this order.
pub const WAVES: [&str; 23] = [
    "Sine",
    "Sine 4 Bit",
    "Sine 8 Bit",
    "Saw D",
    "Saw 3",
    "Saw 4",
    "Saw 6",
    "Saw 8",
    "Saw 16",
    "Saw 32",
    "Saw 64",
    "Square D",
    "Square 4",
    "Square 6",
    "Square 8",
    "Square 16",
    "Square 32",
    "Square 64",
    "Triangle 16",
    "Triangle 64",
    "Noise Loop",
    "Noise White",
    "Drawn",
];
/// Where a modulation can go; `modulation.rs` follows this order.
pub const DESTINATIONS: [&str; 25] = [
    "Off",
    "Op 1 Level",
    "Op 2 Level",
    "Op 3 Level",
    "Op 4 Level",
    "Op 5 Level",
    "Op 6 Level",
    "Crossfade 1/3",
    "Crossfade 2/4",
    "Feedback",
    "Fixed Frequency",
    "FM Amount",
    "Filter Freq",
    "Filter Res",
    "Filter Morph",
    "Filter Drive",
    "Filter Env",
    "Shaper Drive",
    "LFO Rate",
    "LFO Amount",
    "Pitch Env",
    "Volume",
    "Pan",
    "Tone",
    "Time",
];
const LOOPS: &[&str] = &["Off", "Loop", "Beat", "Sync", "Trigger"];
/// Repeat lengths of the Beat and Sync loops, in beats; see `time_env.rs`.
pub const REPEATS: &[&str] = &["1/32", "1/16", "1/8", "1/8 D", "1/4", "1/4 D", "1/2", "1 Bar", "2 Bars", "4 Bars"];
pub const REPEAT_BEATS: [f64; 10] = [0.125, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0, 8.0, 16.0];
const SECONDS: Scale = Scale::Logarithmic;

/// Fields of an operator, in parameter order (the first 21 are its stored bytes).
pub mod op {
    pub const LEVEL: usize = 16;
    pub const DETUNE: usize = 20;
    pub const ON: usize = 21;
}

/// The controllers that can modulate, in parameter order.
pub const SOURCES: [&str; 4] = ["wheel", "foot", "breath", "pressure"];

const CURVES: &[&str] = &["-Lin", "-Exp", "+Exp", "+Lin"];
const MODES: &[&str] = &["Ratio", "Fixed"];
const WAVES: &[&str] = &["Triangle", "Saw Down", "Saw Up", "Square", "Sine", "Sample & Hold"];

macro_rules! op_params {
    ($n:literal, $level:literal) => {
        [
            ParamInfo::integer(concat!("op", $n, ".rate1"), concat!("Op ", $n, " Rate 1"), (0, 99), 99, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".rate2"), concat!("Op ", $n, " Rate 2"), (0, 99), 99, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".rate3"), concat!("Op ", $n, " Rate 3"), (0, 99), 99, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".rate4"), concat!("Op ", $n, " Rate 4"), (0, 99), 99, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".level1"), concat!("Op ", $n, " Level 1"), (0, 99), 99, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".level2"), concat!("Op ", $n, " Level 2"), (0, 99), 99, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".level3"), concat!("Op ", $n, " Level 3"), (0, 99), 99, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".level4"), concat!("Op ", $n, " Level 4"), (0, 99), 0, Unit::None),
            ParamInfo::integer(
                concat!("op", $n, ".breakPoint"),
                concat!("Op ", $n, " Break Point"),
                (0, 99),
                39,
                Unit::None,
            ),
            ParamInfo::integer(
                concat!("op", $n, ".leftDepth"),
                concat!("Op ", $n, " Left Depth"),
                (0, 99),
                0,
                Unit::None,
            ),
            ParamInfo::integer(
                concat!("op", $n, ".rightDepth"),
                concat!("Op ", $n, " Right Depth"),
                (0, 99),
                0,
                Unit::None,
            ),
            ParamInfo::choice(concat!("op", $n, ".leftCurve"), concat!("Op ", $n, " Left Curve"), CURVES, 0),
            ParamInfo::choice(concat!("op", $n, ".rightCurve"), concat!("Op ", $n, " Right Curve"), CURVES, 0),
            ParamInfo::integer(
                concat!("op", $n, ".rateScaling"),
                concat!("Op ", $n, " Rate Scaling"),
                (0, 7),
                0,
                Unit::None,
            ),
            ParamInfo::integer(
                concat!("op", $n, ".ampModSens"),
                concat!("Op ", $n, " Amp Mod Sensitivity"),
                (0, 3),
                0,
                Unit::None,
            ),
            ParamInfo::integer(
                concat!("op", $n, ".velocitySens"),
                concat!("Op ", $n, " Velocity Sensitivity"),
                (0, 7),
                0,
                Unit::None,
            ),
            ParamInfo::integer(
                concat!("op", $n, ".level"),
                concat!("Op ", $n, " Output Level"),
                (0, 99),
                $level,
                Unit::None,
            ),
            ParamInfo::choice(concat!("op", $n, ".mode"), concat!("Op ", $n, " Mode"), MODES, 0),
            ParamInfo::integer(concat!("op", $n, ".coarse"), concat!("Op ", $n, " Coarse"), (0, 31), 1, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".fine"), concat!("Op ", $n, " Fine"), (0, 99), 0, Unit::None),
            ParamInfo::integer(concat!("op", $n, ".detune"), concat!("Op ", $n, " Detune"), (-7, 7), 0, Unit::None),
            ParamInfo::toggle(concat!("op", $n, ".on"), concat!("Op ", $n, " On"), true),
        ]
    };
}

const OPS: [[ParamInfo; OP_PARAMS]; 6] =
    [op_params!(1, 99), op_params!(2, 0), op_params!(3, 0), op_params!(4, 0), op_params!(5, 0), op_params!(6, 0)];

const GLOBALS: [ParamInfo; GLOBAL_COUNT] = [
    ParamInfo::integer("pitchRate1", "Pitch Rate 1", (0, 99), 99, Unit::None),
    ParamInfo::integer("pitchRate2", "Pitch Rate 2", (0, 99), 99, Unit::None),
    ParamInfo::integer("pitchRate3", "Pitch Rate 3", (0, 99), 99, Unit::None),
    ParamInfo::integer("pitchRate4", "Pitch Rate 4", (0, 99), 99, Unit::None),
    ParamInfo::integer("pitchLevel1", "Pitch Level 1", (0, 99), 50, Unit::None),
    ParamInfo::integer("pitchLevel2", "Pitch Level 2", (0, 99), 50, Unit::None),
    ParamInfo::integer("pitchLevel3", "Pitch Level 3", (0, 99), 50, Unit::None),
    ParamInfo::integer("pitchLevel4", "Pitch Level 4", (0, 99), 50, Unit::None),
    ParamInfo::integer("algorithm", "Algorithm", (1, 43), 1, Unit::None),
    ParamInfo::integer("feedback", "Feedback", (0, 7), 0, Unit::None),
    ParamInfo::toggle("oscKeySync", "Oscillator Key Sync", true),
    ParamInfo::integer("lfoSpeed", "LFO Speed", (0, 99), 35, Unit::None),
    ParamInfo::integer("lfoDelay", "LFO Delay", (0, 99), 0, Unit::None),
    ParamInfo::integer("lfoPitchDepth", "LFO Pitch Depth", (0, 99), 0, Unit::None),
    ParamInfo::integer("lfoAmpDepth", "LFO Amp Depth", (0, 99), 0, Unit::None),
    ParamInfo::toggle("lfoKeySync", "LFO Key Sync", true),
    ParamInfo::choice("lfoWave", "LFO Wave", WAVES, 0),
    ParamInfo::integer("pitchModSens", "Pitch Mod Sensitivity", (0, 7), 3, Unit::None),
    ParamInfo::integer("transpose", "Transpose", (-24, 24), 0, Unit::None),
];

macro_rules! route_params {
    ($id:literal, $name:literal, $range:literal, $pitch:literal) => {
        [
            ParamInfo::integer(concat!($id, "Range"), concat!($name, " Range"), (0, 99), $range, Unit::None),
            ParamInfo::toggle(concat!($id, "Pitch"), concat!($name, " to Pitch"), $pitch),
            ParamInfo::toggle(concat!($id, "Amp"), concat!($name, " to Amp"), false),
            ParamInfo::toggle(concat!($id, "EgBias"), concat!($name, " to EG Bias"), false),
        ]
    };
}

const PERFORMANCE_PARAMS: [ParamInfo; 5] = [
    ParamInfo::integer("pitchBendRange", "Pitch Bend Range", (0, 12), 2, Unit::None),
    ParamInfo::integer("pitchBendStep", "Pitch Bend Step", (0, 12), 0, Unit::None),
    ParamInfo::toggle("portamento", "Portamento", false),
    ParamInfo::toggle("glissando", "Glissando", false),
    ParamInfo::integer("portamentoTime", "Portamento Time", (0, 99), 0, Unit::None),
];

const ROUTE_PARAMS: [[ParamInfo; 4]; 4] = [
    route_params!("wheel", "Wheel", 50, true),
    route_params!("foot", "Foot", 0, false),
    route_params!("breath", "Breath", 0, false),
    route_params!("pressure", "Pressure", 0, false),
];

/// The group of each operator's parameters.
const OP_GROUPS: [&str; 6] = ["Op 1", "Op 2", "Op 3", "Op 4", "Op 5", "Op 6"];

/// The group of each global voice parameter, in [`GLOBALS`] order.
const GLOBAL_GROUPS: [&str; GLOBAL_COUNT] = [
    "Pitch EG", "Pitch EG", "Pitch EG", "Pitch EG", "Pitch EG", "Pitch EG", "Pitch EG", "Pitch EG", "Voice", "Voice",
    "Voice", "LFO", "LFO", "LFO", "LFO", "LFO", "LFO", "LFO", "Voice",
];

const VOLUME_PARAM: ParamInfo =
    ParamInfo::continuous("volume", "Volume", (-70.0, 6.0), 0.0, Unit::Decibels, Scale::Linear);

const GLOBAL_EXT_PARAMS: [ParamInfo; GLOBAL_EXT] = [
    ParamInfo::continuous("pan", "Pan", (-1.0, 1.0), 0.0, Unit::Balance, Scale::Linear).in_group("Output"),
    ParamInfo::continuous("panByKey", "Pan by Key", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Output"),
    ParamInfo::continuous("panRandom", "Random Pan", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Output"),
    ParamInfo::continuous("spread", "Spread", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Output"),
    ParamInfo::integer("voices", "Voices", (1, 32), 16, Unit::None).in_group("Performance"),
    ParamInfo::continuous("tone", "Tone", (0.0, 1.0), 1.0, Unit::Percent, Scale::Linear).in_group("Output"),
    ParamInfo::continuous("time", "Time", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Output"),
    ParamInfo::toggle("retrigger", "Retrigger", true).in_group("Performance"),
    ParamInfo::toggle("antialias", "Antialias", true).in_group("Performance"),
    ParamInfo::toggle("interpolation", "Interpolation", true).in_group("Performance"),
];

const FILTER_PARAMS: [ParamInfo; FILTER_FIELDS] = [
    ParamInfo::toggle("filterOn", "Filter On", false).in_group("Filter"),
    ParamInfo::choice("filterType", "Filter Type", &["Low-pass", "High-pass", "Band-pass", "Notch", "Morph"], 0)
        .in_group("Filter"),
    ParamInfo::choice("filterSlope", "Filter Slope", &["12 dB", "24 dB"], 1).in_group("Filter"),
    ParamInfo::choice("filterCircuit", "Filter Circuit", &["Clean", "Warm", "Edge", "Bite", "Grit"], 0)
        .in_group("Filter"),
    ParamInfo::continuous("filterFreq", "Filter Freq", (20.0, 20_000.0), 20_000.0, Unit::Hertz, Scale::Logarithmic)
        .in_group("Filter"),
    ParamInfo::continuous("filterRes", "Filter Res", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Filter"),
    ParamInfo::continuous("filterMorph", "Filter Morph", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Filter"),
    ParamInfo::continuous("filterDrive", "Filter Drive", (0.0, 24.0), 0.0, Unit::Decibels, Scale::Linear)
        .in_group("Filter"),
    ParamInfo::continuous("filterEnv", "Filter Env", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Filter"),
    ParamInfo::continuous("filterByVel", "Filter by Velocity", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Filter"),
    ParamInfo::continuous("filterByKey", "Filter by Key", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Filter"),
    ParamInfo::choice("shaper", "Shaper", &["Off", "Soft", "Hard", "Sine", "Fold"], 0).in_group("Filter"),
    ParamInfo::continuous("shaperDrive", "Shaper Drive", (0.0, 24.0), 0.0, Unit::Decibels, Scale::Linear)
        .in_group("Filter"),
    ParamInfo::continuous("shaperMix", "Shaper Dry/Wet", (0.0, 1.0), 1.0, Unit::Percent, Scale::Linear)
        .in_group("Filter"),
];

macro_rules! env_params {
    ($p:literal, $name:literal, $group:literal, $range:expr, $unit:expr, $initial:literal, $peak:literal, $sustain:literal, $end:literal) => {
        [
            ParamInfo::continuous(
                concat!($p, "attack"),
                concat!($name, " Attack"),
                (0.0005, 60.0),
                0.001,
                Unit::Seconds,
                SECONDS,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!($p, "decay"),
                concat!($name, " Decay"),
                (0.0005, 60.0),
                0.6,
                Unit::Seconds,
                SECONDS,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!($p, "release"),
                concat!($name, " Release"),
                (0.0005, 60.0),
                0.05,
                Unit::Seconds,
                SECONDS,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!($p, "initial"),
                concat!($name, " Initial"),
                $range,
                $initial,
                $unit,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::continuous(concat!($p, "peak"), concat!($name, " Peak"), $range, $peak, $unit, Scale::Linear)
                .in_group($group),
            ParamInfo::continuous(
                concat!($p, "sustain"),
                concat!($name, " Sustain"),
                $range,
                $sustain,
                $unit,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::continuous(concat!($p, "end"), concat!($name, " End"), $range, $end, $unit, Scale::Linear)
                .in_group($group),
            ParamInfo::choice(concat!($p, "loop"), concat!($name, " Loop"), LOOPS, 0).in_group($group),
            ParamInfo::continuous(
                concat!($p, "loopTime"),
                concat!($name, " Loop Time"),
                (0.0005, 20.0),
                0.1,
                Unit::Seconds,
                SECONDS,
            )
            .in_group($group),
            ParamInfo::choice(concat!($p, "repeat"), concat!($name, " Repeat"), REPEATS, 4).in_group($group),
            ParamInfo::continuous(
                concat!($p, "timeByVel"),
                concat!($name, " Time by Velocity"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!($p, "timeByKey"),
                concat!($name, " Time by Key"),
                (0.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!($p, "attackSlope"),
                concat!($name, " Attack Slope"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!($p, "decaySlope"),
                concat!($name, " Decay Slope"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!($p, "releaseSlope"),
                concat!($name, " Release Slope"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
        ]
    };
}

const FILTER_ENV_PARAMS: [ParamInfo; ENV_FIELDS] =
    env_params!("filterEnv.", "Filter Env", "Filter", (0.0, 1.0), Unit::Percent, 0.0, 1.0, 0.0, 0.0);
const LFO_ENV_PARAMS: [ParamInfo; ENV_FIELDS] =
    env_params!("lfoEnv.", "LFO Env", "LFO", (0.0, 1.0), Unit::Percent, 1.0, 1.0, 1.0, 1.0);
const PITCH_ENV_PARAMS: [ParamInfo; ENV_FIELDS] =
    env_params!("pitchEnv.", "Pitch Env", "Pitch Env", (-48.0, 48.0), Unit::Semitones, 0.0, 0.0, 0.0, 0.0);

const LFO_PARAMS: [ParamInfo; LFO_FIELDS] = [
    ParamInfo::toggle("lfoOn", "LFO 2 On", false).in_group("LFO"),
    ParamInfo::choice(
        "lfo2Wave",
        "LFO 2 Wave",
        &["Sine", "Square", "Triangle", "Saw Down", "Saw Up", "Sample & Hold", "Noise"],
        0,
    )
    .in_group("LFO"),
    ParamInfo::choice("lfo2Range", "LFO 2 Range", &["Low", "High", "Sync"], 0).in_group("LFO"),
    ParamInfo::continuous("lfo2Rate", "LFO 2 Rate", (0.0, 1.0), 0.5, Unit::Percent, Scale::Linear).in_group("LFO"),
    ParamInfo::choice("lfo2Sync", "LFO 2 Sync", REPEATS, 4).in_group("LFO"),
    ParamInfo::toggle("lfo2Retrigger", "LFO 2 Retrigger", true).in_group("LFO"),
    ParamInfo::continuous("lfo2ByKey", "LFO 2 Rate by Key", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("LFO"),
    ParamInfo::continuous("lfo2ByVel", "LFO 2 Amount by Velocity", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("LFO"),
    ParamInfo::continuous("lfo2Amount", "LFO 2 Amount", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("LFO"),
    ParamInfo::toggle("lfo2ToOp1", "LFO 2 to Op 1", true).in_group("LFO"),
    ParamInfo::toggle("lfo2ToOp2", "LFO 2 to Op 2", true).in_group("LFO"),
    ParamInfo::toggle("lfo2ToOp3", "LFO 2 to Op 3", true).in_group("LFO"),
    ParamInfo::toggle("lfo2ToOp4", "LFO 2 to Op 4", true).in_group("LFO"),
    ParamInfo::toggle("lfo2ToOp5", "LFO 2 to Op 5", true).in_group("LFO"),
    ParamInfo::toggle("lfo2ToOp6", "LFO 2 to Op 6", true).in_group("LFO"),
    ParamInfo::toggle("lfo2ToFilter", "LFO 2 to Filter", false).in_group("LFO"),
    ParamInfo::choice("lfo2DestB", "LFO 2 Destination B", &DESTINATIONS, 0).in_group("LFO"),
    ParamInfo::continuous("lfo2AmountB", "LFO 2 Amount B", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("LFO"),
];

const PITCH_PARAMS: [ParamInfo; PITCH_FIELDS] = [
    ParamInfo::toggle("pitchEnvOn", "Pitch Env On", false).in_group("Pitch Env"),
    ParamInfo::continuous("pitchEnvAmount", "Pitch Env Amount", (-1.0, 1.0), 1.0, Unit::Percent, Scale::Linear)
        .in_group("Pitch Env"),
    ParamInfo::toggle("pitchEnvToOp1", "Pitch Env to Op 1", true).in_group("Pitch Env"),
    ParamInfo::toggle("pitchEnvToOp2", "Pitch Env to Op 2", true).in_group("Pitch Env"),
    ParamInfo::toggle("pitchEnvToOp3", "Pitch Env to Op 3", true).in_group("Pitch Env"),
    ParamInfo::toggle("pitchEnvToOp4", "Pitch Env to Op 4", true).in_group("Pitch Env"),
    ParamInfo::toggle("pitchEnvToOp5", "Pitch Env to Op 5", true).in_group("Pitch Env"),
    ParamInfo::toggle("pitchEnvToOp6", "Pitch Env to Op 6", true).in_group("Pitch Env"),
    ParamInfo::toggle("pitchEnvToLfo", "Pitch Env to LFO 2", false).in_group("Pitch Env"),
    ParamInfo::choice("pitchEnvDestB", "Pitch Env Destination B", &DESTINATIONS, 0).in_group("Pitch Env"),
    ParamInfo::continuous("pitchEnvAmountB", "Pitch Env Amount B", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Pitch Env"),
];

macro_rules! matrix_params {
    ($p:literal, $name:literal) => {
        [
            ParamInfo::choice(concat!("mod.", $p, ".dest1"), concat!($name, " Destination"), &DESTINATIONS, 0)
                .in_group("Modulation"),
            ParamInfo::continuous(
                concat!("mod.", $p, ".amount1"),
                concat!($name, " Amount"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group("Modulation"),
            ParamInfo::choice(concat!("mod.", $p, ".dest2"), concat!($name, " Destination 2"), &DESTINATIONS, 0)
                .in_group("Modulation"),
            ParamInfo::continuous(
                concat!("mod.", $p, ".amount2"),
                concat!($name, " Amount 2"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group("Modulation"),
        ]
    };
}

const MATRIX_PARAMS: [[ParamInfo; 4]; MATRIX_SOURCES] = [
    matrix_params!("velocity", "Velocity"),
    matrix_params!("key", "Key"),
    matrix_params!("pressure", "Pressure"),
    matrix_params!("bend", "Pitch Bend"),
    matrix_params!("wheel", "Mod Wheel"),
    matrix_params!("foot", "Foot"),
    matrix_params!("breath", "Breath"),
    matrix_params!("lfo", "LFO 2"),
    matrix_params!("random", "Random"),
];

macro_rules! op_ext_params {
    ($n:literal, $group:literal) => {
        [
            ParamInfo::choice(concat!("op", $n, ".wave"), concat!("Op ", $n, " Wave"), &WAVES, 0).in_group($group),
            ParamInfo::continuous(
                concat!("op", $n, ".feedback"),
                concat!("Op ", $n, " Feedback"),
                (0.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::continuous(
                concat!("op", $n, ".phase"),
                concat!("Op ", $n, " Phase"),
                (0.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::toggle(concat!("op", $n, ".retrigger"), concat!("Op ", $n, " Retrigger"), false)
                .in_group($group),
            ParamInfo::continuous(
                concat!("op", $n, ".freqByVel"),
                concat!("Op ", $n, " Frequency by Velocity"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::toggle(concat!("op", $n, ".freqQuantize"), concat!("Op ", $n, " Frequency Quantize"), false)
                .in_group($group),
            ParamInfo::continuous(
                concat!("op", $n, ".levelByKey"),
                concat!("Op ", $n, " Level by Key"),
                (-1.0, 1.0),
                0.0,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group($group),
            ParamInfo::choice(
                concat!("op", $n, ".envMode"),
                concat!("Op ", $n, " Envelope Mode"),
                &["Rates and levels", "Time"],
                0,
            )
            .in_group($group),
            ParamInfo::choice(concat!("op", $n, ".partials"), concat!("Op ", $n, " Partials"), &["16", "32", "64"], 0)
                .in_group($group),
            ParamInfo::choice(
                concat!("op", $n, ".repeat"),
                concat!("Op ", $n, " Partial Repeat"),
                &["Off", "1", "2", "3", "4", "5", "6"],
                0,
            )
            .in_group($group),
            ParamInfo::toggle(concat!("op", $n, ".normalize"), concat!("Op ", $n, " Normalize"), true).in_group($group),
            ParamInfo::choice(
                concat!("op", $n, ".oddEven"),
                concat!("Op ", $n, " Partials Used"),
                &["All", "Odd", "Even"],
                0,
            )
            .in_group($group),
        ]
    };
}

const OP_EXT_PARAMS: [[ParamInfo; 12]; 6] = [
    op_ext_params!(1, "Op 1"),
    op_ext_params!(2, "Op 2"),
    op_ext_params!(3, "Op 3"),
    op_ext_params!(4, "Op 4"),
    op_ext_params!(5, "Op 5"),
    op_ext_params!(6, "Op 6"),
];

const OP_ENV_PARAMS: [[ParamInfo; ENV_FIELDS]; 6] = [
    env_params!("op1.env.", "Op 1 Env", "Op 1", (-70.0, 0.0), Unit::Decibels, -70.0, 0.0, 0.0, -70.0),
    env_params!("op2.env.", "Op 2 Env", "Op 2", (-70.0, 0.0), Unit::Decibels, -70.0, 0.0, 0.0, -70.0),
    env_params!("op3.env.", "Op 3 Env", "Op 3", (-70.0, 0.0), Unit::Decibels, -70.0, 0.0, 0.0, -70.0),
    env_params!("op4.env.", "Op 4 Env", "Op 4", (-70.0, 0.0), Unit::Decibels, -70.0, 0.0, 0.0, -70.0),
    env_params!("op5.env.", "Op 5 Env", "Op 5", (-70.0, 0.0), Unit::Decibels, -70.0, 0.0, 0.0, -70.0),
    env_params!("op6.env.", "Op 6 Env", "Op 6", (-70.0, 0.0), Unit::Decibels, -70.0, 0.0, 0.0, -70.0),
];

macro_rules! harmonics {
    ($n:literal, $group:literal; $($h:literal)*) => {
        [$(ParamInfo::continuous(
            concat!("op", $n, ".h", $h),
            concat!("Op ", $n, " Partial ", $h),
            (0.0, 1.0),
            if $h == 1 { 1.0 } else { 0.0 },
            Unit::Percent,
            Scale::Linear,
        ).in_group($group)),*]
    };
}

macro_rules! op_harmonics {
    ($n:literal, $group:literal) => {
        harmonics!($n, $group;
            1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32
            33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64)
    };
}

const DRAWN_PARAMS: [[ParamInfo; HARMONICS]; 6] = [
    op_harmonics!(1, "Op 1 Partials"),
    op_harmonics!(2, "Op 2 Partials"),
    op_harmonics!(3, "Op 3 Partials"),
    op_harmonics!(4, "Op 4 Partials"),
    op_harmonics!(5, "Op 5 Partials"),
    op_harmonics!(6, "Op 6 Partials"),
];

macro_rules! section_params {
    ($p:literal, $name:literal, $amount:literal, $on:literal) => {
        [
            ParamInfo::continuous(
                concat!("random.", $p, ".amount"),
                concat!("Randomize ", $name),
                (0.0, 1.0),
                $amount,
                Unit::Percent,
                Scale::Linear,
            )
            .in_group("Panel"),
            ParamInfo::toggle(concat!("random.", $p, ".on"), concat!("Randomize ", $name, " On"), $on)
                .in_group("Panel"),
            ParamInfo::toggle(concat!("random.", $p, ".lock"), concat!("Lock ", $name), false).in_group("Panel"),
        ]
    };
}

const PANEL_HEAD: [ParamInfo; 5] = [
    ParamInfo::integer("panel.op", "Selected Operator", (1, 6), 1, Unit::None).in_group("Panel"),
    ParamInfo::choice("panel.page", "Page", &["Env", "Wave", "Filter", "LFO", "Pitch", "Mod"], 0).in_group("Panel"),
    ParamInfo::choice(
        "panel.expandedPage",
        "Expanded Page",
        &["Operator", "Filter", "LFO", "Pitch", "Matrix", "Algorithm", "Random"],
        0,
    )
    .in_group("Panel"),
    ParamInfo::continuous("random.amount", "Randomize Amount", (0.0, 1.0), 0.35, Unit::Percent, Scale::Linear)
        .in_group("Panel"),
    ParamInfo::integer("random.seed", "Randomize Seed", (0, 99_999), 1, Unit::None).in_group("Panel"),
];

const SECTION_PARAMS: [[ParamInfo; 3]; SECTIONS.len()] = [
    section_params!("envelopes", "Envelopes", 0.6, true),
    section_params!("levels", "Levels", 0.4, true),
    section_params!("waves", "Waves", 0.3, true),
    section_params!("filter", "Filter", 0.45, true),
    section_params!("lfo", "LFO", 0.5, true),
    section_params!("pitchEnv", "Pitch Envelope", 0.2, false),
    section_params!("modulation", "Modulation", 0.35, true),
    section_params!("stereo", "Stereo", 0.25, true),
    section_params!("ratios", "Ratios", 0.15, false),
    section_params!("algorithm", "Algorithm", 1.0, false),
];

const fn build() -> [ParamInfo; COUNT] {
    let mut out = [VOLUME_PARAM; COUNT];
    let mut i = 0;
    while i < 6 {
        let mut j = 0;
        while j < OP_PARAMS {
            out[i * OP_PARAMS + j] = OPS[i][j].in_group(OP_GROUPS[i]);
            j += 1;
        }
        i += 1;
    }
    let mut g = 0;
    while g < GLOBAL_COUNT {
        out[GLOBAL + g] = GLOBALS[g].in_group(GLOBAL_GROUPS[g]);
        g += 1;
    }
    let mut p = 0;
    while p < PERFORMANCE_PARAMS.len() {
        out[PERFORMANCE + p] = PERFORMANCE_PARAMS[p].in_group("Performance");
        p += 1;
    }
    let mut s = 0;
    while s < 4 {
        let mut r = 0;
        while r < 4 {
            out[ROUTES + s * 4 + r] = ROUTE_PARAMS[s][r].in_group("Modulation");
            r += 1;
        }
        s += 1;
    }
    let mut at = EXTENDED;
    let mut i = 0;
    while i < GLOBAL_EXT {
        out[at] = GLOBAL_EXT_PARAMS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < FILTER_FIELDS {
        out[at] = FILTER_PARAMS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < ENV_FIELDS {
        out[at] = FILTER_ENV_PARAMS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < LFO_FIELDS {
        out[at] = LFO_PARAMS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < ENV_FIELDS {
        out[at] = LFO_ENV_PARAMS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < PITCH_FIELDS {
        out[at] = PITCH_PARAMS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < ENV_FIELDS {
        out[at] = PITCH_ENV_PARAMS[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < MATRIX_SOURCES * 4 {
        out[at] = MATRIX_PARAMS[i / 4][i % 4];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < 6 * OP_EXT_FIELDS {
        let (operator, field) = (i / OP_EXT_FIELDS, i % OP_EXT_FIELDS);
        out[at] =
            if field < opx::ENV { OP_EXT_PARAMS[operator][field] } else { OP_ENV_PARAMS[operator][field - opx::ENV] };
        at += 1;
        i += 1;
    }
    i = 0;
    while i < 6 * HARMONICS {
        out[at] = DRAWN_PARAMS[i / HARMONICS][i % HARMONICS];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < 5 {
        out[at] = PANEL_HEAD[i];
        at += 1;
        i += 1;
    }
    i = 0;
    while i < 3 * SECTIONS.len() {
        out[at] = SECTION_PARAMS[i / 3][i % 3];
        at += 1;
        i += 1;
    }
    out
}

/// Every parameter, in index order, each in its group.
pub static PARAMS: [ParamInfo; COUNT] = build();
