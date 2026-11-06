// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MP3 encoding through LAME 3.100's encoder, vendored as C source in `third_party/lame/`
//! ([ADR 0029](../../docs/decisions/0029-mp3-encoder.md)). This is the one crate that may
//! touch the encoder's C API: the `unsafe` lives in the calls below, everything outward is
//! safe, checked and panic-free, and no engine or audio-callback code reaches an encoder at
//! all — encoding runs on an export's own path, from a render that is already finished.
//!
//! The safe API mirrors how an export uses it: build an [`Mp3Encoder`] from [`Mp3Settings`],
//! feed it the rendered stereo in blocks with [`Mp3Encoder::encode`], and close it with
//! [`Mp3Encoder::finish`], which flushes the stream and returns the LAME tag frame the file
//! carries at its start (players read it for gapless playback and the true length). The
//! file's bytes are assembled by [`assemble_mp3_file`].

use core::ffi::{c_char, c_int};
use core::fmt;

/// The encoder's C API handle. A `lame_global_flags` is only ever created, configured and
/// freed by the library itself; nothing here reads or writes its fields, so an unsized
/// stand-in type is what the pointers carry.
#[repr(C)]
struct LameFlags {
    _private: [u8; 0],
}

/// The MPEG channel mode the encoder writes. Export is stereo, so the default is joint
/// stereo; a mono file is mono.
const MODE_JOINT_STEREO: c_int = 1;
const MODE_MONO: c_int = 3;

/// LAME's default VBR algorithm, from `lame.h` (its constant-bitrate case is `VBR_OFF`, 0,
/// and is the library's default mode, so the CBR path only sets the bitrate).
const VBR_DEFAULT: c_int = 4;

/// The sample rates LAME's MPEG tables encode: MPEG 1 at 32, 44.1 and 48 kHz, MPEG 2 and
/// 2.5 at the halved and quartered ones. Anything else must be resampled first.
const SAMPLE_RATES: [u32; 9] = [8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000];

/// LAME's algorithm quality, the `-q 2` of the command line, what the encoder was tuned for.
const ALGORITHM_QUALITY: c_int = 2;

/// The bitrate a constant-bitrate file carries, in kbps, or the quality a variable-bitrate
/// file targets, 0 (best) to 9 (worst). The export dialog offers 128, 192, 256 and 320 kbps
/// and the qualities 0 and 2; the encoder itself takes any of LAME's legal values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp3Rate {
    /// Constant bitrate, in kbps (8 to 320).
    Cbr(u32),
    /// Variable bitrate by LAME's quality, 0 to 9.
    Vbr(u8),
}

impl Default for Mp3Rate {
    fn default() -> Self {
        Self::Cbr(320)
    }
}

impl Mp3Rate {
    /// The rate the command's name says: the constant ones in kbps and the two variable
    /// qualities the dialog offers, `v0` best and `v2` its well-used middle.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "128" => Some(Self::Cbr(128)),
            "192" => Some(Self::Cbr(192)),
            "256" => Some(Self::Cbr(256)),
            "320" => Some(Self::Cbr(320)),
            "v0" => Some(Self::Vbr(0)),
            "v2" => Some(Self::Vbr(2)),
            _ => None,
        }
    }
}

/// The tags the file's ID3v2 header carries. Nothing but the title, per the export spec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mp3Tags {
    /// The title, normally the project's name; empty writes no header at all.
    pub title: String,
}

/// What an encoder is built from: the rate, the input's sample rate and channel count, and
/// the tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mp3Settings {
    pub rate: Mp3Rate,
    pub sample_rate: u32,
    /// 1 or 2 channels; 2 is written as joint stereo.
    pub channels: u8,
    pub tags: Mp3Tags,
}

/// Why an encode failed. No encoder failure panics; every one is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mp3Error {
    /// A setting is outside what the encoder takes, or a text carries a NUL.
    InvalidSettings(String),
    /// The encoder could not allocate its own buffers.
    OutOfMemory,
    /// The encoder rejected a block mid-stream; the code is LAME's.
    Encode(i32),
    /// The encoder rejected the flush; the code is LAME's.
    Flush(i32),
    /// Putting the finished stream and its LAME tag together into a file failed.
    Assemble(String),
}

