// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use suhina_device_api::NoteEvent as E;
use suhina_device_sdk::sample_resource::{PreparedSamples, SampleAsset};
use suhina_device_sdk::{
    Connection as C, DeviceGraph, DeviceRole, Node as N, NodeKind as K, Parameter, ProcessContext,
};
fn samples(frames: usize) -> PreparedSamples {
    let mut s = PreparedSamples::default();
    s.insert("sample", Arc::new(SampleAsset::new(Arc::new(vec![0.25; frames]), 48000, 1).unwrap())).unwrap();
    s
}
fn source(kind: K) -> DeviceGraph {
    let mut g = DeviceGraph::new("org.test.player", "Player", DeviceRole::Instrument);
    g.nodes = vec![N::new("player", kind), N::audio_output("out")];
    g.connections = vec![C::new("player", "out")];
    g
}
fn render(g: &DeviceGraph, size: usize) -> Vec<f32> {
    let mut p = g.compile_with_samples_at(48000., &samples(12000)).unwrap();
    let mut out = vec![0.; 8192];
    for (i, b) in out.chunks_mut(size * 2).enumerate() {
        let e = if i == 0 { &[(3, E::On { key: 60, velocity: 1. })][..] } else { &[] };
        p.process_instrument_context(&ProcessContext::default(), e, b);
    }
    assert!(out.iter().all(|v| v.is_finite()));
    assert!(out.iter().any(|v| v.abs() > 0.001));
    out
}
#[test]
fn high_level_players_share_the_bounded_preparation_and_block_contract() {
    for kind in [
        K::SamplerAsset { sample: "sample".into() },
        K::TimeStretchPlayer { sample: "sample".into() },
        K::GranularPlayer { sample: "sample".into(), voices: 2, grains: 8, seed: 4 },
    ] {
        let g = source(kind);
        let g = DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap();
        let a = render(&g, 64);
        for size in [7, 129, 512] {
            let b = render(&g, size);
            let error = a.iter().zip(b).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
            assert!(error < 1e-6, "{} error {error}", g.nodes[0].kind.info().name);
        }
    }
}
#[test]
fn slice_playback_handles_reverse_gate_empty_and_single_frame_regions() {
    for (frames, reverse, one_shot) in
        [(4, false, true), (4, true, true), (1, false, true), (0, false, true), (4, false, false)]
    {
        let mut g = source(K::SlicePlayback { sample: "sample".into(), one_shot });
        g.nodes.extend([
            N::new("asset", K::SampleAsset { sample: "sample".into() }),
            N::new("region", K::SampleRegion),
            N::new("rate", K::Constant { value: if reverse { -1. } else { 1. } }),
        ]);
        g.connections.extend([
            C::ports("asset", "out", "region", "asset"),
            C::ports("region", "out", "player", "region"),
            C::ports("rate", "out", "player", "rate"),
        ]);
        let mut p = g.compile_with_samples_at(48000., &samples(frames)).unwrap();
        let mut out = [0.; 24];
        p.process_instrument_context(
            &ProcessContext::default(),
            &[(2, E::On { key: 60, velocity: 1. }), (3, E::Off { key: 60, velocity: 0. })],
            &mut out,
        );
        let count = if one_shot { frames } else { 1 };
        for (i, pair) in out.as_chunks::<2>().0.iter().enumerate() {
            assert_eq!(pair, &[if (2..2 + count).contains(&i) { 0.25 } else { 0. }; 2], "frame {i}");
        }
    }
}
#[test]
fn parameter_bindings_are_serialized_checked_and_applied_to_private_instances() {
    let mut child = DeviceGraph::new("org.test.child", "Child", DeviceRole::Effect);
    child.parameters.push(Parameter::new("value", "Value", (0., 1.), 0.));
    child.nodes = vec![
        N::audio_input("in"),
        N::new("param", K::Parameter { parameter: "value".into() }),
        N::new("sum", K::Add),
        N::audio_output("out"),
    ];
    child.connections =
        vec![C::ports("in", "out", "sum", "a"), C::ports("param", "out", "sum", "b"), C::new("sum", "out")];
    let mut g = DeviceGraph::new("org.test.binding", "Binding", DeviceRole::Effect);
    g.parameters.push(Parameter::new("parent", "Parent", (0., 1.), 0.5));
    g.nodes = vec![
        N::audio_input("in"),
        N::new("component", K::Component { name: "Control".into(), version: 1, graph: child.to_sd().unwrap() })
            .bind("parent", "value"),
        N::audio_output("out"),
    ];
    g.connections = vec![C::new("in", "component"), C::new("component", "out")];
    let g = DeviceGraph::from_sd(&g.to_sd().unwrap()).unwrap();
    let mut p = g.compile_at(48000.).unwrap();
    let mut out = [0.; 2];
    p.process(&mut out);
    assert_eq!(out, [0.5; 2]);
    p.set_parameter(0, 0.25);
    let mut out = [0.; 4096];
    p.process(&mut out);
    assert_eq!(&out[4094..], &[0.25; 2]);
    let mut bad = g.clone();
    bad.nodes[1].bindings[0].target = "missing".into();
    assert!(bad.compile_at(48000.).is_err());
    let mut bad = g.clone();
    bad.nodes[1].bindings[0].parameter = "missing".into();
    assert!(bad.validate().is_err());
    let mut bad = g.clone();
    let duplicate = bad.nodes[1].bindings[0].clone();
    bad.nodes[1].bindings.push(duplicate);
    assert!(bad.validate().is_err());
}
#[test]
fn compound_budgets_reject_hidden_excess_before_rendering() {
    assert!(suhina_device_sdk::granular_template("sample", 0, 8, 1).is_err());
    let g = source(K::GranularPlayer { sample: "sample".into(), voices: 32, grains: 128, seed: 1 });
    assert!(g.compile_with_samples_at(48000., &samples(4)).is_err());
    let g = source(K::TimeStretchPlayer { sample: "missing".into() });
    assert!(g.compile_with_samples_at(48000., &samples(4)).is_err());
}

#[test]
fn resident_high_level_players_do_not_allocate_or_free_in_callbacks() {
    for kind in [
        K::SamplerAsset { sample: "sample".into() },
        K::TimeStretchPlayer { sample: "sample".into() },
        K::GranularPlayer { sample: "sample".into(), voices: 2, grains: 8, seed: 1 },
    ] {
        let mut p = source(kind).compile_with_samples_at(48000., &samples(12000)).unwrap();
        let mut output = [0.; 258];
        let allocations = allocation_counter::measure(|| {
            for _ in 0..8 {
                p.process_instrument_context(
                    &ProcessContext::default(),
                    &[(3, E::On { key: 60, velocity: 1. }), (90, E::Off { key: 60, velocity: 0. })],
                    &mut output,
                );
            }
            p.reset();
        });
        assert_eq!(allocations.count_total, 0, "{allocations:?}");
        assert_eq!(allocations.count_current, 0, "{allocations:?}");
    }
}
