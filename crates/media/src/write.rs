// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing the audio files an export renders: one entry point for WAV (32-bit float and
//! 16/24-bit PCM with dithering), FLAC, Ogg Vorbis and MP3, so the export everywhere — the
//! app, the CLI and the MCP server — and every caller of an offline render write through the
//! same code ([`docs/spec/export.md`](../../docs/spec/export.md) › Formats). The engine
//! decodes and never writes; this is the crate that writes.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use suhina_mp3::{Mp3Encoder, Mp3Rate, Mp3Settings, Mp3Tags, assemble_mp3_file};

/// The file formats an export writes, by the names the `project.export` command and the
/// dialog use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// WAV: 32-bit float or 16/24-bit PCM.
    Wav,
    /// FLAC: lossless, at 24 or 16 bits.
    Flac,
    /// Ogg Vorbis, no depth.
    Ogg,
    /// MP3, through the vendored LAME encoder, no depth.
    Mp3,
}

impl AudioFormat {
    /// The format a file name's extension says, when the extension is one of the export's.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "ogg" => Some(Self::Ogg),
            "mp3" => Some(Self::Mp3),
            _ => None,
        }
    }

    /// The name of the format, and the extension its file carries.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Mp3 => "mp3",
        }
    }

    /// Whether the format carries a bit depth at all: PCM ones do, Ogg Vorbis and MP3 do not.
    pub fn has_depth(self) -> bool {
        matches!(self, Self::Wav | Self::Flac)
    }
}

/// The depth a PCM file keeps its samples at; `32f` for WAV only, FLAC takes the two PCM
/// depths, Ogg takes none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitDepth {
    /// 32-bit float (WAV only).
    Float32,
    /// 24-bit PCM.
    Pcm24,
    /// 16-bit PCM.
    Pcm16,
}

impl BitDepth {
    /// The depth the command's name says: `32f`, `24` or `16`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "32f" => Some(Self::Float32),
            "24" => Some(Self::Pcm24),
            "16" => Some(Self::Pcm16),
            _ => None,
        }
    }

    /// The name the command takes.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Float32 => "32f",
            Self::Pcm24 => "24",
            Self::Pcm16 => "16",
        }
    }

    /// Whether the depth reduces a 32-bit float mix to PCM, so dithering applies.
    pub fn is_reduced(self) -> bool {
        matches!(self, Self::Pcm16 | Self::Pcm24)
    }
}

/// How a file is written: the format, the depth, the sample rate, whether dithering applies
/// to a reduced depth, the seed the dither's noise is made from — exports are deterministic,
/// so the same render writes the same bytes — and, for MP3, the rate and the ID3 title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioFileSpec {
    pub format: AudioFormat,
    pub depth: BitDepth,
    pub sample_rate: u32,
    pub dither: bool,
    pub seed: u64,
    /// The MP3 rate; other formats leave it at the default.
    pub mp3_rate: Mp3Rate,
    /// The ID3 title an MP3 file carries, normally the project's name; empty writes none.
    pub title: String,
}

impl AudioFileSpec {
    /// The spec for a 32-bit float WAV at `sample_rate`, what a render is shared as when no
    /// depth is asked for.
    pub fn float_wav(sample_rate: u32) -> Self {
        Self {
            format: AudioFormat::Wav,
            depth: BitDepth::Float32,
            sample_rate,
            dither: false,
            seed: 0,
            mp3_rate: Mp3Rate::default(),
            title: String::new(),
        }
    }
}

/// How the writer takes an existing destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// The destination is rewritten whatever it holds: a preview, a recording or a render the
    /// same caller made before. A symlink at the destination is replaced by a plain file, not
    /// written through.
    Replace,
    /// An export's rules: an existing destination is replaced only when `overwrite` is set,
    /// and a symlink is never written through — the same rules the export checks before the
    /// render, now enforced again when the bytes land.
    Export { overwrite: bool },
}

impl WriteMode {
    /// Whether a destination that is there already is replaced.
    fn replaces(self) -> bool {
        match self {
            Self::Replace => true,
            Self::Export { overwrite } => overwrite,
        }
    }

