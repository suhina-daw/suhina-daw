// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Launcher view's channel strip: the part of a column under its slots, which shows more the
//! taller the mixer is (`docs/spec/ui-session-view.md` › Mixer, sizes and steps in
//! `docs/design/components.md` › Mixer strip). [`layout`] says where each part goes for a height;
//! [`show`] draws a column's strip and turns what is done there into [`Action`]s.

use crate::kit::surface::Backgrounds as _;
use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{InputChannels, Monitor, TrackId};

use crate::interaction::colour_picker;
use crate::interaction::commands;
use crate::interaction::grid::{Column, ColumnKind};
use crate::interaction::menus;
use crate::kit::control_layout::{self, Direction};
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
const FADER_COLUMN: f32 = 20.0;
const SCALE_WIDTH: f32 = 14.0;
const METER_GAP: f32 = 4.0;
const SEND_HEIGHT: f32 = 22.0;
/// Launcher meters use wider stereo bars than compact Arrangement headers.
pub const METER_WIDTH: f32 = 14.0;
const CAP: Vec2 = Vec2::new(20.0, 26.0);

/// The fader cap's size: `.fader-cap { width; height }` in the theme, else [`CAP`].
pub(crate) fn cap_size() -> Vec2 {
    let _style = crate::kit::style_class::enter("fader-cap");
    let computed = crate::kit::style_class::computed();
    Vec2::new(
        computed.as_ref().and_then(|c| c.width).filter(|w| *w >= 4.0).unwrap_or(CAP.x).min(64.0),
        computed.as_ref().and_then(|c| c.height).filter(|h| *h >= 4.0).unwrap_or(CAP.y).min(96.0),
    )
}

