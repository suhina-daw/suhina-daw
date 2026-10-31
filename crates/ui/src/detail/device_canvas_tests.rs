// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn sampler_mode_icons_have_distinct_geometry_and_keep_their_captions() {
    let context = egui::Context::default();
    for label in ["Sample", "Slice", "Grain"] {
        let mut output = context.run_ui(Default::default(), |ui| {
            crate::kit::theme::install(ui.ctx());
            paint_sample_mode(
                ui.painter(),
                Rect::from_min_size(Pos2::new(10., 10.), Vec2::new(60., 46.)),
                label,
                true,
                crate::kit::skin::State::Normal,
                widget_palette(),
            );
        });
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == label)));
        match label {
            // A wave between two bars, a strip cut in three, scattered grains.
            "Sample" => {
                assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Path(p) if p.points.len() == 25)));
                assert_eq!(
                    output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::LineSegment { .. })).count(),
                    2
                );
            }
            "Slice" => assert!(output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(_))).count() >= 3),
            _ => assert_eq!(output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Circle(_))).count(), 7),
        }
    }
}

#[test]
fn sample_zoom_keeps_the_source_point_under_the_pointer() {
    let field = Rect::from_min_size(Pos2::new(70., 24.), Vec2::new(832., 110.));
    let view = SampleView { start: 0.2, span: 0.5 };
    let anchor = field.left() + field.width() * 0.7;
    let zoomed = view.zoomed(anchor, 2., field, 48000);
    assert!((view.point(anchor, field) - zoomed.point(anchor, field)).abs() < 1e-7);
    assert_eq!(zoomed.span, 0.25);
    assert_eq!(zoomed.zoomed(anchor, 0., field, 48000), zoomed);
    assert_eq!(zoomed.zoomed(anchor, f32::NAN, field, 48000), zoomed);
}

#[test]
fn sample_pan_and_zoom_stay_inside_the_file() {
    let field = Rect::from_min_size(Pos2::ZERO, Vec2::new(800., 110.));
    let view = SampleView { start: 0.25, span: 0.5 };
    assert_eq!(view.panned(1e6, field).start, 0.);
    assert_eq!(view.panned(-1e6, field).start, 0.5);
    assert_eq!(view.zoomed(400., 0.01, field, 48000), SampleView::default());
    assert_eq!(view.zoomed(400., 1e9, field, 1), SampleView::default());
}

#[test]
fn theme_device_geometry_moves_paint_and_input_and_hidden_controls_do_not_dispatch() {
    use crate::kit::{control_layout::Placement, skin};
    let canvas = Arc::new(DeviceUi {
        size: [240.0, 120.0],
        controls: vec![CustomControl {
            id: "enabled".into(),
            name: "Enabled".into(),
            tooltip: String::new(),
            bounds: [20.0, 20.0, 40.0, 20.0],
            widget: Some(Widget::Button { label: "On".into(), off_label: None, indicator: false }),
            visible_when: None,
            gesture: UiGesture::Toggle,
            x: None,
            y: Some(UiBinding::new("enabled")),
            drawing: Vec::new(),
        }],
        ..Default::default()
    });
    let parameters = [GraphParameter { value: 0.0, ..parameter("enabled", false) }];
    let mut skin = skin::Skin::default();
    skin.controls.insert(
        "device-enabled".into(),
        Placement { x: 80, y: 30, width: Some(80), height: Some(40), ..Default::default() },
    );
    skin::configure(&skin);
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(240.0, 120.0)),
            &canvas,
            &parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        )
    });
    output.textures_delta.clear();
    let expected = Rect::from_min_size(egui::pos2(100.0, 50.0), Vec2::new(80.0, 40.0));
    assert!(
        output.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.rect == expected && rect.brush.is_some())
        )
    );
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "On" && expected.contains(text.pos))));
    assert!(click(&canvas, &parameters, egui::pos2(40.0, 30.0)).is_empty());
    let actions = click(&canvas, &parameters, expected.center());
    assert!(actions.iter().any(|action| action.id == "device.setParam" && action.params["param"] == "enabled"));
    skin.controls.get_mut("device-enabled").unwrap().hidden = true;
    skin::configure(&skin);
    assert!(click(&canvas, &parameters, expected.center()).is_empty());
    skin::configure(&skin::Skin::default());
    assert!(click(&canvas, &parameters, egui::pos2(40.0, 30.0)).iter().any(|action| action.id == "device.setParam"));
}

#[test]
fn sdk_knob_and_button_use_theme_svg_without_changing_parameter_commands() {
    let control = |id: &str, widget: Widget, x: f32, gesture| CustomControl {
        id: id.into(),
        name: id.into(),
        tooltip: String::new(),
        bounds: [x, 0.0, 40.0, 40.0],
        widget: Some(widget),
        visible_when: None,
        gesture,
        x: None,
        y: Some(UiBinding::new(id)),
        drawing: Vec::new(),
    };
    let canvas = Arc::new(DeviceUi {
        size: [100.0, 60.0],
        controls: vec![
            control("amount", Widget::Knob { label: "Amount".into() }, 0.0, UiGesture::Drag),
            control(
                "enabled",
                Widget::Button { label: "On".into(), off_label: None, indicator: false },
                50.0,
                UiGesture::Toggle,
            ),
        ],
        ..Default::default()
    });
    let parameters = [parameter("amount", false), parameter("enabled", false)];
    let svg = |fill: &str| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"20\"><rect width=\"20\" height=\"20\" fill=\"{fill}\"/></svg>"
        )
    };
    let skin = crate::kit::skin::Skin {
        assets: [("knob.body".into(), svg("#12AB34")), ("button".into(), svg("#AB1234"))].into(),
        css: Some(Arc::from(
            ".device-panel .knob { color:rgb(120 40 180); font-size:17px; } .device-panel .button {background:linear-gradient(90deg,red,blue);border-radius:6px;}",
        )),
        ..Default::default()
    };
    crate::kit::skin::configure(&skin);
    let ctx = egui::Context::default();
    let mut actions = Vec::new();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let _frame = crate::kit::native_css::frame(ui);
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 60.0)),
            &canvas,
            &parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut actions,
        )
    });
    let colors: Vec<_> = output
        .textures_delta
        .set
        .values()
        .flat_map(|d| d.iter())
        .flat_map(|delta| match &delta.image {
            egui::ImageData::Color(image) => image.pixels.iter().copied(),
        })
        .collect();
    assert!(colors.contains(&Color32::from_rgb(0x12, 0xAB, 0x34)));
    assert!(colors.contains(&Color32::from_rgb(0xAB, 0x12, 0x34)));
    let button = Rect::from_min_size(Pos2::new(50.0, 0.0), Vec2::splat(40.0));
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Rect(rect) if rect.rect == button && rect.brush.is_some() && rect.corner_radius.nw == 6)));
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Mesh(mesh) if mesh.calc_bounds() == button && mesh.vertices.len() > 4)));
    assert!(actions.is_empty(), "painting artwork must not edit parameters");
    let label = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Amount" => text.galley.job.sections.first(),
            _ => None,
        })
        .unwrap();
    assert_eq!(label.format.font_id.size, 17.0);
    assert_eq!(label.format.color, Color32::from_rgb(120, 40, 180));
    output.textures_delta.clear();
    let actions = click(&canvas, &parameters, Pos2::new(70.0, 20.0));
    assert!(actions.iter().any(|action| action.id == "device.setParam" && action.params["param"] == "enabled"));
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
}
#[test]
fn sample_markers_have_one_solid_color_without_an_outline() {
    let context = egui::Context::default();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        sample_marker(&ui.ctx().layer_painter(egui::LayerId::background()), 20., 10., 80.);
    });
    let strokes: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::LineSegment { stroke, .. } => Some(*stroke),
            _ => None,
        })
        .collect();
    assert_eq!(strokes, vec![Stroke::new(2., sample_marker_color())]);
    output.textures_delta.clear();
}

