// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The EQ's response display (the SDK's `Widget::EqCurve`, docs/spec/eq.md › The panel), drawn
//! on the UI thread from the device's own filter maths: the curve is the summed magnitude
//! response of the on bands of the curve in use, every band is a numbered dot at its frequency
//! and gain, the selected band's Frequency, Gain and Q are strips beside the display, the
//! bands' type menus and numbered selectors run along the bottom, and the Edit switch picks
//! which curve the display edits in Left/Right and Mid/Side. While sound plays through the
//! device, the analyser also draws the spectrum of its output as a filled grey area behind the
//! curve, on the display's own vertical scale, from a ring the engine's audio thread keeps in
//! the meter monitor. Which band is selected and which curve is edited are panel state; every
//! edit is a `device.setParam` command, addressed by the device's full path, so a panel works
//! the same on a device inside a group as on one of the track's own.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use serde_json::json;
use std::sync::Arc;
use suhina_device_api::DEFAULT_SAMPLE_RATE;
use suhina_device_eq::{BANDS, TYPE_NAMES, band_sections};
use suhina_dsp::{AttackRelease, SvfCoefficients};
use suhina_engine::MeterMonitor;
use suhina_engine::model::TrackId;
use suhina_engine::spectrum::{self, RING};

use crate::detail::detail_view::{DeviceAt, GraphParameter};
use crate::kit::theme;
use crate::kit::widgets::{Action, fixed};

/// The display's rectangle in the panel, `[x, y, width, height]`: the strips' column at its
/// left, the globals' column (`devices/eq/src/panel.rs`) at its right, the per-band rows under
/// it.
const DISPLAY: [f32; 4] = [58.0, 2.0, 288.0, 98.0];
/// The per-band rows under the display: the type menus, then the numbered selectors. Eight
/// bands, one column each, the display's width split between them.
const MENU_ROW: f32 = 104.0;
const NUMBER_ROW: f32 = 132.0;
const ROW_HEIGHT: f32 = 26.0;
const BAND_WIDTH: f32 = DISPLAY[2] / BANDS as f32;
/// The strips' column: `[x, y of the first group, width, a group's height]`; a group is its
/// caption, its track and its value.
const STRIPS: [f32; 4] = [2.0, 4.0, 52.0, 54.0];
/// The Edit switch's rectangle in the globals' column, under the four global controls
/// (`devices/eq/src/panel.rs`).
const EDIT: [f32; 4] = [356.0, 135.0, 82.0, 20.0];
/// The display's height in dB: the gain parameter's full range, top to bottom.
const DB: f64 = 15.0;
/// The frequency axis: the frequency parameter's full range, left to right.
const FREQUENCY: (f64, f64) = (10.0, 22_000.0);
/// The curve's points across the display.
const POINTS: usize = 192;
/// The rate the response is drawn at; the device runs at the project's rate, and only the bend
/// near this Nyquist differs (docs/spec/eq.md › The panel › Decisions).
const DRAWN_RATE: f64 = DEFAULT_SAMPLE_RATE;
/// The types whose gain shapes the response; the cuts and the notch have no gain.
const GAIN_TYPES: [usize; 3] = [2, 3, 5];
/// How many bands the analyser folds the spectrum into over the display's frequency range.
const ANALYSER_BANDS: usize = 64;
/// The analyser's own vertical scale across the display's height: this level in dB at the top
/// edge, this at the bottom. The line is not read against the gain grid — normal material
/// sits far below a ±15 dB grid, so on the grid the line would fall below the display and
/// never show (docs/spec/eq.md › The panel › The analyser).
const ANALYSER_DB: (f64, f64) = (0.0, -60.0);
/// The strength of the analyser's filled area's grey, of the theme's secondary grey at its
/// full value: enough to read the shape at a glance, dim enough to sit behind the curve and
/// the grid's labels.
const ANALYSER_FILL: f32 = 0.45;
/// How long a band's shown level takes to rise to a louder reading, in seconds: quick enough
/// that a kick still punches the area up.
const ANALYSER_ATTACK: f64 = 0.05;
/// How long a band's shown level takes to fall after a quieter reading, in seconds: the fresh
/// transform of each 43 ms window flickers window to window, especially on percussive
/// material, and a slow fall is what turns that flicker into a flowing shape.
const ANALYSER_RELEASE: f64 = 0.35;

/// The analyser's filled area's grey: the theme's secondary grey scaled to [`ANALYSER_FILL`] of
/// its value, opaque, so the area reads as a solid shape over the grid behind it
/// (`gamma_multiply` would thin it to a veil the grid shows through).
fn analyser_grey(colors: &crate::kit::theme::Palette) -> Color32 {
    let [r, g, b, _] = colors.text_secondary.to_array();
    Color32::from_rgb(
        (r as f32 * ANALYSER_FILL).round() as u8,
        (g as f32 * ANALYSER_FILL).round() as u8,
        (b as f32 * ANALYSER_FILL).round() as u8,
    )
}

/// One band of one curve, as the display reads it: its values and the filter sections the
/// device's own maths ([`band_sections`]) runs for them.
#[derive(Clone, Copy)]
struct Band {
    on: bool,
    kind: usize,
    frequency: f64,
    gain: f64,
    q: f64,
    sections: [SvfCoefficients; 4],
    count: usize,
}
impl Band {
    /// Whether the band's type has a gain to drag: the cuts and the notch have none.
    fn gain_editable(&self) -> bool {
        GAIN_TYPES.contains(&self.kind)
    }
}