/// The fader rail's thickness: `.fader-rail { width }` in the theme, else 5 points.
pub(crate) fn rail_width() -> f32 {
    let _style = crate::kit::style_class::enter("fader-rail");
    crate::kit::style_class::computed().and_then(|c| c.width).filter(|w| *w >= 1.0).unwrap_or(5.0).min(32.0)
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
    /// The line between the device list and the controls under it: dragged, it shares the
    /// strip's height between them. A theme places it as `mixer-divider`.
    pub divider: Option<Rect>,
    pub sends: Option<Rect>,
    pub pan: Option<Rect>,
    /// The dB value over the fader.
    pub readout: Option<Rect>,
    pub fader: Option<Rect>,
    pub meter: Option<Rect>,
    /// The volume as a value across the strip, at heights too small for the fader.
    pub compact: Option<Rect>,
    pub toggles: Rect,
    /// A strip without an input keeps its input's room: the caption, source and monitor where
    /// they would be, so controls a theme anchors to them line up across every strip.
    pub reserved_input: Option<[Rect; 3]>,
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
/// sends leaves its footer empty; an audio track's strip holds the input's block above the
/// toggles from [`INPUT_AT`] up.
pub fn layout(rect: Rect, has_sends: bool, can_hear_input: bool) -> Layout {
    layout_shared(rect, has_sends, can_hear_input, Shared::default())
}

/// [`layout`], with what the mixer's strips share: the input block's room and the divider.
pub fn layout_shared(rect: Rect, has_sends: bool, can_hear_input: bool, shared: Shared) -> Layout {
    layout_native(rect, has_sends, can_hear_input, shared, false)
}

pub fn layout_main(rect: Rect, shared: Shared) -> Layout {
    layout_native(rect, false, false, shared, true)
}

fn layout_native(rect: Rect, has_sends: bool, can_hear_input: bool, shared: Shared, main: bool) -> Layout {
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("mixer layout");
    let inner = rect.shrink(PAD);
    let height = rect.height();
    // Reserve the footer on all strips so returns and Main keep their controls aligned.
    let footer = if height >= SENDS_AT { SEND_HEIGHT + SEND_GAP } else { 0.0 };
    let toggle_bottom = inner.bottom() - footer;
    let toggles = Rect::from_min_max(
        Pos2::new(inner.left(), (toggle_bottom - theme::TOGGLE).max(inner.top())),
        Pos2::new(inner.right(), toggle_bottom),
    );
    let mut parts = Layout {
        name: None,
        output: None,
        audio_caption: None,
        audio_source: None,
        audio_monitor: None,
        devices: None,
        divider: None,
        sends: None,
        pan: None,
        readout: None,
        fader: None,
        meter: None,
        compact: None,
        toggles,
        reserved_input: None,
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
        } else {
            parts.reserved_input = Some([caption, source, monitor]);
        }
    }
    let row = |top: f32, height: f32| {
        Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + height))
    };
    if height < FADER_AT {
        let top = (bottom - COMPACT).max(inner.top());
        parts.compact = Some(row(top, bottom - top)).filter(|r| r.height() > 0.0);
        return place_parts(parts, inner, main);
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
    if height >= SENDS_AT && has_sends {
        parts.sends = Some(row(toggles.bottom() + SEND_GAP, SEND_HEIGHT));
    }
    let fixed = READOUT + UNDER_READOUT + if pan { KNOB_ROW } else { 0.0 };
    let rest = (bottom - top - fixed).max(0.0);
    let fader = if height >= DEVICES_AT {
        // Devices, pan and fader align across tracks, returns and Main, including the send footer.
        let room = rest;
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
            fader
        } else {
            rest
        }
    } else {
        rest
    };
    if pan {
        parts.pan = Some(row(top, theme::KNOB));
        top += KNOB_ROW;
    }
    let centred = rect.width() <= theme::SCENE_COLUMN;
    let group_left =
        if rect.width() <= theme::SCENE_COLUMN { inner.center().x - FADER_COLUMN / 2.0 } else { inner.left() };
    let column =
        |top: f32, height: f32| Rect::from_min_size(Pos2::new(group_left, top), Vec2::new(FADER_COLUMN, height));
    // The readout spans the strip, so "-12.0" is never cut; it starts over the fader.
    parts.readout = Some(Rect::from_min_max(Pos2::new(group_left, top), Pos2::new(inner.right(), top + READOUT)));
    top += READOUT + UNDER_READOUT;
    let fader = column(top, fader.min(bottom - top).max(0.0));
    parts.meter = Some(Rect::from_min_size(
        Pos2::new(fader.right() + if centred { 0.0 } else { SCALE_WIDTH } + METER_GAP, fader.top()),
        Vec2::new(METER_WIDTH, fader.height()),
    ));
    parts.fader = Some(fader);
    place_parts(parts, inner, main)
}

fn horizontal_volume() -> bool {
    control_layout::placement("mixer-volume").direction == Direction::Horizontal
}

