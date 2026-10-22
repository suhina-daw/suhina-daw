// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Data-only theme skins. Interaction stays with the original widgets.
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{Color32, Painter, Rect};
use serde_json::{Value, json};

pub const MAX_SVG: usize = 128 * 1024;
pub const MAX_ASSETS: usize = 64;
/// Base roles plus bounded variants; portable exports can always add missing base artwork.
pub const MAX_PACKAGE_ASSETS: usize = MAX_ASSETS + ROLES.len();
pub const MAX_PNG: usize = 8 * 1024 * 1024;
pub const MAX_ARTWORK_BYTES: usize = 16 * 1024 * 1024;
pub const MODULES: &[&str] = &[
    "browser",
    "tempo",
    "signature",
    "grid",
    "count-in",
    "metronome",
    "transport",
    "position",
    "mapping",
    "performance",
    "views",
    "agents",
];
pub const ROLES: &[&str] = &[
    "fader.cap",
    "fader.rail",
    "knob.body",
    "knob.pointer",
    "button",
    "play",
    "stop",
    "record",
    "solo",
    "arm",
    "cue",
    "track.midi",
    "track.audio",
    "track.name",
    "track.body",
    "meter",
];

/// The very same original SVG sources are embedded and distributed as editable files.
pub const DEFAULT_SVGS: &[(&str, &str)] = &[
    ("fader.cap", include_str!("../../../../assets/themes/default/fader.cap.svg")),
    ("fader.rail", include_str!("../../../../assets/themes/default/fader.rail.svg")),
    ("knob.body", include_str!("../../../../assets/themes/default/knob.body.svg")),
    ("knob.pointer", include_str!("../../../../assets/themes/default/knob.pointer.svg")),
    ("button", include_str!("../../../../assets/themes/default/button.svg")),
    ("play", include_str!("../../../../assets/themes/default/play.svg")),
    ("stop", include_str!("../../../../assets/themes/default/stop.svg")),
    ("record", include_str!("../../../../assets/themes/default/record.svg")),
    ("solo", include_str!("../../../../assets/themes/default/solo.svg")),
    ("arm", include_str!("../../../../assets/themes/default/arm.svg")),
    ("cue", include_str!("../../../../assets/themes/default/cue.svg")),
    ("track.midi", include_str!("../../../../assets/themes/default/track.midi.svg")),
    ("track.audio", include_str!("../../../../assets/themes/default/track.audio.svg")),
    ("track.name", include_str!("../../../../assets/themes/default/track.name.svg")),
    ("track.body", include_str!("../../../../assets/themes/default/track.body.svg")),
    ("meter", include_str!("../../../../assets/themes/default/meter.svg")),
];

pub fn default_assets() -> BTreeMap<String, String> {
    DEFAULT_SVGS.iter().map(|(role, svg)| ((*role).into(), (*svg).into())).collect()
}

