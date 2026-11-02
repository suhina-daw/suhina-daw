// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A voice in the layout the engine reads, and DX7 system-exclusive voice data: one voice
//! (163 bytes) or a bank of 32 packed voices (4104 bytes), as Yamaha's published MIDI data
//! format describes them.
//!
//! The layout is the single-voice one: 21 bytes per operator, operator 6 first, then the pitch
//! envelope, algorithm, feedback, LFO, transposition and a 10-character name; one more byte
//! holds which operators are on.

use std::fmt;

use crate::lfo::LfoSettings;

/// Bytes of one voice in a single-voice dump.
pub const VOICE_BYTES: usize = 155;
/// Bytes of one packed voice in a bank.
const PACKED_BYTES: usize = 128;
/// Voices in a bank.
pub const BANK_VOICES: usize = 32;
const BANK_DATA: usize = PACKED_BYTES * BANK_VOICES;

const OP_BYTES: usize = 21;
pub const PITCH_RATE: usize = 126;
pub const PITCH_LEVEL: usize = 130;
pub const ALGORITHM: usize = 134;
pub const FEEDBACK: usize = 135;
pub const OSC_KEY_SYNC: usize = 136;
pub const LFO_SPEED: usize = 137;
pub const LFO_DELAY: usize = 138;
pub const LFO_PITCH_DEPTH: usize = 139;
pub const LFO_AMP_DEPTH: usize = 140;
pub const LFO_SYNC: usize = 141;
pub const LFO_WAVE: usize = 142;
pub const PITCH_MOD_SENS: usize = 143;
pub const TRANSPOSE: usize = 144;
pub const NAME: usize = 145;
/// Not part of the dump: bit `n` is on when operator `6 - n` plays.
pub const OP_SWITCH: usize = 155;

/// Highest value of each byte of a single voice (operator bytes repeat every 21).
const OP_MAX: [u8; OP_BYTES] = [99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 3, 3, 7, 3, 7, 99, 1, 31, 99, 14];
const GLOBAL_MAX: [u8; 19] = [99, 99, 99, 99, 99, 99, 99, 99, 42, 7, 1, 99, 99, 99, 99, 1, 5, 7, 48];
/// The highest algorithm a voice file can hold (algorithm 1 is 0).
const FILE_ALGORITHMS: u8 = 31;

/// The highest value byte `index` of a voice may hold.
pub fn max_value(index: usize) -> u8 {
    if index < 6 * OP_BYTES {
        OP_MAX.get(index % OP_BYTES).copied().unwrap_or(0)
    } else if index < NAME {
        GLOBAL_MAX.get(index - PITCH_RATE).copied().unwrap_or(0)
    } else if index < OP_SWITCH {
        127
    } else {
        0x3f
    }
}

/// One operator's settings, read from a [`Patch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Op {
    pub rates: [i32; 4],
    pub levels: [i32; 4],
    pub break_point: i32,
    pub left_depth: i32,
    pub right_depth: i32,
    pub left_curve: i32,
    pub right_curve: i32,
    pub rate_scaling: i32,
    pub amp_mod_sens: i32,
    pub velocity_sens: i32,
    pub level: i32,
    pub fixed: bool,
    pub coarse: i32,
    pub fine: i32,
    /// 0..=14, 7 is none.
    pub detune: i32,
    pub on: bool,
}

/// A voice: the 155 bytes of a single-voice dump and which operators are on. Every byte is
/// kept within its range, so the engine can trust it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Patch {
    bytes: [u8; VOICE_BYTES + 1],
}

impl fmt::Debug for Patch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Patch").field("name", &self.name()).finish_non_exhaustive()
    }
}

