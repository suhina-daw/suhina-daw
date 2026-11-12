// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bars lay out their items in one CSS row (#1372): a theme's `gap` and `padding` move the
//! items, `display: none` takes one away with its input, and a `flexible-space` takes the room
//! left. The toolbar's own tests are in `themed_toolbar_tests.rs`.

use super::*;

/// The window with a MIDI clip open in the piano roll, under the user sheet `css`.
fn with_clip(css: &str) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    let mut h = window(app);
    h.get_by_label("Launcher view").click();
    h.run();
    h.get_by_label("Select Beat B on Drums (scene 3)").click();
    h.run();
    h
}

/// The rectangle of the first label containing `label`, under `css`.
fn rect_of(css: &str, label: &str) -> Option<egui::Rect> {
    let h = with_clip(css);
    let rect = h.query_all_by_label_contains(label).next().map(|node| node.rect());
    suhina_ui::kit::native_css::set_user_sheet(None);
    rect
}

/// Where the painted text containing `text` lies, under `css`.
fn text_at(css: &str, text: &str) -> Option<egui::Rect> {
    let h = with_clip(css);
    let found = h.output().shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(shape) if shape.galley.text().contains(text) => Some(shape.visual_bounding_rect()),
        _ => None,
    });
    suhina_ui::kit::native_css::set_user_sheet(None);
    found
}

#[test]
fn the_project_tabs_follow_the_sheet() {
    let first = rect_of("", "Tab ").unwrap();
    let padded = rect_of("project-tabs { padding-left: 30px; }", "Tab ").unwrap();
    assert_eq!(padded.left(), first.left() + 30.0);
    let second = |css: &str| {
        let h = with_clip(css);
        let tabs: Vec<_> = h.query_all_by_label_contains("Tab ").map(|node| node.rect()).collect();
        suhina_ui::kit::native_css::set_user_sheet(None);
        tabs
    };
    let plain = second("");
    let spaced = second("project-tabs { gap: 11px; }");
    assert_eq!(spaced[1].left() - plain[1].left(), 10.0, "the gap was 1 point");
    // A hidden tab is gone with its close button; the other closes up.
    let hidden = second("project-tab[selected] { display: none; }");
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].left(), plain[0].left());
}

#[test]
fn the_piano_roll_toolbar_follows_the_sheet() {
    let select = rect_of("", "Select (").unwrap();
    let draw = rect_of("", "Draw (").unwrap();
    let padded = rect_of("piano-roll-toolbar { padding-left: 20px; }", "Select (").unwrap();
    assert_eq!(padded.left(), select.left() + 12.0, "20 points, not 8");
    let spaced = rect_of("piano-roll-toolbar { gap: 10px; }", "Draw (").unwrap();
    assert!((spaced.left() - draw.left() - 6.25).abs() < 0.01, "{spaced:?} {draw:?}");
    assert!(rect_of("#piano-erase { display: none; }", "Erase (").is_none(), "no input where it is hidden");
    // The flexible space puts the grid menu and the × at the right end; without it they follow
    // the Rows group.
    let grid = rect_of("", "Grid value").unwrap();
    let packed = rect_of("piano-roll-toolbar flexible-space { display: none; }", "Grid value").unwrap();
    assert!(packed.left() < grid.left() - 50.0, "{packed:?} {grid:?}");
}

#[test]
fn the_midi_tools_tabs_follow_the_sheet() {
    let open = |css: &str| {
        let h = with_clip(css);
        let rects =
            (h.query_by_label("Notes tab").map(|n| n.rect()), h.query_by_label("Transform tab").map(|n| n.rect()));
        suhina_ui::kit::native_css::set_user_sheet(None);
        rects
    };
    let (Some(notes), Some(transform)) = open("") else { panic!("the MIDI tools show") };
    assert_eq!(transform.left() - notes.right(), 2.0);
    let (_, Some(spaced)) = open("tab-row { gap: 12px; }") else { panic!("the Transform tab") };
    assert_eq!(spaced.left() - transform.left(), 10.0);
    assert_eq!(open("#tab-transform { display: none; }").1, None);
}

#[test]
fn the_status_bar_follows_the_sheet() {
    let counts = text_at("", "scenes").unwrap();
    let padded = text_at("status-bar { padding-right: 40px; }", "scenes").unwrap();
    assert!((counts.right() - padded.right() - 34.0).abs() < 0.5, "{counts:?} {padded:?}");
    assert!(text_at("#status-counts { display: none; }", "scenes").is_none());
}
