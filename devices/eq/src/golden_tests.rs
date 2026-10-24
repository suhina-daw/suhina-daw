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

/// Band `band` (from 1) of curve `curve` (0 for A): `field` set to `value`.
const fn at(curve: usize, band: usize, field: usize, value: f64) -> (usize, f64) {
    (param::band(curve, band - 1, field), value)
}

/// Curve A's default bands switched off, so a scenario hears only the bands it sets.
const QUIET: [(usize, f64); 4] =
    [at(0, 1, param::ON, 0.0), at(0, 2, param::ON, 0.0), at(0, 3, param::ON, 0.0), at(0, 4, param::ON, 0.0)];

const SCENARIOS: &[Scenario] = &[
    // Every control at its default: four bands on, none with gain, so the noise passes.
    Scenario {
        name: "defaults",
        sample_rate: 48_000.0,
        seconds: 0.2,
        keep: 12,
        input: noise_bed,
        settings: &[],
        changes: &[],
    },
    Scenario {
        name: "every-type",
        sample_rate: 44_100.0,
        seconds: 0.2,
        keep: 9,
        input: noise_bed,
        settings: &[
            QUIET[0],
            QUIET[1],
            QUIET[2],
            QUIET[3],
            at(0, 1, param::ON, 1.0),
            at(0, 1, param::TYPE, 0.0),
            at(0, 1, param::FREQUENCY, 60.0),
            at(0, 2, param::ON, 1.0),
            at(0, 2, param::TYPE, 1.0),
            at(0, 2, param::FREQUENCY, 120.0),
            at(0, 2, param::Q, 3.0),
            at(0, 3, param::ON, 1.0),
            at(0, 3, param::TYPE, 2.0),
            at(0, 3, param::FREQUENCY, 250.0),
            at(0, 3, param::GAIN, 6.0),
            at(0, 4, param::ON, 1.0),
            at(0, 4, param::TYPE, 3.0),
            at(0, 4, param::FREQUENCY, 900.0),
            at(0, 4, param::GAIN, -9.0),
            at(0, 5, param::ON, 1.0),
            at(0, 5, param::TYPE, 4.0),
            at(0, 5, param::FREQUENCY, 2_500.0),
            at(0, 5, param::Q, 4.0),
            at(0, 6, param::ON, 1.0),
            at(0, 6, param::TYPE, 5.0),
            at(0, 6, param::FREQUENCY, 6_000.0),
            at(0, 6, param::GAIN, 4.0),
            at(0, 7, param::ON, 1.0),
            at(0, 7, param::TYPE, 6.0),
            at(0, 7, param::FREQUENCY, 14_000.0),
            at(0, 7, param::Q, 2.0),
            at(0, 8, param::ON, 1.0),
            at(0, 8, param::TYPE, 7.0),
            at(0, 8, param::FREQUENCY, 18_000.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "adaptive-q-and-scale",
        sample_rate: 16_000.0,
        seconds: 0.2,
        keep: 4,
        input: noise_bed,
        settings: &[
            at(0, 2, param::GAIN, 12.0),
            at(0, 3, param::GAIN, -4.0),
            at(0, 4, param::GAIN, 8.0),
            (param::SCALE, -1.5),
        ],
        changes: &[],
    },
    Scenario {
        name: "left-right",
        sample_rate: 16_000.0,
        seconds: 0.2,
        keep: 4,
        input: noise_bed,
        settings: &[
            at(0, 3, param::GAIN, 9.0),
            at(1, 3, param::GAIN, -9.0),
            at(1, 6, param::GAIN, 6.0),
            (param::MODE, 1.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "mid-side",
        sample_rate: 16_000.0,
        seconds: 0.2,
        keep: 4,
        input: noise_bed,
        settings: &[at(0, 3, param::GAIN, 6.0), at(1, 4, param::GAIN, 12.0), (param::MODE, 2.0), (param::OUTPUT, -6.0)],
        changes: &[],
    },
    // Changes while playing: a frequency sweep, a type change, a band off, the mode.
    Scenario {
        name: "changes",
        sample_rate: 16_000.0,
        seconds: 0.5,
        keep: 4,
        input: noise_bed,
        settings: &[at(0, 3, param::GAIN, 10.0), at(0, 3, param::Q, 4.0)],
        changes: &[
            (0.1, param::band(0, 2, param::FREQUENCY), 4_000.0),
            (0.2, param::band(0, 2, param::TYPE), 7.0),
            (0.3, param::band(0, 1, param::ON), 0.0),
            (0.4, param::MODE, 2.0),
        ],
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

/// Noise at a quarter of full scale, different on each side.
fn noise_bed(_sample_rate: f64, frame: usize) -> [f32; 2] {
    [0.25 * noise(frame, 1), 0.25 * noise(frame, 2)]
}

/// The scenario rendered, interleaved, every `keep`-th frame.
fn render(s: &Scenario) -> Vec<f32> {
    let mut d = Eq::new();
    for &(index, value) in s.settings {
        d.set_param(index, value);
    }
    d.prepare(s.sample_rate).unwrap();
    let frames = (s.seconds * s.sample_rate) as usize;
    let mut buffer: Vec<f32> = (0..frames).flat_map(|n| (s.input)(s.sample_rate, n)).collect();
    let changes: Vec<(usize, usize, f64)> =
        s.changes.iter().map(|&(at, index, value)| ((at * s.sample_rate) as usize, index, value)).collect();
    let mut start = 0;
    while start < frames {
        for &(_, index, value) in changes.iter().filter(|c| c.0 == start) {
            d.set_param(index, value);
        }
        let next_change = changes.iter().map(|c| c.0).filter(|&at| at > start).min().unwrap_or(frames);
        let end = (start + BLOCK).min(next_change).min(frames);
        d.process(&ProcessContext::default(), &mut buffer[start * 2..end * 2]);
        start = end;
    }
    buffer.as_chunks::<2>().0.iter().step_by(s.keep).flatten().copied().collect()
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[test]
fn renders_like_the_golden_files() {
    let bless = std::env::var_os("SUHINA_BLESS").is_some();
    let mut failures = Vec::new();
    for s in SCENARIOS {
        let ours = render(s);
        assert!(ours.iter().all(|v| v.is_finite()), "{}: not finite", s.name);
        assert!(ours.iter().any(|v| v.abs() > 0.01), "{} is silent", s.name);
        let path = golden_dir().join(format!("{}.f32", s.name));
        if bless {
            std::fs::write(&path, ours.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}; run with SUHINA_BLESS=1", path.display()));
        let theirs: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
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
