// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a click or a drag does to the notes: the effect worked out, and the commands it sends.

use super::*;

/// A single click in the grid, on note `over` or an empty spot at `pos`: what the tool does with
/// it (› Drawing notes, › Selecting, › Muting).
pub(super) fn single_click(
    gesture: &Gestures<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    over: Option<usize>,
    pos: Pos2,
    inputs: Inputs,
    workflow: Workflow,
) {
    let clip = gesture.clip;
    match (over, state.tool) {
        (Some(index), Tool::Erase) => gesture.delete_note(state, actions, index),
        // A click with Mute mutes the note, or unmutes it when it is already muted: one
        // `note.setMuted`, one undo step (› Muting).
        (Some(index), Tool::Mute) => {
            if let Some(note) = clip.notes.get(index) {
                let mut params = clip.address.clone();
                params["notes"] = json!([index]);
                params["muted"] = json!(!note.muted);
                actions.push(Action::new("note.setMuted", params));
            }
        }
        (None, Tool::Erase | Tool::Mute) => {}
        (Some(index), _) if inputs.shift || inputs.command => {
            // A note the press already added stays; any other is taken out or added.
            if state.press_added.take() != Some(index) && !state.selected.remove(&index) {
                state.selected.insert(index);
                state.audition_keys(clip.notes.get(index).map(|n| n.key));
            }
        }
        (Some(index), _) => {
            state.selected = BTreeSet::from([index]);
            if let Some(note) = clip.notes.get(index) {
                state.remember(note);
                state.audition_keys([note.key]);
            }
        }
        // Draw and Paint add a note; so does Select when a click on empty grid draws (the
        // Mouse-first setting, › Workflow modes).
        (None, Tool::Draw | Tool::Paint) => click_adds(gesture, state, actions, pos, inputs),
        (None, Tool::Select) if workflow.empty_click_draws => click_adds(gesture, state, actions, pos, inputs),
        (None, Tool::Slice) => {}
        // A click on an empty spot leaves the mark a paste goes to.
        (None, Tool::Select) => {
            state.selected.clear();
            state.clear_span();
            state.insert_at = Some(gesture.grid.place(gesture.layout.beat_at(pos.x), inputs.alt, true));
        }
    }
}

/// A click that adds a note at `pos`, remembering which index it will have, so the second click
/// of a double-click selects it instead of deleting it.
pub(super) fn click_adds(
    gesture: &Gestures<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    pos: Pos2,
    inputs: Inputs,
) {
    let before = actions.len();
    gesture.add_note(state, actions, pos, inputs.alt);
    if actions.len() > before {
        state.click_added = Some(gesture.clip.notes.len());
    }
}

/// The second click of a double-click, on note `over` (with `added` the note the first click
/// added, if it did) or an empty spot at `pos` (› Double-click in `docs/spec/ui-clip-editor.md`).
pub(super) fn second_click(
    gesture: &Gestures<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    (over, added): (Option<usize>, Option<usize>),
    pos: Pos2,
    inputs: Inputs,
    workflow: Workflow,
) {
    match (state.tool, over) {
        // The first click already erased or muted the note: the second adds nothing, so a
        // double-click is one note and one undo step.
        (Tool::Erase | Tool::Mute, _) => {}
        // The first click added a note: the second selects it and never deletes it.
        (_, Some(index)) if added.is_some() => state.selected = BTreeSet::from([index]),
        (_, None) if added.is_some() => {}
        (Tool::Select, Some(index)) if workflow.double_click_deletes => gesture.delete_note(state, actions, index),
        (Tool::Select, None) if workflow.double_click_adds => click_adds(gesture, state, actions, pos, inputs),
        _ => single_click(gesture, state, actions, over, pos, inputs, workflow),
    }
}

/// How far a drag of `raw` beats moves a grabbed note that starts at `start`, the grid snapping
/// (› Moving): a note on the grid goes by whole grid steps (`stepped`, as [`move_delta`] gives
/// them). A note off the grid (played in, or humanised) moves freely until it reaches the first
/// grid line, and from there snaps to whichever is nearer: a grid line, or its own offset from
/// the grid a whole number of steps away. So it can be put back on the grid by dragging, or moved
/// keeping its feel. Never before the start for any of `moved`.
pub(super) fn magnetic(start: f64, raw: f64, moved: &[Note], grid: TimelineGrid) -> f64 {
    let beats = magnetic_by(start, raw, grid, false);
    let earliest = moved.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
    if earliest.is_finite() { beats.max(-earliest) } else { beats }
}