impl Default for Patch {
    /// The initial voice: operator 1 alone, a sine at full level.
    fn default() -> Self {
        let mut bytes = [0u8; VOICE_BYTES + 1];
        for op in 0..6 {
            let o = op * OP_BYTES;
            let defaults = [99, 99, 99, 99, 99, 99, 99, 0, 39, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 7];
            if let Some(slot) = bytes.get_mut(o..o + OP_BYTES) {
                slot.copy_from_slice(&defaults);
            }
        }
        // Operator 1 is the last group.
        if let Some(level) = bytes.get_mut(5 * OP_BYTES + 16) {
            *level = 99;
        }
        let global = [99, 99, 99, 99, 50, 50, 50, 50, 0, 0, 1, 35, 0, 0, 0, 1, 0, 3, 24];
        if let Some(slot) = bytes.get_mut(PITCH_RATE..NAME) {
            slot.copy_from_slice(&global);
        }
        if let Some(slot) = bytes.get_mut(NAME..OP_SWITCH) {
            slot.copy_from_slice(b"INIT VOICE");
        }
        if let Some(slot) = bytes.get_mut(OP_SWITCH) {
            *slot = 0x3f;
        }
        Self { bytes }
    }
}

impl Patch {
    /// A voice from the 155 bytes of a single-voice dump; values out of range are limited and
    /// every operator is on.
    pub fn from_voice(data: &[u8; VOICE_BYTES]) -> Self {
        let mut patch = Self::default();
        for (i, &value) in data.iter().enumerate() {
            patch.set(i, value);
        }
        // The algorithms after the file format's 32 are the device's own.
        if data.get(ALGORITHM).copied().unwrap_or(0) > FILE_ALGORITHMS {
            patch.set(ALGORITHM, FILE_ALGORITHMS);
        }
        patch.set(OP_SWITCH, 0x3f);
        patch
    }

    /// The 155 bytes of a single-voice dump.
    pub fn to_voice(&self) -> [u8; VOICE_BYTES] {
        let mut out = [0u8; VOICE_BYTES];
        if let Some(bytes) = self.bytes.get(..VOICE_BYTES) {
            out.copy_from_slice(bytes);
        }
        if let Some(algorithm) = out.get_mut(ALGORITHM) {
            *algorithm = (*algorithm).min(FILE_ALGORITHMS);
        }
        out
    }

    /// Byte `index` (see the constants), as a number.
    pub fn byte(&self, index: usize) -> i32 {
        i32::from(self.bytes.get(index).copied().unwrap_or(0))
    }

    /// Sets byte `index`, limited to its range; unknown indices are ignored.
    pub fn set(&mut self, index: usize, value: u8) {
        let max = max_value(index);
        if let Some(slot) = self.bytes.get_mut(index) {
            *slot = value.min(max);
        }
    }

    /// Operator settings; `op` 0 is operator 6, as stored.
    pub fn op(&self, op: usize) -> Op {
        let o = op * OP_BYTES;
        let b = |i: usize| self.byte(o + i);
        Op {
            rates: [b(0), b(1), b(2), b(3)],
            levels: [b(4), b(5), b(6), b(7)],
            break_point: b(8),
            left_depth: b(9),
            right_depth: b(10),
            left_curve: b(11),
            right_curve: b(12),
            rate_scaling: b(13),
            amp_mod_sens: b(14),
            velocity_sens: b(15),
            level: b(16),
            fixed: b(17) != 0,
            coarse: b(18),
            fine: b(19),
            detune: b(20),
            on: self.byte(OP_SWITCH) & (1 << op) != 0,
        }
    }

    pub fn pitch_rates(&self) -> [i32; 4] {
        std::array::from_fn(|i| self.byte(PITCH_RATE + i))
    }

    pub fn pitch_levels(&self) -> [i32; 4] {
        std::array::from_fn(|i| self.byte(PITCH_LEVEL + i))
    }

    /// 0..=42 (algorithm 1 is 0).
    pub fn algorithm(&self) -> usize {
        self.byte(ALGORITHM).clamp(0, 42) as usize
    }

    pub fn feedback(&self) -> i32 {
        self.byte(FEEDBACK)
    }

    pub fn osc_key_sync(&self) -> bool {
        self.byte(OSC_KEY_SYNC) != 0
    }

    pub fn lfo(&self) -> LfoSettings {
        LfoSettings {
            speed: self.byte(LFO_SPEED) as u8,
            delay: self.byte(LFO_DELAY) as u8,
            sync: self.byte(LFO_SYNC) != 0,
            wave: self.byte(LFO_WAVE) as u8,
        }
    }