#[test]
fn waveform_zoom_reads_only_the_visible_end_of_a_long_source() {
    let mut samples = vec![0.; 200000];
    samples[199998] = 0.75;
    samples[199999] = -0.5;
    let audio = suhina_engine::AudioData { sample_rate: 48000, channels: 2, samples };
    let peaks = visible_sample_peaks(&audio, (99999, 100001));
    assert_eq!(peaks.len(), 1024);
    assert!(peaks.iter().all(|v| *v == 0.75));
    assert!(visible_sample_peaks(&audio, (100000, 100000)).iter().all(|v| *v == 0.));
}

#[test]
fn invalid_device_graphs_are_cached_without_retaining_input_bytes() {
    GRAPHS.with(|cache| cache.borrow_mut().clear());
    assert!(graph(b"{invalid graph}").is_none());
    GRAPHS.with(|cache| assert_eq!(cache.borrow().len(), 1));
    assert!(graph(b"{invalid graph}").is_none());
    GRAPHS.with(|cache| assert_eq!(cache.borrow().len(), 1));
    for i in 0..100 {
        assert!(graph(format!("invalid {i}").as_bytes()).is_none());
    }
    GRAPHS.with(|cache| assert!(cache.borrow().len() <= 64));
}

#[test]
fn sample_waveform_draws_a_line_for_each_active_voice() {
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        crate::kit::theme::install(ui.ctx());
        let mut positions = [None::<f32>; 32];
        positions[0] = Some(0.2);
        positions[7] = Some(0.7);
        ui.ctx().data_mut(|d| d.insert_temp(sample_source_id(TrackId(1), 0).with("position"), positions));
        sample_waveform(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(550., 180.)),
            &[],
            TrackId(1),
            0,
            serde_json::json!(0),
            &mut Vec::new(),
            false,
        );
    });
    output.textures_delta.clear();
    let heads = output
        .shapes
        .iter()
        .filter(|s| {
            matches!(&s.shape,
                egui::Shape::LineSegment { points, stroke } if *stroke == Stroke::new(1., crate::kit::theme::palette().text_primary)
                    && points[0].x == points[1].x && points[0].y == 4.
            )
        })
        .count();
    assert_eq!(heads, 2);
}

#[test]
fn sample_stop_is_only_visible_while_recording() {
    use egui_kittest::kittest::Queryable;
    for recording in [false, true] {
        let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(600., 220.)).build_ui(move |ui| {
            crate::kit::theme::install(ui.ctx());
            ui.ctx().data_mut(|d| d.insert_temp(sample_source_id(TrackId(1), 0).with("recording"), recording));
            sample_recording(
                ui,
                Rect::from_min_size(Pos2::new(8., 8.), Vec2::new(550., 180.)),
                TrackId(1),
                0,
                &mut Vec::new(),
            );
        });
        harness.run_steps(4);
        assert!(harness.query_by_label("Input: No input").is_some());
        assert_eq!(harness.query_by_label("Stop").is_some(), recording);
    }
}

#[test]
fn historical_sampler_waveforms_keep_recording_controls() {
    use egui_kittest::kittest::Queryable;
    let panel = suhina_device_registry::panel("sampler").unwrap();
    let mut canvas = DeviceUi::clone(panel.ui.as_ref().unwrap());
    canvas.controls.retain(|c| !matches!(c.widget, Some(Widget::SampleRecording)));
    canvas.controls.iter_mut().find(|c| matches!(c.widget, Some(Widget::SampleWaveform))).unwrap().bounds =
        [0., 22., 900., 76.];
    let canvas = Arc::new(canvas);
    let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(916., 184.)).build_ui(move |ui| {
        crate::kit::theme::install(ui.ctx());
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 220.)),
            &canvas,
            &[],
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        );
    });
    harness.run_steps(4);
    assert!(harness.query_by_label("Input: No input").is_some());
    assert!(harness.query_by_label_contains("Record").is_some());
}

#[test]
fn envelope_geometry_is_cumulative_and_padded_at_all_extremes() {
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(284., 40.));
    for adsr in [false, true] {
        for level in [0., 0.5, 1.] {
            for time in [0., 0.001, 0.2, 10.] {
                let plot = EnvelopePlot::new(bounds, [time, 0., level, 0.], if adsr { 30. } else { 20. }, adsr);
                assert_eq!(plot.points[1].x, plot.points[2].x, "zero decay must reach A");
                if adsr {
                    assert_eq!(plot.points[3].x, plot.points[4].x, "zero release must reach S");
                }
                for p in &plot.points[..if adsr { 5 } else { 3 }] {
                    assert!(bounds.shrink(5.).contains(*p), "{p:?}");
                }
                assert!((plot.time(plot.x(time)) - time).abs() < 1e-4);
            }
        }
    }
}

