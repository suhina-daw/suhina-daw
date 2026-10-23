// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn every_theme_that_comes_with_suhina_sets_every_token() {
    let tokens: Vec<&str> = Palette::DARK.tokens().iter().map(|(key, _, _)| *key).collect();
    for (id, text) in BUILT_IN_FILES {
        let (_, warnings) = Theme::from_json(text);
        assert!(warnings.is_empty(), "{id}: {warnings:?}");
        let colors = match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(root)) => root.get("colors").and_then(Value::as_object).cloned().unwrap_or_default(),
            _ => Map::new(),
        };
        for token in &tokens {
            assert!(colors.contains_key(*token), "{id} leaves out {token}");
        }
    }
}

#[test]
fn track_and_kind_colours_keep_their_values_in_suhina_dark() {
    let dark = Palette::DARK;
    let tracks = [
        dark.track_color_1,
        dark.track_color_2,
        dark.track_color_3,
        dark.track_color_4,
        dark.track_color_5,
        dark.track_color_6,
        dark.track_color_7,
        dark.track_color_8,
    ];
    let expected = [0xFF9A3C, 0xF2D541, 0x4FC3F7, 0xB38BFF, 0x7FDB5A, 0xFF6FA8, 0x5C8DFF, 0x2FD9C4].map(rgb);
    assert_eq!(tracks, expected);
    assert_eq!((dark.kind_kick, dark.kind_other), (rgb(0xD06E5E), rgb(0x8A8680)));
    // A theme can change them like any other token.
    let (theme, warnings) = Theme::from_json(r##"{ "colors": { "track.1": "#010203", "kind.snare": "#040506" } }"##);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!((theme.colors.track_color_1, theme.colors.kind_snare), (rgb(0x010203), rgb(0x040506)));
}

#[test]
fn a_theme_survives_a_round_trip() {
    let mut theme = Theme { name: "Night".into(), colors: Palette::DARK };
    theme.colors.accent = rgb(0x12AB9F);
    theme.colors.window_background = rgb(0x000000);
    assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
}

#[test]
fn a_theme_file_needs_only_the_colours_it_changes() {
    let (theme, warnings) = Theme::from_json(r##"{ "name": "Blue", "colors": { "accent": "#3366ff" } }"##);
    assert!(warnings.is_empty());
    assert_eq!(theme.name, "Blue");
    assert_eq!(theme.colors.accent, rgb(0x3366FF));
    assert_eq!(theme.colors.window_background, Palette::DARK.window_background);
}

#[test]
fn bad_values_keep_the_built_in_colours() {
    let text =
        r##"{ "name": 3, "colors": { "accent": "blue", "surface.9": "#000000", "border": "#12345" }, "font": 1 }"##;
    let (theme, warnings) = Theme::from_json(text);
    assert_eq!(theme, Theme::default());
    assert_eq!(warnings.len(), 5, "{warnings:?}");
    assert_eq!(Theme::from_json("[1, 2]").1.len(), 1);
    assert_eq!(Theme::from_json("{ not json").0, Theme::default());
}

#[test]
fn colours_are_written_and_read_as_hex() {
    assert_eq!(hex(rgb(0x1D1C1A)), "#1D1C1A");
    assert_eq!(parse_hex("#a6a3ff"), Some(rgb(0xA6A3FF)));
    assert_eq!(parse_hex("A6A3FF"), Some(rgb(0xA6A3FF)));
    assert_eq!(parse_hex("#A6A3F"), None);
    assert_eq!(parse_hex("#GGGGGG"), None);
    assert_eq!(parse_hex("#+1+2+3"), None);
}

#[test]
fn files_with_the_first_token_names_still_read() {
    let (theme, warnings) =
        Theme::from_json(r##"{ "colors": { "surface.0": "#010203", "line": "#040506", "accent": "#070809" } }"##);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(theme.colors.window_background, rgb(0x010203));
    assert_eq!(theme.colors.border, rgb(0x040506));
    assert_eq!(theme.colors.accent, rgb(0x070809));
    let mut copy = Palette::DARK;
    for (old, new) in RENAMED {
        assert!(copy.token_mut(new).is_some(), "{new} is not a token");
        assert!(Palette::DARK.tokens().iter().all(|(key, _, _)| *key != old), "{old} is still a token");
    }
}

#[test]
fn the_themes_that_come_with_suhina_read_whole() {
    let themes = built_in_themes();
    assert_eq!(themes.first().map(|(id, _)| *id), Some(BUILT_IN));
    for (id, text) in BUILT_IN_FILES {
        let (theme, warnings) = Theme::from_json(text);
        assert!(warnings.is_empty(), "{id}: {warnings:?}");
        // Every colour is written out, so none falls back to Suhina Dark by mistake.
        let listed = serde_json::from_str::<Value>(text).ok().and_then(|v| v.get("colors").cloned());
        assert_eq!(listed.and_then(|c| c.as_object().map(Map::len)), Some(Palette::DARK.tokens().len()), "{id}");
        assert!(is_built_in(id));
        assert_ne!(theme.name, Theme::default().name);
    }
    assert!(!is_built_in("night"));
}

#[test]
fn the_readme_names_every_token() {
    let readme = template_readme();
    for (key, purpose, color) in Palette::DARK.tokens() {
        assert!(readme.contains(&format!("| `{key}` | `{}` | {purpose} |", hex(color))), "{key}");
    }
}

#[test]
fn every_token_has_its_own_name() {
    let tokens = Palette::DARK.tokens();
    let mut keys: Vec<_> = tokens.iter().map(|(key, _, _)| *key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), tokens.len());
}

#[test]
fn text_on_a_colour_is_the_text_token_that_stands_out_more() {
    // The default track colours are light: names on them are dark.
    for fill in [Palette::DARK.track_color_1, Palette::DARK.track_color_8] {
        assert_eq!(text_on(fill), window_background(), "{fill:?}");
        assert!(contrast(fill, text_on(fill)) >= 4.5, "{fill:?}");
    }
    assert_eq!(text_on(rgb(0x202020)), text_primary());
    assert!((contrast(rgb(0x000000), rgb(0xFFFFFF)) - 21.0).abs() < 0.01);
}

#[test]
fn the_palette_in_use_changes_with_the_theme() {
    let ctx = egui::Context::default();
    let mut night = Palette::DARK;
    night.accent = rgb(0x00FF00);
    set_palette(&ctx, night);
    assert_eq!(accent(), rgb(0x00FF00));
    set_palette(&ctx, Palette::DARK);
    assert_eq!(accent(), Palette::DARK.accent);
}
