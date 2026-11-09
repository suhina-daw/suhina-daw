// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sends and returns (ADR 0004 › 12). A channel's sends feed return tracks: after its devices,
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
    /// What it adds to its return this block, staged when its chain is done ([`Chain::stage`]).
    pub(crate) staged: Vec<f32>,
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
        Self {
            target,
            track,
            gains,
            post_fader,
            delay: Delay::new(delay),
            level,
            live: LiveVolume::default(),
            staged: vec![0.0; MAX_BLOCK * 2],
        }
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

impl Capture<'_> {
    /// Whether chain `i` is inside the source node's subtree.
    fn inside(&self, i: usize) -> bool {
        self.keep.get(i).copied().unwrap_or(true)
    }
}

/// The most bus levels with plug-ins a chain's deadline is shortened for: its plug-ins keep at
/// least a quarter of the block's time.
const MAX_LEVELS: u32 = 3;

/// When a chain runs in a block (#1261). Built with the graph: how many chains play into it and
/// how many bus levels with plug-ins come after it; and this block's progress.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Turn {
    /// Chains that play into it, through their output or a send. It starts once they are done.
    pub(crate) feeders: usize,
    /// Buses with plug-ins switched on, on the longest way from it to the output, at most [`MAX_LEVELS`]:
    /// its plug-ins are waited for that many shares of the deadline sooner, so those buses
    /// still have time when one of its plug-ins is late.
    pub(crate) levels_after: u32,
    /// This block: feeders not done yet; whether it has started and whether it is done.
    left: usize,
    started: bool,
    done: bool,
}

impl Turn {
    /// The time its plug-ins have in a block that began at `start`, with plug-in blocks due by
    /// `deadline`: a quarter of that time less for each bus level with plug-ins after it.
    pub(crate) fn limit(&self, start: std::time::Instant, deadline: std::time::Instant) -> std::time::Instant {
        let levels = self.levels_after.min(MAX_LEVELS);
        let time = deadline.saturating_duration_since(start);
        deadline.checked_sub(time / 4 * levels).unwrap_or(deadline)
    }
}

/// What a done chain adds where it plays, before it is added (see [`Chain::stage`]).
pub(crate) struct Staged {
    post: Vec<f32>,
    cue: Vec<f32>,
}

impl Staged {
    /// Room for a block. Allocates.
    pub(crate) fn new() -> Self {
        Self { post: vec![0.0; MAX_BLOCK * 2], cue: vec![0.0; MAX_BLOCK * 2] }
    }
}

/// Counts every chain's feeders and its bus levels after it, for [`run`]: Main's chain, when
/// there is one, is what every chain without a bus plays into. Allocates.
pub(crate) fn plan_turns(chains: &mut [Chain]) {
    let main = chains.iter().position(|c| c.track == suhina_model::TrackId::MAIN);
    let targets = |c: &Chain| -> Vec<usize> {
        let mut t: Vec<usize> = c.sends.iter().map(|s| s.target).chain(c.target).collect();
        if c.target.is_none()
            && let Some(main) = main
            && c.track != suhina_model::TrackId::MAIN
        {
            t.push(main);
        }
        t.sort_unstable();
        t.dedup();
        t
    };
    let all: Vec<Vec<usize>> = chains.iter().map(targets).collect();
    // A switched-off plug-in passes the sound and sends no block, so it needs no time.
    let plugins: Vec<bool> = chains.iter_mut().map(|c| c.plugin_slots().any(|s| s.enabled)).collect();
    let mut levels = vec![0u32; chains.len()];
    for i in (0..chains.len()).rev() {
        let after = all.get(i).map_or(&[][..], Vec::as_slice);
        let level = after
            .iter()
            .filter(|&&t| t > i)
            .map(|&t| levels.get(t).copied().unwrap_or(0) + u32::from(plugins.get(t).copied().unwrap_or(false)))
            .max()
            .unwrap_or(0);
        if let Some(slot) = levels.get_mut(i) {
            *slot = level;
        }
    }
    for (i, chain) in chains.iter_mut().enumerate() {
        // Main is fed by what reaches the output; it starts once every other chain is done.
        let feeders = if Some(i) == main { 0 } else { all.iter().filter(|t| t.contains(&i)).count() };
        chain.turn =
            Turn { feeders, levels_after: levels.get(i).copied().unwrap_or(0).min(MAX_LEVELS), ..Turn::default() };
    }
}

