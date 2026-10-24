// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sends and returns (ADR 0004 › 11). A channel's sends feed return tracks: after its devices,
//! a track's chain adds its sound to the input of each return it sends to, at the send's level,
//! before (pre-fader) or after (post-fader) its own volume and pan. A return sums what it is
//! sent, runs its devices over the sum and plays into its output (the main channel or a group),
//! like any other chain.
//!
//! Every track with a heard send has a chain (one without devices when it has none) and every
//! return fed by a send has one too, so the send has a buffer to read and one to add to. Chains
//! run in an order where each comes before every chain it plays into, through its output or its
//! sends; a send that would close a loop is left out, so the order always exists. Each send has
//! its own delay line: latency compensation lines up a return's input as it lines up a group's.

use suhina_device_api::ProcessContext;
use suhina_model::{ChannelId, ChannelRole, Project, TrackId};

use crate::chain::{Chain, LiveVolume, MAX_BLOCK};
use crate::plugin::Delay;
use crate::track_gains;

/// The most channels a loop check follows, so a malformed project cannot make it run long.
const MAX_HOPS: usize = 256;

/// A send the engine plays: from `source`'s chain into `target`'s, at `gains` relative to the
/// source's chain buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Planned {
    pub(crate) source: TrackId,
    pub(crate) target: TrackId,
    pub(crate) gains: (f32, f32),
    /// The send's level the gains were worked out for: a live turn scales them by its ratio.
    pub(crate) level: f64,
    /// The source's pan, for a post-fader send: its gains are centred, and the pan applies
    /// as the chain builds it ([`crate::chain::pan_factors`]).
    pub(crate) pan: f64,
    /// After the fader: the channel's live volume ([`Chain::set_live_volume`]) scales it too.
    pub(crate) post_fader: bool,
}

/// One send of a built chain: the return chain's index, the gains and its latency compensation.
pub(crate) struct SendOut {
    pub(crate) target: usize,
    /// The return track, so a graph swap can find the same send in the old graph.
    pub(crate) track: TrackId,
    pub(crate) gains: (f32, f32),
    pub(crate) post_fader: bool,
    pub(crate) delay: Delay,
    /// The level the gains were built for, and the knob's live level against it, smoothed like
    /// a fader ([`Chain::set_live_send`]).
    pub(crate) level: f64,
    pub(crate) live: LiveVolume,
}

impl SendOut {
    /// A send into the chain at `target` (return `track`), at `gains` for the send's `level`,
    /// delayed by `delay` frames. Allocates.
    pub(crate) fn new(
        target: usize,
        track: TrackId,
        (gains, level): ((f32, f32), f64),
        post_fader: bool,
        delay: usize,
    ) -> Self {
        Self { target, track, gains, post_fader, delay: Delay::new(delay), level, live: LiveVolume::default() }
    }
}

/// The sends of `project` that are heard, in channel and send order: from a heard track to a
/// heard return track, at a level above silence, without closing a loop through channel
/// outputs and the sends before it. A send to a channel that is not a return's, or to its own
/// channel, is left out.
pub(crate) fn plan(project: &Project) -> Vec<Planned> {
    let track_of = |channel: ChannelId| project.tracks.iter().find(|t| t.channel == Some(channel)).map(|t| t.id);
    // Edges between channels: outputs, then each accepted send.
    let mut edges: Vec<(ChannelId, ChannelId)> =
        project.channels.iter().filter_map(|c| c.output.map(|o| (c.id, o))).collect();
    let mut planned = Vec::new();
    for channel in &project.channels {
        let Some(source) = track_of(channel.id) else { continue };
        if track_gains(project, source).is_none() {
            continue;
        }
        for send in &channel.sends {
            if send.target == channel.id || !send.level.is_finite() || send.level <= 0.0 {
                continue;
            }
            let Some(target_channel) = project.channel(send.target) else { continue };
            if target_channel.role != ChannelRole::Effect {
                continue;
            }
            let Some(target) = track_of(send.target) else { continue };
            if track_gains(project, target).is_none() || reaches(&edges, send.target, channel.id) {
                continue;
            }
            if planned.iter().any(|p: &Planned| p.source == source && p.target == target) {
                continue;
            }
            edges.push((channel.id, send.target));
            planned.push(Planned {
                source,
                target,
                gains: gains(channel, send.level, send.pre_fader),
                level: send.level,
                // A group's pan does not apply to its own output yet (see `buses`), so not to its sends.
                pan: if channel.pan.is_finite() && channel.role != ChannelRole::Submix {
                    channel.pan.clamp(-1.0, 1.0)
                } else {
                    0.0
                },
                post_fader: !send.pre_fader,
            });
        }
    }
    planned
}

