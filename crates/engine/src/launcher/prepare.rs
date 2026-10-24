// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launched clips' stretchers, built off the audio thread (ADR 0003, decision 7).
//!
//! A warped clip in a launcher slot plays through a [`StretchVoice`], whose stretcher and buffers
//! take 1 to 4 ms to build (a mono clip at 48 kHz, release build). The audio thread must never
//! build one (AGENTS.md rule 3), and building one for every warped slot whenever a graph is built
//! (every edit, ADR 0005) added seconds to each edit in a project with hundreds of them. So a
//! graph's launcher holds its voices unprepared, and a [`Preparer`] on the stretch thread builds
//! them: first the voices the audio side asks for (a launch queued, a clip playing), then the
//! rest in slot order.
//!
//! The built parts reach the audio side through a ring per graph, and the launcher moves them
//! into their voices at the start of a block ([`Launcher::service_voices`]); a voice whose clip is
//! playing goes to where the clip is by then. Nothing is allocated or freed there: parts the
//! launcher cannot use go back to the worker on a second ring, and it only takes parts while
//! that ring has room.
//!
//! A launch whose voice has not arrived starts on time all the same: its stretched audio is
//! silent until the voice arrives, then plays from where the clip has got to. With a bar's
//! quantisation the voice is there long before; a launch right after an edit that changed
//! hundreds of warped slots (a new project tempo) can lose its first few milliseconds.
//!
//! A new graph takes over the old graph's prepared voices for the slots whose audio did not
//! change, at the swap on the audio thread ([`Launcher::take_voices`]). One claim per voice says
//! who provides it, whichever comes first: the worker as it starts building it, or the swap as it
//! takes the old one over. So a voice never gets two. The worker leaves alone, until the new graph
//! is swapped in, the voices the graph before has with the same audio (the swap will most likely
//! take them over), and it stops working for a graph once it has been swapped out.
//!
//! Without a worker thread (offline rendering) the launcher builds a clip's voices itself as it
//! launches the clip, so offline renders stay the test oracle (AGENTS.md rule 4).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use rtrb::{Consumer, Producer, RingBuffer};

use super::{Launcher, Playing};
use crate::stretch::{StretchParts, StretchSpec, StretchVoice};

/// A voice nobody provides yet.
const OPEN: u8 = 0;
/// The worker builds the voice.
const BUILDING: u8 = 1;
/// The swap took the voice over from the graph before.
pub(super) const TAKEN: u8 = 2;
/// Built parts that can wait in either direction, at most.
const RING: usize = 16;

/// State both sides share.
struct Shared {
    /// Each voice's claim: [`OPEN`], [`BUILDING`] or [`TAKEN`]. Whichever side moves it from
    /// [`OPEN`] first provides the voice.
    claims: Vec<AtomicU8>,
    /// The graph has been swapped in: the voices the swap did not take over may be built.
    live: AtomicBool,
    /// The graph has been swapped out: nothing more is built for it.
    retired: AtomicBool,
    /// Voices the worker has built, for tests.
    built: AtomicU64,
}

/// Where a voice is in the launcher.
#[derive(Debug, Clone, Copy)]
pub(super) struct Place {
    pub(super) column: usize,
    pub(super) slot: usize,
    /// Its index among the clip's audio.
    pub(super) audio: usize,
}

/// Builds a graph's launcher voices on the stretch thread ([`crate::ahead::StretchThread`]).
pub(crate) struct Preparer {
    specs: Vec<StretchSpec>,
    fingerprints: Vec<u64>,
    shared: Arc<Shared>,
    parts: Producer<(usize, StretchParts)>,
    returned: Consumer<StretchParts>,
    requests: Consumer<usize>,
    /// A voice built that the ring had no room for yet.
    waiting: Option<(usize, StretchParts)>,
    /// The next voice in slot order.
    next: usize,
    /// The voices the graph before has with the same audio: built only once this graph is
    /// swapped in, and only those the swap did not take over.
    deferred: Vec<bool>,
    /// The next deferred voice to look at once the graph is swapped in.
    next_deferred: usize,
}

impl Preparer {
    /// Before it starts: defers the voices `before`, the graph this one replaces, has with the
    /// same audio. Runs on the worker thread; allocates a set of `before`'s voices.
    pub(crate) fn follow(&mut self, before: &Preparer) {
        let theirs: std::collections::HashSet<u64> = before.fingerprints.iter().copied().collect();
        for (deferred, fingerprint) in self.deferred.iter_mut().zip(&self.fingerprints) {
            *deferred = theirs.contains(fingerprint);
        }
    }

    /// Whether its graph has gone: swapped out, or dropped.
    pub(crate) fn finished(&self) -> bool {
        self.shared.retired.load(Ordering::Acquire) || Arc::strong_count(&self.shared) < 2
    }

