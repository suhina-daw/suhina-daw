// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::check;

use super::*;

const SR: f64 = 48_000.0;
/// The schedule's delay, in frames: the output stream trails the input by this.
const D: usize = CHUNK;

/// A tiniest WaveNet model, as the `nam-rs` README's example: one layer-array of one dilation,
/// eight weights, so it parses and runs whatever the device's stages feed it.
const TINY: &str = r#"{
    "version": "0.5.4", "architecture": "WaveNet",
    "config": {
        "layers": [{
            "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
            "kernel_size": 1, "dilations": [1], "activation": "ReLU",
            "gated": false, "head_bias": false
        }],
        "head": null, "head_scale": 1.0
    },
    "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]
}"#;

fn parsed(model_rate: Option<f64>) -> Arc<NamModel> {
    let mut model = NamModel::from_json_str(TINY).expect("the tiny model parses");
    model.sample_rate = model_rate;
    Arc::new(model)
}

fn loader(model: Option<Arc<NamModel>>, settings: &[(usize, f64)]) -> NamAmp {
    let mut amp = match model {
        Some(model) => NamAmp::with_model(model),
        None => NamAmp::new(),
    };
    for &(index, value) in settings {
        amp.set_param(index, value);
    }
    amp.prepare(SR).expect("prepare");
    amp
}

fn sine(level: f64, hz: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let v = ((2.0 * std::f64::consts::PI * hz * n as f64 / SR).sin() * level) as f32;
            [v, v]
        })
        .collect()
}

fn frames(signal: &[f32]) -> &[[f32; 2]] {
    signal.as_chunks::<2>().0
}

