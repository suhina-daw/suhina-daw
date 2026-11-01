// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The late reverb, as measured (docs/spec/reverb.md › Late reverb): lines mixed by one orthogonal
//! matrix, each fed by its own mix of the direct reflections, and each line's family of
//! reflections heard again, low-passed, one offset later, joining that line's output in the mix.
//! The direct reflections come in groups that reach the lines as one, and there are as many lines
//! as groups: Sparse 4, Low 6, Mid 8, High 10 ([`design`]).
//!
//! ```text
//! direct taps -> groups -> each line's input ------------------------> own line
//! family lines (from the reflections, an entry later) -> low-pass ---+
//! line outputs ------------------------------------------------------+-> MIX -> lines
//! ```
//!
//! At Low a line's family is its input. A family's copy is its filtered reflections, an entry
//! later (the reverb's family lines hold both). The tables' times are frames at 44.1 kHz; at
//! another rate each is converted first and rounded after, in the session's frames (read at 44.1,
//! 48 and 96 kHz, `reverb-rate-48k`, `reverb-rate-96k`). A line is a delay and an allpass (two in
//! series at Mid and High), each a whole number of 8-frame blocks and one frame with Smooth None,
//! whole frames under Fast ([`length`]). The input reaches the lines [`entry`]
//! after each direct reflection, whatever the Size. Every value was read from the reference's impulse
//! responses at Diffusion 0.1 % and Decay 60 s (`reverb-network-exact`, `reverb-network-density`
//! and the runs named below), where each arrival is one sample once the input's fixed filters are
//! divided out.

use suhina_dsp::{Allpass, Biquad, Coefficients, DelayLine};

use crate::{Damper, Damping, Line};

/// Lines in Low's loop: A, B, C, D, F and R.
const LOOP: usize = 6;
/// Groups of Low's direct reflections the families are made of.
const GROUPS: usize = 6;
/// The most lines a network has (High's).
pub(crate) const MAX_LINES: usize = 10;
/// The most groups of direct reflections a network takes (High's).
pub(crate) const MAX_GROUPS: usize = 10;
/// The most direct reflections a Density has (Low, Mid and High; Sparse has four).
pub(crate) const DIRECT_TAPS: usize = 10;
/// The families of filtered reflections every Density draws on: Low's six (A, B, C, D, F and R's)
/// and the four only Mid and High have (E, H, J and K), in table order ([`families`]).
pub(crate) const FAMILY_COUNT: usize = 10;
/// Each family's index in the tables, by name. Low's six take the names of its lines; the other
/// four fill the alphabet after them, leaving out G (a part of R's line, [`LOW_LINES`]) and I. At
/// Size 100 under Smooth None their filtered reflections peak 401 (A), 3193 (B), 1641 (C), 1337
/// (D), 2985 (F), 2337 (R), 961 (E), 2073 (H), 2393 (J) and 2497 (K) frames after their direct
/// ones ([`peak`]).
pub(crate) mod families {
    pub(crate) const A: usize = 0;
    pub(crate) const B: usize = 1;
    pub(crate) const C: usize = 2;
    pub(crate) const D: usize = 3;
    pub(crate) const F: usize = 4;
    pub(crate) const R: usize = 5;
    pub(crate) const E: usize = 6;
    pub(crate) const H: usize = 7;
    pub(crate) const J: usize = 8;
    pub(crate) const K: usize = 9;
}
use families::{A, B, C, D, E, F, H, J, K, R};
/// The side each family's reflections and line are heard on (0 left, 1 right), at every Density.
pub(crate) const FAMILY_SIDES: [usize; FAMILY_COUNT] = [1, 0, 1, 0, 0, 1, 0, 1, 0, 1];
/// The frame rate the tables' lengths are at.
pub(crate) const REFERENCE_RATE: f64 = 44_100.0;

/// Under Smooth None the reference builds every delay of the reverb from blocks of 8 frames, at the
/// session's rate (the same 8 at 44.1, 48 and 96 kHz).
const BLOCK: f64 = 8.0;
/// ... rounding a length up to its next block unless it is less than this much of a block past
/// one ([`rounded`]): with 0.88 to 0.9 every length read fits, lines, allpasses, families, the
/// Shape entry and Stereo's delay.
const BLOCK_EDGE: f64 = 0.9;

/// A delay of `x` frames (at the session's rate) as the reference rounds it: under Smooth Fast (and Slow)
/// down to a whole frame, under None to whole blocks, up but for the last tenth of a block. While
/// things move, the straight line through the middle of the steps, so a glide does not jump a step at a
/// time.
pub(crate) fn rounded(x: f64, fast: bool, settled: bool) -> f64 {
    match (fast, settled) {
        (true, true) => x.floor(),
        (true, false) => x - 0.5,
        (false, true) => BLOCK * (x / BLOCK + BLOCK_EDGE).floor(),
        (false, false) => x + BLOCK * (BLOCK_EDGE - 0.5),
    }
}

