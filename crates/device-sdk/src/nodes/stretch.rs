// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preallocated per-note streaming stretch state; source access remains in the sampler.
use suhina_stretch::Stretch;

const BLOCK: usize = 64;

pub(crate) struct Voice {
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
    pub(crate) fn new(rate: f64) -> Self {
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

    pub(crate) fn restart(&mut self) {
        self.ready = false;
        self.cursor = BLOCK;
    }

    pub(crate) fn next(&mut self, speed: f64, pitch: f64, source: impl Fn(f64) -> [f64; 2]) -> [f64; 2] {
        self.speed = speed;
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

use super::{NodeInfo, PortInfo};
use crate::{ProcessContext, native_node::NativeNode, sample_resource::SampleAsset};
use std::sync::Arc;
use suhina_device_api::NoteEvent;
pub(crate) const TIME_STRETCH_PLAYER: NodeInfo = NodeInfo {
    kind: "time_stretch_player",
    name: "TimeStretchPlayer",
    rust_variant: "TimeStretchPlayer",
    inputs: &[
        PortInfo { name: "speed", default: Some(1.), unit: "ratio" },
        PortInfo { name: "pitch", default: Some(1.), unit: "ratio" },
        PortInfo { name: "start", default: Some(0.), unit: "frames" },
        PortInfo { name: "end", default: Some(-1.), unit: "frames" },
        PortInfo { name: "events", default: Some(0.), unit: "midi" },
    ],
    outputs: &["out", "left", "right", "position", "finished"],
    description: "Triggered resident-sample playback through the shared native stretch voice.",
    configuration: "sample binding",
    example: r#"{"kind":"time_stretch_player","sample":"sample"}"#,
};
struct Player {
    source: Arc<SampleAsset>,
    voice: Voice,
    rate: f64,
    position: f64,
    active: bool,
    pending: bool,
    cache: [[[f32; 2]; 64]; 2],
}
pub(crate) fn player(source: Arc<SampleAsset>, rate: f64) -> Box<dyn NativeNode> {
    Box::new(Player {
        source,
        voice: Voice::new(rate),
        rate,
        position: 0.,
        active: false,
        pending: false,
        cache: [[[0.; 2]; 64]; 2],
    })
}
impl NativeNode for Player {
    fn prepare_block(&mut self, _: &ProcessContext) {}
    fn process(&mut self, inputs: [&[f32]; 5], output: &mut [f32]) {
        let source_ratio = f64::from(self.source.sample_rate()) / self.rate;
        for (frame, pair) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let x: [f64; 4] = std::array::from_fn(|p| {
                let v = inputs[p].get(frame * 2).copied().unwrap_or(0.);
                if v.is_finite() { f64::from(v) } else { 0. }
            });
            let speed = x[0].clamp(0.25, 4.);
            let pitch = x[1].clamp(1. / 256., 256.);
            let start = x[2].max(0.).floor();
            let end = if x[3] < 0. { self.source.frames() as f64 } else { x[3].floor() };
            if self.pending {
                self.position = start;
                self.voice.restart();
                self.pending = false;
                self.active = true;
            }
            *pair = if self.active
                && start < end
                && end <= self.source.frames() as f64
                && self.position >= start
                && self.position < end
            {
                if end - start <= 1. {
                    self.source.read_region_stereo(start as usize, end as usize, self.position - start)
                } else {
                    let position = self.position;
                    let source = &self.source;
                    self.voice
                        .next(speed, pitch, |ahead| {
                            source
                                .read_region_stereo(
                                    start as usize,
                                    end as usize,
                                    position - start + ahead * source_ratio,
                                )
                                .map(f64::from)
                        })
                        .map(|v| v as f32)
                }
            } else {
                self.active = false;
                [0.; 2]
            };
            self.cache[0][frame] = [self.position as f32; 2];
            self.cache[1][frame] = [f32::from(!self.active); 2];
            if self.active {
                self.position += speed * source_ratio;
            }
        }
    }
    fn output(&self, port: usize, frame: usize) -> [f32; 2] {
        self.cache.get(port).and_then(|v| v.get(frame)).copied().unwrap_or([0.; 2])
    }
    fn event(&mut self, event: NoteEvent) {
        match super::events::normalized(event) {
            Some(NoteEvent::On { .. }) => self.pending = true,
            Some(NoteEvent::AllNotesOff) => {
                self.pending = false;
                self.active = false;
            }
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.position = 0.;
        self.active = false;
        self.pending = false;
        self.voice.restart();
        self.cache.fill([[0.; 2]; 64]);
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn speed_changes_preserve_buffered_output() {
        let mut voice = super::Voice::new(48000.);
        voice.next(1., 1., |_| [0.5; 2]);
        let cursor = voice.cursor;
        voice.next(1.1, 1., |_| [0.5; 2]);
        assert_eq!(voice.cursor, cursor + 1);
        assert!(voice.ready);
    }
}
