// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_engine::model::Rgb;
use suhina_ui::interaction::color_picker::{hex, swatch_name, swatches};

/// The color window's palette swatch of `color`.
fn palette(color: Rgb) -> String {
    swatch_name("Palette", color)
}

#[allow(clippy::unwrap_used)] // a test helper outside #[test] functions
fn track_color(h: &Harness<'static, App>, track: u32) -> Option<Rgb> {
    h.state().session().unwrap().project().track(TrackId(track)).unwrap().color
}

#[test]
fn the_menu_and_tool_offer_three_full_rows_of_twelve_clickable_colors() {
    let colors = swatches();
    let distinct: std::collections::BTreeSet<_> = colors.iter().map(|Rgb(r, g, b)| (*r, *g, *b)).collect();
    assert_eq!(distinct.len(), 36);
    for tool in [false, true] {
        let mut app = demo_app();
        if tool {
            app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
        }
        let mut h = window(app);
        if !tool {
            h.get_by_label("Track Drums").click_secondary();
            h.run();
        }
        let label = |color: Rgb| if tool { palette(color) } else { format!("Color {}", hex(color)) };
        let rects: Vec<_> = colors.iter().map(|color| h.get_by_label(&label(*color)).rect()).collect();
        let bounds = if tool {
            h.ctx.memory(|m| m.area_rect(egui::Id::unique("track-color-window"))).unwrap()
        } else {
            h.get_by_label("More colors").rect()
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
        for pair in [rects[0], rects[1], rects[12]].windows(2) {
            let gap = (pair[1].left() - pair[0].right()).max(pair[1].top() - pair[0].bottom());
            assert!(gap > 0.0, "a gap between swatches across and down");
        }
        // The last column remains reachable and dispatches the same command.
        let chosen = colors[11];
        h.get_by_label(&label(chosen)).click();
        h.run();
        assert_eq!(track_color(&h, 1), Some(chosen));
    }
}

#[test]
fn embedded_color_background_stays_inside_its_fitted_window_in_every_mode() {
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
        app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
        let mut h = window(app);
        let captured = egui::Id::unique("test-color-window-background");
        h.ctx.on_end_pass(
            "capture color window background",
            std::sync::Arc::new(move |ui| {
                let ctx = ui.ctx();
                let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::unique("track-color-window"));
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
            suhina_ui::kit::native_css::set_user_sheet(Some(
                "dialog[name=\"color-picker\"] { background-color: #ff00ff; }".into(),
            ));
        }
        for control in [None, Some("View: Field"), Some("View: Steps")] {
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
fn color_tool_previews_selected_headers_and_restores_the_selection_appearance_on_close() {
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
        let color = swatches()[7];
        h.get_by_label(&palette(color)).click();
        h.run();
        let header = h.get_by_label("Track Drums").rect();
        let expected = suhina_ui::interaction::color_picker::color32(color);
        let covers = |shape: &egui::epaint::ClippedShape, fill| {
            matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.fill == fill && rect.rect.expand(0.5).contains_rect(header)
            )
        };
        // The header is drawn as an unselected one: the Launcher's in the color itself, the
        // Arrangement's in its header shading of it with the color's band down its edge.
        assert!(!h.output().shapes.iter().any(|shape| covers(shape, selection.custom)), "no selection fill");
        let shows = h.output().shapes.iter().any(|shape| {
            if arrangement {
                matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == expected && rect.rect.intersects(header))
            } else {
                covers(shape, expected)
            }
        });
        assert!(shows, "header previews its own color");
        assert!(
            h.output().shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(rect) if rect.stroke.width > 0.0 && rect.rect.intersects(header)
                    && suhina_ui::kit::theme::text_contrast(rect.stroke.color, expected) >= 3.0
            )),
            "selection remains visible as an outline"
        );
        let project = h.state().session().unwrap().project().clone();
        h.get_by_label_contains("Close Track color").click();
        h.run();
        assert!(h.output().shapes.iter().any(|shape| covers(shape, selection.custom)), "selection fill returns");
        assert_eq!(h.state().settings().view.selection, selection);
        assert_eq!(h.state().session().unwrap().project(), &project);
    }
}

