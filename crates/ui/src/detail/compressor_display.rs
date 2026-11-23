// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Compressor's display (the SDK's `Widget::CompressorDisplay`, docs/spec/compressor.md ›
//! The panel), drawn on the UI thread from the device's own gain computer
//! (`suhina_device_compressor::gain_change`) and its readouts, so what it shows is what sounds.
//! A row of tabs picks one of three views:
//!
//! - **Curve**: the transfer curve (input level across, output level up) with the threshold
//!   as a ring on it, dragged sideways for the threshold and up and down for the ratio, and the
//!   level the detector hears now as a dot, where the gain reduction puts it;
//! - **Activity**: the last few seconds scrolling by, the level the detector hears (the key's
//!   while one is set) filled from the bottom, the output's as a line and the gain reduction
//!   hanging from the top, with the threshold as a line dragged up and down;
//! - **Meters**: the input, gain-reduction and output meters alone, large.
//!
//! The Curve and Activity views keep the three meters beside them, the input one with an arrow
//! at the threshold that drags it. Which view shows is panel state, never a parameter. Every
//! edit is a `device.setParam` command, addressed by the device's full path, so automation and
//! undo see it like any other edit.

use std::collections::VecDeque;

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use serde_json::json;
use suhina_device_compressor::{READOUT_FLOOR_DB, gain_change, readout};
use suhina_device_sdk::ui::READOUT_IDS;
use suhina_engine::model::TrackId;

use crate::detail::detail_view::{DeviceAt, GraphParameter};
use crate::kit::theme;
use crate::kit::widgets::{Action, fixed};

/// The level range the curve, the activity and the level meters span, in dB: bottom and top.
const RANGE: (f64, f64) = (-60.0, 6.0);
/// The gain reduction the reduction meter and the activity's reduction span, in dB.
const REDUCTION_RANGE: f64 = 24.0;
/// How many seconds the activity view shows.
const HISTORY_SECONDS: f64 = 4.0;
/// How many readings a second the activity view keeps.
const HISTORY_RATE: f64 = 40.0;
/// The tab row's height.
const TABS_HEIGHT: f32 = 18.0;
/// The values row's height under the meters.
const VALUES_HEIGHT: f32 = 13.0;
/// A meter's width beside a view.
const METER_WIDTH: f32 = 9.0;
/// The room the threshold arrow takes right of the input meter.
const ARROW_WIDTH: f32 = 9.0;
/// The room the Meters view keeps on each side of a meter: its scale's numbers, or the input's
/// threshold arrow.
const METER_SIDE: f32 = 24.0;
/// The curve's points across the display.
const POINTS: usize = 120;
/// The ratio's range, which a vertical drag on the curve's ring moves through.
const RATIO_RANGE: (f64, f64) = (1.0, 20.0);
/// How far a pointer may be from the activity's threshold line and still grab it.
const LINE_GRAB: f32 = 5.0;

/// The display's three views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum View {
    #[default]
    Curve,
    Activity,
    Meters,
}

impl View {
    const ALL: [View; 3] = [View::Curve, View::Activity, View::Meters];

    fn label(self) -> &'static str {
        match self {
            View::Curve => "Curve",
            View::Activity => "Activity",
            View::Meters => "Meters",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            View::Curve => "The transfer curve: input level across, output level up",
            View::Activity => "The level the detector hears and the gain reduction over the last seconds",
            View::Meters => "The input, gain-reduction and output meters alone",
        }
    }
}

/// The theme's colors the display draws with beyond the canvas's own: the display's ground
/// (`--device-display`), the gain reduction's (`--device-series-1`) and the levels'
/// (`--device-series-2`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub display: Color32,
    pub reduction: Color32,
    pub level: Color32,
}

/// What the device heard and did in its last block (its readouts).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Live {
    /// The level the detector read, dBFS.
    pub detector: f64,
    /// The output's peak, dBFS.
    pub output: f64,
    /// The gain reduction, dB, zero or positive.
    pub reduction: f64,
    /// Whether the detector reads a key.
    pub keyed: bool,
}

/// The panel's values the display reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Values {
    pub threshold: f64,
    pub ratio: f64,
    pub knee: f64,
    /// The output gain, dB.
    pub output: f64,
    /// The dry/wet mix, 0 to 1.
    pub mix: f64,
    /// The device's readouts, when it hands any.
    pub live: Option<Live>,
}

/// The parameter's value, or `fallback` when the panel has no such id.
fn value(parameters: &[GraphParameter], id: &str, fallback: f64) -> f64 {
    parameters.iter().find(|p| p.id == id).map_or(fallback, |p| p.value)
}

fn parameter<'a>(parameters: &'a [GraphParameter], id: &str) -> Option<&'a GraphParameter> {
    parameters.iter().find(|p| p.id == id)
}

/// The readout `index`, when the device has handed its panel any.
fn readout_value(parameters: &[GraphParameter], index: usize) -> Option<f64> {
    let id = READOUT_IDS.get(index)?;
    parameters.iter().find(|p| p.id == *id).map(|p| p.value).filter(|v| v.is_finite())
}