/// How the panel addresses its device: `target` is the `device` parameter of every
/// `device.setParam` command it writes — the index of the device on the track, or its path
/// through device groups (`DeviceAt::value`), so a grouped EQ's panel edits the grouped EQ
/// and not whatever its branch-local index would name on the track. `ids` keys the panel's
/// widget and state ids to the full path, so a grouped EQ and a top-level one never share
/// selection state or widget identity.
struct Address {
    target: serde_json::Value,
    ids: egui::Id,
}

impl Address {
    /// The address of the device at `at`: its command target and its id key.
    fn new(track: TrackId, at: &DeviceAt) -> Self {
        Self { target: at.value(), ids: egui::Id::new(("eq-curve", track.0, &at.0)) }
    }
}

/// Draws the panel into `bounds` (the widget's rectangle in the host): the display, the dots,
/// the strips, the rows and the switch, editing through `device.setParam` actions addressed
/// to `at` (see [`Address`]) while the analyser claims its watch through the frame's meter
/// monitor.
pub(crate) fn show(
    ui: &mut egui::Ui,
    bounds: Rect,
    parameters: &[GraphParameter],
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let address = Address::new(track, at);
    let display =
        Rect::from_min_size(bounds.min + Vec2::new(DISPLAY[0], DISPLAY[1]), Vec2::new(DISPLAY[2], DISPLAY[3]));
    let mode = value(parameters, "mode").round().min(2.0);
    let selection_id = address.ids.with("selection");
    let mut selection = ui.ctx().data(|d| d.get_temp::<(usize, usize)>(selection_id)).unwrap_or((0, 0));
    if mode == 0.0 {
        selection.0 = 0;
    }
    selection.0 = selection.0.min(usize::from(mode > 0.0));
    selection.1 = selection.1.min(BANDS - 1);

    let curve_a = bands(parameters, 0);
    let curve_b = (mode > 0.0).then(|| bands(parameters, 1));
    let edited: &[Band] = match selection.0 {
        1 if curve_b.is_some() => curve_b.as_ref().unwrap_or(&curve_a),
        _ => &curve_a,
    };
    let analyser = watch_key(at).and_then(|device| analyser_bands(ui, track, device));

    draw_display(ui, display, &curve_a, curve_b.as_deref().unwrap_or(&curve_a), mode > 0.0, analyser.as_ref());
    draw_dots(ui, display, edited, &mut selection, parameters, track, &address, actions);
    for field in 0..3 {
        draw_strip(ui, bounds, field, selection, parameters, track, &address, actions);
    }
    draw_type_menus(ui, bounds, edited, selection, parameters, track, &address, actions);
    draw_numbers(ui, bounds, edited, &mut selection, parameters, track, &address, actions);
    if mode > 0.0 {
        draw_edit_switch(ui, bounds, &mut selection, &address);
    }
    ui.ctx().data_mut(|d| d.insert_temp(selection_id, selection));
}

/// The parameter's value, or 0 when the panel has no such id.
fn value(parameters: &[GraphParameter], id: &str) -> f64 {
    parameters.iter().find(|p| p.id == id).map_or(0.0, |p| p.value)
}

/// The parameter of a curve's band named by its field, e.g. `a3Gain`.
fn parameter_by_field<'a>(
    parameters: &'a [GraphParameter],
    curve: usize,
    band: usize,
    field: &str,
) -> Option<&'a GraphParameter> {
    let prefix = ['a', 'b'][curve.min(1)];
    parameters.iter().find(|p| p.id == format!("{prefix}{}{field}", band + 1))
}

/// A curve's bands, on and off alike, from the panel's parameters.
fn bands(parameters: &[GraphParameter], curve: usize) -> Vec<Band> {
    let prefix = ['a', 'b'][curve.min(1)];
    let scale = value(parameters, "scale");
    let adaptive = value(parameters, "adaptiveQ") >= 0.5;
    (1..=BANDS)
        .map(|n| {
            let named = |field: &str| value(parameters, &format!("{prefix}{n}{field}"));
            let kind = named("Type").round().min((TYPE_NAMES.len() - 1) as f64) as usize;
            let frequency = named("Frequency").clamp(FREQUENCY.0, FREQUENCY.1);
            let gain = named("Gain").clamp(-DB, DB);
            let q = named("Q");
            let (sections, count) = band_sections(kind, DRAWN_RATE, frequency, gain, q, scale, adaptive);
            Band { on: named("On") >= 0.5, kind, frequency, gain, q, sections, count }
        })
        .collect()
}

/// The x of a frequency on the display's logarithmic axis.
fn frequency_x(frequency: f64, display: Rect) -> f32 {
    display.left()
        + ((frequency / FREQUENCY.0).ln() / (FREQUENCY.1 / FREQUENCY.0).ln()).clamp(0.0, 1.0) as f32 * display.width()
}