impl fmt::Display for Mp3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSettings(text) => f.write_str(text),
            Self::OutOfMemory => f.write_str("the encoder could not allocate memory"),
            Self::Encode(code) => write!(f, "the encoder failed with code {code}"),
            Self::Flush(code) => write!(f, "flushing the encoder failed with code {code}"),
            Self::Assemble(text) => f.write_str(text),
        }
    }
}

impl std::error::Error for Mp3Error {}

/// The encoder's worst-case output for one call, LAME's own bound for `mp3buf_size`:
/// 1.25 bytes per sample plus 7200. Hostile sizes are refused, not wrapped.
fn worst_case_bytes(samples: usize) -> Result<usize, Mp3Error> {
    samples
        .checked_mul(5)
        .and_then(|v| v.checked_add(7200 * 4))
        .map(|v| v / 4)
        .ok_or_else(|| Mp3Error::InvalidSettings("the block is too long to size an output buffer for".into()))
}

// The C API, declared by hand for the handful of functions the safe API uses. All of them
// are CDECL on the targets this workspace builds.
unsafe extern "C" {
    fn lame_init() -> *mut LameFlags;
    fn lame_set_in_samplerate(gfp: *mut LameFlags, rate: c_int) -> c_int;
    fn lame_set_num_channels(gfp: *mut LameFlags, channels: c_int) -> c_int;
    fn lame_set_brate(gfp: *mut LameFlags, kbps: c_int) -> c_int;
    fn lame_set_VBR(gfp: *mut LameFlags, vbr: c_int) -> c_int;
    fn lame_set_VBR_quality(gfp: *mut LameFlags, quality: f32) -> c_int;
    fn lame_set_mode(gfp: *mut LameFlags, mode: c_int) -> c_int;
    fn lame_set_quality(gfp: *mut LameFlags, quality: c_int) -> c_int;
    fn lame_init_params(gfp: *mut LameFlags) -> c_int;
    fn lame_encode_buffer_ieee_float(
        gfp: *mut LameFlags,
        pcm_l: *const f32,
        pcm_r: *const f32,
        nsamples: c_int,
        mp3buf: *mut u8,
        mp3buf_size: c_int,
    ) -> c_int;
    fn lame_encode_flush(gfp: *mut LameFlags, mp3buf: *mut u8, mp3buf_size: c_int) -> c_int;
    fn lame_get_lametag_frame(gfp: *const LameFlags, buffer: *mut u8, size: usize) -> usize;
    fn lame_close(gfp: *mut LameFlags) -> c_int;
    fn id3tag_init(gfp: *mut LameFlags);
    fn id3tag_v2_only(gfp: *mut LameFlags);
    fn id3tag_set_textinfo_utf16(gfp: *mut LameFlags, id: *const c_char, text: *const u16) -> c_int;
}

/// A negative return from one of the setters: the setting was rejected.
fn rejected(name: &str) -> Mp3Error {
    Mp3Error::InvalidSettings(format!("the {name} was rejected by the encoder"))
}

/// An encoder, owning its C handle for as long as it lives.
///
/// The handle is not `Send` (the raw pointer says so itself): build, feed and finish the
/// encoder on one thread, which is what an export already does.
pub struct Mp3Encoder {
    gfp: *mut LameFlags,
}

impl Mp3Encoder {
    /// Builds an encoder for `settings` and settles its configuration inside the library.
    /// The sample rate must be one of [`SAMPLE_RATES`] and the channel count 1 or 2.
    pub fn new(settings: &Mp3Settings) -> Result<Self, Mp3Error> {
        if !SAMPLE_RATES.contains(&settings.sample_rate) {
            return Err(Mp3Error::InvalidSettings(format!(
                "sample rate {} is not one the encoder writes; 32, 44.1 and 48 kHz and their MPEG 2 halves are",
                settings.sample_rate
            )));
        }
        match settings.channels {
            1 | 2 => {}
            other => {
                return Err(Mp3Error::InvalidSettings(format!("{other} channels is not 1 or 2")));
            }
        }
        match settings.rate {
            Mp3Rate::Cbr(kbps) => {
                if !(8..=320).contains(&kbps) {
                    return Err(Mp3Error::InvalidSettings(format!(
                        "a bitrate of {kbps} kbps is not between 8 and 320"
                    )));
                }
            }
            Mp3Rate::Vbr(quality) => {
                if quality > 9 {
                    return Err(Mp3Error::InvalidSettings(format!("a VBR quality of {quality} is not 0 to 9")));
                }
            }
        }

        // SAFETY: `lame_init` allocates the encoder; a null return is its only failure.
        let gfp = unsafe { lame_init() };
        if gfp.is_null() {
            return Err(Mp3Error::OutOfMemory);
        }
        let encoder = Self { gfp };
        encoder.configure(settings)?;
        Ok(encoder)
    }

