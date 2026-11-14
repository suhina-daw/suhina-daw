// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the Launcher, the mixer, the Arrangement, the browser and the detail panels draw is each an
//! element a theme styles on its own (#1417, #1023): a rule for one reaches what it paints, and nothing else takes its colour.

use super::*;

/// Every colour the window painted in its last frame: fills, strokes, lines and text.
fn painted(h: &Harness<'static, App>) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
            egui::Shape::Rect(rect) => found.extend([rect.fill, rect.stroke.color]),
            egui::Shape::LineSegment { stroke, .. } => found.push(stroke.color),
            egui::Shape::Path(path) => {
                found.push(path.fill);
                if let egui::epaint::PathStroke { color: egui::epaint::ColorMode::Solid(color), .. } = &path.stroke {
                    found.push(*color);
                }
            }
            egui::Shape::Circle(circle) => found.extend([circle.fill, circle.stroke.color]),
            egui::Shape::Text(text) => {
                found.push(text.fallback_color);
                found.extend(text.galley.job.sections.iter().map(|section| section.format.color));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, &mut found));
    found
}

/// A colour no theme or track uses, the `n`-th of a set.
fn odd(n: u8) -> egui::Color32 {
    egui::Color32::from_rgb(0x13, 0x57, n)
}

fn hex(color: egui::Color32) -> String {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Shows `h` with `rules` (selector, property, colour) as the user's sheet, then every rule's
/// colour that no shape painted.
fn missing(h: &mut Harness<'static, App>, rules: &[(&str, &str, egui::Color32)]) -> Vec<String> {
    let css: String = rules
        .iter()
        .map(|(selector, property, color)| format!("{selector} {{ {property}: {}; }}\n", hex(*color)))
        .collect();
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let shown = painted(h);
    suhina_ui::kit::native_css::set_user_sheet(None);
    rules
        .iter()
        .filter(|(_, _, color)| !shown.contains(color))
        .map(|(selector, property, _)| format!("{selector} {property}"))
        .collect()
}

#[test]
fn the_launcher_s_and_the_mixer_s_parts_take_their_own_rules() {
    let mut h = window(demo_app());
    let rules = [
        ("separator[kind=\"slot\"]", "color", odd(1)),
        ("separator[kind=\"column\"]", "color", odd(2)),
        ("separator[kind=\"band\"]", "color", odd(3)),
        ("scene", "background-color", odd(4)),
        ("label[kind=\"scene-number\"]", "color", odd(5)),
        ("slot[kind=\"stop\"]", "background-color", odd(6)),
        ("scene[kind=\"stop-all\"]", "background-color", odd(7)),
        ("track-header[kind=\"main\"]", "background-color", odd(8)),
        ("fader-scale", "color", odd(10)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

#[test]
fn the_arrangement_s_parts_take_their_own_rules() {
    let mut h = arrangement_app();
    let rules = [
        ("grid-line[kind=\"bar\"]", "color", odd(21)),
        ("separator[kind=\"row\"]", "color", odd(23)),
        ("separator[kind=\"band\"]", "color", odd(24)),
        ("separator[kind=\"lane\"]", "color", odd(25)),
        ("track-header[kind=\"main\"]", "background-color", odd(26)),
        ("start-marker", "color", odd(27)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// How many painted fills and strokes are `color`.
fn count(h: &Harness<'static, App>, color: egui::Color32) -> usize {
    painted(h).into_iter().filter(|shown| *shown == color).count()
}

/// A rule for every Arrangement tool does not hide the chosen one: only a `[checked]` rule gives
/// it a colour of its own (#1428 review).
#[test]
fn a_rule_for_every_arrangement_tool_keeps_the_chosen_one_lit() {
    let mut h = arrangement_app();
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.arrangement_toolbar = true;
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    // The toolbar's tools: select, split, erase, draw, mute and range.
    let tools = 6;
    let fills = |h: &mut Harness<'static, App>, css: &str| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        let shown = (count(h, odd(31)), count(h, odd(32)));
        suhina_ui::kit::native_css::set_user_sheet(None);
        shown
    };
    let general = format!("arrangement-tool {{ background-color: {}; }}", hex(odd(31)));
    assert_eq!(fills(&mut h, &general), (tools - 1, 0), "the chosen tool keeps its native fill");
    let chosen = format!("{general} arrangement-tool[checked] {{ background-color: {}; }}", hex(odd(32)));
    assert_eq!(fills(&mut h, &chosen), (tools - 1, 1), "a rule for `[checked]` recolours it");
}

/// Shows `h` with `css` as the user's sheet for a few frames, and how many painted fills, strokes,
/// lines and texts are `color`.
fn count_with(h: &mut Harness<'static, App>, css: &str, color: egui::Color32) -> usize {
    suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
    for _ in 0..4 {
        h.step();
    }
    let shown = count(h, color);
    suhina_ui::kit::native_css::set_user_sheet(None);
    shown
}

/// A clip picked by a click in its body is outlined from the same `clip` element its body is
/// drawn as, with its `[color]` and its states: a rule for `clip[color][selected]` reaches the
/// outline (#1428 review).
#[test]
fn a_clip_picked_in_its_body_takes_its_own_selected_rule() {
    let mut h = arrangement_app();
    let body = h.get_by_label("Clip Beat B on Drums at bar 17").rect().left_center() + egui::vec2(64.0, 0.0);
    h.hover_at(body);
    h.run();
    h.drag_at(body);
    h.drop_at(body);
    h.run_steps(2);
    assert!(h.state().arrangement().selected_clip().is_some(), "the clip is picked");
    assert_eq!(missing(&mut h, &[("clip[color][selected]", "border-color", odd(41))]), Vec::<String>::new());
}

/// A take's name is drawn in its header's text colour: `take-header { color }` reaches the name
/// itself, and the header's buttons, which sit on its background, inherit it as their glyphs'
/// colour where no rule of theirs sets one (#1428 review).
#[test]
fn a_take_s_name_takes_its_header_s_colour() {
    let mut h = super::arrangement::window_with_drums_takes(16.0);
    assert!(h.query_by_label("Solo Drums Take 1").is_some(), "the take lanes are shown");
    let css = format!("take-header {{ color: {}; }}", hex(odd(42)));
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    // The colours of the texts that read `text` inside a take's header (its clips carry names
    // of their own), each text once.
    let headers: Vec<egui::Rect> = [("Drums Take 1", "Solo Drums Take 1"), ("Drums Take 2", "Solo Drums Take 2")]
        .iter()
        .filter_map(|(name, solo)| {
            let solo = h.get_by_label(solo).rect();
            h.get_all_by_label(name).map(|node| node.rect()).find(|rect| rect.intersects(solo))
        })
        .collect();
    assert_eq!(headers.len(), 2, "each take's header is found by its name");
    let texts = |h: &Harness<'static, App>, text: &str| -> Vec<egui::Color32> {
        h.output()
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(shown)
                    if shown.galley.text() == text
                        && headers
                            .iter()
                            .any(|header| header.contains(shape.shape.visual_bounding_rect().center())) =>
                {
                    Some(shown.galley.job.sections.first().map_or(shown.fallback_color, |section| section.format.color))
                }
                _ => None,
            })
            .collect()
    };
    let names = [texts(&h, "Drums Take 1"), texts(&h, "Drums Take 2")];
    let solo = texts(&h, "S");
    suhina_ui::kit::native_css::set_user_sheet(None);
    for (n, name) in names.iter().enumerate() {
        assert_eq!(name, &[odd(42)], "take {}'s name is drawn once, in the header's colour", n + 1);
    }
    assert_eq!(solo.iter().filter(|ink| **ink == odd(42)).count(), 2, "each take's Solo inherits it");
}

/// The lit Edit gain envelope button paints the fill a `[checked]` rule gives it (#1428 review).
#[test]
fn the_lit_envelope_button_takes_its_checked_fill() {
    let mut h = window_with_step(audio_app(), 1.0 / 60.0);
    h.get_by_label("Arrangement view").click();
    h.run();
    h.get_by_label("Edit gain envelope of Loop").click();
    h.run();
    h.get_by_label("Free move").click();
    h.run();
    let rules = [
        ("button#control-clip-envelope-edit[checked]", "background-color", odd(43)),
        ("button#control-clip-envelope-free[checked]", "background-color", odd(44)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// A hover rule is not the chosen tool's colour: hovered, the chosen tool keeps its native fill
/// unless a `[checked]` rule gives it one (#1428 review).
#[test]
fn a_hover_rule_does_not_light_the_chosen_tool() {
    let mut h = arrangement_app();
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.arrangement_toolbar = true;
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    let chosen = h.get_by_label("Arrangement tool Select").rect().center();
    h.hover_at(chosen);
    h.run();
    let hover = format!("arrangement-tool:hover {{ background-color: {}; }}", hex(odd(45)));
    assert_eq!(count_with(&mut h, &hover, odd(45)), 0, "the hovered chosen tool keeps its native fill");
    let lit = format!("{hover} arrangement-tool[checked]:hover {{ background-color: {}; }}", hex(odd(46)));
    assert_eq!(count_with(&mut h, &lit, odd(46)), 1, "a rule for the hovered `[checked]` tool recolours it");
}

/// How many rectangles the window stroked with `color`, with the user's sheet `css`.
fn strokes_with(h: &mut Harness<'static, App>, css: &str, color: egui::Color32) -> usize {
    fn walk(shape: &egui::Shape, color: egui::Color32, found: &mut usize) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, color, found)),
            egui::Shape::Rect(rect) if rect.stroke.color == color && rect.stroke.width > 0.0 => *found += 1,
            _ => {}
        }
    }
    suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
    for _ in 0..4 {
        h.step();
    }
    let mut found = 0;
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, color, &mut found));
    suhina_ui::kit::native_css::set_user_sheet(None);
    found
}