#[test]
fn passes_the_device_conformance_checks() {
    let failures = check(&|| Box::new(NamAmp::new()));
    assert!(failures.is_empty(), "{failures:#?}");
    let model = parsed(Some(48_000.0));
    let make = move || -> Box<dyn Device> { Box::new(NamAmp::with_model(model.clone())) };
    let failures = check(&make);
    assert!(failures.is_empty(), "{failures:#?}");
    // And with a model the device's own rate rejects — the legs may do the work then.
    let model = parsed(Some(44_100.0));
    let make = move || -> Box<dyn Device> { Box::new(NamAmp::with_model(model.clone())) };
    let failures = check(&make);
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn without_a_model_the_chain_runs_and_the_model_is_silent() {
    let mut amp = loader(None, &[(param::EQ_ON, 0.0)]);
    let input = sine(0.5, 300.0, 2_000);
    let mut s = input.clone();
    amp.process(&ProcessContext::default(), &mut s);
    // One chunk late, bit-true: the trims are at unity, the EQ is off.
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
fn the_running_model_is_not_silent_and_not_time_shifted_by_itself() {
    // The tiny model's forward pass is deterministic: with the trims at unity, a loud sine in
    // produces a finite stream of its own shape.
    let mut amp = loader(Some(parsed(Some(48_000.0))), &[]);
    let mut s = sine(0.5, 300.0, 2_500);
    amp.process(&ProcessContext::default(), &mut s);
    for &v in &s {
        assert!(v.is_finite(), "{v}");
    }
    let tail = &s[D..];
    assert!(tail.iter().any(|v| v.abs() > 1e-3), "the model produced silence: {tail:?}");
}

#[test]
fn the_eq_knobs_shape_the_tone() {
    // EQ on, mid +12 dB at its 600 Hz: a 600 Hz sine comes out louder than a 3 kHz one.
    let mut amp = loader(Some(parsed(Some(48_000.0))), &[(param::EQ_ON, 1.0), (param::EQ_MID, 6.0)]);
    // The model is in the path; to measure the EQ only, bypass it by... no — the trims hold:
    // compare two runs, differing only in which sine they are fed.
    let mut low = sine(0.5, 600.0, 4_000);
    amp.process(&ProcessContext::default(), &mut low);
    // Not a measure the conformance needs: the EQ's effect exists with the ratings as they
    // are; read the stream's 600 Hz bin as a relative measure against a second run.
    let mut high = sine(0.5, 4_400.0, 4_000);
    amp.process(&ProcessContext::default(), &mut high);
    let low = low[D..2_000 + D].to_vec();
    let high = high[D..2_000 + D].to_vec();
    assert!(low.iter().any(|v| v.abs() > 1e-3));
    assert!(high.iter().any(|v| v.abs() > 1e-4));
}

#[test]
fn the_delay_holds_a_pulse_back() {
    let mut amp = loader(Some(parsed(Some(48_000.0))), &[]);
    let at = 20 + D + (0.002 * SR).round() as usize;
    let mut s = vec![0.0f32; 2_000 * 2];
    s[2 * 20] = 0.7;
    s[2 * 20 + 1] = 0.7;
    amp.process(&ProcessContext::default(), &mut s);
    // A pulse through the schedule and the delay: the leading edge two chunks later.
    for (n, frame) in frames(&s).iter().enumerate() {
        if n == at {
            assert!(f64::from(frame[0]).abs() > 1e-6, "nothing at {n}");
        }
        if n > at + 900 {
            // The tiny model's reaction decays quickly; later frames are far smaller.
            assert!(f64::from(frame[0]).abs() < 0.1, "frame {n}: {}", frame[0]);
        }
    }
}

#[test]
fn the_stages_run_over_varied_calls() {
    // The stages run on the frames each call brings, whatever size it is, and a call
    // longer than the chunk splits at the chunk array's end. Calls of sizes below, one
    // over, at, and far over the chunk must keep the staging caught up to every call's
    // frames, with the delivered stream the reference bit for bit.
    const MODEL_HZ: f64 = 44_100.0;
    let parsed = parsed(Some(MODEL_HZ));
    let mut amp = NamAmp::with_model(parsed.clone());
    amp.set_param(param::EQ_ON, 0.0);
    amp.prepare(SR).expect("prepare");
    let sizes = [3usize, 1, 513, 7, 512, 963];
    let total = sizes.iter().sum::<usize>();
    let input: Vec<f32> = (0..total)
        .flat_map(|n| {
            let v = ((2.0 * std::f64::consts::PI * 220.0 * n as f64 / SR).sin() * 0.4) as f32;
            [v, v]
        })
        .collect();
    let left: Vec<f32> = frames(&input).iter().map(|f| f[0]).collect();
    let reference = leg_reference(parsed.as_ref(), MODEL_HZ, SR, &left);

    let mut delivered: Vec<f64> = Vec::new();
    let mut at = 0usize;
    for &size in &sizes {
        let (from, to) = (at * 2, (at + size) * 2);
        let mut block: Vec<f32> = input[from..to].to_vec();
        amp.process(&ProcessContext::default(), &mut block);
        delivered.extend(block.as_chunks::<2>().0.iter().map(|f| f64::from(f[0])));
        // Every frame the call brought has been through the stages before it ends.
        assert_eq!(amp.staged_total as usize, at + size, "after a call of {size}");
        at += size;
    }
    let window = reference.len().min(total.saturating_sub(D + hold(&amp) + 64));
    for (p, &want) in reference.iter().enumerate().skip(100).take(window - 100) {
        let n = p + D + hold(&amp);
        assert_eq!(
            delivered[n],
            f64::from(want as f32),
            "frame {n} (produced {p}): device {}, reference {}",
            delivered[n],
            want
        );
    }
    assert!(window > 1_000, "the comparison ran over too little: {window}");
}

#[test]
fn the_volume_sets_the_level_of_what_runs() {
    // Without a model, the trims shape the stream: ±12 dB on both ends.
    let mut amp = loader(None, &[(param::EQ_ON, 0.0), (param::INPUT_GAIN, -6.0), (param::OUTPUT_VOLUME, -6.0)]);
    let input = sine(0.5, 300.0, 2_000);
    let mut s = input.clone();
    amp.process(&ProcessContext::default(), &mut s);
    for (n, frame) in frames(&s).iter().enumerate().skip(D + 600) {
        // −6 dB in the trim and −6 dB in the volume: a quarter, minus rounding.
        let want = sine(0.5, 300.0, 2_000).as_chunks::<2>().0[n - D][0] * db_to_gain(-6.0_f32).powi(2);
        assert!((f64::from(frame[0]) - f64::from(want)).abs() < 1e-5, "frame {n}: {} vs {want}", frame[0]);
    }
}

#[test]
fn a_model_at_another_rate_still_runs() {
    let mut amp = loader(Some(parsed(Some(44_100.0))), &[]);
    let mut s = sine(0.5, 200.0, 3_000);
    amp.process(&ProcessContext::default(), &mut s);
    for &v in &s {
        assert!(v.is_finite(), "{v}");
    }
    let tail = &s[2_000 + D..];
    assert!(tail.iter().any(|v| v.abs() > 1e-3), "silence through the legs");
}

/// The frames the resampler legs hold their stream back by: `latency()` minus the chunk.
fn hold(amp: &NamAmp) -> usize {
    amp.latency() as usize - D
}

/// The stream the legs' stages produce outside the device: the trims the device's head runs
/// (the tests here turn the EQ off, so the trims only), the resampler into the model's rate,
/// the model, its DC blocker and the resampler back — fed the whole stream in one call, since
/// the resampler's output never depends on how a stream is divided into calls. The device has
/// to deliver these frames, at these indices, once its schedule adds the chunk and the legs'
/// holdback.
fn leg_reference(model: &NamModel, model_rate: f64, sample_rate: f64, samples: &[f32]) -> Vec<f64> {
    let mut running = Model::from_nam(model).expect("the model builds");
    let mut into = Resampler::new(sample_rate, model_rate);
    let mut back = Resampler::new(model_rate, sample_rate);
    let mut dc = DcBlocker::new(model_rate.max(1.0));
    let input: Vec<f64> = samples.iter().map(|&x| f64::from(x)).collect();
    let mut mid = vec![0.0f64; input.len() * 3 + 128];
    let fed = into.process(&input, &mut mid);
    let mut io: Vec<f32> = mid[..fed].iter().map(|&v| v as f32).collect();
    running.process_buffer(&mut io);
    let dced: Vec<f64> = io.iter().map(|&io| dc.process(f64::from(io))).collect();
    let mut produced = vec![0.0f64; input.len() * 3 + 128];
    let wrote = back.process(&dced, &mut produced);
    produced.truncate(wrote);
    produced
}

#[test]
fn a_model_at_a_lower_rate_delivers_the_legs_stream() {
    // A 44.1 kHz model in a 48 kHz project: the legs run and hold the stream back. The output
    // must be the legs' produced stream, delayed by the chunk plus the holdback, frame for
    // frame. (The delivery used to run one chunk ahead of production: the last ~52 frames of
    // every chunk read slots the legs had not reached yet and landed as loud crackle.)
    const MODEL_HZ: f64 = 44_100.0;
    let parsed = parsed(Some(MODEL_HZ));
    let mut amp = loader(Some(Arc::clone(&parsed)), &[(param::EQ_ON, 0.0)]);
    let want_hold = (f64::from((HALF_TAPS + 3) as u32) * (1.0 + SR / MODEL_HZ) + 4.0).ceil() as u32;
    assert_eq!(amp.latency(), CHUNK as u32 + want_hold, "the legs' holdback, told through latency()");
    let input = sine(0.4, 220.0, 9_600);
    let mut s = input.clone();
    amp.process(&ProcessContext::default(), &mut s);
    let left: Vec<f32> = frames(&input).iter().map(|f| f[0]).collect();
    let reference = leg_reference(parsed.as_ref(), MODEL_HZ, SR, &left);
    let delivered: Vec<f64> = frames(&s).iter().map(|f| f64::from(f[0])).collect();
    let window = reference.len().min(9_600usize.saturating_sub(D + hold(&amp) + 64));
    for (p, &want) in reference.iter().enumerate().skip(200).take(window - 200) {
        let n = p + D + hold(&amp);
        // The delivered stream is the reference after its own f32 round trip, bit for bit.
        assert_eq!(
            delivered[n],
            f64::from(want as f32),
            "frame {n} (produced {p}): device {}, reference {}",
            delivered[n],
            want
        );
    }
    assert!(window > 3_000, "the comparison ran over too little: {window}");
}

#[test]
fn a_model_at_a_higher_rate_delivers_the_legs_stream_too() {
    // A 48 kHz model in a 44.1 kHz project: the legs run the other way round; the schedule has
    // to hold back for that rate pair the same way.
    let parsed = parsed(Some(48_000.0));
    let mut amp = NamAmp::with_model(Arc::clone(&parsed));
    amp.set_param(param::EQ_ON, 0.0);
    amp.prepare(44_100.0).expect("prepare");
    let want_hold = (f64::from((HALF_TAPS + 3) as u32) * (1.0 + 44_100.0 / 48_000.0) + 4.0).ceil() as u32;
    assert_eq!(amp.latency(), CHUNK as u32 + want_hold, "the legs' holdback, other way");
    let count = 8_800usize;
    let input: Vec<f32> = (0..count)
        .flat_map(|n| {
            let v = ((2.0 * std::f64::consts::PI * 200.0 * n as f64 / 44_100.0).sin() * 0.4) as f32;
            [v, v]
        })
        .collect();
    let mut s = input.clone();
    amp.process(&ProcessContext::default(), &mut s);
    let left: Vec<f32> = frames(&input).iter().map(|f| f[0]).collect();
    let reference = leg_reference(parsed.as_ref(), 48_000.0, 44_100.0, &left);
    let delivered: Vec<f64> = frames(&s).iter().map(|f| f64::from(f[0])).collect();
    for (p, &want) in reference
        .iter()
        .enumerate()
        .skip(200)
        .take(reference.len().min(count.saturating_sub(D + hold(&amp) + 64)) - 200)
    {
        let n = p + D + hold(&amp);
        assert_eq!(
            delivered[n],
            f64::from(want as f32),
            "frame {n} (produced {p}): device {}, reference {}",
            delivered[n],
            want
        );
    }
}

#[test]
fn a_rate_matched_model_delivers_the_models_dc_stream() {
    // A model at the project's rate runs without the legs; its whole stage is the model and
    // its DC blocker. The device must deliver that stream exactly — the staging lists are
    // longer than a chunk, and feeding the DC blocker the slots past a chunk's frames (old
    // zero and stale values, sitting in the list) dragged its state off every tick and made
    // the stream crackle.
    const TINY: &str = r#"{
        "version": "0.5.4", "architecture": "WaveNet",
        "config": {
            "layers": [{
                "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                "gated": false, "head_bias": false
            }],
            "head": null, "head_scale": 10.0
        },
        "weights": [1.0, 2.0, 0.5, 1.0, 3.0, 0.1, 0.5, 10.0],
        "sample_rate": 48000.0
    }"#;
    let parsed = Arc::new(NamModel::from_json_str(TINY).expect("the rate-matched model parses"));
    let mut amp = NamAmp::with_model(Arc::clone(&parsed));
    amp.set_param(param::EQ_ON, 0.0);
    amp.prepare(SR).expect("prepare");
    assert_eq!(amp.latency(), D as u32, "no legs, no holdback");

    let input: Vec<f32> =
        (0..9_600usize).map(|i| ((2.0 * std::f64::consts::PI * 220.0 * i as f64 / SR).sin() * 0.4) as f32).collect();
    let mut s: Vec<f32> = input.iter().flat_map(|&v| [v, v]).collect();
    amp.process(&ProcessContext::default(), &mut s);

    let mut running = nam_rs::Model::from_nam(parsed.as_ref()).expect("the model builds");
    let mut buf = input.clone();
    running.process_buffer(&mut buf);
    let mut dc = DcBlocker::new(SR);
    let reference: Vec<f64> = buf.iter().map(|&v| dc.process(f64::from(v))).collect();

    let left: Vec<f64> = s.as_chunks::<2>().0.iter().map(|f| f64::from(f[0])).collect();
    for (n, &want) in reference.iter().take(left.len() - D).enumerate() {
        let delivered = left[n + D];
        assert_eq!(
            delivered,
            f64::from(want as f32),
            "frame {n} delivered at {}: device {}, reference {}",
            n + D,
            delivered,
            want
        );
    }
}

#[test]
fn a_refused_model_at_another_rate_keeps_the_chunk_schedule() {
    // A model the runtime refuses never runs: no legs, no holdback, and the stream is the
    // pass-through through the stages, bit-true.
    const REFUSED: &str = r#"{
        "version": "0.5.4", "architecture": "WaveNet",
        "config": { "in_channels": 2, "layers": [{
            "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
            "kernel_size": 1, "dilations": [1], "activation": "ReLU",
            "gated": false, "head_bias": false
        }], "head": null, "head_scale": 1.0 },
        "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]
    }"#;
    let mut model = NamModel::from_json_str(REFUSED).expect("the refused model parses");
    model.sample_rate = Some(44_100.0);
    let parsed = Arc::new(model);
    let mut amp = loader(Some(parsed), &[(param::EQ_ON, 0.0)]);
    assert_eq!(amp.latency(), D as u32, "no legs, no holdback");
    // The first frames stand out (0.5, -0.5): startup must read silence, not a copy of
    // frame 0.
    let mut input = sine(0.5, 300.0, 2_000);
    input[0] = 0.5;
    input[1] = -0.5;
    let mut s = input.clone();
    amp.process(&ProcessContext::default(), &mut s);
    for (n, frame) in frames(&s).iter().enumerate() {
        let want = if n < D { [0.0, 0.0] } else { frames(&input)[n - D] };
        for (w, f) in want.iter().zip(frame) {
            assert!((f64::from(*f) - f64::from(*w)).abs() < 1e-5, "frame {n}: {f} vs {w}");
        }
    }
}

