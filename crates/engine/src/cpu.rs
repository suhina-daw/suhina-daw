// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio thread's CPU load: how much of the wall time between the starts of
//! [`crate::Renderer::process`](crate::Renderer) calls the calls themselves took, smoothed
//! and published as an atomic any thread can read. Like [`crate::LauncherMonitor`] and
//! [`crate::PreviewMonitor`], written by the audio thread only; an offline render runs
//! flat out and is not measured at all.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Wall time one measurement covers before it is folded into the smoothed load: long
/// enough that a device callback the output splits into several `process` calls is one
/// measurement, short enough that the meter follows within a second.
const WINDOW: Duration = Duration::from_millis(200);
/// How much each measured window moves the smoothed load: 0.25 settles in about a second.
const SMOOTHING: f64 = 0.25;

/// The audio thread's CPU load, written by the audio thread and readable from any thread:
/// the smoothed share of wall time spent inside `Renderer::process`, in per mille (1000
/// is the audio thread busy all of the time).
#[derive(Debug, Default)]
pub struct CpuMonitor {
    load: AtomicU32,
    late_render_us: AtomicU64,
    late_queued: AtomicU64,
    late_period: AtomicU64,
    late_frames: AtomicU64,
}

impl CpuMonitor {
    /// Last render call with missed plug-in blocks: duration in microseconds, queued
    /// frames, device period and render frames. Diagnostic samples, not a synchronized clock.
    pub fn late_render(&self) -> (u64, u64, u64, u64) {
        (
            self.late_render_us.load(Ordering::Relaxed),
            self.late_queued.load(Ordering::Relaxed),
            self.late_period.load(Ordering::Relaxed),
            self.late_frames.load(Ordering::Relaxed),
        )
    }

    pub(crate) fn record_late_render(&self, elapsed: Duration, queued: usize, period: usize, frames: usize) {
        self.late_render_us.store(elapsed.as_micros().min(u128::from(u64::MAX)) as u64, Ordering::Relaxed);
        self.late_queued.store(queued as u64, Ordering::Relaxed);
        self.late_period.store(period as u64, Ordering::Relaxed);
        self.late_frames.store(frames as u64, Ordering::Relaxed);
    }

    /// The smoothed load, in per mille: 123 is 12.3 %.
    pub fn load_milli(&self) -> u32 {
        self.load.load(Ordering::Relaxed)
    }

    /// The smoothed load as a percentage: 12.3.
    pub fn load_percent(&self) -> f64 {
        f64::from(self.load_milli()) / 10.0
    }
}

/// Measures the load of the renderer's `process` calls and publishes it to a
/// [`CpuMonitor`]. Owned by the renderer; nothing else may write to it.
pub(crate) struct CpuMeter {
    pub(crate) monitor: Arc<CpuMonitor>,
    /// The smoothed load, 0 … 1.
    load: f64,
    /// Busy and idle wall time since the last window was published.
    busy: Duration,
    idle: Duration,
    /// When the last call ended; `None` before the first call, which has no idle time
    /// before it to count.
    last_end: Option<Instant>,
}

impl CpuMeter {
    pub(crate) fn new() -> Self {
        Self {
            monitor: Arc::new(CpuMonitor::default()),
            load: 0.0,
            busy: Duration::ZERO,
            idle: Duration::ZERO,
            last_end: None,
        }
    }

    /// Whether a call has been measured yet; an offline render never measures one.
    #[cfg(test)]
    pub(crate) fn measured_any(&self) -> bool {
        self.last_end.is_some()
    }

    /// Folds one `Renderer::process` call, from `started` to `ended`, into the load; the
    /// time between the last call's end and this one's start is idle. The first call only
    /// sets where idle begins. Nothing is published until a window of busy and idle time
    /// has passed, so the smoothing does not move on a single short call.
    pub(crate) fn measured(&mut self, started: Instant, ended: Instant) {
        if let Some(last) = self.last_end {
            self.idle += started.saturating_duration_since(last);
        }
        self.last_end = Some(ended);
        self.busy += ended.duration_since(started);
        if self.busy + self.idle < WINDOW {
            return;
        }
        let duty = self.busy.as_secs_f64() / (self.busy + self.idle).as_secs_f64();
        self.load += (duty - self.load) * SMOOTHING;
        let milli = (self.load * 1000.0).round().clamp(0.0, 1000.0) as u32;
        self.monitor.load.store(milli, Ordering::Relaxed);
        self.busy = Duration::ZERO;
        self.idle = Duration::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `calls` calls that each took `busy`, each with `idle` quiet before it, starting now.
    /// The times are given, so what is measured is the meter's maths, not the machine.
    fn paced(meter: &mut CpuMeter, busy: Duration, idle: Duration, calls: usize) {
        let mut now = Instant::now();
        for _ in 0..calls {
            let started = now + idle;
            meter.measured(started, started + busy);
            now = started + busy;
        }
    }

    #[test]
    fn a_quarter_busy_thread_shows_a_quarter_load() {
        let mut meter = CpuMeter::new();
        paced(&mut meter, Duration::from_millis(2), Duration::from_millis(6), 400);
        // Sixteen windows of 25 % duty: the smoothed load has settled at 25 % ± 3 %.
        let load = meter.monitor.load_milli();
        assert!((220..=280).contains(&load), "{load}");
    }

    #[test]
    fn a_thread_that_never_idles_shows_a_full_load() {
        let mut meter = CpuMeter::new();
        paced(&mut meter, Duration::from_millis(5), Duration::ZERO, 600);
        assert!(meter.monitor.load_milli() >= 950, "{}", meter.monitor.load_milli());
    }

    #[test]
    fn an_idle_thread_decays_to_a_low_load() {
        let mut meter = CpuMeter::new();
        paced(&mut meter, Duration::from_millis(5), Duration::ZERO, 600);
        paced(&mut meter, Duration::from_millis(1), Duration::from_millis(99), 100);
        assert!(meter.monitor.load_milli() <= 20, "{}", meter.monitor.load_milli());
    }

    #[test]
    fn the_first_call_only_sets_where_idle_begins() {
        let mut meter = CpuMeter::new();
        // One call shorter than a window on its own publishes nothing: no idle time is
        // known yet, and its busy time alone does not fill a window.
        paced(&mut meter, Duration::from_millis(100), Duration::ZERO, 1);
        assert_eq!(meter.monitor.load_milli(), 0);
    }
}