#[test]
fn sampler_tabs_render_with_sdk_controls_and_dispatch() {
    use egui_kittest::kittest::Queryable;
    let panel = suhina_device_registry::panel("sampler").unwrap();
    let canvas = panel.ui.clone().unwrap();
    let mut parameters: Vec<_> = panel.parameters.iter().map(|p| GraphParameter::new(p, p.default)).collect();
    let empty_canvas = canvas.clone();
    let empty_parameters = parameters.clone();
    let mut empty = egui_kittest::Harness::builder().with_size(Vec2::new(916., 184.)).build_ui(move |ui| {
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 220.)),
            &empty_canvas,
            &empty_parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        );
    });
    empty.run_steps(2);
    assert!(empty.query_by_label("Warp").is_none());
    // The middle of the control bound to `param` whose gesture sets `value`, in `canvas`.
    let button = |canvas: &DeviceUi, param: &str, value: u16| {
        let c = canvas
            .controls
            .iter()
            .find(|c| {
                c.x.as_ref().is_some_and(|b| b.parameter == param)
                    && matches!(c.gesture, UiGesture::Set(v) | UiGesture::Tab(v) if v == value)
            })
            .unwrap();
        Pos2::new(c.bounds[0] + c.bounds[2] / 2., c.bounds[1] + c.bounds[3] / 2.)
    };
    let actions = click(&canvas, &parameters, button(&canvas, "mode", 1));
    assert!(actions.iter().any(|a| a.params["param"] == "mode" && a.params["value"] == 1.), "mode");
    // The pages are in the header strip: Controls and More are tabs a second click closes.
    let header = panel.header.clone().unwrap();
    for (current, tab, expected) in [(0., 1, 1.), (1., 1, 0.), (1., 3, 3.), (3., 3, 0.), (3., 0, 0.)] {
        parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = current;
        let actions = click(&header, &parameters, button(&header, "panel_view", tab));
        assert!(
            actions.iter().any(|a| a.params["param"] == "panel_view" && a.params["value"] == expected),
            "{current} -> {expected}: {actions:?}"
        );
    }
    parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 1.;
    for (id, value) in [
        ("attack", 0.1),
        ("decay", 0.2),
        ("sustain", 0.6),
        ("release", 0.3),
        ("filter_attack", 0.1),
        ("filter_decay", 0.2),
        ("filter_sustain", 0.6),
        ("filter_release", 0.3),
        ("pitch_attack", 0.1),
        ("pitch_decay", 0.2),
        ("pitch_env", 12.),
    ] {
        parameters.iter_mut().find(|p| p.id == id).unwrap().value = value;
    }
    for (name, param, index) in [
        ("AMP_envelope", "attack", 1),
        ("AMP_envelope", "decay", 2),
        ("AMP_envelope", "release", 4),
        ("FILTER_envelope", "filter_attack", 1),
        ("FILTER_envelope", "filter_decay", 2),
        ("FILTER_envelope", "filter_release", 4),
        ("PITCH_envelope", "pitch_attack", 1),
        ("PITCH_envelope", "pitch_decay", 2),
    ] {
        let control = canvas.controls.iter().find(|c| c.id == name).unwrap();
        let [x, y, w, h] = control.bounds;
        let adsr = index == 4 || !param.starts_with("pitch");
        let plot = EnvelopePlot::new(
            Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h)),
            [0.1, 0.2, if adsr { 0.6 } else { 0.625 }, 0.3],
            if adsr { 30. } else { 20. },
            adsr,
        );
        let offset = Vec2::new(3., 1.);
        let start = plot.points[index] + offset;
        assert!(click(&canvas, &parameters, start).is_empty(), "click must not jump {name}");
        // Each drag moves its stage to `target`; release goes to 2 s, as 0.5 s is closer than a
        // drag's least distance on the narrow graph.
        let target = if index == 4 { 2. } else { 0.5 };
        let time = target
            + if index == 2 {
                0.1
            } else if index == 4 {
                0.3
            } else {
                0.
            };
        let end = Pos2::new(plot.x(time) + if index == 4 { plot.hold } else { 0. }, plot.points[index].y) + offset;
        let actions = pointer_gesture(&canvas, &parameters, start, Some(end));
        assert!(
            actions
                .iter()
                .any(|a| a.params["param"] == param && (a.params["value"].as_f64().unwrap() - target).abs() < 1e-5),
            "{param}: {actions:?}"
        );
        assert!(actions.iter().any(|a| a.params["continuous"] == false));
        let reset = pointer_gesture_with_clicks(&canvas, &parameters, start, None, 2);
        let default = parameters.iter().find(|p| p.id == param).unwrap().default;
        assert!(
            reset.iter().any(|a| a.params["param"] == param && a.params["value"] == default),
            "reset {param}: {reset:?}"
        );
    }
    for (name, index, param, expected) in [
        // Eight pixels down (past a drag's least distance) of a 36 px graph's 20 px field.
        ("AMP_envelope", 3, "sustain", 0.6 - 8. / 20.),
        ("FILTER_envelope", 3, "filter_sustain", 0.6 - 8. / 20.),
        ("PITCH_envelope", 1, "pitch_env", 12. - 96. * 8. / 20.),
    ] {
        let control = canvas.controls.iter().find(|c| c.id == name).unwrap();
        let [x, y, w, h] = control.bounds;
        let adsr = index == 3;
        let plot = EnvelopePlot::new(
            Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h)),
            [0.1, 0.2, if adsr { 0.6 } else { 0.625 }, 0.3],
            if adsr { 30. } else { 20. },
            adsr,
        );
        let start = plot.points[index];
        let actions = pointer_gesture(&canvas, &parameters, start, Some(start + Vec2::new(0., 8.)));
        assert!(
            actions
                .iter()
                .any(|a| a.params["param"] == param && (a.params["value"].as_f64().unwrap() - expected).abs() < 1e-5),
            "vertical {param}: {actions:?}"
        );
    }
    for (id, value) in [("attack", 0.), ("decay", 0.), ("sustain", 0.78)] {
        parameters.iter_mut().find(|p| p.id == id).unwrap().value = value;
    }
    let zero = EnvelopePlot::new(
        {
            let c = canvas.controls.iter().find(|c| c.id == "AMP_envelope").unwrap();
            Rect::from_min_size(Pos2::new(c.bounds[0], c.bounds[1]), Vec2::new(c.bounds[2], c.bounds[3]))
        },
        [0., 0., 0.78, 0.3],
        30.,
        true,
    );
    let actions =
        pointer_gesture(&canvas, &parameters, zero.points[2], Some(Pos2::new(zero.x(0.05), zero.points[2].y)));
    assert!(
        actions
            .iter()
            .any(|a| a.params["param"] == "decay" && (a.params["value"].as_f64().unwrap() - 0.05).abs() < 1e-5)
    );
    assert!(!actions.iter().any(|a| a.params["param"] == "attack"));
    parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 1.;
    let preview = canvas.controls.iter().find(|c| c.id == "lfo_preview").unwrap().bounds;
    // The rate follows the pointer across the drawing: half way is 0.01 · 2000^0.5 Hz.
    let from = Pos2::new(preview[0] + 20., preview[1] + preview[3] / 2.);
    let actions = pointer_gesture(&canvas, &parameters, from, Some(Pos2::new(preview[0] + preview[2] / 2., from.y)));
    assert!(actions.iter().any(
        |a| a.params["param"] == "lfo_rate" && (a.params["value"].as_f64().unwrap() - 0.4472136).abs() < 0.001
    ));
    assert!(!actions.iter().any(|a| a.params["param"] == "lfo_pitch"));
    parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 0.;
    let middle = |param: &str| {
        let c = canvas
            .controls
            .iter()
            .find(|c| c.x.as_ref().or(c.y.as_ref()).is_some_and(|b| b.parameter == param))
            .unwrap();
        Pos2::new(c.bounds[0] + c.bounds[2] / 2., c.bounds[1] + c.bounds[3] / 2.)
    };
    let actions = click(&canvas, &parameters, middle("warp"));
    assert!(actions.iter().any(|a| a.params["param"] == "warp" && a.params["value"] == 1.));
    let gain = middle("gain");
    let actions = pointer_gesture(&canvas, &parameters, gain, Some(gain - Vec2::new(0., 10.)));
    assert!(
        actions
            .iter()
            .any(|a| a.params["param"] == "gain" && (a.params["value"].as_f64().unwrap() - 1.8).abs() < 0.001),
        "{actions:?}"
    );
    assert!(
        canvas
            .controls
            .iter()
            .any(|c| c.x.as_ref().is_some_and(|b| b.parameter == "filter_slope")
                && matches!(c.widget, Some(Widget::Menu)))
    );
    for (mode, playback, param) in [(0., 0., "start"), (0., 1., "start"), (0., 2., "loop_start"), (1., 2., "start")] {
        parameters.iter_mut().find(|p| p.id == "mode").unwrap().value = mode;
        parameters.iter_mut().find(|p| p.id == "playback").unwrap().value = playback;
        // The S marker at the waveform's left (62 + 6) and 6 px into its field (4 + 6); a quarter
        // of the field's 826 px further.
        let actions = pointer_gesture(&canvas, &parameters, Pos2::new(68., 10.), Some(Pos2::new(68. + 206.5, 10.)));
        assert!(
            actions
                .iter()
                .any(|a| a.params["param"] == param && (a.params["value"].as_f64().unwrap() - 0.25).abs() < 0.001),
            "{param}: {actions:?}"
        );
        assert!(!actions.iter().any(|a| a.params["param"] == if param == "start" { "loop_start" } else { "start" }));
    }
    for view in [0., 1., 2.] {
        parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = view;
        parameters.iter_mut().find(|p| p.id == "mode").unwrap().value = 0.;
        parameters.iter_mut().find(|p| p.id == "playback").unwrap().value = 2.;
        for control in &canvas.controls {
            if control.widget.is_none() || matches!(control.widget, Some(Widget::Number { bordered: true, .. })) {
                continue;
            }
            let Some(binding) = control.x.as_ref().or(control.y.as_ref()) else {
                continue;
            };
            if control.visible_when.as_ref().is_some_and(|v| {
                v.evaluate(&|id, norm, log| {
                    parameters
                        .iter()
                        .find(|p| p.id == id)
                        .map_or(0., |p| if norm { normalized(p, log) } else { p.value })
                }) <= 0.
            }) {
                continue;
            }
            let mut edited = parameters.clone();
            let p = edited.iter_mut().find(|p| p.id == binding.parameter).unwrap();
            // Playback's last value (Slices) puts the slice controls in its place; One shot keeps it.
            p.value = if p.id == "playback" {
                1.
            } else if p.default == p.min {
                p.max
            } else {
                p.min
            };
            let expected = p.default;
            let [x, y, w, h] = control.bounds;
            let actions = pointer_gesture_with_clicks(&canvas, &edited, Pos2::new(x + w / 2., y + h / 2.), None, 2);
            assert!(
                actions
                    .iter()
                    .rev()
                    .find(|a| a.params["param"] == binding.parameter)
                    .is_some_and(|a| a.params["value"] == expected),
                "double-click {}: {actions:?}",
                control.id
            );
        }
    }
    for playback in [0., 1., 2.] {
        parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 3.;
        parameters.iter_mut().find(|p| p.id == "playback").unwrap().value = playback;
        for id in ["loop_start", "loop_end"] {
            let visible = canvas.controls.iter().filter(|c| c.y.as_ref().is_some_and(|b| b.parameter == id)).any(|c| {
                c.visible_when
                    .as_ref()
                    .unwrap()
                    .evaluate(&|id, _, _| parameters.iter().find(|p| p.id == id).unwrap().value)
                    > 0.
            });
            assert!(visible);
        }
    }
    parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 0.;
    for (id, expected) in [("cutoff", 3100. * 1000_f64.powf(0.05)), ("resonance", 0.707 + 9.9 * 0.05)] {
        let at = middle(id);
        let actions = pointer_gesture(&canvas, &parameters, at, Some(at - Vec2::new(0., 10.)));
        assert!(
            actions
                .iter()
                .any(|a| a.params["param"] == id && (a.params["value"].as_f64().unwrap() - expected).abs() < 1e-6),
            "{id}: {actions:?}"
        );
    }
    for (view, mode, filename) in [
        (0., 0., "sampler-simple.png"),
        (0., 0., "sampler-loop.png"),
        (1., 0., "sampler-envelopes.png"),
        (1., 0., "sampler-envelopes-zero-light.png"),
        (0., 1., "sampler-grain.png"),
        (2., 1., "sampler-lfo.png"),
    ] {
        parameters.iter_mut().find(|p| p.id == "source_bpm").unwrap().value = 120.;
        parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = view;
        parameters.iter_mut().find(|p| p.id == "mode").unwrap().value = mode;
        parameters.iter_mut().find(|p| p.id == "lfo_pitch").unwrap().value = 3.;
        parameters.iter_mut().find(|p| p.id == "playback").unwrap().value =
            if filename == "sampler-simple.png" { 0. } else { 2. };
        for (id, normal) in [
            ("attack", 0.1),
            ("decay", 0.2),
            ("filter_attack", 0.1),
            ("filter_decay", 0.2),
            ("pitch_attack", 0.1),
            ("pitch_decay", 0.2),
        ] {
            parameters.iter_mut().find(|p| p.id == id).unwrap().value =
                if filename.contains("zero") { 0. } else { normal };
        }
        let light =
            crate::kit::theme::built_in_themes().into_iter().find(|(id, _)| *id == "suhina-light").unwrap().1.colors;
        let preview_canvas = canvas.clone();
        let preview_parameters = parameters.clone();
        let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(916., 184.)).build_ui(move |ui| {
            crate::kit::theme::install(ui.ctx());
            if filename.contains("light") {
                crate::kit::theme::set_palette(ui.ctx(), light);
            }
            let data = Arc::new(suhina_engine::AudioData {
                sample_rate: 48000,
                channels: 1,
                samples: (0..4800).map(|i| (i as f32 * 0.17).sin() * (1. - i as f32 / 4800.)).collect(),
            });
            ui.ctx().data_mut(|d| d.insert_temp(sample_source_id(TrackId(1), 0), data));
            ui.ctx().data_mut(|d| {
                let mut positions = [None::<f32>; 32];
                positions[0] = Some(0.22);
                positions[1] = Some(0.67);
                d.insert_temp(sample_source_id(TrackId(1), 0).with("position"), positions);
                d.insert_temp(sample_source_id(TrackId(1), 0).with("name"), "Sampler-test.wav".to_string());
            });
            show(
                ui,
                Rect::from_min_size(Pos2::new(8., 8.), Vec2::new(900., 168.)),
                &preview_canvas,
                &preview_parameters,
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut Vec::new(),
            );
        });
        harness.run_steps(4);
        snapshot(&mut harness, filename);
        if filename == "sampler-simple.png" {
            assert!(harness.query_by_label("Warp").is_some());
            harness.get_by_label("warp mode").click();
            harness.run_steps(4);
            for mode in ["Repitch", "Rhythmic", "Tonal", "Texture", "Smooth", "HighQuality"] {
                assert!(harness.query_by_label(mode).is_some(), "{mode}");
            }
            snapshot(&mut harness, "sampler-warp-menu.png");
        }
        crate::kit::theme::set_palette(&harness.ctx, crate::kit::theme::Palette::DARK);
        if view == 2. {
            harness.get_by_label("lfo shape").click();
            harness.run_steps(4);
            assert!(harness.query_by_label("Random").is_some());
            snapshot(&mut harness, "sampler-lfo-menu.png");
            harness.get_by_label("Random").click();
            harness.run_steps(4);
            assert!(harness.query_by_label("Random").is_none());
        }
    }
}
fn parameter(id: &str, logarithmic: bool) -> GraphParameter {
    GraphParameter {
        text: String::new(),
        default: 0.5,
        ui: Default::default(),
        control: Default::default(),
        unit: String::new(),
        id: id.into(),
        name: id.into(),
        min: if logarithmic { 20.0 } else { 0.0 },
        max: if logarithmic { 20000.0 } else { 1.0 },
        value: if logarithmic { 1000.0 } else { 0.5 },
    }
}
#[test]
fn delay_panel_renders_and_selector_dispatches_a_literal() {
    let graph = super::delay_example::build().unwrap();
    let parameters: Vec<_> = graph.parameters.iter().map(|p| GraphParameter::new(p, p.default)).collect();
    let canvas = Arc::new(graph.ui.unwrap());
    let preview_canvas = canvas.clone();
    let preview_parameters = parameters.clone();
    let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(956., 180.)).build_ui(move |ui| {
        crate::kit::theme::install(ui.ctx());
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
            &preview_canvas,
            &preview_parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        );
    });
    harness.run();
    for control in &canvas.controls {
        for drawing in control.drawings().iter() {
            if let Draw::Text { text, size, parameter: None, position, align, .. } = drawing {
                let galley = harness.ctx.fonts_mut(|fonts| {
                    fonts.layout_no_wrap(text.clone(), egui::FontId::proportional(*size), Color32::WHITE)
                });
                let x = position[0].evaluate(&|_, _, _| 0.);
                if x < 0. || x > control.bounds[2] {
                    continue;
                }
                let width =
                    if *align == TextAlign::Center { 2. * x.min(control.bounds[2] - x) } else { control.bounds[2] - x };
                assert!(
                    galley.size().x <= width + 0.5,
                    "{} caption clipped: {} > {}",
                    control.id,
                    galley.size().x,
                    width
                );
            }
        }
    }
    snapshot(&mut harness, "delay-panel.png");
    let nord =
        crate::kit::theme::built_in_themes().into_iter().find(|(id, _)| *id == "nord").map(|(_, theme)| theme).unwrap();
    crate::kit::theme::set_palette(&harness.ctx, nord.colors);
    harness.run();
    snapshot(&mut harness, "delay-panel-nord.png");
    crate::kit::theme::set_palette(&harness.ctx, crate::kit::theme::Palette::DARK);
    use egui_kittest::kittest::Queryable;
    let mut narrow_parameters = parameters.clone();
    for p in &mut narrow_parameters {
        p.value = match p.id.as_str() {
            "cutoff" => 4010.0,
            "width" => 2.4,
            "offset_left" => -33.3,
            "offset_right" => 33.3,
            _ => p.value,
        };
    }
    let narrow_canvas = canvas.clone();
    let mut narrow = egui_kittest::Harness::builder().with_size(Vec2::new(956., 180.)).build_ui(move |ui| {
        crate::kit::theme::install(ui.ctx());
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
            &narrow_canvas,
            &narrow_parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        );
    });
    narrow.run();
    narrow.get_by_label("Feedback").click();
    narrow.run();
    snapshot(&mut narrow, "delay-focus.png");
    let mut time_parameters = parameters.clone();
    for p in &mut time_parameters {
        if p.id.starts_with("sync_") {
            p.value = 0.0;
        }
        if p.id.starts_with("time_") {
            p.value = 427.0;
        }
    }
    let time_canvas = canvas.clone();
    let mut time_harness = egui_kittest::Harness::builder().with_size(Vec2::new(956., 180.)).build_ui(move |ui| {
        crate::kit::theme::install(ui.ctx());
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
            &time_canvas,
            &time_parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        );
    });
    time_harness.run();
    snapshot(&mut time_harness, "delay-time-panel.png");
    let start = Pos2::new(64., 110.);
    let clicked = click(&canvas, &parameters, start);
    assert!(
        clicked.iter().all(|a| a.params["param"] == "offset_left" && a.params["value"] == 0.0),
        "clicking a drag field must preserve its value"
    );
    let offset = pointer_gesture(&canvas, &parameters, start, Some(start - Vec2::new(0., 12.)));
    assert!(
        offset
            .iter()
            .any(|a| a.params["param"] == "offset_left" && (a.params["value"].as_f64().unwrap() - 1.998).abs() < 1e-6)
    );
    assert!(offset.iter().any(|a| a.params["param"] == "offset_left" && a.params["continuous"] == false));
    let actions = click(&canvas, &parameters, Pos2::new(100., 151.));
    assert!(actions.iter().any(|a| a.params["param"] == "mode" && a.params["value"] == 1.0));
    let mut time_parameters = parameters.clone();
    for p in &mut time_parameters {
        if p.id.starts_with("sync_") {
            p.value = 0.0;
        }
    }
    assert!(
        click(&canvas, &time_parameters, Pos2::new(89., 59.))
            .iter()
            .all(|a| !a.params["param"].as_str().unwrap_or_default().starts_with("division_")),
        "hidden tempo field must not intercept clicks"
    );
    let actions = click(&canvas, &time_parameters, Pos2::new(35., 59.));
    assert!(actions.iter().any(|a| a.params["param"] == "sync_left" && a.params["value"] == 1.0));
}
#[test]
fn delay_menus_open_choose_and_cancel_without_spurious_edits() {
    use egui_kittest::kittest::Queryable;
    let graph = super::delay_example::build().unwrap();
    let parameters: Vec<_> = graph.parameters.iter().map(|p| GraphParameter::new(p, p.default)).collect();
    let canvas = Arc::new(graph.ui.unwrap());
    let reset_canvas = canvas.clone();
    let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(956., 210.)).build_ui_state(
        move |ui, state: &mut (Vec<GraphParameter>, Vec<Action>)| {
            crate::kit::theme::install(ui.ctx());
            let mut actions = Vec::new();
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
                &canvas,
                &state.0,
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut actions,
            );
            for a in &actions {
                if let Some(p) = state.0.iter_mut().find(|p| a.params["param"] == p.id) {
                    p.value = a.params["value"].as_f64().unwrap();
                }
            }
            state.1.extend(actions);
        },
        (parameters, Vec::new()),
    );
    harness.run();
    harness.get_by_label("Motion waveform").click();
    harness.run();
    assert!(harness.state().1.is_empty(), "opening Wave must not edit the parameter");
    snapshot(&mut harness, "delay-wave-menu.png");
    harness.get_by_label("Wander").click();
    harness.run();
    assert_eq!(harness.state().1.len(), 1);
    assert_eq!(harness.state().1[0].params["param"], "mod_shape");
    assert_eq!(harness.state().1[0].params["value"], 6.0);
    assert_eq!(harness.state().1[0].params["continuous"], false);
    harness.get_by_label("Motion rate mode").click();
    harness.run();
    assert_eq!(harness.state().1.len(), 1);
    snapshot(&mut harness, "delay-rate-menu.png");
    harness.get_by_label("Triplet").click();
    harness.run();
    assert_eq!(harness.state().1.len(), 2);
    assert_eq!(harness.state().1[1].params["param"], "mod_rate_mode");
    assert_eq!(harness.state().1[1].params["value"], 3.0);
    harness.get_by_label("Motion rate mode").click();
    harness.run();
    harness.key_combination(&[egui::Key::Escape]);
    harness.run();
    assert_eq!(harness.state().1.len(), 2, "Escape must not edit a parameter");
    assert!(harness.query_by_label("Motion details").is_none());
    assert!(harness.query_by_label("Motion waveform").is_some());
    assert!(harness.query_by_label("Morph").is_some());
    assert!(harness.query_by_label("Delay depth").is_some());
    assert!(harness.query_by_label("Filter depth").is_some());
    harness.get_by_label("Motion rhythm").click();
    harness.run();
    assert_eq!(harness.state().1.len(), 2);
    snapshot(&mut harness, "delay-rhythm-menu.png");
    let before = harness.get_by_label("1/8 T").rect();
    harness.hover_at(egui::pos2(before.right() + 8., before.center().y));
    harness.run();
    assert_eq!(before, harness.get_by_label("1/8 T").rect(), "hovering the scroll bar must not resize menu content");
    snapshot(&mut harness, "delay-rhythm-hover.png");
    harness.get_by_label("1/8 T").click();
    harness.run();
    assert_eq!(harness.state().1.last().unwrap().params["param"], "mod_division");
    assert_eq!(harness.state().1.last().unwrap().params["value"], 8.);
    harness.get_by_label("Motion rate mode").click();
    harness.run();
    harness.get_by_label("16th steps").click();
    harness.run();
    harness.get_by_label("Motion rhythm").click();
    harness.run();
    harness.get_by_label("3 x 1/16").click();
    harness.run();
    assert_eq!(harness.state().1.last().unwrap().params["value"], 3.);
    harness.get_by_label("Motion rate mode").click();
    harness.run();
    harness.get_by_label("Time").click();
    harness.run();
    assert!(harness.query_by_label("Motion rhythm").is_none());
    assert!(harness.query_by_label("mod time").is_some());
    assert!(harness.query_by_label("motion_time").is_none());
    harness.get_by_label("Left delay").click();
    harness.run();
    harness.get_by_label("1/4").click();
    harness.run();
    assert_eq!(harness.state().1.last().unwrap().params["param"], "division_left");
    assert_eq!(harness.state().1.last().unwrap().params["value"], 4.);
    harness.get_by_label("Left timing mode").click();
    harness.run();
    assert!(harness.query_by_label("Left delay").is_none());
    assert!(harness.query_by_label("time left").is_some());
    snapshot(&mut harness, "delay-motion.png");

    let feedback = |parameters: &[GraphParameter]| parameters.iter().find(|p| p.id == "feedback").unwrap().value;
    harness.get_by_label("Feedback").click();
    harness.run();
    let before = feedback(&harness.state().0);
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    let normal = feedback(&harness.state().0);
    assert!(normal > before, "the constructed dial accepts keyboard edits");
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowUp);
    harness.run();
    let fine = feedback(&harness.state().0);
    assert!(fine > normal && fine - normal < normal - before, "Shift makes the keyboard step finer");
    let at = harness.get_by_label("Feedback").rect().center();
    let reset = pointer_gesture_with_clicks(&reset_canvas, &harness.state().0, at, None, 2);
    assert!(reset.iter().any(|a| a.params["param"] == "feedback" && a.params["value"] == before));
    for action in reset {
        if let Some(p) = harness.state_mut().0.iter_mut().find(|p| action.params["param"] == p.id) {
            p.value = action.params["value"].as_f64().unwrap();
        }
        harness.state_mut().1.push(action);
    }
    assert_eq!(feedback(&harness.state().0), before, "double click restores the default");
    assert!(harness.state().1.iter().any(|a| a.params["param"] == "feedback" && a.params["continuous"] == false));
}
/// Saves a picture of `harness` to `target/` for the design review when
/// `SUHINA_UI_SNAPSHOTS` is set; drawing it needs a GPU adapter, which test machines may lack.
#[test]
fn sampler_slice_controls_replace_loop_fields() {
    let panel = suhina_device_registry::panel("sampler").unwrap();
    let canvas = panel.ui.clone().unwrap();
    let parameters: Vec<_> = panel
        .parameters
        .iter()
        .map(|p| GraphParameter::new(p, if p.id == "playback" { 3. } else { p.default }))
        .collect();
    for (id, expected) in [("loop_start", false), ("loop_end", false), ("sensitivity", true), ("slice_by", true)] {
        let visible =
            canvas.controls.iter().filter(|c| c.x.iter().chain(c.y.iter()).any(|b| b.parameter == id)).any(|c| {
                c.visible_when
                    .as_ref()
                    .is_none_or(|v| v.evaluate(&|id, _, _| parameters.iter().find(|p| p.id == id).unwrap().value) > 0.)
            });
        assert_eq!(visible, expected, "{id}");
    }
    let sensitivity =
        canvas.controls.iter().find(|c| c.y.as_ref().is_some_and(|b| b.parameter == "sensitivity")).unwrap();
    let [x, y, w, h] = sensitivity.bounds;
    let center = Pos2::new(x + w / 2., y + h / 2.);
    let actions = pointer_gesture(&canvas, &parameters, center, Some(center - Vec2::new(0., 20.)));
    assert!(actions.iter().any(|a| a.params["param"] == "sensitivity"));
    let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(916., 184.)).build_ui(move |ui| {
        crate::kit::theme::install(ui.ctx());
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 220.)),
            &canvas,
            &parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        );
    });
    harness.run_steps(2);
    snapshot(&mut harness, "sampler-slices.png");
}
#[test]
fn waveform_manual_edit_navigation_and_inactive_colors() {
    let panel = suhina_device_registry::panel("sampler").unwrap();
    let canvas = panel.ui.clone().unwrap();
    let parameters: Vec<_> = panel
        .parameters
        .iter()
        .map(|p| {
            let value = match p.id.as_str() {
                "playback" | "slice_by" => 3.,
                "start" => 0.2,
                "end" => 0.8,
                _ => p.default,
            };
            GraphParameter::new(p, value)
        })
        .collect();
    let ctx = egui::Context::default();
    let id = sample_source_id(TrackId(1), 0);
    let source = Arc::new(suhina_engine::AudioData {
        sample_rate: 48000,
        channels: 1,
        samples: (0..48000).map(|i| (i as f32 * 0.05).sin()).collect(),
    });
    ctx.data_mut(|d| {
        d.insert_temp(id, source.clone());
        d.insert_temp(id.with("manual-points"), Arc::<[f64]>::from([0.3, 0.5]));
    });
    let mut edits = Vec::new();
    let mut time = 0.;
    let mut frame = |position: Pos2, pressed: Option<bool>| {
        let edits_before = edits.len();
        time += 0.05;
        let mut events = vec![egui::Event::PointerMoved(position)];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(916., 184.))),
                events,
                ..Default::default()
            },
            |ui| {
                crate::kit::theme::install(ui.ctx());
                show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 220.)),
                    &canvas,
                    &parameters,
                    TrackId(1),
                    &crate::detail::detail_view::DeviceAt(vec![0]),
                    &mut actions,
                );
            },
        );
        output.textures_delta.clear();
        for action in actions {
            if action.id == "device.setSampleSlices" {
                let mut points: Vec<_> =
                    action.params["points"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
                points.sort_unstable_by(f64::total_cmp);
                points.dedup();
                ctx.data_mut(|d| d.insert_temp(id.with("manual-points"), Arc::<[f64]>::from(points)));
                edits.push(action);
            } else if !action.id.starts_with("preview.") {
                // A click on a slice plays it (preview); anything else is not expected.
                panic!("unexpected edit {action:?}");
            }
        }
        if pressed != Some(false) {
            assert_eq!(edits.len(), edits_before, "drag preview must not dispatch project edits");
        }
        output
    };
    // The waveform at (62, 0) of a canvas drawn at (0, 0), its field 6 px in at the sides and 4 at
    // the top and bottom.
    let field = Rect::from_min_max(Pos2::new(68., 4.), Pos2::new(894., 92.));
    let line = Pos2::new(SampleView::default().x(0.3, field), 80.);
    frame(line, None);
    let output = frame(line, None);
    assert_eq!(output.platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
    assert!(
        output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::LineSegment {stroke,..} if stroke.color==crate::kit::theme::palette().text_primary.gamma_multiply(0.18)))
    );
    frame(line, Some(true));
    let moved = Pos2::new(SampleView::default().x(0.6, field), 80.);
    let preview = frame(moved, None);
    assert_eq!(ctx.data(|d| d.get_temp::<Arc<[f64]>>(id.with("manual-points"))).unwrap().as_ref(), &[0.3, 0.5]);
    assert!(
        preview.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::LineSegment { points, stroke }
                if stroke.color == slice_line(sample_marker_color()) && (points[0].x - moved.x).abs() < 0.01
        )),
        "the marker follows the pointer before the command is committed"
    );
    frame(moved, Some(false));
    let points = ctx.data(|d| d.get_temp::<Arc<[f64]>>(id.with("manual-points"))).unwrap();
    assert_eq!(points.len(), 2);
    assert!((points[1] - 0.6).abs() < 1e-6, "{points:?}");
    assert_eq!(points[0], 0.5);
    frame(moved, None);
    for _ in 0..2 {
        frame(moved, Some(true));
        frame(moved, Some(false));
    }
    assert_eq!(ctx.data(|d| d.get_temp::<Arc<[f64]>>(id.with("manual-points"))).unwrap().as_ref(), &[0.5]);
    let empty = Pos2::new(SampleView::default().x(0.4, field), 80.);
    frame(empty, None);
    frame(empty, Some(true));
    frame(empty, Some(false));
    frame(empty, Some(true));
    frame(empty, Some(false));
    let points = ctx.data(|d| d.get_temp::<Arc<[f64]>>(id.with("manual-points"))).unwrap();
    assert_eq!(points.len(), 2);
    assert!((points[0] - 0.4).abs() < 1e-6);
    let blank = Pos2::new(SampleView::default().x(0.7, field), 34.);
    frame(blank, None);
    frame(blank, Some(true));
    // Dragging the waveform away from a line: down zooms in, sideways moves.
    frame(blank + Vec2::new(-25., 20.), None);
    frame(blank + Vec2::new(-25., 20.), Some(false));
    let view =
        ctx.data(|d| d.get_temp::<(Option<Arc<suhina_engine::AudioData>>, SampleView)>(id.with("view"))).unwrap().1;
    assert!(view.span < 1.);
    assert!(view.start > 0.);
    for point in [0., 0.2, 0.4, 1.] {
        assert!((view.point(view.x(point, field), field) - point).abs() < 1e-6);
    }
    ctx.data_mut(|d| {
        d.insert_temp(id.with("view"), (Some(source.clone()), SampleView { start: 0.3, span: 4. / 48000. }))
    });
    frame(blank, None);
    let peaks = ctx.data(|d| d.get_temp::<SamplePeaks>(id.with("visible-peaks"))).unwrap();
    assert!(
        peaks.peaks.iter().filter(|p| **p > 0.1).count() > 500,
        "zoomed samples fill the viewport instead of leaving empty bins"
    );
    assert_eq!(edits.len(), 3, "navigation must not emit project edits; line drag commits once on release");
    assert_eq!(edits[0].params["continue"], false);
    assert!(edits.iter().all(|a| a.params["continue"] == false));
    let zoomed = view.dragged(blank, Vec2::new(1e6, -1e6), field, 48000);
    assert_eq!(zoomed, SampleView::default());
    let zoomed = view.dragged(blank, Vec2::new(-1e6, 1e6), field, 48000);
    assert!(zoomed.span >= 1. / 48000.);
    assert!(zoomed.start + zoomed.span <= 1.);
    ctx.data_mut(|d| {
        d.insert_temp(id, Arc::new(suhina_engine::AudioData { sample_rate: 48000, channels: 1, samples: vec![0.; 32] }))
    });
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 220.)),
            &canvas,
            &parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        )
    });
    output.textures_delta.clear();
    assert_eq!(
        ctx.data(|d| d.get_temp::<(Option<Arc<suhina_engine::AudioData>>, SampleView)>(id.with("view"))).unwrap().1,
        SampleView::default()
    );
    let snapshot_source = source.clone();
    let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(916., 184.)).build_ui(move |ui| {
        crate::kit::theme::install(ui.ctx());
        ui.ctx().data_mut(|d| {
            d.insert_temp(id, snapshot_source.clone());
            d.insert_temp(
                id.with("manual-points"),
                Arc::<[f64]>::from((1..30).map(|i| i as f64 / 30.).collect::<Vec<_>>()),
            );
        });
        show(
            ui,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 220.)),
            &canvas,
            &parameters,
            TrackId(1),
            &crate::detail::detail_view::DeviceAt(vec![0]),
            &mut Vec::new(),
        );
    });
    harness.run_steps(2);
    snapshot(&mut harness, "sampler-manual-navigation.png");
}