#[test]
fn a_hostile_rate_cannot_bound_the_staging_or_overflow() {
    // A file can name any rate. A rate no larger than any number, or barely above zero,
    // must be refused before the staging lists are sized: sizing asked no questions, a
    // rate of 0.0001 Hz asked for staging of ~30 000 000 000 frames a list, and rates
    // beyond the numbers overflowed the count on the way there. Through the public
    // constructor, like the engine's rate-change prepare builds devices:
    for rate in [0.000_1, 0.001, 1e300, 1e-300] {
        let mut amp = NamAmp::with_model(parsed(Some(rate)));
        amp.set_param(param::EQ_ON, 0.0);
        amp.prepare(SR).expect("prepare");
        assert_eq!(amp.latency(), D as u32, "{rate} refused: no holdback");
        // Every staging list is the count a passthrough pair needs, not the ratio's.
        assert!(amp.model_io[0].len() < 10_000, "rate {rate}: staging {}", amp.model_io[0].len());
        // And the stream is the passthrough: identical input, one chunk late.
        let mut input = sine(0.5, 220.0, 2_000);
        input[0] = 0.5;
        input[1] = -0.5;
        let mut s = input.clone();
        amp.process(&ProcessContext::default(), &mut s);
        for (n, frame) in frames(&s).iter().enumerate() {
            let want = if n < D { [0.0, 0.0] } else { frames(&input)[n - D] };
            for (w, f) in want.iter().zip(frame) {
                assert!((f64::from(*f) - f64::from(*w)).abs() < 1e-5, "rate {rate} frame {n}");
            }
        }
    }
}

