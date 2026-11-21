// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native SDK presentation shared by the compact stock panel and expanded editor.

#[path = "editor.rs"]
pub(crate) mod editor;
#[path = "editor_bodies.rs"]
mod editor_bodies;
#[path = "editor_pages.rs"]
mod editor_pages;

use suhina_device_sdk::{
    builtin::Layout,
    ui::{
        CustomControl, DeviceUi, Draw, ImageAsset, PAGE_ID, TextAlign, TextFormat, UiBinding, UiExpression, UiGesture,
        UiValue, Widget,
    },
};

/// The background (`devices/instrument/panel/README.md`): the expanded editor's surface, of
/// which the compact panel shows a strip ([`COMPACT_CROP`]).
const EDITOR_LIGHT: &[u8] = include_bytes!("../panel/editor-surface.jpg");
/// The part of the editor's surface behind the compact panel, as shares of the image's axes
/// (left, top, right, bottom): 620 × 168 of its 880 × 560 points, a little below its middle.
const COMPACT_CROP: [f32; 4] = [130.0 / 880.0, 196.0 / 560.0, 750.0 / 880.0, 364.0 / 560.0];
/// The logo, and the dials' machined faces, large and small.
const LOGO: &[u8] = include_bytes!("../panel/logo.png");
const KNOB_LARGE: &[u8] = include_bytes!("../panel/knob-large.png");
const KNOB_SMALL: &[u8] = include_bytes!("../panel/knob-small.png");
/// Both canvases look the same with every theme: their own sheet gives every control espresso
/// ink and brass over the ivory artwork, so a dark theme's light text never lands on it.
const ARTWORK_STYLE: &str = "device-section { background-color: transparent; color: #6b5543;
  border-color: rgba(52, 38, 29, 0.22); border-width: 1px 0 0 0; radius: 0; box-shadow: none; }
