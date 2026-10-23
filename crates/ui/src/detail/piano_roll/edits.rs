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
            if !state.selected.remove(&index) {
                state.selected.insert(index);
            }
        }
        (Some(index), _) => {
            state.selected = BTreeSet::from([index]);
            if let Some(note) = clip.notes.get(index) {
                state.remember(note);
            }
        }
        // Draw and Paint add a note; so does Select when a click on empty grid draws (the
        // Mouse-first setting, › Workflow modes).
        (None, Tool::Draw | Tool::Paint) => click_adds(gesture, state, actions, pos, inputs),
        (None, Tool::Select) if workflow.empty_click_draws => click_adds(gesture, state, actions, pos, inputs),
        (None, Tool::Slice) => {}
        (None, Tool::Select) => state.selected.clear(),
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

/// What a gesture does with the pointer at `now`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Effect {
    Move(f64, i32),
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
    /// A slice: the beat the cut is at, and the notes it crosses.
    Slice(f64, Vec<usize>),
    /// A paint or erase stroke: what it does is in the state.
    Stroke,
}

pub(super) fn effect(
    gesture: &Gesture,
    now: Pos2,
    layout: &Layout,
    clip: &PianoClip,
    chosen: &[Note],
    grid: TimelineGrid,
    free: bool,
) -> Effect {
    match gesture {
        Gesture::Move { from, grabbed, .. } => {
            let raw = f64::from((now.x - from.x) / layout.beat_width);
            let rows = (-(now.y - from.y) / layout.key_height).round() as i32;
            // Folded, the grabbed note steps through the shown rows; the rest move as far.
            let keys = clip.notes.get(*grabbed).map_or(rows, |n| layout.rows.semitones(n.key, rows));
            let (beats, keys) = move_delta(raw, keys, chosen, grid, free);
            Effect::Move(beats, keys)
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
        Gesture::Slice { .. } => match slice_at(clip, layout, grid, now.x, free) {
            Some((at, cut)) => Effect::Slice(at, cut),
            None => Effect::Stroke,
        },
        Gesture::Draw { .. }
        | Gesture::DrawRun
        | Gesture::Paint
        | Gesture::Erase
        | Gesture::Mute
        | Gesture::KeySelect { .. } => Effect::Stroke,
    }
}

/// The beat a cut at `x` lands on, and the notes it crosses; `None` when it misses every note.
/// The line's own x is used, so the cut is where the pointer is, not where the drag started.
pub(super) fn slice_at(
    clip: &PianoClip,
    layout: &Layout,
    grid: TimelineGrid,
    x: f32,
    free: bool,
) -> Option<(f64, Vec<usize>)> {
    let at = if free { layout.beat_at(x).max(0.0) } else { nearest_line(grid, layout.beat_at(x)) };
    if !at.is_finite() {
        return None;
    }
    let crossed: Vec<usize> = clip
        .notes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.time < at - EPSILON && n.time + n.duration > at + EPSILON)
        .map(|(i, _)| i)
        .collect();
    if crossed.is_empty() { None } else { Some((at, crossed)) }
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
        (Gesture::Move { copy, .. }, Effect::Move(beats, keys)) if !notes.is_empty() && (beats != 0.0 || keys != 0) => {
            let moved: Vec<Note> = notes.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
            state.audition_moved(&moved, keys);
            params["beats"] = json!(round_beat(beats));
            params["semitones"] = json!(keys);
            if *copy {
                // Ctrl+drag: one `note.duplicate` puts the copies where the drag let go; the
                // copies are appended, so they are the next indices, and they are selected.
                actions.push(Action::new("note.duplicate", params));
                state.selected = (clip.notes.len()..clip.notes.len() + notes.len()).collect();
            } else {
                // Notes moved onto others of the same key and start replace them (Settings ›
                // Piano roll › Moved notes replace the notes under them, on by default); the
                // selection stays on the moved notes as the indices close up.
                let replace = state.notes_replace;
                if replace {
                    params["replace"] = json!(true);
                    let landed: Vec<Note> = notes
                        .iter()
                        .filter_map(|i| clip.notes.get(*i))
                        .map(|n| Note {
                            time: n.time + round_beat(beats),
                            key: u8::try_from(i32::from(n.key) + keys).unwrap_or(n.key),
                            ..*n
                        })
                        .collect();
                    let covered: Vec<usize> = (0..clip.notes.len())
                        .filter(|i| !notes.contains(i))
                        .filter(|i| {
                            landed
                                .iter()
                                .any(|m| m.key == clip.notes[*i].key && (m.time - clip.notes[*i].time).abs() < 1e-9)
                        })
                        .collect();
                    if !covered.is_empty() {
                        state.selected =
                            state.selected.iter().map(|i| i - covered.iter().filter(|c| *c < i).count()).collect();
                    }
                }
                actions.push(Action::new("note.move", params));
            }
        }
        (Gesture::Resize { .. }, Effect::Resize(by)) if !notes.is_empty() && by != 0.0 => {
            params["by"] = json!(round_beat(by));
            actions.push(Action::new("note.resize", params));
        }
        (Gesture::ResizeStart { .. }, Effect::ResizeStart(by)) if !notes.is_empty() && by != 0.0 => {
            params["startBy"] = json!(round_beat(by));
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
            actions.push(Action::new("note.add", add_params(clip, &[note])));
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
                actions.push(Action::new("note.add", add_params(clip, &stroke)));
                state.selected = (clip.notes.len()..clip.notes.len() + stroke.len()).collect();
                state.audition_keys(stroke.iter().map(|n| n.key));
                if let Some(last) = stroke.last() {
                    state.remember(last);
                }
            }
        }
        // Slice: one `note.split` for the whole stroke, at the beat the line ends at.
        (Gesture::Slice { .. }, Effect::Slice(at, cut)) if !cut.is_empty() => {
            let mut params = clip.address.clone();
            params["notes"] = json!(cut);
            params["at"] = json!(round_beat(at));
            // `note.split` appends the second halves: they are the notes after the ones there.
            state.selected = (clip.notes.len()..clip.notes.len() + cut.len()).collect();
            actions.push(Action::new("note.split", params));
        }
        (Gesture::Paint, _) => {
            // A painted note on top of one already there (same key, same start) is left out.
            let stroke: Vec<Note> = std::mem::take(&mut state.stroke)
                .into_iter()
                .filter(|p| !clip.notes.iter().any(|n| n.key == p.key && (n.time - p.time).abs() < EPSILON))
                .collect();
            if !stroke.is_empty() {
                actions.push(Action::new("note.add", add_params(clip, &stroke)));
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
            if !*add {
                state.selected.clear();
            }
            for (i, note) in clip.notes.iter().enumerate() {
                if layout.note_rect(note).intersects(rect) {
                    state.selected.insert(i);
                }
            }
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
