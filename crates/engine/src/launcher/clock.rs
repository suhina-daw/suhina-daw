// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`LaunchClock`]: the launcher's bars (#764 › 5). The time signature playing, where its bars
//! are counted from, a scene's signature waiting for its beat, and the global launch
//! quantisation, and from them the beat a launch issued now happens at. The tempo is the
//! [`crate::tempo::TempoClock`]'s; this clock reads the beat it is handed. Never allocates.

use std::sync::atomic::Ordering;

use suhina_model::{Meter, TimeSignature};

use super::{LauncherMonitor, Quantization};

/// The launcher's bars and quantisation, on the project's meter or, after a scene changed the
/// tempo or the time signature, on a signature of its own
/// (`docs/spec/tempo-map.md` › Scenes and the tempo map).
#[derive(Debug, Clone)]
pub(crate) struct LaunchClock {
    pub(crate) quantization: Quantization,
    /// The time signature playing, and the project's, which the playing one follows when it is
    /// edited.
    pub(crate) time_signature: TimeSignature,
    project_signature: TimeSignature,
    pub(crate) bar_beats: f64,
    /// The beat bars are counted from: where a scene last changed the time signature.
    pub(crate) bar_origin: f64,
    /// Bars played before `bar_origin`.
    pub(crate) bars_before: f64,
    /// A scene's time signature waiting for its launch beat.
    pub(crate) pending_signature: Option<(f64, TimeSignature)>,
    /// The project's meter (`docs/spec/tempo-map.md`).
    pub(crate) meter: Meter,
    /// The bars are the meter's; a scene's tempo or time signature takes the clock off it until
    /// the map is edited.
    pub(crate) follows_map: bool,
}

impl LaunchClock {
    /// The clock at the start of `meter`, whose first signature is `signature`, launching on the
    /// bar.
    pub(crate) fn new(signature: TimeSignature, meter: Meter) -> Self {
        Self {
            quantization: Quantization::Bars(1),
            time_signature: signature,
            project_signature: signature,
            bar_beats: meter.start_bar_beats(),
            bar_origin: 0.0,
            bars_before: 0.0,
            pending_signature: None,
            meter,
            follows_map: true,
        }
    }

    /// Takes over `old`'s quantisation and, when the meter is the same and `same_tempo` says the
    /// tempo map is too, its bars and waiting signature: an edited map or meter puts the clock
    /// back on the map and drops a scene's signature still waiting for its launch beat. Whether
    /// the bars were kept.
    pub(crate) fn adopt(&mut self, old: &LaunchClock, same_tempo: bool) -> bool {
        self.quantization = old.quantization;
        let kept = same_tempo && self.meter == old.meter;
        if kept {
            self.pending_signature = old.pending_signature;
            self.follows_map = old.follows_map;
            self.time_signature = old.time_signature;
            self.bar_beats = old.bar_beats;
            self.bar_origin = old.bar_origin;
            self.bars_before = old.bars_before;
        } else {
            self.pending_signature = None;
            self.follows_map = true;
        }
        kept
    }

    /// Whether beat number `index` (counted from beat 0) starts a bar, for the metronome:
    /// through the project's meter while the clock follows the tempo map, otherwise through the
    /// time signature playing, its bars counted from where it last changed.
    pub(crate) fn starts_bar(&self, index: i64) -> bool {
        if self.follows_map {
            return crate::tempo::starts_bar(&self.meter, index);
        }
        let into_bar = (index as f64 - self.bar_origin).rem_euclid(self.bar_beats);
        self.bar_beats > 0.0 && (into_bar < 1e-9 || self.bar_beats - into_bar < 1e-9)
    }

    /// Takes the bars off the meter at `beat`, as a scene's tempo or time signature does: the
    /// bar `beat` is in stays as the meter has it, and the bars from here on are counted in that
    /// bar's time signature.
    pub(crate) fn leave_map(&mut self, beat: f64) {
        if !self.follows_map {
            return;
        }
        let here = self.meter.position(beat.max(0.0) + 1e-9);
        self.time_signature = here.time_signature;
        self.bar_beats = here.bar_beats;
        self.bar_origin = here.bar_start;
        self.bars_before = f64::from(here.bar);
        self.follows_map = false;
    }

