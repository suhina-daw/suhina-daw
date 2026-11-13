// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 ItsAlphaNeon
// Copyright (C) 2026 thacer13
// Copyright (C) 2026 The Suhina contributors

//! Channel interpretation adapted from Lemon Studio os-flp src/project.rs at
//! 011a03f23d9db7f10f07cd7d165cc978a9dda263 (<https://github.com/ItsAlphaNeon/Lemon-Studio>).
//! Changes: Suhina mapping, bounded inputs, retained source and explicit partial-import warnings.
//! Gain and mixer-EQ parameter laws also adapt os-dsp/src/{level,mixer}.rs at that revision.
//! See docs/spec/flp-model-mapping.md.

use crate::{
    Event, FileKind, Flp, FlpError,
    records::{data, dword, error, note, number, placements, word},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};
use suhina_model::{
    ArrangementLane, AudioRegion, Channel, ChannelId, ChannelRole, Clip, ClipContent, ContentId, ContentKinds,
    ContentLane, Device, DeviceKind, DeviceParam, Extension, Extensions, LaneData, Note, Project, Send, Track, TrackId,
};

const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_OBJECTS: usize = 4096;
const MAX_NOTES: usize = 1_000_000;

fn color(value: u32) -> suhina_model::Rgb {
    suhina_model::Rgb(value as u8, (value >> 8) as u8, (value >> 16) as u8)
}

/// An imported project and its explicit playback limitations.
#[derive(Debug)]
pub struct Import {
    pub project: Project,
    pub warnings: Vec<String>,
}

/// Opens a project for import without writing to it or loading any plug-in.
pub fn import_file(path: &Path) -> Result<Import, FlpError> {
    import_file_with_samples(path, |files| vec![None; files.len()])
}

/// Import using the caller's library candidates for unresolved paths, in the supplied order.
/// Candidates are read-only references; ambiguous results should be returned as `None`.
pub fn import_file_with_samples(
    path: &Path,
    resolve: impl FnOnce(&[String]) -> Vec<Option<PathBuf>>,
) -> Result<Import, FlpError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(MAX_IMPORT_BYTES + 1).read_to_end(&mut bytes)?;
    let mut imported = parse_project(&bytes)?;
    if let Some(folder) = path.parent() {
        let found = relocate_samples(&mut imported, folder, resolve);
        crate::stretch::apply(&crate::read_bytes(&bytes)?, &mut imported, folder, &found)?;
    }
    finish(&mut imported)?;
    Ok(imported)
}

/// Whether the importer may look at a stored absolute path: one on a local disk, decided from
/// the path alone, so a network share or a device path a project names is never touched
/// (`docs/spec/missing-files.md` › Project file boundaries).
fn may_probe(path: &Path) -> bool {
    path.is_absolute()
        && suhina_media::on_local_disk(path)
        && !path.components().any(|c| matches!(c, std::path::Component::ParentDir))
}

/// Points each sample at a file that is there and returns the references it resolved, the only
/// sample paths the rest of the import reads.
fn relocate_samples(
    import: &mut Import,
    folder: &Path,
    library: impl FnOnce(&[String]) -> Vec<Option<PathBuf>>,
) -> BTreeSet<String> {
    let resolve = |file: &str| -> Option<String> {
        let stored = Path::new(file);
        if may_probe(stored) && stored.is_file() {
            return Some(file.to_owned());
        }
        let relative = if stored.is_absolute() { stored.strip_prefix(folder).ok() } else { Some(stored) };
        let name = stored.file_name()?;
        let mut candidates = relative.map(Path::to_path_buf).into_iter().collect::<Vec<_>>();
        candidates.push(name.into());
        for subfolder in ["Samples", "Recorded", "Rendered"] {
            candidates.push(Path::new(subfolder).join(name));
        }
        candidates.into_iter().find_map(|relative| {
            let mut path = folder.to_path_buf();
            for part in relative.components() {
                match part {
                    std::path::Component::Normal(part) => path.push(part),
                    std::path::Component::CurDir => continue,
                    _ => return None,
                }
                if std::fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
                    return None;
                }
            }
            path.is_file().then(|| relative.to_string_lossy().replace('\\', "/"))
        })
    };
    let files: BTreeSet<String> =
        import
            .project
            .channels
            .iter()
            .flat_map(|c| &c.devices)
            .filter_map(|d| d.sample_file().map(str::to_owned))
            .chain(import.project.contents.iter().flat_map(|c| c.lanes.iter()).filter_map(|lane| {
                if let LaneData::Audio(region) = &lane.data { Some(region.file.clone()) } else { None }
            }))
            .collect();
    let mut resolved: BTreeMap<String, String> =
        files.iter().filter_map(|file| resolve(file).map(|path| (file.clone(), path))).collect();
    let missing: Vec<String> = files.into_iter().filter(|f| !resolved.contains_key(f)).collect();
    for (file, candidate) in missing.iter().zip(library(&missing)) {
        if let Some(candidate) = candidate.filter(|p| p.is_absolute() && p.is_file()) {
            resolved.insert(file.clone(), candidate.to_string_lossy().replace('\\', "/"));
        }
    }
    let mut missing = BTreeSet::new();
    for device in import.project.channels.iter_mut().flat_map(|c| &mut c.devices) {
        let Some(file) = device.sample_file().map(str::to_owned) else {
            continue;
        };
        if let Some(local) = resolved.get(&file) {
            let absolute = device.sample_absolute_file().unwrap_or(&file).to_owned();
            device.set_sample_reference(local, if Path::new(local).is_absolute() { local } else { &absolute });
        } else {
            missing.insert(file);
        }
    }
    for content in &mut import.project.contents {
        for lane in content.lanes.iter_mut() {
            let LaneData::Audio(region) = &mut lane.data else {
                continue;
            };
            if let Some(local) = resolved.get(&region.file) {
                if region.absolute_file.is_none() && Path::new(&region.file).is_absolute() {
                    region.absolute_file = Some(region.file.clone());
                }
                if Path::new(local).is_absolute() {
                    region.absolute_file = Some(local.clone());
                }
                region.file = local.clone();
            } else {
                missing.insert(region.file.clone());
            }
        }
    }
    for file in missing {
        import
            .warnings
            .push(format!("Sample not found in the project folder or library: {file}. Relink it to play this source."));
    }
    resolved.into_values().collect()
}

