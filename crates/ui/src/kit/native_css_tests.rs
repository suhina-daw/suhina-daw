// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
#[test]
fn native_gradients_publish_every_layer_and_their_end_colours() {
    for (image, layers, first, last) in [
        ("linear-gradient(red,blue)", 1, Color32::RED, Color32::BLUE),
        ("linear-gradient(to right, red, transparent)", 1, Color32::RED, Color32::TRANSPARENT),
        ("linear-gradient(to top right, white 0 45%, green 45% 100%)", 1, Color32::WHITE, Color32::from_rgb(0, 128, 0)),
        (
            "repeating-linear-gradient(90deg, transparent 0 21px, #e5ece7 21px 22px)",
            1,
            Color32::TRANSPARENT,
            Color32::from_rgb(0xe5, 0xec, 0xe7),
        ),
        ("linear-gradient(red, green, blue), linear-gradient(90deg, #fff3, #0008)", 2, Color32::RED, Color32::BLUE),
        (
            "radial-gradient(ellipse at 65% 45%, #29424280, transparent 70%)",
            1,
            Color32::from_rgba_unmultiplied(0x29, 0x42, 0x42, 0x80),
            Color32::TRANSPARENT,
        ),
        ("radial-gradient(circle 10px at left top, red, blue)", 1, Color32::RED, Color32::BLUE),
        (
            "conic-gradient(from 90deg at 25% 75%, red, blue 0.25turn, green 50%)",
            1,
            Color32::RED,
            Color32::from_rgb(0, 128, 0),
        ),
        ("repeating-conic-gradient(red 0 10%, blue 10% 20%)", 1, Color32::RED, Color32::BLUE),
    ] {
        let css = format!(".button {{color:#34789a; background-image:{image};}}");
        prepare(&css, 800.0);
        let style = scoped(&["button".into()]).unwrap();
        assert_eq!(style.gradients.len(), layers, "{image}");
        let gradient = &style.gradients[0];
        let samples = gradient.pixel_samples(100.0);
        let close = |a: Color32, b: Color32| a.to_array().iter().zip(b.to_array()).all(|(x, y)| x.abs_diff(y) <= 2);
        assert!(close(samples[0], first), "{image}: {:?}", samples[0]);
        if !image.starts_with("repeating") {
            assert!(close(samples[1023], last), "{image}: {:?}", samples[1023]);
        } else {
            assert!(samples.iter().any(|s| close(*s, last)), "{image}");
        }
        let geometry = gradient.geometry_samples(Vec2::new(100.0, 50.0));
        assert!(geometry.iter().all(|v| v.is_finite()), "{image}");
    }
    skin::configure(&skin::Skin::default());
}
#[test]
fn native_paint_values_resolve_variables_currentcolor_em_and_media() {
    let rgba = Color32::from_rgba_unmultiplied;
    prepare(
        ".button { color:#123456; background-color:#abcdef80; border-color:currentColor; border-radius:4px; opacity:.7; }",
        800.0,
    );
    let style = scoped(&["toolbar".into(), "button".into()]).unwrap();
    assert_eq!(style.color, Color32::from_rgb(0x12, 0x34, 0x56));
    assert_eq!(style.background, rgba(0xab, 0xcd, 0xef, 0x80));
    assert_eq!(style.border, Color32::from_rgb(0x12, 0x34, 0x56));
    assert_eq!(style.radius, 4.0);
    assert!((style.opacity - 0.7).abs() < 1e-6);
    assert_eq!(
        style.authored,
        Authored {
            color: true,
            background: true,
            border: true,
            font_size: false,
            radius: true,
            accent: false,
            border_width: false
        }
    );
    prepare(
        ":root { --ink:#2468ac; } .toolbar { color:var(--ink); font-size:12px; } .button { font-size:1.5em; border-radius:.25em; box-shadow:inset 0 1px 2px -1px #fff4, 1px 2px 3px currentColor; text-shadow:0 1px 1px #000a; }",
        800.0,
    );
    let style = scoped(&["toolbar".into(), "button".into()]).unwrap();
    assert_eq!(style.color, Color32::from_rgb(0x24, 0x68, 0xac));
    assert_eq!(style.font_size, 18.0);
    assert_eq!(style.radius, 4.5);
    assert_eq!(style.shadows.len(), 2);
    assert_eq!(style.text_shadows.len(), 1);
    assert!(style.authored.color && style.authored.font_size);
    for (width, expected) in
        [(800.0, Color32::from_rgb(0x0c, 0x22, 0x38)), (600.0, Color32::from_rgb(0x65, 0x43, 0x21))]
    {
        prepare(
            ".toolbar { font-size:12px; color:rgba(12, 34, 56, .5); } @media (max-width:700px) { .button { color:#654321; } }",
            width,
        );
        let style = scoped(&["toolbar".into(), "button".into()]).unwrap();
        assert_eq!(style.color.to_srgba_unmultiplied()[..3], expected.to_srgba_unmultiplied()[..3], "{width}");
    }
    skin::configure(&skin::Skin::default());
}

#[test]
fn unsupported_declarations_are_skipped_and_the_rest_of_the_sheet_applies() {
    for css in [
        ".button { color:red; background-image:radial-gradient(in oklch, blue, green); }",
        ".button { color:red; filter:blur(2px); }",
    ] {
        prepare(css, 800.0);
        let actual = scoped(&["button".into()]).unwrap();
        assert!(actual.authored.color, "{css}");
        assert_eq!(actual.color, Color32::RED, "{css}");
        assert!(!actual.has_gradient(), "{css}");
        assert!(!inspect(css).unwrap().is_empty(), "{css}");
    }
    // A variable that does not make a valid colour makes the property unset, as in CSS:
    // the colour is inherited, not the earlier declaration's.
    prepare(":root { --ink:color-mix(in lab, red, blue); } .button { color:red; color:var(--ink); }", 800.0);
    assert_ne!(scoped(&["button".into()]).unwrap().color, Color32::RED);
    // A sheet that cannot be parsed at all keeps the last working styles.
    prepare(".button { color:blue; }", 800.0);
    let _ = scoped(&["button".into()]);
    let deep = format!(".button {{ color:red; }} {}", "@media (min-width:1px) {".repeat(20));
    prepare(&deep, 800.0);
    ENGINE.with(|engine| assert!(engine.borrow().native.is_some(), "the previous sheet stays"));
    skin::configure(&skin::Skin::default());
}