    /// Semitones every key is moved by.
    pub fn transpose(&self) -> i32 {
        self.byte(TRANSPOSE) - 24
    }

    /// The voice's name, with characters outside printable ASCII shown as spaces.
    pub fn name(&self) -> String {
        let name = self.bytes.get(NAME..OP_SWITCH).unwrap_or_default();
        name.iter()
            .map(|&c| if (32..127).contains(&c) { char::from(c) } else { ' ' })
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// Sets the name: up to 10 printable ASCII characters, padded with spaces.
    pub fn set_name(&mut self, name: &str) {
        let mut chars = name.chars().filter(|c| c.is_ascii() && !c.is_ascii_control());
        for i in NAME..OP_SWITCH {
            let c = chars.next().map_or(b' ', |c| c as u8);
            if let Some(slot) = self.bytes.get_mut(i) {
                *slot = c;
            }
        }
    }

    /// The 128-byte packed form a bank holds.
    fn pack(&self) -> [u8; PACKED_BYTES] {
        let mut p = [0u8; PACKED_BYTES];
        let b = |i: usize| self.bytes.get(i).copied().unwrap_or(0);
        let mut put = |i: usize, v: u8| {
            if let Some(slot) = p.get_mut(i) {
                *slot = v & 0x7f;
            }
        };
        for op in 0..6 {
            let (s, d) = (op * OP_BYTES, op * 17);
            for i in 0..11 {
                put(d + i, b(s + i));
            }
            put(d + 11, (b(s + 11) & 3) | ((b(s + 12) & 3) << 2));
            put(d + 12, (b(s + 13) & 7) | ((b(s + 20) & 15) << 3));
            put(d + 13, (b(s + 14) & 3) | ((b(s + 15) & 7) << 2));
            put(d + 14, b(s + 16));
            put(d + 15, (b(s + 17) & 1) | ((b(s + 18) & 31) << 1));
            put(d + 16, b(s + 19));
        }
        for i in 0..8 {
            put(102 + i, b(PITCH_RATE + i));
        }
        put(110, b(ALGORITHM));
        put(111, (b(FEEDBACK) & 7) | ((b(OSC_KEY_SYNC) & 1) << 3));
        for i in 0..4 {
            put(112 + i, b(LFO_SPEED + i));
        }
        put(116, (b(LFO_SYNC) & 1) | ((b(LFO_WAVE) & 7) << 1) | ((b(PITCH_MOD_SENS) & 7) << 4));
        put(117, b(TRANSPOSE));
        for i in 0..10 {
            put(118 + i, b(NAME + i));
        }
        p
    }

    /// A voice from its packed form; values out of range are limited.
    fn unpack(p: &[u8]) -> Self {
        let g = |i: usize| p.get(i).copied().unwrap_or(0) & 0x7f;
        let mut v = [0u8; VOICE_BYTES];
        let mut put = |i: usize, x: u8| {
            if let Some(slot) = v.get_mut(i) {
                *slot = x;
            }
        };
        for op in 0..6 {
            let (s, d) = (op * 17, op * OP_BYTES);
            for i in 0..11 {
                put(d + i, g(s + i));
            }
            put(d + 11, g(s + 11) & 3);
            put(d + 12, (g(s + 11) >> 2) & 3);
            put(d + 13, g(s + 12) & 7);
            put(d + 20, (g(s + 12) >> 3) & 15);
            put(d + 14, g(s + 13) & 3);
            put(d + 15, (g(s + 13) >> 2) & 7);
            put(d + 16, g(s + 14));
            put(d + 17, g(s + 15) & 1);
            put(d + 18, (g(s + 15) >> 1) & 31);
            put(d + 19, g(s + 16));
        }
        for i in 0..8 {
            put(PITCH_RATE + i, g(102 + i));
        }
        put(ALGORITHM, g(110) & 31);
        put(FEEDBACK, g(111) & 7);
        put(OSC_KEY_SYNC, (g(111) >> 3) & 1);
        for i in 0..4 {
            put(LFO_SPEED + i, g(112 + i));
        }
        put(LFO_SYNC, g(116) & 1);
        put(LFO_WAVE, (g(116) >> 1) & 7);
        put(PITCH_MOD_SENS, (g(116) >> 4) & 7);
        put(TRANSPOSE, g(117));
        for i in 0..10 {
            put(NAME + i, g(118 + i));
        }
        Self::from_voice(&v)
    }
}

/// Why voice data could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysexError {
    /// Not a DX7 voice or bank message (wrong length, header or end).
    NotVoiceData,
}