fn place_parts(mut parts: Layout, bounds: Rect, main: bool) -> Layout {
    parts.divider = parts.devices.map(|list| {
        let line = list.bottom() + UNDER_DEVICES / 2.0;
        Rect::from_x_y_ranges(bounds.x_range(), egui::Rangef::new(line - 3.0, line + 3.0))
    });
    if horizontal_volume()
        && let Some(fader) = parts.fader
    {
        let horizontal = Rect::from_min_size(
            egui::pos2(bounds.left(), fader.top()),
            Vec2::new(bounds.width(), 20.0_f32.min(fader.height())),
        );
        parts.fader = Some(horizontal);
        parts.meter = Some(Rect::from_min_size(
            egui::pos2(bounds.left(), horizontal.bottom() + 12.0),
            Vec2::new(bounds.width(), 10.0),
        ));
    }
    // Placeholders: resolved, so anchors find them, but neither drawn nor offered for editing.
    if let Some(reserved) = parts.reserved_input.as_mut() {
        for (name, rect) in ["mixer-input-caption", "mixer-input", "mixer-monitor"].into_iter().zip(reserved.iter_mut())
        {
            *rect = control_layout::resolve(name, *rect, bounds);
        }
    }
    for (name, part) in [
        ("mixer-name", &mut parts.name),
        ("mixer-output", &mut parts.output),
        ("mixer-input-caption", &mut parts.audio_caption),
        ("mixer-input", &mut parts.audio_source),
        ("mixer-monitor", &mut parts.audio_monitor),
        // The divider before the list, so the list can end where the theme places it.
        ("mixer-divider", &mut parts.divider),
        ("mixer-devices", &mut parts.devices),
        ("mixer-sends", &mut parts.sends),
        ("mixer-pan", &mut parts.pan),
        ("mixer-volume-readout", &mut parts.readout),
        ("mixer-volume", &mut parts.fader),
        ("mixer-meter", &mut parts.meter),
        ("mixer-compact-volume", &mut parts.compact),
    ] {
        *part = part.filter(|_| !control_layout::placement(name).hidden).map(|rect| {
            if main
                && !matches!(
                    name,
                    "mixer-devices" | "mixer-divider" | "mixer-volume-readout" | "mixer-volume" | "mixer-meter"
                )
            {
                control_layout::resolve(name, rect, bounds)
            } else {
                control_layout::place(name, rect, bounds)
            }
        });
    }
    parts.toggles = control_layout::place("mixer-toggles", parts.toggles, bounds);
    drop_crowded(&mut parts);
    parts
}