/// The gains a send adds a chain's buffer to its return at. The buffer holds the track's sound
/// as a centred track at unity volume would play it (`CHAIN_INPUT_GAIN`, see `chain`); a post-fader send
/// applies the channel's volume, centred: its equal-power pan comes from the chain
/// ([`Planned::pan`]), as the chain's own output's does.
fn gains(channel: &suhina_model::Channel, level: f64, pre_fader: bool) -> (f32, f32) {
    if pre_fader {
        return (level as f32, level as f32);
    }
    let volume = if channel.volume.is_finite() { channel.volume.max(0.0) } else { 0.0 };
    let centred = (level * volume) as f32;
    (centred, centred)
}

/// Whether `to` can be reached from `from` along `edges`.
pub(crate) fn reaches(edges: &[(ChannelId, ChannelId)], from: ChannelId, to: ChannelId) -> bool {
    let mut seen = vec![from];
    let mut at = 0;
    while let Some(&channel) = seen.get(at) {
        if channel == to {
            return true;
        }
        if seen.len() > MAX_HOPS {
            return false;
        }
        for &(a, b) in edges {
            if a == channel && !seen.contains(&b) {
                seen.push(b);
            }
        }
        at += 1;
    }
    false
}

/// Puts `chains` in an order where every chain comes before the chains `after` names for it
/// (its bus, its returns), keeping their order otherwise. Chains left in a loop (which `plan`
/// and validation keep out) stay at the end in their order. Allocates.
pub(crate) fn order(chains: Vec<Chain>, after: impl Fn(&Chain) -> Vec<TrackId>) -> Vec<Chain> {
    let next: Vec<Vec<usize>> = chains
        .iter()
        .map(|c| after(c).iter().filter_map(|t| chains.iter().position(|o| o.track == *t)).collect())
        .collect();
    let mut incoming = vec![0usize; chains.len()];
    for targets in &next {
        for &t in targets {
            if let Some(n) = incoming.get_mut(t) {
                *n += 1;
            }
        }
    }
    let mut placed = vec![false; chains.len()];
    let mut sequence = Vec::with_capacity(chains.len());
    while let Some(i) = (0..chains.len()).find(|&i| placed.get(i) == Some(&false) && incoming.get(i) == Some(&0)) {
        if let Some(p) = placed.get_mut(i) {
            *p = true;
        }
        sequence.push(i);
        for &t in next.get(i).map_or(&[][..], Vec::as_slice) {
            if let Some(n) = incoming.get_mut(t) {
                *n = n.saturating_sub(1);
            }
        }
    }
    sequence.extend((0..chains.len()).filter(|&i| placed.get(i) == Some(&false)));
    let mut slots: Vec<Option<Chain>> = chains.into_iter().map(Some).collect();
    sequence.into_iter().filter_map(|i| slots.get_mut(i).and_then(Option::take)).collect()
}

/// A source render's capture: the chain whose output the render reads, which chains are
/// inside the source node's subtree (`keep[i]` is chain `i`'s; the returns the subtree sends
/// to are never inside), and the block buffer the source chain's output is mixed into.
pub(crate) struct Capture<'a> {
    pub(crate) chain: usize,
    pub(crate) keep: &'a [bool],
    pub(crate) buffer: &'a mut [f32],
}

