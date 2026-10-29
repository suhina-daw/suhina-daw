// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Data-only drawing programs, image resources and input bindings for custom panels.
use crate::{DeviceSdkError, Parameter};
mod author;
mod widgets;
pub use author::ControlGroup;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Cursor, path::Path};
pub use widgets::Widget;

pub const MAX_ASSET_BYTES: usize = 512 * 1024;
pub const MAX_ASSETS_BYTES: usize = 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 1_000_000;
pub const MAX_DRAW_COMMANDS: usize = 1024;
type Color = [u8; 4];
fn white() -> Color {
    [255; 4]
}
fn one() -> f32 {
    1.0
}
fn uv_default() -> [f32; 4] {
    [0.0, 0.0, 1.0, 1.0]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeviceUi {
    /// Canvas width and height in logical pixels; defaults to 640 × 240.
    pub size: [f32; 2],
    /// Preferred scrollable body width and height; None preserves automatic host sizing.
    pub viewport_size: Option<[f32; 2]>,
    pub background: Color,
    pub assets: Vec<ImageAsset>,
    pub layers: Vec<Draw>,
    pub controls: Vec<CustomControl>,
    pub hidden_parameters: Vec<String>,
    /// The panel's own style sheet (version 6): presentation only, applied above the
    /// application theme and below the user's overrides, and only inside this panel. `#id`
    /// selects a control by its id. Parts it leaves out keep the application's theme.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

/// Longest panel style sheet, in bytes.
pub const MAX_STYLE_BYTES: usize = 64 * 1024;

/// Host theme colours used to resolve the SDK's built-in widget drawings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetPalette {
    pub text: [u8; 4],
    pub panel: [u8; 4],
    pub control: [u8; 4],
    pub border: [u8; 4],
    pub accent: [u8; 4],
    pub accent_text: [u8; 4],
}
impl Default for WidgetPalette {
    fn default() -> Self {
        Self {
            text: [236, 232, 225, 255],
            panel: [37, 36, 33, 255],
            control: [46, 45, 42, 255],
            border: [56, 55, 51, 255],
            accent: [166, 163, 255, 255],
            accent_text: [29, 28, 26, 255],
        }
    }
}
impl Default for DeviceUi {
    fn default() -> Self {
        Self {
            size: [640.0, 240.0],
            viewport_size: None,
            background: [37, 36, 33, 255],
            assets: Vec::new(),
            layers: Vec::new(),
            controls: Vec::new(),
            hidden_parameters: Vec::new(),
            style: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAsset {
    pub id: String,
    pub data: String,
}
impl ImageAsset {
    /// Authoring helper: embeds a PNG/JPEG rather than retaining its file path.
    pub fn from_file(id: impl Into<String>, path: &Path) -> Result<Self, DeviceSdkError> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| DeviceSdkError(format!("image {}: {e}", path.display())))?;
        let mut bytes = Vec::new();
        file.take((MAX_ASSET_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(|e| DeviceSdkError(e.to_string()))?;
        Self::from_bytes(id, &bytes)
    }
    pub fn from_bytes(id: impl Into<String>, bytes: &[u8]) -> Result<Self, DeviceSdkError> {
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(DeviceSdkError("image exceeds 512 KiB".into()));
        }
        let asset = Self { id: id.into(), data: STANDARD.encode(bytes) };
        asset.dimensions()?;
        Ok(asset)
    }
    fn reader(&self) -> Result<image::ImageReader<Cursor<Vec<u8>>>, DeviceSdkError> {
        if self.data.len() > MAX_ASSET_BYTES.div_ceil(3) * 4 {
            return Err(DeviceSdkError(format!("image {:?} exceeds 512 KiB", self.id)));
        }
        let bytes = STANDARD.decode(&self.data).map_err(|e| DeviceSdkError(format!("image {:?}: {e}", self.id)))?;
        if bytes.len() > MAX_ASSET_BYTES {
            return Err(DeviceSdkError("decoded asset bytes exceed 512 KiB".into()));
        }
        let mut reader = image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| DeviceSdkError(e.to_string()))?;
        if !matches!(reader.format(), Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)) {
            return Err(DeviceSdkError("UI images must be PNG or JPEG".into()));
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(16 * 1024 * 1024);
        reader.limits(limits);
        Ok(reader)
    }
    pub fn dimensions(&self) -> Result<[u32; 2], DeviceSdkError> {
        let (w, h) =
            self.reader()?.into_dimensions().map_err(|e| DeviceSdkError(format!("image {:?}: {e}", self.id)))?;
        if w == 0 || h == 0 || w > 4096 || h > 4096 || u64::from(w) * u64::from(h) > MAX_IMAGE_PIXELS {
            return Err(DeviceSdkError(format!("image {:?} exceeds dimension/pixel limits", self.id)));
        }
        Ok([w, h])
    }
    /// Decode with limits off the audio thread; callers cache these pixels/textures.
    pub fn decode_rgba(&self) -> Result<(Vec<u8>, [usize; 2]), DeviceSdkError> {
        let [w, h] = self.dimensions()?;
        let rgba = self.reader()?.decode().map_err(|e| DeviceSdkError(format!("image {:?}: {e}", self.id)))?.to_rgba8();
        Ok((rgba.into_raw(), [w as usize, h as usize]))
    }
}

/// Shared oscillator preview for waveform menus and drawing expressions.
pub fn waveform(shape: usize, cycles: f64) -> f64 {
    if let Some(shape) = suhina_dsp::lfo::Shape::ALL.get(shape) {
        suhina_dsp::lfo::wave(*shape, cycles)
    } else {
        (cycles * 8.).sin() * 0.6 + (cycles * 3.).cos() * 0.4
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UiValue {
    Number(f32),
    Expression(Box<UiExpression>),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum UiExpression {
    Parameter {
        parameter: String,
        #[serde(default)]
        normalized: bool,
        #[serde(default)]
        logarithmic: bool,
    },
    Add {
        a: UiValue,
        b: UiValue,
    },
    Multiply {
        a: UiValue,
        b: UiValue,
    },
    Equal {
        a: UiValue,
        b: UiValue,
    },
    Exp {
        value: UiValue,
    },
    Log {
        value: UiValue,
    },
    Sin {
        value: UiValue,
    },
    Cos {
        value: UiValue,
    },
    Wave {
        shape: UiValue,
        cycles: UiValue,
    },
}
impl From<f32> for UiValue {
    fn from(value: f32) -> Self {
        Self::Number(value)
    }
}
impl UiValue {
    pub fn parameter(id: impl Into<String>, normalized: bool) -> Self {
        Self::Expression(Box::new(UiExpression::Parameter { parameter: id.into(), normalized, logarithmic: false }))
    }
    pub fn normalized(id: impl Into<String>, logarithmic: bool) -> Self {
        Self::Expression(Box::new(UiExpression::Parameter { parameter: id.into(), normalized: true, logarithmic }))
    }
    pub fn exp(self) -> Self {
        Self::Expression(Box::new(UiExpression::Exp { value: self }))
    }
    pub fn ln(self) -> Self {
        Self::Expression(Box::new(UiExpression::Log { value: self }))
    }
    pub fn sin(self) -> Self {
        Self::Expression(Box::new(UiExpression::Sin { value: self }))
    }
    pub fn cos(self) -> Self {
        Self::Expression(Box::new(UiExpression::Cos { value: self }))
    }
    pub fn evaluate(&self, parameter: &impl Fn(&str, bool, bool) -> f64) -> f32 {
        self.eval(parameter, 0).clamp(-16384.0, 16384.0) as f32
    }
    fn eval(&self, parameter: &impl Fn(&str, bool, bool) -> f64, depth: usize) -> f64 {
        if depth > 16 {
            return 0.0;
        }
        let v = match self {
            Self::Number(v) => *v as f64,
            Self::Expression(expr) => match expr.as_ref() {
                UiExpression::Parameter { parameter: id, normalized, logarithmic } => {
                    parameter(id, *normalized, *logarithmic)
                }
                UiExpression::Add { a, b } => a.eval(parameter, depth + 1) + b.eval(parameter, depth + 1),
                UiExpression::Multiply { a, b } => a.eval(parameter, depth + 1) * b.eval(parameter, depth + 1),
                UiExpression::Equal { a, b } => {
                    f64::from((a.eval(parameter, depth + 1) - b.eval(parameter, depth + 1)).abs() < 0.0001)
                }
                UiExpression::Exp { value } => value.eval(parameter, depth + 1).exp(),
                UiExpression::Log { value } => value.eval(parameter, depth + 1).ln(),
                UiExpression::Sin { value } => value.eval(parameter, depth + 1).sin(),
                UiExpression::Cos { value } => value.eval(parameter, depth + 1).cos(),
                UiExpression::Wave { shape, cycles } => {
                    waveform(shape.eval(parameter, depth + 1) as usize, cycles.eval(parameter, depth + 1))
                }
            },
        };
        if v.is_finite() { v.clamp(-1e12, 1e12) } else { 0.0 }
    }
}
impl std::ops::Add for UiValue {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::Expression(Box::new(UiExpression::Add { a: self, b: rhs }))
    }
}
impl std::ops::Mul for UiValue {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::Expression(Box::new(UiExpression::Multiply { a: self, b: rhs }))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    #[default]
    Stretch,
    Contain,
    Cover,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextFormat {
    Choice,
    #[default]
    Number,
    Frequency,
    Tempo,
    Seconds,
    Percent,
    Decibels,
    Balance,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Draw {
    Rect {
        rect: [UiValue; 4],
        color: Color,
        #[serde(default)]
        radius: f32,
    },
    Circle {
        center: [UiValue; 2],
        radius: UiValue,
        color: Color,
    },
    Line {
        from: [UiValue; 2],
        to: [UiValue; 2],
        color: Color,
        #[serde(default = "one")]
        width: f32,
    },
    Path {
        points: Vec<[UiValue; 2]>,
        color: Color,
        #[serde(default = "one")]
        width: f32,
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        fill: Option<Color>,
    },
    Arc {
        center: [UiValue; 2],
        radius: UiValue,
        start: UiValue,
        end: UiValue,
        color: Color,
        #[serde(default = "one")]
        width: f32,
    },
    Text {
        #[serde(default)]
        align: TextAlign,
        #[serde(default)]
        format: TextFormat,
        position: [UiValue; 2],
        text: String,
        color: Color,
        size: f32,
        #[serde(default)]
        parameter: Option<String>,
        #[serde(default)]
        decimals: usize,
    },
    Image {
        asset: String,
        rect: [UiValue; 4],
        #[serde(default = "uv_default")]
        uv: [f32; 4],
        #[serde(default = "white")]
        tint: Color,
        #[serde(default)]
        tile_size: Option<[f32; 2]>,
        #[serde(default)]
        fit: ImageFit,
    },
    Gradient {
        rect: [UiValue; 4],
        colors: [Color; 4],
    },
}
impl Draw {
    pub fn rect(rect: [f32; 4], color: Color) -> Self {
        Self::Rect { rect: rect.map(UiValue::from), color, radius: 0.0 }
    }
    pub fn image(asset: impl Into<String>, rect: [f32; 4]) -> Self {
        Self::Image {
            asset: asset.into(),
            rect: rect.map(UiValue::from),
            uv: uv_default(),
            tint: white(),
            tile_size: None,
            fit: ImageFit::Stretch,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiGesture {
    #[default]
    Drag,
    Position,
    Toggle,
    Momentary,
    Menu,
    /// Set the X parameter to a literal value, for segmented selectors.
    Set(u16),
    /// Select a tab; selecting the active tab returns to value zero.
    Tab(u16),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiBinding {
    pub parameter: String,
    #[serde(default)]
    pub logarithmic: bool,
    #[serde(default = "one")]
    pub drag_scale: f32,
}
impl UiBinding {
    pub fn new(parameter: impl Into<String>) -> Self {
        Self { parameter: parameter.into(), logarithmic: false, drag_scale: 1. }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomControl {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<Widget>,
    #[serde(default)]
    pub visible_when: Option<UiValue>,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tooltip: String,
    pub bounds: [f32; 4],
    #[serde(default)]
    pub gesture: UiGesture,
    #[serde(default)]
    pub x: Option<UiBinding>,
    #[serde(default)]
    pub y: Option<UiBinding>,
    #[serde(default)]
    pub drawing: Vec<Draw>,
}

impl DeviceUi {
    pub(crate) fn validate(&self, parameters: &[Parameter]) -> Result<(), DeviceSdkError> {
        let fail = |s: &str| DeviceSdkError(format!("device UI: {s}"));
        if self.viewport_size.is_some_and(|size| size.iter().any(|v| !v.is_finite() || !(24.0..=8192.0).contains(v))) {
            return Err(fail("viewport dimensions exceed limits"));
        }
        if self.size.iter().any(|v| !v.is_finite() || !(24.0..=8192.0).contains(v))
            || self.controls.len() > 128
            || self.assets.len() > 32
            || self.hidden_parameters.len() > 128
        {
            return Err(fail("canvas dimensions or item count exceed limits"));
        }
        if self.style.as_ref().is_some_and(|style| style.len() > MAX_STYLE_BYTES || style.contains('\0')) {
            return Err(fail("panel style exceeds 64 KiB or contains NUL"));
        }
        let ids = parameters.iter().map(|p| p.id.as_str()).collect::<HashSet<_>>();
        let mut asset_ids = HashSet::new();
        let mut pixels = 0u64;
        let mut bytes = 0usize;
        for asset in &self.assets {
            if asset.id.is_empty() || asset.id.len() > 128 || !asset_ids.insert(asset.id.as_str()) {
                return Err(fail("image ids must be unique and 1..128 bytes"));
            }
            let [w, h] = asset.dimensions()?;
            pixels += u64::from(w) * u64::from(h);
            bytes = bytes
                .checked_add(STANDARD.decode(&asset.data).map_err(|e| fail(&e.to_string()))?.len())
                .ok_or_else(|| fail("asset size overflow"))?;
            if pixels > 4_000_000 || bytes > MAX_ASSETS_BYTES {
                return Err(fail("combined image assets exceed byte/pixel limits"));
            }
        }
        if self.hidden_parameters.iter().any(|id| !ids.contains(id.as_str())) {
            return Err(fail("hidden row refers to unknown parameter"));
        }
        let count = self.controls.iter().try_fold(self.layers.len(), |total, c| {
            total.checked_add(c.drawings().len()).ok_or_else(|| fail("drawing count overflow"))
        })?;
        if count > MAX_DRAW_COMMANDS {
            return Err(fail("more than 1024 drawing commands"));
        }
        let mut controls = HashSet::new();
        let mut expression_budget = 32768usize;
        for c in &self.controls {
            if c.widget.is_some() && !c.drawing.is_empty() {
                return Err(fail("widget and drawing are mutually exclusive"));
            }
            if let Some(widget) = &c.widget {
                let valid = match widget {
                    Widget::Label { .. }
                    | Widget::Panel { .. }
                    | Widget::Separator
                    | Widget::SampleWaveform
                    | Widget::SampleRecording
                    | Widget::EqCurve => c.x.is_none() && c.y.is_none(),
                    Widget::Envelope { attack, decay, level, release } => {
                        let ids: Vec<_> =
                            [Some(attack), Some(decay), Some(level), release.as_ref()].into_iter().flatten().collect();
                        c.x.is_none()
                            && c.y.is_none()
                            && c.bounds[2] >= 40.
                            && c.bounds[3] >= 32.
                            && ids.iter().collect::<HashSet<_>>().len() == ids.len()
                            && ids.iter().enumerate().all(|(i, id)| {
                                parameters.iter().any(|p| {
                                    p.id == id.as_str()
                                        && p.min < p.max
                                        && if i == 2 {
                                            if release.is_some() {
                                                p.min == 0. && p.max == 1.
                                            } else {
                                                p.min < 0. && p.max > 0.
                                            }
                                        } else {
                                            p.min == 0. && p.max <= 60.
                                        }
                                })
                            })
                    }
                    Widget::Button { .. } => {
                        matches!(
                            c.gesture,
                            UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Tab(_)
                        )
                    }
                    Widget::Disclosure { .. } => c.gesture == UiGesture::Toggle,
                    Widget::Number { .. } | Widget::Knob { .. } | Widget::Dial { .. } | Widget::Stepper { .. } => {
                        c.gesture == UiGesture::Drag && c.x.is_none() && c.y.is_some()
                    }
                    Widget::ValueMenu { choices } => {
                        let mut values = HashSet::new();
                        c.gesture == UiGesture::Menu
                            && !choices.is_empty()
                            && choices.len() <= 128
                            && choices.iter().all(|(value, label)| {
                                values.insert(value)
                                    && !label.is_empty()
                                    && label.len() <= 128
                                    && c.x.as_ref().is_some_and(|b| {
                                        parameters.iter().any(|p| {
                                            p.id == b.parameter
                                                && f64::from(*value) >= p.min
                                                && f64::from(*value) <= p.max
                                        })
                                    })
                            })
                    }
                    Widget::Menu => c.gesture == UiGesture::Menu,
                    Widget::WaveformPreview { shape } => {
                        c.gesture == UiGesture::Position
                            && c.x.is_some()
                            && c.y.is_none()
                            && parameters.iter().any(|p| p.id == *shape && p.min == 0. && matches!(p.max, 5. | 6.))
                    }
                    Widget::WaveformMenu => {
                        c.gesture == UiGesture::Menu
                            && c.x.as_ref().is_some_and(|binding| {
                                parameters.iter().any(|p| {
                                    p.id == binding.parameter
                                        && p.min == 0.
                                        && matches!(p.max, 5. | 6.)
                                        && p.ui.options.len() == p.max as usize + 1
                                })
                            })
                    }
                    Widget::FilterBand { min_hz, max_hz } => {
                        c.gesture == UiGesture::Position
                            && c.x.as_ref().is_some_and(|binding| binding.logarithmic)
                            && c.y.as_ref().is_some_and(|binding| !binding.logarithmic)
                            && min_hz.is_finite()
                            && max_hz.is_finite()
                            && *min_hz > 0.
                            && max_hz > min_hz
                    }
                };
                if !valid {
                    return Err(fail("widget has incompatible gesture, bindings or frequency range"));
                }
            }
            if let UiGesture::Tab(value) = c.gesture {
                let valid = value > 0
                    && c.x.as_ref().is_some_and(|b| {
                        !b.logarithmic && parameters.iter().any(|p| p.id == b.parameter && p.min == 0.)
                    });
                if !valid {
                    return Err(fail("tabs require a positive value and a zero-based linear parameter"));
                }
            }
            let [x, y, w, h] = c.bounds;
            if c.id.is_empty()
                || c.id.len() > 128
                || !controls.insert(c.id.as_str())
                || c.name.is_empty()
                || c.name.len() > 128
                || c.tooltip.len() > 1024
                || c.bounds.iter().any(|v| !v.is_finite())
                || x < 0.0
                || y < 0.0
                || w < 8.0
                || h < 8.0
                || x + w > self.size[0]
                || y + h > self.size[1]
            {
                return Err(fail("invalid custom control id/name/bounds"));
            }
            if let Some(condition) = &c.visible_when {
                condition.validate(&ids, 0, &mut expression_budget)?;
            }
            if c.x.is_none()
                && c.y.is_none()
                && !matches!(
                    c.widget,
                    Some(
                        Widget::Label { .. }
                            | Widget::Panel { .. }
                            | Widget::Separator
                            | Widget::SampleWaveform
                            | Widget::SampleRecording
                            | Widget::Envelope { .. }
                            | Widget::EqCurve
                    )
                )
            {
                return Err(fail("custom control needs a parameter binding"));
            }
            if c.x.as_ref().zip(c.y.as_ref()).is_some_and(|(x, y)| x.parameter == y.parameter) {
                return Err(fail("X and Y must bind different parameters"));
            }
            if matches!(
                c.gesture,
                UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Tab(_) | UiGesture::Menu
            ) && (c.x.is_none() || c.y.is_some())
            {
                return Err(fail("button gestures require only X binding"));
            }
            for binding in [c.x.as_ref(), c.y.as_ref()].into_iter().flatten() {
                if !binding.drag_scale.is_finite() || !(0.01..=100.).contains(&binding.drag_scale) {
                    return Err(fail("drag scale must be finite and within 0.01..100"));
                }
                let p = parameters
                    .iter()
                    .find(|p| p.id == binding.parameter)
                    .ok_or_else(|| fail("unknown bound parameter"))?;
                if c.gesture == UiGesture::Menu
                    && ((!matches!(c.widget, Some(Widget::ValueMenu { .. }))
                        && p.control != crate::ParameterControl::Choice)
                        || binding.logarithmic)
                {
                    return Err(fail("menu needs a non-logarithmic Choice binding"));
                }
                if p.min >= p.max || (binding.logarithmic && p.min <= 0.0) {
                    return Err(fail("binding needs a nonempty range; logarithmic range must be positive"));
                }
                if let UiGesture::Set(value) | UiGesture::Tab(value) = c.gesture
                    && !(p.min..=p.max).contains(&f64::from(value))
                {
                    return Err(fail("button value is outside the bound parameter range"));
                }
            }
        }
        for draw in self.layers.iter().cloned().chain(self.controls.iter().flat_map(|c| c.drawings().into_owned())) {
            draw.validate(&ids, &asset_ids, &mut expression_budget)?;
        }
        Ok(())
    }
}
impl UiValue {
    fn validate(&self, ids: &HashSet<&str>, depth: usize, budget: &mut usize) -> Result<(), DeviceSdkError> {
        let fail = || DeviceSdkError("UI expression: invalid value, parameter or complexity".into());
        if depth > 16 || *budget == 0 {
            return Err(fail());
        }
        *budget -= 1;
        match self {
            Self::Number(v) => {
                if !v.is_finite() || v.abs() > 1e6 {
                    return Err(fail());
                }
            }
            Self::Expression(expr) => match expr.as_ref() {
                UiExpression::Parameter { parameter, .. } => {
                    if !ids.contains(parameter.as_str()) {
                        return Err(fail());
                    }
                }
                UiExpression::Add { a, b }
                | UiExpression::Multiply { a, b }
                | UiExpression::Equal { a, b }
                | UiExpression::Wave { shape: a, cycles: b } => {
                    a.validate(ids, depth + 1, budget)?;
                    b.validate(ids, depth + 1, budget)?;
                }
                UiExpression::Sin { value }
                | UiExpression::Cos { value }
                | UiExpression::Exp { value }
                | UiExpression::Log { value } => value.validate(ids, depth + 1, budget)?,
            },
        }
        Ok(())
    }
}
impl Draw {
    fn validate(&self, ids: &HashSet<&str>, assets: &HashSet<&str>, budget: &mut usize) -> Result<(), DeviceSdkError> {
        let fail = |s: &str| DeviceSdkError(format!("UI drawing: {s}"));
        let width = |v: f32| {
            if v.is_finite() && (0.0..=128.0).contains(&v) { Ok(()) } else { Err(fail("invalid stroke width/radius")) }
        };
        let values: Vec<&UiValue> = match self {
            Self::Rect { rect, radius, .. } => {
                width(*radius)?;
                rect.iter().collect()
            }
            Self::Gradient { rect, .. } => rect.iter().collect(),
            Self::Circle { center, radius, .. } => center.iter().chain([radius]).collect(),
            Self::Line { from, to, width: w, .. } => {
                width(*w)?;
                from.iter().chain(to).collect()
            }
            Self::Path { points, width: w, fill, .. } => {
                width(*w)?;
                if points.len() < if fill.is_some() { 3 } else { 2 } || points.len() > 256 {
                    return Err(fail("path requires 2..256 points, at least 3 when filled"));
                }
                points.iter().flatten().collect()
            }
            Self::Arc { center, radius, start, end, width: w, .. } => {
                width(*w)?;
                center.iter().chain([radius, start, end]).collect()
            }
            Self::Text { position, text, size, parameter, decimals, .. } => {
                if text.len() > 1024
                    || !size.is_finite()
                    || !(8.0..=48.0).contains(size)
                    || *decimals > 8
                    || parameter.as_ref().is_some_and(|p| !ids.contains(p.as_str()))
                {
                    return Err(fail("invalid text/font/parameter"));
                }
                position.iter().collect()
            }
            Self::Image { asset, rect, uv, tile_size, .. } => {
                if !assets.contains(asset.as_str())
                    || uv.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                    || uv[2] <= uv[0]
                    || uv[3] <= uv[1]
                    || tile_size.is_some_and(|s| s.iter().any(|v| !v.is_finite() || !(1.0..=4096.0).contains(v)))
                {
                    return Err(fail("invalid asset reference, UV or tile size"));
                }
                rect.iter().collect()
            }
        };
        for value in values {
            value.validate(ids, 0, budget)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for scale in [1., 2., 0., -1., f32::NAN, f32::INFINITY, 101.] {
            ui.controls[0].x.as_mut().unwrap().drag_scale = scale;
            assert_eq!(ui.validate(&parameters).is_ok(), scale == 1. || scale == 2.);
        }
    }
}
