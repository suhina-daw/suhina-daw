// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Launcher view's channel strip: the part of a column under its slots, which shows more the
//! taller the mixer is (`docs/spec/ui-session-view.md` › Mixer, sizes and steps in
//! `docs/design/components.md` › Mixer strip). [`layout`] says where each part goes for a height;
//! [`show`] draws a column's strip and turns what is done there into [`Action`]s.

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{InputChannels, Monitor, TrackId};

use crate::interaction::color_picker;
use crate::interaction::commands;
use crate::interaction::grid::{Column, ColumnKind};
use crate::interaction::menus;
use crate::kit::element::{Checked, Muted, Selected};
use crate::kit::meter;
use crate::kit::motion;
use crate::kit::theme::{self, PAD};
use crate::kit::widgets::{self, Action, Drag};
use crate::session::session_view::{Control, SessionViewState};

/// Volume range of the strip's controls, in dB.
const VOLUME_RANGE: (f64, f64) = (-70.0, 6.0);
/// Space between the knobs of the sends row.
const SEND_GAP: f32 = 4.0;
/// The compact volume's height, and the gap between the parts and the toggles.
const COMPACT: f32 = 12.0;
const ABOVE_TOGGLES: f32 = 4.0;
/// The dB readout over the fader, and the gap under it.
const READOUT: f32 = 12.0;
const UNDER_READOUT: f32 = 2.0;
/// A row of knobs (pan, sends) and the gap under it.
const KNOB_ROW: f32 = theme::KNOB + 4.0;
/// The track color's band down the strip's left edge, in its padding.
const COLOR_BAND: f32 = 5.0;
/// The name row and the gap under it.
const NAME: f32 = 16.0;
const UNDER_NAME: f32 = 2.0;
/// The output row ("→" and the dropdown), the arrow's width, and the gap under the row.
const OUTPUT: f32 = 16.0;
const ARROW: f32 = 14.0;
const UNDER_OUTPUT: f32 = 4.0;
/// The audio tracking's block above the toggles: the "Audio From" caption, the source dropdown,
/// and the three monitor buttons, with a gap between the parts.
const AUDIO_CAPTION: f32 = 12.0;
const AUDIO_LIST: f32 = 16.0;
const AUDIO_MONITOR: f32 = 18.0;
const UNDER_AUDIO: f32 = 3.0;
/// The height the audio block appears at, over the height where the name and output arrive and
/// below where the device list does (`docs/design/components.md` › Height steps).
pub const INPUT_AT: f32 = 236.0;
/// A row of the device list, the least number of rows it shows, and the gap under it.
const DEVICE_ROW: f32 = 16.0;
const LEAST_DEVICES: f32 = 4.0;
const UNDER_DEVICES: f32 = 4.0;
/// The fader column's width, its cap, the least fader and the tallest it gets while the device
/// list shows (the list takes the rest).
const FADER_COLUMN: f32 = 20.0;
const SCALE_WIDTH: f32 = 14.0;
const SEND_HEIGHT: f32 = 22.0;
/// Launcher meters use wider stereo bars than compact Arrangement headers.
pub const METER_WIDTH: f32 = 14.0;
const CAP: Vec2 = Vec2::new(20.0, 26.0);

/// The fader cap's size: `fader-cap { width; height }` in the theme, else [`CAP`].
pub(crate) fn cap_size() -> Vec2 {
    let el = crate::kit::element::FADER_CAP.enter();
    let computed = el.style();
    Vec2::new(
        computed.as_ref().and_then(|c| c.width).filter(|w| *w >= 4.0).unwrap_or(CAP.x).min(64.0),
        computed.as_ref().and_then(|c| c.height).filter(|h| *h >= 4.0).unwrap_or(CAP.y).min(96.0),
    )
}

/// The fader rail's thickness: `fader-rail { width }` in the theme, else 5 points.
#[cfg(test)]
pub(crate) fn rail_width() -> f32 {
    rail_width_of(&crate::kit::element::FADER_RAIL.enter())
}

/// The thickness of `el`, a `fader-rail` already entered: `fader-rail { width }` in the theme,
/// else 5 points. A fader enters one rail element for both its thickness and its artwork's color.
fn rail_width_of(el: &crate::kit::element::Entered) -> f32 {
    el.style().and_then(|c| c.width).filter(|w| *w >= 1.0).unwrap_or(5.0).min(32.0)
}
const LEAST_FADER: f32 = 48.0;
const TALLEST_FADER_WITH_DEVICES: f32 = 140.0;

/// The mixer heights at which each part appears (`docs/design/components.md` › Height steps).
pub const FADER_AT: f32 = 96.0;
pub const PAN_AT: f32 = 122.0;
pub const SENDS_AT: f32 = 148.0;
pub const NAME_AT: f32 = 166.0;
pub const OUTPUT_AT: f32 = 192.0;
pub const DEVICES_AT: f32 = 260.0;

/// Where each part of a strip goes, as the strip's box tree lays them out (#1453); a part the
/// strip's height has no room for, or the sheet hides, is `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The band of the track's color down the strip's left edge (`track-color`).
    pub band: Option<Rect>,
    pub name: Option<Rect>,
    /// Where the track's audio goes: "→" and the output dropdown.
    pub output: Option<Rect>,
    /// The audio tracking's block, on a track that holds audio: the caption, the source
    /// ("No input", a channel or a pair of the input device) and the three monitor buttons.
    pub audio_caption: Option<Rect>,
    pub audio_source: Option<Rect>,
    pub audio_monitor: Option<Rect>,
    /// The monitor buttons In, Auto and Off in [`Layout::audio_monitor`].
    pub monitor_buttons: [Option<Rect>; 3],
    pub devices: Option<Rect>,
    /// The line between the device list and the controls under it: dragged, it shares the
    /// strip's height between them (`divider#mixer-divider`).
    pub divider: Option<Rect>,
    pub sends: Option<Rect>,
    pub pan: Option<Rect>,
    /// The dB value over the fader.
    pub readout: Option<Rect>,
    pub fader: Option<Rect>,
    /// The scale beside the fader (`fader-scale`).
    pub scale: Option<Rect>,
    pub meter: Option<Rect>,
    /// The volume as a value across the strip, at heights too small for the fader.
    pub compact: Option<Rect>,
    /// The row of the track-on switch and the toggles (`group#mixer-buttons`).
    pub toggles: Rect,
    /// The track-on switch with the track's number, a part of its own (#995).
    pub track_on: Option<Rect>,
    /// Solo, cue and arm, in `mixer-toggles`.
    pub solo: Option<Rect>,
    pub cue: Option<Rect>,
    pub arm: Option<Rect>,
    /// Main's metronome, where a track has its track-on switch.
    pub metronome: Option<Rect>,
}

/// What every strip of the mixer lays itself out by, besides its own track.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shared {
    /// Some strip holds the input's block, so every strip keeps its room from [`INPUT_AT`] up and
    /// the controls stay in line across the mixer (an empty space where a track has no input).
    pub reserve_input: bool,
    /// The fader's height the divider under the device lists was dragged to; 0 is automatic.
    pub fader: f32,
}

/// What a strip is laid out under: on its own (under `html` and `body`, as in tests), as the
/// strip it paints as, entered, or in an element it is not entered under (Main's, in the Launcher).
#[derive(Clone, Copy)]
pub enum Host<'a> {
    Alone,
    Strip(&'a crate::kit::element::Entered),
    In(&'a crate::kit::element::Entered),
}

/// What kind of strip is laid out: a track's, and whether it can be armed, or Main's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strip {
    Track { armable: bool },
    Main,
}

/// The parts of a strip filling `rect` (the column's width, the mixer's height). A strip without
/// sends leaves its footer empty; an audio track's strip holds the input's block above the
/// toggles from [`INPUT_AT`] up.
pub fn layout(rect: Rect, has_sends: bool, can_hear_input: bool) -> Layout {
    layout_shared(rect, has_sends, can_hear_input, Shared::default())
}

/// [`layout`], with what the mixer's strips share: the input block's room and the divider.
pub fn layout_shared(rect: Rect, has_sends: bool, can_hear_input: bool, shared: Shared) -> Layout {
    layout_in(Host::Alone, rect, has_sends, can_hear_input, shared, Strip::Track { armable: true })
}

