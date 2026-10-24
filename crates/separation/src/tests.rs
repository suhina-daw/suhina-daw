// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::*;

/// A backend for tests: stem 0 is `f(input sample, window index)`, the other stems are silent.
/// It records every window it is handed.
struct Probe {
    shape: ModelShape,
    f: fn(f32, usize) -> f32,
    calls: usize,
    seen: Vec<Vec<f32>>,
    fail_at: Option<usize>,
}

impl Probe {
    fn new(shape: ModelShape, f: fn(f32, usize) -> f32) -> Self {
        Probe { shape, f, calls: 0, seen: Vec::new(), fail_at: None }
    }
}

impl Backend for Probe {
    fn shape(&self) -> &ModelShape {
        &self.shape
    }

    fn run(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), BackendError> {
        if self.fail_at == Some(self.calls) {
            return Err(BackendError("out of memory".into()));
        }
        self.seen.push(input.to_vec());
        for (o, &x) in output.iter_mut().zip(input) {
            *o = (self.f)(x, self.calls);
        }
        self.calls += 1;
        Ok(())
    }
}

fn shape(rate: u32, channels: usize, window: usize, hop: usize, weighting: Weighting, stems: usize) -> ModelShape {
    ModelShape { sample_rate: rate, channels, window, hop, weighting, stems }
}

fn run(source: &Audio, backend: &mut dyn Backend) -> Result<Separated, Error> {
    separate(source, backend, &AtomicBool::new(false), &mut |_| {})
}

fn mono(rate: u32, samples: Vec<f32>) -> Audio {
    Audio::new(rate, vec![samples]).unwrap()
}

fn sine(rate: u32, hz: f64, frames: usize, gain: f64) -> Vec<f32> {
    (0..frames).map(|i| (gain * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin()) as f32).collect()
}

/// The largest difference between two signals over `range`.
fn max_diff(a: &[f32], b: &[f32], range: std::ops::Range<usize>) -> f32 {
    a[range.clone()].iter().zip(&b[range]).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

/// Every stem's channel `c` added up.
fn stem_sum(out: &Separated, c: usize) -> Vec<f32> {
    let frames = out.stems[0].frames();
    (0..frames).map(|i| out.stems.iter().map(|s| s.channels()[c][i]).sum()).collect()
}

// ---- weighting and the plan ----

#[test]
fn every_weighting_is_above_zero_at_every_frame() {
    for weighting in [Weighting::Flat, Weighting::Triangle, Weighting::Hann] {
        for len in [1, 2, 3, 16, 17, 1000, 343_980] {
            let w = weighting.weights(len);
            assert_eq!(w.len(), len);
            assert!(w.iter().all(|&x| x > 0.0 && x <= 1.0), "{weighting:?} {len}");
        }
    }
    // The triangle is symmetric and peaks at the centre.
    let t = Weighting::Triangle.weights(8);
    assert_eq!(t, [0.25, 0.5, 0.75, 1.0, 1.0, 0.75, 0.5, 0.25]);
}

#[test]
fn the_plan_covers_every_frame_and_pads_only_the_last_window() {
    let s = shape(8000, 1, 100, 75, Weighting::Triangle, 1);
    for frames in [1, 99, 100, 101, 175, 176, 1000, 1001] {
        let plan = Plan::new(frames, &s);
        let last = plan.count - 1;
        assert!(plan.start(last) < frames, "{frames}: the last window starts past the end");
        assert!(plan.start(last) + plan.window >= frames, "{frames}: the end is not covered");
        if last > 0 {
            assert!(plan.start(last - 1) + plan.window < frames, "{frames}: one window too many");
        }
        for w in 0..last {
            assert_eq!(plan.filled(w), plan.window, "{frames}: window {w} is padded");
        }
    }
    assert_eq!(Plan::new(101, &s).count, 2);
    assert_eq!(Plan::new(175, &s).count, 2);
    assert_eq!(Plan::new(176, &s).count, 3);
}

// ---- overlap-add ----

#[test]
fn the_weighting_divides_out_to_unity_gain_on_a_constant() {
    for weighting in [Weighting::Flat, Weighting::Triangle, Weighting::Hann] {
        for (window, hop, frames) in [(64, 48, 1000), (64, 32, 1000), (100, 7, 333), (16, 16, 50), (128, 100, 20)] {
            let mut probe = Probe::new(shape(8000, 1, window, hop, weighting, 2), |x, _| x);
            let out = run(&mono(8000, vec![0.3; frames]), &mut probe).unwrap();
            let stem = &out.stems[0].channels()[0];
            assert_eq!(stem.len(), frames);
            for (i, s) in stem.iter().enumerate() {
                assert!((s - 0.3).abs() < 1e-6, "{weighting:?} {window}/{hop}: frame {i} is {s}");
            }
            assert!(out.stems[1].channels()[0].iter().all(|&s| s == 0.0));
        }
    }
}

#[test]
fn an_identity_model_gives_the_source_back_including_the_padded_tail() {
    let frames = 1013;
    let ramp: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32 - 0.5).collect();
    let mut probe = Probe::new(shape(8000, 1, 128, 96, Weighting::Hann, 1), |x, _| x);
    let out = run(&mono(8000, ramp.clone()), &mut probe).unwrap();
    assert!(max_diff(&out.stems[0].channels()[0], &ramp, 0..frames) < 1e-6);
}