/// An anchored part that the strip's height has pushed onto another part draws nothing, rather
/// than over it: when a short strip leaves the chains from the top and the bottom no room, the
/// anchored part gives way, so nothing jumps between frames.
fn drop_crowded(parts: &mut Layout) {
    let anchored = |name: &str| !control_layout::placement(name).anchors.is_empty();
    let names = [
        "mixer-name",
        "mixer-output",
        "mixer-input-caption",
        "mixer-input",
        "mixer-monitor",
        "mixer-devices",
        "mixer-sends",
        "mixer-pan",
        "mixer-volume-readout",
        "mixer-volume",
        "mixer-meter",
    ];
    if !names.iter().any(|name| anchored(name)) {
        return;
    }
    let rects = [
        parts.name,
        parts.output,
        parts.audio_caption,
        parts.audio_source,
        parts.audio_monitor,
        parts.devices,
        parts.sends,
        parts.pan,
        parts.readout,
        parts.fader,
        parts.meter,
    ];
    let mut crowded = [false; 11];
    for (i, (name, rect)) in names.iter().zip(rects).enumerate() {
        let Some(rect) = rect.filter(|_| anchored(name)) else { continue };
        // A part over another one it is not inside of (the meter stands in the fader's column).
        crowded[i] = rects.iter().enumerate().any(|(j, other)| {
            other.is_some_and(|other| {
                j != i
                    && !crowded[j]
                    && rect.intersect(other).height() > 1.0
                    && rect.intersect(other).width() > 1.0
                    && !other.contains_rect(rect)
                    && !rect.contains_rect(other)
            })
        }) || rect.intersect(parts.toggles).height() > 1.0 && rect.intersect(parts.toggles).width() > 1.0;
    }
    let slots = [
        &mut parts.name,
        &mut parts.output,
        &mut parts.audio_caption,
        &mut parts.audio_source,
        &mut parts.audio_monitor,
        &mut parts.devices,
        &mut parts.sends,
        &mut parts.pan,
        &mut parts.readout,
        &mut parts.fader,
        &mut parts.meter,
    ];
    for (slot, crowded) in slots.into_iter().zip(crowded) {
        if crowded {
            *slot = None;
        }
    }
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
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("mixer strip");
    let clipped_painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let painter = &clipped_painter;
    let _artwork =
        crate::kit::artwork::track_color(egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2));
    let _style_class = crate::kit::style_class::enter_named("mixer", &format!("track-{}", column.track.0));
    crate::kit::native_css::set_variable(
        "--track-colour",
        crate::kit::native_css::hex(egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2)),
    );
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Selected, state.selected == Some(column.track));
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Muted, column.mute);
    // The strip in a shade of its track's colour, as the column above it.
    let tint = crate::kit::style_class::background(motion::mix(
        theme::panel_background(),
        egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2),
        0.16,
    ));
    let strip = ui.interact(rect.intersect(painter.clip_rect()), id.with("strip"), Sense::click());
    if !crate::kit::skin::paint_colored(
        painter,
        rect,
        "track.body",
        crate::kit::skin::State::response(&strip, state.selected == Some(column.track)),
        tint,
        egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2),
    ) {
        painter.fill_background(rect, 0.0, tint);
    }
    crate::kit::liquid::paint(painter, rect, tint, crate::kit::liquid::Surface::Track);
    crate::kit::native_css::background(painter, rect);
    // The strip's own background, under its controls: a double-click shows the track's devices.
    strip.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{} channel strip", column.name))
    });
    if strip.double_clicked() {
        state.open_devices = Some(column.track);
    }
    // A click on the strip selects its track, a return's too; a slot selected on that track stays.
    if strip.clicked() {
        state.selected = Some(column.track);
        state.selected_tracks = vec![column.track];
        state.devices_clicked = true;
        if state.selected_slot.is_some_and(|(track, _)| track != column.track) {
            state.selected_slot = None;
        }
    }
    // The track's colour down the left edge, at every height; a click opens the palette.
    let band = control_layout::place(
        "mixer-color",
        Rect::from_min_size(rect.min, Vec2::new(COLOUR_BAND, rect.height())),
        rect,
    );
    if !control_layout::placement("mixer-color").hidden {
        painter.fill_background(band, 0.0, colour_picker::fill(column.color, column.mute));
    }
    let band_hit = band.intersect(painter.clip_rect());
    if band_hit.is_positive() && !control_layout::placement("mixer-color").hidden {
        let colour_id = id.with("strip-colour");
        colour_picker::area(ui, band_hit, colour_id, column.track, &column.name, column.color, actions);
    }
    let shared = Shared { reserve_input: state.reserve_input, fader: state.strip_fader_shown() };
    let parts = layout_shared(rect, !column.sends.is_empty(), column.plays_audio, shared);
    if let Some(name) = parts.name {
        name_row(ui, painter, name, column, id, state.selected == Some(column.track));
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
        fader_column(ui, painter, parts.readout.unwrap_or(Rect::NOTHING), fader, column, state, id, actions);
    }
    if let Some(compact) = parts.compact {
        compact_volume(ui, painter, compact, column, state, id, actions);
    }
    if let Some(at) = parts.meter {
        let level = state.meters.track(column.track);
        meter::show(ui, painter, at, &column.name, meter::Source::Track(column.track), &level, actions);
    }
    if !control_layout::placement("mixer-toggles").hidden {
        toggles(ui, painter, parts.toggles, column, track_number, id, actions);
    }
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
        let _style = crate::kit::style_class::enter_named_response("divider", "mixer-divider", &response);
        painter.hline(
            rect.x_range(),
            line,
            egui::Stroke::new(2.0, crate::kit::style_class::foreground(theme::accent())),
        );
    }
}

