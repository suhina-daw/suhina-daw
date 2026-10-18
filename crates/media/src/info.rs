// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a file in the browser is, and what can be told about a sample without decoding it: its
//! length, rate and channels, and its tempo and key from the file's metadata or its name
//! (`docs/spec/browser.md` › Tempo and key of a file).

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, RawValue, StandardTag, Tag};

/// The kinds of files the browser lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileKind {
    Sample,
    /// A Stems file: a master and its stems in one MP4 (`docs/spec/stems-format.md`).
    Stems,
    Midi,
    Preset,
    Project,
    Effect,
}

impl FileKind {
    /// The name it is shown and saved as.
    pub fn name(self) -> &'static str {
        match self {
            Self::Sample => "sample",
            Self::Stems => "stems",
            Self::Midi => "midi",
            Self::Preset => "preset",
            Self::Project => "project",
            Self::Effect => "effect",
        }
    }

    /// The kind [`FileKind::name`] gives `text`.
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Sample, Self::Stems, Self::Midi, Self::Preset, Self::Project, Self::Effect]
            .into_iter()
            .find(|k| k.name() == text)
    }

    /// Whether it is audio that becomes audio clips (a sample, or a Stems file).
    pub fn is_audio(self) -> bool {
        matches!(self, Self::Sample | Self::Stems)
    }
}

/// The kind of the file at `path`, by its extension; `None` for files the browser does not list.
pub fn kind_of(path: &Path) -> Option<FileKind> {
    if crate::stems::is_stems_path(path) {
        return Some(FileKind::Stems);
    }
    if suhina_project_file::is_project_file(path) {
        return Some(FileKind::Project);
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "wav" | "aif" | "aiff" | "flac" | "mp3" | "ogg" | "oga" => Some(FileKind::Sample),
        "mid" | "midi" => Some(FileKind::Midi),
        "syx" => Some(FileKind::Preset),
        "als" => Some(FileKind::Project),
        "sd" => Some(FileKind::Effect),
        _ => None,
    }
}

/// Where a tempo came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempoSource {
    /// The file's own metadata.
    Metadata,
    /// The file's name ("loop_128bpm").
    Name,
    /// Detected from the sound ([`crate::tempo::detect`]).
    Detected,
}

impl TempoSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Name => "name",
            Self::Detected => "detected",
        }
    }
}

/// A tempo in beats per minute and where it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
    pub source: TempoSource,
}

/// The lowest and highest tempo taken from metadata or a name.
pub const TEMPO_RANGE: std::ops::RangeInclusive<f64> = 40.0..=300.0;

/// What is known about a sample before decoding it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SampleInfo {
    pub duration_seconds: Option<f64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub tempo: Option<Tempo>,
    /// The key as `C`, `F#m`, `Bb` (a trailing `m` is minor).
    pub key: Option<String>,
}

/// Reads a sample's header and metadata (not its audio). The tempo and key come from the
/// metadata first, then the file name.
pub fn sample_info(path: &Path) -> Result<SampleInfo, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("unsupported or unreadable format: {e}"))?;
    let mut info = SampleInfo::default();
    if let Some(track) = format.default_track(TrackType::Audio) {
        let params = track.codec_params.as_ref().and_then(|p| p.audio());
        info.sample_rate = params.and_then(|p| p.sample_rate).filter(|r| (1..=1_000_000).contains(r));
        info.channels = params
            .and_then(|p| p.channels.as_ref())
            .and_then(|c| u16::try_from(c.count()).ok())
            .filter(|c| (1..=64).contains(c));
        if let (Some(frames), Some(rate)) = (track.num_frames, info.sample_rate) {
            let seconds = frames as f64 / f64::from(rate);
            info.duration_seconds = Some(seconds).filter(|s| s.is_finite());
        }
    }
    let mut tags: Vec<Tag> = Vec::new();
    if let Some(revision) = format.metadata().skip_to_latest() {
        tags.extend(revision.media.tags.iter().cloned());
        for track in &revision.per_track {
            tags.extend(track.metadata.tags.iter().cloned());
        }
    }
    let (tempo, key) = tempo_and_key_from_tags(&tags);
    info.tempo = tempo.map(|bpm| Tempo { bpm, source: TempoSource::Metadata });
    info.key = key;
    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        && let Ok(Some(acid)) = read_acid(path)
    {
        if info.tempo.is_none() && !acid.one_shot {
            info.tempo = acid.tempo.map(|bpm| Tempo { bpm, source: TempoSource::Metadata });
        }
        if info.key.is_none() {
            info.key = acid.root.map(|n| note_name(n).to_string());
        }
    }
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if info.tempo.is_none() {
        info.tempo = tempo_from_name(&name).map(|bpm| Tempo { bpm, source: TempoSource::Name });
    }
    if info.key.is_none() {
        info.key = key_from_name(&name);
    }
    Ok(info)
}

