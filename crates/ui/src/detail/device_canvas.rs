// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SDK drawing programs are evaluated only on the UI thread.
use crate::detail::detail_view::GraphParameter;
use crate::kit::widgets::Action;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use serde_json::json;
use std::{cell::RefCell, sync::Arc};
use suhina_device_sdk::{DeviceGraph, ui::*};
use suhina_engine::model::TrackId;

pub(crate) struct Parsed {
    pub graph: DeviceGraph,
    pub canvas: Option<Arc<DeviceUi>>,
}
thread_local! {static GRAPHS:RefCell<Vec<(u64,Option<Arc<Parsed>>)>>=const{RefCell::new(Vec::new())};}
pub(crate) fn graph(bytes: &[u8]) -> Option<Arc<Parsed>> {
    if bytes.len() > 2 * 1024 * 1024 {
        return None;
    }
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    let key = hash.finish();
    if let Some(found) = GRAPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let index = cache.iter().position(|(hash, _)| *hash == key)?;
        let entry = cache.remove(index);
        let found = entry.1.clone();
        cache.push(entry);
        Some(found)
    }) {
        return found;
    }
    let parsed = std::str::from_utf8(bytes).ok().and_then(|text| DeviceGraph::from_sd(text).ok()).map(|mut graph| {
        let canvas = graph.ui.take().map(Arc::new);
        Arc::new(Parsed { graph, canvas })
    });
    GRAPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= 64 {
            cache.remove(0);
        }
        cache.push((key, parsed.clone()));
    });
    parsed
}
#[derive(Clone)]
struct Loaded {
    source: Arc<DeviceUi>,
    palette: WidgetPalette,
    images: Vec<Result<egui::TextureHandle, String>>,
    controls: Vec<Vec<Draw>>,
}

fn visible_sample_peaks(source: &suhina_engine::AudioData, range: (usize, usize)) -> Vec<f32> {
    let range = (range.0.min(source.frames()), range.1.min(source.frames()));
    let mut peaks = vec![0f32; 1024];
    let channels = usize::from(source.channels.max(1));
    let span = range.1.saturating_sub(range.0).max(1);
    for (i, frame) in source
        .samples
        .get(range.0.saturating_mul(channels)..range.1.saturating_mul(channels))
        .unwrap_or_default()
        .chunks(channels)
        .enumerate()
    {
        let bin = i.saturating_mul(1024) / span;
        if let Some(peak) = peaks.get_mut(bin) {
            for &sample in frame {
                if sample.is_finite() {
                    *peak = peak.max(sample.abs().min(1.));
                }
            }
        }
    }
    if span < peaks.len() {
        let bins = peaks.len();
        for (i, peak) in peaks.iter_mut().enumerate() {
            if let Some(frame) = source
                .samples
                .get(range.0.saturating_add(i.saturating_mul(span) / bins).saturating_mul(channels)..)
                .and_then(|samples| samples.get(..channels))
            {
                *peak = frame.iter().copied().filter(|v| v.is_finite()).fold(0f32, |p, v| p.max(v.abs().min(1.)));
            }
        }
    }
    peaks
}

