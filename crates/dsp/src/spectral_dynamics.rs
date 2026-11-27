// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Spectral dynamics: an equaliser band that moves only the frequencies inside it that stand out
//! (docs/spec/eq.md › Dynamic and Spectral bands › Spectral). The stereo signal runs through a
//! short-time Fourier transform (a square-root Hann window, an eighth-frame hop, overlap-add). For
//! each band, every bin's level is compared with a reference made of the spectrum around it (the
//! mean level of a neighbourhood on each side, the bin itself left out), so a resonance that rises
//! above its neighbours is turned down (or, with a positive range, up) and the rest of the band is
//! left alone; each cut is spread over the bins next to it and follows the band's attack and
//! release, faster in the highs than in the lows. The output is the input delayed by [`FRAME`]
//! frames where nothing moves.
//!
//! `new` allocates (the plans, the buffers); everything else runs on the audio thread without
//! allocating, locking or panicking.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::band_dynamics::gain_change;

/// The transform's length in frames: the delay it adds (about 43 ms at 48 kHz).
pub const FRAME: usize = 2048;
/// The hop between two transforms: an eighth of the frame.
pub const HOP: usize = FRAME / 8;
/// The bins of one transform, up to and with Nyquist.
pub const BINS: usize = FRAME / 2 + 1;
/// How many bands the block holds.
pub const BANDS: usize = 24;
/// The points the live cut is read at, spread logarithmically over 10 Hz – 22 kHz.
pub const LIVE_POINTS: usize = 100;
/// The live points' frequency range.
pub const LIVE_RANGE: (f64, f64) = (10.0, 22_000.0);
/// How far above its reference a bin must rise, in dB, before it counts as standing out.
const STANDS_OUT_DB: f64 = 12.0;
/// Where the two sides of a bin's neighbourhood differ by more than this, in dB (a cliff in the
/// spectrum), the reference is the louder side less this, so a slope never reads as a peak.
const CLIFF_DB: f64 = 12.0;
/// Bins more than this far below the frame's loudest bin, in dB, count as being at that floor:
/// silence between tones never makes a quiet tone stand out.
const FLOOR_DB: f64 = 40.0;
/// The bins next to a bin that its reference leaves out, each side (a tone's main lobe).
const GAP_BINS: usize = 2;
/// The power mean that spreads a cut over its neighbours: high, so the deepest cut nearby decides.
const SPREAD_POWER: i32 = 8;
/// How the attack and release scale with frequency: `(1 kHz / f)^this`, so a bin at 10 kHz moves
/// about three times as fast as one at 1 kHz.
const TIME_EXPONENT: f64 = 0.5;

/// A band's shape, which sets how much of each bin belongs to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Bell,
    LowShelf,
    HighShelf,
}

/// Where a band works in the stereo image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Stereo,
    Left,
    Right,
    Mid,
    Side,
}

/// One band's settings, set from its parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub on: bool,
    pub shape: Shape,
    pub frequency: f64,
    pub q: f64,
    /// The most a bin may move, in dB (negative turns down).
    pub range: f64,
    /// The level a bin must pass, in dB below full scale.
    pub threshold: f64,
    /// Seconds, at 1 kHz (a bin's own times scale with its frequency).
    pub attack: f64,
    pub release: f64,
    /// 0 to 1: how selective the detector is; low compares each bin with a wide neighbourhood and
    /// spreads its cut wide (broad humps move), high with a narrower one and cuts narrow notches.
    pub density: f64,
    /// Weights the detector by 3 dB per octave around 1 kHz, so the highs trigger a little more.
    pub tilt: bool,
    pub place: Place,
    /// Whether the detector hears the key instead of the band's own signal.
    pub keyed: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            on: false,
            shape: Shape::Bell,
            frequency: 1000.0,
            q: 0.71,
            range: -6.0,
            threshold: -40.0,
            attack: 0.01,
            release: 0.12,
            density: 0.5,
            tilt: true,
            place: Place::Stereo,
            keyed: false,
        }
    }
}

/// One band's state: its settings and every bin's gain change now, in dB.
struct Band {
    settings: Settings,
    gains: Vec<f32>,
}

