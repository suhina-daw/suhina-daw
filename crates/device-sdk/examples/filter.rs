// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build the filter effect as a self-contained .sd file.
use suhina_device_sdk::{
    DeviceGraph, DeviceRole, FilterMode, NodeKind as K, Parameter, ParameterControl, author::GraphBuilder,
};

pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = GraphBuilder::new("org.example.filter", "filter", DeviceRole::Effect);
    let input = g.node("in", K::AudioInput)?;
    let output = g.node("out", K::AudioOutput)?;

    let cutoff = g.parameter_signal(
        Parameter::new("cutoff", "cutoff", (20.0, 20_000.0), 1000.0).unit("Hz").control(ParameterControl::LogKnob),
    )?;
    let filter =
        g.op("filter", K::Filter { mode: FilterMode::Lowpass }, &[("in", input.out()), ("frequency", cutoff.out())])?;
    g.connect(filter.out(), output.input("in"))?;

    finish(g.finish()?)
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    save(build()?, "filter")
}

fn style_parameter(parameter: &mut suhina_device_sdk::Parameter) {
    parameter.ui.font_size = 11.0;
    parameter.ui.label_position = suhina_device_sdk::LabelPosition::Top;
    let knob = matches!(
        parameter.control,
        suhina_device_sdk::ParameterControl::Knob | suhina_device_sdk::ParameterControl::LogKnob
    );
    parameter.ui.shape =
        if knob { suhina_device_sdk::ControlShape::Rounded } else { suhina_device_sdk::ControlShape::Square };
    parameter.ui.corner_radius = if knob { 5.0 } else { 0.0 };
}

fn finish(mut g: suhina_device_sdk::DeviceGraph) -> Result<suhina_device_sdk::DeviceGraph, Box<dyn std::error::Error>> {
    let mut visible_parameter = 0usize;
    // The examples teach their controls; their implementation graph does not need panel space.
    for node in &mut g.nodes {
        node.visible = false;
    }
    for p in &mut g.parameters {
        style_parameter(p);
        p.ui.tooltip = format!("{} ({})", p.name, p.unit);
        if !g.ui.as_ref().is_some_and(|ui| ui.hidden_parameters.contains(&p.id)) {
            p.ui.position = Some([(visible_parameter % 2) as f32 * 104.0, (visible_parameter / 2) as f32 * 84.0]);
            p.ui.size = Some([100.0, 80.0]);
            visible_parameter += 1;
        }
    }
    g.validate()?;
    Ok(g)
}

#[cfg(not(test))]
fn save(graph: suhina_device_sdk::DeviceGraph, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(format!("target/sdk-examples/{name}.sd")));
    graph.write_file(&path)?;
    Ok(())
}