button { image: none; background-color: transparent; color: #6b5543; border-color: rgba(52, 38, 29, 0.35);
  border-width: 0 0 1px 0; radius: 0; box-shadow: none; accent-color: #b8742f; }
button:hover { color: #34261d; }
button[checked] { background-color: transparent; color: #34261d; border-color: #b8742f; }
dropdown { background-color: transparent; color: #34261d; border-color: rgba(52, 38, 29, 0.35);
  border-width: 0 0 1px 0; radius: 0; }
knob { color: #34261d; accent-color: #b8742f; }
knob-ring { background-color: rgba(52, 38, 29, 0.14); }
knob-pointer { color: #2a1c13; }
label { color: #6b5543; }
value { color: #34261d; accent-color: #8a5422; border-color: rgba(52, 38, 29, 0.3); background-color: transparent; }
waveform { color: #34261d; accent-color: #b8742f; background-color: rgba(52, 38, 29, 0.05);
  border-color: rgba(52, 38, 29, 0.22); }
device-control { color: #34261d; accent-color: #b8742f; }
popup { background-color: #efe4d2; color: #34261d; border-color: rgba(52, 38, 29, 0.3); }";
/// Espresso ink and the dark brass of values, for drawings that must not follow the theme.
pub(super) const INK: [u8; 4] = [52, 38, 29, 255];
pub(super) const VALUE: [u8; 4] = [138, 84, 34, 255];
/// Fainter espresso text, for captions and the lines on a control.
pub(super) const MUTED: [u8; 4] = [107, 85, 67, 255];
/// The brass of the markers dragged on the pages.
pub(super) const BRASS: [u8; 4] = [168, 100, 31, 255];
/// A hairline across a page.
pub(super) const HAIRLINE: [u8; 4] = [52, 38, 29, 56];
/// How much of the artwork a dimmed control's veil lets through: the control keeps about 45 %
/// of its strength.
const VEIL_ALPHA: u8 = 140;

pub(super) struct Canvas {
    ui: DeviceUi,
    /// While which pages the controls added now show: none for every page.
    page: Option<UiValue>,
    /// The share of the artwork image behind the canvas, once it has the artwork.
    artwork_uv: Option<[f32; 4]>,
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
            artwork_uv: None,
        }
    }

    /// The canvas's background artwork, behind every control and on every page, with every
    /// theme: `bytes` as the asset `artwork-light`, and the sheet that colors the controls for
    /// it. An image the SDK refuses leaves the canvas without it rather than failing the panel.
    pub(super) fn artwork(&mut self, bytes: &[u8]) {
        self.artwork_cropped(bytes, [0.0, 0.0, 1.0, 1.0]);
    }

    /// [`Self::artwork`] showing only `uv` of the image (left, top, right, bottom, as shares of
    /// its axes), stretched over the canvas.
    pub(super) fn artwork_cropped(&mut self, bytes: &[u8], uv: [f32; 4]) {
        let Ok(asset) = ImageAsset::from_bytes("artwork-light", bytes) else { return };
        self.ui.assets.push(asset);
        let [width, height] = self.ui.size;
        let mut image = Draw::image("artwork-light", [0.0, 0.0, width, height]);
        if let Draw::Image { uv: crop, .. } = &mut image {
            *crop = uv;
        }
        self.artwork_uv = Some(uv);
        self.ui.controls.push(CustomControl {
            id: "artwork-light".into(),
            name: "Instrument artwork".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, width, height],
            widget: Some(Widget::Graphic { drawing: vec![image] }),
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

    pub(super) fn page(&mut self, page: u16) {
        self.page = Some(page_is(page));
    }

    /// The controls added from now on show on any of `pages`.
    pub(super) fn pages(&mut self, pages: &[u16]) {
        self.page = Some(any_of(PAGE_ID, pages));
    }

    /// The controls added from now on show on every page.
    pub(super) fn every_page(&mut self) {
        self.page = None;
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
            visible_when: self.page.clone(),
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

    /// Show the last `controls` controls only while `condition` holds too.
    pub(super) fn also_when(&mut self, controls: usize, condition: &UiValue) {
        self.when(controls, condition);
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
            "root" => TextFormat::Key,
            "body_decay" | "release" | "attack" | "decay" => TextFormat::Seconds,
            "structure" => TextFormat::Number,
            _ => TextFormat::Percent,
        };
        let decimals = match id {
            "structure" => 4,
            "root" => 0,
            _ => 2,
        };
        self.add(
            name,
            Widget::Dial { label: name.into(), decimals, format, image: None },
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

    /// A value written as text, centered in `bounds`, that vertical drags change.
    pub(super) fn readout(&mut self, id: &str, name: &str, bounds: [f32; 4], format: TextFormat, decimals: usize) {
        self.add(
            name,
            Widget::Number {
                decimals,
                format,
                color: VALUE,
                center: true,
                bordered: false,
                prefix: String::new(),
                suffix: String::new(),
            },
            bounds,
            Some(id),
            UiGesture::Drag,
            "Drag vertically; Shift for precision; double click resets",
        );
    }

    /// The compact panel's way into the editor while the editor's page is `page`: it opens the
    /// editor on that page, so the editor keeps the page it showed last. There is one for each
    /// page, each shown only while its page is the current one: one button, seen from outside.
    pub(super) fn open_editor(&mut self, page: u16, bounds: [f32; 4]) {
        self.add(
            "Open editor",
            Widget::Button { label: "EDITOR ›".into(), off_label: None, indicator: false },
            bounds,
            Some(PAGE_ID),
            UiGesture::Expand(page),
            "Open the expanded instrument editor on the page it showed last",
        );
        self.show_when(page_is(page));
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

    /// An image asset of the canvas, under `id`. An image the SDK refuses is left out.
    pub(super) fn asset(&mut self, id: &str, bytes: &[u8]) {
        if let Ok(asset) = ImageAsset::from_bytes(id, bytes) {
            self.ui.assets.push(asset);
        }
    }

    /// Shows the last control only while `condition` holds, whatever its page said.
    pub(super) fn show_when(&mut self, condition: UiValue) {
        if let Some(control) = self.ui.controls.last_mut() {
            control.visible_when = Some(condition);
        }
    }

    /// Text drawn as a graphic: `align` against `bounds`' left or center, in `color`.
    pub(super) fn text(
        &mut self,
        name: &str,
        text: &str,
        bounds: [f32; 4],
        size: f32,
        align: TextAlign,
        color: [u8; 4],
    ) {
        let x = if matches!(align, TextAlign::Center) { bounds[2] / 2.0 } else { 0.0 };
        self.add(
            name,
            Widget::Graphic {
                drawing: vec![Draw::Text {
                    position: [x.into(), 0.0.into()],
                    text: text.into(),
                    size,
                    color,
                    parameter: None,
                    decimals: 0,
                    align,
                    format: TextFormat::Number,
                }],
            },
            bounds,
            None,
            UiGesture::Drag,
            "",
        );
    }

    /// A drawing over the whole canvas that takes no input, its drawings in canvas points.
    pub(super) fn graphic(&mut self, name: &str, drawing: Vec<Draw>) {
        let [width, height] = self.ui.size;
        self.add(name, Widget::Graphic { drawing }, [0.0, 0.0, width, height], None, UiGesture::Drag, "");
    }

    /// Dims the canvas's `rects` while `condition` holds (and their page shows): the artwork
    /// drawn over them at [`VEIL_ALPHA`], so the controls under them read faded, as controls
    /// that have no effect in the current Source do. The veil takes no input, so they still take
    /// edits, and their values can be prepared before Source changes. Without the artwork there
    /// is no veil.
    pub(super) fn dim(&mut self, name: &str, rects: &[[f32; 4]], condition: &UiValue) {
        let Some([u0, v0, u1, v1]) = self.artwork_uv else { return };
        let [width, height] = self.ui.size;
        let u = |x: f32| u0 + (u1 - u0) * x / width;
        let v = |y: f32| v0 + (v1 - v0) * y / height;
        let drawing = rects
            .iter()
            .map(|&[x, y, w, h]| {
                let mut image = Draw::image("artwork-light", [x, y, w, h]);
                if let Draw::Image { uv, tint, .. } = &mut image {
                    *uv = [u(x), v(y), u(x + w), v(y + h)];
                    *tint = [255, 255, 255, VEIL_ALPHA];
                }
                image
            })
            .collect();
        self.graphic(name, drawing);
        self.when(1, condition);
    }

    /// A marker dragged directly ([`Widget::Handle`]): with both `x` and `y` a dot at both
    /// values, with one a line across `bounds`, or a dot in its middle with `dot`. `x` names its
    /// parameter and whether it maps logarithmically; `offset` moves `bounds` sideways.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle(
        &mut self,
        name: &str,
        bounds: [f32; 4],
        x: Option<(&str, bool)>,
        y: Option<&str>,
        offset: Option<UiValue>,
        dot: bool,
        tooltip: &str,
    ) {
        self.ui.controls.push(CustomControl {
            id: format!("control_{}", self.ui.controls.len()),
            name: name.into(),
            tooltip: tooltip.into(),
            bounds,
            widget: Some(Widget::Handle { color: BRASS, span: None, offset, dot, relative: false, hollow: false }),
            gesture: UiGesture::Position,
            x: x.map(|(id, logarithmic)| UiBinding { logarithmic, ..UiBinding::new(id) }),
            y: y.map(UiBinding::new),
            drawing: vec![],
            visible_when: self.page.clone(),
        });
    }

    /// A horizontal slider for `id` across `bounds`, drawn by the canvas itself: it takes the
    /// pointer anywhere along `bounds` and moves the value to it ([`Widget::Handle`], unseen).
    pub(super) fn slider(&mut self, id: &str, name: &str, bounds: [f32; 4], tooltip: &str) {
        self.ui.controls.push(CustomControl {
            id: format!("control_{}", self.ui.controls.len()),
            name: name.into(),
            tooltip: tooltip.into(),
            bounds,
            widget: Some(Widget::Handle {
                color: [0, 0, 0, 0],
                span: None,
                offset: None,
                dot: false,
                relative: false,
                hollow: false,
            }),
            gesture: UiGesture::Position,
            x: Some(UiBinding::new(id)),
            y: None,
            drawing: vec![],
            visible_when: self.page.clone(),
        });
    }

    /// A button setting `id` to `value`, named `name` (its accessible name) and showing `label`.
    pub(super) fn choice_labelled(
        &mut self,
        id: &str,
        value: u16,
        name: &str,
        label: &str,
        bounds: [f32; 4],
        tooltip: &str,
    ) {
        self.add(
            name,
            Widget::Button { label: label.into(), off_label: None, indicator: false },
            bounds,
            Some(id),
            UiGesture::Set(value),
            tooltip,
        );
    }

    /// A ringed dial with the canvas's image asset `face` as its knob.
    pub(super) fn face_dial(&mut self, id: &str, name: &str, bounds: [f32; 4], tooltip: &str, face: &str) {
        self.dial(id, name, bounds, tooltip);
        if let Some(control) = self.ui.controls.last_mut()
            && let Some(Widget::Dial { image, .. }) = control.widget.as_mut()
        {
            *image = Some(face.into());
        }
    }

    /// A control drawn entirely by `drawing` that sets `id` by vertical drags, as a dial does.
    pub(super) fn drawn_dial(&mut self, id: &str, name: &str, bounds: [f32; 4], drawing: Vec<Draw>, tooltip: &str) {
        self.ui.controls.push(CustomControl {
            id: format!("control_{}", self.ui.controls.len()),
            name: name.into(),
            tooltip: tooltip.into(),
            bounds,
            widget: None,
            gesture: UiGesture::Drag,
            x: None,
            y: Some(UiBinding::new(id)),
            drawing,
            visible_when: self.page.clone(),
        });
    }

    pub(super) fn finish(self) -> DeviceUi {
        self.ui
    }
}

/// While the editor shows page `page`.
pub(super) fn page_is(page: u16) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal {
        a: UiValue::parameter(PAGE_ID, false),
        b: f32::from(page).into(),
    }))
}

/// While parameter `id` has one of `values`.
pub(super) fn any_of(id: &str, values: &[u16]) -> UiValue {
    values
        .iter()
        .map(|value| value_is(id, *value))
        .reduce(|a, b| UiValue::Expression(Box::new(UiExpression::Add { a, b })))
        .unwrap_or_else(|| 0.0_f32.into())
}

/// Both conditions.
pub(super) fn both(a: UiValue, b: UiValue) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Multiply { a, b }))
}

/// The opposite of a condition that is 0 or 1.
pub(super) fn not(condition: UiValue) -> UiValue {
    UiValue::from(1.0) + condition * UiValue::from(-1.0)
}

/// The sample layer's controls (Root, the source's region and loop, Playback, the waveform) have
/// no effect in Modal, which plays no sample.
pub(super) fn sample_silent() -> UiValue {
    value_is("source", 0)
}

/// The amplitude envelope (Attack, Decay, Sustain) shapes the sample layer and fitted models
/// (revision 4): it has no effect in Modal with any other revision.
pub(super) fn envelope_silent() -> UiValue {
    both(value_is("source", 0), not(revision_is(4)))
}

/// The modal body's controls have no effect in Sample, which plays no modal body.
pub(super) fn modal_silent() -> UiValue {
    value_is("source", 1)
}

/// The Hybrid blend has no effect outside Hybrid.
pub(super) fn blend_silent() -> UiValue {
    not(value_is("source", 2))
}

fn revision_is(revision: u16) -> UiValue {
    value_is("model_revision", revision)
}

pub(super) fn value_is(id: &str, value: u16) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Equal { a: UiValue::parameter(id, false), b: f32::from(value).into() }))
}

/// Letters spread apart, the way the pages' names and captions are set.
pub(super) fn spaced(text: &str) -> String {
    let mut out = String::new();
    for (i, c) in text.to_uppercase().chars().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

pub(super) fn add(a: UiValue, b: UiValue) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Add { a, b }))
}

