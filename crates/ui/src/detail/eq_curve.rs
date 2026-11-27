// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The EQ's panel (the SDK's `Widget::EqCurve`, docs/spec/eq.md › The panel), drawn on the UI
//! thread from the device's own filter maths. It lays itself out from the rectangle it is given,
//! in one of three ways:
//!
//! - **Compact** (the device row, 560 × 168): the selected band's column at the left (its number,
//!   previous and next, on/off, the shape menu, the placement menu, delete, and its Frequency,
//!   Gain and Q dials), the response display beside it, and the output meter at the right edge of
//!   the globals' column (whose controls are the SDK's own, `devices/eq/src/panel.rs`).
//! - **Band list**: while the device's expanded strip is open, the compact panel lists the bands
//!   in use instead of drawing a second display: one row each, every value written out.
//! - **Strip** (the expanded panel, over the device row): the display over the whole strip and,
//!   once the strip is tall enough, the selected band's panel floating under its band.
//!
//! The display draws the curve of the stereo bands and, for each placement in use, the curve that
//! side hears in its placement's colour; every band in use is a numbered dot in its placement's
//! colour. While sound plays through the device, the analyser draws the spectrum of its output as
//! a filled gray area behind the curves. Which band is selected, the display's dB range and its
//! switches are panel state, shared by the compact panel and the strip; every edit is a
//! `device.setParam` command, addressed by the device's full path, so a panel works the same on
//! a device inside a group as on one of the track's own.

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, Vec2};
use serde_json::json;
use std::sync::Arc;
use suhina_device_api::DEFAULT_SAMPLE_RATE;
use suhina_device_eq::{BANDS, MODE_NAMES, PLACE_LETTERS, PLACE_NAMES, TYPE_NAMES, band_sections, has_gain};
use suhina_dsp::{AttackRelease, SvfCoefficients};
use suhina_engine::MeterMonitor;
use suhina_engine::model::TrackId;
use suhina_engine::spectrum::{self, RING};

use crate::detail::detail_view::{DeviceAt, GraphParameter};
use crate::kit::theme;
use crate::kit::widgets::{Action, fixed};

/// The compact panel's selected band column, `[x, y, width, height]` in the panel.
const COLUMN: [f32; 4] = [2.0, 2.0, 92.0, 164.0];
/// The compact panel's display: beside the column, up to the globals' column.
const DISPLAY: [f32; 4] = [98.0, 2.0, 384.0, 164.0];
/// The band list the compact panel shows while the strip is open: the column's and the
/// display's room together.
const LIST: [f32; 4] = [2.0, 2.0, 480.0, 164.0];
/// The output meter at the right edge of the compact panel's globals' column.
const METER: [f32; 4] = [550.0, 2.0, 8.0, 164.0];
/// From this height (in points) the strip shows the selected band's floating panel under the
/// curve, with a lane of its own below the display's lowest gain.
const STRIP_PANEL_MIN_HEIGHT: f32 = 340.0;
/// The floating band panel's size in the strip.
const FLOATING: Vec2 = Vec2::new(428.0, 84.0);
/// How much wider the floating panel is while it shows the band's dynamics.
const FLOATING_DYNAMICS: f32 = 214.0;
/// The lane under the strip's curve the floating panel sits in.
const FLOATING_LANE: f32 = 100.0;
/// The display's dB ranges, top to bottom ±this; a click on the range button steps through them,
/// and a band dragged past the range widens it.
const DB_RANGES: [f64; 4] = [6.0, 12.0, 15.0, 30.0];
/// The frequency axis: the frequency parameter's full range, left to right.
const FREQUENCY: (f64, f64) = (10.0, 22_000.0);
/// The curve's points across the display.
const POINTS: usize = 256;
/// The rate the response is drawn at; the device runs at the project's rate, and only the bend
/// near this Nyquist differs (docs/spec/eq.md › The panel › Decisions).
const DRAWN_RATE: f64 = DEFAULT_SAMPLE_RATE;
/// How many bands the analyser folds the spectrum into over the display's frequency range.
/// 128 keeps a sine's peak narrow: each band spans about 6.2 % of its center frequency,
/// about 27 Hz at 440 Hz, so the transforms closest to the tone carry its peak alone and
/// the columns beside it read the skirts.
const ANALYSER_BANDS: usize = 256;
/// The analyser's own vertical scale across the display's height: this level in dB at the top
/// edge, this at the bottom. The line is not read against the gain grid — normal material
/// sits far below the grid, so on it the line would fall below the display and never show
/// (docs/spec/eq.md › The panel › The analyser).
const ANALYSER_DB: (f64, f64) = (0.0, -60.0);
/// The strength of the analyser's filled area's gray, of the theme's secondary gray at its
/// full value.
const ANALYSER_FILL: f32 = 0.45;
/// How long a band's shown level takes to rise to a louder reading, in seconds.
const ANALYSER_ATTACK: f64 = 0.02;
/// How long a band's shown level takes to fall after a quieter reading, in seconds: a slow fall
/// turns the window-to-window flicker of a fresh transform into a flowing shape.
const ANALYSER_RELEASE: f64 = 1.2;
/// The placements' colours, in `PLACE_NAMES` order; Stereo takes the theme's accent.
const PLACE_COLORS: [Color32; 5] = [
    Color32::from_rgb(0x73, 0xD2, 0xBC),
    Color32::from_rgb(0x6F, 0xA8, 0xFF),
    Color32::from_rgb(0xFF, 0x7A, 0x9A),
    Color32::from_rgb(0xF2, 0xC9, 0x4C),
    Color32::from_rgb(0xB5, 0x8C, 0xFF),
];
/// The note names of an octave, from C.
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The analyser's filled area's gray: the theme's secondary gray scaled to [`ANALYSER_FILL`] of
/// its value, opaque, so the area reads as a solid shape over the grid behind it.
fn analyser_gray(colors: &super::device_canvas::Colors) -> Color32 {
    let [r, g, b, _] = colors.text_secondary.to_array();
    Color32::from_rgb(
        (r as f32 * ANALYSER_FILL).round() as u8,
        (g as f32 * ANALYSER_FILL).round() as u8,
        (b as f32 * ANALYSER_FILL).round() as u8,
    )
}

/// One band slot, as the panel reads it: its values and the filter sections the device's own
/// maths ([`band_sections`]) runs for them.
#[derive(Clone, Copy)]
struct Band {
    in_use: bool,
    on: bool,
    kind: usize,
    frequency: f64,
    gain: f64,
    q: f64,
    place: usize,
    /// The mode it runs: 0 Static, 1 Dynamic, 2 Spectral (a type that cannot take the mode it
    /// asks for runs Static, as the device does), and how far its gain may move, in dB.
    mode: usize,
    range: f64,
    /// Whether its detector hears the device's key.
    keyed: bool,
    sections: [SvfCoefficients; 4],
    count: usize,
}

/// The mode a band of type `kind` runs for its Mode parameter: Dynamic for the types with gain,
/// Spectral for the bell and the shelves, Static otherwise (as the device decides).
fn effective_mode(asked: f64, kind: usize) -> usize {
    match asked.round() as usize {
        1 if has_gain(kind) => 1,
        2 if matches!(kind, 2 | 3 | 5) => 2,
        _ => 0,
    }
}

/// A band's own colour: hues spread around the wheel slot by slot (a golden-ratio step, so
/// neighboring slots differ), at one saturation and brightness. Its dot, its column and its
/// dynamics (reach, live curve, handle) take it.
fn band_color(slot: usize) -> Color32 {
    let hue = (0.58 + slot as f32 * 0.618_034).fract();
    let (s, v) = (0.62, 0.98);
    let h = hue * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |u: f32| ((u + m) * 255.0).round() as u8;
    Color32::from_rgb(to(r), to(g), to(b))
}

impl Band {
    /// Whether the band's type has a gain to drag: the cuts and the notch have none.
    fn gain_editable(&self) -> bool {
        has_gain(self.kind)
    }

    /// Whether the band shapes the sound now.
    fn heard(&self) -> bool {
        self.in_use && self.on
    }
}

/// The panel's own state, kept per device while the app runs and shared by the compact panel
/// and the strip: none of it is a parameter, so none of it is saved or undone.
#[derive(Clone, Copy)]
struct PanelState {
    /// The selected band's slot: the one the column, the floating panel and the list follow.
    selected: Option<usize>,
    /// Every selected band, one bit per slot, the selected one among them: a drag on one of their
    /// dots moves them together, and Delete removes them together.
    group: u32,
    /// The display's range, ± this many dB.
    db_range: f64,
    /// Whether the analyser draws.
    analyser: bool,
    /// The analyser's settings (docs/spec/eq.md › The analyser › Settings).
    spectrum: SpectrumSettings,
    /// Whether the frequency axis shows piano keys.
    piano: bool,
    /// Whether a drag on the display draws a curve that becomes bands.
    draw: bool,
    /// Whether holding a band's dot listens to it alone (the headphones switch).
    audition: bool,
    /// The band the engine was last told to play alone, if any.
    listening: Option<usize>,
    /// The band list's first row.
    list_scroll: usize,
    /// Whether the column shows the selected band's dynamics instead of its Frequency, Gain and Q.
    dynamics: bool,
    /// Whether the strip's floating panel follows its band (it sits in the middle otherwise).
    follow: bool,
    /// Whether the settings page covers the display.
    settings: bool,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            selected: None,
            group: 0,
            db_range: 15.0,
            analyser: true,
            spectrum: SpectrumSettings::default(),
            piano: false,
            draw: false,
            audition: false,
            listening: None,
            list_scroll: 0,
            dynamics: false,
            follow: false,
            settings: false,
        }
    }
}

/// The analyser's settings, panel state like the rest: its range below full scale, how long a
/// band takes to fall, its tilt around 1 kHz, and whether it holds its highest reading.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SpectrumSettings {
    /// The level at the display's bottom edge, in dB below full scale: 60, 90 or 120.
    range: f64,
    /// The fall, in seconds.
    release: f64,
    /// dB per octave added around 1 kHz, so a mix's falling top reads flatter.
    tilt: f64,
    /// Holds each band's highest reading until it is switched off.
    freeze: bool,
}

impl Default for SpectrumSettings {
    fn default() -> Self {
        Self { range: -ANALYSER_DB.1, release: ANALYSER_RELEASE, tilt: 0.0, freeze: false }
    }
}

/// The analyser's ranges, falls and tilts its menu offers.
const SPECTRUM_RANGES: [f64; 3] = [60.0, 90.0, 120.0];
const SPECTRUM_RELEASES: [(&str, f64); 3] = [("Fast", 0.3), ("Medium", ANALYSER_RELEASE), ("Slow", 3.0)];
const SPECTRUM_TILTS: [f64; 3] = [0.0, 3.0, 4.5];

/// A band's shown level with the tilt added: `tilt` dB per octave above 1 kHz, as much less
/// below it.
fn tilted(db: f64, frequency: f64, tilt: f64) -> f64 {
    db + tilt * (frequency / 1000.0).log2()
}

/// Whether a toggle keyed `id` has not happened yet in this pass, marking it done. The panel can be
/// drawn more than once in one pass, and a toggle (a Ctrl-click, a switch) must flip once.
fn once(ui: &egui::Ui, id: egui::Id) -> bool {
    let pass = ui.ctx().cumulative_pass_nr();
    let first = ui.ctx().data(|d| d.get_temp::<u64>(id)) != Some(pass);
    if first {
        ui.ctx().data_mut(|d| d.insert_temp(id, pass));
    }
    first
}

/// The panel's state as kept between frames: as it is now, and as the frame `frame` began, last
/// written in pass `pass` (see `show`).
#[derive(Clone, Copy)]
struct KeptState {
    frame: u64,
    pass: u64,
    began: PanelState,
    state: PanelState,
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
    track: TrackId,
}

impl Address {
    /// The address of the device at `at`: its command target and its id key.
    fn new(track: TrackId, at: &DeviceAt) -> Self {
        Self { target: at.value(), ids: egui::Id::unique(("eq-curve", track.0, &at.0)), track }
    }
}

/// What a frame of the panel works with: the parameters, the bands they make, the state and the
/// edits it collects.
struct Panel<'a> {
    parameters: &'a [GraphParameter],
    /// The band held to listen to this frame, if any.
    listen: Option<usize>,
    /// Whether this device can be auditioned: one on a track's own chain.
    can_listen: bool,
    /// Each band's gain change now, from the device's readouts, while sound plays through it.
    live: Option<[f32; BANDS]>,
    /// The first heard Spectral band's parts' gain changes now, from the readouts after the bands'.
    live_parts: Option<[f32; suhina_device_eq::SPECTRAL_READOUTS]>,
    bands: Vec<Band>,
    state: PanelState,
    address: Address,
    actions: &'a mut Vec<Action>,
}

impl Panel<'_> {
    /// The parameter of band `slot` (from 0) named by its field, e.g. `a3Gain`.
    fn parameter(&self, slot: usize, field: &str) -> Option<&GraphParameter> {
        parameter_by_field(self.parameters, slot, field)
    }

    /// Writes the edits as `device.setParam` commands in one gesture (one undo step).
    fn edit(&mut self, edits: &[(usize, &str, f64)], gesture: &str) {
        let parameters = self.parameters;
        let pairs: Vec<(&GraphParameter, f64)> = edits
            .iter()
            .filter_map(|(slot, field, value)| parameter_by_field(parameters, *slot, field).map(|p| (p, *value)))
            .collect();
        push_edits(self.actions, pairs, self.address.track, &self.address.target, gesture);
    }

    /// Selects one band, alone.
    fn select(&mut self, slot: usize) {
        self.state.selected = Some(slot);
        self.state.group = 1 << slot;
    }

    /// Clears the selection.
    fn deselect(&mut self) {
        self.state.selected = None;
        self.state.group = 0;
    }

    /// Whether the band is among the selected ones.
    fn in_group(&self, slot: usize) -> bool {
        self.state.group & (1 << slot) != 0
    }

    /// The selected bands' slots, in order.
    fn group(&self) -> Vec<usize> {
        (0..BANDS).filter(|&slot| self.in_group(slot)).collect()
    }

    /// The selected band, when it is in use.
    fn selected(&self) -> Option<(usize, Band)> {
        let slot = self.state.selected?;
        self.bands.get(slot).copied().filter(|band| band.in_use).map(|band| (slot, band))
    }

    /// The slots in use, in order.
    fn used(&self) -> Vec<usize> {
        self.bands.iter().enumerate().filter(|(_, band)| band.in_use).map(|(slot, _)| slot).collect()
    }

    /// Selects the band in use before or after the selected one.
    fn step_selection(&mut self, forward: bool) {
        let used = self.used();
        if used.is_empty() {
            return;
        }
        let at = self.state.selected.and_then(|slot| used.iter().position(|&s| s == slot));
        let next = match (at, forward) {
            (None, _) => 0,
            (Some(at), true) => (at + 1) % used.len(),
            (Some(at), false) => (at + used.len() - 1) % used.len(),
        };
        if let Some(&slot) = used.get(next) {
            self.select(slot);
        }
    }

    /// Frees the band's slot, and every other selected band's with it; the selection moves to the
    /// next band in use.
    fn delete(&mut self, slot: usize) {
        let mut gone = self.group();
        if !gone.contains(&slot) {
            gone = vec![slot];
        }
        let edits: Vec<(usize, &str, f64)> = gone.iter().map(|&s| (s, "InUse", 0.0)).collect();
        self.edit(&edits, "eq-curve-delete");
        let used: Vec<usize> = self.used().into_iter().filter(|s| !gone.contains(s)).collect();
        match used.iter().copied().find(|&s| s > slot).or_else(|| used.last().copied()) {
            Some(next) => self.select(next),
            None => self.deselect(),
        }
    }

    /// Widens the display's range when a gain reaches past it.
    fn widen_for(&mut self, gain: f64) {
        if gain.abs() > self.state.db_range - 0.5
            && let Some(range) = DB_RANGES.iter().copied().find(|range| gain.abs() <= range - 0.5)
        {
            self.state.db_range = self.state.db_range.max(range);
        } else if gain.abs() > self.state.db_range {
            self.state.db_range = 30.0;
        }
    }
}

