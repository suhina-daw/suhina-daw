// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::patch::{Patch, SysexError, read, write_bank, write_voice};

const SR: f64 = 48_000.0;

fn synth() -> FmSynth {
    let mut s = FmSynth::new();
    let _ = s.prepare(SR);
    s
}

fn index(id: &str) -> usize {
    INFO.param_index(id).unwrap_or(usize::MAX)
}

/// Plays `events` (frame, event) for `frames` frames, splitting blocks of 512 at events.
fn play(s: &mut FmSynth, events: &[(usize, NoteEvent)], frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    let mut frame = 0;
    let mut next = 0;
    while frame < frames {
        while let Some((at, e)) = events.get(next)
            && *at <= frame
        {
            s.event(*e);
            next += 1;
        }
        let until = events.get(next).map_or(frames, |(at, _)| (*at).min(frames));
        let size = 512.min(until - frame).max(1).min(frames - frame);
        s.process(&ProcessContext::default(), &mut out[frame * 2..(frame + size) * 2]);
        frame += size;
    }
    out
}

fn on(key: u8, velocity: f32) -> NoteEvent {
    NoteEvent::On { key, velocity }
}

fn off(key: u8) -> NoteEvent {
    NoteEvent::Off { key, velocity: 0.5 }
}

fn left(signal: &[f32]) -> Vec<f32> {
    signal.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect()
}

fn peak(signal: &[f32]) -> f32 {
    signal.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// Frequency from upward zero crossings over `signal` (mono).
fn frequency(signal: &[f32], sample_rate: f64) -> f64 {
    let crossings: Vec<usize> =
        signal.windows(2).enumerate().filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0).map(|(i, _)| i).collect();
    let (Some(first), Some(last)) = (crossings.first(), crossings.last()) else { return 0.0 };
    (crossings.len() - 1) as f64 * sample_rate / (last - first) as f64
}

suhina_device_api::conformance_test!(FmSynth::new());

#[test]
fn the_initial_voice_is_a_sine_at_the_key_pitch() {
    let mut s = synth();
    let out = left(&play(&mut s, &[(0, on(69, 1.0))], 48_000));
    let steady = &out[4_800..];
    assert!((frequency(steady, SR) - 440.0).abs() < 0.5, "{}", frequency(steady, SR));
    // One operator at level 99 is an eighth of full scale (-18 dB), as in the original, which
    // leaves room for six operators and sixteen voices.
    assert!((0.12..=0.13).contains(&peak(steady)), "{}", peak(steady));
    let out = left(&play(&mut synth(), &[(0, on(57, 1.0))], 48_000));
    assert!((frequency(&out[4_800..], SR) - 220.0).abs() < 0.5);
}

#[test]
fn ratio_fixed_frequency_detune_and_transpose_set_the_pitch() {
    let mut s = synth();
    s.set_param(index("op1.coarse"), 2.0);
    let out = left(&play(&mut s, &[(0, on(69, 1.0))], 24_000));
    assert!((frequency(&out[4_800..], SR) - 880.0).abs() < 1.0);

    // Fixed: coarse 2 is 100 Hz, whatever the key.
    let mut s = synth();
    s.set_param(index("op1.mode"), 1.0);
    s.set_param(index("op1.coarse"), 2.0);
    let out = left(&play(&mut s, &[(0, on(30, 1.0))], 48_000));
    assert!((frequency(&out[4_800..], SR) - 100.0).abs() < 0.5, "{}", frequency(&out[4_800..], SR));

    let mut s = synth();
    s.set_param(index("transpose"), 12.0);
    let out = left(&play(&mut s, &[(0, on(57, 1.0))], 24_000));
    assert!((frequency(&out[4_800..], SR) - 440.0).abs() < 0.5);

    let mut s = synth();
    s.set_param(index("op1.detune"), 7.0);
    let out = left(&play(&mut s, &[(0, on(69, 1.0))], 48_000));
    let f = frequency(&out[4_800..], SR);
    assert!(f > 440.2 && f < 442.0, "{f}");
}