pub fn layout_main(rect: Rect, shared: Shared) -> Layout {
    layout_in(Host::Alone, rect, false, false, shared, Strip::Main)
}

/// The parts of a strip, as `el` (the strip, entered) lays them out under the sheet: the app says
/// which parts the strip's height has room for and how big each is, as the native layout had them
/// (`docs/design/components.md` › Height steps), and the sheet places them. Every strip of one
/// kind shares the one layout ([`crate::kit::native_css::row::Bar::kind_of`]).
pub fn layout_in(
    host: Host<'_>,
    rect: Rect,
    has_sends: bool,
    can_hear_input: bool,
    shared: Shared,
    strip: Strip,
) -> Layout {
    use crate::kit::native_css::row::{self, Bar, Item};
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("mixer layout");
    let main = strip == Strip::Main;
    let mut bar = match host {
        Host::Alone => Bar::strip("mixer", rect.size()),
        Host::Strip(el) => Bar::kind_of(el, rect.size()),
        Host::In(parent) => Bar::under(parent, "mixer", rect.size()),
    };
    if main {
        bar = bar.attribute("kind", "main");
    }
    // The room the strip's own border and padding take, as the layout gives it.
    let insets = row::insets(&bar).unwrap_or([0.0; 4]);
    let narrow = rect.width() - insets[1] - insets[3] <= theme::SCENE_COLUMN;
    bar = bar.flag("narrow", narrow);
    let inner = row::inset(rect, insets).shrink(PAD);
    let height = rect.height() - insets[0] - insets[2];
    let footer = height >= SENDS_AT;
    let input = (can_hear_input || shared.reserve_input) && height >= INPUT_AT;
    let compact = height < FADER_AT;
    // The room above the toggles and the input's block, from the strip's top.
    let bottom = inner.bottom()
        - if footer { SEND_HEIGHT + SEND_GAP } else { 0.0 }
        - theme::TOGGLE
        - ABOVE_TOGGLES
        - if input { AUDIO_MONITOR + UNDER_AUDIO + AUDIO_LIST + UNDER_AUDIO + AUDIO_CAPTION } else { 0.0 };
    let name = !compact && height >= NAME_AT;
    let output = !compact && height >= OUTPUT_AT;
    let pan = !compact && height >= PAN_AT;
    // The name is a bar along the strip's top, in the body's padding (the user-agent sheet's
    // `margin: -6px -6px 2px`), so the parts under it have that much more room.
    let top = inner.top()
        + if name { NAME + UNDER_NAME - PAD } else { 0.0 }
        + if output { OUTPUT + UNDER_OUTPUT } else { 0.0 };
    let fixed = READOUT + UNDER_READOUT + if pan { KNOB_ROW } else { 0.0 };
    let rest = (bottom - top - fixed).max(0.0);
    // Devices, pan and fader align across tracks, returns and Main, including the send footer.
    let (devices, fader) = if !compact && height >= DEVICES_AT {
        let fader = if shared.fader > 0.0 {
            // The divider's height, as long as one device row still shows over it.
            shared.fader.clamp(LEAST_FADER, (rest - DEVICE_ROW - UNDER_DEVICES).max(LEAST_FADER))
        } else {
            (rest - LEAST_DEVICES * DEVICE_ROW - UNDER_DEVICES).clamp(LEAST_FADER, TALLEST_FADER_WITH_DEVICES)
        };
        let list = rest - fader - UNDER_DEVICES;
        if list >= DEVICE_ROW { (Some(list), fader) } else { (None, rest) }
    } else {
        (None, rest)
    };
    let full = inner.width();
    let small = if matches!(strip, Strip::Track { armable: true }) { (full - 6.0) / 5.0 } else { (full - 4.0) / 4.0 };
    let part = Item::part;
    let mut tree = row::Tree::default();
    let mut add = |item: Item, slot: Slot| tree.add(item, slot);
    add(part("track-color", "mixer-color").width(COLOR_BAND), Slot::Band);
    let body = add(part("group", "mixer-body"), Slot::None);
    if name {
        add(part("track-name", "mixer-name").height(NAME).in_group(body), Slot::Name);
    }
    if output {
        add(part("group", "mixer-output").height(OUTPUT).in_group(body), Slot::Output);
    }
    if compact {
        if main {
            // Main's meter stands at the top of a strip too low for its fader.
            let tall =
                (height - PAD - if footer { SEND_HEIGHT + SEND_GAP } else { 0.0 } - theme::TOGGLE - 2.0 * PAD).max(0.0);
            add(part("meter", "mixer-meter").width(METER_WIDTH).height(tall).in_group(body), Slot::Meter);
        } else {
            let tall = COMPACT.min(bottom - inner.top()).max(0.0);
            if tall > 0.0 {
                add(part("value", "mixer-compact-volume").height(tall).in_group(body), Slot::Compact);
            }
        }
    } else {
        if let Some(list) = devices {
            add(part("device-list", "mixer-devices").height(list).in_group(body), Slot::Devices);
            add(part("divider", "mixer-divider").height(UNDER_DEVICES + 2.0).in_group(body), Slot::Divider);
        }
        if pan {
            add(part("pan", "mixer-pan").height(theme::KNOB).in_group(body), Slot::Pan);
        }
        // The dB value over the fader, as wide as its column: drawn and taking the pointer there.
        add(part("value", "mixer-volume-readout").width(FADER_COLUMN).height(READOUT).in_group(body), Slot::Readout);
        // The fader's length is its content size: the row of the fader, its scale and the meter
        // takes the room the other parts leave.
        let level = add(part("group", "mixer-level").in_group(body), Slot::None);
        add(part("fader-scale", "mixer-volume-scale").width(SCALE_WIDTH).in_group(level), Slot::Scale);
        add(part("fader", "mixer-volume").width(FADER_COLUMN).height(fader).in_group(level), Slot::Fader);
        add(part("meter", "mixer-meter").width(METER_WIDTH).in_group(level), Slot::Meter);
    }
    if input {
        let block = add(part("group", "mixer-input").in_group(body), Slot::Input);
        add(part("label", "mixer-input-caption").height(AUDIO_CAPTION).in_group(block), Slot::Caption);
        add(part("dropdown", "mixer-input").height(AUDIO_LIST).in_group(block), Slot::Source);
        let monitor = add(part("group", "mixer-monitor").height(AUDIO_MONITOR).in_group(block), Slot::Monitor);
        for (index, kind) in ["mixer-monitor-in", "mixer-monitor-auto", "mixer-monitor-off"].into_iter().enumerate() {
            add(part("button", kind).in_group(monitor), Slot::MonitorButton(index));
        }
    }
    let buttons = add(part("group", "mixer-buttons").in_group(body), Slot::Toggles);
    if main {
        add(
            part("button", "main-metronome").width(28.0_f32.min(full)).height(theme::TOGGLE).in_group(buttons),
            Slot::Metronome,
        );
    } else {
        add(part("button", "track-on").height(theme::TOGGLE).in_group(buttons), Slot::TrackOn);
        let toggles = add(part("mixer-toggles", "mixer-toggles").in_group(buttons), Slot::None);
        add(part("button", "solo").width(small).height(theme::TOGGLE).in_group(toggles), Slot::Solo);
        add(part("button", "cue").width(small).height(theme::TOGGLE).in_group(toggles), Slot::Cue);
        if matches!(strip, Strip::Track { armable: true }) {
            add(part("button", "arm").width(small).height(theme::TOGGLE).in_group(toggles), Slot::Arm);
        }
    }
    if footer {
        add(part("group", "mixer-sends").height(SEND_HEIGHT).in_group(body), Slot::Sends);
    }
    let mut parts = Layout {
        band: None,
        name: None,
        output: None,
        audio_caption: None,
        audio_source: None,
        audio_monitor: None,
        monitor_buttons: [None; 3],
        devices: None,
        divider: None,
        sends: None,
        pan: None,
        readout: None,
        fader: None,
        scale: None,
        meter: None,
        compact: None,
        toggles: Rect::NOTHING,
        track_on: None,
        solo: None,
        cue: None,
        arm: None,
        metronome: None,
    };
    for (slot, at) in tree.lay_out(&bar, rect.min) {
        match slot {
            Slot::None | Slot::Input => {}
            Slot::Band => parts.band = at,
            Slot::Name => parts.name = at,
            Slot::Output => parts.output = at,
            Slot::Caption => parts.audio_caption = at,
            Slot::Source => parts.audio_source = at,
            Slot::Monitor => parts.audio_monitor = at,
            Slot::MonitorButton(button) => {
                if let Some(place) = parts.monitor_buttons.get_mut(button) {
                    *place = at;
                }
            }
            Slot::Devices => parts.devices = at,
            Slot::Divider => parts.divider = at,
            Slot::Pan => parts.pan = at,
            Slot::Readout => parts.readout = at,
            Slot::Scale => parts.scale = at,
            Slot::Fader => parts.fader = at,
            Slot::Meter => parts.meter = at,
            Slot::Compact => parts.compact = at,
            Slot::Toggles => parts.toggles = at.unwrap_or(Rect::NOTHING),
            Slot::TrackOn => parts.track_on = at,
            Slot::Solo => parts.solo = at,
            Slot::Cue => parts.cue = at,
            Slot::Arm => parts.arm = at,
            Slot::Metronome => parts.metronome = at,
            Slot::Sends => parts.sends = at.filter(|_| has_sends),
        }
    }
    // A strip with no input of its own keeps the block's room, empty: nothing in it is drawn.
    if !can_hear_input {
        parts.audio_caption = None;
        parts.audio_source = None;
        parts.audio_monitor = None;
        parts.monitor_buttons = [None; 3];
    }
    parts
}

