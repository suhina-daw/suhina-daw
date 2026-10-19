// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stereo delay with an embedded control panel.
use suhina_device_sdk::{
    Connection, DeviceGraph, DeviceRole, FilterMode, Node as N, NodeKind as K, ParameterControl as C, Waveform,
};
fn op(g: &mut DeviceGraph, id: &str, kind: K, inputs: &[(&str, &str)]) {
    g.nodes.push(N::new(id, kind));
    for (port, source) in inputs {
        wire(g, source, id, port);
    }
}
fn binary(g: &mut DeviceGraph, id: &str, kind: K, a: &str, b: &str) {
    op(g, id, kind, &[("a", a), ("b", b)]);
}
pub fn build() -> Result<DeviceGraph, Box<dyn std::error::Error>> {
    let mut g = DeviceGraph::new("org.example.delay", "Delay", DeviceRole::Effect);
    g.nodes = vec![N::audio_input("in"), N::audio_output("out")];
    for (id, lo, hi, value, unit, log) in [
        ("feedback", 0., 95., 50., "%", false),
        ("wet", 0., 100., 50., "%", false),
        ("time_left", 1., 4000., 250., "ms", true),
        ("time_right", 1., 4000., 250., "ms", true),
        ("sync_left", 0., 1., 1., "", false),
        ("sync_right", 0., 1., 1., "", false),
        ("division_left", 1., 16., 3., "", false),
        ("division_right", 1., 16., 3., "", false),
        ("offset_left", -33.3, 33.3, 0., "%", false),
        ("offset_right", -33.3, 33.3, 0., "%", false),
        ("link", 0., 1., 1., "", false),
        ("filter_on", 0., 1., 1., "", false),
        ("cutoff", 50., 18000., 1000., "Hz", true),
        ("width", 0.5, 9., 8., "", false),
        ("ping_pong", 0., 1., 0., "", false),
        ("freeze", 0., 1., 0., "", false),
        ("mode", 0., 2., 0., "", false),
        ("mod_delay", 0., 100., 0., "%", false),
        ("mod_filter", 0., 100., 0., "%", false),
        ("mod_rate", 0.01, 20., 0.5, "Hz", true),
        ("mod_shape", 0., 6., 0., "", false),
        ("mod_rate_mode", 0., 5., 0., "", false),
        ("mod_time", 1., 20000., 2000., "ms", true),
        ("mod_division", 1., 64., 4., "", false),
        ("mod_morph", 0., 100., 0., "%", false),
    ] {
        parameter(&mut g, id, (lo, hi), value, unit, log);
        if let Some(p) = g.parameters.last_mut() {
            p.smoothing_ms = 0.;
            p.control = if matches!(id, "sync_left" | "sync_right" | "link" | "filter_on" | "ping_pong" | "freeze") {
                C::Toggle
            } else if id.starts_with("division")
                || matches!(id, "mode" | "mod_shape" | "mod_division" | "mod_rate_mode")
            {
                C::Integer
            } else if log {
                C::LogKnob
            } else {
                C::Knob
            };
        }
    }
    let mut expanded = suhina_device_sdk::Parameter::new("lfo_expanded", "LFO details", (0., 1.), 1.);
    expanded.control = C::Toggle;
    expanded.smoothing_ms = 0.;
    g.parameters.push(expanded);
    for (id, labels) in [
        ("mod_shape", vec!["Sine", "Triangle", "Ramp Up", "Ramp Down", "Square", "S&H", "Wander"]),
        ("mod_rate_mode", vec!["Hz", "Time", "Synced", "Triplet", "Dotted", "16th steps"]),
    ] {
        if let Some(p) = g.parameters.iter_mut().find(|p| p.id == id) {
            p.control = C::Choice;
            p.ui.options = labels.into_iter().map(str::to_string).collect();
        }
    }
    for side in ["left", "right"] {
        let peer = if side == "left" { "right" } else { "left" };
        for kind in ["sync", "division", "offset", "time"] {
            if let Some(p) = g.parameters.iter_mut().find(|p| p.id == format!("{kind}_{side}")) {
                p.ui.link = Some(suhina_device_sdk::ParameterLink {
                    enabled_by: "link".into(),
                    peer: format!("{kind}_{peer}"),
                });
            }
        }
    }
    for (id, value) in [
        ("zero", 0.),
        ("one", 1.),
        ("two", 2.),
        ("four", 4.),
        ("sixty", 60.),
        ("percent", 0.01),
        ("millisecond", 0.001),
    ] {
        g.nodes.push(N::new(id, K::Constant { value }));
    }
    op(&mut g, "tempo", K::Tempo, &[]);
    binary(&mut g, "beat_time", K::Divide, "sixty", "tempo");
    binary(&mut g, "sixteenth", K::Divide, "beat_time", "four");
    for side in ["left", "right"] {
        binary(&mut g, &format!("seconds_{side}"), K::Multiply, &format!("time_{side}"), "millisecond");
        binary(&mut g, &format!("sync_time_{side}"), K::Multiply, "sixteenth", &format!("division_{side}"));
        op(
            &mut g,
            &format!("chosen_{side}"),
            K::Select,
            &[
                ("a", &format!("seconds_{side}")),
                ("b", &format!("sync_time_{side}")),
                ("condition", &format!("sync_{side}")),
            ],
        );
        binary(&mut g, &format!("offset_fraction_{side}"), K::Multiply, &format!("offset_{side}"), "percent");
        binary(
            &mut g,
            &format!("effective_offset_{side}"),
            K::Multiply,
            &format!("offset_fraction_{side}"),
            &format!("sync_{side}"),
        );
        binary(&mut g, &format!("offset_scale_{side}"), K::Add, "one", &format!("effective_offset_{side}"));
        binary(
            &mut g,
            &format!("base_{side}"),
            K::Multiply,
            &format!("chosen_{side}"),
            &format!("offset_scale_{side}"),
        );
    }
    op(&mut g, "right_time", K::Select, &[("a", "base_right"), ("b", "base_left"), ("condition", "link")]);
    op(&mut g, "stereo_time", K::Stereo, &[("left", "base_left"), ("right", "right_time")]);
    op(
        &mut g,
        "mod_frequency",
        K::LfoRate,
        &[
            ("rate", "mod_rate"),
            ("time", "mod_time"),
            ("division", "mod_division"),
            ("mode", "mod_rate_mode"),
            ("tempo", "tempo"),
        ],
    );
    let shapes =
        [Waveform::Sine, Waveform::Triangle, Waveform::SawUp, Waveform::SawDown, Waveform::Square, Waveform::Random];
    for (i, shape) in shapes.into_iter().enumerate() {
        op(&mut g, &format!("wave{i}"), K::Oscillator { shape }, &[("frequency", "mod_frequency")]);
        if i > 0 {
            g.nodes.push(N::new(format!("shape_threshold{i}"), K::Constant { value: (i - 1) as f64 }));
            binary(&mut g, &format!("shape_active{i}"), K::Greater, "mod_shape", &format!("shape_threshold{i}"));
            let prev = if i == 1 { "wave0".into() } else { format!("shape_mix{}", i - 1) };
            op(
                &mut g,
                &format!("shape_mix{i}"),
                K::Select,
                &[("a", &prev), ("b", &format!("wave{i}")), ("condition", &format!("shape_active{i}"))],
            );
        }
    }
    g.nodes.push(N::new("wander_time", K::Constant { value: 0.2 }));
    g.nodes.push(N::new("wander_threshold", K::Constant { value: 5.0 }));
    op(&mut g, "wander", K::Smooth, &[("in", "wave5"), ("time", "wander_time")]);
    binary(&mut g, "wander_active", K::Greater, "mod_shape", "wander_threshold");
    op(&mut g, "selected_wave", K::Select, &[("a", "shape_mix5"), ("b", "wander"), ("condition", "wander_active")]);
    binary(&mut g, "morph_amount", K::Multiply, "mod_morph", "percent");
    op(&mut g, "lfo", K::Mix, &[("a", "selected_wave"), ("b", "wave0"), ("amount", "morph_amount")]);
    binary(&mut g, "delay_amount", K::Multiply, "mod_delay", "percent");
    binary(&mut g, "delay_motion", K::Multiply, "lfo", "delay_amount");
    binary(&mut g, "delay_scale", K::Add, "one", "delay_motion");
    binary(&mut g, "delay_time", K::Multiply, "stereo_time", "delay_scale");
    binary(&mut g, "filter_amount", K::Multiply, "mod_filter", "percent");
    binary(&mut g, "filter_motion", K::Multiply, "lfo", "filter_amount");
    binary(&mut g, "filter_octaves", K::Multiply, "filter_motion", "four");
    binary(&mut g, "filter_scale", K::Power, "two", "filter_octaves");
    binary(&mut g, "filter_center", K::Multiply, "cutoff", "filter_scale");
    binary(&mut g, "half_width", K::Divide, "width", "two");
    binary(&mut g, "band_ratio", K::Power, "two", "half_width");
    binary(&mut g, "low_edge", K::Divide, "filter_center", "band_ratio");
    binary(&mut g, "high_edge", K::Multiply, "filter_center", "band_ratio");
    binary(&mut g, "input_amount", K::Subtract, "one", "freeze");
    op(&mut g, "ping_input", K::Stereo, &[("left", "in"), ("right", "zero")]);
    op(&mut g, "source", K::Select, &[("a", "in"), ("b", "ping_input"), ("condition", "ping_pong")]);
    binary(&mut g, "feed", K::Multiply, "source", "input_amount");
    op(&mut g, "sum", K::Add, &[("a", "feed"), ("b", "feedback_audio")]);
    op(&mut g, "delay", K::Delay { max_seconds: 4. }, &[("in", "sum"), ("time", "delay_time"), ("mode", "mode")]);
    op(&mut g, "highpass", K::Filter { mode: FilterMode::Highpass }, &[("in", "delay"), ("frequency", "low_edge")]);
    op(&mut g, "lowpass", K::Filter { mode: FilterMode::Lowpass }, &[("in", "highpass"), ("frequency", "high_edge")]);
    op(&mut g, "filtered", K::Mix, &[("a", "delay"), ("b", "lowpass"), ("amount", "filter_on")]);
    g.nodes.push(N::new("swapped", K::Stereo));
    g.connections.push(Connection::ports("filtered", "right", "swapped", "left"));
    g.connections.push(Connection::ports("filtered", "left", "swapped", "right"));
    op(&mut g, "return_audio", K::Select, &[("a", "filtered"), ("b", "swapped"), ("condition", "ping_pong")]);
    binary(&mut g, "feedback_fraction", K::Multiply, "feedback", "percent");
    binary(&mut g, "wet_fraction", K::Multiply, "wet", "percent");
    op(&mut g, "feedback_amount", K::Select, &[("a", "feedback_fraction"), ("b", "one"), ("condition", "freeze")]);
    binary(&mut g, "feedback_audio", K::Multiply, "return_audio", "feedback_amount");
    op(&mut g, "mix", K::Mix, &[("a", "in"), ("b", "filtered"), ("amount", "wet_fraction")]);
    wire(&mut g, "mix", "out", "in");
    g.ui = Some(build_panel(&g.parameters));
    for node in &mut g.nodes {
        node.visible = false;
    }
    let mut g = finish(g)?;
    for node in &mut g.nodes {
        node.visible = false;
    }
    Ok(g)
}
#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    save(build()?, "delay")
}

