// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::NoteEvent;
use suhina_device_sdk::native_node::NativeNode;
use suhina_device_sdk::{
    Connection as C, DeviceGraph, DeviceRole, DeviceSdkError, Node as N, NodeKind as K, ProcessContext,
};

struct Sum;
impl NativeNode for Sum {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (i, sample) in output.iter_mut().enumerate() {
            *sample = inputs.iter().map(|input| input[i]).sum();
        }
    }
    fn event(&mut self, _: NoteEvent) {}
    fn reset(&mut self) {}
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
fn graph() -> DeviceGraph {
    let mut g = DeviceGraph::new("org.test.native-routing", "Native routing", DeviceRole::Effect);
    g.nodes =
        vec![N::audio_input("in"), N::new("fx", K::NativeEffect { effect: "sum".into() }), N::audio_output("out")];
    g.connections =
        vec![C::new("in", "fx"), C::ports("in", "left", "fx", "control1"), C::ports("fx", "right", "out", "in")];
    g
}
#[test]
fn trusted_effect_routes_stereo_channels_and_default_inputs_after_roundtrip() {
    let json = graph().to_sd().unwrap();
    let g: DeviceGraph = DeviceGraph::from_sd(&json).unwrap();
    let mut prepared = g
        .compile_with_native_at(48000., |id, rate| {
            assert_eq!(id, "sum");
            assert_eq!(rate, 48000.);
            Ok(Box::new(Sum))
        })
        .unwrap();
    let mut audio = [0.25, 0.5, -0.5, 0.25];
    prepared.process(&mut audio);
    assert_eq!(audio, [0.75, 0.75, -0.25, -0.25]);
}
#[test]
fn unresolved_native_effect_is_rejected() {
    assert!(graph().compile_with_native_at(48000., |_, _| Err(DeviceSdkError("unknown kernel".into()))).is_err());
    assert!(graph().compile_at(48000.).is_err());
}

#[test]
fn native_effect_feedback_uses_previous_frame_and_reset_clears_delay() {
    let mut g = graph();
    g.nodes.push(N::new("delay", K::SampleDelay));
    g.connections = vec![
        C::new("in", "fx"),
        C::ports("delay", "out", "fx", "control1"),
        C::new("fx", "delay"),
        C::new("fx", "out"),
    ];
    let mut prepared = g.compile_with_native_at(48000., |_, _| Ok(Box::new(Sum))).unwrap();
    for _ in 0..2 {
        let mut expected = [0.0f32; 2];
        for frames in [1, 65, 129] {
            let mut audio: Vec<f32> = (0..frames).flat_map(|_| [0.25, -0.5]).collect();
            let mut reference = audio.clone();
            for frame in reference.as_chunks_mut::<2>().0 {
                expected = std::array::from_fn(|channel| expected[channel] + frame[channel]);
                *frame = expected;
            }
            prepared.process(&mut audio);
            assert_eq!(audio, reference);
        }
        prepared.reset();
    }
}

struct EventEffect(f32);
impl NativeNode for EventEffect {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        for (sample, input) in output.iter_mut().zip(inputs[0]) {
            *sample = *input + self.0;
        }
    }
    fn event(&mut self, _: NoteEvent) {
        self.0 = 0.5;
    }
    fn reset(&mut self) {
        self.0 = 0.0;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
#[test]
fn native_effect_receives_timestamped_events_and_reset() {
    let mut g = graph();
    g.connections = vec![C::new("in", "fx"), C::new("fx", "out")];
    let mut prepared = g.compile_with_native_at(48000., |_, _| Ok(Box::new(EventEffect(0.0)))).unwrap();
    let mut audio = [0.0; 260];
    prepared.process_instrument_context(
        &ProcessContext::default(),
        &[(65, NoteEvent::On { key: 60, velocity: 1.0 })],
        &mut audio,
    );
    assert!(audio[..130].iter().all(|&sample| sample == 0.0));
    assert!(audio[130..].iter().all(|&sample| sample == 0.5));
    prepared.reset();
    audio.fill(0.0);
    prepared.process(&mut audio);
    assert!(audio.iter().all(|&sample| sample == 0.0));
}