    /// Builds one voice, a voice the audio side asked for first; only those when
    /// `requested_only`. Drops what the audio side gave back. Returns whether it did anything.
    pub(crate) fn work(&mut self, requested_only: bool) -> bool {
        while let Ok(parts) = self.returned.pop() {
            drop(parts);
        }
        if let Some((index, parts)) = self.waiting.take() {
            return match self.parts.push((index, parts)) {
                Ok(()) => true,
                Err(rtrb::PushError::Full(item)) => {
                    self.waiting = Some(item);
                    false
                }
            };
        }
        if self.finished() {
            return false;
        }
        let Some(index) = self.pick(requested_only) else { return false };
        let Some(spec) = self.specs.get(index) else { return false };
        let parts = spec.build();
        self.shared.built.fetch_add(1, Ordering::Relaxed);
        if let Err(rtrb::PushError::Full(item)) = self.parts.push((index, parts)) {
            self.waiting = Some(item);
        }
        true
    }

    /// The voice to build next, claimed: one asked for, then the rest in slot order, then the
    /// deferred ones once the graph is swapped in.
    fn pick(&mut self, requested_only: bool) -> Option<usize> {
        let claim = |shared: &Shared, index: usize| {
            shared
                .claims
                .get(index)
                .is_some_and(|c| c.compare_exchange(OPEN, BUILDING, Ordering::AcqRel, Ordering::Acquire).is_ok())
        };
        while let Ok(index) = self.requests.pop() {
            if claim(&self.shared, index) {
                return Some(index);
            }
        }
        if requested_only {
            return None;
        }
        while self.next < self.specs.len() {
            let index = self.next;
            self.next += 1;
            if !self.deferred.get(index).copied().unwrap_or(false) && claim(&self.shared, index) {
                return Some(index);
            }
        }
        if !self.shared.live.load(Ordering::Acquire) {
            return None;
        }
        while self.next_deferred < self.specs.len() {
            let index = self.next_deferred;
            self.next_deferred += 1;
            if self.deferred.get(index).copied().unwrap_or(false) && claim(&self.shared, index) {
                return Some(index);
            }
        }
        None
    }
}

/// The launcher's side: where each voice is, the rings, and the preparer while no thread has
/// taken it.
pub(super) struct Voices {
    places: Vec<Place>,
    shared: Arc<Shared>,
    parts: Consumer<(usize, StretchParts)>,
    returned: Producer<StretchParts>,
    requests: Producer<usize>,
    /// The voices asked for already: each is asked for once.
    requested: Vec<bool>,
    /// The preparer, while no thread has taken it: the launcher then builds voices itself as it
    /// launches their clips.
    inline: Option<Preparer>,
}

impl Voices {
    /// The voices at `places`, each built from its spec, with the fingerprint of its audio.
    pub(super) fn new(voices: Vec<(Place, StretchSpec, u64)>) -> Self {
        let count = voices.len();
        let shared = Arc::new(Shared {
            claims: (0..count).map(|_| AtomicU8::new(OPEN)).collect(),
            live: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            built: AtomicU64::new(0),
        });
        let ring = RING.min(count).max(1);
        let (parts_in, parts) = RingBuffer::new(ring);
        let (returned, returned_out) = RingBuffer::new(ring);
        // Each voice is asked for once, so the ring never fills.
        let (requests, requests_out) = RingBuffer::new(count.max(1));
        let mut places = Vec::with_capacity(count);
        let mut specs = Vec::with_capacity(count);
        let mut fingerprints = Vec::with_capacity(count);
        for (place, spec, fingerprint) in voices {
            places.push(place);
            specs.push(spec);
            fingerprints.push(fingerprint);
        }
        let preparer = Preparer {
            specs,
            fingerprints,
            shared: Arc::clone(&shared),
            parts: parts_in,
            returned: returned_out,
            requests: requests_out,
            waiting: None,
            next: 0,
            deferred: vec![false; count],
            next_deferred: 0,
        };
        Self { places, shared, parts, returned, requests, requested: vec![false; count], inline: Some(preparer) }
    }

    /// Voice `index`'s claim.
    pub(super) fn claim(&self, index: usize) -> u8 {
        self.shared.claims.get(index).map_or(OPEN, |c| c.load(Ordering::Acquire))
    }

    /// Whether no thread has taken the preparer: voices are built where they are needed.
    pub(super) fn inline(&self) -> bool {
        self.inline.is_some()
    }

    /// Voices the worker has built for this graph.
    #[cfg(test)]
    pub(super) fn built(&self) -> u64 {
        self.shared.built.load(Ordering::Relaxed)
    }
}

impl Launcher {
    /// Hands the preparer to a thread of its own. `None` once taken.
    pub(crate) fn take_preparer(&mut self) -> Option<Preparer> {
        self.voices.inline.take()
    }