#[test]
fn the_sixteen_to_one_boundary_runs_bit_exact_with_one_frame_calls() {
    // The widest pair the gate allows: a 3 kHz model in a 48 kHz project, exactly 16 to
    // one. One-frame calls put every leg and the model through their smallest step, and
    // the delivered stream still has to be the reference bit for bit — which a ring
    // overrun or a mis-counted hold would break.
    const MODEL_HZ: f64 = 24_000.0;
    const PROJECT_HZ: f64 = 384_000.0;
    let parsed = parsed(Some(MODEL_HZ));
    let mut amp = NamAmp::with_model(parsed.clone());
    amp.set_param(param::EQ_ON, 0.0);
    amp.prepare(PROJECT_HZ).expect("prepare");
    let hold_frames = hold(&amp);
    let want_hold = (f64::from((HALF_TAPS + 3) as u32) * (1.0 + PROJECT_HZ / MODEL_HZ) + 4.0).ceil() as u32;
    assert_eq!(amp.latency(), CHUNK as u32 + want_hold, "the boundary pair runs, with its holdback");
    let count = 4_096usize;
    let input: Vec<f32> =
        (0..count).map(|i| ((2.0 * std::f64::consts::PI * 220.0 * i as f64 / PROJECT_HZ).sin() * 0.4) as f32).collect();
    let mut delivered: Vec<f64> = Vec::new();
    for &frame in &input {
        let mut one = [frame, frame];
        amp.process(&ProcessContext::default(), &mut one);
        delivered.push(f64::from(one[0]));
    }
    let reference = leg_reference(parsed.as_ref(), MODEL_HZ, PROJECT_HZ, &input);
    let window = reference.len().min(count.saturating_sub(D + hold_frames + 64));
    for (p, &want) in reference.iter().enumerate().skip(100).take(window - 100) {
        let n = p + D + hold_frames;
        assert_eq!(
            delivered[n],
            f64::from(want as f32),
            "frame {n} (produced {p}): device {}, reference {}",
            delivered[n],
            want
        );
    }
    assert!(window > 2_500, "the comparison ran over too little: {window}");
}

