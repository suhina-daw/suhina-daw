// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Session view's channel strip: the part of a column under its slots, which shows more the
//! taller the mixer is (`docs/spec/ui-session-view.md` › Mixer, sizes and steps in
//! `docs/design/components.md` › Mixer strip). [`layout`] says where each part goes for a height;
//! [`show`] draws a column's strip and turns what is done there into [`Action`]s.

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{InputChannels, Monitor, TrackId};

use crate::interaction::colour_picker;
use crate::interaction::commands;
use crate::interaction::grid::{Column, ColumnKind};
use crate::interaction::menus;
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
/// The track colour's band down the strip's left edge, in its padding.
const COLOUR_BAND: f32 = 5.0;
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
const FADER_COLUMN: f32 = 22.0;
const CAP: Vec2 = Vec2::new(18.0, 7.0);
const LEAST_FADER: f32 = 48.0;
const TALLEST_FADER_WITH_DEVICES: f32 = 140.0;

/// The mixer heights at which each part appears (`docs/design/components.md` › Height steps).
pub const FADER_AT: f32 = 96.0;
pub const PAN_AT: f32 = 122.0;
pub const SENDS_AT: f32 = 148.0;
pub const NAME_AT: f32 = 166.0;
pub const OUTPUT_AT: f32 = 192.0;
pub const DEVICES_AT: f32 = 260.0;

/// Where each part of a strip goes; a part that does not fit at this height is `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub name: Option<Rect>,
    /// Where the track's audio goes: "→" and the output dropdown.
    pub output: Option<Rect>,
    /// The audio tracking's block, on a track that holds audio: the caption, the source
    /// ("No input", a channel or a pair of the input device) and the three monitor buttons.
    pub audio_caption: Option<Rect>,
    pub audio_source: Option<Rect>,
    pub audio_monitor: Option<Rect>,
    pub devices: Option<Rect>,
    pub sends: Option<Rect>,
    pub pan: Option<Rect>,
    /// The dB value over the fader.
    pub readout: Option<Rect>,
    pub fader: Option<Rect>,
    pub meter: Option<Rect>,
    /// The volume as a value across the strip, at heights too small for the fader.
    pub compact: Option<Rect>,
    pub toggles: Rect,
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

/// The parts of a strip filling `rect` (the column's width, the mixer's height). A strip without
/// sends gives their row to the fader; an audio track's strip holds the input's block above the
/// toggles from [`INPUT_AT`] up.
pub fn layout(rect: Rect, has_sends: bool, can_hear_input: bool) -> Layout {
    layout_shared(rect, has_sends, can_hear_input, Shared::default())
}

