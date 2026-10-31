// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip on a lane: its box, its drags (move, edges, fades, gain), and how it is painted.

use super::*;

/// The keys held over a clip's edge drag: Alt alone lets the edge off the grid, as Alt does for
/// every other drag; Shift (or Ctrl and Alt) stretches an audio clip's audio with it (Settings ›
/// Arrangement).
pub(super) fn edge_keys(ui: &Ui) -> (bool, bool) {
    // Off the grid with Alt; a stretch with Shift (Alt and Shift: a free stretch), or Ctrl and
    // Alt as before (#833).
    ui.input(|i| {
        let stretch = i.modifiers.shift || (i.modifiers.alt && i.modifiers.command);
        (i.modifiers.alt && !i.modifiers.command, stretch)
    })
}

/// Where a clip's edges are while `drag` moves one: on the grid (free with `free`), kept at
/// least a grid step (or a 16th of a beat) long, its left edge no earlier than its content's
/// start and its right edge no later than its audio's end (none for a looping clip), as
/// `clip.trim` keeps them. A stretch (`stretch`) scales the audio to the new length, so it has
/// neither limit, only the song's start (#833).
pub(super) fn edge_extent(
    clip: &ClipBox,
    drag: EdgeDrag,
    grid: &TimelineGrid,
    free: bool,
    stretch: bool,
) -> (f64, f64) {
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
        EdgeGrab::Left if stretch => (beat.min(end - least).max(0.0), end),
        EdgeGrab::Left => (beat.max(clip.earliest_start).min(end - least).max(0.0), end),
        EdgeGrab::Right if stretch => (clip.start, beat.max(clip.start + least)),
        EdgeGrab::Right => {
            let to = beat.max(clip.start + least);
            (clip.start, clip.latest_end.map_or(to, |last| to.min(last.max(clip.start + least))))
        }
    }
}

/// Where `clip`'s edges go when another selected clip's `grab` edge is dragged `by` beats: its
/// same edge moves as far, within its own limits, as `clip.trimMany` does (#956).
pub(super) fn edge_by(clip: &ClipBox, grab: EdgeGrab, by: f64, grid: &TimelineGrid) -> (f64, f64) {
    let edge = if grab == EdgeGrab::Left { clip.start } else { clip.start + clip.length };
    let drag = EdgeDrag { key: (TrackId(0), clip.id), grab, raw: edge + by, from: edge, origin: 0.0 };
    edge_extent(clip, drag, grid, true, false)
}

