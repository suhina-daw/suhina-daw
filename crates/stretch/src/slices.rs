// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The slice stretch, for the Rhythmic mode (`docs/decisions/0003-time-stretching.md` ›
//! Amendment: a sound of each mode). The file is cut into slices (at its transients, or at a note
//! value of the clip's beats, and at the warp markers); each slice starts when the clip's time
//! reaches it and plays at the file's own speed (times the transpose), so a drum hit keeps its
//! attack and lands on its beat. Played faster than the file, a slice is cut off by the next one,
//! with a short crossfade; played slower, it ends before the next one starts, and its **tail**
//! fills the gap: silence (`Off`), its second half again and again (`Loop`), or its second half
//! back and forth (`BackAndForth`). The **envelope** fades each slice out over its length: 1 keeps
//! it level, 0 fades it to silence by its end.
//!
//! Nothing here allocates once built.

/// What a slice that ends before the next one starts plays until then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tail {
    /// Nothing: the slice fades out at its end.
    Off,
    /// Its second half, again and again.
    #[default]
    Loop,
    /// Its second half, forwards then backwards.
    BackAndForth,
}

/// Frames over which a slice cut off by the next one fades out.
const CROSSFADE_SECONDS: f64 = 0.004;
/// Frames over which a slice that starts away from its own start (after a seek or a jump)
/// fades in; one that starts on its start keeps its attack.
const FADE_IN_SECONDS: f64 = 0.0015;
/// Frames over which a slice that starts on its own start fades in: short enough to keep a drum's
/// attack, long enough that a slice cut in the middle of a sound does not click.
const ATTACK_SECONDS: f64 = 0.000_25;
/// A new slice starts on its own start, rather than where the clip's time is, when that is at
/// most this far into it: it was reached by playing, not by a jump.
const SNAP_SECONDS: f64 = 0.03;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Play {
    /// The slice's number, the output frame it started at and the file frame it started from.
    index: usize,
    out_start: u64,
    in_start: f64,
    /// File frames from where it started to the next slice's start.
    length: f64,
    /// It carries on from the slice before without a seam (the one before had just reached its
    /// start): no crossfade and no fade in, so audio at its own speed plays as it is.
    seamless: bool,
    /// The next slice starts as this one runs out: it has no tail, so its end is not faded or
    /// blended into a loop either.
    followed: bool,
}

/// A file cut into slices, as the Rhythmic mode plays it.
#[derive(Debug, Clone)]
pub struct Slices {
    channels: usize,
    /// The file frames where slices start, in order, the first 0.
    points: Vec<f64>,
    frames: f64,
    tail: Tail,
    envelope: f64,
    pitch: f64,
    crossfade: f64,
    fade_in: f64,
    attack: f64,
    snap: f64,
    playing: Option<Play>,
    /// A slice cut off by the one playing, and the output frame it started fading at.
    leaving: Option<(Play, u64)>,
    /// The last output frame rendered, to tell playing on from a jump.
    last: Option<u64>,
}

impl Slices {
    /// Slices of a file of `frames` frames at `rate` frames a second, starting at `points` (file
    /// frames, any order; 0 is added). Allocates: build it off the audio thread.
    pub fn new(channels: usize, rate: f32, frames: usize, points: &[f64], tail: Tail, envelope: f32) -> Self {
        let frames = frames as f64;
        let mut points: Vec<f64> = std::iter::once(0.0)
            .chain(points.iter().copied().filter(|p| p.is_finite() && *p > 0.0 && *p < frames))
            .collect();
        points.sort_by(f64::total_cmp);
        points.dedup_by(|a, b| (*a - *b).abs() < 1.0);
        let rate = f64::from(rate.max(1.0));
        Self {
            channels: channels.max(1),
            points,
            frames,
            tail,
            envelope: f64::from(if envelope.is_finite() { envelope.clamp(0.0, 1.0) } else { 1.0 }),
            pitch: 1.0,
            crossfade: (CROSSFADE_SECONDS * rate).max(1.0),
            fade_in: (FADE_IN_SECONDS * rate).max(1.0),
            attack: (ATTACK_SECONDS * rate).max(1.0),
            snap: SNAP_SECONDS * rate,
            playing: None,
            leaving: None,
            last: None,
        }
    }