pub(crate) fn values(parameters: &[GraphParameter]) -> Values {
    let live = (|| {
        Some(Live {
            detector: readout_value(parameters, readout::DETECTOR)?,
            output: readout_value(parameters, readout::OUTPUT)?,
            reduction: readout_value(parameters, readout::REDUCTION)?.max(0.0),
            keyed: readout_value(parameters, readout::KEYED)? > 0.5,
        })
    })();
    Values {
        threshold: value(parameters, "threshold", -12.0),
        ratio: value(parameters, "ratio", 3.0).max(1.0),
        knee: value(parameters, "knee", 0.0).max(0.0),
        output: value(parameters, "output", 0.0),
        mix: value(parameters, "mix", 1.0).clamp(0.0, 1.0),
        live,
    }
}

impl Values {
    /// The level out of the device, dB, for `level` dB at the detector with `reduction` dB of
    /// gain reduction: the reduced signal blended with the dry one by the mix, then the output
    /// gain, as the device applies them.
    fn out_with(&self, level: f64, reduction: f64) -> f64 {
        let gain = 10f64.powf(-reduction / 20.0);
        let applied = 10f64.powf(self.output / 20.0) * (1.0 + self.mix * (gain - 1.0));
        level + 20.0 * applied.max(1e-10).log10()
    }

    /// The gain reduction the gain computer asks for at `level` dB, zero or positive.
    pub(crate) fn wanted(&self, level: f64) -> f64 {
        -gain_change(level, self.threshold, 1.0 - 1.0 / self.ratio, self.knee)
    }

    /// The transfer curve at `level` dB: the output level once the reduction has settled.
    pub(crate) fn curve(&self, level: f64) -> f64 {
        self.out_with(level, self.wanted(level))
    }
}

/// One reading of the activity history.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Reading {
    time: f64,
    detector: f32,
    output: f32,
    reduction: f32,
}

/// The activity history of one display, kept between frames.
#[derive(Debug, Clone, Default)]
struct History(VecDeque<Reading>);

impl History {
    /// Adds `live` at `time` when a reading is due, and drops what has scrolled out.
    fn record(&mut self, time: f64, live: &Live) {
        // A clock that went back (another window's time) starts the history again, so it stays in
        // order.
        if self.0.back().is_some_and(|last| time < last.time) {
            self.0.clear();
        }
        let due = self.0.back().is_none_or(|last| time - last.time >= 1.0 / HISTORY_RATE - 1e-6);
        if due {
            self.0.push_back(Reading {
                time,
                detector: live.detector as f32,
                output: live.output as f32,
                reduction: live.reduction as f32,
            });
        }
        while self.0.front().is_some_and(|first| time - first.time > HISTORY_SECONDS) {
            self.0.pop_front();
        }
    }
}

/// The share of the level range at `db`, 0 at the bottom and 1 at the top.
fn level_share(db: f64) -> f32 {
    ((db - RANGE.0) / (RANGE.1 - RANGE.0)).clamp(0.0, 1.0) as f32
}

/// The y of a level in `rect`.
fn level_y(db: f64, rect: Rect) -> f32 {
    rect.bottom() - level_share(db) * rect.height()
}

/// The level at a y in `rect`.
fn y_level(y: f32, rect: Rect) -> f64 {
    RANGE.0 + f64::from(((rect.bottom() - y) / rect.height().max(1.0)).clamp(0.0, 1.0)) * (RANGE.1 - RANGE.0)
}

/// The x of a level across `rect`.
fn level_x(db: f64, rect: Rect) -> f32 {
    rect.left() + level_share(db) * rect.width()
}

