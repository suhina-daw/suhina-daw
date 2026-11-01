// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The count-in before recording ([`docs/spec/recording.md`](../../../docs/spec/recording.md) ›
//! The count-in): with the transport stopped, the metronome clicks a number of beats at the
//! tempo playing, the first beat of each bar higher, whether or not the metronome is on; then
//! the transport starts on the frame the count ends. [`crate::LaunchCommand::CountIn`] starts
//! it, a stop or a start cancels it. Nothing here allocates.

use std::sync::atomic::Ordering;

use crate::Renderer;
use crate::metronome::Metronome;

/// The longest count-in, in beats: four bars of 16 beats.
pub(crate) const MAX_COUNT_IN_BEATS: f64 = 64.0;

/// A count-in that is counting.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CountIn {
    /// Frames until the transport starts.
    left: u64,
    /// The count's beat, from minus its length up to 0, and beats per frame.
    beat: f64,
    step: f64,
    /// Beats in a bar, which the higher click marks.
    bar_beats: f64,
    click: Metronome,
}

impl Renderer {
    /// Counts `beats` beats (bars of `bar_beats`) before the transport starts; a transport that
    /// runs already keeps running, and a count of nothing starts it at once.
    pub(crate) fn start_count_in(&mut self, beats: f64, bar_beats: f64) {
        if self.transport.running {
            return;
        }
        let beats = if beats.is_nan() { 0.0 } else { beats.clamp(0.0, MAX_COUNT_IN_BEATS) };
        let step = self.graph.launcher.tempo_bpm() / 60.0 / f64::from(self.sample_rate);
        let frames = beats / step;
        if !(frames.is_finite() && frames >= 1.0) {
            self.start_transport();
            return;
        }
        let mut click = Metronome::new(self.sample_rate);
        click.set(true, Some(self.metronome.gain()));
        let bar_beats = if bar_beats.is_finite() && bar_beats > 0.0 { bar_beats } else { 4.0 };
        self.transport.count_in = Some(CountIn { left: frames.round() as u64, beat: -beats, step, bar_beats, click });
        self.publish_count_in();
    }

    /// Frames until a count-in that counts starts the transport.
    pub(crate) fn frames_to_count_in_end(&self) -> Option<usize> {
        self.transport.count_in.map(|c| usize::try_from(c.left).unwrap_or(usize::MAX))
    }

    /// Starts the transport when the count-in has counted its last frame.
    pub(crate) fn end_count_in(&mut self) {
        if self.transport.count_in.is_some_and(|c| c.left == 0) {
            self.transport.count_in = None;
            self.start_transport();
        }
        self.publish_count_in();
    }

    /// The count-in's click for this frame, moving the count on; 0 when none counts.
    pub(crate) fn count_in_frame(&mut self) -> f32 {
        let Some(count) = self.transport.count_in.as_mut().filter(|c| c.left > 0) else { return 0.0 };
        let bar = count.bar_beats;
        let sample = count.click.frame(true, count.beat, |index| {
            let into = (index as f64).rem_euclid(bar);
            into < 1e-9 || bar - into < 1e-9
        });
        count.beat += count.step;
        count.left -= 1;
        sample
    }

    /// Ends a count-in without starting anything (the transport stopped or started).
    pub(crate) fn cancel_count_in(&mut self) {
        self.transport.count_in = None;
        self.publish_count_in();
    }

    /// Tells the controller the beats the count-in still counts.
    pub(crate) fn publish_count_in(&self) {
        let left = self.transport.count_in.map_or(0.0, |c| c.left as f64 * c.step);
        self.graph.launcher.monitor.count_in.store(left.to_bits(), Ordering::Relaxed);
    }
}