#[test]
fn f2_opens_a_nonmodal_picker_that_follows_tracks_and_reuses_saved_colors() {
    let mut h = window(demo_app());
    h.get_by_label("Track Drums").click();
    h.run();
    h.key_press(Key::F2);
    h.run();
    assert!(h.query_by_label("Hex color").is_some());
    let color = swatches()[7];
    h.get_by_label(&palette(color)).click();
    h.run();
    assert_eq!(track_color(&h, 1), Some(color));
    assert!(h.state().settings().saved_colors.is_empty(), "choosing a color does not save it");
    h.get_by_label("Save current color").click();
    h.run();
    assert_eq!(h.state().settings().saved_colors, [color]);
    // The underlying DAW accepts a new selection without closing the tool window.
    h.get_by_label("Track Bass").click();
    h.run();
    h.get_by_label(&swatch_name("Saved color", color)).click();
    h.run();
    assert_eq!(track_color(&h, 2), Some(color));
    assert_eq!(h.state().settings().saved_colors, [color]);
    assert_eq!(h.state().session().unwrap().undo_steps().0, 2, "saving is not an edit of the project");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_ne!(track_color(&h, 2), Some(color));
    assert!(h.query_by_label("Hex color").is_some());
}

#[test]
fn a_saved_color_is_removed_with_delete_and_undo_puts_it_back_in_its_place() {
    let mut app = demo_app();
    let saved = vec![Rgb(0x12, 0x34, 0x56), Rgb(0x65, 0x43, 0x21), Rgb(0xa0, 0xb0, 0xc0)];
    let mut settings = app.settings().clone();
    settings.saved_colors = saved.clone();
    app.set_settings(&egui::Context::default(), settings);
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label(&swatch_name("Saved color", saved[1])).click();
    h.run();
    assert_eq!(track_color(&h, 1), Some(saved[1]));
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(h.state().settings().saved_colors, [saved[0], saved[2]]);
    assert_eq!(track_color(&h, 1), Some(saved[1]), "removing a saved color leaves the track as it is");
    h.get_by_label("Undo").click();
    h.run();
    assert_eq!(h.state().settings().saved_colors, saved);
    assert!(h.query_by_label("Undo").is_none(), "Undo is offered once");
    // The current color is saved already: the + saves nothing more.
    h.get_by_label("Save current color").click();
    h.run();
    assert_eq!(h.state().settings().saved_colors, saved);
}

/// Deleting the only saved color leaves the keys on the +, which then saves it again.
#[test]
fn deleting_the_last_saved_color_hands_the_keys_to_the_plus() {
    let mut app = demo_app();
    let color = Rgb(0x12, 0x34, 0x56);
    let mut settings = app.settings().clone();
    settings.saved_colors = vec![color];
    app.set_settings(&egui::Context::default(), settings);
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label(&swatch_name("Saved color", color)).click();
    h.run();
    h.key_press(Key::Delete);
    h.run();
    assert!(h.state().settings().saved_colors.is_empty());
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().settings().saved_colors, [color], "Enter pressed the + that has the keys");
}

#[test]
fn a_color_menu_pages_between_the_palette_and_the_saved_colors() {
    let mut app = demo_app();
    let saved = [Rgb(0x12, 0x34, 0x56), Rgb(0x65, 0x43, 0x21)];
    let mut settings = app.settings().clone();
    settings.saved_colors = saved.to_vec();
    app.set_settings(&egui::Context::default(), settings);
    let mut h = window(app);
    let first = format!("Color {}", hex(swatches()[0]));
    h.get_by_label("Track Drums").click_secondary();
    h.run();
    assert!(h.query_by_label(&first).is_some(), "a menu opens on the palette");
    assert!(h.query_by_label("Palette").is_some());
    h.get_by_label("Next swatches").click();
    h.run();
    assert!(h.query_by_label("Saved colors").is_some());
    assert!(h.query_by_label(&first).is_none());
    h.get_by_label(&format!("Saved color {}", hex(saved[1]))).click();
    h.run();
    assert_eq!(track_color(&h, 1), Some(saved[1]));
    assert!(h.query_by_label("Saved colors").is_none(), "a swatch closes the menu");
    // The next menu opens on the page the last one showed, and either arrow turns it.
    h.get_by_label("Track Drums").click_secondary();
    h.run();
    assert!(h.query_by_label(&format!("Saved color {}", hex(saved[0]))).is_some());
    h.get_by_label("Previous swatches").click();
    h.run();
    assert!(h.query_by_label(&first).is_some());
}

