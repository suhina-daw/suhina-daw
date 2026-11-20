// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_engine::model::Rgb;

#[test]
fn the_menu_and_tool_offer_three_full_rows_of_twelve_clickable_colours() {
    use suhina_ui::interaction::colour_picker::{hex, swatches};
    let palette = swatches();
    let distinct: std::collections::BTreeSet<_> = palette.iter().map(|Rgb(r, g, b)| (*r, *g, *b)).collect();
    assert_eq!(distinct.len(), 36);
    for tool in [false, true] {
        let mut app = demo_app();
        if tool {
            app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
        }
        let mut h = window(app);
        if !tool {
            h.get_by_label("Track Drums").click_secondary();
            h.run();
        }
        let prefix = if tool { "Palette" } else { "Colour" };
        let rects: Vec<_> =
            palette.iter().map(|colour| h.get_by_label(&format!("{prefix} {}", hex(*colour))).rect()).collect();
        let bounds = if tool {
            h.ctx.memory(|m| m.area_rect(egui::Id::unique("track-colour-window"))).unwrap()
        } else {
            h.get_by_label("+ More colours").rect()
        };
        for row in rects.chunks(12) {
            for rect in row {
                assert_eq!(rect.top(), row[0].top(), "each palette row stays on one line");
                assert!(rect.width() >= 14.0 && rect.right() <= bounds.right(), "{rect:?} outside {bounds:?}");
            }
            for pair in row.windows(2) {
                assert!(pair[0].right() < pair[1].left(), "swatches do not overlap");
            }
        }
        assert!(rects[0].bottom() < rects[12].top() && rects[12].bottom() < rects[24].top());
        // The newly added last column remains reachable and dispatches the same command.
        let chosen = palette[11];
        h.get_by_label(&format!("{prefix} {}", hex(chosen))).click();
        h.run();
        assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(chosen));
    }
}

#[test]
fn embedded_colour_background_stays_inside_its_fitted_window_in_every_mode() {
    fn fills(shape: &egui::Shape, clip: egui::Rect, found: &mut Vec<(egui::Rect, egui::Color32)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| fills(shape, clip, found)),
            egui::Shape::Rect(rect) => found.push((rect.rect.intersect(clip), rect.fill)),
            _ => {}
        }
    }
    type Background = (egui::Rect, Vec<(egui::Rect, egui::Color32)>);
    for custom in [false, true] {
        let mut app = demo_app();
        app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
        let mut h = window(app);
        let captured = egui::Id::unique("test-colour-window-background");
        h.ctx.on_end_pass(
            "capture colour window background",
            std::sync::Arc::new(move |ui| {
                let ctx = ui.ctx();
                let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::unique("track-colour-window"));
                let Some(bounds) = ctx.memory(|m| m.area_rect(layer.id)) else { return };
                let painted = ctx.graphics(|layers| {
                    let mut painted = Vec::new();
                    if let Some(list) = layers.get(layer) {
                        for shape in list.all_entries() {
                            fills(&shape.shape, shape.clip_rect, &mut painted);
                        }
                    }
                    painted
                });
                ctx.data_mut(|data| data.insert_temp(captured, (bounds, painted)));
            }),
        );
        if custom {
            suhina_ui::kit::native_css::set_user_sheet(Some("colour-picker { background-color: #ff00ff; }".into()));
        }
        for control in [None, Some("Fine adjustment"), Some("View: Wheel")] {
            if let Some(control) = control {
                h.get_by_label(control).click();
            }
            h.step();
            h.run();
            let expected = if custom { egui::Color32::from_rgb(255, 0, 255) } else { live("panel-background") };
            let (bounds, painted) = h.ctx.data(|data| data.get_temp::<Background>(captured)).unwrap();
            let backgrounds: Vec<_> =
                painted.iter().filter(|(rect, fill)| *fill == expected && rect.is_positive()).collect();
            assert!(!backgrounds.is_empty(), "the theme background is painted");
            for (rect, _) in backgrounds {
                assert!(bounds.expand(0.5).contains_rect(*rect), "background {rect:?} escapes {bounds:?}");
            }
        }
        suhina_ui::kit::native_css::set_user_sheet(None);
    }
}

