// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reverb: an algorithmic room reverb in the stages a designer thinks in. The design is in
//! `docs/spec/reverb.md`.
//!
//! ```text
//! in -> mono sum -> roll-off, Low Cut, High Cut -> predelay line -+-> direct taps (Spin) -+-> families -> Reflect -+
//!                                                                  |                       +-------------------------+-> Stereo -> Dry/Wet
//!                                                                  +-> direct taps -> network -> Diffuse ----------+
//! ```
//!
//! The early reflections are the reference's measured direct taps, each followed by its filtered copies,
//! one per family of the Density. The late reverb is the reference's own network, as measured
//! ([`network`]): the direct taps feed its lines after the Shape entry.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::biquad::BUTTERWORTH_Q;
use suhina_dsp::{Allpass, Biquad, Coefficients, DelayLine, FirstOrder, FirstOrderCoefficients, Smoother};

mod network;

use network::{DIRECT_TAPS, FAMILY_COUNT, FAMILY_SIDES, REFERENCE_RATE};

/// Ramp for levels, mix, stereo and most other changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// Ramp for predelay changes: slower, so a long jump bends the pitch less on the way.
pub const PREDELAY_RAMP_SECONDS: f64 = 0.1;
/// Size glides with this time constant when Smooth is Slow.
pub const SMOOTH_SLOW_SECONDS: f64 = 0.045;
/// Size glides with this time constant when Smooth is Fast.
pub const SMOOTH_FAST_SECONDS: f64 = 0.015;
/// Frames between updates of delay lengths, tap levels and filters, counted from the last reset.
const CONTROL_FRAMES: u64 = 32;
/// However fast Smooth glides Size, no line or reflection lengthens or shortens by more than this
/// many frames a frame: faster, a line is read backwards and a jump across the range buzzes.
const GLIDE_SLEW: f64 = 0.5;

/// The reference room: delays below are for Size 100, and scale with `(size / 100)^(1/3)`.
const REFERENCE_SIZE: f64 = 100.0;
/// Which side an early reflection is heard on.
const LEFT: usize = 0;
/// See [`LEFT`].
const RIGHT: usize = 1;

/// One direct reflection: its time after the predelay in the reference room (ms), its side and
/// its level against the first (before the fade).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tap {
    ms: f64,
    side: usize,
    gain: f64,
}

/// A [`Tap`].
const fn tap(ms: f64, side: usize, gain: f64) -> Tap {
    Tap { ms, side, gain }
}

// The direct reflections of each Density, and the level of each family's filtered reflection of
// each of them (family by family, against the first direct reflection; zero where a family has
// none), as measured at 44.1 kHz (docs/spec/reverb.md › Early reflections). Times are in the
// reference room, each a whole number of frames at 44.1 kHz plus the fraction by which the reference
// rounds it to a frame (read from where it steps as the predelay moves by tenths of a frame). The
// reflections at 17.90 and 41.12 ms sit 0.59 of a frame past a frame: between that read (0.65)
// and what their frames at Size 0.222 to 500 allow.
const TAPS_SPARSE: [Tap; 4] =
    [tap(0.0011, RIGHT, 1.5008), tap(26.5367, LEFT, -1.5008), tap(41.1244, LEFT, 1.5007), tap(55.6475, RIGHT, 1.5007)];
