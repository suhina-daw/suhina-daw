// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stems files (`.stem.mp4`, `.stem.m4a`): an MP4 file whose first audio track is the finished
//! mix (the master) and whose further audio tracks are its parts (stems), with names, colours
//! and mastering settings in a JSON document in `moov/udta/stem`. The format is described in
//! `docs/spec/stems-format.md`.
//!
//! Audio is decoded with symphonia; the MP4 boxes symphonia does not read (the edit lists that
//! trim an AAC encoder's priming and padding, and the stem metadata) are read here.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;
use suhina_engine::AudioData;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::{decode_limit, reserve_audio};

/// Most audio tracks a Stems file may have (the master and up to 15 stems).
pub const MAX_TRACKS: usize = 16;
/// Largest `moov` box read into memory.
const MAX_MOOV: u64 = 64 * 1024 * 1024;
/// Largest stem metadata document.
const MAX_JSON: usize = 1024 * 1024;
/// Deepest box nesting followed.
const MAX_DEPTH: usize = 8;
/// Most boxes looked at in one file, so a file of tiny boxes cannot keep us busy.
const MAX_BOXES: usize = 100_000;

/// Whether a file is a Stems file by its name: `.stem.mp4` or `.stem.m4a`, in any case.
pub fn is_stems_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let name = name.to_ascii_lowercase();
    name.ends_with(".stem.mp4") || name.ends_with(".stem.m4a")
}

/// One stem's name and colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Stem {
    pub name: String,
    /// `#RRGGBB` in the file.
    pub color: Option<[u8; 3]>,
}

/// The compressor settings of a Stems file, in the file's own units. `None` fields were missing
/// or not numbers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CompressorSettings {
    pub enabled: bool,
    pub ratio: Option<f64>,
    /// Decibels.
    pub threshold: Option<f64>,
    /// Seconds.
    pub attack: Option<f64>,
    /// Seconds.
    pub release: Option<f64>,
    /// 0..1, 0.5 being unity.
    pub input_gain: Option<f64>,
    /// 0..1, 0.5 being unity.
    pub output_gain: Option<f64>,
    /// Hertz, a high-pass on the level detector.
    pub hp_cutoff: Option<f64>,
    /// Percent.
    pub dry_wet: Option<f64>,
}

/// The limiter settings of a Stems file, in the file's own units.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LimiterSettings {
    pub enabled: bool,
    /// Decibels.
    pub threshold: Option<f64>,
    /// Decibels.
    pub ceiling: Option<f64>,
    /// Seconds.
    pub release: Option<f64>,
}

/// What a Stems file holds, read from its boxes without decoding audio.
#[derive(Debug, Clone, PartialEq)]
pub struct StemsInfo {
    /// Audio tracks in the file, the master first.
    pub tracks: usize,
    /// One per stem (`tracks - 1`), in track order. Names and colours come from the metadata;
    /// stems it does not name are called `"Stem <n>"`.
    pub stems: Vec<Stem>,
    /// Whether the file has stem metadata at all.
    pub has_metadata: bool,
    pub compressor: Option<CompressorSettings>,
    pub limiter: Option<LimiterSettings>,
}

/// Where one audio track's sound starts and how long it is, from its edit list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Edit {
    /// Frames to skip at the start (encoder priming), in the track's own time scale.
    skip: u64,
    /// Length in seconds, `None` to the end.
    seconds: Option<f64>,
    /// The track's time scale (`mdhd`).
    timescale: u32,
}

/// An audio track as the boxes describe it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TrackBoxes {
    id: u32,
    edit: Option<Edit>,
}

#[derive(Debug, Default)]
struct Boxes {
    audio: Vec<TrackBoxes>,
    json: Option<Vec<u8>>,
}

/// Reads a Stems file's track layout and metadata.
pub fn read_info(path: &Path) -> Result<StemsInfo, String> {
    let boxes = read_boxes(path)?;
    info_from(&boxes)
}

