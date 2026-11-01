// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The volume, pan and send controls in a track's header.

use super::*;

/// A header's send knob to one return: dragged up or down for the level (`track.setSend`, sent
/// when let go), double-click for none, right-click for before or after the fader
/// (`track.setSendMode`), as the Launcher view's strip's.
#[allow(clippy::too_many_arguments)]
pub(super) fn send_knob(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    send: &crate::interaction::grid::SendKnob,
    slot: usize,
    id: Id,
    state: &mut ArrangementViewState,
    background: Color32,
    actions: &mut Vec<Action>,
) {
    let name = format!("arrangement-send-{}", slot + 1);
    if crate::kit::control_layout::placement(&name).hidden {
        return;
    }
    let rect = crate::kit::control_layout::place(&name, rect, clip);
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
    // `.send` styles it here as in the mixer.
    let _style = crate::kit::style_class::enter_response("send", &response);
    let amount = widgets::hover(ui, &response);
    let surface = motion::mix(background, theme::control_hover(), amount);
    crate::kit::native_css::surface(painter, rect, 0.0, egui::Color32::TRANSPARENT);
    let pointer = crate::kit::style_class::foreground(theme::text_on(surface));
    if amount > 0.0 || response.has_focus() {
        let fill = theme::control_hover().gamma_multiply(amount.max(0.3));
        widgets::track_button_face(ui, painter, rect, &response, fill);
    }
    let letter = send.name.chars().next().map(String::from).unwrap_or_default();
    // `.send { --suhina-appearance: knob }` draws it as a knob, as in the mixer (`send.knob.*`
    // artwork when the theme has it), its value beside it unless `--suhina-value-position` says
    // otherwise.
    if crate::kit::style_class::knob_preference(false) {
        let text = format!("{letter} {}", widgets::format_db(shown));
        let right = widgets::ValuePosition::Right;
        widgets::knob_with_value(
            painter,
            rect,
            position,
            pointer,
            Some(&response),
            &text,
            pointer,
            right,
            theme::TEXT_SMALL,
        );
    } else {
        crate::kit::mixer_controls::send_field(painter, rect, &letter, shown, send.pre_fader, pointer, position);
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
    let name = match control {
        Control::Volume => "arrangement-volume",
        Control::Pan => "arrangement-pan",
    };
    if crate::kit::control_layout::placement(name).hidden {
        return;
    }
    let rect = crate::kit::control_layout::place(name, rect, clip);
    // Named by their placement names, so a theme selects one by id (`#arrangement-volume`).
    let _style = crate::kit::style_class::enter_named("value", name);
    let _pan_style = matches!(control, Control::Pan).then(|| crate::kit::style_class::enter("pan"));
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
    let moved = state
        .value_drag
        .as_ref()
        .filter(|d| d.control == control && d.tracks.contains(&track))
        .map(|d| moved_by_drag(value, d, range));
    let shown = preview.or(moved).unwrap_or(value);
    let mut hover = 0.0;
    let mut control_response = None;
    let mut committed = None;
    if rect.intersects(clip) {
        // Where the field is, so the wheel over it moves the value instead of scrolling the view.
        state.value_areas.push(rect);
        let (label, text) = match control {
            Control::Volume => (format!("{} volume", lane.name), widgets::format_db(shown)),
            Control::Pan => (format!("{} pan", lane.name), widgets::format_pan(shown)),
        };
        let response = widgets::value_area(ui, rect.intersect(clip), id.with(label.as_str()), &label, text);
        control_response = Some(response.clone());
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
        match widgets::drag_value_axis(
            ui,
            &response,
            &mut preview,
            value,
            per_point,
            per_tick,
            range,
            default,
            crate::kit::control_layout::placement(name).direction == crate::kit::control_layout::Direction::Horizontal,
        ) {
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
        Some(drag) if drag.control == control && drag.tracks.contains(&track) => {
            Some(moved_by_drag(value, drag, range))
        }
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
        Control::Volume if state.knobs => volume_knob(painter, rect, shown, hover, control_response.as_ref()),
        Control::Volume => volume_field(painter, rect, shown, hover),
        Control::Pan if crate::kit::style_class::knob_preference(state.knobs) => {
            pan_knob(painter, rect, shown, control_response.as_ref())
        }
        Control::Pan => crate::kit::mixer_controls::pan_field(painter, rect, shown, hover),
    }
}

/// A volume as a small knob with its value beside it, for Settings › Launcher and mixer ›
/// Mixer controls: Knob.
pub(super) fn volume_knob(painter: &egui::Painter, rect: Rect, db: f64, hover: f32, response: Option<&egui::Response>) {
    let pointer = motion::mix(theme::text_value(), theme::text_primary(), hover);
    let text = widgets::format_db(db);
    let right = widgets::ValuePosition::Right;
    let ink = theme::text_primary();
    widgets::knob_with_value(
        painter,
        rect,
        volume_position(db),
        pointer,
        response,
        &text,
        ink,
        right,
        theme::TEXT_SMALL,
    );
}

/// A pan as a knob with its value where `--suhina-value-position` puts it (beside it by default).
pub(super) fn pan_knob(painter: &egui::Painter, rect: Rect, pan: f64, response: Option<&egui::Response>) {
    let position = ((pan + 1.0) / 2.0) as f32;
    let text = widgets::format_pan(pan);
    let (pointer, ink) = (theme::text_primary(), theme::text_secondary());
    let right = widgets::ValuePosition::Right;
    widgets::knob_with_value(painter, rect, position, pointer, response, &text, ink, right, theme::TEXT_SMALL);
}

/// What a track whose value is `value` shows while `drag` moves its selection: the dragged track
/// shows what the drag reached and the others keep their distance from it, as [`value_action`]
/// will leave them. A drag to silence, or from it, is set on them all outright.
pub(super) fn moved_by_drag(value: f64, drag: &ValueDrag, range: (f64, f64)) -> f64 {
    let delta = drag.now - drag.start;
    let silenced = drag.control == Control::Volume && drag.now <= range.0;
    if value == drag.start || silenced || !delta.is_finite() {
        return drag.now;
    }
    if value.is_finite() { (value + delta).clamp(range.0, range.1) } else { value }
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

/// A volume field: fill from the left up to the level, the value on the right. Its track and fill
/// take their shape from the sheet (`#arrangement-volume` and its `.value-fill`: background,
/// border, radius, `height` for the thickness) or from `volume.track` and `volume.fill` artwork;
/// without either it is the native capsule.
pub(super) fn volume_field(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    crate::kit::mixer_controls::level_bar(
        painter,
        rect,
        "volume",
        (0.0, volume_position(db)),
        hover,
        &widgets::format_db(db),
        Align2::RIGHT_CENTER,
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
