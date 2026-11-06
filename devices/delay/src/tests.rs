// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance;

use super::*;

const SR: f64 = 48_000.0;

/// A delay at `SR` with Time mode, no filter, no feedback, fully wet, then `settings`.
fn delay(settings: &[(usize, f64)]) -> Delay {
    let mut d = Delay::new();
    let base = [
        (param::SYNC_LEFT, 0.0),
        (param::SYNC_RIGHT, 0.0),
        (param::LINK, 0.0),
        (param::FILTER, 0.0),
        (param::FEEDBACK, 0.0),
        (param::MIX, 1.0),
        (param::TIME_LEFT, 0.1),
        (param::TIME_RIGHT, 0.1),
    ];
    for &(index, value) in base.iter().chain(settings) {
        d.set_param(index, value);
    }
    let _ = d.prepare(SR);
    d
}

/// `frames` frames of `input` (interleaved, padded with silence) through `d` in blocks of 256.
fn run(d: &mut Delay, input: &[f32], frames: usize, context: ProcessContext) -> Vec<f32> {
    let mut buffer = input.to_vec();
    buffer.resize(frames * 2, 0.0);
    for block in buffer.chunks_mut(512) {
        d.process(&context, block);
    }
    buffer
}

fn impulse(left: f32, right: f32) -> Vec<f32> {
    vec![left, right]
}

fn side(signal: &[f32], right: bool) -> Vec<f32> {
    signal.as_chunks::<2>().0.iter().map(|f| f[usize::from(right)]).collect()
}

/// (frame, value) of every sample above 1e-6 in `samples`.
fn taps(samples: &[f32]) -> Vec<(usize, f32)> {
    samples.iter().enumerate().filter(|(_, v)| v.abs() > 1e-6).map(|(i, v)| (i, *v)).collect()
}

fn sine(hz: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let v = (std::f64::consts::TAU * hz * n as f64 / SR).sin() as f32 * 0.5;
            [v, v]
        })
        .collect()
}

suhina_device_api::conformance_test!(Delay::new());

#[test]
fn an_impulse_comes_back_after_the_time() {
    let mut d = delay(&[(param::TIME_RIGHT, 0.25)]);
    let out = run(&mut d, &impulse(1.0, 0.5), 20_000, ProcessContext::default());
    assert_eq!(taps(&side(&out, false)), [(4_800, 1.0)]);
    assert_eq!(taps(&side(&out, true)), [(12_000, 0.5)]);
}

#[test]
fn every_repeat_is_the_last_times_the_feedback() {
    for feedback in [0.25, 0.5, 0.95] {
        let mut d = delay(&[(param::FEEDBACK, feedback)]);
        let out = side(&run(&mut d, &impulse(1.0, 1.0), 48_000, ProcessContext::default()), false);
        let taps = taps(&out);
        assert!(taps.len() > 3, "{feedback}: {taps:?}");
        for (k, (frame, value)) in taps.iter().take(8).enumerate() {
            assert_eq!(*frame, 4_800 * (k + 1));
            assert!((f64::from(*value) - feedback.powi(k as i32)).abs() < 1e-5, "{feedback} repeat {k}: {value}");
        }
    }
}

#[test]
fn synced_times_follow_the_tempo_and_the_offset() {
    // At 120 BPM a sixteenth is 125 ms: "4" is 500 ms, and +33.3 % makes it 666.5 ms.
    let context = ProcessContext { tempo: 120.0, beat: 0.0, playing: true };
    let mut d = delay(&[
        (param::SYNC_LEFT, 1.0),
        (param::SYNC_RIGHT, 1.0),
        (param::SIXTEENTHS_LEFT, 3.0),
        (param::SIXTEENTHS_RIGHT, 0.0),
        (param::OFFSET_RIGHT, 0.333),
    ]);
    let out = run(&mut d, &impulse(1.0, 1.0), 40_000, context);
    assert_eq!(taps(&side(&out, false)), [(24_000, 1.0)]);
    let right = taps(&side(&out, true));
    let at = right.iter().map(|t| t.0).collect::<Vec<_>>();
    // 6000 × 1.333 = 7998 frames, read between frames by the cubic: a few taps around it.
    assert!(at.iter().all(|f| (7_995..=8_001).contains(f)), "{right:?}");

    // Linked, the right side takes the left side's sync step and keeps its own offset.
    d.set_param(param::LINK, 1.0);
    d.set_param(param::OFFSET_RIGHT, 0.0);
    d.reset();
    let out = run(&mut d, &impulse(1.0, 1.0), 40_000, context);
    assert_eq!(taps(&side(&out, true)), [(24_000, 1.0)]);
}

