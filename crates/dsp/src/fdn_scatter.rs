// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Homogeneously lossy Schroeder section for the experimental feedback network.
use crate::DelayLine;

#[derive(Debug)]
pub(super) struct Scatter {
    line: DelayLine,
    frames: usize,
    loss: f32,
    gain: f32,
}
impl Scatter {
    pub(super) fn new(frames: usize, rate: f64, decay: f64, gain: f32) -> Self {
        Self {
            line: DelayLine::new(frames),
            frames,
            loss: 10_f64.powf(-3. * frames as f64 / rate / decay) as f32,
            gain,
        }
    }
    pub(super) fn process(&mut self, input: f32) -> f32 {
        if self.gain == 0. {
            return input;
        }
        let delayed = self.line.read(self.frames) * self.loss;
        let state = input + self.gain * delayed;
        self.line.push(state);
        delayed - self.gain * state
    }
    pub(super) fn reset(&mut self) {
        self.line.clear();
    }
    pub(super) fn history_bytes(&self) -> usize {
        (self.line.capacity() as usize + 3) * size_of::<f32>()
    }
}
