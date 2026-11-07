// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Controller mappings: a source (a computer key or a MIDI note) that runs a target command. The
//! targets built so far are the launch commands, a slot, a scene or a track's stop button
//! (`docs/spec/midi-mapping.md` › Launch keys, `docs/spec/clip-launching.md` › Launching from keys
//! and notes).

use crate::TrackId;

/// The longest key name a mapping keeps.
pub const KEY_NAME_MAX: usize = 24;

/// The most mappings a project keeps, so a script or a file cannot grow the project without bound.
pub const MAX_MAPPINGS: usize = 1024;

/// What a mapping listens to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MappingSource {
    /// A computer key by its name: "Q", "1", "F5", "Space". Pressed without Ctrl, Alt or Cmd.
    Key(String),
    /// A MIDI note: `channel` 1 to 16, `note` 0 to 127.
    Note { channel: u8, note: u8 },
}

impl MappingSource {
    /// A key source, when `name` is 1 to [`KEY_NAME_MAX`] printable ASCII characters.
    pub fn key(name: &str) -> Option<Self> {
        let ok = !name.is_empty() && name.len() <= KEY_NAME_MAX && name.bytes().all(|b| b.is_ascii_graphic());
        ok.then(|| Self::Key(name.to_string()))
    }

    /// A note source, when the channel is 1 to 16 and the note 0 to 127.
    pub fn note(channel: u8, note: u8) -> Option<Self> {
        ((1..=16).contains(&channel) && note <= 127).then_some(Self::Note { channel, note })
    }

    /// The source as the commands and the project file write it: `key:Q`, `note:1:60`.
    pub fn name(&self) -> String {
        match self {
            Self::Key(key) => format!("key:{key}"),
            Self::Note { channel, note } => format!("note:{channel}:{note}"),
        }
    }

    /// Reads a name written by [`MappingSource::name`]; anything else is `None`.
    pub fn from_name(text: &str) -> Option<Self> {
        if let Some(key) = text.strip_prefix("key:") {
            return Self::key(key);
        }
        let (channel, note) = text.strip_prefix("note:")?.split_once(':')?;
        let whole = |s: &str| if s.bytes().all(|b| b.is_ascii_digit()) { s.parse::<u8>().ok() } else { None };
        Self::note(whole(channel)?, whole(note)?)
    }

    /// What the window shows on a mapped control: `Q`, `Note 60`, `Ch 2 Note 60`.
    pub fn label(&self) -> String {
        match self {
            Self::Key(key) => key.clone(),
            Self::Note { channel: 1, note } => format!("Note {note}"),
            Self::Note { channel, note } => format!("Ch {channel} Note {note}"),
        }
    }
}

/// What a mapping runs: a launch command and the parameters it fixes. Scenes count from 0, like
/// [`crate::Launcher::scenes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MappingTarget {
    /// The slot of `track` in `scene`: the press is `clip.launch`, the release `clip.release`.
    Slot { track: TrackId, scene: usize },
    /// A scene's launch button: `scene.launch`.
    Scene { scene: usize },
    /// A track's stop button: `track.stop`.
    Stop { track: TrackId },
}

impl MappingTarget {
    /// The commands a target can name, as the commands and the project file write them.
    pub const COMMANDS: [&'static str; 3] = ["clip.launch", "scene.launch", "track.stop"];

    /// The parameters `command` takes beside itself, `track` and `scene`; `None` when it is not
    /// one of [`MappingTarget::COMMANDS`].
    pub fn takes(command: &str) -> Option<&'static [&'static str]> {
        match command {
            "clip.launch" => Some(&["track", "scene"]),
            "scene.launch" => Some(&["scene"]),
            "track.stop" => Some(&["track"]),
            _ => None,
        }
    }

    /// The target that runs `command` with `track` and `scene`, when the command is one of
    /// [`MappingTarget::COMMANDS`] and the parameters it needs are given; `None` otherwise.
    pub fn from_command(command: &str, track: Option<TrackId>, scene: Option<usize>) -> Option<Self> {
        match command {
            "clip.launch" => Some(Self::Slot { track: track?, scene: scene? }),
            "scene.launch" => Some(Self::Scene { scene: scene? }),
            "track.stop" => Some(Self::Stop { track: track? }),
            _ => None,
        }
    }

    /// The command a press runs.
    pub fn command(&self) -> &'static str {
        match self {
            Self::Slot { .. } => "clip.launch",
            Self::Scene { .. } => "scene.launch",
            Self::Stop { .. } => "track.stop",
        }
    }

    pub fn track(&self) -> Option<TrackId> {
        match self {
            Self::Slot { track, .. } | Self::Stop { track } => Some(*track),
            Self::Scene { .. } => None,
        }
    }

    pub fn scene(&self) -> Option<usize> {
        match self {
            Self::Slot { scene, .. } | Self::Scene { scene } => Some(*scene),
            Self::Stop { .. } => None,
        }
    }
}