/// Low's lines, A, B, C, D, F and R: each delay in blocks of the reference room, and its allpass
/// in blocks per % of Scale ([`length`]). C's delay takes in
/// its family's offset, and R's A's delay and a delay G, each as one rounded length: at Size 0.222
/// rounding the parts on their own makes C a block longer and R a block shorter
/// (`reverb-network-small`). Each is the middle of what both Smooths' reads allow, at Size 0.222 to
/// 500 (R's also with its 1854 blocks at Size 350, `reverb-network-size-level`).
const LOW_LINES: [[f64; 3]; LOOP] = [
    [287.0993, 0.63476, 0.0],
    [340.0779, 1.22412, 0.0],
    [628.7004, 0.87055, 0.0],
    [459.9281, 3.97956, 0.0],
    [846.6372, 2.18481, 0.0],
    [1220.5325, 0.80455, 0.0],
];

/// Each family's line in Sparse, Mid and High: its delay (blocks), and its two allpasses' blocks
/// per Scale %, the shorter first ([`length`]). Each is the middle of what every line's first pass
/// allows at Size 0.222, 100, 300, 400 and 500 with Smooth None and Fast and at Scale 0 to 100 %
/// at Size 100 (`reverb-network-density`, `reverb-network-density-size`,
/// `reverb-network-size-level`). How a line's length splits between its two allpasses shows only
/// above Diffusion 0.1 % (`reverb-network-density-decay` at 60 %, `reverb-network-density-split`
/// at 30 and 60 % and Size 10.54). Each line's whole first pass at 48 and 96 kHz, Smooth None and
/// Fast, Scale 40 % (`reverb-rate-48k`, `reverb-rate-96k`, read line by line) narrowed seven of
/// them to the middle of what those reads also allow: A's, B's, C's and H's longer allpass, D's
/// delay and shorter allpass, and J's shorter one. E's line under Fast is 1 frame shorter than this
/// at 48 kHz and 2 at 96 kHz, which no length the 44.1 kHz reads allow gives.
const SHARED_LINES: [[f64; 3]; FAMILY_COUNT] = [
    [1184.4905, 0.6886, 12.7522],
    [442.6705, 1.1039, 3.7120],
    [436.192, 1.3804, 5.95122],
    [1026.830, 1.7352, 3.4461],
    [356.0745, 2.4623, 5.7064],
    [560.7041, 0.7406, 8.3358],
    [621.4635, 2.9617, 3.7544],
    [1983.288, 0.7584, 2.6934],
    [379.8665, 0.8040, 5.6726],
    [501.2132, 1.1817, 3.8076],
];

/// Where each family's filtered reflection starts after the direct one (blocks, reference room,
/// [`rounded`]): to its peak at Size 100 the frames [`families`] gives under Smooth None, 0 to 6
/// frames sooner under Fast. Its copy into the mix is heard there
/// too, through the same low-pass. Each is the middle of what the reads at Size 0.222 to 500 allow,
/// None and Fast (`reverb-network-small`, `reverb-network-density`, `reverb-network-density-size`,
/// `reverb-network-fast-train`, `reverb-network-size-level`); F's also with its filtered
/// reflections under Fast at 48 and 96 kHz, a frame later than 372.7764 gave
/// (`reverb-rate-48k`, `reverb-rate-96k`).
const FAMILY_LENGTHS: [f64; FAMILY_COUNT] =
    [49.3955, 398.8085, 204.9654, 166.9645, 372.7928, 292.0502, 120.0675, 258.9858, 299.0174, 311.7368];

/// With Smooth None, the lines of families A and B run this much of a frame past their whole
/// length, read by linear interpolation: a slight loss of the highs on every pass (0.18 dB at the
/// top), and the network's only loss besides the decay. Under Fast their delays keep their whole
/// fraction instead ([`length`]). Without it the tails at Size 0.222 keep up to 5 dB too much above
/// 12 kHz by 3 s, and no other hundredth fits as well (`reverb-network-density-size`). It is a
/// hundredth of a frame at 48 and 96 kHz too (`reverb-rate-48k`, `reverb-rate-96k`).
const NONE_FRACTION: f64 = 0.01;

/// A line's delay and allpasses (frames at `sample_rate`) from `table`: the delay of row `delay`
/// and the allpasses of row `family` (the second only when `second`), for size factor `room` and
/// Scale `percent`. Each part is its blocks times `room` (an allpass's also times Scale), converted
/// to `sample_rate`, [`rounded`], but under Smooth Fast the delay of a line of family A or B keeps
/// its fraction ([`reads_between_frames`]); and a frame.
#[allow(clippy::too_many_arguments)]
fn length(
    table: &[[f64; 3]],
    delay: usize,
    family: usize,
    second: bool,
    room: f64,
    percent: f64,
    settled: bool,
    fast: bool,
    sample_rate: f64,
) -> (f64, f64, f64) {
    let [blocks_delay, _, _] = table[delay];
    let [_, shorter, longer] = table[family];
    let per_frame = sample_rate / REFERENCE_RATE;
    let part = |blocks: f64| rounded(BLOCK * blocks * per_frame, fast, settled);
    let line = if fast && reads_between_frames(family) {
        BLOCK * blocks_delay * room * per_frame
    } else {
        part(blocks_delay * room)
    };
    let first = part(shorter * percent * room) + 1.0;
    let second = if second { part(longer * percent * room) + 1.0 } else { 0.0 };
    (line + 1.0, first, second)
}

