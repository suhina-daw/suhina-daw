// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The laws in `docs/spec/pitch.md`, each checked against the keys the measurements gave.

use super::*;
use suhina_device_api::conformance;

/// A pitch effect with the given parameter values, the rest at their defaults.
fn pitch(settings: &[(usize, f64)]) -> Pitch {
    let mut device = Pitch::new();
    for &(index, value) in settings {
        device.set_param(index, value);
    }
    device
}

/// What every key from 0 to 127 sounds at.
fn map(device: &Pitch) -> Vec<Option<u8>> {
    (0..=127).map(|key| device.transform(key)).collect()
}

/// `device` after a scale event for `root` and `intervals`.
fn with_scale(mut device: Pitch, root: u8, intervals: &[u8]) -> Pitch {
    let scale = MusicalScale::new(root, intervals);
    run(&mut device, &[(0, NoteEvent::Scale(scale))]);
    device
}

const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const DORIAN: [u8; 7] = [0, 2, 3, 5, 7, 9, 10];
const MINOR_PENTATONIC: [u8; 5] = [0, 3, 5, 7, 10];

/// The events `device` sends for `input` in one block, with room for twice as many.
fn run(device: &mut Pitch, input: &[(usize, NoteEvent)]) -> Vec<(usize, NoteEvent)> {
    let mut output = vec![(0, NoteEvent::AllNotesOff); input.len() * 2];
    let written = device.process_events(&ProcessContext::default(), 1_024, input, &mut output);
    output.truncate(written);
    output
}

suhina_device_api::conformance_test!(Pitch::new());

/// At its defaults, the effect leaves every key where it is.
#[test]
fn at_its_defaults_every_key_passes_unchanged() {
    assert_eq!(map(&Pitch::new()), (0..=127).map(Some).collect::<Vec<_>>());
}

/// Pitch transposes by semitones, and keys pushed past either end of the keyboard stay at that
/// end.
#[test]
fn transposes_and_limits_to_the_keyboard() {
    let up = map(&pitch(&[(param::PITCH, 12.0)]));
    assert_eq!(up[48], Some(60));
    assert!(up[115..].iter().all(|&k| k == Some(127)), "116 to 127 all sound 127");
    assert_eq!(up[114], Some(126));
    let down = map(&pitch(&[(param::PITCH, -12.0)]));
    assert!(down[..=12].iter().all(|&k| k == Some(0)));
    assert_eq!(down[13], Some(1));
    for (amount, end) in [(127.0, 127), (128.0, 127), (-128.0, 0)] {
        assert!(map(&pitch(&[(param::PITCH, amount)])).iter().all(|&k| k == Some(end)), "{amount}");
    }
    assert_eq!(pitch(&[(param::PITCH, 1.0)]).transform(60), Some(61));
}

/// Block drops the keys played outside the window, then transposes the rest.
#[test]
fn block_tests_the_played_key_then_transposes() {
    let device = pitch(&[(param::LOWEST, 60.0), (param::RANGE, 12.0), (param::PITCH, 24.0)]);
    for (key, sent) in map(&device).into_iter().enumerate() {
        let expected = (60..=72).contains(&key).then(|| key as u8 + 24);
        assert_eq!(sent, expected, "key {key}");
    }
    let one = pitch(&[(param::LOWEST, 60.0), (param::RANGE, 0.0)]);
    assert_eq!(map(&one).iter().flatten().copied().collect::<Vec<_>>(), [60]);
}

