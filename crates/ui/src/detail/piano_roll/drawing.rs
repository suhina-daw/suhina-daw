// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Drawing the piano roll: the key column, the notes, the playhead, and what it says to screen readers.

use super::*;

/// The key the drag at height `y` is on: the row there, or the nearest one in view above or below.
fn key_at_height(layout: &Layout, y: f32) -> Option<u8> {
    layout.key_at(y.clamp(layout.grid.top() + 0.5, layout.grid.bottom() - 0.5))
}

/// The key column: a press selects the notes of that key and plays it, and a drag up or down makes
/// that a range of keys, the key under the pointer playing. Shift adds the range to the selection;
/// Ctrl selects without playing anything.
pub(super) fn key_column(
    ui: &mut Ui,
    keys: &Rect,
    layout: &Layout,
    clip: &PianoClip,
    state: &mut PianoRollState,
    inputs: Inputs,
    actions: &mut Vec<Action>,
) {
    if keys.height() <= 1.0 {
        return;
    }
    let response = ui.interact(*keys, Id::unique("piano-roll-keys"), Sense::click_and_drag());
    // The key under the pointer along the keys and the grid: the strip names it and the keys light
    // it. Over the strip itself the pointer is zooming, not reading a row.
    let rows = keys.union(layout.grid);
    let hovered = ui.ctx().pointer_hover_pos().filter(|p| rows.contains(*p)).and_then(|p| layout.key_at(p.y));
    let down = ui.input(|i| i.pointer.primary_down());
    let pressed = response.interact_pointer_pos().and_then(|p| key_at_height(layout, p.y));
    if let Some(key) = pressed
        && down
    {
        state.focused = true;
        let (anchor, silent) = match state.gesture {
            Some(Gesture::KeySelect { anchor, silent }) => (anchor, silent),
            _ => {
                let silent = inputs.command;
                state.gesture = Some(Gesture::KeySelect { anchor: key, silent });
                state.begin_key_selection(key, inputs.shift);
                (key, silent)
            }
        };
        state.select_key_range(clip, &layout.rows, anchor, key);
        if !silent {
            play_key(state, clip, actions, key);
        }
    }
    // Let go: the note goes off and the notes stay selected. A gesture that belongs to another part
    // of the piano roll (the grid's) is left alone.
    if matches!(state.gesture, Some(Gesture::KeySelect { .. })) && !down {
        state.gesture = None;
        if let Some(action) = state.key_release(clip) {
            actions.push(action);
        }
        state.end_key_selection();
    }
    state.hover_key = hovered;
}

/// Plays `key`: the key that was sounding goes off and `key` goes on, so a drag up and down the
/// column sounds one key at a time.
pub(super) fn play_key(state: &mut PianoRollState, clip: &PianoClip, actions: &mut Vec<Action>, key: u8) {
    if state.playing_key() == Some(key) {
        return;
    }
    if let Some(action) = state.key_release(clip) {
        actions.push(action);
    }
    if let Some(action) = state.key_press(clip, key) {
        actions.push(action);
    }
}

/// The play start marker at content beat `beat`: a thin line through the ruler and the notes, as
/// the Arrangement's start marker is (#951).
pub(super) fn draw_start_marker(ui: &Ui, layout: &Layout, beat: f64) {
    let x = layout.x_of(beat).round() + 0.5;
    if !(layout.grid.left()..=layout.grid.right()).contains(&x) {
        return;
    }
    let area = layout.ruler.union(layout.grid);
    let painter = ui.painter_at(area);
    // `start-marker`, as in the Arrangement.
    let ink = crate::kit::element::START_MARKER.look().color_or(theme::text_secondary());
    painter.vline(x, area.y_range(), Stroke::new(1.0, ink.gamma_multiply(0.6)));
    // A small flag on the ruler, so it reads as the start, not the playhead.
    let top = layout.ruler.top();
    painter.add(egui::Shape::convex_polygon(
        vec![Pos2::new(x - 4.0, top), Pos2::new(x + 4.0, top), Pos2::new(x, top + 6.0)],
        ink,
        Stroke::NONE,
    ));
    let response = ui.interact(
        Rect::from_center_size(Pos2::new(x, layout.ruler.center().y), Vec2::new(3.0, layout.ruler.height())),
        Id::unique("piano-roll-start-marker"),
        Sense::hover(),
    );
    let label = format!("Play start at beat {}", round_beat(beat));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &label));
}

