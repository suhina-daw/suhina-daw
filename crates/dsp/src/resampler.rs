// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Converting a signal from one sample rate to another with a windowed-sinc polyphase
//! resampler: a fixed table of the filter's phases, a history of the last taps and the output's
//! exact position in input time. One channel; a device keeps one per side and per leg (into the
//! model's rate, and back).
//!
//! The output is the input rebuilt at the other rate. A device that resamples holds the signal
//! back once while going in and once while coming out ([`HALF_TAPS`] output frames each side),
//! which it reports to the engine's latency compensation.

/// Taps of the windowed-sinc kernel. Even, so the middle sits between the two centre taps.
const TAPS: usize = 48;
/// Half the taps: how far ahead of an output's time the kernel reaches, in input frames.
/// This is the delay the conversion adds, in frames of the resampler's output rate.
pub const HALF_TAPS: usize = TAPS / 2;
/// The phases of the table; an output's fractional position picks the nearest of them.
const PHASES: usize = 64;

/// Converts one channel's signal from `from` Hz to `to` Hz.
#[derive(Debug, Clone, PartialEq)]
pub struct Resampler {
    from: f64,
    to: f64,
    /// `PHASES` rows of `TAPS` taps: `table[phase * TAPS + tap]`.
    table: Box<[f64]>,
    /// The last `TAPS` input frames, oldest first, newest last.
    history: Box<[f64]>,
    /// How many input frames were fed in total.
    fed: usize,
    /// How many output frames were produced in total; the next output's input time is
    /// `produced * from / to` input frames from the stream's start.
    produced: f64,
}

impl Resampler {
    /// A resampler from `from` Hz to `to` Hz (a rate that is zero, negative or not finite is
    /// 48 kHz). Building the table allocates: run it off the audio thread, as a device's
    /// `prepare` runs. The fed and produced counts carry across calls, so the output never
    /// depends on how a stream is divided into calls.
    pub fn new(from: f64, to: f64) -> Self {
        let rate = |r: f64| if r.is_finite() && r > 0.0 { r } else { 48_000.0 };
        let (from, to) = (rate(from), rate(to));
        let mut table = vec![0.0; PHASES * TAPS].into_boxed_slice();
        // The cutoff keeps whatever the lower of the two rates can carry, in cycles per input
        // sample: the input's full range when upsampling, the target rate's Nyquist when
        // downsampling.
        let cutoff = 0.5f64.min(to / from);
        let width = HALF_TAPS as f64;
        for phase in 0..PHASES {
            let mut row = [0.0; TAPS];
            let offset = phase as f64 / PHASES as f64 - 0.5;
            for (tap, value) in row.iter_mut().enumerate() {
                // The tap sits `u` input frames before (negative) or after the output's time.
                let u = tap as f64 - (HALF_TAPS as f64 - 1.0) + offset;
                let window = if u.abs() < width { 0.5 * (1.0 + (std::f64::consts::PI * u / width).cos()) } else { 0.0 };
                let argument = std::f64::consts::PI * 2.0 * cutoff * u;
                let sinc = if argument.abs() < 1e-9 { 1.0 } else { argument.sin() / argument };
                *value = 2.0 * cutoff * sinc * window;
            }
            // The row's sum is the gain a constant signal keeps: one, whatever the phase.
            let sum: f64 = row.iter().sum();
            for (slot, value) in table[phase * TAPS..(phase + 1) * TAPS].iter_mut().zip(row) {
                *slot = if sum.abs() > 1e-12 { value / sum } else { 0.0 };
            }
        }
        Self { from, to, table, history: vec![0.0; TAPS].into_boxed_slice(), fed: 0, produced: 0.0 }
    }

    /// Whether both rates are the same: a device leaves such a stream alone.
    pub fn passthrough(&self) -> bool {
        self.from == self.to
    }

    /// How many frames at the output's rate the conversion trails the input: the half kernel
    /// an output has to wait for.
    pub fn delay_frames(&self) -> f64 {
        HALF_TAPS as f64
    }