/// Maps a complete FLP into the model, retaining the original bytes as an extension.
pub fn import_bytes(bytes: &[u8]) -> Result<Import, FlpError> {
    let mut imported = parse_project(bytes)?;
    finish(&mut imported)?;
    Ok(imported)
}

fn finish(import: &mut Import) -> Result<(), FlpError> {
    crate::normalize::apply(import)?;
    crate::mixer::apply(import);
    if let Some(source) = import.project.extensions.0.iter().find(|e| e.format == "flp" && e.key == "source") {
        let flp = crate::read_bytes(&source.data)?;
        let selected = selected_arrangement(&flp)?;
        let rows = crate::appearance::rows(&flp, selected)?;
        crate::appearance::apply(import, &rows);
    }
    validate_routes(&import.project)?;
    import.project.assign_ids().map_err(|_| error(0, "project identity limit exceeded"))?;
    import.project.validate().map_err(|errors| error(0, format!("invalid imported project: {errors:?}")))
}

fn parse_project(bytes: &[u8]) -> Result<Import, FlpError> {
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(error(0, "import exceeds the 64 MiB budget"));
    }
    let flp = crate::read_bytes(bytes)?;
    if flp.header.kind != FileKind::Project {
        return Err(error(8, "only project files can be imported"));
    }
    if flp.header.ppq == 0 {
        return Err(error(12, "PPQ must be positive"));
    }
    let version = flp.version().ok_or(FlpError::NoVersion)?;
    let mut parts = version.split('.').filter_map(|p| p.parse::<u32>().ok());
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    if !(10..=26).contains(&major) {
        return Err(error(0, "this importer supports FLP versions 10 through 26; save a copy in a supported version"));
    }
    let ppq = f64::from(flp.header.ppq);
    let mut project = Project::default();
    let mut warnings = vec!["Partial FLP import: unsupported plug-ins, unmapped automation, note expression and advanced sample processing are retained but not played.".into()];
    if major < 12 {
        warnings.push("Legacy step-sequencer events and non-ASCII legacy text are not supported yet.".into());
    }
    if let Some(e) = flp.first(156).or_else(|| flp.first(66)) {
        let tempo = f64::from(number(e)?) / if e.id == 156 { 1000. } else { 1. };
        if !suhina_model::valid_bpm(tempo) {
            return Err(error(e.offset, "invalid project tempo"));
        }
        project.transport.tempo_bpm = tempo;
    }
    // Channel sections never extend into playlist or mixer events, including older files
    // without event 51. Only channel-local state is interpreted as a generator.
    let mut channels = BTreeMap::new();
    let mut start = None;
    for (i, e) in flp.events.iter().enumerate() {
        if matches!(e.id, 64 | 51 | 99 | 233 | 238 | 236 | 149 | 42) {
            if let Some(from) = start.take() {
                add_channel(&flp, &flp.events[from..i], &mut project, &mut channels, &mut warnings, major)?;
            }
            if e.id == 64 {
                start = Some(i);
            }
        }
    }
    if let Some(from) = start {
        add_channel(&flp, &flp.events[from..], &mut project, &mut channels, &mut warnings, major)?;
    }
    add_mixer(&flp, &mut project, &mut warnings)?;
    let mut patterns: BTreeMap<u32, Pattern> = BTreeMap::new();
    let mut current = None;
    let mut total_notes = 0usize;
    let mut expression = false;
    for e in &flp.events {
        if matches!(e.id, 64 | 99 | 233 | 236) {
            current = None;
        }
        if e.id == 65 {
            let id = number(e)?;
            if !patterns.contains_key(&id) && patterns.len() >= MAX_OBJECTS {
                return Err(error(e.offset, "too many patterns"));
            }
            patterns.entry(id).or_default();
            current = Some(id);
        }
        let Some(p) = current.and_then(|id| patterns.get_mut(&id)) else { continue };
        match e.id {
            193 => p.name = e.text(flp.encoding()?)?,
            150 => {
                let value = number(e)?;
                p.color = (value >> 24 == 255).then(|| color(value));
            }
            164 => p.length = f64::from(number(e)?) / ppq,
            224 => {
                let d = data(e)?;
                if !d.len().is_multiple_of(24) {
                    return Err(error(e.offset, "truncated pattern note record"));
                }
                total_notes = total_notes.checked_add(d.len() / 24).ok_or_else(|| error(e.offset, "too many notes"))?;
                if total_notes > MAX_NOTES {
                    return Err(error(e.offset, "too many notes"));
                }
                for r in d.as_chunks::<24>().0 {
                    let n = note(r, ppq, e.offset)?;
                    let mut targets = BTreeSet::new();
                    layer_targets(
                        u32::from(n.channel),
                        n.note.key,
                        &channels,
                        &mut Vec::new(),
                        &mut targets,
                        &mut BTreeSet::new(),
                    )?;
                    total_notes = total_notes
                        .checked_add(targets.len().saturating_sub(1))
                        .ok_or_else(|| error(e.offset, "layer note limit"))?;
                    if total_notes > MAX_NOTES {
                        return Err(error(e.offset, "layer note limit"));
                    }
                    expression |= n.expression;
                    p.length = p.length.max(n.note.time + n.note.duration);
                    for target in targets {
                        p.notes.entry(target).or_default().push(n.note);
                    }
                }
            }
            _ => {}
        }
    }
    if expression {
        warnings.push("Some notes have pitch, slide or pan expression that is not played.".into());
    }
    let pattern_colors: BTreeMap<_, _> = patterns.iter().filter_map(|(&id, p)| p.color.map(|c| (id, c))).collect();
    for (id, p) in patterns {
        let lanes = p
            .notes
            .into_iter()
            .map(|(target, mut notes)| {
                notes.sort_by(|a, b| a.time.total_cmp(&b.time));
                ContentLane { target: Some(target), data: LaneData::Notes(notes.into_iter().collect()) }
            })
            .collect::<Vec<_>>();
        project.contents.push(ClipContent {
            id: ContentId(id),
            name: if p.name.is_empty() { format!("Pattern {id}") } else { p.name },
            length: p.length.max(1.),
            lanes: lanes.into(),
            extensions: Extensions::default(),
            scale: None,
        });
    }
    let first_arrangement = flp.first(99).map(number).transpose()?.unwrap_or(0);
    let selected = selected_arrangement(&flp)?;
    let mut arrangement = first_arrangement;
    let mut disabled_rows = BTreeSet::new();
    for e in &flp.events {
        if e.id == 99 {
            arrangement = number(e)?;
        }
        if e.id == 238 && arrangement == selected {
            let d = data(e)?;
            if d.get(12) == Some(&0) {
                let row = dword(d, 0, e.offset)?
                    .checked_sub(1)
                    .ok_or_else(|| error(e.offset, "invalid playlist track number"))?;
                disabled_rows.insert(row);
            }
        }
    }
    arrangement = first_arrangement;
    let mut rows: BTreeMap<(u16, u8), Vec<Clip>> = BTreeMap::new();
    let content_indices: BTreeMap<_, _> = project.contents.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
    let mut total_clips = 0usize;
    let mut automation_placements = Vec::new();
    for e in &flp.events {
        if e.id == 99 {
            arrangement = number(e)?;
        }
        if e.id != 233 || arrangement != selected {
            continue;
        }
        for mut p in placements(e, ppq, (major, minor) < (12, 9))? {
            p.enabled &= !disabled_rows.contains(&u32::from(p.row));
            total_clips += 1;
            if total_clips > MAX_NOTES {
                return Err(error(e.offset, "too many playlist placements"));
            }
            let (content, kind, offset) = if let Some(id) = p.source.checked_sub(0x5000) {
                (ContentId(u32::from(id)), 0, p.offset)
            } else {
                let source = channels
                    .get(&u32::from(p.source))
                    .ok_or_else(|| error(e.offset, "playlist refers to missing channel"))?;
                if !matches!(source.kind, 4 | 5) {
                    return Err(error(e.offset, "unsupported playlist channel type"));
                }
                let kind = if source.kind == 4 { 1 } else { 2 };
                (
                    ContentId(65_536 + u32::from(p.source)),
                    kind,
                    if kind == 1 { p.offset / 1000. * project.transport.tempo_bpm / 60. } else { p.offset },
                )
            };
            if !content_indices.contains_key(&content) {
                return Err(error(e.offset, "playlist refers to missing content"));
            }
            let mut clip = Clip::new(content, p.start, p.length);
            clip.extensions.0.push(Extension {
                format: "flp".into(),
                key: "playlist-index".into(),
                data: u32::from(p.row).to_le_bytes().to_vec(),
            });
            clip.color = if kind == 0 {
                pattern_colors.get(&content.0).copied()
            } else {
                project.track(TrackId(u32::from(p.source))).and_then(|t| t.color)
            };
            clip.offset = offset;
            clip.enabled = p.enabled && kind != 2;
            if kind == 1 {
                for (duration, tension) in &mut p.fades {
                    if *duration == 0. {
                        *tension = 0.;
                        continue;
                    }
                    if !duration.is_finite() || *duration < 0. || !(-4_294_967_295. ..=4_294_967_295.).contains(tension)
                    {
                        return Err(error(
                            e.offset,
                            format!("invalid audio clip fade: duration={duration}, tension={tension}"),
                        ));
                    }
                    *tension = tension.signum() * tension.abs().ln_1p() / (32. * std::f64::consts::LN_2);
                }
                let beats_per_ms = project.transport.tempo_bpm / 60_000.;
                if (p.fades[0].0 + p.fades[1].0) * beats_per_ms > clip.length + 1e-9 {
                    let warning = "Overlapping or overlong audio fades were fitted to the clip; their original durations remain in the retained source.";
                    if !warnings.iter().any(|w| w == warning) {
                        warnings.push(warning.into());
                    }
                }
                clip.fade_in = (p.fades[0].0 * beats_per_ms).min(clip.length);
                clip.fade_out = (p.fades[1].0 * beats_per_ms).min(clip.length);
                clip.fade_in_curve =
                    suhina_model::FadeCurve { shape: suhina_model::FadeShape::Cosine, curve: p.fades[0].1 };
                clip.fade_out_curve =
                    suhina_model::FadeCurve { shape: suhina_model::FadeShape::Cosine, curve: p.fades[1].1 };
                clip.fit_fades();
                clip.audio.gain = p.gain.min(10f64.powf(suhina_model::CLIP_GAIN_MAX_DB / 20.));
            }
            if kind == 2 {
                automation_placements.push(p);
            } else {
                rows.entry((p.row, kind)).or_default().push(clip);
            }
        }
    }
    if flp.events.iter().filter(|e| e.id == 99).count() > 1 {
        warnings.push(
            "Only the selected arrangement was imported; other arrangements remain in the retained source.".into(),
        );
    }
    let mut row_types: BTreeMap<u16, usize> = BTreeMap::new();
    for (row, _) in rows.keys() {
        *row_types.entry(*row).or_default() += 1;
    }
    let mut groups = BTreeSet::new();
    for ((row, kind), mut clips) in rows {
        clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let group = TrackId(65_536 + u32::from(row) * 4);
        let mixed = row_types.get(&row).is_some_and(|n| *n > 1);
        if mixed && groups.insert(group) {
            let name = format!("Playlist {}", row + 1);
            let mut track = Track::new(group, &name);
            track.channel = Some(ChannelId(group.0));
            let mut channel = Channel::new(ChannelId(group.0), name);
            channel.role = ChannelRole::Submix;
            project.channels.push(channel);
            project.tracks.push(track);
        }
        let row_track = TrackId(group.0 + 1 + u32::from(kind));
        let label = match kind {
            1 => "Audio",
            2 => "Automation (source)",
            _ => "Notes",
        };
        let mut track = Track::new(row_track, format!("Playlist {} — {label}", row + 1));
        track.parent = mixed.then_some(group);
        track.content.notes = kind != 1;
        track.content.audio = kind == 1;
        for clip in &clips {
            if kind != 0
                && let Some(content) = content_indices.get(&clip.content).and_then(|&i| project.contents.get_mut(i))
            {
                content.length = content.length.max(clip.offset + clip.length);
            }
        }
        project.tracks.push(track);
        project.arrangement.lanes.push(ArrangementLane {
            track: Some(row_track),
            name: format!("Playlist {}", row + 1),
            clips,
            ..Default::default()
        });
    }
    crate::automation::apply(&flp, &mut project, &automation_placements, &mut warnings)?;
    project.extensions.0.push(Extension { format: "flp".into(), key: "source".into(), data: bytes.to_vec() });
    validate_routes(&project)?;
    project.validate().map_err(|errors| error(0, format!("invalid imported project: {errors:?}")))?;
    Ok(Import { project, warnings })
}

