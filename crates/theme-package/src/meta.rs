// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `theme.json`, a theme's metadata (`docs/spec/theme-skins.md` › Theme folders): the format
//! version, name, author, description, the default variant's name, the swatch colour and an
//! optional suggested toolbar arrangement. The theme's name comes from here, not from its CSS.
//!
//! ```json
//! {
//!   "format": 1,
//!   "name": "Night",
//!   "author": "Someone",
//!   "description": "Dark blue with an orange accent",
//!   "default-variant": "Dark",
//!   "swatch": "color-mix(in oklab, var(--window-background), var(--accent) 30%)"
//! }
//! ```
//!
//! The swatch is any CSS `<color>` value. It is resolved for each variant from the theme's
//! `@property` rules and `:root` declarations alone ([`suhina_style::cascade::resolve_color`]),
//! never by applying the sheet, so `currentColor`, `attr()` and units relative to an element are
//! refused.

use suhina_style::cascade::{AppliedSheet, Layer};
use suhina_style::media::Viewport;
use suhina_style::sheet::Sheet;

/// The metadata file's name at a theme's root.
pub const FILE: &str = "theme.json";
/// The theme's optional thumbnail at its root.
pub const THUMBNAIL: &str = "thumbnail.svg";
/// The `format` this build writes and reads.
pub const FORMAT: u64 = 1;
/// What the default variant is called when `theme.json` names none.
pub const DEFAULT_VARIANT: &str = "Default";
/// A name, an author, a variant's name: at most this many characters.
const MAX_SHORT: usize = 64;
/// A description: at most this many characters.
const MAX_DESCRIPTION: usize = 500;
/// A swatch value: at most this many bytes.
const MAX_SWATCH: usize = 512;
/// How a theme is checked when there is no window to ask: the size `theme_vars` reads a sheet at.
pub const CHECK_VIEWPORT: Viewport = Viewport { width: 1280.0, height: 800.0, reduced_motion: false };

/// A theme's `theme.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeMeta {
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    /// The default variant's name: the theme's own values, which are not a file in `variants/`.
    pub default_variant: Option<String>,
    /// A CSS `<color>` value.
    pub swatch: String,
    /// The suggested toolbar arrangement, kept as written (#1021 reads it).
    pub toolbar: Option<serde_json::Value>,
    /// Keys the file has that Suhina does not read (`$schema`, or those of a newer feature), kept
    /// so that rewriting the file does not drop them.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ThemeMeta {
    /// A theme named `name` whose swatch is `swatch`, with nothing else said.
    pub fn new(name: &str, swatch: &str) -> Self {
        Self {
            name: name.to_owned(),
            author: None,
            description: None,
            default_variant: None,
            swatch: swatch.to_owned(),
            toolbar: None,
            extra: serde_json::Map::new(),
        }
    }

    /// The default variant's name, `Default` when the file names none.
    pub fn default_variant(&self) -> &str {
        self.default_variant.as_deref().unwrap_or(DEFAULT_VARIANT)
    }

    /// Reads `theme.json`, with warnings about what it ignored. A file without a name or a
    /// swatch, or of a newer format, is refused.
    pub fn parse(bytes: &[u8]) -> Result<(Self, Vec<String>), String> {
        let root: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("{FILE}: not JSON: {e}"))?;
        let root = root.as_object().ok_or(format!("{FILE}: a JSON object"))?;
        match root.get("format").and_then(serde_json::Value::as_u64) {
            Some(FORMAT) => {}
            Some(newer) if newer > FORMAT => {
                return Err(format!("{FILE}: format {newer} is for a newer Suhina; this one reads format {FORMAT}"));
            }
            _ => return Err(format!("{FILE}: \"format\": {FORMAT} is missing")),
        }
        let mut warnings = Vec::new();
        let text = |key: &str, max: usize, warnings: &mut Vec<String>| -> Option<String> {
            match root.get(key)? {
                serde_json::Value::String(value) => {
                    let value = value.trim();
                    if value.is_empty() {
                        None
                    } else if value.chars().count() > max || value.chars().any(|c| c.is_control() && c != '\n') {
                        warnings.push(format!("{FILE}: {key} is text of at most {max} characters"));
                        None
                    } else {
                        Some(value.to_owned())
                    }
                }
                _ => {
                    warnings.push(format!("{FILE}: {key} is not text"));
                    None
                }
            }
        };
        let name = text("name", MAX_SHORT, &mut warnings).ok_or(format!("{FILE}: the theme has no name"))?;
        let author = text("author", MAX_SHORT, &mut warnings);
        let description = text("description", MAX_DESCRIPTION, &mut warnings);
        let default_variant = text("default-variant", MAX_SHORT, &mut warnings);
        let swatch = match root.get("swatch") {
            Some(serde_json::Value::String(swatch)) if !swatch.trim().is_empty() => swatch.trim().to_owned(),
            _ => return Err(format!("{FILE}: the theme has no swatch colour")),
        };
        if swatch.len() > MAX_SWATCH || swatch.contains([';', '{', '}']) {
            return Err(format!("{FILE}: the swatch is not a colour value"));
        }
        let toolbar = match root.get("toolbar") {
            Some(value @ serde_json::Value::Object(_)) => Some(value.clone()),
            Some(_) => {
                warnings.push(format!("{FILE}: toolbar is not an object"));
                None
            }
            None => None,
        };
        let known = ["format", "name", "author", "description", "default-variant", "swatch", "toolbar"];
        let mut extra = serde_json::Map::new();
        for (key, value) in root.iter().filter(|(key, _)| !known.contains(&key.as_str())) {
            if key != "$schema" {
                warnings.push(format!("{FILE}: {key} is not part of theme.json"));
            }
            extra.insert(key.clone(), value.clone());
        }
        Ok((Self { name, author, description, default_variant, swatch, toolbar, extra }, warnings))
    }

    /// The file's text, as Suhina writes it.
    pub fn to_json(&self) -> String {
        let mut root = self.extra.clone();
        root.insert("format".into(), FORMAT.into());
        root.insert("name".into(), self.name.clone().into());
        for (key, value) in
            [("author", &self.author), ("description", &self.description), ("default-variant", &self.default_variant)]
        {
            if let Some(value) = value {
                root.insert(key.into(), value.clone().into());
            }
        }
        root.insert("swatch".into(), self.swatch.clone().into());
        if let Some(toolbar) = &self.toolbar {
            root.insert("toolbar".into(), toolbar.clone());
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(root)).unwrap_or_default() + "\n"
    }
}

