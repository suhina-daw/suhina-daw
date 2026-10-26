// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mix space analysis thread (`docs/spec/mix-space.md`): reads every track's sound from the
//! engine's mix tap and keeps a [`suhina_dsp::MixSpaceAnalyzer`] per track and tap point, so the
//! Mix Space device can draw where each track sits in the mix.
//!
//! The thread copies nothing while no one looks: a view calls [`MixSpace::want`] every frame it
//! is drawn, and once no view has asked for half a second the thread turns the engine's tap off.
//! The readings are published as a whole [`MixSpaceSnapshot`] behind a mutex the audio thread
//! never touches.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use suhina_dsp::mix_space::{BandReading, DEFAULT_BANDS_PER_OCTAVE, MixSpaceAnalyzer};
use suhina_engine::{MeterMonitor, MixTapReader, TapPoint};
use suhina_model::TrackId;

/// How often the thread reads the queue.
const POLL: Duration = Duration::from_millis(8);
/// How often it publishes readings: about 60 times a second.
const PUBLISH: Duration = Duration::from_millis(16);
/// How fast levels rise, in seconds: short, so drum hits show.
const ATTACK: f32 = 0.01;
/// How long a request lasts: a view that stops asking turns the tap off after this.
const LINGER: Duration = Duration::from_millis(500);

/// One track's readings at one tap point.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSpace {
    /// The track.
    pub track: TrackId,
    /// Where it was tapped.
    pub point: TapPoint,
    /// Its bands, lowest first.
    pub bands: Vec<BandReading>,
}

/// Every analysed track's readings at one moment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MixSpaceSnapshot {
    /// The tracks, in their meter slots' order.
    pub tracks: Vec<TrackSpace>,
    /// Counts up with every publish, so a view can tell a new snapshot from an old one.
    pub serial: u64,
}

impl MixSpaceSnapshot {
    /// The readings of `track` at `point`.
    pub fn track(&self, track: TrackId, point: TapPoint) -> Option<&TrackSpace> {
        self.tracks.iter().find(|t| t.track == track && t.point == point)
    }
}

#[derive(Default)]
struct Shared {
    snapshot: Mutex<Arc<MixSpaceSnapshot>>,
    /// The milliseconds since `start` at which a view last asked, per point.
    pre_until: AtomicU64,
    post_until: AtomicU64,
    stop: AtomicBool,
    /// How fast levels fall, in seconds, as `f32` bits; 0 for the analyzer's own.
    release: AtomicU32,
    /// How fast levels rise, in seconds, as `f32` bits; 0 for [`ATTACK`].
    attack: AtomicU32,
}

/// The analysis thread of one engine's meters. Dropping it stops the thread.
pub struct MixSpace {
    shared: Arc<Shared>,
    monitor: Arc<MeterMonitor>,
    start: Instant,
    thread: Option<JoinHandle<()>>,
}

impl MixSpace {
    /// Starts the thread for the engine whose meters are `monitor`. `None` when another
    /// analysis already took the engine's tap, or the thread cannot start.
    pub fn start(monitor: Arc<MeterMonitor>) -> Option<Self> {
        let reader = monitor.mix_tap().take_reader()?;
        let shared = Arc::new(Shared::default());
        let start = Instant::now();
        let thread = std::thread::Builder::new()
            .name("suhina-mix-space".into())
            .spawn({
                let shared = Arc::clone(&shared);
                let monitor = Arc::clone(&monitor);
                move || run(&shared, &monitor, reader, start)
            })
            .ok()?;
        Some(Self { shared, monitor, start, thread: Some(thread) })
    }

    /// Whether this analysis reads the engine whose meters are `monitor`.
    pub fn reads(&self, monitor: &Arc<MeterMonitor>) -> bool {
        Arc::ptr_eq(&self.monitor, monitor)
    }

    /// Asks for the points that are true for the next half second. Call it every frame a view
    /// shows the readings.
    pub fn want(&self, pre_fader: bool, post_fader: bool) {
        let until = u64::try_from((self.start.elapsed() + LINGER).as_millis()).unwrap_or(u64::MAX);
        if pre_fader {
            self.shared.pre_until.store(until, Ordering::Relaxed);
        }
        if post_fader {
            self.shared.post_until.store(until, Ordering::Relaxed);
        }
    }

    /// Sets how fast the levels rise and fall, in seconds (each 0.005 to 10).
    pub fn set_times(&self, attack: f32, release: f32) {
        let clean = |s: f32, default: f32| if s.is_finite() { s.clamp(0.005, 10.0) } else { default };
        self.shared.attack.store(clean(attack, ATTACK).to_bits(), Ordering::Relaxed);
        self.shared.release.store(clean(release, 0.3).to_bits(), Ordering::Relaxed);
    }

