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

/// The gesture of a drag of `control`: one id for every frame of it, so the drag is one undo
/// step; the window ends it when the mouse is let go ([`crate::app`], #1146).
pub fn gesture(control: impl std::fmt::Display) -> Value {
    let id = control.to_string();
    // A gesture is 1 to 128 bytes; a control's name is far shorter.
    json!(if id.is_empty() || id.len() > 128 { "drag".to_string() } else { id })
}

/// A fader at `db` for `tracks`; `dragged` while a drag moves it, its frames one gesture.
pub fn set_volume(tracks: &[u32], db: f64, dragged: bool) -> Action {
    let mut params = json!({ "tracks": tracks, "db": db_param(db, VOLUME_RANGE.0) });
    if dragged {
        params["gesture"] = gesture(format_args!("volume:{tracks:?}"));
    }
    Action::new("track.setVolume", params)
}

/// The send from `track` to the return `target` set to `db` itself, typed or reset, which a
/// selection takes as it is.
pub fn set_send_outright(track: TrackId, target: TrackId, db: f64) -> Action {
    Action::new("track.setSend", json!({ "track": track.0, "return": target.0, "level": db_param(db, VOLUME_RANGE.0) }))
}

/// The send from `track` to the return `target` at `db`, one frame of a drag: its frames are one
/// gesture.
pub fn set_send(track: TrackId, target: TrackId, db: f64) -> Action {
    Action::new(
        "track.setSend",
        json!({
            "track": track.0, "return": target.0, "level": db_param(db, VOLUME_RANGE.0),
            "gesture": gesture(format_args!("send:{}:{}", track.0, target.0)),
        }),
    )
}