#[test]
fn native_states_live_reload_and_compatibility_switch_keep_stable_keys() {
    let nodes = vec![
        StyleNode {
            key: 1,
            parent: None,
            class: "toolbar".into(),
            name: None,
            state: 0,
            variables: Vec::new(),
            sheet: 0,
        },
        StyleNode {
            key: 2,
            parent: Some(0),
            class: "button".into(),
            name: Some("play".into()),
            state: 0,
            variables: Vec::new(),
            sheet: 0,
        },
    ];
    prepare(".toolbar { color:#112233; } .toolbar:hover { color:#445566; } #control-play { box-shadow:0 1px; }", 800.0);
    ENGINE.with(|engine| engine.borrow_mut().nodes = nodes.clone());
    assert_eq!(finish_tree(0.0), (true, false));
    ENGINE.with(|engine| assert_eq!(engine.borrow().values[&2].color, Color32::from_rgb(0x11, 0x22, 0x33)));
    assert_eq!(finish_tree(0.1), (false, false), "unchanged scopes must use the retained result");
    ENGINE.with(|engine| engine.borrow_mut().nodes[1].state = 1);
    assert_eq!(finish_tree(0.2), (true, false));
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        assert_eq!(engine.values[&2].color, Color32::from_rgb(0x44, 0x55, 0x66));
    });
    for source in [
        ".button { color:#abcdef; }",
        ".button { color:#abcdef; background-image:radial-gradient(in oklch, red, blue); }",
        ".button { color:#abcdef; }",
    ] {
        prepare(source, 800.0);
        ENGINE.with(|engine| engine.borrow_mut().nodes = nodes.clone());
        assert_eq!(finish_tree(0.3), (true, false));
        ENGINE.with(|engine| {
            let engine = engine.borrow();
            assert_eq!(engine.values[&2].color, Color32::from_rgb(0xab, 0xcd, 0xef), "{source}");
            assert!(engine.native.is_some());
            assert_eq!(engine.values.len(), 2);
        });
    }
    skin::configure(&skin::Skin::default());
}
/// Styles of the `device-control` scopes named `cutoff` in two device panels, with the
/// first panel given `device` as its own sheet.
fn device_controls(theme: &str, device: &str, user: Option<&str>) -> (Computed, Computed, Computed) {
    prepare(theme, 800.0);
    set_user_sheet(user.map(Arc::from));
    let ctx = egui::Context::default();
    let mut styles = None;
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = frame(ui);
            let _window = super::super::style_class::enter("window");
            let mut found = Vec::new();
            for panel in ["track-1-device-0", "track-1-device-1"] {
                let _panel = super::super::style_class::enter_named("device-panel", panel);
                if panel.ends_with('0') {
                    device_sheet(ui.ctx(), Some(device));
                }
                let _control = super::super::style_class::enter_named("device-control", "cutoff");
                found.push(super::super::style_class::computed().unwrap());
            }
            let _browser = super::super::style_class::enter("browser");
            found.push(super::super::style_class::computed().unwrap());
            if let [a, b, browser] = found.as_slice() {
                styles = Some((a.clone(), b.clone(), browser.clone()));
            }
        })
        .textures_delta
        .clear();
    }
    set_user_sheet(None);
    skin::configure(&skin::Skin::default());
    styles.unwrap()
}

#[test]
fn a_device_sheet_styles_its_own_panel_by_control_id_and_never_leaks() {
    let (styled, other, browser) = device_controls(
        ".device-control { color: #102030; }",
        "#cutoff { color: #ff0000; accent-color: #0000ff; } .browser { color: #00ff00; }",
        None,
    );
    assert_eq!(styled.color, Color32::from_rgb(255, 0, 0), "the device sheet wins over the theme");
    assert!(styled.authored.accent);
    assert_eq!(styled.accent, Color32::from_rgb(0, 0, 255));
    assert_eq!(other.color, Color32::from_rgb(0x10, 0x20, 0x30), "another device keeps the theme");
    assert!(!other.authored.accent);
    assert_ne!(browser.color, Color32::from_rgb(0, 255, 0), "a device sheet stays inside its panel");
}

#[test]
fn the_user_sheet_wins_over_device_and_theme_and_runs_without_a_theme_sheet() {
    let (styled, other, _) = device_controls(
        ".device-control { color: #102030; }",
        "#cutoff { color: #ff0000 !important; }",
        Some("#cutoff { color: #abcdef; }"),
    );
    assert_eq!(styled.color, Color32::from_rgb(0xab, 0xcd, 0xef), "user beats even device !important");
    assert_eq!(other.color, Color32::from_rgb(0xab, 0xcd, 0xef));
    // No theme sheet at all: device and user sheets still apply.
    let (styled, other, _) = device_controls("", "#cutoff { color: #ff0000; }", None);
    assert_eq!(styled.color, Color32::from_rgb(255, 0, 0));
    assert!(!other.authored.color, "unstyled parts keep the application theme");
}