/// Runs every chain over the rest of a block of `frames` frames, each as soon as the chains that
/// play into it are done (#1261): the chains no chain plays into start at once, a bus or a
/// return once its last feeder is done, and Main, last, once every other chain is. Plug-ins'
/// blocks are out together: a chain goes on as soon as its own plug-in answers and waits for no
/// other chain's (#151), only until its deadline ([`Turn::limit`]).
///
/// What a done chain adds to its bus, its returns, the output and the cue bus is staged first,
/// then added in chain order, as when the chains ran one after another: which plug-in answered
/// first never changes a sum, so offline renders stay the engine's oracle. With `capture`, a
/// source render: the named chain's output is mixed into the capture buffer instead of where it
/// plays, and a chain outside the source node's subtree adds nothing into a chain inside it (a
/// return routed into the node's bus plays into the mix, not into the node's file). Never
/// allocates; in real time it returns by the deadline.
pub(crate) fn run(
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
    let main = chains.iter().position(|c| c.track == suhina_model::TrackId::MAIN);
    for chain in chains.iter_mut() {
        chain.turn.left = chain.turn.feeders;
        chain.turn.started = false;
        chain.turn.done = false;
    }
    for i in 0..chains.len() {
        if Some(i) != main && chains.get(i).is_some_and(|c| c.turn.feeders == 0) {
            start(chains, i, frames, context, capture.as_ref());
        }
    }
    loop {
        let now = crate::clock::now();
        let mut moved = false;
        for i in 0..chains.len() {
            let Some(chain) = chains.get_mut(i).filter(|c| c.turn.started && !c.turn.done) else { continue };
            if chain.is_waiting() && chain.answered(now) {
                chain.collect(frames);
                chain.advance(frames, context);
                moved = true;
            }
            if !chain.is_waiting() {
                done(chains, i, frames, context, capture.as_ref());
                moved = true;
            }
        }
        let others_done = chains.iter().enumerate().all(|(i, c)| Some(i) == main || c.turn.done);
        if others_done {
            match main.and_then(|m| chains.get(m).map(|c| (m, c.turn))) {
                Some((m, turn)) if !turn.started => {
                    to_output(chains, out, cue, Some(m), false, capture.as_mut());
                    // What reached the output so far is Main's input, and its output is the
                    // main output (#687).
                    if let Some(chain) = chains.get_mut(m) {
                        for (to, from) in chain.input(frames).iter_mut().zip(out.iter_mut()) {
                            *to += *from;
                            *from = 0.0;
                        }
                    }
                    start(chains, m, frames, context, capture.as_ref());
                    continue;
                }
                Some((_, turn)) if !turn.done => {}
                _ => break,
            }
        }
        if moved {
            continue;
        }
        // A chain whose feeders can never all be done (a loop the graph should not have) starts
        // anyway, in order, rather than holding up the block.
        if !chains.iter().any(|c| c.turn.started && !c.turn.done)
            && let Some(i) =
                (0..chains.len()).find(|&i| Some(i) != main && chains.get(i).is_some_and(|c| !c.turn.started))
        {
            start(chains, i, frames, context, capture.as_ref());
            continue;
        }
        wait_for_answers(chains, now);
    }
    match main {
        Some(m) => to_output(chains, out, cue, Some(m), true, capture.as_mut()),
        None => to_output(chains, out, cue, None, false, capture.as_mut()),
    }
}

/// The most chains' plug-ins one wait watches; more chains wait in turn.
const MAX_WATCHED: usize = 64;

