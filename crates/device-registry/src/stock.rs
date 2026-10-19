// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Build the distributable definitions without replacing native DSP with approximations.
use suhina_device_api::{ParamKind, Role, Scale, Unit};
use suhina_device_sdk::{
    Connection, DeviceGraph, DeviceRole, DeviceSdkError, Node, NodeKind, Parameter, ParameterControl,
};

/// Whether `id` gets a stock `.sd` wrapper: the built-in effects and instruments. A MIDI effect
/// (#678) has none yet, as the native node runs audio devices; the browser lists it from the
/// registry.
pub fn bundled(id: &str) -> bool {
    crate::info(id).is_some_and(|info| info.role != Role::MidiEffect)
}

pub fn definition(id: &str) -> Result<DeviceGraph, DeviceSdkError> {
    let info = crate::info(id).ok_or_else(|| DeviceSdkError(format!("unregistered stock device {id}")))?;
    let instrument = info.role == Role::Instrument;
    let mut graph =
        DeviceGraph::new(info.id, info.name, if instrument { DeviceRole::Instrument } else { DeviceRole::Effect });
    graph.parameters = info
        .params
        .iter()
        .map(|p| {
            let mut parameter = Parameter::new(p.id, p.name, (p.min, p.max), p.default);
            parameter.smoothing_ms = 0.;
            parameter.control = match p.kind {
                ParamKind::Continuous(Scale::Logarithmic) => ParameterControl::LogKnob,
                ParamKind::Continuous(_) => ParameterControl::Knob,
                ParamKind::Integer => ParameterControl::Integer,
                ParamKind::Toggle => ParameterControl::Toggle,
                ParamKind::Choice(options) => {
                    parameter.ui.options = options.iter().map(|s| (*s).into()).collect();
                    ParameterControl::Choice
                }
            };
            parameter.unit = match p.unit {
                Unit::None => "",
                Unit::Decibels => "dB",
                Unit::Hertz => "Hz",
                Unit::Seconds => "s",
                Unit::Percent => "%",
                Unit::Balance => "balance",
                Unit::Octaves => "oct",
                Unit::Semitones => "st",
                Unit::ScaleDegrees => "degrees",
                Unit::Key => "key",
            }
            .into();
            parameter
        })
        .collect();
    let mut kernel = Node::new("device", NodeKind::NativeDevice { device: info.id.into() });
    kernel.visible = false;
    graph.nodes = vec![kernel, Node::audio_output("out")];
    graph.connections = vec![Connection::new("device", "out")];
    if !instrument {
        graph.nodes.insert(0, Node::audio_input("in"));
        graph.connections.push(Connection::new("in", "device"));
    }
    for node in &mut graph.nodes {
        node.visible = false;
    }
    if id == "sampler" {
        graph.device_version = 12;
        for from_version in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] {
            graph.migrations.push(suhina_device_sdk::DeviceMigration { from_version, parameter_ids: vec![] });
        }
    }
    graph.ui = crate::panel(id).map(|(panel, _)| panel);
    graph.validate()?;
    let _prepared = graph.prepare_native_at(48000., &crate::create)?;
    Ok(graph)
}