/// [`layout`], with what the mixer's strips share: the input block's room and the divider.
pub fn layout_shared(rect: Rect, has_sends: bool, can_hear_input: bool, shared: Shared) -> Layout {
    let inner = rect.shrink(PAD);
    let toggles =
        Rect::from_min_max(Pos2::new(inner.left(), (inner.bottom() - theme::TOGGLE).max(inner.top())), inner.max);
    let height = rect.height();
    let mut parts = Layout {
        name: None,
        output: None,
        audio_caption: None,
        audio_source: None,
        audio_monitor: None,
        devices: None,
        sends: None,
        pan: None,
        readout: None,
        fader: None,
        meter: None,
        compact: None,
        toggles,
    };
    let block = |top: f32, height: f32| {
        Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + height))
    };
    let mut bottom = toggles.top() - ABOVE_TOGGLES;
    if (can_hear_input || shared.reserve_input) && height >= INPUT_AT {
        // Above the toggles: the three monitor buttons, over them the source's dropdown, and
        // at the top the block's caption. A strip with no input keeps the room empty.
        bottom -= AUDIO_MONITOR;
        let monitor = block(bottom, AUDIO_MONITOR);
        bottom -= UNDER_AUDIO + AUDIO_LIST;
        let source = block(bottom, AUDIO_LIST);
        bottom -= UNDER_AUDIO + AUDIO_CAPTION;
        let caption = block(bottom, AUDIO_CAPTION);
        if can_hear_input {
            parts.audio_monitor = Some(monitor);
            parts.audio_source = Some(source);
            parts.audio_caption = Some(caption);
        }
    }
    let row = |top: f32, height: f32| {
        Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + height))
    };
    if height < FADER_AT {
        let top = (bottom - COMPACT).max(inner.top());
        parts.compact = Some(row(top, bottom - top)).filter(|r| r.height() > 0.0);
        return parts;
    }
    let mut top = inner.top();
    if height >= NAME_AT {
        parts.name = Some(row(top, NAME));
        top += NAME + UNDER_NAME;
    }
    if height >= OUTPUT_AT {
        parts.output = Some(row(top, OUTPUT));
        top += OUTPUT + UNDER_OUTPUT;
    }
    let pan = height >= PAN_AT;
    let sends = height >= SENDS_AT && has_sends;
    let fixed = READOUT + UNDER_READOUT + if pan { KNOB_ROW } else { 0.0 } + if sends { KNOB_ROW } else { 0.0 };
    let rest = (bottom - top - fixed).max(0.0);
    let fader = if height >= DEVICES_AT {
        // The device list is as tall on every strip, Main and the returns too (#683): it is
        // worked out as if the strip had a row of sends, and a strip without them gives that
        // row to its fader.
        let spare = if height >= SENDS_AT && !has_sends { KNOB_ROW } else { 0.0 };
        let room = (rest - spare).max(0.0);
        let fader = if shared.fader > 0.0 {
            // The divider's height, as long as one device row still shows over it.
            shared.fader.clamp(LEAST_FADER, (room - DEVICE_ROW - UNDER_DEVICES).max(LEAST_FADER))
        } else {
            (room - LEAST_DEVICES * DEVICE_ROW - UNDER_DEVICES).clamp(LEAST_FADER, TALLEST_FADER_WITH_DEVICES)
        };
        let list = room - fader - UNDER_DEVICES;
        if list >= DEVICE_ROW {
            parts.devices = Some(row(top, list));
            top += list + UNDER_DEVICES;
            fader + spare
        } else {
            rest
        }
    } else {
        rest
    };
    if sends {
        parts.sends = Some(row(top, theme::KNOB));
        top += KNOB_ROW;
    }
    if pan {
        parts.pan = Some(row(top, theme::KNOB));
        top += KNOB_ROW;
    }
    let column = |top: f32, height: f32| {
        Rect::from_min_size(Pos2::new(inner.left(), top), Vec2::new(FADER_COLUMN.min(inner.width()), height))
    };
    // The readout spans the strip, so "-12.0" is never cut; it starts over the fader.
    parts.readout = Some(row(top, READOUT));
    top += READOUT + UNDER_READOUT;
    let fader = column(top, fader.min(bottom - top).max(0.0));
    parts.meter =
        Some(Rect::from_min_size(Pos2::new(fader.right() + PAD, fader.top()), Vec2::new(meter::WIDTH, fader.height())));
    parts.fader = Some(fader);
    parts
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
    // The strip in a shade of its track's colour, as the column above it.
    let tint = motion::mix(
        theme::panel_background(),
        egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2),
        0.16,
    );
    painter.rect_filled(rect, 0.0, tint);
    // The strip's own background, under its controls: a double-click shows the track's devices.
    let strip = ui.interact(rect.intersect(painter.clip_rect()), id.with("strip"), Sense::click());
    strip.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{} channel strip", column.name))
    });
    if strip.double_clicked() {
        state.open_devices = Some(column.track);
    }
    // A click on the strip selects its track, a return's too; a slot selected on that track stays.
    if strip.clicked() {
        state.selected = Some(column.track);
        if state.selected_slot.is_some_and(|(track, _)| track != column.track) {
            state.selected_slot = None;
        }
    }
    // The track's colour down the left edge, at every height; a click opens the palette.
    let band = Rect::from_min_size(rect.min, Vec2::new(COLOUR_BAND, rect.height()));
    painter.rect_filled(band, 0.0, colour_picker::fill(column.color, column.mute));
    let band_hit = band.intersect(painter.clip_rect());
    if band_hit.is_positive() {
        let colour_id = id.with("strip-colour");
        colour_picker::area(ui, band_hit, colour_id, column.track, &column.name, column.color, actions);
    }
    let shared = Shared { reserve_input: state.reserve_input, fader: state.strip_fader_shown() };
    let parts = layout_shared(rect, !column.sends.is_empty(), column.plays_audio, shared);
    if let Some(name) = parts.name {
        name_row(ui, painter, name, column, id);
    }
    if let Some(row) = parts.output {
        output_row(ui, painter, row, column, groups, id, actions);
    }
    if let Some(caption) = parts.audio_caption
        && let Some(row) = parts.audio_source
        && let Some(toggles) = parts.audio_monitor
    {
        audio_block(ui, painter, caption, row, toggles, column, input_channels, id, actions);
    }
    if let Some(list) = parts.devices {
        devices(ui, painter, list, column, state, id);
        if let Some(fader) = parts.fader {
            divider(ui, painter, rect, list, fader.height(), state);
        }
    }
    // Sends first, so a dragged send's level can stand where the pan's value is.
    let sending = parts.sends.and_then(|row| send_knobs(ui, painter, row, column, state, id, actions));
    if let Some(row) = parts.pan {
        pan_row(ui, painter, row, column, sending, state, id, actions);
    }
    if let (Some(readout), Some(fader)) = (parts.readout, parts.fader) {
        fader_column(ui, painter, readout, fader, column, state, id, actions);
    }
    if let Some(compact) = parts.compact {
        compact_volume(ui, painter, compact, column, state, id, actions);
    }
    if let Some(at) = parts.meter {
        let level = state.meters.track(column.track);
        meter::show(ui, painter, at, &column.name, meter::Source::Track(column.track), &level, actions);
    }
    toggles(ui, painter, parts.toggles, column, track_number, id, actions);
    strip
}

