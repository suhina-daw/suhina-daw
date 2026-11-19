// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{selector::Element, value::Length};

fn viewport(width: f32) -> Viewport {
    Viewport { width, height: 700.0, reduced_motion: false, light: false }
}
fn tree() -> Tree {
    Tree::new(vec![
        Element::new("window", None),
        Element::new("device-panel", Some(0)),
        Element::new("knob", Some(1)).with_id("cutoff"),
        Element::new("device-panel", Some(0)),
        Element::new("knob", Some(3)).with_id("cutoff"),
    ])
    .unwrap()
}

#[test]
fn device_styles_inherit_app_defaults_and_user_overrides_stay_scoped() {
    let app = Sheet::parse("window { --accent: #79e2bd; color: #eeeeee; } knob { radius: 4px; }").unwrap();
    let device = Sheet::parse("knob#cutoff { color: var(--accent); radius: 8px !important; }").unwrap();
    let user = Sheet::parse("#cutoff { radius: 12px; }").unwrap();
    let styles = compute(
        &tree(),
        viewport(1138.0),
        &[
            AppliedSheet { sheet: &app, layer: Layer::Theme, root: None },
            AppliedSheet { sheet: &device, layer: Layer::Device, root: Some(1) },
            AppliedSheet { sheet: &user, layer: Layer::User, root: Some(1) },
        ],
    )
    .unwrap();
    assert_eq!(styles[2].get(Property::Color), Value::Color(Color::Rgba([121, 226, 189, 255])));
    assert_eq!(styles[2].get(Property::Radius), Value::Length(Length::Px(12.0)));
    assert_eq!(styles[4].get(Property::Color), Value::Color(Color::Rgba([238, 238, 238, 255])));
    assert_eq!(styles[4].get(Property::Radius), Value::Length(Length::Px(4.0)));
}

#[test]
fn viewport_changes_and_inherited_variables_recompute_without_stale_values() {
    let sheet = Sheet::parse("window { --ink: #123456; --inherited: var(--ink); } device-panel { --ink: #abcdef; } knob { color: var(--inherited); width: 50%; } @media (max-width: 900px) { knob { color: var(--ink); } }").unwrap();
    assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
    let applied = [AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }];
    let wide = compute(&tree(), viewport(1138.0), &applied).unwrap();
    let narrow = compute(&tree(), viewport(800.0), &applied).unwrap();
    assert_eq!(wide[2].get(Property::Color), Value::Color(Color::Rgba([18, 52, 86, 255])));
    assert_eq!(narrow[2].get(Property::Color), Value::Color(Color::Rgba([171, 205, 239, 255])));
    assert_eq!(wide[2].get(Property::Width), Value::Length(Length::Percent(0.5)));
}

#[test]
fn malformed_declarations_report_locations_and_do_not_erase_valid_ones() {
    let sheet =
        Sheet::parse("knob { color: red; radius: NaNpx; opacity: 0.5; }\nknob { imaginary: 1; color: wat; }").unwrap();
    assert_eq!(sheet.notices.len(), 3);
    assert_eq!(sheet.notices[1].line, 2);
    let values =
        compute(&tree(), viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }]).unwrap();
    assert_eq!(values[2].get(Property::Color), Value::Color(Color::Rgba([255, 0, 0, 255])));
    assert_eq!(values[2].get(Property::Opacity), Value::Number(0.5));
    assert!(compute(&tree(), viewport(f32::NAN), &[]).is_err());
}

#[test]
fn relative_font_size_is_computed_once_before_inheritance() {
    let sheet = Sheet::parse("window { font-size: 10px; } device-panel { font-size: 2em; }").unwrap();
    let styles =
        compute(&tree(), viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }]).unwrap();
    assert_eq!(styles[1].get(Property::FontSize), Value::Length(Length::Px(20.0)));
    assert_eq!(styles[2].get(Property::FontSize), Value::Length(Length::Px(20.0)));
}

#[test]
fn cascade_layers_order_normal_and_reverse_important_declarations() {
    let color = |css: &str| {
        let sheet = Sheet::parse(css).unwrap_or_else(|e| panic!("{e}"));
        assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
        let styles =
            compute(&tree(), viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }])
                .unwrap_or_else(|e| panic!("{e}"));
        styles[2].get(Property::Color)
    };
    let red = Value::Color(Color::Rgba([255, 0, 0, 255]));
    let blue = Value::Color(Color::Rgba([0, 0, 255, 255]));
    // A later layer wins over an earlier one whatever the specificity.
    assert_eq!(color("@layer a { #cutoff { color: red; } } @layer b { knob { color: blue; } }"), blue);
    // The order statement decides, not where the rules appear.
    assert_eq!(color("@layer b, a; @layer a { knob { color: red; } } @layer b { knob { color: blue; } }"), red);
    // Unlayered rules win over layered ones.
    assert_eq!(color("knob { color: blue; } @layer a { #cutoff { color: red; } }"), blue);
    // !important reverses: the earliest layer's important wins, unlayered important loses.
    assert_eq!(
        color("@layer a { knob { color: red !important; } } @layer b { knob { color: blue !important; } }"),
        red
    );
    assert_eq!(color("knob { color: blue !important; } @layer a { knob { color: red !important; } }"), red);
    // A layer's own rules win over its nested layers.
    assert_eq!(color("@layer a { knob { color: red; } @layer b { knob { color: blue; } } }"), red);
    assert_eq!(
        color("@layer a.b; @layer a { @layer b { knob { color: blue; } } } @layer c { knob { color: red; } }"),
        red
    );
    // Media inside a layer keeps the layer.
    assert_eq!(
        color("@layer a { @media (max-width: 900px) { knob { color: red; } } } @layer b { knob { color: blue; } }"),
        blue
    );
}

