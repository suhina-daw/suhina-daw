// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A documented, bounded CSS subset for native controls. No browser or executable content.
use super::{skin, theme::Theme};
use serde_json::json;

pub use suhina_style::edit as source;

pub const MAX_CSS: usize = 128 * 1024;

/// Apply declarations in source order. Unsupported selectors/properties are visible warnings.
pub fn apply(theme: &mut Theme, css: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    if css.len() > MAX_CSS {
        return vec!["stylesheet exceeds 128 KiB".into()];
    }
    let mut source = String::new();
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        source.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("*/") else {
            return vec!["unterminated CSS comment".into()];
        };
        rest = &rest[start + 2 + end + 2..];
    }
    source.push_str(rest);
    let mut rest = source.as_str();
    let mut rules = 0;
    while !rest.trim().is_empty() {
        rules += 1;
        if rules > 128 {
            warnings.push("stylesheet exceeds 128 rules".into());
            break;
        }
        let Some((selector, after)) = rest.split_once('{') else {
            warnings.push("CSS rule needs {".into());
            break;
        };
        let Some((body, after)) = after.split_once('}') else {
            warnings.push("CSS rule needs }".into());
            break;
        };
        rest = after;
        let selector = selector.trim();
        let control_selector = selector.trim_start_matches('.');
        for declaration in body.split(';').filter(|s| !s.trim().is_empty()) {
            let Some((property, value)) = declaration.split_once(':') else {
                warnings.push(format!("{selector}: expected property: value"));
                continue;
            };
            let property = property.trim();
            let value = value.trim();
            let result = match selector {
                "session" | ".session" if super::session_geometry::SessionGeometry::limits(property).is_some() => {
                    theme.skin.session.set(property, value)
                }
                "artwork" if property == "fps" => match value.parse::<u8>().ok().filter(|fps| (1..=30).contains(fps)) {
                    Some(fps) => {
                        theme.skin.artwork_fps = fps;
                        Ok(())
                    }
                    None => Err("artwork fps must be 1..30"),
                },
                _ if selector.starts_with(".control-") => {
                    let name = selector.trim_start_matches(".control-");
                    if !super::control_layout::valid_name(name)
                        || (!theme.skin.controls.contains_key(name) && theme.skin.controls.len() >= 256)
                    {
                        Err("invalid or excessive native control placement")
                    } else {
                        let mut next = theme.skin.controls.get(name).copied().unwrap_or_default();
                        match next.set(property, value) {
                            Ok(()) => {
                                theme.skin.controls.insert(name.into(), next);
                                Ok(())
                            }
                            Err(error) => Err(error),
                        }
                    }
                }
                ":root" if property.starts_with("--") => {
                    let key = property.trim_start_matches("--").replace('-', ".");
                    match (theme.colors.token_mut(&key), super::theme::parse_rgba(value)) {
                        (Some(slot), Some(rgba)) => {
                            *slot = egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
                            theme.transparent_rgb.remove(&key);
                            if rgba[3] == 0 && rgba[..3] != [0, 0, 0] {
                                theme.transparent_rgb.insert(key, [rgba[0], rgba[1], rgba[2]]);
                            }
                            Ok(())
                        }
                        _ => Err("unknown colour token or invalid hex colour"),
                    }
                }
                ".window" | "window"
                    if matches!(property, "background-image" | "background-opacity" | "background-blur") =>
                {
                    match property {
                        "background-image" if value == "none" => {
                            theme.skin.background = None;
                            Ok(())
                        }
                        "background-image" => {
                            if let Some(path) = value.strip_prefix("url(").and_then(|s| s.strip_suffix(')')) {
                                let path = path.trim().trim_matches(['\'', '"']);
                                if path.ends_with(".png") && skin::asset_path(&path.replace(".png", ".svg")) {
                                    theme.skin.background = Some(std::sync::Arc::from(path));
                                    Ok(())
                                } else {
                                    Err("background-image must name a relative PNG")
                                }
                            } else {
                                Err("background-image requires url()")
                            }
                        }
                        _ => {
                            if let Some(n) = value.trim_end_matches('%').parse::<u8>().ok().filter(|n| *n <= 100) {
                                if property == "background-opacity" {
                                    theme.skin.background_opacity = n;
                                } else {
                                    theme.skin.background_blur = n;
                                }
                                Ok(())
                            } else {
                                Err("background opacity/blur must be 0..100")
                            }
                        }
                    }
                }
                "meter" | ".meter" if matches!(property, "style" | "segments" | "intensity") => match property {
                    "intensity" => {
                        match value.strip_suffix('%').unwrap_or(value).parse::<u8>().ok().filter(|n| *n <= 100) {
                            Some(n) => {
                                theme.skin.meter_intensity = n;
                                Ok(())
                            }
                            None => Err("meter intensity must be 0..100%"),
                        }
                    }
                    "style" => match value {
                        "continuous" => {
                            theme.skin.meter = skin::MeterStyle::Continuous;
                            Ok(())
                        }
                        "segmented" => {
                            theme.skin.meter = skin::MeterStyle::Segmented;
                            Ok(())
                        }
                        "needle" => {
                            theme.skin.meter = skin::MeterStyle::Needle;
                            Ok(())
                        }
                        "frames" => {
                            theme.skin.meter = skin::MeterStyle::Frames;
                            Ok(())
                        }
                        _ => Err("unknown meter style"),
                    },
                    "segments" => match value.parse::<u8>().ok().filter(|n| (4..=64).contains(n)) {
                        Some(n) => {
                            theme.skin.segments = n;
                            Ok(())
                        }
                        None => Err("segments must be 4..64"),
                    },
                    _ => Err("unsupported meter property"),
                },
                "toolbar" | ".toolbar" if property.starts_with("offset-") => {
                    let module = property.trim_start_matches("offset-");
                    match value.trim_end_matches("px").parse::<i32>() {
                        Ok(offset) if skin::MODULES.contains(&module) && (-16384..=16384).contains(&offset) => {
                            theme.skin.toolbar_offsets.insert(module.into(), offset);
                            Ok(())
                        }
                        _ => Err("expected a known module offset from -16384 to 16384"),
                    }
                }
                "toolbar" | ".toolbar" if matches!(property, "order" | "hidden") => {
                    let list: Vec<_> =
                        value.split(',').map(str::trim).filter(|s| !s.is_empty() && *s != "none").collect();
                    if list.len() > skin::MODULES.len() || list.iter().any(|s| !skin::MODULES.contains(s)) {
                        Err("unknown toolbar module")
                    } else {
                        let current = theme.skin.toolbar.get_or_insert_with(Default::default);
                        let mut v = json!({"order":current.order,"hidden":current.hidden});
                        v[property] = json!(list);
                        let parsed = skin::Skin::from_json(&json!({"toolbar":v}), &mut warnings);
                        theme.skin.toolbar = parsed.toolbar;
                        Ok(())
                    }
                }
                _ if property == "image" => {
                    let role = control_selector.replace(':', ".").replace(" .", ".");
                    if !skin::valid_role(&role) {
                        Err("unsupported control selector")
                    } else if value == "none" {
                        theme.skin.assets.remove(&role);
                        Ok(())
                    } else if let Some(path) = value.strip_prefix("url(").and_then(|v| v.strip_suffix(')')) {
                        let path = path.trim().trim_matches(['\'', '"']);
                        if skin::asset_path(path) {
                            theme.skin.assets.insert(role, path.into());
                            Ok(())
                        } else {
                            Err("image must name a relative SVG file")
                        }
                    } else {
                        Err("image requires url(relative.svg) or none")
                    }
                }
                _ if super::style_class::valid_selector(selector)
                    && super::style_class::valid_property(property, value) =>
                {
                    theme
                        .skin
                        .styles
                        .entry(selector.to_string())
                        .or_default()
                        .insert(property.to_string(), value.to_string());
                    Ok(())
                }
                _ => Err("unsupported selector or property"),
            };
            if let Err(reason) = result {
                warnings.push(format!("CSS {selector} {{ {property}: {value} }}: {reason}"));
            }
        }
    }
    warnings
}

