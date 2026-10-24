// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A theme as CSS properties (ADR 0031, amendment *Theme settings and variants*).
//!
//! A theme is one style sheet, `theme.css`, written by its author and never rewritten by
//! Suhina. What a user can change are custom properties: the **core set** every theme has (the
//! palette tokens and the settings that were fields of the theme model: meter, artwork frame
//! rate, liquid materials, surface gradients, the background's strength, Session geometry),
//! registered here with Suhina Dark's values, and any the theme declares itself with
//! `@property`. A **variant** is a small JSON file of values for those properties; the chosen
//! one applies in the Variant tier, above the theme's own rules.
//!
//! [`read`] reads a theme's sheet into its declared properties and its own values; [`resolve`]
//! sets the model ([`Theme`]) from those values and the variant's; [`sync`] turns edits of the
//! model back into variant values.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use suhina_style::registered::Registered;
use suhina_style::sheet::Sheet;

use super::theme::{Theme, hex_rgba};

/// Most values one variant may set, and the longest one value may be.
pub const MAX_VALUES: usize = 256;
pub const MAX_VALUE_BYTES: usize = 1024;

/// A theme's properties: those it declares and their values, and the chosen variant's values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Vars {
    /// The theme's own `@property` rules beyond the core set, in source order.
    pub declared: Arc<Vec<Registered>>,
    /// Every custom property's value at the root from the theme's sheet alone, resolved.
    pub base: Arc<BTreeMap<String, String>>,
    /// The chosen variant's values that differ from `base`.
    pub values: BTreeMap<String, String>,
    /// The chosen variant's id (its file's name); `None` is the theme's own values.
    pub variant: Option<String>,
    /// The chosen variant's name, as the picker shows it.
    pub variant_name: Option<String>,
    /// Whether the chosen variant says it is light (`Some(true)`) or dark.
    pub light: Option<bool>,
    /// Why the theme is still in an earlier file format, when its migration was refused.
    pub kept: Option<Arc<str>>,
}

/// A setting's smallest and largest value.
type Range = Option<(f32, f32)>;

/// What a theme's sheet holds: its declared properties, every custom property's value at the
/// root, and the sheet's notices.
pub type SheetValues = (Vec<Registered>, BTreeMap<String, String>, Vec<String>);

/// The core settings beyond the palette: name, syntax and range.
const SETTINGS: &[(&str, &str, Range)] = &[
    ("--meter-style", "continuous | segmented | needle | frames", None),
    ("--meter-segments", "<integer>", Some((4.0, 64.0))),
    ("--meter-intensity", "<integer>", Some((0.0, 100.0))),
    ("--artwork-fps", "<integer>", Some((1.0, 30.0))),
    ("--liquid", "auto | <integer>+", None),
    ("--gradient-panels", "*", None),
    ("--gradient-tracks", "*", None),
    ("--gradient-clips", "*", None),
    ("--background-image", "*", None),
    ("--background-opacity", "<integer>", Some((0.0, 100.0))),
    ("--background-blur", "<integer>", Some((0.0, 100.0))),
    ("--session-track-width", "auto | <length>", None),
    ("--session-return-width", "auto | <length>", None),
    ("--session-scene-width", "auto | <length>", None),
    ("--session-slot-height", "auto | <length>", None),
    ("--session-header-height", "auto | <length>", None),
];

/// A palette token's custom property: `accent` is `--accent`, `track.headerBackground` is
/// `--track-headerBackground`.
pub fn token_property(key: &str) -> String {
    format!("--{}", key.replace('.', "-"))
}

/// The core set as a style sheet: an `@property` rule for each, with Suhina Dark's values.
pub fn core_sheet() -> &'static str {
    static SHEET: OnceLock<String> = OnceLock::new();
    SHEET.get_or_init(|| {
        let defaults = core_values(&Theme::default());
        let mut css = String::from("/* Suhina's core theme properties, with Suhina Dark's values. */\n");
        for (name, syntax, range) in core_syntaxes() {
            let Some(initial) = defaults.get(name.as_str()) else { continue };
            css.push_str(&format!("@property {name} {{ syntax: \"{syntax}\"; initial-value: {initial};"));
            if let Some((min, max)) = range {
                css.push_str(&format!(" range: {min} {max};"));
            }
            css.push_str(" }\n");
        }
        css
    })
}

fn core_syntaxes() -> Vec<(String, &'static str, Range)> {
    let palette =
        super::theme::Palette::DARK.tokens().into_iter().map(|(key, _, _)| (token_property(key), "<color>", None));
    palette.chain(SETTINGS.iter().map(|(name, syntax, range)| ((*name).to_owned(), *syntax, *range))).collect()
}

