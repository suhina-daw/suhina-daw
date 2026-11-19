// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The grain stretch, for the Tonal and Texture modes (`docs/decisions/0003-time-stretching.md`
//! › Amendment: a sound of each mode). The output is made of overlapping grains: every half grain
//! a new one starts, read from the file at its own speed (so its pitch is the file's, times the
//! transpose) from where the clip's time is in the file then, and faded in and out with a Hann
//! window, so two grains always add up to one.
//!
//! - **Tonal** lines each grain up with the one before: its start moves by up to a quarter of a
//!   grain to where its sound best matches how the previous grain's sound goes on, so a held note
//!   carries on without the beating of grains out of phase.
//! - **Texture** moves each grain's start at random by up to `flux` grains, which smears the sound
//!   into a cloud as flux rises; the randomness depends only on the grain's number, so a render
//!   is the same every time and wherever it starts.
//!
//! Nothing here allocates once built.

/// The most grains a frame of output overlaps: two, with a little room.
const RECENT: usize = 4;

/// Grains of a file, as the Tonal or Texture mode plays them.
#[derive(Debug, Clone)]
pub struct Grains {
    channels: usize,
    grain: usize,
    hop: usize,
    window: Vec<f32>,
    /// How far, in grains, a start may move at random (Texture).
    flux: f64,
    /// Whether each start is lined up with the grain before it (Tonal).
    align: bool,
    pitch: f64,
    /// The starts of the last grains worked out, by grain number, so a grain that spans two
    /// renders starts in the same place in both and the next one lines up with it.
    recent: [(u64, f64); RECENT],
}

impl Grains {
    /// Grains of `grain_seconds` at `rate` frames a second, for `channels` channels. Allocates
    /// the window: build it off the audio thread.
    pub fn new(channels: usize, rate: f32, grain_seconds: f32, flux: f32, align: bool) -> Self {
        let grain = ((grain_seconds * rate) as usize).clamp(32, 1 << 16) & !1;
        let window = (0..grain).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / grain as f32).cos()).collect();
        Self {
            channels: channels.max(1),
            grain,
            hop: grain / 2,
            window,
            flux: f64::from(flux.clamp(0.0, 1.0)),
            align,
            pitch: 1.0,
            recent: [(u64::MAX, 0.0); RECENT],
        }
    }

    /// Plays the file `factor` times higher; 1 keeps its pitch.
    pub fn set_pitch(&mut self, factor: f64) {
        self.pitch = if factor.is_finite() && factor > 0.0 { factor } else { 1.0 };
    }

    /// Frames a grain lasts.
    pub fn grain_frames(&self) -> usize {
        self.grain
    }

    /// Forgets the grains worked out: the next render starts afresh (after a seek).
    pub fn reset(&mut self) {
        self.recent = [(u64::MAX, 0.0); RECENT];
    }

    /// Renders output frames `first..` into `out` (interleaved, `channels` a frame): `position`
    /// gives the file frame the clip's time is at for an output frame, and `sample` a file
    /// channel's value at a (fractional) frame, 0 outside the file. Never allocates.
    pub fn render(
        &mut self,
        first: u64,
        out: &mut [f32],
        position: &impl Fn(u64) -> f64,
        sample: &impl Fn(f64, usize) -> f32,
    ) {
        out.fill(0.0);
        let channels = self.channels;
        let frames = (out.len() / channels) as u64;
        if frames == 0 {
            return;
        }
        let (grain, hop) = (self.grain as u64, self.hop as u64);
        let end = first.saturating_add(frames);
        // The grains that sound in first..end: those starting after first - grain.
        let from = if first >= grain { (first - grain) / hop + 1 } else { 0 };
        let to = (end - 1) / hop;
        for k in from..=to {
            let start_out = k * hop;
            let start_in = self.start_of(k, position, sample);
            let (a, b) = (start_out.max(first), start_out.saturating_add(grain).min(end));
            for t in a..b {
                let Some(w) = self.window.get((t - start_out) as usize).copied() else { continue };
                let at = start_in + (t - start_out) as f64 * self.pitch;
                let base = (t - first) as usize * channels;
                for c in 0..channels {
                    if let Some(o) = out.get_mut(base + c) {
                        *o += w * sample(at, c);
                    }
                }
            }
        }
    }

    /// Where in the file grain `k` starts.
    fn start_of(&mut self, k: u64, position: &impl Fn(u64) -> f64, sample: &impl Fn(f64, usize) -> f32) -> f64 {
        if let Some(&(_, start)) = self.recent.iter().find(|(n, _)| *n == k) {
            return start;
        }
        let ideal = position(k * self.hop as u64);
        let start = if self.align {
            match k.checked_sub(1).and_then(|p| self.recent.iter().find(|(n, _)| *n == p)).copied() {
                Some((_, before)) => self.lined_up(ideal, before + self.hop as f64 * self.pitch, sample),
                None => ideal,
            }
        } else if self.flux > 0.0 {
            ideal + (unit(k) - 0.5) * 2.0 * self.flux * self.grain as f64
        } else {
            ideal
        };
        let start = if start.is_finite() { start } else { 0.0 };
        // The oldest of the recent starts makes room.
        let slot = (k % RECENT as u64) as usize;
        if let Some(r) = self.recent.get_mut(slot) {
            *r = (k, start);
        }
        start
    }

    /// The start near `ideal` (within a quarter grain) whose sound best matches the sound from
    /// `going_on`, over the half grain where the two overlap, on the mixed channels.
    fn lined_up(&self, ideal: f64, going_on: f64, sample: &impl Fn(f64, usize) -> f32) -> f64 {
        let reach = (self.grain / 4) as i64;
        let mono = |at: f64| (0..self.channels).map(|c| sample(at, c)).sum::<f32>();
        // How well the sound from `ideal + offset` matches the sound going on, over `points`
        // frames spread across the half grain where the two overlap.
        let overlap = self.hop as f64;
        let score = |offset: i64, points: usize| {
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for p in 0..points {
                let j = overlap * p as f64 / points as f64;
                let a = mono(ideal + offset as f64 + j * self.pitch);
                let b = mono(going_on + j * self.pitch);
                dot += a * b;
                energy += a * a;
            }
            dot / energy.sqrt()
        };
        // The nearest of equal matches, so silence keeps the ideal start.
        let better = |s: f32, offset: i64, best: (f32, i64)| {
            s > best.0 + 1e-6 || (s >= best.0 - 1e-6 && offset.abs() < best.1.abs())
        };
        // Coarse steps over the whole reach, then every frame around the best of them: a few
        // thousand reads a grain, so a voice renders far ahead of real time.
        let coarse = (reach / COARSE_STEPS).max(1);
        let mut best = (f32::NEG_INFINITY, 0i64);
        let mut offset = -reach;
        while offset <= reach {
            let s = score(offset, COARSE_POINTS);
            if better(s, offset, best) {
                best = (s, offset);
            }
            offset += coarse;
        }
        let around = best.1;
        let mut fine = (f32::NEG_INFINITY, around);
        for offset in (around - coarse).max(-reach)..=(around + coarse).min(reach) {
            let s = score(offset, FINE_POINTS);
            if better(s, offset, fine) {
                fine = (s, offset);
            }
        }
        ideal + fine.1 as f64
    }
}