pub(super) fn draw_playhead(ui: &Ui, layout: &Layout, beat: f64) {
    let x = layout.x_of(beat).round() + 0.5;
    let area = layout.ruler.union(layout.grid).union(layout.lane);
    if !(layout.grid.left()..=layout.grid.right()).contains(&x) {
        return;
    }
    let painter = ui.painter_at(area);
    let ink = crate::kit::element::PLAYHEAD.look().color_or(theme::accent());
    painter.vline(x, area.y_range(), Stroke::new(1.0, ink));
    let response = ui.interact(
        Rect::from_center_size(Pos2::new(x, area.center().y), Vec2::new(3.0, area.height())),
        Id::unique("piano-roll-playhead"),
        Sense::hover(),
    );
    let label = format!("Playhead at beat {}", round_beat(beat));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &label));
}

/// The note a draw drag from `from` to `now` makes: it starts where the press was, and its end
/// follows the pointer on the grid (`free`: as it is), at least one step long. A drag back to or
/// before the start keeps it one step long, never the last note's length, which a click draws.
pub(super) fn drawn_note(
    state: &PianoRollState,
    layout: &Layout,
    clip: &PianoClip,
    grid: TimelineGrid,
    from: Pos2,
    now: Pos2,
    free: bool,
) -> Option<Note> {
    let mut note = state.new_note(layout, clip, grid, from, free)?;
    let end = grid.place(layout.beat_at(now.x), free, false);
    let least = if grid.snaps && !free { grid.step } else { MIN_LENGTH };
    note.duration = (end - note.time).max(least);
    Some(note)
}

/// What is left of `note` once the notes `covers` (each a key, a start and an end) make room as
/// `note.makeRoom` does: nothing when they cover it from its start to its end, the part before the
/// first one that starts inside it, or the part after those that cover its start (nothing with
/// `whole`, Covered notes' Delete them).
pub(super) fn left_after(note: &Note, covers: &[(u8, f64, f64)], whole: bool) -> Option<Note> {
    let end = note.time + note.duration;
    let mut row: Vec<(f64, f64)> = covers.iter().filter(|c| c.0 == note.key).map(|c| (c.1, c.2)).collect();
    if row.is_empty() {
        return Some(*note);
    }
    row.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut start = note.time;
    for (from, to) in row {
        if to <= start + EPSILON {
            continue;
        }
        if from >= end - EPSILON {
            break;
        }
        if start < from - EPSILON {
            return Some(Note { time: start, duration: from - start, ..*note });
        }
        start = start.max(to);
    }
    let covered_start = start > note.time + EPSILON;
    (start < end - EPSILON && !(whole && covered_start)).then_some(Note { time: start, duration: end - start, ..*note })
}

/// A paint stroke at `now`: the slot of the stroke's note length under the pointer gets a note
/// in the row under it, unless the stroke has one there already.
pub(super) fn paint(state: &mut PianoRollState, layout: &Layout, clip: &PianoClip, now: Pos2) {
    let Some(first) = state.stroke.first().copied() else { return };
    let Some(key) = layout.key_at(now.y) else { return };
    let length = first.duration.max(MIN_LENGTH);
    let slots = ((layout.beat_at(now.x) - first.time) / length).floor();
    let time = first.time + slots * length;
    // A stroke never paints before the content's start or past the furthest beat.
    if !(time.is_finite() && time >= -EPSILON && time <= f64::from(BEAT_LIMIT)) || state.stroke.len() >= 10_000 {
        return;
    }
    let time = time.max(0.0);
    if state.stroke.iter().any(|n| (n.time - time).abs() < EPSILON) {
        return;
    }
    let taken = clip.notes.iter().any(|n| n.key == key && (n.time - time).abs() < EPSILON);
    if !taken {
        state.stroke.push(Note { time, key, ..first });
    }
}