/// Which part of [`Layout`] an item of the strip's tree is.
#[derive(Clone, Copy)]
enum Slot {
    None,
    Band,
    Name,
    Output,
    Input,
    Caption,
    Source,
    Monitor,
    MonitorButton(usize),
    Devices,
    Divider,
    Pan,
    Readout,
    Scale,
    Fader,
    Meter,
    Compact,
    Toggles,
    TrackOn,
    Solo,
    Cue,
    Arm,
    Metronome,
    Sends,
}

/// One column's strip in `rect`: what [`layout`] gives this height, drawn with `painter` (clipped
/// to the mixer), the meter included. `groups` are the group tracks the output can name.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    groups: &[(TrackId, String)],
    track_number: usize,
    input_channels: u16,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) -> egui::Response {
    if column.folder {
        return ui.interact(rect.intersect(painter.clip_rect()), id.with("folder"), Sense::hover());
    }
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("mixer strip");
    let clipped_painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let painter = &clipped_painter;
    let strip_id = format!("track-{}", column.track.0);
    let style_class = crate::kit::element::MIXER
        .id(&strip_id)
        .color(egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2))
        .attr(Selected, state.selected == Some(column.track))
        .attr(Muted, column.mute)
        .enter();
    // The strip in a shade of its track's color, as the column above it.
    let tint = style_class.background_or(motion::mix(
        crate::kit::style_class::surface_or(style_class.background()),
        egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2),
        0.16,
    ));
    let strip = ui.interact(rect.intersect(painter.clip_rect()), id.with("strip"), Sense::click());
    if !crate::kit::skin::paint_in_view(
        painter,
        rect,
        "track.body",
        crate::kit::skin::State::response(&strip, state.selected == Some(column.track)),
        style_class.own_color_or(tint),
    ) {
        painter.rect_filled(rect, 0.0, tint);
    }
    crate::kit::liquid::paint(painter, rect, &style_class, tint, crate::kit::liquid::Surface::Track);
    style_class.paint_background(painter, rect, 0.0);
    // Its border, and its controls in its content box, inside its border and padding (#1336): the
    // strip's box tree is laid out in its whole box, which holds that border and padding.
    let whole = rect;
    let rect = style_class.paint_border(painter, rect);
    // The strip's own background, under its controls: a double-click shows the track's devices.
    strip
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, format!("{} channel strip", column.name)));
    if strip.double_clicked() {
        state.open_devices = Some(column.track);
    }
    // A click on the strip selects its track, a return's too; a slot selected on that track stays.
    // With Ctrl or Shift the column does it, as on its header.
    if strip.clicked() && !ui.input(|i| i.modifiers.command || i.modifiers.shift) {
        state.selected = Some(column.track);
        state.selected_tracks = vec![column.track];
        state.devices_clicked = true;
        if state.selected_slot.is_some_and(|(track, _)| track != column.track) {
            state.selected_slot = None;
        }
    }
    // The track's color down the left edge, at every height; a click opens the palette.
    let shared = Shared { reserve_input: state.reserve_input, fader: state.strip_fader_shown() };
    let kind = Strip::Track { armable: column.armable };
    let parts = layout_in(Host::Strip(&style_class), whole, !column.sends.is_empty(), column.plays_audio, shared, kind);
    if let Some(band) = parts.band {
        painter.rect_filled(band, 0.0, color_picker::fill(column.color, column.mute));
        let band_hit = band.intersect(painter.clip_rect());
        if band_hit.is_positive() {
            let color_id = id.with("strip-color");
            color_picker::area(ui, band_hit, color_id, column.track, &column.name, column.color, actions);
        }
    }
    if let Some(name) = parts.name {
        name_row(ui, painter, name, column, id, state.selected == Some(column.track));
    }
    if let Some(row) = parts.output {
        output_row(ui, painter, row, column, groups, id, actions);
    }
    if let Some(caption) = parts.audio_caption
        && let Some(row) = parts.audio_source
    {
        audio_block(ui, painter, caption, row, parts.monitor_buttons, column, input_channels, id, actions);
    }
    if let Some(list) = parts.devices {
        devices(ui, painter, list, column, state, id);
        if let (Some(fader), Some(line)) = (parts.fader, parts.divider) {
            divider(ui, painter, rect, line, fader.height(), state);
        }
    }
    // Sends first, so a dragged send's level can stand where the pan's value is.
    let sending = parts.sends.and_then(|row| send_knobs(ui, painter, row, column, state, id, actions));
    if let Some(row) = parts.pan {
        pan_row(ui, painter, row, column, sending, state, id, actions);
    }
    if let Some(fader) = parts.fader {
        let readout = parts.readout.unwrap_or(Rect::NOTHING);
        fader_column(ui, painter, readout, fader, parts.scale, column, state, id, actions);
    }
    if let Some(compact) = parts.compact {
        compact_volume(ui, painter, compact, column, state, id, actions);
    }
    if let Some(at) = parts.meter {
        let level = state.meters.track(column.track);
        meter::show(ui, painter, at, &column.name, meter::Source::Track(column.track), &level, actions);
    }
    toggles(ui, painter, &parts, column, track_number, id, actions);
    strip
}

/// The line under a strip's device list: dragged down, the lists grow and the controls under them
/// get lower in every strip at once; double-clicked, the split is automatic again.
fn divider(ui: &Ui, painter: &egui::Painter, strip: Rect, rect: Rect, fader: f32, state: &mut SessionViewState) {
    // The drag area and its highlight follow where the theme places it, inside the strip.
    let rect = rect.intersect(strip);
    if !rect.is_positive() {
        return;
    }
    let line = rect.center().y;
    let divider = crate::kit::divider::show(
        ui,
        crate::kit::divider::Divider {
            name: "mixer-divider",
            key: Id::unique(("strip-divider", strip.left() as i32)),
            strip: rect.intersect(painter.clip_rect()),
            across: false,
            label: "Device list and controls divider",
            size: fader,
            // Down makes the controls lower and the list taller.
            grows_forward: false,
            bounds: (LEAST_FADER, strip.height().max(LEAST_FADER)),
            closed: false,
        },
    );
    let response = &divider.response;
    if response.double_clicked() {
        state.strip_fader_dragged = None;
        state.strip_fader = 0.0;
    } else if let Some(size) = divider.size {
        state.strip_fader_dragged = Some(size);
    } else if response.drag_stopped_by(egui::PointerButton::Primary) && state.strip_fader_dragged.take().is_some() {
        state.strip_fader = fader.round();
    }
    if response.hovered() || response.dragged() {
        painter.hline(rect.x_range(), line, egui::Stroke::new(2.0, divider.style.color()));
    }
}

