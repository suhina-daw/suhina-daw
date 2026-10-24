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
const TEMPO: f64 = 120.0;

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

/// Free times, so the scenarios that are not about the tempo do not depend on it.
const FREE: [(usize, f64); 3] = [(param::SYNC_LEFT, 0.0), (param::SYNC_RIGHT, 0.0), (param::LINK, 0.0)];

const SCENARIOS: &[Scenario] = &[
    // Every control at its default: synced to 3 and 4 sixteenths, the band-pass in the loop.
    Scenario {
        name: "defaults",
        sample_rate: 48_000.0,
        seconds: 1.5,
        keep: 24,
        input: burst,
        settings: &[],
        changes: &[],
    },
    Scenario {
        name: "free-times",
        sample_rate: 16_000.0,
        seconds: 1.5,
        keep: 6,
        input: burst,
        settings: &[
            FREE[0],
            FREE[1],
            FREE[2],
            (param::TIME_LEFT, 0.13),
            (param::TIME_RIGHT, 0.21),
            (param::FEEDBACK, 0.7),
            (param::FILTER_FREQUENCY, 2_000.0),
            (param::FILTER_WIDTH, 2.0),
            (param::MIX, 0.6),
        ],
        changes: &[],
    },
    Scenario {
        name: "synced-offsets",
        sample_rate: 44_100.0,
        seconds: 1.5,
        keep: 18,
        input: burst,
        settings: &[
            (param::LINK, 0.0),
            (param::SIXTEENTHS_LEFT, 1.0),
            (param::SIXTEENTHS_RIGHT, 5.0),
            (param::OFFSET_LEFT, 0.2),
            (param::OFFSET_RIGHT, -0.3),
            (param::FEEDBACK, 0.4),
            (param::FILTER, 0.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "ping-pong",
        sample_rate: 16_000.0,
        seconds: 1.5,
        keep: 6,
        input: left_burst,
        settings: &[
            FREE[0],
            FREE[1],
            (param::TIME_LEFT, 0.1),
            (param::PING_PONG, 1.0),
            (param::FEEDBACK, 0.6),
            (param::FILTER, 0.0),
            (param::MIX, 1.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "lfo",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: tone,
        settings: &[
            FREE[0],
            FREE[1],
            (param::TIME_LEFT, 0.05),
            (param::FEEDBACK, 0.3),
            (param::FILTER_WIDTH, 2.0),
            (param::LFO_RATE, 3.0),
            (param::LFO_SHAPE, 0.0),
            (param::LFO_TO_TIME, 0.5),
            (param::LFO_TO_FILTER, 0.7),
            (param::MIX, 1.0),
        ],
        changes: &[],
    },
    Scenario {
        name: "lfo-synced-wander",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: tone,
        settings: &[
            FREE[0],
            FREE[1],
            (param::TIME_LEFT, 0.08),
            (param::FEEDBACK, 0.3),
            (param::LFO_MODE, 2.0),
            (param::LFO_SYNCED, 12.0), // 1/4
            (param::LFO_SHAPE, 6.0),
            (param::LFO_MORPH, 0.4),
            (param::LFO_TO_TIME, 0.3),
            (param::LFO_TO_FILTER, 0.3),
            (param::MIX, 1.0),
        ],
        changes: &[],
    },
    // The same time change under each transition.
    Scenario {
        name: "repitch",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: tone,
        settings: &TIME_CHANGE,
        changes: &[(0.4, param::TIME_LEFT, 0.05)],
    },
    Scenario {
        name: "fade",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: tone,
        settings: &with_transition(1.0),
        changes: &[(0.4, param::TIME_LEFT, 0.05)],
    },
    Scenario {
        name: "jump",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: tone,
        settings: &with_transition(2.0),
        changes: &[(0.4, param::TIME_LEFT, 0.05)],
    },
    // Frozen while the tone goes on, then let go.
    Scenario {
        name: "freeze",
        sample_rate: 16_000.0,
        seconds: 1.5,
        keep: 6,
        input: tone,
        settings: &[FREE[0], FREE[1], (param::TIME_LEFT, 0.15), (param::FEEDBACK, 0.5), (param::MIX, 1.0)],
        changes: &[(0.5, param::FREEZE, 1.0), (1.0, param::FREEZE, 0.0)],
    },
    Scenario {
        name: "equal-loudness",
        sample_rate: 16_000.0,
        seconds: 1.0,
        keep: 4,
        input: burst,
        settings: &[(param::MIX, 0.3), (param::EQUAL_LOUDNESS, 1.0)],
        changes: &[(0.5, param::MIX, 0.8)],
    },
];

const TIME_CHANGE: [(usize, f64); 5] =
    [FREE[0], FREE[1], (param::TIME_LEFT, 0.2), (param::FEEDBACK, 0.4), (param::MIX, 1.0)];

const fn with_transition(transition: f64) -> [(usize, f64); 6] {
    let t = TIME_CHANGE;
    [t[0], t[1], t[2], t[3], t[4], (param::TRANSITION, transition)]
}

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

fn left_burst(sample_rate: f64, frame: usize) -> [f32; 2] {
    [burst(sample_rate, frame)[0], 0.0]
}

/// 330 Hz on the left and 495 Hz on the right, all the way through.
fn tone(sample_rate: f64, frame: usize) -> [f32; 2] {
    let t = frame as f64 / sample_rate;
    let side = |hz: f64| (0.4 * (std::f64::consts::TAU * hz * t).sin()) as f32;
    [side(330.0), side(495.0)]
}

/// The scenario rendered, interleaved, every `keep`-th frame.
fn render(s: &Scenario) -> Vec<f32> {
    let mut d = Delay::new();
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
        let beat = start as f64 / s.sample_rate * TEMPO / 60.0;
        let context = ProcessContext { tempo: TEMPO, beat, playing: true };
        d.process(&context, &mut buffer[start * 2..end * 2]);
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