/// A variant file's values as the `:root` rule they set, from `variants/<id>.json`'s text.
pub fn variant_sheet(text: &[u8]) -> Result<String, String> {
    let root: serde_json::Value = serde_json::from_slice(text).map_err(|e| format!("not JSON: {e}"))?;
    let mut css = String::from(":root {");
    if let Some(values) = root.get("values").and_then(serde_json::Value::as_object) {
        for (name, value) in values {
            if let Some(value) = value.as_str()
                && name.starts_with("--")
                && !value.contains([';', '{', '}'])
            {
                css.push_str(&format!(" {name}: {value};"));
            }
        }
    }
    css.push_str(" }");
    Ok(css)
}

/// The swatch's colour over `defaults` (sheets of the host's own, such as its registered
/// properties), the theme's sheet `css` and a variant's sheet: `var()` takes the root values the
/// sheets give, without applying them.
pub fn swatch_color(
    swatch: &str,
    defaults: &[&Sheet],
    theme: &Sheet,
    variant: Option<&Sheet>,
) -> Result<[u8; 4], String> {
    swatch_color_at(swatch, defaults, theme, variant, CHECK_VIEWPORT)
}

/// [`swatch_color`] as a window of `viewport` sees it (`@media` rules that change `:root`).
pub fn swatch_color_at(
    swatch: &str,
    defaults: &[&Sheet],
    theme: &Sheet,
    variant: Option<&Sheet>,
    viewport: Viewport,
) -> Result<[u8; 4], String> {
    let mut sheets: Vec<AppliedSheet<'_>> =
        defaults.iter().map(|sheet| AppliedSheet { sheet, layer: Layer::UserAgent, root: None }).collect();
    sheets.push(AppliedSheet { sheet: theme, layer: Layer::Theme, root: None });
    sheets.extend(variant.map(|sheet| AppliedSheet { sheet, layer: Layer::Variant, root: None }));
    suhina_style::cascade::resolve_color(swatch, viewport, &sheets).map_err(|error| {
        if swatch.contains("var(") && error.contains("ParseError") {
            format!("{swatch} does not resolve to a colour: a var() it uses has no colour value")
        } else {
            error
        }
    })
}

/// Checks a thumbnail's SVG: self-contained vector artwork (no scripts, embedded images, foreign
/// objects or links) whose root says a 16:9 picture.
pub fn check_thumbnail(source: &str) -> Result<(), String> {
    let doc = svg(source).map_err(|e| format!("{THUMBNAIL}: {e}"))?;
    is_sixteen_by_nine(doc.root_element())
}

/// An SVG document after the artwork checks a theme's SVGs get.
pub fn svg(source: &str) -> Result<roxmltree::Document<'_>, String> {
    let options = roxmltree::ParsingOptions { allow_dtd: false, nodes_limit: 100_000, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(source, options).map_err(|e| e.to_string())?;
    let root = doc.root_element().tag_name();
    if root.name() != "svg" || !matches!(root.namespace(), None | Some("http://www.w3.org/2000/svg")) {
        return Err("not an SVG image".into());
    }
    let lower = source.to_ascii_lowercase();
    if ["<script", "<image", "<foreignobject"].iter().any(|s| lower.contains(s)) || has_href(&lower) {
        return Err("an SVG holds only self-contained vector artwork".into());
    }
    Ok(doc)
}

/// Whether `lower` (lower case) names a link: `href`, then any space, then `=`, as the app's own check refuses
/// both `href=` and `href =`.
fn has_href(lower: &str) -> bool {
    lower.match_indices("href").any(|(at, _)| lower[at + 4..].trim_start().starts_with('='))
}

/// Whether an SVG's root says a 16:9 picture, by its `viewBox` or else its `width` and `height`.
pub fn is_sixteen_by_nine(root: roxmltree::Node<'_, '_>) -> Result<(), String> {
    let number =
        |text: &str| text.trim().trim_end_matches("px").parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0);
    let size = root
        .attribute("viewBox")
        .and_then(|view| {
            let parts: Vec<f64> = view
                .split([' ', ','])
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok().filter(|n: &f64| n.is_finite()))
                .collect();
            (parts.len() == 4 && parts[2] > 0.0 && parts[3] > 0.0).then(|| (parts[2], parts[3]))
        })
        .or_else(|| Some((number(root.attribute("width")?)?, number(root.attribute("height")?)?)))
        .ok_or(format!("{THUMBNAIL}: needs a viewBox, or a width and height, to say its size"))?;
    let ratio = size.0 / size.1;
    if (ratio / (16.0 / 9.0) - 1.0).abs() > 0.01 {
        return Err(format!("{THUMBNAIL}: is {}×{}; a thumbnail is 16:9", size.0, size.1));
    }
    Ok(())
}