/// The frequency at an x on the display's axis.
fn x_frequency(x: f32, display: Rect) -> f64 {
    FREQUENCY.0
        * ((((x - display.left()) / display.width()).clamp(0.0, 1.0) as f64) * (FREQUENCY.1 / FREQUENCY.0).ln()).exp()
}

/// The y of a level in dB: −`DB` at the bottom, +`DB` at the top, 0 in the middle.
fn db_y(db: f64, display: Rect) -> f32 {
    display.bottom() - ((db / DB) + 1.0) as f32 / 2.0 * display.height()
}

/// The level in dB at a y, clamped to the display's range.
fn y_db(y: f32, display: Rect) -> f64 {
    ((display.bottom() - y) / display.height()).clamp(0.0, 1.0) as f64 * 2.0 * DB - DB
}

/// The y of the analyser's level in dB, on its own scale across the display's height: full
/// scale at the top edge, [`ANALYSER_DB`]'s floor at its bottom, clamped to the display.
fn analyser_y(db: f64, display: Rect) -> f32 {
    let down = ((ANALYSER_DB.0 - db) / (ANALYSER_DB.0 - ANALYSER_DB.1)).clamp(0.0, 1.0);
    display.top() + down as f32 * display.height()
}

/// The analyser's watch key for a device's path: a top-level device's index, or `None` for a
/// device inside a group — the chain pass taps no grouped device, and its branch-local index
/// would name a top-level device's row and show that device's spectrum (#1050 keys watches
/// by path; docs/spec/eq.md › The panel › The analyser).
fn watch_key(at: &DeviceAt) -> Option<usize> {
    (at.0.len() == 1).then(|| at.0[0])
}

/// The summed response of the on bands, in points across the display.
fn curve_points(display: Rect, curve: &[Band]) -> Vec<Pos2> {
    (0..=POINTS)
        .map(|i| {
            let frequency = FREQUENCY.0 * ((i as f64 / POINTS as f64) * (FREQUENCY.1 / FREQUENCY.0).ln()).exp();
            let mut total = 1.0;
            for band in curve.iter().filter(|band| band.on) {
                for coefficients in &band.sections[..band.count] {
                    total *= coefficients.magnitude(DRAWN_RATE, frequency);
                }
            }
            Pos2::new(frequency_x(frequency, display), db_y(20.0 * total.max(1.0e-6).log10(), display))
        })
        .collect()
}

/// The edited values as `device.setParam` commands, all but the last one continuous so a drag
/// is one undo step. `device` is the panel's address's `target` — the index on the track, or
/// the path through groups.
fn push_edits(
    actions: &mut Vec<Action>,
    edits: Vec<(&GraphParameter, f64)>,
    track: TrackId,
    device: &serde_json::Value,
    gesture: &str,
    dragged: bool,
) {
    let count = edits.len();
    for (index, (parameter, value)) in edits.into_iter().enumerate() {
        actions.push(Action::new(
            "device.setParam",
            json!({
                "track": track.0, "device": device, "param": parameter.id, "value": value,
                "continuous": index + 1 < count || dragged, "gesture": gesture
            }),
        ));
    }
}

/// A frequency as the left column reads it: one decimal, Hz below a kilohertz and kHz above.
fn frequency_text(frequency: f64) -> String {
    if frequency >= 1000.0 {
        format!("{} kHz", fixed(frequency / 1000.0, 1))
    } else {
        format!("{} Hz", fixed(frequency, 1))
    }
}

