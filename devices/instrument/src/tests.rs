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
    instrument.set_param(param::REVISION, 2.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut buffer = [1.0; 1024];
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(!instrument.model_supported());
    assert!(buffer.iter().all(|value| *value == 0.0));
    assert_eq!(instrument.param(param::REVISION), Some(2.0));
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

/// A note held past the end of a looping, decaying sample and released, at `source`, with one
/// parameter (by id) set to another value first.
fn render_with(source: f64, change: Option<(&str, f64)>) -> Vec<f32> {
    let mut instrument = Instrument::new();
    let sample: Vec<f32> = (0..9600).map(|i| (i as f32 * 0.013).sin() * (1.0 - i as f32 / 9600.0)).collect();
    instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(sample));
    instrument.set_param(param::SOURCE, source);
    instrument.set_param(param::PLAYBACK, 2.0);
    if let Some((id, value)) = change {
        let index = PARAMS.iter().position(|info| info.id == id).unwrap();
        instrument.set_param(index, value);
    }
    instrument.reset();
    let mut out = vec![0.0; 2 * 32768];
    instrument.event(NoteEvent::On { key: 64, velocity: 0.8 });
    let (held, released) = out.split_at_mut(2 * 16384);
    instrument.process(&ProcessContext::default(), held);
    instrument.event(NoteEvent::Off { key: 64, velocity: 0.0 });
    instrument.process(&ProcessContext::default(), released);
    out
}

/// The panel dims a control exactly when it cannot change the sound: in each Source, every
/// control its rule calls inactive leaves the render bit for bit the same, and every one it calls
/// active changes it. Controls without a rule change the sound in every Source.
#[test]
fn inactive_controls_are_the_ones_that_change_nothing_in_that_source() {
    let changes = [
        ("quality", 0.0),
        ("body_decay", 0.3),
        ("brightness", 1.0),
        ("structure", 0.04),
        ("strike", 0.6),
        ("blend", 0.1),
        ("root", 48.0),
        ("start", 0.3),
        ("end", 0.5),
        ("loop_start", 0.2),
        ("loop_end", 0.6),
        ("playback", 1.0),
        ("attack", 0.05),
        ("sustain", 0.3),
    ];
    for source in [0.0, 1.0, 2.0] {
        let plain = render_with(source, None);
        assert!(plain.iter().any(|v| v.abs() > 0.001), "source {source} plays");
        for (id, value) in changes {
            let rule = active::active_rule(id).unwrap_or_else(|| panic!("{id} has a rule"));
            let lookup = |parameter: &str, _: bool, _: bool| {
                if parameter == "source" {
                    source
                } else {
                    PARAMS.iter().find(|info| info.id == parameter).map_or(0.0, |info| info.default)
                }
            };
            let active = rule.condition.evaluate(&lookup) > 0.0;
            let changed = render_with(source, Some((id, value))) != plain;
            assert_eq!(changed, active, "{id} at source {source}: changes the sound {changed}, drawn active {active}");
        }
        for (id, value) in [
            ("gain", -20.0),
            ("release", 2.0),
            ("key_low", 70.0),
            ("key_high", 50.0),
            ("velocity_low", 0.9),
            ("velocity_high", 0.5),
        ] {
            assert!(active::active_rule(id).is_none(), "{id} acts on both layers");
            assert_ne!(render_with(source, Some((id, value))), plain, "{id} at source {source}");
        }
    }
    assert_eq!(
        active::active_rule(active::WAVEFORM).map(|r| r.condition),
        active::active_rule("start").map(|r| r.condition)
    );
}

/// Every control of the compact panel and the expanded editor that edits a parameter dims by
/// that parameter's rule, each caption with its control, the waveform by the sample layer's;
/// page buttons and labels of their own never dim.
#[test]
fn both_presentations_dim_each_control_and_caption_by_its_parameter() {
    use suhina_device_sdk::ui::Widget;
    let layout = panel::layout();
    for ui in [layout.ui.unwrap(), layout.expanded.unwrap()] {
        for (index, control) in ui.controls.iter().enumerate() {
            let bound = control.x.iter().chain(control.y.iter()).next().map(|b| b.parameter.as_str());
            let expected = match (&control.widget, bound) {
                (_, Some(parameter)) => active::active_rule(parameter),
                (Some(Widget::SampleWaveform), None) => active::active_rule(active::WAVEFORM),
                // A number's caption sits right before it and dims with it.
                (Some(Widget::Label { .. }), None) => match ui.controls.get(index + 1) {
                    Some(next) if matches!(next.widget, Some(Widget::Number { .. })) => {
                        next.y.as_ref().and_then(|b| active::active_rule(&b.parameter))
                    }
                    _ => control.active_when.clone(),
                },
                _ => None,
            };
            assert_eq!(control.active_when, expected, "{}", control.name);
        }
        let label = |name: &str| {
            ui.controls
                .iter()
                .find(|c| c.name == name && matches!(c.widget, Some(Widget::Label { .. })))
                .map(|c| c.active_when.clone())
        };
        if ui.size[0] < 700.0 {
            assert_eq!(label("Drop a local sample to play it"), Some(active::active_rule(active::WAVEFORM)));
            assert_eq!(label("STRUCK MODAL · experimental v1"), Some(None));
        } else {
            assert_eq!(label("Modes per voice"), Some(active::active_rule("quality")));
            assert_eq!(label("Playback"), Some(active::active_rule("playback")));
            assert_eq!(label("Root key"), Some(active::active_rule("root")));
            assert_eq!(label("Lowest key"), Some(None));
        }
    }
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
    assert!(instrument.voices.iter().all(|voice| !voice.bank.is_active()));
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
