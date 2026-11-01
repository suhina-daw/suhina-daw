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
    let response = ui.interact(*keys, Id::new("piano-roll-keys"), Sense::click_and_drag());
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
                let silent = inputs.command && !inputs.shift;
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
        state.end_key_selection(clip);
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
    painter.vline(x, area.y_range(), Stroke::new(1.0, theme::text_secondary().gamma_multiply(0.6)));
    // A small flag on the ruler, so it reads as the start, not the playhead.
    let top = layout.ruler.top();
    painter.add(egui::Shape::convex_polygon(
        vec![Pos2::new(x - 4.0, top), Pos2::new(x + 4.0, top), Pos2::new(x, top + 6.0)],
        theme::text_secondary(),
        Stroke::NONE,
    ));
    let response = ui.interact(
        Rect::from_center_size(Pos2::new(x, layout.ruler.center().y), Vec2::new(3.0, layout.ruler.height())),
        Id::new("piano-roll-start-marker"),
        Sense::hover(),
    );
    let label = format!("Play start at beat {}", round_beat(beat));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
}

pub(super) fn draw_playhead(ui: &Ui, layout: &Layout, beat: f64) {
    let x = layout.x_of(beat).round() + 0.5;
    let area = layout.ruler.union(layout.grid).union(layout.lane);
    if !(layout.grid.left()..=layout.grid.right()).contains(&x) {
        return;
    }
    let painter = ui.painter_at(area);
    painter.vline(x, area.y_range(), Stroke::new(1.0, theme::accent()));
    let response = ui.interact(
        Rect::from_center_size(Pos2::new(x, area.center().y), Vec2::new(3.0, area.height())),
        Id::new("piano-roll-playhead"),
        Sense::hover(),
    );
    let label = format!("Playhead at beat {}", round_beat(beat));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
}

/// The note a draw drag from `from` to `now` makes: it starts where the press was, and its end
/// follows the pointer on the grid (`free`: as it is), at least one step long.
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
    let least = if grid.snaps && !free { grid.step.min(note.duration) } else { MIN_LENGTH };
    if end - note.time >= least {
        note.duration = end - note.time;
    }
    Some(note)
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

pub(crate) fn part_style(class: &str, states: &[crate::kit::native_css::HostState]) -> PartStyle {
    let _style = crate::kit::style_class::enter(class);
    for state in states {
        crate::kit::native_css::mark(*state, true);
    }
    crate::kit::style_class::computed().map_or_else(PartStyle::default, |c| PartStyle {
        background: c.authored.background.then_some(c.background),
        border: c.authored.border.then_some(c.border),
        color: c.authored.color.then_some(c.color),
        radius: c.authored.radius.then_some(c.radius),
    })
}

