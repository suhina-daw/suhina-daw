// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{
    cascade::{self, AppliedSheet, Layer},
    media::Viewport,
    selector::Element,
    sheet::Sheet,
};
fn layout(source: &str, elements: Vec<Element>, width: f32) -> Vec<Option<Rect>> {
    measured(source, elements, width, &[])
}
fn measured(source: &str, elements: Vec<Element>, width: f32, sizes: &[Option<[Option<f32>; 2]>]) -> Vec<Option<Rect>> {
    let sheet = Sheet::parse(source).unwrap();
    assert!(sheet.notices.is_empty(), "{:?}", sheet.notices);
    let tree = Tree::new(elements).unwrap();
    let styles = cascade::compute(
        &tree,
        Viewport { width, height: 600.0, reduced_motion: false },
        &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }],
    )
    .unwrap();
    compute(&tree, &styles, [width, 600.0], sizes).unwrap()
}
#[test]
fn an_area_the_template_does_not_name_leaves_the_element_out() {
    let css = "window { display: grid; grid-template-rows: auto minmax(0, 1fr); grid-template-areas: 'bar' 'main'; } #bar { grid-area: bar; height: 20px; } #lost { grid-area: nowhere; height: 30px; } #main { grid-area: main; }";
    let tree = vec![
        Element::new("window", None),
        Element::new("pane", Some(0)).with_id("bar"),
        Element::new("pane", Some(0)).with_id("lost"),
        Element::new("pane", Some(0)).with_id("main"),
    ];
    let rects = layout(css, tree, 800.0);
    assert_eq!(rects[2], None, "no implicit track for a name the template lacks");
    // Without a template, or as `auto`, the name places nothing and hides nothing.
    let free = layout(
        "window { display: flex; } #a { grid-area: somewhere; height: 10px; } #b { grid-area: auto; height: 10px; }",
        vec![
            Element::new("window", None),
            Element::new("pane", Some(0)).with_id("a"),
            Element::new("pane", Some(0)).with_id("b"),
        ],
        800.0,
    );
    assert!(free[1].is_some() && free[2].is_some(), "{free:?}");
    let main = rects[3].unwrap();
    assert_eq!((main.y, main.height), (20.0, 580.0));
}
#[test]
fn a_content_size_is_the_content_box_and_a_sheet_size_wins() {
    let css = "window { display: grid; grid-template-columns: auto minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); } #bar { grid-column: 1 / -1; padding: 3px 0 2px; border-width: 0 0 1px; } #side { grid-row: 2; } #main { grid-row: 2; grid-column: 2; }";
    let tree = vec![
        Element::new("window", None),
        Element::new("pane", Some(0)).with_id("bar"),
        Element::new("pane", Some(0)).with_id("side"),
        Element::new("pane", Some(0)).with_id("main"),
    ];
    let sizes = [None, Some([None, Some(24.0)]), Some([Some(240.0), None]), None];
    let rects = measured(css, tree.clone(), 800.0, &sizes);
    // The bar's text height plus its padding and its border.
    assert_eq!(rects[1].unwrap().height, 30.0);
    assert_eq!(rects[2].unwrap().width, 240.0);
    assert_eq!(rects[3].unwrap().x, 240.0);
    let sized = format!("{css} #bar {{ height: 40px; }} #side {{ width: 100px; }}");
    let rects = measured(&sized, tree.clone(), 800.0, &sizes);
    assert_eq!(rects[1].unwrap().height, 40.0);
    assert_eq!(rects[2].unwrap().width, 100.0);
    // Hostile sizes are clamped, not laid out.
    let hostile = [None, Some([None, Some(f32::NAN)]), Some([Some(f32::INFINITY), None]), None];
    let rects = measured(css, tree, 800.0, &hostile);
    assert_eq!(rects[1].unwrap().height, 6.0);
    assert!(rects[2].unwrap().width <= 800.0);
}
#[test]
fn absent_panel_auto_track_collapses_and_resize_keeps_real_bounds() {
    let css = "window { layout: grid; columns: auto 1fr; rows: 1fr; grid-template-areas: 'browser main'; } #browser { grid-area: browser; width: 248px; } #main { grid-area: main; }";
    let tree = vec![Element::new("window", None), Element::new("pane", Some(0)).with_id("main")];
    for width in [1138.0, 800.0, 1.0, 0.0, 1138.0] {
        let rects = layout(css, tree.clone(), width);
        assert_eq!(rects[1].unwrap().width, width);
        assert_eq!(rects[1].unwrap().x, 0.0);
    }
    let mut open = tree;
    open.push(Element::new("pane", Some(0)).with_id("browser"));
    let rects = layout(css, open, 800.0);
    assert_eq!(rects[1].unwrap().x, 248.0);
    assert_eq!(rects[1].unwrap().width, 552.0);
}
#[test]
fn proportional_row_and_order_preserve_control_identity() {
    let elements = vec![
        Element::new("device-panel", None),
        Element::new("knob", Some(0)).with_id("cutoff"),
        Element::new("knob", Some(0)).with_id("resonance"),
    ];
    let rects = layout(
        "device-panel { layout: row; } #cutoff { width: 1fr; } #resonance { width: 4fr; order: -1; }",
        elements,
        500.0,
    );
    assert_eq!(rects[1].unwrap().width, 100.0);
    assert_eq!(rects[1].unwrap().x, 400.0);
    assert_eq!(rects[2].unwrap().width, 400.0);
    assert_eq!(rects[2].unwrap().x, 0.0);
}
#[test]
fn standard_grid_places_panels_by_lines_and_collapses_auto_tracks() {
    let css = "#workspace { display:grid; grid-template-columns:minmax(0, 1fr) auto; grid-template-rows:minmax(0, 1fr) auto; gap:8px; } #panel-browser { grid-column:2; grid-row:1 / -1; width:240px; } #panel-main { grid-column:1; grid-row:1; } #panel-detail { grid-column:1; grid-row:2; height:180px; }";
    let mut elements = vec![
        Element::new("div", None).with_id("workspace"),
        Element::new("div", Some(0)).with_id("panel-main"),
        Element::new("div", Some(0)).with_id("panel-browser"),
        Element::new("div", Some(0)).with_id("panel-detail"),
    ];
    let rects = layout(css, elements.clone(), 800.0);
    let (main, browser, detail) = (rects[1].unwrap(), rects[2].unwrap(), rects[3].unwrap());
    assert_eq!(browser.width, 240.0);
    assert_eq!(browser.x, 800.0 - 240.0);
    assert_eq!(browser.height, 600.0);
    assert_eq!(main.width, 800.0 - 240.0 - 8.0);
    assert_eq!(detail.height, 180.0);
    assert_eq!(detail.y, 600.0 - 180.0);
    assert_eq!(main.height, 600.0 - 180.0 - 8.0);
    // Closed panels leave no track behind.
    elements.truncate(2);
    let main = layout(css, elements, 800.0)[1].unwrap();
    assert!(main.width >= 790.0 && main.height >= 590.0, "{main:?}");
}