    /// Feeds `input` in and writes what the conversion gives so far into `output`, as much as
    /// fits; returns how many frames were written. Feed a slice fully before the next: an
    /// unfed-and-unread remainder is only what the output slice lacked room for.
    pub fn process(&mut self, input: &[f64], output: &mut [f64]) -> usize {
        let mut written = 0usize;
        for &x in input {
            self.history.copy_within(1.., 0);
            // A frame that is not a number or not finite is silence, so bad input cannot bleed
            // into the history and the output for good.
            self.history[TAPS - 1] = if x.is_finite() { x } else { 0.0 };
            self.fed += 1;
            // The kernel reaches `HALF_TAPS` frames behind and `HALF_TAPS` ahead of its
            // centre, and the history holds `TAPS` frames, so the output whose centre sits at
            // `fed − HALF_TAPS − 1` reads exactly what is stored: produce it now, before the
            // next frame pushes it out. Outputs ahead of that centre wait here for the rest of
            // the feed.
            let bound = self.fed as f64 - (HALF_TAPS + 1) as f64; // centre < bound
            while written < output.len() {
                let t = self.produced * self.from / self.to;
                let centre = (t + 0.5).floor();
                if centre >= bound {
                    break;
                }
                if let Some(slot) = output.get_mut(written) {
                    *slot = self.output_at(t, centre);
                }
                self.produced += 1.0;
                written += 1;
            }
        }
        written
    }

    /// Rebuilds the signal at input-time `t` (`centre = (t + 0.5).floor()`); frames before the
    /// stream's start are silence.
    #[inline]
    fn output_at(&self, t: f64, centre: f64) -> f64 {
        // The kernel sits `d` input frames off the frame at `centre`; the table phase nearest
        // that does the fractional part.
        let d = centre - t;
        let phase = (((d + 0.5) * PHASES as f64).round() as usize).min(PHASES - 1);
        let row = &self.table[phase * TAPS..(phase + 1) * TAPS];
        let first = centre as i64 - (HALF_TAPS as i64 - 1);
        let base = self.fed as i64 - TAPS as i64;
        let mut sum = 0.0;
        for (tap, h) in row.iter().enumerate() {
            let frame = first + tap as i64;
            let value = if frame < 0 {
                0.0
            } else {
                let slot = (frame - base).max(0) as usize;
                self.history.get(slot).copied().unwrap_or(0.0)
            };
            sum += value * *h;
        }
        sum
    }

    /// Clears the history and both counts, as if the resampler was new.
    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.fed = 0;
        self.produced = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `input` through `resampler` in calls of `chunk` input frames, collecting every
    /// output frame, including what stays queued for the kernel's tail at the end.
    fn run(resampler: &mut Resampler, input: &[f64], chunk: usize) -> Vec<f64> {
        let cap = input.len() * 2 + TAPS;
        let mut out = vec![0.0f64; cap];
        let mut at = 0usize;
        let mut fed = 0usize;
        while fed < input.len() {
            let take = chunk.min(input.len() - fed);
            fed += take;
            at += resampler.process(&input[fed - take..fed], &mut out[at..]);
        }
        while at < out.len() {
            let n = resampler.process(&[], &mut out[at..]);
            if n == 0 {
                break;
            }
            at += n;
        }
        out.truncate(at);
        out
    }

    #[test]
    fn a_constant_signal_survives_both_legs() {
        // A 44.1 kHz project into a 48 kHz model and back to 44.1 kHz.
        let input = vec![1.0f64; 2_000];
        let mid = run(&mut Resampler::new(44_100.0, 48_000.0), &input, 256);
        let steady = &mid[mid.len() / 2..];
        let back = run(&mut Resampler::new(48_000.0, 44_100.0), steady, 256);
        for &y in back.iter().skip(48) {
            assert!((y - 1.0).abs() < 1e-9, "{y}");
        }
    }