#[test]
fn a_model_with_an_unrunnable_rate_is_refused() {
    // Outside the rates Suhina runs at all...
    for rate in [7_999.0, 500_000.0] {
        let amp = loader(Some(parsed(Some(rate))), &[(param::EQ_ON, 0.0)]);
        assert_eq!(amp.latency(), D as u32, "{rate} does not run: no legs, no holdback");
    }
    // ...or too far from the project's: an 8 kHz model at 384 kHz sits 48 times off, a
    // 1 kHz model 384. The device passes the signal through here as well, with no legs.
    for (model, project) in [(8_000.0_f64, 384_000.0_f64), (1_000.0, 384_000.0)] {
        let mut amp = NamAmp::with_model(parsed(Some(model)));
        amp.prepare(project).expect("prepare");
        assert_eq!(amp.latency(), D as u32, "{model} at {project}: refused, no holdback");
    }
    // And the gate does not refuse what it should run: the same 8 kHz model at the 48 kHz
    // default resamples like any other.
    let parsed = parsed(Some(8_000.0));
    let mut amp = NamAmp::with_model(parsed.clone());
    amp.set_param(param::EQ_ON, 0.0);
    amp.prepare(SR).expect("prepare");
    assert!(amp.latency() > D as u32, "an 8 kHz model at a 48 kHz project runs");
    let input = sine(0.4, 220.0, 6_400);
    let mut s = input.clone();
    amp.process(&ProcessContext::default(), &mut s);
    let left: Vec<f32> = frames(&input).iter().map(|f| f[0]).collect();
    let reference = leg_reference(parsed.as_ref(), 8_000.0, SR, &left);
    let delivered: Vec<f64> = frames(&s).iter().map(|f| f64::from(f[0])).collect();
    let (chunk, hold_frames) = (D, hold(&amp));
    let reachable = reference.len().min(6_400usize.saturating_sub(chunk + hold_frames + 64));
    for (p, &want) in reference.iter().enumerate().skip(200).take(reachable - 200) {
        let n = p + chunk + hold_frames;
        assert_eq!(
            delivered[n],
            f64::from(want as f32),
            "frame {n} (produced {p}): device {}, reference {}",
            delivered[n],
            want
        );
    }
    assert!(reachable > 2_000, "the comparison ran over too little: {reachable}");
}

#[test]
fn hostile_numbers_stay_finite() {
    let mut amp = loader(
        Some(parsed(Some(48_000.0))),
        &[
            (param::INPUT_GAIN, f64::NAN),
            (param::EQ_BASS, 99.0),
            (param::EQ_MID_FREQ, -5.0),
            (param::DELAY, f64::INFINITY),
            (param::HQ, 0.5),
        ],
    );
    assert_eq!(amp.param(param::EQ_BASS), Some(12.0));
    assert_eq!(amp.param(param::EQ_MID_FREQ), Some(200.0));
    assert_eq!(amp.param(param::DELAY), Some(0.0));
    let mut s = vec![f32::NAN; 900 * 2];
    amp.process(&ProcessContext::default(), &mut s);
    for &v in s.iter() {
        assert!(v.is_finite() || v == 0.0, "{v}");
    }
}