/// While no plug-in has answered: sleeps in the operating system until one of the waiting
/// chains' plug-ins answers or the earliest of their limits passes (#1256). When one of them
/// cannot be watched, it waits on the first chain still out for at most
/// [`crate::chain::ANSWER_SLICE`] instead, and the loop looks at all of them again. Never
/// allocates.
fn wait_for_answers(chains: &mut [Chain], now: std::time::Instant) {
    let waiting = |c: &&Chain| c.turn.started && !c.turn.done && c.is_waiting();
    let all = chains.iter().filter(waiting).count();
    let mut watches: [Option<suhina_ipc::Watch>; MAX_WATCHED] = [None; MAX_WATCHED];
    let mut until: Option<std::time::Instant> = None;
    let mut watched = 0;
    for chain in chains.iter().filter(waiting).take(MAX_WATCHED) {
        if let (Some(watch), Some(slot)) = (chain.watch(), watches.get_mut(watched)) {
            *slot = Some(watch);
            watched += 1;
        }
        until = match (until, chain.limit()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    if watched > 0
        && watched == all
        && let Some(until) = until
    {
        suhina_ipc::wait_any(watches.get(..watched).unwrap_or_default(), until);
        return;
    }
    if let Some(chain) = chains.iter_mut().find(|c| c.turn.started && !c.turn.done && c.is_waiting()) {
        chain.answered(now + crate::chain::ANSWER_SLICE);
    }
}

/// Starts chain `i`'s block: what its feeders staged for it goes into its input, in their order,
/// and its devices run as far as they can without waiting ([`Chain::advance`]).
fn start(chains: &mut [Chain], i: usize, frames: usize, context: &ProcessContext, capture: Option<&Capture>) {
    let (before, rest) = chains.split_at_mut(i.min(chains.len()));
    let Some(chain) = rest.first_mut() else { return };
    let inside = |j: usize| capture.is_none_or(|c| c.inside(j));
    // Nothing from outside the source node's subtree goes into a chain inside it.
    let shut = |j: usize| !inside(j) && capture.is_some_and(|c| c.inside(i));
    for (j, feeder) in before.iter().enumerate() {
        if shut(j) {
            continue;
        }
        for send in feeder.sends.iter().filter(|s| s.target == i) {
            add(chain.input(frames), &send.staged);
        }
        let captured = capture.is_some_and(|c| c.chain == j);
        if feeder.target == Some(i) && !captured {
            add(chain.input(frames), &feeder.staged.post);
        }
    }
    chain.turn.started = true;
    chain.advance(frames, context);
}

/// Chain `i` has run its devices: the rest of its block runs, what it adds where it plays is
/// staged, and every chain it plays into that has all its feeders done now starts.
fn done(chains: &mut [Chain], i: usize, frames: usize, context: &ProcessContext, capture: Option<&Capture>) {
    let Some(chain) = chains.get_mut(i) else { return };
    chain.complete(frames, context);
    chain.stage(frames);
    chain.turn.done = true;
    let (target, sends) = (chain.target, chain.sends.len());
    for k in 0..=sends {
        let to = if k < sends { chains.get(i).and_then(|c| c.sends.get(k)).map(|s| s.target) } else { target };
        // A send and the output into the same chain count once, as `plan_turns` counts them.
        let repeated = (0..k).any(|e| chains.get(i).and_then(|c| c.sends.get(e)).map(|s| s.target) == to);
        let Some(t) = to.filter(|_| !repeated) else { continue };
        let Some(next) = chains.get_mut(t) else { continue };
        next.turn.left = next.turn.left.saturating_sub(1);
        if next.turn.left == 0 && !next.turn.started && next.track != suhina_model::TrackId::MAIN {
            start(chains, t, frames, context, capture);
        }
    }
}

/// Adds what the chains staged for the output and the cue bus, in their order: every chain but
/// Main when `main` names it and `last` is false, Main alone when `last` is true, every chain
/// without `main`. The capture chain's output goes to the capture buffer instead.
fn to_output(
    chains: &mut [Chain],
    out: &mut [f32],
    cue: &mut [f32],
    main: Option<usize>,
    last: bool,
    mut capture: Option<&mut Capture>,
) {
    for (i, chain) in chains.iter().enumerate() {
        if main.is_some() && (Some(i) == main) != last {
            continue;
        }
        match capture.as_mut().filter(|c| c.chain == i) {
            Some(cap) => add(cap.buffer, &chain.staged.post),
            None if chain.target.is_none() => add(out, &chain.staged.post),
            None => {}
        }
        if chain.cue.is_some() {
            add(cue, &chain.staged.cue);
        }
    }
}

/// `to += from`, sample by sample, over the shorter of the two.
fn add(to: &mut [f32], from: &[f32]) {
    for (to, from) in to.iter_mut().zip(from) {
        *to += *from;
    }
}

impl Chain {
    /// Stages what the chain adds where it plays, once its devices have run the block: each
    /// send's share for its return, then its output for its bus or the output and its cue, as
    /// [`Chain::send`] and [`Chain::mix`] would add them. Each starts at -0.0, which adding
    /// leaves every value as it is, so adding the staged sound later gives the same samples, bit
    /// for bit. Never allocates.
    pub(crate) fn stage(&mut self, frames: usize) {
        let end = frames.min(MAX_BLOCK) * 2;
        for k in 0..self.sends.len() {
            let Some(send) = self.sends.get_mut(k) else { continue };
            let mut staged = std::mem::take(&mut send.staged);
            let part = staged.get_mut(..end).unwrap_or_default();
            part.fill(-0.0);
            self.send(k, part, frames);
            if let Some(send) = self.sends.get_mut(k) {
                send.staged = staged;
            }
        }
        let mut staged = std::mem::replace(&mut self.staged, Staged { post: Vec::new(), cue: Vec::new() });
        for buffer in [&mut staged.post, &mut staged.cue] {
            buffer.get_mut(..end).unwrap_or_default().fill(-0.0);
        }
        let (post, cue) =
            (staged.post.get_mut(..end).unwrap_or_default(), staged.cue.get_mut(..end).unwrap_or_default());
        self.mix(post, cue, frames);
        self.staged = staged;
    }

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
