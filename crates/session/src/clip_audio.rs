// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's own audio state set by commands: its gain envelope (ADR 0035) and the tempo its
//! audio is warped from (#824, `docs/spec/clip-panel.md` › Tempo). Every clip has its own; a clip
//! of the same audio elsewhere is left as it is (#742).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_model::{CLIP_TEMPO_RANGE, Clip, GAIN_POINT_RANGE, GainPoint, MAX_GAIN_POINTS, TempoSource};

use crate::commands::Args;
use crate::notes::{ClipAt, clip_mut, locate};
use crate::{CommandError, Session};

/// The clip `params` names, where it is, and the audio region it plays; fails for a clip without
/// audio, with "only an audio clip {what}"; a selector that names no clip keeps its own error.
fn audio_clip(
    session: &Session,
    params: &Value,
    what: &str,
) -> Result<(ClipAt, Clip, suhina_model::AudioRegion), CommandError> {
    let (at, content, track) = locate(&session.project, params)?;
    let region = crate::audio_timing::region_on(&session.project, content, track)
        .cloned()
        .ok_or_else(|| format!("only an audio clip {what}"))?;
    let clip = crate::notes::clip_at(&session.project, at).cloned().ok_or("the clip is missing")?;
    Ok((at, clip, region))
}

/// `clip.setEnvelope`: an audio clip's gain envelope, as pairs of a content beat and an offset in
/// dB from the clip's gain; an empty list clears it. Only this clip changes.
pub(crate) fn set_envelope(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let values = Args::new(params)
        .value("points")
        .and_then(Value::as_array)
        .ok_or("points must be a list of numbers: a beat and a dB for each point")?;
    if values.len() % 2 != 0 {
        return Err("points must come in pairs: a beat and a dB for each point".into());
    }
    if values.len() / 2 > MAX_GAIN_POINTS {
        return Err(format!("an envelope has at most {MAX_GAIN_POINTS} points").into());
    }
    let (low, high) = (*GAIN_POINT_RANGE.start(), *GAIN_POINT_RANGE.end());
    // Each point's curve to the next, as automation's: a bend from -1 to 1 or four numbers;
    // missing or null for a straight one.
    let curves = match Args::new(params).value("curves") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) if list.len() <= values.len() / 2 => list.clone(),
        Some(_) => return Err("curves must be a list with at most one curve for each point".into()),
    };
    let mut points = Vec::with_capacity(values.len() / 2);
    for (index, [beat, db]) in values.as_chunks::<2>().0.iter().enumerate() {
        let (Some(beat), Some(db)) = (beat.as_f64(), db.as_f64()) else {
            return Err("points must be numbers".into());
        };
        if !(beat.is_finite() && (0.0..=suhina_engine::MAX_START_BEAT).contains(&beat)) {
            return Err(
                format!("a point's beat must be from 0 to {}, got {beat}", suhina_engine::MAX_START_BEAT).into()
            );
        }
        if !(db.is_finite() && (low..=high).contains(&db)) {
            return Err(format!("a point's dB must be from {low} to +{high}, got {db}").into());
        }
        let curve = match curves.get(index) {
            None | Some(Value::Null) => None,
            Some(curve) => crate::automation::curve(&json!({ "curve": curve }))?,
        };
        points.push(GainPoint { beat, db, curve: curve.unwrap_or_default() });
    }
    // In beat order; points on one beat keep the order they were given in, so a step can be made.
    points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    let points = merged(points);
    let (at, _, _) = audio_clip(session, params, "has a gain envelope")?;
    let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
    clip.audio.gain_envelope = points;
    // Only the envelope changed: an arrangement clip's goes to the playing engine as a message.
    // An ARA clip's is applied by its host's chain, which only a new graph reaches.
    session.sync.envelope_edit = match at {
        ClipAt::Arrangement(lane, index) if !clip.ara => Some((lane, index)),
        _ => None,
    };
    let listed: Vec<f64> = clip.audio.gain_envelope.iter().flat_map(|p| [p.beat, p.db]).collect();
    let curves: Vec<[f64; 4]> = clip.audio.gain_envelope.iter().map(|p| p.curve.to_array()).collect();
    Ok(json!({ "clip": clip.id.0, "points": listed, "curves": curves }))
}