/// Draws the grid, the notes, the velocity lane, the key column and the ruler's bar numbers;
/// `played` is the part of the content the clip plays (› Ruler): the grid and the notes outside
/// it are shaded.
pub(super) fn draw(
    ui: &Ui,
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

    use crate::kit::native_css::HostState;
    // The theme's parts: `.piano-grid` (its border-color for the lines), `.piano-row` and
    // `.piano-row-black`, `.piano-key` and `.piano-key-black`, `.piano-note` (`:selected`,
    // `:muted`; `--clip-colour` is the clip's colour), `.velocity`.
    let grid_style = part_style("piano-grid", &[]);
    let row_style = part_style("piano-row", &[]);
    let black_row_style = part_style("piano-row-black", &[]);
    crate::kit::native_css::set_variable("--clip-colour", crate::kit::native_css::hex(clip.color));
    let note_style = part_style("piano-note", &[]);
    let selected_style = part_style("piano-note", &[HostState::Selected]);
    let muted_style = part_style("piano-note", &[HostState::Muted]);
    let velocity_style = part_style("velocity", &[]);
    let line_colour = |strong: bool| {
        grid_style.border.map_or_else(
            || if strong { theme::border_strong() } else { theme::border() },
            |c| {
                if strong { c } else { c.gamma_multiply(0.6) }
            },
        )
    };
    // Rows: black keys darker, a stronger line under each C.
    painter.fill_background(layout.grid, 0.0, grid_style.background.unwrap_or_else(theme::window_background));
    crate::kit::native_css::background(&painter, layout.grid);
    for key in (first_key..=last_key).filter(|k| layout.rows.shows(*k)) {
        let top = layout.y_of(key);
        let row = Rect::from_min_max(
            Pos2::new(layout.grid.left(), top),
            Pos2::new(layout.grid.right(), top + layout.key_height),
        );
        // With a scale, its keys are lighter; without one, the white keys are (› Scale and key).
        let own = if is_black(key) { black_row_style.background } else { row_style.background };
        let fill = match (clip.scale, own) {
            (None, Some(own)) => own,
            _ => scale_view::row_fill(clip.scale, key, is_black(key)),
        };
        grid_painter.fill_background(row, 0.0, fill);
        // The row under the pointer, so it can be followed to the strip that names it.
        if state.hover_key() == Some(key) {
            grid_painter.rect_filled(row, 0.0, theme::text_primary().gamma_multiply(0.05));
        }
        grid_painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(line_colour(key % 12 == 0)));
    }
    draw_ranges(&grid_painter, layout, state);

    // Vertical lines: the grid, bars stronger; the ruler's bar numbers.
    let first_beat = layout.beat_at(layout.grid.left()).max(0.0);
    let last_beat = layout.beat_at(layout.grid.right());
    let bar = clip.bar_beats.max(EPSILON);
    painter.fill_background(
        layout.ruler,
        0.0,
        part_style("ruler", &[]).background.unwrap_or_else(theme::panel_background),
    );
    let lane_painter = painter.with_clip_rect(layout.lane);
    for (beat, on_bar) in grid.lines(first_beat, last_beat, MAX_LINES) {
        let x = layout.x_of(beat).round() + 0.5;
        let color = line_colour(on_bar);
        grid_painter.vline(x, layout.grid.y_range(), theme::hairline(color));
        lane_painter.vline(x, layout.lane.y_range(), theme::hairline(color));
    }
    let ruler_painter = painter.with_clip_rect(layout.ruler);
    let bar_points = (clip.bar_beats as f32 * layout.beat_width).max(1.0);
    let label_every = (36.0 / bar_points).ceil().max(1.0) as u64;
    let first_bar = (first_beat / bar).floor() as u64;
    let last_bar = (last_beat / bar).ceil().max(0.0) as u64;
    for n in (first_bar..=last_bar).filter(|n| n % label_every == 0).take(MAX_LINES) {
        let x = layout.x_of(n as f64 * bar).round() + 0.5;
        ruler_painter.vline(x, layout.ruler.y_range(), theme::hairline(theme::border_strong()));
        // The bar numbers sit under the ruler's loop row.
        let top = layout.ruler.top() + ruler_view::LOOP_ROW;
        let label = Rect::from_min_max(Pos2::new(x + 3.0, top), Pos2::new(x + 40.0, layout.ruler.bottom()));
        widgets::text_in(
            &ruler_painter,
            label,
            Align2::LEFT_CENTER,
            &(n + 1).to_string(),
            theme::mono_font(theme::TEXT_TINY),
            theme::text_value(),
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
            Some(Effect::Move(beats, keys)) => Note {
                time: note.time + beats,
                key: u8::try_from(i32::from(note.key) + keys).unwrap_or(note.key).min(127),
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
        lane_painter.line_segment([*from, *to], Stroke::new(1.0, theme::accent()));
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
    for (i, note) in clip.notes.iter().enumerate() {
        let note = shown(i, note);
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
        let alpha = if state.erasing.contains(&i) {
            0.12
        } else if muted {
            0.25
        } else {
            0.4 + 0.6 * velocity / 127.0
        };
        let alpha = if outside(&note) { alpha * 0.45 } else { alpha };
        let selected = state.selected.contains(&i) || selecting.is_some_and(|r| r.intersects(rect));
        let style = if selected {
            selected_style
        } else if muted {
            muted_style
        } else {
            note_style
        };
        let radius = style.radius.unwrap_or(0.0);
        grid_painter.rect_filled(rect, radius, style.background.unwrap_or(clip.color).gamma_multiply(alpha));
        if let Some(border) = style.border {
            grid_painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        }
        if muted {
            dashed_outline(&grid_painter, rect, theme::text_hint());
        }
        if selected && style.border.is_none() {
            grid_painter.rect_stroke(rect, radius, Stroke::new(1.0, theme::text_primary()), StrokeKind::Inside);
        }
        if unmuting {
            // The stroke is bringing this note back: a solid edge says so before the button is up.
            grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::text_hint()), StrokeKind::Inside);
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
            theme::text_hint()
        } else if selected {
            theme::text_primary()
        } else {
            velocity_style.color.unwrap_or(clip.color)
        };
        lane_painter.vline(x, egui::Rangef::new(top, layout.lane.bottom()), Stroke::new(1.0, stem));
        lane_painter.rect_filled(Rect::from_min_size(Pos2::new(x - 2.5, top), Vec2::new(5.0, 2.0)), 0.0, stem);
    }
    if let Some(rect) = selecting {
        grid_painter.rect_stroke(rect, 0.0, theme::hairline(theme::text_secondary()), StrokeKind::Inside);
    }
    // Notes a draw or paint drag is making, outlined until they are added.
    let drawn = match &preview {
        Some(Effect::Draw(Some(note))) => vec![*note],
        _ => state.stroke.clone(),
    };
    for note in &drawn {
        let rect = layout.note_rect(note);
        grid_painter.rect_filled(rect, 0.0, clip.color.gamma_multiply(0.7));
        grid_painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::text_primary()), StrokeKind::Inside);
    }
    // The cut line of a slice drag, and a mark on each note it would split.
    if let Some(Effect::Slice(at, cut)) = &preview {
        let x = layout.x_of(*at).round() + 0.5;
        grid_painter.vline(x, layout.grid.y_range(), Stroke::new(1.0, theme::accent()));
        for index in cut {
            if let Some(note) = clip.notes.get(*index) {
                let row = layout.note_rect(note);
                grid_painter.hline(row.x_range(), row.top() - 0.5, Stroke::new(1.0, theme::accent()));
            }
        }
    }
    painter.hline(layout.lane.x_range(), layout.lane.top() + 0.5, theme::hairline(theme::border_strong()));

    draw_keys(&painter, layout, clip, state);
    let corner = Rect::from_min_max(
        Pos2::new(layout.strip.left(), layout.ruler.top()),
        Pos2::new(layout.keys.right(), layout.ruler.bottom()),
    );
    painter.fill_background(corner, 0.0, theme::panel_background());
    let lane_keys = Rect::from_min_max(
        Pos2::new(layout.strip.left(), layout.lane.top()),
        Pos2::new(layout.keys.right(), layout.lane.bottom()),
    );
    painter.fill_background(lane_keys, 0.0, theme::panel_background());
    if lane_keys.height() >= 24.0 {
        let font = theme::mono_font(theme::TEXT_TINY);
        widgets::text_in(
            &painter,
            Rect::from_min_size(lane_keys.min, Vec2::new(lane_keys.width() - 3.0, 10.0)),
            Align2::RIGHT_CENTER,
            "127",
            font.clone(),
            theme::text_hint(),
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
            theme::text_hint(),
        );
    }
    painter.vline(layout.keys.right() - 0.5, layout.keys.y_range(), theme::hairline(theme::border_strong()));
}

/// Names the piano roll's parts for screen readers and tests: the grid, each note in view and
/// its velocity stem.
pub(super) fn for_readers(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState) {
    let grid = ui.interact(layout.grid, Id::new("piano-roll-readers"), Sense::hover());
    let count = clip.notes.len();
    grid.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Piano roll, {count} notes")));
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
        let response = ui.interact(rect, Id::new(("piano-roll-note", i)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Other, true, selected, &label));
        let x = layout.x_of(note.time);
        let stem = Rect::from_min_max(Pos2::new(x - 2.0, layout.lane.top()), Pos2::new(x + 2.0, layout.lane.bottom()))
            .intersect(layout.lane);
        if stem.is_positive() {
            let velocity = suhina_session::midi_velocity(note.velocity);
            let label =
                format!("Velocity of note {} at beat {}: {velocity}", key_name(note.key), round_beat(note.time));
            let response = ui.interact(stem, Id::new(("piano-roll-stem", i)), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
        }
    }
}
