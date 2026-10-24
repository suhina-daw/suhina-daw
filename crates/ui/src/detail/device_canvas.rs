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

/// An `.sd` device's state read into its graph, kept per state so a panel parses it once.
pub(crate) struct Parsed {
    /// The graph, without its panels (they are the fields below).
    pub graph: DeviceGraph,
    /// Its compact panel, shared so the canvas's images can be cached.
    pub canvas: Option<Arc<DeviceUi>>,
    /// Its expanded panel, opened from the device's header.
    pub expanded: Option<Arc<DeviceUi>>,
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
        let expanded = graph.expanded.take().map(Arc::new);
        Arc::new(Parsed { graph, canvas, expanded })
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
#[path = "device_canvas_tests.rs"]
mod tests;

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../../device-sdk/examples/delay.rs"]
mod delay_example;
