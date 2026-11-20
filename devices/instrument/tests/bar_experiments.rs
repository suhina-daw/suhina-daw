// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The F11 bar prototype's experiments E1–E4 and E7 (docs/research/instrument-f11-mallets.md › 5),
//! measured on offline renders at fixed frame counts. Each test asserts its pass criterion and
//! prints what it measured (`cargo test -p suhina-device-instrument --test bar_experiments --
//! --nocapture`); the results are recorded in docs/research/instrument-f11-bar-prototype.md.

use std::f64::consts::TAU;
use suhina_device_api::{Device, NoteEvent, ProcessContext};
use suhina_device_instrument::{Instrument, bars::Profile, param};

const MODELS: [(f64, &str); 2] = [(0.0, "Contact"), (1.0, "Pulse")];
const PROFILES: [(Profile, f64, &str, u8, u8); 2] =
    [(Profile::Marimba, 1.0, "Marimba bar", 36, 96), (Profile::Vibraphone, 2.0, "Vibraphone bar", 53, 89)];

fn bar(rate: f64, body: f64, model: f64, hardness: f64) -> Instrument {
    let mut instrument = Instrument::with_rate(rate).unwrap();
    instrument.set_param(param::REVISION, 3.0);
    instrument.set_param(param::BODY, body);
    instrument.set_param(param::MALLET_MODEL, model);
    instrument.set_param(param::HARDNESS, hardness);
    instrument.set_param(param::GAIN, 0.0);
    instrument.reset();
    instrument
}

/// The left channel of `frames` frames, with `events` sent before the frame they name.
fn render(instrument: &mut Instrument, frames: usize, events: &[(usize, NoteEvent)]) -> Vec<f32> {
    let mut left = Vec::with_capacity(frames);
    let mut frame = [0.0f32; 2];
    for n in 0..frames {
        for (at, event) in events {
            if *at == n {
                instrument.event(*event);
            }
        }
        instrument.process(&ProcessContext::default(), &mut frame);
        left.push(frame[0]);
    }
    left
}

fn hann(n: usize, len: usize) -> f64 {
    0.5 - 0.5 * (TAU * n as f64 / len as f64).cos()
}

/// Hann-windowed DFT magnitude of `signal` at `hz`.
fn magnitude(signal: &[f32], rate: f64, hz: f64) -> f64 {
    let step = TAU * hz / rate;
    let (mut re, mut im) = (0.0, 0.0);
    for (n, x) in signal.iter().enumerate() {
        let w = f64::from(*x) * hann(n, signal.len());
        re += w * (step * n as f64).cos();
        im -= w * (step * n as f64).sin();
    }
    (re * re + im * im).sqrt()
}

/// The spectral peak nearest `hz`, within ±25 cents, by a 1-cent search and a parabolic refinement.
fn peak(signal: &[f32], rate: f64, hz: f64) -> f64 {
    let at = |cents: f64| magnitude(signal, rate, hz * 2.0_f64.powf(cents / 1200.0));
    let (best, _) = (-25..=25).map(|c| (c, at(f64::from(c)))).fold((0, 0.0), |m, x| if x.1 > m.1 { x } else { m });
    let c = f64::from(best);
    let (a, b, d) = (at(c - 1.0).ln(), at(c).ln(), at(c + 1.0).ln());
    let offset = 0.5 * (a - d) / (a - 2.0 * b + d);
    hz * 2.0_f64.powf((c + if offset.is_finite() { offset.clamp(-1.0, 1.0) } else { 0.0 }) / 1200.0)
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

fn fundamental(key: u8) -> f64 {
    440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0)
}

/// Magnitude-weighted spectral centroid of the first `len` samples (Hann window, every DFT bin).
fn centroid(signal: &[f32], rate: f64, len: usize) -> f64 {
    let signal = &signal[..len];
    let (mut weighted, mut total) = (0.0, 0.0);
    for bin in 1..len / 2 {
        let hz = bin as f64 * rate / len as f64;
        let m = magnitude(signal, rate, hz);
        weighted += hz * m;
        total += m;
    }
    weighted / total
}

#[test]
fn e1_tuned_modes_sit_on_the_profile_ratios() {
    let rate = 48000.0;
    for (profile, body, name, low, high) in PROFILES {
        for (model, model_name) in MODELS {
            let mut worst: (f64, u8, usize) = (0.0, 0, 0);
            let mut measured = 0;
            for key in (low..=high).step_by(4) {
                let mut instrument = bar(rate, body, model, 0.5);
                let signal = render(&mut instrument, 24000, &[(0, NoteEvent::On { key, velocity: 0.8 })]);
                // The tuned modes: the profile's targets and the fundamental.
                let tuned = if profile == Profile::Marimba { 3 } else { 2 };
                for n in 1..=tuned {
                    let expected = fundamental(key) * profile.ratio(n, f64::from(key));
                    if expected >= rate * 0.45 * 0.95 {
                        continue;
                    }
                    let error = cents(peak(&signal, rate, expected), expected);
                    measured += 1;
                    if error.abs() > worst.0.abs() {
                        worst = (error, key, n);
                    }
                }
            }
            println!(
                "E1 {name} {model_name}: {measured} tuned modes, worst {:.3} cents (key {}, mode {})",
                worst.0, worst.1, worst.2
            );
            assert!(
                worst.0.abs() <= 5.0,
                "{name} {model_name}: mode {} of key {} is {} cents off",
                worst.2,
                worst.1,
                worst.0
            );
        }
    }
}

