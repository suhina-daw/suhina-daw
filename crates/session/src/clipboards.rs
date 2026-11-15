// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Clipboards`]: what a session's copy commands hold for its paste commands.

use std::collections::HashSet;

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
    /// Ranges of several lanes copied together (`automation.copy` with `lanes`, #1322): each
    /// lane's device and parameter and its range; `automation.paste` puts each back on its lane.
    pub(crate) automation_lanes: Vec<crate::automation_range::CopiedLane>,
    /// The automation of a time selection copied with `range.copy`, clips or none: where the
    /// selection started and each track's lanes over it (#1205). `clip.paste` with no track lays
    /// it back on its tracks, the selection's start where the paste starts.
    pub(crate) span: Option<SpanAutomation>,
    /// Files the person chose in the project these were copied from (`device.setNamFile`) that
    /// the copied devices and tracks name. A paste in another project makes them chosen there, so
    /// a pasted Amp Modeler runs its model; no other project reads them without a paste.
    pub(crate) chosen_files: HashSet<String>,
}

impl Clipboards {
    /// The files the copied devices and whole tracks name, inside device groups too.
    pub(crate) fn file_states(&self) -> HashSet<String> {
        let tracks = self.whole_tracks.devices();
        self.devices
            .iter()
            .chain(tracks)
            .flat_map(|device| suhina_model::walk_devices(std::slice::from_ref(device)).into_iter().map(|(_, d)| d))
            .filter_map(|device| crate::device_files::named_file(device).map(|(path, _)| path.to_string()))
            .collect()
    }
}

/// The automation of a copied time selection, each track's lanes from the selection's start.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpanAutomation {
    pub(crate) start: f64,
    pub(crate) lanes: Vec<(TrackId, Vec<crate::automation_range::Under>)>,
}
