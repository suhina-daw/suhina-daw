// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native SDK presentation shared by the compact stock panel and expanded editor.

#[path = "editor.rs"]
mod editor;
#[path = "editor_bodies.rs"]
mod editor_bodies;

use suhina_device_sdk::{
    builtin::Layout,
    ui::{
        CustomControl, DeviceUi, Draw, ImageAsset, PAGE_ID, TextAlign, TextFormat, UiBinding, UiExpression, UiGesture,
        UiValue, Widget,
    },
};

/// The light themes' backgrounds (`devices/instrument/panel/README.md`): the compact panel's strip
/// and the expanded editor's, each with the logo at its bottom right.
const PANEL_LIGHT: &[u8] = include_bytes!("../panel/panel-light.jpg");
const EDITOR_LIGHT: &[u8] = include_bytes!("../panel/editor-light.jpg");
/// The artwork shows with a light theme only: a dark theme's light text would not read on it.
const ARTWORK_STYLE: &str = "@media (prefers-color-scheme: dark) { #artwork-light { display: none; } }";

pub(super) struct Canvas {
    ui: DeviceUi,
    page: Option<u16>,
}

impl Canvas {
    pub(super) fn new(size: [f32; 2]) -> Self {
        Self {
            ui: DeviceUi {
                size,
                viewport_size: Some(size),
                accent_on: true,
                // Everything is deliberately placed in the two canvases. Revision is metadata.
                hidden_parameters: crate::PARAMS.iter().map(|p| p.id.to_string()).collect(),
                ..DeviceUi::default()
            },
            page: None,
        }
    }