/// `extent` (a clip's new start and end) kept off the other selected clips of its lane, as
/// `clip.trimMany` keeps it: an end moved on stops where the next of them begins, a start moved
/// back where the one before ends.
pub(super) fn off_the_selection(
    extent: (f64, f64),
    clip: &ClipBox,
    lane: &Lane,
    selected: &[(TrackId, ClipId)],
) -> (f64, f64) {
    let end = clip.start + clip.length;
    let others = || lane.clips.iter().filter(|c| c.id != clip.id && selected.contains(&(lane.track, c.id)));
    let next = others().filter(|c| c.start >= end - 1e-9).map(|c| c.start).fold(f64::INFINITY, f64::min);
    let previous = others()
        .filter(|c| c.start + c.length <= clip.start + 1e-9)
        .map(|c| c.start + c.length)
        .fold(f64::NEG_INFINITY, f64::max);
    let to = if extent.1 > end { extent.1.min(next.max(end)) } else { extent.1 };
    let from = if extent.0 < clip.start { extent.0.max(previous.min(clip.start)) } else { extent.0 };
    (from, to)
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
    // Just moved or trimmed with its selection: drawn where it went until the project has it.
    let pass = ui.ctx().cumulative_pass_nr();
    state.settling.retain(|s| {
        s.key != key
            || !((clip.start - s.from).abs() < 1e-9 && (clip.start + clip.length - s.to).abs() < 1e-9 || pass > s.until)
    });
    let settled = state.settling.iter().find(|s| s.key == key).copied();
    let at = match settled {
        Some(s) if s.moved => s.from,
        _ => at,
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
    // moving the clip), or the upper half (Lower half, the lower half moving it); with Ctrl held
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
            menus::ara_menu(ui, lane.track, clip.id, name, clip.ara, actions);
        }
    });
    let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
    let tool = state.tool;
    // A drag that selects time rather than moving the clip: with Ctrl held, or from the clip's
    // lower half on a lane tall enough to have one, as the setting says; with the Range tool,
    // from anywhere (#991).
    let selects_time = response.drag_started_by(egui::PointerButton::Primary)
        && (tool == ArrangementTool::Range
            || (tool == ArrangementTool::Select
                && ui
                    .input(|i| i.pointer.press_origin())
                    .is_some_and(|origin| if state.time_select_ctrl { command } else { selects_at(origin.y) })));
    // The Split, Erase and Mute tools act on the clip clicked, and nothing else does (#991).
    let tool_click =
        response.clicked() && matches!(tool, ArrangementTool::Split | ArrangementTool::Erase | ArrangementTool::Mute);
    if tool_click && let Some(pointer) = response.interact_pointer_pos() {
        match tool {
            ArrangementTool::Split => {
                let raw = clip.start + f64::from((pointer.x - x_of(clip.start)) / state.beat_width);
                let on_grid = snapped(raw);
                let free =
                    ui.input(|i| i.modifiers.alt) || on_grid <= clip.start || on_grid >= clip.start + clip.length;
                let at = if free { raw } else { on_grid };
                actions.push(Action::new("clip.split", json!({ "clip": clip.id.0, "at": at })));
            }
            ArrangementTool::Erase => actions.push(Action::new("clip.delete", json!({ "clip": clip.id.0 }))),
            _ => actions.push(Action::new(
                "clip.setEnabled",
                json!({ "clips": [{ "clip": clip.id.0 }], "enabled": !clip.enabled }),
            )),
        }
    }
    if response.hovered() {
        match tool {
            ArrangementTool::Split | ArrangementTool::Draw => ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair),
            ArrangementTool::Erase | ArrangementTool::Mute => ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand),
            ArrangementTool::Range => ui.ctx().set_cursor_icon(egui::CursorIcon::Text),
            ArrangementTool::Select => {}
        }
    }
    if selects_time {
        state.marquee = ui.input(|i| i.pointer.press_origin());
        state.marquee_scroll = state.scroll;
        state.marquee_time = true;
        state.marquee_row = (clip.rows > 1).then_some((lane.track, clip.row, clip.rows));
    } else if response.clicked() || response.drag_started_by(egui::PointerButton::Primary) {
        // Clips picked by a click or moved: the time selection gives way to them.
        state.time_selection = None;
    }
    if response.double_clicked() {
        state.clip_opened = true;
    }
    if response.clicked() && !tool_click {
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
            // The painting part of the clip (or the Range tool); where the clip is picked or moved,
            // it is picked whole, however its start lies on the grid.
            Some(pointer) if tool == ArrangementTool::Range || (!on_bar(pointer.y) && in_body(pointer.y)) => {
                let free = ui.input(|i| i.modifiers.alt);
                let raw = f64::from((pointer.x - x_of(0.0)) / state.beat_width);
                let beat = clip_body_beat(raw, clip.start, grid, free);
                state.picked_whole = false;
                state.insert = Some((lane.track, beat));
                // The next start begins there; playback is not moved (#657).
                actions.push(Action::new("transport.setStart", json!({ "beats": beat })));
            }
            _ => {
                state.picked_whole = true;
                state.whole_clip_clicked = true;
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
    if response.drag_started_by(egui::PointerButton::Primary) && !selects_time && tool == ArrangementTool::Select {
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
        // Over a track of the other kind that holds clips, the drop is refused, and the pointer
        // says so; an empty one takes the clip's kind (#945).
        if let Some(target) = timeline.lanes.iter().find(|l| l.track == drag.target && l.track != lane.track) {
            let other =
                if clip.audio { !target.plays_audio && target.notes } else { !target.notes && target.plays_audio };
            let holds = !target.clips.is_empty() || timeline.takes.iter().any(|t| t.track == target.track);
            if other && holds {
                ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
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
            // this one, by as many tracks when it is on several, or to a new track. On its own
            // tracks, each clip is drawn where it went until the project has it.
            if moved && !drag.new_track && drag.target == lane.track && tracks_by == 0 {
                let by = start - clip.start;
                let until = ui.ctx().cumulative_pass_nr() + 30;
                for &(track, id) in &state.selected {
                    if let Some(c) = timeline
                        .lanes
                        .iter()
                        .find(|l| l.track == track)
                        .and_then(|l| l.clips.iter().find(|c| c.id == id))
                    {
                        let from = (c.start + by).max(0.0);
                        state.settling.push(Settling {
                            key: (track, id),
                            from,
                            to: from + c.length,
                            moved: true,
                            until,
                        });
                    }
                }
            }
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
                if drag.target == lane.track {
                    let until = ui.ctx().cumulative_pass_nr() + 30;
                    state.settling.push(Settling { key, from: start, to: start + clip.length, moved: true, until });
                }
            }
        }
        // Dropped on another track: the view draws it there, as while it was dragged, until the
        // project has moved it.
        if moved && !copying && (drag.target != lane.track || drag.new_track) {
            state.landing = Some(ClipDrag { start, ..drag });
            return Some((lane, clip, start));
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
    // Another selected clip's edge dragged: this one's same edge moves as far (#956); or it was
    // just trimmed with them and the project does not have it yet.
    let others_edge = state
        .edge_drag
        .filter(|d| d.key != key && state.selected.len() > 1 && state.is_selected(d.key) && state.is_selected(key))
        .map(|d| edge_by(clip, d.grab, grid.place(d.raw, edge_keys(ui).0, false) - d.from, &grid))
        .map(|extent| off_the_selection(extent, clip, lane, &state.selected))
        .or(settled.filter(|s| !s.moved).map(|s| (s.from, s.to)));
    let released = released.or(others_edge);
    // Copying leaves the clip where it is: its handles stay on it; moving takes them along.
    let copy_drag = state.clip_drag.is_some_and(|d| d.copy || copy_key);
    let base = if copy_drag { clip.start } else { at };
    let (live_start, live_length) = match state.edge_drag.filter(|d| d.key == key) {
        Some(edge) => {
            let stretching = edge_keys(ui).1 && state.stretch_alt && clip.audio;
            let mut extent = edge_extent(clip, edge, &grid, edge_keys(ui).0, stretching);
            // One of several selected clips: it stops at the next of them, as the command does.
            if !stretching && state.selected.len() > 1 && state.is_selected(key) {
                extent = off_the_selection(extent, clip, lane, &state.selected);
            }
            let (from, to) = extent;
            (from, to - from)
        }
        None => released.map_or((base, clip.length), |(from, to)| (from, to - from)),
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
    if state.clip_drag.is_none_or(|d| !mine(&d)) && edge_rect.width() >= 12.0 && tool == ArrangementTool::Select {
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
                let stretch = stretch_held && state.stretch_alt && clip.audio;
                let (from, to) = edge_extent(clip, drag, &grid, free, stretch);
                let beat = if grab == EdgeGrab::Left { from } else { to };
                let mut params = json!({ "clip": clip.id.0 });
                params[if grab == EdgeGrab::Left { "start" } else { "end" }] = json!(beat);
                if (clip.audio && state.stack_audio) || (!clip.audio && state.stack_midi) {
                    params["stack"] = json!(true);
                }
                let old = if grab == EdgeGrab::Left { clip.start } else { clip.start + clip.length };
                // Between two parts of a comp the edge is their cut: both move (#989).
                let end = clip.start + clip.length;
                let cut = clip.take.and(match grab {
                    EdgeGrab::Right => lane
                        .clips
                        .iter()
                        .any(|c| c.take.is_some() && c.id != clip.id && (c.start - end).abs() < 1e-6)
                        .then_some(clip.id),
                    EdgeGrab::Left => lane
                        .clips
                        .iter()
                        .find(|c| c.take.is_some() && c.id != clip.id && (c.start + c.length - clip.start).abs() < 1e-6)
                        .map(|c| c.id),
                });
                if (beat - old).abs() > 1e-9
                    && let Some(before) = cut.filter(|_| !stretch)
                {
                    actions.push(Action::new("take.moveCut", json!({ "clip": before.0, "to": beat })));
                    state.edge_released = Some((key, from, to, 30));
                } else if (beat - old).abs() > 1e-9 {
                    if !stretch && state.selected.len() > 1 && state.is_selected(key) {
                        // Every selected clip's same edge moves as far, in one step (#956).
                        let clips: Vec<Value> = state.selected.iter().map(|(_, id)| json!({ "clip": id.0 })).collect();
                        let edge = if grab == EdgeGrab::Left { "start" } else { "end" };
                        let mut many = json!({ "clips": clips, "edge": edge, "by": beat - old });
                        if params.get("stack").is_some() {
                            many["stack"] = json!(true);
                        }
                        actions.push(Action::new("clip.trimMany", many));
                        let until = ui.ctx().cumulative_pass_nr() + 30;
                        for &(track, id) in &state.selected {
                            if let Some(c) = timeline
                                .lanes
                                .iter()
                                .find(|l| l.track == track)
                                .and_then(|l| l.clips.iter().find(|c| c.id == id))
                            {
                                let (from, to) = match timeline.lanes.iter().find(|l| l.track == track) {
                                    Some(l) => {
                                        off_the_selection(edge_by(c, grab, beat - old, &grid), c, l, &state.selected)
                                    }
                                    None => edge_by(c, grab, beat - old, &grid),
                                };
                                state.settling.push(Settling { key: (track, id), from, to, moved: false, until });
                            }
                        }
                    } else {
                        actions.push(Action::new(if stretch { "clip.stretch" } else { "clip.trim" }, params));
                    }
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
                ui.separator();
                // Back to no fade, its shape and curve the defaults.
                if ui.button("Reset fade").clicked() {
                    let (length, shape, bend) = if edge == FadeEdge::In {
                        ("in", "inShape", "inCurve")
                    } else {
                        ("out", "outShape", "outCurve")
                    };
                    let mut params = json!({ "clip": clip.id.0 });
                    params[length] = json!(0.0);
                    params[shape] = json!(FadeShape::default().name());
                    params[bend] = json!(0.0);
                    actions.push(Action::new("clip.setFades", params));
                    ui.close();
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
                if state.is_selected(key) && state.selected.len() > 1 && !response.double_clicked() {
                    // One of several selected clips: every selected audio clip's fade on this
                    // edge takes the curve, in one step, as their lengths do (#715).
                    let audio = |(track, id): &(TrackId, ClipId)| {
                        timeline
                            .lanes
                            .iter()
                            .find(|l| l.track == *track)
                            .is_some_and(|l| l.clips.iter().any(|c| c.id == *id && c.audio))
                    };
                    let clips: Vec<Value> =
                        state.selected.iter().filter(|k| audio(k)).map(|(_, id)| json!({ "clip": id.0 })).collect();
                    let param = if edge == FadeEdge::In { "in" } else { "out" };
                    actions.push(Action::new("clip.shiftFades", json!({ "clips": clips, "edge": param, "curve": v })));
                } else if (v - stored).abs() > 1e-9 {
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
                Some(edge) => {
                    let stretching = edge_keys(ui).1 && state.stretch_alt && clip.audio;
                    Some(edge_extent(clip, edge, &grid, free, stretching))
                }
                None => {
                    state.edge_released.filter(|(k, ..)| *k == key).map(|(_, from, to, _)| (from, to)).or(others_edge)
                }
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
        let edge = Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height()));
        let body = motion::mix(theme::panel_background(), rgb(clip.color), 0.82);
        painter.with_clip_rect(row.clip).rect_filled(edge, 0.0, motion::mix(body, ink_on(body), 0.5));
        painter.with_clip_rect(row.clip).rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, theme::text_primary()),
            StrokeKind::Inside,
        );
    }
    // The gain sits on top of the clip.
    if let Some((field, db, hover)) = gain_paint {
        let gain_painter = painter.with_clip_rect(row.clip);
        // `.clip-gain`: its background, border, radius and text.
        let _style = crate::kit::style_class::enter("clip-gain");
        let fill = motion::mix(theme::control_background(), theme::control_hover(), hover);
        crate::kit::native_css::surface(&gain_painter, field, 0.0, fill);
        let text = format!("{} dB", widgets::format_db(db));
        let font = theme::mono_font(
            crate::kit::style_class::computed()
                .filter(|c| c.authored.font_size)
                .map_or(theme::TEXT_TINY, |c| c.font_size),
        );
        widgets::text_in(
            &gain_painter,
            field.shrink2(Vec2::new(3.0, 0.0)),
            Align2::RIGHT_CENTER,
            &text,
            font,
            crate::kit::style_class::foreground(theme::text_value()),
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
        // `.clip-fade-handle`: its fill (`background-color`), border and radius, or the
        // `clip.fade-handle` artwork.
        let _style = crate::kit::style_class::enter("clip-fade-handle");
        let fill = motion::mix(theme::text_secondary(), theme::text_primary(), hover);
        let state = if hover > 0.5 { crate::kit::skin::State::Hover } else { crate::kit::skin::State::Normal };
        if !crate::kit::skin::paint(&handle_painter, handle, "clip.fade-handle", state, fill, 0.0) {
            let authored = crate::kit::style_class::computed();
            if authored.as_ref().is_some_and(|c| c.authored.background || c.authored.border) {
                crate::kit::native_css::surface(&handle_painter, handle, 0.0, fill);
            } else {
                handle_painter.rect_filled(handle, 0.0, fill);
                handle_painter.rect_stroke(
                    handle,
                    0.0,
                    theme::hairline(theme::window_background()),
                    StrokeKind::Outside,
                );
            }
        }
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
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("arrangement clip");
    let _style_class = crate::kit::style_class::enter_named("clip", &format!("clip-{}", clip.id.0));
    let view = painter.clip_rect();
    if !rect.intersects(view) {
        return;
    }
    let off = lane.mute || !clip.enabled;
    crate::kit::native_css::set_variable("--clip-colour", crate::kit::native_css::hex(rgb(clip.color)));
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Selected, selected);
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Muted, off);
    // `:playing` while the play position is inside it, `:recording` while a take records over it.
    let end = clip.start + clip.length;
    let playing = timeline.running && !off && (clip.start..end).contains(&timeline.beat);
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Playing, playing);
    let recording = timeline
        .recording_spans
        .iter()
        .any(|(track, from, to)| *track == lane.track && *from < end && *to > clip.start);
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Recording, recording);
    // The sheet's opacity fades the whole clip, its waveform, notes and name too.
    let faded;
    let painter = match crate::kit::style_class::computed().map(|style| style.opacity).filter(|o| *o < 1.0) {
        Some(opacity) => {
            let mut fade = painter.clone();
            fade.multiply_opacity(opacity.clamp(0.0, 1.0));
            faded = fade;
            &faded
        }
        None => painter,
    };
    // Keep the clip colour legible against the quiet grid. Selection changes brightness,
    // never hue; the reserved title area leaves notes and waveforms unobstructed.
    let strength = if clip.audio {
        painter.ctx().data(|d| d.get_temp::<u8>(Id::new(AUDIO_COLOUR_STRENGTH))).unwrap_or(82)
    } else {
        82
    };
    let strength = if selected { strength.saturating_add(12).min(100) } else { strength };
    let body =
        crate::kit::style_class::background(colour_surface(theme::panel_background(), rgb(clip.color), strength, off));
    let radius = crate::kit::style_class::radius(2.0);
    // A clip is content, not a background: it blends with the lane under it, so its alpha never
    // reaches the window and the desktop never shows through it.
    painter.rect_filled(rect, radius, body);
    crate::kit::native_css::background_rounded(painter, rect, radius);
    let bar = clip_name_bar(rect);
    if let Some(bar) = bar {
        let _name_bar = crate::kit::style_class::enter("clip-name");
        let fill = crate::kit::style_class::background(motion::mix(body, ink_on(body), 0.045));
        // The bar keeps the clip's rounded top corners.
        let top = radius.min(bar.height()).clamp(0.0, 255.0) as u8;
        painter.rect_filled(
            bar.shrink2(Vec2::new(2.0, 0.0)),
            egui::CornerRadius { nw: top, ne: top, sw: 0, se: 0 },
            fill,
        );
    } else {
        painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)), 0.0, body);
    }
    crate::kit::liquid::paint(painter, rect.shrink(1.0), body, crate::kit::liquid::Surface::Clip);
    let ink = ink_on(body);
    let edge_width = if selected { 4.0 } else { 3.0 };
    let edge_ink = motion::mix(body, ink, 0.5);
    if let Some(bar) = bar {
        // A quiet divider ties the title to the stronger leading edge without framing the preview.
        painter.hline(
            (rect.left() + edge_width)..=rect.right(),
            bar.bottom() - 0.5,
            theme::hairline(motion::mix(body, edge_ink, 0.4)),
        );
    }
    // The leading edge follows the clip's rounded left corners.
    let left = radius.clamp(0.0, 255.0) as u8;
    painter.rect_filled(
        // Inset from rounder corners, so it never stands outside them.
        Rect::from_min_size(rect.min, Vec2::new(edge_width, rect.height()))
            .shrink2(Vec2::new(0.0, (radius - edge_width).max(0.0).min(rect.height() / 2.0 - 1.0).max(0.0))),
        egui::CornerRadius { nw: left, sw: left, ne: 0, se: 0 },
        edge_ink,
    );
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
    let appearance = painter
        .ctx()
        .data(|d| d.get_temp::<crate::settings::view_settings::WaveformAppearance>(Id::new(WAVEFORM_APPEARANCE)))
        .unwrap_or_default();
    // Theme: the sheet's `.clip-waveform` and `.clip-notes` colours, else Dark.
    let themed = |class: &str| {
        let _part = crate::kit::style_class::enter(class);
        crate::kit::style_class::foreground(appearance.ink(ink))
    };
    let content_ink = if appearance.mode == crate::settings::view_settings::WaveformColour::Theme {
        themed(if clip.audio { "clip-waveform" } else { "clip-notes" })
    } else {
        appearance.ink(ink)
    };
    let wave_ink = if ara_notes.is_some() { motion::mix(body, content_ink, 0.75) } else { content_ink };
    let dim = |_: Color32| wave_ink;
    let color = ink;
    let text_color = ink_on(body);
    // The name's place, for a rename in place (Ctrl+R).
    let strip = Rect::from_min_max(
        Pos2::new(rect.left().max(view.left()), rect.top()),
        Pos2::new(rect.right().min(view.right()), rect.top() + 18.0),
    );
    let pass = painter.ctx().cumulative_pass_nr();
    painter.ctx().data_mut(|d| {
        // The name bar's colour, as it is drawn above (a hair lighter or darker than the body).
        let fill = if bar.is_some() { motion::mix(body, ink_on(body), 0.045) } else { body };
        let size = crate::kit::style_class::font_size(theme::TEXT);
        let spot = crate::app::NameSpot { rect: strip, pass, fill, text: text_color, size };
        d.insert_temp(Id::new((crate::app::NAME_RECT_CLIP, u64::from(clip.id.0))), spot);
    });
    let has_name = bar.is_some();
    if has_name {
        // The name stays in view while the clip's start is scrolled off to the left.
        let left = rect.left().max(view.left()) + 7.0;
        let name_rect = Rect::from_min_max(
            Pos2::new(left, rect.top() + 3.0),
            Pos2::new(rect.right() - 5.0, bar.map_or(rect.top(), |bar| bar.bottom()) - 3.0),
        );
        let _name = crate::kit::style_class::enter("clip-name");
        let font = theme::ui_font(crate::kit::style_class::font_size(theme::TEXT));
        let text_color = crate::kit::style_class::foreground(text_color);
        widgets::text_in(painter, name_rect, Align2::LEFT_CENTER, &clip.name, font, text_color);
    }
    let marks = Rect::from_min_max(
        Pos2::new(rect.left() + 3.0, bar.map_or(rect.top() + 4.0, |bar| bar.bottom() + 3.0)),
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
        let note_ink = content_ink;
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
        paint_ara_notes(painter, rect, marks, notes, tempo, start, clip.length, x_of, beat_width, content_ink);
    }
    // An ARA plug-in loading or analysing the track's audio: said on the clip, which shows no
    // notes yet and cannot be edited in it until it is done.
    if clip.audio
        && let Some((name, progress)) = timeline.ara_activity.get(&lane.track)
        && marks.height() >= 4.0
    {
        paint_ara_activity(painter, marks, body, ink, name, *progress);
    }
    // Loop boundaries share the leading edge's contrast, with a small top notch so they
    // remain distinct from the grid and playhead at every lane height.
    let span = f64::from((rect.right() - x_of(start)) / beat_width);
    let preview_seams = (span > clip.length + 1e-6)
        .then(|| crate::arrangement::timeline::loop_seams_for_span(clip.loop_range, clip.offset, span));
    let seams = preview_seams.as_deref().unwrap_or(&clip.seams);
    let pass_width = clip.loop_range.map_or(0.0, |range| (range.end - range.start) as f32 * beat_width);
    for seam in seams {
        let x = x_of(start + seam).round() + 0.5;
        if x < view.left() - 4.0 || x > view.right() + 4.0 || x <= rect.left() || x >= rect.right() {
            continue;
        }
        painter.vline(x, rect.y_range(), egui::Stroke::new(1.0, motion::mix(body, edge_ink, 0.5)));
        // Dense passes should not turn the top edge into a sawtooth.
        if pass_width < 64.0 {
            continue;
        }
        let notch = 2.5_f32.min(pass_width * 0.012).min(rect.height() * 0.1);
        // Only a shallow circular segment cuts into the clip; leave the grid above intact.
        painter.with_clip_rect(painter.clip_rect().intersect(rect)).circle_filled(
            Pos2::new(x, rect.top() - notch * 0.5),
            notch,
            theme::window_background(),
        );
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
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, motion::mix(body, ink, 0.45)), StrokeKind::Inside);
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
    (rect.height() >= CLIP_BODY_LEAST).then(|| {
        // Leave the lower-half body gesture clear even on compact two-row lanes.
        let height = CLIP_NAME_BAR.min(rect.height() * 0.5 - 1.0);
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), height))
    })
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