pub(super) fn times(a: UiValue, b: f32) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Multiply { a, b: b.into() }))
}

pub(super) fn line(from: [f32; 2], to: [f32; 2], color: [u8; 4], width: f32) -> Draw {
    Draw::Line { from: [from[0].into(), from[1].into()], to: [to[0].into(), to[1].into()], color, width }
}

pub(super) fn text(position: [f32; 2], text: &str, size: f32, align: TextAlign, color: [u8; 4]) -> Draw {
    Draw::Text {
        position: [position[0].into(), position[1].into()],
        text: text.into(),
        size,
        color,
        parameter: None,
        decimals: 0,
        align,
        format: TextFormat::Number,
    }
}

/// `prefix` and then parameter `id`'s value in `format`, at `position`.
pub(super) fn value_text(
    position: [UiValue; 2],
    prefix: &str,
    id: &str,
    format: TextFormat,
    size: f32,
    align: TextAlign,
    color: [u8; 4],
) -> Draw {
    Draw::Text {
        position,
        text: prefix.into(),
        size,
        color,
        parameter: Some(id.into()),
        decimals: if format == TextFormat::Percent { 1 } else { 2 },
        align,
        format,
    }
}

/// The revisions whose compact panel names a body.
const BODY_REVISIONS: &[u16] = &[1, 3];