const FILTERED_SPARSE: [[f64; DIRECT_TAPS]; FAMILY_COUNT] = [
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.5231, 1.9045, -0.1019, -0.6703, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.8118, 0.0871, 1.8241, 0.6035, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [-1.2085, -0.0990, 0.9975, -1.3755, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [1.4018, -0.8461, -0.1582, -1.2853, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
];
const TAPS_LOW: [Tap; 10] = [
    tap(0.0011, RIGHT, 1.0006),
    tap(7.3707, RIGHT, -1.0015),
    tap(11.4138, LEFT, 0.9997),
    tap(17.9045, LEFT, -1.0005),
    tap(26.5367, LEFT, -0.9953),
    tap(28.4206, LEFT, 0.9985),
    tap(34.1757, RIGHT, -0.9989),
    tap(41.1244, LEFT, 0.9976),
    tap(48.2846, RIGHT, -0.9966),
    tap(55.6475, RIGHT, 0.9951),
];
// Low's filtered reflections are its network's line inputs as heard ([`HEARD_COPY`]), not
// its own read taps: they match the reference's reflections and tail more closely.
const FILTERED_LOW: [[f64; DIRECT_TAPS]; FAMILY_COUNT] = [
    [1.0046, -1.0055, 0.6008, 0.0880, 0.0876, -0.3697, -0.2277, 0.5996, 0.5789, -0.5780],
    [-0.1137, 0.1138, 0.5670, -0.7464, -0.7425, 0.6401, 0.6626, 0.5658, 0.3845, -0.3839],
    [0.1224, -0.1226, 0.5327, 0.8629, 0.8584, 0.8568, -0.0886, 0.5316, -0.3712, 0.3706],
    [-0.6059, 0.6064, 0.7324, -0.3616, -0.3598, -0.1434, -0.9257, 0.7309, -0.1136, 0.1135],
    [-0.6745, 0.6751, 0.4080, 0.6666, 0.6632, -0.5739, 0.5313, 0.4072, 0.4912, -0.4905],
    [0.2339, -0.2341, 0.4853, -0.1921, -0.1911, -0.5374, 0.5163, 0.4842, -1.0149, 1.0133],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
];
const TAPS_MID: [Tap; 10] = [
    tap(0.0011, RIGHT, 1.0000),
    tap(7.3707, RIGHT, 1.0000),
    tap(11.4138, LEFT, 1.0000),
    tap(17.9045, RIGHT, -1.0000),
    tap(26.5367, LEFT, -1.0000),
    tap(28.4206, LEFT, 0.9999),
    tap(34.1757, RIGHT, -1.0000),
    tap(41.1244, LEFT, 1.0000),
    tap(48.2846, RIGHT, 0.9999),
    tap(55.6475, RIGHT, 0.9999),
];
const FILTERED_MID: [[f64; DIRECT_TAPS]; FAMILY_COUNT] = [
    [-0.4317, -0.1195, -0.3981, 0.4319, 0.9028, -0.3047, 0.1194, 0.4944, 0.6194, -0.0160],
    [-0.5411, 0.1315, 0.7646, 0.5408, -0.5835, 0.0546, -0.1316, 0.5936, 0.5456, 0.0858],
    [0.6484, -0.0709, 0.2397, -0.6487, 0.2295, -0.0302, 0.0709, 0.4650, -0.1003, 1.0641],
    [0.7252, 0.4949, -0.5185, -0.7250, -0.4603, -0.3055, -0.4949, 0.5126, 0.3687, -0.3907],
    [-0.0920, 0.9311, 0.2723, 0.0918, 0.5024, 0.3676, -0.9311, 0.3900, -0.5791, -0.2662],
    [-0.5899, -0.1251, -0.7313, 0.5901, -0.4578, 0.0127, 0.1252, 0.5213, -0.6395, 0.3267],
    [0.3106, -0.6956, -0.0172, -0.3105, 0.0747, 0.9352, 0.6954, 0.4950, 0.0161, -0.4359],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000, 0.0000],
    [0.1315, -0.4977, 0.4659, -0.1319, 0.0900, -0.8341, 0.4976, 0.3885, -0.5687, -0.4850],
];
const TAPS_HIGH: [Tap; 10] = [
    tap(0.0011, RIGHT, 1.0000),
    tap(7.3707, RIGHT, 1.0000),
    tap(11.4138, LEFT, 1.0000),
    tap(17.9045, LEFT, -1.0002),
    tap(26.5367, LEFT, -1.0000),
    tap(28.4206, LEFT, 0.9998),
    tap(34.1757, RIGHT, -1.0000),
    tap(41.1244, LEFT, 1.0000),
    tap(48.2846, RIGHT, 1.0000),
    tap(55.6475, RIGHT, 1.0000),
];
const FILTERED_HIGH: [[f64; DIRECT_TAPS]; FAMILY_COUNT] = [
    [-0.6668, -0.1437, -0.0629, 0.8160, -0.6538, -0.0838, -0.1165, 0.2255, 0.3069, 0.4120],
    [0.3059, -0.7069, -0.2268, -0.6291, -0.2988, -0.5208, 0.0000, 0.2707, 0.5770, 0.3007],
    [-0.4857, 0.1872, -0.2408, -0.3882, -0.1026, 0.6148, 0.4628, 0.4663, 0.5408, -0.5526],
    [0.0201, -0.7228, -0.3483, 0.5178, 0.5493, -0.1218, 0.5763, 0.3938, -0.3523, -0.2411],
    [-0.0314, -0.2769, 0.6985, 0.0384, -0.3848, -0.1344, 0.8028, -0.6665, 0.1176, -0.2507],
    [0.3623, -0.2285, 0.7793, 0.3540, 0.4578, 0.3675, -0.3262, 0.3103, 0.6584, -0.0275],
    [0.6719, -0.2095, -0.4210, 0.3561, -0.6232, 0.3210, -0.3266, -0.2049, 0.0252, -0.6771],
    [-0.0427, -0.4897, 0.3989, -0.3442, -0.4179, 0.6754, -0.0900, 0.3630, -0.7236, 0.2912],
    [-0.4708, -0.5851, -0.3286, -0.1239, 0.3855, 0.4733, -0.3302, -0.8169, 0.2067, 0.1370],
    [-0.5626, -0.2564, 0.3512, -0.1747, 0.0000, -0.5390, -0.6032, 0.1400, -0.2000, -0.8052],
];

/// The direct reflections in Density order.
const TAP_TABLES: [&[Tap]; 4] = [&TAPS_SPARSE, &TAPS_LOW, &TAPS_MID, &TAPS_HIGH];
/// The filtered reflections in Density order.
const FILTERED_TABLES: [&[[f64; DIRECT_TAPS]; FAMILY_COUNT]; 4] =
    [&FILTERED_SPARSE, &FILTERED_LOW, &FILTERED_MID, &FILTERED_HIGH];
/// The reflections are read this many frames after the predelay: one before it, as the reference's are at
/// Stereo 120 (`reverb-network-exact`), at every rate (the first reflection at the default 2.5 ms
/// is at frame 119 at 48 kHz and 239 at 96 kHz: `reverb-rate-48k`, `reverb-rate-96k`); below 120
/// Stereo delays them ([`stereo_delay`]).
const TAP_OFFSET_FRAMES: f64 = -1.0;
/// Below Stereo 120 the reflections, not the tail, come later: `STEREO_DELAY_SECONDS × (1 − Stereo
/// / 120)` in frames at the session's rate, [`network::rounded`] (`reverb-er-stereo`: 27 and 32
/// frames at Stereo 0 under Fast and None, 4 and 8 at 100; `reverb-rate-48k`: 29 and 32 at
/// Stereo 0, 14 and 16 at 60; `reverb-rate-96k`: 59 and 64, 29 and 32). 27.5625 frames at 44.1 kHz.
const STEREO_DELAY_SECONDS: f64 = 0.000_625;
/// Under Smooth Fast, where Stereo's delay is a whole number of frames the reference's is a frame less
/// (30, 15, 60 and 30 at 48 and 96 kHz come out 29, 14, 59 and 29): it is floored this far below.
const STEREO_FLOOR_SLACK: f64 = 1e-6;
/// Level of the first reflection against the input, before the predelay's fade.
const TAP_GAIN: f64 = 0.0942;
/// How fast the reflections fade, in dB per ms of their delay, times the Decay setting in
/// seconds: this plus [`TAP_FADE_PER_SHAPE`] times Shape.
const TAP_FADE: f64 = 0.0328;
/// How much faster the reflections fade per unit of Shape (dB per ms, times Decay in seconds).
const TAP_FADE_PER_SHAPE: f64 = 0.0668;
/// The whole wet signal is this many dB quieter per ms of predelay, times 1 / Decay in seconds
/// (0.0565 dB per ms at 0.2 s, 0.0094 at 1.2 s).
const PREDELAY_FADE: f64 = 0.0113;
/// Spin swings every reflection's whole delay, predelay included, by this fraction of it per unit
/// of Spin Amount, times [`spin_reach`] (docs/spec/reverb.md › Early reflections): tracked tap by
/// tap in the reference's click trains at Amount 17.5 and 55 and Predelay 0.5 to 250 ms, the swing is
/// 1.000 to 1.005 times this (`reverb-spin-taps`, `reverb-spin-rate`).
const SPIN_FRACTION: f64 = 0.01;
/// Above the knee, [`spin_reach`] is this many ms over the predelay less
/// [`SPIN_REACH_OFFSET_MS`]: a long predelay swings by about the same 2 ms times the fraction
/// whatever the reflection's own delay.
const SPIN_REACH_MS: f64 = 2.0;
/// See [`SPIN_REACH_MS`]. The knee is where the two meet, 5 ms: tracked tap by tap, the reference's swing
/// at Predelay 6.5, 8, 10, 25 and 50 ms is 2 / (predelay − 3.00) within 0.1 % (`reverb-spin-knee`,
/// `reverb-spin-taps`).
const SPIN_REACH_OFFSET_MS: f64 = 3.0;
/// Spin's LFOs, one per direct reflection (the same ten times in every Density): the
/// reflection's time in the reference room (ms), its rate against Spin Rate and its phase when
/// playback starts (degrees, of a sine that lengthens the delay). Tracked tap by tap in the reference's click
/// trains (`reverb-spin-taps`, `reverb-spin-rate`, `reverb-spin-knee` at Predelay 8 to 250 ms):
/// the rates agree to 0.0001 at Spin Rate 0.30 and 1.30 Hz and at every Predelay and Amount, and
/// the phases, the same at every Rate, are within 0.1° of whole tens of degrees.
const SPIN_VOICES: [(f64, f64, f64); 10] = [
    (0.000, 0.9897, 40.0),
    (7.370, 1.0430, 100.0),
    (11.429, 0.9981, 70.0),
    (17.914, 0.9825, 90.0),
    (26.553, 1.0230, 30.0),
    (28.435, 1.0071, 50.0),
    (34.172, 1.0300, 60.0),
    (41.134, 1.0150, 10.0),
    (48.299, 1.0150, 80.0),
    (55.646, 0.9917, 20.0),
];
/// The reference's Spin LFOs run no slower than this (Hz): at Spin Rate 0.07 Hz every tap swings 1.057 times
/// as fast as at 0.30 and 1.30 (`reverb-spin-rate` r0.07-*), at 0.15 Hz at the Rate itself
/// (`reverb-white-noise` spin-amt35-0.15Hz is 15 dB further from the reference's 2 % faster).
const SPIN_RATE_FLOOR: f64 = 0.074;
/// The reference reads its Spin LFOs this far ahead (seconds): each tap arrives later than an LFO read at
/// its own time would put it by 6.1 to 6.8 frames times the speed its delay changes at
/// (`reverb-spin-rate` r1.3-p0.5 and r1.3-a55-p100, tap by tap); 0.12 to 0.18 ms all bring
/// `reverb-white-noise-modulation` spin-amt55-1.3Hz 4 to 5 dB nearer the reference's, 0.15 the most.
const SPIN_LEAD_SECONDS: f64 = 0.000_15;
/// Longest delay from a direct reflection to its family's copy in the network: its filtered
/// reflection at the largest Size (0.13 s) and the Shape entry after it (seconds).
const FAMILY_LINE_SECONDS: f64 = 0.2;
/// Longest delay from a direct reflection to the network, [`network::entry`] at Shape 1
/// (seconds).
const ENTRY_LINE_SECONDS: f64 = 0.06;
/// Peak swing of a line's delay per unit of Chorus Amount: a 1 kHz tone through the reference's tail swings
/// 12 to 343 cents (left and right added) at five Amounts and Rates, and through Suhina's 12 to 397
/// with this swing (`reverb-texture` chorus-*). The loop adds each trip's shift to the last, so a
/// line swings about a fifth of what the tone's deviation read as one line's would say (1.8 ms).
const CHORUS_SECONDS: f64 = 0.000_38;
/// The reflections' low-pass: a cookbook low-pass with this Q, so two equal poles ...
const REFLECTION_Q: f64 = 0.5;
/// ... whose corner rises towards this frequency (Hz) as Decay grows ...
const REFLECTION_TOP: f64 = 11_980.0;
/// ... its pole (at 44.1 kHz) above the top's by this much times `1 - exp(-t × room / Decay)` ...
const REFLECTION_REACH: f64 = 1.115;
/// ... with `t` (seconds) by Density, in Density order, read at Size 100 (`reverb-er-decay`), and
/// `room` the size factor: a smaller room's reflections are brighter at the same Decay (a Low
/// fit at Size 1.53, 10.54 and 100 gives `t / room` 0.053 to 0.066 s).
const REFLECTION_SECONDS: [f64; 4] = [0.2092, 0.0512, 0.3200, 0.2657];

/// The reflections' low-pass corner (Hz) for a Decay of `decay` seconds at Density `density` and
/// size factor `room`.
fn reflection_corner(decay: f64, density: usize, room: f64) -> f64 {
    // A cookbook low-pass of Q 0.5 at angle w has its double pole at tan(pi/4 - w/2).
    let angle = |hz: f64| std::f64::consts::TAU * hz / REFERENCE_RATE;
    let top = (std::f64::consts::FRAC_PI_4 - angle(REFLECTION_TOP) / 2.0).tan();
    let seconds = REFLECTION_SECONDS.get(density).copied().unwrap_or(REFLECTION_SECONDS[LOW]);
    let pole = top + REFLECTION_REACH * (1.0 - (-seconds * room / decay.max(1e-3)).exp());
    (std::f64::consts::FRAC_PI_2 - 2.0 * pole.atan()) * REFERENCE_RATE / std::f64::consts::TAU
}
/// The input band's high cut never goes above this frequency, and the input passes a low-pass
/// at it when High Cut is off.
const TOP_CUT: f64 = 18_000.0;

/// Density Low: what an unknown Density plays.
const LOW: usize = 1;
/// A family's copy in the network is its filtered reflections, heard this many times as loud: read
/// tap by tap against the copies fitted to the tails, the ratio is 1.69 to 1.71 at every Density
/// (within 1.5 % for all but the faintest taps).
const HEARD_COPY: f64 = 1.70;
/// Level of the network's input against the input, before the predelay's fade: with it, each
/// line's first pass is [`network`]'s gain times the direct reflection it comes from, as the reference's
/// is (`reverb-network-exact`).
const NETWORK_GAIN: f64 = 0.294_538_8;
/// Every Density's reflections are `a + b × Size` of their reference-room level: a pure gain on the
/// Size 100 response, read at Sizes 0.222 to 500 for Low (`reverb-network-small`) and 1.53 to 100
/// for Sparse, Mid and High (`reverb-network-density-filters`). The network's input takes it, and
/// [`NETWORK_SIZE_LEVEL`] too.
const SIZE_LEVEL: (f64, f64) = (0.92594, 0.000_740_59);
/// The network's input grows with Size a second time, `a + b × Size + c × Size²`, which keeps
/// close to the reflections' own law to Size 100 and falls behind it above (0.45 dB by Size 500):
/// read against the reflections at Low at Size 0.222 to 500 (`reverb-network-small`,
/// `reverb-network-scale`, `reverb-network-size-level`), to 0.012 dB, and the same at Mid and High
/// at 300 and 400.
const NETWORK_SIZE_LEVEL: (f64, f64, f64) = (0.935_054, 0.000_751_857, -0.000_000_350_508);

/// The damping filters' loss is per trip round a loop this long (seconds, reference room).
const LOSS_TRIP_SECONDS: f64 = 0.17;
/// The damping shelves are cookbook shelves of this Q ...
const SHELF_Q: f64 = 0.70;
/// ... the High Shelf half way at this much of its frequency and the Low Shelf at its frequency
/// over this: with them `reverb-network-filters` (1 kHz and 500 Hz, gain 0.2) and
/// `reverb-network-small` (4 kHz, gain 0.4, Size 1.53 to 100) match the reference's renders to −32 to −36 dB
/// from 4 to 8 s, against −12 to −18 dB with Q 0.707 at the frequency.
const SHELF_SHIFT: f64 = 0.98;
/// Decay time the network is tuned to, against the Decay setting.
const DECAY_RATIO: f64 = 0.9;
/// Low Cut, as measured (`reverb-locut-law`, docs/spec/reverb.md): a first-order high-pass at
/// this many times the band's lower edge, with two first-order shelves whose gains follow the
/// edge ...
const LOW_CUT_RATIO: f64 = 2.5;
/// ... their corners (Hz) ...
const LOW_CUT_SHELVES: [f64; 2] = [25.0, 250.0];
/// ... and their gains (dB) at these lower edges (Hz), joined linearly against the edge's
/// logarithm and held beyond the ends.
const LOW_CUT_GAINS: [(f64, [f64; 2]); 6] = [
    (132.0, [10.57, 1.17]),
    (192.0, [7.09, 1.30]),
    (264.0, [4.60, 0.71]),
    (500.0, [2.65, -3.49]),
    (658.0, [4.38, -7.11]),
    (841.0, [7.58, -12.42]),
];
/// Below this lower edge (Hz) the reference's Low Cut does nothing (read at 40 and 44.19 Hz; between there
/// and 132 Hz, the lowest edge read otherwise, it is not read).
const LOW_CUT_LOWEST: f64 = 44.2;
/// The input always passes a first-order high-pass at this frequency, as measured
/// (docs/spec/reverb.md).
const RUMBLE_FREQUENCY: f64 = 127.0;
/// ... and a second one at this frequency.
const SUBSONIC_FREQUENCY: f64 = 19.0;
/// The network frozen with Flat off falls this many dB per second besides its damping and A's and
/// B's reads, and with Flat on nothing besides those reads: the reference's frozen tail at 1 kHz falls 0.11
/// and 0.20 dB/s with Flat on and off (`reverb-freeze-static`, Smooth Fast), most of it from A's
/// and B's fractions of a frame.
const FREEZE_DRAIN_UNFLAT: f64 = 0.03;
/// Inside a frozen loop that still takes input, levels above this are compressed so the tail
/// cannot grow without end.
const FREEZE_LIMIT: f64 = 2.0;

/// Longest delay the predelay line needs, with some room.
const PREDELAY_LINE_SECONDS: f64 = 1.0;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    /// Time from the input to the first reflection, in seconds.
    pub const PREDELAY: usize = 0;
    /// Low Cut on the input band.
    pub const LOW_CUT: usize = 1;
    /// High Cut on the input band.
    pub const HIGH_CUT: usize = 2;
    /// Centre of the input band in hertz.
    pub const INPUT_FREQUENCY: usize = 3;
    /// Width of the input band in octaves.
    pub const INPUT_WIDTH: usize = 4;
    /// Spin, which swings the early reflections' delays, on or off.
    pub const SPIN: usize = 5;
    /// How fast Spin swings, in hertz.
    pub const SPIN_RATE: usize = 6;
    /// How far Spin swings.
    pub const SPIN_AMOUNT: usize = 7;
    /// How the reflections fade and when the tail sets in, 0..1.
    pub const SHAPE: usize = 8;
    /// High damping in the loop on or off.
    pub const HIGH_DAMPING: usize = 9;
    /// High damping as a shelf (0) or a low-pass (1).
    pub const HIGH_DAMPING_TYPE: usize = 10;
    /// High damping's frequency in hertz.
    pub const HIGH_DAMPING_FREQUENCY: usize = 11;
    /// High damping's gain per second, linear.
    pub const HIGH_DAMPING_GAIN: usize = 12;
    /// Low damping in the loop on or off.
    pub const LOW_DAMPING: usize = 13;
    /// Low damping's frequency in hertz.
    pub const LOW_DAMPING_FREQUENCY: usize = 14;
    /// Low damping's gain per second, linear.
    pub const LOW_DAMPING_GAIN: usize = 15;
    /// The diffusers' allpass gain.
    pub const DIFFUSION: usize = 16;
    /// The diffusers' delays as a fraction of their longest.
    pub const SCALE: usize = 17;
    /// Chorus, which swings the network's line lengths, on or off.
    pub const CHORUS: usize = 18;
    /// How fast Chorus swings, in hertz.
    pub const CHORUS_RATE: usize = 19;
    /// How far Chorus swings.
    pub const CHORUS_AMOUNT: usize = 20;
    /// The room's size; 100 is the reference room.
    pub const SIZE: usize = 21;
    /// How Size glides to a new value: None, Slow or Fast.
    pub const SMOOTH: usize = 22;
    /// Decay time of the tail, in seconds.
    pub const DECAY: usize = 23;
    /// Holds the tail without decay.
    pub const FREEZE: usize = 24;
    /// While frozen, leaves the damping out.
    pub const FLAT: usize = 25;
    /// While frozen, takes no new input.
    pub const CUT: usize = 26;
    /// Width of the wet signal, 0 (mono) to 120.
    pub const STEREO: usize = 27;
    /// How many lines the network runs: Sparse, Low, Mid or High.
    pub const DENSITY: usize = 28;
    /// Level of the early reflections in decibels.
    pub const REFLECT: usize = 29;
    /// Level of the late reverb in decibels.
    pub const DIFFUSE: usize = 30;
    /// Share of the wet signal in the output.
    pub const MIX: usize = 31;
    /// How many parameters there are.
    pub(crate) const COUNT: usize = 32;
}