    /// The canvas's background artwork, behind every control and on every page: `bytes` as the
    /// asset `artwork-light`, shown with a light theme. An image the SDK refuses leaves the
    /// canvas without it rather than failing the panel.
    pub(super) fn artwork(&mut self, bytes: &[u8]) {
        let Ok(asset) = ImageAsset::from_bytes("artwork-light", bytes) else { return };
        self.ui.assets.push(asset);
        let [width, height] = self.ui.size;
        self.ui.controls.push(CustomControl {
            id: "artwork-light".into(),
            name: "Instrument artwork".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, width, height],
            widget: Some(Widget::Graphic { drawing: vec![Draw::image("artwork-light", [0.0, 0.0, width, height])] }),
            gesture: UiGesture::Drag,
            x: None,
            y: None,
            drawing: vec![],
            visible_when: None,
        });
        self.ui.style = Some(ARTWORK_STYLE.into());
    }

    /// More of the canvas's style sheet, after what it has.
    pub(super) fn style(&mut self, sheet: &str) {
        let style = self.ui.style.take().unwrap_or_default();
        self.ui.style = Some(if style.is_empty() { sheet.into() } else { format!("{style}\n{sheet}") });
    }

    /// A dot dragged along a drawing that sets parameter `id`, its minimum at the right of
    /// `bounds` and its maximum at the left, drawn as a short line across `span` (canvas y).
    pub(super) fn handle(
        &mut self,
        id: &str,
        name: &str,
        bounds: [f32; 4],
        span: [f32; 2],
        color: [u8; 4],
        tooltip: &str,
    ) {
        self.add(
            name,
            Widget::Handle {
                color,
                span: Some([span[0].into(), span[1].into()]),
                offset: None,
                dot: true,
                relative: false,
                hollow: false,
            },
            bounds,
            Some(id),
            UiGesture::Position,
            tooltip,
        );
        if let Some(binding) = self.ui.controls.last_mut().and_then(|control| control.x.as_mut()) {
            binding.drag_scale = -1.0;
        }
    }

    pub(super) fn page(&mut self, page: u16) {
        self.page = Some(page);
    }

    fn add(
        &mut self,
        name: &str,
        widget: Widget,
        bounds: [f32; 4],
        binding: Option<&str>,
        gesture: UiGesture,
        tooltip: &str,
    ) {
        let binding = binding.map(UiBinding::new);
        let vertical = gesture == UiGesture::Drag && binding.is_some();
        self.ui.controls.push(CustomControl {
            id: format!("control_{}", self.ui.controls.len()),
            name: name.into(),
            tooltip: tooltip.into(),
            bounds,
            widget: Some(widget),
            gesture,
            x: if vertical { None } else { binding.clone() },
            y: if vertical { binding } else { None },
            drawing: vec![],
            visible_when: self.page.map(|page| {
                UiValue::Expression(Box::new(UiExpression::Equal {
                    a: UiValue::parameter(PAGE_ID, false),
                    b: f32::from(page).into(),
                }))
            }),
        });
    }

    pub(super) fn label(&mut self, text: &str, bounds: [f32; 4]) {
        self.add(text, Widget::Label { label: text.into() }, bounds, None, UiGesture::Drag, "");
    }

    /// Model identity is read-only metadata; it never adds a sound parameter or edit action.
    pub(super) fn only_revision(&mut self, revision: u16) {
        self.when(1, &revision_is(revision));
    }

    /// The last `controls` controls are shown only for `revisions` (distinct values).
    pub(super) fn only_revisions(&mut self, revisions: &[u16], controls: usize) {
        let condition = revisions
            .iter()
            .map(|revision| revision_is(*revision))
            .reduce(|a, b| UiValue::Expression(Box::new(UiExpression::Add { a, b })));
        self.when(controls, &condition.unwrap_or_else(|| 0.0_f32.into()));
    }

    /// The last `controls` controls show only while parameter `id` has one of `values`.
    pub(super) fn only_values(&mut self, id: &str, values: &[u16], controls: usize) {
        let condition = values
            .iter()
            .map(|value| value_is(id, *value))
            .reduce(|a, b| UiValue::Expression(Box::new(UiExpression::Add { a, b })));
        self.when(controls, &condition.unwrap_or_else(|| 0.0_f32.into()));
    }

    /// A segmented button: pressing it sets parameter `id` to `value`.
    pub(super) fn choice(&mut self, id: &str, value: u16, label: &str, bounds: [f32; 4], tooltip: &str) {
        self.add(
            label,
            Widget::Button { label: label.into(), off_label: None, indicator: false },
            bounds,
            Some(id),
            UiGesture::Set(value),
            tooltip,
        );
    }

    /// A drawing, its coordinates relative to `bounds`' top left.
    pub(super) fn graphic(&mut self, name: &str, drawing: Vec<Draw>, bounds: [f32; 4]) {
        self.add(name, Widget::Graphic { drawing }, bounds, None, UiGesture::Drag, "");
    }

    /// Show the last `controls` controls only while `condition` holds, on their page.
    fn when(&mut self, controls: usize, condition: &UiValue) {
        let start = self.ui.controls.len().saturating_sub(controls);
        for control in self.ui.controls.iter_mut().skip(start) {
            control.visible_when = Some(match control.visible_when.take() {
                Some(page) => UiValue::Expression(Box::new(UiExpression::Multiply { a: page, b: condition.clone() })),
                None => condition.clone(),
            });
        }
    }

    pub(super) fn title(&mut self, text: &str, bounds: [f32; 4], size: f32) {
        self.add(
            text,
            Widget::Graphic {
                drawing: vec![Draw::Text {
                    position: [0.0.into(), 0.0.into()],
                    text: text.into(),
                    size,
                    color: [3, 1, 1, 255],
                    parameter: None,
                    decimals: 0,
                    align: TextAlign::Left,
                    format: TextFormat::Number,
                }],
            },
            bounds,
            None,
            UiGesture::Drag,
            "",
        );
    }

    pub(super) fn group(&mut self, title: &str, bounds: [f32; 4]) {
        self.add(title, Widget::Panel { label: title.into() }, bounds, None, UiGesture::Drag, "");
    }

    pub(super) fn menu(&mut self, id: &str, name: &str, bounds: [f32; 4], tooltip: &str) {
        self.add(name, Widget::Menu, bounds, Some(id), UiGesture::Menu, tooltip);
    }

    pub(super) fn toggle(&mut self, id: &str, name: &str, bounds: [f32; 4], tooltip: &str) {
        self.add(
            name,
            Widget::Button { label: name.into(), off_label: None, indicator: true },
            bounds,
            Some(id),
            UiGesture::Toggle,
            tooltip,
        );
    }

    pub(super) fn dial(&mut self, id: &str, name: &str, bounds: [f32; 4], tooltip: &str) {
        let format = match id {
            "gain" => TextFormat::Decibels,
            "body_decay" | "release" | "attack" | "decay" => TextFormat::Seconds,
            "structure" => TextFormat::Number,
            _ => TextFormat::Percent,
        };
        self.add(
            name,
            Widget::Dial { label: name.into(), decimals: if id == "structure" { 4 } else { 2 }, format, image: None },
            bounds,
            Some(id),
            UiGesture::Drag,
            tooltip,
        );
        if let Some(control) = self.ui.controls.last_mut()
            && let Some(binding) = control.y.as_mut()
        {
            binding.logarithmic = id == "body_decay";
        }
    }

    pub(super) fn number(&mut self, id: &str, name: &str, x: f32, y: f32, w: f32) {
        self.label(name, [x, y, w, 18.0]);
        let integer = matches!(id, "root" | "key_low" | "key_high");
        self.add(
            name,
            Widget::Number {
                decimals: if integer { 0 } else { 2 },
                format: if integer { TextFormat::Key } else { TextFormat::Percent },
                color: [3, 1, 5, 255],
                center: false,
                bordered: true,
                prefix: String::new(),
                suffix: String::new(),
            },
            [x, y + 22.0, w, 28.0],
            Some(id),
            UiGesture::Drag,
            "Drag vertically; Shift for precision; double click resets",
        );
    }

    pub(super) fn navigate(&mut self, name: &str, bounds: [f32; 4], page: u16, expand: bool) {
        self.add(
            name,
            Widget::Button { label: name.into(), off_label: None, indicator: false },
            bounds,
            Some(PAGE_ID),
            if expand { UiGesture::Expand(page) } else { UiGesture::Set(page) },
            if expand {
                "Open the expanded instrument editor"
            } else {
                "Show this editor page; sound and undo history stay unchanged"
            },
        );
        if !expand && let Some(control) = self.ui.controls.last_mut() {
            control.name = format!("{name} page");
        }
    }

    pub(super) fn waveform(&mut self, bounds: [f32; 4]) {
        // Suppress the sampler's legacy recording strip; this device imports local files.
        self.add(
            "Sample recording unavailable",
            Widget::SampleRecording,
            [bounds[0], bounds[1], 8.0, 8.0],
            None,
            UiGesture::Drag,
            "",
        );
        if let Some(control) = self.ui.controls.last_mut() {
            control.visible_when = Some(0.0.into());
        }
        self.add(
            "Local sample waveform",
            Widget::SampleWaveform,
            bounds,
            None,
            UiGesture::Position,
            "Drop a local audio file here; drag the source or loop boundaries",
        );
    }

    pub(super) fn finish(self) -> DeviceUi {
        self.ui
    }
}

