// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conversions between decibels and linear gain, in `f32` for signal paths and in `f64`
//! ([`db_to_gain_f64`], [`gain_to_db_f64`]) for control values and levels.

/// Gain at or below this many decibels is treated as silence by [`db_to_gain_or_silence`].
pub const SILENCE_DB: f32 = -70.0;

/// Linear gain for `db` decibels: 0 dB is 1.0, -6 dB about 0.5. Non-finite input gives 1.0.
pub fn db_to_gain(db: f32) -> f32 {
    if db.is_finite() { 10.0f32.powf(db / 20.0) } else { 1.0 }
}

/// Like [`db_to_gain`], but `db` at or below [`SILENCE_DB`] gives exactly 0.0, so a gain control
/// turned all the way down is silent.
pub fn db_to_gain_or_silence(db: f32) -> f32 {
    if db.is_finite() && db <= SILENCE_DB { 0.0 } else { db_to_gain(db) }
}

/// Decibels for a linear `gain`, never below [`SILENCE_DB`] (silence, negative and non-finite
/// gains included).
pub fn gain_to_db(gain: f32) -> f32 {
    if gain.is_finite() && gain > 0.0 { (20.0 * gain.log10()).max(SILENCE_DB) } else { SILENCE_DB }
}

/// Linear gain for `db` decibels, in `f64`: 0 dB is 1.0, -6 dB about 0.5. Non-finite input
/// gives 1.0.
pub fn db_to_gain_f64(db: f64) -> f64 {
    if db.is_finite() { 10.0f64.powf(db / 20.0) } else { 1.0 }
}

/// Decibels for a linear `gain`, in `f64`, never below `floor_db`: silence, negative and
/// non-finite gains give `floor_db`. Callers name their own floor, since what counts as
/// silence differs between a meter and a gain computer.
pub fn gain_to_db_f64(gain: f64, floor_db: f64) -> f64 {
    if gain.is_finite() && gain > 0.0 { (20.0 * gain.log10()).max(floor_db) } else { floor_db }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_both_ways() {
        assert_eq!(db_to_gain(0.0), 1.0);
        assert!((db_to_gain(-6.0) - 0.501_187).abs() < 1e-5);
        assert!((db_to_gain(20.0) - 10.0).abs() < 1e-4);
        for db in [-60.0, -12.5, 0.0, 6.0, 24.0] {
            assert!((gain_to_db(db_to_gain(db)) - db).abs() < 1e-3, "{db}");
        }
    }

    #[test]
    fn silence_and_hostile_values() {
        assert_eq!(db_to_gain_or_silence(SILENCE_DB), 0.0);
        assert_eq!(db_to_gain_or_silence(-1_000.0), 0.0);
        assert!(db_to_gain_or_silence(SILENCE_DB + 1.0) > 0.0);
        assert_eq!(db_to_gain(f32::NAN), 1.0);
        assert_eq!(db_to_gain_or_silence(f32::NEG_INFINITY), 1.0);
        assert_eq!(gain_to_db(0.0), SILENCE_DB);
        assert_eq!(gain_to_db(-1.0), SILENCE_DB);
        assert_eq!(gain_to_db(f32::NAN), SILENCE_DB);
    }

    #[test]
    fn converts_both_ways_in_f64() {
        assert_eq!(db_to_gain_f64(0.0), 1.0);
        assert_eq!(db_to_gain_f64(-12.5), 10.0f64.powf(-12.5 / 20.0));
        assert_eq!(db_to_gain_f64(f64::NAN), 1.0);
        for db in [-90.0, -12.5, 0.0, 6.0, 24.0] {
            assert!((gain_to_db_f64(db_to_gain_f64(db), -150.0) - db).abs() < 1e-9, "{db}");
        }
        assert_eq!(gain_to_db_f64(1e-12, -120.0), -120.0);
        for silent in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(gain_to_db_f64(silent, -96.0), -96.0);
        }
    }
}
