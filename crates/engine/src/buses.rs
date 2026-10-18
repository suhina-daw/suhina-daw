// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group buses (ADR 0004 › 10). A group track whose channel holds devices sums what plays into
//! it and runs its devices over the sum, as a compressor on a group must. Every track that plays
//! into a bus has a chain (one without devices when it has none), so its sound reaches the bus
//! through a delay line and latency compensation lines up every path; the bus chains come after
//! the chains that feed them, so in a block they run after them.
//!
//! A group without devices stays what it was: its volume and mute scale the tracks routed
//! through it (`track_gains`), with no buffer of its own.

use suhina_model::{ChannelId, ChannelRole, Project, TrackId};

use crate::automation::{self, MixLanes};
use crate::chain::Chain;
use crate::plugin::{Delay, Plugins};
use crate::sends::{self, SendOut};
use crate::{MAX_LATENCY_SECONDS, track_gains};

/// A send of a chain: the return chain's index, the return track and the send's gains.
type SendTarget = (usize, TrackId, (f32, f32));

/// The most channels a route is followed through, so a routing cycle cannot loop forever.
const MAX_HOPS: usize = 64;

/// The chains of `project`'s tracks, ordered so every bus comes after the chains that feed it,
/// with their targets, gains and latency compensation set; and the latency every path is
/// brought up to. Allocates: call it off the audio thread.
pub(crate) fn build(
    project: &Project,
    sample_rate: u32,
    plugins: &mut Plugins,
    column_of: impl Fn(TrackId) -> u32,
    assets: &crate::DeviceAssets,
) -> (Vec<Chain>, u32) {
    // Every track with devices has a chain, a silent one too (muted, or not soloed): its
    // devices keep running at a gain of zero, so a plug-in keeps its state and its process,
    // and sounds again as soon as the track does. A chain that was left out of a graph would
    // give its plug-in's processor back, and the plug-in would fall silent for good.
    let mut chains: Vec<Chain> = project
        .tracks
        .iter()
        .filter_map(|track| {
            Chain::build(project, track.id, column_of(track.id), (0.0, 0.0), sample_rate, plugins, assets)
        })
        .collect();
    // A track whose mixer follows lanes applies its gains after a chain, one without devices
    // when it has none (#395); a group's chain makes it a bus.
    for track in &project.tracks {
        if chains.iter().all(|c| c.track != track.id) && automation::mixer_automated(project, track.id) {
            chains.push(Chain::passthrough(track.id, column_of(track.id)));
        }
    }

    // Sends (see `sends`): every track with a heard send, and every return fed by one, has a
    // chain, so the send has a buffer to read and one to add to.
    let planned = sends::plan(project);
    for track in &project.tracks {
        if chains.iter().all(|c| c.track != track.id)
            && planned.iter().any(|p| p.source == track.id || p.target == track.id)
        {
            chains.push(Chain::passthrough(track.id, column_of(track.id)));
        }
    }

    // A track whose cue is on has a chain too, a passthrough when it has no devices, so its
    // sound is there on its own to add to the cue bus.
    for track in &project.tracks {
        if cued(project, track.id)
            && chains.iter().all(|c| c.track != track.id)
            && track_gains(project, track.id).is_some()
        {
            chains.push(Chain::passthrough(track.id, column_of(track.id)));
        }
    }

    // A bus: a track with a chain that some other channel plays into.
    let channel_of = |track: TrackId| project.tracks.iter().find(|t| t.id == track).and_then(|t| t.channel);
    let feeds = |channel: ChannelId| project.channels.iter().any(|c| c.id != channel && c.output == Some(channel));
    let buses: Vec<(TrackId, ChannelId)> =
        chains.iter().filter_map(|c| channel_of(c.track).filter(|&ch| feeds(ch)).map(|ch| (c.track, ch))).collect();
    // The buses a track's sound passes through after its own channel, nearest first.
    let buses_after = |track: TrackId| -> Vec<(TrackId, ChannelId)> {
        let mut out = Vec::new();
        let mut visited = Vec::new();
        let mut next =
            channel_of(track).and_then(|ch| project.channels.iter().find(|c| c.id == ch)).and_then(|c| c.output);
        while let Some(id) = next.take() {
            if visited.len() > MAX_HOPS || visited.contains(&id) {
                break;
            }
            visited.push(id);
            if let Some(&bus) = buses.iter().find(|(_, ch)| *ch == id) {
                out.push(bus);
            }
            next = project.channels.iter().find(|c| c.id == id).and_then(|c| c.output);
        }
        out
    };

    if !buses.is_empty() {
        for track in &project.tracks {
            if chains.iter().all(|c| c.track != track.id)
                && !buses_after(track.id).is_empty()
                && track_gains(project, track.id).is_some()
            {
                chains.push(Chain::passthrough(track.id, column_of(track.id)));
            }
        }
    }
    // Every chain before the bus it plays into and the returns it sends to.
    let chains = sends::order(chains, |c| {
        buses_after(c.track)
            .first()
            .map(|(bus, _)| *bus)
            .into_iter()
            .chain(planned.iter().filter(|p| p.source == c.track).map(|p| p.target))
            .collect()
    });
    let mut chains = chains;

    let targets: Vec<Option<(usize, ChannelId)>> = chains
        .iter()
        .map(|c| {
            let (bus, channel) = *buses_after(c.track).first()?;
            Some((chains.iter().position(|o| o.track == bus)?, channel))
        })
        .collect();
    let send_targets: Vec<Vec<SendTarget>> = chains
        .iter()
        .map(|c| {
            planned
                .iter()
                .filter(|p| p.source == c.track)
                .filter_map(|p| Some((chains.iter().position(|o| o.track == p.target)?, p.target, p.gains)))
                .collect()
        })
        .collect();
    for (chain, target) in chains.iter_mut().zip(&targets) {
        let is_bus = buses.iter().any(|(t, _)| *t == chain.track);
        // A group's pan does not apply (as for a group without devices); a track's does.
        let post = gains_until(project, chain.track, target.map(|(_, ch)| ch), !is_bus).unwrap_or((0.0, 0.0));
        chain.set_post(post);
        chain.target = target.map(|(index, _)| index);
        chain.set_cue(if cued(project, chain.track) { cue_gains(project, chain.track, !is_bus) } else { None });
        let frames_per_beat = 60.0 / project.transport.tempo_bpm * f64::from(sample_rate);
        chain.mix = automation::channel_of(project, chain.track)
            .and_then(|channel| MixLanes::new(channel, frames_per_beat, !is_bus));
    }
    for index in 0..chains.len() {
        let fed = targets.iter().any(|t| t.is_some_and(|(target, _)| target == index))
            || send_targets.iter().flatten().any(|(target, _, _)| *target == index);
        if let Some(chain) = chains.get_mut(index) {
            chain.is_bus = fed;
        }
    }

    // Latency compensation. A chain's sources start without delay; a bus's or a return's input
    // is the latest of the chains that feed it, each delayed to it; everything reaching the output is delayed
    // to the latest path, bounded so a plug-in that reports nonsense cannot take all memory.
    let max_latency = sample_rate.saturating_mul(MAX_LATENCY_SECONDS);
    let mut arrive = vec![0u32; chains.len()];
    let mut input = vec![0u32; chains.len()];
    for i in 0..chains.len() {
        let own = chains.get(i).map_or(0, |c| c.latency.min(max_latency));
        let at = input.get(i).copied().unwrap_or(0).saturating_add(own).min(max_latency);
        if let Some(slot) = arrive.get_mut(i) {
            *slot = at;
        }
        let fed = targets.get(i).copied().flatten().map(|(target, _)| target).into_iter();
        let sent = send_targets.get(i).into_iter().flatten().map(|(target, _, _)| *target);
        for target in fed.chain(sent) {
            if let Some(slot) = input.get_mut(target) {
                *slot = (*slot).max(at);
            }
        }
    }
    let latency = (0..chains.len())
        .filter(|&i| matches!(targets.get(i), Some(None)))
        .map(|i| arrive.get(i).copied().unwrap_or(0))
        .max()
        .unwrap_or(0);
    for (i, chain) in chains.iter_mut().enumerate() {
        let arrived = arrive.get(i).copied().unwrap_or(0);
        let to = match targets.get(i) {
            Some(Some((target, _))) => input.get(*target).copied().unwrap_or(0),
            _ => latency,
        };
        chain.delay = Delay::new(to.saturating_sub(arrived) as usize);
        if let Some(mix) = chain.mix.as_mut() {
            mix.set_latency(to);
        }
        let sends = send_targets
            .get(i)
            .into_iter()
            .flatten()
            .map(|&(target, track, gains)| {
                let delay = input.get(target).copied().unwrap_or(0).saturating_sub(arrived);
                SendOut::new(target, track, gains, delay as usize)
            })
            .collect();
        chain.set_sends(sends);
    }
    (chains, latency)
}