fn selected_arrangement(flp: &Flp) -> Result<u32, FlpError> {
    let selected = flp.first(100).map(number).transpose()?.unwrap_or(0);
    if flp.events.iter().any(|e| e.id == 99 && number(e).ok() == Some(selected)) {
        Ok(selected)
    } else {
        Ok(flp.first(99).map(number).transpose()?.unwrap_or(0))
    }
}

fn validate_routes(project: &Project) -> Result<(), FlpError> {
    let mut incoming: BTreeMap<_, usize> = project.channels.iter().map(|c| (c.id, 0)).collect();
    let mut edges: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for channel in &project.channels {
        if !(-1. ..=1.).contains(&channel.pan) || !(0. ..=16.).contains(&channel.volume) {
            return Err(error(0, "invalid mixer level or pan"));
        }
        for target in channel.output.into_iter().chain(channel.sends.iter().filter(|s| s.level > 0.).map(|s| s.target))
        {
            let count = incoming.get_mut(&target).ok_or_else(|| error(0, "missing routing destination"))?;
            *count += 1;
            edges.entry(channel.id).or_default().push(target);
        }
    }
    let mut ready = incoming.iter().filter_map(|(&id, &n)| (n == 0).then_some(id)).collect::<Vec<_>>();
    let mut visited = 0;
    while let Some(id) = ready.pop() {
        visited += 1;
        for target in edges.get(&id).into_iter().flatten() {
            if let Some(n) = incoming.get_mut(target) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    ready.push(*target);
                }
            }
        }
    }
    if visited != incoming.len() {
        return Err(error(0, "cyclic mixer routing is not supported"));
    }
    Ok(())
}