/// The analyser's bands of this panel's device, or `None` while no meter monitor is in the
/// frame's data or nothing has played through the device yet. The panel claims its watch row
/// here, so the engine starts appending as soon as the panel shows, and asks for the next
/// frame while the ring's appended count moves: the meters' repaints alone hold steady with a
/// steady level, and the line would freeze with them (monitoring with the transport stopped).
/// The bands it returns are the shown levels, gliding towards each frame's fresh reading with
/// attack/release ballistics, so the area flows with the music instead of flickering.
fn analyser_bands(ui: &mut egui::Ui, track: TrackId, device: usize) -> Option<[f32; ANALYSER_BANDS]> {
    let monitor = ui.ctx().data(|d| d.get_temp::<Arc<MeterMonitor>>(egui::Id::new("eq-spectrum")))?;
    let spectrum = monitor.spectrum();
    spectrum.watch(device % spectrum::ROWS, track, device);
    let (samples, _) = spectrum.samples(track, device)?;
    // While the engine renders the device, the count moves and the panel comes back for the
    // next window; when the engine stops, the count stops and the line holds the last
    // spectrum.
    let cursor = spectrum.appended(track, device).unwrap_or_default();
    let cursor_id = egui::Id::new(("eq-spectrum-cursor", track.0, device));
    if cursor != ui.ctx().data(|d| d.get_temp::<usize>(cursor_id)).unwrap_or(0) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    ui.ctx().data_mut(|d| d.insert_temp(cursor_id, cursor));
    // The window and the transform's plan are the same for every panel and every frame; the
    // scratch is built anew, on the UI thread, where allocating is free.
    let cache = egui::Id::new("eq-spectrum-analyser");
    let (window, plan) = ui.ctx().data_mut(|d| {
        if d.get_temp::<(Arc<[f32; RING]>, Arc<dyn rustfft::Fft<f32>>)>(cache).is_none() {
            let plan: Arc<dyn rustfft::Fft<f32>> = rustfft::FftPlanner::<f32>::new().plan_fft_forward(RING);
            d.insert_temp(cache, (Arc::new(hann()), plan));
        }
        d.get_temp::<(Arc<[f32; RING]>, Arc<dyn rustfft::Fft<f32>>)>(cache)
    })?;
    let mut bins: Vec<rustfft::num_complex::Complex<f32>> = samples
        .iter()
        .zip(window.iter())
        .map(|(&sample, &weight)| rustfft::num_complex::Complex::new(sample * weight, 0.0))
        .collect();
    plan.process(&mut bins);
    let sum = window.iter().sum::<f32>().max(1.0);
    let bands = fold(&bins, sum, spectrum.sample_rate());
    // The shown levels glide towards the fresh reading, one band at a time: up with the
    // attack, down with the release — the DSP crate's follower, the compressor's own block,
    // driven once a frame at the frame's own step. Without it each window's transform redraws
    // the area from nothing every 30 ms and percussive material reads as a flicker.
    let shown_id = egui::Id::new(("eq-spectrum-shown", track.0, device));
    let now = ui.input(|i| i.time);
    let stored = ui.ctx().data(|d| d.get_temp::<([f32; ANALYSER_BANDS], f64)>(shown_id));
    let last = stored.map_or(now, |(_, last)| last);
    let mut shown = stored.map_or(bands, |(shown, _)| shown);
    let frame = 1.0 / (now - last).clamp(1.0e-3, 1.0);
    let follower = AttackRelease::new(ANALYSER_ATTACK, ANALYSER_RELEASE, frame);
    for (shown, target) in shown.iter_mut().zip(bands.iter()) {
        *shown = follower.rise(f64::from(*shown), f64::from(*target)).clamp(-90.0, 30.0) as f32;
    }
    ui.ctx().data_mut(|d| d.insert_temp(shown_id, (shown, now)));
    Some(shown)
}

/// The analyser's Hann window over the ring's length.
fn hann() -> [f32; RING] {
    let mut window = [0.0f32; RING];
    for (n, weight) in window.iter_mut().enumerate() {
        *weight = (0.5 - 0.5 * (std::f64::consts::PI * n as f64 / RING as f64).cos()) as f32;
    }
    window
}

/// The transform's bins folded into the analyser's bands: each band the loudest bin in it, as
/// a sine's amplitude against full scale in dB. `window_sum` is the Hann window's sum, `rate`
/// the rate the ring was filled at.
fn fold(bins: &[rustfft::num_complex::Complex<f32>], window_sum: f32, rate: f64) -> [f32; ANALYSER_BANDS] {
    let mut bands = [0.0; ANALYSER_BANDS];
    for (band, bands_out) in bands.iter_mut().enumerate() {
        let lo = FREQUENCY.0 * (FREQUENCY.1 / FREQUENCY.0).powf(band as f64 / ANALYSER_BANDS as f64);
        let hi = FREQUENCY.0 * (FREQUENCY.1 / FREQUENCY.0).powf((band + 1) as f64 / ANALYSER_BANDS as f64);
        let first = (lo * RING as f64 / rate).ceil().clamp(1.0, (RING / 2 - 1) as f64) as usize;
        let last = (hi * RING as f64 / rate).ceil().clamp(first as f64, (RING / 2) as f64) as usize;
        let mut peak = 0.0f32;
        for binned in first..=last {
            if let Some(cell) = bins.get(binned % RING) {
                peak = peak.max(cell.norm());
            }
        }
        let db = 20.0 * f64::from(2.0 * peak / window_sum).max(1e-9).log10();
        *bands_out = (db as f32).clamp(-90.0, 30.0);
    }
    bands
}

