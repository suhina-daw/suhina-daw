// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's gestures from [#351](https://github.com/suhina-daw/suhina-daw/issues/351)
//! (`docs/spec/ui-clip-editor.md` › Resizing, › Moving, › Velocity, › Layout, › Playing the keys):
//! which part of a note the pointer is on (its left edge moves its start), Alt+drag on a note for
//! its velocity, a line drawn in the velocity lane, the lane's top edge and the audition. A child
//! module of [`super`], so it shares the piano roll's private state and layout.

use crate::kit::surface::Backgrounds as _;
use std::time::Duration;

use egui::{Align2, CursorIcon, Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::Note;

use super::{
    EDGE, EPSILON, Effect, LEAST_KEY_ROWS, Layout, MAX_NOTES_PER_COMMAND, MIN_LENGTH, PianoClip, PianoRollState,
    velocity_at,
};
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::theme;
use crate::kit::widgets::{self, Action};

/// How many velocity steps one point of an Alt+drag on a note moves.
const VELOCITY_PER_POINT: f64 = 1.0;
/// How long an auditioned note sounds before the window lets it go, in seconds.
pub(super) const AUDITION_SECONDS: f64 = 0.3;
/// The most keys one audition sounds together.
const MAX_AUDITION_KEYS: usize = 16;
/// How far either side of the velocity lane's top edge the pointer grabs it, in points.
const LANE_EDGE: f32 = 3.0;
/// The heights the lane's top edge can be dragged to, in points; the five key rows the grid
/// keeps above the lane can make it shorter still (› Layout).
pub(super) const LANE_RANGE: (f32, f32) = (24.0, 480.0);

/// Which part of a note the pointer is on: the first or the last few points resize it, the middle
/// moves it. On a small note each end is at most a third of its width, so the middle still moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Part {
    Start,
    Body,
    End,
}

impl Part {
    /// The part of the note drawn in `rect` at `x`.
    pub(super) fn of(rect: Rect, x: f32) -> Self {
        let zone = EDGE.min(rect.width() / 3.0);
        if x >= rect.right() - zone {
            Self::End
        } else if x <= rect.left() + zone {
            Self::Start
        } else {
            Self::Body
        }
    }
}

/// How far a drag of the grabbed note's left edge by `raw` beats moves the selection's starts,
/// their ends kept: the start on the grid unless `free` (onto or past its end, the last line
/// before it), no start before beat 0 and no note shorter than a hundredth of a beat.
pub fn start_by(raw: f64, grabbed: &Note, moved: &[Note], grid: TimelineGrid, free: bool) -> f64 {
    if !raw.is_finite() {
        return 0.0;
    }
    let end = grabbed.time + grabbed.duration;
    let mut start = grid.place(grabbed.time + raw, free, false);
    if grid.snaps && !free && start >= end - EPSILON {
        start = grid.previous_line(end);
    }
    let shortest = moved.iter().map(|n| n.duration).fold(f64::INFINITY, f64::min);
    let earliest = moved.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
    let mut by = start - grabbed.time;
    if shortest.is_finite() {
        by = by.min(shortest - MIN_LENGTH);
    }
    if earliest.is_finite() {
        by = by.max(-earliest);
    }
    if by.is_finite() { by } else { 0.0 }
}

/// How much an Alt+drag on a note from `from` to `now` changes the selection's velocities: a step
/// a point, up louder, the grabbed note kept within 1 to 127.
pub(super) fn velocity_by(from: Pos2, now: Pos2, grabbed: Option<&Note>) -> f64 {
    let raw = (f64::from(from.y - now.y) * VELOCITY_PER_POINT).round();
    let raw = if raw.is_finite() { raw } else { 0.0 };
    match grabbed {
        Some(note) => {
            let midi = f64::from(suhina_session::midi_velocity(note.velocity));
            raw.clamp(1.0 - midi, 127.0 - midi)
        }
        None => raw.clamp(-126.0, 126.0),
    }
}