/// Lowest Reflect and Diffuse level: a linear gain of 0.03.
pub const MIN_LEVEL_DB: f64 = -30.4576;

/// The parameters, in [`param`] order.
const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("predelay", "Predelay", (0.000_5, 0.25), 0.002_5, Unit::Seconds, Scale::Logarithmic)
        .in_group("Input"),
    ParamInfo::toggle("lowCut", "Low Cut", false).in_group("Input"),
    ParamInfo::toggle("highCut", "High Cut", true).in_group("Input"),
    ParamInfo::continuous(
        "inputFrequency",
        "Input Frequency",
        (50.0, 18_000.0),
        830.0,
        Unit::Hertz,
        Scale::Logarithmic,
    )
    .in_group("Input"),
    ParamInfo::continuous("inputWidth", "Input Width", (0.5, 9.0), 5.85, Unit::Octaves, Scale::Linear)
        .in_group("Input"),
    ParamInfo::toggle("spin", "Spin", true).in_group("Early Reflections"),
    ParamInfo::continuous("spinRate", "Spin Rate", (0.07, 1.3), 0.3, Unit::Hertz, Scale::Logarithmic)
        .in_group("Early Reflections"),
    ParamInfo::continuous("spinAmount", "Spin Amount", (2.0, 55.0), 17.5, Unit::None, Scale::Logarithmic)
        .in_group("Early Reflections"),
    ParamInfo::continuous("shape", "Shape", (0.0, 1.0), 0.5, Unit::None, Scale::Linear).in_group("Early Reflections"),
    ParamInfo::toggle("highDamping", "High Damping", true).in_group("Damping"),
    ParamInfo::choice("highDampingType", "High Damping Type", &["Shelf", "Low-Pass"], 0).in_group("Damping"),
    ParamInfo::continuous(
        "highDampingFrequency",
        "High Damping Frequency",
        (20.0, 16_000.0),
        4_500.0,
        Unit::Hertz,
        Scale::Logarithmic,
    )
    .in_group("Damping"),
    ParamInfo::continuous("highDampingGain", "High Damping Gain", (0.2, 1.0), 0.7, Unit::None, Scale::Linear)
        .in_group("Damping"),
    ParamInfo::toggle("lowDamping", "Low Damping", false).in_group("Damping"),
    ParamInfo::continuous(
        "lowDampingFrequency",
        "Low Damping Frequency",
        (20.0, 15_000.0),
        90.0,
        Unit::Hertz,
        Scale::Logarithmic,
    )
    .in_group("Damping"),
    ParamInfo::continuous("lowDampingGain", "Low Damping Gain", (0.2, 1.0), 0.75, Unit::None, Scale::Linear)
        .in_group("Damping"),
    ParamInfo::continuous("diffusion", "Diffusion", (0.001, 0.96), 0.6, Unit::Percent, Scale::Linear)
        .in_group("Diffusion"),
    ParamInfo::continuous("scale", "Scale", (0.05, 1.0), 0.4, Unit::Percent, Scale::Linear).in_group("Diffusion"),
    ParamInfo::toggle("chorus", "Chorus", true).in_group("Diffusion"),
    ParamInfo::continuous("chorusRate", "Chorus Rate", (0.01, 8.0), 0.02, Unit::Hertz, Scale::Logarithmic)
        .in_group("Diffusion"),
    ParamInfo::continuous("chorusAmount", "Chorus Amount", (0.01, 4.0), 0.02, Unit::None, Scale::Linear)
        .in_group("Diffusion"),
    ParamInfo::continuous("size", "Size", (0.222, 500.0), 100.0, Unit::None, Scale::Logarithmic).in_group("Decay"),
    ParamInfo::choice("smooth", "Smooth", &["None", "Slow", "Fast"], 2).in_group("Decay"),
    ParamInfo::continuous("decay", "Decay", (0.2, 60.0), 1.2, Unit::Seconds, Scale::Logarithmic).in_group("Decay"),
    ParamInfo::toggle("freeze", "Freeze", false).in_group("Decay"),
    ParamInfo::toggle("flat", "Flat", true).in_group("Decay"),
    ParamInfo::toggle("cut", "Cut", true).in_group("Decay"),
    ParamInfo::continuous("stereo", "Stereo", (0.0, 120.0), 100.0, Unit::None, Scale::Linear).in_group("Global"),
    ParamInfo::choice("density", "Density", &["Sparse", "Low", "Mid", "High"], 1).in_group("Global"),
    ParamInfo::continuous("reflect", "Reflect", (MIN_LEVEL_DB, 6.0), 0.0, Unit::Decibels, Scale::Linear)
        .in_group("Output"),
    ParamInfo::continuous("diffuse", "Diffuse", (MIN_LEVEL_DB, 6.0), 0.0, Unit::Decibels, Scale::Linear)
        .in_group("Output"),
    ParamInfo::continuous("mix", "Dry/Wet", (0.0, 1.0), 0.55, Unit::Percent, Scale::Linear).in_group("Output"),
];