/// The level at an x across `rect`.
fn x_level(x: f32, rect: Rect) -> f64 {
    RANGE.0 + f64::from(((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0)) * (RANGE.1 - RANGE.0)
}

/// The y of a gain reduction hanging from the top of `rect`.
fn reduction_y(db: f64, rect: Rect) -> f32 {
    rect.top() + (db / REDUCTION_RANGE).clamp(0.0, 1.0) as f32 * rect.height()
}

/// A level as the values row writes it.
fn level_text(db: f64) -> String {
    if db <= RANGE.0 { "-inf".into() } else { fixed(db, 1) }
}

/// Where the display's parts go.
struct Areas {
    /// The input meter, the threshold arrow's column right of it.
    input: Rect,
    arrow: Rect,
    /// The view between the meters.
    view: Rect,
    reduction: Rect,
    output: Rect,
    /// The values row under the meters and the view.
    values: Rect,
}

fn areas(display: Rect, view: View) -> Areas {
    let tabs = Rect::from_min_size(display.min, Vec2::new(display.width(), TABS_HEIGHT));
    let body = Rect::from_min_max(
        Pos2::new(display.left() + 4.0, tabs.bottom() + 4.0),
        Pos2::new(display.right() - 4.0, display.bottom() - VALUES_HEIGHT - 2.0),
    );
    let values =
        Rect::from_min_max(Pos2::new(body.left(), body.bottom() + 1.0), Pos2::new(body.right(), display.bottom()));
    if view == View::Meters {
        // Three large meters side by side in the middle of the body, the input's arrow beside it:
        // as wide as a third of the body leaves room for a scale's numbers (or the arrow) on each
        // side, and only that room apart, so they read as one group.
        let third = body.width() / 3.0;
        let width = (third - 2.0 * METER_SIDE).clamp(12.0, 64.0);
        let gap = METER_SIDE + 6.0;
        let left = body.center().x - (3.0 * width + 2.0 * gap) / 2.0;
        let meter = |i: f32| {
            let x = left + i * (width + gap);
            Rect::from_min_max(Pos2::new(x, body.top()), Pos2::new(x + width, body.bottom()))
        };
        let input = meter(0.0);
        let arrow = Rect::from_min_max(
            Pos2::new(input.right(), body.top()),
            Pos2::new(input.right() + ARROW_WIDTH + 4.0, body.bottom()),
        );
        return Areas { input, arrow, view: Rect::NOTHING, reduction: meter(1.0), output: meter(2.0), values };
    }
    let input = Rect::from_min_size(body.min, Vec2::new(METER_WIDTH, body.height()));
    let arrow = Rect::from_min_size(Pos2::new(input.right(), body.top()), Vec2::new(ARROW_WIDTH, body.height()));
    let output =
        Rect::from_min_size(Pos2::new(body.right() - METER_WIDTH, body.top()), Vec2::new(METER_WIDTH, body.height()));
    let reduction = Rect::from_min_size(
        Pos2::new(output.left() - 4.0 - METER_WIDTH, body.top()),
        Vec2::new(METER_WIDTH, body.height()),
    );
    let view = Rect::from_min_max(
        Pos2::new(arrow.right() + 3.0, body.top()),
        Pos2::new(reduction.left() - 6.0, body.bottom()),
    );
    Areas { input, arrow, view, reduction, output, values }
}

/// Draws the display into `display` with `palette` and edits through `device.setParam` actions
/// addressed to `at`.
pub(crate) fn show(
    ui: &mut egui::Ui,
    display: Rect,
    parameters: &[GraphParameter],
    palette: Palette,
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let id = egui::Id::unique(("compressor-display", track.0, &at.0));
    let values = values(parameters);
    let time = ui.input(|i| i.time);
    let history = ui.ctx().data_mut(|d| {
        let history = d.get_temp_mut_or_default::<History>(id.with("history"));
        if let Some(live) = &values.live {
            history.record(time, live);
        }
        history.clone()
    });
    let view = tabs(ui, display, id, &values, palette);
    let areas = areas(display, view);
    let painter = ui.painter_at(display);
    let colors = super::device_canvas::colors();
    let live = values.live.unwrap_or(Live {
        detector: READOUT_FLOOR_DB,
        output: READOUT_FLOOR_DB,
        reduction: 0.0,
        keyed: false,
    });

    match view {
        View::Curve => draw_curve(&painter, areas.view, &values, &live, palette),
        View::Activity => draw_activity(&painter, areas.view, &values, &history, time, palette),
        View::Meters => {}
    }
    if view == View::Meters {
        meter_scales(&painter, &areas, palette);
    }
    level_meter(&painter, areas.input, live.detector, palette);
    reduction_meter(&painter, areas.reduction, live.reduction, palette);
    level_meter(&painter, areas.output, live.output, palette);

    // The values row: under each meter its value, the input's named after what it hears.
    let font = theme::ui_font(9.0);
    let row = |rect: Rect, text: String, color: Color32| {
        let x = if view == View::Meters {
            rect.center().x
        } else {
            rect.center().x.clamp(display.left() + 14.0, display.right() - 14.0)
        };
        painter.text(Pos2::new(x, areas.values.center().y), Align2::CENTER_CENTER, text, font.clone(), color);
    };
    if view == View::Meters {
        row(
            areas.input,
            format!("{} {}", if live.keyed { "Key" } else { "In" }, level_text(live.detector)),
            colors.text_secondary,
        );
        row(areas.reduction, format!("GR {}", fixed(-live.reduction, 1)), palette.reduction);
        row(areas.output, format!("Out {}", level_text(live.output)), colors.text_secondary);
    } else {
        painter.text(
            Pos2::new(areas.input.left(), areas.values.center().y),
            Align2::LEFT_CENTER,
            format!("{} dB", fixed(values.threshold, 1)),
            font.clone(),
            colors.text_secondary,
        );
        painter.text(
            Pos2::new(areas.output.right(), areas.values.center().y),
            Align2::RIGHT_CENTER,
            format!("{} dB", level_text(live.output)),
            font.clone(),
            colors.text_secondary,
        );
        if view == View::Curve {
            painter.text(
                Pos2::new(areas.view.center().x, areas.values.center().y),
                Align2::CENTER_CENTER,
                format!("{} : 1   knee {} dB", fixed(values.ratio, 1), fixed(values.knee, 1)),
                font.clone(),
                colors.text_secondary,
            );
        } else {
            painter.text(
                Pos2::new(areas.view.center().x, areas.values.center().y),
                Align2::CENTER_CENTER,
                format!("{} s", fixed(HISTORY_SECONDS, 0)),
                font.clone(),
                colors.text_secondary,
            );
        }
    }

    threshold_arrow(ui, &areas, &values, parameters, track, at, actions, id);
    match view {
        View::Curve => drag_ring(ui, areas.view, &values, parameters, (track, at, palette.display), actions, id),
        View::Activity => drag_line(ui, areas.view, &values, parameters, track, at, actions, id),
        View::Meters => {}
    }
}

/// The tab row: the three views at the left, the key's state and the gain reduction at the
/// right. Returns the view that shows.
fn tabs(ui: &mut egui::Ui, display: Rect, id: egui::Id, values: &Values, palette: Palette) -> View {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(display);
    painter.rect_filled(display, 3.0, palette.display);
    let state = id.with("view");
    let mut view = ui.ctx().data(|d| d.get_temp::<View>(state)).unwrap_or_default();
    let font = theme::ui_font(9.5);
    let mut x = display.left() + 3.0;
    for option in View::ALL {
        let galley = painter.layout_no_wrap(option.label().into(), font.clone(), colors.text_primary);
        let rect = Rect::from_min_size(
            Pos2::new(x, display.top() + 3.0),
            Vec2::new(galley.size().x + 12.0, TABS_HEIGHT - 4.0),
        );
        x = rect.right() + 2.0;
        let response = ui.interact(rect, id.with(option.label()), Sense::click()).on_hover_text(option.tooltip());
        let selected = option == view;
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, option.label()));
        if response.clicked() {
            view = option;
            ui.ctx().data_mut(|d| d.insert_temp(state, option));
        }
        let selected = option == view;
        let fill = if selected {
            colors.accent
        } else if response.hovered() {
            colors.control_active
        } else {
            colors.control_background
        };
        painter.rect_filled(rect, 2.0, fill);
        let text = if selected { colors.text_inverse } else { colors.text_primary };
        painter.text(rect.center(), Align2::CENTER_CENTER, option.label(), font.clone(), text);
    }
    // At the right: the key's badge while the detector reads one, and the gain reduction now.
    let font = theme::ui_font(9.0);
    let mut right = display.right() - 4.0;
    if let Some(live) = values.live {
        let galley =
            painter.layout_no_wrap(format!("GR {} dB", fixed(-live.reduction, 1)), font.clone(), palette.reduction);
        let width = galley.size().x;
        painter.galley(
            Pos2::new(right - width, display.top() + (TABS_HEIGHT - galley.size().y) / 2.0 + 1.0),
            galley,
            palette.reduction,
        );
        // The readout's room is as wide as its widest usual value, so the badge beside it stays put
        // while the number changes.
        let room = painter.layout_no_wrap("GR -88.8 dB".to_string(), font.clone(), palette.reduction).size().x;
        right -= width.max(room) + 6.0;
        if live.keyed {
            let rect = Rect::from_min_max(
                Pos2::new(right - 24.0, display.top() + 4.0),
                Pos2::new(right, display.top() + TABS_HEIGHT - 2.0),
            );
            painter.rect_filled(rect, 2.0, palette.level.gamma_multiply(0.85));
            painter.text(rect.center(), Align2::CENTER_CENTER, "SC", font.clone(), palette.display);
            ui.interact(rect, id.with("keyed"), Sense::hover())
                .on_hover_text("The detector hears the side-chain key, not the device's own input");
        }
    }
    view
}

