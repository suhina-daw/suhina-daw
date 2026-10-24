// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip on a lane: its box, its drags (move, edges, fades, gain), and how it is painted.

use super::*;

/// The keys held over a clip's edge drag: Alt alone lets the edge off the grid, as Alt does for
/// every other drag; Ctrl and Alt stretch an audio clip's audio with it (Settings › Arrangement).
pub(super) fn edge_keys(ui: &Ui) -> (bool, bool) {
    ui.input(|i| (i.modifiers.alt && !i.modifiers.command, i.modifiers.alt && i.modifiers.command))
}

/// Where a clip's edges are while `drag` moves one: on the grid (free with `free`), kept at
/// least a grid step (or a 16th of a beat) long, its left edge no earlier than its content's
/// start and its right edge no later than its audio's end (none for a looping clip), as
/// `clip.trim` keeps them.
pub(super) fn edge_extent(clip: &ClipBox, drag: EdgeDrag, grid: &TimelineGrid, free: bool) -> (f64, f64) {
    let end = clip.start + clip.length;
    // A clip already shorter than a grid step keeps at least its length: a coarse grid (1 bar)
    // does not stretch a short clip just because its edge was touched.
    let least = if free || !grid.snaps { 1.0 / 16.0 } else { grid.step.min(clip.length).max(1.0 / 16.0) };
    let start_edge = if drag.grab == EdgeGrab::Left { clip.start } else { end };
    // Not moved yet (a press on the edge): nothing changes.
    if (drag.raw - start_edge).abs() < 1e-9 {
        return (clip.start, end);
    }
    let beat = grid.place(drag.raw, free, false);
    match drag.grab {
        EdgeGrab::Left => (beat.max(clip.earliest_start).min(end - least).max(0.0), end),
        EdgeGrab::Right => {
            let to = beat.max(clip.start + least);
            (clip.start, clip.latest_end.map_or(to, |last| to.min(last.max(clip.start + least))))
        }
    }
}

/// A fade's length after a drag of `raw` beats: its end on the grid (unless `grid` is `None`,
/// Alt held, or a grid that does not snap), and no longer than the clip leaves beside the other
/// fade. No fade and all the room
/// there is snap too, so a clip that does not start or end on the grid can still be faded over
/// its whole length.
pub(super) fn fade_length(raw: f64, edge: FadeEdge, clip: &ClipBox, other: f64, grid: Option<TimelineGrid>) -> f64 {
    if !raw.is_finite() {
        return 0.0;
    }
    let end = clip.start + clip.length;
    let room = (clip.length - other).max(0.0);
    let snapped = match grid.filter(|g| g.snaps) {
        Some(g) => {
            let on_grid = match edge {
                FadeEdge::In => g.place(clip.start + raw, false, false) - clip.start,
                FadeEdge::Out => end - g.place(end - raw, false, false),
            };
            [0.0, room].into_iter().fold(on_grid, |best, v| if (v - raw).abs() < (best - raw).abs() { v } else { best })
        }
        None => raw,
    };
    if snapped.is_finite() { snapped.clamp(0.0, room) } else { 0.0 }
}

/// The gain a clip's fades give `beats` into a clip `length` beats long, each fade following its
/// shape and curve, 0 … 1 (`docs/spec/arrangement-editing.md` › Fades).
pub(super) fn fade_gain(beats: f64, length: f64, shown: &Shown) -> f32 {
    let fade_in = if shown.fade_in > 0.0 { shown.fade_in_curve.gain(beats / shown.fade_in) } else { 1.0 };
    let fade_out =
        if shown.fade_out > 0.0 { shown.fade_out_curve.gain((length - beats) / shown.fade_out) } else { 1.0 };
    (fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0)) as f32
}

/// What a fade shape is called in its menu.
pub(super) fn shape_label(shape: FadeShape) -> &'static str {
    match shape {
        FadeShape::Linear => "Linear",
        FadeShape::EqualPower => "Equal power",
        FadeShape::Fast => "Fast",
        FadeShape::Slow => "Slow",
        FadeShape::SCurve => "S-curve",
    }
}

/// The curve that puts a fade's curve handle at height `y` in the clip `rect`: the handle sits on
/// the fade's line half-way through it, so it follows the pointer up and down until the curve
/// reaches −1 or +1.
pub(super) fn curve_at(rect: Rect, shape: FadeShape, y: f32) -> f64 {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let gain = if bottom > top { f64::from((bottom - y) / (bottom - top)) } else { f64::NAN };
    FadeCurve::through(shape, gain)
}

/// Most points a fade's line is drawn with, however wide the fade is on screen.
pub(super) const FADE_LINE_POINTS: usize = 256;

/// The points of a fade's line in the clip `rect`: from the silent bottom corner to full level
/// at the top, following `curve`, from `from` to `to` (left to right; `to` is the silent end for
/// a fade-out). Sampled every 2 points, at most [`FADE_LINE_POINTS`].
pub(super) fn fade_line(rect: Rect, edge: FadeEdge, from: f32, to: f32, curve: FadeCurve) -> Vec<Pos2> {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let width = (to - from).max(0.0);
    let steps = ((width / 2.0).ceil() as usize).clamp(1, FADE_LINE_POINTS - 1);
    (0..=steps)
        .map(|i| {
            let x = from + width * i as f32 / steps as f32;
            let along = if width > 0.0 { f64::from((x - from) / width) } else { 1.0 };
            let t = if edge == FadeEdge::In { along } else { 1.0 - along };
            Pos2::new(x, bottom - curve.gain(t) as f32 * (bottom - top))
        })
        .collect()
}

/// Where a fade's curve handle sits: on its line half-way through the fade.
pub(super) fn curve_handle(rect: Rect, edge: FadeEdge, fade: f32, curve: FadeCurve) -> Pos2 {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let x = match edge {
        FadeEdge::In => rect.left() + fade / 2.0,
        FadeEdge::Out => rect.right() - fade / 2.0,
    };
    Pos2::new(x, bottom - curve.gain(0.5) as f32 * (bottom - top))
}

/// The part of a lane a clip in sub-row `row` of `rows` is drawn in.
pub(super) fn sub_row(lane_time: Rect, row: usize, rows: usize) -> Rect {
    let rows = rows.max(1) as f32;
    let height = lane_time.height() / rows;
    let top = lane_time.top() + height * row as f32;
    Rect::from_x_y_ranges(lane_time.x_range(), egui::Rangef::new(top.round(), (top + height).round() - 1.0))
}

pub(super) fn clip_rect(lane_time: Rect, start: f64, length: f64, x_of: &dyn Fn(f64) -> f32) -> Rect {
    let left = x_of(start).round();
    Rect::from_min_max(
        Pos2::new(left, lane_time.top()),
        Pos2::new((x_of(start + length).round() - 1.0).max(left + 1.0), lane_time.bottom()),
    )
}

