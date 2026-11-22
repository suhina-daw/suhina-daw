// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn transparent_rgb_survives_theme_round_trip() {
    let (theme, warnings) = Theme::from_json(r##"{"colors":{"text.primary":"#FF804000"}}"##);
    assert!(warnings.is_empty());
    assert_eq!(theme.rgba("text-primary", theme.color("text-primary")), [255, 128, 64, 0]);
    let (restored, warnings) = Theme::from_json(&theme.to_json());
    assert!(warnings.is_empty());
    assert_eq!(restored, theme);
}

#[test]
fn legacy_and_inherited_transparent_colors_keep_rgb_when_saved() {
    let (theme, warnings) =
        Theme::from_json(r##"{"colors":{"window.background":"#FF804000","text.primary":"#20A04000"}}"##);
    assert!(warnings.is_empty());
    let saved: Value = serde_json::from_str(&theme.to_json()).unwrap();
    assert_eq!(saved["colors"]["window.background"], "#FF804000");
    // Text inherits the background's color but never its alpha: it would vanish.
    assert_eq!(saved["colors"]["text.onColorDark"], "#FF8040");
    for key in ["text.primary", "browser.text.primary", "text.onColorLight"] {
        assert_eq!(saved["colors"][key], "#20A04000");
    }
    assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
    let (theme, warnings) = Theme::from_json(r##"{"colors":{"window.background":"#00000000"}}"##);
    assert!(warnings.is_empty());
    assert_eq!(theme.rgba("window-background", theme.color("window-background")), [0, 0, 0, 0]);
}

#[test]
fn translucent_theme_colors_preserve_alpha_in_files() {
    for color in [Color32::from_rgba_unmultiplied(100, 150, 200, 128), Color32::TRANSPARENT, rgb(0xAABBCC)] {
        assert_eq!(parse_hex(&hex(color)), Some(color));
        let mut theme = Theme::default();
        theme.colors.insert("grid-major".into(), color);
        assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
    }
    assert!(parse_hex("#1234567").is_none());
    assert!(parse_hex("#112233GG").is_none());
}

#[test]
fn former_track_colors_are_left_behind_quietly_and_materials_round_trip() {
    // Every file an earlier version saved has them: they go without a word, so the file still
    // converts to a theme.css.
    let (old, warnings) = Theme::from_json(
        r##"{"colors":{"track.1":"#123456","clip.midiDefault":"#ABCDEF","clip.audioDefault":"#040506"}}"##,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(old.liquid, None);
    let (theme, warnings) = Theme::from_json(r##"{"liquid":{"panels":38,"tracks":52,"clips":40}}"##);
    assert!(warnings.is_empty());
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
        assert!(sheet.skin.assets.len() >= super::super::skin::roles().count());
        for role in super::super::skin::roles() {
            let svg = &sheet.skin.assets[role];
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
        let mut native = sheet.clone();
        assert!(super::super::theme_css::apply(&mut native, &super::super::theme_css::export(&sheet)).is_empty());
        assert_eq!(native.skin.session, sheet.skin.session);
        assert_eq!(native.skin.meter_intensity, sheet.skin.meter_intensity);
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
fn a_theme_sheet_that_sets_the_former_track_colors_is_told_they_are_not_read() {
    let meta = ThemeMeta::new("Old", DEFAULT_SWATCH);
    let (_, warnings) =
        from_folder(meta, ":root { --track-3: #010203; --clip-audioDefault: #040506; --track-9: #000; }");
    let former: Vec<_> = warnings.iter().filter(|w| w.contains("no longer read")).collect();
    assert_eq!(former.len(), 1, "one notice for them all: {warnings:?}");
    assert!(former[0].contains("--track-3") && former[0].contains("--clip-audioDefault"));
    assert!(!former[0].contains("--track-9"), "only the former ones");
}

#[test]
fn a_theme_survives_a_round_trip() {
    let mut theme = Theme {
        name: "Night".into(),
        liquid: None,
        gradients: Default::default(),
        transparent_rgb: Default::default(),
        skin: Default::default(),
        vars: Default::default(),
        ..Default::default()
    };
    theme.colors.insert("accent".into(), rgb(0x12AB9F));
    theme.colors.insert("window-background".into(), rgb(0x000000));
    assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
}

#[test]
fn a_theme_file_needs_only_the_colors_it_changes() {
    let (theme, warnings) = Theme::from_json(r##"{ "name": "Blue", "colors": { "accent": "#3366ff" } }"##);
    assert!(warnings.is_empty());
    assert_eq!(theme.name, "Blue");
    assert!(theme.colors.contains_key("accent") && !theme.colors.contains_key("window-background"));
    assert_eq!(theme.color("accent"), rgb(0x3366FF));
    for variable in ["window-background", "track-headerBackground", "track-headerSelectedBackground"] {
        assert_eq!(theme.color(variable), studio(variable), "{variable}");
    }
}

#[test]
fn bad_values_keep_the_built_in_colors() {
    let text =
        r##"{ "name": 3, "colors": { "accent": "blue", "surface.9": "#000000", "border": "#12345" }, "font": 1 }"##;
    let (theme, warnings) = Theme::from_json(text);
    assert_eq!(theme, Theme::default());
    assert_eq!(warnings.len(), 5, "{warnings:?}");
    assert_eq!(Theme::from_json("[1, 2]").1.len(), 1);
    assert_eq!(Theme::from_json("{ not json").0, Theme::default());
}

#[test]
fn colors_are_written_and_read_as_hex() {
    assert_eq!(hex(rgb(0x1D1C1A)), "#1D1C1A");
    assert_eq!(parse_hex("#a6a3ff"), Some(rgb(0xA6A3FF)));
    assert_eq!(parse_hex("A6A3FF"), Some(rgb(0xA6A3FF)));
    assert_eq!(parse_hex("#A6A3F"), None);
    assert_eq!(parse_hex("#GGGGGG"), None);
    assert_eq!(parse_hex("#+1+2+3"), None);
}

#[test]
fn the_first_token_names_are_no_longer_read() {
    let (theme, warnings) =
        Theme::from_json(r##"{ "colors": { "surface.0": "#010203", "line": "#040506", "accent": "#070809" } }"##);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with("surface.0 ")) && warnings.iter().any(|w| w.starts_with("line ")));
    assert_eq!(theme.color("window-background"), studio("window-background"));
    assert_eq!(theme.color("border"), studio("border"));
    assert_eq!(theme.color("accent"), rgb(0x070809));
    // In a style sheet the old name is a custom property of the theme's own, like any other that
    // is not a token: it colors nothing.
    let (theme, _) = from_css(":root { --surface-0: #010203; --accent: #070809; }");
    assert_eq!(theme.color("window-background"), studio("window-background"));
    assert_eq!(theme.color("accent"), rgb(0x070809));
    // The editor's CSS box reads only the current names.
    let mut theme = Theme::default();
    let warnings = super::super::theme_css::apply(&mut theme, ":root { --surface-0: #010203; }");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(theme.colors.is_empty());
}

/// The variant's declared color scheme, not its colors, sets the native window's: dark text
/// declared dark stays dark, and a sheet that asks for the scheme with `light-dark()` sees it.
#[test]
fn the_declared_color_scheme_sets_the_native_windows_scheme() {
    let ctx = egui::Context::default();
    let suhina_light = built_in_themes().into_iter().find(|(id, _)| *id == "suhina-light").unwrap().1;
    assert!(suhina_light.light());
    super::super::skin::configure(&suhina_light.skin);
    use_colors(&ctx, &suhina_light.colors);
    in_style_frame(&ctx, |_| ());
    assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::Light);
    assert!(is_light());
    // The same colors declared dark: the scheme follows the declaration.
    let mut dark = suhina_light.skin.clone();
    dark.light = false;
    super::super::skin::configure(&dark);
    in_style_frame(&ctx, |_| ());
    assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::Dark);
    assert!(!is_light());
    // A sheet whose window text asks for the scheme does not flip it.
    let asking = super::super::skin::Skin {
        css: Some(":root { --text-primary: light-dark(white, black); } window { color: var(--text-primary); }".into()),
        light: true,
        ..Default::default()
    };
    super::super::skin::configure(&asking);
    use_colors(&ctx, &Colors::new());
    for _ in 0..3 {
        in_style_frame(&ctx, |_| ());
        assert!(is_light());
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    in_style_frame(&ctx, |_| ());
}

/// Every theme and variant that comes with Suhina declares the color scheme its text is made
/// for: dark text on a light look.
#[test]
fn a_built_in_themes_declared_color_scheme_matches_its_text() {
    let themes = built_in_themes().into_iter().map(|(id, theme)| (id.to_owned(), theme));
    let mut wrong = Vec::new();
    for (id, theme) in themes.chain(built_in_variants()) {
        let text = theme
            .skin
            .variables
            .get("--text-primary")
            .and_then(|value| super::super::native_css::color_of(value))
            .unwrap_or_else(|| theme.color("text-primary"));
        if (luminance(text) < 0.18) != theme.light() {
            wrong.push(format!("{id}: declares {}", if theme.light() { "light" } else { "dark" }));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
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
        // Every color is written out, so none falls back to Suhina Studio by mistake.
        for (key, _) in TOKENS {
            assert!(sheet.contains(&format!("  --{}: ", key.replace('.', "-"))), "{id} leaves out {key}");
        }
        // The sheet holds the theme's own values: nothing is a variant's.
        assert!(theme.vars.values.is_empty() && theme.skin.variant.is_none(), "{id}");
        assert!(is_built_in(id));
        assert!(id == BUILT_IN || theme.name != Theme::default().name, "{id}");
        if id == BUILT_IN {
            // Its sheet sets only colors, and it runs the style engine like every theme (#1448).
            assert!(
                sheet
                    .split(":root")
                    .nth(1)
                    .is_some_and(|rest| rest.trim_end().ends_with('}') && rest.matches('{').count() == 1)
            );
            assert_eq!(built_in_themes()[0].1.skin.css.as_deref(), Some(sheet));
            assert!(built_in_themes()[0].1.colors.is_empty(), "Suhina Studio sets nothing over itself");
        }
    }
    assert!(!is_built_in("night"));
}

#[test]
fn suhina_sheet_takes_its_colors_from_its_two_accent_shades() {
    use super::super::theme_vars::{Variant, with_variant};
    let (_, sheet) = built_in_themes().into_iter().find(|(id, _)| *id == "suhina-sheet").unwrap();
    let labels: Vec<_> = sheet.vars.declared.iter().filter_map(|p| p.label.clone()).collect();
    assert_eq!(labels, ["Accent", "Accent, light"]);
    let values = [("--tint", "#A02050"), ("--tint-light", "#E8A0B8")];
    let variant = Variant {
        light: None,
        id: "rose".into(),
        name: "Rose".into(),
        values: values.map(|(name, value)| (name.to_string(), value.to_string())).into(),
    };
    let (rose, warnings) = with_variant(&sheet, Some(&variant));
    assert!(warnings.is_empty(), "{warnings:?}");
    // Colors that say what something is or holds stay, and the paper stays white.
    let fixed = [
        "track.",
        "clip.",
        "recording",
        "meter.",
        "window.background",
        "control.background",
        "text.onColorLight",
        "text.inverse",
        "pan.background",
    ];
    for (key, _) in TOKENS {
        let (before, after) = (sheet.color(&variable_name(key)), rose.color(&variable_name(key)));
        if !fixed.iter().any(|prefix| key.starts_with(prefix)) {
            assert_ne!(before, after, "{key} does not follow the accent");
        }
    }
    assert_eq!(rose.swatch, Some(rgb(0xA02050)));
    // Nothing in its rules or its artwork is a fixed color of its own, the green panel included.
    let css = include_str!("../../themes/suhina-sheet/theme.css");
    let rules = &css[css.find("@layer").unwrap()..];
    let browser =
        rules.lines().skip_while(|line| !line.trim_start().starts_with("browser {")).take(3).collect::<String>();
    assert!(browser.contains("var(--tint-light)") && !browser.contains('#'), "{browser}");
    for (path, bytes) in built_in_files("suhina-sheet").unwrap() {
        if path.starts_with("assets/") {
            let svg = String::from_utf8_lossy(bytes).to_lowercase().replace("#ffffff", "");
            assert!(!svg.contains('#'), "{path}");
        }
    }
}

#[test]
fn suhina_glass_clears_its_window_as_far_as_its_glassiness_says() {
    use super::super::theme_vars::{Variant, with_variant};
    let (_, glass) = built_in_themes().into_iter().find(|(id, _)| *id == "suhina-glass").unwrap();
    let window_alpha = |glassiness: &str| {
        let values = [("--glassiness".to_string(), glassiness.to_string())].into();
        let variant = Variant { light: None, id: "test".into(), name: "Test".into(), values };
        let (theme, warnings) = with_variant(&glass, Some(&variant));
        assert!(warnings.is_empty(), "{warnings:?}");
        theme.color("window-background").a()
    };
    assert_eq!(glass.color("window-background").a(), window_alpha("18"));
    assert_eq!(window_alpha("0"), 255);
    assert_eq!(window_alpha("18"), 209);
    assert_eq!(window_alpha("60"), 102);
    // The swatch is the color, whatever shows through it.
    assert_eq!(glass.swatch, Some(rgb(0x0B2638)));
    assert_eq!(glass.color("border").a(), 255);
}

#[test]
fn the_built_in_variants_read_whole() {
    let variants = built_in_variants();
    for id in [
        "suhina-studio@darker",
        "suhina-studio@darkest",
        "suhina-studio@light",
        "nord@snow-storm",
        "suhina-glass@frost",
    ] {
        let (_, theme) = variants.iter().find(|(key, _)| key == id).unwrap_or_else(|| panic!("{id}"));
        assert!(theme.swatch.is_some() && theme.picture.is_some(), "{id}");
    }
    let base = |id: &str| built_in_themes().into_iter().find(|(b, _)| *b == id).unwrap().1;
    for (id, theme) in &variants {
        let (name, variant) = id.split_once('@').unwrap();
        assert_ne!(theme.colors, base(name).colors, "{id}");
        assert_eq!(theme.vars.variant.as_deref(), Some(variant));
    }
    // A variant of Suhina Studio has the sheet back, so what follows its settings does.
    let (_, darkest) = variants.iter().find(|(key, _)| key == "suhina-studio@darkest").unwrap();
    assert_eq!(darkest.color("window-background"), rgb(0x000000));
    assert!(darkest.skin.css.is_some());
    assert_eq!(darkest.color("text-primary"), rgb(0xD4D4D4));
    assert_ne!(darkest.color("panel-hoverBackground"), base("suhina-studio").color("panel-hoverBackground"));
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
        let c = |variable: &str| theme.color(variable);
        for background in [c("window-background"), c("panel-background"), c("control-background")] {
            assert!(contrast(c("text-primary"), background) >= 7.0, "{id}: primary text");
            assert!(contrast(c("text-secondary"), background) >= 4.5, "{id}: secondary text");
            assert!(contrast(c("accent"), background) >= 3.0, "{id}: accent");
        }
        // Its own sheet, as a copy of it is saved, reads back as the same theme.
        let (restored, warnings) = from_css(&super::super::theme_css::sheet(&theme, None));
        assert!(warnings.is_empty(), "{id}: {warnings:?}");
        let values = super::super::theme_vars::core_values;
        assert_eq!(values(&restored), values(&theme), "{id}: custom colors must survive saving");
    }
}

#[test]
fn text_never_takes_a_background_color_in_files_from_before_text_inverse() {
    let (old, warnings) = Theme::from_json(r##"{"colors":{"window.background":"#10203080","text.value":"#ABCDEF"}}"##);
    assert!(warnings.is_empty());
    // Text on lit fills keeps the window's color, opaque, and the chosen option its fill.
    assert_eq!(old.color("text-inverse"), rgb(0x102030));
    assert_eq!(old.color("control-selectedBackground"), rgb(0xABCDEF));
    let (new, _) = Theme::from_json(
        r##"{"colors":{"window.background":"#102030","text.inverse":"#FFFFFF","control.selectedBackground":"#00FF00"}}"##,
    );
    assert_eq!((new.color("text-inverse"), new.color("control-selectedBackground")), (rgb(0xFFFFFF), rgb(0x00FF00)));
}

#[test]
fn a_theme_is_see_through_when_a_background_has_alpha() {
    let ctx = egui::Context::default();
    let see_through_with = |variable: &str, color: Color32| {
        use_colors(&ctx, &[(variable.to_owned(), color)].into());
        in_style_frame(&ctx, |_| ());
        see_through()
    };
    assert!(!see_through_with("accent", studio("accent")));
    assert!(see_through_with("panel-background", Color32::from_rgba_unmultiplied(10, 20, 30, 200)));
    assert!(see_through_with("window-background", Color32::from_rgba_unmultiplied(10, 20, 30, 200)));
    // A see-through border is not a background.
    assert!(!see_through_with("border", Color32::from_rgba_unmultiplied(255, 255, 255, 40)));
    use_colors(&ctx, &Colors::new());
}

/// The theme author guide, `docs/guide/theme-authoring.md`.
fn guide() -> String {
    // A Windows checkout has CRLF line ends.
    include_str!("../../../../docs/guide/theme-authoring.md").replace("\r\n", "\n")
}

#[test]
fn the_author_guide_names_every_token() {
    let readme = guide();
    for rule in
        ["Backgrounds on the see-through window", "Closed-panel Grid tracks", "Value text on lit controls", "4:1"]
    {
        assert!(readme.contains(rule), "missing authoring rule: {rule}");
    }
    for (key, purpose) in TOKENS {
        let color = studio(&variable_name(key));
        assert!(readme.contains(&format!("| `{key}` | `{}` | {purpose} |", hex(color))), "{key}");
    }
}

#[test]
fn every_token_has_its_own_name() {
    let mut keys: Vec<_> = TOKENS.iter().map(|(key, _)| *key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), TOKENS.len());
    // Each is a color the core sheet registers, with Suhina Studio's value.
    for (key, _) in TOKENS {
        assert!(default_color(&variable_name(key)).is_some(), "{key}");
    }
}

#[test]
fn text_on_a_color_is_the_text_token_that_stands_out_more() {
    // The default track colors are light: names on them are dark.
    let ctx = egui::Context::default();
    use_colors(&ctx, &Colors::new());
    for fill in [rgb(0xFF9A3C), rgb(0x2FD9C4)] {
        let on = in_style_frame(&ctx, |_| text_on(fill));
        assert_eq!(on, studio("window-background"), "{fill:?}");
        assert!(contrast(fill, on) >= 4.5, "{fill:?}");
    }
    assert_eq!(in_style_frame(&ctx, |_| text_on(rgb(0x202020))), studio("text-primary"));
    assert!((contrast(rgb(0x000000), rgb(0xFFFFFF)) - 21.0).abs() < 0.01);
    let faint_white = Color32::from_rgba_unmultiplied(255, 255, 255, 20);
    assert!(text_contrast(faint_white, rgb(0x000000)) < 1.5, "faint text counts as faint");
    assert!((text_contrast(rgb(0xFFFFFF), rgb(0x000000)) - 21.0).abs() < 0.01);
}

#[test]
fn palette_and_steps_keep_readable_inks_on_composited_track_fills() {
    let ctx = egui::Context::default();
    let variants = built_in_themes().into_iter().map(|(id, theme)| (id.to_owned(), theme)).chain(built_in_variants());
    for (id, theme) in variants {
        use_colors(&ctx, &theme.colors);
        in_style_frame(&ctx, |_| {
            use crate::interaction::color_picker::panel::steps;
            let stepped = steps::hues().iter().flat_map(|&hue| steps::grid(hue));
            for shade in crate::interaction::color_picker::swatches().into_iter().chain(stepped) {
                let solid = crate::interaction::color_picker::color32(shade);
                for fill in [
                    solid,
                    crate::kit::motion::mix(theme.color("control-background"), solid, 0.55),
                    theme.color("selection-background"),
                    theme.color("track-headerMutedBackground"),
                    theme.color("track-headerMutedSelectedBackground"),
                ] {
                    assert!(text_contrast(text_on(fill), fill) >= 4.5, "{id}: {fill:?}");
                }
            }
        });
    }
    // Even a custom theme with translucent mid-gray inks has a readable fallback.
    use_colors(
        &ctx,
        &[
            ("text-onColorLight".into(), Color32::from_gray(125)),
            ("text-onColorDark".into(), Color32::from_rgba_unmultiplied(120, 120, 120, 50)),
        ]
        .into(),
    );
    in_style_frame(&ctx, |_| {
        for level in 0..=255 {
            let fill = Color32::from_gray(level);
            assert!(text_contrast(text_on(fill), fill) >= 4.5);
        }
    });
}

#[test]
fn the_automation_pin_is_a_themes_own_color_and_older_files_get_their_accent() {
    let (theme, warnings) = Theme::from_json(r##"{"colors":{"accent":"#123456"}}"##);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(theme.color("automation-pin"), rgb(0x123456));
    let (theme, _) = Theme::from_json(r##"{"colors":{"accent":"#123456","automation.pin":"#ABCDEF"}}"##);
    assert_eq!(theme.color("automation-pin"), rgb(0xABCDEF));

    let night = || ThemeMeta::new("Night", DEFAULT_SWATCH);
    let (theme, _) = from_folder(night(), ":root { --accent: #123456; --automation-pin: var(--accent); }\n");
    assert_eq!(theme.color("automation-pin"), rgb(0x123456), "a style sheet's pin from its accent");
    let (theme, _) = from_folder(night(), ":root { --accent: #123456; --automation-pin: #ABCDEF; }\n");
    assert_eq!(theme.color("automation-pin"), rgb(0xABCDEF), "a style sheet's own pin");
}

#[test]
fn independent_text_roles_round_trip_and_old_themes_keep_their_colors() {
    let (mut theme, warnings) = Theme::from_json(
        r##"{"colors":{"text.primary":"#EEDDAA","text.secondary":"#998877","window.background":"#112233"}}"##,
    );
    assert!(warnings.is_empty());
    assert_eq!(theme.color("browser-text-primary"), theme.color("text-primary"));
    assert_eq!(theme.color("browser-text-secondary"), theme.color("text-secondary"));
    assert_eq!(theme.color("text-onColorDark"), theme.color("window-background"));
    theme.colors.insert("browser-text-primary".into(), rgb(0x55AAFF));
    theme.colors.insert("text-onColorDark".into(), rgb(0x445566));
    let (restored, warnings) = Theme::from_json(&theme.to_json());
    assert!(warnings.is_empty());
    assert_eq!(restored.colors, theme.colors);
    let ctx = egui::Context::default();
    use_colors(&ctx, &restored.colors);
    assert_eq!(in_style_frame(&ctx, |_| text_on(rgb(0xFFFFFF))), rgb(0x445566));
    assert_eq!(restored.color("browser-text-primary"), rgb(0x55AAFF));
    use_colors(&ctx, &Colors::new());
}

#[test]
fn the_colors_in_use_change_with_the_theme() {
    let ctx = egui::Context::default();
    use_colors(&ctx, &[("accent".to_owned(), rgb(0x00FF00))].into());
    assert_eq!(root_in(&ctx, "--accent"), rgb(0x00FF00));
    use_colors(&ctx, &Colors::new());
    assert_eq!(root_in(&ctx, "--accent"), studio("accent"));
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

#[test]
fn the_core_sheet_registers_every_color_token_and_base_color() {
    // The core sheet is suhina-theme-package's (`CORE_SHEET`), the one place Suhina Studio's
    // colors are written: the package checks, the CLI and the app read the same values.
    assert_eq!(super::super::theme_vars::core_sheet(), suhina_theme_package::CORE_SHEET);
    for (key, _) in TOKENS {
        let property = super::super::theme_vars::token_property(key);
        assert!(suhina_theme_package::CORE_SHEET.contains(&format!("@property {property} ")), "{key}");
    }
    for (name, value) in super::super::theme_vars::BASE_COLORS {
        assert_eq!(default_color(&name[2..]).map(hex), parse_hex(value).map(hex), "{name}");
    }
}

#[test]
fn toolbar_rules_in_a_current_sheet_are_not_read_and_are_named_once() {
    let sheet = ":root { --accent: #123456; }\ntoolbar { hidden: tempo; }\ntoolbar { offset-transport: 80px; }\n";
    let (theme, warnings) = from_folder(ThemeMeta::new("Night", DEFAULT_SWATCH), sheet);
    assert!(theme.skin.toolbar.is_none() && theme.skin.toolbar_offsets.is_empty());
    assert_eq!(warnings.iter().filter(|w| w.contains("toolbar rules are not read")).count(), 1, "{warnings:?}");
    // The arrangement is theme.json's.
    let mut meta = ThemeMeta::new("Night", DEFAULT_SWATCH);
    meta.toolbar = Some(serde_json::json!({ "items": ["transport"], "hidden": ["tempo"] }));
    let (theme, _) = from_folder(meta, sheet);
    assert_eq!(theme.skin.toolbar.unwrap().hidden, ["tempo"]);
}

/// A sample-map color given as a `kind.*` color token, which is gone, is not read silently: the
/// notice says the colors are `sample-point` rules now, in a sheet and in a theme file alike.
#[test]
fn a_former_kind_token_gets_a_notice_naming_its_sample_point_rule() {
    let (_, warnings) = from_css(":root { --kind-kick: #FF0000; --kind-closedHat: #00FF00; --kind-ness: #0000FF; }");
    let kinds: Vec<_> = warnings.iter().filter(|w| w.contains("is no longer read")).collect();
    assert_eq!(kinds.len(), 2, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("sample-point[kind=\"kick\"] { color: … }")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with("--kind-closedHat is no longer read")), "{warnings:?}");
    let (_, warnings) = Theme::from_json(r##"{"colors":{"kind.snare":"#123456","kind.nothing":"#123456"}}"##);
    assert!(warnings.iter().any(|w| w.contains("sample-point[kind=\"snare\"]")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w == "kind.nothing is not a color token"), "{warnings:?}");
}

/// The guide's examples for track and clip colors are rules a theme can use as written.
#[test]
fn the_author_guide_s_track_and_clip_color_examples_read_without_warnings() {
    let readme = guide();
    let from = readme.find("### Tracks").unwrap();
    let to = readme[from..].find("piano-grid {").unwrap() + from;
    let css: String =
        readme[from..to].split("```css\n").skip(1).filter_map(|block| block.split("```").next()).collect();
    assert!(css.contains("attr(color type(<color>))"), "{css}");
    let (_, warnings) = from_folder(ThemeMeta::new("Readme", DEFAULT_SWATCH), &css);
    assert!(warnings.is_empty(), "{warnings:?}\n{css}");
    assert!(suhina_style::sheet::Sheet::parse(&css).is_ok());
}

/// A sheet from an earlier version that reads the variables the app set on tracks, or gives
/// artwork chosen by a track's color, is told that neither is used any more.
#[test]
fn a_sheet_reading_former_track_variables_or_color_artwork_is_told() {
    let sheet = "track-name { background-color: var(--track-color); } \
                 track.body.color-ff9a3c { image: url(\"assets/orange.svg\"); }";
    let (_, warnings) = from_folder(ThemeMeta::new("Old", DEFAULT_SWATCH), sheet);
    assert!(warnings.iter().any(|w| w.starts_with("--track-color no longer set")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with("track.body.color-ff9a3c not used")), "{warnings:?}");
    let (_, warnings) =
        from_folder(ThemeMeta::new("New", DEFAULT_SWATCH), "track-name { color: attr(color type(<color>)); }");
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_theme_file_holds_only_the_colors_the_theme_sets() {
    let (theme, warnings) = Theme::from_json(r##"{"colors":{"grid.major":"#102030","recording":"#405060"}}"##);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(theme.colors.keys().map(String::as_str).collect::<Vec<_>>(), ["grid-major", "recording"]);
    let saved: Value = serde_json::from_str(&theme.to_json()).unwrap();
    let written: Vec<&str> = saved["colors"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(written, ["grid.major", "recording"]);
    assert_eq!(Theme::from_json(&theme.to_json()), (theme, Vec::new()));
    let saved: Value = serde_json::from_str(&Theme::default().to_json()).unwrap();
    assert!(saved.get("colors").is_none(), "a theme that sets nothing has no color block");
}

#[test]
fn a_themes_colors_are_suhina_studios_with_its_own_over_them() {
    for (key, _) in TOKENS {
        assert_eq!(Theme::default().color(&variable_name(key)), studio(&variable_name(key)), "{key}");
    }
    let mut theme = Theme::default();
    theme.colors.insert("track-headerBackground".into(), rgb(0x010203));
    assert_eq!(theme.color("track-headerBackground"), rgb(0x010203));
    for (key, _) in TOKENS.iter().filter(|(key, _)| *key != "track.headerBackground") {
        assert_eq!(theme.color(&variable_name(key)), studio(&variable_name(key)), "{key}");
    }
    assert_eq!(theme.color("not-a-token"), Color32::TRANSPARENT);
}
