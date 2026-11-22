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

/// Two warp markers closer than this many beats are one.
const SAME_MARKER: f64 = 1e-6;
/// Most warp markers a clip keeps, as the engine reads them.
const MAX_MARKERS: usize = 100_000;

/// The clip's warp markers as the engine plays them: finite, in beat order, one to a beat (the
/// first of those on it). Fails for a clip that is not warped or has fewer than two.
fn marker_list(clip: &Clip) -> Result<Vec<suhina_model::WarpMarker>, CommandError> {
    if !clip.audio.warped {
        return Err("warp is off: turn Warp on to edit warp markers".into());
    }
    let mut markers: Vec<suhina_model::WarpMarker> =
        clip.audio.warp_markers.iter().copied().filter(|m| m.time.is_finite() && m.audio_time.is_finite()).collect();
    markers.sort_by(|a, b| a.time.total_cmp(&b.time));
    markers.dedup_by(|a, b| (a.time - b.time).abs() <= SAME_MARKER);
    if markers.len() < 2 {
        return Err("the clip needs two warp markers: set its tempo first".into());
    }
    Ok(markers)
}

/// A marker's beat, read from `name`: finite and within the beats a clip can start at.
fn marker_beat(params: &Value, name: &str) -> Result<f64, CommandError> {
    let beat = Args::new(params).f64(name)?;
    let most = suhina_engine::MAX_START_BEAT;
    if !(beat.is_finite() && (-most..=most).contains(&beat)) {
        return Err(format!("{name} must be from -{most} to {most}, got {beat}").into());
    }
    Ok(beat)
}

/// Whether the markers rise in file seconds as well as in beats, as the engine needs.
fn rising(markers: &[suhina_model::WarpMarker]) -> bool {
    markers.windows(2).all(|w| matches!(w, [a, b] if b.time > a.time && b.audio_time > a.audio_time))
}

/// The markers as `[beat, seconds]` pairs, for a command's answer.
fn listed(markers: &[suhina_model::WarpMarker]) -> Vec<[f64; 2]> {
    markers.iter().map(|m| [m.time, m.audio_time]).collect()
}

/// Stores `markers` on the clip `at` (which was `before`, playing `region`) and answers with them.
fn store_markers(
    session: &mut Session,
    at: ClipAt,
    before: &Clip,
    region: &suhina_model::AudioRegion,
    markers: Vec<suhina_model::WarpMarker>,
) -> Result<Value, CommandError> {
    let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
    clip.audio.warp_markers = markers;
    // The tempo the markers make is the user's now.
    clip.audio.tempo_source = Some(TempoSource::Manual);
    follow_audio_end(session, at, before, region)?;
    let clip = crate::notes::clip_at(&session.project, at).ok_or("the clip is missing")?;
    Ok(json!({ "clip": clip.id.0, "markers": listed(&clip.audio.warp_markers) }))
}