/// The display: its background, its dB and frequency grids with their labels, the spectrum the
/// analyser read, and the curve of each curve group in use — curve A, and curve B beside it in
/// Left/Right and Mid/Side.
fn draw_display(
    ui: &mut egui::Ui,
    display: Rect,
    curve_a: &[Band],
    curve_b: &[Band],
    two_curves: bool,
    spectrum: Option<&[f32; ANALYSER_BANDS]>,
) {
    let colors = theme::palette();
    let painter = ui.painter_at(display);
    ui.interact(display, ui.id().with("eq-display"), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "EQ response display"));
    painter.rect_filled(display, 2.0, colors.window_background);
    let label = theme::ui_font(9.0);
    // The dB grid: a line every 6 dB, labelled at the left edge; the 0 dB line brightest. The
    // labels are drawn after the analyser's filled area, so they stay readable where it
    // covers the grid.
    let mut db_labels: Vec<(f32, String)> = Vec::new();
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let y = db_y(db, display);
        let color = if db == 0.0 { colors.text_secondary } else { colors.border };
        painter.line_segment([Pos2::new(display.left(), y), Pos2::new(display.right(), y)], Stroke::new(1.0, color));
        db_labels.push((y, if db > 0.0 { format!("+{db}") } else { format!("{db}") }));
    }
    // The frequency grid: a line at every decade and a fainter one between, labelled below.
    let mut frequency_labels: Vec<(f32, String)> = Vec::new();
    for decade in 1..=4 {
        for step in 1..10 {
            let frequency = 10f64.powi(decade) * f64::from(step);
            if !(FREQUENCY.0..=FREQUENCY.1).contains(&frequency) {
                continue;
            }
            let x = frequency_x(frequency, display);
            let color = if step == 1 { colors.border } else { colors.border.gamma_multiply(0.5) };
            painter
                .line_segment([Pos2::new(x, display.top()), Pos2::new(x, display.bottom())], Stroke::new(1.0, color));
            if step == 1 && frequency > FREQUENCY.0 {
                frequency_labels.push((
                    x,
                    if frequency >= 1000.0 { format!("{}k", frequency / 1000.0) } else { format!("{frequency}") },
                ));
            }
        }
    }
    // The analyser's filled grey area behind the response curves: the same frequency axis, its
    // own vertical scale across the display's height, each band by its centre frequency — the
    // shape of the sound as the EQ shaped it, filled from its boundary down to the display's
    // bottom, not a level read against the gain grid.
    if let Some(bands) = spectrum {
        let boundary: Vec<Pos2> = bands
            .iter()
            .enumerate()
            .map(|(band, &db)| {
                let centre =
                    FREQUENCY.0 * (FREQUENCY.1 / FREQUENCY.0).powf((band as f64 + 0.5) / ANALYSER_BANDS as f64);
                Pos2::new(frequency_x(centre, display), analyser_y(f64::from(db), display))
            })
            .collect();
        let grey = analyser_grey(&colors);
        // The filled area is a strip of triangles between the boundary and the display's
        // bottom; a raw mesh has a hard edge, so the boundary is drawn again as a stroke over
        // it, softening it the way the grid's own lines are softened.
        let mut mesh = egui::Mesh::default();
        for point in boundary.iter() {
            mesh.colored_vertex(*point, grey);
            mesh.colored_vertex(Pos2::new(point.x, display.bottom()), grey);
        }
        for index in 1..boundary.len() as u32 {
            let top = index * 2;
            mesh.add_triangle(top - 2, top - 1, top);
            mesh.add_triangle(top - 1, top, top + 1);
        }
        painter.add(egui::Shape::mesh(mesh));
        painter.add(egui::Shape::line(boundary, Stroke::new(1.0, grey)));
    }
    for (y, text) in db_labels {
        painter.text(
            Pos2::new(display.left() + 3.0, y),
            Align2::LEFT_CENTER,
            text,
            label.clone(),
            colors.text_secondary,
        );
    }
    for (x, text) in frequency_labels {
        painter.text(
            Pos2::new(x, display.bottom() - 4.0),
            Align2::CENTER_CENTER,
            text,
            label.clone(),
            colors.text_secondary,
        );
    }
    let curves: [(&[Band], f32); 2] = [(curve_a, 1.0), (curve_b, 0.5)];
    for (curve, strength) in curves.into_iter().take(usize::from(two_curves) + 1) {
        painter.add(egui::Shape::line(
            curve_points(display, curve),
            Stroke::new(1.5, colors.accent.gamma_multiply(strength)),
        ));
    }
}