    /// Plays each slice `factor` times higher (and faster); 1 keeps its pitch.
    pub fn set_pitch(&mut self, factor: f64) {
        self.pitch = if factor.is_finite() && factor > 0.0 { factor } else { 1.0 };
    }

    /// The file frames where slices start.
    pub fn points(&self) -> &[f64] {
        &self.points
    }

    /// Forgets the slice playing: the next render starts where the clip's time is (after a seek).
    pub fn reset(&mut self) {
        self.playing = None;
        self.leaving = None;
        self.last = None;
    }

    /// Renders output frames `first..` into `out` (interleaved): `position` gives the file frame
    /// the clip's time is at for an output frame, `sample` a file channel's value at a frame.
    /// Never allocates.
    pub fn render(
        &mut self,
        first: u64,
        out: &mut [f32],
        position: &impl Fn(u64) -> f64,
        sample: &impl Fn(f64, usize) -> f32,
    ) {
        let channels = self.channels;
        for (i, frame) in out.chunks_exact_mut(channels).enumerate() {
            let t = first + i as u64;
            let on = self.last.is_some_and(|l| l.checked_add(1) == Some(t));
            let p = position(t);
            let index = self.points.partition_point(|s| *s <= p).saturating_sub(1);
            if !on || self.playing.is_none_or(|play| play.index != index) {
                let start = self.points.get(index).copied().unwrap_or(0.0);
                let next = self.points.get(index + 1).copied().unwrap_or(self.frames);
                // Reached by playing on: from the slice's own start, so its attack is on time.
                let in_start = if on && p.is_finite() && p - start <= self.snap { start } else { p };
                // The slice before has just reached this one's start (the clip at the file's
                // speed): it carries on, with no seam.
                let seamless = on
                    && self.playing.is_some_and(|before| {
                        let gone = (t - before.out_start) as f64 * self.pitch;
                        gone <= before.length + 1.0 && (before.in_start + gone - in_start).abs() < 1.0
                    });
                let length = (next - in_start).max(1.0);
                // Where the clip's time is when this slice runs out: at the next one already, or
                // short of it (a tail plays).
                let runs_out = t.saturating_add((length / self.pitch).floor() as u64);
                let followed = index + 1 < self.points.len() && position(runs_out) >= next - 1.0;
                let fresh = Play { index, out_start: t, in_start, length, seamless, followed };
                self.leaving = if on && !seamless { self.playing.map(|play| (play, t)) } else { None };
                self.playing = Some(fresh);
            }
            self.last = Some(t);
            for c in 0..channels {
                let mut value = self.playing.map_or(0.0, |play| self.voice(play, t, c, sample));
                if let Some((play, from)) = self.leaving {
                    let fade = 1.0 - (t - from) as f64 / self.crossfade;
                    if fade > 0.0 {
                        value += self.voice(play, t, c, sample) * fade as f32;
                    }
                }
                if let Some(o) = frame.get_mut(c) {
                    *o = value;
                }
            }
            if self.leaving.is_some_and(|(_, from)| (t - from) as f64 >= self.crossfade) {
                self.leaving = None;
            }
        }
    }