    /// Sets the encoder's parameters and settles them; every step's failure is an error and
    /// leaves the handle to `Drop`.
    fn configure(&self, settings: &Mp3Settings) -> Result<(), Mp3Error> {
        let rate = c_int::try_from(settings.sample_rate)
            .map_err(|_| Mp3Error::InvalidSettings("the sample rate does not fit the encoder".into()))?;
        // SAFETY: `gfp` is a live handle; the setters check and store plain values.
        if unsafe { lame_set_in_samplerate(self.gfp, rate) } < 0 {
            return Err(rejected("sample rate"));
        }
        // SAFETY: as above.
        if unsafe { lame_set_num_channels(self.gfp, c_int::from(settings.channels)) } < 0 {
            return Err(rejected("channel count"));
        }
        match settings.rate {
            Mp3Rate::Cbr(kbps) => {
                let kbps = c_int::try_from(kbps).map_err(|_| rejected("bitrate"))?;
                // SAFETY: as above.
                if unsafe { lame_set_brate(self.gfp, kbps) } < 0 {
                    return Err(rejected("bitrate"));
                }
            }
            Mp3Rate::Vbr(quality) => {
                // SAFETY: as above.
                if unsafe { lame_set_VBR(self.gfp, VBR_DEFAULT) } < 0 {
                    return Err(rejected("VBR mode"));
                }
                // SAFETY: as above.
                if unsafe { lame_set_VBR_quality(self.gfp, f32::from(quality)) } < 0 {
                    return Err(rejected("VBR quality"));
                }
            }
        }
        let mode = match settings.channels {
            1 => MODE_MONO,
            _ => MODE_JOINT_STEREO,
        };
        // SAFETY: as above.
        if unsafe { lame_set_mode(self.gfp, mode) } < 0 {
            return Err(rejected("channel mode"));
        }
        // SAFETY: as above.
        if unsafe { lame_set_quality(self.gfp, ALGORITHM_QUALITY) } < 0 {
            return Err(rejected("algorithm quality"));
        }
        if !settings.tags.title.is_empty() {
            if settings.tags.title.contains('\0') {
                return Err(Mp3Error::InvalidSettings("the title carries a NUL".into()));
            }
            // The title as UTF-16 led by its byte order mark: LAME's plain-text setter writes
            // its bytes as Latin-1, which turns any name beyond ASCII into another one.
            let title: Vec<u16> =
                core::iter::once(0xFEFF).chain(settings.tags.title.encode_utf16()).chain(core::iter::once(0)).collect();
            // SAFETY: `id3tag_init` and `id3tag_v2_only` take no data.
            unsafe {
                id3tag_init(self.gfp);
            }
            // SAFETY: as above. V2 only, so the file carries one header at its start and no
            // v1 trailer.
            unsafe {
                id3tag_v2_only(self.gfp);
            }
            // SAFETY: the frame id is a NUL-terminated literal; `title` is BOM-led,
            // NUL-terminated UTF-16 that outlives the call, which copies it.
            if unsafe { id3tag_set_textinfo_utf16(self.gfp, c"TIT2".as_ptr(), title.as_ptr()) } < 0 {
                return Err(rejected("title"));
            }
        }
        // SAFETY: `lame_init_params` settles the set parameters; a negative return is a
        // rejected combination.
        if unsafe { lame_init_params(self.gfp) } < 0 {
            return Err(Mp3Error::InvalidSettings("the settings together were rejected by the encoder".into()));
        }
        Ok(())
    }