/// Contact must pass E2. Pulse is measured and reported but not held to it: a half-sine force's
/// spectrum has zeros (the first at 1.5 / duration) that sweep across the bar's upper modes as
/// the contact shortens, so its centroid is not monotonic for soft mallets
/// (docs/research/instrument-f11-bar-prototype.md › E2).
#[test]
fn e2_brightness_rises_with_velocity_and_hardness() {
    let rate = 48000.0;
    let len = 2400; // 50 ms
    for (_, body, name, ..) in PROFILES {
        for (model, model_name) in MODELS {
            let grid: Vec<Vec<f64>> = [0.0, 0.25, 0.5, 0.75, 1.0]
                .into_iter()
                .map(|hardness| {
                    let row: Vec<f64> = [0.1, 0.25, 0.4, 0.55, 0.7, 0.85, 1.0]
                        .into_iter()
                        .map(|velocity| {
                            let mut instrument = bar(rate, body, model, hardness);
                            let signal = render(&mut instrument, len, &[(0, NoteEvent::On { key: 60, velocity })]);
                            centroid(&signal, rate, len)
                        })
                        .collect();
                    println!(
                        "E2 {name} {model_name} hardness {hardness:.2}: centroid Hz by velocity 0.1..1.0 = {:?}",
                        row.iter().map(|c| c.round() as i64).collect::<Vec<_>>()
                    );
                    row
                })
                .collect();
            let by_velocity = grid.iter().filter(|row| row.windows(2).all(|w| w[1] > w[0])).count();
            let by_hardness = (0..grid[0].len())
                .filter(|&column| grid.windows(2).all(|rows| rows[1][column] > rows[0][column]))
                .count();
            println!(
                "E2 {name} {model_name}: monotonic in velocity at {by_velocity}/{} hardnesses, in hardness at {by_hardness}/{} velocities",
                grid.len(),
                grid[0].len()
            );
            if model == 0.0 {
                assert_eq!((by_velocity, by_hardness), (grid.len(), grid[0].len()), "{name} Contact");
            }
        }
    }
}

#[test]
fn e3_a_repeated_stroke_meets_the_moving_bar() {
    let rate = 48000.0;
    let frames = 24000;
    for (_, body, name, ..) in PROFILES {
        for (model, model_name) in MODELS {
            let mut differences = Vec::new();
            for interval_ms in [2.0, 5.0, 11.0, 23.0, 47.0, 96.0] {
                let second = (interval_ms * rate / 1000.0) as usize;
                let stroke = |at: usize| (at, NoteEvent::On { key: 60, velocity: 0.8 });
                let both = render(&mut bar(rate, body, model, 0.5), frames, &[stroke(0), stroke(second)]);
                let first = render(&mut bar(rate, body, model, 0.5), frames, &[stroke(0)]);
                let alone = render(&mut bar(rate, body, model, 0.5), frames, &[stroke(second)]);
                let (mut error, mut energy) = (0.0, 0.0);
                for n in 0..frames {
                    let sum = f64::from(first[n]) + f64::from(alone[n]);
                    error += (f64::from(both[n]) - sum).powi(2);
                    energy += sum * sum;
                }
                assert!(both.iter().all(|x| x.is_finite() && x.abs() < 4.0));
                differences.push((interval_ms, (error / energy).sqrt()));
            }
            println!(
                "E3 {name} {model_name}: relative RMS difference from superposition by interval (ms) = {:?}",
                differences.iter().map(|(ms, d)| format!("{ms}: {d:.2e}")).collect::<Vec<_>>()
            );
            if model == 0.0 {
                assert!(
                    differences.iter().all(|(_, d)| *d > 1e-3),
                    "{name}: Contact must interact with the moving bar"
                );
            } else {
                assert!(differences.iter().all(|(_, d)| *d < 1e-5), "{name}: Pulse superposes");
            }
        }
    }
}