/// A dashed 1 px outline: how a muted note is edged, so it reads as present but not playing.
pub(super) fn dashed_outline(painter: &egui::Painter, rect: Rect, color: Color32) {
    if !(rect.width().is_finite() && rect.height().is_finite()) || rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }
    let (dash, gap) = (3.0, 2.0);
    let stroke = Stroke::new(1.0, color);
    let mut x = rect.left();
    while x < rect.right() {
        let end = (x + dash).min(rect.right());
        painter.line_segment([Pos2::new(x, rect.top() + 0.5), Pos2::new(end, rect.top() + 0.5)], stroke);
        painter.line_segment([Pos2::new(x, rect.bottom() - 0.5), Pos2::new(end, rect.bottom() - 0.5)], stroke);
        x = end + gap;
    }
    let mut y = rect.top();
    while y < rect.bottom() {
        let end = (y + dash).min(rect.bottom());
        painter.line_segment([Pos2::new(rect.left() + 0.5, y), Pos2::new(rect.left() + 0.5, end)], stroke);
        painter.line_segment([Pos2::new(rect.right() - 0.5, y), Pos2::new(rect.right() - 0.5, end)], stroke);
        y = end + gap;
    }
}

/// What the theme's sheet says for one part of the piano roll (`docs/spec/theme-skins.md` ›
/// Piano roll): its background, border, text colour and radius, each only when the sheet sets it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PartStyle {
    pub(crate) background: Option<Color32>,
    pub(crate) border: Option<Color32>,
    pub(crate) color: Option<Color32>,
    pub(crate) radius: Option<f32>,
}

pub(crate) fn part_style(class: &str, states: &[crate::kit::element::Attr]) -> PartStyle {
    let part = crate::kit::element::named(class).map(|name| {
        let builder = name.native(crate::kit::element::Native::default());
        states.iter().fold(builder, |builder, state| builder.attr(*state, true)).enter()
    });
    part.and_then(|part| part.style()).map_or_else(PartStyle::default, |c| PartStyle {
        background: c.authored.background.then_some(c.background),
        border: c.authored.border.then_some(c.border),
        color: c.authored.color.then_some(c.color),
        radius: c.authored.radius.then_some(c.radius),
    })
}