/// Draws the panel into `bounds` (the widget's rectangle in the host), editing through
/// `device.setParam` actions addressed to `at` (see [`Address`]) while the analyser claims its
/// watch through the frame's meter monitor.
pub(crate) fn show(
    ui: &mut egui::Ui,
    bounds: Rect,
    parameters: &[GraphParameter],
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let address = Address::new(track, at);
    let state_id = address.ids.with("state");
    // egui may run a frame's pass again with the same input (a discarded pass). The state is kept
    // with the one the frame began with, so a pass run again starts from it, and a toggle (a
    // Ctrl-click, a switch) happens once per frame, not once per pass. Two widgets of the same pass
    // (the compact panel and the strip) share the state as it goes.
    let frame = ui.ctx().cumulative_frame_nr();
    let pass = ui.ctx().cumulative_pass_nr();
    let kept = ui.ctx().data(|d| d.get_temp::<KeptState>(state_id));
    let (state, began) = match kept {
        Some(kept) if kept.frame == frame && kept.pass == pass => (kept.state, kept.began),
        Some(kept) if kept.frame == frame => (kept.began, kept.began),
        Some(kept) => (kept.state, kept.state),
        None => (PanelState::default(), PanelState::default()),
    };
    let can_listen = watch_key(at).is_some();
    let live = ui
        .ctx()
        .data(|d| d.get_temp::<Arc<MeterMonitor>>(egui::Id::unique("eq-spectrum")))
        .and_then(|monitor| monitor.readouts(track, &at.0))
        .map(|values| std::array::from_fn(|i| values.get(i).copied().unwrap_or(0.0)));
    let live_parts = ui
        .ctx()
        .data(|d| d.get_temp::<Arc<MeterMonitor>>(egui::Id::unique("eq-spectrum")))
        .and_then(|monitor| monitor.readouts(track, &at.0))
        .map(|values| std::array::from_fn(|i| values.get(BANDS + i).copied().unwrap_or(0.0)));
    let mut panel = Panel {
        parameters,
        listen: None,
        can_listen,
        live,
        live_parts,
        bands: bands(parameters),
        state,
        address,
        actions,
    };
    if panel.selected().is_none() {
        panel.state.selected = None;
    }
    // A band that left use leaves the selection; the selected band is always among the group.
    let used = panel.bands.iter().enumerate().filter(|(_, b)| b.in_use).fold(0u32, |m, (s, _)| m | (1 << s));
    panel.state.group &= used;
    if let Some(slot) = panel.state.selected {
        panel.state.group |= 1 << slot;
    }
    // The expanded strip draws in the foreground layer (`detail_view/expanded.rs`); the compact
    // panel of a device whose strip is open lists its bands instead of a second display.
    let strip = ui.layer_id().order == egui::Order::Foreground;
    let strip_open = !strip && super::detail_view::expanded(ui.ctx()).is_some_and(|(t, a)| t == track && a == *at);
    let analyser = if panel.state.analyser || !strip {
        watch_key(at).and_then(|device| analyser_bands(ui, track, device, panel.state.spectrum, panel.state.analyser))
    } else {
        None
    };
    let rect = |r: [f32; 4]| Rect::from_min_size(bounds.min + Vec2::new(r[0], r[1]), Vec2::new(r[2], r[3]));
    if strip {
        let display = Rect::from_min_max(bounds.min + Vec2::splat(4.0), bounds.max - Vec2::new(16.0, 4.0));
        let meter =
            Rect::from_min_max(Pos2::new(bounds.right() - 12.0, bounds.top() + 4.0), bounds.max - Vec2::new(4.0, 4.0));
        let floating = bounds.height() >= STRIP_PANEL_MIN_HEIGHT;
        let lane = if floating { FLOATING_LANE } else { 0.0 };
        draw_display(ui, &mut panel, display, lane, true, analyser.as_ref().map(|a| &a.0));
        if floating {
            draw_floating_panel(ui, &mut panel, display);
        }
        draw_meter(ui, meter, analyser.as_ref().map(|a| a.1));
    } else if strip_open {
        draw_band_list(ui, &mut panel, rect(LIST));
        draw_meter(ui, rect(METER), analyser.as_ref().map(|a| a.1));
    } else {
        draw_column(ui, &mut panel, rect(COLUMN));
        draw_display(ui, &mut panel, rect(DISPLAY), 0.0, false, analyser.as_ref().map(|a| &a.0));
        draw_meter(ui, rect(METER), analyser.as_ref().map(|a| a.1));
    }
    // Delete removes the selected band while the pointer is over the panel and no text field
    // takes the keys.
    if let Some((slot, _)) = panel.selected()
        && ui.rect_contains_pointer(bounds)
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Delete))
    {
        panel.delete(slot);
    }
    // Audition: the engine hears the held band alone, and the whole EQ again once it is let go
    // (`device.audition`, never saved or undone).
    let released = !ui.input(|i| i.pointer.primary_down());
    let wanted = panel.listen.filter(|_| panel.can_listen);
    if wanted.is_some() && wanted != panel.state.listening
        || wanted.is_none() && released && panel.state.listening.is_some()
    {
        let band = wanted.map_or(0, |slot| slot + 1);
        panel.actions.push(Action::new(
            "device.audition",
            json!({ "track": track.0, "device": panel.address.target, "band": band }),
        ));
        panel.state.listening = wanted;
    }
    let state = panel.state;
    ui.ctx().data_mut(|d| d.insert_temp(state_id, KeptState { frame, pass, began, state }));
}

/// The parameter's value, or 0 when the panel has no such id.
fn value(parameters: &[GraphParameter], id: &str) -> f64 {
    parameters.iter().find(|p| p.id == id).map_or(0.0, |p| p.value)
}

/// The parameter of band `slot` (from 0) named by its field, e.g. `a3Gain`.
fn parameter_by_field<'a>(parameters: &'a [GraphParameter], slot: usize, field: &str) -> Option<&'a GraphParameter> {
    let id = format!("a{}{field}", slot + 1);
    // The panel's parameters are the device's, in its order: a band's field is found at its
    // index without a search (hundreds of lookups a frame); any other order falls back to one.
    BAND_FIELDS
        .iter()
        .position(|f| *f == field)
        .and_then(|at| parameters.get(slot * BAND_FIELDS.len() + at))
        .filter(|p| p.id == id)
        .or_else(|| parameters.iter().find(|p| p.id == id))
}

/// A band's parameters' fields in the device's order (`suhina_device_eq::param`).
const BAND_FIELDS: [&str; 14] = [
    "On",
    "Type",
    "Frequency",
    "Gain",
    "Q",
    "InUse",
    "Place",
    "Mode",
    "Range",
    "Threshold",
    "Attack",
    "Release",
    "Detail",
    "Trigger",
];

/// Every band slot, in use or not, from the panel's parameters.
fn bands(parameters: &[GraphParameter]) -> Vec<Band> {
    let scale = value(parameters, "scale");
    let adaptive = value(parameters, "adaptiveQ") >= 0.5;
    (1..=BANDS)
        .map(|n| {
            let named = |field: &str| parameter_by_field(parameters, n - 1, field).map_or(0.0, |p| p.value);
            let kind = named("Type").round().clamp(0.0, (TYPE_NAMES.len() - 1) as f64) as usize;
            let frequency = named("Frequency").clamp(FREQUENCY.0, FREQUENCY.1);
            let gain = named("Gain");
            let q = named("Q").max(0.01);
            let (sections, count) = band_sections(kind, DRAWN_RATE, frequency, gain, q, scale, adaptive);
            Band {
                in_use: named("InUse") >= 0.5,
                on: named("On") >= 0.5,
                kind,
                frequency,
                gain,
                q,
                place: named("Place").round().clamp(0.0, (PLACE_NAMES.len() - 1) as f64) as usize,
                mode: effective_mode(named("Mode"), kind),
                range: named("Range"),
                keyed: named("Trigger") >= 0.5,
                sections,
                count,
            }
        })
        .collect()
}

/// A placement's colour; Stereo is the theme's accent.
fn place_color(place: usize, colors: &super::device_canvas::Colors) -> Color32 {
    if place == 0 { colors.accent } else { PLACE_COLORS.get(place).copied().unwrap_or(colors.accent) }
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

/// The y of a level in dB on a display of ±`range` dB: −`range` at the bottom, +`range` at the
/// top, 0 in the middle.
fn db_y(db: f64, display: Rect, range: f64) -> f32 {
    display.bottom() - ((db / range) + 1.0) as f32 / 2.0 * display.height()
}

/// The level in dB at a y, clamped to the display's range.
fn y_db(y: f32, display: Rect, range: f64) -> f64 {
    ((display.bottom() - y) / display.height()).clamp(0.0, 1.0) as f64 * 2.0 * range - range
}

/// The y of the analyser's level in dB, on its own scale across the display's height: full
/// scale at the top edge, [`ANALYSER_DB`]'s floor at its bottom, clamped to the display.
#[cfg(test)]
fn analyser_y(db: f64, display: Rect) -> f32 {
    analyser_y_in(db, display, -ANALYSER_DB.1)
}

/// The y of the analyser's level in dB with the floor `range` dB below full scale, clamped to the display.
fn analyser_y_in(db: f64, display: Rect, range: f64) -> f32 {
    let down = ((ANALYSER_DB.0 - db) / range.max(1.0)).clamp(0.0, 1.0);
    display.top() + down as f32 * display.height()
}

/// The analyser's watch key for a device's path: a top-level device's index, or `None` for a
/// device inside a group — the chain pass taps no grouped device, and its branch-local index
/// would name a top-level device's row and show that device's spectrum (#1050 keys watches
/// by path; docs/spec/eq.md › The panel › The analyser).
fn watch_key(at: &DeviceAt) -> Option<usize> {
    (at.0.len() == 1).then(|| at.0[0])
}

/// The summed response in dB of the bands `heard` picks, at `frequency`.
#[cfg(test)]
fn response_db(bands: &[Band], frequency: f64, heard: impl Fn(&Band) -> bool) -> f64 {
    let mut total = 1.0;
    for band in bands.iter().filter(|band| band.heard() && heard(band)) {
        for coefficients in &band.sections[..band.count] {
            total *= coefficients.magnitude(DRAWN_RATE, frequency);
        }
    }
    20.0 * total.max(1.0e-6).log10()
}

/// The summed response of the bands `heard` picks, in points across the display.
#[cfg(test)]
fn curve_points(display: Rect, range: f64, bands: &[Band], heard: impl Fn(&Band) -> bool) -> Vec<Pos2> {
    (0..=POINTS)
        .map(|i| {
            let frequency = FREQUENCY.0 * ((i as f64 / POINTS as f64) * (FREQUENCY.1 / FREQUENCY.0).ln()).exp();
            let db = response_db(bands, frequency, &heard).clamp(-range * 1.2, range * 1.2);
            Pos2::new(frequency_x(frequency, display), db_y(db, display, range))
        })
        .collect()
}

/// The edited values as `device.setParam` commands, all in `gesture`, so an edit of several
/// values and a drag are one undo step each: the window ends the gesture on mouse up (#1146).
fn push_edits(
    actions: &mut Vec<Action>,
    edits: Vec<(&GraphParameter, f64)>,
    track: TrackId,
    device: &serde_json::Value,
    gesture: &str,
) {
    for (parameter, value) in edits {
        actions.push(Action::new(
            "device.setParam",
            json!({
                "track": track.0, "device": device, "param": parameter.id, "value": value,
                "gesture": gesture
            }),
        ));
    }
}

/// A frequency as the panel reads it: one decimal, Hz below a kilohertz and kHz above.
fn frequency_text(frequency: f64) -> String {
    if frequency >= 1000.0 {
        format!("{} kHz", fixed(frequency / 1000.0, 1))
    } else {
        format!("{} Hz", fixed(frequency, 1))
    }
}

/// A gain as the panel reads it, signed, one decimal.
fn gain_text(gain: f64) -> String {
    if gain > 0.04 { format!("+{gain:.1} dB") } else { format!("{gain:.1} dB") }
}

/// The note nearest a frequency and how many cents it is off, e.g. "A4 +0" or "D#5 -13".
fn note_text(frequency: f64) -> String {
    let midi = 69.0 + 12.0 * (frequency / 440.0).log2();
    let nearest = midi.round();
    let cents = ((midi - nearest) * 100.0).round() as i64;
    let name = NOTE_NAMES[(nearest as i64).rem_euclid(12) as usize];
    let octave = (nearest as i64).div_euclid(12) - 1;
    format!("{name}{octave} {}{cents}", if cents >= 0 { "+" } else { "" })
}

/// Whether a character can be part of a typed frequency: a number, "k" notation or a note name.
fn frequency_char(c: char) -> bool {
    c.is_ascii_digit()
        || matches!(c, '-' | '+' | '.' | ',' | ' ' | '#' | 'k' | 'K' | 'b')
        || ('A'..='G').contains(&c)
        || ('a'..='g').contains(&c)
}

/// A typed frequency in Hz: a number ("440", "1,5"), "k" notation ("6k3" is 6.3 kHz, "2k" 2 kHz,
/// "1.2k" 1.2 kHz), or a note name with an optional octave and cents ("A4", "d#5 +13", "Bb3 -20");
/// `None` when it is none of these.
fn parse_frequency(typed: &str) -> Option<f64> {
    let text = typed.trim().replace(',', ".");
    if text.is_empty() {
        return None;
    }
    if let Ok(number) = text.parse::<f64>() {
        return number.is_finite().then_some(number);
    }
    let lower = text.to_ascii_lowercase();
    if let Some((whole, fraction)) = lower.split_once('k')
        && !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit() || c == '.')
        && fraction.chars().all(|c| c.is_ascii_digit())
    {
        let whole: f64 = whole.parse().ok()?;
        let fraction = if fraction.is_empty() { 0.0 } else { format!("0.{fraction}").parse::<f64>().ok()? };
        return Some((whole + fraction) * 1000.0);
    }
    // A note: the letter, a sharp or flat, the octave (4 when it is left out), then cents.
    let mut chars = text.chars().peekable();
    let letter = chars.next()?.to_ascii_uppercase();
    let mut class = NOTE_NAMES.iter().position(|n| n.len() == 1 && n.starts_with(letter))? as i64;
    match chars.peek() {
        Some('#') => {
            class += 1;
            chars.next();
        }
        Some('b') => {
            class -= 1;
            chars.next();
        }
        _ => {}
    }
    let rest: String = chars.collect();
    let rest = rest.trim();
    // Cents alone ("A +13") keep octave 4.
    let (octave, cents) = if rest.starts_with(['+', '-']) {
        ("", rest)
    } else {
        match rest
            .find(['+', ' '])
            .filter(|&at| at > 0)
            .or_else(|| rest.get(1..).and_then(|r| r.find('-').map(|at| at + 1)))
        {
            Some(at) => (rest.get(..at)?.trim(), rest.get(at..)?.trim()),
            None => (rest, ""),
        }
    };
    let octave: i64 = if octave.is_empty() { 4 } else { octave.parse().ok()? };
    // A typed octave is hostile input: only octaves a frequency in range can have.
    if !(-1..=10).contains(&octave) {
        return None;
    }
    let cents: f64 = if cents.is_empty() { 0.0 } else { cents.trim_start_matches('+').trim().parse().ok()? };
    if !cents.is_finite() || cents.abs() > 1200.0 {
        return None;
    }
    let midi = (octave + 1) * 12 + class;
    Some(440.0 * 2f64.powf((midi as f64 - 69.0 + cents / 100.0) / 12.0))
}