/// The track's name at the top of a tall strip; screen readers and tests find it as a label.
fn name_row(ui: &Ui, painter: &egui::Painter, rect: Rect, column: &Column, id: Id, selected: bool) {
    let response = ui.interact(rect.intersect(painter.clip_rect()), id.with("strip-name"), Sense::hover());
    let track_id = format!("track-{}", column.track.0);
    let name_style = crate::kit::element::TRACK_NAME.id(&track_id).response(&response).attr(Selected, selected).enter();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{} strip", column.name)));
    // The name stands on the track's color, as in the column's header.
    let native_fill = color_picker::fill(column.color, column.mute);
    // An alpha material layers over the track color. An explicitly authored
    // transparent solid background can still remove that color completely.
    let fill = if name_style.style().is_some_and(|style| style.has_gradient() && style.background.a() == 0) {
        native_fill
    } else {
        name_style.background_or(native_fill)
    };
    if !crate::kit::skin::paint_in_view(
        painter,
        rect,
        "track.name",
        crate::kit::skin::State::response(&response, selected),
        name_style.own_color_or(fill),
    ) {
        painter.rect_filled(rect, 0.0, fill);
    }
    crate::kit::liquid::paint(painter, rect, &name_style, fill, crate::kit::liquid::Surface::Track);
    name_style.paint_background(painter, rect, 0.0);
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    widgets::text_in(
        painter,
        text,
        Align2::LEFT_CENTER,
        &column.name,
        theme::ui_font(theme::TEXT),
        // Ink that reads on the fill, unless the sheet gives the name a color.
        name_style.color_or(theme::text_on(fill)),
    );
}

/// Where the track's audio goes: "→" and a dropdown of "Main" and the group tracks; picking one
/// runs `track.setOutput`. A track cannot play into itself, so a group is not offered its own name.
fn output_row(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    groups: &[(TrackId, String)],
    id: Id,
    actions: &mut Vec<Action>,
) {
    let current = column.output_name.as_deref().unwrap_or("Main");
    let arrow = Rect::from_min_size(rect.min, Vec2::new(ARROW, rect.height()));
    let hint = crate::kit::element::hint_ink();
    widgets::text_in(painter, arrow, Align2::LEFT_CENTER, "→", theme::ui_font(theme::TEXT_SMALL), hint);
    let field = Rect::from_min_max(Pos2::new(arrow.right(), rect.top()), rect.max);
    let response = widgets::button_area(
        ui,
        field.intersect(painter.clip_rect()),
        id.with("strip-output"),
        format!("{} strip output: {current}", column.name),
    );
    widgets::dropdown(ui, painter, field, &response, current);
    let mut picked = None;
    egui::Popup::menu(&response).show(|ui| {
        if crate::kit::menu::item(ui, "Main", column.output_name.is_none()).clicked() && column.output_name.is_some() {
            picked = Some(serde_json::Value::from("main"));
        }
        for (group, name) in groups.iter().filter(|(g, _)| *g != column.track) {
            if crate::kit::menu::item(ui, name, column.output == Some(*group)).clicked()
                && column.output != Some(*group)
            {
                picked = Some(serde_json::Value::from(group.0));
            }
        }
    });
    if let Some(to) = picked {
        actions.push(Action::new("track.setOutput", json!({ "track": column.track.0, "to": to })));
    }
}

/// The main channel's devices in its strip, at the same height as every track's list, and with
/// the same wheel, clicks and drags as a track's (#683). Main has no track: its devices go by
/// [`TrackId::MAIN`], which the device commands take.
pub fn main_devices(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    devices: &[crate::interaction::grid::StripDevice],
    state: &mut SessionViewState,
) {
    let id = Id::unique("session-main-devices");
    device_list(ui, painter, rect, TrackId::MAIN, "Main", devices, state, id);
}

/// The track's devices and plug-ins, one row each; the wheel scrolls them, a click shows the
/// track's chain in the detail area.
fn devices(ui: &Ui, painter: &egui::Painter, rect: Rect, column: &Column, state: &mut SessionViewState, id: Id) {
    device_list(ui, painter, rect, column.track, &column.name, &column.devices, state, id);
}

/// A strip's device list, a track's or Main's: one row a device, which the wheel scrolls, a click
/// picks, a double-click opens and a drag carries to another track or place.
#[allow(clippy::too_many_arguments)]
fn device_list(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    name: &str,
    devices: &[crate::interaction::grid::StripDevice],
    state: &mut SessionViewState,
    id: Id,
) {
    // The strip's `device-list`, a `device-row` for each device in it.
    let list = crate::kit::element::DEVICE_LIST.enter();
    painter.rect_filled(rect, 0.0, list.background_or(crate::kit::style_class::surface_or(list.background())));
    let hint = crate::kit::element::hint_ink();
    if devices.is_empty() {
        widgets::text_in(
            painter,
            rect.shrink2(Vec2::new(4.0, 0.0)),
            Align2::LEFT_TOP,
            "No devices",
            theme::ui_font(theme::TEXT_SMALL),
            hint,
        );
        return;
    }
    let clip = rect.intersect(painter.clip_rect());
    let list_painter = painter.with_clip_rect(clip);
    // The wheel over the list scrolls it, not the view.
    state.scroll_areas.push(rect);
    let most = (devices.len() as f32 * DEVICE_ROW - rect.height()).max(0.0);
    let mut scroll = state.device_scroll.get(&track).copied().unwrap_or(0.0);
    if ui.rect_contains_pointer(clip) {
        scroll -= ui.input(|i| i.smooth_scroll_delta().y);
    }
    let scroll = scroll.clamp(0.0, most);
    if scroll > 0.0 {
        state.device_scroll.insert(track, scroll);
    } else {
        state.device_scroll.remove(&track);
    }
    let mut slots = Vec::new();
    for (row_index, device) in devices.iter().enumerate() {
        let index = device.at;
        let top = rect.top() - scroll + row_index as f32 * DEVICE_ROW;
        let row = Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(rect.width(), DEVICE_ROW));
        slots.push((index, row));
        if !row.intersects(clip) {
            continue;
        }
        let response = ui.interact(row.intersect(clip), id.with(("device", index)), Sense::click_and_drag());
        let label = format!("Show {} on {name}", device.name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        // Dragged onto another track: moved there, Ctrl copies it (#688).
        if response.drag_started_by(egui::PointerButton::Primary) {
            crate::interaction::drop::start_device_carry(ui.ctx(), track, index);
        }
        if response.double_clicked() {
            state.device_event =
                Some(crate::session::session_view::DeviceEvent::Open { track, index, plugin: device.plugin });
        } else if response.clicked() {
            state.selected = Some(track);
            if !state.selected_tracks.contains(&track) {
                state.selected_tracks = vec![track];
            }
            state.selected_slot = None;
            state.shown_device = Some((track, index));
            state.open_detail = true;
            let (add, run) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            state.device_event = Some(crate::session::session_view::DeviceEvent::Pick { track, index, add, run });
        }
        // Lit as the detail area's chain lights it: the devices picked in either.
        let lit = state.device_picks.iter().any(|(t, at)| *t == track && at.index() == Some(index));
        let el = crate::kit::element::DEVICE_ROW
            .response(&response)
            .attr(Selected, lit)
            .attr(crate::kit::element::Muted, !device.enabled)
            .enter();
        // Its fill fades in under the pointer; `[selected]` and `[muted]` give the lit fill and the
        // dim ink.
        let fill = el.hover_fade(widgets::hover(ui, &response), crate::kit::element::Look::background);
        list_painter.rect_filled(row, 0.0, fill);
        let text = row.shrink2(Vec2::new(4.0, 0.0));
        let color = el.color();
        let tag = match device.group {
            Some(count) => Some(format!("group {count}")),
            None => device.plugin.then(|| "plug-in".to_owned()),
        };
        let name = if tag.is_some() {
            Rect::from_min_max(text.min, Pos2::new(text.right() - 40.0, text.bottom()))
        } else {
            text
        };
        widgets::text_in(
            &list_painter,
            name,
            Align2::LEFT_CENTER,
            &device.name,
            theme::ui_font(theme::TEXT_SMALL),
            color,
        );
        if let Some(tag) = &tag {
            widgets::text_in(&list_painter, text, Align2::RIGHT_CENTER, tag, theme::ui_font(theme::TEXT_TINY), hint);
        }
    }
    // A device of this track carried over its list: a line where it would go, and its place
    // for the drop (#688).
    let carrying = ui.data(|d| {
        d.get_temp::<(suhina_engine::model::TrackId, usize)>(Id::unique(crate::interaction::drop::DEVICE_CARRYING))
    });
    if let Some((_, from)) = carrying.filter(|(t, _)| *t == track)
        && let Some(pointer) = ui.ctx().pointer_latest_pos().filter(|p| clip.contains(*p))
        && let Some(to) = crate::interaction::drop::device_slot_at(&slots, pointer.y, true)
        && to != from
        && to != from + 1
    {
        let y = slots
            .iter()
            .find(|(i, _)| *i == to)
            .map_or_else(|| slots.last().map_or(rect.top(), |(_, r)| r.bottom()), |(_, r)| r.top());
        let color = crate::kit::element::DROP_TARGET.attribute("kind", "insert").look().color();
        list_painter.hline(rect.x_range(), y, egui::Stroke::new(2.0, color));
    }
    let pass = ui.ctx().cumulative_pass_nr();
    crate::interaction::drop::note_device_slots(
        ui.ctx(),
        crate::interaction::drop::DeviceSlots { track, area: clip, slots, vertical: true, pass },
    );
}