/// A band's dot: a numbered circle at its frequency and gain. Clicking or dragging selects the
/// band; dragging moves its frequency (and its gain where its type has one); double-click
/// resets the band. Dragging a band that is off turns it on.
#[allow(clippy::too_many_arguments)]
fn draw_dots(
    ui: &mut egui::Ui,
    display: Rect,
    edited: &[Band],
    selection: &mut (usize, usize),
    parameters: &[GraphParameter],
    track: TrackId,
    address: &Address,
    actions: &mut Vec<Action>,
) {
    let colors = theme::palette();
    let painter = ui.painter_at(display);
    for (index, band) in edited.iter().copied().enumerate() {
        let at = Pos2::new(
            frequency_x(band.frequency, display).clamp(display.left() + 8.0, display.right() - 8.0),
            db_y(band.gain, display),
        );
        let id = address.ids.with(("eq-dot", selection.0, index));
        let caption = format!(
            "Band {}: {} at {}, {:.2} dB, Q {:.2}",
            index + 1,
            TYPE_NAMES[band.kind],
            frequency_text(band.frequency),
            band.gain,
            band.q
        );
        let response = ui
            .interact(Rect::from_center_size(at, Vec2::splat(22.0)), id, Sense::click_and_drag())
            .on_hover_text(format!("{caption}. Drag to move; double-click resets the band."))
            .on_hover_cursor(egui::CursorIcon::Move);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &caption));
        let selected = selection.1 == index;
        let (fill, ring, ink) = if selected {
            (colors.accent, colors.accent, theme::text_inverse())
        } else if band.on {
            (colors.panel_background, colors.accent, colors.accent)
        } else {
            (colors.panel_background, colors.border, colors.text_secondary)
        };
        painter.circle_filled(at, 7.5, fill);
        painter.circle_stroke(at, 7.5, Stroke::new(if band.on { 1.5 } else { 1.0 }, ring));
        painter.text(at, Align2::CENTER_CENTER, (index + 1).to_string(), theme::ui_font(9.0), ink);
        if response.clicked() || response.drag_started() {
            selection.1 = index;
        }
        let state_id = id.with("drag");
        let held = response.is_pointer_button_down_on();
        let start = ui.ctx().data_mut(|d| {
            if held && d.get_temp::<[f32; 2]>(state_id).is_none() {
                d.insert_temp(state_id, [frequency_x(band.frequency, display), db_y(band.gain, display)]);
            }
            d.get_temp::<[f32; 2]>(state_id)
        });
        let fine = if ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
        if let Some([start_x, start_y]) = start {
            let mut edits: Vec<(&GraphParameter, f64)> = Vec::new();
            if response.double_clicked() {
                // The band's three parameters go back to their defaults, as an envelope's
                // point does.
                for field in ["Frequency", "Gain", "Q"] {
                    if let Some(parameter) = parameter_by_field(parameters, selection.0, index, field) {
                        edits.push((parameter, parameter.default));
                    }
                }
            } else if response.dragged() {
                let total = response.total_drag_delta().unwrap_or_default();
                let frequency = x_frequency(start_x + total.x * fine, display);
                if frequency != band.frequency
                    && let Some(parameter) = parameter_by_field(parameters, selection.0, index, "Frequency")
                {
                    edits.push((parameter, frequency));
                }
                if band.gain_editable() {
                    let gain = y_db(start_y + total.y * fine, display);
                    if gain != band.gain
                        && let Some(parameter) = parameter_by_field(parameters, selection.0, index, "Gain")
                    {
                        edits.push((parameter, gain));
                    }
                }
                // A band dragged while it is off comes on as it moves.
                if (!edits.is_empty())
                    && !band.on
                    && let Some(parameter) = parameter_by_field(parameters, selection.0, index, "On")
                {
                    edits.insert(0, (parameter, 1.0));
                }
            } else if response.drag_stopped() {
                // The drag ends with the values as they are, so the gesture closes even when
                // the pointer came back to where it started.
                if let Some(parameter) = parameter_by_field(parameters, selection.0, index, "Frequency") {
                    edits.push((parameter, band.frequency));
                }
                if band.gain_editable()
                    && let Some(parameter) = parameter_by_field(parameters, selection.0, index, "Gain")
                {
                    edits.push((parameter, band.gain));
                }
            }
            push_edits(actions, edits, track, &address.target, "eq-curve-dot", response.dragged());
        }
        if !held {
            ui.ctx().data_mut(|d| d.remove::<[f32; 2]>(state_id));
        }
    }
}

