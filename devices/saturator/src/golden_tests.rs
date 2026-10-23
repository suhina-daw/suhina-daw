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

/// A curve driven 12 dB into a tone.
const fn curve(shape: f64) -> [(usize, f64); 2] {
    [(param::TYPE, shape), (param::DRIVE, 12.0)]
}

const SCENARIOS: &[Scenario] = &[
    // Every control at its default: Analog Clip, a tone peaking at -8 dBFS, so it stays clean.
    Scenario {
        name: "defaults",
        sample_rate: 48_000.0,
        seconds: 0.3,
        keep: 12,
        input: tone,
        settings: &[],
        changes: &[],
    },
    Scenario {
        name: "analog-clip",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &curve(0.0),
        changes: &[],
    },
    Scenario {
        name: "soft-sine",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &curve(1.0),
        changes: &[],
    },
    Scenario {
        name: "bass-shaper",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &[(param::TYPE, 2.0), (param::DRIVE, 12.0), (param::THRESHOLD, -20.0)],
        changes: &[],
    },
    Scenario {
        name: "medium-curve",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &curve(3.0),
        changes: &[],
    },
    Scenario {
        name: "hard-curve",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &curve(4.0),
        changes: &[],
    },
    Scenario {
        name: "sinoid-fold",
        sample_rate: 44_100.0,
        seconds: 0.3,
        keep: 9,
        input: tone,
        settings: &curve(5.0),
        changes: &[],
    },
    Scenario {
        name: "digital-clip",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &curve(6.0),
        changes: &[],
    },
    Scenario {
        name: "waveshaper",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &[
            (param::TYPE, 7.0),
            (param::DRIVE, 6.0),
            (param::SHAPER_CURVE, 0.6),
            (param::SHAPER_DEPTH, 0.4),
            (param::SHAPER_LINEAR, 0.7),
            (param::SHAPER_DAMP, 0.2),
            (param::SHAPER_PERIOD, 0.3),
            (param::SHAPER_DRIVE, 0.8),
        ],
        changes: &[],
    },
    // Color on noise into a hard clip: the low shelf cut, the bell boosted at 2 kHz.
    Scenario {
        name: "color",
        sample_rate: 44_100.0,
        seconds: 0.3,
        keep: 9,
        input: noise_bed,
        settings: &[
            (param::TYPE, 6.0),
            (param::DRIVE, 12.0),
            (param::COLOR_LOW, -0.7),
            (param::COLOR_HIGH, 0.8),
            (param::COLOR_FREQUENCY, 2_000.0),
            (param::COLOR_WIDTH, 0.5),
        ],
        changes: &[],
    },
    Scenario {
        name: "post-clip-soft",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &[
            (param::TYPE, 7.0),
            (param::SHAPER_LINEAR, 1.0),
            (param::DRIVE, 6.0),
            (param::POST_CLIP, 1.0),
            (param::OUTPUT, -6.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "post-clip-hard",
        sample_rate: 16_000.0,
        seconds: 0.3,
        keep: 4,
        input: tone,
        settings: &[
            (param::TYPE, 7.0),
            (param::SHAPER_LINEAR, 1.0),
            (param::DRIVE, 6.0),
            (param::POST_CLIP, 2.0),
            (param::OUTPUT, -6.0),
        ],
        changes: &[],
    },
    // Changes while playing: the curve, the drive, the Color amounts, Dry/Wet and the DC filter.
    Scenario {
        name: "changes",
        sample_rate: 16_000.0,
        seconds: 0.6,
        keep: 4,
        input: tone_with_offset,
        settings: &[(param::DRIVE, 6.0)],
        changes: &[
            (0.1, param::TYPE, 5.0),
            (0.2, param::DRIVE, 18.0),
            (0.3, param::COLOR_LOW, 1.0),
            (0.4, param::MIX, 0.4),
            (0.5, param::DC_FILTER, 1.0),
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

/// 110 Hz on the left and 165 Hz on the right, all the way through.
fn tone(sample_rate: f64, frame: usize) -> [f32; 2] {
    let t = frame as f64 / sample_rate;
    let side = |hz: f64| (0.4 * (std::f64::consts::TAU * hz * t).sin()) as f32;
    [side(110.0), side(165.0)]
}

/// The tone with a DC offset of 0.2, for the DC filter.
fn tone_with_offset(sample_rate: f64, frame: usize) -> [f32; 2] {
    tone(sample_rate, frame).map(|v| v + 0.2)
}

/// The scenario rendered, interleaved, every `keep`-th frame.
fn render(s: &Scenario) -> Vec<f32> {
    let mut d = Saturator::new();
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
