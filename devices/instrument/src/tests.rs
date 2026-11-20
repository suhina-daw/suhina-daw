// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

suhina_device_api::conformance_test!(Instrument::with_rate);

#[test]
fn revision_one_matches_the_independent_analytical_reference_render() {
    let mut instrument = Instrument::new();
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    let mut buffer = vec![0.0; 16384];
    instrument.process(&ProcessContext::default(), &mut buffer);
    for line in include_str!("../tests/data/modal-v1-reference.txt").lines().filter(|line| !line.starts_with('#')) {
        let (frame, expected) = line.split_once(' ').unwrap();
        let frame: usize = frame.parse().unwrap();
        let expected: f32 = expected.parse().unwrap();
        assert!(
            (buffer[frame * 2] - expected).abs() < 3e-7,
            "revision 1 changed at frame {frame}: {} vs {expected}",
            buffer[frame * 2]
        );
        assert_eq!(buffer[frame * 2], buffer[frame * 2 + 1]);
    }
    assert!(std::mem::size_of::<Voice>() <= 2048);
    assert!(std::mem::size_of::<Instrument>() <= 256 * 1024);
}

#[test]
fn modal_notes_are_audible_and_release_and_reset_are_exactly_silent() {
    let mut instrument = Instrument::new();
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    let mut buffer = [0.0; 1024];
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(buffer.iter().any(|value| value.abs() > 0.001));
    instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
    for _ in 0..32 {
        instrument.process(&ProcessContext::default(), &mut buffer);
    }
    assert!(buffer.iter().all(|value| *value == 0.0));
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    instrument.reset();
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(buffer.iter().all(|value| *value == 0.0));
}

#[test]
fn unsupported_model_revisions_do_not_silently_play_a_different_model() {
    let mut instrument = Instrument::new();
    instrument.set_param(param::REVISION, 4.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut buffer = [1.0; 1024];
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(!instrument.model_supported());
    assert!(buffer.iter().all(|value| *value == 0.0));
    assert_eq!(instrument.param(param::REVISION), Some(4.0));
}

#[test]
fn single_sample_maps_play_through_the_shared_runtime() {
    let mut instrument = Instrument::new();
    instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.5; 4800]));
    instrument.set_param(param::SOURCE, 1.0);
    instrument.set_param(param::ATTACK, 0.0);
    instrument.set_param(param::GAIN, 0.0);
    instrument.set_param(param::KEY_LOW, 60.0);
    instrument.set_param(param::KEY_HIGH, 60.0);
    instrument.reset();
    let mut buffer = [0.0; 1024];
    instrument.event(NoteEvent::On { key: 59, velocity: 1.0 });
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(buffer.iter().all(|value| *value == 0.0));
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!((buffer[512] - 0.5).abs() < 1e-6);
}

#[test]
fn stock_panel_is_valid_sdk_layout() {
    let panel = panel::layout().panel(&INFO).unwrap();
    assert!(panel.ui.is_some());
}

#[test]
fn sustain_holds_both_hybrid_layers_until_the_same_pedal_release() {
    for source in [0.0, 1.0, 2.0] {
        let mut instrument = Instrument::new();
        instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
        instrument.set_param(param::SOURCE, source);
        instrument.set_param(param::PLAYBACK, 2.0);
        instrument.set_param(param::RELEASE, 0.005);
        instrument.reset();
        instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
        instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
        instrument.event(NoteEvent::On { key: 60, velocity: 0.0 });
        let mut block = [0.0; 1024];
        for _ in 0..4 {
            instrument.process(&ProcessContext::default(), &mut block);
        }
        assert!(block.iter().any(|value| value.abs() > 0.0001), "source {source} must sustain");
        instrument.event(NoteEvent::Controller { number: 64, value: 0.0 });
        for _ in 0..4 {
            instrument.process(&ProcessContext::default(), &mut block);
        }
        assert!(block.iter().all(|value| *value == 0.0), "source {source} must release");
    }
}