/// A level meter filling from the bottom, with a line at 0 dB.
fn level_meter(painter: &egui::Painter, rect: Rect, db: f64, palette: Palette) {
    let colors = super::device_canvas::colors();
    painter.rect_filled(rect, 1.0, colors.window_background);
    // Lit from its foot as a hardware meter is: the theme's level color up to -12 dB, warming to
    // amber by -3 dB and red past 0 dB. The unlit rest shows the same colors faintly.
    let stops = [
        (rect.bottom(), palette.level),
        (level_y(-12.0, rect), palette.level),
        (level_y(-3.0, rect), METER_AMBER),
        (level_y(0.0, rect), METER_HOT),
        (rect.top(), METER_HOT),
    ];
    gradient(painter, rect, rect.bottom(), rect.top(), &stops, METER_UNLIT);
    let top = level_y(db, rect);
    if top < rect.bottom() {
        gradient(painter, rect, rect.bottom(), top, &stops, METER_LIT);
    }
    segments(painter, rect, colors.window_background);
    let tick = colors.text_secondary.gamma_multiply(0.35);
    for mark in [-12.0, -24.0, -36.0, -48.0] {
        let y = level_y(mark, rect).round() + 0.5;
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, tick));
    }
    let zero = level_y(0.0, rect).round() + 0.5;
    painter.line_segment(
        [Pos2::new(rect.left(), zero), Pos2::new(rect.right(), zero)],
        Stroke::new(1.0, colors.text_secondary.gamma_multiply(0.7)),
    );
}

