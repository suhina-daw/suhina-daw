// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;

fn recording(key: u8, velocity: f32, ratio_two: f64, decay: f64) -> ModelRecording {
    ModelRecording {
        key,
        source: 0,
        velocity,
        level: 0.5,
        modes: vec![
            ModelMode { phase: 0.4, ..ModelMode::plain(1.0, 0.6, decay) },
            ModelMode {
                fast_decay_seconds: 0.2,
                fast_share: 0.5,
                beat_ratio: 1.002,
                beat_share: 0.3,
                phase: -1.0,
                ..ModelMode::plain(ratio_two, 0.3, decay / 2.0)
            },
        ],
        attack_rate: 22050,
        attack: (0..2205).map(|i| (i as f32 * 0.01).sin() * 0.1).collect(),
    }
}

fn frequency_near(voice: &Voice, hz: f64) -> Option<&Mode> {
    voice.modes.get(..voice.count).unwrap().iter().find(|m| (m.frequency_hz / hz).ln().abs() < 0.001)
}

#[test]
fn sets_round_trip_canonically_and_reject_hostile_bytes() {
    let set = ModelSet::new(vec![recording(72, 0.8, 2.01, 1.0), recording(48, 0.8, 2.0, 2.0)]).unwrap();
    assert_eq!(set.recordings()[0].key, 48, "recordings are sorted by key");
    let bytes = set.encode();
    assert_eq!(ModelSet::decode(&bytes).unwrap(), set);
    for cut in [0, 7, 15, 16, bytes.len() / 2, bytes.len() - 1] {
        assert!(ModelSet::decode(&bytes[..cut]).is_err(), "{cut}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(ModelSet::decode(&longer).is_err());
    for (offset, value) in [(0, b'X'), (8, 9), (10, 2), (12, 0), (12, 17), (13, 1)] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        assert!(ModelSet::decode(&bad).is_err(), "byte {offset} = {value}");
    }
    // A nonfinite level in the first recording.
    let mut nan = bytes.clone();
    nan[24..32].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(ModelSet::decode(&nan).is_err());
    // Recordings out of key order are not canonical.
    let mut swapped = set.encode();
    let first = 16 + 28 + 2 * 64 + 2205 * 4;
    let (a, b) = swapped[16..].split_at_mut(first - 16);
    let mut reordered = b.to_vec();
    reordered.extend_from_slice(a);
    swapped.truncate(16);
    swapped.extend_from_slice(&reordered);
    assert!(ModelSet::decode(&swapped).is_err());
}

/// A change that makes a recording invalid.
type Breakage = Box<dyn Fn(&mut ModelRecording)>;

#[test]
fn recordings_validate_every_bound() {
    let good = recording(60, 0.8, 2.0, 1.0);
    assert!(good.validate().is_ok());
    let cases: Vec<Breakage> = vec![
        Box::new(|r| r.key = 128),
        Box::new(|r| r.source = 16),
        Box::new(|r| r.velocity = 0.0),
        Box::new(|r| r.velocity = 1.5),
        Box::new(|r| r.level = 0.0),
        Box::new(|r| r.level = 17.0),
        Box::new(|r| r.modes.clear()),
        Box::new(|r| r.modes[1].ratio = 0.5),
        Box::new(|r| r.modes[0].magnitude = 0.9),
        Box::new(|r| r.modes[0].decay_seconds = 61.0),
        Box::new(|r| r.modes[1].fast_decay_seconds = 5.0),
        Box::new(|r| r.modes[0].fast_share = 0.3),
        Box::new(|r| r.modes[1].beat_ratio = 1.1),
        Box::new(|r| r.modes[0].beat_share = 0.2),
        Box::new(|r| r.modes[1].beat_share = 0.6),
        Box::new(|r| r.modes[0].phase = 4.0),
        Box::new(|r| r.modes[0].ratio = f64::NAN),
        Box::new(|r| r.attack_rate = 0),
        Box::new(|r| r.attack = vec![0.0; 6000]),
        Box::new(|r| r.attack[3] = f32::INFINITY),
        Box::new(|r| {
            r.attack.clear();
            r.attack_rate = 22050;
        }),
    ];
    for (index, break_it) in cases.iter().enumerate() {
        let mut bad = good.clone();
        break_it(&mut bad);
        assert!(bad.validate().is_err(), "case {index}");
        assert!(ModelSet::new(vec![bad]).is_err(), "case {index}");
    }
    assert!(ModelSet::new(vec![]).is_err());
    assert!(ModelSet::new(vec![good.clone(), good.clone()]).is_err(), "same key and velocity");
    assert!(ModelSet::new((0..33).map(|k| recording(40 + k, 0.8, 2.0, 1.0)).collect()).is_err());
}

#[test]
fn one_recording_plays_its_modes_at_neutral_controls() {
    let set = ModelSet::new(vec![recording(60, 0.8, 2.0, 1.0)]).unwrap();
    let mut voice = Voice::default();
    set.voice(60, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    let c3 = 440.0 * 2.0_f64.powf(-9.0 / 12.0);
    assert!((voice.level - 0.5).abs() < 1e-12);
    let fundamental = frequency_near(&voice, c3).unwrap();
    assert!((fundamental.amplitude - 0.6).abs() < 1e-12 && (fundamental.decay_seconds - 1.0).abs() < 1e-12);
    // The second mode: slow part 0.3 * 0.5 * 0.7, fast part 0.3 * 0.5, partner 0.3 * 0.5 * 0.3.
    let parts: Vec<_> = voice.modes[..voice.count]
        .iter()
        .filter(|m| (m.frequency_hz / (2.0 * c3)).ln().abs() < 0.01)
        .map(|m| (m.amplitude, m.decay_seconds))
        .collect();
    assert_eq!(parts.len(), 3);
    for expected in [(0.105, 0.5), (0.15, 0.2), (0.045, 0.5)] {
        assert!(
            parts.iter().any(|p| (p.0 - expected.0).abs() < 1e-12 && (p.1 - expected.1).abs() < 1e-12),
            "{parts:?}"
        );
    }
    assert!(frequency_near(&voice, 2.0 * c3 * 1.002).is_some(), "the beating partner");
    assert_eq!(voice.count, 4);
    let total: f64 = voice.modes[..voice.count].iter().map(|m| m.amplitude).sum();
    assert!((total - 0.9).abs() < 1e-12);
    let (attack, step) = voice.attack.unwrap();
    assert_eq!(attack, 0);
    assert!((step - 22050.0 / 48000.0).abs() < 1e-12);
    // An octave up: twice the frequencies, the attack read twice as fast.
    set.voice(72, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    assert!(frequency_near(&voice, 2.0 * c3).is_some());
    assert!((voice.attack.unwrap().1 - 2.0 * 22050.0 / 48000.0).abs() < 1e-12);
}

#[test]
fn notes_between_recordings_fuse_parameters_and_equal_recordings_average() {
    // Recordings an octave apart with different second-mode ratios and decays.
    let set = ModelSet::new(vec![recording(48, 0.8, 2.0, 2.0), recording(72, 0.8, 2.02, 0.5)]).unwrap();
    let mut voice = Voice::default();
    set.voice(60, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    let c3 = 440.0 * 2.0_f64.powf(-9.0 / 12.0);
    // Halfway: weighted geometric means of ratio and of T60, each recording's T60 first carried
    // to the key by its own decay law (slope of ln T60 over ln ratio: its second mode decays
    // twice as fast at ratio 2 or 2.02).
    let ratio = (2.0_f64 * 2.02).sqrt();
    let fundamental = frequency_near(&voice, c3).unwrap();
    let (low_law, high_law) = (-1.0, -(2.0_f64.ln()) / 2.02_f64.ln());
    let expected = (2.0 * 2.0_f64.powf(low_law) * 0.5 * 2.0_f64.powf(-high_law)).sqrt();
    assert!((expected - 0.995).abs() < 0.001, "{expected}");
    assert!((fundamental.decay_seconds - expected).abs() < 1e-9, "{fundamental:?}");
    assert!(voice.modes[..voice.count].iter().any(|m| (m.frequency_hz - c3 * ratio).abs() < 1e-6));
    // Nearer the lower recording, its parameters dominate.
    set.voice(50, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    let low = frequency_near(&voice, 440.0 * 2.0_f64.powf(-19.0 / 12.0)).unwrap();
    assert!(low.decay_seconds > 1.6, "{low:?}");
    // Two recordings at one key and velocity are not allowed; at one key with neighbouring
    // velocities they average with equal weight at the midpoint.
    let pair = ModelSet::new(vec![recording(60, 0.6, 2.0, 2.0), recording(60, 1.0, 2.0, 0.5)]).unwrap();
    pair.voice(60, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    assert!((frequency_near(&voice, c3).unwrap().decay_seconds - 1.0).abs() < 1e-9);
}

#[test]
fn a_resonance_only_one_recording_has_fades_in_proportion() {
    let mut lonely = recording(60, 0.6, 2.0, 1.0);
    lonely.modes.push(ModelMode::plain(5.3, 0.1, 0.4));
    let other = recording(60, 1.0, 2.0, 1.0);
    let set = ModelSet::new(vec![lonely, other]).unwrap();
    let mut voice = Voice::default();
    set.voice(60, 0.6, &Controls::NEUTRAL, 48000.0, &mut voice);
    let c3 = 440.0 * 2.0_f64.powf(-9.0 / 12.0);
    let alone = frequency_near(&voice, 5.3 * c3).unwrap().amplitude;
    set.voice(60, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    let shared = frequency_near(&voice, 5.3 * c3).unwrap().amplitude;
    assert!(alone > 0.099 && (shared - 0.05).abs() < 1e-6, "{alone} {shared}");
}

#[test]
fn controls_transform_fused_modes_and_the_sine_budget_holds() {
    let modes: Vec<_> = (1..=32)
        .map(|n| ModelMode {
            fast_decay_seconds: 0.1,
            fast_share: 0.5,
            beat_ratio: 1.001,
            beat_share: 0.2,
            ..ModelMode::plain(f64::from(n), 1.0 / 32.0, 1.0)
        })
        .collect();
    let set = ModelSet::new(vec![ModelRecording {
        source: 0,
        key: 36,
        velocity: 0.8,
        level: 1.0,
        modes,
        attack_rate: 0,
        attack: vec![],
    }])
    .unwrap();
    let mut voice = Voice::default();
    set.voice(36, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    assert_eq!(voice.count, MAX_VOICE_SINES, "96 candidate sines are cut to the budget");
    assert!(voice.attack.is_none());
    let mut half = Controls::NEUTRAL;
    half.sines = 32;
    set.voice(36, 0.8, &half, 48000.0, &mut voice);
    assert_eq!(voice.count, 32);
    let mut longer = Controls::NEUTRAL;
    longer.decay = 4.0;
    let mut neutral = Voice::default();
    set.voice(36, 0.8, &Controls::NEUTRAL, 48000.0, &mut neutral);
    set.voice(36, 0.8, &longer, 48000.0, &mut voice);
    let c1 = 440.0 * 2.0_f64.powf(-33.0 / 12.0);
    let slow = |v: &Voice| {
        v.modes[..v.count]
            .iter()
            .filter(|m| (m.frequency_hz - c1).abs() < 1e-9)
            .map(|m| m.decay_seconds)
            .fold(0.0, f64::max)
    };
    assert!((slow(&voice) - 2.0 * slow(&neutral)).abs() < 1e-9);
}

#[test]
fn source_morph_tilts_the_fusion_between_instruments() {
    let mut a = recording(60, 0.8, 2.0, 2.0);
    let mut b = recording(60, 0.8, 2.0, 0.5);
    a.source = 0;
    b.source = 1;
    let set = ModelSet::new(vec![a, b]).unwrap();
    let encoded = set.encode();
    assert_eq!(ModelSet::decode(&encoded).unwrap(), set);
    let c3 = 440.0 * 2.0_f64.powf(-9.0 / 12.0);
    let t60 = |morph: f64| {
        let mut controls = Controls::NEUTRAL;
        controls.morph = morph;
        let mut voice = Voice::default();
        set.voice(60, 0.8, &controls, 48000.0, &mut voice);
        frequency_near(&voice, c3).unwrap().decay_seconds
    };
    // Equal at 0.5 (geometric mean 1 s); 70 % / 30 % at 0.3; only the first source at 0.
    assert!((t60(0.5) - 1.0).abs() < 1e-9);
    assert!((t60(0.3) - (0.7 * 2.0_f64.ln() + 0.3 * 0.5_f64.ln()).exp()).abs() < 1e-9);
    assert!((t60(0.0) - 2.0).abs() < 1e-9);
    assert!((t60(1.0) - 0.5).abs() < 1e-9);
}

#[test]
fn a_recordings_decay_law_shortens_higher_notes() {
    // Higher modes ring shorter: T60 proportional to 1 / frequency gives a law of -1.
    let modes = vec![ModelMode::plain(1.0, 0.5, 2.0), ModelMode::plain(2.0, 0.3, 1.0), ModelMode::plain(4.0, 0.2, 0.5)];
    let set = ModelSet::new(vec![ModelRecording {
        key: 48,
        source: 0,
        velocity: 0.8,
        level: 1.0,
        modes,
        attack_rate: 0,
        attack: vec![],
    }])
    .unwrap();
    let mut voice = Voice::default();
    set.voice(60, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    let c3 = 440.0 * 2.0_f64.powf(-9.0 / 12.0);
    // An octave above the recording: the fundamental rings half as long.
    assert!((frequency_near(&voice, c3).unwrap().decay_seconds - 1.0).abs() < 1e-9);
    set.voice(48, 0.8, &Controls::NEUTRAL, 48000.0, &mut voice);
    assert!((frequency_near(&voice, c3 / 2.0).unwrap().decay_seconds - 2.0).abs() < 1e-9);
}

#[test]
fn hammer_velocity_soft_pedal_and_pedal_resonance_shape_a_note() {
    let set = ModelSet::new(vec![recording(60, 0.8, 2.0, 1.0)]).unwrap();
    let c3 = 440.0 * 2.0_f64.powf(-9.0 / 12.0);
    let mut controls = Controls::NEUTRAL;
    controls.velocity_brightness = 1.0;
    let mut soft = Voice::default();
    let mut hard = Voice::default();
    set.voice(60, 0.4, &controls, 48000.0, &mut soft);
    set.voice(60, 1.0, &controls, 48000.0, &mut hard);
    let upper =
        |v: &Voice| v.modes[..v.count].iter().filter(|m| m.frequency_hz > 1.5 * c3).map(|m| m.amplitude).sum::<f64>();
    assert!(upper(&hard) > upper(&soft), "a harder strike is brighter");
    assert!((soft.attack_gain - 0.5).abs() < 1e-6 && (hard.attack_gain - 1.25).abs() < 1e-6);
    // The soft pedal: quieter and without the beating partner.
    let mut pedal = Controls::NEUTRAL;
    pedal.soft = true;
    let mut voice = Voice::default();
    set.voice(60, 0.8, &pedal, 48000.0, &mut voice);
    assert!((voice.level - 0.35).abs() < 1e-12);
    assert!(frequency_near(&voice, 2.0 * c3 * 1.002).is_none());
    // The sustain pedal's resonance adds a long sine at each partial.
    let mut resonant = Controls::NEUTRAL;
    resonant.pedal_resonance = 1.0;
    set.voice(60, 0.8, &resonant, 48000.0, &mut voice);
    let at_fundamental: Vec<_> =
        voice.modes[..voice.count].iter().filter(|m| (m.frequency_hz - c3).abs() < 1e-9).collect();
    assert_eq!(at_fundamental.len(), 2);
    assert!(at_fundamental.iter().any(|m| (m.decay_seconds - 2.0).abs() < 1e-9 && (m.amplitude - 0.09).abs() < 1e-12));
}