/// The selected band's Frequency, Gain and Q strips: a caption, a vertical track with a thumb
/// and the value. Dragging moves the value; double-click resets it; the arrow keys step it
/// while a strip has focus.
#[allow(clippy::too_many_arguments)]
fn draw_strip(
    ui: &mut egui::Ui,
    bounds: Rect,
    field: usize,
    selection: (usize, usize),
    parameters: &[GraphParameter],
    track: TrackId,
    address: &Address,
    actions: &mut Vec<Action>,
) {
    let field_name = ["Frequency", "Gain", "Q"][field];
    let Some(parameter) = parameter_by_field(parameters, selection.0, selection.1, field_name) else { return };
    let logarithmic = parameter.id.ends_with('Q') || parameter.id.ends_with("Frequency");
    let colors = theme::palette();
    let group = Rect::from_min_size(
        bounds.min + Vec2::new(STRIPS[0], STRIPS[1] + field as f32 * STRIPS[3]),
        Vec2::new(STRIPS[2], STRIPS[3]),
    );
    let painter = ui.painter_at(group);
    let slot = Rect::from_min_size(Pos2::new(group.center().x - 13.0, group.top() + 13.0), Vec2::new(26.0, 24.0));
    painter.text(
        Pos2::new(group.center().x, group.top()),
        Align2::CENTER_TOP,
        ["Freq", "Gain", "Q"][field],
        theme::ui_font(9.0),
        colors.text_secondary,
    );
    painter.rect_filled(slot, 3.0, colors.window_background);
    painter.rect_stroke(slot, 3.0, Stroke::new(1.0, colors.border), egui::StrokeKind::Inside);
    painter.line_segment(
        [Pos2::new(slot.center().x, slot.top() + 3.0), Pos2::new(slot.center().x, slot.bottom() - 3.0)],
        Stroke::new(1.0, colors.border),
    );
    let text = if parameter.id.ends_with("Frequency") {
        frequency_text(parameter.value)
    } else if parameter.id.ends_with("Gain") {
        format!("{:.2} dB", parameter.value)
    } else {
        format!("{:.2}", parameter.value)
    };
    painter.text(
        Pos2::new(group.center().x, group.bottom() - 12.0),
        Align2::CENTER_CENTER,
        text.clone(),
        theme::ui_font(10.0),
        colors.text_value,
    );
    let response = ui
        .interact(
            Rect::from_min_size(Pos2::new(group.left(), group.top() + 11.0), Vec2::new(group.width(), 28.0)),
            address.ids.with(("eq-strip", field)),
            Sense::click_and_drag(),
        )
        .on_hover_text(format!(
            "{}: drag vertically; Shift for precision; double-click resets; the arrow keys step it.",
            parameter.name
        ))
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &parameter.name);
        info.current_text_value = Some(text.clone());
        info
    });
    if response.clicked() || response.dragged() {
        response.request_focus();
    }
    let normalized = |p: &GraphParameter, value: f64| {
        if logarithmic && p.min > 0.0 {
            ((value.max(p.min).ln() - p.min.ln()) / (p.max.ln() - p.min.ln())).clamp(0.0, 1.0)
        } else if p.max > p.min {
            ((value - p.min) / (p.max - p.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let denormalized = |p: &GraphParameter, norm: f64| {
        if logarithmic && p.min > 0.0 {
            (p.min.ln() + norm * (p.max.ln() - p.min.ln())).exp()
        } else {
            p.min + norm * (p.max - p.min)
        }
    };
    let at = normalized(parameter, parameter.value);
    let thumb_y = slot.bottom() - 5.0 - (slot.height() - 10.0) * at as f32;
    painter.circle_filled(
        Pos2::new(slot.center().x, thumb_y),
        5.0,
        if response.dragged() { colors.accent } else { colors.control_hover },
    );
    painter.circle_stroke(Pos2::new(slot.center().x, thumb_y), 5.0, Stroke::new(1.0, colors.border));
    let held = response.is_pointer_button_down_on();
    let state_id = response.id.with("start");
    let start = ui.ctx().data_mut(|d| {
        if held && d.get_temp::<f64>(state_id).is_none() {
            d.insert_temp(state_id, at);
        }
        d.get_temp::<f64>(state_id)
    });
    let fine: f64 = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
    let focused = response.has_focus();
    let keys = ui.input(|i| {
        f32::from(focused && i.key_pressed(egui::Key::ArrowUp))
            - f32::from(focused && i.key_pressed(egui::Key::ArrowDown))
    });
    let mut next = start.unwrap_or(at);
    if response.dragged() {
        let total = response.total_drag_delta().unwrap_or_default();
        next = (next - f64::from(total.y) * fine / crate::kit::widgets::PARAMETER_DRAG_PIXELS).clamp(0.0, 1.0);
    }
    next = (next + f64::from(keys) * 0.01 * fine).clamp(0.0, 1.0);
    if next != at || response.double_clicked() || response.drag_stopped() {
        let value = if response.double_clicked() { parameter.default } else { denormalized(parameter, next) };
        push_edits(actions, vec![(parameter, value)], track, &address.target, "eq-curve-strip", response.dragged());
    }
    if !held {
        ui.ctx().data_mut(|d| d.remove::<f64>(state_id));
    }
}

/// The bands' type menus under the display: the type's glyph and a caret; the menu lists every
/// type. A type change is the same command a generated list writes.
#[allow(clippy::too_many_arguments)]
fn draw_type_menus(
    ui: &mut egui::Ui,
    bounds: Rect,
    edited: &[Band],
    selection: (usize, usize),
    parameters: &[GraphParameter],
    track: TrackId,
    address: &Address,
    actions: &mut Vec<Action>,
) {
    let colors = theme::palette();
    for (index, band) in edited.iter().copied().enumerate() {
        let rect = Rect::from_min_size(
            bounds.min + Vec2::new(DISPLAY[0] + index as f32 * BAND_WIDTH, MENU_ROW),
            Vec2::new(BAND_WIDTH - 2.0, ROW_HEIGHT - 2.0),
        );
        let response = ui
            .interact(rect, address.ids.with(("eq-type", selection.0, index)), Sense::click())
            .on_hover_text(format!("Band {}'s type: {}", index + 1, TYPE_NAMES[band.kind]));
        response.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Band {} type", index + 1));
            info.current_text_value = Some(TYPE_NAMES[band.kind].to_string());
            info
        });
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, colors.control_background);
        painter.rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, if response.hovered() { colors.text_secondary } else { colors.border }),
            egui::StrokeKind::Inside,
        );
        glyph(&painter, rect.shrink2(Vec2::new(10.0, 5.0)), band.kind, colors.text_value);
        let caret = Pos2::new(rect.right() - 6.0, rect.center().y);
        painter.add(egui::Shape::convex_polygon(
            vec![caret + Vec2::new(-2.5, -1.0), caret + Vec2::new(2.5, -1.0), caret + Vec2::new(0.0, 2.0)],
            colors.text_secondary,
            Stroke::NONE,
        ));
        let mut picked = None;
        egui::Popup::menu(&response)
            .frame(
                egui::Frame::popup(ui.style())
                    .corner_radius(0)
                    .fill(colors.panel_background)
                    .stroke(Stroke::new(1.0, colors.accent)),
            )
            .show(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.spacing_mut().interact_size.y = 14.0;
                for (i, name) in TYPE_NAMES.iter().enumerate() {
                    if ui.selectable_label(band.kind == i, *name).clicked() {
                        picked = Some(i);
                    }
                }
            });
        if let Some(kind) = picked.filter(|kind| *kind != band.kind)
            && let Some(parameter) = parameter_by_field(parameters, selection.0, index, "Type")
        {
            push_edits(actions, vec![(parameter, kind as f64)], track, &address.target, "eq-curve-type", false);
        }
    }
}