/// The outline of a clip picked in its body is looked up without taking a place in the style
/// tree: the body, drawn under the same key without `[selected]`, keeps its own background with a
/// transition on (#1428 review).
#[test]
fn a_clip_picked_in_its_body_keeps_its_own_background_with_a_transition() {
    let mut h = arrangement_app();
    let body = h.get_by_label("Clip Beat B on Drums at bar 17").rect().left_center() + egui::vec2(64.0, 0.0);
    h.hover_at(body);
    h.run();
    h.drag_at(body);
    h.drop_at(body);
    h.run_steps(2);
    assert!(h.state().arrangement().selected_clip().is_some(), "the clip is picked");
    let css = format!(
        "clip {{ transition: background-color 10s linear; }} clip[selected] {{ background-color: {}; }}",
        hex(odd(51))
    );
    assert_eq!(count_with(&mut h, &css, odd(51)), 0, "the body is not drawn as selected");
}

/// A hovered selected slot clip is compared with the same clip hovered: a `clip:hover` rule is not
/// taken for its selection's outline (#1428 review).
#[test]
fn a_hover_rule_is_not_a_slot_clip_s_selected_outline() {
    let mut h = window(demo_app());
    h.get_by_label("Select Walk on Bass (scene 2)").click();
    h.run();
    let at = h.get_by_label("Select Walk on Bass (scene 2)").rect().center();
    h.hover_at(at);
    h.run();
    let hover = format!("clip:hover {{ border-color: {}; }}", hex(odd(52)));
    assert_eq!(strokes_with(&mut h, &hover, odd(52)), 0, "the outline keeps its native colour");
    let selected = format!("{hover} clip[selected]:hover {{ border-color: {}; }}", hex(odd(53)));
    assert_eq!(strokes_with(&mut h, &selected, odd(53)), 1, "a `[selected]` rule recolours it");
}