/// One arrangement clip: draws it and moves it when dragged, along its lane or onto another.
/// Returns the clip and its start when it is being dragged onto another lane, to be drawn there.
#[allow(clippy::too_many_arguments)]
pub(super) fn clip_box<'a>(
    ui: &Ui,
    painter: &egui::Painter,
    row: Row,
    lane: &'a Lane,
    clip: &'a ClipBox,
    x_of: &dyn Fn(f64) -> f32,
    bar_beats: f64,
    lane_at: &dyn Fn(Pos2) -> Option<TrackId>,
    timeline: &Timeline,
    clip_order: &[ClipSpot],
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) -> Option<(&'a Lane, &'a ClipBox, f64)> {
    let key = (lane.track, clip.id);
    let mine = |drag: &ClipDrag| (drag.track, drag.clip) == key;
    // A moved clip starts on a grid line, or where it is let go with Alt held.
    let grid = arrangement_grid(state.grid, state.beat_width, bar_beats);
    let free = ui.input(|i| i.modifiers.alt);
    let snapped = |raw: f64| grid.place(raw, free, false);
    let at = match state.clip_drag {
        Some(drag) if mine(&drag) => snapped(drag.start),
        // Another clip of the selection is dragged: this one moves by as much.
        Some(drag) if state.is_selected(key) && state.is_selected((drag.track, drag.clip)) => {
            (clip.start + snapped(drag.start) - drag.origin).max(0.0)
        }
        _ => clip.start,
    };
    // Where it was last frame takes the pointer; it is drawn where this frame's drag puts it.
    let space = if clip.rows > 1 { sub_row(row.time, clip.row, clip.rows) } else { row.time };
    let visible = clip_rect(space, at, clip.length, x_of).intersect(row.clip);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return None;
    }
    let bar = (clip.start / bar_beats).floor() as u64 + 1;
    let name = if clip.name.is_empty() { "clip" } else { clip.name.as_str() };
    let response = ui.interact(visible, Id::new(("arrangement-clip", clip.id.0)), Sense::click_and_drag());
    // The cursor says what a drag does: the lower half moves the clip (a hand; with the copy key
    // held, the copy cursor), the upper half selects time (a text cursor).
    let copy_key = ui.input(|i| if state.time_select_ctrl { i.modifiers.shift } else { i.modifiers.command });
    // A click on the name bar picks the whole clip; one in the body below it sets where play
    // starts.
    let on_bar = |y: f32| clip_name_bar(visible).is_some_and(|bar| y < bar.bottom());
    // Where a plain drag selects time: the body under the name bar (the default, the name bar
    // moving the clip), or the upper half (Mirror, the lower half moving it); with Ctrl held
    // instead when that setting is on.
    let bar_moves = state.name_bar_moves && clip_name_bar(visible).is_some();
    let selects_at = |y: f32| {
        if bar_moves { !on_bar(y) } else { clip_grab_area(visible).is_some_and(|grab| y < grab.top()) }
    };
    let in_body = |y: f32| !state.time_select_ctrl && selects_at(y);
    if state.clip_drag.is_some_and(|d| mine(&d) && (d.copy || copy_key)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    } else if response.hover_pos().is_some_and(|p| in_body(p.y)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(if copy_key { egui::CursorIcon::Copy } else { egui::CursorIcon::Grab });
    }
    let label = format!("Clip {name} on {} at bar {bar}", lane.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let stack = if clip.audio { state.stack_audio } else { state.stack_midi };
    // A right-click remembers the beat under the pointer, for the menu's Split here.
    if response.secondary_clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let raw = clip.start + f64::from((pointer.x - x_of(clip.start)) / state.beat_width);
        let on_grid = snapped(raw);
        let free = ui.input(|i| i.modifiers.alt) || on_grid <= clip.start || on_grid >= clip.start + clip.length;
        state.split_at = Some(if free { raw } else { on_grid });
    }
    let split_at = state.split_at;
    // The menu acts on the whole selection when this clip is part of it, else on this clip alone.
    let menu_clips: Vec<(TrackId, ClipId)> = if state.is_selected(key) { state.selected.clone() } else { vec![key] };
    // The menu stays open on a click inside: the rename's field and button live here. When it
    // is not open, its rename's draft dies: a reopen starts from the name as it is now.
    if !response.context_menu_opened() {
        ui.ctx().data_mut(|d| d.remove::<String>(Id::new(("clip-rename", key.1.0))));
    }
    egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        menus::clip_menu(ui, &menu_clips, stack, clip.enabled, &clip.name, actions);
        let ara = state
            .ara_tracks
            .iter()
            .find(|(t, _)| *t == lane.track)
            .map(|(_, name)| name.as_str())
            .or(state.ara_default.as_deref());
        // Consolidate joins every clip of the menu's on this track, from the first start to the
        // last end, so two selected clips become one.
        let joined = menu_clips
            .iter()
            .filter(|(track, _)| *track == lane.track)
            .filter_map(|(_, id)| lane.clips.iter().find(|c| c.id == *id))
            .fold((clip.start, clip.start + clip.length), |(s, e), c| (s.min(c.start), e.max(c.start + c.length)));
        menus::clip_edit_menu(ui, lane.track, clip.id, (clip.start, clip.length), joined, split_at, actions);
        if clip.audio
            && let Some(name) = ara
        {
            menus::ara_menu(ui, lane.track, clip.id, name, actions);
        }
    });
    let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
    // A drag that selects time rather than moving the clip: with Ctrl held, or from the clip's
    // lower half on a lane tall enough to have one, as the setting says.
    let selects_time = response.drag_started_by(egui::PointerButton::Primary)
        && ui
            .input(|i| i.pointer.press_origin())
            .is_some_and(|origin| if state.time_select_ctrl { command } else { selects_at(origin.y) });
    if selects_time {
        state.marquee = ui.input(|i| i.pointer.press_origin());
        state.marquee_scroll = state.scroll;
        state.marquee_time = true;
        state.marquee_row = (clip.rows > 1).then_some((lane.track, clip.row, clip.rows));
    } else if response.clicked() || response.drag_started_by(egui::PointerButton::Primary) {
        // Clips picked by a click or moved: the time selection gives way to them.
        state.time_selection = None;
    }
    if response.clicked() {
        state.pick_clip(key, shift, command, clip_order);
        state.clip_clicked = true;
        // The clips drawn before this one this frame still show the old selection: one more
        // frame at once, or the old highlight stays until the pointer moves again (#695).
        ui.ctx().request_repaint();
        // A click on its name bar picks the whole clip: play starts from its start, as the
        // setting says. A click in its body also moves the play position there, on the grid
        // (Alt: free), as a click on an empty lane does, and play starts there however the clip
        // is picked; an ARA plug-in's editor follows it (#717).
        match response.interact_pointer_pos() {
            Some(pointer) if !on_bar(pointer.y) => {
                let free = ui.input(|i| i.modifiers.alt);
                let raw = f64::from((pointer.x - x_of(0.0)) / state.beat_width);
                let beat = ruler_beat(raw, grid, free);
                state.picked_whole = false;
                state.insert = Some((lane.track, beat));
                actions.push(Action::new("transport.setPosition", json!({ "beats": beat })));
            }
            _ => {
                state.picked_whole = true;
                state.insert = None;
            }
        }
    } else if response.drag_started_by(egui::PointerButton::Primary) && !selects_time && !state.is_selected(key) {
        // Pressing a clip that is already selected keeps the selection, so a drag can start there.
        state.pick_clip(key, shift, command, clip_order);
        state.picked_whole = true;
        ui.ctx().request_repaint();
    }
    if response.clicked() || (response.drag_started_by(egui::PointerButton::Primary) && !selects_time) {
        // The track being worked on: a folded group shows this track while it is one of its own.
        state.targeted = Some(lane.track);
    }
    if response.drag_started_by(egui::PointerButton::Primary) && !selects_time {
        state.clip_drag = Some(ClipDrag {
            track: lane.track,
            clip: clip.id,
            start: clip.start,
            target: lane.track,
            origin: clip.start,
            copy: copy_key,
            new_track: false,
        });
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(drag) = state.clip_drag.as_mut()
        && (drag.track, drag.clip) == key
    {
        drag.start += f64::from(response.drag_delta().x / state.beat_width);
        if let Some(pointer) = response.interact_pointer_pos() {
            match lane_at(pointer) {
                Some(target) => {
                    drag.target = target;
                    drag.new_track = false;
                }
                None => {
                    drag.new_track =
                        state.below_tracks.is_some_and(|(top, bottom)| pointer.y > top && pointer.y < bottom);
                }
            }
        }
    }
    if response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(drag) = state.clip_drag.take()
        && (drag.track, drag.clip) == key
    {
        let start = snapped(drag.start);
        let stack = (clip.audio && state.stack_audio) || (!clip.audio && state.stack_midi);
        let moved = drag.new_track || drag.target != lane.track || (start - clip.start).abs() > 1e-9;
        // How many tracks down the drop is from this clip's own, among the tracks that hold clips:
        // a selection on several tracks moves by as many.
        let holders: Vec<TrackId> =
            timeline.lanes.iter().filter(|l| l.kind == ColumnKind::Track).map(|l| l.track).collect();
        let place = |track: TrackId| holders.iter().position(|t| *t == track).map(|i| i as i64);
        let tracks_by = place(drag.target).zip(place(lane.track)).map(|(to, from)| to - from).unwrap_or(0);
        // Copied: the clips stay, and copies (each playing a content of its own) go where they
        // were dropped; a selection is copied whole.
        let copying = drag.copy || copy_key;
        if copying {
            let chosen: Vec<(TrackId, ClipId)> =
                if state.is_selected(key) && state.selected.len() > 1 { state.selected.clone() } else { vec![key] };
            let clips: Vec<Value> = chosen.iter().map(|(_, id)| json!({ "clip": id.0 })).collect();
            let mut params = json!({ "clips": clips, "beats": start - clip.start, "copy": true });
            if drag.new_track {
                params["toNewTrack"] = json!(true);
                params["color"] = json!(hex(clip.color));
            } else if drag.target != lane.track && chosen.iter().all(|(track, _)| *track == lane.track) {
                params["toTrack"] = json!(drag.target.0);
                params["color"] = json!(hex(clip.color));
            } else if tracks_by != 0 {
                params["tracksBy"] = json!(tracks_by);
            }
            if stack {
                params["stack"] = json!(true);
            }
            if moved {
                actions.push(Action::new("clip.moveMany", params));
            }
        } else if state.selected.len() > 1 && state.is_selected(key) {
            // The whole selection moves by as much, at once: to another track when all of it is on
            // this one, by as many tracks when it is on several, or to a new track.
            let clips: Vec<Value> = state.selected.iter().map(|(_, id)| json!({ "clip": id.0 })).collect();
            let mut params = json!({ "clips": clips, "beats": start - clip.start });
            if drag.new_track {
                params["toNewTrack"] = json!(true);
                params["color"] = json!(hex(lane.color));
            } else if drag.target != lane.track && state.selected.iter().all(|(track, _)| *track == lane.track) {
                params["toTrack"] = json!(drag.target.0);
                params["color"] = json!(hex(lane.color));
            } else if tracks_by != 0 {
                params["tracksBy"] = json!(tracks_by);
            }
            if stack {
                params["stack"] = json!(true);
            }
            if moved {
                actions.push(Action::new("clip.moveMany", params));
            }
        } else if drag.new_track {
            let mut params = json!({
                "clips": [{ "clip": clip.id.0 }],
                "beats": start - clip.start,
                "toNewTrack": true,
                "color": hex(clip.color),
            });
            if stack {
                params["stack"] = json!(true);
            }
            actions.push(Action::new("clip.moveMany", params));
        } else {
            let mut params = json!({ "clip": clip.id.0, "start": start });
            if drag.target != lane.track {
                params["toTrack"] = json!(drag.target.0);
                params["color"] = json!(hex(clip.color));
            }
            if stack {
                params["stack"] = json!(true);
            }
            if moved {
                actions.push(Action::new("clip.move", params));
            }
        }
        // Drawn where it was dropped until the project has moved it.
        let rect = clip_rect(space, start, clip.length, x_of);
        let shown = Shown::of(clip);
        paint_clip(
            &painter.with_clip_rect(row.clip),
            rect,
            lane,
            clip,
            shown,
            start,
            x_of,
            state.beat_width,
            timeline,
            true,
        );
        return None;
    }

    // While one of its edges is dragged the clip is drawn at the length it has so far; its gain
    // field, its edges' handles and its fade handles follow it on every frame (#698).
    // An edge let go of: drawn where it was left until the project has it.
    if state.edge_released.is_some_and(|(k, from, to, _)| {
        k == key && (clip.start - from).abs() < 1e-9 && (clip.start + clip.length - to).abs() < 1e-9
    }) {
        state.edge_released = None;
    }
    let released = state.edge_released.filter(|(k, ..)| *k == key).map(|(_, from, to, _)| (from, to));
    let (live_start, live_length) = match state.edge_drag.filter(|d| d.key == key) {
        Some(edge) => {
            let (from, to) = edge_extent(clip, edge, &grid, edge_keys(ui).0);
            (from, to - from)
        }
        None => released.map_or((clip.start, clip.length), |(from, to)| (from, to - from)),
    };
    // What the clip shows: the project's values, or those of a drag in progress, so the
    // waveform follows the gain and the fades while they are dragged.
    let fade_grid = (!free).then_some(grid);
    let mut shown = state.shown(key, clip, fade_grid);

    // An audio clip big enough shows its gain, which a vertical drag changes.
    let mut gain_paint = None;
    if let Some(view) = &clip.audio_view
        && state.clip_drag.is_none_or(|d| !mine(&d))
    {
        let rect = clip_rect(space, live_start, live_length, x_of);
        let field = Rect::from_min_size(
            Pos2::new(rect.right() - GAIN_FIELD_INSET - GAIN_FIELD.x, rect.top() + 3.0),
            GAIN_FIELD,
        );
        if rect.width() >= 80.0 && rect.height() >= 30.0 && field.intersects(row.clip) {
            let label = format!("Gain of {name} on {}", lane.name);
            state.value_areas.push(field.intersect(row.clip));
            let response = widgets::value_area(
                ui,
                field.intersect(row.clip),
                Id::new(("clip-gain", clip.id.0)),
                &label,
                widgets::format_db(shown.gain_db),
            );
            let mut preview = match state.gain_drag {
                Some((k, v)) if k == key => Some(v),
                _ => None,
            };
            let mut committed = None;
            match widgets::drag_value(ui, &response, &mut preview, view.gain_db, 0.1, 1.0, CLIP_GAIN_RANGE, 0.0) {
                Drag::Moving(v) => state.gain_drag = Some((key, v)),
                Drag::Done(v) => {
                    state.gain_drag = None;
                    committed = Some(v);
                    let db = commands::db_param(v, CLIP_GAIN_RANGE.0);
                    actions.push(Action::new("clip.setGain", json!({ "clip": clip.id.0, "db": db })));
                }
                Drag::None => {}
            }
            shown.gain_db = match state.gain_drag {
                Some((k, v)) if k == key => v,
                _ => committed.unwrap_or(view.gain_db),
            };
            gain_paint = Some((field, shown.gain_db, widgets::hover(ui, &response)));
        }
    }

    // A clip's edges: the left or right edge dragged moves it (on the grid; Alt: free), the
    // content staying where it plays; with Ctrl and Alt an audio clip's audio stretches to the new length
    // instead (`clip.trim`, `clip.stretch`; `docs/spec/arrangement-editing.md` › Clip edges).
    // An audio clip keeps its top corners for the fade handles.
    let edge_rect = clip_rect(space, live_start, live_length, x_of);
    if state.clip_drag.is_none_or(|d| !mine(&d)) && edge_rect.width() >= 12.0 {
        const ZONE: f32 = 5.0;
        let top = edge_rect.top() + if clip.audio { FADE_HANDLE + 2.0 } else { 2.0 };
        let (free, stretch_held) = edge_keys(ui);
        for grab in [EdgeGrab::Left, EdgeGrab::Right] {
            let hit = match grab {
                EdgeGrab::Left => Rect::from_min_max(
                    Pos2::new(edge_rect.left(), top),
                    Pos2::new(edge_rect.left() + ZONE, edge_rect.bottom()),
                ),
                EdgeGrab::Right => Rect::from_min_max(
                    Pos2::new(edge_rect.right() - ZONE, top),
                    Pos2::new(edge_rect.right(), edge_rect.bottom()),
                ),
            }
            .intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let id = Id::new(("clip-edge", clip.id.0, grab as u8));
            let response = ui.interact(hit, id, Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let what = match grab {
                EdgeGrab::Left => "Left edge",
                EdgeGrab::Right => "Right edge",
            };
            let label = format!("{what} of {name} on {}", lane.name);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label));
            let mine_edge = |d: &EdgeDrag| d.key == key && d.grab == grab;
            if response.drag_started_by(egui::PointerButton::Primary) {
                let raw = if grab == EdgeGrab::Left { clip.start } else { clip.start + clip.length };
                // Where the button went down, not where the drag was recognised: summing the
                // frames' movement left out the few points before that, and the edge trailed
                // the pointer by them for the whole drag.
                let origin =
                    ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos()).map_or(0.0, |p| p.x);
                state.edge_drag = Some(EdgeDrag { key, grab, raw, from: raw, origin });
            }
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pointer) = response.interact_pointer_pos()
                && let Some(drag) = state.edge_drag.as_mut().filter(|d| mine_edge(d))
            {
                drag.raw = drag.from + f64::from((pointer.x - drag.origin) / state.beat_width);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(drag) = state.edge_drag.take_if(|d| mine_edge(d))
            {
                let (from, to) = edge_extent(clip, drag, &grid, free);
                let beat = if grab == EdgeGrab::Left { from } else { to };
                let stretch = stretch_held && state.stretch_alt && clip.audio;
                let mut params = json!({ "clip": clip.id.0 });
                params[if grab == EdgeGrab::Left { "start" } else { "end" }] = json!(beat);
                if (clip.audio && state.stack_audio) || (!clip.audio && state.stack_midi) {
                    params["stack"] = json!(true);
                }
                let old = if grab == EdgeGrab::Left { clip.start } else { clip.start + clip.length };
                if (beat - old).abs() > 1e-9 {
                    actions.push(Action::new(if stretch { "clip.stretch" } else { "clip.trim" }, params));
                    state.edge_released = Some((key, from, to, 30));
                }
            }
        }
    }

    // Fade handles at the top corners of an audio clip, while it is hovered or selected: a
    // horizontal drag sets the fade (its end on the grid, free with Alt), a double-click
    // removes it, a right-click chooses its shape.
    let mut handles = Vec::new();
    let mut curves = Vec::new();
    let rect = clip_rect(space, live_start, live_length, x_of);
    let selected = state.is_selected(key);
    // A clip picked by a click in its body (the insert marker set there) looks as one not
    // picked, as the play start is what the click chose; picked whole, it is lit.
    let lit = selected && state.picked_whole;
    let fading = state.fade_drag.is_some_and(|d| d.key == key) || state.curve_drag.is_some_and(|d| d.key == key);
    let pointer_in = widgets::pointer_over(ui, visible).is_some();
    let fits = rect.width() >= 3.0 * FADE_HANDLE && rect.height() >= 2.0 * FADE_HANDLE;
    if clip.audio && fits && state.clip_drag.is_none_or(|d| !mine(&d)) && (pointer_in || selected || fading) {
        let grid = fade_grid;
        for edge in [FadeEdge::In, FadeEdge::Out] {
            let (current, now, other) = match edge {
                FadeEdge::In => (clip.fade_in, shown.fade_in, shown.fade_out),
                FadeEdge::Out => (clip.fade_out, shown.fade_out, shown.fade_in),
            };
            let handle = fade_handle(rect, edge, now as f32 * state.beat_width);
            let hit = handle.expand((FADE_HANDLE_HIT - FADE_HANDLE) / 2.0).intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let which = if edge == FadeEdge::In { "Fade-in" } else { "Fade-out" };
            let label = format!("{which} of {name} on {}", lane.name);
            let id = Id::new(("clip-fade", clip.id.0, edge == FadeEdge::In));
            let response =
                ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label);
                info.value = Some(now);
                info
            });
            let mine_fade = |d: &FadeDrag| d.key == key && d.edge == edge;
            let mut done = None;
            if response.double_clicked() {
                state.fade_drag = None;
                done = Some(0.0);
            } else {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    state.fade_drag = Some(FadeDrag { key, edge, raw: current, from: current });
                }
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.fade_drag.as_mut().filter(|d| mine_fade(d))
                {
                    let dx = f64::from(response.drag_delta().x / state.beat_width);
                    drag.raw += if edge == FadeEdge::In { dx } else { -dx };
                }
                if response.drag_stopped_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.fade_drag.take_if(|d| mine_fade(d))
                {
                    done = Some(fade_length(drag.raw, edge, clip, other, grid));
                }
            }
            let length = match (done, state.fade_drag) {
                (Some(v), _) => v,
                (None, Some(d)) if mine_fade(&d) => fade_length(d.raw, edge, clip, other, grid),
                _ => now,
            };
            if let Some(v) = done
                && (v - current).abs() > 1e-9
            {
                let param = if edge == FadeEdge::In { "in" } else { "out" };
                // One of several selected clips: every selected clip's fade moves by as much,
                // each from its own (#715); a double-click still clears only this one.
                if state.is_selected(key) && state.selected.len() > 1 && response.drag_stopped() {
                    let audio = |(track, id): &(TrackId, ClipId)| {
                        timeline
                            .lanes
                            .iter()
                            .find(|l| l.track == *track)
                            .is_some_and(|l| l.clips.iter().any(|c| c.id == *id && c.audio))
                    };
                    let clips: Vec<Value> =
                        state.selected.iter().filter(|k| audio(k)).map(|(_, id)| json!({ "clip": id.0 })).collect();
                    let beats = ((v - current) * 1e6).round() / 1e6;
                    actions
                        .push(Action::new("clip.shiftFades", json!({ "clips": clips, "edge": param, "beats": beats })));
                } else {
                    let mut params = json!({ "clip": clip.id.0 });
                    params[param] = json!((v * 1e6).round() / 1e6);
                    actions.push(Action::new("clip.setFades", params));
                }
            }
            match edge {
                FadeEdge::In => shown.fade_in = length,
                FadeEdge::Out => shown.fade_out = length,
            }
            // A right-click chooses the fade's shape.
            let shape = shown.curve(edge).shape;
            response.context_menu(|ui| {
                for choice in FadeShape::ALL {
                    if ui.radio(choice == shape, shape_label(choice)).clicked() {
                        let param = if edge == FadeEdge::In { "inShape" } else { "outShape" };
                        let mut params = json!({ "clip": clip.id.0 });
                        params[param] = json!(choice.name());
                        actions.push(Action::new("clip.setFades", params));
                        ui.close();
                    }
                }
            });
            handles.push((edge, widgets::hover(ui, &response)));

            // The curve handle, half-way along a fade wide enough: a vertical drag bends the
            // curve, a double-click sets it back to 0.
            let fade_width = length as f32 * state.beat_width;
            if fade_width < CURVE_HANDLE_MIN_FADE || state.fade_drag.is_some_and(|d| mine_fade(&d)) {
                continue;
            }
            let current_curve = shown.curve(edge);
            let center = curve_handle(rect, edge, fade_width, current_curve);
            let hit = Rect::from_center_size(center, Vec2::splat(FADE_HANDLE_HIT)).intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let label = format!("{which} curve of {name} on {}", lane.name);
            let id = Id::new(("clip-fade-curve", clip.id.0, edge == FadeEdge::In));
            let response =
                ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label);
                info.value = Some(current_curve.curve);
                info
            });
            let mine_curve = |d: &CurveDrag| d.key == key && d.edge == edge;
            let mut done = None;
            if response.double_clicked() {
                state.curve_drag = None;
                done = Some(0.0);
            } else {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    // Where the button went down, not where the drag was recognised: the handle
                    // keeps its place under the pointer.
                    let pressed = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
                    let grab = pressed.map_or(0.0, |p| p.y - center.y);
                    state.curve_drag = Some(CurveDrag { key, edge, curve: current_curve.curve, grab });
                }
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(pointer) = response.interact_pointer_pos()
                    && let Some(drag) = state.curve_drag.as_mut().filter(|d| mine_curve(d))
                {
                    drag.curve = curve_at(rect, current_curve.shape, pointer.y - drag.grab);
                }
                if response.drag_stopped_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.curve_drag.take_if(|d| mine_curve(d))
                {
                    done = Some(drag.curve);
                }
            }
            let stored = match edge {
                FadeEdge::In => clip.fade_in_curve.curve,
                FadeEdge::Out => clip.fade_out_curve.curve,
            };
            if let Some(v) = done {
                let v = (v * 1000.0).round() / 1000.0;
                if (v - stored).abs() > 1e-9 {
                    let param = if edge == FadeEdge::In { "inCurve" } else { "outCurve" };
                    let mut params = json!({ "clip": clip.id.0 });
                    params[param] = json!(v);
                    actions.push(Action::new("clip.setFades", params));
                }
                match edge {
                    FadeEdge::In => shown.fade_in_curve.curve = v,
                    FadeEdge::Out => shown.fade_out_curve.curve = v,
                }
            }
            curves.push((edge, widgets::hover(ui, &response)));
        }
    }

    // Copying (Ctrl held): the clip stays where it is drawn, and its copy shows where it lands.
    let copying = state.clip_drag.is_some_and(|d| d.copy || copy_key) && (at - clip.start).abs() > 1e-9;
    let paint_at = |start: f64| {
        paint_clip(
            &painter.with_clip_rect(row.clip),
            clip_rect(space, start, clip.length, x_of),
            lane,
            clip,
            shown,
            start,
            x_of,
            state.beat_width,
            timeline,
            lit,
        );
    };
    let result = match state.clip_drag {
        Some(drag) if mine(&drag) && drag.target != lane.track => {
            if drag.copy || copy_key {
                paint_at(clip.start);
            }
            Some((lane, clip, snapped(drag.start)))
        }
        _ if copying => {
            paint_at(clip.start);
            paint_at(at);
            None
        }
        _ => {
            // Where the drag puts it (the dragged clip and the rest of its selection alike).
            let start = at;
            // An edge being dragged: the clip is drawn at the length it has so far, its content
            // where it plays.
            let free = edge_keys(ui).0;
            let extent = match state.edge_drag.filter(|d| d.key == key) {
                Some(edge) => Some(edge_extent(clip, edge, &grid, free)),
                None => state.edge_released.filter(|(k, ..)| *k == key).map(|(_, from, to, _)| (from, to)),
            };
            let (rect, clip_area) = match extent {
                Some((from, to)) => {
                    let rect = clip_rect(space, from, to - from, x_of);
                    (rect, row.clip.intersect(rect))
                }
                None => (clip_rect(space, start, clip.length, x_of), row.clip),
            };
            // An audio clip's edge dragged with Ctrl and Alt stretches its audio: drawn stretched
            // as it goes, about the edge that stays, not only once it is let go.
            let stretching = state
                .edge_drag
                .filter(|d| d.key == key && clip.audio && state.stretch_alt && edge_keys(ui).1 && clip.length > 0.0)
                .zip(extent);
            match stretching {
                Some((drag, (from, to))) => {
                    let factor = (to - from) / clip.length;
                    let anchor = if drag.grab == EdgeGrab::Left { clip.start + clip.length } else { clip.start };
                    let stretched = move |beat: f64| x_of(anchor + (beat - anchor) * factor);
                    paint_clip(
                        &painter.with_clip_rect(clip_area),
                        rect,
                        lane,
                        clip,
                        shown,
                        // The clip's own start: `stretched` already carries it to the new edge.
                        start,
                        &stretched,
                        state.beat_width * factor as f32,
                        timeline,
                        lit,
                    );
                }
                None => paint_clip(
                    &painter.with_clip_rect(clip_area),
                    rect,
                    lane,
                    clip,
                    shown,
                    start,
                    x_of,
                    state.beat_width,
                    timeline,
                    lit,
                ),
            }
            None
        }
    };
    // Picked by a click in its body: not lit (the play start is what the click chose), but
    // outlined, so the clip the editor and the keys work on is plain to see.
    if selected && !lit && state.clip_drag.is_none() {
        painter.with_clip_rect(row.clip).rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.5, theme::text_primary()),
            StrokeKind::Inside,
        );
    }
    // The gain sits on top of the clip.
    if let Some((field, db, hover)) = gain_paint {
        let gain_painter = painter.with_clip_rect(row.clip);
        let fill = motion::mix(theme::control_background(), theme::control_hover(), hover);
        gain_painter.rect_filled(field, 0.0, fill);
        let text = format!("{} dB", widgets::format_db(db));
        let font = theme::mono_font(theme::TEXT_TINY);
        widgets::text_in(
            &gain_painter,
            field.shrink2(Vec2::new(3.0, 0.0)),
            Align2::RIGHT_CENTER,
            &text,
            font,
            theme::text_value(),
        );
    }
    // The handles sit on top of everything on the clip, where the fades now end.
    let handle_painter = painter.with_clip_rect(row.clip);
    if suhina_plugins::trace::on() && !handles.is_empty() {
        // Which clip each drawn handle belongs to and where, for a handle seen without its clip.
        suhina_plugins::trace::state(
            &format!("fade handles of clip-{}", clip.id.0),
            format!(
                "on {} row {}/{} at beat {:.3} for {:.3}, selected {selected}, pointer in {pointer_in}",
                lane.name, clip.row, clip.rows, clip.start, clip.length
            ),
        );
    }
    for (edge, hover) in handles {
        let length = if edge == FadeEdge::In { shown.fade_in } else { shown.fade_out };
        let handle = fade_handle(rect, edge, length as f32 * state.beat_width);
        let fill = motion::mix(theme::text_secondary(), theme::text_primary(), hover);
        handle_painter.rect_filled(handle, 0.0, fill);
        handle_painter.rect_stroke(handle, 0.0, theme::hairline(theme::window_background()), StrokeKind::Outside);
    }
    for (edge, hover) in curves {
        let length = if edge == FadeEdge::In { shown.fade_in } else { shown.fade_out };
        let center = curve_handle(rect, edge, length as f32 * state.beat_width, shown.curve(edge));
        let fill = motion::mix(theme::text_secondary(), theme::text_primary(), hover);
        handle_painter.circle(center, CURVE_HANDLE / 2.0, fill, theme::hairline(theme::window_background()));
    }
    result
}

