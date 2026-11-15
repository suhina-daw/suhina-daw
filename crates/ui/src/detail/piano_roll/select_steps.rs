// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stepping through and growing the note selection from the keyboard (`docs/spec/ui-clip-editor.md`
//! › Selecting from the keyboard): with Ctrl held the arrow keys change which notes are selected,
//! and the notes stay where they are. Left and Right go along each selected note's key; Up and Down
//! go through every note of the clip in one order, by start and, among notes that start together,
//! by key from the lowest.

use std::collections::BTreeMap;

use super::*;

/// What a selection key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// Ctrl+Right, Ctrl+Left: each selected key's selection goes to its next (previous) note.
    NextOnKey,
    PreviousOnKey,
    /// Ctrl+Up, Ctrl+Down: one note, the next (previous) in the clip's order.
    Next,
    Previous,
    /// Ctrl+Shift+Right, Ctrl+Shift+Left: each selected key's selection has its moving end
    /// stepped one note later (earlier), as Shift and an arrow do to text: away from the key's
    /// anchor it grows, towards it it shrinks.
    GrowOnKey,
    ShrinkOnKey,
    /// Ctrl+Shift+Up, Ctrl+Shift+Down: the same, in the clip's order.
    Grow,
    Shrink,
}

impl Step {
    /// The step a keymap action asks for, if it is one of these.
    pub(super) fn of(action: &str) -> Option<Self> {
        Some(match action {
            "pianoRoll.selectNextOnKey" => Self::NextOnKey,
            "pianoRoll.selectPreviousOnKey" => Self::PreviousOnKey,
            "pianoRoll.selectNextNote" => Self::Next,
            "pianoRoll.selectPreviousNote" => Self::Previous,
            "pianoRoll.growSelectionOnKey" => Self::GrowOnKey,
            "pianoRoll.shrinkSelectionOnKey" => Self::ShrinkOnKey,
            "pianoRoll.growSelection" => Self::Grow,
            "pianoRoll.shrinkSelection" => Self::Shrink,
            _ => return None,
        })
    }

    /// Whether the step goes on in time: with nothing selected it takes the clip's first note,
    /// else its last.
    fn forward(self) -> bool {
        matches!(self, Self::NextOnKey | Self::Next | Self::GrowOnKey | Self::Grow)
    }
}

/// The notes the rows show, in the order Ctrl+Up steps through: by start, then by key from the
/// lowest, then by index (stacked notes of one key keep the clip's order).
fn clip_order(notes: &[Note], shown: &dyn Fn(u8) -> bool) -> Vec<usize> {
    let mut order: Vec<(usize, &Note)> = notes.iter().enumerate().filter(|(_, n)| shown(n.key)).collect();
    order.sort_by(|(a, x), (b, y)| x.time.total_cmp(&y.time).then(x.key.cmp(&y.key)).then(a.cmp(b)));
    order.into_iter().map(|(i, _)| i).collect()
}

/// Where Shift's steps are anchored, as note indices: the end of each key's selection that stays
/// put, and the one in the clip's order. A selection with no anchor of its own (a click, a
/// rectangle) is anchored at its first note, so Shift+Right grows it and Shift+Left shrinks it
/// back towards that note and then grows it the other way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Anchor {
    on_key: BTreeMap<u8, usize>,
    order: Option<usize>,
}

/// The selection after `step`, from `selected` among `notes`, stepping only to notes whose key
/// `shown` says the rows show. Empty only when the rows show no note.
#[cfg(test)]
pub(super) fn stepped(
    step: Step,
    notes: &[Note],
    selected: &BTreeSet<usize>,
    shown: &dyn Fn(u8) -> bool,
) -> BTreeSet<usize> {
    stepped_from(step, notes, selected, shown, &Anchor::default()).0
}

