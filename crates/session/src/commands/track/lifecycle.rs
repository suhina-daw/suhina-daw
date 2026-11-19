// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conditional processing removal when an edit dissolves a group (#1569). The executor
//! rolls the whole edit back on refusal; the desktop may ask, other callers must opt in.

use super::*;
use suhina_model::{AudioInput, MidiInput};

pub(super) struct Removal {
    confirmed: Vec<TrackId>,
    needed: Vec<TrackId>,
}

impl Removal {
    pub(super) fn new(params: &Value) -> Result<Self, CommandError> {
        let mut confirmed = Vec::new();
        if let Some(value) = Args::new(params).value("removeGroupProcessing") {
            let ids = value.as_array().ok_or("removeGroupProcessing must be a list of group track ids")?;
            if ids.len() > files::MAX_TRACKS {
                return Err(format!("removeGroupProcessing holds at most {} track ids", files::MAX_TRACKS).into());
            }
            for id in ids {
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
        self.require(group, has_processing(project, group));
    }

    /// A cut preserves the group's own state, but not references in channels left behind.
    pub(super) fn check_cut(&mut self, project: &Project, removed: &[TrackId]) {
        let cut_channels: std::collections::BTreeSet<ChannelId> =
            project.tracks.iter().filter(|t| removed.contains(&t.id)).filter_map(|t| t.channel).collect();
        let surviving: Vec<_> = project.channels.iter().filter(|c| !cut_channels.contains(&c.id)).collect();
        for &group in removed {
            let channel = project.track(group).and_then(|t| t.channel).and_then(|id| project.channel(id));
            if let Some(channel) = channel.filter(|c| c.role == ChannelRole::Submix) {
                let referenced = surviving.iter().any(|c| references(c, channel.id));
                self.require(group, referenced);
            }
        }
    }

    fn require(&mut self, group: TrackId, needed: bool) {
        if needed && !self.confirmed.contains(&group) && !self.needed.contains(&group) {
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

fn references(channel: &suhina_model::Channel, target: ChannelId) -> bool {
    channel.output == Some(target)
        || channel.sends.iter().any(|s| s.target == target)
        || suhina_model::walk_devices(&channel.devices)
            .iter()
            .any(|(_, d)| d.inputs.iter().any(|i| i.source.channel() == target))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_is_optional_bounded_and_still_validates_every_id() {
        for params in [json!({}), json!({"removeGroupProcessing": null})] {
            assert!(Removal::new(&params).unwrap().confirmed.is_empty());
        }
        for length in [0, 1, files::MAX_TRACKS] {
            let ids = vec![7; length];
            let removal = Removal::new(&json!({"removeGroupProcessing": ids})).unwrap();
            assert_eq!(removal.confirmed.len(), usize::from(length > 0));
        }
        let ids: Vec<usize> = (0..files::MAX_TRACKS).collect();
        assert_eq!(Removal::new(&json!({"removeGroupProcessing": ids})).unwrap().confirmed.len(), files::MAX_TRACKS);
        assert!(Removal::new(&json!({"removeGroupProcessing": vec![7; files::MAX_TRACKS + 1]})).is_err());
        for value in [json!(true), json!({}), json!([-1]), json!([u64::MAX]), json!([7, "7"])] {
            assert!(Removal::new(&json!({"removeGroupProcessing": value})).is_err());
        }
    }
}