pub(crate) type SampleTarget = (Rect, TrackId, usize);
pub(crate) fn sample_targets_id() -> egui::Id {
    egui::Id::new("sampler-drop-targets")
}
pub(crate) fn sample_source_id(track: TrackId, device: usize) -> egui::Id {
    egui::Id::new(("sampler-source", track.0, device))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SampleView {
    start: f64,
    span: f64,
}
impl Default for SampleView {
    fn default() -> Self {
        Self { start: 0., span: 1. }
    }
}
impl SampleView {
    fn x(self, point: f64, field: Rect) -> f32 {
        field.left() + ((point - self.start) / self.span) as f32 * field.width()
    }
    fn point(self, x: f32, field: Rect) -> f64 {
        self.start + f64::from((x - field.left()) / field.width()) * self.span
    }
    fn dragged(self, anchor: Pos2, delta: Vec2, field: Rect, frames: usize) -> Self {
        let fraction = f64::from((anchor.x - field.left()) / field.width());
        let span = (self.span * (f64::from(delta.y) / 120.).exp()).clamp(1. / frames.max(1) as f64, 1.);
        let start = (self.point(anchor.x, field) - fraction * span - f64::from(delta.x / field.width()) * self.span)
            .clamp(0., 1. - span);
        Self { start, span }
    }
}
#[derive(Clone)]
struct SampleDrag {
    view: SampleView,
    anchor: Pos2,
    selected: Option<usize>,
    points: Arc<[f64]>,
    delta: Vec2,
}
#[derive(Clone)]
struct SamplePeaks {
    source: Arc<suhina_engine::AudioData>,
    range: (usize, usize),
    peaks: Arc<Vec<f32>>,
}
fn nearest_slice(points: &[f64], view: SampleView, field: Rect, pointer: Pos2, start: f64, end: f64) -> Option<usize> {
    let index = points.partition_point(|p| *p < view.point(pointer.x, field));
    [index.checked_sub(1), Some(index)]
        .into_iter()
        .flatten()
        .filter(|&i| {
            points.get(i).is_some_and(|p| *p > start && *p < end && (view.x(*p, field) - pointer.x).abs() <= 6.)
        })
        .min_by(|&a, &b| {
            (view.x(points[a], field) - pointer.x).abs().total_cmp(&(view.x(points[b], field) - pointer.x).abs())
        })
}
fn slice_edit(actions: &mut Vec<Action>, track: TrackId, device: usize, points: Vec<f64>, continued: bool) {
    actions.push(Action::new(
        "device.setSampleSlices",
        json!({"track":track.0,"device":device,"points":points,"continue":continued}),
    ));
}

const SAMPLE_MARKER: Color32 = Color32::from_rgb(230, 120, 35);

fn sample_marker(painter: &egui::Painter, x: f32, top: f32, bottom: f32) {
    let line = [Pos2::new(x, top), Pos2::new(x, bottom)];
    painter.line_segment(line, Stroke::new(2., SAMPLE_MARKER));
}

fn sample_waveform(
    ui: &mut egui::Ui,
    bounds: Rect,
    parameters: &[GraphParameter],
    track: TrackId,
    device: usize,
    actions: &mut Vec<Action>,
    inline_recording: bool,
) {
    let visible = bounds.intersect(ui.clip_rect());
    if visible.is_positive() {
        // MIDI can start a voice while the transport and read head are idle.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
    ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Vec<SampleTarget>>(sample_targets_id()).push((visible, track, device))
    });
    let source = ui.ctx().data(|data| data.get_temp::<Arc<suhina_engine::AudioData>>(sample_source_id(track, device)));
    let painter = ui.painter_at(visible);
    painter.rect_filled(bounds, 0., crate::kit::theme::control_background());
    let colors = crate::kit::theme::palette();
    let compact = bounds.height() < 100.;
    let field = if compact {
        Rect::from_min_max(
            bounds.min + Vec2::new(8., 8.),
            bounds.max - Vec2::new(8., if inline_recording { 26. } else { 8. }),
        )
    } else {
        bounds.shrink2(Vec2::new(8., 28.))
    };
    let value = |id: &str| parameters.iter().find(|p| p.id == id).map_or(0., |p| p.value) as f32;
    let edit_loop = value("mode") == 0. && value("playback") == 2.;
    let active_start = f64::from(value(if edit_loop { "loop_start" } else { "start" }));
    let active_end = f64::from(value(if edit_loop { "loop_end" } else { "end" }));
    let source_id = sample_source_id(track, device);
    let view_id = source_id.with("view");
    let old = ui.ctx().data(|d| d.get_temp::<(Option<Arc<suhina_engine::AudioData>>, SampleView)>(view_id));
    let mut view = old
        .filter(|(old, _)| match (old.as_ref(), source.as_ref()) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        })
        .map_or(SampleView::default(), |(_, v)| v);
    let slicing = value("mode") == 0. && value("playback") == 3.;
    let manual = slicing && value("slice_by") == 3.;
    let mut manual_points =
        ui.ctx().data(|d| d.get_temp::<Arc<[f64]>>(source_id.with("manual-points"))).unwrap_or_else(|| {
            let mut points: Vec<_> = parameters
                .iter()
                .filter(|p| p.id.starts_with("slice_") && p.id != "slice_by" && p.value > 0. && p.value < 1.)
                .map(|p| p.value)
                .collect();
            points.sort_unstable_by(f64::total_cmp);
            points.dedup();
            Arc::from(points)
        });
    let response = ui.interact(field, source_id.with("wave-navigation"), egui::Sense::click_and_drag());
    let hovered = ui.input(|i| i.pointer.hover_pos()).filter(|p| field.contains(*p));
    let nearest = if manual {
        hovered.and_then(|p| nearest_slice(&manual_points, view, field, p, active_start, active_end))
    } else {
        None
    };
    let drag_id = source_id.with("wave-drag");
    if response.is_pointer_button_down_on() {
        ui.ctx().data_mut(|d| {
            if d.get_temp::<SampleDrag>(drag_id).is_none() {
                let anchor = response.interact_pointer_pos().unwrap_or(field.center());
                d.insert_temp(
                    drag_id,
                    SampleDrag { view, anchor, selected: nearest, points: manual_points.clone(), delta: Vec2::ZERO },
                );
            }
        });
    }
    let mut drag = ui.ctx().data(|d| d.get_temp::<SampleDrag>(drag_id));
    let click_id = source_id.with("manual-click");
    let double_click = if manual
        && response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let time = ui.input(|i| i.time);
        let options = ui.ctx().options(|o| o.input_options);
        ui.ctx().data_mut(|d| {
            let paired = d.get_temp::<(f64, Pos2)>(click_id).is_some_and(|(last, pos)| {
                time - last <= options.max_double_click_delay && pos.distance(pointer) <= options.max_click_dist
            });
            if paired {
                d.remove::<(f64, Pos2)>(click_id);
            } else {
                d.insert_temp(click_id, (time, pointer));
            }
            paired
        })
    } else {
        false
    };
    if response.dragged() {
        ui.ctx().data_mut(|d| d.remove::<(f64, Pos2)>(click_id));
    }
    if double_click && manual && source.is_some() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let mut points = manual_points.to_vec();
            if let Some(index) = nearest {
                points.remove(index);
                slice_edit(actions, track, device, points, false);
            } else {
                let point = view.point(pointer.x, field);
                if point > active_start && point < active_end && point > 0. && point < 1. {
                    points.push(point);
                    slice_edit(actions, track, device, points, false);
                }
            }
        }
    } else if (response.dragged() || response.drag_stopped())
        && let Some(state) = &mut drag
    {
        let delta = response.interact_pointer_pos().map_or(state.delta, |p| p - state.anchor);
        state.delta = delta;
        if let Some(index) = state.selected {
            let mut points = state.points.to_vec();
            if let Some(point) = points.get_mut(index) {
                *point = (state.points[index] + f64::from(delta.x / field.width()) * state.view.span)
                    .clamp(f64::EPSILON, 1. - f64::EPSILON);
                points.sort_unstable_by(f64::total_cmp);
                points.dedup();
                // Preparing the audio graph on each pointer move stalls the UI.
                if response.drag_stopped() && points.as_slice() != state.points.as_ref() {
                    slice_edit(actions, track, device, points.clone(), false);
                }
                manual_points = Arc::from(points);
            }
        } else if let Some(source) = &source {
            view = state.view.dragged(state.anchor, delta, field, source.frames());
        }
    }
    let cursor = if nearest.is_some() || drag.as_ref().is_some_and(|d| d.selected.is_some()) {
        egui::CursorIcon::ResizeHorizontal
    } else if response.dragged() && response.drag_delta().y > 0. {
        egui::CursorIcon::ZoomOut
    } else {
        egui::CursorIcon::ZoomIn
    };
    response.on_hover_cursor(cursor).on_hover_text(
        "Drag up/down to zoom, left/right to pan. Manual: double-click to add/remove a slice; drag a line to move it.",
    );
    let pointer_down = ui.input(|i| i.pointer.primary_down());
    ui.ctx().data_mut(|d| {
        d.insert_temp(view_id, (source.clone(), view));
        if pointer_down {
            if let Some(state) = drag {
                d.insert_temp(drag_id, state);
            }
        } else {
            d.remove::<SampleDrag>(drag_id);
        }
    });
    let wave_painter = ui.painter_at(field.intersect(visible));
    let loop_area = Rect::from_min_max(
        Pos2::new(view.x(active_start, field), field.top()),
        Pos2::new(view.x(active_end, field), field.bottom()),
    )
    .intersect(field);
    if edit_loop && loop_area.is_positive() {
        wave_painter.rect_filled(loop_area, 0., colors.accent.gamma_multiply(0.12));
    }
    if let Some(source) = &source {
        let frames = source.frames();
        let range = ((view.start * frames as f64) as usize, ((view.start + view.span) * frames as f64).ceil() as usize);
        let cache_id = source_id.with("visible-peaks");
        let cached = ui.ctx().data(|d| d.get_temp::<SamplePeaks>(cache_id));
        let peaks = if let Some(cached) = cached.filter(|c| Arc::ptr_eq(&c.source, source) && c.range == range) {
            cached.peaks
        } else {
            let peaks = visible_sample_peaks(source, range);
            let peaks = Arc::new(peaks);
            ui.ctx().data_mut(|d| {
                d.insert_temp(cache_id, SamplePeaks { source: source.clone(), range, peaks: peaks.clone() })
            });
            peaks
        };
        for (i, peak) in peaks.iter().enumerate() {
            let x = field.left() + i as f32 / 1024. * field.width();
            let point = view.point(x, field);
            let color = if point < active_start || point > active_end { Color32::GRAY } else { colors.accent };
            let h = peak * field.height() * 0.48;
            wave_painter.line_segment(
                [Pos2::new(x, field.center().y - h), Pos2::new(x, field.center().y + h)],
                Stroke::new(1., color),
            );
        }
        if slicing {
            let points: Arc<[f64]> = if manual {
                manual_points
            } else {
                let cache_id = source_id.with("slices");
                let cached = ui.ctx().data(|d| {
                    d.get_temp::<(Arc<suhina_engine::AudioData>, suhina_device_sdk::sampler::SliceAnalysis)>(cache_id)
                });
                let analysis = if let Some((_, a)) = cached.filter(|(old, _)| Arc::ptr_eq(old, source)) {
                    a
                } else {
                    let a = suhina_device_sdk::sampler::SliceAnalysis::new(
                        &source.samples,
                        usize::from(source.channels),
                        f64::from(source.sample_rate),
                    );
                    ui.ctx().data_mut(|d| d.insert_temp(cache_id, (source.clone(), a.clone())));
                    a
                };
                let params: Vec<_> = suhina_device_sdk::sampler::PARAMS
                    .iter()
                    .map(|p| parameters.iter().find(|v| v.id == p.id).map_or(p.default, |v| v.value))
                    .collect();
                Arc::from(analysis.points(
                    &params,
                    source.frames(),
                    f64::from(source.sample_rate),
                    ui.ctx().data(|d| d.get_temp::<f64>(egui::Id::new("sampler-tempo"))).unwrap_or(120.),
                ))
            };
            let first = points.partition_point(|p| *p <= active_start.max(view.start));
            for &point in points
                .get(first..)
                .unwrap_or_default()
                .iter()
                .take_while(|p| **p < active_end.min(view.start + view.span))
            {
                let x = view.x(point, field);
                sample_marker(&wave_painter, x, field.top(), field.bottom());
            }
        }
    } else {
        wave_painter.text(
            field.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a sample here",
            crate::kit::theme::ui_font(12.),
            colors.text_primary,
        );
    }
    let positions = ui
        .ctx()
        .data(|d| d.get_temp::<[Option<f32>; 32]>(sample_source_id(track, device).with("position")))
        .unwrap_or([None; 32]);
    for position in positions.into_iter().flatten() {
        let x = view.x(f64::from(position.clamp(0., 1.)), field);
        sample_marker(&wave_painter, x, field.top(), field.bottom());
    }
    for (id, caption, offset) in [
        (if edit_loop { "loop_start" } else { "start" }, "S", 0.),
        (if edit_loop { "loop_end" } else { "end" }, "E", 0.),
        ("position", "G", 40.),
    ] {
        if id == "position" && value("mode") == 0. {
            continue;
        }
        let (origin, span) =
            if id == "position" { (value("start"), (value("end") - value("start")).max(1e-6)) } else { (0., 1.) };
        let x = view.x(f64::from(origin + value(id) * span), field);
        if x < field.left() || x > field.right() {
            continue;
        }
        let y = bounds.top() + if compact { 8. + offset * 0.7 } else { 26. + offset };
        let marker = Rect::from_center_size(Pos2::new(x, y), Vec2::new(14., if compact { 12. } else { 18. }));
        sample_marker(&painter, x, y, field.bottom());
        painter.rect_filled(marker, 0., SAMPLE_MARKER);
        painter.text(
            marker.center(),
            egui::Align2::CENTER_CENTER,
            caption,
            crate::kit::theme::ui_font(10.),
            crate::kit::theme::window_background(),
        );
        let response = ui
            .interact(marker, egui::Id::new(("sample-marker", track.0, device, id)), egui::Sense::click_and_drag())
            .on_hover_text(format!("Drag {}", id.replace('_', " ")))
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if response.double_clicked() {
            if let Some(p) = parameters.iter().find(|p| p.id == id) {
                actions.push(Action::new(
                    "device.setParam",
                    json!({"track":track.0,"device":device,"param":id,"value":p.default,"continuous":false}),
                ));
            }
        } else if (response.dragged() || response.drag_stopped())
            && let Some(pos) = response.interact_pointer_pos()
        {
            actions.push(Action::new("device.setParam", json!({"track": track.0, "device": device, "param": id, "value": ((view.point(pos.x,field) - f64::from(origin)) / f64::from(span)).clamp(0., 1.), "continuous": !response.drag_stopped()})));
        }
    }
    if inline_recording {
        sample_recording(
            ui,
            Rect::from_min_size(bounds.right_bottom() - Vec2::new(324., 24.), Vec2::new(316., 22.)),
            track,
            device,
            actions,
        );
    }
}
fn sample_recording(ui: &mut egui::Ui, bounds: Rect, track: TrackId, device: usize, actions: &mut Vec<Action>) {
    let recording =
        ui.ctx().data(|d| d.get_temp::<bool>(sample_source_id(track, device).with("recording"))).unwrap_or(false);
    let (input, channels, armed) = ui
        .ctx()
        .data(|d| {
            d.get_temp::<(suhina_engine::model::AudioInput, u16, bool)>(sample_source_id(track, device).with("input"))
        })
        .unwrap_or_default();
    let record_x = bounds.right() - if recording { 152. } else { 78. };
    let menu = Rect::from_min_size(Pos2::new(record_x - 164., bounds.top()), Vec2::new(160., bounds.height()));
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(menu).layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            ui.spacing_mut().interact_size.y = bounds.height();
            ui.menu_button(format!("Input: {}", crate::interaction::menus::audio_input_text(&input)), |ui| {
                let mut next_armed = armed;
                if ui.checkbox(&mut next_armed, "Arm track").changed() {
                    actions.push(Action::new("track.setArm", json!({"track":track.0, "armed":next_armed})));
                }
                crate::interaction::menus::audio_input_choices(ui, track, &input, channels, actions);
            });
        },
    );
    for (text, on, x) in [(if recording { "Recording" } else { "Record" }, true, 8.), ("Stop", false, 90.)] {
        if !on && !recording {
            continue;
        }
        let button = Rect::from_min_size(
            Pos2::new(record_x + x - 8., bounds.top()),
            Vec2::new(if on { 78. } else { 70. }, bounds.height()),
        );
        let response = ui.put(
            button,
            egui::Button::new(if on { format!("   {text}") } else { text.into() })
                .corner_radius(0.)
                .selected(on && recording),
        );
        if on {
            ui.painter().circle_filled(
                Pos2::new(button.left() + 10., button.center().y),
                3.5,
                Color32::from_rgb(225, 72, 86),
            );
        }
        if response.clicked() {
            actions.push(Action::new("device.recordSample", json!({"track": track.0, "device": device, "on": on})));
        }
    }
}
struct EnvelopePlot {
    points: [Pos2; 5],
    field: Rect,
    time_width: f32,
    hold: f32,
    max_time: f64,
}
impl EnvelopePlot {
    fn new(bounds: Rect, values: [f64; 4], max_time: f64, adsr: bool) -> Self {
        let field = bounds.shrink(8.);
        let hold = if adsr { field.width() * 0.25 } else { 0. };
        let mut plot =
            Self { points: [field.left_bottom(); 5], field, time_width: field.width() - hold, hold, max_time };
        let baseline = if adsr { field.bottom() } else { field.center().y };
        let level = field.bottom() - values[2] as f32 * field.height();
        plot.points = [
            Pos2::new(field.left(), baseline),
            Pos2::new(plot.x(values[0]), if adsr { field.top() } else { level }),
            Pos2::new(plot.x(values[0] + values[1]), if adsr { level } else { baseline }),
            Pos2::new(plot.x(values[0] + values[1]) + hold, level),
            Pos2::new(plot.x(values[0] + values[1] + values[3]) + hold, baseline),
        ];
        plot
    }
    fn x(&self, time: f64) -> f32 {
        self.field.left() + ((time.max(0.) / 0.01).ln_1p() / (self.max_time / 0.01).ln_1p()) as f32 * self.time_width
    }
    fn time(&self, x: f32) -> f64 {
        (((x - self.field.left()) / self.time_width).clamp(0., 1.) as f64 * (self.max_time / 0.01).ln_1p()).exp_m1()
            * 0.01
    }
}