#[test]
fn automatic_boundary_drag_dispatches_one_correction_on_release() {
    let panel = suhina_device_registry::panel("sampler").unwrap();
    let canvas = panel.ui.clone().unwrap();
    let parameters: Vec<_> = panel
        .parameters
        .iter()
        .map(|parameter| {
            let value = match parameter.id.as_str() {
                "mode" | "slice_by" => 2.,
                "slice_regions" => 4.,
                _ => parameter.default,
            };
            GraphParameter::new(parameter, value)
        })
        .collect();
    // The waveform at (62, 0) of a canvas drawn at (0, 0), its field 6 px in at the sides and 4 at
    // the top and bottom.
    let field = Rect::from_min_max(Pos2::new(68., 4.), Pos2::new(894., 92.));
    let from = Pos2::new(SampleView::default().x(0.25, field), 80.);
    let to = Pos2::new(SampleView::default().x(0.3, field), 80.);
    let actions = pointer_gesture(&canvas, &parameters, from, Some(to));
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0].id, "device.editSampleSlice");
    assert_eq!(actions[0].params["operation"], "move");
    assert_eq!(actions[0].params["identity"], 1);
    assert!((actions[0].params["point"].as_f64().unwrap() - 0.3).abs() < 1e-6);
}

#[test]
fn stereo_peaks_preserve_separate_channel_levels() {
    let source = suhina_engine::AudioData { sample_rate: 48000, channels: 2, samples: [0.1, 0.8].repeat(16) };
    assert!(sample_channel_peaks(&source, (0, 16), Some(0)).iter().all(|peak| *peak == 0.1));
    assert!(sample_channel_peaks(&source, (0, 16), Some(1)).iter().all(|peak| *peak == 0.8));
}

