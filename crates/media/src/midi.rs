// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Standard MIDI files (`.mid`): the notes of each MIDI track, in beats, for the browser's MIDI
//! clips and previews (`docs/spec/browser.md` › Files). Read by hand from the public file layout
//! (a header chunk, then one chunk per track of delta-timed events); every count and length in
//! the file is treated as hostile.

use std::path::Path;

use suhina_model::Note;

/// Largest file read.
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
/// Most MIDI tracks read.
pub const MAX_TRACKS: usize = 64;
/// Most notes in a file.
pub const MAX_NOTES: usize = 1_000_000;

/// One MIDI track that has notes.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiTrack {
    /// The track's name event, if it has one.
    pub name: Option<String>,
    /// Notes sorted by time, in beats from the file's start.
    pub notes: Vec<Note>,
}

impl MidiTrack {
    /// Where the last note ends, in beats.
    pub fn end(&self) -> f64 {
        self.notes.iter().map(|n| n.time + n.duration).fold(0.0, f64::max)
    }
}

/// What a MIDI file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiFile {
    /// The tracks with notes, in the file's order.
    pub tracks: Vec<MidiTrack>,
    /// The first tempo event, in beats per minute.
    pub tempo_bpm: Option<f64>,
}

/// Reads the MIDI file at `path`.
pub fn read_file(path: &Path) -> Result<MidiFile, String> {
    let length = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if length > MAX_BYTES {
        return Err(format!("larger than {} MiB", MAX_BYTES / 1024 / 1024));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    read(&bytes)
}

/// Reads a MIDI file from its bytes.
pub fn read(bytes: &[u8]) -> Result<MidiFile, String> {
    let mut reader = Reader { bytes, at: 0 };
    let (id, header) = reader.chunk()?;
    if id != *b"MThd" || header.len() < 6 {
        return Err("not a MIDI file".into());
    }
    let format = u16::from_be_bytes([header[0], header[1]]);
    let count = usize::from(u16::from_be_bytes([header[2], header[3]]));
    let division = u16::from_be_bytes([header[4], header[5]]);
    if format > 2 {
        return Err(format!("unknown MIDI file format {format}"));
    }
    if division & 0x8000 != 0 {
        return Err("time in SMPTE frames is not supported".into());
    }
    if division == 0 {
        return Err("zero ticks per beat".into());
    }
    let ticks_per_beat = f64::from(division);

    let mut tracks = Vec::new();
    let mut tempo_bpm = None;
    let mut notes_total = 0usize;
    let mut read_tracks = 0usize;
    while read_tracks < count.min(MAX_TRACKS) && !reader.done() {
        let (id, body) = reader.chunk()?;
        if id != *b"MTrk" {
            // Unknown chunks are skipped, as the format allows.
            continue;
        }
        read_tracks += 1;
        let track = read_track(body, ticks_per_beat, &mut tempo_bpm, MAX_NOTES - notes_total)?;
        notes_total += track.notes.len();
        if !track.notes.is_empty() {
            tracks.push(track);
        }
    }
    Ok(MidiFile { tracks, tempo_bpm })
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn byte(&mut self) -> Result<u8, String> {
        let b = self.bytes.get(self.at).copied().ok_or("the file ends in the middle of an event")?;
        self.at += 1;
        Ok(b)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(count).ok_or("a length is too large")?;
        let slice = self.bytes.get(self.at..end).ok_or("a length runs past the end of the file")?;
        self.at = end;
        Ok(slice)
    }

    fn chunk(&mut self) -> Result<([u8; 4], &'a [u8]), String> {
        let head = self.take(8)?;
        let id = [head[0], head[1], head[2], head[3]];
        let length = u32::from_be_bytes([head[4], head[5], head[6], head[7]]);
        let length = usize::try_from(length).map_err(|_| "a chunk is too large")?;
        // A last chunk that claims more than is left is read up to the end, as players do.
        let length = length.min(self.bytes.len() - self.at);
        Ok((id, self.take(length)?))
    }

    /// A variable-length number: seven bits a byte, at most four bytes.
    fn number(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let b = self.byte()?;
            value = (value << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("a variable-length number is longer than four bytes".into())
    }
}

fn read_track(
    body: &[u8],
    ticks_per_beat: f64,
    tempo: &mut Option<f64>,
    notes_left: usize,
) -> Result<MidiTrack, String> {
    let mut reader = Reader { bytes: body, at: 0 };
    let mut tick: u64 = 0;
    let mut status: Option<u8> = None;
    let mut name = None;
    // Notes sounding, per channel and key: when each started (ticks) and how hard.
    let mut sounding: Vec<Vec<(u64, u8)>> = vec![Vec::new(); 16 * 128];
    let mut notes: Vec<Note> = Vec::new();
    let finish = |notes: &mut Vec<Note>, start: u64, end: u64, channel: u8, key: u8, velocity: u8, release: u8| {
        if notes.len() >= notes_left {
            return false;
        }
        notes.push(Note {
            time: start as f64 / ticks_per_beat,
            duration: (end.saturating_sub(start) as f64 / ticks_per_beat).max(1.0 / 64.0),
            key,
            velocity: f32::from(velocity) / 127.0,
            release_velocity: f32::from(release) / 127.0,
            channel,
            // A MIDI file does not mark a note as muted; every note it holds plays.
            muted: false,
        });
        true
    };
    while !reader.done() {
        tick = tick.saturating_add(u64::from(reader.number()?));
        let first = reader.byte()?;
        let (event, data1) = if first & 0x80 != 0 {
            (first, None)
        } else {
            // Running status: this byte is the first data byte of the last channel event.
            (status.ok_or("data byte without a status")?, Some(first))
        };
        match event {
            0xFF => {
                let kind = reader.byte()?;
                let length = usize::try_from(reader.number()?).map_err(|_| "a meta event is too long")?;
                let data = reader.take(length)?;
                match kind {
                    0x03 if name.is_none() => {
                        let text = String::from_utf8_lossy(data).trim().to_string();
                        name = Some(text.chars().take(128).collect::<String>()).filter(|t| !t.is_empty());
                    }
                    0x51 if data.len() >= 3 && tempo.is_none() => {
                        let micros = u32::from(data[0]) << 16 | u32::from(data[1]) << 8 | u32::from(data[2]);
                        if micros > 0 {
                            *tempo = Some(60_000_000.0 / f64::from(micros)).filter(|t| (1.0..=1000.0).contains(t));
                        }
                    }
                    0x2F => break,
                    _ => {}
                }
                status = None;
            }
            0xF0 | 0xF7 => {
                let length = usize::try_from(reader.number()?).map_err(|_| "a system-exclusive event is too long")?;
                reader.take(length)?;
                status = None;
            }
            0x80..=0xEF => {
                status = Some(event);
                let channel = event & 0x0F;
                let a = match data1 {
                    Some(a) => a,
                    None => reader.byte()?,
                } & 0x7F;
                let two = !matches!(event & 0xF0, 0xC0 | 0xD0);
                let b = if two { reader.byte()? & 0x7F } else { 0 };
                let slot = usize::from(channel) * 128 + usize::from(a);
                match event & 0xF0 {
                    0x90 if b > 0 => {
                        if let Some(list) = sounding.get_mut(slot) {
                            list.push((tick, b));
                        }
                    }
                    0x80 | 0x90 => {
                        // The earliest sounding note of this key ends (first in, first out).
                        if let Some(list) = sounding.get_mut(slot)
                            && !list.is_empty()
                        {
                            let (start, velocity) = list.remove(0);
                            let release = if event & 0xF0 == 0x80 { b } else { 64 };
                            if !finish(&mut notes, start, tick, channel, a, velocity, release) {
                                return Err(format!("more than {MAX_NOTES} notes"));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => return Err(format!("unknown event {event:#04x}")),
        }
    }
    // Notes never released end where the track ends.
    for (slot, list) in sounding.iter().enumerate() {
        for (start, velocity) in list {
            let channel = u8::try_from(slot / 128).unwrap_or(0);
            let key = u8::try_from(slot % 128).unwrap_or(0);
            if !finish(&mut notes, *start, tick.max(*start + 1), channel, key, *velocity, 64) {
                return Err(format!("more than {MAX_NOTES} notes"));
            }
        }
    }
    notes.sort_by(|a, b| a.time.total_cmp(&b.time).then(a.key.cmp(&b.key)));
    Ok(MidiTrack { name, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    fn file(format: u16, division: u16, tracks: &[Vec<u8>]) -> Vec<u8> {
        let mut header = format.to_be_bytes().to_vec();
        header.extend_from_slice(&u16::try_from(tracks.len()).unwrap_or(0).to_be_bytes());
        header.extend_from_slice(&division.to_be_bytes());
        let mut out = chunk(b"MThd", &header);
        for t in tracks {
            out.extend(chunk(b"MTrk", t));
        }
        out
    }

    #[test]
    fn reads_notes_names_and_tempo_from_a_type_1_file() {
        // Track 1: tempo 100 BPM (600 000 µs), no notes.
        let conductor = vec![0x00, 0xFF, 0x51, 0x03, 0x09, 0x27, 0xC0, 0x00, 0xFF, 0x2F, 0x00];
        // Track 2: named "Bass", C3 for a beat, then (running status) E3 at beat 1 for half a beat
        // ended by a note-on with velocity 0.
        let bass = vec![
            0x00, 0xFF, 0x03, 0x04, b'B', b'a', b's', b's', //
            0x00, 0x90, 48, 100, //
            0x60, 0x80, 48, 0x40, //
            0x00, 0x90, 52, 64, //
            0x30, 52, 0, //
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let midi = read(&file(1, 96, &[conductor, bass])).unwrap();
        assert_eq!(midi.tempo_bpm, Some(100.0));
        assert_eq!(midi.tracks.len(), 1, "the conductor track has no notes");
        let track = &midi.tracks[0];
        assert_eq!(track.name.as_deref(), Some("Bass"));
        let got: Vec<(f64, f64, u8)> = track.notes.iter().map(|n| (n.time, n.duration, n.key)).collect();
        assert_eq!(got, vec![(0.0, 1.0, 48), (1.0, 0.5, 52)]);
        assert!((track.notes[0].velocity - 100.0 / 127.0).abs() < 1e-6);
        assert_eq!(track.end(), 1.5);
    }

    #[test]
    fn a_note_never_released_ends_with_its_track() {
        let track = vec![0x00, 0x91, 60, 90, 0x83, 0x00, 0xFF, 0x2F, 0x00];
        let midi = read(&file(0, 96, &[track])).unwrap();
        let note = &midi.tracks[0].notes[0];
        assert_eq!((note.time, note.duration, note.channel), (0.0, 4.0, 1));
    }

    #[test]
    fn hostile_files_are_errors_not_crashes() {
        assert!(read(b"").is_err());
        assert!(read(b"RIFF....").is_err());
        assert!(read(&file(0, 0, &[])).is_err(), "zero ticks per beat");
        assert!(read(&file(0, 0xE728, &[])).is_err(), "SMPTE time");
        assert!(read(&file(7, 96, &[])).is_err(), "unknown format");
        // A five-byte variable-length number.
        assert!(read(&file(0, 96, &[vec![0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x90, 1, 1]])).is_err());
        // A meta event longer than the track.
        assert!(read(&file(0, 96, &[vec![0x00, 0xFF, 0x01, 0x7F, 0x00]])).is_err());
        // A data byte with no status before it.
        assert!(read(&file(0, 96, &[vec![0x00, 0x40, 0x40]])).is_err());
        // An event cut off.
        assert!(read(&file(0, 96, &[vec![0x00, 0x90, 60]])).is_err());
        // A chunk that claims more than is left is read up to the end.
        let mut short = file(0, 96, &[vec![0x00, 0x90, 60, 90, 0x60, 0x80, 60, 0]]);
        let at = short.len() - 12;
        short[at..at + 4].copy_from_slice(&1_000_000u32.to_be_bytes());
        assert_eq!(read(&short).unwrap().tracks[0].notes.len(), 1);
        // Garbage of every length never panics.
        let mut seed = 3u32;
        for length in 0..400 {
            let mut bytes = file(1, 96, &[]);
            bytes.extend(b"MTrk");
            bytes.extend(&u32::try_from(length).unwrap_or(0).to_be_bytes());
            for _ in 0..length {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                bytes.push((seed >> 16) as u8);
            }
            let _ = read(&bytes);
        }
    }

    #[test]
    fn the_track_count_is_capped() {
        let tracks: Vec<Vec<u8>> = (0..100).map(|_| vec![0x00, 0x90, 60, 90, 0x10, 0x80, 60, 0]).collect();
        let midi = read(&file(1, 96, &tracks)).unwrap();
        assert_eq!(midi.tracks.len(), MAX_TRACKS);
    }
}