/// The selection after `step`, as `stepped` gives it, from `anchor` (which a selection Shift grew
/// keeps), with the anchor the next step goes on from: the one given, where it still holds, else
/// the selection's first note.
pub(super) fn stepped_from(
    step: Step,
    notes: &[Note],
    selected: &BTreeSet<usize>,
    shown: &dyn Fn(u8) -> bool,
    anchor: &Anchor,
) -> (BTreeSet<usize>, Anchor) {
    let order = clip_order(notes, shown);
    // The places of the selected notes in that order, first to last.
    let places: Vec<usize> = order.iter().enumerate().filter(|(_, i)| selected.contains(*i)).map(|(p, _)| p).collect();
    let (Some(&first), Some(&last)) = (places.first(), places.last()) else {
        let end = if step.forward() { order.first() } else { order.last() };
        return (end.copied().into_iter().collect(), Anchor::default());
    };
    let at = |place: usize| order.get(place).copied();
    let one = |note: Option<usize>| (note.into_iter().collect(), Anchor::default());
    match step {
        // From a chord the first step takes its lowest note (Up) or its highest (Down); from one
        // note, the next or the previous; at the end the note stays.
        Step::Next if places.len() > 1 => one(at(first)),
        Step::Previous if places.len() > 1 => one(at(last)),
        Step::Next => one(at(last + 1).or_else(|| at(last))),
        Step::Previous => one(last.checked_sub(1).and_then(at).or_else(|| at(last))),
        Step::Grow | Step::Shrink => {
            let mut after: BTreeSet<usize> = places.iter().filter_map(|p| at(*p)).collect();
            let held = anchor.order.and_then(|i| order.iter().position(|o| *o == i));
            let held = held.filter(|p| places.contains(p)).unwrap_or(first);
            match moving_end(step.forward(), first, last, held, order.len()) {
                End::Drop(place) => {
                    after.remove(&order[place]);
                }
                End::Take(place) => {
                    after.insert(order[place]);
                }
                End::Stay => {}
            }
            (after, Anchor { order: at(held), ..Anchor::default() })
        }
        Step::NextOnKey | Step::PreviousOnKey | Step::GrowOnKey | Step::ShrinkOnKey => {
            on_keys(step, notes, &order, selected, anchor)
        }
    }
}

/// What a Shift step does to a run of places `first..=last`: a place let go or taken in.
enum End {
    Drop(usize),
    Take(usize),
    Stay,
}

/// The moving end of a run `first..=last` anchored at `held`, in a line of `len` places, steps
/// later (`forward`) or earlier: towards the anchor it lets a note go, away from it it takes the
/// next one in, and at the line's end it stays.
fn moving_end(forward: bool, first: usize, last: usize, held: usize, len: usize) -> End {
    match forward {
        true if first < held => End::Drop(first),
        true if last + 1 < len => End::Take(last + 1),
        false if last > held => End::Drop(last),
        false if first > 0 => End::Take(first - 1),
        _ => End::Stay,
    }
}

/// The steps along each key: every key with a selected note on it, on its own. A key whose
/// selection cannot go further keeps it, so a selected chord walks along its keys and the lines
/// that end stay put.
fn on_keys(
    step: Step,
    notes: &[Note],
    order: &[usize],
    selected: &BTreeSet<usize>,
    anchor: &Anchor,
) -> (BTreeSet<usize>, Anchor) {
    // Each key's notes in time order (the clip's order keeps it within a key).
    let mut lines: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
    for &index in order {
        if let Some(note) = notes.get(index) {
            lines.entry(note.key).or_default().push(index);
        }
    }
    let mut after = BTreeSet::new();
    let mut anchors = Anchor::default();
    for (key, line) in &lines {
        let places: Vec<usize> =
            line.iter().enumerate().filter(|(_, i)| selected.contains(*i)).map(|(p, _)| p).collect();
        let (Some(&first), Some(&last)) = (places.first(), places.last()) else { continue };
        let kept = places.iter().filter_map(|p| line.get(*p).copied());
        match step {
            Step::NextOnKey if line.get(last + 1).is_some() => {
                after.extend(places.iter().filter_map(|p| line.get(p + 1).copied()));
            }
            Step::PreviousOnKey if places.first().is_some_and(|first| *first > 0) => {
                after.extend(places.iter().filter_map(|p| p.checked_sub(1).and_then(|p| line.get(p).copied())));
            }
            Step::GrowOnKey | Step::ShrinkOnKey => {
                let held = anchor.on_key.get(key).and_then(|i| line.iter().position(|l| l == i));
                let held = held.filter(|p| places.contains(p)).unwrap_or(first);
                anchors.on_key.extend(line.get(held).map(|i| (*key, *i)));
                match moving_end(step.forward(), first, last, held, line.len()) {
                    End::Drop(place) => after.extend(kept.filter(|i| line.get(place) != Some(i))),
                    End::Take(place) => {
                        after.extend(kept);
                        after.extend(line.get(place).copied());
                    }
                    End::Stay => after.extend(kept),
                }
            }
            _ => after.extend(kept),
        }
    }
    (after, anchors)
}

