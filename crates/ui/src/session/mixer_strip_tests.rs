// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use egui::Color32;

#[test]
fn volume_scale_labels_move_independently_and_can_be_hidden_in_both_directions() {
    use crate::kit::skin;
    for horizontal in [false, true] {
        for hidden in [false, true] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut skin = skin::Skin::default();
            skin.controls.insert(
                "mixer-volume-scale".into(),
                control_layout::Placement { x: 10, y: 5, hidden, ..Default::default() },
            );
            if horizontal {
                skin.controls.insert(
                    "mixer-volume".into(),
                    control_layout::Placement {
                        direction: control_layout::Direction::Horizontal,
                        ..Default::default()
                    },
                );
            }
            skin::configure(&skin);
            let draw = || {
                ctx.run_ui(egui::RawInput::default(), |ui| {
                    let fader = Rect::from_min_size(
                        egui::pos2(40.0, 40.0),
                        if horizontal { egui::vec2(120.0, 20.0) } else { egui::vec2(18.0, 120.0) },
                    );
                    let response = ui.interact(fader, Id::new("scale-test"), egui::Sense::hover());
                    paint_fader(ui, ui.painter(), Rect::NOTHING, fader, -6.0, &response, false);
                })
            };
            let mut output = draw();
            let text: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.pos),
                    _ => None,
                })
                .collect();
            output.textures_delta.clear();
            assert_eq!(text.is_empty(), hidden);
            if !hidden {
                skin.controls.remove("mixer-volume-scale");
                skin::configure(&skin);
                let mut baseline = draw();
                let native: Vec<_> = baseline
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some(text.pos),
                        _ => None,
                    })
                    .collect();
                baseline.textures_delta.clear();
                assert_eq!(text.len(), native.len());
                for (moved, native) in text.iter().zip(native) {
                    assert!(
                        (*moved - native - egui::vec2(10.0, 5.0)).length() < 0.01,
                        "{horizontal}: {moved:?} vs {native:?}"
                    );
                }
            }
        }
    }
    skin::configure(&skin::Skin::default());
}

#[test]
fn mixer_and_name_css_materials_override_native_tint_and_paint_gradients() {
    use crate::interaction::grid::Grid;
    use crate::kit::{native_css, skin};
    for (gradient, name_gradient) in [(false, false), (true, false), (false, true), (true, true)] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let body = if gradient { "linear-gradient(#102030, #304050)" } else { "#102030" };
        let name = if name_gradient { "linear-gradient(#ffffff22, #00000044)" } else { "#203040" };
        skin::configure(&skin::Skin {
            css: Some(format!(".mixer {{ background: {body}; }} .track-name {{ background: {name}; }}").into()),
            ..Default::default()
        });
        let grid = Grid::build(&crate::demo::demo_project(), None);
        let name_fill = if name_gradient {
            colour_picker::fill(grid.columns[0].color, false)
        } else {
            Color32::from_rgb(0x20, 0x30, 0x40)
        };
        let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(116.0, 340.0));
        let mut state = SessionViewState::default();
        for frame in 0..3 {
            let mut output =
                ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() }, |ui| {
                    let _frame = native_css::frame(ui);
                    show(
                        ui,
                        ui.painter(),
                        rect,
                        &grid.columns[0],
                        &[],
                        1,
                        0,
                        &mut state,
                        Id::new("material-strip"),
                        &mut Vec::new(),
                    );
                });
            if frame == 2 {
                let body_painted = output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if gradient => mesh.calc_bounds() == rect,
                    egui::Shape::Rect(shape) if !gradient => {
                        shape.rect == rect && shape.fill == Color32::from_rgb(0x10, 0x20, 0x30)
                    }
                    _ => false,
                });
                let name_painted = output
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape, egui::Shape::Rect(shape) if shape.fill == name_fill));
                output.textures_delta.clear();
                assert!(body_painted, "missing mixer material, gradient={gradient}");
                assert!(name_painted, "missing name material");
            }
            output.textures_delta.clear();
        }
    }
    skin::configure(&skin::Skin::default());
}

#[test]
fn main_edit_handles_only_include_sections_the_main_strip_draws() {
    let ctx = egui::Context::default();
    let capture = control_layout::Capture::begin(&ctx);
    control_layout::draft(&ctx, Some(Default::default()));
    layout_main(Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::SCENE_COLUMN, 340.0)), Shared::default());
    let entries = capture.entries();
    assert!(entries.iter().any(|entry| entry.name == "mixer-volume"));
    assert!(entries.iter().any(|entry| entry.name == "mixer-meter"));
    assert!(!entries.iter().any(|entry| matches!(entry.name.as_str(), "mixer-name" | "mixer-pan" | "mixer-output")));
    control_layout::draft(&ctx, None);
}