/// Fold wraps the transposed key into the window, with a period of the window's size plus one.
#[test]
fn fold_wraps_into_the_window() {
    let fold = |range: f64, pitch_by: f64| {
        map(&pitch(&[
            (param::MODE, mode::FOLD),
            (param::LOWEST, 60.0),
            (param::RANGE, range),
            (param::PITCH, pitch_by),
        ]))
    };
    let eleven = fold(11.0, 0.0);
    for (key, sent) in [(0, 60), (12, 60), (13, 61), (71, 71), (72, 60)] {
        assert_eq!(eleven[key], Some(sent), "key {key}");
    }
    assert!(eleven.iter().enumerate().all(|(key, sent)| *sent == Some(60 + (key % 12) as u8)));
    let seven = fold(7.0, 0.0);
    for (key, sent) in [(0, 64), (4, 60), (8, 64), (127, 63)] {
        assert_eq!(seven[key], Some(sent), "key {key}");
    }
    assert!(fold(12.0, 0.0).iter().flatten().all(|k| (60..=72).contains(k)));
    // A period of 13: 0 is 60 below the window, which wraps to 60 + 5.
    assert_eq!(fold(12.0, 0.0)[0], Some(65));
    assert!(fold(24.0, 0.0).iter().flatten().all(|k| (60..=84).contains(k)));
    assert!(fold(0.0, 0.0).iter().all(|&k| k == Some(60)));
    // After the transposition: 0 + 5 wraps to 65, 7 + 5 = 12 wraps to 60.
    let shifted = fold(11.0, 5.0);
    assert_eq!((shifted[0], shifted[7]), (Some(65), Some(60)));
    // A period of 12 cancels an octave.
    assert_eq!(fold(11.0, 12.0), eleven);
}

/// Limit moves a transposed key outside the window to its nearest edge.
#[test]
fn limit_moves_keys_to_the_nearest_edge_after_the_transposition() {
    let limit = |range: f64, pitch_by: f64| {
        map(&pitch(&[
            (param::MODE, mode::LIMIT),
            (param::LOWEST, 60.0),
            (param::RANGE, range),
            (param::PITCH, pitch_by),
        ]))
    };
    for range in [7.0, 11.0, 12.0, 24.0] {
        for (key, sent) in limit(range, 0.0).into_iter().enumerate() {
            assert_eq!(sent, Some((key as f64).clamp(60.0, 60.0 + range) as u8), "range {range}, key {key}");
        }
    }
    let up = limit(11.0, 5.0);
    assert!(up[..=55].iter().all(|&k| k == Some(60)));
    assert_eq!(up[56], Some(61));
    assert!(limit(11.0, 12.0)[..=48].iter().all(|&k| k == Some(60)));
    assert!(limit(0.0, 0.0).iter().all(|&k| k == Some(60)));
}

/// A window reaching past key 127 still gives keys on the keyboard, in every mode.
#[test]
fn a_window_past_the_top_key_still_gives_keys() {
    for mode in [mode::BLOCK, mode::FOLD, mode::LIMIT] {
        let device = pitch(&[(param::MODE, mode), (param::LOWEST, 120.0), (param::RANGE, 20.0), (param::PITCH, 3.0)]);
        assert!(map(&device).iter().flatten().all(|&k| k <= 127));
        assert_eq!(device.transform(125), Some(127));
    }
}

/// With the scale followed, the transposition counts the scale's degrees, snaps keys outside it,
/// and the semitone setting is not used.
#[test]
fn scale_degrees_step_along_the_clips_scale() {
    let degrees = |amount: f64, intervals: &[u8], root: u8| {
        with_scale(pitch(&[(param::FOLLOW_SCALE, 1.0), (param::PITCH_DEGREES, amount)]), root, intervals)
    };
    let up = degrees(1.0, &MAJOR, 0);
    assert_eq!([60, 64, 71].map(|k| up.transform(k)), [Some(62), Some(65), Some(72)]);
    assert_eq!(degrees(7.0, &MAJOR, 0).transform(60), Some(72));
    assert_eq!(degrees(-1.0, &MAJOR, 0).transform(60), Some(59));
    assert_eq!(degrees(2.0, &MAJOR, 0).transform(60), Some(64));
    // Keys outside the scale move to the nearest key of the scale, a tie going down.
    let still = degrees(0.0, &MAJOR, 0);
    assert_eq!([61, 63, 66, 68, 70].map(|k| still.transform(k)), [60, 62, 65, 67, 69].map(Some));
    // Only the scale's keys matter, not its root.
    assert_eq!(map(&degrees(3.0, &DORIAN, 2)), map(&degrees(3.0, &MAJOR, 0)));
    let pentatonic = degrees(1.0, &MINOR_PENTATONIC, 0);
    assert_eq!([60, 67, 70, 62, 64].map(|k| pentatonic.transform(k)), [63, 70, 72, 65, 65].map(Some));
    assert_eq!(degrees(5.0, &MINOR_PENTATONIC, 0).transform(60), Some(72));
    // The semitone Pitch is not used while the scale is followed, and the other way round.
    let both =
        with_scale(pitch(&[(param::FOLLOW_SCALE, 1.0), (param::PITCH, 5.0), (param::PITCH_DEGREES, 1.0)]), 0, &MAJOR);
    assert_eq!(both.transform(60), Some(62));
    let off = with_scale(pitch(&[(param::PITCH, 5.0), (param::PITCH_DEGREES, 1.0)]), 0, &MAJOR);
    assert_eq!(off.transform(61), Some(66));
}

