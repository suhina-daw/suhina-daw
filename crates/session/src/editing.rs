// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Splitting an arrangement clip and consolidating a range of one track into one clip
//! (`docs/spec/arrangement-editing.md` › Splitting, Consolidating).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_model::{
    AudioRegion, Clip, ClipContent, ContentId, ContentLane, LaneData, Note, Project, StretchMode, TrackId,
};

use crate::commands::{content_at, make_room, number, track_id};
use crate::{CommandError, Session};

/// How close to a clip's edge, in beats, a cut may be; closer would leave a part with no length.
const MIN_PART: f64 = 1e-6;
/// The sample rate a consolidated audio clip is rendered at when its sources do not say.
const DEFAULT_RATE: u32 = 48_000;
/// Files with the same name tried before giving up on finding a free one.
pub(crate) const MAX_NAME_TRIES: u32 = 1000;
/// The folder next to the project file that consolidated audio is written to.
pub(crate) const CONSOLIDATED_FOLDER: &str = "Consolidated";

/// `clip.split {track, index, at}`: cuts a clip in two at song position `at`.
pub(crate) fn split_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let at = number(params, "at")?;
    let (lane, index) = crate::commands::lane_and_index(&session.project, params)?;
    let p = &mut session.project;
    let clip = p.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).cloned().ok_or("the clip is missing")?;
    let end = clip.start + clip.length;
    if !(at > clip.start + MIN_PART && at < end - MIN_PART) {
        return Err(format!("at must be inside the clip, between beats {} and {end}, got {at}", clip.start).into());
    }
    let rate = content_rate(p, &clip);
    let (first, mut second) = split(&clip, at, rate);
    // The first part is the clip, shortened; the second is a new clip.
    second.id = p.new_clip_id()?;
    let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
    if let Some(slot) = lane.clips.get_mut(index) {
        *slot = first.clone();
    }
    lane.clips.push(second.clone());
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let (first_id, second_id) = (first.id, second.id);
    let first = lane.clips.iter().position(|c| c.id == first_id).unwrap_or_default();
    let track = lane.track;
    // The track's take lanes are cut at the same beat, so they stay cut where the comp is (#989).
    for take in p.arrangement.takes.iter_mut().filter(|t| Some(t.track) == track) {
        if let Some(i) = take.clips.iter().position(|c| at > c.start + MIN_PART && at < c.start + c.length - MIN_PART)
            && let Some(clip) = take.clips.get(i).cloned()
        {
            let (head, tail) = split(&clip, at, 1.0);
            if let Some(slot) = take.clips.get_mut(i) {
                *slot = head;
            }
            take.clips.insert(i + 1, tail);
        }
    }
    let lane = p.arrangement.lanes.iter().find(|l| l.track == track && l.clips.iter().any(|c| c.id == first_id));
    let first = lane.and_then(|l| l.clips.iter().position(|c| c.id == first_id)).unwrap_or(first);
    let second = lane.and_then(|l| l.clips.iter().position(|c| c.id == second_id)).unwrap_or_default();
    Ok(json!({ "first": first, "second": second, "first_clip": first_id.0, "second_clip": second_id.0, "at": at }))
}

/// The two parts of `clip` cut at song position `at` (inside it). The second part plays the
/// content from where the clip was at the cut; the first keeps the fade-in, the second the
/// fade-out, and the fades at the cut are none.
pub(crate) fn split(clip: &Clip, at: f64, rate: f64) -> (Clip, Clip) {
    let before = at - clip.start;
    let mut first = Clip { length: before, fade_out: 0.0, fade_out_curve: Default::default(), ..clip.clone() };
    first.fit_fades();
    let mut second = Clip {
        start: at,
        length: clip.length - before,
        offset: content_at(clip, before, rate),
        fade_in: 0.0,
        fade_in_curve: Default::default(),
        ..clip.clone()
    };
    second.fit_fades();
    (first, second)
}