#[test]
fn invalid_or_excessive_layers_are_notices_not_crashes() {
    let sheet = Sheet::parse("@layer 1bad { knob { color: red; } } knob { color: blue; }").unwrap_or_default();
    assert_eq!(sheet.notices.len(), 1);
    let many: String = (0..80).map(|i| format!("@layer l{i} {{ knob {{ color: red; }} }}")).collect();
    assert!(Sheet::parse(&many).is_err());
}

#[test]
fn background_shorthand_sets_colour_and_images_and_resets_an_omitted_colour() {
    let style = |css: &str| {
        let sheet = Sheet::parse(css).unwrap_or_else(|e| panic!("{e}"));
        let styles =
            compute(&tree(), viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }])
                .unwrap_or_else(|e| panic!("{e}"));
        (sheet.notices.len(), styles[2].clone())
    };
    let (notices, knob) = style("knob { background-color: red; background: linear-gradient(red, blue); }");
    assert_eq!(notices, 0);
    // As in CSS, the shorthand resets the colour it omits: the native fill becomes transparent.
    assert_eq!(knob.get(Property::BackgroundColor), Value::Color(Color::Rgba([0; 4])));
    assert!(knob.authored(Property::BackgroundColor));
    assert!(matches!(knob.get(Property::BackgroundImage), Value::Gradients(g) if g.len() == 1));
    let (_, knob) = style(":root { --base: #102030; } knob { background: linear-gradient(red, blue), var(--base); }");
    assert_eq!(knob.get(Property::BackgroundColor), Value::Color(Color::Rgba([16, 32, 48, 255])));
    let (_, knob) = style("knob { background: none; }");
    assert_eq!(knob.get(Property::BackgroundImage), Value::Gradients(Vec::new()));
    for bad in
        ["url(a.png)", "red center / cover", "red, blue", "linear-gradient(red, blue) linear-gradient(red, blue)"]
    {
        let (notices, _) = style(&format!("knob {{ background: {bad}; }}"));
        assert_eq!(notices, 1, "{bad}");
    }
    // Background longhands written later still win over the shorthand.
    let (_, knob) = style("knob { background: red; background-color: blue; }");
    assert_eq!(knob.get(Property::BackgroundColor), Value::Color(Color::Rgba([0, 0, 255, 255])));
}

#[test]
fn keyframes_resolve_for_each_element_with_its_variables() {
    let css = ":root { --to: blue; } knob { animation: ink 1s linear forwards; } \
                   @keyframes ink { from { color: red; } 50%, to { color: var(--to); opacity: .5; } }";
    let sheet = Sheet::parse(css).unwrap_or_else(|e| panic!("{e}"));
    assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
    let styles = compute(&tree(), viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }])
        .unwrap_or_else(|e| panic!("{e}"));
    let animations = styles[2].animations();
    assert_eq!(animations.len(), 1);
    let frames = &animations[0].1;
    assert_eq!(frames.iter().map(|f| f.0).collect::<Vec<_>>(), [0.0, 0.5, 1.0]);
    assert_eq!(frames[2].1[0], (Property::Color, Value::Color(Color::Rgba([0, 0, 255, 255]))));
    assert!(styles[2].authored(Property::Opacity), "a keyframe-only property counts as written");
    assert!(styles[2].declared(Property::Color), "a keyframe's property is the element's own");
    assert!(!styles[1].declared(Property::Color));
    assert!(styles[1].animations().is_empty());
    // An unknown name animates nothing; a bad offset is a notice.
    let sheet = Sheet::parse("knob { animation: missing 1s; } @keyframes x { 200% { color: red; } }").unwrap();
    assert_eq!(sheet.notices.len(), 1);
}

#[test]
fn registered_properties_give_their_initial_value_and_a_variant_wins_over_the_theme() {
    let theme = Sheet::parse(
        "@property --glow { syntax: \"<number>\"; initial-value: 0.6; label: \"Glow\"; }\n\
         @property --accent { syntax: \"<color>\"; initial-value: #112233; }\n\
         :root { --accent: #445566; }\n\
         @media (width < 10px) { :root { --accent: #000000; } }",
    )
    .unwrap();
    assert_eq!(theme.properties.len(), 2);
    let variant = Sheet::parse(":root { --accent: #abcdef; --glow: bright; }").unwrap();
    let tree = Tree::new(vec![Element::new("html", None), Element::new("body", Some(0))]).unwrap();
    let viewport = Viewport { width: 800.0, height: 600.0, reduced_motion: false, light: false };
    let alone = compute(&tree, viewport, &[AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None }]).unwrap();
    assert_eq!(alone[1].variable("--glow"), Some("0.6"));
    assert_eq!(alone[1].variable("--accent"), Some("#445566"));
    let both = compute(
        &tree,
        viewport,
        &[
            AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None },
            AppliedSheet { sheet: &variant, layer: Layer::Variant, root: None },
        ],
    )
    .unwrap();
    assert_eq!(both[1].variable("--accent"), Some("#abcdef"));
    // A value that does not match the registered syntax gives the initial value.
    assert_eq!(both[1].variable("--glow"), Some("0.6"));
}