/// Two presentations of the same native parameter set, without a second sound-state path.
pub fn layout() -> Layout {
    Layout { ui: Some(compact()), expanded: Some(editor::canvas()), ..Layout::default() }
}

/// The compact panel (620 × 168): the logo and the body at the top left, the two layers and the
/// way into the editor at the right; the body's sound (or the sample's waveform, or both with both
/// layers on) at the left and five dials at the right; the model's name under them.
fn compact() -> DeviceUi {
    let mut panel = Canvas::new([620.0, 168.0]);
    panel.artwork_cropped(EDITOR_LIGHT, COMPACT_CROP);
    panel.asset("logo", LOGO);
    panel.asset("knob-small", KNOB_SMALL);
    panel.add(
        "Suhina",
        Widget::Graphic { drawing: vec![Draw::image("logo", [0.0, 0.0, 54.0, 15.0])] },
        [14.0, 11.0, 54.0, 15.0],
        None,
        UiGesture::Drag,
        "",
    );
    panel.menu(
        "body",
        "Body",
        [84.0, 6.0, 124.0, 22.0],
        "The body Modal plays: the abstract struck body, a struck bar or a plucked tine",
    );
    panel.only_revisions(BODY_REVISIONS, 1);
    // The two layers, each a lamp and its name that turn it on or off (Source); the one layer
    // left on stays on.
    for (name, x, w, other) in [("Body", 352.0, 54.0, 1_u16), ("Sample", 424.0, 72.0, 0)] {
        let layer = name.to_lowercase();
        let center = [(x + 6.0).into(), 17.0.into()];
        panel.graphic(
            &format!("{name} lamp lit"),
            vec![Draw::Circle { center: center.clone(), radius: 3.5.into(), color: BRASS }],
        );
        panel.show_when(not(value_is("source", other)));
        panel.graphic(
            &format!("{name} lamp unlit"),
            vec![Draw::Arc {
                center,
                radius: 3.5.into(),
                start: 0.0.into(),
                end: std::f32::consts::TAU.into(),
                color: MUTED,
                width: 1.2,
            }],
        );
        panel.show_when(value_is("source", other));
        let bounds = [x + 14.0, 8.0, w - 14.0, 18.0];
        let label = spaced(name);
        panel.choice_labelled(
            "source",
            other,
            &format!("Turn off the {layer} layer"),
            &label,
            bounds,
            "Turn this layer off",
        );
        panel.show_when(value_is("source", 2));
        panel.choice_labelled("source", 2, &format!("Turn on the {layer} layer"), &label, bounds, "Turn this layer on");
        panel.show_when(value_is("source", other));
        panel.text(&format!("{name} layer alone"), &label, [x + 14.0, 12.0, w - 14.0, 12.0], 8.0, TextAlign::Left, INK);
        panel.show_when(value_is("source", 1 - other));
    }
    for page in 0..=editor::LAST_PAGE {
        panel.open_editor(page, [540.0, 6.0, 66.0, 22.0]);
    }
    panel.graphic(
        "Dividers",
        vec![line([14.0, 34.5], [606.0, 34.5], HAIRLINE, 1.0), line([258.5, 46.0], [258.5, 152.0], HAIRLINE, 1.0)],
    );

    // The body's sound with Modal and Hybrid (fainter with Hybrid, under the waveform), the
    // sample's waveform with Sample and Hybrid.
    let presence = value_is("source", 0) + times(value_is("source", 2), 0.55);
    panel.add(
        "Sound",
        Widget::Strands {
            modes: (0..5).map(|n| UiValue::parameter(format!("#r{n}"), false)).collect(),
            since: UiValue::parameter("#r5", false),
            velocity: UiValue::parameter("#r6", false),
            height: UiValue::normalized("stereo_width", false),
            thickness: UiValue::normalized("mallet_weight", false),
            speed: UiValue::normalized("mallet_hardness", false),
            presence,
            light: [255, 249, 237, 255],
            mid: [222, 189, 135, 255],
            shadow: [128, 87, 54, 255],
            thread: [199, 128, 51, 255],
        },
        [14.0, 40.0, 230.0, 116.0],
        None,
        UiGesture::Drag,
        "",
    );
    panel.show_when(not(modal_silent()));
    panel.waveform([14.0, 44.0, 230.0, 100.0]);
    panel.show_when(not(sample_silent()));
    panel.graphic(
        "Sample root",
        vec![value_text([14.0.into(), 150.0.into()], "ROOT  ", "root", TextFormat::Key, 8.5, TextAlign::Left, MUTED)],
    );
    panel.show_when(not(sample_silent()));

    // Five dials: the body's Decay and Brightness, Release, Blend and Gain.
    let step = 340.0 / 5.0;
    let at = |n: usize| [266.0 + step * (n as f32 + 0.5) - 28.0, 42.0, 56.0, 104.0];
    for (n, (id, name, tooltip)) in [
        ("body_decay", "Decay", "The body layer: how long it rings; next strike"),
        ("brightness", "Brightness", "The body layer: energy in the higher resonances; next strike"),
        ("release", "Release", "After note-off, for both layers"),
        ("blend", "Blend", "With both layers on: from the body to the sample"),
        ("gain", "Gain", "Output gain for both layers"),
    ]
    .into_iter()
    .enumerate()
    {
        panel.face_dial(id, name, at(n), tooltip, "knob-small");
    }
    // Faded while they have no effect: the body's dials with the body layer off; Blend with one
    // layer on.
    panel.dim("Modal dimmed", &[at(0), at(1)], &modal_silent());
    panel.dim("Blend dimmed", &[at(3)], &blend_silent());

    for (revision, name) in [
        (1, "STRUCK MODAL · EXPERIMENTAL"),
        (2, "MODAL PROFILE · EXPERIMENTAL"),
        (3, "MODAL BODIES · EXPERIMENTAL"),
        (4, "FITTED MODEL · EXPERIMENTAL"),
    ] {
        panel.text(name, name, [266.0, 152.0, 340.0, 12.0], 8.0, TextAlign::Center, MUTED);
        panel.only_revision(revision);
    }
    panel.finish()
}