/// How strongly a meter's lit part shows, and its unlit part.
const METER_LIT: f32 = 0.72;
const METER_UNLIT: f32 = 0.1;
/// A level meter's colors near and past full scale.
const METER_AMBER: Color32 = Color32::from_rgb(0xe8, 0xc0, 0x4f);
const METER_HOT: Color32 = Color32::from_rgb(0xe8, 0x5a, 0x4f);

/// `rect`'s width filled from y `from` to y `to` in the colors `stops` give at their y (from the
/// bottom up), blended between them, at `alpha`.
fn gradient(painter: &egui::Painter, rect: Rect, from: f32, to: f32, stops: &[(f32, Color32)], alpha: f32) {
    let (low, high) = (from.max(to), from.min(to));
    let mut mesh = egui::Mesh::default();
    for pair in stops.windows(2) {
        let [(y0, c0), (y1, c1)] = pair else { continue };
        let (bottom, top) = (y0.max(*y1), y0.min(*y1));
        let (a, b) = (bottom.min(low), top.max(high));
        if a <= b {
            continue;
        }
        let at = |y: f32| {
            let t = if (y1 - y0).abs() > f32::EPSILON { ((y - y0) / (y1 - y0)).clamp(0.0, 1.0) } else { 0.0 };
            blend(*c0, *c1, t).gamma_multiply(alpha)
        };
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(rect.left(), a), at(a));
        mesh.colored_vertex(Pos2::new(rect.right(), a), at(a));
        mesh.colored_vertex(Pos2::new(rect.right(), b), at(b));
        mesh.colored_vertex(Pos2::new(rect.left(), b), at(b));
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(Shape::mesh(mesh));
}

/// The color `t` of the way from `a` to `b`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

/// Thin gaps across a meter every few points, so it reads as a column of segments.
fn segments(painter: &egui::Painter, rect: Rect, gap: Color32) {
    let gap = Stroke::new(1.0, gap.gamma_multiply(0.85));
    let mut y = rect.bottom() - 3.0;
    while y > rect.top() {
        let line = y.round() + 0.5;
        painter.line_segment([Pos2::new(rect.left(), line), Pos2::new(rect.right(), line)], gap);
        y -= 3.0;
    }
}

/// The large meters' scales: dB beside the input and output meters, the reduction's beside its
/// own.
fn meter_scales(painter: &egui::Painter, areas: &Areas, palette: Palette) {
    let colors = super::device_canvas::colors();
    let font = theme::ui_font(8.0);
    for db in [0.0, -12.0, -24.0, -36.0, -48.0] {
        let text = if db == 0.0 { "0".to_string() } else { fixed(db, 0) };
        let y = level_y(db, areas.input);
        painter.text(
            Pos2::new(areas.input.left() - 3.0, y),
            Align2::RIGHT_CENTER,
            &text,
            font.clone(),
            colors.text_secondary,
        );
        painter.text(
            Pos2::new(areas.output.right() + 3.0, y),
            Align2::LEFT_CENTER,
            &text,
            font.clone(),
            colors.text_secondary,
        );
    }
    for db in [6.0, 12.0, 18.0] {
        painter.text(
            Pos2::new(areas.reduction.right() + 3.0, reduction_y(db, areas.reduction)),
            Align2::LEFT_CENTER,
            fixed(-db, 0),
            font.clone(),
            palette.reduction.gamma_multiply(0.8),
        );
    }
}

/// The gain-reduction meter hanging from the top.
fn reduction_meter(painter: &egui::Painter, rect: Rect, db: f64, palette: Palette) {
    let colors = super::device_canvas::colors();
    painter.rect_filled(rect, 1.0, colors.window_background);
    let stops = [(rect.top(), palette.reduction), (rect.bottom(), palette.reduction)];
    gradient(painter, rect, rect.top(), rect.bottom(), &stops, METER_UNLIT);
    let bottom = reduction_y(db, rect);
    if bottom > rect.top() + 0.5 {
        gradient(painter, rect, rect.top(), bottom, &stops, METER_LIT);
    }
    segments(painter, rect, colors.window_background);
    let tick = colors.text_secondary.gamma_multiply(0.35);
    for mark in [6.0, 12.0, 18.0] {
        let y = reduction_y(mark, rect).round() + 0.5;
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, tick));
    }
}

/// The level grid behind a view: a line every 12 dB, 0 dB brighter.
fn level_grid(painter: &egui::Painter, rect: Rect, vertical: bool) {
    let colors = super::device_canvas::colors();
    let mut db = RANGE.0;
    while db <= RANGE.1 {
        let color =
            if db == 0.0 { colors.text_secondary.gamma_multiply(0.5) } else { colors.border.gamma_multiply(0.7) };
        let y = level_y(db, rect);
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, color));
        if vertical {
            let x = level_x(db, rect);
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, color));
        }
        db += 12.0;
    }
}