#[test]
fn host_properties_and_bad_property_rules_do_not_break_a_sheet() {
    let sheet = Sheet::parse(
        ".knob { image: url(\"assets/knob.svg\"); color: red; }\n\
         .window { background-opacity: 70; }\n\
         @property --x { syntax: \"<color>\"; }\n\
         @property --y { syntax: \"<integer>\"; initial-value: 4; range: 1 8; }",
    )
    .unwrap();
    assert_eq!(sheet.notices.len(), 1, "{:?}", sheet.notices);
    assert!(sheet.notices[0].message.contains("initial-value"));
    assert_eq!(sheet.properties.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["--y"]);
}

#[test]
fn attr_colours_a_track_and_its_children_through_a_variable() {
    let sheet = Sheet::parse(
        "track-header { --track: attr(color type(<color>)); \
           background-color: color-mix(in srgb, attr(color type(<color>)), black 50%); \
           color: contrast-color(attr(color type(<color>))); } \
         track-name { border-color: var(--track); } \
         clip { border-color: attr(color type(<color>), #00ff00); } \
         sample-point[kind='kick'] { color: red; }",
    )
    .unwrap();
    assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
    let tree = Tree::new(vec![
        Element::new("window", None),
        Element::new("track-header", Some(0)).with_attribute("color", "#ffff00"),
        Element::new("track-name", Some(1)),
        Element::new("track-header", Some(0)).with_attribute("color", "not a colour"),
        Element::new("track-name", Some(3)),
        Element::new("clip", Some(0)),
        Element::new("sample-point", Some(0)).with_attribute("kind", "kick"),
    ])
    .unwrap();
    let styles =
        compute(&tree, viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }]).unwrap();
    let rgba = |rgba| Value::Color(Color::Rgba(rgba));
    assert_eq!(styles[1].get(Property::BackgroundColor), rgba([128, 128, 0, 255]));
    assert_eq!(styles[1].get(Property::Color), rgba([0, 0, 0, 255]));
    // A child reads its parent's attribute only through the variable the parent set.
    assert_eq!(styles[2].get(Property::BorderColor), rgba([255, 255, 0, 255]));
    assert_eq!(styles[2].variable("--track"), Some("#ffff00"));
    // An attribute that is not a colour leaves the declarations that read it invalid.
    assert_eq!(styles[3].get(Property::BackgroundColor), Property::BackgroundColor.initial());
    assert_eq!(styles[4].variable("--track"), None);
    assert_eq!(styles[5].get(Property::BorderColor), rgba([0, 255, 0, 255]));
    assert_eq!(styles[6].get(Property::Color), rgba([255, 0, 0, 255]));
    let checked =
        compute_checked(&tree, viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }]);
    assert!(checked.is_err());
}

#[test]
fn a_registered_variable_dropped_by_substitution_takes_its_initial_value() {
    let theme = Sheet::parse(
        "@property --accent { syntax: '<color>'; initial-value: #0000ff; } \
         @property --loop { syntax: '<color>'; initial-value: #00ff00; } \
         @property --bare { syntax: '*'; }",
    )
    .unwrap();
    assert!(theme.notices.is_empty(), "{:?}", theme.notices);
    let user = Sheet::parse(
        ":root { --accent: var(--missing); --loop: var(--other); --other: var(--loop); \
         --bare: var(--missing); --reads: var(--accent); }",
    )
    .unwrap();
    let sheets = [
        AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None },
        AppliedSheet { sheet: &user, layer: Layer::User, root: None },
    ];
    let variables = root_variables(viewport(800.0), &sheets).unwrap();
    assert_eq!(variables.get("--accent").map(String::as_str), Some("#0000ff"));
    assert_eq!(variables.get("--loop").map(String::as_str), Some("#00ff00"));
    // A variable that reads the dropped one sees its initial value.
    assert_eq!(variables.get("--reads").map(String::as_str), Some("#0000ff"));
    // With no initial value the registered variable is left out, as an unregistered one is.
    assert!(!variables.contains_key("--bare"));
    assert_eq!(resolve_color("var(--accent)", viewport(800.0), &sheets), Ok([0, 0, 255, 255]));
}

#[test]
fn a_swatch_colour_resolves_for_each_variant_without_applying_the_sheet() {
    let theme = Sheet::parse(
        "@property --accent { syntax: '<color>'; initial-value: #0000ff; } \
         :root { --background: #000000; --ink: currentColor; --tag: attr(color type(<color>), red); } \
         window { --background: #ffffff; } \
         @media (max-width: 500px) { :root { --background: #222222; } }",
    )
    .unwrap();
    assert!(theme.notices.is_empty(), "{:?}", theme.notices);
    let light = Sheet::parse(":root { --background: #ffffff; --accent: #ff0000; }").unwrap();
    let wrong = Sheet::parse(":root { --accent: 12px; }").unwrap();
    let swatch = "color-mix(in srgb, var(--background), var(--accent) 50%)";
    let with = |variant: Option<&Sheet>, width: f32, value: &str| {
        let mut sheets = vec![AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None }];
        sheets.extend(variant.map(|sheet| AppliedSheet { sheet, layer: Layer::Variant, root: None }));
        resolve_color(value, viewport(width), &sheets)
    };
    assert_eq!(with(None, 800.0, swatch), Ok([0, 0, 128, 255]));
    assert_eq!(with(None, 400.0, swatch), Ok([17, 17, 145, 255]));
    assert_eq!(with(Some(&light), 800.0, swatch), Ok([255, 128, 128, 255]));
    // A variant's value that does not match the registered syntax falls back to the initial value.
    assert_eq!(with(Some(&wrong), 800.0, "var(--accent)"), Ok([0, 0, 255, 255]));
    assert_eq!(with(None, 800.0, "contrast-color(var(--background))"), Ok([255, 255, 255, 255]));
    for refused in ["currentColor", "var(--ink)", "attr(color type(<color>))", "var(--tag)", "var(--missing)", "12px"] {
        assert!(with(None, 800.0, refused).is_err(), "{refused}");
    }
    let variables =
        root_variables(viewport(800.0), &[AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None }]).unwrap();
    assert_eq!(variables.get("--background").map(String::as_str), Some("#000000"));
    assert!(!variables.contains_key("--tag"));
}