/// A typed gain or Q: a number, the comma as a decimal point too.
fn parse_number(typed: &str) -> Option<f64> {
    typed.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The analyser's bands of this panel's device and the peak level of its output in dB, or `None`
/// while no meter monitor is in the frame's data or nothing has played through the device yet.
/// The panel claims its watch row here, so the engine starts appending as soon as the panel
/// shows, and asks for the next frame while the ring's appended count moves. The bands it
/// returns are the shown levels, gliding towards each frame's fresh reading with attack/release
/// ballistics, so the area flows with the music instead of flickering.
fn analyser_bands(
    ui: &mut egui::Ui,
    track: TrackId,
    device: usize,
    settings: SpectrumSettings,
    spectrum_shown: bool,
) -> Option<([f32; ANALYSER_BANDS], f32)> {
    let monitor = ui.ctx().data(|d| d.get_temp::<Arc<MeterMonitor>>(egui::Id::unique("eq-spectrum")))?;
    let spectrum = monitor.spectrum();
    // The watch's generation so the held levels know when the row's owner changed: a graph
    // swap resets the watch (and the ring with it).
    let watch_generation = spectrum.watch(device % spectrum::ROWS, track, device);
    let cursor = spectrum.appended(track, device).unwrap_or_default();
    let cursor_id = egui::Id::unique(("eq-spectrum-cursor", track.0, device));
    if cursor != ui.ctx().data(|d| d.get_temp::<usize>(cursor_id)).unwrap_or(0) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    ui.ctx().data_mut(|d| d.insert_temp(cursor_id, cursor));
    // The ring's reading is folded once for each new append, not once a frame per panel: while
    // nothing new came in (or the compact panel and the strip both ask), the last fold is reused.
    let folded_id = egui::Id::unique(("eq-spectrum-folded", track.0, device));
    let folded = ui.ctx().data(|d| d.get_temp::<(usize, u32, bool, Arc<[f32; ANALYSER_BANDS]>, f32)>(folded_id));
    let (bands, peak_db) = match folded {
        Some((at, generation, shown, bands, peak))
            if at == cursor && generation == watch_generation && shown == spectrum_shown =>
        {
            (*bands, peak)
        }
        _ => {
            let (bands, peak) = read_spectrum(ui, spectrum, track, device, spectrum_shown)?;
            ui.ctx().data_mut(|d| {
                d.insert_temp(folded_id, (cursor, watch_generation, spectrum_shown, Arc::new(bands), peak))
            });
            (bands, peak)
        }
    };
    if !spectrum_shown {
        return Some((bands, peak_db));
    }
    let shown_id = egui::Id::unique(("eq-spectrum-shown", track.0, device));
    let now = ui.input(|i| i.time);
    // The held levels belong to one monitor at one watch generation: a session switch reuses
    // the same keys, and a graph swap gives them a new owner and a reset ring.
    let owner: std::sync::Weak<suhina_engine::MeterMonitor> = Arc::downgrade(&monitor);
    let stored = ui.ctx().data(|d| {
        d.get_temp::<(std::sync::Weak<suhina_engine::MeterMonitor>, u32, [f32; ANALYSER_BANDS], f64)>(shown_id)
    });
    let fresh = stored.is_none();
    let (mut shown, last) = match stored {
        Some((kept, generation, shown, last)) if kept.ptr_eq(&owner) && generation == watch_generation => (shown, last),
        _ => (bands, now),
    };
    // The compact panel and the strip may both ask in one frame: the glide steps once.
    if now > last || fresh {
        let frame = 1.0 / (now - last).clamp(1.0e-3, 1.0);
        let follower = AttackRelease::new(ANALYSER_ATTACK, settings.release, frame);
        for (shown, target) in shown.iter_mut().zip(bands.iter()) {
            // Frozen, a band only rises: it holds its highest reading.
            *shown = if settings.freeze {
                shown.max(*target)
            } else {
                follower.rise(f64::from(*shown), f64::from(*target)).clamp(-130.0, 30.0) as f32
            };
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(shown_id, (owner, watch_generation, shown, now)));
    Some((shown, peak_db))
}

/// A Catmull-Rom curve through `points`, `steps` pieces between each two: the analyser's line
/// drawn smooth.
fn smooth(points: &[Pos2], steps: usize) -> Vec<Pos2> {
    if points.len() < 3 || steps < 2 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * steps);
    for i in 0..points.len() - 1 {
        let p0 = points[i.saturating_sub(1)];
        let (p1, p2) = (points[i], points[i + 1]);
        let p3 = points[(i + 2).min(points.len() - 1)];
        for step in 0..steps {
            let t = step as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let along = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push(Pos2::new(along(p0.x, p1.x, p2.x, p3.x), along(p0.y, p1.y, p2.y, p3.y)));
        }
    }
    if let Some(last) = points.last() {
        out.push(*last);
    }
    out
}

/// The ring's newest reading: the analyser's bands (none while it is off) and the meter's peak
/// in dB. The transform's window and plan are made once and kept.
fn read_spectrum(
    ui: &egui::Ui,
    spectrum: &suhina_engine::spectrum::Spectrum,
    track: TrackId,
    device: usize,
    spectrum_shown: bool,
) -> Option<([f32; ANALYSER_BANDS], f32)> {
    let (samples, _) = spectrum.samples(track, device)?;
    // The meter reads the newest tenth of the ring: its peak, in dB.
    let recent = samples.len().saturating_sub(RING / 10);
    let peak = samples.get(recent..).unwrap_or_default().iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let peak_db = (20.0 * f64::from(peak).max(1e-6).log10()) as f32;
    // With the analyser off only the meter is drawn: no transform, no fold.
    if !spectrum_shown {
        return Some(([ANALYSER_DB.1 as f32 * 2.0; ANALYSER_BANDS], peak_db));
    }
    let cache = egui::Id::unique("eq-spectrum-analyser");
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
    Some((bands, peak_db))
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
        let last = (hi * RING as f64 / rate).floor().clamp(0.0, (RING / 2) as f64) as usize;
        let mut peak = 0.0f32;
        if last >= first {
            // A band holding bins: the loudest of them.
            for binned in first..=last {
                if let Some(cell) = bins.get(binned) {
                    peak = peak.max(cell.norm());
                }
            }
        } else {
            // A band narrower than a bin (the low end): the level between the two bins around its
            // center, so neighboring bands glide instead of repeating one bin as a step.
            let center = ((lo * hi).sqrt() * RING as f64 / rate).clamp(1.0, (RING / 2 - 1) as f64);
            let below = center.floor() as usize;
            let t = (center - below as f64) as f32;
            let at = |i: usize| bins.get(i).map_or(0.0, |c| c.norm());
            peak = at(below) * (1.0 - t) + at(below + 1) * t;
        }
        let db = 20.0 * f64::from(2.0 * peak / window_sum).max(1e-9).log10();
        *bands_out = (db as f32).clamp(-130.0, 30.0);
    }
    bands
}

/// A small square button with a face drawn by `face`; returns its response.
fn small_button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    label: &str,
    lit: Option<Color32>,
    face: impl FnOnce(&egui::Painter, Rect, Color32),
) -> Response {
    let colors = super::device_canvas::colors();
    let response = ui.interact(rect, id, Sense::click()).on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let painter = ui.painter_at(rect.expand(1.0));
    let fill = match lit {
        Some(color) => color,
        None if response.hovered() => colors.control_active,
        None => colors.control_background,
    };
    painter.rect_filled(rect, 3.0, fill);
    let ink = if lit.is_some() { colors.text_inverse } else { colors.text_secondary };
    face(&painter, rect, ink);
    response
}

/// The icons the panel's buttons draw.
#[derive(Clone, Copy)]
enum Icon {
    Power,
    Delete,
    Previous,
    Next,
    Piano,
    Analyser,
    Pencil,
    Phones,
    Gear,
    Down,
    Sliders,
}

