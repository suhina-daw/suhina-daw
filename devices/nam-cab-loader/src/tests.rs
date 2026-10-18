// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::{self};

use super::*;

const SR: f64 = 48_000.0;
/// The schedule's delay, in frames: the output stream trails the input by this.
const D: usize = CHUNK;

fn loader(ir: Option<Ir>, settings: &[(usize, f64)]) -> CabLoader {
    let mut l = match ir {
        Some(ir) => CabLoader::with_ir(ir),
        None => CabLoader::new(),
    };
    for &(index, value) in settings {
        l.set_param(index, value);
    }
    // A prepare jumps the smoothed parameters to their set values and clears the schedule.
    l.prepare(SR).expect("prepare");
    l
}

/// `frames` frames of `hz`'s sine at `level`, both sides.
fn sine(level: f64, hz: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let v = ((2.0 * std::f64::consts::PI * hz * n as f64 / SR).sin() * level) as f32;
            [v, v]
        })
        .collect()
}

/// Frames of one test signal as left/right pairs.
fn frames(signal: &[f32]) -> &[[f32; 2]] {
    signal.as_chunks::<2>().0
}

/// Amplitude of `hz` on the left, from one DFT bin, over a window from `from` on.
fn level(signal: &[[f32; 2]], hz: f64, from: usize) -> f64 {
    let frames = signal.get(from..).unwrap_or(&[]);
    let (mut re, mut im) = (0.0, 0.0);
    for (n, [l, _]) in frames.iter().enumerate() {
        let w = 2.0 * std::f64::consts::PI * hz * n as f64 / SR;
        re += f64::from(*l) * w.cos();
        im += f64::from(*l) * w.sin();
    }
    2.0 * re.hypot(im) / frames.len().max(1) as f64
}