/// The audio tracking's block above the toggles: "Audio From" with a dropdown of where the
/// track listens (the same list the track menu shows, one `track.setAudioInput` a pick), and
/// under it three monitor buttons. The one the track's monitoring is on now is lit.
#[allow(clippy::too_many_arguments)]
fn audio_block(
    ui: &Ui,
    painter: &egui::Painter,
    caption: Rect,
    source: Rect,
    monitor: [Option<Rect>; 3],
    column: &Column,
    input_channels: u16,
    id: Id,
    actions: &mut Vec<Action>,
) {
    widgets::text_in(
        painter,
        caption.shrink2(Vec2::new(4.0, 0.0)),
        Align2::LEFT_CENTER,
        "Audio From",
        theme::ui_font(theme::TEXT_SMALL),
        crate::kit::element::hint_ink(),
    );
    let pick = source;
    let current = match column.audio_input.channels {
        InputChannels::None => "No input".to_string(),
        InputChannels::Mono(c) => format!("In {c}"),
        InputChannels::Stereo(c) => format!("In {c}/{}", c.saturating_add(1)),
    };
    let track = column.track;
    let mut picked = None;
    let response = widgets::button_area(
        ui,
        pick.intersect(painter.clip_rect()),
        id.with("strip-audio-input"),
        format!("{} audio from: {current}", column.name),
    );
    widgets::dropdown(ui, painter, pick, &response, &current);
    egui::Popup::menu(&response).show(|ui| {
        for (label, params) in menus::audio_input_picks(&column.audio_input, input_channels) {
            let on = menus::audio_is(&column.audio_input, &params);
            if crate::kit::menu::item(ui, &label, on).clicked() {
                if !on {
                    picked = Some(params);
                }
                ui.close();
            }
        }
        if input_channels == 0 {
            crate::kit::menu::hint(ui, "No input device: choose one in Settings \u{203a} Audio");
        }
    });
    if let Some(mut params) = picked {
        params["track"] = json!(track.0);
        actions.push(Action::new("track.setAudioInput", params));
    }
    // The three monitor picks over the source row: In always hears the input, Auto while the
    // track is armed, Off never; the one the monitoring is on is lit and stays put.
    for ((word, value, monitor_state), area) in
        [("In", "in", Monitor::In), ("Auto", "auto", Monitor::Auto), ("Off", "off", Monitor::Off)]
            .into_iter()
            .zip(monitor)
    {
        // A button the sheet hides has no place, and is neither drawn nor clicked.
        let Some(area) = area else { continue };
        let name = format!("mixer-monitor-{value}");
        let on = column.audio_input.monitor == monitor_state;
        let response = widgets::button_area(
            ui,
            area.intersect(painter.clip_rect()),
            id.with(("strip-monitor", value)),
            format!("{} monitor {word}", column.name),
        );
        // `button#mixer-monitor-…` as `button[kind="monitor"]`, `[checked]` for the one lit.
        let widgets::Lit { el, toggle, lit } =
            widgets::lit_toggle_kind(ui, &response, crate::kit::element::BUTTON.id(&name), &[Checked], on, "monitor");
        let (fill, ink) = toggle.at(lit);
        painter.rect_filled(area, 0.0, fill);
        drop(el);
        widgets::text_in(
            painter,
            area.shrink2(Vec2::new(3.0, 0.0)),
            Align2::CENTER_CENTER,
            word,
            theme::ui_font(theme::TEXT_SMALL),
            ink,
        );
        if response.clicked() && !on {
            actions.push(Action::new("track.setAudioInput", json!({ "track": track.0, "monitor": value })));
        }
    }
}

/// The value a control shows: the one being dragged, else the column's.
fn shown(state: &SessionViewState, column: &Column, control: Control, value: f64) -> f64 {
    match state.drag {
        Some((t, c, v)) if t == column.track && c == control => v,
        _ => value,
    }
}

/// A finished or moving volume drag as its command; every frame of a drag is in one gesture, so
/// the drag is one step.
fn set_volume(track: u32, v: f64) -> Action {
    commands::set_volume(&[track], v, true)
}

/// Runs a volume drag on `response`; returns the value to draw this frame.
fn volume_drag(
    ui: &Ui,
    response: &egui::Response,
    fader: Rect,
    column: &Column,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) -> f64 {
    let track = column.track;
    let mut preview = match state.drag {
        Some((t, Control::Volume, v)) if t == track => Some(v),
        _ => None,
    };
    let continued = preview.is_some();
    let mut committed = None;
    match volume_gesture(ui, response, &mut preview, column.volume_db, fader) {
        Drag::Moving(v) => {
            state.drag = Some((track, Control::Volume, v));
            actions.push(set_volume(track.0, v));
        }
        Drag::Done(v) => {
            state.drag = None;
            committed = Some(v);
            actions.push(set_volume(track.0, v));
        }
        Drag::Set(v) => {
            state.drag = None;
            committed = Some(v);
            actions.push(commands::set_volume(&[track.0], v, false));
        }
        Drag::None => {
            if continued {
                state.drag = None;
            }
        }
    }
    match state.drag {
        Some((t, Control::Volume, v)) if t == track => v,
        _ => committed.unwrap_or(column.volume_db),
    }
}

/// The dB a vertical fader's cap stands for with its center at `y`.
fn fader_db_at(y: f32, fader: Rect) -> f64 {
    let (bottom, top) = (fader.bottom() - cap_size().y / 2.0, fader.top() + cap_size().y / 2.0);
    let position = f64::from((bottom - y) / (bottom - top).max(1.0)).clamp(0.0, 1.0);
    suhina_engine::model::automation::volume_db(position).clamp(VOLUME_RANGE.0, VOLUME_RANGE.1)
}

/// A volume control's gesture. A drag on a vertical fader moves its cap as far as the pointer moves,
/// the pointer hidden and held, so past either end the cap waits and turns back at once; on release
/// the pointer is on the cap. Shift: ten times finer. Everything else is an ordinary value drag.
fn volume_gesture(ui: &Ui, response: &egui::Response, preview: &mut Option<f64>, value: f64, fader: Rect) -> Drag {
    let on_fader = !lies(fader) && fader.height() > cap_size().y && fader.contains(response.rect.center());
    if on_fader && response.dragged_by(egui::PointerButton::Primary) {
        // The arrows and typing go to the fader dragged last, as to any value.
        let _ = widgets::value_keys(ui, response);
        let (bottom, top) = (fader.bottom() - cap_size().y / 2.0, fader.top() + cap_size().y / 2.0);
        let cap_y = |db: f64| bottom + (top - bottom) * widgets::volume_position(db);
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let moved = widgets::value_drag_delta(ui, response).y * fine;
        let db = fader_db_at(cap_y(preview.unwrap_or(value)) + moved, fader);
        widgets::value_drag_returns_to(ui.ctx(), Pos2::new(fader.center().x, cap_y(db)));
        *preview = Some(db);
        return Drag::Moving(db);
    }
    // A drag cut off by losing focus ends here, so the next one is its own undo step.
    if !response.dragged() && !response.drag_stopped() {
        *preview = None;
    }
    // A fader drags along its long side: across, one laid out wider than tall.
    let axis = if lies(fader) { widgets::DragAxis::Horizontal } else { widgets::DragAxis::Vertical };
    widgets::drag_value_axis(ui, response, preview, value, 0.25, 1.0, VOLUME_RANGE, 0.0, axis)
}