    /// Encodes one block of the render: `left` and `right` must hold the same number of
    /// samples, in the ±1 full scale the exported mix is in: a sample that is not finite or
    /// lies past full scale is refused, since the quantizer indexes its tables by the value.
    /// The frames produced are appended to `out`.
    pub fn encode(&mut self, left: &[f32], right: &[f32], out: &mut Vec<u8>) -> Result<(), Mp3Error> {
        if left.len() != right.len() {
            return Err(Mp3Error::InvalidSettings(
                "the left and right blocks hold a different number of samples".into(),
            ));
        }
        if left.iter().chain(right).any(|sample| !sample.is_finite() || sample.abs() > 1.0) {
            return Err(Mp3Error::InvalidSettings("a sample is not finite or past full scale".into()));
        }
        let samples = left.len();
        if samples == 0 {
            return Ok(());
        }
        let too_long = || Mp3Error::InvalidSettings("the block is too long for one encode call".into());
        let nsamples = c_int::try_from(samples).map_err(|_| too_long())?;
        let size = worst_case_bytes(samples)?;
        let size_c = c_int::try_from(size).map_err(|_| too_long())?;
        let start = out.len();
        let end = start
            .checked_add(size)
            .ok_or_else(|| Mp3Error::InvalidSettings("the block is too long to size an output buffer for".into()))?;
        out.resize(end, 0);
        // SAFETY: the buffer was just grown to hold `end` bytes, so `ptr` points at `size`
        // writable ones.
        let buf = unsafe { out.as_mut_ptr().add(start) };
        // SAFETY: `self.gfp` is a live encoder with `lame_init_params` settled; the two
        // slices hold `samples` readable floats each; `buf` holds `size` writable bytes.
        let written =
            unsafe { lame_encode_buffer_ieee_float(self.gfp, left.as_ptr(), right.as_ptr(), nsamples, buf, size_c) };
        if written < 0 {
            return Err(Mp3Error::Encode(written));
        }
        out.truncate(start + written as usize);
        Ok(())
    }

    /// Flushes the encoder: the last frames are padded and appended to `out`, the encoder is
    /// dropped, and the LAME tag frame the file carries at its start is returned. Empty when
    /// the encoder wrote none.
    pub fn finish(self, out: &mut Vec<u8>) -> Result<Vec<u8>, Mp3Error> {
        // LAME documents 7200 bytes for the flush; the flush works in bunches of up to 1152
        // samples and can still emit a frame and the reservoir, so twice that is comfortable.
        let size = 7200 * 2;
        let start = out.len();
        let end = start + size;
        out.resize(end, 0);
        // SAFETY: the buffer was just grown to hold `end` bytes, so `ptr` points at `size`
        // writable ones.
        let buf = unsafe { out.as_mut_ptr().add(start) };
        // SAFETY: `self.gfp` is a live encoder with `lame_init_params` settled; `buf` holds
        // `size` writable bytes.
        let written = unsafe { lame_encode_flush(self.gfp, buf, size as c_int) };
        if written < 0 {
            return Err(Mp3Error::Flush(written));
        }
        out.truncate(start + written as usize);

        // The tag: ask for its size with a too-small buffer, then fetch it whole.
        // SAFETY: `self.gfp` is a live encoder that has been flushed; a null buffer asks for
        // the size only.
        let needed = unsafe { lame_get_lametag_frame(self.gfp, core::ptr::null_mut(), 0) };
        let tag = if needed == 0 {
            Vec::new()
        } else {
            if needed > 65_536 {
                return Err(Mp3Error::Assemble(format!("the LAME tag claims {needed} bytes; that is not a frame")));
            }
            let mut tag = vec![0u8; needed];
            // SAFETY: `tag` holds `needed` writable bytes, which the call fills.
            let copied = unsafe { lame_get_lametag_frame(self.gfp, tag.as_mut_ptr(), tag.len()) };
            if copied != needed {
                return Err(Mp3Error::Assemble(format!(
                    "the LAME tag returned {copied} bytes where {needed} were asked for"
                )));
            }
            tag
        };
        Ok(tag)
        // `self` drops here: the encoder's buffers are freed after the tag is copied out.
    }
}

impl Drop for Mp3Encoder {
    fn drop(&mut self) {
        if !self.gfp.is_null() {
            // SAFETY: the handle was made by `lame_init` and is not shared; `lame_close`
            // frees it exactly once. Its return code has nothing left to act on here.
            unsafe {
                lame_close(self.gfp);
            }
        }
    }
}