fn info_from(boxes: &Boxes) -> Result<StemsInfo, String> {
    let tracks = boxes.audio.len();
    if tracks < 2 {
        return Err(format!("a Stems file needs a master and at least one stem; this one has {tracks} audio track(s)"));
    }
    if tracks > MAX_TRACKS {
        return Err(format!("more than {MAX_TRACKS} audio tracks"));
    }
    let meta = boxes.json.as_deref().and_then(parse_json);
    let mut stems = Vec::with_capacity(tracks - 1);
    let listed = meta.as_ref().and_then(|m| m.get("stems")).and_then(Value::as_array);
    for n in 1..tracks {
        let entry = listed.and_then(|l| l.get(n - 1));
        let name = entry
            .and_then(|e| e.get("name"))
            .and_then(Value::as_str)
            .map(clean_name)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("Stem {n}"));
        let color = entry.and_then(|e| e.get("color")).and_then(Value::as_str).and_then(parse_color);
        stems.push(Stem { name, color });
    }
    let dsp = meta.as_ref().and_then(|m| m.get("mastering_dsp"));
    let compressor = dsp.and_then(|d| d.get("compressor")).filter(|c| c.is_object()).map(|c| CompressorSettings {
        enabled: c.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        ratio: number(c, "ratio"),
        threshold: number(c, "threshold"),
        attack: number(c, "attack"),
        release: number(c, "release"),
        input_gain: number(c, "input_gain"),
        output_gain: number(c, "output_gain"),
        hp_cutoff: number(c, "hp_cutoff"),
        dry_wet: number(c, "dry_wet"),
    });
    let limiter = dsp.and_then(|d| d.get("limiter")).filter(|l| l.is_object()).map(|l| LimiterSettings {
        enabled: l.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        threshold: number(l, "threshold"),
        ceiling: number(l, "ceiling"),
        release: number(l, "release"),
    });
    Ok(StemsInfo { tracks, stems, has_metadata: meta.is_some(), compressor, limiter })
}

fn number(object: &Value, key: &str) -> Option<f64> {
    object.get(key).and_then(Value::as_f64).filter(|v| v.is_finite())
}

/// The metadata document: JSON, or (as some tools write it) JSON in base64.
fn parse_json(bytes: &[u8]) -> Option<Value> {
    if bytes.len() > MAX_JSON {
        return None;
    }
    let trimmed = trim_nul(bytes);
    if let Ok(value @ Value::Object(_)) = serde_json::from_slice::<Value>(trimmed) {
        return Some(value);
    }
    use base64::Engine as _;
    let decoded = base64::engine::general_purpose::STANDARD.decode(trimmed).ok()?;
    match serde_json::from_slice::<Value>(trim_nul(&decoded)) {
        Ok(value @ Value::Object(_)) => Some(value),
        _ => None,
    }
}

fn trim_nul(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().rposition(|&b| b != 0 && !b.is_ascii_whitespace()).map_or(0, |i| i + 1);
    let start = bytes.iter().position(|&b| !b.is_ascii_whitespace()).unwrap_or(end).min(end);
    bytes.get(start..end).unwrap_or_default()
}

/// A stem name made safe to show: control characters dropped, at most 64 characters.
fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(64).collect::<String>().trim().to_string()
}

fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// A built-in effect or instrument and its parameter values (plain values, ADR 0004), for the group the
/// stems play into.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSetting {
    /// The id of the built-in effect or instrument.
    pub id: &'static str,
    /// Whether the file has it switched on; a device that is off is added but disabled.
    pub enabled: bool,
    pub params: Vec<(&'static str, f64)>,
}