#[test]
fn the_band_pass_is_in_the_loop() {
    // Width 1 around 1 kHz: about -1.9 dB at the centre, and once more on every repeat.
    let mut d = delay(&[(param::FILTER, 1.0), (param::FILTER_WIDTH, 1.0), (param::FEEDBACK, 0.5)]);
    let out = side(&run(&mut d, &sine(1_000.0, 48_000), 48_000, ProcessContext::default()), false);
    let level = |from: usize| {
        let window = out.get(from..from + 4_800).unwrap_or_default();
        (window.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / window.len() as f64 * 2.0).sqrt()
    };
    // Steady state: the sum of every repeat, sine times g / (1 - 0.5 g) with g the filter's gain.
    let expected = 0.5 * 0.80 / (1.0 - 0.5 * 0.80);
    let steady = level(40_000);
    assert!((steady - expected).abs() < 0.03, "{steady} against {expected}");
    // Filter gain at the centre: corners at 707 Hz and 1414 Hz, two Butterworth slopes.
    let (hp, lp) =
        (Coefficients::highpass(SR, 707.1, BUTTERWORTH_Q), Coefficients::lowpass(SR, 1_414.2, BUTTERWORTH_Q));
    let g = hp.magnitude(SR, 1_000.0) * lp.magnitude(SR, 1_000.0);
    assert!((20.0 * g.log10() + 1.9).abs() < 0.2, "{}", 20.0 * g.log10());
}

#[test]
fn the_band_is_held_to_its_range() {
    let mut d = delay(&[(param::FILTER, 1.0), (param::FILTER_WIDTH, 9.0)]);
    let _ = run(&mut d, &[], 64, ProcessContext::default());
    let hp = d.sides[0].high_pass.coefficients();
    let lp = d.sides[0].low_pass.coefficients();
    assert!((hp.magnitude(SR, 50.0) - BUTTERWORTH_Q).abs() < 1e-3);
    assert!((lp.magnitude(SR, 18_000.0) - BUTTERWORTH_Q).abs() < 1e-3);
}

#[test]
fn ping_pong_alternates_the_sides() {
    let mut d = delay(&[(param::PING_PONG, 1.0), (param::FEEDBACK, 0.5)]);
    let out = run(&mut d, &impulse(1.0, 0.0), 30_000, ProcessContext::default());
    let (left, right) = (taps(&side(&out, false)), taps(&side(&out, true)));
    // The mono sum (half the left click) goes left first, then right, then left.
    assert_eq!(left, [(4_800, 0.5), (14_400, 0.125), (24_000, 0.031_25)]);
    assert_eq!(right, [(9_600, 0.25), (19_200, 0.0625), (28_800, 0.015_625)]);
}

#[test]
fn freeze_holds_the_lines_and_shuts_out_the_input() {
    let mut d = delay(&[(param::FEEDBACK, 0.5)]);
    let mut out = run(&mut d, &impulse(1.0, 1.0), 6_000, ProcessContext::default());
    d.set_param(param::FREEZE, 1.0);
    // More input while frozen does not get in.
    let mut more = vec![0.0f32; 48_000 * 2];
    more[2 * 1_500] = 1.0;
    more[2 * 2_000] = 1.0;
    for block in more.chunks_mut(512) {
        d.process(&ProcessContext::default(), block);
    }
    out.extend(more);
    let left = taps(&side(&out, false));
    // The impulse repeats at full level every 100 ms (the freeze ramp ends before the second
    // repeat); the clicks while frozen are only heard dry, as wet 0 here.
    let repeats: Vec<_> = left.iter().filter(|t| t.0 > 9_000).collect();
    assert!(repeats.len() >= 8, "{left:?}");
    for (frame, value) in repeats {
        assert_eq!(frame % 4_800, 0, "{left:?}");
        assert!((value - 0.5).abs() < 1e-6, "{left:?}");
    }
}