/// One source that runs one target, an entry of [`crate::Project::controller_mappings`]. A project
/// keeps at most one mapping per source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerMapping {
    pub source: MappingSource,
    pub target: MappingTarget,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_read_back() {
        for source in [
            MappingSource::key("Q").unwrap(),
            MappingSource::key("Space").unwrap(),
            MappingSource::key(":").unwrap(),
            MappingSource::note(1, 0).unwrap(),
            MappingSource::note(16, 127).unwrap(),
        ] {
            assert_eq!(MappingSource::from_name(&source.name()), Some(source.clone()), "{}", source.name());
        }
        assert_eq!(MappingSource::from_name("note:10:36"), Some(MappingSource::Note { channel: 10, note: 36 }));
        for bad in [
            "",
            "key:",
            "key:two words",
            "key:é",
            "Q",
            "note:0:60",
            "note:17:60",
            "note:1:128",
            "note:1",
            "note:1:+5",
            "note:1:60:2",
            "note::60",
            "cc:1:20",
        ] {
            assert_eq!(MappingSource::from_name(bad), None, "{bad}");
        }
        assert!(MappingSource::key(&"A".repeat(KEY_NAME_MAX)).is_some());
        assert!(MappingSource::key(&"A".repeat(KEY_NAME_MAX + 1)).is_none());
    }

    #[test]
    fn labels_are_short() {
        assert_eq!(MappingSource::key("Q").unwrap().label(), "Q");
        assert_eq!(MappingSource::note(1, 36).unwrap().label(), "Note 36");
        assert_eq!(MappingSource::note(10, 36).unwrap().label(), "Ch 10 Note 36");
    }

    #[test]
    fn targets_name_their_command() {
        let slot = MappingTarget::Slot { track: TrackId(2), scene: 1 };
        assert_eq!((slot.command(), slot.track(), slot.scene()), ("clip.launch", Some(TrackId(2)), Some(1)));
        let scene = MappingTarget::Scene { scene: 3 };
        assert_eq!((scene.command(), scene.track(), scene.scene()), ("scene.launch", None, Some(3)));
        let stop = MappingTarget::Stop { track: TrackId(4) };
        assert_eq!((stop.command(), stop.track(), stop.scene()), ("track.stop", Some(TrackId(4)), None));
        for target in [slot, scene, stop] {
            assert_eq!(MappingTarget::from_command(target.command(), target.track(), target.scene()), Some(target));
            assert!(MappingTarget::COMMANDS.contains(&target.command()));
        }
    }

    #[test]
    fn a_target_needs_a_known_command_and_its_parameters() {
        let (track, scene) = (Some(TrackId(1)), Some(0));
        assert_eq!(MappingTarget::from_command("clip.launch", track, None), None);
        assert_eq!(MappingTarget::from_command("clip.launch", None, scene), None);
        assert_eq!(MappingTarget::from_command("scene.launch", track, None), None);
        assert_eq!(MappingTarget::from_command("track.stop", None, scene), None);
        assert_eq!(MappingTarget::from_command("track.setVolume", track, scene), None);
        assert_eq!(MappingTarget::from_command("slot", track, scene), None);
        // A parameter the command does not take is not part of the target.
        assert_eq!(
            MappingTarget::from_command("track.stop", track, scene),
            Some(MappingTarget::Stop { track: TrackId(1) })
        );
    }

    #[test]
    fn a_project_with_duplicate_or_too_many_mappings_does_not_validate() {
        let mapping = |source: MappingSource| ControllerMapping { source, target: MappingTarget::Scene { scene: 0 } };
        let q = MappingSource::key("Q").unwrap();
        let mut project = crate::Project { controller_mappings: vec![mapping(q.clone())], ..Default::default() };
        assert_eq!(project.validate(), Ok(()));
        project.controller_mappings.push(mapping(q));
        assert_eq!(project.validate(), Err(vec![crate::ValidationError::DuplicateMapping("key:Q".into())]));
        project.controller_mappings = (0..=MAX_MAPPINGS)
            .map(|i| mapping(MappingSource::note(1 + (i / 128 % 16) as u8, (i % 128) as u8).unwrap()))
            .collect();
        assert_eq!(project.validate(), Err(vec![crate::ValidationError::TooManyMappings(MAX_MAPPINGS + 1)]));
    }
}
