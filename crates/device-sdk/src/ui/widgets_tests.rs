// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
#[test]
fn the_eq_curves_display_takes_no_binding_and_draws_nothing_of_its_own() {
    let control = |x: Option<UiBinding>, y: Option<UiBinding>| CustomControl {
        widget: Some(Widget::EqCurve),
        id: "curve".into(),
        name: "EQ curve".into(),
        tooltip: String::new(),
        bounds: [0., 0., 440., 168.],
        visible_when: None,
        gesture: UiGesture::Drag,
        x,
        y,
        drawing: vec![],
    };
    let ui = DeviceUi { controls: vec![control(None, None)], ..DeviceUi::default() };
    assert!(ui.validate(&[]).is_ok(), "the display finds its bands on the host side");
    assert!(ui.controls[0].drawings().is_empty(), "the host draws it");
    let ui = DeviceUi { controls: vec![control(Some(UiBinding::new("x")), None)], ..DeviceUi::default() };
    assert!(ui.validate(&[]).is_err(), "a binding would name one parameter where the bands are forty");
}

#[test]
fn panels_follow_the_theme_and_tabs_validate_their_zero_based_range() {
    let parameter = Parameter::new("view", "View", (0., 2.), 0.);
    let mut ui = DeviceUi::default();
    ui.controls.push(CustomControl {
        id: "tab".into(),
        name: "Tab".into(),
        tooltip: String::new(),
        bounds: [0., 0., 100., 20.],
        widget: Some(Widget::Button { label: "LFO".into(), off_label: None, indicator: false }),
        gesture: UiGesture::Tab(2),
        x: Some(UiBinding::new("view")),
        y: None,
        drawing: vec![],
        visible_when: None,
    });
    assert!(ui.validate(std::slice::from_ref(&parameter)).is_ok());
    let decoded: DeviceUi = serde_json::from_str(&serde_json::to_string(&ui).unwrap()).unwrap();
    assert_eq!(decoded.controls[0].gesture, UiGesture::Tab(2));
    for invalid in [0, 3] {
        ui.controls[0].gesture = UiGesture::Tab(invalid);
        assert!(ui.validate(std::slice::from_ref(&parameter)).is_err());
    }
    ui.controls[0].gesture = UiGesture::Tab(1);
    let mut nonzero = parameter.clone();
    nonzero.min = 1.;
    assert!(ui.validate(&[nonzero]).is_err());
    ui.controls[0].widget = Some(Widget::Panel { label: "FILTER".into() });
    ui.controls[0].gesture = UiGesture::Drag;
    ui.controls[0].x = None;
    assert!(ui.validate(&[parameter]).is_ok());
    let palette = WidgetPalette { control: [100, 110, 120, 255], ..WidgetPalette::default() };
    let drawing = ui.controls[0].drawings_with_palette(&palette);
    assert!(matches!(&drawing[1], Draw::Rect { color, .. } if *color == palette.control));
}
#[test]
fn widgets_work_with_unrelated_parameter_names() {
    let mut amount = Parameter::new("amount", "Amount", (1., 64.), 4.);
    amount.control = crate::ParameterControl::Choice;
    amount.ui.options = (1..=64).map(|i| i.to_string()).collect();
    let spread = Parameter::new("spread", "Spread", (0.5, 9.), 2.);
    let widgets = [
        (Widget::Button { label: "Enable".into(), off_label: None, indicator: false }, UiGesture::Toggle),
        (
            Widget::Number {
                decimals: 2,
                format: TextFormat::Number,
                color: ACCENT,
                center: false,
                bordered: false,
                prefix: String::new(),
                suffix: String::new(),
            },
            UiGesture::Drag,
        ),
        (Widget::Knob { label: "Amount".into() }, UiGesture::Drag),
        (Widget::Dial { label: "Amount".into(), decimals: 0, format: TextFormat::Number }, UiGesture::Drag),
        (Widget::Stepper { decimals: 1 }, UiGesture::Drag),
        (Widget::Disclosure { label: "Details".into() }, UiGesture::Toggle),
        (Widget::Menu, UiGesture::Menu),
        (Widget::WaveformMenu, UiGesture::Menu),
        (Widget::FilterBand { min_hz: 1., max_hz: 64. }, UiGesture::Position),
    ];
    for (widget, gesture) in widgets {
        let mut bound_amount = amount.clone();
        if matches!(widget, Widget::WaveformMenu) {
            bound_amount.min = 0.;
            bound_amount.max = 6.;
            bound_amount.ui.options.truncate(7);
        }
        let position = gesture == UiGesture::Position;
        let drag = gesture == UiGesture::Drag;
        let control = CustomControl {
            widget: Some(widget),
            id: "reused".into(),
            name: "Reused".into(),
            tooltip: String::new(),
            bounds: [0., 0., 100., 100.],
            visible_when: None,
            gesture,
            x: (!drag).then(|| UiBinding { parameter: "amount".into(), logarithmic: position, drag_scale: 1. }),
            y: if drag {
                Some(UiBinding::new("amount"))
            } else if position {
                Some(UiBinding::new("spread"))
            } else {
                None
            },
            drawing: vec![],
        };
        assert!(!control.drawings().is_empty());
        let ui = DeviceUi { controls: vec![control], ..DeviceUi::default() };
        ui.validate(&[bound_amount, spread.clone()]).unwrap();
    }
}

