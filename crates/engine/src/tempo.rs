// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine's side of the tempo map (`docs/spec/tempo-map.md`): beats to arrangement frames
//! through [`TempoMap`] counted in frames, and bars through [`Meter`]. Every conversion in the
//! engine goes through these, so the arrangement, the play position, the launcher and the
//! metronome agree on when a beat is. At a constant tempo they do the arithmetic the engine has
//! always done, so renders of a project without changes stay the same to the bit.

use std::sync::Arc;

use suhina_model::{Meter, TempoMap};

/// When beats play (#764 › 5): through a tempo map counted in frames, or at one tempo. The
/// arrangement's audio clips, their fades, the stretched clips and the launcher's clock all ask
/// it, so they agree on the frame of a beat. At a constant tempo every method does the arithmetic
/// the engine has always done, in the same order (the fast path), so renders of a project
/// without tempo changes stay the same to the bit; only a changing map goes through the map.
/// Allocation-free once built.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TempoClock {
    /// One tempo: `bpm`, and `per_beat` frames per beat.
    Constant { bpm: f64, per_beat: f64 },
    /// A tempo that changes, and its start tempo as the callers that read one give it.
    Map { map: Arc<TempoMap>, bpm: f64, per_beat: f64 },
}

impl TempoClock {
    /// The clock of `map`: its fast path when the map is constant. The start tempo is the map's.
    pub(crate) fn of(map: &Arc<TempoMap>) -> Self {
        Self::of_with(map, map.start_bpm(), map.start_per_beat())
    }

    /// Like [`TempoClock::of`], with the start tempo `bpm` and `per_beat` as the caller has them
    /// (the project's tempo as it is written, which the map may have corrected).
    pub(crate) fn of_with(map: &Arc<TempoMap>, bpm: f64, per_beat: f64) -> Self {
        if map.is_constant() {
            Self::Constant { bpm, per_beat }
        } else {
            Self::Map { map: Arc::clone(map), bpm, per_beat }
        }
    }

    /// At one tempo, `bpm`, at `sample_rate`.
    pub(crate) fn constant(bpm: f64, sample_rate: f64) -> Self {
        Self::Constant { bpm, per_beat: frames_per_beat(bpm, sample_rate) }
    }

    /// The start tempo, in bpm.
    pub(crate) fn bpm(&self) -> f64 {
        match self {
            Self::Constant { bpm, .. } | Self::Map { bpm, .. } => *bpm,
        }
    }

    /// Frames per beat at the start tempo.
    pub(crate) fn per_beat(&self) -> f64 {
        match self {
            Self::Constant { per_beat, .. } | Self::Map { per_beat, .. } => *per_beat,
        }
    }

    /// The map, when the tempo changes.
    pub(crate) fn map(&self) -> Option<&Arc<TempoMap>> {
        match self {
            Self::Constant { .. } => None,
            Self::Map { map, .. } => Some(map),
        }
    }

    /// The tempo at `beat`, in bpm.
    pub(crate) fn bpm_at(&self, beat: f64) -> f64 {
        match self {
            Self::Constant { bpm, .. } => *bpm,
            Self::Map { map, .. } => map.bpm_at(beat),
        }
    }

    /// The frame `beat` plays at, rounded; a beat before 0 is frame 0.
    pub(crate) fn frame_of(&self, beat: f64) -> u64 {
        match self {
            Self::Constant { per_beat, .. } => (beat.max(0.0) * per_beat).round() as u64,
            Self::Map { map, .. } => frame_of(map, beat),
        }
    }

    /// The frames from `beat` to `beat + beats`.
    pub(crate) fn span(&self, beat: f64, beats: f64) -> f64 {
        match self {
            Self::Constant { per_beat, .. } => beats * per_beat,
            Self::Map { map, .. } => map.span(beat, beats),
        }
    }

    /// Seconds from song beat `start` to `played` beats after it, at `sample_rate`.
    pub(crate) fn seconds_from(&self, start: f64, played: f64, sample_rate: f64) -> f64 {
        match self {
            Self::Constant { bpm, .. } => played * seconds_per_beat(*bpm),
            Self::Map { map, .. } => (map.time_at(start + played) - map.time_at(start)) / sample_rate,
        }
    }

    /// Seconds that the `beats` from `played` beats after song beat `start` take.
    pub(crate) fn seconds_over(&self, start: f64, played: f64, beats: f64, sample_rate: f64) -> f64 {
        match self {
            Self::Constant { bpm, .. } => beats * seconds_per_beat(*bpm),
            Self::Map { .. } => {
                self.seconds_from(start, played + beats, sample_rate) - self.seconds_from(start, played, sample_rate)
            }
        }
    }

