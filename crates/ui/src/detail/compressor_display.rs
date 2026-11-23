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
/// The widest a Meters view column grows, and the strip above its meter that holds its value.
const METER_COLUMN: f32 = 110.0;
const READOUT_HEIGHT: f32 = 30.0;
/// The room the Meters view keeps on each side of a meter for its scale's numbers.
const METER_ROOM: f32 = 18.0;
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
        // Three meters in columns across the middle of the body, each under its value read large,
        // the input's arrow beside it; a column is a third of the body, at most a meter and its
        // scale's room wide, so the group keeps together on a wide panel.
        let column = (body.width() / 3.0).min(METER_COLUMN);
        let width = (column - 2.0 * METER_ROOM).clamp(12.0, 64.0);
        let top = body.top() + READOUT_HEIGHT;
        let meter = |i: f32| {
            let center = body.center().x + (i - 1.0) * column;
            Rect::from_min_max(Pos2::new(center - width / 2.0, top), Pos2::new(center + width / 2.0, body.bottom()))
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
    // The meters move smoothly: they rise quickly and settle slowly, as meters with ballistics do.
    let live = Live {
        detector: smoothed(ui.ctx(), areas.input, live.detector, RANGE.0),
        output: smoothed(ui.ctx(), areas.output, live.output, RANGE.0),
        reduction: smoothed(ui.ctx(), areas.reduction, live.reduction, 0.0),
        keyed: live.keyed,
    };
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
        // Each meter's value read large above it, its name under it.
        let big = theme::ui_font(13.0);
        let small = theme::ui_font(9.0);
        let readout = |rect: Rect, text: String, color: Color32| {
            painter.text(Pos2::new(rect.center().x, rect.top() - 9.0), Align2::CENTER_CENTER, text, big.clone(), color);
        };
        // Over each level meter its peak, held as its peak line is: red once past 0 dB.
        let peak = |rect: Rect, db: f64| {
            let held = peak_hold(ui.ctx(), rect, db);
            let color = if held > 0.0 { METER_HOT } else { colors.text_secondary };
            painter.text(
                Pos2::new(rect.center().x, rect.top() - READOUT_HEIGHT + 6.0),
                Align2::CENTER_CENTER,
                format!("peak {}", level_text(held)),
                small.clone(),
                color,
            );
        };
        peak(areas.input, live.detector);
        peak(areas.output, live.output);
        readout(areas.input, level_text(live.detector), colors.text_primary);
        readout(areas.reduction, fixed(-live.reduction, 1), METER_REDUCTION);
        readout(areas.output, level_text(live.output), colors.text_primary);
        row(areas.input, (if live.keyed { "Sidechain" } else { "In" }).to_string(), colors.text_secondary);
        row(areas.reduction, "GR".to_string(), colors.text_secondary);
        row(areas.output, "Out".to_string(), colors.text_secondary);
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
    glass(&ui.painter_at(display), display);
}