#[test]
fn exactly_one_sampler_mode_is_selected_for_each_mode() {
    let panel = suhina_device_registry::panel("sampler").unwrap();
    let canvas = panel.ui.as_ref().unwrap();
    for mode in [0., 1., 2.] {
        let parameters: Vec<_> = panel
            .parameters
            .iter()
            .map(|p| GraphParameter::new(p, if p.id == "mode" { mode } else { p.default }))
            .collect();
        let selected: Vec<_> = canvas
            .controls
            .iter()
            .filter(|c| c.x.as_ref().is_some_and(|b| b.parameter == "mode") && control_selected(c, &parameters))
            .collect();
        assert_eq!(selected.len(), 1);
        assert!(matches!(selected[0].gesture, UiGesture::Set(target) if f64::from(target) == mode));
    }
}

#[test]
fn sampler_fits_fixed_height_without_shrinking_parameter_rows() {
    let panel = suhina_device_registry::panel("sampler").unwrap();
    let canvas = panel.ui.as_ref().unwrap();
    for available in [168., 180., 200., 220.] {
        let size = fitted_size(canvas, available);
        assert_eq!(size.y, 168., "the compact sampler never grows the fixed device row");
        let reduction = canvas.size[1] - size.y;
        for control in &canvas.controls {
            let [_, y, _, height] = fitted_control_bounds(control, reduction);
            assert!(y >= 0. && y + height <= available, "{} outside {available}px: {y}+{height}", control.id);
            if control.bounds[1] >= 138. {
                assert_eq!(height, control.bounds[3], "parameter row must retain readable size");
            }
        }
    }
    assert_eq!(fitted_size(canvas, 120.).y, 168., "smaller hosts scroll instead of crushing controls");
}