#[test]
fn workspace_example_collapses_closed_panels() {
    prepare(theme::WORKSPACE_GRID_EXAMPLE, 800.0);
    let open = workspace(Vec2::new(800.0, 600.0), Some(240.0), Some((false, 180.0))).unwrap();
    assert_eq!(open.browser.unwrap().width(), 240.0);
    assert_eq!(open.detail.unwrap().height(), 180.0);
    let closed = workspace(Vec2::new(800.0, 600.0), None, None).unwrap();
    assert!(closed.browser.is_none() && closed.detail.is_none());
    assert!(
        closed.main.width() > 780.0 && closed.main.height() > 580.0,
        "closed panels reserved their fixed dimensions: {:?}",
        closed.main
    );
    skin::configure(&skin::Skin::default());
}
#[test]
fn gloss_surfaces_have_opaque_bases_and_sheet_overlays_preserve_native_fills() {
    prepare(theme::GLOSS_EXAMPLE, 1138.0);
    for class in ["session", "arrangement", "browser", "detail", "device-panel", "mixer", "button"] {
        let style = scoped(&[class.into()]).unwrap();
        assert!(style.authored.background, "{class} has no explicit base");
        assert_eq!(style.background.a(), 255, "{class} exposes the desktop");
        assert!(style.has_gradient(), "{class} lost its gradient");
    }
    for css in [theme::GLOSS_EXAMPLE, include_str!("../../themes/suhina-sheet.theme.css")] {
        prepare(css, 1138.0);
        for class in ["track-header", "track-name"] {
            let style = scoped(&[class.into()]).unwrap();
            assert!(!style.authored.background, "{class} overlay replaces the native fill");
            assert!(style.has_gradient());
        }
    }
    prepare(include_str!("../../themes/suhina-meadow.theme.css"), 1138.0);
    for class in ["window", "application-bar", "mixer", "track-header", "track-name"] {
        assert_eq!(scoped(&[class.into()]).unwrap().background.a(), 0, "Meadow's deliberate transparency changed");
    }
    skin::configure(&skin::Skin::default());
}
#[test]
fn responsive_workspace_and_toolbar_survive_resize() {
    let css = include_str!("../../tests/fixtures/responsive-theme.css");
    prepare(css, 1138.0);
    let ctx = egui::Context::default();
    for size in
        [Vec2::new(1138.0, 1032.0), Vec2::new(1138.0, 1032.0), Vec2::new(800.0, 700.0), Vec2::new(1138.0, 1032.0)]
    {
        ctx.run_ui(
            egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() },
            |ui| {
                let _frame = frame(ui);
                let _window = super::super::style_class::enter("window");
                {
                    let _toolbar = super::super::style_class::enter("toolbar");
                    for name in ["transport", "position", "performance", "mapping"] {
                        let _module = super::super::style_class::enter_named("toolbar-module", name);
                        let _button = super::super::style_class::enter("button");
                    }
                }
                let modules = toolbar(
                    &[
                        ("transport".into(), 100.0),
                        ("position".into(), 100.0),
                        ("performance".into(), 80.0),
                        ("mapping".into(), 60.0),
                    ],
                    Vec2::new(size.x, 56.0),
                )
                .unwrap();
                assert_eq!(modules.iter().any(|module| module.name == "performance"), size.x > 1000.0);
                assert_eq!(modules.iter().any(|module| module.name == "mapping"), size.x > 820.0);
                let layout = workspace(Vec2::new(size.x, size.y - 100.0), None, Some((false, 200.0))).unwrap();
                assert!(layout.browser.is_none());
                assert_eq!(layout.detail.unwrap().height(), if size.y <= 760.0 { 150.0 } else { 200.0 });
                let _workspace = super::super::style_class::enter_named("workspace", "workspace");
                {
                    let _main = super::super::style_class::enter_named("workspace-panel", "panel-main");
                    let _view = super::super::style_class::enter("arrangement");
                }
                {
                    let _detail = super::super::style_class::enter_named("workspace-panel", "panel-detail");
                    let _editor = super::super::style_class::enter("clip-editor");
                }
            },
        )
        .textures_delta
        .clear();
    }
    skin::configure(&skin::Skin::default());
}
#[test]
fn palette_and_resize_restyle_retained_tree_and_preserve_author_colours() {
    let saved = theme::palette();
    prepare(
        ".button { background-color:currentColor; } .browser { color:#123456; } @media (max-width:600px) { .browser { color:#654321; } }",
        800.0,
    );
    let ctx = egui::Context::default();
    for (pass, width) in [(0, 800.0), (1, 800.0), (2, 500.0)] {
        let mut palette = saved;
        palette.text_primary = Color32::from_rgba_unmultiplied(24, 48, 72, 80 + pass * 40);
        theme::local_palette(palette);
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 600.0))),
                ..Default::default()
            },
            |ui| {
                let _frame = frame(ui);
                if pass > 0 {
                    ENGINE.with(|engine| {
                        let engine = engine.borrow();
                        assert!(engine.native.is_some(), "live edit discarded the native style engine");
                        assert!(!engine.values.is_empty(), "live edit forced per-control fallback documents");
                    });
                }
                let _window = super::super::style_class::enter("window");
                {
                    let _button = super::super::style_class::enter("button");
                }
                {
                    let _browser = super::super::style_class::enter("browser");
                }
            },
        )
        .textures_delta
        .clear();
        ENGINE.with(|engine| {
            let engine = engine.borrow();
            let button = engine.nodes.iter().find(|node| node.class == "button").unwrap();
            assert_eq!(engine.values[&button.key].background, palette.text_primary);
            let browser = engine.nodes.iter().find(|node| node.class == "browser").unwrap();
            let expected =
                if width > 600.0 { Color32::from_rgb(0x12, 0x34, 0x56) } else { Color32::from_rgb(0x65, 0x43, 0x21) };
            assert_eq!(engine.values[&browser.key].color, expected);
            assert!(!engine.styles_changed);
        });
    }
    theme::local_palette(saved);
    skin::configure(&skin::Skin::default());
}
#[test]
#[ignore = "manual timing comparison for live palette edits"]
fn measure_primary_colour_edits() {
    let saved = theme::palette();
    let css = include_str!("../../themes/suhina-sheet.theme.css");
    prepare(css, 800.0);
    let ctx = egui::Context::default();
    let mut samples = Vec::new();
    for sample in 0..62 {
        let mut palette = saved;
        palette.text_primary = Color32::from_rgba_unmultiplied(40, 60, 50, 100 + sample);
        theme::local_palette(palette);
        let start = std::time::Instant::now();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = frame(ui);
            let _window = super::super::style_class::enter("window");
            for class in ["toolbar", "browser", "session", "mixer", "settings"] {
                let _scope = super::super::style_class::enter(class);
                for _ in 0..8 {
                    let _button = super::super::style_class::enter("button");
                }
            }
        })
        .textures_delta
        .clear();
        if sample > 1 {
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    samples.sort_by(f64::total_cmp);
    eprintln!("primary colour edit: median {:.3} ms, max {:.3} ms", samples[30], samples[59]);
    theme::local_palette(saved);
    skin::configure(&skin::Skin::default());
}
#[test]
fn unreferenced_palette_edits_keep_computed_contexts_but_variables_invalidate() {
    let saved = theme::palette();
    prepare(".browser { color: var(--accent); background-color: white; }", 800.0);
    let stack = ["browser".into()];
    let original = scoped(&stack).unwrap();
    let mut palette = saved;
    palette.grid_major = Color32::from_rgba_unmultiplied(120, 120, 120, 90);
    theme::local_palette(palette);
    prepare(".browser { color: var(--accent); background-color: white; }", 800.0);
    ENGINE.with(|engine| assert_eq!(engine.borrow().contexts.len(), 1, "unrelated opacity cleared style cache"));
    assert_eq!(scoped(&stack).unwrap().color, original.color);
    palette.accent = Color32::from_rgb(10, 30, 70);
    theme::local_palette(palette);
    prepare(".browser { color: var(--accent); background-color: white; }", 800.0);
    assert_eq!(scoped(&stack).unwrap().color, palette.accent);
    theme::local_palette(saved);
    skin::configure(&skin::Skin::default());
}
fn prepare(css: &str, width: f32) {
    let ctx = egui::Context::default();
    let s = skin::Skin { css: Some(Arc::from(css)), ..Default::default() };
    skin::configure(&s);
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 600.0))),
            ..Default::default()
        },
        |ui| begin_frame(ui),
    )
    .textures_delta
    .clear();
}
#[test]
fn a_variant_s_values_paint_over_the_theme_s_own_and_registered_defaults_apply() {
    let css = "@property --ink { syntax: \"<color>\"; initial-value: #010203; }\n\
               :root { --edge: #445566; } .browser { color: var(--ink); border-color: var(--edge); }";
    let ctx = egui::Context::default();
    let run = |variant: Option<&str>| {
        skin::configure(&skin::Skin {
            css: Some(Arc::from(css)),
            variant: variant.map(Arc::from),
            ..Default::default()
        });
        ctx.run_ui(Default::default(), |ui| begin_frame(ui)).textures_delta.clear();
        scoped(&["window".into(), "browser".into()]).unwrap()
    };
    let own = run(None);
    assert_eq!(own.color, Color32::from_rgb(1, 2, 3), "the registered initial value");
    assert_eq!(own.border, Color32::from_rgb(0x44, 0x55, 0x66));
    let chosen = run(Some(":root { --ink: #ABCDEF; --edge: #102030; }"));
    assert_eq!(chosen.color, Color32::from_rgb(0xAB, 0xCD, 0xEF));
    assert_eq!(chosen.border, Color32::from_rgb(0x10, 0x20, 0x30), "the variant wins over the theme's :root");
    skin::configure(&skin::Skin::default());
}

