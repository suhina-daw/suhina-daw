// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::duplicate_mod)]
use image::ImageEncoder as _;
use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ui::*};
#[path = "../examples/custom_panel.rs"]
mod name_demo;
#[path = "../examples/orbit_filter.rs"]
mod panel;

#[test]
fn filename_demo_and_custom_ui_have_distinct_names_and_ids() {
    let graph = name_demo::build().unwrap();
    let orbit = panel::build().unwrap();
    assert_eq!(graph.name, "Velvet Volume");
    assert_ne!(graph.id, orbit.id);
    assert!(graph.ui.as_ref().unwrap().controls.is_empty());
    assert_eq!(DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap(), graph);
}

#[test]
fn example_panel_roundtrips_and_processes_audio() {
    let graph = panel::build().unwrap();
    assert_eq!(graph.name, "Orbit Filter");
    let canvas = graph.ui.as_ref().unwrap();
    assert!(!canvas.assets.is_empty());
    assert!(canvas.controls.iter().any(|control| control.id == "orbit"
        && control.x.as_ref().is_some_and(|binding| binding.parameter == "cutoff")
        && control.y.as_ref().is_some_and(|binding| binding.parameter == "balance")));
    assert!(canvas.controls.iter().any(|control| control.id == "output"));
    assert!(graph.parameters.iter().all(|parameter| canvas.hidden_parameters.contains(&parameter.id)));
    for parameter in &graph.parameters {
        if matches!(
            parameter.control,
            suhina_device_sdk::ParameterControl::Knob | suhina_device_sdk::ParameterControl::LogKnob
        ) {
            assert_eq!(parameter.ui.shape, suhina_device_sdk::ControlShape::Rounded, "{}", parameter.id);
        }
    }
    let restored = DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap();
    assert_eq!(restored, graph);
    let mut effect = restored.compile().unwrap();
    effect.process(&mut [0.1, 0.2]);
}
#[test]
fn custom_ui_rejects_invalid_resources_and_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let graph = test_ui_graph()?;
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().assets[0].data = "not-base64".into();
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().controls[0].x.as_mut().unwrap().parameter = "missing".into();
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().controls[0].bounds[2] = f32::INFINITY;
    assert!(bad.validate().is_err());
    let mut bad = graph.clone();
    bad.ui.as_mut().unwrap().controls[0].y = Some(UiBinding::new("level"));
    assert!(bad.validate().is_err());
    let mut bad = graph;
    let mut value = UiValue::from(1.0);
    for _ in 0..20 {
        value = value.sin();
    }
    bad.ui.as_mut().unwrap().layers.push(Draw::Circle {
        center: [value, 0.0.into()],
        radius: 1.0.into(),
        color: [255; 4],
    });
    assert!(bad.validate().is_err());
    Ok(())
}

fn test_ui_graph() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut graph = DeviceGraph::new("org.example.test-ui", "Test UI", DeviceRole::Effect);
    graph.parameters.extend([
        Parameter::new("level", "Level", (0.0, 1.0), 0.5),
        Parameter::new("balance", "Balance", (-1.0, 1.0), 0.0),
    ]);
    graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "level"), Node::audio_output("out")];
    graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(&[0, 0, 0, 0], 1, 1, image::ExtendedColorType::Rgba8)?;
    graph.ui = Some(DeviceUi {
        size: [320.0, 180.0],
        assets: vec![ImageAsset::from_bytes("pixel", &png)?],
        controls: vec![CustomControl {
            active_when: None,
            widget: None,
            id: "level-control".into(),
            name: "Level".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 100.0, 24.0],
            visible_when: None,
            gesture: UiGesture::Position,
            x: Some(UiBinding::new("level")),
            y: Some(UiBinding::new("balance")),
            drawing: Vec::new(),
        }],
        ..DeviceUi::default()
    });
    Ok(graph)
}
#[test]
fn jpeg_assets_decode_without_external_paths() {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode(&[10, 20, 30], 1, 1, image::ExtendedColorType::Rgb8)
        .unwrap();
    let asset = ImageAsset::from_bytes("photo", &bytes).unwrap();
    assert_eq!(asset.decode_rgba().unwrap().1, [1, 1]);
    assert!(ImageAsset::from_bytes("bad", b"not an image").is_err());
    assert!(ImageAsset::from_bytes("huge", &vec![0; MAX_ASSET_BYTES + 1]).is_err());
}

#[test]
fn canvas_and_viewport_sizes_roundtrip_and_validate() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = test_ui_graph()?;
    let defaults: DeviceUi = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults.size, [640.0, 240.0]);
    assert_eq!(defaults.viewport_size, None);
    let ui = graph.ui.as_mut().unwrap();
    ui.size = [800.0, 480.0];
    ui.viewport_size = Some([320.0, 180.0]);
    assert_eq!(DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap(), graph);
    for size in [[0.0, 240.0], [320.0, 8193.0], [f32::NAN, 240.0], [320.0, f32::INFINITY]] {
        graph.ui.as_mut().unwrap().viewport_size = Some(size);
        assert!(graph.validate().is_err());
    }
    Ok(())
}
