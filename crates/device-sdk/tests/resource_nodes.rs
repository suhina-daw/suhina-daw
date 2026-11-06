// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use suhina_device_sdk::sample_resource::{PreparedSamples, SampleAsset};
use suhina_device_sdk::{Connection as C, DeviceGraph, DeviceRole, Node as N, NodeKind as K, SampleInfoField};
fn bindings(value: Option<f32>) -> PreparedSamples {
    let mut samples = PreparedSamples::default();
    let pcm = value.map_or_else(|| vec![0., 1., 0., -1.], |v| vec![v; 4]);
    let asset = SampleAsset::new(Arc::new(pcm), 48000, 1).unwrap().with_tempo(120.).unwrap();
    samples.insert("sample", Arc::new(asset)).unwrap();
    samples
}
fn read_graph(sliced: bool) -> DeviceGraph {
    let mut graph = DeviceGraph::new("org.test.resources", "Resources", DeviceRole::Effect);
    graph.nodes = vec![
        N::audio_input("in"),
        N::audio_output("out"),
        N::new("asset", K::SampleAsset { sample: "sample".into() }),
        N::new("read", K::SampleRead),
        N::new("position", K::Constant { value: 0.5 }),
        N::new("sum", K::Add),
    ];
    graph.connections = vec![
        C::ports("asset", "out", "read", "asset"),
        C::ports("position", "out", "read", "position"),
        C::ports("in", "out", "sum", "a"),
        C::ports("read", "out", "sum", "b"),
        C::new("sum", "out"),
    ];
    if sliced {
        graph.nodes.extend([
            N::new("map", K::SliceMap { sample: "sample".into(), boundaries: vec![0, 2, 4] }),
            N::new("region", K::SliceSelect),
            N::new("index", K::Constant { value: 1. }),
        ]);
        graph
            .connections
            .extend([C::ports("map", "out", "region", "map"), C::ports("index", "out", "region", "index")]);
    } else {
        graph.nodes.extend([
            N::new("region", K::SampleRegion),
            N::new("frames", K::SampleInfo { field: SampleInfoField::Frames }),
        ]);
        graph.connections.extend([
            C::ports("asset", "out", "region", "asset"),
            C::ports("asset", "out", "frames", "asset"),
            C::ports("frames", "out", "region", "end"),
        ]);
    }
    graph.connections.push(C::ports("region", "out", "read", "region"));
    graph
}
#[test]
fn regions_slices_and_reads_roundtrip_and_ignore_block_size() {
    for sliced in [false, true] {
        let graph = read_graph(sliced);
        let graph = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
        for size in [1, 7, 64, 129] {
            let mut prepared = graph.compile_with_samples_at(48000., &bindings(None)).unwrap();
            let mut output = vec![0.25; 514];
            for block in output.chunks_mut(size * 2) {
                prepared.process(block);
            }
            let expected = if sliced { -0.25 } else { 0.75 };
            assert!(output.iter().all(|&v| v == expected));
            prepared.reset();
            let mut output = [0.; 2];
            prepared.process(&mut output);
            assert_eq!(output, [if sliced { -0.5 } else { 0.5 }; 2]);
        }
    }
}
#[test]
fn prepared_graph_keeps_old_resource_and_missing_or_wrong_types_fail() {
    let graph = read_graph(false);
    let mut samples = bindings(None);
    let mut old = graph.compile_with_samples_at(48000., &samples).unwrap();
    let replacement = SampleAsset::new(Arc::new(vec![2.; 4]), 48000, 1).unwrap();
    samples.insert("sample", Arc::new(replacement)).unwrap();
    let mut new = graph.compile_with_samples_at(48000., &samples).unwrap();
    let mut a = [0.; 2];
    let mut b = a;
    old.process(&mut a);
    new.process(&mut b);
    assert_eq!(a, [0.5; 2]);
    assert_eq!(b, [2.; 2]);
    assert!(graph.compile_with_samples_at(48000., &PreparedSamples::default()).is_err());
    assert!(graph.compile_at(48000.).is_err());
    let mut wrong = graph.clone();
    wrong.connections.iter_mut().find(|e| e.to == "read" && e.to_port == "asset").unwrap().from = "in".into();
    assert!(wrong.validate().is_err());
}
#[test]
fn all_metadata_modes_and_tempo_have_defined_units() {
    for (kind, expected) in [
        (K::SampleInfo { field: SampleInfoField::Frames }, 4.),
        (K::SampleInfo { field: SampleInfoField::Duration }, 4. / 48000.),
        (K::SampleInfo { field: SampleInfoField::SampleRate }, 48000.),
        (K::SampleInfo { field: SampleInfoField::Channels }, 1.),
        (K::SampleTempo { beats: false }, 120.),
        (K::SampleTempo { beats: true }, 8. / 48000.),
    ] {
        let mut graph = DeviceGraph::new("org.test.info", "Info", DeviceRole::Effect);
        graph.nodes = vec![
            N::audio_input("in"),
            N::new("asset", K::SampleAsset { sample: "sample".into() }),
            N::new("info", kind),
            N::new("sum", K::Add),
            N::audio_output("out"),
        ];
        graph.connections = vec![
            C::ports("asset", "out", "info", "asset"),
            C::ports("in", "out", "sum", "a"),
            C::ports("info", "out", "sum", "b"),
            C::new("sum", "out"),
        ];
        let mut prepared = graph.compile_with_samples_at(48000., &bindings(None)).unwrap();
        let mut output = [0.; 2];
        prepared.process(&mut output);
        assert!((output[0] - expected).abs() <= 1e-6);
    }
}

