// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use suhina_device_sdk::{
    DeviceGraph, DeviceRole, NodeKind as K, Parameter, ParameterControl as C, author::GraphBuilder, ui::*,
};
#[path = "common/mod.rs"]
mod examples;

#[test]
fn references_ports_and_failed_operations_do_not_corrupt_the_builder() {
    let mut g = GraphBuilder::new("test", "Test", DeviceRole::Effect);
    let input = g.node("in", K::AudioInput).unwrap();
    let output = g.node("out", K::AudioOutput).unwrap();
    let foreign = GraphBuilder::new("other", "Other", DeviceRole::Effect).node("in", K::AudioInput).unwrap();
    assert!(g.connect(foreign.out(), output.input("in")).unwrap_err().to_string().contains("another graph"));
    assert!(g.connect(input.output("missing"), output.input("in")).is_err());
    assert!(g.connect(input.out(), output.input("missing")).is_err());
    assert!(g.connect(output.out(), output.input("in")).is_err());
    assert!(g.node("in", K::Add).is_err());
    assert!(g.node("", K::Add).is_err());
    assert!(g.op("retry", K::Add, &[("a", input.out()), ("a", input.out())]).is_err());
    assert!(g.parameter_signal(Parameter::new("in", "Collision", (0., 1.), 1.)).is_err());
    // Both operations must roll back their partial insertions.
    let retry = g.binary("retry", K::Multiply, input.out(), input.out()).unwrap();
    g.parameter(Parameter::new("in", "UI only", (0., 1.), 1.)).unwrap();
    assert!(g.parameter(Parameter::new("in", "Duplicate", (0., 1.), 1.)).is_err());
    g.connect(retry.out(), output.input("in")).unwrap();
    assert!(g.connect(input.out(), output.input("in")).is_err());
    let graph = g.finish().unwrap();
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.parameters.len(), 1);
    assert_eq!(DeviceGraph::from_sd(&graph.to_sd().unwrap()).unwrap(), graph);
}

fn feedback(delayed: bool) -> Result<DeviceGraph, suhina_device_sdk::DeviceSdkError> {
    let mut g = GraphBuilder::new("feedback", "Feedback", DeviceRole::Effect);
    let input = g.node("in", K::AudioInput)?;
    let output = g.node("out", K::AudioOutput)?;
    let sum = g.node("sum", K::Add)?;
    let state = g.op("state", if delayed { K::SampleDelay } else { K::Abs }, &[("in", sum.out())])?;
    g.connect(input.out(), sum.input("a"))?;
    g.connect(state.out(), sum.input("b"))?;
    g.connect(sum.out(), output.input("in"))?;
    g.finish()
}

#[test]
fn finish_still_enforces_feedback_parameter_and_resource_contracts() {
    assert!(feedback(true).is_ok());
    assert!(feedback(false).unwrap_err().to_string().contains("cycle"));
    let mut g = GraphBuilder::new("test", "Test", DeviceRole::Effect);
    g.parameter(Parameter::new("bad", "Bad", (1., 2.), f64::NAN)).unwrap();
    assert!(g.finish().is_err());
    let mut g = GraphBuilder::new("test", "Test", DeviceRole::Effect);
    for i in 0..suhina_device_sdk::MAX_NODES {
        g.constant(format!("n{i}"), 0.).unwrap();
    }
    assert!(g.constant("extra", 0.).is_err());
    assert!(g.finish().is_err()); // Unconnected graphs never bypass the normal validator.
}

#[test]
fn ui_constructors_preserve_metadata_identity_and_independent_conditions() {
    let p = Parameter::new("cutoff", "Frequency", (20., 20000.), 1000.).unit("Hz").control(C::LogKnob);
    let control = CustomControl::number("cutoff", [0., 0., 80., 24.])
        .parameter_metadata(&p)
        .id("frequency_free")
        .visible_when(UiValue::parameter("sync", false).equals(0.));
    assert_eq!(control.id, "frequency_free");
    assert_eq!(control.name, "Frequency");
    assert_eq!(control.gesture, UiGesture::Drag);
    assert!(control.y.as_ref().unwrap().logarithmic);
    let mut graph = DeviceGraph::from_sd(include_str!("fixtures/authoring/filter.sd")).unwrap();
    graph.parameters.push(Parameter::new("sync", "Sync", (0., 1.), 0.).control(C::Toggle));
    let second = control.clone().id("frequency_sync").visible_when(UiValue::parameter("sync", false).equals(1.));
    let toggle = CustomControl::toggle("sync", "Sync", [0., 30., 80., 24.]);
    let menu = CustomControl::value_menu("sync", vec![(0, "Free".into()), (1, "Synced".into())], [90., 30., 80., 24.]);
    let menu = menu.id("sync_menu");
    assert_eq!(toggle.gesture, UiGesture::Toggle);
    assert_eq!(menu.gesture, UiGesture::Menu);
    graph.ui = Some(DeviceUi { controls: vec![control, second, toggle, menu], ..DeviceUi::default() });
    let saved = graph.to_sd().unwrap();
    assert_eq!(DeviceGraph::from_sd(&saved).unwrap(), graph);
    let ui = graph.ui.unwrap();
    for sync in [0., 1.] {
        for (i, control) in ui.controls.iter().take(2).enumerate() {
            assert_eq!(control.visible_when.as_ref().unwrap().evaluate(&|_, _, _| sync), f32::from(i == sync as usize));
        }
    }
    assert!(ui.hidden_parameters.is_empty());
}