/// After the warp markers of the clip `at` changed from `before`'s: moves the end of its content
/// to where the end of its audio is now, so a clip whose audio was stretched longer can be drawn
/// out over all of it (and one squeezed shorter stops where it ends), its own start and length
/// left as they are (`docs/spec/ui-audio-editor.md` › Warp markers). A content other clips play
/// too is copied first, as their warp is their own.
pub(crate) fn follow_audio_end(
    session: &mut Session,
    at: ClipAt,
    before: &Clip,
    region: &suhina_model::AudioRegion,
) -> Result<(), CommandError> {
    let Some(seconds) = region.duration_seconds.filter(|s| s.is_finite() && *s > 0.0) else { return Ok(()) };
    let p = &mut session.project;
    let tempo = p.transport.tempo_bpm;
    let now = crate::notes::clip_at(p, at).ok_or("the clip is missing")?;
    // Where the audio's end falls in content beats: by the warp markers when warped, else at the
    // tempo where the clip plays, as `clip.setAudio` reads it (an arrangement clip on the song's
    // tempo map from its content's beat 0, a slot's clip at the song's tempo): a clip in a slower
    // section of the song does not grow by the song's main tempo.
    let place = crate::clip_shape::Place::of(now, matches!(at, ClipAt::Slot(..)));
    let end_of = |audio: &suhina_model::ClipAudio| {
        if audio.warped {
            crate::audio_timing::Mapping::of(audio, tempo).map(|m| m.beat(seconds))
        } else {
            Some(place.beats_for(&p.transport, seconds))
        }
    };
    let (Some(old), Some(new)) = (end_of(&before.audio), end_of(&now.audio)) else {
        return Ok(());
    };
    let moved = new - old;
    if !moved.is_finite() || moved.abs() < SAME_MARKER {
        return Ok(());
    }
    let content = now.content;
    let lanes = p.arrangement.lanes.iter().flat_map(|l| &l.clips);
    let takes = p.arrangement.takes.iter().flat_map(|t| &t.clips);
    let slots = p.launcher.columns.iter().flat_map(|c| c.slots.iter().filter_map(|s| s.clip.as_ref()));
    if lanes.chain(takes).chain(slots).filter(|c| c.content == content).count() > 1 {
        let id = crate::commands::new_content_id(p)?;
        let copy = p.content(content).cloned().ok_or("the clip's content is missing")?;
        p.contents.push(suhina_model::ClipContent { id, ..copy });
        clip_mut(p, at).ok_or("the clip is missing")?.content = id;
        return grow(p, id, moved);
    }
    grow(p, content, moved)
}

/// Moves the end of `content` by `beats`, keeping it a 64th of a beat long at least.
fn grow(p: &mut suhina_model::Project, content: suhina_model::ContentId, beats: f64) -> Result<(), CommandError> {
    let content = p.contents.iter_mut().find(|c| c.id == content).ok_or("the clip's content is missing")?;
    let length = content.length + beats;
    if length.is_finite() {
        content.length = length.max(1.0 / 64.0);
    }
    Ok(())
}

/// `clip.addWarpMarker`: pins the sound that plays at content beat `beat` to that beat, so nothing
/// moves yet; dragging the marker (`clip.moveWarpMarker`) then moves the sound. A beat that has a
/// marker already changes nothing.
pub(crate) fn add_warp_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beat = marker_beat(params, "beat")?;
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let mut markers = marker_list(&clip)?;
    if markers.iter().any(|m| (m.time - beat).abs() <= SAME_MARKER) {
        return Ok(json!({ "clip": clip.id.0, "markers": listed(&markers) }));
    }
    if markers.len() >= MAX_MARKERS {
        return Err(format!("a clip has at most {MAX_MARKERS} warp markers").into());
    }
    let mapping = crate::audio_timing::Mapping::of(&clip.audio, session.project.transport.tempo_bpm);
    let Some(mapping @ crate::audio_timing::Mapping::Warp(_)) = mapping else {
        return Err("the clip's warp markers must rise in beats and in seconds".into());
    };
    let seconds = mapping.seconds(beat);
    if !(seconds.is_finite() && seconds >= 0.0) {
        return Err("that beat is before the start of the audio".into());
    }
    markers.push(suhina_model::WarpMarker { time: beat, audio_time: seconds });
    markers.sort_by(|a, b| a.time.total_cmp(&b.time));
    if !rising(&markers) {
        return Err("a warp marker must lie between the sounds of its neighbours".into());
    }
    store_markers(session, at, &clip, &region, markers)
}

/// Two warp markers placed at transients are at least this many seconds apart in the file, and a
/// transient this close to a marker the clip has already gets none.
const TRANSIENT_GAP_SECONDS: f64 = 0.04;

/// How far, in seconds, a note's start found from its spectrum moves to an attack found from its
/// level: the level places an attack to the millisecond, the spectrum within a window's length.
const SNAP_SECONDS: f64 = 0.025;
/// Two transients a marker goes on are at least this far apart (as `note_onsets` keeps them).
const MARKER_GAP_SECONDS: f64 = 0.1;