#[test]
fn a_color_menu_says_when_no_colors_are_saved() {
    let mut h = window(demo_app());
    h.get_by_label("Track Drums").click_secondary();
    h.run();
    h.get_by_label("Next swatches").click();
    h.run();
    assert!(h.query_by_label("No saved colors").is_some());
    assert!(h.query_all_by_label_contains("Saved color #").next().is_none());
}

/// Past 35 saved colors a menu's saved page shows the first 35 and "..." in the last cell,
/// which opens the color window as More colors does; 36 cells, no scrolling.
#[test]
fn a_color_menu_with_more_than_35_saved_colors_ends_in_a_cell_that_opens_the_window() {
    let mut app = demo_app();
    let saved: Vec<Rgb> = (0..40u8).map(|i| Rgb(i, 0x40, 0x80)).collect();
    let mut settings = app.settings().clone();
    settings.saved_colors = saved.clone();
    app.set_settings(&egui::Context::default(), settings);
    let mut h = window(app);
    h.get_by_label("Track Drums").click_secondary();
    h.run();
    h.get_by_label("Next swatches").click();
    h.run();
    assert_eq!(h.query_all_by_label_contains("Saved color #").count(), 35);
    assert!(h.query_by_label(&format!("Saved color {}", hex(saved[34]))).is_some());
    assert!(h.query_by_label(&format!("Saved color {}", hex(saved[35]))).is_none());
    let first = h.get_by_label(&format!("Saved color {}", hex(saved[0]))).rect();
    let more = h.get_by_label("More saved colors").rect();
    assert!((more.size() - first.size()).length() < 0.01, "... takes a swatch's cell");
    assert!(more.top() > first.top() && more.left() > first.left(), "... is the last cell");
    h.get_by_label("More saved colors").click();
    h.run();
    assert!(h.query_by_label("Hex color").is_some(), "... opens the color window");
    assert!(h.query_by_label("Next swatches").is_none(), "and closes the menu");
}

/// A menu's swatches are square and fill its width; More colors ends the row of the arrows.
#[test]
fn a_color_menus_square_swatches_fill_it_and_more_colors_ends_the_arrows_row() {
    let mut h = window(demo_app());
    h.get_by_label("Track Drums").click_secondary();
    h.run();
    let first = h.get_by_label(&format!("Color {}", hex(swatches()[0]))).rect();
    let last = h.get_by_label(&format!("Color {}", hex(swatches()[11]))).rect();
    let more = h.get_by_label("More colors").rect();
    let arrow = h.get_by_label("Next swatches").rect();
    assert!((first.width() - first.height()).abs() < 0.01, "{first:?} is square");
    assert!(first.width() >= 14.0);
    assert!(
        (last.right() - more.right()).abs() < 1.0,
        "the grid and More colors end together: {last:?} {more:?} {first:?}"
    );
    assert!(more.center().y > arrow.top() && more.center().y < arrow.bottom(), "More colors is on the arrows' row");
    assert!(more.top() > last.bottom());
}

#[test]
fn the_window_opened_on_arrangement_clips_colors_them_and_default_gives_back_the_tracks() {
    let mut app = demo_app();
    let project = app.session().unwrap().project().clone();
    let (track, clip) = project
        .arrangement
        .lanes
        .iter()
        .find_map(|lane| Some((lane.track?, lane.clips.first()?.id)))
        .expect("the demo has an arrangement clip");
    let track_before = project.track(track).unwrap().color;
    app.run(&Action::new("ui.colorPicker", json!({"clips":[clip.0]})));
    let mut h = window(app);
    let own = |h: &Harness<'static, App>| {
        let project = h.state().session().unwrap().project();
        project.clip_place(clip).and_then(|place| project.clip_at(place)).and_then(|c| c.color)
    };
    assert!(h.query_by_label_contains("Close Clip color").is_some(), "the title names clips");
    let color = swatches()[9];
    h.get_by_label(&palette(color)).click();
    h.run();
    assert_eq!(own(&h), Some(color));
    assert_eq!(h.state().session().unwrap().project().track(track).unwrap().color, track_before);
    h.get_by_label("Default").click();
    h.run();
    assert_eq!(own(&h), None, "the clip shows its track's color again");
}