#[test]
fn a_released_note_fades_and_the_voice_is_free_again() {
    let mut s = synth();
    s.set_param(index("op1.rate4"), 60.0);
    let out = left(&play(&mut s, &[(0, on(60, 1.0)), (4_800, off(60))], 48_000));
    assert!(peak(&out[..4_800]) > 0.1);
    assert!(peak(&out[40_000..]) < 1e-4, "{}", peak(&out[40_000..]));
    assert!(s.slots.iter().all(|slot| !slot.note.is_playing()));
}

#[test]
fn velocity_sensitivity_makes_soft_notes_quieter() {
    let level = |velocity: f32| {
        let mut s = synth();
        s.set_param(index("op1.velocitySens"), 7.0);
        peak(&left(&play(&mut s, &[(0, on(60, velocity))], 9_600))[4_800..])
    };
    let (soft, loud) = (level(0.2), level(1.0));
    assert!(soft < loud * 0.5, "{soft} {loud}");
}

#[test]
fn the_sustain_pedal_holds_released_notes() {
    let events = [
        (0, NoteEvent::Controller { number: controller::SUSTAIN, value: 1.0 }),
        (0, on(60, 1.0)),
        (2_000, off(60)),
        (30_000, NoteEvent::Controller { number: controller::SUSTAIN, value: 0.0 }),
    ];
    let out = left(&play(&mut synth(), &events, 48_000));
    assert!(peak(&out[25_000..30_000]) > 0.1);
    assert!(peak(&out[40_000..]) < 1e-3);
}

#[test]
fn more_notes_than_voices_steal_the_oldest() {
    let events: Vec<(usize, NoteEvent)> = (0..20u8).map(|i| (usize::from(i) * 64, on(40 + i, 1.0))).collect();
    let mut s = synth();
    let out = play(&mut s, &events, 4_800);
    assert!(out.iter().all(|x| x.is_finite()));
    let keys: Vec<u8> = s.slots.iter().filter(|slot| slot.down).map(|slot| slot.key).collect();
    assert_eq!(keys.len(), 16);
    // The four oldest (40 to 43) were taken over.
    assert!((44..60).all(|k| keys.contains(&k)), "{keys:?}");
}

#[test]
fn a_voice_parameter_change_reaches_a_held_note() {
    let mut s = synth();
    let first = left(&play(&mut s, &[(0, on(69, 1.0))], 12_000));
    s.set_param(index("op1.coarse"), 2.0);
    let second = left(&play(&mut s, &[], 12_000));
    assert!((frequency(&first[4_800..], SR) - 440.0).abs() < 0.5);
    assert!((frequency(&second[1_000..], SR) - 880.0).abs() < 1.0);
    // The envelope kept its stage: no new attack, the level stays where it was.
    assert!((peak(&second[1_000..]) - peak(&first[4_800..])).abs() < 0.05);
}

#[test]
fn volume_scales_the_output() {
    let mut loud = synth();
    let mut quiet = synth();
    quiet.set_param(index("volume"), -20.0);
    quiet.reset();
    let a = peak(&play(&mut loud, &[(0, on(60, 1.0))], 9_600)[4_800..]);
    let b = peak(&play(&mut quiet, &[(0, on(60, 1.0))], 9_600)[4_800..]);
    assert!((b / a - 0.1).abs() < 0.005, "{a} {b}");
}

// --- DX7 voice data --------------------------------------------------------------------------

fn sample_patch() -> Patch {
    let mut s = synth();
    for (id, value) in
        [("algorithm", 5.0), ("feedback", 6.0), ("op2.coarse", 14.0), ("op3.detune", -3.0), ("op4.on", 0.0)]
    {
        s.set_param(index(id), value);
    }
    let mut p = s.patch();
    p.set_name("TEST VOICE");
    p
}