#[test]
fn outline_shorthand_sets_width_style_and_colour_in_any_order() {
    let style = |css: &str| {
        let sheet = Sheet::parse(css).unwrap_or_else(|e| panic!("{e}"));
        let styles =
            compute(&tree(), viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }])
                .unwrap_or_else(|e| panic!("{e}"));
        (sheet.notices.len(), styles[2].clone())
    };
    let (notices, knob) = style("knob { outline: solid #ff0000 2px; outline-offset: -1px; }");
    assert_eq!(notices, 0);
    assert_eq!(knob.get(Property::OutlineWidth), Value::Length(crate::value::Length::Px(2.0)));
    assert_eq!(knob.get(Property::OutlineStyle), Value::Keyword("solid"));
    assert_eq!(knob.get(Property::OutlineColor), Value::Color(Color::Rgba([255, 0, 0, 255])));
    assert_eq!(knob.get(Property::OutlineOffset), Value::Length(crate::value::Length::Px(-1.0)));
    // As in CSS, what the shorthand omits is reset: no style draws no outline.
    let (_, knob) = style("knob { outline-style: solid; outline: 1px red; }");
    assert_eq!(knob.get(Property::OutlineStyle), Value::Keyword("none"));
    assert_eq!(knob.get(Property::OutlineColor), Value::Color(Color::Rgba([255, 0, 0, 255])));
    let (_, knob) = style("knob { outline: 2px solid; }");
    assert_eq!(knob.get(Property::OutlineColor), knob.get(Property::Color), "the foreground");
    for bad in ["2px 3px", "dotted red", "url(a.png)"] {
        assert_eq!(style(&format!("knob {{ outline: {bad}; }}")).0, 1, "{bad}");
    }
}

#[test]
fn overflow_and_the_host_settings_are_properties_with_their_own_syntax() {
    let style = |css: &str| {
        let sheet = Sheet::parse(css).unwrap_or_else(|e| panic!("{e}"));
        let styles =
            compute(&tree(), viewport(800.0), &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }])
                .unwrap_or_else(|e| panic!("{e}"));
        (sheet.notices.len(), styles[2].clone())
    };
    let (_, knob) = style("knob { overflow: hidden; value-unit: \" dB\"; liquid: 48 24 12; }");
    assert_eq!(knob.get(Property::Overflow), Value::Keyword("hidden"));
    assert_eq!(knob.get(Property::ValueUnit), Value::Text("\" dB\"".into()));
    assert_eq!(knob.get(Property::Liquid), Value::Text("48 24 12".into()));
    assert!(knob.authored(Property::ValueUnit));
    // A value the setting's syntax refuses is skipped with a notice, as an invalid declaration is.
    for bad in [
        "value-unit: dB",
        "value-unit: \"far too long a unit\"",
        "liquid: 48 24",
        "liquid: 200 0 0",
        "backdrop-image: url(\"../up.png\")",
        "backdrop-image: url(\"/etc/up.png\")",
        "appearance: dial",
        "overflow: scroll",
    ] {
        assert_eq!(style(&format!("knob {{ {bad}; }}")).0, 1, "{bad}");
    }
    // The settings of a value inherit, as the registry says.
    let (_, knob) = style("window { appearance: knob; value-position: top; }");
    assert_eq!(knob.get(Property::Appearance), Value::Keyword("knob"));
    assert_eq!(knob.get(Property::ValuePosition), Value::Keyword("top"));
    let (_, knob) = style("window { meter-style: needle; overflow: hidden; }");
    assert_eq!(knob.get(Property::MeterStyle), Value::Keyword(""), "a meter's style is its own");
    assert_eq!(knob.get(Property::Overflow), Value::Keyword("visible"));
}

#[test]
fn a_user_agent_rule_on_a_child_is_not_authored_though_its_parent_is() {
    let tree = Tree::new(vec![
        Element::new("window", None),
        Element::new("knob", Some(0)),
        Element::new("knob-body", Some(1)),
        Element::new("knob-pointer", Some(1)),
    ])
    .unwrap();
    let agent = Sheet::parse("knob-body { color: #202020; }").unwrap();
    let theme = Sheet::parse("knob { color: #ffb000; }").unwrap();
    let styles = compute(
        &tree,
        viewport(800.0),
        &[
            AppliedSheet { sheet: &agent, layer: Layer::UserAgent, root: None },
            AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None },
        ],
    )
    .unwrap();
    // The body keeps the user agent's colour, which no sheet wrote; the pointer inherits the knob's.
    assert_eq!(styles[2].get(Property::Color), Value::Color(Color::Rgba([32, 32, 32, 255])));
    assert!(!styles[2].authored(Property::Color));
    assert_eq!(styles[3].get(Property::Color), Value::Color(Color::Rgba([255, 176, 0, 255])));
    assert!(styles[3].authored(Property::Color));
}