/// A line in the velocity lane from `from` to `now`: every note whose start is between the two
/// ends in time takes the line's height at its start.
pub(super) fn velocity_line(layout: &Layout, notes: &[Note], from: Pos2, now: Pos2) -> Effect {
    let (a, b) = if from.x <= now.x { (from, now) } else { (now, from) };
    let (va, vb) = (velocity_at(layout.lane, a.y), velocity_at(layout.lane, b.y));
    let width = b.x - a.x;
    let set = notes
        .iter()
        .enumerate()
        .filter_map(|(i, note)| {
            let x = layout.x_of(note.time);
            if !(x >= a.x - 0.5 && x <= b.x + 0.5) {
                return None;
            }
            let t = if width > 0.5 { f64::from(((x - a.x) / width).clamp(0.0, 1.0)) } else { 1.0 };
            Some((i, (va + (vb - va) * t).round().clamp(1.0, 127.0)))
        })
        .take(MAX_NOTES_PER_COMMAND)
        .collect();
    Effect::VelocityLine { from, to: now, set }
}

/// The velocity lane's top edge: dragging it makes the lane taller or shorter, within
/// [`LANE_RANGE`] and never so tall that fewer than five key rows are left in `room` (the height
/// under the ruler). Window state, kept until Suhina closes.
pub(super) fn lane_edge(ui: &Ui, layout: &Layout, room: f32, state: &mut PianoRollState) {
    let lane = layout.lane;
    if lane.height() <= 4.0 {
        return;
    }
    let rect = Rect::from_min_max(
        Pos2::new(lane.left(), lane.top() - LANE_EDGE),
        Pos2::new(lane.right(), lane.top() + LANE_EDGE),
    );
    let response = ui.interact(rect, Id::new("piano-roll-lane-edge"), Sense::drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Velocity lane top edge"));
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if response.dragged() {
        let most = (room - LEAST_KEY_ROWS * layout.key_height).max(LANE_RANGE.0);
        let current = state.lane_wanted.unwrap_or(lane.height());
        let wanted = (current - response.drag_delta().y).clamp(LANE_RANGE.0, LANE_RANGE.1).min(most);
        if wanted.is_finite() {
            state.lane_wanted = Some(wanted);
        }
    }
}

/// The velocity an Alt+drag gives the grabbed note, shown over it while the drag lasts.
pub(super) fn draw_velocity_label(ui: &Ui, layout: &Layout, clip: &PianoClip, grabbed: usize, by: f64) {
    let Some(note) = clip.notes.get(grabbed) else { return };
    let midi = (f64::from(suhina_session::midi_velocity(note.velocity)) + by).clamp(1.0, 127.0);
    let rect = layout.note_rect(note);
    let label = Rect::from_min_size(Pos2::new(rect.left(), rect.top() - 15.0), Vec2::new(76.0, 14.0));
    let painter = ui.painter_at(layout.grid);
    painter.fill_background(label, 2.0, theme::control_background());
    let text = format!("Velocity {midi}");
    widgets::text_in(
        &painter,
        label.shrink2(Vec2::new(4.0, 0.0)),
        Align2::LEFT_CENTER,
        &text,
        theme::mono_font(theme::TEXT_TINY),
        theme::text_primary(),
    );
    let response = ui.interact(label.intersect(layout.grid), Id::new("piano-roll-velocity-value"), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &text));
}

impl PianoRollState {
    /// The keys an audition sounds at the end of this frame, when the toolbar's Audition is on:
    /// those of notes just drawn, moved or transposed. Each key once, at most a handful.
    pub(super) fn audition_keys(&mut self, keys: impl IntoIterator<Item = u8>) {
        if !self.audition {
            return;
        }
        for key in keys {
            if self.audition_pending.len() >= MAX_AUDITION_KEYS {
                break;
            }
            if key <= 127 && !self.audition_pending.contains(&key) {
                self.audition_pending.push(key);
            }
        }
    }

    /// The notes `moved` sound where a move by `semitones` puts them.
    pub(super) fn audition_moved(&mut self, moved: &[Note], semitones: i32) {
        let keys: Vec<u8> = moved
            .iter()
            .filter_map(|n| u8::try_from(i32::from(n.key) + semitones).ok().filter(|k| *k <= 127))
            .collect();
        self.audition_keys(keys);
    }