#[test]
fn theme_repositions_name_pan_and_meter_and_rotates_the_volume_layout() {
    use crate::kit::{skin, theme_css};
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::TRACK_COLUMN, 340.0));
    let original = layout(bounds, true, false);
    let mut theme = theme::Theme::default();
    assert!(theme_css::apply(&mut theme, ".control-mixer-volume { --suhina-direction: horizontal; } .control-mixer-name { bottom: 0px; } .control-mixer-meter { width: 80px; height: 10px; } .control-mixer-pan { left: 5px; width: 80px; }").is_empty());
    skin::configure(&theme.skin);
    let moved = layout(bounds, true, false);
    let fader = moved.fader.unwrap();
    assert!(fader.width() > fader.height());
    assert_eq!(moved.name.unwrap().bottom(), bounds.bottom() - PAD);
    assert_eq!(moved.meter.unwrap().size(), Vec2::new(80.0, 10.0));
    assert_eq!(moved.pan.unwrap().left(), original.pan.unwrap().left() + 5.0);
    for rect in [moved.name, moved.pan, moved.fader, moved.meter].into_iter().flatten() {
        assert!(bounds.contains_rect(rect));
    }
    skin::configure(&skin::Skin::default());
    assert_eq!(layout(bounds, true, false), original);
}

fn strip(height: f32) -> Layout {
    layout(Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::TRACK_COLUMN, height)), true, false)
}

fn shows(layout: &Layout) -> Vec<&'static str> {
    let mut parts = Vec::new();
    for (name, part) in [
        ("name", layout.name),
        ("output", layout.output),
        ("devices", layout.devices),
        ("sends", layout.sends),
        ("pan", layout.pan),
        ("fader", layout.fader),
        ("compact", layout.compact),
    ] {
        if part.is_some() {
            parts.push(name);
        }
    }
    parts
}

#[test]
fn each_height_step_adds_its_part_and_shrinking_drops_it_again() {
    assert_eq!(shows(&strip(46.0)), ["compact"]);
    assert_eq!(shows(&strip(FADER_AT - 1.0)), ["compact"]);
    assert_eq!(shows(&strip(FADER_AT)), ["fader"]);
    assert_eq!(shows(&strip(PAN_AT)), ["pan", "fader"]);
    assert_eq!(shows(&strip(SENDS_AT)), ["sends", "pan", "fader"]);
    assert_eq!(shows(&strip(NAME_AT)), ["name", "sends", "pan", "fader"]);
    assert_eq!(shows(&strip(OUTPUT_AT - 1.0)), ["name", "sends", "pan", "fader"]);
    assert_eq!(shows(&strip(OUTPUT_AT)), ["name", "output", "sends", "pan", "fader"]);
    assert_eq!(shows(&strip(DEVICES_AT - 1.0)), ["name", "output", "sends", "pan", "fader"]);
    assert_eq!(shows(&strip(DEVICES_AT)), ["name", "output", "devices", "sends", "pan", "fader"]);
}

#[test]
fn parts_never_overlap_and_the_fader_is_never_shorter_than_its_least() {
    for height in (46..600).map(|h| h as f32) {
        let parts = strip(height);
        let mut rows: Vec<Rect> = [parts.name, parts.output, parts.devices, parts.pan, parts.readout, parts.fader]
            .into_iter()
            .flatten()
            .collect();
        rows.push(parts.toggles);
        if let Some(sends) = parts.sends {
            rows.push(sends);
            assert_eq!(sends.x_range(), parts.toggles.x_range());
            assert!(sends.bottom() <= height - PAD + 0.01);
        }
        for pair in rows.windows(2) {
            assert!(pair[0].bottom() <= pair[1].top(), "{height}: {pair:?}");
        }
        if let Some(fader) = parts.fader {
            assert!(fader.height() >= LEAST_FADER, "{height}: {fader:?}");
            let meter = parts.meter.unwrap_or(Rect::NOTHING);
            assert_eq!(meter.y_range(), fader.y_range());
            assert!(meter.left() >= fader.right() + METER_GAP);
            assert_eq!(meter.width(), METER_WIDTH);
        }
        if let Some(list) = parts.devices {
            assert!(list.height() >= LEAST_DEVICES * DEVICE_ROW, "{height}: {list:?}");
        }
        assert!(parts.toggles.bottom() <= height - PAD + 0.01);
    }
}

#[test]
fn wide_meters_fit_track_return_and_main_strips() {
    for width in [theme::RETURN_COLUMN, theme::SCENE_COLUMN, 116.0] {
        for height in [FADER_AT, 166.0, 300.0, 700.0] {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
            for sends in [false, true] {
                let parts = layout(rect, sends, false);
                let fader = parts.fader.unwrap_or(Rect::NOTHING);
                let meter = parts.meter.unwrap_or(Rect::NOTHING);
                assert!(fader.width() >= CAP.x);
                assert!(meter.left() >= fader.right() + METER_GAP);
                assert!(meter.right() <= rect.right() - PAD);
                assert_eq!(meter.width(), METER_WIDTH);
                if width <= theme::SCENE_COLUMN {
                    assert!((fader.center().x - rect.center().x).abs() < 0.01);
                } else {
                    assert_eq!(fader.left(), rect.left() + PAD);
                }
            }
        }
    }
}