#[test]
fn the_padded_tail_window_holds_the_last_frames_at_its_start() {
    let frames = 1013;
    let ramp: Vec<f32> = (0..frames).map(|i| i as f32 + 1.0).collect();
    let s = shape(8000, 1, 128, 96, Weighting::Triangle, 1);
    let mut probe = Probe::new(s.clone(), |x, _| x);
    run(&mono(8000, ramp.clone()), &mut probe).unwrap();
    let plan = Plan::new(frames, &s);
    assert_eq!(probe.seen.len(), plan.count);
    // 1 + ceil((1013 - 128) / 96) = 11 windows; the last starts at 960 and holds 53 frames.
    assert_eq!(plan.count, 11);
    let last = &probe.seen[10];
    assert_eq!(last[0], ramp[960]);
    assert_eq!(&last[..53], &ramp[960..]);
    assert!(last[53..].iter().all(|&x| x == 0.0), "the tail is padded with silence");
    // Every other window holds the frames at its start.
    for (w, seen) in probe.seen.iter().enumerate().take(10) {
        assert_eq!(seen[..], ramp[w * 96..w * 96 + 128], "window {w}");
    }
}

#[test]
fn a_window_boundary_does_not_click() {
    // A model whose windows disagree: each adds its own offset. With a triangle the windows cross
    // over smoothly; with a flat weighting the same disagreement is a step, which shows that the
    // test sees a click when there is one.
    let offset = |x: f32, w: usize| x + 0.1 * w as f32;
    let source = mono(8000, vec![0.0; 2000]);
    let largest_step = |weighting| {
        let mut probe = Probe::new(shape(8000, 1, 256, 128, weighting, 1), offset);
        let out = run(&source, &mut probe).unwrap();
        let stem = &out.stems[0].channels()[0];
        stem.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0, f32::max)
    };
    let smooth = largest_step(Weighting::Triangle);
    // 0.1 shared out over the 128 frames of overlap, with room for the triangle's curvature.
    assert!(smooth < 0.1 / 128.0 * 2.5, "triangle: a step of {smooth}");
    let hann = largest_step(Weighting::Hann);
    assert!(hann < 0.1 / 128.0 * 2.5, "hann: a step of {hann}");
    let flat = largest_step(Weighting::Flat);
    assert!(flat > 0.04, "flat: the step of {flat} should be visible");
}

// ---- the test backend ----

fn splitter(rate: u32, channels: usize) -> BandSplitter {
    BandSplitter::new(rate, channels, 1024, 768, Weighting::Triangle, &[200.0, 2000.0]).unwrap()
}

/// A stereo test signal: a low, a middle and a high sine, different on each side.
fn stereo_mix(rate: u32, frames: usize) -> Audio {
    let mix = |a: f64, b: f64, c: f64| -> Vec<f32> {
        let (x, y, z) = (sine(rate, a, frames, 0.3), sine(rate, b, frames, 0.2), sine(rate, c, frames, 0.1));
        (0..frames).map(|i| x[i] + y[i] + z[i]).collect()
    };
    Audio::new(rate, vec![mix(60.0, 700.0, 3000.0), mix(90.0, 900.0, 3500.0)]).unwrap()
}

/// The stems sum to the source within this, at the model's rate (spec › Tests).
const SUM_TOLERANCE: f32 = 1e-5;
/// … and within this away from the ends after resampling there and back, for a source below
/// 0.4 times the lower sample rate.
const RESAMPLED_SUM_TOLERANCE: f32 = 1e-3;

#[test]
fn the_stems_sum_to_the_source() {
    let source = stereo_mix(16_000, 16_000);
    let out = run(&source, &mut splitter(16_000, 2)).unwrap();
    assert_eq!(out.stems.len(), 3);
    for c in 0..2 {
        let sum = stem_sum(&out, c);
        let worst = max_diff(&sum, &source.channels()[c], 0..source.frames());
        assert!(worst < SUM_TOLERANCE, "channel {c}: off by {worst}");
    }
}