#[derive(Default)]
struct Pattern {
    name: String,
    color: Option<suhina_model::Rgb>,
    length: f64,
    notes: BTreeMap<TrackId, Vec<Note>>,
}

struct ChannelSource {
    track: TrackId,
    kind: u32,
    children: Option<Vec<u32>>,
    key_range: (u32, u32),
}

fn layer_targets(
    id: u32,
    key: u8,
    sources: &BTreeMap<u32, ChannelSource>,
    path: &mut Vec<u32>,
    targets: &mut BTreeSet<TrackId>,
    visited: &mut BTreeSet<u32>,
) -> Result<(), FlpError> {
    if path.len() >= 32 || path.contains(&id) {
        return Err(error(0, "cyclic or excessively nested Layer"));
    }
    if !visited.insert(id) {
        return Ok(());
    }
    let source = sources.get(&id).ok_or_else(|| error(0, "missing Layer child or note channel"))?;
    if !path.is_empty() && !(source.key_range.0..=source.key_range.1).contains(&u32::from(key)) {
        return Ok(());
    }
    if let Some(children) = &source.children {
        path.push(id);
        for child in children {
            layer_targets(*child, key, sources, path, targets, visited)?;
        }
        path.pop();
    } else {
        targets.insert(source.track);
    }
    Ok(())
}

