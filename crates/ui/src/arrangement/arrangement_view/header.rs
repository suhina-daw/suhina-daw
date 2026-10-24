// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A track's header beside its lane: name, routing, inputs, meter and the colour band.

use super::*;

/// The output dropdown: "→ Main" or a group; picking one runs `track.setOutput`.
pub(super) fn routing(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    groups: &[(TrackId, String)],
    actions: &mut Vec<Action>,
) {
    let current = lane.output.and_then(|t| groups.iter().find(|(g, _)| *g == t)).map_or("Main", |(_, name)| name);
    let label = Rect::from_min_size(rect.min, Vec2::new(18.0, rect.height()));
    widgets::text_in(painter, label, Align2::LEFT_CENTER, "→", theme::ui_font(theme::TEXT_SMALL), theme::text_value());
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let mut picked = None;
    let response =
        widgets::button_area(ui, field, Id::new(("output", lane.track.0)), format!("{} output: {current}", lane.name));
    widgets::dropdown(ui, painter, field, &response, current);
    egui::Popup::menu(&response).show(|ui| {
        let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        if ui.selectable_label(lane.output.is_none(), text("Main")).clicked() && lane.output.is_some() {
            picked = Some(serde_json::Value::from("main"));
        }
        for (group, name) in groups.iter().filter(|(g, _)| *g != lane.track) {
            if ui.selectable_label(lane.output == Some(*group), text(name)).clicked() && lane.output != Some(*group) {
                picked = Some(serde_json::Value::from(group.0));
            }
        }
    });
    if let Some(to) = picked {
        actions.push(Action::new("track.setOutput", json!({ "track": lane.track.0, "to": to })));
    }
}

/// The MIDI input dropdown (ADR 0017): "MIDI" and what the track hears while armed; its menu
/// runs `track.setMidiInput`.
pub(super) fn midi_input(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    ports: &[String],
    actions: &mut Vec<Action>,
) {
    let current = menus::midi_input_text(&lane.midi_input);
    let label = Rect::from_min_size(rect.min, Vec2::new(28.0, rect.height()));
    widgets::text_in(
        painter,
        label,
        Align2::LEFT_CENTER,
        "MIDI",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_value(),
    );
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let response = widgets::button_area(
        ui,
        field,
        Id::new(("midi-input", lane.track.0)),
        format!("{} MIDI input: {current}", lane.name),
    );
    widgets::dropdown(ui, painter, field, &response, &current);
    egui::Popup::menu(&response).show(|ui| menus::midi_input_choices(ui, lane.track, &lane.midi_input, ports, actions));
}

/// The audio input dropdown (ADR 0017): "In" and the track's input channels and monitoring;
/// its menu runs `track.setAudioInput`.
pub(super) fn audio_input(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    channels: u16,
    actions: &mut Vec<Action>,
) {
    let current = menus::audio_input_text(&lane.audio_input);
    let label = Rect::from_min_size(rect.min, Vec2::new(28.0, rect.height()));
    widgets::text_in(
        painter,
        label,
        Align2::LEFT_CENTER,
        "Audio",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_value(),
    );
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let response = widgets::button_area(
        ui,
        field,
        Id::new(("audio-input", lane.track.0)),
        format!("{} audio input: {current}", lane.name),
    );
    widgets::dropdown(ui, painter, field, &response, &current);
    egui::Popup::menu(&response)
        .show(|ui| menus::audio_input_choices(ui, lane.track, &lane.audio_input, channels, actions));
}

/// Width of the meter lying in a return or main row, left of its volume.
pub(super) const LYING_METER: f32 = 48.0;

/// Where a return or main row shows its meter: lying down, ending at `right`, centred on `y`.
pub(super) fn lying_meter(right: f32, y: f32) -> Rect {
    Rect::from_min_max(Pos2::new(right - LYING_METER, y - meter::WIDTH / 2.0), Pos2::new(right, y + meter::WIDTH / 2.0))
}

/// A header's colour band: the track's colour down its left edge, the full height of the header
/// (dimmed while the track is off). A click opens the palette; gives the band's rectangle.
pub(super) fn colour_band(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    id: Id,
    actions: &mut Vec<Action>,
) -> Rect {
    let band = Rect::from_min_size(rect.min, Vec2::new(COLOUR_BAND, rect.height()));
    painter.rect_filled(band, 0.0, colour_picker::fill(lane.color, lane.mute));
    if band.intersects(clip) {
        colour_picker::area(ui, band.intersect(clip), id.with("colour"), lane.track, &lane.name, lane.color, actions);
    }
    band
}