/// Whether a fader laid out in `fader` lies on its side: wider than it is tall.
fn lies(fader: Rect) -> bool {
    fader.width() > fader.height()
}

/// The fader with its dB value over it: the column takes the drag, the wheel and the double-click.
#[allow(clippy::too_many_arguments)]
fn fader_column(
    ui: &Ui,
    painter: &egui::Painter,
    readout: Rect,
    fader: Rect,
    scale: Option<Rect>,
    column: &Column,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) {
    // The fader column takes the pointer, up to the readout's top; the meter beside it stays its own.
    state.value_areas.extend([readout, fader].into_iter().filter(Rect::is_positive));
    let label = widgets::format_db(shown(state, column, Control::Volume, column.volume_db));
    let response =
        volume_response(ui, painter, readout, fader, id.with("volume"), &format!("{} volume", column.name), &label);
    let db = volume_drag(ui, &response, fader, column, state, actions);
    paint_fader(ui, painter, readout, fader, scale, db, &response);
}

/// Separate hit rectangles keep a displaced readout from capturing controls in the gap.
fn volume_response(
    ui: &Ui,
    painter: &egui::Painter,
    readout: Rect,
    fader: Rect,
    id: Id,
    name: &str,
    value: &str,
) -> egui::Response {
    let slider = widgets::value_area(ui, fader.intersect(painter.clip_rect()), id, name, value);
    if !readout.is_positive() {
        return slider;
    }
    let field = widgets::value_area(
        ui,
        readout.intersect(painter.clip_rect()),
        id.with("readout"),
        format!("{name} readout"),
        value,
    );
    if slider.dragged() || slider.drag_stopped() || slider.clicked() || slider.hovered() {
        return slider;
    }
    if field.dragged() || field.drag_stopped() || field.clicked() || field.hovered() || field.has_focus() {
        field
    } else {
        slider
    }
}

/// The main output's fader in the main column: as a track's, running `mixer.setMainVolume`. Its
/// dB readout, the fader and its scale where the strip's layout put them.
pub fn main_fader(
    ui: &Ui,
    painter: &egui::Painter,
    (readout, fader, scale): (Rect, Rect, Option<Rect>),
    volume_db: f64,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    state.value_areas.extend([readout, fader].into_iter().filter(Rect::is_positive));
    let mut preview = match state.drag {
        Some((t, Control::Volume, v)) if t == MAIN => Some(v),
        _ => None,
    };
    let label = widgets::format_db(preview.unwrap_or(volume_db));
    let response = volume_response(ui, painter, readout, fader, Id::unique("main-volume"), "Main volume", &label);
    let continued = preview.is_some();
    let send = |v: f64, dragged: bool| {
        let mut params = json!({ "db": commands::db_param(v, VOLUME_RANGE.0) });
        if dragged {
            params["gesture"] = commands::gesture("main-volume");
        }
        Action::new("mixer.setMainVolume", params)
    };
    let mut db = volume_db;
    match volume_gesture(ui, &response, &mut preview, volume_db, fader) {
        Drag::Moving(v) => {
            state.drag = Some((MAIN, Control::Volume, v));
            actions.push(send(v, true));
            db = v;
        }
        Drag::Done(v) => {
            state.drag = None;
            actions.push(send(v, true));
            db = v;
        }
        Drag::Set(v) => {
            state.drag = None;
            actions.push(send(v, false));
            db = v;
        }
        Drag::None => {
            if continued {
                state.drag = None;
            }
        }
    }
    paint_fader(ui, painter, readout, fader, scale, db, &response);
}

/// Where a drag on the main fader is kept in [`SessionViewState::drag`]: no track has this id.
pub const MAIN: TrackId = TrackId(u32::MAX);

/// A fader's look: the dB value over it, the track, the level and the cap.
fn paint_fader(
    ui: &Ui,
    painter: &egui::Painter,
    readout: Rect,
    fader: Rect,
    scale: Option<Rect>,
    db: f64,
    response: &egui::Response,
) {
    let style = crate::kit::element::FADER.id("mixer-volume").response(response).enter();
    widgets::value_drag_pointer(ui.ctx(), response, &style);
    style.paint_background(painter, fader, 0.0);
    let hover = widgets::hover(ui, response);
    let value_color = style.hover_fade(hover, crate::kit::element::Look::color);
    if readout.is_positive() {
        // Drawn where it takes the pointer: over the fader, as wide as its column, unless the
        // sheet lays it out elsewhere.
        let area = readout;
        // `#mixer-volume-readout`: its own background, corners, `font-size`, `color` and number format.
        let readout = crate::kit::element::VALUE.id("mixer-volume-readout").enter();
        readout.paint(painter, area);
        let size = readout.font_size_or(9.0);
        let ink = readout.color_or(value_color);
        let text = widgets::formatted_value(&readout, &widgets::format_db(db));
        painter.text(area.center(), Align2::CENTER_CENTER, text, theme::mono_font(size), ink);
    }
    if lies(fader) {
        paint_horizontal_fader(painter, fader, scale, db, hover, response);
        return;
    }
    let cap_size = cap_size();
    if fader.height() < cap_size.y {
        return;
    }
    // The cap's center travels from half a cap above the bottom to half a cap under the top.
    let travel = egui::Rangef::new(fader.bottom() - cap_size.y / 2.0, fader.top() + cap_size.y / 2.0);
    let at = |position: f32| travel.min + (travel.max - travel.min) * position;
    let x = fader.center().x;
    // An unfilled recessed rail: volume is the cap's position, not a bright background bar.
    let rail_el = crate::kit::element::FADER_RAIL.response(response).enter();
    let rail =
        Rect::from_center_size(Pos2::new(x, fader.center().y), Vec2::new(rail_width_of(&rail_el), fader.height()));
    if !crate::kit::skin::paint(
        painter,
        rail,
        "fader.rail",
        crate::kit::skin::State::response(response, false),
        crate::kit::mixer_controls::rail_ink(&rail_el),
        0.0,
    ) {
        painter.rect_filled(rail, 2.0, crate::kit::mixer_controls::recess());
    }
    drop(rail_el);
    let scale_style = crate::kit::element::FADER_SCALE.enter();
    let ink = scale_style.color();
    // The scale on the side of the fader the sheet lays it out on: its labels against the fader,
    // its ticks by the rail, moved as far as the scale is from its place beside the fader.
    let scale = scale.unwrap_or(Rect::NOTHING);
    let centered = scale.center().x < fader.center().x;
    let native_scale = Rect::from_min_size(
        egui::pos2(if centered { fader.left() - SCALE_WIDTH - 2.0 } else { fader.right() }, fader.top()),
        egui::vec2(SCALE_WIDTH, fader.height()),
    );
    let mut last_label_y = f32::NEG_INFINITY;
    for db in [0.0, -6.0, -12.0, -24.0, -70.0].into_iter().filter(|_| scale.is_positive()) {
        let y = (scale.top()
            + (at(widgets::volume_position(db)) - native_scale.top()) * scale.height() / native_scale.height())
        .round()
            + 0.5;
        if y - last_label_y >= 10.0 {
            let label = if db == -70.0 { "−∞".to_owned() } else { format!("{db:.0}") };
            painter.text(
                Pos2::new(scale.right() - if centered { 0.0 } else { 1.0 }, y),
                Align2::RIGHT_CENTER,
                label,
                theme::mono_font(8.0),
                ink,
            );
            last_label_y = y;
        }
        let length = if db == 0.0 { 5.0 } else { 3.0 };
        let shift = scale.left() - native_scale.left();
        painter.hline(
            egui::Rangef::new(rail.right() + 2.0 + shift, rail.right() + length + 2.0 + shift),
            y,
            theme::hairline(ink),
        );
    }
    drop(scale_style);
    let level = at(widgets::volume_position(db));
    let cap = Rect::from_center_size(Pos2::new(x, level), cap_size);
    crate::kit::mixer_controls::fader_cap(painter, cap, hover, response);
}

