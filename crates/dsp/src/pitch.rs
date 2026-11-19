// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded YIN frame estimation, shared by sample-map features and instrument analysis.
//! Construction allocates working buffers; estimation only reuses them. The caller chooses
//! activity/DC handling. The periodicity measure is not a probability of correct pitch.

/// A measured periodic component of one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pitch {
    pub hz: f32,
    /// One minus the normalized difference at the chosen period, bounded to 0..1.
    pub periodicity: f32,
}

impl Pitch {
    pub fn midi_note(self) -> f32 {
        69.0 + 12.0 * (self.hz / 440.0).log2()
    }
}

/// Prepared estimator. At most 8,192 frame samples and 2,048 lag candidates are admitted.
pub struct Yin {
    rate: f32,
    frame_len: usize,
    tau_min: usize,
    tau_max: usize,
    threshold: f32,
    difference: Vec<f32>,
    normalized: Vec<f32>,
}

impl Yin {
    /// Rejects invalid/oversized configurations before allocating.
    pub fn new(rate: f32, frame_len: usize, min_hz: f32, max_hz: f32, threshold: f32) -> Option<Self> {
        if !rate.is_finite()
            || !(1_000.0..=384_000.0).contains(&rate)
            || !(8..=8192).contains(&frame_len)
            || !min_hz.is_finite()
            || !max_hz.is_finite()
            || !(min_hz > 0.0 && min_hz < max_hz && max_hz < rate * 0.5)
            || !threshold.is_finite()
            || !(0.0..1.0).contains(&threshold)
        {
            return None;
        }
        let tau_min = (rate / max_hz) as usize;
        let tau_max = (rate / min_hz) as usize;
        if tau_max <= tau_min.max(2) || tau_max >= frame_len || tau_max > 2048 {
            return None;
        }
        Some(Self {
            rate,
            frame_len,
            tau_min,
            tau_max,
            threshold,
            difference: vec![0.0; tau_max + 1],
            normalized: vec![1.0; tau_max + 1],
        })
    }

    pub fn estimate(&mut self, frame: &[f32]) -> Option<Pitch> {
        self.estimate_while(frame, || true)
    }

    /// Checks cancellation between individual lags, never inside the audio callback.
    /// `None` means no estimate (including rejected input or cancellation).
    pub fn estimate_while(&mut self, frame: &[f32], mut keep_going: impl FnMut() -> bool) -> Option<Pitch> {
        if frame.len() != self.frame_len || frame.iter().any(|x| !x.is_finite()) {
            return None;
        }
        let width = self.frame_len.saturating_sub(self.tau_max);
        for (tau, slot) in self.difference.iter_mut().enumerate() {
            if !keep_going() {
                return None;
            }
            *slot = frame.iter().take(width).zip(frame.iter().skip(tau)).map(|(a, b)| (a - b) * (a - b)).sum();
            if !slot.is_finite() {
                return None;
            }
        }
        let mut running = 0.0;
        for tau in 1..=self.tau_max {
            let d = self.difference.get(tau).copied().unwrap_or(0.0);
            running += d;
            if !running.is_finite() {
                return None;
            }
            if let Some(slot) = self.normalized.get_mut(tau) {
                *slot = if running > 0.0 { d * tau as f32 / running } else { 1.0 };
            }
        }
        let value = |tau: usize| self.normalized.get(tau).copied().unwrap_or(1.0);
        let mut tau = self.tau_min.max(2);
        let mut found = None;
        while tau < self.tau_max {
            if value(tau) < self.threshold {
                while tau + 1 < self.tau_max && value(tau + 1) < value(tau) {
                    tau += 1;
                }
                found = Some(tau);
                break;
            }
            tau += 1;
        }
        let tau = found?;
        let (a, b, c) = (value(tau - 1), value(tau), value(tau + 1));
        let denominator = a - 2.0 * b + c;
        let shift = if denominator.abs() > 1e-9 { 0.5 * (a - c) / denominator } else { 0.0 };
        let hz = self.rate / (tau as f32 + shift.clamp(-1.0, 1.0));
        (hz.is_finite() && hz > 0.0).then_some(Pitch { hz, periodicity: (1.0 - b).clamp(0.0, 1.0) })
    }
}
