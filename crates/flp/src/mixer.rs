// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Map mixer ownership to source tracks, shared groups and send destinations.
use std::collections::{BTreeMap, BTreeSet};
use suhina_model::{ChannelId, ChannelRole, DeviceKind, Extension, Project, TrackId};

const MIXER: u32 = 131_072;

pub(crate) fn apply(import: &mut crate::Import) {
    let p = &mut import.project;
    let mut needed: BTreeSet<_> =
        p.channels.iter().filter(|c| c.id.0 < MIXER || c.role == ChannelRole::Master).map(|c| c.id).collect();
    let mut pending: Vec<_> = needed.iter().copied().collect();
    while let Some(id) = pending.pop() {
        if let Some(channel) = p.channel(id) {
            // A track a key reads is needed too, though it may play nowhere (sidechain only).
            let keys: Vec<ChannelId> = suhina_model::walk_devices(&channel.devices)
                .iter()
                .flat_map(|(_, d)| d.inputs.iter().map(|i| i.source.channel()))
                .collect();
            for next in channel.output.into_iter().chain(channel.sends.iter().map(|s| s.target)).chain(keys) {
                if needed.insert(next) {
                    pending.push(next);
                }
            }
        }
    }
    p.channels.retain(|c| needed.contains(&c.id));
    p.tracks.retain(|t| t.channel.is_none_or(|id| needed.contains(&id)));
    let mut incoming: BTreeMap<ChannelId, BTreeSet<ChannelId>> = BTreeMap::new();
    for channel in &p.channels {
        for target in channel.output.into_iter().chain(channel.sends.iter().map(|s| s.target)) {
            incoming.entry(target).or_default().insert(channel.id);
        }
    }
    let candidates: Vec<_> = p
        .channels
        .iter()
        .filter(|c| c.id.0 < 65_536 && c.sends.is_empty() && c.volume == 1. && c.pan == 0. && c.automation.is_empty())
        .filter_map(|c| c.output.map(|target| (c.id, target)))
        .filter(|(source, target)| {
            target.0 > MIXER
                && target.0 < MIXER + 501
                && incoming.get(target).is_some_and(|ids| ids.len() == 1 && ids.contains(source))
        })
        .collect();
    let mut folded = BTreeSet::new();
    let mut into: BTreeMap<ChannelId, ChannelId> = BTreeMap::new();
    for (source_id, mixer_id) in candidates {
        let Some(mixer) = p.channel(mixer_id).cloned() else { continue };
        let Some(source) = p.channel_mut(source_id) else { continue };
        if suhina_model::walk_devices(&source.devices).len() + suhina_model::walk_devices(&mixer.devices).len()
            > suhina_model::MAX_DEVICES
        {
            continue;
        }
        if source.mute {
            let Some(utility) = source.devices.last_mut().filter(|d| d.kind == DeviceKind::Builtin("utility".into()))
            else {
                continue;
            };
            let Some(mute) = utility.params.iter_mut().find(|p| p.id == "mute") else { continue };
            mute.value = 1.;
        }
        source.devices.extend(mixer.devices);
        source.volume = mixer.volume;
        source.pan = mixer.pan;
        source.mute = mixer.mute;
        source.solo = mixer.solo;
        source.output = mixer.output;
        source.sends = mixer.sends;
        source.automation = mixer.automation;
        source.extensions.0.push(Extension {
            format: "flp".into(),
            key: "source-mixer".into(),
            data: mixer_id.0.to_le_bytes().to_vec(),
        });
        folded.insert(mixer_id);
        into.insert(mixer_id, source_id);
    }
    p.channels.retain(|c| !folded.contains(&c.id));
    p.tracks.retain(|t| t.channel.is_none_or(|id| !folded.contains(&id)));
    // A key read from a mixer insert that folded into its source reads the source now; one whose
    // track is gone reads nothing (the device hears its own input).
    let present: BTreeSet<ChannelId> = p.channels.iter().map(|c| c.id).collect();
    for channel in &mut p.channels {
        suhina_model::visit_devices_mut(&mut channel.devices, &mut |device| {
            for input in &mut device.inputs {
                if let Some(&to) = into.get(&input.source.channel()) {
                    input.source = suhina_model::SignalPoint::Channel { channel: to, tap: input.source.tap() };
                }
            }
            device.inputs.retain(|i| present.contains(&i.source.channel()));
        });
    }
    // A key that now reads its own track after its devices, or closes a loop, is left out.
    let looped: Vec<(ChannelId, Vec<usize>)> = p
        .channels
        .iter()
        .flat_map(|c| {
            suhina_model::walk_devices(&c.devices)
                .into_iter()
                .filter(|(_, d)| d.inputs.iter().any(|i| p.key_closes_loop(i.source, c.id)))
                .map(|(path, _)| (c.id, path))
                .collect::<Vec<_>>()
        })
        .collect();
    for (channel, path) in looped {
        if let Some(device) = p.channel_mut(channel).and_then(|c| suhina_model::device_at_mut(&mut c.devices, &path)) {
            device.inputs.clear();
        }
    }
    let primary: BTreeSet<_> = p.channels.iter().filter_map(|c| c.output).collect();
    for channel in &mut p.channels {
        if channel.id.0 > MIXER && channel.id.0 < MIXER + 501 && primary.contains(&channel.id) {
            channel.role = ChannelRole::Submix;
        }
    }
    group_tracks(p);
}

