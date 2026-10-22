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
    /// What asked for the second's frames (a source file and line each), and how often: a
    /// window that keeps drawing while nothing moves says here why.
    causes: Vec<(String, u32)>,
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

    /// Counts the calls that asked for this frame ([`egui::Context::repaint_causes`]).
    pub fn causes(&mut self, ctx: &egui::Context) {
        if self.started.is_none() {
            return;
        }
        for cause in ctx.repaint_causes() {
            let file = cause.file.rsplit(['/', '\\']).next().unwrap_or(cause.file);
            let name = format!("{file}:{}", cause.line);
            match self.causes.iter_mut().find(|(n, _)| *n == name) {
                Some((_, count)) => *count += 1,
                None => self.causes.push((name, 1)),
            }
        }
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
            self.causes.sort_by_key(|cause| std::cmp::Reverse(cause.1));
            let causes: Vec<String> = self.causes.iter().take(3).map(|(name, n)| format!("{name} {n}")).collect();
            suhina_plugins::trace!(
                "frames: {} in {:.1} s ({:.0} a second), drawing {:.1} ms on average, {:.1} ms the longest; longest wait between frames {:.1} ms; asked by {}",
                self.frames,
                passed.as_secs_f64(),
                frames / passed.as_secs_f64(),
                ms(self.total) / frames,
                ms(self.worst),
                ms(self.widest_gap),
                if causes.is_empty() { "input".to_string() } else { causes.join(", ") },
            );
            self.causes.clear();
            self.second = Some(started);
            self.frames = 0;
            self.total = Duration::ZERO;
            self.worst = Duration::ZERO;
            self.widest_gap = Duration::ZERO;
        }
    }
}

/// A UI-only nested measurement, including early returns. Never use on the audio thread.
/// Reports only work lasting at least five milliseconds; disabled tracing reads no clock.
pub(crate) struct WorkSpan {
    name: &'static str,
    started: Option<Instant>,
    last: Option<Instant>,
    parts: [Option<(&'static str, Duration)>; 12],
    count: usize,
}

impl WorkSpan {
    pub(crate) fn new(name: &'static str) -> Self {
        let started = suhina_plugins::trace::on().then(Instant::now);
        Self { name, started, last: started, parts: [None; 12], count: 0 }
    }
    /// Partition a measured call without allocating or writing one line per checkpoint.
    pub(crate) fn mark(&mut self, name: &'static str) {
        let Some(last) = self.last else { return };
        let now = Instant::now();
        if let Some(slot) = self.parts.get_mut(self.count) {
            *slot = Some((name, now - last));
            self.count += 1;
            self.last = Some(now);
        }
    }
}

impl Drop for WorkSpan {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            let elapsed = started.elapsed();
            if elapsed >= suhina_plugins::trace::SLOW_WORK {
                self.mark("rest");
                let parts = self
                    .parts
                    .iter()
                    .flatten()
                    .map(|(name, elapsed)| format!("{name} {:.2}", ms(*elapsed)))
                    .collect::<Vec<_>>()
                    .join(", ");
                suhina_plugins::trace!("slow {}: {:.1} ms; {}", self.name, ms(elapsed), parts);
            }
        }
    }
}

#[cfg(test)]
mod work_span_tests {
    use super::*;

    #[test]
    fn disabled_checkpoints_do_not_record_and_active_storage_is_bounded() {
        let mut span = WorkSpan { name: "test", started: None, last: None, parts: [None; 12], count: 0 };
        span.mark("disabled");
        assert_eq!(span.count, 0);
        span.last = Some(Instant::now());
        for _ in 0..20 {
            span.mark("part");
        }
        assert_eq!(span.count, 12);
        assert!(span.parts.iter().all(Option::is_some));
    }
}