/// `points` (in beat order) with no two in one place: a point on the beat and dB of the one before
/// it is one point, with the later one's curve to the next. On one beat at most two are kept, the
/// first and the last, which draw a step; one between them would only sit on the step's edge.
fn merged(points: Vec<GainPoint>) -> Vec<GainPoint> {
    const SAME_BEAT: f64 = 1e-9;
    const SAME_DB: f64 = 1e-6;
    let mut out: Vec<GainPoint> = Vec::with_capacity(points.len());
    for point in points {
        let on_beat = |p: &GainPoint| (p.beat - point.beat).abs() <= SAME_BEAT;
        // Whether the two before it are on its beat already: it would be a third.
        let third = out.len() >= 2 && out.iter().rev().take(2).all(on_beat);
        match out.last_mut() {
            Some(last) if on_beat(last) && (last.db - point.db).abs() <= SAME_DB => last.curve = point.curve,
            // A third point on the beat replaces the second: the step goes from the first to it.
            Some(last) if third => *last = point,
            _ => out.push(point),
        }
    }
    out
}

/// A tempo source by the name the commands use.
fn source_named(params: &Value) -> Result<TempoSource, CommandError> {
    match Args::new(params).value("source") {
        None | Some(Value::Null) => Ok(TempoSource::Manual),
        Some(Value::String(name)) => TempoSource::from_name(name)
            .ok_or_else(|| format!("source must be one of metadata, name, detected or manual, got {name:?}").into()),
        Some(_) => Err("source must be a name: metadata, name, detected or manual".into()),
    }
}

/// `clip.setTempo`: the tempo an audio clip's audio is warped from. The audio then fills as many
/// beats as it holds at that tempo: a higher tempo gives more beats, so it plays slower against
/// the song, and the clip's length follows while its start stays. Warp turns on. A clip whose
/// content another clip plays gets a content of its own first.
pub(crate) fn set_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let bpm = Args::new(params).f64("bpm")?;
    if !CLIP_TEMPO_RANGE.contains(&bpm) {
        let (low, high) = (CLIP_TEMPO_RANGE.start(), CLIP_TEMPO_RANGE.end());
        return Err(format!("bpm must be from {low} to {high}, got {bpm}").into());
    }
    let source = source_named(params)?;
    let (at, mut clip, region) = audio_clip(session, params, "has a tempo")?;
    let content_length = session.project.content(clip.content).map_or(0.0, |c| c.length);
    let seconds = region.duration_seconds.filter(|s| s.is_finite() && *s > 0.0);
    // A clip that is not warped plays its file at its own speed: its content beats are its seconds
    // at the song's tempo, as the engine counts them, whatever markers it kept from before. It is
    // warped from there, on two markers that map it so.
    let mut unwarped_beats = None;
    let old = if clip.audio.warped {
        match clip.audio.tempo() {
            Some(old) => old,
            None => {
                content_length / seconds.ok_or("the audio's length is not known, so its tempo cannot be set")? * 60.0
            }
        }
    } else {
        let seconds = seconds.ok_or("the audio's length is not known, so its tempo cannot be set")?;
        let tempo = session.project.transport.tempo_bpm;
        let beats = seconds * tempo / 60.0;
        clip.audio.warp_markers = vec![
            suhina_model::WarpMarker { time: 0.0, audio_time: 0.0 },
            suhina_model::WarpMarker { time: beats, audio_time: seconds },
        ];
        unwarped_beats = Some(beats);
        tempo
    };
    if !(old.is_finite() && old > 0.0) {
        return Err("the clip's tempo now is not known".into());
    }
    let factor = bpm / old;
    crate::clip_edges::scale_audio(&mut session.project, &mut clip, factor)?;
    // The content is the clip's own now (a shared one was copied); its length follows the audio.
    if let Some(beats) = unwarped_beats
        && let Some(content) = session.project.contents.iter_mut().find(|c| c.id == clip.content)
    {
        content.length = beats * factor;
    }
    clip.length *= factor;
    clip.fit_fades();
    clip.audio.tempo_source = Some(source);
    let id = clip.id;
    match at {
        // An arrangement clip made longer stops at the next clip on its lane: a tempo, dragged while
        // it plays, never removes the clips after it.
        ClipAt::Arrangement(lane, index) => {
            let clips = &mut session.project.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?.clips;
            let next = clips
                .iter()
                .enumerate()
                .filter(|(i, c)| *i != index && c.start > clip.start)
                .map(|(_, c)| c.start)
                .fold(f64::INFINITY, f64::min);
            if clip.start + clip.length > next {
                clip.length = (next - clip.start).max(1.0 / 64.0);
                clip.fit_fades();
            }
            *clips.get_mut(index).ok_or("the clip is missing")? = clip.clone();
        }
        ClipAt::Slot(..) => *clip_mut(&mut session.project, at).ok_or("the clip is missing")? = clip.clone(),
    }
    Ok(json!({
        "clip": id.0,
        "bpm": clip.audio.tempo().unwrap_or(bpm),
        "source": source.name(),
        "length": clip.length,
    }))
}