fn revision_is(revision: u16) -> UiValue {
    value_is("model_revision", revision)
}

fn value_is(id: &str, value: u16) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter(id, false), b: f32::from(value).into() }))
}

/// Two presentations of the same native parameter set, without a second sound-state path.
pub fn layout() -> Layout {
    let mut panel = Canvas::new([620.0, 168.0]);
    panel.artwork(PANEL_LIGHT);
    panel.label("STRUCK MODAL · experimental v1", [10.0, 4.0, 248.0, 20.0]);
    panel.only_revision(1);
    panel.label("MODAL PROFILE · experimental v2", [10.0, 4.0, 248.0, 20.0]);
    panel.only_revision(2);
    panel.label("MODAL BODIES · experimental v3", [10.0, 4.0, 248.0, 20.0]);
    panel.only_revision(3);
    panel.label("FITTED MODEL · experimental v4", [10.0, 4.0, 248.0, 20.0]);
    panel.only_revision(4);
    panel.menu("source", "Source", [272.0, 4.0, 136.0, 24.0], "Modal, local Sample, or both in Hybrid");
    panel.navigate("Open editor", [456.0, 4.0, 152.0, 24.0], 0, true);
    panel.waveform([10.0, 36.0, 244.0, 88.0]);
    panel.label("Drop a local sample to play it", [10.0, 132.0, 244.0, 20.0]);
    for (i, (id, name, hint)) in [
        ("body_decay", "Decay", "Modal: time to decay by 60 dB; next strike"),
        ("brightness", "Brightness", "Modal: energy in higher resonances; next strike"),
        ("release", "Release", "Release damping for the modal body and sample gate"),
        ("blend", "Blend", "Hybrid only: amount of local sample in the mix"),
        ("gain", "Gain", "Output gain for both sources"),
    ]
    .into_iter()
    .enumerate()
    {
        panel.dial(id, name, [270.0 + i as f32 * 68.0, 44.0, 64.0, 88.0], hint);
    }
    // Clear of the artwork's logo at the bottom right.
    panel.label("Model and sample details in the editor", [270.0, 140.0, 228.0, 18.0]);
    Layout { ui: Some(panel.finish()), expanded: Some(editor::canvas()), ..Layout::default() }
}