/// A filled strip between a line of points and the bottom (or top) of `rect`.
fn fill_to(points: &[Pos2], edge: f32, color: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    for point in points {
        mesh.colored_vertex(*point, color);
        mesh.colored_vertex(Pos2::new(point.x, edge), color);
    }
    for index in 1..points.len() as u32 {
        let top = index * 2;
        mesh.add_triangle(top - 2, top - 1, top);
        mesh.add_triangle(top - 1, top, top + 1);
    }
    Shape::mesh(mesh)
}

fn draw_curve(painter: &egui::Painter, rect: Rect, values: &Values, live: &Live, palette: Palette) {
    let colors = super::device_canvas::colors();
    level_grid(painter, rect, true);
    // The knee: a faint band across the levels it bends over.
    if values.knee > 0.0 {
        let band = Rect::from_min_max(
            Pos2::new(level_x(values.threshold - values.knee / 2.0, rect), rect.top()),
            Pos2::new(level_x(values.threshold + values.knee / 2.0, rect), rect.bottom()),
        );
        painter.rect_filled(band, 0.0, colors.accent.gamma_multiply(0.08));
    }
    // Unity: what goes in comes out.
    painter.add(Shape::dashed_line(
        &[Pos2::new(rect.left(), rect.bottom()), Pos2::new(level_x(RANGE.1, rect), level_y(RANGE.1, rect))],
        Stroke::new(1.0, colors.text_secondary.gamma_multiply(0.35)),
        3.0,
        3.0,
    ));
    // The threshold across the whole view.
    let tx = level_x(values.threshold, rect);
    painter.add(Shape::dashed_line(
        &[Pos2::new(tx, rect.top()), Pos2::new(tx, rect.bottom())],
        Stroke::new(1.0, colors.accent.gamma_multiply(0.45)),
        2.0,
        3.0,
    ));
    let points: Vec<Pos2> = (0..=POINTS)
        .map(|i| {
            let level = RANGE.0 + (RANGE.1 - RANGE.0) * i as f64 / POINTS as f64;
            Pos2::new(level_x(level, rect), level_y(values.curve(level), rect))
        })
        .collect();
    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    clipped.add(fill_to(&points, rect.bottom(), colors.accent.gamma_multiply(0.10)));
    clipped.add(Shape::line(points, Stroke::new(1.75, colors.accent)));
    // The level the detector hears now, where the gain reduction puts it.
    if live.detector > RANGE.0 {
        let dot =
            Pos2::new(level_x(live.detector, rect), level_y(values.out_with(live.detector, live.reduction), rect));
        let guide = Stroke::new(1.0, palette.level.gamma_multiply(0.35));
        clipped.line_segment([Pos2::new(dot.x, rect.bottom()), dot], guide);
        clipped.line_segment([Pos2::new(rect.left(), dot.y), dot], guide);
        clipped.circle_filled(dot, 3.5, palette.level);
    }
}