/// The tempo of the file at `path` by each source: its metadata, its name, and detection in its
/// decoded audio `data`. Any of them may be missing.
pub(crate) fn tempo_candidates(path: &Path, data: Option<&suhina_engine::AudioData>) -> [Option<f64>; 3] {
    let info = suhina_media::info::sample_info(path).ok().and_then(|i| i.tempo);
    let metadata = info.filter(|t| t.source == suhina_media::info::TempoSource::Metadata).map(|t| t.bpm);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let name = suhina_media::info::tempo_from_name(stem);
    let detected = data.and_then(suhina_media::tempo::detect);
    [metadata, name, detected]
}

/// What each source says the tempo of clip `id`'s audio is (metadata, file name, detection), for
/// the clip panel's Find menu; `None` for a clip without audio or one that is not there.
pub fn clip_tempo_candidates(session: &Session, id: suhina_model::ClipId) -> Option<[Option<f64>; 3]> {
    let clip = session.project.all_clips().find(|c| c.id == id)?;
    let track = match session.project.clip_place(id)? {
        suhina_model::ClipPlace::Lane { lane, .. } => session.project.arrangement.lanes.get(lane)?.track?,
        suhina_model::ClipPlace::Slot { column, .. } => session.project.launcher.columns.get(column)?.track,
    };
    let region = crate::audio_timing::region_on(&session.project, clip.content, track)?;
    let path = PathBuf::from(region.absolute_file.as_deref().unwrap_or(&region.file));
    let data = session.audio().files.get(region.pool_key().as_ref()).cloned();
    Some(tempo_candidates(&path, data.as_deref()))
}

/// `clip.findTempo`: what each source says the clip's audio tempo is, in the order a drop reads
/// them (its metadata, its name, detection), and the tempo the clip has now with its source. A
/// source that finds nothing says null. Changes nothing.
pub(crate) fn find_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (_, clip, region) = audio_clip(session, params, "has a tempo")?;
    let path = PathBuf::from(region.absolute_file.as_deref().unwrap_or(&region.file));
    let data = session.audio().files.get(region.pool_key().as_ref()).cloned();
    let [metadata, name, detected] = tempo_candidates(&path, data.as_deref());
    let found = [metadata, name, detected].into_iter().flatten().next();
    Ok(json!({
        "clip": clip.id.0,
        "metadata": metadata,
        "name": name,
        "detected": detected,
        "found": found,
        "bpm": clip.audio.tempo().filter(|_| clip.audio.warped),
        "source": clip.audio.tempo_source.map(TempoSource::name),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_named_with_its_tempo_has_that_tempo_by_name() {
        let [metadata, name, detected] = tempo_candidates(Path::new("does-not-exist/drums_96bpm.wav"), None);
        assert_eq!((metadata, name, detected), (None, Some(96.0), None));
    }
}