    /// The audition's timing, once a frame: the keys sounding go off once their time is up (or
    /// when new ones start), and the keys this frame asked for go on, `preview.noteOn` through
    /// `track`. The window counts the time; nothing waits on the audio thread.
    pub(super) fn audition_frame(&mut self, ui: &Ui, track: Option<u64>, actions: &mut Vec<Action>) {
        let now = ui.input(|i| i.time);
        if let Some((sounding, until)) = self.audition_off
            && (now >= until || !self.audition_pending.is_empty())
        {
            self.audition_off = None;
            self.audition_sounding.clear();
            // A key held in the key column has replaced the audition: letting go of every preview
            // note would cut it short.
            if self.playing_key.is_none() {
                actions.push(Action::new("preview.noteOff", json!({ "track": sounding })));
            }
        }
        let keys = std::mem::take(&mut self.audition_pending);
        if let Some(track) = track
            && !keys.is_empty()
            && self.audition
        {
            actions.push(Action::new(
                "preview.noteOn",
                json!({ "track": track, "keys": keys, "velocity": self.last_velocity }),
            ));
            self.audition_off = Some((track, now + AUDITION_SECONDS));
            self.audition_sounding = keys;
        }
        if let Some((_, until)) = self.audition_off {
            ui.ctx().request_repaint_after(Duration::from_secs_f64((until - now).clamp(0.0, AUDITION_SECONDS)));
        }
    }

    /// Lets go of the notes the audition sounds, for a piano roll that is no longer shown:
    /// `preview.noteOff`, or `None` when nothing sounds.
    pub fn stop_audition(&mut self) -> Option<Action> {
        self.audition_pending.clear();
        self.audition_sounding.clear();
        let (track, _) = self.audition_off.take()?;
        Some(Action::new("preview.noteOff", json!({ "track": track })))
    }

    /// The keys the audition sounds now, for tests and readers.
    pub fn auditioning(&self) -> &[u8] {
        &self.audition_sounding
    }