/// The ID3v2 header's declared size: ten bytes of header, then the sync-safe size, and ten
/// more when the optional footer flag says one follows. A stream that does not start with
/// one has none.
fn id3v2_size(stream: &[u8]) -> Result<usize, Mp3Error> {
    if stream.len() < 10 || !stream.starts_with(b"ID3") {
        return Ok(0);
    }
    let flags = stream[5];
    let mut size = 0usize;
    for byte in &stream[6..10] {
        let byte = usize::from(byte & 0x7F);
        size = (size << 7) | byte;
    }
    size += 10;
    if flags & 0x10 != 0 {
        size += 10;
    }
    if size > stream.len() {
        return Err(Mp3Error::Assemble(format!(
            "the ID3v2 header claims {size} bytes in a stream of {}",
            stream.len()
        )));
    }
    Ok(size)
}

/// The length of the first MPEG audio frame in `stream`, from the header's own tables
/// (the MPEG audio frame size formula, `version` × layer × bitrate × padding). Refuses
/// anything that is not a Layer III frame, which is all this encoder writes.
fn first_frame_len(stream: &[u8]) -> Result<usize, Mp3Error> {
    const BITRATES_MPEG1: [usize; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
    const BITRATES_MPEG2: [usize; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    let fail = || Mp3Error::Assemble("the stream does not start with an MPEG audio frame".into());
    if stream.len() < 4 {
        return Err(fail());
    }
    if stream[0] != 0xFF || stream[1] & 0xE0 != 0xE0 {
        return Err(fail());
    }
    let version = (stream[1] >> 3) & 0x03;
    if version == 1 {
        return Err(fail());
    }
    let layer = (stream[1] >> 1) & 0x03;
    if layer != 0x01 {
        return Err(fail());
    }
    let bitrate_index = usize::from(stream[2] >> 4);
    let rate_index = usize::from((stream[2] >> 2) & 0x03);
    let padding = usize::from((stream[2] >> 1) & 0x01);
    if bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return Err(fail());
    }
    let kbps = match version {
        3 => BITRATES_MPEG1[bitrate_index],
        _ => BITRATES_MPEG2[bitrate_index],
    };
    let rate = match version {
        3 => [44_100, 48_000, 32_000][rate_index],
        2 => [22_050, 24_000, 16_000][rate_index],
        _ => [11_025, 12_000, 8_000][rate_index],
    };
    // Layer III: 144 samples per frame at MPEG 1, 72 at MPEG 2 and 2.5.
    let coefficient = match version {
        3 => 144,
        _ => 72,
    };
    Ok(coefficient * kbps * 1000 / rate + padding)
}

/// Puts the finished MP3 file together: the stream as the encoder produced it, with the
/// empty frame LAME writes at its start replaced by the final LAME tag, which sits after
/// the ID3v2 header when there is one. A stream without a tag frame passes through whole.
pub fn assemble_mp3_file(stream: &[u8], lametag: &[u8]) -> Result<Vec<u8>, Mp3Error> {
    if lametag.is_empty() {
        return Ok(stream.to_vec());
    }
    let start = id3v2_size(stream)?;
    let rest =
        stream.get(start..).ok_or_else(|| Mp3Error::Assemble("the ID3v2 header does not fit the stream".into()))?;
    let frame_len = first_frame_len(rest)?;
    if frame_len != lametag.len() {
        return Err(Mp3Error::Assemble(format!(
            "the LAME tag is {} bytes and the stream's first frame is {}",
            lametag.len(),
            frame_len
        )));
    }
    let after = start
        .checked_add(frame_len)
        .ok_or_else(|| Mp3Error::Assemble("the first frame does not fit the stream".into()))?;
    if after > stream.len() {
        return Err(Mp3Error::Assemble("the first frame does not fit the stream".into()));
    }
    let mut file = Vec::with_capacity(stream.len());
    file.extend_from_slice(&stream[..start]);
    file.extend_from_slice(lametag);
    file.extend_from_slice(&stream[after..]);
    Ok(file)
}

#[cfg(test)]
#[path = "mp3_tests.rs"]
mod tests;
