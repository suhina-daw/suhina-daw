// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use suhina_device_api::{Device, NoteEvent, ProcessContext, SampleSource};
use suhina_dsp::continuation::{Continuation, NOISE_POINTS, Partial};

use super::ContinuationControls;
use crate::sample_instrument::{INFO, Sampler};

const RATE: f64 = 48000.0;

/// One second of a 220 Hz tone (root key 57), and a continuation of it: one partial matching
/// the tone, taking over at 0.6 s.
fn setup(enabled: bool) -> Sampler {
    let frames = RATE as usize;
    let tone: Vec<f32> = (0..frames)
        .flat_map(|n| {
            let x = (0.3 * (std::f64::consts::TAU * 220.0 * n as f64 / RATE).sin()) as f32;
            [x, x]
        })
        .collect();
    let mut sampler = Sampler::with_rate(RATE).unwrap();
    sampler.load_sample(RATE as u32, 2, tone.into());
    for (id, value) in [("playback", 4.0), ("root", 57.0), ("loop_start", 0.2), ("loop_end", 0.6), ("release", 0.1)] {
        sampler.set_param(INFO.param_index(id).unwrap(), value);
    }
    let continuation = Continuation {
        recording_rate: RATE as f32,
        f0: 220.0,
        vibrato_hz: 5.5,
        vibrato_cents: 10.0,
        vibrato_phase: 0.0,
        phase: 0.0,
        deviation_phase: 0.0,
        join: 0.6,
        release_start: 0.0,
        jitter_ar: [0.0; 4],
        jitter_innovation: 0.0,
        jitter_cents: 0.0,
        jitter_gain: 0.0,
        level_db: 0.0,
        coef_target: [0.0; 3],
        coef_model: [0.0; 3],
        noise_db: [0.0; NOISE_POINTS],
        noise_rms: 0.0,
        side_noise_db: [0.0; NOISE_POINTS],
        side_noise_rms: 0.0,
        stereo: false,
        partials: vec![Partial { carrier: [0.3, 0.0], mean_log: 0.3_f32.ln(), ..Partial::NONE }],
    };
    continuation.validate().unwrap();
    sampler.set_continuation(Some(Arc::new(continuation)), None);
    sampler.set_continuation_controls(ContinuationControls { enabled, ..ContinuationControls::default() });
    sampler.prepare(RATE).unwrap();
    sampler
}

fn render(sampler: &mut Sampler, seconds: f64, block: usize) -> Vec<f32> {
    let context = ProcessContext::default();
    let mut out = Vec::new();
    let total = (seconds * RATE) as usize * 2;
    let mut buffer = vec![0.0; block * 2];
    while out.len() < total {
        let length = (total - out.len()).min(block * 2);
        let part = &mut buffer[..length];
        part.fill(0.0);
        sampler.process(&context, part);
        out.extend_from_slice(part);
    }
    out
}

fn rms(values: &[f32]) -> f64 {
    (values.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / values.len().max(1) as f64).sqrt()
}

#[test]
fn a_held_note_continues_past_its_recording() {
    let mut sampler = setup(true);
    assert!(sampler.has_continuation());
    sampler.event(NoteEvent::On { key: 57, velocity: 1.0 });
    let out = render(&mut sampler, 3.0, 256);
    assert!(out.iter().all(|v| v.is_finite()));
    // Two seconds after the recording ended, the note still sounds near its level.
    let late = &out[(2.0 * RATE) as usize * 2..];
    let early = &out[(0.3 * RATE) as usize * 2..(0.5 * RATE) as usize * 2];
    let ratio = 20.0 * (rms(late) / rms(early)).log10();
    assert!(ratio.abs() < 4.0, "the continuation is {ratio} dB from the recording");
    // Released, it fades out.
    sampler.event(NoteEvent::Off { key: 57, velocity: 0.0 });
    let after = render(&mut sampler, 1.0, 256);
    assert!(rms(&after[(0.8 * RATE) as usize * 2..]) < 1e-3);
}

#[test]
fn a_continuing_note_renders_the_same_for_every_block_split() {
    let mut a = setup(true);
    a.event(NoteEvent::On { key: 57, velocity: 1.0 });
    let first = render(&mut a, 2.0, 64);
    let mut b = setup(true);
    b.event(NoteEvent::On { key: 57, velocity: 1.0 });
    let second = render(&mut b, 2.0, 500);
    assert_eq!(first, second);
}

#[test]
fn without_continue_the_note_holds_in_segments() {
    let mut sampler = setup(false);
    sampler.event(NoteEvent::On { key: 57, velocity: 1.0 });
    let out = render(&mut sampler, 2.0, 256);
    assert!(out.iter().all(|v| v.is_finite()));
    // Sustain's segments hold the note too: what continues is the recording's own sound.
    assert!(rms(&out[(1.5 * RATE) as usize * 2..]) > 0.05);
}