/// Largest step from one sample to the next in the right half of `out`.
fn largest_step(out: &[f32]) -> f32 {
    side(out, false).windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
}

#[test]
fn a_time_change_glides_fades_or_jumps() {
    for (transition, smooth) in [(0.0, true), (1.0, true), (2.0, false)] {
        let mut d = delay(&[(param::TRANSITION, transition)]);
        let tone = sine(200.0, 96_000);
        let mut out = run(&mut d, tone.get(..48_000 * 2).unwrap_or_default(), 24_000, ProcessContext::default());
        d.set_param(param::TIME_LEFT, 0.2013);
        out.extend(run(&mut d, tone.get(48_000..96_000).unwrap_or_default(), 24_000, ProcessContext::default()));
        let step = largest_step(out.get(10_000..).unwrap_or_default());
        // A 200 Hz sine at 0.5 moves at most 0.0131 per sample; a jump to another point of the
        // wave is a step of up to the whole swing.
        if smooth {
            assert!(step < 0.03, "transition {transition}: step {step}");
        } else {
            assert!(step > 0.1, "transition {transition}: step {step}");
        }
    }
}

#[test]
fn a_tempo_change_glides_the_synced_time() {
    // Synced to "4" at 120 BPM (500 ms), then the song jumps to 97 BPM: the time lengthens to
    // about 619 ms through Repitch, without a step in the 200 Hz tone.
    let mut d = delay(&[(param::SYNC_LEFT, 1.0), (param::SIXTEENTHS_LEFT, 3.0)]);
    let tone = sine(200.0, 96_000);
    let at = |tempo: f64| ProcessContext { tempo, beat: 0.0, playing: true };
    let mut out = run(&mut d, tone.get(..48_000 * 2).unwrap_or_default(), 48_000, at(120.0));
    out.extend(run(&mut d, tone.get(48_000 * 2..).unwrap_or_default(), 48_000, at(97.0)));
    let step = largest_step(out.get(30_000 * 2..).unwrap_or_default());
    assert!(step < 0.03, "step {step}");

    // Once the glide is over, an impulse comes back after the new time.
    d.reset();
    let out = run(&mut d, &impulse(1.0, 0.0), 40_000, at(97.0));
    let left = taps(&side(&out, false));
    let expected = 4.0 * 15.0 / 97.0 * SR;
    assert!(left.iter().all(|t| (t.0 as f64 - expected).abs() < 3.0), "{left:?}");
}

#[test]
fn repitch_bends_the_pitch_while_the_time_glides() {
    let mut d = delay(&[]);
    let tone = sine(1_000.0, 48_000);
    let _ = run(&mut d, &tone, 24_000, ProcessContext::default());
    d.set_param(param::TIME_LEFT, 0.2);
    let out = side(&run(&mut d, tone.get(..19_200).unwrap_or_default(), 9_600, ProcessContext::default()), false);
    // Lengthening at the rate limit reads at half speed: about 500 Hz, counted from zero crossings
    // once the glide's start has been rounded off.
    let middle = out.get(4_800..9_000).unwrap_or_default();
    let crossings = middle.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    let hz = crossings as f64 / (middle.len() as f64 / SR);
    assert!((hz - 500.0).abs() < 40.0, "{hz} Hz");
}

#[test]
fn the_lfo_moves_the_delay_time() {
    // A square LFO at 100 %: the time swings to 75 ms and 125 ms; Jump makes the clicks arrive
    // there exactly.
    let mut d =
        delay(&[(param::TRANSITION, 2.0), (param::LFO_SHAPE, 4.0), (param::LFO_RATE, 1.0), (param::LFO_TO_TIME, 1.0)]);
    let mut clicks = vec![0.0f32; 48_000 * 2];
    // The square is high for the first half second and low for the second.
    clicks[2 * 10_000] = 1.0;
    clicks[2 * 34_000] = 1.0;
    let out = taps(&side(&run(&mut d, &clicks, 48_000, ProcessContext::default()), false));
    assert_eq!(out.iter().map(|t| t.0).collect::<Vec<_>>(), [10_000 + 6_000, 34_000 + 3_600]);
}