#[test]
fn numeric_options_and_metadata_are_checked_and_can_be_overridden() {
    let mut parameter = Parameter::new("frequency", "Frequency", (20., 20000.), 1000.).unit("Hz").control(C::LogKnob);
    parameter.ui.decimals = 2;
    let control = CustomControl::number("frequency", [0., 0., 80., 24.]).parameter_metadata(&parameter);
    assert!(matches!(control.widget, Some(Widget::Number { decimals: 2, format: TextFormat::Frequency, .. })));
    let customized = control
        .clone()
        .format(TextFormat::Number)
        .unwrap()
        .decimals(1)
        .unwrap()
        .prefix("Hz: ")
        .unwrap()
        .bounds([10., 20., 90., 24.]);
    assert!(
        matches!(&customized.widget, Some(Widget::Number { decimals: 1, format: TextFormat::Number, prefix, .. }) if prefix == "Hz: ")
    );
    assert_eq!(customized.bounds, [10., 20., 90., 24.]);
    assert!(customized.y.as_ref().unwrap().logarithmic);
    assert!(control.clone().decimals(9).is_err());
    assert!(control.off_label("off").is_err());
    let label = CustomControl::label("label", "Label", [0., 0., 40., 16.]);
    assert!(label.clone().format(TextFormat::Number).is_err());
    assert!(label.clone().decimals(0).is_err());
    assert!(label.prefix("prefix").is_err());
    let toggle = CustomControl::toggle("enabled", "On", [0., 0., 40., 24.]).off_label("Off").unwrap();
    assert!(matches!(toggle.widget, Some(Widget::Button { off_label: Some(ref label), .. }) if label == "Off"));
    let unrelated = CustomControl::number("other", [0., 0., 80., 24.]);
    assert_eq!(unrelated.clone().parameter_metadata(&parameter), unrelated);
    let choice = Parameter::new("shape", "Shape", (0., 1.), 0.).control(C::Choice).options(["Sine", "Square"]);
    let readout = CustomControl::number("shape", [0., 0., 80., 24.]).parameter_metadata(&choice);
    assert!(matches!(readout.widget, Some(Widget::Number { format: TextFormat::Choice, .. })));
}

#[test]
fn groups_move_controls_together_without_changing_bindings_drawings_or_order() {
    let a = CustomControl::number("cutoff", [0., 0., 80., 24.]);
    let b = CustomControl::label("label", "Frequency", [0., 30., 80., 16.]);
    let group = ControlGroup::new([10., 20.]).unwrap();
    let moved = ControlGroup::new([110., 120.]).unwrap();
    for control in [a.clone(), b.clone()] {
        let first = group.place(control.clone()).unwrap();
        let second = moved.place(control.clone()).unwrap();
        assert_eq!(second.bounds[0] - first.bounds[0], 100.);
        assert_eq!(second.bounds[1] - first.bounds[1], 100.);
        let mut restored = first;
        restored.bounds = control.bounds;
        assert_eq!(restored, control);
    }
    let mut graph = DeviceGraph::from_sd(include_str!("fixtures/authoring/filter.sd")).unwrap();
    graph.ui =
        Some(DeviceUi { controls: vec![group.place(a).unwrap(), group.place(b).unwrap()], ..DeviceUi::default() });
    let saved = graph.to_sd().unwrap();
    assert_eq!(DeviceGraph::from_sd(&saved).unwrap(), graph);
    assert!(!saved.contains("origin"));
    assert!(ControlGroup::new([f32::NAN, 0.]).is_err());
    assert!(ControlGroup::new([-1., 0.]).is_err());
    assert!(ControlGroup::new([8193., 0.]).is_err());
    for bounds in [[f32::INFINITY, 0., 80., 24.], [-1., 0., 80., 24.], [0., 0., 0., 24.], [8190., 0., 80., 24.]] {
        assert!(group.place(CustomControl::number("cutoff", bounds)).is_err());
    }
    graph.ui.as_mut().unwrap().size = [40., 40.];
    assert!(graph.to_sd().is_err()); // Final bounds are also checked against the actual canvas.
}

#[test]
fn migrated_examples_preserve_serialized_contracts_and_offline_renders() {
    for (name, text) in [
        ("filter", include_str!("fixtures/authoring/filter.sd")),
        ("auto-pan", include_str!("fixtures/authoring/auto-pan.sd")),
        ("delay", include_str!("fixtures/authoring/delay.sd")),
    ] {
        let mut before = DeviceGraph::from_sd(text).unwrap();
        let mut after = examples::build(name).unwrap();
        for graph in [&mut before, &mut after] {
            graph.connections.sort_by(|a, b| {
                (&a.to, &a.to_port, &a.from, &a.from_port).cmp(&(&b.to, &b.to_port, &b.from, &b.from_port))
            });
        }
        assert_eq!(before, after, "serialized contract changed for {name}");
        for rate in [44100., 96000.] {
            for graph in [&mut before, &mut after] {
                if name == "delay" {
                    for p in &mut graph.parameters {
                        p.default = match p.id.as_str() {
                            "ping_pong" | "mod_rate" => 1.,
                            "mod_delay" => 30.,
                            "mod_filter" => 20.,
                            "time_left" => 15.,
                            "time_right" => 21.,
                            "sync_left" | "sync_right" => 0.,
                            _ => p.default,
                        };
                    }
                }
            }
            let mut expected = vec![0.; 48000];
            expected[0] = 1.;
            expected[1] = -0.5;
            let mut actual = expected.clone();
            before.compile_at(rate).unwrap().process(&mut expected);
            after.compile_at(rate).unwrap().process(&mut actual);
            assert_eq!(expected, actual, "render changed for {name} at {rate}");
        }
    }
}