/// What the reverb is and the parameters it has.
pub const INFO: DeviceInfo = DeviceInfo { id: "reverb", name: "Reverb", role: Role::Effect, params: &PARAMS };

/// `out` set to the values `t` (0..1) of the way from `a` to `b`.
fn lerp<const N: usize>(out: &mut [f64; N], a: &[f64; N], b: &[f64; N], t: f64) {
    for ((o, a), b) in out.iter_mut().zip(a).zip(b) {
        *o = a + (b - a) * t;
    }
}

/// What the control-rate update works out for the taps and the network.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tuning {
    /// Each direct reflection's delay after the predelay (frames).
    tap_delays: [f64; DIRECT_TAPS],
    /// Each direct reflection's gain (linear).
    tap_gains: [f64; DIRECT_TAPS],
    /// Each direct reflection's level before its own gain: what its filtered reflections are
    /// weighed against.
    reflection_levels: [f64; DIRECT_TAPS],
    /// Each direct reflection's share of the network's input.
    feed_gains: [f64; DIRECT_TAPS],
    /// Each family's delay after its direct reflections (frames).
    family_delays: [f64; FAMILY_COUNT],
    /// Each family's fade over that delay.
    family_gains: [f64; FAMILY_COUNT],
    /// The network's lengths and gains.
    network: network::Lengths,
    /// Delay from the predelay to the network's input (frames).
    entry: f64,
    /// Gain of the input into the network.
    network_feed: f64,
    /// Gain of the families' filtered reflections into the network.
    copy_gain: f64,
}

impl Default for Tuning {
    /// Silent taps and lines at the shortest lengths, until the first tuning.
    fn default() -> Self {
        Self {
            tap_delays: [2.0; DIRECT_TAPS],
            tap_gains: [0.0; DIRECT_TAPS],
            reflection_levels: [0.0; DIRECT_TAPS],
            feed_gains: [0.0; DIRECT_TAPS],
            family_delays: [2.0; FAMILY_COUNT],
            family_gains: [0.0; FAMILY_COUNT],
            network: network::Lengths::default(),
            entry: 2.0,
            network_feed: 0.0,
            copy_gain: 0.0,
        }
    }
}

impl Tuning {
    /// The values `t` (0..1) of the way from `from` to `to`.
    fn between(&mut self, from: &Self, to: &Self, t: f64) {
        lerp(&mut self.tap_delays, &from.tap_delays, &to.tap_delays, t);
        lerp(&mut self.tap_gains, &from.tap_gains, &to.tap_gains, t);
        lerp(&mut self.reflection_levels, &from.reflection_levels, &to.reflection_levels, t);
        lerp(&mut self.feed_gains, &from.feed_gains, &to.feed_gains, t);
        lerp(&mut self.family_delays, &from.family_delays, &to.family_delays, t);
        lerp(&mut self.family_gains, &from.family_gains, &to.family_gains, t);
        self.network.between(&from.network, &to.network, t);
        self.entry = from.entry + (to.entry - from.entry) * t;
        self.network_feed = from.network_feed + (to.network_feed - from.network_feed) * t;
        self.copy_gain = from.copy_gain + (to.copy_gain - from.copy_gain) * t;
    }
}

/// One line of the network and its damping.
#[derive(Debug, Clone, Default, PartialEq)]
struct Line {
    /// The line's delay.
    delay: DelayLine,
    /// The allpass the line's output passes on every trip.
    diffuser: Allpass,
    /// The second allpass after it (Mid's and High's lines).
    second: Allpass,
    /// The line's damping.
    damper: Damper,
}

/// The network's damping: the high shelf and the low-pass (High damping's two types) and the low
/// shelf. Every filter always runs and the switches crossfade,
/// so turning one on, off or to the other type does not click.
#[derive(Debug, Clone, Default, PartialEq)]
struct Damper {
    /// High damping as a shelf.
    high_shelf: Biquad,
    /// High damping as a low-pass: one whole one-pole on every pass.
    low_pass: FirstOrder,
    /// Low damping.
    low_shelf: Biquad,
}

/// A filter section the damping runs.
trait Section {
    /// One sample through the section.
    fn run(&mut self, x: f64) -> f64;
    /// Silences the section.
    fn clear(&mut self);
}

impl Section for FirstOrder {
    fn run(&mut self, x: f64) -> f64 {
        self.process(x)
    }

    fn clear(&mut self) {
        self.reset();
    }
}

impl Section for Biquad {
    fn run(&mut self, x: f64) -> f64 {
        self.process(x)
    }

    fn clear(&mut self) {
        self.reset();
    }
}

impl Line {
    /// Silences the line and its damping.
    fn clear(&mut self) {
        self.delay.clear();
        self.diffuser.clear();
        self.second.clear();
        self.damper.clear();
    }
}

impl Damper {
    /// Silences the filters.
    fn clear(&mut self) {
        self.high_shelf.reset();
        self.low_pass.reset();
        self.low_shelf.reset();
    }

    /// `y` through the damping, each filter heard in proportion to its amount. A filter that is
    /// not heard does not run, and starts again from silence (faded in by its amount).
    #[inline]
    fn damp(&mut self, y: f64, amounts: &Damping) -> f64 {
        fn heard(filter: &mut impl Section, x: f64, amount: f64) -> f64 {
            if amount > 0.0 {
                filter.run(x)
            } else {
                filter.clear();
                x
            }
        }
        let y = if amounts.high > 0.0 {
            let shelf = heard(&mut self.high_shelf, y, 1.0 - amounts.low_pass);
            let pass = heard(&mut self.low_pass, y, amounts.low_pass);
            let high = shelf + (pass - shelf) * amounts.low_pass;
            y + (high - y) * amounts.high
        } else {
            self.high_shelf.reset();
            self.low_pass.reset();
            y
        };
        y + (heard(&mut self.low_shelf, y, amounts.low) - y) * amounts.low
    }
}