    /// Channel `c` of slice `play` at output frame `t`: its sound, its tail past its end, its
    /// envelope and its fade in.
    fn voice(&self, play: Play, t: u64, c: usize, sample: &impl Fn(f64, usize) -> f32) -> f32 {
        let gone = (t - play.out_start) as f64 * self.pitch;
        let length = play.length;
        let at = if gone < length {
            play.in_start + gone
        } else {
            let half = length / 2.0;
            let span = (length - half).max(1.0);
            let past = gone - length;
            match self.tail {
                Tail::Off => return 0.0,
                Tail::Loop => play.in_start + half + past.rem_euclid(span),
                Tail::BackAndForth => {
                    let turn = (past / span).floor();
                    let into = past - turn * span;
                    // First back from the end, then forwards again.
                    if turn.rem_euclid(2.0) < 1.0 { play.in_start + length - into } else { play.in_start + half + into }
                }
            }
        };
        let mut gain = (1.0 - (1.0 - self.envelope) * gone / length).clamp(0.0, 1.0);
        if self.tail == Tail::Off && !play.followed {
            // A slice with nothing after it fades out over its last moment, without a click.
            gain *= ((length - gone) / self.crossfade).clamp(0.0, 1.0);
        }
        // Started away from a slice start (a seek or a jump): faded in, so there is no click; on
        // its start, faded in only over its attack's first moment.
        let on_start = self.points.get(play.index).is_some_and(|s| (s - play.in_start).abs() < 1.0);
        if !play.seamless {
            let fade = if on_start { self.attack } else { self.fade_in };
            gain *= ((t - play.out_start) as f64 / fade).min(1.0);
        }
        // A looping tail jumps from the slice's end back to its middle: over the moment before
        // each jump the sound fades into the sound after the jump (both the slice's own), so the
        // seam does not click. A slice the next one follows on time has no tail and plays as it is.
        let end = play.in_start + length;
        let back = length - length / 2.0;
        let seam = self.crossfade.min(back / 2.0);
        if self.tail == Tail::Loop && !play.followed && seam >= 1.0 && at > end - seam && at < end {
            let w = ((at - (end - seam)) / seam) as f32;
            return (sample(at, c) * (1.0 - w) + sample(at - back, c) * w) * gain as f32;
        }
        sample(at, c) * gain as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A file of 1 s with a click (a 1 ms burst) every 0.25 s, as a function of the frame.
    fn clicks(at: f64, _: usize) -> f32 {
        if !(0.0..48_000.0).contains(&at) {
            return 0.0;
        }
        if (at as usize % 12_000) < 48 { 1.0 } else { 0.0 }
    }

    fn onsets(out: &[f32]) -> Vec<usize> {
        let mut found = Vec::new();
        for (i, w) in out.windows(2).enumerate() {
            if w[0].abs() < 0.5 && w[1].abs() >= 0.5 && found.last().is_none_or(|l: &usize| i + 1 > l + 2_000) {
                found.push(i + 1);
            }
        }
        found
    }

    /// Whether `found` are `want`, each within the attack's fade (12 frames).
    fn near(found: &[usize], want: &[usize]) -> bool {
        found.len() == want.len() && found.iter().zip(want).all(|(f, w)| f.abs_diff(*w) <= 12)
    }

    fn points() -> Vec<f64> {
        (1..4).map(|k| f64::from(k) * 12_000.0).collect()
    }

    /// At half speed every click starts on its new time, twice as far apart, at its own pitch:
    /// the clicks stay 48 frames long.
    #[test]
    fn slowed_down_each_slice_starts_on_its_time() {
        let mut slices = Slices::new(1, RATE, 48_000, &points(), Tail::Off, 1.0);
        let mut out = vec![0.0; 96_000];
        for (i, chunk) in out.chunks_mut(500).enumerate() {
            slices.render((i * 500) as u64, chunk, &|t| t as f64 * 0.5, &clicks);
        }
        assert!(near(&onsets(&out), &[0, 24_000, 48_000, 72_000]), "{:?}", onsets(&out));
        let loud = out[24_000..24_100].iter().filter(|s| s.abs() > 0.5).count();
        assert!((40..=50).contains(&loud), "{loud} frames of click");
    }

    #[test]
    fn sped_up_each_slice_cuts_the_one_before() {
        let mut slices = Slices::new(1, RATE, 48_000, &points(), Tail::Off, 1.0);
        let mut out = vec![0.0; 24_000];
        slices.render(0, &mut out, &|t| t as f64 * 2.0, &clicks);
        assert!(near(&onsets(&out), &[0, 6_000, 12_000, 18_000]), "{:?}", onsets(&out));
    }

    /// Slowed down, Loop fills the gap after a slice with its second half; Off leaves silence.
    #[test]
    fn the_tail_fills_the_gap_or_not() {
        let tone = |at: f64, _: usize| if (0.0..48_000.0).contains(&at) { 0.5 } else { 0.0 };
        for (tail, sounds) in [(Tail::Off, false), (Tail::Loop, true), (Tail::BackAndForth, true)] {
            let mut slices = Slices::new(1, RATE, 48_000, &points(), tail, 1.0);
            let mut out = vec![0.0; 96_000];
            slices.render(0, &mut out, &|t| t as f64 * 0.5, &tone);
            // 0.25 s into the output the first slice (12 000 frames) has ended; the next starts
            // at 24 000.
            let gap = out[16_000..23_000].iter().map(|s| s.abs()).fold(0.0, f32::max);
            assert_eq!(gap > 0.4, sounds, "{tail:?}: {gap}");
        }
    }

    #[test]
    fn the_envelope_fades_a_slice_over_its_length() {
        let tone = |at: f64, _: usize| if (0.0..48_000.0).contains(&at) { 1.0 } else { 0.0 };
        let mut slices = Slices::new(1, RATE, 48_000, &points(), Tail::Off, 0.0);
        let mut out = vec![0.0; 12_000];
        slices.render(0, &mut out, &|t| t as f64, &tone);
        assert!((out[20] - 1.0).abs() < 2e-3 && (out[6_000] - 0.5).abs() < 0.01 && out[11_990] < 0.01);
    }

    #[test]
    fn a_seek_starts_mid_slice_faded_in_and_plays_on() {
        let tone = |at: f64, _: usize| if (0.0..48_000.0).contains(&at) { 1.0 } else { 0.0 };
        let mut slices = Slices::new(1, RATE, 48_000, &points(), Tail::Loop, 1.0);
        let mut out = vec![0.0; 1_000];
        slices.render(30_000, &mut out, &|t| t as f64, &tone);
        assert!(out[0] < 0.05, "faded in: {}", out[0]);
        assert!((out[500] - 1.0).abs() < 1e-3);
    }

    #[test]
    fn odd_points_are_left_out() {
        let slices = Slices::new(2, RATE, 1_000, &[f64::NAN, -5.0, 2_000.0, 500.0, 500.2], Tail::Loop, f32::NAN);
        assert_eq!(slices.points(), [0.0, 500.0]);
    }
}

#[cfg(test)]
mod seam_tests {
    use super::*;