/// Where a header stands among the drawn lanes: how many lanes are inside it (a group's fold
/// triangle shows when there are any) and the tracks in the order drawn, for a Shift-click's run.
pub(super) struct HeaderOrder<'a> {
    pub(super) children: usize,
    pub(super) tracks: &'a [TrackId],
}

/// The track list's groups, for a header's menu.
pub(super) struct Nest<'a> {
    /// Every track but the returns, in track order.
    pub(super) places: &'a [Place],
    /// The group tracks, with their names.
    pub(super) groups: &'a [(TrackId, String)],
}

#[allow(clippy::too_many_arguments)]
pub(super) fn track_header(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lanes_area: Rect,
    lane: &Lane,
    track_number: usize,
    nest: &Nest<'_>,
    order: &HeaderOrder<'_>,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let id = Id::new(("arrangement-header", lane.track.0));
    let clip = rect.intersect(lanes_area);

    // The header itself: click selects (Ctrl adds, Shift takes a run), right-click opens the
    // track's menu, a drag moves the track. Its controls, added after it, take their own clicks.
    let body = ui.interact(clip, id.with("body"), Sense::click_and_drag());
    body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Track {}", lane.name)));
    if body.clicked() {
        state.targeted = Some(lane.track);
        let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        state.pick_track(lane.track, shift, command, order.tracks);
        // The track, not a clip, is what Delete acts on now (#690); a clip on it stays selected
        // for the detail area.
        state.track_focus = true;
    }
    // A double-click shows the track's devices in the detail area.
    if body.double_clicked() {
        state.open_devices = Some(lane.track);
    }
    if body.drag_started_by(egui::PointerButton::Primary) {
        state.track_drag = Some(lane.track);
    }
    if body.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(dragged) = state.track_drag.take()
        && let Some(pointer) = body.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos())
    {
        state.track_drop = Some((dragged, pointer.y));
    }
    let selection = state.selected_tracks.clone();
    // Open until a click outside: its items close it themselves, and a colour dragged in its
    // picker does not (#712).
    egui::Popup::context_menu(&body).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        let into: Vec<(TrackId, String)> = nesting::move_targets(nest.places, lane.track)
            .into_iter()
            .filter_map(|g| nest.groups.iter().find(|(t, _)| *t == g).cloned())
            .collect();
        let menu = TrackMenu {
            track: lane.track,
            name: &lane.name,
            colour: lane.color,
            is_group: lane.kind == ColumnKind::Group,
            is_return: lane.kind == ColumnKind::Return,
            selection: &selection,
            into: &into,
            out: nesting::out_of_group(nest.places, lane.track),
            midi: lane.plays_notes.then_some((&lane.midi_input, timeline.midi_ports.as_slice())),
            audio: lane.plays_audio.then_some((&lane.audio_input, timeline.input_channels)),
        };
        menus::track_menu(ui, &menu, actions);
    });
    let selected = state.selected_tracks.contains(&lane.track) || state.focus == Some(lane.track);
    // The header in its track's colour; a selected track's is brighter and more saturated, with
    // an outline, so the selection reads at a glance (#690).
    let header_fill = if selected {
        motion::mix(theme::panel_background(), saturated(rgb(lane.color)), 0.85)
    } else {
        motion::mix(theme::panel_background(), rgb(lane.color), 0.6)
    };
    painter.rect_filled(rect, 0.0, header_fill);
    if selected {
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.5, saturated(rgb(lane.color))), StrokeKind::Inside);
    }
    // The track's colour runs down the header's left edge; a click there opens the palette.
    let band = colour_band(ui, painter, rect, clip, lane, id, actions);
    // A track inside groups has their colours down its indent, the outermost first, so a group and
    // its tracks read as one block: the group's colour runs from its header down all of them.
    let mut ancestors = Vec::new();
    let mut parent = lane.parent;
    while let Some(group) = parent.filter(|_| ancestors.len() < lane.depth) {
        let Some(group_lane) = timeline.lanes.iter().find(|l| l.track == group) else { break };
        ancestors.push(group_lane.color);
        parent = group_lane.parent;
    }
    for (level, color) in ancestors.iter().rev().enumerate() {
        let left = band.right() + level as f32 * INDENT;
        let strip = Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(left + INDENT - 2.0, rect.bottom()));
        if strip.intersects(clip) {
            painter.rect_filled(strip.intersect(clip), 0.0, rgb(*color));
        }
    }
    // The meter stands at the right edge; the rows keep clear of it.
    let meter_x = rect.right() - PAD - meter::WIDTH;
    let inner = Rect::from_min_max(
        Pos2::new(band.right() + PAD + lane.depth as f32 * INDENT, rect.top() + HEADER_PAD_Y),
        Pos2::new(meter_x - PAD, rect.bottom() - HEADER_PAD_Y),
    );
    let meter_bottom =
        if rect.height() >= SHOWS_MIXER { inner.top() + HEADER_ROW + PAD + FIELD } else { inner.top() + HEADER_ROW };
    let at = Rect::from_min_max(
        Pos2::new(meter_x, inner.top()),
        Pos2::new(meter_x + meter::WIDTH, meter_bottom.min(inner.bottom())),
    );
    let level = state.meters.track(lane.track);
    meter::show(ui, painter, at.intersect(clip), &lane.name, meter::Source::Track(lane.track), &level, actions);

    // Row 1: a group's fold triangle, track on, solo, cue, arm, name. Mixer controls act on the whole
    // selection while this track is part of one.
    let controlled: Vec<u32> = state.controlled_tracks(lane.track).iter().map(|t| t.0).collect();
    let fold = (lane.kind == ColumnKind::Group && order.children > 0)
        .then(|| Rect::from_min_size(inner.min, Vec2::new(FOLD, HEADER_ROW)));
    if let Some(fold) = fold {
        let folded = state.folds.is_folded(lane.track);
        let label = if folded { format!("Unfold {}", lane.name) } else { format!("Fold {}", lane.name) };
        let response = widgets::button_area(ui, fold.intersect(clip), id.with("fold"), label);
        let color = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &response));
        let center = fold.center();
        let (wide, tall) = (7.0, 4.0);
        // Pointing right while the group is folded, down while it is open.
        let points = if folded {
            vec![
                Pos2::new(center.x - tall / 2.0, center.y - wide / 2.0),
                Pos2::new(center.x - tall / 2.0, center.y + wide / 2.0),
                Pos2::new(center.x + tall / 2.0, center.y),
            ]
        } else {
            vec![
                Pos2::new(center.x - wide / 2.0, center.y - tall / 2.0),
                Pos2::new(center.x + wide / 2.0, center.y - tall / 2.0),
                Pos2::new(center.x, center.y + tall / 2.0),
            ]
        };
        painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
        if response.clicked() {
            state.folds.toggle(lane.track);
            // What the lanes are changes without the project changing: draw the next frame.
            ui.ctx().request_repaint();
        }
    }
    // Every header keeps the fold triangle's room, so a group's tracks line up under it and their
    // indent shows; a group without the triangle (no tracks yet) lines up the same.
    let first_x = inner.left() + FOLD + 2.0;
    let row = Rect::from_min_max(Pos2::new(first_x, inner.top()), Pos2::new(inner.right(), inner.top() + HEADER_ROW));
    let toggle = Rect::from_min_size(row.min, Vec2::new(ON_TOGGLE, HEADER_ROW));
    let solo = Rect::from_min_size(Pos2::new(toggle.right() + 2.0, row.top()), Vec2::new(SOLO, HEADER_ROW));
    let cue = Rect::from_min_size(Pos2::new(solo.right() + 2.0, row.top()), Vec2::new(SOLO, HEADER_ROW));
    // A track that can be armed has its arm button after cue, as wide.
    let arm =
        lane.armable.then(|| Rect::from_min_size(Pos2::new(cue.right() + 2.0, row.top()), Vec2::new(SOLO, HEADER_ROW)));
    let monitor_width = if rect.height() < SHOWS_MIXER { 24.0 } else { 32.0 };
    let monitor = lane.plays_notes.then(|| {
        Rect::from_min_size(
            Pos2::new(arm.unwrap_or(cue).right() + 2.0, row.top()),
            Vec2::new(monitor_width, HEADER_ROW),
        )
    });
    let controls_end = monitor.unwrap_or(arm.unwrap_or(cue)).right();
    // A lane tall enough for two rows has its volume and pan beside the buttons, where the
    // other level controls are, and the automation button, the name and the sends on the row
    // below, the button under the track-on toggle; a short lane keeps one row of buttons, name
    // and automation button.
    let tall = rect.height() >= SHOWS_MIXER;
    // Beside the buttons only where the volume bar keeps its room; a narrow header has the name
    // on the first row and the volume, the sends and the pan across the second, the bar as wide
    // as it can be.
    let beside_room = row.right() - PAN_FIELD - 4.0 - (controls_end + PAD);
    let side = tall && beside_room >= LEAST_VOLUME;
    let stacked = tall && !side;
    let below = Rect::from_min_max(
        Pos2::new(first_x, row.bottom() + PAD),
        Pos2::new(inner.right(), row.bottom() + PAD + FIELD),
    );
    // The automation button shows and hides the track's lanes.
    let lanes_button = if side {
        Rect::from_min_size(
            Pos2::new(below.left() + (ON_TOGGLE - SOLO) / 2.0, below.top()),
            Vec2::new(SOLO, HEADER_ROW),
        )
    } else {
        Rect::from_min_size(Pos2::new(row.right() - SOLO, row.top()), Vec2::new(SOLO, HEADER_ROW))
    };
    if lanes_button.intersects(clip) {
        automation::header_button(
            ui,
            painter,
            lanes_button,
            lane.track,
            &lane.name,
            &mut state.automation,
            &timeline.automation,
        );
    }
    // Two rows: the sends at the end of the lower row, as many as leave the name its room.
    let step = FIELD + SEND_GAP;
    let stacked_pan =
        Rect::from_min_max(Pos2::new(below.right() - PAN_FIELD, below.top()), Pos2::new(below.right(), below.bottom()));
    let shown_sends = if side {
        let room = ((below.right() - lanes_button.right() - PAD - LEAST_NAME) / step).floor().max(0.0) as usize;
        lane.sends.len().min(room)
    } else if stacked {
        let room = ((stacked_pan.left() - 4.0 - below.left() - LEAST_VOLUME) / step).floor().max(0.0) as usize;
        lane.sends.len().min(room)
    } else {
        0
    };
    let sends_left = if stacked {
        stacked_pan.left() - 4.0 - step * shown_sends as f32 + SEND_GAP
    } else {
        below.right() - step * shown_sends as f32 + SEND_GAP
    };
    let name = if side {
        Rect::from_min_max(
            Pos2::new(lanes_button.right() + PAD, below.top()),
            Pos2::new(sends_left - SEND_GAP - 4.0, below.top() + HEADER_ROW),
        )
    } else {
        Rect::from_min_max(Pos2::new(controls_end + PAD, row.top()), Pos2::new(lanes_button.left() - 4.0, row.bottom()))
    };
    let name_color = if lane.mute { theme::text_hint() } else { theme::text_primary() };
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &lane.name, theme::ui_font(theme::TEXT), name_color);
    // The name's place, for a rename in place (Ctrl+R, #696).
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| {
        d.insert_temp(Id::new((crate::app::NAME_RECT_TRACK, u64::from(lane.track.0))), (name.intersect(clip), pass));
    });
    if solo.intersects(clip)
        && let Some(action) =
            widgets::solo_button(ui, painter, solo, id.with("solo"), &controlled, &lane.name, lane.solo)
    {
        actions.push(action);
    }
    if cue.intersects(clip)
        && let Some(action) = widgets::cue_button(ui, painter, cue, id.with("cue"), lane.track.0, &lane.name, lane.cue)
    {
        actions.push(action);
    }
    if let Some(arm) = arm.filter(|a| a.intersects(clip))
        && let Some(action) =
            widgets::arm_button(ui, painter, arm, id.with("arm"), lane.track.0, &lane.name, lane.armed)
    {
        actions.push(action);
    }
    if toggle.intersects(clip) {
        let label = if lane.mute { format!("Turn {} on", lane.name) } else { format!("Turn {} off", lane.name) };
        let response = widgets::button_area(ui, toggle.intersect(clip), id.with("on"), label);
        if response.clicked() {
            actions.push(Action::new("track.setMute", json!({ "tracks": controlled, "mute": !lane.mute })));
        }
        let (fill, text) = widgets::toggle_colors(ui, &response, !lane.mute);
        painter.rect_filled(toggle, 0.0, fill);
        let number = if lane.kind == ColumnKind::Return {
            lane.name.chars().next().map(String::from).unwrap_or_default()
        } else {
            track_number.to_string()
        };
        widgets::text_in(painter, toggle, Align2::CENTER_CENTER, &number, theme::mono_font(theme::TEXT_SMALL), text);
    }

    if let Some(at) = monitor.filter(|at| at.intersects(clip)) {
        let mode = lane.midi_input.monitor;
        let text = menus::monitor_name(mode);
        let response = widgets::button_area(
            ui,
            at.intersect(clip),
            id.with("midi-monitor"),
            format!("MIDI monitoring of {}: {text}", lane.name),
        )
        .on_hover_text("Choose when you hear the MIDI input");
        let active = mode == suhina_engine::model::Monitor::In;
        let (fill, ink) = widgets::toggle_colors(ui, &response, active);
        painter.rect_filled(at, 0.0, fill);
        widgets::text_in(
            painter,
            at,
            Align2::CENTER_CENTER,
            text,
            theme::ui_font(if at.height() < 16.0 { 9.0 } else { 10.0 }),
            ink,
        );
        egui::Popup::menu(&response).show(|ui| menus::midi_monitor_choices(ui, lane.track, mode, actions));
    }
    // Two rows: volume and pan beside the buttons, the sends after the name below; or, narrow,
    // the volume, the sends and the pan across the second row.
    if tall {
        let (volume, pan) = if side {
            let left = controls_end + PAD;
            let pan =
                Rect::from_min_max(Pos2::new(row.right() - PAN_FIELD, row.top()), Pos2::new(row.right(), row.bottom()));
            (Rect::from_min_max(Pos2::new(left, row.top()), Pos2::new((pan.left() - 4.0).max(left), row.bottom())), pan)
        } else {
            let right = (sends_left - SEND_GAP).max(below.left());
            (Rect::from_min_max(below.min, Pos2::new(right, below.bottom())), stacked_pan)
        };
        value_control(ui, painter, volume, clip, lane, Control::Volume, false, id, state, actions);
        value_control(ui, painter, pan, clip, lane, Control::Pan, false, id, state, actions);
        for (index, send) in lane.sends.iter().take(shown_sends).enumerate() {
            let knob =
                Rect::from_min_size(Pos2::new(sends_left + step * index as f32, below.top()), Vec2::splat(FIELD));
            send_knob(ui, painter, knob, clip, lane, send, id, state, actions);
        }
    }

    // Row 3, taller still: where the track's audio goes.
    if rect.height() >= SHOWS_ROUTING {
        let top = row.bottom() + PAD + FIELD + PAD + 4.0;
        let line = Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + DROPDOWN));
        if line.intersects(clip) {
            routing(ui, painter, line, lane, nest.groups, actions);
        }
        // Taller still, on a track that plays notes: which controllers it hears when armed.
        let input = line.translate(Vec2::new(0.0, DROPDOWN + PAD));
        if rect.height() >= SHOWS_MIDI_INPUT && input.intersects(clip) {
            if lane.plays_notes {
                midi_input(ui, painter, input, lane, &timeline.midi_ports, actions);
            } else if lane.plays_audio {
                audio_input(ui, painter, input, lane, timeline.input_channels, actions);
            }
        }
    }

    // The bottom edge resizes the lane; a return's top edge, as the fixed rows sit along the
    // bottom and grow upward.
    let upward = lane.kind == ColumnKind::Return;
    let edge = if upward {
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() - 1.0), Pos2::new(rect.right(), rect.top() + 3.0))
    } else {
        Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 3.0), Pos2::new(rect.right(), rect.bottom() + 2.0))
    };
    if edge.intersects(lanes_area) {
        let response = ui
            .interact(edge.intersect(lanes_area), id.with("resize"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if response.dragged_by(egui::PointerButton::Primary) {
            let dy = response.drag_delta().y;
            let height = (state.height(lane) + if upward { -dy } else { dy }).clamp(MIN_LANE, MAX_LANE);
            state.heights.insert(lane.track, height);
        }
    }
}
