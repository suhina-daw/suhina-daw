// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Follow actions: what a launched clip does after it has played for its follow time. Behavior:
//! `docs/spec/clip-launching.md` › Follow actions.
//!
//! The launcher asks [`FollowPlan::due`] every frame while a clip plays; when it is due, the
//! launcher picks an action with [`FollowPlan::pick`] and turns it into a launch or a stop with
//! [`resolve`]. Nothing here allocates, so it runs on the audio thread.

use suhina_model::{Clip, FollowKind};

/// The shortest follow time, in beats, so a chain of clips cannot launch on every frame.
const MIN_BEATS: f64 = 1.0 / 64.0;
/// The seed the random numbers start from until [`Random::seed`] is called: offline renders
/// pick the same actions every time.
pub(crate) const DEFAULT_SEED: u64 = 0x5EED_F011_0AC7_1000;

/// A slot clip's follow action, ready to run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FollowPlan {
    /// Beats after the launch.
    beats: f64,
    a: FollowKind,
    b: FollowKind,
    chance_a: u32,
    chance_b: u32,
}

impl FollowPlan {
    /// The follow action of `clip`, if it has one turned on. A clip that plays once runs it at
    /// its end at the latest.
    pub(crate) fn new(clip: &Clip) -> Option<Self> {
        let f = clip.follow_action;
        if !f.enabled || (f.chance_a == 0 && f.chance_b == 0) {
            return None;
        }
        let mut beats = if f.linked { clip.length } else { f.time };
        if clip.loop_range.is_none() {
            beats = beats.min(clip.length);
        }
        if !beats.is_finite() {
            return None;
        }
        Some(Self {
            beats: beats.max(MIN_BEATS),
            a: f.action_a,
            b: f.action_b,
            chance_a: f.chance_a,
            chance_b: f.chance_b,
        })
    }

    /// Whether the action is due `elapsed` beats after the launch.
    pub(crate) fn due(&self, elapsed: f64) -> bool {
        elapsed >= self.beats - 1e-9
    }

    /// Action A or B, by chance with their weights.
    pub(crate) fn pick(&self, random: &mut Random) -> FollowKind {
        let total = u64::from(self.chance_a) + u64::from(self.chance_b);
        if total == 0 {
            return FollowKind::Nothing;
        }
        if random.below(total) < u64::from(self.chance_a) { self.a } else { self.b }
    }
}

/// What a follow action does to the column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The clip keeps playing.
    Keep,
    Stop,
    Launch(usize),
}

/// The outcome of `kind` for the clip in slot `current` of a column of `slots` slots, where
/// `playable(slot)` says whether a slot holds a clip that can play.
pub(crate) fn resolve(
    kind: FollowKind,
    current: usize,
    slots: usize,
    playable: impl Fn(usize) -> bool,
    random: &mut Random,
) -> Outcome {
    let launch = |slot: Option<usize>| slot.map_or(Outcome::Keep, Outcome::Launch);
    match kind {
        FollowKind::Nothing => Outcome::Keep,
        FollowKind::Stop => Outcome::Stop,
        FollowKind::Again => Outcome::Launch(current),
        FollowKind::Next => launch((1..=slots).map(|step| (current + step) % slots).find(|s| playable(*s))),
        FollowKind::Previous => {
            launch((1..=slots).map(|step| (current + slots * 2 - step) % slots).find(|s| playable(*s)))
        }
        FollowKind::First => launch((0..slots).find(|s| playable(*s))),
        FollowKind::Last => launch((0..slots).rev().find(|s| playable(*s))),
        FollowKind::Any => launch(nth_playable(slots, &playable, None, random)),
        FollowKind::Other => match nth_playable(slots, &playable, Some(current), random) {
            Some(slot) => Outcome::Launch(slot),
            None => Outcome::Launch(current),
        },
        FollowKind::Jump(slot) => match usize::try_from(slot) {
            Ok(slot) if slot < slots && playable(slot) => Outcome::Launch(slot),
            _ => Outcome::Keep,
        },
    }
}

/// A playable slot picked at random, leaving out `except`.
fn nth_playable(
    slots: usize,
    playable: &impl Fn(usize) -> bool,
    except: Option<usize>,
    random: &mut Random,
) -> Option<usize> {
    let candidate = |s: &usize| Some(*s) != except && playable(*s);
    let count = (0..slots).filter(candidate).count();
    let pick = random.below(u64::try_from(count).ok()?.max(1));
    let pick = usize::try_from(pick).ok()?;
    (0..slots).filter(candidate).nth(pick)
}

/// Random numbers for picking follow actions: SplitMix64, small and allocation-free, and the
/// same sequence for the same seed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Random(u64);

impl Default for Random {
    fn default() -> Self {
        Self(DEFAULT_SEED)
    }
}