/// Whether `family`'s line runs between frames: A's and B's keep their fraction under Smooth Fast
/// and run [`NONE_FRACTION`] past their length under None; every other line is whole frames.
pub(crate) fn reads_between_frames(family: usize) -> bool {
    matches!(family, A | B)
}

/// Where `family`'s filtered reflections peak after their direct ones (frames at 44.1 kHz) for
/// size factor `room`, under Smooth Fast (or Slow) when `fast`: [`front`] and a frame.
pub(crate) fn peak(family: usize, room: f64, settled: bool, fast: bool) -> f64 {
    front(family, room, settled, fast, REFERENCE_RATE) + 1.0
}

/// Where `family`'s copy starts (frames at `sample_rate`) for size factor `room`, at every Density.
pub(crate) fn front(family: usize, room: f64, settled: bool, fast: bool, sample_rate: f64) -> f64 {
    rounded(BLOCK * FAMILY_LENGTHS[family] * room * sample_rate / REFERENCE_RATE, fast, settled)
}

/// Which group each direct reflection of Density Low belongs to, in table order.
const TAP_GROUPS: [usize; DIRECT_TAPS] = [0, 0, 1, 2, 2, 4, 5, 1, 3, 3];
/// Each line's family: its weight on each group, the level of the family's filtered reflection
/// against the direct reflection it follows (the same for both reflections of a group, to 1 %).
const FAMILIES: [[f64; GROUPS]; LOOP] = [
    [1.044, 0.625, -0.0915, -0.604, -0.385, 0.237],
    [-0.1175, 0.5865, 0.7715, -0.399, 0.663, -0.686],
    [0.1255, 0.5465, -0.8845, 0.382, 0.880, 0.091],
    [-0.624, 0.755, 0.3725, 0.1175, -0.148, 0.955],
    [-0.692, 0.419, -0.684, -0.506, -0.590, -0.546],
    [0.241, 0.5005, 0.198, 1.050, -0.555, -0.533],
];
/// How much of its family each line takes, times its output level: the first pass of each line
/// against one direct reflection.
const FAMILY_GAINS: [f64; LOOP] = [1.7860, 1.7959, 1.8109, 1.8022, 1.8092, 1.8012];
/// The loop's mixing: row `i` is what line `i` takes from each line's output (and from each
/// family's copy, at the same weight). Read from the renders, it is orthogonal to 0.3 % without
/// being made so; [`orthonormal`] makes it exactly so.
const MIX: [[f64; LOOP]; LOOP] = [
    [0.1651, 0.43434, 0.7290, -0.2671, -0.0618, -0.4213],
    [-0.4845, 0.4152, -0.0838, 0.4661, 0.5380, -0.2813],
    [0.0587, 0.3811, 0.0875, 0.6188, -0.6243, 0.2665],
    [0.6681, 0.5295, -0.4350, -0.1034, 0.2579, 0.0826],
    [-0.3852, 0.2966, -0.4863, -0.4104, -0.4819, -0.3556],
    [-0.3740, 0.3540, 0.1679, -0.3865, 0.1348, 0.7342],
];
/// How much later the input reaches the lines per unit of Shape (frames at 44.1 kHz; 35 ms),
/// whatever the Size and the Density ([`entry`]).
const SHAPE_ENTRY: f64 = 1543.5;

/// The input's delay to the lines after each direct reflection (frames at `sample_rate`) at
/// `shape`: [`SHAPE_ENTRY`] times Shape at `sample_rate`, [`rounded`], less a frame (at 48 kHz 424,
/// 840 and 1680 frames later at Shape 0.25, 0.5 and 1 than at 0 under None, 420, 840 and 1680
/// under Fast; at 96 kHz 840, 1680 and 3360 under both: `reverb-rate-48k`,
/// `reverb-rate-96k`). With None −1, 391, 775, 1159 and 1543 frames at Shape 0, 0.25, 0.5, 0.75
/// and 1 (`reverb-network-exact`, `reverb-network-id`, `reverb-size` shape-*-tail), under Fast −1,
/// 384, 770, 1156 and 1542 (`reverb-structure` shape-*). Read at Size 100 and inferred at the
/// others: Shape 0.5's entry is 775 frames at every Size (`reverb-network-small`).
pub(crate) fn entry(shape: f64, settled: bool, fast: bool, sample_rate: f64) -> f64 {
    rounded(SHAPE_ENTRY * shape * sample_rate / REFERENCE_RATE, fast, settled) - 1.0
}
/// Longest delay a line needs (seconds, with room for Size 500): High's longest line.
const LINE_SECONDS: f64 = 0.65;
/// Longest allpass a line needs (seconds, with room for Size 500): Mid's and High's longest
/// line's Scale part.
const ALLPASS_SECONDS: f64 = 0.45;