/// E3's energy half on the bar itself: the bar never holds more than the strokes brought.
#[test]
fn e3_a_roll_never_gives_the_bar_more_energy_than_the_mallet_brought() {
    use suhina_dsp::bar::{Excitation, StruckBar};
    for (profile, _, name, ..) in PROFILES {
        for (excitation, model_name) in [(Excitation::Contact, "Contact"), (Excitation::Pulse, "Pulse")] {
            let mut worst: f64 = 0.0;
            for hardness in [0.0, 1.0] {
                for strike in [0.02, 0.25, 0.98] {
                    let mallet = suhina_device_instrument::bars::mallet(hardness);
                    let mut bar = StruckBar::<16>::default();
                    bar.set_modes(48000.0, &profile.modes::<16>(60, 16, 2.0, 0.5, strike));
                    let mut injected = 0.0;
                    for n in 0..48000 {
                        // A roll: alternating strokes every 61 ms with uneven speeds.
                        if n % 2928 == 0 {
                            let speed = if (n / 2928) % 2 == 0 { 4.0 } else { 2.5 };
                            bar.strike(excitation, mallet, speed);
                            injected += 0.5 * mallet.mass * speed * speed;
                        }
                        assert!(bar.next_sample().is_finite());
                        worst = worst.max(bar.energy() / injected);
                    }
                }
            }
            println!("E3 {name} {model_name}: largest bar energy / injected mallet energy during rolls = {worst:.4}");
            assert!(worst <= 1.01, "{name} {model_name}: {worst}");
        }
    }
}

#[test]
fn e4_strike_position_follows_bar_mode_shapes() {
    let rate = 48000.0;
    for (profile, body, name, ..) in PROFILES {
        for (model, model_name) in MODELS {
            let mode_two = fundamental(60) * profile.ratio(2, 60.0);
            let level = |strike: f64| {
                let mut instrument = bar(rate, body, model, 0.5);
                instrument.set_param(param::STRIKE, strike);
                let signal = render(&mut instrument, 9600, &[(0, NoteEvent::On { key: 60, velocity: 0.8 })]);
                magnitude(&signal, rate, mode_two)
            };
            // The lowest Strike position is the centre; a quarter of the length is x = 0.25.
            let quarter = 0.98 - 0.96 * 2.0 * 0.25;
            let difference = 20.0 * (level(quarter) / level(0.02)).log10();
            println!("E4 {name} {model_name}: mode 2 is {difference:.1} dB weaker at the centre than at a quarter");
            assert!(difference >= 20.0, "{name} {model_name}: {difference} dB");
        }
    }
}

#[test]
fn e7_low_rates_and_extreme_settings_stay_finite_and_decay() {
    let mut largest: f32 = 0.0;
    for rate in [8000.0, 22050.0] {
        for (_, body, ..) in PROFILES {
            for (model, _) in MODELS {
                let mut instrument = bar(rate, body, model, 1.0);
                for (index, value) in [
                    (param::DECAY, 20.0),
                    (param::BRIGHTNESS, 1.0),
                    (param::STRIKE, 0.98),
                    (param::HARDNESS, f64::NAN),
                    (param::HARDNESS, 7.0),
                    (param::BODY, f64::INFINITY),
                    (param::MALLET_MODEL, -3.0),
                ] {
                    instrument.set_param(index, value);
                }
                // Every key at once, then a dense roll on every key, with bends.
                let mut events: Vec<(usize, NoteEvent)> =
                    (0..=127).map(|key| (0, NoteEvent::On { key, velocity: 1.0 })).collect();
                for n in 1..40 {
                    for key in (0..=127).step_by(5) {
                        events.push((n * 97, NoteEvent::On { key, velocity: 1.0 }));
                    }
                    events.push((n * 97, NoteEvent::PitchBend(if n % 2 == 0 { 1.0 } else { -1.0 })));
                }
                let frames = (rate * 3.0) as usize;
                let signal = render(&mut instrument, frames, &events);
                assert!(signal.iter().all(|x| x.is_finite()));
                largest = largest.max(signal.iter().fold(0.0, |m, x| m.max(x.abs())));
                let tail = &signal[frames - 4000..];
                let roll = &signal[2000..6000];
                let rms = |s: &[f32]| (s.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / s.len() as f64).sqrt();
                assert!(rms(tail) < rms(roll), "the bars decay after the roll");
            }
        }
    }
    println!(
        "E7 hardest mallet, velocity 1, all keys and rolls at 8 and 22.05 kHz: finite, decaying, peak {largest:.3}"
    );
    assert!(largest < 16.0);
}

#[test]
fn bar_levels_are_comparable_with_struck() {
    let rate = 48000.0;
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let mut struck = Instrument::with_rate(rate).unwrap();
    struck.set_param(param::GAIN, 0.0);
    struck.reset();
    let reference = peak(&render(&mut struck, 9600, &[(0, NoteEvent::On { key: 60, velocity: 1.0 })]));
    for (_, body, name, low, high) in PROFILES {
        for (model, model_name) in MODELS {
            let levels: Vec<f32> = [low, 60, high]
                .into_iter()
                .map(|key| {
                    peak(&render(&mut bar(rate, body, model, 0.5), 9600, &[(0, NoteEvent::On { key, velocity: 1.0 })]))
                })
                .collect();
            println!("level {name} {model_name}: peaks at keys {low}, 60, {high} = {levels:?} (Struck {reference:.3})");
            assert!(levels.iter().all(|l| *l > reference * 0.25 && *l < reference * 8.0));
        }
    }
}