    /// A looping tail on a held tone has no step where it jumps back: the largest change from one
    /// frame to the next stays near the tone's own.
    #[test]
    fn a_looping_tail_has_no_click_at_its_seam() {
        let rate = 48_000.0;
        let tone = |at: f64, _: usize| {
            if (0.0..48_000.0).contains(&at) {
                (std::f64::consts::TAU * 440.0 * at / 48_000.0).sin() as f32 * 0.5
            } else {
                0.0
            }
        };
        let mut slices = Slices::new(1, rate, 48_000, &[12_000.0, 24_000.0, 36_000.0], Tail::Loop, 1.0);
        let mut out = vec![0.0; 40_000];
        slices.render(0, &mut out, &|t| t as f64 * 0.6, &tone);
        let own = 0.5 * std::f32::consts::TAU * 440.0 / 48_000.0;
        // Away from the slice starts, where a new slice cuts in.
        let worst = out[2_000..19_000].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(worst < own * 2.0, "a step of {worst}, the tone's own {own}");
    }
}

#[cfg(test)]
mod speed_tests {
    use super::*;

    /// At the file's own speed slices play the file as it is: no seam, no bump at a slice start.
    #[test]
    fn at_its_own_speed_the_file_plays_as_it_is() {
        let tone = |at: f64, _: usize| {
            if (0.0..48_000.0).contains(&at) {
                (std::f64::consts::TAU * 440.0 * at / 48_000.0).sin() as f32 * 0.5
            } else {
                0.0
            }
        };
        // Slices that are no whole number of the tone's periods long, so a blend would show.
        let mut slices = Slices::new(1, 48_000.0, 48_000, &[12_345.0, 24_017.0, 36_931.0], Tail::Loop, 1.0);
        let mut out = vec![0.0; 40_000];
        for (i, chunk) in out.chunks_mut(700).enumerate() {
            slices.render((i * 700) as u64, chunk, &|t| t as f64, &tone);
        }
        let worst = (100..40_000).map(|i| (out[i] - tone(i as f64, 0)).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-6, "{worst}");
    }
}