#[test]
fn crossfade_heads_are_periodic_and_invalid_regions_clear_auxiliary_outputs() {
    let mut g = DeviceGraph::new("org.test.crossfade", "Crossfade", DeviceRole::Effect);
    g.nodes = vec![
        N::audio_input("in"),
        N::audio_output("out"),
        N::new("asset", K::SampleAsset { sample: "sample".into() }),
        N::new("region", K::SampleRegion),
        N::new("position", K::PlaybackPosition),
        N::new("fade", K::Constant { value: 1. }),
        N::new("loop", K::LoopCrossfade),
        N::new("sum", K::Add),
    ];
    g.connections = vec![
        C::ports("asset", "out", "region", "asset"),
        C::ports("asset", "out", "loop", "asset"),
        C::ports("region", "out", "loop", "region"),
        C::ports("position", "out", "loop", "position"),
        C::ports("fade", "out", "loop", "fade"),
        C::ports("loop", "primary", "sum", "a"),
        C::ports("in", "out", "sum", "b"),
        C::new("sum", "out"),
    ];
    let mut p = g.compile_with_samples_at(48000., &bindings(None)).unwrap();
    let mut output = [0.; 18];
    p.process(&mut output);
    assert_eq!(output, [1., 1., 2., 2., 3., 3., 1., 1., 2., 2., 3., 3., 1., 1., 2., 2., 3., 3.]);
    let start = N::new("start", K::Constant { value: 4. });
    g.nodes.push(start);
    g.connections.push(C::ports("start", "out", "region", "start"));
    let mut p = g.compile_with_samples_at(48000., &bindings(None)).unwrap();
    let mut out = [0.; 16];
    p.process(&mut out);
    assert_eq!(out, [0.; 16]);
}

#[test]
fn catalog_exposes_the_same_port_types_as_validation() {
    for info in suhina_device_sdk::NODE_CATALOG {
        let json = serde_json::to_value(info).unwrap();
        for &port in info.outputs {
            assert_eq!(json["output_types"][port].as_str(), info.output_type(port));
        }
        assert_eq!(info.output_type("not_a_port"), None);
    }
    assert_eq!(K::SampleAsset { sample: "sample".into() }.info().output_type("out"), Some("sample"));
    assert_eq!(K::SliceSelect.info().output_type("out"), Some("region"));
    assert_eq!(K::Sustain.info().output_type("out"), Some("midi"));
}