impl Random {
    pub(crate) fn seed(&mut self, seed: u64) {
        self.0 = seed;
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in 0 … `n` - 1; 0 when `n` is 0.
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

#[cfg(test)]
mod tests {
    use suhina_model::{ContentId, FollowAction, LoopRange};

    use super::*;

    fn clip(length: f64, looping: bool, follow: FollowAction) -> Clip {
        Clip {
            loop_range: looping.then_some(LoopRange { start: 0.0, end: length }),
            follow_action: follow,
            ..Clip::new(ContentId(1), 0.0, length)
        }
    }

    fn on() -> FollowAction {
        FollowAction { enabled: true, ..FollowAction::default() }
    }

    #[test]
    fn a_plan_exists_only_when_turned_on_with_a_chance() {
        assert!(FollowPlan::new(&clip(4.0, true, FollowAction::default())).is_none());
        let none = FollowAction { chance_a: 0, chance_b: 0, ..on() };
        assert!(FollowPlan::new(&clip(4.0, true, none)).is_none());
        assert!(FollowPlan::new(&clip(4.0, true, on())).is_some());
    }

    #[test]
    fn the_time_is_the_clip_length_when_linked_and_at_most_the_end_of_a_one_shot() {
        let linked = FollowPlan::new(&clip(3.0, true, on())).unwrap();
        assert!(!linked.due(2.99) && linked.due(3.0));
        let free = FollowAction { linked: false, time: 8.0, ..on() };
        let looping = FollowPlan::new(&clip(3.0, true, free)).unwrap();
        assert!(!looping.due(7.9) && looping.due(8.0));
        let once = FollowPlan::new(&clip(3.0, false, free)).unwrap();
        assert!(once.due(3.0));
        let tiny = FollowPlan::new(&clip(3.0, true, FollowAction { linked: false, time: 1e-12, ..on() })).unwrap();
        assert!(!tiny.due(0.0) && tiny.due(MIN_BEATS));
    }

    #[test]
    fn the_chances_weigh_a_against_b() {
        let mut random = Random::default();
        let only_b = FollowPlan::new(&clip(4.0, true, FollowAction { chance_a: 0, chance_b: 5, ..on() })).unwrap();
        assert!((0..100).all(|_| only_b.pick(&mut random) == FollowKind::Nothing));
        let third = FollowAction { action_b: FollowKind::Stop, chance_a: 25, chance_b: 75, ..on() };
        let third = FollowPlan::new(&clip(4.0, true, third)).unwrap();
        let a = (0..10_000).filter(|_| third.pick(&mut random) == FollowKind::Next).count();
        assert!((2_200..2_800).contains(&a), "{a}");
    }

    #[test]
    fn the_same_seed_picks_the_same_sequence() {
        let picks = |seed| {
            let mut random = Random::default();
            random.seed(seed);
            (0..32).map(|_| random.below(7)).collect::<Vec<_>>()
        };
        assert_eq!(picks(1), picks(1));
        assert_ne!(picks(1), picks(2));
    }

    #[test]
    fn actions_find_their_slot_and_skip_slots_that_cannot_play() {
        // Slots 0, 2 and 3 play; 1 and 4 cannot.
        let playable = |s: usize| [true, false, true, true, false].get(s).copied().unwrap_or(false);
        let mut r = Random::default();
        let mut at = |kind, current| resolve(kind, current, 5, playable, &mut r);
        assert_eq!(at(FollowKind::Nothing, 0), Outcome::Keep);
        assert_eq!(at(FollowKind::Stop, 0), Outcome::Stop);
        assert_eq!(at(FollowKind::Again, 2), Outcome::Launch(2));
        assert_eq!(at(FollowKind::Next, 0), Outcome::Launch(2));
        assert_eq!(at(FollowKind::Next, 3), Outcome::Launch(0), "wraps to the top");
        assert_eq!(at(FollowKind::Previous, 2), Outcome::Launch(0));
        assert_eq!(at(FollowKind::Previous, 0), Outcome::Launch(3), "wraps to the bottom");
        assert_eq!(at(FollowKind::First, 3), Outcome::Launch(0));
        assert_eq!(at(FollowKind::Last, 0), Outcome::Launch(3));
        assert_eq!(at(FollowKind::Jump(3), 0), Outcome::Launch(3));
        assert_eq!(at(FollowKind::Jump(1), 0), Outcome::Keep, "a slot that cannot play");
        assert_eq!(at(FollowKind::Jump(9), 0), Outcome::Keep, "a slot that does not exist");
    }

    #[test]
    fn any_and_other_pick_among_the_playable_slots() {
        let playable = |s: usize| s != 1;
        let mut r = Random::default();
        let mut seen = [0usize; 4];
        for _ in 0..400 {
            let Outcome::Launch(slot) = resolve(FollowKind::Any, 0, 4, playable, &mut r) else { panic!() };
            seen[slot] += 1;
        }
        assert!(seen[0] > 0 && seen[1] == 0 && seen[2] > 0 && seen[3] > 0, "{seen:?}");
        for _ in 0..100 {
            let outcome = resolve(FollowKind::Other, 2, 4, playable, &mut r);
            assert!(matches!(outcome, Outcome::Launch(0 | 3)), "{outcome:?}");
        }
        // Alone in its column, "next" and "other" play the clip again.
        let alone = |s: usize| s == 2;
        assert_eq!(resolve(FollowKind::Next, 2, 4, alone, &mut r), Outcome::Launch(2));
        assert_eq!(resolve(FollowKind::Other, 2, 4, alone, &mut r), Outcome::Launch(2));
        assert_eq!(resolve(FollowKind::Any, 0, 0, alone, &mut r), Outcome::Keep, "no slots at all");
    }
}
