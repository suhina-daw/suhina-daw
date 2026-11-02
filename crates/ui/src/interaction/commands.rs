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

/// A mixer control a drag moves, on the Launcher view's strips and the Arrangement's headers
/// alike (#764 › 6): one place for what each frame of its drag sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelControl {
    Volume,
    Pan,
    /// The send to this return track.
    Send(TrackId),
}

/// A drag of `control` on `track`'s channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelControlDrag {
    pub track: TrackId,
    pub control: ChannelControl,
}

/// What a frame of a [`ChannelControlDrag`] gives its view: the value to show while it moves, the
/// value it was left at, and the command to run.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelDragFrame {
    /// Still moving: the value to show. `None` once it ends.
    pub moving: Option<f64>,
    /// Ended (released, or typed or reset): the value it was left at.
    pub committed: Option<f64>,
    pub action: Option<Action>,
}

impl ChannelControlDrag {
    /// What `drag` does this frame: a moving or finished drag sends each value in one gesture, so
    /// it is heard as it moves and is one undo step; a typed or reset value is sent as it is.
    pub fn frame(self, drag: crate::kit::widgets::Drag) -> ChannelDragFrame {
        use crate::kit::widgets::Drag;
        let (moving, committed, action) = match drag {
            Drag::None => (None, None, None),
            Drag::Moving(v) => (Some(v), None, Some(self.action(v, true))),
            Drag::Done(v) => (None, Some(v), Some(self.action(v, true))),
            Drag::Set(v) => (None, Some(v), Some(self.action(v, false))),
        };
        ChannelDragFrame { moving, committed, action }
    }

    /// The command that sets the control to `value`; `dragged` puts it in the drag's gesture.
    pub fn action(self, value: f64, dragged: bool) -> Action {
        let track = self.track;
        match self.control {
            ChannelControl::Volume => set_volume(&[track.0], value, dragged),
            ChannelControl::Pan => {
                let mut params = json!({ "tracks": [track.0], "pan": (value * 100.0).round() / 100.0 });
                if dragged {
                    params["gesture"] = gesture(format_args!("pan:{}", track.0));
                }
                Action::new("track.setPan", params)
            }
            ChannelControl::Send(target) if dragged => set_send(track, target, value),
            ChannelControl::Send(target) => set_send_outright(track, target, value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::widgets::Drag;

    #[test]
    fn a_channel_drag_sends_every_frame_in_one_gesture_and_a_typed_value_outright() {
        let send = ChannelControlDrag { track: TrackId(3), control: ChannelControl::Send(TrackId(9)) };
        let moving = send.frame(Drag::Moving(-6.0));
        assert_eq!((moving.moving, moving.committed), (Some(-6.0), None));
        let action = moving.action.unwrap();
        assert_eq!((action.id, &action.params["gesture"]), ("track.setSend", &json!("send:3:9")));
        let done = send.frame(Drag::Done(-3.0));
        assert_eq!((done.moving, done.committed), (None, Some(-3.0)));
        assert!(done.action.unwrap().params["gesture"].is_string());
        let typed = send.frame(Drag::Set(0.0)).action.unwrap();
        assert!(typed.params.get("gesture").is_none());
        assert_eq!(send.frame(Drag::None), ChannelDragFrame { moving: None, committed: None, action: None });
        let pan = ChannelControlDrag { track: TrackId(3), control: ChannelControl::Pan }.action(0.257, true);
        assert_eq!(pan.params, json!({ "tracks": [3], "pan": 0.26, "gesture": "pan:3" }));
    }
}
