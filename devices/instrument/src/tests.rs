// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

suhina_device_api::conformance_test!(Instrument::with_rate);

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