#[test]
fn the_bands_split_where_the_crossovers_are() {
    // A 60 Hz sine lands in the low stem, a 5 kHz sine in the high one.
    let rate = 16_000;
    let frames = 8000;
    for (hz, stem) in [(60.0, 0), (700.0, 1), (5000.0, 2)] {
        let out = run(&mono(rate, sine(rate, hz, frames, 0.5)), &mut splitter(rate, 1)).unwrap();
        let rms = |s: &[f32]| (s[1000..7000].iter().map(|x| x * x).sum::<f32>() / 6000.0).sqrt();
        let levels: Vec<f32> = out.stems.iter().map(|s| rms(&s.channels()[0])).collect();
        let loudest = levels.iter().enumerate().fold(0, |best, (i, l)| if *l > levels[best] { i } else { best });
        assert_eq!(loudest, stem, "{hz} Hz: levels {levels:?}");
    }
}

#[test]
fn a_source_at_another_rate_is_resampled_there_and_back() {
    let source = stereo_mix(44_100, 22_050);
    let mut backend = splitter(16_000, 2);
    let out = run(&source, &mut backend).unwrap();
    for stem in &out.stems {
        assert_eq!(stem.sample_rate(), 44_100);
        assert_eq!(stem.frames(), 22_050);
        assert_eq!(stem.channels().len(), 2);
    }
    for c in 0..2 {
        let sum = stem_sum(&out, c);
        let worst = max_diff(&sum, &source.channels()[c], 1000..21_050);
        assert!(worst < RESAMPLED_SUM_TOLERANCE, "channel {c}: off by {worst}");
    }
}

#[test]
fn a_mono_source_runs_through_a_stereo_model_and_comes_back_mono() {
    let source = mono(16_000, sine(16_000, 440.0, 5000, 0.5));
    let mut probe = Probe::new(shape(16_000, 2, 512, 384, Weighting::Triangle, 1), |x, _| x);
    let out = run(&source, &mut probe).unwrap();
    // Both model channels got the mono signal.
    assert_eq!(probe.seen[0][..512], probe.seen[0][512..]);
    assert_eq!(out.stems[0].channels().len(), 1);
    assert!(max_diff(&out.stems[0].channels()[0], &source.channels()[0], 0..5000) < 1e-6);
}

#[test]
fn a_stereo_source_runs_through_a_mono_model_one_side_at_a_time() {
    let source = stereo_mix(16_000, 5000);
    let mut probe = Probe::new(shape(16_000, 1, 512, 384, Weighting::Hann, 1), |x, _| x);
    let total = std::cell::Cell::new(0);
    let out = separate(&source, &mut probe, &AtomicBool::new(false), &mut |p| total.set(p.total)).unwrap();
    let windows = Plan::new(5000, probe.shape()).count;
    assert_eq!(probe.calls, 2 * windows, "a pass per side");
    assert_eq!(total.get(), 2 * windows);
    for c in 0..2 {
        assert!(max_diff(&out.stems[0].channels()[c], &source.channels()[c], 0..5000) < 1e-6, "side {c}");
    }
}

// ---- resampling ----

#[test]
fn resampling_keeps_a_constant_and_a_sine() {
    for (from, to) in [(44_100, 48_000), (48_000, 44_100), (44_100, 16_000), (8_000, 96_000)] {
        let r = Resampler::new(from, to);
        let frames = from as usize / 2;
        let out_frames = r.output_len(frames);
        assert_eq!(out_frames, (frames as u64 * u64::from(to)).div_ceil(u64::from(from)) as usize);
        let edge = out_frames / 10;
        let constant = r.process(&vec![0.5; frames], out_frames);
        assert!(max_diff(&constant, &vec![0.5; out_frames], edge..out_frames - edge) < 1e-6, "{from}->{to}");
        // A sine at a quarter of the lower Nyquist frequency comes out as the same sine.
        let hz = f64::from(from.min(to)) / 8.0;
        let out = r.process(&sine(from, hz, frames, 0.5), out_frames);
        let expected = sine(to, hz, out_frames, 0.5);
        let worst = max_diff(&out, &expected, edge..out_frames - edge);
        assert!(worst < 1e-3, "{from}->{to}: off by {worst}");
    }
}