/// The file seconds of `data`'s transients a warp marker goes on, in order: what
/// `clip.addWarpMarkersAtTransients` pins markers on and the audio editor suggests. They are where
/// notes start (`suhina_stretch::note_onsets`: where the spectrum rises and the pitches sounding
/// change or the level jumps, so a legato arpeggio's notes count and a held chord's wobble does
/// not), each moved onto the attack the level finds within 25 ms of it
/// (`suhina_stretch::transients`, the Rhythmic mode's), at least [`MARKER_GAP_SECONDS`] apart.
/// Empty for a file with no sample rate.
pub fn transient_seconds(data: &suhina_engine::AudioData) -> Vec<f64> {
    let rate = f64::from(data.sample_rate);
    let channels = usize::from(data.channels).max(1);
    if rate <= 0.0 {
        return Vec::new();
    }
    let attacks = suhina_stretch::transients(&data.samples, channels, data.sample_rate as f32);
    let snap = (SNAP_SECONDS * rate) as usize;
    let mut found: Vec<f64> = Vec::new();
    for onset in suhina_stretch::note_onsets(&data.samples, channels, data.sample_rate as f32) {
        // The attacks are in order: the nearest is one of the two either side of the onset.
        let at = attacks.partition_point(|a| *a < onset);
        let nearest = [at.checked_sub(1), Some(at)]
            .into_iter()
            .flatten()
            .filter_map(|i| attacks.get(i).copied())
            .filter(|a| a.abs_diff(onset) <= snap)
            .min_by_key(|a| a.abs_diff(onset));
        let seconds = nearest.unwrap_or(onset) as f64 / rate;
        if found.last().is_none_or(|last| seconds - last >= MARKER_GAP_SECONDS) {
            found.push(seconds);
        }
    }
    found
}

/// `clip.addWarpMarkersAtTransients`: pins a warp marker on every transient of the clip's file
/// (where its level jumps, as the Rhythmic mode finds them), each at the beat where that sound
/// plays now, so nothing moves yet; dragging one then puts its hit on a beat. A transient near a
/// marker the clip has keeps that marker. One undo step (#1185).
pub(crate) fn add_warp_markers_at_transients(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let mut markers = marker_list(&clip)?;
    let mapping = crate::audio_timing::Mapping::of(&clip.audio, session.project.transport.tempo_bpm);
    let Some(mapping @ crate::audio_timing::Mapping::Warp(_)) = mapping else {
        return Err("the clip's warp markers must rise in beats and in seconds".into());
    };
    let data = session
        .audio()
        .files
        .get(region.pool_key().as_ref())
        .cloned()
        .ok_or("the clip's audio is not loaded, so its transients cannot be found")?;
    if data.sample_rate == 0 {
        return Err("the clip's audio has no sample rate".into());
    }
    let mut added = 0usize;
    let existing = markers.clone();
    for seconds in transient_seconds(&data) {
        if markers.iter().any(|m| (m.audio_time - seconds).abs() < TRANSIENT_GAP_SECONDS) {
            continue;
        }
        if markers.len() >= MAX_MARKERS {
            break;
        }
        let beat = mapping.beat(seconds);
        if !beat.is_finite() {
            continue;
        }
        // A pin the clip has keeps its place: a transient too close to one in beats (closer than
        // a marker may be to its neighbour) gets none, rather than the merge below dropping the
        // existing pin for it.
        if existing.iter().any(|m| (m.time - beat).abs() <= SAME_MARKER) {
            continue;
        }
        markers.push(suhina_model::WarpMarker { time: beat, audio_time: seconds });
        added += 1;
    }
    if added == 0 {
        return Ok(json!({ "clip": clip.id.0, "added": 0, "markers": listed(&markers) }));
    }
    markers.sort_by(|a, b| a.time.total_cmp(&b.time));
    // Read back through the markers' own mapping, so they rise; one closer than a marker can be
    // to its neighbour in beats is left out rather than refusing the rest.
    let mut kept: Vec<suhina_model::WarpMarker> = Vec::with_capacity(markers.len());
    let was_there =
        |m: &suhina_model::WarpMarker| existing.iter().any(|e| e.time == m.time && e.audio_time == m.audio_time);
    for m in markers {
        match kept.last() {
            Some(last) if m.time - last.time <= SAME_MARKER || m.audio_time <= last.audio_time => {
                // Of the two, the clip's own pin stays; the new one is left out.
                if was_there(&m) && !was_there(last) {
                    kept.pop();
                    kept.push(m);
                }
                added = added.saturating_sub(1)
            }
            _ => kept.push(m),
        }
    }
    if !rising(&kept) {
        return Err("the transients could not be placed between the clip's markers".into());
    }
    let mut answer = store_markers(session, at, &clip, &region, kept)?;
    if let Some(map) = answer.as_object_mut() {
        map.insert("added".into(), json!(added));
    }
    Ok(answer)
}