#[test]
fn native_engine_resolves_variables_layers_calc_and_media() {
    prepare(
        "@layer base,custom; @layer base { .browser { color:red; }} @layer custom { :root { --ink:rgb(10 20 30); } .browser { color:var(--ink); font-size:calc(10px + 2px); }} @media (max-width:500px) { .browser { color:rgb(30 40 50); }}",
        800.0,
    );
    let values = scoped(&["window".into(), "browser".into()]).unwrap();
    assert_eq!(values.color, Color32::from_rgb(10, 20, 30));
    assert!((values.font_size - 12.0).abs() < 0.1);
    prepare("@media (max-width:500px) { .browser { color:rgb(30 40 50); }}", 400.0);
    assert_eq!(scoped(&["browser".into()]).unwrap().color, Color32::from_rgb(30, 40, 50));
    skin::configure(&skin::Skin::default());
}
#[test]
fn taffy_grid_changes_real_module_rectangles_and_hidden_modules() {
    prepare(
        ".toolbar { display:grid; grid-template-columns:80px 120px; gap:10px; } #tempo { width:auto; } #transport { width:auto; } #mapping { display:none; }",
        800.0,
    );
    let modules = toolbar(
        &[("tempo".into(), 62.0), ("transport".into(), 86.0), ("mapping".into(), 64.0)],
        Vec2::new(400.0, 32.0),
    )
    .unwrap();
    assert_eq!(modules.len(), 2);
    assert!((modules[0].rect.width() - 80.0).abs() < 0.1);
    assert!((modules[1].rect.left() - 90.0).abs() < 0.1);
    skin::configure(&skin::Skin::default());
}
#[test]
fn retained_scopes_match_siblings_and_keep_named_track_identity() {
    prepare(
        ".mixer[data-object=track-2] { color:rgb(10 20 30); } .mixer + .mixer { background-color:rgb(40 50 60); }",
        800.0,
    );
    let ctx = egui::Context::default();
    for pass in 0..4 {
        let order = if pass < 2 { [1, 2] } else { [2, 1] };
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = frame(ui);
            let _window = super::super::style_class::enter("window");
            let _session = super::super::style_class::enter("session");
            for (index, track) in order.iter().enumerate() {
                let _track = super::super::style_class::enter_named("mixer", &format!("track-{track}"));
                if pass > 0 && *track == 2 {
                    assert_eq!(theme::text_primary(), Color32::from_rgb(10, 20, 30));
                }
                if (pass == 1 || pass == 3) && index == 1 {
                    assert_eq!(theme::panel_background(), Color32::from_rgb(40, 50, 60));
                }
            }
        })
        .textures_delta
        .clear();
    }
    skin::configure(&skin::Skin::default());
}
#[test]
fn disabled_state_uses_the_native_response_without_changing_its_id() {
    prepare(".button:disabled { background-color:rgb(50 60 70); }", 800.0);
    let ctx = egui::Context::default();
    let mut previous_id = None;
    for pass in 0..2 {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = frame(ui);
            let _window = super::super::style_class::enter("window");
            let response = ui.add_enabled(false, egui::Button::new("Disabled"));
            let _button = super::super::style_class::enter_response("button", &response);
            if let Some(id) = previous_id {
                assert_eq!(response.id, id);
            }
            previous_id = Some(response.id);
            assert!(!response.enabled());
            if pass == 1 {
                assert_eq!(theme::control_background(), Color32::from_rgb(50, 60, 70));
            }
        })
        .textures_delta
        .clear();
    }
    skin::configure(&skin::Skin::default());
}
#[test]
fn computed_linear_gradients_publish_their_real_rgba_pixels_and_reuse_textures() {
    prepare(".browser { background-image:linear-gradient(90deg in srgb, rgb(255 0 0 / 0), blue); }", 800.0);
    let style = scoped(&["browser".into()]).unwrap();
    ENGINE.with(|engine| assert!(engine.borrow().native.is_some()));
    assert_eq!(style.gradients.len(), 1);
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(200.0, 40.0));
        assert!(background_with(ui.painter(), rect, &style));
        assert!(background_with(ui.painter(), rect, &style));
    });
    let ramps: Vec<_> = output
        .textures_delta
        .set
        .values()
        .flat_map(|deltas| deltas.iter())
        .filter(|delta| delta.image.size() == [1024, 1])
        .collect();
    assert_eq!(ramps.len(), 1);
    let egui::ImageData::Color(image) = &ramps[0].image;
    let midpoint = image.pixels[512].to_srgba_unmultiplied();
    assert_eq!(midpoint[0], 0, "CSS alpha interpolation is premultiplied");
    assert!(midpoint[2] > 250);
    assert!((127..=129).contains(&midpoint[3]));
    assert_eq!(image.pixels[1023], Color32::BLUE);
    output.textures_delta.clear();
    skin::configure(&skin::Skin::default());
}
#[test]
fn radial_and_conic_gradients_publish_spatial_pixels_at_the_authored_origin() {
    for (css, red, blue) in [
        (
            ".browser { background-image:radial-gradient(circle 20px at 25% 75%, red 0 99%, blue 100%); }",
            (64, 191),
            (255, 0),
        ),
        (
            ".browser { background-image:conic-gradient(from 90deg at 50% 50%, red 0deg 90deg, blue 90deg 360deg); }",
            (224, 160),
            (128, 0),
        ),
        (
            ".browser { background-image:repeating-radial-gradient(circle 20px at 50% 50%, red 0 50%, blue 50% 100%); }",
            (128, 128),
            (147, 128),
        ),
    ] {
        prepare(css, 800.0);
        let style = scoped(&["browser".into()]).unwrap();
        assert_eq!(style.gradients.len(), 1);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(256.0));
            assert!(background_with(ui.painter(), rect, &style));
            assert!(background_with(ui.painter(), rect, &style));
        });
        let images: Vec<_> = output
            .textures_delta
            .set
            .values()
            .flat_map(|d| d.iter())
            .filter(|d| d.image.size() == [256, 256])
            .collect();
        assert_eq!(images.len(), 1, "spatial textures are cached");
        let egui::ImageData::Color(image) = &images[0].image;
        assert_eq!(image.pixels[red.1 * 256 + red.0], Color32::RED, "{css}");
        assert_eq!(image.pixels[blue.1 * 256 + blue.0], Color32::BLUE, "{css}");
        output.textures_delta.clear();
    }
    skin::configure(&skin::Skin::default());
}
#[test]
fn keyframes_advance_with_the_clock_and_the_motion_setting_suppresses_them() {
    let css = ".browser { color:red; animation:ink 1s linear forwards; } @keyframes ink { from {color:red;} to {color:blue;} }";
    for motion in [true, false] {
        prepare(css, 800.0);
        let ctx = egui::Context::default();
        let mut colors = Vec::new();
        for time in [0.0, 0.5, 1.1] {
            ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    let _frame = frame_with_motion(ui, motion);
                    let _browser = super::super::style_class::enter("browser");
                },
            )
            .textures_delta
            .clear();
            colors.push(ENGINE.with(|engine| {
                let engine = engine.borrow();
                let key = engine.nodes.iter().find(|n| n.class == "browser").unwrap().key;
                engine.values[&key].color
            }));
        }
        if motion {
            assert!(colors[1].r() > 20 && colors[1].b() > 20, "{colors:?}");
            assert_eq!(colors[2], Color32::BLUE);
        } else {
            assert_eq!(colors, [Color32::RED; 3]);
            assert!(ENGINE.with(|engine| engine.borrow().motions.values().all(|m| m.running.is_empty())));
        }
    }
    skin::configure(&skin::Skin::default());
}
#[test]
fn native_state_change_starts_a_transition() {
    prepare(
        ".button { background-color:red; transition:background-color 1s linear; } .button:disabled { background-color:blue; }",
        800.0,
    );
    let ctx = egui::Context::default();
    let mut colors = Vec::new();
    for (index, time) in [0.0, 0.1, 0.6, 1.2].into_iter().enumerate() {
        ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                let _frame = frame(ui);
                let response = ui.add_enabled(index == 0, egui::Button::new("Native state"));
                let _scope = super::super::style_class::enter_response("button", &response);
            },
        )
        .textures_delta
        .clear();
        colors.push(ENGINE.with(|engine| {
            let engine = engine.borrow();
            let key = engine.nodes.iter().find(|n| n.class == "button").unwrap().key;
            engine.values[&key].background
        }));
    }
    assert_eq!(colors[0], Color32::RED);
    assert!(colors[2].r() > 20 && colors[2].b() > 20, "{colors:?}");
    assert_eq!(colors[3], Color32::BLUE);
    skin::configure(&skin::Skin::default());
}
#[test]
fn computed_inset_shadow_uses_current_color_and_reuses_its_texture() {
    prepare(".browser { color:rgb(120 30 180); box-shadow:inset 8px 0 0 0 currentColor; }", 800.0);
    let style = scoped(&["browser".into()]).unwrap();
    assert_eq!(style.shadows.len(), 1);
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(40.0));
        assert!(background_with(ui.painter(), rect, &style));
        assert!(background_with(ui.painter(), rect, &style));
    });
    let images: Vec<_> = output
        .textures_delta
        .set
        .iter()
        .filter(|(id, _)| **id != egui::TextureId::default())
        .flat_map(|(_, deltas)| deltas.iter())
        .map(|d| d.image.clone())
        .collect();
    output.textures_delta.clear();
    assert_eq!(images.len(), 1);
    let egui::ImageData::Color(image) = &images[0];
    assert_eq!(image.pixels[(image.size[1] / 2) * image.size[0] + image.size[0] / 16], Color32::from_rgb(120, 30, 180));
    assert_eq!(image.pixels[(image.size[1] / 2) * image.size[0] + image.size[0] / 2].a(), 0);
    output.textures_delta.clear();
    skin::configure(&skin::Skin::default());
}
#[test]
fn keyframe_only_properties_are_applied_without_a_static_declaration() {
    prepare(
        ".browser { animation:ink 1s linear forwards; } @keyframes ink { from {color:red;} to {color:blue;} }",
        800.0,
    );
    let ctx = egui::Context::default();
    for time in [0.0, 1.1] {
        ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                let _frame = frame(ui);
                let _scope = super::super::style_class::enter("browser");
            },
        )
        .textures_delta
        .clear();
    }
    ENGINE.with(|engine| {
        let engine = engine.borrow();
        let key = engine.nodes.iter().find(|n| n.class == "browser").unwrap().key;
        assert!(engine.values[&key].authored.color);
        assert_eq!(engine.values[&key].color, Color32::BLUE);
    });
    skin::configure(&skin::Skin::default());
}
#[test]
fn workspace_grid_resolves_real_panels_and_respects_native_visibility() {
    prepare(
        "#workspace { display:grid; grid-template-columns:minmax(0,1fr) 240px; grid-template-rows:minmax(0,1fr) 160px; gap:8px; } #panel-browser {grid-column:2;grid-row:1 / -1;} #panel-main {grid-column:1;grid-row:1;} #panel-detail {grid-column:1;grid-row:2;}",
        800.0,
    );
    let layout = workspace(Vec2::new(800.0, 600.0), Some(200.0), Some((false, 120.0))).unwrap();
    assert_eq!(layout.main, Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(552.0, 432.0)));
    assert_eq!(layout.browser.unwrap().left(), 560.0);
    assert_eq!(layout.detail.unwrap().top(), 440.0);
    let closed = workspace(Vec2::new(800.0, 600.0), None, None).unwrap();
    assert!(closed.browser.is_none() && closed.detail.is_none());
    prepare(".browser {color:red;}", 800.0);
    assert!(workspace(Vec2::new(800.0, 600.0), Some(200.0), Some((false, 120.0))).is_none());
    skin::configure(&skin::Skin::default());
}
#[test]
fn unrelated_css_preserves_native_colours_font_sizes_and_corner_defaults() {
    prepare(".browser { color:rgb(10 20 30); }", 800.0);
    let original = theme::palette();
    {
        let _mixer = super::super::style_class::enter("mixer");
        assert_eq!(theme::palette(), original);
        assert_eq!(super::super::style_class::font_size(9.0), 9.0);
        assert_eq!(super::super::style_class::radius(4.0), 4.0);
    }
    {
        let _browser = super::super::style_class::enter("browser");
        assert_eq!(theme::text_primary(), Color32::from_rgb(10, 20, 30));
    }
    skin::configure(&skin::Skin::default());
}

