// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The volume, pan and send controls in a track's header.

use super::*;

/// A header's send knob to one return: dragged up or down for the level (`track.setSend`, sent
/// when let go), double-click for none, right-click for before or after the fader
/// (`track.setSendMode`), as the Session view's strip's.
#[allow(clippy::too_many_arguments)]
pub(super) fn send_knob(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    send: &crate::interaction::grid::SendKnob,
    id: Id,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    if !rect.intersects(clip) {
        return;
    }
    let track = lane.track;
    let mode = if send.pre_fader { "pre" } else { "post" };
    state.value_areas.push(rect.intersect(clip));
    let response = widgets::value_area(
        ui,
        rect.intersect(clip),
        id.with(("send", send.target.0)),
        format!("{} send to {}", lane.name, send.name),
        format!("{} {mode}", widgets::format_db(send.level_db)),
    );
    let mut preview = match state.send_drag {
        Some((t, r, v)) if t == track && r == send.target => Some(v),
        _ => None,
    };
    let level = if send.level_db.is_finite() { send.level_db } else { VOLUME_RANGE.0 };
    let mut committed = None;
    // Heard while it is dragged, in one undo step (#705).
    let continued = preview.is_some();
    match widgets::drag_value(ui, &response, &mut preview, level, 0.25, 1.0, VOLUME_RANGE, VOLUME_RANGE.0) {
        Drag::Moving(v) => {
            state.send_drag = Some((track, send.target, v));
            actions.push(commands::set_send(track, send.target, v, continued));
        }
        Drag::Done(v) => {
            state.send_drag = None;
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
    let shown = match state.send_drag {
        Some((t, r, v)) if t == track && r == send.target => v,
        _ => committed.unwrap_or(level),
    };
    let position = ((shown - VOLUME_RANGE.0) / (VOLUME_RANGE.1 - VOLUME_RANGE.0)) as f32;
    let pointer = motion::mix(theme::text_value(), theme::text_primary(), widgets::hover(ui, &response));
    widgets::knob(painter, rect.center(), FIELD, position, pointer);
    // The return's letter in the knob, and a dot under it when the send is before the fader.
    let letter = send.name.chars().next().map(String::from).unwrap_or_default();
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &letter,
        theme::mono_font(theme::TEXT_SMALL - 2.0),
        theme::text_hint(),
    );
    if send.pre_fader {
        painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() + 1.5), 1.5, theme::text_value());
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn value_control(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    control: Control,
    plain: bool,
    id: Id,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let track = lane.track;
    let (value, per_point, per_tick, range, default) = match control {
        Control::Volume => (lane.volume_db, 0.25, 1.0, VOLUME_RANGE, 0.0),
        Control::Pan => (lane.pan, 0.01, 0.02, (-1.0, 1.0), 0.0),
    };
    // The tracks this drag moves: one track, or the whole selection it is part of.
    let tracks = state.controlled_tracks(track);
    let mut preview = state.value_drag.as_ref().filter(|d| d.track == track && d.control == control).map(|d| d.now);
    // Another track of the same selection moves with the drag, by the same amount: what it shows
    // is its own value moved by the drag's offset, before the command lands.
    let dragged = match state.value_drag.as_ref() {
        Some(drag) if drag.control == control && drag.tracks.contains(&track) => value + (drag.now - drag.start),
        _ => value,
    };
    let shown = preview.unwrap_or(dragged);
    let mut hover = 0.0;
    let mut committed = None;
    if rect.intersects(clip) {
        // Where the field is, so the wheel over it moves the value instead of scrolling the view.
        state.value_areas.push(rect);
        let (label, text) = match control {
            Control::Volume => (format!("{} volume", lane.name), widgets::format_db(shown)),
            Control::Pan => (format!("{} pan", lane.name), widgets::format_pan(shown)),
        };
        let response = widgets::value_area(ui, rect.intersect(clip), id.with(label.as_str()), &label, text);
        hover = widgets::hover(ui, &response);
        // One track hears every frame of a drag: each sets the value, and all but the first
        // continue its undo step, so the drag is one step. A selection of several moves together
        // when the drag ends, by the same amount (`value_action`).
        let continued = preview.is_some();
        let one = tracks.len() == 1;
        let set = |v: f64| match control {
            Control::Volume => commands::set_volume(&[track.0], v, Some(continued)),
            Control::Pan => Action::new(
                "track.setPan",
                json!({ "tracks": [track.0], "pan": (v * 100.0).round() / 100.0, "continue": continued }),
            ),
        };
        match widgets::drag_value(ui, &response, &mut preview, value, per_point, per_tick, range, default) {
            Drag::Moving(v) => {
                state.value_drag = Some(ValueDrag { track, control, start: value, now: v, tracks: tracks.clone() });
                if one {
                    actions.push(set(v));
                }
            }
            Drag::Done(v) => {
                state.value_drag = None;
                committed = Some(v);
                actions.push(if one { set(v) } else { value_action(control, &tracks, value, v) });
            }
            Drag::None => {}
        }
    }
    // A drag (and the value it commits) follows the pointer directly and keeps the animation
    // there, so nothing glides back afterwards; other changes (an undo, say) glide.
    let anim = id.with(("shown", control as u8));
    let dragged = match state.value_drag.as_ref() {
        Some(drag) if drag.control == control && drag.tracks.contains(&track) => Some(drag.now),
        _ => committed,
    };
    let shown = match dragged {
        Some(v) => {
            motion::jump(ui.ctx(), anim, v as f32);
            v
        }
        None if value.is_finite() => f64::from(motion::value(ui.ctx(), anim, value as f32, motion::VALUE)),
        None => value,
    };
    match control {
        Control::Volume if plain => volume_text(painter, rect, shown, hover),
        Control::Volume if state.knobs => volume_knob(painter, rect, shown, hover),
        Control::Volume => volume_field(painter, rect, shown, hover),
        Control::Pan if state.knobs => pan_knob(painter, rect, shown),
        Control::Pan => pan_field(painter, rect, shown, hover),
    }
}

/// A volume as a small knob with its value beside it, for Settings › Session and mixer ›
/// Mixer controls: Knob.
pub(super) fn volume_knob(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    let size = rect.height().min(theme::KNOB);
    let center = Pos2::new(rect.left() + size / 2.0 + 2.0, rect.center().y);
    let pointer = motion::mix(theme::text_value(), theme::text_primary(), hover);
    widgets::knob(painter, center, size, volume_position(db), pointer);
    let text = Rect::from_min_max(Pos2::new(center.x + size / 2.0 + 4.0, rect.top()), rect.max);
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}

/// A pan as a knob in the middle of its field, with the value at its right.
pub(super) fn pan_knob(painter: &egui::Painter, rect: Rect, pan: f64) {
    let size = rect.height().min(theme::KNOB);
    let center = Pos2::new(rect.left() + size / 2.0 + 2.0, rect.center().y);
    widgets::knob(painter, center, size, ((pan + 1.0) / 2.0) as f32, theme::text_primary());
    let text = Rect::from_min_max(Pos2::new(center.x + size / 2.0 + 4.0, rect.top()), rect.max);
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
}

/// The command a finished volume or pan drag runs: one track is set to the value the drag reached,
/// and a selection of several moves together by the same amount, keeping their differences. A move
/// that cannot be counted in dB (a track at silence, whose value is -inf) is set outright.
pub(super) fn value_action(control: Control, tracks: &[TrackId], from: f64, to: f64) -> Action {
    let ids: Vec<u32> = tracks.iter().map(|t| t.0).collect();
    let one = tracks.len() == 1;
    let delta = to - from;
    match control {
        Control::Volume if !one && delta.is_finite() => {
            let db = if to <= VOLUME_RANGE.0 { -200.0 } else { (delta * 10.0).round() / 10.0 };
            Action::new("track.adjustVolume", json!({ "tracks": ids, "db": db }))
        }
        Control::Volume => commands::set_volume(&ids, to, None),
        Control::Pan if !one && delta.is_finite() => {
            Action::new("track.adjustPan", json!({ "tracks": ids, "pan": (delta * 100.0).round() / 100.0 }))
        }
        Control::Pan => Action::new("track.setPan", json!({ "tracks": ids, "pan": (to * 100.0).round() / 100.0 })),
    }
}

/// A volume field: fill from the left up to the level, a tick at its end, the value on the right.
pub(super) fn volume_field(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    painter.rect_filled(rect, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    let x = rect.left() + rect.width() * volume_position(db);
    painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())), 0.0, theme::control_playing());
    let tick = x.round() - 0.5;
    painter.vline(tick, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::text_value()));
    painter.vline(tick, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::text_value()));
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}

/// A volume as a plain value, for the compact return and main rows.
pub(super) fn volume_text(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    let color = motion::mix(theme::text_value(), theme::text_primary(), hover);
    widgets::text_in(
        painter,
        rect,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        color,
    );
}

/// A pan field: a bar along the bottom from the centre, a centre tick, the value in the middle.
pub(super) fn pan_field(painter: &egui::Painter, rect: Rect, pan: f64, hover: f32) {
    painter.rect_filled(rect, 0.0, motion::mix(theme::pan_background(), theme::panel_hover(), hover));
    let centre = rect.center().x.round() - 0.5;
    painter.vline(centre, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::pan_centre()));
    painter.vline(centre, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::pan_centre()));
    let to = rect.center().x + rect.width() / 2.0 * pan.clamp(-1.0, 1.0) as f32;
    let bar =
        Rect::from_min_max(Pos2::new(centre.min(to), rect.bottom() - 2.0), Pos2::new(centre.max(to), rect.bottom()));
    painter.rect_filled(bar, 0.0, theme::text_value());
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}