/// Draws the grid, the notes, the velocity lane, the key column and the ruler's bar numbers;
/// `played` is the part of the content the clip plays (› Ruler): the grid and the notes outside
/// it are shaded.
/// `el` is the piano roll, whose background lies over the grid.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ui: &Ui,
    el: &crate::kit::element::Entered,
    layout: &Layout,
    clip: &PianoClip,
    state: &PianoRollState,
    grid: TimelineGrid,
    preview: Option<Effect>,
    played: (f64, f64),
) {
    let painter =
        ui.painter_at(layout.strip.union(layout.keys).union(layout.ruler).union(layout.grid).union(layout.lane));
    let grid_painter = painter.with_clip_rect(layout.grid);
    let first_key = layout.key_at(layout.grid.bottom() - 0.5).unwrap_or(0);
    let last_key = layout.key_at(layout.grid.top() + 0.5).unwrap_or(127);

    use crate::kit::element::Attr;
    // The theme's parts: `piano-grid` (its border-color for the lines), `piano-row` and
    // `piano-row-black`, `piano-key` and `piano-key-black`, `piano-note` (`[selected]`,
    // `[muted]`; the `piano-roll` has the clip's colour as `[color]`), `velocity`.
    let grid_style = part_style("piano-grid", &[]);
    let row_style = part_style("piano-row", &[]);
    let black_row_style = part_style("piano-row-black", &[]);
    let note_style = part_style("piano-note", &[]);
    let selected_style = part_style("piano-note", &[Attr::Selected]);
    let muted_style = part_style("piano-note", &[Attr::Muted]);
    let velocity_style = part_style("velocity", &[]);
    let (muted_stem, selected_stem) =
        (part_style("velocity", &[Attr::Muted]), part_style("velocity", &[Attr::Selected]));
    // The row under the pointer: `piano-row[kind="hover"]`'s background.
    let hover_row = crate::kit::element::PIANO_ROW.attribute("kind", "hover").look();
    let line_colour = |strong: bool| {
        grid_style.border.map_or_else(
            || if strong { theme::border_strong() } else { theme::border() },
            |c| {
                if strong { c } else { c.gamma_multiply(0.6) }
            },
        )
    };
    // Rows: black keys darker, a stronger line under each C.
    painter.rect_filled(
        layout.grid,
        0.0,
        grid_style.background.unwrap_or_else(|| crate::kit::style_class::surface_or(theme::window_background())),
    );
    el.paint_background(&painter, layout.grid, 0.0);
    for key in (first_key..=last_key).filter(|k| layout.rows.shows(*k)) {
        let top = layout.y_of(key);
        let row = Rect::from_min_max(
            Pos2::new(layout.grid.left(), top),
            Pos2::new(layout.grid.right(), top + layout.key_height),
        );
        // With a scale highlighted, its keys are lighter; without one, the white keys are
        // (› Scale and key).
        let own = if is_black(key) { black_row_style.background } else { row_style.background };
        let highlighted = clip.scale.filter(|_| state.highlight_scale);
        let fill = match (highlighted, own) {
            (None, Some(own)) => own,
            _ => scale_view::row_fill(highlighted, key, is_black(key), clip.color),
        };
        grid_painter.rect_filled(row, 0.0, fill);
        // The row under the pointer, so it can be followed to the strip that names it.
        if state.hover_key() == Some(key) {
            grid_painter.rect_filled(row, 0.0, hover_row.background_or(theme::text_primary().gamma_multiply(0.05)));
        }
        grid_painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(line_colour(key % 12 == 0)));
    }
    draw_ranges(&grid_painter, layout, state);

    // Vertical lines: the grid, bars stronger; the ruler's bar numbers.
    let first_beat = layout.beat_at(layout.grid.left()).max(0.0);
    let last_beat = layout.beat_at(layout.grid.right());
    let bar = clip.bar_beats.max(EPSILON);
    painter.rect_filled(
        layout.ruler,
        0.0,
        part_style("ruler", &[])
            .background
            .unwrap_or_else(|| crate::kit::style_class::surface_or(theme::panel_background())),
    );
    let lane_painter = painter.with_clip_rect(layout.lane);
    for (beat, on_bar) in grid.lines(first_beat, last_beat, MAX_LINES) {
        let x = layout.x_of(beat).round() + 0.5;
        let color = line_colour(on_bar);
        grid_painter.vline(x, layout.grid.y_range(), theme::hairline(color));
        lane_painter.vline(x, layout.lane.y_range(), theme::hairline(color));
    }
    let ruler_painter = painter.with_clip_rect(layout.ruler);
    // The ruler's lines and numbers: `ruler-tick` and `ruler-number`, as in the Arrangement.
    let (tick, number) = (crate::kit::element::RULER_TICK.look(), crate::kit::element::RULER_NUMBER.look());
    let (tick, number) = (tick.color_or(theme::border_strong()), number.text_or(theme::text_value()));
    let bar_points = (clip.bar_beats as f32 * layout.beat_width).max(1.0);
    let label_every = (36.0 / bar_points).ceil().max(1.0) as u64;
    let first_bar = (first_beat / bar).floor() as u64;
    let last_bar = (last_beat / bar).ceil().max(0.0) as u64;
    for n in (first_bar..=last_bar).filter(|n| n % label_every == 0).take(MAX_LINES) {
        let x = layout.x_of(n as f64 * bar).round() + 0.5;
        ruler_painter.vline(x, layout.ruler.y_range(), theme::hairline(tick));
        // The bar numbers sit under the ruler's loop row.
        let top = layout.ruler.top() + ruler_view::LOOP_ROW;
        let label = Rect::from_min_max(Pos2::new(x + 3.0, top), Pos2::new(x + 40.0, layout.ruler.bottom()));
        widgets::text_in(
            &ruler_painter,
            label,
            Align2::LEFT_CENTER,
            &(n + 1).to_string(),
            theme::mono_font(theme::TEXT_TINY),
            number,
        );
    }

    // Outside the part the clip plays (before its start marker or its loop, past its loop's end
    // or its end marker): shaded, and the notes there with it.
    let shade = Color32::from_black_alpha(90);
    let (played_start, played_end) = played;
    let end = layout.x_of(played_end);
    if end < layout.grid.right() {
        let past = Rect::from_min_max(Pos2::new(end.max(layout.grid.left()), layout.grid.top()), layout.grid.max);
        grid_painter.rect_filled(past, 0.0, shade);
    }
    let start = layout.x_of(played_start);
    if start > layout.grid.left() {
        let before =
            Rect::from_min_max(layout.grid.min, Pos2::new(start.min(layout.grid.right()), layout.grid.bottom()));
        grid_painter.rect_filled(before, 0.0, shade);
    }
    let outside =
        |note: &Note| note.time + note.duration <= played_start + EPSILON || note.time >= played_end - EPSILON;
    draw_time_span(&grid_painter, layout, state);
    // Ghost notes of other clips, under the clip's own (› Ghost notes).
    ghost_view::paint(&grid_painter, &clip.ghosts, layout.grid, |g| layout.note_rect(&g.note()));

    // Notes, where a drag in progress puts them.
    let shown = |i: usize, note: &Note| -> Note {
        if let Some(Effect::VelocityLine { set, .. }) = &preview {
            return match set.iter().find(|(index, _)| *index == i) {
                Some((_, midi)) => Note { velocity: (midi.clamp(1.0, 127.0) / 127.0) as f32, ..*note },
                None => *note,
            };
        }
        if !state.selected.contains(&i) {
            return *note;
        }
        match preview {
            Some(Effect::Move(beats, keys, rows)) => Note {
                time: note.time + beats,
                key: super::edits::moved_key(&layout.rows, note.key, keys, rows),
                ..*note
            },
            Some(Effect::Resize(by)) => Note { duration: (note.duration + by).max(MIN_LENGTH), ..*note },
            Some(Effect::ResizeStart(by)) => {
                Note { time: note.time + by, duration: (note.duration - by).max(MIN_LENGTH), ..*note }
            }
            Some(Effect::Velocity(by)) => {
                let midi = (f64::from(suhina_session::midi_velocity(note.velocity)) + by).clamp(1.0, 127.0);
                Note { velocity: (midi / 127.0) as f32, ..*note }
            }
            _ => *note,
        }
    };
    let selecting = match preview {
        Some(Effect::Select(rect)) => Some(rect),
        _ => None,
    };
    // The velocity line being drawn, over the lane.
    if let Some(Effect::VelocityLine { from, to, .. }) = &preview {
        let ink = crate::kit::element::VELOCITY.attribute("kind", "line").look().color_or(theme::accent());
        lane_painter.line_segment([*from, *to], Stroke::new(1.0, ink));
    }
    // A copying drag (Ctrl): the selected notes stay where they are, their copies move.
    if matches!(state.gesture, Some(Gesture::Move { copy: true, .. })) && matches!(preview, Some(Effect::Move(..))) {
        for (_, note) in clip.notes.iter().enumerate().filter(|(i, _)| state.selected.contains(i)) {
            let rect = layout.note_rect(note);
            if rect.intersects(layout.grid) {
                let velocity = f32::from(suhina_session::midi_velocity(note.velocity));
                grid_painter.rect_filled(rect, 0.0, clip.color.gamma_multiply(0.4 + 0.6 * velocity / 127.0));
            }
        }
    }
    // With stacked notes off, the notes of a key under selected notes that are to make room for
    // them (moved, resized or nudged and not let go yet, or being dragged) show what is left of them
    // once they do, as `note.makeRoom` leaves them: gone where covered whole, cut where covered in
    // part. Moved off them before being let go, they show whole again (› Resizing).
    let making_room = state.notes_replace
        && (state.settle_pending
            || state.settling.as_ref().is_some_and(|s| s.notes == *state.acting())
            || matches!(preview, Some(Effect::Move(..) | Effect::Resize(_) | Effect::ResizeStart(_))));
    let covers: Vec<(u8, f64, f64)> = if making_room {
        state
            .selected
            .iter()
            .filter_map(|&i| clip.notes.get(i).map(|n| shown(i, n)))
            .map(|n| (n.key, n.time, n.time + n.duration))
            .collect()
    } else {
        Vec::new()
    };
    // The selected notes are drawn last, over the others: a note moved onto another of its key is
    // seen while it is moved.
    let order = (0..clip.notes.len()).filter(|i| !state.selected.contains(i)).chain(state.selected.iter().copied());
    for i in order {
        let Some(note) = clip.notes.get(i) else { continue };
        let note = shown(i, note);
        let note = if covers.is_empty() || state.selected.contains(&i) {
            note
        } else {
            match left_after(&note, &covers, state.covered_whole) {
                Some(left) => left,
                None => continue,
            }
        };
        let rect = layout.note_rect(&note);
        if !rect.intersects(layout.grid) {
            continue;
        }
        let velocity = f32::from(suhina_session::midi_velocity(note.velocity));
        // A note an erase stroke has passed over is nearly gone; a muted note, and one a mute
        // stroke holds, is faded with a dashed edge, so it is clearly there and clearly not
        // playing (› Muting).
        let muted = note.muted || (state.muting.contains(&i) && state.mute_target);
        let unmuting = state.muting.contains(&i) && !state.mute_target;
        let erasing = state.erasing.contains(&i);
        let selected = state.selected.contains(&i) || selecting.is_some_and(|r| r.intersects(rect));
        let alpha = note_alpha(velocity, NoteLook { erasing, muted, selected, outside: outside(&note) });
        let style = if selected {
            selected_style
        } else if muted {
            muted_style
        } else {
            note_style
        };
        let radius = style.radius.unwrap_or(0.0);
        let own =
            if selected { selected_note_color(clip.color, theme::accent(), theme::text_primary()) } else { clip.color };
        grid_painter.rect_filled(rect, radius, style.background.unwrap_or(own).gamma_multiply(alpha));
        if let Some(border) = style.border {
            grid_painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        }
        if muted {
            dashed_outline(&grid_painter, rect, muted_style.border.unwrap_or_else(theme::text_hint));
        }
        if selected && style.border.is_none() {
            grid_painter.rect_stroke(rect, radius, Stroke::new(1.0, theme::text_primary()), StrokeKind::Inside);
        }
        if unmuting {
            // The stroke is bringing this note back: a solid edge says so before the button is up.
            let edge = muted_style.border.unwrap_or_else(theme::text_hint);
            grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, edge), StrokeKind::Inside);
        }
        // Every note says which it is; on a block too small for its name, as much as fits.
        let readable = if alpha >= 0.55 { theme::text_inverse() } else { theme::text_primary() };
        grid_painter.with_clip_rect(rect.intersect(layout.grid)).text(
            Pos2::new(rect.left() + 2.0, rect.center().y),
            Align2::LEFT_CENTER,
            key_name(note.key),
            theme::mono_font(theme::TEXT_TINY),
            style.color.unwrap_or(readable),
        );
        // Its velocity stem.
        let x = rect.left().round() + 0.5;
        let top = layout.lane.bottom() - 2.0 - velocity_height(note.velocity, layout.lane);
        let stem = if muted {
            muted_stem.color.unwrap_or_else(theme::text_hint)
        } else if selected {
            selected_stem.color.unwrap_or_else(theme::text_primary)
        } else {
            velocity_style.color.unwrap_or(clip.color)
        };
        lane_painter.vline(x, egui::Rangef::new(top, layout.lane.bottom()), Stroke::new(1.0, stem));
        lane_painter.rect_filled(Rect::from_min_size(Pos2::new(x - 2.5, top), Vec2::new(5.0, 2.0)), 0.0, stem);
    }
    if let Some(rect) = selecting {
        let edge = crate::kit::element::MARQUEE.look().color_or(theme::text_secondary());
        grid_painter.rect_stroke(rect, 0.0, theme::hairline(edge), StrokeKind::Inside);
    }
    // Notes a draw or paint drag is making, outlined until they are added.
    let drawn = match &preview {
        Some(Effect::Draw(Some(note))) => vec![*note],
        _ => state.stroke.clone(),
    };
    // `piano-note[kind="draft"]`.
    let draft = (!drawn.is_empty()).then(|| crate::kit::element::PIANO_NOTE.attribute("kind", "draft").look());
    for note in &drawn {
        let rect = layout.note_rect(note);
        let style = draft.clone().unwrap_or_default();
        grid_painter.rect_filled(rect, 0.0, style.background_or(clip.color.gamma_multiply(0.7)));
        let edge = style.border_or(theme::text_primary());
        grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, edge), StrokeKind::Inside);
    }
    // The cut line of a slice drag, and a mark where it cuts each note; the right button's slice
    // is drawn in the recording red and fades the piece of each note it deletes.
    if let Some(Effect::Slice(cut)) = &preview {
        // `piano-slice`, `[kind="delete"]` for the right button's: its `color` and, over what it
        // deletes, its `background-color`.
        let look = crate::kit::element::PIANO_SLICE.attribute("kind", if cut.delete { "delete" } else { "cut" }).look();
        let ink = look.color_or(if cut.delete { theme::recording() } else { theme::accent() });
        grid_painter.line_segment(cut.line, Stroke::new(1.5, ink));
        for &(index, at) in &cut.cuts {
            let Some(note) = clip.notes.get(index) else { continue };
            let rect = layout.note_rect(note);
            if cut.delete {
                let (start, end) = edits::SliceCut::removed(note, at);
                let gone = Rect::from_x_y_ranges(layout.x_of(start)..=layout.x_of(end), rect.y_range());
                let fill = look.background_or(theme::window_background().gamma_multiply(0.75));
                grid_painter.rect_filled(gone.intersect(rect), 0.0, fill);
            }
            grid_painter.vline(layout.x_of(at).round() + 0.5, rect.y_range().expand(1.0), Stroke::new(2.0, ink));
        }
    }
    let band = crate::kit::element::SEPARATOR.attribute("kind", "band").look().color_or(theme::border_strong());
    painter.hline(layout.lane.x_range(), layout.lane.top() + 0.5, theme::hairline(band));

    draw_keys(&painter, layout, clip, state);
    let corner = Rect::from_min_max(
        Pos2::new(layout.strip.left(), layout.ruler.top()),
        Pos2::new(layout.keys.right(), layout.ruler.bottom()),
    );
    painter.rect_filled(corner, 0.0, crate::kit::style_class::surface_or(theme::panel_background()));
    let lane_keys = Rect::from_min_max(
        Pos2::new(layout.strip.left(), layout.lane.top()),
        Pos2::new(layout.keys.right(), layout.lane.bottom()),
    );
    painter.rect_filled(lane_keys, 0.0, crate::kit::style_class::surface_or(theme::panel_background()));
    if lane_keys.height() >= 24.0 {
        let font = theme::mono_font(theme::TEXT_TINY);
        let hint = crate::kit::element::LABEL.look().text_or(theme::text_hint());
        widgets::text_in(
            &painter,
            Rect::from_min_size(lane_keys.min, Vec2::new(lane_keys.width() - 3.0, 10.0)),
            Align2::RIGHT_CENTER,
            "127",
            font.clone(),
            hint,
        );
        widgets::text_in(
            &painter,
            Rect::from_min_max(
                Pos2::new(lane_keys.left(), lane_keys.bottom() - 10.0),
                Pos2::new(lane_keys.right() - 3.0, lane_keys.bottom()),
            ),
            Align2::RIGHT_CENTER,
            "1",
            font,
            hint,
        );
    }
    painter.vline(layout.keys.right() - 0.5, layout.keys.y_range(), theme::hairline(band));
}