impl PianoRollState {
    /// Runs a selection key on `clip`: the selection changes, the notes stay. What follows a
    /// selection follows it: the view scrolls to keep the selected notes on screen, the time
    /// selection over the ruler is their span (as after a rectangle), and with the audition on the
    /// newly selected notes sound, as a click on them does.
    pub(super) fn step_selection(&mut self, step: Step, clip: &PianoClip) {
        let rows = crate::detail::piano_roll_scale::rows(clip, self.fold_for(clip));
        let before = self.selected.clone();
        // Shift's anchor holds while the selection is the one its last step left.
        let anchor = match &self.step_anchor {
            Some((left, anchor)) if *left == before => anchor.clone(),
            _ => Anchor::default(),
        };
        let (after, anchor) = stepped_from(step, &clip.notes, &before, &|key| rows.shows(key), &anchor);
        if after.is_empty() {
            return;
        }
        self.step_anchor = Some((after.clone(), anchor));
        self.audition_keys(after.difference(&before).filter_map(|i| clip.notes.get(*i)).map(|n| n.key));
        self.selected = after;
        let chosen = || self.selected.iter().filter_map(|i| clip.notes.get(*i));
        let start = chosen().map(|n| n.time).fold(f64::INFINITY, f64::min);
        let end = chosen().map(|n| n.time + n.duration).fold(f64::NEG_INFINITY, f64::max);
        if start.is_finite() && end.is_finite() {
            self.select_span(start, end);
        } else {
            self.clear_span();
        }
        self.reveal_selection = 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(time: f64, key: u8) -> Note {
        Note {
            time,
            duration: 0.5,
            key,
            velocity: 0.8,
            release_velocity: 0.5,
            velocity_range: 0,
            channel: 0,
            muted: false,
        }
    }

    fn set(indices: &[usize]) -> BTreeSet<usize> {
        indices.iter().copied().collect()
    }

    fn every(_: u8) -> bool {
        true
    }

    /// Two lines: key 60 at beats 0, 1, 2 (indices 0, 1, 2) and key 64 at beats 0, 1 (3, 4).
    fn lines() -> Vec<Note> {
        vec![note(0.0, 60), note(1.0, 60), note(2.0, 60), note(0.0, 64), note(1.0, 64)]
    }

    #[test]
    fn a_selected_chord_walks_along_its_keys_and_an_ended_line_stays() {
        let notes = lines();
        let chord = set(&[0, 3]);
        let once = stepped(Step::NextOnKey, &notes, &chord, &every);
        assert_eq!(once, set(&[1, 4]));
        // Key 64 has no further note: its note stays selected, key 60 goes on.
        assert_eq!(stepped(Step::NextOnKey, &notes, &once, &every), set(&[2, 4]));
        assert_eq!(stepped(Step::PreviousOnKey, &notes, &once, &every), set(&[0, 3]));
        assert_eq!(stepped(Step::PreviousOnKey, &notes, &chord, &every), chord);
    }

    #[test]
    fn up_and_down_step_through_every_note_by_time_then_key() {
        // Given out of order: the order is by start, then key from the lowest.
        let notes = vec![note(1.0, 62), note(0.0, 67), note(0.0, 60), note(2.0, 50)];
        let mut at = set(&[2]);
        let mut seen = vec![2];
        for _ in 0..3 {
            at = stepped(Step::Next, &notes, &at, &every);
            seen.extend(at.iter().copied());
        }
        assert_eq!(seen, vec![2, 1, 0, 3]);
        // At the end the note stays; Down goes back the same way.
        assert_eq!(stepped(Step::Next, &notes, &set(&[3]), &every), set(&[3]));
        assert_eq!(stepped(Step::Previous, &notes, &set(&[0]), &every), set(&[1]));
        assert_eq!(stepped(Step::Previous, &notes, &set(&[2]), &every), set(&[2]));
    }

    #[test]
    fn from_a_chord_up_takes_its_lowest_note_and_down_its_highest() {
        let notes = vec![note(0.0, 64), note(0.0, 60), note(0.0, 67), note(1.0, 60)];
        let chord = set(&[0, 1, 2]);
        assert_eq!(stepped(Step::Next, &notes, &chord, &every), set(&[1]));
        assert_eq!(stepped(Step::Previous, &notes, &chord, &every), set(&[2]));
    }

    #[test]
    fn with_nothing_selected_right_and_up_take_the_first_note_left_and_down_the_last() {
        let notes = lines();
        let none = BTreeSet::new();
        for step in [Step::NextOnKey, Step::Next, Step::GrowOnKey, Step::Grow] {
            assert_eq!(stepped(step, &notes, &none, &every), set(&[0]), "{step:?}");
        }
        for step in [Step::PreviousOnKey, Step::Previous, Step::ShrinkOnKey, Step::Shrink] {
            assert_eq!(stepped(step, &notes, &none, &every), set(&[2]), "{step:?}");
        }
        assert!(stepped(Step::Next, &[], &none, &every).is_empty());
    }

    #[test]
    fn shift_grows_and_shrinks_along_each_key() {
        let notes = lines();
        let grown = stepped(Step::GrowOnKey, &notes, &set(&[0, 3]), &every);
        assert_eq!(grown, set(&[0, 1, 3, 4]));
        // Key 64's line has ended: it takes in nothing more.
        assert_eq!(stepped(Step::GrowOnKey, &notes, &grown, &every), set(&[0, 1, 2, 3, 4]));
        assert_eq!(stepped(Step::ShrinkOnKey, &notes, &grown, &every), set(&[0, 3]));
        // A key whose one selected note is its first keeps it: nothing comes before it.
        assert_eq!(stepped(Step::ShrinkOnKey, &notes, &set(&[0, 3]), &every), set(&[0, 3]));
    }

    #[test]
    fn shift_left_from_a_keys_last_note_grows_back_and_right_then_shrinks_it() {
        let notes = lines();
        // Key 60's last note alone: Left grows the selection back along the key.
        let (once, anchor) = stepped_from(Step::ShrinkOnKey, &notes, &set(&[2]), &every, &Anchor::default());
        assert_eq!(once, set(&[1, 2]));
        let (twice, anchor) = stepped_from(Step::ShrinkOnKey, &notes, &once, &every, &anchor);
        assert_eq!(twice, set(&[0, 1, 2]));
        // At the line's start it stays; Right shrinks it back towards the anchor.
        assert_eq!(stepped_from(Step::ShrinkOnKey, &notes, &twice, &every, &anchor).0, twice);
        let (back, anchor) = stepped_from(Step::GrowOnKey, &notes, &twice, &every, &anchor);
        assert_eq!(back, set(&[1, 2]));
        let (home, anchor) = stepped_from(Step::GrowOnKey, &notes, &back, &every, &anchor);
        assert_eq!(home, set(&[2]));
        // At the anchor, with the line ended after it, Right stays.
        assert_eq!(stepped_from(Step::GrowOnKey, &notes, &home, &every, &anchor).0, set(&[2]));
    }

    #[test]
    fn shift_up_and_down_move_the_end_away_from_the_anchor() {
        let notes = lines();
        // The order: 0, 3, 1, 4, 2. From 1 (the third), Down grows back and Up shrinks again.
        let (down, anchor) = stepped_from(Step::Shrink, &notes, &set(&[1]), &every, &Anchor::default());
        assert_eq!(down, set(&[3, 1]));
        let (up, anchor) = stepped_from(Step::Grow, &notes, &down, &every, &anchor);
        assert_eq!(up, set(&[1]));
        assert_eq!(stepped_from(Step::Grow, &notes, &up, &every, &anchor).0, set(&[1, 4]));
    }

    #[test]
    fn shift_grows_and_shrinks_in_the_clip_order() {
        let notes = lines();
        // The order: 0 (0, 60), 3 (0, 64), 1 (1, 60), 4 (1, 64), 2 (2, 60).
        let grown = stepped(Step::Grow, &notes, &set(&[0]), &every);
        assert_eq!(grown, set(&[0, 3]));
        assert_eq!(stepped(Step::Grow, &notes, &grown, &every), set(&[0, 1, 3]));
        assert_eq!(stepped(Step::Shrink, &notes, &grown, &every), set(&[0]));
        assert_eq!(stepped(Step::Shrink, &notes, &set(&[0]), &every), set(&[0]));
    }

    #[test]
    fn only_notes_the_rows_show_are_stepped_to() {
        let notes = vec![note(0.0, 60), note(1.0, 61), note(2.0, 62)];
        let no_black = |key: u8| key != 61;
        assert_eq!(stepped(Step::Next, &notes, &set(&[0]), &no_black), set(&[2]));
    }

    #[test]
    fn every_action_names_its_step() {
        for action in [
            "pianoRoll.selectNextOnKey",
            "pianoRoll.selectPreviousOnKey",
            "pianoRoll.selectNextNote",
            "pianoRoll.selectPreviousNote",
            "pianoRoll.growSelectionOnKey",
            "pianoRoll.shrinkSelectionOnKey",
            "pianoRoll.growSelection",
            "pianoRoll.shrinkSelection",
        ] {
            assert!(Step::of(action).is_some(), "{action}");
        }
        assert!(Step::of("pianoRoll.later").is_none());
    }
}