#[test]
fn a_voice_survives_a_single_voice_dump_and_a_bank() {
    let p = sample_patch();
    let dump = write_voice(&p);
    assert_eq!(dump.len(), 163);
    let back = read(&dump).unwrap_or_else(|e| panic!("{e}"));
    assert!(back.checksum_ok);
    let [one] = back.patches.as_slice() else { panic!("{:?}", back.patches) };
    assert_eq!(one.to_voice(), p.to_voice());
    assert_eq!(one.name(), "TEST VOICE");

    let bank = write_bank(&[Patch::default(), p]);
    assert_eq!(bank.len(), 4_104);
    let back = read(&bank).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(back.patches.len(), 32);
    assert_eq!(back.patches[1].to_voice(), p.to_voice());
    assert_eq!(back.patches[0], Patch::default());
    // The bare 4096 bytes of a bank read the same.
    assert_eq!(read(&bank[6..4_102]).map(|v| v.patches), Ok(back.patches));
}

#[test]
fn loading_a_voice_sets_the_parameters() {
    let p = sample_patch();
    let mut s = FmSynth::new();
    s.load_patch(&p);
    assert_eq!(s.param(index("algorithm")), Some(5.0));
    assert_eq!(s.param(index("op3.detune")), Some(-3.0));
    assert_eq!(s.param(index("op4.on")), Some(0.0));
    assert_eq!(s.patch().to_voice(), p.to_voice());
    assert_eq!(s.patch().name(), "TEST VOICE");
}

#[test]
fn bad_voice_data_is_refused_or_limited() {
    assert_eq!(read(&[]), Err(SysexError::NotVoiceData));
    assert_eq!(read(&[0xf0, 0x43, 0, 0, 1, 0x1b, 0xf7]), Err(SysexError::NotVoiceData));
    let mut dump = write_voice(&Patch::default());
    dump[0] = 0x00;
    assert_eq!(read(&dump), Err(SysexError::NotVoiceData));

    // A wrong checksum is reported, not refused; values out of range are limited.
    let mut dump = write_voice(&Patch::default());
    dump[6] = 0x7f; // operator 6 rate 1: 127
    dump[6 + 134] = 0x7f; // algorithm
    let voices = read(&dump).unwrap_or_else(|e| panic!("{e}"));
    assert!(!voices.checksum_ok);
    let p = voices.patches[0];
    assert_eq!(p.byte(0), 99);
    assert_eq!(p.algorithm(), 31);

    // Random bytes of the right length still make voices that play.
    let mut noise = vec![0u8; 4_096];
    let mut x = 7u32;
    for b in &mut noise {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *b = (x >> 16) as u8;
    }
    let voices = read(&noise).unwrap_or_else(|e| panic!("{e}"));
    for p in &voices.patches {
        let mut s = synth();
        s.load_patch(p);
        let out = play(&mut s, &[(0, on(60, 1.0)), (2_000, on(90, 1.0))], 4_096);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 16.0));
    }
}

// --- Against the original engine -------------------------------------------------------------

struct Scenario {
    name: String,
    rate: f64,
    frames: usize,
    keep: usize,
    patch: Vec<u8>,
    perf: Vec<i32>,
    events: Vec<(usize, NoteEvent)>,
}