    /// Why the destination must not be written yet, for a clear error before a whole render
    /// runs; enforced again where the bytes are committed. A symlink is blocked for an export
    /// (never written through); for a replace it is the destination itself that the one rename
    /// swaps out — the link goes, the file behind it is not written.
    fn blocked(self, path: &Path) -> Option<String> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() && self != Self::Replace => {
                Some(format!("{} is a symlink; refusing to write through it", path.display()))
            }
            Ok(meta) if meta.is_dir() => Some(format!("{} is a folder", path.display())),
            Ok(_) if !self.replaces() => {
                Some(format!("{} already exists; pass overwrite: true to replace it", path.display()))
            }
            _ => None,
        }
    }
}

/// Writes interleaved stereo `samples` to `path` as `spec` says, under `mode`'s rules for a
/// destination that is there already. The bytes go to a file of their own beside the
/// destination first, and the finished one is put in place when they are done — so a
/// half-written encode never tears what was there, and the rules above hold in the moment the
/// file appears, not minutes before. The samples are the mix's own: never clipped here, and
/// `dither` only applies where the depth is reduced.
pub fn write_audio_file(path: &Path, samples: &[f32], spec: &AudioFileSpec, mode: WriteMode) -> Result<(), String> {
    if spec.sample_rate < 8000 || spec.sample_rate > 384_000 {
        return Err(format!("sample rate {} is not supported", spec.sample_rate));
    }
    if !samples.len().is_multiple_of(2) {
        return Err("the audio is not a whole number of stereo frames".into());
    }
    if let Some(why) = mode.blocked(path) {
        return Err(why);
    }
    // The staged file, beside the destination so the move is a rename on one filesystem.
    let stage = staged(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let encoded = match spec.format {
        AudioFormat::Wav => write_wav(&stage, samples, spec),
        AudioFormat::Flac => write_flac(&stage, samples, spec),
        AudioFormat::Ogg => write_ogg(&stage, samples, spec),
        AudioFormat::Mp3 => write_mp3(&stage, samples, spec),
    };
    if let Err(e) = encoded {
        // A failed encode leaves the destination alone: what was there still plays.
        let _ = std::fs::remove_file(&stage);
        return Err(e);
    }
    // The finished file is put in place in the one step the rules hold in.
    let landing = match mode {
        WriteMode::Replace | WriteMode::Export { overwrite: true } => rename_over(&stage, path),
        WriteMode::Export { overwrite: false } => link_in_place(&stage, path),
    };
    match landing {
        Ok(()) => {
            let _ = std::fs::remove_file(&stage);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&stage);
            Err(format!("{}: {e}", path.display()))
        }
    }
}

/// The file the bytes are written to first: a hidden name beside the destination, unique to
/// this process and try, so two writes to one destination never write the same stage.
fn staged(path: &Path) -> Result<std::path::PathBuf, std::io::Error> {
    let name = path
        .file_name()
        .map(|n| n.to_os_string())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "the destination has no file name"))?;
    static TRY: AtomicU64 = AtomicU64::new(0);
    for _ in 0..64 {
        let number = TRY.fetch_add(1, Ordering::Relaxed);
        let stage =
            path.with_file_name(format!(".{}.suhina-part-{}-{number}", name.to_string_lossy(), std::process::id()));
        match std::fs::File::create_new(&stage) {
            Ok(_) => return Ok(stage),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "no free name for the writing"))
}

/// Swaps the staged file in over whatever is at `path`: one rename, so the reader of `path`
/// sees either the old file or the new one, and a symlink at the destination is replaced
/// itself, not written through.
fn rename_over(stage: &Path, path: &Path) -> Result<(), std::io::Error> {
    std::fs::rename(stage, path)
}

