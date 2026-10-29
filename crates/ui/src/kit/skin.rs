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

/// Roles only a theme's own artwork fills: without it the control is drawn natively, so Suhina
/// ships none for them.
pub const OWN_ROLES: &[&str] = &[
    "volume.track",
    "volume.fill",
    "pan.track",
    "pan.fill",
    "volume.handle",
    "pan.handle",
    // A pan drawn as a knob (`.pan { --suhina-appearance: knob }`), apart from every other knob.
    "pan.knob.body",
    "pan.knob.pointer",
    // A send's knob (a return's level), apart from every other knob.
    "send.knob.body",
    "send.knob.pointer",
    // A send's tile and its level rail.
    "send",
    "send.fill",
    // The buttons that add a track, after the last one.
    "add-track",
    // The Arrangement view's ruler and its loop region.
    "ruler",
    "loop-region",
    // An arrangement clip's fade handles.
    "clip.fade-handle",
    // A device parameter's slider: its rail, the fill up to its value and its handle.
    "slider.rail",
    "slider.fill",
    "slider.handle",
    // The piano roll's tool buttons, by the id after `#piano-`.
    "piano.select",
    "piano.draw",
    "piano.paint",
    "piano.erase",
    "piano.slice",
    "piano.mute",
    "piano.cut",
    "piano.copy",
    "piano.paste",
    "piano.duplicate",
    "piano.quantise",
    "piano.midi-tools",
    "piano.audition",
    "piano.hide",
    // A track's artwork in one view only, over `track.name` and `track.body`.
    "track.name.session",
    "track.name.mixer",
    "track.name.arrangement",
    "track.body.session",
    "track.body.arrangement",
    // An Arrangement header's own buttons: the track-on toggle (over its number), automation,
    // the fold triangle and the MIDI monitoring chip.
    "track-on",
    "automation",
    "fold",
    "monitor",
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
    /// The theme's style sheet (`theme.css`), as its author wrote it.
    pub css: Option<Arc<str>>,
    /// The chosen variant's values as a `:root` rule, applied above the theme's sheet.
    pub variant: Option<Arc<str>>,
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
    /// The resolved `:root` variables, substituted into artwork that writes `var(--name)`.
    pub variables: Arc<BTreeMap<String, String>>,
}
impl Default for Skin {
    fn default() -> Self {
        Self {
            css: None,
            variant: None,
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
            variables: Arc::default(),
        }
    }
}