    /// At the start of each block: moves the voices that have arrived into their clips (a playing
    /// clip's voice to where the clip is) and asks for the voices of the launches queued and the
    /// clips playing that have not arrived. Never allocates or frees.
    pub(crate) fn service_voices(&mut self) {
        self.receive_voices();
        let Self { columns, voices, .. } = self;
        for column in columns.iter() {
            let launching = column.pending.and_then(|p| match p.action {
                super::Queued::Launch(slot) => Some(slot),
                super::Queued::Stop => None,
            });
            let playing = [column.playing, column.fading].map(|p| p.map(|p| p.slot));
            for slot in [launching, playing[0], playing[1]].into_iter().flatten() {
                let Some(plan) = column.slots.get(slot).and_then(|s| s.clip.as_ref()) else { continue };
                for a in plan.audio.iter().filter(|a| a.stretch.as_ref().is_some_and(|v| !v.is_prepared())) {
                    let Some(index) = a.voice else { continue };
                    if let Some(asked) = voices.requested.get_mut(index)
                        && !*asked
                        && voices.requests.push(index).is_ok()
                    {
                        *asked = true;
                    }
                }
            }
        }
    }

    /// Moves the voices that have arrived into their clips; a voice whose clip is playing goes to
    /// where the clip is. Takes parts only while it can give them back. Never allocates or frees.
    pub(super) fn receive_voices(&mut self) {
        let frames_per_beat = self.frames_per_beat_at_build;
        let Self { columns, voices, .. } = self;
        while voices.returned.slots() > 0 {
            let Ok((index, parts)) = voices.parts.pop() else { break };
            let place = voices.places.get(index).copied();
            let column = place.and_then(|p| columns.get_mut(p.column));
            let Some((place, column)) = place.zip(column) else {
                let _ = voices.returned.push(parts);
                continue;
            };
            let states = [column.playing, column.fading];
            // There is room on `returned` for these parts, should the voice not take them.
            let Some(voice) = column
                .slots
                .get_mut(place.slot)
                .and_then(|s| s.clip.as_mut())
                .and_then(|plan| plan.audio.get_mut(place.audio))
                .and_then(|a| a.stretch.as_mut())
            else {
                let _ = voices.returned.push(parts);
                continue;
            };
            let mut parts = Some(parts);
            voice.install(&mut parts);
            if let Some(parts) = parts {
                let _ = voices.returned.push(parts);
                continue;
            }
            if let Some(playing) = states.into_iter().flatten().find(|p| p.slot == place.slot) {
                to_playing(voice, &playing, frames_per_beat);
            }
        }
    }

    /// Builds the voices of `column`'s clip in `slot` now, when no thread has taken the preparer
    /// (an offline render: this allocates). Nothing otherwise: the worker builds them, and
    /// [`Launcher::service_voices`] asks for them.
    pub(super) fn prepare_inline(&mut self, column: usize, slot: usize) {
        if !self.voices.inline() {
            return;
        }
        let Some(plan) = self.columns.get_mut(column).and_then(|c| c.slots.get_mut(slot)).and_then(|s| s.clip.as_mut())
        else {
            return;
        };
        for voice in plan.audio.iter_mut().filter_map(|a| a.stretch.as_mut()) {
            voice.prepare();
        }
    }

    /// At a swap: takes over from `old`, the graph swapped out, the prepared voices of the slots
    /// whose audio did not change (its slot `s` is slot `map(s)` here), each one's claim
    /// permitting; `old` gets this graph's fresh ones back. Then this graph is the one the worker
    /// builds for, and `old` is done with. On the audio thread; never allocates or frees.
    pub(super) fn take_voices(&mut self, old: &mut Launcher, map: impl Fn(usize) -> Option<usize>) {
        old.receive_voices();
        let claims = &self.voices.shared.claims;
        for (new_column, old_column) in self.columns.iter_mut().zip(old.columns.iter_mut()) {
            for (slot, old_slot) in old_column.slots.iter_mut().enumerate() {
                let Some(old_plan) = old_slot.clip.as_mut() else { continue };
                let Some(new_plan) = map(slot).and_then(|s| new_column.slots.get_mut(s)).and_then(|s| s.clip.as_mut())
                else {
                    continue;
                };
                for (new, old) in new_plan.audio.iter_mut().zip(old_plan.audio.iter_mut()) {
                    let ready = old.stretch.as_ref().is_some_and(StretchVoice::is_prepared);
                    let fresh = new.stretch.as_ref().is_some_and(|v| !v.is_prepared());
                    if new.fingerprint != old.fingerprint || !ready || !fresh {
                        continue;
                    }
                    let Some(claim) = new.voice.and_then(|index| claims.get(index)) else { continue };
                    if claim.compare_exchange(OPEN, TAKEN, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                        std::mem::swap(&mut new.stretch, &mut old.stretch);
                    }
                }
            }
        }
        self.voices.shared.live.store(true, Ordering::Release);
        old.voices.shared.retired.store(true, Ordering::Release);
    }
}

/// Moves `voice` to where `playing`'s clip is, when that playback stretches.
pub(super) fn to_playing(voice: &mut StretchVoice, playing: &Playing, frames_per_beat: f64) {
    if playing.stretched {
        voice.seek((playing.elapsed_beats * frames_per_beat).round().max(0.0) as u64);
    }
}

#[cfg(test)]
mod tests;