/// The file's mastering settings as Suhina's compressor and limiter, in that order; settings
/// the file does not have are left out. Missing values take the devices' defaults.
/// `docs/spec/stems-format.md` › Mastering gives the mapping.
pub fn mastering_devices(info: &StemsInfo) -> Vec<DeviceSetting> {
    let mut out = Vec::new();
    if let Some(c) = info.compressor {
        let mut params = Vec::new();
        let mut put = |id: &'static str, value: Option<f64>| {
            if let Some(v) = value.filter(|v| v.is_finite()) {
                params.push((id, v));
            }
        };
        put("input", c.input_gain.map(gain_db));
        put("threshold", c.threshold.map(|v| v.clamp(-60.0, 0.0)));
        put("ratio", c.ratio.map(|v| v.clamp(1.0, 20.0)));
        put("attack", c.attack.map(|v| v.clamp(0.0001, 0.1)));
        put("release", c.release.map(|v| v.clamp(0.01, 2.0)));
        put("sidechainHighPass", c.hp_cutoff.map(|v| v.clamp(20.0, 1000.0)));
        put("output", c.output_gain.map(gain_db));
        // Percent in the file; a value of at most 1 is taken as a fraction already.
        put("mix", c.dry_wet.map(|v| if v > 1.0 { v / 100.0 } else { v }.clamp(0.0, 1.0)));
        out.push(DeviceSetting { id: "compressor", enabled: c.enabled, params });
    }
    if let Some(l) = info.limiter {
        let mut params = Vec::new();
        if let Some(v) = l.threshold.filter(|v| v.is_finite()) {
            params.push(("threshold", v.clamp(-30.0, 0.0)));
        }
        if let Some(v) = l.ceiling.filter(|v| v.is_finite()) {
            params.push(("ceiling", v.clamp(-12.0, 0.0)));
        }
        if let Some(v) = l.release.filter(|v| v.is_finite()) {
            params.push(("release", v.clamp(0.001, 1.0)));
        }
        out.push(DeviceSetting { id: "limiter", enabled: l.enabled, params });
    }
    out
}

/// A compressor gain of the file in decibels: 0..1 with 0.5 at unity spans -12..+12 dB; a value
/// outside 0..1 is taken as decibels already.
fn gain_db(value: f64) -> f64 {
    let db = if (0.0..=1.0).contains(&value) { (value - 0.5) * 24.0 } else { value };
    db.clamp(-24.0, 24.0)
}