    /// The latest readings.
    pub fn snapshot(&self) -> Arc<MixSpaceSnapshot> {
        self.shared.snapshot.lock().map(|s| Arc::clone(&s)).unwrap_or_default()
    }
}

impl Drop for MixSpace {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.monitor.mix_tap().set_points(false, false);
    }
}

struct Analysis {
    track: TrackId,
    analyzer: MixSpaceAnalyzer,
}

fn run(shared: &Shared, monitor: &MeterMonitor, mut reader: MixTapReader, start: Instant) {
    let tap = monitor.mix_tap();
    let sample_rate = f64::from(tap.sample_rate().max(1));
    let mut analyses: HashMap<(TapPoint, usize), Analysis> = HashMap::new();
    let mut last_publish = Instant::now();
    let mut serial = 0u64;
    let mut on = (false, false);
    let mut release = 0u32;
    let mut attack = ATTACK;
    while !shared.stop.load(Ordering::Relaxed) {
        let now = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let wanted = (shared.pre_until.load(Ordering::Relaxed) > now, shared.post_until.load(Ordering::Relaxed) > now);
        if wanted != on {
            tap.set_points(wanted.0, wanted.1);
            if !wanted.0 {
                analyses.retain(|(point, _), _| *point != TapPoint::PreFader);
            }
            if !wanted.1 {
                analyses.retain(|(point, _), _| *point != TapPoint::PostFader);
            }
            on = wanted;
        }
        let wanted_release = shared.release.load(Ordering::Relaxed);
        let wanted_attack = match shared.attack.load(Ordering::Relaxed) {
            0 => ATTACK,
            bits => f32::from_bits(bits),
        };
        if wanted_release != release || wanted_attack != attack {
            release = wanted_release;
            attack = wanted_attack;
            for analysis in analyses.values_mut() {
                analysis.analyzer.set_times(attack, f32::from_bits(release));
            }
        }
        reader.read(|point, slot, block| {
            let Some(track) = monitor.slot_track(slot) else { return };
            let analysis = analyses.entry((point, slot)).or_insert_with(|| {
                let mut analyzer = MixSpaceAnalyzer::new(sample_rate, DEFAULT_BANDS_PER_OCTAVE);
                if release != 0 {
                    analyzer.set_times(attack, f32::from_bits(release));
                }
                Analysis { track, analyzer }
            });
            if analysis.track != track {
                analysis.track = track;
                analysis.analyzer.reset();
            }
            let (left, right): (Vec<f32>, Vec<f32>) = block.as_chunks::<2>().0.iter().map(|[l, r]| (*l, *r)).unzip();
            analysis.analyzer.push(&left, &right);
        });
        if last_publish.elapsed() >= PUBLISH {
            last_publish = Instant::now();
            serial += 1;
            let mut tracks: Vec<(usize, TrackSpace)> = analyses
                .iter()
                .filter(|(_, a)| a.analyzer.frames() > 0)
                .map(|(&(point, slot), a)| {
                    (slot, TrackSpace { track: a.track, point, bands: a.analyzer.readings().to_vec() })
                })
                .collect();
            tracks.sort_by_key(|(slot, t)| (*slot, t.point == TapPoint::PostFader));
            let snapshot = MixSpaceSnapshot { tracks: tracks.into_iter().map(|(_, t)| t).collect(), serial };
            if let Ok(mut slot) = shared.snapshot.lock() {
                *slot = Arc::new(snapshot);
            }
        }
        std::thread::sleep(POLL);
    }
    tap.set_points(false, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_engine::Renderer;

    #[test]
    fn a_playing_track_shows_up_while_wanted_and_the_tap_stops_after() {
        let project = suhina_test_support::ProjectBuilder::new().midi_track(1, "Lead").build();
        let mut renderer = Renderer::new(&project, 48_000).expect("renderer");
        let monitor = renderer.meter_monitor();
        let space = MixSpace::start(Arc::clone(&monitor)).expect("thread");
        assert!(MixSpace::start(Arc::clone(&monitor)).is_none(), "one analysis per engine");
        assert!(space.reads(&monitor));
        space.want(false, true);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !monitor.mix_tap().is_tapping(TapPoint::PostFader) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(monitor.mix_tap().is_tapping(TapPoint::PostFader));
        assert!(!monitor.mix_tap().is_tapping(TapPoint::PreFader));
        let mut out = vec![0.0f32; 512];
        for _ in 0..200 {
            renderer.process(&mut out);
        }
        while space.snapshot().serial == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let snapshot = space.snapshot();
        let lead = snapshot.track(TrackId(1), TapPoint::PostFader).expect("the track is analysed");
        assert!(lead.bands.iter().all(|b| b.level_db <= suhina_dsp::mix_space::FLOOR_DB + 1.0), "silent");
        drop(space);
        assert!(!monitor.mix_tap().is_tapping(TapPoint::PostFader), "dropping it turns the tap off");
    }
}
