// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;
use std::f64::consts::TAU;
use suhina_dsp::modal::model::{ModelMode, ModelRecording};

const RATE: f64 = 48000.0;

fn recording(attack: Vec<f32>) -> ModelRecording {
    ModelRecording {
        key: 60,
        source: 0,
        velocity: 0.8,
        level: 0.5,
        modes: vec![
            ModelMode { phase: 0.9, ..ModelMode::plain(1.0, 0.7, 0.6) },
            ModelMode { phase: -0.4, ..ModelMode::plain(3.1, 0.2, 0.3) },
        ],
        attack_rate: if attack.is_empty() { 0 } else { 48000 },
        attack,
    }
}

fn core(set: ModelSet) -> Instrument {
    let mut core = Instrument::with_rate(RATE).unwrap();
    core.load_model_state(&set.encode()).unwrap();
    core.set_param(param::GAIN, 0.0);
    // The recording as fitted: no envelope attack, no velocity colouring.
    core.set_param(param::ATTACK, 0.0);
    core.set_param(param::VELOCITY_BRIGHTNESS, 0.0);
    core.reset();
    core
}

fn play(core: &mut Instrument, key: u8, velocity: f32, frames: usize, block: usize) -> Vec<f32> {
    core.event(NoteEvent::On { key, velocity });
    let mut output = vec![0.0; frames * 2];
    for buffer in output.chunks_mut(block * 2) {
        core.process(&ProcessContext::default(), buffer);
    }
    output.chunks(2).map(|frame| frame[0]).collect()
}

#[test]
fn a_fitted_recording_plays_its_phased_modes_at_its_level() {
    let set = ModelSet::new(vec![recording(Vec::new())]).unwrap();
    let mut instrument = core(set);
    assert!(instrument.model_supported());
    assert_eq!(instrument.param(param::REVISION), Some(4.0));
    let output = play(&mut instrument, 60, 1.0, 4800, 64);
    let c3 = 440.0 * 2.0_f64.powf(-9.0 / 12.0);
    // After the 0.25 ms gate: velocity * level * the modes with their phases.
    for (n, sample) in output.iter().enumerate().skip(32) {
        let t = n as f64;
        let expected = 0.5
            * [(1.0, 0.7, 0.6, 0.9), (3.1, 0.2, 0.3, -0.4)]
                .iter()
                .map(|(ratio, amplitude, t60, phase)| {
                    amplitude
                        * (-1000.0_f64.ln() * t / (RATE * t60)).exp()
                        * (TAU * c3 * ratio * t / RATE + phase).sin()
                })
                .sum::<f64>();
        assert!((f64::from(*sample) - expected).abs() < 1e-4, "{n}: {sample} vs {expected}");
    }
    // Half the velocity, half the level.
    let mut softer = core(ModelSet::new(vec![recording(Vec::new())]).unwrap());
    let half = play(&mut softer, 60, 0.5, 4800, 64);
    for (a, b) in output.iter().zip(&half).skip(32) {
        assert!((a * 0.5 - b).abs() < 1e-6);
    }
}

#[test]
fn the_attack_plays_from_the_strike_at_the_keys_rate() {
    let attack: Vec<f32> = (0..2000).map(|i| 0.3 * ((i as f32) * 0.05).sin()).collect();
    // A nearly silent mode, so the output is the attack.
    let quiet = |attack: Vec<f32>| {
        let mut r = recording(attack);
        r.modes = vec![ModelMode::plain(1.0, 1e-9, 0.01)];
        ModelSet::new(vec![r]).unwrap()
    };
    let mut at_root = core(quiet(attack.clone()));
    let output = play(&mut at_root, 60, 1.0, 2400, 128);
    for (n, (out, expected)) in output.iter().zip(&attack).enumerate().take(1990).skip(32) {
        assert!((out - 0.5 * expected).abs() < 1e-5, "{n}");
    }
    assert!(output[2100..].iter().all(|x| x.abs() < 1e-6), "the attack ends");
    // An octave up reads it twice as fast.
    let mut octave = core(quiet(attack.clone()));
    let output = play(&mut octave, 72, 1.0, 1200, 128);
    for (n, (out, expected)) in output.iter().zip(attack.iter().step_by(2)).enumerate().take(990).skip(32) {
        assert!((out - 0.5 * expected).abs() < 1e-5, "{n}");
    }
}

#[test]
fn revision_four_rendering_does_not_depend_on_block_size() {
    let attack: Vec<f32> = (0..3000).map(|i| ((i as f32) * 0.37).sin() * 0.1).collect();
    let set = ModelSet::new(vec![recording(attack)]).unwrap();
    let reference = play(&mut core(set.clone()), 64, 0.9, 8192, 8192);
    for block in [1, 17, 64, 513] {
        assert_eq!(play(&mut core(set.clone()), 64, 0.9, 8192, block), reference, "{block}");
    }
    assert!(reference.iter().all(|x| x.is_finite()) && reference.iter().any(|x| x.abs() > 1e-3));
}

#[test]
fn malformed_sets_are_silent_and_a_set_replaces_a_profile() {
    let mut bytes = ModelSet::new(vec![recording(Vec::new())]).unwrap().encode();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x7f;
    let mut core = Instrument::with_rate(RATE).unwrap();
    assert!(core.load_model_state(&bytes).is_err());
    core.set_param(param::REVISION, 4.0);
    assert!(!core.model_supported());
    let output = play(&mut core, 60, 1.0, 1024, 64);
    assert!(output.iter().all(|x| *x == 0.0));
    // A valid set after a profile: revision 4, the profile gone.
    let profile = suhina_dsp::modal::profile::ModalProfile::new(
        60,
        261.6,
        &[suhina_dsp::modal::profile::ProfileMode { ratio: 1.0, magnitude: 0.5, decay_seconds: 1.0 }],
    )
    .unwrap();
    core.set_modal_profile(profile);
    assert_eq!(core.param(param::REVISION), Some(2.0));
    core.load_model_state(&ModelSet::new(vec![recording(Vec::new())]).unwrap().encode()).unwrap();
    assert!(core.modal_profile().is_none() && core.model_set().is_some() && core.model_supported());
    // A revision that does not match the loaded state stays silent.
    core.set_param(param::REVISION, 2.0);
    assert!(!core.model_supported());
}