/// Runs every chain over the rest of a block of `frames` frames, in order, so a bus or a
/// return has everything that plays into it before it runs: each chain adds its sound to its
/// returns, then to its bus or `out`, and, when its cue is on, to the cue bus. With `capture`,
/// a source render: the named chain's output is mixed into the capture buffer instead of where
/// it plays, and a chain outside the source node's subtree adds nothing into a chain inside
/// it (a return routed into the node's bus plays into the mix, not into the node's file).
/// Never allocates.
pub(crate) fn finish(
    chains: &mut [Chain],
    out: &mut [f32],
    cue: &mut [f32],
    frames: usize,
    context: &ProcessContext,
    mut capture: Option<Capture>,
) {
    // The capture buffer starts empty; the source chain's mix adds to it.
    if let Some(cap) = capture.as_mut()
        && let Some(buffer) = cap.buffer.get_mut(..(frames.min(MAX_BLOCK) * 2).min(cap.buffer.len()))
    {
        buffer.fill(0.0);
    }
    for i in 0..chains.len() {
        let (done, later) = chains.split_at_mut(i + 1);
        let Some(chain) = done.last_mut() else { continue };
        chain.complete(frames, context);
        // Whether chain `i` is inside the source node's subtree; every chain is, when there is
        // no capture (a normal render).
        let inside = capture.as_ref().is_none_or(|c| c.keep.get(i).copied().unwrap_or(true));
        for k in 0..chain.sends.len() {
            let target = chain.sends.get(k).and_then(|s| s.target.checked_sub(i + 1));
            let Some(ret) = target.and_then(|t| later.get_mut(t)) else { continue };
            if !inside
                && target
                    .is_some_and(|t| capture.as_ref().is_some_and(|c| c.keep.get(i + 1 + t).copied().unwrap_or(true)))
            {
                continue;
            }
            chain.send(k, ret.input(frames), frames);
        }
        // The source chain's output goes into the capture buffer instead of where it plays; its
        // latency compensation, gains and mixer lanes are the same `mix` applies.
        if let Some(cap) = capture.as_mut()
            && cap.chain == i
        {
            chain.mix(&mut *cap.buffer, cue, frames);
            continue;
        }
        let target = chain.target.and_then(|t| t.checked_sub(i + 1));
        if !inside
            && target.is_some_and(|t| capture.as_ref().is_some_and(|c| c.keep.get(i + 1 + t).copied().unwrap_or(true)))
        {
            continue;
        }
        match target.and_then(|t| later.get_mut(t)) {
            Some(bus) => chain.mix(bus.input(frames), cue, frames),
            None => chain.mix(out, cue, frames),
        }
    }
}

impl Chain {
    /// Adds the chain's sound, as its devices left it, through send `index`'s delay and at its
    /// gains, to `input` (the return's). Never allocates.
    pub(crate) fn send(&mut self, index: usize, input: &mut [f32], frames: usize) {
        let end = frames.min(MAX_BLOCK) * 2;
        let (Some(send), Some(scratch), Some(buffer)) =
            (self.sends.get_mut(index), self.scratch.get_mut(..end), self.buffer.get(..end))
        else {
            return;
        };
        scratch.copy_from_slice(buffer);
        send.delay.run(scratch);
        let (gain_left, gain_right) = send.gains;
        let post_fader = send.post_fader;
        let pan = self.pan;
        let live = post_fader.then_some(self.live);
        let knob = send.live;
        for (frame, ([l, r], [sl, sr])) in
            input.as_chunks_mut::<2>().0.iter_mut().zip(scratch.as_chunks::<2>().0).enumerate()
        {
            let fader = live.map_or(1.0, |live| live.at(frame)) * knob.at(frame);
            // After the fader, the pan's live turn too, as the chain's own output has it.
            let (pan_left, pan_right) = if post_fader { (pan.0.at(frame), pan.1.at(frame)) } else { (1.0, 1.0) };
            *l += sl * gain_left * fader * pan_left;
            *r += sr * gain_right * fader * pan_right;
        }
    }

    /// Takes a turn of the send to return `target` to `level` (linear, at most +6 dB) from the
    /// next block, smoothed; ignored for a send the graph does not have, and for a level whose
    /// ratio to the built one is not a finite `f32`. Never allocates.
    pub(crate) fn set_live_send(&mut self, target: TrackId, level: f64) {
        if let Some(send) = self.sends.iter_mut().find(|s| s.track == target)
            && send.level > 0.0
            && level.is_finite()
            && level >= 0.0
            && let Some(ratio) = crate::chain::live_ratio(level, send.level)
        {
            send.live.target = ratio;
        }
    }

    /// The returns this chain sends to, as a new graph has them: a turn of their knob reaches
    /// them as a message ([`crate::MIXER_SEND`]).
    pub(crate) fn live_sends(&self) -> impl Iterator<Item = TrackId> + '_ {
        self.sends.iter().filter(|s| s.level > 0.0).map(|s| s.track)
    }

    /// Plans the sends' smoothing for the next block of `frames` frames. Never allocates.
    pub(crate) fn plan_sends(&mut self, frames: usize) {
        let sample_rate = self.sample_rate;
        for send in &mut self.sends {
            send.live.plan(frames, sample_rate);
        }
    }

    /// Takes over the delay lines of `old`'s sends to the same returns. Never allocates.
    pub(crate) fn adopt_sends(&mut self, old: &mut Chain) {
        for send in &mut self.sends {
            if let Some(previous) = old.sends.iter_mut().find(|s| s.track == send.track) {
                send.delay.adopt(&mut previous.delay);
            }
        }
    }

    /// Gives the chain its sends and the buffer they read through. Allocates.
    pub(crate) fn set_sends(&mut self, sends: Vec<SendOut>) {
        self.scratch = if sends.is_empty() { Vec::new() } else { vec![0.0; MAX_BLOCK * 2] };
        self.sends = sends;
    }
}