pub fn valid_role(role: &str) -> bool {
    // One button's own artwork: `button.play`, `button.record:on`.
    if let Some(rest) = role.strip_prefix("button.") {
        let name = rest.split('.').next().unwrap_or_default();
        if super::native_css::CONTROL_IDS.contains(&name) {
            return valid_role(&format!("button{}", &rest[name.len()..]));
        }
    }
    ROLES.iter().chain(OWN_ROLES).any(|base| {
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
        if parts.peek().is_some_and(|part| matches!(*part, "hover" | "active" | "disabled" | "on")) {
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

/// SVG `<text>` draws with Suhina's own typefaces, never the system's: Manrope as the default
/// and sans-serif family, IBM Plex Mono as monospace.
fn svg_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> = std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_font_data(super::theme::UI_FONT.to_vec());
            db.load_font_data(super::theme::MONO_FONT.to_vec());
            db.set_sans_serif_family("Manrope");
            db.set_serif_family("Manrope");
            db.set_monospace_family("IBM Plex Mono");
            Arc::new(db)
        })
        .clone()
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
        font_family: "Manrope".into(),
        fontdb: svg_fonts(),
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
            super::control_layout::validate_anchors(&mut skin.controls, warnings);
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
    /// Pressed.
    Active,
    Disabled,
    /// A toggle that is on and not pressed: artwork `ROLE.on`, else `ROLE.active`.
    On,
}
impl State {
    pub fn response(response: &egui::Response, on: bool) -> Self {
        if !response.enabled() {
            Self::Disabled
        } else if response.is_pointer_button_down_on() {
            Self::Active
        } else if on {
            Self::On
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
    /// Per source hash: the nine-slice insets and the artwork's own size, when it has them.
    slices: HashMap<u64, Option<Slice>>,
}

/// Artwork that stretches only its middle: `data-suhina-slice="top right bottom left"` (or one
/// value for all four) on the root `<svg>`, in the SVG's own units, which are logical points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slice {
    pub insets: [f32; 4],
    pub size: [f32; 2],
}

pub fn slice_of(source: &str) -> Option<Slice> {
    let start = source.find("data-suhina-slice=\"")? + "data-suhina-slice=\"".len();
    let end = source[start..].find('"')? + start;
    let values: Vec<f32> = source[start..end].split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
    let insets = match values.as_slice() {
        [all] => [*all; 4],
        [v, h] => [*v, *h, *v, *h],
        [t, r, b, l] => [*t, *r, *b, *l],
        _ => return None,
    };
    if insets.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return None;
    }
    let tree = parse_svg(source).ok()?;
    let size = [tree.size().width(), tree.size().height()];
    (insets[1] + insets[3] < size[0] && insets[0] + insets[2] < size[1]).then_some(Slice { insets, size })
}

/// The nine quads of `slice` over `rect`: corners keep their size (shrunk evenly on both axes when
/// `rect` is smaller than two insets), edges stretch along one axis, the middle along both. A
/// stretched part samples half a texel (`texel`, its size in UV) inside its own pixels, so the
/// corners' and edges' pixels never bleed into it as a gradient.
pub fn nine_slice(mesh: &mut egui::Mesh, rect: Rect, slice: Slice, texel: [f32; 2]) {
    let [t, r, b, l] = slice.insets;
    // One scale for both axes, so a rounded end shrinks as a whole rather than flattening.
    let fit = (rect.width() / (l + r).max(f32::EPSILON)).min(rect.height() / (t + b).max(f32::EPSILON)).min(1.0);
    let (fit_x, fit_y) = (fit, fit);
    let xs = [rect.left(), rect.left() + l * fit_x, rect.right() - r * fit_x, rect.right()];
    let ys = [rect.top(), rect.top() + t * fit_y, rect.bottom() - b * fit_y, rect.bottom()];
    let us = [0.0, l / slice.size[0], 1.0 - r / slice.size[0], 1.0];
    let vs = [0.0, t / slice.size[1], 1.0 - b / slice.size[1], 1.0];
    // The stretched middle column and row, half a texel in from each side (never crossing).
    let inner = |from: f32, to: f32, half: f32| {
        let half = half.min((to - from) / 2.0).max(0.0);
        (from + half, to - half)
    };
    let (u1, u2) = inner(us[1], us[2], texel[0] / 2.0);
    let (v1, v2) = inner(vs[1], vs[2], texel[1] / 2.0);
    for row in 0..3 {
        for col in 0..3 {
            let (u_from, u_to) = if col == 1 { (u1, u2) } else { (us[col], us[col + 1]) };
            let (v_from, v_to) = if row == 1 { (v1, v2) } else { (vs[row], vs[row + 1]) };
            mesh.add_rect_with_uv(
                Rect::from_min_max(egui::pos2(xs[col], ys[row]), egui::pos2(xs[col + 1], ys[row + 1])),
                Rect::from_min_max(egui::pos2(u_from, v_from), egui::pos2(u_to, v_to)),
                Color32::WHITE,
            );
        }
    }
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
/// Whether the theme in use has artwork of its own for `role`, not the artwork Suhina ships.
pub fn has_own(role: &str) -> bool {
    let skin = current();
    let Some(own) = skin.assets.get(role) else { return false };
    !DEFAULT_SVGS.iter().any(|(default, svg)| *default == role && *svg == own.as_str())
}

pub fn paint(painter: &Painter, rect: Rect, role: &str, state: State, ink: Color32, angle: f32) -> bool {
    paint_inner(painter, rect, role, state, ink, angle, 0.0)
}

pub fn button(painter: &Painter, rect: Rect, state: State, fill: Color32, radius: f32) -> bool {
    let role = button_role();
    // A state the button's own artwork leaves out is every button's.
    (role != "button" && paint_inner(painter, rect, &role, state, fill, 0.0, radius))
        || paint_inner(painter, rect, "button", state, fill, 0.0, radius)
}

/// The artwork role of the button now in scope: its own (`button.play`, with any of its states)
/// when the theme has one, else every button's.
pub fn button_role() -> String {
    super::native_css::current_name()
        .filter(|name| super::native_css::CONTROL_IDS.contains(&name.as_str()))
        .map(|name| format!("button.{name}"))
        .filter(|role| {
            let skin = current();
            skin.assets
                .keys()
                .any(|key| key == role || key.strip_prefix(role.as_str()).is_some_and(|rest| rest.starts_with('.')))
        })
        .unwrap_or_else(|| "button".to_owned())
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
    // The view's own artwork for the role, when the theme has one (`track.name.mixer`).
    let view = super::style_class::view().map(|view| format!("{role}.{view}"));
    let role = view.as_deref().filter(|role| has_own(role)).unwrap_or(role);
    paint_variant(painter, rect, role, state, ink, 0.0, 0.0, Some(track_color), None)
}

/// A missing or invalid signal-driven sequence lets the native meter draw normally.
pub fn meter_frame(painter: &Painter, rect: Rect, state: State, level: f32) -> bool {
    paint_variant(painter, rect, "meter", state, Color32::WHITE, 0.0, 0.0, super::artwork::current_color(), Some(level))
}

/// `var(--name)` and `var(--name, fallback)` in artwork take the theme's resolved values, colours
/// as computed colours; an unknown name without a fallback stays as written.
pub fn substitute_variables<'a>(source: &'a str, variables: &BTreeMap<String, String>) -> std::borrow::Cow<'a, str> {
    suhina_style::artwork::substitute(source, variables)
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
        State::Active | State::On => ".active",
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
    // The variables of the elements in scope (a track's `--track-colour`) win over the theme's.
    let scoped = if source.contains("var(--") { super::native_css::scope_variables() } else { Vec::new() };
    let substituted = if scoped.is_empty() {
        substitute_variables(source, &skin.variables)
    } else {
        let mut variables = (*skin.variables).clone();
        variables.extend(scoped.iter().cloned());
        std::borrow::Cow::Owned(substitute_variables(source, &variables).into_owned())
    };
    let source = substituted.as_ref();
    // Plain inherited track surfaces use the native fill and avoid filling the SVG
    // texture cache with a separate rectangle for every colour and strip height.
    if matches!(role, "track.name" | "track.body") && defaults.get(role).map(String::as_str) == Some(source) {
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
                if matches!(state, State::Hover | State::Active | State::On) {
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
    let mut source_key = variant.as_ref().map_or_else(|| key.clone(), |(key, _)| key.clone());
    for (name, value) in &scoped {
        source_key.push_str(&format!("|{name}={value}"));
    }
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
    let slice = if png { None } else { *textures.slices.entry(source_hash).or_insert_with(|| slice_of(source)) };
    if textures.slices.len() >= 256 {
        textures.slices.clear();
    }
    // Sliced artwork rasters once at its own size; the mesh stretches only its middle.
    let (w, h) = slice.map_or((w, h), |slice| {
        (
            (slice.size[0] * scale).ceil().clamp(1.0, 1024.0) as u32,
            (slice.size[1] * scale).ceil().clamp(1.0, 1024.0) as u32,
        )
    });
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
                    | "pan.knob.body"
                    | "pan.knob.pointer"
                    | "send.knob.body"
                    | "send.knob.pointer"
            ) || role.starts_with("piano."),
        )
        .map(|image| painter.ctx().load_texture(format!("skin-{key}"), image, egui::TextureOptions::LINEAR));
        textures.images.insert(key, texture);
    }
    let Some(Some(texture)) = textures.images.get(&key) else {
        return false;
    };
    // The native see-through host writes a background's alpha rather than blending
    // it twice. Clear the covered background before compositing its artwork; private
    // image backdrops and the opaque/OpenGL path keep ordinary blending. Only a theme whose
    // surfaces show the desktop needs it: over an opaque one, clearing would let the desktop
    // show through wherever the artwork leaves a pixel uncovered (rounded corners, edges).
    if (matches!(role, "button" | "fader.rail" | "fader.cap" | "track.name" | "track.body")
        || role.starts_with("button."))
        && super::surface::available(painter.ctx())
        && desktop_shows()
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
    if let Some(slice) = slice {
        let mut mesh = egui::Mesh::with_texture(texture.id());
        let size = texture.size();
        let texel = [1.0 / size[0].max(1) as f32, 1.0 / size[1].max(1) as f32];
        nine_slice(&mut mesh, rect, slice, texel);
        mesh.rotate(egui::emath::Rot2::from_angle(angle), rect.center());
        cast_shadows(painter, &mesh);
        painter.add(egui::Shape::mesh(mesh));
        return true;
    }
    if role == "button" || role.starts_with("button.") {
        let mut outline = egui::Mesh::with_texture(texture.id());
        outline.add_rect_with_uv(rect, Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
        outline.rotate(egui::emath::Rot2::from_angle(angle), rect.center());
        cast_shadows(painter, &outline);
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

/// The element in scope's `box-shadow` (a glow is one without an offset), cast by a surface's
/// artwork about to be drawn as `mesh`, under it and in its shape, in place of the rectangle's.
fn cast_shadows(painter: &Painter, mesh: &egui::Mesh) {
    let slot = painter.add(egui::Shape::Noop);
    super::native_css::artwork_shadows(painter, slot, mesh);
}

/// Whether the surfaces artwork stands on let the desktop show through: the window's, the
/// panels' and the track headers'. A translucent hover or selection colour does not count.
fn desktop_shows() -> bool {
    use super::theme;
    [theme::window_background(), theme::panel_background(), theme::track_header_background()]
        .iter()
        .any(|color| color.a() < 255)
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
#[path = "skin_tests.rs"]
mod tests;