#[test]
fn hex_entry_is_exact_and_invalid_input_cannot_recolor_the_track() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label("Hex color").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#2577e7".into()));
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(Rgb(37, 119, 231)));
    let depth = h.state().session().unwrap().undo_steps();
    h.get_by_label("Hex color").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#12zz".into()));
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session().unwrap().undo_steps(), depth);
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(Rgb(37, 119, 231)));
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Hex color").is_some());
    assert!(h.query_by_label("Enter a valid color value").is_none());
    h.get_by_label("Default").click();
    h.run();
    assert!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color.is_none());
}

#[test]
fn escape_discards_a_valid_text_draft_instead_of_committing_on_focus_loss() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label("Hex color").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#2577e7".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
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
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
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
        assert!(h.query_by_label("Enter a valid color value").is_some());
        h.key_press(Key::Escape);
        h.run();
        assert!(h.query_by_label("Enter a valid color value").is_none());
        assert_eq!(h.state().session().unwrap().project(), &before);
    }
    h.get_by_label("Red").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("999".into()));
    h.key_press(Key::Enter);
    h.run();
    h.get_by_label("Default").click();
    h.run();
    assert!(h.query_by_label("Enter a valid color value").is_none());
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, None);
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
    h.get_by_label("View: Field").click();
    h.run();
    h.run();
    h.get_by_label("Color field").click();
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
    assert!(h.query_by_label("Hex color").is_some());
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
    let color = swatches()[6];
    h.get_by_label(&palette(color)).click();
    h.run();
    for id in [1, 2] {
        assert_eq!(track_color(&h, id), Some(color));
    }
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    h.state_mut().open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    h.run();
    assert!(h.query_by_label("Hex color").is_none());
}

#[test]
fn escape_stops_a_live_drag_until_release_without_undoing_an_intervening_edit() {
    for intervening_edit in [false, true] {
        let mut app = demo_app();
        app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
        let mut h = window(app);
        let before = h.state().session().unwrap().project().track(TrackId(1)).unwrap().color;
        h.get_by_label("View: Field").click();
        h.run();
        h.run();
        let field = h.get_by_label("Color field").rect();
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
        }
    }
}

#[test]
fn deleting_the_last_target_disables_the_open_picker() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.state_mut().run(&Action::new("track.delete", json!({"track":1})));
    h.run();
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label(&palette(swatches()[6])).click();
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    assert!(h.query_by_label("Hex color").is_some());
}

#[test]
fn closing_keeps_the_applied_color_and_reopening_preserves_exact_values() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label(&palette(swatches()[6])).click();
    h.run();
    let chosen = h.state().session().unwrap().project().clone();
    h.get_by_label_contains("Close Track color").click();
    h.run();
    assert!(h.query_by_label("Hex color").is_none());
    assert_eq!(h.state().session().unwrap().project(), &chosen);
    h.state_mut().run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    h.run();
    h.get_by_label("View: Field").click();
    // Growing takes a few frames, and the harness runs at most four at a time.
    h.run();
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &chosen);
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
}

#[test]
fn the_window_fits_its_controls_in_either_view() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    let area =
        |h: &Harness<'static, App>| h.ctx.memory(|m| m.area_rect(egui::Id::unique("track-color-window"))).unwrap();
    // The last row ends at the window's padding: no room is left under it.
    let fits = |h: &Harness<'static, App>| {
        let gap = area(h).bottom() - h.get_by_label("Default").rect().bottom();
        assert!((0.0..=24.0).contains(&gap), "{gap} under the last row of {:?}", area(h));
    };
    let shades = |h: &Harness<'static, App>| h.query_all_by_label_contains("Shade ").next().is_some();
    assert!(shades(&h), "the window opens in Steps");
    fits(&h);
    let steps = area(&h).height();
    // The views are as tall as each other in the built-in themes: switching keeps the height.
    h.get_by_label("View: Field").click();
    h.run();
    h.run();
    h.get_by_label("Soft colors").rect();
    assert!(!shades(&h));
    fits(&h);
    assert!((area(&h).height() - steps).abs() < 1.0, "Field {} against Steps {steps}", area(&h).height());
    h.get_by_label("View: Steps").click();
    h.run();
    h.run();
    assert!(shades(&h));
    assert!(h.query_by_label("Soft colors").is_none());
    fits(&h);
}

