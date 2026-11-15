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

/// A rule for every Arrangement tool is the chosen one's fill too, as the base sheet's `[checked]`
/// rule is hidden by it: a theme that wants the chosen tool apart states `[checked]` (#1450).
#[test]
fn a_rule_for_every_arrangement_tool_needs_a_checked_rule_to_keep_the_chosen_one_lit() {
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
    assert_eq!(fills(&mut h, &general), (tools, 0), "a rule for every tool is the chosen one's fill too");
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

/// A hover rule is the hovered chosen tool's fill too: the base sheet's `[checked]` look is hidden by
/// a rule for every hovered tool, so a theme that wants it apart states `[checked]:hover` (#1450).
#[test]
fn a_hover_rule_is_the_hovered_chosen_tool_s_fill_too() {
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
    assert_eq!(count_with(&mut h, &hover, odd(45)), 1, "the hovered chosen tool takes the hover rule");
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

/// A `clip:hover` rule's border colour is the hovered selected slot clip's outline too: the base
/// sheet's `clip[selected]` outline is hidden by it, so a theme states `clip[selected]:hover`
/// (#1450).
#[test]
fn a_hover_rule_is_a_slot_clip_s_selected_outline_too() {
    let mut h = window(demo_app());
    h.get_by_label("Select Walk on Bass (scene 2)").click();
    h.run();
    let at = h.get_by_label("Select Walk on Bass (scene 2)").rect().center();
    h.hover_at(at);
    h.run();
    let hover = format!("clip:hover {{ border-color: {}; }}", hex(odd(52)));
    assert_eq!(strokes_with(&mut h, &hover, odd(52)), 1, "the outline takes the hover rule");
    let selected = format!("{hover} clip[selected]:hover {{ border-color: {}; }}", hex(odd(53)));
    assert_eq!(strokes_with(&mut h, &selected, odd(53)), 1, "a `[selected]` rule recolours it");
}

/// A rule for every clip's `color` is not a selected clip's outline, but its `background-color` is
/// the deactivated clip's fill too: the base sheet's `clip[muted]` is hidden by it, so a theme
/// states `clip[muted]` (#1450).
#[test]
fn a_rule_for_every_slot_clip_needs_a_muted_rule_to_keep_the_deactivated_clip_grey() {
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
    assert_eq!(count_with(&mut h, &fill, odd(55)), 1, "the deactivated clip takes the clips' fill");
}

/// A `color` rule for every tool is the chosen tool's ink too: a theme that wants it apart states
/// `arrangement-tool[checked]` (#1450).
#[test]
fn a_colour_rule_for_every_tool_is_the_chosen_one_s_ink_too() {
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
    assert_eq!(inked.len(), tools, "every tool takes the colour: {inked:?}");
}

/// A `color` rule for the monitor buttons is the lit one's label ink too: a theme that wants it
/// apart states `[checked]` (#1450).
#[test]
fn a_colour_rule_for_the_monitor_buttons_is_the_lit_one_s_label_ink_too() {
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
    assert_eq!(inked.len(), 3, "all three take the colour: {inked:?}");
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

/// A rule for every `button` is the lit solo, cue and arm's too until the theme states their lit
/// look (`button[kind="toggle"][checked]`, `[kind="solo"]` and `[kind="arm"]`); the unlit ones keep
/// the rule's fill (#1449).
#[test]
fn a_rule_for_every_button_needs_lit_rules_to_keep_solo_cue_and_arm_lit() {
    let mut h = lit_drums();
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
    let general = format!("button {{ background-color: {}; }}", hex(odd(57)));
    show(&mut h, &general);
    for (lit, unlit) in pairs {
        assert!(fills_at(&h, h.get_by_label(lit).rect()).contains(&odd(57)), "{lit} takes the rule too");
        assert!(fills_at(&h, h.get_by_label(unlit).rect()).contains(&odd(57)), "{unlit} takes the rule");
    }
    let stated = format!(
        "{general} button[kind=\"toggle\"][checked], button[kind=\"solo\"][checked], button[kind=\"arm\"][checked] \
         {{ background-color: {}; }}",
        hex(odd(58))
    );
    show(&mut h, &stated);
    for (lit, unlit) in pairs {
        let lit_fills = fills_at(&h, h.get_by_label(lit).rect());
        assert!(lit_fills.contains(&odd(58)) && !lit_fills.contains(&odd(57)), "{lit} is lit: {lit_fills:?}");
        assert!(fills_at(&h, h.get_by_label(unlit).rect()).contains(&odd(57)), "{unlit} keeps the rule");
    }
    suhina_ui::kit::native_css::set_user_sheet(None);
}

/// The browser's chips are `button[kind="chip"]`, `[checked]` while chosen: a rule for every
/// `button` is the chosen chip's too, so a theme states the lit look, which then shows which chip
/// is chosen (#1023, #1451).
#[test]
fn a_rule_for_every_button_needs_a_lit_rule_to_keep_the_browser_s_chosen_chip_lit() {
    let mut h = browser_with_chips();
    let general = format!("button {{ background-color: {}; }}", hex(odd(97)));
    let lit_and_unlit = |h: &mut Harness<'static, App>, css: &str| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        let lit = fills_at(h, h.get_by_label("Recent All").rect());
        let unlit = fills_at(h, h.get_by_label("Recent Samples").rect());
        suhina_ui::kit::native_css::set_user_sheet(None);
        (lit, unlit)
    };
    let (lit, unlit) = lit_and_unlit(&mut h, &general);
    assert!(lit.contains(&odd(97)), "the chosen chip takes the rule too: {lit:?}");
    assert!(unlit.contains(&odd(97)), "an unlit chip takes the rule: {unlit:?}");
    let stated = format!("{general} button[kind=\"chip\"][checked] {{ background-color: {}; }}", hex(odd(98)));
    let (lit, unlit) = lit_and_unlit(&mut h, &stated);
    assert!(lit.contains(&odd(98)) && !lit.contains(&odd(97)), "the chosen chip is lit: {lit:?}");
    assert!(unlit.contains(&odd(97)), "an unlit chip keeps the rule: {unlit:?}");
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

/// A rule for every `text-input` is the typed number's too, which is drawn over a value control,
/// until the theme gives `text-input[kind="typed"]` its own (#1449).
#[test]
fn a_rule_for_every_text_field_is_a_typed_number_s_until_it_has_its_own() {
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
    assert!(fills.contains(&odd(91)), "the typed fill takes the rule: {fills:?}");
    assert!(inks.contains(&odd(92)), "the typed text takes the rule: {inks:?}");
    let own = format!("text-input[kind=\"typed\"] {{ background-color: {}; color: {}; }}", hex(odd(93)), hex(odd(94)));
    let (fills, inks) = typed(&mut h, own);
    assert!(fills.contains(&odd(93)) && inks.contains(&odd(94)), "the kind's rule reaches it");
}

/// A rule for every `slider-fill` is the lit swing's too until the theme states the lit slider's
/// fill: only a rule for `slider[checked] slider-fill` changes it (#1449).
#[test]
fn a_rule_for_every_slider_fill_needs_a_lit_rule_to_keep_the_swing_lit() {
    let mut h = arrangement_app();
    let general = format!("slider-fill {{ background-color: {}; }}", hex(odd(95)));
    assert_eq!(count_with(&mut h, &general, odd(95)), 1, "the lit swing takes the rule too");
    let lit = format!("{general} slider[checked] slider-fill {{ background-color: {}; }}", hex(odd(96)));
    assert_eq!(count_with(&mut h, &lit, odd(96)), 1, "a rule for the lit slider's fill reaches it");
    assert_eq!(count_with(&mut h, &lit, odd(95)), 0, "and it leaves the general rule");
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

/// The follow action's switches are `button[kind="toggle"]` (its own On, `button[kind="solo"]`),
/// lit with `[checked]`: a rule for every `button` is the lit switch's too, so a theme states the
/// lit look, which then shows which switch is on (#1023, #1451).
#[test]
fn a_rule_for_every_button_needs_a_lit_rule_to_keep_the_follow_action_s_switch_lit() {
    let mut h = walk_picked();
    let general = format!("button {{ background-color: {}; }}", hex(odd(80)));
    let lit_and_unlit = |h: &mut Harness<'static, App>, css: &str| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        let lit = fills_at(h, h.get_by_label("Linked to the clip's length").rect());
        let unlit = fills_at(h, h.get_by_label("Follow action off").rect());
        suhina_ui::kit::native_css::set_user_sheet(None);
        (lit, unlit)
    };
    let (lit, unlit) = lit_and_unlit(&mut h, &general);
    assert!(lit.contains(&odd(80)), "the lit switch takes the rule too: {lit:?}");
    assert!(unlit.contains(&odd(80)), "the unlit switch takes the rule: {unlit:?}");
    let stated = format!(
        "{general} button[kind=\"toggle\"][checked], button[kind=\"solo\"][checked] {{ background-color: {}; }}",
        hex(odd(88))
    );
    let (lit, unlit) = lit_and_unlit(&mut h, &stated);
    assert!(lit.contains(&odd(88)) && !lit.contains(&odd(80)), "the lit switch is lit: {lit:?}");
    assert!(unlit.contains(&odd(80)), "the unlit switch keeps the rule: {unlit:?}");
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

/// The text colours painted with their galley's top-left inside `rect` in the last frame.
fn text_colours_at(h: &Harness<'static, App>, rect: egui::Rect) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, rect: egui::Rect, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, rect, found)),
            egui::Shape::Text(text) if rect.contains(text.galley.rect.translate(text.pos.to_vec2()).center()) => {
                found.push(text.fallback_color);
                found.extend(text.galley.job.sections.iter().map(|section| section.format.color));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, rect, &mut found));
    found
}

/// The browser's new-folder field is `text-input#browser-new-folder`: a rule for that id colours
/// the text typed in it (#1438).
#[test]
fn the_browser_s_new_folder_field_takes_its_own_id_s_text_colour() {
    let mut app = demo_app();
    app.set_browser_open(true);
    let mut h = window(app);
    h.get_by_label("Add folder").click();
    h.run();
    h.event(egui::Event::Text("some/path".into()));
    h.run();
    let rules = [("text-input#browser-new-folder", "color", odd(110))];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// The browser's filter chips are drawn inside the browser: a rule scoped to it reaches their
/// words, which are the chip's own colour, `button[kind="chip"]` (#1438, #1451).
#[test]
fn a_rule_scoped_to_the_browser_reaches_the_filter_chips_words() {
    let mut h = browser_with_chips();
    suhina_ui::kit::native_css::set_user_sheet(Some(
        format!("browser button[kind=\"chip\"] {{ color: {}; }}", hex(odd(111))).into(),
    ));
    for _ in 0..4 {
        h.step();
    }
    let words = text_colours_at(&h, h.get_by_label("Recent Samples").rect());
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(words.contains(&odd(111)), "an unlit chip's words take the browser's rule: {words:?}");
}

/// The device chain's insert line, drawn while a device is carried to a place, is
/// `drop-target[kind="insert"]`, as the other views' insert lines are (#1438).
#[test]
fn the_device_chain_s_insert_line_is_a_drop_target_of_kind_insert() {
    let mut session = suhina_session::Session::new(demo_project());
    for id in ["utility", "utility"] {
        session.execute("device.add", &serde_json::json!({ "track": "Keys", "id": id })).unwrap();
    }
    let track = session.project().tracks.iter().find(|t| t.name == "Keys").map(|t| t.id).unwrap();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut h = window(app);
    h.get_by_label("Track Keys").click();
    h.run();
    // The first device is carried, with the pointer far right of the chain: the end of it.
    let pointer = egui::pos2(h.ctx.content_rect().right() - 300.0, h.ctx.content_rect().bottom() - 100.0);
    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(suhina_ui::interaction::drop::DEVICE_CARRYING), (track, 0usize)));
    h.event(egui::Event::PointerMoved(pointer));
    h.step();
    let rules = [("drop-target[kind=\"insert\"]", "color", odd(112))];
    let missing = missing(&mut h, &rules);
    h.ctx.data_mut(|d| {
        d.remove::<(suhina_engine::model::TrackId, usize)>(egui::Id::unique(
            suhina_ui::interaction::drop::DEVICE_CARRYING,
        ))
    });
    assert_eq!(missing, Vec::<String>::new());
}

/// The control bar's words, the tabs, the toolbar's lit switches and the Launcher's mixer take
/// their own rules (#1440).
#[test]
fn the_control_bar_s_words_and_switches_take_their_own_rules() {
    let mut h = window(demo_app());
    h.get_by_label("Computer keyboard").click();
    h.run();
    let rules = [
        // "Quantize" and "Count-in" are hints, the keys' and the clock's words the toolbar's own.
        ("label[kind=\"hint\"]", "color", odd(120)),
        ("toolbar-module#keys button[checked]", "background-color", odd(121)),
        ("toolbar-module#position", "color", odd(122)),
        ("toolbar-module#views", "color", odd(123)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// A rule for every `button` is the toolbar's lit switch's too until the theme states the lit look:
/// the computer keyboard takes the lit rule's fill while it plays (#1449).
#[test]
fn a_rule_for_every_button_needs_a_lit_rule_to_keep_the_computer_keyboard_lit() {
    let mut h = window(demo_app());
    let general = format!("button {{ background-color: {}; }}", hex(odd(124)));
    let fills = |h: &mut Harness<'static, App>, css: &str, label: &str| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        let found = fills_at(h, h.get_by_label_contains(label).rect());
        suhina_ui::kit::native_css::set_user_sheet(None);
        found
    };
    let unlit = fills(&mut h, &general, "Computer keyboard");
    assert!(unlit.contains(&odd(124)), "the unlit switch takes the rule: {unlit:?}");
    h.get_by_label("Computer keyboard").click();
    h.run();
    let lit = fills(&mut h, &general, "Computer keyboard on");
    assert!(lit.contains(&odd(124)), "the lit switch takes it too: {lit:?}");
    let stated = format!("{general} button[kind=\"toggle\"][checked] {{ background-color: {}; }}", hex(odd(127)));
    let lit = fills(&mut h, &stated, "Computer keyboard on");
    assert!(lit.contains(&odd(127)) && !lit.contains(&odd(124)), "its lit rule lights it: {lit:?}");
}

/// A theme's rule for every `button` is a track's on switch's too, lit or not, until the theme
/// states the lit look: its rule for `button[kind="toggle"][checked]` lights the switches of the
/// tracks that are on, in the Arrangement's header and in the mixer strip (#1449).
#[test]
fn a_rule_for_every_button_needs_a_lit_rule_to_keep_a_track_s_on_switch_lit() {
    for view in [None, Some("Arrangement view")] {
        let mut h = window(demo_app());
        if let Some(view) = view {
            h.get_by_label(view).click();
            h.run();
        }
        h.get_by_label("Turn Keys off").click();
        h.run();
        let fills = |h: &mut Harness<'static, App>, css: String| {
            suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
            for _ in 0..4 {
                h.step();
            }
            let lit = fills_at(h, h.get_by_label("Turn Drums off").rect());
            let off = fills_at(h, h.get_by_label("Turn Keys on").rect());
            suhina_ui::kit::native_css::set_user_sheet(None);
            (lit, off)
        };
        let general = format!("button {{ background-color: {}; }}", hex(odd(125)));
        let (lit, off) = fills(&mut h, general.clone());
        assert!(lit.contains(&odd(125)), "{view:?}: the rule is a lit switch's too: {lit:?}");
        assert!(off.contains(&odd(125)), "{view:?}: a track that is off takes the rule: {off:?}");
        let stated = format!("{general} button[kind=\"toggle\"][checked] {{ background-color: {}; }}", hex(odd(126)));
        let (lit, off) = fills(&mut h, stated);
        assert!(lit.contains(&odd(126)) && !lit.contains(&odd(125)), "{view:?}: its lit rule lights it: {lit:?}");
        assert!(off.contains(&odd(125)), "{view:?}: a track that is off keeps the rule: {off:?}");
    }
}

/// A dialog's title row, its words and its lists take their own rules (#1440).
#[test]
fn a_dialog_s_title_words_and_rows_take_their_own_rules() {
    let mut h = window(demo_app());
    h.state_mut().open_project_dialog(false);
    h.run();
    let rules = [
        ("dialog-title", "background-color", odd(126)),
        ("dialog-title label[kind=\"title\"]", "color", odd(127)),
        ("dialog label[kind=\"hint\"]", "color", odd(128)),
        ("dialog separator[kind=\"row\"]", "color", odd(129)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// The unsaved-changes dialog and the agent prompt read their words from `label`'s kinds (#1440).
#[test]
fn the_questions_a_dialog_asks_take_their_own_words() {
    let mut h = window(demo_app());
    h.state_mut().ask_agent(1, "An assistant".into());
    h.run();
    let rules = [("dialog label", "color", odd(130)), ("dialog label[kind=\"hint\"]", "color", odd(131))];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// The start page's tabs are `editor-tab#tab-recent` and `#tab-templates`, `[checked]` for the one
/// shown (#1440).
#[test]
fn the_start_page_s_tabs_take_their_own_rules() {
    let mut app = App::new();
    app.set_browser_open(false);
    let mut h = window(app);
    let rules = [
        ("editor-tab#tab-recent[checked]", "color", odd(132)),
        ("editor-tab#tab-templates", "color", odd(133)),
        ("start-page label[kind=\"hint\"]", "color", odd(134)),
        ("start-page separator[kind=\"row\"]", "color", odd(135)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// Settings' words, headings and buttons take their own rules, and its lit switches and chosen
/// segments stay lit under a rule for every `button` (#1440).
#[test]
fn settings_words_and_buttons_take_their_own_rules() {
    let mut h = window(demo_app());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_by_label("Settings page: Keys").click();
    h.run();
    let rules = [
        ("settings label[kind=\"hint\"]", "color", odd(136)),
        ("settings label[kind=\"caption\"]", "color", odd(137)),
        ("settings separator[kind=\"row\"]", "color", odd(138)),
        ("settings button", "color", odd(139)),
        ("settings button[checked]", "background-color", odd(140)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

#[test]
fn a_rule_for_every_button_keeps_settings_chosen_segment_lit() {
    let mut h = window(demo_app());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_by_label("Settings page: Keys").click();
    h.run();
    h.get_by_label("Preset: Mouse-first").click();
    h.run();
    let css = format!("button {{ background-color: {}; }}", hex(odd(141)));
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let lit = fills_at(&h, h.get_by_label("Preset: Mouse-first").rect());
    let unlit = fills_at(&h, h.get_by_label("Preset: Draw-first").rect());
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(!lit.is_empty() && !lit.contains(&odd(141)), "the chosen preset keeps its fill: {lit:?}");
    assert!(unlit.contains(&odd(141)), "an unchosen one takes the rule: {unlit:?}");
}

/// A yes/no setting's switch is lit by a rule for `[checked]` only: a rule for every `button`
/// colours the unlit ones and leaves the lit ones lit (#1440).
#[test]
fn a_rule_for_every_button_keeps_settings_switches_lit() {
    use egui_kittest::kittest::NodeT;
    let mut h = window(demo_app());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    let css = format!("button {{ background-color: {}; }}", hex(odd(142)));
    let (mut lit, mut unlit) = (0, 0);
    for page in ["Projects and backups", "Appearance", "Browser", "Launcher and mixer", "Arrangement"] {
        h.get_by_label(&format!("Settings page: {page}")).click();
        h.run();
        suhina_ui::kit::native_css::set_user_sheet(Some(css.clone().into()));
        for _ in 0..4 {
            h.step();
        }
        for switch in h.query_all_by_role(egui::accesskit::Role::CheckBox) {
            let fills = fills_at(&h, switch.rect());
            if fills.is_empty() {
                continue;
            }
            if switch.accesskit_node().toggled() == Some(egui::accesskit::Toggled::True) {
                lit += 1;
                assert!(!fills.contains(&odd(142)), "a lit switch keeps its fill: {fills:?}");
            } else {
                unlit += 1;
                assert!(fills.contains(&odd(142)), "an unlit switch takes the rule: {fills:?}");
            }
        }
        suhina_ui::kit::native_css::set_user_sheet(None);
    }
    assert!(lit > 0 && unlit > 0, "the pages hold lit and unlit switches ({lit} and {unlit})");
}

/// The colour picker's swatches are `colour-swatch`es: the chosen one is outlined in its
/// `border-color` (#1440).
#[test]
fn the_colour_picker_s_swatches_take_their_own_rules() {
    let mut h = window(demo_app());
    h.get_by_label("Track Drums").click_secondary();
    h.run();
    assert_eq!(
        missing(&mut h, &[("colour-swatch", "border-color", odd(143))]),
        Vec::<String>::new(),
        "the swatch under the pointer or the chosen one is outlined"
    );
}

/// The detail area's splitter takes `[closed]` while the area is closed, and only then (#1440).
#[test]
fn the_detail_splitter_takes_closed_only_while_the_area_is_closed() {
    let mut h = window(demo_app());
    let open_rules = [("divider#detail-splitter", "background-color", odd(144))];
    let closed_rules = [("divider#detail-splitter[closed]", "background-color", odd(145))];
    h.run();
    // Closed as it starts, or open: say which, then the other.
    let shown_closed = |h: &mut Harness<'static, App>| {
        let all = missing(h, &open_rules);
        assert!(all.is_empty(), "the splitter is drawn: {all:?}");
        missing(h, &closed_rules).is_empty()
    };
    let first = shown_closed(&mut h);
    h.get_by_label("View").click();
    h.run();
    let item = if first { "Show detail area" } else { "Hide detail area" };
    h.get_by_label_contains(item).click();
    h.run();
    let second = shown_closed(&mut h);
    assert_ne!(first, second, "[closed] reaches the splitter of the closed area only");
}

/// The colours of the text painted inside `rect` in the last frame.
fn inks_in(h: &Harness<'static, App>, rect: egui::Rect) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, rect: egui::Rect, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, rect, found)),
            egui::Shape::Text(text) if rect.contains(text.pos) || rect.intersects(text.visual_bounding_rect()) => {
                found.push(text.fallback_color);
                found.extend(text.galley.job.sections.iter().map(|section| section.format.color));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, rect, &mut found));
    found
}

/// Shows `h` under `css` for a few frames, runs `read`, and puts the user's sheet away again.
fn under<R>(h: &mut Harness<'static, App>, css: String, read: impl FnOnce(&Harness<'static, App>) -> R) -> R {
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let out = read(h);
    suhina_ui::kit::native_css::set_user_sheet(None);
    out
}

/// A rule for every `list-row` is the picked row's too until the theme states the picked one's:
/// its rule for `list-row[selected]` gives that row its own ink (#1449).
#[test]
fn a_rule_for_every_list_row_needs_a_selected_rule_to_keep_the_picked_row_lit() {
    let mut h = window(demo_app());
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    h.run();
    let general = format!("list-row {{ color: {}; }}", hex(odd(150)));
    let rows = |h: &mut Harness<'static, App>, css: String| {
        under(h, css, |h| {
            let rows = h.query_all_by_label_contains("Add ").collect::<Vec<_>>();
            assert!(rows.len() >= 2, "the list shows rows");
            (inks_in(h, rows[0].rect()), inks_in(h, rows[1].rect()))
        })
    };
    let (picked, other) = rows(&mut h, general.clone());
    assert!(picked.contains(&odd(150)), "the picked row takes the rule too: {picked:?}");
    assert!(other.contains(&odd(150)), "an other row takes the rule: {other:?}");
    let stated = format!("{general} list-row[selected] {{ color: {}; }}", hex(odd(151)));
    let (picked, other) = rows(&mut h, stated);
    assert!(picked.contains(&odd(151)) && !picked.contains(&odd(150)), "the picked row's own rule: {picked:?}");
    assert!(other.contains(&odd(150)), "an other row keeps the rule: {other:?}");
}

/// A rule for every `editor-tab` does not make the shown start tab look like the other: it keeps
/// its own ink (#1440 review).
#[test]
fn a_rule_for_every_tab_keeps_the_shown_start_tab_apart() {
    let mut app = App::new();
    app.set_browser_open(false);
    let mut h = window(app);
    let css = format!("editor-tab {{ color: {}; }}", hex(odd(151)));
    let (shown, other) = under(&mut h, css, |h| {
        (inks_in(h, h.get_by_label("Recent projects").rect()), inks_in(h, h.get_by_label("Templates projects").rect()))
    });
    assert!(!shown.contains(&odd(151)), "the shown tab keeps its ink: {shown:?}");
    assert!(other.contains(&odd(151)), "the other tab takes the rule: {other:?}");
}

/// A rule for every `theme-card` does not take the edge of the theme in use: its 2-point edge
/// keeps its own colour (#1440 review).
#[test]
fn a_rule_for_every_theme_card_keeps_the_theme_in_use_marked() {
    fn edges(shape: &egui::Shape, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| edges(shape, found)),
            egui::Shape::Rect(rect) if rect.stroke.width >= 2.0 => found.push(rect.stroke.color),
            _ => {}
        }
    }
    let mut h = window(demo_app());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_by_label("Settings page: Themes").click();
    h.run();
    let css = format!("theme-card {{ border-color: {}; }}", hex(odd(152)));
    let found = under(&mut h, css, |h| {
        let mut found = Vec::new();
        h.output().shapes.iter().for_each(|shape| edges(&shape.shape, &mut found));
        found
    });
    assert!(!found.is_empty(), "the theme in use has its edge");
    assert!(!found.contains(&odd(152)), "the edge stays its own: {found:?}");
}

/// Frames of a window holding one bare egui widget, outside any style scope, under the user sheet
/// `css`; `show` draws it.
fn bare_widgets(css: &str, mut show: impl FnMut(&mut egui::Ui) + 'static) -> Harness<'static> {
    suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
    let mut h = Harness::builder().allow_missing_glyphs().with_size((400.0, 300.0)).build_ui(move |ui| {
        let _frame = suhina_ui::kit::native_css::frame(ui);
        show(ui);
    });
    theme::install(&h.ctx);
    h.run_steps(4);
    h
}

/// How many fills of shapes painted in `h`'s last frame are `color`.
fn filled_with<S>(h: &Harness<'_, S>, color: egui::Color32) -> usize {
    fn walk(shape: &egui::Shape, color: egui::Color32) -> usize {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().map(|shape| walk(shape, color)).sum(),
            egui::Shape::Rect(rect) => usize::from(rect.fill == color),
            _ => 0,
        }
    }
    h.output().shapes.iter().map(|shape| walk(&shape.shape, color)).sum()
}

/// egui's own text field, outside any scope an element opens, takes the sheet's `text-input` rule:
/// the `Visuals` at the root are built from it (#1442).
#[test]
fn a_text_field_outside_any_scope_takes_the_text_input_rule() {
    let mut text = String::new();
    let css = format!("text-input {{ background-color: {}; }}", hex(odd(51)));
    let h = bare_widgets(&css, move |ui| {
        let field = ui.text_edit_singleline(&mut text);
        field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "A field"));
    });
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert_eq!(h.ctx.global_style().visuals.text_edit_bg_color, Some(odd(51)));
    assert!(filled_with(&h, odd(51)) > 0, "the field is filled with the rule's colour");
}

/// An open egui menu, which egui frames with the root's window fill, takes the sheet's `menu` rule
/// (#1442): a menu button's menu, and the File menu of the application bar.
#[test]
fn an_open_egui_menu_takes_the_menu_rule_at_the_root() {
    let css = format!("menu {{ background-color: {}; }}", hex(odd(52)));
    let mut h = bare_widgets(&css, |ui| {
        ui.menu_button("Open", |ui| {
            ui.label("An entry");
        });
    });
    h.get_by_label("Open").click();
    h.run_steps(3);
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert_eq!(h.ctx.global_style().visuals.window_fill, odd(52));
    assert!(h.query_by_label("An entry").is_some(), "the menu is open");
    assert!(filled_with(&h, odd(52)) > 0, "its frame is filled with the rule's colour");

    let mut window = window(demo_app());
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    window.run_steps(4);
    window.get_by_label("File").click();
    window.run_steps(3);
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert_eq!(window.ctx.global_style().visuals.window_fill, odd(52));
    assert!(filled_with(&window, odd(52)) > 0, "the File menu is filled with the rule's colour");
}

/// The window with the Keys track's device chain shown and its first device carried to the end of
/// the chain, so that the chain's insert line (`drop-target[kind="insert"]`) is drawn; in the
/// theme `theme` when one is given (#1448).
#[allow(clippy::unwrap_used)] // A test's helper: the demo project has the Keys track.
fn carrying_a_device(theme: Option<suhina_ui::kit::theme::Theme>) -> Harness<'static, App> {
    let mut session = suhina_session::Session::new(demo_project());
    for id in ["utility", "utility"] {
        session.execute("device.add", &serde_json::json!({ "track": "Keys", "id": id })).unwrap();
    }
    let track = session.project().tracks.iter().find(|t| t.name == "Keys").map(|t| t.id).unwrap();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut h = window(app);
    if let Some(theme) = theme {
        let ctx = h.ctx.clone();
        let entry = suhina_ui::settings::themes::ThemeEntry { id: "under-test".into(), theme };
        h.state_mut().set_themes(&ctx, vec![entry], None);
        h.state_mut().use_theme(&ctx, "under-test");
    }
    h.get_by_label("Track Keys").click();
    h.run();
    let pointer = egui::pos2(h.ctx.content_rect().right() - 300.0, h.ctx.content_rect().bottom() - 100.0);
    h.ctx.data_mut(|d| d.insert_temp(egui::Id::unique(suhina_ui::interaction::drop::DEVICE_CARRYING), (track, 0usize)));
    h.event(egui::Event::PointerMoved(pointer));
    for _ in 0..4 {
        h.step();
    }
    h
}

/// The device being carried is put down again.
fn put_down(h: &mut Harness<'static, App>) {
    h.ctx.data_mut(|d| {
        d.remove::<(suhina_engine::model::TrackId, usize)>(egui::Id::unique(
            suhina_ui::interaction::drop::DEVICE_CARRYING,
        ))
    });
}

/// A drop target's line takes its colour from the base sheet's rule, which reads `--accent`: a
/// `user.css` that sets the variable reaches it in Suhina Studio, with no rule for the line (#1448).
#[test]
fn a_user_sheet_s_accent_reaches_a_drop_target_line_in_suhina_studio() {
    let mut h = carrying_a_device(None);
    assert_eq!(h.state().settings().theme, suhina_ui::kit::theme::BUILT_IN);
    let missing = missing(&mut h, &[(":root", "--accent", odd(130))]);
    put_down(&mut h);
    assert_eq!(missing, Vec::<String>::new());
}

/// A theme's rule beats the base sheet's for the same element, also a less specific one and one
/// inside `@layer`: `drop-target` over the base sheet's `drop-target[kind="insert"]` (#1448).
#[test]
fn a_theme_rule_beats_the_base_sheet_s_less_specific_or_layered() {
    for css in [
        format!("drop-target {{ color: {}; }}", hex(odd(131))),
        format!("@layer mine {{ drop-target {{ color: {}; }} }}", hex(odd(131))),
    ] {
        let (theme, warnings) = suhina_ui::kit::theme::from_css(&css);
        assert!(warnings.is_empty(), "{warnings:?}");
        let mut h = carrying_a_device(Some(theme));
        let shown = painted(&h);
        put_down(&mut h);
        assert!(shown.contains(&odd(131)), "{css}: the theme's colour is the line's");
    }
}

/// Colouring a region goes through its variables: `mixer { --text-value: … }` reaches a field in
/// the mixer, its volume's readout (#1448).
#[test]
fn a_mixer_s_text_value_variable_reaches_a_field_in_the_mixer() {
    let mut h = window(demo_app());
    assert_eq!(missing(&mut h, &[("mixer", "--text-value", odd(132))]), Vec::<String>::new());
}

/// A theme's rule beats each rule the base sheet gives the shared controls, whatever its
/// specificity: one rule for each element the sheet gives colours to, in a window that shows it,
/// and its colour is painted (#1449). A fader's cap and a meter's level are not here: what they
/// paint is shades worked out from the colour, not the colour, which the style tests read.
#[test]
fn a_theme_rule_beats_the_base_sheet_s_rules_for_the_shared_controls() {
    fn audio_clip() -> Harness<'static, App> {
        let mut h = window_with_step(audio_app(), 1.0 / 60.0);
        h.get_by_label("Arrangement view").click();
        h.run();
        h.get_by_label("Clip Loop on Loop at bar 1").click();
        h.run();
        h
    }
    let mut dialog = window(demo_app());
    dialog.state_mut().open_project_dialog(false);
    dialog.run();
    let mut list = window(demo_app());
    list.key_press_modifiers(Modifiers::COMMAND, Key::P);
    list.run();
    type Scene = (&'static str, Harness<'static, App>, Vec<(&'static str, &'static str)>);
    let scenes: Vec<Scene> = vec![
        (
            "mixer",
            lit_drums(),
            vec![
                ("button", "background-color"),
                ("button[kind=\"solo\"][checked]", "background-color"),
                ("button[kind=\"arm\"][checked]", "background-color"),
                ("label[kind=\"hint\"]", "color"),
                ("label[kind=\"caption\"]", "color"),
                ("label", "color"),
                ("button", "color"),
                ("value", "color"),
                ("value-fill", "background-color"),
                ("pan", "background-color"),
                ("send", "background-color"),
                ("dropdown", "color"),
                ("dropdown", "border-color"),
            ],
        ),
        (
            "arrangement",
            arrangement_app(),
            vec![
                ("value[kind=\"bar\"]", "background-color"),
                ("slider-rail", "background-color"),
                ("slider-fill", "background-color"),
                ("slider-handle", "background-color"),
            ],
        ),
        (
            "clip panel",
            walk_picked(),
            vec![
                ("toggle", "background-color"),
                ("button[kind=\"toggle\"][checked]", "background-color"),
                ("dropdown-arrow", "color"),
            ],
        ),
        ("dialog", dialog, vec![("dialog", "border-color"), ("text-input", "border-color")]),
        ("list", list, vec![("list-row", "color"), ("list-row[selected]", "color")]),
        (
            "audio clip",
            audio_clip(),
            vec![
                ("knob", "background-color"),
                ("knob-ring", "background-color"),
                ("label[kind=\"value\"]", "color"),
                ("toggle[checked]", "background-color"),
            ],
        ),
    ];
    let mut shade = 140u8;
    for (name, mut h, rules) in scenes {
        let rules: Vec<_> = rules
            .into_iter()
            .map(|(selector, property)| {
                shade += 1;
                (selector, property, odd(shade))
            })
            .collect();
        assert_eq!(missing(&mut h, &rules), Vec::<String>::new(), "{name}");
    }
    // A typed number: its ring, its fill and its text.
    let mut h = audio_clip();
    h.get_by_label_contains("Pitch: ").click();
    h.run();
    h.event(egui::Event::Text("3".into()));
    h.run();
    let typed = [
        ("text-input[kind=\"typed\"]", "border-color", odd(shade + 1)),
        ("text-input[kind=\"typed\"]", "background-color", odd(shade + 2)),
        ("text-input[kind=\"typed\"]", "color", odd(shade + 3)),
    ];
    assert_eq!(missing(&mut h, &typed), Vec::<String>::new(), "a typed number");
}

/// A built-in theme with a rule for every `button` still shows which track buttons are on: Suhina
/// Meadow, Blocks and Sheet and the Dark Gloss example state the lit look their rule for every
/// button would hide, and the text on a lit button reads on its fill (#1449).
#[test]
fn a_built_in_theme_with_a_rule_for_every_button_still_shows_its_lit_buttons() {
    let rgb = egui::Color32::from_rgb;
    for (id, example, expected_ink) in [
        ("suhina-meadow", None, Some(rgb(0x18, 0x2a, 0x44))),
        ("suhina-blocks", None, Some(rgb(0xee, 0xe1, 0xc0))),
        ("suhina-sheet", None, Some(rgb(0x28, 0x5c, 0x3d))),
        ("suhina-studio", Some(suhina_ui::kit::theme::GLOSS_EXAMPLE), None),
    ] {
        let mut h = lit_drums();
        let ctx = h.ctx.clone();
        h.state_mut().use_theme(&ctx, id);
        suhina_ui::kit::native_css::set_user_sheet(example.map(|css| css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        for (lit, unlit) in [
            ("End solo on Drums", "Solo Keys"),
            ("Stop cueing Drums", "Cue Keys on the headphones"),
            ("Disarm Drums", "Arm Keys for recording"),
        ] {
            // What is painted exactly over the button: a fill, or the artwork a fill is drawn in.
            let faces = |label: &str| {
                let rect = h.get_by_label(label).rect();
                let mut found = Vec::new();
                for shape in &h.output().shapes {
                    if let egui::Shape::Rect(shape) = &shape.shape
                        && (shape.rect.min - rect.min).length() < 0.5
                        && (shape.rect.max - rect.max).length() < 0.5
                    {
                        found.push(format!(
                            "{:?} {:?}",
                            shape.fill,
                            shape.brush.as_ref().map(|brush| brush.fill_texture_id)
                        ));
                    }
                }
                found
            };
            let (lit_faces, unlit_faces) = (faces(lit), faces(unlit));
            assert!(!lit_faces.is_empty() && !unlit_faces.is_empty(), "{id}: both are painted");
            assert_ne!(lit_faces, unlit_faces, "{id}: {lit} shows it is on");
        }
        // The mixer's switch is lit while its track is on: the ink of its number. Gloss's reads on
        // the lit fill; the three sheets' are the button's own, as before the base sheet (the
        // recorded painted digests hold them), which this test pins so a change is a decision.
        let switch = h.get_by_label("Turn Drums off").rect();
        let fill = suhina_ui::kit::theme::control_selected();
        let inks = text_colours_at(&h, switch);
        suhina_ui::kit::native_css::set_user_sheet(None);
        assert!(!inks.is_empty(), "{id}: the lit switch shows its number");
        match expected_ink {
            Some(ink) => assert!(inks.iter().all(|c| *c == ink), "{id}: the lit switch's ink is {ink:?}: {inks:?}"),
            None => {
                for ink in inks {
                    let contrast = suhina_ui::kit::theme::text_contrast(ink, fill);
                    assert!(contrast >= 3.0, "{id}: the lit switch's text {ink:?} on {fill:?} reads at {contrast}");
                }
            }
        }
    }
}

/// A built-in theme with a rule for every `button` still shows which of a mixer strip's monitor
/// picks is chosen: Suhina Meadow, Blocks and Sheet and the Dark Gloss example state the lit look
/// of `button[kind="monitor"]` that their rule for every button would hide (#1450).
#[test]
fn a_built_in_theme_with_a_rule_for_every_button_still_shows_the_chosen_monitor_pick() {
    for (id, example) in [
        ("suhina-meadow", None),
        ("suhina-blocks", None),
        ("suhina-sheet", None),
        ("suhina-studio", Some(suhina_ui::kit::theme::GLOSS_EXAMPLE)),
    ] {
        let mut project = demo_project();
        project.tracks[0].content = suhina_engine::model::ContentKinds { notes: false, audio: true };
        let track = project.tracks[0].name.clone();
        let mut app = App::new();
        app.set_browser_open(false);
        app.set_session(Session::new(project), &[]);
        let mut settings = suhina_ui::settings::Settings::default();
        settings.view.mixer_height = 320;
        app.set_settings(&egui::Context::default(), settings);
        let mut h = window(app);
        let ctx = h.ctx.clone();
        h.state_mut().use_theme(&ctx, id);
        suhina_ui::kit::native_css::set_user_sheet(example.map(|css| css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        // The fills painted exactly over each pick.
        let fills = |word: &str| {
            let rect = h.get_by_label(&format!("{track} monitor {word}")).rect();
            let mut found = Vec::new();
            for shape in &h.output().shapes {
                if let egui::Shape::Rect(shape) = &shape.shape
                    && (shape.rect.min - rect.min).length() < 0.5
                    && (shape.rect.max - rect.max).length() < 0.5
                {
                    found.push(shape.fill);
                }
            }
            found
        };
        let (auto, off) = (fills("Auto"), fills("Off"));
        suhina_ui::kit::native_css::set_user_sheet(None);
        assert!(!auto.is_empty() && !off.is_empty(), "{id}: both are painted");
        assert_ne!(auto, off, "{id}: the chosen pick (Auto) shows it is chosen");
    }
}

/// Colouring a region goes through its variables: `window { --text-hint: … }` reaches the hint
/// words inside it (an empty detail panel's note) and a switch's dim words, which the base sheet
/// colours with the same variable: a track that is off, its number (#1449).
#[test]
fn a_region_s_text_hint_variable_reaches_the_hints_and_toggles_inside_it() {
    let mut h = lit_drums();
    h.get_by_label("Turn Keys off").click();
    h.run();
    // The pointer leaves it: under the pointer a switch's words are the brighter ones.
    h.event(egui::Event::PointerGone);
    h.run();
    let switch = h.get_by_label("Turn Keys on").rect();
    let css = format!("window {{ --text-hint: {}; }}", hex(odd(151)));
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let toggle = text_colours_at(&h, switch);
    // The colours of the hint's words: the empty detail panel's note.
    fn colours_of(shape: &egui::Shape, words: &str, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| colours_of(shape, words, found)),
            egui::Shape::Text(text) if text.galley.text() == words => {
                found.extend(text.galley.job.sections.iter().map(|section| section.format.color));
            }
            _ => {}
        }
    }
    let mut hint = Vec::new();
    h.output()
        .shapes
        .iter()
        .for_each(|shape| colours_of(&shape.shape, "Select a track or a clip to see it here", &mut hint));
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(!toggle.is_empty() && toggle.iter().all(|c| *c == odd(151)), "the off switch's number: {toggle:?}");
    assert!(!hint.is_empty() && hint.iter().all(|c| *c == odd(151)), "the hint's words: {hint:?}");
}

/// The Launcher with a mixer tall enough for its strips' device lists.
fn tall_mixer() -> Harness<'static, App> {
    let mut app = demo_app();
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.mixer_height = 320;
    app.set_settings(&egui::Context::default(), settings);
    window(app)
}

/// A theme's rule for each element the base sheet colours in the Launcher, the mixer, the control
/// bar and the Arrangement (#1450) is painted: a rule beats the base sheet's, whatever it says.
/// Each rule has a colour of its own, so a rule that reaches nothing is named.
#[test]
fn a_theme_rule_beats_the_base_sheet_s_rules_for_the_launcher_the_mixer_and_the_arrangement() {
    type Scene = (&'static str, Harness<'static, App>, Vec<(&'static str, &'static str)>);
    let scenes: Vec<Scene> = vec![
        (
            "launcher",
            window(demo_app()),
            vec![
                ("scene", "background-color"),
                ("scene", "color"),
                ("scene[kind=\"stop-all\"]", "background-color"),
                ("scene[kind=\"stop-all\"]", "color"),
                ("slot[kind=\"stop\"]", "background-color"),
                ("slot[kind=\"stop\"]", "color"),
                ("session slot", "background-color"),
                ("label[kind=\"scene-number\"]", "color"),
                ("track-header[kind=\"main\"]", "color"),
                ("button[kind=\"metronome\"]", "color"),
            ],
        ),
        (
            "mixer",
            lit_drums(),
            vec![
                ("mixer", "background-color"),
                ("fader", "color"),
                ("fader-scale", "color"),
                ("send", "color"),
                ("slot-mark", "color"),
            ],
        ),
        ("device lists", tall_mixer(), vec![("device-list", "background-color")]),
        (
            "arrangement",
            {
                // Zoomed in with a finer grid, so the lines between bars are drawn too.
                let mut h = arrangement_app();
                zoom_in_at_beat_b(&mut h);
                h.get_by_label("Lane Drums").click();
                h.run();
                h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Num1);
                h.run();
                h
            },
            vec![
                ("arrangement[kind=\"panel\"]", "background-color"),
                ("ruler", "background-color"),
                ("grid-line[kind=\"beat\"]", "color"),
                ("slider#waveform-zoom", "color"),
                ("button[kind=\"lanes\"]", "color"),
                ("arrangement track-header[kind=\"main\"]", "color"),
            ],
        ),
    ];
    let mut shade = 160u8;
    for (name, mut h, rules) in scenes {
        let rules: Vec<_> = rules
            .into_iter()
            .map(|(selector, property)| {
                shade += 1;
                (selector, property, odd(shade))
            })
            .collect();
        assert_eq!(missing(&mut h, &rules), Vec::<String>::new(), "{name}");
    }
}

/// Colouring a region goes through its variables in the Launcher and the mixer too: a scene's
/// number, a fader's value and a pan field's value take the variables of the region they are in
/// (#1450).
#[test]
fn a_region_s_variables_reach_the_launcher_s_and_the_mixer_s_parts() {
    let mut h = window(demo_app());
    let css = format!(
        "session {{ --text-hint: {}; }} mixer {{ --text-value: {}; --text-primary: {}; }}",
        hex(odd(181)),
        hex(odd(182)),
        hex(odd(183))
    );
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let shown = painted(&h);
    suhina_ui::kit::native_css::set_user_sheet(None);
    for (what, colour) in
        [("the scene numbers", odd(181)), ("a fader's value", odd(182)), ("a pan field's value", odd(183))]
    {
        assert!(shown.contains(&colour), "{what} take their region's variable");
    }
}

/// Keys with a Utility, grouped when `group`, its panel shown in the detail area.
fn keys_with_utility(group: bool) -> Harness<'static, App> {
    let mut session = suhina_session::Session::new(demo_project());
    assert!(session.execute("device.add", &serde_json::json!({ "track": "Keys", "id": "utility" })).is_ok());
    if group {
        assert!(session.execute("device.group", &serde_json::json!({ "track": "Keys", "device": 0 })).is_ok());
    }
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut h = window(app);
    h.get_by_label("Track Keys").click();
    h.run();
    h
}

/// The browser open on Recent, its kinds as chips, Recent chosen: its "All" chip the lit one.
fn browser_with_chips() -> Harness<'static, App> {
    let mut app = demo_app();
    app.set_browser_open(true);
    let mut h = window(app);
    let ctx = h.ctx.clone();
    let mut chips = h.state().settings().clone();
    chips.view.recent_chips = true;
    h.state_mut().set_settings(&ctx, chips);
    h.get_by_label("Place Recent").click();
    h.run();
    h
}

/// A theme's rule beats each rule the base sheet gives the detail panels, the piano roll and the
/// browser, whatever its specificity: one rule for each element the sheet gives colours to, in a
/// window that shows it, and its colour is painted (#1451).
#[test]
fn a_theme_rule_beats_the_base_sheet_s_rules_for_the_detail_area_and_the_browser() {
    type Scene = (&'static str, Harness<'static, App>, Vec<(&'static str, &'static str)>);
    let scenes: Vec<Scene> = vec![
        (
            "a MIDI clip and its piano roll",
            walk_picked(),
            vec![
                ("detail-fill", "background-color"),
                ("value[kind=\"field\"]", "background-color"),
                ("value[kind=\"field\"]", "color"),
                ("editor-tab", "background-color"),
                ("piano-grid", "border-color"),
                ("ruler-number", "color"),
                ("separator[kind=\"bar\"]", "color"),
                ("piano-tool", "color"),
                ("bar-item", "color"),
                ("loop-region", "background-color"),
                ("start-marker", "color"),
            ],
        ),
        (
            "a device",
            keys_with_utility(false),
            vec![("device-header", "background-color"), ("device-header", "color"), ("device-control", "color")],
        ),
        (
            "a device group",
            keys_with_utility(true),
            vec![("device-row", "color"), ("separator[kind=\"group\"]", "color")],
        ),
        (
            "the browser",
            browser_with_chips(),
            vec![
                ("browser-fill", "background-color"),
                ("label[kind=\"caption\"]", "color"),
                ("button", "color"),
                ("button[checked]", "background-color"),
            ],
        ),
    ];
    let mut shade = 200u8;
    for (name, mut h, rules) in scenes {
        let rules: Vec<_> = rules
            .into_iter()
            .map(|(selector, property)| {
                shade += 1;
                (selector, property, odd(shade))
            })
            .collect();
        assert_eq!(missing(&mut h, &rules), Vec::<String>::new(), "{name}");
    }
}

/// A built-in theme with a rule for every `button` still shows which of the browser's chips is
/// chosen: Suhina Meadow, Blocks and Sheet and the Dark Gloss example state the chip's lit fill
/// their rule for every button would hide (#1451).
#[test]
fn a_built_in_theme_with_a_rule_for_every_button_still_shows_the_chosen_chip() {
    for (id, example) in [
        ("suhina-meadow", None),
        ("suhina-blocks", None),
        ("suhina-sheet", None),
        ("suhina-studio", Some(suhina_ui::kit::theme::GLOSS_EXAMPLE)),
    ] {
        let mut h = browser_with_chips();
        let ctx = h.ctx.clone();
        h.state_mut().use_theme(&ctx, id);
        suhina_ui::kit::native_css::set_user_sheet(example.map(|css| css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        // What is painted exactly over the chip: a fill, or the artwork a fill is drawn in.
        let faces = |label: &str| {
            let rect = h.get_by_label(label).rect();
            let mut found = Vec::new();
            for shape in &h.output().shapes {
                if let egui::Shape::Rect(shape) = &shape.shape
                    && (shape.rect.min - rect.min).length() < 0.5
                    && (shape.rect.max - rect.max).length() < 0.5
                {
                    found.push(format!(
                        "{:?} {:?}",
                        shape.fill,
                        shape.brush.as_ref().map(|brush| brush.fill_texture_id)
                    ));
                }
            }
            found
        };
        let (lit, unlit) = (faces("Recent All"), faces("Recent Samples"));
        suhina_ui::kit::native_css::set_user_sheet(None);
        assert!(!lit.is_empty() && !unlit.is_empty(), "{id}: both are painted");
        assert_ne!(lit, unlit, "{id}: the chosen chip shows it is chosen");
    }
}

/// The colours of the words `words` painted in the last frame.
fn colours_of_words(h: &Harness<'static, App>, words: &str) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, words: &str, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, words, found)),
            egui::Shape::Text(text) if text.galley.text() == words => {
                found.extend(text.galley.job.sections.iter().map(|section| section.format.color));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, words, &mut found));
    found
}

/// Shows `h` with `css` as the user's sheet, then the colours of the words `words`.
fn words_under(h: &mut Harness<'static, App>, css: String, words: &str) -> Vec<egui::Color32> {
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let found = colours_of_words(h, words);
    suhina_ui::kit::native_css::set_user_sheet(None);
    found
}

/// Colouring a region goes through its variables: `--text-value` given to the piano roll reaches
/// its ruler's bar numbers, given to a device's panel its values, which the base sheet
/// colours with that variable; `--browser-text-secondary` given to the browser its secondary words
/// (#1451).
#[test]
fn a_region_s_variables_reach_the_ruler_s_numbers_a_device_s_values_and_the_browser_s_words() {
    let mut h = walk_picked();
    let numbers = words_under(&mut h, format!("piano-roll {{ --text-value: {}; }}", hex(odd(221))), "2");
    assert!(numbers.contains(&odd(221)), "the ruler's bar numbers: {numbers:?}");
    let mut h = keys_with_utility(false);
    let values = words_under(&mut h, format!("device {{ --text-value: {}; }}", hex(odd(222))), "0.00 dB");
    assert!(!values.is_empty() && values.iter().all(|c| *c == odd(222)), "a device's value: {values:?}");
    let mut h = browser_with_chips();
    assert_eq!(missing(&mut h, &[("browser", "--browser-text-secondary", odd(223))]), Vec::<String>::new());
}