fn core_parsed() -> Option<&'static Sheet> {
    static SHEET: OnceLock<Option<Sheet>> = OnceLock::new();
    SHEET.get_or_init(|| Sheet::parse(core_sheet()).ok()).as_ref()
}

/// The core properties, registered.
pub fn core() -> &'static [Registered] {
    core_parsed().map_or(&[], |sheet| sheet.properties.as_slice())
}

/// Whether `name` is one of the core properties.
pub fn is_core(name: &str) -> bool {
    core().iter().any(|p| p.name == name)
}

/// The values of the core properties that `theme` (the model) holds, as CSS text.
pub fn core_values(theme: &Theme) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for (key, _, color) in theme.colors.tokens() {
        values.insert(token_property(key), hex_rgba(theme.rgba(key, color)));
    }
    let skin = &theme.skin;
    values.insert("--meter-style".into(), skin.meter.name().into());
    values.insert("--meter-segments".into(), skin.segments.to_string());
    values.insert("--meter-intensity".into(), skin.meter_intensity.to_string());
    values.insert("--artwork-fps".into(), skin.artwork_fps.to_string());
    values.insert(
        "--liquid".into(),
        theme.liquid.map_or_else(|| "auto".into(), |l| format!("{} {} {}", l.panels, l.tracks, l.clips)),
    );
    for (name, gradient) in [
        ("--gradient-panels", &theme.gradients.panels),
        ("--gradient-tracks", &theme.gradients.tracks),
        ("--gradient-clips", &theme.gradients.clips),
    ] {
        let text = gradient.as_ref().map_or_else(
            || "none".into(),
            |gradient| {
                let stops: Vec<String> =
                    gradient.stops.iter().map(|stop| format!("{} {}%", hex_rgba(stop.rgba()), stop.position)).collect();
                format!("{}deg, {}", gradient.angle, stops.join(", "))
            },
        );
        values.insert(name.into(), text);
    }
    values.insert(
        "--background-image".into(),
        if skin.background.is_some() { "url(\"background.png\")".into() } else { "none".into() },
    );
    values.insert("--background-opacity".into(), skin.background_opacity.to_string());
    values.insert("--background-blur".into(), skin.background_blur.to_string());
    for (name, value) in skin.session.entries() {
        values.insert(format!("--session-{name}"), value.map_or_else(|| "auto".into(), |v| format!("{v}px")));
    }
    values
}