use suhina_device_sdk::{Parameter, ui::*};
// Widget text is resolved against the host palette at render time.
const TEXT: [u8; 4] = [236, 232, 225, 255];
fn equal(id: &str, v: u16) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter(id, false), b: (v as f32).into() }))
}
fn button(ui: &mut DeviceUi, id: &str, value: Option<u16>, label: &str, bounds: [f32; 4]) {
    ui.controls.push(CustomControl {
        widget: Some(Widget::Button { label: label.into(), off_label: None, indicator: false }),
        id: format!("{id}_{}", value.unwrap_or(99)),
        name: label.into(),
        tooltip: format!("{id}: {label}"),
        bounds,
        visible_when: None,
        gesture: value.map_or(UiGesture::Toggle, UiGesture::Set),
        x: Some(UiBinding::new(id)),
        y: None,
        drawing: vec![],
    });
}
fn when(ui: &mut DeviceUi, id: &str, value: u16) {
    if let Some(c) = ui.controls.last_mut() {
        c.visible_when = Some(equal(id, value));
    }
}
fn number(ui: &mut DeviceUi, id: &str, bounds: [f32; 4], decimals: usize, log: bool, color: [u8; 4], center: bool) {
    ui.controls.push(CustomControl {
        widget: Some(Widget::Number {
            decimals,
            format: if id == "cutoff" { TextFormat::Frequency } else { TextFormat::Number },
            color,
            center,
            bordered: true,
            prefix: String::new(),
            suffix: String::new(),
        }),
        id: id.into(),
        name: id.replace('_', " "),
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds,
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding { parameter: id.into(), logarithmic: log, drag_scale: 1. }),
        drawing: vec![],
    });
}
fn dial(ui: &mut DeviceUi, id: &str, label: &str, bounds: [f32; 4], log: bool) {
    ui.controls.push(CustomControl {
        widget: Some(Widget::Dial { label: label.into(), decimals: 0, format: TextFormat::Number }),
        id: id.into(),
        name: if label.is_empty() {
            if id == "feedback" { "Feedback".into() } else { "Mix".into() }
        } else {
            label.replace('\n', " ")
        },
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds,
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding { parameter: id.into(), logarithmic: log, drag_scale: 1. }),
        drawing: vec![],
    });
}
fn caption(ui: &mut DeviceUi, id: &str, label: &str, bounds: [f32; 4]) {
    ui.controls.push(CustomControl {
        widget: Some(Widget::Label { label: label.into() }),
        id: id.into(),
        name: label.into(),
        tooltip: String::new(),
        bounds,
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: None,
        drawing: vec![],
    });
}
fn build_panel(parameters: &[Parameter]) -> DeviceUi {
    let mut ui = DeviceUi {
        size: [944., 168.],
        viewport_size: Some([944., 168.]),
        hidden_parameters: parameters.iter().map(|p| p.id.clone()).collect(),
        ..DeviceUi::default()
    };
    for (label, x, width) in
        [("TIME", 14., 80.), ("FEEDBACK", 289., 74.), ("MIX", 872., 40.), ("TONE", 384., 80.), ("MOTION", 579., 90.)]
    {
        caption(&mut ui, label, label, [x, 6., width, 16.]);
    }
    for x in [275., 367., 563., 851.] {
        ui.controls.push(CustomControl {
            widget: Some(Widget::Separator),
            id: format!("divider_{x}"),
            name: "Section divider".into(),
            tooltip: String::new(),
            bounds: [x - 4., 6., 8., 160.],
            visible_when: None,
            gesture: UiGesture::Drag,
            x: None,
            y: None,
            drawing: vec![],
        });
    }
    for (side, x) in [("left", 14.), ("right", 164.)] {
        let title = if side == "left" { "Left" } else { "Right" };
        caption(&mut ui, &format!("{side}_label"), title, [x, 28., 100., 16.]);
        let sync = format!("sync_{side}");
        button(&mut ui, &sync, None, "Beats", [x, 48., 42., 24.]);
        if let Some(c) = ui.controls.last_mut() {
            c.name = format!("{title} timing mode");
            c.widget = Some(Widget::Button { label: "Beats".into(), off_label: Some("ms".into()), indicator: false });
            c.tooltip = "Choose beat-synchronized notes or milliseconds".into();
        }
        let choices = (1..=16)
            .map(|steps| {
                let divisor = (1..=16).rev().find(|d| steps % d == 0 && 16 % d == 0).unwrap_or(1);
                (steps, format!("{}/{}", steps / divisor, 16 / divisor))
            })
            .collect();
        ui.controls.push(CustomControl {
            widget: Some(Widget::ValueMenu { choices }),
            id: format!("division_{side}"),
            name: format!("{title} delay"),
            tooltip: "Choose the delay length as a fraction of a whole note".into(),
            bounds: [x + 50., 48., 50., 24.],
            visible_when: Some(equal(&sync, 1)),
            gesture: UiGesture::Menu,
            x: Some(UiBinding::new(format!("division_{side}"))),
            y: None,
            drawing: vec![],
        });
        number(&mut ui, &format!("time_{side}"), [x + 50., 48., 50., 24.], 0, true, TEXT, false);
        when(&mut ui, &sync, 0);
        caption(&mut ui, &format!("{side}_offset_caption"), "Offset", [x, 80., 100., 16.]);
        when(&mut ui, &sync, 1);
        number(&mut ui, &format!("offset_{side}"), [x, 98., 100., 24.], 1, false, TEXT, false);
        when(&mut ui, &sync, 1);
    }
    button(&mut ui, "link", None, "↔", [122., 48., 34., 24.]);
    if let Some(c) = ui.controls.last_mut() {
        c.name = "Link channels".into();
        c.tooltip = "Link both channels; enabling copies the left timing to the right".into();
    }
    button(&mut ui, "ping_pong", None, "Ping Pong", [864., 140., 72., 22.]);
    button(&mut ui, "freeze", None, "Freeze", [289., 140., 64., 22.]);
    for (i, label) in ["Repitch", "Fade", "Jump"].into_iter().enumerate() {
        button(&mut ui, "mode", Some(i as u16), label, [14. + i as f32 * 86., 140., 78., 22.]);
    }
    dial(&mut ui, "feedback", "", [289., 48., 64., 86.], false);
    dial(&mut ui, "wet", "", [872., 48., 56., 86.], false);
    button(&mut ui, "filter_on", None, "Filter", [384.0, 32., 51., 26.]);
    number(&mut ui, "cutoff", [444.0, 36., 56., 18.], 0, true, TEXT, false);
    number(&mut ui, "width", [504.0, 36., 46., 18.], 2, false, TEXT, false);
    if let Some(c) = ui.controls.last_mut()
        && let Some(Widget::Number { prefix, .. }) = &mut c.widget
    {
        *prefix = "Oct: ".into();
    }
    ui.controls.push(CustomControl {
        widget: Some(Widget::FilterBand { min_hz: 50., max_hz: 18000. }),
        id: "filter_graph".into(),
        name: "Filter frequency and width".into(),
        tooltip: "Drag horizontally for frequency, vertically for width".into(),
        bounds: [384.0, 74., 166., 80.],
        visible_when: None,
        gesture: UiGesture::Position,
        x: Some(UiBinding { parameter: "cutoff".into(), logarithmic: true, drag_scale: 1. }),
        y: Some(UiBinding::new("width")),
        drawing: vec![],
    });
    caption(&mut ui, "motion_mode_caption", "Mode", [579., 28., 32., 16.]);
    button(&mut ui, "mod_rate_mode", None, "Rate", [615., 28., 90., 20.]);
    if let Some(c) = ui.controls.last_mut() {
        c.name = "Motion rate mode".into();
        c.widget = Some(Widget::Menu);
        c.gesture = UiGesture::Menu;
    }
    for (id, mode, decimals, log) in [("mod_rate", 0, 2, true), ("mod_time", 1, 0, true)] {
        number(&mut ui, id, [579., 50., 96., 20.], decimals, log, TEXT, false);
        when(&mut ui, "mod_rate_mode", mode);
    }
    for mode in 2..=5 {
        let choices = (1..=64)
            .map(|value| {
                (
                    value,
                    match mode {
                        3 => format!("1/{value} T"),
                        4 => format!("1/{value} D"),
                        5 => format!("{value} x 1/16"),
                        _ => format!("1/{value}"),
                    },
                )
            })
            .collect();
        ui.controls.push(CustomControl {
            widget: Some(Widget::ValueMenu { choices }),
            id: format!("motion_division_{mode}"),
            name: "Motion rhythm".into(),
            tooltip: "Choose the note length; T = triplet, D = dotted; 16th counts sixteenth-note steps".into(),
            bounds: [579., 50., 96., 20.],
            visible_when: Some(equal("mod_rate_mode", mode)),
            gesture: UiGesture::Menu,
            x: Some(UiBinding::new("mod_division")),
            y: None,
            drawing: vec![],
        });
    }
    caption(&mut ui, "shape_caption", "Shape", [739., 28., 60., 16.]);
    ui.controls.push(CustomControl {
        widget: Some(Widget::WaveformMenu),
        id: "mod_shape".into(),
        name: "Motion waveform".into(),
        tooltip: "Choose the waveform".into(),
        bounds: [739., 46., 60., 22.],
        visible_when: None,
        gesture: UiGesture::Menu,
        x: Some(UiBinding::new("mod_shape")),
        y: None,
        drawing: vec![],
    });
    dial(&mut ui, "mod_delay", "Delay\ndepth", [579.0, 78., 64., 86.], false);
    dial(&mut ui, "mod_filter", "Filter\ndepth", [659., 78., 64., 86.], false);
    dial(&mut ui, "mod_morph", "Morph", [739., 78., 64., 86.], false);
    for control in &mut ui.controls {
        if control.id.starts_with("offset_") {
            control.widget = Some(Widget::Number {
                decimals: 1,
                format: suhina_device_sdk::ui::TextFormat::Number,
                color: TEXT,
                center: false,
                bordered: true,
                prefix: String::new(),
                suffix: String::new(),
            });
        }
    }
    ui
}

fn parameter(
    graph: &mut suhina_device_sdk::DeviceGraph,
    id: &str,
    range: (f64, f64),
    default: f64,
    unit: &str,
    logarithmic: bool,
) {
    let mut p = suhina_device_sdk::Parameter::new(id, id, range, default);
    p.unit = unit.into();
    if logarithmic {
        p.control = suhina_device_sdk::ParameterControl::LogSlider;
    }
    graph.parameters.push(p);
    graph.nodes.push(suhina_device_sdk::Node::new(id, suhina_device_sdk::NodeKind::Parameter { parameter: id.into() }));
}

fn wire(graph: &mut suhina_device_sdk::DeviceGraph, from: &str, to: &str, port: &str) {
    graph.connections.push(suhina_device_sdk::Connection::ports(from, "out", to, port));
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
        p.control = match p.id.as_str() {
            "cutoff" | "rate" | "time" => suhina_device_sdk::ParameterControl::LogKnob,
            "drive" | "feedback" | "width" => suhina_device_sdk::ParameterControl::Knob,
            "ratio" | "attack" | "release" => suhina_device_sdk::ParameterControl::Number,
            _ => p.control,
        };
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