/// The square of a fade handle: at the clip's top corner with no fade, at the fade's end along
/// the top edge with one, `fade` points from the clip's edge; always inside the clip.
pub(super) fn fade_handle(clip: Rect, edge: FadeEdge, fade: f32) -> Rect {
    let fade = if fade.is_finite() { fade.max(0.0) } else { 0.0 };
    let left = match edge {
        FadeEdge::In => (clip.left() + fade).min(clip.right() - FADE_HANDLE),
        FadeEdge::Out => (clip.right() - fade - FADE_HANDLE).max(clip.left()),
    };
    Rect::from_min_size(Pos2::new(left.round(), clip.top()), Vec2::splat(FADE_HANDLE))
}

pub(super) fn view_rect(painter: &egui::Painter) -> Rect {
    painter.clip_rect()
}

/// Draws the notes an ARA plug-in found in an audio clip drawn at beat `start`: bars from each
/// note's start to its end, its pitch between the clip's lowest and highest (a row spare at each
/// side), more opaque the louder. Times come in seconds from the clip's start and go to beats
/// through the tempo map; only the notes in view, and in the clip, are drawn.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_ara_notes(
    painter: &egui::Painter,
    rect: Rect,
    marks: Rect,
    notes: &suhina_session::ClipNotes,
    tempo: &suhina_engine::model::TempoMap,
    start: f64,
    length: f64,
    x_of: &dyn Fn(f64) -> f32,
    beat_width: f32,
    ink: Color32,
) {
    let view = painter.clip_rect().intersect(rect);
    if !view.is_positive() {
        return;
    }
    let at = tempo.time_at(start);
    let beat_at_x = |x: f32| start + f64::from((x - x_of(start)) / beat_width);
    let (first, last) = (tempo.time_at(beat_at_x(view.left())) - at, tempo.time_at(beat_at_x(view.right())) - at);
    let end = tempo.time_at(start + length) - at;
    let rows = f32::from(notes.high - notes.low) + 3.0;
    let height = (marks.height() / rows).clamp(2.0, 6.0);
    let painter = painter.with_clip_rect(view);
    for note in notes.notes.iter().filter(|n| n.start + n.length >= first.max(0.0) && n.start <= last.min(end)) {
        let from = x_of(tempo.beat_at(at + note.start.max(0.0)));
        let to = x_of(tempo.beat_at(at + (note.start + note.length).min(end))).max(from + 1.0);
        let row = f32::from(note.pitch - notes.low) + 1.0;
        let y = marks.bottom() - (row + 0.5) / rows * marks.height();
        let bar = Rect::from_min_max(Pos2::new(from, y - height / 2.0), Pos2::new(to, y + height / 2.0));
        painter.rect_filled(bar, 1.0, ink.gamma_multiply(0.45 + 0.55 * note.volume));
    }
}