/// Runs `paint` inside the window for a few frames with `css`, so element ids resolve, and
/// returns the last frame's shapes.
fn painted(css: &str, mut paint: impl FnMut(&mut egui::Ui)) -> Vec<egui::epaint::ClippedShape> {
    prepare(css, 800.0);
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                ..Default::default()
            },
            |ui| {
                let _frame = frame(ui);
                let _window = super::super::style_class::enter("window");
                paint(ui);
            },
        );
        output.textures_delta.clear();
        shapes = std::mem::take(&mut output.shapes);
    }
    skin::configure(&skin::Skin::default());
    shapes
}

fn rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::epaint::RectShape> {
    shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) => Some(rect.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_level_bar_takes_its_shape_and_thickness_from_the_sheet() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(200.0, 20.0));
    let draw = |ui: &mut egui::Ui| {
        let _value = super::super::style_class::enter_named("value", "arrangement-volume");
        crate::kit::mixer_controls::level_bar(
            ui.painter(),
            field,
            "volume",
            (0.0, 0.5),
            0.0,
            "0.0",
            egui::Align2::RIGHT_CENTER,
        );
    };
    // Natively a capsule as tall as the field.
    let native = rects(&painted("", draw));
    assert!(native.iter().any(|r| r.rect == field && r.corner_radius.nw == 10), "{native:?}");
    let css = "#arrangement-volume { height: 4px; border-radius: 0; background-color: #202020; \
               border-width: 1px; border-color: #FFFFFF; } \
               #arrangement-volume .value-fill { background-color: #FF0000; height: 4px; }";
    let shaped = rects(&painted(css, draw));
    let track = Rect::from_center_size(field.center(), Vec2::new(200.0, 4.0));
    assert!(
        shaped
            .iter()
            .any(|r| r.rect == track && r.fill == egui::Color32::from_rgb(0x20, 0x20, 0x20) && r.corner_radius.nw == 0),
        "{shaped:?}"
    );
    assert!(shaped.iter().any(|r| r.rect == track && r.stroke.width == 1.0), "the border: {shaped:?}");
    assert!(
        shaped.iter().any(|r| r.fill == egui::Color32::from_rgb(0xFF, 0, 0) && r.rect.height() == 4.0),
        "{shaped:?}"
    );
}

#[test]
fn a_transparent_control_background_paints_nothing() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(60.0, 20.0));
    let shapes = painted("#tempo { background-color: transparent; }", |ui| {
        let _toolbar = super::super::style_class::enter("toolbar");
        let _module = super::super::style_class::enter_named("toolbar-module", "tempo");
        surface(ui.painter(), field, 0.0, theme::window_background());
    });
    assert!(!rects(&shapes).iter().any(|r| r.rect == field), "{shapes:?}");
}

#[test]
fn a_view_background_leaves_the_controls_on_it_their_own_fill() {
    let control = theme::control_background();
    let mut seen = None;
    painted(".arrangement { background-color: #102030; }", |_| {
        let _view = super::super::style_class::enter("arrangement");
        seen = Some((theme::window_background(), theme::control_background()));
    });
    let (window, inside) = seen.unwrap();
    assert_eq!(window, egui::Color32::from_rgb(0x10, 0x20, 0x30));
    assert_eq!(inside, control, "the add-track buttons keep the theme's control fill");
}

#[test]
fn a_level_bar_or_surface_squeezed_below_a_point_draws_nothing_and_never_panics() {
    let css = "#arrangement-volume { height: 4px; border-radius: 3px; box-shadow: 0 1px 2px #000; }";
    for field in [
        Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(200.0, 0.968)),
        Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(0.5, 20.0)),
        Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(200.0, -3.0)),
        Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(200.0, f32::NAN)),
    ] {
        painted(css, |ui| {
            let _value = super::super::style_class::enter_named("value", "arrangement-volume");
            crate::kit::mixer_controls::level_bar(
                ui.painter(),
                field,
                "volume",
                (0.0, 0.5),
                0.0,
                "0.0",
                egui::Align2::RIGHT_CENTER,
            );
            surface(ui.painter(), field, 4.0, theme::control_background());
        });
    }
}