/// The display's glass, over all it shows: a soft light from the top fading out by its middle, a
/// bright hairline along the top edge and a dark one round the rest, so it reads as a pane set
/// into the panel. Faint enough that what is under it reads as before.
fn glass(painter: &egui::Painter, display: Rect) {
    let mut mesh = egui::Mesh::default();
    let fade = display.top() + display.height() * 0.45;
    let (top, clear) = (Color32::from_white_alpha(14), Color32::TRANSPARENT);
    mesh.colored_vertex(display.left_top(), top);
    mesh.colored_vertex(display.right_top(), top);
    mesh.colored_vertex(Pos2::new(display.right(), fade), clear);
    mesh.colored_vertex(Pos2::new(display.left(), fade), clear);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
    painter.rect_stroke(display, 3.0, Stroke::new(1.0, Color32::from_black_alpha(90)), egui::StrokeKind::Inside);
    let y = display.top() + 1.0;
    painter.line_segment(
        [Pos2::new(display.left() + 3.0, y), Pos2::new(display.right() - 3.0, y)],
        Stroke::new(1.0, Color32::from_white_alpha(28)),
    );
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

/// A level meter filling from the bottom in three zones: the meter green up to -6 dB, amber to
/// full scale and red past it, solid, in a rounded groove that shows the zones faintly. A line
/// marks the highest level lately, and a cap at the top lights red while it is past 0 dB.
fn level_meter(painter: &egui::Painter, rect: Rect, db: f64, _palette: Palette) {
    let colors = super::device_canvas::colors();
    let ground = colors.window_background;
    track(painter, rect, ground);
    let zones = [(RANGE.0, -6.0, METER_GREEN), (-6.0, 0.0, METER_AMBER), (0.0, RANGE.1, METER_HOT)];
    for (i, (from, to, color)) in zones.into_iter().enumerate() {
        let (low, high) = (level_y(from, rect), level_y(to, rect));
        let foot = i == 0;
        band(painter, rect, low, high, blend(ground, color, METER_UNLIT), foot, i == zones.len() - 1);
        if db > from {
            band(painter, rect, low, level_y(db.min(to), rect), blend(ground, color, METER_FILL), foot, false);
        }
    }
    let held = peak_hold(painter.ctx(), rect, db);
    if held > RANGE.0 {
        let y = level_y(held, rect).round();
        let color = zones.iter().find(|(from, to, _)| held >= *from && held < *to).map_or(METER_HOT, |z| z.2);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(rect.left(), y - 1.0), Pos2::new(rect.right(), y + 1.0)),
            0.0,
            blend(color, Color32::WHITE, 0.3),
        );
    }
    let tick = colors.text_secondary.gamma_multiply(0.25);
    for mark in [-12.0, -24.0, -36.0, -48.0] {
        let y = level_y(mark, rect).round() + 0.5;
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, tick));
    }
}

/// The meters' own colors: green for level, then amber and red near and past full scale, and the
/// orange of gain reduction. How strongly a meter's lit part shows over the groove, and its unlit
/// zones.
const METER_GREEN: Color32 = Color32::from_rgb(0x33, 0xc1, 0x6a);
const METER_AMBER: Color32 = Color32::from_rgb(0xe8, 0xc0, 0x4f);
const METER_HOT: Color32 = Color32::from_rgb(0xe8, 0x5a, 0x4f);
const METER_REDUCTION: Color32 = Color32::from_rgb(0xea, 0x9c, 0x48);
const METER_FILL: f32 = 0.95;
const METER_UNLIT: f32 = 0.06;
/// The corner radius of a meter's groove, and how much darker than the display it is.
const METER_ROUND: f32 = 4.0;
const METER_GROOVE: f32 = 0.4;
/// How long a meter's peak line holds, in seconds, and how fast it falls after, in dB a second.
const PEAK_HOLD: f64 = 0.8;
const PEAK_FALL: f64 = 12.0;
/// How fast a meter follows its level: the time constants (seconds) of its rise and of its fall.
const METER_RISE: f64 = 0.012;
const METER_SETTLE: f64 = 0.18;

/// `target` as the meter at `rect` shows it: moving toward it by the frame's time, quickly when
/// it rises and slowly when it falls, from `floor` the first time. Drawn again until it settles.
fn smoothed(ctx: &egui::Context, rect: Rect, target: f64, floor: f64) -> f64 {
    let now = ctx.input(|i| i.time);
    let id = egui::Id::unique(("compressor-meter-shown", rect.left().round() as i32, rect.top().round() as i32));
    let shown = ctx.data_mut(|d| {
        let (value, at) = d.get_temp::<(f64, f64)>(id).unwrap_or((floor, now));
        let dt = (now - at).clamp(0.0, 0.25);
        let tau = if target > value { METER_RISE } else { METER_SETTLE };
        let next = value + (target - value) * (1.0 - (-dt / tau).exp());
        let next = if next.is_finite() { next } else { target };
        d.insert_temp(id, (next, now));
        next
    });
    if (shown - target).abs() > 0.05 {
        ctx.request_repaint();
    }
    shown
}

