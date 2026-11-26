// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Compressor's panel (docs/spec/compressor.md › The panel): the curve's dials and the input
//! at the left, the display in the middle ([`Widget::CompressorDisplay`], drawn on the host side from
//! [`crate::gain_change`] and the device's readouts, with its Curve, Activity and Meters views)
//! and the timing, mix, output and key filter dials at the right. The dials are the SDK's own, so the theme draws them like
//! every other device's.

use suhina_device_sdk::builtin::{COMPACT_HEIGHT, Layout};
use suhina_device_sdk::ui::*;

/// A dial's cell.
const DIAL: [f32; 2] = [60.0, 84.0];
/// The gap between the dial columns and the display.
const GAP: f32 = 6.0;
/// The display's rectangle, between the two groups of dials.
pub const DISPLAY: [f32; 4] = [2.0 * DIAL[0] + GAP, 0.0, 316.0, COMPACT_HEIGHT];
/// Where the level dials start, right of the display.
const RIGHT_X: f32 = DISPLAY[0] + DISPLAY[2] + GAP;
/// The panel's width: two dial columns, the display, three dial columns.
pub const WIDTH: f32 = RIGHT_X + 3.0 * DIAL[0];
const TOOLTIP: &str = "Drag vertically; Shift for precision; double click resets";

/// The layout of the Compressor's panel (ADR 0021 › Amendment: device panels for built-in
/// devices).
pub fn layout() -> Layout {
    Layout { ui: Some(panel()), ..Layout::default() }
}

/// A dial for `id` in the cell at `x`, row `row`.
fn dial(
    id: &str,
    label: &str,
    x: f32,
    row: usize,
    logarithmic: bool,
    format: TextFormat,
    decimals: usize,
) -> CustomControl {
    let mut c = CustomControl::widget(
        id,
        label,
        Widget::Dial { label: label.into(), decimals, format, image: None, readout: None },
        [x, row as f32 * DIAL[1], DIAL[0], DIAL[1]],
    );
    c.tooltip = TOOLTIP.into();
    c.y = Some(UiBinding { parameter: id.into(), logarithmic, drag_scale: 1.0 });
    c
}

fn panel() -> DeviceUi {
    let left = |i: usize| i as f32 * DIAL[0];
    let right = |i: usize| RIGHT_X + i as f32 * DIAL[0];
    let mut display = CustomControl::widget("display", "Compressor display", Widget::CompressorDisplay, DISPLAY);
    display.tooltip = "Curve: the input level across, the output up; drag the dot for the threshold (sideways) \
                       and the ratio (up and down). Activity: the level the detector hears and the gain \
                       reduction scrolling by. Meters: input, gain reduction and output. Drag the arrow by \
                       the input meter for the threshold."
        .into();
    let controls = vec![
        // What is compressed: the curve's threshold, ratio and knee, and the input driving into
        // them, beside the display's threshold arrow.
        dial("threshold", "Thresh", left(0), 0, false, TextFormat::Decibels, 1),
        dial("ratio", "Ratio", left(1), 0, true, TextFormat::Ratio, 1),
        dial("input", "Input", left(0), 1, false, TextFormat::Decibels, 1),
        dial("knee", "Knee", left(1), 1, false, TextFormat::Decibels, 1),
        display,
        // How it sounds: its timing and the mix above, the level out and the key's filter below.
        dial("attack", "Attack", right(0), 0, true, TextFormat::Seconds, 2),
        dial("release", "Release", right(1), 0, true, TextFormat::Seconds, 0),
        dial("mix", "Dry/Wet", right(2), 0, false, TextFormat::Percent, 0),
        dial("output", "Output", right(0), 1, false, TextFormat::Decibels, 1),
        dial("sidechainHighPass", "SC HP", right(1), 1, true, TextFormat::Frequency, 0),
    ];
    // The canvas draws every parameter, so the generated list under it draws none of them.
    let hidden_parameters = crate::INFO.params.iter().map(|p| p.id.to_string()).collect();
    DeviceUi { size: [WIDTH, COMPACT_HEIGHT], hidden_parameters, controls, ..DeviceUi::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::INFO;

    #[test]
    fn the_panel_loads_and_places_every_parameter_inside_it() {
        let loaded = layout().panel(&INFO).unwrap();
        let canvas = loaded.ui.as_ref().unwrap();
        assert_eq!(canvas.size, [WIDTH, COMPACT_HEIGHT]);
        for control in &canvas.controls {
            let [x, y, w, h] = control.bounds;
            assert!(x >= 0.0 && y >= 0.0, "{} is outside", control.id);
            assert!(x + w <= canvas.size[0] && y + h <= canvas.size[1], "{} is outside", control.id);
        }
        let bound: Vec<_> = canvas
            .controls
            .iter()
            .flat_map(|c| [c.x.as_ref(), c.y.as_ref()].into_iter().flatten())
            .map(|b| b.parameter.as_str())
            .collect();
        for parameter in INFO.params.iter() {
            assert!(bound.contains(&&*parameter.id), "{} has no control", parameter.id);
        }
        assert!(canvas.controls.iter().any(|c| matches!(c.widget, Some(Widget::CompressorDisplay))));
        assert_eq!(canvas.hidden_parameters.len(), INFO.params.len());
    }

    #[test]
    fn no_two_controls_overlap() {
        let canvas = layout().panel(&INFO).unwrap().ui.unwrap();
        for (i, a) in canvas.controls.iter().enumerate() {
            for b in canvas.controls.iter().skip(i + 1) {
                let ([ax, ay, aw, ah], [bx, by, bw, bh]) = (a.bounds, b.bounds);
                let apart = ax + aw <= bx || bx + bw <= ax || ay + ah <= by || by + bh <= ay;
                assert!(apart, "{} overlaps {}", a.id, b.id);
            }
        }
    }
}