/// Content beats a clip moves through per song beat: a transposed audio clip that is not warped
/// plays faster or slower, as the engine plays it; everything else 1.
pub(crate) fn content_rate(p: &Project, clip: &Clip) -> f64 {
    let region = p.content(clip.content).and_then(|c| {
        c.lanes.iter().find_map(|l| match &l.data {
            LaneData::Audio(region) => Some(region),
            LaneData::Notes(_) => None,
        })
    });
    match region {
        Some(r) if !r.warped => {
            let rate = 2f64.powf(r.transpose / 12.0);
            if rate.is_finite() && rate > 0.0 { rate } else { 1.0 }
        }
        _ => 1.0,
    }
}

/// `clip.consolidate {track, start, end, clips?}`: the range on one track becomes one clip. With
/// `clips` (clip ids), only those clips are joined and cut away, the others (another row of a
/// stack) left as they are.
pub(crate) fn consolidate(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let start = number(params, "start")?;
    let end = number(params, "end")?;
    if start < 0.0 || end > suhina_engine::MAX_START_BEAT {
        return Err(format!("start and end must be from 0 to {}", suhina_engine::MAX_START_BEAT).into());
    }
    if end - start < MIN_PART {
        return Err(format!("end must be after start, got start {start} and end {end}").into());
    }
    let track = track_id(&session.project, params)?;
    let only: Option<Vec<u32>> = match params.get("clips") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_array()
                .ok_or("clips must be a list of clip ids")?
                .iter()
                .map(|id| id.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("clips must be a list of clip ids"))
                .collect::<Result<_, _>>()?,
        ),
    };
    let chosen = |c: &Clip| only.as_ref().is_none_or(|ids| ids.contains(&c.id.0));
    let p = &session.project;
    let lane =
        p.arrangement.lanes.iter().position(|l| l.track == Some(track)).ok_or("the track has no arrangement lane")?;
    let clips: Vec<Clip> = p
        .arrangement
        .lanes
        .get(lane)
        .map(|l| {
            l.clips
                .iter()
                .filter(|c| c.enabled && c.start < end && c.start + c.length > start && chosen(c))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    if clips.is_empty() {
        return Err(format!("track {:?} has no clips between beats {start} and {end}", track_name(p, track)).into());
    }
    let mut notes = false;
    let mut audio = false;
    for clip in &clips {
        let content = p.content(clip.content).ok_or("a clip's content is missing")?;
        for l in &content.lanes {
            if l.target.is_some_and(|t| t != track) {
                return Err("a clip that plays on other tracks (a shared pattern) cannot be consolidated".into());
            }
            match l.data {
                LaneData::Notes(_) => notes = true,
                LaneData::Audio(_) => audio = true,
            }
        }
    }
    let name = clips.first().and_then(|c| p.content(c.content)).map(|c| c.name.clone()).unwrap_or_default();
    // A MIDI clip keeps the scale of the first clip, as it keeps its name (ADR 0001 › clip scales).
    let scale = if notes { clips.first().and_then(|c| p.content(c.content)).and_then(|c| c.scale) } else { None };
    let data = match (notes, audio) {
        (true, true) => return Err("the range holds both MIDI and audio clips".into()),
        (false, false) => return Err("the clips in the range play nothing".into()),
        (true, false) => LaneData::Notes(joined_notes(p, &clips, start, end)),
        (false, true) => LaneData::Audio(render_audio(session, track, &clips, start, end)?),
    };
    let file = match &data {
        LaneData::Audio(region) => Some(region.file.clone()),
        LaneData::Notes(_) => None,
    };
    let p = &mut session.project;
    let id = ContentId(p.contents.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many clips")?);
    p.contents.push(ClipContent {
        id,
        name,
        length: end - start,
        lanes: vec![ContentLane { target: None, data }],
        extensions: Default::default(),
        scale,
    });
    let clip = Clip {
        id: p.new_clip_id()?,
        content: id,
        start,
        length: end - start,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        color: None,
        ara: false,
        extensions: Default::default(),
    };
    let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
    // Only the joined clips give way; others (another row of a stack) stay as they are.
    let (mut joined, kept): (Vec<Clip>, Vec<Clip>) = std::mem::take(&mut lane.clips).into_iter().partition(chosen);
    make_room(&mut joined, start, end);
    lane.clips = kept;
    lane.clips.extend(joined);
    lane.clips.push(clip.clone());
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let index = lane.clips.iter().position(|c| c.id == clip.id).unwrap_or_default();
    Ok(
        json!({ "clip": clip.id.0, "track": track.0, "index": index, "start": start, "length": end - start, "file": file }),
    )
}

/// The notes the clips play between `start` and `end`, as one content from `start`: each note
/// where it sounds, cut where its clip ends or the range ends. A note that starts before the
/// range is left out, as a split leaves it out of the part after the cut.
fn joined_notes(p: &Project, clips: &[Clip], start: f64, end: f64) -> Vec<Note> {
    let mut joined = Vec::new();
    for clip in clips {
        let Some(content) = p.content(clip.content) else { continue };
        for lane in &content.lanes {
            let LaneData::Notes(notes) = &lane.data else { continue };
            for (at, note) in placed_notes(clip, notes) {
                if at < start || at >= end {
                    continue;
                }
                let stop = (at + note.duration.max(0.0)).min(clip.start + clip.length).min(end);
                joined.push(Note { time: at - start, duration: stop - at, ..note });
            }
        }
    }
    joined.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.key.cmp(&b.key)));
    joined
}

