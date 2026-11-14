// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// The plain linear convolution the convolver must reproduce: `y[n] = sum h[k] * x[n-k]`.
/// `x` is the whole input stream handed over so far.
fn naive(x: &[f32], h: &[f32]) -> Vec<f32> {
    let mut y = vec![0.0f32; x.len()];
    for (n, y_n) in y.iter_mut().enumerate() {
        let mut sum = 0.0;
        for (k, &h_k) in h.iter().enumerate() {
            let at = n as isize - k as isize;
            if at < 0 {
                break;
            }
            sum += (x[at as usize] as f64) * (h_k as f64);
        }
        *y_n = sum as f32;
    }
    y
}

fn chunks_of(x: &[f32], n: usize) -> Vec<Vec<f32>> {
    x.chunks(n).map(|c| c.to_vec()).collect::<Vec<_>>()
}

#[test]
fn a_convolved_stream_equals_the_naive_convolution() {
    let h: Vec<f32> =
        (0..2_000).map(|k| (((k as f64) * 0.013).sin() * 0.01) as f32).chain(std::iter::once(1.0)).collect();
    let x: Vec<f32> =
        (0..8_000).map(|n| (((n as f64) * 0.0021).sin() + ((n as f64) * 0.0107).sin() * 0.5) as f32).collect();
    let want = naive(&x, &h);
    let mut convolver = Convolver::default();
    convolver.init(&h).expect("init");
    let mut got = vec![0.0f32; 0];
    for chunk in chunks_of(&x, 97) {
        let mut out = vec![0.0f32; chunk.len()];
        convolver.process(&chunk, &mut out);
        got.extend(out);
    }
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert!((g - w).abs() < 1e-4, "{g} vs {w}");
    }
}

#[test]
fn an_impulse_response_of_one_gives_the_input_back() {
    let h = [1.0f32];
    let x: Vec<f32> = (0..500).map(|n| ((n as f64) * 0.11).sin() as f32).collect();
    let mut convolver = Convolver::default();
    convolver.init(&h).expect("init");
    let mut got = Vec::new();
    for chunk in chunks_of(&x, 64) {
        let mut out = vec![0.0f32; chunk.len()];
        convolver.process(&chunk, &mut out);
        got.extend(out);
    }
    for (g, w) in got.iter().zip(&x) {
        assert!((g - w).abs() < 1e-5, "{g} vs {w}");
    }
}

#[test]
fn an_ir_beyond_one_partition_lines_up_across_chunk_sizes() {
    // The same input in chunks of one size and in chunks of another gives the same stream.
    let h: Vec<f32> = (0..999).map(|k| (((k as f64) * 0.05).cos() * 0.03) as f32).collect();
    let x: Vec<f32> = (0..3_000).map(|n| (((n as f64) * 0.0023).sin()) as f32).collect();
    let want = naive(&x, &h);
    let mut sizes = Convolver::default();
    sizes.init(&h).expect("init");
    let mut got = Vec::new();
    for chunk in chunks_of(&x, 64) {
        let mut out = vec![0.0f32; chunk.len()];
        sizes.process(&chunk, &mut out);
        got.extend(out);
    }
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        assert!((g - w).abs() < 1e-4, "chunked vs naive at {i}: {g} vs {w}");
    }
    // And the odd-tail last chunk, as a project's end has.
    let mut but_first = Convolver::default();
    but_first.init(&h).expect("init");
    let first = x.len() - 7;
    let mut head = vec![0.0f32; first];
    but_first.process(&x[..first], &mut head);
    let mut tail = vec![0.0f32; 7];
    but_first.process(&x[first..], &mut tail);
    let combined: Vec<f32> = head.into_iter().chain(tail).collect();
    for (i, (g, w)) in combined.iter().zip(&want).enumerate() {
        assert!((g - w).abs() < 1e-4, "split end vs naive at {i}: {g} vs {w}");
    }
}

#[test]
fn an_ir_tells_its_sides_apart() {
    let ir = Ir::from_interleaved(2, 5, 48_000, &[1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0, 5.0, 50.0]);
    assert_eq!(ir.frames(), 5);
    assert_eq!(ir.side(0), &[1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!(ir.side(1), &[10.0, 20.0, 30.0, 40.0, 50.0]);
    assert!(ir.side(2).is_empty());
    // Hostile numbers and a short buffer do not panic.
    let ir = Ir::from_interleaved(2, 4, 48_000, &[f32::NAN, 1.0, 2.0]);
    // Only one whole frame pair fits; a NaN frame is silence.
    assert_eq!(ir.side(0), &[0.0, 0.0, 0.0, 0.0]);
    assert_eq!(ir.side(1), &[1.0, 0.0, 0.0, 0.0]);
    let ir = Ir::from_interleaved(0, 3, 48_000, &[]);
    assert_eq!(ir.frames(), 3);
}

#[test]
fn a_zero_led_ir_keeps_only_what_it_has() {
    // An IR whose first samples are zero stays silent at first, as the IR says.
    let mut h = vec![0.0f32; 1_000];
    h[700] = 1.0;
    let x: Vec<f32> = (0..2_000).map(|n| ((n as f64) * 0.09).sin() as f32).collect();
    let mut convolver = Convolver::default();
    convolver.init(&h).expect("init");
    let mut got = Vec::new();
    for chunk in chunks_of(&x, 512) {
        let mut out = vec![0.0f32; chunk.len()];
        convolver.process(&chunk, &mut out);
        got.extend(out);
    }
    let want = naive(&x, &h);
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        assert!((g - w).abs() < 1e-4, "{i}: {g} vs {w}");
    }
}

/// Index and value of the largest magnitude in `side`.
fn peak(side: &[f32]) -> (usize, f32) {
    side.iter().copied().enumerate().fold((0, 0.0), |best, (i, v)| if v.abs() > best.1.abs() { (i, v) } else { best })
}

#[test]
fn a_resampled_ir_keeps_its_length_and_its_last_frames() {
    // An impulse 10 frames before the end of a 44.1 kHz IR: at 48 kHz it sits at 1077.6 of
    // 1088 frames, inside the kernel's tail the resampler still held back.
    let mut data = vec![0.0f32; 1_000];
    data[990] = 1.0;
    let ir = Ir::from_interleaved(1, 1_000, 44_100, &data).resampled_to(48_000);
    assert_eq!(ir.sample_rate, 48_000);
    assert!(ir.frames().abs_diff(1_088) <= 1, "{}", ir.frames());
    let (at, value) = peak(ir.side(0));
    assert!(at.abs_diff(1_078) <= 1, "{at}");
    // The impulse falls between two output frames (1077.6): the nearest holds sinc(0.4).
    assert!(value > 0.7, "{value}");
}

#[test]
fn a_very_short_resampled_ir_is_not_empty() {
    for (from, to, frames, want) in [(44_100, 48_000, 4, 4), (48_000, 44_100, 3, 3), (96_000, 44_100, 1, 1)] {
        let mut data = vec![0.0f32; frames];
        data[0] = 1.0;
        let ir = Ir::from_interleaved(1, frames, from, &data).resampled_to(to);
        assert_eq!(ir.frames(), want, "{from} -> {to}");
        assert!(ir.side(0).iter().any(|v| v.abs() > 0.1), "{from} -> {to}");
    }
}

#[test]
fn an_empty_ir_stays_empty_when_resampled() {
    assert_eq!(Ir::from_interleaved(1, 0, 44_100, &[]).resampled_to(48_000).frames(), 0);
}