fn envelope(
    ui: &mut egui::Ui,
    bounds: Rect,
    control: &CustomControl,
    parameters: &[GraphParameter],
    target: (TrackId, usize),
    actions: &mut Vec<Action>,
) {
    let Some(Widget::Envelope { attack, decay, level, release }) = &control.widget else { return };
    let find = |id: &str| parameters.iter().find(|p| p.id == id);
    let (Some(a), Some(d), Some(l)) = (find(attack), find(decay), find(level)) else { return };
    let r = release.as_deref().and_then(find);
    let adsr = r.is_some();
    let values = [a.value, d.value, normalized(l, false), r.map_or(0., |p| p.value)];
    let max_time = (a.max + d.max + r.map_or(0., |p| p.max)).max(0.01);
    let plot = EnvelopePlot::new(bounds, values, max_time, adsr);
    let palette = crate::kit::theme::palette();
    let painter = ui.painter_at(bounds.intersect(ui.clip_rect()));
    painter.line_segment(
        [plot.points[0], Pos2::new(plot.field.right(), plot.points[0].y)],
        Stroke::new(1., palette.border),
    );
    let count = if adsr { 5 } else { 3 };
    painter.add(egui::Shape::line(plot.points[..count].to_vec(), Stroke::new(1.2, palette.accent)));
    painter.circle_filled(plot.points[0], 2.5, palette.accent);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let nearest = pointer.and_then(|pos| {
        (1..count)
            .filter(|i| plot.points[*i].distance(pos) <= 11.)
            .min_by(|i, j| plot.points[*i].distance_sq(pos).total_cmp(&plot.points[*j].distance_sq(pos)))
    });
    for index in 1..count {
        let id = egui::Id::new(("sd-envelope", target.0.0, target.1, &control.id, index));
        let state_id = id.with("press");
        let state = ui.ctx().data(|data| data.get_temp::<([f64; 4], [f64; 4])>(state_id));
        let sense = if nearest == Some(index) || state.is_some() {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::focusable_noninteractive()
        };
        let response = ui.interact(Rect::from_center_size(plot.points[index], Vec2::splat(22.)), id, sense);
        let (time_param, level_param) = match index {
            1 => (Some(a), (!adsr).then_some(l)),
            2 => (Some(d), adsr.then_some(l)),
            3 => (None, Some(l)),
            _ => (r, None),
        };
        let caption = [time_param.map(|p| p.name.as_str()), level_param.map(|p| p.name.as_str())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" + ");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &caption));
        let response = response
            .on_hover_text(format!("{caption}: drag point; Shift for precision; double-click resets"))
            .on_hover_cursor(egui::CursorIcon::Grab);
        if response.is_pointer_button_down_on() {
            response.request_focus();
            if state.is_none() {
                ui.ctx().data_mut(|data| data.insert_temp(state_id, (values, values)));
            }
        }
        let focused = response.has_focus();
        let (keys, fine) = ui.input(|i| {
            (
                if focused {
                    Vec2::new(
                        f32::from(i.key_pressed(egui::Key::ArrowRight))
                            - f32::from(i.key_pressed(egui::Key::ArrowLeft)),
                        f32::from(i.key_pressed(egui::Key::ArrowDown)) - f32::from(i.key_pressed(egui::Key::ArrowUp)),
                    )
                } else {
                    Vec2::ZERO
                },
                if i.modifiers.shift { 0.1 } else { 1. },
            )
        });
        let (start, mut next) = state.unwrap_or((values, values));
        if response.dragged() {
            let initial = EnvelopePlot::new(bounds, start, max_time, adsr);
            let delta = response.total_drag_delta().unwrap_or_default() * fine;
            let point = initial.points[index] + delta;
            match index {
                1 => next[0] = initial.time(point.x).clamp(a.min, a.max),
                2 => next[1] = (initial.time(point.x) - start[0]).clamp(d.min, d.max),
                4 => {
                    if let Some(r) = r {
                        next[3] = (initial.time(point.x - initial.hold) - start[0] - start[1]).clamp(r.min, r.max);
                    }
                }
                _ => {}
            }
            if level_param.is_some() {
                next[2] = (start[2] - f64::from(delta.y / initial.field.height())).clamp(0., 1.);
            }
            ui.ctx().data_mut(|data| data.insert_temp(state_id, (start, next)));
        }
        if response.dragged() || response.drag_stopped() || response.double_clicked() || keys != Vec2::ZERO {
            let mut edits = Vec::new();
            if let Some(p) = time_param {
                let value = next[if index == 1 {
                    0
                } else if index == 2 {
                    1
                } else {
                    3
                }];
                edits.push((
                    p,
                    (value + f64::from(keys.x) * (p.max - p.min) * 0.01 * f64::from(fine)).clamp(p.min, p.max),
                ));
            }
            if let Some(p) = level_param {
                let value = p.min + next[2] * (p.max - p.min);
                edits.push((
                    p,
                    (value - f64::from(keys.y) * (p.max - p.min) * 0.01 * f64::from(fine)).clamp(p.min, p.max),
                ));
            }
            let len = edits.len();
            for (i, (p, value)) in edits.into_iter().enumerate() {
                actions.push(Action::new("device.setParam", json!({"track":target.0.0,"device":target.1,"param":p.id,"value":if response.double_clicked() { p.default } else { value },"continuous":i+1<len || response.dragged(),"gesture":format!("{}_{}",control.id,index)})));
            }
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            ui.ctx().data_mut(|data| data.remove::<([f64; 4], [f64; 4])>(state_id));
        }
        let active = nearest == Some(index) || response.has_focus() || response.dragged();
        painter.circle_filled(plot.points[index], if active { 5. } else { 4.5 }, palette.accent);
        painter.circle_stroke(
            plot.points[index],
            if active { 5. } else { 4.5 },
            Stroke::new(0.75, palette.panel_background),
        );
    }
}

