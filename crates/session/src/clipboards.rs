// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Clipboards`]: what a session's copy commands hold for its paste commands.

use suhina_model::{Clip, ClipContent, Device, TrackId};

use crate::notes::NoteClipboard;

/// Copied clips, devices and notes, each for its own paste.
#[derive(Default, Clone)]
pub(crate) struct Clipboards {
    /// The arrangement clips copied for `clip.paste`, with the content each plays.
    pub(crate) clips: Vec<(Clip, ClipContent)>,
    /// The track each copied clip came from, when the copy knows it (arrangement clips and time
    /// selections); a paste with no track puts each clip back on its own.
    pub(crate) tracks: Vec<TrackId>,
    /// The automation under each copied arrangement clip, in the order of `clips` (empty for a
    /// slot's clip): a paste lays it under the clip's copy (#916).
    pub(crate) under: Vec<Vec<crate::automation_range::Under>>,
    /// Devices copied for `device.paste`, with their state as it was then.
    pub(crate) devices: Vec<Device>,
    /// Notes copied from a MIDI clip, for `note.paste`.
    pub(crate) notes: Option<NoteClipboard>,
    /// Whole tracks copied for `track.paste` (#919).
    pub(crate) whole_tracks: crate::track_copy::TrackClipboard,
    /// A range of an automation lane copied for `automation.paste`.
    pub(crate) automation: Option<crate::automation_range::AutomationClip>,
}