/// The coarse search's steps over a grain's reach, and the frames each match reads; the fine
/// search reads more.
const COARSE_STEPS: i64 = 48;
const COARSE_POINTS: usize = 48;
const FINE_POINTS: usize = 96;

/// A number from 0 to 1 that depends only on `k`.
fn unit(k: u64) -> f64 {
    let mut z = k.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn sine(hz: f64) -> impl Fn(f64, usize) -> f32 {
        move |at: f64, _| {
            if at >= 0.0 { (std::f64::consts::TAU * hz * at / f64::from(RATE)).sin() as f32 * 0.5 } else { 0.0 }
        }
    }

    fn crossings_hz(out: &[f32]) -> f64 {
        let n = out.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        n as f64 / 2.0 / (out.len() as f64 / f64::from(RATE))
    }

    fn rms(out: &[f32]) -> f32 {
        (out.iter().map(|s| s * s).sum::<f32>() / out.len().max(1) as f32).sqrt()
    }

    /// Played at half speed, a 440 Hz tone keeps its pitch and level, in both modes.
    #[test]
    fn half_speed_keeps_the_pitch_and_the_level() {
        for (flux, align) in [(0.0, true), (0.3, false)] {
            let mut grains = Grains::new(1, RATE, 0.06, flux, align);
            let mut out = vec![0.0; 96_000];
            for (i, chunk) in out.chunks_mut(1000).enumerate() {
                grains.render((i * 1000) as u64, chunk, &|t| t as f64 * 0.5, &sine(440.0));
            }
            let middle = &out[24_000..72_000];
            let hz = crossings_hz(middle);
            assert!((hz - 440.0).abs() < 440.0 * 0.03, "align {align}: {hz} Hz");
            if align {
                // Lined up, the grains add up to the tone's level without beating.
                assert!((rms(middle) - 0.354).abs() < 0.03, "{}", rms(middle));
            }
        }
    }

    #[test]
    fn the_transpose_moves_the_pitch_not_the_time() {
        let mut grains = Grains::new(1, RATE, 0.05, 0.0, true);
        grains.set_pitch(2.0);
        let mut out = vec![0.0; 48_000];
        grains.render(0, &mut out, &|t| t as f64, &sine(220.0));
        let hz = crossings_hz(&out[12_000..36_000]);
        assert!((hz - 440.0).abs() < 440.0 * 0.03, "{hz} Hz");
    }

    /// A render is the same whether it is made at once or in pieces.
    #[test]
    fn rendering_in_pieces_gives_the_same_frames() {
        let source = sine(330.0);
        let position = |t: u64| t as f64 * 0.7;
        for (flux, align) in [(0.0, true), (0.5, false)] {
            let mut whole = vec![0.0; 2 * 10_000];
            Grains::new(2, RATE, 0.04, flux, align).render(0, &mut whole, &position, &source);
            let mut pieces = vec![0.0; 2 * 10_000];
            let mut grains = Grains::new(2, RATE, 0.04, flux, align);
            for (i, chunk) in pieces.chunks_mut(2 * 777).enumerate() {
                grains.render((i * 777) as u64, chunk, &position, &source);
            }
            assert_eq!(whole, pieces);
        }
    }

    #[test]
    fn odd_settings_stay_in_range() {
        let grains = Grains::new(0, RATE, -1.0, f32::NAN, false);
        assert!(grains.grain_frames() >= 32);
        let grains = Grains::new(1, RATE, 1e9, 9.0, false);
        assert!(grains.grain_frames() <= 1 << 16);
        let mut silent = Grains::new(1, RATE, 0.05, 0.5, false);
        let mut out = vec![1.0; 100];
        silent.render(u64::MAX - 1_000, &mut out, &|_| f64::NAN, &|_, _| 0.0);
        assert!(out.iter().all(|s| *s == 0.0));
    }
}