/// Names the piano roll's parts for screen readers and tests: the grid, each note in view and
/// its velocity stem.
pub(super) fn for_readers(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState) {
    let grid = ui.interact(layout.grid, Id::unique("piano-roll-readers"), Sense::hover());
    let count = clip.notes.len();
    grid.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, format!("Piano roll, {count} notes")));
    ghost_view::for_readers(ui, &clip.ghosts, layout.grid, |g| layout.note_rect(&g.note()));
    for (i, note) in clip.notes.iter().enumerate() {
        let rect = layout.note_rect(note).intersect(layout.grid);
        if !rect.is_positive() {
            continue;
        }
        let selected = state.selected.contains(&i);
        // A muted note says so, so a reader and a test tell it from the others.
        let label = if note.muted {
            format!("Note {} at beat {} (muted)", key_name(note.key), round_beat(note.time))
        } else {
            format!("Note {} at beat {}", key_name(note.key), round_beat(note.time))
        };
        let response = ui.interact(rect, Id::unique(("piano-roll-note", i)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Unknown, true, selected, &label));
        let x = layout.x_of(note.time);
        let stem = Rect::from_min_max(Pos2::new(x - 2.0, layout.lane.top()), Pos2::new(x + 2.0, layout.lane.bottom()))
            .intersect(layout.lane);
        if stem.is_positive() {
            let velocity = suhina_session::midi_velocity(note.velocity);
            let label =
                format!("Velocity of note {} at beat {}: {velocity}", key_name(note.key), round_beat(note.time));
            let response = ui.interact(stem, Id::unique(("piano-roll-stem", i)), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &label));
        }
    }
}

