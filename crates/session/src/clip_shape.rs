// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip's loop, length and start marker: `clip.setLoop`, `clip.setLength` and `clip.setStart`,
//! which the piano roll's ruler runs when its handles and markers are dragged
//! (`docs/spec/ui-clip-editor.md` › Ruler, › Commands).
//!
//! A clip is named as the `note.*` commands name it: `track` and `index` (an arrangement clip)
//! or `scene` (the clip in a launcher slot). Beats are content beats (the loop, the start
//! marker) or beats of playing time (the length), from 0 to 120 000.

use serde_json::{Map, Value, json};
use suhina_model::{Clip, LoopRange, Project};

use crate::notes::{ClipAt, MAX_BEAT, clip_mut, locate};
use crate::{CommandError, Session};

/// How far past the next clip's start a clip may end: the piano roll rounds beats to a millionth.
const EPSILON: f64 = 1e-6;

/// An optional beat in `params`: a finite number from 0 to [`MAX_BEAT`], or `None` when left out.
fn beat(params: &Value, key: &str) -> Result<Option<f64>, CommandError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let beat = v.as_f64().filter(|b| b.is_finite()).ok_or_else(|| format!("{key} must be a number"))?;
            if !(0.0..=MAX_BEAT).contains(&beat) {
                return Err(format!("{key} must be between 0 and {MAX_BEAT}, got {beat}").into());
            }
            Ok(Some(beat))
        }
    }
}

/// A length in beats: finite, above 0 and at most [`MAX_BEAT`].
fn length(value: Option<&Value>, key: &str) -> Result<f64, CommandError> {
    let length = value
        .and_then(Value::as_f64)
        .filter(|l| l.is_finite())
        .ok_or_else(|| format!("{key} must be a number of beats"))?;
    if !(length > 0.0 && length <= MAX_BEAT) {
        return Err(format!("{key} must be above 0 and at most {MAX_BEAT} beats, got {length}").into());
    }
    Ok(length)
}

/// What names the clip `id` at `at` in a result: its `clip` id, and `index` or `scene` (from 1).
fn address(at: ClipAt, id: suhina_model::ClipId) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("clip".into(), json!(id.0));
    match at {
        ClipAt::Arrangement(_, index) => out.insert("index".into(), json!(index)),
        ClipAt::Slot(_, slot) => out.insert("scene".into(), json!(slot.saturating_add(1))),
    };
    out
}

/// Fails when an arrangement clip at `at`, `length` beats long, would run into the next clip on
/// its lane (the nearest one that starts after it). A slot's clip has no neighbours.
fn check_room(p: &Project, at: ClipAt, length: f64) -> Result<(), CommandError> {
    let ClipAt::Arrangement(lane, index) = at else { return Ok(()) };
    let Some(lane) = p.arrangement.lanes.get(lane) else { return Err("the clip is missing".into()) };
    let start = lane.clips.get(index).ok_or("the clip is missing")?.start;
    let next = lane
        .clips
        .iter()
        .enumerate()
        .filter(|(i, c)| *i != index && c.start > start)
        .map(|(_, c)| c.start)
        .fold(f64::INFINITY, f64::min);
    if start + length > next + EPSILON {
        return Err(format!(
            "the clip would run into the next clip on the lane, which starts at beat {next}; \
             it can be at most {} beats long",
            next - start
        )
        .into());
    }
    Ok(())
}

/// The clip `params` names, to change, and where it is.
fn named<'a>(session: &'a mut Session, params: &Value) -> Result<(ClipAt, &'a mut Clip), CommandError> {
    let (at, _, _) = locate(&session.project, params)?;
    let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
    Ok((at, clip))
}

/// `clip.setLoop`: a clip's loop on (over its whole length, or the loop it already has) or off,
/// or its loop set to `start` … `end` (which turns it on). An arrangement clip or a slot's clip
/// (`docs/spec/clip-launching.md` › Loop switch).
pub(crate) fn set_loop(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let on = match params.get("loop") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or("loop must be true or false")?),
    };
    let (start, end) = (beat(params, "start")?, beat(params, "end")?);
    if on.is_none() && start.is_none() && end.is_none() {
        return Err("give loop (true or false), or the loop's start and end".into());
    }
    let (at, clip) = named(session, params)?;
    let range = if start.is_some() || end.is_some() {
        if on == Some(false) {
            return Err("a loop that is off has no start or end; leave out loop or make it true".into());
        }
        let missing = "the clip does not loop yet: give both start and end";
        let start = start.or(clip.loop_range.map(|r| r.start)).ok_or(missing)?;
        let end = end.or(clip.loop_range.map(|r| r.end)).ok_or(missing)?;
        if end <= start {
            return Err(format!("the loop's end ({end}) must be after its start ({start})").into());
        }
        Some(LoopRange { start, end })
    } else {
        match (on, clip.loop_range) {
            (Some(false), _) | (None, None) => None,
            (_, Some(range)) => Some(range),
            (Some(true), None) => Some(LoopRange { start: 0.0, end: clip.length }),
        }
    };
    clip.loop_range = range;
    let mut out = address(at, clip.id);
    out.insert("loop".into(), json!(range.is_some()));
    if let Some(range) = range {
        out.insert("start".into(), json!(range.start));
        out.insert("end".into(), json!(range.end));
    }
    Ok(Value::Object(out))
}