#[test]
fn checked_and_selected_states_reach_the_sheet() {
    let css = "#control-solo:checked { background-color: #FF8800; } \
               .track-header:selected { background-color: #102030; }";
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(20.0, 16.0));
    for on in [false, true] {
        let mut header = None;
        let shapes = painted(css, |ui| {
            let response = ui.interact(field, egui::Id::new("solo-test"), egui::Sense::click());
            {
                let _solo = super::super::style_class::enter_named_response("button", "solo", &response);
                mark(HostState::Checked, on);
                crate::kit::widgets::button_surface(
                    ui.painter(),
                    field,
                    &response,
                    on,
                    theme::control_background(),
                    0.0,
                );
            }
            let _header = super::super::style_class::enter_named("track-header", "track-1");
            mark(HostState::Selected, on);
            header = Some(theme::window_background());
        });
        let orange = rects(&shapes).iter().any(|r| r.rect == field && r.fill == egui::Color32::from_rgb(0xFF, 0x88, 0));
        assert_eq!(orange, on, "the solo button is orange only when on");
        assert_eq!(
            header == Some(egui::Color32::from_rgb(0x10, 0x20, 0x30)),
            on,
            "the header's fill only when selected"
        );
    }
}

#[test]
fn a_menu_opened_from_a_light_text_bar_reads_on_its_own_background() {
    let mut seen = None;
    painted(".menu-bar { background-color: #217346; color: #ffffff; }", |_| {
        let _bar = super::super::style_class::enter("menu-bar");
        let _menu = super::super::style_class::enter("menu");
        let ink = super::super::style_class::readable_on(egui::Color32::WHITE);
        seen = Some((ink, theme::text_primary()));
    });
    let (ink, text) = seen.unwrap();
    assert!(ink.is_some(), "the bar's white text is replaced in the menu");
    assert!(theme::contrast(text, egui::Color32::WHITE) >= 3.0, "{text:?}");
}

#[test]
fn a_selected_header_s_background_never_fills_its_buttons() {
    let mut seen = None;
    painted(".track-header:selected { background-color: #304050; }", |_| {
        let _header = super::super::style_class::enter_named("track-header", "track-1");
        mark(HostState::Selected, true);
        let shown = super::super::style_class::background(egui::Color32::BLACK);
        let _button = super::super::style_class::enter("button");
        seen = Some((shown, theme::control_background()));
    });
    let (shown, control) = seen.unwrap();
    assert_eq!(shown, egui::Color32::from_rgb(0x30, 0x40, 0x50), "the header shows the sheet's colour");
    assert_ne!(control, shown, "its buttons keep their own fill");
}

#[test]
fn a_track_gives_its_colour_to_the_sheet_and_each_view_is_known() {
    let mut seen = None;
    painted(".mixer .track-name { background-color: var(--track-colour); }", |_| {
        let _session = super::super::style_class::enter("session");
        let _strip = super::super::style_class::enter_named("mixer", "track-1");
        set_variable("--track-colour", hex(egui::Color32::from_rgb(0xE8, 0x91, 0x3A)));
        let _name = super::super::style_class::enter_named("track-name", "track-1");
        seen = Some((theme::window_background(), super::super::style_class::view()));
    });
    let (fill, view) = seen.unwrap();
    assert_eq!(fill, egui::Color32::from_rgb(0xE8, 0x91, 0x3A));
    assert_eq!(view, Some("mixer"), "a strip in the Launcher view is the mixer's");
    assert_eq!(hex(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 128)), "#ff000080");
}

#[test]
fn artwork_reads_the_variables_of_the_elements_in_scope() {
    let mut seen = Vec::new();
    painted("", |_| {
        let _header = super::super::style_class::enter_named("track-header", "track-1");
        set_variable("--track-colour", hex(egui::Color32::from_rgb(0xE8, 0x91, 0x3A)));
        let _button = super::super::style_class::enter("button");
        seen = scope_variables();
    });
    assert_eq!(seen, [("--track-colour".to_string(), "#e8913a".to_string())]);
    let mut theme = std::collections::BTreeMap::new();
    theme.insert("--track-colour".to_string(), "#000000".to_string());
    theme.extend(seen);
    let svg = r#"<rect fill="var(--track-colour, #ff00ff)"/>"#;
    assert_eq!(crate::kit::skin::substitute_variables(svg, &theme), r##"<rect fill="#e8913a"/>"##);
}

#[test]
fn a_knob_s_value_goes_where_the_sheet_puts_it() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(60.0, 60.0));
    // The position is a host rule, read from the theme's sheet as the app reads it.
    let text_top = |css: &str| {
        let (theme, _) = super::super::theme::from_sheet(css, "");
        let shapes = painted("", |ui| {
            skin::configure(&theme.skin);
            let _pan = super::super::style_class::enter("pan");
            crate::kit::widgets::knob_with_value(
                ui.painter(),
                field,
                0.5,
                egui::Color32::WHITE,
                None,
                "C",
                theme::text_secondary(),
                crate::kit::widgets::ValuePosition::Right,
                theme::TEXT_SMALL,
            );
        });
        shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.pos.y),
            _ => None,
        })
    };
    let beside = text_top("").unwrap();
    assert!(beside > field.top() + 10.0, "beside the knob, in the middle of the field");
    let above = text_top(".pan { --suhina-value-position: top; }").unwrap();
    assert!(above < field.top() + 4.0, "over the knob: {above}");
    assert_eq!(text_top(".pan { --suhina-value-position: none; }"), None);
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_level_bar_s_value_may_sit_left_and_its_lit_part_take_its_own_ink() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(200.0, 20.0));
    let css = "#arrangement-volume { text-align: left; } #arrangement-volume .value-fill { color: #00FF00; }";
    let shapes = painted(css, |ui| {
        let _value = super::super::style_class::enter_named("value", "arrangement-volume");
        crate::kit::mixer_controls::level_bar(
            ui.painter(),
            field,
            "volume",
            (0.0, 0.9),
            0.0,
            "0.0",
            egui::Align2::RIGHT_CENTER,
        );
    });
    let green = shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text)
            if text.galley.job.sections.iter().any(|s| s.format.color == egui::Color32::from_rgb(0, 0xFF, 0)) =>
        {
            Some(text.pos.x)
        }
        _ => None,
    });
    let x = green.expect("the lit part's text is green");
    assert!(x < field.center().x, "the value sits at the left: {x}");
}

#[test]
fn a_sheet_s_border_is_the_ordinary_widgets_outline() {
    let mut strokes = Vec::new();
    for css in [".dropdown { border-width: 0px; }", ".dropdown { border-width: 2px; border-color: #FF0000; }"] {
        let mut seen = None;
        painted(css, |ui| {
            let _menu = super::super::style_class::enter("dropdown");
            let mut child = ui.new_child(egui::UiBuilder::new());
            let styled = super::super::style_class::ui(&mut child);
            seen = Some(styled.visuals().widgets.inactive.bg_stroke);
        });
        strokes.push(seen.unwrap());
    }
    assert_eq!(strokes[0].width, 0.0);
    assert_eq!(strokes[1], egui::Stroke::new(2.0, egui::Color32::from_rgb(0xFF, 0, 0)));
}

#[test]
fn the_fader_cap_and_rail_take_their_size_from_the_sheet() {
    let mut sizes = (Vec2::ZERO, 0.0);
    painted("", |_| sizes = (crate::session::mixer_strip::cap_size(), crate::session::mixer_strip::rail_width()));
    assert_eq!(sizes, (Vec2::new(20.0, 26.0), 5.0));
    painted(".fader-cap { width: 30px; height: 12px; } .fader-rail { width: 2px; }", |_| {
        sizes = (crate::session::mixer_strip::cap_size(), crate::session::mixer_strip::rail_width());
    });
    assert_eq!(sizes, (Vec2::new(30.0, 12.0), 2.0));
}

