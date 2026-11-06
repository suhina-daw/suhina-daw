// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Instrument-only stop envelope. Fade the actual continuing waveform before delivering
//! a stopped transport, which some instruments interpret as an immediate voice reset.

pub(super) struct TransportGate {
    playing: bool,
    gain: f32,
    from: f32,
    target: f32,
    frame: u32,
    frames: u32,
    live_at: Option<usize>,
}

impl TransportGate {
    pub(super) fn new(rate: u32) -> Self {
        Self { playing: false, gain: 1.0, from: 1.0, target: 1.0, frame: 0, frames: (rate / 200).max(1), live_at: None }
    }

    fn ramp(&mut self, target: f32) {
        self.from = self.gain;
        self.target = target;
        self.frame = 0;
    }

    pub(super) fn transport(&mut self, playing: bool) {
        if self.playing != playing {
            self.ramp(if playing { 1.0 } else { 0.0 });
        }
        self.playing = playing;
    }

    pub(super) fn holds_transport(&self) -> bool {
        !self.playing && self.target == 0.0 && self.gain > 0.0
    }

    pub(super) fn unity(&self) -> bool {
        self.gain == 1.0 && self.target == 1.0
    }

    pub(super) fn live_note(&mut self, frame: usize) {
        if !self.playing && self.target == 0.0 {
            self.live_at = Some(self.live_at.map_or(frame, |old| old.min(frame)));
        }
    }

    pub(super) fn process(&mut self, audio: &mut [f32]) {
        for (frame, pair) in audio.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            if self.live_at == Some(frame) {
                self.live_at = None;
                self.ramp(1.0);
            }
            if self.frame < self.frames {
                let t = self.frame as f32 / self.frames as f32;
                let smooth = t * t * (3.0 - 2.0 * t);
                self.gain = self.from + (self.target - self.from) * smooth;
                self.frame += 1;
            } else {
                self.gain = self.target;
            }
            pair[0] *= self.gain;
            pair[1] *= self.gain;
        }
        self.live_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_fades_the_waveform_before_reporting_stopped() {
        let mut gate = TransportGate::new(48_000);
        gate.transport(true);
        gate.transport(false);
        assert!(gate.holds_transport());
        let mut audio = [1.0; 512];
        gate.process(&mut audio);
        assert_eq!(&audio[..2], &[1.0, 1.0]);
        assert_eq!(&audio[480..], &[0.0; 32]);
        assert!(!gate.holds_transport());
        assert!(audio.windows(3).step_by(2).all(|s| (s[2] - s[0]).abs() < 0.007));
    }

    #[test]
    fn a_quick_restart_reverses_the_fade_without_a_gain_step() {
        let mut gate = TransportGate::new(48_000);
        gate.transport(true);
        gate.transport(false);
        let mut out = [1.0; 128];
        gate.process(&mut out);
        let last = out[126];
        gate.transport(true);
        out.fill(1.0);
        gate.process(&mut out);
        assert_eq!(out[0], last);
        assert!(out[126] > last);
        assert!(!gate.holds_transport());
    }

    #[test]
    fn a_live_note_after_pause_reopens_at_its_sample_offset() {
        let mut gate = TransportGate::new(48_000);
        gate.transport(true);
        gate.transport(false);
        gate.process(&mut [1.0; 1024]);
        gate.live_note(64);
        let mut out = [1.0; 1024];
        gate.process(&mut out);
        assert!(out[..128].iter().all(|s| *s == 0.0));
        assert_eq!(out[1023], 1.0);
        assert!(!gate.holds_transport());
    }
}