pub fn inherited_assets() -> BTreeMap<String, String> {
    default_assets()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MeterStyle {
    #[default]
    Continuous,
    Segmented,
    Needle,
    Frames,
}
impl MeterStyle {
    pub fn name(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Segmented => "segmented",
            Self::Needle => "needle",
            Self::Frames => "frames",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolbar {
    pub order: Vec<String>,
    pub hidden: Vec<String>,
}
impl Default for Toolbar {
    fn default() -> Self {
        Self { order: MODULES.iter().map(|s| (*s).into()).collect(), hidden: Vec::new() }
    }
}
impl Toolbar {
    pub fn move_before(&mut self, from: &str, to: &str) {
        if from == to || !self.order.iter().any(|s| s == to) {
            return;
        }
        if let Some(index) = self.order.iter().position(|s| s == from) {
            let item = self.order.remove(index);
            if let Some(index) = self.order.iter().position(|s| s == to) {
                self.order.insert(index, item);
            }
        }
    }
    pub fn move_after(&mut self, from: &str, to: &str) {
        if from == to || !self.order.iter().any(|item| item == to) {
            return;
        }
        if let Some(index) = self.order.iter().position(|item| item == from) {
            let item = self.order.remove(index);
            if let Some(index) = self.order.iter().position(|item| item == to) {
                self.order.insert(index + 1, item);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skin {
    /// Authored modern CSS, preserved losslessly alongside the native JSON projection.
    pub css: Option<Arc<str>>,
    /// A portable PNG data URL, or a package-relative PNG path before resource resolution.
    pub background: Option<Arc<str>>,
    pub background_opacity: u8,
    pub background_blur: u8,
    /// Inline SVG/PNG data, or a relative image path resolved by the theme file loader.
    pub assets: BTreeMap<String, String>,
    pub styles: BTreeMap<String, BTreeMap<String, String>>,
    pub meter: MeterStyle,
    pub segments: u8,
    pub meter_intensity: u8,
    pub artwork_fps: u8,
    pub session: super::session_geometry::SessionGeometry,
    /// None preserves the original centred control-bar layout.
    pub toolbar: Option<Toolbar>,
    /// Horizontal offsets from each module's native/CSS anchor, in logical points.
    pub toolbar_offsets: BTreeMap<String, i32>,
    /// Semantic native controls; offsets and sizes are shared by paint and hit testing.
    pub controls: BTreeMap<String, super::control_layout::Placement>,
}
impl Default for Skin {
    fn default() -> Self {
        Self {
            css: None,
            background: None,
            background_opacity: 70,
            background_blur: 0,
            assets: BTreeMap::new(),
            styles: BTreeMap::new(),
            meter: MeterStyle::Continuous,
            segments: 24,
            meter_intensity: 100,
            artwork_fps: 12,
            session: Default::default(),
            toolbar: None,
            toolbar_offsets: BTreeMap::new(),
            controls: BTreeMap::new(),
        }
    }
}

pub fn valid_role(role: &str) -> bool {
    ROLES.iter().any(|base| {
        if role == *base {
            return true;
        }
        let Some(rest) = role.strip_prefix(&format!("{base}.")) else {
            return false;
        };
        let mut parts = rest.split('.').peekable();
        if parts.peek().is_some_and(|part| part.starts_with("color-")) {
            let Some(color) = parts.next().and_then(|part| part.strip_prefix("color-")) else {
                return false;
            };
            if color.len() != 6 || !color.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return false;
            }
        }
        if parts.peek().is_some_and(|part| matches!(*part, "hover" | "active" | "disabled")) {
            parts.next();
        }
        if let Some(frame) = parts.next() {
            let Some(number) = frame.strip_prefix("frame-").and_then(|v| v.parse::<u8>().ok()) else {
                return false;
            };
            if number >= 32 || frame != format!("frame-{number}") {
                return false;
            }
        }
        parts.next().is_none()
    })
}

/// A lexical portable path; filesystem readers also reject symlinks component by component.
pub fn asset_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
        && (path.ends_with(".svg") || path.ends_with(".png"))
}

pub fn png_artwork(source: &str) -> bool {
    source.starts_with("data:image/png;base64,")
}
pub fn artwork_extension(source: &str) -> &'static str {
    if png_artwork(source) || source.ends_with(".png") { "png" } else { "svg" }
}
pub fn validate_artwork(source: &str) -> Result<(), String> {
    if png_artwork(source) { super::artwork_png::decode(source).map(|_| ()) } else { parse_svg(source).map(|_| ()) }
}

pub fn parse_svg(source: &str) -> Result<resvg::usvg::Tree, String> {
    if source.len() > MAX_SVG {
        return Err("SVG exceeds 128 KiB".into());
    }
    // Resource resolvers below are disabled as well. Reject these explicitly so authors get a useful error.
    let lower = source.to_ascii_lowercase();
    if ["<!doctype", "<!entity", "<script", "<image", "<foreignobject", "href=", "href ="]
        .iter()
        .any(|s| lower.contains(s))
    {
        return Err("SVG must contain only self-contained vector artwork".into());
    }
    let options = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    resvg::usvg::Tree::from_str(source, &options).map_err(|e| format!("invalid SVG: {e}"))
}

impl Skin {
    pub fn from_json(value: &Value, warnings: &mut Vec<String>) -> Self {
        let mut skin = Self::default();
        let Some(root) = value.as_object() else {
            warnings.push("skin must be an object".into());
            return skin;
        };
        for key in root.keys() {
            if !matches!(
                key.as_str(),
                "css"
                    | "background"
                    | "background-opacity"
                    | "background-blur"
                    | "styles"
                    | "assets"
                    | "meter"
                    | "segments"
                    | "meter-intensity"
                    | "artwork-fps"
                    | "toolbar"
                    | "toolbar-offsets"
                    | "controls"
                    | "session"
            ) {
                warnings.push(format!("skin.{key} is unknown"));
            }
        }
        if let Some(value) = root.get("css") {
            if let Some(css) = value.as_str().filter(|css| css.len() <= super::theme_css::MAX_CSS) {
                match super::native_css::inspect(css) {
                    Ok(_) => skin.css = (!css.is_empty()).then(|| Arc::from(css)),
                    Err(error) => warnings.push(error),
                }
            } else {
                warnings.push("skin.css must be a stylesheet of at most 128 KiB".into());
            }
        }
        if let Some(value) = root.get("background") {
            if let Some(source) = value.as_str() {
                if source.starts_with("data:image/png;base64,") {
                    if decode_png(source).is_ok() {
                        skin.background = Some(Arc::from(source));
                    } else {
                        warnings.push("invalid embedded background PNG".into());
                    }
                } else if asset_path(&source.replace(".png", ".svg")) && source.ends_with(".png") {
                    skin.background = Some(Arc::from(source));
                } else {
                    warnings.push("background must be a relative PNG path or embedded PNG".into());
                }
            } else {
                warnings.push("background must be a PNG path".into());
            }
        }
        for (key, slot) in
            [("background-opacity", &mut skin.background_opacity), ("background-blur", &mut skin.background_blur)]
        {
            if let Some(value) = root.get(key) {
                if let Some(n) = value.as_u64().filter(|n| *n <= 100) {
                    *slot = n as u8;
                } else {
                    warnings.push(format!("skin.{key} must be 0..100"));
                }
            }
        }
        if let Some(value) = root.get("styles") {
            if let Some(styles) = value.as_object().filter(|v| v.len() <= 128) {
                for (selector, properties) in styles {
                    if !super::style_class::valid_selector(selector) {
                        warnings.push(format!("unknown CSS class {selector}"));
                        continue;
                    }
                    if let Some(properties) = properties.as_object().filter(|v| v.len() <= 8) {
                        for (key, value) in properties {
                            if let Some(value) = value.as_str().filter(|v| super::style_class::valid_property(key, v)) {
                                skin.styles.entry(selector.clone()).or_default().insert(key.clone(), value.into());
                            } else {
                                warnings.push(format!("invalid style {selector} {key}"));
                            }
                        }
                    } else {
                        warnings.push(format!("{selector}: styles must be a bounded object"));
                    }
                }
            } else {
                warnings.push("skin.styles must be a bounded object".into());
            }
        }
        if let Some(value) = root.get("assets") {
            if let Some(assets) = value.as_object().filter(|a| {
                a.len() <= MAX_PACKAGE_ASSETS
                    && a.keys().filter(|role| !ROLES.contains(&role.as_str())).count() <= MAX_ASSETS
                    && a.values()
                        .filter_map(Value::as_str)
                        .try_fold(0usize, |total, value| total.checked_add(value.len()))
                        .is_some_and(|total| total <= MAX_ARTWORK_BYTES)
            }) {
                for (role, value) in assets {
                    let Some(source) = value.as_str() else {
                        warnings.push(format!("skin.assets.{role} must be SVG, PNG data or an image path"));
                        continue;
                    };
                    if !valid_role(role) {
                        warnings.push(format!("unknown skin asset {role}"));
                        continue;
                    }
                    if source.trim_start().starts_with('<') || png_artwork(source) {
                        if let Err(e) = validate_artwork(source) {
                            warnings.push(format!("{role}: {e}"));
                            continue;
                        }
                    } else if !asset_path(source) {
                        warnings.push(format!("{role}: invalid relative image path"));
                        continue;
                    }
                    skin.assets.insert(role.clone(), source.to_string());
                }
            } else {
                warnings.push("skin.assets must contain base image roles and at most 64 variants within 16 MiB".into());
            }
        }
        if let Some(style) = root.get("meter") {
            skin.meter = match style.as_str() {
                Some("continuous") => MeterStyle::Continuous,
                Some("segmented") => MeterStyle::Segmented,
                Some("needle") => MeterStyle::Needle,
                Some("frames") => MeterStyle::Frames,
                _ => {
                    warnings.push("skin.meter must be continuous, segmented, needle or frames".into());
                    MeterStyle::Continuous
                }
            };
        }
        if let Some(value) = root.get("segments") {
            if let Some(n) = value.as_u64().filter(|n| (4..=64).contains(n)) {
                skin.segments = n as u8;
            } else {
                warnings.push("skin.segments must be an integer from 4 to 64".into());
            }
        }
        if let Some(value) = root.get("meter-intensity") {
            if let Some(n) = value.as_u64().filter(|n| *n <= 100) {
                skin.meter_intensity = n as u8;
            } else {
                warnings.push("skin.meter-intensity must be an integer from 0 to 100".into());
            }
        }
        if let Some(value) = root.get("artwork-fps") {
            if let Some(fps) = value.as_u64().filter(|fps| (1..=30).contains(fps)) {
                skin.artwork_fps = fps as u8;
            } else {
                warnings.push("skin.artwork-fps must be an integer from 1 to 30".into());
            }
        }
        if let Some(value) = root.get("toolbar") {
            if let Some(root) = value.as_object() {
                let mut toolbar = Toolbar { order: Vec::new(), hidden: Vec::new() };
                for (key, list) in [("order", &mut toolbar.order), ("hidden", &mut toolbar.hidden)] {
                    if let Some(value) = root.get(key) {
                        if let Some(items) = value.as_array().filter(|v| v.len() <= MODULES.len()) {
                            for item in items {
                                match item.as_str().filter(|s| MODULES.contains(s)) {
                                    Some(s) if !list.iter().any(|v| v == s) => list.push(s.to_string()),
                                    _ => warnings.push(format!("skin.toolbar.{key}: unknown or duplicate module")),
                                }
                            }
                        } else {
                            warnings.push(format!("skin.toolbar.{key} must be a bounded list of modules"));
                        }
                    }
                }
                for module in MODULES {
                    if !toolbar.order.iter().any(|s| s == module) {
                        toolbar.order.push((*module).into());
                    }
                }
                skin.toolbar = Some(toolbar);
            } else {
                warnings.push("skin.toolbar must be an object".into());
            }
        }
        if let Some(value) = root.get("toolbar-offsets") {
            if let Some(offsets) = value.as_object().filter(|v| v.len() <= MODULES.len()) {
                for (name, value) in offsets {
                    if let Some(offset) =
                        value.as_i64().filter(|n| MODULES.contains(&name.as_str()) && (-16384..=16384).contains(n))
                    {
                        skin.toolbar_offsets.insert(name.clone(), offset as i32);
                    } else {
                        warnings.push(format!(
                            "skin.toolbar-offsets.{name}: expected a known module and integer from -16384 to 16384"
                        ));
                    }
                }
            } else {
                warnings.push("skin.toolbar-offsets must be a bounded object".into());
            }
        }
        if let Some(value) = root.get("controls") {
            if let Some(controls) = value.as_object().filter(|v| v.len() <= 256) {
                for (name, value) in controls {
                    if super::control_layout::valid_name(name) {
                        skin.controls
                            .insert(name.clone(), super::control_layout::Placement::from_json(value, warnings));
                    } else {
                        warnings.push(format!("invalid control name {name}"));
                    }
                }
            } else {
                warnings.push("skin.controls must be an object of at most 256 controls".into());
            }
        }
        if let Some(value) = root.get("session") {
            skin.session = super::session_geometry::SessionGeometry::from_json(value, warnings);
        }
        skin
    }

    pub fn to_json(&self) -> Value {
        let mut value = json!({"styles":self.styles,"assets":self.assets,"meter":self.meter.name(),"segments":self.segments,"meter-intensity":self.meter_intensity,"artwork-fps":self.artwork_fps});
        if self.session != Default::default() {
            value["session"] = self.session.to_json();
        }
        if let Some(css) = &self.css {
            value["css"] = Value::String(css.to_string());
        }
        if let Some(toolbar) = &self.toolbar {
            value["toolbar"] = json!({"order": toolbar.order, "hidden": toolbar.hidden});
        }
        if !self.toolbar_offsets.is_empty() {
            value["toolbar-offsets"] = json!(self.toolbar_offsets);
        }
        if !self.controls.is_empty() {
            value["controls"] = self.controls.iter().map(|(name, p)| (name.clone(), p.to_json())).collect();
        }
        if let Some(background) = &self.background {
            value["background"] = Value::String(background.to_string());
        }
        value["background-opacity"] = json!(self.background_opacity);
        value["background-blur"] = json!(self.background_blur);
        value
    }
}

thread_local! {
    static ACTIVE: RefCell<Arc<Skin>> = RefCell::new(Arc::new(Skin::default()));
}
pub fn configure(skin: &Skin) {
    ACTIVE.with(|active| {
        if active.borrow().as_ref() != skin {
            *active.borrow_mut() = Arc::new(skin.clone());
        }
    });
}

pub fn active_toolbar_offset(module: &str) -> i32 {
    ACTIVE.with(|skin| skin.borrow().toolbar_offsets.get(module).copied().unwrap_or(0))
}
pub fn current() -> Arc<Skin> {
    ACTIVE.with(|skin| Arc::clone(&skin.borrow()))
}

pub fn decode_png(source: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let data = source.strip_prefix("data:image/png;base64,").ok_or("not an embedded PNG")?;
    if data.len() > MAX_PNG * 4 / 3 + 4 {
        return Err("PNG exceeds 8 MiB".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_PNG || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("invalid background PNG".into());
    }
    Ok(bytes)
}
pub fn encode_png(bytes: &[u8]) -> Result<Arc<str>, String> {
    use base64::Engine;
    if bytes.len() > MAX_PNG || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("invalid or oversized background PNG".into());
    }
    Ok(Arc::from(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

#[derive(Debug, Clone, Copy)]
pub enum State {
    Normal,
    Hover,
    Active,
    Disabled,
}
impl State {
    pub fn response(response: &egui::Response, on: bool) -> Self {
        if !response.enabled() {
            Self::Disabled
        } else if on || response.is_pointer_button_down_on() {
            Self::Active
        } else if response.hovered() {
            Self::Hover
        } else {
            Self::Normal
        }
    }
}
#[derive(Default)]
struct Textures {
    images: HashMap<u64, Option<egui::TextureHandle>>,
    source_owner: Option<Arc<Skin>>,
    sources: HashMap<String, u64>,
}

fn raster_artwork(source: &str, ink: Color32, width: u32, height: u32, contain: bool) -> Option<egui::ColorImage> {
    let (width, height) = (width.clamp(1, 1024), height.clamp(1, 1024));
    if png_artwork(source) {
        return super::artwork_png::raster(source, width, height);
    }
    let color = super::theme::hex_rgba(ink.to_srgba_unmultiplied());
    let source = source.replace("currentColor", &color);
    let tree = parse_svg(&source).ok()?;
    let mut pixels = resvg::tiny_skia::Pixmap::new(width, height)?;
    let sx = width as f32 / tree.size().width();
    let sy = height as f32 / tree.size().height();
    let transform = if contain {
        let scale = sx.min(sy);
        resvg::tiny_skia::Transform::from_row(
            scale,
            0.0,
            0.0,
            scale,
            (width as f32 - tree.size().width() * scale) * 0.5,
            (height as f32 - tree.size().height() * scale) * 0.5,
        )
    } else {
        resvg::tiny_skia::Transform::from_scale(sx, sy)
    };
    resvg::render(&tree, transform, &mut pixels.as_mut());
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for pixel in pixels.pixels() {
        let p = pixel.demultiply();
        rgba.extend_from_slice(&[p.red(), p.green(), p.blue(), p.alpha()]);
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba))
}

/// Cached vector/raster artwork. Rotation affects only the final mesh, not the cached image.
pub fn paint(painter: &Painter, rect: Rect, role: &str, state: State, ink: Color32, angle: f32) -> bool {
    paint_inner(painter, rect, role, state, ink, angle, 0.0)
}

pub fn button(painter: &Painter, rect: Rect, state: State, fill: Color32, radius: f32) -> bool {
    paint_inner(painter, rect, "button", state, fill, 0.0, radius)
}

fn paint_inner(painter: &Painter, rect: Rect, role: &str, state: State, ink: Color32, angle: f32, radius: f32) -> bool {
    paint_variant(painter, rect, role, state, ink, angle, radius, super::artwork::current_color(), None)
}

/// Raw track colour chooses artwork independently of the muted/selected paint tint.
pub fn paint_colored(
    painter: &Painter,
    rect: Rect,
    role: &str,
    state: State,
    ink: Color32,
    track_color: Color32,
) -> bool {
    paint_variant(painter, rect, role, state, ink, 0.0, 0.0, Some(track_color), None)
}

/// A missing or invalid signal-driven sequence lets the native meter draw normally.
pub fn meter_frame(painter: &Painter, rect: Rect, state: State, level: f32) -> bool {
    paint_variant(painter, rect, "meter", state, Color32::WHITE, 0.0, 0.0, super::artwork::current_color(), Some(level))
}

#[allow(clippy::too_many_arguments)]
fn paint_variant(
    painter: &Painter,
    rect: Rect,
    role: &str,
    state: State,
    ink: Color32,
    angle: f32,
    radius: f32,
    track_color: Option<Color32>,
    level: Option<f32>,
) -> bool {
    if !rect.is_positive() || !rect.is_finite() || !angle.is_finite() {
        return false;
    }
    let skin = current();
    let suffix = match state {
        State::Normal => "",
        State::Hover => ".hover",
        State::Active => ".active",
        State::Disabled => ".disabled",
    };
    static DEFAULTS: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    let defaults = DEFAULTS.get_or_init(default_assets);
    let key = format!("{role}{suffix}");
    let variant = if let Some(level) = level {
        let variant = super::artwork::level_variant(&skin, state, track_color, level);
        if variant.is_none() {
            return false;
        }
        variant
    } else {
        super::artwork::variant(
            &skin,
            role,
            state,
            track_color,
            painter.ctx().input(|input| input.time),
            super::motion::enabled(painter.ctx()),
        )
    };
    if variant.as_ref().is_some_and(|(_, animated)| *animated) {
        painter
            .ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(1.0 / f64::from(skin.artwork_fps.clamp(1, 30))));
    }
    let own = variant.as_ref().and_then(|(key, _)| skin.assets.get(key));
    let Some(source) = own.or_else(|| defaults.get(&key)).or_else(|| defaults.get(role)) else {
        return false;
    };
    // Plain inherited track surfaces use the native fill and avoid filling the SVG
    // texture cache with a separate rectangle for every colour and strip height.
    if matches!(role, "track.name" | "track.body") && defaults.get(role) == Some(source) {
        return false;
    }
    // Semantic ink travels with the SVG, so exported inherited artwork retains its colour.
    let png = png_artwork(source);
    let ink = if png {
        Color32::WHITE
    } else if source.contains(&format!("data-suhina-ink=\"{role}\"")) {
        match role {
            "knob.body" => super::theme::control_background(),
            "fader.rail" => super::motion::mix(super::theme::control_background(), Color32::BLACK, 0.65),
            "fader.cap" => super::motion::mix(
                if matches!(state, State::Hover | State::Active) {
                    super::theme::control_hover()
                } else {
                    super::theme::control_background()
                },
                ink,
                0.28,
            ),
            _ => ink,
        }
    } else {
        ink
    };
    let scale = painter.ctx().pixels_per_point();
    let w = (rect.width() * scale).ceil().clamp(1.0, 1024.0) as u32;
    let h = (rect.height() * scale).ceil().clamp(1.0, 1024.0) as u32;
    let cache_id = egui::Id::new("skin-textures");
    let cache = painter
        .ctx()
        .data_mut(|data| data.get_temp_mut_or_default::<Arc<std::sync::Mutex<Textures>>>(cache_id).clone());
    let Ok(mut textures) = cache.lock() else {
        return false;
    };
    if textures.source_owner.as_ref().is_none_or(|owner| !Arc::ptr_eq(owner, &skin)) {
        textures.sources.clear();
        textures.source_owner = Some(Arc::clone(&skin));
    }
    let source_key = variant.as_ref().map_or_else(|| key.clone(), |(key, _)| key.clone());
    let source_hash = if let Some(hash) = textures.sources.get(&source_key) {
        *hash
    } else {
        let mut hash = std::hash::DefaultHasher::new();
        source.hash(&mut hash);
        let hash = hash.finish();
        if textures.sources.len() >= 256 {
            textures.sources.clear();
        }
        textures.sources.insert(source_key, hash);
        hash
    };
    let mut hash = std::hash::DefaultHasher::new();
    (source_hash, role, ink.to_array(), w, h).hash(&mut hash);
    let key = hash.finish();
    if !textures.images.contains_key(&key) {
        if textures.images.len() >= 64 {
            textures.images.clear();
        }
        let texture = raster_artwork(
            source,
            ink,
            w,
            h,
            matches!(
                role,
                "play"
                    | "stop"
                    | "record"
                    | "solo"
                    | "arm"
                    | "cue"
                    | "track.midi"
                    | "track.audio"
                    | "knob.body"
                    | "knob.pointer"
            ),
        )
        .map(|image| painter.ctx().load_texture(format!("skin-{key}"), image, egui::TextureOptions::LINEAR));
        textures.images.insert(key, texture);
    }
    let Some(Some(texture)) = textures.images.get(&key) else {
        return false;
    };
    // The native see-through host writes a background's alpha rather than blending
    // it twice. Clear the covered background before compositing its artwork; private
    // image backdrops and the opaque/OpenGL path keep ordinary blending.
    if matches!(role, "button" | "fader.rail" | "fader.cap" | "track.name" | "track.body")
        && super::surface::available(painter.ctx())
    {
        let (sin, cos) = angle.sin_cos();
        let size = egui::vec2(
            rect.width() * cos.abs() + rect.height() * sin.abs(),
            rect.width() * sin.abs() + rect.height() * cos.abs(),
        );
        super::surface::Backgrounds::fill_background(
            painter,
            Rect::from_center_size(rect.center(), size),
            radius,
            Color32::TRANSPARENT,
        );
    }
    if role == "button" {
        painter.add(
            egui::epaint::RectShape::filled(rect, super::style_class::radius(radius), Color32::WHITE)
                .with_texture(texture.id(), Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)))
                .with_angle(angle),
        );
        return true;
    }
    let mut mesh = egui::Mesh::with_texture(texture.id());
    mesh.add_rect_with_uv(rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
    mesh.rotate(egui::emath::Rot2::from_angle(angle), rect.center());
    painter.add(egui::Shape::mesh(mesh));
    true
}

/// Original editable artwork supplied with the theme template.
pub fn example_assets() -> BTreeMap<String, String> {
    let mut assets = default_assets();
    assets.extend([
        ("fader.cap", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 18 26"><rect x="1" y="1" width="16" height="24" rx="3" fill="#555b62" stroke="currentColor"/><path d="M3 13h12" stroke="currentColor" stroke-width="2"/></svg>"##),
        ("fader.rail", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 5 100"><rect width="5" height="100" rx="2" fill="#11151a"/></svg>"##),
        ("knob.body", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="#30363d" stroke="currentColor"/></svg>"##),
        ("knob.pointer", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 4v7" stroke="currentColor" stroke-width="2"/></svg>"##),
        ("button", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="1" y="1" width="22" height="22" rx="4" fill="#30363d" stroke="currentColor"/></svg>"##),
        ("play", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M8 5l11 7L8 19z" fill="currentColor"/></svg>"##),
        ("stop", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" fill="currentColor"/></svg>"##),
        ("record", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="7" fill="currentColor"/></svg>"##),
        ("solo", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M17 7H10a3 3 0 0 0 0 6h4a3 3 0 0 1 0 6H7" fill="none" stroke="currentColor" stroke-width="2"/></svg>"##),
        ("arm", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="6" fill="none" stroke="currentColor" stroke-width="2"/></svg>"##),
        ("cue", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 14v-3a7 7 0 0 1 14 0v3M5 13v6M19 13v6" fill="none" stroke="currentColor" stroke-width="3"/></svg>"##),
    ].into_iter().map(|(k,v)| (k.to_string(), v.to_string())));
    assets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_symbols_keep_aspect_ratio_while_surface_artwork_stretches() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ffffff"/></svg>"##;
        for (width, height) in [(32, 16), (16, 32)] {
            let symbol = raster_artwork(svg, Color32::WHITE, width, height, true).unwrap();
            let surface = raster_artwork(svg, Color32::WHITE, width, height, false).unwrap();
            let lit = symbol.pixels.iter().filter(|p| p.a() > 0).count();
            assert_eq!(lit, 16 * 16);
            assert!(surface.pixels.iter().all(|p| p.a() == 255));
            assert_eq!(symbol.pixels[0].a(), 0);
        }
    }

    #[test]
    fn png_roles_reuse_pixels_until_a_live_edit_changes_the_source() {
        let png = super::super::artwork_png::fixture(2, 2, [20, 80, 160, 255]);
        let mut skin = Skin { assets: BTreeMap::from([("button".into(), png)]), ..Default::default() };
        let mut warnings = Vec::new();
        assert_eq!(Skin::from_json(&skin.to_json(), &mut warnings), skin);
        assert!(warnings.is_empty());
        let ctx = egui::Context::default();
        for frame in 0..3 {
            if frame == 2 {
                skin.assets.insert("button".into(), super::super::artwork_png::fixture(2, 2, [40, 160, 80, 255]));
            }
            configure(&skin);
            let mut output = ctx.run_ui(Default::default(), |ui| {
                assert!(button(
                    ui.painter(),
                    Rect::from_min_size(egui::pos2(10.0, 10.0), egui::Vec2::splat(20.0)),
                    State::Normal,
                    Color32::RED,
                    0.0
                ));
            });
            let expected = if frame == 2 { Color32::from_rgb(40, 160, 80) } else { Color32::from_rgb(20, 80, 160) };
            let found = output.textures_delta.set.values().flat_map(|d| d.iter()).any(|delta| match &delta.image {
                egui::ImageData::Color(image) => image.pixels.iter().all(|pixel| *pixel == expected),
            });
            output.textures_delta.clear();
            assert_eq!(found, frame != 1, "warm draws reuse the same PNG; live edits publish new pixels");
        }
        configure(&Skin::default());
        let huge = json!({"assets":{"button": "x".repeat(MAX_ARTWORK_BYTES + 1)}});
        assert!(Skin::from_json(&huge, &mut warnings).assets.is_empty());
        assert!(!warnings.is_empty());
    }

    #[test]
    fn svg_backgrounds_clear_native_alpha_once_and_keep_private_images_underneath() {
        struct Mark;
        impl super::super::surface::Provider for Mark {
            fn shape(&self, rect: Rect, _: egui::CornerRadius, color: Color32) -> egui::Shape {
                assert_eq!(color, Color32::TRANSPARENT);
                egui::Shape::circle_filled(rect.center(), 1.0, Color32::BLUE)
            }
        }
        configure(&Skin::default());
        let ctx = egui::Context::default();
        super::super::surface::install(&ctx, Arc::new(Mark));
        for private_image in [false, true] {
            super::super::liquid::set_background_image(
                &ctx,
                private_image.then(|| super::super::liquid::BackgroundImage {
                    id: egui::TextureId::Managed(42),
                    pixels: Arc::new(egui::ColorImage::filled([2, 2], Color32::RED)),
                }),
            );
            let mut output = ctx.run_ui(Default::default(), |ui| {
                super::super::surface::begin_frame(ui.ctx());
                let painter = ui.ctx().layer_painter(egui::LayerId::background());
                assert!(button(
                    &painter,
                    Rect::from_min_size(egui::pos2(10.0, 10.0), egui::Vec2::splat(30.0)),
                    State::Normal,
                    Color32::from_rgba_unmultiplied(50, 60, 70, 128),
                    3.0
                ));
                super::super::surface::finish_frame(ui.ctx());
            });
            output.textures_delta.clear();
            assert_eq!(
                output.shapes.iter().filter(|shape| matches!(shape.shape, egui::Shape::Circle(_))).count(),
                usize::from(!private_image)
            );
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.brush.is_some()))
            );
        }
    }
    #[test]
    fn inherited_svg_renders_missing_roles_while_theme_artwork_overrides_one_role() {
        assert_eq!(DEFAULT_SVGS.len(), ROLES.len());
        for (role, svg) in DEFAULT_SVGS {
            assert!(parse_svg(svg).is_ok(), "{role}");
        }
        let skin = Skin {assets:BTreeMap::from([("play".into(), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"28\" height=\"28\"><rect width=\"28\" height=\"28\" fill=\"#00ff00\"/></svg>".into())]), ..Default::default()};
        configure(&skin);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(28.0));
            assert!(paint(ui.painter(), rect, "button", State::Normal, Color32::BLUE, 0.0));
            assert!(paint(ui.painter(), rect, "play", State::Normal, Color32::RED, 0.0));
            assert!(paint(ui.painter(), rect, "stop", State::Normal, Color32::RED, 0.0));
        });
        let pixels: Vec<_> = output
            .textures_delta
            .set
            .values()
            .flat_map(|deltas| deltas.iter())
            .map(|delta| match &delta.image {
                egui::ImageData::Color(image) => image.pixels[14 * 28 + 14],
            })
            .collect();
        for color in [Color32::BLUE, Color32::GREEN, Color32::RED] {
            assert!(pixels.contains(&color));
        }
        assert_eq!(skin.assets.len(), 1, "inheritance must not populate the theme's overrides");
        output.textures_delta.clear();
        configure(&Skin::default());
    }
    #[test]
    fn svg_states_publish_distinct_pixels_and_reuse_cached_textures() {
        let artwork = |color: &str| {
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"><rect width=\"16\" height=\"16\" fill=\"{color}\"/></svg>"
            )
        };
        let skin = Skin {
            assets: [("button".into(), artwork("#FF0000")), ("button.active".into(), artwork("#00FF00"))].into(),
            ..Default::default()
        };
        configure(&skin);
        let ctx = egui::Context::default();
        let mut frame = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(16.0));
            assert!(paint(ui.painter(), rect, "button", State::Normal, Color32::WHITE, 0.0));
            assert!(paint(ui.painter(), rect, "button", State::Normal, Color32::WHITE, 0.0));
            assert!(paint(ui.painter(), rect, "button", State::Active, Color32::WHITE, 0.0));
            assert!(!paint(ui.painter(), rect, "absent", State::Normal, Color32::WHITE, 0.0));
        });
        let images: Vec<_> = frame
            .textures_delta
            .set
            .values()
            .flat_map(|deltas| deltas.iter())
            .filter(|delta| delta.image.size() == [16, 16])
            .collect();
        assert_eq!(images.len(), 2, "a repeated state should reuse its texture");
        let pixels: Vec<_> = images
            .iter()
            .map(|delta| match &delta.image {
                egui::ImageData::Color(image) => image.pixels[8 * 16 + 8],
            })
            .collect();
        assert!(pixels.contains(&Color32::RED));
        assert!(pixels.contains(&Color32::GREEN));
        frame.textures_delta.clear();
        configure(&Skin::default());
    }
    #[test]
    fn skin_round_trip_and_old_defaults() {
        let skin = Skin {
            css: Some(Arc::from("/* preserved */ @layer test { .browser { color:var(--ink); } }")),
            background: None,
            background_opacity: 70,
            background_blur: 0,
            styles: Default::default(),
            assets: example_assets(),
            session: Default::default(),
            meter: MeterStyle::Segmented,
            segments: 16,
            meter_intensity: 65,
            artwork_fps: 12,
            toolbar: Some(Toolbar::default()),
            toolbar_offsets: BTreeMap::from([("transport".into(), 80)]),
            controls: BTreeMap::new(),
        };
        let mut warnings = Vec::new();
        assert_eq!(Skin::from_json(&skin.to_json(), &mut warnings), skin);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(Skin::from_json(&json!({}), &mut warnings), Skin::default());
    }
    #[test]
    fn malformed_assets_and_layout_are_bounded() {
        let mut warnings = Vec::new();
        let skin = Skin::from_json(
            &json!({"assets":{"fader.cap":"../secret.svg", "play":"<svg>"},"segments":999,"meter":"oops","toolbar":{"order":["tempo","tempo","bogus"]}}),
            &mut warnings,
        );
        assert!(skin.assets.is_empty());
        assert_eq!(skin.segments, 24);
        assert_eq!(
            skin.toolbar.unwrap().order,
            Toolbar::default().order.into_iter().filter(|s| s != "tempo").fold(vec!["tempo".into()], |mut v, s| {
                v.push(s);
                v
            })
        );
        assert_eq!(warnings.len(), 6);
        for path in ["/tmp/a.svg", "../a.svg", "a/../b.svg", "C:/a.svg", "a\\b.svg", "a//b.svg"] {
            assert!(!asset_path(path));
        }
        for svg in ["<!DOCTYPE svg><svg/>", "<svg><image href=\"file:///tmp/x\"/></svg>"] {
            assert!(parse_svg(svg).is_err());
        }
    }

    #[test]
    fn toolbar_offsets_reject_unknown_modules_and_non_integer_or_unbounded_values() {
        let mut warnings = Vec::new();
        let skin = Skin::from_json(
            &json!({"toolbar-offsets":{"transport":80,"tempo":0.5,"views":16385,"bogus":4}}),
            &mut warnings,
        );
        assert_eq!(skin.toolbar_offsets, BTreeMap::from([("transport".into(), 80)]));
        assert_eq!(warnings.len(), 3);
        assert!(skin.toolbar.is_none());
    }
    #[test]
    fn moving_module_preserves_all_others() {
        let mut toolbar = Toolbar::default();
        toolbar.move_before("views", "tempo");
        assert_eq!(toolbar.order[1], "views");
        assert_eq!(toolbar.order.len(), MODULES.len());
        let before = toolbar.clone();
        toolbar.move_before("views", "views");
        toolbar.move_before("nope", "tempo");
        assert_eq!(toolbar, before);
        toolbar.move_after("browser", "agents");
        assert_eq!(toolbar.order.last().map(String::as_str), Some("browser"));
        assert_eq!(toolbar.order.len(), MODULES.len());
    }
}