fn scenarios(text: &str) -> Vec<Scenario> {
    let mut out = Vec::new();
    let mut cur: Option<Scenario> = None;
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| words.get(i).and_then(|w| w.parse::<i64>().ok()).unwrap_or(0);
        match words.first().copied() {
            Some("scenario") => {
                cur = Some(Scenario {
                    name: words.get(1).copied().unwrap_or_default().to_string(),
                    rate: num(2) as f64,
                    frames: num(3) as usize,
                    keep: num(4) as usize,
                    patch: Vec::new(),
                    perf: Vec::new(),
                    events: Vec::new(),
                });
            }
            Some("patch") => {
                if let Some(s) = cur.as_mut() {
                    s.patch = (1..words.len()).map(|i| num(i) as u8).collect();
                }
            }
            Some("perf") => {
                if let Some(s) = cur.as_mut() {
                    s.perf = (1..words.len()).map(|i| num(i) as i32).collect();
                }
            }
            Some("event") => {
                let (a, b) = (num(3), num(4));
                let event = match words.get(2).copied() {
                    Some("on") => on(a as u8, b as f32 / 127.0),
                    Some("off") => off(a as u8),
                    Some("bend") => NoteEvent::PitchBend((a - 8_192) as f32 / 8_192.0),
                    Some("wheel") => NoteEvent::Controller { number: controller::MODULATION, value: a as f32 / 127.0 },
                    Some("pressure") => NoteEvent::Pressure(a as f32 / 127.0),
                    Some("sustain") => NoteEvent::Controller { number: controller::SUSTAIN, value: a as f32 },
                    _ => continue,
                };
                if let Some(s) = cur.as_mut() {
                    s.events.push((num(1) as usize, event));
                }
            }
            Some("end") => out.extend(cur.take()),
            _ => {}
        }
    }
    out
}