fn icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.35, color);
    match icon {
        Icon::Power => {
            // An arc open at the top, with the stem through the gap.
            let points: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = (-0.32 + 1.64 * i as f32 / 20.0) * std::f32::consts::PI;
                    c + Vec2::new(0.0, 0.5) + Vec2::new(a.cos(), a.sin()) * 4.1
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            painter.line_segment([c + Vec2::new(0.0, -4.8), c], stroke);
        }
        Icon::Delete => {
            painter.line_segment([c + Vec2::splat(-3.2), c + Vec2::splat(3.2)], stroke);
            painter.line_segment([c + Vec2::new(3.2, -3.2), c + Vec2::new(-3.2, 3.2)], stroke);
        }
        Icon::Previous => {
            painter.add(egui::Shape::line(
                vec![c + Vec2::new(2.0, -3.8), c + Vec2::new(-2.0, 0.0), c + Vec2::new(2.0, 3.8)],
                stroke,
            ));
        }
        Icon::Next => {
            painter.add(egui::Shape::line(
                vec![c + Vec2::new(-2.0, -3.8), c + Vec2::new(2.0, 0.0), c + Vec2::new(-2.0, 3.8)],
                stroke,
            ));
        }
        Icon::Piano => {
            let keys = Rect::from_center_size(c, Vec2::new(12.0, 7.6));
            painter.rect_stroke(keys, 0.0, stroke, egui::StrokeKind::Inside);
            for x in [-2.0, 2.0] {
                painter.line_segment([Pos2::new(c.x + x, keys.top()), Pos2::new(c.x + x, keys.bottom())], stroke);
            }
            for x in [-3.0, 1.0] {
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(c.x + x, keys.top()), Vec2::new(2.0, 4.2)),
                    0.0,
                    color,
                );
            }
        }
        Icon::Pencil => {
            painter.add(egui::Shape::closed_line(
                vec![
                    c + Vec2::new(-4.2, 4.2),
                    c + Vec2::new(-3.2, 1.4),
                    c + Vec2::new(2.4, -4.2),
                    c + Vec2::new(4.2, -2.4),
                    c + Vec2::new(-1.4, 3.2),
                ],
                stroke,
            ));
        }
        Icon::Phones => {
            let points: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 16.0;
                    c + Vec2::new(0.0, 1.0) + Vec2::new(a.cos(), a.sin()) * 4.4
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            for x in [-5.2, 3.1] {
                painter.rect_filled(Rect::from_min_size(c + Vec2::new(x, 0.5), Vec2::new(2.1, 3.8)), 0.0, color);
            }
        }
        Icon::Down => {
            painter.add(egui::Shape::convex_polygon(
                vec![c + Vec2::new(-3.0, -1.5), c + Vec2::new(3.0, -1.5), c + Vec2::new(0.0, 2.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Sliders => {
            for (i, at) in [-0.4f32, 0.3, -0.1].into_iter().enumerate() {
                let x = c.x - 4.0 + i as f32 * 4.0;
                painter.line_segment([Pos2::new(x, c.y - 5.0), Pos2::new(x, c.y + 5.0)], Stroke::new(1.0, color));
                painter.rect_filled(
                    Rect::from_center_size(Pos2::new(x, c.y + at * 8.0), Vec2::new(3.0, 2.0)),
                    0.0,
                    color,
                );
            }
        }
        Icon::Gear => {
            painter.circle_stroke(c, 2.2, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = Vec2::new(a.cos(), a.sin());
                painter.line_segment([c + d * 3.6, c + d * 5.4], stroke);
            }
        }
        Icon::Analyser => {
            for (dx, h) in [(-5.0, 3.0), (-2.0, 6.0), (1.0, 4.0), (4.0, 7.0)] {
                painter.rect_filled(
                    Rect::from_min_size(Pos2::new(c.x + dx, c.y + 4.0 - h), Vec2::new(2.0, h)),
                    0.0,
                    color,
                );
            }
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
        7 => &[(0.0, 0.9), (0.35, 0.9), (0.5, 0.5), (0.7, 0.1), (1.0, 0.05)],
        8 => &[(0.0, 0.2), (0.3, 0.2), (0.5, 0.5), (0.7, 0.8), (1.0, 0.8)],
        _ => &[(0.0, 0.1), (1.0, 0.9)],
    };
    let points: Vec<Pos2> = shape.iter().map(|(x, y)| at(*x, *y)).collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.2, color)));
}

/// The frame of the panel's menus.
fn menu_frame(colors: &super::device_canvas::Colors) -> egui::Frame {
    egui::Frame::new()
        .fill(colors.panel_background)
        .stroke(Stroke::new(1.0, colors.border))
        .corner_radius(4)
        .inner_margin(4)
}

/// The shape menu of band `slot`, opened from `response`: every type with its glyph.
fn shape_menu(panel: &mut Panel<'_>, response: &Response, slot: usize, band: Band) {
    let colors = super::device_canvas::colors();
    let mut picked = None;
    egui::Popup::menu(response).frame(menu_frame(&colors)).show(|ui| shape_items(ui, band.kind, &mut picked));
    if let Some(kind) = picked.filter(|kind| *kind != band.kind) {
        panel.edit(&[(slot, "Type", kind as f64)], "eq-curve-type");
    }
}

/// The shape menu's items: each type with its glyph, the band's own lit.
fn shape_items(ui: &mut egui::Ui, current: usize, picked: &mut Option<usize>) {
    let colors = super::device_canvas::colors();
    ui.spacing_mut().item_spacing.y = 1.0;
    ui.set_min_width(130.0);
    for (kind, name) in TYPE_NAMES.iter().enumerate() {
        let response = ui.selectable_label(kind == current, *name);
        glyph(
            ui.painter(),
            Rect::from_min_size(
                Pos2::new(ui.max_rect().right() - 18.0, response.rect.center().y - 4.5),
                Vec2::new(14.0, 9.0),
            ),
            kind,
            colors.text_value,
        );
        if response.clicked() {
            *picked = Some(kind);
        }
    }
}

/// The placement menu of band `slot`, opened from `response`: the five placements, then a split
/// into a left/right or a mid/side pair.
fn place_menu(panel: &mut Panel<'_>, response: &Response, slot: usize, band: Band) {
    let colors = super::device_canvas::colors();
    let mut picked = None;
    let mut split = None;
    let free = panel.bands.iter().any(|b| !b.in_use);
    egui::Popup::menu(response).frame(menu_frame(&colors)).show(|ui| {
        for (place, name) in PLACE_NAMES.iter().enumerate() {
            let text = egui::RichText::new(*name).color(place_color(place, &colors));
            if ui.selectable_label(place == band.place, text).clicked() {
                picked = Some(place);
            }
        }
        ui.separator();
        for (label, pair) in [("Split into L/R", (1usize, 2usize)), ("Split into M/S", (3, 4))] {
            if ui.add_enabled(free, egui::Button::new(label).frame(false)).clicked() {
                split = Some(pair);
            }
        }
    });
    if let Some(place) = picked.filter(|place| *place != band.place) {
        panel.edit(&[(slot, "Place", place as f64)], "eq-curve-place");
    }
    if let Some(pair) = split {
        split_band(panel, slot, band, pair);
    }
}

/// The dynamics' parameters, which a new band takes at their defaults and a split copies.
const DYNAMICS_FIELDS: [&str; 6] = ["Mode", "Range", "Threshold", "Attack", "Release", "Detail"];

/// The edits that put band `slot`'s mode and dynamics at their defaults, for a band made anew in
/// a slot that may have held a Dynamic or Spectral band before.
fn dynamics_defaults(panel: &Panel<'_>, slot: usize) -> Vec<(usize, &'static str, f64)> {
    DYNAMICS_FIELDS.iter().filter_map(|&field| panel.parameter(slot, field).map(|p| (slot, field, p.default))).collect()
}

/// Splits a band into a pair: it takes the first placement of `pair`, and a copy of it in the
/// first free slot takes the second, so the two sides can then be set apart. One undo step.
fn split_band(panel: &mut Panel<'_>, slot: usize, band: Band, pair: (usize, usize)) {
    let Some(copy) = panel.bands.iter().position(|b| !b.in_use) else { return };
    panel.edit(
        &[
            (slot, "Place", pair.0 as f64),
            (copy, "InUse", 1.0),
            (copy, "On", if band.on { 1.0 } else { 0.0 }),
            (copy, "Type", band.kind as f64),
            (copy, "Frequency", band.frequency),
            (copy, "Gain", band.gain),
            (copy, "Q", band.q),
            (copy, "Place", pair.1 as f64),
        ]
        .into_iter()
        // The copy has the band's mode and dynamics too: only the placements differ.
        .chain(DYNAMICS_FIELDS.iter().filter_map(|&field| panel.parameter(slot, field).map(|p| (copy, field, p.value))))
        .collect::<Vec<_>>(),
        "eq-curve-split",
    );
}

/// The menu of the bands in use, opened from the band number: picking one selects it.
fn band_menu(panel: &mut Panel<'_>, response: &Response) {
    let colors = super::device_canvas::colors();
    let mut picked = None;
    let listed: Vec<(usize, Band)> = panel.used().into_iter().filter_map(|s| Some((s, *panel.bands.get(s)?))).collect();
    let selected = panel.state.selected;
    egui::Popup::context_menu(response).frame(menu_frame(&colors)).show(|ui| {
        for (slot, band) in &listed {
            let text = format!(
                "{}. {} · {} · {}{}",
                slot + 1,
                TYPE_NAMES[band.kind],
                frequency_text(band.frequency),
                PLACE_LETTERS[band.place],
                if band.on { "" } else { " (off)" }
            );
            if ui.selectable_label(selected == Some(*slot), text).clicked() {
                picked = Some(*slot);
            }
        }
    });
    if let Some(slot) = picked {
        panel.select(slot);
    }
}

/// Where a value control draws its dial: at the left with the caption and value beside it (the
/// compact column), or on top with the caption and value under it (the floating panel).
#[derive(Clone, Copy, PartialEq)]
enum DialLayout {
    Beside,
    Above,
}

/// The band values a dial edits, in this order: the static three, then the dynamics.
const FIELDS: [&str; 8] = ["Frequency", "Gain", "Q", "Range", "Threshold", "Attack", "Release", "Detail"];
const CAPTIONS: [&str; 8] = ["Freq", "Gain", "Q", "Range", "Thresh", "Attack", "Release", "Detail"];
const RANGE: usize = 3;
const THRESHOLD: usize = 4;
const ATTACK: usize = 5;
const RELEASE: usize = 6;
const DETAIL: usize = 7;

/// A dial's value as the panel reads it.
fn field_text(field: usize, value: f64) -> String {
    match field {
        0 => frequency_text(value),
        1 | RANGE => gain_text(value),
        THRESHOLD => format!("{value:.0} dB"),
        ATTACK | RELEASE => {
            let ms = value * 1000.0;
            if ms < 10.0 { format!("{ms:.1} ms") } else { format!("{ms:.0} ms") }
        }
        DETAIL => format!("{:.0} %", value * 100.0),
        _ => format!("{value:.2}"),
    }
}

/// A typed value of a dial in its parameter's unit: a frequency (also a note or "k"), attack and
/// release in milliseconds, Detail in percent, anything else a number.
fn parse_field(field: usize, typed: &str) -> Option<f64> {
    match field {
        0 => parse_frequency(typed),
        ATTACK | RELEASE => parse_number(typed).map(|ms| ms / 1000.0),
        DETAIL => parse_number(typed).map(|percent| percent / 100.0),
        _ => parse_number(typed),
    }
}

/// One of the selected band's values: a dial with its caption and value. Dragging moves the
/// value (Shift for precision); a double click resets it; a click makes it the active value, so
/// the arrows step it and a typed value is set with Enter.
fn value_dial(ui: &mut egui::Ui, panel: &mut Panel<'_>, cell: Rect, slot: usize, field: usize, layout: DialLayout) {
    let Some(&field_name) = FIELDS.get(field) else { return };
    let Some(parameter) = panel.parameter(slot, field_name).cloned() else { return };
    let Some(band) = panel.bands.get(slot).copied() else { return };
    let dynamics = field >= RANGE;
    let inactive = !band.on
        || (field == 1 && !band.gain_editable())
        || (dynamics && band.mode == 0)
        || (field == DETAIL && band.mode != 2);
    let logarithmic = matches!(field, 0 | 2 | ATTACK | RELEASE);
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(cell);
    if layout == DialLayout::Beside {
        painter.rect_filled(cell, 3.0, colors.window_background.gamma_multiply(0.45));
    }
    let text = field_text(field, parameter.value);
    let caption = CAPTIONS.get(field).copied().unwrap_or_default();
    let (ink, value_ink) = if inactive {
        (colors.text_secondary.gamma_multiply(0.6), colors.text_secondary.gamma_multiply(0.6))
    } else {
        (colors.text_secondary, colors.text_value)
    };
    let (center, size) = match layout {
        DialLayout::Beside => {
            painter.text(
                Pos2::new(cell.left() + 27.0, cell.top() + 8.0),
                Align2::LEFT_CENTER,
                caption,
                theme::ui_font(9.5),
                ink,
            );
            painter.text(
                Pos2::new(cell.left() + 27.0, cell.top() + 19.5),
                Align2::LEFT_CENTER,
                text.clone(),
                theme::mono_font(9.5),
                value_ink,
            );
            (Pos2::new(cell.left() + 12.0, cell.center().y), 20.0)
        }
        DialLayout::Above => {
            let x = cell.center().x;
            painter.text(
                Pos2::new(x, cell.bottom() - 18.0),
                Align2::CENTER_CENTER,
                caption.to_uppercase(),
                theme::ui_font(9.0),
                ink,
            );
            painter.text(
                Pos2::new(x, cell.bottom() - 6.0),
                Align2::CENTER_CENTER,
                text.clone(),
                theme::mono_font(9.5),
                value_ink,
            );
            (
                Pos2::new(x, cell.top() + 22.0),
                match field {
                    1 => 34.0,
                    _ if dynamics => 26.0,
                    _ => 30.0,
                },
            )
        }
    };

    let response = ui
        .interact(
            cell,
            panel.address.ids.with(("eq-dial", slot, field, layout == DialLayout::Above)),
            Sense::click_and_drag(),
        )
        .on_hover_text(format!(
            "{}: drag; Shift for precision; double-click resets; click and type a value, then Enter{}.",
            parameter.name,
            if field == 0 { " (also a note, A4, or 6k3)" } else { "" }
        ))
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::SpinButton, true, &parameter.name);
        info.current_text_value = Some(text.clone());
        info
    });
    if response.clicked() || response.dragged() {
        panel.select(slot);
    }
    let normalized = |value: f64| {
        if logarithmic && parameter.min > 0.0 {
            ((value.max(parameter.min).ln() - parameter.min.ln()) / (parameter.max.ln() - parameter.min.ln()))
                .clamp(0.0, 1.0)
        } else if parameter.max > parameter.min {
            ((value - parameter.min) / (parameter.max - parameter.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let denormalized = |norm: f64| {
        if logarithmic && parameter.min > 0.0 {
            (parameter.min.ln() + norm * (parameter.max.ln() - parameter.min.ln())).exp()
        } else {
            parameter.min + norm * (parameter.max - parameter.min)
        }
    };
    let at = normalized(parameter.value);
    crate::kit::widgets::dial(&painter, center, size, at as f32, Some(&response));
    let held = response.is_pointer_button_down_on();
    // In listen mode, a band whose dial is held is heard alone.
    if held && panel.state.audition {
        panel.listen = Some(slot);
    }
    let state_id = response.id.with("start");
    let start = ui.ctx().data_mut(|d| {
        if held && d.get_temp::<f64>(state_id).is_none() {
            d.insert_temp(state_id, at);
        }
        d.get_temp::<f64>(state_id)
    });
    let fine: f64 = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
    // A click makes it the active value: the arrows step it, and what is typed is set with Enter
    // (a frequency also as a note or in "k" notation); Escape or a click elsewhere lets it go.
    let accept: fn(char) -> bool =
        if field == 0 { frequency_char } else { |c| c.is_ascii_digit() || matches!(c, '-' | '.' | ',') };
    let field_name = FIELDS.get(field).copied().unwrap_or("Frequency");
    let (keys, key_fine, typed) = crate::kit::widgets::value_keys_accepting(ui, &response, accept);
    let key_fine = if key_fine { 0.1 } else { 1.0 };
    let typed_value = typed.as_deref().and_then(|t| parse_field(field, t));
    let mut next = start.unwrap_or(at);
    if response.dragged() {
        let total = response.total_drag_delta().unwrap_or_default();
        next = (next - f64::from(total.y) * fine / crate::kit::widgets::PARAMETER_DRAG_PIXELS).clamp(0.0, 1.0);
    }
    next = (next + f64::from(keys) * 0.01 * key_fine).clamp(0.0, 1.0);
    if let Some(value) = typed_value {
        let value = value.clamp(parameter.min, parameter.max);
        if field == 1 {
            panel.widen_for(value);
        }
        panel.edit(&[(slot, field_name, value)], "eq-curve-typed");
    } else if response.drag_stopped() && !response.double_clicked() {
        // The release closes the gesture with the value the drag left.
        panel.edit(&[(slot, field_name, parameter.value)], "eq-curve-dial");
    } else if next != at || response.double_clicked() {
        let value = if response.double_clicked() { parameter.default } else { denormalized(next) };
        if field == 1 {
            panel.widen_for(value);
        }
        panel.edit(&[(slot, field_name, value)], "eq-curve-dial");
    }
    if !held {
        ui.ctx().data_mut(|d| d.remove::<f64>(state_id));
    }
}

/// The band's number in its placement's colour: filled while it is on, ringed while it is off.
fn band_number(painter: &egui::Painter, center: Pos2, radius: f32, slot: usize, band: Band, filled: bool) {
    let colors = super::device_canvas::colors();
    let color = band_color(slot);
    if band.on && filled {
        painter.circle_filled(center, radius, color);
    } else {
        painter.circle_filled(center, radius, colors.control_background);
        painter.circle_stroke(center, radius, Stroke::new(1.0, color));
    }
    let ink = if band.on && filled { colors.text_inverse } else { color };
    painter.text(
        center,
        Align2::CENTER_CENTER,
        (slot + 1).to_string(),
        theme::ui_font(if slot >= 9 { 8.0 } else { 9.5 }),
        ink,
    );
}

/// The band's number as a button: a click switches the band on or off, a right click lists the
/// bands in use to pick another.
#[allow(clippy::too_many_arguments)] // the panel, its painter, the place and the band
fn number_button(
    ui: &mut egui::Ui,
    panel: &mut Panel<'_>,
    painter: &egui::Painter,
    rect: Rect,
    id: egui::Id,
    slot: usize,
    band: Band,
    filled: bool,
) {
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_text("Click to switch the band on or off; right-click to pick another band");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Band {}", slot + 1)));
    band_number(painter, rect.center(), (rect.height() / 2.0 - 0.5).min(9.0), slot, band, filled);
    if response.clicked() {
        panel.edit(&[(slot, "On", if band.on { 0.0 } else { 1.0 })], "eq-curve-on");
    }
    band_menu(panel, &response);
}

/// The compact panel's selected band column (docs/spec/eq.md › The panel › The band column): the
/// band's number with previous and next, on/off and its shape, its placement, listen and delete,
/// its mode, then three dials — Frequency, Gain and Q, or its dynamics.
fn draw_column(ui: &mut egui::Ui, panel: &mut Panel<'_>, column: Rect) {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(column);
    painter.rect_filled(column, 3.0, colors.control_background.gamma_multiply(0.42));
    let Some((slot, band)) = panel.selected() else {
        painter.text(
            column.center() - Vec2::new(0.0, 8.0),
            Align2::CENTER_CENTER,
            "No band",
            theme::ui_font(10.0),
            colors.text_secondary,
        );
        painter.text(
            column.center() + Vec2::new(0.0, 8.0),
            Align2::CENTER_CENTER,
            "selected",
            theme::ui_font(10.0),
            colors.text_secondary,
        );
        return;
    };
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(column.min + Vec2::new(x, y), Vec2::new(w, h));
    let ids = panel.address.ids;
    // Row 1: the number (the band menu), previous and next.
    number_button(ui, panel, &painter, at(2.0, 2.0, 22.0, 18.0), ids.with("eq-number"), slot, band, true);
    let power_ink = if band.on { band_color(slot) } else { colors.text_secondary };
    if small_button(
        ui,
        at(26.0, 2.0, 20.0, 18.0),
        ids.with("eq-power"),
        "Switch the band on or off",
        None,
        |p, r, _| icon(p, r, Icon::Power, power_ink),
    )
    .clicked()
    {
        panel.edit(&[(slot, "On", if band.on { 0.0 } else { 1.0 })], "eq-curve-on");
    }
    if small_button(ui, at(50.0, 2.0, 19.0, 18.0), ids.with("eq-prev"), "Previous band", None, |p, r, c| {
        icon(p, r, Icon::Previous, c)
    })
    .clicked()
        && once(ui, panel.address.ids.with("once-prev"))
    {
        panel.step_selection(false);
    }
    if small_button(ui, at(71.0, 2.0, 19.0, 18.0), ids.with("eq-next"), "Next band", None, |p, r, c| {
        icon(p, r, Icon::Next, c)
    })
    .clicked()
        && once(ui, panel.address.ids.with("once-next"))
    {
        panel.step_selection(true);
    }
    // Row 2: the shape menu.
    let shape = small_button(
        ui,
        at(2.0, 22.0, 88.0, 18.0),
        ids.with("eq-shape"),
        &format!("Band {} type", slot + 1),
        None,
        |p, r, _| {
            glyph(
                p,
                Rect::from_min_size(r.min + Vec2::new(4.0, 4.5), Vec2::new(14.0, 9.0)),
                band.kind,
                colors.text_value,
            );
            p.text(
                r.left_center() + Vec2::new(22.0, 0.0),
                Align2::LEFT_CENTER,
                TYPE_NAMES.get(band.kind).copied().unwrap_or_default(),
                theme::ui_font(9.5),
                colors.text_primary,
            );
        },
    );
    shape_menu(panel, &shape, slot, band);
    // Row 3: placement, listen and delete.
    let place = small_button(
        ui,
        at(2.0, 42.0, 44.0, 18.0),
        ids.with("eq-place"),
        &format!("Band {} placement", slot + 1),
        None,
        |p, r, _| {
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                PLACE_NAMES[band.place],
                theme::ui_font(9.5),
                place_color(band.place, &colors),
            );
        },
    );
    place_menu(panel, &place, slot, band);
    listen_button(ui, panel, at(48.0, 42.0, 20.0, 18.0), ids.with("eq-listen"), slot);
    let delete =
        small_button(ui, at(70.0, 42.0, 20.0, 18.0), ids.with("eq-delete"), "Delete the band", None, |p, r, _| {
            icon(p, r, Icon::Delete, Color32::from_rgb(0xFF, 0x6B, 0x6B))
        });
    if delete.clicked() {
        panel.delete(slot);
        return;
    }
    // Row 4: the mode's menu, and the switch between the main dials and the dynamics' dials.
    mode_menu(ui, panel, at(2.0, 62.0, 64.0, 16.0), slot, band);
    dials_switch(ui, panel, at(68.0, 62.0, 22.0, 16.0), slot, band);
    // The dials: Frequency, Gain and Q, or the band's dynamics while they are open.
    let fields = if panel.state.dynamics && band.mode > 0 {
        [RANGE, THRESHOLD, if band.mode == 2 { DETAIL } else { ATTACK }]
    } else {
        [0, 1, 2]
    };
    for (row, field) in fields.into_iter().enumerate() {
        value_dial(ui, panel, at(2.0, 80.0 + row as f32 * 28.0, 88.0, 27.0), slot, field, DialLayout::Beside);
    }
}

/// The mode's menu button: the band's mode written out in its colour, and a menu of Static,
/// Dynamic and Spectral (a mode the type cannot take is dimmed) and whether the detector hears the
/// device's sidechain. Picking Dynamic or Spectral shows the band's dynamics.
fn mode_menu(ui: &mut egui::Ui, panel: &mut Panel<'_>, rect: Rect, slot: usize, band: Band) {
    let colors = super::device_canvas::colors();
    let ink = if band.mode > 0 { band_color(slot) } else { colors.text_secondary };
    let response = small_button(
        ui,
        rect,
        panel.address.ids.with(("eq-mode", slot, rect.min.x as i32)),
        &format!("Band {} mode", slot + 1),
        None,
        |p, r, _| {
            p.text(
                r.center() - Vec2::new(4.0, 0.0),
                Align2::CENTER_CENTER,
                MODE_NAMES[band.mode],
                theme::ui_font(9.5),
                ink,
            );
            icon(
                p,
                Rect::from_center_size(Pos2::new(r.right() - 7.0, r.center().y), Vec2::splat(8.0)),
                Icon::Down,
                colors.text_secondary,
            );
        },
    );
    let mut picked = None;
    let mut keyed = None;
    egui::Popup::menu(&response).frame(menu_frame(&colors)).show(|ui| {
        ui.set_min_width(180.0);
        for (mode, name) in MODE_NAMES.iter().enumerate() {
            let allowed = effective_mode(mode as f64, band.kind) == mode;
            if ui.add_enabled(allowed, egui::Button::selectable(band.mode == mode, *name)).clicked() {
                picked = Some(mode);
            }
        }
        ui.separator();
        let can = band.mode > 0;
        if ui
            .add_enabled(can, egui::Button::selectable(band.keyed, "Detector hears the sidechain"))
            .on_hover_text(
                "The band's detector hears the device's sidechain key (set in the header) instead of the band",
            )
            .clicked()
        {
            keyed = Some(!band.keyed);
        }
    });
    if let Some(mode) = picked {
        if mode != band.mode {
            panel.edit(&[(slot, "Mode", mode as f64)], "eq-curve-mode");
        }
        panel.state.dynamics = mode > 0;
    }
    if let Some(on) = keyed {
        panel.edit(&[(slot, "Trigger", if on { 1.0 } else { 0.0 })], "eq-curve-trigger");
    }
}

/// The switch between the band's main dials and its dynamics' dials, for a Dynamic or Spectral
/// band (dimmed for a Static one); lit while the dynamics show.
fn dials_switch(ui: &mut egui::Ui, panel: &mut Panel<'_>, rect: Rect, slot: usize, band: Band) {
    let colors = super::device_canvas::colors();
    let open = panel.state.dynamics && band.mode > 0;
    let lit = open.then_some(band_color(slot));
    let ink = if band.mode > 0 { colors.text_secondary } else { colors.text_secondary.gamma_multiply(0.35) };
    let wide = rect.width() > 40.0;
    let response = small_button(
        ui,
        rect,
        panel.address.ids.with(("eq-dials", slot, rect.min.x as i32)),
        &format!("Band {} dynamics", slot + 1),
        lit,
        |p, r, c| {
            let ink = if lit.is_some() { c } else { ink };
            if wide {
                p.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    if band.keyed { "Dynamics · SC" } else { "Dynamics" },
                    theme::ui_font(9.5),
                    ink,
                );
            } else {
                icon(p, r, Icon::Sliders, ink);
            }
        },
    )
    .on_hover_text(if open { "Show the band's frequency, gain and Q" } else { "Show the band's dynamics" });
    if response.clicked() && band.mode > 0 && once(ui, panel.address.ids.with("once-dials")) {
        panel.state.dynamics = !open;
    }
}

/// The listen button: the listen mode's switch, the same as the display's headphones. While it is
/// on, grabbing any band (its dot, or one of its dials) plays that band alone until it is let go.
fn listen_button(ui: &mut egui::Ui, panel: &mut Panel<'_>, rect: Rect, id: egui::Id, _slot: usize) {
    let colors = super::device_canvas::colors();
    let on = panel.state.audition;
    let lit = on.then_some(colors.accent);
    let label = if panel.can_listen {
        "Listen: while it is on, grabbing a band plays that band alone"
    } else {
        "A band of an EQ inside a group cannot be heard alone yet"
    };
    if small_button(ui, rect, id, label, lit, |p, r, c| icon(p, r, Icon::Phones, c)).clicked()
        && once(ui, panel.address.ids.with("once-listen"))
    {
        panel.state.audition = !on;
    }
}

/// The strip's floating band panel in the lane under the curve: in the middle of the display, or
/// under its band when the settings say it follows the band.
fn draw_floating_panel(ui: &mut egui::Ui, panel: &mut Panel<'_>, display: Rect) {
    let Some((slot, band)) = panel.selected() else { return };
    let colors = super::device_canvas::colors();
    let color = band_color(slot);
    let dynamics = panel.state.dynamics && band.mode > 0;
    let size = if dynamics { FLOATING + Vec2::new(FLOATING_DYNAMICS, 0.0) } else { FLOATING };
    let wanted = if panel.state.follow {
        frequency_x(band.frequency, display) - size.x / 2.0
    } else {
        display.center().x - size.x / 2.0
    };
    let x = wanted.clamp(display.left() + 10.0, (display.right() - size.x - 10.0).max(display.left() + 10.0));
    let rect = Rect::from_min_size(Pos2::new(x.round(), (display.bottom() - size.y - 10.0).round()), size);
    let painter = ui.painter_at(rect.expand(8.0));
    painter.rect_filled(rect.translate(Vec2::new(0.0, 3.0)).expand(2.0), 8.0, Color32::from_black_alpha(90));
    painter.rect_filled(rect, 8.0, colors.panel_background);
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, color.gamma_multiply(0.55)), egui::StrokeKind::Inside);
    // The panel takes the pointer, so a click on it does not reach the display under it.
    ui.interact(rect, panel.address.ids.with("eq-floating"), Sense::click());
    let at = |dx: f32, dy: f32, w: f32, h: f32| Rect::from_min_size(rect.min + Vec2::new(dx, dy), Vec2::new(w, h));
    let ids = panel.address.ids;
    // The band: its number (on/off), listen; its shape; its placement.
    number_button(ui, panel, &painter, at(8.0, 8.0, 28.0, 20.0), ids.with("eq-f-number"), slot, band, true);
    listen_button(ui, panel, at(40.0, 8.0, 60.0, 20.0), ids.with("eq-f-listen"), slot);
    let shape = small_button(
        ui,
        at(8.0, 33.0, 92.0, 20.0),
        ids.with("eq-f-shape"),
        &format!("Band {} type", slot + 1),
        None,
        |p, r, _| {
            glyph(
                p,
                Rect::from_min_size(r.min + Vec2::new(5.0, 5.5), Vec2::new(15.0, 9.0)),
                band.kind,
                colors.text_value,
            );
            p.text(
                r.left_center() + Vec2::new(25.0, 0.0),
                Align2::LEFT_CENTER,
                TYPE_NAMES[band.kind],
                theme::ui_font(10.0),
                colors.text_primary,
            );
        },
    );
    shape_menu(panel, &shape, slot, band);
    let place = small_button(
        ui,
        at(8.0, 57.0, 92.0, 20.0),
        ids.with("eq-f-place"),
        &format!("Band {} placement", slot + 1),
        None,
        |p, r, _| {
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                format!("● {}", PLACE_NAMES[band.place]),
                theme::ui_font(10.0),
                color,
            );
        },
    );
    place_menu(panel, &place, slot, band);
    // Frequency, Gain and Q.
    for (field, (dx, w)) in [(108.0, 66.0), (176.0, 72.0), (250.0, 62.0)].into_iter().enumerate() {
        value_dial(ui, panel, at(dx, 4.0, w, FLOATING.y - 8.0), slot, field, DialLayout::Above);
    }
    // The mode's menu and the dynamics' switch, then delete, previous and next.
    mode_menu(ui, panel, at(318.0, 8.0, 74.0, 21.0), slot, band);
    dials_switch(ui, panel, at(318.0, 32.0, 74.0, 21.0), slot, band);
    let right = FLOATING.x - 30.0;
    if small_button(ui, at(right, 8.0, 22.0, 21.0), ids.with("eq-f-delete"), "Delete the band", None, |p, r, _| {
        icon(p, r, Icon::Delete, Color32::from_rgb(0xFF, 0x6B, 0x6B))
    })
    .clicked()
    {
        panel.delete(slot);
        return;
    }
    if small_button(ui, at(right, 32.0, 22.0, 21.0), ids.with("eq-f-prev"), "Previous band", None, |p, r, c| {
        icon(p, r, Icon::Previous, c)
    })
    .clicked()
        && once(ui, panel.address.ids.with("once-prev"))
    {
        panel.step_selection(false);
    }
    if small_button(ui, at(right, 56.0, 22.0, 21.0), ids.with("eq-f-next"), "Next band", None, |p, r, c| {
        icon(p, r, Icon::Next, c)
    })
    .clicked()
        && once(ui, panel.address.ids.with("once-next"))
    {
        panel.step_selection(true);
    }
    // The dynamics, in a section of their own while they are open.
    if dynamics {
        painter.line_segment(
            [rect.min + Vec2::new(FLOATING.x + 2.0, 8.0), rect.min + Vec2::new(FLOATING.x + 2.0, FLOATING.y - 8.0)],
            Stroke::new(1.0, colors.border),
        );
        let fields =
            if band.mode == 2 { [RANGE, THRESHOLD, DETAIL, ATTACK] } else { [RANGE, THRESHOLD, ATTACK, RELEASE] };
        for (i, field) in fields.into_iter().enumerate() {
            let cell = at(FLOATING.x + 8.0 + i as f32 * 50.0, 4.0, 48.0, FLOATING.y - 8.0);
            value_dial(ui, panel, cell, slot, field, DialLayout::Above);
        }
    }
}