/// How much of each damping filter is heard, 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Damping {
    /// High damping.
    high: f64,
    /// The low-pass's share of high damping (the shelf has the rest).
    low_pass: f64,
    /// Low damping.
    low: f64,
}

/// Declares [`Ramps`] from one list of fields, so the smoothers and their targets are paired by
/// name and cannot fall out of step.
macro_rules! ramps {
    ($($(#[$doc:meta])* $field:ident,)*) => {
        /// Values that ramp to their settings, so moving a control does not click:
        /// `Ramps<Smoother>` holds the smoothers and `Ramps<f64>` their targets.
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct Ramps<T> {
            $($(#[$doc])* $field: T,)*
        }

        impl<T: Copy> Ramps<T> {
            /// Every field set to `value`.
            fn splat(value: T) -> Self {
                Self { $($field: value,)* }
            }
        }

        impl Ramps<Smoother> {
            /// Calls `f` with each smoother and its target.
            fn each(&mut self, targets: &Ramps<f64>, mut f: impl FnMut(&mut Smoother, f32)) {
                $(f(&mut self.$field, targets.$field as f32);)*
            }
        }
    };
}

ramps! {
    /// Predelay (frames).
    predelay,
    /// Reflect (linear).
    reflect,
    /// Diffuse (linear).
    diffuse,
    /// Dry/Wet.
    mix,
    /// Stereo's mixing angle (radians).
    angle,
    /// Stereo's delay of the reflections (frames, [`stereo_delay`]).
    stereo_delay,
    /// Gain of the input into the network (0 while frozen with Cut on).
    feed,
    /// How much of Low Cut is heard (0 off, 1 on).
    low_cut,
    /// How much of High Cut is heard (0 off, 1 on).
    high_cut,
    /// Spin's swing, as a fraction of each reflection's whole delay before [`spin_reach`].
    spin,
    /// Chorus's swing (frames).
    chorus,
    /// Shape, read at the control rate.
    shape,
    /// Scale, read at the control rate.
    scale,
    /// The natural log of Decay, read at the control rate.
    decay,
    /// log2 of the input band's centre, read at the control rate.
    centre,
    /// The input band's width in octaves, read at the control rate.
    width,
    /// The diffusers' gain.
    diffusion,
    /// log2 of high damping's frequency, read at the control rate.
    high_frequency,
    /// High damping's gain, read at the control rate.
    high_gain,
    /// log2 of low damping's frequency, read at the control rate.
    low_frequency,
    /// Low damping's gain, read at the control rate.
    low_gain,
    /// How much high damping is heard (see [`Damping`]).
    high,
    /// The low-pass's share of high damping (see [`Damping`]).
    low_pass,
    /// How much low damping is heard (see [`Damping`]).
    low,
}

impl Ramps<Smoother> {
    /// Whether the values read at the control rate are still moving.
    fn control_moving(&self) -> bool {
        [
            &self.shape,
            &self.scale,
            &self.decay,
            &self.high_frequency,
            &self.high_gain,
            &self.low_frequency,
            &self.low_gain,
        ]
        .iter()
        .any(|s| !s.is_settled())
    }

    /// Whether the input band's centre or width is still moving.
    fn band_moving(&self) -> bool {
        !self.centre.is_settled() || !self.width.is_settled()
    }
}

/// The algorithmic room reverb: input filters, predelay, early reflections and a feedback delay
/// network.
pub struct Reverb {
    /// The sample rate it was prepared for.
    sample_rate: f64,
    /// Every parameter's value, in [`param`] order.
    params: ParamBank<{ param::COUNT }>,
    /// Values that ramp to their settings.
    ramps: Ramps<Smoother>,
    /// Low Cut: a high-pass and two shelves.
    low_cut: [FirstOrder; 3],
    /// The fixed roll-off of the lowest octaves at the input.
    rumble: FirstOrder,
    /// A second, lower high-pass of the lowest octave at the input.
    subsonic: FirstOrder,
    /// High Cut, a second-order low-pass.
    high_cut: Biquad,
    /// The low-pass the input passes while High Cut is off.
    top_cut: Biquad,
    /// The reflections' low-pass, for the filtered reflections of each side.
    reflection_filters: [Biquad; 2],
    /// The reflections of each side, for Stereo's delay of them ([`stereo_delay`]).
    stereo_lines: [DelayLine; 2],
    /// The direct reflections of the Density setting.
    taps: &'static [Tap],
    /// Their filtered reflections, family by family.
    filtered: &'static [[f64; DIRECT_TAPS]; FAMILY_COUNT],
    /// Each direct reflection's Spin LFO, one of [`SPIN_VOICES`].
    voices: [usize; DIRECT_TAPS],
    /// The Density whose reflections these are.
    reflections: usize,
    /// One line per family, holding the mix of direct reflections its filtered reflections play.
    family_lines: [DelayLine; FAMILY_COUNT],
    /// Which families the Density setting's reflections have.
    families_used: [bool; FAMILY_COUNT],
    /// The predelay line, which the taps and the network's entry read.
    input: DelayLine,
    /// The network's input groups, each the sum of its direct reflections as they were read
    /// (Spin's shift included), delayed by the Shape entry before the network takes them.
    group_lines: [DelayLine; network::MAX_GROUPS],
    /// The network.
    network: network::Network,
    /// The size factor `(size / 100)^(1/3)` as it glides.
    room: f64,
    /// The tuning at the last control point.
    from: Tuning,
    /// The tuning at the next control point.
    to: Tuning,
    /// The tuning in between, for this frame.
    now: Tuning,
    /// Whether `from` and `to` differ, so frames in between need `now`.
    moving: bool,
    /// Spin's LFO positions, in cycles (0..1), one per [`SPIN_VOICES`].
    spin_phases: [f64; SPIN_VOICES.len()],
    /// Chorus's LFO position, in cycles (0..1).
    chorus_phase: f64,
    /// Whether the transport ran in the last block: Spin's LFOs start again when it starts.
    playing: bool,
    /// Frames since the last reset, for the control-rate grid.
    frame: u64,
    /// Whether a parameter changed since the last control update.
    dirty: bool,
}

impl Default for Reverb {
    /// The same as [`Reverb::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// A level in decibels as a linear gain.
fn db_to_gain(db: f64) -> f64 {
    10.0f64.powf(db / 20.0)
}

/// How much later than at Stereo 120 the reflections are heard at `stereo` (frames at
/// `sample_rate`), under Smooth Fast or Slow when `fast` ([`STEREO_DELAY_SECONDS`]).
fn stereo_delay(stereo: f64, fast: bool, sample_rate: f64) -> f64 {
    let frames = STEREO_DELAY_SECONDS * sample_rate * (1.0 - stereo / 120.0);
    let frames = if fast { frames - STEREO_FLOOR_SLACK } else { frames };
    network::rounded(frames, fast, true).max(0.0)
}

/// How far Spin reaches with `predelay_ms` of predelay, as a share of [`SPIN_FRACTION`]: all of
/// it up to the knee (5 ms), then [`SPIN_REACH_MS`] over the predelay less
/// [`SPIN_REACH_OFFSET_MS`] (0.29 at 10 ms, 0.0081 at 250 ms).
fn spin_reach(predelay_ms: f64) -> f64 {
    SPIN_REACH_MS / (predelay_ms - SPIN_REACH_OFFSET_MS).max(SPIN_REACH_MS)
}

/// Each of `taps`' Spin LFO: the one of [`SPIN_VOICES`] at its time.
fn voices(taps: &[Tap]) -> [usize; DIRECT_TAPS] {
    let mut voices = [0; DIRECT_TAPS];
    for (voice, tap) in voices.iter_mut().zip(taps) {
        *voice = (0..SPIN_VOICES.len())
            .min_by(|&a, &b| (SPIN_VOICES[a].0 - tap.ms).abs().total_cmp(&(SPIN_VOICES[b].0 - tap.ms).abs()))
            .unwrap_or(0);
    }
    voices
}

/// Spin's LFO positions at the start, in cycles.
fn spin_start() -> [f64; SPIN_VOICES.len()] {
    SPIN_VOICES.map(|(_, _, degrees)| degrees / 360.0)
}

/// Soft limit for a frozen loop: unchanged up to [`FREEZE_LIMIT`], then compressed towards twice it.
#[inline]
fn limit(x: f64) -> f64 {
    let over = x.abs() - FREEZE_LIMIT;
    if over <= 0.0 { x } else { x.signum() * (FREEZE_LIMIT + over / (1.0 + over / FREEZE_LIMIT)) }
}

/// The Low Cut shelves' gains (dB) for lower edge `edge` (Hz).
fn low_cut_gains(edge: f64) -> [f64; 2] {
    let at = edge.max(1.0).ln();
    let [(first, lowest), .., (last, highest)] = LOW_CUT_GAINS;
    if edge <= first {
        return lowest;
    }
    if edge >= last {
        return highest;
    }
    for pair in LOW_CUT_GAINS.windows(2) {
        let ((e0, g0), (e1, g1)) = (pair[0], pair[1]);
        if edge <= e1 {
            let t = (at - e0.ln()) / (e1.ln() - e0.ln());
            return [g0[0] + (g1[0] - g0[0]) * t, g0[1] + (g1[1] - g0[1]) * t];
        }
    }
    highest
}

impl Reverb {
    /// A reverb with every parameter at its default, prepared for the default sample rate
    /// (allocates).
    pub fn new() -> Self {
        let sr = DEFAULT_SAMPLE_RATE;
        let mut reverb = Self {
            sample_rate: sr,
            params: ParamBank::new(&PARAMS),
            ramps: Ramps::splat(Smoother::new(0.0, 1)),
            low_cut: [FirstOrder::default(); 3],
            rumble: FirstOrder::default(),
            subsonic: FirstOrder::default(),
            high_cut: Biquad::default(),
            stereo_lines: Default::default(),
            top_cut: Biquad::default(),
            reflection_filters: Default::default(),
            taps: &TAPS_LOW,
            filtered: &FILTERED_LOW,
            voices: voices(&TAPS_LOW),
            reflections: LOW,
            family_lines: Default::default(),
            families_used: [false; FAMILY_COUNT],
            input: DelayLine::default(),
            group_lines: std::array::from_fn(|_| DelayLine::default()),
            network: network::Network::default(),
            room: 1.0,
            from: Tuning::default(),
            to: Tuning::default(),
            now: Tuning::default(),
            moving: false,
            spin_phases: spin_start(),
            playing: false,
            chorus_phase: 0.0,
            frame: 0,
            dirty: true,
        };
        reverb.allocate();
        reverb.rebuild();
        reverb
    }

    /// Allocates the lines for the current sample rate.
    fn allocate(&mut self) {
        let sr = self.sample_rate;
        self.input = DelayLine::for_seconds(PREDELAY_LINE_SECONDS, sr);
        self.stereo_lines = std::array::from_fn(|_| DelayLine::for_seconds(2.0 * STEREO_DELAY_SECONDS, sr));
        self.group_lines = std::array::from_fn(|_| DelayLine::for_seconds(ENTRY_LINE_SECONDS, sr));
        self.family_lines = std::array::from_fn(|_| DelayLine::for_seconds(FAMILY_LINE_SECONDS, sr));
        self.network = network::Network::new(sr);
    }

    /// The value of parameter `index`, 0.0 for an unknown one.
    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    /// Whether switch `index` is on.
    fn on(&self, index: usize) -> bool {
        self.params.on(index)
    }

    /// Whether Freeze is on.
    fn frozen(&self) -> bool {
        self.on(param::FREEZE)
    }

    /// Where each ramped value is heading, from the parameters.
    fn targets(&self) -> Ramps<f64> {
        let sr = self.sample_rate;
        let flag = |index| if self.on(index) { 1.0 } else { 0.0 };
        // Frozen with Flat on, the damping is left out; with Flat off it stays in, as the reference's frozen
        // tail goes on losing the bands the shelves take.
        let filtered = if self.frozen() && self.on(param::FLAT) { 0.0 } else { 1.0 };
        Ramps {
            predelay: self.value(param::PREDELAY) * sr,
            reflect: db_to_gain(self.value(param::REFLECT)),
            diffuse: db_to_gain(self.value(param::DIFFUSE)),
            mix: self.value(param::MIX),
            angle: std::f64::consts::FRAC_PI_4 * (1.0 - self.value(param::STEREO) / 120.0),
            // Smooth Slow renders as Fast (`reverb-network-fast`).
            stereo_delay: stereo_delay(self.value(param::STEREO), self.value(param::SMOOTH) as usize != 0, sr),
            feed: if self.frozen() && self.on(param::CUT) { 0.0 } else { 1.0 },
            low_cut: flag(param::LOW_CUT),
            high_cut: flag(param::HIGH_CUT),
            spin: flag(param::SPIN) * SPIN_FRACTION * self.value(param::SPIN_AMOUNT),
            chorus: flag(param::CHORUS) * CHORUS_SECONDS * self.value(param::CHORUS_AMOUNT) * sr,
            shape: self.value(param::SHAPE),
            scale: self.value(param::SCALE),
            decay: self.value(param::DECAY).max(0.01).ln(),
            centre: self.value(param::INPUT_FREQUENCY).max(1.0).log2(),
            width: self.value(param::INPUT_WIDTH),
            diffusion: self.value(param::DIFFUSION),
            high_frequency: self.value(param::HIGH_DAMPING_FREQUENCY).max(1.0).log2(),
            high_gain: self.value(param::HIGH_DAMPING_GAIN),
            low_frequency: self.value(param::LOW_DAMPING_FREQUENCY).max(1.0).log2(),
            low_gain: self.value(param::LOW_DAMPING_GAIN),
            high: flag(param::HIGH_DAMPING) * filtered,
            low_pass: flag(param::HIGH_DAMPING_TYPE),
            low: flag(param::LOW_DAMPING) * filtered,
        }
    }

    /// The size factor `(size / 100)^(1/3)` for the Size setting.
    fn room_target(&self) -> f64 {
        (self.value(param::SIZE) / REFERENCE_SIZE).cbrt()
    }

    /// Every state cleared, every ramped value at its setting.
    fn rebuild(&mut self) {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, self.sample_rate);
        let targets = self.targets();
        self.ramps.each(&targets, |smoother, target| {
            smoother.set_ramp(ramp);
            smoother.reset(target);
        });
        self.ramps.predelay.set_ramp(Smoother::ramp_frames(PREDELAY_RAMP_SECONDS, self.sample_rate));
        self.low_cut.iter_mut().for_each(FirstOrder::reset);
        self.rumble = FirstOrder::new(FirstOrderCoefficients::highpass(self.sample_rate, RUMBLE_FREQUENCY));
        self.subsonic = FirstOrder::new(FirstOrderCoefficients::highpass(self.sample_rate, SUBSONIC_FREQUENCY));
        self.high_cut.reset();
        for line in &mut self.stereo_lines {
            line.clear();
        }
        self.top_cut = Biquad::new(Coefficients::lowpass(self.sample_rate, TOP_CUT, BUTTERWORTH_Q));
        if TOP_CUT >= 0.45 * self.sample_rate {
            self.top_cut.set(Coefficients::IDENTITY);
        }
        for filter in &mut self.reflection_filters {
            filter.reset();
        }
        self.input.clear();
        for line in &mut self.group_lines {
            line.clear();
        }
        for line in &mut self.family_lines {
            line.clear();
        }
        self.set_reflections();
        self.network.set_design(network::design(self.value(param::DENSITY) as usize));
        self.network.clear();
        self.room = self.room_target();
        self.spin_phases = spin_start();
        self.chorus_phase = 0.0;
        self.frame = 0;
        self.tune_input();
        self.tune();
        self.from = self.to;
        self.now = self.to;
        self.moving = false;
        self.dirty = false;
    }

    /// Plays the Density setting's reflections.
    fn set_reflections(&mut self) {
        self.reflections = self.value(param::DENSITY) as usize;
        let density = if self.reflections < TAP_TABLES.len() { self.reflections } else { LOW };
        self.taps = TAP_TABLES[density];
        self.filtered = FILTERED_TABLES[density];
        self.voices = voices(self.taps);
        let used = self.filtered.map(|row| row.iter().any(|&g| g != 0.0));
        // Family lines that start again begin silent.
        for ((line, &now), was) in self.family_lines.iter_mut().zip(&used).zip(self.families_used) {
            if now && !was {
                line.clear();
            }
        }
        self.families_used = used;
    }

    /// The input filters from the band's (ramped) centre and width.
    fn tune_input(&mut self) {
        let sr = self.sample_rate;
        let centre = 2.0f64.powf(f64::from(self.ramps.centre.current()));
        let half = f64::from(self.ramps.width.current()) / 2.0;
        let (low, high) = (centre / 2.0f64.powf(half), centre * 2.0f64.powf(half));
        let identity = FirstOrderCoefficients::IDENTITY;
        let mut low_cut = [identity; 3];
        if low >= LOW_CUT_LOWEST {
            low_cut[0] = FirstOrderCoefficients::highpass(sr, low * LOW_CUT_RATIO);
            let gains = low_cut_gains(low);
            for ((section, corner), db) in low_cut[1..].iter_mut().zip(LOW_CUT_SHELVES).zip(gains) {
                let gain = db_to_gain(db);
                // The shelf is half way (in decibels) at the geometric mean of its zero and pole.
                *section = FirstOrderCoefficients::low_shelf(sr, corner * gain.sqrt(), gain);
            }
        }
        for (section, coefficients) in self.low_cut.iter_mut().zip(low_cut) {
            section.set(coefficients);
        }
        let high = high.min(TOP_CUT);
        self.high_cut.set(if high < sr * 0.45 {
            Coefficients::lowpass(sr, high, BUTTERWORTH_Q)
        } else {
            Coefficients::IDENTITY
        });
    }

    /// Delay lengths, tap levels, line gains and damping for the current size and settings, into
    /// `to`.
    fn tune(&mut self) {
        let sr = self.sample_rate;
        let room = self.room;
        let ms = sr / 1_000.0;
        let shape = f64::from(self.ramps.shape.current());
        let scale = f64::from(self.ramps.scale.current());
        let setting = f64::from(self.ramps.decay.current()).exp();
        let fade = (TAP_FADE + TAP_FADE_PER_SHAPE * shape) / setting;
        let predelay_ms = self.value(param::PREDELAY) * 1_000.0;
        let frozen = self.frozen();
        let decay = setting * DECAY_RATIO;
        // Once nothing moves, the lines are whole frames long, so a line without chorus is read
        // without interpolation, which would take away highs on every trip. While Size or Decay
        // glide they move smoothly between frames.
        let settled = self.room == self.room_target() && !self.ramps.control_moving();
        let density = self.value(param::DENSITY) as usize;
        let predelay_gain = db_to_gain(-PREDELAY_FADE * predelay_ms / setting);
        // Smooth Slow renders as Fast (`reverb-network-fast`).
        let fast = self.value(param::SMOOTH) as usize != 0;
        let size = REFERENCE_SIZE * room.powi(3);
        let size_level = SIZE_LEVEL.0 + SIZE_LEVEL.1 * size;
        // The network is fed by the direct reflections, each at its own level and fade.
        let norm = self.taps.iter().map(|tap| tap.gain * tap.gain).sum::<f64>().sqrt().max(1e-9);
        let level = TAP_GAIN * predelay_gain * size_level;
        let t = &mut self.to;
        t.tap_gains = [0.0; DIRECT_TAPS];
        t.reflection_levels = [0.0; DIRECT_TAPS];
        t.feed_gains = [0.0; DIRECT_TAPS];
        for ((((delay, gain), reflection), feed), tap) in t
            .tap_delays
            .iter_mut()
            .zip(&mut t.tap_gains)
            .zip(&mut t.reflection_levels)
            .zip(&mut t.feed_gains)
            .zip(self.taps)
        {
            let time = tap.ms * room;
            *delay = time * ms + TAP_OFFSET_FRAMES;
            let faded = db_to_gain(-fade * time);
            *reflection = level * faded;
            *gain = tap.gain * *reflection;
            *feed = tap.gain * faded / norm;
        }
        // A family's filtered reflections peak where the network hears its copy, in the reference's 8-frame
        // blocks or under Smooth Fast in whole frames, and fade over that delay as the reflections do.
        for (family, (delay, gain)) in t.family_delays.iter_mut().zip(&mut t.family_gains).enumerate() {
            let frames = network::front(family, room, settled, fast, sr);
            *delay = if settled { frames.round() } else { frames }.max(2.0);
            *gain = db_to_gain(-fade * network::peak(family, room, false, false) * 1_000.0 / REFERENCE_RATE);
        }
        let gain = |seconds: f64| {
            if frozen {
                db_to_gain(-seconds * if self.params.on(param::FLAT) { 0.0 } else { FREEZE_DRAIN_UNFLAT })
            } else {
                db_to_gain(-60.0 * seconds / decay)
            }
        };
        let design = network::design(density);
        t.network.tune(design, room, scale, sr, settled, fast, gain, LOSS_TRIP_SECONDS);
        t.entry = network::entry(shape, settled, fast, sr);
        // The network's lines scale with the room's own size factor, small rooms too, and its input
        // with the reflections' level and its own ([`NETWORK_SIZE_LEVEL`]). Every path through it
        // fades with its whole time, so it needs no cut for short decays.
        let (a, b, c) = NETWORK_SIZE_LEVEL;
        let network_level = a + b * size + c * size * size;
        t.network_feed = NETWORK_GAIN * predelay_gain * size_level * network_level;
        // The families' copies are their filtered reflections at the network's level.
        t.copy_gain = NETWORK_GAIN * network_level / (HEARD_COPY * TAP_GAIN);
        let corner = reflection_corner(setting, density, room).clamp(20.0, 0.45 * sr);
        let reflection = Coefficients::lowpass(sr, corner, REFLECTION_Q);
        for filter in &mut self.reflection_filters {
            filter.set(reflection);
        }
        self.network.set_copy_filter(reflection);

        // Each line passes the shelves, with a gain per trip as long as the line, or one whole
        // one-pole of the Low-Pass on every pass, however long the line (`reverb-network-filters`:
        // at 250 Hz, 1 kHz and 4 kHz and Size 1.53 to 100 that alone matches the reference's renders to −27
        // to −37 dB). The input passes no damping: in the reference's network each line has its own, and the
        // input none (`reverb-network-small hi-shelf-4k`, `reverb-network-filters`).
        let r = &self.ramps;
        let (high_f, high_g) = (2.0f64.powf(f64::from(r.high_frequency.current())), f64::from(r.high_gain.current()));
        let (low_f, low_g) = (2.0f64.powf(f64::from(r.low_frequency.current())), f64::from(r.low_gain.current()));
        let line_pole = (-std::f64::consts::TAU * high_f / sr).exp();
        let line_pass = FirstOrderCoefficients { b0: 1.0 - line_pole, b1: 0.0, a1: -line_pole };
        for (line, &trips) in self.network.lines.iter_mut().zip(&self.to.network.trips) {
            let db = |g: f64| 20.0 * g.max(1e-6).log10() * trips;
            let damper = &mut line.damper;
            damper.high_shelf.set(Coefficients::high_shelf(sr, high_f * SHELF_SHIFT, SHELF_Q, db(high_g)));
            damper.low_shelf.set(Coefficients::low_shelf(sr, low_f / SHELF_SHIFT, SHELF_Q, db(low_g)));
            damper.low_pass.set(line_pass);
        }
    }

    /// The longest delay Size moves, a line or a direct reflection, in a room of size factor
    /// `room` (frames).
    fn longest_delay(&self, room: f64) -> f64 {
        let tap = self.taps.iter().map(|tap| tap.ms).fold(0.0, f64::max) * room * self.sample_rate / 1_000.0;
        let design = network::design(self.value(param::DENSITY) as usize);
        tap.max(network::Lengths::longest(design, room, self.sample_rate))
    }

    /// Size moves toward its setting, as fast as Smooth says, over one control period.
    fn glide_room(&mut self) {
        let target = self.room_target();
        let seconds = match self.value(param::SMOOTH) as usize {
            1 => SMOOTH_SLOW_SECONDS,
            2 => SMOOTH_FAST_SECONDS,
            _ => 0.0,
        };
        let frames = seconds * self.sample_rate;
        let keep = if frames > 1.0 { (-(CONTROL_FRAMES as f64) / frames).exp() } else { 0.0 };
        let mut room = target + (self.room - target) * keep;
        let moved = (self.longest_delay(room) - self.longest_delay(self.room)).abs();
        let most = GLIDE_SLEW * CONTROL_FRAMES as f64;
        if frames > 1.0 && moved > most {
            room = self.room + (room - self.room) * most / moved;
        }
        let room = if (room - target).abs() < 1e-6 { target } else { room };
        if room != self.room {
            self.room = room;
            self.dirty = true;
        }
    }

    /// At a control point: what the next stretch of frames glides to.
    fn control(&mut self) {
        self.from = self.to;
        self.glide_room();
        if self.ramps.band_moving() {
            self.tune_input();
        }
        let moving = self.ramps.control_moving();
        if self.dirty || moving {
            self.tune();
            // One more update once the ramps stop, so the lines settle on whole frames.
            self.dirty = moving;
        }
        self.moving = self.from != self.to;
    }
}

impl Device for Reverb {
    /// The reverb's [`INFO`].
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    /// Allocates the lines for `sample_rate` and starts silent.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        self.sample_rate = check_sample_rate(sample_rate)?;
        self.allocate();
        self.rebuild();
        Ok(())
    }

    /// Silences the reverb and settles every ramped value at its setting.
    fn reset(&mut self) {
        self.rebuild();
    }

    /// Sets parameter `index`, clamped to its range; the ramped values move to it.
    fn set_param(&mut self, index: usize, value: f64) {
        if self.params.set(index, value).is_none() {
            return;
        }
        let targets = self.targets();
        self.ramps.each(&targets, |smoother, target| smoother.set_target(target));
        self.dirty = true;
    }

    /// The value of parameter `index`, or `None` for an unknown one.
    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    /// Runs interleaved stereo `buffer` through the reverb in place.
    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        let sr = self.sample_rate;
        // The reference's Spin LFOs start from the same phases whenever playback starts.
        if context.playing && !self.playing {
            self.spin_phases = spin_start();
        }
        self.playing = context.playing;
        if self.reflections != self.value(param::DENSITY) as usize {
            self.set_reflections();
        }
        let design = network::design(self.value(param::DENSITY) as usize);
        // A network that takes over from another starts silent.
        self.network.set_design(design);
        let (spin_step, chorus_step) =
            (self.value(param::SPIN_RATE).max(SPIN_RATE_FLOOR) / sr, self.value(param::CHORUS_RATE) / sr);
        // How far ahead of their phases Spin's LFOs are read, in cycles at Spin Rate.
        let spin_lead = SPIN_LEAD_SECONDS * sr * spin_step;
        let frozen = self.frozen();
        // Stereo's rotation and the mix's gains, worked out again only when their inputs move
        // (NaN never equals an input, so the first frame works them out).
        let mut output_key = (f64::NAN, f64::NAN);
        let (mut rotation, mut gains) = ((0.0, 0.0), (0.0, 0.0));

        for frame in buffer.as_chunks_mut::<2>().0 {
            let step = self.frame % CONTROL_FRAMES;
            if step == 0 {
                self.control();
            }
            if self.moving {
                self.now.between(&self.from, &self.to, (step + 1) as f64 / CONTROL_FRAMES as f64);
            }
            let r = &mut self.ramps;
            let stereo_settled = r.stereo_delay.is_settled();
            let [
                predelay,
                reflect,
                diffuse,
                mix,
                angle,
                stereo_delay,
                feed,
                low_cut,
                high_cut,
                spin_swing,
                chorus_swing,
            ] = [
                &mut r.predelay,
                &mut r.reflect,
                &mut r.diffuse,
                &mut r.mix,
                &mut r.angle,
                &mut r.stereo_delay,
                &mut r.feed,
                &mut r.low_cut,
                &mut r.high_cut,
                &mut r.spin,
                &mut r.chorus,
            ]
            .map(|s| f64::from(s.next_value()));
            for s in [
                &mut r.shape,
                &mut r.scale,
                &mut r.decay,
                &mut r.centre,
                &mut r.width,
                &mut r.high_frequency,
                &mut r.high_gain,
                &mut r.low_frequency,
                &mut r.low_gain,
            ] {
                s.next_value();
            }
            let diffusion = r.diffusion.next_value();
            let damping = Damping {
                high: f64::from(r.high.next_value()),
                low_pass: f64::from(r.low_pass.next_value()),
                low: f64::from(r.low.next_value()),
            };

            let input = frame.map(|s| if s.is_finite() { f64::from(s) } else { 0.0 });
            // Both cuts always run, so switching one crossfades instead of clicking; High Cut
            // takes the place of the fixed top cut.
            let mut x = self.subsonic.process(self.rumble.process((input[0] + input[1]) / 2.0));
            let cut = self.low_cut.iter_mut().fold(x, |y, section| section.process(y));
            x += (cut - x) * low_cut;
            let top = self.top_cut.process(x);
            let cut = self.high_cut.process(x);
            x = top + (cut - top) * high_cut;
            let t = if self.moving { &self.now } else { &self.to };

            // Early reflections and the network's input, read from the predelay line. Spin swings
            // each direct reflection's whole delay on its own LFO, by a share of it that falls as
            // the predelay grows, and the network's input with it; a filtered one is a direct one
            // a family's delay later, so it is played from a mix of the direct ones per family.
            // With nothing moving, each tap's own delay falls on a whole frame and Spin's swing is
            // added to it, as the reference's are (`reverb-spin-taps`: its swinging taps centre on the
            // frames its still ones are read at); without Spin the taps are read at whole frames.
            let mut spin = [0.0f64; SPIN_VOICES.len()];
            if spin_swing > 0.0 {
                let reach = spin_swing * spin_reach(predelay * 1_000.0 / sr);
                for ((s, phase), (_, rate, _)) in spin.iter_mut().zip(&self.spin_phases).zip(SPIN_VOICES) {
                    *s = reach * (std::f64::consts::TAU * (phase + spin_lead * rate)).sin();
                }
            }
            let settled = !self.moving && self.ramps.predelay.is_settled();
            let whole = spin_swing == 0.0 && settled;
            // The groups wait the entry in their own lines, at least two frames; an entry shorter
            // than that (the reference's is a frame early at Shape 0) reads the feed line sooner instead.
            let group_delay = t.entry.max(2.0);
            let group_whole = group_delay.round();
            // Spin's reflections are read between frames by linear interpolation, as the reference's: on
            // white noise the reference's lose 0.4 dB at 6 kHz to 1.6 at 15 kHz as soon as Spin moves them,
            // at any speed, which is linear's loss against a cubic read within 0.2 dB
            // (`reverb-texture` spin-*, `reverb-spin-grid`; their static reflections match).
            let read_from = |line: &DelayLine, at: f64| {
                f64::from(if whole {
                    line.read(at.round() as usize)
                } else if spin_swing > 0.0 {
                    line.read_linear(at)
                } else {
                    line.read_hermite(at)
                })
            };
            // Direct reflections, left and right; each also feeds its family mixes and the
            // network's group it belongs to.
            let mut direct = [0.0f64; 2];
            let mut groups = [0.0f64; network::MAX_GROUPS];
            let mut reads = [0.0f64; DIRECT_TAPS];
            for (d, tap) in self.taps.iter().enumerate() {
                let delay = t.tap_delays[d];
                let still = predelay + delay;
                let at = if settled { still.round() } else { still } + spin[self.voices[d]] * still;
                let read = read_from(&self.input, at);
                direct[tap.side] += read * t.tap_gains[d];
                reads[d] = read * t.reflection_levels[d];
                // Read where the reflection is, and delayed by the entry below, so the network
                // hears the shift Spin gave the reflection, not the one it has an entry later. At
                // whole frames each is rounded on its own, the reflection where it is heard and the
                // entry, as the reference's are (`reverb-network-small`).
                let from = at + if settled { t.entry.round() - group_whole } else { t.entry - group_delay };
                let u = read_from(&self.input, from) * t.feed_gains[d] * feed * t.network_feed;
                if let Some(group) = design.tap_groups.get(d).and_then(|&g| groups.get_mut(g)) {
                    *group += u;
                }
            }
            // Each family line is heard twice: as the family's filtered reflections, and an entry
            // later as its copy in the network.
            let mut filtered = [0.0f64; 2];
            let mut copies = [0.0f64; FAMILY_COUNT];
            for (family, line) in self.family_lines.iter_mut().enumerate() {
                if !self.families_used[family] {
                    continue;
                }
                let read = |delay: f64| {
                    f64::from(if self.moving { line.read_hermite(delay) } else { line.read(delay.round() as usize) })
                };
                let delay = t.family_delays[family];
                filtered[FAMILY_SIDES[family]] += read(delay);
                copies[family] = read(delay + t.entry) * t.copy_gain;
                let mix: f64 = self.filtered[family].iter().zip(&reads).map(|(g, r)| g * r).sum();
                // Cut keeps new input out here, so what is on its way into the network still gets there
                // (the reflections heard are cut at the output).
                line.push((mix * t.family_gains[family] * feed) as f32);
            }
            let [left_filter, right_filter] = &mut self.reflection_filters;
            let early = [direct[0] + left_filter.process(filtered[0]), direct[1] + right_filter.process(filtered[1])];
            // Stereo's delay, on the reflections heard; the network took them as they were read.
            let mut delayed = [0.0f64; 2];
            for ((out, line), x) in delayed.iter_mut().zip(&mut self.stereo_lines).zip(early) {
                line.push(x as f32);
                *out = f64::from(if stereo_settled {
                    line.read(stereo_delay.round() as usize + 1)
                } else {
                    line.read_linear(stereo_delay + 1.0)
                });
            }
            let early = delayed;
            for (group, line) in groups.iter_mut().zip(&mut self.group_lines) {
                let late = if settled { line.read(group_whole as usize) } else { line.read_hermite(group_delay) };
                line.push(*group as f32);
                *group = f64::from(late);
            }

            // The network. Without Chorus the swing is zero whatever the phase.
            let (chorus_sin, chorus_cos) =
                if chorus_swing > 0.0 { (std::f64::consts::TAU * self.chorus_phase).sin_cos() } else { (0.0, 0.0) };
            let swing =
                |(c, s): (f64, f64), length: f64| chorus_swing.min(0.5 * length) * (chorus_sin * c + chorus_cos * s);
            let tail = self.network.process(&groups, &copies, &t.network, diffusion, &damping, swing, frozen);
            self.input.push(x as f32);

            // Output: the two parts, Stereo's rotation, then the mix.
            // Cut keeps the input out of the reflections as well as the network (the reference's render is
            // silent when it is frozen from the first sample: `reverb-freeze-static`).
            let reflect = reflect * feed;
            let wet = [reflect * early[0] + diffuse * tail[0], reflect * early[1] + diffuse * tail[1]];
            if (angle, mix) != output_key {
                output_key = (angle, mix);
                rotation = angle.sin_cos();
                let (wet_gain, dry_gain) = (mix * std::f64::consts::FRAC_PI_2).sin_cos();
                gains = (dry_gain, wet_gain);
            }
            let ((sin, cos), (dry_gain, wet_gain)) = (rotation, gains);
            let wet = [cos * wet[0] + sin * wet[1], sin * wet[0] + cos * wet[1]];
            for ((sample, dry), wet) in frame.iter_mut().zip(input).zip(wet) {
                *sample = (dry * dry_gain + wet * wet_gain) as f32;
            }

            for (phase, (_, rate, _)) in self.spin_phases.iter_mut().zip(SPIN_VOICES) {
                *phase = (*phase + spin_step * rate).fract();
            }
            self.chorus_phase = (self.chorus_phase + chorus_step).fract();
            self.frame = self.frame.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;