#[test]
fn a_synced_lfo_follows_the_song_position() {
    let at = |beat: f64| {
        let mut d = Delay::new();
        d.set_param(param::LFO_MODE, 2.0);
        d.set_param(param::LFO_SYNCED, 6.0); // one bar
        let _ = d.prepare(SR);
        let _ = run(&mut d, &[], 1, ProcessContext { tempo: 120.0, beat, playing: true });
        d.lfo().at(beat / 4.0)
    };
    // A triangle a bar long is at its top a quarter of the bar in, and at its bottom three
    // quarters in.
    assert!((at(1.0) - 1.0).abs() < 1e-9);
    assert!((at(3.0) + 1.0).abs() < 1e-9);
}

#[test]
fn a_synced_lfo_runs_on_from_the_song_position_when_the_transport_stops() {
    let mut d = Delay::new();
    d.set_param(param::LFO_MODE, 2.0);
    d.set_param(param::LFO_SYNCED, 6.0); // one bar
    let _ = d.prepare(SR);
    // Playing, one beat into a bar: the triangle is at its top.
    let _ = run(&mut d, &[], 1, ProcessContext { tempo: 120.0, beat: 1.0, playing: true });
    // Stopped, the LFO goes on from there instead of from where it last ran free: after
    // half a bar (1 s at 120 bpm) it is at 3/4 of the bar, the bottom.
    let frames = SR as usize;
    let _ = run(&mut d, &[], frames, ProcessContext { tempo: 120.0, beat: 0.0, playing: false });
    let cycles = d.lfo_cycles as f64 + d.lfo_phase;
    assert!((d.lfo().at(cycles) + 1.0).abs() < 1e-3, "{cycles}");
}

#[test]
fn mix_laws() {
    for (equal, dry, wet) in [(0.0, 0.5, 0.5), (1.0, std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2)]
    {
        let mut d = delay(&[(param::MIX, 0.5), (param::EQUAL_LOUDNESS, equal)]);
        let out = side(&run(&mut d, &impulse(1.0, 1.0), 6_000, ProcessContext::default()), false);
        assert!((f64::from(out[0]) - dry).abs() < 1e-6, "{equal}");
        assert!((f64::from(out[4_800]) - wet).abs() < 1e-6, "{equal}");
    }
}

#[test]
fn the_longest_times_fit_every_sample_rate() {
    for sr in [8_000.0, 384_000.0] {
        let mut d = Delay::new();
        for (index, value) in
            [(param::SYNC_LEFT, 0.0), (param::TIME_LEFT, 5.0), (param::MIX, 1.0), (param::FEEDBACK, 0.0)]
        {
            d.set_param(index, value);
        }
        d.set_param(param::FILTER, 0.0);
        let _ = d.prepare(sr);
        let frames = (5.0 * sr) as usize + 10;
        let mut buffer = vec![0.0f32; frames * 2];
        buffer[0] = 1.0;
        for block in buffer.chunks_mut(1_024) {
            d.process(&ProcessContext::default(), block);
        }
        assert!((buffer[((5.0 * sr) as usize) * 2] - 1.0).abs() < 1e-6, "{sr}");
    }
}

