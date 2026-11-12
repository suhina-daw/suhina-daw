// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `automation.setLayout`: how a track's automation lanes are laid out in the Arrangement view
//! (#1322), the lanes mounted under its top lane and their groups, set whole as one undo step.

use serde_json::{Value, json};
use suhina_model::{LaneGroup, LaneLayout, LaneRef, MAX_GROUP_NAME, MAX_LAYOUT_GROUPS, MAX_LAYOUT_LANES};

use crate::commands::{Args, track_or_main_id};
use crate::{CommandError, Session};

/// Longest parameter id a layout's lane names.
const MAX_PARAM: usize = 256;

/// A lane of the command, `{device?, param}`, the device by its index on the track's chain,
/// as a lane of a layout: the device by its id.
fn lane(value: &Value, devices: &[suhina_model::Device], at: &str) -> Result<LaneRef, CommandError> {
    let param = value
        .get("param")
        .and_then(Value::as_str)
        .filter(|p| !p.trim().is_empty() && p.chars().count() <= MAX_PARAM)
        .ok_or_else(|| format!("{at}: param must be a parameter id of 1 to {MAX_PARAM} characters"))?;
    let device = match value.get("device").filter(|d| !d.is_null()) {
        None => {
            if !matches!(param, "volume" | "pan" | "mute") {
                return Err(
                    format!("{at}: {param:?} is not a mixer parameter: give device for a device's parameter").into()
                );
            }
            None
        }
        Some(index) => {
            let index = index
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| format!("{at}: device must be a device's index"))?;
            Some(devices.get(index).ok_or_else(|| format!("{at}: the track has no device {index}"))?.id)
        }
    };
    Ok(LaneRef { device, param: param.to_string() })
}

/// A list `key` of `params`, each read by `read`, at most `most` long.
fn listed<T>(
    params: &Value,
    key: &str,
    most: usize,
    mut read: impl FnMut(&Value, &str) -> Result<T, CommandError>,
) -> Result<Vec<T>, CommandError> {
    let Some(list) = Args::new(params).value(key).filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| format!("{key} must be a list"))?;
    if list.len() > most {
        return Err(format!("{key} holds at most {most}").into());
    }
    list.iter().enumerate().map(|(i, v)| read(v, &format!("{key}[{i}]"))).collect()
}

/// `automation.setLayout {track, lanes, groups}`: sets how the track's lanes are laid out, whole.
pub(crate) fn set_layout(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_or_main_id(&session.project, params)?;
    let channel = session.project.channel_of_track(track).ok_or("the track has no channel")?;
    let devices = channel.devices.clone();
    let id = channel.id;
    let lanes = listed(params, "lanes", MAX_LAYOUT_LANES, |v, at| lane(v, &devices, at))?;
    let groups = listed(params, "groups", MAX_LAYOUT_GROUPS, |v, at| {
        let name = v
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty() && n.chars().count() <= MAX_GROUP_NAME)
            .ok_or_else(|| format!("{at}: name must have 1 to {MAX_GROUP_NAME} characters"))?;
        let members = listed(v, "lanes", MAX_LAYOUT_LANES, |m, inner| lane(m, &devices, &format!("{at}.{inner}")))?;
        let device = match v.get("device").filter(|d| !d.is_null()) {
            None => None,
            Some(index) => {
                let index = index
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .ok_or_else(|| format!("{at}: device must be a device's index"))?;
                Some(devices.get(index).ok_or_else(|| format!("{at}: the track has no device {index}"))?.id)
            }
        };
        let folded = v
            .get("folded")
            .map(|f| f.as_bool().ok_or_else(|| format!("{at}: folded must be true or false")))
            .transpose()?
            .unwrap_or(false);
        Ok(LaneGroup { name: name.to_string(), lanes: members, folded, device })
    })?;
    let layout = LaneLayout { channel: id, lanes, groups };
    layout.check()?;
    let count = (layout.lanes.len(), layout.groups.len());
    match session.project.lane_layouts.iter_mut().find(|l| l.channel == id) {
        Some(kept) => *kept = layout,
        None => session.project.lane_layouts.push(layout),
    }
    Ok(json!({ "track": track.0, "lanes": count.0, "groups": count.1 }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::{Channel, ChannelId, Device, DeviceKind, Project, Track, TrackId};

    fn session() -> Session {
        let mut project = Project::default();
        let mut track = Track::new(TrackId(1), "Synth");
        track.channel = Some(ChannelId(1));
        let mut channel = Channel::new(ChannelId(1), "Synth");
        channel.devices.push(Device::new("Serum", DeviceKind::Vst3 { id: "AABBCCDDEEFF0011AABBCCDDEEFF0011".into() }));
        project.tracks.push(track);
        project.channels.push(channel);
        project.assign_ids().unwrap();
        Session::new(project)
    }

    /// A layout is set whole, its devices by index turned into their ids; a second call replaces
    /// it; one that does not hold together is refused and changes nothing (#1322).
    #[test]
    fn a_layout_is_set_whole_with_its_devices_by_id() {
        let mut session = session();
        let device = session.project.channels[0].devices[0].id;
        let lanes = json!([{ "param": "pan" }, { "device": 0, "param": "7" }]);
        let groups =
            json!([{ "name": "Serum", "lanes": [{ "device": 0, "param": "7" }], "folded": true, "device": 0 }]);
        set_layout(&mut session, &json!({ "track": 1, "lanes": lanes, "groups": groups })).unwrap();
        let layout = &session.project.lane_layouts[0];
        assert_eq!(layout.lanes[1], LaneRef { device: Some(device), param: "7".into() });
        assert!(layout.groups[0].folded && layout.groups[0].device == Some(device));
        set_layout(&mut session, &json!({ "track": 1, "lanes": [{ "param": "volume" }] })).unwrap();
        assert_eq!(session.project.lane_layouts.len(), 1);
        assert!(session.project.lane_layouts[0].groups.is_empty());
        for wrong in [
            json!({ "track": 1, "lanes": [{ "param": "cutoff" }] }),
            json!({ "track": 1, "lanes": [{ "device": 4, "param": "7" }] }),
            json!({ "track": 1, "lanes": [{ "param": "pan" }, { "param": "pan" }] }),
            json!({ "track": 1, "lanes": [], "groups": [{ "name": "G", "lanes": [{ "param": "pan" }] }] }),
            json!({ "track": 1, "groups": [{ "name": "", "lanes": [] }] }),
        ] {
            assert!(set_layout(&mut session, &wrong).is_err(), "{wrong}");
        }
        assert_eq!(session.project.lane_layouts[0].lanes, [LaneRef::mixer("volume")]);
    }
}