/// Sets the model from core property `values` (resolved, no `var()` left). Values that cannot be
/// read leave the model as it is, with a warning. `--background-image` only says whether the
/// theme has its background; the image itself is the theme's `background.png`.
pub fn apply_core(theme: &mut Theme, values: &BTreeMap<String, String>) -> Vec<String> {
    let mut warnings = Vec::new();
    let tokens: Vec<&'static str> = theme.colors.tokens().into_iter().map(|(key, _, _)| key).collect();
    for key in tokens {
        let Some(value) = values.get(&token_property(key)) else { continue };
        match suhina_style::value::Color::parse(value) {
            Ok(suhina_style::value::Color::Rgba([r, g, b, a])) => {
                if let Some(slot) = theme.colors.token_mut(key) {
                    *slot = egui::Color32::from_rgba_unmultiplied(r, g, b, a);
                }
                theme.transparent_rgb.remove(key);
                if a == 0 && [r, g, b] != [0, 0, 0] {
                    theme.transparent_rgb.insert(key.into(), [r, g, b]);
                }
            }
            _ => warnings.push(format!("{}: {value} is not a colour", token_property(key))),
        }
    }
    let number = |name: &str, min: u8, max: u8| -> Option<Result<u8, String>> {
        let value = values.get(name)?;
        Some(
            value
                .trim()
                .trim_end_matches('%')
                .parse::<u8>()
                .ok()
                .filter(|n| (min..=max).contains(n))
                .ok_or_else(|| format!("{name}: {value} is not a whole number from {min} to {max}")),
        )
    };
    let mut set = |result: Option<Result<u8, String>>, slot: &mut u8| match result {
        Some(Ok(n)) => *slot = n,
        Some(Err(warning)) => warnings.push(warning),
        None => {}
    };
    let skin = &mut theme.skin;
    set(number("--meter-segments", 4, 64), &mut skin.segments);
    set(number("--meter-intensity", 0, 100), &mut skin.meter_intensity);
    set(number("--artwork-fps", 1, 30), &mut skin.artwork_fps);
    set(number("--background-opacity", 0, 100), &mut skin.background_opacity);
    set(number("--background-blur", 0, 100), &mut skin.background_blur);
    if let Some(value) = values.get("--meter-style") {
        skin.meter = match value.trim() {
            "continuous" => super::skin::MeterStyle::Continuous,
            "segmented" => super::skin::MeterStyle::Segmented,
            "needle" => super::skin::MeterStyle::Needle,
            "frames" => super::skin::MeterStyle::Frames,
            other => {
                warnings.push(format!("--meter-style: unknown style {other}"));
                skin.meter
            }
        };
    }
    for (name, _) in skin.session.entries() {
        if let Some(value) = values.get(&format!("--session-{name}"))
            && let Err(error) = skin.session.set(name, value.trim())
        {
            warnings.push(format!("--session-{name}: {error}"));
        }
    }
    if let Some(value) = values.get("--background-image") {
        match value.trim() {
            "none" => skin.background = None,
            image => match background_path(image) {
                // A path is resolved to the image by the theme's files; a loaded image stays.
                Some(path) if skin.background.as_deref().is_none_or(|b| !b.starts_with("data:")) => {
                    skin.background = Some(Arc::from(path))
                }
                Some(_) => {}
                None => warnings.push(format!("--background-image: {image} is not url(\"<file>.png\")")),
            },
        }
    }
    if let Some(value) = values.get("--liquid") {
        let parts: Vec<Option<u8>> =
            value.split_whitespace().map(|n| n.parse::<u8>().ok().filter(|n| *n <= 100)).collect();
        match parts[..] {
            [_] if value.trim() == "auto" => theme.liquid = None,
            [Some(panels), Some(tracks), Some(clips)] => {
                theme.liquid = Some(super::liquid::Options { panels, tracks, clips })
            }
            _ => warnings.push(format!("--liquid: {value} is auto or three strengths from 0 to 100")),
        }
    }
    for (name, slot) in [
        ("--gradient-panels", &mut theme.gradients.panels),
        ("--gradient-tracks", &mut theme.gradients.tracks),
        ("--gradient-clips", &mut theme.gradients.clips),
    ] {
        let Some(value) = values.get(name) else { continue };
        match gradient(value) {
            Some(gradient) => *slot = gradient,
            None => warnings.push(format!("{name}: {value} is none or `<angle>deg, <colour> <position>%, ...`")),
        }
    }
    warnings
}

/// The relative PNG `url("…")` names, inside the theme's folder.
fn background_path(value: &str) -> Option<String> {
    let path = value.strip_prefix("url(")?.strip_suffix(')')?.trim().trim_matches(['"', '\'']);
    (path.ends_with(".png") && super::skin::asset_path(&path.replace(".png", ".svg"))).then(|| path.to_owned())
}

/// `none`, or `<angle>deg, <colour> <position>%, ...` with 2 to 8 stops (0 degrees runs left to
/// right, 90 top to bottom).
fn gradient(value: &str) -> Option<Option<super::gradient::Gradient>> {
    if value.trim() == "none" {
        return Some(None);
    }
    let mut parts = value.split(',').map(str::trim);
    let angle = parts.next()?.strip_suffix("deg")?.trim().parse::<u16>().ok().filter(|a| *a <= 360)?;
    let mut stops = Vec::new();
    for part in parts {
        let (color, position) = part.rsplit_once(char::is_whitespace)?;
        let position = position.strip_suffix('%')?.parse::<u8>().ok().filter(|p| *p <= 100)?;
        let suhina_style::value::Color::Rgba(rgba) = suhina_style::value::Color::parse(color.trim()).ok()? else {
            return None;
        };
        stops.push(super::gradient::Stop {
            position,
            color: egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
            transparent_rgb: (rgba[3] == 0).then_some([rgba[0], rgba[1], rgba[2]]),
        });
    }
    (2..=8).contains(&stops.len()).then_some(Some(super::gradient::Gradient { angle, stops }))
}

/// Reads a theme's sheet: the properties it declares beyond the core set, every custom
/// property's value at the root from the sheet alone (its `:root` rules over the initial values, as a
/// 1280 by 800 dark window sees them), and the sheet's notices.
pub fn read(text: &str) -> Result<SheetValues, String> {
    let sheet = Sheet::parse(text)?;
    let declared: Vec<Registered> = sheet.properties.iter().filter(|p| !is_core(&p.name)).cloned().collect();
    // Every variable the root sees, so a variant's value may refer to the theme's own.
    let base = root_variables(&sheet, None)?;
    let notices = sheet.notices.iter().map(|n| format!("line {}:{}: {}", n.line, n.column, n.message)).collect();
    Ok((declared, base, notices))
}

