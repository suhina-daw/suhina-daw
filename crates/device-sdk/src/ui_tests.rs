// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn readout(index: u8) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Readout { index }))
}

#[test]
fn readouts_read_the_hosts_values_and_refuse_an_index_past_the_last() {
    let lookup = |id: &str, _: bool, _: bool| if id == "#r3" { -4.5 } else { 0.0 };
    assert_eq!(readout(3).evaluate(&lookup), -4.5);
    assert!(readout(31).validate(&HashSet::new(), 0, &mut 32768).is_ok());
    assert!(readout(35).validate(&HashSet::new(), 0, &mut 32768).is_err());
    let min = UiValue::Expression(Box::new(UiExpression::Min { a: 2.0.into(), b: readout(3) }));
    assert_eq!(min.evaluate(&lookup), -4.5);
}

#[test]
fn graphics_need_no_binding_and_a_negative_drag_scale_is_allowed() {
    let params = [Parameter::new("slope", "Slope", (0., 2.), 1.)];
    let mut ui = DeviceUi { size: [100., 100.], ..DeviceUi::default() };
    ui.controls.push(CustomControl {
        id: "view".into(),
        name: "View".into(),
        tooltip: String::new(),
        bounds: [0., 0., 100., 50.],
        widget: Some(Widget::Graphic { drawing: vec![] }),
        gesture: UiGesture::Drag,
        x: None,
        y: None,
        drawing: vec![],
        visible_when: None,
    });
    ui.controls.push(CustomControl {
        id: "ratio".into(),
        name: "Ratio".into(),
        tooltip: String::new(),
        bounds: [0., 50., 50., 50.],
        widget: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding { drag_scale: -1.0, ..UiBinding::new("slope") }),
        drawing: vec![],
        visible_when: None,
    });
    ui.validate(&params).unwrap();
    ui.controls[1].y = Some(UiBinding { drag_scale: -0.001, ..UiBinding::new("slope") });
    assert!(ui.validate(&params).is_err(), "a scale under 0.01 either way");
}

#[test]
fn handles_bind_one_axis_and_presets_name_declared_values() {
    let params = [Parameter::new("cut", "Cut", (20., 20_000.), 1_000.)];
    let mut ui = DeviceUi { size: [200., 60.], ..DeviceUi::default() };
    ui.controls.push(CustomControl {
        id: "cut_handle".into(),
        name: "Cut".into(),
        tooltip: String::new(),
        bounds: [0., 0., 200., 60.],
        widget: Some(Widget::Handle { color: [255; 4], span: Some([0.0.into(), readout(0)]) }),
        gesture: UiGesture::Position,
        x: Some(UiBinding::new("cut")),
        y: None,
        drawing: vec![],
        visible_when: Some(UiValue::parameter(EXPANDED_ID, false)),
    });
    ui.validate(&params).unwrap();
    ui.controls[0].y = Some(UiBinding::new("cut"));
    assert!(ui.validate(&params).is_err(), "a handle has one axis");
    ui.controls[0].y = None;
    ui.presets.push(UiPreset { name: "Bright".into(), values: [("cut".into(), 8_000.0)].into() });
    ui.validate(&params).unwrap();
    ui.presets[0].values.insert("cut".into(), 50_000.0);
    assert!(ui.validate(&params).is_err(), "a preset value outside its range");
}

#[test]
fn waveform_expressions_round_trip_and_validate_parameter_references() {
    let value = UiValue::Expression(Box::new(UiExpression::Wave {
        shape: UiValue::parameter("shape", false),
        cycles: 0.25.into(),
    }));
    let saved = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<UiValue>(&saved).unwrap(), value);
    assert_eq!(value.evaluate(&|_, _, _| 1.), 1.);
    assert!(value.validate(&HashSet::from(["shape"]), 0, &mut 32768).is_ok());
    assert!(value.validate(&HashSet::new(), 0, &mut 32768).is_err());
}

#[test]
fn waveform_preview_round_trips_and_rejects_missing_shape() {
    let mut shape = Parameter::new("shape", "Shape", (0., 5.), 0.);
    shape.ui.options = ["Sine", "Triangle", "Saw up", "Saw down", "Square", "Random"].map(String::from).to_vec();
    let params = [Parameter::new("rate", "Rate", (0.01, 20.), 1.), shape];
    let mut ui = DeviceUi { size: [200., 60.], ..DeviceUi::default() };
    ui.controls.push(CustomControl {
        id: "preview".into(),
        name: "Preview".into(),
        tooltip: String::new(),
        bounds: [0., 0., 200., 60.],
        widget: Some(Widget::WaveformPreview { shape: "shape".into() }),
        gesture: UiGesture::Position,
        x: Some(UiBinding::new("rate")),
        y: None,
        drawing: vec![],
        visible_when: None,
    });
    ui.validate(&params).unwrap();
    assert_eq!(serde_json::from_str::<DeviceUi>(&serde_json::to_string(&ui).unwrap()).unwrap(), ui);
    assert!(!ui.controls[0].drawings().is_empty());
    ui.controls[0].widget = Some(Widget::WaveformPreview { shape: "missing".into() });
    assert!(ui.validate(&params).is_err());
}

#[test]
fn envelope_widgets_round_trip_and_reject_invalid_stage_references() {
    let params = [
        Parameter::new("a", "Attack", (0., 10.), 0.),
        Parameter::new("d", "Decay", (0., 10.), 0.),
        Parameter::new("s", "Sustain", (0., 1.), 1.),
        Parameter::new("r", "Release", (0., 10.), 0.),
    ];
    let widget =
        Widget::Envelope { attack: "a".into(), decay: "d".into(), level: "s".into(), release: Some("r".into()) };
    let mut ui = DeviceUi {
        controls: vec![CustomControl {
            widget: Some(widget),
            visible_when: None,
            id: "env".into(),
            name: "Envelope".into(),
            tooltip: String::new(),
            bounds: [0., 0., 284., 40.],
            gesture: UiGesture::Position,
            x: None,
            y: None,
            drawing: vec![],
        }],
        ..DeviceUi::default()
    };
    ui.validate(&params).unwrap();
    assert_eq!(serde_json::from_str::<DeviceUi>(&serde_json::to_string(&ui).unwrap()).unwrap(), ui);
    for attack in ["missing", "d"] {
        if let Some(Widget::Envelope { attack: a, .. }) = &mut ui.controls[0].widget {
            *a = attack.into();
        }
        assert!(ui.validate(&params).is_err());
    }
}

#[test]
fn drag_scale_defaults_to_one_and_rejects_invalid_values() {
    let old: UiBinding = serde_json::from_str(r#"{"parameter":"amount"}"#).unwrap();
    assert_eq!(old.drag_scale, 1.);
    let mut ui = DeviceUi {
        controls: vec![CustomControl {
            widget: None,
            visible_when: None,
            id: "amount".into(),
            name: "Amount".into(),
            tooltip: String::new(),
            bounds: [0., 0., 40., 20.],
            gesture: UiGesture::Drag,
            x: Some(old),
            y: None,
            drawing: vec![],
        }],
        ..DeviceUi::default()
    };
    let parameters = [Parameter::new("amount", "Amount", (0., 1.), 0.)];
    // A negative scale drags the other way; its size has the same limits.
    for scale in [1., 2., -1., 0., -0.001, f32::NAN, f32::INFINITY, 101., -101.] {
        ui.controls[0].x.as_mut().unwrap().drag_scale = scale;
        assert_eq!(ui.validate(&parameters).is_ok(), [1., 2., -1.].contains(&scale), "{scale}");
    }
}