#[test]
fn colour_tool_previews_selected_headers_and_restores_the_selection_appearance_on_close() {
    use suhina_ui::kit::selection::{SelectionColour, SelectionLook};
    for arrangement in [false, true] {
        let mut app = demo_app();
        let mut settings = app.settings().clone();
        let selection = SelectionLook { mode: SelectionColour::Custom, custom: egui::Color32::from_rgb(139, 107, 231) };
        settings.view.selection = selection;
        app.set_settings(&egui::Context::default(), settings);
        let mut h = window(app);
        if arrangement {
            h.get_by_label("Arrangement view").click();
            h.run();
        }
        h.get_by_label("Track Drums").click();
        h.run();
        h.key_press(Key::F2);
        h.run();
        let colour = suhina_ui::interaction::colour_picker::swatches()[7];
        let label = format!("Palette {}", suhina_ui::interaction::colour_picker::hex(colour));
        h.get_by_label(&label).click();
        h.run();
        let header = h.get_by_label("Track Drums").rect();
        let expected = suhina_ui::interaction::colour_picker::color32(colour);
        let covers = |shape: &egui::epaint::ClippedShape, fill| {
            matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.fill == fill && rect.rect.expand(0.5).contains_rect(header)
            )
        };
        assert!(h.output().shapes.iter().any(|shape| covers(shape, expected)), "header previews its own colour");
        assert!(
            h.output().shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.stroke.width > 0.0 && rect.rect.intersects(header)
                    && suhina_ui::kit::theme::text_contrast(rect.stroke.color, expected) >= 3.0
            )),
            "selection remains visible as an outline"
        );
        let project = h.state().session().unwrap().project().clone();
        h.get_by_label("Close window").click();
        h.run();
        assert!(h.output().shapes.iter().any(|shape| covers(shape, selection.custom)), "selection fill returns");
        assert_eq!(h.state().settings().view.selection, selection);
        assert_eq!(h.state().session().unwrap().project(), &project);
    }
}

#[test]
fn f2_opens_a_nonmodal_picker_that_follows_tracks_and_reuses_recent_colours() {
    let mut h = window(demo_app());
    h.get_by_label("Track Drums").click();
    h.run();
    h.key_press(Key::F2);
    h.run();
    assert!(h.query_by_label("Hex colour").is_some());
    let colour = suhina_ui::interaction::colour_picker::swatches()[7];
    let value = suhina_ui::interaction::colour_picker::hex(colour);
    h.get_by_label(&format!("Palette {value}")).click();
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(colour));
    assert_eq!(h.state().settings().recent_colours, [colour]);
    // The underlying DAW accepts a new selection without closing the tool window.
    h.get_by_label("Track Bass").click();
    h.run();
    h.get_by_label(&format!("Recent colour {value}")).click();
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(2)).unwrap().color, Some(colour));
    assert_eq!(h.state().settings().recent_colours, [colour]);
    assert_eq!(h.state().session().unwrap().undo_steps().0, 2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_ne!(h.state().session().unwrap().project().track(TrackId(2)).unwrap().color, Some(colour));
    assert!(h.query_by_label("Hex colour").is_some());
}

#[test]
fn hex_entry_is_exact_and_invalid_input_cannot_recolour_the_track() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label("Hex colour").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#2577e7".into()));
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(Rgb(37, 119, 231)));
    let depth = h.state().session().unwrap().undo_steps();
    h.get_by_label("Hex colour").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#12zz".into()));
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session().unwrap().undo_steps(), depth);
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(Rgb(37, 119, 231)));
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Hex colour").is_some());
    assert!(h.query_by_label("Enter a valid colour value").is_none());
    h.get_by_label("Default").click();
    h.run();
    assert!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color.is_none());
    assert_eq!(h.state().settings().recent_colours, [Rgb(37, 119, 231)]);
}

#[test]
fn escape_discards_a_valid_text_draft_instead_of_committing_on_focus_loss() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label("Hex colour").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#2577e7".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    assert!(h.state().settings().recent_colours.is_empty());
    h.get_by_label("Fine adjustment").click();
    h.run();
    h.get_by_label("Red").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("17".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
}

#[test]
fn invalid_numeric_entries_remain_cancellable_and_default_clears_the_error() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label("Fine adjustment").click();
    h.run();
    let before = h.state().session().unwrap().project().clone();
    for (label, invalid) in [("Red", "999"), ("HSL saturation", "no")] {
        h.get_by_label(label).click();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.event(egui::Event::Text(invalid.into()));
        h.key_press(Key::Enter);
        h.run();
        // Focus must survive beyond the frame which submitted the invalid draft.
        for _ in 0..3 {
            h.step();
        }
        assert!(h.query_by_label("Enter a valid colour value").is_some());
        h.key_press(Key::Escape);
        h.run();
        assert!(h.query_by_label("Enter a valid colour value").is_none());
        assert_eq!(h.state().session().unwrap().project(), &before);
    }
    h.get_by_label("Red").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("999".into()));
    h.key_press(Key::Enter);
    h.run();
    h.get_by_label("Default").click();
    h.run();
    assert!(h.query_by_label("Enter a valid colour value").is_none());
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, None);
    assert!(h.state().settings().recent_colours.is_empty());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
}

#[test]
fn picker_arrows_and_tab_stay_in_the_tool_and_main_shortcuts_resume_outside_it() {
    let mut h = window(demo_app());
    h.get_by_label("Track Drums").click();
    h.run();
    h.key_press(Key::F2);
    h.run();
    h.get_by_label("Fine adjustment").click();
    h.run();
    h.get_by_label("Colour field").click();
    h.run();
    let view = h.state().view();
    for _ in 0..3 {
        let before = h.state().session().unwrap().project().track(TrackId(1)).unwrap().color;
        h.key_press(Key::ArrowRight);
        h.run();
        assert_ne!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, before);
    }
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(h.state().view(), view);
    h.get_by_label("Track Bass").click();
    h.run();
    h.key_press(Key::Tab);
    h.run();
    assert_ne!(h.state().view(), view);
    assert!(h.query_by_label("Hex colour").is_some());
}

