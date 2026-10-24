// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The marker row and the loop brace above the lanes.

use super::*;

/// The markers in the row under the ruler: each a line at its beat with its name beside it. A
/// click jumps there, a drag moves it (on the clip grid; Alt: free), a right-click renames or
/// deletes it (`docs/spec/arrangement-editing.md` › Markers).
pub(super) fn marker_flags(
    ui: &Ui,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
    row: Rect,
    x_of: &dyn Fn(f64) -> f32,
) {
    let painter = ui.painter_at(row);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let shown_beat = |state: &ArrangementViewState, index: usize, beat: f64| match state.marker_drag {
        Some((i, raw)) if i == index => {
            let free = ui.input(|input| input.modifiers.alt);
            let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };
            ruler_beat(raw, arrangement_grid(state.grid, state.beat_width, bar_beats), free)
        }
        _ => beat,
    };
    for (n, marker) in timeline.markers.iter().enumerate() {
        let beat = shown_beat(state, marker.index, marker.beat);
        let x = x_of(beat).round() + 0.5;
        // The tag ends where the next marker starts, so names never cover each other.
        let next = timeline.markers.get(n + 1).map_or(f32::INFINITY, |m| x_of(shown_beat(state, m.index, m.beat)));
        let width = (widgets::text_width(ui, &marker.name, font.clone()) + 2.0 * 3.0 + 2.0)
            .clamp(MARKER.x, MARKER_TAG_MAX)
            .min((next - x - 1.0).max(MARKER.x));
        let tag = Rect::from_min_max(Pos2::new(x, row.top() + 1.0), Pos2::new(x + width, row.bottom() - 1.0));
        if tag.right() < row.left() || tag.left() > row.right() {
            continue;
        }
        let id = Id::new(("arrangement-marker", marker.index));
        let response = ui.interact(tag.expand2(Vec2::new(2.0, 0.0)), id, Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Marker {}", marker.name))
        });
        let fill = if state.marker_drag.is_some_and(|(i, _)| i == marker.index) {
            theme::control_active()
        } else if response.hovered() {
            theme::control_hover()
        } else {
            theme::control_background()
        };
        painter.rect_filled(tag, 0.0, fill);
        painter.vline(x, row.y_range(), theme::hairline(theme::text_secondary()));
        widgets::text_in(
            &painter,
            tag.shrink2(Vec2::new(3.0, 0.0)).translate(Vec2::new(1.0, 0.0)),
            Align2::LEFT_CENTER,
            &marker.name,
            font.clone(),
            theme::text_value(),
        );

        if response.clicked() {
            // A click loops from the marker to the next one and jumps there, so a section is
            // looped with one click; the last marker only jumps.
            if let Some(next) = timeline.markers[n + 1..].iter().find(|m| m.beat > marker.beat) {
                actions.push(Action::new(
                    "transport.setLoop",
                    json!({ "start": marker.beat, "end": next.beat, "enabled": true }),
                ));
            }
            actions.push(Action::new("marker.jump", json!({ "index": marker.index })));
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            state.marker_drag = Some((marker.index, marker.beat));
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some((i, raw)) = state.marker_drag.as_mut()
            && *i == marker.index
        {
            // Where the pointer is, in beats, unsnapped: snapping happens as it is shown.
            *raw = f64::from((pointer.x - row.left() + state.scroll.x) / state.beat_width);
        }
        if response.drag_stopped() && state.marker_drag.is_some_and(|(i, _)| i == marker.index) {
            let to = shown_beat(state, marker.index, marker.beat);
            state.marker_drag = None;
            if to != marker.beat {
                actions.push(Action::new("marker.move", json!({ "index": marker.index, "beat": to })));
            }
        }
        if response.secondary_clicked() {
            state.marker_name = Some((marker.index, marker.name.clone()));
        }
        response.context_menu(|ui| {
            let Some((index, name)) = state.marker_name.as_mut().filter(|(i, _)| *i == marker.index) else {
                ui.close();
                return;
            };
            let index = *index;
            let field = ui.add(egui::TextEdit::singleline(name).hint_text("Marker name").desired_width(140.0));
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Rename").clicked() || entered {
                let name = name.trim().to_string();
                if !name.is_empty() && name != marker.name {
                    actions.push(Action::new("marker.rename", json!({ "index": index, "name": name })));
                }
                ui.close();
            }
            if ui.button("Jump to marker").clicked() {
                actions.push(Action::new("marker.jump", json!({ "index": index })));
                ui.close();
            }
            if ui.button("Delete marker").clicked() {
                actions.push(Action::new("marker.delete", json!({ "index": index })));
                ui.close();
            }
        });
    }
    // A drag that lost its marker (the project changed under it) ends.
    if state.marker_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.marker_drag = None;
    }
}

/// The loop region as the row under the ruler draws it while `state.loop_drag` changes it, on the
/// ruler's grid (Alt: free). `None` while a new region is no wider than a grid step.
pub(super) fn dragged_loop(
    timeline: &Timeline,
    state: &ArrangementViewState,
    free: bool,
    grab: LoopGrab,
    raw: f64,
) -> Option<(f64, f64)> {
    let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };
    let snap = |beat: f64| ruler_beat(beat, arrangement_grid(state.grid, state.beat_width, bar_beats), free);
    let brace = timeline.loop_brace.map(|b| (b.start, b.end));
    let (start, end) = match (grab, brace) {
        (LoopGrab::New(anchor), _) => {
            let to = snap(raw);
            (anchor.min(to), anchor.max(to))
        }
        (LoopGrab::Start, Some((_, end))) => (snap(raw).min(end), end),
        (LoopGrab::End, Some((start, _))) => (start, snap(raw).max(start)),
        (LoopGrab::Body(into), Some((start, end))) => {
            let to = snap((raw - into).max(0.0));
            (to, to + (end - start))
        }
        _ => return None,
    };
    (end > start && start >= 0.0 && end <= suhina_engine::MAX_START_BEAT).then_some((start, end))
}