/// The block: up to [`BANDS`] spectral bands over one stereo signal.
pub struct SpectralDynamics {
    sample_rate: f64,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    window: Vec<f32>,
    /// The window's sum, which turns a bin's magnitude into a sine's amplitude.
    window_sum: f64,
    /// The last [`FRAME`] input frames of each side, and of the key, as rings.
    input: [Vec<f32>; 2],
    key: [Vec<f32>; 2],
    /// The overlap-added output of each side, read [`FRAME`] frames later.
    output: [Vec<f32>; 2],
    at: usize,
    since_hop: usize,
    spectra: [Vec<Complex<f32>>; 2],
    key_spectra: [Vec<Complex<f32>>; 2],
    levels: Vec<f64>,
    sums: Vec<f64>,
    /// Each bin's cut before spreading, and the running sums of its power mean.
    cuts: Vec<f64>,
    cut_sums: Vec<f64>,
    bands: Vec<Band>,
    /// Each bin's frequency.
    frequencies: Vec<f64>,
}

impl SpectralDynamics {
    /// Every plan and buffer for `sample_rate`. Allocates.
    pub fn new(sample_rate: f64) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(FRAME);
        let inverse = planner.plan_fft_inverse(FRAME);
        let scratch_len = forward.get_inplace_scratch_len().max(inverse.get_inplace_scratch_len());
        let window: Vec<f32> = (0..FRAME)
            .map(|n| (0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / FRAME as f64).cos()).sqrt() as f32)
            .collect();
        let window_sum = window.iter().map(|w| f64::from(*w)).sum();
        let zeros = || vec![0.0f32; FRAME];
        let spectrum = || vec![Complex::new(0.0f32, 0.0); FRAME];
        Self {
            sample_rate,
            forward,
            inverse,
            scratch: vec![Complex::new(0.0, 0.0); scratch_len],
            window,
            window_sum,
            input: [zeros(), zeros()],
            key: [zeros(), zeros()],
            output: [zeros(), zeros()],
            at: 0,
            since_hop: 0,
            spectra: [spectrum(), spectrum()],
            key_spectra: [spectrum(), spectrum()],
            levels: vec![0.0; BINS],
            sums: vec![0.0; BINS + 1],
            cuts: vec![0.0; BINS],
            cut_sums: vec![0.0; BINS + 1],
            bands: (0..BANDS).map(|_| Band { settings: Settings::default(), gains: vec![0.0; BINS] }).collect(),
            frequencies: (0..BINS).map(|k| k as f64 * sample_rate / FRAME as f64).collect(),
        }
    }

    /// The delay the block adds, in frames.
    pub const fn latency() -> usize {
        FRAME
    }

    /// Band `band`'s settings.
    pub fn set(&mut self, band: usize, settings: Settings) {
        if let Some(b) = self.bands.get_mut(band) {
            if !settings.on && b.settings.on {
                b.gains.iter_mut().for_each(|g| *g = 0.0);
            }
            b.settings = settings;
        }
    }

    /// Back to silence: the rings, the overlap and every gain change cleared.
    pub fn reset(&mut self) {
        for ring in self.input.iter_mut().chain(self.key.iter_mut()).chain(self.output.iter_mut()) {
            ring.iter_mut().for_each(|x| *x = 0.0);
        }
        for band in &mut self.bands {
            band.gains.iter_mut().for_each(|g| *g = 0.0);
        }
        self.at = 0;
        self.since_hop = 0;
    }

    /// Band `band`'s gain change now at each of the [`LIVE_POINTS`] frequencies, in dB.
    pub fn live(&self, band: usize) -> [f32; LIVE_POINTS] {
        let Some(b) = self.bands.get(band) else { return [0.0; LIVE_POINTS] };
        std::array::from_fn(|i| {
            let f = LIVE_RANGE.0 * (LIVE_RANGE.1 / LIVE_RANGE.0).powf(i as f64 / (LIVE_POINTS - 1) as f64);
            let bin = (f * FRAME as f64 / self.sample_rate).round() as usize;
            b.gains.get(bin.min(BINS - 1)).copied().unwrap_or(0.0)
        })
    }

    /// One frame: `input` in, the processed frame [`FRAME`] frames earlier out. `key` is the
    /// key's frame, when the device has one.
    pub fn process(&mut self, input: [f64; 2], key: Option<[f64; 2]>) -> [f64; 2] {
        let at = self.at;
        let mut out = [0.0f64; 2];
        for side in 0..2 {
            if let Some(cell) = self.output[side].get_mut(at) {
                out[side] = f64::from(*cell);
                *cell = 0.0;
            }
            if let Some(cell) = self.input[side].get_mut(at) {
                *cell = input[side] as f32;
            }
            if let Some(cell) = self.key[side].get_mut(at) {
                *cell = key.map_or(0.0, |k| k[side] as f32);
            }
        }
        self.at = (at + 1) % FRAME;
        self.since_hop += 1;
        if self.since_hop >= HOP {
            self.since_hop = 0;
            self.hop(key.is_some());
        }
        out
    }

    /// One transform: analyse the last frame, move each band's bins, write the frame back.
    fn hop(&mut self, keyed: bool) {
        let start = self.at;
        for side in 0..2 {
            for n in 0..FRAME {
                let ring = (start + n) % FRAME;
                let w = self.window.get(n).copied().unwrap_or(0.0);
                let x = self.input[side].get(ring).copied().unwrap_or(0.0);
                let k = self.key[side].get(ring).copied().unwrap_or(0.0);
                if let Some(cell) = self.spectra[side].get_mut(n) {
                    *cell = Complex::new(x * w, 0.0);
                }
                if let Some(cell) = self.key_spectra[side].get_mut(n) {
                    *cell = Complex::new(k * w, 0.0);
                }
            }
            self.forward.process_with_scratch(&mut self.spectra[side], &mut self.scratch);
            if keyed {
                self.forward.process_with_scratch(&mut self.key_spectra[side], &mut self.scratch);
            }
        }
        let hop_seconds = HOP as f64 / self.sample_rate;
        // Every band decides from the input as it came in, then all are applied.
        for index in 0..self.bands.len() {
            let Some(settings) = self.bands.get(index).map(|b| b.settings) else { continue };
            if !settings.on || settings.range.abs() < 1e-6 {
                continue;
            }
            self.detect(index, settings, keyed, hop_seconds);
        }
        self.apply();
        // The inverse transform, windowed again and overlap-added (the square-root Hann windows'
        // product, at an eighth-frame hop, sums to 4; the transform is unnormalised).
        let scale = 0.25 / FRAME as f32;
        for side in 0..2 {
            self.inverse.process_with_scratch(&mut self.spectra[side], &mut self.scratch);
            for n in 0..FRAME {
                let w = self.window.get(n).copied().unwrap_or(0.0);
                let y = self.spectra[side].get(n).map_or(0.0, |c| c.re) * w * scale;
                if let Some(cell) = self.output[side].get_mut((start + n) % FRAME) {
                    *cell += y;
                }
            }
        }
    }

    /// One band's gains: each bin's level (of the band's part of the signal, or of the key), held
    /// at the frame's floor; its reference from the neighbourhood on each side; the soft-knee cut
    /// of what passes both the reference and the band's threshold; spread over its neighbours,
    /// weighted by the band's shape and smoothed in time and across bins.
    fn detect(&mut self, index: usize, settings: Settings, keyed: bool, hop_seconds: f64) {
        let sources = if settings.keyed && keyed { &self.key_spectra } else { &self.spectra };
        let window_sum = self.window_sum;
        let mut loudest = f64::MIN;
        for k in 0..BINS {
            let (l, r) =
                (sources[0].get(k).copied().unwrap_or_default(), sources[1].get(k).copied().unwrap_or_default());
            let magnitude = match settings.place {
                Place::Stereo => l.norm().max(r.norm()),
                Place::Left => l.norm(),
                Place::Right => r.norm(),
                Place::Mid => ((l + r) * 0.5).norm(),
                Place::Side => ((l - r) * 0.5).norm(),
            };
            let f = self.frequencies.get(k).copied().unwrap_or(1.0).max(1.0);
            let tilt = if settings.tilt { 3.0 * (f / 1000.0).log2() } else { 0.0 };
            let level = 20.0 * (2.0 * f64::from(magnitude) / window_sum).max(1e-9).log10() + tilt;
            if k > 0 {
                loudest = loudest.max(level);
            }
            if let Some(cell) = self.levels.get_mut(k) {
                *cell = level;
            }
        }
        let floor = loudest - FLOOR_DB;
        // Running sums of the floored levels, so a neighbourhood's mean is one subtraction.
        let mut total = 0.0;
        if let Some(first) = self.sums.get_mut(0) {
            *first = 0.0;
        }
        for k in 0..BINS {
            if let Some(level) = self.levels.get_mut(k) {
                *level = level.max(floor);
                total += *level;
            }
            if let Some(cell) = self.sums.get_mut(k + 1) {
                *cell = total;
            }
        }
        // Density 0 – 1: the neighbourhood is 4.5 octaves wide in all at 0 and 2.6 at 1; the cut
        // spreads over an octave at 0 and a twentieth of one at 1.
        let density = settings.density.clamp(0.0, 1.0);
        let reach = 2f64.powf(4.5 * 2f64.powf(-density * 10.0 / 12.3) / 2.0);
        let spread = 2f64.powf(2f64.powf(-density * 10.0 / 2.3) / 2.0);
        // The cut each bin asks for now, before spreading.
        for k in 0..BINS {
            let f = self.frequencies.get(k).copied().unwrap_or(0.0);
            let level = self.levels.get(k).copied().unwrap_or(floor);
            let cut = if k == 0 || shape_weight(settings.shape, f, settings.frequency, settings.q) < 0.01 {
                0.0
            } else {
                let below_lo = ((k as f64 / reach).floor() as usize).min(k);
                let below = mean(&self.sums, below_lo, k.saturating_sub(GAP_BINS));
                let above_hi = ((k as f64 * reach).ceil() as usize).min(BINS);
                let above = mean(&self.sums, (k + GAP_BINS + 1).min(BINS), above_hi);
                let reference = match (below, above) {
                    (Some(b), Some(a)) => (0.5 * (a + b)).max(a.max(b) - CLIFF_DB),
                    (Some(one), None) | (None, Some(one)) => one,
                    (None, None) => level,
                };
                let threshold = settings.threshold.max(reference + STANDS_OUT_DB);
                gain_change(level, threshold, settings.range).abs()
            };
            if let Some(cell) = self.cuts.get_mut(k) {
                *cell = cut;
            }
        }
        // Running sums of the cuts' powers, for the power mean that spreads them.
        let mut total = 0.0;
        if let Some(first) = self.cut_sums.get_mut(0) {
            *first = 0.0;
        }
        for k in 0..BINS {
            total += self.cuts.get(k).copied().unwrap_or(0.0).powi(SPREAD_POWER);
            if let Some(cell) = self.cut_sums.get_mut(k + 1) {
                *cell = total;
            }
        }
        let sign = settings.range.signum();
        let most = settings.range.abs();
        let Some(band) = self.bands.get_mut(index) else { return };
        for k in 1..BINS {
            let f = self.frequencies.get(k).copied().unwrap_or(0.0);
            let weight = shape_weight(settings.shape, f, settings.frequency, settings.q);
            let lo = ((k as f64 / spread).floor() as usize).min(k - 1);
            let hi = ((k as f64 * spread).ceil() as usize + 1).clamp(k + 1, BINS);
            let spread_cut =
                mean(&self.cut_sums, lo, hi).map_or(0.0, |m| m.max(0.0).powf(1.0 / f64::from(SPREAD_POWER)));
            let target = (sign * spread_cut.min(most) * weight) as f32;
            let attack = (-hop_seconds / bin_time(settings.attack, f).max(1e-4)).exp() as f32;
            let release = (-hop_seconds / bin_time(settings.release, f).max(1e-3)).exp() as f32;
            if let Some(gain) = band.gains.get_mut(k) {
                let keep = if target.abs() > gain.abs() { attack } else { release };
                *gain = target + (*gain - target) * keep;
            }
        }
        smooth_across(&mut band.gains);
    }

    /// Every band's gains on the spectra: the left/right bands first, then the mid/side ones.
    fn apply(&mut self) {
        let gain = |db: f32| 10f32.powf(db / 20.0);
        let [left, right] = &mut self.spectra;
        for band in self.bands.iter().filter(|b| b.settings.on) {
            match band.settings.place {
                Place::Stereo | Place::Left | Place::Right => {
                    for (k, g) in band.gains.iter().enumerate() {
                        if g.abs() < 1e-3 {
                            continue;
                        }
                        let (to_left, to_right) = match band.settings.place {
                            Place::Left => (true, false),
                            Place::Right => (false, true),
                            _ => (true, true),
                        };
                        if to_left && let Some(c) = left.get_mut(k) {
                            *c *= gain(*g);
                        }
                        if to_right && let Some(c) = right.get_mut(k) {
                            *c *= gain(*g);
                        }
                    }
                }
                Place::Mid | Place::Side => {
                    for (k, g) in band.gains.iter().enumerate() {
                        if g.abs() < 1e-3 {
                            continue;
                        }
                        let (Some(l), Some(r)) = (left.get(k).copied(), right.get(k).copied()) else { continue };
                        let (mut m, mut s) = ((l + r) * 0.5, (l - r) * 0.5);
                        if band.settings.place == Place::Mid {
                            m *= gain(*g);
                        } else {
                            s *= gain(*g);
                        }
                        if let Some(c) = left.get_mut(k) {
                            *c = m + s;
                        }
                        if let Some(c) = right.get_mut(k) {
                            *c = m - s;
                        }
                    }
                }
            }
        }
        // The upper half mirrors the lower, so the inverse transform stays real.
        for spectrum in [left, right] {
            for k in 1..FRAME / 2 {
                if let Some(c) = spectrum.get(k).copied()
                    && let Some(mirror) = spectrum.get_mut(FRAME - k)
                {
                    *mirror = c.conj();
                }
            }
        }
    }
}

