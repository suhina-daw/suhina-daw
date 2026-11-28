// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The recorded glide of a fitted recording: how its pitch moved into the note (a cello sliding
//! into it, a voice scooping up to it). See `docs/spec/instrument-modal-profile.md` › Glide.

/// Points of a glide, and the f32 values one encodes to.
pub const GLIDE_POINTS: usize = 16;
pub const GLIDE_FIELDS: usize = 2 * GLIDE_POINTS;
/// The longest glide and the farthest it may start from its note.
pub const MAX_GLIDE_SECONDS: f32 = 4.0;
pub const MAX_GLIDE_CENTS: f32 = 2400.0;

/// A glide into the note: `count` points of (seconds from the strike, cents from the note),
/// the first at 0 s, times increasing, the last at 0 cents; the note holds its pitch after it.
/// Fewer than two points is no glide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    pub count: u8,
    pub times: [f32; GLIDE_POINTS],
    pub cents: [f32; GLIDE_POINTS],
}

impl Default for Glide {
    fn default() -> Self {
        Self::NONE
    }
}

impl Glide {
    pub const NONE: Self = Self { count: 0, times: [0.0; GLIDE_POINTS], cents: [0.0; GLIDE_POINTS] };

    /// The glide through `points` (seconds, cents), at most [`GLIDE_POINTS`], unvalidated.
    pub fn from_points(points: &[(f32, f32)]) -> Self {
        let mut glide = Self::NONE;
        for ((time, cents), (t, c)) in glide.times.iter_mut().zip(glide.cents.iter_mut()).zip(points) {
            *time = *t;
            *cents = *c;
        }
        glide.count = points.len().min(GLIDE_POINTS) as u8;
        glide
    }

    pub fn is_empty(&self) -> bool {
        self.count < 2
    }

    /// Seconds until the note holds its pitch.
    pub fn end(&self) -> f64 {
        self.times.get(usize::from(self.count).saturating_sub(1)).map_or(0.0, |t| f64::from(*t))
    }

    /// Cents from the note at `seconds` after the strike: interpolated linearly, 0 once over.
    pub fn cents_at(&self, seconds: f64) -> f64 {
        let count = usize::from(self.count).min(GLIDE_POINTS);
        if count < 2 || !seconds.is_finite() || seconds >= self.end() {
            return 0.0;
        }
        let mut previous = (f64::from(self.times[0]), f64::from(self.cents[0]));
        if seconds <= previous.0 {
            return previous.1;
        }
        for (time, cents) in self.times.iter().zip(&self.cents).take(count).skip(1) {
            let (t, c) = (f64::from(*time), f64::from(*cents));
            if seconds <= t {
                let span = (t - previous.0).max(1e-9);
                return previous.1 + (c - previous.1) * (seconds - previous.0) / span;
            }
            previous = (t, c);
        }
        0.0
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        const ERROR: &str = "a glide needs 2..16 points from 0 s, times increasing to at most 4 s, cents within two octaves, ending at 0";
        let count = usize::from(self.count);
        if !(2..=GLIDE_POINTS).contains(&count) || self.times[0] != 0.0 {
            return Err(ERROR);
        }
        let used = self.times.iter().zip(&self.cents).take(count);
        let mut last = -1.0_f32;
        for (time, cents) in used {
            if !time.is_finite() || *time <= last || *time > MAX_GLIDE_SECONDS || !cents.is_finite() {
                return Err(ERROR);
            }
            if cents.abs() > MAX_GLIDE_CENTS {
                return Err(ERROR);
            }
            last = *time;
        }
        let unused = self.times.iter().zip(&self.cents).skip(count).all(|(t, c)| *t == 0.0 && *c == 0.0);
        if self.cents[count - 1] != 0.0 || !unused {
            return Err(ERROR);
        }
        Ok(())
    }

    /// The values in their encoded order: times, then cents.
    pub fn fields(&self) -> [f32; GLIDE_FIELDS] {
        let mut out = [0.0; GLIDE_FIELDS];
        for (slot, value) in out.iter_mut().zip(self.times.iter().chain(&self.cents)) {
            *slot = *value;
        }
        out
    }

    /// The glide of `count` points whose encoded values are `fields`, unvalidated.
    pub fn from_fields(count: u8, fields: &[f32; GLIDE_FIELDS]) -> Self {
        let mut glide = Self { count, ..Self::NONE };
        for (target, value) in glide.times.iter_mut().chain(glide.cents.iter_mut()).zip(fields) {
            *target = *value;
        }
        glide
    }
}

#[cfg(test)]
#[path = "glide_tests.rs"]
mod tests;
