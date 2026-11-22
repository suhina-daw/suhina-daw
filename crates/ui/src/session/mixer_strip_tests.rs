// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use egui::Color32;

/// Where `paint_fader` draws the scale's labels for a fader in `fader` with its scale in `scale`.
fn scale_labels(fader: Rect, scale: Option<Rect>) -> Vec<Pos2> {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let response = ui.interact(fader, Id::unique("scale-test"), egui::Sense::hover());
        paint_fader(ui, ui.painter(), Rect::NOTHING, fader, scale, -6.0, &response);
    });
    output.textures_delta.clear();
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.pos),
            _ => None,
        })
        .collect()
}

#[test]
fn the_scale_is_drawn_where_it_is_laid_out_and_not_at_all_when_hidden() {
    // Standing up, beside the fader; lying down (a fader laid out wider than tall), under it.
    for (fader, scale) in [
        (
            Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::new(20.0, 120.0)),
            Rect::from_min_size(Pos2::new(60.0, 40.0), Vec2::new(14.0, 120.0)),
        ),
        (
            Rect::from_min_size(Pos2::new(40.0, 40.0), Vec2::new(120.0, 20.0)),
            Rect::from_min_size(Pos2::new(40.0, 60.0), Vec2::new(120.0, 12.0)),
        ),
    ] {
        let native = scale_labels(fader, Some(scale));
        assert!(!native.is_empty());
        let moved = scale_labels(fader, Some(scale.translate(Vec2::new(10.0, 5.0))));
        assert_eq!(moved.len(), native.len());
        for (moved, native) in moved.iter().zip(&native) {
            assert!((*moved - *native - Vec2::new(10.0, 5.0)).length() < 0.01, "{moved:?} vs {native:?}");
        }
        assert!(scale_labels(fader, None).is_empty(), "a hidden scale draws no labels");
    }
}

#[test]
fn mixer_and_name_css_materials_override_native_tint_and_paint_gradients() {
    use crate::interaction::grid::TrackGrid;
    use crate::kit::{native_css, skin};
    for (gradient, name_gradient) in [(false, false), (true, false), (false, true), (true, true)] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let body = if gradient { "linear-gradient(#102030, #304050)" } else { "#102030" };
        let name = if name_gradient { "linear-gradient(#ffffff22, #00000044)" } else { "#203040" };
        skin::configure(&skin::Skin {
            css: Some(format!("mixer {{ background: {body}; }} track-name {{ background: {name}; }}").into()),
            ..Default::default()
        });
        let grid = TrackGrid::build(&crate::demo::demo_project(), None);
        let name_fill = if name_gradient {
            color_picker::fill(grid.columns[0].color, false)
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
                        Id::unique("material-strip"),
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

/// A strip `height` tall laid out under `css` (its parts by the theme's sheet over the user-agent
/// sheet), as the Launcher lays one out.
fn styled(css: &str, width: f32, height: f32) -> Layout {
    use crate::kit::{native_css, skin};
    skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
    let ctx = egui::Context::default();
    let mut parts = None;
    for _ in 0..2 {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = native_css::frame(ui);
            let el = crate::kit::element::MIXER.id("track-1").enter();
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
            parts =
                Some(layout_in(Host::Strip(&el), rect, true, false, Shared::default(), Strip::Track { armable: true }));
        })
        .textures_delta
        .clear();
    }
    skin::configure(&skin::Skin::default());
    parts.unwrap()
}

#[test]
fn mixer_toggles_standing_up_stack_solo_cue_and_arm() {
    // #995: the S, cue and arm row stands up; the track-on switch with the number is its own part.
    let row = styled("", theme::TRACK_COLUMN, 300.0);
    let column = styled("mixer-toggles { flex-direction: column; }", theme::TRACK_COLUMN, 300.0);
    let [solo, cue, arm] = [column.solo, column.cue, column.arm].map(Option::unwrap);
    assert_eq!((solo.left(), cue.left()), (arm.left(), arm.left()), "one above the other");
    assert!(solo.bottom() <= cue.top() && cue.bottom() <= arm.top());
    assert_eq!(solo.size(), row.solo.unwrap().size(), "each as big as before");
    // The column is taller than the row was, and the fader gives up the room.
    assert!(column.fader.unwrap().height() < row.fader.unwrap().height(), "{column:#?} {row:#?}");
    assert!(column.fader.unwrap().bottom() <= solo.top());
    // The track number stays a part of its own beside them, as tall as their column.
    let on = column.track_on.unwrap();
    assert!(on.right() <= solo.left());
    assert_eq!(on.y_range(), column.toggles.y_range());
    let hidden = styled("#control-track-on { display: none; }", theme::TRACK_COLUMN, 300.0);
    assert!(hidden.track_on.is_none());
    assert!(hidden.solo.is_some() && hidden.cue.is_some() && hidden.arm.is_some());
}