/// Following the scale with no scale, or after the scale is cleared, counts degrees as semitones.
#[test]
fn without_a_scale_degrees_count_semitones() {
    let device = pitch(&[(param::FOLLOW_SCALE, 1.0), (param::PITCH_DEGREES, 3.0)]);
    assert_eq!(device.transform(61), Some(64));
    let cleared = with_scale(with_scale(device, 0, &MAJOR), 0, &[]);
    assert_eq!(cleared.transform(61), Some(64));
}

/// Block tests the key as played, before it is snapped to the scale.
#[test]
fn block_with_the_scale_tests_the_key_as_played() {
    let device =
        with_scale(pitch(&[(param::FOLLOW_SCALE, 1.0), (param::LOWEST, 62.0), (param::RANGE, 5.0)]), 0, &MAJOR);
    let passed: Vec<u8> = (0..=127).filter(|&k| device.transform(k).is_some()).collect();
    assert_eq!(passed, (62..=67).collect::<Vec<_>>());
    assert_eq!(device.transform(63), Some(62));
}

/// A note-off ends the key its note-on sent even after the setting changes, and only once.
#[test]
fn a_note_off_ends_the_key_its_note_started() {
    let mut device = pitch(&[(param::PITCH, 12.0)]);
    let on = run(&mut device, &[(3, NoteEvent::On { key: 60, velocity: 0.8 })]);
    assert_eq!(on, [(3, NoteEvent::On { key: 72, velocity: 0.8 })]);
    device.set_param(param::PITCH, 5.0);
    let off = run(&mut device, &[(9, NoteEvent::Off { key: 60, velocity: 0.25 })]);
    assert_eq!(off, [(9, NoteEvent::Off { key: 72, velocity: 0.25 })]);
    // Ended once: a second release sends nothing.
    assert!(run(&mut device, &[(10, NoteEvent::Off { key: 60, velocity: 0.25 })]).is_empty());
}

/// A note Block drops sends nothing, neither its note-on nor its note-off.
#[test]
fn a_blocked_note_sends_neither_on_nor_off() {
    let mut device = pitch(&[(param::LOWEST, 60.0), (param::RANGE, 0.0)]);
    let out = run(
        &mut device,
        &[(0, NoteEvent::On { key: 50, velocity: 1.0 }), (5, NoteEvent::Off { key: 50, velocity: 1.0 })],
    );
    assert!(out.is_empty(), "{out:?}");
}

/// Two keys moved to the same key each send their own note-on and note-off.
#[test]
fn several_keys_can_sound_the_same_key_and_each_ends_its_own() {
    let mut device = pitch(&[(param::MODE, mode::LIMIT), (param::LOWEST, 60.0), (param::RANGE, 0.0)]);
    let out = run(
        &mut device,
        &[
            (0, NoteEvent::On { key: 40, velocity: 1.0 }),
            (1, NoteEvent::On { key: 80, velocity: 0.5 }),
            (2, NoteEvent::Off { key: 40, velocity: 0.0 }),
            (3, NoteEvent::Off { key: 80, velocity: 0.0 }),
        ],
    );
    assert_eq!(
        out,
        [
            (0, NoteEvent::On { key: 60, velocity: 1.0 }),
            (1, NoteEvent::On { key: 60, velocity: 0.5 }),
            (2, NoteEvent::Off { key: 60, velocity: 0.0 }),
            (3, NoteEvent::Off { key: 60, velocity: 0.0 }),
        ]
    );
}