/// The key a note on `key` goes to with a move of `keys` semitones, or, with the rows folded,
/// `rows` shown rows (each note through the rows from its own, stopping at the top and bottom).
pub(super) fn moved_key(shown: &KeyRows, key: u8, keys: i32, rows: Option<i32>) -> u8 {
    let step = match rows {
        Some(rows) => shown.semitones(key, rows),
        None => keys,
    };
    u8::try_from(i32::from(key) + step).ok().filter(|k| *k <= 127).unwrap_or(key)
}

/// What a gesture does with the pointer at `now`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Effect {
    /// Beats and semitones the selection moves by; with the rows folded, also how many shown rows,
    /// which each note steps through on its own (› [`moved_key`]).
    Move(f64, i32, Option<i32>),
    Resize(f64),
    /// Beats the selection's starts move by, their ends kept.
    ResizeStart(f64),
    Select(Rect),
    Velocity(f64),
    /// A line in the velocity lane from `from` to `to`, and the velocity each stem it spans takes.
    VelocityLine {
        from: Pos2,
        to: Pos2,
        set: Vec<(usize, f64)>,
    },
    /// The note a draw drag makes.
    Draw(Option<Note>),
    /// A slice: its cut line and the cuts it makes.
    Slice(SliceCut),
    /// A paint or erase stroke: what it does is in the state.
    Stroke,
}

/// A slice drag's cut line and what it cuts (› Drawing notes, Slice).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SliceCut {
    /// The line as drawn: from where the press was to the pointer.
    pub(super) line: [Pos2; 2],
    /// Each note the line cuts and the beat it is cut at.
    pub(super) cuts: Vec<(usize, f64)>,
    /// The right button's slice: the shorter piece of each note cut goes.
    pub(super) delete: bool,
}

impl SliceCut {
    /// The beats of the piece of `note`, cut at `at`, that a deleting slice removes: the shorter
    /// one, the later when they are as long (`note.split` with `keepLonger` keeps the first then).
    pub(super) fn removed(note: &Note, at: f64) -> (f64, f64) {
        let end = note.time + note.duration;
        if at - note.time >= end - at { (at, end) } else { (note.time, at) }
    }
}

pub(super) fn effect(
    gesture: &Gesture,
    now: Pos2,
    layout: &Layout,
    clip: &PianoClip,
    chosen: &[Note],
    grid: TimelineGrid,
    inputs: Inputs,
) -> Effect {
    let free = inputs.alt;
    match gesture {
        Gesture::Move { from, grabbed, .. } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            let rows = (-(now.y - from.y) / layout.key_height).round() as i32;
            // Folded, the grabbed note steps through the shown rows; the rest move as far.
            let keys = clip.notes.get(*grabbed).map_or(rows, |n| layout.rows.semitones(n.key, rows));
            let (beats, keys) = move_delta(raw, keys, chosen, grid, free);
            let beats = match clip.notes.get(*grabbed) {
                Some(g) if !free && grid.snaps => magnetic(g.time, raw, chosen, grid),
                _ => beats,
            };
            Effect::Move(beats, keys, layout.rows.folded().then_some(rows))
        }
        Gesture::Resize { from, grabbed } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            clip.notes
                .get(*grabbed)
                .map_or(Effect::Resize(0.0), |g| Effect::Resize(resize_by(raw, g, chosen, grid, free)))
        }
        Gesture::ResizeStart { from, grabbed } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            clip.notes
                .get(*grabbed)
                .map_or(Effect::ResizeStart(0.0), |g| Effect::ResizeStart(start_by(raw, g, chosen, grid, free)))
        }
        Gesture::NoteVelocity { from, grabbed } => {
            Effect::Velocity(gestures::velocity_by(*from, now, clip.notes.get(*grabbed)))
        }
        Gesture::VelocityLine { from } => gestures::velocity_line(layout, &clip.notes, *from, now),
        Gesture::Select { from, .. } => Effect::Select(Rect::from_two_pos(*from, now)),
        Gesture::Velocity { grabbed, .. } => {
            // The grabbed bar follows the pointer's height; the other selected notes change by
            // the same amount (each kept within 1 to 127 by the command).
            let target = velocity_at(layout.lane, now.y);
            let current =
                clip.notes.get(*grabbed).map_or(target, |n| f64::from(suhina_session::midi_velocity(n.velocity)));
            Effect::Velocity(target - current)
        }
        Gesture::Slice { from, delete, by_shift } => {
            // Shift slants the line, unless Shift is what started the slice (Shift right-drag).
            let slanted = inputs.shift && !by_shift;
            Effect::Slice(SliceCut { delete: *delete, ..slice_cut(clip, layout, grid, (*from, now), slanted, free) })
        }
        Gesture::Draw { .. }
        | Gesture::DrawRun
        | Gesture::Paint
        | Gesture::Erase
        | Gesture::Mute
        | Gesture::KeySelect { .. } => Effect::Stroke,
    }
}