    #[test]
    fn an_impulse_spreads_over_the_kernel_and_nowhere_else() {
        let mut input = vec![0.0f64; 400];
        input[200] = 1.0;
        let out = run(&mut Resampler::new(44_100.0, 48_000.0), &input, 256);
        assert!(!out.is_empty());
        // The impulse's time in the output rate is 200 * 48 / 44.1 = 217.7; the kernel's spread
        // sits around it and the rest is quiet. The stream starts the instant, so the impulse
        // is where its time says.
        let impulse_time = 200.0 * 48_000.0 / 44_100.0;
        let strongest = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map_or(0, |(i, _)| i);
        assert!((strongest as f64 - impulse_time).abs() <= 2.0, "peak at {strongest} of {}", out.len());
        for (i, &y) in out.iter().enumerate() {
            if (i as f64 - impulse_time).abs() > 2.0 * HALF_TAPS as f64 {
                assert!(y.abs() < 1e-7, "tail {i}: {y}");
            }
        }
        // The stream has almost all of the 400 inputs' worth of output: everything except the
        // kernel's last half, which waits for input that never came.
        assert!(out.len() >= (400.0_f64 * 44_100.0 / 48_000.0).round() as usize);
    }

    #[test]
    fn the_output_never_depends_on_how_the_feed_is_divided() {
        let input: Vec<f64> = (0..2_000).map(|i| (i as f64 * 0.017).sin()).collect();
        let one = run(&mut Resampler::new(48_000.0, 44_100.0), &input, input.len());
        let seven = run(&mut Resampler::new(48_000.0, 44_100.0), &input, 7);
        assert_eq!(one, seven);
    }

    #[test]
    fn unfed_frames_wait_for_a_bigger_output_slice() {
        // An output slice with no room keeps the input: the next call resumes where it stopped.
        let mut line = Resampler::new(48_000.0, 96_000.0);
        let mut a = [0.0f64; 2];
        let wrote_now = line.process(&[0.5; 100], &mut a);
        let mut b = [0.0f64; 64];
        let rest = line.process(&[], &mut b);
        // The stream is the same as one call with room for all of it.
        let full = run(&mut Resampler::new(48_000.0, 96_000.0), &vec![0.5f64; 100], 100);
        let combined: Vec<f64> = a.iter().take(wrote_now).copied().chain(b.iter().take(rest).copied()).collect();
        assert_eq!(combined, full[..wrote_now + rest]);
    }

    #[test]
    fn a_steady_signal_comes_out_at_its_level() {
        let out = run(&mut Resampler::new(48_000.0, 96_000.0), &vec![0.5f64; 400], 64);
        for &y in out.iter().skip(64).take(200) {
            assert!((y - 0.5).abs() < 1e-9, "{y}");
        }
    }

    #[test]
    fn hostile_frames_are_silence_and_outputs_stay_finite() {
        let input = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.0, 1.0, 0.0];
        let out = run(&mut Resampler::new(44_100.0, 48_000.0), &input, 3);
        for &y in &out {
            assert!(y.is_finite(), "{y}");
        }
    }

    #[test]
    fn equal_rates_are_left_to_the_device() {
        assert!(Resampler::new(48_000.0, 48_000.0).passthrough());
        assert!(!Resampler::new(48_000.0, 96_000.0).passthrough());
    }

    #[test]
    fn reset_starts_over() {
        let mut line = Resampler::new(48_000.0, 96_000.0);
        let first = run(&mut line, &vec![0.5f64; 200], 64);
        line.reset();
        let fresh = Resampler::new(48_000.0, 96_000.0);
        assert_eq!(line.table, fresh.table);
        assert_eq!(line.history.as_ref(), fresh.history.as_ref());
        assert_eq!(line.fed, 0);
        assert_eq!(line.produced, 0.0);
        let again = run(&mut line, &vec![0.5f64; 200], 64);
        assert_eq!(first, again);
    }
}