#[test]
fn sample_only_notes_never_start_inaudible_modal_voices() {
    let mut instrument = Instrument::new();
    instrument.set_param(param::SOURCE, 1.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    assert!(instrument.voices.iter().all(|voice| !voice.body.is_active()));
}

#[test]
fn source_switches_retire_hidden_sample_voices_independently_of_block_size() {
    fn performance(block_frames: usize) -> Vec<f32> {
        let mut instrument = Instrument::new();
        let source: std::sync::Arc<[f32]> = (0..4800).map(|i| (i as f32 * 0.04).sin() * 0.2).collect();
        instrument.samples().unwrap().load_sample(48000, 1, source);
        instrument.set_param(param::SOURCE, 1.0);
        instrument.set_param(param::PLAYBACK, 2.0);
        instrument.reset();
        instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
        let mut rendered = Vec::new();
        for (source, frames) in [(1.0, 1024), (0.0, 4096), (1.0, 1024)] {
            instrument.set_param(param::SOURCE, source);
            let mut section = vec![0.0; frames * 2];
            for block in section.chunks_mut(block_frames * 2) {
                instrument.process(&ProcessContext::default(), block);
            }
            rendered.extend(section);
        }
        assert!(rendered[10240..].iter().all(|value| *value == 0.0), "switching back needs a new note");
        rendered
    }
    assert_eq!(performance(17), performance(512));
}

#[test]
fn retiring_a_hidden_source_preserves_the_current_pitch_wheel_for_the_next_note() {
    let mut instrument = Instrument::new();
    let source: std::sync::Arc<[f32]> = (0..4800).map(|i| (i as f32 * 0.04).sin() * 0.2).collect();
    instrument.samples().unwrap().load_sample(48000, 1, source);
    instrument.set_param(param::SOURCE, 1.0);
    instrument.set_param(param::PLAYBACK, 2.0);
    instrument.reset();
    instrument.event(NoteEvent::PitchBend(1.0));
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut before = [0.0; 2048];
    instrument.process(&ProcessContext::default(), &mut before);
    instrument.set_param(param::SOURCE, 0.0);
    let mut silent = [0.0; 2048];
    instrument.process(&ProcessContext::default(), &mut silent);
    instrument.set_param(param::SOURCE, 1.0);
    instrument.process(&ProcessContext::default(), &mut silent);
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut after = [0.0; 2048];
    instrument.process(&ProcessContext::default(), &mut after);
    assert_eq!(after, before);
}

#[test]
fn hybrid_blend_endpoints_advance_held_and_released_layers() {
    for released in [false, true] {
        let mut hybrid = Instrument::new();
        let mut sample = Instrument::new();
        for instrument in [&mut hybrid, &mut sample] {
            let source: std::sync::Arc<[f32]> = (0..4800).map(|i| (i as f32 * 0.04).sin() * 0.2).collect();
            instrument.samples().unwrap().load_sample(48000, 1, source);
            instrument.set_param(param::SOURCE, 1.0);
            instrument.set_param(param::PLAYBACK, 2.0);
            instrument.set_param(param::RELEASE, 0.005);
        }
        hybrid.set_param(param::SOURCE, 2.0);
        hybrid.set_param(param::BLEND, 0.0);
        for instrument in [&mut hybrid, &mut sample] {
            instrument.reset();
            instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
            let mut silent = [0.0; 4096];
            instrument.process(&ProcessContext::default(), &mut silent);
            if released {
                instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
            }
            instrument.process(&ProcessContext::default(), &mut silent);
        }
        hybrid.set_param(param::BLEND, 1.0);
        let mut got = [0.0; 4096];
        let mut reference = [0.0; 4096];
        hybrid.process(&ProcessContext::default(), &mut got);
        sample.process(&ProcessContext::default(), &mut reference);
        assert_eq!(&got[1024..], &reference[1024..], "the blend reveals current sample phase or completed release");
        assert_eq!(got[1024..].iter().all(|value| *value == 0.0), released);
    }
}

#[test]
fn finite_velocities_are_normalized_before_both_source_layers_receive_them() {
    for source in [0.0, 1.0, 2.0] {
        let mut normal = Instrument::new();
        let mut hostile = Instrument::new();
        for instrument in [&mut normal, &mut hostile] {
            instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
            instrument.set_param(param::SOURCE, source);
            instrument.reset();
        }
        normal.event(NoteEvent::On { key: 60, velocity: 1.0 });
        hostile.event(NoteEvent::On { key: 60, velocity: 10.0 });
        let mut expected = [0.0; 1024];
        let mut actual = [0.0; 1024];
        normal.process(&ProcessContext::default(), &mut expected);
        hostile.process(&ProcessContext::default(), &mut actual);
        assert_eq!(actual, expected, "source {source}");
        normal.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        hostile.event(NoteEvent::On { key: 60, velocity: -2.0 });
        normal.process(&ProcessContext::default(), &mut expected);
        hostile.process(&ProcessContext::default(), &mut actual);
        assert_eq!(actual, expected, "nonpositive velocity releases source {source}");
    }
}

#[test]
fn hostile_sample_levels_remain_finite_after_the_final_gain() {
    for source in [1.0, 2.0] {
        for level in [f32::MAX, -f32::MAX, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut instrument = Instrument::new();
            instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![level; 4800]));
            instrument.set_param(param::SOURCE, source);
            instrument.set_param(param::GAIN, 12.0);
            instrument.set_param(param::ATTACK, 0.0);
            instrument.reset();
            instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
            let mut rendered = [0.0; 2048];
            instrument.process(&ProcessContext::default(), &mut rendered);
            assert!(rendered.iter().all(|value| value.is_finite()), "source {source}, level {level}");
            if level.is_finite() {
                assert!(rendered[1024..].iter().all(|value| *value == level.signum() * f32::MAX));
            }
        }
    }
}

