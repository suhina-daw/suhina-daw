// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Clipboards`]: what a session's copy commands hold for its paste commands.

use suhina_model::{Clip, ClipContent, Device, TrackId};

use crate::notes::NoteClipboard;

/// Copied clips, devices and notes, each for its own paste.
#[derive(Default)]
pub(crate) struct Clipboards {
    /// The arrangement clips copied for `clip.paste`, with the content each plays.
    pub(crate) clips: Vec<(Clip, ClipContent)>,
    /// The track each copied clip came from, when the copy knows it (arrangement clips and time
    /// selections); a paste with no track puts each clip back on its own.
    pub(crate) tracks: Vec<TrackId>,
    /// Devices copied for `device.paste`, with their state as it was then.
    pub(crate) devices: Vec<Device>,
    /// Notes copied from a MIDI clip, for `note.paste`.
    pub(crate) notes: Option<NoteClipboard>,
}