#[test]
fn the_fader_grows_until_the_device_list_shows_then_the_list_takes_the_rest() {
    let tall = strip(600.0);
    assert_eq!(tall.fader.map(|f| f.height()), Some(TALLEST_FADER_WITH_DEVICES));
    let taller = strip(700.0);
    assert_eq!(taller.fader.map(|f| f.height()), Some(TALLEST_FADER_WITH_DEVICES));
    assert!(taller.devices.map(|d| d.height()) > tall.devices.map(|d| d.height()));
    // Without sends the footer stays reserved so faders remain aligned.
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::RETURN_COLUMN, 200.0));
    let without = layout(rect, false, false);
    assert!(without.sends.is_none());
    assert_eq!(without.fader.map(|f| f.height()), strip(200.0).fader.map(|f| f.height()));
}

#[test]
fn the_device_list_is_as_tall_on_a_strip_without_sends() {
    for height in [DEVICES_AT, 300.0, 420.0, 700.0] {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::RETURN_COLUMN, height));
        let (with, without) = (layout(rect, true, false), layout(rect, false, false));
        assert_eq!(with.devices.map(|d| d.y_range()), without.devices.map(|d| d.y_range()), "{height}");
        let divided = Shared { fader: 90.0, ..Shared::default() };
        assert_eq!(
            layout_shared(rect, true, false, divided).devices.map(|d| d.y_range()),
            layout_shared(rect, false, false, divided).devices.map(|d| d.y_range()),
            "{height}"
        );
    }
}

#[test]
fn a_send_takes_its_class_and_its_own_id() {
    use crate::interaction::grid::Grid;
    use crate::kit::{native_css, skin};
    let grid = Grid::build(&crate::demo::demo_project(), None);
    let column = grid.columns.iter().find(|c| !c.sends.is_empty()).expect("the demo has a track with sends");
    for (css, colour) in [
        (".send { background-color: #00FF00; }", Color32::from_rgb(0, 0xFF, 0)),
        (".send { background-color: #00FF00; } #mixer-send-1 { background-color: #FF0000; }", Color32::RED),
    ] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
        let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(116.0, 400.0));
        let mut state = SessionViewState::default();
        let mut found = false;
        for frame in 0..3 {
            let mut output =
                ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() }, |ui| {
                    let _frame = native_css::frame(ui);
                    show(ui, ui.painter(), rect, column, &[], 1, 0, &mut state, Id::new("send-strip"), &mut Vec::new());
                });
            found = output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Rect(shape) if shape.fill == colour));
            output.textures_delta.clear();
        }
        assert!(found, "{css}");
    }
    skin::configure(&skin::Skin::default());
}

#[test]
fn the_divider_is_a_control_a_theme_places_and_others_anchor_to() {
    use crate::kit::{skin, theme_css};
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::TRACK_COLUMN, DEVICES_AT + 120.0));
    let original = layout(bounds, true, false);
    let divider = original.divider.expect("a strip with devices has a divider");
    assert!(divider.top() >= original.devices.unwrap().bottom() - 3.0);
    let mut theme = theme::Theme::default();
    assert!(
        theme_css::apply(
            &mut theme,
            ".control-mixer-divider { top: -10px; } \
             .control-mixer-devices { --suhina-anchor-top: mixer-output bottom 4px; \
                                      --suhina-anchor-bottom: mixer-divider top 0px; }"
        )
        .is_empty()
    );
    skin::configure(&theme.skin);
    let ctx = egui::Context::default();
    let capture = crate::kit::control_layout::Capture::begin(&ctx);
    let moved = layout(bounds, true, false);
    drop(capture);
    assert_eq!(moved.divider.unwrap().top(), divider.top() - 10.0);
    assert_eq!(moved.devices.unwrap().bottom(), moved.divider.unwrap().top());
    skin::configure(&skin::Skin::default());
}

#[test]
fn an_anchored_part_with_no_room_left_draws_nothing() {
    use crate::kit::{skin, theme_css};
    let mut theme = theme::Theme::default();
    assert!(theme_css::apply(&mut theme, ".control-mixer-pan { --suhina-anchor-top: mixer-name top 0px; }").is_empty());
    skin::configure(&theme.skin);
    let ctx = egui::Context::default();
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::TRACK_COLUMN, NAME_AT + 40.0));
    let frame = || {
        let _capture = crate::kit::control_layout::Capture::begin(&ctx);
        layout(bounds, true, false)
    };
    frame();
    let parts = frame();
    skin::configure(&skin::Skin::default());
    assert!(parts.name.is_some());
    assert!(parts.pan.is_none(), "the pan stood on the name: {:?}", parts.pan);
}

#[test]
fn a_strip_without_an_input_keeps_its_room_for_anchors() {
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::TRACK_COLUMN, INPUT_AT + 200.0));
    let with = layout_shared(bounds, true, true, Shared { reserve_input: true, ..Default::default() });
    let without = layout_shared(bounds, true, false, Shared { reserve_input: true, ..Default::default() });
    assert!(without.audio_monitor.is_none());
    assert_eq!(without.reserved_input.map(|r| r[2]), with.audio_monitor);
}
