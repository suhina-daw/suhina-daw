// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The EQ's panel (docs/spec/eq.md › The panel): the selected band's column, the response display
//! with the bands' dots and the output meter — everything that follows the selected band is one
//! host-drawn widget ([`Widget::EqCurve`]) — and the global controls beside it, the SDK's own.
//! The expanded panel is a strip over the device row, as tall as the user drags it, with the
//! same widget over all of it; the widget lays itself out from the room it is given.

use suhina_device_sdk::builtin::{COMPACT_HEIGHT, Layout};
use suhina_device_sdk::ui::*;

/// The panel's width: the band column, the display, the globals' column and the output meter.
pub const WIDTH: f32 = 560.0;
/// Where the globals' column starts.
pub const GLOBALS_X: f32 = 486.0;
/// The globals' column's width; the output meter takes the 12 px right of it.
pub const GLOBALS_WIDTH: f32 = 60.0;
/// The expanded strip's canvas: as wide as the strip, its height the strip's at that width.
pub const STRIP_SIZE: [f32; 2] = [1200.0, 160.0];

const TEXT: [u8; 4] = [236, 232, 225, 255];

/// The layout of the EQ's panel (ADR 0021 › Amendment: device panels for built-in devices).
pub fn layout() -> Layout {
    Layout { ui: Some(panel()), expanded: Some(strip()), ..Layout::default() }
}

/// The expanded strip: the curve display's widget over all of it, laid out from the strip's room.
fn strip() -> DeviceUi {
    let mut ui = DeviceUi {
        size: STRIP_SIZE,
        strip: true,
        hidden_parameters: crate::INFO.params.iter().map(|p| p.id.to_string()).collect(),
        ..DeviceUi::default()
    };
    ui.controls.push(curve([0.0, 0.0, STRIP_SIZE[0], STRIP_SIZE[1]]));
    ui
}

/// The host-drawn widget over `bounds`.
fn curve(bounds: [f32; 4]) -> CustomControl {
    CustomControl {
        id: "eq-curve".into(),
        name: "EQ curve".into(),
        tooltip: "Double-click the display to make a band. Drag a band's dot to move its frequency                   and gain, the wheel over it sets its Q, a double-click switches it, a right                   click picks its shape. The column edits the selected band."
            .into(),
        bounds,
        widget: Some(Widget::EqCurve),
        gesture: UiGesture::Drag,
        x: None,
        y: None,
        drawing: vec![],
        visible_when: None,
    }
}

/// The compact canvas: the curve display's widget over the whole panel, the globals' column
/// beside it. The globals are ordinary controls so the theme draws them; the display needs
/// the selected band, which is panel state, so it draws itself on the host side.
fn panel() -> DeviceUi {
    let mut ui = DeviceUi {
        size: [WIDTH, COMPACT_HEIGHT],
        // The canvas draws every parameter, so the generated list under it draws none of them.
        hidden_parameters: crate::INFO.params.iter().map(|p| p.id.to_string()).collect(),
        ..DeviceUi::default()
    };
    ui.controls.push(curve([0., 0., WIDTH, COMPACT_HEIGHT]));
    // The globals' column: Adaptive Q, then Scale and Output each under its caption.
    toggle(&mut ui, "Adapt. Q", 4.0);
    caption(&mut ui, "Scale", 28.0);
    number(&mut ui, "scale", "Scale", TextFormat::Percent, 42.0);
    caption(&mut ui, "Output", 66.0);
    number(&mut ui, "output", "Output", TextFormat::Decibels, 80.0);
    ui
}

/// A small heading in the globals' column. The label's 11-px font needs 14 px of bounds for its
/// ink (the sampler's labels give it that too); a shorter box clips the glyphs' bottoms.
fn caption(ui: &mut DeviceUi, text: &str, y: f32) {
    ui.controls.push(CustomControl {
        id: format!("{text}-caption"),
        name: format!("{text} caption"),
        tooltip: String::new(),
        bounds: [GLOBALS_X, y, GLOBALS_WIDTH, 14.0],
        widget: Some(Widget::Label { label: text.into() }),
        gesture: UiGesture::Drag,
        x: None,
        y: None,
        drawing: vec![],
        visible_when: None,
    });
}

/// A toggle in the globals' column: Adaptive Q, the only one.
fn toggle(ui: &mut DeviceUi, label: &str, y: f32) {
    ui.controls.push(CustomControl {
        id: "adaptiveQ".into(),
        name: "Adaptive Q".into(),
        tooltip: "Narrows a bell as its gain grows past 6 dB and widens it below.".into(),
        bounds: [GLOBALS_X, y, GLOBALS_WIDTH, 18.0],
        widget: Some(Widget::Button { label: label.into(), off_label: None, indicator: false }),
        gesture: UiGesture::Toggle,
        x: Some(UiBinding::new("adaptiveQ")),
        y: None,
        drawing: vec![],
        visible_when: None,
    });
}

/// A value in the globals' column: Scale or Output, under its caption.
fn number(ui: &mut DeviceUi, id: &str, name: &str, format: TextFormat, y: f32) {
    ui.controls.push(CustomControl {
        id: id.into(),
        name: name.into(),
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds: [GLOBALS_X, y, GLOBALS_WIDTH, 20.0],
        widget: Some(Widget::Number {
            decimals: 0,
            format,
            color: TEXT,
            center: true,
            bordered: true,
            prefix: String::new(),
            suffix: String::new(),
        }),
        gesture: UiGesture::Drag,
        x: None,
        y: Some(UiBinding::new(id)),
        drawing: vec![],
        visible_when: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::INFO;
    use suhina_device_sdk::ui::Widget;

    #[test]
    fn the_panel_loads_and_keeps_the_devices_parameters() {
        let loaded = layout().panel(&INFO).unwrap();
        let canvas = loaded.ui.as_ref().unwrap();
        assert_eq!(canvas.size, [WIDTH, COMPACT_HEIGHT]);
        let strip = loaded.expanded.as_ref().expect("the EQ has an expanded strip");
        assert!(strip.strip, "it opens as a strip over the device row");
        assert!(matches!(strip.controls.as_slice(), [c] if matches!(c.widget, Some(Widget::EqCurve))));
        let ids: Vec<_> = loaded.parameters.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, INFO.params.iter().map(|p| &*p.id).collect::<Vec<_>>());
        let curve = canvas.controls.first().unwrap();
        assert!(matches!(curve.widget, Some(Widget::EqCurve)), "the display is one widget");
        for control in &canvas.controls {
            let [x, y, w, h] = control.bounds;
            assert!(x + w <= canvas.size[0] && y + h <= canvas.size[1], "{} is outside", control.id);
        }
        let bound: Vec<_> = canvas
            .controls
            .iter()
            .flat_map(|c| [c.x.as_ref(), c.y.as_ref()].into_iter().flatten())
            .map(|b| b.parameter.as_str())
            .collect();
        assert_eq!(bound, ["adaptiveQ", "scale", "output"], "the globals are the canvas's bindings");
    }
}