/// A slice's cut line from `from` to the pointer at `now`, and the cuts it makes. The line stands
/// straight up or down on the beat the pointer is at, or, `slanted` (Shift), runs from `from` to
/// the pointer. Each note whose row the line reaches is cut where the line is at the row's middle
/// (at the line's nearer end when it stops short of the middle), on the grid unless `free`; a cut
/// on a note's start or end, or outside it, leaves the note alone. A row the fold hides is never
/// reached.
pub(super) fn slice_cut(
    clip: &PianoClip,
    layout: &Layout,
    grid: TimelineGrid,
    (from, now): (Pos2, Pos2),
    slanted: bool,
    free: bool,
) -> SliceCut {
    let snap = |x: f32| {
        let beat = layout.beat_at(x).max(0.0);
        if free { beat } else { nearest_line(grid, beat) }
    };
    let straight = snap(now.x);
    let line = if slanted {
        [from, now]
    } else {
        let x = layout.x_of(straight);
        [Pos2::new(x, from.y), Pos2::new(x, now.y)]
    };
    let [a, b] = line;
    let (top, bottom) = (a.y.min(b.y), a.y.max(b.y));
    let x_at = |y: f32| if (b.y - a.y).abs() < 0.5 { b.x } else { a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y) };
    let cuts = clip
        .notes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| {
            let row_top = layout.y_of(n.key);
            let row_bottom = row_top + layout.key_height;
            if row_bottom < top || row_top > bottom {
                return None;
            }
            let at = if slanted { snap(x_at(((row_top + row_bottom) / 2.0).clamp(top, bottom))) } else { straight };
            (at.is_finite() && n.time < at - EPSILON && n.time + n.duration > at + EPSILON).then_some((i, at))
        })
        .collect();
    SliceCut { line, cuts, delete: false }
}