#[test]
fn a_border_takes_room_inside_the_box_as_padding_does() {
    let css = "toolbar { display:flex; padding:0 6px; border-width:1px 2px 3px; } #a { width:100px; height:20px; }";
    let elements = vec![Element::new("toolbar", None), Element::new("div", Some(0)).with_id("a")];
    let rects = layout(css, elements, 800.0);
    assert_eq!(rects[1].unwrap().x, 2.0 + 6.0, "the left border, then the padding");
    assert_eq!(rects[1].unwrap().y, 1.0, "the top border");
}

/// Padding in `em` lays a child out where `spacing_points`, which painters use, says the
/// content box starts (#1336).
#[test]
fn padding_in_em_is_where_painters_find_it() {
    let css = "toolbar { display:flex; font-size:10px; padding:0.5em 1.5em; } #a { width:100px; height:20px; }";
    let elements = vec![Element::new("toolbar", None), Element::new("div", Some(0)).with_id("a")];
    let rects = layout(css, elements, 800.0);
    assert_eq!((rects[1].unwrap().x, rects[1].unwrap().y), (15.0, 5.0));
    let sheet = Sheet::parse(css).unwrap();
    let tree = Tree::new(vec![Element::new("toolbar", None)]).unwrap();
    let viewport = Viewport { width: 800.0, height: 600.0, reduced_motion: false };
    let styles =
        cascade::compute(&tree, viewport, &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }]).unwrap();
    assert_eq!(spacing_points(&styles[0], Property::PaddingLeft), Some(15.0));
    assert_eq!(spacing_points(&styles[0], Property::PaddingTop), Some(5.0));
}