/// The piano keys along the bottom of the display, `height` tall.
fn draw_piano(painter: &egui::Painter, display: Rect, height: f32) {
    let top = display.bottom() - height;
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(display.left(), top), display.max),
        0.0,
        Color32::from_rgba_unmultiplied(0xD8, 0xDE, 0xE6, 224),
    );
    for note in 12..=127 {
        let class = note % 12;
        let frequency = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
        let left = frequency_x(frequency / 2f64.powf(1.0 / 24.0), display);
        let right = frequency_x(frequency * 2f64.powf(1.0 / 24.0), display);
        if right < display.left() || left > display.right() {
            continue;
        }
        if [1, 3, 6, 8, 10].contains(&class) {
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(left, top), Vec2::new(right - left, height * 0.62)),
                0.0,
                Color32::from_rgb(0x16, 0x1B, 0x22),
            );
        } else if class == 0 || class == 5 {
            painter.line_segment(
                [Pos2::new(left, top), Pos2::new(left, display.bottom())],
                Stroke::new(0.6, Color32::from_rgb(0x7A, 0x85, 0x94)),
            );
        }
    }
}

/// The display: its grids, the analyser's spectrum, the curves, the dots and the switches on it.
/// `lane` is the room under the curve left for the strip's floating panel; `big` is the strip.
fn draw_display(
    ui: &mut egui::Ui,
    panel: &mut Panel<'_>,
    display: Rect,
    lane: f32,
    big: bool,
    spectrum: Option<&[f32; ANALYSER_BANDS]>,
) {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(display);
    let range = panel.state.db_range;
    let piano = if panel.state.piano { if big { 16.0 } else { 10.0 } } else { 0.0 };
    // The curve's own area: inside the display, above the piano and the floating panel's lane.
    let plot = Rect::from_min_max(
        display.min + Vec2::new(0.0, if big { 20.0 } else { 4.0 }),
        Pos2::new(
            display.right(),
            (display.bottom() - piano - if big { 18.0 } else { 12.0 } - lane).max(display.top() + 40.0),
        ),
    );
    let background = ui
        .interact(display, panel.address.ids.with("eq-display"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::Crosshair);
    background.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "EQ response display"));
    // A press on the background itself (not on a dot or a button over it) starts a box or a drawn
    // stroke; both follow the pointer's own state, so the background stays a click target and
    // its double-click makes a band.
    let pressed_here = background.hovered() && ui.input(|i| i.pointer.primary_pressed());
    painter.rect_filled(display, 3.0, colors.window_background);
    // The frequency grid: a line at every decade and a fainter one between.
    for decade in 1..=4 {
        for step in 1..10 {
            let frequency = 10f64.powi(decade) * f64::from(step);
            if !(FREQUENCY.0..=FREQUENCY.1).contains(&frequency) {
                continue;
            }
            let x = frequency_x(frequency, display).round() + 0.5;
            let color = if step == 1 { colors.border } else { colors.border.gamma_multiply(0.45) };
            painter
                .line_segment([Pos2::new(x, display.top()), Pos2::new(x, display.bottom())], Stroke::new(1.0, color));
        }
    }
    let db_lines: &[f64] = match range as u32 {
        0..=6 => &[-6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0],
        7..=12 => &[-12.0, -9.0, -6.0, -3.0, 0.0, 3.0, 6.0, 9.0, 12.0],
        13..=15 => &[-15.0, -12.0, -6.0, 0.0, 6.0, 12.0, 15.0],
        _ => &[-30.0, -20.0, -10.0, 0.0, 10.0, 20.0, 30.0],
    };
    for &db in db_lines {
        let y = db_y(db, plot, range).round() + 0.5;
        let color = if db == 0.0 { colors.text_secondary } else { colors.border };
        painter.line_segment(
            [Pos2::new(display.left(), y), Pos2::new(display.right(), y)],
            Stroke::new(if db == 0.0 { 0.85 } else { 1.0 }, color),
        );
    }
    if lane > 0.0 {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(display.left(), plot.bottom() + 1.0), display.max),
            0.0,
            colors.panel_background.gamma_multiply(0.28),
        );
    }
    // The analyser's filled gray area behind the response curves, on its own vertical scale.
    if let Some(bands) = spectrum.filter(|_| panel.state.analyser) {
        let boundary: Vec<Pos2> = bands
            .iter()
            .enumerate()
            .map(|(band, &db)| {
                let center =
                    FREQUENCY.0 * (FREQUENCY.1 / FREQUENCY.0).powf((band as f64 + 0.5) / ANALYSER_BANDS as f64);
                let db = tilted(f64::from(db), center, panel.state.spectrum.tilt);
                Pos2::new(frequency_x(center, display), analyser_y_in(db, display, panel.state.spectrum.range))
            })
            .collect();
        // A smooth line through the bands' centers, not straight pieces between them.
        let boundary = smooth(&boundary, 4);
        let gray = analyser_gray(&colors);
        let mut mesh = egui::Mesh::default();
        for point in boundary.iter() {
            mesh.colored_vertex(*point, gray);
            mesh.colored_vertex(Pos2::new(point.x, display.bottom()), gray);
        }
        for index in 1..boundary.len() as u32 {
            let top = index * 2;
            mesh.add_triangle(top - 2, top - 1, top);
            mesh.add_triangle(top - 1, top, top + 1);
        }
        painter.add(egui::Shape::mesh(mesh));
        painter.add(egui::Shape::line(boundary, Stroke::new(1.0, gray)));
    }
    // The labels, over the analyser so they stay readable.
    for &db in db_lines {
        if db.abs() > range - 1.0 && !big {
            continue;
        }
        let text = if db > 0.0 { format!("+{db}") } else { format!("{db}") };
        let ink = if db == 0.0 { colors.text_primary } else { colors.text_secondary };
        if big {
            painter.text(
                Pos2::new(display.right() - 6.0, db_y(db, plot, range)),
                Align2::RIGHT_CENTER,
                text,
                theme::ui_font(10.0),
                ink,
            );
        } else {
            painter.text(
                Pos2::new(display.left() + 4.0, db_y(db, plot, range)),
                Align2::LEFT_CENTER,
                text,
                theme::ui_font(8.5),
                ink,
            );
        }
    }
    let labels: &[f64] = if big {
        &[20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0]
    } else {
        &[50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0]
    };
    for &frequency in labels {
        let text = if frequency >= 1000.0 { format!("{}k", frequency / 1000.0) } else { format!("{frequency}") };
        painter.text(
            Pos2::new(frequency_x(frequency, display), display.bottom() - piano - if big { 7.0 } else { 5.0 }),
            Align2::CENTER_CENTER,
            text,
            theme::ui_font(if big { 10.0 } else { 8.5 }),
            colors.text_secondary,
        );
    }
    if piano > 0.0 {
        draw_piano(&painter, display, piano);
    }

    // While a band is heard alone, the range it works on is shaded.
    if let Some((heard_slot, band)) = panel.state.listening.and_then(|slot| panel.bands.get(slot).map(|b| (slot, *b))) {
        let (low, high) = match band.kind {
            0 | 1 => (FREQUENCY.0, band.frequency),
            6 | 7 => (band.frequency, FREQUENCY.1),
            _ => {
                let q = if matches!(band.kind, 3 | 4) { band.q } else { suhina_device_eq::DEFAULT_Q };
                let octaves = 2.0 * (1.0 / (2.0 * q)).asinh() / std::f64::consts::LN_2;
                (band.frequency / 2f64.powf(octaves / 2.0), band.frequency * 2f64.powf(octaves / 2.0))
            }
        };
        let shade = Rect::from_x_y_ranges(frequency_x(low, display)..=frequency_x(high, display), display.y_range());
        painter.rect_filled(shade, 0.0, band_color(heard_slot).gamma_multiply(0.14));
    }
    // The curves, from each heard band's response computed once this frame and summed, drawn
    // only inside the curve's area: a curve past the display's range leaves it, it is not
    // flattened onto its edge.
    let curves = painter.with_clip_rect(plot.expand2(Vec2::new(0.0, 1.0)).intersect(display));
    let frequencies = grid();
    let cache = panel.address.ids.with("eq-responses");
    let responses: Vec<Option<Vec<f64>>> = cached_responses(ui, cache, &panel.bands, &frequencies);
    let scale = value(panel.parameters, "scale");
    let adaptive = value(panel.parameters, "adaptiveQ") >= 0.5;
    let to_points = |db: &[f64]| line_points(db, &frequencies, plot, range);
    // The selected band's own response, filled faintly from 0 dB.
    if let Some((slot, _)) = panel.selected().filter(|(_, band)| band.on)
        && let Some(Some(own)) = responses.get(slot)
    {
        let zero = vec![0.0; own.len()];
        fill_between(&curves, &to_points(own), &to_points(&zero), band_color(slot).gamma_multiply(0.13));
    }
    // A Dynamic or Spectral band's reach: the curve its side would have with the whole range
    // applied, faint, the area between filled, so the band's dynamics show before anything plays.
    for (slot, band) in panel.bands.iter().enumerate() {
        if !band.heard() || band.mode == 0 || band.range.abs() < 0.05 {
            continue;
        }
        let side = |b: &Band| b.place == 0 || b.place == band.place;
        let here = summed(&responses, &panel.bands, side);
        let reach = reached(band, &frequencies, scale, adaptive);
        let own = responses.get(slot).cloned().flatten().unwrap_or_else(|| vec![0.0; frequencies.len()]);
        let there: Vec<f64> = here.iter().zip(&own).zip(&reach).map(|((h, o), r)| h - o + r).collect();
        let tint = band_color(slot);
        let (a, b) = (to_points(&here), to_points(&there));
        fill_between(&curves, &a, &b, tint.gamma_multiply(0.07));
        curves.add(egui::Shape::line(b, Stroke::new(1.0, tint.gamma_multiply(0.4))));
    }
    // A curve for each placement in use, what that side hears: the stereo bands and its own.
    let selected_place = panel.selected().map(|(_, band)| band.place);
    for place in 1..PLACE_NAMES.len() {
        if !panel.bands.iter().any(|band| band.heard() && band.place == place) {
            continue;
        }
        let points = to_points(&summed(&responses, &panel.bands, |band| band.place == 0 || band.place == place));
        let active = selected_place == Some(place);
        let color = place_color(place, &colors).gamma_multiply(if active { 0.95 } else { 0.65 });
        curves.add(egui::Shape::line(points, Stroke::new(if active { 1.65 } else { 1.25 }, color)));
    }
    curves.add(egui::Shape::line(
        to_points(&summed(&responses, &panel.bands, |band| band.place == 0)),
        Stroke::new(if big { 1.9 } else { 1.55 }, colors.accent),
    ));
    // The live curve: each side as it is now, with every Dynamic band's gain change and every
    // Spectral band's deepest part, while one moves (docs/spec/eq.md › Dynamic and Spectral bands).
    if let Some(live) = panel.live {
        let moving: Vec<bool> = panel.bands.iter().zip(live).map(|(band, _)| band.heard() && band.mode > 0).collect();
        if moving.iter().any(|m| *m) {
            let mut now = responses.clone();
            // The first heard Spectral band shows every part's cut; another its deepest part's.
            let first_spectral = panel.bands.iter().position(|b| b.heard() && b.mode == 2);
            for (slot, band) in panel.bands.iter().enumerate() {
                if moving.get(slot).copied().unwrap_or(false)
                    && let (Some(value), Some(cell)) = (live.get(slot), now.get_mut(slot))
                {
                    let points = (Some(slot) == first_spectral).then_some(panel.live_parts).flatten();
                    *cell = Some(live_db(band, *value, points.as_ref().map(|p| &p[..]), &frequencies, scale, adaptive));
                }
            }
            for place in 0..PLACE_NAMES.len() {
                if !panel.bands.iter().zip(&moving).any(|(b, m)| *m && b.place == place) {
                    continue;
                }
                let side = |b: &Band| b.place == 0 || b.place == place;
                let tint = panel
                    .bands
                    .iter()
                    .zip(&moving)
                    .position(|(b, m)| *m && b.place == place)
                    .map_or(colors.accent, band_color);
                curves.add(egui::Shape::line(
                    to_points(&summed(&now, &panel.bands, side)),
                    Stroke::new(1.4, tint.gamma_multiply(0.9)),
                ));
            }
        }
    }

    // A double click on the background makes a band there; a click clears the selection; the
    // pointer over it previews the band a double click would make.
    let hovering_dot = draw_dots(ui, panel, display, plot, big);
    if let Some(pointer) = background.hover_pos().filter(|_| !hovering_dot) {
        painter.circle_stroke(pointer, 4.5, Stroke::new(1.0, colors.text_secondary.gamma_multiply(0.5)));
        if big {
            let hud = Rect::from_min_size(Pos2::new(display.left() + 8.0, plot.bottom() - 46.0), Vec2::new(78.0, 38.0));
            painter.rect_filled(hud, 4.0, colors.panel_background.gamma_multiply(0.92));
            painter.rect_stroke(
                hud,
                4.0,
                Stroke::new(1.0, colors.accent.gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            );
            let frequency = x_frequency(pointer.x, display);
            for (row, (text, ink)) in [
                (frequency_text(frequency), colors.accent),
                (note_text(frequency), colors.text_secondary),
                (gain_text(y_db(pointer.y, plot, range)), colors.text_primary),
            ]
            .into_iter()
            .enumerate()
            {
                painter.text(
                    Pos2::new(hud.center().x, hud.top() + 8.5 + row as f32 * 10.5),
                    Align2::CENTER_CENTER,
                    text,
                    theme::mono_font(8.5),
                    ink,
                );
            }
        }
    }
    if background.double_clicked()
        && let Some(pointer) = background.interact_pointer_pos()
    {
        add_band(panel, pointer, display, plot);
    } else if background.clicked() {
        panel.deselect();
    }
    // In draw mode a drag on the background draws a curve, and its release makes bands that
    // follow it (docs/spec/eq.md › The panel › Draw mode).
    if panel.state.draw {
        draw_stroke(ui, panel, pressed_here, &painter, display, plot);
    }
    // Otherwise a drag on the background draws a box; the bands whose dots it holds become the
    // selection (with Ctrl or Shift, they join it).
    let box_id = panel.address.ids.with("eq-box");
    if pressed_here
        && !panel.state.draw
        && let Some(from) = ui.input(|i| i.pointer.press_origin())
    {
        ui.ctx().data_mut(|d| d.insert_temp(box_id, from));
    }
    if let Some(from) = ui.ctx().data(|d| d.get_temp::<Pos2>(box_id)) {
        let to = ui.input(|i| i.pointer.interact_pos()).unwrap_or(from);
        let area = Rect::from_two_pos(from, to);
        painter.rect_filled(area, 0.0, colors.accent.gamma_multiply(0.08));
        painter.rect_stroke(area, 0.0, Stroke::new(1.0, colors.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        // A press and release in place is a click, not a box: the selection is the click's.
        if !ui.input(|i| i.pointer.primary_down()) && area.width() + area.height() > 4.0 {
            ui.ctx().data_mut(|d| d.remove::<Pos2>(box_id));
            let adding = adds_to_selection(ui);
            let mut group = if adding { panel.state.group } else { 0 };
            let scale = value(panel.parameters, "scale");
            let mut first = None;
            for (slot, band) in panel.bands.iter().enumerate().filter(|(_, b)| b.in_use) {
                if area.contains(dot_position(band, scale, display, plot, panel.state.db_range)) {
                    group |= 1 << slot;
                    first = first.or(Some(slot));
                }
            }
            panel.state.group = group;
            if !adding || panel.state.selected.is_none() {
                panel.state.selected = first;
            }
        } else if !ui.input(|i| i.pointer.primary_down()) {
            ui.ctx().data_mut(|d| d.remove::<Pos2>(box_id));
        }
    }

    // The switches on the display: the piano keys and the analyser at the top left, the dB range
    // at the top right.
    let ids = panel.address.ids;
    let (x, y) = if big {
        (display.left() + 6.0, display.top() + 4.0)
    } else {
        (display.left() + 4.0, display.bottom() - piano - 32.0)
    };
    let piano_on = panel.state.piano;
    if small_button(
        ui,
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(18.0, 16.0)),
        ids.with("eq-piano"),
        "Show the frequency axis as piano keys",
        piano_on.then_some(colors.accent),
        |p, r, c| icon(p, r, Icon::Piano, c),
    )
    .clicked()
        && once(ui, panel.address.ids.with("once-piano"))
    {
        panel.state.piano = !piano_on;
    }
    let analyser_on = panel.state.analyser;
    let analyser = small_button(
        ui,
        Rect::from_min_size(Pos2::new(x + 20.0, y), Vec2::new(18.0, 16.0)),
        ids.with("eq-analyser"),
        "Show the output's spectrum; right-click for its settings",
        analyser_on.then_some(colors.accent),
        |p, r, c| icon(p, r, Icon::Analyser, c),
    );
    if analyser.clicked() && once(ui, panel.address.ids.with("once-analyser")) {
        panel.state.analyser = !analyser_on;
    }
    let mut settings = panel.state.spectrum;
    analyser.context_menu(|ui| spectrum_menu(ui, &mut settings));
    panel.state.spectrum = settings;
    let draw_on = panel.state.draw;
    if small_button(
        ui,
        Rect::from_min_size(Pos2::new(x + 40.0, y), Vec2::new(18.0, 16.0)),
        ids.with("eq-draw"),
        "Draw a curve: a drag on the display makes bands that follow it",
        draw_on.then_some(colors.accent),
        |p, r, c| icon(p, r, Icon::Pencil, c),
    )
    .clicked()
        && once(ui, panel.address.ids.with("once-draw"))
    {
        panel.state.draw = !draw_on;
    }
    let audition_on = panel.state.audition;
    if small_button(
        ui,
        Rect::from_min_size(Pos2::new(x + 60.0, y), Vec2::new(18.0, 16.0)),
        ids.with("eq-audition"),
        "Audition: while it is on, holding a band's dot plays that band alone",
        audition_on.then_some(colors.accent),
        |p, r, c| icon(p, r, Icon::Phones, c),
    )
    .clicked()
        && once(ui, panel.address.ids.with("once-audition"))
    {
        panel.state.audition = !audition_on;
    }
    let settings_on = panel.state.settings;
    if small_button(
        ui,
        Rect::from_min_size(Pos2::new(x + 80.0, y), Vec2::new(18.0, 16.0)),
        ids.with("eq-settings"),
        "Settings",
        settings_on.then_some(colors.accent),
        |p, r, c| icon(p, r, Icon::Gear, c),
    )
    .clicked()
        && once(ui, panel.address.ids.with("once-settings"))
    {
        panel.state.settings = !settings_on;
    }
    let range_rect = Rect::from_min_size(
        Pos2::new(display.right() - if big { 62.0 } else { 38.0 }, display.top() + 4.0),
        Vec2::new(if big { 38.0 } else { 34.0 }, 15.0),
    );
    let range_button = small_button(
        ui,
        range_rect,
        ids.with("eq-range"),
        "The display's range: click for the next",
        None,
        |p, r, _| {
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                format!("±{range}"),
                theme::mono_font(8.5),
                Color32::from_rgb(0xF2, 0xA6, 0x5A),
            );
        },
    );
    if range_button.clicked() {
        let at = DB_RANGES.iter().position(|r| *r == range).unwrap_or(2);
        if once(ui, panel.address.ids.with("once-range")) {
            panel.state.db_range = DB_RANGES[(at + 1) % DB_RANGES.len()];
        }
    }
    if panel.state.settings {
        draw_settings(ui, panel, display);
    }
}