fn add_channel(
    flp: &Flp,
    block: &[Event],
    project: &mut Project,
    channels: &mut BTreeMap<u32, ChannelSource>,
    warnings: &mut Vec<String>,
    major: u32,
) -> Result<(), FlpError> {
    let Some(first) = block.first() else { return Ok(()) };
    if channels.len() >= MAX_OBJECTS {
        return Err(error(first.offset, "too many channels"));
    }
    let source_id = number(first)?;
    let track_id = TrackId(source_id);
    if channels.contains_key(&source_id) {
        return Err(error(first.offset, "duplicate channel id"));
    }
    let find = |id| block.iter().find(|e| e.id == id);
    let text = |id| find(id).map(|e| e.text(flp.encoding()?)).transpose();
    let name = text(if major < 12 { 192 } else { 203 })?
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Channel {}", source_id + 1));
    let mut track = Track::new(track_id, &name);
    if find(41).map(number).transpose()?.is_some_and(|v| v != 0) {
        track.color = find(128).map(number).transpose()?.map(color);
    }
    track.channel = Some(ChannelId(source_id));
    track.content = ContentKinds { notes: true, audio: false };
    let mut channel = Channel::new(ChannelId(source_id), &name);
    channel.volume = 10000. / 12800.;
    let mixer = if let Some(e) = find(104) {
        number(e)?
    } else {
        find(22).map(number).transpose()?.map_or(0, |n| (n as i8).max(0) as u32)
    };
    if mixer > 500 {
        return Err(error(first.offset, "invalid mixer assignment"));
    }
    // Resolved after every stored mixer bus has been read.
    channel.output = Some(ChannelId(131_072 + mixer));
    let source_muted = find(0).map(number).transpose()?.is_some_and(|v| v == 0);
    channel.mute = source_muted;
    let mut pitch = 0.;
    if let Some(e) = find(219) {
        let d = data(e)?;
        channel.pan = f64::from(dword(d, 0, e.offset)? as i32) / 6400. - 1.;
        channel.volume = f64::from(dword(d, 4, e.offset)? as i32) / 12800.;
        pitch = f64::from(dword(d, 8, e.offset)? as i32) / 100.;
        if !(-1. ..=1.).contains(&channel.pan) || !(0. ..=16.).contains(&channel.volume) {
            return Err(error(e.offset, "invalid channel levels"));
        }
    }
    let kind = find(21).map(number).transpose()?.unwrap_or(255);
    let settings = find(215).map(data).transpose()?.unwrap_or_default();
    let key_range = if settings.len() >= 76 {
        (dword(settings, 68, first.offset)?, dword(settings, 72, first.offset)?)
    } else {
        (0, 127)
    };
    let adjustment = find(229).map(data).transpose()?.unwrap_or_default();
    let neutral_adjustment = adjustment.is_empty()
        || adjustment.len() >= 20
            && dword(adjustment, 0, 0)? == 0
            && dword(adjustment, 4, 0)? == 12800
            && dword(adjustment, 12, 0)? == 0
            && dword(adjustment, 16, 0)? == 0;
    let neutral = neutral_adjustment
        && channel.volume == 1.
        && channel.pan == 0.
        && pitch == 0.
        && find(144).map(number).transpose()?.unwrap_or(0) == 0
        && find(135).map(number).transpose()?.unwrap_or(60) == 60
        && find(142).map(number).transpose()?.unwrap_or(0) == 0
        && find(89).map(number).transpose()?.unwrap_or(0) == 0
        && (settings.is_empty()
            || settings.len() >= 80
                && [20, 24, 28, 32, 36]
                    .into_iter()
                    .all(|i| dword(settings, i, 0).ok().map(f32::from_bits) == Some(1.))
                && dword(settings, 40, 0)? == 0
                && dword(settings, 64, 0)? == 1447);
    let children = if kind == 3 && neutral {
        Some(if channel.mute {
            Vec::new()
        } else {
            block.iter().filter(|e| e.id == 94).map(number).collect::<Result<BTreeSet<_>, _>>()?.into_iter().collect()
        })
    } else {
        None
    };
    let mapped_layer = children.is_some();
    channels.insert(source_id, ChannelSource { track: track_id, kind, children, key_range });
    let sample = text(196)?.filter(|s| !s.is_empty());
    if matches!(kind, 4 | 5) {
        track.content = ContentKinds { notes: false, audio: kind == 4 };
        let lanes = if kind == 4 {
            let file = sample.unwrap_or_default().replace('\\', "/");
            if file.is_empty() {
                warnings.push(format!("{name}: audio source path is missing."));
            }
            vec![ContentLane { target: Some(track_id), data: LaneData::Audio(AudioRegion::new(file)) }]
        } else {
            Vec::new()
        };
        project.contents.push(ClipContent {
            id: ContentId(65_536 + source_id),
            name: name.clone(),
            length: 1.,
            lanes: lanes.into(),
            extensions: Extensions::default(),
            scale: None,
        });
    } else if mapped_layer {
        // Its notes are expanded into the existing child instruments below.
    } else if kind == 0 {
        let mut device = Device::new("Sampler", DeviceKind::Builtin("sampler".into()));
        if let Some(path) = sample {
            device.set_sample_file(&path.replace('\\', "/"));
        }
        let root = find(135).map(number).transpose()?.unwrap_or(60);
        if root > 127 {
            return Err(error(first.offset, "invalid sampler root key"));
        }
        device.params = vec![
            DeviceParam::new("root", f64::from(root)),
            DeviceParam::new("pitch", pitch),
            DeviceParam::new("playback", 1.),
            DeviceParam::new("attack", 0.),
            DeviceParam::new("decay", 0.),
            DeviceParam::new("sustain", 1.),
        ];
        channel.devices.push(device);
    } else {
        let plugin = text(201)?.filter(|s| !s.is_empty()).unwrap_or_else(|| format!("channel-type-{kind}"));
        let mut device = Device::new(&name, DeviceKind::Foreign { format: "flp".into(), id: plugin });
        device.state = find(213).map(|e| data(e).map(|b| b.to_vec().into())).transpose()?;
        if let Some(group) = crate::patcher::decode(&device, true, project.transport.tempo_bpm, 63) {
            channel.devices.push(group);
        } else if let Some(mut hosted) = crate::wrapper::decode(&device) {
            hosted.mark_external_instrument();
            channel.devices.push(hosted);
        } else {
            device.mark_missing_instrument();
            channel.devices.push(device);
            warnings.push(format!(
                "{name}: instrument identity or state is unsupported; retained as a silent placeholder."
            ));
        }
    }
    if !matches!(kind, 4 | 5) && !mapped_layer {
        if let Some(instrument) = channel.devices.first_mut() {
            instrument.enabled = !source_muted;
        }
        channel.mute = false;
    }
    if kind != 5 {
        let gain = level_gain(channel.volume * 12800.)? / if kind == 4 { level_gain(10000.)? } else { 1. };
        channel.devices.push(source_utility(gain, channel.pan));
        channel.volume = 1.;
        channel.pan = 0.;
    }
    if kind != 5 && !mapped_layer {
        project.tracks.push(track);
        project.channels.push(channel);
    }
    Ok(())
}