pub(super) fn commit(
    gesture: &Gesture,
    effect: Effect,
    clip: &PianoClip,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    layout: &Layout,
) {
    let notes: Vec<usize> = state.selected.iter().copied().collect();
    let mut params = clip.address.clone();
    params["notes"] = json!(notes);
    match (gesture, effect) {
        (Gesture::Move { copy, .. }, Effect::Move(beats, keys, rows))
            if !notes.is_empty() && (beats != 0.0 || keys != 0 || rows.is_some_and(|r| r != 0)) =>
        {
            let moved: Vec<Note> = notes.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
            let landing: Vec<u8> = moved.iter().map(|n| moved_key(&layout.rows, n.key, keys, rows)).collect();
            state.audition_keys(landing.iter().copied());
            if !*copy {
                state.span_moved(beats);
            }
            params["beats"] = json!(round_beat(beats));
            // Folded, each note (or its copy) steps through the shown rows on its own, so none
            // lands on a key the fold hides.
            if rows.is_some() {
                params["keys"] = json!(landing);
            } else {
                params["semitones"] = json!(keys);
            }
            if *copy {
                // Ctrl+drag: one `note.duplicate` puts the copies where the drag let go; the
                // copies are appended, so they are the next indices, and they are selected.
                state.replacing(&mut params);
                actions.push(Action::new("note.duplicate", params));
                state.select_copies((clip.notes.len()..clip.notes.len() + notes.len()).collect(), &clip.notes);
                // They went where the drag put them, not a stretch on.
                state.span = None;
            } else {
                // With stacked notes off, moved notes replace what they land on; the window moves
                // the selection as the indices close up.
                state.replacing(&mut params);
                actions.push(Action::new("note.move", params));
            }
        }
        (Gesture::Resize { .. }, Effect::Resize(by)) if !notes.is_empty() && by != 0.0 => {
            params["by"] = json!(round_beat(by));
            state.replacing(&mut params);
            actions.push(Action::new("note.resize", params));
        }
        (Gesture::ResizeStart { .. }, Effect::ResizeStart(by)) if !notes.is_empty() && by != 0.0 => {
            params["startBy"] = json!(round_beat(by));
            state.replacing(&mut params);
            actions.push(Action::new("note.resize", params));
        }
        (Gesture::Velocity { .. } | Gesture::NoteVelocity { .. }, Effect::Velocity(by))
            if !notes.is_empty() && by != 0.0 =>
        {
            params["by"] = json!(by);
            actions.push(Action::new("note.setVelocity", params));
        }
        // The velocity line: one `note.setVelocity` with a value for each stem it spans.
        (Gesture::VelocityLine { .. }, Effect::VelocityLine { set, .. }) if !set.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = set.iter().map(|(i, _)| json!(i)).collect();
            params["velocities"] = set.iter().map(|(_, v)| json!(v)).collect();
            actions.push(Action::new("note.setVelocity", params));
        }
        (Gesture::Draw { .. }, Effect::Draw(Some(note))) => {
            actions.push(state.add_action(clip, &[note]));
            state.selected = BTreeSet::from([clip.notes.len()]);
            state.remember(&note);
            state.audition_keys([note.key]);
        }
        // Shift and Draw: one `note.add` for the whole stroke, the notes it drew.
        (Gesture::DrawRun, _) => {
            let stroke: Vec<Note> = std::mem::take(&mut state.stroke)
                .into_iter()
                .filter(|p| !clip.notes.iter().any(|n| n.key == p.key && (n.time - p.time).abs() < EPSILON))
                .collect();
            if !stroke.is_empty() {
                actions.push(state.add_action(clip, &stroke));
                state.selected = (clip.notes.len()..clip.notes.len() + stroke.len()).collect();
                state.audition_keys(stroke.iter().map(|n| n.key));
                if let Some(last) = stroke.last() {
                    state.remember(last);
                }
            }
        }
        // Slice: one `note.split` for the whole stroke, each note at the beat the line cuts it.
        (Gesture::Slice { .. }, Effect::Slice(cut)) if !cut.cuts.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = json!(cut.cuts.iter().map(|(index, _)| *index).collect::<Vec<_>>());
            let times: Vec<f64> = cut.cuts.iter().map(|(_, at)| round_beat(*at)).collect();
            match times.split_first() {
                Some((first, rest)) if rest.iter().all(|at| at == first) => params["at"] = json!(first),
                _ => params["times"] = json!(times),
            }
            if cut.delete {
                // The right button's slice keeps the longer piece of each note, in its place.
                params["keepLonger"] = json!(true);
                state.selected.clear();
            } else {
                // `note.split` appends the second halves: they are the notes after the ones there.
                state.selected = (clip.notes.len()..clip.notes.len() + cut.cuts.len()).collect();
            }
            actions.push(Action::new("note.split", params));
        }
        (Gesture::Paint, _) => {
            // A painted note on top of one already there (same key, same start) is left out, and so,
            // where notes do not stack, is one that would start inside it: a stroke paints the gaps.
            let inside = state.notes_replace;
            let stroke: Vec<Note> = std::mem::take(&mut state.stroke)
                .into_iter()
                .filter(|p| {
                    !clip.notes.iter().any(|n| {
                        n.key == p.key
                            && ((n.time - p.time).abs() < EPSILON
                                || (inside && n.time < p.time && p.time < n.time + n.duration - EPSILON))
                    })
                })
                .collect();
            if !stroke.is_empty() {
                actions.push(state.add_action(clip, &stroke));
                state.selected = (clip.notes.len()..clip.notes.len() + stroke.len()).collect();
                state.audition_keys(stroke.iter().map(|n| n.key));
            }
        }
        (Gesture::Erase, _) if !state.erasing.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = json!(std::mem::take(&mut state.erasing).into_iter().collect::<Vec<_>>());
            state.selected.clear();
            actions.push(Action::new("note.delete", params));
        }
        // Mute: one `note.setMuted` for the whole stroke, every note it passed over set to the
        // state the first note it touched takes (› Muting).
        (Gesture::Mute, _) if !state.muting.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = json!(std::mem::take(&mut state.muting).into_iter().collect::<Vec<_>>());
            params["muted"] = json!(state.mute_target);
            actions.push(Action::new("note.setMuted", params));
        }
        (Gesture::Select { add, .. }, Effect::Select(rect)) => {
            let kept = if *add { state.span_shown(clip) } else { None };
            if !*add {
                state.selected.clear();
            }
            for (i, note) in clip.notes.iter().enumerate() {
                if layout.note_rect(note).intersects(rect) {
                    state.selected.insert(i);
                }
            }
            // What it covered is selected in time, the empty parts too, and one that adds keeps what
            // was selected in time: the stretch runs over both.
            let (start, end) = swept_span(note_grid(clip, layout.beat_width), layout, &clip.notes, rect);
            let (start, end) = kept.map_or((start, end), |k| (k.0.min(start), k.1.max(end)));
            state.select_span(start, end);
        }
        _ => {}
    }
}

/// The tool `step` places on from `tool`, stopping at the first and the last tool: what the
/// right button and the wheel choose.
pub(super) fn step_tool(tool: Tool, step: i32) -> Tool {
    let last = Tool::ALL.len().saturating_sub(1) as i32;
    let index = Tool::ALL.iter().position(|(t, _)| *t == tool).unwrap_or(0) as i32;
    let next = (index + step).clamp(0, last) as usize;
    Tool::ALL.get(next).map_or(tool, |(tool, _)| *tool)
}