/// The base tier (#1448): every theme rule beats every base rule, a less specific one and one
/// inside `@layer` too, a base rule beats the user agent's, and a base rule alone leaves the
/// element unauthored, as the user agent's does.
#[test]
fn a_theme_rule_beats_a_base_rule_whatever_its_specificity_or_layer() {
    let tree = Tree::new(vec![
        Element::new("window", None),
        Element::new("button", Some(0)).with_attribute("checked", ""),
        Element::new("progress", Some(0)),
        Element::new("knob", Some(0)),
    ])
    .unwrap();
    let agent = Sheet::parse("progress { background-color: #010101; color: #010101; }").unwrap();
    let base = Sheet::parse(
        "window button[checked] { background-color: #020202; } progress { background-color: #030303; } \
         knob { color: #040404; }",
    )
    .unwrap();
    let theme = Sheet::parse("@layer looks { button { background-color: #ffb000; } }").unwrap();
    let styles = compute(
        &tree,
        viewport(800.0),
        &[
            AppliedSheet { sheet: &agent, layer: Layer::UserAgent, root: None },
            AppliedSheet { sheet: &base, layer: Layer::Base, root: None },
            AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None },
        ],
    )
    .unwrap();
    // The theme's plain `button`, in a layer, beats the base's more specific state rule.
    assert_eq!(styles[1].get(Property::BackgroundColor), Value::Color(Color::Rgba([255, 176, 0, 255])));
    assert!(styles[1].authored(Property::BackgroundColor));
    // A base rule beats the user agent's, and is not authored: no theme wrote it.
    assert_eq!(styles[2].get(Property::BackgroundColor), Value::Color(Color::Rgba([3, 3, 3, 255])));
    assert!(!styles[2].authored(Property::BackgroundColor) && !styles[2].declared(Property::BackgroundColor));
    assert_eq!(styles[2].get(Property::Color), Value::Color(Color::Rgba([1, 1, 1, 255])));
    assert!(!styles[3].authored(Property::Color) && !styles[3].declared(Property::Color));
}

/// The cascade goes property by property (#1448): a theme rule for an element overrides a base
/// state rule only in the properties it sets; another property keeps the base value.
#[test]
fn a_theme_rule_overrides_a_base_rule_only_in_the_properties_it_sets() {
    let tree =
        Tree::new(vec![Element::new("window", None), Element::new("button", Some(0)).with_attribute("checked", "")])
            .unwrap();
    let base = Sheet::parse("button[checked] { background-color: #020202; color: #050505; }").unwrap();
    let theme = Sheet::parse("button { background-color: #ffb000; }").unwrap();
    let styles = compute(
        &tree,
        viewport(800.0),
        &[
            AppliedSheet { sheet: &base, layer: Layer::Base, root: None },
            AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None },
        ],
    )
    .unwrap();
    assert_eq!(styles[1].get(Property::BackgroundColor), Value::Color(Color::Rgba([255, 176, 0, 255])));
    assert_eq!(styles[1].get(Property::Color), Value::Color(Color::Rgba([5, 5, 5, 255])));
    assert!(!styles[1].authored(Property::Color));
}

/// A base rule on an element replaces the colour a theme's rule around it gives, and what it
/// gives is not authored, as a user-agent rule's is not; a theme rule on the element itself beats
/// it.
#[test]
fn a_base_rule_on_a_child_is_not_authored_though_its_parent_is() {
    let tree = Tree::new(vec![
        Element::new("window", None),
        Element::new("mixer", Some(0)),
        Element::new("drop-target", Some(1)),
        Element::new("drop-target", Some(1)).with_attribute("kind", "insert"),
    ])
    .unwrap();
    let base = Sheet::parse(":root { --accent: #a6a3ff; } drop-target { color: var(--accent); }").unwrap();
    let theme =
        Sheet::parse("mixer { color: #ffb000; --accent: #00ff00; } drop-target[kind] { color: #ff0000; }").unwrap();
    let styles = compute(
        &tree,
        viewport(800.0),
        &[
            AppliedSheet { sheet: &base, layer: Layer::Base, root: None },
            AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None },
        ],
    )
    .unwrap();
    // The base rule reads the variable the mixer's rule sets: a region is coloured through it.
    assert_eq!(styles[2].get(Property::Color), Value::Color(Color::Rgba([0, 255, 0, 255])));
    assert!(!styles[2].authored(Property::Color));
    assert!(styles[1].authored(Property::Color));
    assert_eq!(styles[3].get(Property::Color), Value::Color(Color::Rgba([255, 0, 0, 255])));
    assert!(styles[3].declared(Property::Color));
}