/// Puts the staged file in place where nothing was: a hard link lands in one step and is
/// refused when something is already there, and a symlink at the name is refused as itself.
/// Where the filesystem has no hard links, a file created with `create_new` takes it the same
/// way, without the atomic landing.
fn link_in_place(stage: &Path, path: &Path) -> Result<(), std::io::Error> {
    match std::fs::hard_link(stage, path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        Err(_) => {
            // A copy that fails halfway leaves the name it made behind: it goes, too, and a
            // name nothing of ours was written to stays as it was found.
            let mut created = false;
            let landed = (|| -> std::io::Result<()> {
                let mut from = std::fs::File::open(stage)?;
                let mut to = std::fs::File::create_new(path)?;
                created = true;
                std::io::copy(&mut from, &mut to)?;
                use std::io::Write as _;
                to.flush()?;
                to.sync_all()?;
                Ok(())
            })();
            match landed {
                Ok(()) => {
                    let _ = std::fs::remove_file(stage);
                    Ok(())
                }
                Err(e) => {
                    if created {
                        let _ = std::fs::remove_file(path);
                    }
                    Err(e)
                }
            }
        }
    }
}

/// A deterministic pseudo-random stream for the dither's noise: the same seed makes the same
/// noise, so the same export is byte-identical every run (xorshift64*).
struct Dither {
    state: u64,
    /// The depth's bits, for the quantization's range.
    bits: u32,
    /// One least-significant bit of the target depth, in the float's scale.
    lsb: f64,
}

impl Dither {
    fn new(seed: u64, bits: u32) -> Self {
        Self { state: seed.max(1), bits, lsb: 1.0 / f64::from(1u32 << (bits - 1)) }
    }

