// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the window's time goes, for the trace log (`docs/debugging.md` › UI time): with
//! tracing on, a frame whose drawing takes [`SLOW`] or longer is written with how long each
//! part of it took and what was on screen, and once a second a line sums up the frames drawn.
//! Without tracing nothing is measured.

use std::time::{Duration, Instant};

/// A frame that takes this long or longer is written out part by part.
pub const SLOW: Duration = Duration::from_millis(25);

/// One frame's parts and the second's frames, kept by the window.
#[derive(Debug, Default)]
pub struct FrameTrace {
    started: Option<Instant>,
    last: Option<Instant>,
    parts: Vec<(&'static str, Duration)>,
    notes: Vec<String>,
    /// The second being summed up: when it began, its frames, their time, the longest.
    second: Option<Instant>,
    frames: u32,
    total: Duration,
    worst: Duration,
    /// The start of the frame before, and the longest wait between two frame starts.
    previous: Option<Instant>,
    widest_gap: Duration,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

impl FrameTrace {
    /// A frame begins; nothing is measured while tracing is off.
    pub fn start(&mut self) {
        if !suhina_plugins::trace::on() {
            self.started = None;
            return;
        }
        let now = Instant::now();
        if let Some(previous) = self.previous {
            self.widest_gap = self.widest_gap.max(now - previous);
        }
        self.previous = Some(now);
        self.started = Some(now);
        self.last = Some(now);
        self.parts.clear();
        self.notes.clear();
    }

    /// The time since the last mark was spent on `part` (added up when a part is marked twice).
    pub fn mark(&mut self, part: &'static str) {
        let Some(last) = self.last else { return };
        let now = Instant::now();
        let spent = now - last;
        match self.parts.iter_mut().find(|(p, _)| *p == part) {
            Some((_, total)) => *total += spent,
            None => self.parts.push((part, spent)),
        }
        self.last = Some(now);
    }

    /// What was on screen, written with a slow frame; only made while tracing is on.
    pub fn note(&mut self, note: impl FnOnce() -> String) {
        if self.started.is_some() {
            self.notes.push(note());
        }
    }

    /// The frame ends: a slow one is written out, and a second's summary when one has passed.
    pub fn finish(&mut self) {
        let Some(started) = self.started.take() else { return };
        self.mark("rest");
        let spent = started.elapsed();
        if spent >= SLOW {
            let parts: Vec<String> = self
                .parts
                .iter()
                .filter(|(_, d)| *d >= Duration::from_micros(500))
                .map(|(part, d)| format!("{part} {:.1}", ms(*d)))
                .collect();
            suhina_plugins::trace!("slow frame {:.1} ms: {}; {}", ms(spent), parts.join(", "), self.notes.join(", "));
        }
        self.frames += 1;
        self.total += spent;
        self.worst = self.worst.max(spent);
        let second = *self.second.get_or_insert(started);
        let passed = started - second;
        if passed >= Duration::from_secs(1) {
            let frames = f64::from(self.frames);
            suhina_plugins::trace!(
                "frames: {} in {:.1} s ({:.0} a second), drawing {:.1} ms on average, {:.1} ms the longest; longest wait between frames {:.1} ms",
                self.frames,
                passed.as_secs_f64(),
                frames / passed.as_secs_f64(),
                ms(self.total) / frames,
                ms(self.worst),
                ms(self.widest_gap),
            );
            self.second = Some(started);
            self.frames = 0;
            self.total = Duration::ZERO;
            self.worst = Duration::ZERO;
            self.widest_gap = Duration::ZERO;
        }
    }
}