fn tempo_and_key_from_tags(tags: &[Tag]) -> (Option<f64>, Option<String>) {
    let mut tempo = None;
    let mut key = None;
    for tag in tags {
        match &tag.std {
            Some(StandardTag::Bpm(bpm)) if tempo.is_none() => tempo = Some(*bpm as f64),
            Some(StandardTag::InitialKey(text)) if key.is_none() => key = normalise_key(text),
            _ => {}
        }
        let raw = tag.raw.key.to_ascii_uppercase();
        if tempo.is_none() && matches!(raw.as_str(), "BPM" | "TEMPO" | "TBPM") {
            tempo = match &tag.raw.value {
                RawValue::Float(f) => Some(*f),
                RawValue::UnsignedInt(n) => Some(*n as f64),
                RawValue::SignedInt(n) => Some(*n as f64),
                RawValue::String(s) => s.trim().parse::<f64>().ok(),
                _ => None,
            };
        }
        if key.is_none()
            && matches!(raw.as_str(), "KEY" | "INITIALKEY" | "TKEY")
            && let RawValue::String(s) = &tag.raw.value
        {
            key = normalise_key(s);
        }
    }
    (tempo.filter(|t| TEMPO_RANGE.contains(t)), key)
}

/// What a WAV file's `acid` chunk says.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Acid {
    one_shot: bool,
    root: Option<u8>,
    tempo: Option<f64>,
}

/// Chunks looked at before giving up on finding `acid`.
const MAX_CHUNKS: usize = 64;

/// Finds and reads the `acid` chunk of a WAV file: flags (bit 0 a one-shot, bit 1 the root note
/// is set), the root note as a MIDI key, and the tempo as a 32-bit float at byte 20.
fn read_acid(path: &Path) -> std::io::Result<Option<Acid>> {
    let mut file = std::fs::File::open(path)?;
    let length = file.metadata()?.len();
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Ok(None);
    }
    let mut position: u64 = 12;
    for _ in 0..MAX_CHUNKS {
        if position.saturating_add(8) > length {
            break;
        }
        file.seek(SeekFrom::Start(position))?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk)?;
        let size = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
        if &chunk[0..4] == b"acid" {
            if size < 24 {
                return Ok(None);
            }
            let mut body = [0u8; 24];
            file.read_exact(&mut body)?;
            let flags = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            let root = u16::from_le_bytes([body[4], body[5]]);
            let tempo = f32::from_le_bytes([body[20], body[21], body[22], body[23]]);
            return Ok(Some(Acid {
                one_shot: flags & 1 != 0,
                root: (flags & 2 != 0).then(|| u8::try_from(root % 128).unwrap_or(0)),
                tempo: Some(f64::from(tempo)).filter(|t| TEMPO_RANGE.contains(t)),
            }));
        }
        // Chunks are padded to an even length.
        position = position.saturating_add(8).saturating_add(size).saturating_add(size & 1);
    }
    Ok(None)
}

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

fn note_name(key: u8) -> &'static str {
    NOTE_NAMES.get(usize::from(key % 12)).copied().unwrap_or("C")
}

/// A tempo written in a name next to "bpm": `kick_128bpm`, `BPM 90 loop`, `120 BPM`.
pub fn tempo_from_name(name: &str) -> Option<f64> {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut search = 0;
    while let Some(found) = lower.get(search..).and_then(|rest| rest.find("bpm")) {
        let at = search + found;
        // The number just before ("128bpm", "128 bpm", "128_bpm") or just after ("bpm128", "bpm 128").
        let before = number_ending_at(bytes, at);
        let after = number_starting_at(bytes, at + 3);
        if let Some(bpm) = before.or(after).filter(|t| TEMPO_RANGE.contains(t)) {
            return Some(bpm);
        }
        search = at + 3;
    }
    None
}

fn is_gap(b: u8) -> bool {
    matches!(b, b' ' | b'_' | b'-' | b'.')
}

fn number_ending_at(bytes: &[u8], end: usize) -> Option<f64> {
    let mut end = end;
    while end > 0 && bytes.get(end - 1).copied().is_some_and(is_gap) {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && bytes.get(start - 1).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
        start -= 1;
    }
    // A digit right before the number belongs to something else.
    let text = std::str::from_utf8(bytes.get(start..end)?).ok()?.trim_matches('.');
    if text.is_empty() || text.len() > 6 {
        return None;
    }
    text.parse().ok()
}