fn paint_horizontal_fader(
    painter: &egui::Painter,
    fader: Rect,
    scale: Option<Rect>,
    db: f64,
    hover: f32,
    response: &egui::Response,
) {
    let cap_size = cap_size();
    let length = cap_size.y.min(fader.width());
    let thickness = cap_size.x.min(fader.height());
    let at = |position: f32| fader.left() + length / 2.0 + (fader.width() - length) * position;
    let state = crate::kit::skin::State::response(response, false);
    // Rasterize the same vertical artwork, then rotate its geometry and texture together.
    let rail_el = crate::kit::element::FADER_RAIL.response(response).enter();
    let rail = Rect::from_center_size(fader.center(), Vec2::new(rail_width_of(&rail_el), fader.width()));
    let rail_ink = crate::kit::mixer_controls::rail_ink(&rail_el);
    if !crate::kit::skin::paint(painter, rail, "fader.rail", state, rail_ink, std::f32::consts::FRAC_PI_2) {
        painter.rect_filled(
            Rect::from_center_size(fader.center(), Vec2::new(fader.width(), 5.0)),
            2.0,
            rail_el.background(),
        );
    }
    drop(rail_el);
    let mut previous = f32::NEG_INFINITY;
    // Under the fader, unless the sheet lays the scale out lying down elsewhere; none where it
    // hides it.
    let native_scale = Rect::from_min_size(egui::pos2(fader.left(), fader.bottom()), egui::vec2(fader.width(), 12.0));
    let shown = scale.is_some();
    let scale = scale.filter(|scale| scale.width() > scale.height()).unwrap_or(native_scale);
    let scale_style = crate::kit::element::FADER_SCALE.enter();
    let ink = scale_style.color();
    for value in [-70.0, -24.0, -12.0, -6.0, 0.0].into_iter().filter(|_| shown) {
        let x = scale.left()
            + (at(widgets::volume_position(value)) - native_scale.left()) * scale.width() / native_scale.width();
        if x - previous < 18.0 {
            continue;
        }
        previous = x;
        painter.vline(x, scale.top()..=scale.top() + 3.0, theme::hairline(ink));
        widgets::text_in(
            painter,
            Rect::from_center_size(egui::pos2(x, scale.top() + 7.0), Vec2::new(22.0, 8.0)),
            Align2::CENTER_CENTER,
            if value == -70.0 {
                "−∞"
            } else if value == 0.0 {
                "0"
            } else {
                continue;
            },
            theme::mono_font(8.0),
            ink,
        );
    }
    drop(scale_style);
    let center = egui::pos2(at(widgets::volume_position(db)), fader.center().y);
    let cap = Rect::from_center_size(center, Vec2::new(thickness, length));
    let cap_style = crate::kit::element::FADER_CAP.response(response).enter();
    cap_style.paint_background(painter, Rect::from_center_size(center, Vec2::new(length, thickness)), 2.0);
    let cap_ink = crate::kit::mixer_controls::cap_ink(&cap_style);
    if !crate::kit::skin::paint(painter, cap, "fader.cap", state, cap_ink, std::f32::consts::FRAC_PI_2) {
        let cap = Rect::from_center_size(center, Vec2::new(length, thickness));
        let fill = cap_style.hover_fade(hover, crate::kit::element::Look::background);
        painter.rect_filled(cap, 2.0, fill);
        painter.vline(center.x, cap.shrink(2.0).y_range(), theme::hairline(crate::kit::element::value_ink()));
    }
}

/// The volume as a value across the strip, for heights too small for the fader: a field, or a
/// small knob with its value when Settings asks for knobs.
fn compact_volume(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    column: &Column,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) {
    state.value_areas.push(rect);
    let label = widgets::format_db(shown(state, column, Control::Volume, column.volume_db));
    let response = widgets::value_area(
        ui,
        rect.intersect(painter.clip_rect()),
        id.with("volume"),
        format!("{} volume", column.name),
        label,
    );
    let db = volume_drag(ui, &response, Rect::NOTHING, column, state, actions);
    let hover = widgets::hover(ui, &response);
    let position = crate::kit::widgets::volume_position(db);
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    let font = theme::mono_font(theme::TEXT_SMALL);
    // `value#mixer-volume`, its lit part `value-fill`.
    // Drawn as a bar, it is `value[kind="bar"]`.
    let el = crate::kit::element::VALUE.id("mixer-volume").response(&response);
    let el = if state.knobs { el } else { el.attribute("kind", "bar") }.enter();
    if state.knobs {
        let size = rect.height();
        let center = Pos2::new(rect.left() + size / 2.0, rect.center().y);
        widgets::knob(
            painter,
            center,
            size,
            position,
            el.color_or(motion::mix(crate::kit::element::value_ink(), crate::kit::element::name_ink(), hover)),
            Some(&response),
        );
    } else {
        // A step brighter under the pointer; its lit part is `value-fill`.
        let fill = el.hover_fade(hover, crate::kit::element::Look::background);
        painter.rect_filled(rect, 0.0, fill);
        let x = rect.left() + rect.width() * position;
        let lit = crate::kit::element::VALUE_FILL.look().background();
        painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())), 0.0, lit);
    }
    let ink = el.color();
    widgets::text_in(painter, text, Align2::RIGHT_CENTER, &widgets::format_db(db), font, ink);
}

/// The full-width pan field, sharing its face with Arrangement.
#[allow(clippy::too_many_arguments)]
fn pan_row(
    ui: &Ui,
    painter: &egui::Painter,
    row: Rect,
    column: &Column,
    _sending: Option<(String, f64)>,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) {
    let style = crate::kit::element::PAN.id("mixer-pan").enter();
    let track = column.track;
    let knob_rect = row;
    state.value_areas.push(knob_rect);
    let pan_response = widgets::value_area(
        ui,
        knob_rect.intersect(painter.clip_rect()),
        id.with("pan"),
        format!("{} pan", column.name),
        widgets::format_pan(shown(state, column, Control::Pan, column.pan)),
    );
    widgets::value_drag_pointer(ui.ctx(), &pan_response, &style);
    let mut preview = match state.drag {
        Some((t, Control::Pan, v)) if t == track => Some(v),
        _ => None,
    };
    // Every frame of a drag sets the value, so it is heard while dragging, all in one gesture,
    // so the drag is one step (`ChannelControlDrag`, as the Arrangement's headers do).
    let mut committed = None;
    let drag = widgets::drag_value(ui, &pan_response, &mut preview, column.pan, 0.01, 0.02, (-1.0, 1.0), 0.0);
    if !matches!(drag, Drag::None) {
        let frame = commands::ChannelControlDrag { track, control: commands::ChannelControl::Pan }.frame(drag);
        state.drag = frame.moving.map(|v| (track, Control::Pan, v));
        committed = frame.committed;
        actions.extend(frame.action);
    }
    // Shown after this frame's drag, so the knob never trails the pointer.
    let pan = match state.drag {
        Some((t, Control::Pan, v)) if t == track => v,
        _ => committed.unwrap_or(column.pan),
    };
    if crate::kit::style_class::knob_preference(&style, state.knobs) {
        let size = row.height().min(row.width());
        widgets::knob(
            painter,
            Pos2::new(row.left() + size / 2.0, row.center().y),
            size,
            ((pan.clamp(-1.0, 1.0) + 1.0) / 2.0) as f32,
            style.color_or(crate::kit::element::value_ink()),
            Some(&pan_response),
        );
        if row.width() >= size + 20.0 {
            widgets::text_in(
                painter,
                Rect::from_min_max(egui::pos2(row.left() + size + 4.0, row.top()), row.max),
                Align2::CENTER_CENTER,
                &widgets::format_pan(pan),
                theme::mono_font(theme::TEXT_SMALL),
                style.color(),
            );
        }
    } else {
        crate::kit::mixer_controls::pan_field(painter, row, &style, pan, widgets::hover(ui, &pan_response));
    }
}