/// A type's glyph: the shape of its response, drawn small, up from the bottom of `rect`.
fn glyph(painter: &egui::Painter, rect: Rect, kind: usize, color: Color32) {
    let at = |x: f32, y: f32| Pos2::new(rect.left() + x * rect.width(), rect.bottom() - y * rect.height());
    let shape: &[(f32, f32)] = match kind {
        0 => &[(0.0, 0.05), (0.3, 0.1), (0.5, 0.5), (0.65, 0.9), (1.0, 0.9)],
        1 => &[(0.0, 0.2), (0.35, 0.35), (0.6, 0.6), (0.8, 0.9), (1.0, 0.9)],
        2 => &[(0.0, 0.9), (0.4, 0.85), (0.6, 0.5), (0.8, 0.2), (1.0, 0.15)],
        3 => &[(0.0, 0.5), (0.3, 0.52), (0.45, 0.75), (0.5, 0.85), (0.55, 0.75), (0.7, 0.52), (1.0, 0.5)],
        4 => &[(0.0, 0.5), (0.35, 0.48), (0.45, 0.25), (0.5, 0.15), (0.55, 0.25), (0.65, 0.48), (1.0, 0.5)],
        5 => &[(0.0, 0.15), (0.2, 0.2), (0.4, 0.5), (0.6, 0.85), (1.0, 0.9)],
        6 => &[(0.0, 0.9), (0.2, 0.9), (0.4, 0.6), (0.65, 0.35), (1.0, 0.2)],
        _ => &[(0.0, 0.9), (0.35, 0.9), (0.5, 0.5), (0.7, 0.1), (1.0, 0.05)],
    };
    let points: Vec<Pos2> = shape.iter().map(|(x, y)| at(*x, *y)).collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.2, color)));
}

/// The bands' numbered selectors under the type menus: lit while the band is on, outlined
/// while it is selected. Clicking a band's number selects it and switches it on; clicking the
/// selected band's number switches it off.
#[allow(clippy::too_many_arguments)]
fn draw_numbers(
    ui: &mut egui::Ui,
    bounds: Rect,
    edited: &[Band],
    selection: &mut (usize, usize),
    parameters: &[GraphParameter],
    track: TrackId,
    address: &Address,
    actions: &mut Vec<Action>,
) {
    let colors = theme::palette();
    for (index, band) in edited.iter().copied().enumerate() {
        let rect = Rect::from_min_size(
            bounds.min + Vec2::new(DISPLAY[0] + index as f32 * BAND_WIDTH, NUMBER_ROW),
            Vec2::new(BAND_WIDTH - 2.0, ROW_HEIGHT),
        );
        let selected = selection.1 == index;
        let response = ui
            .interact(rect, address.ids.with(("eq-number", selection.0, index)), Sense::click())
            .on_hover_text(if band.on {
                format!("Band {} is on. Click again to switch it off.", index + 1)
            } else {
                format!("Band {} is off. Click to switch it on and select it.", index + 1)
            });
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Band {}", index + 1)));
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, if band.on { colors.accent } else { colors.control_background });
        painter.rect_stroke(
            rect,
            2.0,
            Stroke::new(if selected { 2.0 } else { 1.0 }, if selected { colors.text_primary } else { colors.border }),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            (index + 1).to_string(),
            theme::ui_font(10.0),
            if band.on { theme::text_inverse() } else { colors.text_secondary },
        );
        if response.clicked() {
            let mut edits: Vec<(&GraphParameter, f64)> = Vec::new();
            if selected && band.on {
                if let Some(parameter) = parameter_by_field(parameters, selection.0, index, "On") {
                    edits.push((parameter, 0.0));
                }
            } else if !band.on
                && let Some(parameter) = parameter_by_field(parameters, selection.0, index, "On")
            {
                edits.push((parameter, 1.0));
            }
            push_edits(actions, edits, track, &address.target, "eq-curve-number", false);
            selection.1 = index;
        }
    }
}

/// The Edit switch in the globals' column: which curve's bands the display edits, in Left/Right
/// and Mid/Side. Panel state only; the device has no such parameter.
fn draw_edit_switch(ui: &mut egui::Ui, bounds: Rect, selection: &mut (usize, usize), address: &Address) {
    let colors = theme::palette();
    let rect = Rect::from_min_size(bounds.min + Vec2::new(EDIT[0], EDIT[1]), Vec2::new(EDIT[2], EDIT[3]));
    let painter = ui.painter_at(rect);
    let width = rect.width() / 2.0 - 1.0;
    for (index, letter) in ["A", "B"].iter().enumerate() {
        let button = Rect::from_min_size(
            Pos2::new(rect.left() + index as f32 * (width + 2.0), rect.top()),
            Vec2::new(width, rect.height()),
        );
        let selected = selection.0 == index;
        let response = ui
            .interact(button, address.ids.with(("eq-edit", index)), Sense::click())
            .on_hover_text(format!("Edit curve {letter}'s bands"));
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Edit curve {letter}")));
        painter.rect_filled(button, 2.0, if selected { colors.accent } else { colors.control_background });
        painter.rect_stroke(button, 2.0, Stroke::new(1.0, colors.border), egui::StrokeKind::Inside);
        painter.text(
            button.center(),
            Align2::CENTER_CENTER,
            *letter,
            theme::ui_font(10.0),
            if selected { theme::text_inverse() } else { colors.text_value },
        );
        if response.clicked() {
            selection.0 = index;
        }
    }
}

#[cfg(test)]
#[path = "eq_curve_tests.rs"]
mod tests;