#[test]
fn a_multitrack_choice_is_one_undo_and_project_switch_closes_the_picker() {
    let mut h = window(demo_app());
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label("Track Drums").click();
    h.run();
    h.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    h.run();
    h.key_press(Key::F2);
    h.run();
    h.get_by_label("Palette #2fd9c4").click();
    h.run();
    for id in [TrackId(1), TrackId(2)] {
        assert_eq!(h.state().session().unwrap().project().track(id).unwrap().color, Some(Rgb(47, 217, 196)));
    }
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    h.state_mut().open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    h.run();
    assert!(h.query_by_label("Hex colour").is_none());
}

#[test]
fn escape_stops_a_live_drag_until_release_without_undoing_an_intervening_edit() {
    for intervening_edit in [false, true] {
        let mut app = demo_app();
        app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
        let mut h = window(app);
        let before = h.state().session().unwrap().project().track(TrackId(1)).unwrap().color;
        h.get_by_label("Fine adjustment").click();
        h.run();
        let field = h.get_by_label("Colour field").rect();
        let from = field.left_top() + field.size() * 0.3;
        let to = field.left_top() + field.size() * 0.7;
        h.hover_at(from);
        h.step();
        h.drag_at(from);
        h.step();
        h.hover_at(to);
        h.step();
        let dragged = h.state().session().unwrap().project().track(TrackId(1)).unwrap().color;
        assert_ne!(dragged, before);
        if intervening_edit {
            h.state_mut().run(&Action::new("track.rename", json!({"track":2,"name":"Keep this edit"})));
        }
        h.key_press(Key::Escape);
        h.step();
        h.hover_at(from);
        h.step();
        h.drop_at(from);
        h.run();
        let session = h.state().session().unwrap();
        if intervening_edit {
            assert_eq!(session.project().track(TrackId(2)).unwrap().name, "Keep this edit");
            assert_eq!(session.project().track(TrackId(1)).unwrap().color, dragged);
            assert_eq!(session.undo_steps().0, 2);
        } else {
            assert_eq!(session.project().track(TrackId(1)).unwrap().color, before);
            assert_eq!(session.undo_steps().0, 0);
            assert!(h.state().settings().recent_colours.is_empty());
        }
    }
}

#[test]
fn deleting_the_last_target_disables_the_open_picker() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.state_mut().run(&Action::new("track.delete", json!({"track":1})));
    h.run();
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label("Palette #2fd9c4").click();
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    assert!(h.query_by_label("Hex colour").is_some());
}

#[test]
fn closing_keeps_the_applied_colour_and_reopening_preserves_exact_values() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label("Palette #2fd9c4").click();
    h.run();
    let chosen = h.state().session().unwrap().project().clone();
    h.get_by_label("Close window").click();
    h.run();
    assert!(h.query_by_label("Hex colour").is_none());
    assert_eq!(h.state().session().unwrap().project(), &chosen);
    h.state_mut().run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    h.run();
    h.get_by_label("Fine adjustment").click();
    h.run();
    h.get_by_label("View: Wheel").click();
    // Growing takes a few frames, and the harness runs at most four at a time.
    h.run();
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &chosen);
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
    assert_eq!(h.state().settings().recent_colours, [Rgb(47, 217, 196)]);
}

#[test]
fn the_window_fits_its_controls_and_grows_before_fine_adjustment_is_drawn() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    let area =
        |h: &Harness<'static, App>| h.ctx.memory(|m| m.area_rect(egui::Id::unique("track-colour-window"))).unwrap();
    // The last row ends at the window's padding: no room is left under it.
    let fits = |h: &Harness<'static, App>| {
        let gap = area(h).bottom() - h.get_by_label("Default").rect().bottom();
        assert!((0.0..=24.0).contains(&gap), "{gap} under the last row of {:?}", area(h));
    };
    fits(&h);
    let compact = area(&h).height();
    h.get_by_label("Fine adjustment").click();
    // The frame that takes the click asks, and the window grows before the new controls draw:
    // no frame shows them in the compact window.
    for _ in 0..4 {
        h.step();
        let hue = h.query_by_label("Hue").is_some();
        assert!(!hue || area(&h).height() > compact, "Hue drawn in a window {} tall", area(&h).height());
    }
    h.run();
    assert!(h.query_by_label("Hue").is_some());
    fits(&h);
    h.get_by_label("View: Wheel").click();
    // Growing takes a few frames, and the harness runs at most four at a time.
    h.run();
    h.run();
    h.get_by_label("Value").rect();
    fits(&h);
    h.get_by_label("Fine adjustment").click();
    h.run();
    assert!(h.query_by_label("Hue").is_none());
    assert_eq!(area(&h).height(), compact, "closing gives the room back");
    fits(&h);
}
