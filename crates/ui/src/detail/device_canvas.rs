// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SDK drawing programs are evaluated only on the UI thread.
use crate::detail::detail_view::GraphParameter;
use crate::kit::surface::Backgrounds as _;
use crate::kit::widgets::{Action, fixed};
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
    sample_channel_peaks(source, range, None)
}

fn sample_channel_peaks(source: &suhina_engine::AudioData, range: (usize, usize), channel: Option<usize>) -> Vec<f32> {
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
            for (_, &sample) in
                frame.iter().enumerate().filter(|(index, _)| channel.is_none_or(|channel| channel == *index))
            {
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
                *peak = frame
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| channel.is_none_or(|channel| channel == *index))
                    .map(|(_, value)| *value)
                    .filter(|v| v.is_finite())
                    .fold(0f32, |p, v| p.max(v.abs().min(1.)));
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

pub(crate) fn finish_sample_audition(
    ctx: &egui::Context,
    project: Option<suhina_session::ProjectId>,
    actions: &mut Vec<Action>,
) {
    let id = egui::Id::new("sample-audition");
    let changed_project = ctx.data_mut(|data| {
        let old = data.get_temp::<Option<suhina_session::ProjectId>>(id.with("project")).flatten();
        data.insert_temp(id.with("project"), project);
        old != project
    });
    if changed_project {
        // Switching project closes the old player; never release a note on the new one.
        ctx.data_mut(|data| data.remove::<(TrackId, u8, f64)>(id));
        return;
    }
    if let Some((track, key, until)) = ctx.data(|d| d.get_temp::<(TrackId, u8, f64)>(id)) {
        let now = ctx.input(|i| i.time);
        if now >= until {
            actions.push(Action::new("preview.noteOff", json!({"track":track.0,"key":key})));
            ctx.data_mut(|d| d.remove::<(TrackId, u8, f64)>(id));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64((until - now).min(0.25)));
        }
    }
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
    fn zoomed(self, anchor: f32, factor: f32, field: Rect, frames: usize) -> Self {
        if !factor.is_finite() || factor <= 0. || field.width() <= 0. {
            return self;
        }
        let fraction = f64::from((anchor - field.left()) / field.width()).clamp(0., 1.);
        let span = (self.span / f64::from(factor)).clamp(1. / frames.max(1) as f64, 1.);
        let start = (self.point(anchor, field) - fraction * span).clamp(0., 1. - span);
        Self { start, span }
    }

    fn panned(self, pixels: f32, field: Rect) -> Self {
        if !pixels.is_finite() || field.width() <= 0. {
            return self;
        }
        Self { start: (self.start - f64::from(pixels / field.width()) * self.span).clamp(0., 1. - self.span), ..self }
    }

    fn x(self, point: f64, field: Rect) -> f32 {
        field.left() + ((point - self.start) / self.span) as f32 * field.width()
    }
    fn point(self, x: f32, field: Rect) -> f64 {
        self.start + f64::from((x - field.left()) / field.width()) * self.span
    }
    fn dragged(self, anchor: Pos2, delta: Vec2, field: Rect, frames: usize) -> Self {
        let fraction = f64::from((anchor.x - field.left()) / field.width());
        // Down zooms in, up out.
        let span = (self.span * (f64::from(-delta.y) / 120.).exp()).clamp(1. / frames.max(1) as f64, 1.);
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
    right: Option<Arc<Vec<f32>>>,
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
fn slice_edit(
    actions: &mut Vec<Action>,
    track: TrackId,
    device: &serde_json::Value,
    points: Vec<f64>,
    continued: bool,
) {
    actions.push(Action::new(
        "device.setSampleSlices",
        json!({"track":track.0,"device":device,"points":points,"continue":continued}),
    ));
}

fn sample_marker_color() -> Color32 {
    color(styled_palette(widget_palette()).accent)
}

fn sample_marker(painter: &egui::Painter, x: f32, top: f32, bottom: f32) {
    let line = [Pos2::new(x, top), Pos2::new(x, bottom)];
    painter.line_segment(line, Stroke::new(2., sample_marker_color()));
}

/// The colour of slice lines: the waveform's accent turned to the opposite hue, fully saturated
/// and bright, so the lines stand apart from the waveform in any theme.
fn slice_line(accent: Color32) -> Color32 {
    let hsva = egui::ecolor::Hsva::from(accent);
    let hue = if hsva.s < 0.15 { 0.5 } else { (hsva.h + 0.5).fract() };
    Color32::from(egui::ecolor::Hsva::new(hue, 0.85, 1.0, 1.0))
}

/// A time in a source as minutes, seconds and, when it has them, milliseconds: `0:00`,
/// `0:00.480`, `1:02.500`.
fn clock(seconds: f64) -> String {
    let millis = (seconds.max(0.) * 1000.).round().min(f64::from(u32::MAX)) as u32;
    let (minutes, rest, fraction) = (millis / 60_000, millis / 1000 % 60, millis % 1000);
    if fraction == 0 { format!("{minutes}:{rest:02}") } else { format!("{minutes}:{rest:02}.{fraction:03}") }
}

/// Plays a slice's note for a moment, as a click on the slice does.
fn audition(ui: &egui::Ui, actions: &mut Vec<Action>, track: TrackId, key: u8) {
    let id = egui::Id::new("sample-audition");
    if let Some((track, key, _)) = ui.ctx().data(|d| d.get_temp::<(TrackId, u8, f64)>(id)) {
        actions.push(Action::new("preview.noteOff", json!({"track":track.0,"key":key})));
    }
    actions.push(Action::new("preview.noteOn", json!({"track":track.0,"key":key,"velocity":100})));
    let until = ui.input(|i| i.time) + 0.25;
    ui.ctx().data_mut(|d| d.insert_temp(id, (track, key, until)));
}

#[allow(clippy::too_many_arguments)]
fn sample_waveform(
    ui: &mut egui::Ui,
    bounds: Rect,
    parameters: &[GraphParameter],
    track: TrackId,
    device: usize,
    address: serde_json::Value,
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
    painter.fill_background(bounds, 0., crate::kit::theme::control_background());
    let mut colors = crate::kit::theme::palette();
    let scoped = styled_palette(widget_palette());
    colors.accent = color(scoped.accent);
    colors.text_primary = color(scoped.text);
    colors.border = color(scoped.border);
    painter.fill_background(bounds, 0., color(scoped.control));
    let compact = bounds.height() < 100. && inline_recording;
    let field = if compact {
        Rect::from_min_max(
            bounds.min + Vec2::new(8., 8.),
            bounds.max - Vec2::new(8., if inline_recording { 26. } else { 8. }),
        )
    } else {
        // The waveform fills the control; its times sit on the grid lines along its bottom.
        Rect::from_min_max(bounds.min + Vec2::new(6., 4.), bounds.max - Vec2::new(6., 4.))
    };
    let value = |id: &str| parameters.iter().find(|p| p.id == id).map_or(0., |p| p.value) as f32;
    let edit_loop = value("mode") == 0. && value("playback") == 2.;
    let active_start = f64::from(value(if edit_loop { "loop_start" } else { "start" }));
    let active_end = f64::from(value(if edit_loop { "loop_end" } else { "end" }));
    // Where a boundary can be moved to, kept off the edges: the values here are rounded to f32.
    let boundary_low = active_start + 1e-6;
    let boundary_high = (active_end - 1e-6).max(boundary_low);
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
    let slicing = value("mode") == 2. || value("mode") == 0. && value("playback") == 3.;
    let manual = slicing && value("slice_by") == 3.;
    let mut manual_points =
        ui.ctx().data(|d| d.get_temp::<Arc<[f64]>>(source_id.with("manual-points"))).unwrap_or_else(|| {
            let mut points: Vec<_> = parameters
                .iter()
                .filter(|p| {
                    p.id.strip_prefix("slice_").is_some_and(|suffix| suffix.parse::<u8>().is_ok())
                        && p.value > 0.
                        && p.value < 1.
                })
                .map(|p| p.value)
                .collect();
            points.sort_unstable_by(f64::total_cmp);
            points.dedup();
            Arc::from(points)
        });
    let correction_mode = value("mode") == 2.;
    let edits = ui
        .ctx()
        .data(|d| d.get_temp::<Arc<suhina_device_api::sample_edits::SampleEdits>>(source_id.with("slice-edits")));
    let params: Vec<_> = suhina_device_sdk::sampler::PARAMS
        .iter()
        .map(|p| parameters.iter().find(|v| v.id == p.id).map_or(p.default, |v| v.value))
        .collect();
    let mut automatic_points = Vec::new();
    if slicing && let Some(source) = &source {
        let cache_id = source_id.with("slices");
        let cached = ui.ctx().data(|d| {
            d.get_temp::<(Arc<suhina_engine::AudioData>, suhina_device_sdk::sampler::SliceAnalysis)>(cache_id)
        });
        let analysis = if let Some((_, analysis)) = cached.filter(|(old, _)| Arc::ptr_eq(old, source)) {
            analysis
        } else {
            let analysis = suhina_device_sdk::sampler::SliceAnalysis::new(
                &source.samples,
                usize::from(source.channels),
                f64::from(source.sample_rate),
            );
            ui.ctx().data_mut(|d| d.insert_temp(cache_id, (source.clone(), analysis.clone())));
            analysis
        };
        let tempo = ui.ctx().data(|d| d.get_temp::<f64>(egui::Id::new("sampler-tempo"))).unwrap_or(120.);
        automatic_points = analysis
            .points(&params, source.frames(), f64::from(source.sample_rate), tempo)
            .into_iter()
            .filter(|point| *point > active_start && *point < active_end)
            .collect();
        if let Some(edits) = edits.as_ref().filter(|_| correction_mode) {
            manual_points = Arc::from(
                analysis
                    .edited_points(&params, source.frames(), f64::from(source.sample_rate), tempo, edits)
                    .collect::<Vec<_>>(),
            );
        } else if !manual {
            manual_points = Arc::from(
                analysis
                    .points(&params, source.frames(), f64::from(source.sample_rate), tempo)
                    .into_iter()
                    .filter(|point| *point > active_start && *point < active_end)
                    .collect::<Vec<_>>(),
            );
        }
    }
    let edit_boundary = |actions: &mut Vec<Action>, operation: &str, previous: Option<f64>, point: Option<f64>| {
        let mut params_json = json!({"track":track.0,"device":address,"operation":operation});
        if let Some(point) = point {
            params_json["point"] = json!(point);
        }
        if let Some(previous) = previous {
            params_json["previous"] = json!(previous);
            if !manual
                && automatic_points.contains(&previous)
                && let Some(source) = &source
            {
                params_json["identity"] = json!(suhina_device_sdk::sampler::SliceAnalysis::identity(
                    &params,
                    source.frames(),
                    f64::from(source.sample_rate),
                    previous
                ));
            }
        }
        actions.push(Action::new("device.editSampleSlice", params_json));
    };
    let axis = source.as_ref().and_then(|source| {
        suhina_device_sdk::sample_axis::SampleAxis::new(
            source.frames(),
            f64::from(source.sample_rate),
            f64::from(value("source_bpm")),
            f64::from(value("first_beat")),
        )
    });
    let division =
        [0., 1., 0.5, 0.25, 0.125, 1. / 3., 1. / 6.].get(value("grid_division") as usize).copied().unwrap_or(0.);
    let snap_enabled = value("grid_snap") > 0. && !ui.input(|i| i.modifiers.alt);
    let snap = |point: f64| {
        if snap_enabled { axis.and_then(|axis| axis.snap(point, division)).unwrap_or(point) } else { point }
    };
    let response = ui.interact(field, source_id.with("wave-navigation"), egui::Sense::click_and_drag());
    let selected_id = source_id.with("selected-slice");
    let mut selected = ui.ctx().data(|d| d.get_temp::<f64>(selected_id)).unwrap_or(active_start);
    if slicing
        && response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let point = view.point(pointer.x, field);
        selected =
            manual_points.iter().copied().take_while(|boundary| *boundary <= point).last().unwrap_or(active_start);
        ui.ctx().data_mut(|d| d.insert_temp(selected_id, selected));
    }
    let selection_start = manual_points
        .iter()
        .copied()
        .rfind(|point| *point > active_start && *point <= selected)
        .unwrap_or(active_start);
    let selection_end =
        manual_points.iter().copied().find(|point| *point > selection_start).unwrap_or(active_end).min(active_end);
    let selected_index =
        manual_points.iter().filter(|point| **point > active_start && **point <= selection_start).count();
    let selected_key =
        (value("root") as usize).checked_add(selected_index).filter(|key| *key <= 127).map(|key| key as u8);
    // A click on a slice picks it and plays it.
    if slicing
        && response.clicked()
        && let Some(key) = selected_key
    {
        audition(ui, actions, track, key);
    }
    let hovered = ui.input(|i| i.pointer.hover_pos()).filter(|p| field.intersect(visible).contains(*p));
    if let Some(pointer) = hovered {
        let (zoom, wheel, shift) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta(), i.modifiers.shift));
        if let Some(source) = &source {
            if zoom != 1. {
                view = view.zoomed(pointer.x, zoom, field, source.frames());
            } else {
                let horizontal = if shift && wheel.x == 0. { wheel.y } else { wheel.x };
                view = view.panned(horizontal, field);
            }
        }
    }
    let pan_id = source_id.with("middle-pan");
    let (middle_pressed, middle_down, delta) = ui.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Middle),
            i.pointer.button_down(egui::PointerButton::Middle),
            i.pointer.delta(),
        )
    });
    let mut panning = ui.ctx().data(|d| d.get_temp::<bool>(pan_id)).unwrap_or(false);
    if middle_pressed {
        panning = hovered.is_some();
    }
    if !middle_down {
        panning = false;
    }
    if panning {
        view = view.panned(delta.x, field);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    ui.ctx().data_mut(|d| d.insert_temp(pan_id, panning));
    response.context_menu(|ui| {
        if slicing {
            if let Some(key) = selected_key {
                ui.label(format!(
                    "Slice {} · {} (MIDI {key})",
                    selected_index + 1,
                    crate::detail::piano_roll::key_name(key)
                ));
            }
            if ui.button("Zoom to selected slice").clicked() {
                if let Some(source) = &source {
                    let span = (selection_end - selection_start).clamp(1. / source.frames().max(1) as f64, 1.);
                    view = SampleView { start: selection_start.clamp(0., 1. - span), span };
                }
                ui.close();
            }
            if correction_mode && selection_start > active_start {
                let numeric_id = source_id.with("slice-number");
                let mut position = ui
                    .ctx()
                    .data(|d| d.get_temp::<(f64, f64)>(numeric_id))
                    .filter(|(old, _)| *old == selection_start)
                    .map_or(selection_start, |(_, value)| value);
                ui.horizontal(|ui| {
                    ui.label("Boundary (source fraction)");
                    ui.add(egui::DragValue::new(&mut position).speed(0.0001).range(boundary_low..=boundary_high));
                });
                ui.ctx().data_mut(|d| d.insert_temp(numeric_id, (selection_start, position)));
                if ui.button("Apply boundary").clicked() {
                    edit_boundary(actions, "move", Some(selection_start), Some(position));
                    ui.close();
                }
                if ui.button("Delete boundary").clicked() {
                    edit_boundary(actions, "delete", Some(selection_start), None);
                    ui.close();
                }
            }
        }
        if correction_mode && ui.button("Regenerate slices").clicked() {
            edit_boundary(actions, "regenerate", None, None);
            ui.close();
        }
        ui.menu_button("Grid", |ui| {
            for (index, label) in
                ["Off", "1/4", "1/8", "1/16", "1/32", "1/8 triplet", "1/16 triplet"].iter().enumerate()
            {
                if ui.selectable_label(value("grid_division") as usize == index, *label).clicked() {
                    actions.push(Action::new(
                        "device.setParam",
                        json!({"track":track.0,"device":address,"param":"grid_division","value":index}),
                    ));
                    ui.close();
                }
            }
        });
        ui.menu_button("Source time signature", |ui| {
            let meter_id = source_id.with("meter-edit");
            let mut meter = ui
                .ctx()
                .data(|d| d.get_temp::<(f64, usize)>(meter_id))
                .unwrap_or((f64::from(value("source_numerator")).max(1.), value("source_denominator") as usize));
            ui.add(egui::DragValue::new(&mut meter.0).range(1. ..=16.).speed(1.));
            for (index, label) in ["2", "4", "8", "16"].iter().enumerate() {
                ui.radio_value(&mut meter.1, index, *label);
            }
            ui.ctx().data_mut(|d| d.insert_temp(meter_id, meter));
            if ui.button("Apply time signature").clicked() {
                // One undo step: the first stays open, the second closes it.
                for (param, value, open) in
                    [("source_numerator", meter.0.round(), true), ("source_denominator", meter.1 as f64, false)]
                {
                    actions.push(Action::new(
                        "device.setParam",
                        json!({"track":track.0,"device":address,"param":param,"value":value,"continuous":open,"gesture":"source_meter"}),
                    ));
                }
                ui.close();
            }
        });
        let mut snapping = value("grid_snap") > 0.;
        if ui.checkbox(&mut snapping, "Snap to grid (Alt bypasses)").changed() {
            actions.push(Action::new(
                "device.setParam",
                json!({"track":track.0,"device":address,"param":"grid_snap","value":u8::from(snapping)}),
            ));
        }
        if ui.button("Show whole sample").clicked() {
            view = SampleView::default();
            ui.close();
        }
        if ui.button("Zoom to source region").clicked() {
            if let Some(source) = &source {
                let minimum = 1. / source.frames().max(1) as f64;
                let span = (active_end - active_start).clamp(minimum, 1.);
                view = SampleView { start: active_start.clamp(0., 1. - span), span };
            }
            ui.close();
        }
    });
    let nearest = if correction_mode || manual {
        hovered.and_then(|p| nearest_slice(&manual_points, view, field, p, active_start, active_end))
    } else {
        None
    };
    let drag_id = source_id.with("wave-drag");
    if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down()) {
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
    let double_click = if (correction_mode || manual)
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
    if double_click && slicing && source.is_some() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let mut points = manual_points.to_vec();
            if let Some(index) = nearest {
                if correction_mode {
                    edit_boundary(actions, "delete", points.get(index).copied(), None);
                } else {
                    points.remove(index);
                    slice_edit(actions, track, &address, points, false);
                }
            } else {
                let point = snap(view.point(pointer.x, field));
                if point > active_start && point < active_end && point > 0. && point < 1. {
                    if correction_mode {
                        edit_boundary(actions, "add", None, Some(point));
                    } else {
                        points.push(point);
                        slice_edit(actions, track, &address, points, false);
                    }
                }
            }
        }
    } else if (response.dragged_by(egui::PointerButton::Primary)
        || response.drag_stopped_by(egui::PointerButton::Primary))
        && let Some(state) = &mut drag
    {
        let delta = response.interact_pointer_pos().map_or(state.delta, |p| p - state.anchor);
        state.delta = delta;
        if let Some(index) = state.selected {
            let mut points = state.points.to_vec();
            if let Some(point) = points.get_mut(index) {
                *point = snap(state.points[index] + f64::from(delta.x / field.width()) * state.view.span)
                    .max(boundary_low)
                    .min(boundary_high)
                    .clamp(f64::EPSILON, 1. - f64::EPSILON);
                let destination = *point;
                points.sort_unstable_by(f64::total_cmp);
                points.dedup();
                // Preparing the audio graph on each pointer move stalls the UI.
                if response.drag_stopped() && points.as_slice() != state.points.as_ref() {
                    if correction_mode {
                        edit_boundary(actions, "move", state.points.get(index).copied(), Some(destination));
                    } else {
                        slice_edit(actions, track, &address, points.clone(), false);
                    }
                }
                manual_points = Arc::from(points);
            }
        } else if let Some(source) = &source {
            // Away from a boundary, dragging zooms the view, up and down, and moves it sideways.
            view = state.view.dragged(state.anchor, delta, field, source.frames());
        }
    }
    let cursor = if panning {
        egui::CursorIcon::Grabbing
    } else if nearest.is_some() || drag.as_ref().is_some_and(|d| d.selected.is_some()) {
        egui::CursorIcon::ResizeHorizontal
    } else {
        egui::CursorIcon::Default
    };
    response.on_hover_cursor(cursor).on_hover_text(
        "Drag down to zoom in, up to zoom out, sideways to move. Ctrl+wheel: zoom at pointer. Shift+wheel or middle drag: pan. Click a slice to play it. Right-click: fit or edit. Slice: double-click to add/remove a boundary; drag a line to move it. Alt bypasses snap.",
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
    let active_rect = Rect::from_min_max(
        Pos2::new(view.x(active_start, field), field.top()),
        Pos2::new(view.x(active_end, field), field.bottom()),
    )
    .intersect(field);
    if active_rect.is_positive() {
        wave_painter.rect_filled(active_rect, 0., colors.accent.gamma_multiply(0.06));
    }
    if slicing && selection_end > selection_start {
        let selected_rect = Rect::from_min_max(
            Pos2::new(view.x(selection_start, field), field.top()),
            Pos2::new(view.x(selection_end, field), field.bottom()),
        )
        .intersect(field);
        if selected_rect.is_positive() {
            wave_painter.rect_filled(selected_rect, 0., colors.accent.gamma_multiply(0.28));
            wave_painter.line_segment(
                [selected_rect.left_bottom(), selected_rect.right_bottom()],
                Stroke::new(3., colors.accent),
            );
        }
    }
    let label_y = field.bottom() - 7.;
    if let Some(axis) = axis.filter(|_| division > 0.) {
        // Bound work by the available screen width, even for long sources at high BPM.
        let mut step = division;
        let visible_beats = axis.beat(view.start + view.span) - axis.beat(view.start);
        while visible_beats / step > f64::from(field.width().max(1.) / 12.) {
            step *= 2.;
        }
        let first = (axis.beat(view.start) / step).ceil();
        let mut last_label = field.left() - 140.;
        for index in 0..1024 {
            let beat = (first + f64::from(index)) * step;
            let point = axis.point(beat);
            if point > view.start + view.span {
                break;
            }
            let x = view.x(point, field);
            wave_painter.line_segment(
                [Pos2::new(x, field.top()), Pos2::new(x, field.bottom())],
                Stroke::new(1., colors.text_primary.gamma_multiply(0.09)),
            );
            if !compact && x >= last_label + 70. && x <= field.right() - 40. {
                last_label = x;
                let numerator = f64::from(value("source_numerator")).round().clamp(1., 16.);
                let denominator = [2., 4., 8., 16.].get(value("source_denominator") as usize).copied().unwrap_or(4.);
                let source_beat = beat * denominator / 4.;
                let bar = (source_beat / numerator).floor() as i64 + 1;
                let within = source_beat.rem_euclid(numerator) + 1.;
                painter.text(
                    Pos2::new(x + 2., label_y),
                    egui::Align2::LEFT_CENTER,
                    // Bar, beat and, between beats, the sixteenth: 5, 5.3, 5.3.2.
                    match (within.floor(), ((within.fract() * 4.).round() as i64 + 1).min(4)) {
                        (1., 1) => format!("{bar}"),
                        (beat, 1) => format!("{bar}.{beat}"),
                        (beat, sixteenth) => format!("{bar}.{beat}.{sixteenth}"),
                    },
                    crate::kit::theme::ui_font(10.),
                    colors.text_primary,
                );
            }
        }
    } else if let Some(source) = &source {
        let duration = source.frames() as f64 / f64::from(source.sample_rate.max(1));
        let visible_seconds = duration * view.span;
        let target_step = visible_seconds / f64::from((field.width() / 64.).max(1.));
        let magnitude = 10f64.powf(target_step.max(f64::MIN_POSITIVE).log10().floor());
        let step = [1., 2., 5., 10.]
            .into_iter()
            .map(|factor| factor * magnitude)
            .find(|step| *step >= target_step)
            .unwrap_or(visible_seconds);
        let first = (duration * view.start / step).ceil();
        let mut last_label = field.left() - 100.;
        for index in 0..1024 {
            let seconds = (first + f64::from(index)) * step;
            if !seconds.is_finite() || duration <= 0. || seconds > duration * (view.start + view.span) {
                break;
            }
            let point = seconds / duration;
            let x = view.x(point, field);
            wave_painter.line_segment(
                [Pos2::new(x, field.top()), Pos2::new(x, field.bottom())],
                Stroke::new(1., colors.text_primary.gamma_multiply(0.09)),
            );
            if !compact && x >= last_label + 90. && x <= field.right() - 50. {
                last_label = x;
                painter.text(
                    Pos2::new(view.x(point, field) + 2., label_y),
                    egui::Align2::LEFT_CENTER,
                    clock(seconds),
                    crate::kit::theme::ui_font(10.),
                    colors.text_primary,
                );
            }
        }
    }
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
        let cached = if let Some(cached) = cached.filter(|c| Arc::ptr_eq(&c.source, source) && c.range == range) {
            cached
        } else {
            let peaks = Arc::new(if source.channels > 1 {
                sample_channel_peaks(source, range, Some(0))
            } else {
                visible_sample_peaks(source, range)
            });
            let right = (source.channels > 1).then(|| Arc::new(sample_channel_peaks(source, range, Some(1))));
            let cached = SamplePeaks { source: source.clone(), range, peaks, right };
            ui.ctx().data_mut(|d| d.insert_temp(cache_id, cached.clone()));
            cached
        };
        let lanes = if cached.right.is_some() { 2. } else { 1. };
        for (lane, peaks) in std::iter::once(&cached.peaks).chain(cached.right.iter()).enumerate() {
            let height = field.height() / lanes;
            let center = field.top() + (lane as f32 + 0.5) * height;
            for (i, peak) in peaks.iter().enumerate() {
                let x = field.left() + i as f32 / 1024. * field.width();
                let point = view.point(x, field);
                let color = if point < f64::from(value("start")) || point > f64::from(value("end")) {
                    colors.text_primary.gamma_multiply(0.18)
                } else {
                    colors.accent
                };
                let h = peak * height * 0.44;
                wave_painter.line_segment([Pos2::new(x, center - h), Pos2::new(x, center + h)], Stroke::new(1., color));
            }
        }
        if slicing {
            // Each slice from Start to End: every other one on a lighter band, so neighbours stand
            // apart, and the ones past the last key (127) dimmed, with no line: no note plays them.
            let inside = manual_points.iter().copied().filter(|p| *p > active_start && *p < active_end);
            let edges: Vec<f64> =
                std::iter::once(active_start).chain(inside).chain(std::iter::once(active_end)).collect();
            let playable = 128usize.saturating_sub(value("root").max(0.) as usize);
            let line = slice_line(colors.accent);
            for (index, pair) in edges.windows(2).enumerate() {
                let [from, to] = [pair[0], pair[1]];
                if to < view.start || from > view.start + view.span {
                    continue;
                }
                let band = Rect::from_min_max(
                    Pos2::new(view.x(from, field), field.top()),
                    Pos2::new(view.x(to, field), field.bottom()),
                )
                .intersect(field);
                if index >= playable {
                    wave_painter.rect_filled(band, 0., crate::kit::theme::panel_background().gamma_multiply(0.6));
                    continue;
                }
                if index % 2 == 1 {
                    wave_painter.rect_filled(band, 0., colors.text_primary.gamma_multiply(0.05));
                }
                if index == 0 {
                    continue;
                }
                let x = view.x(from, field);
                let pinned = manual || edits.as_ref().is_some_and(|edits| edits.pins().contains(&from));
                // A bright line in a colour of its own over a dark edge, so it shows over the
                // waveform and over silence, with a tab at its top; one placed by hand is thicker,
                // with a round tab.
                let width = if pinned { 2.5 } else { 1.5 };
                wave_painter.line_segment(
                    [Pos2::new(x, field.top()), Pos2::new(x, field.bottom())],
                    Stroke::new(width + 2., Color32::from_black_alpha(150)),
                );
                wave_painter
                    .line_segment([Pos2::new(x, field.top()), Pos2::new(x, field.bottom())], Stroke::new(width, line));
                if pinned {
                    wave_painter.circle_filled(Pos2::new(x, field.top() + 4.), 4., line);
                } else {
                    wave_painter.add(egui::Shape::convex_polygon(
                        vec![
                            Pos2::new(x - 4., field.top()),
                            Pos2::new(x + 4., field.top()),
                            Pos2::new(x, field.top() + 6.),
                        ],
                        line,
                        Stroke::NONE,
                    ));
                }
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
        wave_painter.line_segment(
            [Pos2::new(x, field.top()), Pos2::new(x, field.bottom())],
            Stroke::new(1., colors.text_primary),
        );
    }
    for (id, caption, offset) in [
        (if edit_loop { "loop_start" } else { "start" }, "S", 0.),
        (if edit_loop { "loop_end" } else { "end" }, "E", 0.),
        ("position", "G", 40.),
    ] {
        if id == "position" && value("mode") != 1. {
            continue;
        }
        let (origin, span) =
            if id == "position" { (value("start"), (value("end") - value("start")).max(1e-6)) } else { (0., 1.) };
        let x = view.x(f64::from(origin + value(id) * span), field);
        if x < field.left() || x > field.right() {
            continue;
        }
        let y = if compact {
            bounds.top() + 8. + offset * 0.7
        } else {
            field.top() + 6. + offset.min(field.height() * 0.5)
        };
        let marker = Rect::from_center_size(Pos2::new(x, y), Vec2::new(12., if compact { 12. } else { 13. }));
        sample_marker(&painter, x, y, field.bottom());
        painter.rect_filled(marker, 0., sample_marker_color());
        painter.text(
            marker.center(),
            egui::Align2::CENTER_CENTER,
            caption,
            crate::kit::theme::ui_font(10.),
            color(accent_text(sample_marker_color())),
        );
        let response = ui
            .interact(marker, ui.id().with(("sample-marker", track.0, device, id)), egui::Sense::click_and_drag())
            .on_hover_text(format!("Drag {}", id.replace('_', " ")))
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if response.double_clicked() {
            if let Some(p) = parameters.iter().find(|p| p.id == id) {
                actions.push(Action::new(
                    "device.setParam",
                    json!({"track":track.0,"device":address,"param":id,"value":p.default,"continuous":false}),
                ));
            }
        } else if (response.dragged() || response.drag_stopped())
            && let Some(pos) = response.interact_pointer_pos()
        {
            actions.push(Action::new("device.setParam", json!({"track": track.0, "device": address, "param": id, "value": ((snap(view.point(pos.x,field)) - f64::from(origin)) / f64::from(span)).clamp(0., 1.), "continuous": !response.drag_stopped()})));
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
    let mut palette = crate::kit::theme::palette();
    let scoped = styled_palette(widget_palette());
    palette.accent = color(scoped.accent);
    palette.border = color(scoped.border);
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
        let id = ui.id().with(("sd-envelope", target.0.0, target.1, &control.id, index));
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
    color.to_srgba_unmultiplied()
}
fn accent_text(accent: egui::Color32) -> [u8; 4] {
    let luminance = 0.2126 * f32::from(accent.r()) + 0.7152 * f32::from(accent.g()) + 0.0722 * f32::from(accent.b());
    if luminance > 145.0 { [29, 28, 26, 255] } else { [255, 255, 255, 255] }
}
fn widget_palette() -> WidgetPalette {
    let colors = crate::kit::theme::palette();
    WidgetPalette {
        text: rgba(colors.text_primary),
        panel: rgba(colors.panel_background),
        control: rgba(colors.control_background),
        border: rgba(colors.border),
        accent: rgba(colors.accent),
        accent_text: accent_text(colors.accent),
    }
}
/// The widget palette of the control whose style scope is open: colours its CSS writes
/// (`color`, `background-color`, `border-color`, `accent-color`) replace the theme's.
fn styled_palette(mut palette: WidgetPalette) -> WidgetPalette {
    let Some(style) = crate::kit::style_class::computed() else { return palette };
    if style.authored.color {
        palette.text = rgba(style.color);
    }
    if style.authored.background {
        palette.control = rgba(style.background);
    }
    if style.authored.border {
        palette.border = rgba(style.border);
    }
    if style.authored.accent {
        palette.accent = rgba(style.accent);
        palette.accent_text = accent_text(style.accent);
    }
    palette
}

/// A control's drawings in a palette other than the panel's, kept so a styled control is not
/// rebuilt every frame. Bounded; cleared when full.
fn styled_drawings(
    ctx: &egui::Context,
    loaded: &Loaded,
    control: &CustomControl,
    palette: WidgetPalette,
) -> Arc<[Draw]> {
    type Cache = std::collections::HashMap<(usize, usize, [[u8; 4]; 6]), Arc<[Draw]>>;
    let key = egui::Id::new("sd-styled-drawings");
    let index = loaded.source.controls.iter().position(|c| std::ptr::eq(c, control)).unwrap_or(usize::MAX);
    let colors = [palette.text, palette.panel, palette.control, palette.border, palette.accent, palette.accent_text];
    let cache_key = (Arc::as_ptr(&loaded.source) as usize, index, colors);
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<std::sync::Mutex<Cache>>>(key).clone());
    let Ok(mut cache) = cache.lock() else { return control.drawings_with_palette(&palette).into_owned().into() };
    if let Some(found) = cache.get(&cache_key) {
        return found.clone();
    }
    let drawings: Arc<[Draw]> = control.drawings_with_palette(&palette).into_owned().into();
    if cache.len() >= 512 {
        cache.clear();
    }
    cache.insert(cache_key, drawings.clone());
    drawings
}

fn images(ctx: &egui::Context, source: &Arc<DeviceUi>) -> Arc<Loaded> {
    let palette = widget_palette();
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
    paint_scaled(painter, drawings, origin, parameters, loaded, Vec2::splat(1.0));
}

fn paint_scaled(
    painter: &egui::Painter,
    drawings: &[Draw],
    origin: Pos2,
    parameters: &[GraphParameter],
    loaded: &Loaded,
    scale: Vec2,
) {
    let weight = scale.x.min(scale.y);
    let lookup = |id: &str, norm: bool, log: bool| {
        parameters.iter().find(|p| p.id == id).map_or(0.0, |p| if norm { normalized(p, log) } else { p.value })
    };
    let value = |v: &UiValue| v.evaluate(&lookup);
    let point = |p: &[UiValue; 2]| origin + Vec2::new(value(&p[0]), value(&p[1])) * scale;
    let rectangle = |r: &[UiValue; 4]| {
        Rect::from_min_size(
            origin + Vec2::new(value(&r[0]), value(&r[1])) * scale,
            Vec2::new(value(&r[2]).clamp(0.0, 8192.0), value(&r[3]).clamp(0.0, 8192.0)) * scale,
        )
    };
    for draw in drawings {
        match draw {
            Draw::Rect { rect, color: c, radius } => {
                let rect = rectangle(rect);
                let radius = crate::kit::style_class::radius(*radius * weight);
                painter.rect_filled(rect, radius, crate::kit::style_class::background(color(*c)));
                crate::kit::native_css::background_rounded(painter, rect, radius);
            }
            Draw::Circle { center, radius, color: c } => {
                painter.circle_filled(
                    point(center),
                    (value(radius).clamp(0.0, 8192.0) * weight).min(8192.0),
                    color(*c),
                );
            }
            Draw::Line { from, to, color: c, width } => {
                painter.line_segment([point(from), point(to)], Stroke::new(*width * weight, color(*c)));
            }
            Draw::Path { points, color: c, width, closed, fill } => {
                let points = points.iter().map(point).collect::<Vec<_>>();
                if let Some(fill) = fill {
                    painter.add(egui::Shape::convex_polygon(
                        points,
                        color(*fill),
                        Stroke::new(*width * weight, color(*c)),
                    ));
                } else if *closed {
                    painter.add(egui::Shape::closed_line(points, Stroke::new(*width * weight, color(*c))));
                } else {
                    painter.add(egui::Shape::line(points, Stroke::new(*width * weight, color(*c))));
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
                        center + Vec2::new(a.cos(), a.sin()) * radius * scale
                    })
                    .collect();
                painter.add(egui::Shape::line(points, Stroke::new(*width * weight, color(*c))));
            }
            Draw::Text { position, text, color: c, size, parameter, decimals, align, format } => {
                let label = parameter.as_ref().and_then(|id| parameters.iter().find(|p| p.id == *id)).map_or_else(
                    || text.clone(),
                    |p| match format {
                        TextFormat::Choice => {
                            p.ui.options.get(p.value.round().max(0.0) as usize).cloned().unwrap_or_default()
                        }
                        TextFormat::Frequency if p.value >= 1000.0 => {
                            format!("{text}{} kHz", fixed(p.value / 1000.0, 2))
                        }
                        TextFormat::Tempo if p.value <= 0. => format!("{text}Auto"),
                        TextFormat::Tempo => format!("{text}{} BPM", fixed(p.value, 2)),
                        TextFormat::Seconds if p.value < 1. => format!("{text}{} ms", fixed(p.value * 1000., 0)),
                        TextFormat::Seconds => format!("{text}{} s", fixed(p.value, 2)),
                        TextFormat::Percent => format!("{text}{}%", fixed(p.value * 100., 0)),
                        TextFormat::Decibels => format!("{text}{} dB", fixed(p.value, 1)),
                        TextFormat::Balance if p.value.abs() < 0.005 => format!("{text}C"),
                        TextFormat::Balance => {
                            format!("{text}{} {}", fixed(p.value.abs() * 100., 0), if p.value < 0. { "L" } else { "R" })
                        }
                        _ => format!(
                            "{text}{}{}{}",
                            fixed(p.value, *decimals),
                            if p.unit.is_empty() || p.unit == "%" { "" } else { " " },
                            p.unit
                        ),
                    },
                );
                let position = point(position);
                let ink = crate::kit::style_class::foreground(color(*c));
                let mut font = crate::kit::theme::ui_font((*size * weight).clamp(1.0, 256.0));
                let mut galley = painter.layout_no_wrap(label.clone(), font.clone(), ink);
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
                        galley = painter.layout_no_wrap(label, font, ink);
                    }
                }
                let offset = if *align == TextAlign::Center { Vec2::new(galley.size().x / 2., 0.) } else { Vec2::ZERO };
                crate::kit::widgets::paint_text(painter, position - offset, galley, ink);
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
                                uv.max = uv.min + Vec2::new(r.width() / (w * scale.x), r.height() / (h * scale.y));
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
fn control_selected(control: &CustomControl, parameters: &[GraphParameter]) -> bool {
    control.x.iter().chain(control.y.iter()).any(|binding| {
        parameters.iter().find(|parameter| parameter.id == binding.parameter).is_some_and(|parameter| {
            match control.gesture {
                UiGesture::Set(target) | UiGesture::Tab(target) => parameter.value == f64::from(target),
                _ => normalized(parameter, binding.logarithmic) >= 0.5,
            }
        })
    })
}

fn control_class(control: &suhina_device_sdk::ui::CustomControl) -> &'static str {
    match &control.widget {
        Some(Widget::Knob { .. } | Widget::Dial { .. }) => "knob",
        Some(Widget::Button { .. } | Widget::Disclosure { .. }) => "button",
        Some(Widget::Number { .. } | Widget::Stepper { .. }) => "value",
        Some(Widget::Menu | Widget::ValueMenu { .. } | Widget::WaveformMenu) => "dropdown",
        Some(Widget::Label { .. }) => "label",
        Some(Widget::Panel { .. }) => "device-section",
        Some(Widget::Separator) => "separator",
        Some(Widget::Envelope { .. }) => "envelope",
        Some(Widget::SampleWaveform | Widget::WaveformPreview { .. }) => "waveform",
        Some(Widget::FilterBand { .. }) => "xy-control",
        _ => "device-control",
    }
}

/// Reuse SDK drawings/geometry and native responses. Artwork replaces only the
/// standard face/pointer; labels, readouts and the input/command path stay native.
#[allow(clippy::too_many_arguments)]
fn paint_control(
    painter: &egui::Painter,
    effects: &egui::Painter,
    control: &suhina_device_sdk::ui::CustomControl,
    drawings: &[Draw],
    bounds: Rect,
    parameters: &[GraphParameter],
    loaded: &Loaded,
    response: Option<&egui::Response>,
) {
    let class = control_class(control);
    let _scope = response
        .map(|r| crate::kit::style_class::enter_response(class, r))
        .unwrap_or_else(|| crate::kit::style_class::enter(class));
    let palette = styled_palette(widget_palette());
    let restyled;
    let drawings: &[Draw] = if palette != loaded.palette {
        restyled = styled_drawings(painter.ctx(), loaded, control, palette);
        &restyled
    } else {
        drawings
    };
    let value = |v: &UiValue| {
        v.evaluate(&|id, norm, log| {
            parameters.iter().find(|p| p.id == id).map_or(0.0, |p| if norm { normalized(p, log) } else { p.value })
        })
    };
    let native = Vec2::new(control.bounds[2], control.bounds[3]).max(Vec2::splat(1.0));
    let knob = matches!(control.widget, Some(Widget::Knob { .. } | Widget::Dial { .. }));
    let mut scale = (bounds.size() / native).clamp(Vec2::splat(1.0 / 8192.0), Vec2::splat(4096.0));
    if knob {
        scale = Vec2::splat(scale.x.min(scale.y));
    }
    let origin = if knob { bounds.center() - native * scale * 0.5 } else { bounds.min };
    let point = |p: &[UiValue; 2]| origin + Vec2::new(value(&p[0]), value(&p[1])) * scale;
    let circle = knob
        .then(|| {
            drawings.iter().find_map(|draw| match draw {
                Draw::Circle { center, radius, .. } => {
                    Some((point(center), (value(radius).clamp(0.0, 8192.0) * scale.x.min(scale.y)).min(8192.0)))
                }
                _ => None,
            })
        })
        .flatten();
    let on = control_selected(control, parameters);
    let state = response.map_or(crate::kit::skin::State::Normal, |r| crate::kit::skin::State::response(r, !knob && on));
    if control.x.as_ref().is_some_and(|b| b.parameter == "mode")
        && matches!(control.gesture, UiGesture::Set(_))
        && let Some(Widget::Button { label, .. }) = &control.widget
        && matches!(label.as_str(), "Sample" | "Slice" | "Grain")
        && bounds.height() >= 40.
    {
        paint_sample_mode(painter, bounds, label, on, state, palette);
        return;
    }
    let ink = crate::kit::style_class::foreground(crate::kit::theme::text_value());
    let mut body = None;
    let mut knob_effects = false;
    for draw in drawings.iter() {
        if let Some((center, radius)) = circle {
            let rect = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
            if matches!(draw, Draw::Circle { .. }) {
                let replaced =
                    *body.get_or_insert_with(|| crate::kit::skin::paint(painter, rect, "knob.body", state, ink, 0.0));
                if replaced {
                    if !knob_effects {
                        crate::kit::native_css::background_rounded(effects, rect, radius);
                        knob_effects = true;
                    }
                    continue;
                }
            }
            if let Draw::Line { from, to, .. } = draw
                && point(from).distance(center) < 0.1
            {
                let direction = point(to) - center;
                let angle = direction.x.atan2(-direction.y);
                if crate::kit::skin::paint(painter, rect, "knob.pointer", state, ink, angle) {
                    continue;
                }
            }
        }
        if matches!(control.widget, Some(Widget::Button { .. }))
            && let Draw::Rect { radius, .. } = draw
        {
            let replaced = *body.get_or_insert_with(|| {
                let fill = crate::kit::style_class::background(if on {
                    crate::kit::theme::control_active()
                } else {
                    crate::kit::theme::control_background()
                });
                let radius = crate::kit::style_class::radius(*radius * scale.x.min(scale.y));
                let painted = crate::kit::skin::button(painter, bounds, state, fill, radius);
                if painted {
                    crate::kit::native_css::background_rounded(effects, bounds, radius);
                }
                painted
            });
            if replaced {
                continue;
            }
        }
        paint_scaled(painter, std::slice::from_ref(draw), origin, parameters, loaded, scale);
        if !knob_effects
            && matches!(draw, Draw::Circle { .. })
            && let Some((center, radius)) = circle
        {
            crate::kit::native_css::background_rounded(
                effects,
                Rect::from_center_size(center, Vec2::splat(radius * 2.0)),
                radius,
            );
            knob_effects = true;
        }
    }
    if on
        && matches!(control.widget, Some(Widget::Button { .. }))
        && matches!(control.gesture, UiGesture::Set(_) | UiGesture::Tab(_))
    {
        // The sampler's page tabs: the open one outlined, as its mode buttons are.
        if control.x.as_ref().is_some_and(|binding| binding.parameter == "panel_view") {
            painter.rect_stroke(
                bounds.shrink(1.),
                3.,
                Stroke::new(1.5, color(palette.accent)),
                egui::StrokeKind::Inside,
            );
            return;
        }
        let mark = Rect::from_min_max(
            bounds.left_top() + Vec2::new(2., 4.),
            Pos2::new(bounds.left() + 5., bounds.bottom() - 4.),
        );
        painter.rect_filled(mark, 1., color(palette.accent));
    }
}

/// The sampler's mode buttons: a drawn glyph over the caption, both in the accent on a lighter
/// face for the chosen mode, in the dimmer text colour for the others.
fn paint_sample_mode(
    painter: &egui::Painter,
    bounds: Rect,
    label: &str,
    selected: bool,
    state: crate::kit::skin::State,
    palette: WidgetPalette,
) {
    let radius = crate::kit::style_class::radius(2.);
    let base = color(palette.control);
    let fill = crate::kit::style_class::background(if selected { base.gamma_multiply(1.35) } else { base });
    if !crate::kit::skin::button(painter, bounds, state, fill, radius) {
        painter.rect_filled(bounds.shrink(0.5), radius, fill);
    }
    crate::kit::native_css::background_rounded(painter, bounds, radius);
    let ink = crate::kit::style_class::foreground(if selected {
        color(palette.accent)
    } else {
        color(palette.text).gamma_multiply(0.62)
    });
    let center = Pos2::new(bounds.center().x, bounds.top() + bounds.height() * 0.36);
    let stroke = Stroke::new(1.8, ink);
    match label {
        // Three cuts across a strip: the source in pieces.
        "Slice" => {
            painter.rect_stroke(
                Rect::from_center_size(center, Vec2::new(5., 16.)),
                1.,
                stroke,
                egui::StrokeKind::Middle,
            );
            for side in [-1., 1.] {
                painter.rect_stroke(
                    Rect::from_center_size(center + Vec2::new(side * 9., 0.), Vec2::new(5., 6.)),
                    1.,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
        }
        // Scattered grains.
        "Grain" => {
            for (x, y) in [(-8., -4.), (-1., -7.), (7., -3.), (-4., 3.), (4., 5.), (10., 6.), (-10., 6.)] {
                painter.circle_filled(center + Vec2::new(x, y), 1.9, ink);
            }
        }
        // A cycle of a wave between two bars: the whole sample.
        _ => {
            for side in [-1., 1.] {
                painter.line_segment([center + Vec2::new(side * 11., -8.), center + Vec2::new(side * 11., 8.)], stroke);
            }
            let points = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.;
                    center + Vec2::new(t * 16. - 8., -(t * std::f32::consts::TAU).sin() * 6.)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
    }
    painter.text(
        Pos2::new(bounds.center().x, bounds.top() + bounds.height() * 0.76),
        egui::Align2::CENTER_CENTER,
        label,
        crate::kit::theme::ui_font(12.),
        ink,
    );
}

/// The native sampler gives up waveform height before its fixed-size controls scroll.
pub(super) fn fitted_size(canvas: &DeviceUi, available_height: f32) -> Vec2 {
    let sampler =
        canvas.size == [900., 220.] && canvas.controls.iter().any(|c| matches!(c.widget, Some(Widget::SampleWaveform)));
    Vec2::new(canvas.size[0], if sampler { available_height.clamp(168., 220.) } else { canvas.size[1] })
}

fn fitted_control_bounds(control: &CustomControl, height_reduction: f32) -> [f32; 4] {
    let [x, mut y, w, mut h] = control.bounds;
    if matches!(control.widget, Some(Widget::SampleWaveform)) {
        h -= height_reduction;
    } else if y >= 138. {
        y -= height_reduction;
    } else if control.x.as_ref().is_some_and(|b| b.parameter == "mode") && height_reduction > 0. {
        // Three mode buttons must fit beside the shorter waveform, above source controls.
        y = 24. + (y - 24.) / 24. * 20.;
        h = 18.;
    }
    [x, y, w, h]
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
    let address = at.0.iter().map(usize::to_string).collect::<Vec<_>>().join("-");
    let _style_class =
        crate::kit::style_class::enter_named("device-panel", &format!("track-{}-device-{address}", track.0));
    // The panel's own sheet applies inside this scope only, over the application theme.
    crate::kit::native_css::device_sheet(ui.ctx(), canvas.style.as_deref());
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
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
    // A clear background (a header strip's) leaves what is under the canvas showing.
    if canvas.background[3] > 0 {
        painter.fill_background(rect, 0.0, crate::kit::style_class::background(background));
        crate::kit::native_css::background_rounded(&painter, rect, 0.);
    }
    paint(&painter, &canvas.layers, rect.min, parameters, &loaded);
    let sample_panel = canvas.controls.iter().any(|c| matches!(c.widget, Some(Widget::SampleWaveform)));
    let reduction = canvas.size[1] - fitted_size(canvas, rect.height()).y;
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
        let [x, y, w, h] = fitted_control_bounds(control, reduction);
        let bounds = Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
        let name = crate::kit::control_layout::semantic_name("device", &control.id);
        if crate::kit::control_layout::placement(&name).hidden {
            continue;
        }
        let bounds = crate::kit::control_layout::place(&name, bounds, rect);
        let _control_scope = crate::kit::style_class::enter_named("device-control", &control.id);
        if matches!(control.widget, Some(Widget::Envelope { .. })) {
            let _role = crate::kit::style_class::enter("envelope");
            let mut styled_ui = crate::kit::style_class::ui(ui);
            let ui = &mut *styled_ui;
            envelope(ui, bounds, control, parameters, (track, device), actions);
            continue;
        }
        if matches!(control.widget, Some(Widget::SampleRecording)) {
            sample_recording(ui, bounds, track, device, actions);
            continue;
        }
        if matches!(control.widget, Some(Widget::SampleWaveform)) {
            let _role = crate::kit::style_class::enter("waveform");
            let mut styled_ui = crate::kit::style_class::ui(ui);
            let ui = &mut *styled_ui;
            sample_waveform(
                ui,
                bounds,
                parameters,
                track,
                device,
                at.value(),
                actions,
                !canvas.controls.iter().any(|c| matches!(c.widget, Some(Widget::SampleRecording))),
            );
            continue;
        }
        if matches!(control.widget, Some(Widget::Label { .. } | Widget::Panel { .. } | Widget::Separator)) {
            paint_control(
                &painter.with_clip_rect(bounds.intersect(painter.clip_rect())),
                &painter,
                control,
                drawings,
                bounds,
                parameters,
                &loaded,
                None,
            );
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
        // A bordered rate field (in Hz, or a number with a unit after it, as the sampler's LFO
        // rate and time) can be typed in as well as dragged. Other bordered numbers (an authored
        // panel's `number`, without a unit) keep the panel's own drag.
        if let Some(Widget::Number { bordered: true, decimals, format, prefix, suffix, .. }) = &control.widget
            && (*format == suhina_device_sdk::ui::TextFormat::Frequency
                || (*format == suhina_device_sdk::ui::TextFormat::Number && !suffix.is_empty()))
            && let Some(binding) = control.y.as_ref().or(control.x.as_ref())
            && let Some(p) = parameters.iter().find(|p| p.id == binding.parameter)
        {
            let _role = crate::kit::style_class::enter("value");
            let mut styled_ui = crate::kit::style_class::ui(ui);
            let ui = &mut *styled_ui;
            let mut value = p.value;
            let suffix = if *format == suhina_device_sdk::ui::TextFormat::Frequency { " Hz" } else { suffix.as_str() };
            let fine = ui.input(|i| if i.modifiers.shift { 0.1 } else { 1.0 });
            let response = ui
                .push_id(("sd-number", track.0, &at.0, &control.id), |ui| {
                    ui.put(
                        bounds,
                        egui::DragValue::new(&mut value)
                            .range(p.min..=p.max)
                            .speed(0.01 * fine)
                            .fixed_decimals(*decimals)
                            .prefix(prefix)
                            .suffix(suffix),
                    )
                })
                .inner
                .on_hover_text("Drag to adjust, click to type; Shift for precision");
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &control.name));
            if value.is_finite() && (response.changed() || response.drag_stopped()) {
                actions.push(Action::new(
                    "device.setParam",
                    json!({
                        "track":track.0,"device":at.value(),"param":p.id,"value":value,
                        "continuous":response.dragged(),"gesture":control.id
                    }),
                ));
            }
            continue;
        }
        let response = ui
            .interact(
                bounds,
                // Scoped to the canvas: the compact panel under an expanded one has controls of the same ids.
                ui.id().with(("sd-custom", track.0, device, &control.id)),
                if control.gesture == UiGesture::Menu { egui::Sense::click() } else { egui::Sense::click_and_drag() },
            )
            .on_hover_text(tooltip);
        let response = if control.gesture == UiGesture::Drag {
            response.on_hover_cursor(egui::CursorIcon::ResizeVertical)
        } else {
            response
        };
        paint_control(
            &painter.with_clip_rect(bounds.intersect(painter.clip_rect())),
            &painter,
            control,
            drawings,
            bounds,
            parameters,
            &loaded,
            Some(&response),
        );
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
                    control_actions.push(Action::new("device.setParam",json!({"track":track.0,"device":at.value(),"param":p.id,"value":p.default,"continuous":false,"gesture":control.id})));
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
            let discrete = matches!(
                p.control,
                suhina_device_sdk::ParameterControl::Integer
                    | suhina_device_sdk::ParameterControl::Toggle
                    | suhina_device_sdk::ParameterControl::Button
                    | suhina_device_sdk::ParameterControl::Choice
                    | suhina_device_sdk::ParameterControl::Radio
            );
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
                    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                    // A discrete value is rounded when it is written, so a drag that moves less than half
                    // a step in a frame would be lost: the unrounded position is kept while the drag lasts.
                    let position_id = response.id.with(("drag-position", axis));
                    if discrete && !response.drag_started() {
                        next = ui.data(|data| data.get_temp::<f64>(position_id)).unwrap_or(old);
                    }
                    next += if axis == 0 {
                        delta.x as f64 / crate::kit::widgets::PARAMETER_DRAG_PIXELS
                    } else {
                        -delta.y as f64 / crate::kit::widgets::PARAMETER_DRAG_PIXELS
                    } * fine
                        * f64::from(binding.drag_scale);
                    next = next.clamp(0.0, 1.0);
                    if discrete {
                        ui.data_mut(|data| data.insert_temp(position_id, next));
                    }
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