    /// The velocity lane's height as its top edge was dragged; `None` for the default height.
    pub fn lane_height(&self) -> Option<f32> {
        self.lane_wanted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(time: f64, duration: f64, key: u8) -> Note {
        Note { time, duration, key, velocity: 100.0 / 127.0, release_velocity: 0.5, channel: 0, muted: false }
    }

    #[test]
    fn the_ends_of_a_note_resize_and_a_small_note_keeps_a_middle() {
        let wide = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(60.0, 10.0));
        assert_eq!(Part::of(wide, 2.0), Part::Start);
        assert_eq!(Part::of(wide, 30.0), Part::Body);
        assert_eq!(Part::of(wide, 58.0), Part::End);
        // Nine points wide: each end is three points, the middle three move it.
        let small = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(9.0, 10.0));
        assert_eq!(Part::of(small, 2.0), Part::Start);
        assert_eq!(Part::of(small, 4.5), Part::Body);
        assert_eq!(Part::of(small, 7.0), Part::End);
    }

    #[test]
    fn the_left_edge_snaps_and_stops_before_the_end_and_at_beat_0() {
        let grid = TimelineGrid::new(0.25, 40.0, 4.0, 6.0);
        let grabbed = note(1.0, 1.0, 60);
        assert_eq!(start_by(0.3, &grabbed, &[grabbed], grid, false), 0.25);
        assert!((start_by(0.3, &grabbed, &[grabbed], grid, true) - 0.3).abs() < 1e-12);
        // Dragged onto or past its end: the last grid line before it.
        assert_eq!(start_by(1.2, &grabbed, &[grabbed], grid, false), 0.75);
        // Free, it stops a hundredth of a beat before the end.
        assert!((start_by(5.0, &grabbed, &[grabbed], grid, true) - (1.0 - MIN_LENGTH)).abs() < 1e-12);
        // Never before beat 0, for the whole selection.
        let early = note(0.5, 1.0, 62);
        assert_eq!(start_by(-3.0, &grabbed, &[grabbed, early], grid, false), -0.5);
        // A shorter selected note keeps a hundredth of a beat.
        let short = note(4.0, 0.25, 64);
        assert!((start_by(0.75, &grabbed, &[grabbed, short], grid, false) - (0.25 - MIN_LENGTH)).abs() < 1e-12);
        assert_eq!(start_by(f64::NAN, &grabbed, &[grabbed], grid, true), 0.0);
    }

    #[test]
    fn an_alt_drag_moves_velocity_a_step_a_point_within_1_to_127() {
        let n = note(0.0, 1.0, 60);
        assert_eq!(velocity_by(Pos2::new(0.0, 50.0), Pos2::new(0.0, 40.0), Some(&n)), 10.0);
        assert_eq!(velocity_by(Pos2::new(0.0, 50.0), Pos2::new(0.0, 60.0), Some(&n)), -10.0);
        assert_eq!(velocity_by(Pos2::new(0.0, 500.0), Pos2::new(0.0, 0.0), Some(&n)), 27.0);
        assert_eq!(velocity_by(Pos2::new(0.0, 0.0), Pos2::new(0.0, 500.0), Some(&n)), -99.0);
    }

    #[test]
    fn the_audition_sounds_only_while_it_is_on_and_each_key_once() {
        let mut state = PianoRollState::default();
        state.audition_keys([60]);
        assert!(state.audition_pending.is_empty(), "off by default: nothing sounds");
        state.audition = true;
        state.audition_moved(&[note(0.0, 1.0, 60), note(0.0, 1.0, 64), note(1.0, 1.0, 60)], 2);
        assert_eq!(state.audition_pending, [62, 66]);
        state.audition_moved(&[note(0.0, 1.0, 127)], 1);
        assert_eq!(state.audition_pending, [62, 66], "past key 127 nothing");
        state.audition_keys(0..100);
        assert_eq!(state.audition_pending.len(), MAX_AUDITION_KEYS);
        // Nothing sounding: nothing to let go of.
        assert!(state.stop_audition().is_none());
        assert!(state.audition_pending.is_empty());
    }

    /// One frame of the audition at `time` seconds: the actions it sends.
    fn frame(ctx: &egui::Context, time: f64, state: &mut PianoRollState) -> Vec<(String, serde_json::Value)> {
        let mut actions = Vec::new();
        let input = egui::RawInput { time: Some(time), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| state.audition_frame(ui, Some(3), &mut actions));
        // No renderer here takes the font texture.
        output.textures_delta.clear();
        actions.into_iter().map(|a| (a.id.to_string(), a.params)).collect()
    }

    #[test]
    fn an_auditioned_note_goes_on_and_off_a_moment_later() {
        let ctx = egui::Context::default();
        let mut state = PianoRollState { audition: true, ..PianoRollState::default() };
        state.set_preview_velocity(90);
        state.audition_keys([64, 60]);
        let on = frame(&ctx, 1.0, &mut state);
        assert_eq!(on, [("preview.noteOn".to_string(), json!({ "track": 3, "keys": [64, 60], "velocity": 90 }))]);
        assert_eq!(state.auditioning(), [64, 60]);
        assert!(frame(&ctx, 1.1, &mut state).is_empty(), "still sounding");
        let off = frame(&ctx, 1.0 + AUDITION_SECONDS, &mut state);
        assert_eq!(off, [("preview.noteOff".to_string(), json!({ "track": 3 }))]);
        assert!(state.auditioning().is_empty());
        assert!(frame(&ctx, 2.0, &mut state).is_empty(), "once");
        // A new audition before the last one ends lets the last one go first.
        state.audition_keys([62]);
        frame(&ctx, 3.0, &mut state);
        state.audition_keys([65]);
        let both = frame(&ctx, 3.1, &mut state);
        assert_eq!(both.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["preview.noteOff", "preview.noteOn"]);
        // A piano roll no longer shown lets its note go at once.
        assert_eq!(state.stop_audition().map(|a| a.id), Some("preview.noteOff"));
        assert!(frame(&ctx, 9.0, &mut state).is_empty());
    }
}