/// The family each of Sparse's lines takes its delay from: its own, but the last's (C), which has
/// A's (`reverb-network-density-size`).
const SPARSE_DELAYS: [usize; 4] = [F, B, R, A];
/// Each of Sparse's lines' input, as [`MID_INPUTS`] (each weight is against a direct reflection of
/// the first one's level, whose sign it does not take).
const SPARSE_INPUTS: [[f64; 4]; 4] = [
    [-1.6120, -0.4120, -0.8280, 1.6400],
    [0.5990, 2.1400, -0.1890, 1.0230],
    [1.6190, -0.8600, -1.5630, 0.5690],
    [0.7570, -0.7360, 1.6880, 1.3990],
];
/// Sparse's inputs times this, fitted to the reference's renders at Scale 5, 40 and 100 %.
const SPARSE_INPUT_GAIN: f64 = 0.9965;
/// Sparse's lines' families.
const SPARSE_FAMILIES: [usize; 4] = [F, B, R, C];
/// Sparse's mixing, as [`MID_MIX`], run with [`SPARSE_LEVELS`].
const SPARSE_MIX: [[f64; 4]; 4] = [
    [-0.166755, -0.334405, 0.663378, -0.648302],
    [0.871775, -0.077310, 0.418061, 0.243425],
    [-0.348542, -0.633013, 0.230244, 0.651767],
    [-0.301196, 0.693896, 0.576318, 0.309270],
];
/// Each of Sparse's lines' output level against the others', as [`MID_LEVELS`].
const SPARSE_LEVELS: [f64; 4] = [1.0032, 0.9986, 1.0023, 0.9960];

/// Mid's and High's inputs times this, fitted to the reference's renders as Sparse's.
const MID_HIGH_INPUT_GAIN: f64 = 1.057;

/// Mid's direct reflections reach the lines in 8 groups: the first and the fourth reflection as
/// one, with weights equal and opposite to their levels, and so the second and the seventh.
const MID_TAP_GROUPS: [usize; DIRECT_TAPS] = [0, 1, 2, 0, 3, 4, 1, 5, 6, 7];
/// Each of Mid's lines' input: its weight on each group times its output level, the first pass
/// against one direct reflection of the group.
const MID_INPUTS: [[f64; 8]; 8] = [
    [0.0752, -0.6203, -1.4157, 0.2637, -1.3792, 0.8928, 0.2110, 0.8309],
    [0.3001, 1.5956, 0.1161, -0.1601, -1.1467, 0.6674, 0.1731, -1.2484],
    [1.2471, -0.5603, 1.0326, 0.9641, -0.1397, 0.8984, -1.1400, 0.0150],
    [0.2102, -0.4120, -0.1727, -2.0335, 0.3170, 0.8667, -0.8004, -0.1686],
    [-0.6229, 0.3253, 1.4170, -0.3365, -0.1671, 0.9650, 0.9465, 1.2416],
    [0.9590, 0.5226, -0.7926, 0.3384, 1.5033, 0.9734, 0.9348, 0.0527],
    [-1.4164, 0.8425, -0.4587, 0.6245, 0.5737, 0.7272, -1.3265, 0.2895],
    [-0.9832, -1.1993, 0.1881, 0.3803, 0.1573, 0.8441, 0.6337, -1.4033],
];
/// Mid's lines' families.
const MID_FAMILIES: [usize; 8] = [B, F, E, D, K, C, R, A];
/// Mid's mixing, as [`MIX`]: first read from the two-line paths, orthogonal to 1 %, then fitted as
/// a rotation of that (`M · exp(A)`, `A` antisymmetric, by Gauss-Newton) to the reference's whole tails at
/// Size 0.222 and 500 (`reverb-network-density-size`), moving no entry more than 0.004 (Sparse's
/// 0.001, High's 0.007).
#[allow(clippy::approx_constant, reason = "a fitted value that is near Euler's constant by chance")]
const MID_MIX: [[f64; 8]; 8] = [
    [0.368161, 0.108775, -0.581296, -0.568820, 0.086265, 0.031482, 0.343733, -0.254124],
    [0.271656, -0.064204, 0.048059, -0.466431, 0.070065, 0.122714, -0.509307, 0.650271],
    [0.365622, 0.394272, 0.422556, -0.057502, -0.466574, 0.509036, 0.004660, -0.228428],
    [0.357116, -0.837262, -0.069221, 0.129876, -0.330419, 0.086300, -0.069221, -0.168481],
    [0.395007, -0.137308, 0.576006, -0.067730, 0.387312, -0.252981, 0.506916, 0.133311],
    [0.398688, 0.136657, -0.322884, 0.612085, 0.381779, 0.390011, 0.022096, 0.212413],
    [0.296946, 0.254839, -0.185537, 0.234442, -0.543292, -0.575630, 0.117608, 0.342265],
    [0.355373, 0.158482, 0.077321, 0.067185, 0.264291, -0.412222, -0.588521, -0.501964],
];
/// Each of Mid's lines' output level against the others'. The reference's lines are not all heard at one
/// level, and [`MID_INPUTS`] were read with these in them, so the loop runs the
/// mixing as `level_i · mix_ij / level_j` ([`loop_mix`]). A free fit of each mix to every trip at
/// Size 0.222 (`reverb-network-density-size`) comes out as this mixing between lines at these levels
/// to 0.0002; they hold at Size 100 and 500 too.
const MID_LEVELS: [f64; 8] = [0.9992, 1.0013, 1.0026, 0.9997, 1.0009, 1.0017, 1.0028, 0.9919];