fn level_gain(stored: f64) -> Result<f64, FlpError> {
    if !(0. ..=25600.).contains(&stored) {
        return Err(error(0, "level outside supported range"));
    }
    Ok((11f64.powf(stored / 12800.) - 1.) / 10.)
}

fn source_utility(gain: f64, pan: f64) -> Device {
    let angle = (pan + 1.) * std::f64::consts::FRAC_PI_4;
    let left = angle.cos() * std::f64::consts::SQRT_2;
    let right = angle.sin() * std::f64::consts::SQRT_2;
    let peak = left.max(right);
    let balance = if right > left { 1. - left / right } else { right / left - 1. };
    let mut device = Device::new("Source level", DeviceKind::Builtin("utility".into()));
    device.params = vec![
        DeviceParam::new("gain", if gain > 0. { 20. * (gain * peak).log10() } else { -120. }),
        DeviceParam::new("balance", balance),
        DeviceParam::new("mute", if gain == 0. { 1. } else { 0. }),
    ];
    device
}

/// Mixer chains live once, separately from source channels and playlist presentation.
fn add_mixer(flp: &Flp, project: &mut Project, warnings: &mut Vec<String>) -> Result<(), FlpError> {
    let mut starts = Vec::new();
    for (i, e) in flp.events.iter().enumerate() {
        if e.id != 236 {
            continue;
        }
        let mut start = i;
        while start > 0 && flp.events.get(start - 1).is_some_and(|e| matches!(e.id, 149 | 42 | 204 | 95 | 27)) {
            start -= 1;
        }
        starts.push(start);
        if starts.len() > 502 {
            return Err(error(e.offset, "too many mixer buses"));
        }
    }
    if starts.is_empty() {
        for c in &mut project.channels {
            c.output = None;
        }
        warnings.push("No mixer block was present; source channels play into Main.".into());
        return Ok(());
    }
    let params = flp.first(225).map(data).transpose()?.unwrap_or_default();
    if !params.len().is_multiple_of(12) || params.len() / 12 > 131_072 {
        return Err(error(0, "truncated mixer parameter record"));
    }
    let sink_id = ChannelId(132_000);
    let mut sink = Channel::new(sink_id, "Unrouted output");
    sink.volume = 0.;
    sink.role = ChannelRole::Submix;
    // Sends address return tracks. A unity return feeds Main, whose chain is owned by
    // the engine's final mix stage and must not also appear as an ordinary track chain.
    let main_send = ChannelId(132_001);
    let mut main_return = Channel::new(main_send, "Main routing");
    main_return.role = ChannelRole::Effect;
    main_return.output = Some(ChannelId(131_072));
    let mut main_return_track = Track::new(TrackId(main_send.0), "Main routing");
    main_return_track.channel = Some(main_send);
    let old_base =
        params.as_chunks::<12>().0.iter().any(|r| word(r, 6, 0).is_ok_and(|loc| (0x2000..0x4000).contains(&loc)))
            && !params.as_chunks::<12>().0.iter().any(|r| word(r, 6, 0).is_ok_and(|loc| loc >= 0x7000));
    // A final current/monitor bus is stored after the actual mixer inserts.
    let count = if starts.len() > 1 { starts.len() - 1 } else { 1 };
    for c in &project.channels {
        if c.output.is_some_and(|id| id.0 - 131_072 >= count as u32) {
            return Err(error(0, "channel refers to missing mixer bus"));
        }
    }
    for (index, &start) in starts.iter().take(count).enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(flp.events.len());
        let block = flp.events.get(start..end).ok_or_else(|| error(0, "invalid mixer section"))?;
        let find = |id| block.iter().find(|e| e.id == id);
        let name = find(204)
            .map(|e| e.text(flp.encoding()?))
            .transpose()?
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| if index == 0 { "Main".into() } else { format!("Mixer {index}") });
        let id = ChannelId(131_072 + index as u32);
        let mut bus = Channel::new(id, &name);
        bus.role = if index == 0 { ChannelRole::Master } else { ChannelRole::Effect };
        if index != 0 {
            bus.output = Some(sink_id);
        }
        let flags = find(236).map(data).transpose()?.filter(|b| b.len() >= 8).and_then(|b| b.get(4)).copied();
        bus.mute = flags.is_some_and(|f| f & 8 == 0);
        let mut slots = BTreeMap::new();
        let mut pending: Option<Device> = None;
        for e in block {
            match e.id {
                201 => {
                    let name = e.text(flp.encoding()?)?;
                    pending = Some(Device::new(&name, DeviceKind::Foreign { format: "flp".into(), id: name.clone() }));
                }
                213 => {
                    if let Some(d) = pending.as_mut() {
                        d.state = Some(data(e)?.to_vec().into());
                    }
                }
                98 => {
                    let slot = number(e)? & 255;
                    if slot >= 10 {
                        return Err(error(e.offset, "invalid mixer effect slot"));
                    }
                    if let Some(d) = pending.take() {
                        slots.insert(slot, d);
                    }
                }
                _ => {}
            }
        }
        if pending.is_some() {
            warnings.push(format!(
                "{name}: a legacy effect without a slot marker is retained only in the original source."
            ));
        }
        let mut levels = BTreeMap::new();
        let mut eq = [[0., 5777., 17500.], [0., 33145., 17500.], [0., 55825., 17500.]];
        for r in params.as_chunks::<12>().0 {
            let loc = u32::from(word(r, 6, 0)?);
            let base = if old_base { 0x2000 } else { 0x7000 };
            let Some(relative) = loc.checked_sub(base) else {
                continue;
            };
            if relative / 64 != index as u32 {
                continue;
            }
            let slot = relative % 64;
            let pid = r[4];
            let kind = r[5];
            let value = f64::from(dword(r, 8, 0)? as i32);
            if kind == 0x20 || (kind == 0x1f && (64..=190).contains(&pid)) {
                let target = if kind == 0x20 { u32::from(pid) } else { u32::from(pid - 64) };
                levels.insert(target, level_gain(value)?);
            } else if kind == 0x1f {
                match pid {
                    0xc0 => bus.volume = level_gain(value)?,
                    0xc1 => bus.pan = value / 6400.,
                    0xd0..=0xd2 => {
                        if let Some(band) = eq.get_mut(usize::from(pid - 0xd0)) {
                            band[0] = value;
                        }
                    }
                    0xd8..=0xda => {
                        if let Some(band) = eq.get_mut(usize::from(pid - 0xd8)) {
                            band[1] = value;
                        }
                    }
                    0xe0..=0xe2 => {
                        if let Some(band) = eq.get_mut(usize::from(pid - 0xe0)) {
                            band[2] = value;
                        }
                    }
                    0 => {
                        if let Some(d) = slots.get_mut(&slot) {
                            d.enabled = value != 0.;
                        }
                    }
                    1 if value != 12800. => {
                        warnings.push(format!("{name}: slot {} wet/dry mix is retained but not translated.", slot + 1))
                    }
                    _ => {}
                }
            }
        }
        if index != 0
            && let Some(e) = find(235)
        {
            for (destination, &on) in data(e)?.iter().enumerate() {
                if on == 0 {
                    continue;
                }
                if destination >= count {
                    return Err(error(e.offset, "mixer route refers to missing bus"));
                }
                bus.sends.push(Send {
                    target: if destination == 0 { main_send } else { ChannelId(131_072 + destination as u32) },
                    level: levels.get(&(destination as u32)).copied().unwrap_or(1.),
                    pre_fader: false,
                });
            }
        }
        if eq.iter().any(|b| b[0] != 0.) {
            bus.devices.push(mixer_eq(eq)?);
            warnings.push(format!(
                "{name}: mixer EQ uses three EQ bands; gain/frequency/Q outside the device range are limited."
            ));
        }
        let mut remaining_slots = slots.len();
        for (slot, mut device) in slots {
            remaining_slots = remaining_slots.saturating_sub(1);
            // Reserve one placeholder per remaining slot and one final route-level device.
            let budget = 64usize.saturating_sub(suhina_model::walk_devices(&bus.devices).len() + remaining_slots + 1);
            device.extensions.0.push(Extension {
                format: "flp".into(),
                key: "mixer-slot".into(),
                data: slot.to_le_bytes().to_vec(),
            });
            device.enabled &= flags.is_none_or(|f| f & 4 != 0);
            if let Some(group) = crate::patcher::decode(&device, false, project.transport.tempo_bpm, budget) {
                bus.devices.push(group);
            } else if let Some(hosted) = crate::wrapper::decode(&device) {
                bus.devices.push(hosted);
            } else if let Some(substitutes) =
                crate::effects::substitute_with_budget(&device, project.transport.tempo_bpm, budget)
            {
                warnings.push(format!("{name}: {} uses approximate native substitutes; algorithms, EQ widths and advanced modulation differ.", device.name));
                bus.devices.extend(substitutes);
            } else {
                warnings.push(format!("{name}: {} is retained as an effect placeholder.", device.name));
                bus.devices.push(device);
            }
        }
        if let [route] = bus.sends.as_slice() {
            let target = route.target;
            let level = route.level;
            bus.output = Some(if target == main_send { ChannelId(131_072) } else { target });
            bus.sends.clear();
            if level != 1. {
                let mut gain = source_utility(level, 0.);
                gain.name = "Output level".into();
                bus.devices.push(gain);
            }
        }
        // With Main still connected at unity, the other connections are parallel sends.
        // Keep Main as the ordinary output instead of a synthetic return destination.
        if bus.sends.len() > 1
            && let Some(at) = bus.sends.iter().position(|s| s.target == main_send && s.level == 1.)
        {
            bus.sends.remove(at);
            bus.output = Some(ChannelId(131_072));
        }
        let mut track = Track::new(TrackId(id.0), name);
        track.channel = Some(id);
        if find(42).map(number).transpose()?.is_some_and(|flag| flag != 0) {
            track.color = find(149).map(number).transpose()?.map(color);
        }
        if index != 0 {
            project.tracks.push(track);
        }
        project.channels.push(bus);
    }
    let returns: BTreeSet<_> = project.channels.iter().flat_map(|c| c.sends.iter().map(|s| s.target)).collect();
    for channel in &mut project.channels {
        if channel.role == ChannelRole::Effect && !returns.contains(&channel.id) {
            channel.role = ChannelRole::Submix;
        }
    }
    project.tracks.push(main_return_track);
    project.channels.push(main_return);
    project.channels.push(sink);
    Ok(())
}