impl fmt::Display for SysexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotVoiceData => write!(f, "not DX7 voice data (a 163-byte voice or a 4104-byte bank)"),
        }
    }
}

impl std::error::Error for SysexError {}

/// Voices read from system-exclusive data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voices {
    pub patches: Vec<Patch>,
    /// Whether the message's checksum matched. Many banks in circulation have a wrong one and
    /// still hold good voices, so a mismatch is reported, not refused.
    pub checksum_ok: bool,
}

const SYSEX_START: u8 = 0xf0;
const SYSEX_END: u8 = 0xf7;
const YAMAHA: u8 = 0x43;
const FORMAT_VOICE: u8 = 0;
const FORMAT_BANK: u8 = 9;

/// The DX7 checksum: the two's complement of the sum of the data, 7 bits.
pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0u8, |sum, &b| sum.wrapping_sub(b)) & 0x7f
}

/// Reads a single-voice message (163 bytes), a bank message (4104 bytes) or a bank's bare
/// 4096 bytes of packed voices. Values out of range are limited, never refused.
pub fn read(data: &[u8]) -> Result<Voices, SysexError> {
    if data.len() == BANK_DATA {
        let patches = data.chunks(PACKED_BYTES).map(Patch::unpack).collect();
        return Ok(Voices { patches, checksum_ok: true });
    }
    let [start, maker, _channel, format, high, low, rest @ ..] = data else {
        return Err(SysexError::NotVoiceData);
    };
    let [body @ .., sum, end] = rest else {
        return Err(SysexError::NotVoiceData);
    };
    if *start != SYSEX_START || *maker != YAMAHA || *end != SYSEX_END {
        return Err(SysexError::NotVoiceData);
    }
    let length = (usize::from(*high) << 7) | usize::from(*low);
    let checksum_ok = checksum(body) == *sum;
    match (*format, length, body.len()) {
        (FORMAT_VOICE, VOICE_BYTES, VOICE_BYTES) => {
            let mut voice = [0u8; VOICE_BYTES];
            voice.copy_from_slice(body);
            Ok(Voices { patches: vec![Patch::from_voice(&voice)], checksum_ok })
        }
        (FORMAT_BANK, 4096, BANK_DATA) => {
            Ok(Voices { patches: body.chunks(PACKED_BYTES).map(Patch::unpack).collect(), checksum_ok })
        }
        _ => Err(SysexError::NotVoiceData),
    }
}

/// A single-voice message for `patch` on MIDI channel 1.
pub fn write_voice(patch: &Patch) -> Vec<u8> {
    let voice = patch.to_voice();
    let mut out = vec![SYSEX_START, YAMAHA, 0, FORMAT_VOICE, 0x01, 0x1b];
    out.extend_from_slice(&voice);
    out.push(checksum(&voice));
    out.push(SYSEX_END);
    out
}

/// A bank message of 32 voices on MIDI channel 1; missing voices are the initial voice and
/// voices past 32 are left out.
pub fn write_bank(patches: &[Patch]) -> Vec<u8> {
    let mut data = Vec::with_capacity(BANK_DATA);
    for i in 0..BANK_VOICES {
        data.extend_from_slice(&patches.get(i).copied().unwrap_or_default().pack());
    }
    let mut out = vec![SYSEX_START, YAMAHA, 0, FORMAT_BANK, 0x20, 0x00];
    out.extend_from_slice(&data);
    out.push(checksum(&data));
    out.push(SYSEX_END);
    out
}
