// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_device_api::Role;

#[test]
fn every_registry_device_has_collision_free_stable_parameter_ids() {
    for info in suhina_device_registry::DEVICES {
        let parameters = Parameters::new(info).unwrap();
        for (index, parameter) in info.params.iter().enumerate() {
            assert_eq!(parameters.index(parameter_id(&parameter.id)), Some(index), "{}", info.id);
        }
    }
}

#[test]
fn native_and_adapter_render_identically_for_every_audio_device() {
    let context = ProcessContext { tempo: 137.0, beat: 4.0, playing: true };
    for info in suhina_device_registry::DEVICES {
        if info.role == Role::MidiEffect {
            continue;
        }
        let parameters = Parameters::new(info).unwrap();
        let mut adapter = Processor::new(&parameters, 48_000.0, 256).unwrap();
        let mut stock = suhina_device_registry::create(&info.id, 48_000.0).unwrap();
        stock.reset();
        let note = NoteEvent::On { key: 60, velocity: 0.8 };
        let mut actual: Vec<f32> = (0..512).map(|i| (i as f32 * 0.17).sin() * 0.2).collect();
        let mut expected = actual.clone();
        stock.set_context(&context);
        stock.process(&context, &mut expected[..74]);
        let at_note = ProcessContext { beat: context.beat + 37.0 * context.tempo / (60.0 * 48_000.0), ..context };
        stock.set_context(&at_note);
        stock.event(note);
        stock.process(&at_note, &mut expected[74..]);
        adapter
            .process(&parameters, context, &mut actual, None, &[Event { frame: 37, kind: EventKind::Note(note) }])
            .unwrap();
        assert_eq!(actual, expected, "{}", info.id);
    }
}

#[test]
fn automation_and_key_input_follow_stock_sample_boundaries() {
    let info = suhina_device_registry::DEVICES.iter().find(|info| info.id == "compressor").unwrap();
    let parameters = Parameters::new(info).unwrap();
    let mut adapter = Processor::new(&parameters, 48_000.0, 128).unwrap();
    let mut stock = suhina_device_registry::create(&info.id, 48_000.0).unwrap();
    stock.reset();
    let index = 0;
    let value = info.params[index].max;
    let mut actual = vec![0.3; 256];
    let mut expected = actual.clone();
    let key = vec![0.9; 256];
    let context = ProcessContext::default();
    stock.process_keyed(&context, &key[..34], &mut expected[..34]);
    stock.set_param(index, value);
    stock.process_keyed(&context, &key[34..], &mut expected[34..]);
    adapter
        .process(
            &parameters,
            context,
            &mut actual,
            Some(&key),
            &[Event { frame: 17, kind: EventKind::Parameter { id: parameters.id(index).unwrap(), value } }],
        )
        .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn malformed_blocks_and_unsorted_events_never_change_parameters() {
    let info = suhina_device_registry::DEVICES[0];
    let parameters = Parameters::new(info).unwrap();
    let mut adapter = Processor::new(&parameters, 48_000.0, 16).unwrap();
    let before = state::save(&parameters);
    let event =
        Event { frame: 8, kind: EventKind::Parameter { id: parameters.id(0).unwrap(), value: info.params[0].max } };
    assert_eq!(
        adapter.process(&parameters, ProcessContext::default(), &mut [0.0; 33], None, &[]),
        Err(ExportError::InvalidBlock)
    );
    assert_eq!(
        adapter.process(
            &parameters,
            ProcessContext::default(),
            &mut [0.0; 32],
            None,
            &[event, Event { frame: 7, ..event }]
        ),
        Err(ExportError::InvalidEvents)
    );
    assert_eq!(before, state::save(&parameters));
    assert!(Processor::new(&parameters, f64::NAN, 16).is_err());
    assert!(Processor::new(&parameters, 48_000.0, usize::MAX).is_err());
}

#[test]
fn state_round_trips_all_parameters_and_rejects_partial_invalid_input_atomically() {
    for info in suhina_device_registry::DEVICES {
        let source = Parameters::new(info).unwrap();
        for (index, parameter) in info.params.iter().enumerate() {
            source.set(source.id(index).unwrap(), parameter.max);
        }
        let bytes = state::save(&source);
        let destination = Parameters::new(info).unwrap();
        state::load(&destination, &bytes).unwrap();
        assert_eq!(state::save(&destination), bytes);
        for length in [0, 5, 8, bytes.len().saturating_sub(1)] {
            assert!(state::load(&destination, &bytes[..length]).is_err());
            assert_eq!(state::save(&destination), bytes);
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(state::load(&destination, &extra).is_err());
        assert_eq!(state::save(&destination), bytes);
        let mut non_finite = bytes.clone();
        let end = non_finite.len();
        non_finite[end - 8..].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(state::load(&destination, &non_finite).is_err());
        assert_eq!(state::save(&destination), bytes);
    }
}