/// Keep hue and dim a muted source by reducing its strength over the same theme surface.
pub(super) fn colour_surface(background: Color32, source: Color32, strength: u8, dim: bool) -> Color32 {
    let amount = f32::from(strength.min(100)) / 100.0;
    motion::mix(background, source, if dim { amount * 0.55 } else { amount })
}

/// Text and marks that read on `background`: dark on a light colour, light on a dark one.
pub(super) fn ink_on(background: Color32) -> Color32 {
    let [r, g, b, _] = background.to_array();
    let light = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    if light > 150.0 { Color32::from_black_alpha(200) } else { Color32::from_white_alpha(225) }
}

/// Where a track's take lanes go: the timeline's and the headers' x ranges, the top of the
/// first, the lanes' visible area, and the beats in a bar for the grid.
pub(super) struct TakeLanes {
    pub(super) time: egui::Rangef,
    pub(super) headers: egui::Rangef,
    pub(super) top: f32,
    pub(super) visible: Rect,
    pub(super) bar_beats: f64,
}

use crate::arrangement::timeline::take_colour;

/// Draws `lane`'s take lanes under it (`docs/spec/recording.md` › Take lanes): each take's
/// clips dimmed in the take's own colour, as they do not play. A click on a take's clip puts the whole clip on the track,
/// a drag across a take the part dragged over (`take.comp`); the take's name has Delete take in
/// its menu.
#[allow(clippy::too_many_arguments)]
pub(super) fn take_lanes(
    ui: &mut Ui,
    painter: &egui::Painter,
    at: &TakeLanes,
    lane: &Lane,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    x_of: &dyn Fn(f64) -> f32,
    actions: &mut Vec<Action>,
) {
    if state.takes_hidden.contains(&lane.track) {
        return;
    }
    let free = ui.input(|i| i.modifiers.alt);
    let grid = arrangement_grid(state.grid, state.beat_width, at.bar_beats);
    let (scroll_x, beat_width) = (state.scroll.x, state.beat_width);
    let beat_of = |x: f32| ruler_beat(f64::from((x - at.time.min + scroll_x) / beat_width), grid, free);
    let mut y = at.top;
    let take_row = state.take_row(lane.track);
    for take in timeline.takes.iter().filter(|t| t.track == lane.track) {
        let row = Rect::from_x_y_ranges(at.time, egui::Rangef::new(y, y + take_row));
        let header = Rect::from_x_y_ranges(at.headers, row.y_range());
        y += take_row + 1.0;
        // The line under a take lane resizes the track's take lanes, all alike; a double-click
        // puts them back to the default height (#989).
        let edge = Rect::from_x_y_ranges(
            egui::Rangef::new(at.time.min.min(at.headers.min), at.time.max.max(at.headers.max)),
            egui::Rangef::new(row.bottom() - 2.0, row.bottom() + 3.0),
        )
        .intersect(at.visible);
        if edge.height() > 0.0 {
            let resize = ui
                .interact(edge, Id::new(("take-lane-edge", lane.track.0, take.number)), Sense::click_and_drag())
                .on_hover_cursor(egui::CursorIcon::ResizeVertical);
            resize.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, format!("Height of {}", take.name))
            });
            if resize.double_clicked() {
                state.take_heights.remove(&lane.track);
            } else if resize.dragged() {
                let height =
                    (state.take_row(lane.track) + resize.drag_delta().y).clamp(TAKE_ROW_RANGE.0, TAKE_ROW_RANGE.1);
                state.take_heights.insert(lane.track, height);
            }
        }
        painter.hline(
            egui::Rangef::new(at.time.min.min(at.headers.min), at.time.max.max(at.headers.max)),
            row.bottom() + 0.5,
            theme::hairline(theme::border()),
        );
        if row.bottom() < at.visible.top() || row.top() > at.visible.bottom() {
            continue;
        }
        let colour = take.color;
        let params = json!({
            "track": lane.track.0,
            "take": take.number,
            "color": crate::interaction::colour_picker::hex(colour),
        });
        // The header in the take's colour, a darker shade of it, with a strip of the colour itself
        // at its inner edge, as a track's header has (#989).
        let swatch = crate::interaction::colour_picker::color32(colour);
        let shade = motion::mix(theme::window_background(), swatch, 0.35);
        painter.rect_filled(header, 0.0, shade);
        let strip = Rect::from_min_size(header.min, Vec2::new(4.0, header.height()));
        painter.rect_filled(strip, 0.0, swatch);
        let name = ui.interact(
            header.intersect(at.visible),
            Id::new(("take-name", lane.track.0, take.number)),
            Sense::click(),
        );
        name.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, take.name.clone()));
        // The piece a right-click on the lane was over, which Recolor colours alone (#989).
        let piece = state.take_piece.filter(|(t, n, _)| *t == lane.track && *n == take.number).map(|(_, _, p)| p);
        let menu = |ui: &mut Ui, actions: &mut Vec<Action>, piece: Option<usize>| {
            take_menu(ui, lane.track, take, piece, &params, actions)
        };
        name.context_menu(|ui| menu(ui, actions, None));
        // Its buttons, as a track's header has (#989): delete, solo, its colour, and the up arrow
        // that puts the whole take on the track.
        let button = Vec2::splat(16.0);
        let row_y = header.center().y - button.y / 2.0;
        let place = |x: f32| Rect::from_min_size(Pos2::new(x, row_y), button);
        let delete = place(header.left() + 8.0);
        let solo = place(delete.right() + 3.0);
        let colour_button = place(solo.right() + 3.0);
        let promote = place(header.right() - 4.0 - button.x);
        let ink = ink_on(shade);
        let small = theme::ui_font(theme::TEXT_SMALL);
        let soloed = state.take_solo == Some((lane.track, take.number));
        if header.height() >= button.y + 4.0 {
            let id = |what: &str| Id::new(("take-button", what, lane.track.0, take.number));
            let clicked = |rect: Rect, what: &str, label: String| {
                widgets::button_area(ui, rect.intersect(at.visible), id(what), label).on_hover_text(what.to_string())
            };
            let r = clicked(delete, "Delete take", format!("Delete {}", take.name));
            painter.rect_filled(delete, 2.0, motion::mix(shade, theme::control_hover(), widgets::hover(ui, &r)));
            // A cross drawn, not a glyph: the interface font has no multiplication cross.
            let c = delete.center();
            let cross = Stroke::new(1.2, ink);
            painter.line_segment([c + Vec2::new(-3.5, -3.5), c + Vec2::new(3.5, 3.5)], cross);
            painter.line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], cross);
            if r.clicked() {
                actions.push(Action::new("take.delete", json!({ "track": params["track"], "take": params["take"] })));
            }
            let r = clicked(solo, "Solo take", format!("Solo {}", take.name));
            let fill = if soloed {
                theme::accent()
            } else {
                motion::mix(shade, theme::control_hover(), widgets::hover(ui, &r))
            };
            painter.rect_filled(solo, 2.0, fill);
            widgets::text_in(
                painter,
                solo,
                Align2::CENTER_CENTER,
                "S",
                small.clone(),
                if soloed { ink_on(fill) } else { ink },
            );
            if r.clicked() {
                let number = if soloed { 0 } else { take.number };
                actions.push(Action::new("take.solo", json!({ "track": lane.track.0, "take": number })));
            }
            let r = clicked(colour_button, "Recolor lane", format!("Colour of {}", take.name));
            painter.rect_filled(colour_button.shrink(3.0), 2.0, swatch);
            egui::Popup::menu(&r).show(|ui| {
                if let Some(picked) = crate::interaction::colour_picker::swatch_row(ui, Some(colour)) {
                    let hex = crate::interaction::colour_picker::hex(picked);
                    actions.push(Action::new(
                        "take.setColor",
                        json!({ "track": lane.track.0, "take": take.number, "color": hex }),
                    ));
                    ui.close();
                }
            });
            let r = clicked(promote, "Put the whole take on the track", format!("Put {} on the track", take.name));
            painter.rect_filled(promote, 2.0, motion::mix(shade, theme::control_hover(), widgets::hover(ui, &r)));
            widgets::text_in(painter, promote, Align2::CENTER_CENTER, "\u{2191}", small.clone(), ink);
            if r.clicked() {
                actions.push(Action::new("take.promote", json!({ "track": lane.track.0, "take": take.number })));
            }
        }
        let name_left =
            if header.height() >= button.y + 4.0 { colour_button.right() + 6.0 } else { header.left() + 10.0 };
        widgets::text_in(
            painter,
            Rect::from_min_max(Pos2::new(name_left, header.top()), Pos2::new(promote.left() - 4.0, header.bottom())),
            Align2::LEFT_CENTER,
            &take.name,
            small,
            ink,
        );
        // Each piece in its colour, a dark shade of it where the comp does not use it and lit where
        // it does (#989): a take keeps its hue either way.
        for clip in &take.clips {
            let lit = Lane { mute: false, color: clip.color, ..lane.clone() };
            let rect = clip_rect(row, clip.start, clip.length, x_of);
            paint_clip(painter, rect, &lit, clip, Shown::of(clip), clip.start, x_of, beat_width, timeline, false);
            painter.rect_filled(rect.intersect(row), 0.0, theme::window_background().gamma_multiply(0.6));
            for &(start, end) in &take.used {
                let span = Rect::from_x_y_ranges(egui::Rangef::new(x_of(start), x_of(end)), row.y_range());
                let span = span.intersect(rect).intersect(painter.clip_rect());
                if span.width() > 0.5 {
                    let used = painter.with_clip_rect(span);
                    paint_clip(&used, rect, &lit, clip, Shown::of(clip), clip.start, x_of, beat_width, timeline, false);
                }
            }
        }
        let body = ui.interact(
            row.intersect(at.visible),
            Id::new(("take-lane", lane.track.0, take.number)),
            Sense::click_and_drag(),
        );
        body.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Comp from {}", take.name))
        });
        if body.secondary_clicked()
            && let Some(pointer) = body.interact_pointer_pos()
        {
            let beat = f64::from((pointer.x - at.time.min + scroll_x) / beat_width);
            state.take_piece = take
                .clips
                .iter()
                .position(|c| (c.start..c.start + c.length).contains(&beat))
                .map(|p| (lane.track, take.number, p));
        }
        body.context_menu(|ui| menu(ui, actions, piece));
        // A take lane selects: the pointer is the text cursor, a drag picks the part to comp, and
        // the piece a double-click would take shows under the pointer (#953, #989). With Ctrl
        // held it is the ordinary pointer: a click picks a piece, a drag comps nothing.
        let ctrl = ui.input(|i| i.modifiers.command);
        for &(_, _, piece) in state.take_selected.iter().filter(|(t, n, _)| *t == lane.track && *n == take.number) {
            if let Some(clip) = take.clips.get(piece) {
                let rect = clip_rect(row, clip.start, clip.length, x_of);
                painter.rect_stroke(rect, 0.0, Stroke::new(1.5, theme::selection()), StrokeKind::Inside);
            }
        }
        // Ctrl and a click picks a piece, or takes it out of what is picked, so Delete deletes
        // them; a click without Ctrl lets go of them all (#989).
        if body.clicked()
            && let Some(pointer) = body.interact_pointer_pos()
        {
            if ctrl {
                let beat = f64::from((pointer.x - at.time.min + scroll_x) / beat_width);
                if let Some(piece) = take.clips.iter().position(|c| (c.start..c.start + c.length).contains(&beat)) {
                    let key = (lane.track, take.number, piece);
                    if let Some(i) = state.take_selected.iter().position(|k| *k == key) {
                        state.take_selected.remove(i);
                    } else {
                        state.take_selected.push(key);
                    }
                    state.take_piece = Some(key);
                }
            } else {
                state.take_selected.clear();
            }
        }
        if !ctrl
            && state.take_drag.is_none()
            && let Some(pointer) = body.hover_pos()
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            let beat = f64::from((pointer.x - at.time.min + scroll_x) / beat_width);
            if let Some(clip) = take.clips.iter().find(|c| (c.start..c.start + c.length).contains(&beat)) {
                let rect = clip_rect(row, clip.start, clip.length, x_of);
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::selection()), StrokeKind::Inside);
            }
        }
        // From where the button went down, not where the drag was recognised.
        if body.drag_started()
            && !ctrl
            && let Some(pointer) = ui.input(|i| i.pointer.press_origin()).or(body.interact_pointer_pos())
        {
            state.take_drag = Some((lane.track, take.number, beat_of(pointer.x)));
        }
        if let Some((_, _, from)) = state.take_drag.filter(|d| d.0 == lane.track && d.1 == take.number)
            && let Some(pointer) = ui.ctx().pointer_latest_pos()
        {
            let to = beat_of(pointer.x);
            let (start, end) = (from.min(to), from.max(to));
            let picked = Rect::from_min_max(Pos2::new(x_of(start), row.top()), Pos2::new(x_of(end), row.bottom()));
            painter.rect_filled(picked, 0.0, theme::selection().gamma_multiply(0.25));
            painter.rect_stroke(picked, 0.0, Stroke::new(1.0, theme::selection()), StrokeKind::Inside);
            if body.drag_stopped() {
                state.take_drag = None;
                if end - start > 1e-6 {
                    let mut comp = params.clone();
                    comp["start"] = json!(start);
                    comp["end"] = json!(end);
                    actions.push(Action::new("take.comp", comp));
                }
            }
        }
        // A double-click takes the whole piece under it; a single click takes nothing.
        if body.double_clicked()
            && !ctrl
            && let Some(pointer) = body.interact_pointer_pos()
        {
            let beat = f64::from((pointer.x - at.time.min + scroll_x) / beat_width);
            if let Some(clip) = take.clips.iter().find(|c| (c.start..c.start + c.length).contains(&beat)) {
                let mut comp = params.clone();
                comp["start"] = json!(clip.start);
                comp["end"] = json!(clip.start + clip.length);
                actions.push(Action::new("take.comp", comp));
            }
        }
    }
    // The passes of a take still recording that the loop or a jump already ended: take lanes
    // at once, numbered after the kept ones, so each pass shows as soon as it is done (#909).
    let kept = timeline.takes.iter().filter(|t| t.track == lane.track).count();
    for (index, pass) in timeline.live_takes.iter().filter(|t| t.track == lane.track && t.finished).enumerate() {
        let row = Rect::from_x_y_ranges(at.time, egui::Rangef::new(y, y + take_row));
        let header = Rect::from_x_y_ranges(at.headers, row.y_range());
        y += take_row + 1.0;
        painter.hline(
            egui::Rangef::new(at.time.min.min(at.headers.min), at.time.max.max(at.headers.max)),
            row.bottom() + 0.5,
            theme::hairline(theme::border()),
        );
        if row.bottom() < at.visible.top() || row.top() > at.visible.bottom() {
            continue;
        }
        // In its colour and named as the take it becomes ("3 Audio Take 2"), as the kept ones are.
        let colour = take_colour(kept + index + 1);
        let swatch = crate::interaction::colour_picker::color32(colour);
        let shade = motion::mix(theme::window_background(), swatch, 0.35);
        painter.rect_filled(header, 0.0, shade);
        painter.rect_filled(Rect::from_min_size(header.min, Vec2::new(4.0, header.height())), 0.0, swatch);
        widgets::text_in(
            painter,
            header.shrink2(Vec2::new(10.0, 0.0)),
            Align2::LEFT_CENTER,
            &format!("{} Take {}", lane.name, kept + index + 1),
            theme::ui_font(theme::TEXT_SMALL),
            ink_on(shade),
        );
        let look = Lane { mute: false, color: colour, ..lane.clone() };
        paint_live_take(painter, row, &look, pass, x_of, beat_width, timeline);
    }
}