/// The loop brace in its row under the ruler: a bar from the loop start to the loop end, bright
/// while looping is on. Dragging on the row draws a new region and turns looping on; dragging an
/// end of the brace moves that end, dragging its middle moves it (on the ruler's grid; Alt:
/// free); a click on it turns looping on or off (`docs/spec/arrangement-editing.md` › Loop region).
pub(super) fn loop_brace(
    ui: &Ui,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
    row: Rect,
    x_of: &dyn Fn(f64) -> f32,
) {
    let painter = ui.painter_at(row);
    let free = ui.input(|i| i.modifiers.alt);
    let raw_at = |x: f32, state: &ArrangementViewState| f64::from((x - row.left() + state.scroll.x) / state.beat_width);
    let response = ui.interact(row, Id::new("arrangement-loop-row"), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Loop brace"));

    // Which part of the brace is under `x`.
    let brace_x = timeline.loop_brace.map(|b| (x_of(b.start), x_of(b.end)));
    let part = |x: f32| -> Option<LoopGrab> {
        let (left, right) = brace_x?;
        if (x - left).abs() <= LOOP_GRAB {
            Some(LoopGrab::Start)
        } else if (x - right).abs() <= LOOP_GRAB {
            Some(LoopGrab::End)
        } else if (left..=right).contains(&x) {
            Some(LoopGrab::Body(0.0))
        } else {
            None
        }
    };
    if let Some(pointer) = response.hover_pos() {
        match part(pointer.x) {
            Some(LoopGrab::Start | LoopGrab::End) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
            Some(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            None => {}
        }
    }

    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
    {
        let raw = raw_at(origin.x, state);
        let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };
        let grab = match (part(origin.x), timeline.loop_brace) {
            (Some(LoopGrab::Body(_)), Some(brace)) => LoopGrab::Body(raw - brace.start),
            (Some(grab), _) => grab,
            (None, _) => {
                LoopGrab::New(ruler_beat(raw, arrangement_grid(state.grid, state.beat_width, bar_beats), free))
            }
        };
        state.loop_drag = Some((grab, raw));
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(pointer) = response.interact_pointer_pos()
        && state.loop_drag.is_some()
    {
        let at = raw_at(pointer.x, state);
        if let Some((_, raw)) = state.loop_drag.as_mut() {
            *raw = at;
        }
    }
    let dragged = state.loop_drag.and_then(|(grab, raw)| dragged_loop(timeline, state, free, grab, raw));
    if response.drag_stopped()
        && let Some((grab, _)) = state.loop_drag.take()
        && let Some((start, end)) = dragged
    {
        let unchanged = timeline.loop_brace.is_some_and(|b| b.start == start && b.end == end);
        match grab {
            // A new region is meant to be heard: it turns looping on.
            LoopGrab::New(_) => {
                actions.push(Action::new("transport.setLoop", json!({ "start": start, "end": end, "enabled": true })));
            }
            _ if !unchanged => actions.push(Action::new("transport.setLoop", json!({ "start": start, "end": end }))),
            _ => {}
        }
    }
    if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
        && part(pointer.x).is_some()
    {
        actions.push(Action::new("transport.setLoopEnabled", json!({})));
    }
    response.context_menu(|ui| {
        let on = timeline.loop_brace.is_some_and(|b| b.enabled);
        if ui.button(if on { "Turn loop off" } else { "Turn loop on" }).clicked() {
            actions.push(Action::new("transport.setLoopEnabled", json!({ "enabled": !on })));
            ui.close();
        }
    });
    // A drag that lost its row (the view changed under it) ends.
    if state.loop_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.loop_drag = None;
    }

    let shown = dragged.or_else(|| timeline.loop_brace.map(|b| (b.start, b.end)));
    let Some((start, end)) = shown else { return };
    let enabled = timeline.loop_brace.is_none_or(|b| b.enabled) || state.loop_drag.is_some();
    let (left, right) = (x_of(start).round(), x_of(end).round());
    let bar = Rect::from_min_max(
        Pos2::new(left, row.center().y - LOOP_BAR / 2.0),
        Pos2::new(right.max(left + 1.0), row.center().y + LOOP_BAR / 2.0),
    );
    let (fill, edge) = if enabled {
        (theme::control_active(), theme::text_secondary())
    } else {
        (theme::control_background(), theme::text_hint())
    };
    painter.rect_filled(bar, 0.0, fill);
    painter.rect_stroke(bar, 0.0, theme::hairline(edge), StrokeKind::Inside);
}

/// The Back to arrangement glyph: an arrow curving over the top from the right to the left, its
/// head pointing down the way the curve arrives — the launch returns to the arrangement it left.
pub(super) fn back_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = egui::Stroke::new(1.1, color);
    let r = rect.height() * 0.42;
    let cx = rect.center().x;
    let cy = rect.top() + (rect.height() + r) / 2.0;
    let mut points = Vec::with_capacity(9);
    for step in 0u32..=8 {
        let angle = std::f32::consts::PI * (1.0 - step as f32 / 8.0);
        points.push(Pos2::new(cx + r * angle.cos(), cy - r * angle.sin()));
    }
    painter.add(egui::Shape::line(points, stroke));
    // The head at the left end, opening against the curve's arrival: pointing down.
    let tip = Pos2::new(cx - r, cy);
    painter.add(egui::Shape::line(vec![tip + Vec2::new(-2.6, -3.2), tip, tip + Vec2::new(2.6, -3.2)], stroke));
}