#[test]
fn passes_the_device_conformance_checks() {
    let failures = conformance::check(&|| Box::new(CabLoader::new()));
    assert!(failures.is_empty(), "{failures:#?}");
    // And with an IR, which fills the convolution but not the rules.
    let ir = Ir::from_interleaved(2, 3_000, 48_000, &{
        let mut s: Vec<f32> = (0..6_000).map(|k| (((k as f64) * 0.0021).sin() * 0.02) as f32).collect();
        s[2] = 0.5;
        s
    });
    let make = move || -> Box<dyn Device> { Box::new(CabLoader::with_ir(ir.clone())) };
    let failures = conformance::check(&make);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A delta IR both sides; the loader without HP is a delay-line of the IR's head.
fn direct_ir() -> Ir {
    Ir::from_interleaved(2, 1, 48_000, &[1.0, 1.0])
}

#[test]
fn without_an_ir_the_signal_comes_out_one_chunk_later() {
    let mut l = loader(None, &[(param::HP, 0.0)]);
    let input = sine(0.5, 440.0, 2_000);
    let mut s = input.clone();
    l.process(&ProcessContext::default(), &mut s);
    // The stream: the input one chunk later, silence before it.
    for (n, frame) in frames(&s).iter().enumerate() {
        let want: [f32; 2] = if n >= D {
            let in_frame = frames(&input)[n - D];
            [in_frame[0], in_frame[1]]
        } else {
            [0.0, 0.0]
        };
        for (w, f) in want.iter().zip(frame) {
            assert!((f64::from(*f) - f64::from(*w)).abs() < 1e-5, "frame {n}: {f} vs {w}");
        }
    }
}

#[test]
fn a_one_sample_ir_is_unity_gain() {
    let mut l = loader(Some(direct_ir()), &[(param::HP, 0.0)]);
    let in_signal = sine(0.5, 220.0, 1_500);
    let mut s = in_signal.clone();
    l.process(&ProcessContext::default(), &mut s);
    for (n, frame) in frames(&s).iter().enumerate().skip(D + 64) {
        let want = sine(0.5, 220.0, 1_500).as_chunks::<2>().0[n - D];
        assert!((f64::from(frame[0]) - f64::from(want[0])).abs() < 1e-4, "frame {n}: {} vs {}", frame[0], want[0]);
    }
}

#[test]
fn phase_inverts_the_signal() {
    let mut a = loader(Some(direct_ir()), &[(param::HP, 0.0)]);
    let mut b = loader(Some(direct_ir()), &[(param::HP, 0.0), (param::PHASE, 1.0)]);
    let mut s = sine(0.5, 330.0, 1_500);
    let mut t = s.clone();
    a.process(&ProcessContext::default(), &mut s);
    b.process(&ProcessContext::default(), &mut t);
    for (x, y) in s.iter().zip(&t) {
        assert!((f64::from(*x) + f64::from(*y)).abs() < 1e-5, "{x} vs -{y}");
    }
}

#[test]
fn the_delay_holds_a_pulse_back() {
    let mut l = loader(Some(direct_ir()), &[(param::HP, 0.0), (param::DELAY, 0.002)]);
    // A one-frame pulse at pair 20, as a full interleaved stream.
    let mut s = vec![0.0f32; 1_500 * 2];
    let want_pulse = D + 20 + (0.002 * SR).round() as usize;
    s[2 * 20] = 1.0;
    l.process(&ProcessContext::default(), &mut s);
    for (n, frame) in frames(&s).iter().enumerate() {
        let want = if n == want_pulse { 1.0 } else { 0.0 };
        assert!((f64::from(frame[0]) - want).abs() < 1e-6, "frame {n}: {} vs {want}", frame[0]);
    }
}

#[test]
fn the_volume_sets_the_level() {
    let mut l = loader(Some(direct_ir()), &[(param::HP, 0.0), (param::VOLUME, -12.0)]);
    let mut s = sine(0.5, 1_000.0, 9_600);
    l.process(&ProcessContext::default(), &mut s);
    let high = level(frames(&s), 1_000.0, D + 4_800);
    assert!((high - 0.1256).abs() < 0.01, "level {high}");
}

#[test]
fn the_treble_cut_quiets_high_tones_only() {
    let mut cut = loader(Some(direct_ir()), &[(param::HP, 0.0), (param::TREBLE, -24.0)]);
    let mut s = sine(0.5, 12_000.0, 9_600);
    cut.process(&ProcessContext::default(), &mut s);
    let high = level(frames(&s), 12_000.0, D + 4_800);
    let mut low_signal = sine(0.5, 300.0, 9_600);
    cut.process(&ProcessContext::default(), &mut low_signal);
    let low = level(frames(&low_signal), 300.0, D + 4_800);
    assert!((high - 0.06).abs() < 0.05, "high {high}");
    assert!((low - 0.5).abs() < 0.05, "low {low}");
}

#[test]
fn hp_takes_the_rumble_out() {
    let mut on = loader(Some(direct_ir()), &[(param::HP, 1.0)]);
    let mut s = sine(0.5, 40.0, 9_600);
    on.process(&ProcessContext::default(), &mut s);
    let rumble = level(frames(&s), 40.0, D + 4_800);
    assert!(rumble < 0.2, "rumble {rumble}");
    let mut off = loader(Some(direct_ir()), &[(param::HP, 0.0)]);
    let mut t = sine(0.5, 40.0, 9_600);
    off.process(&ProcessContext::default(), &mut t);
    assert!((level(frames(&t), 40.0, D + 4_800) - 0.5).abs() < 0.02);
}

#[test]
fn a_mono_ir_feeds_both_sides() {
    // A two-tap mono IR: each side runs through the same taps, so a pulse in both repeats in
    // both.
    let ir = Ir::from_interleaved(1, 2, 48_000, &[0.5, 0.25]);
    let mut l = loader(Some(ir), &[(param::HP, 0.0)]);
    let mut s = vec![0.0f32; 1_200 * 2];
    s[2 * 40] = 1.0;
    s[2 * 40 + 1] = 1.0;
    l.process(&ProcessContext::default(), &mut s);
    for (n, frame) in frames(&s).iter().enumerate() {
        let want = if n == 40 + D {
            0.5
        } else if n == 41 + D {
            0.25
        } else {
            0.0
        };
        assert!((f64::from(frame[0]) - want).abs() < 1e-6, "left frame {n}: {} vs {want}", frame[0]);
        assert!((f64::from(frame[1]) - want).abs() < 1e-6, "right frame {n}: {} vs {want}", frame[1]);
    }
}

#[test]
fn an_ir_at_another_rate_does_not_run() {
    let ir = Ir::from_interleaved(2, 4, 44_100, &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let mut l = CabLoader::with_ir(ir);
    l.set_param(param::HP, 0.0);
    l.prepare(SR).expect("prepare");
    // Nothing but the trims: an identity stream, one chunk late, finite.
    let input = sine(0.5, 700.0, 2_000);
    let mut s = input.clone();
    l.process(&ProcessContext::default(), &mut s);
    for (n, frame) in frames(&s).iter().enumerate().skip(D + 64) {
        let want = frames(&input)[n - D];
        assert!(frame[0].is_finite());
        assert!((f64::from(frame[0]) - f64::from(want[0])).abs() < 1e-5, "frame {n}");
    }
}

#[test]
fn the_odd_trailing_sample_is_left_alone() {
    let mut l = loader(None, &[]);
    let mut s = vec![7.0f32; 5];
    l.process(&ProcessContext::default(), &mut s);
    // The odd trailing sample of the buffer's end is left as it is.
    assert_eq!(s[4], 7.0);
}

#[test]
fn a_longer_ir_convolves_finitely() {
    let mut taps = vec![0.0f32; 3_000];
    for (k, tap) in taps.iter_mut().enumerate().skip(1) {
        *tap = (((k as f64) * 0.0003).exp() * 0.05 * (0.5 + 0.5 * ((k as f64) * 0.011).sin())) as f32;
    }
    taps[0] = 0.8;
    let ir = Ir::from_interleaved(2, 1_500, 48_000, &taps);
    let mut l = loader(Some(ir), &[(param::HP, 0.0)]);
    let mut s = sine(0.5, 2_000.0, 9_600);
    l.process(&ProcessContext::default(), &mut s);
    for &v in &s {
        assert!(v.is_finite(), "{v}");
    }
    let level = level(frames(&s), 2_000.0, D + 4_800);
    assert!(level > 1e-4, "level {level}");
}