#[test]
fn widget_drawings_resolve_the_host_palette() {
    let control = CustomControl {
        widget: Some(Widget::Button { label: "Enable".into(), off_label: None, indicator: false }),
        id: "enable".into(),
        name: "Enable".into(),
        tooltip: String::new(),
        bounds: [0.0, 0.0, 40.0, 12.0],
        visible_when: None,
        gesture: UiGesture::Toggle,
        x: Some(UiBinding::new("enabled")),
        y: None,
        drawing: Vec::new(),
    };
    let palette = WidgetPalette {
        text: [1, 2, 3, 255],
        panel: [4, 5, 6, 255],
        control: [7, 8, 9, 255],
        border: [10, 11, 12, 255],
        accent: [13, 14, 15, 255],
        accent_text: [16, 17, 18, 255],
    };
    let draws = control.drawings_with_palette(&palette);
    assert!(matches!(&draws[0], Draw::Rect { color, .. } if *color == palette.control));
    assert!(matches!(&draws[1], Draw::Rect { color, .. } if *color == palette.accent));
    assert!(draws.iter().any(|draw| matches!(draw, Draw::Text { color, .. } if *color == palette.text)));

    let knob = CustomControl {
        widget: Some(Widget::Knob { label: "Amount".into() }),
        id: "amount".into(),
        name: "Amount".into(),
        tooltip: String::new(),
        bounds: [0.0, 0.0, 40.0, 40.0],
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding::new("amount")),
        drawing: Vec::new(),
    };
    let draws = knob.drawings_with_palette(&palette);
    assert!(draws.iter().any(|draw| matches!(draw, Draw::Circle { color, .. } if *color == palette.border)));
    assert!(draws.iter().any(|draw| matches!(draw, Draw::Line { color, .. } if *color == palette.accent)));
    assert!(!draws.iter().any(|draw| matches!(draw, Draw::Arc { .. })));

    let authored = CustomControl {
        widget: None,
        id: "authored".into(),
        name: "Authored".into(),
        tooltip: String::new(),
        bounds: [0.0, 0.0, 40.0, 12.0],
        visible_when: None,
        gesture: UiGesture::Toggle,
        x: Some(UiBinding::new("enabled")),
        y: None,
        drawing: vec![Draw::Rect {
            rect: [0.0.into(), 0.0.into(), 40.0.into(), 12.0.into()],
            color: [166, 163, 255, 255],
            radius: 0.0,
        }],
    };
    assert!(
        matches!(&authored.drawings_with_palette(&palette)[0], Draw::Rect { color, .. } if *color == [166, 163, 255, 255])
    );

    let number = CustomControl {
        widget: Some(Widget::Number {
            decimals: 0,
            format: TextFormat::Number,
            color: TEXT,
            center: true,
            bordered: true,
            prefix: String::new(),
            suffix: String::new(),
        }),
        id: "value".into(),
        name: "Value".into(),
        tooltip: String::new(),
        bounds: [0.0, 0.0, 40.0, 12.0],
        visible_when: None,
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding::new("value")),
        drawing: Vec::new(),
    };
    let draws = number.drawings_with_palette(&palette);
    assert!(matches!(&draws[0], Draw::Rect { color, .. } if *color == palette.border));
    assert!(matches!(&draws[1], Draw::Rect { color, .. } if *color == palette.control));
}