/// The line under a strip's device list: dragged down, the lists grow and the controls under them
/// get lower in every strip at once; double-clicked, the split is automatic again.
fn divider(ui: &Ui, painter: &egui::Painter, strip: Rect, list: Rect, fader: f32, state: &mut SessionViewState) {
    let line = list.bottom() + UNDER_DEVICES / 2.0;
    let rect = Rect::from_x_y_ranges(strip.x_range(), egui::Rangef::new(line - 3.0, line + 3.0));
    let response = ui
        .interact(
            rect.intersect(painter.clip_rect()),
            Id::new(("strip-divider", strip.left() as i32)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Device list and controls divider"));
    if response.double_clicked() {
        state.strip_fader_dragged = None;
        state.strip_fader = 0.0;
    } else if response.dragged_by(egui::PointerButton::Primary) {
        // Down makes the controls lower and the list taller.
        state.strip_fader_dragged = Some((fader - response.drag_delta().y).max(LEAST_FADER));
    } else if response.drag_stopped_by(egui::PointerButton::Primary) && state.strip_fader_dragged.take().is_some() {
        state.strip_fader = fader.round();
    }
    if response.hovered() || response.dragged() {
        painter.hline(strip.x_range(), line, egui::Stroke::new(2.0, theme::accent()));
    }
}

/// The track's name at the top of a tall strip; screen readers and tests find it as a label.
fn name_row(ui: &Ui, painter: &egui::Painter, rect: Rect, column: &Column, id: Id) {
    let response = ui.interact(rect.intersect(painter.clip_rect()), id.with("strip-name"), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{} strip", column.name)));
    // The name stands on the track's colour, as in the column's header.
    let fill = colour_picker::fill(column.color, column.mute);
    painter.rect_filled(rect, 0.0, fill);
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    widgets::text_in(
        painter,
        text,
        Align2::LEFT_CENTER,
        &column.name,
        theme::ui_font(theme::TEXT),
        theme::text_on(fill),
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
    let current = column.output.and_then(|t| groups.iter().find(|(g, _)| *g == t)).map_or("Main", |(_, name)| name);
    let arrow = Rect::from_min_size(rect.min, Vec2::new(ARROW, rect.height()));
    widgets::text_in(painter, arrow, Align2::LEFT_CENTER, "→", theme::ui_font(theme::TEXT_SMALL), theme::text_hint());
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
        let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        if ui.selectable_label(column.output.is_none(), text("Main")).clicked() && column.output.is_some() {
            picked = Some(serde_json::Value::from("main"));
        }
        for (group, name) in groups.iter().filter(|(g, _)| *g != column.track) {
            if ui.selectable_label(column.output == Some(*group), text(name)).clicked() && column.output != Some(*group)
            {
                picked = Some(serde_json::Value::from(group.0));
            }
        }
    });
    if let Some(to) = picked {
        actions.push(Action::new("track.setOutput", json!({ "track": column.track.0, "to": to })));
    }
}

/// The main channel's devices in its strip, at the same height as every track's list (#683).
pub fn main_devices(painter: &egui::Painter, rect: Rect, devices: &[crate::interaction::grid::StripDevice]) {
    painter.rect_filled(rect, 0.0, theme::window_background());
    let list_painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    if devices.is_empty() {
        widgets::text_in(
            &list_painter,
            rect.shrink2(Vec2::new(4.0, 0.0)),
            Align2::LEFT_TOP,
            "No devices",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        return;
    }
    for (index, device) in devices.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left(), rect.top() + index as f32 * DEVICE_ROW),
            Vec2::new(rect.width(), DEVICE_ROW),
        );
        let color = if device.enabled { theme::text_secondary() } else { theme::text_hint() };
        widgets::text_in(
            &list_painter,
            row.shrink2(Vec2::new(4.0, 0.0)),
            Align2::LEFT_CENTER,
            &device.name,
            theme::ui_font(theme::TEXT_SMALL),
            color,
        );
    }
}