/// Decodes every audio track of a Stems file in one pass, the master first, each trimmed by its
/// edit list. All tracks must share a sample rate (a Stems file's do).
pub fn decode_streams(path: &Path) -> Result<Vec<AudioData>, String> {
    let boxes = read_boxes(path)?;
    let info = info_from(&boxes)?;

    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp4");
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("unsupported or unreadable format: {e}"))?;

    struct Out {
        id: u32,
        decoder: Box<dyn symphonia::core::codecs::audio::AudioDecoder>,
        sample_rate: u32,
        channels: u16,
        samples: Vec<f32>,
        max_samples: usize,
    }
    let mut outs: Vec<Out> = Vec::with_capacity(info.tracks);
    for track in &boxes.audio {
        let t = format.tracks().iter().find(|t| t.id == track.id).ok_or("an audio track symphonia cannot see")?;
        let params = t.codec_params.as_ref().and_then(|p| p.audio()).ok_or("no audio codec parameters")?.clone();
        let sample_rate = params.sample_rate.ok_or("unknown sample rate")?;
        let channels = params
            .channels
            .as_ref()
            .map(|c| c.count())
            .filter(|c| (1..=32).contains(c))
            .ok_or("unknown channel count")?;
        let channels = u16::try_from(channels).map_err(|_| "too many channels")?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("unsupported codec: {e}"))?;
        let max_samples = decode_limit(sample_rate, usize::from(channels))?;
        if t.num_frames
            .is_some_and(|frames| frames.checked_mul(u64::from(channels)).is_none_or(|n| n > max_samples as u64))
        {
            return Err("Stems header exceeds the duration or 1 GiB memory budget".into());
        }
        outs.push(Out { id: track.id, decoder, sample_rate, channels, samples: Vec::new(), max_samples });
    }
    if outs.iter().any(|o| o.sample_rate != outs.first().map_or(0, |f| f.sample_rate)) {
        return Err("the tracks have different sample rates".into());
    }

    let mut packet_samples = Vec::new();
    let mut total_samples = 0usize;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) | Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("read error: {e}")),
        };
        let capacity_left =
            crate::MAX_DECODED_SAMPLES.saturating_sub(outs.iter().map(|out| out.samples.capacity()).sum());
        let Some(out) = outs.iter_mut().find(|o| o.id == packet.track_id) else { continue };
        match out.decoder.decode(&packet) {
            Ok(decoded) => {
                let count = decoded.samples_interleaved();
                total_samples = total_samples
                    .checked_add(count)
                    .filter(|n| *n <= crate::MAX_DECODED_SAMPLES)
                    .ok_or("Stems audio exceeds the 1 GiB memory budget")?;
                let limit = out.max_samples.min(out.samples.capacity().saturating_add(capacity_left));
                reserve_audio(&mut out.samples, count, limit)?;
                packet_samples.clear();
                reserve_audio(&mut packet_samples, count, out.max_samples)?;
                decoded.copy_to_vec_interleaved(&mut packet_samples);
                out.samples.extend_from_slice(&packet_samples);
            }
            // A damaged packet is skipped, as players do.
            Err(SymphoniaError::DecodeError(_) | SymphoniaError::IoError(_)) => continue,
            Err(e) => return Err(format!("decode error: {e}")),
        }
    }

    let mut result = Vec::with_capacity(outs.len());
    for (out, track) in outs.into_iter().zip(&boxes.audio) {
        let mut samples = out.samples;
        if let Some(edit) = track.edit {
            trim(&mut samples, out.sample_rate, out.channels, edit);
        }
        if samples.is_empty() {
            return Err(format!("no audio decoded in track {}", track.id));
        }
        result.push(AudioData { sample_rate: out.sample_rate, channels: out.channels, samples });
    }
    Ok(result)
}

/// Applies an edit list entry: drops the priming at the start and the padding after the length.
fn trim(samples: &mut Vec<f32>, sample_rate: u32, channels: u16, edit: Edit) {
    let channels = usize::from(channels.max(1));
    let frames = samples.len() / channels;
    let skip = if edit.timescale == 0 {
        0
    } else {
        let at = edit.skip as f64 * f64::from(sample_rate) / f64::from(edit.timescale);
        (at.round() as usize).min(frames)
    };
    let keep = edit
        .seconds
        .filter(|s| s.is_finite() && *s > 0.0)
        .map_or(frames - skip, |s| ((s * f64::from(sample_rate)).round() as usize).min(frames - skip));
    samples.truncate((skip + keep) * channels);
    samples.drain(..skip * channels);
}

// ---- MP4 boxes ----

/// Reads the `moov` box and finds the audio tracks (with their edit lists) and the stem JSON.
fn read_boxes(path: &Path) -> Result<Boxes, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let mut at = 0u64;
    for _ in 0..MAX_BOXES {
        if at.saturating_add(8) > length {
            break;
        }
        file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        let mut head = [0u8; 16];
        file.read_exact(&mut head[..8]).map_err(|e| e.to_string())?;
        let (size, kind) = (u32_at(&head, 0).unwrap_or(0), [head[4], head[5], head[6], head[7]]);
        let (size, header) = match size {
            0 => (length - at, 8),
            1 => {
                file.read_exact(&mut head[8..16]).map_err(|e| e.to_string())?;
                (u64_at(&head, 8).unwrap_or(0), 16)
            }
            n => (u64::from(n), 8),
        };
        if size < header || at.saturating_add(size) > length {
            return Err("damaged MP4 file: a box runs past the end".into());
        }
        if &kind == b"moov" {
            let body_len = size - header;
            if body_len > MAX_MOOV {
                return Err("MP4 header too large".into());
            }
            let mut body = vec![0u8; usize::try_from(body_len).map_err(|_| "MP4 header too large")?];
            file.read_exact(&mut body).map_err(|e| e.to_string())?;
            return parse_moov(&body);
        }
        at += size;
    }
    Err("not an MP4 file (no moov box)".into())
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(u32::from_be_bytes)
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    bytes.get(at..at + 8).and_then(|b| b.try_into().ok()).map(u64::from_be_bytes)
}