    /// Puts the bars back on the meter, dropping a scene's signature still waiting for its beat.
    pub(crate) fn follow_map(&mut self) {
        self.follows_map = true;
        self.pending_signature = None;
        self.time_signature = self.project_signature;
        self.bar_beats = self.meter.start_bar_beats();
        self.bar_origin = 0.0;
        self.bars_before = 0.0;
    }

    /// Changes the time signature to `signature` at `at`, the beat a scene's signature waited
    /// for: the bars before it are counted in the old one.
    pub(crate) fn change_signature(&mut self, at: f64, signature: TimeSignature) {
        let bars = ((at - self.bar_origin) / self.bar_beats - 1e-9).ceil().max(0.0);
        self.bars_before += bars;
        self.time_signature = signature;
        self.bar_beats = super::bar_beats(signature);
        self.bar_origin = at;
        self.pending_signature = None;
    }

    /// The bar `beat` is in: its number from 0, the beat it starts at and its time signature,
    /// the map's or the one a scene set.
    pub(crate) fn bar(&self, beat: f64) -> (u32, f64, TimeSignature) {
        let beat = if beat.is_finite() { beat.max(0.0) } else { 0.0 };
        if self.follows_map {
            let here = self.meter.position(beat + 1e-9);
            return (here.bar, here.bar_start, here.time_signature);
        }
        // Signed: a jump back before where the scene set the bars (a locate) is in a bar before
        // it, which starts before the playhead, not at `bar_origin` after it.
        let into = beat - self.bar_origin;
        let bars = if self.bar_beats > 0.0 && into.is_finite() { (into / self.bar_beats + 1e-9).floor() } else { 0.0 };
        let number = (self.bars_before + bars).clamp(0.0, f64::from(u32::MAX)) as u32;
        (number, (self.bar_origin + bars * self.bar_beats).max(0.0), self.time_signature)
    }

    /// The beat a launch issued at `beat` on the grid `quantization` happens at. Bars are
    /// counted from where the time signature last changed; note values from beat 0.
    pub(crate) fn launch_beat(&self, beat: f64, quantization: Quantization) -> f64 {
        if self.follows_map && !self.meter.is_constant() {
            return self.launch_beat_on_meter(beat, quantization);
        }
        let step = quantization.step(self.bar_beats);
        if step == 0.0 {
            return beat;
        }
        let origin = if matches!(quantization, Quantization::Bars(_)) { self.bar_origin } else { 0.0 };
        // A launch exactly on a boundary (within rounding) happens there.
        let index = ((beat - origin - 1e-9) / step).ceil();
        (origin + index * step).max(0.0)
    }

    /// [`LaunchClock::launch_beat`] when the time signature changes: bars are the meter's, and a
    /// note value counts from the start of the bar it is in.
    fn launch_beat_on_meter(&self, beat: f64, quantization: Quantization) -> f64 {
        let at = beat.max(0.0);
        // The bar the beat is in; a beat a rounding error before a bar line counts as on it, and
        // a launch exactly on a boundary happens there.
        let here = self.meter.position(at + 1e-9);
        match quantization {
            Quantization::Bars(bars) if bars > 0 => {
                let next = if (here.bar_start - at).abs() <= 1e-9 { here.bar } else { here.bar.saturating_add(1) };
                self.meter.bar_start(next.div_ceil(bars).saturating_mul(bars))
            }
            Quantization::Beats(step) if step.is_finite() && step > 0.0 => {
                let index = ((at - here.bar_start - 1e-9) / step).ceil().max(0.0);
                // A grid that does not divide the bar ends at the next bar line.
                (here.bar_start + index * step).min(here.bar_start + here.bar_beats)
            }
            _ => beat,
        }
    }

    /// Writes the bars to `monitor`, for the window.
    pub(crate) fn publish(&self, monitor: &LauncherMonitor) {
        monitor.signature.store(super::pack(self.time_signature), Ordering::Relaxed);
        monitor.bar_origin.store(self.bar_origin.to_bits(), Ordering::Relaxed);
        monitor.bars_before.store(self.bars_before.to_bits(), Ordering::Relaxed);
        monitor.follows_map.store(self.follows_map, Ordering::Relaxed);
    }
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;