/// Canonical CSS is generated from the same model the theme editor changes.
pub fn export(theme: &Theme) -> String {
    let mut css = String::from("/* Suhina native theme CSS. See README.md for supported properties. */\n:root {\n");
    for (key, _, color) in theme.colors.tokens() {
        css.push_str(&format!("  --{}: {};\n", key.replace('.', "-"), super::theme::hex_rgba(theme.rgba(key, color))));
    }
    css.push_str(&format!(
        "}}\nmeter {{ style: {}; segments: {}; intensity: {}%; }}\n",
        theme.skin.meter.name(),
        theme.skin.segments,
        theme.skin.meter_intensity
    ));
    css.push_str(&format!("artwork {{ fps: {}; }}\n", theme.skin.artwork_fps));
    for (name, value) in theme.skin.session.entries() {
        if let Some(value) = value {
            css.push_str(&format!("session {{ {name}: {value}px; }}\n"));
        }
    }
    for (name, placement) in &theme.skin.controls {
        css.push_str(&format!(".control-{name} {{ left: {}px; top: {}px;", placement.offset().x, placement.offset().y));
        css.push_str(&format!(
            " bottom: {}; display: {};",
            placement.bottom.map_or("auto".into(), |v| format!("{v}px")),
            if placement.hidden { "none" } else { "block" }
        ));
        css.push_str(&format!(" right: {};", placement.right.map_or("auto".into(), |v| format!("{v}px"))));
        for (key, value) in [("width", placement.width), ("height", placement.height)] {
            if key == "width" && placement.full_width {
                css.push_str(" width: 100%;");
                continue;
            }
            if key == "height" && placement.full_height {
                css.push_str(" height: 100%;");
                continue;
            }
            css.push_str(&format!(" {key}: {};", value.map_or("auto".into(), |v| format!("{v}px"))));
        }
        css.push_str(&format!(
            " --suhina-direction: {}; }}\n",
            if placement.direction == super::control_layout::Direction::Horizontal { "horizontal" } else { "vertical" }
        ));
    }
    if let Some(toolbar) = &theme.skin.toolbar {
        css.push_str(&format!(
            "toolbar {{ order: {}; hidden: {}; }}\n",
            toolbar.order.join(", "),
            if toolbar.hidden.is_empty() { "none".into() } else { toolbar.hidden.join(", ") }
        ));
    }
    for (module, offset) in &theme.skin.toolbar_offsets {
        css.push_str(&format!("toolbar {{ offset-{module}: {offset}px; }}\n"));
    }
    if theme.skin.background.is_some() {
        css.push_str(&format!(
            ".window {{ background-image: url(\"background.png\"); background-opacity: {}; background-blur: {}; }}\n",
            theme.skin.background_opacity, theme.skin.background_blur
        ));
    }
    for (selector, properties) in &theme.skin.styles {
        css.push_str(&format!("{selector} {{"));
        for (key, value) in properties {
            css.push_str(&format!(" {key}: {value};"));
        }
        css.push_str(" }\n");
    }
    for (role, asset) in &theme.skin.assets {
        // Package export writes each inline SVG under this stable relative name.
        let path = if asset.trim_start().starts_with('<') || skin::png_artwork(asset) {
            format!("assets/{role}.{}", skin::artwork_extension(asset))
        } else {
            asset.clone()
        };
        css.push_str(&format!(
            "{} {{ image: url(\"{path}\"); }}\n",
            role.replace(".hover", ":hover").replace(".active", ":active").replace(".disabled", ":disabled"),
        ));
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_geometry_round_trips_between_css_and_json_with_hostile_values_bounded() {
        let mut theme = Theme::default();
        let css = ".control-mixer-volume { left: 8.25px; width: 90px; height: 20px; --suhina-direction: horizontal; } .control-mixer-name { bottom: 0px; } .control-mixer-color { display: none; }";
        assert!(apply(&mut theme, css).is_empty());
        assert_eq!(theme.skin.controls["mixer-volume"].direction, super::super::control_layout::Direction::Horizontal);
        assert_eq!(theme.skin.controls["mixer-name"].bottom, Some(0));
        assert!(theme.skin.controls["mixer-color"].hidden);
        let mut restored = Theme::default();
        assert!(apply(&mut restored, &export(&theme)).is_empty());
        assert_eq!(restored, theme);
        let mut warnings = Vec::new();
        assert_eq!(skin::Skin::from_json(&theme.skin.to_json(), &mut warnings), theme.skin);
        assert!(warnings.is_empty());
        let before = theme.skin.controls.clone();
        assert_eq!(
            apply(&mut theme, ".control-mixer-volume { width: 999999px; top: NaNpx; --suhina-direction: sideways; }")
                .len(),
            3
        );
        assert_eq!(theme.skin.controls, before);
    }
    #[test]
    fn toolbar_offsets_mirror_without_replacing_the_native_layout() {
        let mut theme = Theme::default();
        assert!(apply(&mut theme, "toolbar { offset-transport: 80px; offset-views: -16px; }").is_empty());
        assert!(theme.skin.toolbar.is_none());
        assert_eq!(theme.skin.toolbar_offsets.get("transport"), Some(&80));
        let mut restored = Theme::default();
        assert!(apply(&mut restored, &export(&theme)).is_empty());
        assert_eq!(restored.skin.toolbar_offsets, theme.skin.toolbar_offsets);
        assert!(
            !apply(&mut restored, "toolbar { offset-transport: NaN; offset-views: 999999px; offset-bogus: 1px; }")
                .is_empty()
        );
    }
    #[test]
    fn meter_intensity_is_portable_bounded_and_defaults_to_full_strength() {
        for intensity in [0, 35, 100] {
            let mut theme = Theme::default();
            assert!(apply(&mut theme, &format!("meter {{ intensity: {intensity}%; }}")).is_empty());
            assert_eq!(theme.skin.meter_intensity, intensity);
            let (restored, warnings) = Theme::from_json(&theme.to_json());
            assert!(warnings.is_empty());
            assert_eq!(restored, theme);
            let mut css = Theme::default();
            assert!(apply(&mut css, &export(&theme)).is_empty());
            assert_eq!(css, theme);
        }
        for bad in ["-1", "101%", "NaN", "infinity", "0.5", "1%%"] {
            let mut theme = Theme::default();
            assert!(!apply(&mut theme, &format!("meter {{ intensity: {bad}; }}")).is_empty(), "{bad}");
            assert_eq!(theme.skin.meter_intensity, 100);
        }
        for bad in [json!(-1), json!(101), json!(0.5), json!("NaN"), json!(null)] {
            let mut warnings = Vec::new();
            let skin = skin::Skin::from_json(&json!({"meter-intensity": bad}), &mut warnings);
            assert_eq!(skin.meter_intensity, 100);
            assert_eq!(warnings.len(), 1);
        }
    }
    #[test]
    fn measured_meter_frames_mirror_css_and_json_without_changing_artwork_paths() {
        let mut theme = Theme::default();
        assert!(apply(&mut theme, "meter { style: frames; } meter.frame-0 { image: url('assets/silent.png'); } meter.frame-1 { image: url('assets/loud.svg'); }").is_empty());
        assert_eq!(theme.skin.meter, skin::MeterStyle::Frames);
        let (json, warnings) = Theme::from_json(&theme.to_json());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(json, theme);
        let mut restored = Theme::default();
        assert!(apply(&mut restored, &export(&theme)).is_empty());
        assert_eq!(restored, theme);
    }
    #[test]
    fn css_changes_native_model_and_reports_unsupported_rules() {
        let mut theme = Theme::default();
        let warnings = apply(
            &mut theme,
            "/* own style */ :root { --accent: #123456; } meter {style: segmented; segments: 16;} toolbar {order: transport, tempo; hidden: performance;} fader.cap:hover {image: url('assets/cap.svg');} body {display: grid;}",
        );
        assert_eq!(theme.colors.accent, egui::Color32::from_rgb(0x12, 0x34, 0x56));
        assert_eq!(theme.skin.meter, skin::MeterStyle::Segmented);
        assert_eq!(theme.skin.toolbar.as_ref().unwrap().order[0], "transport");
        assert_eq!(theme.skin.assets["fader.cap.hover"], "assets/cap.svg");
        assert_eq!(warnings.len(), 1);
        let mut restored = Theme::default();
        assert!(apply(&mut restored, &export(&theme)).is_empty());
        assert_eq!(restored, theme);
    }
    #[test]
    fn malformed_css_does_not_panic_or_load_resources() {
        for css in [
            "/*",
            "meter {",
            "@import 'https://example.test/style';",
            "play {image:url(../../x.svg);}",
            "meter {segments: 999999;}",
        ] {
            assert!(!apply(&mut Theme::default(), css).is_empty(), "{css}");
        }
    }
}
