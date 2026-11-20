// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn completion_capacity_is_reserved_before_admission_and_small_drains_lose_nothing() {
    let a = NoteInstanceId::from_parts(1, 1).unwrap();
    let b = NoteInstanceId::from_parts(1, 2).unwrap();
    let c = NoteInstanceId::from_parts(1, 3).unwrap();
    let mut lifetimes = NoteLifetimes::<2>::default();
    lifetimes.admit(a).unwrap();
    lifetimes.admit(b).unwrap();
    assert_eq!(lifetimes.admit(c), Err(NoteInstanceDispatchError::Capacity));
    lifetimes.finish(a);
    lifetimes.finish(b);
    assert_eq!(lifetimes.admit(c), Err(NoteInstanceDispatchError::Capacity));
    assert_eq!(lifetimes.drain(&mut []), 0);
    let mut one = [None];
    assert_eq!(lifetimes.drain(&mut one), 1);
    assert_eq!(one, [Some(a)]);
    lifetimes.admit(c).unwrap();
    assert_eq!(lifetimes.drain(&mut one), 1);
    assert_eq!(one, [Some(b)]);
    assert!(lifetimes.contains(c));
    lifetimes.finish_all();
    assert_eq!(lifetimes.drain(&mut one), 1);
    assert_eq!(one, [Some(c)]);
    assert_eq!(lifetimes.drain(&mut one), 0);
}
use crate::{DeviceError, DeviceInfo, ProcessContext};

#[test]
fn identities_are_distinct_across_origins_and_repeated_allocations() {
    let mut input = NoteIdAllocator::new(1).unwrap();
    let mut clip = NoteIdAllocator::new(2).unwrap();
    let first = input.allocate().unwrap();
    let repeated_key = input.allocate().unwrap();
    let same_pitch_from_clip = clip.allocate().unwrap();
    assert_ne!(first, repeated_key);
    assert_ne!(first, same_pitch_from_clip);
    assert_eq!(NoteInstanceId::from_parts(first.origin(), first.serial()), Some(first));
}

#[test]
fn exhausted_allocator_does_not_recycle_an_old_identity() {
    let mut ids = NoteIdAllocator::new(42).unwrap();
    ids.next = NonZeroU64::new(u64::MAX);
    let last = ids.allocate().unwrap();
    assert_eq!(last.serial(), u64::MAX);
    assert_eq!(ids.allocate(), None);
    assert_eq!(ids.allocate(), None);
}

#[test]
fn zero_identity_components_are_rejected() {
    assert!(NoteIdAllocator::new(0).is_none());
    assert_eq!(NoteInstanceId::from_parts(0, 1), None);
    assert_eq!(NoteInstanceId::from_parts(1, 0), None);
}

#[test]
fn adjacent_32_bit_pressure_values_remain_distinct() {
    let a = f64::from(u32::MAX / 2) / f64::from(u32::MAX);
    let b = f64::from(u32::MAX / 2 + 1) / f64::from(u32::MAX);
    let mut state = NoteExpressionState::NEUTRAL;
    assert!(state.set(NoteExpressionKind::Pressure, a));
    let before = state.value(NoteExpressionKind::Pressure);
    assert!(state.set(NoteExpressionKind::Pressure, b));
    assert_ne!(before, state.value(NoteExpressionKind::Pressure));
    assert_eq!(before, a);
    assert_eq!(state.value(NoteExpressionKind::Pressure), b);
}

#[test]
fn malformed_expression_cannot_poison_the_previous_state() {
    let original = NoteExpressionState::new(-3.25, 0.8, 0.2).unwrap();
    for kind in [NoteExpressionKind::Pitch, NoteExpressionKind::Pressure, NoteExpressionKind::Timbre] {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut state = original;
            assert!(!state.set(kind, value));
            assert_eq!(state, original);
            assert!(NoteInstanceEvent::Expression { kind, value }.validated().is_none());
        }
    }
    assert!(NoteExpressionState::new(1.0, f64::NAN, 0.5).is_none());
}

#[test]
fn finite_extremes_are_bounded_without_changing_neutral_defaults() {
    let state = NoteExpressionState::new(f64::MAX, -f64::MAX, f64::MAX).unwrap();
    assert_eq!(state.value(NoteExpressionKind::Pitch), 256.0);
    assert_eq!(state.value(NoteExpressionKind::Pressure), 0.0);
    assert_eq!(state.value(NoteExpressionKind::Timbre), 1.0);
    let default = NoteExpressionState::default();
    assert_eq!(default.value(NoteExpressionKind::Pitch), 0.0);
    assert_eq!(default.value(NoteExpressionKind::Pressure), 0.0);
    assert_eq!(default.value(NoteExpressionKind::Timbre), 0.5);
}

