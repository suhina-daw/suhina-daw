// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-boundary fades: independent of transport, instruments and queued render blocks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Default)]
pub(crate) struct Control {
    closing: AtomicBool,
    drained: AtomicBool,
}

impl Control {
    pub(crate) fn request_close(&self) {
        self.closing.store(true, Ordering::Release);
    }

    pub(crate) fn drained(&self) -> bool {
        self.drained.load(Ordering::Acquire)
    }

    /// Owner thread only. A missing callback or device error cannot hold up shutdown.
    pub(crate) fn finish(&self, timeout: Duration, failed: impl Fn() -> bool) -> bool {
        self.request_close();
        let start = std::time::Instant::now();
        while !self.drained() && !failed() && start.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.drained()
    }
}

pub(crate) struct Envelope {
    rate: u32,
    length: usize,
    at: usize,
    started: bool,
    closing: bool,
    gain: f32,
    from: f32,
    quiet: usize,
}

impl Envelope {
    pub(crate) fn new(rate: u32) -> Self {
        Self {
            rate,
            length: (rate as usize / 200).max(1),
            at: 0,
            started: false,
            closing: false,
            gain: 0.0,
            from: 0.0,
            quiet: 0,
        }
    }

    pub(crate) fn apply(&mut self, samples: &mut [f32], control: &Control, delay: Duration, period: usize) {
        if control.closing.load(Ordering::Acquire) && !self.closing {
            self.closing = true;
            self.from = self.gain;
            self.at = 0;
        }
        // Keep supplying silence beyond the fade until queued hardware audio has had
        // time to play. The owner independently bounds the wait if callbacks stop.
        let drain = (delay.as_secs_f64().min(0.5) * f64::from(self.rate)).ceil() as usize;
        let drain = drain.saturating_add(period.saturating_mul(2));
        if self.started && !self.closing && self.gain == 1.0 {
            return;
        }
        for frame in samples.as_chunks_mut::<{ super::RING_CHANNELS }>().0 {
            if !self.started && frame.iter().any(|sample| *sample != 0.0) {
                self.started = true;
            }
            if self.closing || self.started {
                let t = self.at.min(self.length) as f32 / self.length as f32;
                let curve = t * t * (3.0 - 2.0 * t);
                self.gain = if self.closing { self.from * (1.0 - curve) } else { curve };
                self.at = self.at.saturating_add(1).min(self.length);
            }
            for sample in frame {
                *sample = if self.gain == 0.0 { 0.0 } else { *sample * self.gain };
            }
            if self.closing && self.gain == 0.0 {
                self.quiet = self.quiet.saturating_add(1);
            }
        }
        if self.closing && self.quiet >= drain {
            control.drained.store(true, Ordering::Release);
        }
    }
}