    /// Seconds between `from` and `to` beats after song beat `start`.
    pub(crate) fn seconds_between(&self, start: f64, from: f64, to: f64, sample_rate: f64) -> f64 {
        match self {
            Self::Constant { bpm, .. } => (to - from) * seconds_per_beat(*bpm),
            Self::Map { .. } => self.seconds_from(start, to, sample_rate) - self.seconds_from(start, from, sample_rate),
        }
    }

    /// The most beats from song beat `beat` that keep one speed: up to the next tempo change,
    /// and `ramp_piece` beats in a ramp. Unlimited at a constant tempo.
    pub(crate) fn steady_beats(&self, beat: f64, ramp_piece: f64) -> f64 {
        let Self::Map { map, .. } = self else { return f64::INFINITY };
        let next_change = map.next_break_after(beat + 1e-9).map_or(f64::INFINITY, |b| b - beat);
        let ramp = if map.ramps_at(beat + 1e-9) { ramp_piece } else { f64::INFINITY };
        next_change.min(ramp)
    }
}

/// Frames per beat at a constant `bpm` and `sample_rate`: the one place `60 / bpm × rate` is
/// written, in that order, so every caller rounds the same way.
#[inline]
pub(crate) fn frames_per_beat(bpm: f64, sample_rate: f64) -> f64 {
    60.0 / bpm * sample_rate
}

/// Beats per frame at a constant `bpm` and `sample_rate`: `bpm / 60 / rate`.
#[inline]
pub(crate) fn beats_per_frame(bpm: f64, sample_rate: f64) -> f64 {
    bpm / 60.0 / sample_rate
}

/// Seconds per beat at a constant `bpm`: `60 / bpm`.
#[inline]
pub(crate) fn seconds_per_beat(bpm: f64) -> f64 {
    60.0 / bpm
}

/// The arrangement frame `beat` starts at, through the project's tempo map, at `sample_rate`;
/// the same placement the engine's renders use. A beat before 0, or one that is not a number,
/// is frame 0. Never allocates.
pub fn frame_of_beat(project: &suhina_model::Project, beat: f64, sample_rate: u32) -> u64 {
    let map = project.transport.tempo_map(f64::from(sample_rate));
    frame_of(&map, beat)
}

/// The arrangement frame of `beat`, rounded; a beat before 0, or one that is not a number, is
/// frame 0, and one too far for a frame number is the last frame (never reached). Never
/// allocates.
pub(crate) fn frame_of(map: &TempoMap, beat: f64) -> u64 {
    let frame = map.time_at(beat.max(0.0)).round();
    // `as` saturates: an infinite time is the last frame, as the engine always placed it.
    if frame > 0.0 { frame as u64 } else { 0 }
}

/// The beat at arrangement frame `frame`. Never allocates.
pub(crate) fn beat_at(map: &TempoMap, frame: u64) -> f64 {
    map.beat_at(frame as f64)
}

/// The beats from a clip's start (`start_frame`, the frame of `start_beat`) to frame `position`:
/// at a constant tempo the frames divided by the frames per beat, as the engine always counted
/// them; otherwise the map's beat at `position` less the start beat. Never allocates.
pub(crate) fn beats_since(map: &TempoMap, start_frame: u64, start_beat: f64, position: u64) -> f64 {
    if map.is_constant() {
        let per_beat = map.start_per_beat();
        return if per_beat > 0.0 { position.saturating_sub(start_frame) as f64 / per_beat } else { 0.0 };
    }
    (map.beat_at(position as f64) - start_beat).max(0.0)
}

/// Arrangement frame `time` in `from`, moved to the same beat in `to` (a tempo edit keeps the
/// playhead on its beat, ADR 0005). Between two constant maps this is the rescale the engine has
/// always done, so the controller and the audio thread agree to the frame.
pub(crate) fn rescale(time: u64, from: &TempoMap, to: &TempoMap) -> u64 {
    if from == to {
        return time;
    }
    if from.is_constant() && to.is_constant() {
        return rescale_constant(time, from.start_per_beat(), to.start_per_beat());
    }
    frame_of(to, from.beat_at(time as f64))
}

/// [`rescale`] between two constant tempos given as frames per beat.
pub(crate) fn rescale_constant(time: u64, from: f64, to: f64) -> u64 {
    if from > 0.0 && to > 0.0 && from != to { (time as f64 / from * to).round() as u64 } else { time }
}

