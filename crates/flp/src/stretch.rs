// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 The Suhina contributors

//! Stretch laws adapted from Lemon Studio os-model ChannelSound and os-engine,
//! revision 011a03f23d9db7f10f07cd7d165cc978a9dda263.
//! <https://github.com/ItsAlphaNeon/Lemon-Studio>. Changes: native warp markers and bounds.

use crate::{
    Flp, FlpError, Import,
    records::{data, dword, number},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use suhina_model::{LaneData, StretchMode, WarpMarker};

/// `found` holds the sample references the import resolved to files that are there; only those
/// are opened to read a duration, never a path the project file names on its own.
pub(crate) fn apply(flp: &Flp, import: &mut Import, folder: &Path, found: &BTreeSet<String>) -> Result<(), FlpError> {
    let mut source = None;
    let mut settings = BTreeMap::new();
    for event in &flp.events {
        match event.id {
            64 => source = Some(number(event)?),
            51 | 99 | 233 | 236 | 149 | 42 => source = None,
            215 => {
                if let Some(source) = source {
                    let d = data(event)?;
                    if d.len() >= 112 {
                        let values = [
                            dword(d, 96, event.offset)?,
                            dword(d, 100, event.offset)?,
                            dword(d, 104, event.offset)?,
                            dword(d, 108, event.offset)?,
                        ];
                        if values[..3] != [0; 3] {
                            settings.insert(source, values);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let p = &mut import.project;
    let mut maps = BTreeMap::new();
    for content in &p.contents {
        for lane in content.lanes.iter() {
            let LaneData::Audio(region) = &lane.data else { continue };
            let source = lane.target.and_then(|t| p.channel_of_track(t)).map(|c| c.id.0);
            let Some(values) = source.and_then(|id| settings.get(&id)) else { continue };
            // Older saved projects also hold integer ticks here (see the mapping spec).
            // This bit range cannot encode a float of one tick or more.
            let raw_time =
                if values[0] <= 192_000_000 { f64::from(values[0]) } else { f64::from(f32::from_bits(values[0])) };
            let multiplier = f64::from(values[2] as i32) / 10000.;
            if !raw_time.is_finite() || !(-10. ..=10.).contains(&multiplier) {
                import.warnings.push(format!("{}: invalid audio stretch settings; not applied.", content.name));
                continue;
            }
            let info = found
                .contains(&region.file)
                .then(|| folder.join(&region.file))
                .and_then(|path| suhina_media::info::sample_info(&path).ok());
            let Some(seconds) = info.and_then(|i| i.duration_seconds).filter(|s| *s > 0.) else {
                import
                    .warnings
                    .push(format!("{}: sample duration unavailable; audio stretch not applied.", content.name));
                continue;
            };
            let beats = if raw_time >= 1. { raw_time / 192. } else { seconds * p.transport.tempo_bpm / 60. }
                * 2f64.powf(multiplier);
            let ratio = beats * 60. / p.transport.tempo_bpm / seconds;
            if !(0.25..=4.).contains(&ratio) || !(0. ..=1_000_000.).contains(&beats) {
                import.warnings.push(format!("{}: audio stretch ratio is outside the supported range.", content.name));
                continue;
            }
            let mode = match values[3] as i32 {
                0 => StretchMode::Repitch,
                2 | 8 | 9 => StretchMode::Tonal,
                3 | 4 | 7 => StretchMode::Rhythmic,
                -2 => StretchMode::HighQuality,
                _ => StretchMode::Smooth,
            };
            let pitch = f64::from(values[1] as i32) / 100.;
            if !(-48. ..=48.).contains(&pitch) {
                import.warnings.push(format!("{}: stretch pitch is outside the supported range.", content.name));
                continue;
            }
            maps.insert(content.id, (beats, seconds, mode, pitch, ratio));
        }
    }
    for clip in p.arrangement.lanes.iter_mut().flat_map(|l| &mut l.clips) {
        let Some(&(beats, seconds, mode, pitch, ratio)) = maps.get(&clip.content) else { continue };
        let old_offset = clip.offset;
        clip.offset *= ratio;
        for point in &mut clip.audio.gain_envelope {
            point.beat += clip.offset - old_offset;
        }
        clip.audio.warped = true;
        clip.audio.stretch = mode;
        clip.audio.transpose = pitch;
        clip.audio.warp_markers =
            vec![WarpMarker { time: 0., audio_time: 0. }, WarpMarker { time: beats, audio_time: seconds }];
    }
    for content in &mut p.contents {
        if let Some(&(beats, _, _, _, _)) = maps.get(&content.id) {
            content.length = content.length.max(beats);
        }
    }
    if !maps.is_empty() {
        import
            .warnings
            .push(format!("{} audio sources use imported stretch timing with native stretch algorithms.", maps.len()));
    }
    Ok(())
}