#[test]
fn the_add_track_buttons_take_their_class_and_ids() {
    let css = ".add-track { background-color: #00FF00; } #add-return-track { background-color: #FF0000; }";
    let fills: Vec<_> = rects(&painted(css, |ui| {
        let area = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(300.0, 300.0));
        crate::interaction::empty_space::empty_space(ui, &ui.painter().clone(), area, &mut Vec::new());
    }))
    .into_iter()
    .map(|r| r.fill)
    .collect();
    assert_eq!(fills.iter().filter(|f| **f == egui::Color32::from_rgb(0, 0xFF, 0)).count(), 2, "{fills:?}");
    assert!(fills.contains(&egui::Color32::RED), "{fills:?}");
}

#[test]
fn the_ruler_loop_region_and_playhead_take_their_classes() {
    use crate::arrangement::{arrangement_view, timeline};
    let mut timeline = timeline::Timeline::build(&crate::demo::demo_project(), None);
    timeline.loop_brace = Some(timeline::LoopBrace { start: 0.0, end: 8.0, enabled: true });
    timeline.beat = 2.0;
    let css = ".ruler { background-color: #FF0000; } \
               .loop-region { background-color: #000080; } \
               .loop-region:checked { background-color: #00FF00; height: 6px; } \
               .playhead { color: #0000FF; width: 3px; }";
    let mut state = arrangement_view::ArrangementViewState::default();
    let shapes = painted(css, |ui| arrangement_view::show(ui, &timeline, &mut state, &mut Vec::new()));
    let fills: Vec<_> = rects(&shapes);
    assert!(fills.iter().any(|r| r.fill == egui::Color32::RED), "the ruler");
    assert!(fills.iter().any(|r| r.fill == egui::Color32::from_rgb(0, 0xFF, 0) && r.rect.height() == 6.0), "the loop");
    assert!(
        shapes.iter().any(|s| matches!(&s.shape, egui::Shape::LineSegment { stroke, .. }
            if stroke.width == 3.0 && stroke.color == egui::Color32::from_rgb(0, 0, 0xFF))),
        "the playhead"
    );
}

#[test]
fn a_clip_under_the_play_position_is_playing() {
    use crate::arrangement::{arrangement_view, timeline};
    let mut timeline = timeline::Timeline::build(&crate::demo::demo_project(), None);
    let first = timeline.lanes.iter().flat_map(|l| &l.clips).next().expect("the demo has clips");
    timeline.beat = first.start + first.length / 2.0;
    let css = ".clip:playing { background-color: #FF0000; }";
    let mut state = arrangement_view::ArrangementViewState::default();
    let stopped = rects(&painted(css, |ui| arrangement_view::show(ui, &timeline, &mut state, &mut Vec::new())));
    assert!(!stopped.iter().any(|r| r.fill == egui::Color32::RED), "stopped, nothing plays");
    timeline.running = true;
    let running = rects(&painted(css, |ui| arrangement_view::show(ui, &timeline, &mut state, &mut Vec::new())));
    assert!(running.iter().any(|r| r.fill == egui::Color32::RED), "the clip under the play position");
}

#[test]
fn a_level_bar_draws_a_handle_where_its_fill_ends() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(200.0, 20.0));
    let draw = |ui: &mut egui::Ui| {
        let _value = super::super::style_class::enter_named("value", "arrangement-volume");
        crate::kit::mixer_controls::level_bar(
            ui.painter(),
            field,
            "volume",
            (0.0, 0.5),
            0.0,
            "0.0",
            egui::Align2::RIGHT_CENTER,
        );
    };
    assert!(!rects(&painted("", draw)).iter().any(|r| r.fill == egui::Color32::RED));
    let shaped = rects(&painted("#arrangement-volume .value-handle { width: 6px; background-color: #FF0000; }", draw));
    let handle = shaped.iter().find(|r| r.fill == egui::Color32::RED).expect("the handle");
    assert_eq!(handle.rect.center().x, 110.0);
    assert_eq!(handle.rect.width(), 6.0);
}

#[test]
fn piano_roll_notes_take_the_clip_colour_and_their_states() {
    use crate::detail::piano_roll::part_style;
    use crate::kit::native_css::HostState;
    let css = ".piano-note { background-color: var(--clip-colour); } \
               .piano-note:selected { border-color: #FFFFFF; } \
               .piano-key-black { background-color: #000000; }";
    let mut seen = None;
    painted(css, |_| {
        let _roll = super::super::style_class::enter("piano-roll");
        super::set_variable("--clip-colour", super::hex(egui::Color32::from_rgb(0x12, 0x34, 0x56)));
        seen = Some((
            part_style("piano-note", &[]),
            part_style("piano-note", &[HostState::Selected]),
            part_style("piano-key-black", &[]),
        ));
    });
    let (note, selected, black) = seen.unwrap();
    assert_eq!(note.background, Some(egui::Color32::from_rgb(0x12, 0x34, 0x56)));
    assert_eq!(note.border, None);
    assert_eq!(selected.border, Some(egui::Color32::WHITE));
    assert_eq!(black.background, Some(egui::Color32::BLACK));
}

#[test]
fn egui_widgets_in_a_scope_take_the_generic_button_and_input_rules() {
    let css = ".settings .button { background-color: #101010; border-width: 0px; } \
               .settings .button:hover { background-color: #202020; } \
               .settings .button:active { background-color: #303030; } \
               .settings .button:checked { background-color: #404040; } \
               .settings .text-input { background-color: #050505; }";
    let mut seen = None;
    painted(css, |ui| {
        let _settings = super::super::style_class::enter("settings");
        let styled = super::super::style_class::ui(ui);
        let visuals = styled.style().visuals.clone();
        seen = Some(visuals);
    });
    let visuals = seen.unwrap();
    assert_eq!(visuals.widgets.inactive.bg_fill, egui::Color32::from_rgb(0x10, 0x10, 0x10));
    assert_eq!(visuals.widgets.inactive.bg_stroke.width, 0.0, "egui's own outline can be removed");
    assert_eq!(visuals.widgets.hovered.bg_fill, egui::Color32::from_rgb(0x20, 0x20, 0x20));
    assert_eq!(visuals.widgets.active.bg_fill, egui::Color32::from_rgb(0x30, 0x30, 0x30));
    assert_eq!(visuals.selection.bg_fill, egui::Color32::from_rgb(0x40, 0x40, 0x40));
    assert_eq!(visuals.extreme_bg_color, egui::Color32::from_rgb(5, 5, 5));
}

#[test]
fn a_built_in_device_s_panel_is_selected_by_its_id() {
    let css = ".device { background-color: #00FF00; } #device-compressor { background-color: #FF0000; } \
               #device-compressor .device-header { background-color: #0000FF; }";
    let mut seen = Vec::new();
    painted(css, |_| {
        seen.clear();
        for id in ["device-compressor", "device-limiter"] {
            let _device = super::super::style_class::enter_named("device", id);
            let panel = super::super::style_class::computed().map(|c| c.background);
            let _header = super::super::style_class::enter("device-header");
            seen.push((
                panel,
                super::super::style_class::computed().filter(|c| c.authored.background).map(|c| c.background),
            ));
        }
    });
    assert_eq!(seen[0], (Some(egui::Color32::RED), Some(egui::Color32::BLUE)));
    assert_eq!(seen[1], (Some(egui::Color32::from_rgb(0, 0xFF, 0)), None));
}

#[test]
fn a_clip_s_radius_reaches_its_name_bar() {
    use crate::arrangement::{arrangement_view, timeline};
    let timeline = timeline::Timeline::build(&crate::demo::demo_project(), None);
    let mut state = arrangement_view::ArrangementViewState::default();
    let css = ".clip { border-radius: 10px; } .clip-name { background-color: #FF0000; }";
    let shapes = rects(&painted(css, |ui| arrangement_view::show(ui, &timeline, &mut state, &mut Vec::new())));
    let bars: Vec<_> = shapes.iter().filter(|r| r.fill == egui::Color32::RED).collect();
    assert!(!bars.is_empty(), "the name bars");
    assert!(bars.iter().all(|r| r.corner_radius.nw > 0 && r.corner_radius.sw == 0), "{bars:?}");
}