/// The frequencies the curves are evaluated at, [`POINTS`] steps across the display.
fn grid() -> Vec<f64> {
    (0..=POINTS).map(|i| FREQUENCY.0 * ((i as f64 / POINTS as f64) * (FREQUENCY.1 / FREQUENCY.0).ln()).exp()).collect()
}

/// A band's response in dB at each of `frequencies`, from its own filter sections.
fn band_db(band: &Band, frequencies: &[f64]) -> Vec<f64> {
    frequencies
        .iter()
        .map(|&frequency| {
            let magnitude: f64 =
                band.sections[..band.count].iter().map(|c| c.magnitude(DRAWN_RATE, frequency)).product();
            20.0 * magnitude.max(1.0e-6).log10()
        })
        .collect()
}

/// Each heard band's response, reused from the last frame for every band whose filter sections
/// did not change: while one dot is dragged, only that band's curve is computed again.
fn cached_responses(ui: &egui::Ui, id: egui::Id, bands: &[Band], frequencies: &[f64]) -> Vec<Option<Vec<f64>>> {
    let key = |band: &Band| -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        band.count.hash(&mut hasher);
        for c in &band.sections[..band.count] {
            format!("{c:?}").hash(&mut hasher);
        }
        hasher.finish()
    };
    let kept: Vec<Option<(u64, Arc<Vec<f64>>)>> = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    let fresh: Vec<Option<(u64, Arc<Vec<f64>>)>> = bands
        .iter()
        .enumerate()
        .map(|(slot, band)| {
            if !band.heard() {
                return None;
            }
            let k = key(band);
            match kept.get(slot).cloned().flatten() {
                Some((was, response)) if was == k => Some((k, response)),
                _ => Some((k, Arc::new(band_db(band, frequencies)))),
            }
        })
        .collect();
    let out = fresh.iter().map(|cell| cell.as_ref().map(|(_, r)| r.as_ref().clone())).collect();
    ui.ctx().data_mut(|d| d.insert_temp(id, fresh));
    out
}

/// The sum of the responses of the heard bands `pick` chooses.
fn summed(responses: &[Option<Vec<f64>>], bands: &[Band], pick: impl Fn(&Band) -> bool) -> Vec<f64> {
    let mut total = vec![0.0; POINTS + 1];
    for (response, band) in responses.iter().zip(bands) {
        if let Some(response) = response.as_ref().filter(|_| pick(band)) {
            for (t, r) in total.iter_mut().zip(response) {
                *t += r;
            }
        }
    }
    total
}

/// The points of a curve of `db` across `plot` at ± `range`; a level far past the range is held
/// a little outside the area (the drawing is clipped to it), never flattened onto its edge.
fn line_points(db: &[f64], frequencies: &[f64], plot: Rect, range: f64) -> Vec<Pos2> {
    db.iter()
        .zip(frequencies)
        .map(|(&level, &frequency)| {
            Pos2::new(frequency_x(frequency, plot), db_y(level.clamp(-range * 3.0, range * 3.0), plot, range))
        })
        .collect()
}