/// On a CSS layout container a padding in percent is taken of the box around it, as in CSS
/// (#1336): the sheet takes it there.
#[test]
fn padding_in_percent_places_a_layout_container_s_children() {
    let css = "toolbar { display:flex; width:400px; padding-left:10%; } #a { width:100px; height:20px; }";
    let elements = vec![Element::new("toolbar", None), Element::new("div", Some(0)).with_id("a")];
    assert_eq!(layout(css, elements, 800.0)[1].unwrap().x, 80.0, "10% of the 800 points around it");
}

/// Padding past 512 points: the layout and `spacing_points_of`, which painters use, give it
/// the same room (#1336).
#[test]
fn large_padding_is_where_painters_find_it() {
    let css = "toolbar { display:flex; padding-left:60%; padding-top:700px; } #a { width:10px; height:20px; }";
    let elements = vec![Element::new("toolbar", None), Element::new("div", Some(0)).with_id("a")];
    let rect = layout(css, elements, 1000.0)[1].unwrap();
    assert_eq!((rect.x, rect.y), (600.0, 700.0));
    let sheet = Sheet::parse(css).unwrap();
    let tree = Tree::new(vec![Element::new("toolbar", None)]).unwrap();
    let viewport = Viewport { width: 1000.0, height: 600.0, reduced_motion: false };
    let styles =
        cascade::compute(&tree, viewport, &[AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }]).unwrap();
    assert_eq!(spacing_points_of(&styles[0], Property::PaddingLeft, 1000.0), 600.0);
    assert_eq!(spacing_points_of(&styles[0], Property::PaddingTop, 1000.0), 700.0);
}

#[test]
fn standard_flex_row_with_auto_margin_padding_and_display_none() {
    let css = ".toolbar { display:flex; align-items:center; gap:6px; padding:0 6px; } #transport { margin-left:auto; } .module { width:100px; height:20px; flex-shrink:0; } @media (max-width:500px) { #mapping { display:none; } }";
    let module = |id: &str| {
        let mut element = Element::new("div", Some(0)).with_id(id);
        element.classes.push("module".into());
        element
    };
    let mut toolbar = Element::new("div", None);
    toolbar.classes.push("toolbar".into());
    let elements = vec![toolbar, module("position"), module("transport"), module("mapping")];
    let rects = layout(css, elements.clone(), 800.0);
    assert_eq!(rects[1].unwrap().x, 6.0);
    // The auto margin takes the free space: transport and mapping sit at the right edge.
    assert_eq!(rects[3].unwrap().x + 100.0, 800.0 - 6.0);
    assert_eq!(rects[2].unwrap().x + 100.0 + 6.0, rects[3].unwrap().x);
    assert_eq!(rects[2].unwrap().y, 290.0, "centred across the row");
    let narrow = layout(css, elements, 400.0);
    assert!(narrow[3].is_none(), "display:none in a media query hides it");
}

#[test]
fn track_lists_and_placements_are_bounded() {
    assert_eq!(track_list("repeat(3, 10px) minmax(0, 1fr)").unwrap().len(), 4);
    for bad in ["repeat(40, 1px)", "minmax(1fr, 2fr)", "repeat(2, repeat(2, 1px))", "-1px", "(", ""] {
        assert!(track_list(bad).is_err(), "{bad}");
    }
    assert_eq!(placement("1 / -1").unwrap(), [GridLine::Line(1), GridLine::Line(-1)]);
    assert_eq!(placement("span 2").unwrap(), [GridLine::Span(2), GridLine::Auto]);
    for bad in ["0", "1 / 2 / 3", "span 0", "999"] {
        assert!(placement(bad).is_err(), "{bad}");
    }
}

#[test]
fn disjoint_named_area_is_refused() {
    assert!(Areas::parse("'a b' 'b a'").is_err());
}