/// A take lane's menu (#989): Recolor lane colours the whole take and its parts on the track;
/// Recolor only the piece of it the right-click was over (`piece`, from 0) and its parts; Delete
/// take.
fn take_menu(
    ui: &mut Ui,
    track: TrackId,
    take: &crate::arrangement::timeline::TakeRow,
    piece: Option<usize>,
    params: &serde_json::Value,
    actions: &mut Vec<Action>,
) {
    ui.menu_button("Recolor lane", |ui| {
        if let Some(colour) = crate::interaction::colour_picker::swatch_row(ui, Some(take.color)) {
            let hex = crate::interaction::colour_picker::hex(colour);
            actions.push(Action::new("take.setColor", json!({ "track": track.0, "take": take.number, "color": hex })));
            ui.close();
        }
    });
    ui.add_enabled_ui(piece.is_some(), |ui| {
        ui.menu_button("Recolor", |ui| {
            let current = piece.and_then(|p| take.clips.get(p)).map(|c| c.color);
            if let (Some(colour), Some(piece)) = (crate::interaction::colour_picker::swatch_row(ui, current), piece) {
                let hex = crate::interaction::colour_picker::hex(colour);
                let params = json!({ "track": track.0, "take": take.number, "color": hex, "piece": piece + 1 });
                actions.push(Action::new("take.setColor", params));
                ui.close();
            }
        });
    });
    ui.separator();
    if ui.button("Delete take").clicked() {
        actions.push(Action::new("take.delete", json!({ "track": params["track"], "take": params["take"] })));
        ui.close();
    }
}

