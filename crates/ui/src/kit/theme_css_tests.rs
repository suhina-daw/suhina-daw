// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn name_liquid_and_surface_gradients_round_trip_through_canonical_css() {
    let mut theme = Theme { name: "Revontuli \"night\"".into(), ..Theme::default() };
    theme.liquid = Some(super::super::liquid::Options { panels: 40, tracks: 0, clips: 100 });
    theme.gradients.tracks = Some(super::super::gradient::Gradient {
        angle: 135,
        stops: vec![
            super::super::gradient::Stop {
                position: 0,
                color: egui::Color32::TRANSPARENT,
                transparent_rgb: Some([10, 20, 30]),
            },
            super::super::gradient::Stop {
                position: 100,
                color: egui::Color32::from_rgba_unmultiplied(200, 100, 50, 128),
                transparent_rgb: None,
            },
        ],
    });
    let css = format!("@import \"advanced.css\";\n{}", export(&theme));
    let mut restored = Theme::default();
    let warnings = apply(&mut restored, &css);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(restored.name, "Revontuli night");
    assert_eq!(restored.liquid, theme.liquid);
    assert_eq!(restored.gradients, theme.gradients);
    assert_eq!(restored.colors, theme.colors);
    let mut bad = Theme::default();
    let warnings = apply(&mut bad, "liquid { panels: 101; } gradient-clips { stops: red 0%; } theme { name: \"\"; }");
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert_eq!(bad.liquid, None);
    assert_eq!(bad.gradients.clips, None);
}

#[test]
fn control_anchors_round_trip_through_css_and_cycles_warn() {
    let mut theme = Theme::default();
    let warnings = apply(
        &mut theme,
        ".control-arrangement-meter { height: 100%; --suhina-anchor-bottom: arrangement-volume top 4px; }",
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    let css = export(&theme);
    assert!(css.contains("--suhina-anchor-bottom: arrangement-volume top 4px;"), "{css}");
    let mut again = Theme::default();
    assert!(apply(&mut again, &css).is_empty());
    assert_eq!(again.skin.controls, theme.skin.controls);
    let warnings = apply(&mut theme, ".control-arrangement-volume { --suhina-anchor-top: arrangement-meter bottom; }");
    assert!(warnings.iter().any(|w| w.contains("cycle")), "{warnings:?}");
    assert!(theme.skin.controls["arrangement-volume"].anchors.is_empty());
    assert!(theme.skin.controls["arrangement-meter"].anchors.is_empty());
}

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
        apply(&mut theme, ".control-mixer-volume { width: 999999px; top: NaNpx; --suhina-direction: sideways; }").len(),
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
fn only_relative_css_imports_are_read() {
    assert_eq!(
        imports("/* notes */ @import \"advanced.css\"; @import url('more/b.css'); :root {}"),
        ["advanced.css", "more/b.css"]
    );
    for bad in ["@import 'https://x.test/a.css'", "@import '../a.css'", "@import '/etc/a.css'", "@import 'a.svg'"] {
        assert_eq!(import(bad), None, "{bad}");
    }
    assert!(apply(&mut Theme::default(), "@import \"advanced.css\"; meter { segments: 8; }").is_empty());
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

#[test]
fn host_rules_inside_a_layer_count_and_media_rules_do_not() {
    let mut theme = Theme::default();
    let css = "@layer base, theme;\n@layer theme { .pan { --suhina-appearance: knob; } @layer inner { .knob { --suhina-value-position: none; } } }\n@media (max-width: 400px) { .slider { --suhina-appearance: knob; } }";
    let warnings = read_host_rules(&mut theme, css);
    assert!(warnings.is_empty(), "{warnings:?}");
    let get = |selector: &str, key: &str| theme.skin.styles.get(selector).and_then(|p| p.get(key)).cloned();
    assert_eq!(get(".pan", "--suhina-appearance").as_deref(), Some("knob"));
    assert_eq!(get(".knob", "--suhina-value-position").as_deref(), Some("none"));
    assert_eq!(get(".slider", "--suhina-appearance"), None, "a conditional rule is the style engine's");
}