fn group_tracks(p: &mut Project) {
    let groups: BTreeMap<_, _> = p
        .tracks
        .iter()
        .filter_map(|t| {
            let channel = p.channel_of_track(t.id)?;
            (channel.role == ChannelRole::Submix).then_some((channel.id, t.id))
        })
        .collect();
    let parents: BTreeMap<_, _> = p
        .tracks
        .iter()
        .map(|t| {
            let parent = p
                .channel_of_track(t.id)
                .filter(|c| c.role != ChannelRole::Effect)
                .and_then(|c| c.output)
                .and_then(|id| groups.get(&id).copied());
            (t.id, parent)
        })
        .collect();
    for track in &mut p.tracks {
        let mut next = parents.get(&track.id).copied().flatten();
        let mut depth = 0;
        while let Some(id) = next {
            depth += 1;
            if depth > suhina_model::MAX_GROUP_DEPTH || id == track.id {
                break;
            }
            next = parents.get(&id).copied().flatten();
        }
        track.parent = if next.is_some() { None } else { parents.get(&track.id).copied().flatten() };
    }
    // Keep siblings contiguous, preserving the first placement order within each group.
    let tracks: BTreeMap<_, _> = p.tracks.iter().map(|t| (t.id, t.clone())).collect();
    let mut children: BTreeMap<Option<TrackId>, Vec<TrackId>> = BTreeMap::new();
    for track in &p.tracks {
        children.entry(track.parent).or_default().push(track.id);
    }
    let mut roots = Vec::new();
    for track in &p.tracks {
        let mut root = track.id;
        for _ in 0..suhina_model::MAX_GROUP_DEPTH {
            let Some(parent) = tracks.get(&root).and_then(|t| t.parent) else { break };
            root = parent;
        }
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    let mut pending: Vec<_> = roots.into_iter().rev().collect();
    let mut seen = BTreeSet::new();
    p.tracks.clear();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(track) = tracks.get(&id) {
            p.tracks.push(track.clone());
        }
        if let Some(ids) = children.get(&Some(id)) {
            pending.extend(ids.iter().rev().copied());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{AutomationLane, Channel, Device, DeviceId, DeviceParam, Extensions, Send, Track};

    fn utility() -> Device {
        Device {
            instrument: false,
            inputs: Vec::new(),
            id: DeviceId::NONE,
            name: "Source level".into(),
            kind: DeviceKind::Builtin("utility".into()),
            enabled: true,
            state: None,
            params: vec![DeviceParam { id: "mute".into(), value: 0. }],
            automation: vec![],
            clip_host: false,
            extensions: Extensions::default(),
        }
    }
    fn project(channels: Vec<Channel>) -> crate::Import {
        crate::Import {
            project: Project {
                tracks: channels
                    .iter()
                    .map(|c| Track { channel: Some(c.id), ..Track::new(TrackId(c.id.0), c.name.clone()) })
                    .collect(),
                channels,
                ..Project::default()
            },
            warnings: vec![],
        }
    }
    #[test]
    fn dedicated_mixer_folds_levels_devices_and_automation_without_losing_source_mute() {
        let mut source = Channel::new(ChannelId(1), "Source");
        source.output = Some(ChannelId(MIXER + 1));
        source.devices.push(utility());
        source.mute = true;
        let mut mixer = Channel::new(ChannelId(MIXER + 1), "Mixer");
        mixer.volume = 0.4;
        mixer.pan = -0.3;
        mixer.devices.push(utility());
        mixer.devices[0].name = "Mixer effect".into();
        mixer.automation.push(AutomationLane::new("volume"));
        let mut import = project(vec![source, mixer]);
        apply(&mut import);
        let p = import.project;
        assert_eq!(p.tracks.len(), 1);
        let c = &p.channels[0];
        assert_eq!((c.volume, c.pan, c.mute), (0.4, -0.3, false));
        assert_eq!(c.devices[0].params[0].value, 1.);
        assert_eq!(c.devices[1].name, "Mixer effect");
        assert_eq!(c.automation[0].param, "volume");
    }
    #[test]
    fn shared_bus_is_one_parent_and_parallel_route_is_one_return_unused_strips_are_pruned() {
        let mut a = Channel::new(ChannelId(1), "A");
        let mut b = Channel::new(ChannelId(2), "B");
        a.output = Some(ChannelId(MIXER + 1));
        b.output = a.output;
        a.sends.push(Send { target: ChannelId(MIXER + 2), level: 0.5, pre_fader: false });
        b.sends.push(Send { target: ChannelId(MIXER + 1), level: 0.25, pre_fader: false });
        let bus = Channel::new(ChannelId(MIXER + 1), "Bus");
        let mut ret = Channel::new(ChannelId(MIXER + 2), "Return");
        ret.role = ChannelRole::Effect;
        let unused = Channel::new(ChannelId(MIXER + 3), "Unused");
        let mut import = project(vec![a, b, bus, ret, unused]);
        apply(&mut import);
        let p = import.project;
        assert_eq!(p.channels.len(), 4);
        assert_eq!(p.channels[2].role, ChannelRole::Submix);
        assert_eq!(p.channels[3].role, ChannelRole::Effect);
        assert_eq!(p.tracks[0].id, TrackId(MIXER + 1));
        assert_eq!(p.track(TrackId(1)).unwrap().parent, Some(TrackId(MIXER + 1)));
        assert_eq!(p.track(TrackId(2)).unwrap().parent, Some(TrackId(MIXER + 1)));
        assert_eq!(p.track(TrackId(MIXER + 2)).unwrap().parent, None);
        assert_eq!(p.channels[1].sends[0].target, ChannelId(MIXER + 1));
    }
    #[test]
    fn cyclic_group_parents_never_remove_tracks_or_loop() {
        let mut a = Channel::new(ChannelId(MIXER + 1), "A");
        let mut b = Channel::new(ChannelId(MIXER + 2), "B");
        a.output = Some(b.id);
        b.output = Some(a.id);
        a.role = ChannelRole::Submix;
        b.role = ChannelRole::Submix;
        let mut import = project(vec![a, b]);
        group_tracks(&mut import.project);
        assert_eq!(import.project.tracks.len(), 2);
        assert!(import.project.tracks.iter().all(|t| t.parent.is_none()));
    }
}