/// The track's devices and plug-ins, one row each; the wheel scrolls them, a click shows the
/// track's chain in the detail area.
fn devices(ui: &Ui, painter: &egui::Painter, rect: Rect, column: &Column, state: &mut SessionViewState, id: Id) {
    painter.rect_filled(rect, 0.0, theme::window_background());
    let track = column.track;
    if column.devices.is_empty() {
        widgets::text_in(
            painter,
            rect.shrink2(Vec2::new(4.0, 0.0)),
            Align2::LEFT_TOP,
            "No devices",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        return;
    }
    let clip = rect.intersect(painter.clip_rect());
    let list_painter = painter.with_clip_rect(clip);
    // The wheel over the list scrolls it, not the view.
    state.scroll_areas.push(rect);
    let most = (column.devices.len() as f32 * DEVICE_ROW - rect.height()).max(0.0);
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
    for (index, device) in column.devices.iter().enumerate() {
        let top = rect.top() - scroll + index as f32 * DEVICE_ROW;
        let row = Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(rect.width(), DEVICE_ROW));
        if !row.intersects(clip) {
            continue;
        }
        let response = ui.interact(row.intersect(clip), id.with(("device", index)), Sense::click_and_drag());
        let label = format!("Show {} on {}", device.name, column.name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        // Dragged onto another track: moved there, Ctrl copies it (#688).
        if response.drag_started_by(egui::PointerButton::Primary) {
            crate::interaction::drop::start_device_carry(ui.ctx(), track, index);
        }
        if response.double_clicked() {
            state.device_event =
                Some(crate::session::session_view::DeviceEvent::Open { track, index, plugin: device.plugin });
        } else if response.clicked() {
            state.selected = Some(track);
            state.selected_slot = None;
            state.shown_device = Some((track, index));
            state.open_detail = true;
            let (add, run) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            state.device_event = Some(crate::session::session_view::DeviceEvent::Pick { track, index, add, run });
        }
        // Lit as the detail area's chain lights it: the devices picked in either.
        let lit = state.device_picks.contains(&(track, index));
        let fill = if lit {
            theme::control_active()
        } else {
            motion::mix(theme::window_background(), theme::control_hover(), widgets::hover(ui, &response))
        };
        list_painter.rect_filled(row, 0.0, fill);
        let text = row.shrink2(Vec2::new(4.0, 0.0));
        let color = if device.enabled { theme::text_secondary() } else { theme::text_hint() };
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
            widgets::text_in(
                &list_painter,
                text,
                Align2::RIGHT_CENTER,
                tag,
                theme::ui_font(theme::TEXT_TINY),
                theme::text_hint(),
            );
        }
    }
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
    monitor: Rect,
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
        theme::text_hint(),
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
            if ui.selectable_label(on, egui::RichText::new(label).font(theme::ui_font(theme::TEXT_SMALL))).clicked()
                && !on
            {
                picked = Some(params);
            }
        }
        if input_channels == 0 {
            ui.label(
                egui::RichText::new("No input device: choose one in Settings \u{203a} Audio")
                    .font(theme::ui_font(theme::TEXT_SMALL)),
            );
        }
    });
    if let Some(mut params) = picked {
        params["track"] = json!(track.0);
        actions.push(Action::new("track.setAudioInput", params));
    }
    // The three monitor picks over the source row: In always hears the input, Auto while the
    // track is armed, Off never; the one the monitoring is on is lit and stays put.
    let width = (monitor.width() - 4.0) / 3.0;
    for (index, (word, value, monitor_state)) in
        [("In", "in", Monitor::In), ("Auto", "auto", Monitor::Auto), ("Off", "off", Monitor::Off)]
            .into_iter()
            .enumerate()
    {
        let area = Rect::from_min_size(
            Pos2::new(monitor.left() + index as f32 * (width + 2.0), monitor.top()),
            Vec2::new(width, monitor.height()),
        );
        let on = column.audio_input.monitor == monitor_state;
        let response = widgets::button_area(
            ui,
            area.intersect(painter.clip_rect()),
            id.with(("strip-monitor", value)),
            format!("{} monitor {word}", column.name),
        );
        let fill = if on {
            theme::control_active()
        } else {
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response))
        };
        painter.rect_filled(area, 0.0, fill);
        widgets::text_in(
            painter,
            area.shrink2(Vec2::new(3.0, 0.0)),
            Align2::CENTER_CENTER,
            word,
            theme::ui_font(theme::TEXT_SMALL),
            if on { theme::text_primary() } else { theme::text_secondary() },
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

/// A finished or moving volume drag as its command; every frame of a drag but the first
/// continues its undo step, so the drag is one step.
fn set_volume(track: u32, v: f64, continued: bool) -> Action {
    commands::set_volume(&[track], v, Some(continued))
}

/// Runs a volume drag on `response`; returns the value to draw this frame.
fn volume_drag(
    ui: &Ui,
    response: &egui::Response,
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
    match widgets::drag_value(ui, response, &mut preview, column.volume_db, 0.25, 1.0, VOLUME_RANGE, 0.0) {
        Drag::Moving(v) => {
            state.drag = Some((track, Control::Volume, v));
            actions.push(set_volume(track.0, v, continued));
        }
        Drag::Done(v) => {
            state.drag = None;
            committed = Some(v);
            actions.push(set_volume(track.0, v, continued));
        }
        Drag::None => {}
    }
    match state.drag {
        Some((t, Control::Volume, v)) if t == track => v,
        _ => committed.unwrap_or(column.volume_db),
    }
}

/// The fader with its dB value over it: the column takes the drag, the wheel and the double-click.
#[allow(clippy::too_many_arguments)]
fn fader_column(
    ui: &Ui,
    painter: &egui::Painter,
    readout: Rect,
    fader: Rect,
    column: &Column,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) {
    // The fader column takes the pointer, up to the readout's top; the meter beside it stays its own.
    let area = Rect::from_min_max(Pos2::new(fader.left(), readout.top()), fader.max);
    state.value_areas.push(area);
    let label = widgets::format_db(shown(state, column, Control::Volume, column.volume_db));
    let response = widgets::value_area(
        ui,
        area.intersect(painter.clip_rect()),
        id.with("volume"),
        format!("{} volume", column.name),
        label,
    );
    let db = volume_drag(ui, &response, column, state, actions);
    paint_fader(ui, painter, readout, fader, db, &response);
}

/// The main output's fader in the main column: as a track's, running `mixer.setMainVolume`.
pub fn main_fader(
    ui: &Ui,
    painter: &egui::Painter,
    readout: Rect,
    fader: Rect,
    volume_db: f64,
    state: &mut SessionViewState,
    actions: &mut Vec<Action>,
) {
    let area = Rect::from_min_max(Pos2::new(fader.left(), readout.top()), fader.max);
    state.value_areas.push(area);
    let mut preview = match state.drag {
        Some((t, Control::Volume, v)) if t == MAIN => Some(v),
        _ => None,
    };
    let label = widgets::format_db(preview.unwrap_or(volume_db));
    let response =
        widgets::value_area(ui, area.intersect(painter.clip_rect()), Id::new("main-volume"), "Main volume", label);
    let continued = preview.is_some();
    let send = |v: f64, continued: bool| {
        Action::new(
            "mixer.setMainVolume",
            json!({ "db": commands::db_param(v, VOLUME_RANGE.0), "continue": continued }),
        )
    };
    let mut db = volume_db;
    match widgets::drag_value(ui, &response, &mut preview, volume_db, 0.25, 1.0, VOLUME_RANGE, 0.0) {
        Drag::Moving(v) => {
            state.drag = Some((MAIN, Control::Volume, v));
            actions.push(send(v, continued));
            db = v;
        }
        Drag::Done(v) => {
            state.drag = None;
            actions.push(send(v, continued));
            db = v;
        }
        Drag::None => {}
    }
    paint_fader(ui, painter, readout, fader, db, &response);
}

/// Where a drag on the main fader is kept in [`SessionViewState::drag`]: no track has this id.
pub const MAIN: TrackId = TrackId(u32::MAX);

/// A fader's look: the dB value over it, the track, the level and the cap.
fn paint_fader(ui: &Ui, painter: &egui::Painter, readout: Rect, fader: Rect, db: f64, response: &egui::Response) {
    let hover = widgets::hover(ui, response);
    let value_color = motion::mix(theme::text_value(), theme::text_primary(), hover);
    widgets::text_in(
        painter,
        readout,
        Align2::LEFT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        value_color,
    );
    if fader.height() < CAP.y {
        return;
    }
    // The cap's centre travels from half a cap above the bottom to half a cap under the top.
    let travel = egui::Rangef::new(fader.bottom() - CAP.y / 2.0, fader.top() + CAP.y / 2.0);
    let at = |position: f32| travel.min + (travel.max - travel.min) * position;
    let x = fader.center().x;
    let track = Rect::from_center_size(Pos2::new(x, fader.center().y), Vec2::new(4.0, fader.height()));
    painter.rect_filled(track, 0.0, theme::control_background());
    let level = at(crate::kit::widgets::volume_position(db));
    painter.rect_filled(Rect::from_min_max(Pos2::new(track.left(), level), track.max), 0.0, theme::control_playing());
    let unity = at(crate::kit::widgets::volume_position(0.0)).round() + 0.5;
    painter.hline(
        egui::Rangef::new(track.left() - 5.0, track.right() + 5.0),
        unity,
        theme::hairline(theme::text_hint()),
    );
    let cap = Rect::from_center_size(Pos2::new(x, level), CAP);
    painter.rect_filled(cap, 0.0, value_color);
    painter.hline(cap.x_range(), cap.center().y.round() + 0.5, theme::hairline(theme::window_background()));
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
    let db = volume_drag(ui, &response, column, state, actions);
    let hover = widgets::hover(ui, &response);
    let position = crate::kit::widgets::volume_position(db);
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    let font = theme::mono_font(theme::TEXT_SMALL);
    if state.knobs {
        let size = rect.height();
        let center = Pos2::new(rect.left() + size / 2.0, rect.center().y);
        widgets::knob(painter, center, size, position, motion::mix(theme::text_value(), theme::text_primary(), hover));
    } else {
        painter.rect_filled(rect, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
        let x = rect.left() + rect.width() * position;
        painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())), 0.0, theme::control_playing());
    }
    widgets::text_in(painter, text, Align2::RIGHT_CENTER, &widgets::format_db(db), font, theme::text_primary());
}