#[test]
fn a_property_a_rule_wrote_on_the_element_is_declared_not_inherited() {
    let tree =
        Tree::new(vec![Element::new("window", None), Element::new("knob", Some(0)), Element::new("knob", Some(0))])
            .unwrap();
    let theme = Sheet::parse("window, knob:last-child { color: #ffb000; }").unwrap();
    let styles =
        compute(&tree, viewport(800.0), &[AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None }]).unwrap();
    // The first knob inherits the window's colour; the second writes the same one itself.
    assert_eq!(styles[1].get(Property::Color), styles[2].get(Property::Color));
    assert!(styles[1].authored(Property::Color) && !styles[1].declared(Property::Color));
    assert!(styles[2].declared(Property::Color));
    assert!(styles[0].declared(Property::Color));
}

// The shared cascade (#1489).

/// A window with a toolbar of three buttons and a list of rows, each row with a label.
fn bar_and_list() -> Vec<Element> {
    vec![
        Element::new("window", None),
        Element::new("toolbar", Some(0)),
        Element::new("button", Some(1)),
        Element::new("button", Some(1)),
        Element::new("button", Some(1)),
        Element::new("list", Some(0)),
        Element::new("row", Some(5)),
        Element::new("label", Some(6)),
        Element::new("row", Some(5)),
        Element::new("label", Some(8)),
    ]
}

fn hovered(mut element: Element) -> Element {
    element.states.push("hover".into());
    element
}

/// The styles of `elements` under `css` worked out whole and shared, which must agree, and how
/// many the shared cascade worked out after `cache` saw the trees before.
fn both(css: &str, elements: Vec<Element>, cache: &mut StyleCache) -> (Vec<ComputedStyle>, usize) {
    let sheet = Sheet::parse(css).unwrap();
    assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
    let applied = [AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }];
    let tree = Tree::new(elements).unwrap();
    let whole = compute(&tree, viewport(1138.0), &applied).unwrap();
    let shared = compute_shared(&tree, viewport(1138.0), &applied, 1, cache).unwrap();
    assert_eq!(whole, shared.iter().map(|(_, style)| style.as_ref().clone()).collect::<Vec<_>>());
    (whole, cache.computed())
}

const VARIABLES: &str = "window { --ink: #102030; color: var(--ink); } \
     toolbar { --ink: color-mix(in oklab, #ffffff, #000000 20%); } \
     button { --face: color-mix(in oklab, var(--ink), #ff0000 10%); background-color: var(--face); } \
     button:hover { --face: #00ff00; } \
     row label { color: var(--ink); } row:hover label { color: #ff00ff; }";

#[test]
fn the_shared_cascade_works_out_identical_elements_once() {
    let mut cache = StyleCache::default();
    let (_, computed) = both(VARIABLES, bar_and_list(), &mut cache);
    // window, toolbar, one button for three, list, one row and one label for two.
    assert_eq!(computed, 6);
    let (_, again) = both(VARIABLES, bar_and_list(), &mut cache);
    assert_eq!(again, 0, "an unchanged tree takes every style from the cache");
}

#[test]
fn a_changed_element_works_out_only_itself_and_what_is_under_it() {
    let mut cache = StyleCache::default();
    both(VARIABLES, bar_and_list(), &mut cache);
    let mut elements = bar_and_list();
    elements[3] = hovered(Element::new("button", Some(1)));
    let (styles, computed) = both(VARIABLES, elements, &mut cache);
    assert_eq!(computed, 1, "the hovered button alone");
    assert_eq!(styles[3].get(Property::BackgroundColor), Value::Color(Color::Rgba([0, 255, 0, 255])));
    assert_ne!(styles[2].get(Property::BackgroundColor), styles[3].get(Property::BackgroundColor));
    let mut elements = bar_and_list();
    elements[8] = hovered(Element::new("row", Some(5)));
    let (styles, computed) = both(VARIABLES, elements, &mut cache);
    assert_eq!(computed, 2, "the hovered row and its label");
    assert_eq!(styles[9].get(Property::Color), Value::Color(Color::Rgba([255, 0, 255, 255])));
    assert_ne!(styles[7].get(Property::Color), styles[9].get(Property::Color));
}

#[test]
fn a_sibling_selector_keeps_later_siblings_apart() {
    let css = "button { color: #000000; } button:hover + button { color: #ff0000; } \
               button:first-child { radius: 2px; } button:last-child { radius: 6px; }";
    let mut cache = StyleCache::default();
    let (styles, _) = both(css, bar_and_list(), &mut cache);
    assert_eq!(styles[2].get(Property::Radius), Value::Length(Length::Px(2.0)));
    assert_eq!(styles[4].get(Property::Radius), Value::Length(Length::Px(6.0)));
    let mut elements = bar_and_list();
    elements[2] = hovered(Element::new("button", Some(1)));
    let (styles, computed) = both(css, elements, &mut cache);
    assert_eq!(styles[3].get(Property::Color), Value::Color(Color::Rgba([255, 0, 0, 255])));
    assert_eq!(styles[4].get(Property::Color), Value::Color(Color::Rgba([0, 0, 0, 255])));
    assert_eq!(computed, 3, "the hovered button and the two after it, whose earlier siblings changed");
}

#[test]
fn empty_is_told_apart_by_whether_an_element_has_children() {
    let css = "row { color: #000000; } row:empty { color: #ff0000; }";
    let mut cache = StyleCache::default();
    let mut elements = bar_and_list();
    elements.truncate(9);
    let (styles, _) = both(css, elements, &mut cache);
    assert_eq!(styles[6].get(Property::Color), Value::Color(Color::Rgba([0, 0, 0, 255])));
    assert_eq!(
        styles[8].get(Property::Color),
        Value::Color(Color::Rgba([255, 0, 0, 255])),
        "the row without its label"
    );
}