/// Each of High's lines' input, as [`MID_INPUTS`]; its direct reflections each reach the lines on
/// their own.
const HIGH_INPUTS: [[f64; 10]; 10] = [
    [0.3489, 1.1670, -0.1361, 1.0533, -0.4631, 0.0100, 1.3012, -0.8455, -0.7958, 0.2516],
    [-0.5246, -0.4824, 1.1040, -0.8033, -0.6152, -1.2950, 1.1572, 0.2154, 0.0009, 0.4947],
    [1.3323, -0.6285, -1.1228, -0.4324, 0.3345, -0.6819, 0.9199, 0.0170, 0.2692, -0.9601],
    [-0.7756, 0.5521, -0.3357, -0.0285, -1.1936, -0.3286, -0.2226, -0.7851, 1.3005, -1.0464],
    [-0.0633, -0.2775, -0.3145, -1.3490, -1.0629, 1.0638, 0.0569, -0.3118, -1.1971, -0.3588],
    [-0.1499, -1.1368, 0.4698, 0.6943, 0.1877, -0.7194, -0.6787, -1.3025, -0.9237, -0.6666],
    [-1.0738, -0.2636, -1.2635, -0.5013, 0.8118, -0.0878, 0.3134, -1.0224, 0.1623, 1.0732],
    [-0.2554, -0.7696, -1.1345, 0.9573, -1.2317, -0.4484, -0.2701, 0.9381, -0.3424, 0.5893],
    [0.7784, 1.0137, -0.2645, -0.7414, -0.2340, -1.1961, -1.1568, -0.2559, -0.4088, 0.6124],
    [-1.1453, 0.7659, -0.3891, -0.0994, 0.6801, -0.5935, 0.0942, 0.8846, -0.9501, -1.0664],
];
/// High's lines' families: Mid's, with J and H.
const HIGH_FAMILIES: [usize; 10] = [B, J, F, E, D, K, C, R, A, H];
/// High's mixing, as [`MID_MIX`], run with [`HIGH_LEVELS`].
const HIGH_MIX: [[f64; 10]; 10] = [
    [-0.348113, 0.431273, -0.190544, -0.054873, 0.003126, -0.328649, 0.142605, 0.102578, 0.534750, 0.478192],
    [0.085721, -0.327071, -0.250251, 0.447068, -0.525203, 0.000062, -0.212144, 0.201146, 0.472698, -0.196046],
    [0.005687, -0.173832, 0.137437, -0.457475, -0.280688, 0.109642, 0.539417, -0.392038, 0.375058, -0.255809],
    [-0.319107, -0.011948, -0.485638, -0.138487, -0.133577, 0.531357, -0.315474, -0.430384, -0.090890, 0.223175],
    [-0.129695, -0.552898, -0.434898, -0.127689, 0.437665, -0.494356, -0.024158, -0.148806, 0.023321, -0.113284],
    [-0.531302, 0.283413, 0.077284, 0.191711, -0.292916, -0.376628, -0.061197, -0.272901, -0.275947, -0.461145],
    [-0.416906, -0.203191, 0.330902, -0.512201, -0.037540, 0.065516, -0.435820, 0.435421, 0.127347, -0.107735],
    [0.385716, 0.390266, -0.501488, -0.462877, -0.182072, -0.138763, -0.106304, 0.241407, -0.110776, -0.314508],
    [-0.107498, -0.309132, -0.097002, -0.110037, -0.501957, -0.170334, 0.331383, 0.257650, -0.484688, 0.423402],
    [0.369272, -0.038617, 0.284305, -0.163968, -0.252294, -0.398535, -0.478311, -0.446345, 0.039609, 0.319961],
];
/// Each of High's lines' output level against the others', as [`MID_LEVELS`].
const HIGH_LEVELS: [f64; 10] = [0.9988, 1.0030, 1.0019, 1.0025, 1.0007, 1.0022, 1.0044, 1.0013, 0.9930, 0.9922];

/// One Density's network.
#[derive(Debug, PartialEq)]
pub(crate) struct Design {
    /// Lines in the loop.
    pub lines: usize,
    /// The group each direct reflection feeds, in table order.
    pub tap_groups: [usize; DIRECT_TAPS],
    /// Each line's input: its weight on each group ...
    inputs: [[f64; MAX_GROUPS]; MAX_LINES],
    /// ... times this.
    input_gains: [f64; MAX_LINES],
    /// Each line's family: its copy's offset, its side, and whether it is an A or B line.
    families: [usize; MAX_LINES],
    /// The lengths, by family ([`length`]).
    lengths: &'static [[f64; 3]],
    /// The family each line takes its delay from, when not its own.
    delays: Option<&'static [usize]>,
    /// Whether each line runs a second allpass.
    second: bool,
    /// The mixing as read; [`orthonormal`] makes it exactly orthogonal.
    mix: [[f64; MAX_LINES]; MAX_LINES],
    /// Each line's output level against the others' ([`loop_mix`]).
    levels: [f64; MAX_LINES],
    /// How many times as much of the damping shelves' loss a line takes per second as Low's law
    /// gives ([`Lengths::trips`]).
    trip_scale: f64,
}