/// The level a meter's peak line shows: the highest `db` seen in the last [`PEAK_HOLD`] seconds,
/// then falling [`PEAK_FALL`] dB a second. Kept by the meter's place on screen; the window is drawn
/// again while it falls.
fn peak_hold(ctx: &egui::Context, rect: Rect, db: f64) -> f64 {
    let now = ctx.input(|i| i.time);
    let id = egui::Id::unique(("compressor-meter-peak", rect.left().round() as i32, rect.top().round() as i32));
    let held = ctx.data_mut(|d| {
        let (peak, at) = d.get_temp::<(f64, f64)>(id).unwrap_or((RANGE.0, now));
        let since = now - at;
        let fallen = if since > PEAK_HOLD { peak - (since - PEAK_HOLD) * PEAK_FALL } else { peak };
        if db >= fallen {
            d.insert_temp(id, (db, now));
            db
        } else {
            fallen
        }
    });
    if held > db && held > RANGE.0 {
        ctx.request_repaint();
    }
    held
}

/// A meter's groove: darker than the display, with rounded corners.
fn track(painter: &egui::Painter, rect: Rect, ground: Color32) {
    painter.rect_filled(rect, METER_ROUND, blend(ground, Color32::BLACK, METER_GROOVE));
}

/// `rect`'s width from y `low` up to y `high` in `color`, its foot rounded as the groove's when
/// `foot`, its top when `top`.
fn band(painter: &egui::Painter, rect: Rect, low: f32, high: f32, color: Color32, foot: bool, top: bool) {
    if low - high < 0.5 {
        return;
    }
    let round = METER_ROUND as u8;
    let corners = egui::CornerRadius {
        nw: if top { round } else { 0 },
        ne: if top { round } else { 0 },
        sw: if foot { round } else { 0 },
        se: if foot { round } else { 0 },
    };
    painter.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), high), Pos2::new(rect.right(), low)), corners, color);
}

/// The color `t` of the way from `a` to `b`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
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

/// The gain-reduction meter hanging from the top, in a groove as the level meters are.
fn reduction_meter(painter: &egui::Painter, rect: Rect, db: f64, _palette: Palette) {
    let colors = super::device_canvas::colors();
    let ground = colors.window_background;
    track(painter, rect, ground);
    band(painter, rect, rect.bottom(), rect.top(), blend(ground, METER_REDUCTION, METER_UNLIT), true, true);
    let bottom = reduction_y(db, rect);
    band(painter, rect, bottom, rect.top(), blend(ground, METER_REDUCTION, METER_FILL), false, true);
    let tick = colors.text_secondary.gamma_multiply(0.25);
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
    // A light line with a dark edge, so it reads over the meter's fill whatever level it shows,
    // and a handle beside the meter in the same light, brighter while it is held or pointed at.
    let light = if active { Color32::WHITE } else { colors.text_primary };
    let edge = Color32::from_black_alpha(160);
    let (left, right) = (areas.input.left() - 2.0, areas.input.right() + 2.0);
    painter.line_segment([Pos2::new(left, y), Pos2::new(right, y)], Stroke::new(3.5, edge));
    painter.line_segment([Pos2::new(left, y), Pos2::new(right, y)], Stroke::new(1.5, light));
    let tip = areas.input.right() + 2.0;
    let size = if active { 6.5 } else { 5.5 };
    painter.add(Shape::convex_polygon(
        vec![Pos2::new(tip, y), Pos2::new(tip + size * 1.5, y - size), Pos2::new(tip + size * 1.5, y + size)],
        light,
        Stroke::new(1.0, edge),
    ));
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