/// A slot clip's native state colours are read before the clip is entered: a rule for every clip's
/// `color` is not a selected clip's outline, and its `background-color` not a deactivated clip's
/// grey (#1428 review).
#[test]
fn a_rule_for_every_slot_clip_keeps_the_native_state_colours() {
    let mut project = demo_project();
    if let Some(clip) = project.launcher.columns[1].slots[1].clip.as_mut() {
        clip.enabled = false;
    }
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut h = window(app);
    h.get_by_label("Select Walk on Bass (scene 2)").click();
    h.run();
    h.hover_at(egui::pos2(1.0, 1.0));
    h.run();
    let ink = format!("clip {{ color: {}; }}", hex(odd(54)));
    assert_eq!(strokes_with(&mut h, &ink, odd(54)), 0, "the outline is not the clips' colour");
    let fill = format!("clip {{ background-color: {}; }}", hex(odd(55)));
    assert_eq!(count_with(&mut h, &fill, odd(55)), 0, "the deactivated clip is not the clips' fill");
}

/// The chosen tool's icon keeps its native ink under a `color` rule for every tool (#1428 review).
#[test]
fn a_colour_rule_for_every_tool_keeps_the_chosen_one_s_ink() {
    let mut h = arrangement_app();
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.arrangement_toolbar = true;
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    let tools = 6;
    let css = format!("arrangement-tool {{ color: {}; }}", hex(odd(56)));
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let range = h.get_by_label("Arrangement tool Select").rect();
    // The icons painted in the rule's colour, by the tool button they are in.
    let mut inked = std::collections::BTreeSet::new();
    let left = range.left();
    let pitch = h.get_by_label("Arrangement tool Range").rect().left() - left;
    fn walk(shape: &egui::Shape, color: egui::Color32, found: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, color, found)),
            other => {
                let colours = {
                    let mut c = Vec::new();
                    match other {
                        egui::Shape::LineSegment { stroke, .. } => c.push(stroke.color),
                        egui::Shape::Path(path) => {
                            c.push(path.fill);
                            if let egui::epaint::PathStroke { color: egui::epaint::ColorMode::Solid(s), .. } =
                                &path.stroke
                            {
                                c.push(*s);
                            }
                        }
                        egui::Shape::Rect(rect) => c.extend([rect.fill, rect.stroke.color]),
                        egui::Shape::Circle(circle) => c.extend([circle.fill, circle.stroke.color]),
                        _ => {}
                    }
                    c
                };
                if colours.contains(&color) {
                    found.push(other.visual_bounding_rect());
                }
            }
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, odd(56), &mut found));
    suhina_ui::kit::native_css::set_user_sheet(None);
    for rect in found.iter().filter(|r| r.center().y >= range.top() && r.center().y <= range.bottom()) {
        inked.insert(((rect.center().x - left) / pitch).floor() as i32);
    }
    assert_eq!(inked.len(), tools - 1, "every tool but the chosen one takes the colour: {inked:?}");
    assert!(!inked.contains(&0), "the chosen tool (Select) keeps its native ink");
}