#[test]
fn new_sheets_or_another_viewport_empty_the_cache() {
    let sheet = Sheet::parse(VARIABLES).unwrap();
    let applied = [AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }];
    let tree = Tree::new(bar_and_list()).unwrap();
    let mut cache = StyleCache::default();
    compute_shared(&tree, viewport(1138.0), &applied, 1, &mut cache).unwrap();
    compute_shared(&tree, viewport(1138.0), &applied, 2, &mut cache).unwrap();
    assert_eq!(cache.computed(), 6, "another sheets key");
    compute_shared(&tree, viewport(800.0), &applied, 2, &mut cache).unwrap();
    assert_eq!(cache.computed(), 6, "another viewport");
    cache.clear();
    assert!(cache.is_empty());
}

#[test]
fn a_scoped_sheet_reaches_only_the_elements_under_its_root() {
    let app = Sheet::parse("knob { color: #000000; }").unwrap();
    let device = Sheet::parse("knob { color: #ff0000; }").unwrap();
    let applied = [
        AppliedSheet { sheet: &app, layer: Layer::Theme, root: None },
        AppliedSheet { sheet: &device, layer: Layer::Device, root: Some(1) },
    ];
    let mut cache = StyleCache::default();
    let styles = compute_shared(&tree(), viewport(1138.0), &applied, 1, &mut cache).unwrap();
    assert_eq!(styles[2].1.get(Property::Color), Value::Color(Color::Rgba([255, 0, 0, 255])));
    assert_eq!(styles[4].1.get(Property::Color), Value::Color(Color::Rgba([0, 0, 0, 255])));
    assert_ne!(styles[2].0, styles[4].0, "the same knob under another panel is another key");
}

#[test]
fn a_full_cache_starts_again_and_every_number_stays_its_place() {
    let css = "row { color: attr(color type(<color>)); } label { color: inherit; }";
    let mut cache = StyleCache::default();
    let mut numbers = std::collections::HashMap::new();
    for round in 0..12u32 {
        let mut elements = vec![Element::new("window", None)];
        for row in 0..400u32 {
            let parent = elements.len();
            let shade = (round * 400 + row) % 0xff_ffff;
            elements.push(Element::new("row", Some(0)).with_attribute("color", &format!("#{shade:06x}")));
            elements.push(Element::new("label", Some(parent)));
        }
        let sheet = Sheet::parse(css).unwrap();
        let applied = [AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }];
        let tree = Tree::new(elements).unwrap();
        let whole = compute(&tree, viewport(1138.0), &applied).unwrap();
        let shared = compute_shared(&tree, viewport(1138.0), &applied, 1, &mut cache).unwrap();
        assert_eq!(whole, shared.iter().map(|(_, style)| style.as_ref().clone()).collect::<Vec<_>>(), "round {round}");
        assert!(cache.len() <= MAX_CACHED_STYLES);
        for (number, style) in &shared {
            numbers.insert(*number, Arc::clone(style));
        }
        for (number, style) in &shared {
            assert!(Arc::ptr_eq(&numbers[number], style), "one number, one style within a call");
        }
    }
}

#[test]
fn many_siblings_take_their_places_in_one_pass() {
    // Two thousand identical rows: without a sheet that tests siblings their places are one, and
    // with one each row's place among them still comes out right.
    let mut elements = vec![Element::new("window", None), Element::new("list", Some(0))];
    elements.extend((0..2000).map(|_| Element::new("row", Some(1))));
    let mut cache = StyleCache::default();
    let (_, computed) = both("row { color: #000000; }", elements.clone(), &mut cache);
    assert_eq!(computed, 3, "window, list and one row for all");
    let css = "row { color: #000000; } row:nth-child(2n) { color: #ff0000; } row + row { radius: 2px; }";
    let mut cache = StyleCache::default();
    let (styles, _) = both(css, elements, &mut cache);
    assert_eq!(styles[2].get(Property::Color), Value::Color(Color::Rgba([0, 0, 0, 255])));
    assert_eq!(styles[3].get(Property::Color), Value::Color(Color::Rgba([255, 0, 0, 255])));
    assert_eq!(styles[3].get(Property::Radius), Value::Length(Length::Px(2.0)), "a row after a row");
    assert_ne!(styles[2].get(Property::Radius), styles[3].get(Property::Radius), "the first row follows none");
}

#[test]
fn an_id_no_selector_tests_does_not_keep_elements_apart() {
    // The host gives elements without an id of their own one (`node-…`): rows that differ only
    // by such an id share one style, and an id a rule tests keeps its element apart.
    let rows = |ids: [&str; 3]| {
        let mut elements = vec![Element::new("window", None), Element::new("list", Some(0))];
        elements.extend(ids.iter().map(|id| Element::new("row", Some(1)).with_id(id)));
        elements
    };
    let mut cache = StyleCache::default();
    let (_, computed) = both("row { color: #000000; }", rows(["node-1", "node-2", "node-3"]), &mut cache);
    assert_eq!(computed, 3, "window, list and one row");
    let mut cache = StyleCache::default();
    let (styles, computed) =
        both("row { color: #000000; } #node-2 { color: #ff0000; }", rows(["node-1", "node-2", "node-3"]), &mut cache);
    assert_eq!(computed, 4, "the row whose id a rule tests is apart");
    assert_eq!(styles[3].get(Property::Color), Value::Color(Color::Rgba([255, 0, 0, 255])));
}