fn long_ringing() -> ModelSet {
    let mut r = recording(Vec::new());
    r.modes = vec![ModelMode::plain(1.0, 0.8, 5.0)];
    ModelSet::new(vec![r]).unwrap()
}

fn render(core: &mut Instrument, frames: usize) -> Vec<f32> {
    let mut output = vec![0.0; frames * 2];
    for buffer in output.chunks_mut(256) {
        core.process(&ProcessContext::default(), buffer);
    }
    output.chunks(2).map(|frame| frame[0]).collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |p, x| p.max(x.abs()))
}

#[test]
fn note_off_lands_a_damper_on_fitted_modes() {
    let mut damped = core(long_ringing());
    damped.set_param(param::RELEASE, 0.1);
    damped.set_param(param::DAMPER_NOISE, 0.0);
    damped.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let before = render(&mut damped, 2400);
    damped.event(NoteEvent::Off { key: 60, velocity: 0.0 });
    let after = render(&mut damped, 9600);
    // A 0.1 s T60 cap: 0.2 s later the note is 120 dB down, where without the damper it rings on.
    assert!(peak(&after[9000..]) < peak(&before[1200..]) * 1e-5, "{}", peak(&after[9000..]));
    let mut held = core(long_ringing());
    held.event(NoteEvent::On { key: 60, velocity: 1.0 });
    render(&mut held, 2400);
    assert!(peak(&render(&mut held, 9600)[9000..]) > peak(&before[1200..]) * 0.5);
    // The sustain pedal keeps the damper up until it lifts.
    let mut pedal = core(long_ringing());
    pedal.set_param(param::RELEASE, 0.1);
    pedal.event(NoteEvent::Controller { number: 64, value: 1.0 });
    pedal.event(NoteEvent::On { key: 60, velocity: 1.0 });
    render(&mut pedal, 2400);
    pedal.event(NoteEvent::Off { key: 60, velocity: 0.0 });
    assert!(peak(&render(&mut pedal, 9600)[9000..]) > peak(&before[1200..]) * 0.5);
    pedal.event(NoteEvent::Controller { number: 64, value: 0.0 });
    assert!(peak(&render(&mut pedal, 9600)[9000..]) < peak(&before[1200..]) * 1e-5);
}

#[test]
fn a_landing_damper_makes_a_short_noise() {
    let release_tail = |noise: f64| {
        let mut core = core(long_ringing());
        core.set_param(param::RELEASE, 0.005);
        core.set_param(param::DAMPER_NOISE, noise);
        core.event(NoteEvent::On { key: 60, velocity: 1.0 });
        render(&mut core, 2400);
        core.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        render(&mut core, 4800)
    };
    let (quiet, noisy) = (release_tail(0.0), release_tail(1.0));
    let difference: Vec<f32> = quiet.iter().zip(&noisy).map(|(a, b)| b - a).collect();
    assert!(peak(&difference[..1900]) > 1e-4, "the damper's noise sounds");
    assert!(peak(&difference[1930..]) < 1e-6, "and ends after 40 ms");
    assert_eq!(release_tail(1.0), noisy, "deterministic");
}

#[test]
fn the_envelope_shapes_fitted_notes() {
    let reference = play(&mut core(long_ringing()), 60, 1.0, 4800, 128);
    let mut slow = core(long_ringing());
    slow.set_param(param::ATTACK, 0.01);
    let ramped = play(&mut slow, 60, 1.0, 4800, 128);
    // A linear 10 ms attack: half way up after 240 samples.
    for n in [120, 240, 360] {
        let expected = reference[n] * (n + 1) as f32 / 480.0;
        assert!((ramped[n] - expected).abs() < 2e-3 * reference[n].abs().max(0.01), "{n}");
    }
    let mut sustain = core(long_ringing());
    sustain.set_param(param::ENVELOPE_DECAY, 0.01);
    sustain.set_param(param::SUSTAIN, 0.5);
    let held = play(&mut sustain, 60, 1.0, 4800, 128);
    for n in [2000, 3000, 4000] {
        assert!((held[n] - 0.5 * reference[n]).abs() < 1e-4, "{n}");
    }
}

#[test]
fn the_soft_pedal_strikes_quieter_and_darker() {
    let loud = play(&mut core(ModelSet::new(vec![recording(Vec::new())]).unwrap()), 60, 1.0, 4800, 128);
    let mut pedal = core(ModelSet::new(vec![recording(Vec::new())]).unwrap());
    pedal.event(NoteEvent::Controller { number: 67, value: 1.0 });
    let soft = play(&mut pedal, 60, 1.0, 4800, 128);
    let energy = |x: &[f32]| x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    assert!(energy(&soft) < 0.6 * energy(&loud));
    // The Play page's switch does the same without a pedal.
    let mut switch = core(ModelSet::new(vec![recording(Vec::new())]).unwrap());
    switch.set_param(param::SOFT_PEDAL, 1.0);
    assert_eq!(play(&mut switch, 60, 1.0, 4800, 128), soft);
}