#[test]
fn pedal_up_retires_old_same_key_samples_but_preserves_the_held_retrigger() {
    for source in [1.0, 2.0] {
        let mut instrument = Instrument::new();
        let mut reference = Instrument::new();
        for core in [&mut instrument, &mut reference] {
            core.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
            core.set_param(param::SOURCE, source);
            core.set_param(param::PLAYBACK, 2.0);
            core.set_param(param::RELEASE, 0.005);
            core.reset();
        }
        instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
        for _ in 0..3 {
            instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
            instrument.process(&ProcessContext::default(), &mut [0.0; 256]);
            reference.process(&ProcessContext::default(), &mut [0.0; 256]);
            instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        }
        for core in [&mut instrument, &mut reference] {
            core.event(NoteEvent::On { key: 60, velocity: 0.8 });
            core.process(&ProcessContext::default(), &mut [0.0; 256]);
            core.event(NoteEvent::Controller { number: 64, value: 0.0 });
        }
        let mut actual = [0.0; 2048];
        let mut expected = [0.0; 2048];
        instrument.process(&ProcessContext::default(), &mut actual);
        reference.process(&ProcessContext::default(), &mut expected);
        assert!(expected[1024..].iter().any(|value| value.abs() > 0.001));
        assert!(actual[1024..] == expected[1024..], "source {source}: only the held retrigger may remain");
        instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        instrument.process(&ProcessContext::default(), &mut actual);
        assert!(actual[1024..].iter().all(|value| *value == 0.0));
    }
}

#[test]
fn retiring_a_hidden_sample_source_preserves_the_pedal_for_the_next_note() {
    let mut instrument = Instrument::new();
    instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
    instrument.set_param(param::SOURCE, 1.0);
    instrument.set_param(param::PLAYBACK, 2.0);
    instrument.set_param(param::RELEASE, 0.005);
    instrument.reset();
    instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
    let mut block = [0.0; 2048];
    for source in [1.0, 0.0, 1.0] {
        instrument.set_param(param::SOURCE, source);
        instrument.process(&ProcessContext::default(), &mut block);
    }
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
    instrument.process(&ProcessContext::default(), &mut block);
    assert!(block[1024..].iter().any(|value| value.abs() > 0.01));
    instrument.event(NoteEvent::Controller { number: 64, value: 0.0 });
    instrument.process(&ProcessContext::default(), &mut block);
    assert!(block[1024..].iter().all(|value| *value == 0.0));
}

fn bar_instrument(body: f64, model: f64) -> Instrument {
    let mut instrument = Instrument::new();
    instrument.set_param(param::REVISION, 3.0);
    instrument.set_param(param::BODY, body);
    instrument.set_param(param::MALLET_MODEL, model);
    instrument.reset();
    instrument
}

fn active_voices(instrument: &Instrument) -> usize {
    instrument.voices.iter().filter(|voice| voice.body.is_active()).count()
}