impl Design {
    /// The side line `k` is heard on: its family's.
    pub fn side(&self, k: usize) -> usize {
        FAMILY_SIDES[self.families[k]]
    }
}

/// `items` padded with `fill` to `M` values.
const fn pad<T: Copy, const N: usize, const M: usize>(items: [T; N], fill: T) -> [T; M] {
    let mut out = [fill; M];
    let mut i = 0;
    while i < N {
        out[i] = items[i];
        i += 1;
    }
    out
}

/// `rows` padded with zeros to `M` rows of `M` values.
const fn pad_rows<const N: usize, const C: usize, const M: usize>(rows: [[f64; C]; N]) -> [[f64; M]; M] {
    let mut out = [[0.0; M]; M];
    let mut i = 0;
    while i < N {
        out[i] = pad(rows[i], 0.0);
        i += 1;
    }
    out
}

/// Low's network: each line's input is its family.
static LOW: Design = Design {
    lines: LOOP,
    tap_groups: TAP_GROUPS,
    inputs: pad_rows(FAMILIES),
    input_gains: pad(FAMILY_GAINS, 0.0),
    families: pad([0, 1, 2, 3, 4, 5], 0),
    lengths: &LOW_LINES,
    delays: None,
    second: false,
    mix: pad_rows(MIX),
    levels: [1.0; MAX_LINES],
    trip_scale: 1.0,
};
/// Sparse's network: its four direct reflections each reach the lines on their own.
static SPARSE: Design = Design {
    lines: 4,
    tap_groups: pad([0, 1, 2, 3], usize::MAX),
    inputs: pad_rows(SPARSE_INPUTS),
    input_gains: pad([SPARSE_INPUT_GAIN; 4], 0.0),
    families: pad(SPARSE_FAMILIES, 0),
    lengths: &SHARED_LINES,
    delays: Some(&SPARSE_DELAYS),
    second: false,
    mix: pad_rows(SPARSE_MIX),
    levels: pad(SPARSE_LEVELS, 1.0),
    // Fitted to the High and Low Shelves at 4 kHz, 1 kHz and 500 Hz and the default damping at
    // Size 1.53, 10.54 and 100 (`reverb-network-density-filters`); the exponent of the length came
    // out at 0.99.
    trip_scale: 1.37,
};
/// Mid's network.
static MID: Design = Design {
    lines: 8,
    tap_groups: MID_TAP_GROUPS,
    inputs: pad_rows(MID_INPUTS),
    input_gains: [MID_HIGH_INPUT_GAIN; MAX_LINES],
    families: pad(MID_FAMILIES, 0),
    lengths: &SHARED_LINES,
    delays: None,
    second: true,
    mix: pad_rows(MID_MIX),
    levels: pad(MID_LEVELS, 1.0),
    trip_scale: 1.0,
};
/// High's network.
static HIGH: Design = Design {
    lines: 10,
    tap_groups: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    inputs: pad_rows(HIGH_INPUTS),
    input_gains: [MID_HIGH_INPUT_GAIN; MAX_LINES],
    families: HIGH_FAMILIES,
    lengths: &SHARED_LINES,
    delays: None,
    second: true,
    mix: pad_rows(HIGH_MIX),
    levels: HIGH_LEVELS,
    trip_scale: 1.0,
};

/// Cosine and sine of each line's Chorus phase: each side's lines spread evenly round the cycle,
/// the right's half a step after the left's. A side's lines then never swing the same way
/// together, so their Doppler shifts cancel in what it hears (with golden-angle phases Low's
/// highs above 16 kHz rose and fell 1.1 to 1.2 dB at the Chorus rate against the reference's 0.6; with
/// these 0.3 to 0.5 dB, `reverb-modulation-density`).
fn chorus_phases(design: &Design) -> [(f64, f64); MAX_LINES] {
    let sides: [Option<usize>; MAX_LINES] = std::array::from_fn(|k| (k < design.lines).then(|| design.side(k)));
    std::array::from_fn(|k| {
        let Some(side) = sides[k] else { return (0.0, 0.0) };
        let count = sides.iter().flatten().filter(|&&s| s == side).count();
        let place = sides[..k].iter().flatten().filter(|&&s| s == side).count();
        let p = std::f64::consts::TAU * (place as f64 + 0.5 * side as f64) / count as f64;
        (p.cos(), p.sin())
    })
}

/// The network of Density `density` (0 Sparse to 3 High); an unknown one runs Low's.
pub(crate) fn design(density: usize) -> &'static Design {
    match density {
        0 => &SPARSE,
        2 => &MID,
        3 => &HIGH,
        _ => &LOW,
    }
}