/// Whether `track`'s channel has its cue on.
fn cued(project: &Project, track: TrackId) -> bool {
    let channel = project.tracks.iter().find(|t| t.id == track).and_then(|t| t.channel);
    project.channels.iter().any(|c| Some(c.id) == channel && c.cue)
}

/// The gains a cued track is heard at on the cue bus: its own channel's volume and, when `pan`,
/// pan, but not the groups' or the main channel's, so the main mix's level does not change
/// what is heard on the cue. `None` when the track is not heard (muted, or another is soloed).
/// Scaled like `track_gains`.
pub(crate) fn cue_gains(project: &Project, track: TrackId, pan: bool) -> Option<(f32, f32)> {
    track_gains(project, track)?;
    let channel_id = project.tracks.iter().find(|t| t.id == track)?.channel?;
    let channel = project.channels.iter().find(|c| c.id == channel_id)?;
    let volume = if channel.volume.is_finite() { channel.volume.max(0.0) } else { 0.0 };
    let pan = if pan && channel.pan.is_finite() { channel.pan.clamp(-1.0, 1.0) } else { 0.0 };
    let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
    let scale = volume * std::f64::consts::SQRT_2 * 0.25;
    Some(((angle.cos() * scale) as f32, (angle.sin() * scale) as f32))
}