#[test]
fn the_midi_tools_column_takes_its_width_from_the_sheet() {
    let mut widths = (0.0, 0.0);
    painted("", |_| widths.0 = crate::detail::midi_tools::tools_width());
    painted(".midi-tools { width: 220px; }", |_| widths.1 = crate::detail::midi_tools::tools_width());
    assert_eq!(widths, (crate::detail::midi_tools::TOOLS_WIDTH, 220.0));
}

#[test]
fn device_sliders_take_parts_and_knobs_from_the_sheet_and_a_panel_colour_spares_them() {
    use crate::detail::detail_view::{slider_as_knob, themed_slider};
    let mut seen = (true, true);
    painted("", |_| seen = (themed_slider(), slider_as_knob()));
    assert_eq!(seen, (false, false));
    painted(".slider-handle { background-color: #FF0000; }", |_| seen = (themed_slider(), slider_as_knob()));
    assert_eq!(seen, (true, false));
    // The appearance is a host rule, read from the theme's sheet as the app reads it.
    let (theme, _) = super::super::theme::from_sheet(".slider { --suhina-appearance: knob; }", "");
    skin::configure(&theme.skin);
    assert!(slider_as_knob());
    skin::configure(&skin::Skin::default());
    let mut fill = None;
    painted("#device-plugin { background-color: #102030; }", |ui| {
        let _device = super::super::style_class::enter_named("device", "device-plugin");
        let styled = super::super::style_class::ui(ui);
        fill = Some(styled.style().visuals.widgets.inactive.bg_fill);
    });
    assert_ne!(fill, Some(egui::Color32::from_rgb(0x10, 0x20, 0x30)), "the rails keep their own colour");
}

#[test]
fn a_round_button_s_shadow_is_round() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(24.0, 24.0));
    let shapes = painted("#control-stop { border-radius: 50%; background-color: #FF0000; }", |ui| {
        let response = ui.interact(field, egui::Id::new("stop"), egui::Sense::click());
        let _stop = super::super::style_class::enter_named_response("button", "stop", &response);
        crate::kit::widgets::button_surface(ui.painter(), field, &response, false, egui::Color32::GRAY, 0.0);
    });
    let button = rects(&shapes).into_iter().find(|r| r.fill == egui::Color32::RED).expect("the button");
    assert_eq!(button.corner_radius.nw, 12, "50 % is round");
}

#[test]
fn a_theme_sets_how_strongly_the_track_colour_tints_its_header() {
    let (theme, _) = super::super::theme::from_sheet(".arrangement { --suhina-header-tint: 0; }", "");
    skin::configure(&theme.skin);
    assert_eq!(super::super::style_class::theme_header_tint(), Some(0));
    skin::configure(&skin::Skin::default());
    assert_eq!(super::super::style_class::theme_header_tint(), None);
}

#[test]
fn the_native_toolbar_paints_its_gradient() {
    let bar = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 40.0));
    let mut painted_any = false;
    let shapes = painted(".toolbar { background-image: linear-gradient(90deg, #ff0000, #0000ff); }", |ui| {
        let _toolbar = super::super::style_class::enter("toolbar");
        painted_any = background(ui.painter(), bar);
    });
    assert!(painted_any, "{shapes:?}");
}

#[test]
fn a_theme_gives_tracks_without_a_height_its_own() {
    let (theme, _) = super::super::theme::from_sheet(".arrangement { --suhina-track-height: 96px; }", "");
    skin::configure(&theme.skin);
    assert_eq!(super::super::style_class::theme_track_height(), Some(96.0));
    let (theme, _) = super::super::theme::from_sheet(".arrangement { --suhina-track-height: 4px; }", "");
    skin::configure(&theme.skin);
    assert_eq!(super::super::style_class::theme_track_height(), None, "out of range is left out");
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_value_s_number_and_unit_follow_the_sheet() {
    let (theme, warnings) =
        super::super::theme::from_sheet(".pan { --suhina-value-decimals: 0; --suhina-value-unit: \" dB\"; }", "");
    let mut seen = Vec::new();
    painted("", |_| {
        skin::configure(&theme.skin);
        let _pan = super::super::style_class::enter("pan");
        seen = ["-3.6", "-inf", "C"].map(crate::kit::widgets::formatted_value).to_vec();
    });
    assert_eq!(seen, ["-4 dB", "-inf", "C"], "{warnings:?}");
}

#[test]
fn a_family_class_reaches_every_part_of_its_kind() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(24.0, 24.0));
    let css = ".value-control { background-color: #FF0000; } .track-button { background-color: #00FF00; }";
    let mut pan = None;
    let shapes = painted(css, |ui| {
        {
            let _pan = super::super::style_class::enter("pan");
            pan = Some(super::super::style_class::background(egui::Color32::BLACK));
        }
        let response = ui.interact(field, egui::Id::new("solo"), egui::Sense::click());
        let _solo = super::super::style_class::enter_named_response("button", "solo", &response);
        surface(ui.painter(), field, 0.0, egui::Color32::BLACK);
    });
    assert_eq!(pan, Some(egui::Color32::from_rgb(0xFF, 0, 0)), ".value-control reaches .pan");
    assert!(
        rects(&shapes).iter().any(|r| r.fill == egui::Color32::from_rgb(0, 0xFF, 0)),
        ".track-button reaches #control-solo"
    );
    // Host rules read the family too.
    let (theme, _) = super::super::theme::from_sheet(".value-control { --suhina-appearance: knob; }", "");
    skin::configure(&theme.skin);
    let _send = super::super::style_class::enter("send");
    assert!(super::super::style_class::knob_preference(false));
    drop(_send);
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_level_bar_s_fill_can_take_the_track_s_colour() {
    let field = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(200.0, 20.0));
    let css = ".value-fill { background-color: var(--track-colour); }";
    let shapes = painted(css, |ui| {
        let _header = super::super::style_class::enter_named("track-header", "track-1");
        set_variable("--track-colour", hex(egui::Color32::from_rgb(0xE8, 0x91, 0x3A)));
        let _value = super::super::style_class::enter_named("value", "arrangement-volume");
        crate::kit::mixer_controls::level_bar(
            ui.painter(),
            field,
            "volume",
            (0.0, 0.7),
            0.0,
            "0.0",
            egui::Align2::RIGHT_CENTER,
        );
    });
    let orange = egui::Color32::from_rgb(0xE8, 0x91, 0x3A);
    assert!(rects(&shapes).iter().any(|r| r.fill == orange), "the lit part is the track's colour");
}

#[test]
fn a_low_track_keeps_its_native_layout() {
    use crate::kit::control_layout::{native_only, placement};
    let (theme, _) = super::super::theme::from_sheet(
        ".arrangement { --suhina-compact-below: 48px; }",
        ".control-arrangement-enable { left: -30px; }",
    );
    skin::configure(&theme.skin);
    assert_eq!(super::super::style_class::theme_compact_below(), Some(48.0));
    let placed = placement("arrangement-enable");
    assert_ne!(placed, Default::default());
    {
        let _low = native_only(true);
        assert_eq!(placement("arrangement-enable"), Default::default(), "a low track: native");
    }
    assert_eq!(placement("arrangement-enable"), placed, "the next track places it again");
    skin::configure(&skin::Skin::default());
}