#[test]
fn steps_offers_a_seven_by_seven_grid_and_36_hue_steps_that_keep_the_chosen_cell() {
    use egui_kittest::kittest::NodeT;
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.run();
    // Each cell's hex, row by row: a cell is named "Shade <name> #rrggbb".
    let shades = |h: &Harness<'static, App>| {
        let mut cells: Vec<(egui::Rect, String)> = h
            .query_all_by_label_contains("Shade ")
            .map(|node| {
                let label = node.accesskit_node().label().unwrap_or_default();
                (node.rect(), label.rsplit(' ').next().unwrap_or_default().to_owned())
            })
            .collect();
        cells.sort_by(|a, b| (a.0.top(), a.0.left()).partial_cmp(&(b.0.top(), b.0.left())).unwrap());
        cells
    };
    // The whole name of the cell of `hex`.
    let named = |h: &Harness<'static, App>, hex: &str| {
        h.query_all_by_label_contains("Shade ")
            .map(|node| node.accesskit_node().label().unwrap_or_default())
            .find(|label| label.ends_with(hex))
            .unwrap()
    };
    let cells = shades(&h);
    // Seven rows of seven: white bottom left, black bottom right.
    assert_eq!(cells.len(), 49);
    assert_eq!(cells[42].1, "#ffffff", "white bottom left");
    assert_eq!(cells[48].1, "#000000", "black bottom right");
    for row in cells.chunks(7) {
        assert!(row.windows(2).all(|pair| pair[0].0.top() == pair[1].0.top() && pair[0].0.left() < pair[1].0.left()));
    }
    h.get_by_label(&named(&h, &cells[8].1)).click();
    h.run();
    assert_eq!(track_color(&h, 1).map(hex).as_deref(), Some(cells[8].1.as_str()));
    let chosen = named(&h, &cells[8].1);
    assert_eq!(h.get_by_label(&chosen).accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
    // One hue step on: the same cell of the next hue's grid, and the grid follows.
    h.get_by_label("Hue").click();
    h.run();
    let start = h.get_by_label("Hue").accesskit_node().numeric_value();
    h.key_press(Key::ArrowRight);
    h.run();
    assert_eq!(h.get_by_label("Hue").accesskit_node().numeric_value(), start.map(|v| v + 1.0));
    let after = shades(&h);
    assert_eq!(track_color(&h, 1).map(hex).as_deref(), Some(after[8].1.as_str()));
    assert_ne!(after[3].1, cells[3].1, "the full hue moved");
    // Without steps the hue moves by a degree, and the grid with it.
    h.get_by_label("Hue steps").click();
    h.run();
    h.get_by_label("Hue").click();
    h.run();
    let degrees = h.get_by_label("Hue").accesskit_node().numeric_value();
    let before = shades(&h);
    h.key_press(Key::ArrowRight);
    h.run();
    let moved =
        h.get_by_label("Hue").accesskit_node().numeric_value().zip(degrees).map(|(after, before)| after - before);
    assert!(moved.is_some_and(|d| (d - 1.0).abs() < 1e-3), "one degree on: {moved:?}");
    assert_ne!(shades(&h)[3].1, before[3].1, "the grid follows a hue between the steps");
}

#[test]
fn a_palette_color_shows_chosen_in_the_palette_and_in_steps() {
    use egui_kittest::kittest::NodeT;
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    for color in swatches() {
        h.state_mut().run(&Action::new("track.setColor", json!({"track":1,"color":hex(color)})));
        h.run();
        let toggled = |label: &str| h.get_by_label(label).accesskit_node().toggled();
        assert_eq!(toggled(&palette(color)), Some(egui::accesskit::Toggled::True), "{}", hex(color));
        assert_eq!(toggled(&swatch_name("Shade", color)), Some(egui::accesskit::Toggled::True), "{}", hex(color));
    }
}

#[test]
fn the_colour_grids_are_centered_in_the_window() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colorPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.run();
    let grid = h
        .query_all_by_label_contains("Shade ")
        .map(|cell| cell.rect())
        .reduce(|a, b| a.union(b))
        .expect("the Steps grid");
    let middle = h.ctx.memory(|m| m.area_rect(egui::Id::unique("track-color-window"))).unwrap().center().x;
    h.get_by_label("View: Field").click();
    h.run();
    h.run();
    let field = h.get_by_label("Color field").rect();
    let near = |a: f32, b: f32| (a - b).abs() < 0.5;
    assert!(near(grid.center().x, middle), "{grid:?} centered on {middle}");
    assert!(near(field.center().x, middle), "{field:?} centered on {middle}");
}