/// The lit monitor button's label keeps its native ink under a `color` rule for the plain button
/// (#1428 review).
#[test]
fn the_lit_monitor_button_keeps_its_label_ink() {
    let mut project = demo_project();
    project.tracks[0].content = suhina_engine::model::ContentKinds { notes: false, audio: true };
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.mixer_height = 320;
    app.set_settings(&egui::Context::default(), settings);
    let mut h = window(app);
    let css: String = ["in", "auto", "off"]
        .iter()
        .map(|value| format!("button#control-mixer-monitor-{value} {{ color: {}; }}\n", hex(odd(57))))
        .collect();
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let mut inked = Vec::new();
    for shape in h.output().shapes.iter() {
        if let egui::Shape::Text(text) = &shape.shape
            && ["In", "Auto", "Off"].contains(&text.galley.text())
            && (text.fallback_color == odd(57) || text.galley.job.sections.iter().any(|s| s.format.color == odd(57)))
        {
            inked.push(text.galley.text().to_owned());
        }
    }
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert_eq!(inked.len(), 2, "the two off buttons take the colour, the lit one does not: {inked:?}");
}

/// The fills of the rectangles painted exactly over `rect` in the last frame.
fn fills_at(h: &Harness<'static, App>, rect: egui::Rect) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, rect: egui::Rect, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, rect, found)),
            egui::Shape::Rect(shape)
                if (shape.rect.min - rect.min).length() < 0.5 && (shape.rect.max - rect.max).length() < 0.5 =>
            {
                found.push(shape.fill)
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, rect, &mut found));
    found
}