/// The pan knob with its value beside it; while a send is dragged its level stands there.
#[allow(clippy::too_many_arguments)]
fn pan_row(
    ui: &Ui,
    painter: &egui::Painter,
    row: Rect,
    column: &Column,
    sending: Option<(String, f64)>,
    state: &mut SessionViewState,
    id: Id,
    actions: &mut Vec<Action>,
) {
    let track = column.track;
    let knob_rect = Rect::from_min_size(row.min, Vec2::splat(theme::KNOB));
    state.value_areas.push(knob_rect);
    let pan_response = widgets::value_area(
        ui,
        knob_rect.intersect(painter.clip_rect()),
        id.with("pan"),
        format!("{} pan", column.name),
        widgets::format_pan(shown(state, column, Control::Pan, column.pan)),
    );
    let mut preview = match state.drag {
        Some((t, Control::Pan, v)) if t == track => Some(v),
        _ => None,
    };
    // Every frame of a drag sets the value, so it is heard while dragging; all but the first
    // continue its undo step, so the drag is one step.
    let continued = preview.is_some();
    let set_pan = |v: f64| {
        Action::new(
            "track.setPan",
            json!({ "tracks": [track.0], "pan": (v * 100.0).round() / 100.0, "continue": continued }),
        )
    };
    let mut committed = None;
    match widgets::drag_value(ui, &pan_response, &mut preview, column.pan, 0.01, 0.02, (-1.0, 1.0), 0.0) {
        Drag::Moving(v) => {
            state.drag = Some((track, Control::Pan, v));
            actions.push(set_pan(v));
        }
        Drag::Done(v) => {
            state.drag = None;
            committed = Some(v);
            actions.push(set_pan(v));
        }
        Drag::None => {}
    }
    // Shown after this frame's drag, so the knob never trails the pointer.
    let pan = match state.drag {
        Some((t, Control::Pan, v)) if t == track => v,
        _ => committed.unwrap_or(column.pan),
    };
    widgets::knob(painter, knob_rect.center(), theme::KNOB, ((pan + 1.0) / 2.0) as f32, theme::text_primary());
    let value =
        sending.map_or_else(|| widgets::format_pan(pan), |(letter, db)| format!("{letter} {}", widgets::format_db(db)));
    let text = Rect::from_min_max(Pos2::new(knob_rect.right() + PAD, row.top()), row.max);
    widgets::text_in(
        painter,
        text,
        Align2::LEFT_CENTER,
        &value,
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
}

/// The send knobs, one per return from the left of `row`, as many as fit: dragged up or down for
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
    let step = theme::KNOB + SEND_GAP;
    let fit = ((row.width() + SEND_GAP) / step).floor().max(0.0) as usize;
    let mut sending = None;
    for (index, send) in column.sends.iter().take(fit).enumerate() {
        let rect =
            Rect::from_min_size(Pos2::new(row.left() + step * index as f32, row.top()), Vec2::splat(theme::KNOB));
        state.value_areas.push(rect);
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
        let level = if send.level_db.is_finite() { send.level_db } else { VOLUME_RANGE.0 };
        let mut committed = None;
        // Heard while it is dragged, every frame of it in one undo step, as the fader is (#705).
        let continued = preview.is_some();
        match widgets::drag_value(ui, &response, &mut preview, level, 0.25, 1.0, VOLUME_RANGE, VOLUME_RANGE.0) {
            Drag::Moving(v) => {
                state.drag = Some((track, control, v));
                actions.push(commands::set_send(track, send.target, v, continued));
            }
            Drag::Done(v) => {
                state.drag = None;
                committed = Some(v);
                actions.push(commands::set_send(track, send.target, v, continued));
            }
            Drag::None => {}
        }
        response.context_menu(|ui| {
            for (pre, label) in [(true, "Before the fader (pre)"), (false, "After the fader (post)")] {
                if ui.radio(send.pre_fader == pre, label).clicked() {
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
        let position = ((shown - VOLUME_RANGE.0) / (VOLUME_RANGE.1 - VOLUME_RANGE.0)) as f32;
        let pointer = motion::mix(theme::text_value(), theme::text_primary(), widgets::hover(ui, &response));
        widgets::knob(painter, rect.center(), theme::KNOB, position, pointer);
        // The return's letter in the knob, and a dot under it when the send is before the fader.
        let font = theme::mono_font(theme::TEXT_SMALL - 2.0);
        widgets::text_in(painter, rect, Align2::CENTER_CENTER, &letter, font, theme::text_hint());
        if send.pre_fader {
            painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() + 1.5), 1.5, theme::text_value());
        }
    }
    sending
}

/// The toggle row: track on (showing the track number, or a return's letter), solo, cue, and
/// arm on a track that plays notes or audio.
fn toggles(
    ui: &Ui,
    painter: &egui::Painter,
    toggle: Rect,
    column: &Column,
    track_number: usize,
    id: Id,
    actions: &mut Vec<Action>,
) {
    let track = column.track;
    let label = if column.mute { format!("Turn {} on", column.name) } else { format!("Turn {} off", column.name) };
    // Track on takes what solo and cue leave (a quarter each); on a track that can be armed,
    // solo, cue and arm take a fifth each. 2 px apart.
    let small = if column.armable { (toggle.width() - 6.0) / 5.0 } else { (toggle.width() - 4.0) / 4.0 };
    let toggle = if column.armable {
        let arm = Rect::from_min_max(Pos2::new(toggle.right() - small, toggle.top()), toggle.max);
        if let Some(action) = widgets::arm_button(ui, painter, arm, id.with("arm"), track.0, &column.name, column.armed)
        {
            actions.push(action);
        }
        Rect::from_min_max(toggle.min, Pos2::new(arm.left() - 2.0, toggle.bottom()))
    } else {
        toggle
    };
    let cue = Rect::from_min_max(Pos2::new(toggle.right() - small, toggle.top()), toggle.max);
    let solo = Rect::from_min_max(
        Pos2::new(cue.left() - 2.0 - small, toggle.top()),
        Pos2::new(cue.left() - 2.0, toggle.bottom()),
    );
    let toggle = Rect::from_min_max(toggle.min, Pos2::new(solo.left() - 2.0, toggle.bottom()));
    if let Some(action) =
        widgets::solo_button(ui, painter, solo, id.with("solo"), &[track.0], &column.name, column.solo)
    {
        actions.push(action);
    }
    if let Some(action) = widgets::cue_button(ui, painter, cue, id.with("cue"), track.0, &column.name, column.cue) {
        actions.push(action);
    }
    let response = widgets::button_area(ui, toggle, id.with("on"), label);
    if response.clicked() {
        actions.push(Action::new("track.setMute", json!({ "tracks": [track.0], "mute": !column.mute })));
    }
    let (fill, text) = widgets::toggle_colors(ui, &response, !column.mute);
    painter.rect_filled(toggle, 0.0, fill);
    let number = if column.kind == ColumnKind::Return {
        column.name.chars().next().map(String::from).unwrap_or_default()
    } else {
        track_number.to_string()
    };
    widgets::text_in(painter, toggle, Align2::CENTER_CENTER, &number, theme::mono_font(theme::TEXT_SMALL), text);
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let mut rows: Vec<Rect> =
                [parts.name, parts.output, parts.devices, parts.sends, parts.pan, parts.readout, parts.fader]
                    .into_iter()
                    .flatten()
                    .collect();
            rows.push(parts.toggles);
            for pair in rows.windows(2) {
                assert!(pair[0].bottom() <= pair[1].top(), "{height}: {pair:?}");
            }
            if let Some(fader) = parts.fader {
                assert!(fader.height() >= LEAST_FADER, "{height}: {fader:?}");
                let meter = parts.meter.unwrap_or(Rect::NOTHING);
                assert_eq!(meter.y_range(), fader.y_range());
            }
            if let Some(list) = parts.devices {
                assert!(list.height() >= LEAST_DEVICES * DEVICE_ROW, "{height}: {list:?}");
            }
            assert!(parts.toggles.bottom() <= height - PAD + 0.01);
        }
    }

    #[test]
    fn the_fader_grows_until_the_device_list_shows_then_the_list_takes_the_rest() {
        let tall = strip(600.0);
        assert_eq!(tall.fader.map(|f| f.height()), Some(TALLEST_FADER_WITH_DEVICES));
        let taller = strip(700.0);
        assert_eq!(taller.fader.map(|f| f.height()), Some(TALLEST_FADER_WITH_DEVICES));
        assert!(taller.devices.map(|d| d.height()) > tall.devices.map(|d| d.height()));
        // Without sends the fader takes their row.
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(theme::RETURN_COLUMN, 200.0));
        let without = layout(rect, false, false);
        assert!(without.sends.is_none());
        assert_eq!(without.fader.map(|f| f.height()), strip(200.0).fader.map(|f| f.height() + KNOB_ROW));
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
}