/// A key played again while its note sounds ends that note before the new one starts.
#[test]
fn a_key_played_again_while_held_ends_its_first_note() {
    let mut device = pitch(&[(param::PITCH, 2.0)]);
    run(&mut device, &[(0, NoteEvent::On { key: 60, velocity: 1.0 })]);
    device.set_param(param::PITCH, 4.0);
    let out = run(&mut device, &[(7, NoteEvent::On { key: 60, velocity: 0.5 })]);
    assert_eq!(
        out,
        [(7, NoteEvent::Off { key: 62, velocity: RELEASE }), (7, NoteEvent::On { key: 64, velocity: 0.5 })]
    );
}

/// With too little room in the output, note-ons are left out so every note-off fits.
#[test]
fn short_of_room_note_ons_are_left_out_before_note_offs() {
    let mut device = pitch(&[(param::PITCH, 2.0)]);
    run(&mut device, &[(0, NoteEvent::On { key: 60, velocity: 1.0 })]);
    // Room for one: the held note ends and the new one is left out, so nothing sounds.
    let again = [(5, NoteEvent::On { key: 60, velocity: 0.5 })];
    let mut output = [(0, NoteEvent::AllNotesOff); 1];
    assert_eq!(device.process_events(&ProcessContext::default(), 64, &again, &mut output), 1);
    assert_eq!(output, [(5, NoteEvent::Off { key: 62, velocity: RELEASE })]);
    assert!(run(&mut device, &[(9, NoteEvent::Off { key: 60, velocity: 0.5 })]).is_empty());
    // Room for one with a note-off to come: the note-on gives its place to the note-off.
    run(&mut device, &[(0, NoteEvent::On { key: 61, velocity: 1.0 })]);
    let crowded = [(1, NoteEvent::On { key: 70, velocity: 1.0 }), (2, NoteEvent::Off { key: 61, velocity: 0.5 })];
    let mut output = [(0, NoteEvent::AllNotesOff); 1];
    assert_eq!(device.process_events(&ProcessContext::default(), 64, &crowded, &mut output), 1);
    assert_eq!(output, [(2, NoteEvent::Off { key: 63, velocity: 0.5 })]);
    // No room at all: the held note goes on, and its key's note-off still ends it.
    run(&mut device, &[(0, NoteEvent::On { key: 50, velocity: 1.0 })]);
    assert_eq!(
        device.process_events(
            &ProcessContext::default(),
            64,
            &[(3, NoteEvent::On { key: 50, velocity: 1.0 })],
            &mut []
        ),
        0
    );
    assert_eq!(
        run(&mut device, &[(4, NoteEvent::Off { key: 50, velocity: 0.5 })]),
        [(4, NoteEvent::Off { key: 52, velocity: 0.5 })]
    );
}

/// Controllers, pitch bend, pressure, scale and all-notes-off pass unchanged, and a note keeps
/// its velocity and frame.
#[test]
fn other_events_velocities_and_frames_pass_unchanged() {
    let mut device = pitch(&[(param::PITCH, -3.0)]);
    let input = [
        (0, NoteEvent::Controller { number: 64, value: 1.0 }),
        (4, NoteEvent::PitchBend(-0.25)),
        (4, NoteEvent::On { key: 70, velocity: 0.33 }),
        (8, NoteEvent::Pressure(0.5)),
        (12, NoteEvent::Scale(None)),
        (13, NoteEvent::AllNotesOff),
    ];
    let out = run(&mut device, &input);
    let mut expected = input.to_vec();
    expected[2] = (4, NoteEvent::On { key: 67, velocity: 0.33 });
    assert_eq!(out, expected);
    // All notes off forgot the held key: its release sends nothing more.
    assert!(run(&mut device, &[(0, NoteEvent::Off { key: 70, velocity: 0.5 })]).is_empty());
}

/// The audio passes through unchanged and with no latency.
#[test]
fn the_sound_passes_untouched() {
    let mut device = pitch(&[(param::PITCH, 7.0)]);
    let mut buffer = conformance::test_signal(256);
    let before = buffer.clone();
    device.process(&ProcessContext::default(), &mut buffer);
    assert_eq!(buffer, before);
    assert_eq!(device.latency(), 0);
}