#[test]
fn revision_three_struck_is_revision_one_and_a_stored_one_reads_as_three() {
    let render = |revision: f64, body: f64| {
        let mut instrument = Instrument::new();
        instrument.set_param(param::REVISION, revision);
        instrument.set_param(param::BODY, body);
        instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
        let mut buffer = vec![0.0; 16384];
        instrument.process(&ProcessContext::default(), &mut buffer);
        buffer
    };
    assert_eq!(render(3.0, 0.0), render(1.0, 0.0), "the Struck body is the revision 1 renderer");
    for body in [1.0, 2.0] {
        let bar = render(3.0, body);
        assert_eq!(render(1.0, body), bar, "a stored 1 is read as revision 3");
        assert_ne!(bar, render(3.0, 0.0));
        assert!(render(2.0, body).iter().all(|x| *x == 0.0), "revision 2 needs an accepted profile");
    }
    assert_eq!(PARAMS[param::REVISION].default, 1.0, "a project without a stored revision is revision 1");
    assert_eq!(MODEL_REVISION, 3, "a new Instrument stores revision 3");
}

#[test]
fn a_roll_strikes_one_bar_and_keeps_its_motion() {
    for (body, model) in [(1.0, 0.0), (1.0, 1.0), (2.0, 0.0), (2.0, 1.0)] {
        let mut instrument = bar_instrument(body, model);
        instrument.set_param(param::DECAY, 20.0);
        let mut block = [0.0; 256];
        for key in 40..55 {
            instrument.event(NoteEvent::On { key, velocity: 0.7 });
        }
        for _ in 0..64 {
            instrument.event(NoteEvent::On { key: 72, velocity: 0.9 });
            instrument.process(&ProcessContext::default(), &mut block);
            instrument.event(NoteEvent::Off { key: 72, velocity: 0.0 });
            instrument.process(&ProcessContext::default(), &mut block);
        }
        assert_eq!(instrument.voices.iter().filter(|voice| voice.key == 72).count(), 1, "body {body}: one rolled bar");
        let held =
            instrument.voices.iter().filter(|voice| (40..55).contains(&voice.key) && voice.body.is_active()).count();
        assert_eq!(held, 15, "body {body}: the roll steals none of the other bars");
    }
}