fn number_starting_at(bytes: &[u8], start: usize) -> Option<f64> {
    let mut start = start;
    while bytes.get(start).copied().is_some_and(is_gap) {
        start += 1;
    }
    let mut end = start;
    while bytes.get(end).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
        end += 1;
    }
    let text = std::str::from_utf8(bytes.get(start..end)?).ok()?.trim_matches('.');
    if text.is_empty() || text.len() > 6 {
        return None;
    }
    text.parse().ok()
}

/// A key written in a name between separators: `Am`, `C#m`, `Fmin`, `Ebmaj`, `G#`, `Bb minor`.
/// A lone letter is not taken.
pub fn key_from_name(name: &str) -> Option<String> {
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || matches!(c, '_' | '-' | '.' | '(' | ')' | '[' | ']' | ','))
        .filter(|w| !w.is_empty())
        .collect();
    for (i, word) in words.iter().enumerate() {
        // "Bb minor", "C major": a note followed by a mode word.
        if let Some(next) = words.get(i + 1)
            && let Some(mode) = mode_word(next)
            && let Some(note) = parse_note(word).filter(|(_, rest)| rest.is_empty()).map(|(n, _)| n)
        {
            return Some(format!("{note}{}", if mode { "" } else { "m" }));
        }
        if let Some(key) = key_word(word) {
            return Some(key);
        }
    }
    None
}

/// `true` for a major mode word, `false` for minor.
fn mode_word(word: &str) -> Option<bool> {
    match word.to_ascii_lowercase().as_str() {
        "major" | "maj" => Some(true),
        "minor" | "min" => Some(false),
        _ => None,
    }
}

/// A note name at the start of `word` (`C`, `F#`, `Bb`) and what follows it.
fn parse_note(word: &str) -> Option<(String, &str)> {
    let mut chars = word.chars();
    let letter = chars.next()?;
    if !('A'..='G').contains(&letter) {
        return None;
    }
    let rest = chars.as_str();
    match rest.chars().next() {
        Some(c @ ('#' | 'b' | '♯' | '♭')) => {
            let sign = if matches!(c, '#' | '♯') { '#' } else { 'b' };
            Some((format!("{letter}{sign}"), rest.get(c.len_utf8()..).unwrap_or("")))
        }
        _ => Some((letter.to_string(), rest)),
    }
}

/// A single word that is a key: a note with a sharp or flat, or with a mode (`m`, `min`, `maj`,
/// `minor`, `major`).
fn key_word(word: &str) -> Option<String> {
    let (note, rest) = parse_note(word)?;
    let has_sign = note.len() > 1;
    match rest.to_ascii_lowercase().as_str() {
        "" if has_sign => Some(note),
        "m" | "min" | "minor" => Some(format!("{note}m")),
        "maj" | "major" => Some(note),
        // "M" alone is major in some libraries, but too easily a word; only lower-case m is minor.
        _ => None,
    }
}