#[test]
fn a_theme_lays_the_parts_out_and_hides_them() {
    let original = styled("", theme::TRACK_COLUMN, 340.0);
    let moved = styled(
        "mixer meter { order: -1; margin: 0 4px 0 0; } mixer track-colour { display: none; } \
         #mixer-pan { margin-left: 5px; width: 40px; }",
        theme::TRACK_COLUMN,
        340.0,
    );
    let (fader, meter) = (moved.fader.unwrap(), moved.meter.unwrap());
    // The meter goes first in the centered row, before the scale and the fader.
    assert!(meter.left() < original.scale.unwrap().left(), "the meter before the fader");
    assert!(fader.left() >= meter.right() + 4.0);
    assert!(moved.band.is_none());
    let pan = moved.pan.unwrap();
    assert_eq!((pan.left(), pan.width()), (original.pan.unwrap().left() + 5.0, 40.0));
    // A fader laid out wider than tall lies down, and drags across.
    let lying = styled(
        "group#mixer-level { flex-direction: column; } #mixer-volume { height: 20px; }",
        theme::TRACK_COLUMN,
        340.0,
    );
    let fader = lying.fader.unwrap();
    assert!(lies(fader), "{fader:?}");
    assert_eq!(fader.width(), theme::TRACK_COLUMN - 2.0 * PAD);
    assert!(!lies(original.fader.unwrap()));
}

#[test]
fn every_strip_of_a_kind_shares_one_layout_worked_out_once() {
    use crate::kit::native_css::row;
    styled("mixer { padding: 1px; }", theme::TRACK_COLUMN, 300.0);
    let before = row::computed();
    // The same strip again, for another track: nothing is laid out anew.
    let again = || {
        crate::kit::skin::configure(&crate::kit::skin::Skin {
            css: Some("mixer { padding: 1px; }".into()),
            ..Default::default()
        });
        let ctx = egui::Context::default();
        let mut laid = Vec::new();
        for _ in 0..2 {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let _frame = crate::kit::native_css::frame(ui);
                laid.clear();
                for track in 0..4 {
                    let el = crate::kit::element::MIXER
                        .id(&format!("track-{track}"))
                        .color(Color32::from_rgb(track * 40, 0, 0))
                        .enter();
                    let rect = Rect::from_min_size(Pos2::new(f32::from(track) * 116.0, 0.0), Vec2::new(116.0, 300.0));
                    laid.push(layout_in(
                        Host::Strip(&el),
                        rect,
                        true,
                        false,
                        Shared::default(),
                        Strip::Track { armable: true },
                    ));
                }
            })
            .textures_delta
            .clear();
        }
        crate::kit::skin::configure(&crate::kit::skin::Skin::default());
        laid
    };
    let laid = again();
    let computed = row::computed() - before;
    // A strip and its insets: one layout each for the four tracks over both frames.
    assert!(computed <= 2, "{computed} layouts for four strips of one kind");
    let fader = laid[0].fader.unwrap();
    assert_eq!(laid[3].fader.unwrap(), fader.translate(Vec2::new(3.0 * 116.0, 0.0)));
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
            assert!(meter.left() >= fader.right() + 4.0);
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
                assert!(meter.left() >= fader.right() + 4.0);
                assert!(meter.right() <= rect.right() - PAD);
                assert_eq!(meter.width(), METER_WIDTH);
                // Every strip centers its fader, a track's as Main's (#1487 B19).
                assert!((fader.center().x - rect.center().x).abs() < 0.01, "{width}: {fader:?}");
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
    use crate::interaction::grid::TrackGrid;
    use crate::kit::{native_css, skin};
    let grid = TrackGrid::build(&crate::demo::demo_project(), None);
    let column = grid.columns.iter().find(|c| !c.sends.is_empty()).expect("the demo has a track with sends");
    for (css, colour) in [
        ("send { background-color: #00FF00; }", Color32::from_rgb(0, 0xFF, 0)),
        ("send { background-color: #00FF00; } #mixer-send-1 { background-color: #FF0000; }", Color32::RED),
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
                    show(
                        ui,
                        ui.painter(),
                        rect,
                        column,
                        &[],
                        1,
                        0,
                        &mut state,
                        Id::unique("send-strip"),
                        &mut Vec::new(),
                    );
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
fn the_divider_lies_over_the_gap_under_the_device_list() {
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::TRACK_COLUMN, DEVICES_AT + 120.0));
    let parts = layout(bounds, true, false);
    let (list, divider, pan) = (parts.devices.unwrap(), parts.divider.unwrap(), parts.pan.unwrap());
    assert_eq!((divider.top(), divider.bottom()), (list.bottom() - 1.0, pan.top() + 1.0));
    assert_eq!(divider.x_range(), list.x_range());
}

#[test]
fn a_strip_without_an_input_keeps_its_room() {
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::TRACK_COLUMN, INPUT_AT + 200.0));
    let with = layout_shared(bounds, true, true, Shared { reserve_input: true, ..Default::default() });
    let without = layout_shared(bounds, true, false, Shared { reserve_input: true, ..Default::default() });
    assert!(without.audio_monitor.is_none() && without.monitor_buttons.iter().all(Option::is_none));
    assert_eq!(without.fader, with.fader, "the controls stay in line across the mixer");
}

#[test]
fn a_fader_cap_stands_for_the_volume_where_it_is_drawn() {
    let fader = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(20.0, 200.0));
    let (bottom, top) = (fader.bottom() - cap_size().y / 2.0, fader.top() + cap_size().y / 2.0);
    for db in [-40.0, -24.0, -6.0, 0.0, 6.0] {
        let y = bottom + (top - bottom) * widgets::volume_position(db);
        assert!((fader_db_at(y, fader) - db).abs() < 0.01, "{db}");
    }
    assert_eq!(fader_db_at(1000.0, fader), VOLUME_RANGE.0);
    assert_eq!(fader_db_at(-1000.0, fader), VOLUME_RANGE.1);
}