/// The declarations [`explain`] lists for `name`, with each one's rule line and outcome.
fn outcomes(rules: &[MatchedRule], name: &str) -> Vec<(u32, Outcome)> {
    rules
        .iter()
        .flat_map(|rule| rule.declarations.iter().filter(|d| d.name == name).map(|d| (rule.line, d.outcome.clone())))
        .collect()
}

#[test]
fn explain_lists_the_matching_rules_in_cascade_order_and_marks_each_winner() {
    let base = Sheet::parse("knob { color: #111111; radius: 2px; }").unwrap();
    let theme = Sheet::parse(
        "knob#cutoff {\n  color: #222222;\n}\nknob { color: #333333 !important; }\n@media (max-width: 10px) { knob { radius: 9px; } }\nslider { color: red; }",
    )
    .unwrap();
    let sheets = [
        AppliedSheet { sheet: &base, layer: Layer::Base, root: None },
        AppliedSheet { sheet: &theme, layer: Layer::Theme, root: None },
    ];
    let (style, rules) = explain(&tree(), 2, viewport(1138.0), &sheets).unwrap();
    // The media rule for a narrow window and the slider's rule do not match.
    let matched: Vec<_> =
        rules.iter().map(|rule| (rule.sheet, rule.layer, rule.line, rule.selector.as_str())).collect();
    assert_eq!(
        matched,
        [(0, Layer::Base, 1, "knob"), (1, Layer::Theme, 1, "knob#cutoff"), (1, Layer::Theme, 4, "knob")]
    );
    // `!important` beats the more specific rule of the same tier, and both beat the base.
    assert_eq!(
        outcomes(&rules, "color"),
        [(1, Outcome::Overridden), (1, Outcome::Overridden), (4, Outcome::Applied("#333333".into()))]
    );
    assert_eq!(outcomes(&rules, "radius"), [(1, Outcome::Applied("2px".into()))]);
    assert_eq!(style, compute(&tree(), viewport(1138.0), &sheets).unwrap()[2]);
}

#[test]
fn explain_names_a_winning_declaration_whose_value_did_not_hold() {
    // An undefined variable drops the winner: the element keeps its inherited colour, and the
    // weaker rule is not used.
    let sheet = Sheet::parse(
        "window { color: #eeeeee; --ink: #123456; }\nknob { color: #010101; }\nknob#cutoff { color: var(--missing); padding: 3px var(--ink); }",
    )
    .unwrap();
    let sheets = [AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }];
    let (style, rules) = explain(&tree(), 2, viewport(1138.0), &sheets).unwrap();
    assert_eq!(style.get(Property::Color), Value::Color(Color::Rgba([238, 238, 238, 255])));
    let color = outcomes(&rules, "color");
    assert_eq!(color.first(), Some(&(2, Outcome::Overridden)));
    assert!(matches!(color.get(1), Some((3, Outcome::Dropped(_)))), "{color:?}");
    // A shorthand's parts are named as longhands, with the shorthand they came from.
    let padding: Vec<_> =
        rules.iter().flat_map(|rule| &rule.declarations).filter(|d| d.shorthand == Some("padding")).collect();
    // Each part holds or not on its own: the colour is no length, the vertical parts are fine.
    let parts: Vec<_> = padding.iter().map(|d| (d.name.as_str(), matches!(d.outcome, Outcome::Applied(_)))).collect();
    assert_eq!(
        parts,
        [("padding-top", true), ("padding-right", false), ("padding-bottom", true), ("padding-left", false)]
    );
    // A custom property set on an ancestor is the window's rule, not the knob's.
    let (_, window) = explain(&tree(), 0, viewport(1138.0), &sheets).unwrap();
    assert_eq!(outcomes(&window, "--ink"), [(1, Outcome::Applied("#123456".into()))]);
}

#[test]
fn recorded_matches_hold_every_rule_that_matched_any_element_styled() {
    let sheet = Sheet::parse("knob { color: red; }\nslider { color: blue; }\n#cutoff { radius: 1px; }").unwrap();
    // Another sheet in the same tier, whose rules are told apart from the first one's.
    let other = Sheet::parse("window { color: red; }\nknob { color: blue; }").unwrap();
    assert_ne!(sheet.id(), other.id());
    // A sheet parsed again from the same text is the same sheet to the record.
    let again = Sheet::parse("knob { color: red; }\nslider { color: blue; }\n#cutoff { radius: 1px; }").unwrap();
    assert_eq!(again.id(), sheet.id());
    let sheets = [
        AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None },
        AppliedSheet { sheet: &other, layer: Layer::Theme, root: None },
    ];
    assert!(matched_rules().is_empty());
    record_matches(true);
    compute(&tree(), viewport(1138.0), &sheets).unwrap();
    let matched = matched_rules();
    record_matches(false);
    assert_eq!(matched, BTreeSet::from([(sheet.id(), 0), (sheet.id(), 2), (other.id(), 0), (other.id(), 1)]));
    assert!(matched_rules().is_empty());
    let sources: Vec<_> = sheet.rule_sources().collect();
    assert_eq!(sources, [(1, "knob"), (2, "slider"), (3, "#cutoff")]);
}