/// A key from a metadata tag ("Am", "A minor", "F#", "8A" Camelot codes are not taken).
fn normalise_key(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.len() > 16 {
        return None;
    }
    key_from_name(text).or_else(|| {
        let (note, rest) = parse_note(text)?;
        rest.is_empty().then_some(note)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_come_from_extensions_without_regard_to_case() {
        assert_eq!(kind_of(Path::new("a/Kick.WAV")), Some(FileKind::Sample));
        assert_eq!(kind_of(Path::new("x.aiff")), Some(FileKind::Sample));
        assert_eq!(kind_of(Path::new("groove.MID")), Some(FileKind::Midi));
        assert_eq!(kind_of(Path::new("bank.syx")), Some(FileKind::Preset));
        assert_eq!(kind_of(Path::new("song.suhina")), Some(FileKind::Project));
        assert_eq!(kind_of(Path::new("old song.SUHINA")), Some(FileKind::Project));
        assert_eq!(kind_of(Path::new("set.als")), Some(FileKind::Project));
        assert_eq!(kind_of(Path::new("effects/gain.SD")), Some(FileKind::Effect));
        assert_eq!(kind_of(Path::new("readme.txt")), None);
        assert_eq!(kind_of(Path::new("noextension")), None);
        for kind in [FileKind::Sample, FileKind::Midi, FileKind::Preset, FileKind::Project, FileKind::Effect] {
            assert_eq!(FileKind::parse(kind.name()), Some(kind));
        }
    }

    #[test]
    fn tempo_is_read_next_to_bpm_in_a_name() {
        assert_eq!(tempo_from_name("kick_128bpm"), Some(128.0));
        assert_eq!(tempo_from_name("BPM 90 loop"), Some(90.0));
        assert_eq!(tempo_from_name("Loop 120 BPM Am"), Some(120.0));
        assert_eq!(tempo_from_name("break-87.5bpm"), Some(87.5));
        assert_eq!(tempo_from_name("bpm"), None);
        assert_eq!(tempo_from_name("120 loop"), None);
        assert_eq!(tempo_from_name("9999bpm"), None);
        assert_eq!(tempo_from_name("bpm bpm 100bpm"), Some(100.0));
    }

    #[test]
    fn keys_are_read_from_words_but_not_lone_letters() {
        assert_eq!(key_from_name("Bass Am 120bpm").as_deref(), Some("Am"));
        assert_eq!(key_from_name("pad_C#m_long").as_deref(), Some("C#m"));
        assert_eq!(key_from_name("lead-Fmin").as_deref(), Some("Fm"));
        assert_eq!(key_from_name("Ebmaj7 chord").as_deref(), None);
        assert_eq!(key_from_name("chord Ebmaj").as_deref(), Some("Eb"));
        assert_eq!(key_from_name("stab G#").as_deref(), Some("G#"));
        assert_eq!(key_from_name("Bb minor arp").as_deref(), Some("Bbm"));
        assert_eq!(key_from_name("C major scale").as_deref(), Some("C"));
        assert_eq!(key_from_name("Snare A"), None);
        assert_eq!(key_from_name("Drums"), None);
        assert_eq!(key_from_name(""), None);
    }

    #[test]
    fn tags_give_tempo_and_key() {
        use symphonia::core::meta::RawTag;
        let tags = vec![
            Tag::new(RawTag::new("TEMPO", RawValue::String(std::sync::Arc::new("126".into())))),
            Tag::new(RawTag::new("INITIALKEY", RawValue::String(std::sync::Arc::new("F# minor".into())))),
        ];
        assert_eq!(tempo_and_key_from_tags(&tags), (Some(126.0), Some("F#m".into())));
        let silly = vec![Tag::new(RawTag::new("BPM", RawValue::Float(f64::NAN)))];
        assert_eq!(tempo_and_key_from_tags(&silly), (None, None));
    }

    fn wav_with_chunks(dir: &Path, name: &str, extra: &[u8]) -> std::path::PathBuf {
        let mut data = Vec::new();
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // PCM
        data.extend_from_slice(&1u16.to_le_bytes()); // mono
        data.extend_from_slice(&8000u32.to_le_bytes());
        data.extend_from_slice(&16000u32.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&16u16.to_le_bytes());
        data.extend_from_slice(extra);
        data.extend_from_slice(b"data");
        data.extend_from_slice(&16000u32.to_le_bytes());
        data.extend(std::iter::repeat_n(0u8, 16000));
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(u32::try_from(data.len() + 4).unwrap_or(0)).to_le_bytes());
        file.extend_from_slice(b"WAVE");
        file.extend_from_slice(&data);
        let path = dir.join(name);
        std::fs::write(&path, file).unwrap();
        path
    }

    fn acid(flags: u32, root: u16, tempo: f32) -> Vec<u8> {
        let mut chunk = b"acid".to_vec();
        chunk.extend_from_slice(&24u32.to_le_bytes());
        chunk.extend_from_slice(&flags.to_le_bytes());
        chunk.extend_from_slice(&root.to_le_bytes());
        chunk.extend_from_slice(&[0u8; 14]);
        chunk.extend_from_slice(&tempo.to_le_bytes());
        chunk
    }

    #[test]
    fn a_wav_files_acid_chunk_gives_tempo_and_root() {
        let dir = std::env::temp_dir().join(format!("suhina-info-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = wav_with_chunks(&dir, "loop.wav", &acid(2, 57, 97.0));
        let info = sample_info(&path).unwrap();
        assert_eq!(info.tempo, Some(Tempo { bpm: 97.0, source: TempoSource::Metadata }));
        assert_eq!(info.key.as_deref(), Some("A"));
        assert_eq!((info.sample_rate, info.channels), (Some(8000), Some(1)));
        assert!((info.duration_seconds.unwrap_or(0.0) - 1.0).abs() < 1e-6);

        // A one-shot has no tempo; the name is used instead.
        let path = wav_with_chunks(&dir, "hit 140bpm.wav", &acid(1, 60, 97.0));
        assert_eq!(sample_info(&path).unwrap().tempo, Some(Tempo { bpm: 140.0, source: TempoSource::Name }));

        // A chunk claiming to be huge ends the search instead of reading past the file.
        let mut bogus = b"junk".to_vec();
        bogus.extend_from_slice(&u32::MAX.to_le_bytes());
        let path = wav_with_chunks(&dir, "bogus.wav", &bogus);
        assert_eq!(read_acid(&path).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_audio_is_an_error() {
        let dir = std::env::temp_dir().join(format!("suhina-info-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("noise.wav");
        std::fs::write(&path, b"RIFF\x04\x00\x00\x00WAVE").unwrap();
        assert!(sample_info(&path).is_err());
        assert!(sample_info(&dir.join("missing.wav")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