/// A bin's attack or release at `f`, from the band's time at 1 kHz: faster in the highs, slower in
/// the lows (below 40 Hz it stays at 40 Hz's).
fn bin_time(seconds: f64, f: f64) -> f64 {
    seconds * (1000.0 / f.max(40.0)).powf(TIME_EXPONENT)
}

/// The mean of the values whose running sums are `sums`, over `lo..hi`; none for an empty range.
fn mean(sums: &[f64], lo: usize, hi: usize) -> Option<f64> {
    (hi > lo).then(|| (sums.get(hi).copied().unwrap_or(0.0) - sums.get(lo).copied().unwrap_or(0.0)) / (hi - lo) as f64)
}

/// A light (1, 2, 1) / 4 smoothing across bins, so a notch has no hard edges.
fn smooth_across(gains: &mut [f32]) {
    let mut previous = gains.first().copied().unwrap_or(0.0);
    for k in 1..gains.len().saturating_sub(1) {
        let (Some(here), Some(next)) = (gains.get(k).copied(), gains.get(k + 1).copied()) else { continue };
        if let Some(cell) = gains.get_mut(k) {
            *cell = 0.25 * previous + 0.5 * here + 0.25 * next;
        }
        previous = here;
    }
}

/// How much of a bin at `f` belongs to a band of `shape` at `center` and `q`, 0 to 1: a band
/// pass's power for a bell, a 4th-order low or high pass's for a shelf.
pub fn shape_weight(shape: Shape, f: f64, center: f64, q: f64) -> f64 {
    let ratio = (f / center.max(1.0)).max(1e-9);
    match shape {
        Shape::Bell => {
            let x = q.max(0.1) * (ratio - 1.0 / ratio);
            1.0 / (1.0 + x * x)
        }
        Shape::LowShelf => 1.0 / (1.0 + ratio.powi(4)),
        Shape::HighShelf => 1.0 / (1.0 + ratio.powi(-4)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn tone(hz: f64, amplitude: f64, n: usize) -> f64 {
        amplitude * (std::f64::consts::TAU * hz * n as f64 / RATE).sin()
    }

    /// The amplitude of `hz` in the second half of `signal`.
    fn level(signal: &[f64], hz: f64) -> f64 {
        let half = &signal[signal.len() / 2..];
        let (mut re, mut im) = (0.0, 0.0);
        for (n, x) in half.iter().enumerate() {
            let w = std::f64::consts::TAU * hz * (n + signal.len() / 2) as f64 / RATE;
            re += x * w.cos();
            im += x * w.sin();
        }
        2.0 * re.hypot(im) / half.len() as f64
    }

    fn run(block: &mut SpectralDynamics, frames: usize, f: impl Fn(usize) -> f64) -> Vec<f64> {
        (0..frames)
            .map(|n| {
                let x = f(n);
                block.process([x, x], None)[0]
            })
            .collect()
    }

    #[test]
    fn with_nothing_moving_it_is_a_pure_delay() {
        let mut block = SpectralDynamics::new(RATE);
        let input: Vec<f64> = (0..20_000).map(|n| tone(440.0, 0.3, n) + tone(3_000.0, 0.1, n)).collect();
        let output: Vec<f64> = input.iter().map(|&x| block.process([x, x], None)[0]).collect();
        let worst = (FRAME * 2..input.len()).map(|n| (output[n] - input[n - FRAME]).abs()).fold(0.0, f64::max);
        assert!(worst < 1e-4, "the output is the input FRAME frames later: {worst}");
    }

    #[test]
    fn a_resonance_inside_the_band_is_cut_and_its_neighbours_are_not() {
        let mut block = SpectralDynamics::new(RATE);
        block.set(
            0,
            Settings {
                on: true,
                frequency: 2_000.0,
                q: 0.5,
                range: -12.0,
                threshold: -60.0,
                density: 0.9,
                tilt: false,
                ..Settings::default()
            },
        );
        // Quiet noise-like bed of many tones, one loud resonance at 2.2 kHz.
        let bed = |n: usize| (1..40).map(|i| tone(200.0 * i as f64 + 37.0, 0.004, n)).sum::<f64>();
        let out = run(&mut block, 96_000, |n| bed(n) + tone(2_200.0, 0.2, n));
        let resonance = 20.0 * (level(&out, 2_200.0) / 0.2).log10();
        assert!(resonance < -6.0, "the resonance is cut: {resonance} dB");
        let neighbour = 20.0 * (level(&out, 1_637.0) / 0.004).log10();
        assert!(neighbour > -2.0, "a quiet neighbour in the band is left alone: {neighbour} dB");
        assert!(block.live(0).iter().any(|g| *g < -6.0), "the live cut shows it");
    }

    #[test]
    fn a_band_off_or_with_no_range_moves_nothing() {
        let mut block = SpectralDynamics::new(RATE);
        block.set(0, Settings { on: true, range: 0.0, threshold: -80.0, ..Settings::default() });
        let out = run(&mut block, 48_000, |n| tone(1_000.0, 0.5, n));
        assert!((level(&out, 1_000.0) / 0.5 - 1.0).abs() < 0.01);
        assert!(block.live(0).iter().all(|g| *g == 0.0));
    }

    #[test]
    fn low_density_spreads_a_cut_wider_than_high_density() {
        let width = |density: f64| {
            let mut block = SpectralDynamics::new(RATE);
            block.set(
                0,
                Settings {
                    on: true,
                    frequency: 2_000.0,
                    q: 0.3,
                    range: -12.0,
                    threshold: -60.0,
                    density,
                    tilt: false,
                    ..Settings::default()
                },
            );
            let bed = |n: usize| (1..40).map(|i| tone(200.0 * i as f64 + 37.0, 0.004, n)).sum::<f64>();
            run(&mut block, 48_000, |n| bed(n) + tone(2_200.0, 0.2, n));
            block.live(0).iter().filter(|g| **g < -1.0).count()
        };
        let (wide, narrow) = (width(0.1), width(0.9));
        assert!(narrow >= 1, "a narrow cut still shows");
        assert!(wide > narrow, "low density spreads the cut: {wide} points against {narrow}");
    }

    #[test]
    fn a_bins_times_are_faster_in_the_highs() {
        assert!((bin_time(0.01, 1_000.0) - 0.01).abs() < 1e-12, "the band's time is its time at 1 kHz");
        assert!(bin_time(0.01, 10_000.0) < 0.004, "about three times as fast at 10 kHz");
        assert!(bin_time(0.01, 100.0) > 0.03, "about three times as slow at 100 Hz");
        assert_eq!(bin_time(0.01, 10.0), bin_time(0.01, 40.0), "held below 40 Hz");
    }

    #[test]
    fn a_smoothed_notch_keeps_its_depth_in_the_middle_and_softens_its_edges() {
        let mut gains = [0.0f32, 0.0, -12.0, -12.0, -12.0, 0.0, 0.0];
        smooth_across(&mut gains);
        assert_eq!(gains[3], -12.0);
        assert_eq!(gains[2], -9.0);
        assert_eq!(gains[1], -3.0);
        assert_eq!(gains[0], 0.0, "the ends are left as they are");
    }

    #[test]
    fn shapes_weigh_their_own_bins() {
        assert!((shape_weight(Shape::Bell, 1_000.0, 1_000.0, 1.0) - 1.0).abs() < 1e-9);
        assert!(shape_weight(Shape::Bell, 4_000.0, 1_000.0, 2.0) < 0.05);
        assert!(shape_weight(Shape::LowShelf, 100.0, 1_000.0, 1.0) > 0.99);
        assert!(shape_weight(Shape::HighShelf, 100.0, 1_000.0, 1.0) < 0.01);
    }
}