    fn next_f64(&mut self) -> f64 {
        // xorshift64*: a full period, one multiply, no dependency.
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Triangular (TPDF) noise for one sample: two uniforms summed, peak to peak two LSB.
    fn noise(&mut self) -> f64 {
        (self.next_f64() + self.next_f64() - 1.0) * self.lsb
    }

    /// Quantizes one sample at the depth, with the dither's noise applied; the value is in the
    /// depth's own integer range (`-2^(bits-1) .. 2^(bits-1)-1`), which WAV and FLAC both
    /// take. Clamps a sample over full scale at full scale, and a NaN becomes 0 like every
    /// other hostile value.
    fn quantize(&mut self, sample: f32, dither: bool) -> i32 {
        let dithered = if dither { f64::from(sample) + self.noise() } else { f64::from(sample) };
        let half = f64::from(1u32 << (self.bits - 1));
        // `round` of a NaN is a NaN, and `clamp` keeps it; the `as` cast saturates, and a NaN
        // cast is 0.
        (dithered * half).round().clamp(-half, half - 1.0) as i32
    }
}

/// Writes a WAV through hound: `32f` as float samples, `24` and `16` as PCM, dithered when
/// asked. Hound takes the samples as full-range `i32` and shifts them to the depth itself.
fn write_wav(path: &Path, samples: &[f32], spec: &AudioFileSpec) -> Result<(), String> {
    let fail = |e: hound::Error| format!("{}: {e}", path.display());
    let sample_format = match spec.depth {
        BitDepth::Float32 => hound::SampleFormat::Float,
        BitDepth::Pcm24 | BitDepth::Pcm16 => hound::SampleFormat::Int,
    };
    let bits = match spec.depth {
        BitDepth::Float32 => 32,
        BitDepth::Pcm24 => 24,
        BitDepth::Pcm16 => 16,
    };
    let wav_spec = hound::WavSpec { channels: 2, sample_rate: spec.sample_rate, bits_per_sample: bits, sample_format };
    let mut writer = hound::WavWriter::create(path, wav_spec).map_err(fail)?;
    match spec.depth {
        BitDepth::Float32 => {
            for sample in samples {
                writer.write_sample(*sample).map_err(fail)?;
            }
        }
        BitDepth::Pcm24 | BitDepth::Pcm16 => {
            // Hound takes the samples in the depth's own range and errors past it.
            let dither = spec.dither && spec.depth.is_reduced();
            let bits = match spec.depth {
                BitDepth::Pcm16 => 16,
                _ => 24,
            };
            let mut noise = Dither::new(spec.seed, bits);
            for sample in samples {
                writer.write_sample(noise.quantize(*sample, dither)).map_err(fail)?;
            }
        }
    }
    writer.finalize().map_err(fail)?;
    Ok(())
}

/// Writes a FLAC through flacenc (pure Rust, single-threaded, so the file is deterministic) at
/// the PCM depth; a 32-bit float mix is quantized at the depth, dithered when asked. The
/// encoder takes the samples in the depth's own integer range. The frames are encoded one by
/// one with their explicit sizes, so the stream keeps `min_block_size == max_block_size` (a
/// fixed-blocksize stream per the FLAC specification) even when the last frame is short:
/// flacenc's own whole-stream encode lowers the stated minimum to the last frame's length,
/// which a strict decoder (symphonia) reads as a variable stream and refuses. The stream keeps
/// the mix's true total samples, so the file is the mix's length, not a padded one.
///
/// The mix is quantized and encoded one frame at a time, and the frame is put on the file as it
/// comes ([`FileSink`]), under a header of one STREAMINFO block the writer makes itself: the
/// mix's frame count and rate are known before the first frame, the stream's MD5 is hashed
/// while the frames are written and taken back over its placeholder with one seek. A long
/// export never holds a second whole-file copy of itself in integer samples, nor the encoded
/// frames in memory — flacenc's `Stream` component keeps every frame's bits until its write —
/// nor the whole encoded file: the render is the only whole-file buffer left, whichever format
/// is written.
///
/// The stereo side-channel codings (left-side, right-side, mid-side) are off: flacenc's
/// mid-side subframes run at one bit past the depth the frame states (25 bits for a 24-bit
/// file), and the frame min/max sizes the recombined stream counts say that impossible subframe
/// again — a strict decoder (symphonia) reads the file unchanged for a long mix and refuses it.
/// The independent assignment keeps, and costs some size where the channels correlate.
fn write_flac(path: &Path, samples: &[f32], spec: &AudioFileSpec) -> Result<(), String> {
    if spec.depth == BitDepth::Float32 {
        return Err("flac is written at 24 or 16 bits, not 32f".into());
    }
    let dither = spec.dither;
    let bits: u32 = match spec.depth {
        BitDepth::Pcm16 => 16,
        _ => 24,
    };
    let mut noise = Dither::new(spec.seed, bits);
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    use flacenc::source::Fill;
    let mut config = flacenc::config::Encoder::default();
    let mut stereo = flacenc::config::StereoCoding::default();
    stereo.use_leftside = false;
    stereo.use_rightside = false;
    stereo.use_midside = false;
    config.stereo_coding = stereo;
    // flacenc's partitioned-rice search runs to parameter 14 and never takes the format's
    // escape, so a residual it can only frame at parameter 14 comes out several times larger
    // than the verbatim subframe beside it would be; the encoder's own choice takes it
    // regardless. The encode runs once, and a frame that came out fatter than verbatim is
    // encoded again over subframes the coder has no choice in, which never happens.
    let mut universal = flacenc::config::Encoder::default();
    let mut universal_stereo = flacenc::config::StereoCoding::default();
    universal_stereo.use_leftside = false;
    universal_stereo.use_rightside = false;
    universal_stereo.use_midside = false;
    universal.stereo_coding = universal_stereo;
    let mut subframe = flacenc::config::SubFrameCoding::default();
    subframe.use_lpc = false;
    subframe.use_fixed = false;
    universal.subframe_coding = subframe;
    let config = config.into_verified().map_err(|(_, e)| format!("{}: {e}", path.display()))?;
    let universal = universal.into_verified().map_err(|(_, e)| format!("{}: {e}", path.display()))?;
    let block_size = config.block_size;
    // The frame's own facts: the file's rate and depth, the one block size every frame keeps.
    // The frame sizes are left unknown (0), what the fields' own unknown marker is: the sizes
    // are known only to the frames that already went by.
    let mut stream_info =
        flacenc::component::StreamInfo::new(usize::try_from(spec.sample_rate).unwrap_or(48_000), 2, bits as usize)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    stream_info.set_block_sizes(block_size, block_size).map_err(|e| format!("{}: {e}", path.display()))?;
    stream_info.set_frame_sizes(0, 0).map_err(|e| format!("{}: {e}", path.display()))?;
    stream_info.set_total_samples(samples.len() / 2);

    // One frame's worth of quantized samples, made and dropped frame by frame: a scratch the
    // size of a frame, not of the file.
    let mut chunk = Vec::with_capacity(block_size * 2);
    let mut context = flacenc::source::Context::new(bits as usize, 2);
    let mut framebuf =
        flacenc::source::FrameBuf::with_size(2, block_size).map_err(|e| format!("{}: {e}", path.display()))?;

    let mut write = || -> Result<(), String> {
        use std::io::Seek as _;
        use std::io::Write as _;
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut out = std::io::BufWriter::new(file);
        out.write_all(b"fLaC").map_err(|e| format!("{}: {e}", path.display()))?;
        // The one block: last, STREAMINFO (type 0), 34 bytes.
        out.write_all(&[0x80, 0, 0, 34]).map_err(|e| format!("{}: {e}", path.display()))?;
        {
            let mut sink = FileSink::new(&mut out);
            stream_info.write(&mut sink).map_err(|e| format!("{}: {e}", path.display()))?;
            for frame_number in 0..samples.len().div_ceil(block_size * 2) {
                chunk.clear();
                for sample in samples
                    [frame_number * block_size * 2..((frame_number + 1) * block_size * 2).min(samples.len())]
                    .iter()
                {
                    chunk.push(noise.quantize(*sample, dither));
                }
                framebuf.fill_interleaved(&chunk).map_err(|e| format!("{}: {e}", path.display()))?;
                context.fill_interleaved(&chunk).map_err(|e| format!("{}: {e}", path.display()))?;
                let frame = flacenc::encode_fixed_size_frame(&config, &framebuf, frame_number, &stream_info)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                // Verbatim rides the samples' own depth once plus their warm-up and the
                // subframe headers; a frame counted fatter than that is encoded again over
                // subframes the coder has no choice in, and no frame ever costs more than the
                // samples it holds.
                let frame = if frame.count_bits() > verbatim_bits(&framebuf, bits as usize) {
                    flacenc::encode_fixed_size_frame(&universal, &framebuf, frame_number, &stream_info)
                        .map_err(|e| format!("{}: {e}", path.display()))?
                } else {
                    frame
                };
                frame.write(&mut sink).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            sink.finish().map_err(|e| format!("{}: {e}", path.display()))?;
        }
        // The stream's MD5, hashed over the frames' samples as they went by, taken back over the
        // placeholder in the header (its offset in a one-STREAMINFO file is 4 + 4 + 18 = 26).
        out.seek(std::io::SeekFrom::Start(26))
            .and_then(|_| out.write_all(&context.md5_digest()))
            .and_then(|_| out.flush())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(())
    };
    write()
}

/// The bits a verbatim double for the samples in `framebuf` alone: per channel the subframe's
/// own `00 10` two-bit header, the warm-up samples at the full depth, and the samples at the
/// full depth, y-trailed by the frame's alignment and its CRC — a bit normally at the level the
/// file states, and a subframe's waste is famous against it.
fn verbatim_bits(framebuf: &flacenc::source::FrameBuf, bits: usize) -> usize {
    let channels = framebuf.channels();
    let samples = framebuf.filled_size();
    8 + (bits + 2) * samples * channels + 2 * (8 + 16) + 8
}

/// The stream's bits, written to a file as they come. A byte-exact mirror of
/// [`flacenc::bitsink::ByteSink`], whose whole file waits in one buffer in memory first: here
/// only the byte still under writing is held, so an hour-long export does not keep another
/// whole copy of itself in memory while its frames stream out.
struct FileSink<W: std::io::Write> {
    out: W,
    /// The stream's last byte, exactly as [`flacenc::bitsink::ByteSink`]'s own buffer keeps
    /// it: its top bits are the stream's, its bottom ones sit at zero for what comes next.
    /// Every other byte is already on the file.
    last: u8,
    /// Whether there is a last byte at all (a whole stream of one is possible). It leaves
    /// [`Self::last`] when the next byte is ready, or when the file is finished.
    live: bool,
    /// The stream's length in bits, what [`flacenc::bitsink::ByteSink`] counts in its buffer.
    bitlength: usize,
}

impl<W: std::io::Write> FileSink<W> {
    fn new(out: W) -> Self {
        Self { out, last: 0, live: false, bitlength: 0 }
    }

    /// Flushes the buffered writes, the byte still under writing included: its bottom bits
    /// sit at zero, where the next ones would have landed.
    fn finish(mut self) -> Result<(), std::io::Error> {
        if self.live {
            self.out.write_all(std::slice::from_ref(&self.last))?;
            self.live = false;
        }
        self.out.flush()
    }

    /// A new byte joins the stream: the one under writing leaves for the file.
    fn pushes(&mut self, byte: u8) -> std::io::Result<()> {
        if self.live {
            self.out.write_all(std::slice::from_ref(&self.last))?;
        }
        self.last = byte;
        self.live = true;
        Ok(())
    }
}

impl<W: std::io::Write> flacenc::bitsink::BitSink for FileSink<W> {
    type Error = std::io::Error;

    fn align_to_byte(&mut self) -> Result<usize, Self::Error> {
        let r = self.paddings();
        self.bitlength += r;
        Ok(r)
    }

    fn write_lsbs<T: flacenc::bitsink::Bits>(&mut self, val: T, n: usize) -> Result<(), Self::Error> {
        if n == 0 {
            return Ok(());
        }
        // The value's low `n` bits move to the top of its window, as the buffer variant's
        // does; the write then takes the top `n` of what is there.
        let width = std::mem::size_of::<T>() * 8;
        let mut window: u64 = val.into();
        window <<= 64 - width;
        let shifted = window << (width - n);
        self.window_msbs(shifted, n)
    }

    fn write_msbs<T: flacenc::bitsink::Bits>(&mut self, val: T, n: usize) -> Result<(), Self::Error> {
        if n == 0 {
            return Ok(());
        }
        let width = std::mem::size_of::<T>() * 8;
        let mut window: u64 = val.into();
        window <<= 64 - width;
        self.window_msbs(window, n)
    }

    fn write<T: flacenc::bitsink::Bits>(&mut self, val: T) -> Result<(), Self::Error> {
        let width = std::mem::size_of::<T>() * 8;
        let nbitlength = self.bitlength + width;
        let tail = self.paddings();
        if tail > 0 {
            self.write_msbs(val, tail)?;
        }
        // The window's bytes go whole after the partly written one, its bottom bits landing
        // as the zeros the padding leaves below what is written.
        let mut window: u64 = val.into();
        window <<= 64 - width;
        window <<= tail;
        let bytes = window.to_be_bytes();
        for byte in &bytes[..width / 8] {
            self.pushes(*byte)?;
        }
        self.bitlength = nbitlength;
        Ok(())
    }

    /// The mirror of [`flacenc::bitsink::ByteSink`]'s own: the bytes go whole after the last
    /// one is padded out, their length on top of the count it was given.
    fn write_bytes_aligned(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
        let ret = self.align_to_byte()?;
        for byte in bytes {
            self.pushes(*byte)?;
        }
        self.bitlength += 8 * bytes.len();
        Ok(ret)
    }
}

impl<W: std::io::Write> FileSink<W> {
    /// The bits left in the last byte, as the buffer variant computes them.
    const fn paddings(&self) -> usize {
        ((!self.bitlength).wrapping_add(1)) & 7
    }

    /// Writes `n` bits from the top of a 64-bit window, the mirror of
    /// [`flacenc::bitsink::ByteSink`]'s `write_msbs` in one width: the bits land MSB first, a
    /// partly written byte is filled from the top bits first, and whole bytes follow until
    /// what is left is fewer than eight, which goes under writing in the last byte.
    fn window_msbs(&mut self, mut window: u64, mut n: usize) -> Result<(), std::io::Error> {
        let r = self.paddings();
        self.bitlength += n;
        // The top `n` bits are the ones written; the widening left the rest zero already.
        if n < 64 {
            window &= u64::MAX << (64 - n);
        }
        if r != 0 {
            let b = (window >> (64 - r)) as u8;
            self.last |= b;
            window <<= r;
            if r >= n {
                return Ok(());
            }
            n -= r;
        }
        let bytes_to_write = n >> 3;
        if bytes_to_write > 0 {
            let bytes = window.to_be_bytes();
            for byte in &bytes[..bytes_to_write] {
                self.pushes(*byte)?;
            }
            n &= 7;
        }
        if n > 0 {
            window <<= bytes_to_write << 3;
            let tail_byte = (window >> 56) as u8;
            self.pushes(tail_byte)?;
        }
        Ok(())
    }
}

/// Writes an Ogg Vorbis file through vorbis_rs: libvorbis at quality 5, the whole mix as f32.
/// The stream's serial is the spec's seed, so the same export is byte-identical (Ogg embeds a
/// serial number; a random one would break reproducibility).
fn write_ogg(path: &Path, samples: &[f32], spec: &AudioFileSpec) -> Result<(), String> {
    use std::io::{BufWriter, Write as _};
    use std::num::{NonZeroU8, NonZeroU32};
    use vorbis_rs::{VorbisEncoderBuilder, VorbisError};
    let fail = |e: VorbisError| format!("{}: {e}", path.display());
    let rate = NonZeroU32::new(spec.sample_rate).ok_or("sample rate 0 is not supported")?;
    let channels = NonZeroU8::new(2).ok_or("stereo is the only export")?;
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = VorbisEncoderBuilder::new_with_serial(rate, channels, BufWriter::new(file), spec.seed as i32)
        .build()
        .map_err(fail)?;
    // The encoder takes one slice per channel; blocks of a second, so a long mix does not hold
    // a giant block in the encoder at once. The samples are interleaved, so a channel is every
    // other one.
    let block_frames = spec.sample_rate as usize;
    for block in samples.chunks(block_frames * 2) {
        let left: Vec<f32> = block.iter().step_by(2).copied().collect();
        let right: Vec<f32> = block.iter().skip(1).step_by(2).copied().collect();
        encoder.encode_audio_block(&[left, right]).map_err(fail)?;
    }
    // `finish` gives the sink back; flush it, so a failed write is an error and not a lost file.
    let mut sink = encoder.finish().map_err(fail)?;
    sink.flush().map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

/// Writes an MP3 through the vendored LAME 3.100 encoder (`suhina-mp3`): the mix is fed in
/// blocks, the stream is flushed, and the LAME tag frame replaces the empty frame LAME
/// leaves at the stream's start, so players get gapless playback and the true length. The
/// ID3v2 header carries the title when the spec has one.
fn write_mp3(path: &Path, samples: &[f32], spec: &AudioFileSpec) -> Result<(), String> {
    let fail = |e: suhina_mp3::Mp3Error| format!("{}: {e}", path.display());
    let mut encoder = Mp3Encoder::new(&Mp3Settings {
        rate: spec.mp3_rate,
        sample_rate: spec.sample_rate,
        channels: 2,
        tags: Mp3Tags { title: spec.title.clone() },
    })
    .map_err(fail)?;
    let mut stream: Vec<u8> = Vec::new();
    // The encoder takes one slice per channel; blocks of a second, so a long mix does not
    // hold a giant block at once. The samples are interleaved, so a channel is every other
    // one. The encoder takes only full scale: a sample over it clamps there and a NaN
    // becomes 0, as the PCM depths treat hostile values.
    let full_scale = |sample: &f32| if sample.is_nan() { 0.0 } else { sample.clamp(-1.0, 1.0) };
    let block_frames = spec.sample_rate as usize;
    for block in samples.chunks(block_frames * 2) {
        let left: Vec<f32> = block.iter().step_by(2).map(full_scale).collect();
        let right: Vec<f32> = block.iter().skip(1).step_by(2).map(full_scale).collect();
        encoder.encode(&left, &right, &mut stream).map_err(fail)?;
    }
    let tag = encoder.finish(&mut stream).map_err(fail)?;
    let file = assemble_mp3_file(&stream, &tag).map_err(fail)?;
    std::fs::write(path, &file).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