fn rgba(color: egui::Color32) -> [u8; 4] {
    [color.r(), color.g(), color.b(), color.a()]
}
fn accent_text(accent: egui::Color32) -> [u8; 4] {
    let luminance = 0.2126 * f32::from(accent.r()) + 0.7152 * f32::from(accent.g()) + 0.0722 * f32::from(accent.b());
    if luminance > 145.0 { [29, 28, 26, 255] } else { [255, 255, 255, 255] }
}
fn images(ctx: &egui::Context, source: &Arc<DeviceUi>) -> Arc<Loaded> {
    let colors = crate::kit::theme::palette();
    let palette = WidgetPalette {
        text: rgba(colors.text_primary),
        panel: rgba(colors.panel_background),
        control: rgba(colors.control_background),
        border: rgba(colors.border),
        accent: rgba(colors.accent),
        accent_text: accent_text(colors.accent),
    };
    let key = egui::Id::new("sd-image-cache");
    let mut cache = ctx.data_mut(|d| d.get_temp::<Vec<Arc<Loaded>>>(key)).unwrap_or_default();
    if let Some(found) = cache.iter().find(|v| Arc::ptr_eq(&v.source, source) && v.palette == palette) {
        return found.clone();
    }
    let images = source
        .assets
        .iter()
        .map(|a| {
            a.decode_rgba()
                .map(|(pixels, size)| {
                    ctx.load_texture(
                        &a.id,
                        egui::ColorImage::from_rgba_unmultiplied(size, &pixels),
                        egui::TextureOptions {
                            wrap_mode: egui::TextureWrapMode::Repeat,
                            ..egui::TextureOptions::LINEAR
                        },
                    )
                })
                .map_err(|e| e.to_string())
        })
        .collect();
    let controls = source.controls.iter().map(|c| c.drawings_with_palette(&palette).into_owned()).collect();
    let loaded = Arc::new(Loaded { source: source.clone(), palette, images, controls });
    cache.push(loaded.clone());
    if cache.len() > 8 {
        cache.remove(0);
    }
    ctx.data_mut(|d| d.insert_temp(key, cache));
    loaded
}
fn color(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}
fn normalized(p: &GraphParameter, log: bool) -> f64 {
    if log && p.min > 0.0 {
        ((p.value.max(p.min).ln() - p.min.ln()) / (p.max.ln() - p.min.ln())).clamp(0.0, 1.0)
    } else if p.max > p.min {
        ((p.value - p.min) / (p.max - p.min)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn paint(painter: &egui::Painter, drawings: &[Draw], origin: Pos2, parameters: &[GraphParameter], loaded: &Loaded) {
    let lookup = |id: &str, norm: bool, log: bool| {
        parameters.iter().find(|p| p.id == id).map_or(0.0, |p| if norm { normalized(p, log) } else { p.value })
    };
    let value = |v: &UiValue| v.evaluate(&lookup);
    let point = |p: &[UiValue; 2]| origin + Vec2::new(value(&p[0]), value(&p[1]));
    let rectangle = |r: &[UiValue; 4]| {
        Rect::from_min_size(
            origin + Vec2::new(value(&r[0]), value(&r[1])),
            Vec2::new(value(&r[2]).clamp(0.0, 8192.0), value(&r[3]).clamp(0.0, 8192.0)),
        )
    };
    for draw in drawings {
        match draw {
            Draw::Rect { rect, color: c, radius } => {
                painter.rect_filled(rectangle(rect), *radius, color(*c));
            }
            Draw::Circle { center, radius, color: c } => {
                painter.circle_filled(point(center), value(radius).clamp(0.0, 8192.0), color(*c));
            }
            Draw::Line { from, to, color: c, width } => {
                painter.line_segment([point(from), point(to)], Stroke::new(*width, color(*c)));
            }
            Draw::Path { points, color: c, width, closed, fill } => {
                let points = points.iter().map(point).collect::<Vec<_>>();
                if let Some(fill) = fill {
                    painter.add(egui::Shape::convex_polygon(points, color(*fill), Stroke::new(*width, color(*c))));
                } else if *closed {
                    painter.add(egui::Shape::closed_line(points, Stroke::new(*width, color(*c))));
                } else {
                    painter.add(egui::Shape::line(points, Stroke::new(*width, color(*c))));
                }
            }
            Draw::Arc { center, radius, start, end, color: c, width } => {
                let center = point(center);
                let radius = value(radius).clamp(0.0, 8192.0);
                let start = value(start);
                let end = value(end);
                let points = (0..=64)
                    .map(|i| {
                        let a = start + (end - start) * i as f32 / 64.0;
                        center + Vec2::new(a.cos(), a.sin()) * radius
                    })
                    .collect();
                painter.add(egui::Shape::line(points, Stroke::new(*width, color(*c))));
            }
            Draw::Text { position, text, color: c, size, parameter, decimals, align, format } => {
                let label = parameter.as_ref().and_then(|id| parameters.iter().find(|p| p.id == *id)).map_or_else(
                    || text.clone(),
                    |p| match format {
                        TextFormat::Choice => {
                            p.ui.options.get(p.value.round().max(0.0) as usize).cloned().unwrap_or_default()
                        }
                        TextFormat::Frequency if p.value >= 1000.0 => format!("{text}{:.2} kHz", p.value / 1000.0),
                        TextFormat::Tempo if p.value <= 0. => format!("{text}Auto"),
                        TextFormat::Tempo => format!("{text}{:.2} BPM", p.value),
                        TextFormat::Seconds if p.value < 1. => format!("{text}{:.0} ms", p.value * 1000.),
                        TextFormat::Seconds => format!("{text}{:.2} s", p.value),
                        TextFormat::Percent => format!("{text}{:.0}%", p.value * 100.),
                        TextFormat::Decibels => format!("{text}{:.1} dB", p.value),
                        TextFormat::Balance if p.value.abs() < 0.005 => format!("{text}C"),
                        TextFormat::Balance => {
                            format!("{text}{:.0} {}", p.value.abs() * 100., if p.value < 0. { "L" } else { "R" })
                        }
                        _ => format!(
                            "{text}{:.*}{}{}",
                            *decimals,
                            p.value,
                            if p.unit.is_empty() || p.unit == "%" { "" } else { " " },
                            p.unit
                        ),
                    },
                );
                let position = point(position);
                let mut font = egui::FontId::proportional(*size);
                let mut galley = painter.layout_no_wrap(label.clone(), font.clone(), color(*c));
                if parameter.is_some() {
                    let clip = painter.clip_rect();
                    let available_width = if *align == TextAlign::Center {
                        2. * (position.x - clip.left()).min(clip.right() - position.x)
                    } else {
                        clip.right() - position.x
                    };
                    let available_height = clip.bottom() - position.y;
                    let scale = (available_width / galley.size().x.max(1.))
                        .min(available_height / galley.size().y.max(1.))
                        .clamp(0., 1.);
                    if scale < 1. {
                        font.size = (font.size * scale).max(8.);
                        galley = painter.layout_no_wrap(label, font, color(*c));
                    }
                }
                let offset = if *align == TextAlign::Center { Vec2::new(galley.size().x / 2., 0.) } else { Vec2::ZERO };
                painter.galley(position - offset, galley, color(*c));
            }
            Draw::Gradient { rect, colors } => {
                let r = rectangle(rect);
                let mut mesh = egui::Mesh::default();
                for (pos, c) in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].into_iter().zip(colors)
                {
                    mesh.colored_vertex(pos, color(*c));
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(egui::Shape::mesh(mesh));
            }
            Draw::Image { asset, rect, uv, tint, tile_size, fit } => {
                if let Some(i) = loaded.source.assets.iter().position(|a| a.id == *asset) {
                    match loaded.images.get(i) {
                        Some(Ok(texture)) => {
                            let mut r = rectangle(rect);
                            let mut uv = Rect::from_min_max(Pos2::new(uv[0], uv[1]), Pos2::new(uv[2], uv[3]));
                            let size = texture.size_vec2() * uv.size();
                            if let Some([w, h]) = tile_size {
                                uv.max = uv.min + Vec2::new(r.width() / w, r.height() / h);
                            } else if size.x > 0.0 && size.y > 0.0 && r.width() > 0.0 && r.height() > 0.0 {
                                match fit {
                                    ImageFit::Contain => {
                                        let scale = (r.width() / size.x).min(r.height() / size.y);
                                        r = Rect::from_center_size(r.center(), size * scale);
                                    }
                                    ImageFit::Cover => {
                                        let scale = (r.width() / size.x).max(r.height() / size.y);
                                        uv = Rect::from_center_size(uv.center(), uv.size() * r.size() / (size * scale));
                                    }
                                    ImageFit::Stretch => {}
                                }
                            }
                            painter.image(texture.id(), r, uv, color(*tint));
                        }
                        Some(Err(error)) => {
                            painter.text(
                                rectangle(rect).min,
                                egui::Align2::LEFT_TOP,
                                error,
                                egui::FontId::proportional(12.0),
                                Color32::LIGHT_RED,
                            );
                        }
                        None => {}
                    }
                }
            }
        }
    }
}
pub(crate) fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    canvas: &Arc<DeviceUi>,
    parameters: &[GraphParameter],
    track: TrackId,
    at: &crate::detail::detail_view::DeviceAt,
    actions: &mut Vec<Action>,
) {
    // The Sampler's own commands name a device on the track by its index (#600); a device inside
    // a group (#672) is named by its last step.
    let device = at.0.last().copied().unwrap_or(0);
    let loaded = images(ui.ctx(), canvas);
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    let background = if canvas.background == DeviceUi::default().background {
        crate::kit::theme::palette().panel_background
    } else {
        color(canvas.background)
    };
    painter.rect_filled(rect, 0.0, background);
    paint(&painter, &canvas.layers, rect.min, parameters, &loaded);
    let sample_panel = canvas.controls.iter().any(|c| matches!(c.widget, Some(Widget::SampleWaveform)));
    for (control, drawings) in canvas.controls.iter().zip(&loaded.controls) {
        if sample_panel
            && control
                .x
                .iter()
                .chain(control.y.iter())
                .any(|binding| matches!(binding.parameter.as_str(), "warp" | "warp_mode" | "warp_beats" | "source_bpm"))
            && !ui
                .ctx()
                .data(|data| data.get_temp::<Arc<suhina_engine::AudioData>>(sample_source_id(track, device)))
                .is_some_and(|source| !source.samples.is_empty())
        {
            continue;
        }
        if control.visible_when.as_ref().is_some_and(|v| {
            v.evaluate(&|id, norm, log| {
                parameters.iter().find(|p| p.id == id).map_or(0.0, |p| if norm { normalized(p, log) } else { p.value })
            }) <= 0.0
        }) {
            continue;
        }
        let [x, y, w, h] = control.bounds;
        let bounds = Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
        if matches!(control.widget, Some(Widget::Envelope { .. })) {
            envelope(ui, bounds, control, parameters, (track, device), actions);
            continue;
        }
        if matches!(control.widget, Some(Widget::SampleRecording)) {
            sample_recording(ui, bounds, track, device, actions);
            continue;
        }
        if matches!(control.widget, Some(Widget::SampleWaveform)) {
            sample_waveform(
                ui,
                bounds,
                parameters,
                track,
                device,
                actions,
                !canvas.controls.iter().any(|c| matches!(c.widget, Some(Widget::SampleRecording))),
            );
            continue;
        }
        paint(
            &painter.with_clip_rect(bounds.intersect(painter.clip_rect())),
            drawings,
            bounds.min,
            parameters,
            &loaded,
        );
        if matches!(control.widget, Some(Widget::Label { .. } | Widget::Panel { .. } | Widget::Separator)) {
            continue;
        }
        let tooltip = if sample_panel && control.y.as_ref().is_some_and(|b| b.parameter == "source_bpm") {
            let source = parameters.iter().find(|p| p.id == "source_bpm").map_or(0., |p| p.value);
            let enabled = parameters.iter().find(|p| p.id == "warp").is_some_and(|p| p.value > 0.);
            let bpm = ui.ctx().data(|d| d.get_temp::<f64>(egui::Id::new("sampler-tempo"))).unwrap_or(120.);
            format!(
                "{}\nProject tempo: {bpm:.2} BPM. Source: {}. Warp {}.",
                control.tooltip,
                if source > 0. {
                    format!("{source:.2} BPM; playback {:.2}×", bpm / source)
                } else {
                    "Auto / saved legacy length".into()
                },
                if enabled { "on" } else { "off" }
            )
        } else {
            control.tooltip.clone()
        };
        let response = ui
            .interact(
                bounds,
                egui::Id::new(("sd-custom", track.0, device, &control.id)),
                if control.gesture == UiGesture::Menu { egui::Sense::click() } else { egui::Sense::click_and_drag() },
            )
            .on_hover_text(tooltip);
        let response = if control.gesture == UiGesture::Drag {
            response.on_hover_cursor(egui::CursorIcon::ResizeVertical)
        } else {
            response
        };
        if response.is_pointer_button_down_on() || response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = response.has_focus();
        let (dx, dy, button, fine) = ui.input(|i| {
            (
                i32::from(focused && i.key_pressed(egui::Key::ArrowRight))
                    - i32::from(focused && i.key_pressed(egui::Key::ArrowLeft)),
                i32::from(focused && i.key_pressed(egui::Key::ArrowUp))
                    - i32::from(focused && i.key_pressed(egui::Key::ArrowDown)),
                focused && (i.key_down(egui::Key::Space) || i.key_down(egui::Key::Enter)),
                if i.modifiers.shift { 0.1 } else { 1.0 },
            )
        });
        let held_id = response.id.with("held");
        let held = response.is_pointer_button_down_on() || button;
        let was_held = ui.ctx().data_mut(|d| {
            let previous = d.get_temp::<bool>(held_id).unwrap_or(false);
            d.insert_temp(held_id, held);
            previous
        });
        let mut control_actions = Vec::new();
        let mut descriptions = Vec::new();
        for (axis, binding) in [control.x.as_ref(), control.y.as_ref()].into_iter().enumerate() {
            let Some(binding) = binding else { continue };
            let Some(p) = parameters.iter().find(|p| p.id == binding.parameter) else { continue };
            descriptions.push(format!("{}: {} {}", p.name, p.value, p.unit));
            if control.gesture == UiGesture::Menu {
                let key_open =
                    focused && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
                let mut popup = egui::Popup::menu(&response);
                if response.double_clicked() {
                    popup = popup.open_memory(Some(egui::SetOpenCommand::Bool(false)));
                    control_actions.push(Action::new("device.setParam",json!({"track":track.0,"device":device,"param":p.id,"value":p.default,"continuous":false,"gesture":control.id})));
                } else if key_open {
                    popup = popup.open_memory(Some(egui::SetOpenCommand::Toggle));
                }
                let frame = egui::Frame::popup(ui.style())
                    .corner_radius(0)
                    .fill(color(loaded.palette.panel))
                    .stroke(Stroke::new(1., color(loaded.palette.accent)));
                let mut picked = None;
                popup.frame(frame).show(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    ui.spacing_mut().scroll =
                        egui::style::ScrollStyle { bar_width: 6., ..egui::style::ScrollStyle::solid() };
                    ui.spacing_mut().interact_size.y = 14.;
                    ui.visuals_mut().selection.bg_fill = color(loaded.palette.accent);
                    let choices: Vec<(f64, &str)> = if let Some(Widget::ValueMenu { choices }) = &control.widget {
                        choices.iter().map(|(v, label)| (f64::from(*v), label.as_str())).collect()
                    } else {
                        p.ui.options.iter().enumerate().map(|(i, label)| (i as f64, label.as_str())).collect()
                    };
                    let text_width = choices
                        .iter()
                        .map(|(_, label)| {
                            ui.fonts_mut(|fonts| {
                                fonts
                                    .layout_no_wrap((*label).into(), egui::FontId::proportional(11.), Color32::WHITE)
                                    .size()
                                    .x
                            })
                        })
                        .fold(0., f32::max);
                    let scroll = &ui.spacing().scroll;
                    let width = text_width
                        + ui.spacing().button_padding.x * 2.
                        + scroll.bar_width
                        + scroll.bar_inner_margin
                        + scroll.bar_outer_margin;
                    ui.set_min_width(
                        width.ceil() + if matches!(control.widget, Some(Widget::WaveformMenu)) { 48. } else { 0. },
                    );
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    egui::ScrollArea::vertical().max_height(240.).show(ui, |ui| {
                        for (value, label) in choices {
                            let selected = p.value.round() == value;
                            let text = egui::RichText::new(label).size(11.).color(color(if selected {
                                loaded.palette.accent_text
                            } else {
                                loaded.palette.text
                            }));
                            let clicked = if matches!(control.widget, Some(Widget::WaveformMenu)) {
                                let response = ui.add_sized(
                                    Vec2::new(ui.available_width(), 18.),
                                    egui::Button::selectable(selected, text).right_text(()),
                                );
                                let icon = Rect::from_min_size(
                                    response.rect.left_top() + Vec2::new(text_width + 12., 3.),
                                    Vec2::new(36., 10.),
                                );
                                let points = (0..49)
                                    .map(|i| {
                                        let t = f64::from(i) / 48.;
                                        let y = waveform(value as usize, t * if value == 5. { 3. } else { 1. });
                                        Pos2::new(
                                            icon.left() + t as f32 * icon.width(),
                                            icon.center().y - y as f32 * 4.,
                                        )
                                    })
                                    .collect();
                                ui.painter().add(egui::Shape::line(
                                    points,
                                    Stroke::new(
                                        1.,
                                        color(if selected {
                                            loaded.palette.accent_text
                                        } else {
                                            loaded.palette.accent
                                        }),
                                    ),
                                ));
                                response.clicked()
                            } else {
                                ui.selectable_label(selected, text).clicked()
                            };
                            if clicked {
                                picked = Some(value);
                                ui.close();
                            }
                        }
                    });
                });
                if let Some(value) = picked.filter(|v| *v != p.value) {
                    control_actions.push(Action::new("device.setParam",json!({"track":track.0,"device":at.value(),"param":p.id,"value":value,"continuous":false,"gesture":control.id})));
                }
                continue;
            }
            let old = normalized(p, binding.logarithmic);
            let mut next = old;
            match control.gesture {
                UiGesture::Position if response.dragged() || response.clicked() => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        next = if axis == 0 {
                            (pos.x - bounds.left()) as f64 / w as f64
                        } else {
                            (bounds.bottom() - pos.y) as f64 / h as f64
                        };
                    }
                }
                UiGesture::Drag if response.dragged() => {
                    let delta = response.drag_delta();
                    next += if axis == 0 {
                        delta.x as f64 / crate::kit::widgets::PARAMETER_DRAG_PIXELS
                    } else {
                        -delta.y as f64 / crate::kit::widgets::PARAMETER_DRAG_PIXELS
                    } * fine
                        * f64::from(binding.drag_scale);
                }
                UiGesture::Toggle if response.clicked() => next = if old >= 0.5 { 0.0 } else { 1.0 },
                UiGesture::Set(value) | UiGesture::Tab(value) if response.clicked() || button => {
                    let value = if matches!(control.gesture, UiGesture::Tab(_)) && p.value == f64::from(value) {
                        0.
                    } else {
                        f64::from(value)
                    };
                    next = if binding.logarithmic {
                        (value.ln() - p.min.ln()) / (p.max.ln() - p.min.ln())
                    } else {
                        (value - p.min) / (p.max - p.min)
                    };
                }
                UiGesture::Momentary => next = f64::from(response.is_pointer_button_down_on() || button),
                _ => {}
            }
            if !matches!(
                control.gesture,
                UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Tab(_) | UiGesture::Menu
            ) {
                next += (if axis == 0 { dx } else { dy }) as f64 * 0.01 * fine;
            }
            let mut value = if binding.logarithmic {
                (p.min.ln() + next.clamp(0.0, 1.0) * (p.max.ln() - p.min.ln())).exp()
            } else {
                p.min + next.clamp(0.0, 1.0) * (p.max - p.min)
            };
            let discrete = matches!(
                p.control,
                suhina_device_sdk::ParameterControl::Integer
                    | suhina_device_sdk::ParameterControl::Toggle
                    | suhina_device_sdk::ParameterControl::Button
                    | suhina_device_sdk::ParameterControl::Choice
                    | suhina_device_sdk::ParameterControl::Radio
            );
            let step_click = if matches!(control.widget, Some(Widget::Stepper { .. })) && response.clicked() {
                response
                    .interact_pointer_pos()
                    .filter(|pos| pos.x >= bounds.right() - 12.)
                    .map_or(0, |pos| if pos.y < bounds.center().y { 1 } else { -1 })
            } else {
                0
            };
            let key_delta = (if axis == 0 { dx } else { dy }) + step_click;
            if key_delta != 0
                && !matches!(
                    control.gesture,
                    UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Tab(_) | UiGesture::Menu
                )
                && (discrete || p.ui.step > 0.0 || step_click != 0)
            {
                let step = if discrete {
                    p.ui.step.max(1.0)
                } else if let Some(Widget::Stepper { decimals }) = control.widget {
                    10_f64.powi(-(decimals.min(8) as i32)) * fine
                } else {
                    p.ui.step * fine
                };
                value = (p.value + f64::from(key_delta) * step).clamp(p.min, p.max);
            }
            if response.double_clicked() {
                value = p.default;
            }
            // Reuse the SDK's discrete/range rules through the shared command.
            let changed = next != old || response.double_clicked() || step_click != 0;
            let interacting = response.clicked()
                || response.dragged()
                || dx != 0
                || dy != 0
                || (control.gesture == UiGesture::Momentary && (held || was_held));
            if (interacting && changed) || response.drag_stopped() || (was_held && !held) {
                control_actions.push(Action::new(
                    "device.setParam",
                    json!({
                        "track": track.0, "device": at.value(), "param": p.id, "value": value,
                        "continuous": response.dragged() || response.is_pointer_button_down_on() || button,
                        "gesture": control.id
                    }),
                ));
            }
        }
        // Both axes are one undo gesture, including a single click or a keyboard step.
        let count = control_actions.len();
        for (index, mut action) in control_actions.into_iter().enumerate() {
            if index + 1 < count {
                action.params["continuous"] = json!(true);
            }
            actions.push(action);
        }
        response.widget_info(|| {
            let kind = if matches!(
                control.gesture,
                UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Tab(_) | UiGesture::Menu
            ) {
                egui::WidgetType::Button
            } else {
                egui::WidgetType::DragValue
            };
            let mut info = egui::WidgetInfo::labeled(kind, true, &control.name);
            info.current_text_value = Some(descriptions.join(", "));
            info
        });
        if focused || response.is_pointer_button_down_on() || response.dragged() {
            let r = bounds.shrink(0.5);
            let leg = 3.0_f32.min(r.width() * 0.25).min(r.height() * 0.25);
            let stroke = Stroke::new(1., Color32::WHITE);
            for (corner, dx, dy) in [
                (r.left_top(), 1., 1.),
                (r.right_top(), -1., 1.),
                (r.left_bottom(), 1., -1.),
                (r.right_bottom(), -1., -1.),
            ] {
                painter.line_segment([corner + Vec2::new(dx * leg, 0.), corner], stroke);
                painter.line_segment([corner, corner + Vec2::new(0., dy * leg)], stroke);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(strokes, vec![Stroke::new(2., SAMPLE_MARKER)]);
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
                    egui::Shape::LineSegment { points, stroke } if *stroke == Stroke::new(2., SAMPLE_MARKER)
                        && points[0].x == points[1].x && points[0].y == 28.
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
        let mut graph = suhina_device_registry::stock::definition("sampler").unwrap();
        let mut canvas = graph.ui.take().unwrap();
        canvas.controls.retain(|c| !matches!(c.widget, Some(Widget::SampleRecording)));
        canvas.controls.iter_mut().find(|c| matches!(c.widget, Some(Widget::SampleWaveform))).unwrap().bounds =
            [0., 22., 900., 76.];
        let canvas = Arc::new(canvas);
        let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(916., 184.)).build_ui(move |ui| {
            crate::kit::theme::install(ui.ctx());
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 168.)),
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
        let definition = suhina_device_registry::stock::definition("sampler").unwrap();
        let canvas = Arc::new(definition.ui.unwrap());
        let specs = definition.parameters;
        let mut parameters: Vec<_> = specs
            .iter()
            .map(|p| GraphParameter {
                id: p.id.clone(),
                name: p.name.clone(),
                default: p.default,
                min: p.min,
                max: p.max,
                value: p.default,
                ui: p.ui.clone(),
                unit: p.unit.clone(),
                control: p.control,
            })
            .collect();
        let empty_canvas = canvas.clone();
        let empty_parameters = parameters.clone();
        let mut empty = egui_kittest::Harness::builder().with_size(Vec2::new(916., 184.)).build_ui(move |ui| {
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 168.)),
                &empty_canvas,
                &empty_parameters,
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut Vec::new(),
            );
        });
        empty.run_steps(2);
        assert!(empty.query_by_label("warp").is_none());
        for (position, param, expected) in
            [(Pos2::new(120., 10.), "mode", 1.), (Pos2::new(850., 10.), "panel_view", 1.)]
        {
            let actions = click(&canvas, &parameters, position);
            assert!(actions.iter().any(|a| a.params["param"] == param && a.params["value"] == expected), "{param}");
        }
        parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 1.;
        for (current, position, expected) in [
            (1., Pos2::new(850., 10.), 0.),
            (1., Pos2::new(760., 10.), 2.),
            (2., Pos2::new(760., 10.), 0.),
            (2., Pos2::new(850., 10.), 1.),
        ] {
            parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = current;
            let actions = click(&canvas, &parameters, position);
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
            let time = 0.5
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
                    .any(|a| a.params["param"] == param && (a.params["value"].as_f64().unwrap() - 0.5).abs() < 1e-5),
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
            ("AMP_envelope", 3, "sustain", 0.6 + 8. / 24.),
            ("FILTER_envelope", 3, "filter_sustain", 0.6 + 8. / 24.),
            ("PITCH_envelope", 1, "pitch_env", 44.),
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
            let actions = pointer_gesture(&canvas, &parameters, start, Some(start - Vec2::new(0., 8.)));
            assert!(
                actions.iter().any(
                    |a| a.params["param"] == param && (a.params["value"].as_f64().unwrap() - expected).abs() < 1e-5
                ),
                "vertical {param}: {actions:?}"
            );
        }
        for (id, value) in [("attack", 0.), ("decay", 0.), ("sustain", 0.78)] {
            parameters.iter_mut().find(|p| p.id == id).unwrap().value = value;
        }
        let zero = EnvelopePlot::new(
            Rect::from_min_size(Pos2::new(8., 126.), Vec2::new(284., 40.)),
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
        parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 2.;
        let actions = pointer_gesture(&canvas, &parameters, Pos2::new(200., 140.), Some(Pos2::new(244., 136.)));
        assert!(actions.iter().any(
            |a| a.params["param"] == "lfo_rate" && (a.params["value"].as_f64().unwrap() - 0.4472136).abs() < 0.001
        ));
        assert!(!actions.iter().any(|a| a.params["param"] == "lfo_pitch"));
        parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 0.;
        let actions = click(&canvas, &parameters, Pos2::new(639., 95.));
        assert!(actions.iter().any(|a| a.params["param"] == "warp" && a.params["value"] == 1.));
        let actions = pointer_gesture(&canvas, &parameters, Pos2::new(640., 136.), Some(Pos2::new(640., 126.)));
        assert!(
            actions
                .iter()
                .any(|a| a.params["param"] == "gain" && (a.params["value"].as_f64().unwrap() - 1.8).abs() < 0.001),
            "{actions:?}"
        );
        let actions = click(&canvas, &parameters, Pos2::new(286., 158.));
        assert!(actions.iter().any(|a| a.params["param"] == "filter_slope" && a.params["value"] == 1.));
        for (mode, playback, param) in [(0., 0., "start"), (0., 1., "start"), (0., 2., "loop_start"), (1., 2., "start")]
        {
            parameters.iter_mut().find(|p| p.id == "mode").unwrap().value = mode;
            parameters.iter_mut().find(|p| p.id == "playback").unwrap().value = playback;
            let actions = pointer_gesture(&canvas, &parameters, Pos2::new(8., 30.), Some(Pos2::new(229., 30.)));
            assert!(
                actions
                    .iter()
                    .any(|a| a.params["param"] == param && (a.params["value"].as_f64().unwrap() - 0.25).abs() < 0.001),
                "{param}: {actions:?}"
            );
            assert!(
                !actions.iter().any(|a| a.params["param"] == if param == "start" { "loop_start" } else { "start" })
            );
        }
        for view in [0., 1., 2.] {
            parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = view;
            parameters.iter_mut().find(|p| p.id == "mode").unwrap().value = 0.;
            parameters.iter_mut().find(|p| p.id == "playback").unwrap().value = 2.;
            for control in &canvas.controls {
                if control.widget.is_none() {
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
                p.value = if p.default == p.min { p.max } else { p.min };
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
            parameters.iter_mut().find(|p| p.id == "panel_view").unwrap().value = 0.;
            parameters.iter_mut().find(|p| p.id == "playback").unwrap().value = playback;
            for id in ["loop_start", "loop_end"] {
                let visible =
                    canvas.controls.iter().filter(|c| c.y.as_ref().is_some_and(|b| b.parameter == id)).any(|c| {
                        c.visible_when
                            .as_ref()
                            .unwrap()
                            .evaluate(&|id, _, _| parameters.iter().find(|p| p.id == id).unwrap().value)
                            > 0.
                    });
                assert_eq!(visible, playback == 2.);
            }
        }
        for (id, x, expected) in
            [("cutoff", 336., 3100. * 1000_f64.powf(0.05)), ("resonance", 394., 0.707 + 9.9 * 0.05)]
        {
            let actions = pointer_gesture(&canvas, &parameters, Pos2::new(x, 142.), Some(Pos2::new(x, 132.)));
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
            let light = crate::kit::theme::built_in_themes()
                .into_iter()
                .find(|(id, _)| *id == "suhina-light")
                .unwrap()
                .1
                .colors;
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
                assert!(harness.query_by_label("warp").is_some());
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
        let parameters: Vec<_> = graph
            .parameters
            .iter()
            .map(|p| GraphParameter {
                id: p.id.clone(),
                name: p.name.clone(),
                min: p.min,
                max: p.max,
                default: p.default,
                value: p.default,
                unit: p.unit.clone(),
                control: p.control,
                ui: p.ui.clone(),
            })
            .collect();
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
                    let width = if *align == TextAlign::Center {
                        2. * x.min(control.bounds[2] - x)
                    } else {
                        control.bounds[2] - x
                    };
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
        let nord = crate::kit::theme::Theme::from_json(include_str!("../../themes/nord.json")).0;
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
                .any(|a| a.params["param"] == "offset_left"
                    && (a.params["value"].as_f64().unwrap() - 1.998).abs() < 1e-6)
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
        let parameters: Vec<_> = graph
            .parameters
            .iter()
            .map(|p| GraphParameter {
                id: p.id.clone(),
                name: p.name.clone(),
                min: p.min,
                max: p.max,
                default: p.default,
                value: p.default,
                unit: p.unit.clone(),
                control: p.control,
                ui: p.ui.clone(),
            })
            .collect();
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
        assert_eq!(
            before,
            harness.get_by_label("1/8 T").rect(),
            "hovering the scroll bar must not resize menu content"
        );
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
        let graph = suhina_device_registry::stock::definition("sampler").unwrap();
        let canvas = Arc::new(graph.ui.unwrap());
        let parameters: Vec<_> = graph
            .parameters
            .iter()
            .map(|p| GraphParameter {
                id: p.id.clone(),
                name: p.name.clone(),
                default: p.default,
                min: p.min,
                max: p.max,
                value: if p.id == "playback" { 3. } else { p.default },
                ui: p.ui.clone(),
                unit: p.unit.clone(),
                control: p.control,
            })
            .collect();
        for (id, expected) in [("loop_start", false), ("loop_end", false), ("sensitivity", true), ("slice_by", true)] {
            let visible =
                canvas.controls.iter().filter(|c| c.x.iter().chain(c.y.iter()).any(|b| b.parameter == id)).any(|c| {
                    c.visible_when.as_ref().is_none_or(|v| {
                        v.evaluate(&|id, _, _| parameters.iter().find(|p| p.id == id).unwrap().value) > 0.
                    })
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
                Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 168.)),
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
        let graph = suhina_device_registry::stock::definition("sampler").unwrap();
        let canvas = Arc::new(graph.ui.unwrap());
        let parameters: Vec<_> = graph
            .parameters
            .iter()
            .map(|p| GraphParameter {
                id: p.id.clone(),
                name: p.name.clone(),
                default: p.default,
                min: p.min,
                max: p.max,
                value: match p.id.as_str() {
                    "playback" | "slice_by" => 3.,
                    "start" => 0.2,
                    "end" => 0.8,
                    _ => p.default,
                },
                ui: p.ui.clone(),
                unit: p.unit.clone(),
                control: p.control,
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
                        Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 168.)),
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
                } else {
                    panic!("unexpected edit {action:?}");
                }
            }
            if pressed != Some(false) {
                assert_eq!(edits.len(), edits_before, "drag preview must not dispatch project edits");
            }
            output
        };
        let field = Rect::from_min_max(Pos2::new(8., 32.), Pos2::new(892., 74.));
        let line = Pos2::new(SampleView::default().x(0.3, field), 50.);
        frame(line, None);
        let output = frame(line, None);
        assert_eq!(output.platform_output.cursor_icon, egui::CursorIcon::ResizeHorizontal);
        assert!(
            output
                .shapes
                .iter()
                .any(|s| matches!(&s.shape,egui::Shape::LineSegment {stroke,..} if stroke.color==Color32::GRAY))
        );
        frame(line, Some(true));
        let moved = Pos2::new(SampleView::default().x(0.6, field), 50.);
        let preview = frame(moved, None);
        assert_eq!(ctx.data(|d| d.get_temp::<Arc<[f64]>>(id.with("manual-points"))).unwrap().as_ref(), &[0.3, 0.5]);
        assert!(
            preview.shapes.iter().any(|s| matches!(&s.shape,
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == SAMPLE_MARKER && (points[0].x - moved.x).abs() < 0.01
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
        let empty = Pos2::new(SampleView::default().x(0.4, field), 50.);
        frame(empty, None);
        frame(empty, Some(true));
        frame(empty, Some(false));
        frame(empty, Some(true));
        frame(empty, Some(false));
        let points = ctx.data(|d| d.get_temp::<Arc<[f64]>>(id.with("manual-points"))).unwrap();
        assert_eq!(points.len(), 2);
        assert!((points[0] - 0.4).abs() < 1e-6);
        let blank = Pos2::new(SampleView::default().x(0.7, field), 50.);
        frame(blank, None);
        frame(blank, Some(true));
        frame(blank + Vec2::new(-25., -20.), None);
        frame(blank + Vec2::new(-25., -20.), Some(false));
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
        drop(frame);
        assert_eq!(edits.len(), 3, "navigation must not emit project edits; line drag commits once on release");
        assert_eq!(edits[0].params["continue"], false);
        assert!(edits.iter().all(|a| a.params["continue"] == false));
        let zoomed = view.dragged(blank, Vec2::new(1e6, 1e6), field, 48000);
        assert_eq!(zoomed, SampleView::default());
        let zoomed = view.dragged(blank, Vec2::new(-1e6, -1e6), field, 48000);
        assert!(zoomed.span >= 1. / 48000.);
        assert!(zoomed.start + zoomed.span <= 1.);
        ctx.data_mut(|d| {
            d.insert_temp(
                id,
                Arc::new(suhina_engine::AudioData { sample_rate: 48000, channels: 1, samples: vec![0.; 32] }),
            )
        });
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 168.)),
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
                Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 168.)),
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
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../../device-sdk/examples/delay.rs"]
mod delay_example;