#[test]
fn malformed_keys_and_velocities_are_rejected_before_dispatch() {
    for key in 0..=u8::MAX {
        for velocity in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 0.5, 2.0] {
            let event = NoteInstanceEvent::On { key, velocity, expression: NoteExpressionState::NEUTRAL };
            let valid = event.validated();
            assert_eq!(valid.is_some(), key <= 127 && velocity.is_finite());
            if let Some(event) = valid {
                let NoteInstanceEvent::On { velocity, .. } = event else {
                    panic!("native validation must not change the event kind");
                };
                assert!((0.0..=1.0).contains(&velocity));
            }
        }
        assert_eq!(NoteInstanceEvent::Choke { key }.validated().is_some(), key <= 127);
    }
}

#[test]
fn native_zero_velocity_starts_an_identified_note_with_its_initial_expression() {
    let mut device = Endpoint { identified: true, ..Endpoint::default() };
    let id = NoteInstanceId::from_parts(7, 123).unwrap();
    let expression = NoteExpressionState::new(3.5, 0.8, 0.2).unwrap();
    let on = NoteInstanceEvent::On { key: 60, velocity: 0.0, expression };
    assert_eq!(dispatch_note_instance(&mut device, id, on), Ok(()));
    assert_eq!(device.received, Some((id, on)));

    // Native note-on, explicit release and immediate choke are different lifecycle events.
    let off = NoteInstanceEvent::Off { key: 60, velocity: 0.0 };
    let choke = NoteInstanceEvent::Choke { key: 60 };
    for event in [off, choke] {
        assert_eq!(dispatch_note_instance(&mut device, id, event), Ok(()));
        assert_eq!(device.received, Some((id, event)));
        assert_ne!(event, on);
    }
    assert_ne!(off, choke);
}

/// Records one call in fixed storage; it does not pretend to be an instrument implementation.
#[derive(Default)]
struct Endpoint {
    identified: bool,
    reject: Option<NoteInstanceDispatchError>,
    received: Option<(NoteInstanceId, NoteInstanceEvent)>,
}

static INFO: DeviceInfo = DeviceInfo::new("expression-test", "Expression test", crate::Role::Instrument, &[]);

impl Device for Endpoint {
    fn info(&self) -> &DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, rate: f64) -> Result<(), DeviceError> {
        crate::check_sample_rate(rate).map(|_| ())
    }

    fn reset(&mut self) {
        self.received = None;
    }

    fn set_param(&mut self, _index: usize, _value: f64) {}

    fn param(&self, _index: usize) -> Option<f64> {
        None
    }

    fn process(&mut self, _context: &ProcessContext, _buffer: &mut [f32]) {}

    fn note_instances(&mut self) -> Option<&mut dyn NoteInstanceDevice> {
        if self.identified { Some(self) } else { None }
    }
}

impl NoteInstanceDevice for Endpoint {
    fn event(&mut self, id: NoteInstanceId, event: NoteInstanceEvent) -> Result<(), NoteInstanceDispatchError> {
        if let Some(reason) = self.reject {
            return Err(reason);
        }
        self.received = Some((id, event));
        Ok(())
    }

    fn drain_finished(&mut self, _output: &mut [Option<NoteInstanceId>]) -> usize {
        0
    }
}

#[test]
fn unsupported_device_is_explicit_and_no_note_is_silently_dispatched() {
    let mut device = Endpoint::default();
    let id = NoteInstanceId::from_parts(1, 1).unwrap();
    let event = NoteInstanceEvent::On { key: 60, velocity: 1.0, expression: NoteExpressionState::NEUTRAL };
    assert_eq!(dispatch_note_instance(&mut device, id, event), Err(NoteInstanceDispatchError::Unsupported));
    assert_eq!(device.received, None);
}

#[test]
fn admission_failure_is_returned_to_the_adapter_without_a_started_note() {
    let id = NoteInstanceId::from_parts(1, 1).unwrap();
    let event = NoteInstanceEvent::On { key: 60, velocity: 1.0, expression: NoteExpressionState::NEUTRAL };
    for reason in [NoteInstanceDispatchError::Capacity, NoteInstanceDispatchError::AlreadyActive] {
        let mut device = Endpoint { identified: true, reject: Some(reason), ..Endpoint::default() };
        assert_eq!(dispatch_note_instance(&mut device, id, event), Err(reason));
        assert_eq!(device.received, None);
    }
}

#[test]
fn identified_dispatch_carries_initial_state_atomically_and_rejects_bad_followup() {
    let mut device = Endpoint { identified: true, ..Endpoint::default() };
    let id = NoteInstanceId::from_parts(3, 90).unwrap();
    let state = NoteExpressionState::new(19.25, 0.75, 0.1).unwrap();
    let event = NoteInstanceEvent::On { key: 42, velocity: 0.5, expression: state };
    assert_eq!(dispatch_note_instance(&mut device, id, event), Ok(()));
    assert_eq!(device.received, Some((id, event)));
    let bad = NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pressure, value: f64::NAN };
    assert_eq!(dispatch_note_instance(&mut device, id, bad), Err(NoteInstanceDispatchError::InvalidEvent));
    assert_eq!(device.received, Some((id, event)));
}
