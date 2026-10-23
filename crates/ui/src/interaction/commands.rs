// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands the views send most, built in one place (#764 › 6): a level as the commands take
//! it, and the volume and send of a track, which the mixer strip and the Arrangement's header both
//! set.

use serde_json::{Value, json};
use suhina_engine::model::TrackId;

use crate::kit::widgets::{Action, VOLUME_RANGE};

/// A level in decibels as the commands take it: `"-inf"` at or below `floor`, else rounded to a
/// tenth of a decibel.
pub fn db_param(db: f64, floor: f64) -> Value {
    if db <= floor { json!("-inf") } else { json!((db * 10.0).round() / 10.0) }
}

/// Like [`db_param`], with the value as it is: for a field whose value is rounded already.
pub fn exact_db_param(db: f64, floor: f64) -> Value {
    if db <= floor { json!("-inf") } else { json!(db) }
}

/// A fader at `db` for `tracks`; with `continued`, one frame of a drag, every frame but the first
/// continuing its undo step, so the drag is one step.
pub fn set_volume(tracks: &[u32], db: f64, continued: Option<bool>) -> Action {
    let mut params = json!({ "tracks": tracks, "db": db_param(db, VOLUME_RANGE.0) });
    if let Some(continued) = continued {
        params["continue"] = json!(continued);
    }
    Action::new("track.setVolume", params)
}

/// The send from `track` to the return `target` at `db`; every frame of a drag but the first
/// continues its undo step.
pub fn set_send(track: TrackId, target: TrackId, db: f64, continued: bool) -> Action {
    Action::new(
        "track.setSend",
        json!({ "track": track.0, "return": target.0, "level": db_param(db, VOLUME_RANGE.0), "continue": continued }),
    )
}