/// `clip.setLength`: how long a clip plays, in beats. An arrangement clip keeps its start and
/// may not run into the next clip on its lane; fades are shortened to fit.
pub(crate) fn set_length(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let length = length(params.get("length"), "length")?;
    let (at, _, _) = locate(&session.project, params)?;
    check_room(&session.project, at, length)?;
    let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
    clip.length = length;
    clip.fit_fades();
    let mut out = address(at, clip.id);
    out.insert("length".into(), json!(length));
    Ok(Value::Object(out))
}

/// `clip.setStart`: a clip's start marker, where in its content it starts playing. A clip that
/// does not loop keeps its end marker where it is, so it gets longer or shorter by as much as
/// the marker moves (an arrangement clip keeps its start on the arrangement and may not run into
/// the next clip); a looping clip keeps its length, and the marker must be before the loop's end.
pub(crate) fn set_start(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let offset = beat(params, "offset")?.ok_or("offset must be a number of beats")?;
    let (at, _, _) = locate(&session.project, params)?;
    let (loops, end) = {
        let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
        (clip.loop_range, clip.offset + clip.length)
    };
    let length = match loops {
        Some(range) => {
            if offset >= range.end {
                return Err(format!("the start marker ({offset}) must be before the loop's end ({})", range.end).into());
            }
            None
        }
        None => {
            if offset >= end - EPSILON {
                return Err(format!("the start marker ({offset}) must be before the clip's end ({end})").into());
            }
            let length = self::length(Some(&json!(end - offset)), "the new length")?;
            check_room(&session.project, at, length)?;
            Some(length)
        }
    };
    let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
    clip.offset = offset;
    if let Some(length) = length {
        clip.length = length;
        clip.fit_fades();
    }
    let mut out = address(at, clip.id);
    out.insert("offset".into(), json!(offset));
    out.insert("length".into(), json!(clip.length));
    Ok(Value::Object(out))
}

/// The names `clip.setAudio` takes and returns for each warp mode.
const MODES: [(&str, suhina_model::StretchMode); 6] = [
    ("repitch", suhina_model::StretchMode::Repitch),
    ("rhythmic", suhina_model::StretchMode::Rhythmic),
    ("tonal", suhina_model::StretchMode::Tonal),
    ("texture", suhina_model::StretchMode::Texture),
    ("smooth", suhina_model::StretchMode::Smooth),
    ("high-quality", suhina_model::StretchMode::HighQuality),
];

/// The name of warp mode `mode`, as `clip.setAudio` takes it.
pub(crate) fn mode_name(mode: suhina_model::StretchMode) -> &'static str {
    MODES.iter().find(|(_, m)| *m == mode).map_or("smooth", |(name, _)| name)
}

/// `clip.setAudio`: an audio clip's warp switch, warp mode and transpose, the clip properties'
/// controls (#699). Every clip that plays the same audio content follows, as with its gain. Warp
/// turned on for audio that has no warp markers gets two, its start and its end, so it plays as
/// it did and then follows the tempo.
pub(crate) fn set_audio(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let warp = match params.get("warp") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or("warp must be true or false")?),
    };
    let mode = match params.get("mode") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let text = v.as_str().ok_or("mode must be a warp mode's name")?;
            let names = MODES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
            Some(
                MODES
                    .iter()
                    .find(|(n, _)| *n == text)
                    .map(|(_, m)| *m)
                    .ok_or_else(|| format!("mode must be one of {names}, got {text:?}"))?,
            )
        }
    };
    let transpose = match params.get("transpose") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let t = v.as_f64().filter(|t| t.is_finite()).ok_or("transpose must be a number of semitones")?;
            if !(-48.0..=48.0).contains(&t) {
                return Err(format!("transpose must be from -48 to 48 semitones, got {t}").into());
            }
            Some(t)
        }
    };
    if warp.is_none() && mode.is_none() && transpose.is_none() {
        return Err("give warp, mode or transpose".into());
    }
    let (at, clip) = named(session, params)?;
    let (id, content_id) = (clip.id, clip.content);
    let content =
        session.project.contents.iter_mut().find(|c| c.id == content_id).ok_or("the clip's content is missing")?;
    let length = content.length;
    let mut found = None;
    for lane in &mut content.lanes {
        let suhina_model::LaneData::Audio(region) = &mut lane.data else { continue };
        if let Some(on) = warp {
            if on && region.warp_markers.is_empty() {
                let seconds = region.duration_seconds.ok_or("the audio's length is not known, so it cannot warp")?;
                region.warp_markers = vec![
                    suhina_model::WarpMarker { time: 0.0, audio_time: 0.0 },
                    suhina_model::WarpMarker { time: length, audio_time: seconds },
                ];
            }
            region.warped = on;
        }
        if let Some(mode) = mode {
            region.stretch = mode;
        }
        if let Some(t) = transpose {
            region.transpose = t;
        }
        found = Some((region.warped, region.stretch, region.transpose));
    }
    let (warped, stretch, transpose) = found.ok_or("only an audio clip can be warped or transposed")?;
    let mut out = address(at, id);
    out.insert("warp".into(), json!(warped));
    out.insert("mode".into(), json!(mode_name(stretch)));
    out.insert("transpose".into(), json!(transpose));
    Ok(Value::Object(out))
}