/// Every custom property's value at the root from `sheet`, with `variant`'s sheet above it, as a
/// 1280 by 800 dark window sees them.
fn root_variables(sheet: &Sheet, variant: Option<&Sheet>) -> Result<BTreeMap<String, String>, String> {
    use suhina_style::cascade::{AppliedSheet, Layer, compute};
    use suhina_style::selector::{Element, Tree};
    let core = core_parsed().ok_or("the core theme properties do not parse")?;
    let tree = Tree::new(vec![Element::new("html", None)])?;
    let viewport = suhina_style::media::Viewport { width: 1280.0, height: 800.0, dark: true, reduced_motion: false };
    let mut sheets = vec![
        AppliedSheet { sheet: core, layer: Layer::Default, root: None },
        AppliedSheet { sheet, layer: Layer::Theme, root: None },
    ];
    if let Some(variant) = variant {
        sheets.push(AppliedSheet { sheet: variant, layer: Layer::Variant, root: None });
    }
    let styles = compute(&tree, viewport, &sheets)?;
    Ok(styles.first().map(|style| style.variables().clone()).unwrap_or_default())
}

/// The registered property `name`: a core one or one `theme` declares.
pub fn registered<'a>(theme: &'a Theme, name: &str) -> Option<&'a Registered> {
    core().iter().chain(theme.vars.declared.iter()).find(|p| p.name == name)
}

/// Sets the model from the theme's own values and the chosen variant's, and the Variant tier's
/// sheet. A variant value that does not match its property's syntax is skipped with a warning.
pub fn resolve(theme: &mut Theme) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut merged = (*theme.vars.base).clone();
    let mut chosen = BTreeMap::new();
    for (name, value) in &theme.vars.values {
        // A registered property, or a variable the theme's own `:root` sets (a helper colour).
        if registered(theme, name).is_none() && !theme.vars.base.contains_key(name) {
            warnings.push(format!("{name}: the theme declares no such property"));
            continue;
        }
        merged.insert(name.clone(), value.clone());
        chosen.insert(name.clone(), value.clone());
    }
    let resolved = match suhina_style::resolve_variables(&merged) {
        Ok(resolved) => resolved,
        Err(error) => {
            warnings.push(error);
            (*theme.vars.base).clone()
        }
    };
    let mut values = (*theme.vars.base).clone();
    for name in chosen.keys().cloned().collect::<Vec<_>>() {
        let name = &name;
        let value = resolved.get(name).cloned().unwrap_or_default();
        if registered(theme, name).map_or(!value.trim().is_empty(), |p| p.accepts(&value)) {
            values.insert(name.clone(), value);
        } else {
            warnings.push(format!("{name}: {value} does not match the property's syntax"));
            chosen.remove(name);
        }
    }
    theme.vars.values.retain(|name, _| chosen.contains_key(name));
    theme.skin.variant = variant_sheet(&theme.vars.values);
    // The theme's own variables that refer to a value the variant changes follow it, as they do
    // in the painted sheet.
    if let (Some(css), Some(variant)) = (theme.skin.css.as_deref(), theme.skin.variant.as_deref())
        && let (Ok(sheet), Ok(variant)) = (Sheet::parse(css), Sheet::parse(variant))
        && let Ok(full) = root_variables(&sheet, Some(&variant))
    {
        values = full;
    }
    theme.skin.variables = Arc::new(values.clone());
    warnings.extend(apply_core(theme, &values));
    warnings
}

/// The variant's values as a `:root` rule, or nothing when it sets none.
pub fn variant_sheet(values: &BTreeMap<String, String>) -> Option<Arc<str>> {
    if values.is_empty() {
        return None;
    }
    let mut css = String::from(":root {");
    for (name, value) in values {
        css.push_str(&format!(" {name}: {value};"));
    }
    css.push_str(" }");
    Some(Arc::from(css))
}

/// After the model was edited: the variant's values become every core value that differs from
/// the theme's own, keeping the values it sets for the theme's declared properties.
pub fn sync(theme: &mut Theme) {
    let model = core_values(theme);
    theme.vars.values.retain(|name, _| !is_core(name));
    for (name, value) in model {
        // A value written as `var(--x)` or `oklch(…)` that gives the same model stays as written.
        if theme.vars.base.get(&name).is_some_and(|base| same_value(&name, base, &value)) {
            continue;
        }
        theme.vars.values.insert(name, value);
    }
    theme.skin.variant = variant_sheet(&theme.vars.values);
}

