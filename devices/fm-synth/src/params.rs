// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The FM synth's parameters: 22 per operator (operators 1 to 6), the voice's global ones in
//! the order a voice stores them, then performance settings (bend, portamento, what the
//! wheels and pedals do, volume).

use suhina_device_api::{ParamInfo, ParamSpec, Scale, Unit};

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
pub const COUNT: usize = VOLUME + 1;

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
            ParamSpec::integer(concat!("op", $n, ".rate1"), concat!("Op ", $n, " Rate 1"), (0, 99), 99, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".rate2"), concat!("Op ", $n, " Rate 2"), (0, 99), 99, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".rate3"), concat!("Op ", $n, " Rate 3"), (0, 99), 99, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".rate4"), concat!("Op ", $n, " Rate 4"), (0, 99), 99, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".level1"), concat!("Op ", $n, " Level 1"), (0, 99), 99, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".level2"), concat!("Op ", $n, " Level 2"), (0, 99), 99, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".level3"), concat!("Op ", $n, " Level 3"), (0, 99), 99, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".level4"), concat!("Op ", $n, " Level 4"), (0, 99), 0, Unit::None),
            ParamSpec::integer(
                concat!("op", $n, ".breakPoint"),
                concat!("Op ", $n, " Break Point"),
                (0, 99),
                39,
                Unit::None,
            ),
            ParamSpec::integer(
                concat!("op", $n, ".leftDepth"),
                concat!("Op ", $n, " Left Depth"),
                (0, 99),
                0,
                Unit::None,
            ),
            ParamSpec::integer(
                concat!("op", $n, ".rightDepth"),
                concat!("Op ", $n, " Right Depth"),
                (0, 99),
                0,
                Unit::None,
            ),
            ParamSpec::choice(concat!("op", $n, ".leftCurve"), concat!("Op ", $n, " Left Curve"), CURVES, 0),
            ParamSpec::choice(concat!("op", $n, ".rightCurve"), concat!("Op ", $n, " Right Curve"), CURVES, 0),
            ParamSpec::integer(
                concat!("op", $n, ".rateScaling"),
                concat!("Op ", $n, " Rate Scaling"),
                (0, 7),
                0,
                Unit::None,
            ),
            ParamSpec::integer(
                concat!("op", $n, ".ampModSens"),
                concat!("Op ", $n, " Amp Mod Sensitivity"),
                (0, 3),
                0,
                Unit::None,
            ),
            ParamSpec::integer(
                concat!("op", $n, ".velocitySens"),
                concat!("Op ", $n, " Velocity Sensitivity"),
                (0, 7),
                0,
                Unit::None,
            ),
            ParamSpec::integer(
                concat!("op", $n, ".level"),
                concat!("Op ", $n, " Output Level"),
                (0, 99),
                $level,
                Unit::None,
            ),
            ParamSpec::choice(concat!("op", $n, ".mode"), concat!("Op ", $n, " Mode"), MODES, 0),
            ParamSpec::integer(concat!("op", $n, ".coarse"), concat!("Op ", $n, " Coarse"), (0, 31), 1, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".fine"), concat!("Op ", $n, " Fine"), (0, 99), 0, Unit::None),
            ParamSpec::integer(concat!("op", $n, ".detune"), concat!("Op ", $n, " Detune"), (-7, 7), 0, Unit::None),
            ParamSpec::toggle(concat!("op", $n, ".on"), concat!("Op ", $n, " On"), true),
        ]
    };
}

const OPS: [[ParamSpec; OP_PARAMS]; 6] =
    [op_params!(1, 99), op_params!(2, 0), op_params!(3, 0), op_params!(4, 0), op_params!(5, 0), op_params!(6, 0)];

const GLOBALS: [ParamSpec; GLOBAL_COUNT] = [
    ParamSpec::integer("pitchRate1", "Pitch Rate 1", (0, 99), 99, Unit::None),
    ParamSpec::integer("pitchRate2", "Pitch Rate 2", (0, 99), 99, Unit::None),
    ParamSpec::integer("pitchRate3", "Pitch Rate 3", (0, 99), 99, Unit::None),
    ParamSpec::integer("pitchRate4", "Pitch Rate 4", (0, 99), 99, Unit::None),
    ParamSpec::integer("pitchLevel1", "Pitch Level 1", (0, 99), 50, Unit::None),
    ParamSpec::integer("pitchLevel2", "Pitch Level 2", (0, 99), 50, Unit::None),
    ParamSpec::integer("pitchLevel3", "Pitch Level 3", (0, 99), 50, Unit::None),
    ParamSpec::integer("pitchLevel4", "Pitch Level 4", (0, 99), 50, Unit::None),
    ParamSpec::integer("algorithm", "Algorithm", (1, 32), 1, Unit::None),
    ParamSpec::integer("feedback", "Feedback", (0, 7), 0, Unit::None),
    ParamSpec::toggle("oscKeySync", "Oscillator Key Sync", true),
    ParamSpec::integer("lfoSpeed", "LFO Speed", (0, 99), 35, Unit::None),
    ParamSpec::integer("lfoDelay", "LFO Delay", (0, 99), 0, Unit::None),
    ParamSpec::integer("lfoPitchDepth", "LFO Pitch Depth", (0, 99), 0, Unit::None),
    ParamSpec::integer("lfoAmpDepth", "LFO Amp Depth", (0, 99), 0, Unit::None),
    ParamSpec::toggle("lfoKeySync", "LFO Key Sync", true),
    ParamSpec::choice("lfoWave", "LFO Wave", WAVES, 0),
    ParamSpec::integer("pitchModSens", "Pitch Mod Sensitivity", (0, 7), 3, Unit::None),
    ParamSpec::integer("transpose", "Transpose", (-24, 24), 0, Unit::None),
];

macro_rules! route_params {
    ($id:literal, $name:literal, $range:literal, $pitch:literal) => {
        [
            ParamSpec::integer(concat!($id, "Range"), concat!($name, " Range"), (0, 99), $range, Unit::None),
            ParamSpec::toggle(concat!($id, "Pitch"), concat!($name, " to Pitch"), $pitch),
            ParamSpec::toggle(concat!($id, "Amp"), concat!($name, " to Amp"), false),
            ParamSpec::toggle(concat!($id, "EgBias"), concat!($name, " to EG Bias"), false),
        ]
    };
}

const PERFORMANCE_PARAMS: [ParamSpec; 5] = [
    ParamSpec::integer("pitchBendRange", "Pitch Bend Range", (0, 12), 2, Unit::None),
    ParamSpec::integer("pitchBendStep", "Pitch Bend Step", (0, 12), 0, Unit::None),
    ParamSpec::toggle("portamento", "Portamento", false),
    ParamSpec::toggle("glissando", "Glissando", false),
    ParamSpec::integer("portamentoTime", "Portamento Time", (0, 99), 0, Unit::None),
];

const ROUTE_PARAMS: [[ParamSpec; 4]; 4] = [
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

const VOLUME_PARAM: ParamSpec =
    ParamSpec::continuous("volume", "Volume", (-70.0, 6.0), 0.0, Unit::Decibels, Scale::Linear);

const fn build() -> [ParamSpec; COUNT] {
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
    out
}

/// Every parameter, in index order, each in its group.
pub static PARAMS: [ParamInfo; COUNT] = suhina_device_api::params(&build());