/// Each note a clip plays, with the song beat it starts on, as the engine places them: from the
/// start marker, around the loop until the clip ends. Notes are bounded by the content and the
/// loop passes by the clip's length, as in the engine.
fn placed_notes(clip: &Clip, notes: &[Note]) -> Vec<(f64, Note)> {
    let mut placed = Vec::new();
    if !(clip.length.is_finite() && clip.length > 0.0 && clip.start.is_finite() && clip.offset.is_finite()) {
        return placed;
    }
    let clip_end = clip.start + clip.length;
    let mut pass = |from: f64, to: f64, at: f64| {
        for note in notes {
            if !(note.time.is_finite() && note.duration.is_finite()) || note.time < from || note.time >= to {
                continue;
            }
            let beat = clip.start + at + (note.time - from);
            if beat < clip_end {
                placed.push((beat, *note));
            }
        }
    };
    match clip.loop_range {
        Some(range) if range.end.is_finite() && range.start.is_finite() && range.end > range.start => {
            let first_from = clip.offset.clamp(range.start, range.end);
            pass(first_from, range.end, 0.0);
            let mut at = range.end - first_from;
            let mut passes = 0;
            while at < clip.length && passes < 100_000 {
                pass(range.start, range.end, at);
                at += range.end - range.start;
                passes += 1;
            }
        }
        _ => pass(clip.offset, f64::INFINITY, 0.0),
    }
    placed
}