fn draw_activity(painter: &egui::Painter, rect: Rect, values: &Values, history: &History, now: f64, palette: Palette) {
    let colors = super::device_canvas::colors();
    level_grid(painter, rect, false);
    let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let x = |time: f64| rect.right() - ((now - time) / HISTORY_SECONDS).clamp(0.0, 1.0) as f32 * rect.width();
    let readings: Vec<&Reading> = history.0.iter().collect();
    if readings.len() >= 2 {
        let level: Vec<Pos2> =
            readings.iter().map(|r| Pos2::new(x(r.time), level_y(f64::from(r.detector), rect))).collect();
        clipped.add(fill_to(&level, rect.bottom(), palette.level.gamma_multiply(0.30)));
        clipped.add(Shape::line(level, Stroke::new(1.0, palette.level.gamma_multiply(0.8))));
        let output: Vec<Pos2> =
            readings.iter().map(|r| Pos2::new(x(r.time), level_y(f64::from(r.output), rect))).collect();
        clipped.add(Shape::line(output, Stroke::new(1.0, colors.text_primary.gamma_multiply(0.55))));
        let reduction: Vec<Pos2> =
            readings.iter().map(|r| Pos2::new(x(r.time), reduction_y(f64::from(r.reduction), rect))).collect();
        clipped.add(fill_to(&reduction, rect.top(), palette.reduction.gamma_multiply(0.25)));
        clipped.add(Shape::line(reduction, Stroke::new(1.5, palette.reduction)));
    }
    // The reduction's scale down the right edge.
    let font = theme::ui_font(8.0);
    for db in [6.0, 12.0, 18.0] {
        painter.text(
            Pos2::new(rect.right() - 2.0, reduction_y(db, rect)),
            Align2::RIGHT_CENTER,
            format!("-{db}"),
            font.clone(),
            palette.reduction.gamma_multiply(0.7),
        );
    }
    // The threshold across the view, on the level scale.
    let y = level_y(values.threshold, rect);
    painter.add(Shape::dashed_line(
        &[Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(1.0, colors.accent.gamma_multiply(0.8)),
        4.0,
        3.0,
    ));
}

/// The edits, all in one gesture named `gesture`, so a drag is one undo step: the window ends
/// the gesture on mouse up.
fn push_edits(
    actions: &mut Vec<Action>,
    edits: &[(&GraphParameter, f64)],
    track: TrackId,
    at: &DeviceAt,
    gesture: &str,
) {
    for (parameter, value) in edits {
        actions.push(Action::new(
            "device.setParam",
            json!({
                "track": track.0, "device": at.value(), "param": parameter.id, "value": value,
                "gesture": gesture
            }),
        ));
    }
}

/// A drag on a handle, kept while the button is held: the value(s) it moves from and how far
/// it has moved, each frame's movement scaled by [`fine`] then, so pressing or releasing Shift
/// halfway changes the pace from there on and never makes the value jump.
#[derive(Debug, Clone, Copy)]
struct Drag<T> {
    from: T,
    moved: Vec2,
    /// The pointer's distance from the press as last seen, to take each frame's movement from.
    seen: Vec2,
}

/// The drag on `response`, if one is held: where it started and how far it has moved.
fn drag_state<T: Clone + Send + Sync + 'static>(
    ui: &egui::Ui,
    response: &egui::Response,
    state: egui::Id,
    at_press: T,
) -> Option<(T, Vec2)> {
    let held = response.is_pointer_button_down_on();
    let total = response.total_drag_delta().unwrap_or_default();
    let scale = fine(ui);
    let drag = ui.ctx().data_mut(|d| {
        if held && d.get_temp::<Drag<T>>(state).is_none() {
            d.insert_temp(state, Drag { from: at_press, moved: Vec2::ZERO, seen: Vec2::ZERO });
        }
        let mut drag = d.get_temp::<Drag<T>>(state)?;
        drag.moved += (total - drag.seen) * scale;
        drag.seen = total;
        d.insert_temp(state, drag.clone());
        Some(drag)
    });
    if !held && !response.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove::<Drag<T>>(state));
    }
    drag.map(|drag| (drag.from, drag.moved))
}

/// `value` held inside `min ..= max`, whichever way round they are, without the panic
/// `f64::clamp` has for a range given backwards.
fn bounded(value: f64, min: f64, max: f64) -> f64 {
    value.max(min.min(max)).min(max.max(min))
}

/// The range a drag on the curve moves the ratio over: the parameter's own, within the view's
/// 1 : 1 to 20 : 1, or the parameter's alone when the two do not meet.
fn ratio_bounds(min: f64, max: f64) -> (f64, f64) {
    let (low, high) = (min.max(RATIO_RANGE.0), max.min(RATIO_RANGE.1));
    if low <= high { (low, high) } else { (min, max) }
}

/// How far a drag moves for each point: a quarter with Shift.
fn fine(ui: &egui::Ui) -> f32 {
    if ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 }
}

/// The arrow right of the input meter at the threshold: dragged up and down for it.
#[allow(clippy::too_many_arguments)]
fn threshold_arrow(
    ui: &mut egui::Ui,
    areas: &Areas,
    values: &Values,
    parameters: &[GraphParameter],
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
    id: egui::Id,
) {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(areas.arrow.union(areas.input).expand(2.0));
    let y = level_y(values.threshold, areas.input);
    let grab = Rect::from_min_max(
        Pos2::new(areas.input.left(), areas.input.top() - 4.0),
        Pos2::new(areas.arrow.right(), areas.input.bottom() + 4.0),
    );
    let caption = format!("Threshold {} dB", fixed(values.threshold, 1));
    let response = ui
        .interact(grab, id.with("threshold-arrow"), Sense::click_and_drag())
        .on_hover_text(format!("{caption}. Drag up and down; Shift for precision; double-click resets."))
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, &caption));
    let active = response.hovered() || response.dragged();
    let tip = areas.input.right() + 1.0;
    let size = if active { 5.5 } else { 4.5 };
    painter.add(Shape::convex_polygon(
        vec![Pos2::new(tip, y), Pos2::new(tip + size * 1.5, y - size), Pos2::new(tip + size * 1.5, y + size)],
        colors.accent,
        Stroke::NONE,
    ));
    painter.line_segment(
        [Pos2::new(areas.input.left(), y), Pos2::new(areas.input.right(), y)],
        Stroke::new(1.0, colors.accent),
    );
    let Some(threshold) = parameter(parameters, "threshold") else {
        return;
    };
    let drag = drag_state(ui, &response, id.with("threshold-arrow-drag"), y);
    if response.double_clicked() {
        push_edits(actions, &[(threshold, threshold.default)], track, at, "compressor-threshold");
    } else if let Some((start_y, moved)) = drag {
        if response.dragged() {
            let new = bounded(y_level(start_y + moved.y, areas.input), threshold.min, threshold.max);
            if (new - threshold.value).abs() > 1e-9 {
                push_edits(actions, &[(threshold, new)], track, at, "compressor-threshold");
            }
        } else if response.drag_stopped() {
            push_edits(actions, &[(threshold, threshold.value)], track, at, "compressor-threshold");
        }
    }
}

