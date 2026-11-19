// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A compact stock panel composed exclusively of device-SDK controls.

use suhina_device_sdk::{
    builtin::Layout,
    ui::{CustomControl, DeviceUi, TextFormat, UiBinding, UiGesture, UiValue, Widget},
};

fn control(ui: &mut DeviceUi, id: &str, widget: Widget, bounds: [f32; 4], gesture: UiGesture) {
    ui.controls.push(CustomControl {
        id: format!("{id}_{}", ui.controls.len()),
        name: id.replace('_', " "),
        tooltip: id.replace('_', " "),
        bounds,
        widget: Some(widget),
        gesture,
        x: Some(UiBinding::new(id)),
        y: None,
        drawing: vec![],
        visible_when: None,
    });
}

fn label(ui: &mut DeviceUi, name: &str, bounds: [f32; 4]) {
    control(ui, name, Widget::Label { label: name.into() }, bounds, UiGesture::Drag);
    if let Some(control) = ui.controls.last_mut() {
        control.x = None;
    }
}

/// All sound controls remain native parameters; the panel only presents them.
pub fn layout() -> Layout {
    let mut ui = DeviceUi { size: [900.0, 168.0], viewport_size: Some([900.0, 168.0]), ..DeviceUi::default() };
    label(&mut ui, "EXPERIMENTAL · struck modal v1", [8.0, 2.0, 260.0, 20.0]);
    control(&mut ui, "source", Widget::Menu, [278.0, 2.0, 130.0, 20.0], UiGesture::Menu);
    control(&mut ui, "quality", Widget::Menu, [418.0, 2.0, 70.0, 20.0], UiGesture::Menu);
    label(&mut ui, "modes / voice", [496.0, 2.0, 105.0, 20.0]);
    // The shared waveform handles local drag/drop and source boundaries. Recording is not
    // offered by this prototype; this SDK marker disables its legacy inline recording strip.
    control(&mut ui, "recording", Widget::SampleRecording, [8.0, 28.0, 200.0, 18.0], UiGesture::Drag);
    if let Some(control) = ui.controls.last_mut() {
        control.x = None;
        control.visible_when = Some(UiValue::from(0.0));
    }
    control(&mut ui, "sample", Widget::SampleWaveform, [8.0, 28.0, 412.0, 76.0], UiGesture::Position);
    if let Some(control) = ui.controls.last_mut() {
        control.x = None;
    }
    for (i, (id, name, format)) in [
        ("body_decay", "Decay", TextFormat::Seconds),
        ("brightness", "Bright", TextFormat::Percent),
        ("structure", "Structure", TextFormat::Number),
        ("strike", "Strike", TextFormat::Percent),
        ("blend", "Sample", TextFormat::Percent),
        ("gain", "Gain", TextFormat::Decibels),
    ]
    .into_iter()
    .enumerate()
    {
        control(
            &mut ui,
            id,
            Widget::Dial { label: name.into(), decimals: 2, format, image: None },
            [434.0 + i as f32 * 76.0, 30.0, 68.0, 72.0],
            UiGesture::Drag,
        );
        if let Some(control) = ui.controls.last_mut() {
            control.y = control.x.take();
        }
    }
    for (i, (id, name)) in [
        ("root", "Root"),
        ("key_low", "Low"),
        ("key_high", "High"),
        ("attack", "Attack"),
        ("release", "Release"),
        ("sustain", "Sustain"),
        ("loop_start", "Loop start"),
        ("loop_end", "Loop end"),
    ]
    .into_iter()
    .enumerate()
    {
        label(&mut ui, name, [8.0 + i as f32 * 110.0, 112.0, 100.0, 18.0]);
        control(
            &mut ui,
            id,
            Widget::Number {
                decimals: if i < 3 { 0 } else { 2 },
                format: if matches!(id, "attack" | "release") { TextFormat::Seconds } else { TextFormat::Number },
                color: [166, 163, 255, 255],
                center: false,
                bordered: true,
                prefix: String::new(),
                suffix: String::new(),
            },
            [8.0 + i as f32 * 110.0, 132.0, 100.0, 24.0],
            UiGesture::Drag,
        );
        if let Some(control) = ui.controls.last_mut() {
            control.y = control.x.take();
        }
    }
    control(&mut ui, "playback", Widget::Menu, [620.0, 2.0, 160.0, 20.0], UiGesture::Menu);
    ui.hidden_parameters = [
        "source",
        "model_revision",
        "quality",
        "body_decay",
        "brightness",
        "structure",
        "strike",
        "blend",
        "gain",
        "root",
        "key_low",
        "key_high",
        "attack",
        "release",
        "sustain",
        "loop_start",
        "loop_end",
        "playback",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    Layout { ui: Some(ui), ..Layout::default() }
}
