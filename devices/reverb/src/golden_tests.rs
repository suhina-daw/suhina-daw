// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden renders: every scenario below is rendered and compared with its file in
//! `tests/golden` (see the README there).

use std::path::{Path, PathBuf};

use super::*;

/// Largest difference from a golden file. Above the last bits of an f32, so the libm of another
/// platform (sin, exp, pow) does not fail it; far below anything a change to the sound gives.
const TOLERANCE: f32 = 1e-6;
/// Frames per block, as a host gives them; a change splits the block it falls in.
const BLOCK: usize = 256;

/// The stereo signal fed in: a function of the sample rate and the frame.
type Input = fn(f64, usize) -> [f32; 2];

struct Scenario {
    name: &'static str,
    sample_rate: f64,
    seconds: f64,
    /// Every n-th frame is kept in the file, both sides.
    keep: usize,
    input: Input,
    /// Set before `prepare`.
    settings: &'static [(usize, f64)],
    /// (seconds, parameter, value), set while playing, in time order.
    changes: &'static [(f64, usize, f64)],
}

const SCENARIOS: &[Scenario] = &[
    // Every control at its default: chorus and spin moving, flat, cut, a little more than half wet.
    Scenario {
        name: "defaults",
        sample_rate: 48_000.0,
        seconds: 2.0,
        keep: 24,
        input: burst,
        settings: &[],
        changes: &[],
    },
    Scenario {
        name: "still",
        sample_rate: 16_000.0,
        seconds: 2.0,
        keep: 8,
        input: burst,
        settings: &[(param::CHORUS, 0.0), (param::SPIN, 0.0), (param::MIX, 1.0)],
        changes: &[],
    },
    Scenario {
        name: "dense-long",
        sample_rate: 44_100.0,
        seconds: 2.0,
        keep: 24,
        input: burst,
        settings: &[
            (param::DENSITY, 3.0),
            (param::DECAY, 4.0),
            (param::SIZE, 200.0),
            (param::DIFFUSION, 0.9),
            (param::PREDELAY, 0.05),
            (param::MIX, 1.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "sparse-damped",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: burst,
        settings: &[
            (param::DENSITY, 0.0),
            (param::DECAY, 0.5),
            (param::SIZE, 30.0),
            (param::HIGH_DAMPING_TYPE, 1.0),
            (param::HIGH_DAMPING_FREQUENCY, 2_000.0),
            (param::HIGH_DAMPING_GAIN, 0.4),
            (param::LOW_DAMPING, 1.0),
            (param::LOW_DAMPING_FREQUENCY, 300.0),
            (param::LOW_DAMPING_GAIN, 0.5),
            (param::LOW_CUT, 1.0),
            (param::HIGH_CUT, 0.0),
            (param::MIX, 1.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "chorus-spin",
        sample_rate: 16_000.0,
        seconds: 2.0,
        keep: 8,
        input: burst,
        settings: &[
            (param::CHORUS_RATE, 2.0),
            (param::CHORUS_AMOUNT, 1.5),
            (param::SPIN_RATE, 1.0),
            (param::SPIN_AMOUNT, 40.0),
            (param::SHAPE, 0.2),
            (param::STEREO, 120.0),
            (param::MIX, 1.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "reflect-diffuse",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: burst,
        settings: &[
            (param::REFLECT, 4.0),
            (param::DIFFUSE, -12.0),
            (param::SCALE, 0.9),
            (param::STEREO, 30.0),
            (param::FLAT, 0.0),
            (param::MIX, 0.4),
        ],
        changes: &[],
    },
    // Frozen while the tone goes on: Cut keeps it out of the held tail.
    Scenario {
        name: "freeze-cut",
        sample_rate: 16_000.0,
        seconds: 1.5,
        keep: 6,
        input: tone,
        settings: &[(param::MIX, 1.0)],
        changes: &[(0.4, param::FREEZE, 1.0), (1.1, param::FREEZE, 0.0)],
    },
    // Without Cut and Flat the tone keeps feeding the frozen loop, held in by the limit.
    Scenario {
        name: "freeze-open",
        sample_rate: 16_000.0,
        seconds: 1.5,
        keep: 6,
        input: tone,
        settings: &[(param::CUT, 0.0), (param::FLAT, 0.0), (param::MIX, 1.0)],
        changes: &[(0.4, param::FREEZE, 1.0), (1.1, param::FREEZE, 0.0)],
    },
    // The size glides (Smooth) while the tone plays; the predelay glides too.
    Scenario {
        name: "size-change",
        sample_rate: 16_000.0,
        seconds: 1.5,
        keep: 6,
        input: tone,
        settings: &[(param::SIZE, 50.0), (param::SMOOTH, 1.0), (param::MIX, 1.0)],
        changes: &[(0.5, param::SIZE, 300.0), (0.5, param::PREDELAY, 0.1), (1.0, param::DENSITY, 2.0)],
    },
];

/// Repeatable white noise from -1.0 to 1.0 (xorshift on the frame and side), the same on every
/// platform.
fn noise(frame: usize, side: u64) -> f32 {
    let mut x = (frame as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ side.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 31;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 29;
    ((x >> 40) as f32 / (1u64 << 23) as f32) - 1.0
}

/// 50 ms of noise fading out, different on each side, then silence.
fn burst(sample_rate: f64, frame: usize) -> [f32; 2] {
    let length = (0.05 * sample_rate) as usize;
    if frame >= length {
        return [0.0; 2];
    }
    let level = 0.5 * (1.0 - frame as f32 / length as f32);
    [noise(frame, 1) * level, noise(frame, 2) * level]
}

/// 330 Hz on the left and 495 Hz on the right, all the way through.
fn tone(sample_rate: f64, frame: usize) -> [f32; 2] {
    let t = frame as f64 / sample_rate;
    let side = |hz: f64| (0.4 * (std::f64::consts::TAU * hz * t).sin()) as f32;
    [side(330.0), side(495.0)]
}

/// The scenario rendered, interleaved, every `keep`-th frame.
fn render(s: &Scenario) -> Vec<f32> {
    let mut r = Reverb::new();
    for &(index, value) in s.settings {
        r.set_param(index, value);
    }
    r.prepare(s.sample_rate).unwrap();
    let frames = (s.seconds * s.sample_rate) as usize;
    let mut buffer: Vec<f32> = (0..frames).flat_map(|n| (s.input)(s.sample_rate, n)).collect();
    let changes: Vec<(usize, usize, f64)> =
        s.changes.iter().map(|&(at, index, value)| ((at * s.sample_rate) as usize, index, value)).collect();
    let mut start = 0;
    while start < frames {
        for &(_, index, value) in changes.iter().filter(|c| c.0 == start) {
            r.set_param(index, value);
        }
        let next_change = changes.iter().map(|c| c.0).filter(|&at| at > start).min().unwrap_or(frames);
        let end = (start + BLOCK).min(next_change).min(frames);
        r.process(&ProcessContext::default(), &mut buffer[start * 2..end * 2]);
        start = end;
    }
    buffer.as_chunks::<2>().0.iter().step_by(s.keep).flatten().copied().collect()
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[test]
fn renders_like_the_golden_files() {
    let mut failures = Vec::new();
    for s in SCENARIOS {
        let ours = render(s);
        assert!(ours.iter().all(|v| v.is_finite()), "{}: not finite", s.name);
        assert!(ours.iter().any(|v| v.abs() > 0.01), "{} is silent", s.name);
        let path = golden_dir().join(format!("{}.f32", s.name));
        let Some(theirs) = suhina_test_support::golden_f32(&path, &ours) else { continue };
        if theirs.len() != ours.len() {
            failures.push(format!("{}: {} samples, the golden file has {}", s.name, ours.len(), theirs.len()));
            continue;
        }
        let (at, worst) = ours
            .iter()
            .zip(&theirs)
            .map(|(a, b)| (a - b).abs())
            .enumerate()
            .fold((0, 0.0f32), |m, d| if d.1 > m.1 { d } else { m });
        if worst > TOLERANCE {
            let frame = at / 2 * s.keep;
            failures.push(format!(
                "{}: differs by {worst} at frame {frame} ({} side)",
                s.name,
                ["left", "right"][at % 2]
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn every_scenario_has_its_own_file() {
    let mut names: Vec<_> = SCENARIOS.iter().map(|s| s.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SCENARIOS.len());
    let mut files: Vec<String> = std::fs::read_dir(golden_dir())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".f32").map(str::to_owned))
        .collect();
    files.sort_unstable();
    assert_eq!(files, names, "a golden file without its scenario, or the other way round");
}