#[test]
fn moving_any_control_does_not_click() {
    // A low sine through the delay, 50 % wet with feedback and the filter on; each parameter jumps
    // from one end to the other in the middle. Jump and Freeze change the sound at once by design.
    let render = |index: usize, jump: Option<f64>| {
        let mut d = delay(&[(param::MIX, 0.5), (param::FEEDBACK, 0.6), (param::FILTER, 1.0)]);
        if let Some(info) = PARAMS.get(index) {
            d.set_param(index, info.min);
        }
        let _ = d.prepare(SR);
        let mut signal = sine(110.0, 48_000);
        let (first, second) = signal.split_at_mut(24_000 * 2);
        for block in first.chunks_mut(512) {
            d.process(&ProcessContext::default(), block);
        }
        if let Some(value) = jump {
            d.set_param(index, value);
        }
        for block in second.chunks_mut(512) {
            d.process(&ProcessContext::default(), block);
        }
        let frames = signal.as_chunks::<2>().0;
        frames
            .get(23_000..34_000)
            .unwrap_or_default()
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs()))
            .fold(0.0, f32::max)
    };
    for (index, info) in PARAMS.iter().enumerate() {
        if matches!(index, param::TRANSITION | param::FREEZE) {
            continue;
        }
        let still = render(index, None);
        let moved = render(index, Some(info.max));
        assert!(moved < still * 1.5 + 0.01, "{}: a step of {moved} against {still}", info.id);
    }
}

#[test]
fn the_same_settings_sound_the_same_at_every_sample_rate() {
    // Repeats at the same times and levels, the filter's response the same, at 44.1 and 96 kHz.
    let repeats = |sr: f64| {
        let mut d = Delay::new();
        for (index, value) in [
            (param::SYNC_LEFT, 0.0),
            (param::LINK, 1.0),
            (param::TIME_LEFT, 0.25),
            (param::FEEDBACK, 0.5),
            (param::MIX, 1.0),
        ] {
            d.set_param(index, value);
        }
        let _ = d.prepare(sr);
        let frames = (1.2 * sr) as usize;
        let mut buffer = vec![0.0f32; frames * 2];
        // A 1 kHz burst of 5 ms in a Hann window: the same sound at either rate, unlike an
        // impulse, whose energy spreads up to the rate's own top.
        let burst = (0.005 * sr) as usize;
        for n in 0..burst {
            let t = n as f64 / sr;
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / burst as f64).cos();
            buffer[2 * n] = ((std::f64::consts::TAU * 1_000.0 * t).sin() * window) as f32;
        }
        for block in buffer.chunks_mut(512) {
            d.process(&ProcessContext::default(), block);
        }
        // Energy of each repeat and where its peak is, in seconds.
        let left = side(&buffer, false);
        (1..=4)
            .map(|k| {
                let at = (0.25 * k as f64 * sr) as usize;
                let window = left.get(at - (0.01 * sr) as usize..at + (0.02 * sr) as usize).unwrap_or_default();
                let energy: f64 = window.iter().map(|v| f64::from(*v).powi(2)).sum();
                let peak = window
                    .iter()
                    .enumerate()
                    .fold((0, 0.0f32), |a, (i, v)| if v.abs() > a.1 { (i, v.abs()) } else { a });
                (10.0 * energy.log10(), (at - (0.01 * sr) as usize + peak.0) as f64 / sr)
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (repeats(44_100.0), repeats(96_000.0));
    for ((ea, ta), (eb, tb)) in a.iter().zip(&b) {
        // Energy per second, so the rates compare; the peaks within a twentieth of a millisecond.
        let (ea, eb) = (ea - 10.0 * 44_100f64.log10(), eb - 10.0 * 96_000f64.log10());
        assert!((ea - eb).abs() < 0.2, "{a:?} {b:?}");
        assert!((ta - tb).abs() < 5e-5, "{a:?} {b:?}");
    }
}

/// The cost of one instance at its defaults: run with `cargo test --release -p suhina-device-delay
/// cost -- --ignored --nocapture`. The budget (docs/spec/delay.md › Cost) is checked by hand, as CI
/// machines vary too much for a timing test.
#[test]
#[ignore]
fn cost_of_a_minute_at_48_khz() {
    let mut device = Delay::new();
    let _ = device.prepare(48_000.0);
    let mut signal = conformance::test_signal(48_000 * 60);
    let start = std::time::Instant::now();
    for block in signal.chunks_mut(512) {
        device.process(&ProcessContext::default(), block);
    }
    let seconds = start.elapsed().as_secs_f64();
    println!("60 s at 48 kHz in {seconds:.3} s: {:.2} % of a core", seconds / 60.0 * 100.0);
}