/// The demo with Drums soloed, cued and armed: each of its three buttons lit, Keys' unlit.
fn lit_drums() -> Harness<'static, App> {
    let mut h = window(demo_app());
    for label in ["Solo Drums", "Cue Drums on the headphones", "Arm Drums for recording"] {
        h.get_by_label(label).click();
        h.run();
    }
    h
}

/// A track's solo, cue and arm buttons are `button#control-solo`, `#control-cue` and
/// `#control-arm`, `[checked]` while on (and the arm `[armed]`): a rule for each, lit and unlit,
/// reaches its fill (#1023).
#[test]
fn solo_cue_and_arm_take_their_own_rules_lit_and_unlit() {
    let mut h = lit_drums();
    let rules = [
        ("button#control-solo", "background-color", odd(51)),
        ("button#control-solo[checked]", "background-color", odd(52)),
        ("button#control-cue", "background-color", odd(53)),
        ("button#control-cue[checked]", "background-color", odd(54)),
        ("button#control-arm", "background-color", odd(55)),
        ("button#control-arm[armed]", "background-color", odd(56)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// A rule for every `button` does not hide which track button is on: the lit solo, cue and arm keep
/// their lit fills, the unlit ones take the rule's (#1023).
#[test]
fn a_rule_for_every_button_keeps_solo_cue_and_arm_lit() {
    let mut h = lit_drums();
    let css = format!("button {{ background-color: {}; }}", hex(odd(57)));
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    for (lit, unlit) in [
        ("End solo on Drums", "Solo Keys"),
        ("Stop cueing Drums", "Cue Keys on the headphones"),
        ("Disarm Drums", "Arm Keys for recording"),
    ] {
        let lit_fills = fills_at(&h, h.get_by_label(lit).rect());
        assert!(!lit_fills.is_empty(), "{lit} is painted");
        assert!(!lit_fills.contains(&odd(57)), "{lit} keeps its lit fill: {lit_fills:?}");
        assert!(fills_at(&h, h.get_by_label(unlit).rect()).contains(&odd(57)), "{unlit} takes the rule");
    }
    suhina_ui::kit::native_css::set_user_sheet(None);
}

/// The Arrangement's swing is a `slider`, `[checked]` while it is lit: its rail, its fill and its
/// handle take their parts' rules (#1023).
#[test]
fn the_swing_slider_s_parts_take_their_own_rules() {
    let mut h = arrangement_app();
    assert!(h.query_by_label_contains("Global swing").is_some(), "the swing is shown");
    let rules = [
        ("slider[checked] slider-rail", "background-color", odd(61)),
        ("slider[checked] slider-fill", "background-color", odd(62)),
        ("slider[checked] slider-handle", "background-color", odd(63)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// An audio clip's Pitch dial is a `knob`: its rim in its `border-color`, its inner disc in its
/// `background-color`, its pointer in its `accent-color`. Clicked and typed into, the number is a
/// `text-input[kind="typed"]`: its ring, its fill and its text (#1023).
#[test]
fn the_pitch_dial_and_a_typed_number_take_their_rules() {
    let mut h = window_with_step(audio_app(), 1.0 / 60.0);
    h.get_by_label("Arrangement view").click();
    h.run();
    h.get_by_label("Clip Loop on Loop at bar 1").click();
    h.run();
    let rules =
        [("knob", "border-color", odd(71)), ("knob", "background-color", odd(72)), ("knob", "accent-color", odd(73))];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
    h.get_by_label_contains("Pitch: ").click();
    h.run();
    h.event(egui::Event::Text("3".into()));
    h.run();
    let rules = [
        ("text-input[kind=\"typed\"]", "border-color", odd(74)),
        ("text-input[kind=\"typed\"]", "background-color", odd(75)),
        ("text-input[kind=\"typed\"]", "color", odd(76)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// A text field's and a drop-down's underline is their `border-color` (#1023).
#[test]
fn a_field_s_underline_takes_its_border_colour() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut h = window(app);
    let rules = [("text-input", "border-color", odd(81)), ("dropdown", "border-color", odd(82))];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// A track header's routing fields take the colours of the header: a rule for every `dropdown`
/// leaves them alone, one for `dropdown[kind="subtle"]` reaches them (#1023).
#[test]
fn a_routing_field_takes_only_its_own_kind_s_rule() {
    let mut h = arrangement_app();
    // The colours of the lines along the bottom of the corner's Resize all field.
    let underline = |h: &mut Harness<'static, App>, css: String| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
        for _ in 0..4 {
            h.step();
        }
        let field = h.get_by_label("Resize all tracks").rect();
        let mut found = Vec::new();
        for shape in &h.output().shapes {
            if let egui::Shape::LineSegment { points, stroke } = &shape.shape
                && points.iter().all(|p| (p.y - (field.bottom() - 0.5)).abs() < 0.01)
                && (points[0].x - field.left()).abs() < 0.5
            {
                found.push(stroke.color);
            }
        }
        suhina_ui::kit::native_css::set_user_sheet(None);
        found
    };
    let general = underline(&mut h, format!("dropdown {{ border-color: {}; }}", hex(odd(83))));
    assert!(!general.is_empty(), "the field has its line");
    assert!(!general.contains(&odd(83)), "a rule for every dropdown leaves it alone: {general:?}");
    let own = underline(&mut h, format!("dropdown[kind=\"subtle\"] {{ border-color: {}; }}", hex(odd(84))));
    assert!(own.contains(&odd(84)), "its own kind's rule reaches it: {own:?}");
}

/// Turned off, a solo button fades from its lit rule's fill to its plain one, never by way of the
/// palette's: the lit side is looked up while the fade runs, not only while it is on (#1435
/// review). Frames of a fixed step, a few into the fade.
#[test]
fn a_solo_turned_off_fades_from_its_lit_rule_s_fill() {
    let mut h = window_with_step(demo_app(), 1.0 / 60.0);
    h.get_by_label("Solo Drums").click();
    h.run();
    let css = "button#control-solo { background-color: #101010; } \
               button#control-solo[checked] { background-color: #f0f0f0; }";
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    h.run_steps(30);
    h.get_by_label("End solo on Drums").click();
    h.run_steps(3);
    let fills = fills_at(&h, h.get_by_label("Solo Drums").rect());
    suhina_ui::kit::native_css::set_user_sheet(None);
    let grey = |c: &egui::Color32| {
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2 && r > 0x10 && r < 0xf0
    };
    assert!(fills.iter().any(grey), "half way between the two rules' greys: {fills:?}");
}

/// A rule's `background-image` for every `button` does not cover a lit solo, cue or arm, which
/// shows its lit fill; the unlit ones keep the image; a `[checked]` rule's own image paints over
/// the lit one (#1435 review).
#[test]
fn a_rule_s_background_image_does_not_cover_a_lit_track_button() {
    let mut h = lit_drums();
    // The gradients painted exactly over `rect`.
    let meshes_at = |h: &Harness<'static, App>, label: &str| {
        let rect = h.get_by_label(label).rect();
        h.output()
            .shapes
            .iter()
            .filter(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() => {
                    let bounds = mesh.calc_bounds();
                    (bounds.min - rect.min).length() < 0.5 && (bounds.max - rect.max).length() < 0.5
                }
                _ => false,
            })
            .count()
    };
    let show = |h: &mut Harness<'static, App>, css: &str| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
    };
    let pairs = [
        ("End solo on Drums", "Solo Keys"),
        ("Stop cueing Drums", "Cue Keys on the headphones"),
        ("Disarm Drums", "Arm Keys for recording"),
    ];
    // Without a rule: the buttons' artwork alone.
    show(&mut h, "");
    let artwork: Vec<_> = pairs.iter().map(|(lit, unlit)| (meshes_at(&h, lit), meshes_at(&h, unlit))).collect();
    let general = "button { background-image: linear-gradient(#202020, #404040); }";
    show(&mut h, general);
    for ((lit, unlit), (lit_art, unlit_art)) in pairs.iter().zip(&artwork) {
        assert_eq!(meshes_at(&h, lit), *lit_art, "{lit} shows its lit fill, no image over it");
        assert!(meshes_at(&h, unlit) > *unlit_art, "{unlit} keeps the image");
    }
    show(&mut h, &format!("{general} button[checked] {{ background-image: linear-gradient(#f0f0f0, #d0d0d0); }}"));
    assert!(meshes_at(&h, "End solo on Drums") > artwork[0].0, "a lit state's own image paints");
    suhina_ui::kit::native_css::set_user_sheet(None);
}

/// A rule for every `text-input` does not reach a typed number, which is drawn over a value
/// control: only one for `text-input[kind="typed"]` does (#1435 review).
#[test]
fn a_rule_for_every_text_field_leaves_a_typed_number_alone() {
    let mut h = window_with_step(audio_app(), 1.0 / 60.0);
    h.get_by_label("Arrangement view").click();
    h.run();
    h.get_by_label("Clip Loop on Loop at bar 1").click();
    h.run();
    h.get_by_label_contains("Pitch: ").click();
    h.run();
    h.event(egui::Event::Text("3".into()));
    h.run();
    let dial = h.get_by_label_contains("Pitch: ").rect();
    // The typed number's fill, and every text colour over the dial.
    let typed = |h: &mut Harness<'static, App>, css: String| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
        for _ in 0..4 {
            h.step();
        }
        let fills = fills_at(h, dial.shrink(1.0));
        let mut inks = Vec::new();
        for shape in &h.output().shapes {
            if let egui::Shape::Text(text) = &shape.shape
                && dial.contains(text.visual_bounding_rect().center())
            {
                inks.push(text.fallback_color);
            }
        }
        suhina_ui::kit::native_css::set_user_sheet(None);
        (fills, inks)
    };
    let (fills, inks) =
        typed(&mut h, format!("text-input {{ background-color: {}; color: {}; }}", hex(odd(91)), hex(odd(92))));
    assert!(!fills.is_empty() && !inks.is_empty(), "the typed number is drawn");
    assert!(!fills.contains(&odd(91)), "the typed fill keeps its own: {fills:?}");
    assert!(!inks.contains(&odd(92)), "the typed text keeps its own: {inks:?}");
    let own = format!("text-input[kind=\"typed\"] {{ background-color: {}; color: {}; }}", hex(odd(93)), hex(odd(94)));
    let (fills, inks) = typed(&mut h, own);
    assert!(fills.contains(&odd(93)) && inks.contains(&odd(94)), "the kind's rule reaches it");
}

/// A rule for every `slider-fill` does not hide that the swing is lit: only one for the lit
/// slider's fill changes it (#1435 review).
#[test]
fn a_rule_for_every_slider_fill_keeps_the_swing_lit() {
    let mut h = arrangement_app();
    let general = format!("slider-fill {{ background-color: {}; }}", hex(odd(95)));
    assert_eq!(count_with(&mut h, &general, odd(95)), 0, "the lit swing keeps its fill");
    let lit = format!("{general} slider[checked] slider-fill {{ background-color: {}; }}", hex(odd(96)));
    assert_eq!(count_with(&mut h, &lit, odd(96)), 1, "a rule for the lit slider's fill reaches it");
}

/// The demo with Walk on Bass picked in the Launcher: its MIDI clip's panel, its follow action and
/// its piano roll.
fn walk_picked() -> Harness<'static, App> {
    let mut h = window(demo_app());
    h.get_by_label("Select Walk on Bass (scene 2)").click();
    h.run();
    h
}

/// A MIDI clip's panel, its follow action and the piano roll's toolbar draw their words and lines
/// from their elements (#1023): a rule for each kind reaches what it paints.
#[test]
fn a_clip_panel_s_words_lines_and_menus_take_their_own_rules() {
    let mut h = walk_picked();
    let rules = [
        ("label[kind=\"title\"]", "color", odd(71)),
        ("label[kind=\"caption\"]", "color", odd(72)),
        ("label[kind=\"hint\"]", "color", odd(73)),
        ("separator[kind=\"row\"]", "color", odd(75)),
        ("dropdown[kind=\"subtle\"]", "color", odd(76)),
        ("piano-tool#piano-more", "color", odd(78)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// A colour a theme gives a whole clip column reaches its titles, not its dim words: `hint` and
/// `caption` take only a colour given to them (#1023).
#[test]
fn a_colour_for_a_whole_clip_column_leaves_its_dim_words_dim() {
    let mut h = walk_picked();
    let css = format!("clip-info {{ color: {}; }}", hex(odd(79)));
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let shown = painted(&h);
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(shown.contains(&odd(79)), "the titles take the area's colour");
    let (caption, hint) = (suhina_ui::kit::theme::text_secondary(), suhina_ui::kit::theme::text_hint());
    assert!(shown.contains(&caption) && shown.contains(&hint), "the captions and hints stay dim");
}

/// A rule for every `button` does not hide which of the follow action's switches is on (#1023).
#[test]
fn a_rule_for_every_button_keeps_the_follow_action_s_switch_lit() {
    let mut h = walk_picked();
    let css = format!("button {{ background-color: {}; }}", hex(odd(80)));
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let lit = fills_at(&h, h.get_by_label("Linked to the clip's length").rect());
    let unlit = fills_at(&h, h.get_by_label("Follow action off").rect());
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(!lit.is_empty() && !lit.contains(&odd(80)), "the lit switch keeps its fill: {lit:?}");
    assert!(unlit.contains(&odd(80)), "the unlit switch takes the rule: {unlit:?}");
}

/// A device group's panel: its branch's row, its add button and its on switch take their own
/// rules (#1023).
#[test]
fn a_device_group_s_panel_takes_its_own_rules() {
    let mut session = suhina_session::Session::new(demo_project());
    session.execute("device.add", &serde_json::json!({ "track": "Keys", "id": "utility" })).unwrap();
    session.execute("device.group", &serde_json::json!({ "track": "Keys", "device": 0 })).unwrap();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut h = window(app);
    h.get_by_label("Track Keys").click();
    h.run();
    let rules = [
        ("device-row[selected]", "background-color", odd(81)),
        ("add-track[kind=\"branch\"]", "color", odd(82)),
        ("button#control-device-on", "background-color", odd(83)),
        ("device-header", "background-color", odd(84)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// The browser's words and lines take their own rules, and its search field `text-input`'s (#1023).
#[test]
fn the_browser_s_words_and_lines_take_their_own_rules() {
    let mut app = demo_app();
    app.set_browser_open(true);
    let mut h = window(app);
    let rules = [
        ("label[kind=\"hint\"]", "color", odd(85)),
        ("label[kind=\"caption\"]", "color", odd(86)),
        ("separator[kind=\"row\"]", "color", odd(87)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// The Arrangement's track header edge is `divider#arrangement-header-splitter`: unpainted
/// without a rule, its `background-color` with one, and it still resizes the headers (#1023).
#[test]
fn the_header_edge_is_a_divider_that_still_resizes_the_headers() {
    let mut h = window(demo_app());
    h.get_by_label("Arrangement view").click();
    h.run();
    let edge = h.get_by_label("Track header width").rect();
    assert!(!fills_at(&h, edge).contains(&odd(88)));
    assert_eq!(
        missing(&mut h, &[("divider#arrangement-header-splitter", "background-color", odd(88))]),
        Vec::<String>::new()
    );
    let before = h.get_by_label("Track header width").rect().center().x;
    let from = edge.center();
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for dx in [20.0, 40.0] {
        h.hover_at(from + egui::vec2(dx, 0.0));
        h.run();
    }
    h.drop_at(from + egui::vec2(40.0, 0.0));
    h.run();
    let after = h.get_by_label("Track header width").rect().center().x;
    assert!((after - before - 40.0).abs() < 1.0, "the edge moved with the drag: {before} to {after}");
}