/// What a note's look depends on besides its velocity.
#[derive(Clone, Copy)]
struct NoteLook {
    /// An erase stroke has passed over it.
    erasing: bool,
    muted: bool,
    selected: bool,
    /// Outside the clip's window.
    outside: bool,
}

/// How opaque a note is drawn: by its velocity (1 to 127), nearly gone under an erase stroke,
/// faded while muted, fainter outside the clip's window. Selected, it stays bright enough to tell
/// at any velocity, unless it is muted or an erase stroke has passed over it.
fn note_alpha(velocity: f32, look: NoteLook) -> f32 {
    let alpha = if look.erasing {
        0.12
    } else if look.muted {
        0.25
    } else {
        0.4 + 0.6 * velocity / 127.0
    };
    let alpha = if look.outside { alpha * 0.45 } else { alpha };
    if look.selected && !look.muted && !look.erasing { alpha.max(0.75) } else { alpha }
}

#[cfg(test)]
mod note_alpha_tests {
    use super::*;

    #[test]
    fn an_erase_stroke_dims_a_selected_note_too() {
        let look = NoteLook { erasing: false, muted: false, selected: true, outside: false };
        assert!(note_alpha(10.0, look) >= 0.75, "selected stays bright");
        let erased = note_alpha(127.0, NoteLook { erasing: true, ..look });
        assert!((erased - 0.12).abs() < 1e-6, "erased, selected or not: {erased}");
        let plain = NoteLook { selected: false, ..look };
        assert!((note_alpha(127.0, NoteLook { erasing: true, ..plain }) - erased).abs() < 1e-6);
    }
}