#[test]
fn a_new_body_on_a_ringing_key_takes_another_voice() {
    let mut instrument = bar_instrument(1.0, 0.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    instrument.set_param(param::BODY, 2.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    instrument.set_param(param::BODY, 0.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    let mut block = [0.0; 64];
    instrument.process(&ProcessContext::default(), &mut block);
    assert_eq!(active_voices(&instrument), 3);
}

#[test]
fn marimba_rings_through_note_off_and_vibraphone_damps_unless_the_pedal_holds() {
    let tail = |body: f64, pedal: bool| {
        let mut instrument = bar_instrument(body, 0.0);
        instrument.set_param(param::DECAY, 20.0);
        instrument.set_param(param::RELEASE, 0.01);
        if pedal {
            instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
        }
        instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
        instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        let mut block = vec![0.0; 9600];
        instrument.process(&ProcessContext::default(), &mut block);
        let late = block[8000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        (instrument, late)
    };
    assert!(tail(1.0, false).1 > 0.01, "marimba rings freely");
    assert!(tail(2.0, false).1 < 1e-4, "vibraphone is damped on note-off");
    let (mut held, late) = tail(2.0, true);
    assert!(late > 0.01, "the pedal holds the vibraphone");
    held.event(NoteEvent::Controller { number: 64, value: 0.0 });
    let mut block = vec![0.0; 9600];
    held.process(&ProcessContext::default(), &mut block);
    assert!(block[8000..].iter().all(|x| x.abs() < 1e-4), "lifting the pedal damps released bars");
    let (mut marimba, _) = tail(1.0, false);
    marimba.event(NoteEvent::AllNotesOff);
    marimba.process(&ProcessContext::default(), &mut block);
    assert!(block[8000..].iter().all(|x| x.abs() < 1e-4), "all notes off damps every bar");
}

#[test]
fn bar_performances_are_identical_across_block_partitions() {
    for (body, model) in [(1.0, 0.0), (1.0, 1.0), (2.0, 0.0), (2.0, 1.0)] {
        let performance = |block_frames: usize| {
            let mut instrument = bar_instrument(body, model);
            let mut rendered = Vec::new();
            for step in 0..48 {
                match step % 6 {
                    0 => instrument
                        .event(NoteEvent::On { key: 60 + (step % 5) as u8, velocity: 0.3 + step as f32 / 80.0 }),
                    1 => instrument.event(NoteEvent::On { key: 60, velocity: 1.0 }),
                    2 => instrument.event(NoteEvent::PitchBend((step as f32 / 48.0) - 0.5)),
                    3 => instrument.event(NoteEvent::Controller { number: 64, value: (step % 12) as f32 / 6.0 }),
                    4 => instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 }),
                    _ => instrument.set_param(param::HARDNESS, step as f64 / 48.0),
                }
                // 37 frames per event: contacts span block boundaries at every partition.
                let mut section = vec![0.0; 37 * 2];
                for block in section.chunks_mut(block_frames * 2) {
                    instrument.process(&ProcessContext::default(), block);
                }
                rendered.extend(section);
            }
            rendered
        };
        let whole = performance(37);
        assert!(whole.iter().any(|x| x.abs() > 0.01));
        assert_eq!(performance(1), whole, "body {body}, model {model}");
        assert_eq!(performance(5), whole, "body {body}, model {model}");
    }
}

#[test]
fn hostile_bar_settings_and_events_stay_finite() {
    let mut instrument = bar_instrument(1.0, 0.0);
    for index in [param::BODY, param::HARDNESS, param::MALLET_MODEL, param::STRIKE, param::DECAY, param::QUALITY] {
        for value in [f64::NAN, f64::INFINITY, -1e300, 1e300] {
            instrument.set_param(index, value);
        }
    }
    for velocity in [f32::MIN_POSITIVE, 1e-30, 1.0, f32::MAX, f32::NAN] {
        for key in [0, 1, 60, 126, 127, 200] {
            instrument.event(NoteEvent::On { key, velocity });
        }
    }
    let mut block = [0.0; 4096];
    instrument.process(&ProcessContext::default(), &mut block);
    assert!(block.iter().all(|x| x.is_finite() && x.abs() < 16.0));
    assert!(std::mem::size_of::<Voice>() <= 2048, "{}", std::mem::size_of::<Voice>());
    assert!(std::mem::size_of::<Instrument>() <= 256 * 1024);
}

/// Self-authored regression anchors of both profiles and both mallet models, written from this
/// implementation (`SUHINA_BLESS=1`), not an independent reference.
#[test]
fn bar_bodies_match_their_render_anchors() {
    const FRAMES: [usize; 16] = [0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 377, 987, 2584, 6765];
    let mut text = String::from(
        "# CC0-1.0: self-authored regression anchors of Instrument revision 3 bar bodies, written by\n\
         # this implementation (SUHINA_BLESS=1 cargo test -p suhina-device-instrument anchors).\n\
         # Body, mallet model, frame, left channel; defaults, key 60, velocity 0.8, 48000 Hz.\n",
    );
    let mut rendered = Vec::new();
    for (body, name) in [(1.0, "marimba"), (2.0, "vibraphone")] {
        for (model, model_name) in [(0.0, "contact"), (1.0, "pulse")] {
            let mut instrument = bar_instrument(body, model);
            instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
            let mut buffer = vec![0.0; 8192 * 2];
            instrument.process(&ProcessContext::default(), &mut buffer);
            for frame in FRAMES {
                text.push_str(&format!("{name} {model_name} {frame} {:.9e}\n", buffer[frame * 2]));
                rendered.push((name, model_name, frame, buffer[frame * 2]));
                assert_eq!(buffer[frame * 2], buffer[frame * 2 + 1]);
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/modal-v2-bar-anchors.txt");
    if suhina_test_support::blessing() {
        std::fs::write(&path, text).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(&path).unwrap();
    let anchors: Vec<_> = stored.lines().filter(|line| !line.starts_with('#')).collect();
    assert_eq!(anchors.len(), rendered.len());
    for (line, (name, model, frame, value)) in anchors.iter().zip(rendered) {
        let fields: Vec<_> = line.split(' ').collect();
        assert_eq!(fields[..3], [name, model, frame.to_string().as_str()]);
        let expected: f32 = fields[3].parse().unwrap();
        assert!(
            (value - expected).abs() <= 2e-6 + expected.abs() * 1e-5,
            "{name} {model} frame {frame}: {value} vs {expected}"
        );
    }
}
