// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Constructors for the structs that code and tests build most often: the fields that identify
//! the value are arguments, every other field starts at the value a new one has in the editor (a
//! clip enabled, unity gain, no fades, nothing routed). Set the rest with struct update syntax,
//! `Note { channel: 9, ..Note::new(0.0, 1.0, 36, 0.8) }`, so that a field added to the struct
//! later is given its starting value here, once, and the literals that build one keep compiling
//! (issue #462). They are not serialisation defaults: a reader that misses a field still says so.

use crate::{
    AudioInput, AudioRegion, Channel, ChannelId, ChannelRole, Clip, ClipId, ContentId, ContentKinds, Device,
    DeviceKind, Extensions, FadeCurve, FollowAction, LaunchSettings, MidiInput, Note, StretchMode, Track, TrackId,
};

impl Note {
    /// The release velocity a note-off without one has (64 of 127).
    pub const DEFAULT_RELEASE_VELOCITY: f32 = 0.5;

    /// A note on MIDI channel 0, played (not muted), with the default release velocity.
    pub fn new(time: f64, duration: f64, key: u8, velocity: f32) -> Self {
        Self {
            time,
            duration,
            key,
            velocity,
            release_velocity: Self::DEFAULT_RELEASE_VELOCITY,
            channel: 0,
            muted: false,
        }
    }
}

impl Track {
    /// A track that plays nothing yet, on no channel, outside every group, hearing no input.
    pub fn new(id: TrackId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            color: None,
            content: ContentKinds::default(),
            channel: None,
            parent: None,
            midi_input: MidiInput::default(),
            audio_input: AudioInput::default(),
            extensions: Extensions::default(),
        }
    }
}

impl Channel {
    /// A regular channel at unity gain, centred, not muted, soloed or cued, into the main output,
    /// with no sends, devices or automation.
    pub fn new(id: ChannelId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            cue: false,
            output: None,
            sends: Vec::new(),
            devices: Vec::new(),
            automation: Vec::new(),
            extensions: Extensions::default(),
        }
    }
}

impl Clip {
    /// An enabled clip that plays its content from the start, without a loop or fades, with the
    /// default follow action and launch settings.
    pub fn new(content: ContentId, start: f64, length: f64) -> Self {
        Self {
            id: ClipId::NONE,
            content,
            start,
            length,
            offset: 0.0,
            loop_range: None,
            enabled: true,
            fade_in: 0.0,
            fade_out: 0.0,
            fade_in_curve: FadeCurve::default(),
            fade_out_curve: FadeCurve::default(),
            follow_action: FollowAction::default(),
            launch: LaunchSettings::default(),
            color: None,
            extensions: Extensions::default(),
        }
    }
}

impl AudioRegion {
    /// A region of `file` that plays at its original speed and pitch, at unity gain, nothing
    /// known about the file yet.
    pub fn new(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            absolute_file: None,
            sample_rate: None,
            channels: None,
            duration_seconds: None,
            warped: false,
            stretch: StretchMode::default(),
            warp_markers: Vec::new(),
            gain: 1.0,
            transpose: 0.0,
            stream: None,
        }
    }
}

impl Device {
    /// An enabled device with no state, no parameters set and no automation.
    pub fn new(name: impl Into<String>, kind: DeviceKind) -> Self {
        Self {
            name: name.into(),
            kind,
            enabled: true,
            state: None,
            params: Vec::new(),
            automation: Vec::new(),
            extensions: Extensions::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_start_from_the_editors_values() {
        let note = Note::new(1.0, 0.5, 60, 0.8);
        assert_eq!((note.release_velocity, note.channel, note.muted), (0.5, 0, false));

        let track = Track::new(TrackId(3), "Bass");
        assert_eq!((track.id, track.name.as_str()), (TrackId(3), "Bass"));
        assert_eq!((track.channel, track.parent, track.color), (None, None, None));

        let channel = Channel::new(ChannelId(2), "Bass");
        assert_eq!(channel.role, ChannelRole::Regular);
        assert_eq!((channel.volume, channel.pan), (1.0, 0.0));
        assert!(!channel.mute && !channel.solo && !channel.cue && channel.output.is_none());

        let clip = Clip::new(ContentId(1), 4.0, 8.0);
        assert!(clip.enabled && clip.fades_valid());
        assert_eq!((clip.offset, clip.fade_in, clip.fade_out, clip.loop_range), (0.0, 0.0, 0.0, None));

        let region = AudioRegion::new("drums.wav");
        assert_eq!(region.pool_key(), "drums.wav");
        assert_eq!((region.gain, region.transpose, region.warped), (1.0, 0.0, false));

        let device = Device::new("EQ", DeviceKind::Builtin("dj-eq".into()));
        assert!(device.enabled && device.state.is_none() && device.params.is_empty());
    }

    #[test]
    fn struct_update_overrides_only_the_named_fields() {
        let note = Note { channel: 9, muted: true, ..Note::new(0.0, 1.0, 36, 1.0) };
        assert_eq!((note.key, note.channel, note.muted, note.release_velocity), (36, 9, true, 0.5));
    }
}