/// `design`'s mixing with its rows made exactly orthonormal (Gram-Schmidt), so the loop neither
/// gains nor loses energy by itself however long it rings.
fn orthonormal(design: &Design) -> [[f64; MAX_LINES]; MAX_LINES] {
    let n = design.lines;
    let mut rows = design.mix;
    for i in 0..n {
        for j in 0..i {
            let dot: f64 = rows[i][..n].iter().zip(&rows[j][..n]).map(|(a, b)| a * b).sum();
            let done = rows[j];
            for (x, d) in rows[i][..n].iter_mut().zip(done) {
                *x -= dot * d;
            }
        }
        let norm = rows[i][..n].iter().map(|x| x * x).sum::<f64>().sqrt();
        for x in &mut rows[i][..n] {
            *x /= norm;
        }
    }
    rows
}

/// The mixing the loop runs: [`orthonormal`]'s, between the lines as heard, each at its level.
/// The loop keeps its energy measured with each line taken at its level.
fn loop_mix(design: &Design) -> [[f64; MAX_LINES]; MAX_LINES] {
    let mut mix = orthonormal(design);
    let levels = &design.levels[..design.lines];
    for (row, level) in mix.iter_mut().zip(levels) {
        for (m, from) in row.iter_mut().zip(levels) {
            *m *= level / from;
        }
    }
    mix
}

/// The lengths and gains the network runs with, worked out at the control rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Lengths {
    /// Each line's delay (frames).
    pub lines: [f64; MAX_LINES],
    /// How much of a frame past its delay each line is read, by linear interpolation.
    pub fractions: [f64; MAX_LINES],
    /// Each line's allpass (frames).
    pub allpasses: [f64; MAX_LINES],
    /// Each line's second allpass, in series after the first (frames; 0 for none).
    pub seconds: [f64; MAX_LINES],
    /// Each line's gain per trip through its delay.
    pub gains: [f64; MAX_LINES],
    /// Each line's allpass gain per pass through the allpass's delay.
    pub losses: [f64; MAX_LINES],
    /// The same for the second allpass.
    pub second_losses: [f64; MAX_LINES],
    /// Each line's share of a damping trip.
    pub trips: [f64; MAX_LINES],
}

impl Default for Lengths {
    fn default() -> Self {
        Self {
            lines: [2.0; MAX_LINES],
            fractions: [0.0; MAX_LINES],
            allpasses: [2.0; MAX_LINES],
            seconds: [0.0; MAX_LINES],
            gains: [0.0; MAX_LINES],
            losses: [0.0; MAX_LINES],
            second_losses: [0.0; MAX_LINES],
            trips: [0.0; MAX_LINES],
        }
    }
}

impl Lengths {
    /// The values `t` (0..1) of the way from `from` to `to`.
    pub fn between(&mut self, from: &Self, to: &Self, t: f64) {
        use crate::lerp;
        lerp(&mut self.lines, &from.lines, &to.lines, t);
        lerp(&mut self.fractions, &from.fractions, &to.fractions, t);
        lerp(&mut self.allpasses, &from.allpasses, &to.allpasses, t);
        lerp(&mut self.seconds, &from.seconds, &to.seconds, t);
        lerp(&mut self.gains, &from.gains, &to.gains, t);
        lerp(&mut self.losses, &from.losses, &to.losses, t);
        lerp(&mut self.second_losses, &from.second_losses, &to.second_losses, t);
        lerp(&mut self.trips, &from.trips, &to.trips, t);
    }

    /// `design`'s lengths for size factor `room`, Scale `scale` (0..1) at `sample_rate`, under
    /// Smooth Fast (or Slow) when `fast`; `gain` gives a line's gain per trip from its length in
    /// seconds. Whole frames when `settled`, but for the A and B lines' fractions under Fast.
    #[allow(clippy::too_many_arguments)]
    pub fn tune(
        &mut self,
        design: &Design,
        room: f64,
        scale: f64,
        sample_rate: f64,
        settled: bool,
        fast: bool,
        gain: impl Fn(f64) -> f64,
        trip_seconds: f64,
    ) {
        let whole = |x: f64| if settled { x.round() } else { x }.max(2.0);
        let percent = scale * 100.0;
        for k in 0..design.lines {
            let family = design.families[k];
            let delay = design.delays.map_or(family, |d| d[k]);
            let (line, allpass, second) =
                length(design.lengths, delay, family, design.second, room, percent, settled, fast, sample_rate);
            self.seconds[k] = if second > 0.0 { whole(second) } else { 0.0 };
            self.second_losses[k] = gain(second / sample_rate);
            if fast && reads_between_frames(family) {
                // Read between frames, linearly, moving or not.
                self.lines[k] = line.floor().max(2.0);
                self.fractions[k] = line - self.lines[k];
            } else {
                self.lines[k] = whole(line);
                self.fractions[k] = if reads_between_frames(family) { NONE_FRACTION } else { 0.0 };
            }
            self.allpasses[k] = whole(allpass);
            // The allpass loses its own time on each pass, so the echoes it holds back decay
            // as fast as the rest (as the reference's tail does at every Diffusion).
            self.gains[k] = gain(line / sample_rate);
            self.losses[k] = gain(allpass / sample_rate);
            self.trips[k] = design.trip_scale * (line + allpass + second) / sample_rate / trip_seconds;
        }
    }

