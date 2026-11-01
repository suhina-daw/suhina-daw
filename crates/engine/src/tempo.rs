// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine's side of the tempo map (`docs/spec/tempo-map.md`): beats to arrangement frames
//! through [`TempoMap`] counted in frames, and bars through [`Meter`]. Every conversion in the
//! engine goes through these, so the arrangement, the play position, the launcher and the
//! metronome agree on when a beat is. At a constant tempo they do the arithmetic the engine has
//! always done, so renders of a project without changes stay the same to the bit.

use suhina_model::{Meter, TempoMap};

/// The arrangement frame `beat` starts at, through the project's tempo map, at `sample_rate`;
/// the same placement the engine's renders use. A beat before 0, or one that is not a number,
/// is frame 0. Never allocates.
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