fn golden(dir: &std::path::Path, name: &str) -> Vec<f32> {
    let path = dir.join(format!("{name}.f32"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

#[test]
fn renders_like_the_original_engine() {
    let all = scenarios(include_str!("../tests/golden/scenarios.txt"));
    assert_eq!(all.len(), 8);
    compare(all, &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"));
}

/// The same comparison for scenarios of your own: set `SUHINA_FM_SCENARIOS` to a scenarios
/// file whose folder holds the reference renders (see tests/golden/README.md).
#[test]
#[ignore = "needs renders from the reference renderer"]
fn renders_like_the_original_engine_for_other_scenarios() {
    let path = std::path::PathBuf::from(std::env::var("SUHINA_FM_SCENARIOS").unwrap_or_default());
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    compare(scenarios(&text), path.parent().unwrap_or(std::path::Path::new(".")));
}

fn compare(all: Vec<Scenario>, dir: &std::path::Path) {
    let mut failures = Vec::new();
    for sc in all {
        let mut s = FmSynth::new();
        s.prepare(sc.rate).unwrap_or_else(|e| panic!("{e}"));
        let mut voice = [0u8; patch::VOICE_BYTES];
        voice.copy_from_slice(&sc.patch[..patch::VOICE_BYTES]);
        s.load_patch(&Patch::from_voice(&voice));
        let switch = sc.patch[patch::OP_SWITCH];
        for stored in 0..6 {
            s.set_param(index(&format!("op{}.on", 6 - stored)), f64::from((switch >> stored) & 1));
        }
        let perf_ids = ["pitchBendRange", "pitchBendStep", "portamento", "glissando", "portamentoTime"];
        let route_ids = ["wheelRange", "wheelPitch", "wheelAmp", "wheelEgBias"];
        let pressure_ids = ["pressureRange", "pressurePitch", "pressureAmp", "pressureEgBias"];
        for (id, value) in perf_ids.iter().chain(&route_ids).chain(&pressure_ids).zip(&sc.perf) {
            s.set_param(index(id), f64::from(*value));
        }
        let ours: Vec<f32> = left(&play(&mut s, &sc.events, sc.frames)).into_iter().step_by(sc.keep).collect();
        let theirs = golden(dir, &sc.name);
        assert_eq!(ours.len(), theirs.len(), "{}", sc.name);
        assert!(peak(&theirs) > 0.01, "{} is silent", sc.name);
        let worst = ours
            .iter()
            .zip(&theirs)
            .enumerate()
            .map(|(i, (a, b))| (i, (a - b).abs()))
            .fold((0, 0.0f32), |m, d| if d.1 > m.1 { d } else { m });
        let first = ours.iter().zip(&theirs).position(|(a, b)| (a - b).abs() > 1e-6);
        if worst.1 > 1e-6 {
            failures.push(format!("{}: differs by {} at kept frame {}, first at {first:?}", sc.name, worst.1, worst.0));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

fn stereo(s: &mut FmSynth, frames: usize) -> Vec<[f32; 2]> {
    let mut buffer = vec![0.0f32; frames * 2];
    s.process(&suhina_device_api::ProcessContext::default(), &mut buffer);
    buffer.as_chunks::<2>().0.to_vec()
}

fn energy(frames: &[[f32; 2]], side: usize) -> f32 {
    frames.iter().map(|f| f[side] * f[side]).sum()
}

#[test]
fn by_default_both_sides_carry_the_same_signal() {
    let mut s = synth();
    s.event(on(60, 1.0));
    assert!(stereo(&mut s, 2_048).iter().all(|f| f[0] == f[1]));
}

#[test]
fn pan_moves_the_sound_to_one_side() {
    let mut s = synth();
    s.set_param(crate::params::PAN, 1.0);
    s.reset();
    s.event(on(60, 1.0));
    let out = stereo(&mut s, 2_048);
    assert!(energy(&out, 0) == 0.0 && energy(&out, 1) > 0.0);
}

#[test]
fn spread_makes_the_sides_differ_and_is_read_when_a_note_starts() {
    let mut s = synth();
    s.set_param(crate::params::SPREAD, 0.5);
    s.event(on(60, 1.0));
    let out = stereo(&mut s, 4_096);
    assert!(out.iter().any(|f| (f[0] - f[1]).abs() > 1e-4));
    let mut late = synth();
    late.event(on(60, 1.0));
    late.set_param(crate::params::SPREAD, 0.5);
    assert!(stereo(&mut late, 2_048).iter().all(|f| f[0] == f[1]));
}

#[test]
fn one_voice_keeps_the_envelope_running_over_a_second_key() {
    let mut s = synth();
    s.set_param(crate::params::VOICE_COUNT, 1.0);
    s.event(on(60, 1.0));
    stereo(&mut s, 512);
    s.event(on(67, 1.0));
    stereo(&mut s, 512);
    let down: Vec<u8> = s.slots.iter().filter(|slot| slot.down).map(|slot| slot.key).collect();
    assert_eq!(down, [67]);
}

#[test]
fn tone_takes_the_top_off() {
    let bright = {
        let mut s = synth();
        s.event(on(84, 1.0));
        energy(&stereo(&mut s, 4_096), 0)
    };
    let mut s = synth();
    s.set_param(crate::params::TONE, 0.0);
    s.event(on(84, 1.0));
    assert!(energy(&stereo(&mut s, 4_096), 0) < bright * 0.5);
}

#[test]
fn a_voice_resets_what_its_file_does_not_hold() {
    let mut s = synth();
    s.set_param(crate::params::SPREAD, 0.7);
    s.load_patch(&Patch::default());
    assert_eq!(s.param(crate::params::SPREAD), Some(0.0));
}

fn set(s: &mut FmSynth, id: &str, value: f64) {
    s.set_param(index(id), value);
}

fn wave_index(name: &str) -> f64 {
    params::WAVES.iter().position(|w| *w == name).unwrap_or(0) as f64
}

/// How rough a signal is: its change from frame to frame against its size.
fn roughness(signal: &[f32]) -> f32 {
    let change: f32 = signal.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    change / signal.iter().map(|v| v.abs()).sum::<f32>().max(1e-6)
}

fn note(s: &mut FmSynth, key: u8, frames: usize) -> Vec<f32> {
    left(&play(s, &[(0, on(key, 1.0))], frames))
}

#[test]
fn a_saw_is_rougher_than_the_sine() {
    let sine = note(&mut synth(), 48, 4_800);
    let mut s = synth();
    set(&mut s, "op1.wave", wave_index("Saw 64"));
    let saw = note(&mut s, 48, 4_800);
    assert!(peak(&saw) > 0.1);
    assert!(roughness(&saw[1_000..]) > roughness(&sine[1_000..]) * 1.5);
}

#[test]
fn every_wave_plays_and_stays_finite() {
    for (wave, name) in params::WAVES.iter().enumerate() {
        let mut s = synth();
        set(&mut s, "op1.wave", wave as f64);
        let out = note(&mut s, 60, 2_048);
        assert!(out.iter().all(|v| v.is_finite()), "{name}");
        assert!(peak(&out) > 0.01, "{name}");
    }
}

#[test]
fn a_drawn_wave_plays_its_partials() {
    let mut s = synth();
    set(&mut s, "op1.wave", wave_index("Drawn"));
    set(&mut s, "op1.h1", 0.0);
    set(&mut s, "op1.h2", 1.0);
    s.reset();
    // The second partial alone is the note an octave up.
    let out = note(&mut s, 69, 24_000);
    assert!((frequency(&out[4_800..], SR) - 880.0).abs() < 5.0, "{}", frequency(&out[4_800..], SR));
}

#[test]
fn feedback_on_an_operator_changes_its_sound() {
    let plain = note(&mut synth(), 60, 4_800);
    let mut s = synth();
    set(&mut s, "op1.feedback", 0.8);
    let fed = note(&mut s, 60, 4_800);
    assert!(plain.iter().zip(&fed).any(|(a, b)| (a - b).abs() > 1e-3));
    assert!(roughness(&fed[1_000..]) > roughness(&plain[1_000..]));
}

#[test]
fn a_time_envelope_shapes_and_ends_a_note() {
    let mut s = synth();
    set(&mut s, "op1.envMode", 1.0);
    set(&mut s, "op1.env.attack", 0.05);
    set(&mut s, "op1.env.decay", 0.05);
    set(&mut s, "op1.env.sustain", -12.0);
    set(&mut s, "op1.env.release", 0.05);
    let out = left(&play(&mut s, &[(0, on(69, 1.0)), (24_000, off(69))], 48_000));
    let level = |from: usize, to: usize| peak(&out[from..to]);
    assert!(level(0, 240) < level(2_400, 4_800), "the attack rises");
    assert!(level(12_000, 24_000) < level(2_400, 4_800), "the decay falls");
    assert!(level(40_000, 48_000) < 1e-3, "the release ends it");
    let playing = s.slots.iter().filter(|slot| slot.live && slot.note.is_playing()).count();
    assert_eq!(playing, 0);
}

#[test]
fn the_filter_takes_the_top_off_and_its_envelope_opens_it() {
    let mut s = synth();
    set(&mut s, "op1.wave", wave_index("Saw 64"));
    let open = note(&mut s, 48, 4_800);
    let mut s = synth();
    set(&mut s, "op1.wave", wave_index("Saw 64"));
    set(&mut s, "filterOn", 1.0);
    set(&mut s, "filterFreq", 300.0);
    let closed = note(&mut s, 48, 4_800);
    assert!(roughness(&closed[1_000..]) < roughness(&open[1_000..]) * 0.7);

    let mut s = synth();
    set(&mut s, "op1.wave", wave_index("Saw 64"));
    set(&mut s, "filterOn", 1.0);
    set(&mut s, "filterFreq", 100.0);
    set(&mut s, "filterEnv", 1.0);
    set(&mut s, "filterEnv.peak", 1.0);
    set(&mut s, "filterEnv.sustain", 0.0);
    set(&mut s, "filterEnv.decay", 0.05);
    let swept = note(&mut s, 48, 24_000);
    assert!(roughness(&swept[100..1_000]) > roughness(&swept[20_000..]));
}

#[test]
fn a_resonant_filter_moving_fast_stays_finite() {
    let mut s = synth();
    set(&mut s, "op1.wave", wave_index("Square 64"));
    set(&mut s, "filterOn", 1.0);
    set(&mut s, "filterRes", 1.0);
    set(&mut s, "filterCircuit", 4.0);
    set(&mut s, "filterDrive", 24.0);
    set(&mut s, "shaper", 4.0);
    set(&mut s, "shaperDrive", 24.0);
    set(&mut s, "lfoOn", 1.0);
    set(&mut s, "lfo2Range", 1.0);
    set(&mut s, "lfo2Rate", 1.0);
    set(&mut s, "lfo2Amount", 1.0);
    set(&mut s, "lfo2ToFilter", 1.0);
    let out = note(&mut s, 60, 24_000);
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn the_new_lfo_moves_the_pitch() {
    let plain = note(&mut synth(), 69, 24_000);
    let mut s = synth();
    set(&mut s, "lfoOn", 1.0);
    set(&mut s, "lfo2Rate", 0.6);
    set(&mut s, "lfo2Amount", 0.2);
    let moved = note(&mut s, 69, 24_000);
    assert!(plain.iter().zip(&moved).any(|(a, b)| (a - b).abs() > 1e-2));
}

#[test]
fn the_pitch_envelope_moves_the_note_in_semitones() {
    let mut s = synth();
    set(&mut s, "pitchEnvOn", 1.0);
    set(&mut s, "pitchEnv.peak", 12.0);
    set(&mut s, "pitchEnv.sustain", 12.0);
    let out = note(&mut s, 69, 24_000);
    assert!((frequency(&out[4_800..], SR) - 880.0).abs() < 5.0, "{}", frequency(&out[4_800..], SR));
}

#[test]
fn the_matrix_routes_a_source_to_a_destination() {
    let quiet = {
        let mut s = synth();
        set(&mut s, "mod.velocity.dest1", 21.0);
        set(&mut s, "mod.velocity.amount1", -1.0);
        peak(&note(&mut s, 69, 4_800))
    };
    assert!(quiet < peak(&note(&mut synth(), 69, 4_800)) * 0.1, "{quiet}");
}

#[test]
fn every_algorithm_sounds_and_voice_files_stop_at_the_formats_last() {
    for algorithm in 1..=43 {
        let mut s = synth();
        set(&mut s, "algorithm", f64::from(algorithm));
        for op in 1..=6 {
            set(&mut s, &format!("op{op}.level"), 90.0);
        }
        let out = note(&mut s, 60, 2_048);
        assert!(out.iter().all(|v| v.is_finite()) && peak(&out) > 0.01, "algorithm {algorithm}");
    }
    let mut s = synth();
    set(&mut s, "algorithm", 43.0);
    assert_eq!(usize::from(s.patch().to_voice()[patch::ALGORITHM]), 31);
}

#[test]
fn the_output_does_not_depend_on_how_a_stream_is_divided() {
    let build = || {
        let mut s = synth();
        set(&mut s, "op1.wave", wave_index("Saw 16"));
        set(&mut s, "op1.envMode", 1.0);
        set(&mut s, "filterOn", 1.0);
        set(&mut s, "filterFreq", 2_000.0);
        set(&mut s, "lfoOn", 1.0);
        set(&mut s, "lfo2Amount", 0.3);
        set(&mut s, "spread", 0.4);
        s.event(on(60, 1.0));
        s
    };
    let mut whole = vec![0.0f32; 8_192];
    build().process(&ProcessContext::default(), &mut whole);
    let mut parts = vec![0.0f32; 8_192];
    let mut s = build();
    for chunk in parts.chunks_mut(333 * 2) {
        s.process(&ProcessContext::default(), chunk);
    }
    assert_eq!(whole, parts);
}

#[test]
fn a_new_parameter_left_at_its_default_changes_nothing() {
    let plain = note(&mut synth(), 60, 4_800);
    let mut s = synth();
    for (i, info) in PARAMS.iter().enumerate().skip(params::EXTENDED) {
        s.set_param(i, info.default);
    }
    assert_eq!(plain, note(&mut s, 60, 4_800));
}