    /// The longest delay of `design`'s network (frames).
    pub fn longest(design: &Design, room: f64, sample_rate: f64) -> f64 {
        let line = |k: usize| {
            let family = design.families[k];
            let delay = design.delays.map_or(family, |d| d[k]);
            length(design.lengths, delay, family, design.second, room, 100.0, false, false, sample_rate).0
        };
        (0..design.lines).map(line).fold(0.0, f64::max)
    }
}

/// The network's lines and state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Network {
    /// The lines, each with its allpass and damping; the design's first ones run.
    pub lines: [Line; MAX_LINES],
    /// The copies' low-pass, the reflections' own.
    copy_filters: [Biquad; MAX_LINES],
    /// The network running.
    design: &'static Design,
    /// Its mixing ([`loop_mix`]).
    mix: [[f64; MAX_LINES]; MAX_LINES],
    /// Cosine and sine of each line's Chorus phase.
    chorus_phases: [(f64, f64); MAX_LINES],
}

impl Default for Network {
    fn default() -> Self {
        Self {
            lines: Default::default(),
            copy_filters: Default::default(),
            design: &LOW,
            mix: loop_mix(&LOW),
            chorus_phases: chorus_phases(&LOW),
        }
    }
}

impl Network {
    /// A network for `sample_rate`, running Low's design (allocates).
    pub fn new(sample_rate: f64) -> Self {
        Self {
            lines: std::array::from_fn(|_| Line {
                delay: DelayLine::for_seconds(LINE_SECONDS, sample_rate),
                diffuser: Allpass::for_seconds(ALLPASS_SECONDS, sample_rate),
                second: Allpass::for_seconds(ALLPASS_SECONDS, sample_rate),
                damper: Damper::default(),
            }),
            ..Self::default()
        }
    }

    /// Runs `design`, silent from its start if it was not running.
    pub fn set_design(&mut self, design: &'static Design) {
        if !std::ptr::eq(self.design, design) {
            self.design = design;
            self.mix = loop_mix(design);
            self.chorus_phases = chorus_phases(design);
            self.clear();
        }
    }

    /// Silences every line.
    pub fn clear(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for filter in &mut self.copy_filters {
            filter.reset();
        }
    }

    /// Sets the copies' low-pass, the one the filtered reflections pass.
    pub fn set_copy_filter(&mut self, coefficients: Coefficients) {
        for filter in &mut self.copy_filters {
            filter.set(coefficients);
        }
    }

    /// One frame: `groups` is the input of each group of direct reflections and `copies` each
    /// family's filtered reflections, both already delayed by the entry; returns the tail of each
    /// side. `swing` gives each line's chorus swing (frames)
    /// from the cosine and sine of its phase and its length.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        groups: &[f64; MAX_GROUPS],
        copies: &[f64; FAMILY_COUNT],
        n: &Lengths,
        diffusion: f32,
        damping: &Damping,
        swing: impl Fn((f64, f64), f64) -> f64,
        frozen: bool,
    ) -> [f64; 2] {
        let d = self.design;
        let lines = d.lines;
        let weigh = |row: &[f64; MAX_GROUPS]| row.iter().zip(groups).map(|(w, g)| w * g).sum::<f64>();
        let families: [f64; MAX_LINES] = std::array::from_fn(|k| d.input_gains[k] * weigh(&d.inputs[k]));
        let mut heard = [0.0f64; MAX_LINES];
        for ((copy, filter), &family) in heard.iter_mut().zip(&mut self.copy_filters).zip(&d.families).take(lines) {
            *copy = filter.process(copies[family]);
        }

        let mut outs = [0.0f64; MAX_LINES];
        let mut tail = [0.0f64; 2];
        for (k, (line, out)) in self.lines.iter_mut().zip(&mut outs).enumerate().take(lines) {
            let length = n.lines[k] + swing(self.chorus_phases[k], n.lines[k]);
            let mut read = f64::from(line.delay.read_hermite(length));
            if n.fractions[k] != 0.0 {
                read += n.fractions[k] * (f64::from(line.delay.read_hermite(length + 1.0)) - read);
            }
            let mut y = line.diffuser.process_lossy(read as f32, n.allpasses[k], diffusion, n.losses[k] as f32);
            if n.seconds[k] > 0.0 {
                y = line.second.process_lossy(y, n.seconds[k], diffusion, n.second_losses[k] as f32);
            }
            *out = line.damper.damp(f64::from(y), damping) * n.gains[k];
            tail[d.side(k)] += *out;
        }
        // Into the mix: each line's output and its family's copy.
        let into: [f64; MAX_LINES] = std::array::from_fn(|j| outs[j] + heard[j]);
        for (k, line) in self.lines.iter_mut().enumerate().take(lines) {
            let mixed: f64 = self.mix[k][..lines].iter().zip(&into).map(|(m, y)| m * y).sum();
            let mut next = families[k] + mixed;
            if frozen {
                next = crate::limit(next);
            }
            line.delay.push(next as f32);
        }
        tail
    }
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