/// The area between two curves of the same points' x, filled.
fn fill_between(painter: &egui::Painter, a: &[Pos2], b: &[Pos2], color: Color32) {
    let mut mesh = egui::Mesh::default();
    for (p, q) in a.iter().zip(b) {
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(*q, color);
    }
    for index in 1..a.len().min(b.len()) as u32 {
        let top = index * 2;
        mesh.add_triangle(top - 2, top - 1, top);
        mesh.add_triangle(top - 1, top, top + 1);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// How much of a frequency belongs to a Spectral band, 0 to 1, as the device weighs its bins.
fn spectral_weight(band: &Band, frequency: f64) -> f64 {
    use suhina_dsp::spectral_dynamics::{Shape, shape_weight};
    let shape = match band.kind {
        2 => Shape::LowShelf,
        5 => Shape::HighShelf,
        _ => Shape::Bell,
    };
    shape_weight(shape, frequency, band.frequency, band.q)
}

/// A Dynamic or Spectral band's response with its whole range applied: a Dynamic band at its gain
/// plus its range; a Spectral band as it is, with its range across the frequencies it weighs
/// (Scale applied as the device applies it).
fn reached(band: &Band, frequencies: &[f64], scale: f64, adaptive: bool) -> Vec<f64> {
    if band.mode == 2 {
        return band_db(band, frequencies)
            .into_iter()
            .zip(frequencies)
            .map(|(own, &f)| own + band.range * scale * spectral_weight(band, f))
            .collect();
    }
    let mut moved = *band;
    (moved.sections, moved.count) =
        band_sections(moved.kind, DRAWN_RATE, moved.frequency, band.gain + band.range, moved.q, scale, adaptive);
    band_db(&moved, frequencies)
}

/// The live cut read at `frequency` from the device's points over `LIVE_RANGE`, between the two
/// points around it.
fn live_at(points: &[f32], frequency: f64) -> f64 {
    use suhina_dsp::spectral_dynamics::LIVE_RANGE;
    if points.len() < 2 {
        return 0.0;
    }
    let along = (frequency / LIVE_RANGE.0).ln() / (LIVE_RANGE.1 / LIVE_RANGE.0).ln() * (points.len() - 1) as f64;
    let along = along.clamp(0.0, (points.len() - 1) as f64);
    let below = along.floor() as usize;
    let t = along - below as f64;
    let at = |i: usize| f64::from(points.get(i.min(points.len() - 1)).copied().unwrap_or(0.0));
    at(below) * (1.0 - t) + at(below + 1) * t
}

/// A Dynamic or Spectral band's response now, from the readouts: a Dynamic band at its gain plus
/// its change; a Spectral band as it is with its live cut — every frequency's for the first one
/// (`points`), its deepest across the band for another.
fn live_db(
    band: &Band,
    value: f32,
    points: Option<&[f32]>,
    frequencies: &[f64],
    scale: f64,
    adaptive: bool,
) -> Vec<f64> {
    if band.mode == 2 {
        return band_db(band, frequencies)
            .into_iter()
            .zip(frequencies)
            .map(|(own, &f)| match points {
                Some(points) => own + live_at(points, f),
                None => own + f64::from(value) * spectral_weight(band, f),
            })
            .collect();
    }
    let mut moved = *band;
    (moved.sections, moved.count) =
        band_sections(moved.kind, DRAWN_RATE, moved.frequency, band.gain + f64::from(value), moved.q, scale, adaptive);
    band_db(&moved, frequencies)
}

/// The settings page, over the display while the gear is on: whether the strip's floating panel
/// follows its band, and the analyser's settings. Panel state, like the switches.
fn draw_settings(ui: &mut egui::Ui, panel: &mut Panel<'_>, display: Rect) {
    let colors = super::device_canvas::colors();
    let page = display.shrink(6.0);
    let painter = ui.painter_at(display);
    painter.rect_filled(page, 6.0, colors.panel_background);
    painter.rect_stroke(page, 6.0, Stroke::new(1.0, colors.border), egui::StrokeKind::Inside);
    ui.interact(page, panel.address.ids.with("eq-settings-page"), Sense::click());
    let ids = panel.address.ids.with("eq-settings");
    let mut y = page.top() + 10.0;
    let left = page.left() + 12.0;
    painter.text(
        Pos2::new(left, y + 7.0),
        Align2::LEFT_CENTER,
        "EQ settings",
        theme::ui_font(11.0),
        colors.text_primary,
    );
    let done = Rect::from_min_size(Pos2::new(page.right() - 64.0, y), Vec2::new(52.0, 18.0));
    if small_button(ui, done, ids.with("done"), "Back to the display", None, |p, r, c| {
        p.text(r.center(), Align2::CENTER_CENTER, "Done", theme::ui_font(10.0), c);
    })
    .clicked()
    {
        panel.state.settings = false;
    }
    y += 26.0;
    // A row: its caption, then its choices as buttons, the chosen one lit.
    let row = |ui: &mut egui::Ui, y: f32, caption: &str, choices: &[&str], chosen: usize| -> Option<usize> {
        painter.text(
            Pos2::new(left, y + 9.0),
            Align2::LEFT_CENTER,
            caption,
            theme::ui_font(10.0),
            colors.text_secondary,
        );
        let mut picked = None;
        for (i, name) in choices.iter().enumerate() {
            let rect = Rect::from_min_size(Pos2::new(left + 150.0 + i as f32 * 66.0, y), Vec2::new(62.0, 18.0));
            let lit = (i == chosen).then_some(colors.accent);
            if small_button(ui, rect, ids.with((caption, i)), &format!("{caption}: {name}"), lit, |p, r, c| {
                p.text(r.center(), Align2::CENTER_CENTER, *name, theme::ui_font(9.5), c);
            })
            .clicked()
            {
                picked = Some(i);
            }
        }
        picked
    };
    if let Some(i) = row(ui, y, "Floating panel", &["Centered", "Follows band"], usize::from(panel.state.follow)) {
        panel.state.follow = i == 1;
    }
    y += 24.0;
    let spectrum = panel.state.spectrum;
    let range_at = SPECTRUM_RANGES.iter().position(|r| *r == spectrum.range).unwrap_or(0);
    if let Some(i) = row(ui, y, "Analyser range", &["60 dB", "90 dB", "120 dB"], range_at) {
        panel.state.spectrum.range = SPECTRUM_RANGES[i];
    }
    y += 24.0;
    let fall_at = SPECTRUM_RELEASES.iter().position(|r| r.1 == spectrum.release).unwrap_or(1);
    if let Some(i) = row(ui, y, "Analyser fall", &["Fast", "Medium", "Slow"], fall_at) {
        panel.state.spectrum.release = SPECTRUM_RELEASES[i].1;
    }
    y += 24.0;
    let tilt_at = SPECTRUM_TILTS.iter().position(|t| *t == spectrum.tilt).unwrap_or(0);
    if let Some(i) = row(ui, y, "Analyser tilt", &["0 dB/oct", "3 dB/oct", "4.5 dB/oct"], tilt_at) {
        panel.state.spectrum.tilt = SPECTRUM_TILTS[i];
    }
    y += 24.0;
    if y + 18.0 < page.bottom()
        && let Some(i) = row(ui, y, "Analyser freeze", &["Off", "On"], usize::from(spectrum.freeze))
    {
        panel.state.spectrum.freeze = i == 1;
    }
}

/// The columns a drawn curve is kept in across the display: each holds the gain the stroke last
/// passed through it, so moving back within a stroke redraws the part it crosses again.
const STROKE_COLUMNS: usize = 128;
/// A drawn curve within this many dB of 0 dB is flat: no band is made there.
const STROKE_FLAT_DB: f64 = 1.0;
/// A drawn curve that falls this far at an end is a cut there rather than a shelf.
const STROKE_CUT_DB: f64 = -18.0;

/// The frequency at the middle of a drawn curve's column.
fn column_frequency(column: usize) -> f64 {
    FREQUENCY.0 * (FREQUENCY.1 / FREQUENCY.0).powf((column as f64 + 0.5) / STROKE_COLUMNS as f64)
}

/// The draw mode's stroke: kept per device while the drag lasts; on release it becomes bands.
fn draw_stroke(
    ui: &mut egui::Ui,
    panel: &mut Panel<'_>,
    started: bool,
    painter: &egui::Painter,
    display: Rect,
    plot: Rect,
) {
    let stroke_id = panel.address.ids.with("eq-stroke");
    let range = panel.state.db_range;
    if started {
        ui.ctx().data_mut(|d| d.insert_temp(stroke_id, vec![None::<f64>; STROKE_COLUMNS]));
    }
    let Some(mut columns) = ui.ctx().data(|d| d.get_temp::<Vec<Option<f64>>>(stroke_id)) else { return };
    let down = ui.input(|i| i.pointer.primary_down());
    if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()).filter(|_| down) {
        let along = ((pointer.x - display.left()) / display.width()).clamp(0.0, 0.999);
        let column = (along * STROKE_COLUMNS as f32) as usize;
        let gain = y_db(pointer.y, plot, range);
        // A fast drag skips columns between frames: every column from the last one to this one
        // takes the line between their gains.
        let last_id = stroke_id.with("last");
        let last = ui.ctx().data(|d| d.get_temp::<(usize, f64)>(last_id)).filter(|_| !started);
        let (from, from_gain) = last.unwrap_or((column, gain));
        let (low, high) = (from.min(column), from.max(column));
        for c in low..=high {
            let t = if high == low { 1.0 } else { (c as f64 - from as f64) / (column as f64 - from as f64) };
            if let Some(cell) = columns.get_mut(c) {
                *cell = Some(from_gain + (gain - from_gain) * t.clamp(0.0, 1.0));
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(last_id, (column, gain)));
        ui.ctx().data_mut(|d| d.insert_temp(stroke_id, columns.clone()));
    }
    let points: Vec<Pos2> = columns
        .iter()
        .enumerate()
        .filter_map(|(column, gain)| {
            gain.map(|g| Pos2::new(frequency_x(column_frequency(column), display), db_y(g, plot, range)))
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5, Color32::from_rgb(0xF2, 0xA6, 0x5A))));
    if !down {
        ui.ctx().data_mut(|d| d.remove::<Vec<Option<f64>>>(stroke_id));
        let free: Vec<usize> = panel.bands.iter().enumerate().filter(|(_, b)| !b.in_use).map(|(s, _)| s).collect();
        let mut edits: Vec<(usize, &str, f64)> = Vec::new();
        let mut made = Vec::new();
        for ((kind, frequency, gain, q), slot) in stroke_bands(&columns).into_iter().zip(free) {
            edits.extend([
                (slot, "InUse", 1.0),
                (slot, "On", 1.0),
                (slot, "Type", kind as f64),
                (slot, "Frequency", frequency),
                (slot, "Gain", gain),
                (slot, "Q", q),
                (slot, "Place", 0.0),
            ]);
            let defaults = dynamics_defaults(panel, slot);
            edits.extend(defaults);
            made.push(slot);
        }
        panel.edit(&edits, "eq-curve-draw");
        panel.state.group = made.iter().fold(0, |m, s| m | (1 << s));
        panel.state.selected = made.first().copied();
    }
}

/// The bands a drawn curve makes, `(type, frequency, gain, Q)`, low to high: each stretch of the
/// curve away from 0 dB (by more than [`STROKE_FLAT_DB`], one sign) is a band, finished where the
/// curve comes back to 0 dB. A stretch in the middle is a bell at its peak, its Q from the width
/// between its half-gain points; one that reaches the curve's start near the display's left edge
/// is a low shelf (or a 48 dB low cut once it falls past [`STROKE_CUT_DB`]), at its half-gain
/// point; the same at the right edge is a high shelf or cut.
fn stroke_bands(columns: &[Option<f64>]) -> Vec<(usize, f64, f64, f64)> {
    let filled: Vec<(usize, f64)> = columns.iter().enumerate().filter_map(|(c, g)| g.map(|g| (c, g))).collect();
    let (Some(&(first, _)), Some(&(last, _))) = (filled.first(), filled.last()) else { return Vec::new() };
    let octaves_per_column = (FREQUENCY.1 / FREQUENCY.0).log2() / STROKE_COLUMNS as f64;
    let mut bands = Vec::new();
    let mut at = 0;
    while at < filled.len() {
        let (_, gain) = filled[at];
        if gain.abs() < STROKE_FLAT_DB {
            at += 1;
            continue;
        }
        let sign = gain.signum();
        let start = at;
        while at < filled.len() && filled[at].1.abs() >= STROKE_FLAT_DB && filled[at].1.signum() == sign {
            at += 1;
        }
        let run = &filled[start..at];
        let Some(&(peak_column, peak)) = run.iter().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())) else { continue };
        let half = peak.abs() / 2.0;
        let left =
            run.iter().take_while(|(c, _)| *c < peak_column).filter(|(_, g)| g.abs() < half).map(|(c, _)| *c).last();
        let right = run.iter().skip_while(|(c, _)| *c <= peak_column).find(|(_, g)| g.abs() < half).map(|(c, _)| *c);
        let low_end = run.first().is_some_and(|(c, _)| *c == first) && first < STROKE_COLUMNS * 15 / 100;
        let high_end = run.last().is_some_and(|(c, _)| *c == last) && last > STROKE_COLUMNS * 85 / 100;
        if low_end && !high_end {
            // From the left edge until it comes back: a shelf at its half-gain point.
            let corner = right.unwrap_or(run.last().map_or(peak_column, |r| r.0));
            let gain = run.first().map_or(peak, |&r| r.1);
            if gain <= STROKE_CUT_DB {
                bands.push((0, column_frequency(corner), 0.0, suhina_device_eq::DEFAULT_Q));
            } else {
                bands.push((2, column_frequency(corner), gain.clamp(-30.0, 30.0), suhina_device_eq::DEFAULT_Q));
            }
        } else if high_end && !low_end {
            let corner = left.unwrap_or(run.first().map_or(peak_column, |r| r.0));
            let gain = run.last().map_or(peak, |&r| r.1);
            if gain <= STROKE_CUT_DB {
                bands.push((7, column_frequency(corner), 0.0, suhina_device_eq::DEFAULT_Q));
            } else {
                bands.push((5, column_frequency(corner), gain.clamp(-30.0, 30.0), suhina_device_eq::DEFAULT_Q));
            }
        } else {
            let from = left.unwrap_or(run.first().map_or(peak_column, |r| r.0));
            let to = right.unwrap_or(run.last().map_or(peak_column, |r| r.0));
            let width = ((to.saturating_sub(from)) as f64 + 1.0) * octaves_per_column;
            let ratio = 2f64.powf(width.max(0.05));
            let q = (ratio.sqrt() / (ratio - 1.0)).clamp(0.3, 12.0);
            bands.push((3, column_frequency(peak_column), peak.clamp(-30.0, 30.0), q));
        }
    }
    bands
}

/// Whether this frame's click adds to the selection: Ctrl (Cmd) or Shift held, as the window's
/// modifiers say or as the press itself carried them (a press and its release can come in one
/// frame with the keys already let go).
fn adds_to_selection(ui: &egui::Ui) -> bool {
    ui.input(|i| {
        i.modifiers.command
            || i.modifiers.shift
            || i.events.iter().any(|event| {
                matches!(event, egui::Event::PointerButton { modifiers, .. } if modifiers.command || modifiers.shift)
            })
    })
}

/// The order the dots are drawn in, bottom to top: the bands that are off, then those that are on,
/// each by slot, and the selected band last.
fn dot_order(panel: &Panel<'_>) -> Vec<usize> {
    let rank = |slot: usize| {
        let on = panel.bands.get(slot).is_some_and(|band| band.on);
        (panel.state.selected == Some(slot), on)
    };
    let mut slots: Vec<usize> = (0..panel.bands.len()).collect();
    slots.sort_by_key(|&slot| rank(slot));
    slots
}

/// Where a band's dot is drawn: at its frequency and its gain after Scale (on the 0 dB line for a
/// type without gain), kept inside the display.
fn dot_position(band: &Band, scale: f64, display: Rect, plot: Rect, range: f64) -> Pos2 {
    let gain = if band.gain_editable() { band.gain * scale } else { 0.0 };
    Pos2::new(
        frequency_x(band.frequency, display).clamp(display.left() + 9.0, display.right() - 9.0),
        db_y(gain, plot, range).clamp(plot.top(), plot.bottom()),
    )
}

/// The analyser's settings menu: range, fall, tilt and freeze.
fn spectrum_menu(ui: &mut egui::Ui, settings: &mut SpectrumSettings) {
    ui.set_min_width(140.0);
    ui.label("Range");
    for range in SPECTRUM_RANGES {
        if ui.selectable_label(settings.range == range, format!("{range} dB")).clicked() {
            settings.range = range;
        }
    }
    ui.separator();
    ui.label("Fall");
    for (name, release) in SPECTRUM_RELEASES {
        if ui.selectable_label(settings.release == release, name).clicked() {
            settings.release = release;
        }
    }
    ui.separator();
    ui.label("Tilt");
    for tilt in SPECTRUM_TILTS {
        if ui.selectable_label(settings.tilt == tilt, format!("{tilt} dB/oct")).clicked() {
            settings.tilt = tilt;
        }
    }
    ui.separator();
    if ui.selectable_label(settings.freeze, "Freeze").clicked() {
        settings.freeze = !settings.freeze;
    }
}

/// A new band at the pointer, in the first free slot: at the far left a low cut, at the far right
/// a high cut, elsewhere a bell at the pointer's gain.
fn add_band(panel: &mut Panel<'_>, pointer: Pos2, display: Rect, plot: Rect) {
    let Some(slot) = panel.bands.iter().position(|band| !band.in_use) else { return };
    let along = (pointer.x - display.left()) / display.width();
    let frequency = x_frequency(pointer.x, display);
    let gain = y_db(pointer.y, plot, panel.state.db_range);
    let (kind, gain) = if along < 0.06 {
        (0.0, 0.0)
    } else if along > 0.94 {
        (7.0, 0.0)
    } else {
        (3.0, (gain * 10.0).round() / 10.0)
    };
    // A slot freed by a delete keeps its old values: the new band takes the defaults of its mode
    // and dynamics, so it is Static whatever the slot held before.
    let mut edits = vec![
        (slot, "InUse", 1.0),
        (slot, "On", 1.0),
        (slot, "Type", kind),
        (slot, "Frequency", frequency),
        (slot, "Gain", gain),
        (slot, "Q", suhina_device_eq::DEFAULT_Q),
        (slot, "Place", 0.0),
    ];
    edits.extend(dynamics_defaults(panel, slot));
    panel.edit(&edits, "eq-curve-add");
    panel.select(slot);
}

