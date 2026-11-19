// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conditional processing removal when an edit dissolves a group (#1569). The executor
//! rolls the whole edit back on refusal; the desktop may ask, other callers must opt in.

use super::*;

pub(super) struct Removal {
    confirmed: Vec<TrackId>,
    needed: Vec<TrackId>,
}

impl Removal {
    pub(super) fn new(params: &Value) -> Result<Self, CommandError> {
        let mut confirmed = Vec::new();
        if let Some(value) = params.get("removeGroupProcessing") {
            for id in value.as_array().ok_or("removeGroupProcessing must be a list of group track ids")? {
                let id = id
                    .as_u64()
                    .and_then(|id| u32::try_from(id).ok())
                    .map(TrackId)
                    .ok_or("removeGroupProcessing must contain group track ids")?;
                if !confirmed.contains(&id) {
                    confirmed.push(id);
                }
            }
        }
        Ok(Self { confirmed, needed: Vec::new() })
    }

    pub(super) fn check(&mut self, project: &Project, group: TrackId) {
        if has_processing(project, group) && !self.confirmed.contains(&group) && !self.needed.contains(&group) {
            self.needed.push(group);
        }
    }

    /// Only source ancestors, after the entire edit: anchors remain available while moving.
    /// Deepest first lets an outer group disappear when its last inner group disappears.
    pub(super) fn cleanup(&mut self, session: &mut Session, sources: &[TrackId]) -> Result<(), CommandError> {
        for &source in sources {
            let p = &session.project;
            if p.track(source).is_some() && !p.tracks.iter().any(|t| t.parent == Some(source)) {
                self.check(p, source);
                super::ungroup(session, &json!({"track": source.0}))?;
            }
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<(), CommandError> {
        if self.needed.is_empty() { Ok(()) } else { Err(CommandError::removing_group_processing(self.needed)) }
    }
}

fn has_processing(p: &Project, group: TrackId) -> bool {
    let Some(track) = p.track(group) else { return false };
    let Some(channel) = track.channel.and_then(|id| p.channel(id)).filter(|c| c.role == ChannelRole::Submix) else {
        return false;
    };
    let expected_output = track.parent.and_then(|id| p.track(id)).and_then(|t| t.channel);
    let owns_content = p.arrangement.lanes.iter().any(|l| l.track == Some(group) && !l.clips.is_empty())
        || p.arrangement.takes.iter().any(|t| t.track == group)
        || p.launcher.columns.iter().any(|c| c.track == group && c.slots.iter().any(|s| s.clip.is_some()))
        || p.contents.iter().any(|c| c.lanes.iter().any(|l| l.target == Some(group)));
    let referenced = p.channels.iter().any(|c| {
        let direct_member = p.tracks.iter().any(|t| t.channel == Some(c.id) && t.parent == Some(group));
        (c.output == Some(channel.id) && !direct_member)
            || c.sends.iter().any(|s| s.target == channel.id)
            || suhina_model::walk_devices(&c.devices)
                .iter()
                .any(|(_, d)| d.inputs.iter().any(|i| i.source.channel() == channel.id))
    });
    !channel.devices.is_empty()
        || !channel.automation.is_empty()
        || channel.volume != 1.0
        || channel.pan != 0.0
        || channel.mute
        || channel.solo
        || channel.cue
        || !channel.sends.is_empty()
        || channel.output != expected_output
        || track.audio_input != AudioInput::default()
        || track.midi_input != MidiInput::default()
        || !channel.extensions.0.is_empty()
        || !track.extensions.0.is_empty()
        || owns_content
        || referenced
}
