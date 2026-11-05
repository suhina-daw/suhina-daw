// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn transparent_rgb_survives_theme_round_trip() {
    let (theme, warnings) = Theme::from_json(r##"{"colors":{"text.primary":"#FF804000"}}"##);
    assert!(warnings.is_empty());
    assert_eq!(theme.rgba("text.primary", theme.colors.text_primary), [255, 128, 64, 0]);
    let (restored, warnings) = Theme::from_json(&theme.to_json());
    assert!(warnings.is_empty());
    assert_eq!(restored, theme);
}

#[test]
fn legacy_and_inherited_transparent_colours_keep_rgb_when_saved() {
    let (theme, warnings) = Theme::from_json(r##"{"colors":{"surface.0":"#FF804000","text.primary":"#20A04000"}}"##);
    assert!(warnings.is_empty());
    let saved: Value = serde_json::from_str(&theme.to_json()).unwrap();
    assert_eq!(saved["colors"]["window.background"], "#FF804000");
    // Text inherits the background's colour but never its alpha: it would vanish.
    assert_eq!(saved["colors"]["text.onColorDark"], "#FF8040");
    for key in ["text.primary", "browser.text.primary", "text.onColorLight"] {
        assert_eq!(saved["colors"][key], "#20A04000");
    }
    assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
    let (theme, warnings) =
        Theme::from_json(r##"{"colors":{"surface.0":"#FF804000","window.background":"#00000000"}}"##);
    assert!(warnings.is_empty());
    assert_eq!(theme.rgba("window.background", theme.colors.window_background), [0, 0, 0, 0]);
}

#[test]
fn translucent_theme_colours_preserve_alpha_in_files() {
    for colour in [Color32::from_rgba_unmultiplied(100, 150, 200, 128), Color32::TRANSPARENT, rgb(0xAABBCC)] {
        assert_eq!(parse_hex(&hex(colour)), Some(colour));
        let mut theme = Theme::default();
        theme.colors.grid_major = colour;
        assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
    }
    assert!(parse_hex("#1234567").is_none());
    assert!(parse_hex("#112233GG").is_none());
}

#[test]
fn old_clip_defaults_migrate_and_explicit_defaults_and_materials_round_trip() {
    let (old, warnings) = Theme::from_json(r##"{"colors":{"track.1":"#123456","track.3":"#ABCDEF"}}"##);
    assert!(warnings.is_empty());
    assert_eq!(old.colors.audio_clip_default, rgb(0x123456));
    assert_eq!(old.colors.midi_clip_default, rgb(0xABCDEF));
    assert_eq!(old.liquid, None);
    let (theme, warnings) = Theme::from_json(
        r##"{"colors":{"clip.midiDefault":"#010203","clip.audioDefault":"#040506"},"liquid":{"panels":38,"tracks":52,"clips":40}}"##,
    );
    assert!(warnings.is_empty());
    assert_eq!(theme.colors.midi_clip_default, rgb(0x010203));
    assert_eq!(theme.colors.audio_clip_default, rgb(0x040506));
    assert_eq!(theme.liquid, Some(super::super::liquid::Options { panels: 38, tracks: 52, clips: 40 }));
    assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
    let (invalid, warnings) = Theme::from_json(r#"{"liquid":{"panels":999,"tracks":-1,"clips":"bad"}}"#);
    assert_eq!(warnings.len(), 3);
    assert_eq!(invalid.liquid, Some(super::super::liquid::Options::default()));
}

#[test]
fn example_themes_embed_their_paint_and_all_editable_artwork() {
    for (id, width) in [("suhina-sheet", 88), ("suhina-meadow", 92), ("suhina-blocks", 92)] {
        let (_, sheet) = built_in_themes().into_iter().find(|(key, _)| *key == id).unwrap();
        assert!(sheet.skin.assets.len() >= super::super::skin::ROLES.len());
        for role in super::super::skin::ROLES {
            let svg = &sheet.skin.assets[*role];
            assert!(super::super::skin::validate_artwork(svg).is_ok(), "{role}");
        }
        for (role, svg) in &sheet.skin.assets {
            assert!(super::super::skin::validate_artwork(svg).is_ok(), "{role}");
        }
        assert!(super::super::native_css::inspect(sheet.skin.css.as_deref().unwrap()).unwrap().is_empty());
        assert_eq!(sheet.skin.session.track_width, Some(width));
        assert_eq!(sheet.skin.background.is_some(), id == "suhina-meadow");
        // A JSON theme from an earlier version holds the same values.
        let (restored, warnings) = Theme::from_json(&sheet.to_json());
        assert!(warnings.is_empty(), "{warnings:?}");
        let values = super::super::theme_vars::core_values;
        assert_eq!(values(&restored), values(&sheet));
        assert_eq!((&restored.skin.controls, &restored.skin.styles), (&sheet.skin.controls, &sheet.skin.styles));
        let mut native = sheet.clone();
        assert!(super::super::theme_css::apply(&mut native, &super::super::theme_css::export(&sheet)).is_empty());
        assert_eq!(native.skin.session, sheet.skin.session);
        assert_eq!(native.skin.controls, sheet.skin.controls);
        assert_eq!(native.skin.meter_intensity, sheet.skin.meter_intensity);
        assert_eq!(native.skin.styles, sheet.skin.styles);
    }
    let meadow = || built_in_themes().into_iter().find(|(id, _)| *id == "suhina-meadow").unwrap().1;
    let first = meadow().skin.background.unwrap();
    let second = meadow().skin.background.unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    let bytes = super::super::skin::decode_png(&first).unwrap();
    let image = image::load_from_memory(&bytes).unwrap();
    assert!(image.width() >= 1024 && image.width() <= 4096);
    assert!(image.height() >= 512 && image.height() <= 4096);
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
    let mut theme = Theme {
        name: "Night".into(),
        colors: Palette::DARK,
        liquid: None,
        gradients: Default::default(),
        transparent_rgb: Default::default(),
        skin: Default::default(),
        vars: Default::default(),
        ..Default::default()
    };
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
    assert_eq!(theme.colors.track_header_background, Palette::DARK.track_header_background);
    assert_eq!(theme.colors.track_header_selected, Palette::DARK.track_header_selected);
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
    for (id, files) in BUILT_IN_FOLDERS {
        let file = |name: &str, _| Ok(files.iter().find(|(path, _)| *path == name).map(|(_, bytes)| bytes.to_vec()));
        let (theme, warnings) = from_files(file).unwrap();
        assert!(warnings.is_empty(), "{id}: {warnings:?}");
        // A built-in theme is a theme folder like any other: the package checks pass.
        let folder: std::collections::BTreeMap<String, Vec<u8>> =
            files.iter().map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec())).collect();
        assert_eq!(suhina_theme_package::validate(&folder), Ok(Vec::new()), "{id}");
        assert!(!files.iter().any(|(path, _)| *path == "layout.css"), "{id}");
        let sheet = std::str::from_utf8(&folder["theme.css"]).unwrap();
        assert!(!sheet.contains("theme {"), "{id}: the name is theme.json's");
        assert_eq!(theme.name, ThemeMeta::parse(&folder[META]).unwrap().0.name, "{id}");
        assert!(theme.swatch.is_some() && theme.picture.is_some(), "{id}");
        // Every colour is written out, so none falls back to Suhina Dark by mistake.
        for (key, _, _) in Palette::DARK.tokens() {
            assert!(sheet.contains(&format!("  --{}: ", key.replace('.', "-"))), "{id} leaves out {key}");
        }
        // The sheet holds the theme's own values: nothing is a variant's.
        assert!(theme.vars.values.is_empty() && theme.skin.variant.is_none(), "{id}");
        assert!(is_built_in(id));
        assert!(id == BUILT_IN || theme.name != Theme::default().name, "{id}");
        if id == BUILT_IN {
            // Its sheet says only what the app has without one, so the style engine stays off.
            let values = super::super::theme_vars::core_values;
            assert_eq!(values(&theme), values(&Theme::default()));
            assert_eq!(built_in_themes()[0].1.skin.css, None);
        }
    }
    assert!(!is_built_in("night"));
}

#[test]
fn default_ui_font_is_manrope_and_previous_font_remains_available() {
    let definitions = fonts();
    assert_eq!(definitions.families[&FontFamily::Proportional].first().map(String::as_str), Some("manrope"));
    assert!(definitions.font_data.contains_key("encode-sans"));
    assert_eq!(
        definitions.font_data["manrope"].tweak.coords,
        egui::epaint::text::VariationCoords::new([(b"wght", 500.0)])
    );
    assert!(definitions.families.contains_key(&FontFamily::Name("encode-sans".into())));
}

#[test]
fn studio_themes_keep_text_readable_and_round_trip() {
    for (id, theme) in built_in_themes().into_iter().filter(|(id, _)| id.starts_with("suhina-studio-")) {
        let p = theme.colors;
        for background in [p.window_background, p.panel_background, p.control_background] {
            assert!(contrast(p.text_primary, background) >= 7.0, "{id}: primary text");
            assert!(contrast(p.text_secondary, background) >= 4.5, "{id}: secondary text");
            assert!(contrast(p.accent, background) >= 3.0, "{id}: accent");
        }
        // Its own sheet, as a copy of it is saved, reads back as the same theme.
        let (restored, warnings) = from_css(&super::super::theme_css::sheet(&theme, None));
        assert!(warnings.is_empty(), "{id}: {warnings:?}");
        let values = super::super::theme_vars::core_values;
        assert_eq!(values(&restored), values(&theme), "{id}: custom colors must survive saving");
    }
}

#[test]
fn text_never_takes_a_background_colour_in_files_from_before_text_inverse() {
    let (old, warnings) = Theme::from_json(r##"{"colors":{"window.background":"#10203080","text.value":"#ABCDEF"}}"##);
    assert!(warnings.is_empty());
    // Text on lit fills keeps the window's colour, opaque, and the chosen option its fill.
    assert_eq!(old.colors.text_inverse, rgb(0x102030));
    assert_eq!(old.colors.control_selected, rgb(0xABCDEF));
    let (new, _) = Theme::from_json(
        r##"{"colors":{"window.background":"#102030","text.inverse":"#FFFFFF","control.selectedBackground":"#00FF00"}}"##,
    );
    assert_eq!((new.colors.text_inverse, new.colors.control_selected), (rgb(0xFFFFFF), rgb(0x00FF00)));
}

#[test]
fn a_theme_is_see_through_when_a_background_has_alpha() {
    let ctx = egui::Context::default();
    set_palette(&ctx, Palette::DARK);
    assert!(!see_through());
    let mut glass = Palette::DARK;
    glass.panel_background = Color32::from_rgba_unmultiplied(10, 20, 30, 200);
    set_palette(&ctx, glass);
    assert!(see_through());
    // A see-through border is not a background.
    let mut lines = Palette::DARK;
    lines.border = Color32::from_rgba_unmultiplied(255, 255, 255, 40);
    set_palette(&ctx, lines);
    assert!(!see_through());
}

#[test]
fn the_readme_names_every_token() {
    let readme = template_readme();
    for rule in
        ["Backgrounds on the see-through window", "Closed-panel Grid tracks", "Value text on lit controls", "4:1"]
    {
        assert!(readme.contains(rule), "missing authoring rule: {rule}");
    }
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
fn independent_text_roles_round_trip_and_old_themes_keep_their_colours() {
    let (mut theme, warnings) = Theme::from_json(
        r##"{"colors":{"text.primary":"#EEDDAA","text.secondary":"#998877","window.background":"#112233"}}"##,
    );
    assert!(warnings.is_empty());
    assert_eq!(theme.colors.browser_text_primary, theme.colors.text_primary);
    assert_eq!(theme.colors.browser_text_secondary, theme.colors.text_secondary);
    assert_eq!(theme.colors.text_on_color_dark, theme.colors.window_background);
    theme.colors.browser_text_primary = rgb(0x55AAFF);
    theme.colors.text_on_color_dark = rgb(0x445566);
    let (restored, warnings) = Theme::from_json(&theme.to_json());
    assert!(warnings.is_empty());
    assert_eq!(restored.colors, theme.colors);
    let ctx = egui::Context::default();
    set_palette(&ctx, restored.colors);
    assert_eq!(text_on(rgb(0xFFFFFF)), rgb(0x445566));
    assert_eq!(browser_text_primary(), rgb(0x55AAFF));
    set_palette(&ctx, Palette::DARK);
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

#[test]
fn a_toolbar_in_theme_json_lists_a_module_once_and_bounds_its_offsets() {
    let value: Value = serde_json::from_str(
        r#"{ "items": ["views", "transport", "views", "nope"], "hidden": ["transport", "transport"],
             "offsets": { "transport": 2147483647, "views": -16384, "nope": 3 } }"#,
    )
    .unwrap();
    let (toolbar, offsets) = read_toolbar(&value).unwrap();
    assert_eq!(&toolbar.order[..2], ["views", "transport"]);
    assert_eq!(toolbar.order.iter().filter(|m| *m == "views").count(), 1);
    assert_eq!(toolbar.hidden, ["transport"]);
    assert_eq!(offsets.into_iter().collect::<Vec<_>>(), [("views".to_string(), -16384)]);
}