/// Whether beat number `index` (counted from beat 0) starts a bar. Never allocates.
pub(crate) fn starts_bar(meter: &Meter, index: i64) -> bool {
    let beat = index as f64;
    if meter.is_constant() {
        let bar_beats = meter.start_bar_beats();
        return bar_beats > 0.0 && (beat / bar_beats).fract().abs() < 1e-9;
    }
    (meter.position(beat + 1e-9).bar_start - beat).abs() < 1e-9
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{TempoPoint, TimeSignature, TimeSignatureChange};

    const SR: f64 = 48_000.0;

    #[test]
    fn a_constant_clock_does_the_arithmetic_the_engine_always_did() {
        let map = Arc::new(TempoMap::constant(97.0, SR));
        let clock = TempoClock::of(&map);
        assert!(clock.map().is_none(), "a constant map takes the fast path");
        let per_beat = 60.0 / 97.0 * SR;
        assert_eq!(clock.per_beat().to_bits(), per_beat.to_bits());
        for beat in [0.0, 1.0, 3.75, 1000.125] {
            assert_eq!(clock.frame_of(beat), (beat * per_beat).round() as u64);
            assert_eq!(clock.span(beat, 2.5).to_bits(), (2.5 * per_beat).to_bits());
            assert_eq!(clock.seconds_from(beat, 3.0, SR).to_bits(), (3.0_f64 * (60.0 / 97.0)).to_bits());
            assert_eq!(clock.seconds_over(beat, 1.0, 0.5, SR).to_bits(), (0.5_f64 * (60.0 / 97.0)).to_bits());
            assert_eq!(
                clock.seconds_between(beat, 1.0, 2.5, SR).to_bits(),
                ((2.5_f64 - 1.0) * (60.0 / 97.0)).to_bits()
            );
        }
        assert_eq!(clock.frame_of(-2.0), 0);
        assert_eq!(clock.steady_beats(4.0, 0.25), f64::INFINITY);
        assert_eq!(clock.bpm_at(50.0), 97.0);
        // The caller's start tempo is kept as given.
        assert_eq!(TempoClock::of_with(&map, 2000.0, 1.0), TempoClock::Constant { bpm: 2000.0, per_beat: 1.0 });
    }

    #[test]
    fn a_changing_clock_goes_through_the_map() {
        let changes = [TempoPoint { beat: 4.0, bpm: 60.0, ramp: false }];
        let map = Arc::new(TempoMap::new(120.0, &changes, SR));
        let clock = TempoClock::of(&map);
        assert!(clock.map().is_some());
        assert_eq!(clock.frame_of(6.0), frame_of(&map, 6.0));
        assert_eq!(clock.span(3.0, 2.0), map.span(3.0, 2.0));
        // Two beats at 120 then two at 60: one second, then two.
        assert_eq!(clock.seconds_from(2.0, 4.0, SR), 3.0);
        assert_eq!(clock.seconds_between(2.0, 2.0, 4.0, SR), 2.0);
        assert_eq!(clock.steady_beats(1.0, 0.25), 3.0);
        assert_eq!(clock.bpm_at(5.0), 60.0);
        assert_ne!(clock, TempoClock::of(&Arc::new(TempoMap::constant(120.0, SR))));
    }

    #[test]
    fn a_constant_map_gives_the_frames_the_engine_always_gave() {
        let map = TempoMap::constant(97.0, SR);
        let per_beat = 60.0 / 97.0 * SR;
        for beat in [0.0, 1.0, 3.75, 1000.125] {
            assert_eq!(frame_of(&map, beat), (beat * per_beat).round() as u64);
            let frame = frame_of(&map, beat);
            assert_eq!(beats_since(&map, 100, 9.0, frame + 100), frame as f64 / per_beat);
        }
        assert_eq!(frame_of(&map, -2.0), 0);
        assert_eq!(frame_of(&map, f64::NAN), 0);
        assert_eq!(frame_of(&map, f64::INFINITY), u64::MAX);
        let other = TempoMap::constant(120.0, SR);
        assert_eq!(rescale(48_000, &other, &map), (48_000.0 / 24_000.0 * per_beat).round() as u64);
    }

    #[test]
    fn a_change_moves_later_beats() {
        // 120 bpm (24 000 frames a beat) to beat 4, then 60 bpm (48 000).
        let map = TempoMap::new(120.0, &[TempoPoint { beat: 4.0, bpm: 60.0, ramp: false }], SR);
        assert_eq!(frame_of(&map, 4.0), 96_000);
        assert_eq!(frame_of(&map, 5.0), 144_000);
        assert_eq!(beats_since(&map, 96_000, 4.0, 120_000), 0.5);
        // The playhead at beat 5 under a constant 120 stays at beat 5 under the map.
        assert_eq!(rescale(120_000, &TempoMap::constant(120.0, SR), &map), 144_000);
    }

    #[test]
    fn bars_follow_the_meter() {
        let ts = |numerator, denominator| TimeSignature { numerator, denominator };
        let constant = Meter::new(ts(3, 4), &[]);
        assert!(starts_bar(&constant, 0) && starts_bar(&constant, 3) && !starts_bar(&constant, 4));
        // 4/4 for one bar, then 3/4: bars at 0, 4, 7, 10.
        let meter = Meter::new(ts(4, 4), &[TimeSignatureChange { bar: 1, time_signature: ts(3, 4) }]);
        let starts: Vec<i64> = (0..12).filter(|&b| starts_bar(&meter, b)).collect();
        assert_eq!(starts, vec![0, 4, 7, 10]);
    }
}