/// The track's name at the top of a tall strip; screen readers and tests find it as a label.
fn name_row(ui: &Ui, painter: &egui::Painter, rect: Rect, column: &Column, id: Id, selected: bool) {
    let response = ui.interact(rect.intersect(painter.clip_rect()), id.with("strip-name"), Sense::hover());
    let _name_style =
        crate::kit::style_class::enter_named_response("track-name", &format!("track-{}", column.track.0), &response);
    crate::kit::native_css::set_variable(
        "--track-fill",
        crate::kit::native_css::hex(colour_picker::fill(column.color, column.mute)),
    );
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Selected, selected);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{} strip", column.name)));
    // The name stands on the track's colour, as in the column's header.
    let native_fill = colour_picker::fill(column.color, column.mute);
    // An alpha material layers over the track colour. An explicitly authored
    // transparent solid background can still remove that colour completely.
    let fill =
        if crate::kit::style_class::computed().is_some_and(|style| style.has_gradient() && style.background.a() == 0) {
            native_fill
        } else {
            crate::kit::style_class::background(native_fill)
        };
    if !crate::kit::skin::paint_colored(
        painter,
        rect,
        "track.name",
        crate::kit::skin::State::response(&response, selected),
        fill,
        egui::Color32::from_rgb(column.color.0, column.color.1, column.color.2),
    ) {
        painter.fill_background(rect, 0.0, fill);
    }
    crate::kit::liquid::paint(painter, rect, fill, crate::kit::liquid::Surface::Track);
    crate::kit::native_css::background(painter, rect);
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    widgets::text_in(
        painter,
        text,
        Align2::LEFT_CENTER,
        &column.name,
        theme::ui_font(theme::TEXT),
        // Ink that reads on the fill, unless the sheet gives the name a colour.
        crate::kit::style_class::foreground(theme::text_on(fill)),
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
    painter.fill_background(rect, 0.0, theme::window_background());
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
    painter.fill_background(rect, 0.0, theme::window_background());
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
    let mut slots = Vec::new();
    for (row_index, device) in column.devices.iter().enumerate() {
        let index = device.at;
        let top = rect.top() - scroll + row_index as f32 * DEVICE_ROW;
        let row = Rect::from_min_size(Pos2::new(rect.left(), top), Vec2::new(rect.width(), DEVICE_ROW));
        slots.push((index, row));
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
        let fill = if lit {
            theme::control_active()
        } else {
            motion::mix(theme::window_background(), theme::control_hover(), widgets::hover(ui, &response))
        };
        list_painter.fill_background(row, 0.0, fill);
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
    // A device of this track carried over its list: a line where it would go, and its place
    // for the drop (#688).
    let carrying = ui.data(|d| {
        d.get_temp::<(suhina_engine::model::TrackId, usize)>(Id::new(crate::interaction::drop::DEVICE_CARRYING))
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
        list_painter.hline(rect.x_range(), y, egui::Stroke::new(2.0, theme::accent()));
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
        let name = format!("mixer-monitor-{value}");
        if control_layout::placement(&name).hidden {
            continue;
        }
        let area = control_layout::place(&name, area, painter.clip_rect());
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
        painter.fill_background(area, 0.0, fill);
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
    match widgets::drag_value_axis(
        ui,
        response,
        &mut preview,
        column.volume_db,
        0.25,
        1.0,
        VOLUME_RANGE,
        0.0,
        horizontal_volume(),
    ) {
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
    state.value_areas.extend([readout, fader].into_iter().filter(Rect::is_positive));
    let label = widgets::format_db(shown(state, column, Control::Volume, column.volume_db));
    let response =
        volume_response(ui, painter, readout, fader, id.with("volume"), &format!("{} volume", column.name), &label);
    let db = volume_drag(ui, &response, column, state, actions);
    paint_fader(ui, painter, readout, fader, db, &response, column.kind == ColumnKind::Return);
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
    state.value_areas.extend([readout, fader].into_iter().filter(Rect::is_positive));
    let mut preview = match state.drag {
        Some((t, Control::Volume, v)) if t == MAIN => Some(v),
        _ => None,
    };
    let label = widgets::format_db(preview.unwrap_or(volume_db));
    let response = volume_response(ui, painter, readout, fader, Id::new("main-volume"), "Main volume", &label);
    let continued = preview.is_some();
    let send = |v: f64, continued: bool| {
        Action::new(
            "mixer.setMainVolume",
            json!({ "db": commands::db_param(v, VOLUME_RANGE.0), "continue": continued }),
        )
    };
    let mut db = volume_db;
    match widgets::drag_value_axis(
        ui,
        &response,
        &mut preview,
        volume_db,
        0.25,
        1.0,
        VOLUME_RANGE,
        0.0,
        horizontal_volume(),
    ) {
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
    paint_fader(ui, painter, readout, fader, db, &response, true);
}

/// Where a drag on the main fader is kept in [`SessionViewState::drag`]: no track has this id.
pub const MAIN: TrackId = TrackId(u32::MAX);

/// A fader's look: the dB value over it, the track, the level and the cap.
fn paint_fader(
    ui: &Ui,
    painter: &egui::Painter,
    readout: Rect,
    fader: Rect,
    db: f64,
    response: &egui::Response,
    centred: bool,
) {
    let _style = crate::kit::style_class::enter_named_response("fader", "mixer-volume", response);
    crate::kit::native_css::background(painter, fader);
    let hover = widgets::hover(ui, response);
    let value_color = motion::mix(theme::text_value(), theme::text_primary(), hover);
    if readout.is_positive() {
        let placed = horizontal_volume() || control_layout::placement("mixer-volume-readout") != Default::default();
        let area = if placed {
            readout
        } else {
            Rect::from_center_size(
                Pos2::new(readout.left() + FADER_COLUMN / 2.0, readout.center().y),
                Vec2::new(FADER_COLUMN, readout.height()),
            )
        };
        // `#mixer-volume-readout`: its own background, corners, `font-size`, `color` and number format.
        let _readout = crate::kit::style_class::enter_named("value", "mixer-volume-readout");
        crate::kit::native_css::surface(painter, area, 0.0, egui::Color32::TRANSPARENT);
        let size = crate::kit::style_class::font_size(9.0);
        let ink = crate::kit::style_class::foreground(value_color);
        let text = widgets::formatted_value(&widgets::format_db(db));
        painter.text(area.center(), Align2::CENTER_CENTER, text, theme::mono_font(size), ink);
    }
    if horizontal_volume() {
        paint_horizontal_fader(painter, fader, db, hover, response);
        return;
    }
    let cap_size = cap_size();
    if fader.height() < cap_size.y {
        return;
    }
    // The cap's centre travels from half a cap above the bottom to half a cap under the top.
    let travel = egui::Rangef::new(fader.bottom() - cap_size.y / 2.0, fader.top() + cap_size.y / 2.0);
    let at = |position: f32| travel.min + (travel.max - travel.min) * position;
    let x = fader.center().x;
    // An unfilled recessed rail: volume is the cap's position, not a bright background bar.
    let rail = Rect::from_center_size(Pos2::new(x, fader.center().y), Vec2::new(rail_width(), fader.height()));
    let recess = motion::mix(theme::control_background(), egui::Color32::BLACK, 0.65);
    if !crate::kit::skin::paint(
        painter,
        rail,
        "fader.rail",
        crate::kit::skin::State::response(response, false),
        theme::text_value(),
        0.0,
    ) {
        painter.fill_background(rail, 2.0, recess);
    }
    let _scale_style = crate::kit::style_class::enter("fader-scale");
    let native_scale = Rect::from_min_size(
        egui::pos2(if centred { fader.left() - SCALE_WIDTH - 2.0 } else { fader.right() }, fader.top()),
        egui::vec2(SCALE_WIDTH, fader.height()),
    );
    let scale = control_layout::place("mixer-volume-scale", native_scale, painter.clip_rect());
    let mut last_label_y = f32::NEG_INFINITY;
    for db in
        [0.0, -6.0, -12.0, -24.0, -70.0].into_iter().filter(|_| !control_layout::placement("mixer-volume-scale").hidden)
    {
        let y = (scale.top()
            + (at(widgets::volume_position(db)) - native_scale.top()) * scale.height() / native_scale.height())
        .round()
            + 0.5;
        if y - last_label_y >= 10.0 {
            let label = if db == -70.0 { "−∞".to_owned() } else { format!("{db:.0}") };
            painter.text(
                Pos2::new(scale.right() - if centred { 0.0 } else { 1.0 }, y),
                Align2::RIGHT_CENTER,
                label,
                theme::mono_font(8.0),
                theme::text_hint(),
            );
            last_label_y = y;
        }
        let length = if db == 0.0 { 5.0 } else { 3.0 };
        let ink = theme::text_hint();
        let shift = scale.left() - native_scale.left();
        painter.hline(
            egui::Rangef::new(rail.right() + 2.0 + shift, rail.right() + length + 2.0 + shift),
            y,
            theme::hairline(ink),
        );
    }
    drop(_scale_style);
    let level = at(widgets::volume_position(db));
    let cap = Rect::from_center_size(Pos2::new(x, level), cap_size);
    crate::kit::mixer_controls::fader_cap(painter, cap, hover, crate::kit::skin::State::response(response, false));
}

fn paint_horizontal_fader(painter: &egui::Painter, fader: Rect, db: f64, hover: f32, response: &egui::Response) {
    let cap_size = cap_size();
    let length = cap_size.y.min(fader.width());
    let thickness = cap_size.x.min(fader.height());
    let at = |position: f32| fader.left() + length / 2.0 + (fader.width() - length) * position;
    let state = crate::kit::skin::State::response(response, false);
    // Rasterize the same vertical artwork, then rotate its geometry and texture together.
    let rail = Rect::from_center_size(fader.center(), Vec2::new(rail_width(), fader.width()));
    if !crate::kit::skin::paint(painter, rail, "fader.rail", state, theme::text_value(), std::f32::consts::FRAC_PI_2) {
        painter.fill_background(
            Rect::from_center_size(fader.center(), Vec2::new(fader.width(), 5.0)),
            2.0,
            theme::control_background(),
        );
    }
    let mut previous = f32::NEG_INFINITY;
    let native_scale = Rect::from_min_size(egui::pos2(fader.left(), fader.bottom()), egui::vec2(fader.width(), 12.0));
    let scale = control_layout::place("mixer-volume-scale", native_scale, painter.clip_rect());
    let _scale_style = crate::kit::style_class::enter("fader-scale");
    for value in
        [-70.0, -24.0, -12.0, -6.0, 0.0].into_iter().filter(|_| !control_layout::placement("mixer-volume-scale").hidden)
    {
        let x = scale.left()
            + (at(widgets::volume_position(value)) - native_scale.left()) * scale.width() / native_scale.width();
        if x - previous < 18.0 {
            continue;
        }
        previous = x;
        painter.vline(x, scale.top()..=scale.top() + 3.0, theme::hairline(theme::text_hint()));
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
            theme::text_hint(),
        );
    }
    drop(_scale_style);
    let centre = egui::pos2(at(widgets::volume_position(db)), fader.center().y);
    let cap = Rect::from_center_size(centre, Vec2::new(thickness, length));
    let _cap_style = crate::kit::style_class::enter("fader-cap");
    crate::kit::native_css::background_rounded(
        painter,
        Rect::from_center_size(centre, Vec2::new(length, thickness)),
        2.0,
    );
    if !crate::kit::skin::paint(painter, cap, "fader.cap", state, theme::text_value(), std::f32::consts::FRAC_PI_2) {
        let cap = Rect::from_center_size(centre, Vec2::new(length, thickness));
        painter.fill_background(cap, 2.0, motion::mix(theme::control_background(), theme::text_value(), hover * 0.3));
        painter.vline(centre.x, cap.shrink(2.0).y_range(), theme::hairline(theme::text_value()));
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
    let db = volume_drag(ui, &response, column, state, actions);
    let hover = widgets::hover(ui, &response);
    let position = crate::kit::widgets::volume_position(db);
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    let font = theme::mono_font(theme::TEXT_SMALL);
    if state.knobs {
        let size = rect.height();
        let center = Pos2::new(rect.left() + size / 2.0, rect.center().y);
        widgets::knob(
            painter,
            center,
            size,
            position,
            motion::mix(theme::text_value(), theme::text_primary(), hover),
            Some(&response),
        );
    } else {
        painter.fill_background(rect, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
        let x = rect.left() + rect.width() * position;
        painter.fill_background(
            Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())),
            0.0,
            theme::control_playing(),
        );
    }
    widgets::text_in(painter, text, Align2::RIGHT_CENTER, &widgets::format_db(db), font, theme::text_primary());
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
    let _style = crate::kit::style_class::enter_named("pan", "mixer-pan");
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
    if crate::kit::style_class::knob_preference(state.knobs) {
        let size = row.height().min(row.width());
        widgets::knob(
            painter,
            Pos2::new(row.left() + size / 2.0, row.center().y),
            size,
            ((pan.clamp(-1.0, 1.0) + 1.0) / 2.0) as f32,
            theme::text_value(),
            Some(&pan_response),
        );
        if row.width() >= size + 20.0 {
            widgets::text_in(
                painter,
                Rect::from_min_max(egui::pos2(row.left() + size + 4.0, row.top()), row.max),
                Align2::CENTER_CENTER,
                &widgets::format_pan(pan),
                theme::mono_font(theme::TEXT_SMALL),
                theme::text_primary(),
            );
        }
    } else {
        crate::kit::mixer_controls::pan_field(painter, row, pan, widgets::hover(ui, &pan_response));
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
    // `.send { width; height }` sizes every send, within the row.
    let sheet = {
        let _send = crate::kit::style_class::enter("send");
        crate::kit::style_class::computed()
    };
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
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(x, row.bottom() - 1.0), Vec2::new(width, 1.0)),
            1.0,
            theme::text_hint(),
        );
    }
    for (index, send) in column.sends.iter().enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(row.left() + pitch * index as f32 - scroll, row.top()),
            Vec2::new(width, height),
        );
        let name = format!("mixer-send-{}", index + 1);
        if control_layout::placement(&name).hidden {
            continue;
        }
        let rect = control_layout::place(&name, rect, clip);
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
        // `.send` styles every send, `#mixer-send-N` one of them; `--suhina-appearance: knob`
        // draws it as a knob.
        let _style = crate::kit::style_class::enter_named_response("send", &name, &response);
        let pointer = crate::kit::style_class::foreground(motion::mix(
            theme::text_value(),
            theme::text_primary(),
            widgets::hover(ui, &response),
        ));
        if crate::kit::style_class::knob_preference(false) {
            // The value under the knob unless `--suhina-value-position` says otherwise.
            let text = format!("{letter} {}", widgets::format_db(shown));
            let under = widgets::ValuePosition::Bottom;
            widgets::knob_with_value(&painter, rect, position, pointer, Some(&response), &text, pointer, under, 7.0);
        } else {
            let state = crate::kit::skin::State::response(&response, false);
            crate::kit::native_css::surface(&painter, rect, 2.0, egui::Color32::TRANSPARENT);
            crate::kit::skin::paint(&painter, rect, "send", state, pointer, 0.0);
            crate::kit::mixer_controls::send_field(&painter, rect, &letter, shown, send.pre_fader, pointer, position);
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
    if control_layout::placement("mixer-enable").hidden {
        return;
    }
    let toggle = control_layout::place("mixer-enable", toggle, painter.clip_rect());
    let response = widgets::button_area(ui, toggle, id.with("on"), label);
    let _style = crate::kit::style_class::enter_named_response("button", "track-on", &response);
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Muted, column.mute);
    if response.clicked() {
        actions.push(Action::new("track.setMute", json!({ "tracks": [track.0], "mute": !column.mute })));
    }
    let (fill, text) = widgets::toggle_colors(ui, &response, !column.mute);
    widgets::button_surface(painter, toggle, &response, !column.mute, fill, 0.0);
    let text = crate::kit::style_class::foreground(text);
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