/// `colour` more saturated and a little brighter, for a selected track.
pub(super) fn saturated(colour: Color32) -> Color32 {
    let mut hsva = egui::ecolor::Hsva::from(colour);
    hsva.s = (hsva.s * 1.3).min(1.0);
    hsva.v = (hsva.v * 1.12).min(1.0);
    hsva.into()
}

/// A clip as a Shift-click's rectangle finds it: its key, the row its lane is drawn in, and
/// its span in beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ClipSpot {
    pub(super) key: (TrackId, ClipId),
    pub(super) row: usize,
    pub(super) start: f64,
    pub(super) end: f64,
}

/// Draws a clip's body, colour strip, name and the marks of its notes (only those in view).
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_clip(
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    clip: &ClipBox,
    shown: Shown,
    start: f64,
    x_of: &dyn Fn(f64) -> f32,
    beat_width: f32,
    timeline: &Timeline,
    selected: bool,
) {
    let view = painter.clip_rect();
    if !rect.intersects(view) {
        return;
    }
    // The clip in its colour (its own, or its track's): the name bar in full colour, the body a
    // shade of it, and its notes or waveform in ink that reads on both. Off or muted, it greys.
    let off = lane.mute || !clip.enabled;
    let base = if off { motion::mix(rgb(clip.color), theme::text_hint(), 0.7) } else { rgb(clip.color) };
    let body = motion::mix(theme::window_background(), base, if selected { 0.85 } else { 0.62 });
    painter.rect_filled(rect, 0.0, body);
    let bar = clip_name_bar(rect);
    if let Some(bar) = bar {
        painter.rect_filled(bar, 0.0, base);
    } else {
        painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)), 0.0, base);
    }
    let ink = ink_on(body);
    // A looping clip's passes are marked by the notches and lines at their starts (below), not by
    // shades: the body is one colour all along.
    // The grid shows through the body, faintly, the bars a little stronger.
    let lines = painter.ctx().data(|d| d.get_temp::<std::sync::Arc<Vec<(f32, bool)>>>(Id::new(CLIP_GRID)));
    if let Some(lines) = lines.filter(|l| !l.is_empty()) {
        let below = bar.map_or(rect.top() + 2.0, |bar| bar.bottom());
        let span = egui::Rangef::new(below, rect.bottom());
        let (beat_ink, bar_ink) = (motion::mix(body, ink, 0.035), motion::mix(body, ink, 0.07));
        for &(x, on_bar) in lines.iter().filter(|(x, _)| *x > rect.left() + 1.0 && *x < rect.right() - 1.0) {
            painter.vline(x, span, theme::hairline(if on_bar { bar_ink } else { beat_ink }));
        }
    }
    // Notes an ARA plug-in found in an audio clip go over its waveform, which dims a little under
    // them so both read (ADR 0028, #720): dimmed to 40 % it read as an empty clip on a light body.
    let ara_notes = timeline.ara_notes.get(&clip.id).filter(|_| clip.audio).zip(timeline.tempo.as_ref());
    let wave_ink = if ara_notes.is_some() { motion::mix(body, ink, 0.75) } else { ink };
    let dim = |_: Color32| wave_ink;
    let color = ink;
    let text_color = ink_on(if bar.is_some() { base } else { body });
    // The name's place, for a rename in place (Ctrl+R).
    let strip = Rect::from_min_max(
        Pos2::new(rect.left().max(view.left()), rect.top()),
        Pos2::new(rect.right().min(view.right()), rect.top() + 18.0),
    );
    let pass = painter.ctx().cumulative_pass_nr();
    painter.ctx().data_mut(|d| {
        d.insert_temp(Id::new((crate::app::NAME_RECT_CLIP, u64::from(clip.id.0))), (strip, pass));
    });
    let has_name = rect.height() >= 20.0;
    if has_name {
        // The name stays in view while the clip's start is scrolled off to the left.
        let left = rect.left().max(view.left()) + 4.0;
        let name_rect =
            Rect::from_min_max(Pos2::new(left, rect.top() + 3.0), Pos2::new(rect.right() - 2.0, rect.top() + 15.0));
        widgets::text_in(
            painter,
            name_rect,
            Align2::LEFT_CENTER,
            &clip.name,
            theme::ui_font(theme::TEXT_SMALL),
            text_color,
        );
    }
    let marks = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + if has_name && rect.height() >= 30.0 { 17.0 } else { 4.0 }),
        Pos2::new(rect.right(), rect.bottom() - 3.0),
    );
    if let Some(view) = clip.audio_view.as_ref().filter(|_| marks.height() >= 4.0)
        && let Some(peaks) = timeline.waveforms.get(&view.file)
    {
        // One column per point in view: the stretch of the file it shows, scaled by the clip's
        // gain and its fades, drawn as `docs/spec/waveforms.md` says, inside the clip.
        let gain = if shown.gain_db.is_finite() { 10f32.powf(shown.gain_db as f32 / 20.0) } else { 0.0 };
        // The waveform zoom: drawn larger, heard the same.
        let zoom = painter.ctx().data(|d| d.get_temp::<f32>(Id::new(WAVEFORM_ZOOM))).unwrap_or(1.0);
        let gain = gain * zoom;
        let from = rect.left().max(view_rect(painter).left()).floor();
        let to = rect.right().min(view_rect(painter).right()).ceil();
        let per_point = 1.0 / f64::from(beat_width);
        let count = (to - from).max(0.0) as usize;
        // The file stays where it plays in the song while an edge is dragged: `start` is the
        // clip's start, which the left edge (`rect`) has moved away from, so a trim only takes
        // away what it covers. The fades go by the edges.
        let anchor = x_of(start);
        let length = f64::from(rect.width() / beat_width);
        let columns: Vec<waveform::Column> = (0..count)
            .map(|i| {
                let x = from + i as f32;
                let elapsed = f64::from(x - anchor) * per_point;
                let from_edge = f64::from(x - rect.left()) * per_point;
                let gain = gain * fade_gain(from_edge + per_point / 2.0, length, &shown);
                let (a, b) = (view.seconds(elapsed), view.seconds(elapsed + per_point));
                waveform::Column { x: x + 0.5, from: a.min(b), to: a.max(b), gain }
            })
            .collect();
        let shape = waveform::geometry(peaks, marks, &columns, waveform::scale(painter.ctx()));
        waveform::paint(
            &painter.with_clip_rect(marks.intersect(painter.clip_rect())),
            &shape,
            waveform::Colors::from(dim(color)),
        );
    } else if clip.audio {
        painter.hline(marks.x_range(), marks.center().y.round() + 0.5, theme::hairline(dim(color)));
    } else if !clip.notes.is_empty() && marks.height() >= 4.0 {
        let low = clip.notes.iter().map(|n| n.key).min().unwrap_or(0);
        let high = clip.notes.iter().map(|n| n.key).max().unwrap_or(low);
        let rows = f32::from(high - low) + 1.0;
        // Only the notes in view are drawn; a long looped clip has thousands.
        let first = f64::from((view.left() - x_of(start)) / beat_width) - 1.0;
        let last = f64::from((view.right() - x_of(start)) / beat_width) + 1.0;
        // A looping clip's marks run past its end (for a drag of its edge): only those within
        // the span drawn, the clip's or the one its dragged edge gives it.
        let span_end = f64::from((rect.right() - x_of(start)) / beat_width);
        // Dark on the clip's colour, as notes read on a vivid body; light ink read as a
        // highlight and vanished on the light colours.
        let note_ink = motion::mix(body, Color32::BLACK, 0.7);
        for note in clip.notes.iter().filter(|n| n.start + n.length >= first && n.start <= last && n.start < span_end) {
            let x = x_of(start + note.start);
            let right = x_of(start + span_end);
            let width = (note.length as f32 * beat_width - 1.0).max(1.0).min(right - x);
            let row = f32::from(note.key - low);
            let y = marks.bottom() - (row + 0.5) / rows * marks.height();
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y - 1.0), Vec2::new(width, 2.0)), 0.0, note_ink);
        }
    }
    if let Some((notes, tempo)) = ara_notes
        && marks.height() >= 4.0
    {
        paint_ara_notes(painter, rect, marks, notes, tempo, start, clip.length, x_of, beat_width, ink);
    }
    // An ARA plug-in loading or analysing the track's audio: said on the clip, which shows no
    // notes yet and cannot be edited in it until it is done.
    if clip.audio
        && let Some((name, progress)) = timeline.ara_activity.get(&lane.track)
        && marks.height() >= 4.0
    {
        paint_ara_activity(painter, marks, body, ink, name, *progress);
    }
    // Where a looping clip starts another pass: a notch in its top and bottom edges and a
    // dashed line between them, the repeats drawn as the clip itself (#700).
    if !clip.seams.is_empty() {
        // Dark, as the notches cut into the clip are: a light line read as a note or the playhead.
        let mark = motion::mix(body, theme::window_background(), 0.6);
        let top = bar.map_or(rect.top(), |bar| bar.bottom());
        let (first, last) = (view.left() - 4.0, view.right() + 4.0);
        // As wide as the passes allow: zoomed out, a pass a few points wide keeps its body and
        // still shows a small notch.
        let pass =
            clip.seams.windows(2).map(|w| w[1] - w[0]).chain(clip.seams.first().copied()).fold(f64::MAX, f64::min);
        let pass_width = (pass.max(0.0) as f32) * beat_width;
        let notch = (pass_width * 0.1).clamp(1.5, 4.0);
        for seam in &clip.seams {
            let x = x_of(start + seam).round() + 0.5;
            if x < first || x > last || x <= rect.left() || x >= rect.right() {
                continue;
            }
            painter.add(egui::Shape::dashed_line(
                &[Pos2::new(x, top), Pos2::new(x, rect.bottom())],
                theme::hairline(mark),
                3.0,
                3.0,
            ));
            // Cut out of the clip, in the lane's colour, so they show on a light name bar too.
            let cut = theme::window_background();
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x - notch, rect.top()),
                    Pos2::new(x + notch, rect.top()),
                    Pos2::new(x, rect.top() + notch),
                ],
                cut,
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x - notch, rect.bottom()),
                    Pos2::new(x, rect.bottom() - notch),
                    Pos2::new(x + notch, rect.bottom()),
                ],
                cut,
                egui::Stroke::NONE,
            ));
        }
    }
    // A fade is a line over the waveform, from the clip's bottom corner to where it ends at
    // the top, following its shape and curve.
    if clip.audio {
        let fade_stroke = theme::hairline(dim(theme::text_secondary()));
        if shown.fade_in > 0.0 {
            let end = (rect.left() + shown.fade_in as f32 * beat_width).min(rect.right());
            painter.line(fade_line(rect, FadeEdge::In, rect.left(), end, shown.fade_in_curve), fade_stroke);
        }
        if shown.fade_out > 0.0 {
            let from = (rect.right() - shown.fade_out as f32 * beat_width).max(rect.left());
            painter.line(fade_line(rect, FadeEdge::Out, from, rect.right(), shown.fade_out_curve), fade_stroke);
        }
    }
    if selected {
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, theme::text_primary()), StrokeKind::Inside);
    }
}

