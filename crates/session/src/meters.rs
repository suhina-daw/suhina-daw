// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The level meters' commands: reading what the playing engine's meters show and turning off
//! clip indicators (`docs/spec/level-meters.md`).

use serde_json::{Value, json};
use suhina_engine::Level;

use crate::commands::track_id;
use crate::{CommandError, Session};

/// Levels below this read as silence (`null`), as the engine's meters do.
const SILENCE_DB: f64 = -120.0;

/// A linear level in dB, to a tenth; `null` for silence.
fn db(level: f32) -> Value {
    let db = 20.0 * f64::from(level).log10();
    if db.is_finite() && db >= SILENCE_DB { json!((db * 10.0).round() / 10.0) } else { Value::Null }
}

fn describe(level: Level) -> Value {
    json!({
        "peak_db": [db(level.peak[0]), db(level.peak[1])],
        "rms_db": [db(level.rms[0]), db(level.rms[1])],
        "clipped": level.clipped,
    })
}

/// `mixer.levels`: the main output's level and every metered track's, in the project's order.
pub(crate) fn levels(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let monitor = session.player_mut()?.meter_monitor();
    let main = describe(monitor.main());
    let levels = monitor.tracks();
    let project = session.project();
    let tracks: Vec<Value> = project
        .tracks
        .iter()
        .filter_map(|track| {
            let (_, level) = levels.iter().find(|(id, _)| *id == track.id)?;
            let mut value = describe(*level);
            if let Some(object) = value.as_object_mut() {
                object.insert("track".into(), json!(track.id.0));
                object.insert("track_name".into(), json!(project.shown_track_name(track.id)));
            }
            Some(value)
        })
        .collect();
    Ok(json!({ "main": main, "tracks": tracks }))
}

/// `mixer.resetClip`: turns off one track's clip indicator, the main output's, or all of them.
pub(crate) fn reset_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = match params.get("track") {
        Some(_) => Some(track_id(session.project(), params)?),
        None => None,
    };
    let main = match params.get("main") {
        None => false,
        Some(value) => value.as_bool().ok_or("main must be true or false")?,
    };
    let monitor = session.player_mut()?.meter_monitor();
    match (track, main) {
        (None, false) => {
            monitor.reset_all_clips();
            Ok(json!({ "reset": "all" }))
        }
        (track, main) => {
            if main {
                monitor.reset_main_clip();
            }
            if let Some(track) = track
                && !monitor.reset_clip(track)
            {
                return Err(format!("track {} has no meter in the playing engine yet", track.0).into());
            }
            Ok(json!({ "reset": { "track": track.map(|t| t.0), "main": main } }))
        }
    }
}
