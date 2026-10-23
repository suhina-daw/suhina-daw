// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The selected clip's properties beside the chain: loop, warp, gain, transpose and the rest.

use super::*;

/// What a row of the clip column edits, when it edits something (#699).
#[derive(Clone, Copy, PartialEq)]
pub(super) enum ClipEdit {
    None,
    Loop,
    LoopStart,
    LoopEnd,
    Gain,
    Warp,
    Mode,
    Transpose,
}

/// The warp modes in the order a click on the Mode row steps through them: the name
/// `clip.setAudio` takes, and the one shown.
pub(super) const WARP_MODES: [(StretchMode, &str, &str); 6] = [
    (StretchMode::Repitch, "repitch", "Re-pitch"),
    (StretchMode::Rhythmic, "rhythmic", "Rhythmic"),
    (StretchMode::Tonal, "tonal", "Tonal"),
    (StretchMode::Texture, "texture", "Texture"),
    (StretchMode::Smooth, "smooth", "Smooth"),
    (StretchMode::HighQuality, "high-quality", "High quality"),
];

/// `params` with the clip at `at` named in them, for a `clip.*` command.
pub(super) fn on_clip(at: ClipRef, params: serde_json::Value) -> serde_json::Value {
    let mut named = match at {
        ClipRef::Arrangement(_, clip) => json!({ "clip": clip.0 }),
        ClipRef::Slot(track, scene) => json!({ "track": track.0, "scene": scene + 1 }),
    };
    if let (Some(map), serde_json::Value::Object(more)) = (named.as_object_mut(), params) {
        map.extend(more);
    }
    named
}

/// A beat count as the loop rows show it.
pub(super) fn beats_text(beats: f64) -> String {
    let rounded = (beats * 100.0).round() / 100.0;
    format!("{rounded} beats")
}