/// Renders the audio clips between `start` and `end` on `track` (their gains, fades and warping,
/// no devices, gaps silent) to a new WAV file in the project's folder, and returns the region
/// that plays it. The rendered audio joins the session's pool.
fn render_audio(
    session: &mut Session,
    track: TrackId,
    clips: &[Clip],
    start: f64,
    end: f64,
) -> Result<AudioRegion, CommandError> {
    let folder = session
        .source()
        .and_then(Path::parent)
        .map(|f| f.join(CONSOLIDATED_FOLDER))
        .ok_or("save the project first: consolidated audio is written to a folder next to the project file")?;
    let p = &session.project;
    let rate = clips
        .iter()
        .filter_map(|c| p.content(c.content))
        .flat_map(|c| &c.lanes)
        .filter_map(|l| match &l.data {
            LaneData::Audio(region) => session.audio().files.get(region.pool_key().as_ref()).map(|d| d.sample_rate),
            LaneData::Notes(_) => None,
        })
        .max()
        .unwrap_or(DEFAULT_RATE);
    // The clips as they are, moved so the earliest starts at 0: a clip reaching into the range
    // plays into it exactly as it did, warped clips with their stretchers already running.
    let shift = clips.iter().map(|c| c.start).fold(start, f64::min);
    let mut alone = p.clone();
    if let Some(lane) = alone.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) {
        lane.clips = clips.iter().map(|c| Clip { start: c.start - shift, ..c.clone() }).collect();
    }
    let frames_per_beat = 60.0 / p.transport.tempo_bpm * f64::from(rate);
    if !(frames_per_beat.is_finite() && frames_per_beat > 0.0) {
        return Err("the project's tempo is not usable".into());
    }
    let skip = ((start - shift) * frames_per_beat).round();
    let frames = ((end - shift) * frames_per_beat).round();
    let limit = 60.0 * 60.0 * f64::from(rate);
    if frames > limit {
        return Err("the range is too long to render: at most an hour of audio, counted from the earliest clip".into());
    }
    let (skip, frames) = (skip as usize, frames as usize);
    let rendered = suhina_engine::render_track(&alone, session.audio(), track, rate, frames)
        .map_err(|e| format!("could not render the range: {e}"))?;
    let samples = rendered.get(skip.saturating_mul(2)..).unwrap_or_default().to_vec();
    std::fs::create_dir_all(&folder).map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    let base = sanitized(&track_name(p, track));
    let bar = start / bar_beats(p) + 1.0;
    let path = free_path(&folder, &format!("{base} bar {}", bar.floor()))?;
    write_wav(&path, rate, &samples)?;
    let key = crate::files::absolute(&path).display().to_string();
    let seconds = (samples.len() / 2) as f64 / f64::from(rate);
    session.add_audio(key.clone(), suhina_engine::AudioData { sample_rate: rate, channels: 2, samples });
    Ok(AudioRegion {
        file: key.clone(),
        absolute_file: Some(key),
        sample_rate: Some(rate),
        channels: Some(2),
        duration_seconds: Some(seconds),
        warped: false,
        stretch: StretchMode::default(),
        warp_markers: Vec::new(),
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    })
}

fn bar_beats(p: &Project) -> f64 {
    let ts = p.transport.time_signature;
    if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) }
}

fn track_name(p: &Project, track: TrackId) -> String {
    p.shown_track_name(track)
}

/// A track name made safe as part of a file name on every system.
pub(crate) fn sanitized(name: &str) -> String {
    let clean: String =
        name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '_' }).collect();
    let clean = clean.trim();
    if clean.is_empty() { "Track".into() } else { clean.chars().take(60).collect() }
}

/// `<folder>/<name>.wav`, or with " 2", " 3" … after the name when that file exists.
fn free_path(folder: &Path, name: &str) -> Result<PathBuf, CommandError> {
    for n in 1..=MAX_NAME_TRIES {
        let file = if n == 1 { format!("{name}.wav") } else { format!("{name} {n}.wav") };
        let path = folder.join(file);
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(format!("no free file name for {name:?} in {}", folder.display()).into())
}

/// Writes interleaved stereo as a 32-bit float WAV file, through the one writer every export
/// and preview writes with (`media::write`). A bounce rewrites whatever it wrote before, by
/// its own new name each time.
fn write_wav(path: &Path, rate: u32, samples: &[f32]) -> Result<(), CommandError> {
    use suhina_media::write::WriteMode;
    suhina_media::write::write_audio_file(
        path,
        samples,
        &suhina_media::write::AudioFileSpec::float_wav(rate),
        WriteMode::Replace,
    )
    .map_err(|e| CommandError(format!("could not write {e}")))
}