fn mixer_eq(bands: [[f64; 3]; 3]) -> Result<Device, FlpError> {
    let mut device = Device::new("Mixer EQ", DeviceKind::Builtin("eq".into()));
    for i in 0..8 {
        let band = bands.get(i);
        device
            .params
            .push(DeviceParam::new(format!("a{}On", i + 1), if band.is_some_and(|b| b[0] != 0.) { 1. } else { 0. }));
        let Some(&[gain, frequency, width]) = band else {
            continue;
        };
        if !(-1800. ..=1800.).contains(&gain)
            || !(0. ..=65536.).contains(&frequency)
            || !(0. ..=65536.).contains(&width)
        {
            return Err(error(0, "invalid mixer EQ parameter"));
        }
        let db = gain / 100.;
        let hz = (101f64.powf(frequency / 65536.) - 1.) * 159.9 + 10.;
        let q = if i == 1 {
            (1001f64.powf(width / 65536.) - 1.) * 0.0999 + 0.1
        } else {
            let amplitude = 10f64.powf(db / 40.);
            let slope = width / 65536. * 1.9 + 0.1;
            let term = (amplitude + 1. / amplitude) * (1. / slope - 1.) + 2.;
            if term <= 0. { 18. } else { 1. / term.sqrt() }
        };
        for (suffix, value) in [
            (
                "Type",
                if i == 0 {
                    2.
                } else if i == 1 {
                    3.
                } else {
                    5.
                },
            ),
            ("Gain", db.clamp(-15., 15.)),
            ("Frequency", hz.clamp(10., 22000.)),
            ("Q", q.clamp(0.1, 18.)),
        ] {
            device.params.push(DeviceParam::new(format!("a{}{suffix}", i + 1), value));
        }
    }
    Ok(device)
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn only_local_disk_paths_a_project_names_are_looked_at() {
        let local = if cfg!(windows) { "C:\\Samples\\kick.wav" } else { "/samples/kick.wav" };
        assert!(may_probe(Path::new(local)));
        let stepping = if cfg!(windows) { "C:\\Samples\\..\\kick.wav" } else { "/samples/../kick.wav" };
        assert!(!may_probe(Path::new(stepping)));
        assert!(!may_probe(Path::new("Samples/kick.wav")), "relative paths resolve inside the folder");
        if cfg!(windows) {
            for path in ["\\\\host\\share\\kick.wav", "\\\\?\\UNC\\host\\share\\kick.wav", "\\\\.\\COM1", "\\\\.\\pipe\\x"] {
                assert!(!may_probe(Path::new(path)), "{path}");
            }
        }
    }
}

#[cfg(test)]
mod layer_tests {
    use super::*;
    #[test]
    fn repeated_layer_subgraphs_are_visited_once() {
        let mut sources = BTreeMap::new();
        for id in 0..30 {
            sources.insert(
                id,
                ChannelSource {
                    track: TrackId(id),
                    kind: 3,
                    children: Some(vec![id + 1, id + 2]),
                    key_range: (0, 127),
                },
            );
        }
        for id in 30..32 {
            sources.insert(id, ChannelSource { track: TrackId(id), kind: 2, children: None, key_range: (0, 127) });
        }
        let mut visited = BTreeSet::new();
        let mut targets = BTreeSet::new();
        layer_targets(0, 60, &sources, &mut Vec::new(), &mut targets, &mut visited).unwrap();
        assert_eq!(visited.len(), 32);
        assert_eq!(targets, BTreeSet::from([TrackId(30), TrackId(31)]));
    }
}