/// `clip.moveWarpMarker`: moves the marker at content beat `from` to beat `beat`; the sound it is
/// pinned to goes with it. It stays between its neighbours.
pub(crate) fn move_warp_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (from, to) = (marker_beat(params, "from")?, marker_beat(params, "beat")?);
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let mut markers = marker_list(&clip)?;
    let index = markers
        .iter()
        .position(|m| (m.time - from).abs() <= SAME_MARKER)
        .ok_or_else(|| format!("there is no warp marker at beat {from}"))?;
    let below = index.checked_sub(1).and_then(|i| markers.get(i)).map(|m| m.time);
    let above = markers.get(index + 1).map(|m| m.time);
    if below.is_some_and(|b| to <= b + SAME_MARKER) || above.is_some_and(|a| to >= a - SAME_MARKER) {
        return Err("a warp marker stays between its neighbours".into());
    }
    if let Some(marker) = markers.get_mut(index) {
        marker.time = to;
    }
    store_markers(session, at, &clip, &region, markers)
}

/// The indices in `markers` of the markers at the content beats `params[name]` names: at least one,
/// each once (a beat named twice is one marker).
fn marker_indices(
    markers: &[suhina_model::WarpMarker],
    params: &Value,
    name: &str,
) -> Result<Vec<usize>, CommandError> {
    let beats = Args::new(params).numbers(name, MAX_MARKERS)?;
    if beats.is_empty() {
        return Err(format!("{name} names no warp marker").into());
    }
    // The markers are in beat order (`marker_list`): each beat's first match is found by a binary
    // search, not a scan from the start (a selection of every one of a long clip's markers).
    let mut indices = beats
        .iter()
        .map(|beat| {
            let from = markers.partition_point(|m| m.time < beat - SAME_MARKER);
            markers
                .get(from)
                .filter(|m| (m.time - beat).abs() <= SAME_MARKER)
                .map(|_| from)
                .ok_or_else(|| CommandError::from(format!("there is no warp marker at beat {beat}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

/// `clip.moveWarpMarkers`: moves the markers at the content beats `beats` by `by` beats together,
/// each with the sound it is pinned to. The markers between them keep their order; the ones not
/// moved stay where they are, and a moved one stays between them.
pub(crate) fn move_warp_markers(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let by = Args::new(params).f64("by")?;
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let mut markers = marker_list(&clip)?;
    let indices = marker_indices(&markers, params, "beats")?;
    let most = suhina_engine::MAX_START_BEAT;
    for &index in &indices {
        if let Some(marker) = markers.get_mut(index) {
            marker.time += by;
            if !(-most..=most).contains(&marker.time) {
                return Err(format!("a warp marker moved by {by} would leave -{most} to {most}").into());
            }
        }
    }
    if markers.windows(2).any(|pair| pair[1].time <= pair[0].time + SAME_MARKER) {
        return Err("a warp marker stays between its neighbours".into());
    }
    store_markers(session, at, &clip, &region, markers)
}

/// `clip.removeWarpMarkers`: takes away the markers at the content beats `beats` in one step. A
/// warped clip keeps two.
pub(crate) fn remove_warp_markers(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let markers = marker_list(&clip)?;
    let indices = marker_indices(&markers, params, "beats")?;
    if markers.len().saturating_sub(indices.len()) < 2 {
        return Err("a warped clip keeps at least two warp markers".into());
    }
    let kept =
        markers.into_iter().enumerate().filter(|(i, _)| indices.binary_search(i).is_err()).map(|(_, m)| m).collect();
    store_markers(session, at, &clip, &region, kept)
}

/// `clip.quantizeWarpMarkers`: moves the audio's transients towards the nearest line of `grid`
/// (as `note.quantize` takes it) by `amount` percent (default 100: onto it), in the content beats
/// from `start` to `end` (both left out: the whole clip). Each transient there without a marker
/// near it gets one first, pinned where its sound plays now; then every marker in the range moves
/// with its sound. A marker that would land on or past its neighbour stays (a new one is left
/// out). One undo step.
pub(crate) fn quantize_warp_markers(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let step = crate::notes::quantize_step(&session.project, params)?;
    let args = Args::new(params);
    let amount = args.opt_f64("amount")?.unwrap_or(100.0);
    if !(0.0..=100.0).contains(&amount) {
        return Err(format!("amount must be 0 to 100 percent, got {amount}").into());
    }
    let amount = amount / 100.0;
    let most = suhina_engine::MAX_START_BEAT;
    let start = args.opt_f64("start")?.unwrap_or(-most);
    let end = args.opt_f64("end")?.unwrap_or(most);
    if !(start.is_finite() && end.is_finite() && start < end) {
        return Err(format!("start ({start}) must be before end ({end})").into());
    }
    let inside = |beat: f64| beat >= start - SAME_MARKER && beat <= end + SAME_MARKER;
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let markers = marker_list(&clip)?;
    let mapping = crate::audio_timing::Mapping::of(&clip.audio, session.project.transport.tempo_bpm);
    let Some(mapping @ crate::audio_timing::Mapping::Warp(_)) = mapping else {
        return Err("the clip's warp markers must rise in beats and in seconds".into());
    };
    let data = session
        .audio()
        .files
        .get(region.pool_key().as_ref())
        .cloned()
        .ok_or("the clip's audio is not loaded, so its transients cannot be found")?;
    if data.sample_rate == 0 {
        return Err("the clip's audio has no sample rate".into());
    }
    // (marker, whether it was there before)
    let mut all: Vec<(suhina_model::WarpMarker, bool)> = markers.iter().map(|m| (*m, true)).collect();
    for seconds in transient_seconds(&data) {
        if all.len() >= MAX_MARKERS {
            break;
        }
        if markers.iter().any(|m| (m.audio_time - seconds).abs() < TRANSIENT_GAP_SECONDS) {
            continue;
        }
        let beat = mapping.beat(seconds);
        if beat.is_finite() && inside(beat) {
            all.push((suhina_model::WarpMarker { time: beat, audio_time: seconds }, false));
        }
    }
    all.sort_by(|a, b| a.0.time.total_cmp(&b.0.time));
    let line = |beat: f64| {
        let nearest = (beat / step).round() * step;
        beat + (nearest - beat) * amount
    };
    // Each kept marker with the beat it had before the move and whether the clip had it.
    let mut kept: Vec<(suhina_model::WarpMarker, f64, bool)> = Vec::with_capacity(all.len());
    for &(marker, was_there) in &all {
        let to = if inside(marker.time) { line(marker.time) } else { marker.time };
        let after = |kept: &[(suhina_model::WarpMarker, f64, bool)], time: f64| {
            kept.last().is_none_or(|(last, _, _)| time > last.time + SAME_MARKER && marker.audio_time > last.audio_time)
        };
        if after(&kept, to) {
            kept.push((suhina_model::WarpMarker { time: to, audio_time: marker.audio_time }, marker.time, was_there));
            continue;
        }
        // Two land on one line: a new marker is left out; one the clip had stays where it was,
        // the marker before it going back to its own place if it moved over it.
        if !was_there {
            continue;
        }
        if !after(&kept, marker.time)
            && let Some(last) = kept.last_mut()
        {
            last.0.time = last.1;
        }
        if after(&kept, marker.time) {
            kept.push((marker, marker.time, true));
        } else if let Some(last) = kept.pop() {
            // The one before could not go back past it: it was new, and goes.
            if last.2 {
                kept.push(last);
            }
            kept.push((marker, marker.time, true));
        }
    }
    let moved = kept.iter().filter(|(m, from, _)| (m.time - from).abs() > SAME_MARKER).count();
    let kept: Vec<suhina_model::WarpMarker> = kept.into_iter().map(|(m, _, _)| m).collect();
    if !rising(&kept) || kept.len() < 2 {
        return Err("the markers could not be quantised without passing each other".into());
    }
    let mut answer = store_markers(session, at, &clip, &region, kept)?;
    if let Some(map) = answer.as_object_mut() {
        map.insert("moved".into(), json!(moved));
    }
    Ok(answer)
}

/// `clip.removeWarpMarker`: takes away the marker at content beat `beat`. A warped clip keeps two.
pub(crate) fn remove_warp_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beat = marker_beat(params, "beat")?;
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let mut markers = marker_list(&clip)?;
    let index = markers
        .iter()
        .position(|m| (m.time - beat).abs() <= SAME_MARKER)
        .ok_or_else(|| format!("there is no warp marker at beat {beat}"))?;
    if markers.len() <= 2 {
        return Err("a warped clip keeps at least two warp markers".into());
    }
    markers.remove(index);
    store_markers(session, at, &clip, &region, markers)
}

/// `clip.resetWarp`: puts the clip's warp back to two markers, one at each end of the audio, so
/// the audio runs evenly. By default at the tempo the clip has (or the project's, for a clip that
/// is not warped); with `detect` at the tempo found in the file's metadata, its name or its sound,
/// as `clip.findTempo` reads them. The clip's start, length and loop stay where they are.
pub(crate) fn reset_warp(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let detect = Args::new(params).opt_bool("detect")?.unwrap_or(false);
    let (at, clip, region) = audio_clip(session, params, "has warp markers")?;
    let seconds = region
        .duration_seconds
        .filter(|s| s.is_finite() && *s > 0.0)
        .ok_or("the audio's length is not known, so its warp cannot be reset")?;
    let (bpm, source) = if detect {
        let path = PathBuf::from(region.absolute_file.as_deref().unwrap_or(&region.file));
        let data = session.audio().files.get(region.pool_key().as_ref()).cloned();
        let [metadata, name, detected] = tempo_candidates(&path, data.as_deref());
        [(metadata, TempoSource::Metadata), (name, TempoSource::Name), (detected, TempoSource::Detected)]
            .into_iter()
            .find_map(|(bpm, source)| bpm.map(|b| (b, source)))
            .ok_or("no tempo was found in the file's metadata, its name or its sound")?
    } else {
        let own = clip.audio.tempo().filter(|_| clip.audio.warped);
        (own.unwrap_or(session.project.transport.tempo_bpm), clip.audio.tempo_source.unwrap_or(TempoSource::Manual))
    };
    if !(bpm.is_finite() && CLIP_TEMPO_RANGE.contains(&bpm)) {
        return Err(format!("the tempo {bpm} is outside {CLIP_TEMPO_RANGE:?}").into());
    }
    let markers = vec![
        suhina_model::WarpMarker { time: 0.0, audio_time: 0.0 },
        suhina_model::WarpMarker { time: seconds * bpm / 60.0, audio_time: seconds },
    ];
    let before = clip;
    let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
    clip.audio.warped = true;
    clip.audio.warp_markers = markers;
    clip.audio.tempo_source = Some(source);
    follow_audio_end(session, at, &before, &region)?;
    let clip = crate::notes::clip_at(&session.project, at).ok_or("the clip is missing")?;
    Ok(json!({ "clip": clip.id.0, "bpm": bpm, "source": source.name(), "markers": listed(&clip.audio.warp_markers) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_named_with_its_tempo_has_that_tempo_by_name() {
        let [metadata, name, detected] = tempo_candidates(Path::new("does-not-exist/drums_96bpm.wav"), None);
        assert_eq!((metadata, name, detected), (None, Some(96.0), None));
    }

    /// Mono audio at 44.1 kHz: a quiet hiss with a decaying burst of `amplitude` at each `(second,
    /// amplitude)` and, from `swell.0` on, a held tone of `swell.1` that grows by a tenth at
    /// `swell.2`.
    fn sound(seconds: f64, bursts: &[(f64, f32)], swell: (f64, f32, f64)) -> suhina_engine::AudioData {
        let rate = 44_100u32;
        let frames = (seconds * f64::from(rate)) as usize;
        let mut seed = 1u32;
        let samples = (0..frames)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let mut v = (f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5) as f32 * 0.002;
                for (at, amplitude) in bursts {
                    if t >= *at {
                        let age = (t - at) as f32;
                        v += amplitude * (-age * 40.0).exp() * (age * 2.0 * std::f32::consts::PI * 220.0).sin();
                    }
                }
                if t >= swell.0 {
                    let level = if t >= swell.2 { swell.1 * 1.1 } else { swell.1 };
                    v += level * (t as f32 * 2.0 * std::f32::consts::PI * 330.0).sin();
                }
                v
            })
            .collect();
        suhina_engine::AudioData { sample_rate: rate, channels: 1, samples }
    }

    /// Legato notes every 160 ms, each starting as the one before it fades to a dip (as a soft
    /// synth arpeggio does): a marker on each, though the 40 ms before a note are still loud.
    #[test]
    fn notes_that_start_out_of_the_fade_of_the_one_before_get_markers() {
        let rate = 44_100u32;
        let note = 0.16;
        let samples: Vec<f32> = (0..(rate as usize * 2))
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let age = (t % note) / note;
                // Up in 10 ms, held, down over the last third to a fifth of the level.
                let level = if age < 0.06 {
                    0.2 + 0.8 * age / 0.06
                } else if age > 0.66 {
                    1.0 - 0.8 * (age - 0.66) / 0.34
                } else {
                    1.0
                };
                (level * 0.3 * (t * 2.0 * std::f64::consts::PI * 440.0).sin()) as f32
            })
            .collect();
        let found = transient_seconds(&suhina_engine::AudioData { sample_rate: rate, channels: 1, samples });
        let notes = (1..12).map(|k| k as f64 * note);
        let missed: Vec<f64> = notes.filter(|at| !found.iter().any(|s| (s - at).abs() < 0.02)).collect();
        assert!(missed.len() <= 1, "missed {missed:?} of {found:?}");
    }

    #[test]
    fn markers_go_on_clear_attacks_only_and_the_stronger_of_two_close_ones() {
        // Clear attacks at 0.5 and 1.0 s, a weaker one 30 ms after the second, and a held tone
        // from 1.6 s that swells by a tenth (under 1 dB) at 2.2 s.
        let data = sound(3.0, &[(0.5, 0.8), (1.0, 0.8), (1.03, 0.3)], (1.6, 0.4, 2.2));
        let found = transient_seconds(&data);
        let near = |at: f64| found.iter().filter(|s| (*s - at).abs() < 0.002).count();
        assert_eq!((near(0.5), near(1.0)), (1, 1), "{found:?}");
        assert!(found.iter().all(|s| (s - 1.03).abs() > 0.01 && (s - 2.2).abs() > 0.01), "{found:?}");
        assert!(found.windows(2).all(|w| w[1] - w[0] >= MARKER_GAP_SECONDS), "{found:?}");
    }
}
