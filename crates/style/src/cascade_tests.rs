// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{selector::Element, value::Length};

fn viewport(width: f32) -> Viewport {
    Viewport { width, height: 700.0, dark: true, reduced_motion: false }
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
    let viewport = Viewport { width: 800.0, height: 600.0, dark: true, reduced_motion: false };
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
fn a_host_variable_is_inherited_and_a_rule_may_replace_it() {
    use crate::selector::{Element, Tree};
    let sheet =
        Sheet::parse(".name { color: var(--track-colour); } .plain .name { --track-colour: #000000; }").unwrap();
    let mut track = Element::new("div", None);
    track.classes.push("track".into());
    track.variables.push(("--track-colour".into(), "#ff8800".into()));
    let mut name = Element::new("div", Some(0));
    name.classes.push("name".into());
    let tree = Tree::new(vec![track, name]).unwrap();
    let styles = compute(
        &tree,
        Viewport { width: 800.0, height: 600.0, dark: true, reduced_motion: false },
        &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }],
    )
    .unwrap();
    assert_eq!(styles[1].get(Property::Color), Value::Color(crate::value::Color::Rgba([0xff, 0x88, 0, 0xff])));
}