/// A track's equal-power pan (when `pan`) and the volume of every channel from its own up to,
/// not including, `until`; with `until` `None`, up to and including the master. `None` when the
/// track is not heard. Scaled like `track_gains`, which this equals for `until` `None`.
pub(crate) fn gains_until(
    project: &Project,
    track: TrackId,
    until: Option<ChannelId>,
    pan: bool,
) -> Option<(f32, f32)> {
    track_gains(project, track)?;
    let channel_id = project.tracks.iter().find(|t| t.id == track)?.channel?;
    let channel = project.channels.iter().find(|c| c.id == channel_id)?;
    let gain = |c: &suhina_model::Channel| if c.volume.is_finite() { c.volume.max(0.0) } else { 0.0 };
    let mut volume = 1.0;
    let mut visited = Vec::new();
    let mut next = Some(channel);
    while let Some(c) = next.take() {
        if Some(c.id) == until || visited.len() > MAX_HOPS || visited.contains(&c.id) {
            break;
        }
        visited.push(c.id);
        volume *= gain(c);
        next = c.output.and_then(|id| project.channels.iter().find(|o| o.id == id));
    }
    if until.is_none()
        && let Some(master) =
            project.channels.iter().find(|c| c.role == ChannelRole::Master && !visited.contains(&c.id))
    {
        volume *= gain(master);
    }
    let pan = if pan && channel.pan.is_finite() { channel.pan.clamp(-1.0, 1.0) } else { 0.0 };
    let angle = (pan + 1.0) * std::f64::consts::FRAC_PI_4;
    let scale = volume * std::f64::consts::SQRT_2 * 0.25;
    Some(((angle.cos() * scale) as f32, (angle.sin() * scale) as f32))
}