#[test]
fn resampling_down_removes_what_is_above_the_new_nyquist() {
    // 20 kHz cannot exist at 16 kHz; it must not fold back as a lower tone.
    let r = Resampler::new(48_000, 16_000);
    let out = r.process(&sine(48_000, 20_000.0, 24_000, 0.5), 8000);
    let peak = out[1000..7000].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak < 1e-3, "{peak}");
}

// ---- determinism and the golden render ----

#[test]
fn the_same_separation_twice_is_byte_identical() {
    let source = stereo_mix(44_100, 30_000);
    let a = run(&source, &mut splitter(16_000, 2)).unwrap();
    let b = run(&source, &mut splitter(16_000, 2)).unwrap();
    let bytes = |s: &Separated| -> Vec<u8> {
        s.stems.iter().flat_map(|a| a.interleaved()).flat_map(f32::to_le_bytes).collect()
    };
    assert_eq!(bytes(&a), bytes(&b));
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// The golden render's input (tests/golden/README.md): 0.25 s of a stereo mix at 12 kHz, with a
/// click on the left side at frame 1000.
fn golden_source() -> Audio {
    let mut source = stereo_mix(12_000, 3000);
    let mut channels = source.channels().to_vec();
    channels[0][1000] += 0.5;
    source = Audio::new(12_000, channels).unwrap();
    source
}

#[test]
fn the_test_backend_renders_like_its_golden_file() {
    let source = golden_source();
    let mut backend = BandSplitter::new(8000, 2, 512, 384, Weighting::Triangle, &[300.0, 1500.0]).unwrap();
    let out = run(&source, &mut backend).unwrap();
    // Checked independently of the file, so a golden file cannot hold a wrong render: the stems
    // add up to the source as the model's rate can hold it (the click is wider than 4 kHz), and
    // the click is mostly in the high stem.
    let (down, up) = (Resampler::new(12_000, 8000), Resampler::new(8000, 12_000));
    for c in 0..2 {
        let held = up.process(&down.process(&source.channels()[c], 2000), 3000);
        let worst = max_diff(&stem_sum(&out, c), &held, 0..3000);
        assert!(worst < 1e-4, "channel {c}: off by {worst}");
    }
    let click = |stem: usize| out.stems[stem].channels()[0][995..1005].iter().fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(click(2) > click(0), "the click is in the low stem");
    // Stems one after another, each interleaved.
    let render: Vec<f32> = out.stems.iter().flat_map(Audio::interleaved).collect();
    assert_eq!(render.len(), 3 * 2 * 3000);
    let path = golden_dir().join("band-splitter.f32");
    suhina_test_support::assert_golden_f32(&path, &render, 1e-5);
}

// ---- progress and cancel ----

#[test]
fn progress_goes_from_zero_to_the_window_count() {
    let source = mono(8000, vec![0.1; 5000]);
    let mut probe = Probe::new(shape(8000, 1, 256, 192, Weighting::Triangle, 1), |x, _| x);
    let mut seen = Vec::new();
    separate(&source, &mut probe, &AtomicBool::new(false), &mut |p| seen.push(p)).unwrap();
    let total = Plan::new(5000, probe.shape()).count;
    let expected: Vec<Progress> = (0..=total).map(|done| Progress { done, total }).collect();
    assert_eq!(seen, expected);
}

#[test]
fn cancel_stops_before_the_next_window() {
    let source = mono(8000, vec![0.1; 5000]);
    let total = Plan::new(5000, &shape(8000, 1, 256, 192, Weighting::Triangle, 1)).count;
    for stop_after in [0, total / 2, total - 1] {
        let mut probe = Probe::new(shape(8000, 1, 256, 192, Weighting::Triangle, 1), |x, _| x);
        let cancel = AtomicBool::new(false);
        let result = separate(&source, &mut probe, &cancel, &mut |p| {
            if p.done == stop_after {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(result, Err(Error::Cancelled), "after {stop_after}");
        assert_eq!(probe.calls, stop_after, "windows run before the cancel took hold");
    }
}

// ---- hostile input ----

#[test]
fn shapes_out_of_bounds_are_refused_with_the_reason() {
    let ok = shape(44_100, 2, 343_980, 257_985, Weighting::Triangle, 4);
    assert_eq!(ok.validate(), Ok(()));
    let cases = [
        ModelShape { sample_rate: 0, ..ok.clone() },
        ModelShape { sample_rate: 10_000_000, ..ok.clone() },
        ModelShape { channels: 0, ..ok.clone() },
        ModelShape { channels: 3, ..ok.clone() },
        ModelShape { stems: 0, ..ok.clone() },
        ModelShape { stems: 17, ..ok.clone() },
        ModelShape { window: 0, hop: 0, ..ok.clone() },
        ModelShape { window: 15, hop: 15, ..ok.clone() },
        ModelShape { window: usize::MAX, ..ok.clone() },
        ModelShape { hop: 0, ..ok.clone() },
        ModelShape { hop: 343_981, ..ok.clone() },
        ModelShape { hop: 1, ..ok.clone() },
        ModelShape { hop: 343_980 / 16 - 1, ..ok.clone() },
    ];
    for bad in cases {
        let mut probe = Probe::new(bad.clone(), |x, _| x);
        let result = run(&mono(44_100, vec![0.0; 100]), &mut probe);
        assert!(matches!(result, Err(Error::InvalidShape(_))), "{bad:?}: {result:?}");
        assert_eq!(probe.calls, 0);
    }
}

#[test]
fn sources_that_cannot_be_separated_are_refused() {
    let refused = |r: Result<Audio, Error>| matches!(r, Err(Error::InvalidSource(_)));
    assert!(refused(Audio::new(44_100, vec![])));
    assert!(refused(Audio::new(44_100, vec![vec![]])));
    assert!(refused(Audio::new(44_100, vec![vec![0.0]; 3])));
    assert!(refused(Audio::new(44_100, vec![vec![0.0; 10], vec![0.0; 9]])));
    assert!(refused(Audio::new(44_100, vec![vec![0.0, f32::NAN]])));
    assert!(refused(Audio::new(44_100, vec![vec![f32::INFINITY]])));
    assert!(refused(Audio::new(7_999, vec![vec![0.0]])));
    assert!(refused(Audio::new(u32::MAX, vec![vec![0.0]])));
    assert!(refused(Audio::from_interleaved(44_100, 0, &[0.0])));
    assert!(refused(Audio::from_interleaved(44_100, 2, &[0.0])));
    let a = Audio::from_interleaved(44_100, 2, &[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
    assert_eq!(a.channels(), [vec![1.0, 3.0], vec![2.0, 4.0]]);
    assert_eq!(a.interleaved(), [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn a_source_over_the_length_limit_is_refused_before_any_work() {
    let frames = MAX_SOURCE_SECONDS * 8000 + 1;
    let source = mono(8000, vec![0.0; frames]);
    let mut probe = Probe::new(shape(8000, 1, 1024, 768, Weighting::Triangle, 1), |x, _| x);
    assert!(matches!(run(&source, &mut probe), Err(Error::TooLong(_))));
    assert_eq!(probe.calls, 0);
    // Within the time limit, but sixteen stems at 384 kHz would not fit the sample limit.
    let source = mono(8000, vec![0.0; 8000 * 60 * 10]);
    let mut probe = Probe::new(shape(384_000, 2, 1024, 768, Weighting::Triangle, 16), |x, _| x);
    assert!(matches!(run(&source, &mut probe), Err(Error::TooLong(_))));
    assert_eq!(probe.calls, 0);
}

#[test]
fn a_backend_failure_and_a_value_that_is_not_a_number_stop_the_run() {
    let source = mono(8000, vec![0.1; 2000]);
    let mut probe = Probe::new(shape(8000, 1, 256, 192, Weighting::Triangle, 1), |x, _| x);
    probe.fail_at = Some(3);
    match run(&source, &mut probe) {
        Err(Error::Backend { window: 3, error }) => assert_eq!(error.0, "out of memory"),
        other => panic!("{other:?}"),
    }
    let mut probe =
        Probe::new(shape(8000, 1, 256, 192, Weighting::Triangle, 1), |x, w| if w == 2 { f32::NAN } else { x });
    assert_eq!(run(&source, &mut probe), Err(Error::NotFinite { window: 2 }));
    let mut probe = Probe::new(shape(8000, 1, 256, 192, Weighting::Triangle, 1), |_, _| f32::INFINITY);
    assert_eq!(run(&source, &mut probe), Err(Error::NotFinite { window: 0 }));
}

#[test]
fn the_band_splitter_refuses_crossovers_out_of_order_or_range() {
    for crossovers in [&[2000.0, 200.0][..], &[0.0], &[-5.0], &[4000.0], &[f64::NAN], &[100.0, 100.0]] {
        assert!(BandSplitter::new(8000, 1, 256, 192, Weighting::Triangle, crossovers).is_err(), "{crossovers:?}");
    }
    let mut b = BandSplitter::new(8000, 1, 256, 192, Weighting::Triangle, &[]).unwrap();
    assert_eq!(b.shape().stems, 1);
    assert!(b.run(&[0.0; 10], &mut [0.0; 10]).is_err(), "buffers of the wrong size");
}
