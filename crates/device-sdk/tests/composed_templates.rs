// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use suhina_device_api::NoteEvent as E;
use suhina_device_sdk::sample_resource::{PreparedSamples, SampleAsset, SliceMode};
use suhina_device_sdk::{DeviceGraph, ProcessContext, granular_template, sampler_template};
fn samples() -> PreparedSamples {
    let pcm = (0..12000).map(|i| (i as f32 * 0.07).sin() * 0.25).collect::<Vec<_>>();
    let mut s = PreparedSamples::default();
    s.insert("sample", Arc::new(SampleAsset::new(Arc::new(pcm), 48000, 1).unwrap().with_tempo(120.).unwrap())).unwrap();
    s
}
fn render(g: &DeviceGraph, block: usize) -> Vec<f32> {
    let mut p = g.compile_with_samples_at(48000., &samples()).unwrap();
    let events =
        [(3, E::On { key: 60, velocity: 0.8 }), (913, E::PitchBend(0.5)), (1701, E::Off { key: 60, velocity: 0. })];
    let mut out = vec![0.; 4800 * 2];
    for (i, chunk) in out.chunks_mut(block * 2).enumerate() {
        let start = i * block;
        let events = events
            .iter()
            .filter(|(at, _)| *at >= start && *at < start + chunk.len() / 2)
            .map(|(at, e)| (at - start, *e))
            .collect::<Vec<_>>();
        p.process_instrument_context(&ProcessContext::default(), &events, chunk);
    }
    assert!(out.iter().all(|v| v.is_finite()));
    assert!(out.iter().any(|v| v.abs() > 0.0001));
    p.reset();
    out
}
#[test]
fn editable_templates_roundtrip_and_render_identically_across_blocks() {
    for graph in [sampler_template("sample", 2).unwrap(), granular_template("sample", 2, 8, 42).unwrap()] {
        let graph = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
        let expected = render(&graph, 1);
        for size in [7, 64, 129, 512] {
            let actual = render(&graph, size);
            let error = expected.iter().zip(actual).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
            assert!(error < 1e-6, "{} block {size}: {error}", graph.name);
        }
    }
}
#[test]
fn prepared_slice_analysis_is_bounded_and_persists_only_boundaries() {
    let s = samples();
    for mode in [
        SliceMode::Manual(vec![100, 100, 200]),
        SliceMode::TimeGrid { seconds: 0.05 },
        SliceMode::BeatGrid { beats: 0.125, bpm: None },
        SliceMode::Transients { sensitivity: 0.5 },
    ] {
        let node = s.slice_map("sample", mode).unwrap();
        if let suhina_device_sdk::NodeKind::SliceMap { boundaries, .. } = node {
            assert_eq!(boundaries.first(), Some(&0));
            assert_eq!(boundaries.last(), Some(&12000));
            assert!(boundaries.windows(2).all(|p| p[0] < p[1]));
            assert!(boundaries.len() <= 4096);
        } else {
            panic!("wrong kind")
        }
    }
    assert!(s.slice_map("missing", SliceMode::Manual(vec![])).is_err());
    assert!(s.slice_map("sample", SliceMode::TimeGrid { seconds: f64::NAN }).is_err());
    assert!(s.slice_map("sample", SliceMode::Manual(vec![12001])).is_err());
    assert!(s.slice_map("sample", SliceMode::TimeGrid { seconds: 1e-10 }).is_err());
}

#[test]
fn voice_and_grain_callbacks_never_allocate_or_free() {
    // Verify that the test allocator is active before accepting a zero count.
    let probe = allocation_counter::measure(|| {
        std::hint::black_box(vec![1u8; 128]);
    });
    assert!(probe.count_total > 0);
    for graph in [sampler_template("sample", 4).unwrap(), granular_template("sample", 4, 8, 42).unwrap()] {
        let mut prepared = None;
        let memory = allocation_counter::measure(|| {
            prepared = Some(graph.compile_with_samples_at(48000., &samples()).unwrap());
        });
        let mut prepared = prepared.unwrap();
        let mut buffer = [0.; 258];
        let allocations = allocation_counter::measure(|| {
            for i in 0..16 {
                prepared.process_instrument_context(
                    &ProcessContext::default(),
                    &[
                        (0, E::On { key: 60 + i, velocity: 0.8 }),
                        (17, E::Controller { number: 64, value: 1. }),
                        (25, E::Off { key: 60 + i, velocity: 0. }),
                    ],
                    &mut buffer,
                );
            }
            prepared.process_instrument_context(
                &ProcessContext::default(),
                &[(3, E::Controller { number: 64, value: 0. }), (29, E::AllNotesOff)],
                &mut buffer,
            );
            prepared.set_parameter(0, 0.3);
            prepared.reset();
        });
        assert_eq!(allocations.count_total, 0, "{} allocations: {allocations:?}", graph.name);
        assert_eq!(allocations.count_current, 0, "{} freed state: {allocations:?}", graph.name);
        println!("{}: prepared resident {} bytes, peak {} bytes", graph.name, memory.bytes_current, memory.bytes_max);
    }
}
#[test]
#[ignore = "local CPU measurement; not a portable deadline assertion"]
fn composed_cpu_measurement() {
    use suhina_device_api::Device;
    use suhina_device_sdk::{Connection as C, DeviceRole, Node as N, NodeKind as K};
    let mut coarse = DeviceGraph::new("org.test.coarse-bench", "Native sampler", DeviceRole::Instrument);
    coarse.nodes = vec![N::new("player", K::SamplerAsset { sample: "sample".into() }), N::audio_output("out")];
    coarse.connections = vec![C::new("player", "out")];
    for mut graph in [
        coarse,
        sampler_template("sample", 1).unwrap(),
        sampler_template("sample", 4).unwrap(),
        sampler_template("sample", 8).unwrap(),
        granular_template("sample", 4, 16, 42).unwrap(),
    ] {
        for parameter in &mut graph.parameters {
            if parameter.id == "playback" {
                parameter.default = 2.;
            }
        }
        let mut p = graph.compile_with_samples_at(48000., &samples()).unwrap();
        if let Some(sampler) = p.sampler_mut("player") {
            let index = sampler.info().param_index("playback").unwrap();
            sampler.set_param(index, 2.);
        }
        let notes = (0..8).map(|i| (0, E::On { key: 60 + i, velocity: 0.8 })).collect::<Vec<_>>();
        let mut b = [0.; 128];
        p.process_instrument_context(&ProcessContext::default(), &notes, &mut b);
        let start = std::time::Instant::now();
        for _ in 0..750 {
            p.process_instrument_context(&ProcessContext::default(), &[], std::hint::black_box(&mut b));
        }
        println!(
            "{} {:?}: one second audio rendered in {:?}",
            graph.name,
            graph.nodes.first().map(|n| &n.kind).and_then(|kind| if let K::VoiceGroup { voices, .. } = kind {
                Some(*voices)
            } else {
                None
            }),
            start.elapsed()
        );
    }
}