/// The part of a clip a drag moves it from: its lower half, on a lane tall enough to have an
/// upper half for painting time; `None` when the whole clip moves it.
pub(super) fn clip_grab_area(rect: Rect) -> Option<Rect> {
    (rect.height() >= CLIP_BODY_LEAST).then(|| Rect::from_min_max(Pos2::new(rect.left(), rect.center().y), rect.max))
}

/// The bar along a clip's top that carries its name, on a lane tall enough to have a body under
/// it (a title bar); the drag that moves the clip takes the whole upper half.
pub(super) fn clip_name_bar(rect: Rect) -> Option<Rect> {
    (rect.height() >= CLIP_BODY_LEAST).then(|| Rect::from_min_size(rect.min, Vec2::new(rect.width(), CLIP_NAME_BAR)))
}

/// An ARA plug-in loading or analysing a clip's audio, over the clip's waveform: the body dimmed,
/// `"<plug-in> loading"` or `"<plug-in> analysing N %"` in the middle of what is in view, and a
/// thin bar along the bottom that fills with the analysis.
pub(super) fn paint_ara_activity(
    painter: &egui::Painter,
    marks: Rect,
    body: Color32,
    ink: Color32,
    name: &str,
    progress: Option<f32>,
) {
    let view = marks.intersect(painter.clip_rect());
    if !view.is_positive() {
        return;
    }
    painter.rect_filled(view, 0.0, body.gamma_multiply(0.55));
    let text = match progress {
        None => format!("{name} loading\u{2026}"),
        Some(p) => format!("{name} analysing {} %", (p * 100.0).round() as u32),
    };
    if view.width() >= 60.0 && view.height() >= 12.0 {
        widgets::text_in(painter, view, Align2::CENTER_CENTER, &text, theme::ui_font(theme::TEXT_SMALL), ink);
    }
    let track = Rect::from_min_max(Pos2::new(marks.left(), marks.bottom() - 2.0), marks.right_bottom());
    painter.rect_filled(track, 0.0, motion::mix(body, ink, 0.2));
    if let Some(p) = progress {
        let done = Rect::from_min_max(track.min, Pos2::new(track.left() + track.width() * p, track.bottom()));
        painter.rect_filled(done, 0.0, theme::accent());
    }
}

/// Text and marks that read on `background`: dark on a light colour, light on a dark one.
pub(super) fn ink_on(background: Color32) -> Color32 {
    let [r, g, b, _] = background.to_array();
    let light = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    if light > 150.0 { Color32::from_black_alpha(200) } else { Color32::from_white_alpha(225) }
}