/// The labelled send fields, one per return from the left of `row`, as many as fit: dragged up or down for
/// the level (`track.setSend`), double-click for none, right-click for before or after the fader
/// (`track.setSendMode`). Returns the return's letter and the level while one is dragged.
fn send_knobs(
    ui: &Ui,
    painter: &egui::Painter,
    row: Rect,
    column: &Column,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) -> Option<(String, f64)> {
    let track = column.track;
    let clip = row.intersect(painter.clip_rect());
    let painter = painter.with_clip_rect(clip);
    state.scroll_areas.push(row);
    let count = column.sends.len();
    // `send { width; height }` sizes every send, within the row.
    let sheet = {
        let send = crate::kit::element::SEND.enter();
        send.style()
    };
    // The bar that shows how far the sends are scrolled is the row's `scroll-bar`.
    let bar = crate::kit::element::SCROLL_BAR.look().background();
    let width =
        sheet.as_ref().and_then(|s| s.width).map_or((row.width() - SEND_GAP) / 2.0, |w| w.clamp(8.0, row.width()));
    let height = sheet.as_ref().and_then(|s| s.height).map_or(row.height() - 2.0, |h| h.clamp(8.0, row.height()));
    let pitch = width + SEND_GAP;
    let most = (count as f32 * pitch - SEND_GAP - row.width()).max(0.0);
    let mut scroll = state.send_scroll.get(&track).copied().unwrap_or(0.0);
    if ui.rect_contains_pointer(clip) && !widgets::wheel_moves_value(ui) {
        let delta = ui.input(|i| i.smooth_scroll_delta());
        scroll -= if delta.x.abs() > delta.y.abs() { delta.x } else { delta.y };
    }
    scroll = scroll.clamp(0.0, most);
    state.send_scroll.insert(track, scroll);
    let mut sending = None;
    if most > 0.0 {
        let width = (row.width() * row.width() / (row.width() + most)).max(8.0).min(row.width());
        let x = row.left() + (row.width() - width) * scroll / most;
        painter.rect_filled(Rect::from_min_size(Pos2::new(x, row.bottom() - 1.0), Vec2::new(width, 1.0)), 1.0, bar);
    }
    for (index, send) in column.sends.iter().enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(row.left() + pitch * index as f32 - scroll, row.top()),
            Vec2::new(width, height),
        );
        let name = format!("mixer-send-{}", index + 1);
        if !rect.intersects(clip) {
            continue;
        }
        state.value_areas.push(rect.intersect(clip));
        let letter = send.name.chars().next().map(String::from).unwrap_or_default();
        let control = Control::Send(send.target);
        let mode = if send.pre_fader { "pre" } else { "post" };
        let response = widgets::value_area(
            ui,
            rect.intersect(painter.clip_rect()),
            id.with(("send", send.target.0)),
            format!("{} send to {}", column.name, send.name),
            format!("{} {mode}", widgets::format_db(send.level_db)),
        );
        let mut preview = match state.drag {
            Some((t, c, v)) if t == track && c == control => Some(v),
            _ => None,
        };
        let level = if send.level_db.is_finite() { send.level_db } else { widgets::SEND_RANGE.0 };
        let mut committed = None;
        // Heard while it is dragged, every frame of it in one undo step, as the fader is (#705).
        let drag = widgets::drag_value(
            ui,
            &response,
            &mut preview,
            level,
            0.25,
            1.0,
            widgets::SEND_RANGE,
            widgets::SEND_RANGE.0,
        );
        if !matches!(drag, Drag::None) {
            let frame = commands::ChannelControlDrag { track, control: commands::ChannelControl::Send(send.target) }
                .frame(drag);
            state.drag = frame.moving.map(|v| (track, control, v));
            committed = frame.committed;
            actions.extend(frame.action);
        }
        response.context_menu(|ui| {
            for (pre, label) in [(true, "Before the fader (pre)"), (false, "After the fader (post)")] {
                if crate::kit::menu::item(ui, label, send.pre_fader == pre).clicked() {
                    let mode = if pre { "pre" } else { "post" };
                    actions.push(Action::new(
                        "track.setSendMode",
                        json!({ "track": track.0, "return": send.target.0, "mode": mode }),
                    ));
                    ui.close();
                }
            }
        });
        let shown = match state.drag {
            Some((t, c, v)) if t == track && c == control => {
                sending = Some((letter.clone(), v));
                v
            }
            _ => committed.unwrap_or(level),
        };
        let (low, high) = widgets::SEND_RANGE;
        let position = ((shown - low) / (high - low)) as f32;
        // `send` styles every send, `#mixer-send-N` one of them; `appearance: knob` draws it as a
        // knob.
        let el = crate::kit::element::SEND.id(&name).response(&response).enter();
        widgets::value_drag_pointer(ui.ctx(), &response, &el);
        let pointer = el.hover_fade(widgets::hover(ui, &response), crate::kit::element::Look::color);
        if crate::kit::style_class::knob_preference(&el, false) {
            // The value under the knob unless `value-position` says otherwise.
            let text = format!("{letter} {}", widgets::format_db(shown));
            let under = widgets::ValuePosition::Bottom;
            widgets::knob_with_value(
                &painter,
                rect,
                &el,
                position,
                pointer,
                Some(&response),
                &text,
                pointer,
                under,
                7.0,
            );
        } else {
            let state = crate::kit::skin::State::response(&response, false);
            el.paint_with(
                &painter,
                rect,
                crate::kit::element::Native { radius: 2.0, fill: egui::Color32::TRANSPARENT },
            );
            crate::kit::skin::paint_as(&el, &painter, rect, "send", state, pointer);
            crate::kit::mixer_controls::send_field(
                &painter,
                rect,
                &el,
                &letter,
                shown,
                send.pre_fader,
                pointer,
                position,
            );
        }
    }
    sending
}

/// The toggle row: track on (showing the track number, or a return's letter), solo, cue, and
/// arm on a track that plays notes or audio.
fn toggles(
    ui: &Ui,
    painter: &egui::Painter,
    parts: &Layout,
    column: &Column,
    track_number: usize,
    id: Id,
    actions: &mut Vec<Action>,
) {
    let track = column.track;
    let label = if column.mute { format!("Turn {} on", column.name) } else { format!("Turn {} off", column.name) };
    // Each where the strip's tree lays it out (`mixer-toggles` and the track-on switch beside it);
    // one the sheet hides is neither drawn nor clicked.
    if let Some(arm) = parts.arm.filter(|_| column.armable)
        && let Some(action) = widgets::arm_button(ui, painter, arm, id.with("arm"), track.0, &column.name, column.armed)
    {
        actions.push(action);
    }
    if let Some(solo) = parts.solo
        && let Some(action) =
            widgets::solo_button(ui, painter, solo, id.with("solo"), &[track.0], &column.name, column.solo)
    {
        actions.push(action);
    }
    if let Some(cue) = parts.cue
        && let Some(action) = widgets::cue_button(ui, painter, cue, id.with("cue"), track.0, &column.name, column.cue)
    {
        actions.push(action);
    }
    let Some(toggle) = parts.track_on else { return };
    let response = widgets::button_area(ui, toggle, id.with("on"), label);
    if response.clicked() {
        actions.push(Action::new("track.setMute", json!({ "tracks": [track.0], "mute": !column.mute })));
    }
    // Lit only by a rule for `[checked]`: one for every `button` does not hide which tracks are on.
    let lit = widgets::lit_toggle(
        ui,
        &response,
        crate::kit::element::BUTTON.id("track-on").attr(Muted, column.mute),
        &[Checked],
        !column.mute,
    );
    let (fill, text) = lit.colors();
    let style = lit.el;
    widgets::toggle_surface(painter, toggle, &style, &response, fill, lit.toggle.image(lit.lit), 0.0);
    let number = if column.kind == ColumnKind::Return {
        column.name.chars().next().map(String::from).unwrap_or_default()
    } else {
        track_number.to_string()
    };
    let state = crate::kit::skin::State::response(&response, !column.mute);
    if !crate::kit::skin::paint(painter, toggle, "track-on", state, text, 0.0) {
        widgets::track_number_text(painter, toggle, &number, text, column.mute);
    }
}

#[cfg(test)]
#[path = "mixer_strip_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mixer_strip_layout_tests.rs"]
mod layout_tests;
