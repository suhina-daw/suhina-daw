// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preallocated per-note streaming stretch state; source access remains in the sampler.
use suhina_stretch::Stretch;

const BLOCK: usize = 64;

pub(super) struct Voice {
    stretch: Stretch,
    input: Vec<f32>,
    output: [f32; BLOCK * 2],
    cursor: usize,
    ready: bool,
    speed: f64,
    ahead: f64,
    fraction: f64,
}

impl Voice {
    pub(super) fn new(rate: f64) -> Self {
        let stretch = Stretch::new(2, rate as f32);
        let capacity = stretch.output_seek_length(4.).max(BLOCK * 4) * 2;
        Self {
            stretch,
            input: vec![0.; capacity],
            output: [0.; BLOCK * 2],
            cursor: BLOCK,
            ready: false,
            speed: 1.,
            ahead: 0.,
            fraction: 0.,
        }
    }

    pub(super) fn restart(&mut self) {
        self.ready = false;
        self.cursor = BLOCK;
    }

    pub(super) fn next(&mut self, speed: f64, pitch: f64, source: impl Fn(f64) -> [f64; 2]) -> [f64; 2] {
        if speed != self.speed {
            self.restart();
        }
        self.stretch.set_transpose_factor(pitch as f32, 0.);
        if !self.ready {
            self.speed = speed;
            let frames = self.stretch.output_seek_length(speed as f32);
            for (i, frame) in self.input.as_chunks_mut::<2>().0.iter_mut().take(frames).enumerate() {
                let data = source(i as f64);
                frame.copy_from_slice(&[data[0] as f32, data[1] as f32]);
            }
            self.stretch.output_seek(self.input.get(..frames * 2).unwrap_or_default());
            self.ahead = frames as f64;
            self.fraction = 0.;
            self.ready = true;
        }
        if self.cursor == BLOCK {
            let exact = speed * BLOCK as f64 + self.fraction;
            let frames = exact.floor() as usize;
            self.fraction = exact - frames as f64;
            for (i, frame) in self.input.as_chunks_mut::<2>().0.iter_mut().take(frames).enumerate() {
                let data = source(self.ahead + i as f64);
                frame.copy_from_slice(&[data[0] as f32, data[1] as f32]);
            }
            self.stretch.process(self.input.get(..frames * 2).unwrap_or_default(), &mut self.output);
            self.ahead += frames as f64 - speed * BLOCK as f64;
            self.cursor = 0;
        }
        let offset = self.cursor * 2;
        self.cursor += 1;
        [
            f64::from(self.output.get(offset).copied().unwrap_or(0.)),
            f64::from(self.output.get(offset + 1).copied().unwrap_or(0.)),
        ]
    }
}