/// Whether two values of the core property `name` give the same model.
fn same_value(name: &str, a: &str, b: &str) -> bool {
    if a.trim() == b.trim() {
        return true;
    }
    let mut first = Theme::default();
    let mut second = Theme::default();
    let (one, two) = ([(name.to_owned(), a.to_owned())].into(), [(name.to_owned(), b.to_owned())].into());
    apply_core(&mut first, &one).is_empty() && apply_core(&mut second, &two).is_empty() && first == second
}

/// A theme variant: values for a theme's properties, in `variants/<id>.json`.
///
/// ```json
/// { "name": "Frost", "appearance": "light", "values": { "--accent": "#88c0d0" } }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// The file's name without `.json`.
    pub id: String,
    pub name: String,
    /// `Some(true)` for a light variant, `Some(false)` for a dark one.
    pub light: Option<bool>,
    pub values: BTreeMap<String, String>,
}

/// Whether `id` can name a variant file: lower-case letters, digits and `-`.
pub fn is_variant_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
}

impl Variant {
    pub fn from_json(id: &str, text: &str) -> Result<(Self, Vec<String>), String> {
        if !is_variant_id(id) {
            return Err(format!("{id} cannot name a variant"));
        }
        let root: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("{id}: not JSON: {e}"))?;
        let root = root.as_object().ok_or_else(|| format!("{id}: a variant is a JSON object"))?;
        let mut warnings = Vec::new();
        let name = match root.get("name").and_then(|n| n.as_str()).map(str::trim) {
            Some(name) if !name.is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control) => {
                name.to_owned()
            }
            _ => {
                warnings.push(format!("{id}: a variant's name is text of 1 to 64 characters"));
                id.to_owned()
            }
        };
        let light = match root.get("appearance").and_then(|a| a.as_str()) {
            Some("light") => Some(true),
            Some("dark") => Some(false),
            None => None,
            Some(other) => {
                warnings.push(format!("{id}: appearance is light or dark, not {other}"));
                None
            }
        };
        let mut values = BTreeMap::new();
        if let Some(object) = root.get("values").and_then(|v| v.as_object()) {
            for (name, value) in object.iter().take(MAX_VALUES) {
                match value.as_str() {
                    Some(value)
                        if name.starts_with("--")
                            && name.len() <= 128
                            && value.len() <= MAX_VALUE_BYTES
                            && !value.contains([';', '{', '}']) =>
                    {
                        values.insert(name.clone(), value.trim().to_owned());
                    }
                    _ => warnings.push(format!("{id}: {name} is not a custom property with a text value")),
                }
            }
            if object.len() > MAX_VALUES {
                warnings.push(format!("{id}: only the first {MAX_VALUES} values are read"));
            }
        }
        Ok((Self { id: id.to_owned(), name, light, values }, warnings))
    }

    pub fn to_json(&self) -> String {
        let mut root = serde_json::Map::new();
        root.insert("name".into(), self.name.clone().into());
        if let Some(light) = self.light {
            root.insert("appearance".into(), if light { "light" } else { "dark" }.into());
        }
        let values: serde_json::Map<String, serde_json::Value> =
            self.values.iter().map(|(k, v)| (k.clone(), v.clone().into())).collect();
        root.insert("values".into(), values.into());
        serde_json::to_string_pretty(&serde_json::Value::Object(root)).unwrap_or_default() + "\n"
    }
}

/// A theme with the variant `variant` chosen (or its own values with `None`), and warnings
/// about values it could not use.
pub fn with_variant(theme: &Theme, variant: Option<&Variant>) -> (Theme, Vec<String>) {
    let mut chosen = theme.clone();
    chosen.vars.values = variant.map(|v| v.values.clone()).unwrap_or_default();
    chosen.vars.variant = variant.map(|v| v.id.clone());
    chosen.vars.variant_name = variant.map(|v| v.name.clone());
    chosen.vars.light = variant.and_then(|v| v.light);
    let warnings = resolve(&mut chosen);
    (chosen, warnings)
}

impl Variant {
    /// The variant `theme` has chosen, as its file holds it.
    pub fn of(theme: &Theme) -> Option<Self> {
        let id = theme.vars.variant.clone()?;
        Some(Self {
            name: theme.vars.variant_name.clone().unwrap_or_else(|| id.clone()),
            id,
            light: theme.vars.light,
            values: theme.vars.values.clone(),
        })
    }
}

/// The theme and variant an entry's id names: `nord` is the theme `nord` with its own values,
/// `nord@frost` its variant `frost`.
pub fn split_id(id: &str) -> (&str, Option<&str>) {
    match id.split_once('@') {
        Some((theme, variant)) => (theme, Some(variant)),
        None => (id, None),
    }
}

#[cfg(test)]
#[path = "theme_vars_tests.rs"]
mod tests;