/// The ring on the curve at the threshold: dragged sideways for the threshold and up and down
/// for the ratio (down compresses more); double-click resets both.
#[allow(clippy::too_many_arguments)]
fn drag_ring(
    ui: &mut egui::Ui,
    rect: Rect,
    values: &Values,
    parameters: &[GraphParameter],
    (track, at, hole): (TrackId, &DeviceAt, Color32),
    actions: &mut Vec<Action>,
    id: egui::Id,
) {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(rect.expand(8.0));
    let position = Pos2::new(level_x(values.threshold, rect), level_y(values.curve(values.threshold), rect));
    let caption = format!("Threshold {} dB, ratio {} : 1", fixed(values.threshold, 1), fixed(values.ratio, 1));
    let response = ui
        .interact(Rect::from_center_size(position, Vec2::splat(20.0)), id.with("ring"), Sense::click_and_drag())
        .on_hover_text(format!(
            "{caption}. Drag sideways for the threshold, up and down for the ratio; Shift for precision; \
             double-click resets both."
        ))
        .on_hover_cursor(egui::CursorIcon::Move);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, &caption));
    let active = response.hovered() || response.dragged();
    let radius = if active { 6.5 } else { 5.5 };
    // The inside is the display's ground, so the curve does not run through the ring.
    painter.circle_filled(position, radius, hole);
    painter.circle_stroke(position, radius, Stroke::new(2.0, colors.accent));

    let (Some(threshold), Some(ratio)) = (parameter(parameters, "threshold"), parameter(parameters, "ratio")) else {
        return;
    };
    let drag = drag_state(ui, &response, id.with("ring-drag"), (position.x, values.ratio));
    if response.double_clicked() {
        push_edits(actions, &[(threshold, threshold.default), (ratio, ratio.default)], track, at, "compressor-ring");
    } else if let Some(((start_x, start_ratio), total)) = drag {
        if response.dragged() {
            let new_threshold = bounded(x_level(start_x + total.x, rect), threshold.min, threshold.max);
            // The ratio moves on its logarithmic scale: the view's height spans its range.
            let span = (RATIO_RANGE.1 / RATIO_RANGE.0).ln();
            let (low, high) = ratio_bounds(ratio.min, ratio.max);
            let new_ratio = bounded(
                (start_ratio.max(RATIO_RANGE.0).ln() + f64::from(total.y / rect.height().max(1.0)) * span).exp(),
                low,
                high,
            );
            let mut edits = Vec::new();
            if (new_threshold - threshold.value).abs() > 1e-9 {
                edits.push((threshold, new_threshold));
            }
            if (new_ratio - ratio.value).abs() > 1e-9 {
                edits.push((ratio, new_ratio));
            }
            push_edits(actions, &edits, track, at, "compressor-ring");
        } else if response.drag_stopped() {
            push_edits(actions, &[(threshold, threshold.value), (ratio, ratio.value)], track, at, "compressor-ring");
        }
    }
}

/// The activity's threshold line: grabbed near it and dragged up and down.
#[allow(clippy::too_many_arguments)]
fn drag_line(
    ui: &mut egui::Ui,
    rect: Rect,
    values: &Values,
    parameters: &[GraphParameter],
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
    id: egui::Id,
) {
    let y = level_y(values.threshold, rect);
    let grab = Rect::from_min_max(Pos2::new(rect.left(), y - LINE_GRAB), Pos2::new(rect.right(), y + LINE_GRAB));
    let caption = format!("Threshold {} dB", fixed(values.threshold, 1));
    let response = ui
        .interact(grab, id.with("line"), Sense::click_and_drag())
        .on_hover_text(format!("{caption}. Drag up and down; Shift for precision; double-click resets."))
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, &caption));
    if response.hovered() || response.dragged() {
        let colors = super::device_canvas::colors();
        ui.painter_at(rect)
            .line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(2.0, colors.accent));
    }
    let Some(threshold) = parameter(parameters, "threshold") else {
        return;
    };
    let drag = drag_state(ui, &response, id.with("line-drag"), y);
    if response.double_clicked() {
        push_edits(actions, &[(threshold, threshold.default)], track, at, "compressor-threshold");
    } else if let Some((start_y, moved)) = drag {
        if response.dragged() {
            let new = bounded(y_level(start_y + moved.y, rect), threshold.min, threshold.max);
            if (new - threshold.value).abs() > 1e-9 {
                push_edits(actions, &[(threshold, new)], track, at, "compressor-threshold");
            }
        } else if response.drag_stopped() {
            push_edits(actions, &[(threshold, threshold.value)], track, at, "compressor-threshold");
        }
    }
}

#[cfg(test)]
#[path = "compressor_display_tests.rs"]
mod tests;