/// Draws a take recording into the arrangement on its track's row (#909): from where its pass
/// began to the playhead, its notes or its waveform as they come in, edged in the record colour.
pub(super) fn paint_live_take(
    painter: &egui::Painter,
    row: Rect,
    lane: &Lane,
    take: &suhina_session::LiveTake,
    x_of: &dyn Fn(f64) -> f32,
    beat_width: f32,
    timeline: &Timeline,
) {
    let length = (take.end - take.start).max(0.0);
    let rect = clip_rect(row, take.start, length, x_of);
    let clip = ClipBox {
        id: ClipId::NONE,
        index: 0,
        start: take.start,
        length,
        name: "Recording".into(),
        enabled: true,
        audio: take.audio,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: FadeCurve::default(),
        fade_out_curve: FadeCurve::default(),
        audio_view: None,
        notes: take
            .notes
            .iter()
            .map(|&(start, length, key)| crate::arrangement::timeline::NoteMark { start, length, key })
            .collect(),
        seams: Vec::new(),
        loop_range: None,
        offset: 0.0,
        row: 0,
        rows: 1,
        earliest_start: take.start,
        latest_end: None,
        color: lane.color,
        stack_key: (0, take.start.to_bits()),
        take: None,
        ara: false,
    };
    paint_clip(painter, rect, lane, &clip, Shown::of(&clip), take.start, x_of, beat_width, timeline, false);
    // An audio take's waveform so far: a line per peak, about the middle of the clip.
    if take.audio && !take.peaks.is_empty() && take.peaks_end > take.start {
        let body = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + 14.0), rect.max);
        let step = (take.peaks_end - take.start) / take.peaks.len() as f64;
        let ink = theme::text_primary().gamma_multiply(0.8);
        let mut last = f32::NEG_INFINITY;
        for (i, peak) in take.peaks.iter().enumerate() {
            let x = x_of(take.start + i as f64 * step).round() + 0.5;
            // One line per pixel at most: the loudest is what shows.
            if x <= last || x < painter.clip_rect().left() || x > painter.clip_rect().right() {
                continue;
            }
            last = x;
            let half = body.height() / 2.0 * peak.clamp(0.0, 1.0);
            painter.vline(
                x,
                egui::Rangef::new(body.center().y - half, body.center().y + half.max(0.5)),
                Stroke::new(1.0, ink),
            );
        }
    }
    painter.rect_stroke(rect, 0.0, Stroke::new(1.5, theme::recording()), StrokeKind::Inside);
}