/// The child boxes of `body`: (type, contents). Stops at the first damaged box.
fn children(body: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= body.len() && out.len() < MAX_BOXES {
        let Some(size) = u32_at(body, at) else { break };
        let Some(kind) = body.get(at + 4..at + 8).and_then(|k| <[u8; 4]>::try_from(k).ok()) else { break };
        let (size, header) = match size {
            0 => (body.len() - at, 8),
            1 => match u64_at(body, at + 8).and_then(|s| usize::try_from(s).ok()) {
                Some(s) => (s, 16),
                None => break,
            },
            n => (n as usize, 8),
        };
        let Some(end) = at.checked_add(size) else { break };
        if size < header || end > body.len() {
            break;
        }
        if let Some(contents) = body.get(at + header..end) {
            out.push((kind, contents));
        }
        at = end;
    }
    out
}

fn child<'a>(body: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    children(body).into_iter().find(|(k, _)| k == kind).map(|(_, c)| c)
}

/// Follows a path of box types down from `body`.
fn descend<'a>(body: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
    path.iter().take(MAX_DEPTH).try_fold(body, |b, kind| child(b, kind))
}

fn parse_moov(moov: &[u8]) -> Result<Boxes, String> {
    let movie_timescale = child(moov, b"mvhd").and_then(|m| match m.first()? {
        0 => u32_at(m, 12),
        _ => u32_at(m, 20),
    });
    let mut boxes = Boxes::default();
    for (kind, trak) in children(moov) {
        if &kind != b"trak" {
            continue;
        }
        let is_audio = descend(trak, &[b"mdia", b"hdlr"]).and_then(|h| h.get(8..12)) == Some(b"soun".as_slice());
        if !is_audio {
            continue;
        }
        let Some(id) = child(trak, b"tkhd").and_then(|t| match t.first()? {
            0 => u32_at(t, 12),
            _ => u32_at(t, 20),
        }) else {
            continue;
        };
        let timescale = descend(trak, &[b"mdia", b"mdhd"]).and_then(|m| match m.first()? {
            0 => u32_at(m, 12),
            _ => u32_at(m, 20),
        });
        let edit = descend(trak, &[b"edts", b"elst"])
            .and_then(|e| first_edit(e, movie_timescale?, timescale?))
            .filter(|e| e.timescale > 0);
        boxes.audio.push(TrackBoxes { id, edit });
        if boxes.audio.len() > MAX_TRACKS {
            break;
        }
    }
    boxes.json = descend(moov, &[b"udta", b"stem"]).map(<[u8]>::to_vec);
    Ok(boxes)
}

/// The first edit that plays media (an empty edit, a delay, is skipped).
fn first_edit(elst: &[u8], movie_timescale: u32, timescale: u32) -> Option<Edit> {
    let version = *elst.first()?;
    let count = u32_at(elst, 4)? as usize;
    let entry_size = if version == 1 { 20 } else { 12 };
    for i in 0..count.min(1024) {
        let at = 8 + i * entry_size;
        let (duration, media_time) = if version == 1 {
            (u64_at(elst, at)?, u64_at(elst, at + 8)? as i64)
        } else {
            (u64::from(u32_at(elst, at)?), i64::from(u32_at(elst, at + 4)? as i32))
        };
        if media_time < 0 {
            continue;
        }
        let seconds = (duration > 0 && movie_timescale > 0).then(|| duration as f64 / f64::from(movie_timescale));
        return Some(Edit { skip: media_time as u64, seconds, timescale });
    }
    None
}

#[cfg(test)]
#[path = "stems_tests.rs"]
mod tests;
