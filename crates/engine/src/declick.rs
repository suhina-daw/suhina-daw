// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport starts and stops join the last output value to the new sound, including plug-ins.

/// A transport stop's transition, in seconds.
const STOP_SECONDS: f64 = 0.005;

pub(crate) struct StartGuard {
    last: [f32; 4],
    from: [f32; 4],
    left: u32,
    /// How long the transition running takes.
    length: u32,
    frames: u32,
    stop_frames: u32,
    wait: u32,
}

impl StartGuard {
    pub(crate) fn new(sample_rate: u32) -> Self {
        let frames = (f64::from(sample_rate) * crate::audio::CLICK_GUARD_SECONDS).ceil().max(1.0) as u32;
        // As long as a stopped launched clip's fade: over 2 ms a loud sample's end still ticks.
        let stop_frames = (f64::from(sample_rate) * STOP_SECONDS).ceil().max(1.0) as u32;
        Self { last: [0.0; 4], from: [0.0; 4], left: 0, length: frames, frames, stop_frames, wait: 0 }
    }

    pub(crate) fn begin(&mut self, latency: u32) {
        self.from = self.last;
        self.length = self.frames;
        self.left = self.length;
        self.wait = latency;
    }

    /// [`Self::begin`] for a transport stop, which leaves the sound for silence.
    pub(crate) fn begin_stop(&mut self, latency: u32) {
        self.begin(latency);
        self.length = self.stop_frames;
        self.left = self.length;
    }

    /// Runs after mixing; no allocation, and no per-frame work outside a transition.
    pub(crate) fn process(&mut self, main: &mut [f32], cue: &mut [f32]) {
        if self.left > 0 {
            for (main, cue) in main.as_chunks_mut::<2>().0.iter_mut().zip(cue.as_chunks_mut::<2>().0) {
                if self.left == 0 {
                    break;
                }
                // The new transport audio reaches the output after the graph's latency.
                // Preserve the old tail while waiting and join from its actual last sample.
                if self.wait > 0 {
                    self.from = [main[0], main[1], cue[0], cue[1]];
                    self.wait -= 1;
                    continue;
                }
                let t = 1.0 - self.left as f32 / self.length as f32;
                for (sample, from) in main.iter_mut().chain(cue.iter_mut()).zip(self.from) {
                    *sample = from + (*sample - from) * t;
                }
                self.left -= 1;
            }
        }
        if let (Some([l, r]), Some([cl, cr])) = (main.as_chunks::<2>().0.last(), cue.as_chunks::<2>().0.last()) {
            self.last = [*l, *r, *cl, *cr];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_after_a_stop_takes_the_shorter_ramp_again() {
        let mut guard = StartGuard::new(48_000);
        let (start, stop) = (guard.frames, guard.stop_frames);
        assert!(stop > start);
        guard.begin_stop(0);
        assert_eq!((guard.left, guard.length), (stop, stop));
        guard.begin(0);
        assert_eq!((guard.left, guard.length), (start, start));
    }

    #[test]
    fn a_start_continues_from_an_audible_tail_on_both_outputs() {
        let mut guard = StartGuard::new(48_000);
        guard.process(&mut [0.25, -0.25], &mut [0.5, -0.5]);
        guard.begin(0);
        let mut main = [1.0; 256];
        let mut cue = [-1.0; 256];
        guard.process(&mut main, &mut cue);
        assert_eq!(&main[..2], &[0.25, -0.25]);
        assert_eq!(&cue[..2], &[0.5, -0.5]);
        assert_eq!(&main[254..], &[1.0, 1.0]);
        assert_eq!(&cue[254..], &[-1.0, -1.0]);
    }
}
