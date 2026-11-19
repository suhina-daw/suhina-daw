// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The diagnostics for theme authors (#1582): `--probe`, `--coverage` and `--outline`. Headless.

use egui::{Id, LayerId, Order, Rect, pos2};
use suhina_theme_render::{Subject, inspect, outline, scenes};
use suhina_ui::kit::native_css::inspector::{self, FrameElement};

fn studio() -> Option<Subject> {
    Subject::built_in("suhina-studio")
}

fn element(name: &'static str, parent: Option<usize>, rect: Option<(Rect, LayerId)>) -> FrameElement {
    FrameElement { name, id: None, attributes: Vec::new(), states: Vec::new(), parent, painted: rect }
}

#[test]
fn a_probe_names_the_control_at_a_point_its_path_and_the_rules_behind_it() {
    let scene = scenes::find("launcher").unwrap();
    let h = inspect::settled(&studio().unwrap(), &scene).unwrap();
    let elements = inspector::frame_elements();
    let (index, browser) = elements
        .iter()
        .enumerate()
        .find(|(_, e)| e.name == "toolbar-module" && e.id.as_deref() == Some("browser"))
        .expect("the control bar's browser button");
    let (rect, _) = browser.painted.expect("it painted");
    assert_eq!(
        inspect::hit(&elements, &h.ctx.memory(|m| m.layer_ids().collect::<Vec<_>>()), rect.center()),
        Some(index)
    );
    let report = inspect::probe(&h, rect.center()).unwrap();
    let first = report.lines().next().unwrap();
    assert!(first.starts_with("toolbar-module#browser"), "{report}");
    assert!(report.contains("toolbar > group#toolbar-start"), "{report}");
    // The user-agent sheet's rule for this button, with what became of its declaration.
    assert!(report.contains("Suhina's user-agent sheet line"), "{report}");
    assert!(report.contains("(applies"), "{report}");
    assert!(inspect::probe(&h, pos2(-50.0, -50.0)).unwrap_err().contains("no element"));
}

#[test]
fn the_probe_works_out_the_style_each_element_was_painted_with() {
    // Its explanation comes from the frame's tree and sheets: in a still scene it has to agree
    // with what each element was painted with, or the probe would explain another style.
    for name in ["launcher", "menu", "settings"] {
        let scene = scenes::find(name).unwrap();
        let _h = inspect::settled(&studio().unwrap(), &scene).unwrap();
        let elements = inspector::frame_elements();
        assert!(elements.len() > 20, "{name}: {} elements", elements.len());
        for (index, element) in elements.iter().enumerate() {
            let explained = inspector::explain(index).unwrap();
            assert!(!explained.painted_differs, "{name}: {} was painted with another style", element.selector());
        }
    }
}

#[test]
fn the_topmost_layer_wins_a_hit_and_on_one_layer_the_last_entered_element() {
    let view = LayerId::background();
    let menu = LayerId::new(Order::Foreground, Id::NULL.with("menu"));
    let all = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
    let small = Rect::from_min_size(pos2(10.0, 10.0), egui::vec2(20.0, 20.0));
    let elements = [
        element("window", None, Some((all, view))),
        element("popup", None, Some((small, menu))),
        element("arrangement", Some(0), Some((all, view))),
        element("clip", Some(2), Some((small, view))),
        element("track-name", Some(0), None),
    ];
    let layers = [view, menu];
    // The menu is over the clip, though the clip was entered after it.
    assert_eq!(inspect::hit(&elements, &layers, pos2(15.0, 15.0)), Some(1));
    assert_eq!(inspect::hit(&elements, &layers, pos2(50.0, 50.0)), Some(2));
    assert_eq!(inspect::hit(&elements, &layers, pos2(500.0, 50.0)), None);
    assert_eq!(inspect::path(&elements, 3), ["window", "arrangement", "clip"]);
    // A foreground layer that is no area of the window's (a drag's preview) is over the view all
    // the same.
    let drag = LayerId::new(Order::Foreground, Id::NULL.with("drag"));
    let dragged = [element("clip", None, Some((small, drag))), element("arrangement", None, Some((all, view)))];
    assert_eq!(inspect::hit(&dragged, &layers, pos2(15.0, 15.0)), Some(0));
    // An attribute's value is written as a CSS string.
    let mut named = element("clip", None, None);
    named.attributes.push(("color".into(), "a\"b\\c\t".into()));
    assert_eq!(named.selector(), r#"clip[color="a\"b\\c\9 "]"#);
}

#[test]
fn coverage_lists_the_theme_rules_no_element_matched() {
    let folder = std::env::temp_dir().join(format!("suhina-coverage-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("theme.json"), r##"{ "format": 1, "name": "Coverage test", "swatch": "#334455" }"##)
        .unwrap();
    std::fs::write(
        folder.join("theme.css"),
        "toolbar { color: #eeeeee; }\nbutton#no-such-control { color: red; }\n\
         tooltip { background-color: #102030; }\nfader.cap { image: url(\"suhina:fader.cap.svg\"); }\n\
         knob-body { background-color: red; }\n",
    )
    .unwrap();
    let (subject, _) = Subject::folder(&folder).unwrap();
    let driven = [scenes::find("launcher").unwrap(), scenes::find("tooltip").unwrap()];
    let found = inspect::coverage(&subject, &driven).unwrap();
    std::fs::remove_dir_all(&folder).ok();
    assert!(found.failed.is_empty(), "{:?}", found.failed);
    assert_eq!(found.driven, 2);
    let listed: Vec<_> =
        found.unused.iter().map(|rule| (rule.line, rule.selector.as_str(), rule.artwork.as_deref())).collect();
    // The tooltip's frame is read before it is drawn, outside the frame's tree, and counts (in the
    // tooltip scene: the launcher shows none); an artwork rule is told apart.
    // A rule for an artwork role's element that sets no `image` is no artwork rule.
    assert_eq!(
        listed,
        [(2, "button#no-such-control", None), (4, "fader.cap", Some("fader-cap")), (5, "knob-body", None)]
    );
}

#[test]
fn the_outline_draws_boxes_and_names_and_skips_elements_without_one() {
    let blank = image::RgbaImage::from_pixel(200, 100, image::Rgba([20, 20, 20, 255]));
    let layer = LayerId::background();
    let boxed = Rect::from_min_size(pos2(10.0, 10.0), egui::vec2(80.0, 30.0));
    assert_eq!(outline::draw(&blank, &[element("clip", None, None)], &[layer], 1.0).unwrap(), blank);
    let drawn = outline::draw(&blank, &[element("clip", None, Some((boxed, layer)))], &[layer], 2.0).unwrap();
    assert_ne!(drawn, blank);
    // A menu's box is drawn over the view's, whichever was entered first.
    let menu = LayerId::new(Order::Foreground, Id::NULL.with("menu"));
    let both = [element("popup", None, Some((boxed, menu))), element("clip", None, Some((boxed, layer)))];
    let over = outline::draw(&blank, &both, &[layer, menu], 2.0).unwrap();
    let popup_only = outline::draw(&blank, &both[..1], &[layer, menu], 2.0).unwrap();
    assert_eq!(over.get_pixel(20, 20), popup_only.get_pixel(20, 20));
    assert_ne!(drawn, blank);
    // At scale 2 the box's corner is at (20, 20) and its name's plate inside it.
    assert_ne!(drawn.get_pixel(20, 20), blank.get_pixel(20, 20));
    assert_ne!(drawn.get_pixel(24, 24), blank.get_pixel(24, 24));
    assert_eq!(drawn.get_pixel(5, 5), blank.get_pixel(5, 5));
}
