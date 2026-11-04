// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The EQ's panel (docs/spec/eq.md › The panel): a response display with the bands' dots, the
//! selected band's Frequency, Gain and Q strips, the bands' type menus and numbered selectors
//! and the Edit switch — everything that follows the selected band is one host-drawn widget
//! ([`Widget::EqCurve`]) — and the global controls beside it, the SDK's own.

use suhina_device_sdk::builtin::{COMPACT_HEIGHT, Layout};
use suhina_device_sdk::ui::*;

/// The panel's width: the display with the strips beside it, and the globals' column.
pub const WIDTH: f32 = 440.0;
/// Where the globals' column starts.
pub const GLOBALS_X: f32 = 356.0;
/// The globals' column's width.
pub const GLOBALS_WIDTH: f32 = WIDTH - GLOBALS_X - 2.0;

const TEXT: [u8; 4] = [236, 232, 225, 255];

/// The layout of the EQ's panel (ADR 0021 › Amendment: device panels for built-in devices).
pub fn layout() -> Layout {
    Layout { ui: Some(panel()), ..Layout::default() }
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
    ui.controls.push(CustomControl {
        id: "eq-curve".into(),
        name: "EQ curve".into(),
        tooltip: "Drag a band's dot to move its frequency and gain; double-click resets the \
                  band. The number buttons select a band and switch it; the strips edit the \
                  selected band's frequency, gain and Q."
            .into(),
        bounds: [0., 0., WIDTH, COMPACT_HEIGHT],
        widget: Some(Widget::EqCurve),
        gesture: UiGesture::Drag,
        x: None,
        y: None,
        drawing: vec![],
        visible_when: None,
    });
    caption(&mut ui, "Mode", 4.0);
    // The globals' column: a row every 28 px, 8 px between rows, from the caption at 4 to the
    // Edit switch at 135 (which the curve display's host side draws, `eq_curve.rs`).
    ui.controls.push(CustomControl {
        id: "mode".into(),
        name: "Mode".into(),
        tooltip: "Stereo runs curve A on both sides; Left/Right and Mid/Side run curve A on the \
                  left or mid and curve B on the right or side."
            .into(),
        bounds: [GLOBALS_X, 23.0, GLOBALS_WIDTH, 20.0],
        widget: Some(Widget::Menu),
        gesture: UiGesture::Menu,
        x: Some(UiBinding::new("mode")),
        y: None,
        drawing: vec![],
        visible_when: None,
    });
    toggle(&mut ui, "Adapt. Q", 51.0);
    number(&mut ui, "scale", "Scale", "Scale ", TextFormat::Percent, 79.0);
    number(&mut ui, "output", "Output", "Output ", TextFormat::Decibels, 107.0);
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
        bounds: [GLOBALS_X, y, GLOBALS_WIDTH, 20.0],
        widget: Some(Widget::Button { label: label.into(), off_label: None, indicator: false }),
        gesture: UiGesture::Toggle,
        x: Some(UiBinding::new("adaptiveQ")),
        y: None,
        drawing: vec![],
        visible_when: None,
    });
}

/// A value in the globals' column: Scale or Output, named by a prefix on its readout.
fn number(ui: &mut DeviceUi, id: &str, name: &str, prefix: &str, format: TextFormat, y: f32) {
    ui.controls.push(CustomControl {
        id: id.into(),
        name: name.into(),
        tooltip: "Drag vertically; Shift for precision; double click resets".into(),
        bounds: [GLOBALS_X, y, GLOBALS_WIDTH, 20.0],
        widget: Some(Widget::Number {
            decimals: 0,
            format,
            color: TEXT,
            center: false,
            bordered: true,
            prefix: prefix.into(),
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
        assert_eq!(loaded.expanded, None, "the compact panel is the whole panel");
        let ids: Vec<_> = loaded.parameters.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, INFO.params.iter().map(|p| p.id).collect::<Vec<_>>());
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
        assert_eq!(bound, ["mode", "adaptiveQ", "scale", "output"], "the globals are the canvas's bindings");
    }
}