#[test]
fn unknown_tempo_has_a_time_grid_under_its_times_whatever_the_grid() {
    let panel = suhina_device_registry::panel("sampler").unwrap();
    for grid in [0., 3.] {
        let parameters: Vec<_> = panel
            .parameters
            .iter()
            .map(|p| GraphParameter::new(p, if p.id == "grid_division" { grid } else { p.default }))
            .collect();
        let context = egui::Context::default();
        let mut output = context.run_ui(Default::default(), |ui| {
            crate::kit::theme::install(ui.ctx());
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    sample_source_id(TrackId(1), 0),
                    Arc::new(suhina_engine::AudioData { sample_rate: 48000, channels: 1, samples: vec![0.; 20544] }),
                )
            });
            sample_waveform(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(832., 110.)),
                &parameters,
                TrackId(1),
                0,
                serde_json::json!(0),
                &mut Vec::new(),
                false,
            );
        });
        let lines = output.shapes.iter().filter(|shape| matches!(&shape.shape, egui::Shape::LineSegment { points, stroke } if points[0].x == points[1].x && *stroke == Stroke::new(1., crate::kit::theme::palette().text_primary.gamma_multiply(0.09)))).count();
        output.textures_delta.clear();
        // The times sit on their lines, so the lines stay with Grid off too.
        assert!(lines > 2, "grid setting {grid}: {lines} lines");
    }
}
fn snapshot<State>(harness: &mut egui_kittest::Harness<'_, State>, name: &str) {
    if std::env::var_os("SUHINA_UI_SNAPSHOTS").is_some() {
        harness.render().unwrap().save(format!("../../target/{name}")).unwrap();
    }
}
fn click(canvas: &Arc<DeviceUi>, parameters: &[GraphParameter], position: Pos2) -> Vec<Action> {
    pointer_gesture(canvas, parameters, position, None)
}
fn pointer_gesture(
    canvas: &Arc<DeviceUi>,
    parameters: &[GraphParameter],
    position: Pos2,
    drag_to: Option<Pos2>,
) -> Vec<Action> {
    pointer_gesture_with_clicks(canvas, parameters, position, drag_to, 1)
}
fn pointer_gesture_with_clicks(
    canvas: &Arc<DeviceUi>,
    parameters: &[GraphParameter],
    position: Pos2,
    drag_to: Option<Pos2>,
    clicks: usize,
) -> Vec<Action> {
    let ctx = egui::Context::default();
    ctx.data_mut(|d| {
        d.insert_temp(
            sample_source_id(TrackId(1), 0),
            Arc::new(suhina_engine::AudioData { sample_rate: 48000, channels: 1, samples: vec![0.; 1024] }),
        )
    });
    let mut actions = Vec::new();
    let mut frames = vec![(position, None), (position, Some(true))];
    if let Some(end) = drag_to {
        frames.push((end, None));
    }
    frames.push((drag_to.unwrap_or(position), Some(false)));
    for _ in 1..clicks {
        frames.push((position, Some(true)));
        frames.push((position, Some(false)));
    }
    for (position, pressed) in frames {
        let events = pressed.map_or_else(
            || vec![egui::Event::PointerMoved(position)],
            |pressed| {
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ]
            },
        );
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(canvas.size[0] + 12., canvas.size[1] + 12.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(canvas.size[0], canvas.size[1])),
                    canvas,
                    parameters,
                    TrackId(1),
                    &crate::detail::detail_view::DeviceAt(vec![0]),
                    &mut actions,
                )
            },
        );
        output.textures_delta.clear();
    }
    actions
}
#[test]
fn slow_drag_on_an_integer_control_adds_up_across_frames() {
    let canvas = Arc::new(DeviceUi {
        size: [60.0, 60.0],
        controls: vec![CustomControl {
            widget: None,
            visible_when: None,
            id: "feedback".into(),
            name: "Feedback".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 40.0, 40.0],
            gesture: UiGesture::Drag,
            x: None,
            y: Some(UiBinding::new("feedback")),
            drawing: vec![],
        }],

        ..Default::default()
    });
    let mut parameters = vec![GraphParameter {
        control: suhina_device_sdk::ParameterControl::Integer,
        max: 7.0,
        value: 0.0,
        ..parameter("feedback", false)
    }];
    let ctx = egui::Context::default();
    let start = Pos2::new(20., 20.);
    let mut frames = vec![(start, None), (start, Some(true))];
    // 40 px up in 1 px steps: each step is far below half a step of the 0..7 range.
    frames.extend((1..=40).map(|i| (start - Vec2::new(0., i as f32), None)));
    frames.push((start - Vec2::new(0., 40.), Some(false)));
    for (position, pressed) in frames {
        let mut events = vec![egui::Event::PointerMoved(position)];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(72., 72.))),
                events,
                ..Default::default()
            },
            |ui| {
                show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(60., 60.)),
                    &canvas,
                    &parameters,
                    TrackId(1),
                    &crate::detail::detail_view::DeviceAt(vec![0]),
                    &mut actions,
                )
            },
        );
        output.textures_delta.clear();
        // The command rounds an integer before the next frame reads it back.
        if let Some(value) = actions.iter().filter_map(|a| a.params["value"].as_f64()).next_back() {
            parameters[0].value = value.round();
        }
    }
    assert!(parameters[0].value >= 1.0, "a slow drag must move an integer control, got {}", parameters[0].value);
}
#[test]
fn two_axis_integer_drag_keeps_a_position_per_axis() {
    let canvas = Arc::new(DeviceUi {
        size: [60.0, 60.0],
        controls: vec![CustomControl {
            widget: None,
            visible_when: None,
            id: "xy".into(),
            name: "XY".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 40.0, 40.0],
            gesture: UiGesture::Drag,
            x: Some(UiBinding::new("a")),
            y: Some(UiBinding::new("b")),
            drawing: vec![],
        }],

        ..Default::default()
    });
    let mut parameters = vec![
        GraphParameter {
            control: suhina_device_sdk::ParameterControl::Integer,
            max: 7.0,
            value: 5.0,
            ..parameter("a", false)
        },
        GraphParameter {
            control: suhina_device_sdk::ParameterControl::Integer,
            max: 7.0,
            value: 0.0,
            ..parameter("b", false)
        },
    ];
    let ctx = egui::Context::default();
    let start = Pos2::new(20., 20.);
    let mut frames = vec![(start, None), (start, Some(true))];
    frames.extend((1..=12).map(|i| (start + Vec2::new(i as f32, 0.), None)));
    frames.push((start + Vec2::new(12., 0.), Some(false)));
    for (position, pressed) in frames {
        let mut events = vec![egui::Event::PointerMoved(position)];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        let mut actions = Vec::new();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(72., 72.))),
                events,
                ..Default::default()
            },
            |ui| {
                show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(60., 60.)),
                    &canvas,
                    &parameters,
                    TrackId(1),
                    &crate::detail::detail_view::DeviceAt(vec![0]),
                    &mut actions,
                )
            },
        );
        output.textures_delta.clear();
        for action in &actions {
            let id = action.params["param"].as_str().unwrap_or_default();
            if let (Some(value), Some(p)) =
                (action.params["value"].as_f64(), parameters.iter_mut().find(|p| p.id == id))
            {
                p.value = value.round();
            }
        }
    }
    assert!(parameters[0].value >= 5.0, "x must not read y's position, got {}", parameters[0].value);
    assert_eq!(parameters[1].value, 0.0);
}
#[test]
fn idle_log_control_does_not_emit_edits_and_xy_click_is_one_gesture() {
    let ctx = egui::Context::default();
    let canvas = Arc::new(DeviceUi {
        size: [160.0, 160.0],
        controls: vec![CustomControl {
            widget: None,
            visible_when: None,
            id: "xy".into(),
            name: "XY".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 100.0, 100.0],
            gesture: UiGesture::Position,
            x: Some(UiBinding { parameter: "x".into(), logarithmic: true, drag_scale: 1. }),
            y: Some(UiBinding::new("y")),
            drawing: vec![],
        }],
        ..Default::default()
    });
    let params = [parameter("x", true), parameter("y", false)];
    let mut actions = Vec::new();
    let mut frame = |events| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::splat(160.0)),
                &canvas,
                &params,
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut actions,
            )
        });
        output.textures_delta.clear();
    };
    frame(vec![]);
    frame(vec![
        egui::Event::PointerMoved(Pos2::new(75.0, 25.0)),
        egui::Event::PointerButton {
            pos: Pos2::new(75.0, 25.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    frame(vec![egui::Event::PointerButton {
        pos: Pos2::new(75.0, 25.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0].params["continuous"], true);
    assert_eq!(actions[1].params["continuous"], false);
    assert_eq!(actions[0].params["gesture"], "xy");
    assert!((actions[0].params["value"].as_f64().unwrap() - 3556.5588).abs() < 0.1);
    assert_eq!(actions[1].params["value"], 0.75);
}

#[test]
fn built_in_widget_colors_follow_the_active_theme_and_refresh_the_cache() {
    let context = egui::Context::default();
    let canvas = Arc::new(DeviceUi {
        controls: vec![CustomControl {
            widget: Some(Widget::Button { label: "On".into(), off_label: None, indicator: false }),
            id: "on".into(),
            name: "On".into(),
            tooltip: String::new(),
            bounds: [0.0, 0.0, 40.0, 12.0],
            visible_when: None,
            gesture: UiGesture::Toggle,
            x: Some(UiBinding::new("enabled")),
            y: None,
            drawing: Vec::new(),
        }],
        ..DeviceUi::default()
    });
    let dark = crate::kit::theme::Palette::DARK;
    crate::kit::theme::set_palette(&context, dark);
    let dark_loaded = images(&context, &canvas);
    let mut alternate = dark;
    alternate.accent = Color32::from_rgb(12, 34, 56);
    crate::kit::theme::set_palette(&context, alternate);
    let alternate_loaded = images(&context, &canvas);
    assert!(!Arc::ptr_eq(&dark_loaded, &alternate_loaded));
    assert!(
        alternate_loaded.controls[0]
            .iter()
            .any(|draw| matches!(draw, Draw::Rect { color, .. } if *color == [12, 34, 56, 255]))
    );
    let delay = Arc::new(super::delay_example::build().unwrap().ui.unwrap());
    let themed = images(&context, &delay);
    let cutoff = delay.controls.iter().position(|c| c.id == "cutoff").unwrap();
    assert!(themed.controls[cutoff].iter().any(|draw| matches!(
        draw, Draw::Text { color, .. } if *color == rgba(alternate.text_primary)
    )));
    let button = delay.controls.iter().position(|c| matches!(c.widget, Some(Widget::Button { .. }))).unwrap();
    assert!(themed.controls[button].iter().any(|draw| matches!(
        draw, Draw::Rect { color, .. } if *color == rgba(alternate.accent)
    )));
    crate::kit::theme::set_palette(&context, dark);
}

#[test]
fn default_canvas_background_follows_the_theme_and_authored_background_stays_fixed() {
    let context = egui::Context::default();
    let mut palette = crate::kit::theme::Palette::DARK;
    palette.panel_background = Color32::from_rgb(11, 22, 33);
    crate::kit::theme::set_palette(&context, palette);
    let default = Arc::new(DeviceUi::default());
    let authored = Arc::new(DeviceUi { background: [44, 55, 66, 255], ..DeviceUi::default() });
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        let mut actions = Vec::new();
        for (x, canvas) in [(8.0, &default), (56.0, &authored)] {
            show(
                ui,
                Rect::from_min_size(Pos2::new(x, 8.0), Vec2::splat(40.0)),
                canvas,
                &[],
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut actions,
            );
        }
    });
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Rect(rect) if rect.fill == palette.panel_background
    )));
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape,
        egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(44, 55, 66)
    )));
    output.textures_delta.clear();
    crate::kit::theme::set_palette(&context, crate::kit::theme::Palette::DARK);
}
#[test]
fn a_negative_that_rounds_to_zero_shows_no_minus_sign() {
    assert_eq!(fixed(-0.0, 1), "0.0");
    assert_eq!(fixed(-0.004, 2), "0.00");
    assert_eq!(fixed(-0.4, 0), "0");
    assert_eq!(fixed(-0.6, 0), "-1");
    assert_eq!(fixed(-12.5, 1), "-12.5");
}