/// The bands' dots: a numbered circle at each band's frequency and gain in its placement's
/// colour. Clicking or dragging selects the band; dragging moves its frequency (and its gain
/// where its type has one); a double click switches it on or off; the wheel sets its Q (Shift
/// for finer steps); a right click opens its shape menu. Returns whether the pointer is on one.
fn draw_dots(ui: &mut egui::Ui, panel: &mut Panel<'_>, display: Rect, plot: Rect, big: bool) -> bool {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(display);
    let range = panel.state.db_range;
    let radius = if big { 7.5 } else { 5.8 };
    let mut hovered = None;
    // Drawn bottom to top: the bands that are off, then those that are on, the selected band last.
    // The dot drawn on top takes the click, so of two dots in one place (two bands at the same
    // frequency and gain) the one that is heard, or selected, is the one a click picks.
    for slot in dot_order(panel) {
        let Some(band) = panel.bands.get(slot).copied().filter(|band| band.in_use) else { continue };
        let at = dot_position(&band, value(panel.parameters, "scale"), display, plot, range);
        let id = panel.address.ids.with(("eq-dot", slot));
        let caption = format!(
            "Band {}: {} at {}, {}, Q {:.2}, {}",
            slot + 1,
            TYPE_NAMES[band.kind],
            frequency_text(band.frequency),
            gain_text(band.gain),
            band.q,
            PLACE_NAMES[band.place]
        );
        let response = ui
            .interact(Rect::from_center_size(at, Vec2::splat(22.0)), id, Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Move);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, &caption));
        let selected = panel.in_group(slot);
        let color = band_color(slot);
        let (fill, ring, ink) = match (selected, band.on) {
            (true, true) => (color, color, colors.text_inverse),
            (true, false) => (colors.control_active, colors.text_secondary, colors.text_secondary),
            (false, true) => (colors.panel_background, color, color),
            (false, false) => (colors.panel_background, colors.text_secondary, colors.text_secondary),
        };
        painter.circle_filled(at, radius, fill);
        if band.on {
            painter.circle_stroke(at, radius, Stroke::new(if selected { 2.0 } else { 1.5 }, ring));
        } else {
            // Dashed while the band is off.
            let dashes: Vec<Pos2> =
                (0..=24).map(|i| at + Vec2::angled(i as f32 / 24.0 * std::f32::consts::TAU) * radius).collect();
            painter.extend(egui::Shape::dashed_line(&dashes, Stroke::new(1.25, ring), 2.5, 2.0));
        }
        painter.text(
            at + Vec2::new(0.0, 0.5),
            Align2::CENTER_CENTER,
            (slot + 1).to_string(),
            theme::ui_font(if big {
                9.5
            } else if slot >= 9 {
                7.0
            } else {
                8.0
            }),
            ink,
        );
        // A Dynamic or Spectral band's range: a line from the dot to where its gain may go, with
        // an arrow there, and the gain change now as a thicker bar along it.
        if band.mode > 0 && band.on && band.range.abs() > 0.05 {
            let tint = band_color(slot);
            let scale = value(panel.parameters, "scale");
            let base = if band.gain_editable() { band.gain * scale } else { 0.0 };
            let end = Pos2::new(
                at.x,
                db_y(base + band.range * scale, plot, range).clamp(plot.top() - 4.0, plot.bottom() + 4.0),
            );
            let direction = if end.y >= at.y { 1.0 } else { -1.0 };
            painter.add(egui::Shape::convex_polygon(
                vec![
                    end + Vec2::new(0.0, direction * 2.5),
                    end + Vec2::new(-3.8, -direction * 2.2),
                    end + Vec2::new(3.8, -direction * 2.2),
                ],
                tint,
                Stroke::NONE,
            ));
            // The arrow is a handle: dragged up or down it sets the band's range, from where it
            // was when the drag began, at the display's dB per point then (one undo step).
            let handle = ui
                .interact(Rect::from_center_size(end, Vec2::new(18.0, 14.0)), id.with("range"), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeVertical)
                .on_hover_text(format!("Band {} range: drag up or down", slot + 1));
            handle.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Slider, true, format!("Band {} range handle", slot + 1))
            });
            let grab_id = id.with("range-grab");
            if handle.drag_started() {
                let per_point = 2.0 * range / f64::from(plot.height()) / scale.abs().max(0.01);
                ui.ctx().data_mut(|d| d.insert_temp(grab_id, [band.range, per_point]));
            }
            if let Some([from, per_point]) = ui.ctx().data(|d| d.get_temp::<[f64; 2]>(grab_id)) {
                if handle.dragged() {
                    let moved = -f64::from(handle.total_drag_delta().unwrap_or_default().y) * per_point;
                    panel.edit(&[(slot, "Range", (from + moved).clamp(-30.0, 30.0))], "eq-curve-range");
                } else if handle.drag_stopped() {
                    panel.edit(&[(slot, "Range", band.range)], "eq-curve-range");
                    ui.ctx().data_mut(|d| d.remove::<[f64; 2]>(grab_id));
                }
            }
        }
        // A click selects the band alone; with Ctrl or Shift it joins the selection or leaves it.
        // A drag on a band already selected keeps the others, so they move together.
        let adding = adds_to_selection(ui);
        if response.clicked() && adding {
            if once(ui, panel.address.ids.with(("once-group", slot))) {
                panel.state.group ^= 1 << slot;
            }
            panel.state.selected = if panel.in_group(slot) { Some(slot) } else { panel.group().first().copied() };
        } else if response.clicked()
            || response.secondary_clicked()
            || (response.drag_started() && !panel.in_group(slot))
        {
            panel.select(slot);
        } else if response.drag_started() {
            panel.state.selected = Some(slot);
        }
        if response.hovered() || response.dragged() {
            hovered = Some((slot, at));
        }
        response.context_menu(|ui| {
            let mut picked = None;
            shape_items(ui, band.kind, &mut picked);
            if let Some(kind) = picked {
                ui.close();
                if kind != band.kind {
                    panel.edit(&[(slot, "Type", kind as f64)], "eq-curve-type");
                }
            }
        });
        if response.double_clicked() {
            panel.edit(&[(slot, "On", if band.on { 0.0 } else { 1.0 })], "eq-curve-on");
            continue;
        }
        // The wheel over a dot sets the band's Q and does not scroll the panel.
        if response.hovered() {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
                if ui.input(|i| i.modifiers.alt) {
                    // Alt and the wheel set the dynamic range, making a Static band Dynamic.
                    let range = (band.range + if wheel > 0.0 { 0.5 } else { -0.5 }).clamp(-30.0, 30.0);
                    let mut edits = vec![(slot, "Range", range)];
                    if band.mode == 0 && has_gain(band.kind) {
                        edits.insert(0, (slot, "Mode", 1.0));
                    }
                    panel.edit(&edits, "eq-curve-range");
                } else {
                    let step = if ui.input(|i| i.modifiers.shift) { 1.03 } else { 1.12 };
                    let q = (band.q * if wheel > 0.0 { step } else { 1.0 / step }).clamp(0.1, 18.0);
                    panel.edit(&[(slot, "Q", q)], "eq-curve-q");
                }
            }
        }
        let state_id = id.with("drag");
        let held = response.is_pointer_button_down_on();
        if held && panel.state.audition {
            panel.listen = Some(slot);
        }
        let start = ui.ctx().data_mut(|d| {
            // The drag's start and its dB per point, fixed for the whole drag: the display may widen
            // as the gain passes its range, and the gain under a still pointer must not jump.
            if held && d.get_temp::<[f64; 3]>(state_id).is_none() {
                d.insert_temp(
                    state_id,
                    [
                        f64::from(frequency_x(band.frequency, display)),
                        band.gain,
                        2.0 * range / f64::from(plot.height()),
                    ],
                );
            }
            d.get_temp::<[f64; 3]>(state_id)
        });
        let fine = if ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
        // The other selected bands' values as the drag began, to move them by the same steps.
        let group_id = panel.address.ids.with("eq-group-drag");
        if response.drag_started() {
            let others: Vec<(usize, f64, f64)> = panel
                .group()
                .into_iter()
                .filter(|&s| s != slot)
                .filter_map(|s| panel.bands.get(s).map(|b| (s, b.frequency, b.gain)))
                .collect();
            ui.ctx().data_mut(|d| d.insert_temp(group_id, others));
        }
        let others = ui.ctx().data(|d| d.get_temp::<Vec<(usize, f64, f64)>>(group_id)).unwrap_or_default();
        if let Some([start_x, start_gain, db_per_point]) = start {
            let start_x = start_x as f32;
            let mut edits: Vec<(usize, &str, f64)> = Vec::new();
            if response.dragged() && !others.is_empty() {
                // Every selected band moves by the same octaves and decibels as this one.
                let total = response.total_drag_delta().unwrap_or_default();
                let start_frequency = x_frequency(start_x, display);
                let octaves = (x_frequency(start_x + total.x * fine, display) / start_frequency).log2();
                let decibels = -f64::from(total.y * fine) * db_per_point;
                let mut moved = vec![(slot, start_frequency, start_gain)];
                moved.extend(others.iter().copied());
                for (s, frequency, gain) in moved {
                    let Some(b) = panel.bands.get(s).copied() else { continue };
                    edits.push((s, "Frequency", (frequency * 2f64.powf(octaves)).clamp(FREQUENCY.0, FREQUENCY.1)));
                    if b.gain_editable() {
                        let gain = (gain + decibels).clamp(-30.0, 30.0);
                        panel.widen_for(gain);
                        edits.push((s, "Gain", gain));
                    }
                }
            } else if response.dragged() {
                let total = response.total_drag_delta().unwrap_or_default();
                let frequency = x_frequency(start_x + total.x * fine, display);
                if frequency != band.frequency {
                    edits.push((slot, "Frequency", frequency));
                }
                if band.gain_editable() {
                    // Past the display's edge the gain goes on to the parameter's range, and the
                    // display widens to show it.
                    let gain = (start_gain - f64::from(total.y * fine) * db_per_point).clamp(-30.0, 30.0);
                    if gain != band.gain {
                        edits.push((slot, "Gain", gain));
                        panel.widen_for(gain);
                    }
                }
                // A band dragged while it is off comes on as it moves.
                if !edits.is_empty() && !band.on {
                    edits.insert(0, (slot, "On", 1.0));
                }
            } else if response.drag_stopped() {
                // The drag ends with the values as they are, so the gesture closes even when the
                // pointer came back to where it started.
                for s in std::iter::once(slot).chain(others.iter().map(|o| o.0)) {
                    let Some(b) = panel.bands.get(s).copied() else { continue };
                    edits.push((s, "Frequency", b.frequency));
                    if b.gain_editable() {
                        edits.push((s, "Gain", b.gain));
                    }
                }
            }
            panel.edit(&edits, "eq-curve-dot");
        }
        if !held {
            ui.ctx().data_mut(|d| d.remove::<[f64; 3]>(state_id));
            if response.drag_stopped() {
                ui.ctx().data_mut(|d| d.remove::<Vec<(usize, f64, f64)>>(group_id));
            }
        }
    }
    // The hovered or dragged band's values beside its dot.
    if let Some((slot, at)) = hovered
        && let Some(band) = panel.bands.get(slot).copied()
    {
        let color = band_color(slot);
        let gain = if band.gain_editable() { format!("{}  ", gain_text(band.gain)) } else { String::new() };
        let text = if big {
            format!(
                "{}  {}\n{}Q {:.2} · {}",
                frequency_text(band.frequency),
                note_text(band.frequency),
                gain,
                band.q,
                PLACE_NAMES[band.place]
            )
        } else {
            let note = note_text(band.frequency);
            format!(
                "{} ({})  {}Q {:.2}",
                frequency_text(band.frequency),
                note.split(' ').next().unwrap_or_default(),
                gain,
                band.q
            )
        };
        let galley = painter.layout(text, theme::mono_font(8.5), colors.text_primary, f32::INFINITY);
        let size = galley.size() + Vec2::new(10.0, 4.0);
        let mut min = Pos2::new(at.x - size.x / 2.0, at.y - radius - size.y - 5.0);
        if min.y < display.top() + 4.0 {
            min.y = at.y + radius + 5.0;
        }
        min.x = min.x.clamp(display.left() + 4.0, (display.right() - size.x - 4.0).max(display.left() + 4.0));
        let pill = Rect::from_min_size(min.round(), size);
        painter.rect_filled(pill, 3.0, colors.panel_background.gamma_multiply(0.94));
        painter.rect_stroke(pill, 3.0, Stroke::new(1.0, color.gamma_multiply(0.7)), egui::StrokeKind::Inside);
        painter.galley(pill.min + Vec2::new(5.0, 2.0), galley, colors.text_primary);
    }
    hovered.is_some()
}

/// The output meter: the peak of the device's output from the analyser's ring, green to red at
/// the top, with a tick at the level.
fn draw_meter(ui: &mut egui::Ui, rect: Rect, peak_db: Option<f32>) {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, colors.window_background);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, colors.border), egui::StrokeKind::Inside);
    let Some(peak) = peak_db else { return };
    let level = ((peak + 48.0) / 48.0).clamp(0.0, 1.0);
    let inner = rect.shrink(1.5);
    let top = inner.bottom() - inner.height() * level;
    let color = if level > 0.96 {
        Color32::from_rgb(0xFF, 0x6B, 0x6B)
    } else if level > 0.8 {
        Color32::from_rgb(0xF2, 0xC9, 0x4C)
    } else {
        Color32::from_rgb(0x54, 0xC7, 0x9B)
    };
    painter.rect_filled(Rect::from_min_max(Pos2::new(inner.left(), top), inner.max), 0.0, color);
}

/// The band list the compact panel shows while the strip is open: a row for every band in use,
/// its values written out. A click on a row selects its band; the On cell switches it, the Shape
/// and Stereo cells open their menus, and the Freq, Gain and Q cells drag.
fn draw_band_list(ui: &mut egui::Ui, panel: &mut Panel<'_>, list: Rect) {
    let colors = super::device_canvas::colors();
    let painter = ui.painter_at(list);
    painter.rect_filled(list, 3.0, colors.window_background);
    painter.rect_stroke(list, 3.0, Stroke::new(1.0, colors.border), egui::StrokeKind::Inside);
    // The columns: [x, width] at the list's width of 480, scaled to the list.
    let columns: [(&str, f32, f32, Align2); 7] = [
        ("#", 0.0, 32.0, Align2::CENTER_CENTER),
        ("Shape", 34.0, 132.0, Align2::LEFT_CENTER),
        ("Freq", 166.0, 84.0, Align2::RIGHT_CENTER),
        ("Gain", 250.0, 72.0, Align2::RIGHT_CENTER),
        ("Q", 322.0, 54.0, Align2::RIGHT_CENTER),
        ("Stereo", 376.0, 52.0, Align2::CENTER_CENTER),
        ("Mode", 428.0, 52.0, Align2::CENTER_CENTER),
    ];
    let k = list.width() / 480.0;
    let cell = |column: usize, y: f32| {
        let (_, x, w, _) = columns[column];
        Rect::from_min_size(Pos2::new(list.left() + x * k, y), Vec2::new(w * k, 19.0))
    };
    let anchor = |rect: Rect, align: Align2| match align {
        Align2::LEFT_CENTER => rect.left_center() + Vec2::new(4.0, 0.0),
        Align2::RIGHT_CENTER => rect.right_center() - Vec2::new(4.0, 0.0),
        _ => rect.center(),
    };
    for (index, (name, _, _, align)) in columns.iter().enumerate() {
        painter.text(
            anchor(cell(index, list.top() + 1.0), *align),
            *align,
            *name,
            theme::ui_font(9.0),
            colors.text_secondary,
        );
    }
    let used = panel.used();
    let rows = ((list.height() - 20.0) / 20.0).floor().max(1.0) as usize;
    if ui.rect_contains_pointer(list) {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            let scroll = panel.state.list_scroll as i64 - wheel.signum() as i64;
            panel.state.list_scroll = scroll.max(0) as usize;
        }
    }
    panel.state.list_scroll = panel.state.list_scroll.min(used.len().saturating_sub(rows));
    if used.is_empty() {
        painter.text(
            list.center(),
            Align2::CENTER_CENTER,
            "No bands: double-click the display to make one",
            theme::ui_font(9.5),
            colors.text_secondary,
        );
    }
    let ids = panel.address.ids;
    for (row, slot) in used.iter().copied().skip(panel.state.list_scroll).take(rows).enumerate() {
        let Some(band) = panel.bands.get(slot).copied() else { continue };
        let y = list.top() + 20.0 + row as f32 * 20.0;
        let line = Rect::from_min_size(Pos2::new(list.left() + 1.0, y), Vec2::new(list.width() - 2.0, 19.0));
        let color = band_color(slot);
        let row_response = ui.interact(line, ids.with(("eq-row", slot)), Sense::click());
        if panel.state.selected == Some(slot) {
            painter.rect_filled(line, 2.0, color.gamma_multiply(0.16));
        }
        if row_response.clicked() {
            panel.select(slot);
        }
        number_button(
            ui,
            panel,
            &painter,
            cell(0, y).shrink2(Vec2::new(4.0, 2.0)),
            ids.with(("eq-row-number", slot)),
            slot,
            band,
            panel.state.selected == Some(slot),
        );
        let ink = if band.on { colors.text_primary } else { colors.text_secondary };
        let shape = small_button(
            ui,
            cell(1, y).shrink2(Vec2::new(2.0, 1.0)),
            ids.with(("eq-row-shape", slot)),
            &format!("Band {} type", slot + 1),
            None,
            |p, r, _| {
                glyph(
                    p,
                    Rect::from_min_size(r.min + Vec2::new(4.0, 4.0), Vec2::new(13.0, 9.0)),
                    band.kind,
                    colors.text_value,
                );
                p.text(
                    r.left_center() + Vec2::new(21.0, 0.0),
                    Align2::LEFT_CENTER,
                    TYPE_NAMES[band.kind],
                    theme::ui_font(9.5),
                    ink,
                );
            },
        );
        shape_menu(panel, &shape, slot, band);
        for (column, field) in [(2, 0usize), (3, 1), (4, 2)] {
            list_value(ui, panel, cell(column, y), slot, band, field);
        }
        let place = small_button(
            ui,
            cell(5, y).shrink2(Vec2::new(4.0, 1.0)),
            ids.with(("eq-row-place", slot)),
            &format!("Band {} placement", slot + 1),
            None,
            |p, r, _| {
                p.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    PLACE_NAMES[band.place],
                    theme::ui_font(9.5),
                    place_color(band.place, &colors),
                );
            },
        );
        place_menu(panel, &place, slot, band);
        // The mode: a click steps to the next mode the band's type takes.
        let mode = small_button(
            ui,
            cell(6, y).shrink2(Vec2::new(2.0, 1.0)),
            ids.with(("eq-row-mode", slot)),
            &format!("Band {} mode", slot + 1),
            None,
            |p, r, _| {
                let ink = if band.mode > 0 { band_color(slot) } else { colors.text_secondary };
                p.text(r.center(), Align2::CENTER_CENTER, MODE_NAMES[band.mode], theme::ui_font(9.0), ink);
            },
        );
        if mode.clicked() {
            let next = (1..=3).map(|step| (band.mode + step) % 3).find(|&m| effective_mode(m as f64, band.kind) == m);
            if let Some(next) = next.filter(|&m| m != band.mode) {
                panel.edit(&[(slot, "Mode", next as f64)], "eq-curve-mode");
            }
        }
    }
}

/// A Freq, Gain or Q cell of the band list: its value, which drags vertically like a dial.
fn list_value(ui: &mut egui::Ui, panel: &mut Panel<'_>, rect: Rect, slot: usize, band: Band, field: usize) {
    let colors = super::device_canvas::colors();
    let field_name = ["Frequency", "Gain", "Q"][field];
    let Some(parameter) = panel.parameter(slot, field_name).cloned() else { return };
    let editable = field != 1 || band.gain_editable();
    let text = match field {
        0 => frequency_text(band.frequency),
        1 if editable => gain_text(band.gain),
        1 => "—".to_string(),
        _ => format!("{:.2}", band.q),
    };
    let ink = if band.on && editable { colors.text_value } else { colors.text_secondary };
    ui.painter_at(rect).text(
        rect.right_center() - Vec2::new(4.0, 0.0),
        Align2::RIGHT_CENTER,
        text,
        theme::mono_font(9.0),
        ink,
    );
    if !editable {
        return;
    }
    let response = ui
        .interact(rect, panel.address.ids.with(("eq-list-value", slot, field)), Sense::click_and_drag())
        .on_hover_text(format!("{}: drag; Shift for precision; double-click resets.", parameter.name))
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if response.clicked() || response.drag_started() {
        panel.select(slot);
    }
    let logarithmic = field != 1;
    let norm = |v: f64| {
        if logarithmic {
            (v.max(parameter.min).ln() - parameter.min.ln()) / (parameter.max.ln() - parameter.min.ln())
        } else {
            (v - parameter.min) / (parameter.max - parameter.min)
        }
    };
    let state_id = response.id.with("start");
    let held = response.is_pointer_button_down_on();
    let start = ui.ctx().data_mut(|d| {
        if held && d.get_temp::<f64>(state_id).is_none() {
            d.insert_temp(state_id, norm(parameter.value));
        }
        d.get_temp::<f64>(state_id)
    });
    if response.double_clicked() {
        panel.edit(&[(slot, field_name, parameter.default)], "eq-curve-list");
    } else if response.drag_stopped() {
        // The release closes the gesture with the value the drag left, never one recomputed
        // from a drag delta egui no longer has.
        panel.edit(&[(slot, field_name, parameter.value)], "eq-curve-list");
    } else if let Some(start) = start
        && response.dragged()
    {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let total = response.total_drag_delta().unwrap_or_default();
        let n = (start - f64::from(total.y) * fine / crate::kit::widgets::PARAMETER_DRAG_PIXELS).clamp(0.0, 1.0);
        let value = if logarithmic {
            (parameter.min.ln() + n * (parameter.max.ln() - parameter.min.ln())).exp()
        } else {
            parameter.min + n * (parameter.max - parameter.min)
        };
        if field == 1 {
            panel.widen_for(value);
        }
        panel.edit(&[(slot, field_name, value)], "eq-curve-list");
    }
    if !held {
        ui.ctx().data_mut(|d| d.remove::<f64>(state_id));
    }
}

#[cfg(test)]
#[path = "eq_curve_tests.rs"]
mod tests;
