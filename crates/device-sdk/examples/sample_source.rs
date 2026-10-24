// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Render a resource-bound polyphonic source through an ordinary SDK gain node.
use std::sync::Arc;
use suhina_device_api::{Device, NoteEvent};
use suhina_device_sdk::{DeviceRole, NodeKind, Parameter, ProcessContext, author::GraphBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut builder = GraphBuilder::new("org.example.sample-source", "Sample source", DeviceRole::Instrument);
    let source = builder.node("source", NodeKind::Sampler)?;
    let gain = builder.parameter_signal(Parameter::new("gain", "Gain", (0., 1.), 0.5))?;
    let scaled = builder.op("scaled", NodeKind::Multiply, &[("a", source.out()), ("b", gain.out())])?;
    let output = builder.node("out", NodeKind::AudioOutput)?;
    builder.connect(scaled.out(), output.input("in"))?;
    let graph = builder.finish()?;
    let mut prepared = graph.compile_instrument_at(48_000.)?;
    let source = prepared.sampler_mut("source").ok_or("missing sample source")?;
    // Prepare and bind resources before entering the audio callback.
    let pcm: Vec<f32> = (0..48_000).map(|i| (i as f32 * 440. * std::f32::consts::TAU / 48_000.).sin() * 0.25).collect();
    source.load_sample(48_000, 1, Arc::from(pcm));
    let mut audio = vec![0.; 96_000];
    prepared.process_instrument_context(
        &ProcessContext::default(),
        &[(0, NoteEvent::On { key: 60, velocity: 1. }), (24_000, NoteEvent::AllNotesOff)],
        &mut audio,
    );
    println!(
        "Rendered {} stereo frames; peak {}",
        audio.len() / 2,
        audio.iter().fold(0f32, |peak, v| peak.max(v.abs()))
    );
    Ok(())
}