/// The selected clip: its name and where it is, how long, and what it plays; the loop, gain and
/// warp rows edit it (› Clip column in `docs/spec/ui-clip-editor.md`). In a row it takes a column
/// of [`CLIP_WIDTH`] at the left of `area`, stacked the rows it needs at the top; returns the
/// space it took.
pub(super) fn clip_properties(
    ui: &Ui,
    painter: &egui::Painter,
    area: Rect,
    info: &ClipInfo,
    stacked: bool,
    actions: &mut Vec<Action>,
) -> (Rect, Option<Showing>) {
    let mut rows = vec![
        ("Track", info.track.clone(), ClipEdit::None),
        (if info.scene { "Scene" } else { "Start" }, info.start.clone(), ClipEdit::None),
        ("Length", info.length.clone(), ClipEdit::None),
        ("Loop", if info.looping { "On".into() } else { "Off".into() }, ClipEdit::Loop),
    ];
    // A MIDI clip's loop is dragged on the piano roll's ruler; an audio clip has no editor yet,
    // so its loop is set here.
    if let (Some((start, end)), true) = (info.loop_range, info.audio) {
        rows.push(("Loop start", beats_text(start), ClipEdit::LoopStart));
        rows.push(("Loop end", beats_text(end), ClipEdit::LoopEnd));
    }
    if info.audio {
        // A slot's clip has no gain command of its own yet: its gain is shown, not edited.
        let gain_edit = if matches!(info.at, Some(ClipRef::Arrangement(..))) { ClipEdit::Gain } else { ClipEdit::None };
        rows.push((
            "Gain",
            info.gain_db.map_or_else(|| "-".into(), |db| format!("{} dB", widgets::format_db(db))),
            gain_edit,
        ));
        rows.push(("Warp", if info.warped { "On".into() } else { "Off".into() }, ClipEdit::Warp));
        let mode = WARP_MODES.iter().find(|(m, _, _)| *m == info.stretch).map_or("Smooth", |(_, _, shown)| shown);
        rows.push(("Mode", mode.into(), ClipEdit::Mode));
        rows.push(("Transpose", transpose_text(info.transpose), ClipEdit::Transpose));
        let beats = |b: f64| if b > 0.0 { format!("{} beats", (b * 100.0).round() / 100.0) } else { "-".into() };
        rows.push(("Fade in", beats(info.fades.0), ClipEdit::None));
        rows.push(("Fade out", beats(info.fades.1), ClipEdit::None));
    } else {
        // With notes selected the row says how many of the clip's notes they are, and how many of
        // them are muted (› Clip column in `docs/spec/ui-clip-editor.md`).
        let selection = &info.selection;
        let notes = if selection.selected == 0 {
            info.notes.to_string()
        } else if selection.muted == 0 {
            format!("{} of {}", selection.selected, info.notes)
        } else {
            format!("{} of {}, {} muted", selection.selected, info.notes, selection.muted)
        };
        rows.push(("Notes", notes, ClipEdit::None));
        if let Some(from) = &selection.from {
            rows.push(("From", from.clone(), ClipEdit::None));
        }
        if let Some(to) = &selection.to {
            rows.push(("To", to.clone(), ClipEdit::None));
        }
        if let (Some(low), Some(high)) = (selection.low_key, selection.high_key) {
            let one = low == high;
            let low_name = crate::detail::piano_roll::key_name(low);
            rows.push((
                "Pitch",
                if one { low_name } else { format!("{low_name}–{}", crate::detail::piano_roll::key_name(high)) },
                ClipEdit::None,
            ));
        }
        if let (Some(low), Some(high)) = (selection.low_velocity, selection.high_velocity) {
            rows.push((
                "Velocity",
                if low == high { low.to_string() } else { format!("{low}–{high}") },
                ClipEdit::None,
            ));
        }
    }
    let column = if stacked {
        let height = ROW + PAD * 2.0 + rows.len() as f32 * PARAM_ROW;
        Rect::from_min_size(area.min, Vec2::new(area.width(), height))
    } else {
        Rect::from_min_max(area.min, Pos2::new(area.left() + CLIP_WIDTH, area.bottom()))
    }
    .intersect(area);
    painter.rect_filled(column, 0.0, theme::panel_background());
    // Rows that do not fit under the column's bottom are left out rather than cut in half, the
    // title last; text never wraps (› Clip column in `docs/spec/ui-clip-editor.md`).
    let title = Rect::from_min_size(column.min, Vec2::new(column.width(), ROW));
    if title.bottom() > column.bottom() + 0.5 {
        return (column, None);
    }
    // The Clip / Devices switch ends the title row; the name keeps clear of it.
    let (name, picked) = match info.switch {
        Some(showing) => {
            let width = (SWITCH_PART * 2.0 + 1.0).min(title.width() - PAD * 2.0);
            let at = Rect::from_center_size(
                Pos2::new(title.right() - PAD - width / 2.0, title.center().y),
                Vec2::new(width, SWITCH_HEIGHT),
            );
            let picked = switch(ui, painter, at, showing);
            (Rect::from_min_max(title.min, Pos2::new(at.left() - PAD, title.bottom())), picked)
        }
        None => (title, None),
    };
    widgets::text_in(
        painter,
        name.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        &info.name,
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let mut y = title.bottom() + PAD;
    for (label, value, edit) in rows {
        let row = Rect::from_min_max(Pos2::new(column.left() + PAD, y), Pos2::new(column.right() - PAD, y + PARAM_ROW));
        y += PARAM_ROW;
        if row.bottom() > column.bottom() + 0.5 {
            break;
        }
        // The value keeps clear of the label, so a long track name is shortened, not drawn over it.
        let value_rect = Rect::from_min_max(Pos2::new(row.left() + LABEL_WIDTH, row.top()), row.max);
        if let (Some(at), true) = (info.at, edit != ClipEdit::None) {
            clip_edit_row(ui, painter, row, value_rect, (label, &value), edit, at, info, actions);
            continue;
        }
        // The row is named for a reader and a test ("Notes: 1 of 4"), as the piano roll's notes
        // are, so what the column says can be found by name.
        let response = ui.interact(row, egui::Id::new(("clip-property", label)), Sense::hover());
        let named = format!("{label}: {value}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &named));
        widgets::text_in(
            painter,
            row,
            Align2::LEFT_CENTER,
            label,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
        widgets::text_in(
            painter,
            value_rect,
            Align2::RIGHT_CENTER,
            &value,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    }
    (column, picked)
}

/// Semitones as the Transpose row shows them.
pub(super) fn transpose_text(semitones: f64) -> String {
    let rounded = (semitones * 100.0).round() / 100.0;
    if rounded > 0.0 { format!("+{rounded} st") } else { format!("{rounded} st") }
}

/// A row of the clip column that edits the clip: a click switches the loop or warp, or steps
/// the warp mode; the loop range, gain and transpose are dragged sideways (one command when the
/// drag ends, so a drag is one undo step) and a double-click puts gain and transpose back to 0.
#[allow(clippy::too_many_arguments)]
pub(super) fn clip_edit_row(
    ui: &Ui,
    painter: &egui::Painter,
    row: Rect,
    value_rect: Rect,
    (label, value): (&str, &str),
    edit: ClipEdit,
    at: ClipRef,
    info: &ClipInfo,
    actions: &mut Vec<Action>,
) {
    widgets::text_in(
        painter,
        row,
        Align2::LEFT_CENTER,
        label,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
    let id = Id::new(("clip-edit", label));
    let draw_value = |response: &egui::Response, text: &str| {
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, response));
        painter.rect_filled(value_rect, 2.0, fill);
        widgets::text_in(
            painter,
            value_rect.shrink2(Vec2::new(PAD / 2.0, 0.0)),
            Align2::RIGHT_CENTER,
            text,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    };
    if matches!(edit, ClipEdit::Loop | ClipEdit::Warp | ClipEdit::Mode) {
        let response = widgets::button_area(ui, value_rect, id, format!("{label}: {value}"));
        draw_value(&response, value);
        if response.clicked() {
            actions.push(match edit {
                ClipEdit::Loop => Action::new("clip.setLoop", on_clip(at, json!({ "loop": !info.looping }))),
                ClipEdit::Warp => Action::new("clip.setAudio", on_clip(at, json!({ "warp": !info.warped }))),
                _ => {
                    let index = WARP_MODES.iter().position(|(m, _, _)| *m == info.stretch).unwrap_or(4);
                    let next = WARP_MODES[(index + 1) % WARP_MODES.len()].1;
                    Action::new("clip.setAudio", on_clip(at, json!({ "mode": next })))
                }
            });
        }
        return;
    }
    // Dragged: shown from a draft while the drag lasts. The loop's ends keep a quarter beat
    // apart and move by quarter beats, the transpose by semitones, the gain by tenths of a dB.
    let (start, end) = info.loop_range.unwrap_or((0.0, 4.0));
    let (min, max) = loop_bounds(edit, start, end);
    let (stored, per_point, min, max, reset) = match edit {
        ClipEdit::LoopStart => (start, 0.0625, min, max, None),
        ClipEdit::LoopEnd => (end, 0.0625, min, max, None),
        ClipEdit::Gain => {
            (info.gain_db.filter(|db| db.is_finite()).unwrap_or(-70.0).max(-70.0), 0.1, -70.0, 24.0, Some(0.0))
        }
        _ => (info.transpose, 0.1, -48.0, 48.0, Some(0.0)),
    };
    // Rounding can step past a bound (a start of 3.85 rounds to 4.0 below an end of 4.1): kept
    // inside after it too.
    let snap = |v: f64| {
        match edit {
            ClipEdit::LoopStart | ClipEdit::LoopEnd => (v * 4.0).round() / 4.0,
            ClipEdit::Transpose => v.round(),
            _ => (v * 10.0).round() / 10.0,
        }
        .clamp(min, max)
    };
    let draft = ui.data(|d| d.get_temp::<f64>(id));
    let text = match draft.map(snap) {
        None => value.to_string(),
        Some(shown) => match edit {
            ClipEdit::Gain => format!("{} dB", widgets::format_db(shown)),
            ClipEdit::Transpose => transpose_text(shown),
            _ => beats_text(shown),
        },
    };
    let hint = if reset.is_some() { "Drag sideways; double-click resets" } else { "Drag sideways" };
    let response = ui.interact(value_rect, id.with("drag"), Sense::click_and_drag()).on_hover_text(hint);
    let named = format!("{label}: {text}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &named));
    draw_value(&response, &text);
    let moved = |from: f64| snap((from + f64::from(response.drag_delta().x) * per_point).clamp(min, max));
    let set = if response.double_clicked() {
        ui.data_mut(|d| d.remove::<f64>(id));
        reset
    } else if response.drag_stopped() {
        ui.data_mut(|d| d.remove::<f64>(id));
        Some(moved(draft.unwrap_or(stored)))
    } else {
        if response.dragged() {
            let next = (draft.unwrap_or(stored) + f64::from(response.drag_delta().x) * per_point).clamp(min, max);
            ui.data_mut(|d| d.insert_temp(id, next));
        }
        None
    };
    let Some(value) = set.filter(|v| (v - stored).abs() > 1e-9) else { return };
    actions.push(match edit {
        ClipEdit::LoopStart => Action::new("clip.setLoop", on_clip(at, json!({ "start": value }))),
        ClipEdit::LoopEnd => Action::new("clip.setLoop", on_clip(at, json!({ "end": value }))),
        ClipEdit::Gain => {
            let db = commands::exact_db_param(value, -70.0);
            Action::new("clip.setGain", on_clip(at, json!({ "db": db })))
        }
        _ => Action::new("clip.setAudio", on_clip(at, json!({ "transpose": value }))),
    });
}
